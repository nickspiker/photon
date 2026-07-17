# Photon TICKETS

Working list. The fluor migration this file used to document is **finished** — all platforms share one UI (`src/ui/photon_app.rs` under fluor), the legacy stack is deleted, and every screen (Launch/attest, Ready/contacts, Conversation, Settings panel) is live. Conventions live in [AGENT.md](AGENT.md) (+ `../fluor/AGENT.md`); build via `./scripts/dev.sh`, never bare cargo.

Item format: what's wrong / what's wanted, then any scoping notes worth keeping.

---

## Identity / device lifecycle — see [docs/lifecycle.md](docs/lifecycle.md) (the canonical flow tree, screen names, conventions; DESIGNED 2026-07-17)

Punch list from the doc, in order:
1. Redeploy the fgtw.org worker (one-owner-per-device gate + index are in source but the live worker predates them — this is what let one device double-attest two handles). Verify with a scratch two-handle genesis.
2. Device-binding marker + Launch DEVICE BUSY line (client-side one-handle-per-device gate, fires BEFORE the proof is spent).
3. KnownHandle screen (the collision flow): "This name is already claimed" with both readings — pick-another-name first, it's-mine → pairing words; no bind request posted until "It's mine".
4. LastRites red-flood interstitial: last-member detection on Depart + Remove & shred; allowed but ceremonial ("End it — forever"). Ships WITH: the client-side cleanup sweep (avatar blob via pin, submitted logs via tag), the worker purge-on-zero-fold (all hp-keyed traces deleted, name frees first-come), and the genesis-hash pin (friends pin the genesis op hash; different genesis = stranger — the no-counter successor gate). One unit: free without the pin gate is the inheritance bug.
5. JoinerSelected green flood + sponsor-confirm hold (same flood mechanism as 4).
6. Collision counter + notifications toggle on Panel→Fleet (bind_attempt alerts already flow).

## Messaging / protocol

- **Friend consent gate**: `on_search_result(Found)` auto-adds + auto-CLUTCHes — search IS befriend. Needs a pending/accept state so a stranger can't complete a ceremony + deliver a message just because someone searched a handle (observed live 2026-07-16: "an attacker just got this message"). Mutual-consent design notes in the peers-are-fgtw plan.
- **CLUTCH sibling-race**: a peer's sibling device initiating CLUTCH before we fold it into the contact's fleet gets rejected by the PT gate and gives up before the fold lands (~20 min stall observed). Fix = re-initiate after fold, or queue the offer until the fold verdict.
- **Blind ops (private identity S) E2E**: the party-id token seam is fixed (2026-07-16, tokens now derive from party ids on both ends) but the full S lifecycle (probe → generate → deposit → ack → recover) hasn't run E2E across real devices since.
- **Fleet message sync E2E**: phase-2 sibling history sync + live push shipped @ c6d6285; needs the 2-device live run (send on A, watch B; fresh-join backfill; delivered-tick propagation).
- **NAT traversal relay tier**: reflexive/punch/keepalive exist, but symmetric↔symmetric NAT pairs have NO fallback path. Needs a relay tier (friend-fleet devices as relays fits the peers-are-fgtw direction).
- **Chain-advance desync (parked)**: one observed case of ratchet divergence by msg 2 after a Complete (garbage plaintext); salt-from-last_plaintext a prime suspect. Re-test post party-id fixes before digging.

## Transport / NAT traversal — direct-path hardening

Ordered; land + 2-device-test each before the next, relay tier (above) is last. Diagnosed 2026-07-17 from mom (cellular CGNAT, MT) ↔ peer-B (fiber, Seattle) logs: mom never held peer-B's public candidate + no keepalive → one-sided punch (peer-B 209 probes, mom 0). DONE on branch `transport-resilience` (2026-07-17): **validated-path keepalive** — clamp the presence sweep to 20s while any validated path is held so CGNAT mappings don't die mid-session (`photon_app.rs` `presence_ping_interval` + `VALIDATED_PATH_KEEPALIVE`, @5f5bfdc); **punch-on-address-learned** — `refresh_contact_addrs_from_peers` fires an immediate sweep when a peer addr is learned/changed, so the late-fetch side punches at once instead of sitting silent (@b5cc6c2).

