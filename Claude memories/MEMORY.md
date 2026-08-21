# Photon Project Memory Index

## Desktop corpus (project_* / feedback_* / reference_*)

- [feedback_no_redundant_disk_ops.md](feedback_no_redundant_disk_ops.md) — 8 rolling BTRFS snapshots per 8h (+stragglers, Harbor+Chiton+MEGA); a wanted safety copy = REFLINK (cp --reflink=always), never a literal copy; batch full-repo scans, one pass

- [project_fleet_key_redesign.md](project_fleet_key_redesign.md) — fleet-key REDESIGN spec'd (docs/fleet-key.md @2c81c39, Nick reviewing): ira-wrapped, revision-published, shrink-only mint; DANGER: deploying current build to MacBook/Android destroys the last history copy (census deletes un-migrated rings)

- [project_great_cleanup.md](project_great_cleanup.md) — GREAT CLEANUP: Phases 1-6 ALL SHIPPED 2026-08-20 (device vault+migration, self-honest rings, fleet-first rejoin, JPEG gate, LAN add); Phase 7 = Nick's publish

- [project_button_one_renderer.md](project_button_one_renderer.md) — ONE pill/button renderer in fluor (draw_pill_immediate + retained Button); photon never hand-rolls squircles

- [project_render_storm_lag.md](project_render_storm_lag.md) — ROOT-CAUSED 2026-08-15: lag = VAULT MUTEX contention (UI-tick avatar probe read vs background persist writers); photon fix @c6f65e8 (probe cached, ticks vault-free)

- [project_settings_typed_values.md](project_settings_typed_values.md) — fstate v7 SHIPPED 2026-08-16: every settings value natively typed VSF + v6 compat window; window geometry = 2 typed device-local pairs, gesture-settle save
- [project_fgtw_key_desync.md](project_fgtw_key_desync.md) — CLOSED 2026-08-14: rollback un-bricked, guard SHIPPED bcb830d, canonical key RECOVERED (MacBook deploy copy) → keys/fgtw-seed-key.rs + brick worker redeployed a51c9194

- [project_lifecycle_flows.md](project_lifecycle_flows.md) — identity/device lifecycle DESIGNED (docs/lifecycle.md): D1 collision=KnownHandle, D2 double-attest=binding marker; D3 LastRites SUPERSEDED
- [project_identity_never_dies.md](project_identity_never_dies.md) — IDENTITY NEVER DIES SHIPPED 2026-07-17: no terminal op (worker refuses zero-member folds, LastRites cut), brands survive departure, two-signature retire, fleet page "retired — still yours" + Release; retirement = obscure handle + puck
- [succession-emit-side-unwired.md](succession-emit-side-unwired.md) — identity succession primitive + worker slot + contact RECEIVE path SHIPPED (05f7d27)

- [feedback_self_is_a_contact.md](feedback_self_is_a_contact.md) — HARD RULE, repeatedly violated: self and bob are both people; the self/fleet conversation rides the IDENTICAL machinery (only the key material differs)
- [feedback_handles_byte_precise.md](feedback_handles_byte_precise.md) — HARD RULE, repeatedly violated before: handles are BYTE-PRECISE full-Unicode (Kea ≠ Nick, whitespace-only valid, Zoë/李伟/김민준 first-class)
- [feedback_fgtw_deploy_freely.md](feedback_fgtw_deploy_freely.md) — deploy fgtw.org (wrangler) + toka.wasm freely, no per-deploy confirmation
- [project_avatar_encryption_wall.md](project_avatar_encryption_wall.md) — avatars are v'e'-encrypted per-handle; admin can't decrypt; browser AV1 decode infra (rav1d-in-wasm) built + deployed, belongs in photon

- [project_braid_working_baseline.md](project_braid_working_baseline.md) — HISTORIC 2026-06-28: braid+CLUTCH+delivery green E2E on 2 devices @ 6325cd9

