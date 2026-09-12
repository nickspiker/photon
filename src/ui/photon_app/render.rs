//! `render_frame` — the whole-frame paint, split out of the `FluorApp::render` trait method so the frame body lives in its own file.

use super::*;

/// Lines an image attachment's preview band reserves above its pill (typed attachments 2026-09-10): the row's micro thumb, or the decoded preview blob (twice as tall).
pub(super) const IMG_PREVIEW_LINES: usize = 4;
pub(super) const IMG_PREVIEW_LINES_FULL: usize = 8;

/// THE settings-pane hairline (Nick 2026-09-10: "a unified style"): the conversation's divider — pure white at α=1/8, one ru thick — drawn edge to edge across the whole content pane, never inset. Every settings page that separates rows draws its dividers thru here.
fn pane_hairline(canvas: &mut Canvas, layout: &SettingsLayout, y: f32, ru: f32, clip: Option<fluor::paint::Clip>) {
    paint::fill_rect(canvas, layout.content.x as isize, y as isize, layout.content.w as isize, ru.max(1.0) as isize, theme::VERSION_COLOUR, clip, None);
}

/// Greedy word wrap against a pixel width: one measure per candidate join. The attest band and stream entry #0's status line both need it — long ceremony steps and locked-device messages must fold, never run off the sides.
fn wrap_to_width(text: &mut fluor::text::TextRenderer, s: &str, style: &TextStyle, max_w: f32) -> Vec<String> {
    // Explicit '\n' is an authored break (the clutch steps use line returns, never dashes — Nick 2026-09-07): each segment wraps independently and the break always survives.
    if s.contains('\n') {
        return s.lines().flat_map(|seg| wrap_to_width(text, seg, style, max_w)).collect();
    }
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let cand = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if !cur.is_empty() && text.measure_text(&cand, style) > max_w {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        } else {
            cur = cand;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Place a labelled checkbox in a Flow: its natural width when the label fits, the whole pane width with a WRAPPED label when it does not (a scaled-up font, a narrow phone — Nick 2026-09-09: "Auto-attest on reboot", "Be a custodian", "Chime", "Vibrate on incoming", "Check for updates" all ran off the pane). The band grows to the wrapped height; the box stays one square (fluor Checkbox::box_side).
fn flow_checkbox(flow: &mut Flow, canvas: &mut Canvas, text: &mut fluor::text::TextRenderer, hit_map: &mut [HitId], cb: &mut fluor::widgets::Checkbox, label: &str, size: f32) {
    cb.set_label(label);
    cb.set_font_size(size);
    let side = size * 1.3;
    let natural = side + size * 0.5 + text.measure_text(label, &TextStyle::new(size, 0)) + size * 0.3;
    let w = natural.min(flow.w.max(side * 2.0));
    // Width first, so the height measurement wraps at the width the label will actually get.
    cb.set_rect(flow.x + w * 0.5, flow.y, w, side);
    let h = cb.needed_height(text);
    let band = flow.band(h + size * 0.7);
    cb.set_rect(band.x + w * 0.5, band.center_y(), w, h);
    cb.render_content_into(canvas, text, None, Some(hit_map));
}

impl PhotonApp {
    /// The full frame paint — the body of [`FluorApp::render`], verbatim; the trait method in `driver.rs` delegates here so the paint code can live in its own file.
    pub(super) fn render_frame(&mut self, target: &mut [u32], ctx: &mut Context) {
        // Standing render probe (born in the 2026-08-08 typing-lag hunt, kept for regressions). A Drop guard so it fires on every return path. The bar is a MISSED 60fps FRAME: the phone's healthy full-viewport render is 9-16ms, and the hunt's original 8ms bar logged every one of those — 3,326 lines in a 15-minute field log, the single biggest log-volume source (2026-08-09).
        // Stage marks ride the guard (the render-pass sub-profiler, TICKETS 2026-08-21: 5.8/8.3/8.8s single renders named no stage): each `mark` stamps the elapsed ms at a boundary, and a pass past ONE SECOND logs them — bg+chrome, the screen body, and the tail (overlays, extent, chrome finalize) fall out by subtraction.
        struct RenderTimer(std::time::Instant, &'static str, Vec<(&'static str, u128)>);
        impl RenderTimer {
            fn mark(&mut self, stage: &'static str) {
                self.2.push((stage, self.0.elapsed().as_millis()));
            }
        }
        impl Drop for RenderTimer {
            fn drop(&mut self) {
                let ms = self.0.elapsed().as_millis();
                if ms > 16 {
                    crate::logf!("PERF: render took {}ms on {} (UI thread)", ms, self.1);
                }
                if ms > 1000 {
                    let mut prev = 0u128;
                    let stages: Vec<String> = self.2.iter().map(|(s, t)| {
                        let d = t.saturating_sub(prev);
                        prev = *t;
                        format!("{s} {d}ms")
                    }).collect();
                    crate::logf!("PERF: render stages on {} — {}, tail {}ms", self.1, stages.join(", "), ms.saturating_sub(prev));
                }
            }
        }
        let mut _rt = RenderTimer(
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
                AppState::Settings(SettingsPage::Language) => "Settings:Language",
                AppState::Settings(SettingsPage::Dozenal) => "Settings:Dozenal",
                AppState::Settings(SettingsPage::About) => "Settings:About",
                AppState::ContactPanel(_) => "ContactPanel",
            },
            Vec::new(),
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
                // Pending… until they publish a real name — the title bar is a visual surface; the pseudonym lives ONLY in the contact panel's identity section (Nick 2026-08-21, matching the contact list). Siblings show their machine name.
                .map(|c| super::contact_visible_name(c, self.session.as_ref().map(|se| &se.identity_seed), self.fleet_settings.as_ref()))
                .unwrap_or_else(|| tr(Msg::ConversationTitle).into_owned())
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
            tr(Msg::PeersOnline(n)).into_owned()
        } else if matches!(self.state, AppState::Settings(_)) {
            // The settings screen draws its own "Settings" heading in the header band — a chrome title would double up behind it (portrait showed "‹ Network" bleeding thru the heading).
            String::new()
        } else {
            tr(Msg::NetworkBack).into_owned()
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
            // Wrap every listed contact's name at the row's text width (measured at the hover weight, so a hovered row never re-wraps) and sum the resulting row heights — one line = the layout row, each extra line adds a line step. The row walk below reads the same lines.
            let avatar_r = rl.contact_avatar_diameter as f32 * 0.5;
            let text_x = rl.rows.x0 as f32 + avatar_r * 3.0;
            let name_w = (rl.rows.x1 as f32 - text_x - avatar_r * 0.5).max(row_h as f32);
            let text_size = row_h as f32 * 0.5;
            let wrap_style = TextStyle::new(text_size, 0).weight(700).font("Oxanium");
            let seed = self.session.as_ref().map(|se| se.identity_seed);
            let mut lines_by_ci: Vec<Vec<String>> = Vec::with_capacity(self.contacts.len());
            for c in &self.contacts {
                if c.is_sibling {
                    lines_by_ci.push(Vec::new());
                    continue;
                }
                let name = super::contact_visible_name(c, seed.as_ref(), self.fleet_settings.as_ref());
                let mut lines = wrap_text_lines(ctx.text, &name, &wrap_style, name_w);
                if lines.is_empty() {
                    lines.push(String::new());
                }
                lines_by_ci.push(lines);
            }
            let block_h: isize = self
                .contacts
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    // Must mirror the render pass's `matching` filter exactly (siblings hidden) or the two clamps disagree within a frame.
                    !c.is_sibling
                        && (filter.is_empty() || c.display_name().to_lowercase().contains(&filter))
                })
                .map(|(ci, _)| contact_row_height(row_h, lines_by_ci[ci].len()))
                .sum();
            self.contact_row_lines = lines_by_ci;
            let block_bottom_at_zero = rl.rows.y0 as isize + block_h;
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
                // Logo(4) + gap + killswitch + passless + link + no-servers prose(8×0.8) + consent block + TOKEN block + version + toggle + why-dozenal rant ≈ 37 rows collapsed (each prose line 0.8, three 1-row section headers, inter-block gaps); the version reveal adds the spelled line + "dozenal" header + 6 cheat rows ≈ 8.4.
                let rows = 26.0 + if self.about_version_spelled { 1.4 } else { 0.0 };
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
        // No calibration gate on placing a call (the doctrine retired 2026-09-07): every call opens with the v-chirp probe, so the route is measured before any voice connects.
        let call_pill_enabled = self
            .active_contact()
            .and_then(|ci| self.contacts.get(ci))
            .map_or(false, |c| {
                !c.is_sibling && c.is_online && (c.chain_woven || c.friendship_id.is_some())
            });
        // Live call-duration seconds, computed here (a per-frame recompute from the frozen osc stamps — no stored timer): Active counts up from `phase_osc` (re-stamped at answer); Ended freezes at `final_osc - phase_osc`; other phases show 0. Carried in the overlay tuple so the panel + strip render it via the base-aware `fmt_duration`.
        let call_overlay: Option<(crate::call::CallPhase, String, bool, Option<usize>, i64)> =
            self.active_call.as_ref().map(|c| {
                let pi = self
                    .contacts
                    .iter()
                    .position(|k| k.handle_hash == c.peer_handle_hash);
                let peer = pi.map(|i| &self.contacts[i]);
                let name = peer.map(|k| k.display_name()).unwrap_or_else(|| "?".into());
                // LIVE direct-path check, recomputed every frame: relay-only media does not exist yet, so a call with no validated direct path may sit Active-and-silent — the bar says so, and the warning self-clears the instant a punch validates (the engine bootstraps from the peer's first authenticated packet). No stored flag to go stale.
                let direct = peer.map_or(false, |k| k.validated_path.is_some());
                let ops = vsf::OSCILLATIONS_PER_SECOND as i64;
                let dur = match c.phase {
                    crate::call::CallPhase::Active => {
                        (vsf::eagle_time_oscillations() - c.phase_osc).max(0) / ops
                    }
                    _ => 0,
                };
                (c.phase, name, direct, pi, dur)
            });
        // Ringing shows a SECOND action (Decline) beside the primary — hoisted so the end-of-frame hit re-stamp agrees with the early paint without re-deriving the phase.
        let call_two_actions = call_overlay.as_ref().map_or(false, |(p, _, _, _, _)| {
            matches!(p, crate::call::CallPhase::Ringing)
        });
        // Full-screen call panel: Ringing (redesign 2026-08-30 — the compact bar squeezed Answer/Decline under the title band where Android's heads-up notification drops) and Active UNLESS minimized. Ended no longer exists — waves record by default and land straight in the conversation (2026-09-08). Minimized Active yields to the screen underneath (Phase 3 strip / the compact bar), so messaging + navigation stay live.
        let call_minimized = self.call_minimized;
        let call_fullscreen = match call_overlay.as_ref().map(|(p, _, _, _, _)| *p) {
            Some(crate::call::CallPhase::Ringing) => true,
            Some(crate::call::CallPhase::Active) => !call_minimized,
            _ => false,
        };
        // Duration string hoisted BEFORE the chrome borrow (`fmt_duration` reads `&self`; the `&mut self.chrome` borrow below would otherwise block it). Used by the full-screen timer + the Ended summary.
        let call_dur_str = call_overlay
            .as_ref()
            .map(|t| self.fmt_duration(t.4))
            .unwrap_or_default();
        // Receive-drought state for the Active status line (call_drought_tick owns the flag).
        let call_reconnecting = self.active_call.as_ref().is_some_and(|c| c.reconnecting);
        // Fleet call-presence chip text, hoisted like the duration (machine_name reads &self): shown only in the chip's own conversation, only while THIS device has no call UI of its own.
        let fleet_chip_text: Option<String> = self.fleet_call_elsewhere.and_then(|(_, dev, chip_peer)| {
            if self.active_call.is_some() || !matches!(self.state, AppState::Conversation) {
                return None;
            }
            let viewing = self
                .active_contact()
                .and_then(|ci| self.contacts.get(ci))
                .is_some_and(|c| c.handle_hash == chip_peer);
            if !viewing {
                return None;
            }
            match dev {
                Some(pk) => {
                    let seed = self.session.as_ref()?.identity_seed;
                    let name = super::machine_name(&pk, &seed, self.fleet_settings.as_ref());
                    Some(tr(Msg::CallChipElsewhere(&name)).into_owned())
                }
                None => Some(tr(Msg::CallChipElsewhereUnknown).into_owned()),
            }
        });
        // Ring-panel avatar: pre-scale the caller's avatar (or the identity gradient) to the panel diameter — done HERE (before the canvas borrows) because it needs &mut self. Cache keyed by diameter; dropped when nothing rings.
        if call_fullscreen {
            let unit_now = ReadyLayout::compute(buf_w, buf_h, ctx.viewport.ru).unit_height;
            let diameter = ((unit_now * 7.0) as usize).max(2);
            let stale = self
                .ring_avatar_scaled
                .as_ref()
                .map_or(true, |(d, _)| *d != diameter);
            if stale {
                if let Some((_, _, _, Some(pi), _)) = &call_overlay {
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

        // Content-scroll → background offset (hoisted before the chrome borrow, which takes `&mut self`). The background noise translates WITH the foreground content so the whole scene is one rigid vertical shift on scroll — the bg tracks whatever you're reading, and (once the host learns to scroll-copy) a scroll becomes a memcopy of the prior frame plus a repaint of just the newly-exposed slice instead of a full redraw. Sign matches the foreground pixel motion: Contacts moves rows UP as `contacts_scroll` grows (`row_top = … − contacts_scroll`) → texture shifts by `−contacts_scroll`; Conversation moves messages DOWN as `scroll_offset` grows (`y = … + scroll`) → texture shifts by `+scroll_offset`. Settings/ContactPanel keep the split-pane path below. `scroll_offset` is clamped elsewhere (tick clamps the stored conversation offset; contacts_scroll is clamped in the render block), so reading it raw here matches what the foreground draws.
        let content_bg_scroll: isize = match self.state {
            AppState::Ready => -self.contacts_scroll,
            AppState::Conversation => self
                .active_conversation
                .and_then(|id| self.conversations.iter().find(|v| v.id() == id))
                .map_or(0, |v| v.scroll_offset.round() as isize),
            _ => 0,
        };
        // Security page's fleet-of-one gate — hoisted here for the same reason as the scroll above: the pills draw inside the chrome borrow, and asking `self` a question there is a second borrow. Cheap (a filtered pass over sibling rows), and ONE definition shared with the action that would otherwise refuse the tap.
        let has_sibling_device = self.has_usable_sibling();
        // The standing bands are computed BEFORE the chrome borrow (they read plain state), then painted by a free fn on the two screens that show them.
        let standing_bands = self.standing_bands();
        let link_btn_visible = self.compose_link_available();
        let Some(chrome) = self.chrome.as_mut() else {
            return;
        };
        chrome.set_title(title_text);

        // Bg noise. `shimmer` is driven by `bg_scroll` and mixes into each row's starting colour — so the noise colour bias cycles as you scroll without changing the underlying pattern topology. `scroll_offset` is per-screen: Launch/Attest gets `0` (no vertical movement on the attest screen — shimmer only); future screens (Ready, Searching, Conversation) will pass `bg_scroll` so the noise pattern also translates with their page-scroll content. Phase 2+ branches on AppState to pick which.
        let bg_scroll = self.bg_scroll;
        let shimmer = bg_scroll as usize;
        let scroll_offset = content_bg_scroll; // 0 on Launch/Searching; Contacts/Conversation translate the texture with their content (see the hoist above).
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
        // Dozenal zoom is per-GROSS, not per-cent: no ×100, just base convert — 1.0× renders as dozenal 100 ("zila", = ×144), 2.0× as dozenal 200 ("zilor"). No % sign (percent is a decimal concept). Decimal mode keeps the familiar NN%.
        let zoom_text = match crate::num_base() {
            crate::NumBase::Dozenal => crate::dozenal_glyphs((ctx.viewport.ru * 144.0).round().max(0.0) as u32),
            // Hex zoom is per-256 by the same logic: 1.0× renders as hex 100.
            crate::NumBase::Hex => crate::hex_glyphs((ctx.viewport.ru * 256.0).round().max(0.0) as u32),
            crate::NumBase::Arabic => format!("{}%", (ctx.viewport.ru * 100.0).round().max(0.0) as u32),
        };
        let zoom_cx = buf_w as f32 * 0.5;
        let zoom_cy = zoom_size;
        // Split-borrow `ctx.damage` (consumed by rasterize_bg's first arg) and `ctx.text` (captured by the closure for the logo's text rendering). These are disjoint fields of `Context` so the borrow checker allows both reborrows simultaneously. The closure is non-`move` so the text reborrow ends when rasterize_bg returns, leaving `ctx.text` available for `rasterize_chrome` on the next line.
        let text = &mut *ctx.text;
        // Bg-first compose chain: noise paints opaque, the wave reads it for the `sqrt(c*scale + c_bg²)` blend, then the logo (glow / body / highlight) paints over both via legacy visible-RGB ops. Each step preserves α on the pixels it touches. The wave + logo are Launch-screen chrome — once attested the user shouldn't be staring at the wordmark every time they open the app, so Ready / Searching / Conversation get just the background noise and let their own widgets own the canvas.
        let on_launch = matches!(self.state, AppState::Launch(_));
        // ABOUT-PAGE SLAB (Nick 2026-09-02): the wave + wordmark SCROLL with the card but never SCALE (fixed window-proportional size, zoom-independent — content sits on a fixed slab instead of the logo warping with zoom). Drawn HERE in the bg pass because that's the only place the wave works: it quadrature-reads the noise beneath it, and fluor's under-blend made the old content-pass draw invisible ("suspiciously absent"). The content arm advances past the same slab height without painting.
        let about_slab_bands: Option<(usize, usize, isize, usize, isize, usize, usize, usize)> =
            if matches!(self.state, AppState::Settings(SettingsPage::About)) {
                let sl = SettingsLayout::compute(&ctx.viewport);
                let inset = sl.content_inset();
                let (unit, _) = about_slab(buf_w, buf_h, inset.w);
                // LOGICAL band: the attest proportions in slab units (air 0.75u, wave 6u, wordmark 3.5u overlapping by 2u), fixed size, positioned by the scroll — may extend above the pane; the clipped draws crop.
                let top = (inset.y - settings_content_scroll) as isize;
                let sx0 = inset.x.max(0.0) as usize;
                let sx1 = ((inset.x + inset.w).max(0.0) as usize).min(buf_w);
                let clip_y0 = inset.y.max(0.0) as usize;
                let clip_y1 = ((inset.y + inset.h).max(0.0) as usize).min(buf_h);
                (sx1 > sx0 && clip_y1 > clip_y0).then(|| {
                    (
                        sx0,
                        sx1,
                        top + (unit * ABOUT_SLAB_AIR) as isize,
                        (unit * ABOUT_SLAB_WAVE_H) as usize,
                        top + (unit * ABOUT_SLAB_LOGO_TOP) as isize,
                        (unit * ABOUT_SLAB_LOGO_H) as usize,
                        clip_y0,
                        clip_y1,
                    )
                })
            } else {
                None
            };
        let about_wave_phase = self.bg_scroll as f32 / ((1 << 7) as f32);
        // Faint dozenal version watermark shows on the ATTEST screen ONLY (Launch) — a quiet bottom-left mark while you sign in. Ready / Conversation stay clean; the About page carries the version in full (normal-white dozenal glyphs, tap to spell out). Never arabic anywhere.
        let show_version = on_launch;
        // Swap the noise base colour to (*theme::BG_BASE_WARNING) when the dual-ring vault flagged degraded this session — the noise pass already runs every frame so this changes a colour, not the pass count. None on the happy path keeps fluor's default green-dark BG_BASE.
        let bg_base = if self.vault_data_lost || self.vault_degraded {
            Some(*theme::BG_BASE_WARNING)
        } else {
            None
        };
        // The 1-px noise inset exists ONLY to clear the window perimeter hairline / shadow band — so gate it on whether that perimeter is actually drawn, which is exactly `!chrome.full_edge`. A windowed desktop draws the perimeter → inset. A maximized/fullscreen desktop goes full_edge (no perimeter) and Android forces full_edge too → paint to the screen edge, else a 1-px unpainted border shows. (Earlier this was hardcoded per-OS, so desktop-maximized still inset for a perimeter that wasn't there.) `|| cfg!(android)` keeps the Android always-fullscreen guarantee even on a transient pre-resize frame where full_edge hasn't synced yet.
        let bg_fullscreen = chrome.full_edge || cfg!(target_os = "android");
        // MEASURED settings scroll extent (the Flow pages): a converted page arm records (content_height, pane_height) here as it draws; applied to self.settings_content_extent after the borrows release — one frame stale, which the rubber-band tolerates, and it retires the hand-counted row estimates page by page.
        let mut measured_extent: Option<(f32, f32)> = None;
        // Stage marks for the >1s breakdown at the end of the frame — the flat "render took Nms" line named the SCREEN but not the STAGE, which stalled the 2026-08-21 hang hunt (5.8-8.8s Conversation renders, no idea where inside).
        let mark_pre = std::time::Instant::now();
        chrome.rasterize_bg(ctx.damage, |canvas| {
            // LOGO first (an under() layer — first-drawn claims its pixels; the noise then composes beneath). The wave does NOT draw here: pre-noise it lands on α=0 pixels the noise fully replaces — the old both-blocks double-draw burned a full wave AND a full 3-raster logo per bg pass for nothing (Nick 2026-09-02).
            if on_launch {
                paint_photon_logo(canvas, text, logo_rect);
            }
            // About slab wordmark — SAME rule as the launch logo: an under() layer must claim its pixels BEFORE the noise paints them opaque (drawing it post-noise under()'d against opaque pixels = invisible, the second vanished-wordmark bug).
            if let Some((sx0, sx1, _, _, logo_top, logo_h, clip_y0, clip_y1)) = about_slab_bands {
                paint_photon_logo_clipped(canvas, text, sx0, sx1, logo_top, logo_h, clip_y0, clip_y1);
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
            // WAVE after the noise — an RMW quadrature-add that reads the now-opaque noise as its base. One call, post-noise, is the whole spectrum band.
            if on_launch {
                chromatic_wave(canvas, spectrum_rect, phase, period_scale);
            }
            // The About slab: CLIPPED crop-not-shrink variants — the pattern/wordmark stay anchored to the full logical band (top goes negative as the card scrolls) and only pane-visible rows paint. Shrinking the rects instead RESCALED both (the "wave scales when scrolling" + vanished-wordmark field bugs; the shrunken-rect span math also underflowed).
            if let Some((sx0, sx1, wave_top, wave_h, _, _, clip_y0, clip_y1)) = about_slab_bands {
                chromatic_wave_clipped(canvas, sx0, sx1, wave_top, wave_h, clip_y0, clip_y1, about_wave_phase, 1.0);
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
            _rt.mark("bg+chrome");
            if call_fullscreen {
                // ── FULL-SCREEN CALL PANEL ── Ringing / Active / Ended, painted over whatever screen was up; every element scales off `unit` (zoom-honest, no fixed pixels).
                let (phase, name, direct, pi) = match &call_overlay {
                    Some((ph, n, d, p, _)) => (*ph, n.clone(), *d, *p),
                    None => (
                        crate::call::CallPhase::Ringing,
                        String::from("?"),
                        false,
                        None,
                    ),
                };
                let w = buf_w as f32;
                let h = buf_h as f32;
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
                if let Some((diam, px)) = self.ring_avatar_scaled.as_ref() {
                    crate::ui::avatar_render::draw_avatar(
                        &mut canvas, acx, acy, avatar_r, px, *diam, None,
                    );
                }
                // THE PRESENCE RING ON THE CALL SCREEN (Nick 2026-09-11): the path the wave is actually on — cyan the same LAN, blue radio-direct, green across the internet, amber while the engine waits on the sentinel with no direct path — and the contact's own tier while it still rings.
                {
                    let ring = match crate::call::call_tx_addr() {
                        Some(a) if a != crate::network::status::RELAY_ADDR => super::ring_colour_of(match a.ip().to_canonical() {
                            std::net::IpAddr::V4(v4) if crate::network::traverse::gather::is_wfd_subnet(v4) => super::ConnTier::Wfd,
                            std::net::IpAddr::V4(v4) if crate::network::traverse::gather::is_private_ipv4(v4) => super::ConnTier::Lan,
                            // An IPv6 peer on OUR /64 is the same LAN (field 2026-09-12: a same-room wave ran on the router's global v6 at 10 ms and read green).
                            std::net::IpAddr::V6(v6) if self.our_reflexive.map_or(false, |o| matches!(o.ip().to_canonical(), std::net::IpAddr::V6(ours) if ours.segments()[..4] == v6.segments()[..4])) => super::ConnTier::Lan,
                            _ => super::ConnTier::Wan,
                        }),
                        Some(_) => super::ring_colour_of(super::ConnTier::Relay),
                        None => pi.map(|i| super::ring_tier_colour(&self.contacts[i], true)).unwrap_or(super::ring_colour_of(super::ConnTier::Relay)),
                    };
                    paint::draw_circle(&mut canvas, acx, acy, avatar_r + super::ring_thickness(avatar_r), ring, None);
                }
                // The living circle — ONLY while Ringing (Active/Ended sit calm): one perfect circle BEHIND the avatar (paint order per Nick: avatar, circle, text/buttons, background — later paints compose under earlier, so the avatar covers it and it washes over the text where it reaches). Digest-keyed waveforms move it, a spin decouples the offsets from the axes, a fourth scales it, a fifth breathes its opacity (ui::ring_rim); relationship colour, same as the name. Pure function of (digest, now) — the wake_at tick keeps frames coming while Ringing.
                if matches!(phase, crate::call::CallPhase::Ringing) {
                    if let Some(digest) = pi.and_then(|i| {
                        let c = &self.contacts[i];
                        self.session.as_ref().map(|s| {
                            relationship_digest(
                                &c.handle_hash,
                                &crate::crypto::clutch::identity_party_id(&s.identity_seed),
                            )
                        })
                    }) {
                        let orbit = crate::ui::ring_rim::orbit_for(&digest);
                        let t_secs = vsf::eagle_time_oscillations() as f64
                            / vsf::OSCILLATIONS_PER_SECOND as f64;
                        let m = crate::ui::ring_rim::sample(&orbit, t_secs);
                        // Edge budget (Nick 2026-09-04): the rim lives roughly 31/32..17/16 of the avatar radius — mostly peeking, sometimes swallowed. Radius carries ±1/64 of it, the offset the remaining ~1.5/64 (×√2 when both axes peak lands the extremes on the budget).
                        let r = avatar_r * (65.0 + m.scale) / 64.0;
                        let a = (0x28 as f32 + m.opacity * 0x38 as f32) as u32;
                        paint::draw_circle(
                            &mut canvas,
                            acx + m.dx * avatar_r * (1.5 / 64.0),
                            acy + m.dy * avatar_r * (1.5 / 64.0),
                            r,
                            (a << 24) | (colour & 0x00FF_FFFF),
                            None,
                        );
                    }
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
                // Status / timer line beneath the name. Active shows the LIVE call timer (per-frame recompute from phase_osc); Ended a frozen call summary; Ringing the incoming cue. Oxanium so the base-aware `fmt_duration` (Phase 4 dozenal) resolves its glyphs.
                let status_line = match phase {
                    crate::call::CallPhase::Ringing => {
                        if direct {
                            tr(Msg::IncomingCall).into_owned()
                        } else {
                            tr(Msg::IncomingCallNoPath).into_owned()
                        }
                    }
                    crate::call::CallPhase::Active => {
                        if call_reconnecting {
                            tr(Msg::CallReconnecting).into_owned()
                        } else if direct {
                            // Glyph + duration only (no words) — nothing to translate, stays a raw format.
                            format!("\u{260E} {}", call_dur_str)
                        } else {
                            tr(Msg::CallActiveNoPath(&call_dur_str)).into_owned()
                        }
                    }
                    crate::call::CallPhase::Outgoing => tr(Msg::CallingName(&name)).into_owned(),
                };
                ctx.text.draw_text_center(
                    &mut canvas,
                    &status_line,
                    acx,
                    acy + avatar_r + unit * 2.2,
                    &TextStyle::new(unit * 0.62, *theme::STATUS_TEXT_COLOUR).font("Oxanium"),
                    None,
                    None,
                );
                // RUNNING STATS on every build (Nick 2026-09-11): the rung by its Spaceballs name, the round trip as a frequency in the current base, the loss ring, the buffer — refreshed by the engine once a second while the wave runs.
                if matches!(phase, crate::call::CallPhase::Active) {
                    let rtt = crate::call::LAST_LINK_RTT_MS.load(std::sync::atomic::Ordering::Relaxed);
                    if rtt > 0 {
                        let rung = crate::call::engine::tier_name(crate::call::LAST_LINK_TIER.load(std::sync::atomic::Ordering::Relaxed) as usize);
                        let freq = crate::link_freq_label(rtt);
                        let loss = crate::fmt_num(crate::call::LAST_LINK_LOSS.load(std::sync::atomic::Ordering::Relaxed));
                        let buf = crate::fmt_num(crate::call::LAST_LINK_TARGET.load(std::sync::atomic::Ordering::Relaxed));
                        let line = tr(Msg::CallLiveStats { rung, freq: &freq, loss: &loss, buf: &buf });
                        ctx.text.draw_text_center(
                            &mut canvas,
                            &line,
                            acx,
                            acy + avatar_r + unit * 3.0,
                            &TextStyle::new(unit * 0.5, *theme::LABEL_COLOUR).font("Oxanium"),
                            None,
                            None,
                        );
                    }
                }
                // Actions: bottom third, thumb-reach, decline LEFT answer RIGHT with a generous gap — and bottom-anchored so an Android heads-up banner (which owns the top) can never cover them.
                let bw = w * 0.34;
                let bh = unit * 2.4;
                let by = h - bh * 0.5 - unit * 1.5;
                let bfont = unit * 0.75;
                match phase {
                    crate::call::CallPhase::Ringing => {
                        // Reject LEFT (silent: no signal leaves the fleet), Decline MIDDLE (tells them), Wave back RIGHT — three across, thumb-reach.
                        let bw = w * 0.27;
                        if let Some(b) = self.call_reject_btn.as_mut() {
                            b.set_rect(w * 0.5 - bw - unit * 0.6, by, bw, bh);
                            b.set_font_size(bfont * 0.9);
                            b.set_label(tr(Msg::Reject));
                            b.set_fill(Some(theme::PILL_GREY.0));
                            let id = b.hit_id();
                            b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                        }
                        if let Some(b) = self.call_decline_btn.as_mut() {
                            b.set_rect(w * 0.5, by, bw, bh);
                            b.set_font_size(bfont);
                            b.set_label(tr(Msg::Decline));
                            b.set_fill(Some(*theme::CALL_DANGER_FILL));
                            b.set_hover_fill(Some(*theme::CALL_DANGER_HOVER));
                            b.set_held_fill(Some(*theme::CALL_DANGER_HOVER));
                            let id = b.hit_id();
                            b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                        }
                        if let Some(b) = self.call_action_btn.as_mut() {
                            b.set_rect(w * 0.5 + bw + unit * 0.6, by, bw, bh);
                            b.set_font_size(bfont);
                            // "Wave back" (Nick 2026-09-09): answering is choosing AUDIO — the beam answer sits above as its own choice, so the callee picks audio-only even when the caller beams.
                            b.set_label(tr(Msg::WaveBack));
                            b.set_enabled(true);
                            b.set_fill(Some(*theme::CALL_ACCEPT_FILL));
                            b.set_hover_fill(Some(*theme::CALL_ACCEPT_HOVER));
                            b.set_held_fill(Some(*theme::CALL_ACCEPT_HOVER));
                            let id = b.hit_id();
                            b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                        }
                        // Beam back — the video answer, a STUB greyed out until video lands; centred above the decline/answer pair.
                        if let Some(b) = self.call_beam_back_btn.as_mut() {
                            b.set_rect(w * 0.5, by - bh - unit * 0.6, bw, bh * 0.85);
                            b.set_font_size(bfont * 0.9);
                            b.set_label(tr(Msg::BeamBack));
                            b.set_enabled(false);
                            let id = b.hit_id();
                            b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                        }
                    }
                    _ => {
                        // Active in-call screen: a secondary row (+Handle / ‹ Contact) above the primary End call. Add-handle is a stub; ‹ Contact minimizes. Speaker toggle PARKED (Nick 2026-09-03, headset-only + engine output pad) — restore the third slot when a real speaker route lands.
                        let sw = w * 0.29;
                        let sh = unit * 2.0;
                        let sfont = unit * 0.58;
                        let sy = by - bh - unit * 0.6;
                        // let spk_on = self.call_speaker_on;
                        // if let Some(b) = self.call_speaker_btn.as_mut() {
                        //     b.set_rect(w * 0.5 - sw - unit * 0.4, sy, sw, sh);
                        //     b.set_font_size(sfont);
                        //     b.set_label(tr(if spk_on { Msg::SpeakerToggleOn } else { Msg::SpeakerToggleOff }));
                        //     let id = b.hit_id();
                        //     b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                        // }
                        // Beam toggle — switch this side to video mid-wave; a STUB greyed out until video lands, one row above the secondary pair.
                        if let Some(b) = self.call_beam_back_btn.as_mut() {
                            b.set_rect(w * 0.5, sy - sh - unit * 0.4, sw, sh);
                            b.set_font_size(sfont);
                            b.set_label(tr(Msg::BeamToggle));
                            b.set_enabled(false);
                            let id = b.hit_id();
                            b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                        }
                        if let Some(b) = self.call_addhandle_btn.as_mut() {
                            b.set_rect(w * 0.5 - sw - unit * 0.2, sy, sw, sh);
                            b.set_font_size(sfont);
                            b.set_label(tr(Msg::AddHandle));
                            let id = b.hit_id();
                            b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                        }
                        if let Some(b) = self.call_back_btn.as_mut() {
                            b.set_rect(w * 0.5 + unit * 0.2, sy, sw, sh);
                            b.set_font_size(sfont);
                            b.set_label(tr(Msg::BackToContact));
                            let id = b.hit_id();
                            b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                        }
                        if let Some(b) = self.call_action_btn.as_mut() {
                            b.set_rect(w * 0.5, by, w * 0.5, bh);
                            b.set_font_size(bfont);
                            b.set_label(tr(Msg::EndCall));
                            b.set_enabled(true);
                            b.set_fill(Some(*theme::CALL_DANGER_FILL));
                            b.set_hover_fill(Some(*theme::CALL_DANGER_HOVER));
                            b.set_held_fill(Some(*theme::CALL_DANGER_HOVER));
                            let id = b.hit_id();
                            b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                        }
                    }
                }
                // OPAQUE background LAST: fluor composes later paints UNDER earlier ones, so the backdrop must follow the panel's own elements or it covers them — painting it FIRST produced a solid-black dead screen on desktop (field 2026-08-31, the very first Linux ring after the redesign). Painted last it slots exactly one layer beneath the pulse/avatar/name/buttons and still blots out whatever screen was up (α 0xFF, darkness 0xFF ⇒ solid black; the translucent-wash ghosting fix holds).
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
            } else if let Some((phase, name, direct, _pi, _dur)) = &call_overlay {
                let phase = *phase;
                let bar_w = buf_w as f32 * 0.9; // window-relative width — a bar spans the window
                let x0 = (buf_w as f32 - bar_w) * 0.5;
                let gap = unit * 0.5;
                let mut status = match phase {
                    crate::call::CallPhase::Outgoing => tr(Msg::CallingName(name)).into_owned(),
                    crate::call::CallPhase::Ringing => tr(Msg::CallBarCalling(name)).into_owned(),
                    crate::call::CallPhase::Active => tr(Msg::CallBarInCall(name)).into_owned(),
                };
                // No validated direct path in a live phase → say so on the bar (media may be silent until a punch lands; the warning disappears live when it does). The ⚠ is safe everywhere: fonts are fully bundled + deterministic (fluor's explicit-db TextRenderer, zero system-font pulls — verified 2026-08-20), and Noto Sans Symbols 2 covers U+26A0 in the same 2600 block as the field-proven ☎.
                if !direct {
                    status.push_str(&tr(Msg::NoDirectPathSuffix));
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
                let a_label = tr(match phase {
                    crate::call::CallPhase::Ringing => Msg::Answer,
                    _ => Msg::HangUp,
                });
                if let Some(b) = self.call_action_btn.as_mut() {
                    b.set_rect(ax + action_w * 0.5, cy, action_w, pill_h);
                    b.set_font_size(call_font);
                    b.set_label(a_label);
                    b.set_enabled(true);
                    // Traffic light by semantics: Answer/Keep green, HangUp red — same law as the full panel.
                    let accept = matches!(phase, crate::call::CallPhase::Ringing);
                    b.set_fill(Some(if accept { *theme::CALL_ACCEPT_FILL } else { *theme::CALL_DANGER_FILL }));
                    b.set_hover_fill(Some(if accept { *theme::CALL_ACCEPT_HOVER } else { *theme::CALL_DANGER_HOVER }));
                    b.set_held_fill(Some(if accept { *theme::CALL_ACCEPT_HOVER } else { *theme::CALL_DANGER_HOVER }));
                    let id = b.hit_id();
                    b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                }
                if call_two_actions {
                    let dx = x0 + status_w + gap * 2. + action_w;
                    let d_label = tr(Msg::Decline);
                    if let Some(b) = self.call_decline_btn.as_mut() {
                        b.set_rect(dx + action_w * 0.5, cy, action_w, pill_h);
                        b.set_font_size(call_font);
                        b.set_label(d_label);
                        b.set_fill(Some(*theme::CALL_DANGER_FILL));
                        b.set_hover_fill(Some(*theme::CALL_DANGER_HOVER));
                        b.set_held_fill(Some(*theme::CALL_DANGER_HOVER));
                        let id = b.hit_id();
                        b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                    }
                }
            } else if matches!(self.state, AppState::Conversation) && call_pill_show {
                // The ☎ start pill — top-right of the conversation, mirroring the "‹ Contacts" back arrow on the left; sized off `unit` so it matches the back arrow at every zoom. Shown for any friend convo (discoverable), dimmed (disabled) until the friend is reachable — the disabled label dim reads as "can't call yet".
                let pill_w = unit * 5.;
                let px = buf_w as f32 - pill_w - unit; // top-right, one unit of margin from the edge
                // Sit on the SAME vertical as the back arrow (`buf_h·0.06 + unit`) — lower than the old pinned `cy` — and slide up-under with the message scroll (`conv_topbar_off`) exactly like the back arrow, so the whole top bar is one browser-toolbar band. When it scrolls above the top its hit rect leaves the surface with it (no ghost taps).
                let bar_h = buf_h as f32 * 0.06 + unit + pill_h;
                let bar_off = self.conv_topbar_off.min(bar_h);
                let call_cy = buf_h as f32 * 0.06 + unit - bar_off;
                if let Some(b) = self.call_start_btn.as_mut() {
                    b.set_rect(px + pill_w * 0.5, call_cy, pill_w, pill_h);
                    b.set_font_size(call_font);
                    b.set_enabled(call_pill_enabled);
                    let id = b.hit_id();
                    b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                }
                // Beam (video) stub — sits left of Wave, permanently disabled until video lands.
                if let Some(b) = self.call_beam_btn.as_mut() {
                    b.set_rect(px - pill_w * 0.5 - unit * 0.4, call_cy, pill_w, pill_h);
                    b.set_font_size(call_font);
                    b.set_enabled(false);
                    let id = b.hit_id();
                    b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                }
                // Fleet call-presence chip: a wave is live on another of our devices — informational v1, the join/switch affordance lands with handoff.
                if let Some(txt) = &fleet_chip_text {
                    ctx.text.draw_text_center(
                        &mut canvas,
                        txt,
                        buf_w as f32 * 0.5,
                        call_cy,
                        &TextStyle::new(call_font * 0.8, *theme::SEARCH_FOUND_COLOUR).font("Oxanium"),
                        None,
                        None,
                    );
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
            let status: Option<(std::borrow::Cow<'_, str>, u32)> = if self.launch_add_mode
                && !self.add_join_status.is_empty()
            {
                Some((self.add_join_status.as_str().into(), (*theme::STATUS_TEXT_COLOUR)))
            } else {
                match launch_state {
                        LaunchState::Attesting => {
                            Some((tr(Msg::Attesting), (*theme::STATUS_TEXT_COLOUR)))
                        }
                        LaunchState::Error(msg) if !msg.is_empty() => {
                            Some((msg.as_str().into(), (*theme::ERROR_TEXT_COLOUR)))
                        }
                        // Terminal brick: the fleet locked this device. Red, dead-end — no handle re-type helps (the identity is real, the fleet owner marked the hardware stolen), only an unlock from another of the owner's devices clears it.
                        LaunchState::Locked => Some((
                            tr(Msg::RevokedByFleet),
                            (*theme::ERROR_TEXT_COLOUR),
                        )),
                        // Up-front hint: a bound device in Fresh gets the resume-or-wipe line in the STATUS colour (not error-red) so the restriction is visible before any submit.
                        // Confirm/KnownHandle fall thru to None and keep their own bands.
                        LaunchState::Fresh if device_bound => Some((
                            tr(Msg::IdentityCarriedHint),
                            (*theme::STATUS_TEXT_COLOUR),
                        )),
                        _ => None,
                    }
            };
            if let Some((text, colour)) = status {
                let error_rect = attest.error;
                if !error_rect.is_empty() {
                    let region_h = (error_rect.y1 - error_rect.y0) as f32;
                    let region_w = (error_rect.x1 - error_rect.x0) as f32;
                    let cx = (error_rect.x0 + error_rect.x1) as f32 * 0.5;
                    let cy = (error_rect.y0 + error_rect.y1) as f32 * 0.5;
                    // Word wrap (Nick 2026-09-04): greedy against the band width — a message that outgrows one line wraps at a step-smaller size instead of running off the sides.
                    let mut size = region_h * 0.5;
                    let style = |sz: f32| TextStyle::new(sz, colour).weight(500).font("Oxanium");
                    let wrap = |text: &str, sz: f32, tx: &mut fluor::text::TextRenderer| -> Vec<String> {
                        let max_w = region_w * 0.96;
                        let mut lines = Vec::new();
                        let mut cur = String::new();
                        for word in text.split_whitespace() {
                            let cand = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
                            if !cur.is_empty() && tx.measure_text(&cand, &style(sz)) > max_w {
                                lines.push(std::mem::take(&mut cur));
                                cur = word.to_string();
                            } else {
                                cur = cand;
                            }
                        }
                        if !cur.is_empty() {
                            lines.push(cur);
                        }
                        lines
                    };
                    let mut lines = wrap(&text, size, ctx.text);
                    if lines.len() > 1 {
                        size = region_h * 0.38;
                        lines = wrap(&text, size, ctx.text);
                    }
                    let pitch = size * 1.25;
                    let mut y = cy - pitch * (lines.len() as f32 - 1.0) * 0.5;
                    for line in &lines {
                        ctx.text.draw_text_center(&mut canvas, line, cx, y, &style(size), None, None);
                        y += pitch;
                    }
                }
            }

            // Permanence warning block (`LaunchState::Confirm`) — drawn in the empty 6-unit band BELOW the attest button, sized with the same ru-scaled math as the join-words rows. The headline takes the error colour for gravity; the detail lines stay in status grey. The button above now reads "Yes — forever"; editing the handle cancels back to Fresh.
            if matches!(launch_state, LaunchState::Confirm) && !self.launch_add_mode {
                let tb_h = (attest.textbox.y1 - attest.textbox.y0) as f32;
                let line_h = (tb_h * 0.45).min(buf_w as f32 / 22.0);
                let cx = buf_w as f32 * 0.5;
                let mut y = attest.attest.y1 as f32 + line_h * 1.6;
                // What's permanent is the IDENTITY, not the handle: a handle is a mutable label, but attesting mints crypto roots with no password / reset / recovery. Ownership binds to the HUMAN, not the hardware — the first person to attest owns that identity, while devices stay replaceable thru the fleet chain (remove the first device whenever, as long as another is added first). The warning must not mis-teach "this phone owns it" NOR "this name is a life sentence" — it's the identity behind it that can't be undone.
                // One catalog passage; line 0 is the headline and takes the error colour, the detail lines stay in status grey.
                let passage = tr(Msg::PermanenceWarning);
                for (i, line) in passage.lines().enumerate() {
                    let colour = if i == 0 {
                        *theme::ERROR_TEXT_COLOUR
                    } else {
                        *theme::STATUS_TEXT_COLOUR
                    };
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
                // Same passage discipline as the permanence block: line 0 = headline in error colour, the rest in status grey.
                let passage = tr(Msg::KnownHandleWarning);
                for (i, line) in passage.lines().enumerate() {
                    let colour = if i == 0 {
                        *theme::ERROR_TEXT_COLOUR
                    } else {
                        *theme::STATUS_TEXT_COLOUR
                    };
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
                    &tr(Msg::PickAnotherName),
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
                    &tr(Msg::ItsMineShowWords),
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
                    let mut y = attest.error.y1 as f32 + line_h * 1.0;
                    // Guidance FIRST (Nick 2026-09-06): say what to do, then hand over the payload — the instructions end with a colon pointing at the words below. Nearby-tap and far-away-words are the user's two experiences; LAN vs BLE is our plumbing, never their decision.
                    {
                        let gsize = line_h * 0.62;
                        let s = tr(Msg::LaunchJoinInstructions);
                        for line in s.lines() {
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
                        y += line_h * 0.5;
                    }
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
                            &tr(Msg::ThisDeviceName(&name)),
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
                            (tr(Msg::WordsCopied), *theme::STATUS_TEXT_COLOUR)
                        } else {
                            (tr(Msg::CopyWords), *theme::CONTACT_NAME_COLOUR)
                        };
                        ctx.text.draw_text_center(
                            &mut canvas,
                            &clabel,
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
                    // The confirm reminder closes the block: the add is approved on the OTHER device.
                    {
                        y += line_h * 0.9;
                        let gsize = line_h * 0.62;
                        ctx.text.draw_text_center(
                            &mut canvas,
                            &tr(Msg::LaunchJoinConfirmNote),
                            cx,
                            y,
                            &TextStyle::new(gsize, fluor::theme::HINT_COLOUR).font("Oxanium"),
                            None,
                            None,
                        );
                        y += gsize * 1.5;
                    }
                    // "Start fresh (wipe this device)" — the secondary escape: a device that was REMOVED from a fleet can't attest (can't reach the Security page), so this is its only self-clean path. Two-tap confirm. Hit-stamped so a tap on Android works (no chords there). Pushed well below the add guidance so it reads as the edge case, not the main action.
                    {
                        y += line_h * 1.4;
                        let sf_label = tr(if self.join_startfresh_armed {
                            Msg::StartFreshArmed
                        } else {
                            Msg::StartFreshIdle
                        });
                        let sf_size = line_h * 0.7;
                        let sf_colour = if self.join_startfresh_armed {
                            *theme::ERROR_TEXT_COLOUR
                        } else {
                            fluor::theme::HINT_COLOUR
                        };
                        ctx.text.draw_text_center(
                            &mut canvas,
                            &sf_label,
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
                    let hint_label = tr(if self.launch_add_mode {
                        Msg::HandleHintJoin
                    } else {
                        Msg::HandleHint
                    });
                    ctx.text.draw_text_center(
                        &mut canvas,
                        &hint_label,
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
                        &tr(Msg::LockedRetry),
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
            // The standing bands ride the attest screen too (Nick 2026-09-09: contact and attest pages only). Unit = the span term the Ready layout uses, so the two screens agree.
            {
                let span = buf_w.min(buf_h) as f32;
                let band_h = span / 32.0 * ctx.viewport.ru * 1.5;
                draw_standing_bands(&standing_bands, &mut canvas, ctx.text, buf_w, buf_h, band_h);
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
                    &tr(Msg::AvatarDropHint),
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
                        &tr(Msg::SearchPlaceholder),
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
            let block_h: isize = matching
                .iter()
                .map(|&ci| contact_row_height(row_h, self.contact_row_lines.get(ci).map_or(1, |l| l.len())))
                .sum();
            let block_bottom_at_zero = rows.y0 as isize + block_h;
            let block_end = block_bottom_at_zero + row_h * 2;
            let max_scroll = (block_end - buf_h as isize).max(0);
            if self.contacts_scroll > max_scroll {
                self.contacts_scroll = max_scroll;
            }

            // Row geometry: avatar on the left with a half-radius margin, name to its right.
            let avatar_cx = rows.x0 as f32 + avatar_r * 1.5;
            let text_x = avatar_cx + avatar_r * 1.5;
            let text_size = row_h as f32 * 0.5;
            // +1 on top of the proportional thickness so the presence/online ring keeps a visible annulus at small avatar sizes (where `avatar_r * 0.0375` floors at the 1px min and the ring all but vanishes). One extra pixel is imperceptible on large avatars, load-bearing on tiny ones.
            let ring_thickness = super::ring_thickness(avatar_r);
            // Handle names render in each contact's relationship colour (spaghettify per visible row is microseconds; revisit with a cache if contact lists ever get huge). `our_handle_hash` is bound above the sort — one derivation for the ordering and the rows.
            // Rows stack at their OWN heights (a wrapped name grows its row); `row_cursor` is the next row's top at scroll zero.
            let mut row_cursor = rows.y0 as isize;
            for &ci in matching.iter() {
                let name_lines: Vec<String> = self.contact_row_lines.get(ci).cloned().unwrap_or_default();
                let rh = contact_row_height(row_h, name_lines.len().max(1));
                let row_top_at_zero = row_cursor;
                row_cursor += rh;
                // Use the SAME `scroll` snapshot the avatar / hint / search box / separator read (captured up top, before the down-scroll clamp below mutated `self.contacts_scroll`). Reading the live field here made the rows lag the rest of the block by the clamp delta: on an up-scroll past rest the avatar + textbox dragged with the rubber-band overshoot (they read the snapshot) but the names sat still (they read the post-clamp value). One block, one offset.
                let row_top = row_top_at_zero - scroll as isize;
                if row_top + rh <= 0 || row_top >= buf_h as isize {
                    continue; // fully outside the visible content area (rows now scroll up to the top, not just `rows.y0`)
                }
                // Hover/press vocabulary (block tints vetoed): hover = the NAME goes heavier + the presence ring strokes 1px wider; press = the logo's white-glow halo blooms behind the name. No fills, no deltas — weight, stroke, and light.
                let row_hit_here = self.contact_hit_base.wrapping_add(ci as HitId);
                let row_pressed =
                    ci < 256 && ctx.pressed_hit != HIT_NONE && ctx.pressed_hit == row_hit_here;
                let row_hovered = row_pressed
                    || (ci < 256 && ctx.pressed_hit == HIT_NONE && self.hover_hit == row_hit_here);
                // The avatar centres on the WHOLE row block, so a wrapped name sits balanced beside it (Nick 2026-09-09).
                let cy = (row_top + rh / 2) as f32;

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
                // The wrapped name lines (from the pre-borrow wrap), stacked about the row centre at the line step.
                let line_step = contact_line_step(row_h);
                let line_cy = |k: usize| cy - (name_lines.len() as f32 - 1.0) * 0.5 * line_step + k as f32 * line_step;
                for (k, line) in name_lines.iter().enumerate() {
                    ctx.text.draw_text_left(
                        &mut canvas,
                        line,
                        text_x,
                        line_cy(k),
                        &row_style,
                        Some(rows_clip),
                        None,
                    );
                }
                if row_pressed {
                    // Press = the wordmark's halo, scoped to this row — composited AFTER the name (under() = topmost paints first, so program-order-later lands BENEATH the glyphs; the logo calls its glow last for the same reason — glow-first blew the text out to white). Full-width band like the wordmark, so the shared blur math holds.
                    let band_top = row_top.max(0) as usize;
                    let band_h =
                        ((row_top + rh).min(buf_h as isize) as usize).saturating_sub(band_top);
                    if band_h >= 2 {
                        let mut scratch = vec![0u8; buf_w * band_h];
                        for (k, line) in name_lines.iter().enumerate() {
                            ctx.text.draw_text_left_legacy(
                                &mut scratch,
                                buf_w as u32,
                                band_h as u32,
                                line,
                                text_x,
                                line_cy(k) - band_top as f32,
                                text_size,
                                row_weight,
                                vec![0xB0],
                                0,
                                "Oxanium",
                            );
                        }
                        crate::ui::photon_logo::blur_horizontal_soft(&mut scratch);
                        crate::ui::photon_logo::blur_vertical_soft(&mut scratch, buf_w, band_h);
                        crate::ui::photon_logo::composite_glow_white(
                            canvas.pixels,
                            buf_w,
                            band_top as isize,
                            0,
                            buf_h,
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
                        (row_top + rh).min(buf_h as isize),
                        row_hit,
                    );
                }
            }

            // Standing bands (storage, auto-attest, clock, update) stacked from the bottom — one list, one painter (standing_bands / draw_standing_bands).
            draw_standing_bands(&standing_bands, &mut canvas, ctx.text, buf_w, buf_h, ready_layout.unit_height * 1.5);

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
                // A chosen name's line returns are honoured (Nathan's has one on purpose): the lines stack centred on the header row.
                let panel_name = super::contact_visible_name(contact, self.session.as_ref().map(|se| &se.identity_seed), self.fleet_settings.as_ref());
                let panel_lines: Vec<&str> = panel_name.split('\n').collect();
                let panel_step = hspan * 1.15;
                for (k, line) in panel_lines.iter().enumerate() {
                    ctx.text.draw_text_center(
                        &mut canvas,
                        line,
                        layout.content.x,
                        layout.header.center_y() - (panel_lines.len() as f32 - 1.0) * 0.5 * panel_step + k as f32 * panel_step,
                        &TextStyle::new(hspan, name_colour)
                            .weight(600)
                            .font("Oxanium"),
                        None,
                        None,
                    );
                }

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
                        &tr(Msg::SettingsBack),
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
                    // Full rail column, up to the rail's true top — no padding (Nick 2026-09-02).
                    paint::fill_rect(
                        &mut canvas,
                        layout.rail.x as isize,
                        layout.rail.y as isize,
                        layout.rail.w as isize,
                        (r.bottom() - layout.rail.y) as isize,
                        fill,
                        None,
                        None,
                    );
                    restamp_hit_rect(
                        &mut chrome.hit_test_map,
                        buf_w,
                        buf_h,
                        layout.rail.x as isize,
                        layout.rail.y as isize,
                        layout.rail.right() as isize,
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
                        &tr(Msg::ContactPageName(*p)),
                        r.x + rspan * 0.6,
                        r.center_y(),
                        &TextStyle::new(rspan, colour)
                            .weight(if active { 600 } else { 400 })
                            .font("Oxanium"),
                        Some(pages_clip),
                        None,
                    );
                    if held {
                        // Full rail column, edge to edge (Nick 2026-09-02).
                        paint::fill_rect(
                            &mut canvas,
                            layout.rail.x as isize,
                            r.y as isize,
                            layout.rail.w as isize,
                            r.h as isize,
                            fluor::theme::BUTTON_HELD,
                            Some(pages_clip),
                            None,
                        );
                    } else if active {
                        // Half the old separator opacity — the bright hint read too loud.
                        paint::fill_rect(
                            &mut canvas,
                            layout.rail.x as isize,
                            r.y as isize,
                            layout.rail.w as isize,
                            r.h as isize,
                            theme::RAIL_ACTIVE_COLOUR,
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
                            // +1 keeps the presence ring visible at small avatar sizes (see the contacts-row `ring_thickness`).
                            avatar_r + super::ring_thickness(avatar_r),
                            ring,
                            Some(content_clip),
                        );
                        let shared_name = if is_self {
                            tr(Msg::OwnNotesConversation)
                        } else if contact.published_name.is_empty() {
                            tr(Msg::NameNotShared)
                        } else {
                            tr(Msg::NameShared(&contact.published_name))
                        };
                        let shared_avatar: std::borrow::Cow<'_, str> = if is_self {
                            "".into()
                        } else if contact.avatar_pin == [0u8; 64] {
                            tr(Msg::AvatarNotShared)
                        } else {
                            tr(Msg::AvatarShared)
                        };
                        let identity_line = if is_self {
                            tr(Msg::SelfNoCeremony)
                        } else if contact.identity_superseded {
                            tr(Msg::ReclaimedStranger)
                        } else if contact.identity_ended {
                            tr(Msg::IdentityEndedByOwner)
                        } else if contact.pinned_genesis != [0u8; 32] {
                            tr(Msg::ContactFleetPinned(contact.fleet_members.len().max(1)))
                        } else {
                            tr(Msg::IdentityNotFolded)
                        };
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[5],
                            &tr(Msg::WhatTheyShare),
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
                            &tr(Msg::YouIdentity),
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
                            &tr(Msg::PublishedNameExplainer(&crate::network::fgtw::fleet::keyed_pseudonym(&contact.handle_hash))),
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
                            Some(true) => tr(Msg::HistoryComplete),
                            Some(false) => tr(Msg::HistorySyncing),
                            None => tr(Msg::HistoryIdle),
                        };
                        let chain_line = if is_self {
                            tr(Msg::SelfNoChain)
                        } else if contact.chain_woven {
                            tr(Msg::ChainWoven)
                        } else {
                            contact_status_line(
                                contact,
                                self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes()),
                                self.session.as_ref().map(|se| &se.identity_seed),
                                self.contacts.iter().any(|s| s.is_sibling),
                            )
                            .replace('\n', " \u{00b7} ")
                            .into()
                        };
                        // NAME the path, don't just colour the avatar ring with it (Nick 2026-09-08). Same resolution the ring uses, so the word and the colour are one fact — and the legend line below turns the whole scheme from folklore into something the screen explains.
                        let connection_line = if is_self {
                            tr(Msg::AlwaysReachableSelf)
                        } else if contact.is_online {
                            match super::tier_label(super::path_tier_shown(contact, true)).as_deref() {
                                Some(w) => tr(Msg::OnlineVia(w)),
                                None => tr(Msg::Online),
                            }
                        } else {
                            tr(Msg::Offline)
                        };
                        // These rows should CONVERGE across your fleet devices — two devices showing different numbers here IS the sync bug, made visible.
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[0],
                            &tr(Msg::ContactPageName(cpage)),
                            tspan,
                            *theme::CONTACT_NAME_COLOUR,
                            600,
                        );
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[1],
                            &tr(Msg::MessagesSentReceived { total: human.len(), sent, recv }),
                            hspan2,
                            *theme::LABEL_COLOUR,
                            400,
                        );
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[2],
                            &tr(Msg::MessagesDelivered(delivered)),
                            hspan2,
                            *theme::LABEL_COLOUR,
                            400,
                        );
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[3],
                            &tr(Msg::ChatDaysSpan(span_days)),
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
                        if !is_self && contact.is_online {
                            settings_line(
                                &mut canvas,
                                ctx.text,
                                rows[7],
                                &tr(Msg::TierLegend),
                                hspan2 * 0.8,
                                *theme::LABEL_COLOUR,
                                400,
                            );
                        }
                        settings_line(
                            &mut canvas,
                            ctx.text,
                            rows[8],
                            &tr(Msg::RowsShouldMatch),
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
                            &tr(Msg::ContactPageName(cpage)),
                            tspan,
                            *theme::CONTACT_NAME_COLOUR,
                            600,
                        );
                        if is_self || contact.is_sibling {
                            settings_line(
                                &mut canvas,
                                ctx.text,
                                rows[1],
                                &tr(if is_self {
                                    Msg::OwnNotesCantBoot
                                } else {
                                    Msg::SiblingSignsItselfOut
                                }),
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
                            let label = tr(Msg::BootPill { armed: self.contact_boot_armed });
                            draw_stub_pill(
                                &mut canvas,
                                ctx.text,
                                &mut chrome.hit_test_map,
                                buf_w,
                                buf_h,
                                pill,
                                &label,
                                self.contact_panel_btn_base,
                                ctx.pressed_hit,
                            );
                            settings_line(
                                &mut canvas,
                                ctx.text,
                                rows[3],
                                &tr(Msg::BootRemovesEverywhere),
                                hspan2,
                                *theme::LABEL_COLOUR,
                                400,
                            );
                            settings_line(&mut canvas, ctx.text, rows[4], &tr(Msg::BootOstracism), hspan2, *theme::LABEL_COLOUR, 400);
                        }
                    }
                }
            }
        }

        if matches!(self.state, AppState::Conversation) {
            let mut canvas = Canvas::new(target, buf_w, buf_h, ctx.damage);
            // ── DISTINCT VIEWER SCREEN (2026-09-11) ── while the image viewer or text reader is open it IS the conversation screen: the header, message walk, and composer are skipped entirely. They used to render every frame under an opaque pane that discarded them pixel by pixel (full shaping + paint cost for a black rectangle — Nick: "thrashing the CPU, RAM and bus for literally no reason"), and the pane itself was painted FIRST, so under topmost-first under-blend the pane swallowed its own image and pills too (the documented 2026-08-31 backdrop-order class).
            let viewer_open = self.viewer.is_some() || self.reader.is_some();
            if viewer_open {
                if let Some(ci) = active_ci {
                    let ru = ctx.viewport.ru;
                    let unit = ReadyLayout::compute(buf_w, buf_h, ru).unit_height;
                    let msg_size = unit * 0.62;
                    let line_h = msg_size * 1.6;
                    let pad_x = unit;
                    // Pills clear the chrome strip on desktop; Android is full-edge.
                    let top_floor = if cfg!(target_os = "android") { unit * 1.1 } else { fluor::host::chrome::strip_height(ctx.viewport) + unit * 0.9 };
                    let pill_h = line_h * 1.4;
                    let pw = ((buf_w as f32 - pad_x * 2.0) * 0.28).max(msg_size * 3.0);
                    let py = top_floor;
                    let prects = [
                        fluor::region::Region::new(pad_x, py, pw, pill_h * 0.9),
                        fluor::region::Region::new(pad_x + pw + line_h * 0.5, py, pw, pill_h * 0.9),
                        fluor::region::Region::new(pad_x + (pw + line_h * 0.5) * 2.0, py, pw, pill_h * 0.9),
                    ];
                    let exposure_row = self.viewer.is_some();
                    let pw2 = ((buf_w as f32 - pad_x * 2.0 - line_h * 1.5) / 4.0).max(msg_size * 2.0);
                    let py2 = py + pill_h * 1.05;
                    let erects: [fluor::region::Region; 4] = std::array::from_fn(|i| fluor::region::Region::new(pad_x + (pw2 + line_h * 0.5) * i as f32, py2, pw2, pill_h * 0.9));
                    // Topmost first: pills and caption paint before the image, the image before the backdrop — everything wins exactly its own pixels.
                    if let Some(v) = self.viewer.as_ref() {
                        let decoding = self.img_pending.contains(&v.hash) || v.preview_hash.is_some_and(|ph| self.img_pending.contains(&ph));
                        let orig_done = matches!(self.img_cache.get(&v.hash), Some(Some(_)));
                        let small = TextStyle::new(msg_size * 0.85, *theme::LABEL_COLOUR).weight(500).font("Oxanium");
                        let halves = (v.ev * 2.0).round() as i32; let ev_note = if halves != 0 { format!(" \u{00B7} {}", tr(Msg::ExposureStops(&crate::fmt_halves(halves)))) } else { String::new() };
                        let caption = format!("{}{}{}{}", v.name, ev_note, if decoding { format!(" \u{00B7} {}", tr(Msg::ViewerDecoding)) } else { String::new() }, if orig_done { " \u{00B7} 1:1" } else { "" });
                        // Nameless images leave a dangling separator at the front — trim it.
                        let caption = caption.trim_start_matches([' ', '\u{00B7}']).to_string();
                        ctx.text.draw_text_left(&mut canvas, &caption, pad_x, buf_h as f32 - line_h * 0.4, &small, None, None);
                        draw_stub_pill_filled(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, prects[0], &tr(Msg::ViewerBack), self.viewer_base, ctx.pressed_hit, true, None, "Oxanium");
                        draw_stub_pill_filled(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, prects[1], &tr(Msg::ViewerOriginal), self.viewer_base.wrapping_add(1), ctx.pressed_hit, !orig_done && crate::storage::blob_present(&v.hash), None, "Oxanium");
                        draw_stub_pill_filled(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, prects[2], &tr(Msg::SavePill), self.viewer_base.wrapping_add(2), ctx.pressed_hit, crate::storage::blob_present(&v.hash), None, "Oxanium");
                        let labels: [std::borrow::Cow<'_, str>; 4] = ["\u{2212}\u{00BD}".into(), "+\u{00BD}".into(), "0".into(), tr(Msg::ClipPill)];
                        for (i, r) in erects.iter().enumerate() {
                            let lit = i == 3 && v.clip;
                            draw_stub_pill_filled(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, *r, &labels[i], self.viewer_base.wrapping_add(4 + i as HitId), ctx.pressed_hit, true, lit.then_some(*theme::PILL_GREEN), "Oxanium");
                        }
                        // The picture gets the WHOLE buffer: fit against the full screen, zoom/pan about its centre (the driver's zoom anchor is the viewport centre, now exactly the image area's centre).
                        let our_hh = self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)).unwrap_or([0u8; 32]);
                        let msgs: &[crate::types::ChatMessage] = dm_conversation(&self.conversations, &our_hh, &self.contacts[ci]).map(|c| c.messages.as_slice()).unwrap_or(&[]);
                        if let Some((w, h, px)) = super::viewer::viewer_pixels_of(v, &self.img_cache, msgs) {
                            let fit = (buf_w as f32 / w.max(1) as f32).min(buf_h as f32 / h.max(1) as f32);
                            let scale = fit * v.zoom;
                            let (dw, dh) = (w as f32 * scale, h as f32 * scale);
                            let cx = buf_w as f32 * 0.5 + v.pan.0;
                            let cy = buf_h as f32 * 0.5 + v.pan.1;
                            paint::draw_image(&mut canvas, &px, w, h, cx, cy, dw, dh, None);
                        }
                    } else if let Some(r) = self.reader.as_ref() {
                        let mono = TextStyle::new(msg_size * 0.9, *theme::CONTACT_NAME_COLOUR).weight(500).font("Oxanium");
                        let step = msg_size * 1.2;
                        let text_top = py + pill_h * 1.1;
                        let text_bottom = buf_h as f32 - line_h;
                        let clip = fluor::paint::Clip::new(0, text_top as usize, buf_w, text_bottom as usize);
                        let first = (r.scroll / step).floor().max(0.0) as usize;
                        let mut ly = text_top + step - (r.scroll - first as f32 * step);
                        for line in r.lines.iter().skip(first) {
                            if ly > text_bottom + step {
                                break;
                            }
                            ctx.text.draw_text_left(&mut canvas, line, pad_x - r.hscroll, ly, &mono, Some(clip), None);
                            ly += step;
                        }
                        let small = TextStyle::new(msg_size * 0.85, *theme::LABEL_COLOUR).weight(500).font("Oxanium");
                        ctx.text.draw_text_left(&mut canvas, &r.name, pad_x, buf_h as f32 - line_h * 0.4, &small, None, None);
                        draw_stub_pill_filled(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, prects[0], &tr(Msg::ViewerBack), self.viewer_base, ctx.pressed_hit, true, None, "Oxanium");
                        draw_stub_pill_filled(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, prects[2], &tr(Msg::SavePill), self.viewer_base.wrapping_add(2), ctx.pressed_hit, true, None, "Oxanium");
                    }
                    // Backdrop LAST (under-blend: everything above already owns its pixels) — the whole buffer below the chrome strip.
                    let bd_top = if cfg!(target_os = "android") { 0.0 } else { fluor::host::chrome::strip_height(ctx.viewport) };
                    paint::fill_rect(&mut canvas, 0, bd_top as isize, buf_w as isize, (buf_h as f32 - bd_top) as isize, *theme::VIEWER_BG_COLOUR, None, None);
                    // Hit map: swallow the whole screen below the strip (stale stamps from the skipped conversation body must not fire), then the pills win their rects back.
                    restamp_hit_rect(&mut chrome.hit_test_map, buf_w, buf_h, 0, bd_top as isize, buf_w as isize, buf_h as isize, self.viewer_base.wrapping_add(3));
                    let mut stamps: Vec<(fluor::region::Region, HitId)> = vec![(prects[0], 0), (prects[2], 2)];
                    if exposure_row {
                        stamps.push((prects[1], 1));
                        stamps.extend(erects.iter().enumerate().map(|(i, r)| (*r, 4 + i as HitId)));
                    }
                    for (r, off) in stamps {
                        restamp_hit_rect(&mut chrome.hit_test_map, buf_w, buf_h, r.x as isize, r.y as isize, (r.x + r.w) as isize, (r.y + r.h) as isize, self.viewer_base.wrapping_add(off));
                    }
                }
            }
            if let Some(ci) = active_ci.filter(|_| !viewer_open) {
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
                    let back_text = tr(Msg::BackToContacts);
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
                            &back_text,
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
                                &back_text,
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
                                band_top as isize,
                                0,
                                buf_h,
                                &scratch,
                            );
                        }
                    }
                    // Stamp the back button hit rect.
                    let back_w = ctx.text.measure_text(
                        &back_text,
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
                    // Stamp the avatar disc + tier ring at a given centre-y — stream entry #0's avatar. Clip rides in as a parameter and the caller passes the LIST clip: the avatar obeys exactly the same boundary as every message (a hardcoded None once let it paint thru the top edge onto its own visual layer).
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
                            // +1 keeps the presence ring visible at small avatar sizes (see the contacts-row `ring_thickness`).
                            let ring_thick = super::ring_thickness(avatar_r);
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
                                tr(Msg::NameReclaimed).into_owned(),
                                *theme::ERROR_TEXT_COLOUR,
                            )
                        } else if contact.identity_ended {
                            (
                                tr(Msg::IdentityEndedFrozen).into_owned(),
                                *theme::LABEL_COLOUR,
                            )
                        } else if is_self_contact {
                            (tr(Msg::NotesToSelf).into_owned(), *theme::SEARCH_FOUND_COLOUR)
                        } else {
                            (
                                tr(Msg::ClutchStatus(&contact_status_line(
                                    contact,
                                    self.device_keypair
                                        .as_ref()
                                        .map(|kp| *kp.public.as_bytes()),
                                    self.session.as_ref().map(|se| &se.identity_seed),
                                    self.contacts.iter().any(|s| s.is_sibling),
                                )))
                                .into_owned(),
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

                    // Status wrap: computed HERE (once) so the height budget below and the entry-#0 draw agree on the line count — a long ceremony step folds instead of running off the sides.
                    let status_wrapped: Option<(Vec<String>, u32)> =
                        status_in_stream.as_ref().map(|(label, colour)| {
                            let sstyle = TextStyle::new(unit * 0.6, *colour).weight(500).font("Oxanium");
                            (wrap_to_width(ctx.text, label, &sstyle, buf_w as f32 * 0.92), *colour)
                        });

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
                        // The kept recording currently playing (hash, percent) — its bubble shows ■ progress instead of ▶ size.
                        let playing_rec: Option<([u8; 32], u32)> = self.call_playback_hash.and_then(|h| {
                            self.call_playback.as_ref().map(|p| {
                                (h, ((p.position() as f32 / p.total.max(1) as f32) * 100.0).min(100.0) as u32)
                            })
                        });
                        let body_of = |m: &crate::types::ChatMessage| -> String {
                            if crate::types::parse_attachment_content(&m.content).is_none() {
                                if let Some((_, b)) = edit_over.get(&m.timestamp) {
                                    return b.clone();
                                }
                            }
                            if let (Some((ph, pct)), Some((h, name, _))) =
                                (playing_rec, crate::types::parse_attachment_content(&m.content))
                            {
                                if name == "call.audio" && h == ph {
                                    return tr(Msg::RecordingPlaying { pct }).into_owned();
                                }
                            }
                            display_row(m)
                        };
                        let conv_filter = self.conv_filter;
                        let visible: Vec<&crate::types::ChatMessage> = raw_msgs
                            .iter()
                            .filter(|m| chat_row_visible(raw_msgs, m, conv_filter))
                            .collect();
                        let n = visible.len();
                        // The recording that folds into each wave row's card: wave row ts → the live call.audio row referencing it (RefKind::Wave).
                        let rec_over: std::collections::HashMap<i64, &crate::types::ChatMessage> = raw_msgs
                            .iter()
                            .filter(|m| !m.deleted && crate::types::is_call_recording(&m.content))
                            .filter_map(|m| match m.reference {
                                Some((crate::types::RefKind::Wave, t)) => Some((t, m)),
                                _ => None,
                            })
                            .collect();
                        // The wave.env rows that fold into each card: wave ts → (our env blob hash, their env blob hash) — each party's own shared 3-channel tensor (call/wave_env.rs).
                        let env_over: std::collections::HashMap<i64, (Option<[u8; 32]>, Option<[u8; 32]>)> = {
                            let mut m: std::collections::HashMap<i64, (Option<[u8; 32]>, Option<[u8; 32]>)> = std::collections::HashMap::new();
                            for e in raw_msgs.iter().filter(|m| !m.deleted) {
                                if let (Some((crate::types::RefKind::Wave, t)), Some((h, name, _))) = (e.reference, crate::types::parse_attachment_content(&e.content)) {
                                    if name == crate::call::wave_env::WAVE_ENV_NAME {
                                        let slot = m.entry(t).or_default();
                                        if e.is_outgoing {
                                            slot.0 = Some(h);
                                        } else {
                                            slot.1 = Some(h);
                                        }
                                    }
                                }
                            }
                            m
                        };
                        // Stream entry #0 (avatar + name + optional status) is the oldest item: its height joins content_h so scrolling to genesis reveals it above message 1. Unconditional — every conversation has entry #0.
                        // The contact's chosen name may carry line returns — each extra line adds a pitch to entry #0 (the avatar above rides up by the same).
                        let header_name = super::contact_visible_name(contact, self.session.as_ref().map(|se| &se.identity_seed), self.fleet_settings.as_ref());
                        let header_name_lines: Vec<String> = header_name.split('\n').map(|s| s.to_string()).collect();
                        let header_name_extra = unit * 0.9 * (header_name_lines.len().saturating_sub(1)) as f32;
                        let header_block_h = avatar_r * 2.0
                            + unit * 3.0
                            + header_name_extra
                            + status_wrapped
                                .as_ref()
                                .map(|(lines, _)| unit * 0.25 + unit * 0.75 * lines.len() as f32)
                                .unwrap_or(0.0);
                        // Details-strip selection for THIS conversation (identity-keyed): one strip line joins content_h so the stream shifts to make room rather than overdrawing a neighbour row.
                        // The strip's row: the selection, else the NEWEST row unasked (its options stay up until tapped closed or a newer row lands).
                        let sel_key = self
                            .selected_msg
                            .filter(|(sci, _, _)| *sci == ci)
                            .map(|(_, ts, out)| (ts, out))
                            .or_else(|| {
                                if self.selected_msg.is_some() {
                                    return None;
                                }
                                visible
                                    .last()
                                    .map(|m| (m.timestamp, m.is_outgoing))
                                    .filter(|&(ts, out)| self.strip_dismissed != Some((ci, ts, out)))
                            });
                        let detail_h = line_h * 2.5; // two strip lines BELOW the media, padded: the action row (reply · edit · copy · resend · delete) and the reaction row (ranked glyphs + the circled "+"). The META section lives ABOVE the media now (Nick 2026-09-12, the four-section block) and wraps, so its height is dynamic (self.sel_meta_h).
                        let sel_in_stream = sel_key.is_some_and(|(ts, out)| {
                            visible
                                .iter()
                                .any(|m| m.timestamp == ts && m.is_outgoing == out)
                        });
                        // WORD-WRAP: messages wrap to the pane width instead of trailing off-screen. Metrics style matches the draw style below minus colour (colour never changes glyph widths). `intra` = spacing between a message's own wrapped lines; the inter-message gap stays line_h on the last line, so single-line spacing is pixel-identical to the pre-wrap layout. The line-count CACHE (see msg_wrap) covers all of history for content_h; drawn messages re-wrap for their actual strings.
                        let wrap_style = TextStyle::new(msg_size, 0).weight(500);
                        let avail_w = (buf_w as f32 - pad_x * 2.0).max(msg_size);
                        let intra = msg_size * 1.25;
                        // The conversation's contact, for the preview wants the walk collects.
                        let peer_handle_hash = self.contacts.get(ci).map(|c| c.handle_hash).unwrap_or([0u8; 32]);
                        // The decoded-picture count rides the key: a preview blob landing grows its row's band.
                        let wrap_key =
                            (ci, n, raw_msgs.len() + (self.img_cache.len() << 20), avail_w.to_bits(), msg_size.to_bits(), conv_filter as u8);
                        if self.msg_wrap.as_ref().map(|(k, _, _)| *k) != Some(wrap_key) {
                            let mut all_lines: Vec<Vec<String>> = Vec::with_capacity(n);
                            let mut total = 0usize;
                            for m in &visible {
                                // call.audio rows draw in Oxanium (so the dozenal size + ▶ resolve) — measure with the SAME font or the wrapped line count disagrees with the draw.
                                let row_wrap = if crate::types::is_call_recording(&m.content) {
                                    TextStyle::new(msg_size, 0).weight(500).font("Oxanium")
                                } else {
                                    wrap_style.clone()
                                };
                                // A wave row's one line is its header (outcome + duration, base-aware); a LIVE wave reserves two more for the waveform band beneath it.
                                let lines = match m.wave {
                                    Some(w) => {
                                        if w.outcome.was_live() {
                                            total += 2;
                                        }
                                        vec![super::call_ui::wave_header(w)]
                                    }
                                    None => {
                                        let body = body_of(m);
                                        // A picture row has no text: the band IS the row, so it reserves no body line at all (the draw walks an empty body; the band anchors off max(1)).
                                        if body.is_empty() && m.attach.is_some() { Vec::new() } else { wrap_text_lines(ctx.text, &body, &row_wrap, avail_w) }
                                    }
                                };
                                total += lines.len();
                                // An image attachment with a micro preview reserves a band above its pill (IMG_PREVIEW_LINES lines) — the picture draws there before any blob is fetched.
                                total += super::viewer::img_band_lines_of(&self.img_cache, m);
                                total += super::viewer::audio_band_lines_of(m);
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
                            + if sel_in_stream { detail_h + self.sel_meta_h } else { 0.0 };
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
                        self.msg_hit_rows.resize(super::MSG_HIT_SPAN as usize, None);
                        self.msg_wave_bands.clear();
                        self.msg_wave_bands.resize(super::MSG_HIT_SPAN as usize, None);
                        self.msg_attach_visuals.clear();
                        self.msg_attach_visuals.resize(super::MSG_HIT_SPAN as usize, None);
                        self.msg_link_hits.clear();
                        // ── LINK CONSENT PANEL ── painted BEFORE the message walk (earliest paint wins under-blend), hit-stamped AFTER it (latest stamp wins the map). A tapped link never opens silently: the full destination shows verbatim — punycode/homograph honesty — with Open / Copy / Cancel (Nick 2026-09-04).
                        let mut consent_stamp: Option<([fluor::region::Region; 3], f32)> = None;
                        if let Some(dest) = self.link_consent.clone() {
                            let url_style = TextStyle::new(msg_size * 0.9, *theme::LINK_COLOUR).weight(500).font("Oxanium");
                            let panel_w = (buf_w as f32 - pad_x * 2.0).max(msg_size);
                            let url_lines = wrap_text_lines(ctx.text, &dest, &url_style, panel_w * 0.94);
                            let non_ascii = !dest.is_ascii();
                            let warn_h = if non_ascii { line_h } else { 0.0 };
                            let pill_h = line_h * 1.4;
                            let panel_h = line_h * 1.4 + url_lines.len() as f32 * line_h + warn_h + pill_h + line_h * 0.8;
                            let py0 = (list_bottom - panel_h).max(list_top);
                            // Top hairline FIRST (topmost-first under-blend); the opaque backdrop paints LAST, after the text and pills below — painted first it swallowed the panel's own content (the same 2026-08-31 backdrop-order class as the viewer pane).
                            paint::fill_rect(&mut canvas, 0, py0 as isize, buf_w as isize, ctx.viewport.ru.max(1.0) as isize, theme::VERSION_COLOUR, None, None);
                            let mut ty = py0 + line_h;
                            ctx.text.draw_text_left(&mut canvas, &tr(Msg::LinkConsentTitle), pad_x, ty, &TextStyle::new(msg_size, *theme::CONTACT_NAME_COLOUR).weight(600), Some(list_clip), None);
                            ty += line_h * 1.1;
                            for ul in &url_lines {
                                ctx.text.draw_text_left(&mut canvas, ul, pad_x, ty, &url_style, Some(list_clip), None);
                                ty += line_h;
                            }
                            if non_ascii {
                                ctx.text.draw_text_left(&mut canvas, &tr(Msg::LinkNonAsciiWarn), pad_x, ty, &TextStyle::new(msg_size * 0.85, *theme::SEARCH_FAIL_COLOUR).weight(600), Some(list_clip), None);
                                ty += line_h;
                            }
                            let pw = panel_w * 0.28;
                            let prects = [
                                fluor::region::Region::new(pad_x, ty, pw, pill_h * 0.9),
                                fluor::region::Region::new(pad_x + pw + line_h * 0.5, ty, pw, pill_h * 0.9),
                                fluor::region::Region::new(pad_x + (pw + line_h * 0.5) * 2.0, ty, pw, pill_h * 0.9),
                            ];
                            // Every platform with a browser opens (Android thru the Kotlin ACTION_VIEW bridge); Redox alone stays copy-only.
                            let can_open = !cfg!(target_os = "redox");
                            draw_stub_pill_filled(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, prects[0], &tr(Msg::OpenLinkPill), self.link_consent_base, ctx.pressed_hit, can_open, Some(*theme::PILL_GREEN), "Open Sans");
                            draw_stub_pill_filled(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, prects[1], &tr(Msg::CopyPill), self.link_consent_base.wrapping_add(1), ctx.pressed_hit, true, None, "Open Sans");
                            draw_stub_pill_filled(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, prects[2], &tr(Msg::Cancel), self.link_consent_base.wrapping_add(2), ctx.pressed_hit, true, None, "Open Sans");
                            paint::fill_rect(&mut canvas, 0, py0 as isize, buf_w as isize, (list_bottom - py0) as isize, *theme::CONSENT_BG_COLOUR, None, None);
                            consent_stamp = Some((prects, py0));
                        }
                        // (The image viewer / text reader is a DISTINCT screen now — drawn above, this whole body skipped while it's open.)
                        // TOP-ANCHOR while the conversation fits the view: the stream reads avatar/name → msg 1 → msg 2 from the top, ONE strip — bottom-anchoring a short history floated the header block mid-screen above a clump of bottom messages ("rendered in a different layer"). Once content outgrows the view the min() saturates and the classic newest-at-bottom anchor takes over seamlessly.
                        let mut y = (list_top + content_h).min(list_bottom) - msg_size + scroll;
                        // Whether the walk reached the conversation's FIRST message (no early break): the scroll-top avatar/name block may only draw then — drawing it at the break position floated it mid-stream over recent messages in any long conversation ("the avatar and name are rendered in a different block").
                        let mut reached_oldest = true;
                        // Hold the cached wrap strings for the whole walk (disjoint field from everything the loop mutates).
                        let wrap_cache: &Vec<Vec<String>> =
                            &self.msg_wrap.as_ref().expect("wrap cache built above").1;
                        let mut filter_stamp: Option<(fluor::region::Region, HitId)> = None;
                        // STREAM FILTER PILL — bottom RIGHT, drawn BEFORE the row walk so under-blend keeps it above the rows (Nick 2026-09-12: "the filter button is not drawn first like it should be"); its hit rect is re-asserted after the walk, just above the compose box (Nick 2026-09-09: the top-centre spot overlapped the title), sliding off to the RIGHT with the same scroll-tied offset the top bar slides up with. Stamped AFTER the row walk so it wins its rect; no stamp while slid away.
                        {
                            let f_label = tr(match self.conv_filter {
                                ChatFilter::All => Msg::FilterAll,
                                ChatFilter::Waves => Msg::FilterWaves,
                                ChatFilter::Text => Msg::FilterText,
                            });
                            let f_h = unit * 1.3;
                            let f_w = unit * 3.2;
                            let f_x = buf_w as f32 - pad_x - f_w + bar_off;
                            let f_y = list_bottom - f_h - unit * 0.25;
                            filter_stamp = None;
                            if f_x < buf_w as f32 {
                                filter_stamp = Some((fluor::region::Region::new(f_x, f_y, f_w, f_h), if topbar_visible { self.conv_filter_hit } else { HIT_NONE }));
                                super::draw_stub_pill(
                                    &mut canvas,
                                    ctx.text,
                                    &mut chrome.hit_test_map,
                                    buf_w,
                                    buf_h,
                                    fluor::region::Region::new(f_x, f_y, f_w, f_h),
                                    &f_label,
                                    if topbar_visible { self.conv_filter_hit } else { HIT_NONE },
                                    ctx.pressed_hit,
                                );
                            }
                        }
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
                            // A live wave's card carries its waveform band under the header (two lines, matching the wrap total above).
                            let wave_band_h = if msg.wave.is_some_and(|w| w.outcome.was_live()) { 2.0 * intra } else { 0.0 };
                            // The image preview band above an image attachment's pill (typed attachments 2026-09-10).
                            let img_lines = super::viewer::img_band_lines_of(&self.img_cache, msg);
                            let img_band_h = img_lines as f32 * intra;
                            // A music pigeon's waveform band (audio rows that aren't call recordings).
                            let audio_lines = super::viewer::audio_band_lines_of(msg);
                            let audio_band_h = audio_lines as f32 * intra;
                            // The decoded preview blob outranks the micro thumb; either way the band's picture is (w, h, pixels).
                            let decoded: Option<(usize, usize, &Vec<u32>)> = msg.attach.and_then(|a| a.preview_hash).and_then(|ph| match self.img_cache.get(&ph) {
                                Some(Some((w, h, px))) => Some((*w, *h, px)),
                                _ => None,
                            });
                            // Ask for the preview blob once: held → decode; missing → fetch (drained on the tick).
                            if let Some(a) = msg.attach {
                                if a.kind.is_image() {
                                    if let Some(ph) = a.preview_hash {
                                        if !self.img_cache.contains_key(&ph) && !self.img_pending.contains(&ph) && !self.img_wants.iter().any(|(_, h, _)| *h == ph) {
                                            let held = crate::storage::blob_present(&ph);
                                            if held || !self.attach_auto_fetched.contains(&ph) {
                                                self.img_wants.push((peer_handle_hash, ph, held));
                                            }
                                        }
                                    }
                                }
                            }
                            let block_extra = (lines.len().max(1) as f32 - 1.0) * intra
                                + if reply_target.is_some() { intra } else { 0.0 }
                                + react_off
                                + wave_band_h
                                + img_band_h
                                + audio_band_h;
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
                                // Chunk progress by HASH first (a chunked blob's own count), the direction-matched PT snapshot as the whole-value fallback.
                                let chunk_frac = self.attach_chunk_progress.get(&hash).map(|(have, total)| *have as f32 / (*total).max(1) as f32);
                                if relevant {
                                    if let Some(frac) = chunk_frac.or_else(|| {
                                        self.attach_progress
                                            .iter()
                                            .find(|(_, _, _, ob)| *ob == want_outbound)
                                            .map(|(_, done, total, _)| *done as f32 / (*total).max(1) as f32)
                                    }) {
                                        let frac = frac.clamp(0.0, 1.0);
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
                            let mut sel_meta_extra = 0.0f32;
                            if sel_key.is_some_and(|(ts, out)| {
                                msg.timestamp == ts && msg.is_outgoing == out
                            }) {
                                let secs = ((vsf::eagle_time_oscillations() - msg.timestamp)
                                    / crate::OSC_PER_SEC)
                                    .max(0);
                                // Dozenal mode shows the DMS age (how many times a second has doubled — one number, no units; the Dozenal page carries the legend); arabic mode the unit'd count. The detail style is Oxanium, so the glyphs resolve.
                                let dms = crate::dms_age(secs);
                                let age = if crate::dms_ui() {
                                    tr(Msg::AgoDms(&dms))
                                } else {
                                    tr(if secs >= 86400 {
                                        Msg::AgoDays((secs / 86400) as u32)
                                    } else if secs >= 3600 {
                                        Msg::AgoHours((secs / 3600) as u32)
                                    } else if secs >= 60 {
                                        Msg::AgoMinutes((secs / 60) as u32)
                                    } else {
                                        Msg::AgoSeconds(secs as u32)
                                    })
                                };
                                // The delivery ladder (sending → replicated ∥ delivered): "delivered" = the friend's fleet ACKed (the line — nothing beyond it exists, ever; "seen" is only a human's explicit reaction); "replicated" = our own fleet holds it but their ACK hasn't landed yet.
                                let mut detail = if msg.is_outgoing {
                                    let state = tr(if msg.delivered {
                                        Msg::DeliveryDelivered
                                    } else if msg.replicated {
                                        Msg::DeliveryReplicated
                                    } else {
                                        Msg::DeliverySending
                                    });
                                    tr(Msg::SentDetail { age: &age, state: &state }).into_owned()
                                } else {
                                    tr(Msg::ReceivedDetail(&age)).into_owned()
                                };
                                if msg.recovered {
                                    detail.push_str(&tr(Msg::RecoveredSuffix));
                                }
                                if crate::types::parse_attachment_content(&msg.content).is_none()
                                    && edit_over.contains_key(&msg.timestamp)
                                {
                                    detail.push_str(&tr(Msg::EditedSuffix));
                                }
                                // Attachment blob state joins the meta line: held/confirmed vs still travelling.
                                if let Some((hash, _, _)) =
                                    crate::types::parse_attachment_content(&msg.content)
                                {
                                    if msg.is_outgoing {
                                        detail.push_str(&tr(
                                            if self.attach_confirmed.contains(&hash) {
                                                Msg::BlobDeliveredSuffix
                                            } else {
                                                Msg::BlobSendingSuffix
                                            },
                                        ));
                                    } else if !crate::storage::blob_present(&hash) {
                                        detail.push_str(&tr(Msg::BlobNotHereSuffix));
                                    }
                                }
                                // STATS UP TOP (Nick 2026-09-12): an attachment row's meta line leads with name, type, size and dims; the age and delivery state follow.
                                if let (Some(a), Some((_, name, size))) = (msg.attach, crate::types::parse_attachment_content(&msg.content)) {
                                    let size_s = crate::types::size_label(size);
                                    let dims_s = a.dims.map(|(w, h): (u32, u32)| format!("{}\u{00D7}{}", crate::fmt_num(w), crate::fmt_num(h))).unwrap_or_default();
                                    let kind_s = tr(Msg::AttachKindName(a.kind));
                                    let stats = tr(Msg::AttachStats { name: &name, kind: &kind_s, size: &size_s, dims: &dims_s });
                                    detail = format!("{stats} \u{00B7} {detail}");
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
                                        detail.push_str(&tr(Msg::ReactTheySuffix(g)));
                                    }
                                    if let Some(g) =
                                        slots[1].as_ref().map(|(_, g)| g).filter(|g| !g.is_empty())
                                    {
                                        detail.push_str(&tr(Msg::ReactYouSuffix(g)));
                                    }
                                }
                                // SECTION 1, THE META, ABOVE THE MEDIA (Nick 2026-09-12, the four-section block): everything about the message — stats, kind, age, delivery, quality — wrapped to the width the user's window and zoom allow. Its measured height feeds the extent walk thru self.sel_meta_h (one frame late on a selection change; settles like any overshoot).
                                let meta_lh = detail_size * 1.3;
                                let meta_lines = wrap_text_lines(ctx.text, &detail, &detail_style, buf_w as f32 - pad_x * 2.0);
                                let meta_h = meta_lines.len() as f32 * meta_lh + msg_size * 0.4;
                                self.sel_meta_h = meta_h;
                                sel_meta_extra = meta_h;
                                // The meta stacks DOWN from the block's top: above the message body and its media band.
                                let meta_bottom = y - detail_h - block_extra - msg_size * 1.1;
                                for (k, ml) in meta_lines.iter().enumerate() {
                                    let ly = meta_bottom - (meta_lines.len() - 1 - k) as f32 * meta_lh;
                                    ctx.text.draw_text_left(&mut canvas, ml, pad_x, ly, &detail_style, Some(list_clip), None);
                                }
                                // SELECTED HIGHLIGHT, painted after the meta so the veil is uniform under topmost-first: the META section wears YELLOW on a development build (a layout debugging aid — the section boundary is visible) and the rail's white on release; the media + strip below always wear the rail's white.
                                let hl_meta_top = (meta_bottom - (meta_lines.len() as f32 - 0.4) * meta_lh - meta_lh * 0.2).max(list_top);
                                let hl_media_top = (meta_bottom + meta_lh * 0.6).max(list_top);
                                let hl_bot = (y + line_h * 0.7).min(list_bottom);
                                // Development builds tint each SECTION its own colour so the boundaries are visible while the layout iterates (Nick 2026-09-12): meta yellow, media white, actions cyan, reactions red. Release wears the rail's white throughout.
                                let dev = cfg!(feature = "development");
                                let meta_tint = if dev { fluor::theme::fmt(0x20_00_00_FF) } else { theme::RAIL_ACTIVE_COLOUR };
                                let action_tint = if dev { fluor::theme::fmt(0x20_FF_00_00) } else { theme::RAIL_ACTIVE_COLOUR };
                                let react_tint = if dev { fluor::theme::fmt(0x20_00_FF_FF) } else { theme::RAIL_ACTIVE_COLOUR };
                                let hl_actions_top = (y - line_h * 2.0).max(list_top);
                                let hl_react_top = (y - line_h * 0.7).max(list_top);
                                if hl_media_top > hl_meta_top {
                                    paint::fill_rect(&mut canvas, 0, hl_meta_top as isize, buf_w as isize, (hl_media_top - hl_meta_top) as isize, meta_tint, Some(list_clip), None);
                                }
                                if hl_actions_top > hl_media_top {
                                    paint::fill_rect(&mut canvas, 0, hl_media_top as isize, buf_w as isize, (hl_actions_top - hl_media_top) as isize, theme::RAIL_ACTIVE_COLOUR, Some(list_clip), None);
                                }
                                if hl_react_top > hl_actions_top {
                                    paint::fill_rect(&mut canvas, 0, hl_actions_top as isize, buf_w as isize, (hl_react_top - hl_actions_top) as isize, action_tint, Some(list_clip), None);
                                }
                                if hl_bot > hl_react_top {
                                    paint::fill_rect(&mut canvas, 0, hl_react_top as isize, buf_w as isize, (hl_bot - hl_react_top) as isize, react_tint, Some(list_clip), None);
                                }
                                // Lower strip line: the ACTION ROW — reply · edit · copy/copied · resend · delete. Conditional pills: edit only for outgoing (stub until the message-format rework), resend only for undelivered outgoing (manual re-fire on the chain), delete always (LOCAL until tombstones — fleet sync may resurrect it), reply always. Each pill stamps its own hit id with generous padding.
                                let (copy_label, copy_colour) = if self.selected_msg_copied {
                                    (tr(Msg::CopiedPill), *theme::SEARCH_FOUND_COLOUR)
                                } else {
                                    (tr(Msg::CopyPill), *theme::COPY_PILL_COLOUR)
                                };
                                // A WAVE CARD's options (Nick 2026-09-09): wave back (place a wave to this contact) and beam back (a stub, greyed until video lands), then delete — reply/edit/copy make no sense on a wave.
                                let is_wave_row = msg.wave.is_some();
                                let mut pills: Vec<(std::borrow::Cow<'static, str>, u32, HitId)> = if is_wave_row {
                                    let mut v = vec![
                                        (tr(Msg::WaveBack), *theme::COPY_PILL_COLOUR, self.msg_action_base.wrapping_add(6)),
                                        (tr(Msg::BeamBack), theme::dim_colour(*theme::LABEL_COLOUR), HIT_NONE),
                                    ];
                                    // The recording's options, when one folds into this card: save (held) or fetch (missing), and replicate among the fleet.
                                    if let Some(rec) = rec_over.get(&msg.timestamp) {
                                        let held = crate::types::parse_attachment_content(&rec.content).is_some_and(|(h, _, _)| crate::storage::blob_present(&h));
                                        if held {
                                            v.push((tr(Msg::SavePill), *theme::SEARCH_FOUND_COLOUR, self.msg_action_base.wrapping_add(7)));
                                        } else {
                                            v.push((tr(Msg::FetchPill), *theme::HOURGLASS_COLOUR, self.msg_action_base.wrapping_add(7)));
                                        }
                                        v.push((tr(Msg::ReplicatePill), *theme::COPY_PILL_COLOUR, self.msg_action_base.wrapping_add(8)));
                                    }
                                    v
                                } else {
                                    vec![(tr(Msg::ReplyPill), *theme::COPY_PILL_COLOUR, self.msg_action_base)]
                                };
                                if !is_wave_row
                                    && msg.is_outgoing
                                    && crate::types::parse_attachment_content(&msg.content)
                                        .is_none()
                                {
                                    pills.push((
                                        tr(Msg::EditPill),
                                        *theme::COPY_PILL_COLOUR,
                                        self.msg_action_base.wrapping_add(1),
                                    ));
                                }
                                // Copy is for text: an attachment row's body is its visual, nothing to copy.
                                if !is_wave_row && crate::types::parse_attachment_content(&msg.content).is_none() {
                                    pills.push((copy_label, copy_colour, self.msg_copy_id));
                                }
                                if msg.is_outgoing && !msg.delivered {
                                    pills.push((
                                        tr(Msg::ResendPill),
                                        *theme::HOURGLASS_COLOUR,
                                        self.msg_action_base.wrapping_add(2),
                                    ));
                                }
                                // Attachment rows: a call recording PLAYS (blob held) or fetches; a file SAVES (blob held) or fetches. Same slot 4 — the click handler branches on call.audio.
                                if let Some((hash, _, _)) =
                                    crate::types::parse_attachment_content(&msg.content)
                                {
                                    let held = crate::storage::blob_present(&hash);
                                    let is_rec = crate::types::is_call_recording(&msg.content);
                                    // Opening is a tap on the visual itself (2026-09-12); the strip's pill is the file verb: fetch it, play a standalone recording, or save it.
                                    // A held music pigeon gets PLAY/STOP in the action row itself, beside save and delete (Nick 2026-09-12: "same line as reply/save/delete") — the same glyphs the wave card's play wears.
                                    let is_music = msg.attach.is_some_and(|a| a.kind == crate::types::AttachKind::Audio) && !is_rec;
                                    if held && is_music {
                                        let playing_this = self.music_play.as_ref().is_some_and(|m| m.hash == hash && m.playing());
                                        let glyph: std::borrow::Cow<'static, str> = if playing_this { "\u{25A0}".into() } else { "\u{25B6}\u{FE0E}".into() };
                                        pills.push((glyph, *theme::COPY_PILL_COLOUR, self.msg_action_base.wrapping_add(9)));
                                    }
                                    let (label, colour) = if !held {
                                        (tr(Msg::FetchPill), *theme::HOURGLASS_COLOUR)
                                    } else if is_rec {
                                        (tr(Msg::PlayPill), *theme::COPY_PILL_COLOUR)
                                    } else {
                                        (tr(Msg::SavePill), *theme::SEARCH_FOUND_COLOUR)
                                    };
                                    pills.push((label, colour, self.msg_action_base.wrapping_add(4)));
                                }
                                let deleting =
                                    self.pending_delete.as_ref().is_some_and(|(k, _)| {
                                        *k == (ci, msg.timestamp, msg.is_outgoing)
                                    });
                                pills.push((
                                    tr(if deleting {
                                        Msg::DeletingPill
                                    } else {
                                        Msg::DeletePill
                                    }),
                                    *theme::ERROR_TEXT_COLOUR,
                                    self.msg_action_base.wrapping_add(3),
                                ));
                                if deleting {
                                    // The feedback frame is on screen — the tick may do the heavy lift now.
                                    if let Some((_, painted)) = self.pending_delete.as_mut() {
                                        *painted = true;
                                    }
                                }
                                // ACTUAL BUTTONS (Nick 2026-09-12: "each option, like wave back, beam back, fetch, should be actual buttons"): every option is a filled pill thru the one pill renderer, tinted by its verb, a greyed one (no hit) for a stub like beam back.
                                let pill_h = line_h * 0.9;
                                let pad_hit = detail_size;
                                let mut px_cursor = pad_x;
                                for (label, colour, hid) in pills {
                                    let style = TextStyle::new(detail_size, colour).weight(600).font("Oxanium");
                                    let w = ctx.text.measure_text(&label, &style) + pad_hit * 1.4;
                                    let rect = fluor::region::Region::new(px_cursor, y - line_h * 1.4 - pill_h * 0.5, w, pill_h);
                                    if rect.y + rect.h >= list_top && rect.y <= list_bottom {
                                        let fill = Some((theme::near_black(colour, 0.15), theme::near_black(colour, 0.3)));
                                        draw_stub_pill_filled(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, rect, &label, hid, ctx.pressed_hit, hid != HIT_NONE, fill, "Oxanium");
                                    }
                                    px_cursor += w + pad_hit;
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
                                let react_pill_h = line_h * 0.9;
                                let mut rx_cursor = pad_x;
                                self.react_strip_glyphs.clear();
                                for g in ranked_reactions.iter() {
                                    if self.react_strip_glyphs.len() >= 9 {
                                        break;
                                    }
                                    // DARK BUTTONS (Nick 2026-09-12, "text should be bright and colourful but the buttons should be DARK — on reactions specifically"): each glyph is a real pill in the action row's dim verb tint — cyan for react, green for the one that's ours — instead of the old full-brightness bare glyph.
                                    let verb = if ours_now.as_deref() == Some(g.as_str()) { *theme::SEARCH_FOUND_COLOUR } else { *theme::COPY_PILL_COLOUR };
                                    let style = TextStyle::new(glyph_size, verb).weight(500);
                                    let w = ctx.text.measure_text(g, &style) + pad_hit * 1.2;
                                    // Fit gate: always leave room for the circled "+" at the row's end.
                                    if rx_cursor + w + pad_hit + plus_r * 2.0 + pad_hit
                                        > buf_w as f32 - pad_x
                                    {
                                        break;
                                    }
                                    let rect = fluor::region::Region::new(rx_cursor, y - react_pill_h * 0.75, w, react_pill_h);
                                    if rect.y + rect.h >= list_top && rect.y <= list_bottom {
                                        let fill = Some((theme::near_black(verb, 0.15), theme::near_black(verb, 0.3)));
                                        draw_stub_pill_filled(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, rect, g, self.react_strip_base.wrapping_add(self.react_strip_glyphs.len() as HitId), ctx.pressed_hit, true, fill, "Oxanium");
                                    }
                                    self.react_strip_glyphs.push(g.clone());
                                    rx_cursor += w + pad_hit;
                                }
                                // The circled "+": react with ANYTHING — arms the compose box, the system keyboard is the picker. Dim ring, bright glyph — a dark button like its siblings.
                                let plus_cx = rx_cursor + plus_r;
                                let plus_cy = y - glyph_size * 0.32;
                                paint::draw_circle(
                                    &mut canvas,
                                    plus_cx,
                                    plus_cy,
                                    plus_r,
                                    theme::near_black(*theme::COPY_PILL_COLOUR, 0.3),
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
                            // call.audio rows render in Oxanium — matches the wrap-loop font so the dozenal size + ▶ glyph resolve (the default bubble font tofus both).
                            let msg_style = if crate::types::is_call_recording(&msg.content) {
                                TextStyle::new(msg_size, colour).weight(500).font("Oxanium")
                            } else {
                                TextStyle::new(msg_size, colour).weight(500)
                            };
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
                            // Link projection: map validated marks onto the wrapped lines (each line = a contiguous source slice). Only when the drawn body IS the content (attachment/summary bodies differ, and their marks were never minted anyway).
                            let line_starts = if !msg.marks.is_empty() && body_of(msg) == msg.content {
                                super::line_source_starts(&msg.content, lines)
                            } else {
                                None
                            };
                            // ── WAVE CARD ── one event per wave: the header line, then (for a live wave) the waveform band that IS the seek bar. The recording row referencing this wave folds in here; until it lands the band says so.
                            let wave_slot = vi % super::MSG_HIT_SPAN as usize;
                            if let Some(w) = msg.wave {
                                let right_aligned = msg.is_outgoing || is_self_contact;
                                let hy = y - react_off - wave_band_h;
                                // A rejected wave reads SMALL: it is a record for you, never a fuss.
                                let head_size = if w.outcome == crate::types::WaveOutcome::Rejected { msg_size * 0.75 } else { msg_size };
                                let head_style = TextStyle::new(head_size, colour).weight(500).font("Oxanium");
                                let head = lines.first().cloned().unwrap_or_default();
                                if right_aligned {
                                    ctx.text.draw_text_right(&mut canvas, &head, buf_w as f32 - pad_x, hy, &head_style, Some(list_clip), None);
                                } else {
                                    ctx.text.draw_text_left(&mut canvas, &head, pad_x, hy, &head_style, Some(list_clip), None);
                                }
                                if wave_band_h > 0.0 {
                                    let bx0 = pad_x;
                                    let bx1 = buf_w as f32 - pad_x;
                                    let by0 = hy + msg_size * 0.7;
                                    let by1 = y - react_off + msg_size * 0.5;
                                    let glyph_x1 = bx0 + msg_size * 1.5;
                                    let bcy = (by0 + by1) * 0.5;
                                    let half = (by1 - by0) * 0.5;
                                    let hair = ctx.viewport.ru.max(1.0);
                                    let dim = theme::dim_colour(colour);
                                    let small = TextStyle::new(msg_size * 0.8, dim).weight(500).font("Oxanium");
                                    match rec_over.get(&msg.timestamp).copied() {
                                        None => {
                                            // The keep transcode is still running (or the recording never reached this device): say so where the waveform will be.
                                            ctx.text.draw_text_left(&mut canvas, &tr(Msg::WaveKeeping), glyph_x1, bcy + msg_size * 0.35, &small, Some(list_clip), None);
                                        }
                                        Some(rec) => {
                                            let hash = crate::types::parse_attachment_content(&rec.content).map(|(h, _, _)| h).unwrap_or([0u8; 32]);
                                            let held = crate::storage::blob_present(&hash);
                                            let playing = self.call_playback.is_some() && self.call_playback_hash == Some(hash);
                                            // ENVELOPE SOURCES (the wave.env exchange, 2026-09-12): each half of the card comes from that party's OWN shared 3-channel tensor — ours from our blob, theirs from theirs — and a side whose blob hasn't landed (or was never minted: short waves) derives from the held audio, one decode, off-thread. Nothing reads the container's header any more.
                                            let (env_ours_h, env_theirs_h) = env_over.get(&msg.timestamp).copied().unwrap_or((None, None));
                                            let total_slots = if playing { self.call_playback.as_ref().map(|h| h.total).unwrap_or(0) } else { w.secs as usize * 100 };
                                            let scrub = self.wave_scrub.filter(|s| s.band.hash == hash).map(|s| s.frac);
                                            let frac: Option<f32> = scrub.or_else(|| {
                                                playing.then(|| self.call_playback.as_ref().map(|h| h.position() as f32 / h.total.max(1) as f32).unwrap_or(0.0))
                                            });
                                            let wx0 = glyph_x1;
                                            let cols = ((bx1 - wx0).max(1.0)) as usize;
                                            let played_cols = frac.map(|f| (f * cols as f32) as usize).unwrap_or(0);
                                            // Loads on the render edge, one per hash: an env blob parses in the worker (vec of one); the audio derive decodes once and yields every channel. A far blob not yet held is queued for one fetch.
                                            if let Some(seed) = self.session.as_ref().map(|se| se.identity_seed) {
                                                if self.wave_env_tx.is_none() {
                                                    let (tx, rx) = std::sync::mpsc::channel();
                                                    self.wave_env_tx = Some(tx);
                                                    self.wave_env_rx = Some(rx);
                                                }
                                                let tx0 = self.wave_env_tx.as_ref().unwrap().clone();
                                                let wake0 = self.event_proxy.clone();
                                                for eh in [env_ours_h, env_theirs_h].into_iter().flatten() {
                                                    if self.wave_env.contains_key(&eh) || self.wave_env_pending.contains(&eh) {
                                                        continue;
                                                    }
                                                    if crate::storage::blob_present(&eh) {
                                                        self.wave_env_pending.insert(eh);
                                                        let (tx, wake) = (tx0.clone(), wake0.clone());
                                                        let _ = std::thread::Builder::new().name("wave-env".into()).spawn(move || {
                                                            let res = crate::storage::blob_load(&seed, &eh).and_then(|b| crate::call::wave_env::read(&b)).map(|e| vec![e]);
                                                            let _ = tx.send((eh, res));
                                                            #[cfg(not(target_os = "android"))]
                                                            if let Some(wk) = wake.as_ref() {
                                                                let _ = wk.send(crate::ui::PhotonEvent::NetworkUpdate);
                                                            }
                                                            #[cfg(target_os = "android")]
                                                            let _ = wake;
                                                        });
                                                    } else {
                                                        self.wave_env_wants.push((ci, eh));
                                                    }
                                                }
                                                let side_unresolvable = |h: Option<[u8; 32]>| h.is_none_or(|x| !crate::storage::blob_present(&x));
                                                if held && !self.wave_env.contains_key(&hash) && !self.wave_env_pending.contains(&hash) && (side_unresolvable(env_ours_h) || side_unresolvable(env_theirs_h)) {
                                                    self.wave_env_pending.insert(hash);
                                                    let (tx, wake) = (tx0.clone(), wake0.clone());
                                                    let _ = std::thread::Builder::new().name("wave-env-derive".into()).spawn(move || {
                                                        let res = crate::storage::blob_load(&seed, &hash).and_then(|b| crate::call::record::envelopes_from_blob(&b));
                                                        let _ = tx.send((hash, res));
                                                        #[cfg(not(target_os = "android"))]
                                                        if let Some(wk) = wake.as_ref() {
                                                            let _ = wk.send(crate::ui::PhotonEvent::NetworkUpdate);
                                                        }
                                                        #[cfg(target_os = "android")]
                                                        let _ = wake;
                                                    });
                                                }
                                            }
                                            // Resolve each party: their own blob first, the audio-derived channel second (0 = us/mic, 1 = them/wire).
                                            let resolve = |blob_h: Option<[u8; 32]>, chn: usize| -> Option<([u8; 32], usize, std::sync::Arc<crate::call::wave_env::WaveEnv>)> {
                                                if let Some(h) = blob_h {
                                                    if let Some(Some(v)) = self.wave_env.get(&h) {
                                                        if let Some(e) = v.first() {
                                                            return Some((h, 0, e.clone()));
                                                        }
                                                    }
                                                }
                                                if let Some(Some(v)) = self.wave_env.get(&hash) {
                                                    if let Some(e) = v.get(chn) {
                                                        return Some((hash, chn, e.clone()));
                                                    }
                                                }
                                                None
                                            };
                                            let sides = [(resolve(env_ours_h, 0), false, our_colour), (resolve(env_theirs_h, 1), true, their_colour)];
                                            // THE CUMULATIVE STACK WITH OPACITY CONTRIBUTIONS (Nick 2026-09-12): wave_fold_coverage lands every bin in one column at its own height and tracks per-row coverage; the draw is one solid run plus a graded contour. Height reference −12 dBFS; the played/unplayed split rides the colour.
                                            const WAVE_FULL_HEIGHT_AMP: f32 = 0.25;
                                            let rows = half as usize;
                                            for (src, up, pc) in sides {
                                                let Some((src_h, src_ch, e)) = src else { continue };
                                                if rows == 0 {
                                                    continue;
                                                }
                                                let key = (src_h, src_ch, cols, rows);
                                                let bars: std::rc::Rc<(Vec<f32>, Vec<u32>)> = {
                                                    let hit = self.wave_fold_cache.borrow().get(&key).cloned();
                                                    match hit {
                                                        Some(a) => a,
                                                        None => {
                                                            let a = std::rc::Rc::new((wave_fold_coverage(&e, cols, rows, WAVE_FULL_HEIGHT_AMP), wave_fold_colours(&e, cols)));
                                                            let mut cache = self.wave_fold_cache.borrow_mut();
                                                            if cache.len() >= 128 {
                                                                cache.clear();
                                                            }
                                                            cache.insert(key, a.clone());
                                                            a
                                                        }
                                                    }
                                                };
                                                let _ = pc;
                                                let (cov, band_cols) = &*bars;
                                                let darker = |c: u32| {
                                                    let (a, d) = (c & 0xFF00_0000, c & 0x00FF_FFFF);
                                                    let dk = |dark: u32| (255 + dark) / 2;
                                                    a | (dk((d >> 16) & 0xFF) << 16) | (dk((d >> 8) & 0xFF) << 8) | dk(d & 0xFF)
                                                };
                                                let colour_of = move |px: usize| if held && frac.is_some() && px < played_cols { band_cols[px] } else { darker(band_cols[px]) };
                                                wave_draw_coverage(&mut canvas, cov, cols, rows, wx0, bcy, up, &colour_of, list_top, list_bottom, Some(list_clip));
                                            }
                                            // Centreline hairline, then the playhead.
                                            if bcy > list_top && bcy < list_bottom {
                                                paint::fill_rect(&mut canvas, wx0 as isize, bcy as isize, (bx1 - wx0) as isize, hair as isize, dim, Some(list_clip), None);
                                            }
                                            if let Some(f) = frac {
                                                let px = wx0 + f * (bx1 - wx0);
                                                let (py0, py1) = (by0.max(list_top), by1.min(list_bottom));
                                                if py1 > py0 {
                                                    paint::fill_rect(&mut canvas, px as isize, py0 as isize, hair.ceil() as isize, (py1 - py0) as isize, *theme::CONTACT_NAME_COLOUR, Some(list_clip), None);
                                                }
                                                // Elapsed / total beside the header, on the side the header left free.
                                                let pos_secs = (f * total_slots as f32 / 100.0) as i64;
                                                let pos_s = super::call_ui::fmt_duration_secs(pos_secs);
                                                let tot_s = super::call_ui::fmt_duration_secs((total_slots / 100) as i64);
                                                let pos_label = tr(Msg::WavePos { pos: &pos_s, total: &tot_s });
                                                if right_aligned {
                                                    ctx.text.draw_text_left(&mut canvas, &pos_label, pad_x, hy, &small, Some(list_clip), None);
                                                } else {
                                                    ctx.text.draw_text_right(&mut canvas, &pos_label, buf_w as f32 - pad_x, hy, &small, Some(list_clip), None);
                                                }
                                            }
                                            // The glyph: ▶ to play, ■ while playing, the hourglass colour's ▶ until the blob is held (a tap fetches).
                                            let glyph = if playing { "\u{25A0}" } else { "\u{25B6}\u{FE0E}" };
                                            let glyph_style = TextStyle::new(msg_size, if held { colour } else { *theme::HOURGLASS_COLOUR }).weight(500).font("Oxanium");
                                            ctx.text.draw_text_left(&mut canvas, glyph, bx0, bcy + msg_size * 0.35, &glyph_style, Some(list_clip), None);
                                            // Hand the band to the input path (slot-indexed beside the row hit).
                                            if wave_slot < self.msg_wave_bands.len() {
                                                self.msg_wave_bands[wave_slot] = Some(super::WaveBand { hash, held, x0: bx0, glyph_x1, x1: bx1, y0: by0.max(list_top), y1: by1.min(list_bottom), total: total_slots });
                                            }
                                        }
                                    }
                                }
                            }
                            // IMAGE PREVIEW BAND: the row's micro thumb (gamma-2 VSF RGB, ≤24 px) drawn above the pill, aspect-preserved, on the bubble's side. Nearest-neighbour up-scale — this is the before-any-fetch tier; the preview blob replaces it when held (Phase 2).
                            if img_lines > 0 {
                                let micro_px = if decoded.is_none() { crate::types::parse_micro_image(&msg.preview).map(|(w, h, px)| (w, h, crate::ui::attach_preview::micro_to_display(px))) } else { None };
                                let picture: Option<(usize, usize, std::borrow::Cow<Vec<u32>>)> = match (decoded, micro_px) {
                                    (Some((w, h, px)), _) => Some((w, h, std::borrow::Cow::Borrowed(px))),
                                    (None, Some((w, h, px))) => Some((w, h, std::borrow::Cow::Owned(px))),
                                    _ => None,
                                };
                                if let Some((tw, th, pixels)) = picture {
                                    // A picture row has no body text (the picture IS the row), so the band anchors straight off the baseline with symmetric insets — the text-row offset left dead padding under every image (Nick 2026-09-12, same fix as the audio band).
                                    let reply_off = if reply_target.is_some() { intra } else { 0.0 };
                                    let pad_v = msg_size * 0.35;
                                    let band_bot = y - react_off - reply_off - pad_v;
                                    let band_top = y - react_off - reply_off - img_band_h + pad_v;
                                    let bh = (band_bot - band_top).max(1.0);
                                    let aspect = msg.attach.and_then(|a| a.dims).map_or(tw as f32 / th as f32, |(w, h): (u32, u32)| w as f32 / h.max(1) as f32);
                                    let avail = buf_w as f32 - pad_x * 2.0;
                                    let bw = (bh * aspect).min(avail);
                                    let bh = if bw < bh * aspect { bw / aspect } else { bh };
                                    let right_aligned = msg.is_outgoing || is_self_contact;
                                    let cx = if right_aligned { buf_w as f32 - pad_x - bw * 0.5 } else { pad_x + bw * 0.5 };
                                    let cy = band_bot - bh * 0.5;
                                    if cy + bh * 0.5 >= list_top && cy - bh * 0.5 <= list_bottom {
                                        paint::draw_image(&mut canvas, &pixels, tw, th, cx, cy, bw, bh, Some(list_clip));
                                    }
                                    // The picture is its own tap target: inside opens the viewer, the rest of the row opens the actions.
                                    if let (Some(a), Some((hash, _, _))) = (msg.attach, crate::types::parse_attachment_content(&msg.content)) {
                                        let slot = vi % super::MSG_HIT_SPAN as usize;
                                        if slot < self.msg_attach_visuals.len() {
                                            self.msg_attach_visuals[slot] = Some(super::AttachVisual { hash, held: crate::storage::blob_present(&hash), kind: a.kind, x0: cx - bw * 0.5, x1: cx + bw * 0.5, y0: (cy - bh * 0.5).max(list_top), y1: (cy + bh * 0.5).min(list_bottom) });
                                        }
                                    }
                                }
                            }
                            // PIGEONS CARRYING WAVES (Nick 2026-09-12): a dropped song's row IS its waveform — the same three-pyramid tensor and cumulative stack a wave card runs, derived off-thread from the held audio thru symphonia; L up, R down, the row's colour.
                            if audio_lines > 0 {
                                if let Some((ahash, _, _)) = crate::types::parse_attachment_content(&msg.content) {
                                    if !self.wave_env.contains_key(&ahash) && !self.wave_env_pending.contains(&ahash) {
                                        if let Some(seed) = self.session.as_ref().map(|se| se.identity_seed) {
                                            if self.wave_env_tx.is_none() {
                                                let (tx, rx) = std::sync::mpsc::channel();
                                                self.wave_env_tx = Some(tx);
                                                self.wave_env_rx = Some(rx);
                                            }
                                            self.wave_env_pending.insert(ahash);
                                            let tx = self.wave_env_tx.as_ref().unwrap().clone();
                                            let wake = self.event_proxy.clone();
                                            let _ = std::thread::Builder::new().name("music-env".into()).spawn(move || {
                                                let res = crate::storage::blob_load(&seed, &ahash).and_then(|b| crate::call::music::envelopes_from_music(&b));
                                                let _ = tx.send((ahash, res));
                                                #[cfg(not(target_os = "android"))]
                                                if let Some(wk) = wake.as_ref() {
                                                    let _ = wk.send(crate::ui::PhotonEvent::NetworkUpdate);
                                                }
                                                #[cfg(target_os = "android")]
                                                let _ = wake;
                                            });
                                        }
                                    }
                                    if let Some(Some(envs)) = self.wave_env.get(&ahash).cloned() {
                                        // An audio row has no body text (the waveform IS the row), so the band anchors straight off the baseline with symmetric insets — the old text-row offset pushed it out of its box (Nick 2026-09-12).
                                        let pad_v = msg_size * 0.35;
                                        let band_bot = y - react_off - pad_v;
                                        let band_top = y - react_off - audio_band_h + pad_v;
                                        let (wx0, wx1) = (pad_x, buf_w as f32 - pad_x);
                                        let bcy = (band_top + band_bot) * 0.5;
                                        let half = (band_bot - band_top).max(2.0) * 0.5;
                                        let cols = ((wx1 - wx0).max(1.0)) as usize;
                                        let rows = half as usize;
                                        const WAVE_FULL_HEIGHT_AMP: f32 = 0.25;
                                        let mfrac = self.music_play.as_ref().filter(|m| m.hash == ahash && m.playing()).map(|m| m.frac());
                                        // Playhead while playing.
                                        if let Some(f) = mfrac {
                                            let phx = wx0 + f * (wx1 - wx0);
                                            let (py0, py1) = (band_top.max(list_top), band_bot.min(list_bottom));
                                            if py1 > py0 {
                                                paint::fill_rect(&mut canvas, phx as isize, py0 as isize, 0, (py1 - py0) as isize, *theme::CONTACT_NAME_COLOUR, Some(list_clip), None);
                                            }
                                        }
                                        let played_cols = mfrac.map(|f| (f * cols as f32) as usize).unwrap_or(0);
                                        let last = envs.len().saturating_sub(1);
                                        for (chn, up) in [(0usize, true), (last, false)] {
                                            let Some(e) = envs.get(chn) else { continue };
                                            if rows == 0 {
                                                continue;
                                            }
                                            let key = (ahash, chn + 4, cols, rows);
                                            let bars: std::rc::Rc<(Vec<f32>, Vec<u32>)> = {
                                                let hit = self.wave_fold_cache.borrow().get(&key).cloned();
                                                match hit {
                                                    Some(a) => a,
                                                    None => {
                                                        let a = std::rc::Rc::new((wave_fold_coverage(e, cols, rows, WAVE_FULL_HEIGHT_AMP), wave_fold_colours(e, cols)));
                                                        let mut cache = self.wave_fold_cache.borrow_mut();
                                                        if cache.len() >= 128 {
                                                            cache.clear();
                                                        }
                                                        cache.insert(key, a.clone());
                                                        a
                                                    }
                                                }
                                            };
                                            // Bright when idle (Nick: "when not playing always shows bright"); playing splits played bright / rest half. Hue is the AGB spectral balance of the three voice bands.
                                            let (cov, band_cols) = &*bars;
                                            let darker = |c: u32| {
                                                let (a, d) = (c & 0xFF00_0000, c & 0x00FF_FFFF);
                                                let dk = |dark: u32| (255 + dark) / 2;
                                                a | (dk((d >> 16) & 0xFF) << 16) | (dk((d >> 8) & 0xFF) << 8) | dk(d & 0xFF)
                                            };
                                            let playing = mfrac.is_some();
                                            let colour_of = move |px: usize| if !playing || px < played_cols { band_cols[px] } else { darker(band_cols[px]) };
                                            wave_draw_coverage(&mut canvas, cov, cols, rows, wx0, bcy, up, &colour_of, list_top, list_bottom, Some(list_clip));
                                        }
                                        if bcy > list_top && bcy < list_bottom {
                                            paint::fill_rect(&mut canvas, wx0 as isize, bcy as isize, (wx1 - wx0) as isize, 0, theme::dim_colour(colour), Some(list_clip), None);
                                        }
                                        // The band is the row's visual: the driver routes its taps (options / the play twelfth / seek-while-playing).
                                        if let Some(a) = msg.attach {
                                            let slot = vi % super::MSG_HIT_SPAN as usize;
                                            if slot < self.msg_attach_visuals.len() {
                                                self.msg_attach_visuals[slot] = Some(super::AttachVisual { hash: ahash, held: true, kind: a.kind, x0: wx0, x1: wx1, y0: band_top.max(list_top), y1: band_bot.min(list_bottom) });
                                            }
                                        }
                                    }
                                }
                            }
                            // A code or text row's preview lines are its visual: the body rect opens the reader, the margins open the actions.
                            if let (Some(a), Some((hash, _, _))) = (msg.attach, crate::types::parse_attachment_content(&msg.content)) {
                                if a.kind.is_text() && !lines.is_empty() {
                                    let slot = vi % super::MSG_HIT_SPAN as usize;
                                    let top = y - react_off - (lines.len() - 1) as f32 * intra - msg_size * 0.6;
                                    let bot = y - react_off + msg_size * 0.6;
                                    if slot < self.msg_attach_visuals.len() {
                                        self.msg_attach_visuals[slot] = Some(super::AttachVisual { hash, held: crate::storage::blob_present(&hash), kind: a.kind, x0: pad_x, x1: buf_w as f32 - pad_x, y0: top.max(list_top), y1: bot.min(list_bottom) });
                                    }
                                }
                            }
                            static NO_LINES: Vec<String> = Vec::new();
                            let body_lines: &Vec<String> = if msg.wave.is_some() { &NO_LINES } else { lines };
                            for (k, line) in body_lines.iter().enumerate() {
                                let ly = y - react_off - (lines.len() - 1 - k) as f32 * intra;
                                let right_aligned = msg.is_outgoing || is_self_contact;
                                // Which marks intersect this line's source range?
                                let segs: Vec<(usize, usize, Option<&str>)> = match line_starts.as_ref() {
                                    Some(starts) => {
                                        let ls = starts[k];
                                        let le = ls + line.len();
                                        let mut cuts: Vec<(usize, usize, Option<&str>)> = Vec::new();
                                        let mut cur = 0usize; // byte offset within the LINE
                                        for m in &msg.marks {
                                            let s0 = m.start.max(ls);
                                            let s1 = (m.start + m.len).min(le);
                                            if s0 >= s1 {
                                                continue;
                                            }
                                            if s0 - ls > cur {
                                                cuts.push((cur, s0 - ls, None));
                                            }
                                            cuts.push((s0 - ls, s1 - ls, Some(m.dest.as_str())));
                                            cur = s1 - ls;
                                        }
                                        if cuts.is_empty() {
                                            Vec::new()
                                        } else {
                                            if cur < line.len() {
                                                cuts.push((cur, line.len(), None));
                                            }
                                            cuts
                                        }
                                    }
                                    None => Vec::new(),
                                };
                                if segs.is_empty() {
                                    if right_aligned {
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
                                    continue;
                                }
                                // Segmented draw: link runs in LINK_COLOUR with an underline hairline + a tap rect; plain runs in the bubble style. Right-aligned lines anchor at (right − full width) so segments flow left→right identically.
                                let link_style = TextStyle::new(msg_size, *theme::LINK_PURPLE).weight(700);
                                let mut x = if right_aligned {
                                    buf_w as f32 - pad_x - ctx.text.measure_text(line, &msg_style)
                                } else {
                                    pad_x
                                };
                                for (b0, b1, dest) in segs {
                                    let run = &line[b0..b1];
                                    let style = if dest.is_some() { &link_style } else { &msg_style };
                                    let w = ctx.text.draw_text_left(
                                        &mut canvas,
                                        run,
                                        x,
                                        ly,
                                        style,
                                        Some(list_clip),
                                        None,
                                    );
                                    if let Some(d) = dest {
                                        // Underline: one ru hairline just under the run, and the tap target (collected here, dispatched in driver — a tap inside opens the consent dialog).
                                        let uy = (ly + msg_size * 0.55).min(list_bottom);
                                        if uy > list_top {
                                            paint::fill_rect(
                                                &mut canvas,
                                                x as isize,
                                                uy as isize,
                                                w as isize,
                                                ctx.viewport.ru.max(1.0) as isize,
                                                *theme::LINK_PURPLE,
                                                None,
                                                None,
                                            );
                                        }
                                        if self.link_consent.is_none() {
                                            self.msg_link_hits.push((
                                                x,
                                                ly - msg_size * 0.6,
                                                x + w,
                                                ly + msg_size * 0.7,
                                                d.to_string(),
                                            ));
                                        }
                                    }
                                    x += w;
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
                            // Stamp the row band — the WHOLE wrapped block — so a tap selects this message (details strip). Clamped to the list region so header/compose never lose their own hits. MODULAR SLOTS, not a spend-down budget (Nick 2026-09-05, after the field bug where the walk's off-screen rows below the viewport exhausted a 64-id budget before any visible row got a target): the slot is `visible_index % MSG_HIT_SPAN`, so every row HAS an id by construction and there is nothing to exhaust. Stamp + table-write only when the clamped band is non-empty (= on screen), which keeps map and table in lockstep and makes wrap collisions need 256+ rows on one screen.
                            let band_top = ((y - block_extra - line_h * 0.5).max(list_top)) as isize;
                            let band_bot = ((y + line_h * 0.5).min(list_bottom)) as isize;
                            if band_bot > band_top {
                                let slot = vi % super::MSG_HIT_SPAN as usize;
                                let row_hit = self.msg_hit_base.wrapping_add(slot as HitId);
                                restamp_hit_rect(
                                    &mut chrome.hit_test_map,
                                    buf_w,
                                    buf_h,
                                    0,
                                    band_top,
                                    buf_w as isize,
                                    band_bot,
                                    row_hit,
                                );
                                // A reply row's reference line is its own tap target: the band + the referenced ts ride the hit row, and a tap inside it JUMPS to the source row instead of opening the strip.
                                let ref_band = reply_target.map(|t| {
                                    let ref_y = y - react_off - lines.len() as f32 * intra;
                                    (ref_y - line_h * 0.5, ref_y + line_h * 0.5, t)
                                });
                                self.msg_hit_rows[slot] =
                                    Some((msg.timestamp, msg.is_outgoing, ref_band));
                            }
                            y -= line_h + block_extra + sel_meta_extra;
                        }
                        // Re-asserted after the row walk (see the filter pill above the walk).
                        if let Some((r, hid)) = filter_stamp {
                            restamp_hit_rect(&mut chrome.hit_test_map, buf_w, buf_h, r.x as isize, r.y as isize, (r.x + r.w) as isize, (r.y + r.h) as isize, hid);
                        }
                        // Consent panel hit re-assert (the row walk stamped over it): HIT_NONE swallows everything under the panel, then the three pills win their own rects back.
                        if let Some((prects, py0)) = consent_stamp {
                            restamp_hit_rect(&mut chrome.hit_test_map, buf_w, buf_h, 0, py0 as isize, buf_w as isize, list_bottom as isize, HIT_NONE);
                            for (pi, r) in prects.iter().enumerate() {
                                restamp_hit_rect(&mut chrome.hit_test_map, buf_w, buf_h, r.x as isize, r.y as isize, (r.x + r.w) as isize, (r.y + r.h) as isize, self.link_consent_base.wrapping_add(pi as HitId));
                            }
                        }
                        // STREAM ENTRY #0 — avatar, name, optional ceremony/lifecycle status: drawn ONLY when the walk reached message 1 (genesis on screen); `y` then sits just above it and the entry is the stream's literal first item. Ordinary stream content: same clip as every message, no pinning, no slide. Off-screen anywhere but genesis.
                        if reached_oldest && y > list_top - header_block_h - line_h {
                            let (block_status, status_h) = match &status_wrapped {
                                Some((lines, colour)) => (
                                    Some((lines.clone(), *colour)),
                                    unit * 0.25 + unit * 0.75 * lines.len() as f32,
                                ),
                                None => (None, 0.0),
                            };
                            let block_name_y = y - unit * 0.2 - status_h;
                            let block_avatar_cy = block_name_y - header_name_extra - unit * 1.2 - avatar_r;
                            draw_conv_avatar(&mut canvas, block_avatar_cy, Some(list_clip));
                            // Bottom-anchored like the status: the LAST name line sits at block_name_y, earlier lines stack upward at the name pitch.
                            for (k, line) in header_name_lines.iter().enumerate() {
                                ctx.text.draw_text_center(
                                    &mut canvas,
                                    line,
                                    buf_w as f32 * 0.5,
                                    block_name_y - unit * 0.9 * (header_name_lines.len() - 1 - k) as f32,
                                    &header_style,
                                    Some(list_clip),
                                    None,
                                );
                            }
                            if let Some((lines, colour)) = block_status {
                                // Bottom-anchored: the LAST wrapped line sits where the old single line sat; extra lines stack upward (the name above already yielded via status_h).
                                let pitch = unit * 0.75;
                                let mut ly = y - unit * 0.2 - pitch * (lines.len() as f32 - 1.0);
                                for line in &lines {
                                    ctx.text.draw_text_center(
                                        &mut canvas,
                                        line,
                                        buf_w as f32 * 0.5,
                                        ly,
                                        &TextStyle::new(unit * 0.6, colour)
                                            .weight(500)
                                            .font("Oxanium"),
                                        Some(list_clip),
                                        None,
                                    );
                                    ly += pitch;
                                }
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
                                1 => tr(Msg::EditingSnippet(&snippet)).into_owned(),
                                2 => tr(Msg::ReactSnippet(&snippet)).into_owned(),
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
                                // A REAL pill (Nick 2026-08-31: "make that stop an actual button, not just text") — the shared fluor renderer, red fill from the destructiveness ramp; hover/press come free, hit-stamped shape-accurate by fluor itself.
                                let pill_h = unit * 1.15;
                                let pill_w = unit * 3.2;
                                let rect = fluor::region::Region::new(
                                    buf_w as f32 - pad_x - pill_w,
                                    strip_y - pill_h * 0.62,
                                    pill_w,
                                    pill_h,
                                );
                                draw_stub_pill_filled(
                                    &mut canvas,
                                    ctx.text,
                                    &mut chrome.hit_test_map,
                                    buf_w,
                                    buf_h,
                                    rect,
                                    &tr(Msg::StopPill),
                                    self.msg_action_base.wrapping_add(5),
                                    ctx.pressed_hit,
                                    true,
                                    Some(*theme::PILL_RED),
                                    "Oxanium",
                                );
                            }
                        }

                        // ── Compose box (pinned bottom) ──────────────────────────── Shown when THIS device can dispatch — the pre-chrome `compose_ready` snapshot, the same one definition the focus walk reads. No placeholder text: the box's position says what it's for (Nick, 2026-08-09 — the hint lingered after sends and earned nothing).
                        if compose_ready {
                            // LINK BUTTON (purple, chain link) beside send, only while the box holds a bare URL to convert.
                            if link_btn_visible {
                                if let Some(btn) = self.compose_link_btn.as_mut() {
                                    let id = btn.hit_id();
                                    btn.render_content_into(&mut canvas, 0., 0., ctx.text, None, Some(&mut chrome.hit_test_map), id);
                                }
                            }
                            // PAPERCLIP, always: the attachment picker (Android) / the drop hint (desktop).
                            if let Some(btn) = self.compose_attach_btn.as_mut() {
                                let id = btn.hit_id();
                                btn.render_content_into(&mut canvas, 0., 0., ctx.text, None, Some(&mut chrome.hit_test_map), id);
                            }
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
                            // The link button sits inside the textbox's rect too, so it needs the same re-stamp or its hover, hand cursor and click all land on the textbox (Nick 2026-09-09: "needs button hover effects and hand").
                            if link_btn_visible {
                                if let Some(btn) = self.compose_link_btn.as_ref() {
                                    btn.stamp_hit_into(&mut chrome.hit_test_map, buf_w, buf_h, btn.hit_id());
                                }
                            }
                            if let Some(btn) = self.compose_attach_btn.as_ref() {
                                btn.stamp_hit_into(&mut chrome.hit_test_map, buf_w, buf_h, btn.hit_id());
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
                let back_text = tr(Msg::BackToContacts);
                ctx.text.draw_text_left(
                    &mut canvas,
                    &back_text,
                    unit,
                    back_y,
                    &TextStyle::new(back_size, *theme::CONTACT_NAME_COLOUR)
                        .weight(500)
                        .font("Oxanium"),
                    None,
                    None,
                );
                let back_w = ctx.text.measure_text(
                    &back_text,
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
                &tr(Msg::AddDeviceTitle),
                cx,
                tb_cy - u * 2.5,
                &TextStyle::new(u * 0.85, *theme::STATUS_TEXT_COLOUR)
                    .weight(600)
                    .font("Oxanium"),
                None,
                None,
            );
            let subtitle = if self.add_device_bound.is_none() {
                tr(Msg::TypeWords)
            } else if self.add_device_checking {
                "".into() // Words path: bound + auto-rotating; the status row below carries "Adding…".
            } else {
                // BLE/tap path only: load-bearing — the human must check the FAR (new) device's screen, not this one.
                tr(Msg::AddDeviceConfirmOnce)
            };
            ctx.text.draw_text_center(
                &mut canvas,
                &subtitle,
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
                let counter = format!(
                    "{} / {}",
                    crate::fmt_num(count as u32),
                    crate::fmt_num(crate::network::fgtw::fleet::PAIR_WORD_COUNT as u32)
                );
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
                        &tr(Msg::TapNearby),
                        cx,
                        y + u * 0.2,
                        &TextStyle::new(u * 0.4, fluor::theme::HINT_COLOUR).font("Oxanium"),
                        None,
                        None,
                    );
                    y += u * 0.4 + gap * 0.5;
                    let row_h = u * 0.85;
                    for (i, cand) in nearby.iter().enumerate() {
                        let label = tr(Msg::AddDeviceNearby(&cand.name));
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
                    &tr(Msg::YesGreenFinish),
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
                &tr(Msg::TapOrbCancel),
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
                &tr(Msg::SettingsTitle),
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
                    &tr(Msg::SettingsBack),
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
                // Full rail column, and up to the rail's true top — the hint rectangles wear no padding (Nick 2026-09-02).
                paint::fill_rect(
                    &mut canvas,
                    layout.rail.x as isize,
                    layout.rail.y as isize,
                    layout.rail.w as isize,
                    (r.bottom() - layout.rail.y) as isize,
                    fill,
                    None,
                    None,
                );
                restamp_hit_rect(
                    &mut chrome.hit_test_map,
                    buf_w,
                    buf_h,
                    layout.rail.x as isize,
                    layout.rail.y as isize,
                    layout.rail.right() as isize,
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
                    &tr(Msg::PageName(*p)),
                    r.x + rspan * 0.6,
                    r.center_y(),
                    &TextStyle::new(rspan, colour)
                        .weight(if active { 600 } else { 400 })
                        .font("Oxanium"),
                    Some(pages_clip),
                    None,
                );
                if held {
                    // Held (pointer down, release switches to this page) reads brightest — FULL rail column, edge to edge (Nick 2026-09-02).
                    paint::fill_rect(
                        &mut canvas,
                        layout.rail.x as isize,
                        r.y as isize,
                        layout.rail.w as isize,
                        r.h as isize,
                        fluor::theme::BUTTON_HELD,
                        Some(pages_clip),
                        None,
                    );
                } else if active {
                    // Active-row backing bar — full rail column, at HALF the old separator opacity (it read too bright).
                    paint::fill_rect(
                        &mut canvas,
                        layout.rail.x as isize,
                        r.y as isize,
                        layout.rail.w as isize,
                        r.h as isize,
                        theme::RAIL_ACTIVE_COLOUR,
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
                                YouRow::FieldBox(idx) => {
                                    self.you_fields[*idx].tb.reset_paint_tracking();
                                }
                                YouRow::FieldLabel(idx) => {
                                    if let Some(tag) = self.you_fields[*idx].tag_tb.as_mut() {
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
                            YouRow::Header(tier) => {
                                // The plan carries the tier ID; the label translates at this draw edge (unknown ids can't happen, the raw id is the guard fallback).
                                let title = profile_tier_label(tier)
                                    .unwrap_or(std::borrow::Cow::Borrowed(*tier));
                                ctx.text.draw_text_left(
                                    &mut canvas,
                                    &title,
                                    r.x + tspan * 0.3,
                                    r.center_y(),
                                    &TextStyle::new(tspan, *theme::CONTACT_NAME_COLOUR)
                                        .weight(600)
                                        .font("Oxanium"),
                                    Some(content_clip),
                                    None,
                                );
                            }
                            YouRow::ShareHint(n) => {
                                // What the box means, on its own lines under the tier title (Nick 2026-09-10: below, not beside; a line return where the semicolon was).
                                let hint = tr(Msg::YouShareHint);
                                if let Some(line) = hint.split('\n').nth(*n) {
                                    ctx.text.draw_text_left(
                                        &mut canvas,
                                        line,
                                        r.x + tspan * 0.3,
                                        r.center_y(),
                                        &TextStyle::new(hspan2 * 0.8, *theme::LABEL_COLOUR).font("Oxanium"),
                                        Some(content_clip),
                                        None,
                                    );
                                }
                            }
                            YouRow::FieldLabel(idx) => {
                                // Label line (Nick 2026-09-10): the title sits LEFT-CENTRED — centred on the pane's one-third mark, "an average of left aligned and centre aligned", so left edges deliberately do not line up; the tag box rides the right end (laid out in input.rs on the same rect).
                                let label = self.you_fields[*idx].label.clone();
                                // The text's OWN one-third point lands on the pane's one-third mark (not its centre): x = pane third − text width / 3.
                                let style = TextStyle::new(hspan2, *theme::LABEL_COLOUR).font("Oxanium");
                                let tw = ctx.text.measure_text(&label, &style);
                                // Vertically: a quarter of the way up from the box top toward the hairline above (the row's top edge), not the row centre — it sat too high.
                                let box_top = r.bottom() + r.h * 0.44 - layout.unit * 0.6;
                                let label_y = box_top - (box_top - r.y) * 0.25;
                                ctx.text.draw_text_left(
                                    &mut canvas,
                                    &label,
                                    r.x + r.w / 3.0 - tw / 3.0,
                                    label_y,
                                    &style,
                                    Some(content_clip),
                                    None,
                                );
                                let pf = &mut self.you_fields[*idx];
                                if let Some(tag) = pf.tag_tb.as_mut() {
                                    let tid = tag.hit_id();
                                    tag.render_content_into(&mut canvas, 0., 0., ctx.text, Some(glow_clip), None, Some(&mut chrome.hit_test_map), tid);
                                }
                            }
                            YouRow::FieldBox(idx) => {
                                // Value line: the box with its share checkbox touching its right end, then the chat screen's hairline (pure white at α=1/8, one ru thick) across the WHOLE content pane — edge to edge, no inset.
                                let pf = &mut self.you_fields[*idx];
                                let id = pf.tb.hit_id();
                                pf.tb.render_content_into(&mut canvas, 0., 0., ctx.text, Some(glow_clip), None, Some(&mut chrome.hit_test_map), id);
                                if let Some(cb) = pf.share_cb.as_mut() {
                                    cb.render_content_into(&mut canvas, ctx.text, Some(content_clip), Some(&mut chrome.hit_test_map));
                                }
                                let ru = ctx.viewport.ru.max(1.0);
                                pane_hairline(&mut canvas, &layout, r.bottom() - ru, ru, Some(glow_clip));
                            }
                            YouRow::AddHeader => {
                                ctx.text.draw_text_left(
                                    &mut canvas,
                                    &tr(Msg::YouAddCustomField),
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
                            }
                            YouRow::AddPill => {
                                let cols = r.split_h([0.38, 0.62]);
                                draw_stub_pill(
                                    &mut canvas,
                                    ctx.text,
                                    &mut chrome.hit_test_map,
                                    buf_w,
                                    buf_h,
                                    cols[0].center_h(0.72),
                                    &tr(Msg::Add),
                                    btn_base.wrapping_add(2),
                                    ctx.pressed_hit,
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
                                    &tr(Msg::Update),
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
                                    &tr(Msg::ChangeAvatar),
                                    btn_base.wrapping_add(1),
                                    ctx.pressed_hit,
                                );
                            }
                        }
                    }
                }
                SettingsPage::Fleet => {
                    // FLEET, Flow rework (Nick 2026-09-02: "shit overlaps like crazy... much more vertical layout... air top/bottom of each device"): one VERTICAL card per device — name (tap-to-copy), status line, build line, then its action pills on their own band — with real air between cards. Hit-id bands unchanged (copy 16+i · bridge 8+i · release 24+i · lock 32+i · unlock 40+i · approve 48+i · rename 56+i · add 0), so the dispatch arm stays band-shaped. Extent is MEASURED from the flow cursor.
                    let locked_set = &fleet_locked_set;
                    let devices = &fleet_devices;
                    let inset = layout.content_inset();
                    let mut flow = Flow::new(inset, settings_content_scroll);
                    flow.line(&mut canvas, ctx.text, &tr(Msg::FleetTitle), tspan, *theme::CONTACT_NAME_COLOUR, 600);
                    flow.line(&mut canvas, ctx.text, &tr(Msg::FleetTapToCopy), hspan2 * 0.82, *theme::LABEL_COLOUR, 400);
                    flow.gap(hspan2 * 0.8);
                    for (i, (pk, is_self, online, retired, name, link, tier, about)) in
                        devices.iter().enumerate()
                    {
                        let row_locked = locked_set.contains(pk);
                        let tier_colour = super::shown_tier_colour(*tier);
                        let tier_word = super::tier_label(*tier);
                        // NAME band — tap-to-copy stamped over it. The NAME's colour is the transport state (Nick 2026-09-10: no dot — LAN / WFD / WAN / relay in the path colours, offline in the label grey).
                        let name_band = flow.band(hspan2 * 1.7);
                        let name_colour = tier_colour.unwrap_or(*theme::LABEL_COLOUR);
                        // Renaming THIS card: the name band IS the textbox (prefilled, focused); no tap-to-copy stamp while editing. Enter commits, Esc cancels (driver).
                        let renaming_here = self.fleet_rename.as_ref().is_some_and(|(rpk, _)| rpk == pk);
                        if renaming_here {
                            if let Some((_, tb)) = self.fleet_rename.as_mut() {
                                // set_rect takes CENTER x — passing the text's LEFT edge (as this did) centred the box there and threw half its width back across the nav rail, which is why renaming looked like it spanned the whole window. Span exactly the column the name and pills occupy: from the name's left edge to the card's right margin.
                                let tb_left = name_band.x + hspan2 * 0.3;
                                let tb_w = (name_band.right() - hspan2 * 0.5 - tb_left).max(hspan2 * 6.0);
                                tb.set_rect(tb_left + tb_w * 0.5, name_band.center_y(), tb_w, name_band.h * 0.9);
                                tb.set_font_size(hspan2, ctx.text);
                                let id = tb.hit_id();
                                tb.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, Some(&mut chrome.hit_test_map), id);
                            }
                        } else {
                            ctx.text.draw_text_left(
                                &mut canvas,
                                name,
                                name_band.x + hspan2 * 0.3,
                                name_band.center_y(),
                                &TextStyle::new(hspan2 * 1.05, name_colour)
                                    .weight(600)
                                    .font("Oxanium"),
                                None,
                                None,
                            );
                            restamp_hit_rect(
                                &mut chrome.hit_test_map,
                                buf_w,
                                buf_h,
                                name_band.x as isize,
                                name_band.y as isize,
                                name_band.right() as isize,
                                name_band.bottom() as isize,
                                btn_base.wrapping_add(16 + i as HitId),
                            );
                        }
                        // The name is the card's headline — let it breathe before the status beneath it (Nick 2026-09-08).
                        flow.gap(hspan2 * 0.35);
                        // STATUS line: state + the PATH NAMED, not merely coloured — the dot's colour and this word come from the same resolution (path_tier_shown), so they can't drift apart.
                        let (status, status_colour) = if *is_self {
                            (tr(Msg::ThisDevice), *theme::LABEL_COLOUR)
                        } else if *retired {
                            (tr(Msg::RetiredStillYours), *theme::LABEL_COLOUR)
                        } else if row_locked {
                            (tr(Msg::RevokedBadge), theme::PILL_RED.1)
                        } else if *online {
                            (
                                match tier_word.as_deref() {
                                    Some(w) => tr(Msg::OnlineVia(w)),
                                    None => tr(Msg::Online),
                                },
                                *theme::SEARCH_FOUND_COLOUR,
                            )
                        } else {
                            (tr(Msg::Offline), *theme::LABEL_COLOUR)
                        };
                        flow.line(&mut canvas, ctx.text, &status, hspan2 * 0.85, status_colour, 400);
                        // The CLUTCH ladder detail is its own sentence — it used to be smuggled into the status line, which is why the transport never had room to be named.
                        if !link.is_empty() && !*is_self {
                            flow.line(&mut canvas, ctx.text, link, hspan2 * 0.75, *theme::LABEL_COLOUR, 400);
                        }
                        // BUILD line — version · commit · os arch off the sealed pong tail (self shows its own build). A stale version here IS the not-updated indicator.
                        if !about.is_empty() {
                            flow.line(&mut canvas, ctx.text, &super::about_line_display(about), hspan2 * 0.7, *theme::LABEL_COLOUR, 400);
                        }
                        // ACTION pills on their own band — flow_pills sizes to labels and wraps if the pane clamps.
                        let departing = self
                            .pending_depart_req
                            .as_ref()
                            .is_some_and(|(d, _, _, _, _)| d == pk);
                        // ACTION pills — flow_pills sizes each to its label and WRAPS to the next band when the column runs out (Nick 2026-09-08: "if the user scales it such that Bridge / Rename / Revoke are too wide, revoke should wrap to the next line"). Same helper everywhere, so no page hand-rolls a row that can overflow.
                        if *retired {
                            let armed = self.fleet_release_armed.as_ref() == Some(pk);
                            flow_pills(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, ctx.pressed_hit, hspan2 * 0.9, &[
                                (&tr(Msg::ReleasePill { armed }), btn_base.wrapping_add(24 + i as HitId), true, Some(*theme::PILL_RED)),
                            ], "Oxanium");
                        } else if *is_self {
                            // The self card's one action: Rename — this machine's name is the one most worth setting.
                            flow_pills(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, ctx.pressed_hit, hspan2 * 0.9, &[
                                (&tr(Msg::RenamePill), btn_base.wrapping_add(56 + i as HitId), true, None),
                            ], "Oxanium");
                        } else {
                            // Bridge + Rename + the row's state pill (Revoke / Reinstate / Approve departure). Revoke wears PILL_RED unarmed as well as armed — it is the hostile verb on this page and shouldn't have to be tapped once to look like it.
                            let mut pills: Vec<(std::borrow::Cow<'static, str>, HitId, bool, Option<(u32, u32)>)> = Vec::with_capacity(3);
                            let bridge_fill = if *online { Some(*theme::PILL_GREEN) } else { Some(*theme::PILL_GREY) };
                            pills.push((tr(Msg::BridgePill), btn_base.wrapping_add(8 + i as HitId), true, bridge_fill));
                            pills.push((tr(Msg::RenamePill), btn_base.wrapping_add(56 + i as HitId), true, None));
                            if departing {
                                let armed = self.fleet_approve_armed.as_ref() == Some(pk);
                                pills.push((tr(Msg::ApproveSignOutPill { armed }), btn_base.wrapping_add(48 + i as HitId), true, Some(if armed { *theme::PILL_RED } else { *theme::PILL_YELLOW })));
                            } else if row_locked {
                                let armed = self.fleet_unlock_armed.as_ref() == Some(pk);
                                pills.push((tr(Msg::ReinstatePill { armed }), btn_base.wrapping_add(40 + i as HitId), true, if armed { Some(*theme::PILL_RED) } else { None }));
                            } else {
                                let armed = self.fleet_lock_armed.as_ref() == Some(pk);
                                pills.push((tr(Msg::RevokePill { armed }), btn_base.wrapping_add(32 + i as HitId), true, Some(*theme::PILL_RED)));
                            }
                            let refs: Vec<(&str, HitId, bool, Option<(u32, u32)>)> = pills.iter().map(|(l, h, e, f)| (l.as_ref(), *h, *e, *f)).collect();
                            flow_pills(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, ctx.pressed_hit, hspan2 * 0.9, &refs, "Oxanium");
                        }
                        // DEPARTURE ceremony bands (2026-09-04): the declared intent under the pills, and — once Approve is tapped on a words-carrying request — the words-entry box that gates the countersign (type what the departing device's screen shows).
                        let departing_here = self.pending_depart_req.as_ref().is_some_and(|(d, _, _, _, _)| d == pk);
                        if departing_here {
                            let intent = self.pending_depart_req.as_ref().map(|(_, _, _, it, _)| *it).unwrap_or(0);
                            if intent != 0 {
                                flow.line(
                                    &mut canvas,
                                    ctx.text,
                                    &tr(if intent == 1 { Msg::DepartIntentNewOwner } else { Msg::DepartIntentDesk }),
                                    hspan2 * 0.85,
                                    if intent == 1 { theme::PILL_YELLOW.1 } else { *theme::LABEL_COLOUR },
                                    600,
                                );
                            }
                            let entry_here = self.depart_words_entry.as_ref().is_some_and(|(d, _)| d == pk);
                            if entry_here {
                                flow.line(&mut canvas, ctx.text, &tr(Msg::DepartWordsPrompt), hspan2 * 0.85, *theme::CONTACT_NAME_COLOUR, 600);
                                let tb_band = flow.band(hspan2 * 2.0);
                                if let Some((_, tb)) = self.depart_words_entry.as_mut() {
                                    tb.set_rect(tb_band.x + tb_band.w * 0.35, tb_band.center_y(), tb_band.w * 0.66, tb_band.h * 0.9);
                                    tb.set_font_size(hspan2 * 0.95, ctx.text);
                                    let id = tb.hit_id();
                                    tb.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, Some(&mut chrome.hit_test_map), id);
                                }
                            }
                        }
                        // AIR between device cards — the whole point. Between cards (never after the last) the conversation's white hairline rides the midpoint (Nick 2026-09-03: "same white hairlines between messages"): pure white α=1/8 = VERSION_COLOUR, the between-messages divider treatment.
                        flow.gap(hspan2 * 0.6);
                        if i + 1 < devices.len() {
                            let ru = ctx.viewport.ru.max(1.0);
                            let hl = flow.band(ru as Coord);
                            pane_hairline(&mut canvas, &layout, hl.y, ru, None);
                            flow.gap(hspan2 * 0.6);
                        }
                    }
                    // No Remove pill: expulsion is not a verb (sovereign records) — a device leaves by its own signed departure. And leaving never frees the hardware: the brand outlives the membership until the owner releases it.
                    let single_copy = !devices
                        .iter()
                        .any(|(_, is_self, _, retired, ..)| !*is_self && !*retired);
                    if single_copy {
                        flow.prose(&mut canvas, ctx.text, &tr(Msg::SingleCopyWarning), hspan2, theme::PILL_RED.1, 500);
                    } else {
                        flow.prose(&mut canvas, ctx.text, &tr(Msg::DeviceSignsItselfOut), hspan2, *theme::LABEL_COLOUR, 400);
                    }
                    flow.gap(hspan2 * 0.5);
                    // Rename is per-card now (band 56+i) — the page-level stub pill retired with it.
                    let add_device_label = tr(Msg::AddDevicePill);
                    flow_pills(
                        &mut flow,
                        &mut canvas,
                        ctx.text,
                        &mut chrome.hit_test_map,
                        buf_w,
                        buf_h,
                        ctx.pressed_hit,
                        hspan2,
                        &[(add_device_label.as_ref(), btn_base, true, None)],
                        "Open Sans",
                    );
                    flow.gap(hspan2);
                    measured_extent = Some((flow.used(), inset.h));
                }
                SettingsPage::Language => {
                    // LANGUAGE (Nick 2026-09-03: "separate Language page at the bottom of settings but above About... buttons for each language, rather than a dropdown that's hard to find"): one button per language, labelled in ITSELF (autonyms — a lost user must always recognise their own tongue). The current choice is the filled pill; a tap persists device-local display.lang and the whole UI re-renders thru tr().
                    let inset = layout.content_inset();
                    let mut flow = Flow::new(inset, settings_content_scroll);
                    flow.line(&mut canvas, ctx.text, &tr(Msg::LanguageLabel), tspan, *theme::CONTACT_NAME_COLOUR, 600);
                    flow.gap(hspan2 * 0.8);
                    let current = crate::ui::lang::lang();
                    for (i, l) in crate::ui::lang::Lang::ALL.iter().enumerate() {
                        let band = flow.band(hspan2 * 2.6);
                        let selected = *l == current;
                        draw_stub_pill_filled(
                            &mut canvas,
                            ctx.text,
                            &mut chrome.hit_test_map,
                            buf_w,
                            buf_h,
                            fluor::region::Region::new(band.x + hspan2 * 0.3, band.y + band.h * 0.08, (band.w * 0.6).max(hspan2 * 10.0).min(band.w - hspan2 * 0.6), band.h * 0.84),
                            l.autonym(),
                            btn_base.wrapping_add(i as HitId),
                            ctx.pressed_hit,
                            true,
                            if selected { Some(*theme::PILL_GREEN) } else { None },
                            "Oxanium",
                        );
                        flow.gap(hspan2 * 0.5);
                    }
                    measured_extent = Some((flow.used(), inset.h));
                }
                SettingsPage::Security => {
                    // SECURITY, Flow rework (Nick 2026-09-02): SHORT pill labels with the explanation as a wrapped hint BELOW each pill (the parentheticals moved out of the buttons), everything wrapping at the pane edge. Destructiveness ramp unchanged: Lock (green, reversible) · Remove (yellow) · Shred (orange) · Remove & shred (red); the wipers stay two-tap + mutually exclusive; hit ids unchanged (0..3 + the unattended base).
                    let inset = layout.content_inset();
                    let mut flow = Flow::new(inset, settings_content_scroll);
                    flow.line(&mut canvas, ctx.text, &tr(Msg::PageName(page)), tspan, *theme::CONTACT_NAME_COLOUR, 600);
                    flow.prose(&mut canvas, ctx.text, &tr(Msg::SecurityIntro), hspan2, *theme::LABEL_COLOUR, 400);
                    flow.gap(hspan2 * 0.6);
                    // One pill + its hint, flowed. Armed actions turn their hint red + bold — the confirm state IS the explanation.
                    let action = |flow: &mut Flow,
                                      canvas: &mut Canvas,
                                      text: &mut fluor::text::TextRenderer,
                                      hit_map: &mut [HitId],
                                      label: &str,
                                      hint: &str,
                                      slot: HitId,
                                      fill: (u32, u32),
                                      armed: bool,
                                      enabled: bool| {
                        let band = flow.band(hspan2 * 2.4);
                        let pill_h = band.h * 0.8;
                        let w = text
                            .measure_text(label, &TextStyle::new(pill_h * 0.5, 0))
                            + pill_h * 0.8
                            + hspan2 * 0.4;
                        let rect = fluor::region::Region::new(
                            band.x + hspan2 * 0.3,
                            band.y + (band.h - pill_h) * 0.5,
                            w.min(band.w - hspan2 * 0.6),
                            pill_h,
                        );
                        // A disabled pill stamps NO hit id (draw_stub_pill_filled's contract), so a fleet-of-one can't arm what it could never complete — the greying is the gate, not decoration.
                        draw_stub_pill_filled(canvas, text, hit_map, buf_w, buf_h, rect, label, btn_base.wrapping_add(slot), ctx.pressed_hit, enabled, Some(if enabled { fill } else { *theme::PILL_GREY }), "Open Sans");
                        let (hc, hw) = if armed { (*theme::ERROR_TEXT_COLOUR, 600) } else { (*theme::LABEL_COLOUR, 400) };
                        let region = fluor::region::Region::new(flow.x, flow.y, flow.w, hspan2 * 1.6);
                        let n = settings_prose(canvas, text, region, hint, hspan2 * 0.85, hc, hw);
                        flow.y += (n.max(1) as Coord) * hspan2 * 0.85 * 1.25 + hspan2 * 0.9;
                    };
                    // Lock and Wipe are this device's own business and always available, even to a fleet of one. Revoke and Release both REQUIRE another device — one to reinstate us, one to countersign the departure — so on a lone device they render dead with the reason in place of the hint, rather than arming into a refusal toast.
                    let fleet_verbs = has_sibling_device;
                    action(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map,
                        &tr(Msg::SecurityLock),
                        &tr(Msg::SecurityLockHint),
                        0, *theme::PILL_GREEN, false, true);
                    // KILL (Nick 2026-09-10): Lock plus the process ending — one tap, no arm state. The hint carries the whole contract.
                    action(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map,
                        &tr(Msg::SecurityKill),
                        &tr(Msg::SecurityKillHint),
                        4, *theme::PILL_RED, false, true);
                    action(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map,
                        &tr(Msg::SecurityRevoke { armed: self.settings_revoke_armed }),
                        &tr(if fleet_verbs { Msg::SecurityRevokeHint } else { Msg::SecurityRevokeAloneHint }),
                        1, if self.settings_revoke_armed { *theme::PILL_RED } else { *theme::PILL_YELLOW }, self.settings_revoke_armed, fleet_verbs);
                    action(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map,
                        &tr(Msg::SecurityShred { armed: self.settings_shred_armed }),
                        &tr(Msg::SecurityShredHint),
                        2, *theme::PILL_ORANGE, self.settings_shred_armed, true);
                    action(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map,
                        &tr(Msg::SecurityRemoveShred { armed: self.settings_removeshred_armed }),
                        &tr(if fleet_verbs { Msg::SecurityRemoveShredHint } else { Msg::SecurityReleaseAloneHint }),
                        3, *theme::PILL_RED, self.settings_removeshred_armed, fleet_verbs);
                    flow.gap(hspan2 * 0.4);
                    // LEAVER's pending departure: the approval words, big and Oxanium, plus the waiting line — this screen IS the ceremony's display half until the de-fold completes (or relaunch clears it).
                    if self.depart_request_t.is_some() {
                        if let Some(words) = self.depart_words.clone() {
                            flow.line(&mut canvas, ctx.text, &tr(Msg::DepartWordsShow(&words)), hspan2 * 1.25, *theme::SEARCH_FOUND_COLOUR, 700);
                            flow.prose(&mut canvas, ctx.text, &tr(Msg::DepartWaitingLine), hspan2 * 0.9, *theme::LABEL_COLOUR, 400);
                            flow.gap(hspan2 * 0.6);
                        }
                    }
                    flow.line(&mut canvas, ctx.text, &tr(Msg::SecurityStatusLine), hspan2, *theme::LABEL_COLOUR, 400);
                    flow.gap(hspan2 * 0.8);
                    // ── Load on startup (Nick 2026-09-03: auto-attest does no good unless the app also LOADS on reboot — the two belong side by side). The OS artifact IS the setting (platform::autostart, default-ON); the dispatch in protocol.rs works from any page, only the render lives here.
                    #[cfg(not(target_os = "android"))]
                    {
                        let cb_band = flow.band(hspan2 * 2.0);
                        if let Some(cb) = self.settings_background_check.as_mut() {
                            let label = tr(Msg::LoadOnStartup);
                            cb.set_label(&*label);
                            cb.set_font_size(hspan2);
                            let cb_h = hspan2 * 1.3;
                            let label_w = ctx.text.measure_text(&label, &TextStyle::new(hspan2, 0));
                            let w = cb_h + hspan2 * 0.5 + label_w + hspan2 * 0.3;
                            cb.set_rect(cb_band.x + w * 0.5, cb_band.center_y(), w, cb_h);
                            cb.render_content_into(
                                &mut canvas,
                                ctx.text,
                                None,
                                Some(&mut chrome.hit_test_map),
                            );
                        }
                        flow.prose(&mut canvas, ctx.text, &tr(Msg::LoadOnStartupExplainer), hspan2 * 0.9, *theme::LABEL_COLOUR, 400);
                        flow.gap(hspan2 * 0.8);
                    }
                    // ── Bulletproof bridge (Nick 2026-09-07): the headless-lifeline watcher (docs/headless-lifeline.md) as a checkbox — the OS artifact IS the setting (platform::lifeline); the toggle dispatch rides protocol.rs like every settings checkbox.
                    #[cfg(any(target_os = "linux", target_os = "macos"))]
                    {
                        if let Some(cb) = self.settings_lifeline_check.as_mut() {
                            flow_checkbox(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map, cb, &tr(Msg::LifelineCheckbox), hspan2);
                        }
                        flow.prose(&mut canvas, ctx.text, &tr(Msg::LifelineExplainer), hspan2 * 0.9, *theme::LABEL_COLOUR, 400);
                        flow.gap(hspan2 * 0.8);
                    }
                    // ── DANGEROUS: unattended auto-attest-on-reboot. Off by default. Two states, both INLINE (no floating overlay — an over-content modal drawn after chrome.flatten_into never composited its glyphs): the checkbox+disclaimer, OR (while a flip is pending) a handle-entry confirmation that re-proves the operator before arming/disarming.
                    flow.line(&mut canvas, ctx.text, &tr(Msg::UnattendedTitle), hspan2, *theme::CONTACT_NAME_COLOUR, 600);
                    if let Some(target_on) = self.unattended_confirm {
                        flow.prose(
                            &mut canvas,
                            ctx.text,
                            &tr(if target_on {
                                Msg::UnattendedArmExplainer
                            } else {
                                Msg::UnattendedDisarmExplainer
                            }),
                            hspan2,
                            *theme::ERROR_TEXT_COLOUR,
                            600,
                        );
                        let tb_band = flow.band(hspan2 * 2.2);
                        if let Some(tb) = self.unattended_confirm_tb.as_mut() {
                            // Page-scale glyphs like every other box — without this it kept the constructor's placeholder 12.0 (field 2026-09-07: "itty bitty, half the size").
                            tb.set_font_size(hspan2, ctx.text);
                            tb.set_rect(tb_band.center_x(), tb_band.center_y(), tb_band.w * 0.9, tb_band.h * 0.85);
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
                        if self.unattended_confirm_failed {
                            flow.line(&mut canvas, ctx.text, &tr(Msg::UnattendedMismatch), hspan2, *theme::ERROR_TEXT_COLOUR, 600);
                        }
                        flow_pills(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, ctx.pressed_hit, hspan2 * 0.9, &[
                            (&tr(if target_on { Msg::Arm } else { Msg::Disarm }), self.unattended_confirm_base, true, Some(*theme::PILL_RED)),
                            (&tr(Msg::Cancel), self.unattended_confirm_base.wrapping_add(1), true, None),
                        ], "Open Sans");
                    } else {
                        let armed = self
                            .settings_unattended_check
                            .as_ref()
                            .map(|c| c.is_checked())
                            .unwrap_or(false);
                        if let Some(cb) = self.settings_unattended_check.as_mut() {
                            let label = cb.label().to_string();
                            flow_checkbox(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map, cb, &label, hspan2);
                        }
                        let (dc, dw) = if armed { (*theme::ERROR_TEXT_COLOUR, 600) } else { (*theme::LABEL_COLOUR, 400) };
                        flow.prose(&mut canvas, ctx.text, &tr(Msg::UnattendedWarning), hspan2 * 0.9, dc, dw);
                    }
                    flow.gap(hspan2);
                    measured_extent = Some((flow.used(), inset.h));
                }
                SettingsPage::Recovery => {
                    // RECOVERY, Flow rework (ticket queue 2026-09-02): everything wraps at the pane edge, the checkbox positions inline (the About-checkbox pattern), and the measured extent replaces the hand-counted 8-row estimate.
                    let inset = layout.content_inset();
                    let mut flow = Flow::new(inset, settings_content_scroll);
                    flow.line(&mut canvas, ctx.text, &tr(Msg::PageName(page)), tspan, *theme::CONTACT_NAME_COLOUR, 600);
                    flow.line(&mut canvas, ctx.text, &tr(Msg::Custodians), hspan2, *theme::CONTACT_NAME_COLOUR, 600);
                    flow.gap(hspan2 * 0.4);
                    if let Some(cb) = self.settings_custodian_check.as_mut() {
                        flow_checkbox(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map, cb, &tr(Msg::CustodianCheckbox), hspan2);
                    }
                    flow.gap(hspan2 * 0.6);
                    // Why ONE tick box and nothing else: you volunteer as a custodian, but nobody — including you — sees WHOSE recoveries you hold a share of, and an owner never learns which friends hold theirs. Not knowing who to lean on is the anti-collusion property: shares that can't be enumerated can't be gathered.
                    flow.prose(
                        &mut canvas,
                        ctx.text,
                        &tr(Msg::CustodianExplainer),
                        hspan2,
                        *theme::LABEL_COLOUR,
                        400,
                    );
                    // Identity-backup section COMMENTED OUT (Nick 2026-09-01) — custodians are the recovery story; a portable identity backup file re-creates the very honeypot the register model removed.
                    measured_extent = Some((flow.used(), inset.h));
                }
                SettingsPage::Appearance => {
                    let rows = layout
                        .content_scrolled(8, settings_content_scroll)
                        .split_v([1.0; 8]);
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[0],
                        &tr(Msg::PageName(page)),
                        tspan,
                        *theme::CONTACT_NAME_COLOUR,
                        600,
                    );
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[1],
                        &tr(Msg::Theme),
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
                        &tr(Msg::PartyColours),
                        hspan2,
                        *theme::LABEL_COLOUR,
                        400,
                    );
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[4],
                        &tr(Msg::ZoomTextSize),
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
                        &tr(Msg::ColourCalibration),
                        hspan2,
                        *theme::LABEL_COLOUR,
                        400,
                    );
                }
                SettingsPage::Notifications => {
                    // NOTIFICATIONS, Flow rework: one checkbox per flowed band (left-aligned, widths measured per label), everything wraps, measured extent replaces the row estimate. Presence stays COMMENTED OUT (Nick 2026-09-01) — field + dispatch compiled for a one-line restore.
                    let inset = layout.content_inset();
                    let mut flow = Flow::new(inset, settings_content_scroll);
                    flow.line(&mut canvas, ctx.text, &tr(Msg::NotificationsTitle), tspan, *theme::CONTACT_NAME_COLOUR, 600);
                    flow.gap(hspan2 * 0.4);
                    // The background/load-on-startup toggle MOVED to Security (Nick 2026-09-03) — it belongs beside the auto-attest arm it enables, not among the alert sounds.
                    let chime = tr(Msg::ChimeNewMessage);
                    let vib_msg = tr(Msg::VibrateNewMessage);
                    let ring_call = tr(Msg::RingIncomingCall);
                    let vib_call = tr(Msg::VibrateIncomingCall);
                    let wave_hold = tr(Msg::HoldWavesOnDevice);
                    let boxes: [(Option<&mut fluor::widgets::Checkbox>, &str); 5] = [
                        (self.settings_chime_check.as_mut(), &*chime),
                        (self.settings_vibrate_msg_check.as_mut(), &*vib_msg),
                        (self.settings_ring_call_check.as_mut(), &*ring_call),
                        (self.settings_vibrate_call_check.as_mut(), &*vib_call),
                        (self.settings_wave_hold_check.as_mut(), &*wave_hold),
                    ];
                    for (cb, label) in boxes {
                        let Some(cb) = cb else { continue };
                        flow_checkbox(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map, cb, label, hspan2);
                    }
                    flow.gap(hspan2 * 0.6);
                    flow.prose(
                        &mut canvas,
                        ctx.text,
                        &tr(Msg::PerContactOverride),
                        hspan2,
                        *theme::LABEL_COLOUR,
                        400,
                    );
                    measured_extent = Some((flow.used(), inset.h));
                }
                SettingsPage::Updates => {
                    // UPDATES on the Flow (2026-09-09): everything wraps at the pane edge — title, version, the auto-check box, the two channel pills as a wrapping pill row, then the status line or the download bar.
                    let inset = layout.content_inset();
                    let mut flow = Flow::new(inset, settings_content_scroll);
                    flow.line(&mut canvas, ctx.text, &tr(Msg::UpdatesTitle), tspan, *theme::CONTACT_NAME_COLOUR, 600);
                    flow.line(&mut canvas, ctx.text, &tr(Msg::PhotonVersion(&version_dozenal_glyphs())), hspan2, *theme::CONTACT_NAME_COLOUR, 400);
                    flow.gap(hspan2 * 0.5);
                    // ORDER (Nick 2026-09-09): version, then the GREEN release pill with what's new IN THAT RELEASE beneath it (the offered version's notes, fetched beside the manifest — not the running build's), then the AMBER dev pill with its beta invitation (and, on a dev build, what is coming). Each pill sizes to its label and wraps onto its own line when the pane is narrow.
                    let ours = crate::network::updates::our_version();
                    let pill_state = |kind: &str, avail_fill: (u32, u32), state: &ChannelCheck, busy: bool| -> (String, (u32, u32), bool) {
                        match state {
                            ChannelCheck::Idle | ChannelCheck::Checking => (tr(Msg::UpdateChecking(kind)).into_owned(), *theme::PILL_GREY, false),
                            ChannelCheck::Failed => (tr(Msg::UpdateUnavailable(kind)).into_owned(), *theme::PILL_GREY, false),
                            ChannelCheck::Ready(None) => (tr(Msg::UpdateNoBuild(kind)).into_owned(), *theme::PILL_GREY, false),
                            // Tuple equality IS the truth: patch 0 is the release marker and the version scheme guarantees a dev build never wears it.
                            ChannelCheck::Ready(Some(row)) if row.version == ours => {
                                let ver = dozenal_version_tuple(row.version);
                                (tr(Msg::UpdateAlreadyOn { kind, ver: &ver }).into_owned(), *theme::PILL_GREY, false)
                            }
                            ChannelCheck::Ready(Some(row)) => {
                                let ver = dozenal_version_tuple(row.version);
                                (tr(Msg::UpdateGet { kind, ver: &ver }).into_owned(), avail_fill, !busy)
                            }
                        }
                    };
                    // Status: the download bar while bytes stream (label flips "Downloading" → "Updating…" at the end), else the last APPLY outcome — drawn RIGHT UNDER the channel pill that started it (Nick 2026-09-10), at the page bottom only for an auto-update nobody pressed.
                    let update_progress = self.update_progress;
                    let update_status = self.update_status.clone();
                    let update_active_dev = self.update_active_dev;
                    let draw_update_status = |flow: &mut Flow, canvas: &mut Canvas, text: &mut fluor::text::TextRenderer| {
                        if let Some((done, total)) = update_progress {
                            let finishing = total > 0 && done >= total;
                            let label = if finishing {
                                tr(Msg::Updating)
                            } else if total > 0 {
                                tr(Msg::Downloading)
                            } else {
                                tr(Msg::DownloadingSize(&crate::unit_size(done as u64, crate::SizeUnit::MiB)))
                            };
                            flow.gap(hspan2 * 0.3);
                            flow.line(canvas, text, &label, hspan2, *theme::CONTACT_NAME_COLOUR, 500);
                            let bar = flow.band(hspan2 * 0.6);
                            // The bar: proportional fill THEN full-width track. fluor is under-blend (FIRST paint wins), so the fill MUST be painted before the track.
                            let bar_w = bar.w as isize;
                            let bar_y = bar.y as isize;
                            let bar_h = (bar.h * 0.6) as isize;
                            if total > 0 {
                                let fill_w = (bar.w as f64 * (done as f64 / total as f64)) as isize;
                                paint::fill_rect(canvas, bar.x as isize, bar_y, fill_w.clamp(0, bar_w), bar_h, *theme::PROGRESS_FILL, None, None);
                            }
                            paint::fill_rect(canvas, bar.x as isize, bar_y, bar_w, bar_h, *theme::PROGRESS_TRACK, None, None);
                        } else if let Some(status) = &update_status {
                            flow.gap(hspan2 * 0.3);
                            flow.line(canvas, text, status, hspan2, *theme::CONTACT_NAME_COLOUR, 500);
                        }
                    };
                    let (rl, rf, re) = pill_state("release", *theme::PILL_GREEN, &self.update_release, self.update_busy);
                    flow_pills(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, ctx.pressed_hit, hspan2 * 0.9, &[
                        (&rl, btn_base.wrapping_add(1), re, Some(rf)),
                    ], "Oxanium");
                    if update_active_dev == Some(false) {
                        draw_update_status(&mut flow, &mut canvas, ctx.text);
                    }
                    {
                        // The release the green pill names: its minor is the notes section. Fetched notes first (they describe releases newer than this build); the compiled-in copy covers the case where the offered release IS this build (or the fetch hasn't landed).
                        let offered_minor: Option<usize> = match &self.update_release {
                            ChannelCheck::Ready(Some(row)) => Some(row.version.1),
                            _ => None,
                        };
                        if let Some(minor) = offered_minor {
                            let sect = format!("v{minor}");
                            let mut items = self
                                .update_notes
                                .as_deref()
                                .map(|t| crate::ui::release_notes::section_of(t, &sect))
                                .unwrap_or_default();
                            if items.is_empty() {
                                items = crate::ui::release_notes::section(&sect);
                            }
                            if !items.is_empty() {
                                flow.gap(hspan2 * 0.3);
                                flow.line(&mut canvas, ctx.text, &tr(Msg::WhatsNew(&crate::fmt_num(minor as u32))), hspan2, *theme::CONTACT_NAME_COLOUR, 600);
                                for item in &items {
                                    flow.prose(&mut canvas, ctx.text, &format!("• {item}"), hspan2 * 0.9, *theme::LABEL_COLOUR, 400);
                                }
                            }
                        }
                    }
                    flow.gap(hspan2 * 0.6);
                    let (dl, df, de) = pill_state("dev", *theme::PILL_AMBER, &self.update_dev, self.update_busy);
                    flow_pills(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, ctx.pressed_hit, hspan2 * 0.9, &[
                        (&dl, btn_base.wrapping_add(2), de, Some(df)),
                    ], "Oxanium");
                    if update_active_dev == Some(true) {
                        draw_update_status(&mut flow, &mut canvas, ctx.text);
                    }
                    flow.gap(hspan2 * 0.3);
                    flow.prose(&mut canvas, ctx.text, &tr(Msg::DevChannelHint), hspan2 * 0.9, *theme::LABEL_COLOUR, 400);
                    if dev_patch() > 0 {
                        let upcoming = crate::ui::release_notes::section("Upcoming");
                        if !upcoming.is_empty() {
                            flow.gap(hspan2 * 0.3);
                            flow.line(&mut canvas, ctx.text, &tr(Msg::UpcomingChanges), hspan2, *theme::CONTACT_NAME_COLOUR, 600);
                            for item in &upcoming {
                                flow.prose(&mut canvas, ctx.text, &format!("• {item}"), hspan2 * 0.9, *theme::LABEL_COLOUR, 400);
                            }
                        }
                    }
                    flow.gap(hspan2 * 0.6);
                    if let Some(cb) = self.settings_autoupdate_check.as_mut() {
                        let label = cb.label().to_string();
                        flow_checkbox(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map, cb, &label, hspan2);
                    }
                    flow.gap(hspan2 * 0.4);
                    if update_active_dev.is_none() {
                        draw_update_status(&mut flow, &mut canvas, ctx.text);
                    }
                    measured_extent = Some((flow.used(), inset.h));
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
                        &tr(Msg::LogTitle),
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
                        &tr(Msg::DiagBack),
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
                        tr(Msg::DiagRecordInspect { ts: &ts, lines: lines.len() }).into_owned()
                    } else if self.diag_log_rx.is_some() {
                        tr(Msg::DiagDecoding).into_owned()
                    } else if self.diag_log_rows.is_empty() {
                        tr(Msg::LogEmpty).into_owned()
                    } else {
                        let size = crate::unit_size(crate::log_size_bytes() as u64, crate::SizeUnit::KiB);
                        let mut m = tr(Msg::DiagMeta {
                            count: self.diag_log_rows.len(),
                            size: &size,
                        })
                        .into_owned();
                        if self.diag_log_rows.len() >= DIAG_LOG_MAX_ROWS {
                            m.push_str(&tr(Msg::DiagTrimmed));
                        }
                        m
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
                            // Wall-clock coordinate for correlating with photonlog and adb, not a quantity: arabic clock time in every base.
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
                    // DIAGNOSTICS on the Flow (2026-09-09): the log line and the note prompt wrap at the pane edge like every other page; the four actions are a wrapping pill row; the note box and the hard-logs box sit inline.
                    let inset = layout.content_inset();
                    let mut flow = Flow::new(inset, settings_content_scroll);
                    flow.line(&mut canvas, ctx.text, &tr(Msg::PageName(page)), tspan, *theme::CONTACT_NAME_COLOUR, 600);
                    // The live size, not just the cap: "how much have I got to send" is the question this page exists to answer.
                    let used = crate::log_size_bytes();
                    let cap = crate::LOG_CAP_BYTES;
                    let pct = if cap > 0 { used * 100 / cap } else { 0 };
                    flow.prose(&mut canvas, ctx.text, &tr(Msg::DiagInfo { used: &human_bytes(used), cap: &human_bytes(cap), pct: pct as u64 }), hspan2, *theme::LABEL_COLOUR, 400);
                    // The last wave's link (Nick 2026-09-10): the round trip as a dozenal-metric FREQUENCY (Zila = 1 Hz, Zilor = 2 Hz, Ter = 4 Hz, each digit a doubling), the loss ring, the buffer target.
                    {
                        let rtt = crate::call::LAST_LINK_RTT_MS.load(std::sync::atomic::Ordering::Relaxed);
                        let line = if rtt == 0 {
                            tr(Msg::NoWaveYet).into_owned()
                        } else {
                            let loss = crate::fmt_num(crate::call::LAST_LINK_LOSS.load(std::sync::atomic::Ordering::Relaxed));
                            let buffer = crate::fmt_num(crate::call::LAST_LINK_TARGET.load(std::sync::atomic::Ordering::Relaxed));
                            tr(Msg::LastWave { link: &crate::link_freq_label(rtt), loss: &loss, buffer: &buffer }).into_owned()
                        };
                        flow.line(&mut canvas, ctx.text, &line, hspan2 * 0.9, *theme::LABEL_COLOUR, 400);
                    }
                    flow.gap(hspan2 * 0.4);
                    // Submit greys while an upload is in flight or the log hasn't grown past the last successful submit — a resend then would be a byte-identical dup.
                    let submit_disabled = self.log_submit_inflight || self.log_submitted_len == Some(crate::log_size_bytes());
                    flow_pills(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, ctx.pressed_hit, hspan2 * 0.9, &[
                        (&tr(Msg::DiagClear), btn_base.wrapping_add(0), true, None),
                        (&tr(Msg::DiagSnapshot), btn_base.wrapping_add(1), true, None),
                        (&tr(Msg::DiagSubmit), btn_base.wrapping_add(2), !submit_disabled, None),
                        (&tr(Msg::DiagView), btn_base.wrapping_add(3), true, None),
                    ], "Open Sans");
                    flow.gap(hspan2 * 0.6);
                    flow.prose(&mut canvas, ctx.text, &tr(Msg::OptionalNote), hspan2, *theme::LABEL_COLOUR, 400);
                    let tb_band = flow.band(hspan2 * 2.2);
                    if let Some(tb) = self.settings_note_textbox.as_mut() {
                        tb.set_font_size(hspan2, ctx.text);
                        tb.set_rect(tb_band.center_x(), tb_band.center_y(), tb_band.w * 0.95, tb_band.h * 0.85);
                        let id = tb.hit_id();
                        tb.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, Some(&mut chrome.hit_test_map), id);
                    }
                    flow.gap(hspan2 * 0.6);
                    if let Some(cb) = self.settings_hardlogs_check.as_mut() {
                        let label = cb.label().to_string();
                        flow_checkbox(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map, cb, &label, hspan2);
                    }
                    flow.gap(hspan2);
                    measured_extent = Some((flow.used(), inset.h));
                }
                SettingsPage::Dozenal => {
                    // THE BASE PAGE (Nick 2026-09-09: "a distinct dozenal tab that reads Dozenal or Arabic depending on their choice"): the fleet-wide toggle, why (or the tin-foil answer in arabic mode), the digit cheat sheet with the custodian riddle behind it, and the DMS time-ago legend. A centred card like About, manual cursor, measured extent.
                    let inset = layout.content_inset();
                    let line_h = layout.content_line_h();
                    let cx = inset.x + inset.w * 0.5;
                    let wrap_w = inset.w - line_h;
                    let page_clip = Some(fluor::paint::Clip::new(
                        inset.x.max(0.0) as usize,
                        inset.y.max(0.0) as usize,
                        (inset.x + inset.w).max(0.0) as usize,
                        (inset.y + inset.h).max(0.0) as usize,
                    ));
                    let prose_style = TextStyle::new(hspan2 * 0.75, *theme::LABEL_COLOUR).weight(400).font("Oxanium");
                    let head_style = TextStyle::new(hspan2, *theme::CONTACT_NAME_COLOUR).weight(600).font("Oxanium");
                    let cell_style = TextStyle::new(hspan2 * 0.85, *theme::LABEL_COLOUR).weight(400).font("Oxanium");
                    let mut y = inset.y - settings_content_scroll;
                    ctx.text.draw_text_center(&mut canvas, &tr(Msg::PageName(page)), cx, y + line_h * 0.5, &head_style, page_clip, None);
                    y += line_h * 1.4;
                    // Fleet-wide base pills (display.base — linked, so a preference follows the identity): dozenal, hexadecimal, arabic, the chosen one filled. Dozenal and hex fill green; arabic fills the shame red — the disapproval rides the pill, no scold line needed.
                    let base = crate::num_base();
                    {
                        // Flow-aware pills (the Security-page helper): each sizes to its label and they wrap onto further lines when the pane is narrow or the zoom is big (Nick 2026-09-09: "base choice buttons don't wrap upon scale").
                        let fill_for = |this: crate::NumBase| -> Option<(u32, u32)> {
                            if this != base {
                                None
                            } else if this == crate::NumBase::Arabic {
                                Some((*theme::DOZENAL_SCOLD_BOX, *theme::DOZENAL_SCOLD_BOX))
                            } else {
                                Some(*theme::PILL_GREEN)
                            }
                        };
                        let labels = [tr(Msg::Dozenal), tr(Msg::Hexadecimal), tr(Msg::Arabic)];
                        let pills = [
                            (labels[0].as_ref(), btn_base, true, fill_for(crate::NumBase::Dozenal)),
                            (labels[1].as_ref(), btn_base.wrapping_add(1), true, fill_for(crate::NumBase::Hex)),
                            (labels[2].as_ref(), btn_base.wrapping_add(2), true, fill_for(crate::NumBase::Arabic)),
                        ];
                        // A local Flow anchored at the current cursor (its inset.y is pre-scrolled so the flow's y lands exactly at `y`).
                        let mut flow = Flow::new(fluor::region::Region::new(inset.x, y + settings_content_scroll, inset.w, inset.h), settings_content_scroll);
                        flow_pills(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, ctx.pressed_hit, hspan2 * 0.9, &pills, "Oxanium");
                        y += flow.used();
                    }
                    y += line_h * 0.4;
                    // PER-PAGE UNITS (Nick 2026-09-10): the dozenal page speaks dozenal, the hex page hex, whatever the base setting — each page's own examples never change base. So the dozenal legends use dozenal_glyphs directly, the hex legends hex_linear directly, and only the cheat sheet (every base, one column each) is shared.
                    match base {
                        crate::NumBase::Dozenal => {
                            // The logarithm note EARLY: time and size on this base are Dozenal Metric Scaling.
                            for line in tr(Msg::BaseLogNote).lines() {
                                y = centered_wrapped(&mut canvas, ctx.text, cx, wrap_w, y, line, &prose_style, line_h * 0.8, page_clip);
                                y += line_h * 0.3;
                            }
                            y += line_h * 0.4;
                            ctx.text.draw_text_center(&mut canvas, &tr(Msg::WhyDozenal), cx, y + line_h * 0.5, &TextStyle::new(hspan2, *theme::SEARCH_FOUND_COLOUR).weight(600).font("Oxanium"), page_clip, None);
                            y += line_h;
                            for line in tr(Msg::WhyDozenalProse).lines() {
                                y = centered_wrapped(&mut canvas, ctx.text, cx, wrap_w, y, line, &prose_style, line_h * 0.8, page_clip);
                                y += line_h * 0.3;
                            }
                            // THE SCALING, EXPLAINED (Nick 2026-09-11): what one number for how much means, why doublings, the three forms; then what "one" is on every scale.
                            for (head, prose) in [(Msg::DmsScaleHead, Msg::DmsScaleProse), (Msg::DmsUnitsHead, Msg::DmsUnitsProse)] {
                                y += line_h * 0.4;
                                ctx.text.draw_text_center(&mut canvas, &tr(head), cx, y + line_h * 0.5, &head_style, page_clip, None);
                                y += line_h;
                                for line in tr(prose).lines() {
                                    y = centered_wrapped(&mut canvas, ctx.text, cx, wrap_w, y, line, &prose_style, line_h * 0.8, page_clip);
                                    y += line_h * 0.3;
                                }
                            }
                        }
                        crate::NumBase::Hex => {
                            // The coder's page: no dozenal sermon here.
                            ctx.text.draw_text_center(&mut canvas, &tr(Msg::WhyHex), cx, y + line_h * 0.5, &TextStyle::new(hspan2, *theme::SEARCH_FOUND_COLOUR).weight(600).font("Oxanium"), page_clip, None);
                            y += line_h;
                            for line in tr(Msg::WhyHexProse).lines() {
                                y = centered_wrapped(&mut canvas, ctx.text, cx, wrap_w, y, line, &prose_style, line_h * 0.8, page_clip);
                                y += line_h * 0.3;
                            }
                        }
                        crate::NumBase::Arabic => {
                            ctx.text.draw_text_center(&mut canvas, &tr(Msg::WhyYouDozenal), cx, y + line_h * 0.5, &TextStyle::new(hspan2, *theme::SEARCH_FOUND_COLOUR).weight(600).font("Oxanium"), page_clip, None);
                            y += line_h;
                            for line in tr(Msg::WhyYouDozenalProse).lines() {
                                y = centered_wrapped(&mut canvas, ctx.text, cx, wrap_w, y, line, &prose_style, line_h * 0.8, page_clip);
                                y += line_h * 0.3;
                            }
                        }
                    }
                    y += line_h * 0.6;
                    // THE DIGIT CHEAT SHEET, every base (Nick 2026-09-10): three columns — dozenal, hexadecimal, arabic — each counting 0 to F in its own numerals, the dozenal column with its digit names. The same table whatever base is chosen.
                    let index_top = y;
                    ctx.text.draw_text_center(&mut canvas, &tr(Msg::DigitsHead), cx, y + line_h * 0.5, &head_style, page_clip, None);
                    y += line_h;
                    let cols = [inset.x + inset.w * 0.2, inset.x + inset.w * 0.5, inset.x + inset.w * 0.8];
                    let heads = [tr(Msg::Dozenal), tr(Msg::Hexadecimal), tr(Msg::Arabic)];
                    for (k, h) in heads.iter().enumerate() {
                        ctx.text.draw_text_center(&mut canvas, h, cols[k], y + line_h * 0.5, &TextStyle::new(hspan2 * 0.8, *theme::CONTACT_NAME_COLOUR).weight(600).font("Oxanium"), page_clip, None);
                    }
                    y += line_h;
                    for n in 0..16u32 {
                        let doz = format!("{}  {}", crate::dozenal_glyphs(n), crate::dozenal_spell(n));
                        ctx.text.draw_text_center(&mut canvas, &doz, cols[0], y + line_h * 0.5, &cell_style, page_clip, None);
                        ctx.text.draw_text_center(&mut canvas, &crate::hex_glyphs(n), cols[1], y + line_h * 0.5, &cell_style, page_clip, None);
                        ctx.text.draw_text_center(&mut canvas, &n.to_string(), cols[2], y + line_h * 0.5, &cell_style, page_clip, None);
                        y += line_h * 0.9;
                    }
                    restamp_hit_rect(&mut chrome.hit_test_map, buf_w, buf_h, inset.x as isize, index_top as isize, (inset.x + inset.w) as isize, y as isize, btn_base.wrapping_add(5));
                    if self.about_riddle_revealed && base == crate::NumBase::Dozenal {
                        y += line_h * 0.4;
                        ctx.text.draw_text_center(&mut canvas, &crate::dozenal_glyphs(42), cx, y + line_h * 0.5, &TextStyle::new(hspan2, *theme::SEARCH_FOUND_COLOUR).weight(400).font("Oxanium"), page_clip, None);
                        y += line_h;
                        let s = tr(Msg::AboutRiddle);
                        for line in s.lines() {
                            ctx.text.draw_text_center(&mut canvas, line, cx, y + line_h * 0.4, &prose_style, page_clip, None);
                            y += line_h * 0.8;
                        }
                    }
                    y += line_h * 0.6;
                    match base {
                        crate::NumBase::Dozenal => {
                            // DMS — the time-ago legend: the age is the bit length of the seconds count, so each row is a doubling; dozenal glyphs + names, always.
                            ctx.text.draw_text_center(&mut canvas, &tr(Msg::DmsHead), cx, y + line_h * 0.5, &head_style, page_clip, None);
                            y += line_h;
                            for line in tr(Msg::DmsIntro).lines() {
                                y = centered_wrapped(&mut canvas, ctx.text, cx, wrap_w, y, line, &prose_style, line_h * 0.8, page_clip);
                                y += line_h * 0.3;
                            }
                            y += line_h * 0.3;
                            // Zero has no logarithm: the first row is the word.
                            ctx.text.draw_text_center(&mut canvas, &tr(Msg::DmsNow), cx, y + line_h * 0.5, &cell_style, page_clip, None);
                            y += line_h * 0.9;
                            for bits in [0u32, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 16, 21, 24] {
                                let row = format!("{}  {}  {}", crate::dozenal_glyphs(bits), crate::dozenal_spell(bits), tr(Msg::DmsReading(bits)));
                                ctx.text.draw_text_center(&mut canvas, &row, cx, y + line_h * 0.5, &cell_style, page_clip, None);
                                y += line_h * 0.9;
                            }
                            y += line_h * 0.6;
                            ctx.text.draw_text_center(&mut canvas, &tr(Msg::DmsSizeHead), cx, y + line_h * 0.5, &head_style, page_clip, None);
                            y += line_h;
                            for line in tr(Msg::DmsSizeIntro).lines() {
                                y = centered_wrapped(&mut canvas, ctx.text, cx, wrap_w, y, line, &prose_style, line_h * 0.8, page_clip);
                                y += line_h * 0.3;
                            }
                            y += line_h * 0.3;
                            ctx.text.draw_text_center(&mut canvas, &tr(Msg::DmsEmpty), cx, y + line_h * 0.5, &cell_style, page_clip, None);
                            y += line_h * 0.9;
                            for bits in [0u32, 3, 7, 10, 13, 16, 19, 23, 26, 29, 33, 43] {
                                let row = format!("{}  {}  {}", crate::dozenal_glyphs(bits), crate::dozenal_spell(bits), tr(Msg::DmsSizeReading(bits)));
                                ctx.text.draw_text_center(&mut canvas, &row, cx, y + line_h * 0.5, &cell_style, page_clip, None);
                                y += line_h * 0.9;
                            }
                            // LENGTH (Nick 2026-09-11): doublings of the hydrogen line's wavelength, a minus for halvings; the same shape as the time and size legends.
                            y += line_h * 0.6;
                            ctx.text.draw_text_center(&mut canvas, &tr(Msg::DmsLengthHead), cx, y + line_h * 0.5, &head_style, page_clip, None);
                            y += line_h;
                            for line in tr(Msg::DmsLengthIntro).lines() {
                                y = centered_wrapped(&mut canvas, ctx.text, cx, wrap_w, y, line, &prose_style, line_h * 0.8, page_clip);
                                y += line_h * 0.3;
                            }
                            y += line_h * 0.3;
                            for k in [-11i32, -6, -4, -1, 0, 2, 3, 7, 8, 12, 15, 24, 30, 39, 55, 91] {
                                let row = format!("{}  {}  {}", crate::dms_doublings_glyphs(k), crate::dms_doublings_spell(k), tr(Msg::DmsLengthReading(k)));
                                ctx.text.draw_text_center(&mut canvas, &row, cx, y + line_h * 0.5, &cell_style, page_clip, None);
                                y += line_h * 0.9;
                            }
                        }
                        crate::NumBase::Hex => {
                            // LINEAR legends: the seconds count and the bit count in hex, no scaling.
                            ctx.text.draw_text_center(&mut canvas, &tr(Msg::DmsHead), cx, y + line_h * 0.5, &head_style, page_clip, None);
                            y += line_h;
                            y = centered_wrapped(&mut canvas, ctx.text, cx, wrap_w, y, &tr(Msg::HexTimeIntro), &prose_style, line_h * 0.8, page_clip);
                            y += line_h * 0.6;
                            for secs in [1u64, 60, 3600, 86400, 2592000, 31536000] {
                                let row = format!("{}  {}", crate::hex_linear(secs), tr(Msg::HexTimeReading(secs)));
                                ctx.text.draw_text_center(&mut canvas, &row, cx, y + line_h * 0.5, &cell_style, page_clip, None);
                                y += line_h * 0.9;
                            }
                            y += line_h * 0.6;
                            ctx.text.draw_text_center(&mut canvas, &tr(Msg::DmsSizeHead), cx, y + line_h * 0.5, &head_style, page_clip, None);
                            y += line_h;
                            y = centered_wrapped(&mut canvas, ctx.text, cx, wrap_w, y, &tr(Msg::HexSizeIntro), &prose_style, line_h * 0.8, page_clip);
                            y += line_h * 0.6;
                            for bits in [8u64, 8192, 8388608, 8589934592] {
                                let row = format!("{}  {}", crate::hex_linear(bits), tr(Msg::HexSizeReading(bits)));
                                ctx.text.draw_text_center(&mut canvas, &row, cx, y + line_h * 0.5, &cell_style, page_clip, None);
                                y += line_h * 0.9;
                            }
                            y += line_h * 0.6;
                            y = centered_wrapped(&mut canvas, ctx.text, cx, wrap_w, y, &tr(Msg::HexLengthNote), &prose_style, line_h * 0.8, page_clip);
                        }
                        crate::NumBase::Arabic => {}
                    }
                    measured_extent = Some((y + settings_content_scroll - inset.y + line_h, inset.h));
                    let _ = tspan;
                }
                SettingsPage::About => {
                    // An About CARD, not a settings list: the Photon wordmark over its chromatic wave up top, then the two headline properties (killswitch-ready, passless), then the version — tap it to reveal both the spelled-out form AND the dozenal cheat sheet. No feedback line — photon is owned by everyone. All centred under the logo; a manual vertical cursor (elements are variable-height, not equal rows).
                    let inset = layout.content_inset();
                    let line_h = layout.content_line_h();
                    let cx = inset.x + inset.w * 0.5;
                    // Every About text line runs thru centered_wrapped: authored stanzas keep their line breaks but wrap at THIS width instead of clipping at the pane edge when zoomed big.
                    let wrap_w = inset.w - line_h;
                    // Pane clip for the CONTENT-pass text — the bg-pass slab crops at the pane top, and unclipped card text scrolling over the title band beside a trimmed logo read as a layer glitch (Nick 2026-09-02).
                    let about_clip = Some(fluor::paint::Clip::new(
                        inset.x.max(0.0) as usize,
                        inset.y.max(0.0) as usize,
                        (inset.x + inset.w).max(0.0) as usize,
                        (inset.y + inset.h).max(0.0) as usize,
                    ));
                    let mut y = inset.y - settings_content_scroll;
                    // The wave + wordmark now paint in the BG pass (see about_slab in the bg closure): the attest screen's proportions at pane width, never zoom-scaled, scrolled with the card. The card just advances past the slab.
                    let (_, slab_h) = about_slab(buf_w, buf_h, inset.w);
                    y += slab_h + line_h * 0.4;
                    // The two headline properties — the whole pitch in two words each.
                    ctx.text.draw_text_center(
                        &mut canvas,
                        &tr(Msg::AboutKillswitchReady),
                        cx,
                        y + line_h * 0.5,
                        &TextStyle::new(hspan2, *theme::SEARCH_FOUND_COLOUR)
                            .weight(600)
                            .font("Oxanium"),
                        about_clip,
                        None,
                    );
                    y += line_h;
                    ctx.text.draw_text_center(
                        &mut canvas,
                        &tr(Msg::AboutPasslessHead),
                        cx,
                        y + line_h * 0.5,
                        &TextStyle::new(hspan2, *theme::SEARCH_FOUND_COLOUR)
                            .weight(600)
                            .font("Oxanium"),
                        about_clip,
                        None,
                    );
                    y += line_h;
                    // Clickable weblink under the passless headline (slot 4 — opens https://passless.org/ in the system browser).
                    let lead_style = TextStyle::new(hspan2 * 0.8, *theme::LABEL_COLOUR)
                        .weight(400)
                        .font("Oxanium");
                    y = centered_wrapped(&mut canvas, ctx.text, cx, wrap_w, y, &tr(Msg::AboutPasslessLead), &lead_style, line_h * 0.8, about_clip);
                    // The link itself: primary VSF blue, bigger, BOLD + hand cursor on hover (cursor_for), the openey thing on click (slot 4). Hit rect fits the MEASURED text exactly — the old full-width band is why the hitmap didn't line up.
                    let link_hovered = stub_hover() == btn_base.wrapping_add(4);
                    let link_style = TextStyle::new(hspan2 * 1.15, *theme::LINK_COLOUR)
                        .weight(if link_hovered { 700 } else { 500 })
                        .font("Oxanium");
                    let link_w = ctx.text.measure_text("passless.org", &link_style);
                    ctx.text.draw_text_center(
                        &mut canvas,
                        "passless.org",
                        cx,
                        y + line_h * 0.6,
                        &link_style,
                        about_clip,
                        None,
                    );
                    restamp_hit_rect(
                        &mut chrome.hit_test_map,
                        buf_w,
                        buf_h,
                        (cx - link_w * 0.5 - hspan2 * 0.4) as isize,
                        y.max(inset.y) as isize,
                        (cx + link_w * 0.5 + hspan2 * 0.4) as isize,
                        ((y + line_h * 1.2).min(inset.y + inset.h)) as isize,
                        btn_base.wrapping_add(4),
                    );
                    y += line_h * 1.3;
                    y += line_h * 1.4;
                    // The no-servers pitch — what passless actually buys you. Deletion parity with speech: with genuinely two ends and no third copy, mutual deletion is total and silent while unilateral deletion is cryptographically loud (the chain breaks and the other side sees it) — tamper-evidence and consensual ephemerality aren't in tension.
                    let prose_style = TextStyle::new(hspan2 * 0.75, *theme::LABEL_COLOUR)
                        .weight(400)
                        .font("Oxanium");
                    let s = tr(Msg::AboutPasslessProse);
                    for line in s.lines() {
                        y = centered_wrapped(&mut canvas, ctx.text, cx, wrap_w, y, line, &prose_style, line_h * 0.8, about_clip);
                        y += line_h * 0.3;
                    }
                    y += line_h * 0.3;
                    // CONSENT — the third pillar: every lifecycle edge is bilateral (mutual-consent clutch, two-signature add/depart, no expulsion), so nothing happens to an identity without its own key signing.
                    ctx.text.draw_text_center(
                        &mut canvas,
                        &tr(Msg::AboutConsentHead),
                        cx,
                        y + line_h * 0.5,
                        &TextStyle::new(hspan2, *theme::SEARCH_FOUND_COLOUR)
                            .weight(600)
                            .font("Oxanium"),
                        about_clip,
                        None,
                    );
                    y += line_h;
                    let s = tr(Msg::AboutConsentProse);
                    for line in s.lines() {
                        y = centered_wrapped(&mut canvas, ctx.text, cx, wrap_w, y, line, &prose_style, line_h * 0.8, about_clip);
                        y += line_h * 0.3;
                    }
                    y += line_h * 0.6;
                    // TOKEN — the recovery story, mirroring the Recovery page's anti-collusion prose so the pitch and the tick box tell one tale.
                    ctx.text.draw_text_center(
                        &mut canvas,
                        &tr(Msg::AboutTokenHead),
                        cx,
                        y + line_h * 0.5,
                        &TextStyle::new(hspan2, *theme::SEARCH_FOUND_COLOUR)
                            .weight(600)
                            .font("Oxanium"),
                        about_clip,
                        None,
                    );
                    y += line_h;
                    let s = tr(Msg::AboutTokenProse);
                    for line in s.lines() {
                        y = centered_wrapped(&mut canvas, ctx.text, cx, wrap_w, y, line, &prose_style, line_h * 0.8, about_clip);
                        y += line_h * 0.3;
                    }
                    y += line_h * 0.6;
                    // WAVE + BEAM — why photon has no "calls": waves/beams are honestly recorded and never touch a third party (Nick 2026-09-03).
                    ctx.text.draw_text_center(
                        &mut canvas,
                        &tr(Msg::AboutWaveBeamHead),
                        cx,
                        y + line_h * 0.5,
                        &TextStyle::new(hspan2, *theme::SEARCH_FOUND_COLOUR)
                            .weight(600)
                            .font("Oxanium"),
                        about_clip,
                        None,
                    );
                    y += line_h;
                    let s = tr(Msg::AboutWaveBeamProse);
                    for line in s.lines() {
                        y = centered_wrapped(&mut canvas, ctx.text, cx, wrap_w, y, line, &prose_style, line_h * 0.8, about_clip);
                        y += line_h * 0.3;
                    }
                    y += line_h * 0.6;
                    // Version — dozenal glyphs (weight 400 → the Oxanium +glyphs face draws the reserved control bytes as dozenal digits), NEVER arabic. Tap toggles the reveal (spelled form + cheat sheet). Whole row is the tap target (btn_base + 3).
                    let ver_glyphs = version_dozenal_glyphs();
                    let ver = tr(Msg::AboutVersion(&ver_glyphs));
                    ctx.text.draw_text_center(
                        &mut canvas,
                        &ver,
                        cx,
                        y + line_h * 0.5,
                        &TextStyle::new(hspan2, *theme::CONTACT_NAME_COLOUR)
                            .weight(400)
                            .font("Oxanium"),
                        about_clip,
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
                    // The standing clock correction (Nick 2026-09-03): photon orders messages on consensus-corrected time, so the curious should be able to see how far their own clock sits from it. Purely informational — the system clock is never touched, and a deliberately-fast clock is a preference, not a fault.
                    let clock_line = match crate::network::time_base::offset_now() {
                        Some((offset_osc, conf_osc)) => {
                            let ms = |o: i64| o * 1000 / crate::OSC_PER_SEC;
                            let (o, c) = (ms(offset_osc), ms(conf_osc));
                            let sign = if o < 0 { "-" } else { "+" };
                            // Hex is seconds with a hex fraction; the other bases keep linear milliseconds with the unit.
                            let (off, conf) = if crate::hex_ui() {
                                (format!("{sign}{} s", crate::hex_seconds_ms(o.unsigned_abs() as u64)), format!("{} s", crate::hex_seconds_ms(c.unsigned_abs() as u64)))
                            } else {
                                (format!("{sign}{} ms", crate::fmt_num(o.unsigned_abs() as u32)), format!("{} ms", crate::fmt_num(c.unsigned_abs() as u32)))
                            };
                            tr(Msg::AboutClockOffset { ms: &off, conf: &conf })
                            .into_owned()
                        }
                        None => tr(Msg::AboutClockUnknown).into_owned(),
                    };
                    ctx.text.draw_text_center(
                        &mut canvas,
                        &clock_line,
                        cx,
                        y + line_h * 0.5,
                        &TextStyle::new(hspan2 * 0.8, *theme::LABEL_COLOUR)
                            .weight(400)
                            .font("Oxanium"),
                        about_clip,
                        None,
                    );
                    y += line_h;
                    // Version, spelled out (voca words) on tap. The base toggle, the digit cheat sheet, and the time-ago legend live on the Dozenal page now (Nick 2026-09-09) — About keeps the pitch and the version.
                    if self.about_version_spelled {
                        let main = crate::dozenal_spell(deploy_version());
                        let patch = (dev_patch() > 0).then(|| crate::dozenal_spell(dev_patch()));
                        let spelled = tr(Msg::AboutVersionSpelled { main: &main, patch: patch.as_deref() });
                        ctx.text.draw_text_center(
                            &mut canvas,
                            &spelled,
                            cx,
                            y + line_h * 0.5,
                            &TextStyle::new(hspan2 * 0.85, *theme::LABEL_COLOUR)
                                .weight(400)
                                .font("Oxanium"),
                            about_clip,
                            None,
                        );
                        y += line_h * 1.4;
                    }
                    // MEASURED extent (Flow doctrine): the card's true height from the final cursor — retires the hand-counted row arithmetic next frame.
                    measured_extent = Some((y + settings_content_scroll - inset.y + line_h, inset.h));
                    let _ = tspan;
                }
            }
        }

        } // end !call_fullscreen — per-screen bodies skipped while the ring panel owns the surface
        _rt.mark("body");

        // Apply the frame's MEASURED extent (Flow pages) — next frame's clamp reads it.
        if let Some((content_h, pane_h)) = measured_extent {
            self.settings_content_extent = (content_h - pane_h).max(0.0);
        }

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
                &tr(Msg::JoinerSelected),
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
                &tr(Msg::JoinerConfirmOther),
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
            // Full-screen ring panel is MODAL: wipe the whole map first so the screen's own widgets (stamped above) can't be tapped thru the wash — then the two call buttons are the only live targets.
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
            // Full-screen-only controls, phase-gated (the modal wipe above cleared the map, so these must re-assert): Active in-call = speaker / +handle / ‹ contact; Ended = play preview.
            if call_fullscreen {
                match call_overlay.as_ref().map(|t| t.0) {
                    Some(crate::call::CallPhase::Active) => {
                        // call_speaker_btn stays out of the stamp list while the toggle is parked — an unstamped rect is stale from whenever it last rendered.
                        for b in [
                            self.call_addhandle_btn.as_ref(),
                            self.call_back_btn.as_ref(),
                        ]
                        .into_iter()
                        .flatten()
                        {
                            b.stamp_hit_into(&mut chrome.hit_test_map, buf_w, buf_h, b.hit_id());
                        }
                    }
                    _ => {}
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

impl PhotonApp {
    /// The STANDING bands along the bottom of the contact screen and the attest screen (Nick 2026-09-09: stack every alert, keep the update one persistent). Bottom-most first. Toasts are a different thing: transient, keystroke-cleared, drawn in the hint slot. These never clear on interaction — each disappears only when its condition does.
    pub(super) fn standing_bands(&self) -> Vec<(String, u32)> {
        let mut v: Vec<(String, u32)> = Vec::new();
        // Storage, two severities (2026-09-03): RED lost data, amber degraded.
        if self.vault_data_lost {
            v.push((tr(Msg::StorageDataLost).into_owned(), *theme::ERROR_TEXT_COLOUR));
        } else if self.vault_degraded {
            v.push((tr(Msg::StorageDegraded).into_owned(), *theme::DEGRADED_TEXT));
        }
        // Auto-attest armed: a standing security posture the operator must never be allowed to forget (Nick 2026-08-25).
        if self.unattended_on {
            v.push((tr(Msg::AutoAttestBadge).into_owned(), *theme::CLOCK_TEXT));
        }
        // Clock off (nunc-time consensus): warn only, Photon never corrects the clock.
        if let Some(offset_secs) = self.clock_off {
            let mag = offset_secs.unsigned_abs();
            // Hex is linear: the plain seconds count, no hour or minute rung.
            let pretty = tr(if crate::hex_ui() {
                Msg::SecondsShort(mag)
            } else if mag >= 3600 {
                Msg::HoursShort(mag / 3600)
            } else if mag >= 60 {
                Msg::MinutesShort(mag / 60)
            } else {
                Msg::SecondsShort(mag)
            });
            v.push((tr(Msg::ClockOff { pretty: &pretty, ahead: offset_secs < 0 }).into_owned(), *theme::CLOCK_TEXT));
        }
        // Update available: persistent, and ONLY while automatic checking is on — a user who turned the check off asked not to be told (Nick 2026-09-09).
        if let Some(ver) = self.update_available {
            if self.auto_updates_enabled() && ver != crate::network::updates::our_version() {
                v.push((tr(Msg::UpdateAvailableToast(&dozenal_version_tuple(ver))).into_owned(), *theme::SEARCH_FOUND_COLOUR));
            }
        }
        v
    }

}

/// A contact row's line step for a wrapped name: the name size is half the layout row, and lines stack at a quarter more than that.
fn contact_line_step(row_h: isize) -> f32 {
    row_h as f32 * 0.5 * 1.25
}

/// A contact row's height: the layout row for a one-line name, plus one line step per extra wrapped line. The extent clamp and the row walk share it.
fn contact_row_height(row_h: isize, lines: usize) -> isize {
    row_h + (lines.saturating_sub(1) as f32 * contact_line_step(row_h)).round() as isize
}

/// Paint the standing bands stacked up from the screen bottom — `band_h` per LINE, the span-based unit of the page that calls (zoom-aware, no pixel floor). A long band word-wraps at the window width and takes as many lines as it needs (Nick 2026-09-09), so the bands above it ride up by the same amount.
fn draw_standing_bands(bands: &[(String, u32)], canvas: &mut Canvas, text: &mut fluor::text::TextRenderer, buf_w: usize, buf_h: usize, band_h: f32) {
    let cx = buf_w as f32 * 0.5;
    let font_size = band_h * 0.6;
    let style_of = |colour: u32| TextStyle::new(font_size, colour).weight(600).font("Oxanium");
    let max_w = (buf_w as f32 - band_h * 2.0).max(band_h);
    let mut bottom = buf_h as f32;
    for (label, colour) in bands.iter() {
        let lines = wrap_text_lines(text, label, &style_of(*colour), max_w);
        let n = lines.len().max(1);
        // Lines stack top-down within the band; the band's bottom sits on the previous band's top.
        for (k, line) in lines.iter().enumerate() {
            let cy = bottom - band_h * (n as f32 - k as f32 - 0.5);
            text.draw_text_center(canvas, line, cx, cy, &style_of(*colour), None, None);
        }
        bottom -= band_h * n as f32;
    }
}

/// DOWNSCALE WITH OPACITY CONTRIBUTIONS (Nick 2026-09-12, "are we downscaling and tracking opacity contributions?"): fold one envelope channel to per-column VERTICAL COVERAGE — every bin lands in exactly one column with its OWN bar height, and a row's value is the fraction of that column's bins whose bar reaches it. Coverage is monotone non-increasing upward, solid where the bins agree and graded where they don't, so the contour anti-aliases from the true within-column distribution with no oversampling.
pub(super) fn wave_fold_coverage(e: &crate::call::wave_env::WaveEnv, cols: usize, rows: usize, ref_amp: f32) -> Vec<f32> {
    let lsb0 = e.lsb(0);
    let mut h_lut = [0f32; 256];
    for (b, h) in h_lut.iter_mut().enumerate() {
        *h = ((b as f32 * lsb0).max(0.0).sqrt() / ref_amp).clamp(0.0, 1.0) * rows as f32 * 0.92;
    }
    let bins = e.bins.max(1);
    let mut cov = vec![0f32; cols * rows.max(1)];
    let mut n = vec![0u32; cols];
    for b in 0..e.bins {
        let px = (b * cols / bins).min(cols - 1);
        n[px] += 1;
        let h = h_lut[e.data[b] as usize];
        let full = (h.floor() as usize).min(rows);
        let base = px * rows;
        for r in 0..full {
            cov[base + r] += 1.0;
        }
        if full < rows {
            cov[base + full] += h - full as f32;
        }
    }
    for px in 0..cols {
        if n[px] > 1 {
            let inv = 1.0 / n[px] as f32;
            for r in 0..rows {
                cov[px * rows + r] *= inv;
            }
        }
    }
    cov
}

/// Per-column colours from the three band tracks, THE AGB WAY (Nick: each band's power over the geometric mean of the three, the top ratio pinned at full — hue from the ratios, brightness constant): the same integer bin→column stack as the coverage fold, means per band, colour thru the VSF path once per fold.
pub(super) fn wave_fold_colours(e: &crate::call::wave_env::WaveEnv, cols: usize) -> Vec<u32> {
    let bins = e.bins.max(1);
    let mut acc = vec![[0u64; 3]; cols];
    let mut n = vec![0u32; cols];
    for b in 0..e.bins {
        let px = (b * cols / bins).min(cols - 1);
        n[px] += 1;
        for c in 0..3 {
            acc[px][c] += e.data[(c + 1) * e.bins + b] as u64;
        }
    }
    let lsb = [e.lsb(1), e.lsb(2), e.lsb(3)];
    (0..cols)
        .map(|px| {
            if n[px] == 0 {
                return theme::rgb_colour(0, 0, 0);
            }
            let p = |c: usize| (acc[px][c] as f32 / n[px] as f32 * lsb[c]).max(1e-12);
            let g = (p(0) * p(1) * p(2)).cbrt();
            let r = [p(0) / g, p(1) / g, p(2) / g];
            let top = r[0].max(r[1]).max(r[2]).max(1e-12);
            let ch = |v: f32| ((v / top).clamp(0.0, 1.0) * 255.0).round() as u8;
            theme::rgb_colour(ch(r[0]), ch(r[1]), ch(r[2]))
        })
        .collect()
}

/// Draw one half-band from a coverage fold: one solid run where coverage saturates, then per-pixel alpha up the graded contour (coverage is monotone, so the first near-zero row ends the column). `colour_of` picks the column's colour (the played/unplayed split).
#[allow(clippy::too_many_arguments)]
pub(super) fn wave_draw_coverage(canvas: &mut Canvas, cov: &[f32], cols: usize, rows: usize, wx0: f32, bcy: f32, up: bool, colour_of: &dyn Fn(usize) -> u32, list_top: f32, list_bottom: f32, clip: Option<fluor::paint::Clip>) {
    for px in 0..cols {
        let base = px * rows;
        let c = colour_of(px);
        let x = (wx0 + px as f32) as isize;
        let mut solid = 0usize;
        while solid < rows && cov[base + solid] >= 0.999 {
            solid += 1;
        }
        if solid > 0 {
            let (ty, th) = if up { (bcy - solid as f32, solid as f32) } else { (bcy, solid as f32) };
            let run_top = ty.max(list_top);
            let run_bot = (ty + th).min(list_bottom);
            if run_bot > run_top {
                paint::fill_rect(canvas, x, run_top as isize, 1, (run_bot - run_top) as isize, c, clip, None);
            }
        }
        for r in solid..rows {
            let a = cov[base + r];
            if a <= 0.004 {
                break;
            }
            let ac = (c & 0x00FF_FFFF) | (((a * 255.0) as u32) << 24);
            let ry = if up { bcy - r as f32 - 1.0 } else { bcy + r as f32 };
            if ry >= list_top && ry < list_bottom {
                paint::fill_rect(canvas, x, ry as isize, 1, 1, ac, clip, None);
            }
        }
    }
}
