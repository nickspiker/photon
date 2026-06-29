// PHOTON SOURCE MAP — keep updated when pub items or files change
//
// lib.rs ── constants, logging, debug macro, module re-exports PHOTON_PORT=4383, PHOTON_PORT_FALLBACK=3546, MULTICAST_PORT=4384 OSC_PER_SEC, PEER_EXPIRY_OSC (7 days), KBUCKET_STALE_OSC (1 hour) init_logging(), log()
//
// main.rs ── winit event loop, window creation, tokio async runtime
//
// crypto/ ├── chain.rs ── the braid: rolling-chain encryption (512-link, 16KB; see BRAID.md) │   struct Chain { links: [[u8;32]; 512], last_ack_time: Option<EagleTime> } │     ::from_bytes(), to_bytes(), from_full_bytes() │     ::current_key(), link(idx), links() │     ::advance(eagle_time, our_plaintext, their_plaintexts) — the braid: weaves up to two prior peer plaintexts (length-prefixed, peer-entropy-first) │   enum ChainError { DecryptionFailed, InvalidMessage, NotInitialized, │     EncryptionFailed(String), SignatureInvalid, InvalidEncoding, │     AckMismatch, ParticipantNotFound } │   CHAIN_LINKS=512, HISTORY_LINKS=256, ACTIVE_LINKS=256 │   LINK_SIZE=32, CHAIN_SIZE=16384, CURRENT_KEY_INDEX=511 │   L1_SIZE=30720, L1_ROUNDS=3 │   derive_salt(prev_plaintext, chain) │   generate_scratch(chain, salt), generate_scratch_at_offset(chain, salt, off) │   generate_confirmation_smear(message, chain) │   generate_ack_proof(eagle_time, plaintext_hash, chain) │   verify_ack_proof(eagle_time, plaintext_hash, chain, received_proof) │   derive_nonce(eagle_time), encrypt_layers(), decrypt_layers() │ ├── clutch.rs ── 8-algorithm parallel key ceremony │   smear_hash(data), derive_conversation_token(participant_seeds) │   derive_ceremony_instance(offers), spaghettify(input) │ ├── handle_proof.rs ── memory-hard handle attestation (~1s) │   handle_proof(hash) │ ├── keys.rs ── identity key management (TODO) │ ├── self_verify.rs ── Ed25519 binary signature verification │   AUTHOR_PUBKEY, SYSTEM_PUBKEYS │   is_system_pubkey(pubkey), verify_binary_hash() │ └── shards.rs ── social recovery key sharding (TODO)
//
// network/ ├── fgtw/ ── Fractal Gradient Trust Web (Kademlia DHT) │   ├── blob.rs ── binary blob storage/retrieval │   │ │   ├── bootstrap.rs ── initial peer discovery │   │   load_bootstrap_peers() │   │ │   ├── fingerprint.rs ── device fingerprint derivation │   │ │   ├── node.rs ── Kademlia routing table, k-buckets │   │ │   ├── peer_store.rs ── peer caching │   │   struct PeerStore │   │ │   ├── protocol.rs ── VSF-encoded FGTW + CLUTCH messages │   │   enum FgtwMessage { ... + PhonebookRequest/Response (peers-are-FGTW gossip) + AvatarRequest/Response (av_req/av_resp — direct P2P avatar exchange between mutual contacts, signed over the avatar bytes' hash) } │   │   struct PeerRecord { handle_proof, device_pubkey, ip, local_ip, last_seen, signature } (self-signed: ::sign/::verify via the Ed25519 device_pubkey, so gossiped records are relay-independent), SyncRecord │   │ │   ├── keyring.rs ── client side of the multi-device identity keyring: constant-size Merkle-root device-set commitment (leaf = ihi::keyring::leaf(DEVICE, device_pubkey, handle_proof); pubkey-based so FGTW recomputes-and-verifies; root hides device count). genesis/add/remove ops (wire: section "keyring", device-signed envelope, kr_ver forensic stamp NOT a fork), ensure_keyring_and_prove (v0 single-device: deterministic [device_leaf], genesis-once + inclusion proof), add_proof_fields. v0 = single-device only; the device-ADD ceremony (fleet chain/weave + fleet CLUTCH) is the deferred multi-device piece │   └── relay.rs ── relay node logic │ ├── clock_check.rs ── one-shot wall-clock sanity check via nunc-time consensus (desktop-only; warn-only — never corrects the clock) │   spawn_clock_check(tx, wake) ── off-thread nunc::query, posts ClockCheckResult │   struct ClockJumpDetector ── monotonic-vs-wall jump trigger for mid-session re-checks │   enum ClockCheckResult { Ok { offset_secs, confidence_secs, sources_used/queried }, Unavailable } │ ├── handle_query.rs ── handle attestation and lookup │   struct HandleQuery │     ::new(keypair, event_proxy), query(handle), query_resume(session), try_recv() │     ::try_recv_online(), search(handle), try_recv_search() │     ::set_handle_proof(proof), get_handle_proof() │     ::set_transport(), get_transport(), port(), socket() │   enum QueryRequest { FirstAttest(String), Resume(tohu::SessionIdentity) } │   enum QueryResult { Success(AttestationData), AlreadyAttested, Error } │   struct AttestationData { handle_proof, identity_seed, │     contacts, friendships, avatar_pixels, peers } │ ├── http.rs ── shared pooled HTTP for FGTW │   runtime() (one persistent tokio) · async_client() · blocking() — reused reqwest clients keep TLS warm │ ├── inspect.rs ── network diagnostic utilities, VSF disk I/O │   vsf_write(path, encrypted, label, decrypted, device_secret) │   vsf_read(path, label, device_secret) │ ├── peer_updates.rs ── peer state change notifications │   struct PeerUpdate, PeerUpdateClient │ ├── pt/ ── Photon Transfer (large message transport) │   ├── buffer.rs ── reassembly buffer │   ├── packets.rs ── packet framing │   │   struct PTSpec │   ├── state.rs ── transfer state machine │   │   enum Direction, TransferState │   │   enum PTError, struct OutboundTransfer │   └── window.rs ── sliding window flow control │   struct PTManager { outbound, inbound, keypair, next_stream_id } │     SINGLE_PACKET_MAX=1024 │     ::new(keypair), send(addr, data), send_with_pubkey() │     ::handle_spec(), handle_spec_ack(), handle_data(), handle_ack() │   struct RelayInfo { recipient_pubkey, payload } │   struct TickSend { peer_addr, wire_bytes, tcp_payload, relay } │ ├── status.rs ── P2P ping/pong, CLUTCH orchestration │   struct StatusChecker │     ::new(socket, keypair, contacts, sync_records, event_proxy) │   enum StatusUpdate { │     Online { peer_pubkey, is_online, peer_addr, sync_records }, │     ChatMessage { conversation_token, prev_msg_hp, ciphertext, ... }, │     MessageAck { conversation_token, acked_eagle_time, plaintext_hash }, │     PTReceived { peer_addr, data }, │     PTSendComplete { peer_addr }, │     ClutchOfferReceived { conversation_token, offer_provenance, ... }, │     ClutchKemResponseReceived { ... }, │     ClutchCompleteReceived { ... }, │     AvatarRequestReceived { sender_pubkey, sender_addr }, AvatarReceived { responder_pubkey, avatar_vsf, sender_addr } (P2P avatar exchange; UI gates both on ClutchState::Complete = mutual), │     LanPeerDiscovered { handle_proof, local_ip, port } } │   struct PingRequest { peer_addr, peer_pubkey } │   struct MessageRequest { peer_addr, recipient_pubkey, conversation_token, │     prev_msg_hp, ciphertext, eagle_time } │   struct AckRequest { peer_addr, recipient_pubkey, conversation_token, │     acked_eagle_time, plaintext_hash } │   struct PTSendRequest { peer_addr, data } │   struct ClutchOfferRequest, ClutchKemResponseRequest, │     ClutchCompleteRequest, LanBroadcastRequest, ClearPtSendsRequest │ ├── tcp.rs ── TCP fallback for large payloads │   send(stream, data), recv(stream) │ └── udp.rs ── UDP socket utilities async send(socket, data, addr), send_sync(socket, data, addr) log_received(data, addr) get_local_ip(), get_broadcast_addr()
//
// platform/ ├── mod.rs ── platform detection └── jni_android.rs ── Android JNI bridge
//
// storage/ ── the flat vault layer lives in the kete crate (FlatStorage, re-exported here); conversation content (rows/blobs) in the rarangi crate. Vault entries are addressed by a flat 32-byte key, never a path: vault_key(domain, scope) = blake3_kdf("photon.storage.entry.v0", domain || scope), where domain is a plain word ("avatar", "state", "chains", ...) and scope is the 32-byte identity the entry is about (our vault_seed for self/global, a peer seed for per-peer, a friendship_id for per-conversation). No hex, no base64, no tree paths anywhere except the one vault-root .vsf filename. │ ├── mod.rs ── kete re-exports (FlatStorage, StorageError, encrypt/decrypt_bytes, App, APP) + raw-file helpers │   vault_key(domain, scope) ── canonical 32-byte vault address for an entry │   write_file/read_file(path, label) ── raw atomic file I/O (now only inspect.rs diagnostics use it; avatars moved into the vault) │   photon_config_dir() │ ├── cloud.rs ── FGTW cloud backup (contacts sync) │   struct CloudContact, enum CloudError │   contacts_storage_key(identity_seed, device_secret) ── FGTW network locator (base64url, on the wire only) │   contacts_encryption_key(identity_seed, device_secret) │ ├── contacts.rs ── contact + conversation storage via FlatStorage (byte-addressed) │   struct ContactIdentity │   derive_identity_seed(handle) │   save/load_contact_list(storage) ── index at vault_key("contacts", storage.vault_seed()) │   save/load_contact_state, save/load_all_contacts(storage) ── per-peer at vault_key(domain, their_seed) │   save/load_messages(contact, storage) ── conversation CONTENT, now rarangi rows: table = friendship_id bytes (derived early from sorted participant seeds; self/1:1/group/fleet), pk = the message's eagle_time as u64 (monotonic clock → key order == chronological; doubles as the braid's weave reference); row also stores content_hash + ack_hash (re-ACK lost-ACK heal) │   save/load/delete_clutch_keypairs(their_identity_seed, storage), save/load/delete_clutch_slots(slots, their_identity_seed, storage) ── seed-keyed, never the plaintext handle (identity flows as the seed past the contact boundary) │ ├── friendship.rs ── per-friendship chain STATE (the ratchet machinery, not content) via FlatStorage at vault_key("chains", friendship_id) save/load_friendship_chains(chains/id, storage) load_all_friendships(friendship_ids, storage) delete_friendship_chains(friendship_id, storage) │ └── settings.rs ── user-adjustable app settings, plain VSF at photon_config_dir()/settings.vsf (non-secret, NOT the vault) │   struct Settings { hex_head, hex_tail } ── log hex-elision lengths │     ::load_or_create() (self-creates with defaults), ::apply() (pushes to vsf::inspect::set_hex_elision unless VSF_HEX_HEAD/TAIL env override)
// NB: kete::FlatStorage gained byte-addressed write_addr/read_addr/delete_addr(&[u8;32]) — used by all photon storage above — alongside the string write/read/delete still used by rarangi (whose table/pk strings kete hashes to addresses internally).
//
// types/ ├── contact.rs ── contact/friendship re-exports │   struct PartySlot { handle_hash, offer, kem_secrets_from/to_them, ... } │     ::new(handle_hash), is_complete() │   struct ChatMessage { content, timestamp, is_outgoing, delivered, ack_hash } (ack_hash = plaintext_hash we ACK a received msg with, persisted so a duplicate retransmit re-ACKs — lost-ACK heal) │     ::new(content, is_outgoing), new_with_timestamp() │   struct HandleText(String) ::new(s), as_str() │   struct ContactId([u8;32]) ::from_pubkey(), from_bytes(), as_bytes() │   enum ClutchState { Pending, AwaitingProof, Complete } │   enum TrustLevel { Stranger, Known, Trusted, Inner } │   struct Contact { │     id, handle, handle_proof, handle_hash, public_identity, │     ip, local_ip, local_port, │     relationship_seed, friendship_id, clutch_state, │     clutch_our_keypairs, clutch_slots, ceremony_id, │     clutch_pending_kem, clutch_offer_sent, clutch_*_proof, │     clutch_*_in_progress, completed_their_hqc_prefix, │     offer_provenances, trust_level, added, last_seen, │     is_online, messages, message_scroll_offset, │     avatar_pixels, avatar_scaled, avatar_scaled_diameter } │     ::new(), with_ip(), with_seed(), with_trust_level() │     ::update_last_seen(), best_addr(), can_be_custodian() │     ::get_ceremony_id(), init_clutch_slots() │     ::get_slot_index(), get_slot_mut(), get_slot() │     ::all_slots_complete(), insert_message_sorted() │ ├── device.rs ── device identity │   struct DevicePubkey │   ed25519_secret_to_x25519(ed_secret) │ ├── friendship.rs ── friendship and ceremony types │   struct CeremonyId([u8;32]) │     ::derive_base(handle_hashes), derive(handle_hashes, provenances) │     ::from_bytes(), as_bytes() │   struct FriendshipId([u8;32]) │     ::derive(handle_hashes), from_bytes(), as_bytes() │     ::to_base64(), from_base64() │   struct FriendshipChains { friendship_id, conversation_token, │     chains, participants } │ ├── handle.rs ── handle type │   struct Handle { text, key } │     ::new(), to_handle_proof(), username_to_handle_proof() │ ├── message.rs ── message structures │   struct MessageId([u8;32]) ::new(), as_bytes(), to_vsf(), from_vsf() │   struct Message { nonce, sequence, payload, timestamp } │     ::new(), to_vsf_bytes(), from_vsf_bytes() │   struct EncryptedMessage { sequence, ciphertext } │     ::to_vsf_bytes(), from_vsf_bytes() │   enum MessageStatus { Pending, Sent, Delivered, Read, Failed } │     ::to_vsf(), from_vsf() │ ├── peer.rs ── peer connection state │   struct Peer { public_identity, address, last_seen, connection_state } │     ::new(), update_connection_state(), is_online() │   enum ConnectionState { Disconnected, Connecting, Connected, Authenticated } │   struct DhtAnnouncement { public_key, port, timestamp, signature } │ ├── seed.rs ── cryptographic seed │   struct Seed([u8;32]) │ └── shard.rs ── key shard structures struct KeyShard, ShardId([u8;16]), DecryptedShard struct RecoveryRequest, RecoveryApproval, ShardDistribution
//
// ui/ ├── app.rs ── application state machine │   struct PhotonApp (main app state — see text_editing.rs for text methods) │   struct TextState { chars, widths, width, blinkey_index, │     scroll_offset, selection_anchor, textbox_focused, is_empty } │     ::new(), insert(), insert_str(), remove(), delete_range() │   struct TextLayout { center_x/y, box_width/height, usable_*, │     margin, font_size, line_height, button_area } │     ::new(w, h, span, ru, app_state) │     ::blinkey_x(text), blinkey_y(), text_start_x(text) │   struct PixelRegion { x, y, w, h } │     ::new(), from_signed(), contains(), right(), bottom(), center() │   struct Layout { logo_spectrum, photon_text, textbox, │     attest_block, contacts, header, message_area } │   struct AttestBlockLayout { error, textbox, hint, attest } │     ::new(block) │   struct ContactsHeaderLayout { avatar, handle, hint, textbox, separator } │     ::new(block), avatar_center_radius() │   struct ContactsRowsLayout { rows, row_height, avatar_diameter, ... } │     ::new(), row_region(), row_avatar_center() │     ::row_text_position(), visible_row_count() │   struct ContactsUnifiedLayout { user_avatar, handle, hint, ... } │   struct ClutchKeygenResult { contact_id, keypairs } │   struct ClutchKemEncapResult { contact_id, kem_response, local_secrets, ... } │   struct ClutchCeremonyResult { contact_id, friendship_chains, │     eggs_proof, their_handle_hash, ceremony_id, ... } │   soft_limit(x, max) │ ├── avatar.rs ── avatar encoding/upload/download │   AVATAR_SIZE, encode_avatar_from_image(image_data) │ ├── colour.rs ── colour utilities │ ├── compositing.rs ── compositing pipeline, layout calculation, rendering │ ├── display_profile.rs ── ICC colour profile conversion │   struct DisplayConverter { xyz_to_display, r/g/b_trc } │     ::new(), convert_avatar(vsf_rgb) │ ├── drawing.rs ── primitive drawing (circles, lines, fills) │ ├── keyboard.rs ── input handling (key events → app state changes) │ ├── mouse.rs ── mouse/touch input │ ├── renderer_*.rs ── platform-specific renderers (7 backends) │   android, linux_softbuffer, linux_wgpu, macos, │   macos_softbuffer, redox, windows │ ├── text_editing.rs ── text input methods on PhotonApp │   ::textbox_is_focused(), font_size(), textbox_width/height() │   ::textbox_center_y(), textbox_left/right() │   ::recalculate_char_widths(), render_text_clipped() │   ::update_text_scroll(), update_selection_scroll() │   ::get_selection_range(), delete_selection(), get_selected_text() │   ::paste_text(), handle_blinkey_left(), next_blink_wake_time() │   ::start/stop/undraw/draw/flip_blinkey() │   ::add/subtract_blinkey_top/bottom(), invert_selection() │ ├── text_rasterizing.rs ── font rendering (cosmic-text) │ └── theme.rs ── colour palette
//
// bin/ ├── photon-keygen.rs ── signing key generation ├── photon-signature-signer.rs ── binary signing tool └── test-device-key.rs ── device key diagnostic

