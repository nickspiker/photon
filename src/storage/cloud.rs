//! Cloud contact backup via FGTW blob endpoints.
//!
//! ONE blob per identity, readable by every device in the fleet — the whole fleet shares one slot.
//!
//! Key derivation (v1):
//! - Storage key: BLAKE3(identity_seed || fleet_key || "contacts_storage_key_v1")
//! - Encryption key: BLAKE3(identity_seed || fleet_key || "contacts_v1")
//!
//! v0 bound `device_secret` (the device's Ed25519 secret) into BOTH, which meant a blob was
//! readable only by the device that wrote it. That is not a backup: the case people mean by
//! backup is a lost or replaced device, and that is exactly the case where the key is gone.
//! It was also silently orphaned on every Android reinstall, because the Android fingerprint
//! oracle is ANDROID_ID and that rotates. Binding the FLEET key instead makes the blob what
//! the module always claimed to be, and gives it a natural purge path (any device can delete
//! it, and `clean_device_for_reuse` does).
//!
//! This is a BACKUP, not the sync channel. Live cross-device contact state travels on the
//! fleet roster (`fleet::push_roster`/`pull_roster`), which is a proper CRDT with a logical
//! clock and tombstones. This blob is the cold copy for a fleet that has lost every device's
//! vault but still holds the fleet key.
//!
//! Security:
//! - identity_seed = BLAKE3(VsfType::x(handle)) - private
//! - fleet_key = the epoch'd fleet secret, fanned out to members - private, fleet-wide
//! - handle_proof is PUBLIC - never use for encryption!

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use blake3::Hasher;
use vsf::schema::{SectionSchema, TypeConstraint};
use vsf::VsfType;

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

/// Derive the storage key (address) for the fleet's contacts blob. Returns base64url-encoded 32-byte hash (43 chars).
/// Binds the FLEET key, so every device in the fleet computes the same address — that is what makes this a backup rather than a per-device cache.
pub fn contacts_storage_key(identity_seed: &[u8; 32], fleet_key: &[u8; 32]) -> String {
    let mut hasher = Hasher::new();
    hasher.update(identity_seed);
    hasher.update(fleet_key);
    hasher.update(b"contacts_storage_key_v1");
    let hash = hasher.finalize();
    URL_SAFE_NO_PAD.encode(hash.as_bytes())
}

/// Derive the encryption key for the fleet's contacts blob. Fleet-bound for the same reason as the address.
pub fn contacts_encryption_key(identity_seed: &[u8; 32], fleet_key: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(identity_seed);
    hasher.update(fleet_key);
    hasher.update(b"contacts_v1");
    *hasher.finalize().as_bytes()
}

