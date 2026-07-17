// PHOTON SOURCE MAP — one readable line per file. Keep updated when files or major pub items change.
//
// lib.rs   — constants (PHOTON_PORT=4383, PHOTON_PORT_FALLBACK=3546, MULTICAST_PORT=4384, OSC_PER_SEC, PEER_EXPIRY_OSC=7d, KBUCKET_STALE_OSC=1h), always-on VSF logging sink (16 MiB + jittered 24–48h caps, name-scrubbed), and helpers: init_logging/log/log_at/clear_log/snapshot_log_bytes/log_size_bytes/read_log_from/install_log_bridge, LogRecord + parse_log_records (shared record decode: photonlog bin + the in-app Diagnostics viewer), fp(public_id) (non-PII log label), dozenal helpers (DOZENAL_NAMES, dozenal_glyphs UI / dozenal_spell read-aloud / dozenal_words camelCase log form, deglyph_for_log), jitter/jitter_dur (anti-thundering-herd 50–100% pad), module re-exports.
// main.rs  — winit event loop, window creation, tokio async runtime.
//
// crypto/
//   blind.rs        — friend-blinded private identity secret S (RAM-only, never persisted): PrivateS{None,Provisional,Live}, derive_blind_pad (per-device+friend OTP pad), make/open_blind_blob ((S⊕pad)‖check, fail-closed), s_check/s_id (tamper commitment + 4-byte tag epoch), seal/open_sibling_s (kete-AEAD S-transfer to a sibling).
//   chain.rs        — the braid: rolling-chain encryption (512-link, 16KB; see docs/braid.md). Chain, advance() (weaves ≤2 prior peer plaintexts), derive_salt, generate/verify_ack_proof, encrypt/decrypt_layers.
//   clutch.rs       — 8-algorithm parallel key ceremony: smear_hash, derive_conversation_token, derive_ceremony_instance, spaghettify, sibling_party_id (device-derived fleet-weave party id).
//   handle_proof.rs — memory-hard handle attestation (~1s); re-exports ihi::handle_proof.
//   self_verify.rs  — Ed25519 binary signature verification: AUTHOR_PUBKEY, SYSTEM_PUBKEYS, is_system_pubkey, verify_binary_hash, verify_file (update downloads — verify BEFORE exec).
//   shards.rs       — social recovery key sharding (TODO).
//
// network/
//   fgtw/           — Fractal Gradient Trust Web (Kademlia DHT). blob.rs, bootstrap.rs (load_bootstrap_peers), fingerprint.rs (derive_device_keypair/get_machine_fingerprint; Keypair lives in the fgtw crate), node.rs (routing table/k-buckets), peer_store.rs (PeerStore).
//     protocol.rs   — VSF FGTW+CLUTCH frames: FgtwMessage, PeerRecord (self-signed), hist_req/hist_page (friend-history), blind_put/ack/get/srv (friend-blinded S), av_req/av_resp (P2P avatar), reflect/reflect_resp (STUN reflection); all via canonical sign_file + read_verified.
//     fleet.rs      — photon's binding to the fgtw crate (the pure logic lives there, shared by every app + the worker): PhotonTransport (pooled reqwest) + PhotonSealer (roster AEAD) injected into fgtw::client wrappers. Crate side: fgtw::fleet (MembershipBlob genesis/add/depart/fold — fold IS the auth rule: bilateral add via consent egg, self-signed departure only; BindRequest + bindreq_signing_bytes), fgtw::fanout (fleet-key seal/recover/rotate), fgtw::fstate (roster codec), fgtw::pair (masked device words). Photon wrappers: current_members[_with_ts|_verified], bind_device (consent-carrying), depart_device, bindreq_put/list/withdraw, rotate_fleet_key, push/pull_roster.
//     relay.rs      — relay node logic.
//   clock_check.rs  — one-shot wall-clock sanity check via nunc-time consensus (all platforms except Redox, warn-only): spawn_clock_check, ClockJumpDetector, ClockCheckResult.
//   handle_query.rs — handle attestation + lookup: HandleQuery (query/query_resume/search + try_recv*), QueryRequest, QueryResult{Success(AttestationData),AlreadyAttested,Error}, AttestationData{handle_proof, identity_seed, contacts, friendships, avatar_pixels, peers}.
//   history_pages.rs— key-agnostic history-backfill page codec (fleet phase reuses verbatim): seal/open_history_page (VSF + kete ChaCha20-Poly1305), HistoryRow, HistoryPagePlain, MAX_PAGE_ROWS=50, MAX_PAGE_BYTES=24KB.
//   http.rs         — shared pooled HTTP for FGTW: runtime (one persistent tokio), async_client, blocking.
//   inspect.rs      — network diagnostics + VSF disk I/O: vsf_write, vsf_read.
//   pairing_beacon.rs — pairing v2 proximity beacon transport seam (docs/pairing-v2.md, shadow mode): announce_guard/start_scan/stop_scan/on_frame_heard/heard, HeardCandidate; couriers = bluer scan (Linux), PhotonBeacon JNI (Android), stubs elsewhere.
//   peer_updates.rs — peer state change notifications: PeerUpdate, PeerUpdateClient.
//   pt/             — Photon Transfer (large-message transport): buffer.rs (reassembly), packets.rs (PTSpec framing), state.rs (Direction/TransferState/OutboundTransfer), window.rs (PTManager sliding-window, send/send_with_pubkey, handle_spec/data/ack; SINGLE_PACKET_MAX=1024), RelayInfo, TickSend.
//   status.rs       — P2P ping/pong + CLUTCH orchestration: StatusChecker, StatusUpdate (Online/ChatMessage/MessageAck/Clutch*/Avatar*/History*/BlindFrameReceived/LanPeerDiscovered/ReflexiveLearned), request structs (Message/Ack/PTSend/History/ClutchOffer/Kem/Complete/LanBroadcast).
//   tcp.rs          — TCP fallback for large payloads: send, recv.
//   traverse/       — NAT traversal (reflexive discovery so far): reflexive.rs (ReflexiveState, quorum-adopted public addr from pong observed_addr + ReflectResponse).
//   udp.rs          — UDP socket utilities: send/send_sync, canon_socketaddr (::ffff:→v4), get_local_ip, get_broadcast_addr.
//
// platform/  — mod.rs (platform detection), jni_android.rs (Android JNI bridge).
//
// storage/ — flat vault via the kete crate (FlatStorage, re-exported); conversation content in the rarangi crate. Every entry is addressed by a flat 32-byte key vault_key(domain, scope) = blake3_kdf("photon.storage.entry.v0", domain||scope), never a path — domain is a plain word ("avatar","state","chains",...), scope is the 32-byte identity the entry is about.
//   mod.rs        — kete re-exports (FlatStorage, StorageError, encrypt/decrypt_bytes, App, APP, android_vault_dirs), vault_key, raw file helpers, photon_config_dir.
//   cloud.rs      — FGTW cloud backup (contacts sync): CloudContact, CloudError, contacts_storage_key, contacts_encryption_key.
//   contacts.rs   — contact + conversation storage. State keyed by contact.handle_hash (= party id: identity seed for friends, sibling pid for siblings). save/load_contact_list, save/load_contact_state, save/load_all_contacts, save/load_sibling_list + load_all_siblings + delete_sibling (fleet-sibling index), save/load_messages (rarangi rows keyed by eagle_time; carries content_hash/ack_hash/recovered), save_messages_page, load_message_page_before. contact_state persists the history cursor (hist_oldest/hist_complete), the roster LWW clock (roster_updated), blind deposits, and the folded fleet (fleet_member/fleet_folded_once/fleet_members_ts). CLUTCH keypairs/slots are memory-only no-ops.
//   friendship.rs — per-friendship chain STATE (the ratchet, not content) at vault_key("chains", friendship_id); v6 adds history_key. save/load/delete_friendship_chains, load_all_friendships.
//   settings.rs   — user-adjustable app settings, plain VSF (non-secret, NOT the vault): Settings{hex_head,hex_tail}, load_or_create, apply.
//   fleet_settings.rs — linked-settings layer (per-device maps + link-to-global, born linked; docs/global-vault.md): FleetSettings{global,devices,our_device}, effective/linked/set/set_link/merge_from, save/load_fleet_settings (vault "settings" entry via the fgtw::fstate codec).
//
// types/
//   contact.rs    — Contact (id, handle*, public_identity, fleet_members + fleet_folded_once/fleet_members_ts, roster_updated LWW clock, clutch_* ceremony state, chain-weave flags, is_sibling, blind fields), plus ::new/new_sibling, knows_device/answerable_pubkeys (fold-respecting trust), init_clutch_slots, insert_message_sorted, clutch_status_detail. Also PartySlot, ChatMessage, HistoryRecovery, HandleText, ContactId, ClutchState, TrustLevel, CHAIN_PROBE_MARKER.
//   device.rs     — DevicePubkey, ed25519_secret_to_x25519.
//   friendship.rs — CeremonyId (derive_base/derive), FriendshipId (derive/to_base64), FriendshipChains{friendship_id, conversation_token, chains, participants}.
//   handle.rs     — Handle{text,key}: new, to_handle_proof, username_to_handle_proof.
//   peer.rs       — Peer, ConnectionState, DhtAnnouncement.
//   seed.rs       — Seed([u8;32]).
//   shard.rs      — KeyShard, ShardId, DecryptedShard, RecoveryRequest, RecoveryApproval, ShardDistribution.
//
// ui/
//   photon_app.rs      — the whole app: PhotonApp state + the winit event/tick loop, all render arms, CLUTCH ceremony machinery, fleet reconcile, device add/remove, S/blind drivers, history recovery, settings pages. (The old app/compositing/drawing/text_* split was retired into fluor.)
//   avatar.rs          — avatar encode/upload/download, AVATAR_SIZE.
//   colour.rs, colour_convert.rs, display_profile.rs, lms2006so.rs — colour + display-profile conversion (VSF RGB → BT.2020, ICC).
//   chromatic_wave.rs  — the sine-modulated visible-spectrum bar (direct-pixel).
//   state.rs           — AppState{Launch,Ready,Searching,Conversation,AddDevice,Settings(SettingsPage),Connected}, SettingsPage{You,Fleet,Security,Recovery,Appearance,Notifications,Updates,Diagnostics,About}.
//   settings_widgets.rs, settings_layout.rs — Checkbox + SettingsLayout (nav-rail vs content split).
//   keyboard.rs, mouse.rs — input handling.
//
// bin/  — photon-keygen.rs (signing-key gen), photon-signature-signer.rs (binary signing), test-device-key.rs (device-key diagnostic), photonlog.rs (VSF log reader).

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

