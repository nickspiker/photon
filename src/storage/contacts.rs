//! Contact persistence with encrypted VSF storage.
//!
//! Storage layout:
//! - ~/.config/photon/contacts/index.enc - contact list (identity data)
//! - ~/.config/photon/contacts/{identity_seed_hex}/state.vsf - mutable state
//! - ~/.config/photon/contacts/{identity_seed_hex}/{provenance}.vsf - messages
//! - ~/.config/photon/contacts/{identity_seed_hex}/avatar.vsf - avatar
//!
//! Index file format:
//! [contact_list
//!   (contact: handle_proof, handle)
//!   (contact: handle_proof, handle)
//!   ...
//! ]
//!
//! identity_seed is derived on the fly: BLAKE3(VsfType::x(handle).flatten())
//!
//! State file format:
//! [contact_state
//!   (clutch_state: u8)
//!   (trust_level: u8)
//!   (pubkey: ke)
//!   (seed: hb)  // optional, after CLUTCH
//!   (ephemeral_secret: hb)  // optional, during CLUTCH
//!   (ephemeral_pubkey: kx)  // optional
//!   (ephemeral_their: kx)   // optional
//!   (last_seen: u6)  // optional
//!   (ip: d)  // optional
//! ]
//!
//! Encrypted with ChaCha20-Poly1305 using key derived from our identity_seed.

use crate::types::{ClutchState, Contact, ContactId, DevicePubkey, HandleText, Seed, TrustLevel};
use blake3::Hasher;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use std::fs;
use std::path::PathBuf;
use vsf::schema::{SectionBuilder, SectionSchema, TypeConstraint};
use vsf::types::EagleTime;
use vsf::{VsfSection, VsfType};

/// Errors from contact storage operations
#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    Encryption(String),
    Decryption(String),
    Parse(String),
    NoValidSlot,
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Io(e) => write!(f, "IO error: {}", e),
            StorageError::Encryption(s) => write!(f, "Encryption error: {}", s),
            StorageError::Decryption(s) => write!(f, "Decryption error: {}", s),
            StorageError::Parse(s) => write!(f, "Parse error: {}", s),
            StorageError::NoValidSlot => write!(f, "No valid storage slot found"),
        }
    }
}

impl From<std::io::Error> for StorageError {
    fn from(e: std::io::Error) -> Self {
        StorageError::Io(e)
    }
}

/// Static identity data stored in the contact list index
#[derive(Clone, Debug)]
pub struct ContactIdentity {
    pub handle_proof: [u8; 32],
    pub handle: String,
}

impl ContactIdentity {
    /// Derive identity_seed from handle using VSF normalization
    /// This ensures consistent key derivation regardless of Unicode representation
    pub fn identity_seed(&self) -> [u8; 32] {
        derive_identity_seed(&self.handle)
    }
}

/// Derive identity_seed from a handle string using VSF normalization
/// Formula: BLAKE3(VsfType::x(handle).flatten())
pub fn derive_identity_seed(handle: &str) -> [u8; 32] {
    let vsf_bytes = VsfType::x(handle.to_string()).flatten();
    *blake3::hash(&vsf_bytes).as_bytes()
}

/// Get the base photon config directory
fn photon_config_dir() -> Result<PathBuf, StorageError> {
    #[cfg(target_os = "android")]
    let base_dir = {
        crate::ui::avatar::get_android_data_dir().ok_or_else(|| {
            StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Android data dir not set",
            ))
        })?
    };

    #[cfg(not(target_os = "android"))]
    let base_dir = dirs::config_dir()
        .ok_or_else(|| {
            StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No config dir",
            ))
        })?
        .join("photon");

    Ok(base_dir)
}

/// Get the contacts directory, creating if needed
fn contacts_dir() -> Result<PathBuf, StorageError> {
    let contacts_dir = photon_config_dir()?.join("contacts");
    fs::create_dir_all(&contacts_dir)?;
    Ok(contacts_dir)
}

