//! Cloud contact storage via FGTW blob endpoints.
//!
//! Stores encrypted contact list on FGTW so users can backup contacts. Each device gets its own blob slot (key includes device_secret).
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
use vsf::schema::{SectionSchema, TypeConstraint};
use vsf::{VsfSection, VsfType};

use crate::storage::{decrypt_bytes, encrypt_bytes};
use crate::types::{Contact, DevicePubkey, TrustLevel};

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

/// Contact data stored in cloud (minimal for recovery) — the PIN-SET, never a handle string (docs/identity-profile.md).
/// PartialEq is what lets the attest path tell "the cloud already has exactly this" from a real change, so it can skip a no-op upload. It has to compare CONTENT: the encrypted blob carries a fresh random nonce every time, so identical contacts never produce identical bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloudContact {
    pub handle_proof: [u8; 32],
    /// The pinned identity pubkey (party id).
    pub party_id: [u8; 32],
    /// The pinned avatar-wall material: AES key ‖ lookup hash (zero = unpinned).
    pub avatar_pin: [u8; 64],
    /// Local petname (may be empty — renders as the keyed pseudonym).
    pub name: String,
    pub device_pubkey: [u8; 32],
    pub trust_level: u8,
    pub added: i64,
}

impl From<&Contact> for CloudContact {
    fn from(c: &Contact) -> Self {
        CloudContact {
            handle_proof: c.handle_proof,
            party_id: c.handle_hash,
            avatar_pin: c.avatar_pin,
            name: c.petname.clone(),
            device_pubkey: *c.public_identity.as_bytes(),
            trust_level: trust_level_to_u8(c.trust_level),
            added: c.added,
        }
    }
}

/// Derive storage key for contacts blob on FGTW Returns base64url-encoded 32-byte hash (43 chars)
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
/// The section name, shared by the builder and the TOC lookup in `decode_contacts` — `parse_document` finds the section by matching this against the header, so the two must never drift.
const CLOUD_CONTACTS_SECTION: &str = "cloud_contacts";

fn cloud_contacts_schema() -> SectionSchema {
    SectionSchema::new(CLOUD_CONTACTS_SECTION)
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
                    VsfType::ke(c.party_id.to_vec()),
                    VsfType::ge(c.avatar_pin.to_vec()),
                    VsfType::x(c.name.clone()),
                    VsfType::ke(c.device_pubkey.to_vec()),
                    VsfType::u3(c.trust_level),
                    VsfType::e(vsf::types::EtType::e6(c.added)),
                ],
            )
            .map_err(|e| CloudError::Parse(e.to_string()))?;
    }

    let section_bytes = builder
        .encode()
        .map_err(|e| CloudError::Parse(e.to_string()))?;

    // A DOCUMENT, not a bare section. `.encode()` emits the section body ALONE — no header, so no creation time, no TOC, no BLAKE3 provenance hash, and nothing `read_verified`/`parse_document` can check. That is why the decode side had to hand-roll `VsfSection::parse`, and why the attest path had no way to ask "is this the same blob I already have?" (it fell back to re-uploading unconditionally). The crate does all of this for us; we just have to use it.
    // `provenance_only` (not `signed_only`): the blob is sealed under a key derived from the identity seed, so possession already proves the reader is us. The header's BLAKE3 provenance hash is the integrity anchor here. A device signature would additionally mean every device syncing the same contacts had to be enrolled as a distinct signer — which is not what this blob is for.
    let vsf_bytes = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_unboxed(CLOUD_CONTACTS_SECTION, section_bytes)
        .build()
        .map_err(|e| CloudError::Parse(e.to_string()))?;

    encrypt_data(&vsf_bytes, encryption_key)
}

/// Decode contacts from encrypted VSF blob
pub fn decode_contacts(
    encrypted: &[u8],
    encryption_key: &[u8; 32],
) -> Result<Vec<CloudContact>, CloudError> {
    let vsf_bytes = decrypt_data(encrypted, encryption_key)?;

    // Verified read: `parse_document` runs `read_verified` internally (header decode + provenance self-consistency), so a tampered or truncated blob is rejected before any field is trusted — the hand-rolled `VsfSection::parse` this replaces checked nothing.
    // LEGACY FALLBACK: blobs written before the document wrapper landed are a BARE SECTION with no header, so `parse_document` can't find a TOC and fails. Fall back to the raw section parse for those; the next `sync_contacts_to_cloud` rewrites the blob in document form and the fallback stops being reachable for that identity. Remove it once every live blob has been rewritten (it is the only remaining unverified read on this path).
    let section = match vsf::schema::SectionBuilder::parse_document(
        cloud_contacts_schema(),
        &vsf_bytes,
        None,
    ) {
        Ok(s) => s,
        Err(doc_err) => {
            let mut ptr = 0;
            let legacy = VsfSection::parse(&vsf_bytes, &mut ptr).map_err(|e| {
                CloudError::Parse(format!("VSF parse: {doc_err} (legacy fallback: {e})"))
            })?;
            crate::log("Cloud: contacts blob is pre-document (bare section) — reading legacy form; the next sync rewrites it with a header");
            return decode_contact_rows(
                legacy.get_fields("contact").into_iter().map(|f| f.values.as_slice()),
            );
        }
    };

    decode_contact_rows(
        section.get_fields("contact").into_iter().map(|f| f.values.as_slice()),
    )
}

