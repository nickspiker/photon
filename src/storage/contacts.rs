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

use crate::storage::{FlatStorage, StorageError};
use crate::types::{
    ClutchState, Contact, ContactId, DevicePubkey, FriendshipId, HandleText, Seed, TrustLevel,
};
use vsf::schema::{SectionBuilder, SectionSchema, TypeConstraint};
use vsf::types::EagleTime;
use vsf::VsfType;

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
    crate::types::Handle::to_identity_seed(handle)
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

    storage.write_addr(
        &crate::storage::vault_key("contacts", storage.vault_seed()),
        &vsf_bytes,
    )
}

/// Load the contact list from encrypted index with schema validation
pub fn load_contact_list(storage: &FlatStorage) -> Result<Vec<ContactIdentity>, StorageError> {
    let vsf_bytes =
        match storage.read_addr(&crate::storage::vault_key("contacts", storage.vault_seed()))? {
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
        .field("chain_woven", TypeConstraint::AnyUnsigned) // bool: chain proven end-to-end once (double-toggle seal) — persists so an established conversation allows composing (+ the staging queue) across restarts, even to an offline peer
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
    if contact.chain_woven {
        // Persist the seal only once true — absent reads as false (unwoven), so old vaults and fresh ceremonies re-prove as before.
        builder = builder
            .set("chain_woven", true)
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

    // Schema-validated parse — the same contact_state_schema the writer encodes with, so reader and writer can no longer drift. Typed extraction is width-tolerant (the old hand-match on u3 broke if the writer ever emitted a wider uint).
    let section = SectionBuilder::parse(contact_state_schema(), &vsf_bytes)
        .map_err(|e| StorageError::Parse(format!("Contact state parse: {}", e)))?;

    // Required fields
    let clutch_u8 = section.get_value::<u8>("clutch_state").unwrap_or(0);
    let trust_u8 = section.get_value::<u8>("trust_level").unwrap_or(0);
    let pubkey_bytes: [u8; 32] = section
        .get_value::<[u8; 32]>("pubkey")
        .map_err(|_| StorageError::Parse("Missing pubkey".into()))?;
    let added = section
        .get_fields("added")
        .first()
        .and_then(|f| f.values.first())
        .map(vsf_to_oscillations)
        .unwrap_or(0);

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
    if let Ok(s) = section.get_value::<String>("ip") {
        contact.ip = s.parse().ok();
    }
    if let Ok(seed) = section.get_value::<[u8; 32]>("seed") {
        contact.relationship_seed = Some(Seed::from_bytes(seed));
    }
    if let Ok(fid) = section.get_value::<[u8; 32]>("friendship_id") {
        contact.friendship_id = Some(FriendshipId::from_bytes(fid));
    }
    if let Some(v) = section.get_fields("last_seen").first().and_then(|f| f.values.first()) {
        contact.last_seen = Some(vsf_to_oscillations(v));
    }
    if let Ok(id) = section.get_value::<[u8; 32]>("id") {
        contact.id = ContactId::from_bytes(id);
    }
    if let Ok(prefix) = section.get_value::<Vec<u8>>("completed_their_hqc_prefix") {
        if prefix.len() == 8 {
            contact.completed_their_hqc_prefix = prefix.as_slice().try_into().ok();
        }
    }
    // Chain-weave seal: a chain proven once stays proven across restarts, so composing (+ the staging queue) works even while the peer is offline. Set the probe flags coherently so the one-shot probe doesn't refire into an already-proven chain.
    if section.get_value::<bool>("chain_woven").unwrap_or(false) {
        contact.chain_woven = true;
        contact.probe_sent = true;
        contact.their_probe_seen = true;
        contact.chain_advanced_by_ack = true;
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

/// Memory-only no-op. CLUTCH keypairs are ephemeral ceremony scratch (~600KB, McEliece-heavy); persisting them grew the durable dual-mirror vault and the fallocate/zero grow froze the UI mid-ceremony. `contact.clutch_our_keypairs` is the sole source of truth; a mid-ceremony restart re-runs the off-thread (Min-priority) keygen. Retained as a no-op so call sites stay uniform.
pub fn save_clutch_keypairs(
    _keypairs: &ClutchAllKeypairs,
    _their_identity_seed: &[u8; 32],
    _storage: &FlatStorage,
) -> Result<(), StorageError> {
    Ok(())
}

/// Memory-only no-op (see [`save_clutch_keypairs`]): nothing is persisted, so this always reports "no keypairs" and the caller re-runs the off-thread keygen.
pub fn load_clutch_keypairs(
    _their_identity_seed: &[u8; 32],
    _storage: &FlatStorage,
) -> Result<Option<ClutchAllKeypairs>, StorageError> {
    Ok(None)
}

/// Memory-only no-op (see [`save_clutch_keypairs`]): nothing was persisted, so there is nothing to delete — and no vault grow to freeze the UI.
pub fn delete_clutch_keypairs(
    _their_identity_seed: &[u8; 32],
    _storage: &FlatStorage,
) -> Result<(), StorageError> {
    Ok(())
}

// ============================================================================
// CLUTCH Slots Storage (ceremony progress - offers, KEM secrets) ============================================================================

use crate::types::PartySlot;

/// Memory-only no-op. CLUTCH slots are ephemeral ceremony scratch (McEliece/Frodo KEM material, hundreds of KB); persisting them grew the durable dual-mirror vault, and that grow — fallocate + zero + fsync on both mirrors — was the multi-second UI freeze mid-ceremony. `contact.clutch_slots` is the sole source of truth; a mid-ceremony restart re-inits and re-runs CLUTCH. Retained as a no-op so call sites stay uniform.
pub fn save_clutch_slots(
    _slots: &[PartySlot],
    _offer_provenances: &[[u8; 32]],
    _ceremony_id: Option<[u8; 32]>,
    _their_identity_seed: &[u8; 32],
    _storage: &FlatStorage,
) -> Result<(), StorageError> {
    Ok(())
}

/// Loaded CLUTCH ceremony state
pub struct ClutchCeremonyState {
    pub slots: Vec<PartySlot>,
    pub offer_provenances: Vec<[u8; 32]>,
    pub ceremony_id: Option<[u8; 32]>,
}

/// Memory-only no-op (see [`save_clutch_slots`]): nothing is persisted, so this always reports "no slots" and the caller re-inits the ceremony.
pub fn load_clutch_slots(
    _their_identity_seed: &[u8; 32],
    _storage: &FlatStorage,
) -> Result<Option<ClutchCeremonyState>, StorageError> {
    Ok(None)
}

// (The hand-rolled PartySlot/secrets/KEM-payload parsers that lived here were dead code — CLUTCH slot persistence became a memory-only no-op, see save_clutch_slots — and were removed in the vault schema-parse sweep.)

/// Memory-only no-op (see [`save_clutch_slots`]): nothing was persisted, so there is nothing to delete — this was the observed 3.67s UI freeze (the delete tripped a vault grow), now gone.
pub fn delete_clutch_slots(
    _their_identity_seed: &[u8; 32],
    _storage: &FlatStorage,
) -> Result<(), StorageError> {
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

    // Contact already carries the identity seed (handle_hash = BLAKE3(handle)); use it directly rather than re-deriving from the plaintext handle string. Identity flows as the seed, never the handle, past the contact boundary.
    let table = conversation_id(storage.vault_seed(), &contact.handle_hash);

    let mut db = Db::open(storage).map_err(|e| StorageError::Vault(e.to_string()))?;
    for msg in contact.messages.iter() {
        // Key each row by the message's eagle_time, NOT a local enumerate index. eagle_time is monotonic (a clock) so it's stable + shared across both devices (the renumber-on-insert hazard of an index key is gone), it's the braid's weave reference, and Pk::Int encodes big-endian so key order == chronological. eagle_time is i64 but always positive (oscillations since Apollo 11), so `as u64` is safe and order-preserving.
        // `content_hash` = blake3 of the message text, stored so the braid's eagle_time->text weave lookup has an integrity/tiebreak check (the adversarial multi-device-same-tick case).
        let content_hash = blake3::hash(msg.content.as_bytes());
        let mut rec = Record::new()
            .set("content", msg.content.clone())
            .set("timestamp", Value::Time(msg.timestamp))
            .set("is_outgoing", msg.is_outgoing as u64)
            .set("delivered", msg.delivered as u64)
            .set("content_hash", content_hash.as_bytes().to_vec());
        // ack_hash: the plaintext_hash we ACK a RECEIVED message with — persisted so a duplicate retransmit can be re-ACKed after restart (the sender's chain stalls without a matching ACK).
        if let Some(ah) = msg.ack_hash {
            rec = rec.set("ack_hash", ah.to_vec());
        }
        db.put_row_in(&table, Pk::Int(msg.timestamp as u64), &rec)
            .map_err(|e| StorageError::Vault(e.to_string()))?;
    }

    #[cfg(feature = "development")]
    crate::log(&format!(
        "STORAGE: Saved {} messages for seed {}",
        contact.messages.len(),
        hex::encode(&contact.handle_hash[..4])
    ));

    Ok(())
}

/// Load a contact's messages from the conversation table, in counter order (which is chronological).
pub fn load_messages(contact: &mut Contact, storage: &FlatStorage) -> Result<(), StorageError> {
    // Use the contact's cached identity seed (handle_hash), not a re-derivation from the handle.
    let table = conversation_id(storage.vault_seed(), &contact.handle_hash);

    let db = Db::open(storage).map_err(|e| StorageError::Vault(e.to_string()))?;
    let pks = db
        .list_in(&table)
        .map_err(|e| StorageError::Vault(e.to_string()))?;

    contact.messages.clear();
    for pk in pks {
        let Some(rec) = db
            .get_row_in(&table, pk.clone())
            .map_err(|e| StorageError::Vault(e.to_string()))?
        else {
            continue;
        };
        let Some(content) = rec.text("content") else {
            continue;
        };
        let ack_hash: Option<[u8; 32]> = rec
            .bytes("ack_hash")
            .filter(|b| b.len() == 32)
            .map(|b| b.try_into().unwrap());
        contact.messages.push(ChatMessage {
            content: content.to_string(),
            timestamp: rec.time("timestamp").unwrap_or(0),
            is_outgoing: rec.uint("is_outgoing").unwrap_or(0) != 0,
            delivered: rec.uint("delivered").unwrap_or(0) != 0,
            ack_hash,
        });
    }

    #[cfg(feature = "development")]
    crate::log(&format!(
        "STORAGE: Loaded {} messages for seed {}",
        contact.messages.len(),
        hex::encode(&contact.handle_hash[..4])
    ));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use vsf::VsfSection;

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

        let mut contact = Contact::new(
            HandleText::new("bob"),
            [3u8; 32],
            DevicePubkey::from_bytes([0u8; 32]),
        );
        contact.messages = vec![
            ChatMessage {
                content: "hi".to_string(),
                timestamp: 100,
                is_outgoing: true,
                delivered: true,
                ack_hash: None,
            },
            ChatMessage {
                content: "hey".to_string(),
                timestamp: 200,
                is_outgoing: false,
                delivered: false,
                ack_hash: Some([0x7Au8; 32]), // received msg: its ACK hash must survive the round-trip
            },
            ChatMessage {
                content: "👋 unicode".to_string(),
                timestamp: 300,
                is_outgoing: true,
                delivered: false,
                ack_hash: None,
            },
        ];

        // session 1: save, then drop the vault (closes the on-disk files)
        {
            let storage = FlatStorage::new(app, vault_seed, device_secret).unwrap();
            save_messages(&contact, &storage).unwrap();
        }

        // session 2: reopen from disk, load into a fresh contact
        let storage = FlatStorage::new(app, vault_seed, device_secret).unwrap();
        let mut loaded = Contact::new(
            HandleText::new("bob"),
            [3u8; 32],
            DevicePubkey::from_bytes([0u8; 32]),
        );
        load_messages(&mut loaded, &storage).unwrap();

        assert_eq!(loaded.messages.len(), 3);
        assert_eq!(loaded.messages[0].content, "hi");
        assert_eq!(loaded.messages[0].timestamp, 100);
        assert!(loaded.messages[0].is_outgoing && loaded.messages[0].delivered);
        assert_eq!(loaded.messages[1].content, "hey");
        assert!(!loaded.messages[1].is_outgoing && !loaded.messages[1].delivered);
        // The received message's ack_hash must survive the round-trip (re-ACK after restart);
        // outgoing messages carry no ack_hash.
        assert_eq!(loaded.messages[1].ack_hash, Some([0x7Au8; 32]));
        assert_eq!(loaded.messages[0].ack_hash, None);
        assert_eq!(loaded.messages[2].content, "👋 unicode");
        assert_eq!(loaded.messages[2].ack_hash, None);

        // Clean up the on-disk vault so reruns start fresh.
        if let Ok([primary, shadow]) = kete::vault_ring_paths(app, &vault_seed, &device_secret) {
            let _ = std::fs::remove_file(primary);
            let _ = std::fs::remove_file(shadow);
        }
    }
}
