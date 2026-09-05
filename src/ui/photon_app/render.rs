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
                AppState::Settings(SettingsPage::Language) => "Settings:Language",
                AppState::Settings(SettingsPage::About) => "Settings:About",
                AppState::Settings(SettingsPage::Wave) => "Settings:Audio",
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
                // Logo(4) + gap + killswitch + passless + link + no-servers prose(8×0.8) + consent block + TOKEN block + version + toggle + why-dozenal rant ≈ 37 rows collapsed (each prose line 0.8, three 1-row section headers, inter-block gaps); the version reveal adds the spelled line + "dozenal" header + 6 cheat rows ≈ 8.4.
                let rows = 37.0
                    + if self.about_version_spelled {
                        // The reveal (spelled line + index) plus, once found, the riddle beneath the index.
                        8.4 + if self.about_riddle_revealed { 7.0 } else { 0.0 }
                    } else {
                        0.0
                    }
                    + if !crate::dozenal_ui() { 1.8 } else { 0.0 };
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
        // PLACING a call gates on the current route being calibrated (Settings→Audio) — an uncalibrated speakerphone call is the bad-echo experience the calibration exists to end. Answering an INCOMING call never gates: answering with the fallback duck beats missing a call. Hoisted local: the Audio page arm reads it too, deep inside the chrome borrow where &self is unavailable.
        let route_calibrated = self.route_calibrated_now();
        let echo_calibrated = self.echo_calibrated_now();
        let voice_calibrated = self.voice_calibrated_now();
        let call_pill_enabled = self
            .active_contact()
            .and_then(|ci| self.contacts.get(ci))
            .map_or(false, |c| {
                !c.is_sibling && c.is_online && (c.chain_woven || c.friendship_id.is_some())
            })
            && route_calibrated;
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
                    crate::call::CallPhase::Ended => {
                        (c.final_osc.unwrap_or(c.phase_osc) - c.phase_osc).max(0) / ops
                    }
                    _ => 0,
                };
                (c.phase, name, direct, pi, dur)
            });
        // Ringing / Ended show a SECOND action (Decline / Delete) beside the primary — hoisted so the end-of-frame hit re-stamp agrees with the early paint without re-deriving the phase.
        let call_two_actions = call_overlay.as_ref().map_or(false, |(p, _, _, _, _)| {
            matches!(
                p,
                crate::call::CallPhase::Ringing | crate::call::CallPhase::Ended
            )
        });
        // Full-screen call panel: Ringing (redesign 2026-08-30 — the compact bar squeezed Answer/Decline under the title band where Android's heads-up notification drops), Ended (the roomy Keep/Delete/Play decision), and Active UNLESS minimized (the in-call screen: timer + speaker/end/add/back). Minimized Active yields to the screen underneath (Phase 3 strip / the compact bar), so messaging + navigation stay live.
        let call_minimized = self.call_minimized;
        let call_fullscreen = match call_overlay.as_ref().map(|(p, _, _, _, _)| *p) {
            Some(crate::call::CallPhase::Ringing) | Some(crate::call::CallPhase::Ended) => true,
            Some(crate::call::CallPhase::Active) => !call_minimized,
            _ => false,
        };
        // Duration string hoisted BEFORE the chrome borrow (`fmt_duration` reads `&self`; the `&mut self.chrome` borrow below would otherwise block it). Used by the full-screen timer + the Ended summary.
        let call_dur_str = call_overlay
            .as_ref()
            .map(|t| self.fmt_duration(t.4))
            .unwrap_or_default();
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
        let zoom_text = if crate::dozenal_ui() {
            crate::dozenal_glyphs((ctx.viewport.ru * 144.0).round().max(0.0) as u32)
        } else {
            format!("{}%", crate::fmt_num((ctx.viewport.ru * 100.0).round().max(0.0) as u32))
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
                        if direct {
                            // Glyph + duration only (no words) — nothing to translate, stays a raw format.
                            format!("\u{260E} {}", call_dur_str)
                        } else {
                            tr(Msg::CallActiveNoPath(&call_dur_str)).into_owned()
                        }
                    }
                    crate::call::CallPhase::Ended => tr(Msg::CallEndedDur(&call_dur_str)).into_owned(),
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
                // Actions: bottom third, thumb-reach, decline LEFT answer RIGHT with a generous gap — and bottom-anchored so an Android heads-up banner (which owns the top) can never cover them.
                let bw = w * 0.34;
                let bh = unit * 2.4;
                let by = h - bh * 0.5 - unit * 1.5;
                let bfont = unit * 0.75;
                match phase {
                    crate::call::CallPhase::Ringing => {
                        // Decline LEFT, Answer RIGHT — the incoming-call decision. UNCALIBRATED ROUTE = NO ANSWER (Nick 2026-09-02: "a single dropped call sucks but a lifetime of shit calls is worse — it literally tells me I'm shit out of luck until I calibrate"): the Answer button disables with the reason on screen; Decline stays live to silence the ring. One ~15s calibration per route, ever.
                        let cal_ok = route_calibrated;
                        if let Some(b) = self.call_decline_btn.as_mut() {
                            b.set_rect(w * 0.5 - bw * 0.5 - unit * 0.75, by, bw, bh);
                            b.set_font_size(bfont);
                            b.set_label(tr(Msg::Decline));
                            let id = b.hit_id();
                            b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                        }
                        if let Some(b) = self.call_action_btn.as_mut() {
                            b.set_rect(w * 0.5 + bw * 0.5 + unit * 0.75, by, bw, bh);
                            b.set_font_size(bfont);
                            b.set_label(tr(Msg::Answer));
                            b.set_enabled(cal_ok);
                            let id = b.hit_id();
                            b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                        }
                        if !cal_ok {
                            ctx.text.draw_text_center(
                                &mut canvas,
                                &tr(Msg::CallUncalibratedNoAnswer),
                                w * 0.5,
                                by - bh * 0.9,
                                &TextStyle::new(bfont * 0.85, *theme::SEARCH_FAIL_COLOUR).weight(600).font("Oxanium"),
                                None,
                                None,
                            );
                            ctx.text.draw_text_center(
                                &mut canvas,
                                &tr(Msg::CallUncalibratedHint),
                                w * 0.5,
                                by - bh * 0.4,
                                &TextStyle::new(bfont * 0.7, *theme::LABEL_COLOUR).weight(400).font("Oxanium"),
                                None,
                                None,
                            );
                        }
                    }
                    crate::call::CallPhase::Ended => {
                        // The save/discard decision reuses this full-screen panel: Play (preview) centred above, Delete LEFT, Keep RIGHT.
                        if let Some(b) = self.call_play_btn.as_mut() {
                            b.set_rect(w * 0.5, by - bh - unit * 0.6, w * 0.4, bh * 0.85);
                            b.set_font_size(bfont);
                            b.set_label(tr(Msg::Play));
                            let id = b.hit_id();
                            b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                        }
                        if let Some(b) = self.call_decline_btn.as_mut() {
                            b.set_rect(w * 0.5 - bw * 0.5 - unit * 0.75, by, bw, bh);
                            b.set_font_size(bfont);
                            b.set_label(tr(Msg::Delete));
                            let id = b.hit_id();
                            b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                        }
                        if let Some(b) = self.call_action_btn.as_mut() {
                            b.set_rect(w * 0.5 + bw * 0.5 + unit * 0.75, by, bw, bh);
                            b.set_font_size(bfont);
                            b.set_label(tr(Msg::Keep));
                            b.set_enabled(true); // the Ringing arm may have disabled it (uncalibrated-route gate)
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
                            b.set_enabled(true); // Ringing may have disabled it (uncalibrated-route gate)
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
                    crate::call::CallPhase::Ended => tr(Msg::CallBarKeepRecording).into_owned(),
                };
                // No validated direct path in a live phase → say so on the bar (media may be silent until a punch lands; the warning disappears live when it does). The ⚠ is safe everywhere: fonts are fully bundled + deterministic (fluor's explicit-db TextRenderer, zero system-font pulls — verified 2026-08-20), and Noto Sans Symbols 2 covers U+26A0 in the same 2600 block as the field-proven ☎.
                if !direct && !matches!(phase, crate::call::CallPhase::Ended) {
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
                    crate::call::CallPhase::Ended => Msg::Keep,
                    _ => Msg::HangUp,
                });
                if let Some(b) = self.call_action_btn.as_mut() {
                    b.set_rect(ax + action_w * 0.5, cy, action_w, pill_h);
                    b.set_font_size(call_font);
                    b.set_label(a_label);
                    // The compact bar's Answer honors the same uncalibrated-route gate as the panel; other phases re-enable.
                    b.set_enabled(!matches!(phase, crate::call::CallPhase::Ringing) || route_calibrated);
                    let id = b.hit_id();
                    b.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, id);
                }
                if call_two_actions {
                    let dx = x0 + status_w + gap * 2. + action_w;
                    let d_label = tr(if phase == crate::call::CallPhase::Ended {
                        Msg::Delete
                    } else {
                        Msg::Decline
                    });
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
                            tr(Msg::LockedByFleet),
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
                    let cx = (error_rect.x0 + error_rect.x1) as f32 * 0.5;
                    let cy = (error_rect.y0 + error_rect.y1) as f32 * 0.5;
                    // Half-height font: status messages are short by convention; full-rect-height is too loud for one-line text and overflows wide messages off the side.
                    ctx.text.draw_text_center(
                        &mut canvas,
                        &text,
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
                    // How-to guidance: the two ways the OTHER (already-in-fleet) device adds this one, plus the confirm. Small + dim so it reads as instructions, not chrome.
                    {
                        y += line_h * 0.9;
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
            // +1 on top of the proportional thickness so the presence/online ring keeps a visible annulus at small avatar sizes (where `avatar_r * 0.0375` floors at the 1px min and the ring all but vanishes). One extra pixel is imperceptible on large avatars, load-bearing on tiny ones.
            let ring_thickness = (avatar_r * 0.0375).max(1.0) + 1.0;
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
                let row_name = super::contact_visible_name(&self.contacts[ci], self.session.as_ref().map(|se| &se.identity_seed), self.fleet_settings.as_ref());
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
                        (row_top + row_h).min(buf_h as isize),
                        row_hit,
                    );
                }
            }

            // Persistent storage indicator at the bottom, two severities (split 2026-09-03 — a benign mirror hiccup and 43 lost values wore the same words): RED "storage lost data" when values are gone / no vault opened, amber "storage degraded" when the session is merely distrusted. Lost outranks degraded when both latch. The matching warm background tint already lives in the noise pass above (we swap BG_BASE → (*theme::BG_BASE_WARNING)) so we add no extra render pass here, just the text glyph. Full details live in the README.
            if self.vault_data_lost || self.vault_degraded {
                let (msg, colour) = if self.vault_data_lost {
                    (Msg::StorageDataLost, *theme::ERROR_TEXT_COLOUR)
                } else {
                    (Msg::StorageDegraded, *theme::DEGRADED_TEXT)
                };
                // Band height off the span-based layout unit (zoom-aware, aspect-ratio-robust, no pixel floor) — same scaling family as the rest of the screen.
                let band_h = ready_layout.unit_height * 1.5;
                let cx = buf_w as f32 * 0.5;
                let cy = buf_h as f32 - band_h * 0.5;
                let font_size = band_h * 0.6;
                ctx.text.draw_text_center(
                    &mut canvas,
                    &tr(msg),
                    cx,
                    cy,
                    &TextStyle::new(font_size, colour)
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
                let rows_below = if self.vault_data_lost || self.vault_degraded { 1.0 } else { 0.0 };
                let cy = buf_h as f32 - band_h * (0.5 + rows_below);
                let font_size = band_h * 0.6;
                ctx.text.draw_text_center(
                    &mut canvas,
                    &tr(Msg::AutoAttestBadge),
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
                let rows_below = (if self.vault_data_lost || self.vault_degraded { 1.0 } else { 0.0 })
                    + (if self.unattended_on { 1.0 } else { 0.0 });
                let cy = buf_h as f32 - band_h * (0.5 + rows_below);
                let font_size = band_h * 0.6;
                // Human-readable magnitude + direction. ahead = system clock reads later than truth.
                let mag = offset_secs.unsigned_abs();
                let pretty = tr(if mag >= 3600 {
                    Msg::HoursShort(mag / 3600)
                } else if mag >= 60 {
                    Msg::MinutesShort(mag / 60)
                } else {
                    Msg::SecondsShort(mag)
                });
                let label = tr(Msg::ClockOff { pretty: &pretty, ahead: offset_secs < 0 });
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
                    &super::contact_visible_name(contact, self.session.as_ref().map(|se| &se.identity_seed), self.fleet_settings.as_ref()),
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
                            avatar_r + (avatar_r * 0.0375).max(1.0) + 1.0,
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
                            )
                            .into()
                        };
                        let connection_line = if is_self {
                            tr(Msg::AlwaysReachableSelf)
                        } else if contact.is_online {
                            if contact.reached_via_relay {
                                tr(Msg::ConnectedRelay)
                            } else {
                                tr(Msg::ConnectedDirect)
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
                            let ring_thick = (avatar_r * 0.0375).max(1.0) + 1.0;
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
                                // call.audio rows draw in Oxanium (so the dozenal size + ▶ resolve) — measure with the SAME font or the wrapped line count disagrees with the draw.
                                let row_wrap = if crate::types::is_call_recording(&m.content) {
                                    TextStyle::new(msg_size, 0).weight(500).font("Oxanium")
                                } else {
                                    wrap_style.clone()
                                };
                                let lines =
                                    wrap_text_lines(ctx.text, &body_of(m), &row_wrap, avail_w);
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
                            // Opaque backdrop + a top hairline so the panel reads as its own surface over the stream.
                            paint::fill_rect(&mut canvas, pad_x as isize * 0 as isize, py0 as isize, buf_w as isize, (list_bottom - py0) as isize, 0xFF00_0000 | 0x00E8_E2D8, None, None);
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
                            consent_stamp = Some((prects, py0));
                        }
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
                                // Base-aware count (dozenal glyphs or decimal per the About toggle) — the detail style is Oxanium, so the glyphs resolve.
                                let age = tr(if secs >= 86400 {
                                    Msg::AgoDays((secs / 86400) as u32)
                                } else if secs >= 3600 {
                                    Msg::AgoHours((secs / 3600) as u32)
                                } else if secs >= 60 {
                                    Msg::AgoMinutes((secs / 60) as u32)
                                } else {
                                    Msg::AgoSeconds(secs as u32)
                                });
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
                                    (tr(Msg::CopiedPill), *theme::SEARCH_FOUND_COLOUR)
                                } else {
                                    (tr(Msg::CopyPill), *theme::COPY_PILL_COLOUR)
                                };
                                let mut pills: Vec<(std::borrow::Cow<'static, str>, u32, HitId)> =
                                    vec![(tr(Msg::ReplyPill), *theme::COPY_PILL_COLOUR, self.msg_action_base)];
                                if msg.is_outgoing
                                    && crate::types::parse_attachment_content(&msg.content)
                                        .is_none()
                                {
                                    pills.push((
                                        tr(Msg::EditPill),
                                        *theme::COPY_PILL_COLOUR,
                                        self.msg_action_base.wrapping_add(1),
                                    ));
                                }
                                pills.push((copy_label, copy_colour, self.msg_copy_id));
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
                                let pad_hit = detail_size;
                                let mut px_cursor = pad_x;
                                for (label, colour, hid) in pills {
                                    let style = TextStyle::new(detail_size, colour)
                                        .weight(600)
                                        .font("Oxanium");
                                    let w = ctx.text.measure_text(&label, &style);
                                    ctx.text.draw_text_left(
                                        &mut canvas,
                                        &label,
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
                            for (k, line) in lines.iter().enumerate() {
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
                                let link_style = TextStyle::new(msg_size, *theme::LINK_COLOUR).weight(500);
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
                                                *theme::LINK_COLOUR,
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
                            // Stamp the row band — the WHOLE wrapped block — so a tap selects this message (details strip). Clamped to the list region so header/compose never lose their own hits; capped at the 64-id span (a taller screen than that doesn't exist). ON-SCREEN ROWS ONLY (field 2026-09-05, Nick's Android: taps dead on every old message once scrolled up a ways): the walk starts at the NEWEST row and processes everything below the viewport first, so an unconditional push burned the whole 64-id budget on invisible rows — every visible old row then had no tap target. An empty clamped band = off-screen = no id spent.
                            let band_top = ((y - block_extra - line_h * 0.5).max(list_top)) as isize;
                            let band_bot = ((y + line_h * 0.5).min(list_bottom)) as isize;
                            if band_bot > band_top && self.msg_hit_rows.len() < 64 {
                                let row_hit = self
                                    .msg_hit_base
                                    .wrapping_add(self.msg_hit_rows.len() as HitId);
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
                                self.msg_hit_rows
                                    .push((msg.timestamp, msg.is_outgoing, ref_band));
                            }
                            y -= line_h + block_extra;
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
                                &super::contact_visible_name(contact, self.session.as_ref().map(|se| &se.identity_seed), self.fleet_settings.as_ref()),
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
                                    &tr(Msg::Add),
                                    btn_base.wrapping_add(2),
                                    ctx.pressed_hit,
                                );
                            }
                            YouRow::Note => {
                                ctx.text.draw_text_left(&mut canvas, &tr(Msg::YouNote), r.x + hspan2 * 0.3, r.center_y(), &TextStyle::new(hspan2, *theme::LABEL_COLOUR).font("Oxanium"), Some(content_clip), None);
                            }
                            YouRow::IdentityHeader => {
                                ctx.text.draw_text_left(
                                    &mut canvas,
                                    &tr(Msg::YouIdentity),
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
                        devices.iter().take(6).enumerate()
                    {
                        let row_locked = locked_set.contains(pk);
                        // NAME band — tap-to-copy stamped over it; transport dot leads.
                        let name_band = flow.band(hspan2 * 1.7);
                        if let Some(colour) = tier {
                            let r = hspan2 * 0.26;
                            paint::circle_filled(
                                &mut canvas,
                                (name_band.x + hspan2 * 0.5) as isize,
                                name_band.center_y() as isize,
                                r as isize,
                                *colour,
                                None,
                                None,
                            );
                        }
                        // Renaming THIS card: the name band IS the textbox (prefilled, focused); no tap-to-copy stamp while editing. Enter commits, Esc cancels (driver).
                        let renaming_here = self.fleet_rename.as_ref().is_some_and(|(rpk, _)| rpk == pk);
                        if renaming_here {
                            if let Some((_, tb)) = self.fleet_rename.as_mut() {
                                tb.set_rect(name_band.x + hspan2 * 1.1, name_band.center_y(), name_band.w - hspan2 * 1.6, name_band.h * 0.9);
                                tb.set_font_size(hspan2 * 0.95, ctx.text);
                                let id = tb.hit_id();
                                tb.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, Some(&mut chrome.hit_test_map), id);
                            }
                        } else {
                            ctx.text.draw_text_left(
                                &mut canvas,
                                name,
                                name_band.x + hspan2 * 1.1,
                                name_band.center_y(),
                                &TextStyle::new(hspan2 * 1.05, *theme::CONTACT_NAME_COLOUR)
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
                        // STATUS line: state + link path in one sentence.
                        let (status, status_colour) = if *is_self {
                            (tr(Msg::ThisDevice), *theme::LABEL_COLOUR)
                        } else if *retired {
                            (tr(Msg::RetiredStillYours), *theme::LABEL_COLOUR)
                        } else if row_locked {
                            (tr(Msg::LockedOut), theme::PILL_RED.1)
                        } else if *online {
                            (
                                if link.is_empty() { tr(Msg::Online) } else { tr(Msg::OnlineVia(link)) },
                                *theme::SEARCH_FOUND_COLOUR,
                            )
                        } else {
                            (tr(Msg::Offline), *theme::LABEL_COLOUR)
                        };
                        flow.line(&mut canvas, ctx.text, &status, hspan2 * 0.85, status_colour, 400);
                        // BUILD line — version · commit · os arch off the sealed pong tail (self shows its own build). A stale version here IS the not-updated indicator.
                        if !about.is_empty() {
                            flow.line(&mut canvas, ctx.text, about, hspan2 * 0.7, *theme::LABEL_COLOUR, 400);
                        }
                        // ACTION pills on their own band — flow_pills sizes to labels and wraps if the pane clamps.
                        let departing = self
                            .pending_depart_req
                            .as_ref()
                            .is_some_and(|(d, _, _)| d == pk);
                        if *retired {
                            let armed = self.fleet_release_armed.as_ref() == Some(pk);
                            let label = tr(Msg::ReleasePill { armed });
                            let band = flow.band(hspan2 * 2.4);
                            draw_stub_pill_filled(
                                &mut canvas,
                                ctx.text,
                                &mut chrome.hit_test_map,
                                buf_w,
                                buf_h,
                                fluor::region::Region::new(band.x + hspan2 * 0.3, band.y + band.h * 0.1, (band.w * 0.4).max(hspan2 * 6.0), band.h * 0.8),
                                &label,
                                btn_base.wrapping_add(24 + i as HitId),
                                ctx.pressed_hit,
                                true,
                                if armed { Some(*theme::PILL_RED) } else { None },
                                "Oxanium",
                            );
                        } else if *is_self {
                            // The self card's one action: Rename — this machine's name is the one most worth setting.
                            let band = flow.band(hspan2 * 2.4);
                            let pill_h = band.h * 0.8;
                            let rect = fluor::region::Region::new(band.x + hspan2 * 0.3, band.y + (band.h - pill_h) * 0.5, ctx.text.measure_text(&tr(Msg::RenamePill), &TextStyle::new(pill_h * 0.5, 0).font("Oxanium")) + pill_h * 0.8 + hspan2 * 0.4, pill_h);
                            draw_stub_pill_filled(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, rect, &tr(Msg::RenamePill), btn_base.wrapping_add(56 + i as HitId), ctx.pressed_hit, true, None, "Oxanium");
                        } else {
                            // Bridge + Rename + the row's state pill (Lock out / Unlock / Approve sign-out), each sized to its label.
                            let band = flow.band(hspan2 * 2.4);
                            let pill_h = band.h * 0.8;
                            let pill_y = band.y + (band.h - pill_h) * 0.5;
                            let mut x = band.x + hspan2 * 0.3;
                            let mut place = |canvas: &mut Canvas, text: &mut fluor::text::TextRenderer, hit_map: &mut [HitId], label: &str, hit: HitId, fill: Option<(u32, u32)>| {
                                let w = text.measure_text(label, &TextStyle::new(pill_h * 0.5, 0).font("Oxanium")) + pill_h * 0.8 + hspan2 * 0.4;
                                let rect = fluor::region::Region::new(x, pill_y, w, pill_h);
                                draw_stub_pill_filled(canvas, text, hit_map, buf_w, buf_h, rect, label, hit, ctx.pressed_hit, true, fill, "Oxanium");
                                x += w + hspan2 * 0.6;
                            };
                            let bridge_fill = if *online { Some(*theme::PILL_GREEN) } else { Some(*theme::PILL_GREY) };
                            place(&mut canvas, ctx.text, &mut chrome.hit_test_map, &tr(Msg::BridgePill), btn_base.wrapping_add(8 + i as HitId), bridge_fill);
                            place(&mut canvas, ctx.text, &mut chrome.hit_test_map, &tr(Msg::RenamePill), btn_base.wrapping_add(56 + i as HitId), None);
                            if departing {
                                let armed = self.fleet_approve_armed.as_ref() == Some(pk);
                                place(&mut canvas, ctx.text, &mut chrome.hit_test_map, &tr(Msg::ApproveSignOutPill { armed }), btn_base.wrapping_add(48 + i as HitId), Some(if armed { *theme::PILL_RED } else { *theme::PILL_YELLOW }));
                            } else if row_locked {
                                let armed = self.fleet_unlock_armed.as_ref() == Some(pk);
                                place(&mut canvas, ctx.text, &mut chrome.hit_test_map, &tr(Msg::UnlockPill { armed }), btn_base.wrapping_add(40 + i as HitId), if armed { Some(*theme::PILL_RED) } else { None });
                            } else {
                                let armed = self.fleet_lock_armed.as_ref() == Some(pk);
                                place(&mut canvas, ctx.text, &mut chrome.hit_test_map, &tr(Msg::LockOutPill { armed }), btn_base.wrapping_add(32 + i as HitId), if armed { Some(*theme::PILL_RED) } else { None });
                            }
                        }
                        // AIR between device cards — the whole point. Between cards (never after the last) the conversation's white hairline rides the midpoint (Nick 2026-09-03: "same white hairlines between messages"): pure white α=1/8 = VERSION_COLOUR, the between-messages divider treatment.
                        flow.gap(hspan2 * 0.6);
                        if i + 1 < devices.len().min(6) {
                            let hl = flow.band(ctx.viewport.ru.max(1.0) as Coord);
                            paint::fill_rect(
                                &mut canvas,
                                hl.x as isize,
                                hl.y as isize,
                                hl.w as isize,
                                ctx.viewport.ru.max(1.0) as isize,
                                theme::VERSION_COLOUR,
                                None,
                                None,
                            );
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
                        &[(add_device_label.as_ref(), btn_base, true)],
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
                                      armed: bool| {
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
                        draw_stub_pill_filled(canvas, text, hit_map, buf_w, buf_h, rect, label, btn_base.wrapping_add(slot), ctx.pressed_hit, true, Some(fill), "Open Sans");
                        let (hc, hw) = if armed { (*theme::ERROR_TEXT_COLOUR, 600) } else { (*theme::LABEL_COLOUR, 400) };
                        let region = fluor::region::Region::new(flow.x, flow.y, flow.w, hspan2 * 1.6);
                        let n = settings_prose(canvas, text, region, hint, hspan2 * 0.85, hc, hw);
                        flow.y += (n.max(1) as Coord) * hspan2 * 0.85 * 1.25 + hspan2 * 0.9;
                    };
                    action(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map,
                        &tr(Msg::SecurityLock),
                        &tr(Msg::SecurityLockHint),
                        0, *theme::PILL_GREEN, false);
                    action(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map,
                        &tr(Msg::SecurityRemove { armed: self.settings_remove_armed }),
                        &tr(Msg::SecurityRemoveHint),
                        1, if self.settings_remove_armed { *theme::PILL_RED } else { *theme::PILL_YELLOW }, self.settings_remove_armed);
                    action(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map,
                        &tr(Msg::SecurityShred { armed: self.settings_shred_armed }),
                        &tr(Msg::SecurityShredHint),
                        2, *theme::PILL_ORANGE, self.settings_shred_armed);
                    action(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map,
                        &tr(Msg::SecurityRemoveShred { armed: self.settings_removeshred_armed }),
                        &tr(Msg::SecurityRemoveShredHint),
                        3, *theme::PILL_RED, self.settings_removeshred_armed);
                    flow.gap(hspan2 * 0.4);
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
                        let band = flow.band(hspan2 * 2.4);
                        let pill_h = band.h * 0.8;
                        let py = band.y + (band.h - pill_h) * 0.5;
                        draw_stub_pill_filled(
                            &mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h,
                            fluor::region::Region::new(band.x + hspan2 * 0.3, py, band.w * 0.38, pill_h),
                            &tr(if target_on { Msg::Arm } else { Msg::Disarm }),
                            self.unattended_confirm_base, ctx.pressed_hit, true, Some(*theme::PILL_RED), "Open Sans",
                        );
                        draw_stub_pill(
                            &mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h,
                            fluor::region::Region::new(band.x + band.w * 0.45, py, band.w * 0.3, pill_h),
                            &tr(Msg::Cancel),
                            self.unattended_confirm_base.wrapping_add(1), ctx.pressed_hit,
                        );
                    } else {
                        let armed = self
                            .settings_unattended_check
                            .as_ref()
                            .map(|c| c.is_checked())
                            .unwrap_or(false);
                        let cb_band = flow.band(hspan2 * 2.0);
                        if let Some(cb) = self.settings_unattended_check.as_mut() {
                            cb.set_font_size(hspan2);
                            cb.set_rect(cb_band.x + cb_band.w * 0.45, cb_band.center_y(), cb_band.w * 0.85, cb_band.h * 0.8);
                            cb.render_content_into(
                                &mut canvas,
                                ctx.text,
                                None,
                                Some(&mut chrome.hit_test_map),
                            );
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
                    flow.line(&mut canvas, ctx.text, &tr(Msg::CustodiansVersion(&crate::dozenal_glyphs(1))), hspan2, *theme::CONTACT_NAME_COLOUR, 600);
                    flow.gap(hspan2 * 0.4);
                    if let Some(cb) = self.settings_custodian_check.as_mut() {
                        let band = flow.band(hspan2 * 2.0);
                        let cb_h = hspan2 * 1.3;
                        cb.set_font_size(hspan2);
                        let label_w = ctx.text.measure_text(&tr(Msg::CustodianCheckbox), &TextStyle::new(hspan2, 0));
                        let w = cb_h + hspan2 * 0.5 + label_w + hspan2 * 0.3;
                        cb.set_rect(band.x + w * 0.5, band.center_y(), w, cb_h);
                        cb.render_content_into(
                            &mut canvas,
                            ctx.text,
                            None,
                            Some(&mut chrome.hit_test_map),
                        );
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
                    let boxes: [(Option<&mut fluor::widgets::Checkbox>, &str); 4] = [
                        (self.settings_chime_check.as_mut(), &*chime),
                        (self.settings_vibrate_msg_check.as_mut(), &*vib_msg),
                        (self.settings_ring_call_check.as_mut(), &*ring_call),
                        (self.settings_vibrate_call_check.as_mut(), &*vib_call),
                    ];
                    for (cb, label) in boxes {
                        let Some(cb) = cb else { continue };
                        let band = flow.band(hspan2 * 2.0);
                        let cb_h = hspan2 * 1.3;
                        cb.set_font_size(hspan2);
                        let label_w = ctx.text.measure_text(label, &TextStyle::new(hspan2, 0));
                        let w = cb_h + hspan2 * 0.5 + label_w + hspan2 * 0.3;
                        cb.set_rect(band.x + w * 0.5, band.center_y(), w, cb_h);
                        cb.render_content_into(
                            &mut canvas,
                            ctx.text,
                            None,
                            Some(&mut chrome.hit_test_map),
                        );
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
                    // Rows (blanks between the pills for vertical breathing room): 0 title · 1 current version · 2 blank · 3 release pill · 4 blank · 5 dev pill · 6 blank · 7 status.
                    let rows = layout
                        .content_scrolled(8, settings_content_scroll)
                        .split_v([1.0; 8]);
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[0],
                        &tr(Msg::UpdatesTitle),
                        tspan,
                        *theme::CONTACT_NAME_COLOUR,
                        600,
                    );
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[1],
                        &tr(Msg::PhotonVersion(&version_dozenal_glyphs())),
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
                            ChannelCheck::Idle | ChannelCheck::Checking => {
                                (tr(Msg::UpdateChecking(kind)), (*theme::PILL_GREY), false)
                            }
                            ChannelCheck::Failed => {
                                (tr(Msg::UpdateUnavailable(kind)), (*theme::PILL_GREY), false)
                            }
                            ChannelCheck::Ready(None) => {
                                (tr(Msg::UpdateNoBuild(kind)), (*theme::PILL_GREY), false)
                            }
                            // Tuple equality IS the truth: patch 0 is the release marker and the version scheme guarantees a dev build never wears it (deploy.sh opens the dev line at .1; publishes are publish-current-then-bump) — so a dev build and the release can never be tuple-equal, and "already on" needs no flavour check.
                            ChannelCheck::Ready(Some(row)) if row.version == ours => {
                                let ver = dozenal_version_tuple(row.version);
                                (tr(Msg::UpdateAlreadyOn { kind, ver: &ver }), (*theme::PILL_GREY), false)
                            }
                            ChannelCheck::Ready(Some(row)) => {
                                let ver = dozenal_version_tuple(row.version);
                                (tr(Msg::UpdateGet { kind, ver: &ver }), avail_fill, !busy)
                            }
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
                            tr(Msg::Updating)
                        } else if total > 0 {
                            tr(Msg::Downloading)
                        } else {
                            tr(Msg::DownloadingMiB((done >> 20) as i64))
                        };
                        let label = &*label;
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
                        let mut m = tr(Msg::DiagMeta {
                            count: self.diag_log_rows.len(),
                            kib: ((crate::log_size_bytes() + 1023) / 1024) as usize,
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
                        &tr(Msg::PageName(page)),
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
                        &tr(Msg::DiagInfo {
                            used: &human_bytes(used),
                            cap: &human_bytes(cap),
                            pct: pct as u64,
                        }),
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
                        &tr(Msg::DiagClear),
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
                        &tr(Msg::DiagSnapshot),
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
                            &tr(Msg::DiagSubmit),
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
                            &tr(Msg::DiagSubmit),
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
                        &tr(Msg::DiagView),
                        btn_base.wrapping_add(3),
                        ctx.pressed_hit,
                    );
                    settings_line(
                        &mut canvas,
                        ctx.text,
                        rows[6],
                        &tr(Msg::OptionalNote),
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
                SettingsPage::Wave => {
                    // WAVE — two NAMED measurements (Nick 2026-09-02: the combined flow was vague), each with purpose, setup, live phase, and a measured RESULT so the page reads like an instrument. Slots: 0 echo · 1 voice · 2 headset-skip.
                    use crate::call::calibrate::CalPhase;
                    let inset = layout.content_inset();
                    let mut flow = Flow::new(inset, settings_content_scroll);
                    flow.line(&mut canvas, ctx.text, &tr(Msg::PageName(page)), tspan, *theme::CONTACT_NAME_COLOUR, 600);
                    // The 1960 epigraph (docs/lexicon.md, captured off the physical page by the Lumis rig): the page is named for the proper sense, from before the debotcherization. A doubled-escape bug used to render a literal "\u{2014}" here; the catalog arm carries the real em dash.
                    flow.prose(&mut canvas, ctx.text, &tr(Msg::WaveEpigraph), hspan2 * 0.8, *theme::LABEL_COLOUR, 400);
                    flow.gap(hspan2 * 0.5);
                    flow.prose(&mut canvas, ctx.text, &tr(Msg::WaveIntroHow), hspan2 * 0.9, *theme::LABEL_COLOUR, 400);
                    flow.gap(hspan2 * 0.4);
                    flow.prose(&mut canvas, ctx.text, &tr(Msg::WaveIntroSetup), hspan2 * 0.9, *theme::LABEL_COLOUR, 400);
                    flow.gap(hspan2 * 0.4);
                    // The reassurance humans won't infer (Nick 2026-09-02: obvious, but people don't read — say it anyway): the mic is live during calibration, so state plainly that nothing recorded here goes anywhere. "No servers" is an architecture claim; "never leaves this device" is the sentence a person actually needs while a microphone is hot.
                    flow.prose(&mut canvas, ctx.text, &tr(Msg::WaveIntroPrivacy), hspan2 * 0.9, *theme::SEARCH_FOUND_COLOUR, 400);
                    flow.gap(hspan2 * 0.8);
                    let phase = crate::call::calibrate::phase();
                    let running = !matches!(phase, CalPhase::Idle | CalPhase::Done | CalPhase::Failed);
                    let route = crate::platform::audio::route_id();
                    let mic = crate::platform::audio::mic_id();
                    let echo_done = echo_calibrated;
                    let voice_done = voice_calibrated;

                    // ── STEP 1: ECHO ─────────────────────────────────────────
                    let route_disp = if route.is_empty() { tr(Msg::WaveNoOutputYet).into_owned() } else { route.clone() };
                    flow.line(&mut canvas, ctx.text, &tr(Msg::WaveStep1Title(&route_disp)), hspan2, *theme::CONTACT_NAME_COLOUR, 600);
                    flow.line(
                        &mut canvas,
                        ctx.text,
                        &tr(if echo_done { Msg::WaveMeasuredTick } else { Msg::WaveNotMeasured }),
                        hspan2 * 0.85,
                        if echo_done { *theme::SEARCH_FOUND_COLOUR } else { *theme::SEARCH_FAIL_COLOUR },
                        500,
                    );
                    if phase == CalPhase::EchoListen {
                        flow.prose(&mut canvas, ctx.text, &tr(Msg::WaveEchoListenInstruction), hspan2 * 0.9, *theme::CONTACT_NAME_COLOUR, 500);
                    } else {
                        flow.prose(&mut canvas, ctx.text, &tr(Msg::WaveEchoSetupInstruction), hspan2 * 0.85, *theme::LABEL_COLOUR, 400);
                    }
                    flow.gap(hspan2 * 0.3);
                    // NO SKIP (Nick 2026-09-02: "every device leaks, it's just how much — helium inside a stainless capsule will not stay there forever"): a headset's coupling isn't zero, it's small, and the instrument measures small just fine. Every route runs the echo check for real.
                    let echo_pill = tr(if phase == CalPhase::EchoListen { Msg::WaveMeasuring } else if echo_done { Msg::RemeasureEcho } else { Msg::MeasureEcho });
                    flow_pills(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, ctx.pressed_hit, hspan2, &[(
                        &*echo_pill,
                        btn_base,
                        !running,
                    )]);
                    flow.gap(hspan2 * 0.9);

                    // ── STEP 2: VOICE ────────────────────────────────────────
                    let mic_disp = if mic.is_empty() { tr(Msg::WaveNoMicYet).into_owned() } else { mic.clone() };
                    flow.line(&mut canvas, ctx.text, &tr(Msg::WaveStep2Title(&mic_disp)), hspan2, *theme::CONTACT_NAME_COLOUR, 600);
                    flow.line(
                        &mut canvas,
                        ctx.text,
                        &tr(if voice_done { Msg::WaveMeasuredTick } else { Msg::WaveNotMeasured }),
                        hspan2 * 0.85,
                        if voice_done { *theme::SEARCH_FOUND_COLOUR } else { *theme::SEARCH_FAIL_COLOUR },
                        500,
                    );
                    match phase {
                        CalPhase::VoiceExample => {
                            flow.prose(&mut canvas, ctx.text, &tr(Msg::WaveVoiceExampleListen), hspan2 * 0.9, *theme::CONTACT_NAME_COLOUR, 500);
                            flow.prose(&mut canvas, ctx.text, &tr(Msg::WaveVoiceSentence), hspan2, *theme::CONTACT_NAME_COLOUR, 600);
                        }
                        CalPhase::VoiceRepeat => {
                            flow.prose(&mut canvas, ctx.text, &tr(Msg::WaveVoiceRepeatInstruction), hspan2 * 0.9, *theme::CONTACT_NAME_COLOUR, 500);
                            flow.prose(&mut canvas, ctx.text, &tr(Msg::WaveVoiceSentence), hspan2 * 1.05, *theme::CONTACT_NAME_COLOUR, 600);
                        }
                        _ => {
                            flow.prose(&mut canvas, ctx.text, &tr(Msg::WaveVoiceDefaultInstruction), hspan2 * 0.85, *theme::LABEL_COLOUR, 400);
                        }
                    }
                    flow.gap(hspan2 * 0.3);
                    let voice_pill = tr(if matches!(phase, CalPhase::VoiceExample | CalPhase::VoiceRepeat) { Msg::WaveMeasuring } else if voice_done { Msg::RemeasureVoice } else { Msg::MeasureVoice });
                    flow_pills(&mut flow, &mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, ctx.pressed_hit, hspan2, &[(
                        &*voice_pill,
                        btn_base.wrapping_add(1),
                        !running,
                    )]);
                    flow.gap(hspan2 * 0.6);
                    if phase == CalPhase::Failed {
                        flow.prose(&mut canvas, ctx.text, &tr(Msg::WaveFailedHint), hspan2 * 0.9, *theme::SEARCH_FAIL_COLOUR, 600);
                        flow.gap(hspan2 * 0.4);
                    }
                    if echo_done && voice_done {
                        flow.prose(&mut canvas, ctx.text, &tr(Msg::WaveComplete), hspan2 * 0.9, *theme::SEARCH_FOUND_COLOUR, 500);
                    }
                    flow.gap(hspan2);
                    measured_extent = Some((flow.used(), inset.h));
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
                            tr(Msg::AboutClockOffset {
                                ms: &format!("{sign}{}", crate::fmt_num(o.unsigned_abs() as u32)),
                                conf: &crate::fmt_num(c.unsigned_abs() as u32),
                            })
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
                    if self.about_version_spelled {
                        // Spelled-out (voca words), then the dozenal cheat sheet: all twelve digits as GLYPH = name, two columns of six.
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
                        let index_top = y;
                        ctx.text.draw_text_center(
                            &mut canvas,
                            &tr(Msg::AboutDozenalHead),
                            cx,
                            y + line_h * 0.5,
                            &TextStyle::new(hspan2, *theme::CONTACT_NAME_COLOUR)
                                .weight(600)
                                .font("Oxanium"),
                            about_clip,
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
                        // The whole index (header + twelve digit cells) is one tap target — a single tap within it reveals the custodian riddle below (slot 5).
                        restamp_hit_rect(
                            &mut chrome.hit_test_map,
                            buf_w,
                            buf_h,
                            inset.x as isize,
                            index_top as isize,
                            (inset.x + inset.w) as isize,
                            y as isize,
                            btn_base.wrapping_add(5),
                        );
                        // The easter egg — one tap within the dozenal index above and the custodian riddle appears: undecidability as the load-bearing defense (nobody can establish whether key material exists, so there is no answer to rubber-hose out of anyone). Session-permanent once found; collapses with the index.
                        if self.about_riddle_revealed {
                            y += line_h * 0.4;
                            ctx.text.draw_text_center(
                                &mut canvas,
                                &crate::dozenal_glyphs(42),
                                cx,
                                y + line_h * 0.5,
                                &TextStyle::new(hspan2, *theme::SEARCH_FOUND_COLOUR)
                                    .weight(400)
                                    .font("Oxanium"),
                                None,
                                None,
                            );
                            y += line_h;
                            let s = tr(Msg::AboutRiddle);
                            for line in s.lines() {
                                ctx.text.draw_text_center(
                                    &mut canvas,
                                    line,
                                    cx,
                                    y + line_h * 0.4,
                                    &prose_style,
                                    None,
                                    None,
                                );
                                y += line_h * 0.8;
                            }
                        }
                    }
                    // Fleet-wide base toggle (display.dozenal — linked, so a preference follows the identity). Rect set inline off the same y cursor (this page is a card, not equal rows).
                    y += line_h * 0.4;
                    let mut decimal_mode = false;
                    if let Some(cb) = self.settings_dozenal_check.as_mut() {
                        decimal_mode = !cb.is_checked();
                        // The shame fill: untick dozenal and the empty box turns Zil.lun red (half-intensity, dozenal 0;6). Cleared the moment the user repents.
                        cb.set_empty_fill(decimal_mode.then(|| *theme::DOZENAL_SCOLD_BOX));
                        // CENTRED under cx: the widget's width is the measured box+gap+label, not a pane fraction — a wide rect left the box+label reading left-aligned (Nick 2026-09-02).
                        let cb_h = line_h * 0.9;
                        cb.set_font_size(hspan2);
                        let label_w = ctx.text.measure_text(&tr(Msg::Dozenal), &TextStyle::new(hspan2, 0));
                        cb.set_rect(cx, y + line_h * 0.5, cb_h + hspan2 * 0.5 + label_w + hspan2 * 0.3, cb_h);
                        cb.render_content_into(
                            &mut canvas,
                            ctx.text,
                            None,
                            Some(&mut chrome.hit_test_map),
                        );
                    }
                    if decimal_mode {
                        // The scold — in the primary VSF orange (Zila red, Zil.lun green, Zil blue).
                        y += line_h;
                        ctx.text.draw_text_center(
                            &mut canvas,
                            &tr(Msg::WhyDecimalScold),
                            cx,
                            y + line_h * 0.5,
                            &TextStyle::new(hspan2 * 0.85, *theme::DOZENAL_SCOLD_COLOUR)
                                .weight(600)
                                .font("Oxanium"),
                            about_clip,
                            None,
                        );
                    }
                    y += line_h * 1.4;
                    // The dozenal rant — why the toggle above defaults ON. Kept playful on purpose: decimal is an anatomical accident, not a design, and the page should own that opinion out loud.
                    ctx.text.draw_text_center(
                        &mut canvas,
                        &tr(Msg::WhyDozenal),
                        cx,
                        y + line_h * 0.5,
                        &TextStyle::new(hspan2, *theme::SEARCH_FOUND_COLOUR)
                            .weight(600)
                            .font("Oxanium"),
                        about_clip,
                        None,
                    );
                    y += line_h;
                    let rant = tr(Msg::WhyDozenalProse);
                    for line in rant.lines() {
                        y = centered_wrapped(&mut canvas, ctx.text, cx, wrap_w, y, line, &prose_style, line_h * 0.8, about_clip);
                        y += line_h * 0.3;
                    }
                    // MEASURED extent (Flow doctrine): the card's true height from the final cursor — retires the hand-counted row arithmetic next frame.
                    measured_extent = Some((y + settings_content_scroll - inset.y + line_h, inset.h));
                    let _ = tspan;
                }
            }
        }

        } // end !call_fullscreen — per-screen bodies skipped while the ring panel owns the surface

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
                    Some(crate::call::CallPhase::Ended) => {
                        if let Some(b) = self.call_play_btn.as_ref() {
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
