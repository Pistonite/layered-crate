use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use cu::pre::*;
use itertools::Itertools;

use crate::layerfile::{DepGraph, LayerFile};
use crate::syntax::EntryFile;

pub fn build_by_layers(
    args: &crate::Cli,
    manifest_path: &Path,
    package_dir: &Path,
    test_package_dir: &Path,
    layerfile: &LayerFile,
    dep_graph: &DepGraph,
    entryfile: &EntryFile,
) -> cu::Result<()> {
    let manifest_path = manifest_path.normalize()?;
    let manifest_dir = manifest_path.parent_abs()?;
    // first run cargo once on the initial state
    let all_deps_str = dep_graph.top_down_order.join(",");
    run_cargo(
        None,
        &args.cargo_args,
        package_dir,
        &manifest_path,
        &manifest_dir,
        &all_deps_str,
    )?;

    // find extra modules that will always be included
    let mut extra_modules = entryfile.all_modules();
    cu::debug!("all modules: {:?}", extra_modules);
    // if a module is in the dep graph, then it's not "extra"
    for module in &dep_graph.top_down_order {
        extra_modules.remove(module);
    }
    // exclude modules declared in the exclude section
    for module in &layerfile.crate_.exclude {
        extra_modules.remove(module);
    }
    cu::debug!("extra modules: {:?}", extra_modules);

    let test_package_entrypoint = test_package_dir.join("lib.rs");

    // now we check each layer
    for layer in &dep_graph.top_down_order {
        let all_test_modules = layerfile
            .get_test_modules(layer)
            .with_context(|| format!("failed to get test modules for layer '{layer}'"))?;

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
        cu::fs::write(&test_package_entrypoint, test_file)
            .context("failed to write test library to file")?;
        let deps_str = all_deps.iter().join(",");
        run_cargo(
            Some(layer),
            &args.cargo_args,
            test_package_dir,
            &manifest_path,
            &manifest_dir,
            &deps_str,
        )?;
    }

    Ok(())
}

fn run_cargo(
    layer: Option<&str>,
    args: &[String],
    curdir: &Path,
    manifest_path: &Path,
    manifest_dir: &Path,
    deps_layers_str: &str,
) -> cu::Result<()> {
    let has_warning = Arc::new(cu::Atomic::<bool, bool>::new_bool(false));
    let command = cu::which("cargo")?
        .command()
        .args(args)
        .current_dir(curdir)
        .env("LAYERED_CRATE_ORIGINAL_MANIFEST_PATH", manifest_path)
        .env("LAYERED_CRATE_ORIGINAL_MANIFEST_DIR", manifest_dir)
        .env("LAYERED_CRATE_DEPS_LAYERS", deps_layers_str)
        .env("LAYERED_CRATE_TESTING_LAYER", layer.unwrap_or_default());
    let print_diag = {
        let has_warning = Arc::clone(&has_warning);
        move |is_warning: bool, message: &str| {
            has_warning.set(true);
            if is_warning {
                cu::warn!("{message}");
                return;
            }
            cu::error!("{message}");
            print_guessed_hint_for_error(message);
        }
    };
    let command = command.preset(cu::pio::cargo().on_diagnostic(print_diag));
    let command = match layer {
        Some(layer) => command.name(format!("building layer '{layer}'")),
        None => command.name("build full crate"),
    };
    let (child, bar, _) = command.spawn()?;
    match child.wait_nz() {
        Ok(()) => {
            match layer {
                Some(layer) => {
                    if let Some(bar) = bar {
                        cu::progress_done!(&bar, "PASS {layer}");
                    }
                    if has_warning.get() {
                        cu::warn!("layer '{layer}' passed with warning(s).");
                    }
                }
                None => {
                    if has_warning.get() {
                        cu::warn!("initial build finished with warning(s).");
                    }
                }
            }
            Ok(())
        }
        Err(e) => {
            drop(bar);
            if let Some(layer) = layer {
                cu::error!("FAIL {layer}");
                cu::disable_trace_hint();
                cu::rethrow!(
                    e,
                    "layer '{layer}' failed to build (see cargo output above)"
                );
            }
            cu::disable_trace_hint();
            cu::rethrow!(e, "crate failed to build (see cargo output above)");
        }
    }
}

/// print a best-guess hint (if any) for an error line that matches
fn print_guessed_hint_for_error(error: &str) {
    if error.contains("unused import") {
        cu::hint!("(you might have specified an extraneous dependency on this layer)");
        return;
    }
    if error.contains("unresolved import") {
        cu::hint!("(you might be missing a dependency on this layer)");
    }
}