- [project_manifestus_tombstone_bug.md](project_manifestus_tombstone_bug.md) — vault corruption = fast-delete left committed pointer; FIXED @ manifestus 56bde9a, desktop vault repaired zero-loss; publish to all devices PENDING
- [project_storage_layering.md](project_storage_layering.md) — 3 storage layers (vault/chain-state/rārangi conversation DB); file-tree paths half-assed into flat vault, de-stringing to blake3(domain,scope) + wiring rārangi
- [project_reserve_delivery.md](project_reserve_delivery.md) — RE-SERVE SHIPPED 97e2bcc 2026-08-20: durable store outranks pending list (sealed tip + row deficit → re-serve non-pending rows, 8/burst ×2/tip)
- [project_rarangi_messages_fleet.md](project_rarangi_messages_fleet.md) — message rows: table=friendship_id bytes, pk=monotonic u64 counter; fleet=a conversation, vaults byte-identical except device crypt key
- [project_fleet_routing_scale.md](project_fleet_routing_scale.md) — fleet invariants: any size (12+, no 2-device shortcuts) in eggs/braid/fan-out
- [project_fleet_braid_plane.md](project_fleet_braid_plane.md) — §14 CUTOVER CLOSED 2026-08-18 @ 2adff1d: spine built, §14.5 slot superseded by docs/durability.md (FLEET-HOLDS-HISTORY), horizon+shred redesigned local (post-voice-calls), linearizer retired by lanes; next major = voice calls
- [project_fleet_unification_v1.md](project_fleet_unification_v1.md) — unification v1 SHIPPED @ b231592: compose ANYWHERE (fleet-forward → chain owner transmits, original timestamps), both is_online killswitches dead, periodic sweep; §14 linearizer = next stage
- [project_attachments.md](project_attachments.md) — attachments v1+v2 SHIPPED (e8baa81+cd3aa3f): row/blob split, PT blobs no-cloud, true-shred, Android picker, resample card
- [project_unattended_reboot.md](project_unattended_reboot.md) — auto-attest-on-reboot SHIPPED (tohu 8d66b19, FINALLY PUSHED as d26a8d8 2026-08-16 — was stranded desktop-only + photon 23e13f5): off-by-default Security toggle, device-bound reboot capsule, handle re-entry to arm AND disarm
- [project_bridge.md](project_bridge.md) — BRIDGE = passless remote shell between fleet siblings over PT (rustdesk/SSH replacement)
- [project_chain_replication.md](project_chain_replication.md) — chain replication SHIPPED @ 2dfe7ed: chains sync fleet-wide (mutated_osc v7, adopt-iff-newer), adopting device flips sendable
- [project_avatar_bearer_pin_gap.md](project_avatar_bearer_pin_gap.md) — CLOSED: pin-rotate on membership shrink shipped in removal-rotates step 1 (2026-07-23); avatar = 64-byte bearer pin (key‖lookup)
- [project_clutch_token_asymmetry.md](project_clutch_token_asymmetry.md) — "unknown conversation_token" = §4.2 competing ceremony instances
- [project_clutch_offer_deadlock.md](project_clutch_offer_deadlock.md) — CLUTCH offer-loss deadlock generations; FIXED @7d5e356 (retries=no-progress, path-up/stall offer re-fire, pong-drop torches); OPEN: one peer's pongs never arrive
- [project_clutch_ui_thread_hitch.md](project_clutch_ui_thread_hitch.md) — FIXED @c48b0e1 2026-08-15: KEM decap = 4th job stage (HQC-prefix CAS drain), duplicate-KEM short-circuit, proof rides durable chains writer; phone E2E pending
- [project_nat_traversal_relay_gap.md](project_nat_traversal_relay_gap.md) — punch tiers + LIVE relay pipe shipped 2026-07-22: per-recipient Cloudflare DO (PipeHub)
- [project_windows_dark_theme_bug.md](project_windows_dark_theme_bug.md) — PINNED: photon install corrupted Jennifer's Windows dark-theme search text (theme-cache signature, light/dark toggle fixed)
- [project_contacts_glow_damage.md](project_contacts_glow_damage.md) — contacts search-box focus glow not repainted on deselect (stale glow lingers/clips, looks un-deselectable); bg pass dirty-gating skips the glow_bbox
- [project_history_recovery.md](project_history_recovery.md) — history sync: friend backfill + FLEET sync shipped; BOTH is_online gates that silently killed fleet delivery removed 2026-07-25 (push d73c223, pull 648791b)
- [project_rekey_attack_surface.md](project_rekey_attack_surface.md) — re-key/history-injection threat model (docs/rekey-threat-model.md); first-met device un-revocable + revocation unwired
- [project_token_private_identity.md](project_token_private_identity.md) — TOKEN crux SOLVED + phases 1-2 SHIPPED (fleet weave fc841f4, S lifecycle f057c5d): friend-blinded private identity S, OTP-blind, never at rest
- [project_chain_advance_desync.md](project_chain_advance_desync.md) — RESOLVED 2026-08-18: braid-desync class closed, ten-round soak verified bidirectional messaging; four converging ACK heals shipped
- [project_notifications_pinned.md](project_notifications_pinned.md) — fleet-wide notification design (unnotified flag + one-active-clearer) still PINNED
- [project_android_ime_model.md](project_android_ime_model.md) — THE Android keyboard model: surface NEVER resizes for IME (adjustNothing + nativeImeInset mirror + ime_lift on bottom strips); span-pin DELETED from fluor, don't reintroduce
- [project_multimonitor_status.md](project_multimonitor_status.md) — multi-monitor A/B/C-core+macOS-port shipped, phase D + Windows port not built; macOS drag-to-monitor VANISHES (pinned)
- [reference_log_pull.md](reference_log_pull.md) — photonlog --pull --session (own machine) or --handle <LEFT-column map bytes>; MAP = 'handle = petname', handle LEFT petname RIGHT (column trap burned 2026-08-21); desktop log = volatile tmpfs, soft-mode RAM batch; Nick devices: 90e571bf desktop "BarkCook", 1be949c1 phone "TheoryConvertible"
- [project_log_sweep_eats_fresh.md](project_log_sweep_eats_fresh.md) — 13:17Z ate a fresh submission, 14:17Z instrumented cron kept all bait (unconvicted); hardened key-osc sweep committed 8fe1b83, DEPLOY BLOCKED on wrangler login
- [project_vault_op_latency.md](project_vault_op_latency.md) — CONVICTED + ALL FIXES SHIPPED 2026-08-21: ~900ms/put flat → group commit (manifestus put_batch b925230 + kete batch drain 1944ebe) + five UI-thread writes off-thread (photon b7dd871); ping-pong = correct CRDTs, wedge churn
- [project_android_session_capsule.md](project_android_session_capsule.md) — Android de-attest-on-restart fix: boot-locked session capsule (spaghettify(boot_id) wairua, kete AEAD, multi-tier) SPEC'd in docs/, not built
- [project_vsf_canonical_signing.md](project_vsf_canonical_signing.md) — ONE canonical VSF signing scheme (ge over BLAKE3(file, ge zeroed)); hp-value signing retired 2026-07-06
- [project_clutch_completion_rebroadcast.md](project_clutch_completion_rebroadcast.md) — CLUTCH completes crypto-correct but rebroadcasts its proof forever (ceremony decoupled from data plane)
- [project_fgtw_migration_state.md](project_fgtw_migration_state.md) — FGTW substrate extracted into the fgtw crate through M3 (keys/fleet/fanout/fstate/pair/client); photon rides it via thin re-export wrappers
- [project_fgtw_nostd_deferred.md](project_fgtw_nostd_deferred.md) — fgtw crate stays std until ferros; move code verbatim (no alloc::/no_std refactors), fanout feature keeps crypto deps off the worker base
- [project_peers_are_fgtw.md](project_peers_are_fgtw.md) — decentralize FGTW: fgtw.org server retires, peers become the trust web; OPEN phonebook (enumerate all attested) + MUTUAL-CONSENT clutch (ignore until both friend)
- [project_party_colour_perceptual.md](project_party_colour_perceptual.md) — Conversation party colours are placeholder; swap to perceptual L≈50% via vsf spectral/LMS
- [project_presence_vs_online.md](project_presence_vs_online.md) — presence ≠ online (online = avatar ring, always); "show my presence" = busy/song/mood broadcast, DEFAULTS OFF
- [project_theme_rec2020.md](project_theme_rec2020.md) — fluor+photon theme.rs colours = VSF RGB lazily passed thru; convert via vsf_rgb_to_bt2020 + target Rec.2020 output on ALL platforms
- [project_nunc_clock_check.md](project_nunc_clock_check.md) — nunc-time = clock VALIDATOR not photon's clock source; warn-only banner + ONE load-bearing use: `now` in the update stamp window (user mandate)
- [project_update_flow.md](project_update_flow.md) — self-update BUILT @228f68c + release-notice push BUILT (deploy.sh → hub broadcast + FCM → instant poll); stamp window gates installs
- [project_fleet_inbox.md](project_fleet_inbox.md) — fleet inbox DESIGNED in docs/fleet-inbox.md (inbox/<hp>/ + hub/FCM wake; worker events / release notices / member notices, never a control channel); v1 = bind-attempt alert; NOT built
- [project_doorbell.md](project_doorbell.md) — doorbell v1 BUILT 2026-07-19 (clock+ring+bells+FCM v1 sender+Kotlin wake; photon f852cbb, worker f3d621f); OPEN: sibling bell overwrite, opt-in toggle, TCP tier-1, E2E
- [project_device_sovereignty.md](project_device_sovereignty.md) — THE ownership rule for all records: subject signs, others verify-or-withhold; pending expires, completed is permanent testimony; ostracism not erasure
- [project_identity_profile.md](project_identity_profile.md) — identity profile DESIGNED (docs/identity-profile.md): handle at rest NOWHERE (roster handle = honeypot → pin-set), grants-only disclosure, required name + petnames, epoched profile key, NFC bearer invite card; rides roster rework
- [project_device_loaners.md](project_device_loaners.md) — loaners: de-attest keeps claims (dormant); transfer vs loan-annotation UNDECIDED. Removal DECIDED: chain = consent-only (bilateral add + SELF-signed departure, NO exceptions); eviction happens at the S/friendship re-key layer; locks/recalls = routing layer
- [project_total_loss_recovery.md](project_total_loss_recovery.md) — total-loss = custodian-authorized chain SUPERSESSION not edit; quorum-not-secret-share; ALWAYS custodian-gated even with handle (else eviction backdoor)
- [reference_braid_novelty.md](reference_braid_novelty.md) — braid prior-art/novelty: primitives are prior art (double ratchet etc.); the defensible claim is two-distinct-peer-strands CSPRNG-picked-but-explicitly-referenced

