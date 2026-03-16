// ┌──────────────────────────────────────────────────────────────────────────────────┐
// │ PHOTON SOURCE MAP — keep updated when pub items or files change                 │
// ├──────────────────────────────────────────────────────────────────────────────────┤
// │                                                                                 │
// │ lib.rs ── constants, logging, debug macro, module re-exports                    │
// │   PHOTON_PORT=4383, PHOTON_PORT_FALLBACK=3546, MULTICAST_PORT=4384             │
// │   OSC_PER_SEC, PEER_EXPIRY_OSC (7 days), KBUCKET_STALE_OSC (1 hour)           │
// │   init_logging(), log()                                                         │
// │                                                                                 │
// │ main.rs ── winit event loop, window creation, tokio async runtime               │
// │                                                                                 │
// │ crypto/                                                                         │
// │ ├── chain.rs ── rolling-chain encryption (512-link, 16KB)                       │
// │ │   struct Chain { links: [[u8;32]; 512], last_ack_time: Option<EagleTime> }    │
// │ │     ::from_bytes(), to_bytes(), from_full_bytes()                              │
// │ │     ::current_key(), link(idx), links()                                       │
// │ │     ::advance(eagle_time, our_plaintext, their_plaintext)                     │
// │ │   enum ChainError { DecryptionFailed, InvalidMessage, NotInitialized,         │
// │ │     EncryptionFailed(String), SignatureInvalid, InvalidEncoding,              │
// │ │     AckMismatch, ParticipantNotFound }                                        │
// │ │   CHAIN_LINKS=512, HISTORY_LINKS=256, ACTIVE_LINKS=256                       │
// │ │   LINK_SIZE=32, CHAIN_SIZE=16384, CURRENT_KEY_INDEX=511                      │
// │ │   L1_SIZE=30720, L1_ROUNDS=3                                                  │
// │ │   derive_salt(prev_plaintext, chain)                                          │
// │ │   generate_scratch(chain, salt), generate_scratch_at_offset(chain, salt, off) │
// │ │   generate_confirmation_smear(message, chain)                                 │
// │ │   generate_ack_proof(eagle_time, plaintext_hash, chain)                       │
// │ │   verify_ack_proof(eagle_time, plaintext_hash, chain, received_proof)         │
// │ │   derive_nonce(eagle_time), encrypt_layers(), decrypt_layers()                │
// │ │                                                                               │
// │ ├── clutch.rs ── 8-algorithm parallel key ceremony                              │
// │ │   smear_hash(data), derive_conversation_token(participant_seeds)              │
// │ │   derive_ceremony_instance(offers), spaghettify(input)                        │
// │ │                                                                               │
// │ ├── handle_proof.rs ── memory-hard handle attestation (~1s)                     │
// │ │   handle_proof(hash)                                                          │
// │ │                                                                               │
// │ ├── keys.rs ── identity key management (TODO)                                   │
// │ │                                                                               │
// │ ├── self_verify.rs ── Ed25519 binary signature verification                     │
// │ │   AUTHOR_PUBKEY, SYSTEM_PUBKEYS                                               │
// │ │   is_system_pubkey(pubkey), verify_binary_hash()                              │
// │ │                                                                               │
// │ └── shards.rs ── social recovery key sharding (TODO)                            │
// │                                                                                 │
// │ network/                                                                        │
// │ ├── fgtw/ ── Fractal Gradient Trust Web (Kademlia DHT)                          │
// │ │   ├── blob.rs ── binary blob storage/retrieval                                │
// │ │   │                                                                           │
// │ │   ├── bootstrap.rs ── initial peer discovery                                  │
// │ │   │   load_bootstrap_peers()                                                  │
// │ │   │                                                                           │
// │ │   ├── fingerprint.rs ── device fingerprint derivation                         │
// │ │   │                                                                           │
// │ │   ├── node.rs ── Kademlia routing table, k-buckets                            │
// │ │   │                                                                           │
// │ │   ├── peer_store.rs ── peer caching                                           │
// │ │   │   struct PeerStore                                                        │
// │ │   │                                                                           │
// │ │   ├── protocol.rs ── VSF-encoded FGTW + CLUTCH messages                       │
// │ │   │   enum FgtwMessage { ... }                                                │
// │ │   │   struct PeerRecord, SyncRecord                                           │
// │ │   │                                                                           │
// │ │   └── relay.rs ── relay node logic                                            │
// │ │                                                                               │
// │ ├── handle_query.rs ── handle attestation and lookup                            │
// │ │   struct HandleQuery                                                          │
// │ │     ::new(keypair, event_proxy), query(handle), try_recv()                    │
// │ │     ::try_recv_online(), search(handle), try_recv_search()                    │
// │ │     ::set_handle_proof(), get_handle_proof()                                  │
// │ │     ::set_transport(), get_transport(), port(), socket()                      │
// │ │   enum QueryResult { Success(AttestationData), AlreadyAttested, Error }       │
// │ │   struct AttestationData { handle, handle_proof, identity_seed,               │
// │ │     contacts, friendships, avatar_pixels, peers }                             │
// │ │                                                                               │
// │ ├── inspect.rs ── network diagnostic utilities                                  │
// │ │                                                                               │
// │ ├── peer_updates.rs ── peer state change notifications                          │
// │ │   struct PeerUpdate, PeerUpdateClient                                         │
// │ │                                                                               │
// │ ├── pt/ ── Photon Transfer (large message transport)                            │
// │ │   ├── buffer.rs ── reassembly buffer                                          │
// │ │   ├── packets.rs ── packet framing                                            │
// │ │   │   struct PTSpec                                                           │
// │ │   ├── state.rs ── transfer state machine                                      │
// │ │   │   enum Direction, TransferState                                           │
// │ │   │   enum PTError, struct OutboundTransfer                                   │
// │ │   └── window.rs ── sliding window flow control                                │
// │ │   struct PTManager { outbound, inbound, keypair, next_stream_id }             │
// │ │     SINGLE_PACKET_MAX=1024                                                    │
// │ │     ::new(keypair), send(addr, data), send_with_pubkey()                      │
// │ │     ::handle_spec(), handle_spec_ack(), handle_data(), handle_ack()           │
// │ │   struct RelayInfo { recipient_pubkey, payload }                               │
// │ │   struct TickSend { peer_addr, wire_bytes, also_tcp, relay }                  │
// │ │                                                                               │
// │ ├── status.rs ── P2P ping/pong, CLUTCH orchestration                            │
// │ │   struct StatusChecker                                                        │
// │ │     ::new(socket, keypair, contacts, sync_records, event_proxy)               │
// │ │   enum StatusUpdate {                                                         │
// │ │     Online { peer_pubkey, is_online, peer_addr, sync_records },               │
// │ │     ChatMessage { conversation_token, prev_msg_hp, ciphertext, ... },         │
// │ │     MessageAck { conversation_token, acked_eagle_time, plaintext_hash },      │
// │ │     PTReceived { peer_addr, data },                                           │
// │ │     PTSendComplete { peer_addr },                                             │
// │ │     ClutchOfferReceived { conversation_token, offer_provenance, ... },        │
// │ │     ClutchKemResponseReceived { ... },                                        │
// │ │     ClutchCompleteReceived { ... },                                           │
// │ │     LanPeerDiscovered { handle_proof, local_ip, port } }                     │
// │ │   struct PingRequest { peer_addr, peer_pubkey }                               │
// │ │   struct MessageRequest { peer_addr, recipient_pubkey, conversation_token,    │
// │ │     prev_msg_hp, ciphertext, eagle_time }                                     │
// │ │   struct AckRequest { peer_addr, recipient_pubkey, conversation_token,        │
// │ │     acked_eagle_time, plaintext_hash }                                        │
// │ │   struct PTSendRequest { peer_addr, data }                                    │
// │ │   struct ClutchOfferRequest, ClutchKemResponseRequest,                        │
// │ │     ClutchCompleteRequest, LanBroadcastRequest, ClearPtSendsRequest           │
// │ │                                                                               │
// │ ├── tcp.rs ── TCP fallback for large payloads                                   │
// │ │   send(stream, data), recv(stream)                                            │
// │ │                                                                               │
// │ └── udp.rs ── UDP socket utilities                                              │
// │     async send(socket, data, addr), send_sync(socket, data, addr)               │
// │     log_received(data, addr)                                                    │
// │     get_local_ip(), get_broadcast_addr()                                        │
// │                                                                                 │
// │ platform/                                                                       │
// │ ├── mod.rs ── platform detection                                                │
// │ └── jni_android.rs ── Android JNI bridge                                        │
// │                                                                                 │
// │ storage/                                                                        │
// │ ├── cloud.rs ── FGTW cloud backup (contacts sync)                               │
// │ │   struct CloudContact, enum CloudError                                        │
// │ │   contacts_storage_key(identity_seed, device_secret)                          │
// │ │   contacts_encryption_key(identity_seed, device_secret)                       │
// │ │                                                                               │
// │ ├── contacts.rs ── local encrypted contact storage                              │
// │ │   struct ContactIdentity, enum StorageError                                   │
// │ │   derive_identity_seed(handle)                                                │
// │ │                                                                               │
// │ ├── flat.rs ── flat file storage (root dir, identity_seed, device_secret)       │
// │ │   struct FlatStorage                                                          │
// │ │     ::new(identity_seed, device_secret)                                       │
// │ │     ::load_contact_list(), save_contact_list()                                │
// │ │     ::load_contact_blob(), save_contact_blob(), delete_contact_blob()         │
// │ │     ::load_chain_blob(), save_chain_blob(), delete_chain_blob()               │
// │ │     ::load_avatar(), save_avatar(), delete_avatar()                           │
// │ │     ::load_message(), save_message(), delete_message()                        │
// │ │   struct ContactListEntry { handle, identity_seed }                            │
// │ │   struct ContactBlob { identity_seed, handle, added, friendship_id,           │
// │ │     relationship_seed, trust_level, devices: Vec<DeviceBlob> }                │
// │ │   struct DeviceBlob { device_id, device_pubkey, ip, local_ip, local_port,     │
// │ │     last_seen, clutch_state, keypairs, slots, ceremony_id, ... }              │
// │ │   struct ChainBlob { chain_links, chain_last_ack_time, message_index }        │
// │ │   struct MessageBlob { author_index, status, eagle_time, plaintext, ... }     │
// │ │   struct MessageIndexEntry { network_id, eagle_time, author_index }           │
// │ │                                                                               │
// │ └── friendship.rs ── per-friendship chain persistence                           │
// │     enum FriendshipStorageError                                                 │
// │                                                                                 │
// │ types/                                                                          │
// │ ├── contact.rs ── contact/friendship re-exports                                 │
// │ │   struct PartySlot { handle_hash, offer, kem_secrets_from/to_them, ... }      │
// │ │     ::new(handle_hash), is_complete()                                         │
// │ │   struct ChatMessage { content, timestamp, is_outgoing, delivered }            │
// │ │     ::new(content, is_outgoing), new_with_timestamp()                         │
// │ │   struct HandleText(String) ::new(s), as_str()                                │
// │ │   struct ContactId([u8;32]) ::from_pubkey(), from_bytes(), as_bytes()         │
// │ │   enum ClutchState { Pending, AwaitingProof, Complete }                       │
// │ │   enum TrustLevel { Stranger, Known, Trusted, Inner }                         │
// │ │   struct Contact {                                                            │
// │ │     id, handle, handle_proof, handle_hash, public_identity,                   │
// │ │     ip, local_ip, local_port,                                                 │
// │ │     relationship_seed, friendship_id, clutch_state,                           │
// │ │     clutch_our_keypairs, clutch_slots, ceremony_id,                           │
// │ │     clutch_pending_kem, clutch_offer_sent, clutch_*_proof,                    │
// │ │     clutch_*_in_progress, completed_their_hqc_prefix,                         │
// │ │     offer_provenances, trust_level, added, last_seen,                         │
// │ │     is_online, messages, message_scroll_offset,                               │
// │ │     avatar_pixels, avatar_scaled, avatar_scaled_diameter }                    │
// │ │     ::new(), with_ip(), with_seed(), with_trust_level()                       │
// │ │     ::update_last_seen(), best_addr(), can_be_custodian()                     │
// │ │     ::get_ceremony_id(), init_clutch_slots()                                  │
// │ │     ::get_slot_index(), get_slot_mut(), get_slot()                             │
// │ │     ::all_slots_complete(), insert_message_sorted()                            │
// │ │                                                                               │
// │ ├── device.rs ── device identity                                                │
// │ │   struct DevicePubkey                                                         │
// │ │   ed25519_secret_to_x25519(ed_secret)                                         │
// │ │                                                                               │
// │ ├── friendship.rs ── friendship and ceremony types                              │
// │ │   struct CeremonyId([u8;32])                                                  │
// │ │     ::derive_base(handle_hashes), derive(handle_hashes, provenances)          │
// │ │     ::from_bytes(), as_bytes()                                                │
// │ │   struct FriendshipId([u8;32])                                                │
// │ │     ::derive(handle_hashes), from_bytes(), as_bytes()                         │
// │ │     ::to_base64(), from_base64()                                              │
// │ │   struct FriendshipChains { friendship_id, conversation_token,                │
// │ │     chains, participants }                                                    │
// │ │                                                                               │
// │ ├── handle.rs ── handle type                                                    │
// │ │   struct Handle { text, key }                                                 │
// │ │     ::new(), to_handle_proof(), username_to_handle_proof()                    │
// │ │                                                                               │
// │ ├── message.rs ── message structures                                            │
// │ │   struct MessageId([u8;32]) ::new(), as_bytes(), to_vsf(), from_vsf()         │
// │ │   struct Message { nonce, sequence, payload, timestamp }                      │
// │ │     ::new(), to_vsf_bytes(), from_vsf_bytes()                                 │
// │ │   struct EncryptedMessage { sequence, ciphertext }                             │
// │ │     ::to_vsf_bytes(), from_vsf_bytes()                                        │
// │ │   enum MessageStatus { Pending, Sent, Delivered, Read, Failed }               │
// │ │     ::to_vsf(), from_vsf()                                                    │
// │ │                                                                               │
// │ ├── peer.rs ── peer connection state                                            │
// │ │   struct Peer { public_identity, address, last_seen, connection_state }       │
// │ │     ::new(), update_connection_state(), is_online()                            │
// │ │   enum ConnectionState { Disconnected, Connecting, Connected, Authenticated } │
// │ │   struct DhtAnnouncement { public_key, port, timestamp, signature }            │
// │ │                                                                               │
// │ ├── seed.rs ── cryptographic seed                                               │
// │ │   struct Seed([u8;32])                                                        │
// │ │                                                                               │
// │ └── shard.rs ── key shard structures                                            │
// │     struct KeyShard, ShardId([u8;16]), DecryptedShard                            │
// │     struct RecoveryRequest, RecoveryApproval, ShardDistribution                 │
// │                                                                                 │
// │ ui/                                                                             │
// │ ├── app.rs ── application state machine                                         │
// │ │   struct PhotonApp (main app state — see text_editing.rs for text methods)    │
// │ │   struct TextState { chars, widths, width, blinkey_index,                     │
// │ │     scroll_offset, selection_anchor, textbox_focused, is_empty }              │
// │ │     ::new(), insert(), insert_str(), remove(), delete_range()                  │
// │ │   struct TextLayout { center_x/y, box_width/height, usable_*,                │
// │ │     margin, font_size, line_height, button_area }                             │
// │ │     ::new(w, h, span, ru, app_state)                                          │
// │ │     ::blinkey_x(text), blinkey_y(), text_start_x(text)                        │
// │ │   struct PixelRegion { x, y, w, h }                                           │
// │ │     ::new(), from_signed(), contains(), right(), bottom(), center()            │
// │ │   struct Layout { logo_spectrum, photon_text, textbox,                        │
// │ │     attest_block, contacts, header, message_area }                            │
// │ │   struct AttestBlockLayout { error, textbox, hint, attest }                   │
// │ │     ::new(block)                                                              │
// │ │   struct ContactsHeaderLayout { avatar, handle, hint, textbox, separator }    │
// │ │     ::new(block), avatar_center_radius()                                      │
// │ │   struct ContactsRowsLayout { rows, row_height, avatar_diameter, ... }        │
// │ │     ::new(), row_region(), row_avatar_center()                                │
// │ │     ::row_text_position(), visible_row_count()                                │
// │ │   struct ContactsUnifiedLayout { user_avatar, handle, hint, ... }             │
// │ │   struct ClutchKeygenResult { contact_id, keypairs }                          │
// │ │   struct ClutchKemEncapResult { contact_id, kem_response, local_secrets, ... }│
// │ │   struct ClutchCeremonyResult { contact_id, friendship_chains,                │
// │ │     eggs_proof, their_handle_hash, ceremony_id, ... }                         │
// │ │   soft_limit(x, max)                                                          │
// │ │                                                                               │
// │ ├── avatar.rs ── avatar encoding/upload/download                                │
// │ │   AVATAR_SIZE, encode_avatar_from_image(image_data)                           │
// │ │                                                                               │
// │ ├── colour.rs ── colour utilities                                               │
// │ │                                                                               │
// │ ├── compositing.rs ── compositing pipeline, layout calculation, rendering       │
// │ │                                                                               │
// │ ├── display_profile.rs ── ICC colour profile conversion                         │
// │ │   struct DisplayConverter { xyz_to_display, r/g/b_trc }                       │
// │ │     ::new(), convert_avatar(vsf_rgb)                                          │
// │ │                                                                               │
// │ ├── drawing.rs ── primitive drawing (circles, lines, fills)                     │
// │ │                                                                               │
// │ ├── keyboard.rs ── input handling (key events → app state changes)              │
// │ │                                                                               │
// │ ├── mouse.rs ── mouse/touch input                                               │
// │ │                                                                               │
// │ ├── renderer_*.rs ── platform-specific renderers (7 backends)                   │
// │ │   android, linux_softbuffer, linux_wgpu, macos,                               │
// │ │   macos_softbuffer, redox, windows                                            │
// │ │                                                                               │
// │ ├── text_editing.rs ── text input methods on PhotonApp                          │
// │ │   ::textbox_is_focused(), font_size(), textbox_width/height()                 │
// │ │   ::textbox_center_y(), textbox_left/right()                                  │
// │ │   ::recalculate_char_widths(), render_text_clipped()                          │
// │ │   ::update_text_scroll(), update_selection_scroll()                           │
// │ │   ::get_selection_range(), delete_selection(), get_selected_text()            │
// │ │   ::paste_text(), handle_blinkey_left(), next_blink_wake_time()               │
// │ │   ::start/stop/undraw/draw/flip_blinkey()                                     │
// │ │   ::add/subtract_blinkey_top/bottom(), invert_selection()                     │
// │ │                                                                               │
// │ ├── text_rasterizing.rs ── font rendering (cosmic-text)                         │
// │ │                                                                               │
// │ └── theme.rs ── colour palette                                                  │
// │                                                                                 │
// │ bin/                                                                            │
// │ ├── photon-keygen.rs ── signing key generation                                  │
// │ ├── photon-signature-signer.rs ── binary signing tool                           │
// │ └── test-device-key.rs ── device key diagnostic                                 │
// │                                                                                 │
// └──────────────────────────────────────────────────────────────────────────────────┘

