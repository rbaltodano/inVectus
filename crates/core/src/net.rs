//! Network gate. Inference and search never touch the network; only explicit commands (model
//! download) call [`ensure_online`], which hard-fails in offline mode (SPEC §7.7).

use anyhow::{Result, bail};

/// `INVECTUS_OFFLINE=1` (or the CLI's `--offline`) turns every network call into an error.
pub fn offline() -> bool {
    std::env::var("INVECTUS_OFFLINE").is_ok_and(|v| !v.is_empty() && v != "0")
}

pub fn ensure_online(what: &str) -> Result<()> {
    if offline() {
        bail!("offline mode: refusing network access for {what}");
    }
    Ok(())
}
