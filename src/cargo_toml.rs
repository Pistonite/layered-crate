use std::collections::BTreeSet;
use std::path::Path;

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
    /// The [dev-dependencies] section of the Cargo.toml
    pub resolved_dev_dependencies: Option<toml::Table>,
    /// The [target] section of the Cargo.toml
    pub resolved_target: Option<toml::Table>,
    /// [features] section of the Cargo.toml
    pub features: toml::Table,
    /// Names of optional dependencies that have an implicit feature
    /// (i.e. not referenced with `dep:` syntax in any feature)
    pub implicit_dep_features: Vec<String>,
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
    let manifest_path_abs = cu::check!(
        manifest_path.normalize_exists(),
        "failed to read Cargo.toml manifest"
    )?;
    let manifest_dir_abs = manifest_path_abs.parent_abs()?;
    let manifest_dir_rel = cu::check!(
        manifest_path.parent(),
        "failed to get parent directory of Cargo.toml"
    )?;

    let mut cargo_toml = cu::check!(
        toml::read::<toml::Table>(cu::fs::reader(&manifest_path_abs)?),
        "failed to parse Cargo.toml"
    )?;

    cu::trace!("parsed Cargo.toml: {cargo_toml:#?}");
    cu::debug!("reading package.name");
    let package_name = cu::check!(
        cargo_toml
            .get("package")
            .and_then(|pkg| pkg.get("name"))
            .and_then(|name| name.as_str())
            .map(String::from),
        "failed to read package.name from Cargo.toml"
    )?;
    cu::debug!("package name: {package_name}");

    cu::debug!("finding lib entrypoint");
    let lib_entrypoint = match cargo_toml.get("lib") {
        Some(lib) => match lib.get("path") {
            None => {
                cu::debug!("no lib.path found in Cargo.toml, assuming default src/lib.rs");
                "src/lib.rs".to_string()
            }
            Some(path) => {
                let lib_entrypoint = cu::check!(
                    path.as_str(),
                    "failed to read lib.path from Cargo.toml (expecting a string)"
                )?;
                lib_entrypoint.to_string()
            }
        },
        None => {
            cu::debug!("no lib section found in Cargo.toml, assuming default src/lib.rs");
            "src/lib.rs".to_string()
        }
    };
    cu::debug!("lib entrypoint: {lib_entrypoint}");

    let actual_lib_path = manifest_dir_rel.join(&lib_entrypoint);
    let lib_entrypoint_content = cu::check!(
        cu::fs::read_string(&actual_lib_path),
        "failed to read lib entrypoint"
    )?;

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
        cu::bail!("lib entry point path is absolute");
    }

    cu::debug!("checking if we are in a workspace");
    let workspace_info = if let Some(workspace) = cargo_toml.get_mut("workspace") {
        cu::debug!("found workspace section in Cargo.toml");
        cu::check!(
            resolve_paths_in_workspace(workspace, &manifest_dir_abs),
            "failed to resolve paths in workspace section"
        )?;
        Some(WorkspaceInfo::from_workspace_table(workspace))
    } else {
        cu::debug!("traversing up the directories to find workspace");
        // traverse up the directory tree to find a Cargo.toml with a [workspace] section
        let parent_parent = manifest_dir_abs.parent_abs().ok();
        let mut current_path = parent_parent.as_deref();
        let mut workspace_info_out = None;
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
                cu::check!(
                    resolve_paths_in_workspace(workspace_table, current),
                    "failed to resolve paths in workspace section"
                )?;
                workspace_info_out = Some(WorkspaceInfo::from_workspace_table(workspace_table));
                break;
            } else {
                cu::trace!(
                    "no workspace section found in Cargo.toml at {}, continuing search",
                    workspace_manifest_path.display()
                );
                current_path = current.parent();
            }
        }
        workspace_info_out
    };
    let workspace_info = workspace_info.unwrap_or_default();
    let workspace_deps = workspace_info.dependencies;
    cu::debug!("workspace dependencies: {:#?}", workspace_deps);

    cu::debug!("resolving inherited package fields in Cargo.toml");
    cu::check!(
        resolve_package_inheritance(&mut cargo_toml, workspace_info.package.as_ref()),
        "failed to resolve inherited [package] fields in Cargo.toml"
    )?;
    stub_package_files(&mut cargo_toml);
    cu::check!(
        resolve_lints_inheritance(&mut cargo_toml, workspace_info.lints.as_ref()),
        "failed to resolve inherited [lints] in Cargo.toml"
    )?;

    cu::debug!("resolving dependency paths in Cargo.toml");
    cu::check!(
        resolve_dependency_paths(&mut cargo_toml, &manifest_dir_abs, workspace_deps.as_ref()),
        "failed to resolve dependency paths in Cargo.toml"
    )?;

    match cargo_toml.get_mut("target") {
        Some(targets_table) => {
            cu::check!(
                resolve_dependency_paths_in_target(
                    targets_table,
                    &manifest_dir_abs,
                    workspace_deps.as_ref(),
                ),
                "failed to resolve dependency paths in 'target' section"
            )?;
        }
        None => {
            cu::trace!("no 'target' section found in Cargo.toml, skipping path resolution");
        }
    }
    cu::debug!("finished resolving dependency paths in Cargo.toml");

    // a dev-dependency on the crate itself (e.g. for doc tests) must point to the
    // generated copy, otherwise there are 2 different packages with the same name
    let manifest_dir_str = manifest_dir_abs.as_utf8()?;
    for_each_dev_dependency_table(&mut cargo_toml, |deps| {
        replace_dependency_path(deps, manifest_dir_str, ".")
    });

    let resolved_dependencies = cargo_toml
        .get("dependencies")
        .and_then(|deps| deps.as_table())
        .cloned();
    let resolved_build_dependencies = cargo_toml
        .get("build-dependencies")
        .and_then(|deps| deps.as_table())
        .cloned();
    let resolved_dev_dependencies = cargo_toml
        .get("dev-dependencies")
        .and_then(|deps| deps.as_table())
        .cloned();
    let resolved_target = cargo_toml
        .get("target")
        .and_then(|target| target.as_table())
        .cloned();

    cu::debug!("extracting features from Cargo.toml");
    let features = cargo_toml
        .get("features")
        .and_then(|f| f.as_table())
        .cloned()
        .unwrap_or_default();
    let implicit_dep_features = find_implicit_dep_features(&cargo_toml, &features);
    cu::debug!("features: {features:?}, implicit dep features: {implicit_dep_features:?}");

    let content = cu::check!(
        toml::stringify(&cargo_toml),
        "failed to serialize modified Cargo.toml"
    )?;
    cu::trace!("modified Cargo.toml content: {content}");

    Ok(CargoManifestInfo {
        package_name,
        lib_entrypoint,
        lib_entrypoint_content,
        content,
        resolved_dependencies,
        resolved_build_dependencies,
        resolved_dev_dependencies,
        resolved_target,
        features,
        implicit_dep_features,
    })
}

