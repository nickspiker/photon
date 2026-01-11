//! Cloud contact storage via FGTW blob endpoints.
//!
//! Stores encrypted contact list on FGTW so users can backup contacts.
//! Each device gets its own blob slot (key includes device_secret).
//!
//! Key derivation:
//! - Storage key: BLAKE3(identity_seed || device_secret || "contacts_storage_key_v0")
//! - Encryption key: BLAKE3(identity_seed || device_secret || "contacts_v0")
//!
//! Security:
//! - identity_seed = BLAKE3(VsfType::x(handle)) - private
//! - device_secret = Ed25519 signing key bytes - private
//! - handle_proof is PUBLIC - never use for encryption!

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use blake3::Hasher;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use vsf::schema::{SectionSchema, TypeConstraint};
use vsf::{VsfSection, VsfType};

use crate::types::{Contact, DevicePubkey, HandleText, TrustLevel};

/// Errors from cloud storage operations
#[derive(Debug)]
pub enum CloudError {
    Encryption(String),
    Decryption(String),
    Parse(String),
    Network(String),
}

impl std::fmt::Display for CloudError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CloudError::Encryption(s) => write!(f, "Encryption error: {}", s),
            CloudError::Decryption(s) => write!(f, "Decryption error: {}", s),
            CloudError::Parse(s) => write!(f, "Parse error: {}", s),
            CloudError::Network(s) => write!(f, "Network error: {}", s),
        }
    }
}

/// Contact data stored in cloud (minimal for recovery)
#[derive(Clone, Debug)]
pub struct CloudContact {
    pub handle_proof: [u8; 32],
    pub handle: String,
    pub device_pubkey: [u8; 32],
    pub trust_level: u8,
    pub added: f64,
}

impl From<&Contact> for CloudContact {
    fn from(c: &Contact) -> Self {
        CloudContact {
            handle_proof: c.handle_proof,
            handle: c.handle.as_str().to_string(),
            device_pubkey: *c.public_identity.as_bytes(),
            trust_level: trust_level_to_u8(c.trust_level),
            added: c.added,
        }
    }
}

/// Derive storage key for contacts blob on FGTW
/// Returns base64url-encoded 32-byte hash (43 chars)
pub fn contacts_storage_key(identity_seed: &[u8; 32], device_secret: &[u8; 32]) -> String {
    let mut hasher = Hasher::new();
    hasher.update(identity_seed);
    hasher.update(device_secret);
    hasher.update(b"contacts_storage_key_v0");
    let hash = hasher.finalize();
    URL_SAFE_NO_PAD.encode(hash.as_bytes())
}

/// Derive encryption key for contacts blob
pub fn contacts_encryption_key(identity_seed: &[u8; 32], device_secret: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(identity_seed);
    hasher.update(device_secret);
    hasher.update(b"contacts_v0");
    *hasher.finalize().as_bytes()
}

/// Schema for cloud_contacts section
fn cloud_contacts_schema() -> SectionSchema {
    SectionSchema::new("cloud_contacts")
        .field("version", TypeConstraint::AnyUnsigned)
        .field("contact", TypeConstraint::Any) // Mixed types per contact
}

/// Encode contacts to encrypted VSF blob
pub fn encode_contacts(
    contacts: &[CloudContact],
    encryption_key: &[u8; 32],
) -> Result<Vec<u8>, CloudError> {
    let schema = cloud_contacts_schema();
    let mut builder = schema
        .build()
        .set("version", 0u8)
        .map_err(|e| CloudError::Parse(e.to_string()))?;

    for c in contacts {
        builder = builder
            .append_multi(
                "contact",
                vec![
                    VsfType::hP(c.handle_proof.to_vec()),
                    VsfType::x(c.handle.clone()),
                    VsfType::ke(c.device_pubkey.to_vec()),
                    VsfType::u3(c.trust_level),
                    VsfType::e(vsf::types::EtType::f6(c.added)),
                ],
            )
            .map_err(|e| CloudError::Parse(e.to_string()))?;
    }

    let vsf_bytes = builder
        .encode()
        .map_err(|e| CloudError::Parse(e.to_string()))?;

    encrypt_data(&vsf_bytes, encryption_key)
}