/// Get a specific contact's directory using identity_seed
fn contact_dir_from_seed(identity_seed: &[u8; 32]) -> Result<PathBuf, StorageError> {
    let dir_name = hex::encode(&identity_seed[..8]); // 16 hex chars
    let dir = contacts_dir()?.join(dir_name);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Encode provenance hash to URL-safe base64 for filename
#[cfg(test)]
fn provenance_to_filename(provenance: &[u8; 32]) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.encode(provenance)
}

/// Derive encryption key for contact list index
/// Requires device_secret so only this device can decrypt
fn derive_list_key(our_identity_seed: &[u8; 32], device_secret: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(b"photon_contact_list_v3"); // v3: now includes device_secret
    hasher.update(our_identity_seed);
    hasher.update(device_secret);
    *hasher.finalize().as_bytes()
}

/// Derive encryption key for per-contact state
/// Requires device_secret so only this device can decrypt
fn derive_state_key(
    our_identity_seed: &[u8; 32],
    their_identity_seed: &[u8; 32],
    device_secret: &[u8; 32],
) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(b"photon_contact_state_v3"); // v3: now includes device_secret
    hasher.update(our_identity_seed);
    hasher.update(their_identity_seed);
    hasher.update(device_secret);
    *hasher.finalize().as_bytes()
}

/// Encrypt VSF bytes with ChaCha20-Poly1305
fn encrypt_data(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, StorageError> {
    let cipher = ChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| StorageError::Encryption(e.to_string()))?;

    let mut nonce_bytes = [0u8; 12];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut nonce_bytes);
    let nonce: Nonce = nonce_bytes.into();

    let ciphertext = cipher
        .encrypt(&nonce, data)
        .map_err(|e| StorageError::Encryption(e.to_string()))?;

    // Format: [12-byte nonce][ciphertext with 16-byte auth tag]
    let mut result = Vec::with_capacity(12 + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

/// Decrypt data
fn decrypt_data(encrypted: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, StorageError> {
    if encrypted.len() < 12 + 16 {
        return Err(StorageError::Decryption("File too short".to_string()));
    }

    let cipher = ChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| StorageError::Decryption(e.to_string()))?;

    let nonce_bytes: [u8; 12] = encrypted[..12]
        .try_into()
        .map_err(|_| StorageError::Decryption("Invalid nonce length".to_string()))?;
    let nonce: Nonce = nonce_bytes.into();
    let ciphertext = &encrypted[12..];

    cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|e| StorageError::Decryption(e.to_string()))
}

// ============================================================================
// Contact List (Index) - Static Identity Data (Schema-validated)
// ============================================================================

/// Schema for contact_list section
/// Each contact field contains: (handle_proof: hb, handle: x)
fn contact_list_schema() -> SectionSchema {
    SectionSchema::new("contact_list")
        // Contact field allows mixed types (hash, string) - use Any
        .field("contact", TypeConstraint::Any)
}

/// Save the contact list to encrypted index file with schema validation
pub fn save_contact_list(
    contacts: &[ContactIdentity],
    our_identity_seed: &[u8; 32],
    device_secret: &[u8; 32],
) -> Result<(), StorageError> {
    let index_path = contacts_dir()?.join("index.enc");

    let schema = contact_list_schema();
    let mut builder = schema.build();

    for c in contacts {
        builder = builder
            .append_multi(
                "contact",
                vec![
                    VsfType::hb(c.handle_proof.to_vec()),
                    VsfType::x(c.handle.clone()),
                ],
            )
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }

    let vsf_bytes = builder
        .encode()
        .map_err(|e| StorageError::Parse(e.to_string()))?;
    let key = derive_list_key(our_identity_seed, device_secret);
    let encrypted = encrypt_data(&vsf_bytes, &key)?;

    fs::write(&index_path, &encrypted)?;
    Ok(())
}

