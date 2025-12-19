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
///
/// Photon-specific VSF wrapped types (uppercase = application-specific):
/// - vC = CLUTCH chain (512×32 = 16KB key chain per participant)
/// - vX = Ciphertext (encrypted message bytes)
///
/// Standard VSF types:
/// - x = UTF-8 text (Huffman compressed Unicode) for message plaintexts
fn chains_schema() -> SectionSchema {
    SectionSchema::new("friendship_chains")
        .field("version", TypeConstraint::AnyUnsigned)
        .field("friendship_id", TypeConstraint::AnyHash)
        .field("participant", TypeConstraint::AnyHash) // One per participant (handle_hash as hb)
        .field("chain", TypeConstraint::Wrapped(b'C')) // vC: CLUTCH chain (512×32) per participant
        // Hash chain state (v2)
        .field("last_sent_hash", TypeConstraint::AnyHash) // hp type: last msg_hp we sent
        .field("last_received_hash", TypeConstraint::AnyHash) // One per participant (hp or empty hb)
        // Pending messages (v2) - each message has 6 fields
        .field("pending_eagle_time", TypeConstraint::AnyFloat)
        .field("pending_plaintext", TypeConstraint::Utf8Text) // x: UTF-8 message text
        .field("pending_plaintext_hash", TypeConstraint::AnyHash) // hp
        .field("pending_prev_msg_hp", TypeConstraint::AnyHash) // hp
        .field("pending_msg_hp", TypeConstraint::AnyHash) // hp
        .field("pending_ciphertext", TypeConstraint::Wrapped(b'X')) // vX: ciphertext bytes
        // Bidirectional entropy state (v3)
        .field("last_received_weave", TypeConstraint::AnyHash) // hp: derived weave hash (32 bytes)
        .field("last_sent_weave", TypeConstraint::AnyHash) // hp: what we sent (what they received)
        .field("last_incorporated_hp", TypeConstraint::AnyHash) // hp: which of theirs we mixed in
        // Last plaintexts (v4) - needed for salt derivation after restart
        .field("last_plaintext", TypeConstraint::Utf8Text) // x: UTF-8 text, one per participant
        // Last received times (v5) - for duplicate detection after restart
        .field("last_received_time", TypeConstraint::AnyFloat) // f6: one per participant
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
        .set("version", 5u8) // v5: includes last_received_times for duplicate detection after restart
        .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?
        .set(
            "friendship_id",
            VsfType::hb(friendship_id.as_bytes().to_vec()),
        )
        .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?;

    // Add each participant's handle_hash and their chain (vC with 512×32 tensor data)
    for participant in chains.participants() {
        builder = builder
            .append_multi("participant", vec![VsfType::hb(participant.to_vec())])
            .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?;

        // Get this participant's chain as 512×32 tensor bytes
        let chain = chains
            .chain(participant)
            .ok_or_else(|| FriendshipStorageError::InvalidChains("Missing chain".to_string()))?;
        let chain_bytes = chain.to_bytes();

        // Store as vC (CLUTCH chain) - internally it's a 512×32 u8 tensor
        builder = builder
            .append_multi("chain", vec![VsfType::v(b'C', chain_bytes)])
            .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?;
    }

    // === Hash chain state (v2) ===

    // last_sent_hash - use hp (hash provenance) for immutable content ID
    if let Some(hash) = chains.last_sent_hash() {
        builder = builder
            .set("last_sent_hash", VsfType::hp(hash.to_vec()))
            .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?;
    }

    // last_received_hashes - one per participant (None serialized as empty hb)
    for hash_opt in chains.last_received_hashes() {
        let vsf_val = match hash_opt {
            Some(hash) => VsfType::hp(hash.to_vec()),
            None => VsfType::hb(Vec::new()), // Empty = no messages received yet (expect anchor)
        };
        builder = builder
            .append_multi("last_received_hash", vec![vsf_val])
            .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?;
    }

    // === Pending messages (v2) ===
    for pending in chains.pending_messages() {
        // Convert plaintext bytes to UTF-8 string for x type
        let plaintext_str = String::from_utf8_lossy(&pending.plaintext).into_owned();

        builder = builder
            .append_multi("pending_eagle_time", vec![VsfType::f6(pending.eagle_time)])
            .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?
            .append_multi("pending_plaintext", vec![VsfType::x(plaintext_str)])
            .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?
            .append_multi(
                "pending_plaintext_hash",
                vec![VsfType::hp(pending.plaintext_hash.to_vec())],
            )
            .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?
            .append_multi(
                "pending_prev_msg_hp",
                vec![VsfType::hp(pending.prev_msg_hp.to_vec())],
            )
            .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?
            .append_multi("pending_msg_hp", vec![VsfType::hp(pending.msg_hp.to_vec())])
            .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?
            .append_multi(
                "pending_ciphertext",
                vec![VsfType::v(b'X', pending.ciphertext.clone())],
            )
            .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?;
    }

    // === Bidirectional entropy state (v3) ===

    // last_received_weave - derived weave hash for mixing (32 bytes)
    if let Some(weave) = chains.last_received_weave() {
        builder = builder
            .set("last_received_weave", VsfType::hp(weave.to_vec()))
            .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?;
    }

    // last_sent_weave - what we sent (what they received) for their chain advancement
    if let Some(weave) = chains.last_sent_weave() {
        builder = builder
            .set("last_sent_weave", VsfType::hp(weave.to_vec()))
            .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?;
    }

    // last_incorporated_hp - which of their messages we mixed in
    if let Some(hp) = chains.last_incorporated_hp() {
        builder = builder
            .set("last_incorporated_hp", VsfType::hp(hp.to_vec()))
            .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?;
    }

    // === Last plaintexts (v4) - one per participant ===
    for plaintext in chains.last_plaintexts() {
        // Convert plaintext bytes to UTF-8 string for x type
        let plaintext_str = String::from_utf8_lossy(plaintext).into_owned();
        builder = builder
            .append_multi("last_plaintext", vec![VsfType::x(plaintext_str)])
            .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?;
    }

    // === Last received times (v5) - one per participant, for duplicate detection ===
    for time_opt in chains.last_received_times() {
        let time_val = time_opt.unwrap_or(0.0); // 0.0 means no messages received yet
        builder = builder
            .append_multi("last_received_time", vec![VsfType::f6(time_val)])
            .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?;
    }

    let vsf_bytes = builder
        .encode()
        .map_err(|e| FriendshipStorageError::Parse(e.to_string()))?;

    // Encrypt and write
    let encryption_key = chains_encryption_key(identity_seed, device_secret, friendship_id);
    let encrypted = encrypt_data(&vsf_bytes, &encryption_key)?;

    let path = dir.join("chains.vsf.enc");
    let label = format!(
        "friendships/{}/chains.vsf.enc",
        &friendship_id.to_base64()[..8]
    );
    crate::network::inspect::vsf_write(&path, &encrypted, &label, Some(&vsf_bytes), device_secret)?;

    Ok(())
}