/// Find optional dependencies that get an implicit feature of the same name.
///
/// Cargo creates the implicit feature only if `dep:<name>` is not used in any feature.
fn find_implicit_dep_features(cargo_toml: &toml::Table, features: &toml::Table) -> Vec<String> {
    let mut dep_syntax_used = BTreeSet::new();
    for fvalue in features.values() {
        for entry in fvalue.as_array().into_iter().flatten() {
            if let Some(dep) = entry.as_str().and_then(|x| x.strip_prefix("dep:")) {
                dep_syntax_used.insert(dep.to_string());
            }
        }
    }
    let mut dep_tables = vec![];
    for key in ["dependencies", "build-dependencies"] {
        dep_tables.extend(cargo_toml.get(key).and_then(|x| x.as_table()));
    }
    if let Some(targets) = cargo_toml.get("target").and_then(|x| x.as_table()) {
        for target in targets.values() {
            for key in ["dependencies", "build-dependencies"] {
                dep_tables.extend(target.get(key).and_then(|x| x.as_table()));
            }
        }
    }
    let mut out = BTreeSet::new();
    for table in dep_tables {
        for (name, dep) in table {
            let is_optional = dep
                .get("optional")
                .and_then(|x| x.as_bool())
                .unwrap_or_default();
            if is_optional && !dep_syntax_used.contains(name) && !features.contains_key(name) {
                out.insert(name.clone());
            }
        }
    }
    out.into_iter().collect()
}

