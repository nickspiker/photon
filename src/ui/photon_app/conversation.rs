//! Conversation + contact state — hints/orb/focus, conversation accessors, avatar sync and downloads, braid RX commit, chain syncs, history pages, and inbox drains.

use super::*;

/// The recency window behind the walk-away-gap fix (2026-08-18, Nick-approved): silent alert-duty discharge requires human input on this device within this span, checked ONLY at the message-arrival edge (an Instant comparison — never a scheduled timer). The cost side, accepted: passively watching a conversation past this span without touching anything means an arriving message dings the fleet (the watcher's own banner stays suppressed by the attended+attention gate).
const ATTENTION_RECENCY: std::time::Duration = std::time::Duration::from_secs(120);

impl PhotonApp {
    /// Apply a focus change: update `self.focused`, then walk the widget tree via `apply_focus_change` so the old + new widgets fire `set_focused(false/true)` and mark their caches dirty. Returns `true` if anything changed (caller decides whether to request a redraw — most callers do). Also drops a one-shot Android keyboard-show/hide request when focus enters or leaves a textbox; the Activity reads it via `FluorApp::wants_keyboard` after each touch and raises / dismisses the soft IME accordingly. Dismiss the standing hints (the desktop avatar prompt) and clear the transient search status. Called on any click or keystroke: hints are event-shown and interaction-cleared — never hover- or time-driven. The avatar prompt's dismissal is reset on each `Ready` entry.
    pub(super) fn clear_hints(&mut self) {
        self.hints_dismissed = true;
        self.search_status = None;
        // Toasts deliberately DON'T clear here (clicks and scrolls reach this path): the user may want to click around or zoom to READ the toast. Toasts clear on a keystroke/IME commit (clear_toast at those arms) or a screen change (the tick's discriminant watch) — still interaction-cleared, never time-based.
    }

    /// Clear the transient toast + its screen snapshot. Fired by plain keystrokes, IME commits, and the tick's screen-change watch — NEVER by clicks, scrolls, or zoom chords, so the toast survives the user zooming in to read it.
    pub(super) fn clear_toast(&mut self) {
        self.ready_toast = None;
        self.ready_toast_screen = None;
    }

    /// Pixels of the screen bottom currently covered by the Android soft keyboard (0 elsewhere / closed). The surface NEVER resizes for the IME (adjustNothing — so the full-screen harmonic mean IS the scale, by construction, no pinning); bottom-anchored interactive strips (the conversation compose bar + message list) subtract this to ride above the keyboard.
    pub(super) fn ime_lift(&self) -> f32 {
        #[cfg(target_os = "android")]
        {
            crate::platform::jni_android::ime_inset_px() as f32
        }
        #[cfg(not(target_os = "android"))]
        {
            0.0
        }
    }

    /// Point the top-left orb at the right subject: in a conversation it becomes the PEER's avatar with a ring in THEIR presence-tier colour (their online state, not ours); everywhere else it's the Photon brand orb with our own FGTW-connectivity ring. Diffed on (contact, has-avatar) so the Icon rebuild only fires on a real change, not every frame.
    pub(super) fn update_orb(&mut self) {
        let target: Option<usize> = match self.state {
            AppState::Conversation | AppState::ContactPanel(_) => self
                .active_contact()
                .filter(|&ci| !self.contacts[ci].is_sibling),
            _ => None,
        };
        // The diff key must cover EVERYTHING the orb renders from, because this early-return is the only thing between a state change and a stale orb: contact, avatar presence, the avatar PIN (a new picture always rotates it, so pixel churn needs no hashing), and the ring colour + brighten flag (connection tier — validated_path and the relay flag mutate with no event of their own). Keying on (contact, has-avatar) alone left the ring and a re-picked avatar frozen until a screen change rebuilt the orb (field, 2026-08-05).
        let has_avatar = target.map_or(false, |ci| self.contacts[ci].avatar_pixels.is_some());
        // Computed before the chrome borrow pins `self`. A zero-remote conversation is on this machine: LAN ring, always bright.
        let orb_has_remote = target.is_some_and(|ci| self.has_remote(&self.contacts[ci]));
        let orb_key = target.map(|ci| {
            let c = &self.contacts[ci];
            (
                ci,
                has_avatar,
                c.avatar_pin,
                self.row_ring_tier(c),
                c.is_online || !orb_has_remote,
            )
        });
        if orb_key == self.orb_key && target.is_some() == self.orb_contact.is_some() {
            return;
        }
        self.orb_key = orb_key;
        self.orb_contact = target;
        self.orb_had_avatar = has_avatar;
        let Some(chrome) = self.chrome.as_mut() else {
            return;
        };
        match target {
            Some(ci) => {
                let c = &self.contacts[ci];
                // VSF-RGB source (256² avatar, or the deterministic gradient placeholder) → α+darkness packed, exactly the brand orb's format, so the chrome renders it thru the identical pipeline.
                let (src, diam): (Vec<u8>, usize) = match c.avatar_pixels.as_ref() {
                    Some(px)
                        if px.len()
                            == crate::ui::avatar::AVATAR_SIZE
                                * crate::ui::avatar::AVATAR_SIZE
                                * 3 =>
                    {
                        (px.clone(), crate::ui::avatar::AVATAR_SIZE)
                    }
                    _ => {
                        let d = 64usize;
                        (
                            gradient_avatar_rgb(proof_gradient_seed(&c.handle_proof), d),
                            d,
                        )
                    }
                };
                let pixels: Vec<u32> = src
                    .chunks_exact(3)
                    .map(|p| {
                        0xFF00_0000
                            | (((255 - p[0]) as u32) << 16)
                            | (((255 - p[1]) as u32) << 8)
                            | ((255 - p[2]) as u32)
                    })
                    .collect();
                let ring = super::row_ring_tier_in(&self.contacts, c, orb_has_remote);
                let online = c.is_online || !orb_has_remote;
                chrome.app_icon = Some(fluor::host::icon::Icon {
                    width: diam as u32,
                    height: diam as u32,
                    pixels,
                });
                chrome.set_orb_tint(fluor::host::chrome::OrbTint::Custom {
                    ring,
                    brighten: online,
                });
            }
            None => {
                chrome.app_icon = self.photon_orb.clone();
                chrome.set_orb_tint(orb_tint_for(self.online));
            }
        }
    }

    pub(super) fn change_focus(&mut self, new: Option<HitId>) -> bool {
        if new == self.focused {
            // No focus change — but a re-tap on the ALREADY-focused textbox must still re-raise the keyboard (dismissed by a back-press) and reset the IME buffer. Fire those one-shots and return; skip the focus-walk (nothing moved).
            if self.is_textbox(new) {
                self.pending_keyboard_request = Some(true);
                self.pending_input_reset = true;
                return true;
            }
            return false;
        }
        let old = self.focused;
        let was_textbox = self.is_textbox(old);
        let is_textbox = self.is_textbox(new);
        #[cfg(feature = "development")]
        crate::logf!(
            "FOCUS: {} -> {} (textbox {} -> {})",
            format!("{:?}", old),
            format!("{:?}", new),
            was_textbox,
            is_textbox
        );
        if is_textbox {
            // ANY focus landing on a textbox raises the soft keyboard — not only the off→on transition. Tapping a textbox that's already focused (keyboard was dismissed by a back-press) must re-raise it; the old transition-only guard left the box focused with no keyboard and no way up. Leaving a textbox still requests hide.
            self.pending_keyboard_request = Some(true);
            // Entering a textbox also restarts IME input, dropping any stale composing buffer the soft keyboard held from a DIFFERENT screen's textbox (Samsung predictive keyboards resurrect it — "type 'the', switch screens, tap a box, 'the' reappears"). One-shot; the Activity calls InputMethodManager.restartInput.
            self.pending_input_reset = true;
        } else if was_textbox {
            self.pending_keyboard_request = Some(false);
        }
        self.focused = new;
        widget::apply_focus_change(self, old, new);
        // A textbox focus EDGE re-rasters the full bg layer: the focus glow rays extend past the pill into the noise backdrop, and the dirty-gated bg cache never repaints under them on its own — a deselected box's glow lingered baked into the background (the contacts search box "un-deselectable" look). One noise re-raster per focus change, same cost precedent as a screen change.
        if was_textbox || is_textbox {
            if let Some(chrome) = self.chrome.as_mut() {
                chrome.invalidate_bg();
            }
            self.scene_dirty = true;
        }
        // Restart blink so the cursor lands solid on the newly-focused textbox instead of mid-cycle dark. `start` resets the phase to the start of the visible half whether the timer was already running or not.
        self.blink_timer.start(Instant::now());
        true
    }

    // ───────── CLUTCH ceremony machinery (extracted verbatim from the retired src/ui/app.rs; only field-access seams adapted: device_keypair/event_proxy are Option here, user_identity_seed → session.identity_seed, window_dirty → the returned changed bool) ─────────

    /// OUR party id when this device participates in a SIBLING ceremony (fleet weave): device-derived, since all our devices share one handle_hash. `None` only pre-init (device_keypair unset).
    pub(super) fn our_sibling_pid(&self) -> Option<[u8; 32]> {
        self.device_keypair
            .as_ref()
            .map(|kp| crate::crypto::clutch::sibling_party_id(kp.public.as_bytes()))
    }

