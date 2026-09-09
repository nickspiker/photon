// PHOTON SOURCE MAP — one readable line per file. Keep updated when files or major pub items change.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// lib.rs   — constants (PHOTON_PORT=4383, PHOTON_PORT_FALLBACK=3546, MULTICAST_PORT=4384, OSC_PER_SEC, PEER_EXPIRY_OSC=7d, KBUCKET_STALE_OSC=1h), always-on VSF logging sink (16 MiB + jittered 24–48h caps, name-scrubbed), and helpers: init_logging/log/log_at/clear_log/snapshot_log_bytes/log_size_bytes/read_log_from/install_log_bridge, LogRecord + parse_log_records (shared record decode: photonlog bin + the in-app Diagnostics viewer), fp(public_id) (non-PII log label), dozenal helpers (DOZENAL_NAMES, dozenal_glyphs UI / dozenal_spell read-aloud / dozenal_words camelCase log form, deglyph_for_log), jitter/jitter_dur (anti-thundering-herd 50–100% pad), module re-exports. main.rs  — winit event loop, window creation, tokio async runtime.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// crypto/
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   blind.rs        — friend-blinded private identity secret S (RAM-only, never persisted): PrivateS{None,Provisional,Live}, derive_blind_pad (per-device+friend OTP pad), make/open_blind_blob ((S⊕pad)‖check, fail-closed), s_check/s_id (tamper commitment + 4-byte tag epoch), seal/open_sibling_s (kete-AEAD S-transfer to a sibling).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   chain.rs        — the braid: rolling-chain encryption (512-link, 16KB; see docs/braid.md). Chain, advance() (weaves ≤2 prior peer plaintexts), derive_salt, generate/verify_ack_proof, encrypt/decrypt_layers.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   clutch.rs       — 8-algorithm parallel key ceremony: smear_hash, derive_conversation_token, derive_ceremony_instance, spaghettify, sibling_party_id (device-derived fleet-weave party id).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   era.rs          — the LIGHT ERA RATCHET: hybrid KEM (ML-KEM-1024 + X25519 + HQC-256) era_keygen/era_encapsulate/era_decapsulate, derive_era_fresh/derive_era_transcript/derive_era_keys (next era = KDF(old root ‖ old hk ‖ fresh ‖ transcript)), EraSignal Init/Resp/Nudge row grammar, LIGHT_RATCHET_CADENCE_ROWS=256.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   handle_proof.rs — memory-hard handle attestation (~1s); re-exports ihi::handle_proof.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   self_verify.rs  — Ed25519 binary signature verification: AUTHOR_PUBKEY, SYSTEM_PUBKEYS, is_system_pubkey, verify_binary_hash, verify_file (update downloads — verify BEFORE exec).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   shards.rs       — social recovery key sharding (TODO).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// network/
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   fgtw/           — Fractal Gradient Trust Web (Kademlia DHT). blob.rs, bootstrap.rs (load_bootstrap_peers), fingerprint.rs (derive_device_keypair/get_machine_fingerprint; Keypair lives in the fgtw crate), node.rs (routing table/k-buckets), peer_store.rs (PeerStore).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//     protocol.rs   — VSF FGTW+CLUTCH frames: FgtwMessage, PeerRecord (self-signed), hist_req/hist_page (friend-history), friend_knock (consent-gate add-intent), chain_reset (sibling fork repair), blind_put/ack/get/srv (friend-blinded S), av_req/av_resp (P2P avatar), reflect/reflect_resp (STUN reflection); seal/open_pong_sensitive (pong's sync rows + name + avatar pin AEAD-sealed as an inner `pongsec` section under the pairwise pong key — the obs echo alone stays plaintext); all via canonical sign_file + read_verified.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//     fleet.rs      — photon's binding to the fgtw crate (the pure logic lives there, shared by every app + the worker): PhotonTransport (pooled reqwest) + PhotonSealer (roster AEAD) injected into fgtw::client wrappers. Crate side: fgtw::fleet (MembershipBlob genesis/add/depart/fold — fold IS the auth rule: bilateral add via consent egg, self-signed departure only; BindRequest + bindreq_signing_bytes), fgtw::fanout (fleet-key seal/recover/rotate + fanout_needs_rotation, the §14.2 removal-rotates sentinel), fgtw::fstate (roster codec), fgtw::pair (masked device words). Photon wrappers: current_members[_with_ts|_verified], bind_device (consent-carrying), depart_device, bindreq_put/list/withdraw, rotate_fleet_key, push/pull_roster.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//     relay.rs      — the relay SEND half: send_via_relay[_sync] signs a `relay` VSF (recipient kx + payload v'r') and POSTs it to fgtw.org, where the PipeHub DO forwards it live down the recipient's WebSocket (no R2, no mailbox, no polling). The RECEIVE half is a WebSocket the status task holds open to fgtw.org/pipe?dev=<our device>; each frame is injected into the receiver's select! tagged RELAY_ADDR so the whole data plane — CLUTCH, ping/pong presence, chat, acks — rides the real dispatch. See network/status.rs (pipe task + relay_reply).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   clock_check.rs  — wall-clock sanity check via nunc-time consensus (all platforms except Redox): spawn_clock_check, ClockJumpDetector, ClockCheckResult (offset/confidence/anchor in OSCILLATIONS, never seconds).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   time_base.rs    — photon's OWN clock: nunc's offset held against a MONOTONIC anchor, so ordering survives a system clock the human sets wrong on purpose. adopt, now_osc, stamp_osc (never repeats, never regresses), offset_now.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   handle_query.rs — handle attestation + lookup: HandleQuery (query/query_resume/search + try_recv*), QueryRequest, QueryResult{Success(AttestationData),AlreadyAttested,Error}, AttestationData{handle_proof, identity_seed, contacts, friendships, avatar_pixels, peers}.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   history_pages.rs— key-agnostic history-backfill page codec (fleet phase reuses verbatim): seal/open_history_page (VSF + kete ChaCha20-Poly1305), HistoryRow, HistoryPagePlain, MAX_PAGE_ROWS=50, MAX_PAGE_BYTES=24KB.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   http.rs         — shared pooled HTTP for FGTW: runtime (one persistent tokio), async_client, blocking.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   inspect.rs      — network diagnostics + VSF disk I/O: vsf_write, vsf_read.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   pairing_beacon.rs — pairing v2 proximity beacon transport seam (docs/pairing-v2.md, shadow mode): announce_guard/start_scan/stop_scan/on_frame_heard/heard, HeardCandidate; couriers = bluer scan (Linux), PhotonBeacon JNI (Android), stubs elsewhere.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   peer_updates.rs — peer state change notifications: PeerUpdate, PeerUpdateClient.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   pt/             — Photon Transfer (large-message transport): buffer.rs (reassembly), packets.rs (PTSpec framing), state.rs (Direction/TransferState/OutboundTransfer), window.rs (PTManager sliding-window, send/send_with_pubkey, handle_spec/data/ack; SINGLE_PACKET_MAX=1024), RelayInfo, TickSend.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   status.rs       — P2P ping/pong + CLUTCH orchestration: StatusChecker, StatusUpdate (Online/ChatMessage/ChainResetReceived/MessageAck/Clutch*/Avatar*/History*/BlindFrameReceived/LanPeerDiscovered/ReflexiveLearned), request structs (Message/Ack/PTSend/History/ClutchOffer/Kem/Complete/LanBroadcast); PongSealKeys (UI-derived pairwise pong-tail keys, device pubkey → key) + friend/sibling_pong_seal_key.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   tcp.rs          — TCP fallback for large payloads: send, recv.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   traverse/       — NAT traversal (reflexive discovery so far): reflexive.rs (ReflexiveState, quorum-adopted public addr from pong observed_addr + ReflectResponse).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   udp.rs          — UDP socket utilities: send/send_sync, canon_socketaddr (::ffff:→v4), get_local_ip, get_broadcast_addr.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   wfd.rs          — Wi-Fi Direct bearer (docs/offgrid.md): WfdCred mint/seal/open (per-pair pre-provisioned group credential, elect_go lower-pubkey tie-break), rotating DNS-SD friend tokens (wfd_token/build_txt_tokens/match_txt_tokens), WfdBearer state machine (Idle→Stranded→Forming→Up) + WfdPlatform trait (AndroidWfd via JNI, NullWfd elsewhere), platform event queue (push_event/drain_events), RELAY_REACHABLE flag fed by the pipe task. Frames ride the main UDP socket — this module is discovery + group bring-up only.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// call/ — voice waves (docs/calls.md + docs/audio-paths.md): mod.rs (CallPhase, ActiveCall, media sink install/clear, OUTPUT_PAD_STOPS, media-liveness statics + peer redirect), engine.rs (per-call media thread: 5ms Opus CELT CBR ladder → RaptorQ piggyback windows (8/4/2/2 frames per rung) → sealed datagrams; inline PID duck + side-aware gate; v-chirp probe phase holds both directions at start; mute transmits zeros), vchirp.rs (V-chirp connect probe: template/frames, matched-filter fit + skew split + IR-tap export, Verdict mailbox, finish fit thread), nlms.rs (chirp-seeded NLMS echo canceller: RefRing + Nlms::cancel_frame, zero added latency, gated adaptation), learn.rs (in-call passive learner: Learner, PredGate, blend_g), calibrate.rs (profile types + learned-result mailbox + env/quietest_run), ringback.rs (caller-side ring cadence + probe), keys.rs (StepChain), packet.rs (seal/open/parse_header), signal.rs (CallSignal offer/answer/decline/busy/hangup/taken/anchor + device-named offer/answer + express seal/open), spool.rs/record.rs/playback.rs (sealed call recording + ended-screen preview).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// platform/  — mod.rs (platform detection), jni_android.rs (Android JNI bridge), autostart.rs (desktop login-item write/read/remove: HKCU Run / LaunchAgent plist / XDG autostart), control.rs (second-launch handoff channel: "show" surfaces the resident, "yield" makes a lifeline release the lock), desktop_notify.rs (generic "New message" system notification, hidden/unfocused-gated), crash_native.rs (native-fault catcher: SEH filter / unix signal handlers write the panic hook's crash sidecar so segfaults ride the next log submission), locale.rs (os_language: first-launch language seed sniff — LC_* ladder / GetUserDefaultLocaleName).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// storage/ — ONE device vault via the kete crate; conversation content in the rarangi crate. The vault opens from the device secret alone at first launch (device scope: hash(thing|device) — binding, flags, capsule) and gains the identity scope at attest (hash(thing|device|person)). Every entry is addressed by a flat 32-byte key vault_key(domain, scope) = blake3_kdf("photon.storage.entry.v0", domain||scope), never a path — domain is a plain word ("avatar","state","chains",...), scope is the 32-byte identity the entry is about. Blobs are vault values at identity-keyed addresses (blob_store/load/present/delete). NO migration/import layer — the fleet is the backup (chain replication + history sync fill a fresh vault); every file in the primary/secondary photon dirs that isn't `<device token>.vsf`/the log is deleted at first vault open (census_sweep), the legacy `Photon/` sibling dirs wholesale with them.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   mod.rs        — kete re-exports (FlatStorage, StorageError, encrypt/decrypt_bytes, App, APP, android_vault_dirs), open_session_vault (THE session open), device_vault + install_device_secret + device_flag/set_device_flag (pre-identity device scope), vault_key, blob_* (vault-backed), runtime_dir/runtime_artifact (lock + control socket), raw file helpers, photon_config_dir, isolate_test_storage.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   cloud.rs      — FGTW cloud backup (contacts sync): CloudContact, CloudError, contacts_storage_key, contacts_encryption_key.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   contacts.rs   — contact + conversation storage. Contact state keyed by contact.handle_hash (= party id: identity seed for friends, sibling pid for siblings); conversation content and state keyed by the participant-set conversation id. save/load_contact_list, save/load_contact_state, save/load_all_contacts, save/load_sibling_list + load_all_siblings + delete_sibling (fleet-sibling index), save/load_messages on a Conversation (rarangi rows keyed by eagle_time; carries content_hash/ack_hash/recovered), save_messages_page, load_message_page_before, save/load_conversation_state (unread + history cursor, with a legacy fallback read from old contact records). contact_state persists the roster LWW clock (roster_updated), blind deposits, and the folded fleet (fleet_member/fleet_folded_once/fleet_members_ts). CLUTCH keypairs/slots are memory-only no-ops.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   friendship.rs — per-friendship chain STATE (the ratchet, not content) at vault_key("chains", friendship_id); v6 adds history_key. save/load/delete_friendship_chains, load_all_friendships.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   fleet_settings.rs — linked-settings layer (per-device maps + link-to-global, born linked; docs/global-vault.md): FleetSettings{global,devices,our_device}, effective/linked/set/set_link/merge_from, save/load_fleet_settings (vault "settings" entry via the fgtw::fstate codec).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// types/
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   contact.rs    — Contact (id, handle*, public_identity, fleet_members + fleet_folded_once/fleet_members_ts, roster_updated LWW clock, clutch_* ceremony state, consent_mutual gate + knocked_session, chain-weave flags, is_sibling, blind fields), plus ::new/new_sibling, knows_device/answerable_pubkeys (fold-respecting trust), init_clutch_slots, insert_message_sorted, clutch_status_detail. Also PartySlot, ChatMessage, HistoryRecovery, HandleText, ContactId, ClutchState, TrustLevel, CHAIN_PROBE_MARKER.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   device.rs     — DevicePubkey, ed25519_secret_to_x25519.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   friendship.rs — CeremonyId (derive_base/derive), FriendshipId (derive/to_base64), FriendshipChains{friendship_id, conversation_token, chains, participants}.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   handle.rs     — Handle{text,key}: new, to_handle_proof, username_to_handle_proof.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   peer.rs       — Peer, ConnectionState, DhtAnnouncement.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   seed.rs       — Seed([u8;32]).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   shard.rs      — KeyShard, ShardId, DecryptedShard, RecoveryRequest, RecoveryApproval, ShardDistribution.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// ui/
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   photon_app.rs      — the app root: the PhotonApp struct + shared helpers/types, with the method bodies in per-concern child modules under photon_app/ (each a further `impl PhotonApp` glob-importing the root). (The old app/compositing/drawing/text_* split was retired into fluor.)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   photon_app/        — driver.rs (FluorApp trait impl: lifecycle/events/tick; core_init = the display-free startup half), lifeline.rs (headless pump: --lifeline mode runs core_init + advance_protocol with no fluor/winit, yields the instance lock to a full-UI launch — docs/headless-lifeline.md), render.rs (render_frame, the whole-frame paint), protocol.rs (advance_protocol pump), status.rs (check_status_updates drain), devices.rs (fleet add/join/roster/keys/lockout), ceremony.rs (CLUTCH spawns+checks+completion), conversation.rs (conv/contact state, braid RX, chain syncs, history pages), messaging.rs (send path, chain transmit, probe/seal), sync.rs (pings, retransmits, replication, recovery, blind ops), launch.rs (attest flow), settings.rs (settings/profile/updates), attachments.rs, bridge.rs (remote terminal + unattended), peers.rs (peer records/store), input.rs (send_event, layout, clipboard, textboxes, chords, wipe).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//                        era.rs (the era ratchet: friendship_repair — pure, table-tested, no destructive verdict — repair_dispatch, era_owner + recompute_ceremony_owners (computed owner on presence/fold/locked/roster edges), propose_light_ratchet / on_era_signal Init-Resp-Nudge, era_cutover_flush, arm_heavy_weaves / arm_heavy_weave_for (shrink ⇒ woven CLUTCH at epoch+1)).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   avatar.rs          — avatar encode/upload/download, AVATAR_SIZE.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   colour.rs, colour_convert.rs, display_profile.rs, lms2006so.rs — colour + display-profile conversion (VSF RGB → BT.2020, ICC).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   chromatic_wave.rs  — the sine-modulated visible-spectrum bar (direct-pixel).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   state.rs           — AppState{Launch,Ready,Searching,Conversation,AddDevice,Settings(SettingsPage),Connected}, SettingsPage{You,Fleet,Security,Recovery,Appearance,Notifications,Updates,Diagnostics,About}.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   settings_layout.rs — SettingsLayout (nav-rail vs content split). Checkbox is now fluor::widgets::Checkbox (first-class, alongside Button/Slider/Dropdown).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   fonts.rs           — the bundled font set, one loader (load_bundled): Oxanium weights + the dozenal `+glyphs` face, Noto Symbols/Math/Mono (mono symbol/arrow/box/currency coverage), Noto Arabic/Devanagari/Thai/Armenian/Georgian/Runic. No host fonts ever; CJK deliberately unbundled.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   (tools/font-trim.rs — std-only rustc bitty-executable: cut a STATIC TrueType font to the Unicode blocks it owns without renumbering (or with --compact, renumbering only when no layout table survives to disagree); scripts/fonts/build.sh regenerates assets/Noto's trimmed Mono + Math from assets/Noto/sources.)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   lang.rs            — THE language catalog (docs/languages.md): enum Msg (every user-facing string, semantic params), Lang{En,Es,Mi} + code/autonym/index, set_lang/lang statics (device-local display.lang, OS-locale seeded), tr() dispatch; lang/en.rs, lang/es.rs, lang/mi.rs = one exhaustive match each (the compiler is the completeness checker; every numeral thru fmt_num).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//   keyboard.rs, mouse.rs — input handling.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
//
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// bin/  — photon-keygen.rs (signing-key gen), photon-signature-signer.rs (binary signing), test-device-key.rs (device-key diagnostic), photonlog.rs (VSF log reader).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Photon network ports - used for ALL network communication UDP: peer-to-peer status pings, CLUTCH ceremony, chat messages TCP: large payloads (full CLUTCH offers ~548KB, KEM responses ~17KB) FGTW: handle registration and peer discovery announcements Primary: 4383, Fallback: 3546 (both IANA unassigned)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub const PHOTON_PORT: u16 = 4383;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub const PHOTON_PORT_FALLBACK: u16 = 3546;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Multicast port for LAN peer discovery Separate from main port to avoid SO_REUSEADDR complexity 4384 is IANA unassigned
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub const MULTICAST_PORT: u16 = 4384;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Eagle Time: oscillations per second (hydrogen hyperfine transition)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub const OSC_PER_SEC: i64 = vsf::OSCILLATIONS_PER_SECOND as i64;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Peer expiry: 7 days
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub const PEER_EXPIRY_OSC: i64 = 604_800 * OSC_PER_SEC;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// K-bucket stale entry eviction: 1 hour
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub const KBUCKET_STALE_OSC: i64 = 3_600 * OSC_PER_SEC;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// Logging - feature-gated, compiles to nothing without --features logging
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// - Android: log::info!
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// - Windows: %APPDATA%\photon\photon.log
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// - Other: stdout
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Severity of a structured log record. The discriminant IS the on-disk `lvl` value in the VSF log, so these numbers are wire-stable — append new levels at the end, never renumber.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[derive(Clone, Copy)]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub enum LogLevel {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    Trace = 0,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    Debug = 1,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    Info = 2,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    Warn = 3,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    Error = 4,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Retained for the desktop/Windows `main()` call site. The VSF file sink now opens LAZILY on the first log after the platform data dir is known (Android sets it partway thru JNI startup), so this is a no-op — kept only so existing callers compile.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn init_logging() {}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Short, non-PII label for logs: the first 4 bytes of a PUBLIC id (handle_proof / device pubkey) as hex.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Log this instead of a plaintext handle — the durable log then carries pseudonymous identifiers, never names, so it stays diagnostic (you can correlate a fingerprint across a run) without leaking who anyone is.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// The dozenal digit NAMES, digit 0..11 — Zil(0)/Zila(1)/Zilor(2)/Ter(3)/Tera(4)/Teror(5)/Lun(6)/Luna(7)/Lunor(8)/Stel(9)/Stela(10)/Stelor(11); the same set the Oxanium `+glyphs` face draws at 0x10..0x1B. UI shows the GLYPHS, logs/read-aloud show these WORDS. Never arabic.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub const DOZENAL_NAMES: [&str; 12] = [
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    "Zil", "Zila", "Zilor", "Ter", "Tera", "Teror", "Lun", "Luna", "Lunor", "Stel", "Stela",
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    "Stelor",
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
];
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Session mirror of the fleet-wide `display.dozenal` setting (the About-page toggle) — a static so render-edge formatters read the base without threading `&self` everywhere. Synced wherever fleet_settings loads or the toggle flips. Default TRUE: dozenal is the house base (binary at rest, base chosen at the render edge; arabic never by preference).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub static DOZENAL_UI: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn dozenal_ui() -> bool {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    DOZENAL_UI.load(std::sync::atomic::Ordering::Relaxed)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn set_dozenal_ui(on: bool) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    DOZENAL_UI.store(on, std::sync::atomic::Ordering::Relaxed);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Base-aware integer render for the UI: dozenal glyphs (Oxanium `+glyphs` face REQUIRED at the draw site) or arabic decimal, per the toggle.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn fmt_num(n: u32) -> String {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    if dozenal_ui() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        dozenal_glyphs(n)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    } else {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        n.to_string()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Render `n` in dozenal as reserved control-code bytes 0x10+digit — the Oxanium `+glyphs` face draws them as the dozenal digits. UI-only: terminals show garbage, so LOG paths use [`dozenal_words`] instead.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn dozenal_glyphs(mut n: u32) -> String {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    if n == 0 {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        return char::from(0x10).to_string();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let mut digits = Vec::new();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    while n > 0 {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        digits.push(char::from(0x10 + (n % 12) as u8));
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        n /= 12;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    digits.iter().rev().collect()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Dozenal-glyph SERIALIZATION of a non-negative i64 — same byte convention as [`dozenal_glyphs`] (0x10..=0x1B = digits 0..11, most-significant first), widened for eagle times. THE encoding for numbers inside content-string markers (the reply/edit/react references): ASCII decimal never enters a row (AGENT.md — the `s{idx}_` concatenation shape is forbidden), and a client that predates a marker renders photon's own numerals instead of arabic droppings. Negative input clamps to zero (eagle times are non-negative; a clamped reference simply resolves to nothing).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn dozenal_bytes(n: i64) -> String {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let mut n = n.max(0) as u64;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    if n == 0 {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        return char::from(0x10).to_string();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let mut digits = Vec::new();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    while n > 0 {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        digits.push(char::from(0x10 + (n % 12) as u8));
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        n /= 12;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    digits.iter().rev().collect()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Parse a [`dozenal_bytes`] number. None on empty input, any byte outside the digit block, or overflow — marker parsers treat that as "not a reference".
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn parse_dozenal_bytes(s: &str) -> Option<i64> {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    if s.is_empty() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        return None;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let mut n: i64 = 0;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    for c in s.chars() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let d = (c as u32).checked_sub(0x10)?;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        if d > 11 {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            return None;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        n = n.checked_mul(12)?.checked_add(d as i64)?;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    Some(n)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(test)]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
mod dozenal_serialization_tests {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    /// The marker-reference number codec: round-trips at eagle-time scale, zero included, and REJECTS arabic — a decimal string must read as "not a reference", never mis-parse.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    #[test]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    fn dozenal_bytes_round_trip_and_arabic_rejected() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        for n in [0i64, 1, 11, 12, 143, 1_000_000, i64::MAX / 2] {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            assert_eq!(
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                super::parse_dozenal_bytes(&super::dozenal_bytes(n)),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                Some(n)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            );
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        assert!(super::dozenal_bytes(7)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .chars()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .all(|c| (c as u32) >= 0x10 && (c as u32) <= 0x1B));
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        assert_eq!(super::parse_dozenal_bytes("1234"), None);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        assert_eq!(super::parse_dozenal_bytes(""), None);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        assert_eq!(super::dozenal_bytes(-5), super::dozenal_bytes(0));
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Spell `n` in dozenal digit words, space-separated ("Zilor Stela") — the read-aloud form.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn dozenal_spell(mut n: u32) -> String {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    if n == 0 {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        return DOZENAL_NAMES[0].to_string();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let mut parts = Vec::new();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    while n > 0 {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        parts.push(DOZENAL_NAMES[(n % 12) as usize]);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        n /= 12;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    parts.iter().rev().copied().collect::<Vec<_>>().join(" ")
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Spell `n` in dozenal digit words, camelCase-concatenated ("ZilorStela") — the LOG/copy-paste form (double-click selects the whole value; terminals can't draw the glyph control codes).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn dozenal_words(mut n: u32) -> String {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    if n == 0 {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        return DOZENAL_NAMES[0].to_string();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let mut parts = Vec::new();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    while n > 0 {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        parts.push(DOZENAL_NAMES[(n % 12) as usize]);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        n /= 12;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    parts.iter().rev().copied().collect::<Vec<_>>().concat()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Transliterate any dozenal GLYPH control codes (0x10..0x1B) in a UI string into their camelCase digit words, for log emission — one string builder serves both surfaces.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn deglyph_for_log(s: &str) -> String {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let mut out = String::with_capacity(s.len());
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    for c in s.chars() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let cp = c as u32;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        if (0x10..=0x1B).contains(&cp) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            out.push_str(DOZENAL_NAMES[(cp - 0x10) as usize]);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        } else {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            out.push(c);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    out
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn fp(public_id: &[u8]) -> String {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    hex::encode(&public_id[..public_id.len().min(4)])
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// The log-submission encryption key: a ChaCha20-Poly1305 key derived from the identity seed ALONE — deliberately NOT folding in device_secret.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// The identity seed is deterministic from the handle, so anyone who knows the handle (the admin, handed one by a peer with a support request) can re-derive this key and open that peer's submitted log — while anyone who merely grabs the R2 ciphertext, not knowing whose it is, cannot. This is the whole "decryptable if you know the identity seed" property: the log is sealed on the client with this key before it ever leaves the device, so no plaintext hits the wire.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn log_encryption_key(identity_seed: &[u8; 32]) -> [u8; 32] {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let mut hasher = blake3::Hasher::new();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    hasher.update(identity_seed);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    hasher.update(b"photon.log.v0");
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    *hasher.finalize().as_bytes()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// The log RETRIEVAL tag: `spaghettify("photon_log_v1" ‖ identity_seed)`.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// A submitted log is stored on FGTW under this tag, and pulled by presenting it — so the tag is a *capability* derived one-way from the identity seed (which is deterministic from the handle). Whoever knows the seed can both FIND (this tag) and DECRYPT ([`log_encryption_key`]) the logs; whoever doesn't sees only opaque tags over ciphertext. spaghettify (not BLAKE3) matches the stack's one-way primitive and keeps the seed unrecoverable from the tag; the tag is what travels to the server, never the seed. Distinct domain tag from the encryption key so the two derivations can't collide.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn log_retrieval_tag(identity_seed: &[u8; 32]) -> [u8; 32] {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let mut input = Vec::with_capacity(13 + 32);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    input.extend_from_slice(b"photon_log_v1");
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    input.extend_from_slice(identity_seed);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    ihi::spaghettify(&input)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(test)]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
mod log_seal_tests {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    use super::*;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    #[test]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    fn seal_roundtrips_no_plaintext_and_rejects_wrong_seed() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let seed = [7u8; 32];
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let log = b"INFO  FGTW: announce port=4383 ip=redacted".as_slice();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let key = log_encryption_key(&seed);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let sealed = storage::encrypt_bytes(log, &key).unwrap();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        // No plaintext on the wire: the ciphertext must not contain the log bytes.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        assert!(!sealed.windows(log.len()).any(|w| w == log));
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        // The right seed opens it.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        assert_eq!(storage::decrypt_bytes(&sealed, &key).unwrap(), log);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        // A different seed derives a different key → AEAD auth failure, never a wrong plaintext.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let wrong = log_encryption_key(&[8u8; 32]);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        assert!(storage::decrypt_bytes(&sealed, &wrong).is_err());
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Stochastic pad for ANY periodic timer or age threshold: `base` scaled by a fresh random factor in [0.5, 1.0].
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Re-roll on every use. A fixed interval makes every client (and every subsystem) wake on the same tick — a routine timer becomes a synchronised network cascade (the thundering herd), e.g. everyone re-announcing exactly on the hour. Jittering each period spreads the load and makes accidental alignment vanishingly unlikely; the cost is a fuzzy deadline, which time-based housekeeping never needs exact.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn jitter(base: i64) -> i64 {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    (base as f64 * (0.5 + rand::random::<f64>() * 0.5)) as i64
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// [`jitter`] for `std::time::Duration` timers (sleeps, recv-timeouts, periodic loops).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn jitter_dur(base: std::time::Duration) -> std::time::Duration {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    base.mul_f64(0.5 + rand::random::<f64>() * 0.5)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// Disabled: compiles to nothing without --features logging.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(not(feature = "logging"))]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[inline(always)]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn log(_msg: &str) {}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(not(feature = "logging"))]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[inline(always)]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn log_at(_level: LogLevel, _msg: &str) {}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(not(feature = "logging"))]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[inline(always)]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn clear_log() {}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(not(feature = "logging"))]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[inline(always)]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn snapshot_log_bytes() -> Option<Vec<u8>> {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    None
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(not(feature = "logging"))]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[inline(always)]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn log_size_bytes() -> u64 {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    0
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Live size of the VSF log — the cheap "has anything been logged since?" probe (an atomic load + an uncontended lock, no I/O). File bytes PLUS the soft-mode RAM batch: records waiting in the batch are still records to submit, and counting only the file made the Submit pill grey out with a session's worth of unsent lines sitting in memory ("no logs to send"). The pill greys while this still equals the size captured at the last successful submit: identical size = identical bytes = a duplicate upload.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn log_size_bytes() -> u64 {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let pending = LOG_PENDING.lock().map(|p| p.len() as u64).unwrap_or(0);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    LOG_BYTES.load(std::sync::atomic::Ordering::Relaxed) + pending
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// The structured VSF log sink: one COMPLETE VSF record per line — {creation_time (Eagle), section "log" {lvl, msg}} — appended to `photon.log.vsf` in `log_dir()` (Android: external filesDir, pullable via `adb pull`; desktop: the OS temp dir — volatile diagnostics, see log_dir). The log is thus a stream of self-describing, Eagle-time-stamped, vsfinfo-inspectable records; read it with the `photonlog` bin. Opens lazily and RETRIES until the dir is ready — a plain Mutex<Option<File>>, NOT a OnceLock, precisely so a pre-data-dir failure isn't cached forever (the first Kotlin/JNI lines predate Android's data_dir; they buffer in LOG_PENDING below and flush when the file opens).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// Known filename (logging is a dev-build feature, so adb-pull discoverability beats filename privacy).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
static LOG_FILE: std::sync::Mutex<Option<std::fs::File>> = std::sync::Mutex::new(None);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// Records that arrive before the sink can open (Android: everything logged before the JNI data dir lands, including the Kotlin bridge's earliest lifecycle lines) — held as already-built VSF record bytes so their creation stamps stay true, drained into the file the moment it opens. Bounded so a never-initializing process can't grow it unbounded; overflow drops the newest record (the earliest lines are the ones worth keeping).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
static LOG_PENDING: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(Vec::new());
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// The pending buffer doubles as the SOFT-mode batch (see LOG_HARD), so the cap is sized for that role; the pre-data-dir case it originally served never nears it.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
const LOG_PENDING_CAP: usize = 4 << 20;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Soft-mode batch: buffered records write thru in ONE chunk at this size — an idle session writes nothing, a busy one writes rarely and large (flash wear tracks write COUNT more than byte count).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
const SOFT_LOG_FLUSH_BYTES: usize = 512 << 10;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Hard-logs deadline in Eagle oscillations (0 = soft). ON = write thru per record (crash-durable via the kernel page cache); OFF (the default) = records batch in RAM and reach disk on the EDGES — panic, app background, submission, threshold, arming — so steady-state logging costs the disk nothing.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// DEVICE-LOCAL and SELF-EXPIRING: the Diagnostics checkbox arms THIS device for 24h (`LOG_AGE_TRIGGER_BASE_OSC`), because an investigation concerns one piece of hardware and nobody remembers to untick. Expiry is evaluated lazily at each write — no timer; the first record past the deadline simply batches again.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
static LOG_HARD_UNTIL: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Arm (Some(arm_time_osc)) or disarm (None) hard logging. Arming is a flush edge — the batch drains so the durable record starts complete.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn set_hard_logs(armed_at: Option<i64>) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let deadline = armed_at
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        .map(|t| t.saturating_add(LOG_AGE_TRIGGER_BASE_OSC))
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        .unwrap_or(0);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let was = LOG_HARD_UNTIL.swap(deadline, std::sync::atomic::Ordering::Relaxed);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    if deadline > was {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        flush_log_buffer();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(not(feature = "logging"))]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn set_hard_logs(_armed_at: Option<i64>) {}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// True while the 24h hard-logs window is open — the UI reads the checkbox state from here so sink and display can't disagree.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn hard_logs_active() -> bool {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    LOG_HARD_UNTIL.load(std::sync::atomic::Ordering::Relaxed) > vsf::eagle_time_oscillations()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(not(feature = "logging"))]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn hard_logs_active() -> bool {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    false
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// Size cap for the VSF log: once the file passes 16 MiB, drop enough of the OLDEST whole records to bring it back to ~8 MiB.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// Trimming cuts only on record boundaries (the file is a stream of complete VSF records), so the result stays fully decodable by photonlog.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
const LOG_CAP_BYTES: u64 = 16 << 20;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
const LOG_TRIM_TO_BYTES: u64 = 8 << 20;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// Live byte count of the open log file, so the cap check is a cheap atomic load instead of a stat per line; seeded from the file's size when it's first opened.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
static LOG_BYTES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// Age cap with hysteresis + jitter: let the log reach ~2 days, THEN cut it back to ~the most recent 24h.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// Hysteresis avoids "persistent scrubbing" (a trim-at-exactly-24h rewrites the file on nearly every line of a steady low-volume log). Jitter avoids synchronised cascades: the trigger fires at a random 24–48h and the keep window is a random 12–24h, re-rolled each trim, so no two devices (and no two of our subsystems) trim on the same instant.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
const LOG_AGE_TRIGGER_BASE_OSC: i64 = 2 * 24 * 60 * 60 * vsf::OSCILLATIONS_PER_SECOND as i64; // jittered → 24–48h
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
const LOG_AGE_KEEP_BASE_OSC: i64 = 24 * 60 * 60 * vsf::OSCILLATIONS_PER_SECOND as i64; // jittered → 12–24h
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                                                                                       // The currently-chosen (jittered) trigger threshold; re-rolled on open and after each trim.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
static LOG_AGE_TRIGGER_OSC: std::sync::atomic::AtomicI64 =
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    std::sync::atomic::AtomicI64::new(LOG_AGE_TRIGGER_BASE_OSC);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// Eagle-time of the OLDEST record in the open file, cached so the age check is a compare not a head-read per line; i64::MAX = unknown/empty (seeded on open, refreshed on trim).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
static LOG_OLDEST_OSC: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(i64::MAX);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Android-only override for where the VSF log goes — set from JNI to the EXTERNAL files dir (the shadow ring dir), which is adb-readable on a non-debuggable release dev APK where internal `files/` is not.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(all(feature = "logging", target_os = "android"))]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
static ANDROID_LOG_DIR: std::sync::OnceLock<String> = std::sync::OnceLock::new();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(all(feature = "logging", target_os = "android"))]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn set_android_log_dir(dir: String) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    if !dir.is_empty() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let _ = ANDROID_LOG_DIR.set(dir);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Directory the VSF log file lives in. Android prefers the JNI-set external dir (pullable); everything else uses `photon_config_dir`.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub(crate) fn log_dir() -> Option<std::path::PathBuf> {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    // Android stays on the external files dir: apps get no tmpfs (cacheDir is the SAME flash, so zero wear saved) and adb-readability on release APKs is load-bearing for diagnostics.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    #[cfg(target_os = "android")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    if let Some(d) = ANDROID_LOG_DIR.get() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        return Some(std::path::PathBuf::from(d));
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    // Desktop logs are VOLATILE by choice (user call 2026-08-01): diagnostics rarely need to survive a reboot or a temp clean, so they live in the OS temp dir — tmpfs (RAM, zero disk wear) on most Linux, per-user OS-cleaned temp on macOS/Windows. Per-user suffix because Linux /tmp is shared. Falls back to the config dir if temp isn't writable.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    #[cfg(not(target_os = "android"))]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let user = std::env::var("USER")
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .or_else(|_| std::env::var("USERNAME"))
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .unwrap_or_else(|_| "shared".into());
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let dir = std::env::temp_dir().join(format!("photon-{user}"));
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        if std::fs::create_dir_all(&dir).is_ok() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            // One-shot migration: drop the old persistent-dir log so it doesn't sit stale forever (records are ephemeral diagnostics — nothing worth carrying over).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            static OLD_LOG_SWEPT: std::sync::Once = std::sync::Once::new();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            OLD_LOG_SWEPT.call_once(|| {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                if let Ok(old) = crate::storage::photon_config_dir() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    let _ = std::fs::remove_file(old.join("photon.log.vsf"));
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            });
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            return Some(dir);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    crate::storage::photon_config_dir().ok()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Open the sink (idempotent) and drain any buffered records into it FIRST so the file stays chronological; counters seed from post-drain metadata.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
fn ensure_log_open(guard: &mut Option<std::fs::File>) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    use std::io::Write;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    if guard.is_some() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        return;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let Some(dir) = log_dir() else { return };
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let _ = std::fs::create_dir_all(&dir);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let path = dir.join("photon.log.vsf");
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let mut opts = std::fs::OpenOptions::new();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    opts.create(true).append(true);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    // FILE_ATTRIBUTE_TEMPORARY (0x100): the cache manager keeps the contents in RAM and skips lazy writeback unless memory pressure forces it — the file still exists, survives process exit, and reads back for submission, it just avoids physically wearing the disk while it can. Windows' answer to the tmpfs macOS doesn't have.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    #[cfg(windows)]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        use std::os::windows::fs::OpenOptionsExt;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        opts.attributes(0x100);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    if let Ok(mut f) = opts.open(&path) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        if let Ok(mut pending) = LOG_PENDING.lock() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            if !pending.is_empty() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                let _ = f.write_all(&pending);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                pending.clear();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                pending.shrink_to_fit();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let sz = f.metadata().map(|m| m.len()).unwrap_or(0);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        LOG_BYTES.store(sz, std::sync::atomic::Ordering::Relaxed);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        LOG_OLDEST_OSC.store(
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            first_record_osc(&path).unwrap_or(i64::MAX),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            std::sync::atomic::Ordering::Relaxed,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        );
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        LOG_AGE_TRIGGER_OSC.store(
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            jitter(LOG_AGE_TRIGGER_BASE_OSC),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            std::sync::atomic::Ordering::Relaxed,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        );
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        *guard = Some(f);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Trim on EITHER cap: too big (16 MiB) or the oldest record past the jittered age trigger (24–48h). Reopens the handle on the trimmed file and re-rolls the trigger so successive trims never settle into a fixed cadence.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
fn trim_log_if_due(guard: &mut Option<std::fs::File>) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let total = LOG_BYTES.load(std::sync::atomic::Ordering::Relaxed);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let now = vsf::eagle_time_oscillations();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let oldest = LOG_OLDEST_OSC.load(std::sync::atomic::Ordering::Relaxed);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let trigger = LOG_AGE_TRIGGER_OSC.load(std::sync::atomic::Ordering::Relaxed);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let aged = oldest != i64::MAX && now.saturating_sub(oldest) > trigger;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    if total > LOG_CAP_BYTES || aged {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        if let Some((trimmed, new_size, new_oldest)) = trim_log_file(now) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            *guard = Some(trimmed);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            LOG_BYTES.store(new_size, std::sync::atomic::Ordering::Relaxed);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            LOG_OLDEST_OSC.store(new_oldest, std::sync::atomic::Ordering::Relaxed);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            LOG_AGE_TRIGGER_OSC.store(
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                jitter(LOG_AGE_TRIGGER_BASE_OSC),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                std::sync::atomic::Ordering::Relaxed,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            );
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Drain the soft-mode RAM batch to disk — THE edge reaction (panic, app background, submission, threshold, hard-toggle). No-op when nothing is buffered.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Write a panic verbatim to `photon.crash.txt`, touching NO shared lock. The panic hook cannot report thru the normal sink when the panic came from INSIDE it: `log::error!` takes LOG_PENDING and `flush_log_buffer` takes LOG_FILE, both non-reentrant, so the same thread re-locking them deadlocks — the process then dies to the OS watchdog with an empty log, which is exactly the signature of the 2026-08-07 phone death (hook installed, zero PHOTON PANIC lines). A fresh handle on its own path always writes.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn write_crash_sidecar(text: &str) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    use std::io::Write;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let Some(dir) = log_dir() else { return };
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let _ = std::fs::create_dir_all(&dir);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    if let Ok(mut f) = std::fs::OpenOptions::new()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        .create(true)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        .append(true)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        .open(dir.join("photon.crash.txt"))
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let _ = writeln!(f, "{}", text);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let _ = f.flush();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Fold last run's crash sidecar into THIS run's log, then delete it — a crash report is worthless if it never reaches a submission. Called once at startup, right after the hook is installed.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn report_prior_crash() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let Some(dir) = log_dir() else { return };
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let path = dir.join("photon.crash.txt");
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let Ok(text) = std::fs::read_to_string(&path) else {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        return;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    };
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let _ = std::fs::remove_file(&path);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        logf!("PRIOR RUN DIED: {}", line);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn flush_log_buffer() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    use std::io::Write;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let Ok(mut guard) = LOG_FILE.lock() else {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        return;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    };
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    ensure_log_open(&mut guard);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let mut drained = 0usize;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    if let Some(f) = guard.as_mut() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        if let Ok(mut pending) = LOG_PENDING.lock() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            if !pending.is_empty() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                if f.write_all(&pending).is_ok() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    drained = pending.len();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                pending.clear();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                pending.shrink_to_fit();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    if drained > 0 {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        LOG_BYTES.fetch_add(drained as u64, std::sync::atomic::Ordering::Relaxed);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        // A file born from this drain has no seeded oldest — these records are it.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let _ = LOG_OLDEST_OSC.compare_exchange(
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            i64::MAX,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            vsf::eagle_time_oscillations(),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            std::sync::atomic::Ordering::Relaxed,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            std::sync::atomic::Ordering::Relaxed,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        );
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        trim_log_if_due(&mut guard);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(not(feature = "logging"))]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn flush_log_buffer() {}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
fn append_log_record(level: LogLevel, msg: &str, vals: &[LogValue]) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    use std::io::Write;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    // Build first so a buffered record carries the stamp of when it was LOGGED, not when the sink finally opened. `msg` is the pure-text template; each captured value rides as its own TYPED `val` field, in slot order — a number never stringifies into the record (numbers-binary-at-rest).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let mut section = vsf::VsfSection::new("log");
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    section.add_field_multi("lvl", vec![vsf::VsfType::u(level as usize, false)]);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    section.add_field_multi("msg", vec![vsf::VsfType::x(msg.to_string())]);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    for v in vals {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let t = match v {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            LogValue::U(n) => vsf::VsfType::u(*n as usize, false),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            LogValue::Z(n) => vsf::VsfType::z(*n as usize),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            LogValue::I(n) => vsf::VsfType::i(*n as isize),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            LogValue::F(n) => vsf::VsfType::f6(*n),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            LogValue::B(b) => vsf::VsfType::u(*b as usize, false),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            LogValue::T(s) => vsf::VsfType::x(s.clone()),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            LogValue::Addr(a) => vsf::VsfType::v_u3(vsf::types::Vector {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                data: {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    let mut b = match a.ip() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                        std::net::IpAddr::V4(v4) => v4.octets().to_vec(),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                        std::net::IpAddr::V6(v6) => v6.octets().to_vec(),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    };
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    b.extend_from_slice(&a.port().to_le_bytes());
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    b
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                },
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            }),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        };
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        section.add_field_multi("val", vec![t]);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let record = vsf::VsfBuilder::new()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        .creation_time_oscillations(vsf::eagle_time_oscillations())
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        .provenance_only()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        .add_section_direct(section)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        .build();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let Ok(mut guard) = LOG_FILE.lock() else {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        return;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    };
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    // SOFT mode (the default, and hard mode past its 24h deadline): batch in RAM; the batch reaches disk on the edges (panic / background / submit / threshold / arming) via flush_log_buffer.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    if LOG_HARD_UNTIL.load(std::sync::atomic::Ordering::Relaxed) <= vsf::eagle_time_oscillations() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let over = if let (Ok(bytes), Ok(mut pending)) = (&record, LOG_PENDING.lock()) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            if pending.len() + bytes.len() <= LOG_PENDING_CAP {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                pending.extend_from_slice(bytes);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            pending.len() >= SOFT_LOG_FLUSH_BYTES
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        } else {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            false
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        };
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        drop(guard);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        if over {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            flush_log_buffer();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        return;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    ensure_log_open(&mut guard);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let Some(file) = guard.as_mut() else {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        // No sink yet (Android before the JNI data dir lands): hold the built record so the earliest lines aren't lost.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        if let (Ok(bytes), Ok(mut pending)) = (&record, LOG_PENDING.lock()) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            if pending.len() + bytes.len() <= LOG_PENDING_CAP {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                pending.extend_from_slice(bytes);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        return;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    };
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    if let Ok(bytes) = record {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let _ = file.write_all(&bytes);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let _ = file.flush();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        LOG_BYTES.fetch_add(bytes.len() as u64, std::sync::atomic::Ordering::Relaxed);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        // `file`'s borrow of `guard` ends above; the trim reopens the handle on the trimmed file.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        trim_log_if_due(&mut guard);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// Trim the log by dropping the oldest whole records — enough to get under `LOG_TRIM_TO_BYTES` AND to drop anything older than 24h — then reopen it for appending.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// The file is a stream of complete VSF records, so we cut only on record boundaries (never mid-record). Returns the reopened append handle, the kept byte count, and the new oldest-record time; None if the file couldn't be read/rewritten (the cap check just retries next line).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
fn trim_log_file(now_osc: i64) -> Option<(std::fs::File, u64, i64)> {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    use std::io::Write;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let path = log_dir()?.join("photon.log.vsf");
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    // Defence in depth (unix): hold an advisory lock across the read-truncate-rewrite, so even two processes sharing one log — which the single-instance lock already forbids — can't interleave a trim and clobber each other. LOCK_NB: if another process is mid-trim, skip and retry on the next line.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    #[cfg(unix)]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let _trim_lock = {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        use std::os::unix::io::AsRawFd;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let lf = std::fs::OpenOptions::new()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .create(true)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .write(true)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .open(&path)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .ok()?;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        if unsafe { libc::flock(lf.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            return None;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        lf // held until this fn returns (drop closes the fd → releases the lock)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    };
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let bytes = std::fs::read(&path).ok()?;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let age_cutoff = now_osc.saturating_sub(jitter(LOG_AGE_KEEP_BASE_OSC)); // keep a random 12–24h
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let (keep, new_oldest) = log_keep_offset(&bytes, LOG_TRIM_TO_BYTES, age_cutoff);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let kept = &bytes[keep.min(bytes.len())..];
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let mut w = std::fs::OpenOptions::new()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        .create(true)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        .write(true)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        .truncate(true)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        .open(&path)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        .ok()?;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    w.write_all(kept).ok()?;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    w.flush().ok()?;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    drop(w);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let appender = std::fs::OpenOptions::new()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        .create(true)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        .append(true)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        .open(&path)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        .ok()?;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    Some((appender, kept.len() as u64, new_oldest))
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// Pure boundary finder: the first whole-record boundary to keep so that `bytes[offset..]` is both within `trim_to_size` bytes AND free of records older than `age_cutoff_osc`.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// Records are appended in time order, so we drop from the front while a record is EITHER before the size-drop point OR older than the cutoff, stopping at the first record that satisfies both.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// Returns (keep_offset, oldest_kept_time) — the second value re-seeds LOG_OLDEST_OSC. Stops early on any decode error so a corrupt tail never causes a mid-record cut.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
fn log_keep_offset(bytes: &[u8], trim_to_size: u64, age_cutoff_osc: i64) -> (usize, i64) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let total = bytes.len();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let size_drop = (total as u64).saturating_sub(trim_to_size) as usize;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let mut offset = 0usize;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    while offset < total {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let rest = &bytes[offset..];
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let (header, header_end) = match vsf::file_format::VsfHeader::decode(rest) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            Ok(h) => h,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            Err(_) => return (offset, i64::MAX),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        };
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let mut ptr = 0usize;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        if vsf::file_format::VsfSection::parse(&rest[header_end..], &mut ptr).is_err() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            return (offset, i64::MAX);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let rec = header_end + ptr;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        if rec == 0 {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            return (offset, i64::MAX);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let t = match &header.creation_time {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            Some(vsf::VsfType::e(et)) => et_to_osc_log(et),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            _ => i64::MIN, // no/odd timestamp → treat as ancient so it's eligible to drop
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        };
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        // Keep from the first record that is past the size-drop point AND fresh enough.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        if offset >= size_drop && t >= age_cutoff_osc {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            return (offset, t);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        offset += rec;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    (total, i64::MAX) // everything dropped → empty
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// Eagle oscillations from a log record's creation-time field.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
fn et_to_osc_log(et: &vsf::types::EtType) -> i64 {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    use vsf::types::EtType;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    match et {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        EtType::e5(o) => *o as i64,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        EtType::e6(o) => *o,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        EtType::e7(o) => *o as i64,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        _ => i64::MIN,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// The oldest (first) record's eagle-time in a log file, by decoding just its header. None if empty/unreadable.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
fn first_record_osc(path: &std::path::Path) -> Option<i64> {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    use std::io::Read;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let mut buf = vec![0u8; 4096];
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let n = std::fs::File::open(path).ok()?.read(&mut buf).ok()?;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    if n == 0 {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        return None;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let (header, _) = vsf::file_format::VsfHeader::decode(&buf[..n]).ok()?;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    match &header.creation_time {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        Some(vsf::VsfType::e(et)) => Some(et_to_osc_log(et)),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        _ => None,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Wipe the durable log (the `[]x` clean-relaunch chord, and any future privacy "clear logs" action).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Removes `photon.log.vsf` and drops the open handle so the next write reopens a fresh, empty file.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn clear_log() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    if let Ok(mut guard) = LOG_FILE.lock() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        if let Some(dir) = log_dir() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            let _ = std::fs::remove_file(dir.join("photon.log.vsf"));
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        *guard = None;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        LOG_BYTES.store(0, std::sync::atomic::Ordering::Relaxed);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        LOG_OLDEST_OSC.store(i64::MAX, std::sync::atomic::Ordering::Relaxed);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    // The soft-mode batch goes with the file — a clear that left buffered records would resurrect them on the next flush.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    if let Ok(mut pending) = LOG_PENDING.lock() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        pending.clear();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        pending.shrink_to_fit();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Reads the current `photon.log.vsf` as raw bytes for submission (the "Submit" diagnostic action).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Submission is a flush edge: the soft-mode batch drains first so the snapshot carries everything up to this moment. `None` if the log has nothing or can't be read.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn snapshot_log_bytes() -> Option<Vec<u8>> {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    flush_log_buffer();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let path = log_dir()?.join("photon.log.vsf");
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    std::fs::read(&path).ok().filter(|b| !b.is_empty())
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(all(test, feature = "logging"))]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
mod log_cap_tests {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    use super::*;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    fn record(msg: &str) -> Vec<u8> {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        record_at(msg, vsf::eagle_time_oscillations())
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    fn record_at(msg: &str, osc: i64) -> Vec<u8> {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        vsf::VsfBuilder::new()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .creation_time_oscillations(osc)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .provenance_only()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .add_section(
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                "log",
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                vec![
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    ("lvl".to_string(), vsf::VsfType::u(2, false)),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    ("msg".to_string(), vsf::VsfType::x(msg.to_string())),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                ],
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            )
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .build()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .unwrap()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    #[test]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    fn trim_cuts_on_a_record_boundary_and_stays_decodable() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        // Build a stream of records and remember each record's start offset.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let mut bytes = Vec::new();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let mut starts = vec![0usize];
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        for i in 0..50 {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            bytes.extend_from_slice(&record(&format!("message number {i}")));
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            starts.push(bytes.len());
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let trim_to = (bytes.len() / 3) as u64; // keep roughly the newest third
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let (keep, _oldest) = log_keep_offset(&bytes, trim_to, i64::MIN); // age disabled → size-only
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        // The cut lands exactly on a record boundary...
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        assert!(
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            starts.contains(&keep),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            "cut at {keep} is not a record boundary"
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        );
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        assert!(keep > 0, "should have dropped something");
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        // ...the kept tail is no larger than the target (we keep from the FIRST boundary past the drop point)...
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let kept = &bytes[keep..];
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        assert!(kept.len() as u64 <= trim_to && !kept.is_empty());
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        // ...and decodes cleanly as whole records right up to EOF (no half record left at the front).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let mut off = 0usize;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let mut n = 0;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        while off < kept.len() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            let (_, he) = vsf::file_format::VsfHeader::decode(&kept[off..]).unwrap();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            let mut p = 0usize;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            vsf::file_format::VsfSection::parse(&kept[off + he..], &mut p).unwrap();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            off += he + p;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            n += 1;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        assert_eq!(
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            off,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            kept.len(),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            "kept tail must end exactly on a record boundary"
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        );
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        assert!(n > 0);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    #[test]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    fn no_trim_when_under_target() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let bytes = record("solo"); // one record, well under the size cap
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let (keep, _oldest) = log_keep_offset(&bytes, 16 << 20, i64::MIN); // age disabled
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        assert_eq!(keep, 0, "nothing dropped");
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    #[test]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    fn age_cap_drops_records_older_than_cutoff() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        // Ten "old" records at t=1000, then ten "new" at t=9000. Size is generous; only age should trim.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let mut bytes = Vec::new();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        for i in 0..10 {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            bytes.extend_from_slice(&record_at(&format!("old {i}"), 1000));
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let new_start = bytes.len();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        for i in 0..10 {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            bytes.extend_from_slice(&record_at(&format!("new {i}"), 9000));
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        // Cutoff between the two cohorts: drop everything older than 5000.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let (keep, oldest) = log_keep_offset(&bytes, 64 << 20, 5000);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        assert_eq!(keep, new_start, "should drop exactly the old cohort");
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        assert_eq!(oldest, 9000, "oldest kept record is the first new one");
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        // The kept tail is all the new records, decodes clean to EOF.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let kept = &bytes[keep..];
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let mut off = 0;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        while off < kept.len() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            let (_, he) = vsf::file_format::VsfHeader::decode(&kept[off..]).unwrap();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            let mut p = 0;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            vsf::file_format::VsfSection::parse(&kept[off + he..], &mut p).unwrap();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            off += he + p;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        assert_eq!(off, kept.len());
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// The structured VSF file is the ONE durable log — read it live with `photonlog -f`, off a phone with `adb pull`.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// No console mirror: stdout/logcat was redundant noise once everything lands in photon.log.vsf.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn log(msg: &str) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    log_at(LogLevel::Info, msg);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn log_at(level: LogLevel, msg: &str) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    append_log_record(level, msg, &[]);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// ── Structured logging (numbers-binary-at-rest): the record stores the message TEMPLATE as pure text and every interpolated value as a TYPED `val` field beside it — a number never stringifies into storage; photonlog/vsfinfo choose the display base at READ time. Use `logf!`/`logf_at!` (format!-shaped) instead of `log(&format!(...))`. ──
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// One captured log value, typed — becomes a native VSF field in the record.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub enum LogValue {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    U(u128),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    I(i128),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    F(f64),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    B(bool),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    T(String),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    Addr(std::net::SocketAddr),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    /// A version component — emits the VSF `z` (version) type, not a plain integer. Carried by [`Ver`].
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    Z(u128),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Wrap a version-number component so `logf!` captures it as VSF `z` (the version type) rather than a plain `u`. `logf!("v{}.{}.{}", Ver(maj), Ver(min), Ver(pat))` renders normally and stores three `z` fields.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[derive(Clone, Copy)]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub struct Ver(pub u64);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
impl std::fmt::Display for Ver {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        write!(f, "{}", self.0)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
impl CapPrim for Ver {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    fn to_log(self) -> LogValue {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        LogValue::Z(self.0 as u128)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Capture wrapper for `logf!` args. Inherent impls (numerics, bools, addresses) outrank the [`CapDisplay`] blanket at method resolution, so typed capture is automatic and everything else degrades to text — the autoref-free inherent-priority specialization.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub struct Cap<T>(pub T);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Lowest-priority capture: anything Display becomes text (prose, hex labels, hashes — nouns). Implemented on `&Cap<T>` so the typed inherent impls on `Cap<X>` win the method probe without ambiguity (autoref specialization); the macro's `Cap(&arg)` form means args are only ever borrowed.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub trait CapDisplay {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    fn cap(self) -> LogValue;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
impl<T: std::fmt::Display> CapDisplay for &Cap<T> {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    fn cap(self) -> LogValue {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        LogValue::T(self.0.to_string())
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Primitive-typed capture — one impl per primitive, unified under ONE generic inherent impl on [`Cap`] so integer-literal inference resolves (a per-type inherent zoo made `{integer}` ambiguous).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub trait CapPrim: Copy {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    fn to_log(self) -> LogValue;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
macro_rules! cap_prim {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    ($($t:ty => $variant:ident as $conv:ty),* $(,)?) => {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        $(impl CapPrim for $t {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            fn to_log(self) -> LogValue { LogValue::$variant(self as $conv) }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        })*
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    };
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
cap_prim! {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    u8 => U as u128, u16 => U as u128, u32 => U as u128, u64 => U as u128, u128 => U as u128, usize => U as u128,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    i8 => I as i128, i16 => I as i128, i32 => I as i128, i64 => I as i128, i128 => I as i128, isize => I as i128,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    f32 => F as f64, f64 => F as f64,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
impl CapPrim for bool {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    fn to_log(self) -> LogValue {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        LogValue::B(self)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
impl CapPrim for std::net::SocketAddr {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    fn to_log(self) -> LogValue {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        LogValue::Addr(self)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
impl<T: CapPrim> Cap<&T> {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    pub fn cap(self) -> LogValue {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        (*self.0).to_log()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Render a template + captured values for a TERMINAL/console surface (photonlog shares the same walk). Slots `{}`/`{spec}` substitute values in order (spec is a rendering hint only); numbers render in current mixed arabic units per the display doctrine — the point is they were STORED binary. `{{`/`}}` are literal braces.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn render_log_line(template: &str, vals: &[LogValue]) -> String {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let mut out = String::with_capacity(template.len() + vals.len() * 8);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let mut chars = template.chars().peekable();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let mut next = 0usize;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    while let Some(c) = chars.next() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        match c {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            '{' if chars.peek() == Some(&'{') => {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                chars.next();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                out.push('{');
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            '}' if chars.peek() == Some(&'}') => {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                chars.next();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                out.push('}');
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            '{' => {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                // Consume to the closing brace (spec ignored at render).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                for s in chars.by_ref() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    if s == '}' {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                        break;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                if let Some(v) = vals.get(next) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    match v {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                        LogValue::U(n) => out.push_str(&n.to_string()),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                        LogValue::Z(n) => out.push_str(&n.to_string()),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                        LogValue::I(n) => out.push_str(&n.to_string()),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                        LogValue::F(n) => out.push_str(&n.to_string()),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                        LogValue::B(b) => out.push_str(if *b { "true" } else { "false" }),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                        LogValue::T(s) => out.push_str(s),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                        LogValue::Addr(a) => out.push_str(&a.to_string()),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                next += 1;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            c => out.push(c),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    out
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// One decoded log record — the shared decode shape for the `photonlog` bin and the in-app Diagnostics log viewer.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[derive(Clone, Debug)]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub struct LogRecord {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    /// Record creation time, eagle oscillations (0 = the record carried none).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    pub osc: i64,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    /// Severity 0..=4 (TRACE..ERROR); u64::MAX = the record carried none.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    pub level: u64,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    /// The rendered message: the stored template with its typed `val` fields substituted at READ time (numbers live binary in the record; this is the display edge).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    pub msg: String,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    /// The record's raw bytes — one complete VSF document, so the in-app viewer can hand it to `vsf::inspect_vsf` for the coloured structural view. photonlog ignores it.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    pub raw: Vec<u8>,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Decode complete records from a `photon.log.vsf` byte stream (each record = one full VSF file: {creation_time (Eagle), section "log" {lvl, msg, val*}}). Returns the records plus the byte offset of the last COMPLETE record boundary — a half-written trailing record (mid-append) is left for the next pass instead of being mis-decoded. Shared by the `photonlog` bin and the in-app viewer, so the two surfaces can never drift.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn parse_log_records(buf: &[u8]) -> (Vec<LogRecord>, usize) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    use vsf::file_format::{VsfHeader, VsfSection};
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    use vsf::types::EtType;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    use vsf::VsfType;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let mut records = Vec::new();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let mut off = 0usize;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    // TORN-RECORD RESYNC (field 2026-08-24): a kill mid-append leaves a half-record, and stopping at it hid THREE boots of records behind one tear — the file grew while every reader called it frozen. Garbage never decodes, so a failed record skips to the next record magic and the walk continues; a genuinely incomplete TAIL has no further magic, so the pass still ends with `consumed` at the last whole record exactly as before.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let resync = |from: usize| -> Option<usize> {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        buf[from..]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .windows(4)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .position(|w| w == [0x52, 0xC3, 0x85, 0x3C])
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .map(|d| from + d)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    };
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    while off < buf.len() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let rest = &buf[off..];
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let Ok((header, header_end)) = VsfHeader::decode(rest) else {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            match resync(off + 1) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                Some(next) => {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    off = next;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    continue;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                None => break, // incomplete tail — stop, retry next pass
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        };
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let mut ptr = 0usize;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let Ok(section) = VsfSection::parse(&rest[header_end..], &mut ptr) else {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            match resync(off + 1) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                Some(next) => {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    off = next;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    continue;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                None => break,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        };
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let rec = header_end + ptr;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        if rec == 0 {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            break;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let level = section
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .get_field("lvl")
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .and_then(|f| f.values.first())
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .and_then(|v| {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                use vsf::schema::FromVsfType;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                u64::from_vsf_type(v).ok()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            })
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .unwrap_or(u64::MAX);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let template = section
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .get_field("msg")
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .and_then(|f| f.values.first())
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .and_then(|v| match v {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                VsfType::x(s) => Some(s.clone()),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                _ => None,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            })
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .unwrap_or_default();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let vals: Vec<LogValue> = section
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .get_fields("val")
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .iter()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .filter_map(|f| f.values.first())
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .map(|v| match v {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                VsfType::z(n) => LogValue::Z(*n as u128),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                VsfType::f6(n) => LogValue::F(*n),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                VsfType::x(s) => LogValue::T(s.clone()),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                // Width-agnostic numeric arms (the doctrine, at last applied to our own log): writes auto-size, so a parsed number arrives as whatever concrete width its magnitude earned — the old exact `u(..)`/`i6` arms matched only one shape each and fell thru to the debug-string fallback for every other, including every value the auto-sizer ever shrank.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                v if matches!(v, VsfType::u(..) | VsfType::u3(_) | VsfType::u4(_) | VsfType::u5(_) | VsfType::u6(_) | VsfType::u7(_)) => {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    LogValue::U(v.as_u64().unwrap_or(0) as u128)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                v if v.as_i64().is_some() && !matches!(v, VsfType::e(_)) => {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    LogValue::I(v.as_i64().unwrap_or(0) as i128)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                VsfType::v_u3(vec) if vec.data.len() == 6 || vec.data.len() == 18 => {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    let (ip_bytes, port_bytes) = vec.data.split_at(vec.data.len() - 2);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    let port = u16::from_le_bytes([port_bytes[0], port_bytes[1]]);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    let ip: std::net::IpAddr = if ip_bytes.len() == 4 {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                        std::net::Ipv4Addr::new(ip_bytes[0], ip_bytes[1], ip_bytes[2], ip_bytes[3])
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                            .into()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    } else {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                        let mut o = [0u8; 16];
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                        o.copy_from_slice(ip_bytes);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                        std::net::Ipv6Addr::from(o).into()
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    };
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                    LogValue::Addr(std::net::SocketAddr::new(ip, port))
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
                other => LogValue::T(format!("{other:?}")),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            })
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            .collect();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let msg = if vals.is_empty() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            template
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        } else {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            render_log_line(&template, &vals)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        };
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let osc = match &header.creation_time {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            Some(VsfType::e(EtType::e6(o))) => *o,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            Some(VsfType::e(EtType::e5(o))) => *o as i64,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            Some(VsfType::e(EtType::e7(o))) => *o as i64,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            _ => 0,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        };
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        records.push(LogRecord {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            osc,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            level,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            msg,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            raw: rest[..rec].to_vec(),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        });
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        off += rec;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    (records, off)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Read the on-disk log from byte `offset` to EOF — the in-app viewer's tail-follow read (a seek, not a whole-file copy). `None` = no log yet or nothing past the offset; a shrunken file (rotation/clear) also reads `None` here and the caller re-syncs from zero.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn read_log_from(offset: u64) -> Option<Vec<u8>> {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    use std::io::{Read, Seek, SeekFrom};
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let path = log_dir()?.join("photon.log.vsf");
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let mut f = std::fs::File::open(&path).ok()?;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let len = f.metadata().ok()?.len();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    if len <= offset {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        return None;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    f.seek(SeekFrom::Start(offset)).ok()?;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let mut out = Vec::with_capacity((len - offset) as usize);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    f.read_to_end(&mut out).ok()?;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    Some(out)
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(not(feature = "logging"))]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[inline(always)]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn read_log_from(_offset: u64) -> Option<Vec<u8>> {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    None
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn log_structured(level: LogLevel, template: &str, vals: Vec<LogValue>) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    append_log_record(level, template, &vals);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(not(feature = "logging"))]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn log_structured(_level: LogLevel, _template: &str, _vals: Vec<LogValue>) {}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Log this build's version + git commit — the FIRST line at startup, so every submitted log self-identifies its build. Ends the "which build is this device even running?" guesswork that stalled diagnosis (a device silently on an old build reads identically to one on the new build until you catch a behavioural tell). Version parts ride as VSF `z` (the version type), the commit as text — binary at rest, rendered at the edge. Called from both the desktop `main()` and the Android JNI entry so it fires on every platform.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn log_version() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let major: u64 = env!("CARGO_PKG_VERSION_MAJOR").parse().unwrap_or(0);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let minor: u64 = env!("CARGO_PKG_VERSION_MINOR").parse().unwrap_or(0);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let patch: u64 = env!("PHOTON_VERSION_PATCH").parse().unwrap_or(0);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    log_structured(
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        LogLevel::Info,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        "photon v{}.{}.{} \u{00b7} commit {} \u{00b7} {} \u{00b7} {}",
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        vec![
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            LogValue::Z(major as u128),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            LogValue::Z(minor as u128),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            LogValue::Z(patch as u128),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            LogValue::T(env!("PHOTON_GIT_COMMIT").to_string()),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            // Platform/arch as the manifest keys spell them, plus the raw OS family — one file is one device+build, so this per-file banner (with the device fp already in the submission filename) gives every record its provenance without repeating bytes per record (binary-at-rest: no per-entry duplication).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            LogValue::T(network::updates::platform_id()),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            LogValue::T(format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        ],
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    );
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// This build's one-line self-description — "v0.69.1 · 61040fe53ad8 · linux x86_64" — for the fleet page's per-device About. Rides the sealed pong tail (typed optional field, unknown-field-skip, no flag day), so any fleet row answers "what is that device running?" without a bridge session — the hole that burned a week in Europe and an hour of "did the deploy ship?" (ticket 2026-08-28).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn about_string() -> String {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    let commit = env!("PHOTON_GIT_COMMIT");
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    format!(
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        "v{}.{}.{} · {} · {} {}",
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        env!("CARGO_PKG_VERSION_MAJOR"),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        env!("CARGO_PKG_VERSION_MINOR"),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        env!("PHOTON_VERSION_PATCH"),
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        &commit[..commit.len().min(12)],
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        std::env::consts::OS,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        std::env::consts::ARCH
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    )
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// format!-shaped structured log at Info: `logf!("RX {} bytes from {}", n, addr)` — the template stores as pure text, `n`/`addr` as typed fields.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[macro_export]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
macro_rules! logf {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    ($fmt:expr $(, $arg:expr)* $(,)?) => {{
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        #[allow(unused_imports)]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        use $crate::CapDisplay as _;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        $crate::log_structured($crate::LogLevel::Info, $fmt, vec![$($crate::Cap(&$arg).cap()),*]);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }};
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// [`logf!`] with an explicit level.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[macro_export]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
macro_rules! logf_at {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    ($level:expr, $fmt:expr $(, $arg:expr)* $(,)?) => {{
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        #[allow(unused_imports)]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        use $crate::CapDisplay as _;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        $crate::log_structured($level, $fmt, vec![$($crate::Cap(&$arg).cap()),*]);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }};
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// Bridge the `log` crate into the VSF sink, so records from every dependency that uses log macros (fluor, tohu, the JNI platform layer, reqwest, ...) land in photon.log.vsf alongside `crate::log` lines — ONE durable, pullable, user-submittable log, no logcat/stdout fork.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// Mirrors the retired android_logger/env_logger setup: Debug and up globally, the known-noisy crates only at Warn+.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
struct VsfLogBridge;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
impl log::Log for VsfLogBridge {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    fn enabled(&self, meta: &log::Metadata) -> bool {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        // Known-chatty dependencies held to Warn+ so their DEBUG streams don't drown the log: naga/wgpu flood per-shader-variable on every pipeline build; rustls/tungstenite/hyper/h2 flood per-connection handshake detail (observed burying the JOIN ceremony trace within a session).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        const NOISY: &[&str] = &[
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            "cosmic_text",
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            "reqwest",
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            "naga",
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            "wgpu",
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            "rustls",
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            "tungstenite",
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            "tokio_tungstenite",
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            "hyper",
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            "h2",
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            // jni logs an attach/detach DEBUG pair on every status-thread hop — ~16,000 lines per Android session, the single largest consumer of the 16 MiB log window (it shortens how much real history a submission carries).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            "jni",
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        ];
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let t = meta.target();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let noisy = NOISY.iter().any(|p| t.starts_with(p));
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        !noisy || meta.level() <= log::Level::Warn
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    fn log(&self, record: &log::Record) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        if !self.enabled(record.metadata()) {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            return;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        let lvl = match record.level() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            log::Level::Error => LogLevel::Error,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            log::Level::Warn => LogLevel::Warn,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            log::Level::Info => LogLevel::Info,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            log::Level::Debug => LogLevel::Debug,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            log::Level::Trace => LogLevel::Trace,
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        };
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        append_log_record(lvl, &format!("{}: {}", record.target(), record.args()), &[]);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    fn flush(&self) {}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// Route the `log` crate into the VSF sink. Call once at startup (desktop `main`, Android `JNI_OnLoad`); a repeat call is a harmless no-op (`set_logger` fails closed).
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(feature = "logging")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn install_log_bridge() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    if log::set_logger(&VsfLogBridge).is_ok() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        log::set_max_level(log::LevelFilter::Debug);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
/// A silent (`--no-default-features`) build installs no logger at all: `log` crate records are dropped, exactly like `crate::log` lines.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(not(feature = "logging"))]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub fn install_log_bridge() {}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub mod call;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub mod crypto;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub mod network;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub mod platform;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub mod storage;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub mod types;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub mod ui;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// Re-export commonly used items from submodules
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub use ui::avatar;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub use ui::display_profile;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub use types::*;
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
// Android JNI initialization
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[cfg(target_os = "android")]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
#[no_mangle]
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
pub extern "system" fn JNI_OnLoad(vm: jni::JavaVM, _: *mut std::os::raw::c_void) -> jni::sys::jint {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    // Route the `log` crate into the VSF sink — logcat is retired; photon.log.vsf (external files dir, adb-pullable) is the ONE log.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    install_log_bridge();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    // Set panic hook for better crash diagnostics
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    std::panic::set_hook(Box::new(|panic_info| {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        // SIDECAR FIRST, always: a panic inside the log sink deadlocks the two lines below (same-thread re-lock), and the evidence dies with the process. See write_crash_sidecar.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        write_crash_sidecar(&format!("PHOTON PANIC: {}", panic_info));
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        log::error!("PHOTON PANIC: {}", panic_info);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        if let Some(location) = panic_info.location() {
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
            log::error!("PANIC location: {}:{}", location.file(), location.line());
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        }
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        // A panic is THE flush edge: in-process RAM (the soft-mode batch) dies with the process.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
        flush_log_buffer();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    }));
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    // Last run's crash, if any, folded into this run's log so it rides the next submission.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    report_prior_crash();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    // Native faults (SIGSEGV etc.) write the same sidecar the panic hook does — a fold-in next run instead of a silent tombstone.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    platform::crash_native::install();
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    // Hand tohu the JavaVM so its device oracle can read Settings.Secure.ANDROID_ID itself (via ActivityThread.currentApplication()). Done here because JNI_OnLoad is where the vm is handed to us; the actual fetch happens later, once the Application exists.
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    tohu::device::android_init(vm);
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.

//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    log::info!("Photon JNI loaded (PID: {})", std::process::id());
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
    jni::sys::JNI_VERSION_1_6
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
}
//   audio_aaudio.rs — ANDROID device loops on AAudio, Rust-owned (2026-09-09): exclusive LOW_LATENCY streams (shared fallback), HAL-stamped frames (frame_time via AAudioStream_getTimestamp → eagle osc) feeding the same queues; start/stop/ensure_input (mic grant via nativeMicGranted), stream-error rebuild off the callback thread.
