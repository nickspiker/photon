//! Result payloads from the three background CLUTCH job stages.
//!
//! Each stage runs off-thread (keypair generation, KEM encapsulation, ceremony avalanche-expand) and posts its result back over an `mpsc` channel that the UI drains in its tick. These types were extracted from the retired `src/ui/app.rs` so the active `PhotonApp` (`src/ui/photon_app.rs`) can own the CLUTCH job pipeline without importing from a module slated for deletion. The spawning + draining logic lives in `photon_app.rs`; this module is just the shared shapes.

use crate::crypto::clutch::ClutchAllKeypairs;
use crate::crypto::clutch::{ClutchKemResponsePayload, ClutchKemSharedSecrets};
use crate::types::{ContactId, FriendshipChains};
use std::net::SocketAddr;

/// Result from background CLUTCH keypair generation (the 8 ephemeral keypairs for one ceremony).
pub struct ClutchKeygenResult {
    pub contact_id: ContactId,
    pub keypairs: ClutchAllKeypairs,
    // NOTE: ceremony_id is computed on-demand from handle_hashes + offer_provenances after enough offers arrive (2 for a 2-party DM), not in the background.
}

/// Result from background CLUTCH KEM encapsulation (the responder's reply to an offer).
pub struct ClutchKemEncapResult {
    pub contact_id: ContactId,
    pub kem_response: ClutchKemResponsePayload,
    pub local_secrets: ClutchKemSharedSecrets,
    pub ceremony_id: [u8; 32],
    pub conversation_token: [u8; 32],
    pub peer_addr: SocketAddr,
}

/// Result from background CLUTCH KEM decapsulation (opening a peer's KEM response with our 8 secret keys). The 8 PQ decapsulations ran inline in three UI-thread drain arms until 2026-08-15 — the last of the 2026-08-08 "deliberately inline" residue.
pub struct ClutchKemDecapResult {
    pub contact_id: ContactId,
    /// None = malformed material (version skew) — the drain logs and drops, matching the old inline arms.
    pub remote_secrets: Option<ClutchKemSharedSecrets>,
    /// First 8 bytes of the HQC public key of the keypair generation that decapsulated — the CAS marker. The drain drops the result when the contact's CURRENT keypairs carry a different prefix (the round torched and re-keyed while the job flew), the same identity binding the wire already uses to reject stale KEM responses.
    pub keypair_hqc_prefix: [u8; 8],
}

/// Result from background CLUTCH ceremony completion (avalanche_expand → friendship chains + proof).
pub struct ClutchCeremonyResult {
    pub contact_id: ContactId,
    pub friendship_chains: FriendshipChains,
    pub eggs_proof: [u8; 32],
    pub their_handle_hash: [u8; 32],
    pub ceremony_id: [u8; 32],
    pub conversation_token: [u8; 32],
    pub peer_addr: SocketAddr,
    pub their_hqc_prefix: [u8; 8],
    /// The durable pair secret this ceremony minted (Phase A): the post-quantum half of a fan-out wrap between these two DEVICES. Derived in the worker before the eggs drop; stored per pair by the drain. Only meaningful for a SIBLING ceremony — a friend's device never wraps our fleet key — so the drain stores it only then.
    pub fanout_pair_secret: [u8; 32],
}