// Global debug flag - can be toggled at runtime with Ctrl+D
use std::sync::atomic::AtomicBool;
pub static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);

/// Photon network ports - used for ALL network communication
/// UDP: peer-to-peer status pings, CLUTCH ceremony, chat messages
/// TCP: large payloads (full CLUTCH offers ~548KB, KEM responses ~17KB)
/// FGTW: handle registration and peer discovery announcements
/// Primary: 4383, Fallback: 3546 (both IANA unassigned)
pub const PHOTON_PORT: u16 = 4383;
pub const PHOTON_PORT_FALLBACK: u16 = 3546;

/// Multicast port for LAN peer discovery
/// Separate from main port to avoid SO_REUSEADDR complexity
/// 4384 is IANA unassigned
pub const MULTICAST_PORT: u16 = 4384;

/// Eagle Time: oscillations per second (hydrogen hyperfine transition)
pub const OSC_PER_SEC: i64 = vsf::OSCILLATIONS_PER_SECOND as i64;

/// Peer expiry: 7 days
pub const PEER_EXPIRY_OSC: i64 = 604_800 * OSC_PER_SEC;

/// K-bucket stale entry eviction: 1 hour
pub const KBUCKET_STALE_OSC: i64 = 3_600 * OSC_PER_SEC;

// Debug print macro - only prints if DEBUG_ENABLED is true
// Compiled out entirely in release builds
#[cfg(debug_assertions)]
#[macro_export]
macro_rules! debug_println {
    ($($arg:tt)*) => {
        if $crate::DEBUG_ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
            println!($($arg)*);
        }
    };
}