// Global debug flag - can be toggled at runtime with Ctrl+D
use std::sync::atomic::AtomicBool;
pub static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);

/// Photon network ports - used for ALL network communication UDP: peer-to-peer status pings, CLUTCH ceremony, chat messages TCP: large payloads (full CLUTCH offers ~548KB, KEM responses ~17KB) FGTW: handle registration and peer discovery announcements Primary: 4383, Fallback: 3546 (both IANA unassigned)
pub const PHOTON_PORT: u16 = 4383;
pub const PHOTON_PORT_FALLBACK: u16 = 3546;

/// Multicast port for LAN peer discovery Separate from main port to avoid SO_REUSEADDR complexity 4384 is IANA unassigned
pub const MULTICAST_PORT: u16 = 4384;

/// Eagle Time: oscillations per second (hydrogen hyperfine transition)
pub const OSC_PER_SEC: i64 = vsf::OSCILLATIONS_PER_SECOND as i64;

/// Peer expiry: 7 days
pub const PEER_EXPIRY_OSC: i64 = 604_800 * OSC_PER_SEC;

/// K-bucket stale entry eviction: 1 hour
pub const KBUCKET_STALE_OSC: i64 = 3_600 * OSC_PER_SEC;

// Debug print macro - only prints if DEBUG_ENABLED is true Compiled out entirely in release builds
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

