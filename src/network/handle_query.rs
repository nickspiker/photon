// Handle query protocol for checking handle attestation status
//
// Network layer for querying the FGTW (Fractal Gradient Trust Web) to check if a handle
// has been attested (claimed) or is available.
//
// This is a unified implementation that works on all platforms (Linux, Windows, Android, Redox).

use crate::network::fgtw::Keypair;
use crate::network::fgtw::{bootstrap::load_bootstrap_peers, PeerRecord, PeerStore};
use crate::types::{Handle, HandleText};
use crate::ui::app::{FoundPeer, SearchResult};
use std::net::UdpSocket;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

// Desktop-only imports
#[cfg(not(target_os = "android"))]
use crate::ui::PhotonEvent;
#[cfg(not(target_os = "android"))]
use winit::event_loop::EventLoopProxy;

/// Data loaded during attestation (all blocking work done in background)
#[derive(Debug, Clone)]
pub struct AttestationData {
    pub handle: String,
    pub handle_proof: [u8; 32],
    pub identity_seed: [u8; 32],
    pub contacts: Vec<crate::types::Contact>,
    pub friendships: Vec<(crate::types::friendship::FriendshipId, crate::types::friendship::FriendshipChains)>,
    pub avatar_pixels: Option<Vec<u8>>,  // Local avatar if exists
    pub peers: Vec<PeerRecord>,
}

/// Result of a handle query
#[derive(Debug, Clone)]
pub enum QueryResult {
    /// Successfully attested/registered, with all data pre-loaded in background
    /// This includes contacts, friendships, avatar - everything needed to flip to Ready state
    Success(Box<AttestationData>),
    AlreadyAttested(PeerRecord), // Handle is claimed by another device
    Error(String),               // Error during attestation
}

/// Result of a re-announce (refresh) operation
#[derive(Debug, Clone)]
pub struct RefreshResult {
    pub peers: Vec<PeerRecord>, // Updated peer list from FGTW
    pub error: Option<String>,  // Any error that occurred
}

/// Unified handle query system for all platforms
///
/// Provides:
/// - Handle attestation (query/attest)
/// - Connectivity monitoring
/// - Handle search
/// - Periodic refresh (re-announcement to FGTW)
pub struct HandleQuery {
    // Attestation channels
    query_sender: Sender<String>,
    query_receiver: Receiver<QueryResult>,

    // Connectivity channel
    online_receiver: Receiver<bool>,

    // Search channels
    search_sender: Sender<String>,
    search_receiver: Receiver<SearchResult>,

    // Refresh channels (re-announce to FGTW)
    refresh_sender: Sender<([u8; 32], String)>,
    refresh_receiver: Receiver<RefreshResult>,

    // Shared state
    transport: Arc<Mutex<Option<Arc<Mutex<PeerStore>>>>>,
    last_handle_proof: Arc<Mutex<Option<[u8; 32]>>>,
    last_handle: Arc<Mutex<Option<String>>>,

    // UDP socket for P2P and StatusChecker (bound to PHOTON_PORT 4383)
    socket: Arc<Mutex<Arc<UdpSocket>>>,
    port: Arc<Mutex<u16>>,
}

/// Bind UDP socket - tries ports in order: 4383 → 3546 → ephemeral
/// Returns (socket, port) - must have both UDP and TCP free on chosen port
fn bind_photon_socket() -> (UdpSocket, u16) {
    let ports_to_try = [crate::PHOTON_PORT, crate::PHOTON_PORT_FALLBACK];

    for port in ports_to_try {
        // Try to bind UDP first
        match UdpSocket::bind(format!("[::]:{}", port)) {
            Ok(udp) => {
                // Enable broadcast receive (needed for LAN discovery)
                if let Err(e) = udp.set_broadcast(true) {
                    crate::log(&format!("Network: Failed to enable broadcast: {}", e));
                }
                // Check TCP is also free
                match std::net::TcpListener::bind(format!("[::]:{}", port)) {
                    Ok(_tcp) => {
                        // Both free! TCP listener dropped, status.rs will create its own
                        crate::log(&format!("Network: Bound to port {} (UDP+TCP)", port));
                        return (udp, port);
                    }
                    Err(e) => {
                        crate::log(&format!("Network: Port {} TCP busy: {}", port, e));
                    }
                }
            }
            Err(e) => {
                crate::log(&format!("Network: Port {} UDP busy: {}", port, e));
            }
        }
    }

    // Fall back to ephemeral if all fixed ports failed
    crate::log("Network: All fixed ports busy - falling back to ephemeral");
    let udp = UdpSocket::bind("[::]:0").expect("Failed to bind UDP socket");
    // Enable broadcast receive for LAN discovery
    let _ = udp.set_broadcast(true);
    let port = udp
        .local_addr()
        .expect("Failed to get socket address")
        .port();
    (udp, port)
}

