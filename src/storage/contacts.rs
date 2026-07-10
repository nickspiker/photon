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

// ============================================================================ Contact List (Index) - Static Identity Data (Schema-validated) ============================================================================

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

// ============================================================================ Contact State - Mutable Session Data (Schema-validated) ============================================================================

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
        .field("hist_oldest", TypeConstraint::Any) // e6 eagle-time cursor: oldest recovered row so far (i64::MAX = head page pending). Absent = history recovery never ran for this contact.
        .field("hist_complete", TypeConstraint::AnyUnsigned) // bool: friend-history backfill finished (server said no-more, or early-stop). Absent = false.
        .field("sibling", TypeConstraint::AnyUnsigned) // bool: this entry is one of OUR OWN fleet devices (fleet weave), keyed by sibling party id. Absent = false (a friend).
        .field("blind", TypeConstraint::Any) // multi-value per deposited blind: (depositor device ke, 64B blob tensor, deposited-at e6). Friend-side storage of OTP-blinded S blobs; absent = none.
        .field("blind_deposited", TypeConstraint::AnyUnsigned) // bool: OUR blind is disk-confirmed at this friend (their blind_ack arrived). Absent = false.
        .field("fleet_member", TypeConstraint::Ed25519Key) // multi-value: one folded member device pubkey. Absent = empty folded set (bootstrap).
        .field("fleet_folded_once", TypeConstraint::AnyUnsigned) // bool: chain folded ≥1 time (arms members-only trust). Absent = false (bootstrap).
        .field("fleet_members_ts", TypeConstraint::Any) // e6: chain-tip eagle time of last adopted fold (monotonic floor). Absent = 0.
}