/// Severity of a structured log record. The discriminant IS the on-disk `lvl` value in the VSF log, so
/// these numbers are wire-stable — append new levels at the end, never renumber.
#[derive(Clone, Copy)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
}

/// Retained for the desktop/Windows `main()` call site. The VSF file sink now opens LAZILY on the first log
/// after the platform data dir is known (Android sets it partway through JNI startup), so this is a no-op —
/// kept only so existing callers compile.
pub fn init_logging() {}

// Disabled: compiles to nothing without --features logging.
#[cfg(not(feature = "logging"))]
#[inline(always)]
pub fn log(_msg: &str) {}
#[cfg(not(feature = "logging"))]
#[inline(always)]
pub fn log_at(_level: LogLevel, _msg: &str) {}

// The structured VSF log sink: one COMPLETE VSF record per line — {creation_time (Eagle), section "log"
// {lvl, msg}} — appended to `<photon_config_dir>/photon.log.vsf` on EVERY platform (Android: app filesDir,
// pullable via `adb pull`; desktop/Windows: the config dir). The log is thus a stream of self-describing,
// Eagle-time-stamped, vsfinfo-inspectable records; read it with the `photonlog` bin. Opens lazily and RETRIES
// until the dir is ready — a plain Mutex<Option<File>>, NOT a OnceLock, precisely so a pre-data-dir failure
// isn't cached forever (the first few JNI lines predate Android's data_dir and land in the console sink only).
// Known filename (logging is a dev-build feature, so adb-pull discoverability beats filename privacy).
#[cfg(feature = "logging")]
static LOG_FILE: std::sync::Mutex<Option<std::fs::File>> = std::sync::Mutex::new(None);

