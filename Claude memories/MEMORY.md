# Photon Project Memory Index

## Desktop corpus (project_* / feedback_* / reference_*)

- [feedback_no_redundant_disk_ops.md](feedback_no_redundant_disk_ops.md) — BTRFS snapshots exist; a safety copy = REFLINK (cp --reflink=always), never a literal copy; batch repo scans into one pass

- [project_fleet_key_redesign.md](project_fleet_key_redesign.md) — fleet-key REDESIGN spec'd (docs/fleet-key.md): ira-wrapped, revision-published, shrink-only mint

- [project_great_cleanup.md](project_great_cleanup.md) — GREAT CLEANUP Phases 1-6 ALL SHIPPED 2026-08-20

- [project_button_one_renderer.md](project_button_one_renderer.md) — ONE pill/button renderer in fluor (draw_pill_immediate + retained Button); photon never hand-rolls squircles

- [project_era_ratchet.md](project_era_ratchet.md) — ERA RATCHET stages 1-4 SHIPPED 2026-09-08 (light KEM ratchet, computed owner, woven CLUTCH on shrink); NEXT stage 5 consent, 6 soak
- [project_fluor_busy_loop.md](project_fluor_busy_loop.md) — OPEN: main thread 100% CPU = wake_at returns past Instant (animating flags / add_in_flight stuck / Android presence guard
- [project_render_storm_lag.md](project_render_storm_lag.md) — ROOT-CAUSED 2026-08-15: lag = VAULT MUTEX contention (UI-tick avatar probe read vs background persist writers)

- [project_settings_typed_values.md](project_settings_typed_values.md) — fstate v7 SHIPPED 2026-08-16: every settings value natively typed VSF + v6 compat window
- [project_fgtw_key_desync.md](project_fgtw_key_desync.md) — CLOSED 2026-08-14: rollback un-bricked, guard SHIPPED bcb830d

- [project_lifecycle_flows.md](project_lifecycle_flows.md) — identity/device lifecycle DESIGNED (docs/lifecycle.md): D1 collision=KnownHandle, D2 double-attest=binding marker
- [project_identity_never_dies.md](project_identity_never_dies.md) — IDENTITY NEVER DIES SHIPPED 2026-07-17: no terminal op, brands survive departure, two-signature retire
- [succession-emit-side-unwired.md](succession-emit-side-unwired.md) — identity succession primitive + worker slot + contact RECEIVE path SHIPPED (05f7d27)

- [feedback_self_is_a_contact.md](feedback_self_is_a_contact.md) — HARD RULE, repeatedly violated: self and bob are both people
- [feedback_handles_byte_precise.md](feedback_handles_byte_precise.md) — HARD RULE, repeatedly violated before: handles are BYTE-PRECISE full-Unicode (Kea ≠ Nick, whitespace-only valid
- [feedback_fgtw_deploy_freely.md](feedback_fgtw_deploy_freely.md) — deploy fgtw.org (wrangler) + toka.wasm freely, no per-deploy confirmation
- [project_avatar_encryption_wall.md](project_avatar_encryption_wall.md) — avatars are v'e'-encrypted per-handle; admin can't decrypt

- [project_braid_working_baseline.md](project_braid_working_baseline.md) — HISTORIC 2026-06-28: braid+CLUTCH+delivery green E2E on 2 devices @ 6325cd9

- [project_manifestus_plow_reloc_refusal.md](project_manifestus_plow_reloc_refusal.md) — CLOSED 2026-09-08: 'storage degraded' = LiveSet::apply order bug (manifestus f28e851 + KATs)
- [project_manifestus_tombstone_bug.md](project_manifestus_tombstone_bug.md) — vault corruption = fast-delete left committed pointer; FIXED @ manifestus 56bde9a
- [project_storage_layering.md](project_storage_layering.md) — 3 storage layers (vault/chain-state/rārangi conversation DB); file-tree paths half-assed into flat vault
- [project_reserve_delivery.md](project_reserve_delivery.md) — RE-SERVE SHIPPED 97e2bcc 2026-08-20: durable store outranks pending list (sealed tip + row deficit → re-serve)
- [project_rarangi_messages_fleet.md](project_rarangi_messages_fleet.md) — message rows: table=friendship_id bytes, pk=monotonic u64 counter; fleet=a conversation
- [project_self_message_vanish.md](project_self_message_vanish.md) — self-msg vanish ROOT-CAUSED (save amplification + quit ate writes); delta gate + quit drain SHIPPED 68b1912
- [project_fleet_routing_scale.md](project_fleet_routing_scale.md) — fleet invariants: any size (12+, no 2-device shortcuts) in eggs/braid/fan-out
- [project_fleet_braid_plane.md](project_fleet_braid_plane.md) — §14 CUTOVER CLOSED 2026-08-18 @ 2adff1d: spine built, durability = docs/durability.md (FLEET-HOLDS-HISTORY)
- [project_fleet_unification_v1.md](project_fleet_unification_v1.md) — unification v1 SHIPPED @ b231592: compose ANYWHERE (fleet-forward → chain owner transmits)
- [project_attachments.md](project_attachments.md) — attachments v1+v2 SHIPPED (e8baa81+cd3aa3f): row/blob split, PT blobs no-cloud, true-shred, Android picker, resample card
- [project_links.md](project_links.md) — message links = typed marks; live purple spans, chain-link button relabels to the literal trim, label editable (2026-09-09)
- [project_unattended_reboot.md](project_unattended_reboot.md) — auto-attest-on-reboot SHIPPED (tohu d26a8d8 + photon 23e13f5): off-by-default Security toggle, device-bound reboot capsule
- [project_bridge.md](project_bridge.md) — BRIDGE = passless remote shell between fleet siblings over PT (rustdesk/SSH replacement)
- [project_chain_replication.md](project_chain_replication.md) — chain replication SHIPPED @ 2dfe7ed: chains sync fleet-wide (mutated_osc v7, adopt-iff-newer), adopting device flips sendable
- [project_avatar_bearer_pin_gap.md](project_avatar_bearer_pin_gap.md) — CLOSED: pin-rotate on membership shrink shipped in removal-rotates step 1 (2026-07-23)
- [project_clutch_token_asymmetry.md](project_clutch_token_asymmetry.md) — "unknown conversation_token" = §4.2 competing ceremony instances
- [project_call_no_ring_incident.md](project_call_no_ring_incident.md) — no-ring CONVICTED (identity-era split); GLARE fixed; 2026-09-08 instant-drop + two-live-eras both FIXED
- [project_clutch_offer_deadlock.md](project_clutch_offer_deadlock.md) — CLUTCH offer-loss deadlock FIXED @7d5e356 (retries=no-progress, path-up/stall re-fire, pong-drop torches)
- [project_clutch_ui_thread_hitch.md](project_clutch_ui_thread_hitch.md) — FIXED @c48b0e1: KEM decap = 4th job stage (HQC-prefix CAS drain), duplicate-KEM short-circuit
- [project_nat_traversal_relay_gap.md](project_nat_traversal_relay_gap.md) — punch tiers + LIVE relay pipe shipped 2026-07-22: per-recipient Cloudflare DO (PipeHub)
- [project_windows_dark_theme_bug.md](project_windows_dark_theme_bug.md) — PINNED: photon install corrupted Jennifer's Windows dark-theme search text (theme-cache signature, light/dark toggle fixed)
- [project_contacts_glow_damage.md](project_contacts_glow_damage.md) — LIKELY STALE: the invalidate_bg-on-focus-edge fix exists in code (pre-2026-08-14) naming this exact symptom
- [project_history_recovery.md](project_history_recovery.md) — history sync: friend backfill + FLEET sync shipped; BOTH is_online gates that silently killed fleet delivery removed
- [project_rekey_attack_surface.md](project_rekey_attack_surface.md) — re-key/history-injection threat model (docs/rekey-threat-model.md); first-met device un-revocable + revocation unwired
- [project_token_private_identity.md](project_token_private_identity.md) — TOKEN crux SOLVED + phases 1-2 SHIPPED (fleet weave fc841f4
- [project_chain_advance_desync.md](project_chain_advance_desync.md) — RESOLVED 2026-08-18: braid-desync class closed, ten-round soak verified bidirectional messaging
- [project_notifications_pinned.md](project_notifications_pinned.md) — fleet-wide notification design (unnotified flag + one-active-clearer) still PINNED
- [project_android_ime_model.md](project_android_ime_model.md) — THE Android keyboard model: surface NEVER resizes for IME (adjustNothing + nativeImeInset mirror + ime_lift)
- [project_multimonitor_status.md](project_multimonitor_status.md) — multi-monitor A/B/C-core+macOS-port shipped, phase D + Windows port not built; macOS drag-to-monitor VANISHES (pinned)
- [reference_log_pull.md](reference_log_pull.md) — photonlog --pull --session (own machine) or --handle <LEFT-column map bytes>; map = 'handle = petname' (LEFT secret); pull to a file first
- [project_log_sweep_eats_fresh.md](project_log_sweep_eats_fresh.md) — 13:17Z ate a fresh submission, 14:17Z instrumented cron kept all bait (unconvicted)
- [project_vault_op_latency.md](project_vault_op_latency.md) — CONVICTED + FIXED 2026-08-21: ~900ms/put → group commit (manifestus put_batch + kete batch drain) + five UI-thread writes
- [project_android_session_capsule.md](project_android_session_capsule.md) — Android de-attest-on-restart fix: boot-locked session capsule (spaghettify(boot_id) wairua)
- [project_vsf_canonical_signing.md](project_vsf_canonical_signing.md) — ONE canonical VSF signing scheme (ge over BLAKE3(file, ge zeroed)); hp-value signing retired 2026-07-06
- [project_clutch_completion_rebroadcast.md](project_clutch_completion_rebroadcast.md) — CLUTCH completes crypto-correct but rebroadcasts its proof forever (ceremony decoupled from data plane)
- [project_fgtw_migration_state.md](project_fgtw_migration_state.md) — FGTW substrate extracted into the fgtw crate through M3 (keys/fleet/fanout/fstate/pair/client)
- [project_fgtw_nostd_deferred.md](project_fgtw_nostd_deferred.md) — fgtw crate stays std until ferros; move code verbatim (no alloc::/no_std refactors)
- [project_peers_are_fgtw.md](project_peers_are_fgtw.md) — decentralize FGTW: peers = the trust web; MUTUAL-CONSENT clutch SHIPPED f33ebec 2026-08-25
- [project_offgrid_wfd.md](project_offgrid_wfd.md) — Wi-Fi Direct off-grid v1 BUILT 2026-08-30 (LAN→WAN→WFD→relay, BLE=pairing-only); field test PENDING
- [project_party_colour_perceptual.md](project_party_colour_perceptual.md) — Conversation party colours are placeholder; swap to perceptual L≈50% via vsf spectral/LMS
- [project_presence_vs_online.md](project_presence_vs_online.md) — presence ≠ online (online = avatar ring, always); "show my presence" = busy/song/mood broadcast, DEFAULTS OFF
- [project_theme_rec2020.md](project_theme_rec2020.md) — fluor+photon theme.rs colours = VSF RGB lazily passed thru; convert via vsf_rgb_to_bt2020 + target Rec.2020 output on ALL platforms
- [project_incall_learner.md](project_incall_learner.md) — in-call learner + V-CHIRP probe SHIPPED; field rounds 1-5 logged; 2x-TX double capture FIXED 2026-09-08
- [project_languages.md](project_languages.md) — language catalog SHIPPED 2026-09-03: exhaustive-match Msg enum (en+es+mi), every numeral thru fmt_num, You-page picker
- [project_dms_age.md](project_dms_age.md) — DMS age = bit length of seconds-ago in dozenal glyphs (ZilaZil ≈ an hour); Dozenal settings page (rail reads Dozenal/Hexadecimal/Arabic) holds 3 base pills+why+cheat sheet+legend; NumBase/display.base
- [project_dozenal_datetime.md](project_dozenal_datetime.md) — dozenal date = `year month-glyph day` space-separated; months zero-indexed single glyphs (Feb=short Zila)
- [project_nunc_clock_check.md](project_nunc_clock_check.md) — nunc-time = clock VALIDATOR not photon's clock source; warn-only banner + ONE load-bearing use: `now` in the update stamp
- [project_update_flow.md](project_update_flow.md) — self-update + release-notice push BUILT; RELEASE_NOTES.md (Upcoming → vN at deploy) compiled into the Updates page and rendered onto the website
- [project_fleet_inbox.md](project_fleet_inbox.md) — fleet inbox DESIGNED (docs/fleet-inbox.md): inbox/<hp>/ + hub/FCM wake, events/notices never a control channel; v1 = bind-attempt alert; NOT built
- [project_doorbell.md](project_doorbell.md) — doorbell v1 BUILT 2026-07-19 (clock+ring+bells+FCM v1 sender+Kotlin wake; photon f852cbb, worker f3d621f); OPEN: sibling bell overwrite
- [project_device_sovereignty.md](project_device_sovereignty.md) — THE ownership rule for all records: subject signs, others verify-or-withhold; pending expires
- [project_identity_profile.md](project_identity_profile.md) — identity profile DESIGNED (docs/identity-profile.md): handle at rest NOWHERE, grants-only disclosure, petnames
- [project_device_loaners.md](project_device_loaners.md) — loaners DECIDED 2026-09-04: LEASE (docs/device-lease.md) — brand grant/recall, title never moves; airport = delegated session
- [project_total_loss_recovery.md](project_total_loss_recovery.md) — total-loss = custodian-authorized chain SUPERSESSION not edit; quorum-not-secret-share
- [reference_braid_novelty.md](reference_braid_novelty.md) — braid prior-art/novelty: primitives are prior art (double ratchet etc.)

- [feedback_source_map.md](feedback_source_map.md) — Keep the source map comment block at top of src/lib.rs updated when pub items or files change
- [feedback_commit_all.md](feedback_commit_all.md) — When asked to commit, include all modified files unless explicitly told otherwise
- [feedback_legacy_first.md](feedback_legacy_first.md) — Port Photon UI from legacy compositing.rs as visible-RGB RMW first; fluor under-blend is Phase 5 cleanup
- [project_vault_roadmap.md](project_vault_roadmap.md) — Vault phasing: ring tooling, GC, mid-session resurrection, bulk content (avatars/attachments/calls) all wait for device-sync phase
- [project_identity_storage_model.md](project_identity_storage_model.md) — device identity is deterministic from fingerprint (not stored); vault in app-private storage only
- [project_font_bundle.md](project_font_bundle.md) — fonts 100% BUNDLED (src/ui/fonts.rs + KAT); colour-first chain, FE0F/FE0E pick the face; dozenal glyphs fall back to Oxanium
- [project_android_color_pipeline_floor.md](project_android_color_pipeline_floor.md) — Android 1:1 panel floor: ~2% LUT residual with BT.2020+γ=2.2 buffer tag
- [feedback_orb_settings_panel.md](feedback_orb_settings_panel.md) — orb = settings/about/help panel entry; device management is a separate page in that panel
- [feedback_no_time_based_ui.md](feedback_no_time_based_ui.md) — never time-based UI: no auto-expiring toasts/banners/delayed transitions; event-shown
- [feedback_terminal_clipboard.md](feedback_terminal_clipboard.md) — spaces around `=` in dev-log output (double-click selects the value)
- [feedback_script_timestamps.md](feedback_script_timestamps.md) — every build/deploy script ends with `completed $(date)` on each success exit
- [feedback_voca_camelcase.md](feedback_voca_camelcase.md) — Default voca-encoded values to camelCase concatenation; space-separated form is opt-in for read-aloud
- [feedback_sed_address_guard.md](feedback_sed_address_guard.md) — verify a grep-derived line number is non-empty before ANY sed address op (empty address = every line; lib.rs ×1271)
- [feedback_no_comment_wraps.md](feedback_no_comment_wraps.md) — never hard-wrap comments/docstrings/markdown; one sentence per line however long (RECURRING "line wrap virus"
- [feedback_direct_pixel_no_floaters.md](feedback_direct_pixel_no_floaters.md) — rendering is DIRECT PIXEL ACCESS ONLY; no GPU shaders/vertex triangles/float pipeline ("no floaters")
- [project_textbox_one_registry.md](project_textbox_one_registry.md) — adding a textbox = register in TWO walks only (visit_app_widgets + textboxes_mut)
- [project_zero_sentinel_purge.md](project_zero_sentinel_purge.md) — zero-sentinel purge SHIPPED f1d28b3 (Option device keys); RELAY_ADDR/RosterEntry/ACK-API sentinels remain — convert when touched
- [project_arabic_indexing_fixits.md](project_arabic_indexing_fixits.md) — FIX-IT LIST: decimal-indexed VSF field names (pong sync_{i}_*, peer_{i}, profile.addrN → native multi-value fields)
- [feedback_commit_attribution.md](feedback_commit_attribution.md) — Built-With: Claude Opus <version> trailer is wanted; never Co-Authored-By Claude (tool, not author)
- [feedback_spelling.md](feedback_spelling.md) — thru/thruout/altho, and colour spelled British; the rest United Statesian
- [feedback_build_dev_script.md](feedback_build_dev_script.md) — Use ./scripts/dev.sh to compile/check photon, not bare cargo build (thrashes the machine); android dev = scripts/android/dev-adb.sh
- [project_manifestus_custodes_split.md](project_manifestus_custodes_split.md) — manifestus = storage engine (was custodes, dir renamed, package still "custodes")
- [project_device_identity_model.md](project_device_identity_model.md) — tohu device-identity crate (oracle + frozen v0 derivation); Security/Recovery axes; deferred handle-salt collision fix
- [project_keyring_design.md](project_keyring_design.md) — multi-device keyring: fleet chain + device-ADD pairing v1 SHIPPED. OPEN: braid-in of fresh device
- [project_pairing_v2.md](project_pairing_v2.md) — pairing v2 REDESIGNED 2026-07-13 words-first: binding-request registry + masked words + consent-egg bilateral Add + self-departure-only
- [reference_aarch64_cross_libs.md](reference_aarch64_cross_libs.md) — missing system lib for aarch64-linux cross-build: vendor the .so into cross-libs/aarch64 + mirror x11.pc
- [reference_site_cv_pdfs.md](reference_site_cv_pdfs.md) — holdmyoscilloscope.com = /mnt/Chiton/MEGA/holdmyoscilloscope (wrangler pages); CV PDFs via about/make-cv-pdfs.sh after cv-*.html edits
- [reference_backups.md](reference_backups.md) — MEGA mirror fixed (Harbor paths, loud fail, stamp) + PRIVATE github keys repo (push manually, --no-verify)
- [reference_keyring_signing.md](reference_keyring_signing.md) — Android signing password moved to Code/keys/TOKEN.p12.pass (keyring-free build)
- [reference_ihi_primitives.md](reference_ihi_primitives.md) — ihi has TWO one-way primitives: lossy OWF = chaos_amp/spaghettify (32-op data-dependent lossy ALU, PIPE-silicon-exact)
- [project_token_terminology.md](project_token_terminology.md) — whakaira (ceremony) vs ihi (perceptible mana); canonical glossary = ferros/GLOSSARY.md; ira/wairua/state codes fixed
- [project_session_registers.md](project_session_registers.md) — tohu session store = {identity_seed, vault_seed, handle_proof} registers, never the handle string
- [project_secret_memory_hygiene.md](project_secret_memory_hygiene.md) — hot-secret RAM handling: do-now = zeroize + mlock + no-core-dumps + copy discipline
- [reference_vsf_primary_section.md](reference_vsf_primary_section.md) — VSF readers MUST use VsfHeader::primary_section (near-form names are TOC-only, header-only sections have no body)
- [reference_claude_unguard.md](reference_claude_unguard.md) — ~/.local/bin/claude-code-unguard FORCE-opens Claude Code's Edit read guard (patches Bun binaries)
- [feedback_vsf_readers_width_agnostic.md](feedback_vsf_readers_width_agnostic.md) — VSF integers: writers auto-size (VsfType::u/i), readers widen (as_u64/as_i64), never exact-match
- [feedback_numbers_binary_at_rest.md](feedback_numbers_binary_at_rest.md) — THE number doctrine: binary at rest (wire/vault/log)
- [feedback_answer_dont_act.md](feedback_answer_dont_act.md) — user asks a QUESTION → answer and stop; never take action (esp. destructive) on a verification question; do ONLY what's asked
- [project_two_machine_git_divergence.md](project_two_machine_git_divergence.md) — after ANY commit verify HEAD == ls-remote; "missing fgtw/fluor symbol" = stale sibling, fast-forward first
## MacBook corpus (kebab-case)

- [Push after landing](push-after-landing.md) — memories LIVE in 'Claude memories/' of the public photon repo: commit+push memory writes WITH photon
- [MacBook trails remote](macbook-trails-remote.md) — MacBook clones trail with REWRITTEN history: verify against origin, reset --hard, fast-forward ALL sibling path deps together
- [photon not fmt-clean](photon-not-fmt-clean.md) — bare `cargo fmt` churns ~40 unrelated files; checkout-restore untouched files, separate style commit for the rest

- [Per-device lanes](per-device-lanes.md) — SHIPPED 53ad8f9 2026-08-13 (unpublished): any replicated-chain device transmits on its own lane; CRDT lane merge converges
- [Relay asymmetry + ping reflection](relay-asymmetry-ping-reflection.md) — FIXES 271c76c + 30e81b6 2026-08-13: ping reflection + reflect-beside-pings bootstrap (send side of Reflect never existed)

- [Notes-row ceremony wedge](self-pair-sibling-row.md) — 4417b90 FIELD-VERIFIED; mid-ceremony sleep+restart deadlock fixed 5f1535a 2026-08-13 (stall re-fire widened); fe46a74b=MACBOOK
- [Boot blindness](boot-blindness.md) — FIX SHIPPED f280fda 2026-08-12: settings+zoom at vault-open, rehydrate skips Complete, avatars local-first
- [Fleet epoch arc design](fleet-epoch-arc-design.md) — B1-B3 shipped a8b9d48/300886d + worker deployed, field-verified; remaining: hist_page/pong re-seal, row-cadence mint

- [No wrapped comments](no-wrapped-comments.md) — photon comments are one line per thought, never hard-wrapped
- [Connection flow revision](connection-flow-revision.md) — SHIPPED 2026-07-31: ladder narrates the exchange; residue = proof-echo quieting
- [Edges, not timers](edges-not-timers.md) — react on event edges (release/ACK/push), never timers or debounces
- [Commit trailer](commit-trailer-built-with.md) — never "Co-Authored-By: Claude"; end commits with "Built with Claude Fable 5"
- [Nick publishes](nick-publishes.md) — never run publish scripts; commit/push only, check only when warranted
- [No private handles](no-private-handles.md) — everything is public EXCEPT signing keys + handles (keys/ only); handles are keys, never in ANY repo; prose uses the map's petnames
- [Bilateral removal](self-only-removal.md) — SHIPPED 2026-08-31: departure = leaver's signed request + survivor's countersign; expulsion never; stolen = lockout
- [Re-clutch, never store](re-clutch-never-store.md) — recovery = fresh ceremony; secrets at rest only when absolutely required
- [Messaging solidity Phase A](messaging-solidity-phase-a.md) — A + B4 done (2026-08-09, locks commute per-key); flag-day APPROVED for B2's chain op; next: B1→B3 fleet chain+eggs arc
- [Persist findings early](persist-findings-early.md) — Nick undoes via message edits (truncates context): write load-bearing findings to memory/docs as they land
- [UI thread snapshot+CAS](ui-thread-snapshot-cas.md) — SHIPPED 2026-08-08: workers get snapshots, commits CAS live state, writers fire ACK/transmit post-durability
- [Lane rotation wedge heal](lane-rotation-wedge-heal.md) — SHIPPED 2026-08-09: peer-at-anchor + unlinkable exhausted pendings → rotate lane, re-serve rows at original stamps
- [Split contacts incident](split-contacts-incident.md) — CLOSED 2026-08-11: SHADOW CONVERSATIONS — receive arms derived convs from chains.participants, loader/persist use the contact
- [VSF TOC section-name trap](vsf-toc-section-name-trap.md) — section names live in the header TOC; bare VsfSection::parse gives name="" and == checks silently reject all
- [reference_windows_arm64_toolchain.md](reference_windows_arm64_toolchain.md) — Windows-on-ARM: aarch64-pc-windows-gnullvm via llvm-mingw at /mnt/Harbor/Code/llvm-mingw
- [reference_mingw_features_shim.md](reference_mingw_features_shim.md) — x86_64-windows breaks on pqcrypto-mlkem #include <features.h> (MinGW lacks it)
- [project_lockout_enforcement.md](project_lockout_enforcement.md) — lock @b75cc0e + UNLOCK @0f76044/fa9e765: handle-confirmed reversal, typed tombstone
- [project_fleet_epoch_arc_closed.md](project_fleet_epoch_arc_closed.md) — epoch arc CLOSED @ fa3a9c0: hist_page+pong epoch re-seal, row-cadence mint
- [project_wiped_device_roster_clobber.md](project_wiped_device_roster_clobber.md) — 2026-08-16 wiped-mac contactless: stale oracle fleet key + aead breaker clobbered roster slot
- [project_window_geometry_shipped.md](project_window_geometry_shipped.md) — window geometry SHIPPED 2026-08-16 thru fluor's model: apply_window_rect + once-per-gesture settle hook
- [project_humanitys_code.md](project_humanitys_code.md) — openness doctrine: secrecy surface = handles + keys ONLY; everything else public — "this is humanity's code"
- [settings.md](settings.md) — Nick's note: naive fixed-width unlabeled settings converted to proper VSF (the fstate v7 arc)
- [project_wave_card.md](project_wave_card.md) — WAVE CARD (2026-09-09): one row per wave + recording folds in; waveform IS the seek bar; stops envelope; all/waves/text pill; flag-day licensed
- [project_voice_calls.md](project_voice_calls.md) — calls FIELD-WORKING; 2026-09-09 Android audio = Rust-owned AAudio exclusive, HAL-stamped frames (floor 20.6 ms); docs/calls.md
- [project_xchacha_migration.md](project_xchacha_migration.md) — 2026-08-18 stack-wide ChaCha20→XChaCha20-Poly1305 (96→192-bit nonce) EVERYWHERE incl. chain stream layer
- [feedback-one-build-per-check.md](feedback-one-build-per-check.md) — NEVER run dev.sh twice to read one build: capture once to scratchpad, grep the capture (battery + heat + double relaunch)
- [project-ferros-exec-naming.md](project-ferros-exec-naming.md) — ferros exec design: no ambient cwd, bind-dont-search (petname→blake3 + sig at spawn), VSF headers not #!, package roots not $0
- [feedback_stops_not_db.md](feedback_stops_not_db.md) — Nick does STOPS not dB: 1 stop = ×2 amplitude = one bit-shift; always convert and speak stops
- [project_vault_seal_failures.md](project_vault_seal_failures.md) — CONVICTED 2026-09-04: live-LAP; lap guard + airlock doubling + prune logging + banner split SHIPPED
- [project_sibling_presence_flap.md](project_sibling_presence_flap.md) — CONVICTED 2026-09-07: relay-only sibling pair = pongs classify late/unmatched, presence flaps, 11/12 forever