/// Save contact state (mutable data) with schema validation
pub fn save_contact_state(contact: &Contact, storage: &FlatStorage) -> Result<(), StorageError> {
    // Key the state entry off the contact's party id (`handle_hash`), not a re-derivation from the handle string. For friends the two are equal by construction (`Contact::new`), so this is a no-op; for fleet siblings the party id is device-derived (`sibling_party_id`) — deriving from the handle would collide every sibling AND the self-contact onto one state entry.
    let identity_seed = contact.handle_hash;

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
    // History-recovery cursor: persisted whenever recovery has run, so backfill resumes across restarts without waiting for a fresh weave-seal. Absent = feature never touched this contact.
    if let Some(rec) = &contact.history_recovery {
        builder = builder
            .set(
                "hist_oldest",
                VsfType::e(vsf::types::EtType::e6(rec.oldest_recovered_osc)),
            )
            .map_err(|e| StorageError::Parse(e.to_string()))?;
        if rec.complete {
            builder = builder
                .set("hist_complete", true)
                .map_err(|e| StorageError::Parse(e.to_string()))?;
        }
    }
    if contact.is_sibling {
        // Self-describing sibling marker — written only when true (absent = friend), so old vaults parse unchanged.
        builder = builder
            .set("sibling", true)
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    // Friend-side blind deposits: one multi-value field per (device, blob, at) — the contact_list "contact" idiom. Blobs are OTP ciphertexts (provably opaque), safe at rest like any other vault entry.
    for (dev, blob, at) in &contact.deposited_blinds {
        builder = builder
            .append_multi(
                "blind",
                vec![
                    VsfType::ke(dev.to_vec()),
                    VsfType::t_u3(vsf::Tensor::new(vec![blob.len()], blob.clone())),
                    VsfType::e(vsf::types::EtType::e6(*at)),
                ],
            )
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    if contact.blind_deposited {
        builder = builder
            .set("blind_deposited", true)
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    // Folded fleet: persist the adopted member set + the armed flag + the tip ts, so a restart resumes fold-respecting trust immediately (no bootstrap regression, no trust-nobody window). One multi-value field per member device (the `blind` idiom).
    for m in &contact.fleet_members {
        builder = builder
            .append_multi("fleet_member", vec![VsfType::ke(m.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    if contact.fleet_folded_once {
        builder = builder
            .set("fleet_folded_once", true)
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    if contact.fleet_members_ts != 0 {
        builder = builder
            .set(
                "fleet_members_ts",
                VsfType::e(vsf::types::EtType::e6(contact.fleet_members_ts)),
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

    let pubkey = state_pubkey(&vsf_bytes)?;
    let mut contact = Contact::new(
        HandleText::new(&identity.handle),
        identity.handle_proof,
        pubkey,
    );
    apply_contact_state(&mut contact, &vsf_bytes)?;

    Ok(contact)
}

/// Extract just the device pubkey from an encoded contact-state blob (needed before the Contact can be constructed).
fn state_pubkey(vsf_bytes: &[u8]) -> Result<DevicePubkey, StorageError> {
    let section = SectionBuilder::parse(contact_state_schema(), vsf_bytes)
        .map_err(|e| StorageError::Parse(format!("Contact state parse: {}", e)))?;
    let pubkey_bytes: [u8; 32] = section
        .get_value::<[u8; 32]>("pubkey")
        .map_err(|_| StorageError::Parse("Missing pubkey".into()))?;
    Ok(DevicePubkey::from_bytes(pubkey_bytes))
}

/// Apply a parsed contact-state blob onto a freshly-constructed Contact (friend via `Contact::new`, sibling via `Contact::new_sibling`). Shared by both loaders so the field set can't drift between them.
fn apply_contact_state(contact: &mut Contact, vsf_bytes: &[u8]) -> Result<(), StorageError> {
    // Schema-validated parse — the same contact_state_schema the writer encodes with, so reader and writer can no longer drift. Typed extraction is width-tolerant (the old hand-match on u3 broke if the writer ever emitted a wider uint).
    let section = SectionBuilder::parse(contact_state_schema(), vsf_bytes)
        .map_err(|e| StorageError::Parse(format!("Contact state parse: {}", e)))?;

    // Required fields
    let clutch_u8 = section.get_value::<u8>("clutch_state").unwrap_or(0);
    let trust_u8 = section.get_value::<u8>("trust_level").unwrap_or(0);
    let added = section
        .get_fields("added")
        .first()
        .and_then(|f| f.values.first())
        .map(vsf_to_oscillations)
        .unwrap_or(0);

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
    // History-recovery cursor: reconstruct the runtime state machine so an incomplete backfill resumes on the next drive_history_recovery pass (next_request_osc = 0 → immediately eligible; urgent stays false — resume is background work).
    if let Some(v) = section.get_fields("hist_oldest").first().and_then(|f| f.values.first()) {
        let oldest = vsf_to_oscillations(v);
        let complete = section.get_value::<bool>("hist_complete").unwrap_or(false);
        contact.history_recovery = Some(crate::types::HistoryRecovery {
            oldest_recovered_osc: oldest,
            complete,
            in_flight: None,
            next_request_osc: 0,
            urgent: false,
            was_complete_before: complete,
        });
    }
    // Friend-side blind deposits: (device ke, blob tensor, at e6) per multi-value field.
    for field in section.get_fields("blind") {
        if field.values.len() >= 3 {
            let dev: [u8; 32] = match &field.values[0] {
                VsfType::ke(v) if v.len() == 32 => v.as_slice().try_into().unwrap(),
                _ => continue,
            };
            let blob = match &field.values[1] {
                VsfType::t_u3(t) => t.data.clone(),
                _ => continue,
            };
            let at = vsf_to_oscillations(&field.values[2]);
            contact.deposited_blinds.push((dev, blob, at));
        }
    }
    if section.get_value::<bool>("blind_deposited").unwrap_or(false) {
        contact.blind_deposited = true;
    }
    // Folded fleet: restore the adopted set + arm flag + tip ts. Order-independent — fleet_folded_once=true makes knows_device members-only immediately on load. All absent (old vault) = empty set + false + 0 = bootstrap.
    for field in section.get_fields("fleet_member") {
        if let Some(VsfType::ke(v)) = field.values.first() {
            if v.len() == 32 {
                if let Ok(arr) = <[u8; 32]>::try_from(v.as_slice()) {
                    contact.fleet_members.push(arr);
                }
            }
        }
    }
    if section.get_value::<bool>("fleet_folded_once").unwrap_or(false) {
        contact.fleet_folded_once = true;
    }
    if let Some(v) = section.get_fields("fleet_members_ts").first().and_then(|f| f.values.first()) {
        contact.fleet_members_ts = vsf_to_oscillations(v);
    }

    Ok(())
}

// ============================================================================ Sibling Index — own-fleet devices (fleet weave) ============================================================================

/// Schema for the sibling index: one `device` field per sibling device pubkey. Siblings can't live in the contacts index — it's keyed by handle string and dedups on it, so every sibling (sharing OUR handle) would collapse into one entry.
fn sibling_list_schema() -> SectionSchema {
    SectionSchema::new("sibling_list").field("device", TypeConstraint::Ed25519Key)
}

/// Save the sibling device-pubkey index at `vault_key("siblings", vault_seed)`.
pub fn save_sibling_list(devices: &[[u8; 32]], storage: &FlatStorage) -> Result<(), StorageError> {
    let schema = sibling_list_schema();
    let mut builder = schema.build();
    for d in devices {
        builder = builder
            .append_multi("device", vec![VsfType::ke(d.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    let vsf_bytes = builder
        .encode()
        .map_err(|e| StorageError::Parse(e.to_string()))?;
    storage.write_addr(
        &crate::storage::vault_key("siblings", storage.vault_seed()),
        &vsf_bytes,
    )
}

/// Load the sibling device-pubkey index. Missing entry = empty fleet knowledge (single-device or pre-feature vault).
pub fn load_sibling_list(storage: &FlatStorage) -> Result<Vec<[u8; 32]>, StorageError> {
    let vsf_bytes =
        match storage.read_addr(&crate::storage::vault_key("siblings", storage.vault_seed()))? {
            Some(b) => b,
            None => return Ok(Vec::new()),
        };
    let builder = SectionBuilder::parse(sibling_list_schema(), &vsf_bytes)
        .map_err(|e| StorageError::Parse(format!("Sibling list parse: {}", e)))?;
    let mut devices = Vec::new();
    for field in builder.get_fields("device") {
        if let Some(VsfType::ke(v)) = field.values.first() {
            if v.len() == 32 {
                devices.push(v.as_slice().try_into().unwrap());
            }
        }
    }
    Ok(devices)
}

/// Load all persisted fleet-sibling contacts: walk the sibling index, rebuild each via `Contact::new_sibling` (party id re-derived from the device pubkey), then apply its saved state. A missing state entry yields a fresh Pending sibling — the ceremony machinery re-runs CLUTCH.
pub fn load_all_siblings(
    our_handle: &str,
    our_handle_proof: [u8; 32],
    storage: &FlatStorage,
) -> Vec<Contact> {
    let devices = match load_sibling_list(storage) {
        Ok(d) => d,
        Err(e) => {
            crate::log(&format!("Failed to load sibling list: {}", e));
            return Vec::new();
        }
    };

    let mut siblings = Vec::new();
    for device in devices {
        let mut c = Contact::new_sibling(
            HandleText::new(our_handle),
            our_handle_proof,
            DevicePubkey::from_bytes(device),
        );
        match storage.read_addr(&contact_key(&c.handle_hash, "state")) {
            Ok(Some(vsf_bytes)) => {
                if let Err(e) = apply_contact_state(&mut c, &vsf_bytes) {
                    crate::log(&format!(
                        "Failed to parse sibling state for device {}: {}",
                        hex::encode(&device[..4]),
                        e
                    ));
                }
            }
            Ok(None) => {} // Fresh Pending sibling — ceremony re-runs
            Err(e) => {
                crate::log(&format!(
                    "Failed to read sibling state for device {}: {}",
                    hex::encode(&device[..4]),
                    e
                ));
            }
        }
        // The applied state's stored pubkey/id equal the index-derived ones by construction; the sibling flag is authoritative from new_sibling, not the blob.
        c.is_sibling = true;
        siblings.push(c);
    }
    siblings
}

/// Remove a sibling from the index and delete its per-device vault entries. Called when the fold drops a device (revocation hygiene). Chains are deleted by the caller (they're keyed by friendship_id, which the caller holds).
pub fn delete_sibling(device_pubkey: &[u8; 32], storage: &FlatStorage) -> Result<(), StorageError> {
    let pid = crate::crypto::clutch::sibling_party_id(device_pubkey);
    delete_contact(&pid, storage)?;
    let mut list = load_sibling_list(storage).unwrap_or_default();
    let before = list.len();
    list.retain(|d| d != device_pubkey);
    if list.len() != before {
        save_sibling_list(&list, storage)?;
    }
    Ok(())
}

// ============================================================================ High-Level API ============================================================================

/// Save a contact (updates both list and state). Siblings go to the sibling index; friends to the contacts index — a sibling must never enter the contacts index (its handle-string dedup would collapse all siblings into the self entry).
pub fn save_contact(contact: &Contact, storage: &FlatStorage) -> Result<(), StorageError> {
    // Save state file
    save_contact_state(contact, storage)?;

    if contact.is_sibling {
        let mut list = load_sibling_list(storage).unwrap_or_default();
        if !list.contains(&contact.public_identity.key) {
            list.push(contact.public_identity.key);
            save_sibling_list(&list, storage)?;
        }
        return Ok(());
    }

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

/// Delete a contact's per-peer entries from the vault. Conversation messages are NOT deleted here — they live in the rārangi conversation DB keyed by `friendship_id` (a conversation can outlive removing one party from contacts), and are reaped thru that layer.
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

// ============================================================================ CLUTCH Keypairs Storage (~600KB, stored separately) ============================================================================

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

// ============================================================================ CLUTCH Slots Storage (ceremony progress - offers, KEM secrets) ============================================================================

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

// ============================================================================ Message Storage — rārangi conversation rows ============================================================================
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
        // Key each row by the message's eagle_time, NOT a local enumerate index. eagle_time is monotonic (a clock) so it's stable + shared across both devices (the renumber-on-insert hazard of an index key is gone), it's the braid's weave reference, and Pk::Int encodes big-endian so key order == chronological. eagle_time is i64 but always positive (oscillations since Apollo 11), so `as u64` is safe and order-preserving. `content_hash` = blake3 of the message text, stored so the braid's eagle_time->text weave lookup has an integrity/tiebreak check (the adversarial multi-device-same-tick case).
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
        // recovered: friend-attested provenance flag — written only when true (absent = false), matching the contact-state optional-field idiom.
        if msg.recovered {
            rec = rec.set("recovered", 1u64);
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

    // Sort keys numerically — the catalog yields INSERTION order, which matched chronological order only while rows were appended live. History recovery inserts OLDER rows later, so trusting insertion order would interleave the conversation. Key = eagle_time, so numeric sort = time sort.
    let mut keys: Vec<u64> = pks
        .into_iter()
        .filter_map(|pk| match pk {
            Pk::Int(t) => Some(t),
            _ => None,
        })
        .collect();
    keys.sort_unstable();

    contact.messages.clear();
    for key in keys {
        let Some(rec) = db
            .get_row_in(&table, Pk::Int(key))
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
            recovered: rec.uint("recovered").unwrap_or(0) != 0,
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

/// Persist ONLY the given rows into the conversation table (same field layout as [`save_messages`]). History recovery lands pages of ~50 rows at a time — rewriting the whole conversation per page would be O(n) per page; this is O(page).
pub fn save_messages_page(
    their_identity_seed: &[u8; 32],
    msgs: &[ChatMessage],
    storage: &FlatStorage,
) -> Result<(), StorageError> {
    if msgs.is_empty() {
        return Ok(());
    }
    let table = conversation_id(storage.vault_seed(), their_identity_seed);
    let mut db = Db::open(storage).map_err(|e| StorageError::Vault(e.to_string()))?;
    for msg in msgs {
        let content_hash = blake3::hash(msg.content.as_bytes());
        let mut rec = Record::new()
            .set("content", msg.content.clone())
            .set("timestamp", Value::Time(msg.timestamp))
            .set("is_outgoing", msg.is_outgoing as u64)
            .set("delivered", msg.delivered as u64)
            .set("content_hash", content_hash.as_bytes().to_vec());
        if let Some(ah) = msg.ack_hash {
            rec = rec.set("ack_hash", ah.to_vec());
        }
        if msg.recovered {
            rec = rec.set("recovered", 1u64);
        }
        db.put_row_in(&table, Pk::Int(msg.timestamp as u64), &rec)
            .map_err(|e| StorageError::Vault(e.to_string()))?;
    }
    Ok(())
}

/// Serve one newest-first history page: the newest `max_rows` rows strictly OLDER than `before_osc` (pass `i64::MAX` for the head page), bounded by `max_bytes` of summed content. Returns the rows in ascending time order plus `more` = whether older rows remain below the returned page. The catalog scan is O(n) in conversation size — fine to ~10⁵ rows; a rārangi range index is a later optimization.
pub fn load_message_page_before(
    their_identity_seed: &[u8; 32],
    before_osc: i64,
    max_rows: usize,
    max_bytes: usize,
    storage: &FlatStorage,
) -> Result<(Vec<ChatMessage>, bool), StorageError> {
    let table = conversation_id(storage.vault_seed(), their_identity_seed);
    let db = Db::open(storage).map_err(|e| StorageError::Vault(e.to_string()))?;
    let pks = db
        .list_in(&table)
        .map_err(|e| StorageError::Vault(e.to_string()))?;

    // All keys strictly older than the cursor, ascending.
    let before = if before_osc <= 0 { 0u64 } else { before_osc as u64 };
    let mut keys: Vec<u64> = pks
        .into_iter()
        .filter_map(|pk| match pk {
            Pk::Int(t) if t < before => Some(t),
            _ => None,
        })
        .collect();
    keys.sort_unstable();

    // Take the NEWEST max_rows of the older set (the tail), walking backwards under the byte budget.
    let mut page: Vec<ChatMessage> = Vec::new();
    let mut bytes = 0usize;
    let mut taken = 0usize;
    for &key in keys.iter().rev() {
        if taken >= max_rows || bytes >= max_bytes {
            break;
        }
        let Some(rec) = db
            .get_row_in(&table, Pk::Int(key))
            .map_err(|e| StorageError::Vault(e.to_string()))?
        else {
            taken += 1; // a missing row still consumes cursor progress
            continue;
        };
        let Some(content) = rec.text("content") else {
            taken += 1;
            continue;
        };
        bytes += content.len();
        page.push(ChatMessage {
            content: content.to_string(),
            timestamp: rec.time("timestamp").unwrap_or(key as i64),
            is_outgoing: rec.uint("is_outgoing").unwrap_or(0) != 0,
            delivered: rec.uint("delivered").unwrap_or(0) != 0,
            ack_hash: None, // never leaves this device; not part of a served page
            recovered: rec.uint("recovered").unwrap_or(0) != 0,
        });
        taken += 1;
    }
    page.reverse(); // collected newest→oldest; return ascending

    // More rows remain iff any key is older than the oldest we returned.
    let more = match page.first() {
        Some(oldest) => keys.first().is_some_and(|&k| k < oldest.timestamp as u64),
        None => false,
    };
    Ok((page, more))
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

    /// Messages round-trip thru `save_messages`/`load_messages` on a REAL encrypted vault: write three, close the vault, reopen from disk, read them back in order. Proves the rārangi conversation-row path end to end, not just in RAM.
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
                recovered: false,
            },
            ChatMessage {
                content: "hey".to_string(),
                timestamp: 200,
                is_outgoing: false,
                delivered: false,
                ack_hash: Some([0x7Au8; 32]), // received msg: its ACK hash must survive the round-trip
                recovered: false,
            },
            ChatMessage {
                content: "👋 unicode".to_string(),
                timestamp: 300,
                is_outgoing: true,
                delivered: false,
                ack_hash: None,
                recovered: true, // friend-attested provenance must survive the round-trip
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
        // The received message's ack_hash must survive the round-trip (re-ACK after restart); outgoing messages carry no ack_hash.
        assert_eq!(loaded.messages[1].ack_hash, Some([0x7Au8; 32]));
        assert_eq!(loaded.messages[0].ack_hash, None);
        assert_eq!(loaded.messages[2].content, "👋 unicode");
        assert_eq!(loaded.messages[2].ack_hash, None);
        // Provenance flag round-trip: friend-attested stays flagged, originals stay unflagged (absent field = false, so pre-feature rows load unflagged too).
        assert!(loaded.messages[2].recovered);
        assert!(!loaded.messages[0].recovered && !loaded.messages[1].recovered);

        // Clean up the on-disk vault so reruns start fresh.
        if let Ok([primary, shadow]) = kete::vault_ring_paths(app, &vault_seed, &device_secret) {
            let _ = std::fs::remove_file(primary);
            let _ = std::fs::remove_file(shadow);
        }
    }

    /// Fleet siblings round-trip thru their OWN index on a real vault: `save_contact` routes a sibling to the sibling list (never the contacts index — its handle-string dedup would collapse all siblings into one), state persists under the device-derived pid, `load_all_siblings` rebuilds contact + state across a vault close/reopen, and `delete_sibling` removes both index entry and state.
    #[test]
    fn sibling_round_trip_on_real_vault() {
        use crate::types::{ClutchState, HandleText};

        let device_secret = [31u8; 32];
        let vault_seed = *ihi::handle_to_hash("me-sibling-test").as_bytes();
        let app = crate::storage::APP;

        let sib_device = [0x44u8; 32];
        let mut sib = Contact::new_sibling(
            HandleText::new("me"),
            [0x22; 32],
            DevicePubkey::from_bytes(sib_device),
        );
        sib.clutch_state = ClutchState::Complete;
        sib.friendship_id = Some(FriendshipId::from_bytes([0x55; 32]));
        sib.chain_woven = true;

        // session 1: save, then drop the vault
        {
            let storage = FlatStorage::new(app, vault_seed, device_secret).unwrap();
            save_contact(&sib, &storage).unwrap();
            // Never in the contacts index...
            assert!(load_contact_list(&storage).unwrap().is_empty());
            // ...always in the sibling index.
            assert_eq!(load_sibling_list(&storage).unwrap(), vec![sib_device]);
            // Idempotent re-save doesn't duplicate the index entry.
            save_contact(&sib, &storage).unwrap();
            assert_eq!(load_sibling_list(&storage).unwrap().len(), 1);
        }

        // session 2: reopen from disk, rebuild from the index
        let storage = FlatStorage::new(app, vault_seed, device_secret).unwrap();
        let loaded = load_all_siblings("me", [0x22; 32], &storage);
        assert_eq!(loaded.len(), 1);
        let l = &loaded[0];
        assert!(l.is_sibling);
        assert_eq!(l.public_identity.key, sib_device);
        assert_eq!(
            l.handle_hash,
            crate::crypto::clutch::sibling_party_id(&sib_device),
            "pid re-derives from the device pubkey"
        );
        assert_eq!(l.clutch_state, ClutchState::Complete);
        assert_eq!(l.friendship_id.map(|f| *f.as_bytes()), Some([0x55u8; 32]));
        assert!(l.chain_woven, "the weave seal survives the round-trip");

        // delete: gone from index AND state (a fresh load yields a Pending stub only if re-added).
        delete_sibling(&sib_device, &storage).unwrap();
        assert!(load_sibling_list(&storage).unwrap().is_empty());
        assert!(load_all_siblings("me", [0x22; 32], &storage).is_empty());

        // Clean up the on-disk vault so reruns start fresh.
        if let Ok([primary, shadow]) = kete::vault_ring_paths(app, &vault_seed, &device_secret) {
            let _ = std::fs::remove_file(primary);
            let _ = std::fs::remove_file(shadow);
        }
    }

    /// Blind-state persistence: a friend's deposited blinds (device-keyed 64B blobs) + our confirmed-deposit flag survive a vault close/reopen; contacts saved before the feature load with empty/false defaults (absent-field idiom).
    #[test]
    fn blind_state_round_trip_on_real_vault() {
        use crate::types::HandleText;

        let device_secret = [37u8; 32];
        let vault_seed = *ihi::handle_to_hash("me-blind-test").as_bytes();
        let app = crate::storage::APP;

        let mut c = Contact::new(
            HandleText::new("carol"),
            [0x66; 32],
            DevicePubkey::from_bytes([0x10; 32]),
        );
        c.deposited_blinds = vec![
            ([0x10; 32], vec![0xAB; 64], 1_000),
            ([0x11; 32], vec![0xCD; 64], 2_000),
        ];
        c.blind_deposited = true;

        {
            let storage = FlatStorage::new(app, vault_seed, device_secret).unwrap();
            save_contact_state(&c, &storage).unwrap();
        }

        let storage = FlatStorage::new(app, vault_seed, device_secret).unwrap();
        let identity = ContactIdentity {
            handle_proof: [0x66; 32],
            handle: "carol".to_string(),
        };
        let loaded = load_contact_state(&identity, &storage).unwrap();
        assert_eq!(loaded.deposited_blinds.len(), 2);
        assert_eq!(loaded.deposited_blinds[0], ([0x10; 32], vec![0xAB; 64], 1_000));
        assert_eq!(loaded.deposited_blinds[1], ([0x11; 32], vec![0xCD; 64], 2_000));
        assert!(loaded.blind_deposited);

        if let Ok([primary, shadow]) = kete::vault_ring_paths(app, &vault_seed, &device_secret) {
            let _ = std::fs::remove_file(primary);
            let _ = std::fs::remove_file(shadow);
        }
    }

    /// Fold-respecting trust persistence: the adopted folded member set + the arm flag + the tip ts survive a vault close/reopen, so a restart resumes members-only trust immediately. A contact saved before the feature (all three fields absent) loads as bootstrap (empty set, false, 0).
    #[test]
    fn fold_trust_state_round_trips_and_absent_loads_bootstrap() {
        use crate::types::HandleText;

        let device_secret = [41u8; 32];
        let vault_seed = *ihi::handle_to_hash("me-fold-test").as_bytes();
        let app = crate::storage::APP;

        let mut c = Contact::new(
            HandleText::new("dave"),
            [0x77; 32],
            DevicePubkey::from_bytes([0x20; 32]),
        );
        c.fleet_members = vec![[0x20; 32], [0x21; 32]];
        c.fleet_folded_once = true;
        c.fleet_members_ts = 12_345;

        let identity = ContactIdentity { handle_proof: [0x77; 32], handle: "dave".to_string() };

        {
            let storage = FlatStorage::new(app, vault_seed, device_secret).unwrap();
            save_contact_state(&c, &storage).unwrap();
            let loaded = load_contact_state(&identity, &storage).unwrap();
            assert_eq!(loaded.fleet_members, vec![[0x20; 32], [0x21; 32]]);
            assert!(loaded.fleet_folded_once, "the arm flag persists");
            assert_eq!(loaded.fleet_members_ts, 12_345);
        }

        // A contact with none of the fields set (pre-feature vault) loads as bootstrap.
        {
            let storage = FlatStorage::new(app, vault_seed, device_secret).unwrap();
            let bare = Contact::new(HandleText::new("dave"), [0x77; 32], DevicePubkey::from_bytes([0x20; 32]));
            save_contact_state(&bare, &storage).unwrap();
            let loaded = load_contact_state(&identity, &storage).unwrap();
            assert!(loaded.fleet_members.is_empty(), "absent = empty folded set");
            assert!(!loaded.fleet_folded_once, "absent = bootstrap");
            assert_eq!(loaded.fleet_members_ts, 0);
        }

        if let Ok([primary, shadow]) = kete::vault_ring_paths(app, &vault_seed, &device_secret) {
            let _ = std::fs::remove_file(primary);
            let _ = std::fs::remove_file(shadow);
        }
    }

    /// Newest-first cursor pagination over a real vault: head page = the newest rows, the cursor walk visits everything exactly once, terminates with more=false — and `load_messages` returns time-sorted output even though recovery inserts OLDER rows into the catalog LATER.
    #[test]
    fn history_pagination_walk_and_load_sort() {
        use crate::types::HandleText;

        let device_secret = [31u8; 32];
        let vault_seed = *ihi::handle_to_hash("me-paging-test").as_bytes();
        let app = crate::storage::APP;
        let their_seed = [7u8; 32];

        let storage = FlatStorage::new(app, vault_seed, device_secret).unwrap();

        // Write 120 rows OUT OF CHRONOLOGICAL ORDER (newest batch first — the recovery insertion pattern), timestamps 1..=120.
        let make = |t: i64| ChatMessage {
            content: format!("msg {t}"),
            timestamp: t,
            is_outgoing: t % 2 == 0,
            delivered: t % 2 == 0,
            ack_hash: None,
            recovered: t <= 60, // the "older, recovered" half
        };
        let newer: Vec<ChatMessage> = (61..=120).map(make).collect();
        let older: Vec<ChatMessage> = (1..=60).map(make).collect();
        save_messages_page(&their_seed, &newer, &storage).unwrap();
        save_messages_page(&their_seed, &older, &storage).unwrap(); // older inserted LATER

        // Head page: the newest 50 (71..=120), ascending, more remaining.
        let (page1, more1) =
            load_message_page_before(&their_seed, i64::MAX, 50, usize::MAX, &storage).unwrap();
        assert_eq!(page1.len(), 50);
        assert_eq!(page1.first().unwrap().timestamp, 71);
        assert_eq!(page1.last().unwrap().timestamp, 120);
        assert!(more1);

        // Cursor walk: everything exactly once, terminating.
        let mut seen: Vec<i64> = page1.iter().map(|m| m.timestamp).collect();
        let mut cursor = page1.first().unwrap().timestamp;
        let mut more = more1;
        while more {
            let (page, m) =
                load_message_page_before(&their_seed, cursor, 50, usize::MAX, &storage).unwrap();
            assert!(!page.is_empty(), "more=true must yield rows");
            seen.extend(page.iter().map(|m| m.timestamp));
            cursor = page.first().unwrap().timestamp;
            more = m;
        }
        seen.sort_unstable();
        assert_eq!(seen, (1..=120).collect::<Vec<i64>>());

        // Byte budget cuts a page short (each content is ~6 bytes; 30 bytes ≈ 5-6 rows).
        let (small, small_more) =
            load_message_page_before(&their_seed, i64::MAX, 50, 30, &storage).unwrap();
        assert!(small.len() < 50 && !small.is_empty());
        assert!(small_more);

        // load_messages: full conversation, time-sorted despite out-of-order catalog insertion, with the recovered flag intact on the older half.
        let mut contact = Contact::new(
            HandleText::new("paging-peer"),
            [9u8; 32],
            DevicePubkey::from_bytes([0u8; 32]),
        );
        contact.handle_hash = their_seed;
        load_messages(&mut contact, &storage).unwrap();
        assert_eq!(contact.messages.len(), 120);
        let times: Vec<i64> = contact.messages.iter().map(|m| m.timestamp).collect();
        assert_eq!(times, (1..=120).collect::<Vec<i64>>());
        assert!(contact.messages[0].recovered && !contact.messages[119].recovered);

        // Clean up the on-disk vault so reruns start fresh.
        if let Ok([primary, shadow]) = kete::vault_ring_paths(app, &vault_seed, &device_secret) {
            let _ = std::fs::remove_file(primary);
            let _ = std::fs::remove_file(shadow);
        }
    }
}
