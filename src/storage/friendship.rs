//! Friendship chain storage.
//!
//! Stores FriendshipChains (8KB per participant) to disk, encrypted with
//! a key derived from identity_seed + device_secret + friendship_id.
//!
//! Storage layout:
//! ~/.config/photon/friendships/{base64(friendship_id)}/chains.vsf.enc

use blake3::Hasher;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use rand::RngCore;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use vsf::schema::{SectionSchema, TypeConstraint};
use vsf::{VsfSection, VsfType};

use crate::types::{FriendshipChains, FriendshipId};

/// Errors from friendship storage operations
#[derive(Debug)]
pub enum FriendshipStorageError {
    Io(std::io::Error),
    Encryption(String),
    Decryption(String),
    Parse(String),
    InvalidChains(String),
}

impl std::fmt::Display for FriendshipStorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {}", e),
            Self::Encryption(s) => write!(f, "Encryption error: {}", s),
            Self::Decryption(s) => write!(f, "Decryption error: {}", s),
            Self::Parse(s) => write!(f, "Parse error: {}", s),
            Self::InvalidChains(s) => write!(f, "Invalid chains: {}", s),
        }
    }
}

impl From<std::io::Error> for FriendshipStorageError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Get the friendships directory
fn friendships_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("photon")
        .join("friendships")
}

/// Get directory for a specific friendship
fn friendship_dir(friendship_id: &FriendshipId) -> PathBuf {
    friendships_dir().join(friendship_id.to_base64())
}

/// Derive encryption key for a friendship's chains
fn chains_encryption_key(
    identity_seed: &[u8; 32],
    device_secret: &[u8; 32],
    friendship_id: &FriendshipId,
) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(identity_seed);
    hasher.update(device_secret);
    hasher.update(friendship_id.as_bytes());
    hasher.update(b"friendship_chains_v1");
    *hasher.finalize().as_bytes()
}

/// Encrypt data with ChaCha20-Poly1305
fn encrypt_data(plaintext: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, FriendshipStorageError> {
    let cipher = ChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| FriendshipStorageError::Encryption(e.to_string()))?;

    // Random nonce
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce: Nonce = nonce_bytes.into();

    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| FriendshipStorageError::Encryption(e.to_string()))?;

    // Prepend nonce to ciphertext
    let mut result = Vec::with_capacity(12 + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend(ciphertext);
    Ok(result)
}

/// Decrypt data with ChaCha20-Poly1305
fn decrypt_data(encrypted: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, FriendshipStorageError> {
    if encrypted.len() < 12 + 16 {
        return Err(FriendshipStorageError::Decryption(
            "Data too short".to_string(),
        ));
    }

    let cipher = ChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| FriendshipStorageError::Decryption(e.to_string()))?;

    let nonce_bytes: [u8; 12] = encrypted[..12]
        .try_into()
        .map_err(|_| FriendshipStorageError::Decryption("Invalid nonce".to_string()))?;
    let nonce: Nonce = nonce_bytes.into();
    let ciphertext = &encrypted[12..];

    cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|e| FriendshipStorageError::Decryption(e.to_string()))
}

/// Schema for friendship_chains section
fn chains_schema() -> SectionSchema {
    SectionSchema::new("friendship_chains")
        .field("version", TypeConstraint::AnyUnsigned)
        .field("friendship_id", TypeConstraint::AnyHash)
        .field("participant", TypeConstraint::AnyHash) // One per participant (handle_hash)
        .field("chain_bytes", TypeConstraint::AnyHash) // All chains concatenated (stored as hb)
}

/// Save FriendshipChains to disk
pub fn save_friendship_chains(
    chains: &FriendshipChains,
    identity_seed: &[u8; 32],
    device_secret: &[u8; 32],
) -> Result<(), FriendshipStorageError> {
    let friendship_id = chains.id();
    let dir = friendship_dir(friendship_id);
    fs::create_dir_all(&dir)?;

    // Build VSF section
    let schema = chains_schema();
    let mut builder = schema
        .build()
        .set("version", 1u8)
        .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?
        .set(
            "friendship_id",
            VsfType::hb(friendship_id.as_bytes().to_vec()),
        )
        .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?;

    // Add each participant's handle_hash using append_multi
    for participant in chains.participants() {
        builder = builder
            .append_multi("participant", vec![VsfType::hb(participant.to_vec())])
            .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?;
    }

    // Add chain bytes (all chains concatenated) - use hb type for binary data
    builder = builder
        .set("chain_bytes", VsfType::hb(chains.chains_to_bytes()))
        .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?;

    let vsf_bytes = builder
        .encode()
        .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?;

    // Encrypt and write
    let encryption_key = chains_encryption_key(identity_seed, device_secret, friendship_id);
    let encrypted = encrypt_data(&vsf_bytes, &encryption_key)?;

    let path = dir.join("chains.vsf.enc");
    let mut file = fs::File::create(&path)?;
    file.write_all(&encrypted)?;

    Ok(())
}

