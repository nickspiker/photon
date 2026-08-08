//! Friendship types for per-conversation encryption.
//!
//! A "friendship" is a deterministic conversation identifier derived from the sorted handle hashes of all participants. This enables:
//!
//! - **Self-notes**: 1 participant (handle_hash of self)
//! - **DMs**: 2 participants
//! - **Groups**: N participants
//!
//! Each friendship has N chains (one per participant), where each person only advances their own chain on ACK.

use crate::crypto::chain::{Chain, CHAIN_SIZE};

/// Ceremony ID: deterministic CLUTCH ceremony identifier.
///
/// Derived via spaghettify from handle_hashes + sorted ping provenances:
/// 1. Fast base: `BLAKE3("PHOTON_CEREMONY_v1" || sorted_handle_hashes)`
/// 2. Nonce: Sorted ping provenances (unique per ceremony via timestamps)
/// 3. Final: `spaghettify(base || sorted_provenances...)`
///
/// Same value on all participants' devices - both parties collect all pings. Unique per ceremony due to nanosecond timestamps in ping provenances. No memory-hard step needed - timestamp entropy defeats rainbow tables.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CeremonyId(pub [u8; 32]);

impl CeremonyId {
    /// Derive ceremony ID base from participant handle hashes (fast step).
    ///
    /// This is the deterministic BLAKE3 hash that identifies the participants. Handle hashes are sorted for canonical ordering.
    pub fn derive_base(handle_hashes: &[[u8; 32]]) -> [u8; 32] {
        // Sort for canonical ordering
        let mut sorted = handle_hashes.to_vec();
        sorted.sort();

        let mut hasher = blake3::Hasher::new();
        hasher.update(b"PHOTON_CEREMONY_v\x02");
        for hash in &sorted {
            hasher.update(hash);
        }
        *hasher.finalize().as_bytes()
    }

    /// Derive full ceremony ID from handle_hashes and ping provenances.
    ///
    /// Uses spaghettify to mix:
    /// - Base: BLAKE3(domain || sorted_handle_hashes) - identifies participants
    /// - Nonce: Sorted ping provenances - unique per ceremony (timestamp entropy)
    ///
    /// Ping provenances are BLAKE3(sender_pubkey || timestamp_nanos) from each party's ping. Both parties collect all pings, sort them, and derive the same ceremony_id deterministically.
    ///
    /// No memory-hard computation needed - nanosecond timestamps provide enough entropy to defeat rainbow table attacks.
    pub fn derive(handle_hashes: &[[u8; 32]], ping_provenances: &[[u8; 32]]) -> Self {
        use ihi::spaghettify;

        let base = Self::derive_base(handle_hashes);

        // Sort provenances for canonical ordering (should already be sorted, but ensure)
        let mut sorted_provs = ping_provenances.to_vec();
        sorted_provs.sort();

        // Build input: base || sorted_provenances
        let mut input = Vec::with_capacity(32 + 32 * sorted_provs.len());
        input.extend_from_slice(&base);
        for prov in &sorted_provs {
            input.extend_from_slice(prov);
        }

        // Spaghettify: domain-separated, maximally weird mixing
        let ceremony_id = spaghettify(&input);
        Self(ceremony_id)
    }

    /// Create from raw bytes (32 bytes)
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Get the raw bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for CeremonyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CeremonyId({})", hex::encode(&self.0[..8]))
    }
}

/// Friendship ID: deterministic conversation identifier.
///
/// Derived as `BLAKE3("PHOTON_FRIENDSHIP_v1" || sorted_handle_hashes)` Same value on all participants' devices.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FriendshipId(pub [u8; 32]);

impl FriendshipId {
    /// Derive friendship ID from participant handle hashes.
    ///
    /// Handle hashes are sorted for canonical ordering - the same participants will always produce the same friendship ID regardless of order.
    pub fn derive(handle_hashes: &[[u8; 32]]) -> Self {
        // Sort for canonical ordering
        let mut sorted = handle_hashes.to_vec();
        sorted.sort();

        let mut hasher = blake3::Hasher::new();
        hasher.update(b"PHOTON_FRIENDSHIP_v\x02");
        for hash in &sorted {
            hasher.update(hash);
        }
        Self(*hasher.finalize().as_bytes())
    }

    /// Create from raw bytes (32 bytes)
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Get the raw bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Get base64url encoding for filesystem paths
    pub fn to_base64(&self) -> String {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        URL_SAFE_NO_PAD.encode(self.0)
    }

    /// Parse from base64url string
    pub fn from_base64(s: &str) -> Option<Self> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        let bytes = URL_SAFE_NO_PAD.decode(s).ok()?;
        if bytes.len() != 32 {
            return None;
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Some(Self(arr))
    }
}

impl std::fmt::Debug for FriendshipId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FriendshipId({})", hex::encode(&self.0[..8]))
    }
}

impl std::fmt::Display for FriendshipId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_base64())
    }
}

// Domain separation for hash chain pointers
const DOMAIN_MSG_HP: &[u8] = b"PHOTON_MSG_HP_v\x02";
const DOMAIN_ANCHOR: &[u8] = b"PHOTON_ANCHOR_v\x02";

/// Reliability backoff for unacked outgoing messages. A message is (re)sent until an ACK arrives or we hit `MAX_SEND_ATTEMPTS`; between sends we wait `retry_delay_osc(attempts)` — exponential from ~1s, doubling, capped at ~30s. Covers both a dropped message AND a dropped ACK (the sender just keeps resending; the receiver dedupes by eagle_time and its ACK is deterministic, so a re-ACK is free). These live on `PendingMessage` and are runtime-only (not persisted).
const RETRY_BASE_SECS: u64 = 1;
const RETRY_CAP_SECS: u64 = 30;
const MAX_SEND_ATTEMPTS: u8 = 8;

/// How many un-ACKed messages a lane may have in flight at once (advance-on-send makes pipelining safe; each frame encrypts at its own position). Held at 4 while pre-hardening receivers may still be in the field: their now-removed gap-streak fork trigger fired at 8 buffered frames, so a burst must stay well under that. Lift once the fleet has updated.
pub const IN_FLIGHT_WINDOW: usize = 4;

/// Backoff delay (in eagle-time oscillations) before the `attempts`-th send's resend: 1s, 2s, 4s, 8s, 16s, then capped at 30s. `attempts` is 1-based (1 = after the first transmit).
fn retry_delay_osc(attempts: u8) -> i64 {
    let shift = attempts.saturating_sub(1).min(6); // cap the shift so 1<<shift can't overflow
    let secs = (RETRY_BASE_SECS << shift).min(RETRY_CAP_SECS);
    (secs * vsf::OSCILLATIONS_PER_SECOND) as i64
}

/// Per-participant encryption chains for a friendship.
///
/// Each participant has their own chain (16KB). When sending, use sender's chain. When receiving ACK, advance sender's chain. This prevents race conditions in simultaneous sends and scales to N-party conversations.
///
/// ## Hash Chain Protocol
///
/// Every message includes `prev_msg_hp` - a hash pointer to the previous message. This creates a cryptographic chain that:
/// - Provides message ordering (can detect missing/out-of-order)
/// - Enables resync (request messages after known hash)
/// - Prevents replay (each message uniquely identified)
///
/// The hash chain is separate from encryption chain advancement:
/// - **Hash chain**: Links messages for ordering/integrity
/// - **Encryption chain**: Advances on ACK for forward secrecy
#[derive(Clone)]
pub struct FriendshipChains {
    /// Friendship ID (derived from sorted handle_hashes)
    pub friendship_id: FriendshipId,

    /// Privacy-preserving conversation token for wire format. Derived via smear_hash(sorted_participant_seeds) - only participants can compute. Replaces cleartext handle_hashes in messages.
    pub conversation_token: [u8; 32],

    /// One chain per LANE (docs/lanes.md) — a lane is one device's ratchet, derived from `lane_root` ‖ its wire label, device identity nowhere in the derivation. Index matches `lane_labels`.
    chains: Vec<Chain>,

    /// Participant IDENTITY pids (sorted) — conversation membership only: the token, the friendship id, and contact resolution key on these. Chain state does NOT: that moved to lanes, because an identity is many devices and a ratchet with many writers forks.
    participants: Vec<[u8; 32]>,

    /// Lane labels, parallel to every per-lane vec below. 32 random bytes minted by the SENDING device at its first send and carried on every frame — anyone holding `lane_root` derives the lane from the label alone (receive-anywhere), and nothing pubkey-derived ever rides the wire (pseudonymity). Arrival-ordered, never sorted: labels have no canonical order and need none.
    lane_labels: Vec<[u8; 32]>,

    /// Advance count per lane — the checkpoint ordering key: a replicated copy of a lane is adopted iff its position is strictly greater (a fast-forward of a deterministic replay, always safe). Index matches `lane_labels`.
    lane_positions: Vec<u64>,

    /// The label OUR device minted — the ONE lane this device may advance (writer discipline: every other lane is receive-only here, which is what makes forks impossible instead of healed). `None` until our first send mints it.
    our_label: Option<[u8; 32]>,

    /// Last plaintext per lane (for salt derivation). Index matches `lane_labels`. Empty Vec = first message on that lane. Used to derive salt: `derive_salt(prev_plaintext, chain)`
    last_plaintexts: Vec<Vec<u8>>,

    /// Pending sent messages awaiting ACK (for our chain only). When we send, we store plaintext here. On ACK, we advance and clear. Vec because we can send multiple messages before receiving ACKs.
    pub pending_messages: Vec<PendingMessage>,

    /// Last received message time per participant (for duplicate detection). Index matches chain index. None = no message received yet from that sender. If incoming message has eagle_time <= this value, it's a duplicate (skip).
    last_received_times: Vec<Option<i64>>,

    // ==================== HASH CHAIN STATE ====================
    /// First message anchor per participant (deterministic starting point). Derived from: BLAKE3(DOMAIN_ANCHOR || participant_handle_hash || chain_fingerprint) where chain_fingerprint = BLAKE3(chain[256..512]). Both parties compute identical anchors from CLUTCH ceremony.
    first_message_anchors: Vec<[u8; 32]>,

    /// Last received message hash per participant (for hash chain verification). Index matches chain index. None = no message received yet → expect anchor. On receive: verify prev_msg_hp == this value (or anchor if None). After successful decrypt: update to msg_hp of received message.
    last_received_hashes: Vec<Option<[u8; 32]>>,

    /// Last sent message hash (for our chain only). Used as prev_msg_hp in next outgoing message. None = first message → use our anchor. Updated after each send (before ACK - hash chain is independent).
    last_sent_hash: Option<[u8; 32]>,

    // ==================== BIDIRECTIONAL ENTROPY STATE ====================
    /// Last received weave hash (for bidirectional entropy mixing). Derived from: hash(DOMAIN || eagle_time || msg_hp || plaintext) This prevents brute-forcing even if plaintext is guessable. When we send, we mix this into our chain advancement. Updated after each successful decrypt.
    last_received_weave: Option<[u8; 32]>,

    /// Last sent weave hash (what we sent = what they received). When receiver advances their view of our chain, they use this to match what we used for mixing when we received their ACK. Updated after each send.
    last_sent_weave: Option<[u8; 32]>,

    /// Hash pointer of the message whose weave we last incorporated. Included in outgoing messages as `their_incorporated_hp`. Acts as implicit ACK - tells peer we received up to this message.
    last_incorporated_hp: Option<[u8; 32]>,

    /// Buffer for out-of-order messages (gap handling). When we receive a message with prev_msg_hp that doesn't match our last_received_hash, we store it here until the gap is filled.
    gap_buffer: Vec<BufferedMessage>,

