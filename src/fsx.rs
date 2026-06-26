//! Small filesystem helpers shared by the config-writing commands. `par` writes
//! into project directories that may be hostile (a teammate's repo, a checkout
//! of someone else's code), so every write that lands in a project refuses to
//! follow a pre-placed symlink — the hardening the router's installer applies to
//! every managed file it touches.

use std::fs;
use std::path::Path;

/// Error out if `path` already exists as a symlink, so a write can't be
/// redirected to an attacker-chosen target.
pub(crate) fn refuse_if_symlink(path: &Path) -> Result<(), String> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return Err(format!(
                "refusing to write through a symlink: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

/// Write `contents` to `path`, creating parent directories, after refusing to
/// follow a symlink at the destination.
pub(crate) fn write(path: &Path, contents: &str) -> Result<(), String> {
    refuse_if_symlink(path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    fs::write(path, contents).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Like [`write`], but `chmod 600` the file afterward on Unix — for files that
/// may carry credentials.
#[allow(dead_code)]
pub(crate) fn write_private(path: &Path, contents: &str) -> Result<(), String> {
    write(path, contents)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("chmod {}: {e}", path.display()))?;
    }
    Ok(())
}
