//! Owner-only files and folders (audit 2026-11-12, item 6).
//!
//! On macOS and Linux, folders are created `0700` and files `0600`, whatever the umask, and a
//! file is written to a `0600` temporary beside it and renamed into place, so no reader ever
//! sees a partial or world-readable copy. On Windows, files inherit the ACL of their folder;
//! under the user's profile that is already the user (plus SYSTEM and Administrators). An
//! explicit ACL is not set yet: a project placed in a shared folder on Windows keeps that
//! folder's permissions (recorded as a platform limitation in PROGRESS.md).

use anyhow::{Context, Result};
use std::io::Write;
use std::path::Path;

/// Create `dir` and any missing parents; the folders this creates are owner-only.
pub fn create_dir_all(dir: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .with_context(|| format!("creating {}", dir.display()))?;
        // `mode` applies only to folders it creates; existing folders keep their permissions
        // (a shared parent is never tightened behind the person's back).
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))
    }
}

/// Write `bytes` to `path` atomically, owner-only, creating missing folders (owner-only). A
/// failed write leaves no temporary behind.
pub fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    create_dir_all(path.parent().context("a file needs a folder")?)?;
    replace(path, bytes)
}

/// [`write`] into a folder that must already exist (a project's own files: a missing project
/// folder is an error, never silently recreated).
pub fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().context("a file needs a folder")?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    // A unique temporary per write, created exclusively: concurrent writers never share one.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.{}.{seq}.part", std::process::id()));
    let result = (|| -> Result<()> {
        let mut f = create_private_new(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.with_context(|| format!("writing {}", path.display()))
}

/// Create `path` for writing, owner-only, failing if it already exists (no link is followed).
fn create_private_new(path: &Path) -> Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path)
        .with_context(|| format!("creating {}", path.display()))
}

/// Create (or truncate) `path` for writing, owner-only from the first byte.
pub fn open_private(path: &Path) -> Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let f = o
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    #[cfg(unix)]
    restrict(path)?;
    Ok(f)
}

/// Make an existing file or folder owner-only (`0600` / `0700`). No-op off Unix.
pub fn restrict(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::symlink_metadata(path)?;
        if meta.file_type().is_symlink() {
            return Ok(()); // never follow a link to change something elsewhere
        }
        let mode = if meta.is_dir() { 0o700 } else { 0o600 };
        if meta.permissions().mode() & 0o777 != mode {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
                .with_context(|| format!("restricting {}", path.display()))?;
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Tighten a whole tree (an existing project opened by this version). Symlinks are skipped.
pub fn restrict_tree(root: &Path) -> Result<()> {
    restrict(root)?;
    if std::fs::symlink_metadata(root)?.is_dir() {
        for entry in std::fs::read_dir(root)? {
            let p = entry?.path();
            if std::fs::symlink_metadata(&p)?.is_dir() {
                restrict_tree(&p)?;
            } else {
                restrict(&p)?;
            }
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mode(p: &Path) -> u32 {
        std::fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn files_and_folders_are_owner_only_whatever_the_umask() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a/b");
        create_dir_all(&nested).unwrap();
        assert_eq!(mode(&dir.path().join("a")), 0o700);
        assert_eq!(mode(&nested), 0o700);
        let f = nested.join("x.csv");
        write(&f, b"secret").unwrap();
        assert_eq!(mode(&f), 0o600);
        assert_eq!(std::fs::read(&f).unwrap(), b"secret");
        // No temporary left behind.
        let left: Vec<_> = std::fs::read_dir(&nested)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, [std::ffi::OsString::from("x.csv")]);
        // An existing loose tree is tightened.
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::set_permissions(&nested, std::fs::Permissions::from_mode(0o755)).unwrap();
        restrict_tree(dir.path()).unwrap();
        assert_eq!(mode(&f), 0o600);
        assert_eq!(mode(&nested), 0o700);
    }
}
