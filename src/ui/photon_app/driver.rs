//! The [`FluorApp`] trait impl — window lifecycle, event routing, tick cadence, IME, and cursor/hit plumbing; the frame paint body lives in `render.rs` as `render_frame`.

use super::*;

impl FluorApp for PhotonApp {
    /// One-shot absolute-zoom restore: the persisted per-device `display.zoom`, set when settings load; the host applies it exactly like a user zoom.
    fn take_zoom_request(&mut self) -> Option<f32> {
        let r = self.pending_zoom_restore.take();
        if let Some(v) = r {
            // Seed the persistence tracker with the restored value so launch doesn't immediately re-save what's already on disk.
            self.zoom_saved_ru = v;
        }
        r
    }

    /// One-shot window-geometry restore: a fluor `window_rect` in GLOBAL desktop units, applied by the host thru its own maximize machinery (clamped into live surfaces). Never raw winit calls — the parked 2026-08-16 version moved the fullscreen OS surface itself and the window vanished with a dead click region.
    fn take_window_geometry_request(&mut self) -> Option<(i32, i32, u32, u32)> {
        self.pending_geometry_restore.take()
    }

    type UserEvent = PhotonEvent;

    fn title(&self) -> &str {
        // OS WINDOW title only (taskbar / alt-tab / WM) — set once at window creation via winit `with_title`. The brand name lives here. The DRAWN in-app title bar is separate: it's `chrome.set_title(...)` per-frame in `render` ("← Network" on launch, live peer count on Ready). The chrome is constructed with "Photon" too but the first render overrides it before the first rasterize, so the drawn bar never flashes the brand name.
        "Photon"
    }

    fn initial_size(&self, monitor: (u32, u32)) -> (u32, u32) {
        // Portrait launch window — matches the pre-fluor Photon dimensions: height = half the SHORTER screen axis, width = half that. Yields a tall 1:2 (w:h) rectangle on any aspect ratio. Examples: 1920×1080 → 270×540; 1080×1920 → 270×540; 2560×1440 → 360×720.
        let short = monitor.0.min(monitor.1);
        let h = short >> 1;
        let w = h >> 1;
        (w, h)
    }

    fn wants_keyboard(&mut self) -> Option<bool> {
        // Return the one-shot keyboard transition set by `change_focus` and clear it so subsequent polls see `None` until focus moves again — keeps the Android Activity from calling `InputMethodManager.show/hide` every frame.
        self.pending_keyboard_request.take()
    }

    /// Honest-IME read: the FOCUSED textbox's text + cursor (chars), thru the one registry — any box (compose, search, profile fields) mirrors truthfully to the Android InputConnection.
    fn ime_editor_state(&mut self) -> Option<(String, usize)> {
        let focus = self.focused?;
        // The multi-line compose box lives outside the single-line registry — its branch first.
        if let Some(tb) = self
            .message_textbox
            .as_ref()
            .filter(|t| t.hit_id() == focus)
        {
            let text: String = tb.chars.iter().collect();
            let cursor = tb.cursor.min(tb.chars.len());
            return Some((text, cursor));
        }
        let tb = self.textbox_by_hit_mut(focus)?;
        let text: String = tb.chars.iter().collect();
        let cursor = tb.cursor.min(tb.chars.len());
        Some((text, cursor))
    }

    /// Honest-IME write: TRUE range replacement on the focused textbox — how voice dictation rewrites earlier words (setComposingRegion) without the backspace-replay hack.
    fn ime_replace_chars(
        &mut self,
        start: usize,
        end: usize,
        s: &str,
        text: &mut fluor::text::TextRenderer,
    ) {
        let Some(focus) = self.focused else { return };
        if let Some(tb) = self
            .message_textbox
            .as_mut()
            .filter(|t| t.hit_id() == focus)
        {
            tb.replace_char_range(start, end, s, text);
        } else if let Some(tb) = self.textbox_by_hit_mut(focus) {
            tb.replace_char_range(start, end, s, text);
        }
        self.scene_dirty = true;
    }

    fn wants_input_reset(&mut self) -> bool {
        // One-shot: drained after a send so the Activity restarts IME input exactly once.
        std::mem::replace(&mut self.pending_input_reset, false)
    }

    fn set_event_proxy(&mut self, proxy: Arc<dyn WakeSender<Self::UserEvent>>) {
        // Desktop resident mode: start serving the second-launch control channel now that we can wake the UI thread. No-op if main never parked a listener (lock-holder only).
        #[cfg(not(target_os = "android"))]
        {
            crate::platform::control::spawn_accept_thread(proxy.clone());
            // Resident from launch → the orb parks next to the clock now; a later toggle-on spawns it then (tray_spawned gates the once-per-process).
            if self.resident_mode {
                crate::platform::tray::spawn(proxy.clone());
                self.tray_spawned = true;
            }
        }
        self.event_proxy = Some(proxy);
    }

    fn start_hidden(&self) -> bool {
        self.start_in_background
    }

    /// The host settled the visible `window_rect` after a user gesture (drag-move release / resize-drag end) — fires once per gesture, so the gesture IS the durability edge: persist immediately, no dirty tracking, no flush edges.
    fn on_window_rect_changed(&mut self, x: i32, y: i32, w: u32, h: u32) {
        self.save_window_geometry(x, y, w, h);
    }

    /// The app's folded OS focus flipped. GAIN counts as fleet-attention input (2026-08-18): alt-tab and titlebar clicks deliver no press to on_event, yet the human is demonstrably here — take the ball (which internally re-claims the open conversation and clears its away-unread). LOSS keeps the 2026-07-23 edge: retract our active-clearer role so a sibling can ding for this conversation again. No timers.
    fn on_focus_changed(&mut self, focused: bool) {
        if focused {
            // Fluor fires this BEFORE dispatching the Focused event that normally stamps the attended atomic — stamp it now (idempotent with that arm) so the re-claim inside take_fleet_attention sees attended=true. And the gain IS input: stamp the recency clock, or a message arriving right after the alt-tab reads us stale.
            #[cfg(not(target_os = "android"))]
            crate::platform::desktop_notify::set_window_focused(true);
            self.last_interaction = Some(Instant::now());
            // If the ball never left us (alt-tab away and back — no input elsewhere), take() no-ops but the blur edge below already RETRACTED our claim: re-claim explicitly, the 2026-07-23 behavior.
            if !self.take_fleet_attention()
                && self.active_conversation.is_some()
                && matches!(
                    self.state,
                    AppState::Conversation | AppState::ContactPanel(_)
                )
            {
                self.broadcast_focus_claim(true);
                if let Some(ci) = self.active_contact() {
                    self.clear_unread(ci);
                }
            }
        } else if self.active_conversation.is_some()
            && matches!(
                self.state,
                AppState::Conversation | AppState::ContactPanel(_)
            )
        {
            self.broadcast_focus_claim(false);
        }
    }

    fn on_close_requested(&mut self) -> bool {
        // Deliberate-quit overrides: Shift+Escape's one-shot flag, or shift held on the close itself (shift+✕, shift+Alt-F4). Either way the user asked for the REAL exit — decline residency this once and let the host exit.
        if self.exit_requested || self.shift_held {
            crate::log(
                "EXIT: deliberate quit (shift+close / Shift+Escape) — bypassing resident hide",
            );
            // Quit is a flush edge: our `false` makes the host process::exit(), and the soft-mode RAM batch dies with the process (field 2026-08-21: freshly-recreated hang evidence evaporated on close because only panic/background/submit flushed).
            crate::flush_log_buffer();
            return false;
        }
        // Resident mode: close = hide, keep running (network, timers, notifications). The host does the set_visible(false); we track "nobody's looking" for the notification gate. Non-resident closes exit as ever.
        if self.resident_mode {
            #[cfg(not(target_os = "android"))]
            crate::platform::desktop_notify::set_window_visible(false);
            crate::log("RESIDENT: window hidden on close — still running; launch photon again to surface it");
            true
        } else {
            // Same flush edge as the deliberate-quit path above — non-resident close exits the process.
            crate::flush_log_buffer();
            false
        }
    }

    fn on_user_event(&mut self, event: PhotonEvent, _ctx: &mut Context) -> EventResponse {
        if matches!(event, PhotonEvent::ShowWindow) {
            #[cfg(not(target_os = "android"))]
            crate::platform::desktop_notify::set_window_visible(true);
            self.scene_dirty = true;
            return EventResponse::ShowWindow;
        }
        // Every other variant is a pure wake — the loop's tick drains whatever channel the sender filled.
        EventResponse::Pass
    }

