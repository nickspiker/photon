use super::{CeremonyId, DevicePubkey, FriendshipId, Seed};
use crate::crypto::clutch::{
    ClutchAllKeypairs, ClutchKemResponsePayload, ClutchKemSharedSecrets, ClutchOfferPayload,
};
use std::net::{Ipv4Addr, SocketAddr};

/// A slot in the CLUTCH ceremony, indexed by sorted handle_hash position. Same indexing on all devices in the ceremony (N-party ready).
#[derive(Clone, Debug)]
pub struct PartySlot {
    /// Handle hash identifying this party (sorted position determines slot index)
    pub handle_hash: [u8; 32],
    /// 8 public keys from ClutchOffer for this slot's party
    pub offer: Option<ClutchOfferPayload>,
    /// Secrets from encapsulation FROM this slot's party (they encapsulated to local, local decapsulated)
    pub kem_secrets_from_them: Option<ClutchKemSharedSecrets>,
    /// Secrets from encapsulation TO this slot's party (local encapsulated to them)
    pub kem_secrets_to_them: Option<ClutchKemSharedSecrets>,
    /// KEM response payload for re-send if peer missed it
    pub kem_response_for_resend: Option<ClutchKemResponsePayload>,
}

impl PartySlot {
    /// Create empty slot for a party
    pub fn new(handle_hash: [u8; 32]) -> Self {
        Self {
            handle_hash,
            offer: None,
            kem_secrets_from_them: None,
            kem_secrets_to_them: None,
            kem_response_for_resend: None,
        }
    }

    /// Check if this slot has all required data for ceremony completion. Each slot needs offer + ONE KEM contribution (either direction):
    /// - Local slot: kem_secrets_to_them (local encapsulated)
    /// - Remote slots: kem_secrets_from_them (remote encapsulated)
    pub fn is_complete(&self) -> bool {
        self.offer.is_some()
            && (self.kem_secrets_from_them.is_some() || self.kem_secrets_to_them.is_some())
    }
}

/// A chat message in a conversation (UI-level representation)
#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub content: String,
    pub timestamp: i64, // Eagle time oscillations (i64, ~1.42 GHz since Apollo 11 landing)
    pub is_outgoing: bool, // true = we sent it, false = they sent it
    pub delivered: bool, // true = confirmed delivered to recipient
    /// For RECEIVED messages: the ACK plaintext_hash (blake3 of the full decrypted payload) we sent back when we first processed this message. Stored so a duplicate retransmit (our ACK was lost) can be re-ACKed with the SAME hash instead of being silently dropped — the sender's chain only advances on a matching ACK, so a lost ACK would otherwise stall it forever. `None` for outgoing messages and for received messages stored before this field existed.
    pub ack_hash: Option<[u8; 32]>,
    /// `true` when this row was RECOVERED from a friend's copy of the conversation (history recovery after a client reset) rather than witnessed by this device as a signed wire frame. Friend-attested provenance: the friend could in principle have altered it. Persisted so phase-2 fleet recovery (self-attested rows) can supersede friend-attested ones, and so a UI cue can exist later. No UI treatment yet.
    pub recovered: bool,
}

impl ChatMessage {
    pub fn new(content: String, is_outgoing: bool) -> Self {
        Self {
            content,
            timestamp: vsf::eagle_time_oscillations(),
            is_outgoing,
            delivered: false,
            ack_hash: None,
            recovered: false,
        }
    }

    /// Create a message with a specific timestamp (for received messages with known eagle_time)
    pub fn new_with_timestamp(content: String, is_outgoing: bool, timestamp: i64) -> Self {
        Self {
            content,
            timestamp,
            is_outgoing,
            delivered: false,
            ack_hash: None,
            recovered: false,
        }
    }

    /// Builder: attach the ACK hash (the plaintext_hash we ACK this message with). Used on the receive path so a later duplicate can be re-ACKed from storage.
    pub fn with_ack_hash(mut self, ack_hash: [u8; 32]) -> Self {
        self.ack_hash = Some(ack_hash);
        self
    }
}

/// Runtime state machine for friend-assisted history recovery on one conversation. Lives on the Contact (never persisted whole — the durable bits are the `hist_oldest` cursor + `hist_complete` flag in contact state). Newest-first cursor pagination: `oldest_recovered_osc` walks DOWN from `i64::MAX` (head page) as pages land.
#[derive(Clone, Debug)]
pub struct HistoryRecovery {
    /// Cursor: oldest eagle_time we've recovered so far. `i64::MAX` = head page not yet fetched. The next request asks for rows strictly BEFORE this.
    pub oldest_recovered_osc: i64,
    /// All pages fetched (server said no-more, or the early-stop rule fired).
    pub complete: bool,
    /// Outstanding request: (request id, sent eagle_time, before cursor it asked for). Expired + re-issued after a timeout; pages with a non-matching rid are dropped.
    pub in_flight: Option<([u8; 32], i64, i64)>,
    /// Earliest eagle_time the next trickle request may fire (rate limiting).
    pub next_request_osc: i64,
    /// Scrollback jump-queue: user is looking at the old edge — fire the next request immediately, ignoring the trickle interval.
    pub urgent: bool,
    /// The persisted `hist_complete` value at kickoff. Early-stop rule: if history was complete before this (re-)kickoff and a page contributes zero new rows, the history is still complete — a routine re-key on an intact pair stops after one page instead of re-walking 10 years.
    pub was_complete_before: bool,
}

/// A handle name stored as VSF text (normalized Unicode, unambiguous) Wrapper around String that represents a VSF x-type text value
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandleText(String);