- **TCP as a direct path (T1.3)**: `tcp.rs` has NO inbound accept today — TCP is only a whole-payload PT fallback to an already-known UDP address (`pt/mod.rs` ~554). Add (a) an inbound TCP listener on `PHOTON_PORT` handing framed VSF to the same `status.rs` receiver pipeline, and (b) a parallel TCP-connect to a peer's public/forwarded candidate raced alongside the UDP punch. Wins: UDP-blocked networks (corporate / some mobile), and TCP-direct to a forwarded host "just works" (the port-forward / RustDesk-host case). Test: UDP-blocked client reaches a forwarded host over TCP.
- **IPv6 as a first-class direct path (T3)**: `gather.rs` prioritizes `HostV6` (100) but "full local-interface enumeration deferred (P2)" and the announce may not carry a global v6. Add local global-v6 enumeration + carry it in the FGTW announce (worker parse is already generic) so a v6-capable pair connects direct over v6 — NO NAT, no punch. Biggest single win for mom if her carrier is v6 (bypasses CGNAT entirely). Test: two v6 hosts connect with zero punch; verify mom's cellular gets a global v6.
- **Server-coordinated simultaneous-open (T2)**: today each side punches on its own ping cadence with no two-sided timing, and the WS channel (`wss://fgtw.org/ws`) is broadcast-only. Add targeted WS delivery in the worker (per-connection subscription by device_pubkey / handle_proof) + a "punch-request" msg: A deposits "punch B, here are my candidates, window ≈ T" → worker pushes to B → B punches back at once. Breaks the chicken-and-egg AND gives tight timing for restrictive (non-symmetric) NATs. **NEEDS WORKER DEPLOY** (`fgtw-bootstrap`, user-run). Test: cold pair (neither pre-punched) connects on first attempt.
- Then **NAT traversal relay tier** (see Messaging/protocol above): the symmetric↔symmetric / double-CGNAT case (mom↔mom) that direct can never reach.

## Updates

- **Android update NOTIFICATION**: auto-check now toasts in-app once per version; the designed platform notification (→ `REQUEST_INSTALL_PACKAGES` → system installer) is Kotlin work, not started.
- **Push doorbell for releases**: the ~6–8h poll stands in; the designed push is a release notice riding the fleet inbox / hub events (docs/fleet-inbox.md — bind-attempt alerts already flow thru it, so the plumbing half-exists).
- **Rollback**: keep `.prev` and auto-revert after N failed starts (the Windows `.old` shuffle is halfway there).
- **Idle gating + in-flight textbox hand-off across the re-exec**: auto-apply currently swaps + re-execs whenever the gates clear; a mid-typing swap loses the compose box.
- **Heal the live dev manifest**: Android row claims 0.36.12 but carries the 0.36.11 APK; Windows claims 9 with (likely) a v7 exe — mis-stamped by the publish race fixed @ 58bf7ed. Re-run `scripts/publish/dev-android.sh` + `dev-windows.sh` (user-run; bumps + stamps correctly now).

## UI / UX

- ~~Friend avatar-change refresh~~ CLOSED 2026-07-17: the pin now ROTATES on every avatar set (fresh key+lookup, old wall slot deleted after the new upload lands) — friends see the new pin on their next pong and refetch; siblings ride the profile.avatar_ts ding. Rotation also self-heals any cross-identity pin pollution (the a peer-avatar-under-peer-B incident).
- **Textbox glow on search state**: legacy tinted the search pill yellow-during / green-or-red-after a search; fluor's glow is focus-driven — recolouring per state needs a small fluor affordance (`set_glow_colour`?) + photon wiring.
- **Party colours are placeholder**: swap to perceptual L≈50% via vsf spectral/LMS (decided; supersedes the old Fibonacci-sphere colourize sketch this file used to carry).
- **Contacts search-box focus glow damage (parked)**: stale glow lingers/clips on deselect — bg pass dirty-gating skips the glow bbox.
- **CLUTCH ceremony UI-thread hitch (parked)**: ceremony completion runs ~1.2s on the UI thread ("weave feels stuck"); move off-thread.
- **Avatar encode off-thread**: mac avatar drag/drop freezes during the synchronous AV1 encode ("you have to click again after release" was this).
- **Default-share checkboxes** per profile field (excluding display name), default unchecked (queued from profile work).
- **Post-attest multi-device prompt**: right after a successful attest, prompt to add a 2nd/3rd device (redundancy IS the recovery story) + reflect in a Security/Recovery posture strip. Handle-loss warning itself is DONE (the `LaunchState::Confirm` permanence interstitial).
- **Profile rework (D)**: ONE key per base (`profile.addr`), instances = multi-value rows, identity = TAG (home/work/custom); kills `profile._custom`/`addrN` keys. Held/gated.
- **Updates-page checkbox label on Android**: reads "Install updates automatically" but Android can only notify — label should say so there.

