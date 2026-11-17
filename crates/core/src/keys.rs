//! Project data keys (encryption at rest, phase 1: docs/encryption-plan.md).
//!
//! Every new project gets a random 256-bit data key and a random project id. The key lives in
//! the system keychain (service `app.invectus.projects`, account = project id); `project.json`
//! holds only the id and a key check value (HMAC-SHA-256 of a fixed string and the id), so a
//! wrong or missing key is caught before anything is read. The key is also shown as a
//! **recovery code** (Crockford base32 with a checksum) for the person to keep: a project
//! moved to another computer opens with it.
//!
//! Where keys are looked up, in order:
//! 1. `INVECTUS_PROJECT_KEY`: a recovery code, for headless machines without a keychain (less
//!    safe: anything that reads the environment sees it). Used only if its check value matches.
//! 2. `INVECTUS_KEYSTORE_DIR`: a folder of owner-only key files instead of the keychain. The
//!    repository's `.cargo/config.toml` points it at `.spike-data/dev-keys` for `cargo test`
//!    and `cargo run`, so tests and dev builds never touch the person's keychain; a packaged
//!    app does not have it set.
//! 3. The system keychain.
//!
//! New project databases are encrypted with this key. Losing both the stored key and the
//! recovery code makes their encrypted data inaccessible. Legacy projects await migration.

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const SERVICE: &str = "app.invectus.projects";
const CHECK_LABEL: &[u8] = b"inVectus project key check v1";
/// Crockford base32: no I, L, O or U.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A project's 256-bit data key. Never printed; cleared from memory when dropped.
pub struct DataKey([u8; 32]);

impl std::fmt::Debug for DataKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DataKey(…)")
    }
}

impl Drop for DataKey {
    fn drop(&mut self) {
        for b in self.0.iter_mut() {
            // SAFETY: a valid, aligned `u8` inside `self`; volatile so the wipe is not elided.
            unsafe { std::ptr::write_volatile(b, 0) };
        }
    }
}

fn random(buf: &mut [u8]) -> Result<()> {
    getrandom::fill(buf).map_err(|e| anyhow::anyhow!("the system random source failed: {e}"))
}

/// A new random project id (128 bits, hex).
pub fn new_project_id() -> Result<String> {
    let mut id = [0u8; 16];
    random(&mut id)?;
    Ok(hex::encode(id))
}

fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut k = [0u8; 64];
    k[..key.len()].copy_from_slice(key); // keys here are 32 bytes, under the block size
    let pad = |byte: u8| k.map(|b| b ^ byte);
    let mut inner = Sha256::new();
    inner.update(pad(0x36));
    for p in parts {
        inner.update(p);
    }
    let mut outer = Sha256::new();
    outer.update(pad(0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

impl DataKey {
    pub fn generate() -> Result<Self> {
        let mut k = [0u8; 32];
        random(&mut k)?;
        Ok(Self(k))
    }

    /// The bytes, for the encryption layers (phase 3 on).
    pub fn bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// The value `project.json` keeps to recognise this key for `project_id`.
    pub fn check_value(&self, project_id: &str) -> String {
        hex::encode(hmac_sha256(&self.0, &[CHECK_LABEL, project_id.as_bytes()]))
    }

    /// The key as a recovery code: 32 key bytes and a 2-byte checksum in Crockford base32,
    /// 55 characters in groups of five (`XXXXX-XXXXX-…`).
    pub fn recovery_code(&self) -> String {
        let sum = Sha256::digest(self.0);
        let mut bytes = self.0.to_vec();
        bytes.extend_from_slice(&sum[..2]);
        let mut chars = Vec::with_capacity(55);
        let (mut acc, mut bits) = (0u32, 0u32);
        for b in bytes {
            acc = (acc << 8) | u32::from(b);
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                chars.push(ALPHABET[((acc >> bits) & 31) as usize]);
            }
        }
        if bits > 0 {
            chars.push(ALPHABET[((acc << (5 - bits)) & 31) as usize]);
        }
        let groups: Vec<String> = chars
            .chunks(5)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect();
        groups.join("-")
    }

    /// Read a recovery code. Spaces, dashes and case are ignored; O reads as 0, I and L as 1.
    /// A mistyped character is caught by the checksum.
    pub fn from_recovery_code(code: &str) -> Result<Self> {
        let mut out = Vec::with_capacity(35);
        let (mut acc, mut bits) = (0u32, 0u32);
        let mut count = 0;
        for c in code.chars().filter(|c| !c.is_whitespace() && *c != '-') {
            let c = match c.to_ascii_uppercase() {
                'O' => '0',
                'I' | 'L' => '1',
                c => c,
            };
            let v = ALPHABET
                .iter()
                .position(|&a| a as char == c)
                .with_context(|| format!("{c:?} is not part of a recovery code"))?;
            acc = (acc << 5) | v as u32;
            bits += 5;
            count += 1;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
            }
        }
        if count != 55 || out.len() != 34 {
            bail!("a recovery code has 55 characters (11 groups of five); this one has {count}");
        }
        let key: [u8; 32] = out[..32].try_into().expect("34 bytes");
        if Sha256::digest(key)[..2] != out[32..] {
            bail!("this recovery code has a typo (its checksum does not match)");
        }
        Ok(Self(key))
    }
}

// ---- Where keys are kept ----------------------------------------------------------------

/// Where a key was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum KeySource {
    Environment,
    KeyFile,
    Keychain,
}

