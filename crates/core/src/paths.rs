//! Where inVectus keeps per-machine state: `<OS data dir>/inVectus` (consent choices, agent
//! file grants, the agent socket). One place, so every part agrees.

use std::path::PathBuf;
use std::sync::OnceLock;

static OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

/// `<OS data dir>/inVectus`, or `INVECTUS_DATA_DIR` when set, or the test folder set with
/// [`use_data_dir_for_tests`]. Whoever controls this process's environment can point it at
/// other grants and consent; that is the same person who can run the process at all.
pub fn app_data_dir() -> Option<PathBuf> {
    if let Some(p) = OVERRIDE.get() {
        return Some(p.clone());
    }
    // Set by the person (or a test harness on a child process): a separate settings folder.
    if let Some(p) = std::env::var_os("INVECTUS_DATA_DIR").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    // This crate's own tests never use the person's real folder.
    if cfg!(test) {
        return Some(
            std::env::temp_dir().join(format!("invectus-core-test-{}", std::process::id())),
        );
    }
    data_dir().map(|d| d.join("inVectus"))
}

/// Tests call this first so they never touch the person's real consent or grants. Set once
/// per process; later calls are ignored.
#[doc(hidden)]
pub fn use_data_dir_for_tests(dir: PathBuf) {
    let _ = OVERRIDE.set(dir);
}

fn data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    return std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"));
    #[cfg(target_os = "windows")]
    return std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(all(unix, not(target_os = "macos")))]
    return std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
}
