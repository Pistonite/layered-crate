use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LayerFile {
    #[serde(rename = "crate")]
    pub crate_: LayerFileCrateSection,
    #[serde(default)]
    pub layer: BTreeMap<String, Layer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LayerFileCrateSection {
    /// Modules here will never be present when test building
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Layer {
    /// Module(s) that this layer depends on
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Module(s) that this layer implements from,
    /// which must be checked together
    #[serde(default, rename = "impl")]
    pub impl_: Vec<String>,
}

pub struct DepGraph<'a> {
    pub deps: BTreeMap<String, &'a [String]>,
    /// The top-down order of modules based on dependencies
    ///
    /// (i.e. the first in the list depends on modules that come after it,
    /// the last in the list depends on nothing)
    pub top_down_order: Vec<String>,
}

impl<'a> DepGraph<'a> {
    pub fn build(layers: &'a BTreeMap<String, Layer>) -> anyhow::Result<Self> {
        log::debug!("building dependency graph from layers");

        let mut deps = BTreeMap::new();
        let mut temp_deps_for_building = BTreeMap::new();
        for (name, layer) in layers {
            log::trace!("layer: {name} -> {:?}", layer.depends_on);
            deps.insert(name.clone(), &layer.depends_on[..]);
            temp_deps_for_building.insert(name.clone(), layer.depends_on.clone());
        }

        check_circular_dependencies(&deps).context("circular dependency detected")?;
        log::debug!("dependency graph built successfully");

        log::debug!("building topological order from dependencies");
        let mut seen = BTreeSet::new();
        let mut bottom_up_order = Vec::new();
        while !temp_deps_for_building.is_empty() {
            for (name, mut deps) in std::mem::take(&mut temp_deps_for_building) {
                deps.retain(|dep| !seen.contains(dep));
                log::trace!(
                    "processing module `{name}`, remaining dependencies: {:?}",
                    deps
                );
                if deps.is_empty() {
                    log::trace!("adding module `{name}`");
                    seen.insert(name.clone());
                    bottom_up_order.push(name);
                    continue;
                }
                temp_deps_for_building.insert(name, deps);
            }
        }
        log::debug!("bottom-up order: {:?}", bottom_up_order);

        Ok(Self {
            deps,
            top_down_order: bottom_up_order.into_iter().rev().collect(),
        })
    }
}

fn check_circular_dependencies(deps: &BTreeMap<String, &[String]>) -> anyhow::Result<()> {
    let mut checked = BTreeSet::new();
    for name in deps.keys() {
        log::trace!("checking circular dependencies for module `{name}`");
        let mut stack = vec![name.as_str()];
        check_circular_dependencies_recur(deps, name, &mut stack, &mut checked)?;
    }
    log::debug!("no circular dependencies found");
    Ok(())
}

fn check_circular_dependencies_recur<'a>(
    deps: &BTreeMap<String, &'a [String]>,
    curr: &str,
    stack: &mut Vec<&'a str>,
    checked: &mut BTreeSet<String>,
) -> anyhow::Result<()> {
    if !checked.insert(curr.to_string()) {
        // Already checked this module, no need to check again
        return Ok(());
    }
    let Some(edges) = deps.get(curr) else {
        bail!(
            "module `{curr}` not found in dependency graph, stack: {}. (You need to declare [layer.{curr}] even if it has no dependencies",
            format_stack_with_no_next(stack)
        );
    };

    for edge in *edges {
        if stack.iter().any(|&s| s == edge) {
            let graph = format_stack(stack, edge);
            bail!("circular dependency detected: {graph}");
        }
        stack.push(edge);
        check_circular_dependencies_recur(deps, edge, stack, checked)?;
        if stack.pop().is_none() {
            bail!("underflowed dep stack, this is a bug");
        }
    }

    Ok(())
}

fn format_stack(stack: &[&str], next: &str) -> String {
    format!("{} -> {}", stack.join(" -> "), next)
}

fn format_stack_with_no_next(stack: &[&str]) -> String {
    stack.join(" -> ")
}