#[cfg(not(debug_assertions))]
#[macro_export]
macro_rules! debug_println {
    ($($arg:tt)*) => {};
}

// Logging - feature-gated, compiles to nothing without --features logging
// - Android: log::info!
// - Windows: %APPDATA%\photon\photon.log
// - Other: stdout

#[cfg(all(feature = "logging", target_os = "windows"))]
static WINDOWS_LOG_FILE: std::sync::OnceLock<std::sync::Mutex<std::fs::File>> =
    std::sync::OnceLock::new();

/// Initialize logging - must be called early in main() on Windows
#[cfg(all(feature = "logging", target_os = "windows"))]
pub fn init_logging() {
    let _ = WINDOWS_LOG_FILE.get_or_init(|| {
        let config_dir = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        let log_dir = config_dir.join("photon");
        let _ = std::fs::create_dir_all(&log_dir);
        let log_path = log_dir.join("photon.log");
        let file = std::fs::File::create(&log_path).expect("Failed to create log file");
        std::sync::Mutex::new(file)
    });
}

#[cfg(not(all(feature = "logging", target_os = "windows")))]
pub fn init_logging() {}

// Disabled: compiles to nothing
#[cfg(not(feature = "logging"))]
#[inline(always)]
pub fn log(_msg: &str) {}