    /// Friend-history bulk key: seals history-recovery pages between the participants, OUTSIDE the ratchet. Derived once at ceremony birth (`from_clutch`) via spaghettify over the pristine active chains — identical on both sides exactly then, divergent after any advance. `None` for chains loaded from pre-feature vaults (recovery unavailable until their next re-key, which is the recovery scenario anyway). Persisted with the chains; zeroized on supersede.
    history_key: Option<[u8; 32]>,

    /// The LANE ROOT (docs/lanes.md): the 32-byte secret every per-device lane derives from — lane = expand(root ‖ wire label), device identity nowhere in the derivation. Born beside `history_key` from the same pristine-chains moment; the lanes themselves materialize on demand once the lane wire lands. Persisted (schema v8); zeroized on supersede, same custody as the chain links beside it.
    lane_root: Option<[u8; 32]>,
    /// When this blob's ceremony COMPLETED (eagle time) — the ERA stamp. Two blobs over one friendship with different lane_roots are different ceremonies, and this decides which era supersedes in `merge_lanes_from` (newer wins wholesale). 0 on blobs persisted before the field existed, so any freshly-woven era beats a legacy one.
    pub genesis_osc: i64,

    /// Eagle-time of the last LOCAL mutation of this chain state (send prepare, ACK advance, receive advance, plaintext update). The fleet chain-replication ordering key: a sibling's pushed copy is adopted iff its stamp is NEWER than ours — "if another device is ahead, I just catch up". Persisted (schema v7); 0 for pre-feature vaults (any replicated copy beats an unstamped one).
    pub mutated_osc: i64,
}

/// A message buffered due to a gap in the hash chain (out-of-order delivery). Held until its predecessor arrives and the gap fills. Buffered BEFORE decrypt, so the message's own `msg_hp` is not yet known (it needs the plaintext hash); we key purely on the `prev_msg_hp` it awaits. When a successful decrypt advances `last_received_hash` to some `H`, every buffered entry with `prev_msg_hp == H` becomes contiguous and is reprocessed (which can cascade).
#[derive(Clone)]
pub struct BufferedMessage {
    /// The predecessor hash this message is waiting on (its on-wire `prev_msg_hp`).
    pub prev_msg_hp: [u8; 32],
    /// Sender's handle hash.
    pub sender_handle_hash: [u8; 32],
    /// Eagle time of the message (oscillations).
    pub eagle_time: i64,
    /// Encrypted ciphertext (decrypted when the gap fills).
    pub ciphertext: Vec<u8>,
    /// Sender address, so the reprocess path can ACK exactly as the live path would.
    pub sender_addr: std::net::SocketAddr,
    /// The signing device pubkey, so a replayed frame re-enters the receive arm carrying the same identity the known∧not-refused gate needs.
    pub sender_pubkey: [u8; 32],
}

/// A sent message awaiting ACK confirmation.
///
/// Stored in pending_messages until ACKed. Contains everything needed to:
/// 1. Match incoming ACK (eagle_time + plaintext_hash)
/// 2. Advance chain on ACK (plaintext_hash)
/// 3. Resend if no ACK (ciphertext + prev_msg_hp)
/// 4. Derive next message's salt (plaintext)
///
/// After ACK: removed from pending, chain advances, forward secrecy kicks in. Without ACK: can be resent from ciphertext (still have encrypted form).
#[derive(Clone)]
pub struct PendingMessage {
    /// Eagle time oscillations of this message (for ACK matching and nonce)
    pub eagle_time: i64,
    /// Plaintext content (needed for salt derivation of next message)
    pub plaintext: Vec<u8>,
    /// BLAKE3 hash of plaintext (for ACK verification and chain advancement)
    pub plaintext_hash: [u8; 32],
    /// Hash pointer to previous message (for hash chain continuity)
    pub prev_msg_hp: [u8; 32],
    /// This message's hash pointer (becomes prev for next message)
    pub msg_hp: [u8; 32],
    /// Encrypted ciphertext (for resend without re-encryption)
    pub ciphertext: Vec<u8>,
    /// The braid's woven peer strands frozen at send time — the EXACT plaintext bytes of the (up to two) prior peer messages this message braided in, already sorted by eagle_time. Frozen so `process_ack` advances our chain with the identical strands the receiver used to advance its copy (the receiver resolves them from the two eagle_times on the wire). Length 0 = anchor (wove nothing), 1 = single strand (early conversation), 2 = full braid.
    pub woven_strands: Vec<Vec<u8>>,
    /// Reliability (runtime-only, NOT persisted): how many times we've (re)sent this message. The first transmit counts as attempt 1. Used to drive exponential backoff and to give up after a ceiling so an undeliverable message surfaces instead of resending forever.
    pub attempts: u8,
    /// Reliability (runtime-only, NOT persisted): the eagle-time oscillation at which this message is next eligible for resend. The tick-driven retransmit sweep resends any unacked pending whose `next_retry_osc` has passed, then pushes this out by the next backoff step. Set on first send.
    pub next_retry_osc: i64,
}

// ============================================================================ Hash Chain Derivation Functions ============================================================================

/// Derive first message anchor for a participant's hash chain.
///
/// Anchor = BLAKE3(DOMAIN_ANCHOR || handle_hash || chain_fingerprint) where chain_fingerprint = BLAKE3(active_chain_bytes).
///
/// Both parties compute identical anchors from CLUTCH ceremony output.
fn derive_anchor(handle_hash: &[u8; 32], chain: &Chain) -> [u8; 32] {
    // Chain fingerprint: hash of active portion (links[256..512])
    let active_bytes = chain.to_bytes();
    let active_portion = &active_bytes[CHAIN_SIZE / 2..]; // 8KB active links
    let chain_fingerprint = blake3::hash(active_portion);

    let mut hasher = blake3::Hasher::new();
    hasher.update(DOMAIN_ANCHOR);
    hasher.update(handle_hash);
    hasher.update(chain_fingerprint.as_bytes());
    *hasher.finalize().as_bytes()
}

/// Derive message hash pointer (provenance hash) for hash chain.
///
/// msg_hp = BLAKE3(DOMAIN_MSG_HP || prev_msg_hp || plaintext_hash || eagle_time_bytes)
///
/// This creates a cryptographic chain where each message's identity depends on:
/// - The entire history (via prev_msg_hp)
/// - The content (plaintext_hash)
/// - The timestamp (eagle_time)
pub fn derive_msg_hp(
    prev_msg_hp: &[u8; 32],
    plaintext_hash: &[u8; 32],
    eagle_time: i64,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DOMAIN_MSG_HP);
    hasher.update(prev_msg_hp);
    hasher.update(plaintext_hash);
    hasher.update(&eagle_time.to_le_bytes());
    *hasher.finalize().as_bytes()
}

/// Derive weave hash for bidirectional entropy mixing.
///
/// The weave incorporates the full message context (timestamp, msg_hp, plaintext) into a 32-byte hash. This prevents brute-forcing even if the plaintext is guessable ("ok", "yes", etc.) because the exact timestamp acts as a nonce.
///
/// Domain: PHOTON_WEAVE_v0
pub fn derive_weave_hash(eagle_time: i64, msg_hp: &[u8; 32], plaintext: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"PHOTON_WEAVE_v\x01");
    hasher.update(&eagle_time.to_le_bytes());
    hasher.update(msg_hp);
    hasher.update(plaintext);
    *hasher.finalize().as_bytes()
}

impl FriendshipChains {
    /// Initialize chains from CLUTCH shared secrets.
    ///
    /// Defense-in-depth key derivation without keyspace compression:
    ///
    /// 1. **Avalanche expansion**: 640B eggs → 2MB memory-hard mixed buffer
    /// 2. **Domain separation**: Each participant gets unique XOF-expanded state
    /// 3. **Truncate-and-append**: 256 rounds of smear_hash, accumulating links
    /// 4. **Algorithm diversity**: smear_hash uses BLAKE3 ⊕ SHA3 ⊕ SHA512
    ///
    /// Security survives if ANY layer remains unbroken:
    /// - 20 eggs from 8 algorithms (4 classical + 4 post-quantum)
    /// - Memory-hard 2MB intermediate state
    /// - Three hash algorithms in parallel (smear_hash)
    /// - No compression: full entropy preserved thru derivation
    pub fn from_clutch(participants: &[[u8; 32]], eggs: &[[u8; 32]]) -> Self {
        use crate::crypto::clutch::{
            avalanche_expand_eggs, derive_chain_from_avalanche, derive_conversation_token,
            ClutchEggs,
        };

        // Sort participants for canonical ordering
        let mut sorted_participants = participants.to_vec();
        sorted_participants.sort();

        // Derive friendship ID
        let friendship_id = FriendshipId::derive(&sorted_participants);

        // Derive conversation token (privacy-preserving wire identifier)
        let conversation_token = derive_conversation_token(&sorted_participants);

        // Step 1: Expand eggs to 2MB (memory-hard, no compression)
        let eggs_struct = ClutchEggs {
            eggs: eggs.to_vec(),
        };
        let avalanche = avalanche_expand_eggs(&eggs_struct);

        // Step 2: derive the per-participant ACTIVE material — NOT to keep (lanes replaced participant chains), but because the history key and lane root were always derived from these pristine bytes and every v8 blob in the field carries roots born this way. The material lives exactly long enough to seed the two keys, then scrubs.
        let mut active_snapshots: Vec<Vec<u8>> = Vec::with_capacity(sorted_participants.len());
        for participant in &sorted_participants {
            active_snapshots.push(derive_chain_from_avalanche(&avalanche, participant));
        }

        // Friend-history bulk key — derived HERE, at ceremony birth, from the pristine active chains (the one moment both sides are byte-identical). Every completion path flows thru from_clutch, so this is the single derivation site. See crypto::clutch::derive_history_key.
        let (history_key, lane_root) = {
            let refs: Vec<&[u8]> = active_snapshots.iter().map(|v| v.as_slice()).collect();
            (
                crate::crypto::clutch::derive_history_key(friendship_id.as_bytes(), &refs),
                // The lane root shares the birth moment and the input discipline — distinct domain, so the two keys are unrelated (docs/lanes.md).
                crate::crypto::clutch::derive_lane_root(friendship_id.as_bytes(), &refs),
            )
        };
        // The snapshots duplicate live chain secret material — scrub them.
        for snap in active_snapshots.iter_mut() {
            use zeroize::Zeroize;
            snap.zeroize();
        }

        // Lanes are born EMPTY: each device mints its own at first send, and every receive materializes the sender's from root ‖ label. A ceremony creates the shared root, never the ratchets (docs/lanes.md).
        Self {
            friendship_id,
            conversation_token,
            chains: Vec::new(),
            participants: sorted_participants,
            lane_labels: Vec::new(),
            lane_positions: Vec::new(),
            our_label: None,
            last_plaintexts: Vec::new(),
            pending_messages: Vec::new(),
            last_received_times: Vec::new(),
            first_message_anchors: Vec::new(),
            last_received_hashes: Vec::new(),
            last_sent_hash: None,
            // Bidirectional entropy state (initialized empty)
            last_received_weave: None,
            last_sent_weave: None,
            last_incorporated_hp: None,
            gap_buffer: Vec::new(),
            history_key: Some(history_key),
            lane_root: Some(lane_root),
            genesis_osc: vsf::eagle_time_oscillations(),
            mutated_osc: 0,
        }
    }

