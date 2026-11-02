# Photon Project Memory Index
Hooks are reminders; read the topic file before acting on one.

## Hard rules / feedback
- [answer, don't act](feedback_answer_dont_act.md) — a question gets an answer and a stop, never an action
- [build via dev.sh](feedback_build_dev_script.md) — never bare cargo build; android = scripts/android/dev-adb.sh
- [one build per check](feedback-one-build-per-check.md) — capture once to scratchpad, grep the capture
- [test discipline](feedback_test_discipline.md) — ONE full cargo test per batch; never --lib alone
- [render never touches vault](feedback_render_never_touches_vault.md) — HARD: UI thread makes no vault call
- [no reload mid-ceremony](feedback_reload_mid_ceremony.md) — HARD: never dev.sh/deploy mid-CLUTCH or mid-WAVE; check the log
- [commit trailer](commit-trailer-built-with.md) — "Built with Claude <model>"; never Co-Authored-By
- [Nick publishes](nick-publishes.md) — Nick initiates; when asked, deploy.sh + RELEASE_NOTES.md
- [push after landing](push-after-landing.md) — memories live in 'Claude memories/'; commit+push with photon
- [no private handles](no-private-handles.md) — handles never in any repo; petnames · [doctrine](project_humanitys_code.md) · [never on disk](feedback_never_store_handles.md)
- [photon + deps only](feedback_scope_photon_and_deps.md) — HARD: never edit another project
- [stage own files](feedback_stage_only_own_files.md) — HARD: explicit paths; never git add -u/-A
- [commit = all modified](feedback_commit_all.md) · [source map](feedback_source_map.md) — keep src/lib.rs map current
- [no Python](feedback_no_python.md) — HARD; tools are Rust · [launcher is photon](feedback_launcher_named_photon.md)
- [no wrapped comments](feedback_no_comment_wraps.md) — one sentence per line; comment-gate.sh
- [edges, not timers](edges-not-timers.md) · [no time-based UI](feedback_no_time_based_ui.md)
- [direct pixel only](feedback_direct_pixel_no_floaters.md) · [smoothness](feedback_smoothness_for_humans.md) — C¹+ GUI math
- [spelling](feedback_spelling.md) — thru/altho; colour British; rest US
- [photons are messages](feedback_photons_are_messages.md) · [atoms, not notes](feedback_atoms_not_notes.md) · [self is a contact](feedback_self_is_a_contact.md)
- [wave/beam, never call](project_wave_beam_transition.md) — HARD: wave = audio, beam = video
- [stops not dB](feedback_stops_not_db.md) · [voca camelCase](feedback_voca_camelcase.md) · [orb = settings panel](feedback_orb_settings_panel.md)
- [any-size conversation](feedback_any_size_conversation.md) — HARD: 1, 2 or N = ONE model; friendship/molecule split is debt
- [handles byte-precise](feedback_handles_byte_precise.md) — HARD: full Unicode, whitespace-only valid
- [VSF readers widen](feedback_vsf_readers_width_agnostic.md) · [numbers binary at rest](feedback_numbers_binary_at_rest.md) — display Arabic, dozenal = Base setting
- Shell traps: [sed address guard](feedback_sed_address_guard.md) · [perl UTF-8](feedback_perl_utf8_trap.md) · [perl pipe delimiter](feedback_perl_pipe_delimiter.md) · [pgrep self-match](feedback_pgrep_self_match.md)
- [fgtw deploys freely](feedback_fgtw_deploy_freely.md) · [terminal clipboard](feedback_terminal_clipboard.md) — spaces around = · [script timestamps](feedback_script_timestamps.md)
- [no redundant disk ops](feedback_no_redundant_disk_ops.md) — reflink · [legacy first](feedback_legacy_first.md) · [persist findings early](persist-findings-early.md)
- [epoch is fixed](feedback_epoch_fixed.md) — HARD: never move it · [no compat text hatches](feedback_no_compat_text_hatches.md) — retired readers die same day
- [re-clutch, never store](re-clutch-never-store.md) · [discard, not shred](feedback_discard_not_shred.md) — HARD: delete = mark for discard
- [device root never salted](feedback_device_root_never_salted.md) — HARD: one stable machine-ID root; DMV plates until PIPE; vendor-issued ok, vendor-in-the-loop no

## Active / open
- [Android dark in background](project_android_headless_dark.md) — fix built, verify pending
- [Backfill dings](project_backfill_dings.md) · [Redox dropped](project_redox_dropped.md) · [Bindreq peer-to-peer](project_bindreq_peer_to_peer.md) — BUILT 2026-10-26
- [Fleet over the mesh](project_fleet_over_mesh.md) — BUILT 2026-10-26; attest degrades to held chain next; field test pending
- [Beams](project_beams.md) — H.264 BUILT; [standing queue](project_beam_standing_queue.md) FIXED
- [PT path pace](project_pt_shared_congestion.md) — 2026-10-29 one delay window per path; field test pending
- [Census eats copy B](project_census_eats_copy_b.md) — RESOLVED 2026-11-01 (~/.local/lib/photon)
- [opsin byte order](project_opsin_android_byte_order_async_loads.md) · [Chain = text + pointers](project_chain_text_and_pointers.md) — DOCTRINE
- [HDR stack](project_hdr_stack.md) — peak metric open · [New-device history walk](project_new_device_history_walk.md)
- [Phonebook strict anchor](project_phonebook_strict_anchor.md) — FIXED · [Removal safety](project_device_removal_safety.md) — phases 2-3 BUILT
- [Fleet holds everything](project_fleet_holds_everything.md) — DOCTRINE · [Scroll as memmove](project_scroll_memmove.md) — BUILT
- [Ideas page](project_ideas_gripes.md) — BUILT 2026-10-28 · [Braid salt codec](project_braid_salt_codec.md) — CONVICTED
- [Ceremony claim-first](project_ceremony_claim_first.md) — BUILT, OPEN bits · [Consent order deadlock](project_consent_order_deadlock.md) — FIXED 2026-10-29
- [Notification tap](project_notification_tap_unwired.md) — BUILT · [Strip selection](project_strip_selection_model.md) — manual only
- Render: [ready first paint](project_ready_first_paint.md) · [hit wipe inside damage](project_hit_wipe_damage.md)
- [Member relay](project_member_relay.md) · [Sibling group pages](project_sibling_group_pages.md) — FIXED · [Sticky catching up](project_sticky_catching_up.md) — FIXED v132
- [Old-self import removal](project_old_self_import_removal.md) — DELETE at v145
- [Group waves](project_group_waves.md) — BUILT; field test pending · [Group media at any size](project_group_media_scale.md) — 1a+1b+roll BUILT; 2a next
- [Address sets](project_address_sets.md) — stages 1-3 BUILT; Windows + reflexive split open
- [Group avatars](project_group_avatars.md) — BUILT 2026-11-02; field test pending
- [Android Widevine plate](project_android_widevine_plate.md) — 2026-11-02 KEY root = Widevine ‖ cert ‖ ANDROID_ID, PLATE = BLAKE3(Widevine ‖ cert) survives a wipe; conversion BUILT (untested on a phone); plate publication NEXT; pin until v168
- [Security audit](project_security_audit.md) — batch 1 FIXED 2026-11-02; design items await Nick; Mediums next
- [Group pigeon stuck fetch](project_group_pigeon_stuck_fetch.md) — FIXED · [Self atom on demand](project_self_atom_on_demand.md) — FIXED
- [Wave replay + beam video](project_wave_replay.md) — BUILT unverified · [Beam settings + g2t](project_beam_settings.md) — BUILT, field test pending
- [Emma bond era split](project_emma_bond_era_split.md) — FIXED (docs/lanes.md) · [Emma–Brittany group fork](project_emma_brittany_group_fork.md) — FIXED in code
- [File drag-out](project_file_drag_out.md) — BUILT unverified · [One conversation model](project_conversation_convergence.md) — step E SHIPPED v132; F next
- [molecules build](project-molecules-build.md) — phase 1 BUILT · [clutch completion rebroadcast](project_clutch_completion_rebroadcast.md) — fixed
- [era ratchet](project_era_ratchet.md) — stages 1-4 SHIPPED; 5 consent, 6 soak next
- [multimonitor](project_multimonitor_status.md) — phase D + Windows unbuilt · [black span after resolution](project_black_span_after_resolution.md) — FIXED
- [UI cleanup](project_ui_cleanup.md) — opsin panel done; HUD + insets + screens next
- [notifications pinned](project_notifications_pinned.md) · [Windows dark theme bug](project_windows_dark_theme_bug.md) — PINNED
- [doorbell](project_doorbell.md) — v1; OPEN sibling bell overwrite · [off-grid WFD](project_offgrid_wfd.md) — v1, field test pending
- [AAudio ignores clockid](project_aaudio_clockid_ignored.md) · [media fallback rocker](project_media_fallback_rocker.md)
- [aligner windup](project_aligner_windup.md) · [playout L/l slips](project_playout_l_slips.md) · [contact manage](project_contact_nuke_gaps.md) — v106
- [braid v2](project_braid_v2.md) — per-strand S · [audio picker + levels](project_audio_picker_levels.md) — wave field BUILT
- [log sweep eats fresh](project_log_sweep_eats_fresh.md) — never convicted · [arabic indexing fixits](project_arabic_indexing_fixits.md) · [zero sentinel purge](project_zero_sentinel_purge.md)
- [rekey attack surface](project_rekey_attack_surface.md) — docs/rekey-threat-model.md · [canonical wave container](project_wave_canonical_container.md) — LOCKED
- [recording fills](project_recording_fills.md) — next offline AEC · [fleet epoch arc](fleet-epoch-arc-design.md) — B1-B3 shipped
- [messaging solidity A](messaging-solidity-phase-a.md) · [succession emit unwired](succession-emit-side-unwired.md) · [keyring design](project_keyring_design.md)

## Designs / doctrine (docs/ are canonical)
- [device sovereignty](project_device_sovereignty.md) · [bilateral removal](self-only-removal.md) · [identity never dies](project_identity_never_dies.md)
- [lifecycle flows](project_lifecycle_flows.md) · [identity profile](project_identity_profile.md) · [device loaners = lease](project_device_loaners.md)
- [total-loss recovery](project_total_loss_recovery.md) · [fleet inbox](project_fleet_inbox.md) — NOT built · [fleet routing scale](project_fleet_routing_scale.md)
- [presence vs online](project_presence_vs_online.md) — broadcast defaults OFF · [pairing v2](project_pairing_v2.md) · [peers are fgtw](project_peers_are_fgtw.md)
- [fleet key redesign](project_fleet_key_redesign.md) · [braid novelty](reference_braid_novelty.md) · [ferros exec naming](project-ferros-exec-naming.md)
- [token terminology](project_token_terminology.md) · [secret memory hygiene](project_secret_memory_hygiene.md) · [session registers](project_session_registers.md)
- [identity storage model](project_identity_storage_model.md) · [device identity model](project_device_identity_model.md) — tohu oracle + frozen v0
- [vault roadmap](project_vault_roadmap.md) · [retention winnow](project_retention_winnow.md) · [storage layering](project_storage_layering.md) · [rarangi messages](project_rarangi_messages_fleet.md)
- [avatar encryption wall](project_avatar_encryption_wall.md) · [VSF canonical signing](project_vsf_canonical_signing.md) — ONE scheme · [nunc clock check](project_nunc_clock_check.md)
- [theme Rec.2020](project_theme_rec2020.md) · [Android IME model](project_android_ime_model.md) · [level plan](project_level_plan_reaim.md) · [in-call learner](project_incall_learner.md)
- [Eagle units](project_eagle_units_first_principles.md) · [numeral forms](project_numeral_forms.md) · [DMS age](project_dms_age.md) · [dozenal datetime](project_dozenal_datetime.md)
- [one button renderer](project_button_one_renderer.md) · [one textbox registry](project_textbox_one_registry.md) · [font bundle](project_font_bundle.md)

## Shipped arcs
- [great cleanup](project_great_cleanup.md) · [wave card](project_wave_card.md) · [waves](project_waves.md) · [attachments](project_attachments.md)
- [viewer is opsin](project_viewer_is_opsin.md) · [pigeon fetch fanout](project_pigeon_fetch_fanout.md) · [links](project_links.md) · [languages](project_languages.md) — 16
- [update flow](project_update_flow.md) · [unattended reboot](project_unattended_reboot.md) · [bridge](project_bridge.md) · [chain replication](project_chain_replication.md)
- [per-device lanes](per-device-lanes.md) · [fleet braid plane](project_fleet_braid_plane.md) · [reserve delivery](project_reserve_delivery.md) · [history recovery](project_history_recovery.md)
- [token private identity](project_token_private_identity.md) · [settings typed values](project_settings_typed_values.md) · [xchacha migration](project_xchacha_migration.md)
- [lockout enforcement](project_lockout_enforcement.md) · [UI thread snapshot+CAS](ui-thread-snapshot-cas.md) · [lane rotation wedge heal](lane-rotation-wedge-heal.md)
- [connection-flow ladder](connection-flow-revision.md) · [window geometry](project_window_geometry_shipped.md) · [wave fishtank cellular](project_wave_fishtank_cellular.md)
- [fgtw migration](project_fgtw_migration_state.md) · [fgtw stays std](project_fgtw_nostd_deferred.md) · [manifestus/custodes split](project_manifestus_custodes_split.md)
- [colour pipeline doctrine](project_colour_pipeline.md) — wide gamut, tag the surface, γ2 never sRGB OETF · [epoch arc closed](project_fleet_epoch_arc_closed.md) — fa3a9c0

## Closed incidents
- [address ports](project_address_ports.md) · [hotplug abort](project_hotplug_abort.md) · [pigeon fetch reach](project_pigeon_fetch_reach.md) · [early-stop blindness](project_history_early_stop_blindness.md)
- [fluor busy loop](project_fluor_busy_loop.md) · [macOS resize cursor](project_macos_resize_cursor.md) · [party colour](project_party_colour_perceptual.md)
- Perf: [ANR-at-launch](project_android_hang_nag.md) · [idle-CPU](project_idle_tick_cost.md) · [render lag](project_render_storm_lag.md)
- Vault: [seal failures](project_vault_seal_failures.md) · [plow refusal](project_manifestus_plow_reloc_refusal.md) · [tombstone](project_manifestus_tombstone_bug.md)
- CLUTCH/braid: [desync class](project_chain_advance_desync.md) · [offer deadlock](project_clutch_offer_deadlock.md) · [UI-thread hitch](project_clutch_ui_thread_hitch.md)
- [traversal relay gap](project_traversal_relay_gap.md) — RELAY MEDIA unbuilt · [no-ring class](project_call_no_ring_incident.md) · [sibling presence flap](project_sibling_presence_flap.md) · [ping reflection](relay-asymmetry-ping-reflection.md)
- Android: [session capsule](project_android_session_capsule.md) · [crash handler](project_crash_handler_sigchain.md) · [colour floor](project_android_color_pipeline_floor.md)
- [token asymmetry](project_clutch_token_asymmetry.md) · [shadow conversations](split-contacts-incident.md) · [notes-row wedge](self-pair-sibling-row.md) · [self-msg vanish](project_self_message_vanish.md) · [put latency](project_vault_op_latency.md)
- [relay pipe](project_nat_traversal_relay_gap.md) · [roster clobber](project_wiped_device_roster_clobber.md) · [FGTW key desync](project_fgtw_key_desync.md) · [glow](project_contacts_glow_damage.md) · [two-machine git](project_two_machine_git_divergence.md)

## References
- [log pull](reference_log_pull.md) — photonlog --pull; pull to a file first · [desktop trails spirix](project_desktop_trails_spirix.md) · [MacBook trails remote](macbook-trails-remote.md)
- [not fmt-clean](photon-not-fmt-clean.md) · [VSF primary section](reference_vsf_primary_section.md) · [ihi primitives](reference_ihi_primitives.md) · [claude unguard](reference_claude_unguard.md)
- [aarch64 cross libs](reference_aarch64_cross_libs.md) · [Windows arm64 toolchain](reference_windows_arm64_toolchain.md) · [mingw features shim](reference_mingw_features_shim.md)
- [site CV PDFs](reference_site_cv_pdfs.md) · [backups](reference_backups.md) · [keyring signing](reference_keyring_signing.md) — Android signing password in Code/keys