impl HandleText {
    pub fn new(s: &str) -> Self {
        HandleText(s.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for HandleText {
    fn from(s: &str) -> Self {
        HandleText::new(s)
    }
}

impl std::fmt::Display for HandleText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Reserved sentinel content for the hidden chain-weave probe message. After CLUTCH reaches Complete, each device sends exactly one message with this exact content to validate the ratchet end-to-end. The receive path recognises it, advances/ACKs the chain like any message, but suppresses the chat bubble. The control bytes (SOH/STX around the tag) make a collision with a real user message effectively impossible.
pub const CHAIN_PROBE_MARKER: &str = "\u{1}\u{2}photon-chain-probe\u{2}\u{1}";

/// State of the CLUTCH key ceremony for a contact
///
/// Slot-based design: each party has a slot indexed by sorted handle_hash position. Ceremony completes when all slots have both offer and kem_secrets filled, AND both parties have exchanged matching eggs_proof values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ClutchState {
    #[default]
    Pending, // Slots not all filled yet
    AwaitingProof, // Eggs computed, sent our proof, waiting for peer's proof
    Complete,      // Proofs exchanged and verified
}

/// One fleet device's known addresses + liveness, learned from ITS OWN traffic (pong source, FGTW peer row). Runtime only — never persisted; presence rediscovers every session.
#[derive(Clone, Debug)]
pub struct DeviceEndpoint {
    pub pubkey: [u8; 32],
    /// Public (WAN) address, from a pong arriving off-LAN or an FGTW peer row.
    pub public: Option<SocketAddr>,
    /// LAN address, from a pong arriving from a private source.
    pub lan: Option<SocketAddr>,
    /// This device answered its own ping within the timeout window.
    pub online: bool,
}

#[derive(Clone, Debug)]
pub struct Contact {
    pub id: ContactId,
    /// Local petname — the only name at rest for this contact; user-chosen, synced across OUR fleet via the roster, EMPTY by default (empty renders the keyed voca pseudonym). Never defaulted to the typed handle: the handle string derives the identity seed, so storing it anywhere re-creates the honeypot (docs/identity-profile.md).
    pub petname: String,
    /// The friend's published profile name, adopted from their pong's always-granted name slot. Display fallback after petname; carries zero trust.
    pub published_name: String,
    /// Runtime: `published_name` changed since the last state save — the status drain sets it, the post-drain sweep persists + clears it (persisting inside the drain would fight the contacts borrow).
    pub published_name_dirty: bool,
    /// Runtime: `avatar_pin` adopted from a pong since the last save — the post-drain sweep persists the contact list + fetches the avatar, then clears it.
    pub avatar_pin_dirty: bool,
    /// The avatar-wall pin, derived at first-met before the seed dropped: AES key (32) ‖ FGTW lookup hash (32). All zeroes = not pinned (siblings and self use the session-seed path).
    pub avatar_pin: [u8; 64],
    pub handle_proof: [u8; 32], // Cached handle_proof (expensive to compute - ~1 second) - PUBLIC
    /// The PARTY ID: the friend's pinned identity PUBKEY (device-derived pid for siblings). Every ceremony/braid derivation keys on this opaque 32-byte id; it holds no signing power. (Pre-pin-set this was BLAKE3(handle) — the friend's identity SEED.)
    pub handle_hash: [u8; 32],
    pub public_identity: DevicePubkey,
    /// The friend's CURRENT fleet device set, folded from their public membership chain (`fleet::current_members` by `handle_proof`). A contact is an IDENTITY, not a device: pings/pongs/offers/messages are honoured from ANY member here, not just `public_identity` (which is only the device we happened to meet first). Empty until the first refresh; refreshed on load and on a `fleet` bump for this contact. Now persisted alongside `fleet_folded_once` / `fleet_members_ts` so a restart resumes fold-respecting trust immediately instead of a trust-nobody gap while the re-fold is in flight.
    pub fleet_members: Vec<[u8; 32]>,
    /// True once this contact's fleet chain has SUCCESSFULLY folded at least once. Arms the fold-respecting trust rule in `knows_device`: once armed, only current folded members are trusted, and `public_identity` loses its unconditional pass if the fold excluded it (that device was removed). A fold FAILURE never arms it — only a real adopted fold does — so a network outage never flips a healthy contact to trust-nobody. Siblings never fold (their proof routes to `reconcile_fleet_siblings`), so this stays false for them and they keep the bootstrap path.
    pub fleet_folded_once: bool,
    /// The chain-tip eagle time of the last adopted fold — a monotonic guard so we never adopt an OLDER fold over a newer one (guards against an R2 eventual-consistency read serving a stale pre-removal member set over a fresh post-removal one). 0 means no fold adopted yet.
    pub fleet_members_ts: i64,
    /// The GENERATION pin (docs/lifecycle.md): the genesis op hash of the chain this friendship belongs to, pinned at the first adopted fold. A later chain with a DIFFERENT genesis is a successor holding a re-claimed name — the same party id derives from the same handle string, so this pin is the ONLY thing standing between a freed name and inherited trust. Zero = not yet pinned (pre-feature contacts pin on their next fold).
    pub pinned_genesis: [u8; 32],
    /// The contact's chain vanished after we had folded it — their owner ended the identity (last departure, worker purge). Local state FREEZES (verify-or-withhold); rendered as "identity ended". Cleared if the same-genesis chain reappears (a worker blip, not a death).
    pub identity_ended: bool,
    /// A chain with a DIFFERENT genesis appeared under this contact's name — a stranger re-claimed the freed handle. Folds are refused; rendered as NOT-them. Never auto-clears (the pin is permanent testimony).
    pub identity_superseded: bool,
    pub ip: Option<SocketAddr>, // The ACTIVE device's public IP:port (see `active_device`) — the primary TX target
    pub local_ip: Option<Ipv4Addr>, // The ACTIVE device's LAN IP (hairpin NAT workaround)
    pub local_port: Option<u16>, // The ACTIVE device's LAN port
    /// Per-device address table (runtime only, rediscovered every session): one entry per fleet device we've heard from, keyed by device pubkey. Pongs/peer-rows update the SENDER's entry here — never the contact-level `ip` slot directly — so a friend's three devices each keep their own address and presence instead of thrashing one slot (the flip-flop that broke presence AND cancelled mid-flight CLUTCH offers).
    pub device_endpoints: Vec<DeviceEndpoint>,
    /// Which fleet device owns the contact-level `ip`/`local_*` slot: the device we last received DATA from (chat / CLUTCH — the docs/fleet rule: reply-TX to the device in their hand), adopted from the first pong when unset. Only THIS device's address updates move the contact-level slot.
    pub active_device: Option<[u8; 32]>,
    pub relationship_seed: Option<Seed>,
    pub friendship_id: Option<FriendshipId>, // Links to friendship storage (chains live there)
    pub clutch_state: ClutchState,

    // Slot-based CLUTCH state (N-party support)
    /// Our 8 ephemeral keypairs (~512KB secret keys) - generated once per ceremony
    pub clutch_our_keypairs: Option<ClutchAllKeypairs>,
    /// Party slots indexed by sorted handle_hash position Each slot contains offer + kem_secrets for one party (including self)
    pub clutch_slots: Vec<PartySlot>,
    /// Cached ceremony_id - computed from handle_hashes + sorted ping provenances. Uses spaghettify for mixing (no memory-hard step needed). Unique per ceremony due to ping timestamp entropy.
    pub ceremony_id: Option<[u8; 32]>,
    /// Pending KEM response received before our keygen completed Stored here and processed when ceremony_id becomes available
    pub clutch_pending_kem: Option<ClutchKemResponsePayload>,
    /// Track if we've sent our offer (to avoid resending)
    pub clutch_offer_sent: bool,
    /// Our computed eggs_proof (stored while awaiting peer's proof for verification)
    pub clutch_our_eggs_proof: Option<[u8; 32]>,
    /// Peer's eggs_proof if received before we computed ours
    pub clutch_their_eggs_proof: Option<[u8; 32]>,
    /// The ceremony round the stored early proof belongs to (the wire ceremony_id it arrived under). A proof may only ever be COMPARED within its own round — cross-round comparison manufactures "PROOF MISMATCH" out of offer churn (the peer-B/a peer permanent-Pending stall, 2026-07-17). Ceremony scratch like the slots: never persisted.
    pub clutch_their_proof_ceremony: Option<[u8; 32]>,
    /// Bounded retransmit budget for our ClutchComplete proof. The proof is a small single-packet UDP send with no PT retransmit, so one drop would strand the peer in AwaitingProof forever (the asymmetric-completion bug: we go Complete via early-proof, null our state, and never resend). Set to a small N when we compute our eggs proof; ping_contacts re-sends the proof and decrements each cycle until it hits 0, so both sides converge even on a lossy or just-refreshed path. Runtime-only — not persisted (a resumed Complete contact needs no further proof sends).
    pub clutch_proof_resends_left: u8,
    /// Flag to prevent multiple concurrent keygens (race condition guard)
    pub clutch_keygen_in_progress: bool,
    /// Flag to prevent multiple concurrent KEM encapsulations
    pub clutch_kem_encap_in_progress: bool,
    /// Flag to prevent multiple concurrent ceremony completions (avalanche_expand)
    pub clutch_ceremony_in_progress: bool,
    /// HQC public key prefix from peer's last completed ceremony. Used for stale detection: if received offer has same HQC prefix, it's a PT retransmission (stale), not a legitimate re-key request. Stored at completion time, cleared when accepting new ceremony.
    pub completed_their_hqc_prefix: Option<[u8; 8]>,
    /// Collected offer provenances for ceremony nonce derivation. Each offer's VSF header has hp = BLAKE3(signer_pubkey || creation_time_nanos). Sorted and combined via spaghettify to derive unique ceremony_id. Cleared when CLUTCH ceremony completes.
    pub offer_provenances: Vec<[u8; 32]>,

    pub trust_level: TrustLevel,
    pub added: i64,
    /// Roster LWW clock: eagle time of the last change to this contact's SYNCED identity fields (petname / avatar_pin). The fleet roster entry carries it as `updated`; a pulled entry newer than this overwrites those fields, an older one loses. Starts equal to `added`.
    pub roster_updated: i64,
    pub last_seen: Option<i64>,
    pub is_online: bool, // True when we have confirmed bidirectional comms
    pub messages: Vec<ChatMessage>, // Conversation history
    pub message_scroll_offset: f32, // Vertical scroll offset for message area (pixels)
    pub prev_is_online: bool, // For differential rendering (not persisted)
    pub indicator_x: usize, // Cached indicator dot X position (set during draw)
    pub indicator_y: usize, // Cached indicator dot Y position (set during draw)
    pub text_x: f32,     // Cached text X position (set during draw)
    pub text_y: f32,     // Cached text Y position (set during draw)
    // Avatar cache - fetched from FGTW by handle Storage key is deterministic: BLAKE3(BLAKE3(handle) || "avatar")
    pub avatar_pixels: Option<Vec<u8>>, // Full 256x256 VSF RGB pixels (cached)
    pub avatar_scaled: Option<Vec<u8>>, // Pre-scaled to current display size
    pub avatar_scaled_diameter: usize,  // Diameter the scaled pixels were rendered for

    // Chain weave probe — after CLUTCH reaches Complete, both devices auto-exchange one hidden probe chat message each way to prove the ratchet works end-to-end. Once proven, the ceremony proof rebroadcast is cancelled (clutch_proof_resends_left = 0). Runtime-only, not persisted: a resumed Complete contact already has a working chain and needs no re-probe.
    /// The chain has been validated end-to-end (our probe/message got ACKed AND we saw theirs). Gates the status line from "weaving the chain" to "secured" and stops the ceremony rebroadcast.
    pub chain_woven: bool,
    /// We've already sent (or queued) our chain-weave probe for this contact — send it once only.
    pub probe_sent: bool,
    /// We've received the peer's chain-weave probe (their TX chain / our RX proven for at least one hop).
    pub their_probe_seen: bool,
    /// Our own TX chain has advanced via a matching ACK at least once (our TX / their RX proven). Sealed with `their_probe_seen` this is the both-directions-proven condition for `chain_woven`.
    pub chain_advanced_by_ack: bool,
    /// Runtime-only: a punch-validated direct path to this contact `(remote_addr, last_confirmed)`, set when a hole-punch round-trips (see [`crate::network::traverse`]). `race_addrs` prefers it as the primary send address, keeping the public/LAN as the alternate so PT still races if the NAT mapping went stale. Each keepalive ack refreshes `last_confirmed`; once it exceeds the traversal TTL with no ack the path is cleared and re-punched. `Instant` (not eagle-time) because it's never persisted — a resumed session re-punches.
    pub validated_path: Option<(SocketAddr, std::time::Instant)>,
    /// Runtime-only graceful-failure counter: consecutive ping cycles where an ONLINE contact was punched but never validated a direct path (the symmetric↔symmetric case). Past a small threshold the peer is treated as direct-unreachable — the hook the relay milestone (M2) reads. Reset to 0 on any validation.
    pub punch_unvalidated_cycles: u8,
    /// Runtime-only reachability clock (docs/reachability-doorbell.md): the last time ANY signed traffic from this contact's devices reached us — pong, punch ack, chat frame. "The guard's eyes are open." `None` since boot = never heard. Drives the dozed classification: silence past the dozed threshold plus undeliverable traffic = ring the doorbell.
    pub last_heard: Option<std::time::Instant>,
    /// Runtime-only: when we last rang this contact's doorbell — the client-side debounce above the worker's per-target guard. One wake per re-ring window no matter how much traffic queues behind it.
    pub last_ring: Option<std::time::Instant>,
    /// Runtime-only stall counter: consecutive ping cycles spent in `Pending` with our offer sent, a validated direct path up, and still no offer from the peer. The ping cycle re-fires our offer each time this crosses its threshold (then zeroes it) — the pong-driven offer re-send never triggers for a peer whose pongs don't flow, and a one-shot offer whose PT transfer died leaves the ceremony parked forever (2026-07-19 peer-B↔a peer). Reset whenever the stall condition doesn't hold.
    pub clutch_offer_stall_cycles: u8,
    /// Friend-assisted history recovery state machine (newest-first cursor pagination from the friend's copy). `None` = no recovery running/known. Runtime struct; the durable cursor + complete flag persist as `hist_oldest` / `hist_complete` in contact state.
    pub history_recovery: Option<HistoryRecovery>,
    /// Runtime-only: when the CLUTCH ceremony last reached Complete (proof verified). Guards a post-completion RE-KEY COOLDOWN: completion zeroizes our ephemeral keypairs, so a peer's offer that was in flight just before they saw our completion arrives with `clutch_our_keypairs == None` and would trip the "peer lost chains, accept re-key" path — a spurious re-key that, when both sides do it near-simultaneously, storms into divergent ceremonies (observed: two devices wedged at 5/8 and 7/8 forever, though they'd already computed matching eggs). The window opens at completion (before the ~1s-later weave), so it's armed HERE, not at weave. Within it we ignore such offers; a GENUINE reset peer keeps sending and re-keys once it passes. `Instant` — never persisted.
    pub clutch_completed_at: Option<std::time::Instant>,
    /// This "contact" is one of OUR OWN fleet devices (a sibling), not a friend. Siblings run the same full CLUTCH ceremony + braided ratchet as friends — the fleet weave — but key the ceremony on `sibling_party_id(device_pubkey)` (stored in `handle_hash`) instead of the shared handle_hash, which would collide. `handle`/`handle_proof` hold OUR OWN handle so FGTW peer-row address matching works; `public_identity` is the sibling device and `fleet_members` stays EMPTY so `knows_device` answers only that one device (load-bearing for first-match routing). Sibling contacts are excluded from the contacts UI, roster/cloud sync, and friend-history recovery, and persist under the sibling index instead of the contacts index.
    pub is_sibling: bool,
    /// FRIEND-side blind storage: OTP-blinded private-identity-secret blobs this contact's devices deposited WITH US — (depositor device pubkey, 64-byte blob, deposited-at osc). Provably opaque (one-time pad; see `crypto::blind`), served back ONLY to the exact depositing device (the `blind_get` signer), upsert-keyed by device pubkey (a redeposit replaces). Persisted in contact state.
    pub deposited_blinds: Vec<([u8; 32], Vec<u8>, i64)>,
    /// DEPOSITOR-side: our blind is CONFIRMED stored at this friend — their `blind_ack` arrived, which they send only after their own disk commit. This is the signal that flips S Provisional→Live (a crash before any ack orphans nothing: no tag was ever emitted). Persisted (absent = false).
    pub blind_deposited: bool,
    /// Runtime-only: the one in-flight blind op toward this contact — (rid, sent_osc, is_get). `drive_blind_ops` expires it after ~15s and retries; responses match on rid.
    pub blind_in_flight: Option<([u8; 32], i64, bool)>,
    /// Runtime-only: this friend answered our probe with `found=0` (no deposit for this device). When every online+woven friend has missed AND no probe is in flight, S genuinely doesn't exist and genesis may run (probe-before-generate — a reset device must RECOVER S, never regenerate it while a deposit is reachable).
    pub blind_probe_missed: bool,
}

/// Contact identifier - BLAKE3 hash of the contact's public identity key This provides deterministic, collision-resistant identification
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ContactId([u8; 32]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustLevel {
    Stranger,
    Known,
    Trusted,
    Inner,
}

impl ContactId {
    /// Create ContactId from public identity key (deterministic)
    pub fn from_pubkey(pubkey: &DevicePubkey) -> Self {
        Self(*blake3::hash(pubkey.as_bytes()).as_bytes())
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Contact {
    /// FIRST-MET constructor: the one moment the handle string exists on this side. Derives the whole pin-set — party id (identity pubkey), avatar-wall key — and lets both the string and the seed drop; nothing with signing power (or that derives it) lands in the row (docs/identity-profile.md).
    pub fn new(handle: HandleText, handle_proof: [u8; 32], public_identity: DevicePubkey) -> Self {
        let seed = crate::types::Handle::to_identity_seed(handle.as_str());
        let handle_hash = crate::crypto::clutch::identity_party_id(&seed);
        // NO handle-derived avatar pin: that made the avatar readable by anyone who knew the handle (docs/identity-profile.md). A fresh contact starts UNPINNED (zero) — its real pin (random key ‖ lookup) arrives over an authenticated pong once the friendship is mutual, exactly like the published name. Until then the gradient avatar renders.
        Self::from_pin(String::new(), [0u8; 64], handle_proof, handle_hash, public_identity)
    }

    /// PIN-SET constructor: reconstruct a contact from stored/synced material (vault rows, roster entries) — no handle anywhere.
    pub fn from_pin(
        petname: String,
        avatar_pin: [u8; 64],
        handle_proof: [u8; 32],
        party_id: [u8; 32],
        public_identity: DevicePubkey,
    ) -> Self {
        Self {
            id: ContactId::from_pubkey(&public_identity),
            petname,
            published_name: String::new(),
            published_name_dirty: false,
            avatar_pin_dirty: false,
            avatar_pin,
            handle_proof,
            handle_hash: party_id,
            public_identity,
            fleet_members: Vec::new(), // Folded from the friend's chain on the first refresh
            fleet_folded_once: false,  // Armed only by a successful adopted fold
            fleet_members_ts: 0,       // No fold adopted yet (bootstrap)
            pinned_genesis: [0u8; 32], // Pinned at the first adopted fold
            identity_ended: false,
            identity_superseded: false,
            ip: None,
            local_ip: None,   // Discovered via LAN broadcast
            local_port: None, // Discovered via LAN broadcast
            device_endpoints: Vec::new(),
            active_device: None,
            relationship_seed: None,
            friendship_id: None, // Set after CLUTCH ceremony completes
            clutch_state: ClutchState::Pending,
            // Slot-based CLUTCH fields
            clutch_our_keypairs: None,
            clutch_slots: Vec::new(),    // Initialized when ceremony starts
            ceremony_id: None,           // Computed from handle_hashes + ping provenances
            clutch_pending_kem: None,    // KEM response received before keygen completed
            clutch_offer_sent: false,    // Track if we've sent our offer
            clutch_our_eggs_proof: None, // Our proof (stored while awaiting peer's)
            clutch_their_eggs_proof: None, // Peer's proof (if received early)
            clutch_their_proof_ceremony: None, // The round that early proof belongs to
            clutch_proof_resends_left: 0, // Bounded proof-retransmit budget (runtime only)
            clutch_keygen_in_progress: false, // No keygen running yet
            clutch_kem_encap_in_progress: false, // No KEM encap running yet
            clutch_ceremony_in_progress: false, // No ceremony completion running yet
            completed_their_hqc_prefix: None, // Set when CLUTCH completes, persisted
            offer_provenances: Vec::new(), // Collected offer provenances for ceremony nonce
            trust_level: TrustLevel::Stranger,
            added: vsf::eagle_time_oscillations(),
            roster_updated: vsf::eagle_time_oscillations(),
            last_seen: None,
            is_online: false,           // Starts offline until we confirm comms
            messages: Vec::new(),       // No messages yet
            message_scroll_offset: 0.0, // Starts at top (scrolled to latest when messages added)
            prev_is_online: false,      // Match initial state
            indicator_x: 0,             // Set during first draw
            indicator_y: 0,             // Set during first draw
            text_x: 0.0,                // Set during first draw
            text_y: 0.0,                // Set during first draw
            avatar_pixels: None,        // Fetched from FGTW by handle when online
            avatar_scaled: None,        // Scaled on demand for display
            avatar_scaled_diameter: 0,
            chain_woven: false,           // Chain not yet proven end-to-end (probe pending)
            probe_sent: false,            // Chain-weave probe not sent yet
            their_probe_seen: false,      // Haven't seen their chain-weave probe yet
            chain_advanced_by_ack: false, // Our TX chain not yet ACK-advanced
            validated_path: None,         // No punch-validated direct path yet
            punch_unvalidated_cycles: 0,  // No failed punch cycles yet
            clutch_offer_stall_cycles: 0, // No stalled-offer cycles yet
            last_heard: None,             // No signed traffic from them yet this session
            last_ring: None,              // Doorbell never rung this session
            history_recovery: None,       // No history recovery running
            clutch_completed_at: None,         // Ceremony not yet complete
            is_sibling: false,            // A friend, unless made via new_sibling
            deposited_blinds: Vec::new(), // No blinds deposited with us yet
            blind_deposited: false,       // Our blind not confirmed at this friend yet
            blind_in_flight: None,        // No blind op in flight
            blind_probe_missed: false,    // No probe answered found=0 yet
        }
    }

    /// Construct a fleet-sibling contact: one of our OWN devices, discovered from our folded membership chain. Party id (in `handle_hash`) is device-derived so the ceremony machinery can't collide with the self/friend id space; `handle_proof` is OUR OWN so `refresh_contact_addrs_from_peers` matches the sibling's FGTW peer row (keyed hp + device pubkey — no handle string needed). Trust is implicit — the pubkey came from our own fold.
    pub fn new_sibling(our_handle_proof: [u8; 32], sibling_device: DevicePubkey) -> Self {
        let party_id = crate::crypto::clutch::sibling_party_id(&sibling_device.key);
        let mut c = Self::from_pin(String::new(), [0u8; 64], our_handle_proof, party_id, sibling_device);
        c.is_sibling = true;
        c.trust_level = TrustLevel::Inner;
        c
    }

    /// The name this contact renders as everywhere: local petname → their published profile name → the keyed two-word voca pseudonym from the party id. No handle: the string that derives an identity exists at rest nowhere (docs/identity-profile.md). Names carry ZERO trust — the pinned key does.
    pub fn display_name(&self) -> String {
        if !self.petname.is_empty() {
            return self.petname.clone();
        }
        if !self.published_name.is_empty() {
            return self.published_name.clone();
        }
        crate::network::fgtw::fleet::keyed_pseudonym(&self.handle_hash)
    }

    /// True once we have a REAL name — a petname we set or the name they published. Until then the only "name" is the deterministic voca pseudonym, which reads like a real name (PotatoOctopus) then jarringly flips to the actual name once it arrives.
    pub fn has_real_name(&self) -> bool {
        !self.petname.is_empty() || !self.published_name.is_empty()
    }

    /// Name for the VISUAL surfaces (contact row, conversation header): the real name if we have one, else "Pending…" — never the pseudonym. The deterministic gradient avatar (hash-computed) carries the visual identity meanwhile. `display_name` still returns the pseudonym for stable non-visual uses (search filters, log labels).
    pub fn display_name_or_pending(&self) -> String {
        if self.has_real_name() {
            self.display_name()
        } else {
            "Pending\u{2026}".to_string()
        }
    }

    pub fn with_ip(mut self, ip: SocketAddr) -> Self {
        self.ip = Some(ip);
        self
    }

    /// Record the same-LAN address FGTW reported for this device's last announce, alongside the public address from [`with_ip`](Self::with_ip). Lets the offer/KEM send race the LAN path against the WAN path without waiting for LAN multicast discovery (which routers often drop). Only IPv4 LAN addresses are stored — `local_ip`/`local_port` are typed for the v4 hairpin case; a v6 local address is left unset and the WAN path carries it.
    pub fn with_local_ip(mut self, local_ip: Option<std::net::IpAddr>, port: u16) -> Self {
        if let Some(std::net::IpAddr::V4(v4)) = local_ip {
            self.local_ip = Some(v4);
            self.local_port = Some(port);
        }
        self
    }

    pub fn with_seed(mut self, seed: Seed) -> Self {
        self.relationship_seed = Some(seed);
        self
    }

    pub fn with_trust_level(mut self, level: TrustLevel) -> Self {
        self.trust_level = level;
        self
    }

    pub fn update_last_seen(&mut self, timestamp: i64) {
        self.last_seen = Some(timestamp);
    }

    /// Returns the (primary, alternate) address pair for racing a transfer across the reachable paths. A punch-validated direct path (from NAT traversal) wins as primary when present, with the public/LAN kept as the alternate so PT still races if the validated mapping went stale. Otherwise: primary is the LAN address (preferred — no router hairpin, no AP isolation), alternate is the public address; and when no LAN address is known, primary is the public address and alternate is `None`. PT sends the SPEC to both and locks onto whichever ACKs first (see [`crate::network::pt::PtManager::send_with_pubkey_and_alt`]).
    /// Upsert the per-device endpoint for `pubkey` and apply `update` to it. Linear scan — fleets are single-digit sized.
    pub fn endpoint_mut(&mut self, pubkey: &[u8; 32]) -> &mut DeviceEndpoint {
        if let Some(i) = self.device_endpoints.iter().position(|e| e.pubkey == *pubkey) {
            return &mut self.device_endpoints[i];
        }
        self.device_endpoints.push(DeviceEndpoint { pubkey: *pubkey, public: None, lan: None, online: false });
        self.device_endpoints.last_mut().unwrap()
    }

    /// Any device of this contact's fleet currently answering pings. The contact-level online ring shows the IDENTITY reachable, not one particular device.
    pub fn any_device_online(&self) -> bool {
        self.device_endpoints.iter().any(|e| e.online)
    }

    pub fn race_addrs(&self) -> Option<(SocketAddr, Option<SocketAddr>)> {
        // A punch-validated direct path wins — it's proven reachable right now. Keep the best DISTINCT candidate as the alternate so a stale NAT mapping still falls back via PT's race.
        if let Some((validated, _at)) = self.validated_path {
            let alt = crate::network::traverse::gather::gather_peer_candidates(self)
                .sorted()
                .into_iter()
                .map(|c| c.addr)
                .find(|a| *a != validated);
            return Some((validated, alt));
        }

        // No proven path yet: try candidates in priority order — global IPv6 host first (no NAT, no punch), then IPv6 reflexive, then IPv4 LAN, then IPv4 reflexive (the punched WAN path). This is the v6-first send order; it replaces the old LAN-first-then-public choice, so a reachable v6 address is tried before a v4 LAN address that may belong to a foreign network (the Seattle↔Montana `192.168.0.x` collision). Falls back to nothing only when we know no address at all.
        crate::network::traverse::gather::gather_peer_candidates(self).best_pair()
    }

    /// True once the CLUTCH ceremony is Complete — which is cryptographically impossible unless BOTH parties ran it, so it doubles as the mutual-consent signal ("we each added the other"). Used to gate friend-only behaviour like the direct peer-to-peer avatar exchange.
    pub fn is_mutual(&self) -> bool {
        self.clutch_state == ClutchState::Complete
    }

    /// Does `device_pubkey` belong to this contact's identity? Trust respects the fold.
    /// Pre-fold (`fleet_folded_once == false`, i.e. bootstrap or a sibling that never folds): trust the first-met device (`public_identity`) OR any cached member — keeps a fresh first-met friend and sibling contacts working before any successful fold.
    /// Post-fold (`fleet_folded_once == true`): the friend's chain has authoritatively spoken, so trust ONLY current folded members. `public_identity` keeps its pass iff it's still a member (the device we met is still in their fleet), and loses it if the fold excluded it — that device was removed, which is what makes revocation real.
    /// A fold FAILURE never reaches here (it doesn't arm the flag), so a network outage keeps the last-known behaviour, never trust-nobody.
    pub fn knows_device(&self, device_pubkey: &[u8; 32]) -> bool {
        if self.fleet_folded_once {
            self.fleet_members.contains(device_pubkey)
        } else {
            self.public_identity.key == *device_pubkey || self.fleet_members.contains(device_pubkey)
        }
    }

    /// Every device pubkey we'll answer for this contact — feeds the status checker's answerable set. Mirrors `knows_device`: post-fold it's exactly the folded members (public_identity included only if it's still one); pre-fold it's public_identity unioned with any cached members, deduped, first-met leading.
    pub fn answerable_pubkeys(&self) -> Vec<[u8; 32]> {
        if self.fleet_folded_once {
            self.fleet_members.clone()
        } else {
            let mut v = Vec::with_capacity(self.fleet_members.len() + 1);
            v.push(self.public_identity.key);
            for m in &self.fleet_members {
                if *m != self.public_identity.key {
                    v.push(*m);
                }
            }
            v
        }
    }

    pub fn can_be_custodian(&self) -> bool {
        matches!(self.trust_level, TrustLevel::Trusted | TrustLevel::Inner)
    }

    /// Get the cached ceremony ID for CLUTCH with this contact.
    ///
    /// The ceremony_id is computed once during background keygen and cached. It's deterministic from sorted handle_hashes, so both parties compute same value. Returns None if keygen hasn't completed yet.
    pub fn get_ceremony_id(&self) -> Option<CeremonyId> {
        self.ceremony_id.map(CeremonyId::from_bytes)
    }

    /// Initialize CLUTCH slots for a 2-party ceremony. Slots are indexed by sorted handle_hash position.
    pub fn init_clutch_slots(&mut self, our_handle_hash: [u8; 32]) {
        let mut hashes = vec![our_handle_hash, self.handle_hash];
        hashes.sort();

        self.clutch_slots = hashes.into_iter().map(PartySlot::new).collect();
    }

    /// Get the slot index for a given handle_hash. Returns None if the handle_hash is not in the ceremony.
    pub fn get_slot_index(&self, handle_hash: &[u8; 32]) -> Option<usize> {
        self.clutch_slots
            .iter()
            .position(|s| &s.handle_hash == handle_hash)
    }

    /// Get mutable reference to the slot for a given handle_hash.
    pub fn get_slot_mut(&mut self, handle_hash: &[u8; 32]) -> Option<&mut PartySlot> {
        self.clutch_slots
            .iter_mut()
            .find(|s| &s.handle_hash == handle_hash)
    }

    /// Get reference to the slot for a given handle_hash.
    pub fn get_slot(&self, handle_hash: &[u8; 32]) -> Option<&PartySlot> {
        self.clutch_slots
            .iter()
            .find(|s| &s.handle_hash == handle_hash)
    }

    /// Check if all slots are complete (ceremony can finish). For 2-party: both slots have offer + both KEM secret directions.
    pub fn all_slots_complete(&self) -> bool {
        !self.clutch_slots.is_empty() && self.clutch_slots.iter().all(|s| s.is_complete())
    }

    /// The eight independent KEM "eggs" a CLUTCH braids, across three security families — a compromise of any one family still leaves the shared secret protected by the others. Named here so the status line can say WHAT is being exchanged, not just "pending". 4 elliptic-curve (x25519, P-384, secp256k1, P-256) · 2 lattice (Frodo-976, NTRU-701) · 2 code-based (McEliece-460896, HQC-256).
    pub const CLUTCH_EGGS: usize = 8;

    /// Total ceremony milestones for a 2-party CLUTCH — the denominator of the status fraction. Keygen, our offer, their offer, our KEM, their KEM, braid, their proof, verify.
    pub const CLUTCH_STEPS: u8 = 8;

    /// Where the CLUTCH ceremony actually is, as `step/total · what's happening (which eggs)` — for the conversation status line and asymmetric-completion debugging. The fraction is monotonic toward `secured`; the label names the crypto in flight (the egg families) instead of a flat "pending".
    pub fn clutch_status_detail(&self) -> String {
        // Display doctrine (2026-07-16): dozenal is the acclimation surface for VERSION + REPUTATION only; a step counter stays in current mixed arabic units.
        let n = Self::CLUTCH_STEPS;
        let eggs = Self::CLUTCH_EGGS;
        match self.clutch_state {
            ClutchState::Complete => {
                if self.chain_woven {
                    "secured".to_string()
                } else {
                    "testing · weaving the chain".to_string()
                }
            }
            ClutchState::AwaitingProof => {
                if self.clutch_their_eggs_proof.is_some() {
                    format!("{n}/{n} · verifying proof")
                } else {
                    format!("7/{n} · braided · awaiting their proof")
                }
            }
            ClutchState::Pending => {
                let their_offer = self.clutch_slots.iter().any(|s| s.offer.is_some() && s.handle_hash != self.handle_hash);
                let all_kem = self.all_slots_complete();
                // Walk the milestones in order; report the earliest one not yet reached.
                if self.clutch_ceremony_in_progress {
                    format!("6/{n} · braiding {eggs} eggs")
                } else if self.clutch_our_keypairs.is_none() {
                    // Keygen (McEliece dominates the ~1-2s) — name the egg families being forged.
                    format!("1/{n} · forging {eggs} eggs (4 EC · 2 lattice · 2 code)")
                } else if self.clutch_kem_encap_in_progress {
                    format!("4/{n} · sealing KEMs (McEliece·HQC·Frodo·NTRU)")
                } else if all_kem {
                    format!("6/{n} · braiding {eggs} eggs")
                } else if their_offer || self.clutch_pending_kem.is_some() {
                    format!("5/{n} · awaiting their KEMs")
                } else if self.clutch_offer_sent {
                    format!("3/{n} · awaiting their eggs")
                } else {
                    format!("2/{n} · sending offer")
                }
            }
        }
    }

    /// Insert a message in sorted order by timestamp (oldest first). Uses binary search for O(log n) position finding.
    pub fn insert_message_sorted(&mut self, msg: ChatMessage) {
        // Binary search for insertion point (maintains ascending timestamp order)
        let pos = self
            .messages
            .binary_search_by(|m| m.timestamp.cmp(&msg.timestamp))
            .unwrap_or_else(|pos| pos);
        self.messages.insert(pos, msg);
    }
}

#[cfg(test)]
mod fold_honour_tests {
    use super::*;

    fn contact_with(pk: [u8; 32]) -> Contact {
        Contact::new(
            HandleText::new("friend"),
            [0x11; 32],
            DevicePubkey::from_bytes(pk),
        )
    }

    #[test]
    fn knows_first_met_device_before_any_refresh() {
        let c = contact_with([1u8; 32]);
        assert!(c.knows_device(&[1u8; 32]), "the device we met is always known");
        assert!(!c.knows_device(&[2u8; 32]), "a stranger device is not");
    }

    #[test]
    fn knows_any_folded_member_and_answerable_dedups() {
        let mut c = contact_with([1u8; 32]);
        c.fleet_members = vec![[1u8; 32], [2u8; 32], [3u8; 32]];
        assert!(c.knows_device(&[2u8; 32]), "a sibling device folds in");
        assert!(c.knows_device(&[3u8; 32]));
        assert!(!c.knows_device(&[9u8; 32]), "still rejects a non-member");
        // public_identity (1) appears once, siblings 2 and 3 follow — no duplicate of 1.
        let ans = c.answerable_pubkeys();
        assert_eq!(ans.len(), 3, "first-met + 2 distinct siblings, deduped: {ans:?}");
        assert_eq!(ans[0], [1u8; 32], "first-met device leads");
    }

    #[test]
    fn folded_trust_includes_public_identity_when_still_a_member() {
        // The normal post-fold case: the device we first met is still in the friend's fleet, so it stays trusted VIA membership.
        let mut c = contact_with([1u8; 32]);
        c.fleet_members = vec![[1u8; 32], [2u8; 32]];
        c.fleet_folded_once = true;
        assert!(c.knows_device(&[1u8; 32]), "first-met is still a member");
        assert!(c.knows_device(&[2u8; 32]), "their other device is a member");
        assert!(!c.knows_device(&[9u8; 32]), "a stranger is rejected");
        // Post-fold, answerable is exactly the folded members (no unconditional public_identity prepend).
        assert_eq!(c.answerable_pubkeys(), vec![[1u8; 32], [2u8; 32]]);
    }

    #[test]
    fn folded_trust_revokes_public_identity_when_removed() {
        // Revocation: the fold no longer includes the first-met device (it was removed), so it LOSES trust — this is the whole point of fold-respecting trust.
        let mut c = contact_with([1u8; 32]);
        c.fleet_members = vec![[2u8; 32]]; // first-met (1) is gone; only 2 remains
        c.fleet_folded_once = true;
        assert!(!c.knows_device(&[1u8; 32]), "removed first-met device is no longer trusted");
        assert!(c.knows_device(&[2u8; 32]), "the current member is trusted");
        assert_eq!(c.answerable_pubkeys(), vec![[2u8; 32]], "answerable drops the revoked device");
    }

    #[test]
    fn sibling_never_arms_stays_bootstrap() {
        // A sibling contact never folds a contact-chain, so fleet_folded_once stays false and knows_device answers only the one sibling device (empty fleet_members is load-bearing for first-match routing).
        let sib = Contact::new_sibling([0x22; 32], DevicePubkey::from_bytes([5u8; 32]));
        assert!(!sib.fleet_folded_once, "siblings are never armed");
        assert!(sib.knows_device(&[5u8; 32]), "the sibling device is trusted (bootstrap)");
        assert!(!sib.knows_device(&[6u8; 32]), "another device is not");
    }

    #[test]
    fn new_sibling_keys_on_device_pid_and_slots_stay_distinct() {
        let sib_device = [5u8; 32];
        let sib = Contact::new_sibling([0x22; 32], DevicePubkey::from_bytes(sib_device));
        assert!(sib.is_sibling);
        // Party id is device-derived, NOT the handle-derived seed (which every sibling would share).
        assert_eq!(
            sib.handle_hash,
            crate::crypto::clutch::sibling_party_id(&sib_device)
        );
        assert_ne!(sib.handle_hash, crate::types::Handle::to_identity_seed("me"));
        // ContactId is device-keyed — two siblings of one handle never collide.
        assert_eq!(sib.id, ContactId::from_pubkey(&sib.public_identity));
        // knows_device answers ONLY that one device (empty fleet_members is load-bearing for first-match routing).
        assert!(sib.knows_device(&sib_device));
        assert!(!sib.knows_device(&[6u8; 32]));

        // Slot init with OUR device-derived pid: two distinct sorted slots, both findable — the exact collision the shared handle_hash caused.
        let our_pid = crate::crypto::clutch::sibling_party_id(&[6u8; 32]);
        let mut sib = sib;
        sib.init_clutch_slots(our_pid);
        assert_eq!(sib.clutch_slots.len(), 2);
        assert_ne!(sib.clutch_slots[0].handle_hash, sib.clutch_slots[1].handle_hash);
        assert!(sib.get_slot(&our_pid).is_some());
        assert!(sib.get_slot(&sib.handle_hash).is_some());
    }
}