    /// Create from serialized data (for loading from storage).
    pub fn from_storage_v3(
        friendship_id: FriendshipId,
        participants: Vec<[u8; 32]>,
        chain_bytes: &[u8],
        last_sent_hash: Option<[u8; 32]>,
        mut last_received_hashes: Vec<Option<[u8; 32]>>,
        pending_messages: Vec<PendingMessage>,
        last_received_weave: Option<[u8; 32]>,
        last_sent_weave: Option<[u8; 32]>,
        last_incorporated_hp: Option<[u8; 32]>,
    ) -> Option<Self> {
        use crate::crypto::clutch::derive_conversation_token;

        let chain_count = participants.len();
        if chain_bytes.len() != CHAIN_SIZE * chain_count {
            return None;
        }

        let mut chains = Vec::with_capacity(chain_count);
        for i in 0..chain_count {
            let start = i * CHAIN_SIZE;
            let end = start + CHAIN_SIZE;
            let chain = Chain::from_full_bytes(&chain_bytes[start..end])?;
            chains.push(chain);
        }

        // Derive conversation token from participants
        let conversation_token = derive_conversation_token(&participants);

        // Initialize last_plaintexts with empty vecs (will be populated on first message)
        let last_plaintexts = vec![Vec::new(); participants.len()];

        // Initialize last_received_times with None (will be populated on first message)
        let last_received_times = vec![None; participants.len()];

        // Derive first_message_anchors for each participant's hash chain These are deterministic from chain state, so we recompute them
        let first_message_anchors: Vec<[u8; 32]> = participants
            .iter()
            .zip(chains.iter())
            .map(|(handle_hash, chain)| derive_anchor(handle_hash, chain))
            .collect();

        // Use provided last_received_hashes, or initialize to None if empty
        if last_received_hashes.is_empty() {
            last_received_hashes = vec![None; participants.len()];
        }

        Some(Self {
            friendship_id,
            conversation_token,
            chains,
            participants,
            last_plaintexts,
            pending_messages,
            last_received_times,
            first_message_anchors,
            last_received_hashes,
            last_sent_hash,
            // Bidirectional entropy state from storage
            last_received_weave,
            last_sent_weave,
            last_incorporated_hp,
            lane_labels: Vec::new(),  // pre-lane blob: lanes are absent by definition (the loader installs v8 lanes separately)
            lane_positions: Vec::new(),
            our_label: None,
            gap_buffer: Vec::new(), // Gap buffer is transient, not persisted
            history_key: None,      // pre-v6 file: no history key (set by the loader when present)
            lane_root: None,        // loader installs it from a v8 file
            genesis_osc: 0,
            mutated_osc: 0,
        })
    }

    /// Create from serialized data (for loading from storage) - v4 with last_plaintexts.
    pub fn from_storage_v4(
        friendship_id: FriendshipId,
        participants: Vec<[u8; 32]>,
        chain_bytes: &[u8],
        last_sent_hash: Option<[u8; 32]>,
        last_received_hashes: Vec<Option<[u8; 32]>>,
        pending_messages: Vec<PendingMessage>,
        last_received_weave: Option<[u8; 32]>,
        last_sent_weave: Option<[u8; 32]>,
        last_incorporated_hp: Option<[u8; 32]>,
        last_plaintexts: Vec<Vec<u8>>,
    ) -> Option<Self> {
        // Delegate to v5 with empty last_received_times (will be initialized)
        Self::from_storage_v5(
            friendship_id,
            participants,
            chain_bytes,
            last_sent_hash,
            last_received_hashes,
            pending_messages,
            last_received_weave,
            last_sent_weave,
            last_incorporated_hp,
            last_plaintexts,
            Vec::new(), // No persisted times in v4
        )
    }

    pub fn from_storage_v5(
        friendship_id: FriendshipId,
        participants: Vec<[u8; 32]>,
        _legacy_chain_bytes: &[u8],
        last_sent_hash: Option<[u8; 32]>,
        _legacy_last_received_hashes: Vec<Option<[u8; 32]>>,
        pending_messages: Vec<PendingMessage>,
        last_received_weave: Option<[u8; 32]>,
        last_sent_weave: Option<[u8; 32]>,
        last_incorporated_hp: Option<[u8; 32]>,
        _legacy_last_plaintexts: Vec<Vec<u8>>,
        _legacy_last_received_times: Vec<Option<i64>>,
    ) -> Option<Self> {
        use crate::crypto::clutch::derive_conversation_token;

        // The lanes flag-day retired per-participant chain state: the `_legacy_*` args are accepted (old blobs still carry them) and DISCARDED — a lane is derived from root ‖ label, never resurrected from participant bytes. Lanes install separately (`install_lanes`) when the blob carries them; a legacy blob yields laneless chains and the loader drops its pendings (they were built for the retired wire).
        let conversation_token = derive_conversation_token(&participants);

        Some(Self {
            friendship_id,
            conversation_token,
            chains: Vec::new(),
            participants,
            last_plaintexts: Vec::new(),
            pending_messages,
            last_received_times: Vec::new(),
            first_message_anchors: Vec::new(),
            last_received_hashes: Vec::new(),
            last_sent_hash,
            // Bidirectional entropy state from storage
            last_received_weave,
            last_sent_weave,
            last_incorporated_hp,
            lane_labels: Vec::new(),
            lane_positions: Vec::new(),
            our_label: None,
            gap_buffer: Vec::new(), // Gap buffer is transient, not persisted
            history_key: None,      // pre-v6 file default: loader sets it when the field is present
            lane_root: None,        // loader installs it from a v8 file
            genesis_osc: 0,
            mutated_osc: 0,
        })
    }

    /// The friend-history bulk key (None = pre-feature chains; recovery unavailable until re-key).
    pub fn history_key(&self) -> Option<&[u8; 32]> {
        self.history_key.as_ref()
    }

    /// Install the history key (storage loader, after a v6 file carried one).
    pub fn set_history_key(&mut self, key: Option<[u8; 32]>) {
        self.history_key = key;
    }

    /// Scrub the history key (supersede on re-key / delete): zeroize then drop.
    pub fn zeroize_history_key(&mut self) {
        use zeroize::Zeroize;
        if let Some(k) = self.history_key.as_mut() {
            k.zeroize();
        }
        self.history_key = None;
    }

    /// The lane root (docs/lanes.md) — the secret every per-device lane derives from.
    pub fn lane_root(&self) -> Option<&[u8; 32]> {
        self.lane_root.as_ref()
    }

    /// Install the lane root (storage loader, from a v8 file).
    pub fn set_lane_root(&mut self, root: Option<[u8; 32]>) {
        self.lane_root = root;
    }

    /// Scrub the lane root (supersede on re-key / delete) — every lane grown from it dies with it.
    pub fn zeroize_lane_root(&mut self) {
        use zeroize::Zeroize;
        if let Some(k) = self.lane_root.as_mut() {
            k.zeroize();
        }
        self.lane_root = None;
    }

