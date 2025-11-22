// Handle query protocol for checking handle attestation status
//
// Network layer for querying the FGTW (Fractal Gradient Trust Web) to check if a handle
// has been attested (claimed) or is available.

use crate::network::fgtw::{FgtwMessage, FgtwTransport, PeerRecord};
use crate::types::{Handle, PublicIdentity};
use crate::ui::PhotonEvent;
use std::net::UdpSocket;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use winit::event_loop::EventLoopProxy;

/// Result of a handle query
#[derive(Debug, Clone)]
pub enum QueryResult {
    Success,                     // Successfully attested/registered
    AlreadyAttested(PeerRecord), // Handle is claimed by another device
    Error(String),               // Error during attestation
}

/// Handle query request/response channel
pub struct HandleQuery {
    sender: Sender<String>,
    receiver: Receiver<QueryResult>,
    transport: Arc<Mutex<Option<Arc<FgtwTransport>>>>,
    socket: Arc<UdpSocket>,
    port: u16,
    online_receiver: Receiver<bool>,
}

impl HandleQuery {
    /// Create a new handle query system with FGTW
    pub fn new(_our_identity: PublicIdentity, event_proxy: EventLoopProxy<PhotonEvent>) -> Self {
        let (tx_request, rx_request) = channel::<String>();
        let (tx_response, rx_response) = channel::<QueryResult>();
        let (tx_online, rx_online) = channel::<bool>();

        // Bind UDP socket to port 0 (OS picks an available port)
        let socket = UdpSocket::bind("0.0.0.0:0").expect("Failed to bind UDP socket");
        let port = socket
            .local_addr()
            .expect("Failed to get socket address")
            .port();
        println!("Network: Listening on UDP port {}", port);
        let socket = Arc::new(socket);

        let transport = Arc::new(Mutex::new(None::<Arc<FgtwTransport>>));
        let transport_clone = transport.clone();
        let port_clone = port;

        // Spawn connectivity check thread
        let event_proxy_connectivity = event_proxy.clone();
        thread::spawn(move || {
            // Simple HTTP GET to check if FGTW is reachable
            let online = match reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
            {
                Ok(client) => match client.get("https://fgtw.org/status").send() {
                    Ok(resp) if resp.status().is_success() => true,
                    _ => false,
                },
                Err(_) => false,
            };

            // Send via channel (for backward compatibility with try_recv_online)
            let _ = tx_online.send(online);

            // Wake event loop via proxy - this is the key cross-thread signal
            let _ = event_proxy_connectivity.send_event(PhotonEvent::ConnectivityChanged(online));
        });

        // Spawn worker thread to handle FGTW queries
        thread::spawn(move || {
            println!("Network: FGTW query worker initialized");

            while let Ok(username) = rx_request.recv() {
                println!("Network: Querying handle '{}'...", username);

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
                let handle_proof = crate::types::Handle::username_to_handle_proof(&username);
                let devices = store.get_devices_for_handle(&handle_proof);
                if !devices.is_empty() {
                    println!(
                        "Network: Handle '{}' is CLAIMED (found {} device(s) in local store)",
                        username,
                        devices.len()
                    );
                    let result = QueryResult::AlreadyAttested(devices[0].clone());
                    if tx_response.send(result).is_err() {
                        eprintln!("Network: ERROR - Failed to send response (receiver dropped)");
                    }
                    continue;
                }

                // Not in local store - query fgtw.org
                drop(store); // Release lock before network I/O
                println!("Network: Querying fgtw.org for handle '{}'...", username);

                // Load keys for query
                let paths = match crate::network::fgtw::FgtwPaths::new() {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("Network: ERROR - Failed to get FGTW paths: {}", e);
                        let result = QueryResult::Error(format!("Failed to get paths: {}", e));
                        let _ = tx_response.send(result);
                        continue;
                    }
                };

                let device_keypair =
                    match crate::network::fgtw::load_or_generate_device_key(&paths.device_key) {
                        Ok(kp) => kp,
                        Err(e) => {
                            eprintln!("Network: ERROR - Failed to load device key: {}", e);
                            let result =
                                QueryResult::Error(format!("Failed to load device key: {}", e));
                            let _ = tx_response.send(result);
                            continue;
                        }
                    };

                // Query FGTW by announcing ourselves (this returns the peer list for the handle)
                // Port 0 means we're just querying, not actually announcing availability
                // We need a tokio runtime since the worker is a plain thread
                let peers = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create tokio runtime")
                    .block_on(crate::network::fgtw::bootstrap::load_bootstrap_peers(
                        &device_keypair,
                        handle_proof,
                        port_clone,
                    )) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("Network: ERROR - Failed to query fgtw.org: {}", e);
                        let result = QueryResult::Error(e.clone());
                        let _ = tx_response.send(result);
                        continue;
                    }
                };

                // Add peers to local store
                if !peers.is_empty() {
                    let mut store = peer_store.lock().unwrap();
                    for peer in &peers {
                        store.add_peer(peer.clone());
                    }
                    drop(store);
                }

                // Check if this is OUR device or someone else's
                let our_pubkey = device_keypair.public.as_bytes();
                let is_ours = peers.is_empty()
                    || peers
                        .iter()
                        .any(|p| p.device_pubkey.as_bytes() == our_pubkey);

                let result = if is_ours {
                    println!("Network: Handle '{}' registered to this device", username);
                    QueryResult::Success
                } else {
                    println!(
                        "Network: Handle '{}' is CLAIMED by another device",
                        username
                    );
                    QueryResult::AlreadyAttested(peers[0].clone())
                };

                if tx_response.send(result).is_err() {
                    eprintln!("Network: ERROR - Failed to send response (receiver dropped)");
                }
            }
        });

        Self {
            sender: tx_request,
            receiver: rx_response,
            transport,
            socket,
            port,
            online_receiver: rx_online,
        }
    }

    /// Check if FGTW connectivity status is available (non-blocking)
    pub fn try_recv_online(&self) -> Option<bool> {
        self.online_receiver.try_recv().ok()
    }

    /// Get the UDP port we're listening on
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Get a reference to the UDP socket
    pub fn socket(&self) -> &Arc<UdpSocket> {
        &self.socket
    }

    /// Set the FGTW transport (must be called after creating transport)
    pub fn set_transport(&self, t: Arc<FgtwTransport>) {
        let mut guard = self.transport.lock().unwrap();
        *guard = Some(t);
    }

    /// Get the transport (for peer lookup)
    pub fn get_transport(&self) -> Option<Arc<FgtwTransport>> {
        self.transport.lock().unwrap().clone()
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
    let handle_proof = handle.to_handle_proof();
    for peer in peers.iter().take(10) {
        // Announce to first 10 peers
        let message = FgtwMessage::Announce {
            handle_proof,
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

// NOTE: The old TCP-based query functions have been removed.
// We now use the HTTP-based load_bootstrap_peers() function from bootstrap.rs
// which properly implements the FGTW announce/query protocol over HTTPS.

// Helper function for sending FGTW messages (to avoid circular dependency)
async fn transport_send(peer_addr: &str, message: FgtwMessage) -> Result<FgtwMessage, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let mut stream = TcpStream::connect(peer_addr)
        .await
        .map_err(|e| format!("Failed to connect to {}: {}", peer_addr, e))?;

    // Send raw VSF message (self-describing)
    let msg_bytes = message.to_vsf_bytes();
    stream
        .write_all(&msg_bytes)
        .await
        .map_err(|e| format!("Write error: {}", e))?;
    stream
        .flush()
        .await
        .map_err(|e| format!("Flush error: {}", e))?;

    // Shutdown write side to signal EOF to server
    stream
        .shutdown()
        .await
        .map_err(|e| format!("Shutdown error: {}", e))?;

    // Read raw VSF response until EOF
    let mut msg_buf = Vec::new();
    stream
        .read_to_end(&mut msg_buf)
        .await
        .map_err(|e| format!("Read error: {}", e))?;

    FgtwMessage::from_vsf_bytes(&msg_buf)
}
