use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;

use cu::pre::*;
use regex::Regex;

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
) -> cu::Result<()> {
    // first run cargo once on the initial state
    run_cargo(None, &args.cargo_args, package_dir)?;

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
        cu::fs::write(&test_package_entrypoint, test_file)
            .context("failed to write test library to file")?;
        util::format_if_possible(&test_package_entrypoint);
        run_cargo(Some(layer), &args.cargo_args, test_package_dir)?;
    }

    Ok(())
}

fn run_cargo(layer: Option<&str>, args: &[String], curdir: &Path) -> cu::Result<()> {
    let mut has_warning = false;
    let result = {
        let bar = match layer {
            Some(layer) => cu::progress_unbounded_lowp(format!("compiling layer: {layer}")),
            None => cu::progress_unbounded("initial cargo build"),
        };
        let (child, _, output) = cu::which("cargo")?
            .command()
            .args(args)
            .current_dir(curdir)
            .stdio_null()
            .stderr(cu::pio::lines())
            .spawn()?;
        static STATUS_REGEX: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new("^((\x1b[^m]*m)|\\s)*(Compiling|Checking|Finished)((\x1b[^m]*m)|\\s)*")
                .unwrap()
        });
        static WARNING_REGEX: LazyLock<Regex> =
            LazyLock::new(|| Regex::new("^((\x1b[^m]*m)|\\s)*warning").unwrap());
        static ERROR_REGEX: LazyLock<Regex> =
            LazyLock::new(|| Regex::new("^((\x1b[^m]*m)|\\s)*error").unwrap());
        // prettify the output
        let mut last_error_line = String::new();
        for line in output {
            let Ok(line) = line else {
                continue;
            };
            if let Some(m) = STATUS_REGEX.find(&line) {
                let line = &line[m.end()..];
                cu::progress!(&bar, (), "{line}");
                continue;
            }
            if WARNING_REGEX.find(&line).is_some() {
                cu::warn!("{line}");
                has_warning = true;
                continue;
            }
            cu::print!("{line}");
            if layer.is_some() && line.contains("__layer_test") {
                print_guessed_hint_for_error(&line, &last_error_line);
            }
            if ERROR_REGEX.find(&line).is_some() {
                last_error_line = line;
            }
        }
        child.wait_nz()
    };
    match result {
        Ok(()) => {
            if has_warning {
                match layer {
                    Some(layer) => cu::warn!("PASS (with warning) {layer}"),
                    None => cu::warn!("initial build finished with warning(s)."),
                }
            } else {
                if let Some(layer) = layer {
                    cu::info!("PASS {layer}");
                }
            }
            Ok(())
        }
        Err(e) => {
            if let Some(layer) = layer {
                cu::error!("FAIL {layer}");
                cu::rethrow!(e, "layer {layer} failed to build (see cargo output above)");
            }
            cu::rethrow!(e, "crate failed to build (see cargo output above)");
        }
    }
}

/// print a best-guess hint (if any) for an error line that matches
fn print_guessed_hint_for_error(line: &str, last_error: &str) {
    if line.contains("unused import") {
        cu::hint!("(you might have specified an extraneous dependency on this layer)");
        return;
    }
    if last_error.contains("unused import") {
        // printed by the if above
        return;
    }
    cu::hint!("(you might be missing a dependency on this layer)");
}