/// Android-only override for where the VSF log goes — set from JNI to the EXTERNAL files dir (the shadow
/// ring dir), which is adb-readable on a non-debuggable release dev APK where internal `files/` is not.
#[cfg(all(feature = "logging", target_os = "android"))]
static ANDROID_LOG_DIR: std::sync::OnceLock<String> = std::sync::OnceLock::new();
#[cfg(all(feature = "logging", target_os = "android"))]
pub fn set_android_log_dir(dir: String) {
    if !dir.is_empty() {
        let _ = ANDROID_LOG_DIR.set(dir);
    }
}

/// Directory the VSF log file lives in. Android prefers the JNI-set external dir (pullable); everything else
/// uses `photon_config_dir`.
#[cfg(feature = "logging")]
fn log_dir() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "android")]
    if let Some(d) = ANDROID_LOG_DIR.get() {
        return Some(std::path::PathBuf::from(d));
    }
    crate::storage::photon_config_dir().ok()
}

#[cfg(feature = "logging")]
fn append_log_record(level: LogLevel, msg: &str) {
    use std::io::Write;
    let Ok(mut guard) = LOG_FILE.lock() else {
        return;
    };
    if guard.is_none() {
        if let Some(dir) = log_dir() {
            let _ = std::fs::create_dir_all(&dir);
            if let Ok(f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("photon.log.vsf"))
            {
                *guard = Some(f);
            }
        }
    }
    let Some(file) = guard.as_mut() else {
        return;
    };
    let record = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_section(
            "log",
            vec![
                ("lvl".to_string(), vsf::VsfType::u(level as usize, false)),
                ("msg".to_string(), vsf::VsfType::x(msg.to_string())),
            ],
        )
        .build();
    if let Ok(bytes) = record {
        let _ = file.write_all(&bytes);
        let _ = file.flush();
    }
}

