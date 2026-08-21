//! Contact status checker
//!
//! Sends UDP pings to contacts and receives pongs to determine online status. Also handles CLUTCH key ceremony messages. Uses the shared UDP socket from HandleQuery (the same port announced to FGTW).
//!
//! Protocol uses VSF-spec provenance hash for replay protection:
//! - provenance_hash = BLAKE3(sender_pubkey || timestamp_nanos)
//! - Signature covers the provenance_hash
//! - Timestamp uses nanosecond precision (ef6) for uniqueness

use super::udp;
use crate::network::fgtw::protocol::SyncRecord;
use crate::network::fgtw::FgtwMessage;
use crate::network::fgtw::Keypair;
use crate::network::pt::{
    is_pt_data, PTAck, PTComplete, PTControl, PTData, PTManager, PTNak, PTSpec,
};
use crate::types::DevicePubkey;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(not(target_os = "android"))]
use crate::ui::PhotonEvent;
#[cfg(not(target_os = "android"))]
use fluor::host::WakeSender;

/// Shared contact list - UI updates this, background thread reads it
pub type ContactPubkeys = Arc<Mutex<Vec<DevicePubkey>>>;

/// Shared sync records - UI updates this, background thread reads it for pong responses Maps conversation_token to last_received_ef6 (when we last received a message)
pub type SyncRecordsProvider = Arc<Mutex<Vec<SyncRecord>>>;

/// Shared pairwise pong-seal keys — peer DEVICE pubkey → the 32-byte key that seals/opens that device's pong sensitive tail (sync rows + name + avatar pin). The UI derives and reseeds these on ITS thread (friend: static identity DH; sibling: shared identity seed + sorted device pair) in lockstep with `ContactPubkeys`, so the RX worker holds finished per-peer keys only and the identity seed never enters this module (secret-memory hygiene). No entry for a device = its pongs go out with no sensitive tail and inbound sealed tails from it stay unopened.
pub type PongSealKeys = Arc<Mutex<std::collections::HashMap<[u8; 32], [u8; 32]>>>;

/// Pairwise pong-seal key for a FRIEND's devices: one derive over the static identity DH secret ([`crate::crypto::clutch::identity_friendship_secret`]), computable by exactly the two identity holders — the pong tail becomes end-to-end between the identities, opaque to the relay worker and every on-path observer. One key per friendship, inserted under each of the friend's device pubkeys.
pub fn friend_pong_seal_key(friendship_secret: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key("photon.pong.seal.v0", friendship_secret)
}

/// Epoch-riding sibling pong-seal key (the B-arc re-seal): binds the CURRENT epoch-spine key to the sorted device pair — same shape as [`sibling_pong_seal_key`], but the material rotates with every checkpoint, so a compromised epoch never opens past (or future) pong tails. A device whose spine lags a checkpoint reads tail-less pongs until its `ckpt_root`/`ckpt_state` catch-up lands (seconds when live; presence itself is unaffected). Pre-spine devices keep the v0 static derivation until their bootstrap mints.
pub fn sibling_pong_seal_key_epoch(
    epoch_key: &[u8; 32],
    our_device: &[u8; 32],
    their_device: &[u8; 32],
) -> [u8; 32] {
    use zeroize::Zeroize;
    let (lo, hi) = if our_device <= their_device {
        (our_device, their_device)
    } else {
        (their_device, our_device)
    };
    let mut material = [0u8; 96];
    material[..32].copy_from_slice(epoch_key);
    material[32..64].copy_from_slice(lo);
    material[64..].copy_from_slice(hi);
    let key = blake3::derive_key("photon.pong.seal.sib.v1", &material);
    material.zeroize();
    key
}

/// Pairwise pong-seal key for a fleet SIBLING device pair (self-contact devices included — they are our own fleet). Siblings share the identity seed itself (their party ids aren't curve points, so no DH exists), so the key binds the seed to the SORTED device-pubkey pair — symmetric by construction, distinct per pair. The material buffer holds the live identity seed, so it is scrubbed after the derive.
pub fn sibling_pong_seal_key(
    identity_seed: &[u8; 32],
    our_device: &[u8; 32],
    their_device: &[u8; 32],
) -> [u8; 32] {
    use zeroize::Zeroize;
    let (lo, hi) = if our_device <= their_device {
        (our_device, their_device)
    } else {
        (their_device, our_device)
    };
    let mut material = [0u8; 96];
    material[..32].copy_from_slice(identity_seed);
    material[32..64].copy_from_slice(lo);
    material[64..].copy_from_slice(hi);
    let key = blake3::derive_key("photon.pong.seal.sib.v0", &material);
    material.zeroize();
    key
}

/// Get current Eagle Time as i64 oscillations
fn eagle_time_now() -> i64 {
    vsf::eagle_time_oscillations()
}

/// Compute provenance hash = BLAKE3(sender_pubkey || timestamp_bytes)
fn compute_provenance_hash(sender_pubkey: &DevicePubkey, timestamp: i64) -> [u8; 32] {
    use blake3::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(sender_pubkey.as_bytes());
    hasher.update(&timestamp.to_le_bytes());
    *hasher.finalize().as_bytes()
}

/// Our display name as sent in pongs (the always-granted `name` slot). Written by the UI thread (init + every profile Update), read by the status thread when building each pong. One process-wide slot — the checker threads have no path back to `PhotonApp`, and the alternative (threading an `Arc<Mutex<String>>` thru both platform-specific `new()`s) buys nothing over this.
static PROFILE_NAME: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// Publish our display name for outgoing pongs. Empty = unset (the pong omits the field).
pub fn set_profile_name(name: &str) {
    if let Ok(mut n) = PROFILE_NAME.lock() {
        if *n != name {
            *n = name.to_string();
        }
    }
}

fn profile_name() -> Option<String> {
    PROFILE_NAME
        .lock()
        .ok()
        .map(|n| n.clone())
        .filter(|n| !n.is_empty())
}

/// Our avatar pin (random key ‖ lookup) as sent in pongs — the friend-gated avatar capability. Written by the UI thread on avatar set / settings load; read by the status thread per pong. Zero = unset (no avatar).
static AVATAR_PIN: std::sync::Mutex<[u8; 64]> = std::sync::Mutex::new([0u8; 64]);

/// Publish our avatar pin for outgoing pongs. Zero = unset (the pong omits it).
pub fn set_avatar_pin(pin: &[u8; 64]) {
    if let Ok(mut p) = AVATAR_PIN.lock() {
        *p = *pin;
    }
}

/// Our fleet's locked-out devices (treat-as-stolen), carried in every sealed pong tail as the reported-stolen signal. Written by the UI thread on lock edges and locked-set adopts; read by the status thread per pong.
static LOCKED_REPORT: std::sync::Mutex<Vec<[u8; 32]>> = std::sync::Mutex::new(Vec::new());

pub fn set_locked_report(locked: Vec<[u8; 32]>) {
    if let Ok(mut l) = LOCKED_REPORT.lock() {
        *l = locked;
    }
}

fn locked_report() -> Vec<[u8; 32]> {
    LOCKED_REPORT.lock().map(|l| l.clone()).unwrap_or_default()
}

fn avatar_pin() -> Option<[u8; 64]> {
    AVATAR_PIN
        .lock()
        .ok()
        .and_then(|p| if *p == [0u8; 64] { None } else { Some(*p) })
}

/// Request to ping a contact
#[derive(Clone)]
pub struct PingRequest {
    pub peer_addr: SocketAddr,
    pub peer_pubkey: DevicePubkey,
    /// Hole-punch candidate addresses to fire a probe at, alongside the ping. Empty = no punch this cycle (e.g. we already have a fresh validated path). Piggybacking on the ping cycle gives the punch a natural cadence and doubles as keepalive on a validated path.
    pub punch_candidates: Vec<SocketAddr>,
    /// Peer device keys to also send this ping to over the relay pipe (empty = direct only). Set to the peer's device list when no direct path is proven, so PRESENCE rides the relay: the peer receives the ping over its pipe, pongs back over ITS pipe, and each side flips the other online (reached_via_relay → lime-yellow). This is the presence keepalive for a relay-only contact.
    pub relay_to: Vec<[u8; 32]>,
}

// NOTE: ClutchRequest and ClutchRequestType REMOVED Full 8-primitive CLUTCH uses ClutchOfferRequest and ClutchKemResponseRequest which are handled via build_clutch_offer_vsf() and build_clutch_kem_response_vsf() See docs/clutch.md Section 4.2 for the slot-based ceremony protocol.

/// Request to send an encrypted message (CHAIN format)
#[derive(Clone)]
pub struct MessageRequest {
    pub peer_addr: SocketAddr,
    /// Second candidate address to race, from `race_addrs()` — the public/WAN path when `peer_addr` is the peer's LAN IPv4 (or vice versa). The wire bytes go to BOTH so the reachable one wins: a cellular peer can't reach the other's `192.168.x` LAN, and two peers on different LANs can only meet on the public path. Chat used to drop this (send LAN-only), which silently blackholed every message to an off-LAN peer even though CLUTCH — which already races both — completed fine.
    pub alt_addr: Option<SocketAddr>,
    /// Recipient's device pubkey (for relay fallback)
    pub recipient_pubkey: [u8; 32],
    /// Privacy-preserving conversation token (smear_hash of sorted participant seeds). Replaces cleartext handle_hash and friendship_id - only participants can compute.
    pub conversation_token: [u8; 32],
    /// The sending lane's label (docs/lanes.md) — rides every frame; the receiver derives the decrypting lane from it.
    pub lane: [u8; 32],
    /// Hash chain link to previous message (or first_message_anchor)
    pub prev_msg_hp: [u8; 32],
    /// Encrypted message content
    pub ciphertext: Vec<u8>,
    /// Eagle time oscillations used for encryption - MUST match for decryption The nonce is derived from this, so sender and receiver must use identical value
    pub eagle_time: i64,
    /// Peer device keys to also send this message to over the relay pipe (empty = direct only). Set to the peer's device list when no direct path is proven, so CHAT rides the relay identically to how CLUTCH already does.
    pub relay_to: Vec<[u8; 32]>,
}

/// Request to send a message acknowledgment (CHAIN format)
#[derive(Clone)]
pub struct AckRequest {
    pub peer_addr: SocketAddr,
    /// Recipient's device pubkey (for relay fallback)
    pub recipient_pubkey: [u8; 32],
    /// Privacy-preserving conversation token (smear_hash of sorted participant seeds). Replaces cleartext handle_hash - only participants can compute.
    pub conversation_token: [u8; 32],
    /// Eagle time oscillations of the message being ACKed (i64 from their VSF header)
    pub acked_eagle_time: i64,
    /// Hash of the decrypted plaintext - proves we decrypted their message
    pub plaintext_hash: [u8; 32],
    /// Peer device keys to also send this ACK to over the relay pipe (empty = direct only). Set to the peer's device list when no direct path is proven, so the chat ACK returns over the relay and clears the sender's message-layer retransmit.
    pub relay_to: Vec<[u8; 32]>,
}

/// Request to send an AvatarRequest to this peer asking for their avatar.
#[derive(Clone)]
pub struct AvatarRequestSend {
    pub peer_addr: SocketAddr,
    pub recipient_pubkey: [u8; 32],
}

/// Request to send my avatar back to this peer.
#[derive(Clone)]
pub struct AvatarResponseSend {
    pub peer_addr: SocketAddr,
    pub recipient_pubkey: [u8; 32],
    pub avatar_vsf: Vec<u8>,
}

/// Request to send a pre-built, signed history frame (hist_req or hist_page). Bytes are built on the UI thread (which owns device_secret + the vault); this thread just races them down both paths.
#[derive(Clone)]
pub struct HistorySendRequest {
    pub peer_addr: SocketAddr,
    /// Second candidate raced alongside (same LAN/WAN reasoning as MessageRequest::alt_addr).
    pub alt_addr: Option<SocketAddr>,
    /// Recipient's device pubkey (for relay fallback).
    pub recipient_pubkey: [u8; 32],
    /// Pre-built + signed hist_req / hist_page VSF bytes.
    pub vsf_bytes: Vec<u8>,
    /// Devices to ALSO send the whole frame to over the relay pipe (same rule as MessageRequest::relay_to: filled when no validated direct path, or when answering a request that itself arrived over the relay). PT's own relay fallback needs ~31s of failed retries to engage — longer than the requester's expiry — so a relay-only pair starved forever waiting on a ladder that never completed (live-pair history recovery, 2026-07-24).
    pub relay_to: Vec<[u8; 32]>,
}

/// Request to start a PT large transfer (e.g., full CLUTCH offer with all 8 pubkeys)
#[derive(Clone)]
pub struct PTSendRequest {
    pub peer_addr: SocketAddr,
    pub data: Vec<u8>,
}

/// Request to send full CLUTCH offer (~548KB) via TCP fallback
///
/// Uses pre-built VSF bytes from build_clutch_offer_vsf(). The caller builds the VSF to capture the offer_provenance (hp field).
#[derive(Clone)]
pub struct ClutchOfferRequest {
    pub peer_addr: SocketAddr, // Primary path (LAN-preferred); port comes from FGTW (peer's photon_port)
    pub alt_addr: Option<SocketAddr>, // Alternate path raced alongside (WAN) — see PtManager::send_with_pubkey_and_alt
    pub vsf_bytes: Vec<u8>,           // Pre-built and signed VSF message
    pub recipient_pubkey: [u8; 32], // Peer's primary device — PT's own retry-threshold relay fallback stores under relay/{recipient}/
    pub relay_to: Vec<[u8; 32]>, // Store on the FGTW relay for EACH of these peer devices in parallel (empty = don't relay). Set to the peer's full device list when no direct path is proven (asymmetric reachability): the direct transfer keeps getting cancelled on address churn before it could reach PT's own fallback, and we can't tell which of a multi-device peer's phones is polling, so we address them all.
}

/// Request to send CLUTCH KEM response (~31KB) via TCP fallback
///
/// Uses VSF format with proper signing and verification. See protocol.rs build_clutch_kem_response_vsf() for format details.
#[derive(Clone)]
pub struct ClutchKemResponseRequest {
    pub peer_addr: SocketAddr, // Primary path (LAN-preferred); port comes from FGTW (peer's photon_port)
    pub alt_addr: Option<SocketAddr>, // Alternate path raced alongside (WAN)
    pub conversation_token: [u8; 32], // Privacy-preserving smear_hash of sorted participant seeds
    pub ceremony_id: [u8; 32], // Deterministic from sorted handle_hashes
    pub payload: crate::crypto::clutch::ClutchKemResponsePayload,
    pub device_pubkey: [u8; 32],
    pub device_secret: [u8; 32],    // For signing (zeroize after use)
    pub recipient_pubkey: [u8; 32], // Peer primary device (PT fallback)
    pub relay_to: Vec<[u8; 32]>,    // Relay for each of these peer devices (empty = no relay)
}

/// Request to send CLUTCH complete proof (~200 bytes) via TCP fallback
///
/// Uses VSF format with proper signing and verification. See protocol.rs build_clutch_complete_vsf() for format details.
#[derive(Clone)]
pub struct ClutchCompleteRequest {
    pub peer_addr: SocketAddr, // Primary path (LAN-preferred); port comes from FGTW (peer's photon_port)
    pub alt_addr: Option<SocketAddr>, // Alternate path raced alongside (WAN)
    pub conversation_token: [u8; 32], // Privacy-preserving smear_hash of sorted participant seeds
    pub ceremony_id: [u8; 32], // Deterministic from sorted handle_hashes
    pub payload: crate::crypto::clutch::ClutchCompletePayload,
    pub device_pubkey: [u8; 32],
    pub device_secret: [u8; 32],    // For signing (zeroize after use)
    pub recipient_pubkey: [u8; 32], // Peer primary device (PT fallback)
    pub relay_to: Vec<[u8; 32]>,    // Relay for each of these peer devices (empty = no relay)
}

/// Request to broadcast presence on LAN for local peer discovery Solves NAT hairpinning - when peers are on same LAN, use local IPs
#[derive(Clone)]
pub struct LanBroadcastRequest {
    pub our_handle_proof: [u8; 32],
    pub our_port: u16, // Port we're listening on
}

/// Request to clear pending PT sends for a peer (e.g., when CLUTCH completes) Prevents wasteful retransmission of offers/KEM responses after ceremony is done.
#[derive(Clone)]
pub struct ClearPtSendsRequest {
    pub peer_addr: SocketAddr,
}

// Use global PHOTON_PORT for all network communication
use crate::PHOTON_PORT;

/// Status update from the checker
#[derive(Clone, Debug)]
pub enum StatusUpdate {
    /// Online/offline status change
    Online {
        peer_pubkey: DevicePubkey,
        is_online: bool,
        peer_addr: Option<std::net::SocketAddr>,
        /// Sync records from pong: (conversation_token, last_received_ef6) Tells us which messages the peer has received, for retransmit logic
        sync_records: Vec<SyncRecord>,
        /// The peer's chosen display name from the pong (always-granted name slot). None on pings/timeouts/legacy pongs — receiver keeps its stored value.
        display_name: Option<String>,
        /// The peer's avatar pin from the pong (friend-gated key ‖ lookup). None on pings/timeouts/legacy pongs.
        avatar_pin: Option<[u8; 64]>,
        /// The peer fleet's REPORTED-STOLEN devices, from the sealed pong tail (empty on legacy/tail-less pongs). The UI thread applies the two-distinct-reporters threshold before refusing anything.
        locked_reports: Vec<[u8; 32]>,
    },
    // NOTE: ClutchOffer, ClutchInit, ClutchResponse, ClutchComplete REMOVED Full 8-primitive CLUTCH uses ClutchOfferReceived and ClutchKemResponseReceived See docs/clutch.md Section 4.2 for the slot-based ceremony protocol.
    /// Encrypted chat message received (CHAIN format)
    ChatMessage {
        /// Privacy-preserving conversation token (smear_hash of sorted participant seeds)
        conversation_token: [u8; 32],
        /// The sender's lane label — names the decrypting lane (docs/lanes.md).
        lane: [u8; 32],
        /// Hash chain link to previous message
        prev_msg_hp: [u8; 32],
        /// Encrypted message content
        ciphertext: Vec<u8>,
        /// Eagle time oscillations from VSF header (for ACK matching)
        timestamp: i64,
        sender_addr: SocketAddr,
        /// The signing device — carried so the UI thread can apply the full known∧not-refused gate (refused_devices/locked_out live there, not in the RX worker).
        sender_pubkey: DevicePubkey,
    },
    /// Sibling chain-reset frame received (fork repair): the fleet-sealed nonce blob rides opaque to the UI thread, which holds the fleet key + chains.
    ChainResetReceived {
        conversation_token: [u8; 32],
        sealed: Vec<u8>,
        sender_pubkey: DevicePubkey,
        sender_addr: SocketAddr,
    },
    /// A sealed pong tail failed to open (no pairwise key for that device yet) — the UI thread reseeds the pong-seal map (rate-limited): on a freshly-restored device the map fills in fold/roster order, and a pong racing ahead of the reseed walk stayed tail-less forever (names + avatar pins ride the tail — the blank-restore of 2026-07-26).
    PongSealMissing { device: DevicePubkey },
    /// Fleet chain-state replication (chain_sync): a sibling's epoch-sealed chains snapshot, opaque to this layer — the UI thread opens with the chain_sync key of `epoch_k` (accepting k and k−1), decodes, and adopts iff its mutated_osc is newer than the local copy's.
    ChainSyncReceived {
        conversation_token: [u8; 32],
        epoch_k: u64,
        sealed: Vec<u8>,
        sender_pubkey: DevicePubkey,
    },
    /// A checkpoint minter's settled-root hand-off for epoch `k`, sealed under the PRIOR epoch's ckpt_root key — the UI thread opens it (open success under a member-only key IS the authentication), derives epoch_k, and reconciles the chain's commitment on the next refold.
    CkptRootReceived {
        k: u64,
        fanout_epoch: u64,
        sealed: Vec<u8>,
        sender_pubkey: DevicePubkey,
    },
    /// A sibling's "my spine ends at have_k, serve me forward" — the UI thread answers with a fleet-key-sealed ckpt_state if it is ahead.
    /// A sibling's active-clearer claim (or its retraction) for a conversation — the fleet-wide notification suppressor. Newest osc wins in the drain.
    FocusClaimReceived {
        conversation_token: [u8; 32],
        osc: i64,
        active: bool,
        sender_pubkey: DevicePubkey,
    },
    /// A sibling announcing it holds fleet ATTENTION — the human's newest input is there. Newest osc wins in the drain; both ding gates require holding attention.
    AttentionReceived {
        osc: i64,
        sender_pubkey: DevicePubkey,
    },
    CkptReqReceived {
        have_k: u64,
        sender_pubkey: DevicePubkey,
        sender_addr: SocketAddr,
    },
    /// A sibling's whole epoch state (k ‖ epoch ‖ prev), fleet-key-sealed — the UI thread adopts it if it is ahead of the local spine.
    CkptStateReceived {
        k: u64,
        sealed: Vec<u8>,
        sender_pubkey: DevicePubkey,
    },
    /// Attachment blob arrived over PT (signature verified; sealed under the friendship history key or the fleet key — the UI picks by sender and verifies the content hash after opening).
    AttachBlobReceived {
        conversation_token: [u8; 32],
        content_hash: [u8; 32],
        sealed: Vec<u8>,
        sender_pubkey: DevicePubkey,
        sender_addr: SocketAddr,
    },
    /// Live PT transfer progress (throttled ~500ms): (peer, done, total, outbound) per active sharded transfer. Drives the attachment progress bar.
    AttachProgress(Vec<(SocketAddr, u32, u32, bool)>),
    /// A receiver confirmed an attachment blob arrived + verified + stored — flips the sender's pill to delivered.
    AttachHaveReceived {
        content_hash: [u8; 32],
        sender_pubkey: DevicePubkey,
    },
    /// A peer wants the blob for an attachment row it holds (offline race, or a fleet sibling with row-but-no-blob). The UI answers with an attach_blob if the blob is held.
    AttachReqReceived {
        conversation_token: [u8; 32],
        content_hash: [u8; 32],
        sender_pubkey: DevicePubkey,
        sender_addr: SocketAddr,
    },
    /// BRIDGE: a remote-terminal frame arrived (open/data/resize/close/exit/nuke). The payload is still fleet-sealed — the UI opens it, authorizes the signer as a sibling + checks the host opt-in, and drives the PTY host.
    TermReceived {
        session_id: [u8; 16],
        kind: u8,
        sealed_payload: Vec<u8>,
        sender_pubkey: DevicePubkey,
        sender_addr: SocketAddr,
    },
    /// Message acknowledgment received (CHAIN format)
    MessageAck {
        /// Privacy-preserving conversation token (smear_hash of sorted participant seeds)
        conversation_token: [u8; 32],
        /// Eagle time oscillations of the message being ACKed
        acked_eagle_time: i64,
        /// BLAKE3 hash of decrypted plaintext - proves they decrypted our message
        plaintext_hash: [u8; 32],
    },
    /// Avatar request received from a peer - they want our avatar (verified signature)
    AvatarRequestReceived {
        sender_pubkey: DevicePubkey,
        sender_addr: SocketAddr,
    },
    /// Avatar received from a peer in response to our request (verified signature)
    AvatarReceived {
        responder_pubkey: DevicePubkey,
        avatar_vsf: Vec<u8>,
        sender_addr: SocketAddr,
    },
    /// History request received (signature verified; per-contact authorization happens on the UI thread, which owns the contacts + vault).
    HistoryRequestReceived {
        conversation_token: [u8; 32],
        /// Cursor: serve rows strictly older than this (i64::MAX = head page).
        before_osc: i64,
        limit: u32,
        request_id: [u8; 32],
        /// Header creation time — the UI's staleness check.
        sent_osc: i64,
        sender_pubkey: DevicePubkey,
        sender_addr: SocketAddr,
    },
    /// History page received (signature verified; blob is AEAD-sealed — the UI opens it with the friendship history key, or with the epoch hist_page key when `epoch_k` rides the frame: the fleet route).
    HistoryPageReceived {
        conversation_token: [u8; 32],
        request_id: [u8; 32],
        epoch_k: Option<u64>,
        sealed: Vec<u8>,
        sender_pubkey: DevicePubkey,
        sender_addr: SocketAddr,
    },
    /// One of the four blind frames received (blind_put/ack/get/srv — the friend-blinded private-identity-secret S plumbing; signature verified, UI authorizes per-contact and dispatches on `kind`).
    BlindFrameReceived {
        kind: crate::network::fgtw::protocol::BlindFrameKind,
        conversation_token: [u8; 32],
        request_id: [u8; 32],
        /// The 64-byte blind blob (put, srv-hit); empty otherwise.
        blob: Vec<u8>,
        /// srv only: whether the friend held a deposit for the requesting device.
        found: bool,
        /// Header creation time — the UI's staleness check.
        sent_osc: i64,
        sender_pubkey: DevicePubkey,
        sender_addr: SocketAddr,
    },
    /// PT large transfer completed - received data from peer
    PTReceived {
        peer_addr: SocketAddr,
        data: Vec<u8>,
    },
    /// PT outbound transfer completed successfully
    PTSendComplete { peer_addr: SocketAddr },
    /// Full CLUTCH offer received (~548KB with all 8 pubkeys) Payload is already verified and parsed from VSF format.
    ClutchOfferReceived {
        conversation_token: [u8; 32], // Privacy-preserving smear_hash of sorted participant seeds
        offer_provenance: [u8; 32],   // VSF header hp - unique per offer (timestamp entropy)
        sender_pubkey: [u8; 32],      // Device pubkey (verified via signature)
        payload: crate::crypto::clutch::ClutchOfferPayload,
        sender_addr: SocketAddr,
    },
    /// CLUTCH KEM response received (~31KB with 4 ciphertexts) Payload is already verified and parsed from VSF format.
    ClutchKemResponseReceived {
        conversation_token: [u8; 32], // Privacy-preserving smear_hash of sorted participant seeds
        ceremony_id: [u8; 32],        // Deterministic - should match locally computed value
        sender_pubkey: [u8; 32],      // Device pubkey (verified via signature)
        payload: crate::crypto::clutch::ClutchKemResponsePayload,
        sender_addr: SocketAddr,
    },
    /// CLUTCH complete proof received (~200 bytes with eggs_proof) Payload is already verified and parsed from VSF format. Both parties exchange this to verify they derived identical eggs.
    ClutchCompleteReceived {
        conversation_token: [u8; 32], // Privacy-preserving smear_hash of sorted participant seeds
        ceremony_id: [u8; 32],        // Deterministic - should match locally computed value
        sender_pubkey: [u8; 32],      // Device pubkey (verified via signature)
        payload: crate::crypto::clutch::ClutchCompletePayload,
        sender_addr: SocketAddr,
    },
    /// LAN peer discovered via broadcast (NAT hairpinning workaround)
    LanPeerDiscovered {
        /// The beaconing DEVICE (the frame's `ke` field) — present on every current beacon; None only for pre-ke frames. This is what lets an own-handle beacon mean something: it names WHICH device of the fleet (or which joining candidate) is on this LAN.
        device_pubkey: Option<[u8; 32]>,
        handle_proof: [u8; 32],
        local_ip: Ipv4Addr,
        port: u16,
    },
    /// Our own reflexive (public) address, learned+adopted from peer-echoed reflection (pong `observed_addr` or a `ReflectResponse`). The app stores it as `PhotonApp.our_reflexive`, feeding candidate gathering and the FGTW announce (so our published address is the one seen on the live UDP data socket, not fgtw.org's cone-only TLS view).
    ReflexiveLearned { addr: SocketAddr },
    /// Our own LAN address, learned from our OWN looped-back discovery beacon: its SOURCE address is kernel truth for the interface the beacon actually left on. This is the LAN counterpart of `ReflexiveLearned`, and the fix for the multi-homed hole `get_local_ip` falls into — the routing trick asks which interface reaches the INTERNET, and a phone routing internet over cellular answers with the CLAT/CGNAT interface while its Wi-Fi holds the real LAN address (published record then carried no LAN entry; the peer probed only an unreachable WAN and parked on relay, 2026-08-11).
    OurLanAddrObserved { ip: Ipv4Addr },
    /// A hole-punch to `peer_pubkey` round-tripped: `remote` is a validated direct path. The app records it on the matching contact's `validated_path`, so `race_addrs` prefers it. `peer_pubkey` may be any device in the friend's fleet (match via `Contact::knows_device`).
    PathValidated {
        peer_pubkey: DevicePubkey,
        remote: SocketAddr,
    },
}

