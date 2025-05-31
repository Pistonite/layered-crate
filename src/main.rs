#![doc = include_str!("../README.md")]
#![allow(clippy::needless_doctest_main)]

use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use anyhow::Context;
use clap::Parser;

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
    /// Args to pass to cargo. Default is `check --lib --color=always`
    cargo_args: Vec<String>,
}

fn main() -> ExitCode {
    colog::init();
    let start_time = Instant::now();
    let args = Cli::parse();
    if let Err(e) = main_internal(args) {
        println!("Error: {:?}", e);
        checker::error_message("check failed, please see errors above.");
        return ExitCode::FAILURE;
    }

    let elapsed = start_time.elapsed();
    checker::message(format!("done in {:.2?}", elapsed));

    ExitCode::SUCCESS
}

fn main_internal(mut args: Cli) -> anyhow::Result<()> {
    if args.cargo_args.is_empty() {
        args.cargo_args = vec![
            "check".to_string(),
            "--lib".to_string(),
            "--color=always".to_string(),
        ];
    }

    log::debug!("parsed arguments: {args:#?}");
    let manifest_path = Path::new("./Cargo.toml");
    let manifest_info =
        cargo_toml::prepare(manifest_path).context("Failed to prepare Cargo.toml")?;

    let layerfile =
        std::fs::read_to_string(&args.layerfile).context("failed to read Layerfile.toml")?;
    let layerfile =
        toml::from_str::<LayerFile>(&layerfile).context("failed to parse Layerfile.toml")?;
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

    let test_package_name = test_package_name(&manifest_info.package_name);
    let temp_dir = Path::new(&args.temp_dir);
    let package_dir = temp_dir.join(&manifest_info.package_name);
    let test_package_dir = temp_dir.join(&test_package_name);

    log::debug!("start layer testing");

    checker::build_by_layers(
        &args,
        &package_dir,
        &test_package_dir,
        &layerfile,
        &dep_graph,
        &entryfile,
    )
    .context("layer test failed")?;

    log::debug!("layer testing completed successfully");
    Ok(())
}

fn prepare_workspace(
    temp_dir: &str,
    manifest_info: &CargoManifestInfo,
    entryfile: &EntryFile,
) -> anyhow::Result<()> {
    log::debug!("preparing workspace");
    let path = Path::new(temp_dir);
    if !path.exists() {
        log::trace!("creating temporary directory: {temp_dir}");
        std::fs::create_dir_all(temp_dir).context("failed to create temporary directory")?;
    } else {
        log::trace!("temporary directory already exists: {temp_dir}");
    }

    log::debug!("ensuring package directory exists");
    let package_name = &manifest_info.package_name;
    let package_dir = path.join(package_name);
    if !package_dir.exists() {
        log::trace!("creating package directory: {}", package_dir.display());
        std::fs::create_dir_all(&package_dir).context("failed to create package directory")?;
    }

    log::debug!("ensuring test package directory exists");
    let test_package_name = test_package_name(&manifest_info.package_name);
    let test_package_dir = path.join(&test_package_name);
    if !test_package_dir.exists() {
        log::trace!(
            "creating test package directory: {}",
            test_package_dir.display()
        );
        std::fs::create_dir_all(&test_package_dir)
            .context("failed to create test package directory")?;
    }

    log::debug!("writing Cargo.toml to package directory");
    let cargo_toml_path = package_dir.join("Cargo.toml");
    std::fs::write(&cargo_toml_path, &manifest_info.content)
        .context("failed to write modified Cargo.toml to temporary package directory")?;

    log::debug!("preparing workspace Cargo.toml");
    let workspace_cargo_toml_path = path.join("Cargo.toml");
    let cargo_toml_string = if workspace_cargo_toml_path.exists() {
        log::trace!(
            "reading existing workspace Cargo.toml at {}",
            workspace_cargo_toml_path.display()
        );
        match std::fs::read_to_string(&workspace_cargo_toml_path) {
            Ok(content) => {
                log::trace!("read existing workspace Cargo.toml content");
                content
            }
            Err(e) => {
                log::warn!("failed to read existing workspace Cargo.toml: {e}, creating new one");
                "[workspace]".to_string()
            }
        }
    } else {
        log::trace!("no existing workspace Cargo.toml found, creating new one");
        "[workspace]".to_string()
    };
    let mut workspace_cargo_toml: toml::Table = match cargo_toml_string.parse() {
        Ok(table) => table,
        Err(e) => {
            log::error!("failed to parse existing workspace Cargo.toml: {e}");
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
    log::debug!("setting members of workspace: {:?}", members);
    workspace.insert(
        "members".to_string(),
        toml::Value::Array(members.into_iter().map(toml::Value::String).collect()),
    );

    let workspace_serialized = toml::to_string(&workspace_cargo_toml)
        .context("failed to serialize workspace Cargo.toml")?;
    log::trace!("serialized workspace Cargo.toml: {workspace_serialized}");
    std::fs::write(workspace_cargo_toml_path, workspace_serialized)
        .context("failed to write workspace Cargo.toml")?;

    let lib_entry_path = package_dir.join(&manifest_info.lib_entrypoint);
    if let Some(lib_parent) = lib_entry_path.parent() {
        if !lib_parent.exists() {
            log::trace!(
                "creating directory for lib entry point: {}",
                lib_parent.display()
            );
            std::fs::create_dir_all(lib_parent)
                .context("failed to create directory for lib entry point")?;
        }
    }
    log::debug!(
        "writing lib entry point file to: {}",
        lib_entry_path.display()
    );
    let lib_content = entryfile.produce_lib();
    std::fs::write(&lib_entry_path, lib_content).context("failed to write lib entry point file")?;
    util::format_if_possible(&lib_entry_path);

    log::debug!("preparing test package");

    let test_package_manifest =
        cargo_toml::make_test_package_manifest(manifest_info, &test_package_name)
            .context("failed to create test package manifest")?;

    let test_package_manifest_path = test_package_dir.join("Cargo.toml");
    log::debug!(
        "writing test package Cargo.toml to: {}",
        test_package_manifest_path.display()
    );
    std::fs::write(&test_package_manifest_path, test_package_manifest)
        .context("failed to write test package Cargo.toml")?;

    log::debug!("workspace prepared successfully");
    Ok(())
}

fn test_package_name(name: &str) -> String {
    format!("{name}-layer-test-{}", name.len())
}