/// Load the contact list from encrypted index file with schema validation
pub fn load_contact_list(
    our_identity_seed: &[u8; 32],
    device_secret: &[u8; 32],
) -> Result<Vec<ContactIdentity>, StorageError> {
    let index_path = contacts_dir()?.join("index.enc");

    if !index_path.exists() {
        return Ok(Vec::new());
    }

    let encrypted = fs::read(&index_path)?;
    let key = derive_list_key(our_identity_seed, device_secret);
    let vsf_bytes = decrypt_data(&encrypted, &key)?;

    let schema = contact_list_schema();
    let builder = SectionBuilder::parse(schema, &vsf_bytes)
        .map_err(|e| StorageError::Parse(format!("Contact list parse: {}", e)))?;

    let mut contacts = Vec::new();
    for field in builder.get_fields("contact") {
        if field.values.len() >= 2 {
            let handle_proof: [u8; 32] = match &field.values[0] {
                VsfType::hb(v) if v.len() == 32 => v.as_slice().try_into().unwrap(),
                _ => continue,
            };
            let handle = match &field.values[1] {
                VsfType::x(s) => s.clone(),
                _ => continue,
            };

            contacts.push(ContactIdentity {
                handle_proof,
                handle,
            });
        }
    }

    Ok(contacts)
}

// ============================================================================
// Contact State - Mutable Session Data (Schema-validated)
// ============================================================================

/// Schema for contact_state section
fn contact_state_schema() -> SectionSchema {
    SectionSchema::new("contact_state")
        .field("clutch_state", TypeConstraint::AnyUnsigned)
        .field("trust_level", TypeConstraint::AnyUnsigned)
        .field("pubkey", TypeConstraint::Ed25519Key)
        .field("added_timestamp", TypeConstraint::Any) // f64 Eagle Time
        .field("contact_id", TypeConstraint::AnyHash)
        // Optional fields
        .field("ip", TypeConstraint::AnyString)
        .field("seed", TypeConstraint::AnyHash)
        .field("ephemeral_secret", TypeConstraint::AnyHash)
        .field("ephemeral_pubkey", TypeConstraint::X25519Key)
        .field("ephemeral_their", TypeConstraint::X25519Key)
        .field("last_seen", TypeConstraint::Any) // f64 Eagle Time
}