/// The v0 storage key — device-bound, and therefore only ever computable by the device that wrote it.
/// Kept for ONE purpose: deleting the blob a device left behind, which is only possible while that device still holds its own secret. `clean_device_for_reuse` calls this before wiping. Nothing reads or writes v0 content any more.
pub fn contacts_storage_key_v0_device_bound(
    identity_seed: &[u8; 32],
    device_secret: &[u8; 32],
) -> String {
    let mut hasher = Hasher::new();
    hasher.update(identity_seed);
    hasher.update(device_secret);
    hasher.update(b"contacts_storage_key_v0");
    let hash = hasher.finalize();
    URL_SAFE_NO_PAD.encode(hash.as_bytes())
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

    // Verified read, no fallback. `parse_document` runs `read_verified` internally (header decode + provenance self-consistency), so a tampered, truncated, or pre-document blob is REJECTED rather than parsed on faith — the hand-rolled `VsfSection::parse` this replaces checked nothing at all.
    // Blobs written before the document wrapper are bare sections and fail here, which is correct and costs nothing: the storage key is BLAKE3(identity_seed ‖ device_secret), so a blob is written and read by exactly ONE device — there is no other client to stay compatible with. The caller treats a decode failure as "no cloud contacts this round" (`if let Ok(Some(..))`), skips the merge, and then re-uploads from the local vault in document form. One attest without a cloud merge, self-healed, and the vault was the source of truth for those contacts the whole time.
    // A legacy branch here would be exactly the compatibility fork AGENT.md forbids, in exchange for nothing.
    let section =
        vsf::schema::SectionBuilder::parse_document(cloud_contacts_schema(), &vsf_bytes, None)
            .map_err(|e| CloudError::Parse(format!("contacts blob failed verified read: {e}")))?;

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

/// The canonical TrustLevel wire mapping. Public because the fleet roster carries `trust_level` too (PRST3) and must agree byte-for-byte with the backup blob — one mapping, not two.
pub fn trust_level_to_u8(level: TrustLevel) -> u8 {
    match level {
        TrustLevel::Stranger => 0,
        TrustLevel::Known => 1,
        TrustLevel::Trusted => 2,
        TrustLevel::Inner => 3,
    }
}

/// Inverse of [`trust_level_to_u8`]. Unknown values fall back to Stranger — the least-privileged reading, so a future level added by a newer device can never silently grant more trust than it should here.
pub fn u8_to_trust_level(v: u8) -> TrustLevel {
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
/// * `contacts` - Current contacts list * `identity_seed` - Our identity seed (BLAKE3 of VSF-normalized handle) * `fleet_key` - the fleet's shared secret; addresses AND seals the blob, so any device in the fleet can read it back * `device_keypair` - Device Ed25519 keypair, used only to SIGN the blob_put (FGTW authorises the write against the fleet chain) * `handle_proof` - 32-byte handle proof (proves registered user)
pub fn sync_contacts_to_cloud(
    contacts: &[Contact],
    identity_seed: &[u8; 32],
    fleet_key: &[u8; 32],
    device_keypair: &crate::network::fgtw::Keypair,
    handle_proof: &[u8; 32],
) -> Result<(), CloudError> {
    use crate::network::fgtw::{put_blob_blocking, BlobError};

    // Convert contacts to cloud format
    let cloud_contacts: Vec<CloudContact> = contacts.iter().map(CloudContact::from).collect();

    // Derive keys — FLEET-bound, so every device in the fleet resolves the same address and can open the result. The device keypair below only SIGNS the write.
    let storage_key = contacts_storage_key(identity_seed, fleet_key);
    let encryption_key = contacts_encryption_key(identity_seed, fleet_key);

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
    fleet_key: &[u8; 32],
) -> Result<Option<Vec<CloudContact>>, CloudError> {
    use crate::network::fgtw::{get_blob_blocking, BlobError};

    // Derive keys — FLEET-bound. The read needs no device key at all: the GET is unauthenticated (the payload is ciphertext only fleet members can open), which is exactly why any device in the fleet can restore from this blob.
    let storage_key = contacts_storage_key(identity_seed, fleet_key);
    let encryption_key = contacts_encryption_key(identity_seed, fleet_key);

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

    /// The whole point of v1: the address and the key follow the FLEET, not the device. Two devices in one fleet must land on the SAME blob (that is what makes it a restore path), and a different fleet key must land somewhere else entirely.
    #[test]
    fn blob_address_follows_the_fleet_not_the_device() {
        let identity_seed = [1u8; 32];
        let fleet_key = [2u8; 32];
        let other_fleet_key = [3u8; 32];

        // Same identity + same fleet key = same slot, no matter which device asks. (The device
        // secret is not an input at all any more, which is exactly the v0 bug being removed.)
        assert_eq!(
            contacts_storage_key(&identity_seed, &fleet_key),
            contacts_storage_key(&identity_seed, &fleet_key),
            "every device in the fleet must resolve the same address"
        );
        assert_eq!(
            contacts_encryption_key(&identity_seed, &fleet_key),
            contacts_encryption_key(&identity_seed, &fleet_key),
            "every device in the fleet must derive the same encryption key"
        );

        // A different fleet key is a different slot AND a different key — an epoch rotation
        // must not silently read or clobber the previous epoch's blob.
        assert_ne!(
            contacts_storage_key(&identity_seed, &fleet_key),
            contacts_storage_key(&identity_seed, &other_fleet_key)
        );
        assert_ne!(
            contacts_encryption_key(&identity_seed, &fleet_key),
            contacts_encryption_key(&identity_seed, &other_fleet_key)
        );

        // v1 and v0 are different addresses, so the re-key cannot collide with the blob a
        // device already left behind (which only its own secret can still delete).
        let device_secret = [9u8; 32];
        assert_ne!(
            contacts_storage_key(&identity_seed, &fleet_key),
            contacts_storage_key_v0_device_bound(&identity_seed, &device_secret)
        );
    }

    /// A pre-document blob (bare section, no header) must be REJECTED, not silently parsed. The caller treats that as "no cloud contacts this round" and re-uploads in document form, so the slot self-heals — and because the storage key binds the device secret, no other client ever reads this blob.
    #[test]
    fn pre_document_blob_is_rejected() {
        let key = [5u8; 32];
        // Rebuild the OLD wire form: the section body alone, exactly what `builder.encode()` used to ship.
        let schema = cloud_contacts_schema();
        let mut b = schema.build().set("version", 0u8).expect("version");
        for c in sample() {
            b = b
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
                .expect("row");
        }
        let bare = b.encode().expect("bare section");
        let legacy_blob = crate::storage::encrypt_bytes(&bare, &key).expect("seal");

        assert!(
            decode_contacts(&legacy_blob, &key).is_err(),
            "a headerless blob has no provenance hash — it must fail the verified read, not parse on faith"
        );
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
