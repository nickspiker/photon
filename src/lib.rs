// PHOTON SOURCE MAP — keep updated when pub items or files change
//
// lib.rs ── constants, logging (always-on VSF sink, 16 MiB + jittered 24–48h caps, name-scrubbed), debug macro, module re-exports PHOTON_PORT=4383, PHOTON_PORT_FALLBACK=3546, MULTICAST_PORT=4384 OSC_PER_SEC, PEER_EXPIRY_OSC (7 days), KBUCKET_STALE_OSC (1 hour) init_logging(), log(), log_at(), clear_log() (wipe the log), install_log_bridge() (`log` crate → VSF sink; logcat/env_logger retired), fp(public_id) (non-PII log label), jitter(i64)/jitter_dur(Duration) (stochastic 50–100% pad for any timer/threshold — anti-thundering-herd)
//
// main.rs ── winit event loop, window creation, tokio async runtime
//
// crypto/ ├── chain.rs ── the braid: rolling-chain encryption (512-link, 16KB; see BRAID.md) │   struct Chain { links: [[u8;32]; 512], last_ack_time: Option<EagleTime> } │     ::from_bytes(), to_bytes(), from_full_bytes() │     ::current_key(), link(idx), links() │     ::advance(eagle_time, our_plaintext, their_plaintexts) — the braid: weaves up to two prior peer plaintexts (length-prefixed, peer-entropy-first) │   enum ChainError { DecryptionFailed, InvalidMessage, NotInitialized, │     EncryptionFailed(String), SignatureInvalid, InvalidEncoding, │     AckMismatch, ParticipantNotFound } │   CHAIN_LINKS=512, HISTORY_LINKS=256, ACTIVE_LINKS=256 │   LINK_SIZE=32, CHAIN_SIZE=16384, CURRENT_KEY_INDEX=511 │   L1_SIZE=30720, L1_ROUNDS=3 │   derive_salt(prev_plaintext, chain) │   generate_scratch(chain, salt), generate_scratch_at_offset(chain, salt, off) │   generate_confirmation_smear(message, chain) │   generate_ack_proof(eagle_time, plaintext_hash, chain) │   verify_ack_proof(eagle_time, plaintext_hash, chain, received_proof) │   derive_nonce(eagle_time), encrypt_layers(), decrypt_layers() │ ├── clutch.rs ── 8-algorithm parallel key ceremony │   smear_hash(data), derive_conversation_token(participant_seeds) │   derive_ceremony_instance(offers), spaghettify(input) │ ├── handle_proof.rs ── memory-hard handle attestation (~1s) │   handle_proof(hash) │ ├── keys.rs ── identity key management (TODO) │ ├── self_verify.rs ── Ed25519 binary signature verification │   AUTHOR_PUBKEY, SYSTEM_PUBKEYS │   is_system_pubkey(pubkey), verify_binary_hash() │ └── shards.rs ── social recovery key sharding (TODO)
//
// network/ ├── fgtw/ ── Fractal Gradient Trust Web (Kademlia DHT) │   ├── blob.rs ── binary blob storage/retrieval │   │ │   ├── bootstrap.rs ── initial peer discovery │   │   load_bootstrap_peers() │   │ │   ├── fingerprint.rs ── device fingerprint derivation │   │ │   ├── node.rs ── Kademlia routing table, k-buckets │   │ │   ├── peer_store.rs ── peer caching │   │   struct PeerStore │   │ │   ├── protocol.rs ── VSF-encoded FGTW + CLUTCH messages │   │   enum FgtwMessage { ... + PhonebookRequest/Response (peers-are-FGTW gossip) + AvatarRequest/Response (av_req/av_resp — direct P2P avatar exchange between mutual contacts, signed over the avatar bytes' hash) } │   │   struct PeerRecord { handle_proof, device_pubkey, ip, local_ip, last_seen, signature } (self-signed: ::sign/::verify via the Ed25519 device_pubkey, so gossiped records are relay-independent), SyncRecord │   │ │   ├── fleet.rs ── multi-device fleet membership blob (v1 keyring): a network-held, signed, hash-chained device-pubkey set. FleetOp { handle_proof, prev_hash, kind(Genesis/Add/Remove), device_pubkey, eagle_time, signer_pubkey, sigs(egg-list) }; MembershipBlob::{genesis, add, remove, fold, extends, is_member, to_vsf_bytes, from_vsf_bytes}. Each op is Ed25519-signed (egg-list, PQ-additive) and identity-bound; fold() IS the authorisation rule (valid sig + hash chain + signer-was-a-prior-member). Client oracle (stateless, fetch-then-sign): fetch(), publish(), ensure_member(). Verifier mirror lives in fgtw/src/fleet.rs (kept in lockstep via a known-answer test) │   └── relay.rs ── relay node logic │ ├── clock_check.rs ── one-shot wall-clock sanity check via nunc-time consensus (desktop-only; warn-only — never corrects the clock) │   spawn_clock_check(tx, wake) ── off-thread nunc::query, posts ClockCheckResult │   struct ClockJumpDetector ── monotonic-vs-wall jump trigger for mid-session re-checks │   enum ClockCheckResult { Ok { offset_secs, confidence_secs, sources_used/queried }, Unavailable } │ ├── handle_query.rs ── handle attestation and lookup │   struct HandleQuery │     ::new(keypair, event_proxy), query(handle), query_resume(session), try_recv() │     ::try_recv_online(), search(handle), try_recv_search() │     ::set_handle_proof(proof), get_handle_proof() │     ::set_transport(), get_transport(), port(), socket() │   enum QueryRequest { FirstAttest(String), Resume(tohu::SessionIdentity) } │   enum QueryResult { Success(AttestationData), AlreadyAttested, Error } │   struct AttestationData { handle_proof, identity_seed, │     contacts, friendships, avatar_pixels, peers } │ ├── http.rs ── shared pooled HTTP for FGTW │   runtime() (one persistent tokio) · async_client() · blocking() — reused reqwest clients keep TLS warm │ ├── inspect.rs ── network diagnostic utilities, VSF disk I/O │   vsf_write(path, encrypted, label, decrypted, device_secret) │   vsf_read(path, label, device_secret) │ ├── peer_updates.rs ── peer state change notifications │   struct PeerUpdate, PeerUpdateClient │ ├── pt/ ── Photon Transfer (large message transport) │   ├── buffer.rs ── reassembly buffer │   ├── packets.rs ── packet framing │   │   struct PTSpec │   ├── state.rs ── transfer state machine │   │   enum Direction, TransferState │   │   enum PTError, struct OutboundTransfer │   └── window.rs ── sliding window flow control │   struct PTManager { outbound, inbound, keypair, next_stream_id } │     SINGLE_PACKET_MAX=1024 │     ::new(keypair), send(addr, data), send_with_pubkey() │     ::handle_spec(), handle_spec_ack(), handle_data(), handle_ack() │   struct RelayInfo { recipient_pubkey, payload } │   struct TickSend { peer_addr, wire_bytes, tcp_payload, relay } │ ├── status.rs ── P2P ping/pong, CLUTCH orchestration │   struct StatusChecker │     ::new(socket, keypair, contacts, sync_records, event_proxy) │   enum StatusUpdate { │     Online { peer_pubkey, is_online, peer_addr, sync_records }, │     ChatMessage { conversation_token, prev_msg_hp, ciphertext, ... }, │     MessageAck { conversation_token, acked_eagle_time, plaintext_hash }, │     PTReceived { peer_addr, data }, │     PTSendComplete { peer_addr }, │     ClutchOfferReceived { conversation_token, offer_provenance, ... }, │     ClutchKemResponseReceived { ... }, │     ClutchCompleteReceived { ... }, │     AvatarRequestReceived { sender_pubkey, sender_addr }, AvatarReceived { responder_pubkey, avatar_vsf, sender_addr } (P2P avatar exchange; UI gates both on ClutchState::Complete = mutual), │     LanPeerDiscovered { handle_proof, local_ip, port } } │   struct PingRequest { peer_addr, peer_pubkey } │   struct MessageRequest { peer_addr, recipient_pubkey, conversation_token, │     prev_msg_hp, ciphertext, eagle_time } │   struct AckRequest { peer_addr, recipient_pubkey, conversation_token, │     acked_eagle_time, plaintext_hash } │   struct PTSendRequest { peer_addr, data } │   struct ClutchOfferRequest, ClutchKemResponseRequest, │     ClutchCompleteRequest, LanBroadcastRequest, ClearPtSendsRequest │ ├── tcp.rs ── TCP fallback for large payloads │   send(stream, data), recv(stream) │ └── udp.rs ── UDP socket utilities async send(socket, data, addr), send_sync(socket, data, addr) log_received(data, addr) get_local_ip(), get_broadcast_addr()
//
// platform/ ├── mod.rs ── platform detection └── jni_android.rs ── Android JNI bridge
//
// storage/ ── the flat vault layer lives in the kete crate (FlatStorage, re-exported here); conversation content (rows/blobs) in the rarangi crate. Vault entries are addressed by a flat 32-byte key, never a path: vault_key(domain, scope) = blake3_kdf("photon.storage.entry.v0", domain || scope), where domain is a plain word ("avatar", "state", "chains", ...) and scope is the 32-byte identity the entry is about (our vault_seed for self/global, a peer seed for per-peer, a friendship_id for per-conversation). No hex, no base64, no tree paths anywhere except the one vault-root .vsf filename. │ ├── mod.rs ── kete re-exports (FlatStorage, StorageError, encrypt/decrypt_bytes, App, APP) + raw-file helpers │   vault_key(domain, scope) ── canonical 32-byte vault address for an entry │   write_file/read_file(path, label) ── raw atomic file I/O (now only inspect.rs diagnostics use it; avatars moved into the vault) │   photon_config_dir() │ ├── cloud.rs ── FGTW cloud backup (contacts sync) │   struct CloudContact, enum CloudError │   contacts_storage_key(identity_seed, device_secret) ── FGTW network locator (base64url, on the wire only) │   contacts_encryption_key(identity_seed, device_secret) │ ├── contacts.rs ── contact + conversation storage via FlatStorage (byte-addressed) │   struct ContactIdentity │   derive_identity_seed(handle) │   save/load_contact_list(storage) ── index at vault_key("contacts", storage.vault_seed()) │   save/load_contact_state, save/load_all_contacts(storage) ── per-peer at vault_key(domain, their_seed) │   save/load_messages(contact, storage) ── conversation CONTENT, now rarangi rows: table = friendship_id bytes (derived early from sorted participant seeds; self/1:1/group/fleet), pk = the message's eagle_time as u64 (monotonic clock → key order == chronological; doubles as the braid's weave reference); row also stores content_hash + ack_hash (re-ACK lost-ACK heal) │   save/load/delete_clutch_keypairs(their_identity_seed, storage), save/load/delete_clutch_slots(slots, their_identity_seed, storage) ── seed-keyed, never the plaintext handle (identity flows as the seed past the contact boundary) │ ├── friendship.rs ── per-friendship chain STATE (the ratchet machinery, not content) via FlatStorage at vault_key("chains", friendship_id) save/load_friendship_chains(chains/id, storage) load_all_friendships(friendship_ids, storage) delete_friendship_chains(friendship_id, storage) │ └── settings.rs ── user-adjustable app settings, plain VSF at photon_config_dir()/settings.vsf (non-secret, NOT the vault) │   struct Settings { hex_head, hex_tail } ── log hex-elision lengths │     ::load_or_create() (self-creates with defaults), ::apply() (pushes to vsf::inspect::set_hex_elision unless VSF_HEX_HEAD/TAIL env override) NB: kete::FlatStorage gained byte-addressed write_addr/read_addr/delete_addr(&[u8;32]) — used by all photon storage above — alongside the string write/read/delete still used by rarangi (whose table/pk strings kete hashes to addresses internally).
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

