# Photon TICKETS

Working list for Photon. Top section is the **fluor-migration handoff** (current architectural transition, intended for an agent picking up cold); legacy backlog follows.

---

# Fluor migration — handoff notes

Photon has been rewritten onto the **fluor** GUI toolkit (`../fluor`, path dep). **All platforms now share one UI**: `src/ui/photon_app.rs` impls `fluor::host::app::FluorApp` and runs under `fluor::host::app::run_app` on desktop (host-winit) and `fluor::host::android::AndroidShell` on Android (host-android, Phase 2.3+). The legacy stack — `src/ui/{app,compositing,colour,drawing,keyboard,mouse,text_editing,text_rasterizing,renderer_android}.rs` — is **retired**: the files still sit on disk but are no longer in the module tree (no `mod` declaration in `ui/mod.rs`), so nothing compiles them. They're dead code, kept only as a porting reference, pending deletion. This was **break-and-rebuild on main, no fork, no backwards compatibility** — small team, manually-coordinated update; the in-progress binary is not anyone's daily messenger.

**Full plan**: `/home/nick/.claude/plans/buzzing-puzzling-yao.md` (the original strategy + screen-by-screen phasing — note it predates the Android migration landing, so its carveouts are partly out of date). What's changed since: **Android migration is done** — fluor grew `host-android`, and Android now runs the same `photon_app` as desktop (legacy compositor retired). Still outstanding from the original carveouts: no softbuffer-patch port, no Spirix text yet.

## Conventions — these are load-bearing

