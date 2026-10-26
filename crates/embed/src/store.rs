//! Installed models in the OS app-data folder, shared by every project (SPEC §7.4).
//!
//! Layout: `<models>/<key>/<revision>/<file>` plus `install.json` recording each file's SHA-256,
//! size and modification time. Installation (explicit download, or copy from a local folder) is
//! the only step that may use the network; loading never does.

use crate::registry::{ModelFile, ModelSpec};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Clone, Debug)]
pub struct ModelStore {
    pub root: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct InstallRecord {
    key: String,
    repo: String,
    revision: String,
    files: Vec<InstalledFile>,
}

#[derive(Debug, Serialize, Deserialize)]
struct InstalledFile {
    local: String,
    sha256: String,
    bytes: u64,
    mtime_ns: u128,
}

#[derive(Debug, PartialEq, Eq)]
pub enum InstallMethod {
    HardLink,
    Copy,
    Download,
}

impl ModelStore {
    /// `INVECTUS_MODELS_DIR`, else `<OS data dir>/inVectus/models`.
    pub fn default_root() -> Result<PathBuf> {
        if let Some(d) = std::env::var_os("INVECTUS_MODELS_DIR") {
            return Ok(PathBuf::from(d));
        }
        Ok(dirs::data_dir()
            .context("no OS data directory; set INVECTUS_MODELS_DIR")?
            .join("inVectus")
            .join("models"))
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn dir(&self, spec: &ModelSpec) -> PathBuf {
        self.root.join(spec.key).join(spec.revision)
    }

    pub fn is_installed(&self, spec: &ModelSpec) -> bool {
        self.dir(spec).join("install.json").is_file()
    }

    /// Verify an installed model and return its folder. Files whose size and mtime match the
    /// install record are trusted; anything else is re-hashed against the pinned SHA-256.
    pub fn verified_dir(&self, spec: &ModelSpec) -> Result<PathBuf> {
        let dir = self.dir(spec);
        let rec_path = dir.join("install.json");
        let raw = std::fs::read(&rec_path).with_context(|| {
            format!(
                "model {} is not installed in {}; run `invectus models install {}` (from a local \
                 folder) or `invectus models download {}`",
                spec.key,
                self.root.display(),
                spec.key,
                spec.key
            )
        })?;
        let rec: InstallRecord = serde_json::from_slice(&raw)?;
        if rec.revision != spec.revision || rec.repo != spec.repo {
            bail!(
                "install record in {} does not match the pinned revision",
                dir.display()
            );
        }
        for f in spec.files {
            let path = dir.join(f.local);
            let (bytes, mtime_ns) = stat(&path)?;
            let fast_ok = rec.files.iter().any(|r| {
                r.local == f.local
                    && r.sha256 == f.sha256
                    && r.bytes == bytes
                    && r.mtime_ns == mtime_ns
            });
            if !fast_ok {
                let got = sha256_file(&path)?;
                if got != f.sha256 {
                    bail!(
                        "{} has SHA-256 {got}, expected {}; reinstall the model",
                        path.display(),
                        f.sha256
                    );
                }
            }
        }
        Ok(dir)
    }

    /// Install from a local folder containing the files by their local names (e.g. the P0
    /// `.spike-data/models/bge`). Every file is checksum-verified before it is linked or copied.
    pub fn install_from_dir(&self, spec: &ModelSpec, src: &Path) -> Result<InstallMethod> {
        let dir = self.dir(spec);
        std::fs::create_dir_all(&dir)?;
        let mut method = InstallMethod::HardLink;
        for f in spec.files {
            let from = src.join(f.local);
            let got = sha256_file(&from)?;
            if got != f.sha256 {
                bail!(
                    "{} has SHA-256 {got}, expected {}",
                    from.display(),
                    f.sha256
                );
            }
            let to = dir.join(f.local);
            let _ = std::fs::remove_file(&to);
            if std::fs::hard_link(&from, &to).is_err() {
                let tmp = to.with_extension("partial");
                std::fs::copy(&from, &tmp)?;
                std::fs::rename(&tmp, &to)?;
                method = InstallMethod::Copy;
            }
        }
        self.write_record(spec)?;
        Ok(method)
    }

    /// Explicit network download from Hugging Face at the pinned revision (never called by
    /// inference). Hard-fails in offline mode.
    pub fn download(&self, spec: &ModelSpec, log: &mut dyn FnMut(&str)) -> Result<InstallMethod> {
        invectus_core::net::ensure_online(&format!("downloading model {}", spec.key))?;
        let dir = self.dir(spec);
        std::fs::create_dir_all(&dir)?;
        for f in spec.files {
            let to = dir.join(f.local);
            if to.is_file() && sha256_file(&to)? == f.sha256 {
                log(&format!("{} already present", f.local));
                continue;
            }
            let url = format!(
                "https://huggingface.co/{}/resolve/{}/{}",
                spec.repo, spec.revision, f.repo_path
            );
            log(&format!("GET {url} ({} bytes)", f.bytes));
            download_verified(&url, f, &to)?;
        }
        self.write_record(spec)?;
        Ok(InstallMethod::Download)
    }

    fn write_record(&self, spec: &ModelSpec) -> Result<()> {
        let dir = self.dir(spec);
        let files = spec
            .files
            .iter()
            .map(|f| {
                let (bytes, mtime_ns) = stat(&dir.join(f.local))?;
                Ok(InstalledFile {
                    local: f.local.into(),
                    sha256: f.sha256.into(),
                    bytes,
                    mtime_ns,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let rec = InstallRecord {
            key: spec.key.into(),
            repo: spec.repo.into(),
            revision: spec.revision.into(),
            files,
        };
        std::fs::write(dir.join("install.json"), serde_json::to_vec_pretty(&rec)?)?;
        Ok(())
    }
}

fn download_verified(url: &str, f: &ModelFile, to: &Path) -> Result<()> {
    let tmp = to.with_extension("partial");
    let mut reader = ureq::get(url)
        .call()
        .with_context(|| format!("GET {url}"))?
        .into_body()
        .into_reader();
    let mut out = File::create(&tmp)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut n_total = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        n_total += n as u64;
        if n_total > f.bytes {
            bail!("{url} is larger than the pinned {} bytes", f.bytes);
        }
    }
    out.sync_all()?;
    let got = hex::encode(h.finalize());
    if got != f.sha256 {
        let _ = std::fs::remove_file(&tmp);
        bail!("{url} has SHA-256 {got}, expected {}", f.sha256);
    }
    std::fs::rename(&tmp, to)?;
    Ok(())
}

fn stat(path: &Path) -> Result<(u64, u128)> {
    let m = std::fs::metadata(path).with_context(|| format!("{} is missing", path.display()))?;
    let mtime = m
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Ok((m.len(), mtime))
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut f = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ModelFile;
    use invectus_core::{Normalization, Pooling};

    fn sha(b: &[u8]) -> String {
        hex::encode(Sha256::digest(b))
    }

    fn leak(s: String) -> &'static str {
        Box::leak(s.into_boxed_str())
    }

    fn fake_spec(content: &[u8]) -> &'static ModelSpec {
        let files = Box::leak(Box::new([ModelFile {
            repo_path: "w.bin",
            local: "w.bin",
            bytes: content.len() as u64,
            sha256: leak(sha(content)),
        }]));
        Box::leak(Box::new(ModelSpec {
            key: "fake",
            repo: "x/fake",
            revision: "r1",
            files,
            document_prefix: "",
            query_prefix: "",
            pooling: Pooling::Cls,
            normalization: Normalization::L2,
            max_tokens: 8,
            dimensions: 2,
            note: "",
        }))
    }

    #[test]
    fn install_verifies_checksums_and_detects_tampering() {
        let src = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("w.bin"), b"weights").unwrap();
        let store = ModelStore::new(root.path());
        let spec = fake_spec(b"weights");
        assert!(store.verified_dir(spec).is_err(), "not installed yet");
        store.install_from_dir(spec, src.path()).unwrap();
        assert!(store.verified_dir(spec).is_ok());

        // Same size, different content and mtime: detected by re-hashing.
        let installed = store.dir(spec).join("w.bin");
        std::fs::remove_file(&installed).unwrap();
        std::fs::write(&installed, b"WEIGHTS").unwrap();
        let err = store.verified_dir(spec).unwrap_err().to_string();
        assert!(err.contains("expected"), "{err}");

        // A wrong source file is refused before anything is linked.
        let bad = fake_spec(b"other!!");
        assert!(store.install_from_dir(bad, src.path()).is_err());
    }

    #[test]
    fn download_is_refused_offline() {
        // SAFETY: tests in this crate do not read INVECTUS_OFFLINE concurrently except this one.
        unsafe { std::env::set_var("INVECTUS_OFFLINE", "1") };
        let root = tempfile::tempdir().unwrap();
        let err = ModelStore::new(root.path())
            .download(fake_spec(b"z"), &mut |_| {})
            .unwrap_err()
            .to_string();
        unsafe { std::env::remove_var("INVECTUS_OFFLINE") };
        assert!(err.contains("offline"), "{err}");
    }
}