impl StatusUpdate {
    /// The verified authoring DEVICE of this update, when it carries one. The app's drain drops any update authored by OUR OWN device before an arm can touch it: a frame we sent can arrive back at us (relay echo, LAN multicast loopback, a send aimed at an endpoint already poisoned to our own address), and every receive arm trusts its sender enough to adopt endpoints, addresses and liveness from it — processing an own frame as peer traffic is how a sibling contact elected US its active device and a ceremony spent a day offering at itself (field, 2026-08-12).
    pub fn sender_device(&self) -> Option<&[u8; 32]> {
        match self {
            StatusUpdate::Online { peer_pubkey, .. } => Some(peer_pubkey.as_bytes()),
            StatusUpdate::ChatMessage { sender_pubkey, .. } => Some(sender_pubkey.as_bytes()),
            StatusUpdate::ChainResetReceived { sender_pubkey, .. } => {
                Some(sender_pubkey.as_bytes())
            }
            StatusUpdate::PongSealMissing { device } => Some(device.as_bytes()),
            StatusUpdate::ChainSyncReceived { sender_pubkey, .. } => Some(sender_pubkey.as_bytes()),
            StatusUpdate::CkptRootReceived { sender_pubkey, .. } => Some(sender_pubkey.as_bytes()),
            StatusUpdate::CkptReqReceived { sender_pubkey, .. } => Some(sender_pubkey.as_bytes()),
            StatusUpdate::FocusClaimReceived { sender_pubkey, .. } => {
                Some(sender_pubkey.as_bytes())
            }
            StatusUpdate::AttentionReceived { sender_pubkey, .. } => {
                Some(sender_pubkey.as_bytes())
            }
            StatusUpdate::CkptStateReceived { sender_pubkey, .. } => Some(sender_pubkey.as_bytes()),
            StatusUpdate::AttachBlobReceived { sender_pubkey, .. } => {
                Some(sender_pubkey.as_bytes())
            }
            StatusUpdate::AttachHaveReceived { sender_pubkey, .. } => {
                Some(sender_pubkey.as_bytes())
            }
            StatusUpdate::AttachReqReceived { sender_pubkey, .. } => Some(sender_pubkey.as_bytes()),
            StatusUpdate::AvatarRequestReceived { sender_pubkey, .. } => {
                Some(sender_pubkey.as_bytes())
            }
            StatusUpdate::AvatarReceived {
                responder_pubkey, ..
            } => Some(responder_pubkey.as_bytes()),
            StatusUpdate::HistoryRequestReceived { sender_pubkey, .. } => {
                Some(sender_pubkey.as_bytes())
            }
            StatusUpdate::HistoryPageReceived { sender_pubkey, .. } => {
                Some(sender_pubkey.as_bytes())
            }
            StatusUpdate::BlindFrameReceived { sender_pubkey, .. } => {
                Some(sender_pubkey.as_bytes())
            }
            StatusUpdate::ClutchOfferReceived { sender_pubkey, .. } => Some(sender_pubkey),
            StatusUpdate::ClutchKemResponseReceived { sender_pubkey, .. } => Some(sender_pubkey),
            StatusUpdate::ClutchCompleteReceived { sender_pubkey, .. } => Some(sender_pubkey),
            StatusUpdate::PathValidated { peer_pubkey, .. } => Some(peer_pubkey.as_bytes()),
            _ => None,
        }
    }
}

/// Pending ping waiting for pong
struct PendingPing {
    recipient_pubkey: DevicePubkey,
    provenance_hash: [u8; 32],
    sent_at: Instant,
}

/// Contact status checker
///
/// Spawns a background thread to handle async UDP ping/pong and CLUTCH messages. Uses the shared UDP socket from HandleQuery. For large CLUTCH payloads, uses TCP fallback (raw254 not yet implemented).
pub struct StatusChecker {
    /// While true, every direct ping carries a `Reflect` beside it — the UDP-observed self-discovery bootstrap. The app clears it on the first quorum-adopted ReflexiveLearned and re-arms it on a LAN-interface change; default TRUE because a fresh process never has a UDP-confirmed mapping (the announce otherwise publishes the self-claimed bind port, which no NAT honours — field 2026-08-13).
    needs_reflect: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ping_sender: Sender<PingRequest>,
    // NOTE: clutch_sender removed - legacy v1 CLUTCH no longer used
    message_sender: Sender<MessageRequest>,
    ack_sender: Sender<AckRequest>,
    avatar_request_sender: Sender<AvatarRequestSend>,
    avatar_response_sender: Sender<AvatarResponseSend>,
    history_sender: Sender<HistorySendRequest>,
    pt_sender: Sender<PTSendRequest>,
    offer_sender: Sender<ClutchOfferRequest>,
    kem_response_sender: Sender<ClutchKemResponseRequest>,
    complete_proof_sender: Sender<ClutchCompleteRequest>,
    lan_broadcast_sender: Sender<LanBroadcastRequest>,
    clear_pt_sender: Sender<ClearPtSendsRequest>,
    status_receiver: Receiver<StatusUpdate>,
    /// Fire a phonebook-gossip request at a reachable peer (its address). The peer replies with the self-signed peer records it holds, so a device whose own fgtw is unreachable can still learn a friend's address from a friend it CAN reach. Not a relay — only routing records (each independently verifiable) travel, never payload.
    phonebook_req_sender: Sender<SocketAddr>,
}

impl StatusChecker {
    /// Create a new status checker using a shared socket (Desktop version with a fluor wake sender)
    ///
    /// `socket` is the shared UDP socket from HandleQuery (same port announced to FGTW). `keypair` is the device keypair (same one used for FGTW registration). `contacts` is shared with UI - only respond to pings from pubkeys in this list. `sync_records` is shared with UI - provides last_received_ef6 for each conversation. `pong_seal_keys` is shared with UI - the pairwise keys that seal/open each pong's sensitive tail. `event_proxy` is the fluor `WakeSender` used to wake the UI thread when network data arrives (was winit's `EventLoopProxy` pre-migration; HandleQuery took the same path).
    #[cfg(not(target_os = "android"))]
    pub fn new(
        socket: Arc<UdpSocket>,
        keypair: Keypair,
        contacts: ContactPubkeys,
        sync_records: SyncRecordsProvider,
        pong_seal_keys: PongSealKeys,
        event_proxy: Arc<dyn WakeSender<PhotonEvent>>,
        peer_store: Arc<Mutex<crate::network::fgtw::PeerStore>>,
    ) -> Result<Self, String> {
        let needs_reflect = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let needs_reflect_loop = needs_reflect.clone();
        let (ping_tx, ping_rx) = channel::<PingRequest>();
        let (message_tx, message_rx) = channel::<MessageRequest>();
        let (ack_tx, ack_rx) = channel::<AckRequest>();
        let (avatar_request_tx, avatar_request_rx) = channel::<AvatarRequestSend>();
        let (avatar_response_tx, avatar_response_rx) = channel::<AvatarResponseSend>();
        let (history_tx, history_rx) = channel::<HistorySendRequest>();
        let (pt_tx, pt_rx) = channel::<PTSendRequest>();
        let (offer_tx, offer_rx) = channel::<ClutchOfferRequest>();
        let (kem_response_tx, kem_response_rx) = channel::<ClutchKemResponseRequest>();
        let (complete_proof_tx, complete_proof_rx) = channel::<ClutchCompleteRequest>();
        let (lan_broadcast_tx, lan_broadcast_rx) = channel::<LanBroadcastRequest>();
        let (clear_pt_tx, clear_pt_rx) = channel::<ClearPtSendsRequest>();
        let (status_tx, status_rx) = channel::<StatusUpdate>();
        let (phonebook_req_tx, phonebook_req_rx) = channel::<SocketAddr>();

        let our_pubkey = DevicePubkey::from_bytes(keypair.public.to_bytes());

        // Log which port we're using
        let local_addr = socket
            .local_addr()
            .map_err(|e| format!("Failed to get local addr: {}", e))?;
        crate::logf!("Status: Using socket on port {}", local_addr.port());

        socket
            .set_nonblocking(true)
            .map_err(|e| format!("Failed to set non-blocking: {}", e))?;

        // Get local IP for TCP listener (and LAN discovery) Use connect-to-external trick to find actual LAN IP (not 0.0.0.0)
        let local_ip = udp::get_local_ip().unwrap_or(Ipv4Addr::new(0, 0, 0, 0));

        let thread_body = move || {
            crate::log("Status: Background thread started");
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime for StatusChecker");

            rt.block_on(async move {
                run_checker(
                    socket,
                    keypair,
                    our_pubkey,
                    local_ip,
                    ping_rx,
                    message_rx,
                    ack_rx,
                    avatar_request_rx,
                    avatar_response_rx,
                    history_rx,
                    pt_rx,
                    offer_rx,
                    kem_response_rx,
                    complete_proof_rx,
                    lan_broadcast_rx,
                    clear_pt_rx,
                    status_tx,
                    contacts,
                    sync_records,
                    pong_seal_keys,
                    Some(event_proxy),
                    phonebook_req_rx,
                    peer_store,
                    needs_reflect_loop,
                )
                .await;
            });
        };

        #[cfg(not(target_os = "redox"))]
        {
            use thread_priority::{ThreadBuilderExt, ThreadPriority};
            thread::Builder::new()
                .name("network-status".to_string())
                .spawn_with_priority(ThreadPriority::Max, move |_| thread_body())
                .expect("Failed to spawn network thread");
        }
        #[cfg(target_os = "redox")]
        {
            thread::Builder::new()
                .name("network-status".to_string())
                .spawn(thread_body)
                .expect("Failed to spawn network thread");
        }

        Ok(Self {
            needs_reflect,
            ping_sender: ping_tx,
            message_sender: message_tx,
            ack_sender: ack_tx,
            avatar_request_sender: avatar_request_tx,
            avatar_response_sender: avatar_response_tx,
            history_sender: history_tx,
            pt_sender: pt_tx,
            offer_sender: offer_tx,
            kem_response_sender: kem_response_tx,
            complete_proof_sender: complete_proof_tx,
            lan_broadcast_sender: lan_broadcast_tx,
            clear_pt_sender: clear_pt_tx,
            status_receiver: status_rx,
            phonebook_req_sender: phonebook_req_tx,
        })
    }

    /// Create a new status checker using a shared socket (Android version - no EventLoopProxy)
    #[cfg(target_os = "android")]
    pub fn new(
        socket: Arc<UdpSocket>,
        keypair: Keypair,
        contacts: ContactPubkeys,
        sync_records: SyncRecordsProvider,
        pong_seal_keys: PongSealKeys,
        peer_store: Arc<Mutex<crate::network::fgtw::PeerStore>>,
    ) -> Result<Self, String> {
        let needs_reflect = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let needs_reflect_loop = needs_reflect.clone();
        let (ping_tx, ping_rx) = channel::<PingRequest>();
        let (message_tx, message_rx) = channel::<MessageRequest>();
        let (ack_tx, ack_rx) = channel::<AckRequest>();
        let (avatar_request_tx, avatar_request_rx) = channel::<AvatarRequestSend>();
        let (avatar_response_tx, avatar_response_rx) = channel::<AvatarResponseSend>();
        let (history_tx, history_rx) = channel::<HistorySendRequest>();
        let (pt_tx, pt_rx) = channel::<PTSendRequest>();
        let (offer_tx, offer_rx) = channel::<ClutchOfferRequest>();
        let (kem_response_tx, kem_response_rx) = channel::<ClutchKemResponseRequest>();
        let (complete_proof_tx, complete_proof_rx) = channel::<ClutchCompleteRequest>();
        let (lan_broadcast_tx, lan_broadcast_rx) = channel::<LanBroadcastRequest>();
        let (clear_pt_tx, clear_pt_rx) = channel::<ClearPtSendsRequest>();
        let (status_tx, status_rx) = channel::<StatusUpdate>();
        let (phonebook_req_tx, phonebook_req_rx) = channel::<SocketAddr>();

        let our_pubkey = DevicePubkey::from_bytes(keypair.public.to_bytes());

        // Log which port we're using
        let local_addr = socket
            .local_addr()
            .map_err(|e| format!("Failed to get local addr: {}", e))?;
        crate::logf!("Status: Using socket on port {}", local_addr.port());

        socket
            .set_nonblocking(true)
            .map_err(|e| format!("Failed to set non-blocking: {}", e))?;

        // Get local IP for TCP listener (and LAN discovery)
        let local_ip = udp::get_local_ip().unwrap_or(Ipv4Addr::new(0, 0, 0, 0));

        let thread_body = move || {
            crate::log("Status: Background thread started");
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime for StatusChecker");

            rt.block_on(async move {
                run_checker(
                    socket,
                    keypair,
                    our_pubkey,
                    local_ip,
                    ping_rx,
                    message_rx,
                    ack_rx,
                    avatar_request_rx,
                    avatar_response_rx,
                    history_rx,
                    pt_rx,
                    offer_rx,
                    kem_response_rx,
                    complete_proof_rx,
                    lan_broadcast_rx,
                    clear_pt_rx,
                    status_tx,
                    contacts,
                    sync_records,
                    pong_seal_keys,
                    None,
                    phonebook_req_rx,
                    peer_store,
                    needs_reflect_loop,
                )
                .await;
            });
        };

        #[cfg(not(target_os = "redox"))]
        {
            use thread_priority::{ThreadBuilderExt, ThreadPriority};
            thread::Builder::new()
                .name("network-status".to_string())
                .spawn_with_priority(ThreadPriority::Max, move |_| thread_body())
                .expect("Failed to spawn network thread");
        }
        #[cfg(target_os = "redox")]
        {
            thread::Builder::new()
                .name("network-status".to_string())
                .spawn(thread_body)
                .expect("Failed to spawn network thread");
        }

        Ok(Self {
            needs_reflect,
            ping_sender: ping_tx,
            message_sender: message_tx,
            ack_sender: ack_tx,
            avatar_request_sender: avatar_request_tx,
            avatar_response_sender: avatar_response_tx,
            history_sender: history_tx,
            pt_sender: pt_tx,
            offer_sender: offer_tx,
            kem_response_sender: kem_response_tx,
            complete_proof_sender: complete_proof_tx,
            lan_broadcast_sender: lan_broadcast_tx,
            clear_pt_sender: clear_pt_tx,
            status_receiver: status_rx,
            phonebook_req_sender: phonebook_req_tx,
        })
    }

