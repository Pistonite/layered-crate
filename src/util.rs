use std::path::Path;

use cu::pre::*;

/// Resolve the path from a base path. Returns the absolute path as a string.
/// Errors if the path is not UTF-8
pub fn resolve_path(path: impl AsRef<Path>, base_path: &Path) -> cu::Result<String> {
    base_path.join(path).normalize_exists()?.into_utf8()
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

pub fn run_rustfmt(input: String) -> String {
    match run_rustfmt_internal(&input) {
        Ok(x) => x,
        Err(e) => {
            cu::debug!("rustfmt failed: {e:?}");
            input
        }
    }
}
fn run_rustfmt_internal(input: &str) -> cu::Result<String> {
    let (child, output) = cu::which("rustfmt")?
        .command()
        .args(["--edition", "2024", "--emit", "stdout"])
        .stdin(cu::pio::write(input.as_bytes().to_vec()))
        .stdout(cu::pio::string())
        .stderr_null()
        .spawn()?;
    child.wait_nz()?;
    let output = output.join()??;
    Ok(output)
}