/// Severity of a structured log record. The discriminant IS the on-disk `lvl` value in the VSF log, so these numbers are wire-stable — append new levels at the end, never renumber.
#[derive(Clone, Copy)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
}

/// Retained for the desktop/Windows `main()` call site. The VSF file sink now opens LAZILY on the first log after the platform data dir is known (Android sets it partway through JNI startup), so this is a no-op — kept only so existing callers compile.
pub fn init_logging() {}

/// Short, non-PII label for logs: the first 4 bytes of a PUBLIC id (handle_proof / device pubkey) as hex.
/// Log this instead of a plaintext handle — the durable log then carries pseudonymous identifiers, never names, so it stays diagnostic (you can correlate a fingerprint across a run) without leaking who anyone is.
pub fn fp(public_id: &[u8]) -> String {
    hex::encode(&public_id[..public_id.len().min(4)])
}

/// Stochastic pad for ANY periodic timer or age threshold: `base` scaled by a fresh random factor in [0.5, 1.0].
/// Re-roll on every use. A fixed interval makes every client (and every subsystem) wake on the same tick — a routine timer becomes a synchronised network cascade (the thundering herd), e.g. everyone re-announcing exactly on the hour. Jittering each period spreads the load and makes accidental alignment vanishingly unlikely; the cost is a fuzzy deadline, which time-based housekeeping never needs exact.
pub fn jitter(base: i64) -> i64 {
    (base as f64 * (0.5 + rand::random::<f64>() * 0.5)) as i64
}

