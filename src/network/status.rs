//! Contact status checker
//!
//! Sends UDP pings to contacts and receives pongs to determine online status.
//! Uses the shared UDP socket from HandleQuery (the same port announced to FGTW).
//!
//! Protocol uses VSF-spec provenance hash for replay protection:
//! - provenance_hash = BLAKE3(sender_pubkey || timestamp_nanos)
//! - Signature covers the provenance_hash
//! - Timestamp uses nanosecond precision (ef6) for uniqueness

use crate::network::fgtw::FgtwMessage;
use crate::network::fgtw::Keypair;
use crate::types::PublicIdentity;
use std::net::{SocketAddr, UdpSocket};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Shared contact list - UI updates this, background thread reads it
pub type ContactPubkeys = Arc<Mutex<Vec<PublicIdentity>>>;

/// Get current Eagle Time (seconds since Apollo 11 landing: July 20, 1969, 20:17:40 UTC)
fn eagle_time_nanos() -> f64 {
    vsf::eagle_time_nanos()
}

/// Compute provenance hash = BLAKE3(sender_pubkey || timestamp_bytes)
fn compute_provenance_hash(sender_pubkey: &PublicIdentity, timestamp: f64) -> [u8; 32] {
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
    pub peer_pubkey: PublicIdentity,
}

/// Status update from the checker
#[derive(Clone, Debug)]
pub struct StatusUpdate {
    pub peer_pubkey: PublicIdentity,
    pub is_online: bool,
    // Note: avatar is fetched by handle, not exchanged in ping/pong
    // Storage key = BLAKE3(BLAKE3(handle) || "avatar")
}

/// Pending ping waiting for pong
struct PendingPing {
    recipient_pubkey: PublicIdentity,
    provenance_hash: [u8; 32],
    sent_at: Instant,
}

/// Contact status checker
///
/// Spawns a background thread to handle async UDP ping/pong.
/// Uses the shared UDP socket from HandleQuery.
pub struct StatusChecker {
    ping_sender: Sender<PingRequest>,
    status_receiver: Receiver<StatusUpdate>,
}

impl StatusChecker {
    /// Create a new status checker using a shared socket
    ///
    /// `socket` is the shared UDP socket from HandleQuery (same port announced to FGTW).
    /// `keypair` is the device keypair (same one used for FGTW registration).
    /// `contacts` is shared with UI - only respond to pings from pubkeys in this list.
    pub fn new(
        socket: Arc<UdpSocket>,
        keypair: Keypair,
        contacts: ContactPubkeys,
    ) -> Result<Self, String> {
        let (ping_tx, ping_rx) = channel::<PingRequest>();
        let (status_tx, status_rx) = channel::<StatusUpdate>();

        let our_pubkey = PublicIdentity::from_bytes(keypair.public.to_bytes());

        // Note: Avatar is no longer exchanged in ping/pong
        // Contacts fetch avatar by handle: storage key = BLAKE3(BLAKE3(handle) || "avatar")

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
                run_checker(socket, keypair, our_pubkey, ping_rx, status_tx, contacts).await;
            });
        });

        Ok(Self {
            ping_sender: ping_tx,
            status_receiver: status_rx,
        })
    }

    /// Request to ping a contact (non-blocking)
    pub fn ping(&self, peer_addr: SocketAddr, peer_pubkey: PublicIdentity) {
        let _ = self.ping_sender.send(PingRequest {
            peer_addr,
            peer_pubkey,
        });
    }

    /// Check for status updates (non-blocking)
    pub fn try_recv(&self) -> Option<StatusUpdate> {
        self.status_receiver.try_recv().ok()
    }
}

/// Main checker loop running in tokio
async fn run_checker(
    std_socket: Arc<UdpSocket>,
    keypair: crate::network::fgtw::Keypair,
    our_pubkey: PublicIdentity,
    ping_rx: Receiver<PingRequest>,
    status_tx: Sender<StatusUpdate>,
    contacts: ContactPubkeys,
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

                                    // Send pong (no avatar_id - avatars are fetched by handle)
                                    let sig = keypair_recv.sign(&provenance_hash);
                                    let mut sig_bytes = [0u8; 64];
                                    sig_bytes.copy_from_slice(&sig.to_bytes());

                                    let pong = FgtwMessage::StatusPong {
                                        timestamp: eagle_time_nanos(),
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
                                    let _ = status_tx_recv.send(StatusUpdate {
                                        peer_pubkey: responder_pubkey,
                                        is_online: true,
                                    });
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
                let timestamp = eagle_time_nanos();
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
                let _ = status_tx.send(StatusUpdate {
                    peer_pubkey: pubkey,
                    is_online: false,
                });
            }

            list.retain(|ping| now.duration_since(ping.sent_at) < timeout);
        }
    }
}

/// Verify Ed25519 signature on provenance hash
fn verify_provenance_signature(
    provenance_hash: &[u8; 32],
    signer_pubkey: &PublicIdentity,
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