/// Call `f` on `[dev-dependencies]` and every `[target.*.dev-dependencies]` table
fn for_each_dev_dependency_table(
    cargo_toml: &mut toml::Table,
    mut f: impl FnMut(&mut toml::Table),
) {
    if let Some(deps) = cargo_toml
        .get_mut("dev-dependencies")
        .and_then(|x| x.as_table_mut())
    {
        f(deps);
    }
    if let Some(targets) = cargo_toml.get_mut("target").and_then(|x| x.as_table_mut()) {
        for (_, target) in targets.iter_mut() {
            if let Some(deps) = target
                .get_mut("dev-dependencies")
                .and_then(|x| x.as_table_mut())
            {
                f(deps);
            }
        }
    }
}

/// Replace `path = <from>` with `path = <to>` for all dependencies in the table
fn replace_dependency_path(deps: &mut toml::Table, from: &str, to: &str) {
    for (name, dep) in deps.iter_mut() {
        let Some(path) = dep.get_mut("path") else {
            continue;
        };
        if path.as_str() == Some(from) {
            cu::debug!("replacing path of dependency '{name}': '{from}' -> '{to}'");
            *path = to.into();
        }
    }
}

/// Info extracted from the `[workspace]` section that member packages can inherit from
#[derive(Default)]
struct WorkspaceInfo {
    /// `[workspace.dependencies]`
    dependencies: Option<toml::Table>,
    /// `[workspace.package]`
    package: Option<toml::Table>,
    /// `[workspace.lints]`
    lints: Option<toml::Table>,
}

impl WorkspaceInfo {
    fn from_workspace_table(workspace: &toml::Value) -> Self {
        let get = |key: &str| workspace.get(key).and_then(|x| x.as_table()).cloned();
        Self {
            dependencies: get("dependencies"),
            package: get("package"),
            lints: get("lints"),
        }
    }
}

