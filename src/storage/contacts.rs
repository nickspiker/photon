//! Contact persistence via FlatStorage.
//!
//! Vault address scheme — every entry is `vault_key(domain, scope)`, a flat 32-byte address, never a path or an encoded string:
//! - Contact index: `vault_key("contacts", my_vault_seed)` — self-scoped (this vault's own list)
//! - Contact state: `vault_key("state", their_identity_seed)`
//! - Contact keypairs: `vault_key("keypairs", their_identity_seed)`
//! - Contact slots: `vault_key("slots", their_identity_seed)`
//!
//! Messages are NOT here — conversation content lives in the rārangi conversation DB keyed by `friendship_id`, not per-peer under a contact.
//!
//! All encryption, addressing, and atomicity is handled by FlatStorage.

use crate::crypto::clutch::ClutchKemResponsePayload;
use crate::storage::{FlatStorage, StorageError};
use crate::types::{
    ClutchState, Contact, ContactId, DevicePubkey, FriendshipId, HandleText, Seed, TrustLevel,
};
use vsf::schema::{SectionBuilder, SectionSchema, TypeConstraint};
use vsf::types::EagleTime;
use vsf::{VsfSection, VsfType};

/// Convert any VSF Eagle Time variant to i64 oscillations
fn vsf_to_oscillations(v: &VsfType) -> i64 {
    match v {
        VsfType::e(vsf::types::EtType::e6(osc)) => *osc,
        v => {
            let et = EagleTime::new_from_vsf(v.clone());
            et.oscillations().unwrap_or(0)
        }
    }
}

/// Static identity data stored in the contact list index
#[derive(Clone, Debug)]
pub struct ContactIdentity {
    pub handle_proof: [u8; 32],
    pub handle: String,
}

impl ContactIdentity {
    /// Derive identity_seed from handle using VSF normalization This ensures consistent key derivation regardless of Unicode representation
    pub fn identity_seed(&self) -> [u8; 32] {
        derive_identity_seed(&self.handle)
    }
}

/// Derive identity_seed from a handle string. Delegates to `ihi::handle_to_hash` — the canonical "handle string → 32 bytes" intermediate (VsfType::x pre-hash + BLAKE3) that `handle_to_proof` uses internally. Matches `Contact::new`'s `handle_hash` field and the avatar key seeds.
pub fn derive_identity_seed(handle: &str) -> [u8; 32] {
    *ihi::handle_to_hash(handle).as_bytes()
}

/// Vault address for one of a contact's per-peer entries — `vault_key(domain, their_identity_seed)`. `domain` is the plain entry name ("state", "keypairs", "slots"); the peer's seed is the scope. No paths, no hex.
fn contact_key(their_identity_seed: &[u8; 32], domain: &str) -> [u8; 32] {
    crate::storage::vault_key(domain, their_identity_seed)
}

// ============================================================================
// Contact List (Index) - Static Identity Data (Schema-validated) ============================================================================

/// Schema for contact_list section Each contact field contains: (handle_proof: hb, handle: x)
fn contact_list_schema() -> SectionSchema {
    SectionSchema::new("contact_list")
        // Contact field allows mixed types (hash, string) - use Any
        .field("contact", TypeConstraint::Any)
}

/// Save the contact list to encrypted index with schema validation
pub fn save_contact_list(
    contacts: &[ContactIdentity],
    storage: &FlatStorage,
) -> Result<(), StorageError> {
    let schema = contact_list_schema();
    let mut builder = schema.build();

    for c in contacts {
        builder = builder
            .append_multi(
                "contact",
                vec![
                    VsfType::hP(c.handle_proof.to_vec()),
                    VsfType::x(c.handle.clone()),
                ],
            )
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }

    let vsf_bytes = builder
        .encode()
        .map_err(|e| StorageError::Parse(e.to_string()))?;

    storage.write_addr(&crate::storage::vault_key("contacts", storage.vault_seed()), &vsf_bytes)
}

