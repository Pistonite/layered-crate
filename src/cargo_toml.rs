use std::{collections::BTreeMap, path::Path};

use cu::pre::*;

use crate::util;

pub struct CargoManifestInfo {
    /// Name of the package
    pub package_name: String,
    /// Path to the entry point rs file (e.g. "src/lib.rs")
    pub lib_entrypoint: String,
    /// Content of the entry point rs file
    pub lib_entrypoint_content: String,
    /// Modified content of Cargo.toml
    pub content: String,

    // we need these properties below
    // to clone to the test package
    /// The [dependencies] section of the Cargo.toml
    pub resolved_dependencies: Option<toml::Table>,
    /// The [build-dependencies] section of the Cargo.toml
    pub resolved_build_dependencies: Option<toml::Table>,
    /// The [target] section of the Cargo.toml
    pub resolved_target: Option<toml::Table>,
    /// [features] section of the Cargo.toml,
    ///
    /// key is the feature name, value are the dep:* features
    pub dep_features: BTreeMap<String, Vec<String>>,
    pub default_features: Vec<String>,
}

pub fn manifest_has_workspace(manifest_path: &Path) -> bool {
    cu::debug!(
        "checking if Cargo.toml at '{}' has a workspace section",
        manifest_path.display()
    );
    let Ok(cargo_toml_string) = cu::fs::read_string(manifest_path) else {
        cu::debug!(
            "failed to read Cargo.toml at '{}', assuming no workspace section",
            manifest_path.display()
        );
        return false;
    };
    let Ok(cargo_toml) = toml::parse::<toml::Table>(&cargo_toml_string) else {
        cu::debug!("failed to parse Cargo.toml as TOML, assuming no workspace section");
        return false;
    };

    cargo_toml.get("workspace").is_some()
}