    /// Serialize all chains to bytes (for storage).
    pub fn chains_to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(CHAIN_SIZE * self.chains.len());
        for chain in &self.chains {
            bytes.extend(chain.to_bytes());
        }
        bytes
    }

    /// Get the friendship ID.
    pub fn id(&self) -> &FriendshipId {
        &self.friendship_id
    }

    /// Get participant handle_hashes (sorted).
    pub fn participants(&self) -> &[[u8; 32]] {
        &self.participants
    }

    /// Find chain index for a participant.
    fn participant_index(&self, handle_hash: &[u8; 32]) -> Option<usize> {
        self.participants.binary_search(handle_hash).ok()
    }

    /// Get current encryption key for a LANE (by its wire label).
    pub fn current_key(&self, lane_label: &[u8; 32]) -> Option<&[u8; 32]> {
        let idx = self.lane_index(lane_label)?;
        Some(self.chains[idx].current_key())
    }

    /// Advance a participant's chain after ACK.
    ///
    /// Call this when we receive confirmation that a message was decrypted.
    ///
    /// Advance a participant's chain, braiding in `their_plaintexts` (the woven peer strands — two for a full braid, or fewer early in the conversation; the caller passes them sorted by eagle_time so both peers frame identically).
    pub fn advance(
        &mut self,
        lane_label: &[u8; 32],
        eagle_time: &vsf::EagleTime,
        our_plaintext: &[u8],
        their_plaintexts: &[&[u8]],
    ) -> bool {
        // Fleet chain-replication ordering key: every local mutation stamps NOW (see mutated_osc).
        self.mutated_osc = vsf::eagle_time_oscillations();
        if let Some(idx) = self.lane_index(lane_label) {
            self.chains[idx].advance(eagle_time, our_plaintext, their_plaintexts);
            // The checkpoint ordering key: strictly-greater position = a fast-forward of this same deterministic replay.
            self.lane_positions[idx] += 1;
            true
        } else {
            false
        }
    }

    /// Get a lane's chain by label (debugging/inspection/serialization).
    pub fn chain(&self, lane_label: &[u8; 32]) -> Option<&Chain> {
        let idx = self.lane_index(lane_label)?;
        Some(&self.chains[idx])
    }

    /// Number of participants in this friendship.
    pub fn participant_count(&self) -> usize {
        self.participants.len()
    }

    /// Total size in bytes (N × 16KB).
    pub fn total_size(&self) -> usize {
        CHAIN_SIZE * self.chains.len()
    }

    /// Get last plaintext for a lane (for salt derivation). Returns empty slice for the first message on that lane.
    pub fn last_plaintext(&self, lane_label: &[u8; 32]) -> &[u8] {
        if let Some(idx) = self.lane_index(lane_label) {
            &self.last_plaintexts[idx]
        } else {
            &[]
        }
    }

    /// Get the "other" participant in a 2-party conversation. Returns None for self-notes (1-party) or group chats (3+ party).
    pub fn other_participant(&self, our_handle_hash: &[u8; 32]) -> Option<&[u8; 32]> {
        if self.participants.len() != 2 {
            return None;
        }
        if &self.participants[0] == our_handle_hash {
            Some(&self.participants[1])
        } else if &self.participants[1] == our_handle_hash {
            Some(&self.participants[0])
        } else {
            None // We're not in this conversation
        }
    }

    /// Update last plaintext for a participant's chain after successful decrypt/send.
    pub fn set_last_plaintext(&mut self, lane_label: &[u8; 32], plaintext: Vec<u8>) {
        // Fleet chain-replication ordering key: every local mutation stamps NOW (see mutated_osc).
        self.mutated_osc = vsf::eagle_time_oscillations();
        if let Some(idx) = self.lane_index(lane_label) {
            self.last_plaintexts[idx] = plaintext;
        }
    }

    /// Check if a message is a duplicate (already received from this sender). Returns true if this is a duplicate and should be skipped.
    pub fn is_duplicate(&self, lane_label: &[u8; 32], eagle_time: i64) -> bool {
        if let Some(idx) = self.lane_index(lane_label) {
            if let Some(last_time) = self.last_received_times[idx] {
                // Duplicate if eagle_time <= last received (exact match or older)
                return eagle_time <= last_time;
            }
        }
        false
    }

    /// Mark a message as received (update last received time for deduplication).
    pub fn mark_received(&mut self, lane_label: &[u8; 32], eagle_time: i64) {
        if let Some(idx) = self.lane_index(lane_label) {
            // Tip-consistency guard: this is the conversation's high-water mark (the contiguous tip that becomes `last_received_osc`). It must only ever move FORWARD — a buffered / out-of-order ("ahead") message must never reach here (it's gated behind verify_chain_link and only processed in order, so its eagle_time is always strictly newer than the prior tip). If this ever fires, a non-contiguous message inflated the high-water mark, which would falsely tell the peer "I have everything up to here" and suppress a needed resend.
            #[cfg(feature = "development")]
            if let Some(prev) = self.last_received_times[idx] {
                debug_assert!(
                    eagle_time > prev,
                    "mark_received went backward/non-monotonic: prev={} new={} — a buffered/out-of-order message inflated the contiguous tip",
                    prev,
                    eagle_time
                );
            }
            self.last_received_times[idx] = Some(eagle_time);
        }
    }

    /// Index of a lane by its wire label (arrival order, linear scan — fleets are single-digit).
    fn lane_index(&self, label: &[u8; 32]) -> Option<usize> {
        self.lane_labels.iter().position(|l| l == label)
    }

    /// Materialize the lane a label names, deriving it from `lane_root` if it doesn't exist yet — the receive-anywhere primitive: any device holding the root can build any lane from its label alone. `None` only when the blob predates lanes (no root) — the flag-day re-clutch case.
    pub fn ensure_lane(&mut self, label: &[u8; 32]) -> Option<usize> {
        if let Some(i) = self.lane_index(label) {
            return Some(i);
        }
        let root = self.lane_root?;
        let active = crate::crypto::clutch::derive_lane_active(&root, label);
        let mut full_chain = vec![0u8; CHAIN_SIZE];
        full_chain[CHAIN_SIZE / 2..].copy_from_slice(&active);
        let chain = Chain::from_full_bytes(&full_chain).expect("lane chain is 16KB");
        // The anchor binds the LABEL (not a device id) to the chain fingerprint — same construction as ever, identity-free like everything lane-shaped.
        let anchor = derive_anchor(label, &chain);
        self.lane_labels.push(*label);
        self.lane_positions.push(0);
        self.chains.push(chain);
        self.last_plaintexts.push(Vec::new());
        self.first_message_anchors.push(anchor);
        self.last_received_hashes.push(None);
        self.last_received_times.push(None);
        Some(self.lane_labels.len() - 1)
    }

    /// OUR lane's label, minting it on first use — the one lane this device may ever advance. `None` only pre-lanes (no root).
    pub fn mint_our_lane(&mut self) -> Option<[u8; 32]> {
        if let Some(l) = self.our_label {
            return Some(l);
        }
        let label: [u8; 32] = rand::random();
        self.ensure_lane(&label)?;
        self.our_label = Some(label);
        // A fresh lane's hash chain starts at ITS anchor — the send tip is blob-level state, and a value carried over from the pre-lane era (or a retired lane) poisons the first frame with a prev nobody can match: the receiver expects the anchor, forever (live-pair wedge, 2026-08-03).
        self.last_sent_hash = None;
        self.mutated_osc = vsf::eagle_time_oscillations();
        Some(label)
    }

    /// The label our device minted, if any.
    pub fn our_label(&self) -> Option<&[u8; 32]> {
        self.our_label.as_ref()
    }

    /// Install lane state loaded from storage (loader only) — parallel vecs, index-aligned.
    #[allow(clippy::too_many_arguments)]
    pub fn install_lanes(
        &mut self,
        labels: Vec<[u8; 32]>,
        positions: Vec<u64>,
        chains: Vec<Chain>,
        last_plaintexts: Vec<Vec<u8>>,
        last_received_hashes: Vec<Option<[u8; 32]>>,
        last_received_times: Vec<Option<i64>>,
        our_label: Option<[u8; 32]>,
    ) {
        self.first_message_anchors = labels
            .iter()
            .zip(chains.iter())
            .map(|(label, chain)| derive_anchor(label, chain))
            .collect();
        self.lane_labels = labels;
        self.lane_positions = positions;
        self.chains = chains;
        self.last_plaintexts = last_plaintexts;
        self.last_received_hashes = last_received_hashes;
        self.last_received_times = last_received_times;
        self.our_label = our_label;
    }

    /// Strip everything DEVICE-LOCAL from a replicated copy before adopting it wholesale: the sender's minted label, its pendings, its send tip. Adopting those would make this device WRITE on the sender's lane — the exact two-writer fork lanes exist to end.
    pub fn sanitize_replicated(&mut self) {
        self.our_label = None;
        self.pending_messages.clear();
        self.last_sent_hash = None;
    }

    /// True when the two blobs grew from DIFFERENT lane roots — a re-key minted a new era. Era-divergent blobs must never lane-merge: the merge adopts a root only where one is absent, so it would strand the old-era holder on dead chains while stacking new-era labels it derives garbage for.
    pub fn differs_in_era_from(&self, other: &FriendshipChains) -> bool {
        self.lane_root.is_some() && other.lane_root.is_some() && self.lane_root != other.lane_root
    }

    /// True when `other` is a DIFFERENT era that provably superseded ours — the caller replaces this blob wholesale (sanitized). The ceremony's GENESIS stamp decides, not `mutated_osc`: the dead era's clock does not actually go quiet — retransmit and gap bookkeeping keep bumping it, so the stale sibling could out-tick a freshly-woven era indefinitely (live pair, 2026-08-05). Genesis is written once at completion and never moves. Legacy blobs (genesis 0 on both) fall back to the old clock so two pre-stamp eras still converge somewhere.
    pub fn era_superseded_by(&self, other: &FriendshipChains) -> bool {
        if !self.differs_in_era_from(other) {
            return false;
        }
        if self.genesis_osc != other.genesis_osc {
            return other.genesis_osc > self.genesis_osc;
        }
        other.mutated_osc > self.mutated_osc
    }

    /// Merge a sibling's replicated copy, LANE-WISE (docs/lanes.md checkpoints): a lane we lack is taken whole; a lane we hold is replaced iff the incoming position is STRICTLY greater — a fast-forward of the same deterministic replay, always safe. Device-local state (our label, pendings, send tip, weave view) stays OURS untouched; the root and history key adopt only where we lack them. Replaces whole-blob newest-wins, whose fork window was both devices overwriting each other's live lanes. Returns whether anything changed. SAME-ERA ONLY: the caller must judge `differs_in_era_from` first — a re-keyed root never merges, it supersedes wholesale.
    pub fn merge_lanes_from(&mut self, other: &FriendshipChains) -> bool {
        // ERA SUPERSEDE: different lane_roots are different CEREMONIES over one friendship — a re-key happened, and lane-merging across eras is meaningless (labels derive under different roots). Keeping an existing root forever left a parked sibling on yesterday's era pushing stale frames the friend's gap repair read as a fork — it discarded a freshly-woven chain 15 seconds after completion (live pair, 2026-08-05). The newer genesis adopts WHOLESALE and the superseded era's lanes, pendings, and send state die with it; the older side keeps ours and converges when our push reaches it.
        if self.differs_in_era_from(other) {
            if !self.era_superseded_by(other) {
                return false;
            }
            self.zeroize_history_key();
            self.zeroize_lane_root();
            self.lane_root = other.lane_root;
            self.history_key = other.history_key;
            self.genesis_osc = other.genesis_osc;
            self.lane_labels = other.lane_labels.clone();
            self.lane_positions = other.lane_positions.clone();
            self.chains = other.chains.clone();
            self.last_plaintexts = other.last_plaintexts.clone();
            self.first_message_anchors = other.first_message_anchors.clone();
            self.last_received_hashes = other.last_received_hashes.clone();
            self.last_received_times = other.last_received_times.clone();
            self.our_label = None;
            self.pending_messages.clear();
            self.last_sent_hash = None;
            self.gap_buffer.clear();
            self.mutated_osc = vsf::eagle_time_oscillations();
            return true;
        }
        let mut changed = false;
        if self.lane_root.is_none() && other.lane_root.is_some() {
            self.lane_root = other.lane_root;
            changed = true;
        }
        if self.history_key.is_none() && other.history_key.is_some() {
            self.history_key = other.history_key;
            changed = true;
        }
        for (i, label) in other.lane_labels.iter().enumerate() {
            match self.lane_index(label) {
                None => {
                    self.lane_labels.push(*label);
                    self.lane_positions.push(other.lane_positions[i]);
                    self.chains.push(other.chains[i].clone());
                    self.last_plaintexts.push(other.last_plaintexts[i].clone());
                    self.first_message_anchors.push(other.first_message_anchors[i]);
                    self.last_received_hashes.push(other.last_received_hashes[i]);
                    self.last_received_times.push(other.last_received_times[i]);
                    changed = true;
                }
                Some(mine) => {
                    // Never fast-forward the lane WE write: our pendings reference our chain position, and a sibling's copy of our lane is at best a mirror of our past.
                    if Some(*label) == self.our_label {
                        continue;
                    }
                    if other.lane_positions[i] > self.lane_positions[mine] {
                        self.lane_positions[mine] = other.lane_positions[i];
                        self.chains[mine] = other.chains[i].clone();
                        self.last_plaintexts[mine] = other.last_plaintexts[i].clone();
                        self.last_received_hashes[mine] = other.last_received_hashes[i];
                        self.last_received_times[mine] = other.last_received_times[i];
                        changed = true;
                    }
                }
            }
        }
        if changed {
            self.mutated_osc = vsf::eagle_time_oscillations();
        }
        changed
    }

    /// A lane's advance position (the checkpoint ordering key).
    pub fn lane_position(&self, label: &[u8; 32]) -> Option<u64> {
        self.lane_index(label).map(|i| self.lane_positions[i])
    }

    /// Every lane as (label, position) — the checkpoint summary a merge compares.
    pub fn lane_summary(&self) -> Vec<([u8; 32], u64)> {
        self.lane_labels
            .iter()
            .copied()
            .zip(self.lane_positions.iter().copied())
            .collect()
    }

    /// A replication SUBSET: a copy carrying only the lanes named in `labels`, with device-local state (pendings, gap buffer, send tip, weave view) stripped. This is what a per-lane checkpoint push serializes — a sibling's `merge_lanes_from` adopts these lanes and leaves the rest, so we transmit ONLY the lane(s) that advanced instead of the whole chains blob (docs/lanes.md checkpoints; the whole-blob push it replaces re-sent every lane on every mutation — one friendship at 5 device-lanes was an 85KB frame decrypted on the render thread every tick). Root, history key, genesis, participants and token ride so the receiver's era check and token match still work. Never carries pendings: those reference OUR chain position and a sibling never advances our lane (writer discipline), so they are pure waste on the wire.
    pub fn replication_subset(&self, labels: &[[u8; 32]]) -> FriendshipChains {
        let keep: Vec<usize> = self
            .lane_labels
            .iter()
            .enumerate()
            .filter(|(_, l)| labels.contains(l))
            .map(|(i, _)| i)
            .collect();
        let pick_hashes = |v: &Vec<Option<[u8; 32]>>| keep.iter().map(|&i| v[i]).collect();
        FriendshipChains {
            friendship_id: self.friendship_id,
            participants: self.participants.clone(),
            conversation_token: self.conversation_token,
            lane_labels: keep.iter().map(|&i| self.lane_labels[i]).collect(),
            lane_positions: keep.iter().map(|&i| self.lane_positions[i]).collect(),
            chains: keep.iter().map(|&i| self.chains[i].clone()).collect(),
            last_plaintexts: keep.iter().map(|&i| self.last_plaintexts[i].clone()).collect(),
            last_received_times: keep.iter().map(|&i| self.last_received_times[i]).collect(),
            first_message_anchors: keep.iter().map(|&i| self.first_message_anchors[i]).collect(),
            last_received_hashes: pick_hashes(&self.last_received_hashes),
            // Device-local — never replicated, never adopted by a sibling.
            our_label: None,
            pending_messages: Vec::new(),
            gap_buffer: Vec::new(),
            last_sent_hash: None,
            last_received_weave: None,
            last_sent_weave: None,
            last_incorporated_hp: None,
            // Era + custody: the receiver needs these for differs_in_era_from and to adopt a root it lacks.
            history_key: self.history_key,
            lane_root: self.lane_root,
            genesis_osc: self.genesis_osc,
            mutated_osc: self.mutated_osc,
        }
    }

    // ==================== HASH CHAIN METHODS ====================

    /// Get the first message anchor for a lane. Used as prev_msg_hp for the first message on it.
    pub fn get_anchor(&self, lane_label: &[u8; 32]) -> Option<&[u8; 32]> {
        let idx = self.lane_index(lane_label)?;
        Some(&self.first_message_anchors[idx])
    }

    /// Get prev_msg_hp for the next outgoing message. Returns last_sent_hash if we've sent messages, otherwise OUR lane's anchor.
    pub fn get_prev_msg_hp(&self) -> Option<[u8; 32]> {
        if let Some(hash) = self.last_sent_hash {
            Some(hash)
        } else {
            // First message - use our lane's anchor
            self.get_anchor(&self.our_label?).copied()
        }
    }

    /// Get the expected prev_msg_hp for incoming message from a sender. Returns their last_received_hash, or their anchor if first message.
    pub fn get_expected_prev_hp(&self, lane_label: &[u8; 32]) -> Option<[u8; 32]> {
        let idx = self.lane_index(lane_label)?;
        if let Some(hash) = self.last_received_hashes[idx] {
            Some(hash)
        } else {
            // First message from them - expect their anchor
            Some(self.first_message_anchors[idx])
        }
    }

    /// Verify hash chain link: check if received prev_msg_hp matches expected.
    ///
    /// Returns Ok(()) if chain is valid, Err with expected hash if mismatch. Caller can use the expected hash to request resync.
    pub fn verify_chain_link(
        &self,
        lane_label: &[u8; 32],
        received_prev_msg_hp: &[u8; 32],
    ) -> Result<(), [u8; 32]> {
        let expected = self
            .get_expected_prev_hp(lane_label)
            .ok_or([0u8; 32])?;

        if received_prev_msg_hp == &expected {
            Ok(())
        } else {
            Err(expected)
        }
    }

    /// Update hash chain state after successfully receiving and decrypting a message. Call this AFTER verify_chain_link succeeds and decrypt succeeds.
    pub fn update_received_hash(&mut self, lane_label: &[u8; 32], msg_hp: [u8; 32]) {
        // Fleet chain-replication ordering key: every local mutation stamps NOW (see mutated_osc).
        self.mutated_osc = vsf::eagle_time_oscillations();
        if let Some(idx) = self.lane_index(lane_label) {
            self.last_received_hashes[idx] = Some(msg_hp);
        }
    }

    /// Reliability sweep: collect every unacked pending message whose backoff deadline has passed, bump its attempt count + next deadline, and return the data needed to resend it. Drives the tick-based retransmit so a dropped message OR a dropped ACK self-heals (we keep resending until the ACK lands; the receiver dedupes by eagle_time). Messages that have exhausted `MAX_SEND_ATTEMPTS` are NOT returned here (the caller treats them as undelivered) but are left in pending so a late ACK can still clear them.
    ///
    /// Returns `(eagle_time, prev_msg_hp, ciphertext, attempts_now, exhausted)` per due message.
    pub fn collect_due_retransmits(
        &mut self,
        now_osc: i64,
    ) -> Vec<(i64, [u8; 32], Vec<u8>, u8, bool)> {
        let mut due = Vec::new();
        for msg in self.pending_messages.iter_mut() {
            if msg.attempts >= MAX_SEND_ATTEMPTS {
                continue; // exhausted — don't resend, but keep pending for a possible late ACK
            }
            if now_osc < msg.next_retry_osc {
                continue; // not due yet
            }
            msg.attempts += 1;
            let exhausted = msg.attempts >= MAX_SEND_ATTEMPTS;
            msg.next_retry_osc = now_osc + retry_delay_osc(msg.attempts);
            due.push((
                msg.eagle_time,
                msg.prev_msg_hp,
                msg.ciphertext.clone(),
                msg.attempts,
                exhausted,
            ));
        }
        due
    }

    /// Re-arm (reset the retransmit backoff for) pending messages NEWER than the peer's contiguous tip `tip_osc` that have already EXHAUSTED `MAX_SEND_ATTEMPTS`. Drives stall recovery: a receiver stalled on a gap keeps advertising its contiguous tip (its `last_received_osc`) in every ping's sync record; if the gap-filling message was one the sender already gave up on, this revives it so `collect_due_retransmits` will send it again. Without this, a message lost past 8 attempts is permanently undelivered and the receiver stays stuck forever. Non-exhausted pendings are left alone (their normal backoff already covers them). Returns how many were re-armed.
    ///
    /// `tip_osc` is the peer's newest CONTIGUOUS eagle_time ("I have everything up to here, in order"), so anything with `eagle_time > tip_osc` is fair game to resend — it's either the missing message or a successor the peer is buffering behind it.
    pub fn rearm_pending_after(&mut self, tip_osc: i64, now_osc: i64) -> usize {
        let mut rearmed = 0;
        for msg in self.pending_messages.iter_mut() {
            if msg.eagle_time > tip_osc && msg.attempts >= MAX_SEND_ATTEMPTS {
                msg.attempts = 0;
                msg.next_retry_osc = now_osc; // due immediately
                rearmed += 1;
            }
        }
        rearmed
    }

    /// Get last_sent_hash (for debugging/logging).
    pub fn last_sent_hash(&self) -> Option<&[u8; 32]> {
        self.last_sent_hash.as_ref()
    }

    /// Get all last_plaintexts (for serialization).
    pub fn last_plaintexts(&self) -> &Vec<Vec<u8>> {
        &self.last_plaintexts
    }

    /// Get all last_received_times (for serialization).
    pub fn last_received_times(&self) -> &Vec<Option<i64>> {
        &self.last_received_times
    }

    /// Per-lane contiguous heads: (lane_label, last_received_osc) for every lane we've received a frame on. This is what a peer needs to know EXACTLY which of its lanes we're missing — a single max-across-lanes tip over-reports for a multi-device sender (a fast lane's tip hides a slow lane's gap), suppressing the slow lane's resends. Lanes with no receipt yet are omitted (their absence tells the peer "send from the anchor").
    pub fn lane_heads(&self) -> Vec<([u8; 32], i64)> {
        self.lane_labels
            .iter()
            .zip(self.last_received_times.iter())
            .filter_map(|(label, tip)| tip.map(|t| (*label, t)))
            .collect()
    }

    /// Get last_received_hash for a sender (for debugging/logging).
    pub fn last_received_hash(&self, sender_handle_hash: &[u8; 32]) -> Option<&[u8; 32]> {
        let idx = self.participant_index(sender_handle_hash)?;
        self.last_received_hashes[idx].as_ref()
    }

    /// Get all last_received_hashes (for persistence).
    pub fn last_received_hashes(&self) -> &[Option<[u8; 32]>] {
        &self.last_received_hashes
    }

    /// Get pending messages (for persistence).
    pub fn pending_messages(&self) -> &[PendingMessage] {
        &self.pending_messages
    }

    /// Add a pending message (after sending, before ACK).
    ///
    /// Stores everything needed for:
    /// - ACK matching (eagle_time + plaintext_hash)
    /// - Chain advancement on ACK (plaintext_hash)
    /// - Resend capability (ciphertext + prev_msg_hp)
    /// - Next message derivation (plaintext for salt, msg_hp for prev)
    pub fn add_pending(
        &mut self,
        eagle_time: i64,
        plaintext: Vec<u8>,
        plaintext_hash: [u8; 32],
        prev_msg_hp: [u8; 32],
        msg_hp: [u8; 32],
        ciphertext: Vec<u8>,
        woven_strands: Vec<Vec<u8>>,
    ) {
        self.pending_messages.push(PendingMessage {
            eagle_time,
            plaintext,
            plaintext_hash,
            prev_msg_hp,
            msg_hp,
            ciphertext,
            // Freeze the braid's woven strands for THIS step so the matching process_ack advances with the exact bytes the receiver used, regardless of later receives.
            woven_strands,
            // First transmit counts as attempt 1; schedule the first resend one backoff step out.
            attempts: 1,
            next_retry_osc: eagle_time + retry_delay_osc(1),
        });

        // Update last_sent_hash for next message's prev_msg_hp
        self.last_sent_hash = Some(msg_hp);
    }

    /// Encrypt a fresh outgoing message on OUR chain and record it pending.
    ///
    /// The exact inverse of the receive path: derive the salt from our previous plaintext, generate the scratch pad, encrypt with our current chain key. `plaintext` is the already-VSF-encoded message body (the `(message: x{text}, hp{incorporated_hp}, hR{pad})` field the receiver parses) — the caller builds it so this layer stays agnostic to message shape.
    ///
    /// ADVANCES OUR LANE ON SEND. The ciphertext is frozen at the current position, then the lane ratchets forward immediately so the next message encrypts at the next position (pipelining, up to the caller's in-flight window). This is crypto-identical to the old ACK-time advance — same eagle_time, same salt-text as `our_plaintext`, same frozen strands the receiver resolves from the wire — only earlier; the receiver still advances on decrypt, so the two copies stay in lockstep. It also makes a mid-flight restart safe: the advance persists with the chain, so a reloaded pending (whose `woven_strands` aren't persisted) never re-advances and cannot fork.
    ///
    /// Returns `(ciphertext, prev_msg_hp, msg_hp, plaintext_hash)` for the wire send, or `None` if `our_handle_hash` isn't a participant. `plaintext` is the FULL flattened VSF payload (`(message: x{}, hp{}, hR{pad})`) — this is what goes on the wire (encrypted) and what both sides hash for `msg_hp`/ACK. `salt_text` is the bare message x-text only: the salt source + the `our_plaintext` fed to the braid's `derive_fresh_link` on ACK-advance. The two are SEPARATE on purpose — the random `hR` pad and the public `hp` are traffic-analysis/wire concerns, never chain-key material, and keeping them out of the chain ingredient keeps it valid UTF-8 (so it stores losslessly) and matches the receiver, which advances + salts from the decrypted x-text only.
    /// Returns `(ciphertext, prev_msg_hp, msg_hp, plaintext_hash, lane_label)` — the label rides every frame so any holder of the lane root can derive the decrypting lane (docs/lanes.md). Mints OUR lane on first send; writer discipline is structural: this is the only method that ever advances it.
    pub fn prepare_send(
        &mut self,
        plaintext: Vec<u8>,
        salt_text: Vec<u8>,
        eagle_time: i64,
        woven_strands: Vec<Vec<u8>>,
    ) -> Option<(Vec<u8>, [u8; 32], [u8; 32], [u8; 32], [u8; 32])> {
        // Fleet chain-replication ordering key: every local mutation stamps NOW (see mutated_osc).
        self.mutated_osc = vsf::eagle_time_oscillations();
        use crate::crypto::chain::{derive_salt, encrypt_layers, generate_scratch};

        let our_label = self.mint_our_lane()?;
        let our_idx = self.lane_index(&our_label)?;
        let our_chain = self.chains[our_idx].clone();

        // Salt from our previous plaintext (empty on the first message) — both sides derive the same salt for the same chain position.
        let salt = derive_salt(&self.last_plaintexts[our_idx], &our_chain);
        let scratch = generate_scratch(&our_chain, &salt);
        let et = vsf::EagleTime::from_oscillations(eagle_time);
        let ciphertext = encrypt_layers(&plaintext, &our_chain, &scratch, &et);

        // Mirror the receiver's "CHAIN DECRYPT" line so both sides can be diffed: for a given eagle_time the encrypt key+salt here MUST equal the decrypt key+salt on the peer, or the chains have diverged. last_plaintext_len flags the lossy-storage class of bug (a non-empty prev that round-tripped thru storage must be byte-identical on both ends).
        crate::logf!("CHAIN ENCRYPT: lane = {}..., key = {}..., salt = {}..., eagle_time = {}, last_plaintext_len = {}, ciphertext_len = {}", hex::encode(&our_label[..4]), hex::encode(&our_chain.current_key()[..4]), hex::encode(&salt[..4]), eagle_time, self.last_plaintexts[our_idx].len(), ciphertext.len());

        // First message uses our anchor as prev_msg_hp (matches get_expected_prev_hp on the receiver).
        let prev_msg_hp = self
            .last_sent_hash
            .unwrap_or(self.first_message_anchors[our_idx]);
        // Hash + msg_hp are over the FULL payload (the receiver hashes the full decrypted bytes too).
        let plaintext_hash = *blake3::hash(&plaintext).as_bytes();
        let msg_hp = derive_msg_hp(&prev_msg_hp, &plaintext_hash, eagle_time);

        // ADVANCE ON SEND: ratchet our lane forward now, with the SAME arguments the old ACK-path used (this message's eagle_time, its salt-text as our_plaintext, the frozen strands) and set last_plaintext to the salt-text for the next message's salt. Done via borrows BEFORE add_pending moves salt_text/woven_strands.
        {
            let strand_refs: Vec<&[u8]> = woven_strands.iter().map(|s| s.as_slice()).collect();
            self.advance(&our_label, &et, &salt_text, &strand_refs);
        }
        if let Some(idx) = self.lane_index(&our_label) {
            self.last_plaintexts[idx] = salt_text.clone();
        }

        // Pending stores the SALT-TEXT + frozen strands for retransmit and ACK matching. The ACK is now a pure delivery receipt (the chain already moved); the strands stay frozen only so a retransmit resends byte-identically.
        self.add_pending(
            eagle_time,
            salt_text,
            plaintext_hash,
            prev_msg_hp,
            msg_hp,
            ciphertext.clone(),
            woven_strands,
        );

        Some((ciphertext, prev_msg_hp, msg_hp, plaintext_hash, our_label))
    }

    /// Process ACK: match the pending by (eagle_time, plaintext_hash), remove it, report the match. Under advance-on-send the chain already ratcheted forward when this message was encrypted, so the ACK is a pure delivery RECEIPT — it MUST NOT advance again (that would double-ratchet past the receiver). The match edge is what the caller hangs delivery, CLUTCH-ephemeral zeroize, and chain-seal on. No mutated_osc stamp: removing a pending is device-local (siblings never adopt our pendings) and the send already pushed the advanced lane, so an ACK needs no fleet replication.
    pub fn process_ack(
        &mut self,
        acked_eagle_time: i64,
        acked_plaintext_hash: &[u8; 32],
    ) -> bool {
        // Match on eagle_time ALONE. A device physically cannot emit two messages at the same 704ps tick (braid.md §1.5), so eagle_time uniquely names our outgoing message. The plaintext_hash was a hard co-gate, but it is taken over the FULL payload including the random hR pad, so any re-encryption of the same message yields a different hash — a hash mismatch then leaked the pending and the message retransmitted forever while the peer re-ACKed each copy (field, 2026-08-08: Mary re-ACKing one message every ~2s, Nick never advancing). The ACK is already Ed25519-authenticated over (token, eagle_time, hash); keep the hash as a logged soft-check, never a match gate.
        if let Some(idx) = self
            .pending_messages
            .iter()
            .position(|m| m.eagle_time == acked_eagle_time)
        {
            if self.pending_messages[idx].plaintext_hash != *acked_plaintext_hash {
                crate::logf!("CHAT: ACK hash mismatch for eagle_time {} — clearing on eagle_time (the pad differs or a re-encrypt raced); the message is delivered either way", acked_eagle_time);
            }
            self.pending_messages.remove(idx);
            return true;
        }
        false
    }

    /// Get the most recent pending plaintext (for salt derivation of next send). If no pending messages, returns last_plaintext for our chain.
    pub fn current_send_plaintext(&self) -> &[u8] {
        // If we have pending messages, use the last one's plaintext
        if let Some(last_pending) = self.pending_messages.last() {
            &last_pending.plaintext
        } else if let Some(l) = self.our_label {
            // Otherwise use the last acked plaintext from our lane
            self.last_plaintext(&l)
        } else {
            &[]
        }
    }

    // ==================== BIDIRECTIONAL ENTROPY METHODS ====================

    /// Get the last received weave hash (for bidirectional entropy mixing). Returns None if no messages received yet.
    pub fn last_received_weave(&self) -> Option<&[u8; 32]> {
        self.last_received_weave.as_ref()
    }

    /// Get the hash pointer of the message we last incorporated. Include this in outgoing messages as `their_incorporated_hp`.
    pub fn last_incorporated_hp(&self) -> Option<&[u8; 32]> {
        self.last_incorporated_hp.as_ref()
    }

    /// Update bidirectional entropy state after successful decrypt. Call this AFTER verify_chain_link succeeds and decrypt succeeds.
    ///
    /// Derives a weave hash from the full message context (timestamp, msg_hp, plaintext). This prevents brute-forcing even if plaintext is guessable.
    pub fn update_received_for_mixing(
        &mut self,
        eagle_time: i64,
        msg_hp: [u8; 32],
        plaintext: &[u8],
    ) {
        // Fleet chain-replication ordering key: every local mutation stamps NOW (see mutated_osc).
        self.mutated_osc = vsf::eagle_time_oscillations();
        let weave = derive_weave_hash(eagle_time, &msg_hp, plaintext);
        self.last_received_weave = Some(weave);
        self.last_incorporated_hp = Some(msg_hp);
        let _ = plaintext; // braid strands now come from the message DB at send time, not this snapshot
    }

    /// Get the last sent weave hash (what we sent = what they received). Used by receiver to advance their view of our chain with matching entropy.
    pub fn last_sent_weave(&self) -> Option<&[u8; 32]> {
        self.last_sent_weave.as_ref()
    }

    /// Update sent weave after sending a message. Call this after add_pending() to track what weave the receiver will use.
    ///
    /// Derives a weave hash from the full message context (timestamp, msg_hp, plaintext).
    pub fn update_sent_for_mixing(&mut self, eagle_time: i64, msg_hp: [u8; 32], plaintext: &[u8]) {
        let weave = derive_weave_hash(eagle_time, &msg_hp, plaintext);
        self.last_sent_weave = Some(weave);
    }

    /// Look up a pending message's plaintext by its msg_hp. Used by receiver to get the plaintext for bidirectional weave.
    pub fn get_pending_plaintext_by_hp(&self, msg_hp: &[u8; 32]) -> Option<&[u8]> {
        self.pending_messages
            .iter()
            .find(|m| &m.msg_hp == msg_hp)
            .map(|m| m.plaintext.as_slice())
    }

    // ==================== GAP BUFFER METHODS ====================

    /// Buffer a message received out of order (its `prev_msg_hp` doesn't match what we've received so far). Keyed on the awaited `prev_msg_hp`; deduped on (sender, eagle_time) since `msg_hp` is unknown pre-decrypt.
    pub fn buffer_for_gap(
        &mut self,
        prev_msg_hp: [u8; 32],
        sender_handle_hash: [u8; 32],
        eagle_time: i64,
        ciphertext: Vec<u8>,
        sender_addr: std::net::SocketAddr,
        sender_pubkey: [u8; 32],
    ) {
        // Don't buffer duplicates (same sender + same 704ps tick = the same message).
        if self
            .gap_buffer
            .iter()
            .any(|b| b.sender_handle_hash == sender_handle_hash && b.eagle_time == eagle_time)
        {
            return;
        }

        self.gap_buffer.push(BufferedMessage {
            prev_msg_hp,
            sender_handle_hash,
            eagle_time,
            ciphertext,
            sender_addr,
            sender_pubkey,
        });
        // Bounded: an unfillable gap (forged frames, a peer that re-keyed away) must not grow RAM forever — evict oldest, the retransmit path re-serves anything real that gets dropped.
        const GAP_BUFFER_CAP: usize = 256;
        if self.gap_buffer.len() > GAP_BUFFER_CAP {
            self.gap_buffer.remove(0);
        }
    }

    /// Check if we have buffered messages waiting for a specific prev_msg_hp. Returns the buffered messages that can now be processed.
    pub fn take_buffered_for(&mut self, filled_msg_hp: &[u8; 32]) -> Vec<BufferedMessage> {
        let mut ready = Vec::new();
        let mut remaining = Vec::new();

        for buffered in self.gap_buffer.drain(..) {
            if &buffered.prev_msg_hp == filled_msg_hp {
                ready.push(buffered);
            } else {
                remaining.push(buffered);
            }
        }

        self.gap_buffer = remaining;
        ready
    }

    /// Get count of buffered messages (for debugging/logging).
    pub fn gap_buffer_count(&self) -> usize {
        self.gap_buffer.len()
    }

    /// Clear all buffered messages (e.g., on resync).
    pub fn clear_gap_buffer(&mut self) {
        self.gap_buffer.clear();
    }
}