/// Load FriendshipChains from disk
pub fn load_friendship_chains(
    friendship_id: &FriendshipId,
    identity_seed: &[u8; 32],
    device_secret: &[u8; 32],
) -> Result<FriendshipChains, FriendshipStorageError> {
    use crate::types::friendship::PendingMessage;

    let dir = friendship_dir(friendship_id);
    let path = dir.join("chains.vsf.enc");

    // Read encrypted file
    let label = format!(
        "friendships/{}/chains.vsf.enc",
        &friendship_id.to_base64()[..8]
    );
    let encrypted = crate::network::inspect::vsf_read(&path, &label, device_secret)?;

    // Decrypt
    let encryption_key = chains_encryption_key(identity_seed, device_secret, friendship_id);
    let vsf_bytes = decrypt_data(&encrypted, &encryption_key)?;

    #[cfg(feature = "development")]
    crate::network::inspect::vsf_read_decrypted(&vsf_bytes, &label);

    // Parse VSF
    let mut ptr = 0;
    let section = VsfSection::parse(&vsf_bytes, &mut ptr)
        .map_err(|e| FriendshipStorageError::Parse(format!("VSF parse: {}", e)))?;

    // Extract participants (handle hashes as hb)
    let mut participants: Vec<[u8; 32]> = Vec::new();
    for field in section.get_fields("participant") {
        if let Some(VsfType::hb(b)) = field.values.first() {
            if b.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(b);
                participants.push(arr);
            }
        }
    }

    if participants.is_empty() {
        return Err(FriendshipStorageError::InvalidChains(
            "No participants found".to_string(),
        ));
    }

    // Extract chain bytes (vC per participant, 512×32 = 16KB each)
    let mut chain_bytes = Vec::new();
    for field in section.get_fields("chain") {
        if let Some(VsfType::v(b'C', data)) = field.values.first() {
            chain_bytes.extend(data);
        }
    }
    if chain_bytes.is_empty() {
        return Err(FriendshipStorageError::InvalidChains(
            "Missing chain data".to_string(),
        ));
    }

    // === Hash chain state (v2) ===

    // last_sent_hash - optional (None if not present or never sent)
    let last_sent_hash: Option<[u8; 32]> = section
        .get_field("last_sent_hash")
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::hp(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(bytes);
                Some(arr)
            }
            _ => None,
        });

    // last_received_hashes - one per participant (empty hb = None/anchor expected)
    let mut last_received_hashes: Vec<Option<[u8; 32]>> = Vec::new();
    for field in section.get_fields("last_received_hash") {
        if let Some(v) = field.values.first() {
            let hash_opt = match v {
                VsfType::hp(bytes) if bytes.len() == 32 => {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(bytes);
                    Some(arr)
                }
                VsfType::hb(bytes) if bytes.is_empty() => None,
                _ => None,
            };
            last_received_hashes.push(hash_opt);
        }
    }

    // === Pending messages (v2) ===
    let eagle_times: Vec<f64> = section
        .get_fields("pending_eagle_time")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::f6(t) => Some(*t),
            _ => None,
        })
        .collect();

    let plaintexts: Vec<Vec<u8>> = section
        .get_fields("pending_plaintext")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::x(s) => Some(s.as_bytes().to_vec()),
            _ => None,
        })
        .collect();

    let plaintext_hashes: Vec<[u8; 32]> = section
        .get_fields("pending_plaintext_hash")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::hp(b) if b.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(b);
                Some(arr)
            }
            _ => None,
        })
        .collect();

    let prev_msg_hps: Vec<[u8; 32]> = section
        .get_fields("pending_prev_msg_hp")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::hp(b) if b.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(b);
                Some(arr)
            }
            _ => None,
        })
        .collect();

    let msg_hps: Vec<[u8; 32]> = section
        .get_fields("pending_msg_hp")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::hp(b) if b.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(b);
                Some(arr)
            }
            _ => None,
        })
        .collect();

    let ciphertexts: Vec<Vec<u8>> = section
        .get_fields("pending_ciphertext")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::v(b'X', data) => Some(data.clone()),
            _ => None,
        })
        .collect();

    // Reconstruct pending messages (all arrays must have same length)
    let pending_count = eagle_times
        .len()
        .min(plaintexts.len())
        .min(plaintext_hashes.len())
        .min(prev_msg_hps.len())
        .min(msg_hps.len())
        .min(ciphertexts.len());

    let pending_messages: Vec<PendingMessage> = (0..pending_count)
        .map(|i| PendingMessage {
            eagle_time: eagle_times[i],
            plaintext: plaintexts[i].clone(),
            plaintext_hash: plaintext_hashes[i],
            prev_msg_hp: prev_msg_hps[i],
            msg_hp: msg_hps[i],
            ciphertext: ciphertexts[i].clone(),
        })
        .collect();

    // === Bidirectional entropy state (v3) ===

    // last_received_weave - derived weave hash for mixing (32 bytes)
    let last_received_weave: Option<[u8; 32]> = section
        .get_field("last_received_weave")
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::hp(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(bytes);
                Some(arr)
            }
            _ => None,
        });

    // last_sent_weave - what we sent (what they received)
    let last_sent_weave: Option<[u8; 32]> = section
        .get_field("last_sent_weave")
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::hp(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(bytes);
                Some(arr)
            }
            _ => None,
        });

    // last_incorporated_hp - which of their messages we mixed in
    let last_incorporated_hp: Option<[u8; 32]> = section
        .get_field("last_incorporated_hp")
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::hp(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(bytes);
                Some(arr)
            }
            _ => None,
        });

    // === Last plaintexts (v4) - one per participant ===
    let last_plaintexts: Vec<Vec<u8>> = section
        .get_fields("last_plaintext")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::x(s) => Some(s.as_bytes().to_vec()),
            _ => None,
        })
        .collect();

    // === Last received times (v5) - one per participant ===
    let last_received_times: Vec<Option<f64>> = section
        .get_fields("last_received_time")
        .iter()
        .filter_map(|f| f.values.first())
        .map(|v| match v {
            VsfType::f6(t) if *t == 0.0 => None, // 0.0 means no messages received yet
            VsfType::f6(t) => Some(*t),
            _ => None,
        })
        .collect();

    // Reconstruct chains with full v5 state
    FriendshipChains::from_storage_v5(
        *friendship_id,
        participants,
        &chain_bytes,
        last_sent_hash,
        last_received_hashes,
        pending_messages,
        last_received_weave,
        last_sent_weave,
        last_incorporated_hp,
        last_plaintexts,
        last_received_times,
    )
    .ok_or_else(|| {
        FriendshipStorageError::InvalidChains("Failed to reconstruct chains".to_string())
    })
}

/// Load all friendships from disk
pub fn load_all_friendships(
    identity_seed: &[u8; 32],
    device_secret: &[u8; 32],
) -> Vec<(FriendshipId, FriendshipChains)> {
    let mut result = Vec::new();
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
                                result.push((friendship_id, chains));
                            }
                            Err(e) => {
                                crate::log(&format!(
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

/// Delete friendship chains from disk (used on re-key)
pub fn delete_friendship_chains(
    friendship_id: &FriendshipId,
) -> Result<(), FriendshipStorageError> {
    let dir = friendship_dir(friendship_id);
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
        crate::log(&format!(
            "CLUTCH: Deleted old chains directory: {}",
            dir.display()
        ));
    }
    Ok(())
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
        let loaded = load_friendship_chains(chains.id(), &identity_seed, &device_secret).unwrap();

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