    /// `&self` convenience over [`ceremony_parked_by`] for sites that aren't mid-mutable-walk.
    pub(super) fn ceremony_parked(&self, c: &crate::types::Contact) -> bool {
        let our_device = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes());
        ceremony_parked_by(c, our_device, &sibling_presence_snapshot(&self.contacts))
    }

    /// CLUTCH-gate trust for a `sender_pubkey` offering/KEMing/proving against the contact resolved by conversation-token. For a FRIEND that contact is asked `knows_device` (fold-respecting: first-met pre-fold, any current fleet member post-fold). For a SIBLING it's different in kind: a sibling contact only knows its OWN device, so `knows_device` on one sibling can NEVER recognize a DIFFERENT sibling's offer (the "untrusted/removed device — dropping" braid-in stall). The right question for a sibling target is "is the sender any device of OUR OWN fleet" — the sibling set is exactly our other devices, so accept a sender that is another of our siblings (or, defensively, our own device echoed back over the relay).
    pub(super) fn sender_trusted_for(
        &self,
        contact: &crate::types::Contact,
        sender_pubkey: &[u8; 32],
    ) -> bool {
        if contact.is_sibling {
            let our_device = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes());
            our_device == Some(*sender_pubkey)
                || self
                    .contacts
                    .iter()
                    .any(|c| c.is_sibling && c.public_identity.key == *sender_pubkey)
        } else {
            contact.knows_device(sender_pubkey)
        }
    }

    /// OUR party id in a ceremony with `contact`: the identity PUBKEY for friends (the value they pin at first-met — never the seed, which must not travel or be stored anywhere but our own registers), the device-derived sibling pid for fleet siblings. Every slot lookup, conversation token, ceremony id, and chain index in a ceremony must use THIS, not `session.identity_seed` directly.
    pub(super) fn our_party_id(&self, contact: &crate::types::Contact) -> Option<[u8; 32]> {
        if contact.is_sibling {
            self.our_sibling_pid()
        } else {
            self.session
                .as_ref()
                .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
        }
    }

    // (Chains-first paths — chat/ACK receive — resolve our party id INLINE: whichever of (identity seed, sibling pid) is a participant. A &self helper can't be called there while the chains are mutably borrowed.)

    /// Does this contact's conversation reach anyone besides us? THE question every keygen, presence, probe and offer gate actually asks — a zero-remote conversation has nothing to exchange, nobody to ping, and no offline state to show. `false` when the session isn't up, which every gate treats as "not yet".
    pub(super) fn has_remote(&self, c: &crate::types::Contact) -> bool {
        self.our_party_id(c)
            .is_some_and(|us| c.remote_count(&us) > 0)
    }

    /// Zero remote participants — the conversation lives entirely on this device (notes-to-self). NOT the complement of [`Self::has_remote`]: with no session both are `false`, so gates stay closed and display falls back to the peer rendering.
    pub(super) fn is_zero_remote(&self, c: &crate::types::Contact) -> bool {
        self.our_party_id(c)
            .is_some_and(|us| c.remote_count(&us) == 0)
    }

    /// A row's HONEST ring tier — the general rule for every row: the best connectivity tier over the devices this conversation has to reach. For a friend row that is the friend (their own row's classification); for the self/notes row it is the fleet SIBLINGS — same classifier, different counterparty set, so a dead sync partner shows as a grey ring instead of a hardcoded always-LAN lie (the 13-vs-0 notes divergence was invisible precisely because self always rendered "connected"). No siblings = single-device fleet: the row lives here alone, nearest possible = LAN.
    pub(super) fn row_ring_tier(&self, c: &crate::types::Contact) -> u32 {
        super::row_ring_tier_in(&self.contacts, c, self.has_remote(c))
    }

    /// The conversation `self.contacts[ci]` stands for, if it has materialized. `None` before the session is up or before anything touched it.
    pub(super) fn conv_of(&self, ci: usize) -> Option<&crate::types::Conversation> {
        let c = self.contacts.get(ci)?;
        let id = c.conversation(&self.our_party_id(c)?).id();
        self.conversations.iter().find(|v| v.id() == id)
    }

    /// The contact row the active conversation is with — RESOLVED from the id, never stored. `None` when nothing is open or the participant left the roster, which is an honest answer where a stale index was a lie.
    pub(super) fn active_contact(&self) -> Option<usize> {
        let id = self.active_conversation?;
        (0..self.contacts.len()).find(|&ci| {
            let c = &self.contacts[ci];
            self.our_party_id(c)
                .is_some_and(|us| c.conversation(&us).id() == id)
        })
    }

    /// Open the conversation this contact row stands for.
    pub(super) fn open_conversation_with(&mut self, ci: usize) {
        self.active_conversation = self
            .contacts
            .get(ci)
            .and_then(|c| self.our_party_id(c).map(|us| c.conversation(&us).id()));
        // An armed reply/edit/react targets a row of the conversation it was armed IN — switching conversations disarms it.
        self.compose_reply_to = None;
        self.compose_edit_of = None;
        self.compose_react_to = None;
        // The open IS the fleet-wide claim edge: this device is now the conversation's active clearer, and every sibling learns it before the next friend message can ding them.
        self.broadcast_focus_claim(true);
    }

    /// Broadcast our ACTIVE-CLEARER claim (`active = true`) or its retraction for the OPEN conversation (notification design 2026-07-23). One frame to every sibling on the edge — open/focus-gain claims, close/focus-loss retracts — plus local adoption first, so our own ding gate agrees with what the fleet is about to hear. Newest osc wins everywhere: a claim from a device the user just sat down at displaces the old holder even if that window still sits OS-focused. Sibling/self screens never claim (their rows never ding). No-op pre-attest or with no conversation open.
    pub(super) fn broadcast_focus_claim(&mut self, active: bool) {
        let Some(kp) = self.device_keypair.clone() else {
            return;
        };
        let token = (|| {
            let id = self.active_conversation?;
            let c = self.contacts.iter().find(|c| {
                self.our_party_id(c)
                    .map(|us| c.conversation(&us).id() == id)
                    .unwrap_or(false)
            })?;
            if c.is_sibling {
                return None;
            }
            let us = self.our_party_id(c)?;
            Some(crate::crypto::clutch::derive_conversation_token(&[
                us,
                c.handle_hash,
            ]))
        })();
        let Some(token) = token else { return };
        // Lamport bump over BOTH slots: a fresh claim must outrank whatever claim AND attention osc the fleet has seen, or a clock-skewed sibling's stale slot could outrank the human's actual fingers forever (receivers require strictly-newer).
        let floor = self
            .fleet_focus_claim
            .map(|(_, _, c)| c)
            .into_iter()
            .chain(self.fleet_attention.map(|(_, c)| c))
            .max();
        let osc = floor.map_or(vsf::eagle_time_oscillations(), |f| {
            vsf::eagle_time_oscillations().max(f + 1)
        });
        let our_dev = *kp.public.as_bytes();
        if active {
            self.fleet_focus_claim = Some((token, our_dev, osc));
            // A claim IS attention — opening/refocusing a conversation is human input HERE. One frame moves both slots (siblings adopt the same on receipt), which also kills the attn/claim wire-reorder race.
            self.set_fleet_attention(Some((our_dev, osc)));
        } else if self
            .fleet_focus_claim
            .map_or(false, |(t, d, _)| t == token && d == our_dev)
        {
            self.fleet_focus_claim = None;
        }
        if let Ok(frame) = crate::network::fgtw::protocol::build_focus_vsf(
            &token,
            osc,
            active,
            kp.public.as_bytes(),
            kp.secret.as_bytes(),
        ) {
            self.dispatch_frame_to_siblings(frame);
        }
    }

    /// The ONE mutation point for the fleet-attention slot — mirrors "is it ours" into the desktop banner-gate atomic so the notify path (callable from any thread) never reads stale attention. Miss a mutation site and a focused-but-abandoned desktop stays silent; route everything here.
    pub(super) fn set_fleet_attention(&mut self, v: Option<([u8; 32], i64)>) {
        self.fleet_attention = v;
        #[cfg(not(target_os = "android"))]
        crate::platform::desktop_notify::set_attention_ours(self.attention_is_ours());
    }

    /// Do WE hold fleet attention? `None` (bootstrap/single-device) and pre-attest default TRUE — the legacy behavior: a lone device is always its own human's attention.
    pub(super) fn attention_is_ours(&self) -> bool {
        match (self.fleet_attention, self.device_keypair.as_ref()) {
            (Some((d, _)), Some(kp)) => d == *kp.public.as_bytes(),
            _ => true,
        }
    }

    /// Qualifying human input landed HERE while another device held the ball — take it. Adopt with a Lamport bump (max(now, seen+1)): local input supersedes ANY seen osc, so a clock-skewed sibling can never hold attention against the human's actual fingers. Broadcast one `attn` frame; holders emit nothing on further input (the transition edge is the only frame source). Then the housekeeping the gain implies: if a conversation is open+attended here, deterministically re-claim the clearer role (never trust a stale claim slot to spring back) and clear the away-period unread badge.
    pub(super) fn take_fleet_attention(&mut self) -> bool {
        if self.attention_is_ours() && self.fleet_attention.is_some() {
            return false;
        }
        let Some(kp) = self.device_keypair.clone() else {
            return false;
        };
        let our_dev = *kp.public.as_bytes();
        let osc = self
            .fleet_attention
            .map_or(vsf::eagle_time_oscillations(), |(_, seen)| {
                vsf::eagle_time_oscillations().max(seen + 1)
            });
        let stolen_from = self.fleet_attention.map(|(d, _)| d);
        self.set_fleet_attention(Some((our_dev, osc)));
        if stolen_from.is_some() {
            crate::logf!("ATTN: took the ball (osc {})", osc);
        }
        if let Ok(frame) = crate::network::fgtw::protocol::build_attention_vsf(
            osc,
            kp.public.as_bytes(),
            kp.secret.as_bytes(),
        ) {
            self.dispatch_frame_to_siblings(frame);
        }
        // Attention-gain housekeeping: resume the clearer role if we're looking at a conversation right now.
        if self.active_conversation.is_some()
            && matches!(
                self.state,
                AppState::Conversation | AppState::ContactPanel(_)
            )
            && crate::platform::attended_here()
        {
            self.broadcast_focus_claim(true);
            if let Some(ci) = self.active_contact() {
                self.clear_unread(ci);
            }
        }
        true
    }

    /// Sibling link-up heal: re-send our attention and active-clearer claim with their STORED oscs — never minted fresh. A reconnecting sibling may believe stale holders; these frames displace that by ordinary LWW. Critically, an *abandoned* device reconnecting must LOSE this exchange (the human's newer state at other devices outranks its stored oscs) — minting fresh here would yank the ball back to an empty chair.
    pub(super) fn reannounce_attention_state(&mut self) {
        let Some(kp) = self.device_keypair.clone() else {
            return;
        };
        let our_dev = *kp.public.as_bytes();
        if let Some((d, osc)) = self.fleet_attention {
            if d == our_dev {
                if let Ok(frame) = crate::network::fgtw::protocol::build_attention_vsf(
                    osc,
                    kp.public.as_bytes(),
                    kp.secret.as_bytes(),
                ) {
                    self.dispatch_frame_to_siblings(frame);
                }
            }
        }
        if let Some((tok, d, osc)) = self.fleet_focus_claim {
            if d == our_dev {
                if let Ok(frame) = crate::network::fgtw::protocol::build_focus_vsf(
                    &tok,
                    osc,
                    true,
                    kp.public.as_bytes(),
                    kp.secret.as_bytes(),
                ) {
                    self.dispatch_frame_to_siblings(frame);
                }
            }
        }
    }

    /// The open conversation itself, if it has materialized.
    pub(super) fn active_conv_mut(&mut self) -> Option<&mut crate::types::Conversation> {
        let id = self.active_conversation?;
        self.conversations.iter_mut().find(|v| v.id() == id)
    }

    /// Can THIS device write the braid for `contacts[ci]` itself — replicated chains with a live lane root (per-device lanes)? This is the capability every non-owner device gains from chain replication; the owner's local Complete+woven shape is checked beside it at each gate.
    pub(super) fn lane_transmit_capable(&self, ci: usize) -> bool {
        let Some(c) = self.contacts.get(ci) else {
            return false;
        };
        if c.is_sibling {
            return false;
        }
        c.friendship_id.map_or(false, |fid| {
            self.friendship_chains
                .iter()
                .any(|(id, ch)| *id == fid && ch.lane_capable())
        })
    }

    /// Can the open conversation dispatch from THIS device — a locally-woven chain, zero remote participants (loopback), a replicated chain this device writes on its own lane (per-device lanes), or COMPOSE-ANYWHERE (history + a fleet to forward thru)? THE one definition: the focus walk and the render both call it, where two hand-mirrored copies used to drift ("textbox appears but can't type", desktop 2026-07-26). A truly fresh un-clutched contact still answers false (nothing anywhere can transmit yet).
    pub(super) fn compose_ready(&self) -> bool {
        let Some(ci) = self.active_contact() else {
            return false;
        };
        let c = &self.contacts[ci];
        let has_history = self.conv_of(ci).is_some_and(|v| !v.messages.is_empty());
        let can_fleet_forward =
            !c.is_sibling && has_history && self.contacts.iter().any(|s| s.is_sibling);
        // A Complete SIBLING is always composable — it is EXACTLY what chain_transmit accepts for a sibling (lane_capable is false for siblings by construction, so Complete is the whole gate there). Without this, compose_ready collapsed to `chain_woven` alone for a sibling, and that runtime seal flag resets on restart — so a bridge conversation (a per-sibling contact) had NO compose box until a re-seal probe happened to land, and commands typed into the void vanished (field 2026-08-21, the bridge conversation that ate every command). The self/notes row stays covered by is_zero_remote; this covers the per-device sibling conversations the bridge opens.
        let sibling_sendable = c.is_sibling
            && c.clutch_state == crate::types::ClutchState::Complete
            && c.friendship_id.is_some();
        self.is_zero_remote(c)
            || c.chain_woven
            || self.lane_transmit_capable(ci)
            || can_fleet_forward
            || sibling_sendable
    }

    /// The conversation for `self.contacts[ci]`, materialized empty on first touch — so no caller ever branches on "does it exist yet". `None` only before the session is up.
    pub(super) fn conv_mut_of(&mut self, ci: usize) -> Option<&mut crate::types::Conversation> {
        let c = self.contacts.get(ci)?;
        let fresh = c.conversation(&self.our_party_id(c)?);
        let id = fresh.id();
        if let Some(pos) = self.conversations.iter().position(|v| v.id() == id) {
            return self.conversations.get_mut(pos);
        }
        self.conversations.push(fresh);
        self.conversations.last_mut()
    }

    /// Recompute the shared sync-records (last-received-time per conversation) from `friendship_chains` and publish them to the checker, for message retransmit.
    pub fn update_sync_records(&mut self) {
        use crate::network::fgtw::protocol::SyncRecord;

        let mut records = Vec::new();
        for (fid, chains) in &self.friendship_chains {
            // Max last_received across all lanes — kept for legacy peers as the single-tip fallback. Per-lane heads below are the precise version.
            let max_time = chains
                .last_received_times()
                .iter()
                .filter_map(|t| *t)
                .fold(0i64, |acc, t| if t > acc { t } else { acc });

            // Publish a record for EVERY conversation, even one we've received nothing in yet (tip 0): its absence used to hide our head from a peer whose given-up pending needed exactly that head to revive. The peer reads tip 0 / no lane head as "send from the anchor".
            let lane_heads = chains.lane_heads();

            // Anti-entropy digest over the conversation's rows: an ORDER-DEPENDENT rolling hash sorted by eagle_time (Conversation::anti_entropy_digest — the single source of truth, cached). Order matters: a mismatch means the peer is MISSING or has REORDERED a message, which the old order-free XOR fold hid. Digests equal ⇒ same messages in the same sequence; the pong receiver full-walks recovery on mismatch.
            // The conversation shares the friendship's id (both derive from the sorted participant set), so look it up by fid directly — a disjoint-field borrow that doesn't fight the &friendship_chains loop.
            let (row_count, row_digest) = self
                .conversations
                .iter_mut()
                .find(|v| v.id() == *fid)
                .map(|v| v.anti_entropy_digest())
                .unwrap_or((0, [0u8; 32]));
            records.push(SyncRecord {
                conversation_token: chains.conversation_token,
                last_received_osc: max_time,
                row_count,
                row_digest,
                lane_heads,
            });
        }

        // Update the shared provider
        let mut provider = self.sync_records.lock().unwrap();
        *provider = records;
    }

    /// Flag-day edge (docs/lanes.md): a keyed contact whose chains are GONE — a pre-v8 blob the loader rejected — can never speak again on its own, because Complete contacts are invisible to the keygen queue and nothing else re-keys unprompted. Reset the ceremony so the ordinary machinery mints fresh v8 chains; §4.2 parking keeps a fleet from racing itself, and the peer accepts the offer as a routine re-key whatever build it runs. Zero-remote conversations have no ceremony and are untouched.
    pub(super) fn reclutch_chainless_contacts(&mut self, why: &str) {
        for ci in 0..self.contacts.len() {
            let c = &self.contacts[ci];
            // Complete-and-chainless is the flag-day shape; Pending-and-chainless qualifies too when the pair COMPLETED a ceremony in some earlier life (the persisted completion prefix says so) — that is a device the first sweep build reset on BOTH sides, mid-storm, and a fresh add never has the prefix so it is never touched.
            let ever_completed = c.completed_their_hqc_prefix.is_some();
            let sweepable = match c.clutch_state {
                crate::types::ClutchState::Complete => true,
                crate::types::ClutchState::Pending => ever_completed,
                _ => false,
            };
            if !sweepable || !self.has_remote(c) {
                continue;
            }
            let missing = match c.friendship_id {
                Some(fid) => !self.friendship_chains.iter().any(|(id, _)| *id == fid),
                None => true,
            };
            if !missing {
                continue;
            }
            // ONE deterministic initiator per pair, or both sides reset simultaneously and cross offers forever — observed live (a field phone, 2026-08-02): 573KB offers ping-ponging between siblings every few seconds with ceremony_id mismatches, the continuous keygen/expand churn starving the phone's main thread into "Photon isn't responding". Lower key initiates (siblings compare device pubkeys, friends compare identity pids); the higher side stays Complete-but-chainless and takes the offer thru the established "peer lost their chains, accept re-key" responder path.
            let c = &self.contacts[ci];
            let we_initiate = if c.is_sibling {
                self.device_keypair
                    .as_ref()
                    .is_some_and(|kp| kp.public.as_bytes() < &c.public_identity.key)
            } else {
                self.our_party_id(c).is_some_and(|us| us < c.handle_hash)
            };
            let c = &mut self.contacts[ci];
            crate::logf!(
                "LANE: {} is keyed but holds no chains ({}) — {}",
                crate::fp(&c.handle_proof).as_str(),
                why,
                if we_initiate {
                    "re-clutch (we initiate)"
                } else {
                    "awaiting their offer (they initiate)"
                }
            );
            // Whatever round either posture holds is DISCARDED — a storm-era round left in place blocks the keygen queue (it only picks keyless contacts) while its offer keeps re-sending, which IS the churn.
            if let Some(ref mut keys) = c.clutch_our_keypairs {
                keys.zeroize();
            }
            c.clutch_our_keypairs = None;
            c.clutch_slots.clear();
            c.ceremony_id = None;
            c.clutch_state = if we_initiate {
                crate::types::ClutchState::Pending
            } else {
                // Responder posture: Complete keeps it out of the keygen queue; the initiator's offer lands thru the established Complete-without-keypairs re-key path.
                crate::types::ClutchState::Complete
            };
            c.chain_woven = false;
            c.probe_sent = false;
            c.their_probe_seen = false;
            c.chain_advanced_by_ack = false;
            c.clutch_offer_sent = false;
            c.friendship_id = None;
            c.clutch_round_started = None;
            if let Some(storage) = self.storage.as_ref() {
                let _ = crate::storage::contacts::save_contact(&self.contacts[ci], storage);
            }
        }
    }

    /// Spawn at most ONE CLUTCH keygen, for the first Pending contact that needs keypairs, but only if no keygen is already running. McEliece keygen is heavy; running several in parallel (e.g. after a multi-contact cloud merge on launch) starves the UI thread. Serializing to one-at-a-time keeps the app responsive — each completion frees the slot and `tick()` calls this again to start the next. Returns true if a keygen was spawned.
    pub(super) fn spawn_next_pending_keygen(&mut self) -> bool {
        if self.session.is_none() {
            return false;
        }
        // One keygen at a time.
        if self.contacts.iter().any(|c| c.clutch_keygen_in_progress) {
            return false;
        }
        // Eagle-time gate on re-key: a round whose keys read `None` but that STARTED recently is not a failure to re-key — it's a transient loss (a resume that hadn't restored yet, an in-flight round). Re-keying it mints a divergent round the peer never agreed to; instead wait, and only re-key once the round is genuinely stale. A contact that never started a round (`clutch_round_started == None`) is the legitimate initial-keygen case and fires immediately.
        let now = vsf::eagle_time_oscillations();
        let our_device = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes());
        // §4.2 one-CLUTCH-per-friendship: a friend claimed by ANOTHER of our devices PARKS here — its ceremony is the fleet's ceremony (see ceremony_parked_by for the full rules incl. the woven guard and the probed-before-takeover boot-race fix). An owner that is PROBED-offline is presence-driven takeover: the contact re-enters the queue and the pickup below re-claims it. Sibling weaves are per-device-pair by design — never parked.
        let siblings = sibling_presence_snapshot(&self.contacts);
        // FLEET-FIRST REJOIN (the re-clutch storm fix): while ANY fleet sibling still lacks a presence VERDICT (pong or 3-timeout — an evidence edge, never a timer), FRIEND keygens hold. A wiped device's restored contacts arrive Pending+keyless and the old code fired ceremonies at every friend within milliseconds — seconds before chain replication from an online sibling would have flipped them all Complete with no ceremony at all. Once every sibling is probed: online siblings ⇒ chains arrive and adoption drains the queue; all-offline ⇒ this is the identity's only live device and clutching is legitimately ours. Sibling PAIR-WEAVES are exempt — they are the very channel the chains replicate over.
        let sibling_probe_pending = self
            .contacts
            .iter()
            .any(|s| s.is_sibling && !s.locked_out && !s.presence_probed);
        // A conversation with no remote participants has nothing to exchange, so it never enters the queue. (This replaces a comparison against the raw identity SEED that could never match a pid — self was excluded from keygen only because something else forced its state Complete.)
        let next_idx = self.contacts.iter().position(|c| {
            self.has_remote(c)
                && (c.is_sibling || !sibling_probe_pending)
                && c.clutch_state == crate::types::ClutchState::Pending
                && c.clutch_our_keypairs.is_none()
                && !c.clutch_keygen_in_progress
                && !c.locked_out
                && c.clutch_round_started
                    .map_or(true, |t| now - t >= CLUTCH_ROUND_TTL_OSC)
                && !ceremony_parked_by(c, our_device, &siblings)
        });
        if next_idx.is_none() && sibling_probe_pending && !self.keygen_fleet_gate_holding {
            // Only worth a line when the gate is actually holding candidates back.
            let held = self
                .contacts
                .iter()
                .filter(|c| {
                    !c.is_sibling
                        && self.has_remote(c)
                        && c.clutch_state == crate::types::ClutchState::Pending
                        && c.clutch_our_keypairs.is_none()
                })
                .count();
            if held > 0 {
                crate::logf!("CLUTCH: fleet-first — holding {} friend keygen(s) until every sibling is probed (chains may replicate instead)", held);
                self.keygen_fleet_gate_holding = true;
            }
        }
        if !sibling_probe_pending && self.keygen_fleet_gate_holding {
            self.keygen_fleet_gate_holding = false;
            crate::log("CLUTCH: fleet-first gate released — every sibling probed");
        }
        if let Some(i) = next_idx {
            // Party id per contact: identity seed for friends, device-derived pid for fleet siblings.
            let Some(our_pid) = self.our_party_id(&self.contacts[i]) else {
                return false;
            };
            // Claim on pickup (unclaimed legacy contact, or takeover from a probed-absent owner): the claim rides the next roster push so siblings park + discard. LWW settles simultaneous claims; the loser adopts the winner's entry, discards its round, and parks.
            if !self.contacts[i].is_sibling {
                if let Some(ours) = our_device {
                    if self.contacts[i].ceremony_owner != Some(ours) {
                        // Belt-and-braces: never take over a WOVEN friendship even if a caller reaches here with one (ceremony_parked_by already excludes them) — the chain lives on the owner and a re-clutch clobbers the friend's side.
                        if self.contacts[i].ceremony_owner.is_some() && self.contacts[i].owner_woven
                        {
                            return false;
                        }
                        let old_owner = self.contacts[i].ceremony_owner;
                        let c = &mut self.contacts[i];
                        c.ceremony_owner = Some(ours);
                        c.roster_updated = now;
                        match old_owner {
                            Some(prev) => crate::logf!(
                                "CLUTCH: taking over this friendship's ceremony from absent owner {} ({})",
                                hex::encode(&prev[..4]),
                                crate::fp(&c.handle_proof).as_str()
                            ),
                            None => crate::logf!(
                                "CLUTCH: claiming this friendship's ceremony ({})",
                                crate::fp(&c.handle_proof).as_str()
                            ),
                        }
                        if let Some(storage) = self.storage.as_ref() {
                            let _ =
                                crate::storage::contacts::save_contact(&self.contacts[i], storage);
                        }
                        self.spawn_roster_push();
                    }
                }
            }
            let c = &mut self.contacts[i];
            c.clutch_keygen_in_progress = true;
            let (cid, their_hh) = (c.id.clone(), c.handle_hash);
            crate::log("CLUTCH: spawning keygen for Pending contact (serialized, one at a time)");
            self.spawn_clutch_keygen(cid, our_pid, their_hh);
            true
        } else {
            false
        }
    }

    /// Sync this device's own avatar with FGTW, newest-wins (off-thread). Call on attest success (handle_proof fresh). Replaces the old one-way "always upload": a blind upload would clobber a NEWER FGTW copy (e.g. one this same identity set on another device) with our stale local one. `sync_avatar_bidirectional_from_seed` compares the local cache's eagle-time creation stamp to the server copy's and uploads only if we're newer, downloads + re-caches if the server is. When the server wins, the freshly-cached avatar is delivered back over `avatar_dl_tx` with an EMPTY handle so the drain installs it as `device_avatar_pixels`. No-op without keypair / proof / session / storage.
    pub(super) fn spawn_avatar_sync(&self) {
        let (Some(kp), Some(session), Some(storage)) = (
            self.device_keypair.as_ref(),
            self.session.as_ref(),
            self.storage.as_ref().map(Arc::clone),
        ) else {
            return;
        };
        let Some(handle_proof) = self.our_handle_proof() else {
            return;
        };
        // Read the fleet-synced avatar pin (random key ‖ lookup) immutably; absent = no avatar set for this identity yet, so nothing to sync.
        let avatar_pin = match self
            .fleet_settings
            .as_ref()
            .and_then(|fs| fs.effective("profile.avatar_pin"))
            .and_then(crate::storage::fleet_settings::as_bytes)
        {
            Some(v) if v.len() == 64 => {
                let mut p = [0u8; 64];
                p.copy_from_slice(&v);
                p
            }
            _ => return,
        };
        let secret = kp.secret.clone();
        let identity_seed = session.identity_seed;
        let tx = self.avatar_dl_tx.clone();
        #[cfg(not(target_os = "android"))]
        let proxy = self.event_proxy.clone();
        std::thread::spawn(move || {
            use crate::ui::avatar::AvatarSyncResult;
            let result = crate::ui::avatar::sync_avatar_bidirectional_from_seed(
                &secret,
                &identity_seed,
                &avatar_pin,
                Some(&handle_proof),
                &storage,
            );
            match result {
                AvatarSyncResult::ServerNewer => {
                    // FGTW had a newer copy — it's now re-cached; load it and push to the UI.
                    crate::log("Avatar: FGTW copy newer — adopted it (startup sync)");
                    let pixels =
                        crate::ui::avatar::load_cached_avatar_from_seed(&identity_seed, &storage)
                            .map(|(_, p)| p);
                    if pixels.is_some() {
                        let _ = tx.send(crate::ui::avatar::AvatarDownloadResult {
                            owner: None, // self
                            pixels,
                        });
                        #[cfg(not(target_os = "android"))]
                        if let Some(p) = proxy.as_ref() {
                            let _ = p.send(crate::ui::PhotonEvent::NetworkUpdate);
                        }
                    }
                }
                AvatarSyncResult::LocalNewer => {
                    crate::log("Avatar: local newer — published to FGTW (startup sync)")
                }
                AvatarSyncResult::InSync => crate::log("Avatar: already in sync with FGTW"),
                AvatarSyncResult::ServerEmpty | AvatarSyncResult::NoLocalAvatar => {
                    crate::log("Avatar: nothing to sync (startup)")
                }
                AvatarSyncResult::Error(e) => {
                    crate::logf!("Avatar: startup FGTW sync skipped/failed: {}", e)
                }
            }
        });
    }

    /// Kick a background download of `handle`'s avatar from FGTW (once per session per handle). The fetch + decode runs off the UI thread (FGTW round-trip + dav1d decode); the result is delivered over `avatar_dl_tx` and installed by the drain in `check_status_updates`. No-op if storage isn't ready yet or we've already started a download for this handle this session. This is the peer Send a direct P2P AvatarRequest to a MUTUAL (CLUTCH Complete) peer, once per session per peer. The peer's `AvatarResponse` arrives via the status drain and installs on the matching contact. This is the "a friend's avatar comes from the friend" path; if no response lands within the fallback window the sweep escalates to FGTW. `sent_at` (eagle-time) is recorded so the sweep can time the fallback. No-op without a status checker (the pending marker is only set once the request is actually handed off, so a checker that arrives later still triggers the request).
    pub(super) fn spawn_avatar_request_p2p(
        &mut self,
        peer_addr: std::net::SocketAddr,
        recipient_pubkey: [u8; 32],
        sent_at: i64,
    ) {
        let Some(checker) = self.status_checker.as_ref() else {
            return;
        };
        self.avatar_req_pending.insert(recipient_pubkey, sent_at);
        checker.send_avatar_request(crate::network::status::AvatarRequestSend {
            peer_addr,
            recipient_pubkey,
        });
    }

    /// Scroll the open conversation so the row at `target_ts` sits centered — the reply-reference tap's jump. Reuses the render's own wrap cache (line counts per visible row) and mirrors its height math exactly thru the SHARED helpers (chat_row_visible / build_react_over), so the landing can't drift from what the renderer draws. A stale cache (different conversation / no render yet) or a missing target no-ops — the tap just does nothing rather than jumping wrong.
    pub(super) fn scroll_to_message(&mut self, ci: usize, target_ts: i64) {
        let Some(conv) = self.conv_of(ci) else {
            return;
        };
        let raw: &[crate::types::ChatMessage] = &conv.messages;
        let visible: Vec<&crate::types::ChatMessage> =
            raw.iter().filter(|m| chat_row_visible(raw, m)).collect();
        let Some((_, wrap_lines, _)) = self.msg_wrap.as_ref() else {
            return;
        };
        if wrap_lines.len() != visible.len()
            || self.msg_wrap.as_ref().is_some_and(|(k, _, _)| k.0 != ci)
        {
            return; // cache is for another conversation/row set — a jump from stale math lands wrong, so don't
        }
        let react_over = build_react_over(raw);
        // The same per-row metrics the render walk uses. line_h/intra derive from msg_size, which derives from unit — recover the pair from the stored view scale via the ratio the render fixes (line_h = msg_size*1.6, intra = msg_size*1.25); msg_size itself rides the wrap cache key as bits.
        let Some((key, _, _)) = self.msg_wrap.as_ref() else {
            return;
        };
        let msg_size = f32::from_bits(key.4);
        let line_h = msg_size * 1.6;
        let intra = msg_size * 1.25;
        let sel_key = self
            .selected_msg
            .filter(|(sci, _, _)| *sci == ci)
            .map(|(_, ts, out)| (ts, out));
        let mut dist_from_bottom = 0.0f32;
        let mut found: Option<f32> = None;
        for (vi, m) in visible.iter().enumerate().rev() {
            let lines_n = wrap_lines.get(vi).map(|l| l.len()).unwrap_or(1).max(1);
            let mut block = line_h + (lines_n as f32 - 1.0) * intra;
            if matches!(m.reference, Some((crate::types::RefKind::Reply, _))) {
                block += intra;
            }
            if row_has_reaction(&react_over, m.timestamp) {
                block += intra;
            }
            if sel_key.is_some_and(|(ts, out)| m.timestamp == ts && m.is_outgoing == out) {
                block += line_h * 3.0; // the open details strip occupies its slot
            }
            if m.timestamp == target_ts {
                found = Some(dist_from_bottom + block * 0.5);
                break;
            }
            dist_from_bottom += block;
        }
        let Some(center_dist) = found else {
            return; // target not in the visible stream (never synced here)
        };
        let scroll = (center_dist - self.msg_view_h * 0.5).clamp(0.0, self.msg_max_scroll);
        if let Some(v) = self.conv_mut_of(ci) {
            v.scroll_offset = scroll;
        }
        self.scene_dirty = true;
    }

    /// Zero this contact's unread counter — called at every site where their conversation becomes the active view (contact tap, panel back/Esc re-entry). Persists only on an actual change (off-thread, coalesced), so the common already-read path costs nothing. Interaction-cleared by doctrine: this is the ONLY way the counter ever goes down.
    pub(super) fn clear_unread(&mut self, ci: usize) {
        let dirty_id = match self.conv_mut_of(ci) {
            Some(conv) if conv.unread_count > 0 => {
                conv.unread_count = 0;
                Some(conv.id())
            }
            _ => None,
        };
        if let Some(id) = dirty_id {
            if let Some(pos) = self.conversations.iter().position(|v| v.id() == id) {
                self.persist_conv_state_async(pos);
            }
        }
    }

    /// half of the avatar feature — the self avatar loads from the local vault; peers fetch by handle.
    pub(super) fn spawn_avatar_download(&mut self, ci: usize) {
        let Some(c) = self.contacts.get(ci) else {
            return;
        };
        let (hp, party_id, avatar_pin) = (c.handle_proof, c.handle_hash, c.avatar_pin);
        let their_device = *c.public_identity.as_bytes();
        if self.avatar_dl_started.contains(&hp) {
            return;
        }
        let Some(storage) = self.storage.as_ref().map(Arc::clone) else {
            return;
        };
        // The scoped-blob reader key: the CLUTCH pair secret we share with this friend's device. It addresses AND opens our private slot, so no pin has to be announced to us at all — we simply look where only the two of us can look.
        let scoped_kek = self
            .device_keypair
            .as_ref()
            .map(|kp| *kp.public.as_bytes())
            .and_then(|ours| {
                crate::storage::fanout_pairs::load(&ours, &their_device, storage.as_ref())
            });
        if scoped_kek.is_none() && avatar_pin == [0u8; 64] {
            return; // no scoped slot to read and no legacy pin — nothing to fetch with
        }
        self.avatar_dl_started.insert(hp);
        let tx = self.avatar_dl_tx.clone();
        #[cfg(not(target_os = "android"))]
        let proxy = self.event_proxy.clone();
        std::thread::spawn(move || {
            // LOCAL-FIRST, always: the sweep's LocalCached plan promises "never touches the network", but the scoped-slot read used to run before any cache look — every boot re-fetched every scoped avatar, and an offline boot showed placeholders for faces sitting in the vault (Nick, 2026-08-12). The one party-id cache slot holds either form — raw AV1 (scoped write-through below) or the pin-sealed VSF (the pinned path's own cache) — and each decode rejects the other form cleanly, so try both before any network. The pin-rotation sweep evicts this slot, which is what keeps a changed avatar from being served stale.
            let cached = storage
                .read_addr(&crate::storage::vault_key("avatar", &party_id))
                .ok()
                .flatten();
            let mut pixels = cached.as_ref().and_then(|bytes| {
                crate::ui::avatar::decode_avatar_av1_to_display(bytes)
                    .map(|(_, px)| px)
                    .or_else(|| {
                        (avatar_pin != [0u8; 64])
                            .then(|| {
                                let mut key = [0u8; 32];
                                key.copy_from_slice(&avatar_pin[..32]);
                                crate::ui::avatar::load_avatar_from_bytes_with_key(bytes, &key)
                                    .map(|(_, px)| px)
                            })
                            .flatten()
                    })
            });
            // Scoped blob first (docs/scoped-blobs.md): our private slot names the ciphertext and carries its key. Falls back to the legacy pin only while avatars published under the old scheme are still out there — a friend who has re-set their avatar since is served entirely by the slot.
            if pixels.is_none() {
                pixels = scoped_kek
                    .and_then(|kek| {
                        // THEIR avatar, so the purpose carries THEIR pid — the friend is the publisher of the blob we are reading.
                        let raw = crate::ui::avatar_scoped::fetch_blocking(
                            &kek,
                            &crate::ui::avatar_scoped::avatar_purpose(&party_id),
                        )?;
                        let (_, px) = crate::ui::avatar::decode_avatar_av1_to_display(&raw)?;
                        // Write-through so the next boot is local (the vault encrypts at rest; the raw-AV1 form is what the cache read above tries first).
                        if let Err(e) = storage
                            .write_addr(&crate::storage::vault_key("avatar", &party_id), &raw)
                        {
                            crate::logf!("AVATAR: scoped cache write failed: {}", e);
                        }
                        crate::log("AVATAR: fetched from our scoped slot");
                        Some(px)
                    })
                    .or_else(|| {
                        (avatar_pin != [0u8; 64])
                            .then(|| {
                                crate::ui::avatar::download_avatar_pinned(
                                    &party_id,
                                    &avatar_pin,
                                    &storage,
                                )
                                .map(|(_, p)| p)
                            })
                            .flatten()
                    });
            }
            let _ = tx.send(crate::ui::avatar::AvatarDownloadResult {
                owner: Some(hp),
                pixels,
            });
            #[cfg(not(target_os = "android"))]
            if let Some(p) = proxy.as_ref() {
                let _ = p.send(crate::ui::PhotonEvent::NetworkUpdate);
            }
        });
    }

    /// Drain finished braid decrypts and commit each — the other half of the ChatMessage arm.
    pub(super) fn drain_braid_rx(&mut self) {
        while let Ok(d) = self.braid_rx_rx.try_recv() {
            self.commit_braid_rx(d);
        }
    }

    /// The commit half of a received chat frame, after its braid decrypt finished on a worker: re-resolve everything against CURRENT state, CAS the lane position, then run the exact post-decrypt pipeline the arm ran inline — parse (fork detector), strand resolve (hold), advance, gap-buffer replay, durable-ACK enqueue, row insert, notify, seal. Every early-out just drops the frame: the sender's retransmit ladder re-enters it through the arm's full gates, so a drop is never a loss.
    pub(super) fn commit_braid_rx(&mut self, d: BraidRxDecrypted) {
        let BraidRxDecrypted {
            conversation_token,
            lane,
            prev_msg_hp,
            timestamp,
            sender_addr,
            sender_pubkey,
            plaintext,
        } = d;
        let Some(our_handle_hash) = self
            .session
            .as_ref()
            .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
        else {
            return;
        };
        let our_sibling_pid = self.our_sibling_pid();
        let Some(our_device_pubkey) = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes())
        else {
            return;
        };
        // Deferrals out of the `chains` borrow — the same discipline the arm loop used, locally scoped and acted on in the tail below.
        let mut ack_enqueue: Option<(FriendshipChains, AckRequest)> = None;
        let mut fork_sibling_reset: Option<usize> = None;
        let mut fork_friend_rekey: Option<usize> = None;
        let mut sibling_push: Option<(usize, ChatMessage)> = None;
        // Call signal deferred past the `chains` borrow — the state machine takes &mut self (docs/calls.md).
        let mut call_signal_evt: Option<(usize, crate::call::signal::CallSignal, Option<[u8; 32]>, i64)> = None;
        let mut recv_seal_idx: Option<usize> = None;
        let mut persist_ci: Option<usize> = None;
        // BRIDGE host: a `$ ` command arrived as an ordinary sibling message — run it + reply AFTER the chains borrow ends (needs &mut self). Deferred like sibling_push.
        #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
        let mut bridge_run: Option<(usize, String)> = None;
        let mut conv_state_pos: Option<usize> = None;
        let mut replays: Vec<crate::network::status::StatusUpdate> = Vec::new();
        let mut need_sync = false;
        'commit: {
            let Some((_, chains)) = self
                .friendship_chains
                .iter_mut()
                .find(|(_, c)| c.conversation_token == conversation_token)
            else {
                crate::logf!(
                    "CHAT: decrypted frame's friendship vanished (token {}...) — dropped",
                    hex::encode(&conversation_token[..8])
                );
                break 'commit;
            };
            // Party-id seam, re-run: our participant id is the identity PARTY id (friends) or the sibling pid (siblings) — whichever the chain actually holds. The UNSHADOWED identity pid is kept for the conversation resolution below, which must NOT follow the chains' expression of us.
            let identity_hh = our_handle_hash;
            // TOTAL identity resolution — no silent exits. The normal path: one of our pids is a participant, the other participant is a known contact. EITHER half can be stale-era (a pre-flag-day ceremony's expression of us, OR a contact whose key has since migrated) — and the frame still DECRYPTED, because lanes key on wire labels, not participants: the crypto is fine, only the naming is stale. Breaking silently on any half was the field's decrypts-forever-never-ACKs loop (2026-08-11: one device re-decrypted the same retransmitted frame every ~15s for six-plus HOURS — no row, no ACK, no persist, nothing in its log). The fallback resolves the peer by matching participants against the contact list; only a set matching NO known contact drops, loudly.
            let (our_handle_hash, from_handle_hash, contact_idx) = {
                let in_set = |p: &[u8; 32]| chains.participants().contains(p);
                let us = if in_set(&identity_hh) {
                    Some(identity_hh)
                } else {
                    our_sibling_pid.filter(in_set)
                };
                let normal = us.and_then(|us| {
                    let them = *chains.other_participant(&us)?;
                    let idx = self.contacts.iter().position(|c| c.handle_hash == them)?;
                    Some((us, them, idx))
                });
                match normal {
                    Some(resolved) => resolved,
                    None => {
                        let Some((idx, them)) = chains.participants().iter().find_map(|p| {
                            self.contacts
                                .iter()
                                .position(|c| !c.is_sibling && c.handle_hash == *p)
                                .map(|idx| (idx, *p))
                        }) else {
                            crate::logf!("CHAT: SHADOW SEAM — stale-era participant set matches NO known contact; frame dropped (token {}...)", hex::encode(&conversation_token[..8]));
                            break 'commit;
                        };
                        crate::logf!("CHAT: SHADOW SEAM — stale-era participant set ({} half); peer resolved by contact match ({}) — this blob should re-key", if us.is_some() { "their" } else { "our" }, crate::fp(&self.contacts[idx].handle_proof));
                        // The chains' "us" is whatever the set holds beside them — kept only for the relationship-digest calls below, which must use the CHAINS' expression to stay comparable with the peer's.
                        let stale_us = chains
                            .participants()
                            .iter()
                            .find(|p| **p != them)
                            .copied()
                            .unwrap_or(them);
                        (stale_us, them, idx)
                    }
                }
            };
            // AUTH re-check: a refusal or lockout landing while the worker ran must still block the commit.
            if !self.contacts[contact_idx].knows_device(&sender_pubkey.key)
                || self.contacts[contact_idx].locked_out
            {
                crate::logf!("CHAT: decrypted frame's signer lost trust mid-flight — dropped");
                break 'commit;
            }
            // CAS: the lane must be EXACTLY where dispatch saw it. The expected prev still verifying proves no advance, no era swap, no adopt moved the lane while the worker ran — which also proves the decrypt's salt inputs were current, so a parse failure below IS fork evidence. Any mismatch means this plaintext came from dead state: drop it and never feed it to the fork detector; is_duplicate covers the raced-copy case (two UDP copies of one frame both dispatched pre-mark, the first committed).
            if chains.chain(&lane).is_none()
                || chains.verify_chain_link(&lane, &prev_msg_hp).is_err()
                || chains.is_duplicate(&lane, timestamp)
            {
                crate::logf!("CHAT: braid decrypt landed on moved lane state (raced duplicate or era adopt) — dropped; a live frame re-enters clean thru the arm");
                break 'commit;
            }
            // The conversation this frame lands in — resolved THRU THE CONTACT, the same derivation the loader, the persist snapshot, the page server, and the census all use. It used to materialize from chains.participants(): whenever the chains' expression of OUR half differs from the contact path's pid (a stale-era ceremony's participant set), that minted a SHADOW conversation — live rows accumulated in an object no loader fills and no persist snapshot reads, so they rendered all session and DIED at restart (field, 2026-08-11: a device advertised 92 rows from RAM while its disk table held 7). The chains stay crypto truth; the CONTACT is conversation truth.
            let conv_pos = {
                let conv_our_pid = if self.contacts[contact_idx].is_sibling {
                    match our_sibling_pid {
                        Some(p) => p,
                        None => break 'commit,
                    }
                } else {
                    identity_hh
                };
                let derived = self.contacts[contact_idx].conversation(&conv_our_pid);
                let chains_id =
                    crate::types::Conversation::new(chains.participants().iter().copied()).id();
                if chains_id != derived.id() {
                    crate::logf!("CHAT: SHADOW SEAM — chains derive conversation {} but the contact derives {}; rows land in the contact's (the chains carry a stale-era participant set)", hex::encode(&chains_id.as_bytes()[..4]), hex::encode(&derived.id().as_bytes()[..4]));
                }
                match self
                    .conversations
                    .iter()
                    .position(|v| v.id() == derived.id())
                {
                    Some(p) => p,
                    None => {
                        self.conversations.push(derived);
                        self.conversations.len() - 1
                    }
                }
            };
            // THE MESSAGE PACKAGE: one verified framed document — body, incorporated hp, woven times, the typed reference — parsed by name (message_package.rs). A failed parse is the fork detector's evidence exactly as the old bare-field parse was: the CAS above proved the decrypt inputs were current, so garbage here means divergent key material (or a version-skewed peer, which the same repair ladder converges).
            let pkg = match crate::network::message_package::parse_message_package(&plaintext) {
                Ok(p) => p,
                Err(e) => {
                    crate::logf!("CHAT: message package parse error: {}", e);
                    // SINGLE-WRITER LANE FORK — the loud error lanes.md promised: this lane has exactly ONE writer by enforced discipline, so garbage past chain-link verify means one writer's history decrypts two ways — an invariant break (key divergence under one label), never normal operation. Era stragglers are the one honest cause, and the convergence ladder below absorbs those; anything that escalates PAST it deserves this line in both fleets' logs, naming the lane.
                    crate::logf!(
                        "LANE FORK (single-writer!): lane {} pos {} era-genesis {} — garbage decrypt past verify; convergence gets first crack, escalation follows the streak",
                        hex::encode(&lane[..4]),
                        chains.lane_position(&lane).unwrap_or(0),
                        chains.genesis_osc
                    );
                    // FORK DETECTOR — now the SOLE fork evidence (gaps became pure transport; strand-miss holds instead of forking). A frame that passed signature + chain-link verify but decrypted to garbage means the two sides hold different key material at this position. Every non-fork cause is handled upstream, so re-key is the escalation, but CONVERGENCE GETS FIRST CRACK: the commonest real cause is a stale era (the peer re-keyed, we still hold old chains) — collapsing the ping backoff below forces a prompt head exchange, so the owner's chain-sync / era-supersede can adopt the new era before the streak escalates. A genuine fork keeps failing past that; era stragglers and stale-era holders converge and never reach re-key.
                    // Siblings repair via the fleet-key chain_reset at 2; FRIENDS re-key at 3 (no shared key to rebuild from, but a fresh ceremony is always legal: our new-keys offer hits their Complete-rekey path, history rows survive, recovery backfills after the re-weave). A re-key resets chain_woven, so the UI already surfaces it as "establishing the secure channel". Observed live: a woven pair forked mid-conversation — one side decrypted one message as garbage and every later one buffered "ahead" forever, greying every send (2026-07-25).
                    // Fresh-weave grace: LATE relay copies of a superseded era's frames straggle in for a minute after a re-key, and three of them re-keyed a 16-second-old weave (live pair, 2026-08-07). A just-woven chain cannot have forked — one writer per lane — so garbage inside the grace is stragglers, not evidence; a real fork keeps failing past it.
                    let era_grace_active = chains.genesis_osc > 0
                        && vsf::eagle_time_oscillations().saturating_sub(chains.genesis_osc)
                            < 120 * vsf::OSCILLATIONS_PER_SECOND as i64;
                    if let Some(contact) = self.contacts.get_mut(contact_idx) {
                        contact.chain_fail_streak = contact.chain_fail_streak.saturating_add(1);
                        // CONVERGE BEFORE RE-KEY: collapse the presence backoff on every garbage hit so the next sweep pings immediately — the pong carries heads and the fleet chain-sync rides the same edge, letting a stale-era holder adopt the peer's current era instead of destroying it with a re-key.
                        contact.ping_backoff = 0;
                        contact.last_pinged = None;
                        if contact.chain_fail_streak >= 2 && contact.is_sibling {
                            crate::logf!("CHAIN FORK SUSPECTED: {} — {} consecutive garbage decrypts past chain-link verify — initiating sibling chain reset", crate::fp(&contact.handle_proof), contact.chain_fail_streak);
                            fork_sibling_reset = Some(contact_idx);
                        } else if contact.chain_fail_streak >= 3
                            && !contact.is_sibling
                            && era_grace_active
                        {
                            crate::logf!("CHAIN FORK: garbage streak on a freshly-woven chain — era stragglers, holding the re-key");
                            contact.chain_fail_streak = 0;
                        } else if contact.chain_fail_streak >= 3
                            && !contact.is_sibling
                            && contact.ceremony_owner.is_some()
                            && contact.ceremony_owner != Some(our_device_pubkey)
                        {
                            // Mirror of the gap-streak rule: a non-owner's garbage streak is stale-era evidence about ITSELF, not the friendship — only the ceremony owner may re-key (§4.2), everyone else waits for the owner's chain-sync.
                            crate::logf!("CHAIN FORK: garbage streak but this device does not own the ceremony — stale era suspected, awaiting sibling chain-sync");
                            contact.chain_fail_streak = 0;
                        } else if contact.chain_fail_streak >= 3 && !contact.is_sibling {
                            crate::logf!("CHAIN FORK SUSPECTED: {} — {} consecutive garbage decrypts past chain-link verify — initiating friend re-key", crate::fp(&contact.handle_proof), contact.chain_fail_streak);
                            fork_friend_rekey = Some(contact_idx);
                        }
                    }
                    break 'commit;
                }
            };
            // A clean decrypt+parse clears the fork detector.
            if let Some(contact) = self.contacts.get_mut(contact_idx) {
                contact.chain_fail_streak = 0;
            }
            // An EMPTY body is legal now (a reaction retract) — the package parse itself is the validity gate.
            let message_text = pkg.body;
            let incorporated_hp = pkg.incorporated_hp;
            let woven_times = pkg.woven_times;
            let wire_reference: Option<(crate::types::RefKind, i64)> = pkg
                .reference
                .and_then(|(k, t)| crate::types::RefKind::from_wire(k).map(|k| (k, t)));

            // Hidden chain-weave probe: a reserved-marker message that proves the ratchet works but must show NO chat bubble. Everything else on the receive path (chain advance, set_last_plaintext, mark_received, ACK send) still runs so the sender's chain advances and dedup works — only the UI is suppressed.
            let is_chain_probe = message_text == crate::types::CHAIN_PROBE_MARKER;
            // An EDIT or REACTION row lands as an ordinary message (row, ACK, sync) but must not ALERT — the target bubble repaints; a chime/unread/scroll-jump for it would read as a new message that isn't there. (Whether a reaction should ding is a one-gate flip if the field wants it.)
            let is_edit_row = matches!(
                wire_reference,
                Some((
                    crate::types::RefKind::Edit | crate::types::RefKind::React,
                    _
                ))
            );

            crate::logf!(
                "CHAT: Decrypted message from {} ({}, {} chars, incorporated_hp={}...)",
                crate::fp(&from_handle_hash),
                if is_chain_probe {
                    "chain-weave probe"
                } else {
                    "text"
                },
                message_text.chars().count(),
                hex::encode(&incorporated_hp[..8])
            );

            // Compute plaintext hash for ACK
            let plaintext_hash = *blake3::hash(&plaintext).as_bytes();

            // Derive this message's hash pointer (for bidirectional tracking)
            use crate::types::friendship::derive_msg_hp;
            let msg_hp = derive_msg_hp(&prev_msg_hp, &plaintext_hash, timestamp);

            // The braid: resolve each woven eagle_time to its message content. The peer wove messages IT received — i.e. messages WE authored — so we resolve against our OUTGOING rows (is_outgoing == true). Both sides hold identical `content` for any such message → identical strands → the chains advance in lockstep. Sort by eagle_time so framing matches the sender's (which also sorted). A single device can't emit two messages at the same 704ps tick, so eagle_time is unique within our stream; the adversarial same-tick collision is not handled here (would need a content_hash tiebreak carried on the wire) — left as a known guard gap.
            // HOLD ON A STRAND MISS, NEVER SKIP: a woven row we don't hold yet (a sibling composed it and its replication hasn't landed) must NOT be silently dropped — a short strand vector changes strand_count in derive_fresh_link, so our advance diverges from the sender's and the lane forks permanently (silent, surfaced only as later garbage). Resolve strands FIRST, before any chain mutation; if any is missing, make zero mutations, don't ACK, collapse backoff so sibling replication catches us up, and let the sender's retransmit replay this frame once the row lands.
            let mut strand_miss: Option<i64> = None;
            let woven_strands: Vec<Vec<u8>> = {
                let mut times = woven_times.clone();
                times.sort_unstable();
                let mut strands = Vec::with_capacity(times.len());
                for t in times {
                    if let Some(m) = self.conversations[conv_pos]
                        .messages
                        .iter()
                        .find(|m| m.is_outgoing && m.timestamp == t)
                    {
                        strands.push(m.content.as_bytes().to_vec());
                    } else {
                        strand_miss = Some(t);
                        break;
                    }
                }
                strands
            };
            if let Some(t) = strand_miss {
                crate::logf!("LANE: braid strand miss from {} — no outgoing row at eagle_time {} yet; holding this frame (no advance, no ACK)", crate::fp(&from_handle_hash), t);
                let c = &mut self.contacts[contact_idx];
                c.ping_backoff = 0;
                c.last_pinged = None;
                // A strand miss IS row-lack evidence, and the SENDER provably holds the missing row (they just wove it — it's one of OUR OWN messages, held by them as an incoming row). Sibling replication heals a fleet that has the row somewhere, but a device that LOST its history — maybe with no siblings at all — can only get it back from the FRIEND. Arm the full history walk right here: the pages carry our outgoing rows home (original stamps, delivered flags), the retransmitted frame then resolves its strands on replay, and the hold releases. Waiting on sibling replication alone left one device re-decrypting the same frame every ~15s for six-plus hours (2026-08-11), its walk never arming because every digest-record path had its own silent gate.
                let conv = &mut self.conversations[conv_pos];
                if conv.history_recovery.as_ref().map_or(true, |r| r.complete) {
                    crate::logf!("LANE: strand miss arms the history walk — the friend holds our missing row(s)");
                    conv.history_recovery = Some(crate::types::HistoryRecovery {
                        oldest_recovered_osc: i64::MAX,
                        complete: false,
                        in_flight: None,
                        next_request_osc: 0,
                        urgent: true,
                        was_complete_before: false,
                        decrypt_fail_streak: 0,
                        parked_key_fp: None,
                    });
                }
                break 'commit;
            }
            let strand_refs: Vec<&[u8]> = woven_strands.iter().map(|s| s.as_slice()).collect();

            // Update the lane's last_plaintext for the next message's salt — the x-text ONLY (must match what the sender stored: salt source is text, never the full payload/pad).
            // Keyed by LANE LABEL: the pre-lane call here passed the party id, which no lane label ever equals, so the write no-opped and the salt stayed empty while the sender's moved — every second message on a lane garbage-decrypted (field, 2026-08-07).
            chains.set_last_plaintext(&lane, message_text.clone().into_bytes());

            // Update bidirectional entropy state (derive weave hash from full message context)
            chains.update_received_for_mixing(timestamp, msg_hp, &plaintext);

            // CALL BASKET CAPTURE (docs/calls.md): the lane key THIS frame decrypted under, taken pre-advance — for a call-offer row it is the doomed egg of the call-key basket (the advance below destroys it, which is exactly why it's forward-secret). A cheap copy on every frame; only the call-signal arm reads it.
            let rx_lane_key_pre_advance = chains.current_key(&lane).copied();

            // Advance their chain with the braid strands. our_plaintext = the decrypted x-text ONLY (must match the sender's process_ack, which advances with the stored salt-text — never the full payload/pad).
            let message_text_bytes = message_text.clone().into_bytes();
            let eagle_time_for_advance = vsf::EagleTime::from_oscillations(timestamp);
            chains.advance(
                &lane,
                &eagle_time_for_advance,
                &message_text_bytes,
                &strand_refs,
            );

            // Mark as received for deduplication (protects against UDP duplicates)
            chains.mark_received(&lane, timestamp);

            // Update hash chain state for next message verification
            chains.update_received_hash(&lane, msg_hp);
            crate::logf!(
                "CHAT: Updated hash chain for {} - msg_hp={}...",
                crate::fp(&from_handle_hash),
                hex::encode(&msg_hp[..8])
            );

            // Layer 1 gap-buffer drain: this message's msg_hp is now our last_received_hash, so any buffered message that was waiting on THIS as its predecessor is now contiguous. Replay them (front of the queue) so they're processed in order immediately — and each can cascade to fill the next gap when IT advances.
            let ready = chains.take_buffered_for(&msg_hp);
            if !ready.is_empty() {
                crate::logf!(
                    "CHAT: gap filled — replaying {} buffered message(s) after msg_hp={}...",
                    ready.len(),
                    hex::encode(&msg_hp[..8])
                );
                // A fill proves the pipeline is healthy — the no-fill counter starts over.
                self.contacts[contact_idx].gap_streak = (0, 0);
                for buf in ready {
                    replays.push(crate::network::status::StatusUpdate::ChatMessage {
                        conversation_token,
                        lane: buf.sender_handle_hash,
                        prev_msg_hp: buf.prev_msg_hp,
                        ciphertext: buf.ciphertext,
                        timestamp: buf.eagle_time,
                        sender_addr: buf.sender_addr,
                        // (buf.sender_addr is SocketAddr; matches the variant field)
                        sender_pubkey: crate::types::DevicePubkey::from_bytes(buf.sender_pubkey),
                    });
                }
            }

            // CRASH SAFETY, kept — relocated: disk is still the commit point and the ACK is still gated on it, but the write rides the chains writer and the ACK FIRES FROM IT after the write lands (durable-then-signal; the sync encrypt+IO here billed the render thread per received frame). The snapshot is taken NOW — at the advance — so nothing a later arm does to this chain rides along uncommitted. A failed write withholds the ACK: the sender retransmits and we re-process, the exact old skip-ACK semantics.
            let chains_commit_snapshot = chains.clone();
            // Flag to update sync records after borrow ends
            need_sync = true;

            // Add message to contact's message list and persist — UNLESS this is the hidden chain-weave probe, which advances/ACKs the chain but must never surface a bubble or chime. For the probe we flip `their_probe_seen` (their TX / our RX proven), PERSIST a hidden row, and try to seal the chain.
            // CALL SIGNALING (docs/calls.md): an offer/answer/hangup rides the lane as a hidden control row — persist it (re-ACK durable, the probe pattern), push it to our siblings (ring/stop fan-out), and hand it to the state machine WITH the pre-advance lane key (the basket's doomed egg, meaningful for offers).
            if let Some(sig) = crate::call::signal::CallSignal::parse(&message_text) {
                let sig_row =
                    ChatMessage::new_with_timestamp(message_text.clone(), false, timestamp)
                        .with_ack_hash(plaintext_hash);
                self.conversations[conv_pos].insert_message_sorted(sig_row.clone());
                persist_ci = Some(contact_idx);
                sibling_push = Some((contact_idx, sig_row));
                recv_seal_idx = Some(contact_idx);
                call_signal_evt = Some((contact_idx, sig, rx_lane_key_pre_advance, timestamp));
            } else
            // Hidden DELETE marker: the friend tombstoned a message — apply it here (either direction, matched by timestamp), persist a HIDDEN marker row for re-ACK durability (the probe pattern), and gossip the tombstoned row to our siblings. No bubble, no chime, no notify.
            if let Some(ts_str) = message_text.strip_prefix(crate::types::DELETE_MARKER_PREFIX) {
                let target_ts: i64 = ts_str.trim().parse().unwrap_or(0);
                {
                    let conv = &mut self.conversations[conv_pos];
                    let mut tombstoned: Option<ChatMessage> = None;
                    if let Some(m) = conv.messages.iter_mut().find(|m| {
                        m.timestamp == target_ts && !crate::types::is_control_content(&m.content)
                    }) {
                        if !m.deleted {
                            m.deleted = true;
                            tombstoned = Some(m.clone());
                        }
                    }
                    // The marker row itself (hidden, ack_hash-bearing) — a lost ACK re-ACKs from it.
                    let marker_row =
                        ChatMessage::new_with_timestamp(message_text.clone(), false, timestamp)
                            .with_ack_hash(plaintext_hash);
                    conv.insert_message_sorted(marker_row);
                    persist_ci = Some(contact_idx);
                    if let Some(row) = tombstoned {
                        if let Some((hash, _, _)) =
                            crate::types::parse_attachment_content(&row.content)
                        {
                            crate::storage::blob_delete(&hash);
                        }
                        crate::logf!(
                            "CHAT: friend deleted a message (ts {}) — tombstone applied + gossiped",
                            target_ts
                        );
                        sibling_push = Some((contact_idx, row));
                    } else {
                        crate::logf!("CHAT: friend delete marker for ts {} — no matching live row (already tombstoned or never held)", target_ts);
                    }
                    self.scene_dirty = true;
                }
                recv_seal_idx = Some(contact_idx);
            } else if is_chain_probe {
                if let Some(contact) = self.contacts.get_mut(contact_idx) {
                    contact.their_probe_seen = true;
                    // Attribute the probe to the ceremony whose chain just decrypted it, so a completion landing microseconds later can tell "the peer's probe for THIS ceremony" from "a stale seal for the chain we just replaced".
                    contact.their_probe_ceremony = contact.ceremony_id;
                }
                // Persist the probe as a HIDDEN rarangi row carrying its ack_hash: without a row the duplicate handler has nothing to re-ACK from, so a probe whose ACK was lost froze the sender's chain at the pre-probe position forever — the sibling weave fork of 2026-07-23. Every UI/history/preview path already filters CHAIN_PROBE_MARKER, so the row never surfaces; it exists purely as the durable dedup + re-ACK record. No chime, no sibling push (probes stay device-pair-local).
                let probe_row = ChatMessage::new_with_timestamp(
                    crate::types::CHAIN_PROBE_MARKER.to_string(),
                    false,
                    timestamp,
                )
                .with_ack_hash(plaintext_hash);
                self.conversations[conv_pos].insert_message_sorted(probe_row);
                persist_ci = Some(contact_idx);
                crate::log("CHAIN-PROBE: received peer's chain-weave probe — RX chain proven");
                recv_seal_idx = Some(contact_idx);
            } else if let Some(contact) = self.contacts.get_mut(contact_idx) {
                // Any real received message means the chain is demonstrably working end-to-end in at least the RX direction — belt-and-suspenders toward woven.
                contact.their_probe_seen = true;
                contact.their_probe_ceremony = contact.ceremony_id;
                // A real message that DECRYPTED and advanced the chain is DEFINITIVE proof the ratchet works — stronger than the hidden probe ever was. Seal here unconditionally on a Complete contact, WITHOUT waiting for chain_advanced_by_ack. That flag is runtime-only and resets on reload, so a chain that completed but never sealed before a restart (probe lost, or the seal raced) reloaded chain_woven=false with no way back: the compose box stayed hidden, so no outgoing message could ever set chain_advanced_by_ack, so it could never seal — a functional chain locked out of composing forever (observed after a peer's re-attest, 2026-07-25). Receiving a decryptable message breaks that deadlock.
                if !contact.chain_woven
                    && contact.clutch_state == crate::types::ClutchState::Complete
                {
                    contact.chain_advanced_by_ack = true;
                }
                // Snapshot the facts the gates below need, so the &mut contact borrow ends here (the claim check walks self.contacts).
                let contact_is_sibling = contact.is_sibling;
                let sender_name = contact.display_name();
                // BRIDGE host capture: a sibling conversation is a chat-as-shell, so an incoming line from a sibling is a command to run on this box (no `$` — the shell already prompts; Nick 2026-08-21) — EXCEPT a row whose TYPED reference is BridgeOut, which is a reply coming back and must NOT re-execute (else output bounces). Capture the candidate here (before message_text moves); it is promoted to an actual run ONLY if the row is NEW (is_new_row, computed below) — a re-served/duplicate/history-backfilled command must never re-execute, or a reconnect would replay the entire command history (Nick 2026-08-22).
                #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
                let bridge_cmd_candidate: Option<String> = if contact_is_sibling
                    && !is_chain_probe
                    && !matches!(wire_reference, Some((crate::types::RefKind::BridgeOut, _)))
                {
                    let cmd = message_text
                        .strip_prefix("$ ")
                        .or_else(|| message_text.strip_prefix("$\t"))
                        .unwrap_or(&message_text)
                        .to_string();
                    (!cmd.trim().is_empty()).then_some(cmd)
                } else {
                    None
                };
                // Use actual eagle_time and sorted insert for correct chronological order
                let mut msg = ChatMessage::new_with_timestamp(
                    message_text,
                    false,     // is_outgoing = false (received)
                    timestamp, // Use message's actual eagle_time, not current time
                )
                // Persist the ACK hash so a later duplicate (our ACK was lost) can be re-ACKed from storage — keeps the sender's chain from stalling.
                .with_ack_hash(plaintext_hash);
                // The wire's typed reference lands ON THE ROW — without this the sender saw its own reply hint (the send path stamps its row) while the receiver's copy arrived bare (field, 2026-08-09: "responses don't show the hinted message on the receive side").
                msg.reference = wire_reference;

                // Unread gate: is the user plausibly looking at THIS conversation right now? "Looking" = this contact's conversation (or its contact-scoped panel) is the active view AND, on desktop, the window is visible + focused. Event-shown, interaction-cleared doctrine: the counter only ever moves on a message landing or the user opening the conversation — no timers anywhere. Computed BEFORE the insert so the fleet alert-duty flag can ride the row into the sibling push.
                let conversation_open = matches!(
                    self.state,
                    AppState::Conversation | AppState::ContactPanel(_)
                ) && self.active_conversation
                    == Some(self.conversations[conv_pos].id());
                // RECENCY GUARD (2026-08-18, Nick-approved): silent discharge additionally requires the human to have TOUCHED this device recently. The walk-away gap — leaving a screen parked on a conversation — produces no input edge anywhere in the fleet, so a parked-but-attended window would otherwise silently swallow every arriving message (monotone notified=true rides the sibling push; unrecoverable). NOT a scheduled timer: an Instant comparison evaluated only at this message-arrival edge — the arriving message is the clock.
                let fresh = self
                    .last_interaction
                    .map_or(false, |t| t.elapsed() <= ATTENTION_RECENCY);
                // "Looking" = this conversation is the active view, the platform says a human plausibly sees it (desktop: window visible+focused; Android: Activity foregrounded — either alone is not looking), we hold FLEET ATTENTION, and the touch is recent. Attention: the human's newest input is the only trustworthy evidence of which device they're at — a focused-but-abandoned screen must not discharge alert duty while they're demonstrably elsewhere.
                let looking = conversation_open
                    && crate::platform::attended_here()
                    && self.attention_is_ours()
                    && fresh;
                // FLEET layer (notification design 2026-07-23): a LIVE sibling claim on this conversation means THAT device is watching — it discharges the duty; we stay silent. The claim must be another device's (ours is implied by `looking`), its holder must still be present (the offline verdict voids ghosts), AND the holder must also hold fleet attention — a claim whose device the human walked away from suppresses nobody.
                let claimed_elsewhere = self.fleet_focus_claim.map_or(false, |(tok, dev, _)| {
                    tok == conversation_token
                        && self
                            .device_keypair
                            .as_ref()
                            .map_or(false, |kp| *kp.public.as_bytes() != dev)
                        && self.fleet_attention.map_or(true, |(ad, _)| ad == dev)
                        && self.contacts.iter().any(|c| {
                            c.is_sibling && !c.locked_out && c.knows_device(&dev) && c.is_online
                        })
                });
                // Exactly-once duty: `looking` = we are the clearer (discharge silently); a fresh live ding also discharges. Either way the flag is set BEFORE the insert + sibling push, so every forwarded copy arrives pre-discharged and no other device re-dings.
                let will_ding =
                    !contact_is_sibling && !looking && !claimed_elsewhere && !is_edit_row;
                if looking || will_ding {
                    msg.notified = true;
                }
                // STALE-HOLDER BALL DROP: our claim stands on this conversation but we are NOT its live clearer anymore (walked away past the recency window, attention stolen, or a missed blur edge). The arriving message is the edge that discovers it — retract NOW, so every sibling whose independently-received copy sat suppressed under our claim gets the retraction and its drop-sweep chirps. Without this, one lost frame (or the walk itself) leaves our claim muting the fleet with no bound.
                if !looking && !is_edit_row {
                    if let (Some(kp), Some((t, d, cur))) =
                        (self.device_keypair.clone(), self.fleet_focus_claim)
                    {
                        if t == conversation_token && d == *kp.public.as_bytes() {
                            crate::log(
                                "FOCUS: stale holder — dropping the ball at the message edge",
                            );
                            self.fleet_focus_claim = None;
                            // Built directly for THIS token (not via broadcast_focus_claim, which derives from the OPEN view and would no-op on a lingering claim). Lamport-bumped so the retraction outranks the claim at every sibling regardless of clock skew.
                            let osc = vsf::eagle_time_oscillations().max(cur + 1);
                            if let Ok(frame) = crate::network::fgtw::protocol::build_focus_vsf(
                                &conversation_token,
                                osc,
                                false,
                                kp.public.as_bytes(),
                                kp.secret.as_bytes(),
                            ) {
                                self.dispatch_frame_to_siblings(frame);
                            }
                        }
                    }
                }
                let conv = &mut self.conversations[conv_pos];
                // NEW row vs a re-delivery of one we already hold — same (timestamp, content) identity the insert's dedup uses. A re-served/dual-path duplicate must never bump unread or ding: a sender stuck in a re-serve loop rang the receiver every cycle forever (field 2026-08-21, "constant dings"), and every ring inflated the unread count for a message the human already had.
                let is_new_row = !conv
                    .messages
                    .iter()
                    .any(|m| m.timestamp == msg.timestamp && m.content == msg.content);
                conv.insert_message_sorted(msg.clone());
                if !is_edit_row && is_new_row {
                    conv.scroll_offset = 0.0; // Scroll to show new message (an edit repaints in place)
                }
                self.scene_dirty = true;
                // Promote a captured bridge command to an actual run ONLY on first receipt — a re-serve/duplicate/history-backfill must never re-execute (Nick 2026-08-22).
                #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
                if is_new_row {
                    if let Some(cmd) = bridge_cmd_candidate {
                        bridge_run = Some((contact_idx, cmd));
                    }
                }

                // Persist (async — see persist_hashes)
                persist_ci = Some(contact_idx);

                if !contact_is_sibling && !looking && !is_edit_row && is_new_row {
                    // A real friend message landed while nobody was looking — bump the persistent unread counter (contacts-list inner ring + float-to-top; cleared at conversation-open). Written after the loop via the coalescing conv-state writer.
                    conv.unread_count += 1;
                    conv_state_pos = Some(conv_pos);
                }

                // System notification, POST-DECRYPT: real sender display name + message text BY DESIGN — hiding content on the lock screen is the OS's job, and the pre-decrypt RX worker no longer notifies at all (it over-dinged on probes and sibling fleet-sync frames it couldn't tell apart). RUST is the one suppression decision now: `will_ding` (not looking, no live sibling clearer, real friend row) gates the call — the fleet-wide half of the 2026-07-23 design on top of the local `looking` gate. Desktop's notify keeps its own visual gate (no toast while attended) + both dedup on msg_hp.
                if will_ding && is_new_row {
                    // The notification chirp seeds from the RELATIONSHIP DIGEST — the same value the desktop in-app chirp and the contact's colours use — so one sender sounds the same on EVERY device. It seeded from the pinned device key before, which differs per device (each pins its own first-met device) and per platform: "messages from one sender sound different on each device".
                    #[cfg(target_os = "android")]
                    {
                        let chirp_seed =
                            relationship_digest(&from_handle_hash, &our_handle_hash);
                        crate::platform::jni_android::notify_new_message(
                            &msg_hp,
                            &chirp_seed,
                            &sender_name,
                            &msg.content,
                        );
                    }
                    #[cfg(not(target_os = "android"))]
                    crate::platform::desktop_notify::notify_new_message(
                        &msg_hp,
                        &sender_name,
                        &msg.content,
                    );
                }

                // Live fleet propagation: the friend only delivered this to the device in hand — our other devices hear it from us (pushed after the `chains` borrow ends, below).
                sibling_push = Some((contact_idx, msg));

                // Per-contact notification chime: the sender's relationship digest → deterministic modal bell (chirp crate) — the SAME digest that colours their handle and messages, so ears and eyes agree. The handle TEXT never touches the session store by design; the pre-PoW hashes are the canonical identity material. Synthesis (~a second of f64 modal math) + playback run on a detached thread so the receive loop never blocks; desktop-only (Android gets platform notifications).
                // Only ding for a real human message from a friend: a chain-weave probe (hidden ceremony frame) and a sibling/fleet-sync frame (our own devices propagating a conversation) both arrive as ChatMessages, and neither is something a person sent us — so neither should ring. And only when NOT looking at this conversation (`!looking`): watching the message land IS the alert; the chirp is for everyone else's messages (the user ask: "ding when I get a message from anyone and I'm not in a conversation with them"). The old unconditional chirp over-dinged in-conversation.
                #[cfg(not(any(target_os = "redox", target_os = "android")))]
                if !is_chain_probe && will_ding {
                    let digest = relationship_digest(&from_handle_hash, &our_handle_hash);
                    std::thread::spawn(move || {
                        chirp::Chirp::from_hash(digest)
                            .play_blocking()
                            .unwrap_or_else(|e| crate::logf!("CHIME: {}", e));
                    });
                }
                // A real inbound message proves both directions once ACKed, but even the RX half alone can seal if our TX was already ACK-confirmed.
                recv_seal_idx = Some(contact_idx);
            }

            // *** THEN the ACK — attached to the commit snapshot above; the chains writer fires it only after the write lands. If we crash before that, no ACK ever left: the sender resends, we dedup. *** Get recipient pubkey for relay fallback
            let recipient_pubkey = self
                .contacts
                .get(contact_idx)
                .map(|c| *c.public_identity.as_bytes())
                .unwrap_or([0u8; 32]);
            // The re-ACK source is the per-message ack_hash persisted on the stored ChatMessage (see the duplicate handler above + with_ack_hash below), which heals a lost ACK for ANY message — not just the most recent. ACK always rides the relay alongside any direct leg — see the re-ACK site above for the field-observed one-directional case this closes.
            let relay_to = self
                .contacts
                .get(contact_idx)
                .map(|c| c.relay_device_list())
                .unwrap_or_default();
            ack_enqueue = Some((
                chains_commit_snapshot,
                AckRequest {
                    peer_addr: sender_addr,
                    recipient_pubkey,
                    conversation_token,
                    acked_eagle_time: timestamp,
                    plaintext_hash,
                    relay_to,
                },
            ));
            crate::logf!(
                "CHAT: ACK to {} (eagle_time {}, hash {}...) queued behind the durable chains write",
                crate::fp(&from_handle_hash),
                timestamp,
                hex::encode(&plaintext_hash[..8])
            );
        }
        // The tail — everything the arm ran after the `chains` borrow ended or after the loop, direct calls now.
        if let Some((ci, sig, rx_key, ts)) = call_signal_evt {
            self.on_call_signal(ci, sig, rx_key, ts, false, false);
        }
        if let Some((snapshot, req)) = ack_enqueue {
            let dispatch = self.status_checker.as_ref().map(|c| c.ack_dispatch());
            let actions = match dispatch {
                Some(d) => vec![ChainsPostDurable::Ack(d, req)],
                None => Vec::new(),
            };
            self.persist_chains_then(snapshot, actions);
        }
        if let Some(ci) = persist_ci {
            self.persist_messages_async(ci);
        }
        if let Some(pos) = conv_state_pos {
            self.persist_conv_state_async(pos);
        }
        if let Some((idx, m)) = sibling_push {
            self.push_rows_to_siblings(idx, std::slice::from_ref(&m), None);
        }
        if let Some(idx) = recv_seal_idx {
            self.seal_chain_if_ready(idx);
            // ACK-ADVANCE FLUSH, receive-edge twin: a commit can free a window slot (dedup) and always proves the pipeline moves — release any held row now. No-op when nothing is held.
            self.resend_held_messages(idx);
        }
        if let Some(idx) = fork_sibling_reset {
            self.initiate_sibling_chain_reset(idx);
        }
        if let Some(idx) = fork_friend_rekey {
            self.initiate_friend_rekey(idx);
        }
        if need_sync {
            self.update_sync_records();
        }
        if !replays.is_empty() {
            self.chat_replay_queue.extend(replays);
        }
        // BRIDGE host: run the captured `$ ` command and reply over the durable chain. LAST, past every borrow — the command's own bubble + ACK are already committed above, so the operator's row brightens (reached the terminal) before the reply lands.
        #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
        if let Some((ci, cmd)) = bridge_run {
            self.run_bridge_command_chat(ci, &cmd);
        }
    }

    /// Adopt sibling chain_sync blobs the open workers finished — the commit half of the ChainSyncReceived arm. The sender gate re-runs here: a lockout landing while the worker ran must still block the adopt.
    pub(super) fn drain_chain_syncs(&mut self) {
        while let Ok(opened) = self.chain_sync_opened_rx.try_recv() {
            let ChainSyncOpened {
                conversation_token,
                sender_pubkey,
                incoming,
            } = opened;
            if !self
                .contacts
                .iter()
                .any(|c| c.is_sibling && !c.locked_out && c.knows_device(&sender_pubkey.key))
            {
                crate::log(
                    "CHAIN-SYNC: opened frame's sender lost fold trust mid-flight — dropped",
                );
                continue;
            }
            let fid = incoming.friendship_id;
            // Pre-adopt lane positions — the echo kill below stamps only what the ADOPT moved.
            let pre_positions: std::collections::HashMap<[u8; 32], u64> = self
                .friendship_chains
                .iter()
                .find(|(id, _)| *id == fid)
                .map(|(_, c)| c.lane_summary().into_iter().collect())
                .unwrap_or_default();
            // LANE-WISE adopt (docs/lanes.md): a lane merges iff its position is strictly greater — replacing whole-blob newest-wins, whose fork window was two devices clobbering each other's live lanes and pendings. A fresh device takes the whole copy SANITIZED (the sender's minted label, pendings and send tip stripped — adopting those would make this device write on the sender's lane). Echo dies naturally: a sibling merging our pushed union finds no greater positions and stays silent.
            // Arrival at DEBUG: without it a healthy no-op receive and a frame that never arrived are indistinguishable — the exact ambiguity that stalled the 2026-08-21 era-wedge diagnosis (324 pushes logged, zero receive-side lines of any kind).
            crate::logf_at!(
                crate::LogLevel::Debug,
                "CHAIN-SYNC: frame from {} for fid {} (incoming genesis {})",
                crate::fp(&sender_pubkey.key),
                crate::fp(&fid.0),
                incoming.genesis_osc
            );
            let mut incoming = incoming;
            let adopted = match self.friendship_chains.iter_mut().find(|(id, _)| *id == fid) {
                // ERA SUPERSEDE before any lane math: a re-key mints a NEW lane_root, and the lane-wise merge below adopts a root only where one is absent — so a sibling holding the old era would keep dead chains forever, deriving garbage lanes for every new-era label it meets. Two blobs under one friendship with DIFFERENT roots are different eras, and eras replace wholesale: the newer GENESIS wins (era_superseded_by), sanitized like any replicated copy. Losing the race one round just means our next push carries the newer era back.
                Some((_, local)) if local.differs_in_era_from(&incoming) => {
                    if local.era_superseded_by(&incoming) {
                        incoming.sanitize_replicated();
                        *local = incoming;
                        crate::logf!("CHAIN-SYNC: superseded chain era for fid {} — re-keyed root adopted wholesale", crate::fp(&fid.0));
                        true
                    } else {
                        // The refusal is LOAD-BEARING evidence, always loud: two live eras under one friendship (a §4.2 competing-ceremony survivor, or a stale device that out-raced the heal) show up ONLY here — both genesis stamps named so the next log convicts which side holds the elder era.
                        crate::logf!(
                            "CHAIN-SYNC: era REFUSED for fid {} — ours genesis {} vs incoming {} from {} (elder era kept; if this repeats forever, two live eras exist)",
                            crate::fp(&fid.0),
                            local.genesis_osc,
                            incoming.genesis_osc,
                            crate::fp(&sender_pubkey.key)
                        );
                        false
                    }
                }
                Some((_, local)) => local.merge_lanes_from(&incoming),
                None => {
                    incoming.sanitize_replicated();
                    self.friendship_chains.push((fid, incoming));
                    true
                }
            };
            if !adopted {
                continue;
            }
            // WIRE THE CONTACT AT THE CHAINS (per-device lanes): the adopt used to leave contact.friendship_id unset on every non-owner device — the chains sat adopted in RAM while chain_transmit bailed at "no friendship chain" and boot never re-loaded them (the vault load walks contact-referenced fids only). With the id wired and persisted, this device is transmit-capable on its own lane from the first replicated frame.
            if let Some(our_pid) = self
                .session
                .as_ref()
                .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
            {
                let other = self
                    .friendship_chains
                    .iter()
                    .find(|(id, _)| *id == fid)
                    .and_then(|(_, c)| c.participants().iter().find(|p| **p != our_pid).copied());
                if let Some(other) = other {
                    if let Some(c) = self
                        .contacts
                        .iter_mut()
                        .find(|c| !c.is_sibling && c.handle_hash == other)
                    {
                        if c.friendship_id.is_none() {
                            c.friendship_id = Some(fid);
                            crate::logf!("CHAIN-SYNC: wired {} to its replicated chains — this device transmits on its own lane now", crate::fp(&c.handle_proof).as_str());
                            if let Some(storage) = self.storage.as_ref() {
                                let _ = crate::storage::contacts::save_contact(c, storage);
                            }
                        }
                    }
                }
            }
            // ECHO KILL (the chain_pushed_osc field doc promised this): a lane the ADOPT moved is already fleet-known — the ORIGIN pushed it at every sibling itself (relay copies cover the offline ones) — so stamp it pushed at its adopted position. Without the stamp the next replication sweep read "position > pushed" and re-broadcast every adopted lane, one redundant fleet-wide push per adopt. Lanes the adopt did NOT move keep their stamps, so a local advance that raced the adopt still pushes — and the coarse mutated_osc stamp is deliberately NOT recorded here for the same reason (the sweep's empty-changed pass records it safely once nothing per-lane is due).
            let fid_bytes = *fid.as_bytes();
            let post_positions: Vec<([u8; 32], u64)> = self
                .friendship_chains
                .iter()
                .find(|(id, _)| *id == fid)
                .map(|(_, c)| c.lane_summary())
                .unwrap_or_default();
            for (label, pos) in post_positions {
                if pre_positions.get(&label).copied().unwrap_or(0) < pos {
                    let mut key = [0u8; 64];
                    key[..32].copy_from_slice(&fid_bytes);
                    key[32..].copy_from_slice(&label);
                    self.lane_pushed_pos.insert(key, pos);
                }
            }
            // Persist the adopted lanes off-thread (coalesced): a chain-sync adopt is idempotent — a delayed/lost write just re-adopts from the sibling's next push, never a fork, so it is not a commit point.
            self.persist_chains_async(&fid);
            // Wire the contact: a device that never ran this ceremony gains the chain here — flip it sendable (Complete + woven; the owner proved the ratchet end-to-end before the state ever replicated).
            if let Some(ci) = self.contact_idx_for_conversation_token(&conversation_token) {
                let contact = &mut self.contacts[ci];
                let newly_enabled = contact.friendship_id != Some(fid)
                    || contact.clutch_state != crate::types::ClutchState::Complete;
                contact.friendship_id = Some(fid);
                if newly_enabled {
                    contact.clutch_state = crate::types::ClutchState::Complete;
                    contact.chain_woven = true;
                    if let Some(storage) = self.storage.as_ref() {
                        let _ = crate::storage::contacts::save_contact(&self.contacts[ci], storage);
                    }
                    crate::logf!(
                        "CHAIN-SYNC: adopted chain for {} — this device can now transmit directly",
                        crate::fp(&self.contacts[ci].handle_proof)
                    );
                } else {
                    crate::logf!(
                        "CHAIN-SYNC: caught up chain for {} (sibling was ahead)",
                        crate::fp(&self.contacts[ci].handle_proof)
                    );
                }
            } else {
                crate::log("CHAIN-SYNC: adopted chain state for a conversation with no matching contact yet (roster lag) — chain parked under its fid");
            }
            self.scene_dirty = true;
        }
    }

    /// Merge history pages the decrypt workers finished — the commit half of the HistoryPageReceived arm. Runs with full &mut self (no checker borrow pinning it), so the deferral vecs the in-loop arm needed become direct calls. ALL trust/rid gating happens here against CURRENT state: contact indexes, sibling trust, and the in-flight cursor can shift between dispatch and the worker finishing, so nothing decided at dispatch time is trusted for the merge.
    pub(super) fn drain_history_pages(&mut self) {
        while let Ok(opened) = self.hist_opened_rx.try_recv() {
            let HistPageOpened {
                conversation_token,
                request_id,
                sender_pubkey,
                page,
                open_key_fp,
            } = opened;
            let from_sibling = self
                .contacts
                .iter()
                .any(|c| c.is_sibling && !c.locked_out && c.knows_device(&sender_pubkey.key));
            let Some(our_pid) = self
                .session
                .as_ref()
                .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
            else {
                continue;
            };
            // Same sender routing the arm used for the key choice, re-run for the merge target: sibling pages land on the token's contact; friend pages land on the chains' other participant.
            let contact_idx = if from_sibling {
                self.contact_idx_for_conversation_token(&conversation_token)
            } else {
                self.friendship_chains
                    .iter()
                    .find(|(_, c)| c.conversation_token == conversation_token)
                    .and_then(|(_, c)| c.participants().iter().find(|p| **p != our_pid).copied())
                    .and_then(|other| self.contacts.iter().position(|c| c.handle_hash == other))
            };
            let Some(idx) = contact_idx else {
                crate::logf!(
                    "HISTORY: opened page from {} DROPPED — token resolves to no contact",
                    crate::fp(&sender_pubkey.key)
                );
                continue;
            };
            // DECRYPT FAILURE (page: None): the page arrived but the key doesn't open it — a key/era divergence with the sender, not transport. Count it; at the threshold, PARK the walk under the failing key's fingerprint. The park releases itself the moment the conversation's key CHANGES (re-key completed, era adopted) — sweep-side comparison, no timer. Without this, expiry re-requested the same undecryptable 17KB page forever (field 2026-08-21: 161 pages/35min, both fleets, feeding the render storm).
            let Some(page) = page else {
                self.hist_rid_map.remove(&request_id);
                let cid = self.contacts[idx].conversation(&our_pid).id();
                if let Some(rec) = self
                    .conversations
                    .iter_mut()
                    .find(|v| v.id() == cid)
                    .and_then(|v| v.history_recovery.as_mut())
                {
                    rec.in_flight = None;
                    rec.decrypt_fail_streak = rec.decrypt_fail_streak.saturating_add(1);
                    if rec.decrypt_fail_streak >= 4 && rec.parked_key_fp.is_none() {
                        rec.parked_key_fp = Some(open_key_fp);
                        crate::logf!(
                            "HISTORY: walk PARKED for {} — {} consecutive pages undecryptable under key#{}; resumes when the key changes",
                            crate::fp(&sender_pubkey.key),
                            rec.decrypt_fail_streak,
                            hex::encode(open_key_fp)
                        );
                    }
                }
                continue;
            };
            // rid must match a request WE minted — a page we didn't ask for (or asked for long ago) is dropped; merging is idempotent so a raced duplicate that DOES match is harmless. A friend page must match; a sibling page without a matching rid is the live push — merge it, but leave the cursor alone.
            // The rid registry is consulted FIRST and is authoritative: the token-resolved conversation's in_flight alone starved recovery when two contact rows resolved the same peer (the rid lived on the other row's conversation — field, 2026-08-10, every Mary page dropped for days of walk rounds). Consumed on match so the map stays bounded.
            let rid_registered = self.hist_rid_map.remove(&request_id).is_some();
            let rid_matches = rid_registered || {
                let cid = self.contacts[idx].conversation(&our_pid).id();
                self.conversations
                    .iter()
                    .find(|v| v.id() == cid)
                    .and_then(|v| v.history_recovery.as_ref())
                    .and_then(|r| r.in_flight.as_ref())
                    .is_some_and(|(rid, _, _)| *rid == request_id)
            };
            if !(rid_matches || from_sibling) {
                crate::logf!("HISTORY: page from {} DROPPED — rid unmatched and sender is not a fold-trusted sibling", crate::fp(&sender_pubkey.key));
                continue;
            }
            let conv_pos = {
                let derived = self.contacts[idx].conversation(&our_pid);
                match self
                    .conversations
                    .iter()
                    .position(|v| v.id() == derived.id())
                {
                    Some(p) => p,
                    None => {
                        self.conversations.push(derived);
                        self.conversations.len() - 1
                    }
                }
            };
            let mut fresh: Vec<crate::types::ChatMessage> = Vec::new();
            {
                let conv = &mut self.conversations[conv_pos];
                // Merge to OUR perspective: friend pages flip direction (their outgoing = our incoming); sibling pages ride verbatim (same identity, their flags ARE ours). Friend-recovered outgoing is delivered by definition (the friend has it); dedup on (timestamp, content) against what we already hold.
                // Index existing rows ONCE by (timestamp, content-hash) — each row is an O(1) lookup; inserts are deferred so the indices stay valid through the upgrade pass.
                let row_hash = |c: &str| -> u64 {
                    u64::from_le_bytes(
                        blake3::hash(c.as_bytes()).as_bytes()[..8]
                            .try_into()
                            .unwrap(),
                    )
                };
                let mut existing_idx: std::collections::HashMap<(i64, u64), usize> =
                    std::collections::HashMap::with_capacity(conv.messages.len());
                for (i, m) in conv.messages.iter().enumerate() {
                    existing_idx
                        .entry((m.timestamp, row_hash(&m.content)))
                        .or_insert(i);
                }
                let mut to_insert: Vec<crate::types::ChatMessage> = Vec::new();
                let mut tombstoned_in_merge = false;
                for row in &page.rows {
                    if crate::types::is_control_content(&row.content) {
                        continue;
                    }
                    let (is_outgoing, delivered, recovered) = if from_sibling {
                        (row.sender_outgoing, row.delivered, false)
                    } else {
                        (!row.sender_outgoing, !row.sender_outgoing, true)
                    };
                    // O(1) existence check; the exact content compare confirms the hit (guards the astronomically-rare 8-byte-hash + exact-timestamp collision).
                    let hit = existing_idx
                        .get(&(row.timestamp, row_hash(&row.content)))
                        .copied()
                        .filter(|&i| conv.messages[i].content == row.content);
                    if let Some(i) = hit {
                        // Delivered AND deleted are monotonic (true wins): a copy that saw the ACK — or the tombstone — upgrades ours. Upgraded rows ride `fresh` (persist + gossip) but are NOT re-inserted.
                        let existing = &mut conv.messages[i];
                        let mut upgraded = false;
                        if delivered && !existing.delivered && existing.is_outgoing == is_outgoing {
                            existing.delivered = true;
                            upgraded = true;
                        }
                        if row.deleted && !existing.deleted {
                            existing.deleted = true;
                            tombstoned_in_merge = true; // drops a row from the syncable set (inserts self-invalidate; this upgrade path doesn't)
                            if let Some((hash, _, _)) =
                                crate::types::parse_attachment_content(&existing.content)
                            {
                                crate::storage::blob_delete(&hash);
                            }
                            upgraded = true;
                        }
                        // Reference is origin-written row identity — a copy that arrived thru a pre-feature route regains it here (monotonic, never un-set).
                        if existing.reference.is_none() {
                            if let Some(r) = row.reference.and_then(|(k, t)| {
                                crate::types::RefKind::from_wire(k).map(|k| (k, t))
                            }) {
                                existing.reference = Some(r);
                                upgraded = true;
                            }
                        }
                        if upgraded {
                            fresh.push(existing.clone());
                        }
                        continue;
                    }
                    to_insert.push(crate::types::ChatMessage {
                        content: row.content.clone(),
                        timestamp: row.timestamp,
                        is_outgoing,
                        delivered,
                        ack_hash: None,
                        recovered,
                        // Sibling pages carry OUR fleet's discharged-alert flag; a FRIEND page's flag is THEIR fleet's state — recovered history is always silent here (the catch-up summary in the sibling drain is the one place an unnotified batch may ding once).
                        notified: if from_sibling { row.notified } else { true },
                        deleted: row.deleted,
                        reference: row
                            .reference
                            .and_then(|(k, t)| crate::types::RefKind::from_wire(k).map(|k| (k, t))),
                    });
                }
                // Deferred inserts (they'd shift indices the map holds); insert_message_sorted dedups again defensively, so a page carrying two identical rows still lands one.
                for msg in to_insert {
                    conv.insert_message_sorted(msg.clone());
                    fresh.push(msg);
                }
                // A tombstone flip on an existing row doesn't pass through insert_message_sorted, so invalidate the digest for that case (inserts already self-invalidated).
                if tombstoned_in_merge {
                    conv.invalidate_digest();
                }

                // Cursor + completion — only for a page we ASKED for; a live push must not fast-forward a walk that never ran. Early-stop: if history was already complete before this (re-)kickoff and the page brought nothing new, we're still complete — a routine re-key on an intact pair stops after one page instead of re-walking years.
                if rid_matches {
                    if let Some(rec) = conv.history_recovery.as_mut() {
                        rec.in_flight = None;
                        // A page that OPENS clears the divergence evidence — the failing key era is behind us.
                        rec.decrypt_fail_streak = 0;
                        rec.parked_key_fp = None;
                        if page.oldest_osc < rec.oldest_recovered_osc {
                            rec.oldest_recovered_osc = page.oldest_osc;
                        }
                        if !page.more || (rec.was_complete_before && fresh.is_empty()) {
                            rec.complete = true;
                        }
                    }
                }
                crate::logf!(
                    "HISTORY: merged page ({} new of {} rows, more={}, complete={})",
                    fresh.len(),
                    page.rows.len(),
                    page.more,
                    conv.history_recovery.as_ref().is_some_and(|r| r.complete)
                );
            }
            // CATCH-UP SUMMARY (2026-07-23 design): a sibling batch carrying rows NOBODY flagged means the whole fleet was offline when they landed — alert duty was never discharged. ONE summary chirp for the batch (never a per-row storm — this is exactly the wake-from-doze ding-storm killer), then flag every one. Rows arriving pre-flagged (the origin dinged or its clearer watched) stay silent, which is the steady-state forward path. Restricted to the FRESH batch: older stored-but-unflagged rows may be legitimately suppressed by a LIVE claim right now — those belong to the drop-sweep, which fires only when the claim actually drops.
            if from_sibling && !self.contacts[idx].is_sibling {
                let undischarged: Vec<i64> = fresh
                    .iter()
                    .filter(|m| {
                        !m.notified && !m.is_outgoing && !crate::types::is_control_content(&m.content)
                    })
                    .map(|m| m.timestamp)
                    .collect();
                self.summary_chirp_and_flag(idx, conv_pos, &undischarged);
            }
            // CALL signals via sibling merge are STOP edges ONLY (docs/calls.md): our sibling's answer/decline row stops this device's ring; replayed catch-up signals correctly ring nothing (a ring requires the DIRECT offer decrypt — which is also what kills the stale-offer-rings-days-later class).
            if from_sibling {
                let sigs: Vec<(crate::call::signal::CallSignal, i64, bool)> = fresh
                    .iter()
                    .filter_map(|m| {
                        crate::call::signal::CallSignal::parse(&m.content)
                            .map(|s| (s, m.timestamp, m.is_outgoing))
                    })
                    .collect();
                for (sig, ts, out) in sigs {
                    self.on_call_signal(idx, sig, None, ts, true, out);
                }
            }
            // Persist the cursor off-thread (coalesced 13-byte record; the rows ride the coalescing message writer below).
            self.persist_conv_state_async(conv_pos);
            if !fresh.is_empty() {
                self.persist_messages_async(idx);
                // Gossip hop: anything genuinely fresh re-pushes to the OTHER online siblings (never back at the sender), so a message crosses the whole fleet even when only one device can reach its origin. Zero-fresh pages stop the echo.
                self.push_rows_to_siblings(idx, &fresh, Some(sender_pubkey.key));
            }
            // FLEET-FORWARD DRAIN (compose anywhere): outgoing UNDELIVERED rows arriving from a SIBLING are messages another device composed and forwarded — if THIS device can write the braid (locally woven, or lane-capable on replicated chains: per-device lanes), transmit them with their ORIGINAL timestamps (one row identity fleet-wide, so the friend's dedup + the delivered upgrade all cohere; a retransmit after a crash is re-ACKed harmlessly from the friend's stored ack_hash). Only FRESH rows drain — known rows were transmitted before or sit in the retransmit machinery. The origin usually transmits itself now; this drain is the DEAD-ORIGIN backstop (battery died between compose and delivery — the rows replicated, the origin's lane went silent, and this device re-serves them on ITS lane; the friend dedups the brief both-alive overlap).
            if from_sibling && (self.contacts[idx].chain_woven || self.lane_transmit_capable(idx)) {
                let fwd: Vec<(String, i64, Option<(crate::types::RefKind, i64)>)> = fresh
                    .iter()
                    .filter(|m| m.is_outgoing && !m.delivered)
                    .map(|m| (m.content.clone(), m.timestamp, m.reference))
                    .collect();
                for (text, ts, re_ref) in fwd {
                    if self.chain_transmit(idx, &text, ts, re_ref) {
                        crate::log("CHAT: fleet-forwarded row transmitted on the local chain");
                    }
                }
            }
            self.scene_dirty = true;
        }
    }

    /// ONE summary chirp for a batch of undischarged rows, then flag them notified. The shared mechanics behind the sibling-merge catch-up and the claim drop-sweep — never a per-row storm. No-ops on an empty batch. (The digest excludes flags, so flagging live rows needs no invalidation.)
    pub(super) fn summary_chirp_and_flag(
        &mut self,
        idx: usize,
        conv_pos: usize,
        undischarged: &[i64],
    ) {
        if undischarged.is_empty() {
            return;
        }
        let sender_name = self.contacts[idx].display_name();
        crate::logf!(
            "NOTIFY: batch of {} undischarged row(s) from {} — one summary alert",
            undischarged.len(),
            sender_name
        );
        let summary = format!("{} new message(s)", undischarged.len());
        let batch_hp = *blake3::hash(&undischarged[0].to_le_bytes()).as_bytes();
        #[cfg(target_os = "android")]
        {
            let chirp_seed = relationship_digest(
                &self.contacts[idx].handle_hash,
                &self
                    .session
                    .as_ref()
                    .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
                    .unwrap_or([0u8; 32]),
            );
            crate::platform::jni_android::notify_new_message(
                &batch_hp,
                &chirp_seed,
                &sender_name,
                &summary,
            );
        }
        #[cfg(not(target_os = "android"))]
        crate::platform::desktop_notify::notify_new_message(&batch_hp, &sender_name, &summary);
        if let Some(conv) = self.conversations.get_mut(conv_pos) {
            for m in conv.messages.iter_mut() {
                if undischarged.contains(&m.timestamp) {
                    m.notified = true;
                }
            }
        }
    }

    /// DROP-SWEEP: when a focus claim drops (retraction received, holder's presence void, or the ball stolen from the claim holder), any rows THIS device suppressed while honoring that claim are still undischarged — chirp them ONCE and flag. The scan is bounded below by the claim's own osc (suppression-by-claim can only have happened while the claim stood; an unbounded scan would summary-ding ancient anomalies). No-ops when nothing was suppressed — the steady state.
    pub(super) fn sweep_undischarged(&mut self, conversation_token: [u8; 32], min_osc: i64) {
        let Some((idx, conv_id)) = self.contacts.iter().enumerate().find_map(|(i, c)| {
            if c.is_sibling {
                return None;
            }
            let us = self.our_party_id(c)?;
            (crate::crypto::clutch::derive_conversation_token(&[us, c.handle_hash])
                == conversation_token)
                .then(|| (i, c.conversation(&us).id()))
        }) else {
            return;
        };
        let Some(conv_pos) = self.conversations.iter().position(|c| c.id() == conv_id) else {
            return;
        };
        let undischarged: Vec<i64> = self.conversations[conv_pos]
            .messages
            .iter()
            .filter(|m| {
                !m.notified
                    && !m.is_outgoing
                    && m.timestamp >= min_osc
                    && !crate::types::is_control_content(&m.content)
            })
            .map(|m| m.timestamp)
            .collect();
        if undischarged.is_empty() {
            return;
        }
        crate::logf!(
            "NOTIFY: claim dropped with {} suppressed row(s) in the crossing window — sweeping",
            undischarged.len()
        );
        self.summary_chirp_and_flag(idx, conv_pos, &undischarged);
        self.persist_messages_async(idx);
    }

    pub(super) fn drain_avatar_downloads(&mut self) {
        while let Ok(result) = self.avatar_dl_rx.try_recv() {
            let Some(vsf_rgb) = result.pixels else {
                // Own-avatar decode failed (poisoned vault bytes) — arm the FGTW recovery the old synchronous load fired inline. Only the boot decode worker sends owner-None with no pixels (the recover worker sends success only), so this can't loop.
                if result.owner.is_none() {
                    if let Some(seed) = self.session.as_ref().map(|s| s.identity_seed) {
                        self.self_avatar_recover_pending = Some(seed);
                    }
                }
                continue;
            };
            let display = crate::ui::colour_convert::vsf_rgb_to_bt2020(&vsf_rgb);
            // `owner: None` = our OWN avatar (boot vault decode, or recovered from FGTW after a local clear). Install it as the device avatar and invalidate the scaled cache so the Ready screen repaints it.
            let Some(owner_hp) = result.owner else {
                self.device_avatar_pixels = Some(display);
                self.device_avatar_scaled = None;
                self.device_avatar_scaled_diameter = 0;
                crate::log("Avatar: own avatar installed (async vault load / recovery)");
                self.refresh_self_row_avatar();
                continue;
            };
            if let Some(contact) = self
                .contacts
                .iter_mut()
                .find(|c| !c.is_sibling && c.handle_proof == owner_hp)
            {
                contact.avatar_pixels = Some(display);
                contact.avatar_scaled = None; // force rebuild at the current diameter on next render
                contact.avatar_scaled_diameter = 0;
                // An install changes what the vault holds — drop the sweep's remembered probes (one re-probe each, rare edge).
                self.avatar_probe_cache.clear();
                crate::logf!(
                    "Avatar: installed peer avatar for {}",
                    crate::fp(&contact.handle_proof)
                );
            }
        }
    }

    /// Drain the nunc-time clock verdict. A consensus offset beyond ±`CLOCK_OFF_THRESHOLD_SECS` raises the amber "clock off" banner (`clock_off`); within threshold clears it. An `Unavailable` result (we couldn't reach consensus) is NOT an anomaly — we leave the banner as-is rather than claiming the clock is fine. This is warn-only: the system clock is never corrected.
    pub(super) fn drain_clock_check(&mut self) {
        /// How far off (seconds) the system clock must be before we warn. 30s — well past ordinary NTP jitter and nunc's own confidence half-width, so the banner means a real problem.
        const CLOCK_OFF_THRESHOLD_SECS: i64 = 30;

        while let Ok(result) = self.clock_check_rx.try_recv() {
            match result {
                crate::network::ClockCheckResult::Ok {
                    offset_secs,
                    confidence_secs,
                    sources_used,
                    sources_queried,
                } => {
                    crate::logf!(
                        "Clock: nunc consensus offset = {}s (±{}s, {}/{} sources)",
                        offset_secs,
                        confidence_secs,
                        sources_used,
                        sources_queried
                    );
                    // Kept regardless of the banner threshold — the update stamp window's forward-fail tiebreak reads the raw verdict.
                    self.clock_consensus = Some((offset_secs, confidence_secs as i64));
                    self.clock_off = if offset_secs.abs() > CLOCK_OFF_THRESHOLD_SECS {
                        crate::logf!("Clock: system clock off by {}s — raising banner (warn only, not corrected)", offset_secs);
                        Some(offset_secs)
                    } else {
                        None
                    };
                }
                crate::network::ClockCheckResult::Unavailable(why) => {
                    crate::logf!("Clock: consensus unavailable ({}) — banner unchanged", why);
                }
            }
        }
    }

    /// Kick a one-shot fleet-inbox drain off-thread (blocking HTTPS). Pulls this identity's pending worker-observed events (bind-attempt alerts) and posts them over `inbox_check_tx`; `drain_fleet_inbox` surfaces them on a later tick. No-op without a handle_proof + device key (not yet attested).
    pub(super) fn spawn_inbox_drain(&self) {
        if let (Some(hp), Some(kp), tx) = (
            self.our_handle_proof(),
            self.device_keypair.clone(),
            self.inbox_check_tx.clone(),
        ) {
            std::thread::spawn(move || {
                match crate::network::fgtw::inbox_drain_blocking(&kp, &hp) {
                    Ok(events) if !events.is_empty() => {
                        let _ = tx.send(events);
                    }
                    Ok(_) => {}
                    Err(e) => crate::logf!("INBOX: drain failed: {}", e),
                }
            });
        }
    }

    /// Drain any pulled fleet-inbox events and surface them as an event-shown notice (interaction-cleared, never timed). A `bind_attempt` renders "someone tried to enrol one of your devices"; if the attempted-into handle_proof matches a known contact, name it — that's the case that distinguishes an insider or your own fumble from an anonymous thief (docs/fleet-inbox.md).
    pub(super) fn drain_fleet_inbox(&mut self) {
        // Collect first so the rx borrow is released before we touch self.contacts / self.ready_toast.
        let batches: Vec<Vec<crate::network::fgtw::FleetInboxEvent>> =
            self.inbox_check_rx.try_iter().collect();
        for events in batches {
            let mut bind_attempts = 0usize;
            let mut named: Option<String> = None;
            for ev in &events {
                crate::logf!(
                    "INBOX: {} — device {} attempted-by {}",
                    ev.kind,
                    crate::fp(&ev.device),
                    crate::fp(&ev.attempted_by)
                );
                if ev.kind == "bind_attempt" {
                    bind_attempts += 1;
                    if named.is_none() {
                        named = self
                            .contacts
                            .iter()
                            .find(|c| c.handle_proof == ev.attempted_by)
                            .map(|c| c.display_name());
                    }
                }
            }
            if bind_attempts > 0 {
                let who = match &named {
                    Some(name) => format!(" into {name}'s fleet"),
                    None => String::new(),
                };
                let plural = if bind_attempts == 1 { "" } else { "s" };
                self.ready_toast = Some(format!(
                    "\u{26a0} {bind_attempts} attempt{plural} to enrol your device{who}"
                ));
            }
        }
    }

    /// Recover the device's OWN avatar from the wall after a local clear (the vault load returned nothing). Needs the PIN — that is what addresses and decrypts the published copy — so it no-ops until settings carry one, and the tick retries. Off-thread (blocking FGTW round-trip); the result comes back over avatar_dl_tx with an EMPTY handle, which drain_avatar_downloads routes into device_avatar_pixels.
    pub(super) fn spawn_self_avatar_recover(&self, identity_seed: [u8; 32]) -> bool {
        let (Some(storage), Some(pin)) = (
            self.storage.as_ref().map(Arc::clone),
            self.ensure_avatar_pin_readonly(),
        ) else {
            return false;
        };
        let tx = self.avatar_dl_tx.clone();
        #[cfg(not(target_os = "android"))]
        let proxy = self.event_proxy.clone();
        std::thread::spawn(move || {
            let pixels =
                crate::ui::avatar::recover_own_avatar_from_wall(&identity_seed, &pin, &storage);
            if pixels.is_some() {
                let _ = tx.send(crate::ui::avatar::AvatarDownloadResult {
                    owner: None, // self
                    pixels,
                });
                #[cfg(not(target_os = "android"))]
                if let Some(p) = proxy.as_ref() {
                    let _ = p.send(crate::ui::PhotonEvent::NetworkUpdate);
                }
            }
        });
        true
    }
}