/// Save contact state (mutable data) to per-contact file with schema validation
pub fn save_contact_state(
    contact: &Contact,
    our_identity_seed: &[u8; 32],
    device_secret: &[u8; 32],
) -> Result<(), StorageError> {
    let identity_seed = derive_identity_seed(contact.handle.as_str());
    let dir = contact_dir_from_seed(&identity_seed)?;
    let state_path = dir.join("state.vsf");

    let schema = contact_state_schema();
    let mut builder = schema
        .build()
        .set("clutch_state", clutch_state_to_u8(contact.clutch_state))
        .map_err(|e| StorageError::Parse(e.to_string()))?
        .set("trust_level", trust_level_to_u8(contact.trust_level))
        .map_err(|e| StorageError::Parse(e.to_string()))?
        .set(
            "pubkey",
            contact.public_identity.to_vsf(), // Ed25519 (ke)
        )
        .map_err(|e| StorageError::Parse(e.to_string()))?
        .set("added_timestamp", VsfType::f6(contact.added_timestamp))
        .map_err(|e| StorageError::Parse(e.to_string()))?
        .set("contact_id", VsfType::hb(contact.id.as_bytes().to_vec()))
        .map_err(|e| StorageError::Parse(e.to_string()))?;

    // Optional fields
    if let Some(ip) = &contact.ip {
        builder = builder
            .set("ip", ip.to_string())
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    if let Some(seed) = &contact.relationship_seed {
        builder = builder
            .set("seed", VsfType::hb(seed.as_bytes().to_vec()))
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    if let Some(secret) = &contact.clutch_our_ephemeral_secret {
        builder = builder
            .set("ephemeral_secret", VsfType::hb(secret.to_vec()))
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    if let Some(pubkey) = &contact.clutch_our_ephemeral_pubkey {
        builder = builder
            .set("ephemeral_pubkey", VsfType::kx(pubkey.to_vec()))
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    if let Some(pubkey) = &contact.clutch_their_ephemeral_pubkey {
        builder = builder
            .set("ephemeral_their", VsfType::kx(pubkey.to_vec()))
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    if let Some(last_seen) = contact.last_seen {
        builder = builder
            .set("last_seen", VsfType::f6(last_seen))
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }

    let vsf_bytes = builder
        .encode()
        .map_err(|e| StorageError::Parse(e.to_string()))?;
    let key = derive_state_key(our_identity_seed, &identity_seed, device_secret);
    let encrypted = encrypt_data(&vsf_bytes, &key)?;

    fs::write(&state_path, &encrypted)?;
    Ok(())
}

/// Load contact state from per-contact file
pub fn load_contact_state(
    identity: &ContactIdentity,
    our_identity_seed: &[u8; 32],
    device_secret: &[u8; 32],
) -> Result<Contact, StorageError> {
    let their_identity_seed = identity.identity_seed();
    let dir = contact_dir_from_seed(&their_identity_seed)?;
    let state_path = dir.join("state.vsf");

    if !state_path.exists() {
        // No state file yet - return contact with just identity info
        let pubkey = DevicePubkey::from_bytes([0u8; 32]); // placeholder
        let contact = Contact::new(
            HandleText::new(&identity.handle),
            identity.handle_proof,
            pubkey,
        );
        return Ok(contact);
    }

    let encrypted = fs::read(&state_path)?;
    let key = derive_state_key(our_identity_seed, &their_identity_seed, device_secret);
    let vsf_bytes = decrypt_data(&encrypted, &key)?;

    let mut ptr = 0;
    let section = VsfSection::parse(&vsf_bytes, &mut ptr)
        .map_err(|e| StorageError::Parse(format!("Contact state parse: {}", e)))?;

    // Helper to get first value from field
    let get_val = |name: &str| -> Option<&VsfType> { section.get_field(name)?.values.first() };

    // Required fields
    let clutch_u8 = match get_val("clutch_state") {
        Some(VsfType::u3(v)) => *v,
        _ => 0,
    };
    let trust_u8 = match get_val("trust_level") {
        Some(VsfType::u3(v)) => *v,
        _ => 0,
    };
    let pubkey_bytes: [u8; 32] = match get_val("pubkey") {
        Some(VsfType::ke(v)) if v.len() == 32 => v.as_slice().try_into().unwrap(), // Ed25519
        _ => return Err(StorageError::Parse("Missing pubkey".into())),
    };
    let added_timestamp = match get_val("added_timestamp") {
        Some(v) => EagleTime::new_from_vsf(v.clone()).to_f64(),
        None => 0.0,
    };

    let pubkey = DevicePubkey::from_bytes(pubkey_bytes);
    let mut contact = Contact::new(
        HandleText::new(&identity.handle),
        identity.handle_proof,
        pubkey,
    );

    contact.clutch_state = u8_to_clutch_state(clutch_u8);
    contact.trust_level = u8_to_trust_level(trust_u8);
    contact.added_timestamp = added_timestamp;

    // Optional fields
    if let Some(VsfType::x(s) | VsfType::l(s) | VsfType::d(s)) = get_val("ip") {
        contact.ip = s.parse().ok();
    }
    if let Some(VsfType::hb(v)) = get_val("seed") {
        if v.len() == 32 {
            contact.relationship_seed = Some(Seed::from_bytes(v.as_slice().try_into().unwrap()));
        }
    }
    if let Some(VsfType::hb(v)) = get_val("ephemeral_secret") {
        if v.len() == 32 {
            contact.clutch_our_ephemeral_secret = Some(v.as_slice().try_into().unwrap());
        }
    }
    if let Some(VsfType::kx(v)) = get_val("ephemeral_pubkey") {
        if v.len() == 32 {
            contact.clutch_our_ephemeral_pubkey = Some(v.as_slice().try_into().unwrap());
        }
    }
    if let Some(VsfType::kx(v)) = get_val("ephemeral_their") {
        if v.len() == 32 {
            contact.clutch_their_ephemeral_pubkey = Some(v.as_slice().try_into().unwrap());
        }
    }
    if let Some(v) = get_val("last_seen") {
        contact.last_seen = Some(EagleTime::new_from_vsf(v.clone()).to_f64());
    }
    if let Some(VsfType::hb(v)) = get_val("contact_id") {
        if v.len() == 16 {
            contact.id = ContactId::from_bytes(v.as_slice().try_into().unwrap());
        }
    }

    Ok(contact)
}

// ============================================================================
// High-Level API
// ============================================================================

/// Save a contact (updates both list and state)
pub fn save_contact(
    contact: &Contact,
    our_identity_seed: &[u8; 32],
    device_secret: &[u8; 32],
) -> Result<(), StorageError> {
    // Save state file
    save_contact_state(contact, our_identity_seed, device_secret)?;

    // Update contact list
    let mut list = load_contact_list(our_identity_seed, device_secret).unwrap_or_default();

    // Check if contact already exists in list (by handle)
    let exists = list.iter().any(|c| c.handle == contact.handle.as_str());

    if !exists {
        list.push(ContactIdentity {
            handle_proof: contact.handle_proof,
            handle: contact.handle.as_str().to_string(),
        });
        save_contact_list(&list, our_identity_seed, device_secret)?;
    }

    Ok(())
}

/// Load all contacts from disk
pub fn load_all_contacts(our_identity_seed: &[u8; 32], device_secret: &[u8; 32]) -> Vec<Contact> {
    let identities = match load_contact_list(our_identity_seed, device_secret) {
        Ok(list) => list,
        Err(e) => {
            crate::log_error(&format!("Failed to load contact list: {}", e));
            return Vec::new();
        }
    };

    let mut contacts = Vec::new();
    for identity in identities {
        match load_contact_state(&identity, our_identity_seed, device_secret) {
            Ok(contact) => contacts.push(contact),
            Err(e) => {
                crate::log_error(&format!(
                    "Failed to load contact state for '{}': {}",
                    identity.handle, e
                ));
            }
        }
    }
    contacts
}

/// Delete contact from disk
pub fn delete_contact(identity_seed: &[u8; 32]) -> Result<(), StorageError> {
    let dir = contact_dir_from_seed(identity_seed)?;
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

fn clutch_state_to_u8(state: ClutchState) -> u8 {
    match state {
        ClutchState::Pending => 0,
        ClutchState::KeysGenerated => 1,
        ClutchState::OfferSent => 2,
        ClutchState::OfferReceived => 3,
        ClutchState::OffersExchanged => 4,
        ClutchState::KemSent => 5,
        ClutchState::KemReceived => 6,
        ClutchState::Complete => 7,
    }
}

fn u8_to_clutch_state(v: u8) -> ClutchState {
    match v {
        0 => ClutchState::Pending,
        1 => ClutchState::KeysGenerated,
        2 => ClutchState::OfferSent,
        3 => ClutchState::OfferReceived,
        4 => ClutchState::OffersExchanged,
        5 => ClutchState::KemSent,
        6 => ClutchState::KemReceived,
        7 => ClutchState::Complete,
        _ => ClutchState::Pending,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contact_identity_roundtrip() {
        let identity = ContactIdentity {
            handle_proof: [1u8; 32],
            handle: "alice".to_string(),
        };

        // Build section
        let mut section = VsfSection::new("contact_list");
        section.add_field_multi(
            "contact",
            vec![
                VsfType::hb(identity.handle_proof.to_vec()),
                VsfType::x(identity.handle.clone()),
            ],
        );

        let encoded = section.encode();

        // Parse back
        let mut ptr = 0;
        let parsed = VsfSection::parse(&encoded, &mut ptr).unwrap();

        let fields = parsed.get_fields("contact");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].values.len(), 2);

        let proof: [u8; 32] = match &fields[0].values[0] {
            VsfType::hb(v) if v.len() == 32 => v.as_slice().try_into().unwrap(),
            _ => panic!("Expected hb"),
        };
        let handle = match &fields[0].values[1] {
            VsfType::x(s) => s.clone(),
            _ => panic!("Expected x"),
        };

        assert_eq!(proof, identity.handle_proof);
        assert_eq!(handle, identity.handle);

        // Verify identity_seed is derived correctly
        let derived_seed = identity.identity_seed();
        let expected_seed = derive_identity_seed(&identity.handle);
        assert_eq!(derived_seed, expected_seed);
    }

    #[test]
    fn test_provenance_to_filename() {
        let provenance = [0x42u8; 32];
        let filename = provenance_to_filename(&provenance);

        assert_eq!(filename.len(), 43);
        assert!(!filename.contains('/'));
        assert!(!filename.contains('+'));
    }
}
