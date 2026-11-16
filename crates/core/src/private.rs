//! Owner-only files and folders (audit 2026-11-12, item 6).
//!
//! On macOS and Linux, folders are created `0700` and files `0600`, whatever the umask, and a
//! file is written to a `0600` temporary beside it and renamed into place, so no reader ever
//! sees a partial or world-readable copy. On Windows the same places get a protected access
//! list (no inheritance from the parent) that allows the current user only; folders pass it
//! on to everything inside them. A file is restricted right after it is created, before any
//! byte is written.

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
        // Only the folders this creates are restricted, as on Unix.
        let missing: Vec<&Path> = dir.ancestors().take_while(|p| !p.exists()).collect();
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        for p in missing.iter().rev() {
            restrict(p)?;
        }
        Ok(())
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
    let f = o
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    #[cfg(windows)]
    restrict(path)?;
    Ok(f)
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
    restrict(path)?;
    Ok(f)
}

/// Make an existing file or folder owner-only (`0600` / `0700`; on Windows a protected access
/// list for the current user, unless it already has exactly that).
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
    #[cfg(windows)]
    {
        let meta = std::fs::symlink_metadata(path)?;
        if meta.file_type().is_symlink() {
            return Ok(()); // never follow a link (or junction) to change something elsewhere
        }
        win::restrict(path, meta.is_dir())
            .with_context(|| format!("restricting {}", path.display()))?;
    }
    #[cfg(not(any(unix, windows)))]
    let _ = path;
    Ok(())
}

