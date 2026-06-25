//! Result payloads from the three background CLUTCH job stages.
//!
//! Each stage runs off-thread (keypair generation, KEM encapsulation, ceremony avalanche-expand) and posts its result back over an `mpsc` channel that the UI drains in its tick. These types were extracted from the retired `src/ui/app.rs` so the active `PhotonApp` (`src/ui/photon_app.rs`) can own the CLUTCH job pipeline without importing from a module slated for deletion. The spawning + draining logic lives in `photon_app.rs`; this module is just the shared shapes.

use crate::crypto::clutch::{ClutchKemResponsePayload, ClutchKemSharedSecrets};
use crate::types::{ContactId, FriendshipChains};
use crate::crypto::clutch::ClutchAllKeypairs;
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
}
