use std::path::Path;
use std::process::Command;

use anyhow::bail;

pub fn resolve_path(path: impl AsRef<Path>, base_path: &Path) -> anyhow::Result<String> {
    let full_path = base_path.join(path);
    match dunce::canonicalize(&full_path) {
        Ok(path) => Ok(path.to_string_lossy().to_string()),
        Err(e) => {
            bail!("failed to resolve path '{}': {e}", full_path.display());
        }
    }
}

pub fn format_if_possible(path: &Path) {
    log::debug!("formatting file: {}", path.display());
    let Ok(rustfmt) = which::which("rustfmt") else {
        log::debug!("rustfmt not found, skipping formatting");
        return;
    };
    match Command::new(rustfmt).arg(path).output() {
        Ok(output) => {
            if !output.status.success() {
                log::warn!("rustfmt failed on {}", path.display(),);
            } else {
                log::debug!("formatted {} successfully", path.display());
            }
        }
        Err(_) => {
            log::warn!("failed to run rustfmt on {}", path.display());
        }
    }
}