Read [AGENT.md](AGENT.md) (Photon's) and [../fluor/AGENT.md](../fluor/AGENT.md) before writing code. The high-impact rules:

- **Build via `./build-development.sh`**, NOT `cargo build --release`. The release path is reserved for ship builds; development builds are dev profile with debug-info. Per Photon's AGENT.md: *"DO NOT DO RELEASE BUILDS UNLESS THE USER EXPLICITLY ASKS!"*
- **Commit trailer is `Built-With: Claude <model> <version>`** — NEVER `Co-Authored-By:`. The user wants attribution as "built with the tool", not as a co-author.
- **No publishing fluor** without an explicit `publish X.Y.Z please` directive from the user. The 0.0.2 publish in this branch's history was unauthorized and was yanked from crates.io — version slot is permanently consumed; next publish would be 0.0.3.
- **Power-of-two constants** for any tuning knob you pick — `let phase = bg_scroll as f32 * (1. / ((1 << 7) as f32));` not `* 0.0078125`. Algorithm constants from papers / reference impls (REC2020 matrix, harmonic-mean numerator) stay as-is. See AGENT.md "Power-of-Two Constants" section.
- **No trailing zeros on float literals**. `1.` not `1.0`. `2.` not `2.0`. Strip them even on precise matrix coefficients (`3.168241098811690` → `3.16824109881169`).
- **Rule 0 (AGENT.md)**: no bounds checks / clamps without a written proof. The wave & logo composite paths rely on this — bounds proven structurally, no `.get()`/`.min()` defensive ops in the hot loop.
- **Decimal indexing is FORBIDDEN** (AGENT.md). VSF nested sections, not `s0_`/`s1_` string prefixes.
- **No mocks in tests**. Integration tests hit the real thing. Reason: prior incident where mock/prod divergence masked a broken migration.
- **VSF high-level APIs only** — `SectionBuilder` + `SectionSchema`, not manual byte manipulation. The recent `c414c45` commit caught up to VSF's post-rename API (`l` instead of `L`, `n` instead of `m`, `creation_time` now `Option<VsfType>`).
- **fluor is `v0` until told otherwise** — the user does not bump versions lightly.

## Where the code lives

### Desktop entry point — the new world
- [src/main.rs](src/main.rs) — 100-line desktop entry. Just `fluor::host::app::run_app(PhotonApp::new())` + signature verification + panic hook + Linux XCURSOR_SIZE nudge. All winit / softbuffer / decorations / refresh-poll setup belongs to fluor now.
- [src/ui/photon_app.rs](src/ui/photon_app.rs) — `pub struct PhotonApp` + `impl FluorApp for PhotonApp`. THE central file for desktop. Owns: `DefaultChrome`, hit-counter, event-loop proxy, `bg_scroll`, chord state. Handles every WindowEvent, drives render order, wires layout to widgets.
- [src/ui/launch_layout.rs](src/ui/launch_layout.rs) — proportional-slicing layout calculator (port of `Layout::new`'s Launch arm). 22.75 vertical units; spectrum at top, logo overlapping (gap = -2), attest block below. Add Ready/Searching/Conversation layout structs here.
- [src/ui/chromatic_wave.rs](src/ui/chromatic_wave.rs) — sine-modulated visible-spectrum colour bar. Port of legacy `draw_spectrum`. α + darkness format; scroll drives phase; `period_scale` parameter (currently held at `1.`).
- [src/ui/photon_logo.rs](src/ui/photon_logo.rs) — "Photon" wordmark. Three-layer composition (glow + sharp body + highlight rim), Oxanium 800. Port of legacy `draw_logo_text`.
- [src/ui/lms2006so.rs](src/ui/lms2006so.rs) — Stockman-Sharpe LMS2006SO colour-matching data, lifted from `colour.rs` so desktop can read it without dragging nalgebra in. Re-exported thru `colour` for Android compat.
- [src/ui/state.rs](src/ui/state.rs) — `AppState` enum (`Launch(LaunchState) | Ready | Searching | Conversation | Connected{..}`), `FoundPeer`, `SearchResult`. Un-gated (network code depends on it).
- [src/ui/mod.rs](src/ui/mod.rs) — module wiring. The legacy modules are **gone from the tree** (no `mod` declarations); `ui` is the fluor `photon_app` set, shared by desktop and Android. Its header comment is the canonical statement of the retired-legacy / shared-fluor-UI state.

### Legacy stack — RETIRED (orphaned on disk, not compiled, pending deletion)
These files still exist but are in **no `mod` declaration**, so they compile on no platform — dead code kept only as a porting reference until deleted. (They were never truly "Android-only"; Android now runs the same fluor `photon_app`.) What each was the source of:
- [src/ui/app.rs](src/ui/app.rs) (~6,018 lines) — the old `PhotonApp` state machine + render loop. Mine it for any not-yet-ported screen logic (Conversation), then delete.
- [src/ui/compositing.rs](src/ui/compositing.rs) (~5,817 lines) — `draw_spectrum` → chromatic_wave.rs (done), `draw_logo_text` → photon_logo.rs (done), `draw_textbox` → `fluor::widgets::Textbox` (done), avatar / contact-row / message-bubble paint paths (rows done; bubbles pending Conversation).
- [src/ui/colour.rs](src/ui/colour.rs) — nalgebra LMS-to-* matrices; the LMS2006SO array moved to `lms2006so.rs` (live).
- `src/ui/text_rasterizing.rs` — cosmic-text + swash wrapper → `fluor::text::TextRenderer` (same engine).
- `src/ui/{text_editing,mouse,keyboard}.rs` → fluor's `Textbox` widget + `FluorApp` event dispatch.
- `src/ui/drawing.rs` — legacy noise + edges → `fluor::paint::background_noise` etc.
- `src/ui/renderer_android.rs` → superseded by `fluor::host::android` (host-android landed).

### Untouched — survives the migration
- `src/network/*` — TCP + FGTW + handle queries + announcements. Will reconnect to new UI when attest-action wiring lands.
- `src/crypto/*`, `src/ihi/*` (or the `ihi` crate) — CLUTCH / CHAIN / spaghettify protocol code.
- `src/storage/*` — flat-file storage, contacts, blob parsing. Already on the new VSF API (commit `c414c45`).
- `src/avatar.rs` — LRU cache. Will be wrapped by a Photon-side `Avatar` widget in Phase 2.
- `src/types.rs`, `src/platform/*`, `assets/*` (fonts + icons + signing keys).

## Conventions / pixel-format notes

- **fluor pixel format is α + darkness**: top byte = α opacity (`0xFF` = opaque), low 24 bits = darkness (`visible = pixel ^ 0x00FFFFFF`). The OS-boundary finalize flips back to visible RGB once at the very end. Photon's legacy 0xAARRGGBB is gone on desktop — every new paint primitive emits α + darkness.
- **Theme constants**: `src/ui/theme.rs` STILL uses the legacy 0xAARRGGBB format because it's shared with Android. Phase 5 cleanup (per migration plan) ports it to `fluor::theme::dark(...) + pack_argb(...)` mechanically. Until then, new desktop code that needs theme colours either inlines the dark()-converted value or calls `dark(theme::FOO)` at the use site. Logo, chromatic wave, and chord system all inline.
- **Hit-test IDs** are dense `u16`. `hit_counter: HitId` on `PhotonApp` is monotonic; chrome takes 1..=4 at construction; widget constructors threading `&mut self.hit_counter` get 5+. Slot 0 = `HIT_NONE`.
- **Damage rect**: default is full-viewport. PhotonApp overrides `damage_rect` to union the chord-hint bbox when `[+]` are held. Narrow further as widgets land.

## What's done (commit-by-commit)

Recent commits (`git log --oneline` for the canonical sequence):

- `cdc997a` — Phase 0a: fluor path dep.
- `c414c45` — VSF API catch-up (L→l, m→n, `creation_time` → `Option`).
- `f64727a` — Phase 0d: main.rs hands off to `run_app`.
- `6d95b8b` — Phase 0c: `PhotonApp` scaffold + `DefaultChrome` (chrome-only render).
- `59ea9c1` — Phase 0b+0e+0f: legacy UI stack cfg-gated to Android; `state.rs` lifted.
- `5a82465` — wires event dispatch (hover/click/resize/drag) — fixed the "buttons don't work" gap.
- `549b330` — full-edge chrome when maximized.
- `f097130` — Phase 1a: scroll-driven shimmer in background noise.
- `0f98f24` — zero `scroll_offset` on Launch (shimmer only, no vertical translate).
- `4689655` — drop the legacy `compositing::draw_background_texture` pass-through.
- `9c3216b` — chromatic wave on Launch (port of `draw_spectrum`).
- `5e5ebb1` — portrait launch window via new `FluorApp::initial_size` hook.
- `2827580` — launch layout calculator + debug chord system (`[+]` + letter).

**Uncommitted at the time of this writeup**: `src/ui/photon_logo.rs` (new), plus the Oxanium-loading + `WindowEvent::Focused` handler additions in `photon_app.rs` + the `pub mod photon_logo;` line in `mod.rs`. Build clean. **Should be one commit**: `ui: photon wordmark + unfocused chrome dim`.

Corresponding fluor commits (in `../fluor`):

- `5aa889e` — fluor v0.0.1 release (warnings-clean + docs).
- `10b7118` — `FluorApp::UserEvent` associated type + cross-thread wake-up.
- `50b4ac0` — `background_noise` speckle→shimmer rename + power-of-two AGENT rule.
- `a4b39a6` — `FluorApp::initial_size` hook.

## Current status (this is well past the Phase-0 notes above)

The **Launch/attest screen is complete** and the **Ready/contacts screen is largely built**. The commit-by-commit list above is historical (Phase 0); since then the desktop app has gained, on `photon_app.rs`:

- **Launch/attest**: handle textbox + Attest button (button only appears once a handle is typed; a grey ∞ placeholder sits in the empty field), attesting lockout (textbox + button go inert, no hover/click, during the query), error→refocus+select-all, clipboard (Ctrl/Cmd C/X/V via arboard), Tab/Esc focus, "handle" hint, "Attesting…"/error status, app-icon orb wired as the FGTW connectivity indicator, and the full attestation wire-up (Enter/click → `network/*` → `AppState::Ready`).
- **Chrome title bar is dynamic**: "← Network" on launch, live "N peers" on Ready (the OS taskbar title stays "Photon").
- **Version watermark** (bottom, dozenal glyphs from the Oxanium `+glyphs` control-code block, sourced from the **Cargo.toml version** — `v` file retired) and a **zoom-% watermark** (top, only while actively zooming).
- **Ready/contacts**: avatar (drag/drop to update via the new `DroppedFile` path → `set_avatar_from_file`; tap-to-pick on Android; hover/click update hint), search/add textbox with placeholder, the "+" button, the **rotating-hourglass search-in-flight cue** + result text ("added {h}" / "not found" / "error"), and a **scrollable, filterable contact list** with presence rings.

Corresponding fluor gains: `DroppedFile` event + winit mapping, `Viewport` zoom clamp (12.5–300%, release-gated), `chrome.set_title`, textbox arrow-collapses-to-selection-edge, `fill_rect` 0-dimension hairline convention, `draw_app_icon` thin-hairline ring.

## Open tickets — active

**Contacts / search screen**
- **Search box too far from the avatar** — the "search | add" textbox should sit much closer to the avatar, UNLESS the user has opted to show their handle below the avatar (then the handle slot reclaims that space). Today the `handle` slot in `ReadyLayout` always reserves its units even when the handle isn't shown; collapse those units when the handle is hidden.
- **Plus-button press fill is nasty** — clicking the "+" shows a weird fill while held. It should look identical to its idle state until you *release*; on release it flips to the orange + hourglass (search-in-flight). I.e. suppress the Button's pressed-state fill here, and switch to the hourglass on the release edge rather than mid-press.
- **Not-found should select-all** — when a search returns "not found", auto-select the whole word in the search box (same reflex as the attest-error refocus+select-all) so the user can retype immediately.
- **Row click → Conversation** — contact rows render + scroll + filter, but clicking one does nothing (no Conversation screen yet). Wire the geometric row hit-test → open conversation once that screen exists.
- **Row hover highlight** — legacy dimmed/brightened the contact name on hover; not ported.
- **Textbox glow on search state** — legacy tinted the search pill yellow-during / green-or-red-after; fluor's textbox glow is focus-driven, recolouring per state is a separate change.
- **Auto-clear search box on successful add** — legacy cleared it; `fluor::widgets::Textbox` has no public `clear` yet (add `Textbox::clear` or a consume-on-submit option).

**Identity / attest screen**
- **Handle-loss warning** — the attest screen must say plainly that the handle has NO recovery: forget it and you lose the identity *and* burn the name permanently — the `handle_proof` stays bound to a now-dead device, unreclaimable by you (you've lost the only proof-of-ownership input) and unavailable to anyone else (the attestation gate blocks re-claim); new hardware does not help. Wording lives by the handle field, before they commit. See tohu `docs/handle.md` § "Loss — forgetting the handle burns it."
- **Push multi-device immediately after attest** — redundancy is the only protection against handle loss, so right after a successful attest, prompt to add a 2nd and 3rd device, and reflect it in the Security/Recovery posture strip (a lone device reads Recovery-low). This is the recovery story until vouching / PIPE land. tohu stores only the seed by default (never the plain string), so "add devices" — not "write it down" — is the sanctioned answer.

**Conversation screen (Phase 4 — not started)**
- Message list (likely needs a `ScrollContainer<W: Widget>` in fluor), input bar, send button, typing indicator, delivery status, message ordering, notifications.

**Cleanup / migration tail**
- `theme.rs` still in legacy 0xAARRGGBB — port to `fluor::theme::{pack_argb, dark}` (new desktop code currently inlines `dark(fmt(...))`).
- **Delete the orphaned legacy files** — `src/ui/{app,compositing,colour,drawing,keyboard,mouse,text_editing,text_rasterizing,renderer_android}.rs`. host-android landed and Android is on `photon_app`, so they're dead code (no `mod` declaration); they survive only as a porting reference. Mine `app.rs`/`compositing.rs` for the remaining Conversation-screen logic, then remove the lot (the vestigial `v` file goes with them).
- **Colourize handles** (spec below) — hook once contact rows want a per-handle accent colour.

### Likely fluor-side enhancements still pending
- `Textbox::clear` / consume-on-submit.
- `ScrollContainer<W>` for the conversation message list.
- Android multi-touch (`Touch` event) — gates pinch-zoom and the two-finger zoom-hint.

## Non-obvious / open questions

- **Render order**: chromatic wave is currently painted as `noise → wave → logo` directly into bg_layer (legacy additive-blend pattern), NOT as fluor's topmost-first under-blend chain. The wave's per-pixel sqrt-blend doesn't naturally express as `under()`; a Phase 5 architectural cleanup could re-shape it. Identical final pixels in the opaque case; the doctrine cleanliness argument is the only motivation today.
- **photon_logo wrap-add semantics**: intentional. The legacy wrap-adds grey to bg channels, producing characteristic chromatic interactions when the bg is bright (e.g., over the spectrum bar). Do NOT "fix" it to saturating.
- **`bg_scroll` is a multipurpose state knob** — drives `shimmer` (noise colour bias cycle), wave `phase`, and was briefly driving wave `period` before the user vetoed that. Future screens may want their own scroll counter; for now it's window-global.
- **Chord system** (`[+]` + letter) is fully wired in `photon_app.rs`. Hold `[ + ]` to see the hint panel. Bindings: H hit-mask, P skip-premult, A show-alpha cycle, C skip-chrome, L skip-controls, R force-redraw, F FPS strip, W damage outline, D screen decay, B opaque-scan blue tint. ALL toggles backed by atomics in `fluor::paint::DEBUG_*`. Useful for any rendering question — *start here* when something looks wrong.
- **Initial window size**: portrait 1:2 (w:h), `h = short_axis >> 1, w = h >> 1`. On 1920×1080: 270×540. Override is in `PhotonApp::initial_size` (uses fluor's new trait hook).
- **macOS softbuffer present** (line 12 below in the legacy backlog): this is an Android-cfg-gated workaround; the new desktop renderer is fluor's, behaviour may differ. Re-test once Photon-desktop is functional enough to install on a Mac.

## Build / dev loop

```sh
cd /mnt/Octopus/Code/photon
./build-development.sh   # cargo build (dev profile) + cargo test + sign binary
target/debug/photon-messenger
```

Edit cycle: change code → run `./build-development.sh` → run binary → use chord system to inspect layers as needed.

If you only want to compile-check fast without signing: `cargo build --bin photon-messenger`.

---

# Legacy backlog (pre-fluor)

Several of these are dormant during the migration but still relevant once each screen lands. Don't fix them in the retired `compositing.rs` / `app.rs` (orphaned, pending deletion); fix in the fluor-native path.

- **Relative Unit (RU's) scaling code/variable, pinch zoom on touch** — fluor's `viewport.effective_span()` is the RU on every platform now (host-android landed, so Android no longer keeps the old code). Pinch-zoom on touch still waits on fluor's multi-touch `Touch` event.
- **Notifications for messages** — needed once Conversation screen is back online (Phase 4).
- **Message display order** — somewhat out of order. Bug in legacy; re-verify after Phase 4 rebuild.
- **Send button doesn't work** — use Enter key for now. Fix when Phase 4 wires the send button in fluor.
- **Text on send overwrites the send button** — same area; address during Phase 4.
- **Self-updates** — auto-update mechanism. Independent of UI migration.
- **Network broadcast gets stuck/lost** — `network/*` issue; investigate once the new UI can drive enough traffic to reproduce.
- **Chrome downloads on Android** — rename to `.zip` + trigger extract, not install (apk handler). Android-specific; lives behind the cfg-gate.
- **EXIF rotation** — rotated images with EXIF rotate-after-decode flags need to be handled in avatar / image pipeline.
- **Wayland drag-and-drop file support** (avatar upload) — winit doesn't support `HoveredFile` / `DroppedFile` on native Wayland ([issue #1881](https://github.com/rust-windowing/winit/issues/1881)). Need `wl_data_device` impl or wait for winit [PR #4504](https://github.com/rust-windowing/winit/pull/4504). May land in fluor as host-side feature.

## macOS softbuffer transparent-window present-on-clean

From a prior session, an untested fix landed in the now-retired `compositing.rs` (orphaned — so this snippet is dead; re-evaluate against fluor's softbuffer/wgpu renderer path instead). Quoted verbatim:

> ● Update(src/ui/compositing.rs) Gotta actually test this
>   ```rust
>   } else {
>       // macOS with transparent windows + softbuffer doesn't retain buffer contents
>       // between frames. Must re-present even when nothing changed or window goes black.
>       #[cfg(target_os = "macos")]
>       {
>           let mut buffer = self.renderer.lock_buffer();
>           buffer.present().unwrap();
>       }
>   }
>   ```

Once Photon-desktop is up enough to install on a Mac, re-verify this is/isn't needed in fluor's softbuffer path.

## Colourize handles

Generate a deterministic colour per contact handle from a 32-byte hash. Fibonacci-lattice point on unit sphere → ray from cube centre thru that point → first intersection with the RGB cube faces → that's the colour. Spreads evenly in colour space regardless of how many contacts there are.

```rust
fn colourize(hash: [u8; 32], num_handles: usize) -> [f32; 3] {
    // Convert hash to index in [0, num_handles)
    let index = hash_to_index(hash, num_handles);

    // Generate Fibonacci lattice point for this index
    let (x, y, z) = fibonacci_sphere_point(index, num_handles);

    // Sphere is centered at origin, radius 1
    // Project ray from cube center (0.5, 0.5, 0.5) thru sphere point
    // Find intersection with RGB cube [0,1]³

    let ray_dir = (x, y, z); // normalized direction
    let ray_origin = (0.5, 0.5, 0.5);

    // Find t where ray intersects cube face
    let t = intersect_cube(ray_origin, ray_dir);

    let r = 0.5 + t * x;
    let g = 0.5 + t * y;
    let b = 0.5 + t * z;

    [r, g, b]
}

fn hash_to_index(hash: [u8; 32], n: usize) -> usize {
    // Use first 8 bytes as u64, modulo n
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&hash[0..8]);
    u64::from_le_bytes(bytes) as usize % n
}

fn fibonacci_sphere_point(i: usize, n: usize) -> (f32, f32, f32) {
    const PHI: f32 = 1.618033988749895; // golden ratio

    let i_f = i as f32;
    let n_f = n as f32;

    let theta = 2.0 * PI * i_f / PHI;
    let phi = (1.0 - 2.0 * (i_f + 0.5) / n_f).acos();

    let x = phi.sin() * theta.cos();
    let y = phi.sin() * theta.sin();
    let z = phi.cos();

    (x, y, z)
}

fn intersect_cube(origin: (f32, f32, f32), dir: (f32, f32, f32)) -> f32 {
    // Ray: P = origin + t * dir
    // Find smallest positive t where P intersects cube faces [0,1]³

    let mut t_min = f32::INFINITY;

    // Check each axis for intersection with min/max faces
    for axis in 0..3 {
        let o = [origin.0, origin.1, origin.2][axis];
        let d = [dir.0, dir.1, dir.2][axis];

        if d.abs() > 1e-6 {
            // Intersect with face at 0
            let t0 = (0.0 - o) / d;
            if t0 > 0.0 && in_cube_bounds(origin, dir, t0, axis) {
                t_min = t_min.min(t0);
            }

            // Intersect with face at 1
            let t1 = (1.0 - o) / d;
            if t1 > 0.0 && in_cube_bounds(origin, dir, t1, axis) {
                t_min = t_min.min(t1);
            }
        }
    }

    t_min
}

fn in_cube_bounds(origin: (f32, f32, f32), dir: (f32, f32, f32), t: f32, skip_axis: usize) -> bool {
    let p = (
        origin.0 + t * dir.0,
        origin.1 + t * dir.1,
        origin.2 + t * dir.2,
    );

    let coords = [p.0, p.1, p.2];

    for axis in 0..3 {
        if axis != skip_axis {
            if coords[axis] < 0.0 || coords[axis] > 1.0 {
                return false;
            }
        }
    }
    true
}
```

Hook site once Phase 2 lands: avatar / contact-row widget reads `contact.handle_hash` and `contacts.len()`, calls `colourize`, gets a stable colour for that contact's accent (ring / glow / corner pip — designer's choice). Updates as the contact count changes (rings shift colour as new contacts join — feature, not bug; the deterministic distribution stays even).

---

# Suggested first move for a fresh agent

1. `cd /mnt/Octopus/Code/photon && git status` — should show photon_logo.rs untracked + photon_app.rs + ui/mod.rs modified.
2. `git diff src/ui/photon_app.rs src/ui/mod.rs` — read what's pending.
3. `./build-development.sh && ./target/debug/photon-messenger` — verify it boots and the wordmark + chrome look right.
4. If yes, commit (`ui: photon wordmark + unfocused chrome dim` with `Built-With:` trailer).
5. Read the plan file at `/home/nick/.claude/plans/buzzing-puzzling-yao.md`.
6. Start on **handle textbox** (slice 4 above) — drop a `fluor::widgets::Textbox` into the `attest_block` region of `LaunchLayout`. The migration plan section "Phase 1c — Handle textbox" has more detail.
