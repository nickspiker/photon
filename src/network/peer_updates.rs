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

#[cfg(not(target_os = "android"))]
use crate::ui::PhotonEvent;
#[cfg(not(target_os = "android"))]
use winit::event_loop::EventLoopProxy;

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
        use vsf::parse;
        use vsf::types::VsfType;

        // Parse VSF header
        let (_header, header_end) = VsfHeader::decode(data).ok()?;
        let mut ptr = header_end;

        // Skip to section start '['
        if ptr >= data.len() || data[ptr] != b'[' {
            return None;
        }
        ptr += 1;

        // Parse section name
        let section_name = match parse(data, &mut ptr).ok()? {
            VsfType::d(name) => name,
            _ => return None,
        };

        if section_name != "peer_update" {
            return None;
        }

        // Parse fields until section end ']'
        let mut handle_proof = None;
        let mut device_pubkey = None;
        let mut ip = None;
        let mut port = None;
        let mut timestamp = None;

        while ptr < data.len() && data[ptr] != b']' {
            // Expect field start '('
            if data[ptr] != b'(' {
                ptr += 1;
                continue;
            }
            ptr += 1;

            // Parse field name
            let field_name = match parse(data, &mut ptr).ok()? {
                VsfType::d(name) => name,
                _ => continue,
            };

            // Skip ':' separator
            if ptr < data.len() && data[ptr] == b':' {
                ptr += 1;
            }

            // Parse field value
            let value = parse(data, &mut ptr).ok()?;

            // Skip field end ')'
            if ptr < data.len() && data[ptr] == b')' {
                ptr += 1;
            }

            match field_name.as_str() {
                "handle_proof" => {
                    if let VsfType::hP(bytes) = &value {
                        if bytes.len() == 32 {
                            let mut arr = [0u8; 32];
                            arr.copy_from_slice(bytes);
                            handle_proof = Some(arr);
                        }
                    }
                }
                "device_pubkey" => {
                    if let VsfType::ke(bytes) = value {
                        if bytes.len() == 32 {
                            let mut arr = [0u8; 32];
                            arr.copy_from_slice(&bytes);
                            device_pubkey = Some(arr);
                        }
                    }
                }
                "ip" => {
                    if let VsfType::x(s) = value {
                        ip = Some(s);
                    }
                }
                "port" => {
                    let p = match value {
                        VsfType::u(v, _) => Some(v),
                        VsfType::u3(v) => Some(v as usize),
                        VsfType::u4(v) => Some(v as usize),
                        VsfType::u5(v) => Some(v as usize),
                        VsfType::u6(v) => Some(v as usize),
                        VsfType::m(v) => Some(v), // Legacy compat
                        _ => None,
                    };
                    if let Some(v) = p {
                        port = Some(v as u16);
                    }
                }
                "timestamp" => {
                    if let VsfType::e(vsf::types::EtType::f6(ts)) = value {
                        timestamp = Some(ts);
                    }
                }
                _ => {}
            }
        }

        Some(PeerUpdate {
            handle_proof: handle_proof?,
            device_pubkey: device_pubkey?,
            ip: ip?,
            port: port?,
            timestamp: timestamp?,
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