    fn init(&mut self, ctx: &mut Context) {
        // Register Photon's Oxanium font weights with fluor's shared `TextRenderer` so the logo wordmark can resolve `Family::Name("Oxanium")`. ExtraLight/Light/Regular/Medium/SemiBold/Bold/ExtraBold = numeric weights 200/300/400/500/600/700/800. The logo uses weight 800.
        let db = ctx.text.font_system_mut().db_mut();
        db.load_font_data(
            include_bytes!("../../../assets/Oxanium/Oxanium-ExtraLight.ttf").to_vec(),
        );
        db.load_font_data(include_bytes!("../../../assets/Oxanium/Oxanium-Light.ttf").to_vec());
        // Regular weight uses the `+glyphs` superset: identical to plain Oxanium-Regular for 0x20-0x7e (normal text) but adds the dozenal digit glyphs in the reserved control-code block 0x10-0x1b (DLE..ESC = digits 0..11, Zil..Stelor). Rendering a dozenal number is then a plain draw_text of those bytes at weight 400 — no runtime SVG, no separate font family. Other weights stay on the plain faces (the dozenal glyphs only need to exist at one weight, and the version string renders at 400).
        db.load_font_data(
            include_bytes!("../../../assets/Oxanium/Oxanium-Regular+glyphs.ttf").to_vec(),
        );
        db.load_font_data(include_bytes!("../../../assets/Oxanium/Oxanium-Medium.ttf").to_vec());
        db.load_font_data(include_bytes!("../../../assets/Oxanium/Oxanium-SemiBold.ttf").to_vec());
        db.load_font_data(include_bytes!("../../../assets/Oxanium/Oxanium-Bold.ttf").to_vec());
        db.load_font_data(include_bytes!("../../../assets/Oxanium/Oxanium-ExtraBold.ttf").to_vec());

        // Chrome owns its own hit-test map sized to the viewport, allocates four hit-ids for its buttons via the threaded counter, and stamps the perimeter + button rasters in `rasterize_chrome`. The Photon orb (chromatic starburst — same brand mark as the OS-level app icon) ships as a VSF image and decodes into the chrome's app_icon slot. Decode the bundled orb (the Photon brand mark, and the app_icon slot that swaps to a peer's avatar in a conversation). A decode failure logs LOUDLY instead of silently falling back to a plain coloured disk — a stale asset against a bumped vsf format is exactly how a blank orb shipped unnoticed, so make the next one scream rather than degrade in silence.
        // DEV builds get a per-build random gradient orb so a fresh upload is visible at a glance; release ships the real brand mark.
        #[cfg(feature = "development")]
        let orb_icon = Some(dev_gradient_orb());
        #[cfg(not(feature = "development"))]
        let orb_icon = match fluor::host::icon::Icon::from_vsf_bytes(include_bytes!(
            "../../../assets/photon-orb.vsf"
        )) {
            Ok(icon) => Some(icon),
            Err(e) => {
                crate::logf!("ORB: bundled photon-orb.vsf failed to decode ({}) — orb falls back to a plain disk; the asset is likely stale against the current vsf format", format!("{:?}", e));
                None
            }
        };
        let mut chrome = DefaultChrome::new(
            ctx.viewport,
            "Photon",
            orb_icon,
            None,
            &mut self.hit_counter,
        );
        // Android: full-screen surface owns the whole display, so drop the desktop window chrome — no perimeter hairline, no top-right min/max/close buttons. Keeps the orb (connectivity indicator) on the top-left. set_full_edge skips draw_window_edges_and_mask; the `DEBUG_SKIP_CONTROLS` flag (also used by the desktop `[]l` chord) gates the controls-strip rasterization, so flipping it once at startup persistently suppresses the strip on Android.
        #[cfg(target_os = "android")]
        {
            chrome.set_full_edge(true);
            fluor::paint::DEBUG_SKIP_CONTROLS.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        // Top-left orb's ring doubles as the FGTW connectivity indicator. Initialize red/offline; `try_recv_online` flips to green once the FGTW reports the device is reachable.
        chrome.set_orb_tint(orb_tint_for(false));
        // Keep a clone of the brand orb so a conversation can swap in the peer's avatar and this restores it on the way out.
        self.photon_orb = chrome.app_icon.clone();
        self.chrome = Some(chrome);

        // Launch-screen widgets: handle textbox + attest button. Constructed with placeholder geometry; real geometry lands in `update_widget_layout` (called below and on every resize). Hit IDs are allocated from the shared counter AFTER chrome's four — chrome currently takes 1..=4, launch widgets get 5..=6, contacts widgets get 7..=8.
        self.textbox = Some(Textbox::new(&mut self.hit_counter, 0., 0., 1., 1., 12.));
        self.attest_btn = Some(Button::new(
            &mut self.hit_counter,
            0.,
            0.,
            1.,
            1.,
            12.,
            "Attest",
        ));
        // Contacts-page widgets — same placeholder shape; geometry set every frame via `update_widget_layout` based on ReadyLayout. The plus button label is "+" for now; the rotating-hourglass animation lands in a follow-up when we extract `ProgressButton` into fluor.
        self.contacts_textbox = Some(Textbox::new(&mut self.hit_counter, 0., 0., 1., 1., 12.));
        self.contacts_plus_btn = Some(Button::new(&mut self.hit_counter, 0., 0., 1., 1., 12., "+"));
        // Conversation compose box — placeholder geometry; positioned each frame via `update_widget_layout`.
        self.message_textbox = Some(fluor::widgets::MultiTextbox::new(
            &mut self.hit_counter,
            12.,
            "Oxanium",
        ));
        // Send button overlaid in the compose box. ASCII ">" (not "→" U+2192 — absent from the Android font, so it rendered blank there; the contacts "+" button proves ASCII renders). Geometry set each frame in `update_widget_layout`. Empty label — the glyph is a drawn 4-vertex up arrowhead (draw_up_arrowhead), not text.
        self.message_send_btn = Some(Button::new(&mut self.hit_counter, 0., 0., 1., 1., 12., ""));
        // Specific subtle hover for the two overlay-in-textbox action buttons (pre-fluor per-control hover colours), instead of the generic saturated BUTTON_HOVER. Held = the SAME subtle fill: these fire on release, so a press must read as "nothing happened yet" — the default BUTTON_HELD ramp flashed a heavy fill mid-press (the "+" ticket).
        if let Some(b) = self.contacts_plus_btn.as_mut() {
            b.set_hover_fill(Some(*theme::SEND_BUTTON_HOVER));
            b.set_held_fill(Some(*theme::SEND_BUTTON_HOVER));
        }
        if let Some(b) = self.message_send_btn.as_mut() {
            b.set_hover_fill(Some(*theme::SEND_BUTTON_HOVER));
            b.set_held_fill(Some(*theme::SEND_BUTTON_HOVER));
        }
        // Reserve a hit-id for the Ready-screen avatar circle. Not a Widget — the avatar is just a paint primitive — so click dispatch is handled directly in `on_event`'s MouseInput::Pressed arm, not thru `widget::dispatch_click`. Incrementing the shared counter keeps the contiguous-id contract intact for the `[]h` debug overlay.
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.avatar_hit_id = self.hit_counter;
        // KnownHandle fork pills (pick-another / it's-mine) — plain hit rects like the avatar circle, dispatched in the Pressed arm.
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.known_pick_hit = self.hit_counter;
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.known_mine_hit = self.hit_counter;
        // Reserve a block of 256 hit IDs for contact rows. Row i stamps `contact_hit_base + i`.
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.contact_hit_base = self.hit_counter;
        self.hit_counter = self.hit_counter.wrapping_add(255);
        // Back button on conversation screen.
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.back_btn_hit_id = self.hit_counter;

        // "Start fresh (wipe this device)" tappable on the JOIN words screen — the only clean path for a device that was REMOVED from a fleet and so can't attest (can't reach the Security page). Two-tap confirm → clean_device_for_reuse.
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.join_startfresh_hit_id = self.hit_counter;

        // "Copy words" tappable on the JOIN words screen.
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.join_copywords_hit_id = self.hit_counter;

        // Green-confirm tappable on the AddDevice screen ("It's in — finish"): the two-phase press that releases the fleet-key rotation after the human sees the new device enrolled.
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.add_confirm_hit_id = self.hit_counter;

        // Tappable candidate rows on the AddDevice screen (BLE/list select): 8-id block, row i stamps base + i.
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.add_candidate_hit_base = self.hit_counter;
        self.hit_counter = self.hit_counter.wrapping_add(7);

        // Settings panel (STUB) hit-id blocks + widgets. Reserve a contiguous 9-id block for the nav-rail rows and a 32-id block for the immediate-mode action pills, then construct the stateful fluor widgets (dropdown / slider / textbox) and the custom checkboxes. All get placeholder geometry; `update_widget_layout` repositions the ones on the active page each frame.
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.settings_nav_base = self.hit_counter;
        self.hit_counter = self.hit_counter.wrapping_add(8); // rows 0..=8
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.settings_btn_base = self.hit_counter;
        self.hit_counter = self.hit_counter.wrapping_add(39); // pills 0..=39 — the Fleet page's fourth band (32+ Lock-out) lives at the top of the block
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.contact_panel_btn_base = self.hit_counter;
        self.hit_counter = self.hit_counter.wrapping_add(3); // contact-panel pills 0..=3 (0 = Boot)
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.contact_nav_base = self.hit_counter;
        self.hit_counter = self.hit_counter.wrapping_add(3); // contact-panel rail rows 0..=3
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.msg_hit_base = self.hit_counter;
        self.hit_counter = self.hit_counter.wrapping_add(63); // message rows 0..=63
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.msg_copy_id = self.hit_counter;
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.msg_action_base = self.hit_counter;
        self.hit_counter = self.hit_counter.wrapping_add(8); // reply/edit/resend/delete + room
        self.react_strip_base = self.hit_counter;
        self.hit_counter = self.hit_counter.wrapping_add(10); // reaction glyph pills 0..=8 + the "+" (custom) at 9
        self.settings_theme_dropdown = Some(fluor::widgets::Dropdown::new(
            &mut self.hit_counter,
            0.,
            0.,
            1.,
            1.,
            12.,
            vec!["Dark chrome".to_string(), "Light chrome".to_string()],
        ));
        self.settings_zoom_slider = Some(fluor::widgets::Slider::new(
            &mut self.hit_counter,
            0.,
            0.,
            1.,
            1.,
            0.5,
        ));
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.unattended_confirm_base = self.hit_counter;
        self.hit_counter = self.hit_counter.wrapping_add(2); // confirm / cancel
        self.locked_retry_hit = self.hit_counter;
        self.hit_counter = self.hit_counter.wrapping_add(1);
        // Call controls (docs/calls.md) — retained Buttons with placeholder geometry; real rect/label/font-size land each frame in the render overlay block (phase-dependent). Registered cross-screen in `visit_app_widgets`, so hover/press/dispatch ride the same walk as every other Button. Construction order fixes the contiguous-id contract: status / start / action / decline. "Open Sans" matches the old hand-rolled pills' face.
        self.call_status_btn = Some(Button::new(&mut self.hit_counter, 0., 0., 1., 1., 12., ""));
        self.call_start_btn = Some(Button::new(
            &mut self.hit_counter,
            0.,
            0.,
            1.,
            1.,
            12.,
            "\u{260E} Call",
        ));
        self.call_action_btn = Some(Button::new(&mut self.hit_counter, 0., 0., 1., 1., 12., ""));
        self.call_decline_btn = Some(Button::new(&mut self.hit_counter, 0., 0., 1., 1., 12., ""));
        self.call_speaker_btn = Some(Button::new(&mut self.hit_counter, 0., 0., 1., 1., 12., "Speaker"));
        self.call_addhandle_btn = Some(Button::new(&mut self.hit_counter, 0., 0., 1., 1., 12., "Add handle"));
        self.call_back_btn = Some(Button::new(&mut self.hit_counter, 0., 0., 1., 1., 12., "\u{2039} Contact"));
        self.call_play_btn = Some(Button::new(&mut self.hit_counter, 0., 0., 1., 1., 12., "\u{25B6} Play"));
        for b in [
            self.call_status_btn.as_mut(),
            self.call_start_btn.as_mut(),
            self.call_action_btn.as_mut(),
            self.call_decline_btn.as_mut(),
            self.call_speaker_btn.as_mut(),
            self.call_addhandle_btn.as_mut(),
            self.call_back_btn.as_mut(),
            self.call_play_btn.as_mut(),
        ]
        .into_iter()
        .flatten()
        {
            b.set_font_family("Open Sans");
        }
        self.settings_custodian_check = Some(fluor::widgets::Checkbox::new(
            &mut self.hit_counter,
            "Be a custodian for others",
            0.,
            0.,
            1.,
            1.,
            12.,
            false,
        ));
        self.settings_chime_check = Some(fluor::widgets::Checkbox::new(
            &mut self.hit_counter,
            "Chime on new message",
            0.,
            0.,
            1.,
            1.,
            12.,
            true,
        ));
        // Dozenal is the house base — default ON; the About-page toggle flips to decimal for the arabic-inclined. Initial state re-syncs from `display.dozenal` when fleet settings load.
        self.settings_dozenal_check = Some(fluor::widgets::Checkbox::new(
            &mut self.hit_counter,
            "Dozenal numbers (base twelve)",
            0.,
            0.,
            1.,
            1.,
            12.,
            true,
        ));
        // The message-vibrate + call-alert pair (defaults ON — a phone that neither rings nor buzzes is a missed call). Android enforcement rides Kotlin; the settings persist fleet-wide now.
        self.settings_vibrate_msg_check = Some(fluor::widgets::Checkbox::new(
            &mut self.hit_counter,
            "Vibrate on new message",
            0.,
            0.,
            1.,
            1.,
            12.,
            true,
        ));
        self.settings_ring_call_check = Some(fluor::widgets::Checkbox::new(
            &mut self.hit_counter,
            "Ring on incoming call",
            0.,
            0.,
            1.,
            1.,
            12.,
            true,
        ));
        self.settings_vibrate_call_check = Some(fluor::widgets::Checkbox::new(
            &mut self.hit_counter,
            "Vibrate on incoming call",
            0.,
            0.,
            1.,
            1.,
            12.,
            true,
        ));
        // DEFAULTS OFF (user mandate): "presence" is the rich self-disclosure broadcast (busy, now-playing, mood) — NOT the online indicator, which is the avatar ring and is never gated by this. Deliberate disclosure is opt-in.
        self.settings_presence_check = Some(fluor::widgets::Checkbox::new(
            &mut self.hit_counter,
            "Show my presence to contacts",
            0.,
            0.,
            1.,
            1.,
            12.,
            false,
        ));
        self.settings_autoupdate_check = Some(fluor::widgets::Checkbox::new(
            &mut self.hit_counter,
            // Platform-honest: desktop release builds self-apply + re-exec, so "install" is literal. Android auto-CHECKS and notifies but deliberately doesn't auto-DOWNLOAD the APK in the background (metered-data safety — the tap-to-install then rides the unattended session installer, silent after the one-time confirm), so the label there says "check", not "install".
            if cfg!(target_os = "android") {
                "Check for updates automatically"
            } else {
                "Install updates automatically"
            },
            0.,
            0.,
            1.,
            1.,
            12.,
            true,
        ));
        // Hard logs default OFF: steady-state logging batches in RAM and reaches disk on edges only (wear); tick it while chasing a crash on this device — see lib.rs LOG_HARD.
        self.settings_hardlogs_check = Some(fluor::widgets::Checkbox::new(
            &mut self.hit_counter,
            "Hard logs — this device, 24h (write every line to disk)",
            0.,
            0.,
            1.,
            1.,
            12.,
            false,
        ));
        // Desktop only: Android's lifecycle is the OS's business (foreground service + FCM), so no toggle there.
        #[cfg(not(target_os = "android"))]
        {
            self.settings_background_check = Some(fluor::widgets::Checkbox::new(
                &mut self.hit_counter,
                "Run in background (start at login, keep running when closed)",
                0.,
                0.,
                1.,
                1.,
                12.,
                crate::platform::autostart::background_desired(),
            ));
        }
        // Security page: DANGEROUS auto-attest-on-reboot toggle. Off unless the operator opted a failsafe box in.
        self.settings_unattended_check = Some(fluor::widgets::Checkbox::new(
            &mut self.hit_counter,
            "Auto-attest on reboot (unattended)",
            0.,
            0.,
            1.,
            1.,
            12.,
            Self::unattended_enabled(),
        ));
        self.settings_note_textbox = Some(Textbox::new(&mut self.hit_counter, 0., 0., 1., 1., 12.));
        self.you_add_textbox = Some(Textbox::new(&mut self.hit_counter, 0., 0., 1., 1., 12.));
        // Unattended-confirm handle box: built once here so its hit_id is stable (lazy creation at open time bumped hit_counter every open, drifting the id out from under the render's stamp — the box then took no input).
        self.unattended_confirm_tb = Some(Textbox::new(&mut self.hit_counter, 0., 0., 1., 1., 12.));
        // The per-field boxes are built lazily on first You-page open (build_you_fields) — HitId is a u16, so we allocate the ~32 field ids only when the page is actually visited.

        self.update_widget_layout(ctx);

        // HandleQuery: device keypair is derived deterministically from the machine fingerprint (NEVER stored to disk — same machine yields the same keypair so attestations are reproducible across restarts). HandleQuery owns the UDP socket + sends/receives FGTW packets; an empty PeerStore wires the transport so query packets have somewhere to fan out to. The proxy expect is structurally safe: fluor's host calls `set_event_proxy` BEFORE `init` (see `run_app` in fluor/src/host/app.rs), so `event_proxy` is always `Some` here.
        let proxy = self
            .event_proxy
            .as_ref()
            .expect("event_proxy must be set before init (host contract)");
        // Prefer an externally-injected keypair (Android: PhotonContext sets it from NetworkContext before AndroidShell::new calls init). Fall back to deriving from the OS machine fingerprint — desktop reads /etc/machine-id etc., Android has no in-Rust fallback (Build.FINGERPRINT lives Java-side) so a missing keypair there is a panic-worthy programmer error: shipping a zero-derived keypair would silently downgrade every cryptographic identity in the app.
        let keypair = match self.device_keypair.take() {
            Some(kp) => kp,
            None => {
                #[cfg(not(target_os = "android"))]
                {
                    let fingerprint = get_machine_fingerprint()
                        .expect("device-key derivation: machine fingerprint unavailable");
                    crate::network::fgtw::derive_device_keypair(&fingerprint)
                }
                #[cfg(target_os = "android")]
                {
                    panic!(
                        "PhotonApp::set_device_keypair must be called before init on Android — \
                         the JNI shim wires thru the keypair derived from the OS fingerprint \
                         in PhotonConnectionService; a missing keypair here means the wiring was \
                         skipped and would produce a zeroed/insecure key derivation"
                    );
                }
            }
        };
        // Stash a clone for app-level operations that need the keypair after init (avatar upload via `upload_avatar`). The clone is cheap (Ed25519 keypair is ~64 bytes); we can't ask HandleQuery for it back because its constructor moves the keypair into the worker threads.
        self.device_keypair = Some(keypair.clone());
        // Hand the device secret to storage so the pre-identity device vault (D2 binding, opt-in flags, reboot capsule) resolves from here on — on Android this is the ONLY route (no in-Rust fingerprint oracle).
        crate::storage::install_device_secret(*keypair.secret.as_bytes());
        #[cfg(not(target_os = "android"))]
        let hq = HandleQuery::new(keypair, proxy.clone());
        #[cfg(target_os = "android")]
        let hq = {
            let _ = proxy;
            HandleQuery::new(keypair)
        };
        // ONE shared peer store: HandleQuery populates it from fgtw fetches, the status receiver serves/merges phonebook-gossip records into it, and the app harvests learned addresses from it for stalled contacts. All three hold clones of the same Arc.
        let peer_store = Arc::new(Mutex::new(PeerStore::new()));
        self.peer_store = Some(peer_store.clone());
        hq.set_transport(peer_store.clone());

        // Wire the CLUTCH job channels (replace the disconnected placeholders from `new`).
        {
            let (ktx, krx) = std::sync::mpsc::channel();
            self.clutch_keygen_tx = ktx;
            self.clutch_keygen_rx = krx;
            let (etx, erx) = std::sync::mpsc::channel();
            self.clutch_kem_encap_tx = etx;
            self.clutch_kem_encap_rx = erx;
            let (ctx_, crx) = std::sync::mpsc::channel();
            self.clutch_ceremony_tx = ctx_;
            self.clutch_ceremony_rx = crx;
            let (dtx, drx) = std::sync::mpsc::channel();
            self.clutch_kem_decap_tx = dtx;
            self.clutch_kem_decap_rx = drx;
            let (atx, arx) = std::sync::mpsc::channel();
            self.avatar_dl_tx = atx;
            self.avatar_dl_rx = arx;
            let (aitx, airx) = std::sync::mpsc::channel();
            self.attach_installed_tx = aitx;
            self.attach_installed_rx = airx;
            let (hptx, hprx) = std::sync::mpsc::channel();
            self.hist_opened_tx = hptx;
            self.hist_opened_rx = hprx;
            let (cstx, csrx) = std::sync::mpsc::channel();
            self.chain_sync_opened_tx = cstx;
            self.chain_sync_opened_rx = csrx;
            let (brtx, brrx) = std::sync::mpsc::channel();
            self.braid_rx_tx = brtx;
            self.braid_rx_rx = brrx;
            self.chat_replay_queue.clear();
            let (bttx, btrx) = std::sync::mpsc::channel();
            self.braid_tx_tx = bttx;
            self.braid_tx_rx = btrx;
            self.send_encrypt_busy.clear();
            let (frtx, frrx) = std::sync::mpsc::channel();
            self.fleet_rotated_tx = frtx;
            self.fleet_rotated_rx = frrx;
            let (cctx, ccrx) = std::sync::mpsc::channel();
            self.clock_check_tx = cctx;
            self.clock_check_rx = ccrx;
            let (ictx, icrx) = std::sync::mpsc::channel();
            self.inbox_check_tx = ictx;
            self.inbox_check_rx = icrx;
        }

        // One-shot wall-clock sanity check via nunc-time, a few seconds behind attest (off-thread, so the several-seconds consensus query never blocks the UI). Warns via banner if the system clock is grossly wrong — never corrects it. Mid-session re-checks fire from the jump detector in `update`. On Android the wake handle is `None` (redraws come thru the JNI/Choreographer path); the result is drained on a subsequent tick.
        #[cfg(not(target_os = "android"))]
        crate::network::spawn_clock_check(self.clock_check_tx.clone(), Some(proxy.clone()));
        #[cfg(target_os = "android")]
        crate::network::spawn_clock_check(self.clock_check_tx.clone(), None);

        // One-shot fleet-inbox drain: pull any worker-observed alerts (bind attempts on our devices). Off-thread — a blocking HTTPS round trip — with the verdict drained on a later tick.
        self.spawn_inbox_drain();

        // Spawn the presence + CLUTCH status checker on HandleQuery's shared socket. Done BEFORE `hq` is moved into the field so we can take its socket. Without this the UDP recv/pong worker never runs — the socket is bound but nothing reads it or replies, so the device is invisible to every peer (no presence, no CLUTCH). The desktop and Android constructors differ only in the wake sender: desktop passes the winit event proxy; Android's redraws come thru the JNI/Choreographer path so its constructor takes none.
        #[cfg(not(target_os = "android"))]
        let checker_result = crate::network::status::StatusChecker::new(
            hq.socket(),
            self.device_keypair
                .clone()
                .expect("device_keypair set above"),
            self.contact_pubkeys.clone(),
            self.sync_records.clone(),
            self.pong_seal_keys.clone(),
            proxy.clone(),
            peer_store.clone(),
        );
        #[cfg(target_os = "android")]
        let checker_result = crate::network::status::StatusChecker::new(
            hq.socket(),
            self.device_keypair
                .clone()
                .expect("device_keypair set above"),
            self.contact_pubkeys.clone(),
            self.sync_records.clone(),
            self.pong_seal_keys.clone(),
            peer_store.clone(),
        );
        match checker_result {
            Ok(c) => {
                self.status_checker = Some(c);
                crate::log("UI: status checker started (presence + CLUTCH)");
            }
            Err(e) => crate::logf!("UI: status checker failed to start: {}", e),
        }

        self.handle_query = Some(hq);

        // UNATTENDED MODE (off by default, Security → "Auto-attest on reboot"): the boot-locked tohu session dies on reboot BY DESIGN, so a normal reboot lands on the typed-attest screen. When the operator has explicitly opted a failsafe box into unattended mode, a device-bound reboot capsule (sealed under the hardware fingerprint, not the wairua) survives the reboot — adopt it into tohu's live session here so the identical resume path below runs with no handle typed. The capsule opens ONLY on the same hardware; a copy elsewhere fails. If tohu already has a live session (warm restart, same boot) this is a no-op.
        if tohu::session().is_none() {
            if let Some(cap) = crate::storage::device_vault()
                .and_then(|v| v.read_device(Self::REBOOT_CAPSULE_ENTRY).ok().flatten())
                .and_then(|bytes| tohu::open_reboot_capsule(&bytes))
            {
                crate::log("RESUME: unattended reboot capsule opened — auto-attesting with no handle (Security toggle is ON)");
                let _ = tohu::set_session(&cap); // re-arm the normal (boot-locked) session so the rest of this boot behaves like a warm restart
            }
        }

        // Auto-resume from the remembered session roots. If tohu has this login's roots (persisted on a prior, FGTW-confirmed attest), paint Ready IMMEDIATELY from local state — we already own this identity, so there is no reason to block the first frame on the network. The avatar comes from a local cache file (no vault, no network); contacts + peer presence + cloud-merge arrive a beat later via the background `query_resume` and merge in thru `on_query_result`. A rejection (handle claimed by another device) bails back to the attest screen; a transient network error leaves the local session on Ready untouched. None (first run / post-logout) falls thru to the normal typed-attest flow.
        if let Some(remembered) = tohu::session() {
            // Blob name key + the one-time v0→v1 filename re-key walk — BEFORE the first Ready frame, so render-path blob_present reads work immediately (and the plaintext-hash possession oracle is closed the moment a seed exists).
            crate::storage::blob_init_names(&remembered.identity_seed);
            self.session = Some(remembered);
            self.hints_dismissed = false; // fresh Ready entry → the avatar prompt gets a chance until first interaction
                                          // Initialize local storage and load contacts immediately so the contact list is visible before the FGTW round-trip completes.
            if let Some(kp) = &self.device_keypair {
                let device_secret = *kp.secret.as_bytes();
                // open_session_vault = the ONE device vault via the shared registry: query_resume below spawns the attest worker, which opens this same vault — a second independent engine racing this one is how the vault corruption happened (stale engine committed over the live one's blocks → seal verification failed at every subsequent open).
                // Phase-timed (the PERF summary below): everything in this arm runs on the UI thread BEFORE the first Ready frame, and the field measured ~1.2s of it with no line naming the eater — the timers make the next boot log ground truth.
                let t_boot = std::time::Instant::now();
                let opened = crate::storage::open_session_vault(
                    remembered.identity_seed,
                    remembered.vault_seed,
                    device_secret,
                );
                let ms_vault = t_boot.elapsed().as_millis();
                match opened {
                    Ok(s) => {
                        // Preserve any IN-FLIGHT ceremony round across this reload. CLUTCH keypairs/slots are ephemeral scratch, so a wholesale reload from disk wipes a live round — and a warm resume (Android foregrounds constantly) then trips the keygen sweep into minting a DIVERGENT round the peer never agreed to. That is exactly what stranded the relay ceremony: the slow relay round-trip outlived the keys, the peer's KEM came back addressed to keys we'd already discarded, and it was dropped as "old keys". Re-key must be deliberate on real failure — never a side effect of a lifecycle event. Snapshot rounds that are still FRESH by eagle time (a genuinely stale one is let go, to be re-keyed cleanly) and restore them after the reload.
                        let now = vsf::eagle_time_oscillations();
                        let inflight: std::collections::HashMap<[u8; 32], _> = self
                            .contacts
                            .iter()
                            .filter(|c| {
                                c.clutch_our_keypairs.is_some()
                                    && c.clutch_round_started
                                        .map_or(false, |t| now - t < CLUTCH_ROUND_TTL_OSC)
                            })
                            .map(|c| {
                                (
                                    c.handle_hash,
                                    (
                                        c.clutch_our_keypairs.clone(),
                                        c.clutch_slots.clone(),
                                        c.offer_provenances.clone(),
                                        c.ceremony_id,
                                        c.clutch_round_started,
                                        c.clutch_offer_sent,
                                        c.clutch_pending_kem.clone(),
                                        c.clutch_state,
                                    ),
                                )
                            })
                            .collect();
                        let t_phase = std::time::Instant::now();
                        self.contacts = crate::storage::contacts::load_all_contacts(&s);
                        self.apply_locked_set();
                        let ms_contacts = t_phase.elapsed().as_millis();
                        for c in self.contacts.iter_mut() {
                            if let Some((
                                kp,
                                slots,
                                provs,
                                cid,
                                started,
                                offer_sent,
                                pending_kem,
                                state,
                            )) = inflight.get(&c.handle_hash)
                            {
                                c.clutch_our_keypairs = kp.clone();
                                c.clutch_slots = slots.clone();
                                c.offer_provenances = provs.clone();
                                c.ceremony_id = *cid;
                                c.clutch_round_started = *started;
                                c.clutch_offer_sent = *offer_sent;
                                c.clutch_pending_kem = pending_kem.clone();
                                // Keep a mid-ceremony state alive — never downgrade a live AwaitingProof to disk's stale Pending. A persisted Complete on disk wins (the round already sealed).
                                if !matches!(c.clutch_state, crate::types::ClutchState::Complete) {
                                    c.clutch_state = *state;
                                }
                                crate::logf!("CLUTCH: preserved in-flight round for {} across resume (no willy-nilly re-key)", crate::fp(&c.handle_proof));
                            }
                        }
                        // Fleet siblings load from their own index (they never enter the contacts index).
                        {
                            let siblings = crate::storage::contacts::load_all_siblings(
                                remembered.handle_proof,
                                &s,
                            );
                            if !siblings.is_empty() {
                                crate::logf!(
                                    "SIBLING: loaded {} sibling(s) from local vault on resume",
                                    siblings.len()
                                );
                            }
                            self.contacts.extend(siblings);
                        }
                        // Load each contact's conversation too — load_all_contacts only loads per-peer contact STATE from the vault, not the messages (those live in the rārangi DB, loaded separately). Without this the resume frame paints contacts with empty message lists, and the later query_resume result can't fix it: on_query_result merges by handle_proof and SKIPS already-loaded contacts as duplicates, so the message-bearing copy is discarded → history looks wiped until the next app launch. Loading here makes resume show full history at once.
                        let t_phase = std::time::Instant::now();
                        for ci in 0..self.contacts.len() {
                            let (proof, key) = (
                                self.contacts[ci].handle_proof,
                                self.contacts[ci].handle_hash,
                            );
                            let Some(conv) = self.conv_mut_of(ci) else {
                                continue;
                            };
                            crate::storage::contacts::load_conversation_state(conv, &key, &s);
                            if let Err(e) = crate::storage::contacts::load_messages(conv, &s) {
                                crate::logf!(
                                    "UI: resume failed to load messages for {}: {}",
                                    crate::fp(&proof).as_str(),
                                    e
                                );
                            }
                        }
                        let ms_messages = t_phase.elapsed().as_millis();
                        crate::logf!(
                            "UI: loaded {} contact(s) from local vault on resume",
                            self.contacts.len()
                        );
                        // STORAGE CENSUS (field diagnosis, 2026-08-10): one line per contact naming the conversation table and how many rows actually loaded from it. The "messages don't recover" round showed devices advertising 92 rows while serving 3 — the row sets had split across contact keys, and nothing in the logs could say WHICH table held what. This makes the next log round ground truth. Delete once the split-conversation incident is closed.
                        {
                            let mut census: Vec<(String, [u8; 32], usize, String)> = Vec::new();
                            for ci in 0..self.contacts.len() {
                                let c = &self.contacts[ci];
                                let fp = crate::fp(&c.handle_proof);
                                // The row's full identity beside its table: handle_hash names the party id the tokens/tables derive from, first-met names the pinned device, state names the ceremony posture. The 2026-08-12 evening round proved fp+table alone can't distinguish a stale-keyed self row / debris row / sibling from the outside — this makes the next round ground truth without another guessing session.
                                let detail = format!(
                                    " [id {} hash {} first-met {} {:?}{}]",
                                    hex::encode(&c.id.as_bytes()[..4]),
                                    hex::encode(&c.handle_hash[..4]),
                                    hex::encode(&c.device_key().unwrap_or_default()[..4]),
                                    c.clutch_state,
                                    if c.is_sibling { " sibling" } else { "" }
                                );
                                let Some(conv) = self.conv_of(ci) else {
                                    continue;
                                };
                                census.push((
                                    fp,
                                    *conv.id().as_bytes(),
                                    conv.messages.len(),
                                    detail,
                                ));
                            }
                            for (fp, table, rows, detail) in &census {
                                crate::logf!("STORAGE: census — {} table {} holds {} row(s) in RAM after load{}", fp, hex::encode(&table[..4]), rows, detail);
                            }
                            // The reference values the census hashes are read against — without these in the same log, "is that hash our pid or debris?" needs a second device round-trip.
                            if let (Some(sess), Some(sib_pid)) =
                                (self.session.as_ref(), self.our_sibling_pid())
                            {
                                let our_pid =
                                    crate::crypto::clutch::identity_party_id(&sess.identity_seed);
                                crate::logf!(
                                    "STORAGE: census — OUR ids: identity pid {}, this device's sibling pid {}",
                                    hex::encode(&our_pid[..4]),
                                    hex::encode(&sib_pid[..4])
                                );
                            }
                            // Split-identity detector: two FRIEND rows sharing a handle_proof but keyed by different conversation tables IS the duplicated-contact state (one row per identity is the invariant). SIBLINGS are excluded: the whole fleet shares our handle_proof with a per-device hash BY DESIGN — the first census run flagged ordinary sibling pairs as "duplicates" for two log rounds (2026-08-11). Loud, because every downstream system — recovery walks, digest records, ceremonies — silently picks whichever row it finds first.
                            for i in 0..self.contacts.len() {
                                for j in (i + 1)..self.contacts.len() {
                                    if !self.contacts[i].is_sibling
                                        && !self.contacts[j].is_sibling
                                        && self.contacts[i].handle_proof
                                            == self.contacts[j].handle_proof
                                        && self.contacts[i].handle_hash
                                            != self.contacts[j].handle_hash
                                    {
                                        crate::logf!("STORAGE: census — DUPLICATE CONTACT for {}: two rows with different conversation keys ({}… vs {}…) — conversations are SPLIT across them", crate::fp(&self.contacts[i].handle_proof), hex::encode(&self.contacts[i].handle_hash[..4]), hex::encode(&self.contacts[j].handle_hash[..4]));
                                    }
                                }
                            }
                            // SELF-STUB PURGE: a non-sibling row carrying OUR handle_proof under a handle_hash that is NOT our identity pid is a corrupt self-contact stub — the source of the "CLUTCH offers toward its OWN identity" storm (ticketed 2026-08-07; the census caught THREE self rows on one desktop, 2026-08-11, two of them empty stubs spraying 573KB offers at the fleet's own contacts). Empty-conversation stubs only: a row that somehow holds messages is somebody's data and stays for a deliberate repair, never a boot-time sweep.
                            if let Some((our_proof, our_seed)) = self
                                .session
                                .as_ref()
                                .map(|s| (s.handle_proof, s.identity_seed))
                            {
                                let our_pid = crate::crypto::clutch::identity_party_id(&our_seed);
                                let stub_hashes: Vec<[u8; 32]> = self
                                    .contacts
                                    .iter()
                                    .enumerate()
                                    .filter(|(ci, c)| {
                                        !c.is_sibling
                                            && c.handle_proof == our_proof
                                            && c.handle_hash != our_pid
                                            && self
                                                .conv_of(*ci)
                                                .map_or(true, |v| v.messages.is_empty())
                                    })
                                    .map(|(_, c)| c.handle_hash)
                                    .collect();
                                if !stub_hashes.is_empty() {
                                    for hh in &stub_hashes {
                                        crate::logf!("STORAGE: census — PURGING empty self-contact stub (key {}…) — its ceremony queue dies with it", hex::encode(&hh[..4]));
                                        let _ = crate::storage::contacts::delete_contact(hh, &s);
                                    }
                                    self.contacts
                                        .retain(|c| !stub_hashes.contains(&c.handle_hash));
                                    // Rewrite the index too, or the next launch resurrects the stubs from the list (same rule as the ostracism path).
                                    let index: Vec<crate::storage::contacts::ContactIdentity> =
                                        self.contacts
                                            .iter()
                                            .filter(|c| !c.is_sibling)
                                            .map(|c| crate::storage::contacts::ContactIdentity {
                                                handle_proof: c.handle_proof,
                                                party_id: c.handle_hash,
                                                avatar_pin: c.avatar_pin,
                                            })
                                            .collect();
                                    if let Err(e) =
                                        crate::storage::contacts::save_contact_list(&index, &s)
                                    {
                                        crate::logf!(
                                            "STORAGE: census — stub-purge index rewrite failed: {}",
                                            e
                                        );
                                    }
                                }
                            }
                        }
                        // Load friendship chains NOW too, not just contacts. Resume paints Ready and the status checker starts answering immediately, but chains used to arrive only later via query_resume — so any chat that landed in that window hit "No friendship found for conversation_token" and was DROPPED (no chain = no decrypt, no buffer). Loading chains here closes that gap so a peer messaging us the instant we come back online doesn't lose messages. query_resume still merges (and won't clobber these — it only adds ids we don't already hold).
                        let t_phase = std::time::Instant::now();
                        let friendship_ids: Vec<crate::types::FriendshipId> = self
                            .contacts
                            .iter()
                            .filter_map(|c| c.friendship_id)
                            .collect();
                        let loaded_chains =
                            crate::storage::friendship::load_all_friendships(&friendship_ids, &s);
                        for (fid, chains) in loaded_chains {
                            if !self.friendship_chains.iter().any(|(id, _)| *id == fid) {
                                self.friendship_chains.push((fid, chains));
                            }
                        }
                        // Anything the loader REJECTED (pre-v8 blobs, the lanes flag-day) leaves its contact keyed-but-chainless — reset those ceremonies now, while every chain that CAN load already has.
                        self.reclutch_chainless_contacts("resume load");
                        self.update_sync_records();
                        // Seed the checker's answerable-pubkey set with every loaded contact's FULL fleet so pongs/offers from any of their devices are honoured.
                        self.reseed_contact_pubkeys();
                        // Wake-up catch-up: re-fold each contact's fleet so a friend's device added while we were off is honoured now, not next launch. Our OWN hp is included explicitly — the drain routes it to sibling reconcile (fleet weave), so a freshly-joined device discovers its siblings on first resume even with an empty contact list.
                        let mut hps: Vec<[u8; 32]> = self
                            .contacts
                            .iter()
                            .filter(|c| !c.is_sibling)
                            .map(|c| c.handle_proof)
                            .collect();
                        hps.push(remembered.handle_proof);
                        hps.sort_unstable();
                        hps.dedup();
                        self.spawn_contact_fleet_refresh(hps);
                        let ms_chains = t_phase.elapsed().as_millis();
                        // Rehydrate each contact's saved ephemeral keypairs from disk (~588KB each). load_contact_state deliberately doesn't pull these (they're huge and live in a separate vault key), so without this every resume re-runs the McEliece-heavy keygen below — which is what froze the UI on launch. Loading the persisted keypairs makes the re-key filter a no-op for contacts that already have them, so keygen only fires for genuinely keyless Pending ones.
                        // Complete contacts are SKIPPED: their ceremony sealed, the chains carry the conversation, and every live path that could want keys again (peer-lost-chains re-key, reclutch) mints a fresh round anyway — so ~588KB per settled contact was pure frame-one tax (the bulk of the field's 1.2s resume, 2026-08-12).
                        let t_phase = std::time::Instant::now();
                        let mut rehydrated = 0usize;
                        for contact in self.contacts.iter_mut() {
                            if contact.clutch_our_keypairs.is_none()
                                && contact.clutch_state != crate::types::ClutchState::Complete
                            {
                                match crate::storage::contacts::load_clutch_keypairs(
                                    &contact.handle_hash,
                                    &s,
                                ) {
                                    Ok(Some(keypairs)) => {
                                        contact.clutch_our_keypairs = Some(keypairs);
                                        rehydrated += 1;
                                    }
                                    Ok(None) => {}
                                    Err(e) => crate::logf!(
                                        "CLUTCH: failed to rehydrate keypairs for {}: {}",
                                        crate::fp(&contact.handle_proof),
                                        e
                                    ),
                                }
                            }
                        }
                        let ms_keypairs = t_phase.elapsed().as_millis();
                        self.storage = Some(s);
                        // Frame one owns the persisted ergonomics: fleet_settings live on disk, but nothing read them at boot — every ensure_fleet_settings caller was a user action or the network merge drain, so the zoom restore waited ~5s for the first fleet pull (and forever offline), painting every launch at default scale (Nick, 2026-08-12).
                        let t_phase = std::time::Instant::now();
                        self.ensure_fleet_settings();
                        let ms_settings = t_phase.elapsed().as_millis();
                        // This device's avatar: ONE cheap vault read decides recovery now; the heavy VSF-parse + AV1 decode runs off-thread and installs thru the avatar drain (owner: None), so frame one never waits on dav1d.
                        let own_avatar_bytes = self.storage.as_ref().and_then(|storage| {
                            storage
                                .read_addr(&crate::storage::vault_key(
                                    "avatar",
                                    &remembered.identity_seed,
                                ))
                                .ok()
                                .flatten()
                        });
                        match own_avatar_bytes {
                            Some(bytes) => {
                                let seed = remembered.identity_seed;
                                let tx = self.avatar_dl_tx.clone();
                                std::thread::spawn(move || {
                                    let pixels =
                                        crate::ui::avatar::load_avatar_from_bytes_from_seed(
                                            &bytes, &seed,
                                        )
                                        .map(|(_, px)| px);
                                    let _ = tx.send(crate::ui::avatar::AvatarDownloadResult {
                                        owner: None,
                                        pixels,
                                        name: None, // seed-keyed local decode — the name lives in fstate on this path
                                    });
                                });
                                // Decode failure (poisoned bytes) arms the FGTW recovery in the drain — never here, or the tick would race the in-flight decode into a pointless network fetch every boot.
                            }
                            // Local vault had no avatar (e.g. this device was cleared) — recover our own from FGTW, where it was published. Off-thread; installs via the avatar drain.
                            None => {
                                if !self.spawn_self_avatar_recover(remembered.identity_seed) {
                                    // No pin at rest yet (settings still loading) — the tick retries once one lands.
                                    self.self_avatar_recover_pending =
                                        Some(remembered.identity_seed);
                                }
                            }
                        }
                        // Notes-to-self is NOT bootstrapped (Nick 2026-08-01): an empty conversation with yourself is not a contact you asked for, and it sat at the top of the list looking broken because it has no peer to pong a name or avatar. Add yourself deliberately and it appears; until then the list holds only people you chose.
                        self.settle_self_display();
                        self.scrub_zero_remote_rounds();
                        // Re-key Pending contacts that still lack keypairs after the rehydrate — but ONE AT A TIME (spawn_next_pending_keygen, repeated each tick), never all at once: parallel McEliece keygens on launch starved the UI thread.
                        self.spawn_next_pending_keygen();
                        crate::logf!(
                            "PERF: resume load — vault {}ms, contacts {}ms, messages {}ms, chains {}ms, keypairs {}ms ({} rehydrated), settings {}ms → local Ready in {}ms (UI thread)",
                            ms_vault, ms_contacts, ms_messages, ms_chains, ms_keypairs, rehydrated, ms_settings, t_boot.elapsed().as_millis()
                        );
                    }
                    Err(e) => {
                        crate::logf!("STORAGE: init failed on resume: {}", e);
                        // A hard vault-open failure (e.g. seal verification failed) is the WORST storage state — no contacts load and nothing persists — yet it previously showed no warning, while a mere recoverable mirror-divergence (`degraded()`) did. Flag it so the red "storage degraded" banner surfaces a fully-broken vault too.
                        self.vault_degraded = true;
                    }
                }
            }
            self.state = AppState::Ready;
            if let Some(hq) = self.handle_query.as_ref() {
                crate::log("UI: resumed to Ready from local session roots (tohu) — FGTW announce + presence run in background");
                hq.query_resume(remembered);
            }
            // Kick presence immediately for the just-loaded contacts so their online rings reflect reality without waiting for the FGTW round-trip.
            self.ping_contacts();
        }
    }

    fn on_resize(&mut self, _width: u32, _height: u32, ctx: &mut Context) {
        if let Some(chrome) = self.chrome.as_mut() {
            // Use `ctx.viewport` directly — it carries the current `ru` (zoom factor) that fluor's host has already updated from Ctrl/Cmd +/-/0/scroll. Building a fresh `Viewport::new(w, h)` here would reset ru to 1.0 every resize/zoom event and silently strip the user's zoom state. Width/height are redundant with `ctx.viewport.{width_px, height_px}` for the same reason.
            chrome.resize(ctx.viewport);
            // Maximize toggles always change size between user-sized and screen-sized, so on_resize is the natural sync point for full_edge mode (no perimeter hairline / corner cutout / shadow when the window fills the screen). On Android the surface is always fullscreen — soft-keyboard show/hide triggers an on_resize too, and ctx.is_maximized is hard-coded false there, so without this override the perimeter + corner cutout would re-appear every time the IME opens.
            #[cfg(target_os = "android")]
            chrome.set_full_edge(true);
            #[cfg(not(target_os = "android"))]
            chrome.set_full_edge(ctx.is_maximized);
        }
        self.update_widget_layout(ctx);
    }

    // A clickable element was ACTIVATED — pointer went DOWN on `hit_id` and released over the SAME `hit_id`, no drag-off (press-hold-release, arbitrated by fluor's PointerArbiter). Every ACTION lives here so a mis-touch dragged off before release fires NOTHING. Press-time concerns (focus, textbox cursor, drag-select, window drag) stay in `on_event`'s Pressed arm; the raw press/release still arrive there.
    fn on_activate(
        &mut self,
        hit_id: HitId,
        x: Coord,
        y: Coord,
        mods: fluor::event::ModifiersState,
        ctx: &mut Context,
    ) -> EventResponse {
        // CALL overlay controls are now retained Buttons (call_action/decline/start) — their activation rides `dispatch_release` + the `take_click` poll in the Released and key paths (`dispatch_call_button_clicks`), NOT this hit-id branch. Nothing to do here.

        // Unattended-confirm (Security page) ARM/DISARM/cancel — dispatched HERE, at the top of on_activate, BEFORE any state gate. (These pills previously sat inside the `AppState::Conversation` block and so never fired on the Settings page — the arm click reached on_activate but was skipped.)
        if self.unattended_confirm.is_some() {
            if self.unattended_confirm_base != HIT_NONE && hit_id == self.unattended_confirm_base {
                let typed: String = self
                    .unattended_confirm_tb
                    .as_ref()
                    .map(|tb| tb.chars.iter().collect())
                    .unwrap_or_default();
                // Verify the re-typed handle by its ~1s MEMORY-HARD proof, NOT the microsecond identity seed — arming "this box becomes you" must not be a cheap brute-force oracle. The proof runs OFF the UI thread (spawned here, verdict drained in tick) so the app doesn't freeze for the second; re-clicks are ignored while one is in flight. The verdict compares against the session's stored handle_proof (the same public artifact the attest lock/unlock gates on).
                if self.unattended_verify.is_none() {
                    let target_on = self.unattended_confirm.unwrap_or(false);
                    let live_proof = self.session.as_ref().map(|s| s.handle_proof);
                    let (tx, rx) = std::sync::mpsc::channel();
                    let wake = self.event_proxy.clone();
                    std::thread::spawn(move || {
                        let ok = match live_proof {
                            Some(lp) => crate::types::Handle::username_to_handle_proof(&typed) == lp,
                            None => false,
                        };
                        let _ = tx.send(ok);
                        #[cfg(not(target_os = "android"))]
                        if let Some(w) = wake.as_ref() {
                            let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
                        }
                        #[cfg(target_os = "android")]
                        let _ = wake;
                    });
                    self.unattended_verify = Some((rx, target_on));
                    self.unattended_confirm_failed = false;
                }
                self.scene_dirty = true;
                ctx.window.request_redraw();
                return EventResponse::Handled;
            }
            if self.unattended_confirm_base != HIT_NONE
                && hit_id == self.unattended_confirm_base.wrapping_add(1)
            {
                self.unattended_confirm = None;
                self.unattended_confirm_failed = false;
                self.change_focus(None);
                self.scene_dirty = true;
                ctx.window.request_redraw();
                return EventResponse::Handled;
            }
        }
        // Avatar tap on Ready dispatches to the image picker — not a Widget, just a hit-stamp in chrome.hit_test_map. Drops focus first because the picker overlays the whole UI.
        if hit_id == self.avatar_hit_id
            && matches!(self.state, AppState::Ready)
            && self.avatar_hit_id != HIT_NONE
        {
            self.change_focus(None);
            // Android: a tap opens the system image picker directly (the picker IS the update mechanism — tapping the grey circle is self-evident, so no on-screen prompt). Desktop: no picker — the avatar updates by drag/drop — the tap is swallowed here.
            #[cfg(target_os = "android")]
            {
                self.pending_picker_request = true;
            }
            ctx.window.request_redraw();
            return EventResponse::Handled;
        }

        // "Copy words" on the JOIN words screen: space-separated (the AddDevice entry tokenizes either form; spaces read naturally in an email/messenger paste). The words are a short-lived pairing secret for OUR OWN fleet — sharing them over a channel the user trusts is their call; the bind still requires the sponsor device to confirm.
        if hit_id == self.join_copywords_hit_id && self.join_copywords_hit_id != HIT_NONE {
            if let Some(words) = self.add_join_words.clone() {
                // camelCase → space-separated, same split the words screen renders.
                let mut spaced = String::with_capacity(words.len() + 24);
                for c in words.chars() {
                    if c.is_ascii_uppercase() && !spaced.is_empty() {
                        spaced.push(' ');
                    }
                    spaced.push(c);
                }
                if self.copy_to_clipboard(&spaced) {
                    self.join_words_copied = true;
                }
            }
            ctx.window.request_redraw();
            return EventResponse::Handled;
        }

        // Retry pill on the Locked dead-end: the user claims a sibling has unlocked this device, so return to the normal bound-resume entry and let the standard attest re-ask the worker. The handle is typed only on that standard screen — the dead-end itself never invites input (a locked device must not prompt for the root secret).
        if hit_id == self.locked_retry_hit
            && self.locked_retry_hit != HIT_NONE
            && matches!(self.state, AppState::Launch(LaunchState::Locked))
        {
            crate::log(
                "LOCKED: user claims an unlock — returning to the resume screen for a fresh attest",
            );
            self.state = AppState::Launch(LaunchState::Fresh);
            self.refocus_handle_select_all();
            ctx.window.request_redraw();
            return EventResponse::Handled;
        }

        // "Start fresh (wipe this device)" on the JOIN words screen — a removed device's self-clean path. Two-tap confirm → full clean (nuke vault + clear session), leaving a blank slate ready to attest fresh or join another fleet.
        if hit_id == self.join_startfresh_hit_id && self.join_startfresh_hit_id != HIT_NONE {
            if self.join_startfresh_armed {
                self.join_startfresh_armed = false;
                self.end_add_device_flow(); // leave JOIN mode before wiping
                self.clean_device_for_reuse();
            } else {
                self.join_startfresh_armed = true;
            }
            ctx.window.request_redraw();
            return EventResponse::Handled;
        }

        // Green-confirm on the AddDevice screen: the two-phase press that releases the fleet-key rotation (only live while a bind awaits it).
        if hit_id == self.add_confirm_hit_id
            && self.add_confirm_hit_id != HIT_NONE
            && matches!(self.state, AppState::AddDevice)
        {
            self.spawn_confirm_add();
            ctx.window.request_redraw();
            return EventResponse::Handled;
        }

        // Candidate-row tap on the AddDevice screen (BLE / list select): bind the tapped device by its registry request (consent), then wait for the human's "did it turn green?" confirm (two-phase — a list pick isn't a typed-key match, so the key waits on visual confirmation).
        if self.add_candidate_hit_base != HIT_NONE
            && matches!(self.state, AppState::AddDevice)
            && self.add_device_bound.is_none()
            && !self.add_device_checking
            && hit_id >= self.add_candidate_hit_base
            && hit_id < self.add_candidate_hit_base.wrapping_add(7)
        {
            let idx = (hit_id - self.add_candidate_hit_base) as usize;
            // Filter identically to the render — only proximity-heard candidates are tap targets (a flooded registry never populates a tappable row).
            if let Some(cand) = self
                .add_device_candidates
                .iter()
                .filter(|c| c.heard_ble || c.heard_lan)
                .nth(idx)
            {
                let req = cand.req.clone();
                self.add_device_bind_ble = true;
                self.spawn_bind_device(req);
                ctx.window.request_redraw();
                return EventResponse::Handled;
            }
        }

        // Back button — Conversation and Add-device both return to the contact list; the contact panel returns to its conversation. Navigation is a dedicated control; the orb is settings-only.
        if hit_id == self.back_btn_hit_id && self.back_btn_hit_id != HIT_NONE {
            // Leaving a screen deselects whatever textbox held focus (clears its glow + selection) — page changes never carry focus across.
            self.change_focus(None);
            if matches!(self.state, AppState::ContactPanel(_)) {
                self.contact_boot_armed = false;
                self.state = AppState::Conversation;
                self.reset_contact_ping_backoff();
                // The conversation is the active view again — clear any unread that slipped in (no-op when already 0).
                if let Some(ci) = self.active_contact() {
                    self.clear_unread(ci);
                }
                ctx.window.request_redraw();
                return EventResponse::Handled;
            }
            if matches!(self.state, AppState::Conversation) {
                self.broadcast_focus_claim(false);
                self.state = AppState::Ready;
                self.active_conversation = None;
                ctx.window.request_redraw();
                return EventResponse::Handled;
            }
            if matches!(self.state, AppState::AddDevice) {
                // Cancel returns to the Fleet page the flow came from.
                self.end_add_device_flow();
                self.refresh_fleet_retired();
                self.state = AppState::Settings(SettingsPage::Fleet);
                ctx.window.request_redraw();
                return EventResponse::Handled;
            }
            if matches!(self.state, AppState::Settings(_)) {
                self.change_focus(None);
                // Back returns to the screen the orb opened settings FROM: Ready when attested, else the Launch/attest screen (pre-attest the orb opens About/Updates over Launch, so back must land there, not a Ready that doesn't exist yet).
                self.state = if self.session.is_some() {
                    AppState::Ready
                } else {
                    AppState::Launch(LaunchState::Fresh)
                };
                ctx.window.request_redraw();
                return EventResponse::Handled;
            }
        }

        // Contact panel: nav rail rows switch the page (settings-mirror), pills act (slot 0 = Boot).
        if matches!(self.state, AppState::ContactPanel(_)) {
            // Any press that isn't the Boot pill disarms it (event-shown, interaction-cleared).
            if self.contact_boot_armed && hit_id != self.contact_panel_btn_base {
                self.contact_boot_armed = false;
                ctx.window.request_redraw();
            }
            if self.contact_nav_base != HIT_NONE
                && hit_id >= self.contact_nav_base
                && hit_id < self.contact_nav_base.wrapping_add(4)
            {
                let idx = (hit_id - self.contact_nav_base) as usize;
                if let Some(p) = ContactPage::ALL.get(idx).copied() {
                    self.change_focus(None);
                    // Fresh page starts at the top, same rule as settings.
                    self.settings_content_scroll = 0.0;
                    self.state = AppState::ContactPanel(p);
                    ctx.window.request_redraw();
                }
                return EventResponse::Handled;
            }
            if self.contact_panel_btn_base != HIT_NONE
                && hit_id >= self.contact_panel_btn_base
                && hit_id < self.contact_panel_btn_base.wrapping_add(4)
            {
                let slot = hit_id - self.contact_panel_btn_base;
                if slot == 0 {
                    // Boot (two-tap): first press arms, second fires. Removal is unilateral and local-plus-fleet only — ostracism, not erasure.
                    if self.contact_boot_armed {
                        self.contact_boot_armed = false;
                        self.boot_active_contact();
                    } else {
                        self.contact_boot_armed = true;
                    }
                    self.scene_dirty = true;
                    ctx.window.request_redraw();
                }
                return EventResponse::Handled;
            }
        }

        if let AppState::Settings(page) = self.state {
            if self.settings_nav_base != HIT_NONE
                && hit_id >= self.settings_nav_base
                && hit_id < self.settings_nav_base.wrapping_add(9)
            {
                let idx = (hit_id - self.settings_nav_base) as usize;
                if let Some(p) = self.settings_pages().get(idx).copied() {
                    let p = &p;
                    self.change_focus(None);
                    // Leaving a page clears its selection/destructive-action arms (interaction-cleared).
                    if *p != SettingsPage::Fleet {
                        self.settings_fleet_selected = None;
                        self.fleet_release_armed = None;
                        self.fleet_lock_armed = None;
                    }
                    if *p != SettingsPage::Security {
                        self.settings_removeshred_armed = false;
                        self.settings_shred_armed = false;
                        self.settings_remove_armed = false;
                        // A stranded unattended-confirm modal must not survive navigating away.
                        self.unattended_confirm = None;
                        self.unattended_confirm_failed = false;
                    }
                    // Fresh page starts at the top — a leftover scroll from a longer page would strand a short one mid-air.
                    self.settings_content_scroll = 0.0;
                    // Opening the You page reloads its field boxes from the current settings (fleet-synced state).
                    if *p == SettingsPage::You {
                        self.you_fields_loaded = false;
                    }
                    // Opening the Updates page auto-checks BOTH channels so each button shows its target version + colour.
                    if *p == SettingsPage::Updates {
                        self.update_checked = false;
                        self.update_release = ChannelCheck::Idle;
                        self.update_dev = ChannelCheck::Idle;
                        self.check_update_channels();
                    }
                    // Opening the Fleet page refreshes the retired-device rows (chain history minus current members minus already-released brands).
                    if *p == SettingsPage::Fleet {
                        self.refresh_fleet_retired();
                    }
                    self.state = AppState::Settings(*p);
                    ctx.window.request_redraw();
                }
                return EventResponse::Handled;
            }
            if self.settings_btn_base != HIT_NONE
                && hit_id >= self.settings_btn_base
                // 56: the Fleet page's slot map bands — 16+ row tap-copy, 24+ Release, 32+ Lock-out, 40+ Unlock, 48+ Approve-sign-out (six rows each). A pill whose id falls outside this window paints its press but never dispatches — bump the cap with every new band.
                && hit_id < self.settings_btn_base.wrapping_add(56)
            {
                let slot = hit_id - self.settings_btn_base;
                if page == SettingsPage::Fleet {
                    if slot == 0 {
                        // "Add device" pill → the pairing-words flow.
                        self.open_add_device_flow();
                    } else if slot >= 48 {
                        // "Approve sign-out" (two-tap): the CONSENT half of the bilateral removal — countersign the leaver's departure request and publish the consented Remove. The leaver completes its side when it observes itself de-folded.
                        let idx = (slot - 48) as usize;
                        let devices = self.fleet_device_rows();
                        if let Some((pk, false, _, false, name, _, _, _)) = devices.get(idx).cloned() {
                            let matches_pending = self
                                .pending_depart_req
                                .as_ref()
                                .is_some_and(|(d, _, _)| *d == pk);
                            if !matches_pending {
                                // Stale pill (request cleared between paint and tap) — disarm and ignore.
                                self.fleet_approve_armed = None;
                            } else if self.fleet_approve_armed == Some(pk) {
                                self.fleet_approve_armed = None;
                                let Some((_, t, sig)) = self.pending_depart_req.clone() else {
                                    return EventResponse::Handled;
                                };
                                let hp = self.our_handle_proof();
                                if let (Some(hp), Some(kp)) = (hp, self.device_keypair.clone()) {
                                    match crate::network::fgtw::fleet::depart_device_consented(
                                        &kp, &hp, &pk, t, &sig,
                                    ) {
                                        Ok(()) => {
                                            crate::logf!("FLEET: countersigned {}'s departure — consented Remove published", name);
                                            self.pending_depart_req = None;
                                            self.ready_toast = Some(format!("{name} signed out of the fleet."));
                                            // Adopt the shrink immediately (rotation sentinel + row drop) instead of waiting for the next poll.
                                            if let Some(our_hp) = self.our_handle_proof() {
                                                self.spawn_contact_fleet_refresh(vec![our_hp]);
                                            }
                                        }
                                        Err(e) => {
                                            crate::logf!("FLEET: consented remove failed ({}) — request kept", e);
                                            self.ready_toast = Some("Couldn't publish the sign-out — check connection and retry.".to_string());
                                        }
                                    }
                                }
                            } else {
                                self.fleet_approve_armed = Some(pk);
                            }
                        }
                    } else if slot >= 40 {
                        // Locked sibling row's "Unlock" pill (two-tap): the owner's deliberate reversal. Same handle-confirmation shape as the lock — the confirm de-attests, the unlock fires only inside the next successful attest (pending_unlock), so it is proof-of-owner, and the handle is typed only on the standard attest screen.
                        let idx = (slot - 40) as usize;
                        let devices = self.fleet_device_rows();
                        if let Some((pk, false, _, false, name, _, _, _)) = devices.get(idx).cloned() {
                            if self.fleet_unlock_armed == Some(pk) {
                                self.fleet_unlock_armed = None;
                                if let Some(hp) = self.session.as_ref().map(|s| s.handle_proof) {
                                    self.pending_unlock = Some((pk, hp, name.clone()));
                                    tohu::clear_session();
                                    self.session = None;
                                    self.private_s = crate::crypto::blind::PrivateS::None;
                                    self.pending_broadcast_signal = -1;
                                    self.state = AppState::Launch(LaunchState::Error(format!(
                                        "Enter your handle to confirm unlocking {name}."
                                    )));
                                    self.clear_handle_for_reproof();
                                    crate::logf!("FLEET: unlock of {} armed — de-attested, awaiting handle confirmation", name);
                                }
                            } else {
                                self.fleet_unlock_armed = Some(pk);
                            }
                        }
                    } else if slot >= 32 {
                        // Live sibling row's "Lock out" pill (two-tap): treat-as-stolen. The chain is untouched — the fleet-synced locked set + key rotation do all the work.
                        let idx = (slot - 32) as usize;
                        let devices = self.fleet_device_rows();
                        if let Some((pk, false, _, false, name, _, _, _)) = devices.get(idx).cloned() {
                            if self.fleet_lock_armed == Some(pk) {
                                self.fleet_lock_armed = None;
                                // The confirm DE-ATTESTS this device; the lock executes only inside the next successful attest (see pending_lock). Owner knows the handle and sails through; a thief just signed themselves out of the one device they held. Same session teardown as Security's "Lock".
                                if let Some(hp) = self.session.as_ref().map(|s| s.handle_proof) {
                                    // Locking the LAST other live device leaves this one alone holding the fleet — if it is then lost while the lock stands, no member can ever sign the unlock (custodian supersession is the only exit, and it isn't built). Say so at the confirmation.
                                    let other_live = devices
                                        .iter()
                                        .filter(|(rpk, is_self, _, retired, _, _, _, _)| {
                                            !*is_self
                                                && !*retired
                                                && *rpk != pk
                                                && !self.is_locked_device(rpk)
                                        })
                                        .count();
                                    let warn = if other_live == 0 {
                                        " WARNING: this leaves the device you are holding as the ONLY one able to unlock it."
                                    } else {
                                        ""
                                    };
                                    self.pending_lock = Some((pk, hp, name.clone()));
                                    tohu::clear_session();
                                    self.session = None;
                                    self.private_s = crate::crypto::blind::PrivateS::None;
                                    self.pending_broadcast_signal = -1;
                                    self.state = AppState::Launch(LaunchState::Error(format!(
                                        "Enter your handle to confirm locking out {name}.{warn}"
                                    )));
                                    self.clear_handle_for_reproof();
                                    crate::logf!("FLEET: lock-out of {} armed — de-attested, awaiting handle confirmation", name);
                                }
                            } else {
                                self.fleet_lock_armed = Some(pk);
                            }
                        }
                    } else if slot >= 24 {
                        // Retired row's "Release" pill (two-tap): the OWNER frees the departed device's hardware brand — the second signature of the two-signature retire (the first was that device signing itself out). On success the pubkey joins the fleet-synced `fleet.released` setting so the row drops off every device; the chain rows themselves are permanent testimony, untouched.
                        let idx = (slot - 24) as usize;
                        let devices = self.fleet_device_rows();
                        if let Some((pk, _, _, true, name, _, _, _)) = devices.get(idx).cloned() {
                            if self.fleet_release_armed == Some(pk) {
                                self.fleet_release_armed = None;
                                let hp = self.our_handle_proof();
                                if let (Some(hp), Some(kp)) = (hp, self.device_keypair.clone()) {
                                    match crate::network::fgtw::fleet::release_device(&kp, &hp, &pk)
                                    {
                                        Ok(()) => {
                                            crate::logf!("FLEET: released the brand on {} — hardware free for a new identity", name);
                                            // Per-key entry, same shape as the locked set: concurrent releases of different brands commute instead of racing one LWW blob.
                                            self.settings_set(
                                                &format!("fleet.released.{}", hex::encode(pk)),
                                                vsf::VsfType::ke(pk.to_vec()),
                                            );
                                            self.fleet_retired.retain(|d| d != &pk);
                                            self.ready_toast = Some(format!("{name} released \u{2014} it can join a new identity now."));
                                        }
                                        Err(e) => {
                                            crate::logf!(
                                                "FLEET: release failed ({}) — brand kept",
                                                e
                                            );
                                            self.ready_toast = Some("Couldn't release \u{2014} check connection and retry.".to_string());
                                        }
                                    }
                                }
                            } else {
                                self.fleet_release_armed = Some(pk);
                            }
                        }
                    } else if slot >= 16 {
                        // Device-row tap → copy that device's name to the clipboard.
                        let idx = (slot - 16) as usize;
                        let devices = self.fleet_device_rows();
                        if let Some((_pk, _is_self, _online, _retired, name, _link, _tier, _about)) =
                            devices.get(idx)
                        {
                            let name = name.clone();
                            if self.copy_to_clipboard(&name) {
                                self.ready_toast = Some(format!("Copied {name}"));
                            }
                        }
                    } else if slot >= 8 {
                        // Bridge pill on an online sibling row → open a command conversation with THAT device (chat-as-shell: type `$ cmd`).
                        let idx = (slot - 8) as usize;
                        let devices = self.fleet_device_rows();
                        if let Some((pk, _, _, _, _, _, _, _)) = devices.get(idx).cloned() {
                            self.open_bridge_conversation(pk);
                        }
                    } else {
                        // "Rename" (slot 1) is still a stub — no device-label chain-op yet. Remove-other retired with the sovereign-records rule (self-signed departure only; eviction = withholding at the key layer, arriving with the device-trust bundle).
                        crate::log("settings-stub: Rename (no label op yet)");
                    }
                } else if page == SettingsPage::Security {
                    if slot == 0 {
                        // "Lock" → clear session only (de-attest); vault kept, re-unlock by re-typing your handle. Works on Android (the -1 broadcast drops Kotlin's sticky session).
                        self.settings_shred_armed = false;
                        self.settings_removeshred_armed = false;
                        self.settings_remove_armed = false;
                        tohu::clear_session();
                        self.session = None;
                        self.private_s = crate::crypto::blind::PrivateS::None;
                        self.pending_broadcast_signal = -1;
                        self.state = AppState::Launch(LaunchState::Fresh);
                        self.clear_handle_for_reproof();
                        crate::log("SECURITY: locked — session cleared, vault kept; re-type handle to unlock");
                    } else if slot == 2 {
                        // "Shred (crypto-wipe)" → full clean (nuke vault + clear session). Two-tap confirm (destructive + irreversible). Arming disarms the other destructive pill so exactly one confirm is ever live.
                        if self.settings_shred_armed {
                            self.settings_shred_armed = false;
                            self.clean_device_for_reuse();
                        } else {
                            self.settings_shred_armed = true;
                            self.settings_removeshred_armed = false;
                            self.settings_remove_armed = false;
                        }
                    } else if slot == 3 {
                        // "Remove & shred" → the BILATERAL departure request, wipe-on-completion flavor. Two-tap confirm. The wipe is GATED on observing our own de-fold (a sibling approved + published): nothing is wiped while the request is pending — otherwise the fleet would forever list a device whose keys are gone. Plain Shred (orange) remains the wipe-without-departing path.
                        if self.settings_removeshred_armed {
                            self.settings_removeshred_armed = false;
                            self.request_fleet_departure(true);
                        } else {
                            self.settings_removeshred_armed = true;
                            self.settings_shred_armed = false;
                            self.settings_remove_armed = false;
                        }
                    } else {
                        // Slot 1 "Remove this device from fleet" → the BILATERAL departure request, keep-vault flavor (loaner doctrine: the completion de-attests but leaves the vault's claims dormant on disk). Two-tap confirm; last-member gate inside request_fleet_departure.
                        if self.settings_remove_armed {
                            self.settings_remove_armed = false;
                            // BILATERAL: this fires the signed departure REQUEST at the siblings; a surviving member approves on their screen, and the keep-vault de-attest runs when we observe ourselves de-folded. Why not unilateral: whoever briefly holds one unlocked device could sign it out — forcing a key rotation and laundering the hardware into their own fleet.
                            self.request_fleet_departure(false);
                        } else {
                            self.settings_remove_armed = true;
                            self.settings_shred_armed = false;
                            self.settings_removeshred_armed = false;
                        }
                    }
                } else if page == SettingsPage::You {
                    if slot == 0 {
                        // "Update" → persist every field fleet-wide as `profile.<id>` settings, in ONE batched push (not one push per field).
                        self.save_you_profile();
                    } else if slot == 1 {
                        // "Change avatar…" — Android hands us the system image picker; desktop is drag/drop ONLY (no file-picker), so the pill just tells the user how.
                        #[cfg(target_os = "android")]
                        {
                            self.change_focus(None);
                            self.pending_picker_request = true;
                        }
                        #[cfg(not(target_os = "android"))]
                        {
                            self.ready_toast =
                                Some("Drag & drop an image onto the Photon window".to_string());
                        }
                    } else if slot == 2 {
                        // "Add" → register the typed label as a custom field (e.g. "Address 2") and append its box.
                        self.add_custom_field();
                    }
                } else if page == SettingsPage::Updates {
                    use crate::network::updates::Channel;
                    // The two channel buttons install the version they already show (from the page-open auto-check). An "Already on …" button is inert — spawn_update_apply no-ops when the row equals ours.
                    if slot == 1 {
                        self.spawn_update_apply(Channel::Release);
                    } else if slot == 2 {
                        self.spawn_update_apply(Channel::Dev);
                    }
                } else if page == SettingsPage::Diagnostics {
                    if slot == 3 {
                        // "View"/"Back" → in the record inspector, back to the list; else toggle the whole viewer.
                        if self.diag_log_inspect.is_some() {
                            self.diag_log_inspect = None;
                            self.diag_log_follow = true; // return pinned to the newest record
                        } else if self.diag_log_view {
                            self.diag_log_close();
                        } else {
                            self.diag_log_open();
                        }
                        self.scene_dirty = true;
                    } else if self.diag_log_view {
                        // The viewer replaces the Clear/Snapshot/Submit pills — their hit stamps can outlive a frame, and a stale tap must not clear or submit invisibly.
                    } else if slot == 0 {
                        // "Clear" → wipe the on-device log; the next line reopens a fresh, empty file.
                        crate::clear_log();
                        self.ready_toast = Some("Log cleared".to_string());
                    } else if slot == 1 {
                        // "Snapshot" → a peek at the current log size (a cheap "there's something to send" confirmation; the durable copy now lives on FGTW after Submit, not a local freeze).
                        match crate::snapshot_log_bytes() {
                            Some(b) => {
                                self.ready_toast =
                                    Some(format!("Log: {} KiB", (b.len() + 1023) / 1024))
                            }
                            None => self.ready_toast = Some("Log is empty".to_string()),
                        }
                    } else if slot == 2 {
                        // "Submit" → upload the log + optional note to FGTW (outbound HTTPS, NAT-immune — works where P2P is failing, no USB pull needed).
                        // Greyed guard — the disabled pill stamps no hit id, but the hit map is a frame stale right after a success, so a fast second tap could still dispatch here. Same predicate as the render.
                        let submit_disabled = self.log_submit_inflight
                            || self.log_submitted_len == Some(crate::log_size_bytes());
                        if !submit_disabled {
                            let note: String = self
                                .settings_note_textbox
                                .as_ref()
                                .map(|tb| tb.chars.iter().collect())
                                .unwrap_or_default();
                            self.spawn_log_submit(note);
                        }
                    }
                } else if page == SettingsPage::About {
                    if slot == 3 {
                        // Version row tapped → toggle dozenal glyphs ↔ spelled-out voca words (+ the dozenal index).
                        self.about_version_spelled = !self.about_version_spelled;
                    } else if slot == 4 {
                        // The passless weblink → system browser. Desktop-only for now (Android needs an Intent through Kotlin — follow-up).
                        #[cfg(all(
                            unix,
                            not(target_os = "android"),
                            not(target_os = "redox"),
                            not(target_os = "macos")
                        ))]
                        {
                            let _ = std::process::Command::new("xdg-open")
                                .arg("https://passless.org/")
                                .spawn();
                        }
                        #[cfg(target_os = "macos")]
                        {
                            let _ = std::process::Command::new("open")
                                .arg("https://passless.org/")
                                .spawn();
                        }
                        #[cfg(target_os = "windows")]
                        {
                            let _ = std::process::Command::new("cmd")
                                .args(["/C", "start", "https://passless.org/"])
                                .spawn();
                        }
                        crate::log("ABOUT: passless.org link tapped");
                    } else if slot == 5 {
                        // A tap anywhere within the revealed dozenal index → the custodian riddle appears beneath it. One tap; session-permanent once found.
                        self.about_riddle_revealed = true;
                    }
                } else {
                    crate::logf!(
                        "settings-stub: pill {} on {} (no behaviour wired)",
                        slot,
                        format!("{:?}", page)
                    );
                }
                ctx.window.request_redraw();
                return EventResponse::Handled;
            }
        }

        // Orb tap (chrome app-icon) — a no-op widget, so intercept here. Destined for the settings/about/help panel; until that exists it carries the INTERIM add-device entry on Ready (AddDevice cancel is now the dedicated back button, not the orb). Routed by `on_orb_click`.
        let orb_id = self.chrome.as_ref().map(|c| c.app_icon_btn.id());
        if Some(hit_id) == orb_id && hit_id != HIT_NONE && self.on_orb_click() {
            ctx.window.request_redraw();
            return EventResponse::Handled;
        }

        // Contact row tap — hit IDs in [contact_hit_base, contact_hit_base + 255].
        if matches!(self.state, AppState::Ready)
            && self.contact_hit_base != HIT_NONE
            && hit_id >= self.contact_hit_base
            && hit_id < self.contact_hit_base.wrapping_add(256)
        {
            let ci = (hit_id - self.contact_hit_base) as usize;
            if ci < self.contacts.len() {
                crate::logf!(
                    "contact-tap: opening conversation with '{}'",
                    self.contacts[ci].display_name()
                );
                self.open_conversation_with(ci);
                self.state = AppState::Conversation;
                self.reset_contact_ping_backoff();
                self.conv_topbar_off = 0.0;
                // Opening the conversation is the interaction that clears unread (ring + float drop away on the next contacts-list frame).
                self.clear_unread(ci);
                self.change_focus(None);
                // Refresh this contact's presence on conversation-enter so the header reflects reality promptly.
                self.ping_contact(ci);
                // Fetch the peer's avatar (once/session) so the conversation header shows it instead of the grey placeholder. Cache-first, network on miss; off-thread. Keyed by the pin-set (hp + party id + avatar key) — no handle.
                self.spawn_avatar_download(ci);
                ctx.window.request_redraw();
                return EventResponse::Handled;
            }
        }

        // Message-row tap (conversation) — toggle that message's details strip (direction, age, delivery, copy). The copy pill copies the message text via the platform clipboard (arboard / Kotlin poll bridge).
        if matches!(self.state, AppState::Conversation) && self.msg_hit_base != HIT_NONE {
            if hit_id == self.msg_copy_id && hit_id != HIT_NONE {
                if let Some((sci, ts, out)) = self.selected_msg {
                    let text_opt = self.conv_of(sci).and_then(|v| {
                        v.messages
                            .iter()
                            .find(|m| m.timestamp == ts && m.is_outgoing == out)
                            .map(|m| m.content.clone())
                    });
                    if let Some(text) = text_opt.map(|t| display_content(&t)) {
                        if self.copy_to_clipboard(&text) {
                            crate::log("msg-details: message text copied");
                            // Pill flips green + "copied" — event-cleared when the selection moves/closes.
                            self.selected_msg_copied = true;
                            self.scene_dirty = true;
                        }
                    }
                }
                ctx.window.request_redraw();
                return EventResponse::Handled;
            }
            // Details-strip reaction row: ranked glyph pills + the circled "+" on the selected message. Tap your current glyph = retract; another = replace; "+" arms the compose box as the picker.
            if self.react_strip_base != HIT_NONE
                && hit_id >= self.react_strip_base
                && hit_id < self.react_strip_base.wrapping_add(10)
            {
                let slot = (hit_id - self.react_strip_base) as usize;
                if let Some((sci, ts, _)) = self.selected_msg {
                    if slot == 9 {
                        // The circled "+": type anything, send commits it as the reaction.
                        self.compose_react_to = Some(ts);
                        self.compose_reply_to = None;
                        self.compose_edit_of = None;
                        if let Some(tb) = self.message_textbox.as_mut() {
                            tb.clear();
                            let id = tb.hit_id();
                            self.change_focus(Some(id));
                        }
                        self.selected_msg = None;
                        self.scene_dirty = true;
                    } else if let Some(glyph) = self.react_strip_glyphs.get(slot).cloned() {
                        let ours: Option<String> =
                            self.conv_of(sci).and_then(|v| v.current_reaction(ts, true));
                        let toggled_off = ours.as_deref() == Some(glyph.as_str());
                        let body = if toggled_off {
                            String::new()
                        } else {
                            glyph.clone()
                        };
                        if self.send_chain_message(
                            sci,
                            &body,
                            false,
                            Some((crate::types::RefKind::React, ts)),
                            None,
                        ) && !toggled_off
                        {
                            self.stamp_react_used(&glyph);
                        }
                        self.selected_msg = None;
                        self.scene_dirty = true;
                    }
                    ctx.window.request_redraw();
                    return EventResponse::Handled;
                }
            }

            // Details-strip action row: reply / edit / resend / delete on the selected message.
            if self.msg_action_base != HIT_NONE
                && hit_id >= self.msg_action_base
                && hit_id < self.msg_action_base.wrapping_add(8)
            {
                let slot = hit_id - self.msg_action_base;
                // STOP (slot 5, the bridge locus strip's pill): no selected row needed — it always targets the in-flight command; each press escalates the signal.
                if slot == 5 {
                    if let Some(ci) = self.active_contact() {
                        self.bridge_send_interrupt(ci);
                    }
                    ctx.window.request_redraw();
                    return EventResponse::Handled;
                }
                if let Some((sci, ts, out)) = self.selected_msg {
                    match slot {
                        // REPLY = a REFERENCE, never a quote: arm the target eagle_time; the compose strip shows the referenced message at half alpha, and the sent row carries only the reference — the renderer resolves it live, so a later edit of the target updates every reply pointing at it.
                        0 => {
                            let _ = out; // the reference is by eagle_time alone (either direction resolves it)
                            self.compose_reply_to = Some(ts);
                            self.compose_edit_of = None;
                            if let Some(tb) = self.message_textbox.as_mut() {
                                let id = tb.hit_id();
                                self.change_focus(Some(id));
                            }
                            self.selected_msg = None;
                            self.scene_dirty = true;
                        }
                        // EDIT = supersede-by-reference: prefill the box with the target's CURRENT body (respecting prior edits), arm the target, and the send button becomes a check mark. The original row never mutates — its content is braid key material (strands resolve stored content by eagle_time), so the correction rides as its own referencing row.
                        1 => {
                            let body: Option<String> = self.conv_of(sci).and_then(|v| {
                                v.latest_edit_for(ts).map(|(_, b)| b).or_else(|| {
                                    v.messages
                                        .iter()
                                        .find(|m| m.timestamp == ts && m.is_outgoing == out)
                                        .map(|m| display_content(&m.content))
                                })
                            });
                            if let (Some(b), Some(tb)) = (body, self.message_textbox.as_mut()) {
                                tb.clear();
                                tb.insert_str(&b, ctx.text);
                                let id = tb.hit_id();
                                self.change_focus(Some(id));
                                self.compose_edit_of = Some(ts);
                                self.compose_reply_to = None;
                                self.selected_msg = None;
                                self.scene_dirty = true;
                            }
                        }
                        // RESEND: manually re-fire an undelivered outgoing on the chain with its ORIGINAL timestamp (identity preserved — the friend dedups + re-ACKs, so this is always safe); chainless devices re-push thru the fleet instead.
                        2 => {
                            let text_opt = self.conv_of(sci).and_then(|v| {
                                v.messages
                                    .iter()
                                    .find(|m| {
                                        m.timestamp == ts && m.is_outgoing == out && !m.delivered
                                    })
                                    .map(|m| m.content.clone())
                            });
                            if let Some(text) = text_opt {
                                // A zero-remote row's delivery IS the disk write (write-confirm-then-send, 2026-08-21): resend retries the SIGNALLED persist — the verdict flips it bright or toasts the refusal again. The chain/fleet branches below never fit this row; the old fall-thru re-pushed a never-durable row to siblings, which is exactly the amplification the law forbids.
                                if self
                                    .contacts
                                    .get(sci)
                                    .map_or(false, |c| self.is_zero_remote(c))
                                {
                                    self.persist_messages_signalled(sci, vec![ts]);
                                    self.ready_toast =
                                        Some("re-writing to the vault\u{2026}".to_string());
                                    self.ready_toast_screen = None;
                                } else {
                                    let re_ref = self.conv_of(sci).and_then(|v| {
                                        v.messages
                                            .iter()
                                            .find(|m| m.timestamp == ts && m.is_outgoing == out)
                                            .and_then(|m| m.reference)
                                    });
                                    let bw = self.bridge_wire_for_row(sci, ts);
                                    if self.chain_transmit(sci, &text, ts, re_ref, bw.as_ref()) {
                                        self.ready_toast = Some("re-sent on the chain".to_string());
                                    } else {
                                        let row = self.conv_of(sci).and_then(|v| {
                                            v.messages
                                                .iter()
                                                .find(|m| m.timestamp == ts && m.is_outgoing == out)
                                                .cloned()
                                        });
                                        if let Some(row) = row {
                                            self.push_rows_to_siblings(
                                                sci,
                                                std::slice::from_ref(&row),
                                                None,
                                            );
                                            self.ready_toast = Some(
                                            "re-pushed thru the fleet (no chain on this device)"
                                                .to_string(),
                                        );
                                        }
                                    }
                                    self.ready_toast_screen = None;
                                }
                            }
                        }
                        // SAVE / FETCH (attachments): blob held → write to Downloads; missing → ask friend + siblings over PT.
                        4 => {
                            let att = self
                                .conv_of(sci)
                                .and_then(|v| {
                                    v.messages
                                        .iter()
                                        .find(|m| m.timestamp == ts && m.is_outgoing == out)
                                })
                                .and_then(|m| crate::types::parse_attachment_content(&m.content));
                            if let Some((hash, name, _)) = att {
                                if !crate::storage::blob_present(&hash) {
                                    self.attach_fetch(sci, &hash);
                                    self.ready_toast =
                                        Some("fetching from your devices\u{2026}".to_string());
                                } else if name == "call.audio" {
                                    // A kept call recording plays through the mono downmix (call/playback.rs). Holds the handle in call_playback so the worker keeps running (drop = stop); a fresh tap replaces + stops the prior. Refuses (None) if a call owns the audio session.
                                    let seed = self.session.as_ref().map(|s| s.identity_seed);
                                    if let Some(seed) = seed {
                                        self.call_playback =
                                            crate::call::playback::play_blob(&seed, &hash);
                                        self.ready_toast = Some(
                                            if self.call_playback.is_some() {
                                                "playing recording\u{2026}".to_string()
                                            } else {
                                                "can't play now (a call is active?)".to_string()
                                            },
                                        );
                                    }
                                } else {
                                    match self.attach_save(&name, &hash) {
                                        Some(dest) => {
                                            self.ready_toast =
                                                Some(format!("saved \u{2192} {}", dest));
                                            crate::logf!("attach: saved to {}", dest);
                                        }
                                        None => {
                                            self.ready_toast = Some(
                                                "save failed \u{2014} see the log".to_string(),
                                            );
                                        }
                                    }
                                }
                                self.ready_toast_screen = None;
                            }
                        }
                        // DELETE: arm the deferred delete — the strip repaints "deleting…" THIS frame, and the tick performs the removal + mirror-verified persist after that frame painted (doing it synchronously here blocked the UI for the save's duration, reading as stuck). Tombstone caveat unchanged: until they exist, fleet sync can resurrect the row.
                        3 => {
                            self.pending_delete = Some(((sci, ts, out), false));
                        }
                        _ => {}
                    }
                    self.scene_dirty = true;
                    ctx.window.request_redraw();
                }
                return EventResponse::Handled;
            }
            if hit_id >= self.msg_hit_base && hit_id < self.msg_hit_base.wrapping_add(64) {
                let vis = (hit_id - self.msg_hit_base) as usize;
                if let (Some(ci), Some(&(ts, out, ref_band))) =
                    (self.active_contact(), self.msg_hit_rows.get(vis))
                {
                    // Reference-line tap = JUMP to the source row, centered — the hint is a link, not part of the select band.
                    if let Some((band_y0, band_y1, target)) = ref_band {
                        if (ctx.cursor_y as f32) >= band_y0 && (ctx.cursor_y as f32) <= band_y1 {
                            self.scroll_to_message(ci, target);
                            ctx.window.request_redraw();
                            return EventResponse::Handled;
                        }
                    }
                    let key = (ci, ts, out);
                    // Toggle: same message deselects; another message moves the strip. Event-shown, interaction-cleared — no timers.
                    self.selected_msg = if self.selected_msg == Some(key) {
                        None
                    } else {
                        Some(key)
                    };
                    // A fresh selection (or a close) resets the copy pill to its ready state.
                    self.selected_msg_copied = false;
                    self.scene_dirty = true;
                    ctx.window.request_redraw();
                }
                return EventResponse::Handled;
            }
        }

        // A textbox release is ALREADY fully handled by the press/drag/release path in `on_event` (focus + caret + selection — see `textbox_press`). So skip `dispatch_release` for it: fluor's on_click would re-place the caret at the release column and wipe a drag selection. `textbox_by_hit_mut` is the single registry — every box (incl. `you_fields`) is covered with no hand-list.
        if self.textbox_by_hit_mut(hit_id).is_some() {
            ctx.window.request_redraw();
            return EventResponse::Handled;
        }

        // Release-activated Buttons (attest / + / send): `dispatch_release` fires only `activate_on_release()` widgets — a Button's `Click::on_click` (→ `fire`) runs here; the Released arm's `take_click` polls then submits. A drag-off yields no activation → no fire, so nothing commits on a mis-touch.
        let response = widget::dispatch_release(self, hit_id, x, y, mods);
        if matches!(response, EventResponse::Handled) {
            ctx.window.request_redraw();
        }
        response
    }

    fn on_event(&mut self, event: &Event, ctx: &mut Context) -> EventResponse {
        // Any event is user engagement — reset the presence-sweep idle clock so the cadence returns to the active (5s) tier. Cheap (just a timestamp); the immediate-sweep-on-focus is handled in the Focused arm below.
        self.last_interaction = Some(Instant::now());
        // FLEET ATTENTION transition edge (2026-08-18): qualifying human input on a NON-holder takes the ball — one frame, only when the human actually moves between devices (take_fleet_attention early-outs while we hold it, so typing here is free). Presses/wheel/keys/IME only: CursorMoved is bump-and-jitter-prone and Focused has its own edge in on_focus_changed. Android touches arrive as these same variants via fluor's shell.
        if matches!(
            event,
            Event::MouseInput {
                state: ElementState::Pressed,
                ..
            } | Event::MouseWheel { .. }
                | Event::KeyboardInput { .. }
                | Event::Ime(Ime::Commit(_))
        ) {
            self.take_fleet_attention();
        }
        // Live shift mirror for `on_close_requested` (which has no Context): a shift-held close — the chrome ✕, Alt-F4, anything — means the REAL exit, not the resident hide. Refreshed on every event so the click that lands on the close button has already stamped it.
        self.shift_held = ctx.modifiers.shift_key();
        // Every event except cursor movement may move immediate-mode content, so it claims a full-viewport frame. CursorMoved's effects are all narrow-tracked: hover tints live in the host overlay pass, drag-select is the textbox's own damage, and the one content-flavoured hover (the Ready avatar hint) sets `scene_dirty` at its flip site.
        // COMPOSE TYPING is the other narrow case: a plain keystroke (or Android IME commit) into the focused compose box only moves pixels the box's own damage tracking already claims — and the full-viewport re-raster it used to trigger cost the phone an average 21ms PER KEYSTROKE against a 16.6ms frame budget (the 2026-08-08 typing lag, measured by the render probe). Chorded keys stay full (zoom/clipboard reach beyond the box), as do Enter (submits), Esc (disarms/navigates), and Tab (moves focus). Emptiness transitions need nothing extra since the placeholder's removal — no scene pixels depend on the box's char count.
        let compose_typing = matches!(self.state, AppState::Conversation)
            && !ctx.modifiers.control_key()
            && !ctx.modifiers.super_key()
            && self
                .message_textbox
                .as_ref()
                .is_some_and(|tb| Some(tb.hit_id()) == self.focused)
            && match event {
                Event::Ime(Ime::Commit(_)) => true,
                Event::KeyboardInput { event: kev, .. } => matches!(
                    &kev.logical_key,
                    Key::Character(_)
                        | Key::Named(NamedKey::Backspace)
                        | Key::Named(NamedKey::Delete)
                        | Key::Named(NamedKey::ArrowLeft)
                        | Key::Named(NamedKey::ArrowRight)
                        | Key::Named(NamedKey::ArrowUp)
                        | Key::Named(NamedKey::ArrowDown)
                        | Key::Named(NamedKey::Home)
                        | Key::Named(NamedKey::End)
                        | Key::Named(NamedKey::Space)
                        | Key::Named(NamedKey::Shift)
                ),
                _ => false,
            };
        if !matches!(event, Event::CursorMoved { .. }) && !compose_typing {
            self.scene_dirty = true;
        }
        match event {
            Event::CursorMoved { .. } => {
                // Hit-test the shared map (chrome stamps its buttons, widgets stamp their pill silhouettes — all into chrome's map). `hit_at` returns the id under the cursor regardless of owner.
                let new_hit = self
                    .chrome
                    .as_ref()
                    .map(|c| c.hit_at(ctx.cursor_x, ctx.cursor_y))
                    .unwrap_or(HIT_NONE);
                let mut changed = false;
                // Chrome tracks its OWN hover (title-bar controls); the app widgets are flipped in ONE walk below.
                if let Some(chrome) = self.chrome.as_mut() {
                    changed |= chrome.set_hover(new_hit);
                }
                // Pointer-down over a textbox → this move pans its TEXT with the pointer (the caret rides the grabbed character), and while panning the hover doesn't matter. Handled first so a drag reads as a gesture, not a hover.
                if self.pointer_down && self.drag_select_hit != HIT_NONE {
                    if self.drag_pan_text(ctx.cursor_x, ctx.cursor_y) {
                        changed = true;
                    }
                }
                // Hover only re-walks (and repaints) when the hit under the cursor actually changes — one walk over EVERY active widget, so every textbox/button on every screen inherits hover + the I-beam with no hand-list. Frozen (busy) widgets return `None` from `hover()`, so they stay inert for free.
                if new_hit != self.hover_hit {
                    // Contact-row hover tint is CONTENT (painted into the canvas, not an overlay delta), so entering/leaving a row needs the full frame the widget-overlay path avoids.
                    let row_hover = |hit: HitId| {
                        (self.contact_hit_base != HIT_NONE
                            && hit >= self.contact_hit_base
                            && hit < self.contact_hit_base.wrapping_add(256))
                            || (self.back_btn_hit_id != HIT_NONE && hit == self.back_btn_hit_id)
                    };
                    if row_hover(new_hit) || row_hover(self.hover_hit) {
                        self.scene_dirty = true;
                    }
                    self.hover_hit = new_hit;
                    let mut is_tb = false;
                    self.visit_app_widgets(&mut |w| {
                        let over = new_hit == w.id();
                        if over && w.is_text_input() {
                            is_tb = true;
                        }
                        if let Some(h) = w.hover() {
                            h.set_hovered(over);
                        }
                    });
                    self.hover_is_textbox = is_tb;
                    changed = true;
                }
                {
                    let want = self.avatar_hit_id != HIT_NONE && new_hit == self.avatar_hit_id;
                    if self.avatar_hovered != want {
                        self.avatar_hovered = want;
                        // The avatar hover hint is CONTENT (drawn text, not an overlay tint), so its flip needs the full-viewport frame CursorMoved otherwise avoids.
                        self.scene_dirty = true;
                        changed = true;
                    }
                }
                if changed {
                    ctx.window.request_redraw();
                }
                EventResponse::Pass
            }
            Event::CursorLeft { .. } => {
                let mut changed = false;
                if let Some(chrome) = self.chrome.as_mut() {
                    changed |= chrome.set_hover(HIT_NONE);
                }
                if let Some(tb) = self.textbox.as_mut() {
                    if tb.is_hovered() {
                        tb.set_hovered(false);
                        changed = true;
                    }
                }
                if let Some(btn) = self.attest_btn.as_mut() {
                    if btn.is_hovered() {
                        btn.set_hovered(false);
                        changed = true;
                    }
                }
                if let Some(tb) = self.contacts_textbox.as_mut() {
                    if tb.is_hovered() {
                        tb.set_hovered(false);
                        changed = true;
                    }
                }
                if let Some(btn) = self.contacts_plus_btn.as_mut() {
                    if btn.is_hovered() {
                        btn.set_hovered(false);
                        changed = true;
                    }
                }
                if let Some(tb) = self.message_textbox.as_mut() {
                    if tb.is_hovered() {
                        tb.set_hovered(false);
                        changed = true;
                    }
                }
                if let Some(btn) = self.message_send_btn.as_mut() {
                    if btn.is_hovered() {
                        btn.set_hovered(false);
                        changed = true;
                    }
                }
                // Call overlay controls (cross-screen) clear their hover too — otherwise a tint sticks when the pointer leaves the window mid-call.
                for btn in [
                    self.call_start_btn.as_mut(),
                    self.call_action_btn.as_mut(),
                    self.call_decline_btn.as_mut(),
                ]
                .into_iter()
                .flatten()
                {
                    if btn.is_hovered() {
                        btn.set_hovered(false);
                        changed = true;
                    }
                }
                if changed {
                    ctx.window.request_redraw();
                }
                EventResponse::Pass
            }
            Event::ModifiersChanged(mods) => {
                // Zoom hint persists only while a zoom modifier is held. The instant Ctrl/Cmd is released, drop the top-centre percentage watermark (render arms it when `ru` changes under a held modifier). Releasing focus mid-zoom also lands here via the WM clearing modifiers.
                if !(mods.control_key() || mods.super_key()) && self.zoom_hint {
                    self.zoom_hint = false;
                    // The release edge after a zoom IS the persistence point (event-driven, no debounce timer): save the settled ru as this DEVICE's zoom — per-device (unlinked) but mirrored thru the fleet's device maps like every device setting.
                    self.save_zoom_setting(ctx.viewport.ru);
                    // The watermark lives in the bg layer, which `rasterize_bg` only repaints when dirty — invalidate it so the clearing frame actually re-runs the closure without the hint, instead of leaving the stale glyphs painted.
                    if let Some(chrome) = self.chrome.as_mut() {
                        chrome.invalidate_bg();
                    }
                    ctx.window.request_redraw();
                }
                EventResponse::Pass
            }
            Event::Focused(focused) => {
                // Feed the desktop-notification gate: focused = someone's looking, stay quiet; unfocused/hidden = ding.
                #[cfg(not(target_os = "android"))]
                crate::platform::desktop_notify::set_window_focused(*focused);
                // On focus GAIN, force an immediate presence sweep so rings are fresh the instant the user looks — clearing last_presence_ping makes the next tick treat a sweep as due regardless of how far the idle cadence had backed off. (last_interaction was already stamped at the top of on_event, resetting the cadence to the active tier.)
                if *focused {
                    self.last_presence_ping = None;
                    ctx.window.request_redraw();
                }
                // Chrome's edges + title + orb dim when the window loses focus (palette swap to `WINDOW_*_UNFOCUSED` + `TEXT_COLOUR_UNFOCUSED` + `ORB_DARKEN_UNFOCUSED`). The host independently dims the drop shadow via its own `is_focused` tracker; this handler just propagates to chrome's internal flag so the chrome layer re-rasterizes with the dimmed palette.
                if let Some(chrome) = self.chrome.as_mut() {
                    if chrome.set_focused(*focused) {
                        ctx.window.request_redraw();
                    }
                }
                // The blinkey sleeps with the window (Nick 2026-08-28): an unfocused window can't take a keystroke, so pulsing a caret in it burns a wakeup + a damage rect every ~150ms for nobody. Logical textbox focus is KEPT — only the blink engine stops (the caret freezes at whatever phase it held, like every native app) — so refocusing the window resumes typing exactly where it was. The refocus edge restarts the timer iff a textbox actually holds focus.
                if *focused {
                    let tb_focused = self.textboxes_mut().any(|(_, tb)| tb.is_focused())
                        || self.message_textbox.as_ref().map_or(false, |tb| tb.is_focused());
                    if tb_focused {
                        self.blink_timer.start(Instant::now());
                    }
                } else {
                    self.blink_timer.stop();
                }
                EventResponse::Pass
            }
            Event::MouseWheel { delta } => {
                // Bg-noise scroll. Vertical-only for now — horizontal trackpad gestures and shift-modified wheel both fold into the same `bg_scroll` axis. Discrete wheel notches (`Lines`) get multiplied to feel like a normal scroll step; continuous trackpad pixels (`Pixels`) are used directly. The scroll value feeds both `scroll_offset` (translates the noise pattern up/down on screens that want it) and `shimmer` (colour-bias cycle on every screen) in `render`.
                // Pixel deltas (touch drag, trackpads) are REAL distances — they must track 1:1 (Android touch was riding the conversation arm's extra ×8 and outran the finger 8-fold). Discrete notches keep their synthetic step.
                let (dy, is_pixel_delta) = match delta {
                    MouseScrollDelta::Lines(_, y) => ((*y as isize) * 8, false),
                    MouseScrollDelta::Pixels(_, y) => (*y as isize, true),
                };
                if dy != 0 {
                    // A live textbox pan owns the gesture: the finger is carrying the TEXT, so the pane must not also scroll under it (Android's touch-drag synthesizes wheel events alongside the CursorMoved the pan rides).
                    if self.pointer_down && self.drag_select_hit != HIT_NONE {
                        return EventResponse::Handled;
                    }
                    // Rubber-band scrolling on every axis, every platform: past either end the step is asymptotically resisted (never further than `reach` past the bound), and `tick()` eases the overshoot back once the wheel stops. `reach` scales with the window so the give feels the same on a watch and an 8K panel.
                    let reach = ctx.viewport.height_px as f32 / (1 << 3) as f32;
                    if matches!(self.state, AppState::Ready) {
                        // On the contacts screen the wheel scrolls the WHOLE user section + list as one block. Down-scroll (negative dy) moves the block up (reveals lower contacts), so subtract; render publishes the block extent (`contacts_scroll_extent`) and re-runs `update_widget_layout` so the search box + plus button (whose rects are set off `contacts_scroll`) track the same offset.
                        self.contacts_scroll = rubber_step(
                            self.contacts_scroll as f32,
                            -(dy as f32),
                            self.contacts_scroll_extent as f32,
                            reach,
                        )
                        .round() as isize;
                    } else if matches!(
                        self.state,
                        AppState::Settings(_) | AppState::ContactPanel(_)
                    ) {
                        // Settings + the contact panel (its structural mirror): the wheel scrolls the nav rail when the cursor is over it, else the content pane. Down-scroll (negative dy) reveals lower rows → add.
                        let over_rail = {
                            let sl = SettingsLayout::compute(&ctx.viewport);
                            (ctx.cursor_x as f32) < sl.content.x
                        };
                        // The foreground panes (rail + content) position rows as `inset.y − scroll`, the OPPOSITE sign to the background texture's `row − scroll` — so with the raw wheel delta they scrolled against the background (the "foreground inverted" report). Negate the delta here so the foreground gesture lands on the OS natural-scroll convention (down-scroll reveals lower rows); the background is handed the negated offsets below so ITS direction is unchanged (it reads correct already). Android touch rides the same `step`, so this one sign serves both.
                        let step = -(dy as f32);
                        if over_rail {
                            self.settings_rail_scroll = rubber_step(
                                self.settings_rail_scroll,
                                step,
                                self.settings_rail_extent,
                                reach,
                            );
                        } else {
                            self.settings_content_scroll = rubber_step(
                                self.settings_content_scroll,
                                step,
                                self.settings_content_extent,
                                reach,
                            );
                            // Log viewer tail-follow rides where the user LEAVES the scroll: at (or past) the extent = pinned to the newest record; anywhere above = reading history, appends must not yank the view.
                            if self.diag_log_view
                                && matches!(
                                    self.state,
                                    AppState::Settings(SettingsPage::Diagnostics)
                                )
                            {
                                self.diag_log_follow = self.settings_content_scroll
                                    >= self.settings_content_extent - 1.0;
                            }
                        }
                    } else if matches!(self.state, AppState::Conversation) {
                        // In a conversation the wheel scrolls the message history. The list lays out bottom-up with newest at the bottom; a positive offset pushes messages down (reveals older ones above). Scroll-up (positive dy) shows older → add. Only the 0 end rubber-bands (hi = ∞); the old-history end is backfill-paged, not clamped.
                        if self.active_conversation.is_some() {
                            let can_scroll = self.msg_max_scroll > 0.0;
                            if let Some(conv) = self.active_conv_mut() {
                                conv.scroll_offset = rubber_step(
                                    conv.scroll_offset,
                                    // Notches get the wheel step-up; pixel sources are already distances.
                                    dy as f32 * if is_pixel_delta { 1.0 } else { (1 << 3) as f32 },
                                    f32::INFINITY,
                                    reach,
                                );
                                // Scrollback jumps the history-backfill queue: the user is heading toward the old edge, so the next page request fires on the next tick instead of waiting out the trickle interval.
                                if dy > 0 {
                                    if let Some(rec) = conv.history_recovery.as_mut() {
                                        if !rec.complete {
                                            rec.urgent = true;
                                        }
                                    }
                                }
                            }
                            // Top-bar slide: the strip rides the SAME deltas as the content, sliding off as you scroll one way and back on with the other — position-tied like a browser toolbar, no snap, no timers. Only when the conversation can actually scroll.
                            if can_scroll {
                                let unit_b = ReadyLayout::compute(
                                    ctx.viewport.width_px as usize,
                                    ctx.viewport.height_px as usize,
                                    ctx.viewport.ru,
                                )
                                .unit_height;
                                let bar_h = ctx.viewport.height_px as f32 * 0.06 + unit_b * 2.15;
                                // Sign: scrolling toward the NEWEST slides the bar off; heading back into history brings it with you (the first mapping shipped inverted — user: "the contacts thing is backwards").
                                let step = -(dy as f32)
                                    * if is_pixel_delta { 1.0 } else { (1 << 3) as f32 };
                                let off = (self.conv_topbar_off + step).clamp(0.0, bar_h);
                                if (off - self.conv_topbar_off).abs() > 0.01 {
                                    self.conv_topbar_off = off;
                                    self.scene_dirty = true;
                                }
                            }
                        }
                    } else {
                        self.bg_scroll = self.bg_scroll.wrapping_add(dy);
                    }
                    if let Some(chrome) = self.chrome.as_mut() {
                        chrome.invalidate_bg();
                        // Scrolling moves the content (and therefore every per-pixel hit zone) but doesn't dirty the chrome layer on its own, so `rasterize_chrome` would early-return and skip its `hit_test_map.fill(HIT_NONE)` — leaving STALE hit stamps at the pre-scroll row/widget positions. Those ghosts make `hit_at` return the wrong id under the cursor after a scroll, so the hover overlay tints the wrong pixels. Invalidate chrome so the map is cleared and re-stamped against this frame's scrolled positions.
                        chrome.invalidate_chrome();
                    }
                    ctx.window.request_redraw();
                }
                EventResponse::Pass
            }
            Event::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                // Any click dismisses the standing hints (event-driven — never hover or time).
                self.clear_hints();
                // Resize edges OUTRANK widget hits — the CSD rule. The edge check used to run only on HIT_NONE, so a contact row reaching the window edge swallowed the press and the bottom edge was ungrabbable wherever content touched it (field report, 2026-08-16). The band is a thin perimeter strip (strip_height/4), so widget interiors are untouched; cursor_for gives the same band the resize cursor, so the grab matches the cue.
                if !ctx.is_maximized {
                    let edge = chrome::get_resize_edge(ctx.viewport, ctx.cursor_x, ctx.cursor_y);
                    if edge != ResizeEdge::None {
                        return EventResponse::StartResize(edge);
                    }
                }
                let hit_id = self
                    .chrome
                    .as_ref()
                    .map(|c| c.hit_at(ctx.cursor_x, ctx.cursor_y))
                    .unwrap_or(HIT_NONE);

                // Permanence interstitial ("Yes — forever"): a press ANYWHERE other than the attest button cancels back to the pre-proof Fresh state. Editing the handle already cancels; this makes a tap on empty space, the field, the orb — anything else — cancel too, so a stray tap can never corner the user into the forever-claim (on Android "click elsewhere" was otherwise swipe-up → home → long-press → switch away). The attest button press itself is the deliberate confirm, so it's excluded; we fall thru afterwards so the tap still does its normal thing (focus the field, start a drag, open settings, …).
                if matches!(self.state, AppState::Launch(LaunchState::Confirm)) {
                    let attest_hit = self
                        .attest_btn
                        .as_ref()
                        .map(|b| b.hit_id())
                        .unwrap_or(HIT_NONE);
                    if hit_id != attest_hit {
                        self.clear_launch_error();
                        ctx.window.request_redraw();
                    }
                }

                // KnownHandle fork (docs/lifecycle.md D1): the two pills act; any OTHER press cancels back to Fresh (interstitial rules) and falls thru to its normal meaning. "It's mine" is the FIRST moment anything posts to the network — the bind request + beacon start in submit_join_step, never before.
                if matches!(self.state, AppState::Launch(LaunchState::KnownHandle)) {
                    if hit_id == self.known_mine_hit {
                        if let Some(session) = self.probed_session.take() {
                            // ONE IDENTITY PER DEVICE holds HERE too (docs/lifecycle.md D2): this path bypasses the probe worker's marker gate, and the worker's bindreq gate would only fire AFTER the words screen showed. The probe already paid the memory-hard proof, so the hardened compare is free — no cheap-oracle path (2026-08-23 ticket).
                            if crate::storage::device_binding::busy_for(
                                &session.handle_proof,
                                &crate::crypto::clutch::identity_party_id(&session.identity_seed),
                            ) {
                                crate::log("KnownHandle: DEVICE BUSY — bound to another identity; refusing the join");
                                self.state = AppState::Launch(LaunchState::Error(
                                    "this device already carries an identity \u{2014} put it on another device first, then Remove & shred (Settings \u{2192} Security)".to_string(),
                                ));
                                self.refocus_handle_select_all();
                                ctx.window.request_redraw();
                                return EventResponse::Handled;
                            }
                            crate::log(
                                "KnownHandle: it's-mine → pairing words (the ceremony posts NOW)",
                            );
                            self.probed_handle = None;
                            self.launch_add_mode = true;
                            self.state = AppState::Launch(LaunchState::Fresh);
                            self.add_join_handle = None;
                            self.submit_join_step(Some(session.handle_proof));
                        }
                        ctx.window.request_redraw();
                        return EventResponse::Handled;
                    }
                    if hit_id == self.known_pick_hit {
                        crate::log("KnownHandle: pick-another — back to the field");
                        self.clear_launch_error();
                        self.refocus_handle_select_all();
                        ctx.window.request_redraw();
                        return EventResponse::Handled;
                    }
                    self.clear_launch_error();
                    ctx.window.request_redraw();
                }

                // Log-viewer row tap → the VSF inspector for that record (the same coloured structural view vsfinfo prints, parsed from vsf::inspect_vsf's ANSI). Geometric — rows carry no hit ids; the maths mirrors the render/culling exactly.
                if hit_id == HIT_NONE
                    && self.diag_log_view
                    && self.diag_log_inspect.is_none()
                    && matches!(self.state, AppState::Settings(SettingsPage::Diagnostics))
                {
                    let sl = SettingsLayout::compute(&ctx.viewport);
                    let inset = sl.content_inset();
                    let line = sl.content_line_h();
                    let band_top = inset.y + 2. * line;
                    if ctx.cursor_x >= inset.x
                        && ctx.cursor_x <= inset.x + inset.w
                        && ctx.cursor_y >= band_top
                        && ctx.cursor_y <= inset.y + inset.h
                    {
                        let row_h = line * 0.5;
                        let idx = ((ctx.cursor_y - band_top + self.settings_content_scroll)
                            / row_h)
                            .floor();
                        if idx >= 0.0 {
                            let idx = idx as usize;
                            if let Some(rec) = self.diag_log_rows.get(idx) {
                                let text = match vsf::inspect::inspect_vsf(&rec.raw) {
                                    Ok(t) => t,
                                    Err(e) => format!("inspect failed: {e}"),
                                };
                                let lines: Vec<Vec<(String, u32)>> = text
                                    .lines()
                                    .map(|l| ansi_line_to_spans(l, *theme::LABEL_COLOUR))
                                    .collect();
                                crate::logf!(
                                    "LOGVIEW: inspecting record {} ({} line(s))",
                                    idx,
                                    lines.len()
                                );
                                self.diag_log_inspect = Some((idx, lines));
                                self.diag_log_follow = false;
                                self.settings_content_scroll = 0.0; // inspector opens at the TOP of the record
                                self.scene_dirty = true;
                                ctx.window.request_redraw();
                                return EventResponse::Handled;
                            }
                        }
                    }
                }

                if hit_id == HIT_NONE {
                    // No widget under the cursor — clear focus, then start a move-drag (the host promotes it to an actual drag once the cursor moves). Resize edges were already claimed at the top of this arm, before ANY hit dispatch.
                    if self.change_focus(None) {
                        ctx.window.request_redraw();
                    }
                    return EventResponse::StartWindowDrag;
                }

                // Textbox press: photon owns textbox pointer gestures end-to-end — focus + place the caret + drop a drag anchor here (double-click → word, triple → all), extend on drag in `CursorMoved`, finalize on release. `on_activate` therefore SKIPS `dispatch_release` for textboxes, so fluor's on_click can't clobber the selection on release.
                if self.textbox_press(hit_id, ctx.cursor_x, ctx.cursor_y) {
                    ctx.window.request_redraw();
                    return EventResponse::Handled;
                }

                // Every OTHER item — contacts, pills, nav, orb, back, avatar, start-fresh, the Buttons — activates on RELEASE over the same element (fluor's PointerArbiter → `on_activate`); a drag-off before release cancels. So the press arm does NO activation and NO focus change for them: focusing on press left a button stuck in its dark focused tint after a drag-off (and swallowed hover). The host has already armed the element (held colour); we just consume the press so it doesn't fall through to a window drag.
                ctx.window.request_redraw();
                EventResponse::Handled
            }
            Event::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                // End any textbox drag-select and finalize the caret/selection (fires on EVERY release, so a drag-off outside the box clears the state too).
                if self.pointer_down {
                    self.textbox_release();
                    ctx.window.request_redraw();
                }
                // Attest button: poll `take_click` AFTER release — Button::on_click increments the counter at press; we observe the rising edge here so submit fires once per press/release pair regardless of how chrome dispatches subsequent events.
                let clicked = self
                    .attest_btn
                    .as_mut()
                    .map(|b| b.take_click())
                    .unwrap_or(false);
                if clicked {
                    self.submit_handle();
                    ctx.window.request_redraw();
                }
                // Contacts plus button — same release-edge polling pattern.
                let plus_clicked = self
                    .contacts_plus_btn
                    .as_mut()
                    .map(|b| b.take_click())
                    .unwrap_or(false);
                if plus_clicked {
                    self.submit_add_friend();
                    ctx.window.request_redraw();
                }
                // Conversation send button — same release-edge polling pattern as the plus button.
                let send_clicked = self
                    .message_send_btn
                    .as_mut()
                    .map(|b| b.take_click())
                    .unwrap_or(false);
                if send_clicked {
                    self.submit_message();
                    // Return focus to the compose box so the send button releases its focused/active (dark, pressed-in) tint — otherwise it sticks down — and the user keeps typing.
                    if let Some(id) = self.message_textbox.as_ref().map(|t| t.hit_id()) {
                        self.change_focus(Some(id));
                    }
                    ctx.window.request_redraw();
                }
                // Call overlay controls (answer/decline/start) — same release-edge poll.
                self.dispatch_call_button_clicks(ctx);
                EventResponse::Pass
            }
            Event::KeyboardInput { event: kev, .. } => {
                // Any keystroke dismisses the standing hints (event-driven — never hover or time).
                self.clear_hints();
                // A PLAIN keystroke also acknowledges the toast — but zoom chords (Ctrl/Cmd + anything) don't, so the user can zoom in to read it (fluor host handles the zoom itself; the modifier guard covers any chords that fall thru to us).
                if !(ctx.modifiers.control_key() || ctx.modifiers.super_key()) {
                    self.clear_toast();
                }
                // Bracket chord first — tracks Press/Release timestamps regardless of focus so the debug overlay arms as soon as both brackets are held, and the chord action runs before delivery to the focused widget (so an action letter like 'h' doesn't also type into the textbox).
                if let Key::Character(c) = &kev.logical_key {
                    let cs = c.as_str();
                    let now = Instant::now();
                    let mut action_char: Option<char> = None;
                    match (cs, kev.state) {
                        ("[", ElementState::Pressed) => self.chord_lb_press = Some(now),
                        ("[", ElementState::Released) => self.chord_lb_release = Some(now),
                        ("]", ElementState::Pressed) => self.chord_rb_press = Some(now),
                        ("]", ElementState::Released) => self.chord_rb_release = Some(now),
                        (_, ElementState::Pressed) if !kev.repeat => {
                            if self.brackets_held(now) {
                                action_char = c.to_ascii_lowercase().chars().next();
                            }
                        }
                        _ => {}
                    }
                    if cs == "[" || cs == "]" {
                        ctx.window.request_redraw();
                    }
                    if let Some(ac) = action_char {
                        if self.handle_chord_action(ac, ctx) {
                            return EventResponse::Handled;
                        }
                    }
                }

                // Press-only routing for Tab / Esc / Enter and delivery to the focused widget. Released arms (key-up) don't insert characters or trigger actions, so we no-op them. `repeat` keys DO insert characters (auto-repeat typing) so we don't filter on it here.
                if kev.state != ElementState::Pressed {
                    return EventResponse::Pass;
                }

                // Clipboard chords (Ctrl/Cmd + C / X / V) are intercepted HERE, before delivery to the focused widget — fluor's design keeps the OS clipboard (arboard) with the app, not on Textbox (the clipboard is a single global resource; threading it thru every widget would be premature). Ctrl+A stays on the widget (pure selection, no OS resource). Desktop only: Android paste arrives thru the IME commit path, and Redox has no arboard backend.
                #[cfg(not(any(target_os = "redox", target_os = "android")))]
                if ctx.modifiers.control_key() || ctx.modifiers.super_key() {
                    if let Key::Character(c) = &kev.logical_key {
                        let lc = c.to_lowercase();
                        if lc == "c" || lc == "x" || lc == "v" {
                            let resp = self.clipboard_chord(&lc, ctx.text);
                            if matches!(resp, EventResponse::Handled) {
                                ctx.window.request_redraw();
                                self.blink_timer.start(Instant::now());
                            }
                            return resp;
                        }
                    }
                }

                match &kev.logical_key {
                    // Tab cycles focus thru the widget tree in registration order (launch widgets first, then chrome). Intercepted BEFORE delivery so the traversal pair stays traversal; Ctrl+Tab is NOT intercepted — it falls thru to the focused widget, which types a literal tab (the verbatim-insert escape hatch; paste is the other).
                    Key::Named(NamedKey::Tab) if !ctx.modifiers.control_key() => {
                        let dir = if ctx.modifiers.shift_key() {
                            TabDir::Backward
                        } else {
                            TabDir::Forward
                        };
                        let current_focus = self.focused;
                        let next = widget::linear_tab_next(self, current_focus, dir);
                        if self.change_focus(next) {
                            ctx.window.request_redraw();
                        }
                        EventResponse::Handled
                    }
                    // Esc = BACK, one level per press; at the top of the stack it hides the app (resident) on every platform. Shift+Esc = the real exit, from anywhere. Also cancels an in-flight attestation back to Fresh — without this the user is stuck on the "Attesting…" indicator with no way out if the FGTW response never lands. Android's hardware/gesture back routes here via `nativeOnBackPressed` → Escape (long-press back on 3-button nav = the Activity's real close).
                    Key::Named(NamedKey::Escape) => {
                        if ctx.modifiers.shift_key() {
                            // The deliberate quit chord: bypasses residency for ONE close so the host actually exits.
                            self.exit_requested = true;
                            return EventResponse::Close;
                        }
                        if matches!(self.state, AppState::ContactPanel(_)) {
                            self.contact_boot_armed = false;
                            self.state = AppState::Conversation;
                            self.reset_contact_ping_backoff();
                            // Same re-entry clear as the Back button — the conversation is front-of-eyes again.
                            if let Some(ci) = self.active_contact() {
                                self.clear_unread(ci);
                            }
                            ctx.window.request_redraw();
                            return EventResponse::Handled;
                        }
                        if matches!(self.state, AppState::Conversation) {
                            // First Esc (or Android back) disarms a pending reply/edit/react — clearing an armed edit also clears its prefill; the next Esc navigates back.
                            if self.compose_reply_to.is_some()
                                || self.compose_edit_of.is_some()
                                || self.compose_react_to.is_some()
                            {
                                if self.compose_edit_of.take().is_some() {
                                    if let Some(tb) = self.message_textbox.as_mut() {
                                        tb.clear();
                                    }
                                }
                                self.compose_reply_to = None;
                                self.compose_react_to = None;
                                self.scene_dirty = true;
                                ctx.window.request_redraw();
                                return EventResponse::Handled;
                            }
                            self.broadcast_focus_claim(false);
                            self.state = AppState::Ready;
                            self.active_conversation = None;
                            ctx.window.request_redraw();
                            return EventResponse::Handled;
                        }
                        if matches!(self.state, AppState::Settings(_)) {
                            // Mirror the panel's Back affordance: Settings closes to the contacts screen (post-attest) or the launch screen.
                            self.change_focus(None);
                            self.state = if self.session.is_some() {
                                AppState::Ready
                            } else {
                                AppState::Launch(LaunchState::Fresh)
                            };
                            ctx.window.request_redraw();
                            return EventResponse::Handled;
                        }
                        if matches!(self.state, AppState::AddDevice) {
                            // Escape cancels back to the Fleet page the flow came from.
                            self.end_add_device_flow();
                            self.refresh_fleet_retired();
                            self.state = AppState::Settings(SettingsPage::Fleet);
                            ctx.window.request_redraw();
                            return EventResponse::Handled;
                        }
                        // Cancel launch JOIN mode (stops the join thread so it quits re-posting its request).
                        if self.launch_add_mode {
                            self.launch_add_mode = false;
                            self.end_join_flow();
                            self.add_join_status.clear();
                            if let Some(tb) = self.textbox.as_mut() {
                                tb.clear();
                            }
                            ctx.window.request_redraw();
                            return EventResponse::Handled;
                        }
                        if matches!(self.state, AppState::Launch(LaunchState::Attesting)) {
                            self.state = AppState::Launch(LaunchState::Fresh);
                            ctx.window.request_redraw();
                            return EventResponse::Handled;
                        }
                        if self.change_focus(None) {
                            ctx.window.request_redraw();
                            return EventResponse::Handled;
                        }
                        // Top of the stack (contacts screen / idle launch, nothing focused): Escape = the close button. Resident desktop → the host hides the window; Android → the shell reports unhandled and the Activity moveTaskToBack()s. Either way: hidden, still running, never an exit.
                        EventResponse::Close
                    }
                    // Enter submits the handle when the textbox is focused — intercepted before delivery so the textbox doesn't insert a literal newline. When the attest button is focused, route to its on_key (Button activates on Enter / Space and we observe via take_click in tick / on_event Release path). Both Launch and Ready screens follow the same shape with their respective widgets.
                    Key::Named(NamedKey::Enter) => {
                        let focused_is_launch_textbox = self
                            .textbox
                            .as_ref()
                            .map(|t| Some(t.hit_id()) == self.focused)
                            .unwrap_or(false);
                        if focused_is_launch_textbox {
                            if matches!(self.state, AppState::AddDevice) {
                                // Words-entry screen: Enter nudges a re-match (the live matcher re-derives on every edit anyway; this covers "candidates arrived after I finished typing").
                                self.refresh_add_device_match();
                            } else {
                                self.submit_handle();
                            }
                            ctx.window.request_redraw();
                            return EventResponse::Handled;
                        }
                        let focused_is_contacts_textbox = self
                            .contacts_textbox
                            .as_ref()
                            .map(|t| Some(t.hit_id()) == self.focused)
                            .unwrap_or(false);
                        if focused_is_contacts_textbox {
                            self.submit_add_friend();
                            ctx.window.request_redraw();
                            return EventResponse::Handled;
                        }
                        let focused_is_compose = self
                            .message_textbox
                            .as_ref()
                            .map(|t| Some(t.hit_id()) == self.focused)
                            .unwrap_or(false);
                        if focused_is_compose {
                            // Desktop: Enter sends, Shift+Enter inserts the newline. ANDROID: every Enter is a newline — a soft IME has no Shift+Enter, so the send button is the only send (the messenger convention thumbs already know).
                            if ctx.modifiers.shift_key() || cfg!(target_os = "android") {
                                if let Some(focus_id) = self.focused {
                                    let resp = widget::dispatch_key(
                                        self,
                                        focus_id,
                                        kev,
                                        ctx.modifiers,
                                        ctx.text,
                                    );
                                    if matches!(resp, EventResponse::Handled) {
                                        ctx.window.request_redraw();
                                    }
                                    return resp;
                                }
                            }
                            self.submit_message();
                            ctx.window.request_redraw();
                            return EventResponse::Handled;
                        }
                        if let Some(focus_id) = self.focused {
                            let resp =
                                widget::dispatch_key(self, focus_id, kev, ctx.modifiers, ctx.text);
                            // Either button can activate on Enter; poll both and route to the matching submit.
                            let attest_clicked = self
                                .attest_btn
                                .as_mut()
                                .map(|b| b.take_click())
                                .unwrap_or(false);
                            if attest_clicked {
                                self.submit_handle();
                            }
                            let plus_clicked = self
                                .contacts_plus_btn
                                .as_mut()
                                .map(|b| b.take_click())
                                .unwrap_or(false);
                            if plus_clicked {
                                self.submit_add_friend();
                            }
                            // Send button Space-activation (when focused).
                            let send_clicked = self
                                .message_send_btn
                                .as_mut()
                                .map(|b| b.take_click())
                                .unwrap_or(false);
                            if send_clicked {
                                self.submit_message();
                            }
                            // Call overlay controls — Enter/Space activation when one holds focus.
                            let call_clicked = self.dispatch_call_button_clicks(ctx);
                            if attest_clicked
                                || plus_clicked
                                || send_clicked
                                || call_clicked
                                || matches!(resp, EventResponse::Handled)
                            {
                                ctx.window.request_redraw();
                            }
                            return resp;
                        }
                        EventResponse::Pass
                    }
                    // All other keys → focused widget via dispatch_key. The Textbox's on_key handles character insertion, backspace, arrows, selection, clipboard (Ctrl+A); Button's on_key handles Space activation. Unfocused → Pass so the host can ignore. Request redraw on Handled so character insertion paints immediately instead of waiting for the next tick.
                    _ => {
                        // Words-entry screen accepts ONLY letters and space — the 23 pairing words are ASCII-alphabetic voca words, so digits/punctuation/emoji are always typos. Named keys (backspace, arrows, Tab) aren't Character events and pass thru untouched.
                        if matches!(self.state, AppState::AddDevice) {
                            if let Key::Character(c) = &kev.logical_key {
                                if !c.chars().all(|ch| ch.is_ascii_alphabetic() || ch == ' ') {
                                    return EventResponse::Handled;
                                }
                            }
                        }
                        if let Some(focus_id) = self.focused {
                            // Snapshot the handle text so an EDIT (typing, backspace, delete-selection — any content change) tears down the Error/Confirm interstitial. The clipboard chords do this explicitly; this covers the plain-keystroke path, which previously didn't — so a user could arm Confirm on handle A, retype it to handle B, and the press fired A's probed roots (observed: attested as the fresh handle while the box showed the taken one).
                            let launch_text_before: Option<Vec<char>> =
                                if matches!(self.state, AppState::Launch(_)) {
                                    self.textbox.as_ref().map(|tb| tb.chars.clone())
                                } else {
                                    None
                                };
                            let resp =
                                widget::dispatch_key(self, focus_id, kev, ctx.modifiers, ctx.text);
                            if let Some(before) = launch_text_before {
                                if self.textbox.as_ref().map(|tb| &tb.chars) != Some(&before) {
                                    self.clear_launch_error();
                                }
                            }
                            if matches!(resp, EventResponse::Handled) {
                                ctx.window.request_redraw();
                                // Reset blink so the cursor stays solid thru fast typing instead of blinking mid-keystroke.
                                self.blink_timer.start(Instant::now());
                            }
                            return resp;
                        }
                        EventResponse::Pass
                    }
                }
            }
            Event::Ime(Ime::Commit(s)) => {
                // IME typing also dismisses the standing hints (event-driven — never hover or time).
                self.clear_hints();
                // Soft-keyboard input is a keystroke: it acknowledges the toast too (Android has no zoom chords to guard).
                self.clear_toast();
                // Android: soft IME committed `s` (typing, swipe, autocomplete). Route it to whichever textbox holds focus — the attest handle field OR the contacts search box. (This used to be hardcoded to the attest box, so typing on the contacts screen was silently dropped on Android even though focus + the soft keyboard were correct; desktop never hit this because physical keys go thru the focus-generic `widget::dispatch_key`.) Backspace arrives as the literal "\b" character from PhotonSurfaceView's deleteSurroundingText / composing-text replacement path, so peel those off and route to `backspace`; everything else inserts verbatim. No-op when no textbox is focused (focus might sit on the attest button via Tab).
                let mut handled = false;
                let words_screen = matches!(self.state, AppState::AddDevice);
                // The multi-line compose box lives OUTSIDE the single-line registry — its IME branch comes first. A committed "\n" inserts (the Android model: Enter is a newline, the send button sends).
                let compose_focused = self
                    .message_textbox
                    .as_ref()
                    .map(|t| Some(t.hit_id()) == self.focused)
                    .unwrap_or(false);
                if compose_focused {
                    if let Some(tb) = self.message_textbox.as_mut() {
                        for c in s.chars() {
                            if c == '\u{0008}' {
                                tb.backspace(ctx.text);
                            } else {
                                tb.insert_char(c, ctx.text);
                            }
                        }
                        handled = true;
                    }
                } else if let Some(tb) = self.focused_textbox_mut() {
                    for c in s.chars() {
                        if c == '\u{0008}' {
                            tb.backspace(ctx.text);
                        } else if !words_screen || c.is_ascii_alphabetic() || c == ' ' {
                            // Words entry accepts only letters and space — swipe/autocomplete punctuation is silently dropped.
                            tb.insert_char(c, ctx.text);
                        }
                    }
                    handled = true;
                }
                if handled {
                    // Soft-IME edits are edits: tear down the Error/Confirm interstitial exactly like physical keystrokes, so Android can't re-arm stale probed roots either.
                    if matches!(self.state, AppState::Launch(_)) {
                        self.clear_launch_error();
                    }
                    self.blink_timer.start(Instant::now());
                    ctx.window.request_redraw();
                    return EventResponse::Handled;
                }
                EventResponse::Pass
            }
            Event::DroppedFile(path) => {
                // A file dropped on an OPEN CONVERSATION = send it as an attachment. (Ready-screen drops stay the avatar pipeline below.)
                if matches!(self.state, AppState::Conversation) {
                    if let Some(ci) = self.active_contact() {
                        self.send_attachment_from_path(ci, path);
                        ctx.window.request_redraw();
                        return EventResponse::Handled;
                    }
                }
                // Desktop avatar update: a file dropped on the window (Ready screen) is read and run thru the same encode→save→load→install→upload pipeline as the Android picker. Ignored off the Ready screen and when no handle is attested yet (set_avatar_from_file no-ops without a handle). Android has no drop path — it uses the picker.
                if matches!(self.state, AppState::Ready) {
                    match std::fs::read(path) {
                        Ok(bytes) => {
                            self.set_avatar_from_file(bytes);
                            // Force a FULL repaint, not just a redraw request: on macOS the drop arrives during the drag-session teardown and the incremental present can get swallowed — the avatar then only appeared after the next click. Invalidate everything so the post-drop frame rebuilds + re-presents the whole window.
                            self.scene_dirty = true;
                            if let Some(chrome) = self.chrome.as_mut() {
                                chrome.invalidate_bg();
                                chrome.invalidate_chrome();
                            }
                            ctx.window.request_redraw();
                        }
                        Err(e) => crate::logf!("avatar drop: read failed: {}", e),
                    }
                }
                EventResponse::Handled
            }
            _ => EventResponse::Pass,
        }
    }

    fn wake_at(&self) -> Option<Instant> {
        // Schedule the next wakeup at the soonest of: * `blink_timer.next_tick()` — drives the focused-textbox cursor pulse (random 0-300ms intervals); `None` while no textbox is focused.
        // * `now` when an attestation is in flight — `tick()` advances `attest_anim_phase` at 1 cycle/sec for the "query in flight" wave shift; we need a wakeup every frame to keep it animating smoothly. Without this, the host blocks waiting for input and the animation stalls.
        let blink = self.blink_timer.next_tick();
        // An attestation OR an in-flight add-friend search both need a wakeup every frame to animate (the spectrum wave / the hourglass wobble).
        let animating = matches!(
            self.state,
            AppState::Launch(LaunchState::Attesting) | AppState::Searching
        ) || self.add_in_flight
            // The full-screen ring panel's pulse rings animate every frame while a call is Ringing (phase is a pure function of now; the wakeup is what keeps frames coming).
            || self
                .active_call
                .as_ref()
                .map_or(false, |c| c.phase == crate::call::CallPhase::Ringing);
        let anim = animating.then(Instant::now);
        // Next background presence sweep — keeps online/offline rings refreshing while idle (no input/network). Only on Ready; first sweep is due immediately if never run. Interval tapers with idle time, so as the user stays away the scheduled wake naturally pushes further out.
        let presence = matches!(self.state, AppState::Ready).then(|| {
            let now = Instant::now();
            self.last_presence_ping
                .map_or(now, |last| last + self.presence_ping_interval(now))
        });
        // Pairing flows: join-words (new device) and add-device matcher/confirm (old device) results arrive on mpsc channels from worker threads, with nothing else guaranteed to drive a tick while the user's hands are off — so poll-drain at 2 Hz while either flow is live. This is channel plumbing, not time-based UI: nothing is shown or cleared on a clock.
        let pairing = (self.add_join_rx.is_some() || self.add_device_rx.is_some())
            .then(|| Instant::now() + std::time::Duration::from_millis(500));
        // Periodic own-chain re-fold (the fleet-membership doorbell) — scheduled on the screens where a stale fleet view matters, so it fires even while the desktop window sits idle on the Fleet page. 45s matches advance_protocol's cadence.
        let fleet_refold = matches!(
            self.state,
            AppState::Ready | AppState::Conversation | AppState::Settings(_)
        )
        .then(|| {
            self.last_fleet_refold.map_or_else(Instant::now, |last| {
                last + std::time::Duration::from_secs(45)
            })
        });
        // Live call timer: an Active call recomputes its duration each frame, so it needs frames flowing — but only at ~2 Hz (seconds granularity), not the pulse's full rate. A minimized Active call still ticks so the strip's timer stays honest. Ringing already animates at full rate above.
        let call_timer = self
            .active_call
            .as_ref()
            .map_or(false, |c| c.phase == crate::call::CallPhase::Active)
            .then(|| Instant::now() + std::time::Duration::from_millis(500));
        // Soonest of all scheduled wakeups.
        [blink, anim, presence, pairing, fleet_refold, call_timer]
            .into_iter()
            .flatten()
            .min()
    }

    fn tick(&mut self, ctx: &mut Context) -> bool {
        let now = Instant::now();
        let mut needs_redraw = false;
        // Frame fence for the deferred send drain: entries queued during THIS tick's input pass wait until the next one, guaranteeing the pending bubble a rendered frame before the wire half runs.
        self.tick_serial = self.tick_serial.wrapping_add(1);
        // Storage-failure latch → the amber banner. Writer threads and open paths can only set a static (no &mut self there); this mirror is how a fence error or a dead vault open reaches the screen — 1,276 of them once ran for hours as log lines while the UI claimed all was well (2026-08-24).
        if crate::storage::vault_sick() && !self.vault_degraded {
            self.vault_degraded = true;
            self.scene_dirty = true;
            needs_redraw = true;
        }

        // Android foreground edges, latched by nativeSetForeground on the Activity main thread and drained here where &mut self lives (2026-08-18). Pause retracts the clearer claim (siblings may ding again — the drop-sweep at their end covers anything that crossed in flight); resume is the same human-is-here edge as a desktop focus gain. When both latched since the last tick (fast pause→resume), apply them in the order that ends at the CURRENT truth.
        #[cfg(target_os = "android")]
        {
            let (lost, gained) = crate::platform::jni_android::take_foreground_edges();
            if lost || gained {
                // Intermediate flapping collapses to the current truth: a pause→resume that both landed since the last tick is just "still here" (claim never retracted — continuity), and a resume→pause is just "gone".
                self.on_focus_changed(crate::platform::jni_android::app_in_foreground());
            }
        }

        // Point the top-left orb at the current subject (peer avatar + their presence ring in a conversation, else the Photon orb + our connectivity). Self-diffing — a no-op unless the contact / avatar / screen changed.
        self.update_orb();

        // Deferred message delete: runs only AFTER the "deleting…" frame painted (see pending_delete). TOMBSTONE, not removal: the flag propagates monotonically thru fleet sync (push + sweep + merge true-wins), and a hidden DELETE marker rides the chain to the FRIEND so their side tombstones too — delete-for-everyone. Content is preserved internally (braid weave dependency — see ChatMessage::deleted).
        if let Some(((sci, ts, out), true)) = self.pending_delete {
            self.pending_delete = None;
            let mut tombstoned: Option<ChatMessage> = None;
            let storage = self.storage.clone();
            if let Some(conv) = self.conv_mut_of(sci) {
                if let Some(m) = conv
                    .messages
                    .iter_mut()
                    .find(|m| m.timestamp == ts && m.is_outgoing == out)
                {
                    if !m.deleted {
                        m.deleted = true;
                        tombstoned = Some(m.clone());
                    }
                }
                if tombstoned.is_some() {
                    conv.invalidate_digest(); // a tombstone drops a row from the syncable set
                    if let Some(storage) = storage.as_ref() {
                        let _ = crate::storage::contacts::save_messages(conv, storage);
                    }
                }
            }
            if let Some(row) = tombstoned {
                // Attachments truly shred: only ROW CONTENT is braid-bound (preserved) — the blob file has no weave duty, so the bytes themselves are deleted here and on every device that applies this tombstone.
                if let Some((hash, _, _)) = crate::types::parse_attachment_content(&row.content) {
                    crate::storage::blob_delete(&hash);
                }
                // Fleet-wide: the tombstoned row rides the ordinary sibling push (merge upgrades true-wins).
                self.push_rows_to_siblings(sci, std::slice::from_ref(&row), None);
                // Cross-party: the hidden delete marker on the chain (friend conversations with a local chain; a chainless device's fleet tombstone still reaches the chain owner, which is where a follow-up marker could ride — v1 logs the gap).
                // Send the marker only where there is someone to send it TO. Zero remote participants (our own notes) means the row is already gone everywhere it exists; a sibling is our own fleet, which the push above already covered.
                let has_remote = self
                    .contacts
                    .get(sci)
                    .and_then(|c| self.our_party_id(c).map(|us| (c, us)))
                    .is_some_and(|(c, us)| c.remote_count(&us) > 0);
                let is_sib = self.contacts.get(sci).map(|c| c.is_sibling).unwrap_or(true);
                if has_remote && !is_sib {
                    let marker = format!("{}{}", crate::types::DELETE_MARKER_PREFIX, ts);
                    if self.send_chain_message(sci, &marker, true, None, None) {
                        crate::log(
                            "msg-details: delete marker sent to the friend (delete-for-everyone)",
                        );
                    } else {
                        crate::log("msg-details: no local chain for the delete marker — fleet tombstone propagates; the friend keeps their copy until a chain-holding device re-sends");
                    }
                }
                crate::log("msg-details: message tombstoned (deleted-for-everyone)");
            }
            self.selected_msg = None;
            self.selected_msg_copied = false;
            self.msg_wrap = None; // row set changed — drop the wrap cache outright
            self.scene_dirty = true;
            needs_redraw = true;
        }

        // Fleet chain replication push: any friendship chain that mutated since its last push ships to the siblings. Constant-time no-op when nothing changed.
        self.drive_chain_replication();

        // SETTINGS ARE LOCAL-FIRST. A launch pushes NOTHING: settings live in this device's vault, and the fleet slot is consulted only when the vault has no value for a key. They travel outward exactly when the user adjusts one (`save_*_setting` → `persist_and_push_settings`) — never on a timer, never "just to be safe".
        // The old unconditional re-push here treated every launch as an edit. That is what made a launch able to damage the fleet: the push is pull-merge-push, so a pull that failed for ANY reason (network blip, AEAD failure across a key rotation, a roster tag the reader didn't know) rebased the whole slot on empty and the push overwrote everyone's settings with this device's view. Observed on the PRST2→PRST3 bump — "state pulled — 8 roster entries, 0 global settings, 0 device maps". A device that never edits anything must never be able to do that.
        // The ROSTER still re-pushes: it is a CRDT of contacts this device genuinely holds, its merge is union-by-handle_proof with per-entry LWW, and a fleet formed before roster-sync existed needs a seed. Settings have no such need — a value nobody changed is not news.
        if !self.settings_repushed
            && self.session.is_some()
            && self.fleet_key_cached().is_some()
            && self.our_handle_proof().is_some()
            && self.device_keypair.is_some()
        {
            self.settings_repushed = true;
            crate::log(
                "FLEET: session roster re-push — keys settled (settings stay local until edited)",
            );
            self.spawn_roster_push();
        }

        // Load the persisted phonebook once the vault and session are both up. One-shot per session; the flag also stops a failed read retrying every tick.
        if !self.peer_store_loaded
            && self.storage.is_some()
            && self.session.is_some()
            && self.peer_store.is_some()
        {
            self.peer_store_loaded = true;
            self.load_peer_store();
        }

        // Keep our own signed record current. Cheap no-op once published for the current address; re-fires when the address moves or when attestation finally supplies the handle_proof an earlier reflexive echo had to wait for. Timed because this edge USED to trigger the multi-second phonebook freeze (via the every-beacon persist) — if any residue survives the off-thread move, it surfaces as a PERF line here rather than a silent gap.
        {
            let t_phase = std::time::Instant::now();
            if self.our_reflexive.is_some() && self.our_reflexive != self.self_record_published_for
            {
                self.publish_self_peer_record();
                // Our own row changed, so the persisted copy is stale. Mark it dirty — the debounce gate below writes it off-thread, coalescing with any gossip-growth edge. The phonebook is a cache, so a gossiped row we lose to a crash arrives again on the next exchange.
                self.request_peer_persist();
            }

            // Debounced phonebook flush: at most one off-thread write per interval, coalescing the own-address and gossip-growth edges. The write itself (verify + encode + vault IO) runs on the peer-persist worker — this only gates how often it's kicked, sparing flash wear and CPU on a store that grows a row at a time.
            if self.peer_persist_dirty {
                const PEER_PERSIST_DEBOUNCE: std::time::Duration =
                    std::time::Duration::from_secs(30);
                let due = self
                    .last_peer_persist
                    .is_none_or(|t| t.elapsed() >= PEER_PERSIST_DEBOUNCE);
                if due {
                    self.peer_persist_dirty = false;
                    self.last_peer_persist = Some(now);
                    self.persist_peer_store();
                }
            }
            let ms = t_phase.elapsed().as_millis();
            if ms > 50 {
                crate::logf!(
                    "PERF: phonebook publish+persist edge took {}ms (UI thread)",
                    ms
                );
            }
        }

        // Periodic fleet-sweep backstop (~5 min, jittered): re-arm history recovery for every conversation so devices converge even when the edge-triggered kicks (roster merge, sibling-online) were missed. Signed-in only; a complete conversation costs one early-stop page.
        if self.session.is_some() {
            let due = self.last_fleet_sweep.map_or(true, |t| {
                t.elapsed() > std::time::Duration::from_secs(crate::jitter(600).max(60) as u64)
            });
            if due {
                self.last_fleet_sweep = Some(now);
                self.kick_fleet_history_sweep("periodic backstop");
                self.reserve_fleet_forwards();
            }
        }

        // Android zoom persistence rides the pinch-RELEASE edge (Kotlin onScaleEnd → nativeOnScaleEnd), the exact analog of desktop's Ctrl/Cmd key-up save. No timers — the value is settled the moment the fingers lift.
        #[cfg(target_os = "android")]
        if crate::platform::jni_android::take_scale_ended()
            && (ctx.viewport.ru - self.zoom_saved_ru).abs() > 0.001
        {
            self.zoom_saved_ru = ctx.viewport.ru;
            self.save_zoom_setting(ctx.viewport.ru);
        }

        // IME-inset watch (Android): the surface never resizes for the keyboard, so an inset change arrives with NO resize event — diff it here and relayout the bottom-anchored widgets + repaint. Cheap atomic read per tick.
        #[cfg(target_os = "android")]
        {
            let ime = crate::platform::jni_android::ime_inset_px();
            if ime != self.last_ime_inset {
                self.last_ime_inset = ime;
                self.update_widget_layout(ctx);
                self.scene_dirty = true;
                needs_redraw = true;
            }
        }

        // Android sticky-session freshness: while signed in, on a fresh resume/attest (deadline None) and every jittered 30–60 min after, fire the "ensure" signal — Kotlin reads the sticky and only re-posts if the OS evicted it. Keeps the reinstall-survival capsule alive against Samsung's sticky eviction, cheaply (the read-then-skip means no churn when it's already there). Only overrides an idle signal so a fresh attest's force-post (1) or a nuke (-1) is never clobbered.
        #[cfg(target_os = "android")]
        if self.session.is_some() && self.pending_broadcast_signal == 0 {
            let due = self.next_session_broadcast.map_or(true, |t| now >= t);
            if due {
                self.pending_broadcast_signal = 2;
                self.next_session_broadcast =
                    Some(now + std::time::Duration::from_secs(crate::jitter(3600).max(60) as u64));
            }
        }

        // Toast screen-change watch: capture the screen the toast first renders on; a later mismatch (user navigated — INCLUDING settings page-to-page, hence the full AppState value not the discriminant) clears it. Clicks/scrolls/zoom never clear a toast — see clear_toast.
        if self.ready_toast.is_some() {
            match &self.ready_toast_screen {
                None => self.ready_toast_screen = Some(self.state.clone()),
                Some(s) if *s != self.state => {
                    self.clear_toast();
                    needs_redraw = true;
                }
                _ => {}
            }
        }

        // Page-change focus drop (see `last_screen`): a screen swap must never leave the previous screen's textbox focused — the orphaned box kept its blinkey firing and its focus glow lit after navigating away. `change_focus(None)` also lowers the Android IME via the pending-keyboard signal.
        if self.state != self.last_screen {
            let same_screen = match (&self.state, &self.last_screen) {
                (AppState::Launch(_), AppState::Launch(_)) => true,
                (AppState::Ready | AppState::Searching, AppState::Ready | AppState::Searching) => {
                    true
                }
                (AppState::Settings(a), AppState::Settings(b)) => a == b,
                _ => false,
            };
            if !same_screen {
                self.change_focus(None);
                // A screen swap must also re-raster the CACHED bg layer — it's dirty-gated and nothing else invalidates it on navigation, so the previous screen's backdrop stayed baked beneath the new one (the launch chromatic wave + wordmark showing thru the settings panel; the settings divider-split noise lingering after Back). One noise re-raster per screen change is cheap.
                if let Some(chrome) = self.chrome.as_mut() {
                    chrome.invalidate_bg();
                }
                needs_redraw = true;
            }
            self.last_screen = self.state.clone();
        }

        // Freeze / unfreeze the busy widgets (attest field+button while attesting, search box+plus while adding) before anything else this frame — disabled widgets drop out of dispatch via their fluor accessors.
        self.sync_busy_freeze();

        // Pairing v2 beacon scan (docs/pairing-v2.md): scan exactly while the AddDevice screen is up, diffed per tick so EVERY exit path (back, orb, bind, crash of the check thread) stops the radio without scattered call sites. The scanner is identity-agnostic — it collects photon-magic service UUIDs and the candidate matcher resolves each to a fleet device by keyed tag.
        let want_scan = matches!(self.state, AppState::AddDevice);
        if want_scan != self.beacon_scan_active {
            if want_scan {
                crate::network::pairing_beacon::start_scan();
                crate::network::pairing_nfc::start_reader();
                self.beacon_scan_active = true;
            } else {
                crate::network::pairing_beacon::stop_scan();
                crate::network::pairing_nfc::stop_reader();
                self.beacon_scan_active = false;
            }
        }
        // NFC instant add: a tapped secret matching a candidate's published commitment IS the proximity + intent proof — bind it now, no words typed. The green-confirm rotation gate still stands (two-phase), so "instant" = tap phones, press confirm.
        if want_scan {
            if let Some(s) = crate::network::pairing_nfc::take_secret() {
                let matched = self
                    .add_device_candidates
                    .iter()
                    .find(|c| {
                        c.req.nfc_hash != [0u8; 32]
                            && fgtw::pair::nfc_secret_hash(&s, &c.req.device_pubkey, c.req.t)
                                == c.req.nfc_hash
                    })
                    .map(|c| c.req.clone());
                match matched {
                    Some(req) if !self.add_device_checking => {
                        crate::logf!(
                            "NFC: tap matched candidate {} — binding",
                            crate::fp(&req.device_pubkey)
                        );
                        self.spawn_bind_device(req);
                        needs_redraw = true;
                    }
                    Some(_) => {}
                    None => crate::log(
                        "NFC: tapped secret matched no candidate (stale request or foreign tag)",
                    ),
                }
            }
        }

        // Compute per-tick delta_time for the attest-animation accumulator. `last_tick` is None on the very first tick — bootstrap to "zero elapsed" so the accumulator doesn't take a huge jump on startup.
        let delta_time = match self.last_tick {
            Some(prev) => now.duration_since(prev).as_secs_f32(),
            None => 0.,
        };
        self.last_tick = Some(now);

        // Spectrum animation while attesting: wave phase advances at 2π rad/sec = 1 cycle/sec. Provides the visual "query in flight" cue the legacy build had — the bar slowly slides while we wait for FGTW to answer. Idle / Fresh / Error states leave the phase frozen so the screen stays calm.
        if matches!(self.state, AppState::Launch(LaunchState::Attesting))
            || matches!(self.state, AppState::Searching)
        {
            self.attest_anim_phase += delta_time * std::f32::consts::TAU;
            self.attest_anim_phase %= std::f32::consts::TAU;
            if let Some(chrome) = self.chrome.as_mut() {
                chrome.invalidate_bg();
            }
            needs_redraw = true;
        }

        // Add-friend hourglass: stochastic wobble (≈ −12..+13°/tick) while a search is in flight, so the icon "shakes" like sand. xorshift keeps it dependency-free; the icon lives in the foreground (not the bg layer), so a plain redraw repaints it.
        if self.add_in_flight {
            self.hourglass_rng ^= self.hourglass_rng << 13;
            self.hourglass_rng ^= self.hourglass_rng >> 7;
            self.hourglass_rng ^= self.hourglass_rng << 17;
            let wobble = (self.hourglass_rng % 26) as f32 - 12.0; // −12..+13
            self.hourglass_angle = (self.hourglass_angle + wobble).rem_euclid(360.0);
            needs_redraw = true;
        }

        // Answer/Decline pressed on the Android call notification (backgrounded ring): the action intent latched a flag on the service thread; drain it here on the UI thread that owns the call state.
        #[cfg(target_os = "android")]
        if let Some(answer) = crate::platform::jni_android::take_call_action() {
            if answer {
                self.answer_call();
            } else {
                self.decline_call();
            }
            needs_redraw = true;
        }

        // Full-screen ring panel: the pulse rings are a pure function of now, so a ringing call just needs the frame to repaint fully (the panel covers the whole surface; partial damage would leave stale pulse arcs).
        if self
            .active_call
            .as_ref()
            .map_or(false, |c| c.phase == crate::call::CallPhase::Ringing)
        {
            self.scene_dirty = true;
            needs_redraw = true;
        }

        // Rubber-band spring: any scroll axis stretched past its bounds eases back exponentially (overshoot × e^(−8t) — C∞ in time, ~90% recovered in 0.3 s), snapping the final sub-third-pixel so the animation terminates. Runs only while an axis is out of range, so steady-state ticks are free. Scroll moves content (and its hit stamps), so a spring frame is a full scene frame with chrome invalidated — same as the wheel handler's frames.
        {
            let decay = (-delta_time * (1 << 3) as f32).exp();
            let relax = |v: &mut f32, hi: f32| -> bool {
                let bound = if *v < 0.0 {
                    0.0
                } else if *v > hi {
                    hi
                } else {
                    return false;
                };
                let over = (*v - bound) * decay;
                *v = if over.abs() < 0.3 {
                    bound
                } else {
                    bound + over
                };
                true
            };
            let mut spring = false;
            if matches!(self.state, AppState::Settings(_)) {
                spring |= relax(&mut self.settings_rail_scroll, self.settings_rail_extent);
                spring |= relax(
                    &mut self.settings_content_scroll,
                    self.settings_content_extent,
                );
            }
            if matches!(self.state, AppState::Ready) {
                let mut c = self.contacts_scroll as f32;
                if relax(&mut c, self.contacts_scroll_extent as f32) {
                    self.contacts_scroll = c.round() as isize;
                    spring = true;
                }
            }
            if matches!(self.state, AppState::Conversation) {
                let ceiling = self.msg_max_scroll;
                if let Some(conv) = self.active_conv_mut() {
                    spring |= relax(&mut conv.scroll_offset, f32::INFINITY);
                    // Clamp the STORED offset to the last-rendered ceiling: the 0-end rubber-bands (relax above, hi=∞), but drifting PAST the top (offset > max_scroll after the viewport shrank/grew) must snap back, else the list sticks above the oldest message until you scroll down through the excess. Only pull DOWN — never fight an active drag toward 0.
                    if conv.scroll_offset > ceiling {
                        conv.scroll_offset = ceiling;
                        spring = true;
                    }
                }
            }
            if spring {
                self.scene_dirty = true;
                needs_redraw = true;
                if let Some(chrome) = self.chrome.as_mut() {
                    chrome.invalidate_bg();
                    chrome.invalidate_chrome();
                }
            }
            // Textbox TEXT-pan spring: any box carried past its scroll bounds eases home the same way. Skip the box still under the finger (the drag owns it until release). Narrow damage — the box's own text_cache_dirty → damage_rect covers the repaint, so no scene_dirty needed.
            let panning = if self.pointer_down {
                self.drag_select_hit
            } else {
                HIT_NONE
            };
            let mut tb_spring = false;
            for (_, tb) in self.textboxes_mut() {
                if tb.hit_id() != panning && tb.spring_scroll(decay) {
                    tb_spring = true;
                }
            }
            if tb_spring {
                needs_redraw = true;
            }
        }

        // Drive the blinkey on the focused textbox. `BlinkTimer::poll(now)` returns `true` ONLY on the rising edge of each fire (then schedules the next random 0-300ms interval and returns false the rest of the time). On each fire, toggle the focused textbox's blinkey via `flip_blinkey` — which is a no-op on an unfocused textbox, so we can call it on every textbox without gating. Tracked SEPARATELY from `needs_redraw`: a blinkey flip is fully covered by the textbox's own `damage_rect`, so a pure-blink frame must not raise `scene_dirty` — that's what keeps the idle repaint a teeny cursor-sized rect instead of the whole window.
        let mut blink_redraw = false;
        if self.blink_timer.poll(now) {
            if let Some(tb) = self.message_textbox.as_mut() {
                if tb.flip_blinkey() {
                    blink_redraw = true;
                }
            }
            for (_, tb) in self.textboxes_mut() {
                if tb.flip_blinkey() {
                    blink_redraw = true;
                }
            }
        }

        // Diagnostics log viewer: drain the off-thread decode / tail-follow the live file (no-op unless the viewer is open on its page). Rows are CONTENT — a change needs the full scene frame, not just a widget-overlay pass.
        if self.drive_diag_log() {
            self.scene_dirty = true;
            needs_redraw = true;
        }

        // Self-update: drain check/apply results, then re-exec if a verified swap landed. The exec MUST happen here on the main thread, outside every borrow — the process image is replaced in place (unix) or handed off (windows), so nothing after it runs.
        // Update events (progress bar, channel states, status lines) are all page CONTENT — without scene_dirty the redraw runs but the dirty-gated content pass skips the page, so the bar painted its empty track once and froze (observed).
        if self.drain_update_events() {
            self.scene_dirty = true;
            needs_redraw = true;
        }
        // Keep-transcode results: a finished N-channel recording mints its `call.audio` row here (off-thread transcode posted back over the channel).
        if self.drain_call_keep() {
            needs_redraw = true;
        }
        // Auto-attest arm/disarm: apply the off-thread handle-proof verdict (spawned on the confirm click). Done here on the main thread so set_unattended's vault write, the checkbox, and focus stay UI-thread. Compute the verdict first so `self` isn't borrowed while we mutate it.
        let unattended_verdict = self
            .unattended_verify
            .as_ref()
            .and_then(|(rx, t)| rx.try_recv().ok().map(|ok| (ok, *t)));
        if let Some((ok, target_on)) = unattended_verdict {
            self.unattended_verify = None;
            if ok {
                self.unattended_confirm = None;
                self.set_unattended(target_on);
                if let Some(cb) = self.settings_unattended_check.as_mut() {
                    cb.set_checked(target_on);
                }
                self.change_focus(None);
                self.ready_toast = Some(if target_on {
                    "Unattended auto-attest ARMED — this box reboots as you".to_string()
                } else {
                    "Unattended auto-attest disarmed".to_string()
                });
                self.ready_toast_screen = None;
            } else {
                self.unattended_confirm_failed = true;
            }
            self.scene_dirty = true;
            needs_redraw = true;
        }
        if let Some(exe) = self.update_reexec.take() {
            crate::log("UPDATE: re-exec into the new binary");
            // exec() replaces the process image (and the Windows arm exits) — flush the soft-mode batch or the update trail (and everything since the last edge) dies here.
            crate::flush_log_buffer();
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                let err = std::process::Command::new(&exe)
                    .args(std::env::args().skip(1))
                    .exec();
                // exec only returns on failure.
                crate::logf!(
                    "UPDATE: re-exec failed: {} — keep running the old image",
                    err
                );
            }
            #[cfg(windows)]
            {
                match std::process::Command::new(&exe)
                    .args(std::env::args().skip(1))
                    .spawn()
                {
                    Ok(_) => std::process::exit(0),
                    Err(e) => crate::logf!(
                        "UPDATE: relaunch failed: {} — keep running the old image",
                        e
                    ),
                }
            }
            #[cfg(not(any(unix, windows)))]
            {
                let _ = exe;
            }
        }

        // Everything network/protocol lives in advance_protocol(): presence sweep, channel drains, CLUTCH ceremony + chain advancement, retransmits. It touches NO surface, so it can also run headless from the Android foreground service while the app is backgrounded (screen off ⇒ the Choreographer stops calling tick, but the state is alive — see docs/background-tick.md). The frame-only work (animations above, render below) stays here in tick.
        // Compose GROWTH is a layout edge: a keystroke that changes the wrapped line count moves list_bottom, so the following frame reflows the scene — the keystroke frame itself stays narrow (the grown box paints over the stale list for that one frame; its own damage covers the new bbox).
        let compose_lines = self
            .message_textbox
            .as_ref()
            .map(|t| t.line_count())
            .unwrap_or(1);
        if compose_lines != self.painted_compose_lines {
            self.painted_compose_lines = compose_lines;
            self.scene_dirty = true;
            needs_redraw = true;
        }

        needs_redraw |= self.advance_protocol(now);

        // Content-flavoured redraws dirty the scene (full-viewport frame); a pure blinkey flip stays out so its frame narrows to the textbox's own damage rect.
        self.scene_dirty |= needs_redraw;
        let redraw = needs_redraw || blink_redraw;
        if redraw {
            ctx.window.request_redraw();
        }
        redraw
    }

    fn damage_rect(&mut self, viewport: Viewport) -> Option<PixelRect> {
        let vw = viewport.width_px as usize;
        let vh = viewport.height_px as usize;
        // Full viewport whenever immediate-mode content may have moved (`scene_dirty`), and whenever the chord hint is up or just released (stale hint pixels need one covering frame to clear).
        let chord = self.last_chord_held || self.brackets_held(Instant::now());
        if self.scene_dirty || chord {
            let mut combined = PixelRect::new(0, 0, vw, vh);
            if chord {
                combined = combined.union(chord_hint_bbox(viewport, vw, vh));
            }
            return Some(combined);
        }
        // Pure widget frame (blinkey flip, drag-select growth): union each active widget's self-reported damage. This walks the SAME `visit_app_widgets` registry as dispatch/hover/render, so the gate AUTOMATICALLY mirrors what's drawn — a new textbox's blinkey/selection damage is claimed with zero hand-list (the recurring "new box's blinkie stacks / forces full-screen redraws" bug). `None` = nothing changed, host skips the render entirely.
        let mut combined: Option<PixelRect> = None;
        if let Some(chrome) = self.chrome.as_ref() {
            if let Some(r) = chrome.damage_rect() {
                combined = Some(combined.map_or(r, |c| c.union(r)));
            }
        }
        self.visit_app_widgets(&mut |w| {
            if let Some(r) = w.damage_rect(vw, vh) {
                combined = Some(combined.map_or(r, |c| c.union(r)));
            }
        });
        combined
    }

    fn render(&mut self, target: &mut [u32], ctx: &mut Context) {
        self.render_frame(target, ctx);
    }

    fn hit_test_map(&self) -> Option<(&[HitId], usize, usize)> {
        let chrome = self.chrome.as_ref()?;
        let (w, h) = chrome.dims();
        Some((chrome.hit_test_map(), w, h))
    }

    fn overlay_deltas(&mut self) -> Vec<u32> {
        // Walk the container once; every Hover-capable widget contributes its tint to the slot indexed by its HitId. Slot 0 is HIT_NONE (= 0 tint). Chrome's four buttons emit their per-action hover colours via the impl in chrome_widget; future Photon widgets get the same treatment for free as soon as they impl Hover::tint_delta.
        let count = self.hit_counter as usize + 1;
        widget::build_overlay_deltas(self, count)
    }

    fn overlay_bboxes(
        &mut self,
        viewport_w: usize,
        viewport_h: usize,
    ) -> Vec<Option<fluor::canvas::PixelRect>> {
        // Parallel to overlay_deltas: each Hover widget's pill bbox by HitId, so the host bounds the tint scan to the hovered widget's rect instead of the whole window. Widgets without a bbox (e.g. chrome buttons that don't impl hover_bbox yet) get None → full-window fallback for that id.
        let count = self.hit_counter as usize + 1;
        widget::build_overlay_bboxes(self, count, viewport_w, viewport_h)
    }

    fn cursor_for(&self, x: Coord, y: Coord, ctx: &Context) -> CursorIcon {
        // Resize edges OUTRANK every widget cue — the CSD rule, and the same priority the press arm gives them. Checked first: a contact row (or any widget) reaching the window edge used to win the cursor here, so the bottom band showed Pointer and the edge read as ungrabbable wherever content touched it (field report, 2026-08-16). The band is a thin perimeter strip (strip_height/4); widget interiors are untouched.
        if !ctx.is_maximized {
            match chrome::get_resize_edge(ctx.viewport, x, y) {
                ResizeEdge::None => {}
                ResizeEdge::Top | ResizeEdge::Bottom => return CursorIcon::NsResize,
                ResizeEdge::Left | ResizeEdge::Right => return CursorIcon::EwResize,
                ResizeEdge::TopLeft | ResizeEdge::BottomRight => return CursorIcon::NwseResize,
                ResizeEdge::TopRight | ResizeEdge::BottomLeft => return CursorIcon::NeswResize,
            }
        }
        let hit = self
            .chrome
            .as_ref()
            .map(|c| c.hit_at(x, y))
            .unwrap_or(HIT_NONE);
        if let Some(chrome) = self.chrome.as_ref() {
            // Every chrome button is pressable — including the orb (settings/about/help panel; interim add-device wiring) — so all get the pointer cue, matching the orb's hover brighten.
            if chrome.owns_hit(hit) {
                return CursorIcon::Pointer;
            }
        }
        if let Some(btn) = self.attest_btn.as_ref() {
            if btn.hit_id() == hit {
                return CursorIcon::Pointer;
            }
        }
        if let Some(btn) = self.contacts_plus_btn.as_ref() {
            if btn.hit_id() == hit {
                return CursorIcon::Pointer;
            }
        }
        if let Some(btn) = self.message_send_btn.as_ref() {
            if btn.hit_id() == hit {
                return CursorIcon::Pointer;
            }
        }
        // Call overlay controls (cross-screen) — pointer cursor when hovered.
        for btn in [
            self.call_start_btn.as_ref(),
            self.call_action_btn.as_ref(),
            self.call_decline_btn.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if btn.hit_id() == hit {
                return CursorIcon::Pointer;
            }
        }
        // Any textbox under the cursor → I-beam. `hover_is_textbox` is set by the ONE hover walk (via `Widget::is_text_input`) on every `CursorMoved`, so every box on every screen is covered with no hand-list.
        if self.hover_is_textbox && hit == self.hover_hit {
            return CursorIcon::Text;
        }
        // Contact rows and conversation back button — pointer cursor.
        if self.contact_hit_base != HIT_NONE
            && hit >= self.contact_hit_base
            && hit < self.contact_hit_base.wrapping_add(256)
        {
            return CursorIcon::Pointer;
        }
        if hit == self.back_btn_hit_id && self.back_btn_hit_id != HIT_NONE {
            return CursorIcon::Pointer;
        }
        match chrome::get_resize_edge(ctx.viewport, x, y) {
            ResizeEdge::Top | ResizeEdge::Bottom => CursorIcon::NsResize,
            ResizeEdge::Left | ResizeEdge::Right => CursorIcon::EwResize,
            ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorIcon::NwseResize,
            ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorIcon::NeswResize,
            ResizeEdge::None => CursorIcon::Default,
        }
    }
}
