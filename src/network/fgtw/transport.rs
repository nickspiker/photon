use super::{FgtwMessage, PeerRecord, PeerStore};
use crate::types::PublicIdentity;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// FGTW transport layer (TCP-based)
pub struct FgtwTransport {
    our_pubkey: PublicIdentity,
    peer_store: Arc<Mutex<PeerStore>>,
    port: u16,
}

impl FgtwTransport {
    pub fn new(our_pubkey: PublicIdentity, port: u16) -> Self {
        Self {
            our_pubkey,
            peer_store: Arc::new(Mutex::new(PeerStore::new())),
            port,
        }
    }

    /// Start listening for incoming connections
    pub async fn start(&self) -> Result<(), String> {
        let addr = format!("0.0.0.0:{}", self.port);
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| format!("Failed to bind to {}: {}", addr, e))?;

        println!("FGTW: Listening on {}", addr);

        let peer_store = self.peer_store.clone();
        let our_pubkey = self.our_pubkey.clone();

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, addr)) => {
                        let peer_store = peer_store.clone();
                        let our_pubkey = our_pubkey.clone();
                        tokio::spawn(async move {
                            if let Err(e) =
                                handle_connection(socket, addr, peer_store, our_pubkey).await
                            {
                                eprintln!("FGTW: Connection error from {}: {}", addr, e);
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("FGTW: Accept error: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    /// Send a message to a peer
    pub async fn send_message(
        &self,
        peer_addr: &str,
        message: FgtwMessage,
    ) -> Result<FgtwMessage, String> {
        let mut stream = TcpStream::connect(peer_addr)
            .await
            .map_err(|e| format!("Failed to connect to {}: {}", peer_addr, e))?;

        // Send message (length-prefixed)
        let msg_bytes = message.to_vsf_bytes();
        let len = msg_bytes.len() as u32;
        stream
            .write_all(&len.to_be_bytes())
            .await
            .map_err(|e| format!("Write len error: {}", e))?;
        stream
            .write_all(&msg_bytes)
            .await
            .map_err(|e| format!("Write msg error: {}", e))?;
        stream
            .flush()
            .await
            .map_err(|e| format!("Flush error: {}", e))?;

        // Read response
        let mut len_buf = [0u8; 4];
        stream
            .read_exact(&mut len_buf)
            .await
            .map_err(|e| format!("Read len error: {}", e))?;
        let len = u32::from_be_bytes(len_buf) as usize;

        if len > 10_000_000 {
            return Err(format!("Message too large: {} bytes", len));
        }

        let mut msg_buf = vec![0u8; len];
        stream
            .read_exact(&mut msg_buf)
            .await
            .map_err(|e| format!("Read msg error: {}", e))?;

        FgtwMessage::from_vsf_bytes(&msg_buf)
    }

    /// Get peer store (for querying)
    pub fn peer_store(&self) -> Arc<Mutex<PeerStore>> {
        self.peer_store.clone()
    }

    /// Add bootstrap peers to store
    pub fn add_bootstrap_peers(&self, peers: Vec<PeerRecord>) {
        let mut store = self.peer_store.lock().unwrap();
        for peer in peers {
            store.add_peer(peer);
        }
        println!("FGTW: Added {} bootstrap peers", store.peer_count());
    }
}

/// Handle incoming connection
async fn handle_connection(
    mut socket: TcpStream,
    addr: std::net::SocketAddr,
    peer_store: Arc<Mutex<PeerStore>>,
    our_pubkey: PublicIdentity,
) -> Result<(), String> {
    // Read message (length-prefixed)
    let mut len_buf = [0u8; 4];
    socket
        .read_exact(&mut len_buf)
        .await
        .map_err(|e| format!("Read len error: {}", e))?;
    let len = u32::from_be_bytes(len_buf) as usize;

    let mut msg_buf = vec![0u8; len];
    socket
        .read_exact(&mut msg_buf)
        .await
        .map_err(|e| format!("Read msg error: {}", e))?;

    let message = FgtwMessage::from_vsf_bytes(&msg_buf)?;

    // Process message and generate response
    let response = process_message(message, &peer_store, &our_pubkey, addr);

    // Send response
    let resp_bytes = response.to_vsf_bytes();
    let resp_len = resp_bytes.len() as u32;
    socket
        .write_all(&resp_len.to_be_bytes())
        .await
        .map_err(|e| format!("Write len error: {}", e))?;
    socket
        .write_all(&resp_bytes)
        .await
        .map_err(|e| format!("Write msg error: {}", e))?;
    socket
        .flush()
        .await
        .map_err(|e| format!("Flush error: {}", e))?;

    Ok(())
}

/// Process FGTW message and generate response
fn process_message(
    message: FgtwMessage,
    peer_store: &Arc<Mutex<PeerStore>>,
    our_pubkey: &PublicIdentity,
    _addr: std::net::SocketAddr,
) -> FgtwMessage {
    match message {
        FgtwMessage::Ping { device_pubkey: _ } => {
            let store = peer_store.lock().unwrap();
            let peers = store.get_all_peers();
            FgtwMessage::Pong {
                device_pubkey: our_pubkey.clone(),
                peers,
            }
        }
        FgtwMessage::FindNode {
            handle_hash,
            requester_pubkey: _,
        } => {
            let store = peer_store.lock().unwrap();
            let devices = store.get_devices_for_handle(&handle_hash);
            FgtwMessage::FoundNodes { devices }
        }
        FgtwMessage::Announce {
            handle_hash,
            device_pubkey,
            port,
        } => {
            let ip = format!("{}:{}", _addr.ip(), port).parse().unwrap();
            let peer = PeerRecord::new(handle_hash, device_pubkey, ip);
            let mut store = peer_store.lock().unwrap();
            store.add_peer(peer);
            println!("FGTW: Announced handle_hash {:?}", &handle_hash[..8]);
            FgtwMessage::Pong {
                device_pubkey: our_pubkey.clone(),
                peers: vec![],
            }
        }
        FgtwMessage::Query {
            handle_hash,
            requester_pubkey: _,
        } => {
            let store = peer_store.lock().unwrap();
            let devices = store.get_devices_for_handle(&handle_hash);
            FgtwMessage::QueryResponse { devices }
        }
        _ => {
            // For unexpected messages, return empty pong
            FgtwMessage::Pong {
                device_pubkey: our_pubkey.clone(),
                peers: vec![],
            }
        }
    }
}
