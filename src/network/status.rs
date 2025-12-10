//! Contact status checker
//!
//! Sends UDP pings to contacts and receives pongs to determine online status.
//! Also handles CLUTCH key ceremony messages.
//! Uses the shared UDP socket from HandleQuery (the same port announced to FGTW).
//!
//! Protocol uses VSF-spec provenance hash for replay protection:
//! - provenance_hash = BLAKE3(sender_pubkey || timestamp_nanos)
//! - Signature covers the provenance_hash
//! - Timestamp uses nanosecond precision (ef6) for uniqueness

use super::udp;
use crate::network::fgtw::FgtwMessage;
use crate::network::fgtw::Keypair;
use crate::network::pt::{
    is_pt_data, PTAck, PTComplete, PTControl, PTData, PTManager, PTNak, PTSpec,
    TcpTransport,
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
use winit::event_loop::EventLoopProxy;

/// Shared contact list - UI updates this, background thread reads it
pub type ContactPubkeys = Arc<Mutex<Vec<DevicePubkey>>>;

/// Get current Eagle Time as binary64 (seconds since Apollo 11 landing: July 20, 1969, 20:17:40 UTC)
fn eagle_time_binary64() -> f64 {
    vsf::eagle_time_nanos()
}

/// Compute provenance hash = BLAKE3(sender_pubkey || timestamp_bytes)
fn compute_provenance_hash(sender_pubkey: &DevicePubkey, timestamp: f64) -> [u8; 32] {
    use blake3::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(sender_pubkey.as_bytes());
    hasher.update(&timestamp.to_le_bytes());
    *hasher.finalize().as_bytes()
}

/// Request to ping a contact
#[derive(Clone)]
pub struct PingRequest {
    pub peer_addr: SocketAddr,
    pub peer_pubkey: DevicePubkey,
}

// NOTE: ClutchRequest and ClutchRequestType REMOVED
// Full 8-primitive CLUTCH uses ClutchFullOfferRequest and ClutchKemResponseRequest
// which are handled via build_clutch_full_offer_vsf() and build_clutch_kem_response_vsf()
// See CLUTCH.md Section 4.2 for the slot-based ceremony protocol.

/// Request to send an encrypted message
#[derive(Clone)]
pub struct MessageRequest {
    pub peer_addr: SocketAddr,
    pub our_handle_proof: [u8; 32],
    pub sequence: u64,
    pub ciphertext: Vec<u8>,
}

/// Request to send a message acknowledgment
#[derive(Clone)]
pub struct AckRequest {
    pub peer_addr: SocketAddr,
    pub our_handle_proof: [u8; 32],
    pub sequence: u64,
    /// Hash of the decrypted plaintext - proves we decrypted their message
    pub plaintext_hash: [u8; 32],
    /// Hash of our most recent message they ACK'd - bidirectional weave binding
    pub our_last_acked_hash: [u8; 32],
}

/// Request to start a PT large transfer (e.g., full CLUTCH offer with all 8 pubkeys)
#[derive(Clone)]
pub struct PTSendRequest {
    pub peer_addr: SocketAddr,
    pub data: Vec<u8>,
}

/// Request to send full CLUTCH offer (~548KB) via TCP fallback
///
/// Uses VSF format with proper signing and verification.
/// See protocol.rs build_clutch_full_offer_vsf() for format details.
#[derive(Clone)]
pub struct ClutchFullOfferRequest {
    pub peer_addr: SocketAddr, // Port comes from FGTW (peer's photon_port)
    pub handle_hashes: Vec<[u8; 32]>, // N-party sorted handle hashes (BLAKE3(handle))
    pub ceremony_id: [u8; 32], // Deterministic from sorted handle_hashes
    pub payload: crate::crypto::clutch::ClutchFullOfferPayload,
    pub device_pubkey: [u8; 32],
    pub device_secret: [u8; 32], // For signing (zeroize after use)
}

/// Request to send CLUTCH KEM response (~31KB) via TCP fallback
///
/// Uses VSF format with proper signing and verification.
/// See protocol.rs build_clutch_kem_response_vsf() for format details.
#[derive(Clone)]
pub struct ClutchKemResponseRequest {
    pub peer_addr: SocketAddr, // Port comes from FGTW (peer's photon_port)
    pub handle_hashes: Vec<[u8; 32]>, // N-party sorted handle hashes (BLAKE3(handle))
    pub ceremony_id: [u8; 32], // Deterministic from sorted handle_hashes
    pub payload: crate::crypto::clutch::ClutchKemResponsePayload,
    pub device_pubkey: [u8; 32],
    pub device_secret: [u8; 32], // For signing (zeroize after use)
}

/// Request to broadcast presence on LAN for local peer discovery
/// Solves NAT hairpinning - when peers are on same LAN, use local IPs
#[derive(Clone)]
pub struct LanBroadcastRequest {
    pub our_handle_proof: [u8; 32],
    pub our_port: u16, // Port we're listening on
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
    },
    // NOTE: ClutchOffer, ClutchInit, ClutchResponse, ClutchComplete REMOVED
    // Full 8-primitive CLUTCH uses ClutchFullOfferReceived and ClutchKemResponseReceived
    // See CLUTCH.md Section 4.2 for the slot-based ceremony protocol.

    /// Encrypted chat message received
    ChatMessage {
        from_handle_proof: [u8; 32],
        sequence: u64,
        ciphertext: Vec<u8>,
        sender_addr: SocketAddr,
    },
    /// Message acknowledgment received
    MessageAck {
        from_handle_proof: [u8; 32],
        sequence: u64,
        /// Hash of the decrypted plaintext - proves they decrypted our message
        plaintext_hash: [u8; 32],
        /// Hash of their most recent message we ACK'd - bidirectional weave binding
        sender_last_acked: [u8; 32],
    },
    /// PT large transfer completed - received data from peer
    PTReceived {
        peer_addr: SocketAddr,
        data: Vec<u8>,
    },
    /// PT outbound transfer completed successfully
    PTSendComplete { peer_addr: SocketAddr },
    /// Full CLUTCH offer received (~548KB with all 8 pubkeys)
    /// Payload is already verified and parsed from VSF format.
    ClutchFullOfferReceived {
        handle_hashes: Vec<[u8; 32]>,    // N-party sorted handle hashes (includes ours)
        ceremony_id: [u8; 32],           // Deterministic from sorted handle_hashes (verified by receiver)
        sender_pubkey: [u8; 32],         // Device pubkey (verified via signature)
        payload: crate::crypto::clutch::ClutchFullOfferPayload,
        sender_addr: SocketAddr,
    },
    /// CLUTCH KEM response received (~31KB with 4 ciphertexts)
    /// Payload is already verified and parsed from VSF format.
    ClutchKemResponseReceived {
        handle_hashes: Vec<[u8; 32]>,    // N-party sorted handle hashes (includes ours)
        ceremony_id: [u8; 32],           // Deterministic - should match locally computed value
        sender_pubkey: [u8; 32],         // Device pubkey (verified via signature)
        payload: crate::crypto::clutch::ClutchKemResponsePayload,
        sender_addr: SocketAddr,
    },
    /// LAN peer discovered via broadcast (NAT hairpinning workaround)
    LanPeerDiscovered {
        handle_proof: [u8; 32],
        local_ip: Ipv4Addr,
        port: u16,
    },
}

