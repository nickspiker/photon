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
use crate::types::{ClutchState, Contact, ContactId, DevicePubkey, FriendshipId, Seed, TrustLevel};
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

/// Static identity data stored in the contact list index — the PIN-SET (docs/identity-profile.md): party id (identity pubkey), avatar-wall key. Never a handle string, which would re-park the seed-deriving input in the vault. (The index once carried a petname column too — the concept was removed 2026-07-31; names live in the per-contact state as published_name.)
#[derive(Clone, Debug)]
pub struct ContactIdentity {
    pub handle_proof: [u8; 32],
    /// The pinned identity pubkey — the party id every per-contact state entry is keyed under.
    pub party_id: [u8; 32],
    /// The pinned avatar-wall material: AES key ‖ lookup hash (zero = unpinned).
    pub avatar_pin: [u8; 64],
}

impl ContactIdentity {
    /// The contact's PARTY ID — kept as a method so state-key call sites read the same as before the pin-set held it directly.
    pub fn party_id(&self) -> [u8; 32] {
        self.party_id
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
                    VsfType::ke(c.party_id.to_vec()),
                    VsfType::ge(c.avatar_pin.to_vec()),
                ],
            )
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }

    let vsf_bytes = builder
        .encode()
        .map_err(|e| StorageError::Parse(e.to_string()))?;

    storage.write_addr(
        &crate::storage::vault_key("contacts", &storage.vault_seed()),
        &vsf_bytes,
    )
}