pub fn prepare(manifest_path: &Path) -> cu::Result<CargoManifestInfo> {
    cu::debug!("reading Cargo.toml at {}", manifest_path.display());
    let manifest_path_abs = manifest_path
        .normalize_exists()
        .context("failed to read Cargo.toml manifest")?;
    let manifest_dir_abs = manifest_path_abs.parent_abs()?;
    let manifest_dir_rel = manifest_path
        .parent()
        .context("failed to get parent directory of Cargo.toml")?;

    let mut cargo_toml = toml::read::<toml::Table>(cu::fs::reader(&manifest_path_abs)?)
        .context("failed to parse Cargo.toml")?;

    cu::trace!("parsed Cargo.toml: {cargo_toml:#?}");
    cu::debug!("reading package.name");
    let package_name = cargo_toml
        .get("package")
        .and_then(|pkg| pkg.get("name"))
        .and_then(|name| name.as_str())
        .map(String::from)
        .context("Failed to read package.name from Cargo.toml")?;
    cu::debug!("package name: {package_name}");

    cu::debug!("finding lib entrypoint");
    let lib_entrypoint = match cargo_toml.get("lib") {
        Some(lib) => {
            let lib_entrypoint = lib
                .get("path")
                .and_then(|p| p.as_str())
                .context("failed to read lib.path from Cargo.toml")?;
            lib_entrypoint.to_string()
        }
        None => {
            cu::debug!("no lib section found in Cargo.toml, assuming default src/lib.rs");
            "src/lib.rs".to_string()
        }
    };
    cu::debug!("lib entrypoint: {lib_entrypoint}");

    let actual_lib_path = manifest_dir_rel.join(&lib_entrypoint);
    let lib_entrypoint_content =
        cu::fs::read_string(&actual_lib_path).context("failed to read lib entrypoint")?;

    // don't allow absolute paths in the lib entrypoint, for now
    // this is because we are not changing the content in Cargo.toml,
    // just copying the entry point file from the original location
    // to the temporary directory
    if actual_lib_path.is_absolute() {
        cu::error!(
            "lib entry point path is absolute: {}",
            actual_lib_path.display()
        );
        cu::warn!("absolute lib entry point path is not supported right now.");
        cu::hint!(
            "this is because we need to generate a modified entry point at the same relative path as the original crate."
        );
        cu::hint!(
            "if the lib entry point path is absolute, the generated Cargo.toml needs to be modified as well."
        );
        cu::bailfyi!("lib entry point path is absolute");
    }

    cu::debug!("checking if we are in a workspace");
    let workspace_deps = if let Some(workspace) = cargo_toml.get_mut("workspace") {
        cu::debug!("found workspace section in Cargo.toml");
        resolve_paths_in_workspace(workspace, &manifest_dir_abs)
            .context("failed to resolve paths in workspace section")?;
        workspace
            .get("dependencies")
            .and_then(|deps| deps.as_table())
            .cloned()
    } else {
        cu::debug!("traversing up the directories to find workspace");
        // traverse up the directory tree to find a Cargo.toml with a [workspace] section
        let parent_parent = manifest_dir_abs.parent_abs().ok();
        let mut current_path = parent_parent.as_deref();
        let mut workspace_deps_out = None;
        while let Some(current) = current_path {
            cu::trace!("checking directory for workspace: {}", current.display());
            let workspace_manifest_path = current.join("Cargo.toml");
            if !workspace_manifest_path.exists() {
                cu::trace!("no Cargo.toml found in {}, skipping", current.display());
                current_path = current.parent();
                continue;
            }
            let mut workspace_toml = match cu::fs::read_string(&workspace_manifest_path)
                .and_then(|x| toml::parse::<toml::Table>(&x))
            {
                Ok(table) => table,
                Err(e) => {
                    cu::error!(
                        "failed to parse Cargo.toml at {}: {e}, will skip this one",
                        workspace_manifest_path.display()
                    );
                    current_path = current.parent();
                    continue;
                }
            };
            if let Some(workspace_table) = workspace_toml.get_mut("workspace") {
                cu::debug!(
                    "found workspace section in Cargo.toml at {}",
                    workspace_manifest_path.display()
                );
                resolve_paths_in_workspace(workspace_table, current)
                    .context("failed to resolve paths in workspace section")?;
                cu::debug!("getting workspace dependencies");
                workspace_deps_out = workspace_table
                    .get("dependencies")
                    .and_then(|deps| deps.as_table())
                    .cloned();
                break;
            } else {
                cu::trace!(
                    "no workspace section found in Cargo.toml at {}, continuing search",
                    workspace_manifest_path.display()
                );
                current_path = current.parent();
            }
        }
        workspace_deps_out
    };
    cu::debug!("workspace dependencies: {:#?}", workspace_deps);

    cu::debug!("resolving dependency paths in Cargo.toml");
    resolve_dependency_paths(&mut cargo_toml, &manifest_dir_abs, workspace_deps.as_ref())
        .context("failed to resolve dependency paths in Cargo.toml")?;

    match cargo_toml.get_mut("target") {
        Some(targets_table) => {
            resolve_dependency_paths_in_target(
                targets_table,
                &manifest_dir_abs,
                workspace_deps.as_ref(),
            )
            .context("failed to resolve dependency paths in 'target' section")?;
        }
        None => {
            cu::trace!("no 'target' section found in Cargo.toml, skipping path resolution");
        }
    }
    cu::debug!("finished resolving dependency paths in Cargo.toml");

    let resolved_dependencies = cargo_toml
        .get("dependencies")
        .and_then(|deps| deps.as_table())
        .cloned();
    let resolved_build_dependencies = cargo_toml
        .get("build-dependencies")
        .and_then(|deps| deps.as_table())
        .cloned();
    let resolved_target = cargo_toml
        .get("target")
        .and_then(|target| target.as_table())
        .cloned();

    cu::debug!("extracting features from Cargo.toml");
    let feature_table = cargo_toml.get("features").and_then(|f| f.as_table());
    let (dep_features, default_features) = match feature_table {
        Some(x) => {
            let mut dep_features = BTreeMap::new();
            for (fname, fvalue) in x {
                let mut dep_features_list = Vec::new();
                if let Some(deps) = fvalue.as_array() {
                    for dep in deps {
                        if let Some(dep_str) = dep.as_str() {
                            if dep_str.starts_with("dep:") {
                                cu::trace!(
                                    "found dependency feature: {} in feature '{}'",
                                    dep_str,
                                    fname
                                );
                                dep_features_list.push(dep_str.to_string());
                            }
                        }
                    }
                } else {
                    cu::warn!("feature '{}' is not an array, skipping dependencies", fname);
                }
                dep_features.insert(fname.clone(), dep_features_list);
            }
            let default_features: Vec<_> = x
                .get("default")
                .and_then(|f| f.as_array())
                .map(|f| {
                    f.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            (dep_features, default_features)
        }
        None => {
            cu::trace!("no features section found in Cargo.toml, using empty features");
            Default::default()
        }
    };
    cu::debug!("dep_features: {dep_features:?}, default features: {default_features:?}");

    let content =
        toml::stringify(&cargo_toml).context("failed to serialize modified Cargo.toml")?;
    cu::trace!("modified Cargo.toml content: {content}");

    Ok(CargoManifestInfo {
        package_name,
        lib_entrypoint,
        lib_entrypoint_content,
        content,
        resolved_dependencies,
        resolved_build_dependencies,
        resolved_target,
        dep_features,
        default_features,
    })
}

fn resolve_paths_in_workspace(
    workspace_table: &mut toml::Value,
    base_path: &Path,
) -> cu::Result<()> {
    let Some(workspace_table) = workspace_table.as_table_mut() else {
        cu::trace!("found 'workspace' section but not a table, skipping path resolution");
        return Ok(());
    };
    if let Some(members) = workspace_table.get_mut("members") {
        cu::trace!("found 'members' section in workspace, resolving paths");
        if let Some(members) = members.as_array_mut() {
            for m in members {
                let Some(path_str) = m.as_str() else {
                    cu::trace!("workspace member is not a string, skipping path resolution");
                    continue;
                };
                cu::trace!("resolving path for workspace member '{path_str}'");
                let resolved_path = cu::check!(
                    util::resolve_path(path_str, base_path),
                    error!("failed to resolve path for workspace member '{path_str}'")
                )?;
                cu::debug!("resolved path for workspace member '{path_str}': {resolved_path}");
                *m = resolved_path.into();
            }
        } else {
            cu::trace!("'members' section is not an array, skipping path resolution");
        }
    }
    match workspace_table.get_mut("dependencies") {
        Some(dependencies) => {
            cu::debug!("found 'dependencies' section in workspace, resolving paths");
            resolve_dependency_paths_in_table(dependencies, base_path, None)
                .context("failed to resolve dependency paths in 'dependencies' section")?;
        }
        None => {
            cu::trace!("no 'dependencies' section found in workspace, skipping path resolution")
        }
    }

    cu::trace!("finished resolving paths in 'workspace' section");
    Ok(())
}

fn resolve_dependency_paths_in_target(
    targets_table: &mut toml::Value,
    base_path: &Path,
    workspace_deps: Option<&toml::Table>,
) -> cu::Result<()> {
    let Some(targets_table) = targets_table.as_table_mut() else {
        cu::trace!("found 'target' section but not a table, skipping path resolution");
        return Ok(());
    };
    cu::debug!("resolving paths in 'target' section");
    for (target, value) in targets_table {
        let Some(table) = value.as_table_mut() else {
            cu::trace!("target '{target}' is not a table, skipping path resolution");
            continue;
        };
        cu::trace!("resolving paths for target: {target}");
        resolve_dependency_paths(table, base_path, workspace_deps)
            .context("failed to resolve dependency paths in target")?;
    }

    cu::trace!("finished resolving paths in 'target' section");
    Ok(())
}

fn resolve_dependency_paths(
    table: &mut toml::Table,
    base_path: &Path,
    workspace_deps: Option<&toml::Table>,
) -> cu::Result<()> {
    for key in ["dependencies", "dev-dependencies", "build-dependencies"] {
        let Some(dependencies) = table.get_mut(key) else {
            cu::trace!("no '{key}' section, skipping path resolution");
            continue;
        };
        cu::debug!("found '{key}' section, resolving paths");
        resolve_dependency_paths_in_table(dependencies, base_path, workspace_deps)
            .context("Failed to resolve dependency paths in '{dependencies}'")?;
    }
    cu::trace!("finished resolving dependency paths");
    Ok(())
}

fn resolve_dependency_paths_in_table(
    table: &mut toml::Value,
    base_path: &Path,
    workspace_deps: Option<&toml::Table>,
) -> cu::Result<()> {
    let Some(table) = table.as_table_mut() else {
        cu::trace!("not a table, skipping path resolution");
        return Ok(());
    };
    for (dep_name, dep_value) in table {
        let Some(dep_table) = dep_value.as_table_mut() else {
            cu::trace!("dependency '{dep_name}' is not a table, skipping path resolution");
            continue;
        };
        let is_workspace = dep_table
            .get("workspace")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if is_workspace {
            cu::debug!("dependency '{dep_name}' has workspace = true, resolving workspace path");
            match &workspace_deps {
                Some(workspace_deps) => {
                    resolve_dependency_workspace(dep_name, dep_value, workspace_deps);
                }
                None => {
                    cu::debug!(
                        "dependency '{dep_name}' has workspace = true but no workspace dependencies provided, skipping path resolution"
                    );
                }
            }
            continue;
        }
        if let Some(path_value) = dep_table.get_mut("path") {
            resolve_dependency_path(dep_name, path_value, base_path);
        }
    }
    cu::trace!("finished resolving dependency paths in dependency table");
    Ok(())
}

fn resolve_dependency_path(name: &str, value: &mut toml::Value, base_path: &Path) {
    let Some(path_str) = value.as_str() else {
        cu::trace!("dependency '{name}' 'path' is not a string, skipping path resolution");
        return;
    };
    cu::debug!("resolving path for dependency: {name}, path: {path_str}");
    match util::resolve_path(path_str, base_path) {
        Ok(resolved_path) => {
            cu::debug!("resolved path for dependency '{name}': {resolved_path}");
            *value = resolved_path.into();
        }
        Err(e) => {
            cu::error!("failed to resolve path for dependency '{name}': {e}, ignoring this path");
        }
    }
}

fn resolve_dependency_workspace(name: &str, value: &mut toml::Value, workspace_deps: &toml::Table) {
    cu::debug!("resolving workspace path for dependency: {name}");
    let Some(dep) = workspace_deps.get(name) else {
        cu::trace!(
            "dependency '{name}' not found in workspace dependencies, skipping path resolution"
        );
        return;
    };
    *value = dep.clone();
}

pub fn make_test_package_manifest(
    manifest_info: &CargoManifestInfo,
    test_package_name: &str,
) -> cu::Result<String> {
    cu::debug!("preparing test package manifest");
    let package_name = &manifest_info.package_name;

    let mut test_package_manifest = toml! {
        [package]
        name = ""
        version = "0.0.0"
        edition = "2024"
        [lib]
        path = "lib.rs"
        [features]
        default = []
    };
    test_package_manifest["package"]["name"] = toml::Value::String(test_package_name.to_string());

    // add the dependencies from the main package to the test package
    if let Some(deps) = &manifest_info.resolved_dependencies {
        test_package_manifest.insert("dependencies".to_string(), toml::Value::Table(deps.clone()));
    }
    if let Some(deps) = &manifest_info.resolved_build_dependencies {
        test_package_manifest.insert(
            "build-dependencies".to_string(),
            toml::Value::Table(deps.clone()),
        );
    }
    if let Some(target) = &manifest_info.resolved_target {
        test_package_manifest.insert("target".to_string(), toml::Value::Table(target.clone()));
    }
    let test_package_deps = test_package_manifest
        .entry("dependencies")
        .or_insert(toml::Value::Table(toml::Table::new()));
    // add the main package as a dependency to the test package
    let mut main_package_dep = {
        let package_name = package_name.to_string();
        toml! {
            path = ""
            package = package_name
            default-features = false
        }
    };
    main_package_dep["path"] = toml::Value::String(format!("../{package_name}"));
    test_package_deps.as_table_mut().unwrap().insert(
        "__layer_test".to_string(),
        toml::Value::Table(main_package_dep),
    );

    test_package_manifest["features"]["default"] = toml::Value::Array(
        manifest_info
            .default_features
            .iter()
            .map(|f| toml::Value::String(f.clone()))
            .collect(),
    );
    for (fname, fvalue) in &manifest_info.dep_features {
        if fname == "default" {
            // already added above
            continue;
        }
        let mut feature_value = vec![toml::Value::String(format!("__layer_test/{}", fname))];
        feature_value.extend(fvalue.iter().map(|f| toml::Value::String(f.clone())));
        test_package_manifest["features"]
            .as_table_mut()
            .unwrap()
            .insert(fname.clone(), toml::Value::Array(feature_value));
    }

    let test_package_manifest = toml::stringify(&test_package_manifest)
        .context("failed to serialize test package Cargo.toml")?;

    Ok(test_package_manifest)
}
