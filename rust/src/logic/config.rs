//! Application data directory management.
//!
//! Mirrors `src/logic/config.ts`. The directory is created with mode `0o700`
//! and re-chmodded on every access, matching the TypeScript behaviour so
//! that the same `~/.nitro/` directory works for either binary.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Default application data directory: `~/.nitro/`.
///
/// Returns an error rather than panicking if no home directory can be found
/// — the CLI surfaces this to the user instead of crashing.
pub fn default_app_dir() -> io::Result<PathBuf> {
    dirs::home_dir().map(|h| h.join(".nitro")).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "could not determine home directory",
        )
    })
}

/// Create the data directory if missing and ensure it has mode `0o700`.
pub fn ensure_app_data_dir(data_dir: &Path) -> io::Result<()> {
    if !data_dir.exists() {
        fs::create_dir_all(data_dir)?;
    }
    set_dir_mode_700(data_dir)
}

#[cfg(unix)]
fn set_dir_mode_700(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_dir_mode_700(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Set a regular file to mode `0o600`. No-op on non-Unix platforms.
pub fn set_file_mode_600(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn ensure_creates_dir_with_mode_700() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("data");
        ensure_app_data_dir(&dir).unwrap();
        assert!(dir.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        }
    }

    #[test]
    fn ensure_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("data");
        ensure_app_data_dir(&dir).unwrap();
        ensure_app_data_dir(&dir).unwrap();
        assert!(dir.is_dir());
    }
}
