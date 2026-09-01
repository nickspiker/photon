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
    /// The DEVICE that signed this slot's offer — the ceremony's actual participant. Eggs fold a device-pubkey pair, and deriving "their device" from the PINNED contact device (public_identity, re-elected by every pong) desynced multi-device fleets: the friend pins device A while device B wins the ceremony → one egg differs → PROOF MISMATCH on an otherwise perfect round (live pair 2026-07-24). Both sides using the offer SIGNER agree by construction — the ceremony id already binds the offer pair. None on legacy-persisted slots → completion falls back to the pinned device (correct for single-device friends).
    pub offer_device: Option<[u8; 32]>,
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
            offer_device: None,
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
/// What a referencing row IS to its target: a reply to it, an edit superseding it, or a reaction on it. Wire/storage value is the discriminant (u3 on the wire, u64 in the row record) — zero is reserved for "no reference" so an absent field and an explicit none read the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Reply = 1,
    Edit = 2,
    React = 3,
    /// BRIDGE command OUTPUT. A sibling conversation is a chat-as-shell: every plain incoming sibling line is a command the host runs; the host's reply carries THIS typed kind so it renders as an ordinary visible bubble but is never re-executed (the anti-bounce flag). Typed wire field, NOT a content sentinel — the reply/edit/react rework's whole point was to stop embedding markers in human text; the bridge honours the same rule. No target row, so the reference's `i64` is 0 and ignored.
    BridgeOut = 4,
    /// BRIDGE session RESET — the client sends this when it OPENS the bridge, telling the host to kill its persistent shell for us and start fresh (Nick 2026-08-22: "opening the bridge nukes any terminal open, clears and spawns new"). A hidden control row: never displayed, never run as a command. No target; `i64` is 0.
    BridgeReset = 5,
    /// BRIDGE COMMAND — the typed mark that says "run me" (2026-08-23). Execution used to key on being any plain sibling row (with a legacy `$ ` content prefix merely stripped) — content-as-trigger, the exact class the reply/edit/react rework exists to kill. The client stamps every bridge-conversation send with this kind; the host runs ONLY rows carrying it (a transition arm still accepts bare legacy rows, loudly). No target; `i64` is 0.
    BridgeCmd = 6,
    /// BRIDGE INTERRUPT — the operator's stop lever (Ctrl+K / the Stop pill), targeting the command row's eagle_time. The signal number rides the typed `bsig` wire field; the host signals the command's own process group, never bash. Hidden control row; late arrival after completion is a natural no-op.
    BridgeCtl = 7,
}