impl HandleQuery {
    /// Create a new handle query system
    ///
    /// # Arguments
    /// * `device_keypair` - The device's Ed25519 keypair for FGTW authentication
    /// * `event_proxy` - Desktop only: EventLoopProxy for waking the UI on connectivity changes
    #[cfg(not(target_os = "android"))]
    pub fn new(device_keypair: Keypair, event_proxy: EventLoopProxy<PhotonEvent>) -> Self {
        Self::new_internal(device_keypair, Some(event_proxy))
    }

    /// Create a new handle query system (Android version - no EventLoopProxy)
    #[cfg(target_os = "android")]
    pub fn new(device_keypair: Keypair) -> Self {
        Self::new_internal(device_keypair)
    }

    #[cfg(not(target_os = "android"))]
    fn new_internal(
        device_keypair: Keypair,
        event_proxy: Option<EventLoopProxy<PhotonEvent>>,
    ) -> Self {
        // Create all channels
        let (query_tx, query_rx_worker) = channel::<String>();
        let (query_tx_result, query_rx) = channel::<QueryResult>();
        let (online_tx, online_rx) = channel::<bool>();
        let (search_tx, search_rx_worker) = channel::<String>();
        let (search_tx_result, search_rx) = channel::<SearchResult>();
        let (refresh_tx, refresh_rx_worker) = channel::<([u8; 32], String)>();
        let (refresh_tx_result, refresh_rx) = channel::<RefreshResult>();

        // Shared state
        let transport = Arc::new(Mutex::new(None::<Arc<Mutex<PeerStore>>>));
        let last_handle_proof = Arc::new(Mutex::new(None::<[u8; 32]>));
        let last_handle = Arc::new(Mutex::new(None::<String>));

        // Bind UDP socket - tries 4383 → 3546 → ephemeral
        let (initial_socket, initial_port) = bind_photon_socket();
        crate::log(&format!(
            "Network: Using port {} for all traffic",
            initial_port
        ));
        let socket = Arc::new(Mutex::new(Arc::new(initial_socket)));
        let port = Arc::new(Mutex::new(initial_port));

        // Clone for workers
        let transport_query = transport.clone();
        let transport_search = transport.clone();
        let transport_refresh = transport.clone();
        let handle_proof_store = last_handle_proof.clone();
        let keypair_query = device_keypair.clone();
        let keypair_search = device_keypair.clone();
        let keypair_refresh = device_keypair.clone();
        let socket_query = socket.clone();
        let port_query = port.clone();
        let port_refresh = port.clone();

        // Spawn connectivity monitoring thread
        Self::spawn_connectivity_worker(online_tx, event_proxy);

        // Spawn attestation worker
        Self::spawn_query_worker(
            query_rx_worker,
            query_tx_result,
            transport_query,
            handle_proof_store,
            keypair_query,
            socket_query,
            port_query,
        );

        // Spawn search worker
        Self::spawn_search_worker(
            search_rx_worker,
            search_tx_result,
            transport_search,
            keypair_search,
        );

        // Spawn refresh worker
        Self::spawn_refresh_worker(
            refresh_rx_worker,
            refresh_tx_result,
            transport_refresh,
            keypair_refresh,
            port_refresh,
        );

        Self {
            query_sender: query_tx,
            query_receiver: query_rx,
            online_receiver: online_rx,
            search_sender: search_tx,
            search_receiver: search_rx,
            refresh_sender: refresh_tx,
            refresh_receiver: refresh_rx,
            transport,
            last_handle_proof,
            last_handle,
            socket,
            port,
        }
    }

