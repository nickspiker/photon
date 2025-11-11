// Handle query protocol for checking handle attestation status
//
// Network layer for querying the FGTW (Fractal Gradient Trust Web) to check if a handle
// has been attested (claimed) or is available.

use crate::network::fgtw::{FgtwMessage, FgtwTransport, PeerRecord};
use crate::types::{Handle, PublicIdentity};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

/// Result of a handle query
#[derive(Debug, Clone)]
pub enum QueryResult {
    Unattested,                  // Handle is available
    AlreadyAttested(PeerRecord), // Handle is claimed, with peer info
}

/// Handle query request/response channel
pub struct HandleQuery {
    sender: Sender<String>,
    receiver: Receiver<QueryResult>,
    transport: Arc<Mutex<Option<Arc<FgtwTransport>>>>,
}

impl HandleQuery {
    /// Create a new handle query system with FGTW
    pub fn new(_our_identity: PublicIdentity) -> Self {
        let (tx_request, rx_request) = channel::<String>();
        let (tx_response, rx_response) = channel::<QueryResult>();

        let transport = Arc::new(Mutex::new(None::<Arc<FgtwTransport>>));
        let transport_clone = transport.clone();

        // Spawn worker thread to handle FGTW queries
        thread::spawn(move || {
            println!("Network: FGTW query worker initialized");

            while let Ok(username) = rx_request.recv() {
                println!("Network: Querying handle '@{}'...", username);

                // Wait for transport to be set
                let transport_arc = loop {
                    let guard = transport_clone.lock().unwrap();
                    if let Some(t) = &*guard {
                        break t.clone();
                    }
                    drop(guard);
                    thread::sleep(std::time::Duration::from_millis(100));
                };

                // Get all known peers from peer store
                let peer_store = transport_arc.peer_store();
                let store = peer_store.lock().unwrap();

                // First check local peer store
                let handle_hash = crate::types::Handle::username_to_infohash(&username);
                let devices = store.get_devices_for_handle(&handle_hash);
                if !devices.is_empty() {
                    println!(
                        "Network: Handle '@{}' is CLAIMED (found {} device(s) in local store)",
                        username,
                        devices.len()
                    );
                    let result = QueryResult::AlreadyAttested(devices[0].clone());
                    if tx_response.send(result).is_err() {
                        eprintln!("Network: ERROR - Failed to send response (receiver dropped)");
                    }
                    continue;
                }

                // TODO: Query remote peers via FGTW FindNode/Query messages
                // For now, if not in local store, consider it unattested
                println!("Network: Handle '@{}' is AVAILABLE (not found)", username);
                let result = QueryResult::Unattested;

                if tx_response.send(result).is_err() {
                    eprintln!("Network: ERROR - Failed to send response (receiver dropped)");
                }
            }
        });

        Self {
            sender: tx_request,
            receiver: rx_response,
            transport,
        }
    }

    /// Set the FGTW transport (must be called after creating transport)
    pub fn set_transport(&self, t: Arc<FgtwTransport>) {
        let mut guard = self.transport.lock().unwrap();
        *guard = Some(t);
    }

    /// Query a handle (non-blocking)
    pub fn query(&self, handle: String) {
        let _ = self.sender.send(handle);
    }

    /// Check if a response is ready (non-blocking)
    pub fn try_recv(&self) -> Option<QueryResult> {
        self.receiver.try_recv().ok()
    }
}

/// Announce your handle to the FGTW (blocking)
pub fn announce_handle(
    handle: &Handle,
    port: u16,
    transport: &FgtwTransport,
) -> Result<(), String> {
    println!("Network: Announcing handle '{}' on FGTW...", handle.text);

    // Get bootstrap peers to announce to
    let peer_store = transport.peer_store();
    let store = peer_store.lock().unwrap();
    let peers = store.get_all_peers();

    if peers.is_empty() {
        return Err("No peers available to announce to".to_string());
    }

    println!("Network: Announcing to {} peer(s)", peers.len());

    // Announce to all known peers
    let handle_hash = handle.to_infohash();
    for peer in peers.iter().take(10) {
        // Announce to first 10 peers
        let message = FgtwMessage::Announce {
            handle_hash,
            device_pubkey: handle.key.clone(),
            port,
        };

        // Send announce message (async, don't wait for response)
        let peer_addr = peer.ip.to_string();
        tokio::task::spawn(async move {
            // This runs in background, we don't care about errors
            let _ = transport_send(&peer_addr, message).await;
        });
    }

    println!("Network: Handle '{}' announced successfully!", handle.text);
    Ok(())
}

// Helper function for sending FGTW messages (to avoid circular dependency)
async fn transport_send(peer_addr: &str, message: FgtwMessage) -> Result<FgtwMessage, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let mut stream = TcpStream::connect(peer_addr)
        .await
        .map_err(|e| format!("Failed to connect to {}: {}", peer_addr, e))?;

    let msg_bytes = message.to_vsf_bytes();
    let len = msg_bytes.len() as u32;
    stream
        .write_all(&len.to_be_bytes())
        .await
        .map_err(|e| format!("Write error: {}", e))?;
    stream
        .write_all(&msg_bytes)
        .await
        .map_err(|e| format!("Write error: {}", e))?;
    stream
        .flush()
        .await
        .map_err(|e| format!("Flush error: {}", e))?;

    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .await
        .map_err(|e| format!("Read error: {}", e))?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > 10_000_000 {
        return Err(format!("Message too large: {} bytes", len));
    }

    let mut msg_buf = vec![0u8; len];
    stream
        .read_exact(&mut msg_buf)
        .await
        .map_err(|e| format!("Read error: {}", e))?;

    FgtwMessage::from_vsf_bytes(&msg_buf)
}