/// Shared row decoder for both the verified-document path and the legacy bare-section fallback — one place that knows the `(contact: …)` row.
/// Reads by TYPE MARKER, never by index (AGENT.md: "VSF Type Markers Are Self-Describing"). The old positional form read `values[0]`, `values[1]`, … which is silent corruption waiting to happen: `party_id` and `device_pubkey` are BOTH `ke`, so position was the only thing telling them apart, and any reorder or inserted value would have swapped a contact's identity key with its device key with no error anywhere.
/// The two `ke` values are disambiguated by ORDER OF ARRIVAL within the row (first `ke` = party id, second = device pubkey) — the one thing position is still legitimately good for, because both are 32-byte keys of the same marker. Everything else is matched purely on its marker and length.
/// Takes value slices rather than a field type: the verified-document and legacy paths hand back structurally identical but distinct types (`schema::FieldValue` vs `file_format::VsfField`, both `{name, values}`).
fn decode_contact_rows<'a>(
    rows: impl Iterator<Item = &'a [VsfType]>,
) -> Result<Vec<CloudContact>, CloudError> {
    let mut contacts = Vec::new();
    for values in rows {
        let mut handle_proof: Option<[u8; 32]> = None;
        let mut party_id: Option<[u8; 32]> = None;
        let mut device_pubkey: Option<[u8; 32]> = None;
        let mut avatar_pin: Option<[u8; 64]> = None;
        let mut name: Option<String> = None;
        let mut trust_level = 0u8;
        let mut added = 0i64;

        for v in values {
            match v {
                VsfType::hP(b) if b.len() == 32 => {
                    handle_proof = b.as_slice().try_into().ok();
                }
                // Two 32-byte `ke` keys per row: party id first, device pubkey second.
                VsfType::ke(b) if b.len() == 32 => {
                    let key: Option<[u8; 32]> = b.as_slice().try_into().ok();
                    if party_id.is_none() {
                        party_id = key;
                    } else if device_pubkey.is_none() {
                        device_pubkey = key;
                    }
                }
                VsfType::ge(b) if b.len() == 64 => {
                    avatar_pin = b.as_slice().try_into().ok();
                }
                VsfType::x(s) => name = Some(s.clone()),
                VsfType::u3(t) => trust_level = *t,
                VsfType::e(vsf::types::EtType::e6(osc)) => added = *osc,
                _ => {}
            }
        }

        // A row is only a contact if every identifying part is present. Old 5-value handle-bearing rows are flag-day dead and simply never fill these.
        let (Some(handle_proof), Some(party_id), Some(device_pubkey), Some(avatar_pin), Some(name)) =
            (handle_proof, party_id, device_pubkey, avatar_pin, name)
        else {
            continue;
        };

        contacts.push(CloudContact {
            handle_proof,
            party_id,
            avatar_pin,
            name,
            device_pubkey,
            trust_level,
            added,
        });
    }

    Ok(contacts)
}

/// Convert CloudContact back to Contact for local storage
impl CloudContact {
    pub fn to_contact(&self) -> Contact {
        let mut contact = Contact::from_pin(
            self.name.clone(),
            self.avatar_pin,
            self.handle_proof,
            self.party_id,
            DevicePubkey::from_bytes(self.device_pubkey),
        );
        contact.trust_level = u8_to_trust_level(self.trust_level);
        contact.added = self.added;
        contact
    }
}