/// Decode contacts from encrypted VSF blob
pub fn decode_contacts(
    encrypted: &[u8],
    encryption_key: &[u8; 32],
) -> Result<Vec<CloudContact>, CloudError> {
    let vsf_bytes = decrypt_data(encrypted, encryption_key)?;

    let mut ptr = 0;
    let section = VsfSection::parse(&vsf_bytes, &mut ptr)
        .map_err(|e| CloudError::Parse(format!("VSF parse: {}", e)))?;

    let mut contacts = Vec::new();
    for field in section.get_fields("contact") {
        if field.values.len() >= 5 {
            let handle_proof: [u8; 32] = match &field.values[0] {
                VsfType::hP(v) if v.len() == 32 => v.as_slice().try_into().unwrap(),
                _ => continue,
            };
            let handle = match &field.values[1] {
                VsfType::x(s) => s.clone(),
                _ => continue,
            };
            let device_pubkey: [u8; 32] = match &field.values[2] {
                VsfType::ke(v) if v.len() == 32 => v.as_slice().try_into().unwrap(),
                _ => continue,
            };
            let trust_level = match &field.values[3] {
                VsfType::u3(v) => *v,
                _ => 0,
            };
            let added = match &field.values[4] {
                VsfType::e(et) => vsf::EagleTime::new_from_vsf(VsfType::e(et.clone())).to_f64(),
                VsfType::f6(v) => *v, // Legacy
                _ => 0.0,
            };

            contacts.push(CloudContact {
                handle_proof,
                handle,
                device_pubkey,
                trust_level,
                added,
            });
        }
    }

    Ok(contacts)
}

/// Convert CloudContact back to Contact for local storage
impl CloudContact {
    pub fn to_contact(&self) -> Contact {
        let mut contact = Contact::new(
            HandleText::new(&self.handle),
            self.handle_proof,
            DevicePubkey::from_bytes(self.device_pubkey),
        );
        contact.trust_level = u8_to_trust_level(self.trust_level);
        contact.added = self.added;
        contact
    }
}