// Enabled: platform-specific output
#[cfg(feature = "logging")]
pub fn log(msg: &str) {
    #[cfg(target_os = "android")]
    log::info!("{}", msg);

    #[cfg(target_os = "windows")]
    {
        use std::io::Write;
        if let Some(file_mutex) = WINDOWS_LOG_FILE.get() {
            if let Ok(mut file) = file_mutex.lock() {
                let _ = writeln!(file, "{}", msg);
                let _ = file.flush();
            }
        }
    }

    #[cfg(not(any(target_os = "android", target_os = "windows")))]
    println!("{}", msg);
}

pub mod crypto;
pub mod network;
pub mod platform;
pub mod storage;
pub mod types;
pub mod ui;

// Re-export commonly used items from submodules
pub use ui::avatar;
pub use ui::display_profile;

pub use types::*;

// Android JNI initialization
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn JNI_OnLoad(vm: jni::JavaVM, _: *mut std::os::raw::c_void) -> jni::sys::jint {
    // Initialize Android logger with module filtering
    // Filter out noisy cosmic_text and reqwest debug logs
    android_logger::init_once(
        android_logger::Config::default()
            .with_tag("photon")
            .with_max_level(log::LevelFilter::Debug)
            .with_filter(
                android_logger::FilterBuilder::new()
                    .parse("debug,cosmic_text=warn,reqwest=warn")
                    .build(),
            ),
    );

    // Set panic hook for better crash diagnostics
    std::panic::set_hook(Box::new(|panic_info| {
        log::error!("PHOTON PANIC: {}", panic_info);
        if let Some(location) = panic_info.location() {
            log::error!("PANIC location: {}:{}", location.file(), location.line());
        }
    }));

    log::info!("Photon JNI loaded (PID: {})", std::process::id());
    jni::sys::JNI_VERSION_1_6
}