    /// Request to ping a contact (non-blocking). `relay_to` = peer device keys to also ping over the relay pipe (empty = direct only); set when no direct path is proven so presence works for a relay-only peer.
    /// Flip the reflect-beside-pings bootstrap (see `needs_reflect`). The app clears it on the first quorum-adopted ReflexiveLearned and re-arms it when the LAN interface changes.
    pub fn set_reflect_needed(&self, needed: bool) {
        self.needs_reflect
            .store(needed, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn ping(
        &self,
        peer_addr: SocketAddr,
        peer_pubkey: DevicePubkey,
        punch_candidates: Vec<SocketAddr>,
        relay_to: Vec<[u8; 32]>,
    ) {
        let _ = self.ping_sender.send(PingRequest {
            peer_addr,
            peer_pubkey,
            punch_candidates,
            relay_to,
        });
    }

    // NOTE: send_clutch() removed - legacy v1 CLUTCH no longer used

    /// Send an encrypted message (non-blocking)
    pub fn send_message(&self, request: MessageRequest) {
        let _ = self.message_sender.send(request);
    }

    /// Send a message acknowledgment (non-blocking)
    pub fn send_ack(&self, request: AckRequest) {
        let _ = self.ack_sender.send(request);
    }

    /// Send an AvatarRequest to a peer asking for their avatar (non-blocking)
    pub fn send_avatar_request(&self, request: AvatarRequestSend) {
        let _ = self.avatar_request_sender.send(request);
    }

    /// Send my avatar back to a peer (non-blocking)
    pub fn send_avatar_response(&self, request: AvatarResponseSend) {
        let _ = self.avatar_response_sender.send(request);
    }

    /// Send a pre-built history frame (hist_req or hist_page) to a peer (non-blocking)
    pub fn send_history(&self, request: HistorySendRequest) {
        let _ = self.history_sender.send(request);
    }

    /// A cloneable handle for dispatching history pages from a WORKER thread. Serving a page means reading and decrypting up to 50 vault rows and sealing them — measured at 2.2s on the UI thread, which is what a peer's backfill request felt from the inside. The work moves off the render loop; only this sender needs to travel with it.
    pub fn history_dispatch(&self) -> Sender<HistorySendRequest> {
        self.history_sender.clone()
    }

    /// A cloneable handle for firing ACKs from a worker thread — the chains writer sends each receive's ACK only after its durable write lands (durable-then-signal).
    pub fn ack_dispatch(&self) -> Sender<AckRequest> {
        self.ack_sender.clone()
    }

    /// A cloneable handle for dispatching chat frames from a worker thread — the chains writer transmits each send only after its durable write lands (durable-then-signal).
    pub fn message_dispatch(&self) -> Sender<MessageRequest> {
        self.message_sender.clone()
    }

    /// Start a PT large transfer (non-blocking) Ask a reachable peer (by address) for the peer records it holds — phonebook gossip. Used when our own fgtw is unreachable but a friend is: they answer with self-signed records that merge into the shared peer store, so a friend we can't reach gets learned from one we can.
    pub fn send_phonebook_request(&self, addr: SocketAddr) {
        let _ = self.phonebook_req_sender.send(addr);
    }

    pub fn send_pt(&self, peer_addr: SocketAddr, data: Vec<u8>) {
        let _ = self.pt_sender.send(PTSendRequest { peer_addr, data });
    }

    /// Send full CLUTCH offer (~548KB) via TCP fallback (non-blocking)
    ///
    /// Uses VSF format with proper signing. Requires:
    /// - ceremony_id: Deterministic from sorted handle_hashes (same on both sides)
    /// - device keys: For Ed25519 signing of the VSF message
    pub fn send_offer(&self, request: ClutchOfferRequest) {
        let _ = self.offer_sender.send(request);
    }

    /// Send CLUTCH KEM response (~31KB) via TCP fallback (non-blocking)
    ///
    /// Uses VSF format with proper signing. Uses same deterministic ceremony_id.
    pub fn send_kem_response(&self, request: ClutchKemResponseRequest) {
        let _ = self.kem_response_sender.send(request);
    }

    /// Send CLUTCH complete proof (~200 bytes) via TCP fallback (non-blocking)
    ///
    /// Both parties exchange their eggs_proof after computing eggs. Proofs MUST match - if they don't, something is catastrophically wrong.
    pub fn send_complete_proof(&self, request: ClutchCompleteRequest) {
        let _ = self.complete_proof_sender.send(request);
    }

    /// Clone of the proof channel for a deferred send — the ceremony drain attaches it to the durable chains write so the proof fires post-durability (ChainsPostDurable::CeremonyProof).
    pub fn complete_proof_sender(&self) -> Sender<ClutchCompleteRequest> {
        self.complete_proof_sender.clone()
    }

    /// A cloneable handle for off-thread LAN broadcasting — the JOIN loop announces the joining device on the local network with it (the "see local devices" half of add-device).
    pub fn lan_broadcast_handle(&self) -> Sender<LanBroadcastRequest> {
        self.lan_broadcast_sender.clone()
    }

    /// Broadcast presence on LAN for local peer discovery (non-blocking) Solves NAT hairpinning - when peers are on same LAN, they can discover each other's local IPs
    pub fn send_lan_broadcast(&self, our_handle_proof: [u8; 32], our_port: u16) {
        let _ = self.lan_broadcast_sender.send(LanBroadcastRequest {
            our_handle_proof,
            our_port,
        });
    }

    /// Clear pending PT sends for a peer (non-blocking) NOTE: Currently unused - clearing PT sends during CLUTCH completion was killing ClutchComplete transfers in flight. Left for future use.
    #[allow(dead_code)]
    pub fn clear_pt_sends(&self, peer_addr: SocketAddr) {
        let _ = self.clear_pt_sender.send(ClearPtSendsRequest { peer_addr });
    }

    /// Check for status updates (non-blocking)
    pub fn try_recv(&self) -> Option<StatusUpdate> {
        self.status_receiver.try_recv().ok()
    }
}

/// Wake-sender type alias for optional use. Desktop carries a fluor `WakeSender` (post-migration; was winit's `EventLoopProxy`); Android has no UI-thread wake here (the JNI/Choreographer path drives redraws), so it stays unit.
#[cfg(not(target_os = "android"))]
type OptionalEventProxy = Option<Arc<dyn WakeSender<PhotonEvent>>>;
#[cfg(target_os = "android")]
type OptionalEventProxy = Option<()>;

/// Send a status update and wake the UI thread if a wake sender is available Sentinel `sender_addr` for a CLUTCH StatusUpdate that arrived via the FGTW relay, not a direct socket. The app checks for it to skip address-learning (a relayed message carries no reachable peer address) and to mark the contact reached_via_relay (lime-yellow presence). Unspecified v4:0 — never a real peer address.
pub const RELAY_ADDR: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);

/// Send a REPLY (pong, chat ACK, CLUTCH proof ACK) back to whoever sent us the message we're answering.
/// If it arrived directly (`dst` is a real address) this is a plain UDP send. If it arrived over the relay pipe (`dst == RELAY_ADDR`), UDP would black-hole to 0.0.0.0:0 — so instead we relay the reply back to the sender's device key over their pipe. `reply_to_device` is the device key extracted from the inbound message (its signer). This is what makes the relay BIDIRECTIONAL: a pong/ACK returns the same way the message came, so presence flips online on both ends and chat ACKs clear the sender's retransmit.
async fn relay_reply(
    socket: &tokio::net::UdpSocket,
    keypair: &crate::network::fgtw::Keypair,
    dst: SocketAddr,
    reply_to_device: &[u8; 32],
    bytes: &[u8],
) {
    if dst == RELAY_ADDR {
        if let Err(e) =
            crate::network::fgtw::relay::send_via_relay(keypair, reply_to_device, bytes).await
        {
            crate::logf!(
                "RELAY: reply to {} failed: {}",
                hex::encode(&reply_to_device[..4]),
                e
            );
        }
    } else {
        udp::send(socket, bytes, dst).await;
    }
}

// The old relay dispatch (split_concatenated_vsf + dispatch_relayed_clutch) is GONE. The pipe injects each relayed frame directly into the receiver's select! as a whole datagram tagged RELAY_ADDR, so the real dispatch parses it — CLUTCH, ping/pong, chat, acks all — with no bespoke relay parser. There is nothing to split either: a WebSocket frame IS one message (no concatenation, unlike the old fetch response).

fn send_status_update(
    status_tx: &Sender<StatusUpdate>,
    update: StatusUpdate,
    #[allow(unused_variables)] event_proxy: &OptionalEventProxy,
) {
    let _ = status_tx.send(update);
    #[cfg(not(target_os = "android"))]
    if let Some(proxy) = event_proxy {
        if let Err(e) = proxy.send(PhotonEvent::NetworkUpdate) {
            crate::logf!("Status: Failed to send wake event: {}", format!("{:?}", e));
        }
    }
    // Android has no event-loop proxy — the UI thread's tick is Choreographer-driven and stops when the app backgrounds. So the wake instead pokes the foreground service to run a headless protocol tick (advance_protocol), which drains this very update off the channel and advances the ceremony/chain without the screen being on. No-op while foregrounded (the service defers to the live draw) and when the Activity context isn't registered. See docs/background-tick.md.
    #[cfg(target_os = "android")]
    crate::platform::jni_android::request_service_tick();
}

/// Main checker loop running in tokio
async fn run_checker(
    std_socket: Arc<UdpSocket>,
    keypair: crate::network::fgtw::Keypair,
    our_pubkey: DevicePubkey,
    local_ip: Ipv4Addr,
    ping_rx: Receiver<PingRequest>,
    // NOTE: clutch_rx removed - legacy v1 CLUTCH no longer used
    message_rx: Receiver<MessageRequest>,
    ack_rx: Receiver<AckRequest>,
    avatar_request_rx: Receiver<AvatarRequestSend>,
    avatar_response_rx: Receiver<AvatarResponseSend>,
    history_rx: Receiver<HistorySendRequest>,
    pt_rx: Receiver<PTSendRequest>,
    offer_rx: Receiver<ClutchOfferRequest>,
    kem_response_rx: Receiver<ClutchKemResponseRequest>,
    complete_proof_rx: Receiver<ClutchCompleteRequest>,
    lan_broadcast_rx: Receiver<LanBroadcastRequest>,
    clear_pt_rx: Receiver<ClearPtSendsRequest>,
    status_tx: Sender<StatusUpdate>,
    contacts: ContactPubkeys,
    sync_records_provider: SyncRecordsProvider,
    pong_seal_keys: PongSealKeys,
    event_proxy: OptionalEventProxy,
    phonebook_req_rx: Receiver<SocketAddr>,
    peer_store: Arc<Mutex<crate::network::fgtw::PeerStore>>,
    needs_reflect: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    use tokio::net::UdpSocket as TokioUdpSocket;

    // Raw device pubkey for the LAN-discovery paths: stamped into our outgoing beacon and compared against incoming ones, so a device never learns its own looped-back beacon as a peer address ([u8; 32] is Copy — each spawned listener grabs its own).
    let our_device_pk: [u8; 32] = keypair.public.to_bytes();

    let cloned = match std_socket.try_clone() {
        Ok(s) => s,
        Err(e) => {
            crate::logf!("Status: Failed to clone socket: {}", e);
            return;
        }
    };

    let socket = match TokioUdpSocket::from_std(cloned) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            crate::logf!("Status: Failed to convert to tokio socket: {}", e);
            return;
        }
    };

    // Start TCP listener for CLUTCH large payloads (same port as UDP) Try IPv6 first (dual-stack), fall back to IPv4 Skip on Android - tokio TcpListener has issues with accept() returning EINVAL
    #[cfg(not(target_os = "android"))]
    let tcp_listener = {
        let udp_port = std_socket
            .local_addr()
            .map(|a| a.port())
            .unwrap_or(PHOTON_PORT);
        // Try IPv6 dual-stack first (accepts both IPv4 and IPv6 on most systems)
        let tcp_addr_v6 = SocketAddr::new(
            std::net::IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED),
            udp_port,
        );
        match tokio::net::TcpListener::bind(tcp_addr_v6).await {
            Ok(listener) => {
                crate::logf!("Status: TCP listening on [::]:{}  (dual-stack)", udp_port);
                Some(listener)
            }
            Err(_) => {
                // Fall back to IPv4 only
                let tcp_addr_v4 = SocketAddr::new(std::net::IpAddr::V4(local_ip), udp_port);
                match tokio::net::TcpListener::bind(tcp_addr_v4).await {
                    Ok(listener) => {
                        crate::logf!("Status: TCP listening on {} (IPv4 only)", tcp_addr_v4);
                        Some(listener)
                    }
                    Err(e) => {
                        crate::logf!("Status: Failed to bind TCP: {}", e);
                        None
                    }
                }
            }
        }
    };
    #[cfg(target_os = "android")]
    let tcp_listener: Option<tokio::net::TcpListener> = None;

    let pending: Arc<Mutex<Vec<PendingPing>>> = Arc::new(Mutex::new(Vec::new()));

    // Outstanding hole-punch probes, shared with the receiver task: the main loop inserts on send (fired alongside the ping cycle), the receiver resolves on a matching PunchProbeAck → a validated direct path.
    let pending_probes: Arc<Mutex<crate::network::traverse::punch::PendingProbes>> = Arc::new(
        Mutex::new(crate::network::traverse::punch::PendingProbes::new()),
    );

    // Track consecutive failed pings per contact (hysteresis - don't flip offline on 1 lost packet)
    let failed_pings: Arc<Mutex<Vec<([u8; 32], u8)>>> = Arc::new(Mutex::new(Vec::new()));
    const OFFLINE_THRESHOLD: u8 = 3;

    // PT manager for large transfers - shared with receiver task
    let pt: Arc<Mutex<PTManager>> = Arc::new(Mutex::new(PTManager::new(keypair.clone())));

    let socket_recv = socket.clone();
    let pending_recv = pending.clone();
    let pending_probes_recv = pending_probes.clone();
    let our_pubkey_recv = our_pubkey.clone();
    let keypair_recv = keypair.clone();
    let status_tx_recv = status_tx.clone();
    let contacts_recv = contacts.clone();
    let sync_records_recv = sync_records_provider.clone();
    // Both uses of the seal-key map live in the receiver task (pongs are BUILT there answering pings, and OPENED there parsing answers), so the map moves in whole — the `_recv` name just keeps the capture-list idiom.
    let pong_seal_keys_recv = pong_seal_keys;
    let event_proxy_recv = event_proxy.clone();
    let pt_recv = pt.clone();
    let failed_pings_recv = failed_pings.clone();
    let peer_store_recv = peer_store.clone();

    // Spawn multicast listener for LAN peer discovery Multicast is more reliable than broadcast across different network configurations
    {
        let status_tx_mcast = status_tx.clone();
        let event_proxy_mcast = event_proxy.clone();
        tokio::spawn(async move {
            // Photon-specific multicast group in administratively scoped range (239.x.x.x) Address derived from random entropy: 0x68C790 -> 239.104.199.144
            let multicast_addr: Ipv4Addr = Ipv4Addr::new(239, 104, 199, 144);
            let multicast_port = crate::MULTICAST_PORT;

            // Create socket bound to multicast port
            let socket = match std::net::UdpSocket::bind(format!("0.0.0.0:{}", multicast_port)) {
                Ok(s) => s,
                Err(e) => {
                    crate::logf!("LAN: Could not bind multicast socket: {}", e);
                    return;
                }
            };

            // Enable broadcast receive (for subnet broadcast fallback)
            let _ = socket.set_broadcast(true);

            // Join multicast group
            if let Err(e) = socket.join_multicast_v4(&multicast_addr, &Ipv4Addr::UNSPECIFIED) {
                crate::logf!("LAN: Failed to join multicast group: {}", e);
                return;
            }

            // Set non-blocking for async
            if let Err(e) = socket.set_nonblocking(true) {
                crate::logf!("LAN: Failed to set non-blocking: {}", e);
                return;
            }

            // Convert to tokio socket
            let socket = match tokio::net::UdpSocket::from_std(socket) {
                Ok(s) => s,
                Err(e) => {
                    crate::logf!("LAN: Failed to convert socket: {}", e);
                    return;
                }
            };

            crate::logf!(
                "LAN: Multicast listener on {}:{}",
                multicast_addr,
                multicast_port
            );

            // 64 KiB so a sync-record-laden datagram is never silently truncated (a short recv drops the tail → parse error → one-way presence).
            let mut buf = [0u8; 65536];
            loop {
                match socket.recv_from(&mut buf).await {
                    Ok((len, src_addr)) => {
                        crate::logf!("LAN: Multicast RX {} bytes from {}", len, src_addr);
                        let packet = &buf[..len];
                        // Only process pt_disc packets (LAN discovery)
                        if let Some(lan_update) =
                            parse_lan_discovery(packet, src_addr, &our_device_pk)
                        {
                            crate::logf!("LAN: Discovered peer via multicast: {}", src_addr);
                            send_status_update(&status_tx_mcast, lan_update, &event_proxy_mcast);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        // No data available, just continue
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    }
                    Err(e) => {
                        crate::logf!("LAN: Multicast recv error: {}", e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    }
                }
            }
        });
    }

    // Spawn IPv6 multicast listener for LAN peer discovery
    {
        let status_tx_mcast6 = status_tx.clone();
        let event_proxy_mcast6 = event_proxy.clone();
        tokio::spawn(async move {
            // IPv6 multicast group: ff02::68c7:9014 (link-local scope with our random bytes)
            let multicast_addr: std::net::Ipv6Addr =
                std::net::Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0x68c7, 0x9014);
            let multicast_port = crate::MULTICAST_PORT;

            // Create IPv6-only socket using libc to set IPV6_V6ONLY before binding This prevents dual-stack conflict with the IPv4 multicast socket on same port
            #[cfg(unix)]
            let socket = {
                use std::os::unix::io::FromRawFd;

                let fd = unsafe { libc::socket(libc::AF_INET6, libc::SOCK_DGRAM, 0) };
                if fd < 0 {
                    crate::log("LAN: Could not create IPv6 socket");
                    return;
                }

                // Set IPV6_V6ONLY so this socket only binds IPv6, not dual-stack
                let v6only: libc::c_int = 1;
                let ret = unsafe {
                    libc::setsockopt(
                        fd,
                        libc::IPPROTO_IPV6,
                        libc::IPV6_V6ONLY,
                        &v6only as *const _ as *const libc::c_void,
                        std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                    )
                };
                if ret < 0 {
                    crate::log("LAN: Could not set IPV6_V6ONLY");
                    unsafe { libc::close(fd) };
                    return;
                }

                // Set SO_REUSEADDR for multicast
                let reuseaddr: libc::c_int = 1;
                unsafe {
                    libc::setsockopt(
                        fd,
                        libc::SOL_SOCKET,
                        libc::SO_REUSEADDR,
                        &reuseaddr as *const _ as *const libc::c_void,
                        std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                    )
                };

                // Bind to [::]:port
                #[cfg(target_os = "macos")]
                let addr = libc::sockaddr_in6 {
                    sin6_len: std::mem::size_of::<libc::sockaddr_in6>() as u8,
                    sin6_family: libc::AF_INET6 as u8,
                    sin6_port: multicast_port.to_be(),
                    sin6_flowinfo: 0,
                    sin6_addr: libc::in6_addr { s6_addr: [0u8; 16] },
                    sin6_scope_id: 0,
                };
                #[cfg(not(target_os = "macos"))]
                let addr = libc::sockaddr_in6 {
                    sin6_family: libc::AF_INET6 as u16,
                    sin6_port: multicast_port.to_be(),
                    sin6_flowinfo: 0,
                    sin6_addr: libc::in6_addr { s6_addr: [0u8; 16] },
                    sin6_scope_id: 0,
                };
                let ret = unsafe {
                    libc::bind(
                        fd,
                        &addr as *const _ as *const libc::sockaddr,
                        std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
                    )
                };
                if ret < 0 {
                    let err = std::io::Error::last_os_error();
                    crate::logf!("LAN: Could not bind IPv6 multicast socket: {}", err);
                    unsafe { libc::close(fd) };
                    return;
                }

                unsafe { std::net::UdpSocket::from_raw_fd(fd) }
            };

            #[cfg(not(unix))]
            let socket = match std::net::UdpSocket::bind(format!("[::]:{}", multicast_port)) {
                Ok(s) => s,
                Err(e) => {
                    crate::logf!("LAN: Could not bind IPv6 multicast socket: {}", e);
                    return;
                }
            };

            // Join multicast group (interface 0 = default)
            if let Err(e) = socket.join_multicast_v6(&multicast_addr, 0) {
                crate::logf!("LAN: Failed to join IPv6 multicast group: {}", e);
                return;
            }

            if let Err(e) = socket.set_nonblocking(true) {
                crate::logf!("LAN: Failed to set non-blocking: {}", e);
                return;
            }

            let socket = match tokio::net::UdpSocket::from_std(socket) {
                Ok(s) => s,
                Err(e) => {
                    crate::logf!("LAN: Failed to convert IPv6 socket: {}", e);
                    return;
                }
            };

            crate::logf!(
                "LAN: IPv6 multicast listener on [{}]:{}",
                multicast_addr,
                multicast_port
            );

            // 64 KiB so a sync-record-laden datagram is never silently truncated.
            let mut buf = [0u8; 65536];
            loop {
                match socket.recv_from(&mut buf).await {
                    Ok((len, src_addr)) => {
                        crate::logf!("LAN: IPv6 Multicast RX {} bytes from {}", len, src_addr);
                        let packet = &buf[..len];
                        if let Some(lan_update) =
                            parse_lan_discovery(packet, src_addr, &our_device_pk)
                        {
                            crate::logf!("LAN: Discovered peer via IPv6 multicast: {}", src_addr);
                            send_status_update(&status_tx_mcast6, lan_update, &event_proxy_mcast6);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    }
                    Err(e) => {
                        crate::logf!("LAN: IPv6 multicast recv error: {}", e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    }
                }
            }
        });
    }

    // Spawn TCP receiver task for large CLUTCH payloads (VSF format)
    if let Some(listener) = tcp_listener {
        let status_tx_tcp = status_tx.clone();
        let event_proxy_tcp = event_proxy.clone();
        let contacts_tcp = contacts.clone();
        tokio::spawn(async move {
            crate::log("Status: TCP receiver task started");
            loop {
                // Async accept - sleeps until connection arrives (no polling)
                match listener.accept().await {
                    Ok((stream, src_addr)) => {
                        crate::logf!("Status: TCP connection from {}", src_addr);
                        // Convert to std TcpStream for tcp::recv (uses VSF L field for framing)
                        let std_stream = stream.into_std();
                        match std_stream {
                            Ok(mut std_stream) => {
                                // Read payload using VSF L field
                                match crate::network::tcp::recv(&mut std_stream) {
                                    Ok(data) => {
                                        crate::logf!(
                                            "Status: Received {} bytes via TCP from {}",
                                            data.len(),
                                            src_addr
                                        );

                                        // VSF inspection for development builds
                                        #[cfg(feature = "development")]
                                        {
                                            if let Ok(inspection) = vsf::inspect::inspect_vsf(&data)
                                            {
                                                crate::logf!(
                                                    "Status: Received TCP VSF:\n{}",
                                                    inspection
                                                );
                                            }
                                        }

                                        // Check for VSF magic bytes (RÅ< = 0x52 0xC3 0x85 0x3C)
                                        if data.len() >= 4
                                            && &data[0..3] == b"R\xC3\x85"
                                            && data[3] == b'<'
                                        {
                                            // Parse VSF header to determine message type Try parsing as ClutchOffer first
                                            use crate::network::fgtw::protocol::{
                                                parse_clutch_complete_vsf_without_recipient_check,
                                                parse_clutch_kem_response_vsf_without_recipient_check,
                                                parse_clutch_offer_vsf_without_recipient_check,
                                            };

                                            // Helper to check if sender is a known contact
                                            let is_known_sender =
                                                |pubkey_bytes: &[u8; 32]| -> bool {
                                                    let sender =
                                                        DevicePubkey::from_bytes(*pubkey_bytes);
                                                    let contact_list = contacts_tcp.lock().unwrap();
                                                    contact_list.iter().any(|p| *p == sender)
                                                };

                                            // Trust gate BEFORE the ~500KB CLUTCH section parse: the signer pubkey lives in the header (cheap to extract, no section walk), so reject an untrusted sender here rather than after parsing half a megabyte of their payload. The per-message is_known_sender checks below stay as defence-in-depth.
                                            if let Ok(signer) =
                                                vsf::verification::extract_signer_pubkey(&data)
                                            {
                                                if !is_known_sender(&signer) {
                                                    crate::logf!("TCP: CLUTCH message REJECTED before parse - sender not in contacts (pubkey: {})", hex::encode(&signer[..signer.len().min(8)]));
                                                    continue;
                                                }
                                            }

                                            // Try full offer first (has clutch_offer section)
                                            if let Ok((payload, sender_pubkey, offer_provenance, conversation_token)) =
                                                parse_clutch_offer_vsf_without_recipient_check(&data)
                                            {
                                                // SECURITY: Only accept from known contacts
                                                if !is_known_sender(&sender_pubkey) {
                                                    crate::logf!("TCP: ClutchOffer REJECTED from {} - sender not in contacts (pubkey: {})", src_addr, hex::encode(&sender_pubkey[..8]));
                                                    continue;
                                                }
                                                crate::log("Status: Received ClutchOffer via TCP (VSF verified)");
                                                send_status_update(
                                                    &status_tx_tcp,
                                                    StatusUpdate::ClutchOfferReceived {
                                                        conversation_token,
                                                        offer_provenance,
                                                        sender_pubkey,
                                                        payload,
                                                        sender_addr: src_addr,
                                                    },
                                                    &event_proxy_tcp,
                                                );
                                            }
                                            // Try KEM response
                                            else if let Ok((payload, sender_pubkey, ceremony_id, conversation_token)) =
                                                parse_clutch_kem_response_vsf_without_recipient_check(&data)
                                            {
                                                // SECURITY: Only accept from known contacts
                                                if !is_known_sender(&sender_pubkey) {
                                                    crate::logf!("TCP: ClutchKemResponse REJECTED from {} - sender not in contacts (pubkey: {})", src_addr, hex::encode(&sender_pubkey[..8]));
                                                    continue;
                                                }
                                                crate::log("Status: Received ClutchKemResponse via TCP (VSF verified)");
                                                send_status_update(
                                                    &status_tx_tcp,
                                                    StatusUpdate::ClutchKemResponseReceived {
                                                        conversation_token,
                                                        ceremony_id,
                                                        sender_pubkey,
                                                        payload,
                                                        sender_addr: src_addr,
                                                    },
                                                    &event_proxy_tcp,
                                                );
                                            }
                                            // Try complete proof
                                            else if let Ok((payload, sender_pubkey, ceremony_id, conversation_token)) =
                                                parse_clutch_complete_vsf_without_recipient_check(&data)
                                            {
                                                // SECURITY: Only accept from known contacts
                                                if !is_known_sender(&sender_pubkey) {
                                                    crate::logf!("TCP: ClutchComplete REJECTED from {} - sender not in contacts (pubkey: {})", src_addr, hex::encode(&sender_pubkey[..8]));
                                                    continue;
                                                }
                                                crate::log("Status: Received ClutchComplete via TCP (VSF verified)");
                                                send_status_update(
                                                    &status_tx_tcp,
                                                    StatusUpdate::ClutchCompleteReceived {
                                                        conversation_token,
                                                        ceremony_id,
                                                        sender_pubkey,
                                                        payload,
                                                        sender_addr: src_addr,
                                                    },
                                                    &event_proxy_tcp,
                                                );
                                            }
                                            else {
                                                crate::log("Status: Failed to parse TCP VSF as CLUTCH message");
                                            }
                                        } else {
                                            crate::logf!("Status: TCP payload is not VSF format (len={}, magic={})", data.len(), format!("{:02x?}", if data.len() >= 4 { &data[0..4] } else { &data[..] }));
                                        }
                                    }
                                    Err(e) => {
                                        crate::logf!("Status: TCP recv error: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                crate::logf!("Status: Failed to convert TCP stream: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        crate::logf!("Status: TCP accept error: {}", e);
                    }
                }
            }
        });
    }

    // The RELAY PIPE inject channel. Frames that arrive over the live WebSocket pipe are pushed here and the receiver task's select! pulls them out AS IF they'd arrived on the UDP socket, tagged RELAY_ADDR.
    // That means the ENTIRE existing dispatch — PT DATA, ping/pong presence, chat, acks, CLUTCH — runs on relayed bytes with zero bespoke per-message-type handling. A generous bound so a burst (a 548 KB CLUTCH offer arrives as one frame) never blocks the WS reader.
    // VOICE MEDIA egress (docs/calls.md): the engine's packets must leave from THIS socket — the port the peer's NAT already knows — so a dedicated awaited forwarder lives here (the polled request queues would add tens of ms; media gets its own task like the relay pipe).
    {
        let (media_tx, mut media_rx) =
            tokio::sync::mpsc::unbounded_channel::<(Vec<u8>, SocketAddr)>();
        crate::call::install_media_tx(media_tx);
        let media_socket = socket_recv.clone();
        tokio::spawn(async move {
            while let Some((bytes, addr)) = media_rx.recv().await {
                udp::send(&media_socket, &bytes, addr).await;
            }
        });
    }

    let (inject_tx, mut inject_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);

    // Spawn RELAY PIPE task — the live bridge for peers with NO direct path (asymmetric reachability: one end IPv6-only, the other IPv4-only). fgtw.org is dual-stack, so both reach it. We hold ONE WebSocket open to our own device's PipeHub (keyed by our device key); a sender's signed `relay` request is forwarded straight down it by the worker — no polling, no store-and-forward, no R2. Every frame received is fed into the inject channel, so it rides the receiver task's real dispatch tagged RELAY_ADDR (the app skips address-learning + marks reached_via_relay). Trust is applied downstream exactly as for a UDP packet (each parser verifies the signature; CLUTCH handlers gate on fold-respecting knows_device). This carries the WHOLE data plane, not just the ceremony — presence and chat ride it too.
    // Runs on EVERY platform including Android, which is the whole reason the relay exists: the peers that need it are on Android. tokio-tungstenite is not target-gated in Cargo and ring/rustls cross-compile under the NDK (the same precedent that un-gated nunc); unlike peer_updates' desktop-only WS, this runs in the JNI network runtime too. If a device build ever fails to hold the WS over cellular, that's the thing to chase — not a reason to gate it off, which would strand the exact peers it serves.
    {
        let our_dev_hex = hex::encode(our_device_pk);
        let inject_tx_pipe = inject_tx.clone();
        tokio::spawn(async move {
            use futures::StreamExt;
            use tokio_tungstenite::tungstenite::Message;
            let url = crate::network::http::seed_pipe_url(&our_dev_hex);
            crate::logf!(
                "PIPE: relay pipe task started (dev {}...)",
                &our_dev_hex[..8]
            );
            loop {
                match tokio_tungstenite::connect_async(&url).await {
                    Ok((ws_stream, _)) => {
                        crate::log("PIPE: connected — relay is a live socket now");
                        // KEEPALIVE, because a dead pipe is SILENT: dropping the write half meant no client ping ever went out, so a NAT idle-drop or a sleep/wake killed the TCP underneath and `read.next()` blocked forever — one desktop sat 52 minutes "relay: recipient offline" to every peer while believing it was connected (live fleet, 2026-08-05). A ping every 45s keeps carrier NAT mappings warm (cellular drops idle TCP in minutes); 120s of total silence (pongs count) declares the socket dead and reconnects.
                        let (mut write, mut read) = ws_stream.split();
                        let mut last_inbound = tokio::time::Instant::now();
                        let mut ping_tick =
                            tokio::time::interval(std::time::Duration::from_secs(45));
                        ping_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                        loop {
                            let msg = tokio::select! {
                                m = read.next() => match m {
                                    Some(m) => m,
                                    None => break,
                                },
                                _ = ping_tick.tick() => {
                                    use futures::SinkExt;
                                    if last_inbound.elapsed() >= std::time::Duration::from_secs(120) {
                                        crate::log("PIPE: silent past the liveness window — reconnecting");
                                        break;
                                    }
                                    if write.send(Message::Ping(Default::default())).await.is_err() {
                                        crate::log("PIPE: ping write failed — reconnecting");
                                        break;
                                    }
                                    continue;
                                }
                            };
                            last_inbound = tokio::time::Instant::now();
                            match msg {
                                Ok(Message::Binary(data)) => {
                                    // Peel the authenticated relay envelope the worker now forwards intact. The envelope's presence + valid sender signature IS the domain separator: a frame off the pipe is KNOWN-relayed from device X, ground truth in the bytes, not the RELAY_ADDR sentinel. Inject only the inner payload — it's byte-identical to a direct message, so the dispatch below is untouched.
                                    match crate::network::fgtw::relay::peel_relay_envelope(&data) {
                                        Some((sender_key, inner)) => {
                                            crate::logf!("PIPE: ← {}B envelope from {} → {}B inner (injecting)", data.len(), hex::encode(&sender_key[..4]), inner.len());
                                            if inject_tx_pipe.send(inner).await.is_err() {
                                                // Receiver task gone — the whole status task is tearing down.
                                                return;
                                            }
                                        }
                                        None => {
                                            crate::logf!("PIPE: ← {}B dropped — not a valid signed relay envelope", data.len());
                                        }
                                    }
                                }
                                Ok(Message::Close(_)) => {
                                    crate::log("PIPE: server closed");
                                    break;
                                }
                                Ok(_) => {} // text/ping/pong — tungstenite auto-pongs
                                Err(e) => {
                                    crate::logf!("PIPE: read error: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        crate::logf!("PIPE: connect failed: {} — retrying", e);
                    }
                }
                // Reconnect: hold the pipe open for the life of the session so a relay-only peer stays reachable.
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
    }

    // Spawn UDP receiver task
    tokio::spawn(async move {
        crate::log("Status: Receiver task started, waiting for UDP packets...");
        // 64 KiB RX buffer: a pong laden with per-conversation sync records exceeds 2 KiB and a short recv silently truncated it → parse error → one-way presence (a peer never saw the other).
        let mut buf = [0u8; 65536];
        // This node's own reflexive (public) address, learned from peer-echoed reflection (pong `observed_addr` + `ReflectResponse`). Local to the long-lived receiver task; each adoption change is pushed to the app as `StatusUpdate::ReflexiveLearned`.
        let mut reflexive = crate::network::traverse::reflexive::ReflexiveState::new();
        // Ingress twin-collapse for chat frames: the SAME frame routinely arrives twice within milliseconds (direct UDP + relay pipe, or LAN + WAN race) and both copies queue toward the UI. The durable rarangi-row dedup catches reprocessing, but collapsing twins HERE cuts the redundant queue traffic and re-ACK spam at the source. TIME-bounded, never count-bounded: only twins inside a short window are collapsed, so a genuine later retransmit (sender's ACK was lost) still reaches the UI's re-ACK path.
        // 2s, deliberately TIGHT. A genuine dual-path twin (direct + relay copies of one send) lands within milliseconds; a same-ciphertext frame seconds later is the sender's RETRANSMIT ladder, and the app layer NEEDS those — they carry the re-ACK-from-storage heal for a lost ACK and the repeat-evidence the garbage-decrypt fork detector counts. At 10s the collapse ate the entire early ladder: a forked chain garbage-decrypted ONCE, the streak froze at 1, no repair ever fired, and the sender's ACK stayed stuck forever (live pair, desktop→phone, 2026-08-02).
        const CHAT_TWIN_WINDOW: std::time::Duration = std::time::Duration::from_secs(2);
        let mut recent_chat_frames: Vec<(([u8; 8], i64, [u8; 8]), std::time::Instant)> = Vec::new();
        // Devices whose sealed pong tail we could not open (no key seeded yet, or a stale key across their re-attest) — logged ONCE per device, not per pong: pongs arrive every cycle and a still-loading key map would otherwise spam a line every few seconds. An entry clears on the first successful open, so a key change that breaks again re-logs.
        let mut pong_open_failed: Vec<[u8; 32]> = Vec::new();
        // Probe-REFLECTION rate cap: at most one reverse probe per device per minute. Reflection is how the side with NO working candidates ever validates its own direction (see the PunchProbe arm); the cap keeps two reflecting peers from probe ping-pong, and validation quiets both sides naturally (a validated side probes only its validated remote as keepalive).
        let mut reverse_probed: Vec<([u8; 32], std::time::Instant)> = Vec::new();
        loop {
            // Take the next datagram from EITHER the real UDP socket OR the relay pipe. A pipe frame is handed `RELAY_ADDR` as its source, so everything below this line — the entire ~900-line dispatch — cannot tell a relayed message from a directly-received one, except that RELAY_ADDR tells the app to skip address-learning and mark reached_via_relay. This is the whole reason the pipe is one select! arm and not a parallel dispatch: presence, chat, acks and CLUTCH all reuse the proven receive path.
            // A UDP datagram lands in the fixed 64 KiB `buf`; an injected pipe frame is held in `injected_holder` (owned Vec) because it can be a whole ~548 KB CLUTCH offer — FAR larger than `buf`. Copying it into `buf` truncated it to 64 KiB and the offer never parsed ("Not enough data"), which is why the ceremony stalled over the relay: the offer was injected but chopped to 12% of itself. `msg_bytes` points at whichever holds this iteration's frame.
            let mut injected_holder: Option<Vec<u8>> = None;
            let recv_result: std::io::Result<(usize, SocketAddr)> = tokio::select! {
                r = socket_recv.recv_from(&mut buf) => r,
                injected = inject_rx.recv() => match injected {
                    Some(bytes) => {
                        let n = bytes.len();
                        injected_holder = Some(bytes);
                        Ok((n, RELAY_ADDR))
                    }
                    None => {
                        // Pipe task dropped its sender — never expected while the session lives; keep serving UDP.
                        continue;
                    }
                },
            };
            match recv_result {
                Ok((len, src_addr)) => {
                    let msg_bytes: &[u8] = match &injected_holder {
                        Some(v) => &v[..len],
                        None => &buf[..len],
                    };

                    // VOICE MEDIA FAST PATH (docs/calls.md): one-byte high-ASCII magic check BEFORE the entire parse ladder (every other frame leads with plain ASCII: VSF 'R', PT lowercase) — 50 packets/second must not pay for trial parsing, PT acks, or StatusUpdates. Matches route raw to the call engine's sink; with no live call they silently drop (also the correct fate for post-hangup stragglers). The magic collides with nothing here: VSF opens "RÅ<", PT DATA opens with a lowercase stream id.
                    if crate::call::packet::is_media_packet(msg_bytes) {
                        crate::call::deliver_media(msg_bytes, src_addr);
                        continue;
                    }

                    // Check for PT DATA packets first (start with 'd') NOTE: Individual DATA packets not logged - only completion/failure
                    if is_pt_data(msg_bytes) {
                        if let Some(data) = PTData::from_bytes(msg_bytes) {
                            // Handle data and collect responses (must drop lock before await)
                            let (ack_bytes, complete_bytes, received_data, inbound_stats) = {
                                // Capture the stream BEFORE `data` is moved into handle_data: completion + drain are stream-scoped so concurrent transfers from the same peer (CLUTCH offer + KEM response) don't get cross-wired and silently dropped.
                                let stream_id = data.stream_id;
                                let mut pt_mgr = pt_recv.lock().unwrap();
                                let ack = pt_mgr.handle_data(src_addr, data);
                                let complete = pt_mgr.check_inbound_complete(src_addr, stream_id);
                                let stats = pt_mgr.inbound_stats(&src_addr);
                                let data = if complete.is_some() {
                                    pt_mgr.take_inbound_data(src_addr, stream_id)
                                } else {
                                    None
                                };
                                (ack, complete, data, stats)
                            };
                            // Now send responses (lock is dropped)
                            if let Some(ack) = ack_bytes {
                                udp::send(&socket_recv, &ack, src_addr).await;
                            }
                            if let Some(complete) = complete_bytes {
                                udp::send(&socket_recv, &complete, src_addr).await;
                                if let Some(data) = received_data {
                                    // Log utilization summary
                                    if let Some((packets, bytes, duplicates, duration_ms)) =
                                        inbound_stats
                                    {
                                        let total_recv = packets + duplicates;
                                        let utilization = if total_recv > 0 {
                                            (packets as f64 / total_recv as f64) * 100.0
                                        } else {
                                            100.0
                                        };
                                        let thruput_kbps = if duration_ms > 0 {
                                            (bytes as f64 * 8.0) / (duration_ms as f64)
                                        } else {
                                            0.0
                                        };
                                        let thruput_str = if thruput_kbps >= 1000.0 {
                                            format!("{:.1} Mbps", thruput_kbps / 1000.0)
                                        } else {
                                            format!("{:.0} kbps", thruput_kbps)
                                        };
                                        crate::logf!("PT: ← {} OK | {} | {:.1}s | {} pkts | {:.0}% util ({} dups)", src_addr, thruput_str, duration_ms as f64 / 1000.0, packets, utilization, duplicates);
                                    } else {
                                        crate::logf!(
                                            "PT: ← {} OK | {} bytes",
                                            src_addr,
                                            data.len()
                                        );
                                    }

                                    // Inspect completed PT data via the OPT-IN inspector (PHOTON_INSPECT=net). This call site predated the opt-in gate and dumped a full coloured tree for EVERY completed PT transfer — 200 trees in one 35-minute field log during a page storm, exactly the volume problem the gate exists for (16 MiB self-trim eating whole sessions).
                                    #[cfg(feature = "development")]
                                    {
                                        let msg = crate::network::inspect::vsf_inspect(
                                            &data,
                                            "PT",
                                            "RX",
                                            &src_addr.to_string(),
                                        );
                                        if !msg.is_empty() {
                                            crate::log(&msg);
                                        }
                                    }

                                    // Parse PT data as CLUTCH message and emit appropriate event
                                    use crate::network::fgtw::protocol::{
                                        parse_clutch_complete_vsf_without_recipient_check,
                                        parse_clutch_kem_response_vsf_without_recipient_check,
                                        parse_clutch_offer_vsf_without_recipient_check,
                                    };

                                    // Helper to check if sender is a known contact (defense-in-depth) Note: PT SPEC validation should have already rejected unknown senders
                                    let is_known_sender_pt = |pubkey_bytes: &[u8; 32]| -> bool {
                                        let sender = DevicePubkey::from_bytes(*pubkey_bytes);
                                        let contact_list = contacts_recv.lock().unwrap();
                                        contact_list.iter().any(|p| *p == sender)
                                    };

                                    // Trust gate BEFORE the ~500KB CLUTCH section parse: the signer pubkey is a cheap header extraction (no section walk), so an untrusted sender is dropped before we parse their payload. The per-message checks below remain as defence-in-depth.
                                    if let Ok(signer) =
                                        vsf::verification::extract_signer_pubkey(&data)
                                    {
                                        if !is_known_sender_pt(&signer) {
                                            crate::logf!("PT: CLUTCH message REJECTED before parse - sender not in contacts (pubkey: {})", hex::encode(&signer[..signer.len().min(8)]));
                                            continue;
                                        }
                                    }

                                    // Try to parse as ClutchOffer
                                    if let Ok((
                                        payload,
                                        sender_pubkey,
                                        offer_provenance,
                                        conversation_token,
                                    )) = parse_clutch_offer_vsf_without_recipient_check(&data)
                                    {
                                        // Defense-in-depth: verify sender again
                                        if !is_known_sender_pt(&sender_pubkey) {
                                            crate::logf!("PT: ClutchOffer REJECTED (defense-in-depth) - pubkey: {}", hex::encode(&sender_pubkey[..8]));
                                            continue;
                                        }
                                        crate::log("PT: Parsed as ClutchOffer (VSF verified)");
                                        send_status_update(
                                            &status_tx_recv,
                                            StatusUpdate::ClutchOfferReceived {
                                                conversation_token,
                                                offer_provenance,
                                                sender_pubkey,
                                                payload,
                                                sender_addr: src_addr,
                                            },
                                            &event_proxy_recv,
                                        );
                                    }
                                    // Try to parse as ClutchKemResponse
                                    else if let Ok((
                                        payload,
                                        sender_pubkey,
                                        ceremony_id,
                                        conversation_token,
                                    )) =
                                        parse_clutch_kem_response_vsf_without_recipient_check(&data)
                                    {
                                        // Defense-in-depth: verify sender again
                                        if !is_known_sender_pt(&sender_pubkey) {
                                            crate::logf!("PT: ClutchKemResponse REJECTED (defense-in-depth) - pubkey: {}", hex::encode(&sender_pubkey[..8]));
                                            continue;
                                        }
                                        crate::log(
                                            "PT: Parsed as ClutchKemResponse (VSF verified)",
                                        );
                                        send_status_update(
                                            &status_tx_recv,
                                            StatusUpdate::ClutchKemResponseReceived {
                                                conversation_token,
                                                ceremony_id,
                                                sender_pubkey,
                                                payload,
                                                sender_addr: src_addr,
                                            },
                                            &event_proxy_recv,
                                        );
                                    }
                                    // Try to parse as ClutchComplete
                                    else if let Ok((
                                        payload,
                                        sender_pubkey,
                                        ceremony_id,
                                        conversation_token,
                                    )) =
                                        parse_clutch_complete_vsf_without_recipient_check(&data)
                                    {
                                        // Defense-in-depth: verify sender again
                                        if !is_known_sender_pt(&sender_pubkey) {
                                            crate::logf!("PT: ClutchComplete REJECTED (defense-in-depth) - pubkey: {}", hex::encode(&sender_pubkey[..8]));
                                            continue;
                                        }
                                        crate::log("PT: Parsed as ClutchComplete (VSF verified)");
                                        send_status_update(
                                            &status_tx_recv,
                                            StatusUpdate::ClutchCompleteReceived {
                                                conversation_token,
                                                ceremony_id,
                                                sender_pubkey,
                                                payload,
                                                sender_addr: src_addr,
                                            },
                                            &event_proxy_recv,
                                        );
                                    }
                                    // Try to parse as history request (hist_req)
                                    else if let Ok((payload, sender_pubkey)) =
                                        crate::network::fgtw::protocol::parse_history_request_vsf(
                                            &data,
                                        )
                                    {
                                        if !is_known_sender_pt(&sender_pubkey) {
                                            crate::log("PT: hist_req REJECTED - unknown sender");
                                            continue;
                                        }
                                        send_status_update(
                                            &status_tx_recv,
                                            StatusUpdate::HistoryRequestReceived {
                                                conversation_token: payload.conversation_token,
                                                before_osc: payload.before_osc,
                                                limit: payload.limit,
                                                request_id: payload.request_id,
                                                sent_osc: payload.sent_osc,
                                                sender_pubkey: DevicePubkey::from_bytes(
                                                    sender_pubkey,
                                                ),
                                                sender_addr: src_addr,
                                            },
                                            &event_proxy_recv,
                                        );
                                    }
                                    // Try to parse as history page (hist_page)
                                    else if let Ok((
                                        (conversation_token, request_id, epoch_k, sealed),
                                        sender_pubkey,
                                    )) = crate::network::fgtw::protocol::parse_history_page_vsf(
                                        &data,
                                    ) {
                                        if !is_known_sender_pt(&sender_pubkey) {
                                            crate::log("PT: hist_page REJECTED - unknown sender");
                                            continue;
                                        }
                                        send_status_update(
                                            &status_tx_recv,
                                            StatusUpdate::HistoryPageReceived {
                                                conversation_token,
                                                request_id,
                                                epoch_k,
                                                sealed,
                                                sender_pubkey: DevicePubkey::from_bytes(
                                                    sender_pubkey,
                                                ),
                                                sender_addr: src_addr,
                                            },
                                            &event_proxy_recv,
                                        );
                                    }
                                    // Attachment blob (the file bytes — the typical PT large transfer)
                                    else if let Ok((
                                        (conversation_token, content_hash, sealed),
                                        sender_pubkey,
                                    )) = crate::network::fgtw::protocol::parse_attach_blob_vsf(
                                        &data,
                                    ) {
                                        if !is_known_sender_pt(&sender_pubkey) {
                                            crate::log("PT: attach_blob REJECTED - unknown sender");
                                            continue;
                                        }
                                        send_status_update(
                                            &status_tx_recv,
                                            StatusUpdate::AttachBlobReceived {
                                                conversation_token,
                                                content_hash,
                                                sealed,
                                                sender_pubkey: DevicePubkey::from_bytes(
                                                    sender_pubkey,
                                                ),
                                                sender_addr: src_addr,
                                            },
                                            &event_proxy_recv,
                                        );
                                    }
                                    // Attachment blob request (tap on a pill whose blob hasn't arrived)
                                    else if let Ok((
                                        (conversation_token, content_hash),
                                        sender_pubkey,
                                    )) = crate::network::fgtw::protocol::parse_attach_req_vsf(
                                        &data,
                                    ) {
                                        if !is_known_sender_pt(&sender_pubkey) {
                                            crate::log("PT: attach_req REJECTED - unknown sender");
                                            continue;
                                        }
                                        send_status_update(
                                            &status_tx_recv,
                                            StatusUpdate::AttachReqReceived {
                                                conversation_token,
                                                content_hash,
                                                sender_pubkey: DevicePubkey::from_bytes(
                                                    sender_pubkey,
                                                ),
                                                sender_addr: src_addr,
                                            },
                                            &event_proxy_recv,
                                        );
                                    }
                                    // Try to parse as a blind frame (blind_put/ack/get/srv — tiny, but PT delivery is possible under fallback routing)
                                    else if let Some((kind, payload, sender_pubkey)) =
                                        crate::network::fgtw::protocol::parse_any_blind_frame(
                                            &data,
                                        )
                                    {
                                        if !is_known_sender_pt(&sender_pubkey) {
                                            crate::log("PT: blind frame REJECTED - unknown sender");
                                            continue;
                                        }
                                        send_status_update(
                                            &status_tx_recv,
                                            StatusUpdate::BlindFrameReceived {
                                                kind,
                                                conversation_token: payload.conversation_token,
                                                request_id: payload.request_id,
                                                blob: payload.blob,
                                                found: payload.found,
                                                sent_osc: payload.sent_osc,
                                                sender_pubkey: DevicePubkey::from_bytes(
                                                    sender_pubkey,
                                                ),
                                                sender_addr: src_addr,
                                            },
                                            &event_proxy_recv,
                                        );
                                    } else if let Ok(crate::network::fgtw::protocol::FgtwMessage::AvatarResponse {
                                        timestamp: _,
                                        responder_pubkey,
                                        provenance_hash,
                                        signature,
                                        avatar_vsf,
                                    }) = crate::network::fgtw::protocol::FgtwMessage::from_vsf_bytes(&data)
                                    {
                                        // A P2P avatar answer big enough to ride PT (typical: ~24KB AV1) — same verify + emit as the UDP arm. This was the "PT: Received unknown 23.9KB" drop: the PT completion chain knew clutch/hist/blind but not av_resp, so large avatars silently fell thru to the FGTW fallback.
                                        let provenance: [u8; 32] = blake3::hash(&avatar_vsf).into();
                                        if provenance == provenance_hash
                                            && verify_provenance_signature(&provenance_hash, &responder_pubkey, &signature)
                                        {
                                            crate::logf!("PT: avatar response reassembled ({} bytes)", avatar_vsf.len());
                                            send_status_update(
                                                &status_tx_recv,
                                                StatusUpdate::AvatarReceived {
                                                    responder_pubkey,
                                                    avatar_vsf,
                                                    sender_addr: src_addr,
                                                },
                                                &event_proxy_recv,
                                            );
                                        } else {
                                            crate::log("PT: avatar response REJECTED (bad signature)");
                                        }
                                    } else {
                                        // Unknown PT data - emit generic event for debugging
                                        crate::logf!("PT: Failed to parse {} bytes as CLUTCH message", data.len());
                                        send_status_update(
                                            &status_tx_recv,
                                            StatusUpdate::PTReceived {
                                                peer_addr: src_addr,
                                                data,
                                            },
                                            &event_proxy_recv,
                                        );
                                    }
                                }
                            }
                        }
                        continue;
                    }

                    // Centralized UDP RX logging - THE ONLY place incoming packets are logged
                    #[cfg(feature = "development")]
                    udp::log_received(msg_bytes, &src_addr);

                    // Handle LAN discovery packets (same port as main socket now)
                    if let Some(lan_update) =
                        parse_lan_discovery(msg_bytes, src_addr, &our_device_pk)
                    {
                        send_status_update(&status_tx_recv, lan_update, &event_proxy_recv);
                        continue;
                    }

                    // Try to parse as PT VSF packets (SPEC, ACK, NAK, CONTROL, COMPLETE)
                    if let Some(pt_handled) = handle_pt_vsf_packet(
                        msg_bytes,
                        src_addr,
                        &pt_recv,
                        &socket_recv,
                        &status_tx_recv,
                        &event_proxy_recv,
                        &contacts_recv,
                    )
                    .await
                    {
                        if pt_handled {
                            continue;
                        }
                    }

                    // Try to parse small direct UDP VSF messages (ClutchComplete, etc.) These are sent directly without PT overhead for efficiency
                    if msg_bytes.len() >= 4
                        && &msg_bytes[0..3] == b"R\xC3\x85"
                        && msg_bytes[3] == b'<'
                    {
                        use crate::network::fgtw::protocol::parse_clutch_complete_vsf_without_recipient_check;

                        // ClutchComplete (the proof) is ~300 bytes and its parser verifies the whole-file signature internally, so parse it UNCONDITIONALLY — no contact-allowlist pre-gate.
                        // The old gate keyed on `contacts_recv`, which is a race for a freshly-reconciled SIBLING device: its key isn't in the allowlist the instant its proof arrives, so the proof fell thru to FgtwMessage::from_vsf_bytes — which does NOT handle clutch_* and emitted "Parse error: got 'clutch_complete'", dropping the proof.
                        // The sender then retransmitted 5× and gave up, and the sibling weave sat "pending" forever.
                        // The gate was a DoS pre-filter meant for the ~500KB offer (which arrives via PT, not here), so it bought nothing on a 300-byte frame while breaking sibling completion.
                        // Trust is still applied downstream: the app's CLUTCH handler gates on fold-respecting knows_device.
                        {
                            if let Ok((payload, sender_pubkey, ceremony_id, conversation_token)) =
                                parse_clutch_complete_vsf_without_recipient_check(msg_bytes)
                            {
                                crate::log("UDP: Received ClutchComplete directly (VSF verified)");
                                // Delivery ack — ClutchComplete is sent as a reliable PT packet; without acking it the sender's stop-and-wait queue head never clears and it blocks every later packet (chat) behind it. Pure transport "bytes got here"; the proof's own convergence logic is layered on top.
                                {
                                    let ack_bytes = {
                                        let pt_mgr = pt_recv.lock().unwrap();
                                        pt_mgr.build_packet_ack(msg_bytes)
                                    };
                                    udp::send(&socket_recv, &ack_bytes, src_addr).await;
                                }
                                send_status_update(
                                    &status_tx_recv,
                                    StatusUpdate::ClutchCompleteReceived {
                                        conversation_token,
                                        ceremony_id,
                                        sender_pubkey,
                                        payload,
                                        sender_addr: src_addr,
                                    },
                                    &event_proxy_recv,
                                );
                                continue;
                            }
                            // ClutchOffer (~548KB) and ClutchKemResponse (~32KB) arriving as a WHOLE frame — this is the RELAY-INJECTED path.
                            // Direct sends shard these through PT and parse them in the PT-transfer-complete branch above, but a relayed message is injected as one datagram tagged RELAY_ADDR, so it never touches PT and only clutch_complete was parsed here — the offer + KEM were silently dropped, so the ceremony never got past the offer over the relay (presence worked, but no KEM ever came back).
                            // Parse them here too. The parsers verify the signature internally; the app's CLUTCH handler gates action on fold-respecting knows_device.
                            // No packet-ack: unlike a PT-carried frame, a relayed one isn't in a stop-and-wait queue awaiting one.
                            if let Ok((payload, sender_pubkey, offer_provenance, conversation_token)) =
                                crate::network::fgtw::protocol::parse_clutch_offer_vsf_without_recipient_check(msg_bytes)
                            {
                                crate::log("RELAY-INJECT: Received ClutchOffer (VSF verified)");
                                send_status_update(
                                    &status_tx_recv,
                                    StatusUpdate::ClutchOfferReceived {
                                        conversation_token,
                                        offer_provenance,
                                        sender_pubkey,
                                        payload,
                                        sender_addr: src_addr,
                                    },
                                    &event_proxy_recv,
                                );
                                continue;
                            }
                            if let Ok((payload, sender_pubkey, ceremony_id, conversation_token)) =
                                crate::network::fgtw::protocol::parse_clutch_kem_response_vsf_without_recipient_check(msg_bytes)
                            {
                                crate::log("RELAY-INJECT: Received ClutchKemResponse (VSF verified)");
                                send_status_update(
                                    &status_tx_recv,
                                    StatusUpdate::ClutchKemResponseReceived {
                                        conversation_token,
                                        ceremony_id,
                                        sender_pubkey,
                                        payload,
                                        sender_addr: src_addr,
                                    },
                                    &event_proxy_recv,
                                );
                                continue;
                            }
                            // History request (hist_req, ~200B — always rides this small-frame path). MUST packet-ack: it's sent via send_with_pubkey's reliable stop-and-wait queue; an un-acked type retransmits forever and head-of-line-blocks chat.
                            if let Ok((payload, sender_pubkey)) =
                                crate::network::fgtw::protocol::parse_history_request_vsf(msg_bytes)
                            {
                                {
                                    let ack_bytes = {
                                        let pt_mgr = pt_recv.lock().unwrap();
                                        pt_mgr.build_packet_ack(msg_bytes)
                                    };
                                    udp::send(&socket_recv, &ack_bytes, src_addr).await;
                                }
                                send_status_update(
                                    &status_tx_recv,
                                    StatusUpdate::HistoryRequestReceived {
                                        conversation_token: payload.conversation_token,
                                        before_osc: payload.before_osc,
                                        limit: payload.limit,
                                        request_id: payload.request_id,
                                        sent_osc: payload.sent_osc,
                                        sender_pubkey: DevicePubkey::from_bytes(sender_pubkey),
                                        sender_addr: src_addr,
                                    },
                                    &event_proxy_recv,
                                );
                                continue;
                            }
                            // History page (hist_page — small pages ride this path; big ones arrive via the PT-transfer-complete branch). Same mandatory packet-ack.
                            if let Ok((
                                (conversation_token, request_id, epoch_k, sealed),
                                sender_pubkey,
                            )) =
                                crate::network::fgtw::protocol::parse_history_page_vsf(msg_bytes)
                            {
                                {
                                    let ack_bytes = {
                                        let pt_mgr = pt_recv.lock().unwrap();
                                        pt_mgr.build_packet_ack(msg_bytes)
                                    };
                                    udp::send(&socket_recv, &ack_bytes, src_addr).await;
                                }
                                send_status_update(
                                    &status_tx_recv,
                                    StatusUpdate::HistoryPageReceived {
                                        conversation_token,
                                        request_id,
                                        epoch_k,
                                        sealed,
                                        sender_pubkey: DevicePubkey::from_bytes(sender_pubkey),
                                        sender_addr: src_addr,
                                    },
                                    &event_proxy_recv,
                                );
                                continue;
                            }
                            // Fleet chain-state replication (chain_sync — a sibling pushing its advanced chains). Same mandatory packet-ack.
                            if let Ok(((conversation_token, epoch_k, sealed), sender_pubkey)) =
                                crate::network::fgtw::protocol::parse_chain_sync_vsf(msg_bytes)
                            {
                                {
                                    let ack_bytes = {
                                        let pt_mgr = pt_recv.lock().unwrap();
                                        pt_mgr.build_packet_ack(msg_bytes)
                                    };
                                    udp::send(&socket_recv, &ack_bytes, src_addr).await;
                                }
                                send_status_update(
                                    &status_tx_recv,
                                    StatusUpdate::ChainSyncReceived {
                                        conversation_token,
                                        epoch_k,
                                        sealed,
                                        sender_pubkey: DevicePubkey::from_bytes(sender_pubkey),
                                    },
                                    &event_proxy_recv,
                                );
                                continue;
                            }
                            // The checkpoint spine's three sibling frames (root hand-off, catch-up request, state serve). Same mandatory packet-ack on each.
                            if let Ok(((k, fanout_epoch, sealed), sender_pubkey)) =
                                crate::network::fgtw::protocol::parse_ckpt_root_vsf(msg_bytes)
                            {
                                {
                                    let ack_bytes = {
                                        let pt_mgr = pt_recv.lock().unwrap();
                                        pt_mgr.build_packet_ack(msg_bytes)
                                    };
                                    udp::send(&socket_recv, &ack_bytes, src_addr).await;
                                }
                                send_status_update(
                                    &status_tx_recv,
                                    StatusUpdate::CkptRootReceived {
                                        k,
                                        fanout_epoch,
                                        sealed,
                                        sender_pubkey: DevicePubkey::from_bytes(sender_pubkey),
                                    },
                                    &event_proxy_recv,
                                );
                                continue;
                            }
                            if let Ok(((conversation_token, osc, active), sender_pubkey)) =
                                crate::network::fgtw::protocol::parse_focus_vsf(msg_bytes)
                            {
                                {
                                    let ack_bytes = {
                                        let pt_mgr = pt_recv.lock().unwrap();
                                        pt_mgr.build_packet_ack(msg_bytes)
                                    };
                                    udp::send(&socket_recv, &ack_bytes, src_addr).await;
                                }
                                send_status_update(
                                    &status_tx_recv,
                                    StatusUpdate::FocusClaimReceived {
                                        conversation_token,
                                        osc,
                                        active,
                                        sender_pubkey: DevicePubkey::from_bytes(sender_pubkey),
                                    },
                                    &event_proxy_recv,
                                );
                                continue;
                            }
                            if let Ok((osc, sender_pubkey)) =
                                crate::network::fgtw::protocol::parse_attention_vsf(msg_bytes)
                            {
                                {
                                    let ack_bytes = {
                                        let pt_mgr = pt_recv.lock().unwrap();
                                        pt_mgr.build_packet_ack(msg_bytes)
                                    };
                                    udp::send(&socket_recv, &ack_bytes, src_addr).await;
                                }
                                send_status_update(
                                    &status_tx_recv,
                                    StatusUpdate::AttentionReceived {
                                        osc,
                                        sender_pubkey: DevicePubkey::from_bytes(sender_pubkey),
                                    },
                                    &event_proxy_recv,
                                );
                                continue;
                            }
                            if let Ok((have_k, sender_pubkey)) =
                                crate::network::fgtw::protocol::parse_ckpt_req_vsf(msg_bytes)
                            {
                                {
                                    let ack_bytes = {
                                        let pt_mgr = pt_recv.lock().unwrap();
                                        pt_mgr.build_packet_ack(msg_bytes)
                                    };
                                    udp::send(&socket_recv, &ack_bytes, src_addr).await;
                                }
                                send_status_update(
                                    &status_tx_recv,
                                    StatusUpdate::CkptReqReceived {
                                        have_k,
                                        sender_pubkey: DevicePubkey::from_bytes(sender_pubkey),
                                        sender_addr: src_addr,
                                    },
                                    &event_proxy_recv,
                                );
                                continue;
                            }
                            if let Ok(((k, sealed), sender_pubkey)) =
                                crate::network::fgtw::protocol::parse_ckpt_state_vsf(msg_bytes)
                            {
                                {
                                    let ack_bytes = {
                                        let pt_mgr = pt_recv.lock().unwrap();
                                        pt_mgr.build_packet_ack(msg_bytes)
                                    };
                                    udp::send(&socket_recv, &ack_bytes, src_addr).await;
                                }
                                send_status_update(
                                    &status_tx_recv,
                                    StatusUpdate::CkptStateReceived {
                                        k,
                                        sealed,
                                        sender_pubkey: DevicePubkey::from_bytes(sender_pubkey),
                                    },
                                    &event_proxy_recv,
                                );
                                continue;
                            }
                            // Attachment frames on the datagram/relay path (a relay-injected whole frame, or a small blob that fit one packet). Same mandatory packet-ack.
                            if let Ok(((conversation_token, content_hash, sealed), sender_pubkey)) =
                                crate::network::fgtw::protocol::parse_attach_blob_vsf(msg_bytes)
                            {
                                {
                                    let ack_bytes = {
                                        let pt_mgr = pt_recv.lock().unwrap();
                                        pt_mgr.build_packet_ack(msg_bytes)
                                    };
                                    udp::send(&socket_recv, &ack_bytes, src_addr).await;
                                }
                                send_status_update(
                                    &status_tx_recv,
                                    StatusUpdate::AttachBlobReceived {
                                        conversation_token,
                                        content_hash,
                                        sealed,
                                        sender_pubkey: DevicePubkey::from_bytes(sender_pubkey),
                                        sender_addr: src_addr,
                                    },
                                    &event_proxy_recv,
                                );
                                continue;
                            }
                            if let Ok(((_tok, content_hash), sender_pubkey)) =
                                crate::network::fgtw::protocol::parse_attach_have_vsf(msg_bytes)
                            {
                                {
                                    let ack_bytes = {
                                        let pt_mgr = pt_recv.lock().unwrap();
                                        pt_mgr.build_packet_ack(msg_bytes)
                                    };
                                    udp::send(&socket_recv, &ack_bytes, src_addr).await;
                                }
                                send_status_update(
                                    &status_tx_recv,
                                    StatusUpdate::AttachHaveReceived {
                                        content_hash,
                                        sender_pubkey: DevicePubkey::from_bytes(sender_pubkey),
                                    },
                                    &event_proxy_recv,
                                );
                                continue;
                            }
                            if let Ok(((conversation_token, content_hash), sender_pubkey)) =
                                crate::network::fgtw::protocol::parse_attach_req_vsf(msg_bytes)
                            {
                                {
                                    let ack_bytes = {
                                        let pt_mgr = pt_recv.lock().unwrap();
                                        pt_mgr.build_packet_ack(msg_bytes)
                                    };
                                    udp::send(&socket_recv, &ack_bytes, src_addr).await;
                                }
                                send_status_update(
                                    &status_tx_recv,
                                    StatusUpdate::AttachReqReceived {
                                        conversation_token,
                                        content_hash,
                                        sender_pubkey: DevicePubkey::from_bytes(sender_pubkey),
                                        sender_addr: src_addr,
                                    },
                                    &event_proxy_recv,
                                );
                                continue;
                            }
                            // BRIDGE: a remote-terminal frame. Same mandatory packet-ack; the UI opens the payload + authorizes.
                            if let Ok(((session_id, kind, sealed_payload), sender_pubkey)) =
                                crate::network::fgtw::protocol::parse_term_vsf(msg_bytes)
                            {
                                {
                                    let ack_bytes = {
                                        let pt_mgr = pt_recv.lock().unwrap();
                                        pt_mgr.build_packet_ack(msg_bytes)
                                    };
                                    udp::send(&socket_recv, &ack_bytes, src_addr).await;
                                }
                                send_status_update(
                                    &status_tx_recv,
                                    StatusUpdate::TermReceived {
                                        session_id,
                                        kind,
                                        sealed_payload,
                                        sender_pubkey: DevicePubkey::from_bytes(sender_pubkey),
                                        sender_addr: src_addr,
                                    },
                                    &event_proxy_recv,
                                );
                                continue;
                            }
                            // Sibling chain-reset (fork repair, ~200B). Same mandatory packet-ack as hist_page — it rides the reliable queue.
                            if let Ok(((conversation_token, sealed), sender_pubkey)) =
                                crate::network::fgtw::protocol::parse_chain_reset_vsf(msg_bytes)
                            {
                                {
                                    let ack_bytes = {
                                        let pt_mgr = pt_recv.lock().unwrap();
                                        pt_mgr.build_packet_ack(msg_bytes)
                                    };
                                    udp::send(&socket_recv, &ack_bytes, src_addr).await;
                                }
                                send_status_update(
                                    &status_tx_recv,
                                    StatusUpdate::ChainResetReceived {
                                        conversation_token,
                                        sealed,
                                        sender_pubkey: DevicePubkey::from_bytes(sender_pubkey),
                                        sender_addr: src_addr,
                                    },
                                    &event_proxy_recv,
                                );
                                continue;
                            }
                            // Blind frames (blind_put/ack/get/srv, ≤~400B — always this small-frame path). Same MANDATORY packet-ack: they ride send_with_pubkey's reliable queue; an un-acked type retransmits forever and head-of-line-blocks chat.
                            if let Some((kind, payload, sender_pubkey)) =
                                crate::network::fgtw::protocol::parse_any_blind_frame(msg_bytes)
                            {
                                {
                                    let ack_bytes = {
                                        let pt_mgr = pt_recv.lock().unwrap();
                                        pt_mgr.build_packet_ack(msg_bytes)
                                    };
                                    udp::send(&socket_recv, &ack_bytes, src_addr).await;
                                }
                                send_status_update(
                                    &status_tx_recv,
                                    StatusUpdate::BlindFrameReceived {
                                        kind,
                                        conversation_token: payload.conversation_token,
                                        request_id: payload.request_id,
                                        blob: payload.blob,
                                        found: payload.found,
                                        sent_osc: payload.sent_osc,
                                        sender_pubkey: DevicePubkey::from_bytes(sender_pubkey),
                                        sender_addr: src_addr,
                                    },
                                    &event_proxy_recv,
                                );
                                continue;
                            }
                        }
                    }

                    match FgtwMessage::from_vsf_bytes(msg_bytes) {
                        Ok(message) => {
                            // Delivery ack for EVERY message sent thru PT's reliable stop-and-wait queue (send_with_pubkey), keyed by BLAKE3(bytes), so the sender stops retransmitting and its per-peer FIFO advances. This MUST list every reliably-queued type or that type retransmits FOREVER and head-of-line-blocks chat behind it. AvatarRequest/AvatarResponse were the missing entries: both go out via send_with_pubkey but were never acked, so a post-CLUTCH avatar request retransmitted until it blocked every chat message — the "messages stick" bug, hit hardest against an avatar-less peer that also sends no app-level response. Ping/pong are excluded on purpose: best-effort on their own schedule, NOT queued reliably, so acking them would be pointless noise. CLUTCH proof (ClutchComplete) is acked earlier in its own branch. Packet-acks are pt_ack frames handled earlier (no ack-of-ack). The delivery ack is pure transport "bytes received"; the app still sends its own semantic reply (MessageAck / avatar response).
                            let reliable = matches!(
                                message,
                                FgtwMessage::ChatMessage { .. }
                                    | FgtwMessage::MessageAck { .. }
                                    | FgtwMessage::AvatarRequest { .. }
                                    | FgtwMessage::AvatarResponse { .. }
                            );
                            if reliable {
                                let ack_bytes = {
                                    let pt_mgr = pt_recv.lock().unwrap();
                                    pt_mgr.build_packet_ack(msg_bytes)
                                };
                                if !ack_bytes.is_empty() {
                                    udp::send(&socket_recv, &ack_bytes, src_addr).await;
                                }
                            }
                            match message {
                                FgtwMessage::StatusPing {
                                    timestamp: _,
                                    sender_pubkey,
                                    provenance_hash,
                                    signature,
                                } => {
                                    // Only respond to contacts (friends only)
                                    let is_contact = {
                                        let list = contacts_recv.lock().unwrap();
                                        list.iter().any(|p| *p == sender_pubkey)
                                    };
                                    if !is_contact {
                                        continue;
                                    }

                                    // Verify signature
                                    if !verify_provenance_signature(
                                        &provenance_hash,
                                        &sender_pubkey,
                                        &signature,
                                    ) {
                                        continue;
                                    }

                                    // Reset failure counter - they're clearly online if they're pinging us
                                    {
                                        let mut failures = failed_pings_recv.lock().unwrap();
                                        failures.retain(|(k, _)| k != sender_pubkey.as_bytes());
                                    }

                                    // PING REFLECTION — the probe arm's asymmetry killer, on the steadier signal. A DIRECT ping from a known device proves src_addr is a working return path (their NAT opened it toward us), and for a relay-only pair it is the ONLY direct frame that ever arrives: the side with the validated path keeps it warm with pings, so the probe arm's reflection never gets a trigger (field 2026-08-13: Mary direct→Nick while both Nick devices held only her unreachable LAN row and pinged relay forever — both fleets publish self-claimed :4383 records no NAT honours). Probe back at the proven source; the ack validates OUR direction, and its observed-addr echo teaches us the true public mapping our next announce publishes. Relay-injected pings carry the sentinel address, which is_bogus_addr rejects.
                                    if sender_pubkey != our_pubkey_recv
                                        && !crate::network::traverse::gather::is_bogus_addr(
                                            &src_addr,
                                        )
                                    {
                                        let now = std::time::Instant::now();
                                        reverse_probed.retain(|(_, at)| {
                                            now.duration_since(*at)
                                                < std::time::Duration::from_secs(60)
                                        });
                                        if !reverse_probed
                                            .iter()
                                            .any(|(pk, _)| pk == sender_pubkey.as_bytes())
                                        {
                                            reverse_probed.push((*sender_pubkey.as_bytes(), now));
                                            let mut nonce = [0u8; 32];
                                            nonce.copy_from_slice(
                                                blake3::hash(src_addr.to_string().as_bytes())
                                                    .as_bytes(),
                                            );
                                            let (probe_bytes, provenance) =
                                                crate::network::traverse::punch::build_probe(
                                                    &keypair_recv,
                                                    our_pubkey_recv.clone(),
                                                    nonce,
                                                );
                                            {
                                                let mut probes =
                                                    pending_probes_recv.lock().unwrap();
                                                probes.insert(
                                                    provenance,
                                                    sender_pubkey.clone(),
                                                    src_addr,
                                                    now,
                                                );
                                            }
                                            udp::send(&socket_recv, &probe_bytes, src_addr).await;
                                            crate::logf!("TRAVERSE: reflecting probe at pinger {} — their direct ping proved the address, validating our own direction", src_addr);
                                        }
                                    }

                                    // Mark sender as online (they pinged us, so they're online!) No sync_records from ping - we'll send our sync info in pong
                                    send_status_update(
                                        &status_tx_recv,
                                        StatusUpdate::Online {
                                            peer_pubkey: sender_pubkey.clone(),
                                            is_online: true,
                                            peer_addr: Some(src_addr),
                                            sync_records: vec![],
                                            display_name: None,
                                            avatar_pin: None,
                                            locked_reports: Vec::new(),
                                        },
                                        &event_proxy_recv,
                                    );

                                    // Send pong (no avatar_id - avatars are fetched by handle)
                                    let sig = keypair_recv.sign(&provenance_hash);
                                    let mut sig_bytes = [0u8; 64];
                                    sig_bytes.copy_from_slice(&sig.to_bytes());

                                    // Seal the sensitive tail — sync rows, name, avatar pin — to the PINGING device: the pong answers this specific ping, so it seals under exactly that device's pairwise key. No key yet (contact still loading, or a legacy/unknown device) → the pong carries NO sensitive tail at all: presence still works, and the tail arrives once the UI seeds the key. The inputs come from the same places as ever (provider for sync records, statics for own name/pin) — only the wire encoding changed.
                                    let seal_key = {
                                        let keys = pong_seal_keys_recv.lock().unwrap();
                                        keys.get(sender_pubkey.as_bytes()).copied()
                                    };
                                    let sealed = seal_key.and_then(|key| {
                                        let records = {
                                            let records = sync_records_recv.lock().unwrap();
                                            records.clone()
                                        };
                                        match crate::network::fgtw::protocol::seal_pong_sensitive(
                                            &records,
                                            profile_name().as_deref(),
                                            avatar_pin().as_ref(),
                                            &locked_report(),
                                            &key,
                                        ) {
                                            Ok(blob) => Some(blob),
                                            Err(e) => {
                                                crate::logf!("Status: pong tail seal failed for {} — sending tail-less: {}", crate::fp(sender_pubkey.as_bytes()), e);
                                                None
                                            }
                                        }
                                    });

                                    // Reflexive echo: tell the pinger the source address we saw its ping arrive from (canonicalised out of the dual-stack `::ffff:` form). This is the peer-echoed STUN primitive — the pinger learns its own public address on the exact UDP socket data flows over. Stays plaintext on purpose: it is the pinger's own address bootstrapping information, useful before any pairing exists.
                                    let pong = FgtwMessage::StatusPong {
                                        timestamp: eagle_time_now(),
                                        responder_pubkey: our_pubkey_recv.clone(),
                                        provenance_hash,
                                        signature: sig_bytes,
                                        sync_records: Vec::new(),
                                        observed_addr: Some(udp::canon_socketaddr(src_addr)),
                                        display_name: None,
                                        avatar_pin: None,
                                        sealed,
                                    };

                                    let pong_bytes = pong.to_vsf_bytes();
                                    if !pong_bytes.is_empty() {
                                        // Route back the way the ping came: UDP if direct, relay-pipe to the pinger's device key if this ping arrived over the relay (RELAY_ADDR).
                                        relay_reply(
                                            &socket_recv,
                                            &keypair_recv,
                                            src_addr,
                                            sender_pubkey.as_bytes(),
                                            &pong_bytes,
                                        )
                                        .await;
                                    }
                                }

                                FgtwMessage::StatusPong {
                                    timestamp: _,
                                    responder_pubkey,
                                    provenance_hash,
                                    signature,
                                    sync_records,
                                    observed_addr,
                                    display_name,
                                    avatar_pin,
                                    sealed,
                                } => {
                                    // Find and remove matching pending ping
                                    let pending_ping = {
                                        let mut list = pending_recv.lock().unwrap();
                                        if let Some(idx) = list
                                            .iter()
                                            .position(|p| p.provenance_hash == provenance_hash)
                                        {
                                            Some(list.swap_remove(idx))
                                        } else {
                                            None
                                        }
                                    };

                                    // Torch every drop: a pong dying silently in one of these gates is indistinguishable from "peer never answers" in the field (observed: a contact sat TIMEOUT for 20 minutes while its punch acks flowed fine — whether its pongs were absent or discarded was unknowable from the log).
                                    let pending_ping = match pending_ping {
                                        Some(p) => p,
                                        None => {
                                            // LIVENESS SALVAGE: an unmatched pong (doze-delayed past expiry, an answered race twin, a fan-out duplicate) still PROVES the signing device is alive — discarding that fact kept siblings "offline" for whole sessions (hundreds of dropped pongs per day, the fleet-push killswitch + the amber/green ring flap). Verify the signature and count presence ONLY: no address adoption (the source isn't freshness-proven without the nonce match — a replayed pong from an attacker's address could poison the contact's ip), no sync/name/pin (those ride matched pongs). Strikes reset like any live verdict so dead-address fan-out pings can't out-vote a living device.
                                            if verify_provenance_signature(
                                                &provenance_hash,
                                                &responder_pubkey,
                                                &signature,
                                            ) {
                                                {
                                                    let mut failures =
                                                        failed_pings_recv.lock().unwrap();
                                                    failures.retain(|(k, _)| {
                                                        k != responder_pubkey.as_bytes()
                                                    });
                                                }
                                                // SYNC RECORDS RIDE THE SALVAGE: the sealed tail is device-authenticated by the pairwise AEAD independent of ping-nonce freshness, and lane tips / row digests are replay-safe testimony (a replayed OLD tip clears strictly less; a stale digest at worst re-arms one recovery walk). Dropping them here starved receivers whose pongs consistently race to the wrong provenance — 127 salvaged pongs in one session, ZERO records processed, so the tip-clear/anchor-heal never saw the peer's heads (round-8 field, 2026-08-17). Name/pin/locked stay matched-pong-only: those mutate identity/trust state and lean on the nonce for replay protection.
                                                let salvaged = sealed
                                                    .as_ref()
                                                    .and_then(|blob| {
                                                        let key = pong_seal_keys_recv
                                                            .lock()
                                                            .unwrap()
                                                            .get(responder_pubkey.as_bytes())
                                                            .copied();
                                                        key.and_then(|k| {
                                                            crate::network::fgtw::protocol::open_pong_sensitive(blob, &k).ok()
                                                        })
                                                    })
                                                    .map(|(recs, _, _, _)| recs)
                                                    .unwrap_or_default();
                                                crate::logf!("Status: unmatched pong from {} ({}) — liveness + {} sync record(s) (late/twin; no addr adoption)", crate::fp(responder_pubkey.as_bytes()), src_addr, salvaged.len());
                                                send_status_update(
                                                    &status_tx_recv,
                                                    StatusUpdate::Online {
                                                        peer_pubkey: responder_pubkey,
                                                        is_online: true,
                                                        peer_addr: None,
                                                        sync_records: salvaged,
                                                        display_name: None,
                                                        avatar_pin: None,
                                                        locked_reports: Vec::new(),
                                                    },
                                                    &event_proxy_recv,
                                                );
                                            } else {
                                                crate::logf!("Status: pong from {} ({}) dropped — unmatched provenance AND unverifiable signature", crate::fp(responder_pubkey.as_bytes()), src_addr);
                                            }
                                            continue;
                                        }
                                    };

                                    // Verify responder matches who we pinged
                                    if responder_pubkey != pending_ping.recipient_pubkey {
                                        // Another device answered this provenance (a fleet sibling heard the fan-out, or a stale contact record routed the ping). The RESPONDER is provably alive if its signature holds — salvage that as presence-only, same terms as the unmatched arm. And the consumed pending entry still belongs to its intended recipient: put it back so their answer (or honest timeout) isn't silently voided.
                                        if verify_provenance_signature(
                                            &provenance_hash,
                                            &responder_pubkey,
                                            &signature,
                                        ) {
                                            {
                                                let mut failures =
                                                    failed_pings_recv.lock().unwrap();
                                                failures.retain(|(k, _)| {
                                                    k != responder_pubkey.as_bytes()
                                                });
                                            }
                                            // Same salvage as the unmatched arm: the RESPONDER's sealed tail is its own authenticated testimony — a fleet answering fan-out pings from devices we didn't name was the ONLY pong source some sessions ever saw, and 'liveness only' meant zero sync records all session (round-8 field, 2026-08-17). Tips/digests only; name/pin/locked wait for a matched pong.
                                            let salvaged = sealed
                                                .as_ref()
                                                .and_then(|blob| {
                                                    let key = pong_seal_keys_recv
                                                        .lock()
                                                        .unwrap()
                                                        .get(responder_pubkey.as_bytes())
                                                        .copied();
                                                    key.and_then(|k| {
                                                        crate::network::fgtw::protocol::open_pong_sensitive(blob, &k).ok()
                                                    })
                                                })
                                                .map(|(recs, _, _, _)| recs)
                                                .unwrap_or_default();
                                            crate::logf!("Status: pong answered by {} but we pinged {} — responder counted alive + {} sync record(s), ping re-armed for its recipient", crate::fp(responder_pubkey.as_bytes()), crate::fp(pending_ping.recipient_pubkey.as_bytes()), salvaged.len());
                                            send_status_update(
                                                &status_tx_recv,
                                                StatusUpdate::Online {
                                                    peer_pubkey: responder_pubkey,
                                                    is_online: true,
                                                    peer_addr: None,
                                                    sync_records: salvaged,
                                                    display_name: None,
                                                    avatar_pin: None,
                                                    locked_reports: Vec::new(),
                                                },
                                                &event_proxy_recv,
                                            );
                                        } else {
                                            crate::logf!("Status: pong answered by {} but we pinged {} — signature unverifiable, dropped", crate::fp(responder_pubkey.as_bytes()), crate::fp(pending_ping.recipient_pubkey.as_bytes()));
                                        }
                                        pending_recv.lock().unwrap().push(pending_ping);
                                        continue;
                                    }

                                    // Verify signature
                                    if !verify_provenance_signature(
                                        &provenance_hash,
                                        &responder_pubkey,
                                        &signature,
                                    ) {
                                        crate::logf!("Status: pong from {} dropped — provenance signature failed", crate::fp(responder_pubkey.as_bytes()));
                                        continue;
                                    }

                                    // Peer-echoed reflexive address, from a pong we just signature-verified: OUR public address as this contact saw our ping arrive on the data socket. The pong is contact-gated, so the echo is from a friend → trusted, adopt immediately. On an adoption change, push it to the app as `our_reflexive` (feeds candidate gathering + the announce).
                                    if let Some(obs) = observed_addr {
                                        if let Some(addr) = reflexive.record(
                                            udp::canon_socketaddr(obs),
                                            *responder_pubkey.as_bytes(),
                                            true,
                                        ) {
                                            crate::logf!("TRAVERSE: reflexive learned = {}", addr);
                                            send_status_update(
                                                &status_tx_recv,
                                                StatusUpdate::ReflexiveLearned { addr },
                                                &event_proxy_recv,
                                            );
                                        }
                                    }

                                    // Reset failure counter on successful pong (prevents bouncing) — and purge the device's OTHER still-pending pings. Each cycle fans pings across every known address (validated + LAN + public); the ones aimed at dead addresses expire 5s later and were each counted as a "consecutive failure", so a device answering perfectly on its LAN path still accrued strikes from its rotated cell address and flapped offline every few cycles (observed as hundreds of offline marks against a handful of online in a single session). One live path answering = the device is alive; the dead paths' pings must not outlive that verdict.
                                    {
                                        let mut failures = failed_pings_recv.lock().unwrap();
                                        failures.retain(|(k, _)| k != responder_pubkey.as_bytes());
                                        let mut list = pending_recv.lock().unwrap();
                                        list.retain(|p| p.recipient_pubkey != responder_pubkey);
                                    }

                                    // Sensitive tail: an updated peer sends it ONLY sealed — open with the RESPONDING device's pairwise key (the signer, verified just above). A failed open (key not seeded yet, or a stale key across their re-attest) degrades to a tail-less pong: presence still lands, name/pin/sync simply wait for keys — and it logs once per device, not per pong. A legacy peer still sends the plaintext fields; keep honouring them until it updates.
                                    let (sync_records, display_name, avatar_pin, locked_reports) =
                                        match sealed {
                                            Some(blob) => {
                                                let key = {
                                                    let keys = pong_seal_keys_recv.lock().unwrap();
                                                    keys.get(responder_pubkey.as_bytes()).copied()
                                                };
                                                let opened = key.and_then(|k| {
                                                crate::network::fgtw::protocol::open_pong_sensitive(
                                                    &blob, &k,
                                                )
                                                .ok()
                                            });
                                                match opened {
                                                    Some(tail) => {
                                                        pong_open_failed.retain(|d| {
                                                            d != responder_pubkey.as_bytes()
                                                        });
                                                        tail
                                                    }
                                                    None => {
                                                        if !pong_open_failed
                                                            .contains(responder_pubkey.as_bytes())
                                                        {
                                                            pong_open_failed
                                                                .push(*responder_pubkey.as_bytes());
                                                            crate::logf!("Status: sealed pong tail from {} unopenable ({}) — treating as tail-less until keys agree", crate::fp(responder_pubkey.as_bytes()), if key.is_some() { "key mismatch" } else { "no pairwise key seeded yet" });
                                                            // Ask the UI thread to reseed the seal map — self-heal for the fresh-device ordering race (the map fills in fold order; a pong racing ahead stayed tail-less forever).
                                                            send_status_update(
                                                                &status_tx_recv,
                                                                StatusUpdate::PongSealMissing {
                                                                    device: responder_pubkey
                                                                        .clone(),
                                                                },
                                                                &event_proxy_recv,
                                                            );
                                                        }
                                                        (Vec::new(), None, None, Vec::new())
                                                    }
                                                }
                                            }
                                            // Legacy plaintext pong: no sealed tail, so no reported-stolen signal either — the report is trusted only under the pairwise seal.
                                            None => {
                                                (sync_records, display_name, avatar_pin, Vec::new())
                                            }
                                        };

                                    // Send status update with sync_records for retransmit handling
                                    send_status_update(
                                        &status_tx_recv,
                                        StatusUpdate::Online {
                                            peer_pubkey: responder_pubkey,
                                            is_online: true,
                                            peer_addr: Some(src_addr),
                                            sync_records,
                                            display_name,
                                            avatar_pin,
                                            locked_reports,
                                        },
                                        &event_proxy_recv,
                                    );
                                }

                                // NOTE: ClutchOffer, ClutchInit, ClutchResponse, ClutchComplete handlers REMOVED Full 8-primitive CLUTCH uses TCP with ClutchOfferReceived and ClutchKemResponseReceived See docs/clutch.md Section 4.2 for the slot-based ceremony protocol.
                                FgtwMessage::ChatMessage {
                                    timestamp,
                                    conversation_token,
                                    lane,
                                    prev_msg_hp,
                                    ciphertext,
                                    sender_pubkey,
                                    signature,
                                } => {
                                    // KNOWN DEVICE ONLY, and BEFORE the presence flip. The frame is self-authenticating (the signature proves possession of the signing key, nothing about WHO), so without this gate any key could flip a peer online, register an address, and inject an unknown-lane frame that drives the receiver's fork detectors — mirror the StatusPing allowlist gate. The full known∧not-refused decision runs on the UI thread (where refused_devices/locked_out live), keyed by sender_pubkey carried below.
                                    let is_contact = {
                                        let list = contacts_recv.lock().unwrap();
                                        list.iter().any(|p| *p == sender_pubkey)
                                    };
                                    if !is_contact {
                                        continue;
                                    }

                                    // Verify signature (CHAIN format provenance)
                                    let provenance =
                                        compute_chat_provenance(&conversation_token, &prev_msg_hp);
                                    if !verify_provenance_signature(
                                        &provenance,
                                        &sender_pubkey,
                                        &signature,
                                    ) {
                                        continue;
                                    }

                                    // Twin collapse (see recent_chat_frames above): the same frame arriving via direct AND relay inside the window is one message, not two.
                                    {
                                        let now = std::time::Instant::now();
                                        recent_chat_frames.retain(|(_, at)| {
                                            now.duration_since(*at) < CHAT_TWIN_WINDOW
                                        });
                                        let mut token8 = [0u8; 8];
                                        token8.copy_from_slice(&conversation_token[..8]);
                                        let mut ct8 = [0u8; 8];
                                        ct8.copy_from_slice(
                                            &blake3::hash(&ciphertext).as_bytes()[..8],
                                        );
                                        let key = (token8, timestamp, ct8);
                                        if recent_chat_frames.iter().any(|(k, _)| *k == key) {
                                            crate::logf!("Status: collapsed twin chat frame (eagle_time {}) from {}", timestamp, src_addr);
                                            continue;
                                        }
                                        recent_chat_frames.push((key, now));
                                    }

                                    // A chat IS liveness proof — stronger than a pong. Clear the sender's ping-failure counter and mark them online, so the ping-timeout can't flip a peer offline while they're actively messaging us (the "shows offline but receives messages" bug).
                                    {
                                        let mut failures = failed_pings_recv.lock().unwrap();
                                        failures.retain(|(k, _)| k != sender_pubkey.as_bytes());
                                    }
                                    send_status_update(
                                        &status_tx_recv,
                                        StatusUpdate::Online {
                                            peer_pubkey: sender_pubkey.clone(),
                                            is_online: true,
                                            peer_addr: Some(src_addr),
                                            sync_records: vec![],
                                            display_name: None,
                                            avatar_pin: None,
                                            locked_reports: Vec::new(),
                                        },
                                        &event_proxy_recv,
                                    );

                                    // NO notification from here: this layer sees only ciphertext, so it can't tell a real friend message from a chain probe or a sibling fleet-sync frame (the over-ding bug), and it can't name the sender. Notifications now fire POST-DECRYPT from the UI receive path (photon_app's ChatMessage handling) with real sender + text — the pre-decrypt path carries nothing because it no longer notifies. On Android that path still runs while backgrounded via the service tick, so the "notification matters exactly when the Activity is dead" property is preserved.

                                    // Forward to UI for decryption
                                    send_status_update(
                                        &status_tx_recv,
                                        StatusUpdate::ChatMessage {
                                            conversation_token,
                                            lane,
                                            prev_msg_hp,
                                            ciphertext,
                                            timestamp,
                                            sender_addr: src_addr,
                                            sender_pubkey,
                                        },
                                        &event_proxy_recv,
                                    );
                                }

                                FgtwMessage::MessageAck {
                                    timestamp: _,
                                    conversation_token,
                                    acked_eagle_time,
                                    plaintext_hash,
                                    sender_pubkey,
                                    signature,
                                } => {
                                    crate::logf!(
                                        "Status: MESSAGE_ACK received from {} (eagle_time {})",
                                        src_addr,
                                        acked_eagle_time
                                    );

                                    // Verify signature (CHAIN format provenance)
                                    let provenance = compute_ack_provenance_v2(
                                        &conversation_token,
                                        acked_eagle_time,
                                        &plaintext_hash,
                                    );
                                    if !verify_provenance_signature(
                                        &provenance,
                                        &sender_pubkey,
                                        &signature,
                                    ) {
                                        crate::log("  -> REJECTED (bad signature)");
                                        continue;
                                    }

                                    send_status_update(
                                        &status_tx_recv,
                                        StatusUpdate::MessageAck {
                                            conversation_token,
                                            acked_eagle_time,
                                            plaintext_hash,
                                        },
                                        &event_proxy_recv,
                                    );
                                }

                                FgtwMessage::AvatarRequest {
                                    timestamp,
                                    sender_pubkey,
                                    provenance_hash,
                                    signature,
                                } => {
                                    crate::logf!(
                                        "Status: AVATAR_REQUEST received from {}",
                                        src_addr
                                    );

                                    // Verify provenance binds sender_pubkey + timestamp, then the signature
                                    let provenance: [u8; 32] = blake3::hash(
                                        &[
                                            sender_pubkey.as_bytes().as_slice(),
                                            &timestamp.to_le_bytes(),
                                        ]
                                        .concat(),
                                    )
                                    .into();
                                    if provenance != provenance_hash
                                        || !verify_provenance_signature(
                                            &provenance_hash,
                                            &sender_pubkey,
                                            &signature,
                                        )
                                    {
                                        crate::log("  -> REJECTED (bad signature)");
                                        continue;
                                    }

                                    send_status_update(
                                        &status_tx_recv,
                                        StatusUpdate::AvatarRequestReceived {
                                            sender_pubkey,
                                            sender_addr: src_addr,
                                        },
                                        &event_proxy_recv,
                                    );
                                }

                                FgtwMessage::AvatarResponse {
                                    timestamp: _,
                                    responder_pubkey,
                                    provenance_hash,
                                    signature,
                                    avatar_vsf,
                                } => {
                                    crate::logf!("Status: AVATAR_RESPONSE received from {} ({} bytes avatar)", src_addr, avatar_vsf.len());

                                    // Verify provenance is the avatar bytes' hash, then the signature
                                    let provenance: [u8; 32] = blake3::hash(&avatar_vsf).into();
                                    if provenance != provenance_hash
                                        || !verify_provenance_signature(
                                            &provenance_hash,
                                            &responder_pubkey,
                                            &signature,
                                        )
                                    {
                                        crate::log("  -> REJECTED (bad signature)");
                                        continue;
                                    }

                                    send_status_update(
                                        &status_tx_recv,
                                        StatusUpdate::AvatarReceived {
                                            responder_pubkey,
                                            avatar_vsf,
                                            sender_addr: src_addr,
                                        },
                                        &event_proxy_recv,
                                    );
                                }

                                FgtwMessage::Reflect {
                                    timestamp: _,
                                    sender_pubkey,
                                    provenance_hash,
                                    signature,
                                } => {
                                    // Open-tier STUN: answer ANY signed node (not just contacts) with the source address we observed the request arrive from — this is what lets peers reflect for one another so nobody needs a central STUN server. Reveals only the requester's own address, so it's safe to serve openly; the P4 "serve directory" toggle will gate it (default on).
                                    if !verify_provenance_signature(
                                        &provenance_hash,
                                        &sender_pubkey,
                                        &signature,
                                    ) {
                                        continue;
                                    }
                                    let sig = keypair_recv.sign(&provenance_hash);
                                    let mut sig_bytes = [0u8; 64];
                                    sig_bytes.copy_from_slice(&sig.to_bytes());
                                    let resp = FgtwMessage::ReflectResponse {
                                        timestamp: eagle_time_now(),
                                        responder_pubkey: our_pubkey_recv.clone(),
                                        provenance_hash,
                                        signature: sig_bytes,
                                        observed_addr: udp::canon_socketaddr(src_addr),
                                    };
                                    let resp_bytes = resp.to_vsf_bytes();
                                    if !resp_bytes.is_empty() {
                                        udp::send(&socket_recv, &resp_bytes, src_addr).await;
                                    }
                                }

                                FgtwMessage::ReflectResponse {
                                    timestamp: _,
                                    responder_pubkey,
                                    provenance_hash,
                                    signature,
                                    observed_addr,
                                } => {
                                    // Open-tier reflexive answer. Verify it's a signed reply, then feed the quorum buffer as UNtrusted — a stranger's claim about our address needs corroboration from a second source before we adopt and re-publish it (anti-poison). A contact's echo arrives via the trusted pong path instead.
                                    if !verify_provenance_signature(
                                        &provenance_hash,
                                        &responder_pubkey,
                                        &signature,
                                    ) {
                                        continue;
                                    }
                                    if let Some(addr) = reflexive.record(
                                        udp::canon_socketaddr(observed_addr),
                                        *responder_pubkey.as_bytes(),
                                        false,
                                    ) {
                                        crate::logf!("TRAVERSE: reflexive learned = {}", addr);
                                        send_status_update(
                                            &status_tx_recv,
                                            StatusUpdate::ReflexiveLearned { addr },
                                            &event_proxy_recv,
                                        );
                                    }
                                }

                                FgtwMessage::PunchProbe {
                                    timestamp: _,
                                    sender_pubkey,
                                    provenance_hash,
                                    signature,
                                } => {
                                    // NEVER answer our OWN probe reflected back (LAN hairpin / multicast loopback). Our own device is in the contacts list (the self-contact + siblings), so it passes the friend gate below — and ACKing it validates a path TO OURSELVES, which poisons addressing exactly like the 0.0.0.0 sentinel did: sends go to our loopback and relay_to empties out. Observed as a device validating a direct path to its own LAN address.
                                    if sender_pubkey == our_pubkey_recv {
                                        continue;
                                    }
                                    // Friend-tier hole-punch: only a contact/fleet member's probe is answered (the data plane is friend-gated, same set as ping). Receiving the probe means their packet traversed our NAT; replying opens ours toward them, and the ack — echoing their provenance — lets them validate this exact `(local, remote)` path. The ack also carries the address we saw, doubling as a reflexive echo for them.
                                    let is_contact = {
                                        let list = contacts_recv.lock().unwrap();
                                        list.iter().any(|p| *p == sender_pubkey)
                                    };
                                    if !is_contact {
                                        continue;
                                    }
                                    if !verify_provenance_signature(
                                        &provenance_hash,
                                        &sender_pubkey,
                                        &signature,
                                    ) {
                                        continue;
                                    }
                                    let ack = crate::network::traverse::punch::build_probe_ack(
                                        &keypair_recv,
                                        our_pubkey_recv.clone(),
                                        provenance_hash,
                                        udp::canon_socketaddr(src_addr),
                                    );
                                    if !ack.is_empty() {
                                        udp::send(&socket_recv, &ack, src_addr).await;
                                    }
                                    // PROBE REFLECTION — the asymmetry killer. Their probe reaching us proves src_addr is a WORKING address for this device (their NAT opened toward us; our ack just rode back thru it) — yet we used to discard it after acking, so path validation was ONE-DIRECTIONAL by construction: the side with good candidates validated, the side without stayed relay-only forever (field, 2026-08-11: one end's published record carried no LAN address, so its peer probed only an unreachable WAN — "online but no direct path after 3 cycles — pending relay" — while its own probes kept arriving perfectly). Fire our own probe at the proven source; the existing ack machinery then validates OUR direction (PathValidated → endpoint election), no new wire format, no new trust: the probe was signature-verified against a known device above.
                                    if !crate::network::traverse::gather::is_bogus_addr(&src_addr) {
                                        let now = std::time::Instant::now();
                                        reverse_probed.retain(|(_, at)| {
                                            now.duration_since(*at)
                                                < std::time::Duration::from_secs(60)
                                        });
                                        if !reverse_probed
                                            .iter()
                                            .any(|(pk, _)| pk == sender_pubkey.as_bytes())
                                        {
                                            reverse_probed.push((*sender_pubkey.as_bytes(), now));
                                            let mut nonce = [0u8; 32];
                                            nonce.copy_from_slice(
                                                blake3::hash(src_addr.to_string().as_bytes())
                                                    .as_bytes(),
                                            );
                                            let (probe_bytes, provenance) =
                                                crate::network::traverse::punch::build_probe(
                                                    &keypair_recv,
                                                    our_pubkey_recv.clone(),
                                                    nonce,
                                                );
                                            {
                                                let mut probes =
                                                    pending_probes_recv.lock().unwrap();
                                                probes.insert(
                                                    provenance,
                                                    sender_pubkey.clone(),
                                                    src_addr,
                                                    now,
                                                );
                                            }
                                            udp::send(&socket_recv, &probe_bytes, src_addr).await;
                                            crate::logf!("TRAVERSE: reflecting probe back at {} — their probe proved the address, validating our own direction", src_addr);
                                        }
                                    }
                                }

                                FgtwMessage::PunchProbeAck {
                                    timestamp: _,
                                    responder_pubkey,
                                    provenance_hash,
                                    signature,
                                    observed_addr,
                                } => {
                                    // A hole-punch we sent round-tripped. Gate on contact/fleet + signature, fold the reflexive echo the ack carries (trusted — from a contact), then match it to the probe we sent (by provenance) to report the validated direct path.
                                    let is_contact = {
                                        let list = contacts_recv.lock().unwrap();
                                        list.iter().any(|p| *p == responder_pubkey)
                                    };
                                    if !is_contact {
                                        continue;
                                    }
                                    if !verify_provenance_signature(
                                        &provenance_hash,
                                        &responder_pubkey,
                                        &signature,
                                    ) {
                                        continue;
                                    }
                                    if let Some(addr) = reflexive.record(
                                        udp::canon_socketaddr(observed_addr),
                                        *responder_pubkey.as_bytes(),
                                        true,
                                    ) {
                                        crate::logf!("TRAVERSE: reflexive learned = {}", addr);
                                        send_status_update(
                                            &status_tx_recv,
                                            StatusUpdate::ReflexiveLearned { addr },
                                            &event_proxy_recv,
                                        );
                                    }
                                    // Resolve the probe → validated path. The address we sent to (`target`) is what we'll use to reach them; the ack's src confirms reachability. `resolve` removes the entry so a replayed ack can't re-validate.
                                    let resolved = {
                                        pending_probes_recv
                                            .lock()
                                            .unwrap()
                                            .resolve(&provenance_hash)
                                    };
                                    if let Some((peer, target)) = resolved {
                                        crate::logf!(
                                            "TRAVERSE: ACK from {} — path validated {}",
                                            src_addr,
                                            target
                                        );
                                        send_status_update(
                                            &status_tx_recv,
                                            StatusUpdate::PathValidated {
                                                peer_pubkey: peer,
                                                remote: target,
                                            },
                                            &event_proxy_recv,
                                        );
                                    }
                                }

                                FgtwMessage::PhonebookRequest {
                                    timestamp: _,
                                    sender_pubkey,
                                    provenance_hash,
                                    signature,
                                } => {
                                    // Peers-are-FGTW gossip: a contact whose own fgtw is unreachable asks us for the peer records we hold. Friend-gated (same set as ping/probe) + signature-verified. We reply with our self-signed records; each verifies on its own, so this relay is untrusted — we can carry a device's entry but can't forge or redirect it.
                                    let is_contact = {
                                        let list = contacts_recv.lock().unwrap();
                                        list.iter().any(|p| *p == sender_pubkey)
                                    };
                                    if !is_contact {
                                        continue;
                                    }
                                    if !verify_provenance_signature(
                                        &provenance_hash,
                                        &sender_pubkey,
                                        &signature,
                                    ) {
                                        continue;
                                    }
                                    let peers = peer_store_recv.lock().unwrap().get_all_peers();
                                    // Sign the ECHOED request provenance — proves we saw this exact request and are a valid device; the records carry their own trust.
                                    let sig = keypair_recv.sign(&provenance_hash);
                                    let mut sig_bytes = [0u8; 64];
                                    sig_bytes.copy_from_slice(&sig.to_bytes());
                                    let resp = FgtwMessage::PhonebookResponse {
                                        timestamp: eagle_time_now(),
                                        responder_pubkey: our_pubkey_recv.clone(),
                                        provenance_hash,
                                        signature: sig_bytes,
                                        peers,
                                    };
                                    let resp_bytes = resp.to_vsf_bytes();
                                    if !resp_bytes.is_empty() {
                                        udp::send(&socket_recv, &resp_bytes, src_addr).await;
                                    }
                                }

                                FgtwMessage::PhonebookResponse {
                                    timestamp: _,
                                    responder_pubkey,
                                    provenance_hash: _,
                                    signature: _,
                                    peers,
                                } => {
                                    // A friend answered our phonebook request. Gate on contact (only merge gossip from a friend); each record is self-signed, and `merge_peer` rejects anything that doesn't verify, so a lying responder can't inject forged rows — it can only fail to help. The app harvests the shared store on its next stalled-contact tick.
                                    let is_contact = {
                                        let list = contacts_recv.lock().unwrap();
                                        list.iter().any(|p| *p == responder_pubkey)
                                    };
                                    if !is_contact {
                                        continue;
                                    }
                                    let mut merged = 0usize;
                                    {
                                        let mut store = peer_store_recv.lock().unwrap();
                                        for rec in peers {
                                            if store.merge_peer(rec) {
                                                merged += 1;
                                            }
                                        }
                                    }
                                    if merged > 0 {
                                        crate::logf!(
                                            "GOSSIP: merged {} peer record(s) from {}",
                                            merged,
                                            src_addr
                                        );
                                    }
                                }

                                _ => {
                                    crate::log("Status: Unknown message type received");
                                }
                            }
                        }
                        Err(e) => {
                            // Log full packet hex for debugging
                            let preview: String = msg_bytes
                                .iter()
                                .map(|b| format!("{:02x}", b))
                                .collect::<Vec<_>>()
                                .join(" ");
                            crate::logf!(
                                "Status: Parse error: {} (len={}, hex: {})",
                                e,
                                msg_bytes.len(),
                                preview
                            );
                        }
                    }
                }
                Err(_) => {}
            }
        }
    });

    // Attachment progress throttle state (see the PT tick block).
    let mut last_progress_push = std::time::Instant::now();
    let mut last_progress_snap: Vec<(SocketAddr, u32, u32, bool)> = Vec::new();

    // Main event loop
    loop {
        match ping_rx.try_recv() {
            Ok(request) => {
                let timestamp = eagle_time_now();
                let provenance_hash = compute_provenance_hash(&our_pubkey, timestamp);

                let signature = keypair.sign(&provenance_hash);
                let mut sig_bytes = [0u8; 64];
                sig_bytes.copy_from_slice(&signature.to_bytes());

                // Send ping (no avatar_id - avatars are fetched by handle)
                let ping = FgtwMessage::StatusPing {
                    timestamp,
                    sender_pubkey: our_pubkey.clone(),
                    provenance_hash,
                    signature: sig_bytes,
                };

                let msg_bytes = ping.to_vsf_bytes();
                if msg_bytes.is_empty() {
                    crate::logf!("Status: PING build failed for {}", request.peer_addr);
                    continue;
                }

                // Store pending ping
                {
                    let mut list = pending.lock().unwrap();
                    list.push(PendingPing {
                        recipient_pubkey: request.peer_pubkey.clone(),
                        provenance_hash,
                        sent_at: Instant::now(),
                    });
                }

                udp::send(&socket, &msg_bytes, request.peer_addr).await;

                // UDP-OBSERVED SELF-DISCOVERY (the record-heal bootstrap): while the app holds no UDP-confirmed reflexive, every direct ping carries a Reflect beside it. Any signed node answers with the source it saw; two distinct answers pass the anti-poison quorum, ReflexiveLearned adopts, and the next announce publishes the TRUE mapping instead of the self-claimed bind port (field 2026-08-13: both Nick devices published identical :4383 records no NAT honours, stranding every peer without a working record on relay). Self-extinguishing — the app drops the flag on the first quorum-adopted learn. Reuses the ping's signed provenance; the serve arm only verifies the signature and echoes the observed source.
                if needs_reflect.load(std::sync::atomic::Ordering::Relaxed)
                    && !crate::network::traverse::gather::is_bogus_addr(&request.peer_addr)
                {
                    let reflect = FgtwMessage::Reflect {
                        timestamp,
                        sender_pubkey: our_pubkey.clone(),
                        provenance_hash,
                        signature: sig_bytes,
                    };
                    let reflect_bytes = reflect.to_vsf_bytes();
                    if !reflect_bytes.is_empty() {
                        udp::send(&socket, &reflect_bytes, request.peer_addr).await;
                    }
                }

                // No direct path → also ping over the relay pipe so PRESENCE works for a relay-only peer.
                // The peer receives it on its pipe and pongs back over its own pipe; each side flips the other online (reached_via_relay). Best-effort — a live pipe means the peer is reachable; a dropped frame just means they're offline, which a missed pong already conveys.
                // DETACHED, never awaited in the ping loop. Each relayed ping is an HTTPS round trip to the worker — ~1.3s when the recipient is offline — and awaiting them in sequence serialised the whole presence sweep behind the slowest unreachable contact: 8.1 seconds of frozen UI in one measured stall, which is what a sweep across several dozing peers costs. Presence is best-effort by design, so the result is worth nothing to us here: a live pipe means they are reachable and a dropped frame means they are not, which the missed pong already conveys.
                for dev in &request.relay_to {
                    let kp = keypair.clone();
                    let dev = *dev;
                    let bytes = msg_bytes.clone();
                    tokio::spawn(async move {
                        if let Err(e) =
                            crate::network::fgtw::relay::send_via_relay(&kp, &dev, &bytes).await
                        {
                            crate::logf!("RELAY: ping to {} failed: {}", hex::encode(&dev[..4]), e);
                        }
                    });
                }

                // Fire hole-punch probes at the peer's candidates (piggybacked on the ping cycle). Sending each probe opens our NAT toward that candidate; a friend's ack — matched by provenance in `pending_probes` — validates that path. Candidates arrive best-first, so the first to round-trip (usually the lowest-latency path) wins. The nonce is derived from the candidate so concurrent probes get distinct provenances.
                for cand in &request.punch_candidates {
                    let mut nonce = [0u8; 32];
                    nonce.copy_from_slice(blake3::hash(cand.to_string().as_bytes()).as_bytes());
                    let (probe_bytes, provenance) = crate::network::traverse::punch::build_probe(
                        &keypair,
                        our_pubkey.clone(),
                        nonce,
                    );
                    {
                        let mut probes = pending_probes.lock().unwrap();
                        probes.insert(
                            provenance,
                            request.peer_pubkey.clone(),
                            *cand,
                            Instant::now(),
                        );
                    }
                    udp::send(&socket, &probe_bytes, *cand).await;
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
        }

        // Fire any queued phonebook-gossip requests: ask a reachable peer for the peer records it holds, so a friend we CAN'T reach (our fgtw is flaky) is learned from one we can. Small signed control message, best-effort like a ping; the response merges into the shared store.
        while let Ok(addr) = phonebook_req_rx.try_recv() {
            let ts = eagle_time_now();
            let prov = compute_provenance_hash(&our_pubkey, ts);
            let sig = keypair.sign(&prov);
            let mut sig_bytes = [0u8; 64];
            sig_bytes.copy_from_slice(&sig.to_bytes());
            let req = FgtwMessage::PhonebookRequest {
                timestamp: ts,
                sender_pubkey: our_pubkey.clone(),
                provenance_hash: prov,
                signature: sig_bytes,
            };
            let bytes = req.to_vsf_bytes();
            if !bytes.is_empty() {
                udp::send(&socket, &bytes, addr).await;
            }
        }

        // Drop hole-punch probes that never round-tripped (unreachable candidate / symmetric NAT), so pending_probes doesn't grow unbounded across ping cycles.
        {
            pending_probes.lock().unwrap().expire(Instant::now());
        }

        // Cleanup stale pending pings (older than 5 seconds) Use hysteresis: only mark offline after OFFLINE_THRESHOLD consecutive failures
        {
            let mut list = pending.lock().unwrap();
            let mut failures = failed_pings.lock().unwrap();
            let now = Instant::now();
            let timeout = Duration::from_secs(5);

            // Find expired pings and increment failure counters — ONE strike per device per sweep, however many of its pings expired. The multi-address fan-out (validated + LAN + public) parks several pings per device per cycle; counting each expiry burned the 3-strike "consecutive failures" budget in a single cycle (the field log's same-millisecond 2/3→3/3 pairs), turning one round of dead addresses into an instant offline.
            let mut expired: Vec<_> = list
                .iter()
                .filter(|ping| now.duration_since(ping.sent_at) >= timeout)
                .map(|ping| ping.recipient_pubkey.clone())
                .collect();
            expired.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
            expired.dedup();

            for pubkey in expired {
                let pubkey_bytes = *pubkey.as_bytes();
                // Find or insert entry with linear search
                let count =
                    if let Some(entry) = failures.iter_mut().find(|(k, _)| *k == pubkey_bytes) {
                        entry.1 = entry.1.saturating_add(1);
                        entry.1
                    } else {
                        failures.push((pubkey_bytes, 1));
                        1
                    };

                if count >= OFFLINE_THRESHOLD {
                    // Enough consecutive failures - mark offline
                    crate::logf!(
                        "Status: TIMEOUT ({} consecutive) - {} marked offline",
                        count,
                        hex::encode(&pubkey_bytes[..8])
                    );
                    send_status_update(
                        &status_tx,
                        StatusUpdate::Online {
                            peer_pubkey: pubkey,
                            is_online: false,
                            peer_addr: None,      // No address for offline
                            sync_records: vec![], // No sync for offline
                            display_name: None,
                            avatar_pin: None,
                            locked_reports: Vec::new(),
                        },
                        &event_proxy,
                    );
                    // Reset counter after marking offline (so we can detect coming back online)
                    failures.retain(|(k, _)| *k != pubkey_bytes);
                } else {
                    crate::logf!(
                        "Status: TIMEOUT ({}/{}) - {} (waiting for more failures before offline)",
                        count,
                        OFFLINE_THRESHOLD,
                        hex::encode(&pubkey_bytes[..8])
                    );
                }
            }

            list.retain(|ping| now.duration_since(ping.sent_at) < timeout);
        }

        // NOTE: "Process CLUTCH requests" block REMOVED Full 8-primitive CLUTCH uses ClutchOfferRequest and ClutchKemResponseRequest which are processed below using TCP/PT transport.

        // Process message requests (encrypted chat messages - CHAIN format) Routed thru PT for unified transport (UDP → TCP after 1s → relay fallback)
        while let Ok(request) = message_rx.try_recv() {
            // Use the eagle_time from encryption - nonce is derived from this so we MUST use the same timestamp the sender encrypted with
            let timestamp = request.eagle_time;

            // Compute provenance and sign (CHAIN format)
            let provenance =
                compute_chat_provenance(&request.conversation_token, &request.prev_msg_hp);
            let sig = keypair.sign(&provenance);
            let mut sig_bytes = [0u8; 64];
            sig_bytes.copy_from_slice(&sig.to_bytes());

            crate::logf!(
                "Status: Sending CHAT_MESSAGE to {} (tok {}...) via PT",
                request.peer_addr,
                hex::encode(&request.conversation_token[..4])
            );

            let msg = FgtwMessage::ChatMessage {
                timestamp,
                conversation_token: request.conversation_token,
                lane: request.lane,
                prev_msg_hp: request.prev_msg_hp,
                ciphertext: request.ciphertext,
                sender_pubkey: our_pubkey.clone(),
                signature: sig_bytes,
            };

            let msg_bytes = msg.to_vsf_bytes();
            if !msg_bytes.is_empty() {
                // Direct leg — ONLY for a peer that actually has a direct address. A relay-only peer arrives here carrying `RELAY_ADDR` (the caller says so explicitly: "peer_addr is unused for delivery here … the relay_to carries it"), and handing that to PT's RELIABLE queue enqueues a packet that can never be ACKed, burns the whole retry ladder, and head-of-line-blocks the real messages behind it — 86% of one device's PT retransmits were exactly this.
                // Same guard the history drain below already applies for the same reason; the relay fan-out that follows is the delivery path for these peers and is deliberately left outside the `if`.
                if !crate::network::traverse::gather::is_bogus_addr(&request.peer_addr) {
                    // Route thru PT - handles UDP, TCP after 1s, relay fallback
                    let pt_bytes = {
                        let mut pt_mgr = pt.lock().unwrap();
                        pt_mgr.send_with_pubkey(
                            request.peer_addr,
                            msg_bytes.clone(),
                            Some(request.recipient_pubkey),
                        )
                    };
                    // PT returns the first wire bytes to send, or EMPTY if this packet queued behind an in-flight one for this peer (stop-and-wait) — in that case tick() sends it once the head is acked. Don't emit an empty datagram.
                    if !pt_bytes.is_empty() {
                        udp::send(&socket, &pt_bytes, request.peer_addr).await;
                        // Race the alt path with the SAME wire bytes (best-effort duplicate, not a second PT transfer). The reachable address delivers; the receiver dedupes by eagle_time and its ACK is deterministic, so a redelivery just yields a free re-ACK. This is why chat now reaches an off-LAN peer: PT/reliability tracks the primary, but the message rides both addresses on every attempt, and the message-layer retransmit keeps re-spraying both until the ACK clears it.
                        if let Some(alt) = request.alt_addr {
                            udp::send(&socket, &pt_bytes, alt).await;
                        }
                    }
                }
                // No direct path → also send the WHOLE chat VSF (not the PT shard) over the relay pipe.
                // The peer receives it on its pipe and its dispatch decrypts + ACKs exactly as a direct message; the ACK returns over the peer's pipe. Chat now works with no direct path.
                // DETACHED like the ping relay legs: each relayed send is an HTTPS round trip (~1.3s against an offline recipient), and awaiting them inline serialised this whole drain loop — a retransmit burst put every queued ACK 5-10 seconds behind the spray (field log, 2026-08-09: ACK queued at :04.8, dispatched :14.4). Best-effort by design; the message-layer retransmit is the reliability.
                for dev in &request.relay_to {
                    let kp = keypair.clone();
                    let dev = *dev;
                    let bytes = msg_bytes.clone();
                    tokio::spawn(async move {
                        if let Err(e) =
                            crate::network::fgtw::relay::send_via_relay(&kp, &dev, &bytes).await
                        {
                            crate::logf!("RELAY: chat to {} failed: {}", hex::encode(&dev[..4]), e);
                        }
                    });
                }
            }
        }

        // Process history frames (hist_req / hist_page) — pre-built + signed on the UI thread; this loop just routes them thru PT (UDP → TCP after 1s → relay) and races the alt path with the SAME wire bytes, exactly like chat. Requester/server both dedup (rid), so redelivery is free.
        while let Ok(request) = history_rx.try_recv() {
            // A peer with no reachable direct address rides the relay IMMEDIATELY (whole frame down the pipe, like chat's relay_to) — PT's ladder-then-relay needs ~31s of failures, longer than the history requester's expiry, so relay-only pairs starved forever on it.
            if !request.peer_addr.ip().is_unspecified() {
                let pt_bytes = {
                    let mut pt_mgr = pt.lock().unwrap();
                    pt_mgr.send_with_pubkey(
                        request.peer_addr,
                        request.vsf_bytes.clone(),
                        Some(request.recipient_pubkey),
                    )
                };
                if !pt_bytes.is_empty() {
                    udp::send(&socket, &pt_bytes, request.peer_addr).await;
                    if let Some(alt) = request.alt_addr {
                        udp::send(&socket, &pt_bytes, alt).await;
                    }
                }
            }
            // DETACHED for the same reason as the chat drain above: pages are big, HTTPS trips are slow, and the requester's rid-dedup + expiry-refetch already tolerate loss and reorder.
            for dev in &request.relay_to {
                let kp = keypair.clone();
                let dev = *dev;
                let bytes = request.vsf_bytes.clone();
                tokio::spawn(async move {
                    if let Err(e) =
                        crate::network::fgtw::relay::send_via_relay(&kp, &dev, &bytes).await
                    {
                        crate::logf!("RELAY: history to {} failed: {}", hex::encode(&dev[..4]), e);
                    }
                });
            }
        }

        // Process ACK requests (message acknowledgments - CHAIN format) Routed thru PT for unified transport (UDP → TCP after 1s → relay fallback)
        while let Ok(request) = ack_rx.try_recv() {
            let timestamp = eagle_time_now();

            // Compute provenance and sign (CHAIN format - no weave yet)
            let provenance = compute_ack_provenance_v2(
                &request.conversation_token,
                request.acked_eagle_time,
                &request.plaintext_hash,
            );
            let sig = keypair.sign(&provenance);
            let mut sig_bytes = [0u8; 64];
            sig_bytes.copy_from_slice(&sig.to_bytes());

            crate::logf!(
                "Status: Sending MESSAGE_ACK to {} (eagle_time {}) via PT",
                request.peer_addr,
                request.acked_eagle_time
            );

            let msg = FgtwMessage::MessageAck {
                timestamp,
                conversation_token: request.conversation_token,
                acked_eagle_time: request.acked_eagle_time,
                plaintext_hash: request.plaintext_hash,
                sender_pubkey: our_pubkey.clone(),
                signature: sig_bytes,
            };

            let msg_bytes = msg.to_vsf_bytes();
            if !msg_bytes.is_empty() {
                // Direct leg only when there IS a direct address — same reasoning as the chat drain above: a relay-only peer's ACK arrives with `RELAY_ADDR`, and enqueueing that reliably would block the per-peer FIFO with a packet nothing can ever ACK.
                // `relay_to` below is this ACK's real path (that is exactly what the field is for), so the guard costs nothing here.
                if !crate::network::traverse::gather::is_bogus_addr(&request.peer_addr) {
                    // Route thru PT - handles UDP, TCP after 1s, relay fallback
                    let pt_bytes = {
                        let mut pt_mgr = pt.lock().unwrap();
                        pt_mgr.send_with_pubkey(
                            request.peer_addr,
                            msg_bytes.clone(),
                            Some(request.recipient_pubkey),
                        )
                    };
                    // Empty = queued behind an in-flight packet (stop-and-wait); tick() will send it.
                    if !pt_bytes.is_empty() {
                        udp::send(&socket, &pt_bytes, request.peer_addr).await;
                    }
                }
                // No direct path → relay the whole ACK VSF so it returns over the sender's pipe.
                // DETACHED like the chat drain: an inline await here put the NEXT queued ACK a full HTTPS round trip behind this one. The sender's retransmit re-provokes a lost ACK (the receiver's re-ACK is deterministic), so best-effort holds.
                for dev in &request.relay_to {
                    let kp = keypair.clone();
                    let dev = *dev;
                    let bytes = msg_bytes.clone();
                    tokio::spawn(async move {
                        if let Err(e) =
                            crate::network::fgtw::relay::send_via_relay(&kp, &dev, &bytes).await
                        {
                            crate::logf!("RELAY: ack to {} failed: {}", hex::encode(&dev[..4]), e);
                        }
                    });
                }
            }
        }

        // Process avatar request sends (ask a mutual contact for their avatar) Routed thru PT for unified transport (UDP → TCP after 1s → relay fallback)
        while let Ok(request) = avatar_request_rx.try_recv() {
            let timestamp = eagle_time_now();

            // provenance = BLAKE3(sender_pubkey || timestamp) - same shape as a signed ping
            let provenance_hash: [u8; 32] = blake3::hash(
                &[our_pubkey.as_bytes().as_slice(), &timestamp.to_le_bytes()].concat(),
            )
            .into();
            let sig = keypair.sign(&provenance_hash);
            let mut sig_bytes = [0u8; 64];
            sig_bytes.copy_from_slice(&sig.to_bytes());

            // A relay-only peer has no direct address, and AvatarRequestSend carries no `relay_to` — so there is no path for this frame at all. Say so and drop it here rather than handing `RELAY_ADDR` to PT: the guard in send_with_pubkey_and_alt would refuse it anyway, and a silent refusal deep in the transport reads as "sent" from up here.
            // The avatar is not lost — it falls back to the FGTW blob fetch (the same path used when a peer is offline), and the next pong with a fresh avatar_ts re-triggers a request once a direct path exists.
            if crate::network::traverse::gather::is_bogus_addr(&request.peer_addr) {
                crate::logf!("Status: AVATAR_REQUEST to {}... skipped — relay-only peer, no direct path (avatar falls back to the FGTW blob fetch)", hex::encode(&request.recipient_pubkey[..4]));
                continue;
            }

            crate::logf!(
                "Status: Sending AVATAR_REQUEST to {} via PT",
                request.peer_addr
            );

            let msg = FgtwMessage::AvatarRequest {
                timestamp,
                sender_pubkey: our_pubkey.clone(),
                provenance_hash,
                signature: sig_bytes,
            };

            let msg_bytes = msg.to_vsf_bytes();
            if !msg_bytes.is_empty() {
                // Route thru PT - handles UDP, TCP after 1s, relay fallback
                let pt_bytes = {
                    let mut pt_mgr = pt.lock().unwrap();
                    pt_mgr.send_with_pubkey(
                        request.peer_addr,
                        msg_bytes.clone(),
                        Some(request.recipient_pubkey),
                    )
                };
                // Empty = queued behind an in-flight packet (stop-and-wait); tick() will send it.
                if !pt_bytes.is_empty() {
                    udp::send(&socket, &pt_bytes, request.peer_addr).await;
                }
            }
        }

        // Process avatar response sends (return our own avatar to a requesting peer) Routed thru PT for unified transport (UDP → TCP after 1s → relay fallback)
        while let Ok(request) = avatar_response_rx.try_recv() {
            // Defence-in-depth: never device-sign and ship an FGTW error frame as an avatar (the caller validates+decodes first, but a poisoned frame reaching here would be signed as a real avatar the friend can't decode). The full decode needs the seed, so here we reject only the cheap-to-detect error frame; the seed-gated decode happens upstream.
            if let Some((reason, detail)) = fgtw::client::error_frame(&request.avatar_vsf) {
                crate::logf!(
                    "Status: refusing to serve avatar error frame {}: {}",
                    reason,
                    detail
                );
                continue;
            }
            // Same as the request side: no direct address and no `relay_to` on this type means no path. Drop it loudly instead of letting PT's guard swallow it. The requester recovers via the FGTW blob fetch.
            if crate::network::traverse::gather::is_bogus_addr(&request.peer_addr) {
                crate::logf!("Status: AVATAR_RESPONSE to {}... skipped — relay-only peer, no direct path (they fetch the blob from FGTW instead)", hex::encode(&request.recipient_pubkey[..4]));
                continue;
            }

            let timestamp = eagle_time_now();

            // provenance = BLAKE3(avatar_vsf) - the signature covers the avatar bytes' hash
            let provenance_hash: [u8; 32] = blake3::hash(&request.avatar_vsf).into();
            let sig = keypair.sign(&provenance_hash);
            let mut sig_bytes = [0u8; 64];
            sig_bytes.copy_from_slice(&sig.to_bytes());

            crate::logf!(
                "Status: Sending AVATAR_RESPONSE to {} ({} bytes avatar) via PT",
                request.peer_addr,
                request.avatar_vsf.len()
            );

            let msg = FgtwMessage::AvatarResponse {
                timestamp,
                responder_pubkey: our_pubkey.clone(),
                provenance_hash,
                signature: sig_bytes,
                avatar_vsf: request.avatar_vsf,
            };

            let msg_bytes = msg.to_vsf_bytes();
            if !msg_bytes.is_empty() {
                // Route thru PT - handles UDP, TCP after 1s, relay fallback
                let pt_bytes = {
                    let mut pt_mgr = pt.lock().unwrap();
                    pt_mgr.send_with_pubkey(
                        request.peer_addr,
                        msg_bytes.clone(),
                        Some(request.recipient_pubkey),
                    )
                };
                // Empty = queued behind an in-flight packet (stop-and-wait); tick() will send it.
                if !pt_bytes.is_empty() {
                    udp::send(&socket, &pt_bytes, request.peer_addr).await;
                }
            }
        }

        // Process PT send requests (large transfers)
        while let Ok(request) = pt_rx.try_recv() {
            crate::logf!(
                "PT: Starting outbound transfer to {} ({} bytes)",
                request.peer_addr,
                request.data.len()
            );
            let bytes_to_send = {
                let mut pt_mgr = pt.lock().unwrap();
                pt_mgr.send(request.peer_addr, request.data)
            };
            udp::send(&socket, &bytes_to_send, request.peer_addr).await;
        }

        // Process full CLUTCH offer requests (PT/UDP primary, TCP fallback) Uses VSF format with Ed25519 signature for verification
        while let Ok(request) = offer_rx.try_recv() {
            // VSF bytes already built by caller (to capture offer_provenance)
            let vsf_bytes = request.vsf_bytes;

            crate::logf!(
                "Status: Sending ClutchOffer to {} ({} bytes VSF) via PT/UDP",
                request.peer_addr,
                vsf_bytes.len()
            );

            // VSF inspection for development builds
            #[cfg(feature = "development")]
            {
                if let Ok(inspection) = vsf::inspect::inspect_vsf(&vsf_bytes) {
                    crate::logf!("Status: ClutchOffer VSF:\n{}", inspection);
                }
            }

            // Send via PT - handles retries/fallback internally. Races LAN vs WAN if alt given.
            let bytes_to_send = {
                let mut pt_mgr = pt.lock().unwrap();
                pt_mgr.send_with_pubkey_and_alt(
                    request.peer_addr,
                    request.alt_addr,
                    vsf_bytes.clone(),
                    Some(request.recipient_pubkey),
                )
            };
            udp::send(&socket, &bytes_to_send, request.peer_addr).await;
            if let Some(alt) = request.alt_addr {
                udp::send(&socket, &bytes_to_send, alt).await;
            }
            // No direct path proven → store on the relay in parallel. A peer we can't reach directly (asymmetric reachability — one end v6-only, the other v4-only behind symmetric NAT) still gets the offer via dual-stack fgtw.org. We relay explicitly here because the direct transfer keeps getting cancelled on address churn before its own retry-threshold relay fallback could fire.
            let mut relayed = 0usize;
            for dev in &request.relay_to {
                match crate::network::fgtw::relay::send_via_relay(&keypair, dev, &vsf_bytes).await {
                    Ok(()) => {
                        relayed += 1;
                        crate::logf!("RELAY: ClutchOffer delivered to {}", hex::encode(&dev[..4]))
                    }
                    Err(e) => crate::logf!(
                        "RELAY: ClutchOffer to {} did not land: {}",
                        hex::encode(&dev[..4]),
                        e
                    ),
                }
            }
            // Nothing landed anywhere: no direct path took it and every device's pipe was closed, so this ~570KB offer was discarded in full (the pipe has no mailbox). Say so plainly — the old code logged "stored" for exactly this case and the ceremony then stalled with nothing in either log to explain it. The re-fire rides the existing edges: the doorbell wakes a dozing peer, and the offer re-sends when they come back.
            if !request.relay_to.is_empty() && relayed == 0 {
                crate::logf!(
                    "CLUTCH: offer reached NOBODY — {} device(s) all offline; waiting for the peer to wake (doorbell) rather than re-blasting {}KB",
                    request.relay_to.len(),
                    vsf_bytes.len() / 1024
                );
            }
        }

        // Process CLUTCH KEM response requests
        while let Ok(request) = kem_response_rx.try_recv() {
            use crate::network::fgtw::protocol::build_clutch_kem_response_vsf;

            let vsf_bytes = match build_clutch_kem_response_vsf(
                &request.conversation_token,
                &request.ceremony_id,
                &request.payload,
                &request.device_pubkey,
                &request.device_secret,
            ) {
                Ok(bytes) => bytes,
                Err(e) => {
                    crate::logf!("Status: Failed to build ClutchKemResponse: {}", e);
                    continue;
                }
            };

            crate::logf!(
                "Status: Sending ClutchKemResponse to {} ({} bytes)",
                request.peer_addr,
                vsf_bytes.len()
            );

            #[cfg(feature = "development")]
            if let Ok(inspection) = vsf::inspect::inspect_vsf(&vsf_bytes) {
                crate::logf!("Status: ClutchKemResponse VSF:\n{}", inspection);
            }

            // Send via PT - handles retries/fallback internally. Races LAN vs WAN if alt given.
            let bytes_to_send = {
                let mut pt_mgr = pt.lock().unwrap();
                pt_mgr.send_with_pubkey_and_alt(
                    request.peer_addr,
                    request.alt_addr,
                    vsf_bytes.clone(),
                    Some(request.recipient_pubkey),
                )
            };
            udp::send(&socket, &bytes_to_send, request.peer_addr).await;
            if let Some(alt) = request.alt_addr {
                udp::send(&socket, &bytes_to_send, alt).await;
            }
            for dev in &request.relay_to {
                match crate::network::fgtw::relay::send_via_relay(&keypair, dev, &vsf_bytes).await {
                    Ok(()) => crate::logf!(
                        "RELAY: ClutchKemResponse relayed to {}",
                        hex::encode(&dev[..4])
                    ),
                    Err(e) => crate::logf!("RELAY: ClutchKemResponse did not land: {}", e),
                }
            }
        }

        // Process CLUTCH complete proof requests
        while let Ok(request) = complete_proof_rx.try_recv() {
            use crate::network::fgtw::protocol::build_clutch_complete_vsf;

            let vsf_bytes = match build_clutch_complete_vsf(
                &request.conversation_token,
                &request.ceremony_id,
                &request.payload,
                &request.device_pubkey,
                &request.device_secret,
            ) {
                Ok(bytes) => bytes,
                Err(e) => {
                    crate::logf!("Status: Failed to build ClutchComplete: {}", e);
                    continue;
                }
            };

            crate::logf!(
                "Status: Sending ClutchComplete to {} ({} bytes)",
                request.peer_addr,
                vsf_bytes.len()
            );

            #[cfg(feature = "development")]
            if let Ok(inspection) = vsf::inspect::inspect_vsf(&vsf_bytes) {
                crate::logf!("Status: ClutchComplete VSF:\n{}", inspection);
            }

            // Send via PT - handles retries/fallback internally. Races LAN vs WAN if alt given. (Complete proof is small and sent directly, so racing here is just dual UDP send.)
            let bytes_to_send = {
                let mut pt_mgr = pt.lock().unwrap();
                pt_mgr.send_with_pubkey_and_alt(
                    request.peer_addr,
                    request.alt_addr,
                    vsf_bytes.clone(),
                    Some(request.recipient_pubkey),
                )
            };
            udp::send(&socket, &bytes_to_send, request.peer_addr).await;
            if let Some(alt) = request.alt_addr {
                udp::send(&socket, &bytes_to_send, alt).await;
            }
            for dev in &request.relay_to {
                match crate::network::fgtw::relay::send_via_relay(&keypair, dev, &vsf_bytes).await {
                    Ok(()) => crate::logf!(
                        "RELAY: ClutchComplete relayed to {}",
                        hex::encode(&dev[..4])
                    ),
                    Err(e) => crate::logf!("RELAY: ClutchComplete did not land: {}", e),
                }
            }
        }

        // Process LAN discovery requests via multicast (more reliable than broadcast)
        while let Ok(request) = lan_broadcast_rx.try_recv() {
            let packet =
                udp::build_lan_discovery(request.our_handle_proof, request.our_port, our_device_pk);

            // IPv4 multicast: 239.104.199.144 (from random entropy 0x68C790)
            let mcast_v4 = SocketAddr::new(
                std::net::IpAddr::V4(Ipv4Addr::new(239, 104, 199, 144)),
                crate::MULTICAST_PORT,
            );

            // IPv6 multicast: ff02::68c7:9014 (link-local scope with our random bytes)
            let mcast_v6 = SocketAddr::new(
                std::net::IpAddr::V6(std::net::Ipv6Addr::new(
                    0xff02, 0, 0, 0, 0, 0, 0x68c7, 0x9014,
                )),
                crate::MULTICAST_PORT,
            );

            // Send to IPv4 multicast
            if let Ok(mcast_sock) = UdpSocket::bind("0.0.0.0:0") {
                let _ = mcast_sock.set_multicast_ttl_v4(1);
                let _ = udp::send_sync(&mcast_sock, &packet, mcast_v4);
                crate::logf!("LAN: Multicast {} bytes to {}", packet.len(), mcast_v4);
            }

            // Send to IPv6 multicast (hop limit is 1 by default for link-local)
            if let Ok(mcast_sock) = UdpSocket::bind("[::]:0") {
                let _ = udp::send_sync(&mcast_sock, &packet, mcast_v6);
                crate::logf!("LAN: Multicast {} bytes to {}", packet.len(), mcast_v6);
            }

            // Also send to subnet broadcast as fallback (many routers block multicast)
            if let Some((broadcast, local_ip)) = udp::get_broadcast_addr() {
                let bcast_addr =
                    SocketAddr::new(std::net::IpAddr::V4(broadcast), crate::MULTICAST_PORT);
                if let Ok(bcast_sock) = UdpSocket::bind("0.0.0.0:0") {
                    let _ = bcast_sock.set_broadcast(true);
                    let _ = udp::send_sync(&bcast_sock, &packet, bcast_addr);
                    crate::logf!(
                        "LAN: Broadcast {} bytes to {} (from {})",
                        packet.len(),
                        bcast_addr,
                        local_ip
                    );
                }
            }
        }

        // Process clear PT sends requests (when CLUTCH completes)
        while let Ok(request) = clear_pt_rx.try_recv() {
            let mut pt_mgr = pt.lock().unwrap();
            pt_mgr.clear_outbound(&request.peer_addr);
        }

        // PT periodic tick - handles timeouts, retries, TCP+relay fallback
        {
            let mut pt_mgr = pt.lock().unwrap();
            // Throttled transfer-progress push (attachment bars): only sharded transfers exist here, only pushed when the snapshot changed. ~2Hz keeps the pill honest without spamming the UI channel.
            if last_progress_push.elapsed() >= std::time::Duration::from_millis(500) {
                last_progress_push = std::time::Instant::now();
                let snap = pt_mgr.transfer_progress();
                if snap != last_progress_snap {
                    last_progress_snap = snap.clone();
                    send_status_update(
                        &status_tx,
                        StatusUpdate::AttachProgress(snap),
                        &event_proxy,
                    );
                }
            }
            let to_send = pt_mgr.tick();
            let keypair_for_relay = pt_mgr.keypair().clone();
            drop(pt_mgr);

            for tick in to_send {
                // Always send UDP first — UDP is the preferred path (PT shards over it).
                udp::send(&socket, &tick.wire_bytes, tick.peer_addr).await;

                // TCP fallback: send the WHOLE VSF payload once (set by PT tick after the UDP SPEC went ~1s unacked). Not the PT shard — TCP is reliable + ordered and the VSF `l` field self-frames the length, so the receiver's tcp::recv reads it whole and the existing CLUTCH dispatch parses it directly.
                // DETACHED, for the same reason the relay leg below is: `send_tcp` carries a 10s connect timeout, and awaiting it inline let one blackholed peer (a dead IPv6 route → "TCP connect timeout" / "No route to host") stall the WHOLE PT tick — every other peer's ladder, the progress push, and the drains behind this loop waited on it (field: multi-second gaps in the tail ending at a TCP-timeout line). Fire-and-forget: the send self-frames and the receiver dispatches it directly, so nothing in this loop depends on its result; a failure just logs. Whether relay is also attempted is decided by `tick.relay` (set by an earlier PT tick), not by this attempt's outcome, so detaching changes no fallback-ladder decision.
                if let Some(tcp_payload) = tick.tcp_payload {
                    let peer_addr = tick.peer_addr;
                    tokio::spawn(async move {
                        if let Err(e) = crate::network::tcp::send_tcp(&tcp_payload, peer_addr).await {
                            crate::logf!("PT: TCP send failed to {}: {}", peer_addr, e);
                        }
                    });
                }

                // If both UDP and TCP exhausted, try relay via /conduit
                // DETACHED like every other relay leg in this loop: the HTTPS trip blocked the whole PT tick, so every OTHER peer's ladder (and the drains behind this loop) waited on this one's relay round trip. The offline-parking verdict still lands — the task carries its own handle to the PT manager.
                if let Some(relay_info) = tick.relay {
                    crate::logf!(
                        "PT: Relaying to {} via /conduit",
                        hex::encode(&relay_info.recipient_pubkey[..4])
                    );
                    let kp = keypair_for_relay.clone();
                    let pt_for_park = pt.clone();
                    let peer_addr = tick.peer_addr;
                    tokio::spawn(async move {
                        match crate::network::fgtw::relay::send_via_relay(
                            &kp,
                            &relay_info.recipient_pubkey,
                            &relay_info.payload,
                        )
                        .await
                        {
                            Ok(()) => {
                                crate::log("PT: Relay send succeeded");
                            }
                            Err(e) => {
                                // OFFLINE VERDICT PARKS THE LADDER: the relay is authoritative — "recipient offline (frame discarded)" means every further direct retry + TCP connect + relay POST is guaranteed waste, and seven offline fleets' ladders running their full backoff schedules was a sustained retransmit storm (field, 2026-08-09: 32 retransmits in 31s, UI ticks to 1075ms riding it). Clear this peer's outbounds; the came-online edge (pong → retransmit sweep / attach re-request) re-serves the payload when the device actually returns.
                                if e.contains("recipient offline") {
                                    let mut pt_mgr = pt_for_park.lock().unwrap();
                                    pt_mgr.clear_outbound(&peer_addr);
                                    crate::logf!("PT: relay says recipient offline — parking the ladder to {} (re-serves on the came-online edge)", peer_addr);
                                } else {
                                    crate::logf!("PT: Relay send failed: {}", e);
                                }
                            }
                        }
                    });
                }
            }
        }
    }
}

/// Verify Ed25519 signature on provenance hash
fn verify_provenance_signature(
    provenance_hash: &[u8; 32],
    signer_pubkey: &DevicePubkey,
    signature: &[u8; 64],
) -> bool {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    let verifying_key = match VerifyingKey::from_bytes(signer_pubkey.as_bytes()) {
        Ok(k) => k,
        Err(_) => return false,
    };
    let sig = Signature::from_bytes(signature);

    verifying_key.verify(provenance_hash, &sig).is_ok()
}

// NOTE: compute_clutch_provenance and compute_clutch_complete_provenance REMOVED They were only used by the legacy v1 ClutchOffer/ClutchInit/ClutchResponse/ClutchComplete Full 8-primitive CLUTCH uses different provenance via build_clutch_offer_vsf()

/// Compute provenance hash for encrypted chat message (CHAIN format) provenance = BLAKE3(conversation_token || prev_msg_hp)
fn compute_chat_provenance(conversation_token: &[u8; 32], prev_msg_hp: &[u8; 32]) -> [u8; 32] {
    use blake3::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(conversation_token);
    hasher.update(prev_msg_hp);
    *hasher.finalize().as_bytes()
}

/// Compute provenance hash for message acknowledgment (CHAIN format) provenance = BLAKE3(conversation_token || acked_eagle_time_bytes || plaintext_hash || "ack")
fn compute_ack_provenance_v2(
    conversation_token: &[u8; 32],
    acked_eagle_time: i64,
    plaintext_hash: &[u8; 32],
) -> [u8; 32] {
    use blake3::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(conversation_token);
    hasher.update(&acked_eagle_time.to_le_bytes());
    hasher.update(plaintext_hash);
    hasher.update(b"ack");
    *hasher.finalize().as_bytes()
}
/// Handle PT VSF packets (SPEC, ACK, NAK, CONTROL, COMPLETE) Returns Some(true) if packet was handled, Some(false) if not a PT packet, None on error
///
/// Security: SPEC packets are only accepted from known contacts (sender pubkey validated)
async fn handle_pt_vsf_packet(
    msg_bytes: &[u8],
    src_addr: SocketAddr,
    pt: &Arc<Mutex<PTManager>>,
    socket: &Arc<tokio::net::UdpSocket>,
    _status_tx: &Sender<StatusUpdate>,
    _event_proxy: &OptionalEventProxy,
    contacts: &ContactPubkeys,
) -> Option<bool> {
    // Try to parse as PT packet (supports both header-only and section formats)
    let parsed = parse_pt_packet(msg_bytes)?;

    match parsed {
        // Header-only format (new compact format)
        ParsedPtPacket::HeaderOnly {
            name,
            provenance_hash,
            values,
        } => {
            match name.as_str() {
                "pt_ack" => {
                    if let Some(ack) = PTAck::from_vsf_header(provenance_hash, &values) {
                        // Handle ACK - state transitions happen in handle_ack Completion check and cleanup handled by main loop via transfer_id
                        let response_packets = {
                            let mut pt_mgr = pt.lock().unwrap();
                            pt_mgr.handle_ack(src_addr, ack)
                        };
                        for pkt in response_packets {
                            udp::send(socket, &pkt, src_addr).await;
                        }
                        return Some(true);
                    }
                }
                "pt_nak" => {
                    if let Some(nak) = PTNak::from_vsf_header(&values) {
                        // NOTE: NAK not logged individually - handled silently
                        let response_packets = {
                            let mut pt_mgr = pt.lock().unwrap();
                            pt_mgr.handle_nak(src_addr, nak)
                        };
                        for pkt in response_packets {
                            udp::send(socket, &pkt, src_addr).await;
                        }
                        return Some(true);
                    }
                }
                "pt_ctrl" => {
                    if let Some(control) = PTControl::from_vsf_header(&values) {
                        // NOTE: CONTROL not logged - handled silently
                        let mut pt_mgr = pt.lock().unwrap();
                        pt_mgr.handle_control(src_addr, control);
                        return Some(true);
                    }
                }
                "pt_done" => {
                    if let Some(complete) = PTComplete::from_vsf_header(provenance_hash, &values) {
                        // Log completion (success or failure)
                        if !complete.success {
                            crate::logf!("PT: Transfer FAILED from {}", src_addr);
                        }
                        // Handle completion - state transitions happen in handle_complete Completion check and cleanup handled by main loop via transfer_id
                        {
                            let mut pt_mgr = pt.lock().unwrap();
                            pt_mgr.handle_complete(src_addr, complete);
                        }
                        return Some(true);
                    }
                }
                _ => {}
            }
        }

        // Section format (SPEC uses full section, not header-only)
        ParsedPtPacket::Section {
            name,
            fields,
            sender_pubkey,
        } => {
            if name == "pt_spec" {
                if let Some(spec) = PTSpec::from_vsf_fields(&fields) {
                    // SECURITY: Validate sender before accepting any transfer Only accept SPEC from known contacts to prevent resource exhaustion
                    let is_known_contact = match sender_pubkey {
                        Some(pubkey_bytes) => {
                            let sender = DevicePubkey::from_bytes(pubkey_bytes);
                            let contact_list = contacts.lock().unwrap();
                            contact_list.iter().any(|p| *p == sender)
                        }
                        None => false, // No pubkey = unsigned = reject
                    };

                    if !is_known_contact {
                        crate::logf!(
                            "PT: SPEC REJECTED from {} - sender not in contacts (pubkey: {})",
                            src_addr,
                            sender_pubkey
                                .map(|p| hex::encode(&p[..8]))
                                .unwrap_or_else(|| "none".to_string())
                        );
                        // Silent drop - don't send ACK, don't accept transfer
                        return Some(true);
                    }

                    crate::logf!(
                        "PT: SPEC accepted from {} - {} packets, {} bytes",
                        src_addr,
                        spec.total_packets,
                        spec.total_size
                    );
                    let spec_ack = {
                        let mut pt_mgr = pt.lock().unwrap();
                        pt_mgr.handle_spec(src_addr, spec)
                    };
                    udp::send(socket, &spec_ack, src_addr).await;
                    return Some(true);
                }
            }
        }
    }

    Some(false)
}

/// Parse LAN discovery packet from main UDP socket Returns StatusUpdate::LanPeerDiscovered if valid, None otherwise
fn parse_lan_discovery(
    packet: &[u8],
    src_addr: SocketAddr,
    our_device_pubkey: &[u8; 32],
) -> Option<StatusUpdate> {
    let (handle_proof, local_ip, port, beacon_device) = udp::parse_lan_discovery(packet, src_addr)?;
    // Our own beacon loops back to us (multicast loopback + broadcast self-delivery). Pre-fleet that was harmless — our own handle_proof was never a contact — but the self-conversation makes our handle a contact, so accepting our own beacon overwrites that contact's LAN address with OUR OWN IP and every send boomerangs back to ourselves (observed: phone retransmitting to itself for 20+ minutes). The fleet shares one handle_proof, so self is detected by the beacon's device key; the source-IP test is only the fallback for pre-ke beacons, and it misses on multi-homed devices (Android wifi + cellular CLAT have different IPs and get_local_ip sees the cellular one).
    match beacon_device {
        Some(ke) if ke == *our_device_pubkey => {
            // OUR OWN beacon is not just noise to drop — its looped-back SOURCE is our LAN address on the interface the beacon actually left, which is exactly what the multi-homed get_local_ip hole loses. Hand it to the app so the published record carries a real LAN entry.
            if let SocketAddr::V4(v4) = src_addr {
                if udp::is_usable_lan_ipv4(*v4.ip()) {
                    return Some(StatusUpdate::OurLanAddrObserved { ip: *v4.ip() });
                }
            }
            return None;
        }
        None if udp::get_local_ip() == Some(local_ip) => return None,
        _ => {}
    }
    crate::logf!(
        "LAN: Received discovery from {} (handle_proof: {}..., port: {})",
        src_addr,
        hex::encode(&handle_proof[..4]),
        port
    );
    Some(StatusUpdate::LanPeerDiscovered {
        device_pubkey: beacon_device,
        handle_proof,
        local_ip,
        port,
    })
}

/// Parsed PT packet info - either from header inline field or section body
enum ParsedPtPacket {
    /// Header-only format: (pt_name:value1,value2,...) with provenance hash
    HeaderOnly {
        name: String,
        provenance_hash: [u8; 32],
        values: Vec<vsf::VsfType>,
    },
    /// Section format: [pt_name (field:value)...] with optional sender pubkey from signature
    Section {
        name: String,
        fields: Vec<(String, vsf::VsfType)>,
        /// Sender's Ed25519 public key from header signature (for authentication)
        sender_pubkey: Option<[u8; 32]>,
    },
}

/// Parse VSF PT packet - supports both header-only and section formats
fn parse_pt_packet(bytes: &[u8]) -> Option<ParsedPtPacket> {
    use vsf::file_format::VsfHeader;

    let (header, header_end) = VsfHeader::decode(bytes).ok()?;

    // Extract provenance hash from header
    let provenance_hash = match &header.provenance_hash {
        vsf::VsfType::hp(hash) if hash.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(hash);
            arr
        }
        _ => return None,
    };

    // Check for header-only format first (inline fields like pt_ack, pt_nak, pt_ctrl, pt_done) These have fields with values directly in the header, no section body
    for field in &header.fields {
        if field.name.starts_with("pt_") && field.offset_bytes == 0 && field.size_bytes == 0 {
            // This is a header-only field with inline values We need to re-parse to get the actual values
            if let Some(values) = parse_header_inline_values(bytes, &field.name) {
                return Some(ParsedPtPacket::HeaderOnly {
                    name: field.name.clone(),
                    provenance_hash,
                    values,
                });
            }
        }
    }

    // Extract sender pubkey from header signature (if present) This is the Ed25519 public key used to sign the packet
    let sender_pubkey = match &header.signer_pubkey {
        Some(vsf::VsfType::ke(key)) if key.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(key);
            Some(arr)
        }
        _ => None,
    };

    // Fall back to section body parsing — primary_section resolves the near-form name from the header TOC (the knowledge lives in the vsf crate now).
    let section = header.primary_section(bytes, header_end).ok()?;
    let section_name = section.name.clone();

    let fields: Vec<(String, vsf::VsfType)> = section
        .fields
        .iter()
        .filter_map(|f| f.values.first().map(|v| (f.name.clone(), v.clone())))
        .collect();

    Some(ParsedPtPacket::Section {
        name: section_name,
        fields,
        sender_pubkey,
    })
}

/// Parse inline values from a header field by name Returns the values for (name:val1,val2,...) format
fn parse_header_inline_values(bytes: &[u8], target_name: &str) -> Option<Vec<vsf::VsfType>> {
    use vsf::file_format::VsfHeader;

    let (header, _) = VsfHeader::decode(bytes).ok()?;

    // Find the target field in the header and return its inline values
    header
        .fields
        .iter()
        .find(|f| f.name == target_name)
        .map(|f| f.inline_values.clone())
}

/// Parse VSF fields from bytes (legacy section-only format) Parse a PT VSF packet, returns (section_name, fields)
#[allow(dead_code)]
fn parse_pt_vsf_fields(bytes: &[u8]) -> Option<(String, Vec<(String, vsf::VsfType)>)> {
    match parse_pt_packet(bytes)? {
        ParsedPtPacket::Section { name, fields, .. } => Some((name, fields)),
        ParsedPtPacket::HeaderOnly { .. } => None, // Can't convert header-only to named fields
    }
}
