//! DuckDB instances that can write encrypted databases.
//!
//! The bundled DuckDB refuses to write encrypted files with its built-in crypto module, because
//! that module draws nonces from a non-cryptographic generator. `cpp/duckdb_os_random.cpp`
//! installs the same AES-256-GCM module with the operating system's random generator instead;
//! this module opens an in-memory DuckDB instance through the C API, installs it, and hands the
//! instance to a [`Connection`] that owns it (closed when it and all its clones are dropped;
//! `Connection::open_from_raw_owned` is our patch to the vendored duckdb-rs). Encrypted project
//! databases are then attached to that instance (`ATTACH … (ENCRYPTION_KEY …)`).

use crate::keys::DataKey;
use anyhow::{Result, bail};
use duckdb::{Connection, ffi};
use std::ffi::{CStr, c_char, c_int};
use std::ptr;

unsafe extern "C" {
    fn invectus_duckdb_use_os_random(database: ffi::duckdb_database) -> c_int;
    fn invectus_duckdb_os_random_calls() -> u64;
}

/// Open an in-memory instance with extension auto-install and auto-load off (no hidden
/// network) and the OS-random encryption module installed.
pub(crate) fn instance() -> Result<Connection> {
    let mut config: ffi::duckdb_config = ptr::null_mut();
    // SAFETY: plain C API calls on handles created here; the config is destroyed on every path.
    unsafe {
        if ffi::duckdb_create_config(&mut config) != ffi::DuckDBSuccess {
            bail!("could not create a DuckDB configuration");
        }
        for (name, value) in [
            (c"autoinstall_known_extensions", c"false"),
            (c"autoload_known_extensions", c"false"),
        ] {
            if ffi::duckdb_set_config(config, name.as_ptr(), value.as_ptr()) != ffi::DuckDBSuccess {
                ffi::duckdb_destroy_config(&mut config);
                bail!("could not set DuckDB option {name:?}");
            }
        }
        let mut db: ffi::duckdb_database = ptr::null_mut();
        let mut err: *mut c_char = ptr::null_mut();
        let state = ffi::duckdb_open_ext(c":memory:".as_ptr(), &mut db, config, &mut err);
        ffi::duckdb_destroy_config(&mut config);
        if state != ffi::DuckDBSuccess {
            let msg = if err.is_null() {
                "unknown error".to_owned()
            } else {
                let m = CStr::from_ptr(err).to_string_lossy().into_owned();
                ffi::duckdb_free(err.cast());
                m
            };
            bail!("could not open DuckDB: {msg}");
        }
        if invectus_duckdb_use_os_random(db) != 0 {
            ffi::duckdb_close(&mut db);
            bail!("could not install the encryption module in DuckDB");
        }
        // From here the connection owns `db` (also when connecting fails).
        Ok(Connection::open_from_raw_owned(db)?)
    }
}

/// How many times DuckDB has drawn random bytes from the OS generator in this process.
pub fn os_random_calls() -> u64 {
    // SAFETY: reads an atomic counter.
    unsafe { invectus_duckdb_os_random_calls() }
}

/// SQL that attaches `path` as an encrypted database named `alias`. DuckDB base64-decodes the
/// key and derives the file key as SHA-256(per-file salt ‖ key). The returned string holds the
/// key: pass it to DuckDB, then [`wipe`] it.
pub(crate) fn attach_sql(path: &str, alias: &str, key: &DataKey) -> String {
    format!(
        "ATTACH '{}' AS {alias} (ENCRYPTION_KEY '{}', ENCRYPTION_CIPHER 'GCM')",
        path.replace('\'', "''"),
        base64(key.bytes())
    )
}

/// Overwrite a string that held key material. Best effort only: the base64 temporary, the C
/// string duckdb-rs makes, and DuckDB's query and parser copies are not wiped (DuckDB clears
/// only its own copy of the key option after deriving the file key).
pub(crate) fn wipe(s: String) {
    let mut bytes = s.into_bytes();
    for b in bytes.iter_mut() {
        // SAFETY: a valid, aligned `u8`; volatile so the wipe is not elided.
        unsafe { ptr::write_volatile(b, 0) };
    }
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | (u32::from(b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc_4648() {
        for (input, want) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), want);
        }
    }
}
