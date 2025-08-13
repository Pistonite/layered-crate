use clap::Parser;
use cu::pre::*;

use std::path::Path;

mod cargo_toml;
mod checker;
mod layerfile;
mod syntax;
mod util;

use cargo_toml::CargoManifestInfo;
use layerfile::{DepGraph, LayerFile};
use syntax::EntryFile;

/// Enforce internal dependencies in a Rust crate
///
/// See <https://github.com/Pistonite/layered-crate>
#[derive(Parser, Debug, Clone)]
#[clap(version)]
struct Cli {
    /// Temporary directory to put the test package for building by layers
    #[clap(short = 'T', long, default_value = "./target/layered-crate")]
    temp_dir: String,
    /// Path to the Layerfile.toml
    #[clap(short = 'L', long, default_value = "./Layerfile.toml")]
    layerfile: String,

    /// Do not edit the RUSTFLAGS environment variable.
    ///
    /// By default, recommended deny flags such as `-Dunused-imports` are added
    /// if missing.
    #[clap(long)]
    no_rust_flags: bool,

    #[clap(flatten)]
    common: cu::cli::Flags,
    /// Args to pass to cargo, including the command. Default is `check --lib`
    /// and the color flag
    #[clap(trailing_var_arg(true))]
    cargo_args: Vec<String>,
}

#[cu::cli(flags = "common")]
fn main(mut args: Cli) -> cu::Result<()> {
    if args.cargo_args.is_empty() {
        args.cargo_args = vec![
            "check".to_string(),
            "--lib".to_string(),
            cu::color_flag_eq().to_string(),
        ];
    } else {
        let mut found_color_flag = false;
        for arg in &args.cargo_args {
            if arg.starts_with("--color") {
                found_color_flag = true;
            }
        }
        if !found_color_flag {
            args.cargo_args.push(cu::color_flag_eq().to_string());
        }
    }

    if !args.no_rust_flags {
        let mut rust_flags = std::env::var("RUSTFLAGS").unwrap_or_default();
        util::add_rustflag_if_missing("-Dunused-imports", &mut rust_flags);
        // safety: no other threads exist at this point
        unsafe { std::env::set_var("RUSTFLAGS", rust_flags) };
    }

    let _ = cu::which("rustfmt");
    cu::bin::find(
        "cargo",
        [
            // https://doc.rust-lang.org/cargo/reference/environment-variables.html
            // (if we make this into a 3rd party subcommand)
            cu::bin::from_env("CARGO_BIN"),
            cu::bin::in_PATH(),
        ],
    )
    .context("cannot find cargo!")?;

    cu::debug!("parsed arguments: {args:#?}");
    let manifest_path = Path::new("./Cargo.toml");
    let manifest_info =
        cargo_toml::prepare(manifest_path).context("Failed to prepare Cargo.toml")?;

    let layerfile = toml::read::<LayerFile>(cu::fs::reader(&args.layerfile)?)?;

    let dep_graph = DepGraph::build(&layerfile.layer)
        .context("failed to build dependency graph from Layerfile")?;

    let entryfile_path = manifest_path
        .parent()
        .map(|p| p.join(&manifest_info.lib_entrypoint))
        .context("failed to determine entry file path")?;
    let entryfile_base_path = entryfile_path
        .parent()
        .context("failed to determine base path for entry file")?;
    let entryfile = EntryFile::resolve(&manifest_info.lib_entrypoint_content, entryfile_base_path)
        .context("Failed to resolve modules in library entry file")?;

    prepare_workspace(&args.temp_dir, &manifest_info, &entryfile)
        .context("failed to prepare temporary workspace")?;

    let test_package_name = util::test_package_name(&manifest_info.package_name);
    let temp_dir = Path::new(&args.temp_dir);
    let package_dir = temp_dir.join(&manifest_info.package_name);
    let test_package_dir = temp_dir.join(&test_package_name);

    cu::debug!("start layer testing");

    checker::build_by_layers(
        &args,
        &package_dir,
        &test_package_dir,
        &layerfile,
        &dep_graph,
        &entryfile,
    )
    .context("layer test failed")?;

    cu::debug!("layer testing completed successfully");
    Ok(())
}