impl std::fmt::Debug for FriendshipChains {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "FriendshipChains {{ id: {:?}, {} participants, {} bytes }}",
            self.friendship_id,
            self.participants.len(),
            self.total_size()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_friendship_id_derive() {
        let alice = [1u8; 32];
        let bob = [2u8; 32];

        // Same result regardless of order
        let id1 = FriendshipId::derive(&[alice, bob]);
        let id2 = FriendshipId::derive(&[bob, alice]);
        assert_eq!(id1.0, id2.0);

        // Different participants = different ID
        let charlie = [3u8; 32];
        let id3 = FriendshipId::derive(&[alice, charlie]);
        assert_ne!(id1.0, id3.0);
    }

    #[test]
    fn test_friendship_id_self_notes() {
        // Self-notes: just your own handle_hash
        let me = [42u8; 32];
        let id = FriendshipId::derive(&[me]);

        // Should be consistent
        let id2 = FriendshipId::derive(&[me]);
        assert_eq!(id.0, id2.0);
    }

    #[test]
    fn test_friendship_id_base64_roundtrip() {
        let id = FriendshipId::derive(&[[1u8; 32], [2u8; 32]]);
        let encoded = id.to_base64();
        let decoded = FriendshipId::from_base64(&encoded).unwrap();
        assert_eq!(id.0, decoded.0);
    }

