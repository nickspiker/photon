//! Friendship types for per-conversation encryption.
//!
//! A "friendship" is a deterministic conversation identifier derived from
//! the sorted handle hashes of all participants. This enables:
//!
//! - **Self-notes**: 1 participant (handle_hash of self)
//! - **DMs**: 2 participants
//! - **Groups**: N participants
//!
//! Each friendship has N chains (one per participant), where each person
//! only advances their own chain on ACK.

use crate::crypto::chain::{Chain, CHAIN_SIZE};

/// Ceremony ID: deterministic CLUTCH ceremony identifier.
///
/// Derived via spaghettify from handle_hashes + sorted ping provenances:
/// 1. Fast base: `BLAKE3("PHOTON_CEREMONY_v1" || sorted_handle_hashes)`
/// 2. Nonce: Sorted ping provenances (unique per ceremony via timestamps)
/// 3. Final: `spaghettify(base || sorted_provenances...)`
///
/// Same value on all participants' devices - both parties collect all pings.
/// Unique per ceremony due to nanosecond timestamps in ping provenances.
/// No memory-hard step needed - timestamp entropy defeats rainbow tables.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CeremonyId(pub [u8; 32]);

impl CeremonyId {
    /// Derive ceremony ID base from participant handle hashes (fast step).
    ///
    /// This is the deterministic BLAKE3 hash that identifies the participants.
    /// Handle hashes are sorted for canonical ordering.
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
    /// Ping provenances are BLAKE3(sender_pubkey || timestamp_nanos) from each
    /// party's ping. Both parties collect all pings, sort them, and derive
    /// the same ceremony_id deterministically.
    ///
    /// No memory-hard computation needed - nanosecond timestamps provide
    /// enough entropy to defeat rainbow table attacks.
    pub fn derive(handle_hashes: &[[u8; 32]], ping_provenances: &[[u8; 32]]) -> Self {
        use crate::crypto::clutch::spaghettify;

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
/// Derived as `BLAKE3("PHOTON_FRIENDSHIP_v1" || sorted_handle_hashes)`
/// Same value on all participants' devices.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FriendshipId(pub [u8; 32]);

impl FriendshipId {
    /// Derive friendship ID from participant handle hashes.
    ///
    /// Handle hashes are sorted for canonical ordering - the same participants
    /// will always produce the same friendship ID regardless of order.
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

/// Per-participant encryption chains for a friendship.
///
/// Each participant has their own chain (16KB). When sending, use sender's chain.
/// When receiving ACK, advance sender's chain. This prevents race conditions
/// in simultaneous sends and scales to N-party conversations.
///
/// ## Hash Chain Protocol
///
/// Every message includes `prev_msg_hp` - a hash pointer to the previous message.
/// This creates a cryptographic chain that:
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

    /// Privacy-preserving conversation token for wire format.
    /// Derived via smear_hash(sorted_participant_seeds) - only participants can compute.
    /// Replaces cleartext handle_hashes in messages.
    pub conversation_token: [u8; 32],

    /// One chain per participant (sorted by handle_hash)
    chains: Vec<Chain>,

    /// Participant handle_hashes (sorted) - index matches chain index
    participants: Vec<[u8; 32]>,

    /// Last plaintext per chain (for salt derivation).
    /// Index matches chain index. Empty Vec = first message on that chain.
    /// Used to derive salt: `derive_salt(prev_plaintext, chain)`
    last_plaintexts: Vec<Vec<u8>>,

    /// Pending sent messages awaiting ACK (for our chain only).
    /// When we send, we store plaintext here. On ACK, we advance and clear.
    /// Vec because we can send multiple messages before receiving ACKs.
    pub pending_messages: Vec<PendingMessage>,

    /// Last received message time per participant (for duplicate detection).
    /// Index matches chain index. None = no message received yet from that sender.
    /// If incoming message has eagle_time <= this value, it's a duplicate (skip).
    last_received_times: Vec<Option<f64>>,

    // ==================== HASH CHAIN STATE ====================
    /// First message anchor per participant (deterministic starting point).
    /// Derived from: BLAKE3(DOMAIN_ANCHOR || participant_handle_hash || chain_fingerprint)
    /// where chain_fingerprint = BLAKE3(chain[256..512]).
    /// Both parties compute identical anchors from CLUTCH ceremony.
    first_message_anchors: Vec<[u8; 32]>,

    /// Last received message hash per participant (for hash chain verification).
    /// Index matches chain index. None = no message received yet → expect anchor.
    /// On receive: verify prev_msg_hp == this value (or anchor if None).
    /// After successful decrypt: update to msg_hp of received message.
    last_received_hashes: Vec<Option<[u8; 32]>>,

    /// Last sent message hash (for our chain only).
    /// Used as prev_msg_hp in next outgoing message.
    /// None = first message → use our anchor.
    /// Updated after each send (before ACK - hash chain is independent).
    last_sent_hash: Option<[u8; 32]>,

    // ==================== BIDIRECTIONAL ENTROPY STATE ====================
    /// Last received weave hash (for bidirectional entropy mixing).
    /// Derived from: hash(DOMAIN || eagle_time || msg_hp || plaintext)
    /// This prevents brute-forcing even if plaintext is guessable.
    /// When we send, we mix this into our chain advancement.
    /// Updated after each successful decrypt.
    last_received_weave: Option<[u8; 32]>,

    /// Last sent weave hash (what we sent = what they received).
    /// When receiver advances their view of our chain, they use this
    /// to match what we used for mixing when we received their ACK.
    /// Updated after each send.
    last_sent_weave: Option<[u8; 32]>,

    /// Hash pointer of the message whose weave we last incorporated.
    /// Included in outgoing messages as `their_incorporated_hp`.
    /// Acts as implicit ACK - tells peer we received up to this message.
    last_incorporated_hp: Option<[u8; 32]>,

    /// Buffer for out-of-order messages (gap handling).
    /// When we receive a message with prev_msg_hp that doesn't match our
    /// last_received_hash, we store it here until the gap is filled.
    gap_buffer: Vec<BufferedMessage>,
}

/// A message buffered due to gap in hash chain (out-of-order delivery).
/// Stored until preceding messages arrive and gap is filled.
#[derive(Clone)]
pub struct BufferedMessage {
    /// The message's hash pointer (for matching when gap fills)
    pub msg_hp: [u8; 32],
    /// The expected prev_msg_hp (what we need to receive first)
    pub prev_msg_hp: [u8; 32],
    /// Sender's handle hash
    pub sender_handle_hash: [u8; 32],
    /// Eagle time of message
    pub eagle_time: f64,
    /// Encrypted ciphertext (decrypt when gap fills)
    pub ciphertext: Vec<u8>,
}

/// A sent message awaiting ACK confirmation.
///
/// Stored in pending_messages until ACKed. Contains everything needed to:
/// 1. Match incoming ACK (eagle_time + plaintext_hash)
/// 2. Advance chain on ACK (plaintext_hash)
/// 3. Resend if no ACK (ciphertext + prev_msg_hp)
/// 4. Derive next message's salt (plaintext)
///
/// After ACK: removed from pending, chain advances, forward secrecy kicks in.
/// Without ACK: can be resent from ciphertext (still have encrypted form).
#[derive(Clone)]
pub struct PendingMessage {
    /// Eagle time of this message (for ACK matching and nonce)
    pub eagle_time: f64,
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
}

// ============================================================================
// Hash Chain Derivation Functions
// ============================================================================

/// Derive first message anchor for a participant's hash chain.
///
/// Anchor = BLAKE3(DOMAIN_ANCHOR || handle_hash || chain_fingerprint)
/// where chain_fingerprint = BLAKE3(active_chain_bytes).
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
    eagle_time: f64,
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
/// The weave incorporates the full message context (timestamp, msg_hp, plaintext)
/// into a 32-byte hash. This prevents brute-forcing even if the plaintext is
/// guessable ("ok", "yes", etc.) because the exact timestamp acts as a nonce.
///
/// Domain: PHOTON_WEAVE_v0
pub fn derive_weave_hash(eagle_time: f64, msg_hp: &[u8; 32], plaintext: &[u8]) -> [u8; 32] {
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
    /// - No compression: full entropy preserved through derivation
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

        // Step 2: Derive each participant's chain via truncate-and-append
        // derive_chain_from_avalanche returns 8KB (256 active links)
        // We need 16KB (256 history zeros + 256 active links)
        let mut chains = Vec::with_capacity(sorted_participants.len());
        for participant in &sorted_participants {
            let active_bytes = derive_chain_from_avalanche(&avalanche, participant);

            // Build full 16KB chain: [0..8KB] = history zeros, [8KB..16KB] = active links
            let mut full_chain = vec![0u8; CHAIN_SIZE];
            full_chain[CHAIN_SIZE / 2..].copy_from_slice(&active_bytes);

            let chain = Chain::from_full_bytes(&full_chain).expect("chain is 16KB");
            chains.push(chain);
        }

        // Initialize last_plaintexts with empty vecs (first message on each chain)
        let last_plaintexts = vec![Vec::new(); sorted_participants.len()];

        // Initialize last_received_times with None (no messages received yet)
        let last_received_times = vec![None; sorted_participants.len()];

        // Derive first_message_anchors for each participant's hash chain
        // Anchor = BLAKE3(DOMAIN_ANCHOR || handle_hash || chain_fingerprint)
        // where chain_fingerprint = BLAKE3(active_chain_portion)
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

        // Derive first_message_anchors for each participant's hash chain
        // These are deterministic from chain state, so we recompute them
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
        mut last_received_times: Vec<Option<f64>>,
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

        // Derive first_message_anchors for each participant's hash chain
        // These are deterministic from chain state, so we recompute them
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
        })
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
    /// Bidirectional entropy: if `their_plaintext` is provided, it's mixed into
    /// the chain advancement. Pass the other party's most recent plaintext here.
    pub fn advance(
        &mut self,
        sender_handle_hash: &[u8; 32],
        eagle_time: &vsf::EagleTime,
        our_plaintext: &[u8],
        their_plaintext: Option<&[u8]>,
    ) -> bool {
        if let Some(idx) = self.participant_index(sender_handle_hash) {
            self.chains[idx].advance(eagle_time, our_plaintext, their_plaintext);
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

    /// Get last plaintext for a participant's chain (for salt derivation).
    /// Returns empty slice for first message on that chain.
    pub fn last_plaintext(&self, handle_hash: &[u8; 32]) -> &[u8] {
        if let Some(idx) = self.participant_index(handle_hash) {
            &self.last_plaintexts[idx]
        } else {
            &[]
        }
    }

    /// Get the "other" participant in a 2-party conversation.
    /// Returns None for self-notes (1-party) or group chats (3+ party).
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

    /// Check if a message is a duplicate (already received from this sender).
    /// Returns true if this is a duplicate and should be skipped.
    pub fn is_duplicate(&self, sender_handle_hash: &[u8; 32], eagle_time: f64) -> bool {
        if let Some(idx) = self.participant_index(sender_handle_hash) {
            if let Some(last_time) = self.last_received_times[idx] {
                // Duplicate if eagle_time <= last received (exact match or older)
                return eagle_time <= last_time;
            }
        }
        false
    }

    /// Mark a message as received (update last received time for deduplication).
    pub fn mark_received(&mut self, sender_handle_hash: &[u8; 32], eagle_time: f64) {
        if let Some(idx) = self.participant_index(sender_handle_hash) {
            self.last_received_times[idx] = Some(eagle_time);
        }
    }

    // ==================== HASH CHAIN METHODS ====================

    /// Get the first message anchor for a participant.
    /// Used as prev_msg_hp for the first message on their chain.
    pub fn get_anchor(&self, handle_hash: &[u8; 32]) -> Option<&[u8; 32]> {
        let idx = self.participant_index(handle_hash)?;
        Some(&self.first_message_anchors[idx])
    }

    /// Get prev_msg_hp for the next outgoing message.
    /// Returns last_sent_hash if we've sent messages, otherwise our anchor.
    pub fn get_prev_msg_hp(&self, our_handle_hash: &[u8; 32]) -> Option<[u8; 32]> {
        if let Some(hash) = self.last_sent_hash {
            Some(hash)
        } else {
            // First message - use our anchor
            self.get_anchor(our_handle_hash).copied()
        }
    }

    /// Get the expected prev_msg_hp for incoming message from a sender.
    /// Returns their last_received_hash, or their anchor if first message.
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
    /// Returns Ok(()) if chain is valid, Err with expected hash if mismatch.
    /// Caller can use the expected hash to request resync.
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

    /// Update hash chain state after successfully receiving and decrypting a message.
    /// Call this AFTER verify_chain_link succeeds and decrypt succeeds.
    pub fn update_received_hash(&mut self, sender_handle_hash: &[u8; 32], msg_hp: [u8; 32]) {
        if let Some(idx) = self.participant_index(sender_handle_hash) {
            self.last_received_hashes[idx] = Some(msg_hp);
        }
    }

    /// Get pending messages that come after a given hash pointer.
    /// Used for resync: peer says "I have hash X", we return messages after X.
    ///
    /// Returns Vec of (eagle_time, ciphertext, prev_msg_hp) for resending.
    pub fn get_pending_after(&self, after_hash: &[u8; 32]) -> Vec<(f64, Vec<u8>, [u8; 32])> {
        let mut found_start = false;
        let mut result = Vec::new();

        // Check if after_hash matches our anchor (they want everything)
        // or if it's somewhere in our pending chain
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
    pub fn last_received_times(&self) -> &Vec<Option<f64>> {
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
        eagle_time: f64,
        plaintext: Vec<u8>,
        plaintext_hash: [u8; 32],
        prev_msg_hp: [u8; 32],
        msg_hp: [u8; 32],
        ciphertext: Vec<u8>,
    ) {
        self.pending_messages.push(PendingMessage {
            eagle_time,
            plaintext,
            plaintext_hash,
            prev_msg_hp,
            msg_hp,
            ciphertext,
        });

        // Update last_sent_hash for next message's prev_msg_hp
        self.last_sent_hash = Some(msg_hp);
    }

    /// Process ACK: find pending message, update last_plaintext, clear pending.
    /// Chain already advanced on send - this just confirms delivery.
    /// Returns true if ACK was valid.
    pub fn process_ack(
        &mut self,
        our_handle_hash: &[u8; 32],
        acked_eagle_time: f64,
        acked_plaintext_hash: &[u8; 32],
    ) -> bool {
        // Find the pending message by eagle_time and plaintext_hash
        let pos = self.pending_messages.iter().position(|m| {
            (m.eagle_time - acked_eagle_time).abs() < 0.001 // ~1ms tolerance
                && &m.plaintext_hash == acked_plaintext_hash
        });

        if let Some(idx) = pos {
            let pending = self.pending_messages.remove(idx);

            // Chain already advanced on send - just update last_plaintext for salt derivation
            // (used when no pending messages remain)
            if let Some(chain_idx) = self.participant_index(our_handle_hash) {
                self.last_plaintexts[chain_idx] = pending.plaintext;
                return true;
            }
        }
        false
    }

    /// Clear all pending messages up to and including the given msg_hp.
    /// Used for hp-based sync: peer tells us their last_received_hp, we clear everything they have.
    /// Returns count of messages cleared.
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

    /// Get the most recent pending plaintext (for salt derivation of next send).
    /// If no pending messages, returns last_plaintext for our chain.
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

    /// Get the last received weave hash (for bidirectional entropy mixing).
    /// Returns None if no messages received yet.
    pub fn last_received_weave(&self) -> Option<&[u8; 32]> {
        self.last_received_weave.as_ref()
    }

    /// Get the hash pointer of the message we last incorporated.
    /// Include this in outgoing messages as `their_incorporated_hp`.
    pub fn last_incorporated_hp(&self) -> Option<&[u8; 32]> {
        self.last_incorporated_hp.as_ref()
    }

    /// Update bidirectional entropy state after successful decrypt.
    /// Call this AFTER verify_chain_link succeeds and decrypt succeeds.
    ///
    /// Derives a weave hash from the full message context (timestamp, msg_hp, plaintext).
    /// This prevents brute-forcing even if plaintext is guessable.
    pub fn update_received_for_mixing(
        &mut self,
        eagle_time: f64,
        msg_hp: [u8; 32],
        plaintext: &[u8],
    ) {
        let weave = derive_weave_hash(eagle_time, &msg_hp, plaintext);
        self.last_received_weave = Some(weave);
        self.last_incorporated_hp = Some(msg_hp);
    }

    /// Get the last sent weave hash (what we sent = what they received).
    /// Used by receiver to advance their view of our chain with matching entropy.
    pub fn last_sent_weave(&self) -> Option<&[u8; 32]> {
        self.last_sent_weave.as_ref()
    }

    /// Update sent weave after sending a message.
    /// Call this after add_pending() to track what weave the receiver will use.
    ///
    /// Derives a weave hash from the full message context (timestamp, msg_hp, plaintext).
    pub fn update_sent_for_mixing(&mut self, eagle_time: f64, msg_hp: [u8; 32], plaintext: &[u8]) {
        let weave = derive_weave_hash(eagle_time, &msg_hp, plaintext);
        self.last_sent_weave = Some(weave);
    }

    /// Process implicit ACK from their_incorporated_hp.
    /// Removes all pending messages up to and including the given msg_hp.
    /// Returns number of messages cleared.
    pub fn process_implicit_ack(&mut self, their_incorporated_hp: &[u8; 32]) -> usize {
        let mut cleared = 0;

        // Find the position of the acked message
        if let Some(ack_pos) = self
            .pending_messages
            .iter()
            .position(|m| &m.msg_hp == their_incorporated_hp)
        {
            // Remove all messages up to and including this one
            // They're in order, so we can drain 0..=ack_pos
            cleared = ack_pos + 1;
            self.pending_messages.drain(0..cleared);
        }

        cleared
    }

    /// Look up a pending message's plaintext by its msg_hp.
    /// Used by receiver to get the plaintext for bidirectional weave.
    pub fn get_pending_plaintext_by_hp(&self, msg_hp: &[u8; 32]) -> Option<&[u8]> {
        self.pending_messages
            .iter()
            .find(|m| &m.msg_hp == msg_hp)
            .map(|m| m.plaintext.as_slice())
    }

    // ==================== GAP BUFFER METHODS ====================

    /// Add a message to the gap buffer (received out of order).
    pub fn buffer_for_gap(
        &mut self,
        msg_hp: [u8; 32],
        prev_msg_hp: [u8; 32],
        sender_handle_hash: [u8; 32],
        eagle_time: f64,
        ciphertext: Vec<u8>,
    ) {
        // Don't buffer duplicates
        if self.gap_buffer.iter().any(|b| b.msg_hp == msg_hp) {
            return;
        }

        self.gap_buffer.push(BufferedMessage {
            msg_hp,
            prev_msg_hp,
            sender_handle_hash,
            eagle_time,
            ciphertext,
        });
    }

    /// Check if we have buffered messages waiting for a specific prev_msg_hp.
    /// Returns the buffered messages that can now be processed.
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
        let eagle_time = vsf::datetime_to_eagle_time(chrono::Utc::now());
        let plaintext_hash = [0xAA; 32];
        assert!(chains.advance(&alice, &eagle_time, &plaintext_hash, None));

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
}