    #[cfg(target_os = "android")]
    fn new_internal(device_keypair: Keypair) -> Self {
        // Create all channels
        let (query_tx, query_rx_worker) = channel::<String>();
        let (query_tx_result, query_rx) = channel::<QueryResult>();
        let (online_tx, online_rx) = channel::<bool>();
        let (search_tx, search_rx_worker) = channel::<String>();
        let (search_tx_result, search_rx) = channel::<SearchResult>();
        let (refresh_tx, refresh_rx_worker) = channel::<([u8; 32], String)>();
        let (refresh_tx_result, refresh_rx) = channel::<RefreshResult>();

        // Shared state
        let transport = Arc::new(Mutex::new(None::<Arc<Mutex<PeerStore>>>));
        let last_handle_proof = Arc::new(Mutex::new(None::<[u8; 32]>));
        let last_handle = Arc::new(Mutex::new(None::<String>));

        // Bind UDP socket - tries 4383 → 3546 → ephemeral
        let (initial_socket, initial_port) = bind_photon_socket();
        crate::log(&format!(
            "Network: Using port {} for all traffic",
            initial_port
        ));
        let socket = Arc::new(Mutex::new(Arc::new(initial_socket)));
        let port = Arc::new(Mutex::new(initial_port));

        // Clone for workers
        let transport_query = transport.clone();
        let transport_search = transport.clone();
        let transport_refresh = transport.clone();
        let handle_proof_store = last_handle_proof.clone();
        let keypair_query = device_keypair.clone();
        let keypair_search = device_keypair.clone();
        let keypair_refresh = device_keypair.clone();
        let socket_query = socket.clone();
        let port_query = port.clone();
        let port_refresh = port.clone();

        // Spawn connectivity monitoring thread (simplified for Android)
        Self::spawn_connectivity_worker_android(online_tx);

        // Spawn attestation worker
        Self::spawn_query_worker(
            query_rx_worker,
            query_tx_result,
            transport_query,
            handle_proof_store,
            keypair_query,
            socket_query,
            port_query,
        );

        // Spawn search worker
        Self::spawn_search_worker(
            search_rx_worker,
            search_tx_result,
            transport_search,
            keypair_search,
        );

        // Spawn refresh worker
        Self::spawn_refresh_worker(
            refresh_rx_worker,
            refresh_tx_result,
            transport_refresh,
            keypair_refresh,
            port_refresh,
        );

        Self {
            query_sender: query_tx,
            query_receiver: query_rx,
            online_receiver: online_rx,
            search_sender: search_tx,
            search_receiver: search_rx,
            refresh_sender: refresh_tx,
            refresh_receiver: refresh_rx,
            transport,
            last_handle_proof,
            last_handle,
            socket,
            port,
        }
    }