    #[test]
    fn test_friendship_chains_from_clutch() {
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();

        let mut chains = FriendshipChains::from_clutch(&[alice, bob], &eggs);

        // Lanes are born EMPTY — a ceremony creates the shared root, never the ratchets.
        assert_eq!(chains.participant_count(), 2);
        assert_eq!(chains.total_size(), 0);
        assert!(chains.lane_root().is_some());

        // Participants should be sorted
        let participants = chains.participants();
        assert!(participants[0] < participants[1]);

        // A label materializes a lane from the root; an unknown label has no key until ensured.
        let label = [7u8; 32];
        assert!(chains.current_key(&label).is_none());
        assert!(chains.ensure_lane(&label).is_some());
        assert!(chains.current_key(&label).is_some());
        assert_eq!(chains.lane_position(&label), Some(0));

        // Our own lane mints once and is stable.
        let ours = chains.mint_our_lane().unwrap();
        assert_eq!(chains.mint_our_lane().unwrap(), ours);
        assert!(chains.current_key(&ours).is_some());
    }

    #[test]
    fn sibling_pids_key_distinct_friendships_and_chains() {
        // Fleet weave: sibling ceremonies key the braid on device-derived party ids instead of the (shared) handle_hash. The chain machinery is opaque to WHAT the 32 bytes are — prove a 3-device fleet yields 3 distinct friendship ids, and that pid-keyed chains resolve both participants and advance exactly like handle-keyed ones.
        let pids: Vec<[u8; 32]> = [[1u8; 32], [2u8; 32], [3u8; 32]]
            .iter()
            .map(|d| crate::crypto::clutch::sibling_party_id(d))
            .collect();

        let mut fids = Vec::new();
        for i in 0..pids.len() {
            for j in (i + 1)..pids.len() {
                let f_ab = FriendshipId::derive(&[pids[i], pids[j]]);
                let f_ba = FriendshipId::derive(&[pids[j], pids[i]]);
                assert_eq!(f_ab.0, f_ba.0, "friendship id must be order-independent");
                fids.push(f_ab.0);
            }
        }
        fids.sort_unstable();
        fids.dedup();
        assert_eq!(
            fids.len(),
            3,
            "each sibling pair must get a distinct friendship id"
        );

        // pid-keyed IDENTITY layer: other_participant round-trips; the ratchets are lane-keyed and identity-free.
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let mut chains = FriendshipChains::from_clutch(&[pids[0], pids[1]], &eggs);
        assert_eq!(chains.other_participant(&pids[0]), Some(&pids[1]));
        let (a, b) = ([0xA0u8; 32], [0xB0u8; 32]);
        chains.ensure_lane(&a).unwrap();
        chains.ensure_lane(&b).unwrap();
        let key_b_before = *chains.current_key(&b).unwrap();
        let eagle_time = vsf::EagleTime::from_oscillations(vsf::eagle_time_oscillations());
        assert!(chains.advance(&a, &eagle_time, &[0xAA; 32], &[]));
        assert_eq!(*chains.current_key(&b).unwrap(), key_b_before);
    }

