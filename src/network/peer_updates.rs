//! WebSocket client for real-time peer IP updates from FGTW
//!
//! Connects to wss://fgtw.org/ws and receives peer_update messages
//! when any peer's IP changes. This eliminates the 25-second delay
//! caused by stale IP caches.
//!
//! Desktop-only module (not available on Android - uses FCM instead)

use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;
use vsf::schema::{SectionBuilder, SectionSchema, TypeConstraint};

#[cfg(not(target_os = "android"))]
use crate::ui::PhotonEvent;
#[cfg(not(target_os = "android"))]
use winit::event_loop::EventLoopProxy;

/// Schema for peer_update section from FGTW WebSocket
fn peer_update_schema() -> SectionSchema {
    SectionSchema::new("peer_update")
        .field("handle_proof", TypeConstraint::Any) // hP
        .field("device_pubkey", TypeConstraint::Ed25519Key) // ke
        .field("ip", TypeConstraint::Utf8Text) // x
        .field("port", TypeConstraint::AnyUnsigned) // u/u3/u4/u5/u6
        .field("timestamp", TypeConstraint::AnyEagleTime) // ef6
}

/// Parsed peer update from FGTW WebSocket
#[derive(Debug, Clone)]
pub struct PeerUpdate {
    pub handle_proof: [u8; 32],
    pub device_pubkey: [u8; 32],
    pub ip: String,
    pub port: u16,
    pub timestamp: f64,
}

/// WebSocket client for receiving peer updates
pub struct PeerUpdateClient {
    /// Channel to receive parsed peer updates
    update_receiver: Receiver<PeerUpdate>,
    /// Channel to signal shutdown
    shutdown_sender: Option<Sender<()>>,
}

impl PeerUpdateClient {
    /// Create and start a new peer update WebSocket client
    ///
    /// Spawns a background thread that maintains a WebSocket connection
    /// to fgtw.org/ws and receives peer updates.
    #[cfg(not(target_os = "android"))]
    pub fn new(event_proxy: EventLoopProxy<PhotonEvent>) -> Self {
        let (update_tx, update_rx) = channel::<PeerUpdate>();
        let (shutdown_tx, shutdown_rx) = channel::<()>();

        // Spawn WebSocket client thread
        thread::spawn(move || {
            Self::websocket_loop(update_tx, shutdown_rx, event_proxy);
        });

        Self {
            update_receiver: update_rx,
            shutdown_sender: Some(shutdown_tx),
        }
    }

    /// Create client without event proxy (for simpler use cases)
    #[cfg(not(target_os = "android"))]
    pub fn new_simple() -> Self {
        let (update_tx, update_rx) = channel::<PeerUpdate>();
        let (shutdown_tx, shutdown_rx) = channel::<()>();

        // Spawn WebSocket client thread without event proxy
        thread::spawn(move || {
            Self::websocket_loop_simple(update_tx, shutdown_rx);
        });

        Self {
            update_receiver: update_rx,
            shutdown_sender: Some(shutdown_tx),
        }
    }

    /// Try to receive a peer update (non-blocking)
    pub fn try_recv(&self) -> Option<PeerUpdate> {
        match self.update_receiver.try_recv() {
            Ok(update) => Some(update),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => None,
        }
    }