/// [`jitter`] for `std::time::Duration` timers (sleeps, recv-timeouts, periodic loops).
pub fn jitter_dur(base: std::time::Duration) -> std::time::Duration {
    base.mul_f64(0.5 + rand::random::<f64>() * 0.5)
}

// Disabled: compiles to nothing without --features logging.
#[cfg(not(feature = "logging"))]
#[inline(always)]
pub fn log(_msg: &str) {}
#[cfg(not(feature = "logging"))]
#[inline(always)]
pub fn log_at(_level: LogLevel, _msg: &str) {}
#[cfg(not(feature = "logging"))]
#[inline(always)]
pub fn clear_log() {}

// The structured VSF log sink: one COMPLETE VSF record per line — {creation_time (Eagle), section "log" {lvl, msg}} — appended to `<photon_config_dir>/photon.log.vsf` on EVERY platform (Android: app filesDir, pullable via `adb pull`; desktop/Windows: the config dir). The log is thus a stream of self-describing, Eagle-time-stamped, vsfinfo-inspectable records; read it with the `photonlog` bin. Opens lazily and RETRIES until the dir is ready — a plain Mutex<Option<File>>, NOT a OnceLock, precisely so a pre-data-dir failure isn't cached forever (the first few JNI lines predate Android's data_dir and land in the console sink only).
// Known filename (logging is a dev-build feature, so adb-pull discoverability beats filename privacy).
#[cfg(feature = "logging")]
static LOG_FILE: std::sync::Mutex<Option<std::fs::File>> = std::sync::Mutex::new(None);

