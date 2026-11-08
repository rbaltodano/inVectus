//! Star inVectus on GitHub from the app, through the person's own GitHub CLI login: the click
//! runs `gh api -X PUT user/starred/<repo>`, so inVectus never reads or stores a token. Nothing
//! is checked or sent before the click (no hidden network), and offline mode refuses it.

use anyhow::{Result, bail};
use serde::Serialize;
use std::path::Path;
use std::time::Duration;

/// This project's repository on GitHub.
pub const REPOSITORY: &str = "rbaltodano/inVectus";

/// What starring did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StarOutcome {
    Starred,
    /// No `gh` command, or it is not signed in (or GitHub refused): open the page instead.
    Unavailable,
}

/// Star [`REPOSITORY`] with the `gh` found on this computer.
pub fn star() -> Result<StarOutcome> {
    if invectus_core::net::offline() {
        bail!("offline mode: starring on GitHub needs the network");
    }
    let name = format!("gh{}", std::env::consts::EXE_SUFFIX);
    match crate::connect::find_in_path(&name) {
        Some(gh) => star_with(&gh, REPOSITORY),
        None => Ok(StarOutcome::Unavailable),
    }
}

/// `gh api` exits non-zero unless GitHub answers 2xx (204 for a star).
pub fn star_with(gh: &Path, repository: &str) -> Result<StarOutcome> {
    let path = format!("user/starred/{repository}");
    let out = crate::connect::run_with_timeout_env(
        gh,
        &["api", "--method", "PUT", &path, "--silent"],
        &[("GH_PROMPT_DISABLED", "1")],
        Duration::from_secs(20),
    );
    Ok(match out {
        Ok(o) if o.status.success() => StarOutcome::Starred,
        _ => StarOutcome::Unavailable,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fake_gh(dir: &Path, code: i32) -> std::path::PathBuf {
        let path = dir.join("gh");
        let log = dir.join("gh.log");
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\necho \"$GH_PROMPT_DISABLED $@\" > {}\nexit {code}\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn stars_through_gh_and_falls_back_when_refused() {
        let dir = tempfile::tempdir().unwrap();
        let ok = fake_gh(dir.path(), 0);
        assert_eq!(star_with(&ok, "o/r").unwrap(), StarOutcome::Starred);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("gh.log")).unwrap(),
            "1 api --method PUT user/starred/o/r --silent\n"
        );
        let refused = fake_gh(dir.path(), 1);
        assert_eq!(
            star_with(&refused, "o/r").unwrap(),
            StarOutcome::Unavailable
        );
    }
}