- [feedback_source_map.md](feedback_source_map.md) — Keep the source map comment block at top of src/lib.rs updated when pub items or files change
- [feedback_commit_all.md](feedback_commit_all.md) — When asked to commit, include all modified files unless explicitly told otherwise
- [feedback_legacy_first.md](feedback_legacy_first.md) — Port Photon UI from legacy compositing.rs as visible-RGB RMW first; fluor under-blend is Phase 5 cleanup
- [project_vault_roadmap.md](project_vault_roadmap.md) — Vault phasing: ring tooling, GC, mid-session resurrection, bulk content (avatars/attachments/calls) all wait for device-sync phase
- [project_identity_storage_model.md](project_identity_storage_model.md) — Device identity is deterministic from fingerprint (not stored); vault lives in app-private storage only, dual ring is intra-session resilience not uninstall-survival
- [project_android_color_pipeline_floor.md](project_android_color_pipeline_floor.md) — Android 1:1 panel rendering floor: ~2% calibration LUT residual with BT.2020+γ=2.2 buffer tag; ColorMode::NATIVE blocked by vendor init even with root
- [feedback_orb_settings_panel.md](feedback_orb_settings_panel.md) — orb = settings/about/help panel entry; device management is a separate page in that panel, never the orb's direct action (current direct wiring is interim)
- [feedback_no_time_based_ui.md](feedback_no_time_based_ui.md) — never time-based UI: no auto-expiring toasts/banners/delayed transitions; event-shown, interaction-cleared (click/keystroke via clear_hints)
- [feedback_terminal_clipboard.md](feedback_terminal_clipboard.md) — spaces around `=` in dev-log output (double-click selects the value)
- [feedback_script_timestamps.md](feedback_script_timestamps.md) — every build/deploy script ends with `completed $(date)` on each success exit
- [feedback_voca_camelcase.md](feedback_voca_camelcase.md) — Default voca-encoded values to camelCase concatenation; space-separated form is opt-in for read-aloud
- [feedback_no_comment_wraps.md](feedback_no_comment_wraps.md) — Don't hard-wrap comments/docstrings/markdown; one sentence per line, however long (RECURRING "line wrap virus" — user deletes files over it; content is fine, wrapping is not)
- [feedback_direct_pixel_no_floaters.md](feedback_direct_pixel_no_floaters.md) — rendering is DIRECT PIXEL ACCESS ONLY; no GPU shaders/vertex triangles/float pipeline ("no floaters"); wgpu renderer is suspect
- [project_textbox_one_registry.md](project_textbox_one_registry.md) — adding a textbox = register in TWO walks only (visit_app_widgets + textboxes_mut); hover/damage/I-beam/gestures inherit — never hand-list per concern
- [project_arabic_indexing_fixits.md](project_arabic_indexing_fixits.md) — FIX-IT LIST: decimal-indexed VSF field names (pong sync_{i}_*, peer_{i}, profile.addrN → native multi-value fields)
- [feedback_commit_attribution.md](feedback_commit_attribution.md) — Built-With: Claude Opus <version> trailer is wanted; never Co-Authored-By Claude (tool, not author)
- [feedback_spelling.md](feedback_spelling.md) — thru/thruout/altho, and colour spelled British; the rest United Statesian
- [feedback_build_dev_script.md](feedback_build_dev_script.md) — Use ./scripts/dev.sh to compile/check photon, not bare cargo build (thrashes the machine); android dev = scripts/android/dev-adb.sh
- [project_manifestus_custodes_split.md](project_manifestus_custodes_split.md) — manifestus = storage engine (was custodes, dir renamed, package still "custodes"); custodes reclaimed for TOKEN-recovery custodians
- [project_device_identity_model.md](project_device_identity_model.md) — tohu device-identity crate (oracle + frozen v0 derivation); Security/Recovery axes; deferred handle-salt collision fix
- [project_keyring_design.md](project_keyring_design.md) — multi-device keyring: fleet chain + device-ADD pairing v1 SHIPPED. OPEN: braid-in of fresh device
- [project_pairing_v2.md](project_pairing_v2.md) — pairing v2 REDESIGNED 2026-07-13 words-first: binding-request registry + masked words + consent-egg bilateral Add + self-departure-only Remove + two-phase; build NOT started
- [reference_aarch64_cross_libs.md](reference_aarch64_cross_libs.md) — missing system lib for aarch64-linux cross-build: vendor the .so into cross-libs/aarch64 + mirror x11.pc
- [reference_site_cv_pdfs.md](reference_site_cv_pdfs.md) — holdmyoscilloscope.com = /mnt/Chiton/MEGA/holdmyoscilloscope (wrangler pages); CV PDFs via about/make-cv-pdfs.sh after cv-*.html edits
- [reference_ihi_primitives.md](reference_ihi_primitives.md) — ihi has TWO one-way primitives: lossy OWF = chaos_amp/spaghettify (32-op data-dependent lossy ALU, PIPE-silicon-exact)
- [project_token_terminology.md](project_token_terminology.md) — whakaira (ceremony) vs ihi (perceptible mana); canonical glossary = ferros/GLOSSARY.md; ira/wairua/state codes fixed
- [project_session_registers.md](project_session_registers.md) — tohu session store = {identity_seed, vault_seed, handle_proof} registers, never the handle string; keep vault/network roots SEPARATE (security)
- [project_secret_memory_hygiene.md](project_secret_memory_hygiene.md) — hot-secret RAM handling: do-now = zeroize + mlock + no-core-dumps + copy discipline; hibernation/cold-boot uncloseable in userspace (= PIPE line)
- [reference_vsf_primary_section.md](reference_vsf_primary_section.md) — VSF readers MUST use VsfHeader::primary_section (near-form names are TOC-only, header-only sections have no body)
- [reference_claude_unguard.md](reference_claude_unguard.md) — ~/.local/bin/claude-code-unguard FORCE-opens Claude Code's Edit read guard (patches Bun binaries); RE-RUN + RELOAD after every update; file-history → /dev/shm tmpfs
- [feedback_vsf_readers_width_agnostic.md](feedback_vsf_readers_width_agnostic.md) — VSF readers NEVER exact-match integer widths (as_u64/as_i64/as_usize only); auto-sized writes decode concrete, so u(..)/i(..) arms never fire
- [feedback_numbers_binary_at_rest.md](feedback_numbers_binary_at_rest.md) — THE number doctrine: binary at rest (wire/vault/log), base chosen at render edge only (dozenal glyphs UI / words read-aloud); arabic never
- [feedback_answer_dont_act.md](feedback_answer_dont_act.md) — user asks a QUESTION → answer and stop; never take action (esp. destructive) on a verification question; do ONLY what's asked, nothing extra
- [project_two_machine_git_divergence.md](project_two_machine_git_divergence.md) — after ANY commit verify HEAD == ls-remote; "missing fgtw/fluor symbol" = stale sibling, fast-forward first
## MacBook corpus (kebab-case)

