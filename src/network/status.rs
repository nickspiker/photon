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
use crate::network::pltp::{
    is_pltp_data, PLTPAck, PLTPComplete, PLTPControl, PLTPData, PLTPManager, PLTPNak, PLTPSpec,
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

/// Request to initiate or continue CLUTCH ceremony
#[derive(Clone)]
pub struct ClutchRequest {
    pub peer_addr: SocketAddr,
    pub our_handle_proof: [u8; 32],
    pub their_handle_proof: [u8; 32],
    pub message: ClutchRequestType,
}

#[derive(Clone)]
pub enum ClutchRequestType {
    /// Send ClutchOffer with our ephemeral pubkey (parallel v2)
    Offer { ephemeral_pubkey: [u8; 32] },
    /// Send ClutchInit with our ephemeral pubkey (v1 legacy)
    Init { ephemeral_pubkey: [u8; 32] },
    /// Send ClutchResponse with our ephemeral pubkey (v1 legacy)
    Response { ephemeral_pubkey: [u8; 32] },
    /// Send ClutchComplete with proof
    Complete { proof: [u8; 32] },
}

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

/// Request to start a PLTP large transfer (e.g., full CLUTCH offer with all 8 pubkeys)
#[derive(Clone)]
pub struct PLTPSendRequest {
    pub peer_addr: SocketAddr,
    pub data: Vec<u8>,
}

/// Request to send full CLUTCH offer (~548KB) via TCP fallback
#[derive(Clone)]
pub struct ClutchFullOfferRequest {
    pub peer_addr: SocketAddr, // Port comes from FGTW (peer's photon_port)
    pub our_handle_proof: [u8; 32],
    pub payload: Vec<u8>, // ClutchFullOfferPayload.to_bytes()
}

/// Request to send CLUTCH KEM response (~17KB) via TCP fallback
#[derive(Clone)]
pub struct ClutchKemResponseRequest {
    pub peer_addr: SocketAddr, // Port comes from FGTW (peer's photon_port)
    pub our_handle_proof: [u8; 32],
    pub payload: Vec<u8>, // ClutchKemResponsePayload.to_bytes()
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
    /// CLUTCH ceremony message received (parallel v2)
    ClutchOffer {
        from_handle_proof: [u8; 32],
        to_handle_proof: [u8; 32],
        ephemeral_pubkey: [u8; 32],
        sender_addr: SocketAddr,
    },
    /// CLUTCH Init (v1 legacy)
    ClutchInit {
        from_handle_proof: [u8; 32],
        to_handle_proof: [u8; 32],
        ephemeral_pubkey: [u8; 32],
        sender_addr: SocketAddr,
    },
    /// CLUTCH Response (v1 legacy)
    ClutchResponse {
        from_handle_proof: [u8; 32],
        to_handle_proof: [u8; 32],
        ephemeral_pubkey: [u8; 32],
        sender_addr: SocketAddr,
    },
    ClutchComplete {
        from_handle_proof: [u8; 32],
        to_handle_proof: [u8; 32],
        proof: [u8; 32],
    },
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
    /// PLTP large transfer completed - received data from peer
    PLTPReceived {
        peer_addr: SocketAddr,
        data: Vec<u8>,
    },
    /// PLTP outbound transfer completed successfully
    PLTPSendComplete { peer_addr: SocketAddr },
    /// Full CLUTCH offer received (~548KB with all 8 pubkeys)
    ClutchFullOfferReceived {
        from_handle_proof: [u8; 32],
        payload: Vec<u8>,
        sender_addr: SocketAddr,
    },
    /// CLUTCH KEM response received (~17KB with 4 ciphertexts)
    ClutchKemResponseReceived {
        from_handle_proof: [u8; 32],
        payload: Vec<u8>,
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
    clutch_sender: Sender<ClutchRequest>,
    message_sender: Sender<MessageRequest>,
    ack_sender: Sender<AckRequest>,
    pltp_sender: Sender<PLTPSendRequest>,
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
        let (clutch_tx, clutch_rx) = channel::<ClutchRequest>();
        let (message_tx, message_rx) = channel::<MessageRequest>();
        let (ack_tx, ack_rx) = channel::<AckRequest>();
        let (pltp_tx, pltp_rx) = channel::<PLTPSendRequest>();
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
                    clutch_rx,
                    message_rx,
                    ack_rx,
                    pltp_rx,
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
            clutch_sender: clutch_tx,
            message_sender: message_tx,
            ack_sender: ack_tx,
            pltp_sender: pltp_tx,
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
        let (clutch_tx, clutch_rx) = channel::<ClutchRequest>();
        let (message_tx, message_rx) = channel::<MessageRequest>();
        let (ack_tx, ack_rx) = channel::<AckRequest>();
        let (pltp_tx, pltp_rx) = channel::<PLTPSendRequest>();
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
                    clutch_rx,
                    message_rx,
                    ack_rx,
                    pltp_rx,
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
            clutch_sender: clutch_tx,
            message_sender: message_tx,
            ack_sender: ack_tx,
            pltp_sender: pltp_tx,
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

    /// Send a CLUTCH ceremony message (non-blocking)
    pub fn send_clutch(&self, request: ClutchRequest) {
        let _ = self.clutch_sender.send(request);
    }

    /// Send an encrypted message (non-blocking)
    pub fn send_message(&self, request: MessageRequest) {
        let _ = self.message_sender.send(request);
    }

    /// Send a message acknowledgment (non-blocking)
    pub fn send_ack(&self, request: AckRequest) {
        let _ = self.ack_sender.send(request);
    }

    /// Start a PLTP large transfer (non-blocking)
    pub fn send_pltp(&self, peer_addr: SocketAddr, data: Vec<u8>) {
        let _ = self.pltp_sender.send(PLTPSendRequest { peer_addr, data });
    }

    /// Send full CLUTCH offer (~548KB) via TCP fallback (non-blocking)
    pub fn send_full_offer(
        &self,
        peer_addr: SocketAddr,
        our_handle_proof: [u8; 32],
        payload: Vec<u8>,
    ) {
        let _ = self.full_offer_sender.send(ClutchFullOfferRequest {
            peer_addr,
            our_handle_proof,
            payload,
        });
    }

    /// Send CLUTCH KEM response (~17KB) via TCP fallback (non-blocking)
    pub fn send_kem_response(
        &self,
        peer_addr: SocketAddr,
        our_handle_proof: [u8; 32],
        payload: Vec<u8>,
    ) {
        let _ = self.kem_response_sender.send(ClutchKemResponseRequest {
            peer_addr,
            our_handle_proof,
            payload,
        });
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
    clutch_rx: Receiver<ClutchRequest>,
    message_rx: Receiver<MessageRequest>,
    ack_rx: Receiver<AckRequest>,
    pltp_rx: Receiver<PLTPSendRequest>,
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
    // Skip on Android - tokio TcpListener has issues with accept() returning EINVAL
    #[cfg(not(target_os = "android"))]
    let tcp_listener = {
        let udp_port = std_socket
            .local_addr()
            .map(|a| a.port())
            .unwrap_or(PHOTON_PORT);
        let tcp_addr = SocketAddr::new(std::net::IpAddr::V4(local_ip), udp_port);
        match tokio::net::TcpListener::bind(tcp_addr).await {
            Ok(listener) => {
                crate::log_info(&format!("Status: TCP fallback listening on {}", tcp_addr));
                Some(listener)
            }
            Err(e) => {
                crate::log_error(&format!("Status: Failed to bind TCP fallback: {}", e));
                None
            }
        }
    };
    #[cfg(target_os = "android")]
    let tcp_listener: Option<tokio::net::TcpListener> = None;

    // Pending pings - just a Vec, ~10 contacts max
    let pending: Arc<Mutex<Vec<PendingPing>>> = Arc::new(Mutex::new(Vec::new()));

    // PLTP manager for large transfers - shared with receiver task
    let pltp: Arc<Mutex<PLTPManager>> = Arc::new(Mutex::new(PLTPManager::new(keypair.clone())));

    let socket_recv = socket.clone();
    let pending_recv = pending.clone();
    let our_pubkey_recv = our_pubkey.clone();
    let keypair_recv = keypair.clone();
    let status_tx_recv = status_tx.clone();
    let contacts_recv = contacts.clone();
    let event_proxy_recv = event_proxy.clone();
    let pltp_recv = pltp.clone();

    // LAN discovery packets are now handled in the main UDP receiver (parse_lan_discovery)
    // No separate listener needed - all traffic on PHOTON_PORT

    // Spawn TCP receiver task for large CLUTCH payloads
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
                                        // Parse the payload type from first byte
                                        if data.len() > 33 {
                                            let payload_type = data[0];
                                            let handle_proof: [u8; 32] =
                                                data[1..33].try_into().unwrap_or([0; 32]);
                                            let payload = data[33..].to_vec();

                                            let src_ipv4 = match src_addr.ip() {
                                                std::net::IpAddr::V4(ip) => ip,
                                                _ => Ipv4Addr::new(0, 0, 0, 0),
                                            };
                                            let sender_addr = SocketAddr::new(
                                                std::net::IpAddr::V4(src_ipv4),
                                                src_addr.port(),
                                            );

                                            match payload_type {
                                                0x01 => {
                                                    // Full CLUTCH offer
                                                    crate::log_info(
                                                        "Status: Received ClutchFullOffer via TCP",
                                                    );
                                                    send_status_update(
                                                        &status_tx_tcp,
                                                        StatusUpdate::ClutchFullOfferReceived {
                                                            from_handle_proof: handle_proof,
                                                            payload,
                                                            sender_addr,
                                                        },
                                                        &event_proxy_tcp,
                                                    );
                                                }
                                                0x02 => {
                                                    // KEM response
                                                    crate::log_info(
                                                        "Status: Received ClutchKemResponse via TCP",
                                                    );
                                                    send_status_update(
                                                        &status_tx_tcp,
                                                        StatusUpdate::ClutchKemResponseReceived {
                                                            from_handle_proof: handle_proof,
                                                            payload,
                                                            sender_addr,
                                                        },
                                                        &event_proxy_tcp,
                                                    );
                                                }
                                                _ => {
                                                    crate::log_error(&format!(
                                                        "Status: Unknown TCP payload type: 0x{:02x}",
                                                        payload_type
                                                    ));
                                                }
                                            }
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

                    // Check for PLTP DATA packets first (start with 'd')
                    if is_pltp_data(msg_bytes) {
                        if let Some(data) = PLTPData::from_bytes(msg_bytes) {
                            crate::log_info(&format!(
                                "PLTP: DATA packet seq {} from {}",
                                data.sequence, src_addr
                            ));
                            // Handle data and collect responses (must drop lock before await)
                            let (ack_bytes, complete_bytes, received_data) = {
                                let mut pltp_mgr = pltp_recv.lock().unwrap();
                                let ack = pltp_mgr.handle_data(src_addr, data);
                                let complete = pltp_mgr.check_inbound_complete(src_addr);
                                let data = if complete.is_some() {
                                    pltp_mgr.take_inbound_data(src_addr)
                                } else {
                                    None
                                };
                                (ack, complete, data)
                            };
                            // Now send responses (lock is dropped)
                            if let Some(ack) = ack_bytes {
                                udp::send(&socket_recv, &ack, src_addr).await;
                            }
                            if let Some(complete) = complete_bytes {
                                udp::send(&socket_recv, &complete, src_addr).await;
                                if let Some(data) = received_data {
                                    crate::log_info(&format!(
                                        "PLTP: Transfer complete from {} ({} bytes)",
                                        src_addr,
                                        data.len()
                                    ));
                                    send_status_update(
                                        &status_tx_recv,
                                        StatusUpdate::PLTPReceived {
                                            peer_addr: src_addr,
                                            data,
                                        },
                                        &event_proxy_recv,
                                    );
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

                    // Try to parse as PLTP VSF packets (SPEC, ACK, NAK, CONTROL, COMPLETE)
                    if let Some(pltp_handled) = handle_pltp_vsf_packet(
                        msg_bytes,
                        src_addr,
                        &pltp_recv,
                        &socket_recv,
                        &status_tx_recv,
                        &event_proxy_recv,
                    )
                    .await
                    {
                        if pltp_handled {
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

                                // CLUTCH ceremony messages

                                // ClutchOffer - parallel v2
                                FgtwMessage::ClutchOffer {
                                    timestamp: _,
                                    from_handle_proof,
                                    to_handle_proof,
                                    ephemeral_x25519,
                                    sender_pubkey,
                                    signature,
                                } => {
                                    let provenance = compute_clutch_provenance(
                                        &from_handle_proof,
                                        &to_handle_proof,
                                        &ephemeral_x25519,
                                    );
                                    if !verify_provenance_signature(
                                        &provenance,
                                        &sender_pubkey,
                                        &signature,
                                    ) {
                                        continue;
                                    }

                                    send_status_update(
                                        &status_tx_recv,
                                        StatusUpdate::ClutchOffer {
                                            from_handle_proof,
                                            to_handle_proof,
                                            ephemeral_pubkey: ephemeral_x25519,
                                            sender_addr: src_addr,
                                        },
                                        &event_proxy_recv,
                                    );
                                }

                                // ClutchInit - v1 legacy
                                FgtwMessage::ClutchInit {
                                    timestamp: _,
                                    from_handle_proof,
                                    to_handle_proof,
                                    ephemeral_x25519,
                                    sender_pubkey,
                                    signature,
                                } => {
                                    // Verify signature over provenance
                                    let provenance = compute_clutch_provenance(
                                        &from_handle_proof,
                                        &to_handle_proof,
                                        &ephemeral_x25519,
                                    );
                                    if !verify_provenance_signature(
                                        &provenance,
                                        &sender_pubkey,
                                        &signature,
                                    ) {
                                        continue;
                                    }

                                    // Forward to UI for processing
                                    send_status_update(
                                        &status_tx_recv,
                                        StatusUpdate::ClutchInit {
                                            from_handle_proof,
                                            to_handle_proof,
                                            ephemeral_pubkey: ephemeral_x25519,
                                            sender_addr: src_addr,
                                        },
                                        &event_proxy_recv,
                                    );
                                }

                                FgtwMessage::ClutchResponse {
                                    timestamp: _,
                                    from_handle_proof,
                                    to_handle_proof,
                                    ephemeral_x25519,
                                    sender_pubkey,
                                    signature,
                                } => {
                                    let provenance = compute_clutch_provenance(
                                        &from_handle_proof,
                                        &to_handle_proof,
                                        &ephemeral_x25519,
                                    );
                                    if !verify_provenance_signature(
                                        &provenance,
                                        &sender_pubkey,
                                        &signature,
                                    ) {
                                        continue;
                                    }

                                    send_status_update(
                                        &status_tx_recv,
                                        StatusUpdate::ClutchResponse {
                                            from_handle_proof,
                                            to_handle_proof,
                                            ephemeral_pubkey: ephemeral_x25519,
                                            sender_addr: src_addr,
                                        },
                                        &event_proxy_recv,
                                    );
                                }

                                FgtwMessage::ClutchComplete {
                                    timestamp: _,
                                    from_handle_proof,
                                    to_handle_proof,
                                    proof,
                                    sender_pubkey,
                                    signature,
                                } => {
                                    let provenance = compute_clutch_complete_provenance(
                                        &from_handle_proof,
                                        &to_handle_proof,
                                        &proof,
                                    );
                                    if !verify_provenance_signature(
                                        &provenance,
                                        &sender_pubkey,
                                        &signature,
                                    ) {
                                        continue;
                                    }

                                    send_status_update(
                                        &status_tx_recv,
                                        StatusUpdate::ClutchComplete {
                                            from_handle_proof,
                                            to_handle_proof,
                                            proof,
                                        },
                                        &event_proxy_recv,
                                    );
                                }

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
                            crate::log_info(&format!("Status: Parse error: {}", e));
                        }
                    }
                }
                Err(_) => {}
            }
        }
    });

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

        // Cleanup stale pending pings (older than 5 seconds) and mark as offline
        {
            let mut list = pending.lock().unwrap();
            let now = Instant::now();
            let timeout = Duration::from_secs(5);

            // Find expired pings and send offline status for each
            let expired: Vec<_> = list
                .iter()
                .filter(|ping| now.duration_since(ping.sent_at) >= timeout)
                .map(|ping| ping.recipient_pubkey.clone())
                .collect();

            for pubkey in expired {
                crate::log_info(&format!(
                    "Status: TIMEOUT - {} marked offline",
                    hex::encode(&pubkey.as_bytes()[..8])
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
            }

            list.retain(|ping| now.duration_since(ping.sent_at) < timeout);
        }

        // Process CLUTCH requests
        while let Ok(request) = clutch_rx.try_recv() {
            let timestamp = eagle_time_binary64();

            let msg = match request.message {
                ClutchRequestType::Offer { ephemeral_pubkey } => {
                    // Parallel v2 offer
                    let provenance = compute_clutch_provenance(
                        &request.our_handle_proof,
                        &request.their_handle_proof,
                        &ephemeral_pubkey,
                    );
                    let sig = keypair.sign(&provenance);
                    let mut sig_bytes = [0u8; 64];
                    sig_bytes.copy_from_slice(&sig.to_bytes());

                    crate::log_info(&format!(
                        "Status: Sending CLUTCH_OFFER to {}",
                        request.peer_addr
                    ));

                    FgtwMessage::ClutchOffer {
                        timestamp,
                        from_handle_proof: request.our_handle_proof,
                        to_handle_proof: request.their_handle_proof,
                        ephemeral_x25519: ephemeral_pubkey,
                        sender_pubkey: our_pubkey.clone(),
                        signature: sig_bytes,
                    }
                }
                ClutchRequestType::Init { ephemeral_pubkey } => {
                    // v1 legacy init
                    let provenance = compute_clutch_provenance(
                        &request.our_handle_proof,
                        &request.their_handle_proof,
                        &ephemeral_pubkey,
                    );
                    let sig = keypair.sign(&provenance);
                    let mut sig_bytes = [0u8; 64];
                    sig_bytes.copy_from_slice(&sig.to_bytes());

                    crate::log_info(&format!(
                        "Status: Sending CLUTCH_INIT to {}",
                        request.peer_addr
                    ));

                    FgtwMessage::ClutchInit {
                        timestamp,
                        from_handle_proof: request.our_handle_proof,
                        to_handle_proof: request.their_handle_proof,
                        ephemeral_x25519: ephemeral_pubkey,
                        sender_pubkey: our_pubkey.clone(),
                        signature: sig_bytes,
                    }
                }
                ClutchRequestType::Response { ephemeral_pubkey } => {
                    let provenance = compute_clutch_provenance(
                        &request.our_handle_proof,
                        &request.their_handle_proof,
                        &ephemeral_pubkey,
                    );
                    let sig = keypair.sign(&provenance);
                    let mut sig_bytes = [0u8; 64];
                    sig_bytes.copy_from_slice(&sig.to_bytes());

                    crate::log_info(&format!(
                        "Status: Sending CLUTCH_RESPONSE to {}",
                        request.peer_addr
                    ));

                    FgtwMessage::ClutchResponse {
                        timestamp,
                        from_handle_proof: request.our_handle_proof,
                        to_handle_proof: request.their_handle_proof,
                        ephemeral_x25519: ephemeral_pubkey,
                        sender_pubkey: our_pubkey.clone(),
                        signature: sig_bytes,
                    }
                }
                ClutchRequestType::Complete { proof } => {
                    let provenance = compute_clutch_complete_provenance(
                        &request.our_handle_proof,
                        &request.their_handle_proof,
                        &proof,
                    );
                    let sig = keypair.sign(&provenance);
                    let mut sig_bytes = [0u8; 64];
                    sig_bytes.copy_from_slice(&sig.to_bytes());

                    crate::log_info(&format!(
                        "Status: Sending CLUTCH_COMPLETE to {}",
                        request.peer_addr
                    ));

                    FgtwMessage::ClutchComplete {
                        timestamp,
                        from_handle_proof: request.our_handle_proof,
                        to_handle_proof: request.their_handle_proof,
                        proof,
                        sender_pubkey: our_pubkey.clone(),
                        signature: sig_bytes,
                    }
                }
            };

            let msg_bytes = msg.to_vsf_bytes();
            if !msg_bytes.is_empty() {
                udp::send(&socket, &msg_bytes, request.peer_addr).await;
            }
        }

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

        // Process PLTP send requests (large transfers)
        while let Ok(request) = pltp_rx.try_recv() {
            crate::log_info(&format!(
                "PLTP: Starting outbound transfer to {} ({} bytes)",
                request.peer_addr,
                request.data.len()
            ));
            let mut pltp_mgr = pltp.lock().unwrap();
            let spec_bytes = pltp_mgr.start_send(request.peer_addr, request.data);
            drop(pltp_mgr); // Drop lock before async call
            udp::send(&socket, &spec_bytes, request.peer_addr).await;
        }

        // Process full CLUTCH offer requests (TCP fallback)
        while let Ok(request) = full_offer_rx.try_recv() {
            crate::log_info(&format!(
                "Status: Sending ClutchFullOffer to {} ({} bytes) via TCP",
                request.peer_addr,
                request.payload.len()
            ));
            // Build TCP payload: [type:1][handle_proof:32][payload]
            let mut tcp_data = Vec::with_capacity(1 + 32 + request.payload.len());
            tcp_data.push(0x01); // Type: full offer
            tcp_data.extend_from_slice(&request.our_handle_proof);
            tcp_data.extend_from_slice(&request.payload);

            // Send via TCP - use peer_addr directly (port comes from FGTW)
            match TcpTransport::connect(request.peer_addr, Duration::from_secs(10)) {
                Ok(mut stream) => {
                    if let Err(e) = TcpTransport::send_payload(&mut stream, &tcp_data) {
                        crate::log_error(&format!(
                            "Status: Failed to send ClutchFullOffer to {}: {}",
                            request.peer_addr, e
                        ));
                    } else {
                        crate::log_info(&format!(
                            "Status: ClutchFullOffer sent to {} successfully",
                            request.peer_addr
                        ));
                    }
                }
                Err(e) => {
                    crate::log_error(&format!(
                        "Status: Failed to connect to {} for ClutchFullOffer: {}",
                        request.peer_addr, e
                    ));
                }
            }
        }

        // Process CLUTCH KEM response requests (TCP fallback)
        while let Ok(request) = kem_response_rx.try_recv() {
            crate::log_info(&format!(
                "Status: Sending ClutchKemResponse to {} ({} bytes) via TCP",
                request.peer_addr,
                request.payload.len()
            ));
            // Build TCP payload: [type:1][handle_proof:32][payload]
            let mut tcp_data = Vec::with_capacity(1 + 32 + request.payload.len());
            tcp_data.push(0x02); // Type: KEM response
            tcp_data.extend_from_slice(&request.our_handle_proof);
            tcp_data.extend_from_slice(&request.payload);

            // Send via TCP - use peer_addr directly (port comes from FGTW)
            match TcpTransport::connect(request.peer_addr, Duration::from_secs(10)) {
                Ok(mut stream) => {
                    if let Err(e) = TcpTransport::send_payload(&mut stream, &tcp_data) {
                        crate::log_error(&format!(
                            "Status: Failed to send ClutchKemResponse to {}: {}",
                            request.peer_addr, e
                        ));
                    } else {
                        crate::log_info(&format!(
                            "Status: ClutchKemResponse sent to {} successfully",
                            request.peer_addr
                        ));
                    }
                }
                Err(e) => {
                    crate::log_error(&format!(
                        "Status: Failed to connect to {} for ClutchKemResponse: {}",
                        request.peer_addr, e
                    ));
                }
            }
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
                }
            }
        }

        // PLTP periodic tick - check timeouts and retransmit
        {
            let mut pltp_mgr = pltp.lock().unwrap();
            let to_send = pltp_mgr.tick();
            drop(pltp_mgr); // Drop lock before async calls
            for (addr, pkt) in to_send {
                udp::send(&socket, &pkt, addr).await;
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

/// Compute provenance hash for CLUTCH init/response messages
fn compute_clutch_provenance(
    from_handle_proof: &[u8; 32],
    to_handle_proof: &[u8; 32],
    ephemeral_pubkey: &[u8; 32],
) -> [u8; 32] {
    use blake3::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(from_handle_proof);
    hasher.update(to_handle_proof);
    hasher.update(ephemeral_pubkey);
    *hasher.finalize().as_bytes()
}

/// Compute provenance hash for CLUTCH complete message
fn compute_clutch_complete_provenance(
    from_handle_proof: &[u8; 32],
    to_handle_proof: &[u8; 32],
    proof: &[u8; 32],
) -> [u8; 32] {
    use blake3::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(from_handle_proof);
    hasher.update(to_handle_proof);
    hasher.update(proof);
    *hasher.finalize().as_bytes()
}

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
/// Handle PLTP VSF packets (SPEC, ACK, NAK, CONTROL, COMPLETE)
/// Returns Some(true) if packet was handled, Some(false) if not a PLTP packet, None on error
async fn handle_pltp_vsf_packet(
    msg_bytes: &[u8],
    src_addr: SocketAddr,
    pltp: &Arc<Mutex<PLTPManager>>,
    socket: &Arc<tokio::net::UdpSocket>,
    status_tx: &Sender<StatusUpdate>,
    event_proxy: &OptionalEventProxy,
) -> Option<bool> {
    // Try to parse VSF fields to determine packet type
    let fields = parse_pltp_vsf_fields(msg_bytes)?;
    if fields.is_empty() {
        return Some(false);
    }

    // Look for packet type field
    let packet_type = fields.iter().find(|(name, _)| name == "type")?;
    let type_str = match &packet_type.1 {
        vsf::VsfType::d(s) => s.as_str(),
        _ => return Some(false),
    };

    match type_str {
        "pltp_spec" => {
            if let Some(spec) = PLTPSpec::from_vsf_fields(&fields) {
                crate::log_info(&format!(
                    "PLTP: SPEC received from {} - {} packets, {} bytes",
                    src_addr, spec.total_packets, spec.total_size
                ));
                let spec_ack = {
                    let mut pltp_mgr = pltp.lock().unwrap();
                    pltp_mgr.handle_spec(src_addr, spec)
                };
                udp::send(socket, &spec_ack, src_addr).await;
                return Some(true);
            }
        }
        "pltp_ack" => {
            if let Some(ack) = PLTPAck::from_vsf_fields(&fields) {
                let (response_packets, is_complete) = {
                    let mut pltp_mgr = pltp.lock().unwrap();
                    let packets = pltp_mgr.handle_ack(src_addr, ack);
                    let complete = pltp_mgr.is_outbound_complete(&src_addr);
                    if complete {
                        pltp_mgr.remove_outbound(&src_addr);
                    }
                    (packets, complete)
                };
                for pkt in response_packets {
                    udp::send(socket, &pkt, src_addr).await;
                }
                if is_complete {
                    crate::log_info(&format!("PLTP: Outbound transfer to {} complete", src_addr));
                    send_status_update(
                        status_tx,
                        StatusUpdate::PLTPSendComplete {
                            peer_addr: src_addr,
                        },
                        event_proxy,
                    );
                }
                return Some(true);
            }
        }
        "pltp_nak" => {
            if let Some(nak) = PLTPNak::from_vsf_fields(&fields) {
                crate::log_info(&format!(
                    "PLTP: NAK received from {} - {} missing",
                    src_addr,
                    nak.missing_sequences.len()
                ));
                let response_packets = {
                    let mut pltp_mgr = pltp.lock().unwrap();
                    pltp_mgr.handle_nak(src_addr, nak)
                };
                for pkt in response_packets {
                    udp::send(socket, &pkt, src_addr).await;
                }
                return Some(true);
            }
        }
        "pltp_control" => {
            if let Some(control) = PLTPControl::from_vsf_fields(&fields) {
                crate::log_info(&format!("PLTP: CONTROL received from {}", src_addr));
                let mut pltp_mgr = pltp.lock().unwrap();
                pltp_mgr.handle_control(src_addr, control);
                return Some(true);
            }
        }
        "pltp_complete" => {
            if let Some(complete) = PLTPComplete::from_vsf_fields(&fields) {
                crate::log_info(&format!(
                    "PLTP: COMPLETE received from {} - success={}",
                    src_addr, complete.success
                ));
                let is_complete = {
                    let mut pltp_mgr = pltp.lock().unwrap();
                    pltp_mgr.handle_complete(src_addr, complete);
                    let complete = pltp_mgr.is_outbound_complete(&src_addr);
                    if complete {
                        pltp_mgr.remove_outbound(&src_addr);
                    }
                    complete
                };
                if is_complete {
                    send_status_update(
                        status_tx,
                        StatusUpdate::PLTPSendComplete {
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

    Some(false)
}

/// Parse LAN discovery packet from main UDP socket
/// Returns StatusUpdate::LanPeerDiscovered if valid, None otherwise
fn parse_lan_discovery(packet: &[u8], src_addr: SocketAddr) -> Option<StatusUpdate> {
    let (handle_proof, local_ip, port) = udp::parse_lan_discovery(packet, src_addr)?;
    Some(StatusUpdate::LanPeerDiscovered {
        handle_proof,
        local_ip,
        port,
    })
}

/// Parse VSF fields from bytes (simplified for PLTP packet detection)
fn parse_pltp_vsf_fields(bytes: &[u8]) -> Option<Vec<(String, vsf::VsfType)>> {
    use vsf::file_format::VsfHeader;
    use vsf::parse;

    let (_, header_end) = VsfHeader::decode(bytes).ok()?;

    let mut ptr = header_end;
    if ptr >= bytes.len() || bytes[ptr] != b'[' {
        return None;
    }
    ptr += 1;

    // Parse section name
    let _ = parse(bytes, &mut ptr).ok()?;

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

    Some(fields)
}