// Size cap for the VSF log: once the file passes 16 MiB, drop enough of the OLDEST whole records to bring it back to ~8 MiB.
// Trimming cuts only on record boundaries (the file is a stream of complete VSF records), so the result stays fully decodable by photonlog.
#[cfg(feature = "logging")]
const LOG_CAP_BYTES: u64 = 16 << 20;
#[cfg(feature = "logging")]
const LOG_TRIM_TO_BYTES: u64 = 8 << 20;
// Live byte count of the open log file, so the cap check is a cheap atomic load instead of a stat per line; seeded from the file's size when it's first opened.
#[cfg(feature = "logging")]
static LOG_BYTES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
// Age cap with hysteresis + jitter: let the log reach ~2 days, THEN cut it back to ~the most recent 24h.
// Hysteresis avoids "persistent scrubbing" (a trim-at-exactly-24h rewrites the file on nearly every line of a steady low-volume log). Jitter avoids synchronised cascades: the trigger fires at a random 24–48h and the keep window is a random 12–24h, re-rolled each trim, so no two devices (and no two of our subsystems) trim on the same instant.
#[cfg(feature = "logging")]
const LOG_AGE_TRIGGER_BASE_OSC: i64 = 2 * 24 * 60 * 60 * vsf::OSCILLATIONS_PER_SECOND as i64; // jittered → 24–48h
#[cfg(feature = "logging")]
const LOG_AGE_KEEP_BASE_OSC: i64 = 24 * 60 * 60 * vsf::OSCILLATIONS_PER_SECOND as i64; // jittered → 12–24h
// The currently-chosen (jittered) trigger threshold; re-rolled on open and after each trim.
#[cfg(feature = "logging")]
static LOG_AGE_TRIGGER_OSC: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(LOG_AGE_TRIGGER_BASE_OSC);
// Eagle-time of the OLDEST record in the open file, cached so the age check is a compare not a head-read per line; i64::MAX = unknown/empty (seeded on open, refreshed on trim).
#[cfg(feature = "logging")]
static LOG_OLDEST_OSC: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(i64::MAX);