    /// WebSocket event loop with event proxy
    #[cfg(not(target_os = "android"))]
    fn websocket_loop(
        update_tx: Sender<PeerUpdate>,
        shutdown_rx: Receiver<()>,
        event_proxy: EventLoopProxy<PhotonEvent>,
    ) {
        use futures::StreamExt;
        use tokio_tungstenite::tungstenite::Message;

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();

        let Ok(rt) = rt else {
            crate::log("PeerUpdate: Failed to create tokio runtime");
            return;
        };

        rt.block_on(async {
            loop {
                // Check for shutdown
                if shutdown_rx.try_recv().is_ok() {
                    crate::log("PeerUpdate: Shutdown signal received");
                    break;
                }

                // Connect to WebSocket
                crate::log("PeerUpdate: Connecting to wss://fgtw.org/ws");
                let ws_result = tokio_tungstenite::connect_async("wss://fgtw.org/ws").await;

                match ws_result {
                    Ok((ws_stream, _response)) => {
                        crate::log("PeerUpdate: Connected to FGTW WebSocket");

                        let (_, mut read) = ws_stream.split();

                        // Read messages until connection closes
                        while let Some(msg_result) = read.next().await {
                            // Check for shutdown
                            if shutdown_rx.try_recv().is_ok() {
                                crate::log("PeerUpdate: Shutdown during read");
                                return;
                            }

                            match msg_result {
                                Ok(Message::Binary(data)) => {
                                    // Parse VSF peer_update message
                                    if let Some(update) = Self::parse_peer_update(&data) {
                                        crate::log(&format!(
                                            "PeerUpdate: Received update for {}",
                                            &update.ip
                                        ));
                                        let _ = update_tx.send(update);
                                        // Wake up the event loop
                                        let _ = event_proxy.send_event(PhotonEvent::NetworkUpdate);
                                    }
                                }
                                Ok(Message::Ping(_)) => {
                                    // Tungstenite handles pong automatically
                                }
                                Ok(Message::Close(_)) => {
                                    crate::log("PeerUpdate: Server closed connection");
                                    break;
                                }
                                Ok(_) => {
                                    // Ignore text, pong, frame messages
                                }
                                Err(e) => {
                                    crate::log(&format!(
                                        "PeerUpdate: WebSocket error: {}",
                                        e
                                    ));
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        crate::log(&format!("PeerUpdate: Connection failed: {}", e));
                    }
                }

                // Wait before reconnecting
                crate::log("PeerUpdate: Reconnecting in 5 seconds...");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
    }

    /// Simpler WebSocket loop without event proxy
    #[cfg(not(target_os = "android"))]
    fn websocket_loop_simple(update_tx: Sender<PeerUpdate>, shutdown_rx: Receiver<()>) {
        use futures::StreamExt;
        use tokio_tungstenite::tungstenite::Message;

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();

        let Ok(rt) = rt else {
            return;
        };

        rt.block_on(async {
            loop {
                if shutdown_rx.try_recv().is_ok() {
                    break;
                }

                let ws_result = tokio_tungstenite::connect_async("wss://fgtw.org/ws").await;

                if let Ok((ws_stream, _)) = ws_result {
                    let (_, mut read) = ws_stream.split();

                    while let Some(msg_result) = read.next().await {
                        if shutdown_rx.try_recv().is_ok() {
                            return;
                        }

                        if let Ok(Message::Binary(data)) = msg_result {
                            if let Some(update) = Self::parse_peer_update(&data) {
                                let _ = update_tx.send(update);
                            }
                        }
                    }
                }

                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
    }

    /// Parse VSF peer_update message into PeerUpdate struct
    fn parse_peer_update(data: &[u8]) -> Option<PeerUpdate> {
        use vsf::file_format::VsfHeader;
        use vsf::types::VsfType;

        // Parse VSF header
        let (_header, header_end) = VsfHeader::decode(data).ok()?;
        let section_bytes = &data[header_end..];

        // Parse section with schema
        let schema = peer_update_schema();
        let builder = SectionBuilder::parse(schema, section_bytes).ok()?;

        // Extract fields
        let handle_proof_values = builder.get("handle_proof").ok()?;
        let handle_proof = match handle_proof_values.first()? {
            VsfType::hP(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(bytes);
                arr
            }
            _ => return None,
        };

        let device_pubkey_values = builder.get("device_pubkey").ok()?;
        let device_pubkey = match device_pubkey_values.first()? {
            VsfType::ke(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(bytes);
                arr
            }
            _ => return None,
        };

        let ip_values = builder.get("ip").ok()?;
        let ip = match ip_values.first()? {
            VsfType::x(s) => s.clone(),
            _ => return None,
        };

        let port_values = builder.get("port").ok()?;
        let port = match port_values.first()? {
            VsfType::u(v, _) => *v as u16,
            VsfType::u3(v) => *v as u16,
            VsfType::u4(v) => *v,
            VsfType::u5(v) => *v as u16,
            VsfType::u6(v) => *v as u16,
            VsfType::m(v) => *v as u16,
            _ => return None,
        };

        let timestamp_values = builder.get("timestamp").ok()?;
        let timestamp = match timestamp_values.first()? {
            VsfType::e(vsf::types::EtType::f6(ts)) => *ts,
            _ => return None,
        };

        Some(PeerUpdate {
            handle_proof,
            device_pubkey,
            ip,
            port,
            timestamp,
        })
    }
}

impl Drop for PeerUpdateClient {
    fn drop(&mut self) {
        // Signal shutdown to background thread
        if let Some(tx) = self.shutdown_sender.take() {
            let _ = tx.send(());
        }
    }
}