/// Encrypt data with ChaCha20-Poly1305
fn encrypt_data(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, CloudError> {
    let cipher =
        ChaCha20Poly1305::new_from_slice(key).map_err(|e| CloudError::Encryption(e.to_string()))?;

    let mut nonce_bytes = [0u8; 12];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut nonce_bytes);
    let nonce: Nonce = nonce_bytes.into();

    let ciphertext = cipher
        .encrypt(&nonce, data)
        .map_err(|e| CloudError::Encryption(e.to_string()))?;

    // Format: [12-byte nonce][ciphertext with 16-byte auth tag]
    let mut result = Vec::with_capacity(12 + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

/// Decrypt data with ChaCha20-Poly1305
fn decrypt_data(encrypted: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, CloudError> {
    if encrypted.len() < 12 + 16 {
        return Err(CloudError::Decryption("Data too short".to_string()));
    }

    let cipher =
        ChaCha20Poly1305::new_from_slice(key).map_err(|e| CloudError::Decryption(e.to_string()))?;

    let nonce_bytes: [u8; 12] = encrypted[..12]
        .try_into()
        .map_err(|_| CloudError::Decryption("Invalid nonce".to_string()))?;
    let nonce: Nonce = nonce_bytes.into();
    let ciphertext = &encrypted[12..];

    cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|e| CloudError::Decryption(e.to_string()))
}

fn trust_level_to_u8(level: TrustLevel) -> u8 {
    match level {
        TrustLevel::Stranger => 0,
        TrustLevel::Known => 1,
        TrustLevel::Trusted => 2,
        TrustLevel::Inner => 3,
    }
}

fn u8_to_trust_level(v: u8) -> TrustLevel {
    match v {
        0 => TrustLevel::Stranger,
        1 => TrustLevel::Known,
        2 => TrustLevel::Trusted,
        3 => TrustLevel::Inner,
        _ => TrustLevel::Stranger,
    }
}

// ============================================================================
// High-Level API (Blocking)
// ============================================================================

/// Sync contacts to FGTW cloud storage (blocking)
///
/// Uploads current contacts to cloud. Call after any contact change.
///
/// # Arguments
/// * `contacts` - Current contacts list
/// * `identity_seed` - Our identity seed (BLAKE3 of VSF-normalized handle)
/// * `device_keypair` - Device Ed25519 keypair
/// * `handle_proof` - 32-byte handle proof (proves registered user)
pub fn sync_contacts_to_cloud(
    contacts: &[Contact],
    identity_seed: &[u8; 32],
    device_keypair: &crate::network::fgtw::Keypair,
    handle_proof: &[u8; 32],
) -> Result<(), CloudError> {
    use crate::network::fgtw::{put_blob_blocking, BlobError};

    // Convert contacts to cloud format
    let cloud_contacts: Vec<CloudContact> = contacts.iter().map(CloudContact::from).collect();

    // Derive keys
    let device_secret = device_keypair.secret.as_bytes();
    let storage_key = contacts_storage_key(identity_seed, device_secret);
    let encryption_key = contacts_encryption_key(identity_seed, device_secret);

    // Encode and encrypt
    let encrypted = encode_contacts(&cloud_contacts, &encryption_key)?;

    crate::log(&format!(
        "Cloud: Uploading {} contacts ({} bytes encrypted)",
        contacts.len(),
        encrypted.len()
    ));

    #[cfg(feature = "development")]
    crate::log("Cloud: About to call put_blob_blocking...");

    // Upload to FGTW
    put_blob_blocking(&storage_key, &encrypted, device_keypair, handle_proof).map_err(
        |e| match e {
            BlobError::Network(s) => CloudError::Network(s),
            BlobError::NotFound => CloudError::Network("Blob not found".to_string()),
            BlobError::Unauthorized(s) => CloudError::Encryption(s),
            BlobError::ServerError(s) => CloudError::Network(s),
        },
    )?;

    Ok(())
}

/// Load contacts from FGTW cloud storage (blocking)
///
/// Downloads and decrypts contacts from cloud.
///
/// # Returns
/// * `Ok(Some(contacts))` - Contacts found and decrypted
/// * `Ok(None)` - No contacts blob exists on cloud
/// * `Err(...)` - Error
pub fn load_contacts_from_cloud(
    identity_seed: &[u8; 32],
    device_keypair: &crate::network::fgtw::Keypair,
) -> Result<Option<Vec<CloudContact>>, CloudError> {
    use crate::network::fgtw::{get_blob_blocking, BlobError};

    // Derive keys
    let device_secret = device_keypair.secret.as_bytes();
    let storage_key = contacts_storage_key(identity_seed, device_secret);
    let encryption_key = contacts_encryption_key(identity_seed, device_secret);

    // Download from FGTW
    let encrypted = match get_blob_blocking(&storage_key) {
        Ok(Some(data)) => data,
        Ok(None) => return Ok(None),
        Err(e) => {
            return Err(match e {
                BlobError::Network(s) => CloudError::Network(s),
                BlobError::NotFound => return Ok(None),
                BlobError::Unauthorized(s) => CloudError::Decryption(s),
                BlobError::ServerError(s) => CloudError::Network(s),
            })
        }
    };

    crate::log(&format!(
        "Cloud: Downloaded contacts blob ({} bytes)",
        encrypted.len()
    ));

    // Decrypt and decode
    let contacts = decode_contacts(&encrypted, &encryption_key)?;
    crate::log(&format!("Cloud: Decoded {} contacts", contacts.len()));
    Ok(Some(contacts))
}

// ============================================================================
// High-Level API (Async)
// ============================================================================

/// Upload contacts to FGTW cloud storage (async version)
pub async fn upload_contacts_to_cloud(
    contacts: &[Contact],
    identity_seed: &[u8; 32],
    device_keypair: &crate::network::fgtw::Keypair,
    handle_proof: &[u8; 32],
) -> Result<(), CloudError> {
    use crate::network::fgtw::{put_blob, BlobError};

    // Convert contacts to cloud format
    let cloud_contacts: Vec<CloudContact> = contacts.iter().map(CloudContact::from).collect();

    // Derive keys
    let device_secret = device_keypair.secret.as_bytes();
    let storage_key = contacts_storage_key(identity_seed, device_secret);
    let encryption_key = contacts_encryption_key(identity_seed, device_secret);

    // Encode and encrypt
    let encrypted = encode_contacts(&cloud_contacts, &encryption_key)?;

    // Upload to FGTW
    put_blob(&storage_key, &encrypted, device_keypair, handle_proof)
        .await
        .map_err(|e| match e {
            BlobError::Network(s) => CloudError::Network(s),
            BlobError::NotFound => CloudError::Network("Blob not found".to_string()),
            BlobError::Unauthorized(s) => CloudError::Encryption(s),
            BlobError::ServerError(s) => CloudError::Network(s),
        })?;

    Ok(())
}

/// Download contacts from FGTW cloud storage (async version)
pub async fn download_contacts_from_cloud(
    identity_seed: &[u8; 32],
    device_keypair: &crate::network::fgtw::Keypair,
) -> Result<Option<Vec<CloudContact>>, CloudError> {
    use crate::network::fgtw::{get_blob, BlobError};

    // Derive keys
    let device_secret = device_keypair.secret.as_bytes();
    let storage_key = contacts_storage_key(identity_seed, device_secret);
    let encryption_key = contacts_encryption_key(identity_seed, device_secret);

    // Download from FGTW
    let encrypted = match get_blob(&storage_key).await {
        Ok(Some(data)) => data,
        Ok(None) => return Ok(None),
        Err(e) => {
            return Err(match e {
                BlobError::Network(s) => CloudError::Network(s),
                BlobError::NotFound => return Ok(None),
                BlobError::Unauthorized(s) => CloudError::Decryption(s),
                BlobError::ServerError(s) => CloudError::Network(s),
            })
        }
    };

    // Decrypt and decode
    let contacts = decode_contacts(&encrypted, &encryption_key)?;
    Ok(Some(contacts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_derivation() {
        let identity_seed = [1u8; 32];
        let device_secret = [2u8; 32];

        let storage_key = contacts_storage_key(&identity_seed, &device_secret);
        let encryption_key = contacts_encryption_key(&identity_seed, &device_secret);

        // Storage key should be 43 chars (base64url of 32 bytes)
        assert_eq!(storage_key.len(), 43);

        // Keys should be different (different purpose strings)
        let storage_hash = blake3::hash(storage_key.as_bytes());
        assert_ne!(storage_hash.as_bytes(), &encryption_key);
    }

    #[test]
    fn test_contacts_roundtrip() {
        let contacts = vec![
            CloudContact {
                handle_proof: [1u8; 32],
                handle: "alice".to_string(),
                device_pubkey: [2u8; 32],
                trust_level: 1,
                added: 1234567890.0,
            },
            CloudContact {
                handle_proof: [3u8; 32],
                handle: "bob".to_string(),
                device_pubkey: [4u8; 32],
                trust_level: 2,
                added: 1234567891.0,
            },
        ];

        let key = [42u8; 32];
        let encrypted = encode_contacts(&contacts, &key).unwrap();
        let decoded = decode_contacts(&encrypted, &key).unwrap();

        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].handle, "alice");
        assert_eq!(decoded[0].handle_proof, [1u8; 32]);
        assert_eq!(decoded[1].handle, "bob");
        assert_eq!(decoded[1].trust_level, 2);
    }

    #[test]
    fn test_wrong_key_fails() {
        let contacts = vec![CloudContact {
            handle_proof: [1u8; 32],
            handle: "alice".to_string(),
            device_pubkey: [2u8; 32],
            trust_level: 1,
            added: 1234567890.0,
        }];

        let key1 = [42u8; 32];
        let key2 = [43u8; 32];

        let encrypted = encode_contacts(&contacts, &key1).unwrap();
        let result = decode_contacts(&encrypted, &key2);

        assert!(result.is_err());
    }
}
