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

## Updates

- **Android update NOTIFICATION**: auto-check now toasts in-app once per version; the designed platform notification (→ `REQUEST_INSTALL_PACKAGES` → system installer) is Kotlin work, not started.
- **Push doorbell for releases**: the ~6–8h poll stands in; the designed push is a release notice riding the fleet inbox / hub events (docs/fleet-inbox.md — bind-attempt alerts already flow thru it, so the plumbing half-exists).
- **Rollback**: keep `.prev` and auto-revert after N failed starts (the Windows `.old` shuffle is halfway there).
- **Idle gating + in-flight textbox hand-off across the re-exec**: auto-apply currently swaps + re-execs whenever the gates clear; a mid-typing swap loses the compose box.
- **Heal the live dev manifest**: Android row claims 0.36.12 but carries the 0.36.11 APK; Windows claims 9 with (likely) a v7 exe — mis-stamped by the publish race fixed @ 58bf7ed. Re-run `scripts/publish/dev-android.sh` + `dev-windows.sh` (user-run; bumps + stamps correctly now).

## UI / UX

- **Friend avatar-change refresh (pong hash)**: a re-uploaded avatar keeps its pin, so FRIENDS never refetch until a new session — the pong should carry the avatar's provenance hash next to the pin (`ahash`, optional field, old clients ignore it) and a mismatch marks the contact for refetch. Siblings are covered (profile.avatar_ts ding, 2026-07-17); friends are the remaining half.
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