- [Push after landing](push-after-landing.md) — push photon after commits + commit/push the memory repo (private: photon-claude-memory) after memory writes; the MacBook is never the only copy

- [Per-device lanes](per-device-lanes.md) — SHIPPED 53ad8f9 2026-08-13 (unpublished): any replicated-chain device transmits on its own lane; CRDT lane merge converges
- [Relay asymmetry + ping reflection](relay-asymmetry-ping-reflection.md) — FIXES 271c76c + 30e81b6 2026-08-13: ping reflection + reflect-beside-pings bootstrap (send side of Reflect never existed)

- [Notes-row ceremony wedge](self-pair-sibling-row.md) — 4417b90 FIELD-VERIFIED; mid-ceremony sleep+restart deadlock fixed 5f1535a 2026-08-13 (stall re-fire widened); fe46a74b=MACBOOK, 1be949c1=ANDROID
- [Boot blindness](boot-blindness.md) — FIX SHIPPED f280fda 2026-08-12: settings+zoom at vault-open, rehydrate skips Complete, avatars local-first; resume arm phase-timed ("PERF: resume load"); goal = local paint under a frame
- [Fleet epoch arc design](fleet-epoch-arc-design.md) — B1-B3 shipped a8b9d48/300886d + worker deployed, field-verified; remaining: hist_page/pong re-seal, row-cadence mint