/// Android-only override for where the VSF log goes — set from JNI to the EXTERNAL files dir (the shadow ring dir), which is adb-readable on a non-debuggable release dev APK where internal `files/` is not.
#[cfg(all(feature = "logging", target_os = "android"))]
static ANDROID_LOG_DIR: std::sync::OnceLock<String> = std::sync::OnceLock::new();
#[cfg(all(feature = "logging", target_os = "android"))]
pub fn set_android_log_dir(dir: String) {
    if !dir.is_empty() {
        let _ = ANDROID_LOG_DIR.set(dir);
    }
}

/// Directory the VSF log file lives in. Android prefers the JNI-set external dir (pullable); everything else uses `photon_config_dir`.
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
            let path = dir.join("photon.log.vsf");
            if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                let sz = f.metadata().map(|m| m.len()).unwrap_or(0);
                LOG_BYTES.store(sz, std::sync::atomic::Ordering::Relaxed);
                LOG_OLDEST_OSC.store(
                    first_record_osc(&path).unwrap_or(i64::MAX),
                    std::sync::atomic::Ordering::Relaxed,
                );
                LOG_AGE_TRIGGER_OSC.store(
                    jitter(LOG_AGE_TRIGGER_BASE_OSC),
                    std::sync::atomic::Ordering::Relaxed,
                );
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
        let total = LOG_BYTES.fetch_add(bytes.len() as u64, std::sync::atomic::Ordering::Relaxed)
            + bytes.len() as u64;
        // Trim on EITHER cap: too big (16 MiB) or the oldest record past the jittered age trigger (24–48h).
        let now = vsf::eagle_time_oscillations();
        let oldest = LOG_OLDEST_OSC.load(std::sync::atomic::Ordering::Relaxed);
        let trigger = LOG_AGE_TRIGGER_OSC.load(std::sync::atomic::Ordering::Relaxed);
        let aged = oldest != i64::MAX && now.saturating_sub(oldest) > trigger;
        if total > LOG_CAP_BYTES || aged {
            // `file`'s borrow of `guard` ends above; reopen the handle on the trimmed file.
            if let Some((trimmed, new_size, new_oldest)) = trim_log_file(now) {
                *guard = Some(trimmed);
                LOG_BYTES.store(new_size, std::sync::atomic::Ordering::Relaxed);
                LOG_OLDEST_OSC.store(new_oldest, std::sync::atomic::Ordering::Relaxed);
                // Re-roll the next age trigger so successive trims never settle into a fixed cadence.
                LOG_AGE_TRIGGER_OSC.store(
                    jitter(LOG_AGE_TRIGGER_BASE_OSC),
                    std::sync::atomic::Ordering::Relaxed,
                );
            }
        }
    }
}