/// Encrypt for cloud storage — thin wrapper over [`crate::storage::encrypt_bytes`] that maps the stringified error into [`CloudError::Encryption`]. Wire format (12-byte nonce + ChaCha20-Poly1305 ciphertext + 16-byte auth tag) is identical to local-disk blobs by construction.
fn encrypt_data(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, CloudError> {
    encrypt_bytes(data, key).map_err(CloudError::Encryption)
}

/// Decrypt a cloud-storage blob — thin wrapper over [`crate::storage::decrypt_bytes`].
fn decrypt_data(encrypted: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, CloudError> {
    decrypt_bytes(encrypted, key).map_err(CloudError::Decryption)
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
// High-Level API (Blocking) ============================================================================

/// Sync contacts to FGTW cloud storage (blocking)
///
/// Uploads current contacts to cloud. Call after any contact change.
///
/// # Arguments
/// * `contacts` - Current contacts list * `identity_seed` - Our identity seed (BLAKE3 of VSF-normalized handle) * `device_keypair` - Device Ed25519 keypair * `handle_proof` - 32-byte handle proof (proves registered user)
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

    crate::logf!("Cloud: Uploading {} contacts ({} bytes encrypted)", contacts.len(), encrypted.len());

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
/// * `Ok(Some(contacts))` - Contacts found and decrypted * `Ok(None)` - No contacts blob exists on cloud * `Err(...)` - Error
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

    crate::logf!("Cloud: Downloaded contacts blob ({} bytes)", encrypted.len());

    // Decrypt and decode
    let contacts = decode_contacts(&encrypted, &encryption_key)?;
    crate::logf!("Cloud: Decoded {} contacts", contacts.len());
    Ok(Some(contacts))
}

// ============================================================================
// High-Level API (Async) ============================================================================

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
                party_id: [0xA1u8; 32],
                avatar_pin: [0xA2u8; 64],
                name: "alice".to_string(),
                device_pubkey: [2u8; 32],
                trust_level: 1,
                added: 1234567890,
            },
            CloudContact {
                handle_proof: [3u8; 32],
                party_id: [0xB1u8; 32],
                avatar_pin: [0xB2u8; 64],
                name: "bob".to_string(),
                device_pubkey: [4u8; 32],
                trust_level: 2,
                added: 1234567891,
            },
        ];

        let key = [42u8; 32];
        let encrypted = encode_contacts(&contacts, &key).unwrap();
        let decoded = decode_contacts(&encrypted, &key).unwrap();

        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].name, "alice");
        assert_eq!(decoded[0].handle_proof, [1u8; 32]);
        assert_eq!(decoded[1].name, "bob");
        assert_eq!(decoded[1].trust_level, 2);
    }

    #[test]
    fn test_wrong_key_fails() {
        let contacts = vec![CloudContact {
            handle_proof: [1u8; 32],
            party_id: [0xA1u8; 32],
            avatar_pin: [0xA2u8; 64],
            name: "alice".to_string(),
            device_pubkey: [2u8; 32],
            trust_level: 1,
            added: 1234567890,
        }];

        let key1 = [42u8; 32];
        let key2 = [43u8; 32];

        let encrypted = encode_contacts(&contacts, &key1).unwrap();
        let result = decode_contacts(&encrypted, &key2);

        assert!(result.is_err());
    }
}

#[cfg(test)]
mod document_tests {
    use super::*;

    fn sample() -> Vec<CloudContact> {
        vec![
            CloudContact {
                handle_proof: [0x11; 32],
                party_id: [0x22; 32],
                avatar_pin: [0x33; 64],
                name: "alice".to_string(),
                device_pubkey: [0x44; 32],
                trust_level: 2,
                added: 1234567890,
            },
            CloudContact {
                handle_proof: [0x55; 32],
                party_id: [0x66; 32],
                avatar_pin: [0u8; 64],
                name: String::new(),
                device_pubkey: [0x77; 32],
                trust_level: 0,
                added: -42,
            },
        ]
    }

    /// The blob must be a COMPLETE VSF FILE (AGENT.md: "VSF Transport Rule: COMPLETE FILES ONLY") — magic header + provenance hash — not a bare section. A bare section is what shipped before, and it is why the attest path had no provenance to compare and had to re-upload unconditionally.
    #[test]
    fn blob_is_a_complete_vsf_document() {
        let key = [7u8; 32];
        let blob = encode_contacts(&sample(), &key).expect("encode");
        let plain = crate::storage::decrypt_bytes(&blob, &key).expect("decrypt");
        assert!(plain.starts_with(b"R\xc3\x85<"), "must carry the RÅ< magic header, got {:?}", &plain[..plain.len().min(8)]);
        let (header, _) = vsf::verification::read_verified(&plain, None).expect("read_verified must accept our own document");
        assert!(
            matches!(header.provenance_hash, VsfType::hp(ref h) if h.len() == 32),
            "a document without a 32-byte hp has nothing to compare or verify, got {:?}",
            header.provenance_hash
        );
    }

    /// Round-trip through the real encode/decode pair, values matched by type marker.
    #[test]
    fn round_trip_preserves_every_field() {
        let key = [9u8; 32];
        let original = sample();
        let blob = encode_contacts(&original, &key).expect("encode");
        let back = decode_contacts(&blob, &key).expect("decode");
        assert_eq!(back, original, "decode must reproduce exactly what encode was given");
    }

    /// The two 32-byte `ke` values (party id, device pubkey) must not swap. This is the silent corruption the positional decoder invited.
    #[test]
    fn party_id_and_device_pubkey_do_not_swap() {
        let key = [3u8; 32];
        let blob = encode_contacts(&sample(), &key).expect("encode");
        let back = decode_contacts(&blob, &key).expect("decode");
        assert_eq!(back[0].party_id, [0x22; 32]);
        assert_eq!(back[0].device_pubkey, [0x44; 32]);
    }
}