fn keystore_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("INVECTUS_KEYSTORE_DIR").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    // This crate's own tests never use the person's keychain.
    if cfg!(test) {
        return Some(
            std::env::temp_dir().join(format!("invectus-keys-test-{}", std::process::id())),
        );
    }
    None
}

fn checked_id(project_id: &str) -> Result<&str> {
    if project_id.len() != 32 || !project_id.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("not a project id: {project_id:?}");
    }
    Ok(project_id)
}

fn entry(project_id: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, project_id).context("opening the system keychain")
}

/// Keep `key` for `project_id`: in the key folder when one is set, else in the keychain.
pub fn store(project_id: &str, key: &DataKey) -> Result<()> {
    let id = checked_id(project_id)?;
    let code = key.recovery_code();
    match keystore_dir() {
        Some(dir) => crate::private::write(&dir.join(format!("{id}.key")), code.as_bytes()),
        None => entry(id)?
            .set_password(&code)
            .context("saving the project key to the system keychain"),
    }
}

/// The key for `project_id` whose check value is `check`, and where it came from; `None` when
/// no matching key is available. A stored key that does not match is an error (a damaged or
/// swapped entry), never silently used.
pub fn load(project_id: &str, check: &str) -> Result<Option<(DataKey, KeySource)>> {
    let id = checked_id(project_id)?;
    if let Some(code) = std::env::var("INVECTUS_PROJECT_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty())
    {
        let key = DataKey::from_recovery_code(&code).context("reading INVECTUS_PROJECT_KEY")?;
        // The variable can hold another project's key; it applies only where it matches.
        if key.check_value(id) == check {
            return Ok(Some((key, KeySource::Environment)));
        }
    }
    let (stored, source) = match keystore_dir() {
        Some(dir) => match std::fs::read_to_string(dir.join(format!("{id}.key"))) {
            Ok(code) => (Some(code), KeySource::KeyFile),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (None, KeySource::KeyFile),
            Err(e) => return Err(e).context("reading the project key file"),
        },
        None => match entry(id)?.get_password() {
            Ok(code) => (Some(code), KeySource::Keychain),
            Err(keyring::Error::NoEntry) => (None, KeySource::Keychain),
            Err(e) => return Err(e).context("reading the project key from the system keychain"),
        },
    };
    let Some(code) = stored else {
        return Ok(None);
    };
    let key = DataKey::from_recovery_code(&code).context("the stored project key is damaged")?;
    if key.check_value(id) != check {
        bail!("the stored key for this project does not match it (was the project replaced?)");
    }
    Ok(Some((key, source)))
}

/// Remove the stored key for `project_id`; removing one that is not there is not an error.
pub fn forget(project_id: &str) -> Result<()> {
    let id = checked_id(project_id)?;
    match keystore_dir() {
        Some(dir) => match std::fs::remove_file(dir.join(format!("{id}.key"))) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        },
        None => match entry(id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e).context("removing the project key from the system keychain"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_codes_round_trip_and_catch_typos() {
        let key = DataKey::generate().unwrap();
        let code = key.recovery_code();
        assert_eq!(code.len(), 55 + 10, "{code}");
        assert_eq!(code.split('-').count(), 11);
        let back = DataKey::from_recovery_code(&code).unwrap();
        assert_eq!(back.bytes(), key.bytes());
        // Case, spacing and look-alike letters do not matter.
        let loose = code
            .to_lowercase()
            .replace('-', " ")
            .replace('0', "o")
            .replace('1', "l");
        assert_eq!(
            DataKey::from_recovery_code(&loose).unwrap().bytes(),
            key.bytes()
        );
        // One changed character is caught.
        let mut chars: Vec<char> = code.chars().collect();
        chars[7] = if chars[7] == 'A' { 'B' } else { 'A' };
        let typo: String = chars.into_iter().collect();
        let err = DataKey::from_recovery_code(&typo).unwrap_err().to_string();
        assert!(err.contains("typo"), "{err}");
        assert!(DataKey::from_recovery_code("ABCDE-12345").is_err());
        assert!(DataKey::from_recovery_code(&format!("U{}", &code[1..])).is_err());
        assert_eq!(format!("{key:?}"), "DataKey(…)");
    }

    #[test]
    fn check_values_bind_the_key_to_the_project() {
        let key = DataKey::generate().unwrap();
        let (a, b) = (new_project_id().unwrap(), new_project_id().unwrap());
        assert_ne!(a, b);
        assert_eq!(key.check_value(&a), key.check_value(&a));
        assert_ne!(key.check_value(&a), key.check_value(&b));
        assert_ne!(
            key.check_value(&a),
            DataKey::generate().unwrap().check_value(&a)
        );
        // RFC 4231 test case 2 checks the HMAC itself.
        assert_eq!(
            hex::encode(hmac_sha256(
                b"Jefe",
                &[b"what do ya want ", b"for nothing?"]
            )),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn keys_are_stored_found_matched_and_forgotten() {
        let id = new_project_id().unwrap();
        let key = DataKey::generate().unwrap();
        let check = key.check_value(&id);
        assert!(load(&id, &check).unwrap().is_none());
        store(&id, &key).unwrap();
        let (found, source) = load(&id, &check).unwrap().unwrap();
        assert_eq!(found.bytes(), key.bytes());
        assert_eq!(source, KeySource::KeyFile);
        // A stored key that does not match the project is refused, not used.
        let other = DataKey::generate().unwrap();
        assert!(load(&id, &other.check_value(&id)).is_err());
        forget(&id).unwrap();
        forget(&id).unwrap();
        assert!(load(&id, &check).unwrap().is_none());
        assert!(store("../x", &key).is_err());
    }
}
