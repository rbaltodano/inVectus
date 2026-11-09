//! Network gate. Import, embedding, search, regions and gap scans never call the network.
//! Model downloads and outside-source checks call [`ensure_online`]. GitHub starring and client
//! sign-in probes check [`offline`]. Both refuse the work when offline mode is set (SPEC §7.7).

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