/// Tighten a whole tree (an existing project opened by this version). Symlinks are skipped.
/// On Windows the folder's new access list is passed on to everything inside it by the system.
pub fn restrict_tree(root: &Path) -> Result<()> {
    restrict(root)?;
    if cfg!(windows) {
        return Ok(());
    }
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

/// Owner-only access lists on Windows: one "allow" entry for the current user, protected from
/// the parent's entries. Folders mark the entry inheritable, so their contents follow.
#[cfg(windows)]
mod win {
    use anyhow::{Result, bail};
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE};
    use windows_sys::Win32::Foundation::{HLOCAL, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        EXPLICIT_ACCESS_W, GetNamedSecurityInfoW, NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT, SET_ACCESS,
        SetEntriesInAclW, SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
    };
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACL, DACL_SECURITY_INFORMATION, EqualSid, GetAce,
        GetSecurityDescriptorControl, GetTokenInformation, NO_INHERITANCE,
        PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SE_DACL_PROTECTED,
        SUB_CONTAINERS_AND_OBJECTS_INHERIT, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    /// Full control of a file or folder. Not `GENERIC_ALL`: an inheritable entry with generic
    /// rights is split by the system into two entries (one effective, one inherit-only).
    const FILE_ALL_ACCESS: u32 = 0x001F_01FF;

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    fn os_error(code: u32) -> anyhow::Error {
        std::io::Error::from_raw_os_error(code as i32).into()
    }

    /// The current user's `TOKEN_USER`, in a buffer aligned for it (its SID points inside).
    fn token_user() -> Result<Vec<u64>> {
        // SAFETY: plain Win32 calls with out-pointers to live locals; the buffer is sized by
        // the first call and aligned to 8 bytes, enough for TOKEN_USER.
        unsafe {
            let mut token: HANDLE = null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                bail!(std::io::Error::last_os_error());
            }
            let mut len = 0u32;
            GetTokenInformation(token, TokenUser, null_mut(), 0, &mut len);
            let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
            let ok = GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len);
            let err = std::io::Error::last_os_error();
            CloseHandle(token);
            if ok == 0 {
                bail!(err);
            }
            Ok(buf)
        }
    }

    fn sid_of(buf: &[u64]) -> PSID {
        // SAFETY: `buf` was filled by GetTokenInformation(TokenUser).
        unsafe { (*(buf.as_ptr() as *const TOKEN_USER)).User.Sid }
    }

    /// Whether `path` already has a protected list whose entries all allow `sid` (and nobody
    /// else).
    fn is_private(path: &Path, sid: PSID) -> bool {
        let name = wide(path);
        let mut dacl: *mut ACL = null_mut();
        let mut sd: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: out-pointers to live locals; `sd` is freed with LocalFree below, and `dacl`
        // points into it.
        unsafe {
            let code = GetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                &mut dacl,
                null_mut(),
                &mut sd,
            );
            if code != ERROR_SUCCESS {
                return false;
            }
            let mut control = 0u16;
            let mut revision = 0u32;
            let mut ok = GetSecurityDescriptorControl(sd, &mut control, &mut revision) != 0
                && control & SE_DACL_PROTECTED != 0
                && !dacl.is_null()
                && (*dacl).AceCount >= 1;
            if ok {
                for i in 0..u32::from((*dacl).AceCount) {
                    let mut ace: *mut core::ffi::c_void = null_mut();
                    ok = GetAce(dacl, i, &mut ace) != 0 && {
                        let ace = ace as *const ACCESS_ALLOWED_ACE;
                        // ACCESS_ALLOWED_ACE_TYPE is 0; the SID starts at `SidStart`.
                        (*ace).Header.AceType == 0
                            && EqualSid(&(*ace).SidStart as *const u32 as PSID, sid) != 0
                    };
                    if !ok {
                        break;
                    }
                }
            }
            LocalFree(sd as HLOCAL);
            ok
        }
    }

    pub(super) fn restrict(path: &Path, is_dir: bool) -> Result<()> {
        let user = token_user()?;
        let sid = sid_of(&user);
        if is_private(path, sid) {
            return Ok(());
        }
        let access = EXPLICIT_ACCESS_W {
            grfAccessPermissions: FILE_ALL_ACCESS,
            grfAccessMode: SET_ACCESS,
            grfInheritance: if is_dir {
                SUB_CONTAINERS_AND_OBJECTS_INHERIT
            } else {
                NO_INHERITANCE
            },
            Trustee: TRUSTEE_W {
                pMultipleTrustee: null_mut(),
                MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_USER,
                ptstrName: sid as *mut u16,
            },
        };
        let name = wide(path);
        // SAFETY: `access` and `name` outlive the calls; the new list is freed with LocalFree.
        unsafe {
            let mut acl: *mut ACL = null_mut();
            let code = SetEntriesInAclW(1, &access, null_mut(), &mut acl);
            if code != ERROR_SUCCESS {
                return Err(os_error(code));
            }
            let code = SetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                acl,
                null_mut(),
            );
            LocalFree(acl as HLOCAL);
            if code != ERROR_SUCCESS {
                return Err(os_error(code));
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn is_owner_only(path: &Path) -> bool {
        let user = token_user().unwrap();
        is_private(path, sid_of(&user))
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn files_and_folders_allow_only_the_current_user() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b");
        create_dir_all(&nested).unwrap();
        assert!(win::is_owner_only(&dir.path().join("a")));
        assert!(win::is_owner_only(&nested));
        let f = nested.join("x.csv");
        write(&f, b"secret").unwrap();
        assert!(win::is_owner_only(&f));
        assert_eq!(std::fs::read(&f).unwrap(), b"secret");
        // A file written into a folder that is not private is still owner-only.
        let loose = dir.path().join("loose.txt");
        replace(&loose, b"x").unwrap();
        assert!(win::is_owner_only(&loose));
        // An existing tree is tightened from its root; the files inside stay usable.
        let old = dir.path().join("old");
        std::fs::create_dir_all(old.join("inner")).unwrap();
        std::fs::write(old.join("inner").join("f.txt"), b"y").unwrap();
        restrict_tree(&old).unwrap();
        assert!(win::is_owner_only(&old));
        assert_eq!(
            std::fs::read(old.join("inner").join("f.txt")).unwrap(),
            b"y"
        );
        std::fs::write(old.join("inner").join("f.txt"), b"z").unwrap();
        std::fs::remove_dir_all(&old).unwrap();
    }
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
