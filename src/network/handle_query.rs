// Handle query protocol for checking handle attestation status
//
// Network layer for querying the FGTW (Fractal Gradient Trust Web) to check if a handle has been attested (claimed) or is available.
//
// This is a unified implementation that works on all platforms (Linux, Windows, Android, Redox).

use crate::network::fgtw::Keypair;
use crate::network::fgtw::{bootstrap::load_bootstrap_peers, PeerRecord, PeerStore};
use crate::types::{Handle, HandleText};
use crate::ui::state::{FoundPeer, SearchResult};
use std::net::UdpSocket;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

// Desktop-only imports
#[cfg(not(target_os = "android"))]
use crate::ui::PhotonEvent;
use fluor::host::WakeSender;

/// Data loaded during attestation (all blocking work done in background)
#[derive(Debug, Clone)]
pub struct AttestationData {
    pub handle_proof: [u8; 32],
    pub identity_seed: [u8; 32],
    pub contacts: Vec<crate::types::Contact>,
    pub friendships: Vec<(
        crate::types::friendship::FriendshipId,
        crate::types::friendship::FriendshipChains,
    )>,
    pub avatar_pixels: Option<Vec<u8>>, // Local avatar if exists
    pub peers: Vec<PeerRecord>,
    /// True if FlatStorage detected a damaged ring during open this session (missing, permission-denied, corrupt, or HMAC-bad). UI renders a persistent degraded banner when true. Sticky for the session — only clears on next process restart after both rings open cleanly.
    pub vault_degraded: bool,
}

/// A request to the attestation worker.
/// First attest carries the typed handle (the one moment the string exists — the worker derives the roots, persists them, and drops it); resume carries the cached session roots, so it touches neither the string nor the ~1s proof recompute.
pub enum QueryRequest {
    FirstAttest(String),
    /// First-attest SEMANTICS (persist the roots on FGTW confirmation) with roots derived by the caller — the JOIN flow already paid the ~1s proof when it keyed the pairing slots, so re-deriving from the string here would be a second full proof delay on the fresh device.
    FirstAttestWithRoots(tohu::SessionIdentity),
    Resume(tohu::SessionIdentity),
    /// Classify a typed handle WITHOUT announcing: derive the roots (the ~1s proof, paid once), fetch + fold the fleet chain, and report which of the three attest branches applies. The UI gates the permanence warning on the [`ProbeOutcome::Fresh`] result, so "forever" is only ever shown for a genuinely unclaimed handle.
    Probe(String),
}

/// How a typed handle classifies against the network, for the attest three-way branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// No chain exists — a genuine fresh claim. Show the permanence interstitial.
    Fresh,
    /// A chain exists and THIS device folds as a current member — just resume (announce passes the gate).
    Member,
    /// A chain exists, its genesis is identity-bound to THIS handle (so it's ours), but this device isn't enrolled — route to add-this-device (JOIN).
    JoinOurs,
    /// A chain exists whose genesis is NOT bound to this handle's identity — someone else founded it (squatter / different person). Can't claim.
    Taken,
}

/// Result of a handle query
#[derive(Debug, Clone)]
pub enum QueryResult {
    /// Successfully attested/registered, with all data pre-loaded in background This includes contacts, friendships, avatar - everything needed to flip to Ready state
    Success(Box<AttestationData>),
    AlreadyAttested(PeerRecord), // Handle is claimed by another device
    /// Result of a [`QueryRequest::Probe`]: the branch decision plus the derived roots (so the follow-up attest/join reuses the proof instead of recomputing it).
    Probe { outcome: ProbeOutcome, session: tohu::SessionIdentity },
    Error(String),               // Error during attestation
}

/// Unified handle query system for all platforms
///
/// Provides:
/// - Handle attestation (query/attest)
/// - Connectivity monitoring
/// - Handle search
pub struct HandleQuery {
    // Attestation channels
    query_sender: Sender<QueryRequest>,
    query_receiver: Receiver<QueryResult>,

    // Connectivity channel
    online_receiver: Receiver<bool>,

    // Search channels
    search_sender: Sender<String>,
    search_receiver: Receiver<SearchResult>,

