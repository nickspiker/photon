pub mod cloud;
pub mod contacts;
pub mod flat;
pub mod friendship;

pub use flat::{FlatStorage, StorageError};

/// Returns ~/.config/photon/ (or Android equivalent). All Photon files live here.
pub fn photon_config_dir() -> Result<std::path::PathBuf, std::io::Error> {
    #[cfg(target_os = "android")]
    {
        use crate::ui::avatar::get_android_data_dir;
        get_android_data_dir()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "Android data dir not set"))
    }
    #[cfg(not(target_os = "android"))]
    {
        dirs::config_dir()
            .map(|p| p.join("photon"))
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "config dir not found"))
    }
}

// ============================================================================
// Unified Storage I/O ============================================================================

use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use rand::RngCore;
use std::fs;
use std::path::Path;

// ============================================================================
// Shared encryption (one ChaCha20-Poly1305 call site for the whole project) ====================

/// Encrypt with ChaCha20-Poly1305 + a fresh 12-byte random nonce. Output layout is `[nonce: 12B] || [ciphertext + 16B auth tag]` — both local-disk (`flat.rs`) and cloud (`cloud.rs`) blobs share this format so a future cross-cutting change (algorithm bump, AAD scheme, etc.) lands in one place. Returns the error stringified — callers wrap into their domain error type.
pub fn encrypt_bytes(plaintext: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, String> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from(nonce_bytes);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(12 + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt a blob produced by [`encrypt_bytes`]. Expects `[nonce: 12B] || [ciphertext + 16B auth tag]`. AEAD failure (wrong key, tampered ciphertext, truncated input) flows thru as a stringified error.
pub fn decrypt_bytes(blob: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, String> {
    if blob.len() < 12 + 16 {
        return Err(format!(
            "ciphertext too short: {} bytes (need ≥ 28 for nonce + auth tag)",
            blob.len()
        ));
    }
    let (nonce_bytes, ciphertext) = blob.split_at(12);
    let cipher = ChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .map_err(|e| e.to_string())
}

/// Unified disk write: all storage writes go thru this function. Every write is read-back-verified before returning success — if the bytes on disk don't match the bytes we asked to write, the call returns an error and the caller treats that as a hard failure. No "best effort" path; silent corruption is forbidden, and the cost of a `fs::read` per write is cheap against the cost of discovering on next launch that a contact's messages didn't actually persist.
///
/// - Ensures parent directory exists
/// - Writes to a fresh-random-named sibling first, then atomically renames into place
/// - Calls fsync to ensure data reaches disk (critical for crash safety)
/// - Reads back the file and compares byte-for-byte against the data we asked to write
///
/// The pre-rename file uses a random base64url name (not a `.tmp` extension) so in-flight writes are indistinguishable in shape from finished files — `~/.config/photon/` stays FAF (flat as fuck), no metadata leak about which file was being written when a crash happened.
pub fn write_file(path: &Path, data: &[u8], label: &str) -> Result<(), std::io::Error> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use rand::RngCore;

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            crate::log(&format!("STORAGE: Failed to create dir for {}: {}", label, e));
            return Err(e);
        }
    }

    // Fresh random sibling — looks like any other opaque file on disk. 24 random bytes → 32-char base64url, matching the filename-shape FlatStorage already uses for everything else.
    let tmp_path = {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let mut rand_bytes = [0u8; 24];
        rand::thread_rng().fill_bytes(&mut rand_bytes);
        let rand_name = URL_SAFE_NO_PAD.encode(rand_bytes);
        parent.join(rand_name)
    };

    if let Err(e) = fs::write(&tmp_path, data) {
        let _ = fs::remove_file(&tmp_path);
        crate::log(&format!("STORAGE: Failed to write {}: {}", label, e));
        return Err(e);
    }

    // fsync the temp file before rename so the renamed inode points at durable bytes.
    if let Ok(f) = fs::File::open(&tmp_path) {
        let _ = f.sync_all();
    }
    if let Err(e) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        crate::log(&format!("STORAGE: Failed to rename {}: {}", label, e));
        return Err(e);
    }

    // Read-back verify: every write, no exceptions. If the bytes on disk don't match what we sent, fail loudly — silent persistence corruption is the worst failure mode for a personal-data store.
    match fs::read(path) {
        Ok(readback) if readback.len() == data.len() && readback == data => Ok(()),
        Ok(readback) => {
            crate::log(&format!(
                "STORAGE: Write verification failed for {} (wrote {} bytes, read back {} bytes)",
                label,
                data.len(),
                readback.len()
            ));
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "write verification failed: data mismatch",
            ))
        }
        Err(e) => {
            crate::log(&format!(
                "STORAGE: Write verification read-back failed for {}: {}",
                label, e
            ));
            Err(e)
        }
    }
}

/// Unified disk read: all storage reads go thru this function.
///
/// Logs a contextual error message on failure and returns the io::Error.
pub fn read_file(path: &Path, label: &str) -> Result<Vec<u8>, std::io::Error> {
    fs::read(path).map_err(|e| {
        crate::log(&format!("STORAGE: Failed to read {}: {}", label, e));
        e
    })
}