/// Pending ping waiting for pong
struct PendingPing {
    recipient_pubkey: DevicePubkey,
    provenance_hash: [u8; 32],
    sent_at: Instant,
}

/// Contact status checker
///
/// Spawns a background thread to handle async UDP ping/pong and CLUTCH messages.
/// Uses the shared UDP socket from HandleQuery.
/// For large CLUTCH payloads, uses TCP fallback (raw254 not yet implemented).
pub struct StatusChecker {
    ping_sender: Sender<PingRequest>,
    // NOTE: clutch_sender removed - legacy v1 CLUTCH no longer used
    message_sender: Sender<MessageRequest>,
    ack_sender: Sender<AckRequest>,
    pt_sender: Sender<PTSendRequest>,
    full_offer_sender: Sender<ClutchFullOfferRequest>,
    kem_response_sender: Sender<ClutchKemResponseRequest>,
    lan_broadcast_sender: Sender<LanBroadcastRequest>,
    status_receiver: Receiver<StatusUpdate>,
}

impl StatusChecker {
    /// Create a new status checker using a shared socket (Desktop version with EventLoopProxy)
    ///
    /// `socket` is the shared UDP socket from HandleQuery (same port announced to FGTW).
    /// `keypair` is the device keypair (same one used for FGTW registration).
    /// `contacts` is shared with UI - only respond to pings from pubkeys in this list.
    /// `event_proxy` is used to wake the event loop when network data arrives.
    #[cfg(not(target_os = "android"))]
    pub fn new(
        socket: Arc<UdpSocket>,
        keypair: Keypair,
        contacts: ContactPubkeys,
        event_proxy: EventLoopProxy<PhotonEvent>,
    ) -> Result<Self, String> {
        let (ping_tx, ping_rx) = channel::<PingRequest>();
        let (message_tx, message_rx) = channel::<MessageRequest>();
        let (ack_tx, ack_rx) = channel::<AckRequest>();
        let (pt_tx, pt_rx) = channel::<PTSendRequest>();
        let (full_offer_tx, full_offer_rx) = channel::<ClutchFullOfferRequest>();
        let (kem_response_tx, kem_response_rx) = channel::<ClutchKemResponseRequest>();
        let (lan_broadcast_tx, lan_broadcast_rx) = channel::<LanBroadcastRequest>();
        let (status_tx, status_rx) = channel::<StatusUpdate>();

        let our_pubkey = DevicePubkey::from_bytes(keypair.public.to_bytes());

        // Log which port we're using
        let local_addr = socket
            .local_addr()
            .map_err(|e| format!("Failed to get local addr: {}", e))?;
        crate::log_info(&format!(
            "Status: Using socket on port {}",
            local_addr.port()
        ));

        socket
            .set_nonblocking(true)
            .map_err(|e| format!("Failed to set non-blocking: {}", e))?;

        // Get local IP for TCP listener (and LAN discovery)
        // Use connect-to-external trick to find actual LAN IP (not 0.0.0.0)
        let local_ip = udp::get_local_ip().unwrap_or(Ipv4Addr::new(0, 0, 0, 0));

        thread::spawn(move || {
            crate::log_info("Status: Background thread started");
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
                    pt_rx,
                    full_offer_rx,
                    kem_response_rx,
                    lan_broadcast_rx,
                    status_tx,
                    contacts,
                    Some(event_proxy),
                )
                .await;
            });
        });

        Ok(Self {
            ping_sender: ping_tx,
            message_sender: message_tx,
            ack_sender: ack_tx,
            pt_sender: pt_tx,
            full_offer_sender: full_offer_tx,
            kem_response_sender: kem_response_tx,
            lan_broadcast_sender: lan_broadcast_tx,
            status_receiver: status_rx,
        })
    }

    /// Create a new status checker using a shared socket (Android version - no EventLoopProxy)
    #[cfg(target_os = "android")]
    pub fn new(
        socket: Arc<UdpSocket>,
        keypair: Keypair,
        contacts: ContactPubkeys,
    ) -> Result<Self, String> {
        let (ping_tx, ping_rx) = channel::<PingRequest>();
        let (message_tx, message_rx) = channel::<MessageRequest>();
        let (ack_tx, ack_rx) = channel::<AckRequest>();
        let (pt_tx, pt_rx) = channel::<PTSendRequest>();
        let (full_offer_tx, full_offer_rx) = channel::<ClutchFullOfferRequest>();
        let (kem_response_tx, kem_response_rx) = channel::<ClutchKemResponseRequest>();
        let (lan_broadcast_tx, lan_broadcast_rx) = channel::<LanBroadcastRequest>();
        let (status_tx, status_rx) = channel::<StatusUpdate>();

        let our_pubkey = DevicePubkey::from_bytes(keypair.public.to_bytes());

        // Log which port we're using
        let local_addr = socket
            .local_addr()
            .map_err(|e| format!("Failed to get local addr: {}", e))?;
        crate::log_info(&format!(
            "Status: Using socket on port {}",
            local_addr.port()
        ));

        socket
            .set_nonblocking(true)
            .map_err(|e| format!("Failed to set non-blocking: {}", e))?;

        // Get local IP for TCP listener (and LAN discovery)
        let local_ip = udp::get_local_ip().unwrap_or(Ipv4Addr::new(0, 0, 0, 0));

        thread::spawn(move || {
            crate::log_info("Status: Background thread started");
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
                    pt_rx,
                    full_offer_rx,
                    kem_response_rx,
                    lan_broadcast_rx,
                    status_tx,
                    contacts,
                    None,
                )
                .await;
            });
        });

        Ok(Self {
            ping_sender: ping_tx,
            message_sender: message_tx,
            ack_sender: ack_tx,
            pt_sender: pt_tx,
            full_offer_sender: full_offer_tx,
            kem_response_sender: kem_response_tx,
            lan_broadcast_sender: lan_broadcast_tx,
            status_receiver: status_rx,
        })
    }

    /// Request to ping a contact (non-blocking)
    pub fn ping(&self, peer_addr: SocketAddr, peer_pubkey: DevicePubkey) {
        let _ = self.ping_sender.send(PingRequest {
            peer_addr,
            peer_pubkey,
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

    /// Start a PT large transfer (non-blocking)
    pub fn send_pt(&self, peer_addr: SocketAddr, data: Vec<u8>) {
        let _ = self.pt_sender.send(PTSendRequest { peer_addr, data });
    }

    /// Send full CLUTCH offer (~548KB) via TCP fallback (non-blocking)
    ///
    /// Uses VSF format with proper signing. Requires:
    /// - ceremony_id: Deterministic from sorted handle_hashes (same on both sides)
    /// - device keys: For Ed25519 signing of the VSF message
    pub fn send_full_offer(&self, request: ClutchFullOfferRequest) {
        let _ = self.full_offer_sender.send(request);
    }

    /// Send CLUTCH KEM response (~31KB) via TCP fallback (non-blocking)
    ///
    /// Uses VSF format with proper signing. Uses same deterministic ceremony_id.
    pub fn send_kem_response(&self, request: ClutchKemResponseRequest) {
        let _ = self.kem_response_sender.send(request);
    }

    /// Broadcast presence on LAN for local peer discovery (non-blocking)
    /// Solves NAT hairpinning - when peers are on same LAN, they can discover each other's local IPs
    pub fn send_lan_broadcast(&self, our_handle_proof: [u8; 32], our_port: u16) {
        let _ = self.lan_broadcast_sender.send(LanBroadcastRequest {
            our_handle_proof,
            our_port,
        });
    }

    /// Check for status updates (non-blocking)
    pub fn try_recv(&self) -> Option<StatusUpdate> {
        self.status_receiver.try_recv().ok()
    }
}

/// Event loop proxy type alias for optional use
#[cfg(not(target_os = "android"))]
type OptionalEventProxy = Option<EventLoopProxy<PhotonEvent>>;
#[cfg(target_os = "android")]
type OptionalEventProxy = Option<()>;

/// Send a status update and wake the event loop if proxy is available
fn send_status_update(
    status_tx: &Sender<StatusUpdate>,
    update: StatusUpdate,
    #[allow(unused_variables)] event_proxy: &OptionalEventProxy,
) {
    let _ = status_tx.send(update);
    #[cfg(not(target_os = "android"))]
    if let Some(proxy) = event_proxy {
        if let Err(e) = proxy.send_event(PhotonEvent::NetworkUpdate) {
            crate::log_error(&format!("Status: Failed to send wake event: {:?}", e));
        }
    }
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
    pt_rx: Receiver<PTSendRequest>,
    full_offer_rx: Receiver<ClutchFullOfferRequest>,
    kem_response_rx: Receiver<ClutchKemResponseRequest>,
    lan_broadcast_rx: Receiver<LanBroadcastRequest>,
    status_tx: Sender<StatusUpdate>,
    contacts: ContactPubkeys,
    event_proxy: OptionalEventProxy,
) {
    use tokio::net::UdpSocket as TokioUdpSocket;

    let cloned = match std_socket.try_clone() {
        Ok(s) => s,
        Err(e) => {
            crate::log_error(&format!("Status: Failed to clone socket: {}", e));
            return;
        }
    };

    let socket = match TokioUdpSocket::from_std(cloned) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            crate::log_error(&format!("Status: Failed to convert to tokio socket: {}", e));
            return;
        }
    };

    // Start TCP listener for CLUTCH large payloads (same port as UDP)
    // Try IPv6 first (dual-stack), fall back to IPv4
    // Skip on Android - tokio TcpListener has issues with accept() returning EINVAL
    #[cfg(not(target_os = "android"))]
    let tcp_listener = {
        let udp_port = std_socket
            .local_addr()
            .map(|a| a.port())
            .unwrap_or(PHOTON_PORT);
        // Try IPv6 dual-stack first (accepts both IPv4 and IPv6 on most systems)
        let tcp_addr_v6 = SocketAddr::new(std::net::IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED), udp_port);
        match tokio::net::TcpListener::bind(tcp_addr_v6).await {
            Ok(listener) => {
                crate::log_info(&format!("Status: TCP listening on [::]:{}  (dual-stack)", udp_port));
                Some(listener)
            }
            Err(_) => {
                // Fall back to IPv4 only
                let tcp_addr_v4 = SocketAddr::new(std::net::IpAddr::V4(local_ip), udp_port);
                match tokio::net::TcpListener::bind(tcp_addr_v4).await {
                    Ok(listener) => {
                        crate::log_info(&format!("Status: TCP listening on {} (IPv4 only)", tcp_addr_v4));
                        Some(listener)
                    }
                    Err(e) => {
                        crate::log_error(&format!("Status: Failed to bind TCP: {}", e));
                        None
                    }
                }
            }
        }
    };
    #[cfg(target_os = "android")]
    let tcp_listener: Option<tokio::net::TcpListener> = None;

    let pending: Arc<Mutex<Vec<PendingPing>>> = Arc::new(Mutex::new(Vec::new()));

    // Track consecutive failed pings per contact (hysteresis - don't flip offline on 1 lost packet)
    let failed_pings: Arc<Mutex<Vec<([u8; 32], u8)>>> = Arc::new(Mutex::new(Vec::new()));
    const OFFLINE_THRESHOLD: u8 = 3;

    // PT manager for large transfers - shared with receiver task
    let pt: Arc<Mutex<PTManager>> = Arc::new(Mutex::new(PTManager::new(keypair.clone())));

    let socket_recv = socket.clone();
    let pending_recv = pending.clone();
    let our_pubkey_recv = our_pubkey.clone();
    let keypair_recv = keypair.clone();
    let status_tx_recv = status_tx.clone();
    let contacts_recv = contacts.clone();
    let event_proxy_recv = event_proxy.clone();
    let pt_recv = pt.clone();
    let failed_pings_recv = failed_pings.clone();

    // LAN discovery packets are now handled in the main UDP receiver (parse_lan_discovery)
    // No separate listener needed - all traffic on PHOTON_PORT

    // Spawn TCP receiver task for large CLUTCH payloads (VSF format)
    if let Some(listener) = tcp_listener {
        let status_tx_tcp = status_tx.clone();
        let event_proxy_tcp = event_proxy.clone();
        tokio::spawn(async move {
            crate::log_info("Status: TCP receiver task started");
            loop {
                // Async accept - sleeps until connection arrives (no polling)
                match listener.accept().await {
                    Ok((stream, src_addr)) => {
                        crate::log_info(&format!("Status: TCP connection from {}", src_addr));
                        // Convert to std TcpStream for TcpTransport
                        let std_stream = stream.into_std();
                        match std_stream {
                            Ok(mut std_stream) => {
                                // Read the payload
                                match TcpTransport::recv_payload(&mut std_stream) {
                                    Ok(data) => {
                                        crate::log_info(&format!(
                                            "Status: Received {} bytes via TCP from {}",
                                            data.len(),
                                            src_addr
                                        ));

                                        // VSF inspection for development builds
                                        #[cfg(feature = "development")]
                                        {
                                            if let Ok(inspection) = vsf::inspect::inspect_vsf(&data) {
                                                crate::log_info(&format!("Status: Received TCP VSF:\n{}", inspection));
                                            }
                                        }

                                        // Check for VSF magic bytes (RÅ< = 0x52 0xC3 0x85 0x3C)
                                        if data.len() >= 4 && &data[0..3] == b"R\xC3\x85" && data[3] == b'<' {
                                            // Parse VSF header to determine message type
                                            // Try parsing as ClutchFullOffer first
                                            use crate::network::fgtw::protocol::{
                                                parse_clutch_full_offer_vsf_without_recipient_check,
                                                parse_clutch_kem_response_vsf_without_recipient_check,
                                            };

                                            // Try full offer first (has clutch_full_offer section)
                                            if let Ok((payload, sender_pubkey, ceremony_id, handle_hashes)) =
                                                parse_clutch_full_offer_vsf_without_recipient_check(&data)
                                            {
                                                crate::log_info("Status: Received ClutchFullOffer via TCP (VSF verified)");
                                                send_status_update(
                                                    &status_tx_tcp,
                                                    StatusUpdate::ClutchFullOfferReceived {
                                                        handle_hashes,
                                                        ceremony_id,
                                                        sender_pubkey,
                                                        payload,
                                                        sender_addr: src_addr,
                                                    },
                                                    &event_proxy_tcp,
                                                );
                                            }
                                            // Try KEM response
                                            else if let Ok((payload, sender_pubkey, ceremony_id, handle_hashes)) =
                                                parse_clutch_kem_response_vsf_without_recipient_check(&data)
                                            {
                                                crate::log_info("Status: Received ClutchKemResponse via TCP (VSF verified)");
                                                send_status_update(
                                                    &status_tx_tcp,
                                                    StatusUpdate::ClutchKemResponseReceived {
                                                        handle_hashes,
                                                        ceremony_id,
                                                        sender_pubkey,
                                                        payload,
                                                        sender_addr: src_addr,
                                                    },
                                                    &event_proxy_tcp,
                                                );
                                            }
                                            else {
                                                crate::log_error("Status: Failed to parse TCP VSF as CLUTCH message");
                                            }
                                        } else {
                                            crate::log_error(&format!(
                                                "Status: TCP payload is not VSF format (len={}, magic={:02x?})",
                                                data.len(),
                                                if data.len() >= 4 { &data[0..4] } else { &data[..] }
                                            ));
                                        }
                                    }
                                    Err(e) => {
                                        crate::log_error(&format!("Status: TCP recv error: {}", e));
                                    }
                                }
                            }
                            Err(e) => {
                                crate::log_error(&format!(
                                    "Status: Failed to convert TCP stream: {}",
                                    e
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        crate::log_error(&format!("Status: TCP accept error: {}", e));
                    }
                }
            }
        });
    }

    // Spawn UDP receiver task
    tokio::spawn(async move {
        crate::log_info("Status: Receiver task started, waiting for UDP packets...");
        let mut buf = [0u8; 2048];
        loop {
            match socket_recv.recv_from(&mut buf).await {
                Ok((len, src_addr)) => {
                    let msg_bytes = &buf[..len];

                    // Check for PT DATA packets first (start with 'd')
                    // NOTE: Individual DATA packets not logged - only completion/failure
                    if is_pt_data(msg_bytes) {
                        if let Some(data) = PTData::from_bytes(msg_bytes) {
                            // Handle data and collect responses (must drop lock before await)
                            let (ack_bytes, complete_bytes, received_data, inbound_stats) = {
                                let mut pt_mgr = pt_recv.lock().unwrap();
                                let ack = pt_mgr.handle_data(src_addr, data);
                                let complete = pt_mgr.check_inbound_complete(src_addr);
                                let stats = pt_mgr.inbound_stats(&src_addr);
                                let data = if complete.is_some() {
                                    pt_mgr.take_inbound_data(src_addr)
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
                                    if let Some((packets, bytes, duplicates, duration_ms)) = inbound_stats {
                                        let total_recv = packets + duplicates;
                                        let utilization = if total_recv > 0 {
                                            (packets as f64 / total_recv as f64) * 100.0
                                        } else {
                                            100.0
                                        };
                                        let throughput_kbps = if duration_ms > 0 {
                                            (bytes as f64 * 8.0) / (duration_ms as f64)
                                        } else {
                                            0.0
                                        };
                                        let throughput_str = if throughput_kbps >= 1000.0 {
                                            format!("{:.1} Mbps", throughput_kbps / 1000.0)
                                        } else {
                                            format!("{:.0} kbps", throughput_kbps)
                                        };
                                        crate::log_info(&format!(
                                            "PT: ← {} OK | {} | {:.1}s | {} pkts | {:.0}% util ({} dups)",
                                            src_addr,
                                            throughput_str,
                                            duration_ms as f64 / 1000.0,
                                            packets,
                                            utilization,
                                            duplicates,
                                        ));
                                    } else {
                                        crate::log_info(&format!(
                                            "PT: ← {} OK | {} bytes",
                                            src_addr,
                                            data.len()
                                        ));
                                    }

                                    // Inspect completed PT data with VSF inspector
                                    if let Ok(inspection) = vsf::inspect::inspect_vsf(&data) {
                                        crate::log_info(&format!(
                                            "PT: Received VSF ({} bytes):\n{}",
                                            data.len(),
                                            inspection
                                        ));
                                    } else {
                                        crate::log_error(&format!(
                                            "PT: Received {} bytes - NOT valid VSF",
                                            data.len()
                                        ));
                                    }

                                    // Parse PT data as CLUTCH message and emit appropriate event
                                    use crate::network::fgtw::protocol::{
                                        parse_clutch_full_offer_vsf_without_recipient_check,
                                        parse_clutch_kem_response_vsf_without_recipient_check,
                                    };

                                    // Try to parse as ClutchFullOffer
                                    if let Ok((payload, sender_pubkey, ceremony_id, handle_hashes)) =
                                        parse_clutch_full_offer_vsf_without_recipient_check(&data)
                                    {
                                        crate::log_info("PT: Parsed as ClutchFullOffer (VSF verified)");
                                        send_status_update(
                                            &status_tx_recv,
                                            StatusUpdate::ClutchFullOfferReceived {
                                                handle_hashes,
                                                ceremony_id,
                                                sender_pubkey,
                                                payload,
                                                sender_addr: src_addr,
                                            },
                                            &event_proxy_recv,
                                        );
                                    }
                                    // Try to parse as ClutchKemResponse
                                    else if let Ok((payload, sender_pubkey, ceremony_id, handle_hashes)) =
                                        parse_clutch_kem_response_vsf_without_recipient_check(&data)
                                    {
                                        crate::log_info("PT: Parsed as ClutchKemResponse (VSF verified)");
                                        send_status_update(
                                            &status_tx_recv,
                                            StatusUpdate::ClutchKemResponseReceived {
                                                handle_hashes,
                                                ceremony_id,
                                                sender_pubkey,
                                                payload,
                                                sender_addr: src_addr,
                                            },
                                            &event_proxy_recv,
                                        );
                                    }
                                    else {
                                        // Unknown PT data - emit generic event for debugging
                                        crate::log_error(&format!(
                                            "PT: Failed to parse {} bytes as CLUTCH message",
                                            data.len()
                                        ));
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
                    if let Some(lan_update) = parse_lan_discovery(msg_bytes, src_addr) {
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
                    )
                    .await
                    {
                        if pt_handled {
                            continue;
                        }
                    }

                    match FgtwMessage::from_vsf_bytes(msg_bytes) {
                        Ok(message) => {
                            match message {
                                FgtwMessage::StatusPing {
                                    timestamp,
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

                                    // Mark sender as online (they pinged us, so they're online!)
                                    send_status_update(
                                        &status_tx_recv,
                                        StatusUpdate::Online {
                                            peer_pubkey: sender_pubkey.clone(),
                                            is_online: true,
                                            peer_addr: Some(src_addr),
                                        },
                                        &event_proxy_recv,
                                    );

                                    // Send pong (no avatar_id - avatars are fetched by handle)
                                    let sig = keypair_recv.sign(&provenance_hash);
                                    let mut sig_bytes = [0u8; 64];
                                    sig_bytes.copy_from_slice(&sig.to_bytes());

                                    let pong = FgtwMessage::StatusPong {
                                        timestamp: eagle_time_binary64(),
                                        responder_pubkey: our_pubkey_recv.clone(),
                                        provenance_hash,
                                        signature: sig_bytes,
                                    };

                                    let pong_bytes = pong.to_vsf_bytes();
                                    if !pong_bytes.is_empty() {
                                        udp::send(&socket_recv, &pong_bytes, src_addr).await;
                                    }
                                }

                                FgtwMessage::StatusPong {
                                    timestamp: _,
                                    responder_pubkey,
                                    provenance_hash,
                                    signature,
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

                                    let pending_ping = match pending_ping {
                                        Some(p) => p,
                                        None => continue,
                                    };

                                    // Verify responder matches who we pinged
                                    if responder_pubkey != pending_ping.recipient_pubkey {
                                        continue;
                                    }

                                    // Verify signature
                                    if !verify_provenance_signature(
                                        &provenance_hash,
                                        &responder_pubkey,
                                        &signature,
                                    ) {
                                        continue;
                                    }

                                    // Reset failure counter on successful pong (prevents bouncing)
                                    {
                                        let mut failures = failed_pings_recv.lock().unwrap();
                                        failures.retain(|(k, _)| k != responder_pubkey.as_bytes());
                                    }

                                    // Send status update (avatar fetched by handle, not in ping/pong)
                                    send_status_update(
                                        &status_tx_recv,
                                        StatusUpdate::Online {
                                            peer_pubkey: responder_pubkey,
                                            is_online: true,
                                            peer_addr: Some(src_addr),
                                        },
                                        &event_proxy_recv,
                                    );
                                }

                                // NOTE: ClutchOffer, ClutchInit, ClutchResponse, ClutchComplete handlers REMOVED
                                // Full 8-primitive CLUTCH uses TCP with ClutchFullOfferReceived and ClutchKemResponseReceived
                                // See CLUTCH.md Section 4.2 for the slot-based ceremony protocol.

                                FgtwMessage::ChatMessage {
                                    timestamp: _,
                                    from_handle_proof,
                                    sequence,
                                    ciphertext,
                                    sender_pubkey,
                                    signature,
                                } => {
                                    // Verify signature
                                    let provenance =
                                        compute_msg_provenance(&from_handle_proof, sequence);
                                    if !verify_provenance_signature(
                                        &provenance,
                                        &sender_pubkey,
                                        &signature,
                                    ) {
                                        continue;
                                    }

                                    // Forward to UI for decryption
                                    send_status_update(
                                        &status_tx_recv,
                                        StatusUpdate::ChatMessage {
                                            from_handle_proof,
                                            sequence,
                                            ciphertext,
                                            sender_addr: src_addr,
                                        },
                                        &event_proxy_recv,
                                    );
                                }

                                FgtwMessage::MessageAck {
                                    timestamp: _,
                                    from_handle_proof,
                                    sequence,
                                    plaintext_hash,
                                    sender_last_acked,
                                    sender_pubkey,
                                    signature,
                                } => {
                                    crate::log_info(&format!(
                                        "Status: MESSAGE_ACK received from {} (seq {})",
                                        src_addr, sequence
                                    ));

                                    // Verify signature (includes plaintext_hash and weave in provenance)
                                    let provenance = compute_ack_provenance(
                                        &from_handle_proof,
                                        sequence,
                                        &plaintext_hash,
                                        &sender_last_acked,
                                    );
                                    if !verify_provenance_signature(
                                        &provenance,
                                        &sender_pubkey,
                                        &signature,
                                    ) {
                                        crate::log_info("  -> REJECTED (bad signature)");
                                        continue;
                                    }

                                    send_status_update(
                                        &status_tx_recv,
                                        StatusUpdate::MessageAck {
                                            from_handle_proof,
                                            sequence,
                                            plaintext_hash,
                                            sender_last_acked,
                                        },
                                        &event_proxy_recv,
                                    );
                                }

                                _ => {
                                    crate::log_info("Status: Unknown message type received");
                                }
                            }
                        }
                        Err(e) => {
                            // Log first 32 bytes for debugging
                            let preview: String = msg_bytes.iter().take(32)
                                .map(|b| format!("{:02x}", b))
                                .collect::<Vec<_>>()
                                .join(" ");
                            crate::log_info(&format!(
                                "Status: Parse error: {} (len={}, first 32 bytes: {})",
                                e, msg_bytes.len(), preview
                            ));
                        }
                    }
                }
                Err(_) => {}
            }
        }
    });

    // Track pending PT sends for TCP fallback
    let mut pending_pt_sends: Vec<(SocketAddr, Instant, Vec<u8>)> = Vec::new();
    const PT_TIMEOUT: Duration = Duration::from_secs(30);

    // Process ping requests from UI
    loop {
        match ping_rx.try_recv() {
            Ok(request) => {
                let timestamp = eagle_time_binary64();
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
                    crate::log_info(&format!(
                        "Status: PING build failed for {}",
                        request.peer_addr
                    ));
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
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
        }

        // Cleanup stale pending pings (older than 5 seconds)
        // Use hysteresis: only mark offline after OFFLINE_THRESHOLD consecutive failures
        {
            let mut list = pending.lock().unwrap();
            let mut failures = failed_pings.lock().unwrap();
            let now = Instant::now();
            let timeout = Duration::from_secs(5);

            // Find expired pings and increment failure counters
            let expired: Vec<_> = list
                .iter()
                .filter(|ping| now.duration_since(ping.sent_at) >= timeout)
                .map(|ping| ping.recipient_pubkey.clone())
                .collect();

            for pubkey in expired {
                let pubkey_bytes = *pubkey.as_bytes();
                // Find or insert entry with linear search
                let count = if let Some(entry) = failures.iter_mut().find(|(k, _)| *k == pubkey_bytes) {
                    entry.1 = entry.1.saturating_add(1);
                    entry.1
                } else {
                    failures.push((pubkey_bytes, 1));
                    1
                };

                if count >= OFFLINE_THRESHOLD {
                    // Enough consecutive failures - mark offline
                    crate::log_info(&format!(
                        "Status: TIMEOUT ({} consecutive) - {} marked offline",
                        count,
                        hex::encode(&pubkey_bytes[..8])
                    ));
                    send_status_update(
                        &status_tx,
                        StatusUpdate::Online {
                            peer_pubkey: pubkey,
                            is_online: false,
                            peer_addr: None, // No address for offline
                        },
                        &event_proxy,
                    );
                    // Reset counter after marking offline (so we can detect coming back online)
                    failures.retain(|(k, _)| *k != pubkey_bytes);
                } else {
                    crate::log_info(&format!(
                        "Status: TIMEOUT ({}/{}) - {} (waiting for more failures before offline)",
                        count, OFFLINE_THRESHOLD,
                        hex::encode(&pubkey_bytes[..8])
                    ));
                }
            }

            list.retain(|ping| now.duration_since(ping.sent_at) < timeout);
        }

        // NOTE: "Process CLUTCH requests" block REMOVED
        // Full 8-primitive CLUTCH uses ClutchFullOfferRequest and ClutchKemResponseRequest
        // which are processed below using TCP/PT transport.

        // Process message requests (encrypted chat messages)
        while let Ok(request) = message_rx.try_recv() {
            let timestamp = eagle_time_binary64();

            // Compute provenance and sign
            let provenance = compute_msg_provenance(&request.our_handle_proof, request.sequence);
            let sig = keypair.sign(&provenance);
            let mut sig_bytes = [0u8; 64];
            sig_bytes.copy_from_slice(&sig.to_bytes());

            crate::log_info(&format!(
                "Status: Sending CHAT_MESSAGE to {} (seq {})",
                request.peer_addr, request.sequence
            ));

            let msg = FgtwMessage::ChatMessage {
                timestamp,
                from_handle_proof: request.our_handle_proof,
                sequence: request.sequence,
                ciphertext: request.ciphertext,
                sender_pubkey: our_pubkey.clone(),
                signature: sig_bytes,
            };

            let msg_bytes = msg.to_vsf_bytes();
            if !msg_bytes.is_empty() {
                udp::send(&socket, &msg_bytes, request.peer_addr).await;
            }
        }

        // Process ACK requests (message acknowledgments)
        while let Ok(request) = ack_rx.try_recv() {
            let timestamp = eagle_time_binary64();

            // Compute provenance and sign (includes plaintext_hash and weave for chain binding)
            let provenance = compute_ack_provenance(
                &request.our_handle_proof,
                request.sequence,
                &request.plaintext_hash,
                &request.our_last_acked_hash,
            );
            let sig = keypair.sign(&provenance);
            let mut sig_bytes = [0u8; 64];
            sig_bytes.copy_from_slice(&sig.to_bytes());

            crate::log_info(&format!(
                "Status: Sending MESSAGE_ACK to {} (seq {})",
                request.peer_addr, request.sequence
            ));

            let msg = FgtwMessage::MessageAck {
                timestamp,
                from_handle_proof: request.our_handle_proof,
                sequence: request.sequence,
                plaintext_hash: request.plaintext_hash,
                sender_last_acked: request.our_last_acked_hash,
                sender_pubkey: our_pubkey.clone(),
                signature: sig_bytes,
            };

            let msg_bytes = msg.to_vsf_bytes();
            if !msg_bytes.is_empty() {
                udp::send(&socket, &msg_bytes, request.peer_addr).await;
            }
        }

        // Process PT send requests (large transfers)
        while let Ok(request) = pt_rx.try_recv() {
            crate::log_info(&format!(
                "PT: Starting outbound transfer to {} ({} bytes)",
                request.peer_addr,
                request.data.len()
            ));
            let mut pt_mgr = pt.lock().unwrap();
            let spec_bytes = pt_mgr.start_send(request.peer_addr, request.data);
            drop(pt_mgr); // Drop lock before async call
            udp::send(&socket, &spec_bytes, request.peer_addr).await;
        }

        // Process full CLUTCH offer requests (PT/UDP primary, TCP fallback)
        // Uses VSF format with Ed25519 signature for verification
        while let Ok(request) = full_offer_rx.try_recv() {
            use crate::network::fgtw::protocol::build_clutch_full_offer_vsf;

            // Build signed VSF message
            let vsf_bytes = match build_clutch_full_offer_vsf(
                &request.handle_hashes,
                &request.ceremony_id,
                &request.payload,
                &request.device_pubkey,
                &request.device_secret,
            ) {
                Ok(bytes) => bytes,
                Err(e) => {
                    crate::log_error(&format!("Status: Failed to build ClutchFullOffer VSF: {}", e));
                    continue;
                }
            };

            crate::log_info(&format!(
                "Status: Sending ClutchFullOffer to {} ({} bytes VSF) via PT/UDP",
                request.peer_addr,
                vsf_bytes.len()
            ));

            // VSF inspection for development builds
            #[cfg(feature = "development")]
            {
                if let Ok(inspection) = vsf::inspect::inspect_vsf(&vsf_bytes) {
                    crate::log_info(&format!("Status: ClutchFullOffer VSF:\n{}", inspection));
                }
            }

            // Send via PT/UDP (primary) - track for TCP fallback on timeout
            let spec_bytes = {
                let mut pt_mgr = pt.lock().unwrap();
                pt_mgr.start_send(request.peer_addr, vsf_bytes.clone())
            };
            udp::send(&socket, &spec_bytes, request.peer_addr).await;
            pending_pt_sends.push((request.peer_addr, Instant::now(), vsf_bytes));
        }

        // Process CLUTCH KEM response requests (PT/UDP primary, TCP fallback)
        // Uses VSF format with Ed25519 signature for verification
        while let Ok(request) = kem_response_rx.try_recv() {
            use crate::network::fgtw::protocol::build_clutch_kem_response_vsf;

            // Build signed VSF message
            let vsf_bytes = match build_clutch_kem_response_vsf(
                &request.handle_hashes,
                &request.ceremony_id,
                &request.payload,
                &request.device_pubkey,
                &request.device_secret,
            ) {
                Ok(bytes) => bytes,
                Err(e) => {
                    crate::log_error(&format!("Status: Failed to build ClutchKemResponse VSF: {}", e));
                    continue;
                }
            };

            crate::log_info(&format!(
                "Status: Sending ClutchKemResponse to {} ({} bytes VSF) via PT/UDP",
                request.peer_addr,
                vsf_bytes.len()
            ));

            // VSF inspection for development builds
            #[cfg(feature = "development")]
            {
                if let Ok(inspection) = vsf::inspect::inspect_vsf(&vsf_bytes) {
                    crate::log_info(&format!("Status: ClutchKemResponse VSF:\n{}", inspection));
                }
            }

            // Send via PT/UDP (primary) - track for TCP fallback on timeout
            let spec_bytes = {
                let mut pt_mgr = pt.lock().unwrap();
                pt_mgr.start_send(request.peer_addr, vsf_bytes.clone())
            };
            udp::send(&socket, &spec_bytes, request.peer_addr).await;
            pending_pt_sends.push((request.peer_addr, Instant::now(), vsf_bytes));
        }

        // Process LAN broadcast requests (NAT hairpinning workaround)
        while let Ok(request) = lan_broadcast_rx.try_recv() {
            let packet = udp::build_lan_discovery(request.our_handle_proof, request.our_port);

            // Send to broadcast address
            let broadcast_addr = SocketAddr::new(
                std::net::IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255)),
                PHOTON_PORT,
            );

            // Create a separate socket for broadcast (main socket may not have SO_BROADCAST)
            if let Ok(broadcast_sock) = UdpSocket::bind("0.0.0.0:0") {
                if broadcast_sock.set_broadcast(true).is_ok() {
                    let _ = udp::send_sync(&broadcast_sock, &packet, broadcast_addr);
                    crate::log_info(&format!(
                        "LAN: Broadcast {} bytes to {}",
                        packet.len(),
                        broadcast_addr
                    ));
                }
            }
        }

        // PT periodic tick - check timeouts and retransmit
        {
            let mut pt_mgr = pt.lock().unwrap();
            let to_send = pt_mgr.tick();
            drop(pt_mgr); // Drop lock before async calls
            for (addr, pkt) in to_send {
                udp::send(&socket, &pkt, addr).await;
            }
        }

        // Check for completed PT sends - remove from pending
        {
            let pt_mgr = pt.lock().unwrap();
            let completed: Vec<SocketAddr> = pending_pt_sends
                .iter()
                .filter(|(addr, _, _)| pt_mgr.is_outbound_complete(addr))
                .map(|(addr, _, _)| *addr)
                .collect();
            drop(pt_mgr);
            for addr in completed {
                pending_pt_sends.retain(|(a, _, _)| *a != addr);
                crate::log_info(&format!("PT: Transfer to {} completed, removed from pending", addr));
            }
        }

        // Check for PT timeouts - fall back to TCP
        {
            let now = Instant::now();
            let timed_out: Vec<(SocketAddr, Vec<u8>)> = pending_pt_sends
                .iter()
                .filter(|(_, start, _)| now.duration_since(*start) > PT_TIMEOUT)
                .map(|(addr, _, vsf)| (*addr, vsf.clone()))
                .collect();

            for (addr, vsf_bytes) in timed_out {
                pending_pt_sends.retain(|(a, _, _)| *a != addr);
                crate::log_info(&format!(
                    "PT: Transfer to {} timed out after {}s, falling back to TCP",
                    addr, PT_TIMEOUT.as_secs()
                ));

                // Try TCP fallback
                match TcpTransport::connect(addr, Duration::from_secs(10)) {
                    Ok(mut stream) => {
                        if let Err(e) = TcpTransport::send_payload(&mut stream, &vsf_bytes) {
                            crate::log_error(&format!(
                                "Status: TCP fallback failed to send to {}: {}",
                                addr, e
                            ));
                        } else {
                            crate::log_info(&format!(
                                "Status: TCP fallback sent {} bytes to {} successfully",
                                vsf_bytes.len(), addr
                            ));
                        }
                    }
                    Err(e) => {
                        crate::log_error(&format!(
                            "Status: TCP fallback failed to connect to {}: {}",
                            addr, e
                        ));
                    }
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

// NOTE: compute_clutch_provenance and compute_clutch_complete_provenance REMOVED
// They were only used by the legacy v1 ClutchOffer/ClutchInit/ClutchResponse/ClutchComplete
// Full 8-primitive CLUTCH uses different provenance via build_clutch_full_offer_vsf()

/// Compute provenance hash for encrypted message
/// provenance = BLAKE3(from_handle_proof || sequence)
fn compute_msg_provenance(from_handle_proof: &[u8; 32], sequence: u64) -> [u8; 32] {
    use blake3::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(from_handle_proof);
    hasher.update(&sequence.to_le_bytes());
    *hasher.finalize().as_bytes()
}

/// Compute provenance hash for message acknowledgment
/// provenance = BLAKE3(from_handle_proof || sequence || plaintext_hash || weave_hash || "ack")
fn compute_ack_provenance(
    from_handle_proof: &[u8; 32],
    sequence: u64,
    plaintext_hash: &[u8; 32],
    weave_hash: &[u8; 32],
) -> [u8; 32] {
    use blake3::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(from_handle_proof);
    hasher.update(&sequence.to_le_bytes());
    hasher.update(plaintext_hash);
    hasher.update(weave_hash);
    hasher.update(b"ack");
    *hasher.finalize().as_bytes()
}
/// Handle PT VSF packets (SPEC, ACK, NAK, CONTROL, COMPLETE)
/// Returns Some(true) if packet was handled, Some(false) if not a PT packet, None on error
async fn handle_pt_vsf_packet(
    msg_bytes: &[u8],
    src_addr: SocketAddr,
    pt: &Arc<Mutex<PTManager>>,
    socket: &Arc<tokio::net::UdpSocket>,
    status_tx: &Sender<StatusUpdate>,
    event_proxy: &OptionalEventProxy,
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
                        let (response_packets, is_complete) = {
                            let mut pt_mgr = pt.lock().unwrap();
                            let packets = pt_mgr.handle_ack(src_addr, ack);
                            let complete = pt_mgr.is_outbound_complete(&src_addr);
                            if complete {
                                pt_mgr.remove_outbound(&src_addr);
                            }
                            (packets, complete)
                        };
                        for pkt in response_packets {
                            udp::send(socket, &pkt, src_addr).await;
                        }
                        if is_complete {
                            crate::log_info(&format!(
                                "PT: Outbound transfer to {} complete",
                                src_addr
                            ));
                            send_status_update(
                                status_tx,
                                StatusUpdate::PTSendComplete {
                                    peer_addr: src_addr,
                                },
                                event_proxy,
                            );
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
                            crate::log_error(&format!("PT: Transfer FAILED from {}", src_addr));
                        }
                        let is_complete = {
                            let mut pt_mgr = pt.lock().unwrap();
                            pt_mgr.handle_complete(src_addr, complete);
                            let complete = pt_mgr.is_outbound_complete(&src_addr);
                            if complete {
                                pt_mgr.remove_outbound(&src_addr);
                            }
                            complete
                        };
                        if is_complete {
                            send_status_update(
                                status_tx,
                                StatusUpdate::PTSendComplete {
                                    peer_addr: src_addr,
                                },
                                event_proxy,
                            );
                        }
                        return Some(true);
                    }
                }
                _ => {}
            }
        }

        // Section format (SPEC uses full section, not header-only)
        ParsedPtPacket::Section { name, fields } => {
            if name == "pt_spec" {
                if let Some(spec) = PTSpec::from_vsf_fields(&fields) {
                    crate::log_info(&format!(
                        "PT: SPEC received from {} - {} packets, {} bytes",
                        src_addr, spec.total_packets, spec.total_size
                    ));
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

/// Parse LAN discovery packet from main UDP socket
/// Returns StatusUpdate::LanPeerDiscovered if valid, None otherwise
fn parse_lan_discovery(packet: &[u8], src_addr: SocketAddr) -> Option<StatusUpdate> {
    let (handle_proof, local_ip, port) = udp::parse_lan_discovery(packet, src_addr)?;
    crate::log_info(&format!(
        "LAN: Received discovery from {} (handle_proof: {}..., port: {})",
        src_addr,
        hex::encode(&handle_proof[..4]),
        port
    ));
    Some(StatusUpdate::LanPeerDiscovered {
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
    /// Legacy section format: [pt_name (field:value)...]
    Section {
        name: String,
        fields: Vec<(String, vsf::VsfType)>,
    },
}

/// Parse VSF PT packet - supports both header-only and section formats
fn parse_pt_packet(bytes: &[u8]) -> Option<ParsedPtPacket> {
    use vsf::file_format::VsfHeader;
    use vsf::parse;

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

    // Check for header-only format first (inline fields like pt_ack, pt_nak, pt_ctrl, pt_done)
    // These have fields with values directly in the header, no section body
    for field in &header.fields {
        if field.name.starts_with("pt_") && field.offset_bytes == 0 && field.size_bytes == 0 {
            // This is a header-only field with inline values
            // We need to re-parse to get the actual values
            if let Some(values) = parse_header_inline_values(bytes, &field.name) {
                return Some(ParsedPtPacket::HeaderOnly {
                    name: field.name.clone(),
                    provenance_hash,
                    values,
                });
            }
        }
    }

    // Fall back to section body parsing
    let mut ptr = header_end;
    if ptr >= bytes.len() || bytes[ptr] != b'[' {
        return None;
    }
    ptr += 1;

    // Parse section name (this identifies packet type: pt_spec, pt_ack, etc.)
    let section_name = match parse(bytes, &mut ptr).ok()? {
        vsf::VsfType::d(name) => name,
        _ => return None,
    };

    let mut fields = Vec::new();
    while ptr < bytes.len() && bytes[ptr] != b']' {
        if bytes[ptr] != b'(' {
            break;
        }
        ptr += 1;

        let field_name = match parse(bytes, &mut ptr) {
            Ok(vsf::VsfType::d(name)) => name,
            _ => break,
        };

        if ptr < bytes.len() && bytes[ptr] == b':' {
            ptr += 1;
            if let Ok(value) = parse(bytes, &mut ptr) {
                fields.push((field_name, value));
            }
        }

        if ptr < bytes.len() && bytes[ptr] == b')' {
            ptr += 1;
        }
    }

    Some(ParsedPtPacket::Section {
        name: section_name,
        fields,
    })
}

/// Parse inline values from a header field by name
/// Returns the values for (name:val1,val2,...) format
fn parse_header_inline_values(bytes: &[u8], target_name: &str) -> Option<Vec<vsf::VsfType>> {
    use vsf::parse;

    // Skip magic "RÅ<"
    if bytes.len() < 4 || &bytes[0..3] != "RÅ".as_bytes() || bytes[3] != b'<' {
        return None;
    }

    let mut ptr = 4;

    // Skip header fields until we hit '>' or find our field
    while ptr < bytes.len() && bytes[ptr] != b'>' {
        if bytes[ptr] == b'(' {
            ptr += 1;

            // Parse field name
            let field_name = match parse(bytes, &mut ptr) {
                Ok(vsf::VsfType::d(name)) => name,
                _ => continue,
            };

            if field_name == target_name {
                // Found it! Parse values after ':'
                let mut values = Vec::new();
                if ptr < bytes.len() && bytes[ptr] == b':' {
                    ptr += 1;
                    // Parse comma-separated values until ')'
                    loop {
                        if ptr >= bytes.len() || bytes[ptr] == b')' {
                            break;
                        }
                        if let Ok(value) = parse(bytes, &mut ptr) {
                            values.push(value);
                        } else {
                            break;
                        }
                        // Skip comma separator
                        if ptr < bytes.len() && bytes[ptr] == b',' {
                            ptr += 1;
                        }
                    }
                }
                return Some(values);
            }

            // Skip to end of this field
            while ptr < bytes.len() && bytes[ptr] != b')' {
                let _ = parse(bytes, &mut ptr);
                if ptr < bytes.len() && bytes[ptr] == b',' {
                    ptr += 1;
                }
            }
            if ptr < bytes.len() && bytes[ptr] == b')' {
                ptr += 1;
            }
        } else {
            // Skip non-field elements in header
            let _ = parse(bytes, &mut ptr);
        }
    }

    None
}

/// Parse VSF fields from bytes (legacy section-only format)
/// Parse a PT VSF packet, returns (section_name, fields)
#[allow(dead_code)]
fn parse_pt_vsf_fields(bytes: &[u8]) -> Option<(String, Vec<(String, vsf::VsfType)>)> {
    match parse_pt_packet(bytes)? {
        ParsedPtPacket::Section { name, fields } => Some((name, fields)),
        ParsedPtPacket::HeaderOnly { .. } => None, // Can't convert header-only to named fields
    }
}