impl RefKind {
    pub fn from_wire(v: u8) -> Option<RefKind> {
        match v {
            1 => Some(RefKind::Reply),
            2 => Some(RefKind::Edit),
            3 => Some(RefKind::React),
            4 => Some(RefKind::BridgeOut),
            5 => Some(RefKind::BridgeReset),
            6 => Some(RefKind::BridgeCmd),
            7 => Some(RefKind::BridgeCtl),
            _ => None,
        }
    }
}

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
    /// TOMBSTONE: the message is deleted-for-everyone — hidden from every UI, propagated monotonically (true wins) thru fleet sync AND to the friend via the hidden delete marker. The CONTENT IS PRESERVED internally on purpose: the braid weaves prior message content into future keys, so blanking it would fork chains that later weave this row — true content shredding needs a braid-safe redaction design (ticketed). Persisted.
    pub deleted: bool,
    /// TYPED reference metadata: this row points at another row (by eagle_time) — a reply to it, an edit superseding it, or a reaction on it. `content` then carries ONLY the body (reply text / corrected text / reaction glyph — empty glyph = retract). A field, never a string encoding: metadata smuggled thru content is what put marker droppings on old builds' screens (field, 2026-08-09). Rides its own wire field, row-record fields, and history-page columns; the braid is untouched (strands weave content, which stays byte-identical both sides). Persisted; absent on pre-feature rows.
    pub reference: Option<(RefKind, i64)>,
    /// The fleet has DISCHARGED its alert duty for this row exactly once (docs design 2026-07-23): true the moment any device dings for it OR displays it live (the active-clearer). Explicitly NOT "read" — never records whether the human looked. Synced fleet-internally like `delivered` (true wins, monotone; rides the sibling row pushes + history pages, sealed under fleet keys — friends never see it). Absent on pre-feature rows/pages = TRUE, so history can never re-ding. Outgoing rows are born true (your own sends never alert you).
    pub notified: bool,
    /// FLEET-REPLICATED (the delivery ladder's middle state, 2026-09-01): a sibling provably holds this outgoing row — flipped when a fold-trusted sibling page carries our copy back, or when a sibling pong's anti-entropy (count, digest) exactly matches ours. RUNTIME-ONLY v1 (not persisted, not on the wire — re-earned each session on the next gossip/digest edge); `delivered` outranks it. The ladder: sending → replicated (our fleet holds it) ∥ delivered (their fleet ACKed — the line; there is nothing beyond it, "seen" is only ever a human's explicit reaction).
    pub replicated: bool,
    /// BRIDGE runtime only (never persisted — bridge rows are ephemeral): the newest streamed snapshot's sequence on a BridgeOut row, so an out-of-order or duplicated partial can never regress the display.
    pub bridge_seq: u64,
    /// BRIDGE runtime only: the exit code once this BridgeOut row's command completed — present = FINAL frame arrived, the in-flight predicate's other half.
    pub bridge_exit: Option<i64>,
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
            deleted: false,
            reference: None,
            notified: is_outgoing,
            bridge_seq: 0,
            replicated: false,
            bridge_exit: None,
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
            deleted: false,
            reference: None,
            notified: is_outgoing,
            bridge_seq: 0,
            replicated: false,
            bridge_exit: None,
        }
    }

    /// Builder: attach the typed reference (reply/edit/react target).
    pub fn with_reference(mut self, kind: RefKind, target_ts: i64) -> Self {
        self.reference = Some((kind, target_ts));
        self
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
    /// Consecutive pages that arrived but failed to decrypt (a key/era divergence, not transport). Cleared by any page that opens.
    pub decrypt_fail_streak: u32,
    /// Divergence park: fp of the history key that kept failing. While the CURRENT key still fingerprints the same, the walk stays parked — re-requesting just re-downloads 17KB of undecryptable page per cycle forever (field 2026-08-21: 161 pages in 35 min, both fleets). The moment the key CHANGES (re-key completed, era adopted) the fingerprint differs and the walk resumes — an edge expressed as state, no timer.
    pub parked_key_fp: Option<[u8; 4]>,
    /// Consecutive in-flight requests that EXPIRED unanswered (transport black hole, not decrypt). Each one doubles the trickle wait (capped) — a dead route degrades to a slow heartbeat instead of a re-request storm (field 2026-08-31: 7,316 expiry re-requests in one log). Cleared by any page arriving.
    pub expire_streak: u32,
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

/// Prefix for the hidden DELETE marker message: `{prefix}{timestamp}` — a normal chain message (ACKed, retransmitted, re-ACK-durable via its stored hidden row, exactly the probe pattern) instructing the peer to tombstone the row with that eagle timestamp. Control bytes make user-content collision effectively impossible.
pub const DELETE_MARKER_PREFIX: &str = "\u{1}\u{2}photon-delete\u{2}\u{1}";

/// Prefix for call-signaling rows (docs/calls.md): offer/answer/decline/busy/hangup/taken ride the lanes as ordinary encrypted messages — a call is indistinguishable from a text on the wire. Grammar + parsing live in `crate::call::signal`; the type layer only owns the marker so `is_control_content` can hide them.
pub const CALL_PREFIX: &str = "\u{1}\u{2}photon-call\u{2}\u{1}";


/// True for any CONTROL message content (chain probe, delete marker, call signaling) — machinery rows that no UI, digest, weave window, or history page may surface.
pub fn is_control_content(content: &str) -> bool {
    content == CHAIN_PROBE_MARKER
        || content.starts_with(DELETE_MARKER_PREFIX)
        || content.starts_with(CALL_PREFIX)
}

/// Attachment row marker. NOT control content — attachment rows are VISIBLE messages (bubble = pill), they ACK, sync fleet-wide, tombstone, and weave like any row; only their DISPLAY differs. The content string is the whole record: `PREFIX + blake3_hex(64) + \u{2} + filename + \u{2} + size_bytes` — riding the ordinary content field means zero codec changes anywhere (vault, history pages, fleet sync all carry it as text). The blob itself travels separately over PT (attach_blob frames) and lives as a sealed file beside the vault, NEVER in a row.
pub const ATTACHMENT_PREFIX: &str = "\u{1}\u{2}photon-attach\u{2}\u{1}";

/// Build an attachment row's content string.
pub fn attachment_content(content_hash: &[u8; 32], filename: &str, size: u64) -> String {
    format!(
        "{}{}\u{2}{}\u{2}{}",
        ATTACHMENT_PREFIX,
        hex::encode(content_hash),
        filename.replace('\u{2}', " "),
        size
    )
}

/// Parse an attachment row's content → (content_hash, filename, size). None for non-attachment content or a malformed record.
pub fn parse_attachment_content(content: &str) -> Option<([u8; 32], String, u64)> {
    let rest = content.strip_prefix(ATTACHMENT_PREFIX)?;
    let mut parts = rest.splitn(3, '\u{2}');
    let hash_hex = parts.next()?;
    let name = parts.next()?;
    let size: u64 = parts.next()?.parse().ok()?;
    let bytes = hex::decode(hash_hex).ok()?;
    let hash: [u8; 32] = bytes.try_into().ok()?;
    Some((hash, name.to_string(), size))
}

/// Human-readable byte size for attachment pills — dozenal-doctrine exempt? NO: digits render at the edge; this returns arabic-free unit steps with the NUMBER left to the renderer. Kept simple: returns (whole_units, unit_label) so the caller renders the number in dozenal glyphs.
pub fn size_units(size: u64) -> (u32, &'static str) {
    const KI: u64 = 1024;
    if size >= KI * KI {
        (((size + KI * KI / 2) / (KI * KI)) as u32, "MB")
    } else if size >= KI {
        (((size + KI / 2) / KI) as u32, "kB")
    } else {
        (size as u32, "B")
    }
}

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

/// Cycles an online contact may go punched-but-unvalidated before it's treated as direct-unreachable — read by the relay tiering AND the seed-registry resolve (a contact stuck here needs FRESH addresses, whatever stale `ip` it carries: the 2026-08-17 flap — a cross-subnet LAN path validating and dying over and over — kept a perfectly-reachable WAN v6 from ever being resolved, because the resolve gated on "has an address").
pub const PUNCH_UNREACHABLE_THRESHOLD: u8 = 3;

#[derive(Clone, Debug)]
pub struct Contact {
    pub id: ContactId,
    /// The friend's published profile name, adopted from their pong's always-granted name slot and synced across OUR fleet via the roster (PRST4). Empty renders the keyed voca pseudonym; carries zero trust — the pinned key does. Never defaulted to the typed handle: the handle string derives the identity seed, so storing it anywhere re-creates the honeypot (docs/identity-profile.md).
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
    /// The first-met/pinned device key — `None` until a real key is learned (a stateless load, a cloud-restored row from a pre-ke blob). ABSENCE IS ABSENT (the zero-sentinel purge, 2026-08-30): this was a zero-filled placeholder that leaked into ping scheduling ("pinged 00000000"), relay lists, and ContactId derivation (every stateless contact collided on blake3(zeros)).
    pub public_identity: Option<DevicePubkey>,
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
    /// LOCKED OUT (treat-as-stolen): this sibling device stays a permanent chain member — removal is self-signed only, zero exceptions — but the fleet has stopped trusting it: no pings, no pongs honoured, no chain-sync, no ceremony, no relay copies, and the fleet key rotates away from it. Set from the fleet-synced `fleet.locked` set (grow-only); persisted so the refusal survives a relaunch even before settings sync.
    pub locked_out: bool,
    /// FRIEND-side refusal (device-trust-and-recovery.md's one knob): this friend's devices we refuse regardless of fold membership — `effective_trust = member AND NOT refused`. Populated from the fleet's reported-stolen signal (sealed pong tail) once TWO DISTINCT live devices report the same pubkey; persisted, monotonic (unlock is a deliberate future flow).
    pub refused_devices: Vec<[u8; 32]>,
    /// The raw reported-stolen ledger: (reporter device, reported device) pairs seen this SESSION. Runtime-only — the threshold reads distinct reporters from here; refusal, once granted, persists in `refused_devices`.
    pub locked_reports_seen: Vec<([u8; 32], [u8; 32])>,
    /// A chain with a DIFFERENT genesis appeared under this contact's name — a stranger re-claimed the freed handle. Folds are refused; rendered as NOT-them. Never auto-clears (the pin is permanent testimony).
    pub identity_superseded: bool,
    pub ip: Option<SocketAddr>, // The ACTIVE device's public IP:port (see `active_device`) — the primary TX target
    pub local_ip: Option<Ipv4Addr>, // The ACTIVE device's LAN IP (hairpin NAT workaround)
    pub local_port: Option<u16>, // The ACTIVE device's LAN port
    /// Runtime-only: this contact's address inside a live Wi-Fi Direct group (192.168.49.x). Kept SEPARATE from `local_ip` so a p2p address never hits the foreign-LAN `/24` gate — group membership itself vouches reachability. Cleared on group teardown (IFACE-LOST edge) so sends fall back instead of black-holing.
    pub p2p_addr: Option<SocketAddr>,
    /// PERSISTED: the pair's pre-provisioned Wi-Fi Direct group credential (docs/offgrid.md) — minted by the lexicographically-lower device, exchanged sealed over the normal channel while online, so an offline meetup needs zero bootstrap radio.
    pub wfd_cred: Option<crate::network::wfd::WfdCred>,
    /// Runtime-only: the GO-side credential offer fired this session (once per session; a lost frame self-heals next session, the receiver adopts idempotently by epoch).
    pub wfd_cred_sent: bool,
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
    /// Eagle time this ceremony round's keypairs were minted (the round started). Ephemeral, never persisted. Two uses, both serving the rule that re-key is a DELIBERATE act on real failure — never a reflex to transient key loss: (1) a routine resume reloads contacts from disk and wipes the ephemeral keypairs; if this stamp is fresh we RESTORE the in-flight round rather than let the keygen sweep mint a divergent one the peer never agreed to (the relay ceremony stall — a slow relay round-trip outlived the keys); (2) the keygen/re-key sweep only fires when a round is genuinely stale by this clock, not the instant keypairs read `None`. `None` = no round in flight.
    pub clutch_round_started: Option<i64>,
    /// Our computed eggs_proof (stored while awaiting peer's proof for verification)
    pub clutch_our_eggs_proof: Option<[u8; 32]>,
    /// Peer's eggs_proof if received before we computed ours
    pub clutch_their_eggs_proof: Option<[u8; 32]>,
    /// The ceremony round the stored early proof belongs to (the wire ceremony_id it arrived under). A proof may only ever be COMPARED within its own round — cross-round comparison manufactures "PROOF MISMATCH" out of ordinary offer churn (a permanent-Pending stall). Ceremony scratch like the slots: never persisted.
    pub clutch_their_proof_ceremony: Option<[u8; 32]>,
    /// Bounded retransmit budget for our ClutchComplete proof. The proof is a small single-packet UDP send with no PT retransmit, so one drop would strand the peer in AwaitingProof forever (the asymmetric-completion bug: we go Complete via early-proof, null our state, and never resend). Set to a small N when we compute our eggs proof; ping_contacts re-sends the proof and decrements each cycle until it hits 0, so both sides converge even on a lossy or just-refreshed path. Runtime-only — not persisted (a resumed Complete contact needs no further proof sends).
    pub clutch_proof_resends_left: u8,
    /// LIFETIME cap on AwaitingProof-recovery re-arms. The recovery loop tops the budget back up each cycle while the peer is online and we're still stuck — but that loop had NO ceiling, so a mismatched peer (a ghost/wrong-identity contact that answers a ceremony it can never place — the two-era ghost case) retransmitted its proof forever, every ~13s, one relay request each. This counts total re-arms; past `PROOF_RETRY_LIFETIME_CAP` the recovery loop stops topping up and `clutch_proof_gave_up` latches. Runtime-only.
    pub clutch_proof_retry_lifetime: u16,
    /// Consecutive SAME-ROUND proof-value mismatches (runtime only). Deterministic stable inputs can't produce a persistent same-round mismatch between the two real ceremony parties — a streak means a COMPETING INSTANCE (a third device muxing the shared slots computed different eggs under the same round: the 11/12 deadlock, field 2026-08-30, MacBook vs 8c4f6bee/60842217). At the cap the AwaitingProof side YIELDS its round (chain replication or the next single-owner claimed round converges); the Complete side just stops re-arming. Reset on any verified proof.
    pub clutch_mismatch_streak: u16,
    /// The responding device's build self-description off the sealed pong tail ("v0.69.1 · abc · linux x86_64") — the fleet page's per-device About. Runtime-only; None until a tailed pong lands.
    pub device_about: Option<String>,
    /// Latched once proof retransmission gave up (lifetime cap hit) — the peer answered but can never place our proof (token mismatch: a stale-identity ghost, or an interrupted re-genesis). Freezes the resend storm; the UI reads it as "can't complete — remove and re-add". Runtime-only; a fresh session re-tries once (the peer may have re-attested correctly).
    pub clutch_proof_gave_up: bool,
    /// When we last DISCARDED our round to adopt a peer's fresh-keyed mid-ceremony offer (§4.2 wholesale adoption). Rate-limits adoption: a peer that can't hear our responses (one-way reachability — live pair 2026-07-25) re-offers with new keys every ~25s, and unthrottled adoption re-ran keygen+encap on the UI thread each time (a hitch storm). While the last adoption is fresh we hold our round and ignore further re-offers; the peer only needs ONE of our responses to land. Runtime-only, never persisted.
    pub clutch_last_adoption: Option<std::time::Instant>,
    /// When this device DEFERRED a §4.2 responder claim to a lower online sibling (fan-out tie-break). The deference is time-boxed: past one round TTL with the friend's offer still waiting, the winner evidently failed and the rescue falls to us. Runtime-only, never persisted — a restart re-defers from scratch, which only costs the winner another TTL of grace.
    pub clutch_claim_deferred: Option<std::time::Instant>,
    /// Flag to prevent multiple concurrent keygens (race condition guard)
    pub clutch_keygen_in_progress: bool,
    /// Flag to prevent multiple concurrent KEM encapsulations
    pub clutch_kem_encap_in_progress: bool,
    /// Flag to serialize KEM decapsulation jobs — a queued KEM re-arrival mid-flight waits in clutch_pending_kem until the running decap drains.
    pub clutch_kem_decap_in_progress: bool,
    /// Flag to prevent multiple concurrent ceremony completions (avalanche_expand)
    pub clutch_ceremony_in_progress: bool,
    /// HQC public key prefix from peer's last completed ceremony. Used for stale detection: if received offer has same HQC prefix, it's a PT retransmission (stale), not a legitimate re-key request. Stored at completion time, cleared when accepting new ceremony.
    pub completed_their_hqc_prefix: Option<[u8; 8]>,
    /// Collected offer provenances for ceremony nonce derivation. Each offer's VSF header has hp = BLAKE3(signer_pubkey || creation_time_nanos). Sorted and combined via spaghettify to derive unique ceremony_id. Cleared when CLUTCH ceremony completes.
    pub offer_provenances: Vec<[u8; 32]>,

    pub trust_level: TrustLevel,
    /// THE CONSENT GATE (2026-08-25): false = we added them and await their reciprocal add — the ceremony must NOT arm and no key material leaves this fleet toward them. Flips true on the mutuality edge (their knock/offer token-matching our roster). ABSENT AT REST = TRUE: every pre-feature row — the whole existing fleet and friend set — is grandfathered mutual, so shipping this changes nothing for anyone already connected.
    pub consent_mutual: bool,
    /// Runtime only: a knock went out this session — the decay that keeps an unreciprocated add from pinging a stranger on every presence edge forever. Resets each launch (deliberately un-persisted): one knock per session per pending add.
    pub knocked_session: bool,
    pub added: i64,
    /// Roster LWW clock: eagle time of the last change to this contact's SYNCED identity fields (published_name / avatar_pin). The fleet roster entry carries it as `updated`; a pulled entry newer than this overwrites those fields, an older one loses. Starts equal to `added`.
    pub roster_updated: i64,
    /// The ONE fleet device running this friendship's CLUTCH (fleet-sync.md §4.2 — synced via the roster entry). The adding device claims at add time; siblings park their own rounds while the owner is present; presence-loss enables takeover. `None` = unclaimed (legacy / pre-claim) — first device to pick it up claims.
    pub ceremony_owner: Option<[u8; 32]>,
    /// Roster-adopted display truth: the OWNER's ceremony completed ("secured on <device>"). NEVER unlocks our own compose — that stays gated on OUR chain (chain_woven) until chain state travels (braid.md §14).
    pub owner_woven: bool,
    pub last_seen: Option<i64>,
    pub is_online: bool, // True when we have confirmed bidirectional comms
    /// True when the ONLY working path to this contact is the FGTW relay (no direct socket — the asymmetric-reachability case). Drives the lime-yellow presence (theme::RING_RELAY_COLOUR) instead of the direct-connection green, so a relayed link is never mistaken for a direct one. Set when a message arrives via relay / a direct path is proven unreachable; cleared the moment a direct path validates. Not persisted (a session-scoped reachability fact).
    pub reached_via_relay: bool,
    pub prev_is_online: bool, // For differential rendering (not persisted)
    pub indicator_x: usize,   // Cached indicator dot X position (set during draw)
    pub indicator_y: usize,   // Cached indicator dot Y position (set during draw)
    pub text_x: f32,          // Cached text X position (set during draw)
    pub text_y: f32,          // Cached text Y position (set during draw)
    // Avatar cache - fetched from FGTW by handle Storage key is deterministic: BLAKE3(BLAKE3(handle) || "avatar")
    pub avatar_pixels: Option<Vec<u8>>, // Full 256x256 VSF RGB pixels (cached)
    pub avatar_scaled: Option<Vec<u8>>, // Pre-scaled to current display size
    pub avatar_scaled_diameter: usize,  // Diameter the scaled pixels were rendered for

    // Chain weave probe — after CLUTCH reaches Complete, both devices auto-exchange one hidden probe chat message each way to prove the ratchet works end-to-end. Once proven, the ceremony proof rebroadcast is cancelled (clutch_proof_resends_left = 0). Runtime-only, not persisted: a resumed Complete contact already has a working chain and needs no re-probe.
    /// The chain has been validated end-to-end (our probe/message got ACKed AND we saw theirs). Gates the status line from "weaving the chain" to "secured" and stops the ceremony rebroadcast.
    pub chain_woven: bool,
    /// We've already sent (or queued) our chain-weave probe for this contact — send it once, then only on the re-arm below.
    pub probe_sent: bool,
    /// Re-probe budget. The seal needs BOTH halves proven ON THIS DEVICE (their probe received AND our own chain advanced by an ACK), but a probe is one best-effort frame: lose it and the pair deadlocks forever with each side holding a different half — observed live 2026-08-01, both of Nick's devices stuck on "testing the secure channel", one having seen the peer's probe and the other having had its ACK verified. Re-armed on the proof-arrival EDGE (never a timer), bounded so an unreachable peer doesn't re-probe endlessly.
    pub probe_resends_left: u8,
    /// We've received the peer's chain-weave probe (their TX chain / our RX proven for at least one hop).
    pub their_probe_seen: bool,
    /// WHICH ceremony that probe belonged to. A completing ceremony voids a weave seal from a PREVIOUS chain, but the peer's probe for the ceremony now completing routinely arrives a few hundred ms BEFORE our own completion finishes (observed 290ms, Jon↔7ff3835f 2026-07-27) — and a reset that discriminates on timing alone throws that one away too, so `chain_woven` never seals and the proof rebroadcasts until its budget runs out. The real question is never "when" but "which chain", and this answers it.
    pub their_probe_ceremony: Option<[u8; 32]>,
    /// Our own TX chain has advanced via a matching ACK at least once (our TX / their RX proven). Sealed with `their_probe_seen` this is the both-directions-proven condition for `chain_woven`.
    pub chain_advanced_by_ack: bool,
    /// Runtime-only: a punch-validated direct path to this contact `(remote_addr, last_confirmed)`, set when a hole-punch round-trips (see [`crate::network::traverse`]). `race_addrs` prefers it as the primary send address, keeping the public/LAN as the alternate so PT still races if the NAT mapping went stale. Each keepalive ack refreshes `last_confirmed`; once it exceeds the traversal TTL with no ack the path is cleared and re-punched. `Instant` (not eagle-time) because it's never persisted — a resumed session re-punches.
    pub validated_path: Option<(SocketAddr, std::time::Instant)>,
    /// Runtime-only, judged ONCE at the validation edge: the validated path is genuinely OUR LAN (private address on our own subnet, or a live WFD group) — not merely RFC-private. Carrier-grade NAT hands out 10.x to cellular devices, and a validated 10.x path across Verizon's CGNAT round-trips fine while being nothing like "same room" (field 2026-08-30: the cyan ring for a peer hundreds of miles away). Render reads this flag; it never re-derives from the address.
    pub validated_path_lan: bool,
    /// Runtime-only graceful-failure counter: consecutive ping cycles where an ONLINE contact was punched but never validated a direct path (the symmetric↔symmetric case). Past [`PUNCH_UNREACHABLE_THRESHOLD`] the peer is treated as direct-unreachable — the hook the relay milestone (M2) reads, and the seed-registry resolve's "this contact needs fresh addresses" signal. Reset to 0 on any validation.
    pub punch_unvalidated_cycles: u8,
    /// Runtime-only reachability clock (docs/reachability-doorbell.md): the last time ANY signed traffic from this contact's devices reached us — pong, punch ack, chat frame. "The guard's eyes are open." `None` since boot = never heard. Drives the dozed classification: silence past the dozed threshold plus undeliverable traffic = ring the doorbell.
    pub last_heard: Option<std::time::Instant>,
    /// Runtime-only: this contact has received at least one presence VERDICT this session — a pong OR a 3-consecutive-timeout — via the StatusUpdate::Online drain. Fixes the §4.2 takeover boot race: siblings start `is_online = false` before the first sweep, so "owner absent" is meaningless until probed. Never persisted.
    pub presence_probed: bool,
    /// Runtime-only: when we last rang this contact's doorbell — the client-side debounce above the worker's per-target guard. One wake per re-ring window no matter how much traffic queues behind it.
    pub last_ring: Option<std::time::Instant>,
    /// Per-contact presence backoff, doubling 1→2→4…→60 minutes while nothing is happening with THIS contact, and reset to the floor the moment something is: you open their conversation, they speak, or we send to them. Battery and data are spent per contact, so the cadence is decided per contact — a global tier spends the same on the friend you are mid-conversation with and the one you have not spoken to since March.
    pub ping_backoff: u8,
    /// When this contact was last pinged. `None` = never, so the first sweep always reaches them.
    pub last_pinged: Option<std::time::Instant>,
    /// Runtime-only fork detector: consecutive inbound chat frames from this contact that passed signature + chain-link checks but decrypted to garbage (VSF parse failure) — the signature of a chain FORK (the two sides advanced different key material). Reset on any successful decrypt. At the threshold a SIBLING contact triggers the fleet-key chain_reset repair; a friend contact only logs (friend-side repair waits for the fleet-plane linearizer).
    pub chain_fail_streak: u8,
    /// Persistent-gap fork detector: (key = XOR of the repeating expected/got prev-hash pair, repeat count). A hash-chain gap that repeats IDENTICALLY is a committed fork, not latency — the missing predecessor will never arrive, and the decrypt-failure streak never sees it because buffering isn't a failure. At threshold the repair fires (sibling reset / friend re-key). Runtime-only.
    pub gap_streak: (u64, u16),
    /// Runtime-only: the last sibling chain-reset nonce APPLIED for this contact — dedups the echo (the responder bounces the same frame back so the initiator converges) and any retransmit. Never persisted: a restart mid-repair just lets the detector re-fire with a fresh nonce.
    pub last_chain_reset_nonce: Option<[u8; 32]>,
    /// Runtime-only rate limiter: when we last INITIATED a chain reset toward this contact, so a storm of garbage frames can't spam repair rounds. One initiation per window; the detector re-fires after it if the fork persists.
    pub last_chain_reset_sent: Option<std::time::Instant>,
    /// Runtime-only rate limiter for anti-entropy full-resync kicks (eagle time of the last digest-mismatch kick) — a peer that persistently can't serve re-fires at a polite cadence instead of every pong.
    pub digest_kick_osc: i64,
    /// Runtime-only stall counter: consecutive ping cycles spent in `Pending` with our offer sent, a validated direct path up, and still no offer from the peer. The ping cycle re-fires our offer each time this crosses its threshold (then zeroes it) — the pong-driven offer re-send never triggers for a peer whose pongs don't flow, and a one-shot offer whose PT transfer died leaves the ceremony parked forever. Reset whenever the stall condition doesn't hold.
    pub clutch_offer_stall_cycles: u8,
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
        Self::from_pin([0u8; 64], handle_proof, handle_hash, Some(public_identity))
    }

    /// PIN-SET constructor: reconstruct a contact from stored/synced material (vault rows, roster entries) — no handle anywhere.
    pub fn from_pin(
        avatar_pin: [u8; 64],
        handle_proof: [u8; 32],
        party_id: [u8; 32],
        public_identity: Option<DevicePubkey>,
    ) -> Self {
        Self {
            // No device key yet → the id keys on the PARTY id, the durable identity (the same doctrine as new_sibling). The old zero-placeholder derivation collided every device-less contact onto blake3(zeros) — one id, first-match-by-id roulette.
            id: match &public_identity {
                Some(pk) => ContactId::from_pubkey(pk),
                None => ContactId::from_bytes(party_id),
            },
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
            locked_out: false,
            refused_devices: Vec::new(),
            locked_reports_seen: Vec::new(),
            identity_superseded: false,
            ip: None,
            local_ip: None,   // Discovered via LAN broadcast
            local_port: None, // Discovered via LAN broadcast
            p2p_addr: None,   // Set on Wi-Fi Direct group-up, cleared on teardown
            wfd_cred: None,   // Provisioned over the friend channel post-CLUTCH
            wfd_cred_sent: false,
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
            clutch_round_started: None,  // No ceremony round in flight yet
            clutch_our_eggs_proof: None, // Our proof (stored while awaiting peer's)
            clutch_their_eggs_proof: None, // Peer's proof (if received early)
            clutch_their_proof_ceremony: None, // The round that early proof belongs to
            clutch_proof_resends_left: 0, // Bounded proof-retransmit budget (runtime only)
            clutch_proof_retry_lifetime: 0, // Lifetime re-arm counter (runtime only)
            clutch_mismatch_streak: 0, // Same-round mismatch streak (runtime only)
            device_about: None,        // Learned from the sealed pong tail (runtime only)
            clutch_proof_gave_up: false, // Latched when the lifetime cap is hit (runtime only)
            clutch_last_adoption: None,
            clutch_claim_deferred: None,
            clutch_keygen_in_progress: false, // No keygen running yet
            clutch_kem_encap_in_progress: false, // No KEM encap running yet
            clutch_kem_decap_in_progress: false,
            clutch_ceremony_in_progress: false, // No ceremony completion running yet
            completed_their_hqc_prefix: None,   // Set when CLUTCH completes, persisted
            offer_provenances: Vec::new(),      // Collected offer provenances for ceremony nonce
            trust_level: TrustLevel::Stranger,
            // Reconstruct/materialize paths default MUTUAL (legacy grandfathering + sibling stubs, which §4.2 parking already keeps from racing the owner); the ONE local-add site flips this false explicitly.
            consent_mutual: true,
            knocked_session: false,
            added: vsf::eagle_time_oscillations(),
            roster_updated: vsf::eagle_time_oscillations(),
            ceremony_owner: None,
            owner_woven: false,
            last_seen: None,
            is_online: false,         // Starts offline until we confirm comms
            reached_via_relay: false, // Direct until proven relay-only
            prev_is_online: false,    // Match initial state
            indicator_x: 0,           // Set during first draw
            indicator_y: 0,           // Set during first draw
            text_x: 0.0,              // Set during first draw
            text_y: 0.0,              // Set during first draw
            avatar_pixels: None,      // Fetched from FGTW by handle when online
            avatar_scaled: None,      // Scaled on demand for display
            avatar_scaled_diameter: 0,
            chain_woven: false, // Chain not yet proven end-to-end (probe pending)
            probe_sent: false,
            probe_resends_left: 3,        // Chain-weave probe not sent yet
            their_probe_seen: false,      // Haven't seen their chain-weave probe yet
            their_probe_ceremony: None,   // …and so no ceremony to attribute one to
            chain_advanced_by_ack: false, // Our TX chain not yet ACK-advanced
            validated_path: None,         // No punch-validated direct path yet
            validated_path_lan: false,    // …so no same-LAN judgment either
            punch_unvalidated_cycles: 0,  // No failed punch cycles yet
            clutch_offer_stall_cycles: 0, // No stalled-offer cycles yet
            last_heard: None,             // No signed traffic from them yet this session
            presence_probed: false,       // No presence verdict yet this session
            last_ring: None,
            ping_backoff: 0,
            last_pinged: None, // Doorbell never rung this session
            chain_fail_streak: 0,
            gap_streak: (0, 0),
            last_chain_reset_nonce: None,
            last_chain_reset_sent: None,
            digest_kick_osc: 0,
            clutch_completed_at: None,    // Ceremony not yet complete
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
        let mut c = Self::from_pin([0u8; 64], our_handle_proof, party_id, Some(sibling_device));
        // The id keys on the SIBLING pid (domain-separated), never from_pin's blake3(pubkey): that derivation collides with any non-sibling row pinned to the same device — the notes row first-met on device X and the sibling row FOR device X shared one id, and the keygen drain's first-match-by-id installed the sibling's re-key onto the notes row, which then offered 573KB at our own fleet on the self-pair token forever (field, 2026-08-13).
        c.id = ContactId::from_bytes(party_id);
        c.is_sibling = true;
        c.trust_level = TrustLevel::Inner;
        c
    }

    /// The conversation this contact stands for, as a participant SET.
    ///
    /// This is the seam the self special-casing lived in. A contact row for our own identity describes a conversation whose only participant is us, and one for a friend describes a conversation with two — the difference is the size of a set, not a mode to branch on. Callers ask `remote_participants()` and get the right answer for zero, one, or any number without knowing which case they are in.
    pub fn conversation(&self, our_party_id: &crate::types::PartyId) -> crate::types::Conversation {
        crate::types::Conversation::new([*our_party_id, self.handle_hash])
    }

    /// Is this conversation reachable — i.e. can a message sent here actually land?
    ///
    /// Derived, not stored. With no remote participants the answer is unconditionally yes: there is no network hop between writing a note and having it, so there is nothing that could be offline. With remotes it is ordinary presence. This replaces forcing `is_online = true` on our own row at four settle sites, which had to be re-applied on every load, merge and attest — and still showed OFFLINE whenever one of them was missed.
    pub fn is_reachable(&self, our_party_id: &crate::types::PartyId) -> bool {
        self.remote_count(our_party_id) == 0 || self.is_online
    }

    /// Has the key exchange for this conversation finished? A ceremony over a set with no remote participants has nothing to exchange, so it is complete on arrival — the same answer the old code wrote by hand as `clutch_state = Complete`.
    pub fn is_keyed(&self, our_party_id: &crate::types::PartyId) -> bool {
        self.remote_count(our_party_id) == 0
            || self.clutch_state == crate::types::ClutchState::Complete
    }

    /// How many OTHER people a message here has to reach. `0` for our own notes — not because we checked for self, but because a set containing only us has nobody else in it.
    pub fn remote_count(&self, our_party_id: &crate::types::PartyId) -> usize {
        self.conversation(our_party_id).remote_count(our_party_id)
    }

    /// The name this contact renders as everywhere: their published profile name → the keyed two-word voca pseudonym from the party id. No handle: the string that derives an identity exists at rest nowhere (docs/identity-profile.md). Names carry ZERO trust — the pinned key does.
    pub fn display_name(&self) -> String {
        if !self.published_name.is_empty() {
            return self.published_name.clone();
        }
        crate::network::fgtw::fleet::keyed_pseudonym(&self.handle_hash)
    }

    /// True once we have a REAL name — the name they published. Until then the only "name" is the deterministic voca pseudonym, which reads like a real name (PotatoOctopus) then jarringly flips to the actual name once it arrives.
    pub fn has_real_name(&self) -> bool {
        !self.published_name.is_empty()
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
    /// Every device pubkey we know for this contact: the first-met identity device, EVERY folded fleet member, and every discovered endpoint. Used to address a RELAY forward — the relay routes by device pubkey and needs NO endpoint, so a member we've folded but never learned an address for is exactly the device that DEPENDS on the relay copy. Keying only on cached `device_endpoints` here silently dropped a folded-but-endpoint-less sibling from the fan-out, so a message landed on the peer's other devices and never on that one — it stayed stuck at the anchor forever, buffering every later frame (field, 2026-08-08: Emma's 1be949c1 never in Nick's relay list). Fold-first, per fleet-sync.md §7. Deduplicated.
    pub fn relay_device_list(&self) -> Vec<[u8; 32]> {
        let mut out: Vec<[u8; 32]> = self.device_key().into_iter().collect();
        for m in &self.fleet_members {
            if !out.contains(m) {
                out.push(*m);
            }
        }
        for ep in &self.device_endpoints {
            if !out.contains(&ep.pubkey) {
                out.push(ep.pubkey);
            }
        }
        // Never forward to a device the friend's fleet reported stolen: it may be in the fold still (refusal is not removal) but our frames must not reach a thief's hands.
        out.retain(|d| !self.refused_devices.contains(d));
        out
    }

    /// Upsert the per-device endpoint for `pubkey` and apply `update` to it. Linear scan — fleets are single-digit sized.
    pub fn endpoint_mut(&mut self, pubkey: &[u8; 32]) -> &mut DeviceEndpoint {
        if let Some(i) = self
            .device_endpoints
            .iter()
            .position(|e| e.pubkey == *pubkey)
        {
            return &mut self.device_endpoints[i];
        }
        self.device_endpoints.push(DeviceEndpoint {
            pubkey: *pubkey,
            public: None,
            lan: None,
            online: false,
        });
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

        // No proven path yet: try candidates in priority order — global IPv6 host first (no NAT, no punch), then IPv6 reflexive, then IPv4 LAN, then IPv4 reflexive (the punched WAN path). This is the v6-first send order; it replaces the old LAN-first-then-public choice, so a reachable v6 address is tried before a v4 LAN address that may belong to a foreign network (the common `192.168.0.x` collision between two default home routers). Falls back to nothing only when we know no address at all.
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
        // A SIBLING contact knows exactly its ONE device — by contract its fleet_members stays EMPTY and the fold flags never apply. Enforce that here: a stray fold that marked a sibling folded_once turned the membership check into always-false and BLACK-HOLED every fleet frame from that device (the 2026-07-26 self-sync black hole: the desktop dropping its own phone's pages as "not a fold-trusted sibling").
        if self.is_sibling {
            return self.device_key() == Some(*device_pubkey);
        }
        // The refusal outranks the fold: a refused device is still a chain member (removal is self-signed only), but its owner's fleet reported it stolen and two distinct devices vouched — so nothing it sends is honoured, which is what starves a ghost that the fold alone can never shed.
        if self.refused_devices.contains(device_pubkey) {
            return false;
        }
        if self.fleet_folded_once {
            self.fleet_members.contains(device_pubkey)
        } else {
            self.device_key() == Some(*device_pubkey) || self.fleet_members.contains(device_pubkey)
        }
    }

    /// Every device pubkey we'll answer for this contact — feeds the status checker's answerable set. Mirrors `knows_device`: post-fold it's exactly the folded members (public_identity included only if it's still one); pre-fold it's public_identity unioned with any cached members, deduped, first-met leading. A locked-out (treat-as-stolen) sibling answers NONE of its devices, and refused devices are excluded — so the RAW presence-ping gate refuses a locked/refused device's pings, not just its chain/ceremony frames (those were already gated on `knows_device` downstream; presence leaked because it only checked this flat set). Without this, a device you locked kept exchanging presence and rendering peers online.
    pub fn answerable_pubkeys(&self) -> Vec<[u8; 32]> {
        // Treat-as-stolen lockout is whole-contact: answer nothing for it.
        if self.locked_out {
            return Vec::new();
        }
        let keep = |k: &[u8; 32]| !self.refused_devices.contains(k);
        if self.fleet_folded_once {
            self.fleet_members
                .iter()
                .copied()
                .filter(|k| keep(k))
                .collect()
        } else {
            let mut v = Vec::with_capacity(self.fleet_members.len() + 1);
            if let Some(k0) = self.device_key() {
                if keep(&k0) {
                    v.push(k0);
                }
            }
            for m in &self.fleet_members {
                if self.device_key() != Some(*m) && keep(m) {
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

    /// §4.2 canonical round teardown: kill THIS device's in-flight CLUTCH round (parked loser, takeover adoption, or mid-ceremony new-round adoption). Resets every ephemeral round field so no keep-alive path can resurrect it.
    /// Deliberately does NOT touch `friendship_id` / chain state / `roster_updated` — a discard is only legal for a non-Complete round, and the Complete-rekey path clears friendship state itself. Also leaves `clutch_keygen_in_progress` alone: the serialized keygen worker owns that flag, and the result drain drops a stale result for a parked round instead.
    /// A ceremony just completed: void a weave seal that belonged to the chain it REPLACED, and keep one that belongs to the ceremony now completing.
    ///
    /// Both are real cases and only the attribution tells them apart. On a re-key the peer's old probe proved a chain that no longer exists, so the seal must die or the new chain would inherit a proof it never earned. But the peer's probe for the ceremony now completing routinely arrives BEFORE our own completion finishes — 290 ms in the observed case (Jon↔7ff3835f, 2026-07-27) — and a reset that discriminates on timing throws that one away too. The result was a ceremony that both sides had completed sitting on "weaving the chain" forever, with the proof rebroadcasting until its budget ran out.
    ///
    /// Caller resets `chain_woven`, `probe_sent` and `chain_advanced_by_ack` unconditionally; only the probe half is attribution-sensitive, because it is the only half that can arrive before we know the ceremony is done.
    pub fn void_weave_seal_from_previous_chain(&mut self) {
        if self.their_probe_ceremony != self.ceremony_id {
            self.their_probe_seen = false;
            self.their_probe_ceremony = None;
        }
    }

    /// The pinned device key bytes, if a real key was ever learned. THE way to read `public_identity` — every consumer decides explicitly what absence means for it (skip the leg, hold the send, log and move on), instead of inheriting a zero key that pings "00000000".
    pub fn device_key(&self) -> Option<[u8; 32]> {
        self.public_identity.as_ref().map(|p| p.key)
    }

    pub fn discard_clutch_round(&mut self) {
        self.clutch_state = ClutchState::Pending;
        self.completed_their_hqc_prefix = None;
        self.clutch_our_keypairs = None;
        self.clutch_round_started = None;
        self.clutch_slots.clear();
        self.ceremony_id = None;
        self.offer_provenances.clear();
        self.clutch_pending_kem = None;
        self.clutch_offer_sent = false;
        self.clutch_our_eggs_proof = None;
        self.clutch_their_eggs_proof = None;
        self.clutch_kem_encap_in_progress = false;
        self.clutch_kem_decap_in_progress = false;
        self.clutch_offer_stall_cycles = 0;
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

    /// The twelve independent KEM "eggs" a CLUTCH braids, across four security families — a compromise of any one family still leaves the shared secret protected by the others. Named here so the status line can say WHAT is being exchanged, not just "pending". 5 elliptic-curve (x25519, P-384, secp256k1, P-256, P-521) · 3 structured-lattice (NTRU-701, NTRU Prime 761, ML-KEM-1024) · 2 unstructured-lattice (Frodo-976, Frodo-1344) · 2 code-based (McEliece-460896, HQC-256).
    pub const CLUTCH_EGGS: usize = 12;

    /// Total ceremony milestones for a 2-party CLUTCH — the denominator of the status fraction. Steps are ZERO-INDEXED (0..=11), so the ladder reads 0/12 through 11/12; 12/12 is never rendered, because reaching it IS the conversation opening.
    pub const CLUTCH_STEPS: u8 = 12;

    /// Where the CLUTCH ceremony actually is, as `step/total · what's happening` — for the conversation status line and asymmetric-completion debugging. The fraction is monotonic toward `secured`; the label names what is in flight instead of a flat "pending".
    pub fn clutch_status_detail(&self) -> String {
        // Display doctrine: dozenal is the acclimation surface for VERSION + REPUTATION only; a step counter stays in current mixed arabic units.
        let n = Self::CLUTCH_STEPS;
        let eggs = Self::CLUTCH_EGGS;
        // Vocabulary doctrine (2026-07-31): describe the EXCHANGE, not our internals — no "eggs", no "braiding", no "KEMs" in user-facing text. Each step reads as part of one sentence (make keys → swap public halves → lock secrets to each other → combine → prove → confirm → secured), and every "waiting" step names whose side the ball is on, so "it's stuck on 9/12" is a diagnosis by itself. The one flourish kept: the honest facts (12 secrets, 4 crypto families) carry the personality.
        // Zero-indexed (Nick 2026-08-01): the first step is 0/12 and the last VISIBLE one is 11/12. Step 12 exists only as the moment the conversation opens, so it is never drawn.
        match self.clutch_state {
            ClutchState::Complete => {
                if self.chain_woven {
                    "secured".to_string()
                } else {
                    format!("11/{n} · testing the secure channel")
                }
            }
            ClutchState::AwaitingProof => {
                if self.clutch_their_eggs_proof.is_some() {
                    format!("10/{n} · confirming both proofs match")
                } else {
                    format!("9/{n} · sent our proof — waiting for theirs")
                }
            }
            ClutchState::Pending => {
                let their_offer = self
                    .clutch_slots
                    .iter()
                    .any(|s| s.offer.is_some() && s.handle_hash != self.handle_hash);
                let all_kem = self.all_slots_complete();
                // Walk the milestones in order; report the earliest one not yet reached.
                if self.clutch_ceremony_in_progress {
                    format!("8/{n} · combining all {eggs} shared secrets")
                } else if self.clutch_our_keypairs.is_none() {
                    // Keygen (McEliece dominates the ~1-2s).
                    format!("0/{n} · creating {eggs} key pairs (4 families of crypto)")
                } else if self.clutch_kem_encap_in_progress {
                    format!("4/{n} · locking secrets to their keys")
                } else if all_kem {
                    format!("8/{n} · combining all {eggs} shared secrets")
                } else if their_offer || self.clutch_pending_kem.is_some() {
                    format!("6/{n} · waiting for their locked secrets")
                } else if self.clutch_offer_sent {
                    format!("2/{n} · waiting for their public keys")
                } else {
                    format!("1/{n} · sending our public keys")
                }
            }
        }
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
        assert!(
            c.knows_device(&[1u8; 32]),
            "the device we met is always known"
        );
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
        assert_eq!(
            ans.len(),
            3,
            "first-met + 2 distinct siblings, deduped: {ans:?}"
        );
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
        assert!(
            !c.knows_device(&[1u8; 32]),
            "removed first-met device is no longer trusted"
        );
        assert!(c.knows_device(&[2u8; 32]), "the current member is trusted");
        assert_eq!(
            c.answerable_pubkeys(),
            vec![[2u8; 32]],
            "answerable drops the revoked device"
        );
    }

    #[test]
    fn sibling_never_arms_stays_bootstrap() {
        // A sibling contact never folds a contact-chain, so fleet_folded_once stays false and knows_device answers only the one sibling device (empty fleet_members is load-bearing for first-match routing).
        let sib = Contact::new_sibling([0x22; 32], DevicePubkey::from_bytes([5u8; 32]));
        assert!(!sib.fleet_folded_once, "siblings are never armed");
        assert!(
            sib.knows_device(&[5u8; 32]),
            "the sibling device is trusted (bootstrap)"
        );
        assert!(!sib.knows_device(&[6u8; 32]), "another device is not");
    }

    /// The peer's probe for the ceremony now completing must SURVIVE that completion. Theirs beats our own completion by a few hundred ms often enough that discarding it was a permanent stall: `chain_woven` needs both halves, so the seal never fired, the contact sat on "weaving the chain", and the proof rebroadcast until its budget ran out (Jon↔7ff3835f, 2026-07-27 — probe at 08.158, completion at 08.448, ACK at 10.292, never sealed).
    #[test]
    fn probe_for_the_completing_ceremony_survives_completion() {
        let ceremony = [0xAB; 32];
        let mut c = Contact::new_sibling([0x22; 32], DevicePubkey::from_bytes([5u8; 32]));
        c.ceremony_id = Some(ceremony);

        // Peer's probe lands FIRST, attributed to the ceremony whose chain decrypted it.
        c.their_probe_seen = true;
        c.their_probe_ceremony = Some(ceremony);

        // …then our own completion for that same ceremony runs.
        c.void_weave_seal_from_previous_chain();

        assert!(
            c.their_probe_seen,
            "a probe for THIS ceremony must survive its completion"
        );
        assert_eq!(c.their_probe_ceremony, Some(ceremony));
    }

    /// A re-key must still void the old seal: the probe proved a chain that no longer exists, and letting the new chain inherit it would mark it woven without either side proving anything.
    #[test]
    fn probe_from_the_replaced_chain_is_voided() {
        let mut c = Contact::new_sibling([0x22; 32], DevicePubkey::from_bytes([5u8; 32]));
        c.their_probe_seen = true;
        c.their_probe_ceremony = Some([0x11; 32]); // the chain being replaced
        c.ceremony_id = Some([0x99; 32]); // the fresh one

        c.void_weave_seal_from_previous_chain();

        assert!(
            !c.their_probe_seen,
            "a probe from the replaced chain must not seal the new one"
        );
        assert_eq!(c.their_probe_ceremony, None);
    }

    /// An unattributed probe (pre-fix state carried across an upgrade) is treated as stale — the conservative reading: re-probing costs one round trip, inheriting an unearned seal costs correctness.
    #[test]
    fn unattributed_probe_is_voided() {
        let mut c = Contact::new_sibling([0x22; 32], DevicePubkey::from_bytes([5u8; 32]));
        c.their_probe_seen = true;
        c.their_probe_ceremony = None;
        c.ceremony_id = Some([0x99; 32]);

        c.void_weave_seal_from_previous_chain();

        assert!(!c.their_probe_seen);
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
        assert_ne!(
            sib.handle_hash,
            crate::types::Handle::to_identity_seed("me")
        );
        // ContactId keys on the sibling pid — two siblings of one handle never collide, AND a sibling never shares an id with a non-sibling row pinned to the same device (from_pin's blake3(pubkey) id — the notes-row keygen misroute, 2026-08-13).
        assert_eq!(sib.id, ContactId::from_bytes(sib.handle_hash));
        assert_ne!(sib.id, ContactId::from_pubkey(sib.public_identity.as_ref().unwrap()));
        // knows_device answers ONLY that one device (empty fleet_members is load-bearing for first-match routing).
        assert!(sib.knows_device(&sib_device));
        assert!(!sib.knows_device(&[6u8; 32]));

        // Slot init with OUR device-derived pid: two distinct sorted slots, both findable — the exact collision the shared handle_hash caused.
        let our_pid = crate::crypto::clutch::sibling_party_id(&[6u8; 32]);
        let mut sib = sib;
        sib.init_clutch_slots(our_pid);
        assert_eq!(sib.clutch_slots.len(), 2);
        assert_ne!(
            sib.clutch_slots[0].handle_hash,
            sib.clutch_slots[1].handle_hash
        );
        assert!(sib.get_slot(&our_pid).is_some());
        assert!(sib.get_slot(&sib.handle_hash).is_some());
    }
}