/// Retained for the desktop/Windows `main()` call site. The VSF file sink now opens LAZILY on the first log after the platform data dir is known (Android sets it partway thru JNI startup), so this is a no-op — kept only so existing callers compile.
pub fn init_logging() {}

/// Short, non-PII label for logs: the first 4 bytes of a PUBLIC id (handle_proof / device pubkey) as hex.
/// Log this instead of a plaintext handle — the durable log then carries pseudonymous identifiers, never names, so it stays diagnostic (you can correlate a fingerprint across a run) without leaking who anyone is.
/// The dozenal digit NAMES, digit 0..11 — Zil(0)/Zila(1)/Zilor(2)/Ter(3)/Tera(4)/Teror(5)/Lun(6)/Luna(7)/Lunor(8)/Stel(9)/Stela(10)/Stelor(11); the same set the Oxanium `+glyphs` face draws at 0x10..0x1B. UI shows the GLYPHS, logs/read-aloud show these WORDS. Never arabic.
pub const DOZENAL_NAMES: [&str; 12] = [
    "Zil", "Zila", "Zilor", "Ter", "Tera", "Teror", "Lun", "Luna", "Lunor", "Stel", "Stela", "Stelor",
];

/// Render `n` in dozenal as reserved control-code bytes 0x10+digit — the Oxanium `+glyphs` face draws them as the dozenal digits. UI-only: terminals show garbage, so LOG paths use [`dozenal_words`] instead.
pub fn dozenal_glyphs(mut n: u32) -> String {
    if n == 0 {
        return char::from(0x10).to_string();
    }
    let mut digits = Vec::new();
    while n > 0 {
        digits.push(char::from(0x10 + (n % 12) as u8));
        n /= 12;
    }
    digits.iter().rev().collect()
}

