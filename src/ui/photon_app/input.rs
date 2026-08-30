//! Input plumbing and device hygiene — cross-thread `send_event`, widget layout, clipboard chords, textbox registry/press/drag, bracket chords, dev wipe, and device-reuse cleanup.

use super::*;

impl PhotonApp {
    /// Send a [`PhotonEvent`] thru the event-loop proxy. Returns `false` if the proxy hasn't been set yet (host hasn't called `set_event_proxy`) or if the event loop has closed. Background tasks clone the proxy once at startup and call this; UI-thread code should mutate state directly + return `true` from `tick` or `on_event` instead of going thru the proxy.
    #[allow(dead_code)] // Wired for background tasks to push events onto the UI thread; no caller yet.
    pub fn send_event(&self, event: PhotonEvent) -> bool {
        match &self.event_proxy {
            Some(proxy) => proxy.send(event).is_ok(),
            None => false,
        }
    }

    /// Recompute the launch-screen widget geometry from the current viewport. Called from `init` once after construction and from `on_resize` on every viewport/zoom change. Font size and stroke ride `effective_span()` (= span × ru) so widgets grow/shrink with Ctrl+/Ctrl-/Ctrl+scroll zoom in lockstep with chrome.
    pub(super) fn update_widget_layout(&mut self, ctx: &mut Context) {
        let buf_w = ctx.viewport.width_px as usize;
        let buf_h = ctx.viewport.height_px as usize;
        // Launch form squishes above the soft keyboard (ime_lift = keyboard px, 0 on desktop/closed); the 0.4 floor keeps a monster keyboard from crushing the form to nothing. The IME-inset watch in tick() re-runs this layout on every inset change.
        let ime_lift = self.ime_lift();
        let layout = LaunchLayout::compute(buf_w, buf_h, ctx.viewport.ru).lifted(ime_lift, buf_h);
        let attest = AttestBlockLayout::compute(layout.attest_block);
        // Font size = textbox-slot height × 0.75. Derived from the pill so the text-to-pill ratio stays constant at any viewport — span/24 sized text via the harmonic-mean span which scales differently from pill_h (pill_h is linear in viewport_h, span is biased toward the narrower dim), so on a tall narrow phone the pill grew faster than the text and a soft-keyboard show/hide jumped the ratio. Pill-derived sizing keeps padding around the text proportional, so descenders + ascenders never crowd the squircle edge. Same scalar drives the attest button and the resting ∞ so they read as a matched set.
        let textbox_h = (attest.textbox.y1 as f32) - (attest.textbox.y0 as f32);
        let font_size = textbox_h * 0.75;

        if let Some(tb) = self.textbox.as_mut() {
            let (cx, cy, w, h) = rect_center_dims(attest.textbox);
            tb.set_rect(cx, cy, w, h);
            tb.set_font_size(font_size, ctx.text);
        }
        if let Some(btn) = self.attest_btn.as_mut() {
            let (cx, cy, w, h) = rect_center_dims(attest.attest);
            btn.set_rect(cx, cy, w, h);
            btn.set_font_size(font_size);
        }

        // Contacts-page widgets: textbox takes the full ReadyLayout textbox slot; the plus button is OVERLAID inside the textbox's right edge. Button size = 7/8 textbox height, inset from the right by 1/16 of the textbox height. Same font_size as the launch widgets so zoom feels consistent across screens.
        let ready_layout = ReadyLayout::compute(buf_w, buf_h, ctx.viewport.ru);
        let slot = ready_layout.textbox;
        let slot_x0 = slot.x0 as f32;
        let slot_y0 = slot.y0 as f32;
        let slot_w = (slot.x1 - slot.x0) as f32;
        let slot_h = (slot.y1 - slot.y0) as f32;
        // The search box + overlaid plus button scroll with the user section, so subtract the SAME `contacts_scroll` the render pass uses. Both passes read it from `self`, so the rendered content (drawn at this rect) and the hit-stamp (which follows the rect) move together; offsetting the rect moves visual + hit area as one.
        let tb_cx = slot_x0 + slot_w * 0.5;
        let tb_cy = slot_y0 + slot_h * 0.5 - self.contacts_scroll as f32;
        let plus_size = slot_h * 7.0 / 8.0;
        let plus_inset = slot_h / 16.0;
        let plus_cx = slot_x0 + slot_w - plus_inset - plus_size * 0.5;
        let plus_cy = tb_cy;
        if let Some(tb) = self.contacts_textbox.as_mut() {
            tb.set_rect(tb_cx, tb_cy, slot_w, slot_h);
            tb.set_font_size(font_size, ctx.text);
            // The overlaid + button's footprint (size + its inset from the pill edge) is excluded from the text area so a long search string never slides under it.
            tb.set_right_inset(plus_size + plus_inset);
        }
        if let Some(btn) = self.contacts_plus_btn.as_mut() {
            btn.set_rect(plus_cx, plus_cy, plus_size, plus_size);
            btn.set_font_size(font_size);
        }

        // Conversation compose box: a full-width strip lifted off the bottom edge by `compose_margin`. Geometry must match the render block's `compose_h`/`compose_margin`/`compose_cy`, where `unit` is ReadyLayout's span-based harmonic unit (same as the contacts screen — no hardcoded pixels). The send button is OVERLAID inside the box's right edge, exactly like the contacts-screen `+` search button (7/8 of the box height, inset 1/16 from the right).
        let unit = ReadyLayout::compute(buf_w, buf_h, ctx.viewport.ru).unit_height;
        let compose_h = unit * 1.8;
        let compose_margin = unit * 0.8;
        let compose_w = buf_w as f32 - unit * 2.0;
        let compose_cx = buf_w as f32 * 0.5;
        // `ime_lift` rides the anchor so the compose bar (and its overlaid send button) sits above the soft keyboard — same term the render's list_bottom subtracts.
        let compose_cy = buf_h as f32 - self.ime_lift() - compose_margin - compose_h * 0.5;
        if let Some(tb) = self.message_textbox.as_mut() {
            tb.set_font_size(font_size, ctx.text);
            // Exclude the overlaid send button's footprint (7/8-height square + 1/16 inset, matching the button block below) so typing never runs under the arrow.
            tb.set_right_inset(compose_h * 7.0 / 8.0 + compose_h / 16.0);
            // Bottom-anchored growth: the box's bottom edge stays where the single-line bar sat; extra lines grow UPWARD into the list, capped at a third of the screen (the field call: line COUNT varies wildly with zoom, screen fraction doesn't).
            let bottom_y = compose_cy + compose_h * 0.5;
            tb.set_layout(
                compose_cx,
                bottom_y,
                compose_w,
                buf_h as f32 / 3.0,
                ctx.text,
            );
        }
        if let Some(btn) = self.message_send_btn.as_mut() {
            let send_size = compose_h * 7.0 / 8.0;
            let send_inset = compose_h / 16.0;
            let box_right = compose_cx + compose_w * 0.5;
            let send_cx = box_right - send_inset - send_size * 0.5;
            btn.set_rect(send_cx, compose_cy, send_size, send_size);
            btn.set_font_size(font_size);
        }

        // Settings panel (STUB): position the stateful widgets on the selected page. Content-body rows give each control a slot; a control's rect is a portion of its row so the label can sit beside / above it. Only the active page's widgets are repositioned — the others keep their placeholder geometry off-screen, and `visit` gates them out anyway.
        if let AppState::Settings(page) = self.state {
            let layout = SettingsLayout::compute(&ctx.viewport);
            let settings_content_scroll = self.settings_content_scroll;
            // Controls ride the same unit as the page text (zoom + shape aware) — these were the "don't change scale" elements.
            let ctrl_font = layout.unit * 0.58;
            let ctrl_h = layout.unit;
            match page {
                SettingsPage::Appearance => {
                    // Rows: [0]=title [1]=Theme label [2]=Theme dropdown [3]=Party colours [4]=Zoom label [5]=Zoom slider [6]=Calibration.
                    let rows = layout
                        .content_scrolled(8, settings_content_scroll)
                        .split_v([1.0; 8]);
                    if let Some(dd) = self.settings_theme_dropdown.as_mut() {
                        let r = rows[2].center_h(0.7);
                        dd.set_rect(r.center_x(), r.center_y(), r.w, ctrl_h);
                        dd.set_font_size(ctrl_font);
                    }
                    if let Some(sl) = self.settings_zoom_slider.as_mut() {
                        let r = rows[5].center_h(0.8);
                        sl.set_rect(r.center_x(), r.center_y(), r.w, ctrl_h);
                    }
                }
                SettingsPage::Recovery => {
                    let rows = layout
                        .content_scrolled(8, settings_content_scroll)
                        .split_v([1.0; 8]);
                    if let Some(cb) = self.settings_custodian_check.as_mut() {
                        let r = rows[2];
                        cb.set_rect(r.x + r.w * 0.45, r.center_y(), r.w * 0.9, ctrl_h);
                        cb.set_font_size(ctrl_font);
                    }
                }
                SettingsPage::Security => {
                    let rows = layout
                        .content_scrolled(15, settings_content_scroll)
                        .split_v([1.0; 15]);
                    // Checkbox on row 12 (normal state); the confirm handle box shares that row (confirm state) — only one is visited/drawn at a time.
                    if let Some(cb) = self.settings_unattended_check.as_mut() {
                        let r = rows[12];
                        cb.set_rect(r.x + r.w * 0.45, r.center_y(), r.w * 0.9, ctrl_h);
                        cb.set_font_size(ctrl_font);
                    }
                    if let Some(tb) = self.unattended_confirm_tb.as_mut() {
                        let r = rows[13];
                        tb.set_rect(r.center_x(), r.center_y(), r.w * 0.9, ctrl_h);
                        tb.set_font_size(ctrl_font, ctx.text);
                    }
                }
                SettingsPage::Notifications => {
                    let rows = layout
                        .content_scrolled(8, settings_content_scroll)
                        .split_v([1.0; 8]);
                    if let Some(cb) = self.settings_chime_check.as_mut() {
                        let r = rows[1];
                        cb.set_rect(r.x + r.w * 0.45, r.center_y(), r.w * 0.9, ctrl_h);
                        cb.set_font_size(ctrl_font);
                    }
                    if let Some(cb) = self.settings_presence_check.as_mut() {
                        let r = rows[3];
                        cb.set_rect(r.x + r.w * 0.45, r.center_y(), r.w * 0.9, ctrl_h);
                        cb.set_font_size(ctrl_font);
                    }
                    if let Some(cb) = self.settings_background_check.as_mut() {
                        let r = rows[5];
                        cb.set_rect(r.x + r.w * 0.45, r.center_y(), r.w * 0.9, ctrl_h);
                        cb.set_font_size(ctrl_font);
                    }
                }
                SettingsPage::Updates => {
                    let rows = layout
                        .content_scrolled(8, settings_content_scroll)
                        .split_v([1.0; 8]);
                    if let Some(cb) = self.settings_autoupdate_check.as_mut() {
                        let r = rows[2];
                        cb.set_rect(r.x + r.w * 0.45, r.center_y(), r.w * 0.9, ctrl_h);
                        cb.set_font_size(ctrl_font);
                    }
                }
                SettingsPage::You => {
                    // First visit (or a re-entry): build the field boxes if needed + reload each from its stored value, so the form reflects the fleet-synced state.
                    if !self.you_fields_loaded {
                        self.load_you_fields(ctx.text);
                    }
                    // Expansion check each frame: filling the last Address/Email/Phone/… reveals its empty successor as you type (event-driven off the box content — no timers).
                    self.sync_expandable_fields();
                    // Every field except the always-shared display name carries its default-share checkbox; late-born fields (expansion, custom add) get theirs here.
                    self.ensure_share_checkboxes();
                    // Position every box using the SAME row plan + row rects the render pass draws through, so a box sits exactly under its label.
                    let plan = you_rows_plan(&self.you_fields);
                    for (i, row) in plan.iter().enumerate() {
                        let r = you_row_rect(&layout, settings_content_scroll, i);
                        match row {
                            YouRow::Field(idx) => {
                                let pf = &mut self.you_fields[*idx];
                                // Same three columns the render pass draws through: label | value | share box.
                                let three = r.split_h([0.4, 0.54, 0.06]);
                                let col = three[1];
                                if let Some(cb) = pf.share_cb.as_mut() {
                                    let sq = three[2];
                                    cb.set_rect(sq.center_x(), sq.center_y(), ctrl_h, ctrl_h);
                                    cb.set_font_size(ctrl_font);
                                }
                                if pf.tag_tb.is_some() {
                                    // Value + tag share the column: value left ~60%, tag right ~32% (a phone's home/work/custom).
                                    let boxr = fluor::region::Region::new(
                                        col.x + col.w * 0.02,
                                        col.y,
                                        col.w * 0.60,
                                        col.h,
                                    );
                                    let tagr = fluor::region::Region::new(
                                        col.x + col.w * 0.66,
                                        col.y,
                                        col.w * 0.32,
                                        col.h,
                                    );
                                    pf.tb.set_rect(
                                        boxr.center_x(),
                                        boxr.center_y(),
                                        boxr.w,
                                        ctrl_h * 1.2,
                                    );
                                    pf.tb.set_font_size(ctrl_font, ctx.text);
                                    let tag = pf.tag_tb.as_mut().unwrap();
                                    tag.set_rect(
                                        tagr.center_x(),
                                        tagr.center_y(),
                                        tagr.w,
                                        ctrl_h * 1.2,
                                    );
                                    tag.set_font_size(ctrl_font * 0.9, ctx.text);
                                } else {
                                    let boxr = col.center_h(0.92);
                                    pf.tb.set_rect(
                                        boxr.center_x(),
                                        boxr.center_y(),
                                        boxr.w,
                                        ctrl_h * 1.2,
                                    );
                                    pf.tb.set_font_size(ctrl_font, ctx.text);
                                }
                            }
                            YouRow::AddInput => {
                                let boxr = r.split_h([0.62, 0.38])[0].center_h(0.92);
                                if let Some(tb) = self.you_add_textbox.as_mut() {
                                    tb.set_rect(
                                        boxr.center_x(),
                                        boxr.center_y(),
                                        boxr.w,
                                        ctrl_h * 1.2,
                                    );
                                    tb.set_font_size(ctrl_font, ctx.text);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                SettingsPage::Diagnostics => {
                    let rows = layout
                        .content_scrolled(10, settings_content_scroll)
                        .split_v([1.0; 10]);
                    if let Some(tb) = self.settings_note_textbox.as_mut() {
                        let r = rows[7].center_h(0.95);
                        tb.set_rect(r.center_x(), r.center_y(), r.w, ctrl_h * 1.2);
                        tb.set_font_size(ctrl_font, ctx.text);
                    }
                    if let Some(cb) = self.settings_hardlogs_check.as_mut() {
                        let r = rows[9];
                        cb.set_rect(r.x + r.w * 0.45, r.center_y(), r.w * 0.9, ctrl_h);
                        cb.set_font_size(ctrl_font);
                    }
                }
                _ => {}
            }
        }
    }

    /// Submit the contacts-page textbox contents as an FGTW handle search. Called from Enter in `contacts_textbox` and from clicking `contacts_plus_btn`. Bails on empty input, on no `HandleQuery` available (init failure path), and on a search for the user's own attested handle (would just find their own device — no point). Successful Found results land in `tick()`'s drain loop and append to `self.contacts`. Persistence + UI transition into a search-in-flight visual state (the rotating-hourglass plus button) ride in subsequent slices.
    pub(super) fn submit_add_friend(&mut self) {
        let handle: String = match self.contacts_textbox.as_ref() {
            Some(tb) => tb.chars.iter().collect(),
            None => return,
        };
        if handle.is_empty() {
            return;
        }

        let typed_pid = crate::crypto::clutch::identity_party_id(
            &crate::types::Handle::to_identity_seed(&handle),
        );
        if self.contacts.iter().any(|c| c.handle_hash == typed_pid) {
            crate::log("add-friend: handle already in contacts");
            // Feedback instead of a silent no-op — the whole reason the search "looked broken". Callers request the redraw.
            self.ready_toast = Some("Already in your contacts".to_string());
            return;
        }
        // Self-contact: if the handle matches our own identity, create the contact directly — FGTW won't return our own record as a search result.
        let is_self = self.session.as_ref().map_or(false, |s| {
            crate::storage::contacts::derive_identity_seed(&handle) == s.identity_seed
        });
        if is_self {
            if let Some(session) = &self.session {
                let handle_text = crate::types::HandleText::new(&handle);
                let device_pubkey = self
                    .device_keypair
                    .as_ref()
                    .map(|kp| crate::types::DevicePubkey::from_bytes(*kp.public.as_bytes()))
                    .unwrap_or_else(|| crate::types::DevicePubkey::from_bytes([0u8; 32]));
                let contact =
                    crate::types::Contact::new(handle_text, session.handle_proof, device_pubkey);
                // No forced state: a conversation with zero remote participants is reachable and keyed BY DEFINITION, and every gate now derives that from the participant set rather than reading fields nobody maintains.
                crate::log("add-friend: self-contact created (zero remote participants — nothing to exchange)");
                self.contacts.push(contact);
                if let Some(storage) = self.storage.as_ref() {
                    if let Some(c) = self.contacts.last() {
                        if let Err(e) = crate::storage::contacts::save_contact(c, storage) {
                            crate::logf!("Failed to save contact: {}", e);
                        }
                    }
                }
                self.search_status =
                    Some((format!("added {handle}"), (*theme::SEARCH_FOUND_COLOUR)));
                if let Some(tb) = self.contacts_textbox.as_mut() {
                    tb.clear();
                }
                // Propagate to the rest of the fleet on the ADD edge — the same push the friend-add path fires. This was the one add that never pushed, so notes-to-self appeared only on the device that typed it until some unrelated pull's reconcile noticed (field, 2026-08-02).
                self.spawn_roster_push();
            }
            return;
        }

        // OFF-GRID add (docs/offgrid.md open house): the registry is unreachable (the relay pipe's liveness flag is the offline proxy — both are fgtw.org), so the registry's job is done by proximity instead. Arm the open house (group + cleartext creds in the TXT beacon — a deliberate, mode-gated act) and derive the handle's proof off-thread; the peer's pt_disc beacon matching that proof CREATES the contact with the beacon's device key + address, and the ceremony rides the group.
        if !crate::network::wfd::relay_reachable() {
            crate::logf!("add-friend: OFF-GRID — open house armed, listening for {} nearby", handle);
            let (tx, rx) = std::sync::mpsc::channel();
            let h = handle.clone();
            std::thread::spawn(move || {
                // ~1s memory-hard PoW (ihi::handle_to_proof) — never on the UI thread.
                let hp = crate::types::Handle::username_to_handle_proof(&h);
                let _ = tx.send((h, hp));
            });
            self.woods_add_rx = Some(rx);
            crate::network::wfd::start_open_house();
            self.search_status = Some((
                "off-grid: listening nearby...".to_string(),
                *theme::SEARCH_FOUND_COLOUR,
            ));
            if let Some(tb) = self.contacts_textbox.as_mut() {
                tb.clear();
            }
            self.change_focus(None);
            return;
        }

        if let Some(hq) = self.handle_query.as_ref() {
            crate::log("add-friend: searching FGTW");
            hq.search(handle);
            // Enter the search-in-flight visual state: rotating hourglass over the plus button, last result cleared. Defocus the textbox so further typing doesn't mutate the handle being searched.
            self.add_in_flight = true;
            self.search_status = None;
            self.change_focus(None);
        }
    }

    /// Copy `s` to the OS clipboard. Desktop uses arboard; Android has no clipboard JNI yet (returns false — a ClipboardManager bridge is a follow-up), Redox has no arboard backend. Returns true on success.
    pub(super) fn copy_to_clipboard(&mut self, s: &str) -> bool {
        #[cfg(all(not(target_os = "android"), not(target_os = "redox")))]
        {
            return arboard::Clipboard::new()
                .and_then(|mut clip| clip.set_text(s.to_string()))
                .is_ok();
        }
        #[cfg(target_os = "android")]
        {
            // Poll-bridge to Kotlin (same one-shot pattern as pending_apk_install): the Choreographer frame callback drains this into ClipboardManager.setPrimaryClip; Android 13+ shows the system "copied" overlay on its own.
            self.pending_clipboard_copy = Some(s.to_string());
            return true;
        }
        #[cfg(target_os = "redox")]
        {
            let _ = s;
            crate::log("clipboard: copy not wired on this platform yet");
            false
        }
    }

    /// Clipboard chord handler (desktop only). `op` is the lowercased character — "c" copy, "x" cut, "v" paste — acting on whichever textbox holds focus (launch handle or contacts search). Returns `Handled` when a textbox owned the focus, `Pass` otherwise (so the chord doesn't get eaten on a non-text screen). Copy/cut read `selected_text`; cut only deletes after the OS `set_text` succeeds, so a clipboard failure never silently destroys the selection. Paste inserts the clipboard string at the cursor (replacing any selection via `insert_str`). A launch-textbox edit clears a stale `Error` back to `Fresh`; the cursor blink reset is the caller's job.
    #[cfg(not(any(target_os = "redox", target_os = "android")))]
    pub(super) fn clipboard_chord(
        &mut self,
        op: &str,
        text: &mut fluor::text::TextRenderer,
    ) -> EventResponse {
        // Resolve focus thru the ONE textbox registry (`textbox_by_hit_mut`) — the old hand-list covered only the launch + contacts boxes, so Ctrl/Cmd+V silently no-oped in the compose box and every panel field (exactly the per-concern hand-list the registry doctrine forbids). Any focused textbox is a clipboard target; focus elsewhere (button, avatar, nothing) passes thru.
        let Some(focus) = self.focused else {
            return EventResponse::Pass;
        };
        // Captured BEFORE the registry's &mut self borrow: launch-error clearing + the AddDevice words filter both read state the borrow would lock.
        let on_launch = self
            .textbox
            .as_ref()
            .map(|t| Some(t.hit_id()) == self.focused)
            .unwrap_or(false);
        let words_filter = matches!(self.state, AppState::AddDevice);
        // The multi-line compose box lives outside the single-line registry — its clipboard branch first (paste is how long multi-line text usually ARRIVES).
        if self
            .message_textbox
            .as_ref()
            .is_some_and(|t| t.hit_id() == focus)
        {
            let tb = self.message_textbox.as_mut().unwrap();
            let mut edited = false;
            match op {
                "c" => {
                    if let Some(sel) = tb.selected_text() {
                        if let Ok(mut clip) = arboard::Clipboard::new() {
                            let _ = clip.set_text(sel);
                        }
                    }
                }
                "x" => {
                    if let Some(sel) = tb.selected_text() {
                        if let Ok(mut clip) = arboard::Clipboard::new() {
                            if clip.set_text(sel).is_ok() {
                                tb.delete_selection(text);
                                edited = true;
                            }
                        }
                    }
                }
                "v" => {
                    if let Ok(mut clip) = arboard::Clipboard::new() {
                        if let Ok(s) = clip.get_text() {
                            tb.insert_str(&s, text);
                            edited = true;
                        }
                    }
                }
                _ => return EventResponse::Pass,
            }
            if edited {
                self.blink_timer.start(Instant::now());
                self.scene_dirty = true;
            }
            return EventResponse::Handled;
        }
        // A busy field can't be the clipboard target: `sync_busy_freeze` releases focus before disabling it, so a frozen box never matches `self.focused` here.
        let Some(tb) = self.textbox_by_hit_mut(focus) else {
            return EventResponse::Pass;
        };

        let mut edited = false;
        match op {
            "c" => {
                if let Some(sel) = tb.selected_text() {
                    if let Ok(mut clip) = arboard::Clipboard::new() {
                        let _ = clip.set_text(sel);
                    }
                }
            }
            "x" => {
                if let Some(sel) = tb.selected_text() {
                    // Only delete after the clipboard accepts the text — a failed copy must not destroy the selection.
                    let copied = arboard::Clipboard::new()
                        .and_then(|mut clip| clip.set_text(sel))
                        .is_ok();
                    if copied {
                        tb.delete_selection(text);
                        edited = true;
                    } else {
                        crate::log("clipboard: copy failed, not cutting");
                    }
                }
            }
            "v" => {
                if let Ok(mut clip) = arboard::Clipboard::new() {
                    if let Ok(s) = clip.get_text() {
                        // Words entry accepts only letters and space — strip everything else from the paste (newlines/tabs become nothing; the camelCase/space tokenizer handles the rest).
                        let s: String = if words_filter {
                            s.chars()
                                .filter(|c| c.is_ascii_alphabetic() || *c == ' ')
                                .collect()
                        } else {
                            s
                        };
                        if !s.is_empty() {
                            tb.insert_str(&s, text);
                            edited = true;
                        }
                    }
                }
            }
            _ => {}
        }
        if edited && on_launch {
            self.clear_launch_error();
        }
        EventResponse::Handled
    }

    /// A launch-handle edit invalidates any prior attestation error OR an armed permanence confirmation — drop `Error`/`Confirm` back to `Fresh` so the message clears and the user can resubmit. Editing the handle IS the cancel gesture for the Confirm interstitial. No-op off the launch screen or in other states.
    pub(super) fn clear_launch_error(&mut self) {
        if matches!(
            self.state,
            AppState::Launch(LaunchState::Error(_))
                | AppState::Launch(LaunchState::Confirm)
                | AppState::Launch(LaunchState::KnownHandle)
        ) {
            self.state = AppState::Launch(LaunchState::Fresh);
            self.probed_session = None;
            self.probed_handle = None;
            if let Some(btn) = self.attest_btn.as_mut() {
                btn.set_label("Attest");
            }
        }
    }

    /// Encrypt + send the compose-box contents to the open contact, append it as an outgoing bubble, and persist. No-op unless a CLUTCH-Complete contact is open with a friendship chain and the box is non-empty. The crypto/wire/persist layers already exist (`FriendshipChains::prepare_send`, `StatusChecker::send_message`, `save_messages`); this is the UI→chain→network glue. Orb (chrome app-icon) tap. Returns true if it acted (caller redraws). Routed by screen: Ready → open the settings / about / help panel (its own screen with a nine-page nav rail); Settings → no-op (the dedicated back affordance exits). Launch / AddDevice / Conversation ignore the orb. The interim Ready → AddDevice entry moved onto the Fleet page's "Add device" pill.
    /// Which settings pages the nav rail shows, gated by attest state. Pre-attest (no session) there is no identity to configure — You/Fleet/Security/Recovery/Appearance/Notifications/Diagnostics all need one — so only About and Updates make sense. Post-attest: the full rail. The orb opens the panel on EITHER screen; this just narrows what's inside.
    pub(super) fn settings_pages(&self) -> &'static [SettingsPage] {
        if self.session.is_some() {
            &SettingsPage::ALL
        } else {
            &[SettingsPage::About, SettingsPage::Updates]
        }
    }

    pub(super) fn on_orb_click(&mut self) -> bool {
        match self.state {
            AppState::Ready => {
                self.change_focus(None);
                // Reload the profile field boxes from settings on open.
                self.you_fields_loaded = false;
                self.state = AppState::Settings(SettingsPage::You);
                true
            }
            // Pre-attest: the orb ALSO opens the panel, but `settings_pages` narrows it to About + Updates. Land on About. (Editing the handle already cancels the permanence interstitial, so opening settings from Launch is safe.)
            AppState::Launch(_) => {
                self.change_focus(None);
                self.settings_content_scroll = 0.0;
                self.state = AppState::Settings(SettingsPage::About);
                true
            }
            // In a Conversation the orb wears the friend's avatar — it opens the friend panel (same doctrine: the orb is a panel entry, never a direct action).
            AppState::Conversation => {
                self.change_focus(None);
                self.contact_boot_armed = false;
                self.state = AppState::ContactPanel(ContactPage::About);
                true
            }
            // Settings / AddDevice / ContactPanel fall thru: navigation off those screens is a dedicated control (back button), never the orb.
            _ => false,
        }
    }

    /// Send a message to the currently selected contact Returns true if message was sent successfully
    pub(super) fn textboxes_mut(&mut self) -> impl Iterator<Item = (TextboxRole, &mut Textbox)> {
        [
            self.textbox
                .as_mut()
                .map(|t| (TextboxRole::LaunchHandle, t)),
            self.contacts_textbox
                .as_mut()
                .map(|t| (TextboxRole::ContactsSearch, t)),
            self.settings_note_textbox
                .as_mut()
                .map(|t| (TextboxRole::SettingsNote, t)),
            self.you_add_textbox
                .as_mut()
                .map(|t| (TextboxRole::ProfileField, t)),
            self.unattended_confirm_tb
                .as_mut()
                .map(|t| (TextboxRole::UnattendedConfirm, t)),
        ]
        .into_iter()
        .flatten()
        .chain(self.you_fields.iter_mut().flat_map(|pf| {
            // Value box, then its optional tag box — both full registry members (tab / IME / blink / gestures).
            std::iter::once(&mut pf.tb)
                .chain(pf.tag_tb.as_mut())
                .map(|t| (TextboxRole::ProfileField, t))
        }))
    }

    /// Drive the disabled state of every textbox + its sibling button off the "query in flight" busy flags, in ONE place. A busy field returns `None` from its fluor capability accessors, so click / key / Tab / hover dispatch skip it for free — replacing the per-screen hand-rolled "swallow the click / force hover off / lock the field" code that used to live scattered across `on_event`. Symmetric across screens: the launch handle field + Attest button freeze while attesting (`!can_edit_handle()`), the contacts search box + plus button freeze while an add-friend search is in flight (`add_in_flight`).
    ///
    /// Order matters: if the currently-focused widget is about to be disabled, release focus FIRST (via `change_focus(None)`), because a disabled widget's `focus()` accessor returns `None` and `apply_focus_change` could no longer reach it to clear `set_focused`. Called every `tick`; `set_enabled` is idempotent so steady-state frames are free.
    pub(super) fn sync_busy_freeze(&mut self) {
        let busy_launch = matches!(self.state, AppState::Launch(ref s) if !s.can_edit_handle());
        let busy_contacts = self.add_in_flight;

        // Release focus before disabling the widget that holds it.
        let focused = self.focused;
        let focus_on_launch = self.textbox.as_ref().map(|t| t.hit_id()) == focused
            || self.attest_btn.as_ref().map(|b| b.hit_id()) == focused;
        let focus_on_contacts = self.contacts_textbox.as_ref().map(|t| t.hit_id()) == focused
            || self.contacts_plus_btn.as_ref().map(|b| b.hit_id()) == focused;
        if (busy_launch && focus_on_launch) || (busy_contacts && focus_on_contacts) {
            self.change_focus(None);
        }

        if let Some(tb) = self.textbox.as_mut() {
            tb.set_enabled(!busy_launch);
        }
        if let Some(btn) = self.attest_btn.as_mut() {
            btn.set_enabled(!busy_launch);
        }
        if let Some(tb) = self.contacts_textbox.as_mut() {
            tb.set_enabled(!busy_contacts);
        }
        if let Some(btn) = self.contacts_plus_btn.as_mut() {
            btn.set_enabled(!busy_contacts);
        }
    }

    /// True iff `id` belongs to one of photon's textboxes. Used by `change_focus` to detect focus transitions into / out of a text-input target so the Android IME show/hide signal can be triggered.
    pub(super) fn is_textbox(&mut self, id: Option<HitId>) -> bool {
        let Some(id) = id else {
            return false;
        };
        self.message_textbox
            .as_ref()
            .is_some_and(|t| t.hit_id() == id)
            || self.textboxes_mut().any(|(_, t)| t.hit_id() == id)
    }

    /// The textbox that currently holds focus, or `None`. The Android IME commit path routes the committed string here, since (unlike desktop keys) it has no focus-generic dispatcher.
    pub(super) fn focused_textbox_mut(&mut self) -> Option<&mut Textbox> {
        let focused = self.focused?;
        self.textboxes_mut()
            .find(|(_, t)| t.hit_id() == focused)
            .map(|(_, t)| t)
    }

    /// The textbox whose stamped hit id is `id`, or `None`. Screen-safe: off-screen boxes never stamp under the cursor, so a hit match is always the visible one. The single lookup every pointer-gesture path uses — covers every box (incl. `you_fields`) with no hand-list.
    pub(super) fn textbox_by_hit_mut(&mut self, id: HitId) -> Option<&mut Textbox> {
        if id == HIT_NONE {
            return None;
        }
        self.textboxes_mut()
            .find(|(_, t)| t.hit_id() == id)
            .map(|(_, t)| t)
    }

    /// Pointer press over hit `id`: if it's a textbox, focus it, place the caret under the pointer, and grab the text for the pan (drag = the text follows the finger — see `drag_pan_text`). Multi-tap streak: double → select word, triple → select paragraph (the whole single-line box). The tap interval comes from the OS (Android's ViewConfiguration double-tap timeout via JNI, X11 XSettings on Linux, 400 ms default). Returns true if a textbox was engaged (caller consumes the press so it can't start a window drag). Works for ANY textbox on ANY screen — the uniform pointer model, every platform.
    pub(super) fn textbox_press(&mut self, id: HitId, x: Coord, y: Coord) -> bool {
        // The multi-line compose box lives outside the single-line registry — its press branch first: focus (which raises the soft keyboard thru change_focus), caret at (x, y), the same multi-tap streak (double = word, triple = all), and the drag baseline for selection.
        if self
            .message_textbox
            .as_ref()
            .is_some_and(|t| t.hit_id() == id)
        {
            let now = Instant::now();
            let interval = fluor::host::os_input::double_click_interval();
            let continues = self.last_click_hit == id
                && self
                    .last_click_time
                    .map_or(false, |t| now.duration_since(t) <= interval);
            let streak = if continues {
                (self.click_streak + 1).min(3)
            } else {
                1
            };
            self.last_click_hit = id;
            self.last_click_time = Some(now);
            self.click_streak = streak;
            self.drag_select_hit = id;
            self.pointer_down = true;
            self.change_focus(Some(id));
            if let Some(tb) = self.message_textbox.as_mut() {
                match streak {
                    2 => {
                        let idx = tb.cursor_index_from_xy(x, y);
                        tb.select_word_at(idx);
                    }
                    3 => tb.select_all(),
                    _ => tb.pointer_press(x, y),
                }
            }
            return true;
        }
        // A busy-frozen box (`!is_enabled`) takes no pointer input — treat it as "not a textbox" so the press falls through to the normal consume, matching the pre-rework behaviour.
        if self
            .textbox_by_hit_mut(id)
            .map_or(true, |tb| !tb.is_enabled())
        {
            // Press landed off every (live) textbox — break any multi-tap streak so a later tap elsewhere-then-back doesn't count as a double.
            self.last_click_hit = HIT_NONE;
            self.drag_select_hit = HIT_NONE;
            return false;
        }
        let now = Instant::now();
        let interval = fluor::host::os_input::double_click_interval();
        let continues = self.last_click_hit == id
            && self
                .last_click_time
                .map_or(false, |t| now.duration_since(t) <= interval);
        let streak = if continues {
            (self.click_streak + 1).min(3)
        } else {
            1
        };
        self.last_click_hit = id;
        self.last_click_time = Some(now);
        self.click_streak = streak;
        self.drag_select_hit = id;
        self.pointer_down = true;
        self.pan_grab_x = x;
        self.change_focus(Some(id));
        let grab_scroll = self.textbox_by_hit_mut(id).unwrap().scroll_offset();
        self.pan_grab_scroll = grab_scroll;
        let tb = self.textbox_by_hit_mut(id).unwrap();
        match streak {
            2 => {
                let idx = tb.cursor_index_from_x(x);
                tb.select_word_at(idx);
            }
            3 => tb.select_all(),
            _ => tb.pointer_press(x),
        }
        true
    }

    /// Pointer drag while a textbox press is live: pan the TEXT with the pointer — `grab_scroll + (x − grab_x)`, unclamped, so the grabbed character (caret riding it) stays under the finger and the text can be carried infinitely either way; `tick()`'s spring pass eases it home after release. A word/paragraph select (streak ≥ 2) doesn't pan. Returns true if the text moved.
    pub(super) fn drag_pan_text(&mut self, x: Coord, y: Coord) -> bool {
        let id = self.drag_select_hit;
        if self.click_streak >= 2 {
            return false;
        }
        // Compose drag = SELECTION (the multi-line box selects like an editor, it doesn't pan).
        if let Some(tb) = self.message_textbox.as_mut().filter(|t| t.hit_id() == id) {
            tb.pointer_drag(x, y);
            return true;
        }
        let offset = self.pan_grab_scroll + (x - self.pan_grab_x);
        match self.textbox_by_hit_mut(id) {
            Some(tb) => tb.pan_scroll_to(offset),
            None => false,
        }
    }

    /// Pointer release: end the pan (the spring pass in `tick()` takes over easing any overshoot home).
    pub(super) fn textbox_release(&mut self) {
        self.pointer_down = false;
        self.drag_select_hit = HIT_NONE;
    }

    /// True iff both `[` and `]` are currently held. A bracket is "held" if its press timestamp is more recent than its release timestamp, OR the release was within [`CHORD_RELEASE_GRACE`] — that grace absorbs X11's habit of firing a synthetic Release for a held key the instant another key is pressed.
    pub(super) fn brackets_held(&self, now: Instant) -> bool {
        fn key_held(press: Option<Instant>, release: Option<Instant>, now: Instant) -> bool {
            match (press, release) {
                (Some(p), Some(r)) => p > r || now.duration_since(r) < CHORD_RELEASE_GRACE,
                (Some(_), None) => true,
                _ => false,
            }
        }
        key_held(self.chord_lb_press, self.chord_lb_release, now)
            && key_held(self.chord_rb_press, self.chord_rb_release, now)
    }

    /// SCORCHED EARTH: delete the ENTIRE Photon app dirs — vault rings, parked `.legacy` backups, orphan blobs, markers, every stray — both casings, config + data. A `*.vsf` filter used to leave blobs/ and strays standing after a "nuke everything" (field, 2026-08-20). Returns the count of top-level entries removed. Shared by the `[]n` (nuke, keep running) and `[]x` (nuke + exit) chords; `tag` prefixes the log lines. Does NOT touch the tohu session or any in-memory state — callers handle that.
    pub(super) fn dev_wipe_vault_files(tag: &str) -> usize {
        let mut count = 0usize;
        let wipe_dir = |dir: Option<std::path::PathBuf>, count: &mut usize| {
            let Some(base) = dir else { return };
            // Both casings: the unified `photon/` dir (device vault) AND the pre-unification `Photon/` parking lot. A wipe means NOTHING survives, parked or live.
            for sub in [crate::storage::APP.dir, "Photon"] {
                let app_dir = base.join(sub);
                let rd = match std::fs::read_dir(&app_dir) {
                    Ok(rd) => rd,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => {
                        // logf, not eprintln: stderr dies with a .app launch, and a wipe that couldn't even LIST the dir is exactly the forensic line a submitted log must carry (the 2026-08-25 resurrection hunt had only the count).
                        crate::logf!("{} WARN: read_dir {}: {}", tag, app_dir.display(), e);
                        continue;
                    }
                };
                for entry in rd.flatten() {
                    let p = entry.path();
                    let removed = if p.is_dir() {
                        std::fs::remove_dir_all(&p)
                    } else {
                        std::fs::remove_file(&p)
                    };
                    match removed {
                        Ok(()) => {
                            crate::logf!("{} deleted {}", tag, p.display());
                            *count += 1;
                        }
                        Err(e) => {
                            // The worst wipe outcome — a file that SURVIVED — was invisible on stderr.
                            crate::logf!("{} WARN: could not delete {}: {}", tag, p.display(), e)
                        }
                    }
                }
            }
        };
        #[cfg(not(target_os = "android"))]
        {
            wipe_dir(dirs::config_dir(), &mut count);
            wipe_dir(dirs::data_dir(), &mut count);
        }
        #[cfg(target_os = "android")]
        {
            // On Android the vault lives in the JNI-injected dirs, NOT dirs::config_dir() (which doesn't resolve there) — walk the actual (primary, shadow) pair so a clean genuinely nukes the vault.
            if let Some((primary, shadow)) = crate::storage::android_vault_dirs() {
                wipe_dir(Some(std::path::PathBuf::from(primary)), &mut count);
                if !shadow.is_empty() {
                    wipe_dir(Some(std::path::PathBuf::from(shadow)), &mut count);
                }
            }
        }
        count
    }

    /// Fully clean this device for a new owner / a fresh identity: nuke the on-disk vault (all `.vsf` — contacts, chains, keypairs, the cached fleet key) AND clear the tohu session (identity_seed + vault_seed + handle_proof), drop all in-memory state, and drop back to the attest screen. The device KEY is fingerprint-derived (not stored) so it survives — but with no identity bound and an empty vault the device is a blank slate: a new owner types their handle to attest fresh, or JOINs another fleet. This is `[]n` + `[]u` combined, exposed as a real (non-dev-chord) action for the Security page + the removed-device "start fresh" path; the `-1` broadcast signal tells the Android host to drop its sticky session too.
    /// Delete this identity's contacts-backup blobs from FGTW — both the fleet-keyed v1 slot and the device-keyed v0 slot this device may have left behind before the re-key.
    /// Runs on a detached thread: two HTTPS round trips must not stall a wipe the user just confirmed, and the outcome changes nothing locally. Every failure is logged and swallowed.
    pub(super) fn sweep_contacts_backup_blobs(&self) {
        let (Some(session), Some(keypair)) = (self.session.as_ref(), self.device_keypair.as_ref())
        else {
            return; // nothing attested — no identity seed, so no blob was ever written
        };
        let identity_seed = session.identity_seed;
        let device_secret = *keypair.secret.as_bytes();
        let kp = keypair.clone();

        // The fleet key is cached in the vault; absent (never joined / fan-out never landed) means no v1 blob was ever written.
        let fleet_key: Option<[u8; 32]> = self.storage.as_ref().and_then(|s| {
            s.read_addr(&crate::storage::vault_key("fleet_key", &session.vault_seed))
                .ok()
                .flatten()
                .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok())
        });

        std::thread::spawn(move || {
            use crate::network::fgtw::delete_blob_blocking;
            if let Some(fk) = fleet_key {
                let key = crate::storage::cloud::contacts_storage_key(&identity_seed, &fk);
                match delete_blob_blocking(&key, &kp) {
                    Ok(()) => crate::log("CLEAN: contacts backup (fleet-keyed) deleted from FGTW"),
                    Err(e) => crate::logf!("CLEAN: contacts backup delete failed: {}", e),
                }
            }
            // v0: the pre-re-key, device-bound slot. This is the ONLY moment it is deletable — the address needs this device's secret, which is about to be gone.
            let v0 = crate::storage::cloud::contacts_storage_key_v0_device_bound(
                &identity_seed,
                &device_secret,
            );
            match delete_blob_blocking(&v0, &kp) {
                Ok(()) => crate::log("CLEAN: legacy device-keyed contacts blob swept"),
                Err(e) => crate::logf!("CLEAN: legacy contacts blob sweep failed: {}", e),
            }
        });
    }

    pub(super) fn clean_device_for_reuse(&mut self) {
        // LastRites sweep for the contacts backup — BEFORE the wipe, because both keys die with it.
        // The v0 blob is device-bound: its address is BLAKE3(identity_seed ‖ device_secret), which only THIS device can compute, so this is the last moment it can ever be deleted. Miss it and the ciphertext is stranded on FGTW forever — unreadable by anyone and unreachable by any purge, exactly the problem `delete_avatar_blocking` exists to avoid for the wall.
        // The v1 blob is fleet-bound, so a sibling could also delete it; we do it here anyway because the departing device may be the last member.
        // Best-effort and off the critical path: an already-absent blob is the goal state, and a failure must never block the wipe.
        self.sweep_contacts_backup_blobs();

        let count = Self::dev_wipe_vault_files("clean");
        tohu::clear_session();
        self.session = None;
        self.private_s = crate::crypto::blind::PrivateS::None; // zeroized on overwrite
        self.contacts.clear();
        self.friendship_chains.clear();
        if let Ok(mut pks) = self.contact_pubkeys.lock() {
            pks.clear();
        }
        // Pairwise pong-seal keys are identity-flavoured too — the old identity's derived keys must not seal or open anything under the next one.
        if let Ok(mut keys) = self.pong_seal_keys.lock() {
            keys.clear();
        }
        self.storage = None; // next attest re-opens a fresh vault
                             // EVERY identity-flavoured RAM slot dies here (observed: one identity's avatar surfaced under a different identity after a wipe — the in-place reset only cleared what it knew about, and the settings cache kept feeding the OLD identity's avatar pin + name into the new session's pongs and wall sync). Desktop re-execs below anyway; Android's in-place reset is exactly this list, so the list must be COMPLETE.
        self.fleet_settings = None; // the big one: cached profile.avatar_pin / profile.name of the OLD identity
        if let Ok(mut g) = self.fleet_key_ram.lock() {
            *g = None; // the OLD identity's fleet key must not serve the next session's pages
        }
        self.device_avatar_pixels = None;
        self.device_avatar_scaled = None;
        self.device_avatar_scaled_diameter = 0;
        self.avatar_set_rx = None; // an in-flight avatar pick must not install under the next identity
        self.pending_fleet_key = None;
        self.probed_session = None;
        self.probed_handle = None;
        self.active_conversation = None;
        self.ready_toast = None;
        // THE RESURRECTION CLASS (field 2026-08-25, Android): the wipe deleted the vault and the FGTW backup, then the un-cleared RAM re-persisted everything — `conversations` still held every message row (a stuck un-ACK'd note came back with its attempt counter intact) and the next roster push re-uploaded the roster the wipe had just deleted. Android never re-execs, so this in-place list IS the wipe: every identity-flavoured slot below must die or the identity survives.
        self.conversations.clear();
        self.chat_replay_queue.clear();
        self.pending_chain_sends.clear();
        self.send_encrypt_busy.clear();
        if let Ok(mut sr) = self.sync_records.lock() {
            sr.clear(); // pong testimony of the OLD identity's rows
        }
        self.lane_rearm_cycles.clear();
        self.lane_reserve_bursts.clear();
        self.chain_pushed_osc.clear();
        self.lane_pushed_pos.clear();
        self.hist_rid_map.clear();
        self.history_serve.clear();
        self.blind_flip.clear();
        self.attach_progress.clear();
        self.attach_confirmed.clear();
        // Conversation view + drafts: a composed draft is identity content too.
        self.msg_hit_rows.clear();
        self.selected_msg = None;
        self.pending_delete = None;
        self.compose_reply_to = None;
        self.compose_edit_of = None;
        self.compose_react_to = None;
        self.msg_wrap = None;
        if let Some(tb) = self.message_textbox.as_mut() {
            tb.clear();
        }
        if let Some(tb) = self.contacts_textbox.as_mut() {
            tb.clear();
        }
        self.search_status = None;
        self.you_fields.clear();
        self.you_fields_loaded = false;
        // Avatar/orb caches beyond the device-avatar slots already cleared below.
        self.avatar_dl_started.clear();
        self.avatar_req_pending.clear();
        self.avatar_probe_cache.clear();
        self.egged_cache.clear();
        self.orb_contact = None;
        self.orb_key = None;
        self.orb_had_avatar = false;
        // Fleet plane: epochs, claims, spine machinery, roster cycle — all keyed to the old fleet.
        self.fleet_epoch = None;
        self.fleet_epoch_prev = None;
        self.fleet_focus_claim = None;
        self.fleet_attention = None;
        self.ckpt_mint_due = false;
        self.ckpt_spineless_holds = 0;
        self.ckpt_rows_base = None;
        self.ckpt_busy = false;
        self.ckpt_rx = None;
        self.ckpt_loaded = false;
        self.ckpt_last_attempt = None;
        self.roster_pull_rx = None;
        self.roster_push_rx = None;
        self.roster_push_queued = false;
        self.needs_initial_roster_pull = true;
        self.roster_pull_parked_under = None;
        self.roster_pull_exhausted = false;
        self.fleet_retired.clear();
        self.fleet_release_armed = None;
        self.fleet_lock_armed = None;
        self.fleet_unlock_armed = None;
        self.pending_unlock = None;
        self.pending_lock = None;
        self.settings_fleet_selected = None;
        self.successor_inflight.clear();
        self.scoped_regrant_pending.clear();
        self.self_avatar_recover_pending = None;
        self.fanout_grow_pending = false;
        self.keygen_fleet_gate_holding = false;
        self.seed_identity_count = 0;
        self.published_bell = None;
        // Peer registry: the old identity's mirrored phonebook slice.
        if let Some(ps) = self.peer_store.as_ref() {
            if let Ok(mut store) = ps.lock() {
                *store = crate::network::fgtw::PeerStore::new();
            }
        }
        self.peer_store_loaded = false;
        self.peer_store_persisted_len = 0;
        self.registry_converged_fold.clear();
        self.self_record_published_for = None;
        // Live call dies with the identity.
        self.active_call = None;
        // Bridge: locus/interrupt client state everywhere; host-side maps where the shell host exists.
        self.bridge_locus = None;
        self.bridge_int = None;
        #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
        {
            if let Some(p) = self.bridge_partials.as_ref() {
                if let Ok(mut m) = p.lock() {
                    m.clear();
                }
            }
            if let Some(f) = self.bridge_fg.as_ref() {
                if let Ok(mut m) = f.lock() {
                    m.clear();
                }
            }
        }
        // Pairing/join machinery: stop flags signalled, channels dropped, candidates gone.
        if let Some(stop) = self.add_device_stop.take() {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        if let Some(stop) = self.add_stop.take() {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.add_device_status.clear();
        self.add_device_candidates.clear();
        self.add_device_bound = None;
        self.add_device_wordcheck_text.clear();
        self.add_device_typo = None;
        self.add_device_checking = false;
        self.beacon_scan_active = false;
        self.add_device_rx = None;
        self.add_device_tx = None;
        self.lan_heard.clear();
        self.add_join_status.clear();
        self.add_join_words = None;
        self.add_join_rx = None;
        self.add_join_handle = None;
        self.launch_add_mode = false;
        self.add_in_flight = false;
        self.joiner_selected = false;
        // Drain every worker-result channel: an in-flight result of the OLD identity (a keygen completing, a decrypted row, a durable verdict) must not land in the NEW session.
        while self.clutch_keygen_rx.try_recv().is_ok() {}
        while self.clutch_kem_encap_rx.try_recv().is_ok() {}
        while self.clutch_ceremony_rx.try_recv().is_ok() {}
        while self.clutch_kem_decap_rx.try_recv().is_ok() {}
        while self.avatar_dl_rx.try_recv().is_ok() {}
        while self.attach_installed_rx.try_recv().is_ok() {}
        while self.hist_opened_rx.try_recv().is_ok() {}
        while self.chain_sync_opened_rx.try_recv().is_ok() {}
        while self.braid_rx_rx.try_recv().is_ok() {}
        while self.braid_tx_rx.try_recv().is_ok() {}
        while self.persist_done.1.try_recv().is_ok() {}
        while self.fleet_rotated_rx.try_recv().is_ok() {}
        while self.inbox_check_rx.try_recv().is_ok() {}
        if let Some(rx) = self.contact_members_rx.as_ref() {
            while rx.try_recv().is_ok() {}
        }
        if let Some(rx) = self.successor_rx.as_ref() {
            while rx.try_recv().is_ok() {}
        }
        if let Some(rx) = self.fleet_evt_rx.as_ref() {
            while rx.try_recv().is_ok() {}
        }
        self.pb_resolve_rx = None;
        self.pb_resolve_cursor = 0;
        self.diag_log_close();
        crate::network::status::set_profile_name(""); // pong slots: empty/zero = omitted
        crate::network::status::set_avatar_pin(&[0u8; 64]);
        self.pending_broadcast_signal = -1; // Android: drop the sticky session broadcast
        self.state = AppState::Launch(LaunchState::Fresh);
        // A wipe is the ultimate re-proof teardown: a retained handle turned "attest fresh" into "press the button" (field 2026-08-25 — Shred left the handle sitting in the attest field).
        self.clear_handle_for_reproof();
        crate::storage::device_binding::clear();
        crate::logf!("CLEAN: wiped {} vault file(s) + cleared session — device is a blank slate, ready to attest fresh or join another fleet", count);
        // Full process restart (desktop): the in-place reset leaves launch half-initialized (the wordmark went missing after a shred) and a wiped process still holds freed secrets in reachable memory anyway — exec into a pristine self via the same machinery a self-update uses. Android can't exec; its in-place reset stands.
        #[cfg(not(target_os = "android"))]
        {
            if let Ok(exe) = std::env::current_exe() {
                self.update_reexec = Some(exe);
            }
        }
    }

    /// Dispatch a chord action character (`a`, `h`, `p`, etc.) that was pressed while both brackets are held. Returns true if anything happened (caller should request a redraw); false for unknown letters (no-op fallthrough — no whitelist so new bindings only add to dispatch, not gating).
    pub(super) fn handle_chord_action(&mut self, ac: char, ctx: &mut Context) -> bool {
        use std::sync::atomic::Ordering;
        let mut acted = true;
        match ac {
            'h' => {
                self.show_hitmask = !self.show_hitmask;
                paint::DEBUG_SHOW_HITMASK.store(self.show_hitmask, Ordering::Relaxed);
                crate::logf!("[]h hitmask = {}", self.show_hitmask);
                if self.show_hitmask {
                    // xorshift32 seeded from process nanos → 256 random opaque RGBs stored in α + darkness. Fresh palette every toggle so distinct IDs always pop visually.
                    let seed = (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.subsec_nanos())
                        .unwrap_or(1))
                        | 1;
                    let mut s = seed;
                    self.debug_hit_colours.clear();
                    self.debug_hit_colours.reserve(256);
                    for _ in 0..256 {
                        s ^= s << 13;
                        s ^= s >> 17;
                        s ^= s << 5;
                        let r = (s >> 16) & 0xFF;
                        s ^= s << 13;
                        s ^= s >> 17;
                        s ^= s << 5;
                        let g = (s >> 16) & 0xFF;
                        s ^= s << 13;
                        s ^= s >> 17;
                        s ^= s << 5;
                        let b = (s >> 16) & 0xFF;
                        let visible = (r << 16) | (g << 8) | b;
                        let dark = visible ^ 0x00FFFFFF;
                        self.debug_hit_colours.push(0xFF000000 | dark);
                    }
                }
            }
            'p' => {
                let cur = paint::DEBUG_SKIP_PREMULT.load(Ordering::Relaxed);
                paint::DEBUG_SKIP_PREMULT.store(!cur, Ordering::Relaxed);
                crate::logf!("[]p skip-premult = {}", !cur);
            }
            'a' => {
                // Cycle: off (0) → grayscale (1) → force-opaque (2) → off.
                let cur = paint::DEBUG_SHOW_ALPHA.load(Ordering::Relaxed);
                let next = (cur + 1) % 3;
                paint::DEBUG_SHOW_ALPHA.store(next, Ordering::Relaxed);
                let label = match next {
                    0 => "off",
                    1 => "grayscale",
                    _ => "force-opaque",
                };
                crate::logf!("[]a show-alpha = {} ({})", next, label);
            }
            'c' => {
                let cur = paint::DEBUG_SKIP_CHROME.load(Ordering::Relaxed);
                paint::DEBUG_SKIP_CHROME.store(!cur, Ordering::Relaxed);
                if let Some(chrome) = self.chrome.as_mut() {
                    chrome.invalidate_chrome();
                }
                crate::logf!("[]c skip-chrome = {}", !cur);
            }
            'l' => {
                let cur = paint::DEBUG_SKIP_CONTROLS.load(Ordering::Relaxed);
                paint::DEBUG_SKIP_CONTROLS.store(!cur, Ordering::Relaxed);
                if let Some(chrome) = self.chrome.as_mut() {
                    chrome.invalidate_chrome();
                }
                crate::logf!("[]l skip-controls = {}", !cur);
            }
            'r' => {
                if let Some(chrome) = self.chrome.as_mut() {
                    chrome.invalidate_bg();
                    chrome.invalidate_chrome();
                }
                crate::logf!("[]r force-redraw");
            }
            'f' => {
                let cur = paint::DEBUG_SHOW_FPS.load(Ordering::Relaxed);
                paint::DEBUG_SHOW_FPS.store(!cur, Ordering::Relaxed);
                crate::logf!("[]f fps-strip = {}", !cur);
            }
            'w' => {
                let cur = paint::DEBUG_SHOW_DAMAGE.load(Ordering::Relaxed);
                paint::DEBUG_SHOW_DAMAGE.store(!cur, Ordering::Relaxed);
                crate::logf!("[]w damage-outline = {}", !cur);
            }
            'd' => {
                let cur = paint::DEBUG_SHOW_FADE.load(Ordering::Relaxed);
                paint::DEBUG_SHOW_FADE.store(!cur, Ordering::Relaxed);
                crate::logf!("[]d screen-decay = {}", !cur);
            }
            'b' => {
                let cur = paint::DEBUG_SHOW_OPAQUE_SCAN.load(Ordering::Relaxed);
                paint::DEBUG_SHOW_OPAQUE_SCAN.store(!cur, Ordering::Relaxed);
                crate::logf!("[]b opaque-scan tint = {}", !cur);
            }
            'n' => {
                // Nuke the local VAULT only — wipes every .vsf in the Photon app dirs (contacts, CLUTCH slots, ephemeral keypairs, friendship chains; also catches old-path strays and derivation-change orphans). Deliberately does NOT touch the tohu session: the identity_seed/vault_seed/handle_proof stay in memory + cache, so you remain attested on Ready with a freshly-empty vault. To clear the identity itself, use []u (de-attest). Only fires in development builds.
                let count = Self::dev_wipe_vault_files("[]n");
                // Drop the in-memory vault state so the UI reflects the wipe immediately. Keep the session + a live FlatStorage handle: it points at the now-empty dir and recreates files lazily on the next write, so the app stays usable without a relaunch.
                self.contacts.clear();
                self.friendship_chains.clear();
                if let Ok(mut pks) = self.contact_pubkeys.lock() {
                    pks.clear();
                }
                // Contacts are gone, so their pairwise pong-seal keys go with them (a re-add re-derives).
                if let Ok(mut keys) = self.pong_seal_keys.lock() {
                    keys.clear();
                }
                // Drop S too (zeroized) — the reset-recovery E2E is exactly "[]n then reconstitute from a friend's blind"; keeping it in RAM would fake the recovery.
                self.private_s = crate::crypto::blind::PrivateS::None;
                crate::logf!(
                    "[]n nuked {} vault file(s); session kept (still attested)",
                    count
                );
            }
            'u' => {
                // De-attest — clear the tohu session (identity_seed/vault_seed/handle_proof) and drop back to the attest screen, leaving the vault on disk intact. The identity is deterministic from the handle, so re-typing it re-derives the same roots. Mirror of []n: []u forgets WHO you are, []n forgets WHAT you've stored. Only fires in development builds.
                tohu::clear_session();
                self.session = None;
                self.private_s = crate::crypto::blind::PrivateS::None; // zeroized on overwrite — no identity, no S
                self.pending_broadcast_signal = -1; // Android: drop the sticky session broadcast.
                self.state = AppState::Launch(LaunchState::Fresh);
                self.clear_handle_for_reproof();
                crate::logf!("[]u de-attested; session cleared — re-type handle to re-attest");
            }
            'x' => {
                // Full clean-slate reset for the dev loop: nuke the vault ([]n), clear the session ([]u), then KILL the process so the window dies and the next launch starts truly fresh — no lingering in-memory state, no half-reset UI. The disk wipe is the part that must persist; everything else dies with the process, so we exit right after.
                let count = Self::dev_wipe_vault_files("[]x");
                tohu::clear_session();
                // Clear the device-identity BINDING marker too, or the next launch sees a still-bound device and shows "this device already carries an identity — type its handle to resume" (device_binding.vsf lives in the config dir, OUTSIDE the vault the wipe above walks, so it survives a vault nuke). []x means TRULY fresh; the proper Security-page shred (clean_device_for_reuse) already clears it, and this dev chord must match.
                crate::storage::device_binding::clear();
                crate::clear_log(); // wipe photon.log.vsf too — a clean relaunch leaves no trace
                eprintln!(
                    "[]x nuked {} vault file(s) + de-attested + cleared device binding + wiped logs; exiting for a clean relaunch",
                    count
                );
                std::process::exit(0);
            }
            _ => acted = false,
        }
        if acted {
            ctx.window.request_redraw();
        }
        acted
    }
}