fn is_workspace_inherited(value: &toml::Value) -> bool {
    value
        .as_table()
        .and_then(|t| t.get("workspace"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Replace `key.workspace = true` in [package] with the value from `[workspace.package]`
fn resolve_package_inheritance(
    cargo_toml: &mut toml::Table,
    workspace_package: Option<&toml::Table>,
) -> cu::Result<()> {
    let Some(package) = cargo_toml.get_mut("package").and_then(|p| p.as_table_mut()) else {
        cu::trace!("no [package] table, skipping inheritance resolution");
        return Ok(());
    };
    for (key, value) in package.iter_mut() {
        if !is_workspace_inherited(value) {
            continue;
        }
        if STUBBED_PACKAGE_KEYS.contains(&key.as_str()) {
            // will be replaced with stub
            continue;
        }
        let inherited = cu::check!(
            workspace_package.and_then(|p| p.get(key.as_str())),
            "package.{key} is inherited from workspace, but workspace.package.{key} was not found"
        )?;
        cu::debug!("inheriting package.{key} from workspace: {inherited}");
        *value = inherited.clone();
    }
    Ok(())
}

/// Package keys that reference files (or are only needed for publishing),
/// which are replaced with stubs in the generated manifests
const STUBBED_PACKAGE_KEYS: &[&str] = &["readme", "license", "license-file"];

/// Replace readme/license fields in [package] with stubs, since the generated
/// package is not at the original location and is never published
fn stub_package_files(cargo_toml: &mut toml::Table) {
    let Some(package) = cargo_toml.get_mut("package").and_then(|p| p.as_table_mut()) else {
        return;
    };
    for key in STUBBED_PACKAGE_KEYS {
        if package.remove(*key).is_some() {
            cu::debug!("removed package.{key} from generated manifest");
        }
    }
    package.insert("readme".to_string(), false.into());
}

/// Replace `[lints] workspace = true` with `[workspace.lints]`
fn resolve_lints_inheritance(
    cargo_toml: &mut toml::Table,
    workspace_lints: Option<&toml::Table>,
) -> cu::Result<()> {
    let Some(lints) = cargo_toml.get_mut("lints") else {
        return Ok(());
    };
    if !is_workspace_inherited(lints) {
        return Ok(());
    }
    let workspace_lints = cu::check!(
        workspace_lints,
        "lints is inherited from workspace, but workspace.lints was not found"
    )?;
    cu::debug!("inheriting lints from workspace");
    *lints = toml::Value::Table(workspace_lints.clone());
    Ok(())
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
                    "failed to resolve path for workspace member '{path_str}'"
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
            cu::check!(
                resolve_dependency_paths_in_table(dependencies, base_path, None),
                "failed to resolve dependency paths in 'dependencies' section"
            )?;
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
        cu::check!(
            resolve_dependency_paths(table, base_path, workspace_deps),
            "failed to resolve dependency paths in target"
        )?;
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
        cu::check!(
            resolve_dependency_paths_in_table(dependencies, base_path, workspace_deps),
            "failed to resolve dependency paths in '{dependencies}'"
        )?;
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
                    resolve_dependency_workspace(dep_name, dep_table, workspace_deps);
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

fn resolve_dependency_workspace(name: &str, value: &mut toml::Table, workspace_deps: &toml::Table) {
    cu::debug!("resolving workspace path for dependency: {name}");
    let Some(dep) = workspace_deps.get(name) else {
        cu::trace!(
            "dependency '{name}' not found in workspace dependencies, skipping path resolution"
        );
        return;
    };
    let is_optional = value
        .get("optional")
        .and_then(|x| x.as_bool())
        .unwrap_or_default();
    let additional_features = match value.get_mut("features").and_then(|x| x.as_array_mut()) {
        None => vec![],
        Some(x) => std::mem::take(x),
    };
    *value = match dep.as_table() {
        None => {
            // version number
            let mut map = toml::Table::new();
            map.insert("version".to_string(), dep.clone());
            map
        }
        Some(map) => map.clone(),
    };
    value.insert("optional".to_string(), is_optional.into());
    match value.get_mut("features").and_then(|x| x.as_array_mut()) {
        Some(features) => {
            for f in additional_features {
                if features.contains(&f) {
                    continue;
                }
                features.push(f);
            }
        }
        None => {
            if !additional_features.is_empty() {
                value.insert("features".to_string(), additional_features.into());
            }
        }
    };
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
    if let Some(deps) = &manifest_info.resolved_dev_dependencies {
        test_package_manifest.insert(
            "dev-dependencies".to_string(),
            toml::Value::Table(deps.clone()),
        );
    }
    if let Some(target) = &manifest_info.resolved_target {
        test_package_manifest.insert("target".to_string(), toml::Value::Table(target.clone()));
    }
    // dev-dependency on the crate itself is removed. It can't point to the main package
    // since that's already a dependency (__layer_test), and cargo doesn't allow
    // depending on the same package with different names. This is fine since
    // it's only useful for doc tests, examples, etc, which are not built in the test package
    for_each_dev_dependency_table(&mut test_package_manifest, |deps| {
        deps.retain(|name, dep| {
            let is_self = dep.get("path").and_then(|x| x.as_str()) == Some(".");
            if is_self {
                cu::debug!("removing self dev-dependency '{name}' from test package");
            }
            !is_self
        });
    });
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

    // The test package compiles (copies of) the layer modules itself, so it needs
    // the same feature graph as the original crate. Each feature keeps all of its
    // original entries (other features, dep:x, x/y, x?/y), and additionally
    // forwards itself to the main package.
    let test_features = test_package_manifest["features"].as_table_mut().unwrap();
    for (fname, fvalue) in &manifest_info.features {
        let mut feature_value = match fvalue.as_array() {
            Some(x) => x.clone(),
            None => {
                cu::warn!("feature '{fname}' is not an array, ignoring its content");
                vec![]
            }
        };
        feature_value.push(format!("__layer_test/{fname}").into());
        test_features.insert(fname.clone(), feature_value.into());
    }
    // Implicit features of optional deps also need to be forwarded,
    // which requires declaring them explicitly
    for name in &manifest_info.implicit_dep_features {
        let feature_value = vec![
            toml::Value::from(format!("dep:{name}")),
            format!("__layer_test/{name}").into(),
        ];
        test_features.insert(name.clone(), feature_value.into());
    }

    let test_package_manifest = cu::check!(
        toml::stringify(&test_package_manifest),
        "failed to serialize test package Cargo.toml"
    )?;

    Ok(test_package_manifest)
}