- [No wrapped comments](no-wrapped-comments.md) — photon comments are one line per thought, never hard-wrapped
- [Connection flow revision](connection-flow-revision.md) — SHIPPED 2026-07-31: ladder narrates the exchange; residue = proof-echo quieting
- [Edges, not timers](edges-not-timers.md) — react on event edges (release/ACK/push), never timers or debounces
- [Commit trailer](commit-trailer-built-with.md) — never "Co-Authored-By: Claude"; end commits with "Built with Claude Fable 5"
- [Nick publishes](nick-publishes.md) — never run publish scripts; commit/push only, check only when warranted
- [No private handles](no-private-handles.md) — public repo: field incidents in comments/commits use neutral roles + dates, never handles or "Sarah"
- [Self-only removal](self-only-removal.md) — a device removes only itself, zero exceptions; stolen = lockout, never removal
- [Re-clutch, never store](re-clutch-never-store.md) — recovery = fresh ceremony; secrets at rest only when absolutely required
- [Messaging solidity Phase A](messaging-solidity-phase-a.md) — A + B4 done (2026-08-09, locks commute per-key); flag-day APPROVED for B2's chain op; next: B1→B3 fleet chain+eggs arc
- [Persist findings early](persist-findings-early.md) — Nick undoes via message edits, which truncates context; write load-bearing findings to memory/docs as they land
- [UI thread snapshot+CAS](ui-thread-snapshot-cas.md) — SHIPPED 2026-08-08: workers get snapshots, commits CAS live state, writers fire ACK/transmit post-durability; garbage is fork evidence only past the CAS
- [Lane rotation wedge heal](lane-rotation-wedge-heal.md) — SHIPPED 2026-08-09: peer-at-anchor + unlinkable exhausted pendings → rotate lane, re-serve rows at original stamps; relay legs detached (the 5-10s ACK latency)
- [Split contacts incident](split-contacts-incident.md) — CLOSED 2026-08-11: SHADOW CONVERSATIONS — receive arms derived convs from chains.participants, loader/persist use the contact; fixed all three arms
- [VSF TOC section-name trap](vsf-toc-section-name-trap.md) — section names live in the header TOC; bare VsfSection::parse gives name="" and == checks silently reject all; 3rd victim was LAN discovery (dead fleet-wide)
- [reference_windows_arm64_toolchain.md](reference_windows_arm64_toolchain.md) — Windows-on-ARM: aarch64-pc-windows-gnullvm via llvm-mingw at /mnt/Harbor/Code/llvm-mingw; build.rs uses llvm-rc; deploy.sh + installer wired
- [reference_mingw_features_shim.md](reference_mingw_features_shim.md) — x86_64-windows breaks on pqcrypto-mlkem #include <features.h> (MinGW lacks it); FIXED @4809917 via vendored shim + .cargo/config.toml [env]
- [project_lockout_enforcement.md](project_lockout_enforcement.md) — lock @b75cc0e + UNLOCK @0f76044/fa9e765: handle-confirmed reversal, typed tombstone, locked-signer refusal + monotonic guard at worker, locked-rewrap hole fixed
- [project_fleet_epoch_arc_closed.md](project_fleet_epoch_arc_closed.md) — epoch arc CLOSED @ fa3a9c0: hist_page+pong epoch re-seal, row-cadence mint
- [project_wiped_device_roster_clobber.md](project_wiped_device_roster_clobber.md) — 2026-08-16 wiped-mac contactless: stale oracle fleet key + aead breaker clobbered roster slot; guard SHIPPED b83834f; epoch-mint heal verified
- [project_window_geometry_shipped.md](project_window_geometry_shipped.md) — window geometry SHIPPED 2026-08-16 thru fluor's model: apply_window_rect one placement path + once-per-gesture settle hook
- [project_humanitys_code.md](project_humanitys_code.md) — openness doctrine: secrecy surface = handles + keys ONLY; everything else public — "this is humanity's code"
- [settings.md](settings.md) — Nick's note: naive fixed-width unlabeled settings converted to proper VSF (the fstate v7 arc)
- [memories-live-in-repo](push-after-landing.md) — memories LIVE HERE ('Claude memories/' in the PUBLIC photon repo, 2026-08-16): commit+push with photon; scrub names/handles ALWAYS (handles are secrets)
- [project_voice_calls.md](project_voice_calls.md) — calls FIELD-WORKING 2026-08-19 (two-way clean): channel-aware CBR ladder 16k→128k (tier byte, AIMD, VBR permanently banned), soft duck, MEDIA fast-mixer out (vendor AEC traded), adaptive jitter; docs/calls.md
- [project_xchacha_migration.md](project_xchacha_migration.md) — 2026-08-18 stack-wide ChaCha20→XChaCha20-Poly1305 (96→192-bit nonce) EVERYWHERE incl. chain stream layer
