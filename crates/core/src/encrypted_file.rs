//! Authenticated file envelope for project snapshots and caches.
//!
//! Version 1 is, in order: `IVEFILE\0` (8 bytes), version (1 byte), block size (u32 LE,
//! always 65536), plaintext length (u64 LE), and a fresh random salt (32 bytes). The
//! 53-byte header is followed by 64 KiB ciphertext blocks, each with a 16-byte GCM tag.
//! The final block may be shorter. Even an empty file has one empty block and its tag.
//!
//! HKDF-SHA256 derives a fresh AES-256-GCM key per write from the project key and salt,
//! binding the domain, project id and relative path with length-delimited context. A
//! block's nonce is four zero bytes followed by its u64 big-endian index. Nonces are
//! unique within each derived key; rewriting a file uses a fresh salt/key. Each tag
//! authenticates the entire header and block index. Length, order and file context
//! therefore cannot change unnoticed. This does not prevent replaying a complete older
//! version of the same file, or hide its size or relative path.
//!
//! No plaintext files are created here. Input/output plaintext belongs to the caller;
//! temporary plaintext buffers are wiped on drop, but the process and ring's expanded
//! key state are not guaranteed to be free of copies after use.

use crate::keys::DataKey;
use anyhow::{Result, bail};
use ring::{aead, hkdf};

const MAGIC: &[u8; 8] = b"IVEFILE\0";
const VERSION: u8 = 1;
pub const BLOCK_SIZE: usize = 64 * 1024;
pub const MAX_PLAINTEXT_BYTES: usize = 512 * 1024 * 1024;
const HEADER_LEN: usize = 53;
const TAG_LEN: usize = 16;
const SALT_START: usize = 21;
const DOMAIN: &[u8] = b"inVectus encrypted file AES-256-GCM v1";

/// Recognise the envelope marker only. This does not validate the version or authenticate
/// the file; callers must use [`open`] before trusting any content.
pub fn is_encrypted(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// Wipe temporary plaintext, including authenticated earlier blocks if a later block fails.
struct Sensitive(Vec<u8>);

impl Sensitive {
    fn with_capacity(capacity: usize) -> Result<Self> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|_| anyhow::anyhow!("not enough memory for the encrypted file"))?;
        Ok(Self(bytes))
    }

    fn into_vec(mut self) -> Vec<u8> {
        std::mem::take(&mut self.0)
    }
}

