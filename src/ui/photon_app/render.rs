//! `render_frame` — the whole-frame paint, split out of the `FluorApp::render` trait method so the frame body lives in its own file.

use super::*;

impl PhotonApp {
    /// The full frame paint — the body of [`FluorApp::render`], verbatim; the trait method in `driver.rs` delegates here so the paint code can live in its own file.
    pub(super) fn render_frame(&mut self, target: &mut [u32], ctx: &mut Context) {
        // Standing render probe (born in the 2026-08-08 typing-lag hunt, kept for regressions). A Drop guard so it fires on every return path. The bar is a MISSED 60fps FRAME: the phone's healthy full-viewport render is 9-16ms, and the hunt's original 8ms bar logged every one of those — 3,326 lines in a 15-minute field log, the single biggest log-volume source (2026-08-09).
        struct RenderTimer(std::time::Instant, &'static str);
        impl Drop for RenderTimer {
            fn drop(&mut self) {
                let ms = self.0.elapsed().as_millis();
                if ms > 16 {
                    crate::logf!("PERF: render took {}ms on {} (UI thread)", ms, self.1);
                }
            }
        }
        let _rt = RenderTimer(
            std::time::Instant::now(),
            // Every state named exactly — a sustained render storm hid behind "other" in a 2026-08-15 field log and the label couldn't say WHICH screen was looping.
            match self.state {
                AppState::Conversation => "Conversation",
                AppState::Ready => "Ready",
                AppState::Launch(_) => "Launch",
                AppState::Searching => "Searching",
                AppState::AddDevice => "AddDevice",
                AppState::Connected { .. } => "Connected",
                AppState::Settings(SettingsPage::You) => "Settings:You",
                AppState::Settings(SettingsPage::Fleet) => "Settings:Fleet",
                AppState::Settings(SettingsPage::Security) => "Settings:Security",
                AppState::Settings(SettingsPage::Recovery) => "Settings:Recovery",
                AppState::Settings(SettingsPage::Appearance) => "Settings:Appearance",
                AppState::Settings(SettingsPage::Notifications) => "Settings:Notifications",
                AppState::Settings(SettingsPage::Updates) => "Settings:Updates",
                AppState::Settings(SettingsPage::Diagnostics) => "Settings:Diagnostics",
                AppState::Settings(SettingsPage::About) => "Settings:About",
                AppState::ContactPanel(_) => "ContactPanel",
            },
        );
        // Press-hold-release: sync the "held" visual on every clickable WIDGET (attest / + / send Buttons) to the pointer arbiter's currently-pressed hit id. On desktop the host's overlay pass then paints the held tint from each Button's `tint_delta`; the app's own hit-stamped elements (pills, contact rows, nav rows) read `ctx.pressed_hit` directly further down. Must run before the widget tree is walked for overlay deltas (post-render), so a press lights up the same frame.
        let pressed_hit = ctx.pressed_hit;
        widget::apply_pressed(self, pressed_hit);
        // Publish the frame's hovered hit id for every immediate-mode action pill (settings/launch/contact-panel) so fluor's shared pill renderer can light the hovered one — the hover the hand-rolled pills never had. One set, read by draw_stub_pill* below; retained Buttons get hover via the overlay-delta pass instead.
        super::set_stub_hover(self.hover_hit);
        // Compute chord-held state BEFORE taking the mutable `chrome` borrow — `brackets_held` reads `&self` and the chrome borrow lives thru the entire render. Update `last_chord_held` here too so the next frame's `damage_rect` knows whether to include the hint bbox for the one-frame clear.
        let held_now = self.brackets_held(Instant::now());
        self.last_chord_held = held_now;
        let show_hitmask = self.show_hitmask;
        // Snapshot the colour table so the post-flatten hitmask overlay can read it after the chrome borrow ends.
        let buf_w = ctx.viewport.width_px as usize;
        let buf_h = ctx.viewport.height_px as usize;

        // Arm the zoom hint: the host swallows zoom events and mutates `ru` directly, so we detect a zoom by `ru` changing frame-to-frame. Arm only when a zoom modifier is held (so a programmatic/resize ru change wouldn't trigger it, and merely holding Ctrl with no scroll doesn't either — the change is what arms it). `ModifiersChanged` clears it on release.
        let zoom_mod_held = ctx.modifiers.control_key() || ctx.modifiers.super_key();
        if ctx.viewport.ru != self.last_ru {
            if zoom_mod_held {
                self.zoom_hint = true;
            }
            self.last_ru = ctx.viewport.ru;
        }
        // Dev-only: the zoom-% readout is a debugging aid, not a shipped affordance. Desktop shows it while a zoom modifier is held after a change (`zoom_hint`); Android pinch-zoom has NO keyboard modifier to arm/clear against, so there we show it whenever `ru` sits away from 100% — always accurate, no touch-release event needed (which fluor's multi-touch layer doesn't emit yet).
        let show_zoom = cfg!(feature = "development")
            && (self.zoom_hint
                || (cfg!(target_os = "android") && (ctx.viewport.ru - 1.0).abs() > 0.001));

        // The open conversation's contact row + compose gate, resolved ONCE before the chrome borrow — the borrow lives thru the whole render, so no `&self` method can run past this point.
        let active_ci = self.active_contact();
        let compose_ready = self.compose_ready();
        // Prompt-gate snapshot (same pre-chrome discipline): bridge command in flight → the send arrow dims and submit refuses.
        let bridge_held = active_ci.map_or(false, |ci| {
            self.contacts.get(ci).map_or(false, |c| c.is_sibling)
                && self.bridge_inflight_target(ci).is_some()
        });
        // The ranked reaction strip, same pre-chrome discipline (reads fleet settings thru &self). Cheap: a prefix scan of the settings map.
        let ranked_reactions = self.ranked_reactions();
        // Title-bar text by screen, computed BEFORE the chrome borrow (peer count reads `self.handle_query` / `self.session`). Launch/attest shows the "← Network" affordance; once attested (Ready) it shows the peer count — distinct identities in the store EXCLUDING our own: peers are PEOPLE, so the FGTW seed is not a peer (the old `+1` when online) and neither are our own fleet siblings (their records ride the same store for direct routing). `set_title` only re-rasterizes chrome when the string actually changes, so this is cheap to recompute each frame.
        let title_text: String = if matches!(
            self.state,
            AppState::Conversation | AppState::ContactPanel(_)
        ) {
            active_ci
                .and_then(|ci| self.contacts.get(ci))
                // Pending… until they publish a real name — the title bar is a visual surface; the pseudonym lives ONLY in the contact panel's identity section (Nick 2026-08-21, matching the contact list).
                .map(|c| c.display_name_or_pending())
                .unwrap_or_else(|| "Conversation".to_string())
        } else if matches!(self.state, AppState::Ready) {
            let own_hp = self.session.as_ref().map(|s| s.handle_proof);
            let n = self
                .handle_query
                .as_ref()
                .and_then(|hq| hq.get_transport())
                // handle_count_excluding, not peer_count: the title counts PEOPLE (unique identities), and the store carries one row per device — a 3-phone friend must read as one peer, and we must not read as one at all.
                .map(|t| {
                    t.lock()
                        .map(|s| match &own_hp {
                            Some(hp) => s.handle_count_excluding(hp),
                            None => s.handle_count(),
                        })
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            // The store only fills by gossip since the announce cutover, so on a fresh session it holds nothing but our own record and `n` is 0 even with the whole network up. The seed's identity count (off the signed ack, minus ourselves) is the floor: the store wins once gossip carries more than the seed remembers, and the max never shows a friend LESS than what either source can vouch for.
            let n = n.max(self.seed_identity_count.saturating_sub(1) as usize);
            format!("{n} {}", if n == 1 { "peer" } else { "peers" })
        } else if matches!(self.state, AppState::Settings(_)) {
            // The settings screen draws its own "Settings" heading in the header band — a chrome title would double up behind it (portrait showed "‹ Network" bleeding thru the heading).
            String::new()
        } else {
            "\u{2039} Network".to_string()
        };

        // Clamp the contacts block scroll and refresh the contacts widget layout BEFORE taking the long-lived `chrome` borrow. The whole user section (avatar, hint, search box, separator) now scrolls with the contact rows as one block, and the search box / plus button rects are positioned in `update_widget_layout` off `contacts_scroll`; doing this here (rather than inside the borrowed render block, which can't call `&mut self`) keeps the box, the avatar, and the rows all reading the SAME clamped offset within a frame — no one-frame mismatch at the over-scroll boundary. The formula matches the in-block geometry exactly: `max_scroll = (rows.y0 + matching·row_h) − buf_h`, hard-stopped at 0. Scrolled Y for the Ready-screen version watermark (rides the scroll block); `None` on other screens, where the version uses its pinned `version_cy`.
        let mut ready_block_version_y: Option<f32> = None;
        if matches!(self.state, AppState::Ready) {
            let rl = ReadyLayout::compute(buf_w, buf_h, ctx.viewport.ru);
            let row_h = rl.row_height.max(1) as isize;
            let filter: String = self
                .contacts_textbox
                .as_ref()
                .map(|t| t.chars.iter().collect::<String>().to_lowercase())
                .unwrap_or_default();
            let n_matching = self
                .contacts
                .iter()
                .filter(|c| {
                    // Must mirror the render pass's `matching` filter exactly (siblings hidden) or the two clamps disagree within a frame.
                    !c.is_sibling
                        && (filter.is_empty() || c.display_name().to_lowercase().contains(&filter))
                })
                .count();
            let block_bottom_at_zero = rl.rows.y0 as isize + n_matching as isize * row_h;
            // The version footer rides the block one row-height past the last row; extend the scroll extent past it (footer gap + a row-height of bottom margin) so the user can scroll the version fully into view instead of the bottom edge swallowing it.
            let block_end = block_bottom_at_zero + row_h * 2;
            let max_scroll = (block_end - buf_h as isize).max(0);
            // Publish the extent — no hard clamp; the wheel resists past-the-end and tick() springs the overshoot back (rubber-band).
            self.contacts_scroll_extent = max_scroll;
            self.update_widget_layout(ctx);
            // Contacts version watermark rides the scroll block: it sits just past the last contact row (one row-height of breathing room) and scrolls up with everything else, rather than being pinned to the bottom. Stash the scrolled Y for the bg-layer closure below; other screens keep the pinned `version_cy`.
            ready_block_version_y =
                Some((block_bottom_at_zero + row_h - self.contacts_scroll) as f32);
        }
        // Settings scroll: clamp the rail + content offsets to their NATURAL-height extents (no clamp-to-fit in layout → content can overflow → this scroll reveals it, bounded so it can't scroll off the page). MUST run BEFORE update_widget_layout: the wheel handler writes unclamped deltas, and positioning the widgets off the raw value for one frame (then the clamped one next frame) is what made the textboxes rubber-band past the top while the immediate-mode labels (drawn from the clamped locals) hard-stopped. One clamp, then everything this frame reads the same value. Captured into locals for use inside the borrowed render block.
        let (settings_rail_scroll, settings_content_scroll) = if let AppState::Settings(page) =
            self.state
        {
            let sl = SettingsLayout::compute(&ctx.viewport);
            // Publish the extents (rubber-band bounds) — NO hard clamp: the wheel handler resists past-the-end steps and tick() eases the overshoot back, so an out-of-range value here is the rubber-band mid-stretch, rendered as-is. Labels, widgets, and bg all read this same raw value, so the whole pane stretches together.
            self.settings_rail_extent =
                (sl.nav_row_h() * (self.settings_pages().len() as Coord + 1.0) - sl.rail_inset().h)
                    .max(0.0);
            // The You page is a dynamic form — its row count is the field set plus the fixed chrome rows, not a constant. The Diagnostics log viewer counts fractionally: two full-height header rows plus half-height record rows (matching diag_log_row_rect exactly, or the scroll bound and the drawn rows disagree).
            let content_rows_h = if page == SettingsPage::You {
                sl.content_line_h() * you_rows_plan(&self.you_fields).len() as Coord
            } else if page == SettingsPage::About {
                // Logo(4) + gap + killswitch + passless + version ≈ 7.6 rows collapsed; the reveal adds the spelled line + "dozenal" header + 6 cheat rows ≈ 8.4.
                let rows = 7.6 + if self.about_version_spelled { 8.4 } else { 0.0 };
                sl.content_line_h() * rows
            } else if page == SettingsPage::Diagnostics && self.diag_log_view {
                let n = match &self.diag_log_inspect {
                    Some((_, lines)) => lines.len(),
                    None => self.diag_log_rows.len(),
                };
                sl.content_line_h() * (2.5 + n as Coord * 0.5)
            } else {
                sl.content_line_h() * settings_page_rows(page) as Coord
            };
            self.settings_content_extent = (content_rows_h - sl.content_inset().h).max(0.0);
            // The pinned log viewer rides the newest record: scroll sits at the extent as records append, until the user scrolls up (the wheel handler un-pins).
            if page == SettingsPage::Diagnostics && self.diag_log_view && self.diag_log_follow {
                self.settings_content_scroll = self.settings_content_extent;
            }
            (self.settings_rail_scroll, self.settings_content_scroll)
        } else if let AppState::ContactPanel(cpage) = self.state {
            // The contact panel rides the SAME scroll fields + extents machinery as settings (it's the structural mirror). Rail = pinned Back + 3 page rows; content rows are fixed per page (About carries the avatar block's extra height as virtual rows).
            let sl = SettingsLayout::compute(&ctx.viewport);
            self.settings_rail_extent = (sl.nav_row_h() * (ContactPage::ALL.len() as Coord + 1.0)
                - sl.rail_inset().h)
                .max(0.0);
            let n = contact_page_rows(cpage);
            self.settings_content_extent =
                (sl.content_line_h() * n as Coord - sl.content_inset().h).max(0.0);
            (self.settings_rail_scroll, self.settings_content_scroll)
        } else {
            (0.0, 0.0)
        };
        // Settings panel: reposition the active page's widgets each frame so zoom / resize track — AFTER the clamp above (widgets and labels must read the same scroll), before the long-lived `chrome` borrow since it takes `&mut self`.
        if matches!(self.state, AppState::Settings(_)) {
            self.update_widget_layout(ctx);
        }
        // Fleet device inventory, gathered before the long-lived `chrome` borrow (the Fleet render arm can't call the `&self` helper while `chrome` is borrowed mutably). Empty off the Fleet page.
        let fleet_devices = if matches!(self.state, AppState::Settings(SettingsPage::Fleet)) {
            self.fleet_device_rows()
        } else {
            Vec::new()
        };
        // Conversation: lay out the compose textbox + send button each frame. Without this the send button kept stale placeholder geometry (mid-screen), rendered under the opaque message-list fill, and under()-blend discarded it — it never appeared. Same reason as the Ready/Settings branches above; must run before the long-lived `chrome` borrow (takes `&mut self`).
        if matches!(self.state, AppState::Conversation) {
            self.update_widget_layout(ctx);
        }

        // Capture the settings page set BEFORE the chrome mutable-borrow — it returns &'static data (reads only self.session), so the local outlives the borrow and the render loop can't re-borrow self.
        let settings_pages = self.settings_pages();
        // Same hoist for the IME inset (Android keyboard height): read it before the chrome borrow so the conversation block can use it without re-borrowing self.
        let ime_lift = self.ime_lift();
        // Same hoist for the Fleet page's locked set (treat-as-stolen rows): the row loop can't re-borrow self.
        let fleet_locked_set = self.locked_devices();
        // Hoisted for the call overlay (the chrome borrow below outlives it): SHOW the ☎ pill for any real friend conversation (discoverable, dimmed when it can't connect); ENABLE it only when the friend is online with a usable chain. Also whether a call is live (phase + peer name for the bar). All read before the chrome `as_mut` since the overlay draws early, under-blend-topmost, on every screen.
        let call_pill_show = self
            .active_contact()
            .and_then(|ci| self.contacts.get(ci))
            .map_or(false, |c| !c.is_sibling && c.friendship_id.is_some());
        let call_pill_enabled = self
            .active_contact()
            .and_then(|ci| self.contacts.get(ci))
            .map_or(false, |c| {
                !c.is_sibling && c.is_online && (c.chain_woven || c.friendship_id.is_some())
            });
        let call_overlay: Option<(crate::call::CallPhase, String, bool, Option<usize>)> =
            self.active_call.as_ref().map(|c| {
                let pi = self
                    .contacts
                    .iter()
                    .position(|k| k.handle_hash == c.peer_handle_hash);
                let peer = pi.map(|i| &self.contacts[i]);
                let name = peer.map(|k| k.display_name()).unwrap_or_else(|| "?".into());
                // LIVE direct-path check, recomputed every frame: relay-only media does not exist yet, so a call with no validated direct path may sit Active-and-silent — the bar says so, and the warning self-clears the instant a punch validates (the engine bootstraps from the peer's first authenticated packet). No stored flag to go stale.
                let direct = peer.map_or(false, |k| k.validated_path.is_some());
                (c.phase, name, direct, pi)
            });
        // Ringing / Ended show a SECOND action (Decline / Delete) beside the primary — hoisted so the end-of-frame hit re-stamp agrees with the early paint without re-deriving the phase.
        let call_two_actions = call_overlay.as_ref().map_or(false, |(p, _, _, _)| {
            matches!(
                p,
                crate::call::CallPhase::Ringing | crate::call::CallPhase::Ended
            )
        });
        // Ringing takes the WHOLE surface (redesign 2026-08-30): the bar squeezed Answer/Decline under the title band, exactly where Android's heads-up notification drops — the buttons were literally covered on every ring. Full-screen panel: caller avatar + name centred, pulse rings in the party colour, actions bottom-anchored where nothing can sit on top of them.
        let call_fullscreen = matches!(
            call_overlay.as_ref().map(|(p, _, _, _)| *p),
            Some(crate::call::CallPhase::Ringing)
        );
        // Ring-panel avatar: pre-scale the caller's avatar (or the identity gradient) to the panel diameter — done HERE (before the canvas borrows) because it needs &mut self. Cache keyed by diameter; dropped when nothing rings.
        if call_fullscreen {
            let unit_now = ReadyLayout::compute(buf_w, buf_h, ctx.viewport.ru).unit_height;
            let diameter = ((unit_now * 7.0) as usize).max(2);
            let stale = self
                .ring_avatar_scaled
                .as_ref()
                .map_or(true, |(d, _)| *d != diameter);
            if stale {
                if let Some((_, _, _, Some(pi))) = &call_overlay {
                    let c = &self.contacts[*pi];
                    let px = match c.avatar_pixels.as_ref() {
                        Some(base) => crate::ui::avatar_render::update_avatar_scaled(
                            base,
                            crate::ui::avatar::AVATAR_SIZE,
                            diameter,
                        ),
                        None => gradient_avatar_rgb(proof_gradient_seed(&c.handle_proof), diameter),
                    };
                    self.ring_avatar_scaled = Some((diameter, px));
                }
            }
        } else if self.ring_avatar_scaled.is_some() {
            self.ring_avatar_scaled = None;
        }

        let Some(chrome) = self.chrome.as_mut() else {
            return;
        };
        chrome.set_title(title_text);

        // Bg noise. `shimmer` is driven by `bg_scroll` and mixes into each row's starting colour — so the noise colour bias cycles as you scroll without changing the underlying pattern topology. `scroll_offset` is per-screen: Launch/Attest gets `0` (no vertical movement on the attest screen — shimmer only); future screens (Ready, Searching, Conversation) will pass `bg_scroll` so the noise pattern also translates with their page-scroll content. Phase 2+ branches on AppState to pick which.
        let bg_scroll = self.bg_scroll;
        let shimmer = bg_scroll as usize;
        let scroll_offset = 0; // Launch only for now.
                               // Background texture origin + per-half scroll. On Settings the noise mirror-axis sits ON the rail|content divider (1/3 width), and each half scrolls with ITS pane — rail-scroll drives the left half, content-scroll the right — so the background tracks the scroll of whatever you're reading. Every other screen keeps the centred origin with both halves locked together (unified scroll).
        let (bg_split_x, bg_left_scroll, bg_right_scroll) = if matches!(
            self.state,
            AppState::Settings(_) | AppState::ContactPanel(_)
        ) {
            let sl = SettingsLayout::compute(&ctx.viewport);
            (
                Some(sl.content.x as usize),
                // Negated to cancel the foreground gesture flip (see the `step` sign in on_event): the panes and the texture use opposite offset signs, so feeding the texture the negated offset keeps its (already-correct) direction while the foreground gets its inversion fixed.
                Some(-(settings_rail_scroll as isize)),
                -(settings_content_scroll as isize),
            )
        } else {
            (None, None, scroll_offset)
        };
        // Launch layout: faithful proportional slicing port from legacy `Layout::new` — spectrum near the top, logo wordmark overlapping its bottom, attest block (textbox + hint + button) below. Compute every frame; cheap and lets resize flow thru without a separate cache.
        let layout = LaunchLayout::compute(buf_w, buf_h, ctx.viewport.ru).lifted(ime_lift, buf_h);
        // Chromatic wave phase has two summands: * Scroll-driven base (`bg_scroll * 1/128 rad/scroll-unit`) — one wheel-notch ≈ 8 units → ~1/16 rad shift; user-tunable by changing the shift exponent.
        // * `attest_anim_phase` (advanced in `tick()` while `LaunchState::Attesting`) — the "query in flight" cue, 1 cycle/sec.
        // Summing them means the wave responds to BOTH inputs simultaneously: a user scrolling during an attestation still nudges the phase on top of the animation.
        let phase = bg_scroll as f32 * (1. / ((1 << 7) as f32)) + self.attest_anim_phase;
        let period_scale = 1.;
        let spectrum_rect = layout.spectrum;
        let logo_rect = layout.photon_text;
        // Faint dozenal version watermark, bottom-left on every screen it shows. Size = half the "handle" hint text (hint slot height × 0.7, halved); rendered at weight 400 so it resolves to the Oxanium `+glyphs` face carrying the dozenal control-block glyphs, in near-transparent white (theme::VERSION_COLOUR) so it sits in the background like a watermark rather than competing with the foreground.
        let attest_for_version = AttestBlockLayout::compute(layout.attest_block);
        let version_size =
            (attest_for_version.hint.y1 - attest_for_version.hint.y0) as f32 * 0.7 * 0.5;
        let version_glyphs = version_dozenal_glyphs();
        // Bottom-LEFT watermark; the Security/Recovery posture meters sit bottom-right on the Ready strip. Left edge one font-size in from the screen edge, mirroring the posture group's right margin.
        let version_x = version_size;
        // `draw_text_left_u32`'s y is the text BOX CENTRE, not the baseline/bottom. Anchor by the glyph bottom instead: put the text's bottom edge one `version_size` up from the window bottom (mirroring the one-`version_size` left margin), so the version reads as bottom-left-aligned from the corner rather than centre-aligned. line_height = size × 1.2 (the renderer's Metrics::relative ratio), so the centre sits half that above the bottom edge.
        let version_line_h = version_size * 1.2;
        let version_cy = buf_h as f32 - version_size - version_line_h * 0.5;
        // Zoom watermark, top-centre: current `ru` zoom factor as a decimal percentage ("100%", "103%"), twice the version size, at 1/4 opacity. Mirrors the version's bottom-centre placement (one font-size in from the edge). Integer percent — the ~3%/step zoom granularity makes decimals noise.
        let zoom_size = version_size * 2.0;
        let zoom_text = format!("{}%", (ctx.viewport.ru * 100.0).round() as i64);
        let zoom_cx = buf_w as f32 * 0.5;
        let zoom_cy = zoom_size;
        // Split-borrow `ctx.damage` (consumed by rasterize_bg's first arg) and `ctx.text` (captured by the closure for the logo's text rendering). These are disjoint fields of `Context` so the borrow checker allows both reborrows simultaneously. The closure is non-`move` so the text reborrow ends when rasterize_bg returns, leaving `ctx.text` available for `rasterize_chrome` on the next line.
        let text = &mut *ctx.text;
        // Bg-first compose chain: noise paints opaque, the wave reads it for the `sqrt(c*scale + c_bg²)` blend, then the logo (glow / body / highlight) paints over both via legacy visible-RGB ops. Each step preserves α on the pixels it touches. The wave + logo are Launch-screen chrome — once attested the user shouldn't be staring at the wordmark every time they open the app, so Ready / Searching / Conversation get just the background noise and let their own widgets own the canvas.
        let on_launch = matches!(self.state, AppState::Launch(_));
        // Faint dozenal version watermark shows on the ATTEST screen ONLY (Launch) — a quiet bottom-left mark while you sign in. Ready / Conversation stay clean; the About page carries the version in full (normal-white dozenal glyphs, tap to spell out). Never arabic anywhere.
        let show_version = on_launch;
        // Swap the noise base colour to (*theme::BG_BASE_WARNING) when the dual-ring vault flagged degraded this session — the noise pass already runs every frame so this changes a colour, not the pass count. None on the happy path keeps fluor's default green-dark BG_BASE.
        let bg_base = if self.vault_degraded {
            Some(*theme::BG_BASE_WARNING)
        } else {
            None
        };
        // The 1-px noise inset exists ONLY to clear the window perimeter hairline / shadow band — so gate it on whether that perimeter is actually drawn, which is exactly `!chrome.full_edge`. A windowed desktop draws the perimeter → inset. A maximized/fullscreen desktop goes full_edge (no perimeter) and Android forces full_edge too → paint to the screen edge, else a 1-px unpainted border shows. (Earlier this was hardcoded per-OS, so desktop-maximized still inset for a perimeter that wasn't there.) `|| cfg!(android)` keeps the Android always-fullscreen guarantee even on a transient pre-resize frame where full_edge hasn't synced yet.
        let bg_fullscreen = chrome.full_edge || cfg!(target_os = "android");
        // Stage marks for the >1s breakdown at the end of the frame — the flat "render took Nms" line named the SCREEN but not the STAGE, which stalled the 2026-08-21 hang hunt (5.8-8.8s Conversation renders, no idea where inside).
        let mark_pre = std::time::Instant::now();
        chrome.rasterize_bg(ctx.damage, |canvas| {
            // Chromatic wave FIRST, then the background noise — that is the paint order for the spectrum band.
            if on_launch {
                chromatic_wave(canvas, spectrum_rect, phase, period_scale);
                paint_photon_logo(canvas, text, logo_rect);
            }
            if show_version {
                // On the Ready screen the version rides the scroll block (positioned past the last contact row); elsewhere it stays pinned at `version_cy`.
                let vy = ready_block_version_y.unwrap_or(version_cy);
                text.draw_text_left(
                    canvas,
                    &version_glyphs,
                    version_x,
                    vy,
                    &TextStyle::new(version_size, theme::VERSION_COLOUR).font("Oxanium"),
                    None,
                    None,
                );
            }
            // Zoom hint is independent of the version's screen gate — it shows on ANY screen, but only while actively zooming (a held zoom modifier after a `ru` change), per `show_zoom`.
            if show_zoom {
                text.draw_text_center(
                    canvas,
                    &zoom_text,
                    zoom_cx,
                    zoom_cy,
                    &TextStyle::new(zoom_size, theme::ZOOM_COLOUR).font("Oxanium"),
                    None,
                    None,
                );
            }
            paint::background_noise_split(
                canvas,
                shimmer,
                bg_fullscreen,
                bg_right_scroll,
                bg_split_x,
                bg_left_scroll,
                None,
                bg_base,
            );
            // Wave then logo — RMW ops that read the now-opaque noise beneath as their base. The chromatic wave quadrature-blends with the bg colour (sqrt-linear-light) so it MUST follow the noise; the logo composites over the wave/noise. (Watermarks above went before the noise so it composes under them.)
            if on_launch {
                chromatic_wave(canvas, spectrum_rect, phase, period_scale);
                paint_photon_logo(canvas, text, logo_rect);
            }
        });
        // Window-perimeter hairline FIRST — painted straight into `target` (not the chrome group) and carves the window-shape clip_mask. fluor is under-blend only, so whatever lands in `target` first wins at shared edge pixels; drawing the hairline before any content makes it survive over full-bleed screens (Ready/Conversation) whose content reaches the window edge. The chrome group (buttons / orb / strip / title) still composites UNDER content via `flatten_into` below. The clip_mask carve here is the SOLE source of the single window-shape alpha-trim done at the OS boundary in finalize.
        chrome.rasterize_perimeter(target, buf_w, buf_h, ctx.clip_mask);
        // Orb press glow lives IN the chrome layer now (drawn after the orb+ring, so under() blooms it beneath them): feed the pressed state each frame; the setter no-ops when unchanged and re-rasters chrome on the press/release edges.
        chrome.set_orb_pressed(
            ctx.pressed_hit != HIT_NONE && ctx.pressed_hit == chrome.app_icon_btn.id(),
        );
        chrome.rasterize_chrome(ctx.damage, ctx.text, ctx.clip_mask);
        let mark_chrome = std::time::Instant::now();

        // Chord hint — painted INTO `target` BEFORE `flatten_into` so the hint glyphs sit at the TOP of the under-blend chain (chrome composes UNDER them).
        if held_now {
            let span = ctx.viewport.effective_span();
            let mut canvas = Canvas::new(target, buf_w, buf_h, ctx.damage);
            paint::draw_chord_hint(&mut canvas, ctx.text, CHORD_HINTS, span);
        }

        // CALL OVERLAY (docs/calls.md) — retained fluor Buttons (no hand-rolled pills), painted HERE, EARLY, so under-blend keeps them above every screen's body (the whole point: a ring must be visible + answerable from wherever the user is). A live call shows the status chip + action bar; an open callable conversation with no call shows the ☎ start pill. Pixels land now (hit_map = None); the hit rects are RE-STAMPED at the very end via `stamp_hit_into` because each screen re-stamps its own hit_test_map region and would otherwise wipe this. Hover/press/dispatch ride `visit_app_widgets`. y sits just below the chrome title-bar band.
        {
            let mut canvas = Canvas::new(target, buf_w, buf_h, ctx.damage);
            // The ONE zoom-aware line unit the rest of the UI sizes off (back arrow, contact rows, avatar) — harmonic-mean of span·ru and the height budget, so it tracks Ctrl+/− and pinch; every dimension below is a multiple of it: NO fixed pixels, NO clamps (AGENT.md).
            let unit = ReadyLayout::compute(buf_w, buf_h, ctx.viewport.ru).unit_height;
            let y0 = unit; // top margin, one line down — scales with the rest of the top bar
            let pill_h = unit * 2.; // a comfortable tap target, two lines tall
            let cy = y0 + pill_h * 0.5; // Buttons take a CENTRE; the row is one pill tall
            let call_font = unit * 0.55; // button-text scale, proportional to the pill so it tracks zoom
            if call_fullscreen {
                // ── FULL-SCREEN RING PANEL ── painted over whatever screen was up; every element scales off `unit` (zoom-honest, no fixed pixels).
                let (name, direct, pi) = match &call_overlay {
                    Some((_, n, d, p)) => (n.clone(), *d, *p),
                    None => (String::from("?"), false, None),
                };
                let w = buf_w as f32;
                let h = buf_h as f32;
                // OPAQUE background: α+darkness (α 0xFF, darkness 0xFF ⇒ solid black) — the screen underneath must NOT show through (field 2026-08-31: the translucent wash left the contact list ghosting behind the ring panel). The panel is background + its own elements, nothing else.
                paint::fill_rect(
                    &mut canvas,
                    0,
                    0,
                    buf_w as isize,
                    buf_h as isize,
                    0xFFFFFFFF,
                    None,
                    None,
                );
                let colour = pi
                    .and_then(|i| {
                        let c = &self.contacts[i];
                        self.session.as_ref().map(|s| {
                            party_colour(&relationship_digest(
                                &c.handle_hash,
                                &crate::crypto::clutch::identity_party_id(&s.identity_seed),
                            ))
                        })
                    })
                    .unwrap_or(*theme::STATUS_TEXT_COLOUR);
                let (acx, acy) = (w * 0.5, h * 0.36);
                let avatar_r = unit * 3.5;
                // Pulse rings — the visual ring, expanding from the avatar in the relationship colour. Phase is a pure function of now (~1.4s cycle, matching the chirp cadence's feel); the tick keeps frames coming while Ringing (wake_at), so this animates without any stored state. Three staggered translucent discs, oldest largest and popping.
                let cycle = (vsf::eagle_time_oscillations() as f64
                    / (1.4 * vsf::OSCILLATIONS_PER_SECOND as f64))
                    .fract() as f32;
                for k in 0..3 {
                    let t = (cycle + k as f32 / 3.0).fract();
                    let r = avatar_r * (1.05 + 1.15 * t);
                    // Fade with expansion: α walks 0x38 → 0 as the disc grows.
                    let a = ((1.0 - t) * 0x38 as f32) as u32;
                    paint::draw_circle(
                        &mut canvas,
                        acx,
                        acy,
                        r,
                        (a << 24) | (colour & 0x00FF_FFFF),
                        None,
                    );
                }
                if let Some((diam, px)) = self.ring_avatar_scaled.as_ref() {
                    crate::ui::avatar_render::draw_avatar(
                        &mut canvas, acx, acy, avatar_r, px, *diam, None,
                    );
                }
                // Name in the relationship colour, large; the phase line beneath in the status grey.
                ctx.text.draw_text_center(
                    &mut canvas,
                    &name,
                    acx,
                    acy + avatar_r + unit * 1.1,
                    &TextStyle::new(unit * 1.05, colour),
                    None,
                    None,
                );
                let status_line = if direct {
                    "\u{260E} incoming call".to_string()
                } else {
                    "\u{260E} incoming call \u{2014} \u{26A0} no direct path".to_string()
                };
                ctx.text.draw_text_center(
                    &mut canvas,
                    &status_line,
                    acx,
                    acy + avatar_r + unit * 2.2,
                    &TextStyle::new(unit * 0.6, *theme::STATUS_TEXT_COLOUR),
                    None,
                    None,
                );
                // Actions: bottom third, thumb-reach, decline LEFT answer RIGHT with a generous gap — and bottom-anchored so an Android heads-up banner (which owns the top) can never cover them.
                let bw = w * 0.34;
                let bh = unit * 2.4;
                let by = h - bh * 0.5 - unit * 1.5;
                let bfont = unit * 0.75;
                if let Some(b) = self.call_decline_btn.as_mut() {
                    b.set_rect(w * 0.5 - bw * 0.5 - unit * 0.75, by, bw, bh);
                    b.set_font_size(bfont);
                    b.set_label("Decline");
                    let id = b.hit_id();
                    b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                }
                if let Some(b) = self.call_action_btn.as_mut() {
                    b.set_rect(w * 0.5 + bw * 0.5 + unit * 0.75, by, bw, bh);
                    b.set_font_size(bfont);
                    b.set_label("Answer");
                    let id = b.hit_id();
                    b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                }
            } else if let Some((phase, name, direct, _pi)) = &call_overlay {
                let phase = *phase;
                let bar_w = buf_w as f32 * 0.9; // window-relative width — a bar spans the window
                let x0 = (buf_w as f32 - bar_w) * 0.5;
                let gap = unit * 0.5;
                let mut status = match phase {
                    crate::call::CallPhase::Outgoing => {
                        format!("\u{260E} calling {}\u{2026}", name)
                    }
                    crate::call::CallPhase::Ringing => format!("\u{260E} {} calling", name),
                    crate::call::CallPhase::Active => format!("\u{260E} in call \u{2014} {}", name),
                    crate::call::CallPhase::Ended => "\u{260E} keep this recording?".to_string(),
                };
                // No validated direct path in a live phase → say so on the bar (media may be silent until a punch lands; the warning disappears live when it does). The ⚠ is safe everywhere: fonts are fully bundled + deterministic (fluor's explicit-db TextRenderer, zero system-font pulls — verified 2026-08-20), and Noto Sans Symbols 2 covers U+26A0 in the same 2600 block as the field-proven ☎.
                if !direct && !matches!(phase, crate::call::CallPhase::Ended) {
                    status.push_str(" \u{26A0} no direct path");
                }
                let status_w = if call_two_actions {
                    bar_w * 0.44
                } else {
                    bar_w * 0.62
                };
                let action_w = if call_two_actions {
                    (bar_w - status_w - gap * 2.) * 0.5
                } else {
                    bar_w - status_w - gap
                };
                // Status chip — a non-interactive label (full brightness, not in the widget walk, never stamped).
                if let Some(b) = self.call_status_btn.as_mut() {
                    b.set_rect(x0 + status_w * 0.5, cy, status_w, pill_h);
                    b.set_font_size(call_font);
                    b.set_label(status);
                    b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, HIT_NONE);
                }
                let ax = x0 + status_w + gap;
                let a_label = match phase {
                    crate::call::CallPhase::Ringing => "Answer",
                    crate::call::CallPhase::Ended => "Keep",
                    _ => "Hang up",
                };
                if let Some(b) = self.call_action_btn.as_mut() {
                    b.set_rect(ax + action_w * 0.5, cy, action_w, pill_h);
                    b.set_font_size(call_font);
                    b.set_label(a_label);
                    let id = b.hit_id();
                    b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                }
                if call_two_actions {
                    let dx = x0 + status_w + gap * 2. + action_w;
                    let d_label = if phase == crate::call::CallPhase::Ended {
                        "Delete"
                    } else {
                        "Decline"
                    };
                    if let Some(b) = self.call_decline_btn.as_mut() {
                        b.set_rect(dx + action_w * 0.5, cy, action_w, pill_h);
                        b.set_font_size(call_font);
                        b.set_label(d_label);
                        let id = b.hit_id();
                        b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                    }
                }
            } else if matches!(self.state, AppState::Conversation) && call_pill_show {
                // The ☎ start pill — top-right of the conversation, mirroring the "‹ Contacts" back arrow on the left; sized off `unit` so it matches the back arrow at every zoom. Shown for any friend convo (discoverable), dimmed (disabled) until the friend is reachable — the disabled label dim reads as "can't call yet".
                let pill_w = unit * 5.;
                let px = buf_w as f32 - pill_w - unit; // top-right, one unit of margin from the edge
                if let Some(b) = self.call_start_btn.as_mut() {
                    b.set_rect(px + pill_w * 0.5, cy, pill_w, pill_h);
                    b.set_font_size(call_font);
                    b.set_enabled(call_pill_enabled);
                    let id = b.hit_id();
                    b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                }
            }
        }

        // DISTINCT SCREEN while ringing (2026-08-31): the full-screen ring panel is opaque and modal, so painting the underlying screen is pure waste — at full frame rate (the pulse animation), during the one moment the radio + speaker are also busy. Every per-screen body below is skipped; the panel, chrome flatten, joiner flood and the endgame hit re-stamp (which wipes the map modal anyway) still run. Screen-side frame bookkeeping (scroll extents, textbox geometry, hit stamps) goes stale for the ring's duration by design — the drain repaints fully on every ring edge, so the first non-ringing frame rebuilds it all.
        if !call_fullscreen {

        // Launch-screen widgets paint UNDER the chord hint (so the hint always wins over the textbox) and OVER chrome (so the pill sits on top of the spectrum strip / wordmark). Same target buffer as the chord hint; widgets stamp their hit IDs into chrome's shared `hit_test_map`. Only paint when the launch screen is the active state — Ready/Searching/Conversation get their own widgets later.
        if let AppState::Launch(launch_state) = &self.state {
            let layout =
                LaunchLayout::compute(buf_w, buf_h, ctx.viewport.ru).lifted(ime_lift, buf_h);
            let attest = AttestBlockLayout::compute(layout.attest_block);
            let mut canvas = Canvas::new(target, buf_w, buf_h, ctx.damage);

            // ONE IDENTITY PER DEVICE (docs/lifecycle.md D2): if the binding marker exists this device already carries an identity, so the launch screen must offer only RESUME (type the bound handle) + WIPE.
            // Awareness lives at RENDER time here, mirroring the submit-time refusal at attest(): a bound device must never render fresh-attest-as-anyone / join-another-fleet / pairing-words UI, only get bounced after interacting.
            // Presence-only — no handle comparison happens here, so no oracle: the marker's existence gates the UI, its VALUE only ever meets a post-proof compare (2026-08-23 ticket).
            let device_bound = crate::storage::device_binding::binding().is_some();

            // Clear the attest block's footprint in the shared hit_test_map BEFORE re-stamping this frame's widgets. Chrome only wipes the map on its own dirty cycles (`rasterize_chrome` early-returns when chrome is clean), but the launch widgets re-stamp every frame — so when the Attest button stops rendering (handle cleared to empty) on a chrome-clean frame, its old hit-rect would otherwise linger and keep dispatching pointer + hitmask. The attest_block is the only Photon-owned region of the map on this screen, so clearing the whole block each frame is the cheap correct reset; the textbox/button/∞ below re-stamp whatever is actually present.
            restamp_hit_rect(
                &mut chrome.hit_test_map,
                buf_w,
                buf_h,
                layout.attest_block.x0 as isize,
                layout.attest_block.y0 as isize,
                layout.attest_block.x1 as isize,
                layout.attest_block.y1 as isize,
                HIT_NONE,
            );

            // Status slot — `attest.error` rect above the textbox. Carries either the red error message (`LaunchState::Error`) or the white "Attesting…" indicator (`LaunchState::Attesting`); empty in Fresh. Same geometry for both so they swap in place; colour differentiates "something's wrong" from "we're working". Wave's 1-cycle/sec phase animation pairs with the "Attesting…" line as the secondary cue.
            let status: Option<(&str, u32)> = if self.launch_add_mode
                && !self.add_join_status.is_empty()
            {
                Some((self.add_join_status.as_str(), (*theme::STATUS_TEXT_COLOUR)))
            } else {
                match launch_state {
                        LaunchState::Attesting => {
                            Some(("Attesting\u{2026}", (*theme::STATUS_TEXT_COLOUR)))
                        }
                        LaunchState::Error(msg) if !msg.is_empty() => {
                            Some((msg.as_str(), (*theme::ERROR_TEXT_COLOUR)))
                        }
                        // Terminal brick: the fleet locked this device. Red, dead-end — no handle re-type helps (the identity is real, the fleet owner marked the hardware stolen), only an unlock from another of the owner's devices clears it.
                        LaunchState::Locked => Some((
                            "this device has been locked by your fleet \u{2014} unlock it from another of your devices to use it again",
                            (*theme::ERROR_TEXT_COLOUR),
                        )),
                        // Up-front hint: a bound device in Fresh gets the resume-or-wipe line in the STATUS colour (not error-red) so the restriction is visible before any submit.
                        // Confirm/KnownHandle fall thru to None and keep their own bands.
                        LaunchState::Fresh if device_bound => Some((
                            "this device carries an identity \u{2014} type its handle to resume, or wipe (Settings \u{2192} Security)",
                            (*theme::STATUS_TEXT_COLOUR),
                        )),
                        _ => None,
                    }
            };
            if let Some((text, colour)) = status {
                let error_rect = attest.error;
                if !error_rect.is_empty() {
                    let region_h = (error_rect.y1 - error_rect.y0) as f32;
                    let cx = (error_rect.x0 + error_rect.x1) as f32 * 0.5;
                    let cy = (error_rect.y0 + error_rect.y1) as f32 * 0.5;
                    // Half-height font: status messages are short by convention; full-rect-height is too loud for one-line text and overflows wide messages off the side.
                    ctx.text.draw_text_center(
                        &mut canvas,
                        text,
                        cx,
                        cy,
                        &TextStyle::new(
                            region_h * 0.5, // Medium weight — readable at small sizes; matches the Oxanium family already loaded in init().
                            colour,
                        )
                        .weight(500)
                        .font("Oxanium"),
                        None,
                        None,
                    );
                }
            }

            // Permanence warning block (`LaunchState::Confirm`) — drawn in the empty 6-unit band BELOW the attest button, sized with the same ru-scaled math as the join-words rows. The headline takes the error colour for gravity; the detail lines stay in status grey. The button above now reads "Yes — forever"; editing the handle cancels back to Fresh.
            if matches!(launch_state, LaunchState::Confirm) && !self.launch_add_mode {
                let tb_h = (attest.textbox.y1 - attest.textbox.y0) as f32;
                let line_h = (tb_h * 0.45).min(buf_w as f32 / 22.0);
                let cx = buf_w as f32 * 0.5;
                let mut y = attest.attest.y1 as f32 + line_h * 1.6;
                // What's permanent is the IDENTITY, not the handle: a handle is a mutable label, but attesting mints crypto roots with no password / reset / recovery. Ownership binds to the HUMAN, not the hardware — the first person to attest owns that identity, while devices stay replaceable thru the fleet chain (remove the first device whenever, as long as another is added first). The warning must not mis-teach "this phone owns it" NOR "this name is a life sentence" — it's the identity behind it that can't be undone.
                let lines: [(&str, u32); 5] = [
                    (
                        "This mints a permanent identity.",
                        (*theme::ERROR_TEXT_COLOUR),
                    ),
                    (
                        "No password. No reset. No recovery.",
                        (*theme::STATUS_TEXT_COLOUR),
                    ),
                    (
                        "The first human to attest owns it.",
                        (*theme::STATUS_TEXT_COLOUR),
                    ),
                    (
                        "Devices can be replaced. The identity can't.",
                        (*theme::STATUS_TEXT_COLOUR),
                    ),
                    ("Press again if you mean it.", (*theme::STATUS_TEXT_COLOUR)),
                ];
                for (line, colour) in lines {
                    ctx.text.draw_text_center(
                        &mut canvas,
                        line,
                        cx,
                        y,
                        &TextStyle::new(line_h, colour).weight(600).font("Oxanium"),
                        None,
                        None,
                    );
                    y += line_h * 1.35;
                }
            }

            // KnownHandle fork (docs/lifecycle.md D1) — the claimed-name screen, drawn in the same band as the permanence block. Both readings, taken-first (the more common visitor is the collider), then the two pills. Nothing has touched the network yet.
            // Suppressed when device_bound: a bound device can't join another fleet, so the "It's mine — show pairing words" pill and the whole claimed-name fork have no meaning here.
            if matches!(launch_state, LaunchState::KnownHandle)
                && !self.launch_add_mode
                && !device_bound
            {
                let tb_h = (attest.textbox.y1 - attest.textbox.y0) as f32;
                let line_h = (tb_h * 0.45).min(buf_w as f32 / 22.0);
                let cx = buf_w as f32 * 0.5;
                let mut y = attest.attest.y1 as f32 + line_h * 1.6;
                let lines: [(&str, u32); 3] = [
                    ("This name is already claimed.", (*theme::ERROR_TEXT_COLOUR)),
                    (
                        "New here? Someone else owns it \u{2014} pick another.",
                        (*theme::STATUS_TEXT_COLOUR),
                    ),
                    (
                        "Yours? Approve this device from one you're signed in on.",
                        (*theme::STATUS_TEXT_COLOUR),
                    ),
                ];
                for (line, colour) in lines {
                    ctx.text.draw_text_center(
                        &mut canvas,
                        line,
                        cx,
                        y,
                        &TextStyle::new(line_h, colour).weight(600).font("Oxanium"),
                        None,
                        None,
                    );
                    y += line_h * 1.35;
                }
                y += line_h * 0.5;
                let px = attest.textbox.x0 as f32;
                let pw = (attest.textbox.x1 - attest.textbox.x0) as f32;
                let pick = fluor::region::Region::new(px, y, pw, tb_h);
                y += tb_h * 1.3;
                let mine = fluor::region::Region::new(px, y, pw, tb_h);
                draw_stub_pill(
                    &mut canvas,
                    ctx.text,
                    &mut chrome.hit_test_map,
                    buf_w,
                    buf_h,
                    pick,
                    "Pick another name",
                    self.known_pick_hit,
                    ctx.pressed_hit,
                );
                draw_stub_pill(
                    &mut canvas,
                    ctx.text,
                    &mut chrome.hit_test_map,
                    buf_w,
                    buf_h,
                    mine,
                    "It's mine \u{2014} show pairing words",
                    self.known_mine_hit,
                    ctx.pressed_hit,
                );
            }

            // Join words phase (new device): the screen becomes display-only — this device's pairing words, drawn in rows for reading onto the other device, flipping to the found-colour when a member matches them. No textbox, no attest button.
            // Suppressed when device_bound: a bound device must never display its pairing words (it can't be paired into another fleet).
            let join_words_up = self.launch_add_mode && self.add_join_words.is_some();
            if join_words_up && !device_bound {
                if let Some(words) = self.add_join_words.as_ref() {
                    let tokens: Vec<String> = {
                        let mut v = Vec::new();
                        let mut cur = String::new();
                        for c in words.chars() {
                            if c.is_ascii_uppercase() && !cur.is_empty() {
                                v.push(std::mem::take(&mut cur));
                            }
                            cur.push(c);
                        }
                        if !cur.is_empty() {
                            v.push(cur);
                        }
                        v
                    };
                    // No intermediate ready-flip: red-until-green — the words stay neutral until membership folds, at which point this screen is LEFT (that departure is the green the far side confirms).
                    let colour = *theme::STATUS_TEXT_COLOUR;
                    let cx = buf_w as f32 * 0.5;
                    // Size + anchor from the attest-block layout so the words scale with ru/zoom like every other widget and sit BELOW the status slot instead of floating into the wordmark. Width-capped so 4-word lines fit a narrow window.
                    let tb_h = (attest.textbox.y1 - attest.textbox.y0) as f32;
                    let line_h = (tb_h * 0.45).min(buf_w as f32 / 18.0);
                    let lines: Vec<String> = tokens.chunks(4).map(|c| c.join(" ")).collect();
                    let mut y = attest.error.y1 as f32 + line_h * 1.2;
                    for line in &lines {
                        ctx.text.draw_text_center(
                            &mut canvas,
                            line,
                            cx,
                            y,
                            &TextStyle::new(line_h, colour).weight(600).font("Oxanium"),
                            None,
                            None,
                        );
                        y += line_h * 1.35;
                    }
                    // Name the device being enrolled, so a user pairing several devices can tell on both screens which one these words belong to. Deterministic two-word default from the device PUBLIC key + the fleet's identity seed, so the Fleet list on every device in this fleet shows this same name; the owner-edited override arrives with the devices page. Pre-attest the session isn't set yet, so derive the seed from the handle being joined (`add_join_handle`).
                    let join_seed = self.session.as_ref().map(|s| s.identity_seed).or_else(|| {
                        self.add_join_handle
                            .as_ref()
                            .map(|h| crate::storage::contacts::derive_identity_seed(h))
                    });
                    if let (Some(kp), Some(seed)) = (self.device_keypair.as_ref(), join_seed) {
                        let name = crate::network::fgtw::fleet::device_name_default(
                            kp.public.as_bytes(),
                            &seed,
                        );
                        y += line_h * 0.4;
                        ctx.text.draw_text_center(
                            &mut canvas,
                            &format!("this device: {name}"),
                            cx,
                            y,
                            &TextStyle::new(line_h * 0.8, fluor::theme::HINT_COLOUR)
                                .weight(500)
                                .font("Oxanium"),
                            None,
                            None,
                        );
                    }
                    // "Copy words" tappable: puts the space-separated words on the clipboard so they can ride email/messenger to the sponsor device instead of being read + retyped. Label flips on interaction, never on a timer.
                    {
                        y += line_h * 0.9;
                        let csize = line_h * 0.7;
                        let (clabel, ccolour) = if self.join_words_copied {
                            (
                                "copied \u{2014} paste them on your other device",
                                *theme::STATUS_TEXT_COLOUR,
                            )
                        } else {
                            ("copy words", *theme::CONTACT_NAME_COLOUR)
                        };
                        ctx.text.draw_text_center(
                            &mut canvas,
                            clabel,
                            cx,
                            y,
                            &TextStyle::new(csize, ccolour).weight(600).font("Oxanium"),
                            None,
                            None,
                        );
                        let half_w = buf_w as f32 * 0.4;
                        restamp_hit_rect(
                            &mut chrome.hit_test_map,
                            buf_w,
                            buf_h,
                            (cx - half_w) as isize,
                            (y - csize * 0.8) as isize,
                            (cx + half_w) as isize,
                            (y + csize * 0.8) as isize,
                            self.join_copywords_hit_id,
                        );
                        y += csize * 0.9;
                    }
                    // How-to guidance: the two ways the OTHER (already-in-fleet) device adds this one, plus the confirm. Small + dim so it reads as instructions, not chrome.
                    {
                        y += line_h * 0.9;
                        let gsize = line_h * 0.62;
                        for line in [
                            "On your other device: Settings \u{2192} Fleet \u{2192} Add",
                            "Type or paste these words there \u{2014} or, if it's nearby,",
                            "just tap this device in the list.",
                            "You'll confirm the add on that device.",
                        ] {
                            ctx.text.draw_text_center(
                                &mut canvas,
                                line,
                                cx,
                                y,
                                &TextStyle::new(gsize, fluor::theme::HINT_COLOUR).font("Oxanium"),
                                None,
                                None,
                            );
                            y += gsize * 1.5;
                        }
                    }
                    // "Start fresh (wipe this device)" — the secondary escape: a device that was REMOVED from a fleet can't attest (can't reach the Security page), so this is its only self-clean path. Two-tap confirm. Hit-stamped so a tap on Android works (no chords there). Pushed well below the add guidance so it reads as the edge case, not the main action.
                    {
                        y += line_h * 1.4;
                        let sf_label = if self.join_startfresh_armed {
                            "Start fresh — tap again to wipe this device"
                        } else {
                            "Wrong device? Start fresh (wipe this device)"
                        };
                        let sf_size = line_h * 0.7;
                        let sf_colour = if self.join_startfresh_armed {
                            *theme::ERROR_TEXT_COLOUR
                        } else {
                            fluor::theme::HINT_COLOUR
                        };
                        ctx.text.draw_text_center(
                            &mut canvas,
                            sf_label,
                            cx,
                            y,
                            &TextStyle::new(sf_size, sf_colour)
                                .weight(500)
                                .font("Oxanium"),
                            None,
                            None,
                        );
                        let half_w = buf_w as f32 * 0.4;
                        restamp_hit_rect(
                            &mut chrome.hit_test_map,
                            buf_w,
                            buf_h,
                            (cx - half_w) as isize,
                            (y - sf_size * 0.8) as isize,
                            (cx + half_w) as isize,
                            (y + sf_size * 0.8) as isize,
                            self.join_startfresh_hit_id,
                        );
                    }
                }
            } else {
                // Hint slot — static "handle" label below the textbox. Tells the user what to type.
                let hint_rect = attest.hint;
                if !hint_rect.is_empty() {
                    let region_h = (hint_rect.y1 - hint_rect.y0) as f32;
                    let cx = (hint_rect.x0 + hint_rect.x1) as f32 * 0.5;
                    let cy = (hint_rect.y0 + hint_rect.y1) as f32 * 0.5;
                    let hint_label = if self.launch_add_mode {
                        "handle (join a fleet)"
                    } else {
                        "handle"
                    };
                    ctx.text.draw_text_center(
                        &mut canvas,
                        hint_label,
                        cx,
                        cy,
                        &TextStyle::new(region_h * 0.7, fluor::theme::HINT_COLOUR)
                            .weight(500)
                            .font("Oxanium"),
                        None,
                        None,
                    );
                }

                // Resting-state gates for the attest slot. The handle textbox owns the empty/focused truth; the attest button and the infinity glyph are the two mutually-exclusive things that can occupy the slot below it.
                // - handle_entered: any typed character → show the Attest button (mirrors the contacts plus-button's `!chars.is_empty()` reveal).
                // - textbox_active: the textbox is focused (cursor in it) → the user is mid-entry even with no character yet, so the resting infinity steps aside.
                let handle_entered = self
                    .textbox
                    .as_ref()
                    .map(|tb| !tb.chars.is_empty())
                    .unwrap_or(false);
                let textbox_active = self
                    .textbox
                    .as_ref()
                    .map(|tb| Some(tb.hit_id()) == self.focused)
                    .unwrap_or(false);
                // Locked is a terminal brick: suppress the ∞ placeholder AND the handle field entirely, so the screen is just the red "locked by your fleet" line with nothing that invites input (the Attest button is already gated off by handle_entered, which is false on the cleared field).
                let launch_locked = matches!(launch_state, LaunchState::Locked);

                // Dormant infinity centred IN the handle textbox — it sits where the typed handle will appear, a half-brightness grey placeholder for the resting field, shown only while the field is empty AND unfocused. Painted BEFORE the textbox: fluor's under-blend is "topmost paints first; later opaque dst wins", so the glyph must precede the textbox's empty-pill fill to survive (same ordering the contacts plus-button uses). The instant the user focuses (cursor in) or a character lands, the gate goes false and the textbox owns the slot alone. Anchor and size come straight off the textbox (`center_x/center_y/font_size`), so the glyph lands pixel-identical to where a typed character would — the textbox draws its own glyphs via `draw_text_center_u32` at the same anchor, so matching it here keeps the ∞ from sitting high or scaling differently.
                if !handle_entered && !textbox_active && !launch_locked {
                    if let Some(tb) = self.textbox.as_ref() {
                        // ∞ ink sits ~1-2px high because `draw_text_center_u32` centres on the line box (ascent+descent), and a math symbol's ink rides the math axis, slightly above where baseline-seated text reads as centred. Nudge the y anchor down by font_size/32 (≈1-2px here, scales with zoom) to seat the glyph at the pill's visual centre.
                        let baseline_nudge = tb.font_size * (1.0 / (1 << 5) as f32);
                        ctx.text.draw_text_center(
                            &mut canvas,
                            "\u{221E}",
                            tb.center_x,
                            tb.center_y + baseline_nudge,
                            &TextStyle::new(
                                tb.font_size, // Same weight the textbox renders its own glyphs at (see textbox `measure_text_widths_per_char` / draw calls).
                                fluor::theme::HINT_COLOUR,
                            )
                            .font("Oxanium"),
                            None,
                            None,
                        );
                    }
                }

                if !launch_locked {
                    if let Some(tb) = self.textbox.as_mut() {
                        let id = tb.hit_id();
                        tb.render_content_into(
                            &mut canvas,
                            0.,
                            0.,
                            ctx.text,
                            None,
                            None,
                            Some(&mut chrome.hit_test_map),
                            id,
                        );
                    }
                } else {
                    // The dead-end's ONE interaction: the user claims a sibling has unlocked this device. The pill returns to the normal resume entry (locked_retry_hit handler) and deliberately does NOT open a handle field here — a locked device never prompts for the root secret.
                    let r = attest.attest;
                    let pill = fluor::region::Region::new(
                        r.x0 as f32,
                        r.y0 as f32,
                        (r.x1 - r.x0) as f32,
                        (r.y1 - r.y0) as f32,
                    )
                    .center_h(0.85);
                    draw_stub_pill_filled(
                        &mut canvas,
                        ctx.text,
                        &mut chrome.hit_test_map,
                        buf_w,
                        buf_h,
                        pill,
                        "Unlocked from another device? Tap to retry",
                        self.locked_retry_hit,
                        ctx.pressed_hit,
                        true,
                        None,
                        "Open Sans",
                    );
                }
                // The Attest button only exists once there's a handle to attest. An empty, untouched field shows the dormant infinity in its place instead; a focused-but-empty field shows neither (the user is typing). Hiding the button also keeps its hit-rect out of `hit_test_map`, so an empty field can't dispatch a no-op attest click.
                if handle_entered {
                    if let Some(btn) = self.attest_btn.as_mut() {
                        let id = btn.hit_id();
                        btn.render_content_into(
                            &mut canvas,
                            0.,
                            0.,
                            ctx.text,
                            None,
                            Some(&mut chrome.hit_test_map),
                            id,
                        );
                    }
                }
            }
        }

        // Ready screen — slice-based layout matching legacy ContactsUnifiedLayout. Today only the avatar circle is painted; the layout already carries rects for handle / hint / textbox / separator / contact rows so subsequent slices drop into named slots without re-computing geometry.
        if matches!(self.state, AppState::Ready) {
            let mut canvas = Canvas::new(target, buf_w, buf_h, ctx.damage);
            let ready_layout = ReadyLayout::compute(buf_w, buf_h, ctx.viewport.ru);

            // The whole user section (avatar, hint, search box + plus, separator) scrolls together with the contact rows as one block; `contacts_scroll` is the single block offset (0 = rest, avatar at its natural top). Subtract it from the Y of every scrolling element. The version watermark, Sec/Rec meters, and background do NOT scroll (rendered elsewhere / left unoffset here). The upper clamp lands below once `matching`/`rows` are known.
            let scroll = self.contacts_scroll as f32;

            // Clear the contacts textbox slot in the shared hit_test_map before re-stamping. Same reason as the launch screen: chrome only wipes the map on its own dirty cycles, but the textbox + overlaid plus-button re-stamp every frame, and the plus only renders when the field is non-empty. Without this, clearing the search field to empty on a chrome-clean frame would leave the plus-button's old hit-rect dispatching pointer + hitmask. The plus lives inside the textbox slot, so clearing that slot covers both. The slot scrolls with the block, so clear the SCROLLED rect (update_widget_layout offsets the textbox/button rects by the same `contacts_scroll`).
            restamp_hit_rect(
                &mut chrome.hit_test_map,
                buf_w,
                buf_h,
                ready_layout.textbox.x0 as isize,
                ready_layout.textbox.y0 as isize - self.contacts_scroll,
                ready_layout.textbox.x1 as isize,
                ready_layout.textbox.y1 as isize - self.contacts_scroll,
                HIT_NONE,
            );

            let (cx, cy_natural, radius) = ready_layout.avatar_center_radius();
            let cy = cy_natural - scroll;
            // 0xFFC5C5C5 in fluor's α+darkness format = α 0xFF, darkness 0xC5 each channel = visible RGB(0x3A, 0x3A, 0x3A) ≈ 22% brightness. Standalone constant (no theme.rs entry yet) — promote when Ready chrome gets a proper palette pass.
            if self.device_avatar_pixels.is_some() {
                let diameter = (radius * 2.0) as usize;
                if self.device_avatar_scaled.is_none()
                    || self.device_avatar_scaled_diameter != diameter
                {
                    let base = self.device_avatar_pixels.as_ref().unwrap();
                    self.device_avatar_scaled =
                        Some(crate::ui::avatar_render::update_avatar_scaled(
                            base,
                            crate::ui::avatar::AVATAR_SIZE,
                            diameter,
                        ));
                    self.device_avatar_scaled_diameter = diameter;
                }
                crate::ui::avatar_render::draw_avatar(
                    &mut canvas,
                    cx,
                    cy,
                    radius,
                    self.device_avatar_scaled.as_ref().unwrap(),
                    diameter,
                    None,
                );
            } else {
                // Default unset avatar: our deterministic per-identity gradient (public proof) instead of a flat grey disk.
                let gd = (radius * 2.0).max(1.0) as usize;
                let seed = self
                    .session
                    .as_ref()
                    .map(|s| proof_gradient_seed(&s.handle_proof))
                    .unwrap_or(0);
                crate::ui::avatar_render::draw_avatar(
                    &mut canvas,
                    cx,
                    cy,
                    radius,
                    &gradient_avatar_rgb(seed, gd),
                    gd,
                    None,
                );
            }
            // Stamp the avatar circle into the shared hit_test_map so a tap dispatches to the picker. Squared-distance test in the same row-major buffer the renderers use; bbox-clipped against the buffer extent so off-screen circles don't underflow.
            stamp_hit_circle(
                &mut chrome.hit_test_map,
                buf_w,
                buf_h,
                cx,
                cy,
                radius,
                self.avatar_hit_id,
            );

            // Avatar update hint below the circle — DESKTOP ONLY, shown on hover. On Android, tapping the grey circle to pick an image is self-evident.
            #[cfg(not(target_os = "android"))]
            if self.avatar_hovered {
                // Anchored directly below the avatar circle (not the hint slot), at half the hint slot's text size.
                let size = (ready_layout.hint.y1 - ready_layout.hint.y0) as f32 * 0.3;
                let hcy = cy + radius + size;
                ctx.text.draw_text_center(
                    &mut canvas,
                    "drag/drop to update avatar",
                    cx,
                    hcy,
                    &TextStyle::new(size, fluor::theme::HINT_COLOUR)
                        .weight(500)
                        .font("Oxanium"),
                    None,
                    None,
                );
            }

            // Contacts-page textbox + plus button. The plus button is OVERLAID inside the textbox right edge and ONLY rendered when the textbox has content — empty textbox shows no button. While an add-friend search is in flight, a rotating hourglass replaces the button (and the button is not hit-stampable, so it can't be re-clicked mid-search).
            //
            // Under-blend is topmost-FIRST (first opaque writer wins colour AND its per-pixel hit stamp). Paint the button/hourglass BEFORE the textbox: the button claims its exact pill silhouette in the framebuffer and hit map, and the textbox drawn under it can't overwrite either (its own stamp is per-opaque-pixel too). No hit re-stamp — the draw yields the correct pill-shaped hit area on its own.
            let plus_visible = self
                .contacts_textbox
                .as_ref()
                .map(|tb| !tb.chars.is_empty())
                .unwrap_or(false);
            if self.add_in_flight {
                if let Some(btn) = self.contacts_plus_btn.as_ref() {
                    let sz = btn.width.min(btn.height);
                    draw_hourglass(
                        &mut canvas,
                        btn.center_x,
                        btn.center_y,
                        sz,
                        self.hourglass_angle,
                        *theme::HOURGLASS_COLOUR,
                    );
                }
            } else if plus_visible {
                if let Some(btn) = self.contacts_plus_btn.as_mut() {
                    let id = btn.hit_id();
                    btn.render_content_into(
                        &mut canvas,
                        0.,
                        0.,
                        ctx.text,
                        None,
                        Some(&mut chrome.hit_test_map),
                        id,
                    );
                }
            }
            // Search box placeholder — same treatment as the launch screen's ∞: a grey prompt centred in the empty, unfocused box, painted BEFORE the textbox so the under-blend keeps it behind the empty pill fill. Clears on focus or first character.
            let search_empty = self
                .contacts_textbox
                .as_ref()
                .map(|t| t.chars.is_empty())
                .unwrap_or(true);
            let search_focused = self
                .contacts_textbox
                .as_ref()
                .map(|t| Some(t.hit_id()) == self.focused)
                .unwrap_or(false);
            if search_empty && !search_focused {
                if let Some(tb) = self.contacts_textbox.as_ref() {
                    ctx.text.draw_text_center(
                        &mut canvas,
                        "search | add",
                        tb.center_x,
                        tb.center_y,
                        &TextStyle::new(tb.font_size * 0.6, fluor::theme::HINT_COLOUR)
                            .weight(500)
                            .font("Oxanium"),
                        None,
                        None,
                    );
                }
            }
            if let Some(tb) = self.contacts_textbox.as_mut() {
                let id = tb.hit_id();
                tb.render_content_into(
                    &mut canvas,
                    0.,
                    0.,
                    ctx.text,
                    None,
                    None,
                    Some(&mut chrome.hit_test_map),
                    id,
                );
            }
            // Re-win the plus button's hit silhouette after the search textbox clobbered it (only when the button actually rendered — not during the in-flight hourglass, which isn't clickable).
            if !self.add_in_flight && plus_visible {
                if let Some(btn) = self.contacts_plus_btn.as_ref() {
                    btn.stamp_hit_into(&mut chrome.hit_test_map, buf_w, buf_h, btn.hit_id());
                }
            }

            // Add-friend result text in the hint slot above the search box: green "added {h}", red "not found" / "error: …". Stays until the next search starts (cleared in `submit_add_friend`).
            if let Some((text, colour)) = self.search_status.as_ref() {
                let hint = ready_layout.hint;
                if !hint.is_empty() {
                    let region_h = (hint.y1 - hint.y0) as f32;
                    let scx = (hint.x0 + hint.x1) as f32 * 0.5;
                    let scy = (hint.y0 + hint.y1) as f32 * 0.5 - scroll;
                    ctx.text.draw_text_center(
                        &mut canvas,
                        text,
                        scx,
                        scy,
                        &TextStyle::new(region_h * 0.6, *colour)
                            .weight(500)
                            .font("Oxanium"),
                        None,
                        None,
                    );
                }
            }

            // "Device added √" confirmation — in the hint slot ABOVE the search box (not the bottom band). Green; sits until the next click/keystroke clears it via clear_hints (never time-based). Lifts one line when the add-friend result already occupies the hint slot so the two don't overlap.
            if let Some(msg) = &self.ready_toast {
                let hint = ready_layout.hint;
                if !hint.is_empty() {
                    let region_h = (hint.y1 - hint.y0) as f32;
                    let tcx = (hint.x0 + hint.x1) as f32 * 0.5;
                    let lift = if self.search_status.is_some() {
                        region_h * 1.15
                    } else {
                        0.0
                    };
                    let tcy = (hint.y0 + hint.y1) as f32 * 0.5 - scroll - lift;
                    ctx.text.draw_text_center(
                        &mut canvas,
                        msg,
                        tcx,
                        tcy,
                        &TextStyle::new(region_h * 0.6, *theme::SEARCH_FOUND_COLOUR)
                            .weight(600)
                            .font("Oxanium"),
                        None,
                        None,
                    );
                }
            }

            // ───────── Separator + scrollable contact list ───────── 1-pixel hairline centred in the separator slot (height 0 = hairline; the slot itself is just reserved breathing room around the line).
            let sep = ready_layout.separator;
            paint::fill_rect(
                &mut canvas,
                sep.x0 as isize,
                ((sep.y0 + sep.y1) / 2) as isize - self.contacts_scroll,
                (sep.x1 - sep.x0) as isize,
                0,
                theme::SEPARATOR_COLOUR,
                None,
                None,
            );

            let rows = ready_layout.rows;
            let row_h = ready_layout.row_height.max(1) as isize;
            let diam = ready_layout.contact_avatar_diameter;
            let avatar_r = diam as f32 * 0.5;
            // Rows now scroll up into (and past) where the user section sat, so the clip can no longer stop at `rows.y0`. Clip top = the top of the content area (0); the chrome title bar composites on top afterwards via `chrome.flatten_into`, exactly as it does for the unclipped avatar that already draws high. Keep the x extent at the rows' columns.
            let rows_clip = fluor::paint::Clip::new(rows.x0, 0, rows.x1, buf_h);

            // Filter by the search text (case-insensitive substring on the handle); empty filter = all.
            let filter: String = self
                .contacts_textbox
                .as_ref()
                .map(|t| t.chars.iter().collect::<String>().to_lowercase())
                .unwrap_or_default();
            let mut matching: Vec<usize> = self
                .contacts
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    // Fleet siblings are infrastructure, not conversations — never listed (device management gets its own page later).
                    !c.is_sibling
                        && (filter.is_empty() || c.display_name().to_lowercase().contains(&filter))
                })
                .map(|(i, _)| i)
                .collect();
            // ORDER: unread conversations float to the top, then everyone sorts by MOST-RECENT activity (last message either way — a fresh reply or a fresh receipt lifts the contact). `matching` is the ONE place display order exists — the row loop draws from it AND stamps each row's hit id with the TRUE contact index it holds, so the tap handler resolves taps with no knowledge of the permutation. The key is (unread-first, newest-activity-first); i64::MIN for a contact with no messages sinks it below any conversation.
            let our_handle_hash = self
                .session
                .as_ref()
                .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
                .unwrap_or([0u8; 32]);
            matching.sort_by_key(|&ci| {
                let conv =
                    dm_conversation(&self.conversations, &our_handle_hash, &self.contacts[ci]);
                let last_activity = conv
                    .map(|v| v.messages.as_slice())
                    .unwrap_or(&[])
                    .iter()
                    .filter(|m| !crate::types::is_control_content(&m.content) && !m.deleted)
                    .map(|m| m.timestamp)
                    .max()
                    .unwrap_or(i64::MIN);
                (
                    u8::from(conv.is_none_or(|v| v.unread_count == 0)),
                    std::cmp::Reverse(last_activity),
                )
            });

            // Clamp scroll over the FULL block (user section + rows + version footer), hard-stop at both ends. Down-scroll stops when the version footer (one row past the last row) plus a row of bottom margin reaches the screen bottom; up-scroll stops at rest (0), with the avatar at its natural top. MUST match the pre-chrome clamp above (`block_end = block_bottom_at_zero + row_h*2`) so both passes agree within a frame.
            let block_bottom_at_zero = rows.y0 as isize + matching.len() as isize * row_h;
            let block_end = block_bottom_at_zero + row_h * 2;
            let max_scroll = (block_end - buf_h as isize).max(0);
            if self.contacts_scroll > max_scroll {
                self.contacts_scroll = max_scroll;
            }

            // Row geometry: avatar on the left with a half-radius margin, name to its right.
            let avatar_cx = rows.x0 as f32 + avatar_r * 1.5;
            let text_x = avatar_cx + avatar_r * 1.5;
            let text_size = row_h as f32 * 0.5;
            let ring_thickness = (avatar_r * 0.0375).max(1.0);
            // Handle names render in each contact's relationship colour (spaghettify per visible row is microseconds; revisit with a cache if contact lists ever get huge). `our_handle_hash` is bound above the sort — one derivation for the ordering and the rows.
            for (vis, &ci) in matching.iter().enumerate() {
                // Use the SAME `scroll` snapshot the avatar / hint / search box / separator read (captured up top, before the down-scroll clamp below mutated `self.contacts_scroll`). Reading the live field here made the rows lag the rest of the block by the clamp delta: on an up-scroll past rest the avatar + textbox dragged with the rubber-band overshoot (they read the snapshot) but the names sat still (they read the post-clamp value). One block, one offset.
                let row_top = rows.y0 as isize + vis as isize * row_h - scroll as isize;
                if row_top + row_h <= 0 || row_top >= buf_h as isize {
                    continue; // fully outside the visible content area (rows now scroll up to the top, not just `rows.y0`)
                }
                // Hover/press vocabulary (block tints vetoed): hover = the NAME goes heavier + the presence ring strokes 1px wider; press = the logo's white-glow halo blooms behind the name. No fills, no deltas — weight, stroke, and light.
                let row_hit_here = self.contact_hit_base.wrapping_add(ci as HitId);
                let row_pressed =
                    ci < 256 && ctx.pressed_hit != HIT_NONE && ctx.pressed_hit == row_hit_here;
                let row_hovered = row_pressed
                    || (ci < 256 && ctx.pressed_hit == HIT_NONE && self.hover_hit == row_hit_here);
                let cy = (row_top + row_h / 2) as f32;

                // Build/refresh the contact's scaled-avatar cache at the row diameter.
                let has_avatar = self.contacts[ci].avatar_pixels.is_some();
                if has_avatar
                    && (self.contacts[ci].avatar_scaled.is_none()
                        || self.contacts[ci].avatar_scaled_diameter != diam)
                {
                    let base = self.contacts[ci].avatar_pixels.as_ref().unwrap();
                    let scaled = crate::ui::avatar_render::update_avatar_scaled(
                        base,
                        crate::ui::avatar::AVATAR_SIZE,
                        diam,
                    );
                    self.contacts[ci].avatar_scaled = Some(scaled);
                    self.contacts[ci].avatar_scaled_diameter = diam;
                }

                // Avatar (or placeholder) is topmost; the presence ring paints UNDER it so only the rim shows.
                if let Some(scaled) = self.contacts[ci].avatar_scaled.as_ref() {
                    crate::ui::avatar_render::draw_avatar(
                        &mut canvas,
                        avatar_cx,
                        cy,
                        avatar_r,
                        scaled,
                        diam,
                        Some(rows_clip),
                    );
                } else {
                    // Default unset avatar: the contact's deterministic gradient (their public proof).
                    let gd = (avatar_r * 2.0).max(1.0) as usize;
                    let seed = proof_gradient_seed(&self.contacts[ci].handle_proof);
                    crate::ui::avatar_render::draw_avatar(
                        &mut canvas,
                        avatar_cx,
                        cy,
                        avatar_r,
                        &gradient_avatar_rgb(seed, gd),
                        gd,
                        Some(rows_clip),
                    );
                }
                // The contact's relationship colour — computed ahead of the rings because the unread band below borrows it (ears and eyes and now the unread cue all agree on the one per-contact colour). A zero-remote row gets the neutral anchor (no other party, no relationship).
                let row_colour = if self.contacts[ci].remote_count(&our_handle_hash) == 0 {
                    self_colour()
                } else {
                    party_colour(&relationship_digest(
                        &self.contacts[ci].handle_hash,
                        &our_handle_hash,
                    ))
                };
                let _ = row_colour;
                // Presence ring at the rim (connectivity tier), then — if unread — a MAGENTA ring OUTSIDE it (the new-message cue never overlaps or recolours the connectivity ring). Under-composite paints topmost-first, so the presence disc is drawn before the larger magenta disc and the magenta only shows in its outer annulus. Event-shown, cleared on conversation-open.
                let unread =
                    dm_conversation(&self.conversations, &our_handle_hash, &self.contacts[ci])
                        .is_some_and(|v| v.unread_count > 0);
                let unread_band = ring_thickness * 2.0;
                let ring = row_ring_tier_in(
                    &self.contacts,
                    &self.contacts[ci],
                    self.contacts[ci].remote_count(&our_handle_hash) > 0,
                );
                paint::draw_circle(
                    &mut canvas,
                    avatar_cx,
                    cy,
                    avatar_r + ring_thickness + if row_hovered { 1.0 } else { 0.0 },
                    ring,
                    Some(rows_clip),
                );
                if unread {
                    paint::draw_circle(
                        &mut canvas,
                        avatar_cx,
                        cy,
                        avatar_r + ring_thickness + unread_band,
                        *theme::RING_UNREAD_COLOUR,
                        Some(rows_clip),
                    );
                }

                // Handle name, vertically centred in the row, clipped to the list region — in this contact's relationship colour (computed above).
                // "Pending…" reads in SHEAR (the honest oblique — tan 12°): a name-shaped placeholder must not look like a name. Hover reads as WEIGHT (500 → 700), not a fill — and an unread row holds that same 700 weight until opened.
                let row_weight = if row_hovered || unread { 700 } else { 500 };
                let row_style = if self.contacts[ci].has_real_name() {
                    TextStyle::new(text_size, row_colour)
                        .weight(row_weight)
                        .font("Oxanium")
                } else {
                    TextStyle::new(text_size, row_colour)
                        .weight(row_weight)
                        .font("Oxanium")
                        .shear(0.2126)
                };
                let row_name = self.contacts[ci].display_name_or_pending();
                ctx.text.draw_text_left(
                    &mut canvas,
                    &row_name,
                    text_x,
                    cy,
                    &row_style,
                    Some(rows_clip),
                    None,
                );
                if row_pressed {
                    // Press = the wordmark's halo, scoped to this row — composited AFTER the name (under() = topmost paints first, so program-order-later lands BENEATH the glyphs; the logo calls its glow last for the same reason — glow-first blew the text out to white). Full-width band like the wordmark, so the shared blur math holds.
                    let band_top = row_top.max(0) as usize;
                    let band_h =
                        ((row_top + row_h).min(buf_h as isize) as usize).saturating_sub(band_top);
                    if band_h >= 2 {
                        let mut scratch = vec![0u8; buf_w * band_h];
                        ctx.text.draw_text_left_legacy(
                            &mut scratch,
                            buf_w as u32,
                            band_h as u32,
                            &row_name,
                            text_x,
                            cy - band_top as f32,
                            text_size,
                            row_weight,
                            vec![0xB0],
                            0,
                            "Oxanium",
                        );
                        crate::ui::photon_logo::blur_horizontal_soft(&mut scratch);
                        crate::ui::photon_logo::blur_vertical_soft(&mut scratch, buf_w, band_h);
                        crate::ui::photon_logo::composite_glow_white(
                            canvas.pixels,
                            buf_w,
                            band_top,
                            &scratch,
                        );
                    }
                }

                // Stamp the row into the hit map so clicks dispatch to this contact.
                if ci < 256 {
                    let row_hit = self.contact_hit_base.wrapping_add(ci as HitId);
                    restamp_hit_rect(
                        &mut chrome.hit_test_map,
                        buf_w,
                        buf_h,
                        rows.x0 as isize,
                        row_top.max(0),
                        rows.x1 as isize,
                        (row_top + row_h).min(buf_h as isize),
                        row_hit,
                    );
                }
            }

            // Persistent degraded-vault indicator: amber text at the bottom. The matching warm background tint already lives in the noise pass above (we swap BG_BASE → (*theme::BG_BASE_WARNING)) so we add no extra render pass here, just the text glyph. Full details live in the README.
            if self.vault_degraded {
                // Band height off the span-based layout unit (zoom-aware, aspect-ratio-robust, no pixel floor) — same scaling family as the rest of the screen.
                let band_h = ready_layout.unit_height * 1.5;
                let cx = buf_w as f32 * 0.5;
                let cy = buf_h as f32 - band_h * 0.5;
                let font_size = band_h * 0.6;
                ctx.text.draw_text_center(
                    &mut canvas,
                    "storage degraded",
                    cx,
                    cy,
                    &TextStyle::new(font_size, *theme::DEGRADED_TEXT)
                        .weight(600)
                        .font("Oxanium"),
                    None,
                    None,
                );
            }

            // Auto-attest armed: this box will attest at boot WITHOUT a handle — a standing security posture the operator chose once and must never be allowed to forget (Nick 2026-08-25). Same persistent-band treatment as the degraded/clock indicators, stacked into the same column.
            if self.unattended_on {
                let band_h = ready_layout.unit_height * 1.5;
                let cx = buf_w as f32 * 0.5;
                let rows_below = if self.vault_degraded { 1.0 } else { 0.0 };
                let cy = buf_h as f32 - band_h * (0.5 + rows_below);
                let font_size = band_h * 0.6;
                ctx.text.draw_text_center(
                    &mut canvas,
                    "auto-attest on reboot",
                    cx,
                    cy,
                    &TextStyle::new(font_size, *theme::CLOCK_TEXT)
                        .weight(600)
                        .font("Oxanium"),
                    None,
                    None,
                );
            }

            // Clock-off indicator: same amber as the degraded banner (nunc-time consensus says the system clock is grossly wrong). Warn only — Photon never corrects the clock. Stacks one band above "storage degraded" when both are showing so they don't overlap.
            if let Some(offset_secs) = self.clock_off {
                let band_h = ready_layout.unit_height * 1.5;
                let cx = buf_w as f32 * 0.5;
                // Sit at the bottom, lifted past whichever standing bands are up (storage degraded, auto-attest).
                let rows_below = (if self.vault_degraded { 1.0 } else { 0.0 })
                    + (if self.unattended_on { 1.0 } else { 0.0 });
                let cy = buf_h as f32 - band_h * (0.5 + rows_below);
                let font_size = band_h * 0.6;
                // Human-readable magnitude + direction. ahead = system clock reads later than truth.
                let mag = offset_secs.unsigned_abs();
                let pretty = if mag >= 3600 {
                    format!("{}h", mag / 3600)
                } else if mag >= 60 {
                    format!("{}m", mag / 60)
                } else {
                    format!("{}s", mag)
                };
                let dir = if offset_secs < 0 { "ahead" } else { "behind" };
                let label = format!("clock off — {} {}", pretty, dir);
                ctx.text.draw_text_center(
                    &mut canvas,
                    &label,
                    cx,
                    cy,
                    &TextStyle::new(font_size, *theme::CLOCK_TEXT)
                        .weight(600)
                        .font("Oxanium"),
                    None,
                    None,
                );
            }

            // (The Security / Recovery posture meters that used to sit bottom-right were removed — the security posture belongs on a dedicated Security page, not as ambient bottom-strip dots that read as noise. identity_posture/posture_colour/POSTURE_PIPS stay defined for that page.)
        }

        // Conversation screen — shows the selected contact's name, clutch state, and (eventually) messages.
        // Contact panel — the Settings screen's exact structure, contact-scoped: same SettingsLayout, pinned-Back nav rail with page rows (About / Between you / Manage), hairline divider, scrolled natural-height content. Rides the SAME scroll fields/extents as settings.
        if let AppState::ContactPanel(cpage) = self.state {
            let layout = SettingsLayout::compute(&ctx.viewport);
            let mut canvas = Canvas::new(target, buf_w, buf_h, ctx.damage);
            if let Some(ci) = active_ci {
                // Clear the panel region's hit stamps before re-stamping this frame (immediate-mode stamps must not linger across page switches).
                restamp_hit_rect(
                    &mut chrome.hit_test_map,
                    buf_w,
                    buf_h,
                    0,
                    layout.rail.y as isize,
                    buf_w as isize,
                    buf_h as isize,
                    HIT_NONE,
                );

                // Avatar cache at the About-page diameter, rebuilt BEFORE the immutable contact borrow.
                let avatar_r = layout.unit * 2.0;
                let diam = (avatar_r * 2.0) as usize;
                if cpage == ContactPage::About
                    && self.contacts[ci].avatar_pixels.is_some()
                    && (self.contacts[ci].avatar_scaled.is_none()
                        || self.contacts[ci].avatar_scaled_diameter != diam)
                {
                    let base = self.contacts[ci].avatar_pixels.as_ref().unwrap();
                    let scaled = crate::ui::avatar_render::update_avatar_scaled(
                        base,
                        crate::ui::avatar::AVATAR_SIZE,
                        diam,
                    );
                    self.contacts[ci].avatar_scaled = Some(scaled);
                    self.contacts[ci].avatar_scaled_diameter = diam;
                }
                let contact = &self.contacts[ci];
                // Our pid feeds the relationship digest below — a keyed colour, not a self-check. "Is this me" is the participant count.
                let our_hh = self
                    .session
                    .as_ref()
                    .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
                    .unwrap_or([0u8; 32]);
                let is_self = contact.remote_count(&our_hh) == 0;

                // --- Header: the contact's name, centred on the rail|content divider — the panel's "Settings" slot. ---
                let hspan = (layout.unit * 1.05).min(layout.header.h * 0.72);
                let name_colour = if is_self {
                    self_colour()
                } else {
                    party_colour(&relationship_digest(&contact.handle_hash, &our_hh))
                };
                ctx.text.draw_text_center(
                    &mut canvas,
                    &contact.display_name_or_pending(),
                    layout.content.x,
                    layout.header.center_y(),
                    &TextStyle::new(hspan, name_colour)
                        .weight(600)
                        .font("Oxanium"),
                    None,
                    None,
                );

                // --- Nav rail: pinned Back (returns to the conversation), then the page rows scrolling below — the settings rail verbatim. ---
                let rail_inset = layout.rail_inset();
                let nav_h = layout.nav_row_h();
                let rspan = layout.unit * 0.58;
                {
                    let r =
                        fluor::region::Region::new(rail_inset.x, rail_inset.y, rail_inset.w, nav_h);
                    let back_held =
                        ctx.pressed_hit != HIT_NONE && ctx.pressed_hit == self.back_btn_hit_id;
                    ctx.text.draw_text_left(
                        &mut canvas,
                        "\u{2039} Back",
                        r.x + rspan * 0.6,
                        r.center_y(),
                        &TextStyle::new(rspan, *theme::SEARCH_FOUND_COLOUR)
                            .weight(600)
                            .font("Oxanium"),
                        None,
                        None,
                    );
                    let fill = if back_held {
                        fluor::theme::BUTTON_HELD
                    } else {
                        theme::BACK_BUTTON_IDLE_FILL
                    };
                    paint::fill_rect(
                        &mut canvas,
                        r.x as isize,
                        r.y as isize,
                        r.w as isize,
                        r.h as isize,
                        fill,
                        None,
                        None,
                    );
                    restamp_hit_rect(
                        &mut chrome.hit_test_map,
                        buf_w,
                        buf_h,
                        r.x as isize,
                        r.y as isize,
                        r.right() as isize,
                        r.bottom() as isize,
                        self.back_btn_hit_id,
                    );
                }
                let pages_top = rail_inset.y + nav_h;
                let pages_clip = fluor::paint::Clip::new(
                    layout.rail.x.max(0.0) as usize,
                    pages_top.max(layout.rail.y).max(0.0) as usize,
                    layout.rail.right().max(0.0) as usize,
                    layout.rail.bottom().max(0.0) as usize,
                );
                for (i, p) in ContactPage::ALL.iter().enumerate() {
                    let r = fluor::region::Region::new(
                        rail_inset.x,
                        pages_top - settings_rail_scroll + i as Coord * nav_h,
                        rail_inset.w,
                        nav_h,
                    );
                    if r.bottom() <= pages_top || r.y >= layout.rail.bottom() {
                        continue;
                    }
                    let active = *p == cpage;
                    let held = ctx.pressed_hit != HIT_NONE
                        && ctx.pressed_hit == self.contact_nav_base.wrapping_add(i as HitId);
                    let colour = if active {
                        *theme::CONTACT_NAME_COLOUR
                    } else {
                        *theme::LABEL_COLOUR
                    };
                    ctx.text.draw_text_left(
                        &mut canvas,
                        p.label(),
                        r.x + rspan * 0.6,
                        r.center_y(),
                        &TextStyle::new(rspan, colour)
                            .weight(if active { 600 } else { 400 })
                            .font("Oxanium"),
                        Some(pages_clip),
                        None,
                    );
                    if held {
                        paint::fill_rect(
                            &mut canvas,
                            r.x as isize,
                            r.y as isize,
                            r.w as isize,
                            r.h as isize,
                            fluor::theme::BUTTON_HELD,
                            Some(pages_clip),
                            None,
                        );
                    } else if active {
                        paint::fill_rect(
                            &mut canvas,
                            r.x as isize,
                            r.y as isize,
                            r.w as isize,
                            r.h as isize,
                            theme::SEPARATOR_COLOUR,
                            Some(pages_clip),
                            None,
                        );
                    }
                    restamp_hit_rect(
                        &mut chrome.hit_test_map,
                        buf_w,
                        buf_h,
                        r.x as isize,
                        r.y.max(pages_top) as isize,
                        r.right() as isize,
                        r.bottom().min(layout.rail.bottom()) as isize,
                        self.contact_nav_base.wrapping_add(i as HitId),
                    );
                }
                paint::fill_rect(
                    &mut canvas,
                    layout.content.x as isize,
                    layout.content.y as isize,
                    1,
                    layout.content.h as isize,
                    theme::SEPARATOR_COLOUR,
                    None,
                    None,
                );

                // --- Selected page body: natural-height rows over the shared content scroll, clipped to the reading column. ---
                let inset = layout.content_inset();
                let content_clip = fluor::paint::Clip::new(
                    inset.x.max(0.0) as usize,
                    inset.y.max(0.0) as usize,
                    inset.right().max(0.0) as usize,
                    inset.bottom().max(0.0) as usize,
                );
                let tspan = layout.unit * 0.72;
                let hspan2 = tspan * 0.75;
                // Display doctrine (matches clutch_status_detail): dozenal is the acclimation surface for VERSION + REPUTATION only — counters stay in current mixed arabic units for now.
                match cpage {
                    ContactPage::About => {
                        let n = contact_page_rows(ContactPage::About);
                        let rows = layout
                            .content_scrolled(n, settings_content_scroll)
                            .split_v([1.0; 12]);
                        // Avatar block spans the first 5 rows: presence-tier ring under the picture, centred in the column.
                        let block = fluor::region::Region::new(
                            rows[0].x,
                            rows[0].y,
                            rows[0].w,
                            rows[0].h * 5.0,
                        );
                        let (cx, cy) = (block.center_x(), block.center_y());
                        let ring = row_ring_tier_in(&self.contacts, contact, !is_self);
                        if let Some(scaled) = contact.avatar_scaled.as_ref() {
                            crate::ui::avatar_render::draw_avatar(
                                &mut canvas,
                                cx,
                                cy,
                                avatar_r,
                                scaled,
                                diam,
                                Some(content_clip),
                            );
                        } else {
                            let gd = diam.max(1);
                            let seed = proof_gradient_seed(&contact.handle_proof);
                            crate::ui::avatar_render::draw_avatar(
                                &mut canvas,
                                cx,
                                cy,
                                avatar_r,
                                &gradient_avatar_rgb(seed, gd),
                                gd,
                                Some(content_clip),
                            );
                        }
                        paint::draw_circle(
                            &mut canvas,
                            cx,
                            cy,
                            avatar_r + (avatar_r * 0.0375).max(1.0),
                            ring,
                            Some(content_clip),
                        );
                        let shared_name = if is_self {
                            "your own notes-to-self conversation".to_string()
                        } else if contact.published_name.is_empty() {
                            "name: not shared yet".to_string()
                        } else {
                            format!(
                                "name: \u{201c}{}\u{201d} (their published name)",
                                contact.published_name
                            )
                        };
                        let shared_avatar = if is_self {
                            String::new()
                        } else if contact.avatar_pin == [0u8; 64] {
                            "avatar: not shared yet".to_string()
                        } else {
                            "avatar: shared with you".to_string()
                        };
                        let identity_line = if is_self {
                            "no ceremony, no chain \u{2014} rows ride your fleet key".to_string()
                        } else if contact.identity_superseded {
                            "\u{26a0} this name was re-claimed by someone else \u{2014} rendering a stranger".to_string()
                        } else if contact.identity_ended {
                            "identity ended by its owner".to_string()
                        } else if contact.pinned_genesis != [0u8; 32] {
                            format!("identity pinned since first fold \u{00b7} {} device(s) in their fleet", contact.fleet_members.len().max(1))
                        } else {
                            "identity not yet folded (first contact still settling)".to_string()
                        };
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[5],
                            "What they share with you",
                            tspan,
                            *theme::CONTACT_NAME_COLOUR,
                            600,
                        );
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[6],
                            &shared_name,
                            hspan2,
                            *theme::LABEL_COLOUR,
                            400,
                        );
                        if !shared_avatar.is_empty() {
                            settings_line(
                                &mut canvas,
                                ctx.text,
                                rows[7],
                                &shared_avatar,
                                hspan2,
                                *theme::LABEL_COLOUR,
                                400,
                            );
                        }
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[9],
                            "Identity",
                            tspan,
                            *theme::CONTACT_NAME_COLOUR,
                            600,
                        );
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[10],
                            &identity_line,
                            hspan2,
                            *theme::LABEL_COLOUR,
                            400,
                        );
                        // The deterministic two-word voca pseudonym, always shown even when a published name renders elsewhere: it derives from the party id, so it's the one name that can't be changed or spoofed — the human-checkable identity anchor (compare it out-of-band and you've verified the contact).
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[11],
                            &format!("always \u{201c}{}\u{201d} \u{2014} derived from their identity, can\u{2019}t be changed", crate::network::fgtw::fleet::keyed_pseudonym(&contact.handle_hash)),
                            hspan2,
                            *theme::LABEL_COLOUR,
                            400,
                        );
                    }
                    ContactPage::Stats => {
                        let n = contact_page_rows(ContactPage::Stats);
                        let rows = layout
                            .content_scrolled(n, settings_content_scroll)
                            .split_v([1.0; 9]);
                        // Hidden probe rows are bookkeeping, not conversation — keep them out of every human-facing count.
                        let conv = dm_conversation(&self.conversations, &our_hh, contact);
                        let human: Vec<&crate::types::ChatMessage> = conv
                            .map(|v| v.messages.as_slice())
                            .unwrap_or(&[])
                            .iter()
                            .filter(|m| !crate::types::is_control_content(&m.content) && !m.deleted)
                            .collect();
                        let sent = human.iter().filter(|m| m.is_outgoing).count();
                        let recv = human.len() - sent;
                        let delivered = human
                            .iter()
                            .filter(|m| m.is_outgoing && m.delivered)
                            .count();
                        let span_days = {
                            let first = human.iter().map(|m| m.timestamp).min();
                            let last = human.iter().map(|m| m.timestamp).max();
                            match (first, last) {
                                (Some(a), Some(b)) if b > a => {
                                    ((b - a) / (vsf::OSCILLATIONS_PER_SECOND as i64 * 86_400))
                                        .max(0) as usize
                                }
                                _ => 0,
                            }
                        };
                        let history_line = match conv
                            .and_then(|v| v.history_recovery.as_ref())
                            .map(|r| r.complete)
                        {
                            Some(true) => "history: complete on this device".to_string(),
                            Some(false) => "history: still syncing".to_string(),
                            None => "history: idle (no sweep this session)".to_string(),
                        };
                        let chain_line = if is_self {
                            "no chain \u{2014} delivered by definition".to_string()
                        } else if contact.chain_woven {
                            "chain woven \u{2014} secured end-to-end".to_string()
                        } else {
                            contact_status_line(
                                contact,
                                self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes()),
                                self.session.as_ref().map(|se| &se.identity_seed),
                            )
                        };
                        let connection_line = if is_self {
                            "always reachable (this is you)".to_string()
                        } else if contact.is_online {
                            if contact.reached_via_relay {
                                "connected \u{00b7} via relay".to_string()
                            } else {
                                "connected \u{00b7} direct".to_string()
                            }
                        } else {
                            "offline".to_string()
                        };
                        // These rows should CONVERGE across your fleet devices — two devices showing different numbers here IS the sync bug, made visible.
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[0],
                            "Between you",
                            tspan,
                            *theme::CONTACT_NAME_COLOUR,
                            600,
                        );
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[1],
                            &format!(
                                "{} message(s) \u{00b7} {} sent \u{00b7} {} received",
                                human.len(),
                                sent,
                                recv
                            ),
                            hspan2,
                            *theme::LABEL_COLOUR,
                            400,
                        );
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[2],
                            &format!("{} of your messages delivered", delivered),
                            hspan2,
                            *theme::LABEL_COLOUR,
                            400,
                        );
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[3],
                            &format!("chatting across {} day(s)", span_days),
                            hspan2,
                            *theme::LABEL_COLOUR,
                            400,
                        );
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[4],
                            &history_line,
                            hspan2,
                            *theme::LABEL_COLOUR,
                            400,
                        );
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[5],
                            &chain_line,
                            hspan2,
                            *theme::LABEL_COLOUR,
                            400,
                        );
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[6],
                            &connection_line,
                            hspan2,
                            *theme::LABEL_COLOUR,
                            400,
                        );
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[8],
                            "these rows should match on every one of your devices",
                            hspan2 * 0.9,
                            *theme::LABEL_COLOUR,
                            400,
                        );
                    }
                    ContactPage::Manage => {
                        let n = contact_page_rows(ContactPage::Manage);
                        let rows = layout
                            .content_scrolled(n, settings_content_scroll)
                            .split_v([1.0; 6]);
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[0],
                            "Manage",
                            tspan,
                            *theme::CONTACT_NAME_COLOUR,
                            600,
                        );
                        if is_self || contact.is_sibling {
                            settings_line(
                                &mut canvas,
                                ctx.text,
                                rows[1],
                                if is_self {
                                    "your own notes can\u{2019}t be booted"
                                } else {
                                    "a fleet device signs itself out \u{2014} see Settings \u{2192} Fleet"
                                },
                                hspan2,
                                *theme::LABEL_COLOUR,
                                400,
                            );
                        } else {
                            let pill = fluor::region::Region::new(
                                rows[2].x + rows[2].w * 0.1,
                                rows[2].y,
                                rows[2].w * 0.5,
                                rows[2].h * 0.95,
                            );
                            let label = if self.contact_boot_armed {
                                "Tap again \u{2014} boot them"
                            } else {
                                "Boot"
                            };
                            draw_stub_pill(
                                &mut canvas,
                                ctx.text,
                                &mut chrome.hit_test_map,
                                buf_w,
                                buf_h,
                                pill,
                                label,
                                self.contact_panel_btn_base,
                                ctx.pressed_hit,
                            );
                            settings_line(
                                &mut canvas,
                                ctx.text,
                                rows[3],
                                "removes them from every device of YOUR fleet",
                                hspan2,
                                *theme::LABEL_COLOUR,
                                400,
                            );
                            settings_line(&mut canvas, ctx.text, rows[4], "they are not told \u{2014} their records stay theirs (ostracism, not erasure)", hspan2, *theme::LABEL_COLOUR, 400);
                        }
                    }
                }
            }
        }

        if matches!(self.state, AppState::Conversation) {
            let mut canvas = Canvas::new(target, buf_w, buf_h, ctx.damage);
            if let Some(ci) = active_ci {
                {
                    let ru = ctx.viewport.ru;
                    // Build/refresh the contact's scaled-avatar cache at the CONVERSATION-HEADER diameter BEFORE the immutable borrow below. The header renders the avatar bigger than the contact-list rows, but it has no rebuild of its own — it used to draw whatever `avatar_scaled` happened to hold (built at the small row diameter) while telling draw_avatar the buffer was header-sized → it sampled past the smaller buffer → "index out of bounds: len 2028 (26²·3) but index 2307" panic on conversation-open. Rebuilding here at the header diameter keeps the cache and the claimed scaled_diameter in lockstep.
                    {
                        let (_, _, header_r) =
                            ReadyLayout::compute(buf_w, buf_h, ru).avatar_center_radius();
                        let header_diam = (header_r * 2.0) as usize;
                        if self.contacts[ci].avatar_pixels.is_some()
                            && (self.contacts[ci].avatar_scaled.is_none()
                                || self.contacts[ci].avatar_scaled_diameter != header_diam)
                        {
                            let base = self.contacts[ci].avatar_pixels.as_ref().unwrap();
                            let scaled = crate::ui::avatar_render::update_avatar_scaled(
                                base,
                                crate::ui::avatar::AVATAR_SIZE,
                                header_diam,
                            );
                            self.contacts[ci].avatar_scaled = Some(scaled);
                            self.contacts[ci].avatar_scaled_diameter = header_diam;
                        }
                    }
                    let contact = &self.contacts[ci];
                    // Scale off the SAME span-based harmonic unit the contacts screen uses, so the conversation screen scales identically (aspect-ratio-robust, zoom-aware, no hardcoded pixels) instead of the old crude height-only `buf_h·0.04` with a magic 12px floor.
                    let conv_layout = ReadyLayout::compute(buf_w, buf_h, ru);
                    let unit = conv_layout.unit_height;

                    // Back arrow (top-left) — below the chrome title bar area. Slides off vertically by conv_topbar_off (scroll-tied, browser-toolbar style); the hit rect follows and stamps HIT_NONE once mostly gone so a ghost tap can't fire it.
                    let back_size = unit * 1.15;
                    let bar_h = buf_h as f32 * 0.06 + unit + back_size;
                    let bar_off = self.conv_topbar_off.min(bar_h);
                    let back_y = buf_h as f32 * 0.06 + unit - bar_off;
                    let back_text = "\u{2039} Contacts";
                    let topbar_visible = bar_off < bar_h * 0.75;
                    // Same hover/press vocabulary as the contact rows: hover = weight 500 → 700, press = the wordmark's glow behind the label (composited AFTER the text — under() layers beneath).
                    let back_pressed = topbar_visible
                        && ctx.pressed_hit != HIT_NONE
                        && ctx.pressed_hit == self.back_btn_hit_id;
                    let back_hovered = back_pressed
                        || (topbar_visible
                            && ctx.pressed_hit == HIT_NONE
                            && self.hover_hit == self.back_btn_hit_id);
                    let back_weight = if back_hovered { 700 } else { 500 };
                    if back_y > -back_size {
                        ctx.text.draw_text_left(
                            &mut canvas,
                            back_text,
                            unit,
                            back_y,
                            &TextStyle::new(back_size, *theme::CONTACT_NAME_COLOUR)
                                .weight(back_weight)
                                .font("Oxanium"),
                            None,
                            None,
                        );
                    }
                    if back_pressed {
                        let band_top = (back_y - back_size).max(0.) as usize;
                        let band_h =
                            (((back_y + back_size) as usize).min(buf_h)).saturating_sub(band_top);
                        if band_h >= 2 {
                            let mut scratch = vec![0u8; buf_w * band_h];
                            ctx.text.draw_text_left_legacy(
                                &mut scratch,
                                buf_w as u32,
                                band_h as u32,
                                back_text,
                                unit,
                                back_y - band_top as f32,
                                back_size,
                                back_weight,
                                vec![0xB0],
                                0,
                                "Oxanium",
                            );
                            crate::ui::photon_logo::blur_horizontal_soft(&mut scratch);
                            crate::ui::photon_logo::blur_vertical_soft(&mut scratch, buf_w, band_h);
                            crate::ui::photon_logo::composite_glow_white(
                                canvas.pixels,
                                buf_w,
                                band_top,
                                &scratch,
                            );
                        }
                    }
                    // Stamp the back button hit rect.
                    let back_w = ctx.text.measure_text(
                        back_text,
                        &TextStyle::new(back_size, 0)
                            .weight(back_weight)
                            .font("Oxanium"),
                    );
                    restamp_hit_rect(
                        &mut chrome.hit_test_map,
                        buf_w,
                        buf_h,
                        0,
                        (back_y - back_size) as isize,
                        (unit + back_w + unit) as isize,
                        (back_y + back_size) as isize,
                        if topbar_visible {
                            self.back_btn_hit_id
                        } else {
                            HIT_NONE
                        },
                    );

                    // ONE LAYOUT, ONE LAYER (user spec, 2026-07-26): the conversation is a single scrolling stream whose ENTRY #0 is the avatar + name (+ ceremony status while pending) — visible ONLY at the conversation GENESIS, at the literal top of the content area, scrolling like any message. The fixed centred header is DEAD for every state (its pre-woven survival was the root of the "different layer" saga). The fixed strip holds ONLY the tiny always-on name, the orb, and the sliding "‹ Contacts".
                    let (_, _, avatar_r) = conv_layout.avatar_center_radius();
                    let avatar_diam = (avatar_r * 2.0) as usize;
                    let avatar_cx = buf_w as f32 * 0.5;
                    // Relationship colour inputs, hoisted above the avatar closure so both it and the header share them. Our pid feeds the relationship digest — a keyed colour, not a self-check; "is this me" is the participant count.
                    let our_handle_hash = self
                        .session
                        .as_ref()
                        .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
                        .unwrap_or([0u8; 32]);
                    // The device-derived sibling pid, captured here (immutable self reads only) so the conv key below needs no &self call while `contact` holds a borrow.
                    let our_sibling_pid = self
                        .device_keypair
                        .as_ref()
                        .map(|kp| crate::crypto::clutch::sibling_party_id(kp.public.as_bytes()));
                    let is_self_contact = contact.remote_count(&our_handle_hash) == 0;
                    // BRIDGE locus strip inputs (2026-08-23, the blind-cwd fix): the host-reported host:cwd for THIS sibling device, plus whether a command is in flight — which puts the Stop pill in the band.
                    let bridge_strip_txt: Option<String> = if contact.is_sibling {
                        self.bridge_locus
                            .as_ref()
                            .filter(|(d, _, _)| Some(*d) == contact.device_key())
                            .map(|(_, h, c)| format!("{h}:{c}"))
                    } else {
                        None
                    };
                    // THE conversation key MUST match how the SEND path keyed it (`our_party_id`): for a SIBLING that is the device-derived sibling pid, NOT our_handle_hash (the identity pid). Using our_handle_hash here made the render look up an EMPTY phantom conversation for every sibling — the send inserted the bubble into the sibling-pid-keyed conversation, the screen painted the identity-keyed one, and a bridge command vanished on send ("BOOP", field 2026-08-21). Friends and self are unaffected: their party id already EQUALS our_handle_hash. Mirrors `our_party_id` exactly, so insert and render read the same object.
                    let conv_party_id = if contact.is_sibling {
                        our_sibling_pid.unwrap_or(our_handle_hash)
                    } else {
                        our_handle_hash
                    };
                    // The conversation this screen paints — messages, scroll, unread all read from here, never from the contact. Field-precise lookup: the scope below writes disjoint `self` fields while this borrow is live.
                    let conv: Option<&crate::types::Conversation> = {
                        let id = contact.conversation(&conv_party_id).id();
                        self.conversations.iter().find(|v| v.id() == id)
                    };
                    // In-flight = the newest outgoing BridgeCmd with no FINAL output yet (bridge_exit stamped by the replace-in-place). Field-precise mirror of bridge_inflight_target — a &self method call here would collide with the live chrome borrow.
                    let bridge_inflight = contact.is_sibling
                        && conv.map_or(false, |v| {
                            v.messages
                                .iter()
                                .rev()
                                .find(|m| {
                                    m.is_outgoing
                                        && matches!(
                                            m.reference,
                                            Some((crate::types::RefKind::BridgeCmd, _))
                                        )
                                })
                                .map_or(false, |cmd| {
                                    !v.messages.iter().any(|m| {
                                        m.reference
                                            == Some((
                                                crate::types::RefKind::BridgeOut,
                                                cmd.timestamp,
                                            ))
                                            && m.bridge_exit.is_some()
                                    })
                                })
                        });
                    // Ring computed BEFORE the closure: row_ring_tier borrows &self, and the closure outlives writes to disjoint self fields below.
                    let conv_ring = row_ring_tier_in(&self.contacts, contact, !is_self_contact);
                    // Stamp the avatar disc + tier ring at a given centre-y — stream entry #0's avatar. Clip rides in as a parameter and the caller passes the LIST clip: the avatar obeys exactly the same boundary as every message (a hardcoded None once let it paint through the top edge onto its own visual layer).
                    let draw_conv_avatar =
                        |canvas: &mut Canvas, cy: f32, clip: Option<fluor::paint::Clip>| {
                            if let Some(scaled) = contact.avatar_scaled.as_ref() {
                                crate::ui::avatar_render::draw_avatar(
                                    canvas,
                                    avatar_cx,
                                    cy,
                                    avatar_r,
                                    scaled,
                                    avatar_diam,
                                    clip,
                                );
                            } else {
                                let gd = (avatar_r * 2.0).max(1.0) as usize;
                                let seed = proof_gradient_seed(&contact.handle_proof);
                                crate::ui::avatar_render::draw_avatar(
                                    canvas,
                                    avatar_cx,
                                    cy,
                                    avatar_r,
                                    &gradient_avatar_rgb(seed, gd),
                                    gd,
                                    clip,
                                );
                            }
                            let ring = conv_ring;
                            let ring_thick = (avatar_r * 0.0375).max(1.0);
                            paint::draw_circle(
                                canvas,
                                avatar_cx,
                                cy,
                                avatar_r + ring_thick,
                                ring,
                                clip,
                            );
                        };

                    // Relationship colour for this contact: everything handle-specific on this screen (name, their message text) renders in it. A zero-remote conversation has no other party, so no relationship colour — everything is the neutral anchor.
                    let their_colour = if is_self_contact {
                        self_colour()
                    } else {
                        party_colour(&relationship_digest(&contact.handle_hash, &our_handle_hash))
                    };

                    // Petname style for stream entry #0 (pending names shear italic like everywhere else).
                    let name_size = unit * 1.2;
                    let header_style = if contact.has_real_name() {
                        TextStyle::new(name_size, their_colour)
                            .weight(600)
                            .font("Oxanium")
                    } else {
                        TextStyle::new(name_size, their_colour)
                            .weight(600)
                            .font("Oxanium")
                            .shear(0.2126)
                    };

                    // CLUTCH/lifecycle status — computed here, DRAWN inside stream entry #0 (under the name, at genesis). End-of-identity states outrank the ceremony line; a woven chain shows no line at all (the working conversation is its own proof); self shows one only while empty.
                    let show_status = contact.identity_superseded
                        || contact.identity_ended
                        || (is_self_contact && conv.is_none_or(|v| v.messages.is_empty()))
                        || (!is_self_contact && !contact.chain_woven);
                    let status_in_stream: Option<(String, u32)> = if show_status {
                        Some(if contact.identity_superseded {
                            (
                                "name re-claimed by someone new \u{2014} this is NOT them"
                                    .to_string(),
                                *theme::ERROR_TEXT_COLOUR,
                            )
                        } else if contact.identity_ended {
                            (
                                "identity ended \u{2014} conversation frozen".to_string(),
                                *theme::LABEL_COLOUR,
                            )
                        } else if is_self_contact {
                            ("notes to self".to_string(), *theme::SEARCH_FOUND_COLOUR)
                        } else {
                            (
                                format!(
                                    "CLUTCH: {}",
                                    contact_status_line(
                                        contact,
                                        self.device_keypair
                                            .as_ref()
                                            .map(|kp| *kp.public.as_bytes()),
                                        self.session.as_ref().map(|se| &se.identity_seed)
                                    )
                                ),
                                if contact.clutch_state == crate::types::ClutchState::Complete {
                                    *theme::SEARCH_FOUND_COLOUR
                                } else {
                                    *theme::HOURGLASS_COLOUR
                                },
                            )
                        })
                    } else {
                        None
                    };

                    // The stream renders for EVERY conversation state — an empty one is just entry #0 (avatar/name/status) alone. Only the COMPOSE box stays gated below (sending needs a chain somewhere).
                    {
                        // ── Message list ─────────────────────────────────────────── Text-only, right-aligned (outgoing) / left-aligned (incoming), one thin white divider after every message. Newest at the bottom, just above the compose bar; older scroll up off-screen.
                        // Our text is the neutral-grey anchor (same Y = 0.5, zero chroma); theirs is the relationship colour computed above.
                        let our_colour = self_colour();

                        let msg_size = unit * 0.62;
                        let line_h = msg_size * 1.6; // text + breathing room per message
                        let pad_x = unit; // left/right inset
                                          // Woven chat reclaims the whole header strip for the message list (the avatar/name ride the scroll-top instead, drawn below); pre-woven keeps the status header space.
                                          // The floor clears the CHROME title strip on desktop plus the tiny always-on name; Android draws no strip (full-edge) so a slim margin stands.
                        let top_floor = if cfg!(target_os = "android") {
                            unit * 1.1
                        } else {
                            fluor::host::chrome::strip_height(ctx.viewport) + unit * 0.9
                        };
                        let list_top = (back_y + unit).max(top_floor);
                        // Compose bar reserves the bottom strip, lifted off the bottom edge by `compose_margin` — and above the soft keyboard (`ime_lift`; the surface never resizes for the IME). The list lives between list_top and list_bottom. Must match the layout pass's `compose_h`/`compose_margin` below.
                        let compose_h = unit * 1.8;
                        let compose_margin = unit * 0.8;
                        // Armed reply/edit strip: one extra band above the compose box naming what the next send references (drawn half-alpha below). Reserved OUT of the list so it never overdraws the newest message.
                        let compose_strip: Option<(i64, u8)> = if compose_ready {
                            self.compose_edit_of
                                .map(|t| (t, 1u8))
                                .or(self.compose_react_to.map(|t| (t, 2u8)))
                                .or(self.compose_reply_to.map(|t| (t, 0u8)))
                        } else {
                            None
                        };
                        // The compose box GROWS upward now — the list yields to its live height, not the one-line constant.
                        let live_compose_h = self
                            .message_textbox
                            .as_ref()
                            .map(|t| t.height)
                            .unwrap_or(compose_h)
                            .max(compose_h);
                        let list_bottom = buf_h as f32
                            - ime_lift
                            - live_compose_h
                            - compose_margin
                            - unit * 0.5
                            - if compose_strip.is_some()
                                || (bridge_strip_txt.is_some() || bridge_inflight)
                            {
                                unit * 0.9
                            } else {
                                0.0
                            };
                        // Clamp so a short window (tall header) can never invert the clip (list_top > list_bottom) — that's what made every message vanish on resize. When there's no room, list_bottom collapses to list_top and the list is simply empty rather than drawing with a negative-height (inverted) clip.
                        let list_bottom = list_bottom.max(list_top);
                        // Status toast on the CONVERSATION screen — the Ready hint slot and the Settings pane both draw `ready_toast`, but this AppState never did, which made a refused self-row persist (its toast fires while the user is right here) invisible (2026-08-21 erasure ticket). Above the compose bar, painted early so under-blend keeps it over the list; event-shown, cleared on the next interaction via clear_toast, never time-based.
                        if let Some(msg) = &self.ready_toast {
                            let ts = unit * 0.72;
                            ctx.text.draw_text_center(
                                &mut canvas,
                                msg,
                                buf_w as f32 * 0.5,
                                list_bottom - ts * 0.4,
                                &TextStyle::new(ts, *theme::SEARCH_FOUND_COLOUR)
                                    .weight(600)
                                    .font("Oxanium"),
                                None,
                                None,
                            );
                        }
                        let list_clip = fluor::paint::Clip::new(
                            0,
                            list_top as usize,
                            buf_w,
                            list_bottom as usize,
                        );

                        // Lay messages out bottom-up so the newest sits at list_bottom. Clamp scroll offset to the actual overscroll range so a stale offset from a previous (larger) window size can't push every message above list_top on resize.
                        // Probe rows (hidden chain-weave records, persisted for re-ACK durability) never render — filter before layout so the scroll height matches what's drawn.
                        let raw_msgs: &[crate::types::ChatMessage] =
                            conv.map(|v| v.messages.as_slice()).unwrap_or(&[]);
                        // Newest live edit row per target — render-time supersede (the original row is braid key material and never mutates; see EDIT_MARKER_PREFIX). Deleting an edit row reverts to the previous edit or the original.
                        let mut edit_over: std::collections::HashMap<i64, (i64, String)> =
                            std::collections::HashMap::new();
                        for m in raw_msgs.iter().filter(|m| !m.deleted) {
                            if let Some((crate::types::RefKind::Edit, t)) = m.reference {
                                let e = edit_over
                                    .entry(t)
                                    .or_insert_with(|| (m.timestamp, m.content.clone()));
                                if m.timestamp >= e.0 {
                                    *e = (m.timestamp, m.content.clone());
                                }
                            }
                        }
                        // Current reaction per target per direction — the shared builder (the scroll walk counts with the same map).
                        let react_over = build_react_over(raw_msgs);
                        // The line a reacted bubble grows underneath — PER PARTY, because each glyph paints in its reactor's colour (the field call, 2026-08-09: "make sure the colour matches the party"): (theirs, ours), retracts dropped, None when neither.
                        let react_line = |ts: i64| -> Option<(Option<String>, Option<String>)> {
                            let slots = react_over.get(&ts)?;
                            let pick = |s: &Option<(i64, String)>| {
                                s.as_ref().map(|(_, g)| g.clone()).filter(|g| !g.is_empty())
                            };
                            let theirs = pick(&slots[0]);
                            let ours = pick(&slots[1]);
                            if theirs.is_none() && ours.is_none() {
                                None
                            } else {
                                Some((theirs, ours))
                            }
                        };
                        // Bubble DISPLAY body: attachments keep their pill line; an edited row shows its newest edit body; reply/edit markers strip to their text.
                        let body_of = |m: &crate::types::ChatMessage| -> String {
                            if crate::types::parse_attachment_content(&m.content).is_none() {
                                if let Some((_, b)) = edit_over.get(&m.timestamp) {
                                    return b.clone();
                                }
                            }
                            display_content(&m.content)
                        };
                        let visible: Vec<&crate::types::ChatMessage> = raw_msgs
                            .iter()
                            .filter(|m| chat_row_visible(raw_msgs, m))
                            .collect();
                        let n = visible.len();
                        // Stream entry #0 (avatar + name + optional status) is the oldest item: its height joins content_h so scrolling to genesis reveals it above message 1. Unconditional — every conversation has entry #0.
                        let header_block_h = avatar_r * 2.0
                            + unit * 3.0
                            + if status_in_stream.is_some() {
                                unit * 1.0
                            } else {
                                0.0
                            };
                        // Details-strip selection for THIS conversation (identity-keyed): one strip line joins content_h so the stream shifts to make room rather than overdrawing a neighbour row.
                        let sel_key = self
                            .selected_msg
                            .filter(|(sci, _, _)| *sci == ci)
                            .map(|(_, ts, out)| (ts, out));
                        let detail_h = line_h * 3.0; // three strip lines: meta (sent/age/state), the action row (reply · edit · copy · resend · delete), and the reaction row (ranked glyphs + the circled "+")
                        let sel_in_stream = sel_key.is_some_and(|(ts, out)| {
                            visible
                                .iter()
                                .any(|m| m.timestamp == ts && m.is_outgoing == out)
                        });
                        // WORD-WRAP: messages wrap to the pane width instead of trailing off-screen. Metrics style matches the draw style below minus colour (colour never changes glyph widths). `intra` = spacing between a message's own wrapped lines; the inter-message gap stays line_h on the last line, so single-line spacing is pixel-identical to the pre-wrap layout. The line-count CACHE (see msg_wrap) covers all of history for content_h; drawn messages re-wrap for their actual strings.
                        let wrap_style = TextStyle::new(msg_size, 0).weight(500);
                        let avail_w = (buf_w as f32 - pad_x * 2.0).max(msg_size);
                        let intra = msg_size * 1.25;
                        let wrap_key =
                            (ci, n, raw_msgs.len(), avail_w.to_bits(), msg_size.to_bits());
                        if self.msg_wrap.as_ref().map(|(k, _, _)| *k) != Some(wrap_key) {
                            let mut all_lines: Vec<Vec<String>> = Vec::with_capacity(n);
                            let mut total = 0usize;
                            for m in &visible {
                                let lines =
                                    wrap_text_lines(ctx.text, &body_of(m), &wrap_style, avail_w);
                                total += lines.len();
                                // A reply row reserves ONE extra line for its half-alpha reference snippet above the body.
                                if matches!(m.reference, Some((crate::types::RefKind::Reply, _))) {
                                    total += 1;
                                }
                                // A reacted row reserves ONE extra line for its reaction glyphs below the body.
                                if react_line(m.timestamp).is_some() {
                                    total += 1;
                                }
                                all_lines.push(lines);
                            }
                            self.msg_wrap = Some((wrap_key, all_lines, total));
                        }
                        let total_lines = self.msg_wrap.as_ref().map(|(_, _, t)| *t).unwrap_or(n);
                        let content_h = n as f32 * line_h
                            + (total_lines.saturating_sub(n)) as f32 * intra
                            + header_block_h
                            + if sel_in_stream { detail_h } else { 0.0 };
                        let view_h = (list_bottom - list_top).max(0.0);
                        let max_scroll = (content_h - view_h).max(0.0);
                        // Publish the ceiling so the tick can clamp the STORED offset (this field write is disjoint from the `contact` borrow above); the local `scroll` only fixes THIS frame's draw.
                        self.msg_max_scroll = max_scroll;
                        self.msg_view_h = view_h;
                        let scroll = conv
                            .map(|v| v.scroll_offset)
                            .unwrap_or(0.0)
                            .clamp(0.0, max_scroll);
                        self.msg_hit_rows.clear();
                        // TOP-ANCHOR while the conversation fits the view: the stream reads avatar/name → msg 1 → msg 2 from the top, ONE strip — bottom-anchoring a short history floated the header block mid-screen above a clump of bottom messages ("rendered in a different layer"). Once content outgrows the view the min() saturates and the classic newest-at-bottom anchor takes over seamlessly.
                        let mut y = (list_top + content_h).min(list_bottom) - msg_size + scroll;
                        // Whether the walk reached the conversation's FIRST message (no early break): the scroll-top avatar/name block may only draw then — drawing it at the break position floated it mid-stream over recent messages in any long conversation ("the avatar and name are rendered in a different block").
                        let mut reached_oldest = true;
                        // Hold the cached wrap strings for the whole walk (disjoint field from everything the loop mutates).
                        let wrap_cache: &Vec<Vec<String>> =
                            &self.msg_wrap.as_ref().expect("wrap cache built above").1;
                        for (vi, msg) in visible.iter().enumerate().rev() {
                            if y < list_top - line_h {
                                reached_oldest = false;
                                break; // this block's BOTTOM line is above the visible region; wrapped lines extend upward, and older messages sit higher still
                            }
                            // Cached wrapped lines — scroll frames do zero shaping. `y` is the LAST line's baseline, earlier lines stack upward at `intra` spacing.
                            static EMPTY_LINES: Vec<String> = Vec::new();
                            let lines: &Vec<String> = wrap_cache.get(vi).unwrap_or(&EMPTY_LINES);
                            // A reply row's reference snippet occupies one extra line ABOVE the body; a reacted row grows one BELOW (both counted into the wrap total). The reaction line sits at the block's bottom baseline, so the body shifts up by react_off.
                            let reply_target = msg.reference.and_then(|(k, t)| {
                                (k == crate::types::RefKind::Reply).then_some(t)
                            });
                            let reactions = react_line(msg.timestamp);
                            let react_off = if reactions.is_some() { intra } else { 0.0 };
                            let block_extra = (lines.len() as f32 - 1.0) * intra
                                + if reply_target.is_some() { intra } else { 0.0 }
                                + react_off;
                            // Attachment transfer progress: a thin fill under the pill while a matching PT transfer runs (outbound for our un-confirmed sends, inbound for blobs we're missing). Matched loosely by direction — the throttled snapshot only ever contains big sharded transfers.
                            if let Some((hash, _, _)) =
                                crate::types::parse_attachment_content(&msg.content)
                            {
                                let want_outbound = msg.is_outgoing;
                                let relevant = if want_outbound {
                                    !self.attach_confirmed.contains(&hash)
                                } else {
                                    !crate::storage::blob_present(&hash)
                                };
                                if relevant {
                                    if let Some((_, done, total, _)) = self
                                        .attach_progress
                                        .iter()
                                        .find(|(_, _, _, ob)| *ob == want_outbound)
                                    {
                                        let frac =
                                            (*done as f32 / (*total).max(1) as f32).clamp(0.0, 1.0);
                                        let bar_w = (buf_w as f32 - pad_x * 2.0) * frac;
                                        let (bx, bw) = if msg.is_outgoing {
                                            (
                                                (buf_w as f32 - pad_x - bar_w) as isize,
                                                bar_w as isize,
                                            )
                                        } else {
                                            (pad_x as isize, bar_w as isize)
                                        };
                                        paint::fill_rect(
                                            &mut canvas,
                                            bx,
                                            (y + msg_size * 0.55) as isize,
                                            bw,
                                            (ru.max(1.0) * 2.0) as isize,
                                            *theme::PROGRESS_FILL,
                                            Some(list_clip),
                                            None,
                                        );
                                    }
                                }
                            }
                            // Divider under this message (between it and the next-newer one).
                            // Full-bleed divider at the version-watermark treatment: pure white, α=1/8 (VERSION_COLOUR is exactly that, and darkness-0 white is channel-order invariant). Positioned at the MIDPOINT of the inter-message gap (0.8·msg_size below the baseline centre): at 0.5 it sat flush against the descenders — good padding above, none below.
                            paint::fill_rect(
                                &mut canvas,
                                0,
                                (y + msg_size * 0.8) as isize,
                                buf_w as isize,
                                (ru.max(1.0)) as isize,
                                theme::VERSION_COLOUR,
                                Some(list_clip),
                                None,
                            );
                            // Details strip for the SELECTED message: occupies this slot (directly under the message, above the newer row + divider); the message itself shifts up by detail_h. Direction + age + delivery on the left, the copy pill on the right (stamped msg_copy_id).
                            if sel_key.is_some_and(|(ts, out)| {
                                msg.timestamp == ts && msg.is_outgoing == out
                            }) {
                                let secs = ((vsf::eagle_time_oscillations() - msg.timestamp)
                                    / crate::OSC_PER_SEC)
                                    .max(0);
                                let age = if secs >= 86400 {
                                    format!("{}d ago", secs / 86400)
                                } else if secs >= 3600 {
                                    format!("{}h ago", secs / 3600)
                                } else if secs >= 60 {
                                    format!("{}m ago", secs / 60)
                                } else {
                                    format!("{}s ago", secs)
                                };
                                let mut detail = if msg.is_outgoing {
                                    format!(
                                        "sent · {} · {}",
                                        age,
                                        if msg.delivered {
                                            "delivered"
                                        } else {
                                            "sending"
                                        }
                                    )
                                } else {
                                    format!("received · {}", age)
                                };
                                if msg.recovered {
                                    detail.push_str(" · recovered");
                                }
                                if crate::types::parse_attachment_content(&msg.content).is_none()
                                    && edit_over.contains_key(&msg.timestamp)
                                {
                                    detail.push_str(" \u{00b7} edited");
                                }
                                // Attachment blob state joins the meta line: held/confirmed vs still travelling.
                                if let Some((hash, _, _)) =
                                    crate::types::parse_attachment_content(&msg.content)
                                {
                                    if msg.is_outgoing {
                                        detail.push_str(if self.attach_confirmed.contains(&hash) {
                                            " · blob delivered"
                                        } else {
                                            " · blob sending"
                                        });
                                    } else if !crate::storage::blob_present(&hash) {
                                        detail.push_str(" · blob not here yet");
                                    }
                                }
                                let detail_size = msg_size * 0.75;
                                let detail_style =
                                    TextStyle::new(detail_size, *theme::LABEL_COLOUR)
                                        .weight(500)
                                        .font("Oxanium");
                                // Reaction attribution joins the meta line — whose glyph is whose lives here, not on the bubble.
                                if let Some(slots) = react_over.get(&msg.timestamp) {
                                    if let Some(g) =
                                        slots[0].as_ref().map(|(_, g)| g).filter(|g| !g.is_empty())
                                    {
                                        detail.push_str(&format!(" \u{00b7} they {}", g));
                                    }
                                    if let Some(g) =
                                        slots[1].as_ref().map(|(_, g)| g).filter(|g| !g.is_empty())
                                    {
                                        detail.push_str(&format!(" \u{00b7} you {}", g));
                                    }
                                }
                                // Upper strip line: the meta text.
                                ctx.text.draw_text_left(
                                    &mut canvas,
                                    &detail,
                                    pad_x,
                                    y - line_h * 2.0,
                                    &detail_style,
                                    Some(list_clip),
                                    None,
                                );
                                // Lower strip line: the ACTION ROW — reply · edit · copy/copied · resend · delete. Conditional pills: edit only for outgoing (stub until the message-format rework), resend only for undelivered outgoing (manual re-fire on the chain), delete always (LOCAL until tombstones — fleet sync may resurrect it), reply always. Each pill stamps its own hit id with generous padding.
                                let (copy_label, copy_colour) = if self.selected_msg_copied {
                                    ("copied", *theme::SEARCH_FOUND_COLOUR)
                                } else {
                                    ("copy", *theme::COPY_PILL_COLOUR)
                                };
                                let mut pills: Vec<(&str, u32, HitId)> =
                                    vec![("reply", *theme::COPY_PILL_COLOUR, self.msg_action_base)];
                                if msg.is_outgoing
                                    && crate::types::parse_attachment_content(&msg.content)
                                        .is_none()
                                {
                                    pills.push((
                                        "edit",
                                        *theme::COPY_PILL_COLOUR,
                                        self.msg_action_base.wrapping_add(1),
                                    ));
                                }
                                pills.push((copy_label, copy_colour, self.msg_copy_id));
                                if msg.is_outgoing && !msg.delivered {
                                    pills.push((
                                        "resend",
                                        *theme::HOURGLASS_COLOUR,
                                        self.msg_action_base.wrapping_add(2),
                                    ));
                                }
                                // Attachment rows: save (blob held) or fetch (blob missing — ask friend + siblings for it).
                                if let Some((hash, _, _)) =
                                    crate::types::parse_attachment_content(&msg.content)
                                {
                                    if crate::storage::blob_present(&hash) {
                                        pills.push((
                                            "save",
                                            *theme::SEARCH_FOUND_COLOUR,
                                            self.msg_action_base.wrapping_add(4),
                                        ));
                                    } else {
                                        pills.push((
                                            "fetch",
                                            *theme::HOURGLASS_COLOUR,
                                            self.msg_action_base.wrapping_add(4),
                                        ));
                                    }
                                }
                                let deleting =
                                    self.pending_delete.as_ref().is_some_and(|(k, _)| {
                                        *k == (ci, msg.timestamp, msg.is_outgoing)
                                    });
                                pills.push((
                                    if deleting {
                                        "deleting\u{2026}"
                                    } else {
                                        "delete"
                                    },
                                    *theme::ERROR_TEXT_COLOUR,
                                    self.msg_action_base.wrapping_add(3),
                                ));
                                if deleting {
                                    // The feedback frame is on screen — the tick may do the heavy lift now.
                                    if let Some((_, painted)) = self.pending_delete.as_mut() {
                                        *painted = true;
                                    }
                                }
                                let pad_hit = detail_size;
                                let mut px_cursor = pad_x;
                                for (label, colour, hid) in pills {
                                    let style = TextStyle::new(detail_size, colour)
                                        .weight(600)
                                        .font("Oxanium");
                                    let w = ctx.text.measure_text(label, &style);
                                    ctx.text.draw_text_left(
                                        &mut canvas,
                                        label,
                                        px_cursor,
                                        y - line_h,
                                        &style,
                                        Some(list_clip),
                                        None,
                                    );
                                    restamp_hit_rect(
                                        &mut chrome.hit_test_map,
                                        buf_w,
                                        buf_h,
                                        (px_cursor - pad_hit * 0.5) as isize,
                                        ((y - line_h * 1.5).max(list_top)) as isize,
                                        (px_cursor + w + pad_hit * 0.5) as isize,
                                        ((y - line_h * 0.5).min(list_bottom)) as isize,
                                        hid,
                                    );
                                    px_cursor += w + pad_hit * 2.0;
                                }
                                // Bottom strip line: the REACTION ROW — as many ranked glyphs as fit, our current one highlighted green (tap it again to retract; tap another to replace), then the circled "+" for anything the keyboard can type. Drawn order is snapshotted so the tap handler maps slot → glyph even as the ranking shifts.
                                let ours_now: Option<String> = raw_msgs
                                    .iter()
                                    .rev()
                                    .filter(|m| !m.deleted && m.is_outgoing)
                                    .find(|m| {
                                        m.reference
                                            == Some((crate::types::RefKind::React, msg.timestamp))
                                    })
                                    .map(|m| m.content.clone())
                                    .filter(|g| !g.is_empty());
                                let glyph_size = detail_size * 1.2;
                                let plus_r = glyph_size * 0.62;
                                let mut rx_cursor = pad_x;
                                self.react_strip_glyphs.clear();
                                for g in ranked_reactions.iter() {
                                    if self.react_strip_glyphs.len() >= 9 {
                                        break;
                                    }
                                    let style = TextStyle::new(
                                        glyph_size,
                                        if ours_now.as_deref() == Some(g.as_str()) {
                                            *theme::SEARCH_FOUND_COLOUR
                                        } else {
                                            *theme::COPY_PILL_COLOUR
                                        },
                                    )
                                    .weight(500);
                                    let w = ctx.text.measure_text(g, &style);
                                    // Fit gate: always leave room for the circled "+" at the row's end.
                                    if rx_cursor + w + pad_hit + plus_r * 2.0 + pad_hit
                                        > buf_w as f32 - pad_x
                                    {
                                        break;
                                    }
                                    ctx.text.draw_text_left(
                                        &mut canvas,
                                        g,
                                        rx_cursor,
                                        y,
                                        &style,
                                        Some(list_clip),
                                        None,
                                    );
                                    restamp_hit_rect(
                                        &mut chrome.hit_test_map,
                                        buf_w,
                                        buf_h,
                                        (rx_cursor - pad_hit * 0.4) as isize,
                                        ((y - line_h * 0.5).max(list_top)) as isize,
                                        (rx_cursor + w + pad_hit * 0.4) as isize,
                                        ((y + line_h * 0.5).min(list_bottom)) as isize,
                                        self.react_strip_base
                                            .wrapping_add(self.react_strip_glyphs.len() as HitId),
                                    );
                                    self.react_strip_glyphs.push(g.clone());
                                    rx_cursor += w + pad_hit;
                                }
                                // The circled "+": react with ANYTHING — arms the compose box, the system keyboard is the picker.
                                let plus_cx = rx_cursor + plus_r;
                                let plus_cy = y - glyph_size * 0.32;
                                paint::draw_circle(
                                    &mut canvas,
                                    plus_cx,
                                    plus_cy,
                                    plus_r,
                                    *theme::COPY_PILL_COLOUR,
                                    Some(list_clip),
                                );
                                let plus_style =
                                    TextStyle::new(glyph_size, *theme::COPY_PILL_COLOUR)
                                        .weight(500);
                                let plus_w = ctx.text.measure_text("+", &plus_style);
                                ctx.text.draw_text_left(
                                    &mut canvas,
                                    "+",
                                    plus_cx - plus_w * 0.5,
                                    y,
                                    &plus_style,
                                    Some(list_clip),
                                    None,
                                );
                                restamp_hit_rect(
                                    &mut chrome.hit_test_map,
                                    buf_w,
                                    buf_h,
                                    (plus_cx - plus_r - pad_hit * 0.4) as isize,
                                    ((y - line_h * 0.5).max(list_top)) as isize,
                                    (plus_cx + plus_r + pad_hit * 0.4) as isize,
                                    ((y + line_h * 0.5).min(list_bottom)) as isize,
                                    self.react_strip_base.wrapping_add(9),
                                );
                                y -= detail_h;
                            }
                            // Dim outgoing until delivered; incoming always full. Self-as-contact: every message is ours (there is no other party), so everything sits on the right in the neutral grey — their_colour is already the anchor in that case, and the loopback "incoming" copy renders like a delivered outgoing.
                            let colour = if msg.is_outgoing {
                                if msg.delivered {
                                    our_colour
                                } else {
                                    theme::dim_colour(our_colour)
                                }
                            } else {
                                their_colour
                            };
                            let msg_style = TextStyle::new(msg_size, colour).weight(500);
                            // The referenced message, resolved LIVE (so its own edits show) at HALF alpha in the REPLIER'S colour — the whole reply block is one party's utterance, so its reference line tints like its body (field call, 2026-08-09: the target-colour scheme made a friend's reply-to-us carry a grey reference, since our own colour is the neutral grey). Half vs full separates context from content; quarter stays the not-yet-ACKed signal. Missing target (not synced yet) renders as a bare ellipsis.
                            if let Some(t) = reply_target {
                                let ref_text = raw_msgs
                                    .iter()
                                    .find(|x| {
                                        x.timestamp == t
                                            && !x.deleted
                                            && !crate::types::is_control_content(&x.content)
                                            && !matches!(
                                                x.reference,
                                                Some((crate::types::RefKind::Edit, _))
                                            )
                                    })
                                    .map(|x| {
                                        // VERBATIM bytes, truncated only — no flatten, no substitution (the sender's newlines are the sender's message; the shaper renders them however it renders them).
                                        let d = body_of(x);
                                        let mut s: String = d.chars().take(48).collect();
                                        if d.chars().count() > 48 {
                                            s.push('\u{2026}');
                                        }
                                        format!("\u{00bb} {}", s)
                                    })
                                    .unwrap_or("\u{00bb} \u{2026}".to_string());
                                let ref_colour =
                                    theme::half_colour(if msg.is_outgoing || is_self_contact {
                                        our_colour
                                    } else {
                                        their_colour
                                    });
                                let ref_style = TextStyle::new(msg_size, ref_colour).weight(500);
                                let ref_y = y - react_off - lines.len() as f32 * intra;
                                if msg.is_outgoing || is_self_contact {
                                    ctx.text.draw_text_right(
                                        &mut canvas,
                                        &ref_text,
                                        buf_w as f32 - pad_x,
                                        ref_y,
                                        &ref_style,
                                        Some(list_clip),
                                        None,
                                    );
                                } else {
                                    ctx.text.draw_text_left(
                                        &mut canvas,
                                        &ref_text,
                                        pad_x,
                                        ref_y,
                                        &ref_style,
                                        Some(list_clip),
                                        None,
                                    );
                                }
                            }
                            for (k, line) in lines.iter().enumerate() {
                                let ly = y - react_off - (lines.len() - 1 - k) as f32 * intra;
                                if msg.is_outgoing || is_self_contact {
                                    ctx.text.draw_text_right(
                                        &mut canvas,
                                        line,
                                        buf_w as f32 - pad_x,
                                        ly,
                                        &msg_style,
                                        Some(list_clip),
                                        None,
                                    );
                                } else {
                                    ctx.text.draw_text_left(
                                        &mut canvas,
                                        line,
                                        pad_x,
                                        ly,
                                        &msg_style,
                                        Some(list_clip),
                                        None,
                                    );
                                }
                            }
                            // The reaction line: theirs then ours under the bubble, EACH GLYPH IN ITS REACTOR'S COLOUR at half alpha (the reference treatment — and the emoji rasterize thru the style tint, so grey made every reaction read as nobody's). Bubble-side aligned; attribution words live in the details strip meta.
                            if let Some((r_theirs, r_ours)) = reactions.as_ref() {
                                let r_sz = msg_size * 0.8;
                                let r_gap = r_sz * 0.6;
                                let their_style =
                                    TextStyle::new(r_sz, theme::half_colour(their_colour))
                                        .weight(500);
                                let our_style =
                                    TextStyle::new(r_sz, theme::half_colour(our_colour))
                                        .weight(500);
                                if msg.is_outgoing || is_self_contact {
                                    let mut right_x = buf_w as f32 - pad_x;
                                    if let Some(o) = r_ours {
                                        ctx.text.draw_text_right(
                                            &mut canvas,
                                            o,
                                            right_x,
                                            y,
                                            &our_style,
                                            Some(list_clip),
                                            None,
                                        );
                                        right_x -= ctx.text.measure_text(o, &our_style) + r_gap;
                                    }
                                    if let Some(t) = r_theirs {
                                        ctx.text.draw_text_right(
                                            &mut canvas,
                                            t,
                                            right_x,
                                            y,
                                            &their_style,
                                            Some(list_clip),
                                            None,
                                        );
                                    }
                                } else {
                                    let mut left_x = pad_x;
                                    if let Some(t) = r_theirs {
                                        ctx.text.draw_text_left(
                                            &mut canvas,
                                            t,
                                            left_x,
                                            y,
                                            &their_style,
                                            Some(list_clip),
                                            None,
                                        );
                                        left_x += ctx.text.measure_text(t, &their_style) + r_gap;
                                    }
                                    if let Some(o) = r_ours {
                                        ctx.text.draw_text_left(
                                            &mut canvas,
                                            o,
                                            left_x,
                                            y,
                                            &our_style,
                                            Some(list_clip),
                                            None,
                                        );
                                    }
                                }
                            }
                            // Stamp the row band — the WHOLE wrapped block — so a tap selects this message (details strip). Clamped to the list region so header/compose never lose their own hits; capped at the 64-id span (a taller screen than that doesn't exist).
                            if self.msg_hit_rows.len() < 64 {
                                let row_hit = self
                                    .msg_hit_base
                                    .wrapping_add(self.msg_hit_rows.len() as HitId);
                                restamp_hit_rect(
                                    &mut chrome.hit_test_map,
                                    buf_w,
                                    buf_h,
                                    0,
                                    ((y - block_extra - line_h * 0.5).max(list_top)) as isize,
                                    buf_w as isize,
                                    ((y + line_h * 0.5).min(list_bottom)) as isize,
                                    row_hit,
                                );
                                // A reply row's reference line is its own tap target: the band + the referenced ts ride the hit row, and a tap inside it JUMPS to the source row instead of opening the strip.
                                let ref_band = reply_target.map(|t| {
                                    let ref_y = y - react_off - lines.len() as f32 * intra;
                                    (ref_y - line_h * 0.5, ref_y + line_h * 0.5, t)
                                });
                                self.msg_hit_rows
                                    .push((msg.timestamp, msg.is_outgoing, ref_band));
                            }
                            y -= line_h + block_extra;
                        }
                        // STREAM ENTRY #0 — avatar, name, optional ceremony/lifecycle status: drawn ONLY when the walk reached message 1 (genesis on screen); `y` then sits just above it and the entry is the stream's literal first item. Ordinary stream content: same clip as every message, no pinning, no slide. Off-screen anywhere but genesis.
                        if reached_oldest && y > list_top - header_block_h - line_h {
                            let (block_status, status_h) = match &status_in_stream {
                                Some((label, colour)) => {
                                    (Some((label.clone(), *colour)), unit * 1.0)
                                }
                                None => (None, 0.0),
                            };
                            let block_name_y = y - unit * 0.2 - status_h;
                            let block_avatar_cy = block_name_y - unit * 1.2 - avatar_r;
                            draw_conv_avatar(&mut canvas, block_avatar_cy, Some(list_clip));
                            ctx.text.draw_text_center(
                                &mut canvas,
                                &contact.display_name_or_pending(),
                                buf_w as f32 * 0.5,
                                block_name_y,
                                &header_style,
                                Some(list_clip),
                                None,
                            );
                            if let Some((label, colour)) = block_status {
                                ctx.text.draw_text_center(
                                    &mut canvas,
                                    &label,
                                    buf_w as f32 * 0.5,
                                    y - unit * 0.2,
                                    &TextStyle::new(unit * 0.6, colour)
                                        .weight(500)
                                        .font("Oxanium"),
                                    Some(list_clip),
                                    None,
                                );
                            }
                        }
                        let _ = n;

                        // ── Armed reply/edit strip: the referenced message at HALF alpha (its sender's colour), in the reserved band. While editing, the strip shows what the row says NOW — the box holds the correction, so the pair reads as a before/after diff.
                        if let Some((t, strip_kind)) = compose_strip {
                            let target = raw_msgs.iter().find(|x| {
                                x.timestamp == t
                                    && !x.deleted
                                    && !crate::types::is_control_content(&x.content)
                                    && !matches!(
                                        x.reference,
                                        Some((crate::types::RefKind::Edit, _))
                                    )
                            });
                            let snippet = target
                                .map(|x| {
                                    // VERBATIM bytes, truncated only — no flatten, no substitution.
                                    let d = body_of(x);
                                    let mut s: String = d.chars().take(56).collect();
                                    if d.chars().count() > 56 {
                                        s.push('\u{2026}');
                                    }
                                    s
                                })
                                .unwrap_or("\u{2026}".to_string());
                            let col = target
                                .map(|x| {
                                    if x.is_outgoing || is_self_contact {
                                        our_colour
                                    } else {
                                        their_colour
                                    }
                                })
                                .unwrap_or(*theme::LABEL_COLOUR);
                            let text = match strip_kind {
                                1 => format!("editing \u{00bb} {}", snippet),
                                2 => format!("react \u{00bb} {}", snippet),
                                _ => format!("\u{00bb} {}", snippet),
                            };
                            ctx.text.draw_text_left(
                                &mut canvas,
                                &text,
                                pad_x,
                                list_bottom + unit * 0.55,
                                &TextStyle::new(msg_size * 0.85, theme::half_colour(col))
                                    .weight(500),
                                None,
                                None,
                            );
                        }
                        // ── BRIDGE locus strip: `host:cwd` in the reserved band, dim — the terminal finally says where it stands (a real prompt's job; field 2026-08-23: a pull meant for photon ran in keys/). While a command runs, the Stop pill sits at the band's right edge — the operator's lever, escalating INT → TERM → KILL per press. An armed reply/edit strip wins the band for its moment.
                        if compose_strip.is_none() && (bridge_strip_txt.is_some() || bridge_inflight)
                        {
                            let strip_y = list_bottom + unit * 0.55;
                            if let Some(loc) = &bridge_strip_txt {
                                ctx.text.draw_text_left(
                                    &mut canvas,
                                    loc,
                                    pad_x,
                                    strip_y,
                                    &TextStyle::new(
                                        msg_size * 0.8,
                                        theme::half_colour(*theme::LABEL_COLOUR),
                                    )
                                    .weight(500),
                                    None,
                                    None,
                                );
                            }
                            if bridge_inflight {
                                let label = "stop";
                                let style =
                                    TextStyle::new(msg_size * 0.85, *theme::ERROR_TEXT_COLOUR)
                                        .weight(700)
                                        .font("Oxanium");
                                let w = ctx.text.measure_text(label, &style);
                                let sx = buf_w as f32 - pad_x - w;
                                ctx.text.draw_text_left(
                                    &mut canvas,
                                    label,
                                    sx,
                                    strip_y,
                                    &style,
                                    None,
                                    None,
                                );
                                restamp_hit_rect(
                                    &mut chrome.hit_test_map,
                                    buf_w,
                                    buf_h,
                                    (sx - unit * 0.5) as isize,
                                    (strip_y - unit * 0.7) as isize,
                                    (sx + w + unit * 0.5) as isize,
                                    (strip_y + unit * 0.4) as isize,
                                    self.msg_action_base.wrapping_add(5),
                                );
                            }
                        }

                        // ── Compose box (pinned bottom) ──────────────────────────── Shown when THIS device can dispatch — the pre-chrome `compose_ready` snapshot, the same one definition the focus walk reads. No placeholder text: the box's position says what it's for (Nick, 2026-08-09 — the hint lingered after sends and earned nothing).
                        if compose_ready {
                            // Send button COLOUR first (its under() blit lands on the noise), then the arrowhead over the pill (source-over). The textbox draws after — it sits over the button and clobbers the button's hit stamp with its own id — so we re-stamp the button's TRUE pill silhouette (fill + stroke, which also covers the arrowhead) AFTER the textbox, as the last writer. That's the whole click + hover region: shape-accurate, not a bbox rectangle.
                            if let Some(btn) = self.message_send_btn.as_mut() {
                                let id = btn.hit_id();
                                btn.render_content_into(
                                    &mut canvas,
                                    0.,
                                    0.,
                                    ctx.text,
                                    None,
                                    Some(&mut chrome.hit_test_map),
                                    id,
                                );
                                if self.compose_edit_of.is_some() {
                                    // EDIT armed: commit-the-correction — a green check, distinct from the send arrow by shape AND colour.
                                    draw_check_mark(
                                        &mut canvas,
                                        btn.center_x,
                                        btn.center_y,
                                        btn.height * 0.5,
                                        *theme::SEARCH_FOUND_COLOUR,
                                    );
                                } else {
                                    // THE PROMPT GATE, visually: a bridge command in flight dims the arrow (submit_message refuses the send until the final lands), same as a real terminal withholding its prompt. Friend conversations never gate. (bridge_held is the pre-chrome snapshot.)
                                    let arrow = if bridge_held {
                                        *theme::LABEL_COLOUR
                                    } else {
                                        *theme::SEND_ARROW_COLOUR
                                    };
                                    draw_up_arrowhead(
                                        &mut canvas,
                                        btn.center_x,
                                        btn.center_y,
                                        btn.height * 0.5,
                                        arrow,
                                    );
                                }
                            }
                            if let Some(tb) = self.message_textbox.as_mut() {
                                let id = tb.hit_id();
                                tb.render_content_into(
                                    &mut canvas,
                                    0.,
                                    0.,
                                    ctx.text,
                                    None,
                                    Some(&mut chrome.hit_test_map),
                                    id,
                                );
                            }
                            // Re-win the send button's hit silhouette after the textbox clobbered it.
                            if let Some(btn) = self.message_send_btn.as_ref() {
                                btn.stamp_hit_into(
                                    &mut chrome.hit_test_map,
                                    buf_w,
                                    buf_h,
                                    btn.hit_id(),
                                );
                            }
                        } // end chain-woven compose gate
                    } // end CLUTCH-Complete gate (message list + compose box)
                }
            }
        }

        // ── Add-device screen: this (existing) device shows the pairing secret words to type into the new device. ──
        if matches!(self.state, AppState::AddDevice) {
            let mut canvas = Canvas::new(target, buf_w, buf_h, ctx.damage);
            let cx = buf_w as f32 * 0.5;

            // Back affordance (top-left) — same "‹ Contacts" idiom + hit-id as the Conversation screen. Navigation is a dedicated control; the orb is reserved for settings and never carries context actions.
            {
                let unit = ReadyLayout::compute(buf_w, buf_h, ctx.viewport.ru).unit_height;
                let back_y = buf_h as f32 * 0.06 + unit;
                let back_size = unit * 1.15;
                let back_text = "‹ Contacts";
                ctx.text.draw_text_left(
                    &mut canvas,
                    back_text,
                    unit,
                    back_y,
                    &TextStyle::new(back_size, *theme::CONTACT_NAME_COLOUR)
                        .weight(500)
                        .font("Oxanium"),
                    None,
                    None,
                );
                let back_w = ctx.text.measure_text(
                    back_text,
                    &TextStyle::new(back_size, 0).weight(500).font("Oxanium"),
                );
                restamp_hit_rect(
                    &mut chrome.hit_test_map,
                    buf_w,
                    buf_h,
                    0,
                    (back_y - back_size) as isize,
                    (unit + back_w + unit) as isize,
                    (back_y + back_size) as isize,
                    self.back_btn_hit_id,
                );
            }

            // All geometry hangs off the textbox rect (laid out by update_widget_layout from the ru-scaled attest slot), so the whole screen scales with zoom and nothing collides with the pill.
            let (tb_cy, tb_h) = self
                .textbox
                .as_ref()
                .map(|tb| (tb.center_y, tb.font_size / 0.75))
                .unwrap_or((buf_h as f32 * 0.45, 40.0));
            // ONE vertical RU block: every element is a stacked row of unit `u = tb_h`, positioned by a running top-edge cursor `y` so nothing tramples at any zoom or candidate count. The field/confirm slot stays at the textbox rect (tb_cy); the block is anchored so that slot lands in place, and title/subtitle flow above it, counter/list/status/hint below.
            let u = tb_h;
            let gap = u * 0.45;
            // Two header rows above the field.
            ctx.text.draw_text_center(
                &mut canvas,
                "Add a device",
                cx,
                tb_cy - u * 2.5,
                &TextStyle::new(u * 0.85, *theme::STATUS_TEXT_COLOUR)
                    .weight(600)
                    .font("Oxanium"),
                None,
                None,
            );
            let subtitle = if self.add_device_bound.is_none() {
                "Type the words shown on the new device"
            } else if self.add_device_checking {
                "" // Words path: bound + auto-rotating; the status row below carries "Adding…".
            } else {
                // BLE/tap path only: load-bearing — the human must check the FAR (new) device's screen, not this one.
                "Confirm only once the new device shows it's in"
            };
            ctx.text.draw_text_center(
                &mut canvas,
                subtitle,
                cx,
                tb_cy - u * 1.35,
                &TextStyle::new(u * 0.45, *theme::STATUS_TEXT_COLOUR).font("Oxanium"),
                None,
                None,
            );
            // Running cursor for everything BELOW the field slot (top edge of the next row).
            let mut y = tb_cy + u * 0.85;
            if self.add_device_bound.is_none() {
                // Words-entry field (the launch textbox instance, at its rect); it stamps its hit id so click-to-focus works.
                if let Some(tb) = self.textbox.as_mut() {
                    let id = tb.hit_id();
                    tb.render_content_into(
                        &mut canvas,
                        0.,
                        0.,
                        ctx.text,
                        None,
                        None,
                        Some(&mut chrome.hit_test_map),
                        id,
                    );
                }
                // Live word counter (n / 23).
                let typed: String = self
                    .textbox
                    .as_ref()
                    .map(|tb| tb.chars.iter().collect())
                    .unwrap_or_default();
                let count = crate::network::fgtw::fleet::pair_word_tokens(&typed);
                let full = count == crate::network::fgtw::fleet::PAIR_WORD_COUNT;
                let counter = format!("{count} / {}", crate::network::fgtw::fleet::PAIR_WORD_COUNT);
                let counter_colour = if full {
                    *theme::SEARCH_FOUND_COLOUR
                } else {
                    fluor::theme::HINT_COLOUR
                };
                ctx.text.draw_text_center(
                    &mut canvas,
                    &counter,
                    cx,
                    y + u * 0.25,
                    &TextStyle::new(u * 0.5, counter_colour)
                        .weight(500)
                        .font("Oxanium"),
                    None,
                    None,
                );
                y += u * 0.5 + gap;
                // Tappable candidate list — PROXIMITY POPULATION ONLY (docs/pairing-v2.md): only devices HEARD over the BLE announce beacon (later: NFC tap) become tap targets, NEVER the raw registry — a remote attacker who holds the handle can flood the identity-gated registry, so listing registry entries as taps would fill your finger's reach with decoys. Registry = sync only (the consent a tap binds with); proximity is what a remote attacker can't fake. Not-nearby devices don't appear — you type their words (reading them off the physical screen IS the proximity check). Index i = position in the HEARD-only subset; the tap dispatch filters identically.
                let nearby: Vec<&AddCandidate> = self
                    .add_device_candidates
                    .iter()
                    .filter(|c| c.heard_ble || c.heard_lan)
                    .take(7)
                    .collect();
                if !nearby.is_empty() {
                    ctx.text.draw_text_center(
                        &mut canvas,
                        "or tap the nearby device asking to join:",
                        cx,
                        y + u * 0.2,
                        &TextStyle::new(u * 0.4, fluor::theme::HINT_COLOUR).font("Oxanium"),
                        None,
                        None,
                    );
                    y += u * 0.4 + gap * 0.5;
                    let row_h = u * 0.85;
                    for (i, cand) in nearby.iter().enumerate() {
                        let label = format!("{}   · nearby", cand.name);
                        let held = ctx.pressed_hit != HIT_NONE
                            && ctx.pressed_hit
                                == self.add_candidate_hit_base.wrapping_add(i as HitId);
                        ctx.text.draw_text_center(
                            &mut canvas,
                            &label,
                            cx,
                            y + row_h * 0.5,
                            &TextStyle::new(u * 0.55, *theme::SEARCH_FOUND_COLOUR)
                                .weight(if held { 700 } else { 500 })
                                .font("Oxanium"),
                            None,
                            None,
                        );
                        let half_w = buf_w as f32 * 0.42;
                        restamp_hit_rect(
                            &mut chrome.hit_test_map,
                            buf_w,
                            buf_h,
                            (cx - half_w) as isize,
                            y as isize,
                            (cx + half_w) as isize,
                            (y + row_h) as isize,
                            self.add_candidate_hit_base.wrapping_add(i as HitId),
                        );
                        y += row_h;
                    }
                    y += gap;
                }
            } else if !self.add_device_checking {
                // Green-confirm affordance (two-phase) — sits IN the field slot (tb_cy), the same place the words field would be. On the WORDS path the Bound handler auto-fires the rotation, so this never renders; it's the tap/BLE gate. Hit-stamped so Android taps land.
                ctx.text.draw_text_center(
                    &mut canvas,
                    "Yes, it's green \u{2014} finish",
                    cx,
                    tb_cy,
                    &TextStyle::new(u * 0.7, *theme::SEARCH_FOUND_COLOUR)
                        .weight(600)
                        .font("Oxanium"),
                    None,
                    None,
                );
                let half_w = buf_w as f32 * 0.4;
                restamp_hit_rect(
                    &mut chrome.hit_test_map,
                    buf_w,
                    buf_h,
                    (cx - half_w) as isize,
                    (tb_cy - u * 0.7) as isize,
                    (cx + half_w) as isize,
                    (tb_cy + u * 0.7) as isize,
                    self.add_confirm_hit_id,
                );
            }
            // Status row.
            if !self.add_device_status.is_empty() {
                let status_colour = if self.add_device_bound.is_some() {
                    *theme::SEARCH_FOUND_COLOUR
                } else if self.add_device_typo.is_some() {
                    *theme::ERROR_TEXT_COLOUR // live matcher hit: names the diverging word in red
                } else {
                    *theme::STATUS_TEXT_COLOUR
                };
                ctx.text.draw_text_center(
                    &mut canvas,
                    &self.add_device_status,
                    cx,
                    y + u * 0.28,
                    &TextStyle::new(u * 0.5, status_colour).font("Oxanium"),
                    None,
                    None,
                );
                y += u * 0.56 + gap;
            }
            // Cancel hint (the orb cancels; matching words bind automatically).
            ctx.text.draw_text_center(
                &mut canvas,
                "tap the orb to cancel",
                cx,
                y + u * 0.22,
                &TextStyle::new(u * 0.4, *theme::STATUS_TEXT_COLOUR).font("Oxanium"),
                None,
                None,
            );
        }

        // Settings panel (STUB) — nav rail + selected page body. Controls render but wire nothing (a checkbox may flip its own visual state; every button / dropdown / slider is inert).
        if let AppState::Settings(page) = self.state {
            let layout = SettingsLayout::compute(&ctx.viewport);
            let mut canvas = Canvas::new(target, buf_w, buf_h, ctx.damage);

            // Clear the whole settings region in the shared hit_test_map before re-stamping this frame's rail rows + pills — same reason as the launch block: immediate-mode stamps must not linger across page switches.
            restamp_hit_rect(
                &mut chrome.hit_test_map,
                buf_w,
                buf_h,
                0,
                layout.rail.y as isize,
                buf_w as isize,
                buf_h as isize,
                HIT_NONE,
            );

            // Open dropdown popup FIRST (under-blend: topmost content paints first) so it composites over everything painted after it.
            if page == SettingsPage::Appearance {
                if let Some(dd) = self.settings_theme_dropdown.as_mut() {
                    dd.render_popup_into(
                        &mut canvas,
                        ctx.text,
                        None,
                        Some(&mut chrome.hit_test_map),
                    );
                }
            }

            // Status toast ("Sending log (N KiB)…", "Log sent √", "Device removed √", ...) — the Ready screen draws `ready_toast` in its hint slot, but settings is a different AppState, so without this the toasts fired FROM settings pages (log submit, device remove) were invisible. Bottom of the content pane, painted early so under-blend keeps it above the page body; event-shown, cleared on the next interaction via clear_hints, never time-based.
            if let Some(msg) = &self.ready_toast {
                let ts = layout.unit * 0.72;
                ctx.text.draw_text_center(
                    &mut canvas,
                    msg,
                    layout.content.x + layout.content.w * 0.5,
                    layout.content.bottom() - ts,
                    &TextStyle::new(ts, *theme::SEARCH_FOUND_COLOUR)
                        .weight(600)
                        .font("Oxanium"),
                    None,
                    None,
                );
            }

            // --- Header: title, centered ON the rail|content divider hairline (1/3 width) — it caps the column split rather than floating at the far-left edge. ---
            let hspan = (layout.unit * 1.05).min(layout.header.h * 0.72);
            ctx.text.draw_text_center(
                &mut canvas,
                "Settings",
                layout.content.x,
                layout.header.center_y(),
                &TextStyle::new(hspan, *theme::CONTACT_NAME_COLOUR)
                    .weight(600)
                    .font("Oxanium"),
                None,
                None,
            );
            // --- Nav rail: Back is PINNED at the top (never scrolls — you never have to scroll up to go back); the nine page labels scroll BELOW it. Natural row height, no clamp-to-fit. Fills are painted AFTER the label so, under the settings pane's topmost-first (under-blend) compositing, the text sits in FRONT of the fill. ---
            let rail_inset = layout.rail_inset();
            let nav_h = layout.nav_row_h();
            let rspan = layout.unit * 0.58;
            // Pinned Back row at the very top of the rail.
            {
                let r = fluor::region::Region::new(rail_inset.x, rail_inset.y, rail_inset.w, nav_h);
                let back_held =
                    ctx.pressed_hit != HIT_NONE && ctx.pressed_hit == self.back_btn_hit_id;
                // Text FIRST (topmost-first → in front), THEN the fill behind it. 50%-black (α = 0x80) in darkness space is 0x80_FF_FF_FF (visible black is 0xFFFFFF in the RGB bytes); brighter when held.
                ctx.text.draw_text_left(
                    &mut canvas,
                    "‹ Back",
                    r.x + rspan * 0.6,
                    r.center_y(),
                    &TextStyle::new(rspan, *theme::SEARCH_FOUND_COLOUR)
                        .weight(600)
                        .font("Oxanium"),
                    None,
                    None,
                );
                let fill = if back_held {
                    fluor::theme::BUTTON_HELD
                } else {
                    theme::BACK_BUTTON_IDLE_FILL
                };
                paint::fill_rect(
                    &mut canvas,
                    r.x as isize,
                    r.y as isize,
                    r.w as isize,
                    r.h as isize,
                    fill,
                    None,
                    None,
                );
                restamp_hit_rect(
                    &mut chrome.hit_test_map,
                    buf_w,
                    buf_h,
                    r.x as isize,
                    r.y as isize,
                    r.right() as isize,
                    r.bottom() as isize,
                    self.back_btn_hit_id,
                );
            }
            // The page rows scroll within the region BELOW the pinned Back row — clipped so a scrolled row never paints over Back.
            let pages_top = rail_inset.y + nav_h;
            let pages_clip = fluor::paint::Clip::new(
                layout.rail.x.max(0.0) as usize,
                pages_top.max(layout.rail.y).max(0.0) as usize,
                layout.rail.right().max(0.0) as usize,
                layout.rail.bottom().max(0.0) as usize,
            );
            for (i, p) in settings_pages.iter().enumerate() {
                let r = fluor::region::Region::new(
                    rail_inset.x,
                    pages_top - settings_rail_scroll + i as Coord * nav_h,
                    rail_inset.w,
                    nav_h,
                );
                // Skip rows scrolled fully out of the page-scroll region.
                if r.bottom() <= pages_top || r.y >= layout.rail.bottom() {
                    continue;
                }
                let active = *p == page;
                let held = ctx.pressed_hit != HIT_NONE
                    && ctx.pressed_hit == self.settings_nav_base.wrapping_add(i as HitId);
                let colour = if active {
                    *theme::CONTACT_NAME_COLOUR
                } else {
                    *theme::LABEL_COLOUR
                };
                // Label FIRST (in front), then the highlight fill behind it.
                ctx.text.draw_text_left(
                    &mut canvas,
                    p.label(),
                    r.x + rspan * 0.6,
                    r.center_y(),
                    &TextStyle::new(rspan, colour)
                        .weight(if active { 600 } else { 400 })
                        .font("Oxanium"),
                    Some(pages_clip),
                    None,
                );
                if held {
                    // Held (pointer down, release switches to this page) reads brightest.
                    paint::fill_rect(
                        &mut canvas,
                        r.x as isize,
                        r.y as isize,
                        r.w as isize,
                        r.h as isize,
                        fluor::theme::BUTTON_HELD,
                        Some(pages_clip),
                        None,
                    );
                } else if active {
                    // Active-row backing bar (faint) so the selected page reads at a glance.
                    paint::fill_rect(
                        &mut canvas,
                        r.x as isize,
                        r.y as isize,
                        r.w as isize,
                        r.h as isize,
                        theme::SEPARATOR_COLOUR,
                        Some(pages_clip),
                        None,
                    );
                }
                restamp_hit_rect(
                    &mut chrome.hit_test_map,
                    buf_w,
                    buf_h,
                    r.x as isize,
                    r.y.max(pages_top) as isize,
                    r.right() as isize,
                    r.bottom().min(layout.rail.bottom()) as isize,
                    self.settings_nav_base.wrapping_add(i as HitId),
                );
            }

            // Hairline between rail and content.
            paint::fill_rect(
                &mut canvas,
                layout.content.x as isize,
                layout.content.y as isize,
                1,
                layout.content.h as isize,
                theme::SEPARATOR_COLOUR,
                None,
                None,
            );

            // --- Selected page body ---
            // (page body is computed per-arm as a scrolled, natural-height region — see `layout.content_scrolled`) Everything sizes off layout.unit — the ONE span·ru harmonic unit — so text, pills, rows, and controls all scale together with window shape AND zoom. (The old mix — text × ru inside fixed rows, controls off bare region fractions — is what made zoom hit-or-miss.)
            let tspan = layout.unit * 0.72;
            let hspan2 = tspan * 0.75;
            // Stub-pill height as a fraction of its row — the row is already unit-scaled, so no extra ru factor (that would double-scale).
            let pillf = |base: Coord| base.min(1.0);
            // Draw a labelled action pill; stamps `settings_btn_base + slot` and returns nothing (stub). `n` rows must match update_widget_layout's split where widgets coexist.
            let btn_base = self.settings_btn_base;
            // Immediate-mode stub pill helper — captured as a closure over the canvas/text/hit-map isn't possible (multiple &mut borrows), so pills are drawn inline per page below via `draw_stub_pill`.
            match page {
                SettingsPage::You => {
                    // A dynamic profile form: standard fields grouped by taxonomy tier, each an editable box prefilled from its `profile.<id>` setting, then an add-a-custom-field row, the identity read-out, and the action pills. Handle strings live at rest NOWHERE (docs/identity-profile.md) — this page shows names + the identity FINGERPRINT, never a handle. All rows are optional: your handle IS your identity.
                    // Everything clips to the content pane so a scrolled-up row can't bleed into the header band (this page is far taller than the viewport). Rows fully outside the visible band are culled (perf + the pills carry no clip of their own).
                    let inset = layout.content_inset();
                    let content_clip = fluor::paint::Clip::new(
                        inset.x.max(0.0) as usize,
                        inset.y.max(0.0) as usize,
                        inset.right().max(0.0) as usize,
                        inset.bottom().max(0.0) as usize,
                    );
                    // Textboxes clip to the FULL content pane, not the reading inset — the focus glow blooms a few px past the pill, so the tighter inset clip was shaving it at the edges. Still bounded to the content pane, so it never bleeds into the rail or header.
                    let glow_clip = fluor::paint::Clip::new(
                        layout.content.x.max(0.0) as usize,
                        layout.content.y.max(0.0) as usize,
                        layout.content.right().max(0.0) as usize,
                        layout.content.bottom().max(0.0) as usize,
                    );
                    let content_top = inset.y;
                    let content_bot = inset.bottom();
                    let plan = you_rows_plan(&self.you_fields);
                    for (i, row) in plan.iter().enumerate() {
                        let r = you_row_rect(&layout, settings_content_scroll, i);
                        if r.bottom() <= content_top || r.y >= content_bot {
                            // Culled: reset the row's textboxes to never-painted so they report NO damage while hidden — a culled box otherwise keeps dirty-from-birth caches (or a stale prev-rect from before the scroll) and leaks phantom damage every blink frame. The scroll frame that culled it was a full scene repaint, so its old pixels are already gone.
                            match row {
                                YouRow::Field(idx) => {
                                    let pf = &mut self.you_fields[*idx];
                                    pf.tb.reset_paint_tracking();
                                    if let Some(tag) = pf.tag_tb.as_mut() {
                                        tag.reset_paint_tracking();
                                    }
                                }
                                YouRow::AddInput => {
                                    if let Some(tb) = self.you_add_textbox.as_mut() {
                                        tb.reset_paint_tracking();
                                    }
                                }
                                _ => {}
                            }
                            continue;
                        }
                        match row {
                            YouRow::Header(title) => {
                                ctx.text.draw_text_left(
                                    &mut canvas,
                                    title,
                                    r.x + tspan * 0.3,
                                    r.center_y(),
                                    &TextStyle::new(tspan, *theme::CONTACT_NAME_COLOUR)
                                        .weight(600)
                                        .font("Oxanium"),
                                    Some(content_clip),
                                    None,
                                );
                            }
                            YouRow::Field(idx) => {
                                // Label | value | share box — the third column is the default-share checkbox (absent on the display name row).
                                let cols = r.split_h([0.4, 0.54, 0.06]);
                                let label = self.you_fields[*idx].label.clone();
                                ctx.text.draw_text_left(
                                    &mut canvas,
                                    &label,
                                    cols[0].x + hspan2 * 0.3,
                                    cols[0].center_y(),
                                    &TextStyle::new(hspan2, *theme::LABEL_COLOUR).font("Oxanium"),
                                    Some(content_clip),
                                    None,
                                );
                                let pf = &mut self.you_fields[*idx];
                                let id = pf.tb.hit_id();
                                pf.tb.render_content_into(
                                    &mut canvas,
                                    0.,
                                    0.,
                                    ctx.text,
                                    Some(glow_clip),
                                    None,
                                    Some(&mut chrome.hit_test_map),
                                    id,
                                );
                                // Companion tag box (phone: home / work / custom) rides the right end of the same row.
                                if let Some(tag) = pf.tag_tb.as_mut() {
                                    let tid = tag.hit_id();
                                    tag.render_content_into(
                                        &mut canvas,
                                        0.,
                                        0.,
                                        ctx.text,
                                        Some(glow_clip),
                                        None,
                                        Some(&mut chrome.hit_test_map),
                                        tid,
                                    );
                                }
                                if let Some(cb) = pf.share_cb.as_mut() {
                                    cb.render_content_into(
                                        &mut canvas,
                                        ctx.text,
                                        Some(content_clip),
                                        Some(&mut chrome.hit_test_map),
                                    );
                                }
                            }
                            YouRow::AddHeader => {
                                ctx.text.draw_text_left(
                                    &mut canvas,
                                    "Add a custom field",
                                    r.x + tspan * 0.3,
                                    r.center_y(),
                                    &TextStyle::new(tspan, *theme::CONTACT_NAME_COLOUR)
                                        .weight(600)
                                        .font("Oxanium"),
                                    Some(content_clip),
                                    None,
                                );
                            }
                            YouRow::AddInput => {
                                let cols = r.split_h([0.62, 0.38]);
                                if let Some(tb) = self.you_add_textbox.as_mut() {
                                    let id = tb.hit_id();
                                    tb.render_content_into(
                                        &mut canvas,
                                        0.,
                                        0.,
                                        ctx.text,
                                        Some(glow_clip),
                                        None,
                                        Some(&mut chrome.hit_test_map),
                                        id,
                                    );
                                }
                                draw_stub_pill(
                                    &mut canvas,
                                    ctx.text,
                                    &mut chrome.hit_test_map,
                                    buf_w,
                                    buf_h,
                                    cols[1].center_h(0.72),
                                    "Add",
                                    btn_base.wrapping_add(2),
                                    ctx.pressed_hit,
                                );
                            }
                            YouRow::Note => {
                                ctx.text.draw_text_left(&mut canvas, "Your handle IS your identity — you don't have to set any of this.", r.x + hspan2 * 0.3, r.center_y(), &TextStyle::new(hspan2, *theme::LABEL_COLOUR).font("Oxanium"), Some(content_clip), None);
                            }
                            YouRow::IdentityHeader => {
                                ctx.text.draw_text_left(
                                    &mut canvas,
                                    "Identity",
                                    r.x + tspan * 0.3,
                                    r.center_y(),
                                    &TextStyle::new(tspan, *theme::CONTACT_NAME_COLOUR)
                                        .weight(600)
                                        .font("Oxanium"),
                                    Some(content_clip),
                                    None,
                                );
                            }
                            YouRow::IdentityFp => {
                                let fp = self
                                    .session
                                    .as_ref()
                                    .map(|s| {
                                        crate::fp(&crate::crypto::clutch::identity_party_id(
                                            &s.identity_seed,
                                        ))
                                    })
                                    .unwrap_or_else(|| "—".to_string());
                                ctx.text.draw_text_left(
                                    &mut canvas,
                                    &fp,
                                    r.x + hspan2 * 0.3,
                                    r.center_y(),
                                    &TextStyle::new(hspan2, *theme::LABEL_COLOUR).font("Oxanium"),
                                    Some(content_clip),
                                    None,
                                );
                            }
                            YouRow::SavePill => {
                                draw_stub_pill(
                                    &mut canvas,
                                    ctx.text,
                                    &mut chrome.hit_test_map,
                                    buf_w,
                                    buf_h,
                                    r.center_h(pillf(0.5)),
                                    "Update",
                                    btn_base.wrapping_add(0),
                                    ctx.pressed_hit,
                                );
                            }
                            YouRow::Blank => {}
                            YouRow::AvatarPill => {
                                draw_stub_pill(
                                    &mut canvas,
                                    ctx.text,
                                    &mut chrome.hit_test_map,
                                    buf_w,
                                    buf_h,
                                    r.center_h(pillf(0.5)),
                                    "Change avatar…",
                                    btn_base.wrapping_add(1),
                                    ctx.pressed_hit,
                                );
                            }
                        }
                    }
                }
                SettingsPage::Fleet => {
                    let locked_set = &fleet_locked_set;
                    // Live device inventory (gathered above the chrome borrow): this device + our siblings, then the retired rows (signed out, brand still ours — refreshed on page entry). Rows 1..=6 hold up to 6 devices (fleets are usually ≤5; a scroll follows if this grows past the row budget). Member rows are tap-to-copy (btn_base+16+index); retired rows carry a per-row Release pill instead (btn_base+24+index, two-tap).
                    let devices = &fleet_devices;
                    let rows = layout
                        .content_scrolled(8, settings_content_scroll)
                        .split_v([1.0; 8]);
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[0],
                        "Your devices",
                        tspan,
                        *theme::CONTACT_NAME_COLOUR,
                        600,
                    );
                    // "click to copy" — lighter/smaller, right-aligned in the header, over the same (right) side the device NAMES sit on, so it labels the tap-to-copy target.
                    {
                        let cc = "click to copy";
                        let cc_size = hspan2 * 0.82;
                        let cc_w = ctx
                            .text
                            .measure_text(cc, &TextStyle::new(cc_size, 0).font("Oxanium"));
                        ctx.text.draw_text_left(
                            &mut canvas,
                            cc,
                            rows[0].right() - cc_w - hspan2 * 0.3,
                            rows[0].center_y(),
                            &TextStyle::new(cc_size, *theme::LABEL_COLOUR).font("Oxanium"),
                            None,
                            None,
                        );
                    }
                    for (i, (pk, is_self, online, retired, name, link, tier, about)) in
                        devices.iter().take(6).enumerate()
                    {
                        let row = rows[1 + i];
                        // Three columns: STATUS (left) | NAME (middle) | ACTION pill (right). Explicit thirds so nothing overlaps (the name used to draw across the full row on top of the mid pill — that's why Bridge was invisible).
                        let cols = row.split_h([1.0, 1.0, 1.0]);
                        let row_locked = locked_set.contains(pk);
                        // Transport dot, left of the status word: green LAN, cyan WAN, orange relay. Absent when the device isn't reachable.
                        if let Some(colour) = tier {
                            let r = hspan2 * 0.26;
                            paint::circle_filled(
                                &mut canvas,
                                (cols[0].x + r * 1.6) as isize,
                                cols[0].center_y() as isize,
                                r as isize,
                                *colour,
                                None,
                                None,
                            );
                        }
                        let (status, status_colour) = if *is_self {
                            ("(this device)", (*theme::LABEL_COLOUR))
                        } else if *retired {
                            ("retired \u{2014} still yours", (*theme::LABEL_COLOUR))
                        } else if row_locked {
                            ("locked out", theme::PILL_RED.1)
                        } else if *online {
                            ("online", (*theme::SEARCH_FOUND_COLOUR))
                        } else {
                            ("offline", (*theme::LABEL_COLOUR))
                        };
                        // STATUS in the left column, inset past the transport dot.
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            fluor::region::Region::new(
                                cols[0].x + hspan2 * 0.9,
                                cols[0].y,
                                cols[0].w - hspan2 * 0.9,
                                cols[0].h,
                            ),
                            status,
                            hspan2 * 0.85,
                            status_colour,
                            400,
                        );
                        // The link state rides under the status word, dimmer and smaller — present only for siblings (self and retired rows carry none).
                        if !link.is_empty() {
                            ctx.text.draw_text_left(
                                &mut canvas,
                                link,
                                cols[0].x + hspan2 * 0.3,
                                cols[0].center_y() + hspan2 * 0.85,
                                &TextStyle::new(hspan2 * 0.7, *theme::LABEL_COLOUR).font("Oxanium"),
                                None,
                                None,
                            );
                        }
                        // Name centred in the MIDDLE column (tap-to-copy hit stamped over just that column).
                        ctx.text.draw_text_center(
                            &mut canvas,
                            name,
                            cols[1].center_x(),
                            cols[1].center_y(),
                            &TextStyle::new(hspan2, *theme::CONTACT_NAME_COLOUR)
                                .weight(500)
                                .font("Oxanium"),
                            None,
                            None,
                        );
                        // Per-device About under the name — what that device is RUNNING (version · commit · os arch, off its sealed pong tail; this row's own build for self). The "did the deploy ship?" answer without a bridge session; a stale version here IS the not-yet-updated indicator.
                        if !about.is_empty() {
                            ctx.text.draw_text_center(
                                &mut canvas,
                                about,
                                cols[1].center_x(),
                                cols[1].center_y() + hspan2 * 0.85,
                                &TextStyle::new(hspan2 * 0.62, *theme::LABEL_COLOUR).font("Oxanium"),
                                None,
                                None,
                            );
                        }
                        restamp_hit_rect(
                            &mut chrome.hit_test_map,
                            buf_w,
                            buf_h,
                            cols[1].x as isize,
                            cols[1].y as isize,
                            (cols[1].x + cols[1].w) as isize,
                            (cols[1].y + cols[1].h) as isize,
                            btn_base.wrapping_add(16 + i as HitId),
                        );
                        // RIGHT column = the per-device action pill(s): Release (retired) or Bridge + Lock out (live sibling). This device gets nothing there.
                        if *retired {
                            let armed = self.fleet_release_armed.as_ref() == Some(pk);
                            let label = if armed {
                                "Release \u{2014} sure?"
                            } else {
                                "Release"
                            };
                            draw_stub_pill_filled(
                                &mut canvas,
                                ctx.text,
                                &mut chrome.hit_test_map,
                                buf_w,
                                buf_h,
                                cols[2].center_h(0.8),
                                label,
                                btn_base.wrapping_add(24 + i as HitId),
                                ctx.pressed_hit,
                                true,
                                if armed { Some(*theme::PILL_RED) } else { None },
                                "Oxanium",
                            );
                        } else if !*is_self {
                            // Live sibling: the RIGHT column carries Bridge and the row's state pill side by side — Lock out on a trusted row, Unlock on a locked one.
                            let halves = cols[2].split_h([1.0, 1.0]);
                            let (bridge_pill, lock_pill, unlock_pill) = if row_locked {
                                (halves[0].center_h(0.9), None, Some(halves[1].center_h(0.9)))
                            } else {
                                (halves[0].center_h(0.9), Some(halves[1].center_h(0.9)), None)
                            };
                            // Bridge on ANY sibling (not just confirmed-online): the send reports "no address yet" if truly unreachable, which is clearer than a missing button. Green when online, dimmed grey when offline so it still reads as "reachable-ish".
                            let fill = if *online {
                                Some(*theme::PILL_GREEN)
                            } else {
                                Some(*theme::PILL_GREY)
                            };
                            draw_stub_pill_filled(
                                &mut canvas,
                                ctx.text,
                                &mut chrome.hit_test_map,
                                buf_w,
                                buf_h,
                                bridge_pill,
                                "Bridge",
                                btn_base.wrapping_add(8 + i as HitId),
                                ctx.pressed_hit,
                                true,
                                fill,
                                "Oxanium",
                            );
                            // Live sibling rows carry the treat-as-stolen pill (two-tap): lock the device out WITHOUT touching the chain — removal is self-signed only, zero exceptions; this is the fleet refusing to listen.
                            if let Some(pill) = lock_pill {
                                let armed = self.fleet_lock_armed.as_ref() == Some(pk);
                                let label = if armed {
                                    "Lock out \u{2014} sure?"
                                } else {
                                    "Lock out"
                                };
                                draw_stub_pill_filled(
                                    &mut canvas,
                                    ctx.text,
                                    &mut chrome.hit_test_map,
                                    buf_w,
                                    buf_h,
                                    pill,
                                    label,
                                    btn_base.wrapping_add(32 + i as HitId),
                                    ctx.pressed_hit,
                                    true,
                                    if armed { Some(*theme::PILL_RED) } else { None },
                                    "Oxanium",
                                );
                            }
                            // Locked rows carry the reversal (two-tap): the confirm routes thru a handle re-proof exactly like the lock, then clears the worker verdict + fleet marker and re-mints the key with the device wrapped back in.
                            if let Some(pill) = unlock_pill {
                                let armed = self.fleet_unlock_armed.as_ref() == Some(pk);
                                let label = if armed {
                                    "Unlock \u{2014} sure?"
                                } else {
                                    "Unlock"
                                };
                                draw_stub_pill_filled(
                                    &mut canvas,
                                    ctx.text,
                                    &mut chrome.hit_test_map,
                                    buf_w,
                                    buf_h,
                                    pill,
                                    label,
                                    btn_base.wrapping_add(40 + i as HitId),
                                    ctx.pressed_hit,
                                    true,
                                    if armed { Some(*theme::PILL_RED) } else { None },
                                    "Oxanium",
                                );
                            }
                        }
                    }
                    // No Remove pill: expulsion is not a verb (sovereign records) — a device leaves by its own signed departure. And leaving never frees the hardware: the brand outlives the membership until the owner releases it above.
                    // SINGLE-COPY WARNING (docs/durability.md §1): fleet-holds-history means a one-device fleet holds the ONLY copy. The honest-surface line takes the hint row whenever no live sibling exists; the sign-out hint returns once history is replicated.
                    let single_copy = !devices
                        .iter()
                        .any(|(_, is_self, _, retired, ..)| !*is_self && !*retired);
                    if single_copy {
                        settings_line(&mut canvas, ctx.text, rows[6], "Your message history lives only on this device \u{2014} add a device to replicate it.", hspan2, theme::PILL_RED.1, 500);
                    } else {
                        settings_line(&mut canvas, ctx.text, rows[6], "A device can only sign itself out \u{2014} and its hardware stays yours until you release it.", hspan2, *theme::LABEL_COLOUR, 400);
                    }
                    let pr = rows[7].split_h([1.0, 1.0]);
                    draw_stub_pill(
                        &mut canvas,
                        ctx.text,
                        &mut chrome.hit_test_map,
                        buf_w,
                        buf_h,
                        pr[0].center_h(0.85),
                        "Add device",
                        btn_base.wrapping_add(0),
                        ctx.pressed_hit,
                    );
                    draw_stub_pill(
                        &mut canvas,
                        ctx.text,
                        &mut chrome.hit_test_map,
                        buf_w,
                        buf_h,
                        pr[1].center_h(0.85),
                        "Rename",
                        btn_base.wrapping_add(1),
                        ctx.pressed_hit,
                    );
                }
                SettingsPage::Security => {
                    // Destructiveness ramp, least → most, one blank row between each pill so they breathe: Lock (green, reversible) · fleet self-removal (yellow) · Shred (orange, wipe this device) · Remove & shred (red, sign out of the fleet THEN wipe). The two wipers are two-tap confirmed, mutually exclusive. Rows 11-14 hold the DANGEROUS unattended toggle + disclaimer, well below the wipers.
                    let rows = layout
                        .content_scrolled(15, settings_content_scroll)
                        .split_v([1.0; 15]);
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[0],
                        "Security",
                        tspan,
                        *theme::CONTACT_NAME_COLOUR,
                        600,
                    );
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[1],
                        "Named by destructiveness.",
                        hspan2,
                        *theme::LABEL_COLOUR,
                        400,
                    );
                    draw_stub_pill_filled(
                        &mut canvas,
                        ctx.text,
                        &mut chrome.hit_test_map,
                        buf_w,
                        buf_h,
                        rows[2].center_h(pillf(0.55)),
                        "Lock (re-unlock with your handle)",
                        btn_base.wrapping_add(0),
                        ctx.pressed_hit,
                        true,
                        Some(*theme::PILL_GREEN),
                        "Open Sans",
                    );
                    draw_stub_pill_filled(
                        &mut canvas,
                        ctx.text,
                        &mut chrome.hit_test_map,
                        buf_w,
                        buf_h,
                        rows[4].center_h(pillf(0.55)),
                        "Remove this device from fleet",
                        btn_base.wrapping_add(1),
                        ctx.pressed_hit,
                        true,
                        Some(*theme::PILL_YELLOW),
                        "Open Sans",
                    );
                    let shred_label = if self.settings_shred_armed {
                        "Shred — tap again to confirm"
                    } else {
                        "Shred (crypto-wipe)"
                    };
                    draw_stub_pill_filled(
                        &mut canvas,
                        ctx.text,
                        &mut chrome.hit_test_map,
                        buf_w,
                        buf_h,
                        rows[6].center_h(pillf(0.55)),
                        shred_label,
                        btn_base.wrapping_add(2),
                        ctx.pressed_hit,
                        true,
                        Some(*theme::PILL_ORANGE),
                        "Open Sans",
                    );
                    let rs_label = if self.settings_removeshred_armed {
                        "Remove & shred — tap again to confirm"
                    } else {
                        "Remove & shred (sign out, then wipe)"
                    };
                    draw_stub_pill_filled(
                        &mut canvas,
                        ctx.text,
                        &mut chrome.hit_test_map,
                        buf_w,
                        buf_h,
                        rows[8].center_h(pillf(0.55)),
                        rs_label,
                        btn_base.wrapping_add(3),
                        ctx.pressed_hit,
                        true,
                        Some(*theme::PILL_RED),
                        "Open Sans",
                    );
                    if self.settings_shred_armed {
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[9],
                            "Wipes the vault AND identity on this device — irreversible. It stays signed in to your fleet; to pass the device on, use Remove & shred.",
                            hspan2,
                            *theme::ERROR_TEXT_COLOUR,
                            500,
                        );
                    } else if self.settings_removeshred_armed {
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[9],
                            "Signs this device out of your fleet, then wipes it — irreversible.",
                            hspan2,
                            *theme::ERROR_TEXT_COLOUR,
                            500,
                        );
                    }
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[10],
                        "Security: strong   ·   Recovery: not set up",
                        hspan2,
                        *theme::LABEL_COLOUR,
                        400,
                    );
                    // ── DANGEROUS: unattended auto-attest-on-reboot. Off by default. Rows 11-14. Two states, both drawn INLINE in this page (no floating overlay — an over-content modal drawn after chrome.flatten_into never composited its glyphs): the normal checkbox+disclaimer, OR (while a flip is pending) a handle-entry confirmation that must re-prove the operator before arming/disarming.
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[11],
                        "\u{26A0} Auto-attest on reboot (unattended)",
                        hspan2,
                        *theme::CONTACT_NAME_COLOUR,
                        600,
                    );
                    if let Some(target_on) = self.unattended_confirm {
                        // CONFIRM state: re-type the handle to arm/disarm.
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[12],
                            if target_on {
                                "Re-type your handle to ARM (this box will reboot as you):"
                            } else {
                                "Re-type your handle to disarm:"
                            },
                            hspan2,
                            *theme::ERROR_TEXT_COLOUR,
                            600,
                        );
                        if let Some(tb) = self.unattended_confirm_tb.as_mut() {
                            let id = tb.hit_id();
                            tb.render_content_into(
                                &mut canvas,
                                0.,
                                0.,
                                ctx.text,
                                None,
                                None,
                                Some(&mut chrome.hit_test_map),
                                id,
                            );
                        }
                        let pr = rows[14].split_h([1.0, 1.0]);
                        draw_stub_pill_filled(
                            &mut canvas,
                            ctx.text,
                            &mut chrome.hit_test_map,
                            buf_w,
                            buf_h,
                            pr[0].center_h(pillf(0.6)),
                            if target_on { "Arm" } else { "Disarm" },
                            self.unattended_confirm_base,
                            ctx.pressed_hit,
                            true,
                            Some(*theme::PILL_RED),
                            "Open Sans",
                        );
                        draw_stub_pill(
                            &mut canvas,
                            ctx.text,
                            &mut chrome.hit_test_map,
                            buf_w,
                            buf_h,
                            pr[1].center_h(pillf(0.6)),
                            "Cancel",
                            self.unattended_confirm_base.wrapping_add(1),
                            ctx.pressed_hit,
                        );
                        if self.unattended_confirm_failed {
                            settings_line(
                                &mut canvas,
                                ctx.text,
                                rows[13],
                                "Handle didn't match — try again.",
                                hspan2,
                                *theme::ERROR_TEXT_COLOUR,
                                600,
                            );
                        }
                    } else {
                        // NORMAL state: checkbox + disclaimer (red + bold once armed).
                        let armed = self
                            .settings_unattended_check
                            .as_ref()
                            .map(|c| c.is_checked())
                            .unwrap_or(false);
                        if let Some(cb) = self.settings_unattended_check.as_mut() {
                            cb.render_content_into(
                                &mut canvas,
                                ctx.text,
                                None,
                                Some(&mut chrome.hit_test_map),
                            );
                        }
                        let dc = if armed {
                            *theme::ERROR_TEXT_COLOUR
                        } else {
                            *theme::LABEL_COLOUR
                        };
                        settings_line(&mut canvas, ctx.text, rows[13], "BAD IDEA on any device you carry. Defeats the whole point of a passless identity:", hspan2, dc, if armed { 600 } else { 400 });
                        settings_line(&mut canvas, ctx.text, rows[14], "after a reboot this box becomes YOU with no handle typed. Only for remote failsafe boxes you physically control.", hspan2, dc, if armed { 600 } else { 400 });
                    }
                }
                SettingsPage::Recovery => {
                    let rows = layout
                        .content_scrolled(8, settings_content_scroll)
                        .split_v([1.0; 8]);
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[0],
                        "Recovery",
                        tspan,
                        *theme::CONTACT_NAME_COLOUR,
                        600,
                    );
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[1],
                        &format!("Custodians (v{})", crate::dozenal_glyphs(1)),
                        hspan2,
                        *theme::CONTACT_NAME_COLOUR,
                        600,
                    );
                    if let Some(cb) = self.settings_custodian_check.as_mut() {
                        cb.render_content_into(
                            &mut canvas,
                            ctx.text,
                            None,
                            Some(&mut chrome.hit_test_map),
                        );
                    }
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[4],
                        "Identity backup",
                        hspan2,
                        *theme::CONTACT_NAME_COLOUR,
                        600,
                    );
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[5],
                        "Reinstalling won't ask for your handle.",
                        hspan2,
                        *theme::LABEL_COLOUR,
                        400,
                    );
                    draw_stub_pill(
                        &mut canvas,
                        ctx.text,
                        &mut chrome.hit_test_map,
                        buf_w,
                        buf_h,
                        rows[6].center_h(pillf(0.5)),
                        "Back up identity…",
                        btn_base.wrapping_add(0),
                        ctx.pressed_hit,
                    );
                }
                SettingsPage::Appearance => {
                    let rows = layout
                        .content_scrolled(8, settings_content_scroll)
                        .split_v([1.0; 8]);
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[0],
                        "Appearance",
                        tspan,
                        *theme::CONTACT_NAME_COLOUR,
                        600,
                    );
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[1],
                        "Theme",
                        hspan2,
                        *theme::LABEL_COLOUR,
                        400,
                    );
                    if let Some(dd) = self.settings_theme_dropdown.as_mut() {
                        dd.render_content_into(
                            &mut canvas,
                            0.,
                            0.,
                            ctx.text,
                            None,
                            Some(&mut chrome.hit_test_map),
                        );
                    }
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[3],
                        "Party colours (placeholder → perceptual L≈50%)",
                        hspan2,
                        *theme::LABEL_COLOUR,
                        400,
                    );
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[4],
                        "Zoom / text size",
                        hspan2,
                        *theme::LABEL_COLOUR,
                        400,
                    );
                    if let Some(sl) = self.settings_zoom_slider.as_mut() {
                        sl.render_content_into(
                            &mut canvas,
                            Some(&mut chrome.hit_test_map),
                            sl.hit_id(),
                        );
                    }
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[6],
                        "Colour calibration (Android panel)",
                        hspan2,
                        *theme::LABEL_COLOUR,
                        400,
                    );
                }
                SettingsPage::Notifications => {
                    let rows = layout
                        .content_scrolled(8, settings_content_scroll)
                        .split_v([1.0; 8]);
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[0],
                        "Notifications",
                        tspan,
                        *theme::CONTACT_NAME_COLOUR,
                        600,
                    );
                    if let Some(cb) = self.settings_chime_check.as_mut() {
                        cb.render_content_into(
                            &mut canvas,
                            ctx.text,
                            None,
                            Some(&mut chrome.hit_test_map),
                        );
                    }
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[2],
                        "Per-contact override lives in each conversation.",
                        hspan2,
                        *theme::LABEL_COLOUR,
                        400,
                    );
                    if let Some(cb) = self.settings_presence_check.as_mut() {
                        cb.render_content_into(
                            &mut canvas,
                            ctx.text,
                            None,
                            Some(&mut chrome.hit_test_map),
                        );
                    }
                    if let Some(cb) = self.settings_background_check.as_mut() {
                        cb.render_content_into(
                            &mut canvas,
                            ctx.text,
                            None,
                            Some(&mut chrome.hit_test_map),
                        );
                    }
                }
                SettingsPage::Updates => {
                    // Rows (blanks between the pills for vertical breathing room): 0 title · 1 current version · 2 blank · 3 release pill · 4 blank · 5 dev pill · 6 blank · 7 status.
                    let rows = layout
                        .content_scrolled(8, settings_content_scroll)
                        .split_v([1.0; 8]);
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[0],
                        "Updates",
                        tspan,
                        *theme::CONTACT_NAME_COLOUR,
                        600,
                    );
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[1],
                        &format!("Photon {}", version_dozenal_glyphs()),
                        hspan2,
                        *theme::CONTACT_NAME_COLOUR,
                        400,
                    );
                    if let Some(cb) = self.settings_autoupdate_check.as_mut() {
                        cb.render_content_into(
                            &mut canvas,
                            ctx.text,
                            None,
                            Some(&mut chrome.hit_test_map),
                        );
                    }
                    // One channel button: label + colour driven by the auto-check state. Release = green when an update is available, Dev = amber; either goes inert dark grey ("Already on …") when the remote version equals ours. Disabled while an install is in flight.
                    let ours = crate::network::updates::our_version();
                    let button = |canvas: &mut Canvas,
                                  text: &mut fluor::text::TextRenderer,
                                  hit_map: &mut [HitId],
                                  rect: fluor::region::Region,
                                  slot: HitId,
                                  kind: &str,
                                  avail_fill: (u32, u32),
                                  state: &ChannelCheck,
                                  busy: bool| {
                        let (label, fill, enabled) = match state {
                            ChannelCheck::Idle | ChannelCheck::Checking => (
                                format!("Checking {kind}\u{2026}"),
                                (*theme::PILL_GREY),
                                false,
                            ),
                            ChannelCheck::Failed => {
                                (format!("{kind} unavailable"), (*theme::PILL_GREY), false)
                            }
                            ChannelCheck::Ready(None) => (
                                format!("No {kind} build for this device"),
                                (*theme::PILL_GREY),
                                false,
                            ),
                            // Tuple equality IS the truth: patch 0 is the release marker and the version scheme guarantees a dev build never wears it (deploy.sh opens the dev line at .1; publishes are publish-current-then-bump) — so a dev build and the release can never be tuple-equal, and "already on" needs no flavour check.
                            ChannelCheck::Ready(Some(row)) if row.version == ours => (
                                format!("Already on {kind} {}", dozenal_version_tuple(row.version)),
                                (*theme::PILL_GREY),
                                false,
                            ),
                            ChannelCheck::Ready(Some(row)) => (
                                format!("Get {kind} {}", dozenal_version_tuple(row.version)),
                                avail_fill,
                                !busy,
                            ),
                        };
                        draw_stub_pill_filled(
                            canvas,
                            text,
                            hit_map,
                            buf_w,
                            buf_h,
                            rect,
                            &label,
                            slot,
                            ctx.pressed_hit,
                            enabled,
                            Some(fill),
                            "Oxanium",
                        );
                    };
                    button(
                        &mut canvas,
                        ctx.text,
                        &mut chrome.hit_test_map,
                        rows[3].center_h(pillf(0.7)),
                        btn_base.wrapping_add(1),
                        "release",
                        *theme::PILL_GREEN,
                        &self.update_release,
                        self.update_busy,
                    );
                    button(
                        &mut canvas,
                        ctx.text,
                        &mut chrome.hit_test_map,
                        rows[5].center_h(pillf(0.7)),
                        btn_base.wrapping_add(2),
                        "dev",
                        *theme::PILL_AMBER,
                        &self.update_dev,
                        self.update_busy,
                    );
                    // Status line: the download bar while bytes stream (label flips "Downloading" → "Updating…" at the end), else the last APPLY outcome (installing / failed / restarting).
                    if let Some((done, total)) = self.update_progress {
                        let finishing = total > 0 && done >= total;
                        // Unknown length (old manifest without size + a chunked CDN stream): the label carries a live MiB counter so the bar area shows life even without a denominator.
                        let label = if finishing {
                            "Updating\u{2026}".to_string()
                        } else if total > 0 {
                            "Downloading".to_string()
                        } else {
                            format!("Downloading {} MiB\u{2026}", done >> 20)
                        };
                        let label = label.as_str();
                        let r = rows[7];
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            fluor::region::Region::new(r.x, r.y, r.w, r.h * 0.5),
                            label,
                            hspan2,
                            *theme::CONTACT_NAME_COLOUR,
                            500,
                        );
                        // The bar: proportional fill THEN full-width track. fluor is under-blend (FIRST paint wins), so the lime fill MUST be painted before the black track — draw the track first and it wins every pixel, burying the fill (the "permanent grey bar" bug). Fill goes down first over [0..fill_w], then the track over the whole width fills only the un-painted remainder.
                        let bar_y = (r.y + r.h * 0.55) as isize;
                        let bar_h = (r.h * 0.25) as isize;
                        let bar_w = r.w as isize;
                        if total > 0 {
                            let fill_w = (r.w as f64 * (done as f64 / total as f64)) as isize;
                            paint::fill_rect(
                                &mut canvas,
                                r.x as isize,
                                bar_y,
                                fill_w.clamp(0, bar_w),
                                bar_h,
                                *theme::PROGRESS_FILL,
                                None,
                                None,
                            );
                        }
                        paint::fill_rect(
                            &mut canvas,
                            r.x as isize,
                            bar_y,
                            bar_w,
                            bar_h,
                            *theme::PROGRESS_TRACK,
                            None,
                            None,
                        );
                    } else if let Some(status) = &self.update_status {
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[7],
                            status,
                            hspan2,
                            *theme::CONTACT_NAME_COLOUR,
                            500,
                        );
                    }
                }
                SettingsPage::Diagnostics if self.diag_log_view => {
                    // The in-app log viewer: two full-height header rows PINNED (unscrolled — the Back pill must stay reachable while the view opens at the bottom of a 30k-row log), then the decoded records at HALF line height (dense), scrolling UNDER a clip that starts below the header. Culled to the visible slice — drawing ~40 rows is one frame's work. Row geometry mirrors diag_log_row_rect / the extent math exactly.
                    let inset = layout.content_inset();
                    let line = layout.content_line_h();
                    // Records clip BELOW the pinned header band.
                    let content_clip = fluor::paint::Clip::new(
                        inset.x.max(0.0) as usize,
                        (inset.y + 2. * line).max(0.0) as usize,
                        inset.right().max(0.0) as usize,
                        inset.bottom().max(0.0) as usize,
                    );
                    let header = layout.content_scrolled(2, 0.0).split_v([1.0; 2]);
                    let hr = header[0].split_h([2.0, 1.0]);
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        hr[0],
                        "Log",
                        tspan,
                        *theme::CONTACT_NAME_COLOUR,
                        600,
                    );
                    draw_stub_pill(
                        &mut canvas,
                        ctx.text,
                        &mut chrome.hit_test_map,
                        buf_w,
                        buf_h,
                        hr[1].center_h(0.85),
                        "Back",
                        btn_base.wrapping_add(3),
                        ctx.pressed_hit,
                    );
                    let meta = if let Some((idx, lines)) = &self.diag_log_inspect {
                        let ts = self
                            .diag_log_rows
                            .get(*idx)
                            .filter(|r| r.osc != 0)
                            .map(|r| {
                                vsf::types::EagleTime::from_oscillations(r.osc)
                                    .to_datetime()
                                    .format("%m-%d %H:%M:%S%.3f")
                                    .to_string()
                            })
                            .unwrap_or_default();
                        format!(
                            "Record VSF · {} · {} line(s) · tap Back for the list",
                            ts,
                            lines.len()
                        )
                    } else if self.diag_log_rx.is_some() {
                        "Decoding log\u{2026}".to_string()
                    } else if self.diag_log_rows.is_empty() {
                        "Log is empty".to_string()
                    } else {
                        format!(
                            "{} record(s) · {} KiB · newest at the bottom · tap a row for its VSF{}",
                            self.diag_log_rows.len(),
                            (crate::log_size_bytes() + 1023) / 1024,
                            if self.diag_log_rows.len() >= DIAG_LOG_MAX_ROWS { " · oldest trimmed" } else { "" },
                        )
                    };
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        header[1],
                        &meta,
                        hspan2,
                        *theme::LABEL_COLOUR,
                        400,
                    );

                    let row_h = line * 0.5;
                    // INSPECTOR: the tapped record's coloured VSF pretty-print, span by span (the same output vsfinfo pipes to a terminal, ANSI parsed to fluor colours). Same culling/extent math as the list — one branch, then done.
                    if let Some((_, ins_lines)) = &self.diag_log_inspect {
                        let first = ((settings_content_scroll / row_h).floor().max(0.)) as usize;
                        let visible = (inset.h / row_h).ceil() as usize + 2;
                        let size = row_h * 0.62;
                        for i in first..(first + visible).min(ins_lines.len()) {
                            let r = diag_log_row_rect(&layout, settings_content_scroll, i);
                            if r.y > inset.y + inset.h {
                                break;
                            }
                            let mut x = r.x;
                            for (span, colour) in &ins_lines[i] {
                                ctx.text.draw_text_left(
                                    &mut canvas,
                                    span,
                                    x,
                                    r.center_y(),
                                    &TextStyle::new(size, *colour).font("Oxanium"),
                                    Some(content_clip),
                                    None,
                                );
                                x += ctx
                                    .text
                                    .measure_text(span, &TextStyle::new(size, 0).font("Oxanium"));
                            }
                        }
                        // The list rendering below is the OTHER mode.
                    } else {
                        // First visible record: the band's top scrolls as inset.y + 2·line − scroll, the clip top sits at inset.y + 2·line, so the first index is simply scroll/row_h. +2 rows of slack covers the fractional edges.
                        let first = ((settings_content_scroll / row_h).floor().max(0.)) as usize;
                        let visible = (inset.h / row_h).ceil() as usize + 2;
                        let size = row_h * 0.62;
                        for i in first..(first + visible).min(self.diag_log_rows.len()) {
                            let r = diag_log_row_rect(&layout, settings_content_scroll, i);
                            if r.y > inset.y + inset.h {
                                break;
                            }
                            let rec = &self.diag_log_rows[i];
                            // Display-edge time render (records store eagle time binary).
                            let ts = if rec.osc != 0 {
                                vsf::types::EagleTime::from_oscillations(rec.osc)
                                    .to_datetime()
                                    .format("%m-%d %H:%M:%S%.3f")
                                    .to_string()
                            } else {
                                "\u{2014}".to_string()
                            };
                            let (lvl, colour) = match rec.level {
                                4 => ("E", (*theme::ERROR_TEXT_COLOUR)),
                                3 => ("W", (*theme::HOURGLASS_COLOUR)),
                                2 => ("I", (*theme::CONTACT_NAME_COLOUR)),
                                1 => ("D", (*theme::LABEL_COLOUR)),
                                0 => ("T", (*theme::LABEL_COLOUR)),
                                _ => ("?", (*theme::LABEL_COLOUR)),
                            };
                            ctx.text.draw_text_left(
                                &mut canvas,
                                &format!("{ts} {lvl}  {}", rec.msg),
                                r.x,
                                r.center_y(),
                                &TextStyle::new(size, colour).font("Oxanium"),
                                Some(content_clip),
                                None,
                            );
                        }
                    }
                }
                SettingsPage::Diagnostics => {
                    let rows = layout
                        .content_scrolled(10, settings_content_scroll)
                        .split_v([1.0; 10]);
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[0],
                        "Diagnostics",
                        tspan,
                        *theme::CONTACT_NAME_COLOUR,
                        600,
                    );
                    // The live size, not just the cap: "how much have I got to send" is the question this page exists to answer, and a number you can watch grow is also how a log that is filling too fast announces itself.
                    let used = crate::log_size_bytes();
                    let cap = crate::LOG_CAP_BYTES;
                    let pct = if cap > 0 { used * 100 / cap } else { 0 };
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[1],
                        &format!(
                            "On-device log · {} of {} ({}%) · self-expires 24\u{2013}48h",
                            human_bytes(used),
                            human_bytes(cap),
                            pct
                        ),
                        hspan2,
                        *theme::LABEL_COLOUR,
                        400,
                    );
                    let pr = rows[3].split_h([1.0, 1.0, 1.0, 1.0]);
                    draw_stub_pill(
                        &mut canvas,
                        ctx.text,
                        &mut chrome.hit_test_map,
                        buf_w,
                        buf_h,
                        pr[0].center_h(0.85),
                        "Clear",
                        btn_base.wrapping_add(0),
                        ctx.pressed_hit,
                    );
                    draw_stub_pill(
                        &mut canvas,
                        ctx.text,
                        &mut chrome.hit_test_map,
                        buf_w,
                        buf_h,
                        pr[1].center_h(0.85),
                        "Snapshot",
                        btn_base.wrapping_add(1),
                        ctx.pressed_hit,
                    );
                    // Submit greys while an upload is in flight or the log hasn't grown past the last successful submit — a resend then would be a byte-identical duplicate. Any new record (or Clear) moves the size and re-arms it.
                    let submit_disabled = self.log_submit_inflight
                        || self.log_submitted_len == Some(crate::log_size_bytes());
                    if submit_disabled {
                        draw_stub_pill_disabled(
                            &mut canvas,
                            ctx.text,
                            &mut chrome.hit_test_map,
                            buf_w,
                            buf_h,
                            pr[2].center_h(0.85),
                            "Submit",
                            btn_base.wrapping_add(2),
                            ctx.pressed_hit,
                        );
                    } else {
                        draw_stub_pill(
                            &mut canvas,
                            ctx.text,
                            &mut chrome.hit_test_map,
                            buf_w,
                            buf_h,
                            pr[2].center_h(0.85),
                            "Submit",
                            btn_base.wrapping_add(2),
                            ctx.pressed_hit,
                        );
                    }
                    draw_stub_pill(
                        &mut canvas,
                        ctx.text,
                        &mut chrome.hit_test_map,
                        buf_w,
                        buf_h,
                        pr[3].center_h(0.85),
                        "View",
                        btn_base.wrapping_add(3),
                        ctx.pressed_hit,
                    );
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[6],
                        "Optional note",
                        hspan2,
                        *theme::LABEL_COLOUR,
                        400,
                    );
                    if let Some(tb) = self.settings_note_textbox.as_mut() {
                        let id = tb.hit_id();
                        tb.render_content_into(
                            &mut canvas,
                            0.,
                            0.,
                            ctx.text,
                            None,
                            None,
                            Some(&mut chrome.hit_test_map),
                            id,
                        );
                    }
                    if let Some(cb) = self.settings_hardlogs_check.as_mut() {
                        cb.render_content_into(
                            &mut canvas,
                            ctx.text,
                            None,
                            Some(&mut chrome.hit_test_map),
                        );
                    }
                }
                SettingsPage::About => {
                    // An About CARD, not a settings list: the Photon wordmark over its chromatic wave up top, then the two headline properties (killswitch-ready, passless), then the version — tap it to reveal both the spelled-out form AND the dozenal cheat sheet. No feedback line — photon is owned by everyone. All centred under the logo; a manual vertical cursor (elements are variable-height, not equal rows).
                    let inset = layout.content_inset();
                    let line_h = layout.content_line_h();
                    let cx = inset.x + inset.w * 0.5;
                    let mut y = inset.y - settings_content_scroll;
                    // Chromatic wave + wordmark. Static spectrum (phase rides bg_scroll for a touch of life); reads the noise already in `target`. Skipped when scrolled fully off so a negative rect never misbehaves.
                    let wave_phase = self.bg_scroll as f32 / ((1 << 7) as f32);
                    let logo_h = line_h * 4.0;
                    if y + logo_h > inset.y && y < inset.y + inset.h {
                        let lx0 = inset.x.max(0.0) as usize;
                        let ly0 = y.max(inset.y).max(0.0) as usize;
                        let lx1 = (inset.x + inset.w).max(0.0) as usize;
                        let ly1 = (y + logo_h).max(0.0) as usize;
                        if lx1 > lx0 && ly1 > ly0 {
                            let logo_rect = fluor::canvas::PixelRect::new(lx0, ly0, lx1, ly1);
                            chromatic_wave(&mut canvas, logo_rect, wave_phase, 1.0);
                            crate::ui::photon_logo::paint_photon_logo(
                                &mut canvas,
                                ctx.text,
                                logo_rect,
                            );
                        }
                    }
                    y += logo_h + line_h * 0.4;
                    // The two headline properties — the whole pitch in two words each.
                    ctx.text.draw_text_center(
                        &mut canvas,
                        "killswitch ready",
                        cx,
                        y + line_h * 0.5,
                        &TextStyle::new(hspan2, *theme::SEARCH_FOUND_COLOUR)
                            .weight(600)
                            .font("Oxanium"),
                        None,
                        None,
                    );
                    y += line_h;
                    ctx.text.draw_text_center(
                        &mut canvas,
                        "passless",
                        cx,
                        y + line_h * 0.5,
                        &TextStyle::new(hspan2, *theme::SEARCH_FOUND_COLOUR)
                            .weight(600)
                            .font("Oxanium"),
                        None,
                        None,
                    );
                    y += line_h * 1.2;
                    // Version — dozenal glyphs (weight 400 → the Oxanium +glyphs face draws the reserved control bytes as dozenal digits), NEVER arabic. Tap toggles the reveal (spelled form + cheat sheet). Whole row is the tap target (btn_base + 3).
                    let ver = format!("Version {}", version_dozenal_glyphs());
                    ctx.text.draw_text_center(
                        &mut canvas,
                        &ver,
                        cx,
                        y + line_h * 0.5,
                        &TextStyle::new(hspan2, *theme::CONTACT_NAME_COLOUR)
                            .weight(400)
                            .font("Oxanium"),
                        None,
                        None,
                    );
                    restamp_hit_rect(
                        &mut chrome.hit_test_map,
                        buf_w,
                        buf_h,
                        inset.x as isize,
                        y as isize,
                        (inset.x + inset.w) as isize,
                        (y + line_h) as isize,
                        btn_base.wrapping_add(3),
                    );
                    y += line_h;
                    if self.about_version_spelled {
                        // Spelled-out (voca words), then the dozenal cheat sheet: all twelve digits as GLYPH = name, two columns of six.
                        let spelled = format!(
                            "{}{}",
                            crate::dozenal_spell(deploy_version()),
                            if dev_patch() > 0 {
                                format!(" point {}", crate::dozenal_spell(dev_patch()))
                            } else {
                                String::new()
                            }
                        );
                        ctx.text.draw_text_center(
                            &mut canvas,
                            &spelled,
                            cx,
                            y + line_h * 0.5,
                            &TextStyle::new(hspan2 * 0.85, *theme::LABEL_COLOUR)
                                .weight(400)
                                .font("Oxanium"),
                            None,
                            None,
                        );
                        y += line_h * 1.4;
                        ctx.text.draw_text_center(
                            &mut canvas,
                            "dozenal",
                            cx,
                            y + line_h * 0.5,
                            &TextStyle::new(hspan2, *theme::CONTACT_NAME_COLOUR)
                                .weight(600)
                                .font("Oxanium"),
                            None,
                            None,
                        );
                        y += line_h;
                        let col_l = inset.x + inset.w * 0.32;
                        let col_r = inset.x + inset.w * 0.68;
                        for d in 0..6usize {
                            let cell = |digit: usize| {
                                format!(
                                    "{}  {}",
                                    char::from(0x10 + digit as u8),
                                    crate::DOZENAL_NAMES[digit]
                                )
                            };
                            ctx.text.draw_text_center(
                                &mut canvas,
                                &cell(d),
                                col_l,
                                y + line_h * 0.5,
                                &TextStyle::new(hspan2 * 0.85, *theme::LABEL_COLOUR)
                                    .weight(400)
                                    .font("Oxanium"),
                                None,
                                None,
                            );
                            ctx.text.draw_text_center(
                                &mut canvas,
                                &cell(d + 6),
                                col_r,
                                y + line_h * 0.5,
                                &TextStyle::new(hspan2 * 0.85, *theme::LABEL_COLOUR)
                                    .weight(400)
                                    .font("Oxanium"),
                                None,
                                None,
                            );
                            y += line_h;
                        }
                    }
                    let _ = tspan;
                }
            }
        }

        } // end !call_fullscreen — per-screen bodies skipped while the ring panel owns the surface

        // JOINER SELECTED — the green flood (docs/lifecycle.md): this device is bound and waiting on the sponsor's human to confirm "yes, it's green and says Selected". A HOLD, not an interstitial — stray taps must not kill a ceremony mid-confirm, so presses are simply ignored while it's up (the poller or a relaunch are the exits).
        if self.joiner_selected {
            let mut canvas = Canvas::new(target, buf_w, buf_h, ctx.damage);
            paint::fill_rect(
                &mut canvas,
                0,
                0,
                buf_w as isize,
                buf_h as isize,
                *theme::SELECTED_FLOOD,
                None,
                None,
            );
            let span = 2. * buf_w as f32 * buf_h as f32 / (buf_w + buf_h) as f32;
            let cx = buf_w as f32 * 0.5;
            ctx.text.draw_text_center(
                &mut canvas,
                "Selected!",
                cx,
                buf_h as f32 * 0.4,
                &TextStyle::new(span / 8., *theme::CONTACT_NAME_COLOUR)
                    .weight(800)
                    .font("Oxanium"),
                None,
                None,
            );
            ctx.text.draw_text_center(
                &mut canvas,
                "Confirm on your other device to finish.",
                cx,
                buf_h as f32 * 0.58,
                &TextStyle::new(span / 24., *theme::CONTACT_NAME_COLOUR)
                    .weight(500)
                    .font("Oxanium"),
                None,
                None,
            );
        }

        // Re-stamp the call overlay's hit rects LAST: screens re-stamp their own regions of the shared hit_test_map every frame, which would otherwise wipe the top-of-screen call bar's clickable area. Pixels were painted early (under-blend keeps them on top); only the hit rects need re-asserting after every screen has stamped. Each Button stamps its OWN rect (set in the early paint), so the two passes can never disagree. Visibility mirrors `visit_app_widgets` exactly: action always when a call is live, decline in ringing/ended, start only when a callable convo enables it (a dimmed pill must not dispatch a dead tap). The status chip is a label — never stamped.
        if call_overlay.is_some() {
            // Full-screen ring panel is MODAL: wipe the whole map first so the screen's own widgets (stamped above) can't be tapped through the wash — then the two call buttons are the only live targets.
            if call_fullscreen {
                restamp_hit_rect(
                    &mut chrome.hit_test_map,
                    buf_w,
                    buf_h,
                    0,
                    0,
                    buf_w as isize,
                    buf_h as isize,
                    HIT_NONE,
                );
            }
            if let Some(b) = self.call_action_btn.as_ref() {
                b.stamp_hit_into(&mut chrome.hit_test_map, buf_w, buf_h, b.hit_id());
            }
            if call_two_actions {
                if let Some(b) = self.call_decline_btn.as_ref() {
                    b.stamp_hit_into(&mut chrome.hit_test_map, buf_w, buf_h, b.hit_id());
                }
            }
        } else if matches!(self.state, AppState::Conversation)
            && call_pill_show
            && call_pill_enabled
        {
            if let Some(b) = self.call_start_btn.as_ref() {
                b.stamp_hit_into(&mut chrome.hit_test_map, buf_w, buf_h, b.hit_id());
            }
        }

        let mark_content = std::time::Instant::now();
        chrome.flatten_into(target, buf_w, buf_h, None);

        // Development builds get the amber debug theme (orange bg tint / window hairline / title) via fluor's `amber` feature — pure theme-CONSTANT swaps, zero extra drawing steps. The old post-composite amber wash is gone: it wrote straight-RGB into fluor's α+darkness buffer, which inverted to blue.

        // Hit-mask overlay (`[]h`): replace every pixel with the opaque random colour for its hit_test_map ID. Drawn LAST over everything (including chrome + chord hint) — hit testing is per-final-pixel anyway, so the overlay shows exactly what `hit_at` would return. `.get` keeps the index lookup safe for any stale stamp at an unregistered high ID.
        if show_hitmask && !self.debug_hit_colours.is_empty() {
            let map = chrome.hit_test_map();
            let n = map.len().min(target.len());
            for i in 0..n {
                target[i] = self
                    .debug_hit_colours
                    .get(map[i] as usize)
                    .copied()
                    .unwrap_or(0);
            }
        }

        // The stage breakdown, only when the frame is a felt hang: which of the four coarse stages ate it. `content` covers every per-screen paint block (rows, text shaping, avatars) — when it dominates on Conversation, the next split goes inside that block.
        let total_ms = _rt.0.elapsed().as_millis();
        if total_ms > 1000 {
            crate::logf!(
                "PERF: render breakdown — pre {}ms, bg+chrome {}ms, content {}ms, flatten {}ms",
                mark_pre.duration_since(_rt.0).as_millis() as u64,
                mark_chrome.duration_since(mark_pre).as_millis() as u64,
                mark_content.duration_since(mark_chrome).as_millis() as u64,
                mark_content.elapsed().as_millis() as u64
            );
        }
        // Everything content-flavoured is now freshly painted — the next frame can narrow to pure widget damage unless something re-dirties the scene.
        self.scene_dirty = false;
    }
}
