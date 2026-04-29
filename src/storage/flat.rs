//! Flat opaque storage — all disk I/O for Photon goes through here.
//!
//! Files live in ~/.config/photon/ with opaque names derived from the logical key. No subdirectories. No predictable names. No metadata leakage.
//!
//! Filename derivation:
//!   base64url(blake3("photon.storage.filename.v0" || key || identity_seed || device_secret))
//!
//! Encryption per file:
//!   key  = blake3_kdf("photon.storage.encryption.v0", key || identity_seed || device_secret)
//!   data = nonce(12B) || ChaCha20-Poly1305(plaintext)

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use blake3::Hasher;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use rand::RngCore;
use std::fs;
use std::path::PathBuf;

use crate::storage::{read_file, write_file, WritePolicy};

// ============================================================================
// Error ============================================================================

#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    Crypto(String),
    Parse(String),
}

impl From<std::io::Error> for StorageError {
    fn from(e: std::io::Error) -> Self {
        StorageError::Io(e)
    }
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Io(e) => write!(f, "IO: {}", e),
            StorageError::Crypto(s) => write!(f, "Crypto: {}", s),
            StorageError::Parse(s) => write!(f, "Parse: {}", s),
        }
    }
}

// ============================================================================
// FlatStorage ============================================================================

/// All Photon disk I/O goes through this struct.
///
/// Initialized once at auth with identity_seed + device_secret. Callers only see logical keys — filenames and encryption are internal.
pub struct FlatStorage {
    root: PathBuf,
    identity_seed: [u8; 32],
    device_secret: [u8; 32],
}

impl FlatStorage {
    /// Initialize storage. Called once at auth time.
    pub fn new(identity_seed: [u8; 32], device_secret: [u8; 32]) -> Result<Self, StorageError> {
        let root = photon_dir()?;
        fs::create_dir_all(&root)?;
        Ok(Self { root, identity_seed, device_secret })
    }

    /// Write data to opaque file derived from logical key. Atomic (tmp → rename), fsynced, read-back verified. Treat error as fatal.
    pub fn write(&self, key: &str, data: &[u8]) -> Result<(), StorageError> {
        let path = self.root.join(self.derive_filename(key));
        let ciphertext = encrypt(data, &self.derive_enc_key(key))?;
        write_file(&path, &ciphertext, key, WritePolicy::MustSucceed)?;
        Ok(())
    }

    /// Read and decrypt file for logical key. Returns None if not found.
    pub fn read(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let path = self.root.join(self.derive_filename(key));
        if !path.exists() {
            return Ok(None);
        }
        let ciphertext = read_file(&path, key)?;
        Ok(Some(decrypt(&ciphertext, &self.derive_enc_key(key))?))
    }

    /// Delete file for logical key. No-op if not found.
    pub fn delete(&self, key: &str) -> Result<(), StorageError> {
        let path = self.root.join(self.derive_filename(key));
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(())
    }

    // ========================================================================
    // Internal key derivation ========================================================================

    fn derive_filename(&self, key: &str) -> String {
        let mut h = Hasher::new();
        h.update(b"photon.storage.filename.v0");
        h.update(key.as_bytes());
        h.update(&self.identity_seed);
        h.update(&self.device_secret);
        URL_SAFE_NO_PAD.encode(h.finalize().as_bytes())
    }

    fn derive_enc_key(&self, key: &str) -> [u8; 32] {
        let context = [key.as_bytes(), self.identity_seed.as_slice(), self.device_secret.as_slice()].concat();
        blake3::derive_key("photon.storage.encryption.v0", &context)
    }
}

// ============================================================================
// Encryption helpers ============================================================================

fn encrypt(plaintext: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, StorageError> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from(nonce_bytes);
    let mut ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| StorageError::Crypto(e.to_string()))?;
    let mut out = nonce_bytes.to_vec();
    out.append(&mut ciphertext);
    Ok(out)
}

fn decrypt(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, StorageError> {
    if data.len() < 12 {
        return Err(StorageError::Crypto("ciphertext too short for nonce".into()));
    }
    let (nonce_bytes, ciphertext) = data.split_at(12);
    let cipher = ChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .map_err(|e| StorageError::Crypto(e.to_string()))
}

// ============================================================================
// Directory ============================================================================

fn photon_dir() -> Result<PathBuf, StorageError> {
    dirs::config_dir()
        .map(|p| p.join("photon"))
        .ok_or_else(|| StorageError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "config directory not found",
        )))
}