/// Spell `n` in dozenal digit words, space-separated ("Zilor Stela") — the read-aloud form.
pub fn dozenal_spell(mut n: u32) -> String {
    if n == 0 {
        return DOZENAL_NAMES[0].to_string();
    }
    let mut parts = Vec::new();
    while n > 0 {
        parts.push(DOZENAL_NAMES[(n % 12) as usize]);
        n /= 12;
    }
    parts.iter().rev().copied().collect::<Vec<_>>().join(" ")
}

/// Spell `n` in dozenal digit words, camelCase-concatenated ("ZilorStela") — the LOG/copy-paste form (double-click selects the whole value; terminals can't draw the glyph control codes).
pub fn dozenal_words(mut n: u32) -> String {
    if n == 0 {
        return DOZENAL_NAMES[0].to_string();
    }
    let mut parts = Vec::new();
    while n > 0 {
        parts.push(DOZENAL_NAMES[(n % 12) as usize]);
        n /= 12;
    }
    parts.iter().rev().copied().collect::<Vec<_>>().concat()
}

/// Transliterate any dozenal GLYPH control codes (0x10..0x1B) in a UI string into their camelCase digit words, for log emission — one string builder serves both surfaces.
pub fn deglyph_for_log(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let cp = c as u32;
        if (0x10..=0x1B).contains(&cp) {
            out.push_str(DOZENAL_NAMES[(cp - 0x10) as usize]);
        } else {
            out.push(c);
        }
    }
    out
}

