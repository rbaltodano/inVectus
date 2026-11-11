//! Integration tokens in the OS keychain (macOS Keychain, Windows Credential Manager, Linux
//! kernel keyring). They never go into project files, logs or the item store.
//! `INVECTUS_<NAME>_TOKEN` overrides the keychain (CLI and CI use).

use anyhow::{Context, Result, bail};

const SERVICE: &str = "app.invectus.integrations";

/// Integrations that keep a token.
pub const NAMES: &[&str] = &["notion"];

fn check(name: &str) -> Result<()> {
    if !NAMES.contains(&name) {
        bail!(
            "unknown integration {name:?}; expected one of {}",
            NAMES.join(", ")
        );
    }
    Ok(())
}

fn env_name(name: &str) -> String {
    format!("INVECTUS_{}_TOKEN", name.to_ascii_uppercase())
}

fn entry(name: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, name).context("opening the system keychain")
}

/// The token for `name`: the environment override, else the keychain, else `None`.
pub fn get(name: &str) -> Result<Option<String>> {
    check(name)?;
    if let Some(v) = std::env::var(env_name(name))
        .ok()
        .filter(|v| !v.trim().is_empty())
    {
        return Ok(Some(v.trim().to_string()));
    }
    match entry(name)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e).context("reading the system keychain"),
    }
}

pub fn set(name: &str, token: &str) -> Result<()> {
    check(name)?;
    let token = token.trim();
    if token.is_empty() {
        bail!("the token is empty");
    }
    entry(name)?
        .set_password(token)
        .context("saving to the system keychain")
}

/// Remove the stored token; removing one that is not there is not an error.
pub fn delete(name: &str) -> Result<()> {
    check(name)?;
    match entry(name)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e).context("removing from the system keychain"),
    }
}

/// Whether a token is available, and where from ("environment" or "keychain").
pub fn source(name: &str) -> Result<Option<&'static str>> {
    check(name)?;
    if std::env::var(env_name(name)).is_ok_and(|v| !v.trim().is_empty()) {
        return Ok(Some("environment"));
    }
    Ok(get(name)?.map(|_| "keychain"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_names_are_refused() {
        assert!(get("dropbox").is_err());
        assert!(set("dropbox", "x").is_err());
    }
}