## fluor-side

- **Italic text** (wanted: pending-contact label in italic). fluor's `TextRenderer::draw_text_*` family (~12 fns) takes only `(size, weight, colour, font)` — no style axis — and compiles in only Regular + Bold OpenSans faces; the Italic TTFs sit in photon's `assets/Open_Sans/static/` but are excluded from the package. Scope: bundle `OpenSans-Italic.ttf` (+ BoldItalic) into fluor, thread a `style`/`italic` param thru the API + call sites (or `_italic` variants), set `cosmic_text::Style::Italic` on the Attrs. Cheaper faux-italic alt: per-glyph x-shear in the blit (model on the existing `rotation` transform). Consumer waiting: `Contact::display_name_or_pending()` "Pending…".
- **Android multi-touch**: single-touch works; pinch-zoom (and the two-finger zoom hint) waits on a multi-touch `Touch` event in fluor's android host.
- **Wayland drag-and-drop** (avatar upload): winit has no `HoveredFile`/`DroppedFile` on native Wayland (winit #1881 / PR #4504). Wait for upstream or a `wl_data_device` impl in fluor.

## Platform / misc

- **Android session capsule** (de-attest-on-restart): boot-locked capsule spec'd in docs/ (spaghettify(boot_id) wairua, kete AEAD, multi-tier); not built. Root cause = Samsung kills the sticky broadcast.
- **TOKEN session relay** (Android sticky-broadcast gossip across TOKEN apps): protocol spec'd — every TOKEN app re-broadcasts every TOKEN session sticky, `PACKAGE_FULLY_REMOVED` triggers the survivors to re-fill the gap, signature-level permission gates participation. Photon's send/clear/restore side is wired. **Deferred until a second TOKEN app exists to test with.**
- **Chrome downloads on Android** (website): serve the APK so Chrome offers install, not a mystery download; or rename to `.zip` + extract instructions. Website-side.
- **macOS softbuffer present-on-clean**: legacy carried an untested "re-present even when clean or the window goes black" workaround for transparent windows; re-verify against fluor's renderer on a real Mac.
- **dev-adb.sh stale rust builds**: the adb dev deploy sometimes reuses a stale-built .so — force the rust rebuild or hash-check before packaging.

---

## Done (recent — see git log for the full trail)

- 2026-07-16 night: update stamp window (`floor < t ≤ now`, nunc tiebreak) + automatic release-channel updates (jittered ~6–8h, `updates.auto`-gated, desktop release builds self-apply, dev/Android toast) @ 228f68c · publish race fixed (pinned stamp + flock) @ 58bf7ed · blind-ops party-id token seam fixed · contact-row hover highlight · "+"-button press flash suppressed · Ready-layout handle slot collapsed (search box sits by the avatar) · EXIF orientation applied in the avatar decode · last legacy file (`ui/colour.rs`) deleted.
- 2026-07-16: fleet message sync phase 2 + roster CRDT @ c6d6285 · theme extracted to `ui/theme.rs` (fluor format) @ aa1ca3c · dozenal versioning + manifests + manual two-button updates + avatar friend-gating (ChaCha20) + per-device addressing + structured logging (earlier in the week).
- Long-since stale claims this file used to carry, all verified live: Conversation screen (bubbles, send button, Enter, ordering via sorted inserts + numeric key sort), row click → conversation, not-found select-all, auto-clear search on add, `Textbox::clear`, fleet-inbox v1 bind-attempt alerts, handle-loss warning (permanence interstitial), send-button overlap, message notifications on desktop (chirp chime; Android platform notifications still open above).