    #[test]
    fn test_friendship_chains_advance() {
        use vsf::EagleTime;

        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();

        let mut chains = FriendshipChains::from_clutch(&[alice, bob], &eggs);
        let (lane_a, lane_b) = ([0x11u8; 32], [0x22u8; 32]);
        chains.ensure_lane(&lane_a).unwrap();
        chains.ensure_lane(&lane_b).unwrap();

        // Save original keys
        let a_key_before = *chains.current_key(&lane_a).unwrap();
        let b_key_before = *chains.current_key(&lane_b).unwrap();

        // Advance one lane (no bidirectional entropy for this test)
        let eagle_time = vsf::EagleTime::from_oscillations(vsf::eagle_time_oscillations());
        let plaintext_hash = [0xAA; 32];
        assert!(chains.advance(&lane_a, &eagle_time, &plaintext_hash, &[]));

        // Its key changes AND its position moves — the checkpoint ordering key.
        assert_ne!(a_key_before, *chains.current_key(&lane_a).unwrap());
        assert_eq!(chains.lane_position(&lane_a), Some(1));

        // The other lane is untouched: one writer per lane, no cross-talk.
        assert_eq!(b_key_before, *chains.current_key(&lane_b).unwrap());
        assert_eq!(chains.lane_position(&lane_b), Some(0));
    }

    #[test]
    fn test_friendship_chains_storage_roundtrip() {
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();

        // Lane-wise sibling merge (the checkpoint rule): greater position fast-forwards, our own lane never adopts, device-local state never crosses.
        let mut ours = FriendshipChains::from_clutch(&[alice, bob], &eggs);
        let mut theirs = FriendshipChains::from_clutch(&[alice, bob], &eggs);
        let our_lane = ours.mint_our_lane().unwrap();
        let their_lane = theirs.mint_our_lane().unwrap();
        let et = vsf::EagleTime::from_oscillations(vsf::eagle_time_oscillations());
        // The sibling advanced its own lane twice; we have never seen that lane.
        theirs.advance(&their_lane, &et, &[1u8; 8], &[]);
        theirs.advance(&their_lane, &et, &[2u8; 8], &[]);
        assert!(ours.merge_lanes_from(&theirs));
        assert_eq!(ours.lane_position(&their_lane), Some(2));
        // Their copy must NOT have become our writable lane.
        assert_eq!(ours.our_label(), Some(&our_lane));
        // A re-merge of the same state is a no-op — echoes die on the position gate.
        assert!(!ours.merge_lanes_from(&theirs));
        // A merge never rewinds: our record of their lane outrunning their copy stays put.
        let stale = theirs.clone();
        theirs.advance(&their_lane, &et, &[3u8; 8], &[]);
        assert!(ours.merge_lanes_from(&theirs));
        assert!(!ours.merge_lanes_from(&stale));
        assert_eq!(ours.lane_position(&their_lane), Some(3));
    }

    #[test]
    fn era_divergent_blobs_never_lane_merge_and_newer_era_supersedes() {
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let old_eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let new_eggs: Vec<[u8; 32]> = (0..8).map(|i| [0x40 + i as u8; 32]).collect();

        // Same friendship, two ceremonies: different eggs mint a different lane root — a re-key era.
        let mut old_era = FriendshipChains::from_clutch(&[alice, bob], &old_eggs);
        let mut new_era = FriendshipChains::from_clutch(&[alice, bob], &new_eggs);
        old_era.mint_our_lane().unwrap();
        assert!(old_era.differs_in_era_from(&new_era));

        // The era judgment is the mutated clock, both directions.
        let new_lane = new_era.mint_our_lane().unwrap();
        let et = vsf::EagleTime::from_oscillations(vsf::eagle_time_oscillations());
        new_era.advance(&new_lane, &et, &[1u8; 8], &[]);
        assert!(old_era.era_superseded_by(&new_era));
        assert!(!new_era.era_superseded_by(&old_era));

        // Same era never reads as divergent — the lane merge path stays theirs.
        let same = new_era.clone();
        assert!(!new_era.differs_in_era_from(&same));
    }

    #[test]
    fn test_friendship_chains_deterministic() {
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();

        // Two ceremonies from the same inputs agree on id + root — and therefore on EVERY lane either side ever derives from a label. This is the both-sides property the whole receive-anywhere design stands on.
        let mut chains1 = FriendshipChains::from_clutch(&[alice, bob], &eggs);
        let mut chains2 = FriendshipChains::from_clutch(&[bob, alice], &eggs); // Different order

        // Same friendship ID
        assert_eq!(chains1.id().0, chains2.id().0);

        let label = [0x5Au8; 32];
        chains1.ensure_lane(&label).unwrap();
        chains2.ensure_lane(&label).unwrap();
        assert_eq!(
            chains1.current_key(&label).unwrap(),
            chains2.current_key(&label).unwrap()
        );
        assert_eq!(chains1.get_anchor(&label), chains2.get_anchor(&label));
    }

    /// THE lockstep property (docs/lanes.md): a receiver processing a lane's frames in order holds byte-identical lane state to the sender, which now advances on SEND — across multiple messages, so the salt chain (last_plaintext) is proven too. This is the whole contract: one writer, any number of deterministic followers.
    #[test]
    fn send_and_receive_advance_stay_in_lockstep_on_a_lane() {
        use crate::crypto::chain::{
            decrypt_layers, derive_salt, generate_scratch, CURRENT_KEY_INDEX,
        };
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let mut sender = FriendshipChains::from_clutch(&[alice, bob], &eggs);
        let mut receiver = FriendshipChains::from_clutch(&[bob, alice], &eggs);

        for (osc, text) in [(1_000i64, b"hello".as_slice()), (2_000, b"again")] {
            let (ct, prev, msg_hp, ph, lane) = sender
                .prepare_send(text.to_vec(), text.to_vec(), osc, vec![])
                .unwrap();
            // Receiver materializes the lane from the label alone and walks the hash chain.
            receiver.ensure_lane(&lane).unwrap();
            assert!(receiver.verify_chain_link(&lane, &prev).is_ok());
            let chain = receiver.chain(&lane).unwrap().clone();
            let salt = derive_salt(receiver.last_plaintext(&lane), &chain);
            let scratch = generate_scratch(&chain, &salt);
            let et = vsf::EagleTime::from_oscillations(osc);
            let plain = decrypt_layers(&ct, &chain, CURRENT_KEY_INDEX, &scratch, &et);
            assert_eq!(plain, text, "decrypt must invert encrypt at every step");
            receiver.advance(&lane, &et, text, &[]);
            receiver.set_last_plaintext(&lane, text.to_vec());
            receiver.update_received_hash(&lane, msg_hp);
            // The sender already advanced on send; the ACK is a pure receipt that matches + clears the pending, and both sides sit on the same key.
            assert!(sender.process_ack(osc, &ph));
            assert_eq!(
                sender.current_key(&lane).unwrap(),
                receiver.current_key(&lane).unwrap(),
                "lane keys diverged after osc {osc}"
            );
            assert_eq!(sender.lane_position(&lane), receiver.lane_position(&lane));
        }
    }

    /// PIPELINING: advance-on-send lets a sender emit a burst of frames with NO ACK between them, each encrypted at its own position, and a receiver walking them in order decrypts every one and lands byte-identical. This is what the in-flight window makes safe and what the serial gate used to forbid.
    #[test]
    fn a_pipelined_burst_decrypts_in_order_and_stays_in_lockstep() {
        use crate::crypto::chain::{
            decrypt_layers, derive_salt, generate_scratch, CURRENT_KEY_INDEX,
        };
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let mut sender = FriendshipChains::from_clutch(&[alice, bob], &eggs);
        let mut receiver = FriendshipChains::from_clutch(&[bob, alice], &eggs);

        // Burst FIVE messages with no ACK in between — the sender advances on each send.
        let texts: [&[u8]; 5] = [b"one", b"two", b"three", b"four", b"five"];
        let mut sent = Vec::new();
        for (i, text) in texts.iter().enumerate() {
            let osc = 1_000i64 + i as i64;
            let (ct, prev, msg_hp, ph, lane) = sender
                .prepare_send(text.to_vec(), text.to_vec(), osc, vec![])
                .unwrap();
            sent.push((ct, prev, msg_hp, ph, lane, osc, text.to_vec()));
        }
        // All five are in flight at once — the crypto layer imposes no serial limit (the in-flight window is UI-level flow control in chain_transmit, not here).
        assert_eq!(sender.pending_messages.len(), 5);

        // Receiver processes them strictly in order — each decrypts at its own position.
        for (ct, prev, msg_hp, _ph, lane, osc, text) in &sent {
            receiver.ensure_lane(lane).unwrap();
            assert!(
                receiver.verify_chain_link(lane, prev).is_ok(),
                "burst frame osc {osc} did not link in order"
            );
            let chain = receiver.chain(lane).unwrap().clone();
            let salt = derive_salt(receiver.last_plaintext(lane), &chain);
            let scratch = generate_scratch(&chain, &salt);
            let et = vsf::EagleTime::from_oscillations(*osc);
            let plain = decrypt_layers(ct, &chain, CURRENT_KEY_INDEX, &scratch, &et);
            assert_eq!(&plain, text, "pipelined frame osc {osc} failed to decrypt");
            receiver.advance(lane, &et, text, &[]);
            receiver.set_last_plaintext(lane, text.clone());
            receiver.update_received_hash(lane, *msg_hp);
        }

        // ACKs arrive in ANY order and only clear pendings — the chains already agree.
        let lane = sent[0].4;
        for (_ct, _prev, _msg_hp, ph, _lane, osc, _text) in sent.iter().rev() {
            assert!(sender.process_ack(*osc, ph));
        }
        assert!(sender.pending_messages.is_empty());
        assert_eq!(
            sender.current_key(&lane).unwrap(),
            receiver.current_key(&lane).unwrap(),
            "sender and receiver diverged after a pipelined burst"
        );
        assert_eq!(sender.lane_position(&lane), receiver.lane_position(&lane));
    }

