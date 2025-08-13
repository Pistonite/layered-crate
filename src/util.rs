use std::path::Path;

use cu::pre::*;

/// Resolve the path from a base path. Returns the absolute path as a string.
/// Errors if the path is not UTF-8
pub fn resolve_path(path: impl AsRef<Path>, base_path: &Path) -> cu::Result<String> {
    base_path.join(path).normalize_exists()?.into_utf8()
}

/// Format the file with rustfmt if possible
pub fn format_if_possible(path: &Path) {
    let Some(rustfmt) = cu::bin::get("rustfmt") else {
        return;
    };
    cu::debug!("formatting '{}'", path.display());
    match rustfmt.command().arg(path).all_null().wait() {
        Ok(code) if code.success() => {
            cu::debug!("formatted '{}' successfully", path.display());
        }
        Ok(_) => {
            cu::warn!("rustfmt failed for '{}'", path.display(),);
        }
        Err(e) => {
            cu::warn!("failed to run rustfmt on '{}': {e}", path.display());
        }
    }
}

/// The generated package name for building the crate by layers
pub fn test_package_name(name: &str) -> String {
    format!("{name}-layer-test-{}", name.len())
}

pub fn add_rustflag_if_missing(flag: &str, rust_flags: &mut String) {
    // currently we only do basic check
    // so -D unused-imports won't get detected, for example
    if !rust_flags.contains(flag) {
        if !rust_flags.is_empty() {
            rust_flags.push(' ');
        }
        rust_flags.push_str(flag)
    }
}