impl Drop for Sensitive {
    fn drop(&mut self) {
        for byte in &mut self.0 {
            // SAFETY: a live, uniquely borrowed byte; volatile prevents eliding this wipe.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
    }
}

fn key_for(
    key: &DataKey,
    project_id: &str,
    relative_path: &str,
    salt: &[u8],
) -> Result<aead::LessSafeKey> {
    // Bounds context allocation/work as well as keeping context strings unambiguous.
    if project_id.is_empty()
        || project_id.len() > 128
        || relative_path.is_empty()
        || relative_path.len() > 4096
    {
        bail!("invalid encrypted file context");
    }
    let project_len = (project_id.len() as u64).to_le_bytes();
    let path_len = (relative_path.len() as u64).to_le_bytes();
    let info = [
        DOMAIN,
        &project_len,
        project_id.as_bytes(),
        &path_len,
        relative_path.as_bytes(),
    ];
    let salt = hkdf::Salt::new(hkdf::HKDF_SHA256, salt);
    let prk = salt.extract(key.bytes());
    let okm = prk
        .expand(&info, &aead::AES_256_GCM)
        .map_err(|_| anyhow::anyhow!("deriving the encrypted file key failed"))?;
    // ring consumes the HKDF output directly; no derived raw-key temporary is exposed here.
    Ok(aead::LessSafeKey::new(aead::UnboundKey::from(okm)))
}

fn blocks(len: usize) -> usize {
    len.div_ceil(BLOCK_SIZE).max(1)
}

fn envelope_len(len: usize) -> Result<usize> {
    if len > MAX_PLAINTEXT_BYTES {
        bail!("encrypted file exceeds the 512 MiB plaintext limit");
    }
    len.checked_add(blocks(len) * TAG_LEN)
        .and_then(|n| n.checked_add(HEADER_LEN))
        .ok_or_else(|| anyhow::anyhow!("encrypted file length overflow"))
}

fn nonce(index: usize) -> aead::Nonce {
    let mut bytes = [0; 12];
    bytes[4..].copy_from_slice(&(index as u64).to_be_bytes());
    aead::Nonce::assume_unique_for_key(bytes)
}

fn aad(header: &[u8], index: usize) -> [u8; HEADER_LEN + 8] {
    let mut bytes = [0; HEADER_LEN + 8];
    bytes[..HEADER_LEN].copy_from_slice(header);
    bytes[HEADER_LEN..].copy_from_slice(&(index as u64).to_le_bytes());
    bytes
}

/// Seal a whole file in memory. `relative_path` must be the same normalized project-relative
/// path when opening; normalization and project policy belong to the caller.
pub fn seal(
    key: &DataKey,
    project_id: &str,
    relative_path: &str,
    plaintext: &[u8],
) -> Result<Vec<u8>> {
    let len = envelope_len(plaintext.len())?;
    let mut header = [0; HEADER_LEN];
    header[..8].copy_from_slice(MAGIC);
    header[8] = VERSION;
    header[9..13].copy_from_slice(&(BLOCK_SIZE as u32).to_le_bytes());
    header[13..21].copy_from_slice(&(plaintext.len() as u64).to_le_bytes());
    getrandom::fill(&mut header[SALT_START..])
        .map_err(|_| anyhow::anyhow!("the system random source failed"))?;
    let cipher = key_for(key, project_id, relative_path, &header[SALT_START..])?;
    let mut out = Sensitive::with_capacity(len)?;
    out.0.extend_from_slice(&header);
    for index in 0..blocks(plaintext.len()) {
        let start = index * BLOCK_SIZE;
        let end = (start + BLOCK_SIZE).min(plaintext.len());
        let offset = out.0.len();
        out.0.extend_from_slice(&plaintext[start..end]);
        let tag = cipher
            .seal_in_place_separate_tag(
                nonce(index),
                aead::Aad::from(aad(&header, index)),
                &mut out.0[offset..],
            )
            .map_err(|_| anyhow::anyhow!("encrypting the file failed"))?;
        out.0.extend_from_slice(tag.as_ref());
    }
    Ok(out.into_vec())
}

/// Authenticate and decrypt a complete envelope, refusing malformed, oversized or modified
/// input before returning any plaintext. There is no plaintext-format fallback.
pub fn open(
    key: &DataKey,
    project_id: &str,
    relative_path: &str,
    envelope: &[u8],
) -> Result<Vec<u8>> {
    if envelope.len() < HEADER_LEN + TAG_LEN {
        bail!("encrypted file is truncated or has no authentication tag");
    }
    let header = &envelope[..HEADER_LEN];
    if &header[..8] != MAGIC || header[8] != VERSION {
        bail!("unrecognised encrypted file format or version");
    }
    let block_size = u32::from_le_bytes(header[9..13].try_into().expect("fixed header field"));
    if block_size as usize != BLOCK_SIZE {
        bail!("unrecognised encrypted file block size");
    }
    let len = u64::from_le_bytes(header[13..21].try_into().expect("fixed header field"));
    let len = usize::try_from(len).map_err(|_| anyhow::anyhow!("encrypted file is too large"))?;
    if envelope_len(len)? != envelope.len() {
        bail!("encrypted file length does not match its header");
    }
    let cipher = key_for(key, project_id, relative_path, &header[SALT_START..])?;
    let mut out = Sensitive::with_capacity(len)?;
    let mut block = Sensitive::with_capacity(BLOCK_SIZE + TAG_LEN)?;
    let mut offset = HEADER_LEN;
    for index in 0..blocks(len) {
        let plain_len = (len - index * BLOCK_SIZE).min(BLOCK_SIZE);
        let end = offset + plain_len + TAG_LEN;
        block.0.clear();
        block.0.extend_from_slice(&envelope[offset..end]);
        let plaintext = cipher
            .open_in_place(
                nonce(index),
                aead::Aad::from(aad(header, index)),
                &mut block.0,
            )
            .map_err(|_| anyhow::anyhow!("encrypted file authentication failed"))?;
        out.0.extend_from_slice(plaintext);
        // Wipe before clear/reuse, including the final block and any failure path.
        for byte in &mut block.0 {
            // SAFETY: a live, uniquely borrowed byte inside the temporary block.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        offset = end;
    }
    Ok(out.into_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT: &str = "0123456789abcdef0123456789abcdef";
    const PATH: &str = "sources/notion/example.csv";

    #[test]
    fn round_trips_empty_and_multiblock_files() {
        let key = DataKey::generate().unwrap();
        for len in [
            0,
            1,
            BLOCK_SIZE - 1,
            BLOCK_SIZE,
            BLOCK_SIZE + 1,
            3 * BLOCK_SIZE,
        ] {
            let plaintext: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            let sealed = seal(&key, PROJECT, PATH, &plaintext).unwrap();
            assert_eq!(sealed.len(), envelope_len(len).unwrap());
            assert_eq!(open(&key, PROJECT, PATH, &sealed).unwrap(), plaintext);
        }
    }

    #[test]
    fn rewriting_uses_a_new_salt_and_ciphertext() {
        let key = DataKey::generate().unwrap();
        let first = seal(&key, PROJECT, PATH, b"private customer text").unwrap();
        let second = seal(&key, PROJECT, PATH, b"private customer text").unwrap();
        assert_ne!(
            &first[SALT_START..HEADER_LEN],
            &second[SALT_START..HEADER_LEN]
        );
        assert_ne!(&first[HEADER_LEN..], &second[HEADER_LEN..]);
        assert!(!first.windows(21).any(|w| w == b"private customer text"));
    }

    #[test]
    fn keys_and_contexts_cannot_be_substituted() {
        let key = DataKey::generate().unwrap();
        let other = DataKey::generate().unwrap();
        let sealed = seal(&key, PROJECT, PATH, b"private").unwrap();
        assert!(open(&other, PROJECT, PATH, &sealed).is_err());
        assert!(open(&key, "other-project", PATH, &sealed).is_err());
        assert!(open(&key, PROJECT, "cache/verify/example.json", &sealed).is_err());
        // Length delimiters prevent ambiguous concatenations in the derived-key context.
        let sealed = seal(&key, "a", "bc", b"private").unwrap();
        assert!(open(&key, "ab", "c", &sealed).is_err());
    }

    #[test]
    fn every_header_ciphertext_and_tag_byte_is_authenticated() {
        let key = DataKey::generate().unwrap();
        let sealed = seal(&key, PROJECT, PATH, b"private").unwrap();
        for offset in 0..sealed.len() {
            let mut modified = sealed.clone();
            modified[offset] ^= 1;
            assert!(
                open(&key, PROJECT, PATH, &modified).is_err(),
                "byte {offset}"
            );
        }
    }

    #[test]
    fn truncation_extra_bytes_plaintext_and_oversized_headers_are_refused() {
        let key = DataKey::generate().unwrap();
        let sealed = seal(&key, PROJECT, PATH, b"private").unwrap();
        for len in 0..sealed.len() {
            assert!(open(&key, PROJECT, PATH, &sealed[..len]).is_err());
        }
        let mut extra = sealed.clone();
        extra.push(0);
        assert!(open(&key, PROJECT, PATH, &extra).is_err());
        assert!(open(&key, PROJECT, PATH, b"a,b\nprivate,1\n").is_err());
        let mut huge = sealed;
        huge[13..21].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(open(&key, PROJECT, PATH, &huge).is_err());
        assert!(envelope_len(MAX_PLAINTEXT_BYTES + 1).is_err());
    }

    #[test]
    fn blocks_cannot_be_reordered_or_copied_from_another_write() {
        let key = DataKey::generate().unwrap();
        let plaintext = vec![42; BLOCK_SIZE * 2];
        let sealed = seal(&key, PROJECT, PATH, &plaintext).unwrap();
        let span = BLOCK_SIZE + TAG_LEN;
        let mut swapped = sealed.clone();
        let (first, second) = swapped[HEADER_LEN..].split_at_mut(span);
        first.swap_with_slice(second);
        assert!(open(&key, PROJECT, PATH, &swapped).is_err());
        let other = seal(&key, PROJECT, PATH, &plaintext).unwrap();
        let mut substituted = sealed;
        substituted[HEADER_LEN..HEADER_LEN + span]
            .copy_from_slice(&other[HEADER_LEN..HEADER_LEN + span]);
        assert!(open(&key, PROJECT, PATH, &substituted).is_err());
    }
}