    /// REGRESSION (field, 2026-08-08): an ACK whose plaintext_hash differs from the frozen pending — because the random hR pad made the hash unstable, or a re-encrypt raced — must STILL clear the pending, matched on eagle_time alone. When it didn't, the pending leaked and the message retransmitted forever while the peer re-ACKed every copy.
    #[test]
    fn process_ack_clears_on_eagle_time_even_when_hash_differs() {
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let mut sender = FriendshipChains::from_clutch(&[alice, bob], &eggs);

        let osc = 4_242i64;
        let (_ct, _prev, _msg_hp, ph, _lane) = sender
            .prepare_send(b"hi".to_vec(), b"hi".to_vec(), osc, vec![])
            .unwrap();
        assert_eq!(sender.pending_messages.len(), 1);

        // A wrong hash for the RIGHT eagle_time still clears the pending (soft-check, not a gate).
        let wrong_hash = [0xABu8; 32];
        assert_ne!(&wrong_hash, &ph, "test needs a genuinely different hash");
        assert!(sender.process_ack(osc, &wrong_hash));
        assert!(
            sender.pending_messages.is_empty(),
            "pending must clear on eagle_time match despite the hash mismatch — else it retransmits forever"
        );

        // An ACK for an eagle_time we never sent still matches nothing.
        assert!(!sender.process_ack(9_999, &ph));
    }

    /// PER-LANE REPLICATION: a subset carrying only some lanes must round-trip through merge_lanes_from and adopt EXACTLY those lanes at their real positions, leaving other lanes untouched — index-alignment across the parallel per-lane vecs is load-bearing (a slip corrupts a lane's chain/position/anchors).
    #[test]
    fn replication_subset_adopts_only_named_lanes_at_the_right_position() {
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        // A source holding two peer lanes at different positions.
        let mut src = FriendshipChains::from_clutch(&[alice, bob], &eggs);
        let lane_a = [0xAAu8; 32];
        let lane_b = [0xBBu8; 32];
        src.ensure_lane(&lane_a).unwrap();
        src.ensure_lane(&lane_b).unwrap();
        // Advance lane_a twice, lane_b once (distinct positions to catch an index slip).
        let et = vsf::EagleTime::from_oscillations(1);
        src.advance(&lane_a, &et, b"a1", &[]);
        src.advance(&lane_a, &et, b"a2", &[]);
        src.advance(&lane_b, &et, b"b1", &[]);
        let pos_a = src.lane_position(&lane_a).unwrap();
        let pos_b = src.lane_position(&lane_b).unwrap();
        assert_ne!(pos_a, pos_b, "test needs distinct positions");

        // A subset of just lane_a.
        let subset = src.replication_subset(&[lane_a]);
        assert_eq!(subset.lane_summary().len(), 1, "subset carries only the named lane");
        assert!(subset.pending_messages.is_empty(), "subset never carries pendings");

        // A fresh receiver adopts the subset: it gains lane_a at pos_a and knows nothing of lane_b.
        let mut rx = FriendshipChains::from_clutch(&[bob, alice], &eggs);
        assert!(rx.merge_lanes_from(&subset));
        assert_eq!(rx.lane_position(&lane_a), Some(pos_a), "adopted lane_a at its real position");
        assert_eq!(rx.lane_position(&lane_b), None, "lane_b not in the subset, so not adopted");

        // Now push lane_b's subset separately — it adopts alongside, lane_a unchanged.
        let subset_b = src.replication_subset(&[lane_b]);
        assert!(rx.merge_lanes_from(&subset_b));
        assert_eq!(rx.lane_position(&lane_a), Some(pos_a));
        assert_eq!(rx.lane_position(&lane_b), Some(pos_b));
    }

    #[test]
    fn test_gap_buffer_keys_on_prev_and_drains_on_fill() {
        // Layer 1: an out-of-order message is buffered on the prev_msg_hp it awaits, and is released by take_buffered_for ONLY when that exact predecessor's msg_hp fills. This is the wiring the receive path relies on to replay buffered messages strictly in order.
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let mut chains = FriendshipChains::from_clutch(&[alice, bob], &eggs);

        let addr: std::net::SocketAddr = "127.0.0.1:9000".parse().unwrap();
        let prev_a = [0xA1u8; 32]; // predecessor msg2 is waiting on
        let prev_b = [0xB2u8; 32]; // a different predecessor

        // Buffer msg2 (awaiting prev_a) and an unrelated msg (awaiting prev_b).
        chains.buffer_for_gap(prev_a, bob, 1000, vec![1, 2, 3], addr, bob);
        chains.buffer_for_gap(prev_b, bob, 1001, vec![4, 5, 6], addr, bob);
        assert_eq!(chains.gap_buffer_count(), 2);

        // Duplicate (same sender + same eagle_time) is not re-buffered.
        chains.buffer_for_gap(prev_a, bob, 1000, vec![1, 2, 3], addr, bob);
        assert_eq!(chains.gap_buffer_count(), 2);

        // Filling an unrelated hash releases nothing.
        assert!(chains.take_buffered_for(&[0xFFu8; 32]).is_empty());
        assert_eq!(chains.gap_buffer_count(), 2);

        // Filling prev_a releases exactly msg2; the prev_b waiter stays buffered.
        let ready = chains.take_buffered_for(&prev_a);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].eagle_time, 1000);
        assert_eq!(ready[0].ciphertext, vec![1, 2, 3]);
        assert_eq!(chains.gap_buffer_count(), 1);

        let ready_b = chains.take_buffered_for(&prev_b);
        assert_eq!(ready_b.len(), 1);
        assert_eq!(ready_b[0].eagle_time, 1001);
        assert_eq!(chains.gap_buffer_count(), 0);
    }

    #[test]
    fn era_supersede_newer_genesis_wins_wholesale_and_older_never_merges() {
        let a = [1u8; 32];
        let b = [2u8; 32];
        let eggs_old: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let eggs_new: Vec<[u8; 32]> = (0..8).map(|i| [i as u8 + 100; 32]).collect();
        let mut old_era = FriendshipChains::from_clutch(&[a, b], &eggs_old);
        let mut new_era = FriendshipChains::from_clutch(&[a, b], &eggs_new);
        assert!(old_era.differs_in_era_from(&new_era), "distinct eggs must derive distinct roots");
        old_era.genesis_osc = 100;
        new_era.genesis_osc = 200;
        // The dead era's clock keeps ticking (retransmit bookkeeping) — mutated_osc must NOT outvote genesis.
        old_era.mutated_osc = 9_999_999;
        new_era.mutated_osc = 1;
        let label = new_era.mint_our_lane().expect("new era mints a lane");
        let mut adopter = old_era;
        assert!(adopter.merge_lanes_from(&new_era), "older era must adopt the newer wholesale");
        assert_eq!(adopter.lane_root(), new_era.lane_root(), "root replaced");
        assert_eq!(adopter.genesis_osc, 200);
        assert!(adopter.lane_index(&label).is_some(), "newer era's lanes came along");
        assert!(adopter.pending_messages.is_empty(), "device-local state stripped on era adopt");
        // The reverse direction: the newer era must refuse the older blob entirely.
        let mut fresh = FriendshipChains::from_clutch(&[a, b], &eggs_new);
        fresh.genesis_osc = 200;
        let mut stale = FriendshipChains::from_clutch(&[a, b], &eggs_old);
        stale.genesis_osc = 100;
        stale.mutated_osc = 9_999_999;
        assert!(!fresh.merge_lanes_from(&stale), "a superseded era must never merge back in");
        assert_eq!(fresh.genesis_osc, 200);
    }

    #[test]
    fn test_retry_backoff_schedule() {
        // 1s, 2s, 4s, 8s, 16s, then capped at 30s, 30s…
        let s = |secs: u64| (secs * vsf::OSCILLATIONS_PER_SECOND) as i64;
        assert_eq!(retry_delay_osc(1), s(1));
        assert_eq!(retry_delay_osc(2), s(2));
        assert_eq!(retry_delay_osc(3), s(4));
        assert_eq!(retry_delay_osc(4), s(8));
        assert_eq!(retry_delay_osc(5), s(16));
        assert_eq!(retry_delay_osc(6), s(30)); // 32 capped to 30
        assert_eq!(retry_delay_osc(7), s(30));
        assert_eq!(retry_delay_osc(200), s(30)); // no overflow at large attempts
    }

    #[test]
    fn test_collect_due_retransmits_backoff_and_giveup() {
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let mut chains = FriendshipChains::from_clutch(&[alice, bob], &eggs);

        // Record one pending message at t0 (attempts=1, next_retry = t0 + 1s).
        let t0 = 1_000_000_000i64;
        chains.add_pending(
            t0,
            vec![1],
            [0xAA; 32],
            [0; 32],
            [9; 32],
            vec![7, 7, 7],
            vec![],
        );

        // Before the first deadline: nothing due.
        assert!(chains.collect_due_retransmits(t0).is_empty());

        // One second later: exactly one due, attempt becomes 2, ciphertext preserved.
        let one_s = vsf::OSCILLATIONS_PER_SECOND as i64;
        let due = chains.collect_due_retransmits(t0 + one_s);
        assert_eq!(due.len(), 1);
        let (et, _prev, ct, attempts, exhausted) = &due[0];
        assert_eq!(*et, t0);
        assert_eq!(*ct, vec![7, 7, 7]);
        assert_eq!(*attempts, 2);
        assert!(!exhausted);

        // Immediately after, not due again (deadline pushed out by the 2s backoff step).
        assert!(chains.collect_due_retransmits(t0 + one_s).is_empty());

        // Drive it to the give-up ceiling by always asking far in the future.
        let mut last_attempts = 2u8;
        let mut saw_exhausted = false;
        for k in 1..20 {
            let due = chains.collect_due_retransmits(t0 + one_s * 60 * k);
            if let Some((_, _, _, attempts, exhausted)) = due.first() {
                last_attempts = *attempts;
                if *exhausted {
                    saw_exhausted = true;
                }
            } else {
                break; // exhausted messages are no longer returned
            }
        }
        assert!(saw_exhausted, "should report exhausted at the ceiling");
        assert_eq!(last_attempts, MAX_SEND_ATTEMPTS);

        // After give-up the message is still pending (a late ACK can clear it) but never resent again.
        assert!(chains
            .collect_due_retransmits(t0 + one_s * 1_000_000)
            .is_empty());
    }

    #[test]
    fn test_rearm_pending_after_revives_given_up_gap_filler() {
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let mut chains = FriendshipChains::from_clutch(&[alice, bob], &eggs);
        let one_s = vsf::OSCILLATIONS_PER_SECOND as i64;
        let t0 = 1_000_000_000i64;

        // Two pending messages: an older one at t0 and a newer one at t0+10s.
        chains.add_pending(t0, vec![1], [0xAA; 32], [0; 32], [9; 32], vec![1], vec![]);
        chains.add_pending(
            t0 + 10 * one_s,
            vec![2],
            [0xBB; 32],
            [9; 32],
            [10; 32],
            vec![2],
            vec![],
        );

        // Exhaust both by asking far in the future repeatedly.
        for k in 1..20 {
            let _ = chains.collect_due_retransmits(t0 + one_s * 60 * k);
        }
        let far = t0 + one_s * 1_000_000;
        assert!(
            chains.collect_due_retransmits(far).is_empty(),
            "both exhausted"
        );

        // Peer's contiguous tip is t0 (it has the first message, is stalled missing the second). Re-arm should revive ONLY the newer message (eagle_time > tip), not the already-delivered one.
        let rearmed = chains.rearm_pending_after(t0, far);
        assert_eq!(rearmed, 1);

        // Now the revived message is due again immediately; the t0 one stays retired.
        let due = chains.collect_due_retransmits(far);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].0, t0 + 10 * one_s);

        // Re-arming past the newest tip revives nothing.
        assert_eq!(chains.rearm_pending_after(t0 + 10 * one_s, far), 0);
    }
}
