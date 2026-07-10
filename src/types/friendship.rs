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
        hasher.update(b"PHOTON_CEREMONY_v1");
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
        hasher.update(b"PHOTON_FRIENDSHIP_v1");
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
const DOMAIN_MSG_HP: &[u8] = b"PHOTON_MSG_HP_v1";
const DOMAIN_ANCHOR: &[u8] = b"PHOTON_ANCHOR_v1";

/// Reliability backoff for unacked outgoing messages. A message is (re)sent until an ACK arrives or we hit `MAX_SEND_ATTEMPTS`; between sends we wait `retry_delay_osc(attempts)` — exponential from ~1s, doubling, capped at ~30s. Covers both a dropped message AND a dropped ACK (the sender just keeps resending; the receiver dedupes by eagle_time and its ACK is deterministic, so a re-ACK is free). These live on `PendingMessage` and are runtime-only (not persisted).
const RETRY_BASE_SECS: u64 = 1;
const RETRY_CAP_SECS: u64 = 30;
const MAX_SEND_ATTEMPTS: u8 = 8;

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

    /// One chain per participant (sorted by handle_hash)
    chains: Vec<Chain>,

    /// Participant handle_hashes (sorted) - index matches chain index
    participants: Vec<[u8; 32]>,

    /// Last plaintext per chain (for salt derivation). Index matches chain index. Empty Vec = first message on that chain. Used to derive salt: `derive_salt(prev_plaintext, chain)`
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
    hasher.update(b"PHOTON_WEAVE_v0");
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

        // Step 2: Derive each participant's chain via truncate-and-append derive_chain_from_avalanche returns 8KB (256 active links) We need 16KB (256 history zeros + 256 active links)
        let mut chains = Vec::with_capacity(sorted_participants.len());
        let mut active_snapshots: Vec<Vec<u8>> = Vec::with_capacity(sorted_participants.len());
        for participant in &sorted_participants {
            let active_bytes = derive_chain_from_avalanche(&avalanche, participant);

            // Build full 16KB chain: [0..8KB] = history zeros, [8KB..16KB] = active links
            let mut full_chain = vec![0u8; CHAIN_SIZE];
            full_chain[CHAIN_SIZE / 2..].copy_from_slice(&active_bytes);

            let chain = Chain::from_full_bytes(&full_chain).expect("chain is 16KB");
            chains.push(chain);
            active_snapshots.push(active_bytes);
        }

        // Friend-history bulk key — derived HERE, at ceremony birth, from the pristine active chains (the one moment both sides are byte-identical). Every completion path flows thru from_clutch, so this is the single derivation site. See crypto::clutch::derive_history_key.
        let history_key = {
            let refs: Vec<&[u8]> = active_snapshots.iter().map(|v| v.as_slice()).collect();
            crate::crypto::clutch::derive_history_key(friendship_id.as_bytes(), &refs)
        };
        // The snapshots duplicate live chain secret material — scrub them.
        for snap in active_snapshots.iter_mut() {
            use zeroize::Zeroize;
            snap.zeroize();
        }

        // Initialize last_plaintexts with empty vecs (first message on each chain)
        let last_plaintexts = vec![Vec::new(); sorted_participants.len()];

        // Initialize last_received_times with None (no messages received yet)
        let last_received_times = vec![None; sorted_participants.len()];

        // Derive first_message_anchors for each participant's hash chain Anchor = BLAKE3(DOMAIN_ANCHOR || handle_hash || chain_fingerprint) where chain_fingerprint = BLAKE3(active_chain_portion)
        let first_message_anchors: Vec<[u8; 32]> = sorted_participants
            .iter()
            .zip(chains.iter())
            .map(|(handle_hash, chain)| derive_anchor(handle_hash, chain))
            .collect();

        // Initialize hash chain tracking (all None - no messages yet)
        let last_received_hashes = vec![None; sorted_participants.len()];

        Self {
            friendship_id,
            conversation_token,
            chains,
            participants: sorted_participants,
            last_plaintexts,
            pending_messages: Vec::new(),
            last_received_times,
            first_message_anchors,
            last_received_hashes,
            last_sent_hash: None,
            // Bidirectional entropy state (initialized empty)
            last_received_weave: None,
            last_sent_weave: None,
            last_incorporated_hp: None,
            gap_buffer: Vec::new(),
            history_key: Some(history_key),
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
            gap_buffer: Vec::new(), // Gap buffer is transient, not persisted
            history_key: None,      // pre-v6 file: no history key (set by the loader when present)
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
        chain_bytes: &[u8],
        last_sent_hash: Option<[u8; 32]>,
        mut last_received_hashes: Vec<Option<[u8; 32]>>,
        pending_messages: Vec<PendingMessage>,
        last_received_weave: Option<[u8; 32]>,
        last_sent_weave: Option<[u8; 32]>,
        last_incorporated_hp: Option<[u8; 32]>,
        mut last_plaintexts: Vec<Vec<u8>>,
        mut last_received_times: Vec<Option<i64>>,
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

        // If no last_plaintexts in file (v3 or earlier), initialize to empty vecs
        if last_plaintexts.is_empty() || last_plaintexts.len() != participants.len() {
            last_plaintexts = vec![Vec::new(); participants.len()];
        }

        // If no last_received_times in file (v4 or earlier), initialize to None
        if last_received_times.is_empty() || last_received_times.len() != participants.len() {
            last_received_times = vec![None; participants.len()];
        }

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
            gap_buffer: Vec::new(), // Gap buffer is transient, not persisted
            history_key: None,      // pre-v6 file default: loader sets it when the field is present
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

    /// Get current encryption key for a participant (for sending).
    pub fn current_key(&self, sender_handle_hash: &[u8; 32]) -> Option<&[u8; 32]> {
        let idx = self.participant_index(sender_handle_hash)?;
        Some(self.chains[idx].current_key())
    }

    /// Advance a participant's chain after ACK.
    ///
    /// Call this when we receive confirmation that a message was decrypted.
    ///
    /// Advance a participant's chain, braiding in `their_plaintexts` (the woven peer strands — two for a full braid, or fewer early in the conversation; the caller passes them sorted by eagle_time so both peers frame identically).
    pub fn advance(
        &mut self,
        sender_handle_hash: &[u8; 32],
        eagle_time: &vsf::EagleTime,
        our_plaintext: &[u8],
        their_plaintexts: &[&[u8]],
    ) -> bool {
        if let Some(idx) = self.participant_index(sender_handle_hash) {
            self.chains[idx].advance(eagle_time, our_plaintext, their_plaintexts);
            true
        } else {
            false
        }
    }

    /// Get chain for a participant (for debugging/inspection).
    pub fn chain(&self, handle_hash: &[u8; 32]) -> Option<&Chain> {
        let idx = self.participant_index(handle_hash)?;
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

    /// Get last plaintext for a participant's chain (for salt derivation). Returns empty slice for first message on that chain.
    pub fn last_plaintext(&self, handle_hash: &[u8; 32]) -> &[u8] {
        if let Some(idx) = self.participant_index(handle_hash) {
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
    pub fn set_last_plaintext(&mut self, handle_hash: &[u8; 32], plaintext: Vec<u8>) {
        if let Some(idx) = self.participant_index(handle_hash) {
            self.last_plaintexts[idx] = plaintext;
        }
    }

    /// Check if a message is a duplicate (already received from this sender). Returns true if this is a duplicate and should be skipped.
    pub fn is_duplicate(&self, sender_handle_hash: &[u8; 32], eagle_time: i64) -> bool {
        if let Some(idx) = self.participant_index(sender_handle_hash) {
            if let Some(last_time) = self.last_received_times[idx] {
                // Duplicate if eagle_time <= last received (exact match or older)
                return eagle_time <= last_time;
            }
        }
        false
    }

    /// Mark a message as received (update last received time for deduplication).
    pub fn mark_received(&mut self, sender_handle_hash: &[u8; 32], eagle_time: i64) {
        if let Some(idx) = self.participant_index(sender_handle_hash) {
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


    // ==================== HASH CHAIN METHODS ====================

    /// Get the first message anchor for a participant. Used as prev_msg_hp for the first message on their chain.
    pub fn get_anchor(&self, handle_hash: &[u8; 32]) -> Option<&[u8; 32]> {
        let idx = self.participant_index(handle_hash)?;
        Some(&self.first_message_anchors[idx])
    }

    /// Get prev_msg_hp for the next outgoing message. Returns last_sent_hash if we've sent messages, otherwise our anchor.
    pub fn get_prev_msg_hp(&self, our_handle_hash: &[u8; 32]) -> Option<[u8; 32]> {
        if let Some(hash) = self.last_sent_hash {
            Some(hash)
        } else {
            // First message - use our anchor
            self.get_anchor(our_handle_hash).copied()
        }
    }

    /// Get the expected prev_msg_hp for incoming message from a sender. Returns their last_received_hash, or their anchor if first message.
    pub fn get_expected_prev_hp(&self, sender_handle_hash: &[u8; 32]) -> Option<[u8; 32]> {
        let idx = self.participant_index(sender_handle_hash)?;
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
        sender_handle_hash: &[u8; 32],
        received_prev_msg_hp: &[u8; 32],
    ) -> Result<(), [u8; 32]> {
        let expected = self
            .get_expected_prev_hp(sender_handle_hash)
            .ok_or([0u8; 32])?;

        if received_prev_msg_hp == &expected {
            Ok(())
        } else {
            Err(expected)
        }
    }

    /// Update hash chain state after successfully receiving and decrypting a message. Call this AFTER verify_chain_link succeeds and decrypt succeeds.
    pub fn update_received_hash(&mut self, sender_handle_hash: &[u8; 32], msg_hp: [u8; 32]) {
        if let Some(idx) = self.participant_index(sender_handle_hash) {
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

    /// Get pending messages that come after a given hash pointer. Used for resync: peer says "I have hash X", we return messages after X.
    ///
    /// Returns Vec of (eagle_time, ciphertext, prev_msg_hp) for resending.
    pub fn get_pending_after(&self, after_hash: &[u8; 32]) -> Vec<(i64, Vec<u8>, [u8; 32])> {
        let mut found_start = false;
        let mut result = Vec::new();

        // Check if after_hash matches our anchor (they want everything) or if it's somewhere in our pending chain
        for pending in &self.pending_messages {
            if found_start {
                result.push((
                    pending.eagle_time,
                    pending.ciphertext.clone(),
                    pending.prev_msg_hp,
                ));
            } else if &pending.prev_msg_hp == after_hash || pending.msg_hp == *after_hash {
                // Found the starting point - include this one and all after
                if &pending.prev_msg_hp == after_hash {
                    // They have the message before this one
                    result.push((
                        pending.eagle_time,
                        pending.ciphertext.clone(),
                        pending.prev_msg_hp,
                    ));
                }
                found_start = true;
            }
        }

        // If after_hash is one of our anchors, return all pending
        if !found_start && self.first_message_anchors.iter().any(|a| a == after_hash) {
            return self
                .pending_messages
                .iter()
                .map(|p| (p.eagle_time, p.ciphertext.clone(), p.prev_msg_hp))
                .collect();
        }

        result
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
    /// Does NOT advance the chain — advancement is deferred to [`process_ack`](Self::process_ack), the same invariant the receive side relies on (advancing on send would desync if the peer never decrypts).
    ///
    /// Returns `(ciphertext, prev_msg_hp, msg_hp, plaintext_hash)` for the wire send, or `None` if `our_handle_hash` isn't a participant. `plaintext` is the FULL flattened VSF payload (`(message: x{}, hp{}, hR{pad})`) — this is what goes on the wire (encrypted) and what both sides hash for `msg_hp`/ACK. `salt_text` is the bare message x-text only: the salt source + the `our_plaintext` fed to the braid's `derive_fresh_link` on ACK-advance. The two are SEPARATE on purpose — the random `hR` pad and the public `hp` are traffic-analysis/wire concerns, never chain-key material, and keeping them out of the chain ingredient keeps it valid UTF-8 (so it stores losslessly) and matches the receiver, which advances + salts from the decrypted x-text only.
    pub fn prepare_send(
        &mut self,
        our_handle_hash: &[u8; 32],
        plaintext: Vec<u8>,
        salt_text: Vec<u8>,
        eagle_time: i64,
        woven_strands: Vec<Vec<u8>>,
    ) -> Option<(Vec<u8>, [u8; 32], [u8; 32], [u8; 32])> {
        use crate::crypto::chain::{derive_salt, encrypt_layers, generate_scratch};

        let our_idx = self.participant_index(our_handle_hash)?;
        let our_chain = self.chains[our_idx].clone();

        // Salt from our previous plaintext (empty on the first message) — both sides derive the same salt for the same chain position.
        let salt = derive_salt(&self.last_plaintexts[our_idx], &our_chain);
        let scratch = generate_scratch(&our_chain, &salt);
        let et = vsf::EagleTime::from_oscillations(eagle_time);
        let ciphertext = encrypt_layers(&plaintext, &our_chain, &scratch, &et);

        // Mirror the receiver's "CHAIN DECRYPT" line so both sides can be diffed: for a given eagle_time the encrypt key+salt here MUST equal the decrypt key+salt on the peer, or the chains have diverged. last_plaintext_len flags the lossy-storage class of bug (a non-empty prev that round-tripped thru storage must be byte-identical on both ends).
        crate::log(&format!(
            "CHAIN ENCRYPT: our_handle_hash = {}..., key = {}..., salt = {}..., eagle_time = {}, last_plaintext_len = {}, ciphertext_len = {}",
            hex::encode(&our_handle_hash[..4]),
            hex::encode(&our_chain.current_key()[..4]),
            hex::encode(&salt[..4]),
            eagle_time,
            self.last_plaintexts[our_idx].len(),
            ciphertext.len()
        ));

        // First message uses our anchor as prev_msg_hp (matches get_expected_prev_hp on the receiver).
        let prev_msg_hp = self
            .last_sent_hash
            .unwrap_or(self.first_message_anchors[our_idx]);
        // Hash + msg_hp are over the FULL payload (the receiver hashes the full decrypted bytes too).
        let plaintext_hash = *blake3::hash(&plaintext).as_bytes();
        let msg_hp = derive_msg_hp(&prev_msg_hp, &plaintext_hash, eagle_time);

        // Pending stores the SALT-TEXT (not the full payload): process_ack advances the chain with it (as our_plaintext) and it becomes last_plaintext for the next salt — both must equal what the receiver uses, which is the decrypted x-text only.
        self.add_pending(
            eagle_time,
            salt_text,
            plaintext_hash,
            prev_msg_hp,
            msg_hp,
            ciphertext.clone(),
            woven_strands,
        );

        Some((ciphertext, prev_msg_hp, msg_hp, plaintext_hash))
    }

    /// Process ACK: find pending message, advance our chain, update last_plaintext, clear pending. Chain advancement is deferred to ACK to prevent desync — if we advanced on send and the receiver never processed the message, both sides' copies of our chain would diverge. Returns true if ACK was valid and chain was advanced.
    pub fn process_ack(
        &mut self,
        our_handle_hash: &[u8; 32],
        acked_eagle_time: i64,
        acked_plaintext_hash: &[u8; 32],
    ) -> bool {
        // Find the pending message by eagle_time and plaintext_hash (exact i64 match)
        let pos = self.pending_messages.iter().position(|m| {
            m.eagle_time == acked_eagle_time && &m.plaintext_hash == acked_plaintext_hash
        });

        if let Some(idx) = pos {
            let pending = self.pending_messages.remove(idx);

            // The braid: advance with the EXACT woven strands this message braided at send time (frozen on the PendingMessage), NOT whatever we hold now. The receiver advanced its copy of our chain using the strands named by the two eagle_times on the wire; these frozen bytes are those same strands. Using "latest plaintext" here was the original desync (order-dependent). Strands are already sorted by eagle_time.
            let eagle_time = vsf::EagleTime::from_oscillations(pending.eagle_time);
            let strand_refs: Vec<&[u8]> =
                pending.woven_strands.iter().map(|s| s.as_slice()).collect();
            self.advance(
                our_handle_hash,
                &eagle_time,
                &pending.plaintext,
                &strand_refs,
            );

            // Update last_plaintext for salt derivation on next message
            if let Some(chain_idx) = self.participant_index(our_handle_hash) {
                self.last_plaintexts[chain_idx] = pending.plaintext;
                return true;
            }
        }
        false
    }

    /// Clear all pending messages up to and including the given msg_hp. Used for hp-based sync: peer tells us their last_received_hp, we clear everything they have. Returns count of messages cleared.
    pub fn clear_pending_up_to(
        &mut self,
        our_handle_hash: &[u8; 32],
        up_to_hp: &[u8; 32],
    ) -> usize {
        let mut cleared = 0;
        let mut last_plaintext_to_set: Option<Vec<u8>> = None;

        // Remove all pending messages up to and including the one with msg_hp == up_to_hp
        self.pending_messages.retain(|m| {
            if cleared > 0 || m.msg_hp == *up_to_hp {
                // This message or earlier - they have it, clear it
                last_plaintext_to_set = Some(m.plaintext.clone());
                cleared += 1;
                false // remove
            } else {
                true // keep - this is after the sync point
            }
        });

        // Update last_plaintext for salt derivation
        if let (Some(plaintext), Some(chain_idx)) = (
            last_plaintext_to_set,
            self.participant_index(our_handle_hash),
        ) {
            self.last_plaintexts[chain_idx] = plaintext;
        }

        cleared
    }

    /// Get the most recent pending plaintext (for salt derivation of next send). If no pending messages, returns last_plaintext for our chain.
    pub fn current_send_plaintext(&self, our_handle_hash: &[u8; 32]) -> &[u8] {
        // If we have pending messages, use the last one's plaintext
        if let Some(last_pending) = self.pending_messages.last() {
            &last_pending.plaintext
        } else {
            // Otherwise use the last acked plaintext from our chain
            self.last_plaintext(our_handle_hash)
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

    /// Process implicit ACK from their_incorporated_hp. Removes all pending messages up to and including the given msg_hp. Returns number of messages cleared.
    pub fn process_implicit_ack(&mut self, their_incorporated_hp: &[u8; 32]) -> usize {
        let mut cleared = 0;

        // Find the position of the acked message
        if let Some(ack_pos) = self
            .pending_messages
            .iter()
            .position(|m| &m.msg_hp == their_incorporated_hp)
        {
            // Remove all messages up to and including this one They're in order, so we can drain 0..=ack_pos
            cleared = ack_pos + 1;
            self.pending_messages.drain(0..cleared);
        }

        cleared
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
        });
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

        let chains = FriendshipChains::from_clutch(&[alice, bob], &eggs);

        // Should have 2 chains (one per participant)
        assert_eq!(chains.participant_count(), 2);
        assert_eq!(chains.total_size(), 2 * CHAIN_SIZE);

        // Participants should be sorted
        let participants = chains.participants();
        assert!(participants[0] < participants[1]);

        // Should be able to get keys for both
        assert!(chains.current_key(&alice).is_some());
        assert!(chains.current_key(&bob).is_some());

        // Unknown participant should return None
        assert!(chains.current_key(&[99u8; 32]).is_none());
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
        assert_eq!(fids.len(), 3, "each sibling pair must get a distinct friendship id");

        // pid-keyed chains: both participants resolve, other_participant round-trips, and an advance moves only the sender's strand.
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();
        let mut chains = FriendshipChains::from_clutch(&[pids[0], pids[1]], &eggs);
        assert!(chains.current_key(&pids[0]).is_some());
        assert!(chains.current_key(&pids[1]).is_some());
        assert_eq!(chains.other_participant(&pids[0]), Some(&pids[1]));
        let key_b_before = *chains.current_key(&pids[1]).unwrap();
        let eagle_time = vsf::EagleTime::from_oscillations(vsf::eagle_time_oscillations());
        assert!(chains.advance(&pids[0], &eagle_time, &[0xAA; 32], &[]));
        assert_eq!(*chains.current_key(&pids[1]).unwrap(), key_b_before);
    }

    #[test]
    fn test_friendship_chains_advance() {
        use vsf::EagleTime;

        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();

        let mut chains = FriendshipChains::from_clutch(&[alice, bob], &eggs);

        // Save original keys
        let alice_key_before = *chains.current_key(&alice).unwrap();
        let bob_key_before = *chains.current_key(&bob).unwrap();

        // Advance Alice's chain (no bidirectional entropy for this test)
        let eagle_time = vsf::EagleTime::from_oscillations(vsf::eagle_time_oscillations());
        let plaintext_hash = [0xAA; 32];
        assert!(chains.advance(&alice, &eagle_time, &plaintext_hash, &[]));

        // Alice's key should change
        let alice_key_after = *chains.current_key(&alice).unwrap();
        assert_ne!(alice_key_before, alice_key_after);

        // Bob's key should NOT change
        let bob_key_after = *chains.current_key(&bob).unwrap();
        assert_eq!(bob_key_before, bob_key_after);
    }

    #[test]
    fn test_friendship_chains_storage_roundtrip() {
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();

        let original = FriendshipChains::from_clutch(&[alice, bob], &eggs);

        // Serialize
        let chain_bytes = original.chains_to_bytes();
        let participants = original.participants().to_vec();
        let friendship_id = *original.id();

        // Deserialize (v3 with defaults for optional state)
        let restored = FriendshipChains::from_storage_v3(
            friendship_id,
            participants,
            &chain_bytes,
            None,
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        )
        .unwrap();

        // Should have same keys
        assert_eq!(
            original.current_key(&alice).unwrap(),
            restored.current_key(&alice).unwrap()
        );
        assert_eq!(
            original.current_key(&bob).unwrap(),
            restored.current_key(&bob).unwrap()
        );
    }

    #[test]
    fn test_friendship_chains_deterministic() {
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();

        // Two chains from same inputs should be identical
        let chains1 = FriendshipChains::from_clutch(&[alice, bob], &eggs);
        let chains2 = FriendshipChains::from_clutch(&[bob, alice], &eggs); // Different order

        // Same friendship ID
        assert_eq!(chains1.id().0, chains2.id().0);

        // Same keys
        assert_eq!(
            chains1.current_key(&alice).unwrap(),
            chains2.current_key(&alice).unwrap()
        );
        assert_eq!(
            chains1.current_key(&bob).unwrap(),
            chains2.current_key(&bob).unwrap()
        );
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
        chains.buffer_for_gap(prev_a, bob, 1000, vec![1, 2, 3], addr);
        chains.buffer_for_gap(prev_b, bob, 1001, vec![4, 5, 6], addr);
        assert_eq!(chains.gap_buffer_count(), 2);

        // Duplicate (same sender + same eagle_time) is not re-buffered.
        chains.buffer_for_gap(prev_a, bob, 1000, vec![1, 2, 3], addr);
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
        chains.add_pending(t0, vec![1], [0xAA; 32], [0; 32], [9; 32], vec![7, 7, 7], vec![]);

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
        chains.add_pending(t0 + 10 * one_s, vec![2], [0xBB; 32], [9; 32], [10; 32], vec![2], vec![]);

        // Exhaust both by asking far in the future repeatedly.
        for k in 1..20 {
            let _ = chains.collect_due_retransmits(t0 + one_s * 60 * k);
        }
        let far = t0 + one_s * 1_000_000;
        assert!(chains.collect_due_retransmits(far).is_empty(), "both exhausted");

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
