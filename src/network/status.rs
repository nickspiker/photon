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

use crate::network::fgtw::FgtwMessage;
use crate::network::fgtw::Keypair;
use crate::types::DevicePubkey;
use std::net::{SocketAddr, UdpSocket};
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
    pub salt: [u8; 64],
    pub ciphertext: Vec<u8>,
}

/// Request to send a message acknowledgment
#[derive(Clone)]
pub struct AckRequest {
    pub peer_addr: SocketAddr,
    pub our_handle_proof: [u8; 32],
    pub sequence: u64,
}

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
        salt: [u8; 64],
        ciphertext: Vec<u8>,
        sender_addr: SocketAddr,
    },
    /// Message acknowledgment received
    MessageAck {
        from_handle_proof: [u8; 32],
        sequence: u64,
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
pub struct StatusChecker {
    ping_sender: Sender<PingRequest>,
    clutch_sender: Sender<ClutchRequest>,
    message_sender: Sender<MessageRequest>,
    ack_sender: Sender<AckRequest>,
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
                    ping_rx,
                    clutch_rx,
                    message_rx,
                    ack_rx,
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

        thread::spawn(move || {
            crate::log_info("Status: Background thread started");
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime for StatusChecker");

            rt.block_on(async move {
                run_checker(
                    socket, keypair, our_pubkey, ping_rx, clutch_rx, message_rx, ack_rx, status_tx,
                    contacts, None,
                )
                .await;
            });
        });

        Ok(Self {
            ping_sender: ping_tx,
            clutch_sender: clutch_tx,
            message_sender: message_tx,
            ack_sender: ack_tx,
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
        crate::log_info("Status: Sending NetworkUpdate wake event");
        match proxy.send_event(PhotonEvent::NetworkUpdate) {
            Ok(()) => crate::log_info("Status: Wake event sent successfully"),
            Err(e) => crate::log_error(&format!("Status: Failed to send wake event: {:?}", e)),
        }
    } else {
        crate::log_info("Status: No event_proxy available for wake");
    }
}

/// Main checker loop running in tokio
async fn run_checker(
    std_socket: Arc<UdpSocket>,
    keypair: crate::network::fgtw::Keypair,
    our_pubkey: DevicePubkey,
    ping_rx: Receiver<PingRequest>,
    clutch_rx: Receiver<ClutchRequest>,
    message_rx: Receiver<MessageRequest>,
    ack_rx: Receiver<AckRequest>,
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

    // Pending pings - just a Vec, ~10 contacts max
    let pending: Arc<Mutex<Vec<PendingPing>>> = Arc::new(Mutex::new(Vec::new()));

    let socket_recv = socket.clone();
    let pending_recv = pending.clone();
    let our_pubkey_recv = our_pubkey.clone();
    let keypair_recv = keypair.clone();
    let status_tx_recv = status_tx.clone();
    let contacts_recv = contacts.clone();
    let event_proxy_recv = event_proxy.clone();

    // Spawn receiver task
    tokio::spawn(async move {
        crate::log_info("Status: Receiver task started, waiting for UDP packets...");
        let mut buf = [0u8; 2048];
        loop {
            match socket_recv.recv_from(&mut buf).await {
                Ok((len, src_addr)) => {
                    let msg_bytes = &buf[..len];
                    crate::log_info(&format!(
                        "Status: UDP received {} bytes from {}",
                        len, src_addr
                    ));
                    // Save received message for vsfinfo inspection
                    let _ = std::fs::write("/tmp/photon-received.vsf", msg_bytes);
                    // Show VSF inspector in development builds
                    #[cfg(feature = "development")]
                    crate::log_info(&vsf_inspect(msg_bytes, "RX", &src_addr.to_string()));
                    match FgtwMessage::from_vsf_bytes(msg_bytes) {
                        Ok(message) => {
                            match message {
                                FgtwMessage::StatusPing {
                                    timestamp,
                                    sender_pubkey,
                                    provenance_hash,
                                    signature,
                                } => {
                                    crate::log_info(&format!(
                                        "Status: PING received from {} ({})",
                                        src_addr,
                                        hex::encode(&sender_pubkey.as_bytes()[..8])
                                    ));
                                    crate::log_info(&format!("  timestamp: {:.6}", timestamp));
                                    crate::log_info(&format!(
                                        "  provenance: {}",
                                        hex::encode(&provenance_hash[..16])
                                    ));
                                    crate::log_info(&format!(
                                        "  signature: {}...",
                                        hex::encode(&signature[..16])
                                    ));

                                    // Only respond to contacts (friends only)
                                    let is_contact = {
                                        let list = contacts_recv.lock().unwrap();
                                        list.iter().any(|p| *p == sender_pubkey)
                                    };
                                    if !is_contact {
                                        crate::log_info("  -> IGNORED (not in contacts)");
                                        continue;
                                    }

                                    // Verify signature
                                    if !verify_provenance_signature(
                                        &provenance_hash,
                                        &sender_pubkey,
                                        &signature,
                                    ) {
                                        crate::log_info("  -> REJECTED (bad signature)");
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
                                    crate::log_info("  -> marked ONLINE (from ping)");

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
                                        crate::log_info(&format!(
                                            "  -> PONG sent ({} bytes)",
                                            pong_bytes.len()
                                        ));
                                        // Save to file for vsfinfo inspection
                                        let _ = std::fs::write(
                                            "/tmp/photon-pong-sent.vsf",
                                            &pong_bytes,
                                        );
                                        #[cfg(feature = "development")]
                                        crate::log_info(&vsf_inspect(
                                            &pong_bytes,
                                            "TX PONG",
                                            &src_addr.to_string(),
                                        ));
                                        let _ = socket_recv.send_to(&pong_bytes, src_addr).await;
                                    }
                                }

                                FgtwMessage::StatusPong {
                                    timestamp,
                                    responder_pubkey,
                                    provenance_hash,
                                    signature,
                                } => {
                                    crate::log_info(&format!(
                                        "Status: PONG received from {} ({})",
                                        src_addr,
                                        hex::encode(&responder_pubkey.as_bytes()[..8])
                                    ));
                                    crate::log_info(&format!("  timestamp: {:.6}", timestamp));
                                    crate::log_info(&format!(
                                        "  provenance: {}",
                                        hex::encode(&provenance_hash[..16])
                                    ));
                                    crate::log_info(&format!(
                                        "  signature: {}...",
                                        hex::encode(&signature[..16])
                                    ));

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
                                        None => {
                                            crate::log_info(
                                                "  -> IGNORED (no matching pending ping)",
                                            );
                                            continue;
                                        }
                                    };

                                    // Verify responder matches who we pinged
                                    if responder_pubkey != pending_ping.recipient_pubkey {
                                        crate::log_info("  -> REJECTED (responder mismatch)");
                                        continue;
                                    }

                                    // Verify signature
                                    if !verify_provenance_signature(
                                        &provenance_hash,
                                        &responder_pubkey,
                                        &signature,
                                    ) {
                                        crate::log_info("  -> REJECTED (bad signature)");
                                        continue;
                                    }

                                    crate::log_info("  -> ONLINE confirmed!");

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
                                    crate::log_info(&format!(
                                        "Status: CLUTCH_OFFER received from {}",
                                        src_addr
                                    ));
                                    let _ = std::fs::write(
                                        "/tmp/photon-clutch-offer-rx.vsf",
                                        msg_bytes,
                                    );

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
                                        crate::log_info("  -> REJECTED (bad signature)");
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
                                    crate::log_info(&format!(
                                        "Status: CLUTCH_INIT received from {}",
                                        src_addr
                                    ));
                                    // Save to separate file for vsfinfo inspection
                                    let _ =
                                        std::fs::write("/tmp/photon-clutch-init-rx.vsf", msg_bytes);

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
                                        crate::log_info("  -> REJECTED (bad signature)");
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
                                    crate::log_info(&format!(
                                        "Status: CLUTCH_RESPONSE received from {}",
                                        src_addr
                                    ));
                                    // Save to separate file for vsfinfo inspection
                                    let _ = std::fs::write(
                                        "/tmp/photon-clutch-response-rx.vsf",
                                        msg_bytes,
                                    );

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
                                        crate::log_info("  -> REJECTED (bad signature)");
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
                                    crate::log_info(&format!(
                                        "Status: CLUTCH_COMPLETE received from {}",
                                        src_addr
                                    ));
                                    // Save to separate file for vsfinfo inspection
                                    let _ = std::fs::write(
                                        "/tmp/photon-clutch-complete-rx.vsf",
                                        msg_bytes,
                                    );

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
                                        crate::log_info("  -> REJECTED (bad signature)");
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
                                    salt,
                                    ciphertext,
                                    sender_pubkey,
                                    signature,
                                } => {
                                    crate::log_info(&format!(
                                        "Status: CHAT_MESSAGE received from {} (seq {})",
                                        src_addr, sequence
                                    ));
                                    let _ = std::fs::write("/tmp/photon-msg-rx.vsf", msg_bytes);

                                    // Verify signature
                                    let provenance =
                                        compute_msg_provenance(&from_handle_proof, sequence, &salt);
                                    if !verify_provenance_signature(
                                        &provenance,
                                        &sender_pubkey,
                                        &signature,
                                    ) {
                                        crate::log_info("  -> REJECTED (bad signature)");
                                        continue;
                                    }

                                    // Forward to UI for decryption
                                    send_status_update(
                                        &status_tx_recv,
                                        StatusUpdate::ChatMessage {
                                            from_handle_proof,
                                            sequence,
                                            salt,
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
                                    sender_pubkey,
                                    signature,
                                } => {
                                    crate::log_info(&format!(
                                        "Status: MESSAGE_ACK received from {} (seq {})",
                                        src_addr, sequence
                                    ));

                                    // Verify signature
                                    let provenance =
                                        compute_ack_provenance(&from_handle_proof, sequence);
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

                crate::log_info(&format!(
                    "Status: PING sent to {} ({}) - {} bytes",
                    request.peer_addr,
                    hex::encode(&request.peer_pubkey.as_bytes()[..8]),
                    msg_bytes.len()
                ));

                // Save to file for vsfinfo inspection
                let _ = std::fs::write("/tmp/photon-ping.vsf", &msg_bytes);
                #[cfg(feature = "development")]
                crate::log_info(&vsf_inspect(
                    &msg_bytes,
                    "TX PING",
                    &request.peer_addr.to_string(),
                ));

                // Store pending ping
                {
                    let mut list = pending.lock().unwrap();
                    list.push(PendingPing {
                        recipient_pubkey: request.peer_pubkey.clone(),
                        provenance_hash,
                        sent_at: Instant::now(),
                    });
                }

                let _ = socket.send_to(&msg_bytes, request.peer_addr).await;
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
                // Save to separate file based on message type for vsfinfo inspection
                let filename = match request.message {
                    ClutchRequestType::Offer { .. } => "/tmp/photon-clutch-offer-tx.vsf",
                    ClutchRequestType::Init { .. } => "/tmp/photon-clutch-init-tx.vsf",
                    ClutchRequestType::Response { .. } => "/tmp/photon-clutch-response-tx.vsf",
                    ClutchRequestType::Complete { .. } => "/tmp/photon-clutch-complete-tx.vsf",
                };
                let _ = std::fs::write(filename, &msg_bytes);
                #[cfg(feature = "development")]
                crate::log_info(&vsf_inspect(
                    &msg_bytes,
                    "TX CLUTCH",
                    &request.peer_addr.to_string(),
                ));
                let _ = socket.send_to(&msg_bytes, request.peer_addr).await;
            }
        }

        // Process message requests (encrypted chat messages)
        while let Ok(request) = message_rx.try_recv() {
            let timestamp = eagle_time_binary64();

            // Compute provenance and sign
            let provenance =
                compute_msg_provenance(&request.our_handle_proof, request.sequence, &request.salt);
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
                salt: request.salt,
                ciphertext: request.ciphertext,
                sender_pubkey: our_pubkey.clone(),
                signature: sig_bytes,
            };

            let msg_bytes = msg.to_vsf_bytes();
            if !msg_bytes.is_empty() {
                let _ = std::fs::write("/tmp/photon-msg-tx.vsf", &msg_bytes);
                #[cfg(feature = "development")]
                crate::log_info(&vsf_inspect(
                    &msg_bytes,
                    "TX MSG",
                    &request.peer_addr.to_string(),
                ));
                let _ = socket.send_to(&msg_bytes, request.peer_addr).await;
            }
        }

        // Process ACK requests (message acknowledgments)
        while let Ok(request) = ack_rx.try_recv() {
            let timestamp = eagle_time_binary64();

            // Compute provenance and sign
            let provenance = compute_ack_provenance(&request.our_handle_proof, request.sequence);
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
                sender_pubkey: our_pubkey.clone(),
                signature: sig_bytes,
            };

            let msg_bytes = msg.to_vsf_bytes();
            if !msg_bytes.is_empty() {
                let _ = std::fs::write("/tmp/photon-ack-tx.vsf", &msg_bytes);
                #[cfg(feature = "development")]
                crate::log_info(&vsf_inspect(
                    &msg_bytes,
                    "TX ACK",
                    &request.peer_addr.to_string(),
                ));
                let _ = socket.send_to(&msg_bytes, request.peer_addr).await;
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
fn compute_msg_provenance(
    from_handle_proof: &[u8; 32],
    sequence: u64,
    salt: &[u8; 64],
) -> [u8; 32] {
    use blake3::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(from_handle_proof);
    hasher.update(&sequence.to_le_bytes());
    hasher.update(salt);
    *hasher.finalize().as_bytes()
}

/// Compute provenance hash for message acknowledgment
fn compute_ack_provenance(from_handle_proof: &[u8; 32], sequence: u64) -> [u8; 32] {
    use blake3::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(from_handle_proof);
    hasher.update(&sequence.to_le_bytes());
    hasher.update(b"ack");
    *hasher.finalize().as_bytes()
}

/// Format a VSF packet as a human-readable inspection string (like vsfinfo)
/// Public for use across network modules (FGTW transport, P2P, etc.)
#[cfg(feature = "development")]
pub fn vsf_inspect(data: &[u8], direction: &str, addr: &str) -> String {
    let mut result = format!(
        "═══ VSF {} {} ({} bytes) ═══\n",
        direction,
        addr,
        data.len()
    );

    // Try to parse as VSF file first, then section, fall back to hex dump
    match vsf::inspect::inspect_vsf(data) {
        Ok(formatted) => result.push_str(&strip_ansi_if_needed(&formatted)),
        Err(_) => {
            // Not a complete VSF file - try section format
            match vsf::inspect::inspect_section(data) {
                Ok(formatted) => result.push_str(&strip_ansi_if_needed(&formatted)),
                Err(_) => {
                    // Fall back to hex dump
                    result.push_str(&vsf::inspect::hex_dump(data));
                }
            }
        }
    }

    result
}

/// Strip ANSI color codes on platforms that don't support them (Android)
#[cfg(feature = "development")]
fn strip_ansi_if_needed(s: &str) -> String {
    #[cfg(target_os = "android")]
    {
        // Strip ANSI escape sequences on Android
        let mut result = String::with_capacity(s.len());
        let mut in_escape = false;
        for c in s.chars() {
            if c == '\x1b' {
                in_escape = true;
            } else if in_escape {
                if c == 'm' {
                    in_escape = false;
                }
            } else {
                result.push(c);
            }
        }
        result
    }
    #[cfg(not(target_os = "android"))]
    {
        s.to_string()
    }
}
