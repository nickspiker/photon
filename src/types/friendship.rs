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
/// Derived in two steps:
/// 1. Fast: `BLAKE3("PHOTON_CEREMONY_v1" || sorted_handle_hashes)` - deterministic input
/// 2. Slow: `handle_proof(step1)` - memory-hard (~1s) to prevent brute-force enumeration
///
/// Same value on all participants' devices - no race conditions.
/// The memory-hard step prevents attackers from enumerating handle pairs to
/// discover who is communicating with whom.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CeremonyId(pub [u8; 32]);

impl CeremonyId {
    /// Derive ceremony ID from participant handle hashes (fast step only).
    ///
    /// This is the deterministic BLAKE3 hash - use this for the pre-image
    /// that will be passed to handle_proof() for the full ceremony_id.
    ///
    /// Handle hashes are sorted for canonical ordering - the same participants
    /// will always produce the same pre-image regardless of order.
    pub fn derive_preimage(handle_hashes: &[[u8; 32]]) -> [u8; 32] {
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

    /// Derive full ceremony ID with memory-hard step (~1 second).
    ///
    /// This runs handle_proof() on the deterministic pre-image to prevent
    /// brute-force enumeration of handle pairs.
    pub fn derive(handle_hashes: &[[u8; 32]]) -> Self {
        let preimage = Self::derive_preimage(handle_hashes);
        let preimage_hash = blake3::Hash::from_bytes(preimage);
        let ceremony_id = crate::crypto::handle_proof::handle_proof(&preimage_hash);
        Self(*ceremony_id.as_bytes())
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

/// Per-participant encryption chains for a friendship.
///
/// Each participant has their own chain (8KB). When sending, use sender's chain.
/// When receiving ACK, advance sender's chain. This prevents race conditions
/// in simultaneous sends and scales to N-party conversations.
#[derive(Clone)]
pub struct FriendshipChains {
    /// Friendship ID (derived from sorted handle_hashes)
    pub friendship_id: FriendshipId,

    /// One chain per participant (sorted by handle_hash)
    chains: Vec<Chain>,

    /// Participant handle_hashes (sorted) - index matches chain index
    participants: Vec<[u8; 32]>,
}

impl FriendshipChains {
    /// Initialize chains from CLUTCH shared secrets.
    ///
    /// Uses BLAKE3 XOF to expand eggs into N × 8KB of chain material.
    pub fn from_clutch(participants: &[[u8; 32]], eggs: &[[u8; 32]]) -> Self {
        // Sort participants for canonical ordering
        let mut sorted_participants = participants.to_vec();
        sorted_participants.sort();

        // Derive friendship ID
        let friendship_id = FriendshipId::derive(&sorted_participants);

        // Calculate total bytes needed: 8KB per participant
        let chain_count = sorted_participants.len();
        let total_bytes = CHAIN_SIZE * chain_count;

        // Use BLAKE3 XOF to expand eggs into chain material
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"PHOTON_CHAIN_INIT_v1");
        for egg in eggs {
            hasher.update(egg);
        }

        // Expand to needed size
        let mut chain_bytes = vec![0u8; total_bytes];
        hasher.finalize_xof().fill(&mut chain_bytes);

        // Split into per-participant chains
        let mut chains = Vec::with_capacity(chain_count);
        for i in 0..chain_count {
            let start = i * CHAIN_SIZE;
            let end = start + CHAIN_SIZE;
            let chain =
                Chain::from_bytes(&chain_bytes[start..end]).expect("Chain size is correct");
            chains.push(chain);
        }

        Self {
            friendship_id,
            chains,
            participants: sorted_participants,
        }
    }

    /// Create from serialized data (for loading from storage).
    pub fn from_storage(
        friendship_id: FriendshipId,
        participants: Vec<[u8; 32]>,
        chain_bytes: &[u8],
    ) -> Option<Self> {
        let chain_count = participants.len();
        if chain_bytes.len() != CHAIN_SIZE * chain_count {
            return None;
        }

        let mut chains = Vec::with_capacity(chain_count);
        for i in 0..chain_count {
            let start = i * CHAIN_SIZE;
            let end = start + CHAIN_SIZE;
            let chain = Chain::from_bytes(&chain_bytes[start..end])?;
            chains.push(chain);
        }

        Some(Self {
            friendship_id,
            chains,
            participants,
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
    pub fn advance(&mut self, sender_handle_hash: &[u8; 32], plaintext_hash: &[u8; 32]) -> bool {
        if let Some(idx) = self.participant_index(sender_handle_hash) {
            self.chains[idx].advance(plaintext_hash);
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

    /// Total size in bytes (N × 8KB).
    pub fn total_size(&self) -> usize {
        CHAIN_SIZE * self.chains.len()
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
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        let eggs: Vec<[u8; 32]> = (0..8).map(|i| [i as u8; 32]).collect();

        let mut chains = FriendshipChains::from_clutch(&[alice, bob], &eggs);

        // Save original keys
        let alice_key_before = *chains.current_key(&alice).unwrap();
        let bob_key_before = *chains.current_key(&bob).unwrap();

        // Advance Alice's chain
        let plaintext_hash = [0xAA; 32];
        assert!(chains.advance(&alice, &plaintext_hash));

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

        // Deserialize
        let restored =
            FriendshipChains::from_storage(friendship_id, participants, &chain_bytes).unwrap();

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
