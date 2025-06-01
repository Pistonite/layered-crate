use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use anyhow::{Context, bail};

use crate::layerfile::{DepGraph, LayerFile};
use crate::syntax::EntryFile;
use crate::util;

pub fn build_by_layers(
    args: &crate::Cli,
    package_dir: &Path,
    test_package_dir: &Path,
    layerfile: &LayerFile,
    dep_graph: &DepGraph,
    entryfile: &EntryFile,
) -> anyhow::Result<()> {
    let cargo = cargo_bin();

    // first run cargo once on the initial state
    message("building package...");
    let status = run_cargo_inherit(&cargo, &args.cargo_args, package_dir)
        .context("failed to call cargo to build full package")?;
    if !status.success() {
        error_message("initial build failed, please see errors above from cargo.");
        bail!("failed to build full package in (mostly) original form");
    }

    // find extra modules that will always be included
    let mut extra_modules = entryfile.all_modules();
    log::debug!("all modules: {:?}", extra_modules);
    // if a module is in the dep graph, then it's not "extra"
    for module in &dep_graph.top_down_order {
        extra_modules.remove(module);
    }
    // exclude modules declared in the exclude section
    for module in &layerfile.crate_.exclude {
        extra_modules.remove(module);
    }
    log::debug!("extra modules: {:?}", extra_modules);

    let test_package_entrypoint = test_package_dir.join("lib.rs");

    // now we check each layer
    message("checking layers...");
    for layer in &dep_graph.top_down_order {
        let all_test_modules = layerfile
            .get_test_modules(layer)
            .with_context(|| format!("failed to get test modules for layer `{layer}`"))?;

        let mut all_deps = BTreeSet::new();
        // collect all dependencies of the layer
        for m in &all_test_modules {
            if let Some(deps) = dep_graph.deps.get(m) {
                all_deps.extend(deps.iter().cloned());
            }
        }
        // deduplicate the deps from ones already in test module
        for m in &all_test_modules {
            all_deps.remove(m);
        }

        // build with all dependencies of the layer
        let test_file = entryfile
            .produce_test_lib(&all_test_modules, &all_deps)
            .with_context(|| format!("failed to produce test library for module '{layer}'"))?;
        std::fs::write(&test_package_entrypoint, test_file)
            .context("failed to write test library to file")?;
        util::format_if_possible(&test_package_entrypoint);
        let (status, stderr) = run_cargo_capture(&cargo, &args.cargo_args, test_package_dir)
            .with_context(|| format!("failed to run cargo for testing module: '{layer}'"))?;
        if !status.success() {
            println!("{}", stderr);
            error_message(format!("FAIL {layer}"));
            bail!(
                "layer `{layer}` failed to build - check if there are missing dependencies or unused dependencies"
            );
        }
        message(format!("PASS {layer}"));
    }

    Ok(())
}

fn run_cargo_inherit(cargo: &Path, args: &[String], curdir: &Path) -> anyhow::Result<ExitStatus> {
    log::debug!("running cargo with args: {:?}", args);
    let mut child = Command::new(cargo)
        .args(args)
        .current_dir(curdir)
        .spawn()
        .context("failed to spawn cargo process")?;
    let status = child.wait().context("failed to wait for cargo process")?;
    log::debug!("cargo process finished with status: {}", status);
    Ok(status)
}

fn run_cargo_capture(
    cargo: &Path,
    args: &[String],
    curdir: &Path,
) -> anyhow::Result<(ExitStatus, String)> {
    log::debug!("running cargo with args: {:?}", args);
    let output = Command::new(cargo)
        .args(args)
        .current_dir(curdir)
        .output()
        .context("failed to run cargo command")?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if output.status.success() {
        log::debug!("cargo command succeeded");
        log::trace!("cargo stdout: {}", stdout);
        log::trace!("cargo stderr: {}", stderr);
        Ok((output.status, stderr))
    } else {
        log::error!("cargo command failed with status: {}", output.status);
        log::trace!("cargo stdout: {}", stdout);
        Ok((output.status, stderr))
    }
}

fn cargo_bin() -> PathBuf {
    // https://doc.rust-lang.org/cargo/reference/environment-variables.html
    // (if we make this into a 3rd party subcommand)
    if let Ok(x) = std::env::var("CARGO_BIN") {
        if !x.is_empty() {
            log::debug!("using CARGO environment variable: {x}");
            return x.into();
        }
    }
    log::trace!("CARGO envvar not set or empty, using PATH to find cargo");
    if let Ok(x) = which::which("cargo") {
        log::debug!("found cargo in PATH: {}", x.display());
        return x;
    }
    log::warn!("can't find cargo, just using `cargo` command");
    "cargo".into()
}

pub fn message(message: impl std::fmt::Display) {
    println!("\x1b[1;32m[layered-crate]\x1b[0m {message}");
}
pub fn error_message(message: impl std::fmt::Display) {
    println!("\x1b[1;31m[layered-crate]\x1b[0m {message}");
}