/// Load the contact list from encrypted index with schema validation
pub fn load_contact_list(storage: &FlatStorage) -> Result<Vec<ContactIdentity>, StorageError> {
    let vsf_bytes =
        match storage.read_addr(&crate::storage::vault_key("contacts", &storage.vault_seed()))? {
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
        // Read by TYPE MARKER, never by index (AGENT.md: "VSF Type Markers Are Self-Describing"). Each marker appears exactly once in this row, so there is no ordering dependency at all: reordering the writer, or inserting a field, cannot silently shift a value into the wrong slot the way positional reads did.
        // Old 2-value handle-bearing rows simply never fill all four and drop (flag-day).
        let mut handle_proof: Option<[u8; 32]> = None;
        let mut party_id: Option<[u8; 32]> = None;
        let mut avatar_pin: Option<[u8; 64]> = None;

        for v in &field.values {
            match v {
                VsfType::hP(b) if b.len() == 32 => handle_proof = b.as_slice().try_into().ok(),
                VsfType::ke(b) if b.len() == 32 => party_id = b.as_slice().try_into().ok(),
                VsfType::ge(b) if b.len() == 64 => avatar_pin = b.as_slice().try_into().ok(),
                // A stray `x` is a pre-removal petname row — the value is dead, the row is fine.
                _ => {}
            }
        }

        if let (Some(handle_proof), Some(party_id), Some(avatar_pin)) =
            (handle_proof, party_id, avatar_pin)
        {
            contacts.push(ContactIdentity {
                handle_proof,
                party_id,
                avatar_pin,
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
        .field("ip", TypeConstraint::Any) // binary socketaddr bytes (6 = v4, 18 = v6) — an address is a NUMBER, never digit text (numbers-binary-at-rest)
        .field("seed", TypeConstraint::AnyHash)
        .field("friendship_id", TypeConstraint::AnyHash) // Links to friendship storage
        .field("last_seen", TypeConstraint::Any) // f64 Eagle Time
        .field("completed_their_hqc_prefix", TypeConstraint::AnyHash) // Detects stale offers (8 bytes)
        .field("chain_woven", TypeConstraint::AnyUnsigned) // bool: chain proven end-to-end once (double-toggle seal) — persists so an established conversation allows composing (+ the staging queue) across restarts, even to an offline peer
        .field("hist_oldest", TypeConstraint::Any) // e6 eagle-time cursor: oldest recovered row so far (i64::MAX = head page pending). Absent = history recovery never ran for this contact.
        .field("hist_complete", TypeConstraint::AnyUnsigned) // bool: friend-history backfill finished (server said no-more, or early-stop). Absent = false.
        .field("published_name", TypeConstraint::AnyString) // The friend's chosen display name, adopted from their pong (always-granted name slot). Absent = never received.
        .field("sibling", TypeConstraint::AnyUnsigned) // bool: this entry is one of OUR OWN fleet devices (fleet weave), keyed by sibling party id. Absent = false (a friend).
        .field("blind", TypeConstraint::Any) // multi-value per deposited blind: (depositor device ke, 64B blob tensor, deposited-at e6). Friend-side storage of OTP-blinded S blobs; absent = none.
        .field("blind_deposited", TypeConstraint::AnyUnsigned) // bool: OUR blind is disk-confirmed at this friend (their blind_ack arrived). Absent = false.
        .field("fleet_member", TypeConstraint::Ed25519Key) // multi-value: one folded member device pubkey. Absent = empty folded set (bootstrap).
        .field("fleet_folded_once", TypeConstraint::AnyUnsigned) // bool: chain folded ≥1 time (arms members-only trust). Absent = false (bootstrap).
        .field("fleet_members_ts", TypeConstraint::Any) // e6: chain-tip eagle time of last adopted fold (monotonic floor). Absent = 0.
        .field("roster_updated", TypeConstraint::Any) // e6: roster LWW clock — last change to the synced identity fields (published_name/avatar_pin). Absent = `added` (pre-feature contacts).
        .field("ceremony_owner", TypeConstraint::Ed25519Key) // The fleet device that owns this friendship's CLUTCH (§4.2 one-ceremony claim, roster-synced). Absent = unclaimed.
        .field("owner_woven", TypeConstraint::AnyUnsigned) // bool: the owner's ceremony completed (display truth for parked siblings). Absent = false.
        .field("pin_genesis", TypeConstraint::AnyHash) // The generation pin: genesis op hash of the friendship's chain (docs/lifecycle.md). Absent = not yet pinned.
        .field("identity_ended", TypeConstraint::AnyUnsigned) // bool: the chain vanished after a fold — owner ended the identity. Absent = false.
        .field("locked_out", TypeConstraint::AnyUnsigned) // bool: sibling device locked out (treat-as-stolen). Absent = false.
        .field("refused_device", TypeConstraint::AnyHash) // multi: friend devices refused via the reported-stolen signal. Absent = none.
        .field("identity_superseded", TypeConstraint::AnyUnsigned) // bool: a different-genesis chain claimed this name — a stranger. Absent = false.
        .field("unread", TypeConstraint::AnyUnsigned) // u32: inbound messages not yet seen (conversation wasn't the active view when they landed). Absent = 0 (legacy contacts load as read).
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
            .set(
                "ip",
                VsfType::v_u3(vsf::types::Vector {
                    data: crate::network::fgtw::protocol::socketaddr_to_bytes(ip),
                }),
            )
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
    // (hist_oldest / hist_complete / unread stay DECLARED in the schema but are no longer written here — they are conversation state, persisted by `save_conversation_state` under the conversation id. `load_legacy_conv_state` still reads them from old records.)
    if !contact.published_name.is_empty() {
        // The friend's pong-adopted display name — written only when received (absent = never).
        builder = builder
            .set("published_name", VsfType::x(contact.published_name.clone()))
            .map_err(|e| StorageError::Parse(e.to_string()))?;
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
    if contact.pinned_genesis != [0u8; 32] {
        builder = builder
            .set("pin_genesis", VsfType::hb(contact.pinned_genesis.to_vec()))
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    if contact.identity_ended {
        builder = builder
            .set("identity_ended", true)
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    if contact.locked_out {
        builder = builder
            .set("locked_out", true)
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    for dev in &contact.refused_devices {
        builder = builder
            .set_multi("refused_device", vec![VsfType::hb(dev.to_vec())])
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    if contact.identity_superseded {
        builder = builder
            .set("identity_superseded", true)
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    if contact.roster_updated != contact.added {
        // Only a real post-creation bump is worth a field — absent reads back as `added`.
        builder = builder
            .set(
                "roster_updated",
                VsfType::e(vsf::types::EtType::e6(contact.roster_updated)),
            )
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    if let Some(owner) = contact.ceremony_owner {
        builder = builder
            .set("ceremony_owner", VsfType::ke(owner.to_vec()))
            .map_err(|e| StorageError::Parse(e.to_string()))?;
    }
    if contact.owner_woven {
        builder = builder
            .set("owner_woven", true)
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
    // Keyed by the party id (the pinned pubkey) — mirrors save_contact_state keying off `contact.handle_hash`.
    let their_identity_seed = identity.party_id();

    let vsf_bytes = match storage.read_addr(&contact_key(&their_identity_seed, "state"))? {
        Some(b) => b,
        None => {
            // No state yet - return contact with just identity info
            let pubkey = DevicePubkey::from_bytes([0u8; 32]); // placeholder
            let contact = Contact::from_pin(
                identity.avatar_pin,
                identity.handle_proof,
                identity.party_id,
                pubkey,
            );
            return Ok(contact);
        }
    };

    #[cfg(feature = "development")]
    crate::network::inspect::vsf_read_decrypted(&vsf_bytes, "contact/state");

    let pubkey = state_pubkey(&vsf_bytes)?;
    let mut contact = Contact::from_pin(
        identity.avatar_pin,
        identity.handle_proof,
        identity.party_id,
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
    // The roster LWW clock floors at `added`; the explicit field below (if present) then raises it.
    contact.roster_updated = added;

    // Optional fields
    if let Some(VsfType::v_u3(v)) = section
        .get_fields("ip")
        .first()
        .and_then(|f| f.values.first())
    {
        // Cleanse at the well: a bogus address adopted before the ingest guards existed persists in the vault, and restoring it re-points every send at 0.0.0.0 on each launch. None here re-enters the normal resolve path instead.
        contact.ip = crate::network::fgtw::protocol::bytes_to_socketaddr(&v.data)
            .filter(|a| !crate::network::traverse::gather::is_bogus_addr(a));
    }
    if let Ok(name) = section.get_value::<String>("published_name") {
        contact.published_name = name;
    }
    if let Ok(seed) = section.get_value::<[u8; 32]>("seed") {
        contact.relationship_seed = Some(Seed::from_bytes(seed));
    }
    if let Ok(fid) = section.get_value::<[u8; 32]>("friendship_id") {
        contact.friendship_id = Some(FriendshipId::from_bytes(fid));
    }
    if let Some(v) = section
        .get_fields("last_seen")
        .first()
        .and_then(|f| f.values.first())
    {
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
    // (hist_oldest / hist_complete / unread are conversation state now — `load_legacy_conv_state` reads them from old records when no conversation-state record exists yet.)
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
    if section
        .get_value::<bool>("blind_deposited")
        .unwrap_or(false)
    {
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
    if section
        .get_value::<bool>("fleet_folded_once")
        .unwrap_or(false)
    {
        contact.fleet_folded_once = true;
    }
    if let Some(v) = section
        .get_fields("fleet_members_ts")
        .first()
        .and_then(|f| f.values.first())
    {
        contact.fleet_members_ts = vsf_to_oscillations(v);
    }
    // Roster LWW clock: absent = never bumped past creation, so `added` (set by the index-row load) stands.
    if let Some(v) = section
        .get_fields("roster_updated")
        .first()
        .and_then(|f| f.values.first())
    {
        contact.roster_updated = vsf_to_oscillations(v);
    }
    // §4.2 ceremony-owner claim + the owner's woven display truth (absent = unclaimed / not woven).
    if let Some(VsfType::ke(k)) = section
        .get_fields("ceremony_owner")
        .first()
        .and_then(|f| f.values.first())
    {
        if k.len() == 32 {
            let mut o = [0u8; 32];
            o.copy_from_slice(k);
            contact.ceremony_owner = Some(o);
        }
    }
    if section.get_value::<bool>("owner_woven").unwrap_or(false) {
        contact.owner_woven = true;
    }
    // Generation pin + end-of-identity flags (docs/lifecycle.md).
    if let Some(VsfType::hb(h)) = section
        .get_fields("pin_genesis")
        .first()
        .and_then(|f| f.values.first())
    {
        if h.len() == 32 {
            contact.pinned_genesis.copy_from_slice(h);
        }
    }
    if section.get_value::<bool>("identity_ended").unwrap_or(false) {
        contact.identity_ended = true;
    }
    if section.get_value::<bool>("locked_out").unwrap_or(false) {
        contact.locked_out = true;
    }
    for f in section.get_fields("refused_device") {
        if let Some(VsfType::hb(h)) = f.values.first() {
            if h.len() == 32 {
                let mut a = [0u8; 32];
                a.copy_from_slice(h);
                if !contact.refused_devices.contains(&a) {
                    contact.refused_devices.push(a);
                }
            }
        }
    }
    if section
        .get_value::<bool>("identity_superseded")
        .unwrap_or(false)
    {
        contact.identity_superseded = true;
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
        &crate::storage::vault_key("siblings", &storage.vault_seed()),
        &vsf_bytes,
    )
}

/// Load the sibling device-pubkey index. Missing entry = empty fleet knowledge (single-device or pre-feature vault).
pub fn load_sibling_list(storage: &FlatStorage) -> Result<Vec<[u8; 32]>, StorageError> {
    let vsf_bytes =
        match storage.read_addr(&crate::storage::vault_key("siblings", &storage.vault_seed()))? {
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
pub fn load_all_siblings(our_handle_proof: [u8; 32], storage: &FlatStorage) -> Vec<Contact> {
    let devices = match load_sibling_list(storage) {
        Ok(d) => d,
        Err(e) => {
            crate::logf!("Failed to load sibling list: {}", e);
            return Vec::new();
        }
    };

    let mut siblings = Vec::new();
    for device in devices {
        let mut c = Contact::new_sibling(our_handle_proof, DevicePubkey::from_bytes(device));
        match storage.read_addr(&contact_key(&c.handle_hash, "state")) {
            Ok(Some(vsf_bytes)) => {
                if let Err(e) = apply_contact_state(&mut c, &vsf_bytes) {
                    crate::logf!(
                        "Failed to parse sibling state for device {}: {}",
                        hex::encode(&device[..4]),
                        e
                    );
                }
            }
            Ok(None) => {} // Fresh Pending sibling — ceremony re-runs
            Err(e) => {
                crate::logf!(
                    "Failed to read sibling state for device {}: {}",
                    hex::encode(&device[..4]),
                    e
                );
            }
        }
        // The sibling flag AND id are authoritative from new_sibling, not the blob: a pre-fix blob carries the colliding blake3(pubkey) id (the notes-row keygen misroute, 2026-08-13), so the derived pid-keyed id is re-asserted after the state apply.
        c.is_sibling = true;
        c.id = ContactId::from_bytes(c.handle_hash);
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

    // Update contact list: UPSERT by handle_proof — the index carries the mutable pin-set fields (avatar key), so a fresh pin rewrites its row.
    let mut list = load_contact_list(storage).unwrap_or_default();

    let fresh = ContactIdentity {
        handle_proof: contact.handle_proof,
        party_id: contact.handle_hash,
        avatar_pin: contact.avatar_pin,
    };
    match list
        .iter_mut()
        .find(|c| c.handle_proof == contact.handle_proof)
    {
        Some(row) => {
            if row.party_id != fresh.party_id || row.avatar_pin != fresh.avatar_pin {
                *row = fresh;
                save_contact_list(&list, storage)?;
            }
        }
        None => {
            list.push(fresh);
            save_contact_list(&list, storage)?;
        }
    }

    Ok(())
}

/// Load all contacts from disk
pub fn load_all_contacts(storage: &FlatStorage) -> Vec<Contact> {
    let identities = match load_contact_list(storage) {
        Ok(list) => list,
        Err(e) => {
            crate::logf!("Failed to load contact list: {}", e);
            return Vec::new();
        }
    };

    let mut contacts = Vec::new();
    for identity in identities {
        match load_contact_state(&identity, storage) {
            Ok(contact) => contacts.push(contact),
            Err(e) => {
                crate::logf!(
                    "Failed to load contact state for {}: {}",
                    crate::fp(&identity.handle_proof),
                    e
                );
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

/// The rārangi table for a conversation, derived from its PARTICIPANT SET — the one derivation the wire, the UI and local storage all share, so the three can never disagree about which conversation this is.
///
/// It did not use to be one derivation. The local table mixed our RAW IDENTITY SEED with the peer's PARTY ID (their identity pubkey), while every wire value used party ids on both sides — two different representations of "us" in one key. Notes-to-self was the case where that showed: its table was `[identity_seed, identity_pubkey]`, an asymmetric pair naming one person twice, which no participant-set derivation could ever reproduce.
///
/// Sorts and deduplicates via [`Conversation`], so one participant, two, or any number all resolve the same way and a self set stays a set.
fn conversation_table(participants: &[[u8; 32]]) -> [u8; 32] {
    *crate::types::Conversation::new(participants.iter().copied())
        .id()
        .as_bytes()
}

/// Our own party id — the identity pubkey every participant set is expressed in.
fn our_party_id(storage: &FlatStorage) -> [u8; 32] {
    crate::crypto::clutch::identity_party_id(&storage.vault_seed())
}

/// Save a conversation's messages as rows in its table — keyed by the conversation's own participant-set id, the same value the wire and the UI derive. Idempotent: each message is written at its sequence index, so re-saving the same history overwrites row-for-row identically.
/// The message row key: 8 BE bytes of eagle_time ‖ the first 8 of blake3(content). Byte order IS the canonical (time, content-hash) row order, and same-tick rows from DIFFERENT senders get distinct keys — the bare-Int eagle_time key made them ONE row, so the second sender's message silently overwrote the first at persistence (RAM held both, every reboot held one). Within one sender's stream eagle_time is unique (704ps ticks), so collisions are strictly the cross-sender case.
fn message_row_key(timestamp: i64, content: &str) -> [u8; 16] {
    let mut key = [0u8; 16];
    key[..8].copy_from_slice(&(timestamp as u64).to_be_bytes());
    key[8..].copy_from_slice(&blake3::hash(content.as_bytes()).as_bytes()[..8]);
    key
}

pub fn save_messages(
    conv: &crate::types::Conversation,
    storage: &FlatStorage,
) -> Result<(), StorageError> {
    if conv.messages.is_empty() {
        return Ok(()); // Nothing to save
    }

    let table = *conv.id().as_bytes();

    let mut db = Db::open(storage).map_err(|e| StorageError::Vault(e.to_string()))?;
    for msg in conv.messages.iter() {
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
        // notified: the fleet's alert duty discharged — stored INVERTED (written only when false, absent = true) on purpose: pre-feature rows must read as notified (history never re-dings), and steady-state rows are notified so the field is usually absent.
        if !msg.notified {
            rec = rec.set("unnotified", 1u64);
        }
        // recovered: friend-attested provenance flag — written only when true (absent = false), matching the contact-state optional-field idiom.
        if msg.recovered {
            rec = rec.set("recovered", 1u64);
        }
        // deleted: tombstone flag — written only when true (absent = false), same optional-field idiom.
        if msg.deleted {
            rec = rec.set("deleted", 1u64);
        }
        // reference: typed reply/edit/react target — two fields, written only when present (absent = plain row).
        if let Some((kind, target)) = msg.reference {
            rec = rec
                .set("ref_kind", kind as u64)
                .set("ref_ts", Value::Time(target));
        }
        db.put_row_in(
            &table,
            Pk::bytes(&message_row_key(msg.timestamp, &msg.content)),
            &rec,
        )
        .map_err(|e| StorageError::Vault(e.to_string()))?;
    }
    // Self-terminating key migration: every row was just re-put under its composite key, so any legacy bare-Int keys left in the table are stale twins — delete them or the table doubles (load would dedup by row identity, but the vault must not carry the ghosts).
    if let Ok(pks) = db.list_in(&table) {
        for pk in pks {
            if matches!(pk, Pk::Int(_)) {
                let _ = db.delete_row_in(&table, pk);
            }
        }
    }

    #[cfg(feature = "development")]
    crate::logf!(
        "STORAGE: Saved {} messages for conversation {}",
        conv.messages.len(),
        hex::encode(&conv.id().as_bytes()[..4])
    );

    Ok(())
}

/// Load a conversation's messages from its table, in counter order (which is chronological).
pub fn load_messages(
    conv: &mut crate::types::Conversation,
    storage: &FlatStorage,
) -> Result<(), StorageError> {
    let table = *conv.id().as_bytes();

    let db = Db::open(storage).map_err(|e| StorageError::Vault(e.to_string()))?;
    let pks = db
        .list_in(&table)
        .map_err(|e| StorageError::Vault(e.to_string()))?;

    // Sort keys canonically — the catalog yields INSERTION order, which matched chronological order only while rows were appended live. Both key forms load: the 16-byte composite (BE eagle_time ‖ blake3(content)[..8] — byte order == the canonical (time, hash) order, same-tick rows coexist) and the legacy bare-Int eagle_time key (pre-composite vaults; save_messages re-puts + sweeps them, so that arm self-terminates).
    let mut keys: Vec<(u64, [u8; 8], Pk)> = pks
        .into_iter()
        .filter_map(|pk| match &pk {
            Pk::Int(t) => Some((*t, [0u8; 8], pk)),
            Pk::Bytes(b) if b.len() == 16 => {
                let ts = u64::from_be_bytes(b[..8].try_into().unwrap());
                let tie: [u8; 8] = b[8..16].try_into().unwrap();
                Some((ts, tie, pk))
            }
            _ => None,
        })
        .collect();
    keys.sort_unstable_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));

    conv.messages.clear();
    for (_, _, pk) in keys {
        let Some(rec) = db
            .get_row_in(&table, pk)
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
        conv.messages.push(ChatMessage {
            content: content.to_string(),
            timestamp: rec.time("timestamp").unwrap_or(0),
            is_outgoing: rec.uint("is_outgoing").unwrap_or(0) != 0,
            delivered: rec.uint("delivered").unwrap_or(0) != 0,
            ack_hash,
            recovered: rec.uint("recovered").unwrap_or(0) != 0,
            deleted: rec.uint("deleted").unwrap_or(0) != 0,
            reference: record_reference(&rec),
            notified: rec.uint("unnotified").unwrap_or(0) == 0,
        });
    }

    #[cfg(feature = "development")]
    crate::logf!(
        "STORAGE: Loaded {} messages for conversation {}",
        conv.messages.len(),
        hex::encode(&conv.id().as_bytes()[..4])
    );

    Ok(())
}

/// The (vault address, payload) pair `save_conversation_state` writes — split out so the off-thread writer can carry the 13-byte record instead of cloning a whole conversation. Fixed-width layout: unread u32 LE ‖ hist_oldest i64 LE ‖ flags u8 (bit 0 = hist_complete, bit 1 = history has run).
pub fn conversation_state_record(conv: &crate::types::Conversation) -> ([u8; 32], [u8; 13]) {
    let mut buf = [0u8; 13];
    buf[..4].copy_from_slice(&conv.unread_count.to_le_bytes());
    if let Some(rec) = &conv.history_recovery {
        buf[4..12].copy_from_slice(&rec.oldest_recovered_osc.to_le_bytes());
        buf[12] = u8::from(rec.complete) | 0b10;
    }
    (
        crate::storage::vault_key("conv_state", conv.id().as_bytes()),
        buf,
    )
}

/// Decode a row record's typed reference (reply/edit/react target) — absent or unknown-kind reads as None, the pre-feature default.
fn record_reference(rec: &Record) -> Option<(crate::types::RefKind, i64)> {
    let kind = crate::types::RefKind::from_wire(rec.uint("ref_kind")? as u8)?;
    Some((kind, rec.time("ref_ts")?))
}

/// Persist the conversation-scoped durable bits — unread count and the history-recovery cursor — under the conversation id. These historically rode the contact record; a conversation is not a contact, so they get their own tiny record.
pub fn save_conversation_state(
    conv: &crate::types::Conversation,
    storage: &FlatStorage,
) -> Result<(), StorageError> {
    let (addr, buf) = conversation_state_record(conv);
    storage.write_addr(&addr, &buf)
}

/// Hydrate a conversation's durable bits. Prefers the conversation-state record; when none exists yet, falls back to the fields old builds wrote into the CONTACT record at `legacy_contact_key` (the participant the row was filed under). The fallback is self-terminating — the first `save_conversation_state` supersedes it — and deletable once no vault in the field predates the split.
pub fn load_conversation_state(
    conv: &mut crate::types::Conversation,
    legacy_contact_key: &[u8; 32],
    storage: &FlatStorage,
) {
    let addr = crate::storage::vault_key("conv_state", conv.id().as_bytes());
    if let Ok(Some(buf)) = storage.read_addr(&addr) {
        if buf.len() == 13 {
            conv.unread_count = u32::from_le_bytes(buf[..4].try_into().unwrap());
            if buf[12] & 0b10 != 0 {
                let complete = buf[12] & 1 != 0;
                conv.history_recovery = Some(crate::types::HistoryRecovery {
                    oldest_recovered_osc: i64::from_le_bytes(buf[4..12].try_into().unwrap()),
                    complete,
                    in_flight: None,
                    next_request_osc: 0,
                    urgent: false,
                    was_complete_before: complete,
                });
            }
            return;
        }
    }
    let (unread, rec) = load_legacy_conv_state(legacy_contact_key, storage);
    conv.unread_count = unread;
    conv.history_recovery = rec;
}

/// Read the unread count + history cursor out of an OLD contact record — the home they had before conversation state split out. Reconstruction matches the old loader exactly: next_request_osc = 0 so an incomplete backfill is immediately eligible, urgent false because resume is background work.
fn load_legacy_conv_state(
    key: &[u8; 32],
    storage: &FlatStorage,
) -> (u32, Option<crate::types::HistoryRecovery>) {
    let Ok(Some(bytes)) = storage.read_addr(&contact_key(key, "state")) else {
        return (0, None);
    };
    let Ok(section) = SectionBuilder::parse(contact_state_schema(), &bytes) else {
        return (0, None);
    };
    let unread = section.get_value::<u32>("unread").unwrap_or(0);
    let rec = section
        .get_fields("hist_oldest")
        .first()
        .and_then(|f| f.values.first())
        .map(|v| {
            let complete = section.get_value::<bool>("hist_complete").unwrap_or(false);
            crate::types::HistoryRecovery {
                oldest_recovered_osc: vsf_to_oscillations(v),
                complete,
                in_flight: None,
                next_request_osc: 0,
                urgent: false,
                was_complete_before: complete,
            }
        });
    (unread, rec)
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
    let table = conversation_table(&[our_party_id(storage), *their_identity_seed]);
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
        // deleted: tombstone flag — written only when true (absent = false), same optional-field idiom.
        if msg.deleted {
            rec = rec.set("deleted", 1u64);
        }
        // reference: typed reply/edit/react target — two fields, written only when present (absent = plain row).
        if let Some((kind, target)) = msg.reference {
            rec = rec
                .set("ref_kind", kind as u64)
                .set("ref_ts", Value::Time(target));
        }
        db.put_row_in(
            &table,
            Pk::bytes(&message_row_key(msg.timestamp, &msg.content)),
            &rec,
        )
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
    let table = conversation_table(&[our_party_id(storage), *their_identity_seed]);
    let db = Db::open(storage).map_err(|e| StorageError::Vault(e.to_string()))?;
    let pks = db
        .list_in(&table)
        .map_err(|e| StorageError::Vault(e.to_string()))?;

    // All keys strictly older than the cursor, ascending.
    let before = if before_osc <= 0 {
        0u64
    } else {
        before_osc as u64
    };
    // Both key forms page (composite + legacy Int — see load_messages), ordered canonically by (time, hash).
    let mut keys: Vec<(u64, [u8; 8], Pk)> = pks
        .into_iter()
        .filter_map(|pk| match &pk {
            Pk::Int(t) if *t < before => Some((*t, [0u8; 8], pk)),
            Pk::Bytes(b) if b.len() == 16 => {
                let ts = u64::from_be_bytes(b[..8].try_into().unwrap());
                let tie: [u8; 8] = b[8..16].try_into().unwrap();
                (ts < before).then_some((ts, tie, pk))
            }
            _ => None,
        })
        .collect();
    keys.sort_unstable_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    // The oldest candidate's timestamp, read before the loop consumes the keys — the `more` flag below compares against it.
    let oldest_key_ts = keys.first().map(|k| k.0);

    // Take the NEWEST max_rows of the older set (the tail), walking backwards under the byte budget.
    let mut page: Vec<ChatMessage> = Vec::new();
    let mut bytes = 0usize;
    let mut taken = 0usize;
    for (ts, _, pk) in keys.into_iter().rev() {
        if taken >= max_rows || bytes >= max_bytes {
            break;
        }
        let Some(rec) = db
            .get_row_in(&table, pk)
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
            timestamp: rec.time("timestamp").unwrap_or(ts as i64),
            is_outgoing: rec.uint("is_outgoing").unwrap_or(0) != 0,
            delivered: rec.uint("delivered").unwrap_or(0) != 0,
            ack_hash: None, // never leaves this device; not part of a served page
            recovered: rec.uint("recovered").unwrap_or(0) != 0,
            deleted: rec.uint("deleted").unwrap_or(0) != 0,
            reference: record_reference(&rec),
            notified: rec.uint("unnotified").unwrap_or(0) == 0,
        });
        taken += 1;
    }
    page.reverse(); // collected newest→oldest; return ascending

    // More rows remain iff any key is older than the oldest we returned.
    let more = match page.first() {
        Some(oldest) => oldest_key_ts.is_some_and(|k| k < oldest.timestamp as u64),
        None => false,
    };
    Ok((page, more))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vsf::VsfSection;

    /// THE self-conversation storage seam (the notes black-hole class): rows saved under the DEDUPED self participant set must serve back thru the page reader's [pid, pid] degenerate addressing — save table == serve table, or a sibling asking for our notes gets an empty page while both UIs claim "synchronized".
    #[test]
    fn self_conversation_pages_serve_like_any_other() {
        crate::storage::isolate_test_storage();
        let vault_seed = [0x8Au8; 32];
        let device_secret = [0x8Bu8; 32];
        let storage = FlatStorage::new(crate::storage::APP, vault_seed, device_secret).unwrap();
        let our_pid = crate::crypto::clutch::identity_party_id(&vault_seed);
        // The self conversation exactly as Contact::conversation builds it: [us, us] → dedup → {us}.
        let mut conv = crate::types::Conversation::new([our_pid, our_pid]);
        for i in 0..13i64 {
            conv.messages.push(crate::types::ChatMessage::new_with_timestamp(
                format!("note {i}"),
                true,
                1_000_000 + i,
            ));
        }
        save_messages(&conv, &storage).unwrap();
        // Serve side: the fleet-route page read addresses the table as [our_pid, their_seed] with their_seed = our own pid for the self row.
        let (rows, more) =
            load_message_page_before(&our_pid, i64::MAX, 50, usize::MAX, &storage).unwrap();
        assert_eq!(rows.len(), 13, "self rows must serve — the 13-vs-0 black-hole seam");
        assert!(!more);
        assert_eq!(rows.last().unwrap().content, "note 12");
        // And the walk's page cursor form: strictly-before paging returns the older remainder.
        let (older, _) =
            load_message_page_before(&our_pid, 1_000_005, 50, usize::MAX, &storage).unwrap();
        assert_eq!(older.len(), 5);
    }

    #[test]
    fn test_contact_identity_roundtrip() {
        let identity = ContactIdentity {
            handle_proof: [1u8; 32],
            party_id: [2u8; 32],
            avatar_pin: [3u8; 64],
        };

        // Build section — mirror of save_contact_list's pin-set row
        let mut section = VsfSection::new("contact_list");
        section.add_field_multi(
            "contact",
            vec![
                VsfType::hP(identity.handle_proof.to_vec()),
                VsfType::ke(identity.party_id.to_vec()),
                VsfType::ge(identity.avatar_pin.to_vec()),
            ],
        );

        let encoded = section.encode();

        // Parse back
        let mut ptr = 0;
        let parsed = VsfSection::parse(&encoded, &mut ptr).unwrap();

        let fields = parsed.get_fields("contact");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].values.len(), 3);

        let proof: [u8; 32] = match &fields[0].values[0] {
            VsfType::hP(v) if v.len() == 32 => v.as_slice().try_into().unwrap(),
            _ => panic!("Expected hP"),
        };
        assert_eq!(proof, identity.handle_proof);

        // Party id round-trips (no derivation — it IS the pin)
        assert_eq!(identity.party_id(), [2u8; 32]);
    }

    /// Messages round-trip thru `save_messages`/`load_messages` on a REAL encrypted vault: write three, close the vault, reopen from disk, read them back in order. Proves the rārangi conversation-row path end to end, not just in RAM.
    #[test]
    fn messages_round_trip_on_real_vault() {
        use crate::types::HandleText;

        let device_secret = [29u8; 32];
        let vault_seed = *ihi::handle_to_hash("me-messages-test").as_bytes();
        crate::storage::isolate_test_storage();
        let app = crate::storage::APP;

        // A two-participant conversation: our pid derived from the vault seed the same way the app does, the peer's an arbitrary pid.
        let our_pid = crate::crypto::clutch::identity_party_id(&vault_seed);
        let mut conv = crate::types::Conversation::new([our_pid, [3u8; 32]]);
        conv.messages = vec![
            ChatMessage {
                content: "hi".to_string(),
                timestamp: 100,
                is_outgoing: true,
                delivered: true,
                ack_hash: None,
                recovered: false,
                deleted: false,
                reference: None,
                notified: true,
            },
            ChatMessage {
                content: "hey".to_string(),
                timestamp: 200,
                is_outgoing: false,
                delivered: false,
                ack_hash: Some([0x7Au8; 32]), // received msg: its ACK hash must survive the round-trip
                recovered: false,
                deleted: false,
                reference: None,
                notified: true,
            },
            ChatMessage {
                content: "👋 unicode".to_string(),
                timestamp: 300,
                is_outgoing: true,
                delivered: false,
                ack_hash: None,
                recovered: true, // friend-attested provenance must survive the round-trip,
                deleted: false,
                reference: None,
                notified: true,
            },
        ];

        // session 1: save, then drop the vault (closes the on-disk files)
        {
            let storage = FlatStorage::new(app, vault_seed, device_secret).unwrap();
            save_messages(&conv, &storage).unwrap();
        }

        // session 2: reopen from disk, load into a fresh conversation over the same participant set
        let storage = FlatStorage::new(app, vault_seed, device_secret).unwrap();
        let mut loaded = crate::types::Conversation::new([our_pid, [3u8; 32]]);
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
        crate::storage::isolate_test_storage();
        let app = crate::storage::APP;

        let sib_device = [0x44u8; 32];
        let mut sib = Contact::new_sibling([0x22; 32], DevicePubkey::from_bytes(sib_device));
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
        let loaded = load_all_siblings([0x22; 32], &storage);
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
        assert!(load_all_siblings([0x22; 32], &storage).is_empty());

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
        crate::storage::isolate_test_storage();
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
            party_id: crate::crypto::clutch::identity_party_id(
                &crate::types::Handle::to_identity_seed("carol"),
            ),
            avatar_pin: [0u8; 64],
        };
        let loaded = load_contact_state(&identity, &storage).unwrap();
        assert_eq!(loaded.deposited_blinds.len(), 2);
        assert_eq!(
            loaded.deposited_blinds[0],
            ([0x10; 32], vec![0xAB; 64], 1_000)
        );
        assert_eq!(
            loaded.deposited_blinds[1],
            ([0x11; 32], vec![0xCD; 64], 2_000)
        );
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
        crate::storage::isolate_test_storage();
        let app = crate::storage::APP;

        let mut c = Contact::new(
            HandleText::new("dave"),
            [0x77; 32],
            DevicePubkey::from_bytes([0x20; 32]),
        );
        c.fleet_members = vec![[0x20; 32], [0x21; 32]];
        c.fleet_folded_once = true;
        c.fleet_members_ts = 12_345;

        let identity = ContactIdentity {
            handle_proof: [0x77; 32],
            party_id: crate::crypto::clutch::identity_party_id(
                &crate::types::Handle::to_identity_seed("dave"),
            ),
            avatar_pin: [0u8; 64],
        };

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
            let bare = Contact::new(
                HandleText::new("dave"),
                [0x77; 32],
                DevicePubkey::from_bytes([0x20; 32]),
            );
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
        crate::storage::isolate_test_storage();
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
            deleted: false,
            reference: None,
            notified: true,
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

        // load_messages: full conversation, time-sorted despite out-of-order catalog insertion, with the recovered flag intact on the older half. Same participant set the page writers derived, so the two paths hit one table.
        let our_pid = crate::crypto::clutch::identity_party_id(&vault_seed);
        let mut conv = crate::types::Conversation::new([our_pid, their_seed]);
        load_messages(&mut conv, &storage).unwrap();
        assert_eq!(conv.messages.len(), 120);
        let times: Vec<i64> = conv.messages.iter().map(|m| m.timestamp).collect();
        assert_eq!(times, (1..=120).collect::<Vec<i64>>());
        assert!(conv.messages[0].recovered && !conv.messages[119].recovered);

        // Clean up the on-disk vault so reruns start fresh.
        if let Ok([primary, shadow]) = kete::vault_ring_paths(app, &vault_seed, &device_secret) {
            let _ = std::fs::remove_file(primary);
            let _ = std::fs::remove_file(shadow);
        }
    }
}