/// Load FriendshipChains from disk
pub fn load_friendship_chains(
    friendship_id: &FriendshipId,
    identity_seed: &[u8; 32],
    device_secret: &[u8; 32],
) -> Result<FriendshipChains, FriendshipStorageError> {
    let dir = friendship_dir(friendship_id);
    let path = dir.join("chains.vsf.enc");

    // Read encrypted file
    let mut file = fs::File::open(&path)?;
    let mut encrypted = Vec::new();
    file.read_to_end(&mut encrypted)?;

    // Decrypt
    let encryption_key = chains_encryption_key(identity_seed, device_secret, friendship_id);
    let vsf_bytes = decrypt_data(&encrypted, &encryption_key)?;

    // Parse VSF
    let mut ptr = 0;
    let section = VsfSection::parse(&vsf_bytes, &mut ptr)
        .map_err(|e| FriendshipStorageError::Parse(format!("VSF parse: {}", e)))?;

    // Extract participants
    let mut participants: Vec<[u8; 32]> = Vec::new();
    for field in section.get_fields("participant") {
        if let Some(VsfType::hb(v)) = field.values.first() {
            if v.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(v);
                participants.push(arr);
            }
        }
    }

    if participants.is_empty() {
        return Err(FriendshipStorageError::InvalidChains(
            "No participants found".to_string(),
        ));
    }

    // Extract chain bytes (stored as hb)
    let chain_bytes = section
        .get_field("chain_bytes")
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::hb(b) => Some(b.clone()),
            _ => None,
        })
        .ok_or_else(|| {
            FriendshipStorageError::InvalidChains("Missing chain_bytes".to_string())
        })?;

    // Reconstruct chains
    FriendshipChains::from_storage(*friendship_id, participants, &chain_bytes).ok_or_else(|| {
        FriendshipStorageError::InvalidChains("Failed to reconstruct chains".to_string())
    })
}

/// Load all friendships from disk
/// Returns HashMap of FriendshipId -> FriendshipChains
pub fn load_all_friendships(
    identity_seed: &[u8; 32],
    device_secret: &[u8; 32],
) -> std::collections::HashMap<FriendshipId, FriendshipChains> {
    let mut result = std::collections::HashMap::new();
    let dir = friendships_dir();

    if !dir.exists() {
        return result;
    }

    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                // Try to parse directory name as friendship_id
                if let Some(name) = entry.file_name().to_str() {
                    if let Some(friendship_id) = FriendshipId::from_base64(name) {
                        match load_friendship_chains(&friendship_id, identity_seed, device_secret) {
                            Ok(chains) => {
                                result.insert(friendship_id, chains);
                            }
                            Err(e) => {
                                crate::log_error(&format!(
                                    "Failed to load friendship {}: {}",
                                    name, e
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_friendship_storage_roundtrip() {
        // Create test chains
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let chains = FriendshipChains::from_clutch(&[alice, bob], &eggs);

        // Test keys
        let identity_seed = [0xAA; 32];
        let device_secret = [0xBB; 32];

        // Save
        save_friendship_chains(&chains, &identity_seed, &device_secret).unwrap();

        // Load
        let loaded =
            load_friendship_chains(chains.id(), &identity_seed, &device_secret).unwrap();

        // Verify
        assert_eq!(loaded.id().as_bytes(), chains.id().as_bytes());
        assert_eq!(loaded.participants(), chains.participants());
        assert_eq!(
            loaded.current_key(&alice).unwrap(),
            chains.current_key(&alice).unwrap()
        );
        assert_eq!(
            loaded.current_key(&bob).unwrap(),
            chains.current_key(&bob).unwrap()
        );
    }
}
