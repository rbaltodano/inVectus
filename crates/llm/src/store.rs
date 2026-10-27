//! Installed language models: `<models root>/<key>/model.gguf`, checksum-verified once and then
//! trusted while its size and modification time are unchanged (hashing 7 GB takes seconds).

use crate::registry::LlmSpec;
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

pub fn model_path(root: &Path, spec: &LlmSpec) -> PathBuf {
    root.join(spec.key).join("model.gguf")
}

pub fn is_installed(root: &Path, spec: &LlmSpec) -> bool {
    std::fs::metadata(model_path(root, spec)).is_ok_and(|m| m.len() == spec.bytes)
}

fn stamp(path: &Path) -> Result<String> {
    let m = std::fs::metadata(path)?;
    let mtime = m
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    Ok(format!("{}|{mtime}", m.len()))
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
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

/// The model file, verified against its pinned SHA-256 (cached in a `.verified` stamp).
pub fn verified_path(root: &Path, spec: &LlmSpec) -> Result<PathBuf> {
    let path = model_path(root, spec);
    let meta = std::fs::metadata(&path).with_context(|| {
        format!(
            "language model {} is not installed (expected {}); download {} from {}@{}",
            spec.key,
            path.display(),
            spec.file,
            spec.repo,
            &spec.revision[..12]
        )
    })?;
    if meta.len() != spec.bytes {
        bail!(
            "{} is {} bytes, expected {}",
            path.display(),
            meta.len(),
            spec.bytes
        );
    }
    let stamp_path = path.with_extension("verified");
    let current = stamp(&path)?;
    if std::fs::read_to_string(&stamp_path).is_ok_and(|s| s == current) {
        return Ok(path);
    }
    let got = sha256_file(&path)?;
    if got != spec.sha256 {
        bail!(
            "{} has SHA-256 {got}, expected {}",
            path.display(),
            spec.sha256
        );
    }
    std::fs::write(&stamp_path, current)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::LlmSpec;

    #[test]
    fn verification_checks_size_and_hash_and_caches() {
        let dir = tempfile::tempdir().unwrap();
        let body = b"not really a model";
        let spec = LlmSpec {
            key: "t",
            repo: "r",
            revision: "0123456789abcdef",
            file: "f.gguf",
            bytes: body.len() as u64,
            sha256: "a0e6ea0b33a6c09f78bb08c9fe1cb2ccee9c6ec1ba24c4c4a1e7f0e1f0fd5ee8",
            license: "x",
            context: 1,
            note: "",
        };
        assert!(verified_path(dir.path(), &spec).is_err(), "missing");
        std::fs::create_dir_all(dir.path().join("t")).unwrap();
        std::fs::write(model_path(dir.path(), &spec), body).unwrap();
        let real = sha256_file(&model_path(dir.path(), &spec)).unwrap();
        let err = verified_path(dir.path(), &spec).unwrap_err().to_string();
        assert!(err.contains("SHA-256"), "{err}");
        let ok = LlmSpec {
            sha256: Box::leak(real.into_boxed_str()),
            ..spec
        };
        assert!(verified_path(dir.path(), &ok).is_ok());
        assert!(
            model_path(dir.path(), &ok)
                .with_extension("verified")
                .is_file()
        );
        assert!(is_installed(dir.path(), &ok));
    }
}