// Trim the log by dropping the oldest whole records — enough to get under `LOG_TRIM_TO_BYTES` AND to drop anything older than 24h — then reopen it for appending.
// The file is a stream of complete VSF records, so we cut only on record boundaries (never mid-record). Returns the reopened append handle, the kept byte count, and the new oldest-record time; None if the file couldn't be read/rewritten (the cap check just retries next line).
#[cfg(feature = "logging")]
fn trim_log_file(now_osc: i64) -> Option<(std::fs::File, u64, i64)> {
    use std::io::Write;
    let path = log_dir()?.join("photon.log.vsf");

    // Defence in depth (unix): hold an advisory lock across the read-truncate-rewrite, so even two processes sharing one log — which the single-instance lock already forbids — can't interleave a trim and clobber each other. LOCK_NB: if another process is mid-trim, skip and retry on the next line.
    #[cfg(unix)]
    let _trim_lock = {
        use std::os::unix::io::AsRawFd;
        let lf = std::fs::OpenOptions::new().create(true).write(true).open(&path).ok()?;
        if unsafe { libc::flock(lf.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return None;
        }
        lf // held until this fn returns (drop closes the fd → releases the lock)
    };

    let bytes = std::fs::read(&path).ok()?;
    let age_cutoff = now_osc.saturating_sub(jitter(LOG_AGE_KEEP_BASE_OSC)); // keep a random 12–24h
    let (keep, new_oldest) = log_keep_offset(&bytes, LOG_TRIM_TO_BYTES, age_cutoff);
    let kept = &bytes[keep.min(bytes.len())..];
    let mut w = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
        .ok()?;
    w.write_all(kept).ok()?;
    w.flush().ok()?;
    drop(w);
    let appender = std::fs::OpenOptions::new().create(true).append(true).open(&path).ok()?;
    Some((appender, kept.len() as u64, new_oldest))
}

// Pure boundary finder: the first whole-record boundary to keep so that `bytes[offset..]` is both within `trim_to_size` bytes AND free of records older than `age_cutoff_osc`.
// Records are appended in time order, so we drop from the front while a record is EITHER before the size-drop point OR older than the cutoff, stopping at the first record that satisfies both.
// Returns (keep_offset, oldest_kept_time) — the second value re-seeds LOG_OLDEST_OSC. Stops early on any decode error so a corrupt tail never causes a mid-record cut.
#[cfg(feature = "logging")]
fn log_keep_offset(bytes: &[u8], trim_to_size: u64, age_cutoff_osc: i64) -> (usize, i64) {
    let total = bytes.len();
    let size_drop = (total as u64).saturating_sub(trim_to_size) as usize;
    let mut offset = 0usize;
    while offset < total {
        let rest = &bytes[offset..];
        let (header, header_end) = match vsf::file_format::VsfHeader::decode(rest) {
            Ok(h) => h,
            Err(_) => return (offset, i64::MAX),
        };
        let mut ptr = 0usize;
        if vsf::file_format::VsfSection::parse(&rest[header_end..], &mut ptr).is_err() {
            return (offset, i64::MAX);
        }
        let rec = header_end + ptr;
        if rec == 0 {
            return (offset, i64::MAX);
        }
        let t = match &header.creation_time {
            Some(vsf::VsfType::e(et)) => et_to_osc_log(et),
            _ => i64::MIN, // no/odd timestamp → treat as ancient so it's eligible to drop
        };
        // Keep from the first record that is past the size-drop point AND fresh enough.
        if offset >= size_drop && t >= age_cutoff_osc {
            return (offset, t);
        }
        offset += rec;
    }
    (total, i64::MAX) // everything dropped → empty
}

// Eagle oscillations from a log record's creation-time field.
#[cfg(feature = "logging")]
fn et_to_osc_log(et: &vsf::types::EtType) -> i64 {
    use vsf::types::EtType;
    match et {
        EtType::e5(o) => *o as i64,
        EtType::e6(o) => *o,
        EtType::e7(o) => *o as i64,
        _ => i64::MIN,
    }
}

// The oldest (first) record's eagle-time in a log file, by decoding just its header. None if empty/unreadable.
#[cfg(feature = "logging")]
fn first_record_osc(path: &std::path::Path) -> Option<i64> {
    use std::io::Read;
    let mut buf = vec![0u8; 4096];
    let n = std::fs::File::open(path).ok()?.read(&mut buf).ok()?;
    if n == 0 {
        return None;
    }
    let (header, _) = vsf::file_format::VsfHeader::decode(&buf[..n]).ok()?;
    match &header.creation_time {
        Some(vsf::VsfType::e(et)) => Some(et_to_osc_log(et)),
        _ => None,
    }
}

/// Wipe the durable log (the `[]x` clean-relaunch chord, and any future privacy "clear logs" action).
/// Removes `photon.log.vsf` and drops the open handle so the next write reopens a fresh, empty file.
#[cfg(feature = "logging")]
pub fn clear_log() {
    if let Ok(mut guard) = LOG_FILE.lock() {
        if let Some(dir) = log_dir() {
            let _ = std::fs::remove_file(dir.join("photon.log.vsf"));
        }
        *guard = None;
        LOG_BYTES.store(0, std::sync::atomic::Ordering::Relaxed);
        LOG_OLDEST_OSC.store(i64::MAX, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(all(test, feature = "logging"))]
mod log_cap_tests {
    use super::*;

    fn record(msg: &str) -> Vec<u8> {
        record_at(msg, vsf::eagle_time_oscillations())
    }

    fn record_at(msg: &str, osc: i64) -> Vec<u8> {
        vsf::VsfBuilder::new()
            .creation_time_oscillations(osc)
            .provenance_only()
            .add_section(
                "log",
                vec![
                    ("lvl".to_string(), vsf::VsfType::u(2, false)),
                    ("msg".to_string(), vsf::VsfType::x(msg.to_string())),
                ],
            )
            .build()
            .unwrap()
    }

    #[test]
    fn trim_cuts_on_a_record_boundary_and_stays_decodable() {
        // Build a stream of records and remember each record's start offset.
        let mut bytes = Vec::new();
        let mut starts = vec![0usize];
        for i in 0..50 {
            bytes.extend_from_slice(&record(&format!("message number {i}")));
            starts.push(bytes.len());
        }
        let trim_to = (bytes.len() / 3) as u64; // keep roughly the newest third
        let (keep, _oldest) = log_keep_offset(&bytes, trim_to, i64::MIN); // age disabled → size-only

        // The cut lands exactly on a record boundary...
        assert!(starts.contains(&keep), "cut at {keep} is not a record boundary");
        assert!(keep > 0, "should have dropped something");
        // ...the kept tail is no larger than the target (we keep from the FIRST boundary past the drop point)...
        let kept = &bytes[keep..];
        assert!(kept.len() as u64 <= trim_to && !kept.is_empty());
        // ...and decodes cleanly as whole records right up to EOF (no half record left at the front).
        let mut off = 0usize;
        let mut n = 0;
        while off < kept.len() {
            let (_, he) = vsf::file_format::VsfHeader::decode(&kept[off..]).unwrap();
            let mut p = 0usize;
            vsf::file_format::VsfSection::parse(&kept[off + he..], &mut p).unwrap();
            off += he + p;
            n += 1;
        }
        assert_eq!(off, kept.len(), "kept tail must end exactly on a record boundary");
        assert!(n > 0);
    }

    #[test]
    fn no_trim_when_under_target() {
        let bytes = record("solo"); // one record, well under the size cap
        let (keep, _oldest) = log_keep_offset(&bytes, 16 << 20, i64::MIN); // age disabled
        assert_eq!(keep, 0, "nothing dropped");
    }

    #[test]
    fn age_cap_drops_records_older_than_cutoff() {
        // Ten "old" records at t=1000, then ten "new" at t=9000. Size is generous; only age should trim.
        let mut bytes = Vec::new();
        for i in 0..10 {
            bytes.extend_from_slice(&record_at(&format!("old {i}"), 1000));
        }
        let new_start = bytes.len();
        for i in 0..10 {
            bytes.extend_from_slice(&record_at(&format!("new {i}"), 9000));
        }
        // Cutoff between the two cohorts: drop everything older than 5000.
        let (keep, oldest) = log_keep_offset(&bytes, 64 << 20, 5000);
        assert_eq!(keep, new_start, "should drop exactly the old cohort");
        assert_eq!(oldest, 9000, "oldest kept record is the first new one");
        // The kept tail is all the new records, decodes clean to EOF.
        let kept = &bytes[keep..];
        let mut off = 0;
        while off < kept.len() {
            let (_, he) = vsf::file_format::VsfHeader::decode(&kept[off..]).unwrap();
            let mut p = 0;
            vsf::file_format::VsfSection::parse(&kept[off + he..], &mut p).unwrap();
            off += he + p;
        }
        assert_eq!(off, kept.len());
    }
}

// The structured VSF file is the ONE durable log — read it live with `photonlog -f`, off a phone with `adb pull`.
// No console mirror: stdout/logcat was redundant noise once everything lands in photon.log.vsf.
#[cfg(feature = "logging")]
pub fn log(msg: &str) {
    log_at(LogLevel::Info, msg);
}

#[cfg(feature = "logging")]
pub fn log_at(level: LogLevel, msg: &str) {
    append_log_record(level, msg);
}

// Bridge the `log` crate into the VSF sink, so records from every dependency that uses log macros (fluor, tohu, the JNI platform layer, reqwest, ...) land in photon.log.vsf alongside `crate::log` lines — ONE durable, pullable, user-submittable log, no logcat/stdout fork.
// Mirrors the retired android_logger/env_logger setup: Debug and up globally, the known-noisy crates only at Warn+.
#[cfg(feature = "logging")]
struct VsfLogBridge;
#[cfg(feature = "logging")]
impl log::Log for VsfLogBridge {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        let noisy = meta.target().starts_with("cosmic_text") || meta.target().starts_with("reqwest");
        !noisy || meta.level() <= log::Level::Warn
    }
    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let lvl = match record.level() {
            log::Level::Error => LogLevel::Error,
            log::Level::Warn => LogLevel::Warn,
            log::Level::Info => LogLevel::Info,
            log::Level::Debug => LogLevel::Debug,
            log::Level::Trace => LogLevel::Trace,
        };
        append_log_record(lvl, &format!("{}: {}", record.target(), record.args()));
    }
    fn flush(&self) {}
}

/// Route the `log` crate into the VSF sink. Call once at startup (desktop `main`, Android `JNI_OnLoad`); a repeat call is a harmless no-op (`set_logger` fails closed).
#[cfg(feature = "logging")]
pub fn install_log_bridge() {
    if log::set_logger(&VsfLogBridge).is_ok() {
        log::set_max_level(log::LevelFilter::Debug);
    }
}

/// A silent (`--no-default-features`) build installs no logger at all: `log` crate records are dropped, exactly like `crate::log` lines.
#[cfg(not(feature = "logging"))]
pub fn install_log_bridge() {}

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
    // Route the `log` crate into the VSF sink — logcat is retired; photon.log.vsf (external files dir, adb-pullable) is the ONE log.
    install_log_bridge();

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