    // Shared state
    transport: Arc<Mutex<Option<Arc<Mutex<PeerStore>>>>>,
    last_handle_proof: Arc<Mutex<Option<[u8; 32]>>>,
    // Written into the attest/search worker threads via clones; the field itself is the shared holder, never read directly (the clones carry it). Kept as the owning slot.
    #[allow(dead_code)]
    last_identity_seed: Arc<Mutex<Option<[u8; 32]>>>,

    // UDP socket for P2P and StatusChecker (bound to PHOTON_PORT 4383)
    socket: Arc<Mutex<Arc<UdpSocket>>>,
    port: Arc<Mutex<u16>>,
}

/// Bind UDP socket - tries ports in order: 4383 → 3546 → ephemeral Returns (socket, port) - must have both UDP and TCP free on chosen port
fn bind_photon_socket() -> (UdpSocket, u16) {
    let ports_to_try = [crate::PHOTON_PORT, crate::PHOTON_PORT_FALLBACK];

    for port in ports_to_try {
        // Try to bind UDP first
        match UdpSocket::bind(format!("[::]:{}", port)) {
            Ok(udp) => {
                // Enable broadcast receive (needed for LAN discovery)
                if let Err(e) = udp.set_broadcast(true) {
                    crate::logf!("Network: Failed to enable broadcast: {}", e);
                }
                // Check TCP is also free
                match std::net::TcpListener::bind(format!("[::]:{}", port)) {
                    Ok(_tcp) => {
                        // Both free! TCP listener dropped, status.rs will create its own
                        crate::logf!("Network: Bound to port {} (UDP+TCP)", port);
                        return (udp, port);
                    }
                    Err(e) => {
                        crate::logf!("Network: Port {} TCP busy: {}", port, e);
                    }
                }
            }
            Err(e) => {
                crate::logf!("Network: Port {} UDP busy: {}", port, e);
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
    /// * `device_keypair` - The device's Ed25519 keypair for FGTW authentication * `event_proxy` - Desktop only: EventLoopProxy for waking the UI on connectivity changes
    #[cfg(not(target_os = "android"))]
    pub fn new(device_keypair: Keypair, event_proxy: Arc<dyn WakeSender<PhotonEvent>>) -> Self {
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
        event_proxy: Option<Arc<dyn WakeSender<PhotonEvent>>>,
    ) -> Self {
        // Create all channels
        let (query_tx, query_rx_worker) = channel::<QueryRequest>();
        let (query_tx_result, query_rx) = channel::<QueryResult>();
        let (online_tx, online_rx) = channel::<bool>();
        let (search_tx, search_rx_worker) = channel::<String>();
        let (search_tx_result, search_rx) = channel::<SearchResult>();
        // Shared state
        let transport = Arc::new(Mutex::new(None::<Arc<Mutex<PeerStore>>>));
        let last_handle_proof = Arc::new(Mutex::new(None::<[u8; 32]>));
        let last_identity_seed = Arc::new(Mutex::new(None::<[u8; 32]>));

        // Bind UDP socket - tries 4383 → 3546 → ephemeral
        let (initial_socket, initial_port) = bind_photon_socket();
        crate::logf!("Network: Using port {} for all traffic", initial_port);
        let socket = Arc::new(Mutex::new(Arc::new(initial_socket)));
        let port = Arc::new(Mutex::new(initial_port));

        // Clone for workers
        let transport_query = transport.clone();
        let transport_search = transport.clone();
        let handle_proof_store = last_handle_proof.clone();
        let handle_proof_search = last_handle_proof.clone();
        let identity_seed_store = last_identity_seed.clone();
        let identity_seed_search = last_identity_seed.clone();
        let keypair_query = device_keypair.clone();
        let keypair_search = device_keypair.clone();
        let socket_query = socket.clone();
        let port_query = port.clone();
        let port_search = port.clone();

        // Spawn connectivity monitoring thread
        Self::spawn_connectivity_worker(online_tx, event_proxy);

        // Spawn attestation worker
        Self::spawn_query_worker(
            query_rx_worker,
            query_tx_result,
            transport_query,
            handle_proof_store,
            identity_seed_store,
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
            identity_seed_search,
            handle_proof_search,
            port_search,
        );

        Self {
            query_sender: query_tx,
            query_receiver: query_rx,
            online_receiver: online_rx,
            search_sender: search_tx,
            search_receiver: search_rx,
            transport,
            last_handle_proof,
            last_identity_seed,
            socket,
            port,
        }
    }

    #[cfg(target_os = "android")]
    fn new_internal(device_keypair: Keypair) -> Self {
        // Create all channels
        let (query_tx, query_rx_worker) = channel::<QueryRequest>();
        let (query_tx_result, query_rx) = channel::<QueryResult>();
        let (online_tx, online_rx) = channel::<bool>();
        let (search_tx, search_rx_worker) = channel::<String>();
        let (search_tx_result, search_rx) = channel::<SearchResult>();
        // Shared state
        let transport = Arc::new(Mutex::new(None::<Arc<Mutex<PeerStore>>>));
        let last_handle_proof = Arc::new(Mutex::new(None::<[u8; 32]>));
        let last_identity_seed = Arc::new(Mutex::new(None::<[u8; 32]>));

        // Bind UDP socket - tries 4383 → 3546 → ephemeral
        let (initial_socket, initial_port) = bind_photon_socket();
        crate::logf!("Network: Using port {} for all traffic", initial_port);
        let socket = Arc::new(Mutex::new(Arc::new(initial_socket)));
        let port = Arc::new(Mutex::new(initial_port));

        // Clone for workers
        let transport_query = transport.clone();
        let transport_search = transport.clone();
        let handle_proof_store = last_handle_proof.clone();
        let handle_proof_search = last_handle_proof.clone();
        let identity_seed_store = last_identity_seed.clone();
        let identity_seed_search = last_identity_seed.clone();
        let keypair_query = device_keypair.clone();
        let keypair_search = device_keypair.clone();
        let socket_query = socket.clone();
        let port_query = port.clone();
        let port_search = port.clone();

        // Spawn connectivity monitoring thread (simplified for Android)
        Self::spawn_connectivity_worker_android(online_tx);

        // Spawn attestation worker
        Self::spawn_query_worker(
            query_rx_worker,
            query_tx_result,
            transport_query,
            handle_proof_store,
            identity_seed_store,
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
            identity_seed_search,
            handle_proof_search,
            port_search,
        );

        Self {
            query_sender: query_tx,
            query_receiver: query_rx,
            online_receiver: online_rx,
            search_sender: search_tx,
            search_receiver: search_rx,
            transport,
            last_handle_proof,
            last_identity_seed,
            socket,
            port,
        }
    }

    /// Spawn connectivity monitoring thread (desktop - with if-watch)
    #[cfg(not(target_os = "android"))]
    fn spawn_connectivity_worker(
        online_tx: Sender<bool>,
        event_proxy: Option<Arc<dyn WakeSender<PhotonEvent>>>,
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
                    crate::logf!("Connectivity: FGTW {} (GET /status)", if online { "ONLINE" } else { "offline" });
                    let _ = online_tx.send(online);
                    if let Some(ref proxy) = event_proxy {
                        let _ = proxy.send(PhotonEvent::ConnectivityChanged(online));
                    }
                    prev_online = online;
                    first_check = false;
                }

                // Wait for network change or 30 second timeout
                match net_change_rx.recv_timeout(crate::jitter_dur(Duration::from_secs(30))) {
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
                    crate::logf!("Network: Failed to create HTTP client: {}", e);
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
                                crate::logf!("Network: FGTW status check: {} ({})", r.status(), if success { "online" } else { "offline" });
                            }
                            success
                        }
                        Err(e) => {
                            if first_check || prev_online {
                                crate::logf!("Network: FGTW status check failed: {}", e);
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

                // Jittered (15–30s) so a fleet of devices doesn't poll FGTW /status in lockstep.
                thread::sleep(crate::jitter_dur(Duration::from_secs(30)));
            }
        });
    }

    /// Spawn attestation query worker
    fn spawn_query_worker(
        rx: Receiver<QueryRequest>,
        tx: Sender<QueryResult>,
        transport: Arc<Mutex<Option<Arc<Mutex<PeerStore>>>>>,
        handle_proof_store: Arc<Mutex<Option<[u8; 32]>>>,
        identity_seed_store: Arc<Mutex<Option<[u8; 32]>>>,
        keypair: Keypair,
        _socket: Arc<Mutex<Arc<UdpSocket>>>,
        port: Arc<Mutex<u16>>,
    ) {
        thread::spawn(move || {
            crate::log("Network: Query worker initialized");

            while let Ok(req) = rx.recv() {
                // Probe: classify the handle against the network and report the branch — no announce. Computes the roots (the ~1s proof) once; the UI hands them back on the chosen follow-up so the proof is never paid twice.
                if let QueryRequest::Probe(handle) = &req {
                    let identity_seed = crate::storage::contacts::derive_identity_seed(handle);
                    let handle_proof = Handle::username_to_handle_proof(handle); // ~1s
                    let session = tohu::SessionIdentity {
                        identity_seed,
                        vault_seed: identity_seed,
                        handle_proof,
                    };
                    let outcome = match crate::network::fgtw::fleet::fetch(&handle_proof) {
                        Ok(None) => ProbeOutcome::Fresh,
                        Ok(Some(blob)) => {
                            let me = keypair.public.to_bytes();
                            // Match fold() explicitly: a FoldError must NOT be swallowed into a false "Taken" with an unverified genesis compare. An unfoldable chain is indeterminate (corrupt / partial / KV read-lag) — surface an error and let the next cycle retry, never brand the handle taken.
                            match blob.fold() {
                                Ok(members) => {
                                    if members.contains(&me) {
                                        ProbeOutcome::Member
                                    } else if blob.genesis_identity_matches(&identity_seed) {
                                        ProbeOutcome::JoinOurs
                                    } else {
                                        ProbeOutcome::Taken
                                    }
                                }
                                // An EMPTY chain is not corruption — it's "no one holds this handle" (a blob with zero ops folds to Empty, e.g. after a wipe left the slot allocated but unwritten). Same classification as Ok(None).
                                Err(crate::network::fgtw::fleet::FoldError::Empty) => {
                                    ProbeOutcome::Fresh
                                }
                                Err(fold_err) => {
                                    crate::logf!("Network: probe fold failed (indeterminate, not taken): {}", format!("{:?}", fold_err));
                                    let _ = tx.send(QueryResult::Error(format!(
                                        "chain unverifiable: {fold_err:?}"
                                    )));
                                    continue;
                                }
                            }
                        }
                        Err(e) => {
                            // Network unreachable — can't classify. Surface as an error; claiming needs the network anyway.
                            crate::logf!("Network: probe fetch failed: {}", e);
                            // `e` is already a short, plain message from the fleet client (e.g. "No connection to FGTW"). Surface it verbatim — no "can't reach the network to check:" prefix stacking web-stack context the user can't use.
                            let _ = tx.send(QueryResult::Error(e));
                            continue;
                        }
                    };
                    crate::logf!("Network: probe → {}", format!("{:?}", outcome));
                    let _ = tx.send(QueryResult::Probe { outcome, session });
                    continue;
                }

                // Resolve the session roots.
                // First attest is the one moment the handle string exists: derive the three roots (the ~1s spaghettify happens here, once), persist them so a later resume skips both the string and the recompute, then let the string drop.
                // Resume hands the cached roots straight in — no string, no proof recompute.
                let (identity_seed, vault_seed, handle_proof, persist_session) = match req {
                    QueryRequest::FirstAttest(handle) => {
                        let identity_seed = crate::storage::contacts::derive_identity_seed(&handle);
                        let vault_seed = identity_seed;
                        let handle_proof = Handle::username_to_handle_proof(&handle); // ~1s
                                                                                      // Defer persistence until FGTW confirms ownership (below) — a rejected attest must NOT leave session roots that would auto-resume into the same rejection next launch.
                        (identity_seed, vault_seed, handle_proof, true)
                    }
                    QueryRequest::FirstAttestWithRoots(s) => {
                        // Caller-derived roots (the JOIN flow), first-attest persistence semantics.
                        (s.identity_seed, s.vault_seed, s.handle_proof, true)
                    }
                    QueryRequest::Resume(s) => {
                        (s.identity_seed, s.vault_seed, s.handle_proof, false)
                    }
                    // Handled above with an early `continue` — never reaches here.
                    QueryRequest::Probe(_) => unreachable!("Probe is intercepted before roots resolution"),
                };
                crate::log("Network: Querying handle...");

                // Get current port for FGTW query (always PHOTON_PORT now)
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

                // Query FGTW
                let result = crate::network::http::runtime().block_on(load_bootstrap_peers(
                    &keypair,
                    handle_proof,
                    current_port,
                    &identity_seed,
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
                    crate::logf!("Network: Added {} peer(s) to store", other_peers.len());
                }

                // Check result
                let query_result = if let Some(error) = result.error {
                    crate::logf!("Network: ERROR - {}", error);
                    QueryResult::Error(error)
                } else {
                    // Reaching here means the announce did NOT error, and the announce is membership-gated: `ensure_member` already proved this device folds into the fleet chain (bootstrap.rs load_bootstrap_peers_inner). Announce success ⇒ ours.
                    //
                    // The old "taken" verdict was inferred from the UNVERIFIED announce peer echo (a non-empty list not echoing our record), which is a false positive: the echo is not a fold-verified chain, and a device can be a proven member while absent from a given peer-list snapshot (KV lag, list paging, add ordering). A false "taken" then made the consumer clear the session. That inference is deleted.
                    //
                    // "Taken" now comes ONLY from a fold-verified chain naming a DIFFERENT identity, computed exactly like the probe: fetch → fold. Since ensure_member just proved membership, this normally confirms ours; a fold/parse/transport error is INDETERMINATE → QueryResult::Error, NEVER taken, never clear the session.
                    // A resume retries on the next ~30s cycle; a first attest surfaces the error on the launch screen for a manual re-submit (there is no auto-retry loop for FirstAttest).
                    let me = keypair.public.to_bytes();
                    let verdict = match crate::network::fgtw::fleet::fetch(&handle_proof) {
                        // No chain despite a successful membership-gated announce: treat as ours (indeterminate the other way — we were just admitted). Proceed to load.
                        Ok(None) => Ok(true),
                        Ok(Some(blob)) => match blob.fold() {
                            Ok(members) => {
                                if members.contains(&me) {
                                    Ok(true) // fold-proven member — ours
                                } else if blob.genesis_identity_matches(&identity_seed) {
                                    Ok(true) // our identity founded it, this device not yet enrolled — still ours to attest
                                } else {
                                    Ok(false) // fold-verified chain names a DIFFERENT identity — genuinely taken
                                }
                            }
                            // An EMPTY chain (zero ops) is "no one holds this handle", not corruption — same as Ok(None): we were just admitted, ours.
                            Err(crate::network::fgtw::fleet::FoldError::Empty) => Ok(true),
                            Err(fold_err) => {
                                // Dev-log the raw body so a Cloudflare KV read-lag serving a pre-wipe chain is visible (gated to the development feature).
                                #[cfg(feature = "development")]
                                crate::logf!("Network: attest verdict fold failed (indeterminate): {}", format!("{:?}", fold_err));
                                Err(format!("chain unverifiable: {fold_err:?}"))
                            }
                        },
                        Err(e) => Err(e),
                    };

                    let is_ours = match verdict {
                        Ok(ours) => ours,
                        Err(e) => {
                            // Indeterminate — never taken, never clear the session; retry next cycle.
                            crate::logf!("Network: attest verdict indeterminate (keeping session): {}", e);
                            let _ = tx.send(QueryResult::Error(e));
                            continue;
                        }
                    };

                    if is_ours {
                        *handle_proof_store.lock().unwrap() = Some(handle_proof);
                        *identity_seed_store.lock().unwrap() = Some(identity_seed);
                        crate::log("Network: Handle registered to this device");

                        // Ownership confirmed — now it is safe to remember the session roots for resume.
                        // A persist failure must be LOUD: it was silently swallowed here while Android's tohu dirs were unwired, so attest "succeeded" with no session anywhere — avatar picker dead, broadcast empty, every restart back on the attest screen.
                        if persist_session {
                            if let Err(e) = tohu::set_session(&tohu::SessionIdentity {
                                identity_seed,
                                vault_seed,
                                handle_proof,
                            }) {
                                crate::logf!("Network: session persist FAILED (resume will not survive restart): {}", e);
                            }
                        }

                        // === Load all data in background (proof → network → disk → cloud) ===
                        let device_secret_bytes = *keypair.secret.as_bytes();

                        // Dev-mode tap so `vaultinfo` can decrypt this session's vault end-to-end. Logged at the same point in the flow the values themselves come into existence so it's obvious from the trace which run produced which keys. Spaces around `=` so double-clicking the value in a terminal selects only the encoded token. Values printed in voca FULL (PascalCase word concatenation, ~22 words for a 32-byte key) — denser than hex on the page, copy-pasteable as one token, and reads aloud cleanly. `vaultinfo` auto-detects voca vs hex on input. Never enabled in release builds.
                        #[cfg(feature = "development")]
                        {
                            use num_bigint::BigUint;
                            let handle_seed = vault_seed;
                            // device_secret is NEVER logged: the identity/handle seeds are handle-derivable anyway (no new capability in the log), but the device secret is fingerprint-derived and the log is SUBMITTABLE — writing it would hand fleet-membership keys to anyone who can pull the blob (which only needs the handle).
                            crate::logf!("Development: identity_seed = {}  handle_seed = {}", voca::encode(BigUint::from_bytes_be(&identity_seed)), voca::encode(BigUint::from_bytes_be(&handle_seed)));
                        }

                        // Initialize FlatStorage for this session. A bare `return` here would silently strand the UI on the Attesting spinner because the result channel never gets a verdict — the worker has already proven FGTW says the handle is ours, but with no local vault we can't reach Ready. Surface the failure as a QueryResult::Error so the Launch screen flips to its error state and the user sees what happened.
                        // open_shared, NEVER new: on a resume the UI thread already holds this vault's engine and is writing to it (CLUTCH chains, avatars, presence state). A second independent engine here is two in-RAM states racing one file — the exact corruption that bricks a live vault ("seal verification failed" on every open after the stale engine's commit).
                        let storage = match crate::storage::FlatStorage::open_shared(
                            crate::storage::APP,
                            vault_seed,
                            device_secret_bytes,
                        ) {
                            Ok(s) => s,
                            Err(e) => {
                                let msg = format!("storage init failed: {}", e);
                                crate::logf!("Network: {}", msg);
                                let _ = tx.send(QueryResult::Error(msg));
                                return;
                            }
                        };
                        let vault_degraded = storage.degraded();

                        // Load contacts from disk
                        crate::log("Network: Loading contacts from disk...");
                        let mut contacts = crate::storage::contacts::load_all_contacts(&storage);

                        // Load messages for each contact
                        for contact in &mut contacts {
                            if let Err(e) =
                                crate::storage::contacts::load_messages(contact, &storage)
                            {
                                crate::logf!("Network: Failed to load messages for {}: {}", crate::fp(&contact.handle_proof).as_str(), e);
                            }

                            // Load CLUTCH state if ceremony incomplete
                            if contact.clutch_state != crate::types::ClutchState::Complete {
                                if let Ok(Some(state)) = crate::storage::contacts::load_clutch_slots(
                                    &contact.handle_hash,
                                    &storage,
                                ) {
                                    contact.clutch_slots = state.slots;
                                    contact.offer_provenances = state.offer_provenances;
                                    contact.ceremony_id = state.ceremony_id;
                                }
                                if let Ok(Some(keypairs)) =
                                    crate::storage::contacts::load_clutch_keypairs(
                                        &contact.handle_hash,
                                        &storage,
                                    )
                                {
                                    contact.clutch_our_keypairs = Some(keypairs);
                                    if contact.clutch_slots.is_empty() {
                                        contact.init_clutch_slots(identity_seed);
                                    }
                                    // Store local offer in local slot if not already present
                                    if let Some(ref kp) = contact.clutch_our_keypairs {
                                        use crate::crypto::clutch::ClutchOfferPayload;
                                        let our_offer = ClutchOfferPayload::from_keypairs(kp);
                                        if let Some(local_slot) =
                                            contact.get_slot_mut(&identity_seed)
                                        {
                                            if local_slot.offer.is_none() {
                                                local_slot.offer = Some(our_offer);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        crate::logf!("Network: Loaded {} contacts", contacts.len());

                        // Load friendship chains from disk (by friendship_id stored in each contact)
                        crate::log("Network: Loading friendship chains...");
                        let friendship_ids: Vec<crate::types::FriendshipId> =
                            contacts.iter().filter_map(|c| c.friendship_id).collect();
                        let friendships = crate::storage::friendship::load_all_friendships(
                            &friendship_ids,
                            &storage,
                        );
                        crate::logf!("Network: Loaded {} friendships", friendships.len());

                        // Load local avatar
                        let avatar_pixels =
                            crate::avatar::load_avatar_from_seed(&identity_seed, &storage)
                                .map(|(_, p)| p);

                        // Cloud sync (download + merge)
                        crate::log("Network: Syncing with cloud...");
                        if let Ok(Some(cloud_contacts)) =
                            crate::storage::cloud::load_contacts_from_cloud(
                                &identity_seed,
                                &keypair,
                            )
                        {
                            // Merge cloud contacts we don't have locally
                            for cc in cloud_contacts {
                                let exists =
                                    contacts.iter().any(|c| c.handle_proof == cc.handle_proof);
                                if !exists {
                                    let mut contact = cc.to_contact();
                                    // Load CLUTCH state for cloud contact too
                                    if contact.clutch_state != crate::types::ClutchState::Complete {
                                        if let Ok(Some(state)) =
                                            crate::storage::contacts::load_clutch_slots(
                                                &contact.handle_hash,
                                                &storage,
                                            )
                                        {
                                            contact.clutch_slots = state.slots;
                                            contact.offer_provenances = state.offer_provenances;
                                            contact.ceremony_id = state.ceremony_id;
                                        }
                                    }
                                    // Save to local storage
                                    let _ =
                                        crate::storage::contacts::save_contact(&contact, &storage);
                                    contacts.push(contact);
                                }
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
                            handle_proof,
                            identity_seed,
                            contacts,
                            friendships,
                            avatar_pixels,
                            peers: result.peers,
                            vault_degraded,
                        }))
                    } else {
                        // Reached only via a FOLD-VERIFIED chain whose genesis identity is NOT ours (see verdict above) — a genuine chain-proven takeover. Dev-log the raw fetch body (hex) so a Cloudflare KV read-lag serving a pre-wipe chain is catchable from the trace.
                        crate::log("Network: Handle is CLAIMED by another device (fold-verified different identity)");
                        #[cfg(feature = "development")]
                        {
                            // Dump the chain's identity binding so a Cloudflare KV read-lag serving a pre-wipe chain is catchable: a stale chain shows the OLD genesis identity and op set even though the handle was wiped.
                            if let Ok(Some(blob)) =
                                crate::network::fgtw::fleet::fetch(&handle_proof)
                            {
                                let genesis_ident = blob
                                    .ops
                                    .first()
                                    .map(|op| hex::encode(op.identity_pubkey))
                                    .unwrap_or_else(|| "<no ops>".to_string());
                                crate::logf!("Development: attest-taken chain = {} op(s), genesis identity_pubkey = {}", blob.ops.len(), genesis_ident);
                            }
                        }
                        // The `AlreadyAttested` payload is a peer record for the consumer's display.
                        // If the (unverified) peer echo carried one, pass it; if not, the takeover is still chain-proven but we have no concrete peer to show — surface as a (retryable) error rather than fabricate a record or panic on an empty list.
                        match result.peers.first() {
                            Some(peer) => QueryResult::AlreadyAttested(peer.clone()),
                            None => {
                                crate::log(
                                    "Network: chain-proven takeover but no peer record echoed — treating as retryable",
                                );
                                QueryResult::Error(
                                    "handle claimed by another identity (no peer record)".to_string(),
                                )
                            }
                        }
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
        identity_seed: Arc<Mutex<Option<[u8; 32]>>>,
        our_handle_proof: Arc<Mutex<Option<[u8; 32]>>>,
        port: Arc<Mutex<u16>>,
    ) {
        thread::spawn(move || {
            crate::log("Network: Search worker initialized");

            while let Ok(handle) = rx.recv() {
                crate::log("Network: Searching for a handle...");

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

                let result = Self::search_with_refresh(
                    &handle,
                    handle_proof,
                    &transport_arc,
                    &keypair,
                    &identity_seed,
                    &our_handle_proof,
                    &port,
                );

                let _ = tx.send(result);
            }
        });
    }

    /// Look up a handle in the local peer store. If not found, re-announce to FGTW (which refreshes the peer list) and retry once. This covers the common case where the target registered after our last announce.
    fn search_with_refresh(
        handle: &str,
        handle_proof: [u8; 32],
        peer_store: &Arc<Mutex<PeerStore>>,
        keypair: &Keypair,
        identity_seed: &Arc<Mutex<Option<[u8; 32]>>>,
        our_handle_proof: &Arc<Mutex<Option<[u8; 32]>>>,
        port: &Arc<Mutex<u16>>,
    ) -> SearchResult {
        // First pass — local peer store
        if let Some(result) = Self::lookup_in_store(handle, handle_proof, peer_store) {
            return result;
        }

        // Not found locally — re-announce with our own credentials to pull a fresh peer list, then retry. This is the critical path for "both attested, THEN added each other": the target registered on FGTW after our last fetch, so it's absent from the local store until we re-query. Only possible once we've attested (identity_seed + handle_proof are set).
        let (seed, our_port) = {
            let s = identity_seed.lock().unwrap();
            let p = port.lock().unwrap();
            match *s {
                Some(seed) => (seed, *p),
                None => return SearchResult::NotFound,
            }
        };

        // Use our cached attested handle_proof directly (set by the query worker at attest success).
        // Earlier this scanned the peer store for our own device record and bailed to NotFound if absent — which is EXACTLY the fresh-attest case (store not yet populated with ourselves), so the refresh never ran and the peer stayed unfindable until a restart. The arc always has it post-attest.
        let our_handle_proof = match *our_handle_proof.lock().unwrap() {
            Some(hp) => hp,
            None => return SearchResult::NotFound,
        };

        crate::logf!("Network: '{}' not in local store — refreshing peer list from FGTW", handle);
        let refresh = crate::network::http::runtime().block_on(
            crate::network::fgtw::bootstrap::load_bootstrap_peers(
                keypair,
                our_handle_proof,
                our_port,
                &seed,
            ),
        );

        if refresh.peers.is_empty() {
            return SearchResult::NotFound;
        }

        // Merge fresh peers into the store
        let our_pubkey = keypair.public.as_bytes();
        {
            let mut store = peer_store.lock().unwrap();
            for peer in refresh
                .peers
                .iter()
                .filter(|p| p.device_pubkey.as_bytes() != our_pubkey)
            {
                store.add_peer(peer.clone());
            }
        }

        // Second pass after refresh
        Self::lookup_in_store(handle, handle_proof, peer_store).unwrap_or(SearchResult::NotFound)
    }

    fn lookup_in_store(
        handle: &str,
        handle_proof: [u8; 32],
        peer_store: &Arc<Mutex<PeerStore>>,
    ) -> Option<SearchResult> {
        let store = peer_store.lock().unwrap();
        let peers = store.get_devices_for_handle(&handle_proof);
        peers.first().map(|peer| {
            SearchResult::Found(FoundPeer {
                handle: HandleText::new(handle),
                handle_proof,
                device_pubkey: peer.device_pubkey.clone(),
                ip: peer.ip,
                local_ip: peer.local_ip,
            })
        })
    }

    // ===== Public API =====

    /// First attest from a typed handle (non-blocking).
    /// The worker derives + persists the session roots, so subsequent launches use [`query_resume`](Self::query_resume) instead.
    pub fn query(&self, handle: String) {
        let _ = self.query_sender.send(QueryRequest::FirstAttest(handle));
    }

    /// Classify a typed handle without announcing (non-blocking) — drives the attest three-way branch. Returns [`QueryResult::Probe`].
    pub fn probe(&self, handle: String) {
        let _ = self.query_sender.send(QueryRequest::Probe(handle));
    }

    /// First attest with caller-derived roots (non-blocking) — the JOIN flow derives them once up front; this skips the string and the second ~1s proof while keeping first-attest persistence (roots remembered on FGTW confirmation).
    pub fn query_first_attest_with_roots(&self, session: tohu::SessionIdentity) {
        let _ = self.query_sender.send(QueryRequest::FirstAttestWithRoots(session));
    }

    /// Resume attestation from the cached session roots (non-blocking) — no handle string, no ~1s proof recompute.
    pub fn query_resume(&self, session: tohu::SessionIdentity) {
        let _ = self.query_sender.send(QueryRequest::Resume(session));
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

    /// Cache handle_proof after successful attestation (used for in-session handle searches).
    pub fn set_handle_proof(&self, handle_proof: [u8; 32]) {
        *self.last_handle_proof.lock().unwrap() = Some(handle_proof);
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