// Enabled: structured VSF file record + the platform's live console sink (logcat / stdout).
#[cfg(feature = "logging")]
pub fn log(msg: &str) {
    log_at(LogLevel::Info, msg);
}

#[cfg(feature = "logging")]
pub fn log_at(level: LogLevel, msg: &str) {
    append_log_record(level, msg);

    // Live console sink (unchanged behaviour): Android → logcat at the matching level, others → stdout.
    // Windows had no console sink; its durable output is now the VSF file above.
    #[cfg(target_os = "android")]
    match level {
        LogLevel::Trace => log::trace!("{}", msg),
        LogLevel::Debug => log::debug!("{}", msg),
        LogLevel::Info => log::info!("{}", msg),
        LogLevel::Warn => log::warn!("{}", msg),
        LogLevel::Error => log::error!("{}", msg),
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
    // Initialize Android logger with module filtering Filter out noisy cosmic_text and reqwest debug logs
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

    // Hand tohu the JavaVM so its device oracle can read Settings.Secure.ANDROID_ID itself (via ActivityThread.currentApplication()). Done here because JNI_OnLoad is where the vm is handed to us; the actual fetch happens later, once the Application exists.
    tohu::device::android_init(vm);

    log::info!("Photon JNI loaded (PID: {})", std::process::id());
    jni::sys::JNI_VERSION_1_6
}