/// Load the contact list from encrypted index with schema validation
pub fn load_contact_list(storage: &FlatStorage) -> Result<Vec<ContactIdentity>, StorageError> {
    let vsf_bytes = match storage.read_addr(&crate::storage::vault_key("contacts", storage.vault_seed()))? {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };

    #[cfg(feature = "development")]
    crate::network::inspect::vsf_read_decrypted(&vsf_bytes, "contacts/index");

    let schema = contact_list_schema();
    let builder = SectionBuilder::parse(schema, &vsf_bytes)
        .map_err(|e| StorageError::Parse(format!("Contact list parse: {}", e)))?;

    let mut contacts = Vec::new();
    for field in builder.get_fields("contact") {
        if field.values.len() >= 2 {
            let handle_proof: [u8; 32] = match &field.values[0] {
                VsfType::hP(v) if v.len() == 32 => v.as_slice().try_into().unwrap(),
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
// Contact State - Mutable Session Data (Schema-validated) ============================================================================

/// Schema for contact_state section
fn contact_state_schema() -> SectionSchema {
    SectionSchema::new("contact_state")
        .field("clutch_state", TypeConstraint::AnyUnsigned)
        .field("trust_level", TypeConstraint::AnyUnsigned)
        .field("pubkey", TypeConstraint::Ed25519Key)
        .field("added", TypeConstraint::Any) // Eagle Time
        .field("id", TypeConstraint::AnyHash)
        // Optional fields
        .field("ip", TypeConstraint::AnyString)
        .field("seed", TypeConstraint::AnyHash)
        .field("friendship_id", TypeConstraint::AnyHash) // Links to friendship storage
        .field("last_seen", TypeConstraint::Any) // f64 Eagle Time
        .field("completed_their_hqc_prefix", TypeConstraint::AnyHash) // Detects stale offers (8 bytes)
}

/// Save contact state (mutable data) with schema validation
pub fn save_contact_state(contact: &Contact, storage: &FlatStorage) -> Result<(), StorageError> {
    let identity_seed = derive_identity_seed(contact.handle.as_str());

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
        .set("added", VsfType::e(vsf::types::EtType::e6(contact.added)))
        .map_err(|e| StorageError::Parse(e.to_string()))?
        .set("id", VsfType::hb(contact.id.as_bytes().to_vec()))
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
    if let Some(friendship_id) = &contact.friendship_id {
        builder = builder
            .set(
                "friendship_id",
                VsfType::hb(friendship_id.as_bytes().to_vec()),
            )
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    if let Some(last_seen) = contact.last_seen {
        builder = builder
            .set("last_seen", VsfType::e(vsf::types::EtType::e6(last_seen)))
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    if let Some(hqc_prefix) = &contact.completed_their_hqc_prefix {
        builder = builder
            .set(
                "completed_their_hqc_prefix",
                VsfType::hb(hqc_prefix.to_vec()),
            )
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }

    let vsf_bytes = builder
        .encode()
        .map_err(|e| StorageError::Parse(e.to_string()))?;

    storage.write_addr(&contact_key(&identity_seed, "state"), &vsf_bytes)
}

/// Load contact state
pub fn load_contact_state(
    identity: &ContactIdentity,
    storage: &FlatStorage,
) -> Result<Contact, StorageError> {
    let their_identity_seed = identity.identity_seed();

    let vsf_bytes = match storage.read_addr(&contact_key(&their_identity_seed, "state"))? {
        Some(b) => b,
        None => {
            // No state yet - return contact with just identity info
            let pubkey = DevicePubkey::from_bytes([0u8; 32]); // placeholder
            let contact = Contact::new(
                HandleText::new(&identity.handle),
                identity.handle_proof,
                pubkey,
            );
            return Ok(contact);
        }
    };

    #[cfg(feature = "development")]
    crate::network::inspect::vsf_read_decrypted(&vsf_bytes, "contact/state");

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
    let added = match get_val("added") {
        Some(v) => vsf_to_oscillations(v),
        None => 0,
    };

    let pubkey = DevicePubkey::from_bytes(pubkey_bytes);
    let mut contact = Contact::new(
        HandleText::new(&identity.handle),
        identity.handle_proof,
        pubkey,
    );

    contact.clutch_state = u8_to_clutch_state(clutch_u8);
    contact.trust_level = u8_to_trust_level(trust_u8);
    contact.added = added;

    // Optional fields
    if let Some(VsfType::x(s) | VsfType::d(s)) = get_val("ip") {
        contact.ip = s.parse().ok();
    }
    if let Some(VsfType::hb(v)) = get_val("seed") {
        if v.len() == 32 {
            contact.relationship_seed = Some(Seed::from_bytes(v.as_slice().try_into().unwrap()));
        }
    }
    if let Some(VsfType::hb(v)) = get_val("friendship_id") {
        if v.len() == 32 {
            contact.friendship_id =
                Some(FriendshipId::from_bytes(v.as_slice().try_into().unwrap()));
        }
    }
    if let Some(v) = get_val("last_seen") {
        contact.last_seen = Some(vsf_to_oscillations(v));
    }
    if let Some(VsfType::hb(v)) = get_val("id") {
        if v.len() == 32 {
            contact.id = ContactId::from_bytes(v.as_slice().try_into().unwrap());
        }
    }
    if let Some(VsfType::hb(v)) = get_val("completed_their_hqc_prefix") {
        if v.len() == 8 {
            contact.completed_their_hqc_prefix = Some(v.as_slice().try_into().unwrap());
        }
    }

    Ok(contact)
}

// ============================================================================
// High-Level API ============================================================================

/// Save a contact (updates both list and state)
pub fn save_contact(contact: &Contact, storage: &FlatStorage) -> Result<(), StorageError> {
    // Save state file
    save_contact_state(contact, storage)?;

    // Update contact list
    let mut list = load_contact_list(storage).unwrap_or_default();

    // Check if contact already exists in list (by handle)
    let exists = list.iter().any(|c| c.handle == contact.handle.as_str());

    if !exists {
        list.push(ContactIdentity {
            handle_proof: contact.handle_proof,
            handle: contact.handle.as_str().to_string(),
        });
        save_contact_list(&list, storage)?;
    }

    Ok(())
}

/// Load all contacts from disk
pub fn load_all_contacts(storage: &FlatStorage) -> Vec<Contact> {
    let identities = match load_contact_list(storage) {
        Ok(list) => list,
        Err(e) => {
            crate::log(&format!("Failed to load contact list: {}", e));
            return Vec::new();
        }
    };

    let mut contacts = Vec::new();
    for identity in identities {
        match load_contact_state(&identity, storage) {
            Ok(contact) => contacts.push(contact),
            Err(e) => {
                crate::log(&format!(
                    "Failed to load contact state for '{}': {}",
                    identity.handle, e
                ));
            }
        }
    }
    contacts
}

/// Delete a contact's per-peer entries from the vault. Conversation messages are NOT deleted here — they live in the rārangi conversation DB keyed by `friendship_id` (a conversation can outlive removing one party from contacts), and are reaped through that layer.
pub fn delete_contact(identity_seed: &[u8; 32], storage: &FlatStorage) -> Result<(), StorageError> {
    storage.delete_addr(&contact_key(identity_seed, "state"))?;
    storage.delete_addr(&contact_key(identity_seed, "keypairs"))?;
    storage.delete_addr(&contact_key(identity_seed, "slots"))?;
    Ok(())
}

fn clutch_state_to_u8(state: ClutchState) -> u8 {
    // Match enum discriminant order: Pending=0, AwaitingProof=1, Complete=2
    match state {
        ClutchState::Pending => 0,
        ClutchState::AwaitingProof => 1,
        ClutchState::Complete => 2,
    }
}

fn u8_to_clutch_state(v: u8) -> ClutchState {
    // Match enum discriminant order: Pending=0, AwaitingProof=1, Complete=2
    match v {
        1 => ClutchState::AwaitingProof,
        2 => ClutchState::Complete,
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

// ============================================================================
// CLUTCH Keypairs Storage (~600KB, stored separately) ============================================================================

use crate::crypto::clutch::ClutchAllKeypairs;

/// Save CLUTCH keypairs to disk (encrypted). Called after keygen completes - persists ~600KB of ephemeral keypairs.
pub fn save_clutch_keypairs(
    keypairs: &ClutchAllKeypairs,
    handle: &str,
    storage: &FlatStorage,
) -> Result<(), StorageError> {
    let their_identity_seed = derive_identity_seed(handle);

    // Build VSF section from keypairs (two multi-value fields)
    let mut section = VsfSection::new("clutch_keypairs");
    let (pubkeys, secrets) = keypairs.to_vsf_multi();
    section.add_field_multi("pubkeys", pubkeys);
    section.add_field_multi("secrets", secrets);

    let vsf_bytes = section.encode();

    storage.write_addr(&contact_key(&their_identity_seed, "keypairs"), &vsf_bytes)?;

    #[cfg(feature = "development")]
    crate::log(&format!(
        "STORAGE: Saved CLUTCH keypairs for {} (~{}KB)",
        handle,
        vsf_bytes.len() / 1024
    ));

    Ok(())
}

/// Load CLUTCH keypairs from disk. Returns None if no keypairs file exists or parsing fails.
pub fn load_clutch_keypairs(
    handle: &str,
    storage: &FlatStorage,
) -> Result<Option<ClutchAllKeypairs>, StorageError> {
    let their_identity_seed = derive_identity_seed(handle);

    let vsf_bytes = match storage.read_addr(&contact_key(&their_identity_seed, "keypairs"))? {
        Some(b) => b,
        None => return Ok(None),
    };

    #[cfg(feature = "development")]
    crate::network::inspect::vsf_read_decrypted(&vsf_bytes, "contact/keypairs");

    let mut ptr = 0;
    let section = VsfSection::parse(&vsf_bytes, &mut ptr)
        .map_err(|e| StorageError::Parse(format!("Keypairs parse: {}", e)))?;

    let keypairs = ClutchAllKeypairs::from_vsf_section(&section)
        .ok_or_else(|| StorageError::Parse("Invalid keypairs format".into()))?;

    #[cfg(feature = "development")]
    crate::log(&format!(
        "STORAGE: Loaded CLUTCH keypairs for {} (~{}KB)",
        handle,
        vsf_bytes.len() / 1024
    ));

    Ok(Some(keypairs))
}

/// Delete CLUTCH keypairs (called after ceremony completes or on zeroize)
pub fn delete_clutch_keypairs(handle: &str, storage: &FlatStorage) -> Result<(), StorageError> {
    let their_identity_seed = derive_identity_seed(handle);
    storage.delete_addr(&contact_key(&their_identity_seed, "keypairs"))?;
    #[cfg(feature = "development")]
    crate::log(&format!("STORAGE: Deleted CLUTCH keypairs for {}", handle));
    Ok(())
}

// ============================================================================
// CLUTCH Slots Storage (ceremony progress - offers, KEM secrets) ============================================================================

use crate::crypto::clutch::{ClutchKemSharedSecrets, ClutchOfferPayload};
use crate::types::PartySlot;

/// Save CLUTCH slots to disk (encrypted). Persists ceremony progress: offers received, KEM secrets computed.
///
/// VSF structure (proper multi-value fields):
/// ```text
/// [clutch_slots] (ceremony_id: hb{...})                  // if computed (provenances: hb{p0}, hb{p1}, ...)     // only if ceremony_id not yet computed (slot: hb{handle}, u0{offer}, u0{from}, u0{to}, ...data...)  // repeated
/// ```
pub fn save_clutch_slots(
    slots: &[PartySlot],
    offer_provenances: &[[u8; 32]],
    ceremony_id: Option<[u8; 32]>,
    handle: &str,
    storage: &FlatStorage,
) -> Result<(), StorageError> {
    if slots.is_empty() {
        return Ok(()); // Nothing to save
    }

    let their_identity_seed = derive_identity_seed(handle);

    // Build VSF section with all slots
    let mut section = VsfSection::new("clutch_slots");

    // Ceremony ID takes priority - if we have it, no need for provenances
    if let Some(cid) = ceremony_id {
        section.add_field("ceremony_id", VsfType::hb(cid.to_vec()));
    } else if !offer_provenances.is_empty() {
        // Only store provenances if ceremony_id not yet computed (needed to derive it later)
        section.add_field_multi(
            "provenances",
            offer_provenances
                .iter()
                .map(|p| VsfType::hb(p.to_vec()))
                .collect(),
        );
    }

    // Each slot as a repeated "slot" field with multi-value Format: (slot: hb{handle_hash}, u0{has_offer}, u0{has_from}, u0{has_to}, u0{has_resend}, ...offer_keys..., ...from_secrets..., ...to_secrets..., ...resend_payload...)
    for slot in slots {
        let mut values: Vec<VsfType> = Vec::new();

        // Handle hash identifies this slot's party
        values.push(VsfType::hb(slot.handle_hash.to_vec()));

        // Flags for what's present
        values.push(VsfType::u0(slot.offer.is_some()));
        values.push(VsfType::u0(slot.kem_secrets_from_them.is_some()));
        values.push(VsfType::u0(slot.kem_secrets_to_them.is_some()));
        values.push(VsfType::u0(slot.kem_response_for_resend.is_some()));

        // Offer data (if present) - 8 public keys in fixed order
        if let Some(ref offer) = slot.offer {
            values.push(VsfType::kx(offer.x25519_public.to_vec()));
            values.push(VsfType::kp(offer.p384_public.clone()));
            values.push(VsfType::kk(offer.secp256k1_public.clone()));
            values.push(VsfType::kp(offer.p256_public.clone()));
            values.push(VsfType::kf(offer.frodo976_public.clone()));
            values.push(VsfType::kn(offer.ntru701_public.clone()));
            values.push(VsfType::kl(offer.mceliece_public.clone()));
            values.push(VsfType::kh(offer.hqc256_public.clone()));
        }

        // KEM secrets from them (if present) - 8 typed shared secrets
        if let Some(ref secrets) = slot.kem_secrets_from_them {
            values.push(VsfType::ksx(secrets.x25519.to_vec()));
            values.push(VsfType::ksp(secrets.p384.clone()));
            values.push(VsfType::ksk(secrets.secp256k1.clone()));
            values.push(VsfType::ksp(secrets.p256.clone()));
            values.push(VsfType::ksf(secrets.frodo.clone()));
            values.push(VsfType::ksn(secrets.ntru.clone()));
            values.push(VsfType::ksl(secrets.mceliece.clone()));
            values.push(VsfType::ksh(secrets.hqc.clone()));
        }

        // KEM secrets to them (if present) - 8 typed shared secrets
        if let Some(ref secrets) = slot.kem_secrets_to_them {
            values.push(VsfType::ksx(secrets.x25519.to_vec()));
            values.push(VsfType::ksp(secrets.p384.clone()));
            values.push(VsfType::ksk(secrets.secp256k1.clone()));
            values.push(VsfType::ksp(secrets.p256.clone()));
            values.push(VsfType::ksf(secrets.frodo.clone()));
            values.push(VsfType::ksn(secrets.ntru.clone()));
            values.push(VsfType::ksl(secrets.mceliece.clone()));
            values.push(VsfType::ksh(secrets.hqc.clone()));
        }

        // KEM response for resend (if present) - 4 PQC ciphertexts + 4 EC ephemeral + target prefix
        if let Some(ref resend) = slot.kem_response_for_resend {
            // PQC ciphertexts (using v() wrapped type with algorithm marker)
            values.push(VsfType::v(b'f', resend.frodo976_ciphertext.clone()));
            values.push(VsfType::v(b'n', resend.ntru701_ciphertext.clone()));
            values.push(VsfType::v(b'l', resend.mceliece_ciphertext.clone()));
            values.push(VsfType::v(b'h', resend.hqc256_ciphertext.clone()));
            // Target HQC prefix (8 bytes) - use v('t', ...) to distinguish from handle hb
            values.push(VsfType::v(b't', resend.target_hqc_pub_prefix.to_vec()));
            // EC ephemeral pubkeys - use v('e', ...) to distinguish from offer keys
            values.push(VsfType::v(b'x', resend.x25519_ephemeral.to_vec()));
            values.push(VsfType::v(b'3', resend.p384_ephemeral.clone()));
            values.push(VsfType::v(b'k', resend.secp256k1_ephemeral.clone()));
            values.push(VsfType::v(b'2', resend.p256_ephemeral.clone()));
        }

        section.add_field_multi("slot", values);
    }

    let vsf_bytes = section.encode();

    storage.write_addr(&contact_key(&their_identity_seed, "slots"), &vsf_bytes)?;

    #[cfg(feature = "development")]
    crate::log(&format!(
        "STORAGE: Saved CLUTCH slots for {} ({} slots, {}B)",
        handle,
        slots.len(),
        vsf_bytes.len()
    ));

    Ok(())
}

/// Loaded CLUTCH ceremony state
pub struct ClutchCeremonyState {
    pub slots: Vec<PartySlot>,
    pub offer_provenances: Vec<[u8; 32]>,
    pub ceremony_id: Option<[u8; 32]>,
}

/// Load CLUTCH slots from disk. Returns None if no slots file exists.
///
/// Parses VSF structure with multi-value fields (no decimal string prefixes):
/// ```text
/// [clutch_slots] (provenances: hb{p0}, hb{p1}, ...) (ceremony_id: hb{...}) (slot: hb{handle}, u0{offer}, u0{from}, u0{to}, ...data...)
/// ```
pub fn load_clutch_slots(
    handle: &str,
    storage: &FlatStorage,
) -> Result<Option<ClutchCeremonyState>, StorageError> {
    let their_identity_seed = derive_identity_seed(handle);

    let vsf_bytes = match storage.read_addr(&contact_key(&their_identity_seed, "slots"))? {
        Some(b) => b,
        None => return Ok(None),
    };

    #[cfg(feature = "development")]
    crate::network::inspect::vsf_read_decrypted(&vsf_bytes, "contact/slots");

    let mut ptr = 0;
    let section = VsfSection::parse(&vsf_bytes, &mut ptr)
        .map_err(|e| StorageError::Parse(format!("Slots parse: {}", e)))?;

    // Parse provenances from multi-value field
    let offer_provenances: Vec<[u8; 32]> = section
        .get_field("provenances")
        .map(|f| {
            f.values
                .iter()
                .filter_map(|v| match v {
                    VsfType::hb(b) if b.len() == 32 => {
                        let mut arr = [0u8; 32];
                        arr.copy_from_slice(b);
                        Some(arr)
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();

    // Parse ceremony_id (optional single value)
    let ceremony_id = section
        .get_field("ceremony_id")
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::hb(b) if b.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(b);
                Some(arr)
            }
            _ => None,
        });

    // Parse slots from repeated "slot" fields
    let slot_fields = section.get_fields("slot");
    let mut slots = Vec::with_capacity(slot_fields.len());

    for field in slot_fields {
        let slot = parse_slot_from_values(&field.values)?;
        slots.push(slot);
    }

    // Sanity check: ceremony_id requires both parties' provenances to compute If we have a ceremony_id but fewer than 2 provenances, it's stale (peer reset)
    let ceremony_id = if ceremony_id.is_some() && offer_provenances.len() < 2 {
        #[cfg(feature = "development")]
        crate::log(&format!(
            "STORAGE: Clearing stale ceremony_id for {} (only {} provenances)",
            handle,
            offer_provenances.len()
        ));
        None
    } else {
        ceremony_id
    };

    #[cfg(feature = "development")]
    crate::log(&format!(
        "STORAGE: Loaded CLUTCH slots for {} ({} slots, {} provenances, ceremony_id={})",
        handle,
        slots.len(),
        offer_provenances.len(),
        ceremony_id
            .map(|c| hex::encode(&c[..4]))
            .unwrap_or_else(|| "none".into())
    ));

    Ok(Some(ClutchCeremonyState {
        slots,
        offer_provenances,
        ceremony_id,
    }))
}

/// Parse a PartySlot from multi-value field values. Type markers are self-describing - we match on kx/kf/kn/kl/kh/kk/kp, NOT position. Format: hb{handle}, u0{has_offer}, u0{has_from}, u0{has_to}, u0{has_resend}, ...keys by type...
fn parse_slot_from_values(values: &[VsfType]) -> Result<PartySlot, StorageError> {
    // Support old format (4 flags) and new format (5 flags with has_resend)
    if values.len() < 4 {
        return Err(StorageError::Parse("Slot field too short".into()));
    }

    // Parse handle_hash (first value - this one IS positional, it's the identifier)
    let handle_hash = match &values[0] {
        VsfType::hb(b) if b.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(b);
            arr
        }
        _ => return Err(StorageError::Parse("Invalid handle_hash in slot".into())),
    };

    // Parse flags (values 1-3/4 - positional since they're just bools)
    let has_offer = match &values[1] {
        VsfType::u0(b) => *b,
        _ => return Err(StorageError::Parse("Invalid has_offer flag".into())),
    };
    let has_from = match &values[2] {
        VsfType::u0(b) => *b,
        _ => return Err(StorageError::Parse("Invalid has_from flag".into())),
    };
    let has_to = match &values[3] {
        VsfType::u0(b) => *b,
        _ => return Err(StorageError::Parse("Invalid has_to flag".into())),
    };
    // New flag for resend payload (optional for backwards compat)
    let has_resend = if values.len() > 4 {
        match &values[4] {
            VsfType::u0(b) => *b,
            _ => false, // Old format - no resend flag
        }
    } else {
        false
    };

    let mut slot = PartySlot::new(handle_hash);

    // Parse keys and secrets by TYPE MARKER, not position The type (kx, kf, kn, kl, kh, kk, kp, ks) tells us exactly what it is Start at index 5 (after 5 flags: handle, offer, from, to, resend)
    let data_start = 5;
    if has_offer {
        let mut offer = ClutchOfferPayload::default();
        let mut found_keys = 0u8;

        for v in &values[data_start..] {
            match v {
                VsfType::kx(b) if b.len() == 32 => {
                    offer.x25519_public.copy_from_slice(b);
                    found_keys |= 1;
                }
                VsfType::kf(b) => {
                    offer.frodo976_public = b.clone();
                    found_keys |= 2;
                }
                VsfType::kn(b) => {
                    offer.ntru701_public = b.clone();
                    found_keys |= 4;
                }
                VsfType::kl(b) => {
                    offer.mceliece_public = b.clone();
                    found_keys |= 8;
                }
                VsfType::kh(b) => {
                    offer.hqc256_public = b.clone();
                    found_keys |= 16;
                }
                VsfType::kk(b) => {
                    offer.secp256k1_public = b.clone();
                    found_keys |= 32;
                }
                // P-curves disambiguated by size: P-384 = 97B, P-256 = 65B
                VsfType::kp(b) if b.len() == 97 => {
                    offer.p384_public = b.clone();
                    found_keys |= 64;
                }
                VsfType::kp(b) if b.len() == 65 => {
                    offer.p256_public = b.clone();
                    found_keys |= 128;
                }
                // Hit any shared secret type, stop parsing keys
                VsfType::ksx(_)
                | VsfType::ksp(_)
                | VsfType::ksk(_)
                | VsfType::ksf(_)
                | VsfType::ksn(_)
                | VsfType::ksl(_)
                | VsfType::ksh(_)
                | VsfType::ksm(_) => break,
                _ => {}
            }
        }

        if found_keys != 255 {
            return Err(StorageError::Parse(format!(
                "Missing offer keys, found mask: {:#010b}",
                found_keys
            )));
        }
        slot.offer = Some(offer);
    }

    // Parse typed shared secrets (ksx, ksp, ksk, ksf, ksn, ksl, ksh) Secrets come in groups of 8, first group is "from_them", second is "to_them"
    if has_from || has_to {
        let secrets: Vec<&VsfType> = values[data_start..]
            .iter()
            .filter(|v| {
                matches!(
                    v,
                    VsfType::ksx(_)
                        | VsfType::ksp(_)
                        | VsfType::ksk(_)
                        | VsfType::ksf(_)
                        | VsfType::ksn(_)
                        | VsfType::ksl(_)
                        | VsfType::ksh(_)
                        | VsfType::ksm(_)
                )
            })
            .collect();

        let secrets_per_group = 8;

        if has_from {
            if secrets.len() < secrets_per_group {
                return Err(StorageError::Parse("Missing from_them secrets".into()));
            }
            slot.kem_secrets_from_them =
                Some(parse_typed_secrets_group(&secrets[..secrets_per_group])?);
        }

        if has_to {
            let start = if has_from { secrets_per_group } else { 0 };
            if secrets.len() < start + secrets_per_group {
                return Err(StorageError::Parse("Missing to_them secrets".into()));
            }
            slot.kem_secrets_to_them = Some(parse_typed_secrets_group(
                &secrets[start..start + secrets_per_group],
            )?);
        }
    }

    // Parse KEM response for resend (cf, cn, cl, ch ciphertexts + hb prefix + kx, kp, kk, kp ephemerals)
    if has_resend {
        slot.kem_response_for_resend = parse_kem_response_payload(&values[data_start..])?;
    }

    Ok(slot)
}

/// Parse a group of 8 typed shared secrets (ksx, ksp, ksk, ksf, ksn, ksl, ksh). Secrets are now typed, so we extract by VsfType variant. Order in file: x25519, p384, secp256k1, p256, frodo, ntru, mceliece, hqc
fn parse_typed_secrets_group(secrets: &[&VsfType]) -> Result<ClutchKemSharedSecrets, StorageError> {
    if secrets.len() != 8 {
        return Err(StorageError::Parse(format!(
            "Expected 8 secrets, got {}",
            secrets.len()
        )));
    }

    // Extract each secret by expected type and position
    let x25519 = match secrets[0] {
        VsfType::ksx(b) if b.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(b);
            arr
        }
        _ => {
            return Err(StorageError::Parse(
                "x25519 secret missing or wrong type".into(),
            ))
        }
    };

    let p384 = match secrets[1] {
        VsfType::ksp(b) => b.clone(),
        _ => {
            return Err(StorageError::Parse(
                "p384 secret missing or wrong type".into(),
            ))
        }
    };

    let secp256k1 = match secrets[2] {
        VsfType::ksk(b) => b.clone(),
        _ => {
            return Err(StorageError::Parse(
                "secp256k1 secret missing or wrong type".into(),
            ))
        }
    };

    let p256 = match secrets[3] {
        VsfType::ksp(b) => b.clone(),
        _ => {
            return Err(StorageError::Parse(
                "p256 secret missing or wrong type".into(),
            ))
        }
    };

    let frodo = match secrets[4] {
        VsfType::ksf(b) => b.clone(),
        _ => {
            return Err(StorageError::Parse(
                "frodo secret missing or wrong type".into(),
            ))
        }
    };

    let ntru = match secrets[5] {
        VsfType::ksn(b) => b.clone(),
        _ => {
            return Err(StorageError::Parse(
                "ntru secret missing or wrong type".into(),
            ))
        }
    };

    let mceliece = match secrets[6] {
        VsfType::ksl(b) => b.clone(),
        _ => {
            return Err(StorageError::Parse(
                "mceliece secret missing or wrong type".into(),
            ))
        }
    };

    let hqc = match secrets[7] {
        VsfType::ksh(b) => b.clone(),
        _ => {
            return Err(StorageError::Parse(
                "hqc secret missing or wrong type".into(),
            ))
        }
    };

    Ok(ClutchKemSharedSecrets {
        x25519,
        p384,
        secp256k1,
        p256,
        frodo,
        ntru,
        mceliece,
        hqc,
    })
}

/// Parse ClutchKemResponsePayload from values (for resend persistence). Format: v('f',...), v('n',...), v('l',...), v('h',...) ciphertexts
///       + v('t',...) target prefix + v('x',...), v('3',...), v('k',...), v('2',...) ephemerals
fn parse_kem_response_payload(
    values: &[VsfType],
) -> Result<Option<ClutchKemResponsePayload>, StorageError> {
    // Extract ciphertexts and ephemerals by v() marker byte
    let mut frodo_ct = None;
    let mut ntru_ct = None;
    let mut mceliece_ct = None;
    let mut hqc_ct = None;
    let mut target_prefix = None;
    let mut x25519_eph = None;
    let mut p384_eph = None;
    let mut secp256k1_eph = None;
    let mut p256_eph = None;

    for v in values {
        if let VsfType::v(marker, data) = v {
            match marker {
                b'f' => frodo_ct = Some(data.clone()),
                b'n' => ntru_ct = Some(data.clone()),
                b'l' => mceliece_ct = Some(data.clone()),
                b'h' => hqc_ct = Some(data.clone()),
                b't' if data.len() == 8 => {
                    let mut arr = [0u8; 8];
                    arr.copy_from_slice(data);
                    target_prefix = Some(arr);
                }
                b'x' if data.len() == 32 => {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(data);
                    x25519_eph = Some(arr);
                }
                b'3' => p384_eph = Some(data.clone()),
                b'k' => secp256k1_eph = Some(data.clone()),
                b'2' => p256_eph = Some(data.clone()),
                _ => {}
            }
        }
    }

    // All fields required for a valid resend payload
    if let (
        Some(frodo),
        Some(ntru),
        Some(mce),
        Some(hqc),
        Some(prefix),
        Some(x25519),
        Some(p384),
        Some(secp),
        Some(p256),
    ) = (
        frodo_ct,
        ntru_ct,
        mceliece_ct,
        hqc_ct,
        target_prefix,
        x25519_eph,
        p384_eph,
        secp256k1_eph,
        p256_eph,
    ) {
        Ok(Some(ClutchKemResponsePayload {
            frodo976_ciphertext: frodo,
            ntru701_ciphertext: ntru,
            mceliece_ciphertext: mce,
            hqc256_ciphertext: hqc,
            target_hqc_pub_prefix: prefix,
            x25519_ephemeral: x25519,
            p384_ephemeral: p384,
            secp256k1_ephemeral: secp,
            p256_ephemeral: p256,
        }))
    } else {
        // Missing fields - no resend payload
        Ok(None)
    }
}

/// Delete CLUTCH slots (called after ceremony completes)
pub fn delete_clutch_slots(handle: &str, storage: &FlatStorage) -> Result<(), StorageError> {
    let their_identity_seed = derive_identity_seed(handle);
    storage.delete_addr(&contact_key(&their_identity_seed, "slots"))?;
    #[cfg(feature = "development")]
    crate::log(&format!("STORAGE: Deleted CLUTCH slots for {}", handle));
    Ok(())
}

// ============================================================================
// Message Storage — rārangi conversation rows ============================================================================
//
// Messages are conversation *content*, not contact state, so they live in the rārangi conversation DB rather than as a per-peer blob in the vault. Each conversation is one byte-keyed rārangi table addressed by its `friendship_id` (deterministic from the sorted participant seeds, so the same conversation resolves to the same table on every participant's — and every fleet — device). Each message is one row keyed by a monotonic counter (`Pk::Int(0)`, `1`, `2`, …): a conversation is an ordered sequence delivered in the same order everywhere, so message N is message N on every device, and the catalog gives chronological `list_in` for free.

use crate::types::ChatMessage;
use rarangi::{Db, Pk, Record, Value};

/// The conversation id (rārangi table) for the 1:1 between us and `their_identity_seed`. Derived early from the two participant seeds — `FriendshipId::derive` is deterministic and needs no completed CLUTCH ceremony, so messages are always conversation-keyed. Group/fleet conversations derive the same way from their full sorted participant set.
fn conversation_id(my_seed: &[u8; 32], their_identity_seed: &[u8; 32]) -> [u8; 32] {
    *FriendshipId::derive(&[*my_seed, *their_identity_seed]).as_bytes()
}

/// Save a contact's messages as rows in the conversation table. Idempotent: each message is written at its sequence index, so re-saving the same history overwrites row-for-row identically.
pub fn save_messages(contact: &Contact, storage: &FlatStorage) -> Result<(), StorageError> {
    if contact.messages.is_empty() {
        return Ok(()); // Nothing to save
    }

    let their_identity_seed = derive_identity_seed(contact.handle.as_str());
    let table = conversation_id(storage.vault_seed(), &their_identity_seed);

    let mut db = Db::open(storage).map_err(|e| StorageError::Vault(e.to_string()))?;
    for (i, msg) in contact.messages.iter().enumerate() {
        let rec = Record::new()
            .set("content", msg.content.clone())
            .set("timestamp", Value::Time(msg.timestamp))
            .set("is_outgoing", msg.is_outgoing as u64)
            .set("delivered", msg.delivered as u64);
        db.put_row_in(&table, Pk::Int(i as u64), &rec)
            .map_err(|e| StorageError::Vault(e.to_string()))?;
    }

    #[cfg(feature = "development")]
    crate::log(&format!(
        "STORAGE: Saved {} messages for {}",
        contact.messages.len(),
        contact.handle.as_str()
    ));

    Ok(())
}

/// Load a contact's messages from the conversation table, in counter order (which is chronological).
pub fn load_messages(contact: &mut Contact, storage: &FlatStorage) -> Result<(), StorageError> {
    let their_identity_seed = derive_identity_seed(contact.handle.as_str());
    let table = conversation_id(storage.vault_seed(), &their_identity_seed);

    let db = Db::open(storage).map_err(|e| StorageError::Vault(e.to_string()))?;
    let pks = db.list_in(&table).map_err(|e| StorageError::Vault(e.to_string()))?;

    contact.messages.clear();
    for pk in pks {
        let Some(rec) = db
            .get_row_in(&table, pk.clone())
            .map_err(|e| StorageError::Vault(e.to_string()))?
        else {
            continue;
        };
        let Some(content) = rec.text("content") else { continue };
        contact.messages.push(ChatMessage {
            content: content.to_string(),
            timestamp: rec.time("timestamp").unwrap_or(0),
            is_outgoing: rec.uint("is_outgoing").unwrap_or(0) != 0,
            delivered: rec.uint("delivered").unwrap_or(0) != 0,
        });
    }

    #[cfg(feature = "development")]
    crate::log(&format!(
        "STORAGE: Loaded {} messages for {}",
        contact.messages.len(),
        contact.handle.as_str()
    ));

    Ok(())
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
                VsfType::hP(identity.handle_proof.to_vec()),
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
            VsfType::hP(v) if v.len() == 32 => v.as_slice().try_into().unwrap(),
            _ => panic!("Expected hP"),
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

    /// Messages round-trip through `save_messages`/`load_messages` on a REAL encrypted vault: write three, close the vault, reopen from disk, read them back in order. Proves the rārangi conversation-row path end to end, not just in RAM.
    #[test]
    fn messages_round_trip_on_real_vault() {
        use crate::types::HandleText;

        let device_secret = [29u8; 32];
        let vault_seed = *ihi::handle_to_hash("me-messages-test").as_bytes();
        let app = crate::storage::APP;

        let mut contact = Contact::new(HandleText::new("bob"), [3u8; 32], DevicePubkey::from_bytes([0u8; 32]));
        contact.messages = vec![
            ChatMessage { content: "hi".to_string(), timestamp: 100, is_outgoing: true, delivered: true },
            ChatMessage { content: "hey".to_string(), timestamp: 200, is_outgoing: false, delivered: false },
            ChatMessage { content: "👋 unicode".to_string(), timestamp: 300, is_outgoing: true, delivered: false },
        ];

        // session 1: save, then drop the vault (closes the on-disk files)
        {
            let storage = FlatStorage::new(app, vault_seed, device_secret).unwrap();
            save_messages(&contact, &storage).unwrap();
        }

        // session 2: reopen from disk, load into a fresh contact
        let storage = FlatStorage::new(app, vault_seed, device_secret).unwrap();
        let mut loaded = Contact::new(HandleText::new("bob"), [3u8; 32], DevicePubkey::from_bytes([0u8; 32]));
        load_messages(&mut loaded, &storage).unwrap();

        assert_eq!(loaded.messages.len(), 3);
        assert_eq!(loaded.messages[0].content, "hi");
        assert_eq!(loaded.messages[0].timestamp, 100);
        assert!(loaded.messages[0].is_outgoing && loaded.messages[0].delivered);
        assert_eq!(loaded.messages[1].content, "hey");
        assert!(!loaded.messages[1].is_outgoing && !loaded.messages[1].delivered);
        assert_eq!(loaded.messages[2].content, "👋 unicode");

        // Clean up the on-disk vault so reruns start fresh.
        if let Ok([primary, shadow]) = kete::vault_ring_paths(app, &vault_seed, &device_secret) {
            let _ = std::fs::remove_file(primary);
            let _ = std::fs::remove_file(shadow);
        }
    }
}