pub fn fp(public_id: &[u8]) -> String {
    hex::encode(&public_id[..public_id.len().min(4)])
}

/// The log-submission encryption key: a ChaCha20-Poly1305 key derived from the identity seed ALONE — deliberately NOT folding in device_secret.
/// The identity seed is deterministic from the handle, so anyone who knows the handle (the admin, handed one by a peer with a support request) can re-derive this key and open that peer's submitted log — while anyone who merely grabs the R2 ciphertext, not knowing whose it is, cannot. This is the whole "decryptable if you know the identity seed" property: the log is sealed on the client with this key before it ever leaves the device, so no plaintext hits the wire.
pub fn log_encryption_key(identity_seed: &[u8; 32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(identity_seed);
    hasher.update(b"photon.log.v0");
    *hasher.finalize().as_bytes()
}

/// The log RETRIEVAL tag: `spaghettify("photon_log_v1" ‖ identity_seed)`.
/// A submitted log is stored on FGTW under this tag, and pulled by presenting it — so the tag is a *capability* derived one-way from the identity seed (which is deterministic from the handle). Whoever knows the seed can both FIND (this tag) and DECRYPT ([`log_encryption_key`]) the logs; whoever doesn't sees only opaque tags over ciphertext. spaghettify (not BLAKE3) matches the stack's one-way primitive and keeps the seed unrecoverable from the tag; the tag is what travels to the server, never the seed. Distinct domain tag from the encryption key so the two derivations can't collide.
pub fn log_retrieval_tag(identity_seed: &[u8; 32]) -> [u8; 32] {
    let mut input = Vec::with_capacity(13 + 32);
    input.extend_from_slice(b"photon_log_v1");
    input.extend_from_slice(identity_seed);
    ihi::spaghettify(&input)
}

#[cfg(test)]
mod log_seal_tests {
    use super::*;

    #[test]
    fn seal_roundtrips_no_plaintext_and_rejects_wrong_seed() {
        let seed = [7u8; 32];
        let log = b"INFO  FGTW: announce port=4383 ip=redacted".as_slice();
        let key = log_encryption_key(&seed);
        let sealed = storage::encrypt_bytes(log, &key).unwrap();
        // No plaintext on the wire: the ciphertext must not contain the log bytes.
        assert!(!sealed.windows(log.len()).any(|w| w == log));
        // The right seed opens it.
        assert_eq!(storage::decrypt_bytes(&sealed, &key).unwrap(), log);
        // A different seed derives a different key → AEAD auth failure, never a wrong plaintext.
        let wrong = log_encryption_key(&[8u8; 32]);
        assert!(storage::decrypt_bytes(&sealed, &wrong).is_err());
    }
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
#[cfg(not(feature = "logging"))]
#[inline(always)]
pub fn snapshot_log_bytes() -> Option<Vec<u8>> {
    None
}
#[cfg(not(feature = "logging"))]
#[inline(always)]
pub fn log_size_bytes() -> u64 {
    0
}

/// Live size of the open VSF log — the cheap "has anything been logged since?" probe (one atomic load, no I/O). The Diagnostics Submit pill greys while this still equals the size captured at the last successful submit: identical size = identical bytes = a duplicate upload.
#[cfg(feature = "logging")]
pub fn log_size_bytes() -> u64 {
    LOG_BYTES.load(std::sync::atomic::Ordering::Relaxed)
}

// The structured VSF log sink: one COMPLETE VSF record per line — {creation_time (Eagle), section "log" {lvl, msg}} — appended to `<photon_config_dir>/photon.log.vsf` on EVERY platform (Android: app filesDir, pullable via `adb pull`; desktop/Windows: the config dir). The log is thus a stream of self-describing, Eagle-time-stamped, vsfinfo-inspectable records; read it with the `photonlog` bin. Opens lazily and RETRIES until the dir is ready — a plain Mutex<Option<File>>, NOT a OnceLock, precisely so a pre-data-dir failure isn't cached forever (the first Kotlin/JNI lines predate Android's data_dir; they buffer in LOG_PENDING below and flush when the file opens).
// Known filename (logging is a dev-build feature, so adb-pull discoverability beats filename privacy).
#[cfg(feature = "logging")]
static LOG_FILE: std::sync::Mutex<Option<std::fs::File>> = std::sync::Mutex::new(None);

// Records that arrive before the sink can open (Android: everything logged before the JNI data dir lands, including the Kotlin bridge's earliest lifecycle lines) — held as already-built VSF record bytes so their creation stamps stay true, drained into the file the moment it opens. Bounded so a never-initializing process can't grow it unbounded; overflow drops the newest record (the earliest lines are the ones worth keeping).
#[cfg(feature = "logging")]
static LOG_PENDING: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(Vec::new());
#[cfg(feature = "logging")]
const LOG_PENDING_CAP: usize = 64 << 10;

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
fn append_log_record(level: LogLevel, msg: &str, vals: &[LogValue]) {
    use std::io::Write;
    // Build first so a buffered record carries the stamp of when it was LOGGED, not when the sink finally opened. `msg` is the pure-text template; each captured value rides as its own TYPED `val` field, in slot order — a number never stringifies into the record (numbers-binary-at-rest).
    let mut section = vsf::VsfSection::new("log");
    section.add_field_multi("lvl", vec![vsf::VsfType::u(level as usize, false)]);
    section.add_field_multi("msg", vec![vsf::VsfType::x(msg.to_string())]);
    for v in vals {
        let t = match v {
            LogValue::U(n) => vsf::VsfType::u(*n as usize, false),
            LogValue::I(n) => vsf::VsfType::i6(*n as i64),
            LogValue::F(n) => vsf::VsfType::f6(*n),
            LogValue::B(b) => vsf::VsfType::u(*b as usize, false),
            LogValue::T(s) => vsf::VsfType::x(s.clone()),
            LogValue::Addr(a) => vsf::VsfType::v_u3(vsf::types::Vector {
                data: {
                    let mut b = match a.ip() {
                        std::net::IpAddr::V4(v4) => v4.octets().to_vec(),
                        std::net::IpAddr::V6(v6) => v6.octets().to_vec(),
                    };
                    b.extend_from_slice(&a.port().to_le_bytes());
                    b
                },
            }),
        };
        section.add_field_multi("val", vec![t]);
    }
    let record = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_section_direct(section)
        .build();
    let Ok(mut guard) = LOG_FILE.lock() else {
        return;
    };
    if guard.is_none() {
        if let Some(dir) = log_dir() {
            let _ = std::fs::create_dir_all(&dir);
            let path = dir.join("photon.log.vsf");
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                // Drain the pre-dir buffer FIRST so the file stays chronological, then seed the counters (metadata already includes the drained bytes).
                if let Ok(mut pending) = LOG_PENDING.lock() {
                    if !pending.is_empty() {
                        let _ = f.write_all(&pending);
                        pending.clear();
                        pending.shrink_to_fit();
                    }
                }
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
        // No sink yet (Android before the JNI data dir lands): hold the built record so the earliest lines aren't lost.
        if let (Ok(bytes), Ok(mut pending)) = (&record, LOG_PENDING.lock()) {
            if pending.len() + bytes.len() <= LOG_PENDING_CAP {
                pending.extend_from_slice(bytes);
            }
        }
        return;
    };
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

/// Reads the current on-disk `photon.log.vsf` as raw bytes for submission (the "Submit" diagnostic action).
/// A plain file read — records are written unbuffered per line, so the on-disk content is already current; no writer flush needed. `None` if the log hasn't opened yet (pre-data-dir) or can't be read.
#[cfg(feature = "logging")]
pub fn snapshot_log_bytes() -> Option<Vec<u8>> {
    let path = log_dir()?.join("photon.log.vsf");
    std::fs::read(&path).ok().filter(|b| !b.is_empty())
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
    append_log_record(level, msg, &[]);
}

// ── Structured logging (numbers-binary-at-rest, 2026-07-16): the record stores the message TEMPLATE as pure text and every interpolated value as a TYPED `val` field beside it — a number never stringifies into storage; photonlog/vsfinfo choose the display base at READ time. Use `logf!`/`logf_at!` (format!-shaped) instead of `log(&format!(...))`. ──

/// One captured log value, typed — becomes a native VSF field in the record.
pub enum LogValue {
    U(u128),
    I(i128),
    F(f64),
    B(bool),
    T(String),
    Addr(std::net::SocketAddr),
}

/// Capture wrapper for `logf!` args. Inherent impls (numerics, bools, addresses) outrank the [`CapDisplay`] blanket at method resolution, so typed capture is automatic and everything else degrades to text — the autoref-free inherent-priority specialization.
pub struct Cap<T>(pub T);

/// Lowest-priority capture: anything Display becomes text (prose, hex labels, hashes — nouns). Implemented on `&Cap<T>` so the typed inherent impls on `Cap<X>` win the method probe without ambiguity (autoref specialization); the macro's `Cap(&arg)` form means args are only ever borrowed.
pub trait CapDisplay {
    fn cap(self) -> LogValue;
}
impl<T: std::fmt::Display> CapDisplay for &Cap<T> {
    fn cap(self) -> LogValue {
        LogValue::T(self.0.to_string())
    }
}

/// Primitive-typed capture — one impl per primitive, unified under ONE generic inherent impl on [`Cap`] so integer-literal inference resolves (a per-type inherent zoo made `{integer}` ambiguous).
pub trait CapPrim: Copy {
    fn to_log(self) -> LogValue;
}
macro_rules! cap_prim {
    ($($t:ty => $variant:ident as $conv:ty),* $(,)?) => {
        $(impl CapPrim for $t {
            fn to_log(self) -> LogValue { LogValue::$variant(self as $conv) }
        })*
    };
}
cap_prim! {
    u8 => U as u128, u16 => U as u128, u32 => U as u128, u64 => U as u128, u128 => U as u128, usize => U as u128,
    i8 => I as i128, i16 => I as i128, i32 => I as i128, i64 => I as i128, i128 => I as i128, isize => I as i128,
    f32 => F as f64, f64 => F as f64,
}
impl CapPrim for bool {
    fn to_log(self) -> LogValue {
        LogValue::B(self)
    }
}
impl CapPrim for std::net::SocketAddr {
    fn to_log(self) -> LogValue {
        LogValue::Addr(self)
    }
}
impl<T: CapPrim> Cap<&T> {
    pub fn cap(self) -> LogValue {
        (*self.0).to_log()
    }
}
/// Render a template + captured values for a TERMINAL/console surface (photonlog shares the same walk). Slots `{}`/`{spec}` substitute values in order (spec is a rendering hint only); numbers render in current mixed arabic units per the display doctrine — the point is they were STORED binary. `{{`/`}}` are literal braces.
pub fn render_log_line(template: &str, vals: &[LogValue]) -> String {
    let mut out = String::with_capacity(template.len() + vals.len() * 8);
    let mut chars = template.chars().peekable();
    let mut next = 0usize;
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push('}');
            }
            '{' => {
                // Consume to the closing brace (spec ignored at render).
                for s in chars.by_ref() {
                    if s == '}' {
                        break;
                    }
                }
                if let Some(v) = vals.get(next) {
                    match v {
                        LogValue::U(n) => out.push_str(&n.to_string()),
                        LogValue::I(n) => out.push_str(&n.to_string()),
                        LogValue::F(n) => out.push_str(&n.to_string()),
                        LogValue::B(b) => out.push_str(if *b { "true" } else { "false" }),
                        LogValue::T(s) => out.push_str(s),
                        LogValue::Addr(a) => out.push_str(&a.to_string()),
                    }
                }
                next += 1;
            }
            c => out.push(c),
        }
    }
    out
}

/// One decoded log record — the shared decode shape for the `photonlog` bin and the in-app Diagnostics log viewer.
#[derive(Clone, Debug)]
pub struct LogRecord {
    /// Record creation time, eagle oscillations (0 = the record carried none).
    pub osc: i64,
    /// Severity 0..=4 (TRACE..ERROR); u64::MAX = the record carried none.
    pub level: u64,
    /// The rendered message: the stored template with its typed `val` fields substituted at READ time (numbers live binary in the record; this is the display edge).
    pub msg: String,
}

/// Decode complete records from a `photon.log.vsf` byte stream (each record = one full VSF file: {creation_time (Eagle), section "log" {lvl, msg, val*}}). Returns the records plus the byte offset of the last COMPLETE record boundary — a half-written trailing record (mid-append) is left for the next pass instead of being mis-decoded. Shared by the `photonlog` bin and the in-app viewer, so the two surfaces can never drift.
pub fn parse_log_records(buf: &[u8]) -> (Vec<LogRecord>, usize) {
    use vsf::file_format::{VsfHeader, VsfSection};
    use vsf::types::EtType;
    use vsf::VsfType;
    let mut records = Vec::new();
    let mut off = 0usize;
    while off < buf.len() {
        let rest = &buf[off..];
        let Ok((header, header_end)) = VsfHeader::decode(rest) else {
            break; // incomplete tail — stop, retry next pass
        };
        let mut ptr = 0usize;
        let Ok(section) = VsfSection::parse(&rest[header_end..], &mut ptr) else {
            break;
        };
        let rec = header_end + ptr;
        if rec == 0 {
            break;
        }
        let level = section
            .get_field("lvl")
            .and_then(|f| f.values.first())
            .and_then(|v| {
                use vsf::schema::FromVsfType;
                u64::from_vsf_type(v).ok()
            })
            .unwrap_or(u64::MAX);
        let template = section
            .get_field("msg")
            .and_then(|f| f.values.first())
            .and_then(|v| match v {
                VsfType::x(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let vals: Vec<LogValue> = section
            .get_fields("val")
            .iter()
            .filter_map(|f| f.values.first())
            .map(|v| match v {
                VsfType::u(n, _) => LogValue::U(*n as u128),
                VsfType::i6(n) => LogValue::I(*n as i128),
                VsfType::f6(n) => LogValue::F(*n),
                VsfType::x(s) => LogValue::T(s.clone()),
                VsfType::v_u3(vec) if vec.data.len() == 6 || vec.data.len() == 18 => {
                    let (ip_bytes, port_bytes) = vec.data.split_at(vec.data.len() - 2);
                    let port = u16::from_le_bytes([port_bytes[0], port_bytes[1]]);
                    let ip: std::net::IpAddr = if ip_bytes.len() == 4 {
                        std::net::Ipv4Addr::new(ip_bytes[0], ip_bytes[1], ip_bytes[2], ip_bytes[3]).into()
                    } else {
                        let mut o = [0u8; 16];
                        o.copy_from_slice(ip_bytes);
                        std::net::Ipv6Addr::from(o).into()
                    };
                    LogValue::Addr(std::net::SocketAddr::new(ip, port))
                }
                other => LogValue::T(format!("{other:?}")),
            })
            .collect();
        let msg = if vals.is_empty() {
            template
        } else {
            render_log_line(&template, &vals)
        };
        let osc = match &header.creation_time {
            Some(VsfType::e(EtType::e6(o))) => *o,
            Some(VsfType::e(EtType::e5(o))) => *o as i64,
            Some(VsfType::e(EtType::e7(o))) => *o as i64,
            _ => 0,
        };
        records.push(LogRecord { osc, level, msg });
        off += rec;
    }
    (records, off)
}

/// Read the on-disk log from byte `offset` to EOF — the in-app viewer's tail-follow read (a seek, not a whole-file copy). `None` = no log yet or nothing past the offset; a shrunken file (rotation/clear) also reads `None` here and the caller re-syncs from zero.
#[cfg(feature = "logging")]
pub fn read_log_from(offset: u64) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let path = log_dir()?.join("photon.log.vsf");
    let mut f = std::fs::File::open(&path).ok()?;
    let len = f.metadata().ok()?.len();
    if len <= offset {
        return None;
    }
    f.seek(SeekFrom::Start(offset)).ok()?;
    let mut out = Vec::with_capacity((len - offset) as usize);
    f.read_to_end(&mut out).ok()?;
    Some(out)
}
#[cfg(not(feature = "logging"))]
#[inline(always)]
pub fn read_log_from(_offset: u64) -> Option<Vec<u8>> {
    None
}

#[cfg(feature = "logging")]
pub fn log_structured(level: LogLevel, template: &str, vals: Vec<LogValue>) {
    append_log_record(level, template, &vals);
}
#[cfg(not(feature = "logging"))]
pub fn log_structured(_level: LogLevel, _template: &str, _vals: Vec<LogValue>) {}

/// format!-shaped structured log at Info: `logf!("RX {} bytes from {}", n, addr)` — the template stores as pure text, `n`/`addr` as typed fields.
#[macro_export]
macro_rules! logf {
    ($fmt:expr $(, $arg:expr)* $(,)?) => {{
        #[allow(unused_imports)]
        use $crate::CapDisplay as _;
        $crate::log_structured($crate::LogLevel::Info, $fmt, vec![$($crate::Cap(&$arg).cap()),*]);
    }};
}

/// [`logf!`] with an explicit level.
#[macro_export]
macro_rules! logf_at {
    ($level:expr, $fmt:expr $(, $arg:expr)* $(,)?) => {{
        #[allow(unused_imports)]
        use $crate::CapDisplay as _;
        $crate::log_structured($level, $fmt, vec![$($crate::Cap(&$arg).cap()),*]);
    }};
}

// Bridge the `log` crate into the VSF sink, so records from every dependency that uses log macros (fluor, tohu, the JNI platform layer, reqwest, ...) land in photon.log.vsf alongside `crate::log` lines — ONE durable, pullable, user-submittable log, no logcat/stdout fork.
// Mirrors the retired android_logger/env_logger setup: Debug and up globally, the known-noisy crates only at Warn+.
#[cfg(feature = "logging")]
struct VsfLogBridge;
#[cfg(feature = "logging")]
impl log::Log for VsfLogBridge {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        // Known-chatty dependencies held to Warn+ so their DEBUG streams don't drown the log: naga/wgpu flood per-shader-variable on every pipeline build; rustls/tungstenite/hyper/h2 flood per-connection handshake detail (observed burying the JOIN ceremony trace within a session).
        const NOISY: &[&str] = &[
            "cosmic_text", "reqwest", "naga", "wgpu",
            "rustls", "tungstenite", "tokio_tungstenite", "hyper", "h2",
        ];
        let t = meta.target();
        let noisy = NOISY.iter().any(|p| t.starts_with(p));
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
        append_log_record(lvl, &format!("{}: {}", record.target(), record.args()), &[]);
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