    /// Spawn connectivity monitoring thread (desktop - with if-watch)
    #[cfg(not(target_os = "android"))]
    fn spawn_connectivity_worker(
        online_tx: Sender<bool>,
        event_proxy: Option<EventLoopProxy<PhotonEvent>>,
    ) {
        thread::spawn(move || {
            use std::sync::mpsc::channel as std_channel;

            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .ok();

            // Channel for network change notifications
            let (net_change_tx, net_change_rx) = std_channel::<()>();

            // Spawn async network watcher (not available on Redox)
            #[cfg(not(target_os = "redox"))]
            {
                let net_change_tx_clone = net_change_tx.clone();
                thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build();

                    if let Ok(rt) = rt {
                        rt.block_on(async {
                            use futures::StreamExt;
                            use if_watch::tokio::IfWatcher;

                            if let Ok(mut watcher) = IfWatcher::new() {
                                loop {
                                    if watcher.next().await.is_some() {
                                        let _ = net_change_tx_clone.send(());
                                    }
                                }
                            }
                        });
                    }
                });
            }

            let mut prev_online = false;
            let mut first_check = true;

            let check_connectivity = |client: &Option<reqwest::blocking::Client>| -> bool {
                client
                    .as_ref()
                    .and_then(|c| c.get("https://fgtw.org/status").send().ok())
                    .map(|r| r.status().is_success())
                    .unwrap_or(false)
            };

            loop {
                let online = check_connectivity(&client);

                if first_check || online != prev_online {
                    let _ = online_tx.send(online);
                    if let Some(ref proxy) = event_proxy {
                        let _ = proxy.send_event(PhotonEvent::ConnectivityChanged(online));
                    }
                    prev_online = online;
                    first_check = false;
                }

                // Wait for network change or 30 second timeout
                match net_change_rx.recv_timeout(Duration::from_secs(30)) {
                    Ok(()) => thread::sleep(Duration::from_millis(500)), // Stabilization delay
                    Err(_) => {}                                         // Timeout - periodic check
                }
            }
        });
    }

    /// Spawn connectivity monitoring thread (Android - simple polling)
    #[cfg(target_os = "android")]
    fn spawn_connectivity_worker_android(online_tx: Sender<bool>) {
        thread::spawn(move || {
            let client = match reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
            {
                Ok(c) => Some(c),
                Err(e) => {
                    crate::log(&format!("Network: Failed to create HTTP client: {}", e));
                    None
                }
            };

            let mut prev_online = false;
            let mut first_check = true;

            loop {
                let online = match &client {
                    Some(c) => match c.get("https://fgtw.org/status").send() {
                        Ok(r) => {
                            let success = r.status().is_success();
                            if first_check {
                                crate::log(&format!(
                                    "Network: FGTW status check: {} ({})",
                                    r.status(),
                                    if success { "online" } else { "offline" }
                                ));
                            }
                            success
                        }
                        Err(e) => {
                            if first_check || prev_online {
                                crate::log(&format!(
                                    "Network: FGTW status check failed: {}",
                                    e
                                ));
                            }
                            false
                        }
                    },
                    None => false,
                };

                if first_check || online != prev_online {
                    let _ = online_tx.send(online);
                    prev_online = online;
                    first_check = false;
                }

                thread::sleep(Duration::from_secs(30));
            }
        });
    }

    /// Spawn attestation query worker
    fn spawn_query_worker(
        rx: Receiver<String>,
        tx: Sender<QueryResult>,
        transport: Arc<Mutex<Option<Arc<Mutex<PeerStore>>>>>,
        handle_proof_store: Arc<Mutex<Option<[u8; 32]>>>,
        keypair: Keypair,
        socket: Arc<Mutex<Arc<UdpSocket>>>,
        port: Arc<Mutex<u16>>,
    ) {
        thread::spawn(move || {
            crate::log("Network: Query worker initialized");

            while let Ok(handle) = rx.recv() {
                crate::log(&format!("Network: Querying handle '{}'...", handle));

                // Get current port for FGTW query (always PHOTON_PORT now)
                let current_port = *port.lock().unwrap();

                // Compute handle_proof (expensive - ~1 second)
                let handle_proof = Handle::username_to_handle_proof(&handle);

                // Wait for transport
                let transport_arc = loop {
                    let guard = transport.lock().unwrap();
                    if let Some(t) = &*guard {
                        break t.clone();
                    }
                    drop(guard);
                    thread::sleep(Duration::from_millis(100));
                };

                // Query FGTW
                let result = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create tokio runtime")
                    .block_on(load_bootstrap_peers(
                        &keypair,
                        handle_proof,
                        current_port,
                        &handle,
                    ));

                // Add peers to store (skip our own device)
                let our_pubkey = keypair.public.as_bytes();
                let other_peers: Vec<_> = result
                    .peers
                    .iter()
                    .filter(|p| &p.device_pubkey.key != our_pubkey)
                    .cloned()
                    .collect();
                if !other_peers.is_empty() {
                    let peer_store = transport_arc;
                    let mut store = peer_store.lock().unwrap();
                    for peer in &other_peers {
                        store.add_peer(peer.clone());
                    }
                    crate::log(&format!(
                        "Network: Added {} peer(s) to store",
                        other_peers.len()
                    ));
                }

                // Check result
                let query_result = if let Some(error) = result.error {
                    crate::log(&format!("Network: ERROR - {}", error));
                    QueryResult::Error(error)
                } else {
                    // Check if this is our device or someone else's
                    let our_pubkey = keypair.public.as_bytes();
                    let is_ours = result.peers.is_empty()
                        || result
                            .peers
                            .iter()
                            .any(|p| p.device_pubkey.as_bytes() == our_pubkey);

                    if is_ours {
                        *handle_proof_store.lock().unwrap() = Some(handle_proof);
                        crate::log(&format!(
                            "Network: Handle '{}' registered to this device",
                            handle
                        ));

                        // === Load all data in background (proof → network → disk → cloud) ===
                        let device_secret = keypair.secret.as_bytes();
                        let identity_seed = crate::storage::contacts::derive_identity_seed(&handle);

                        // Load contacts from disk
                        crate::log("Network: Loading contacts from disk...");
                        let mut contacts = crate::storage::contacts::load_all_contacts(&identity_seed, device_secret);

                        // Load messages for each contact
                        for contact in &mut contacts {
                            if let Err(e) = crate::storage::contacts::load_messages(
                                contact,
                                &identity_seed,
                                device_secret,
                            ) {
                                crate::log(&format!(
                                    "Network: Failed to load messages for {}: {}",
                                    contact.handle.as_str(),
                                    e
                                ));
                            }

                            // Load CLUTCH state if ceremony incomplete
                            if contact.clutch_state != crate::types::ClutchState::Complete {
                                if let Ok(Some(state)) = crate::storage::contacts::load_clutch_slots(
                                    contact.handle.as_str(),
                                    &identity_seed,
                                    device_secret,
                                ) {
                                    contact.clutch_slots = state.slots;
                                    contact.offer_provenances = state.offer_provenances;
                                    contact.ceremony_id = state.ceremony_id;
                                }
                                if let Ok(Some(keypairs)) = crate::storage::contacts::load_clutch_keypairs(
                                    contact.handle.as_str(),
                                    &identity_seed,
                                    device_secret,
                                ) {
                                    contact.clutch_our_keypairs = Some(keypairs);
                                    if contact.clutch_slots.is_empty() {
                                        contact.init_clutch_slots(identity_seed);
                                    }
                                    // Store local offer in local slot if not already present
                                    if let Some(ref kp) = contact.clutch_our_keypairs {
                                        use crate::crypto::clutch::ClutchOfferPayload;
                                        let our_offer = ClutchOfferPayload::from_keypairs(kp);
                                        if let Some(local_slot) = contact.get_slot_mut(&identity_seed) {
                                            if local_slot.offer.is_none() {
                                                local_slot.offer = Some(our_offer);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        crate::log(&format!("Network: Loaded {} contacts", contacts.len()));

                        // Load friendship chains from disk
                        crate::log("Network: Loading friendship chains...");
                        let friendships = crate::storage::friendship::load_all_friendships(&identity_seed, device_secret);
                        crate::log(&format!("Network: Loaded {} friendships", friendships.len()));

                        // Load local avatar
                        let avatar_pixels = crate::avatar::load_avatar(&handle).map(|(_, p)| p);

                        // Cloud sync (download + merge)
                        crate::log("Network: Syncing with cloud...");
                        match crate::storage::cloud::load_contacts_from_cloud(
                            &identity_seed,
                            &keypair,
                        ) {
                            Ok(Some(cloud_contacts)) => {
                                crate::log(&format!("Network: Cloud returned {} contact(s)", cloud_contacts.len()));
                                // Merge cloud contacts we don't have locally
                                for cc in cloud_contacts {
                                    let exists = contacts.iter().any(|c| c.handle_proof == cc.handle_proof);
                                    if !exists {
                                    let mut contact = cc.to_contact();
                                    // Load CLUTCH state for cloud contact too
                                    if contact.clutch_state != crate::types::ClutchState::Complete {
                                        if let Ok(Some(state)) = crate::storage::contacts::load_clutch_slots(
                                            contact.handle.as_str(),
                                            &identity_seed,
                                            device_secret,
                                        ) {
                                            contact.clutch_slots = state.slots;
                                            contact.offer_provenances = state.offer_provenances;
                                            contact.ceremony_id = state.ceremony_id;
                                        }
                                    }
                                    // Save to local storage
                                    let _ = crate::storage::contacts::save_contact(&contact, &identity_seed, device_secret);
                                    contacts.push(contact);
                                    }
                                }
                            }
                            Ok(None) => {
                                crate::log("Network: No contacts found in cloud (blob doesn't exist)");
                            }
                            Err(e) => {
                                crate::log(&format!("Network: Failed to load contacts from cloud: {:?}", e));
                            }
                        }

                        // Upload to cloud if we have more contacts locally
                        if !contacts.is_empty() {
                            let _ = crate::storage::cloud::sync_contacts_to_cloud(
                                &contacts,
                                &identity_seed,
                                &keypair,
                                &handle_proof,
                            );
                        }
                        crate::log("Network: Background loading complete");

                        QueryResult::Success(Box::new(AttestationData {
                            handle: handle.clone(),
                            handle_proof,
                            identity_seed,
                            contacts,
                            friendships,
                            avatar_pixels,
                            peers: result.peers,
                        }))
                    } else {
                        crate::log(&format!(
                            "Network: Handle '{}' is CLAIMED by another device",
                            handle
                        ));
                        QueryResult::AlreadyAttested(result.peers[0].clone())
                    }
                };

                let _ = tx.send(query_result);
            }
        });
    }

    /// Spawn search worker
    fn spawn_search_worker(
        rx: Receiver<String>,
        tx: Sender<SearchResult>,
        transport: Arc<Mutex<Option<Arc<Mutex<PeerStore>>>>>,
        keypair: Keypair,
    ) {
        thread::spawn(move || {
            crate::log("Network: Search worker initialized");

            while let Ok(handle) = rx.recv() {
                crate::log(&format!("Network: Searching for handle '{}'...", handle));

                // Compute handle_proof
                let handle_proof = Handle::username_to_handle_proof(&handle);

                // Wait for transport
                let transport_arc = loop {
                    let guard = transport.lock().unwrap();
                    if let Some(t) = &*guard {
                        break t.clone();
                    }
                    drop(guard);
                    thread::sleep(Duration::from_millis(100));
                };

                // Check local peer store first
                let peer_store = transport_arc;
                let store = peer_store.lock().unwrap();
                let peers = store.get_devices_for_handle(&handle_proof);

                let result = if let Some(peer) = peers.first() {
                    SearchResult::Found(FoundPeer {
                        handle: HandleText::new(&handle),
                        handle_proof,
                        device_pubkey: peer.device_pubkey.clone(),
                        ip: peer.ip,
                    })
                } else {
                    drop(store);

                    // Query FGTW
                    let bootstrap_result = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("Failed to create tokio runtime")
                        .block_on(load_bootstrap_peers(
                            &keypair,
                            handle_proof,
                            crate::PHOTON_PORT,
                            &handle,
                        ));

                    // Add found peers to store
                    if !bootstrap_result.peers.is_empty() {
                        let mut store = peer_store.lock().unwrap();
                        for peer in &bootstrap_result.peers {
                            store.add_peer(peer.clone());
                        }
                    }

                    if let Some(error) = bootstrap_result.error {
                        SearchResult::Error(error)
                    } else if let Some(peer) = bootstrap_result.peers.first() {
                        SearchResult::Found(FoundPeer {
                            handle: HandleText::new(&handle),
                            handle_proof,
                            device_pubkey: peer.device_pubkey.clone(),
                            ip: peer.ip,
                        })
                    } else {
                        SearchResult::NotFound
                    }
                };

                let _ = tx.send(result);
            }
        });
    }

    /// Spawn refresh (re-announcement) worker
    fn spawn_refresh_worker(
        rx: Receiver<([u8; 32], String)>,
        tx: Sender<RefreshResult>,
        transport: Arc<Mutex<Option<Arc<Mutex<PeerStore>>>>>,
        keypair: Keypair,
        port: Arc<Mutex<u16>>,
    ) {
        thread::spawn(move || {
            crate::log("Network: Refresh worker initialized");

            while let Ok((handle_proof, handle)) = rx.recv() {
                crate::log("Network: Refreshing FGTW announcement...");

                // Get current port
                let current_port = *port.lock().unwrap();

                // Wait for transport
                let transport_arc = loop {
                    let guard = transport.lock().unwrap();
                    if let Some(t) = &*guard {
                        break t.clone();
                    }
                    drop(guard);
                    thread::sleep(Duration::from_millis(100));
                };

                // Re-announce to FGTW
                let bootstrap_result = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create tokio runtime")
                    .block_on(load_bootstrap_peers(
                        &keypair,
                        handle_proof,
                        current_port,
                        &handle,
                    ));

                // Update local peer store
                if !bootstrap_result.peers.is_empty() {
                    let peer_store = transport_arc;
                    let mut store = peer_store.lock().unwrap();
                    for peer in &bootstrap_result.peers {
                        store.add_peer(peer.clone());
                    }
                    crate::log(&format!(
                        "Network: Refresh updated {} peer(s)",
                        bootstrap_result.peers.len()
                    ));
                }

                let _ = tx.send(RefreshResult {
                    peers: bootstrap_result.peers,
                    error: bootstrap_result.error,
                });
            }
        });
    }

    // ===== Public API =====

    /// Query/attest a handle (non-blocking)
    pub fn query(&self, handle: String) {
        let _ = self.query_sender.send(handle);
    }

    /// Check if an attestation response is ready (non-blocking)
    pub fn try_recv(&self) -> Option<QueryResult> {
        self.query_receiver.try_recv().ok()
    }

    /// Check if FGTW connectivity status is available (non-blocking)
    pub fn try_recv_online(&self) -> Option<bool> {
        self.online_receiver.try_recv().ok()
    }

    /// Start a handle search (non-blocking)
    pub fn search(&self, handle: String) {
        let _ = self.search_sender.send(handle);
    }

    /// Check if a search result is ready (non-blocking)
    pub fn try_recv_search(&self) -> Option<SearchResult> {
        self.search_receiver.try_recv().ok()
    }

    /// Trigger a refresh (re-announce to FGTW) using stored handle_proof and handle
    /// Returns true if refresh was triggered, false if no handle_proof/handle stored
    pub fn refresh(&self) -> bool {
        let proof_guard = self.last_handle_proof.lock().unwrap();
        let handle_guard = self.last_handle.lock().unwrap();
        if let (Some(handle_proof), Some(handle)) = (*proof_guard, handle_guard.clone()) {
            drop(proof_guard);
            drop(handle_guard);
            let _ = self.refresh_sender.send((handle_proof, handle));
            true
        } else {
            false
        }
    }

    /// Check if a refresh result is ready (non-blocking)
    pub fn try_recv_refresh(&self) -> Option<RefreshResult> {
        self.refresh_receiver.try_recv().ok()
    }

    /// Store handle_proof and handle string after successful attestation (for periodic refresh)
    pub fn set_handle_proof(&self, handle_proof: [u8; 32], handle: &str) {
        let mut proof_guard = self.last_handle_proof.lock().unwrap();
        *proof_guard = Some(handle_proof);
        drop(proof_guard);

        let mut handle_guard = self.last_handle.lock().unwrap();
        *handle_guard = Some(handle.to_string());
        crate::log("Network: Handle proof stored for periodic refresh");
    }

    /// Get stored handle_proof (for computing handle searches)
    pub fn get_handle_proof(&self) -> Option<[u8; 32]> {
        *self.last_handle_proof.lock().unwrap()
    }

    /// Set the FGTW transport (must be called after creating transport)
    pub fn set_transport(&self, t: Arc<Mutex<PeerStore>>) {
        let mut guard = self.transport.lock().unwrap();
        *guard = Some(t);
    }

    /// Get the transport (for peer lookup)
    pub fn get_transport(&self) -> Option<Arc<Mutex<PeerStore>>> {
        self.transport.lock().unwrap().clone()
    }

    /// Get the UDP port we're listening on
    pub fn port(&self) -> u16 {
        *self.port.lock().unwrap()
    }

    /// Get a clone of the UDP socket (for StatusChecker)
    pub fn socket(&self) -> Arc<UdpSocket> {
        self.socket.lock().unwrap().clone()
    }
}