fn prepare_workspace(
    temp_dir: &str,
    manifest_info: &CargoManifestInfo,
    entryfile: &EntryFile,
) -> cu::Result<()> {
    cu::debug!("preparing workspace");
    let path = Path::new(temp_dir);

    let package_name = &manifest_info.package_name;
    let package_dir = path.join(package_name);
    cu::fs::make_dir(&package_dir).context("failed to create temporary package directory")?;

    cu::debug!("ensuring test package directory exists");
    let test_package_name = util::test_package_name(&manifest_info.package_name);
    let test_package_dir = path.join(&test_package_name);
    cu::fs::make_dir(&test_package_dir).context("failed to create test package directory")?;

    cu::debug!("writing Cargo.toml to package directory");
    let cargo_toml_path = package_dir.join("Cargo.toml");
    cu::fs::write(&cargo_toml_path, &manifest_info.content)
        .context("failed to write modified Cargo.toml to temporary package directory")?;

    cu::debug!("preparing workspace Cargo.toml");
    let workspace_cargo_toml_path = path.join("Cargo.toml");
    let cargo_toml_string = if workspace_cargo_toml_path.exists() {
        cu::trace!(
            "reading existing workspace Cargo.toml at {}",
            workspace_cargo_toml_path.display()
        );
        match cu::fs::read_string(&workspace_cargo_toml_path) {
            Ok(content) => {
                cu::trace!("read existing workspace Cargo.toml content");
                content
            }
            Err(e) => {
                cu::warn!("failed to read existing workspace Cargo.toml: {e}, creating new one");
                "[workspace]".to_string()
            }
        }
    } else {
        cu::trace!("no existing workspace Cargo.toml found, creating new one");
        "[workspace]".to_string()
    };
    let mut workspace_cargo_toml = match toml::parse::<toml::Table>(&cargo_toml_string) {
        Ok(table) => table,
        Err(e) => {
            cu::error!("failed to parse existing workspace Cargo.toml: {e}");
            Default::default()
        }
    };
    let workspace = workspace_cargo_toml
        .entry("workspace")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let workspace = match workspace.as_table_mut() {
        Some(table) => table,
        None => {
            *workspace = toml::Value::Table(toml::Table::new());
            workspace
                .as_table_mut()
                .expect("Failed to create workspace table")
        }
    };
    workspace
        .entry("resolver")
        .or_insert(toml::Value::String("2".to_string()));

    let readdir = std::fs::read_dir(temp_dir).context("failed to read temporary directory")?;
    let mut members = vec![];
    for entry in readdir {
        let entry = entry.context("failed to read directory entry")?;
        let entry_path = entry.path();
        if entry_path.is_dir() && entry.file_name() != "target" {
            let manifest_path = entry_path.join("Cargo.toml");
            if !cargo_toml::manifest_has_workspace(&manifest_path) {
                members.push(entry.file_name().to_string_lossy().to_string());
            }
        }
    }
    cu::debug!("setting members of workspace: {:?}", members);
    workspace.insert(
        "members".to_string(),
        toml::Value::Array(members.into_iter().map(toml::Value::String).collect()),
    );

    let workspace_serialized = toml::stringify(&workspace_cargo_toml)
        .context("failed to serialize workspace Cargo.toml")?;
    cu::trace!("serialized workspace Cargo.toml: {workspace_serialized}");
    cu::fs::write(workspace_cargo_toml_path, workspace_serialized)
        .context("failed to write workspace Cargo.toml")?;

    let lib_entry_path = package_dir.join(&manifest_info.lib_entrypoint);
    if let Some(lib_parent) = lib_entry_path.parent() {
        cu::fs::make_dir(lib_parent).context("failed to create directory for lib entry point")?;
    }
    cu::debug!(
        "writing lib entry point file to: {}",
        lib_entry_path.display()
    );
    let lib_content = entryfile.produce_lib();
    cu::fs::write(&lib_entry_path, lib_content).context("failed to write lib entry point file")?;
    util::format_if_possible(&lib_entry_path);

    cu::debug!("preparing test package");

    let test_package_manifest =
        cargo_toml::make_test_package_manifest(manifest_info, &test_package_name)
            .context("failed to create test package manifest")?;

    let test_package_manifest_path = test_package_dir.join("Cargo.toml");
    cu::debug!(
        "writing test package Cargo.toml to: {}",
        test_package_manifest_path.display()
    );
    cu::fs::write(&test_package_manifest_path, test_package_manifest)
        .context("failed to write test package Cargo.toml")?;

    cu::debug!("workspace prepared successfully");
    Ok(())
}
