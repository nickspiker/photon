//! Settings + profile — the update channel, avatar pin, profile publish, You-page fields, reaction recency, and settings persistence/push.

use super::*;

impl PhotonApp {
    /// Kick an off-thread update check against `channel`'s manifest; `apply` = install on any version DIFFERENCE (the Get-latest buttons — explicit channel hop, downgrade allowed by user intent), else report-only (the Check button). One op at a time.
    /// Ensure the shared update channel exists; return a fresh Sender clone for a worker thread.
    pub(super) fn update_sender(&mut self) -> std::sync::mpsc::Sender<UpdateEvent> {
        if self.update_tx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            self.update_tx = Some(tx);
            self.update_rx = Some(rx);
        }
        self.update_tx.as_ref().unwrap().clone()
    }

    /// On Updates-page open: fetch BOTH channel manifests (report-only) so each button can show its target version + colour. Idempotent per visit via `update_checked`.
    /// Effective `updates.auto` (fleet-synced, born linked): absent = ON — the compiled default per docs/updates.md.
    pub(super) fn auto_updates_enabled(&self) -> bool {
        self.fleet_settings
            .as_ref()
            .and_then(|fs| fs.effective("updates.auto"))
            .and_then(crate::storage::fleet_settings::as_bool)
            .unwrap_or(true)
    }

    /// The AUTOMATIC update path (docs/updates.md): a jittered ~6–8h RELEASE-channel poll, gated by the `updates.auto` fleet setting (default ON). What "apply" means is per-platform (see `on_auto_update_check`): a desktop release build self-applies thru the stamp window and re-execs; dev builds (manual by mandate) and Android (the OS owns package installs) surface a once-per-version toast. The DEV channel is never polled automatically.
    pub(super) fn drive_auto_update(&mut self) {
        if !self.online || self.update_busy || !self.auto_updates_enabled() {
            return;
        }
        // Android: the FCM `updates` topic notice sets a one-shot flag from the messaging service; drain it only past the act-ability gates above so an offline/busy moment doesn't swallow it.
        #[cfg(target_os = "android")]
        if crate::platform::jni_android::check_fcm_update_notice() {
            crate::log("UPDATE: FCM release notice — manifest poll due now");
            self.next_update_check_osc = 1;
        }
        let now = vsf::eagle_time_oscillations();
        if self.next_update_check_osc == 0 {
            // First check a jittered minute-or-two after launch — let attest + the network settle first.
            self.next_update_check_osc =
                now + 60 * crate::OSC_PER_SEC + crate::jitter(60 * crate::OSC_PER_SEC);
            return;
        }
        if now < self.next_update_check_osc {
            return;
        }
        // 4h + jitter(4h) → a 6–8h cadence, de-synchronized across the fleet.
        self.next_update_check_osc =
            now + (4 * 3600 + crate::jitter(4 * 3600)) * crate::OSC_PER_SEC;
        let tx = self.update_sender();
        std::thread::spawn(move || {
            use crate::network::updates::{fetch_manifest_stamped_blocking, our_row, Channel};
            match fetch_manifest_stamped_blocking(Channel::Release) {
                Ok((stamp, rows)) => {
                    let _ = tx.send(UpdateEvent::AutoChecked(stamp, our_row(&rows)));
                }
                Err(e) => crate::logf!("UPDATE: auto check failed ({}) — next cadence retries", e),
            }
        });
    }

    /// Policy for a finished automatic check: tuple-forward only (a downgrade is never automatic), then the stamp window `floor < t ≤ now` with the STAGED clock — system eagle time on the happy path, the nunc consensus verdict (conservative edge) consulted exactly when the check fails forward, a fresh consensus requested when none is in hand. Clearing all gates: desktop release builds self-apply + re-exec; dev builds and Android get a once-per-version toast pointing at Settings → Updates.
    pub(super) fn on_auto_update_check(
        &mut self,
        stamp_osc: i64,
        row: Option<crate::network::updates::ManifestRow>,
    ) {
        use crate::network::updates::{our_version, stamp_window, StampVerdict};
        let Some(row) = row else {
            return; // no artefact for this platform — nothing to move to
        };
        let ours = our_version();
        if row.version <= ours {
            return; // current or ahead — automatic never moves backward
        }
        let now_sys = vsf::eagle_time_oscillations();
        let verdict = match stamp_window(stamp_osc, now_sys) {
            StampVerdict::ForwardDated => match self.clock_consensus {
                // Honest-clock tiebreak: the LOWEST plausible now (offset minus the confidence half-width), so a lagging system clock delays an update rather than rejecting it, and a forward-dated stamp still can't slip in.
                Some((offset, confidence)) => stamp_window(
                    stamp_osc,
                    now_sys + (offset - confidence) * crate::OSC_PER_SEC,
                ),
                None => {
                    crate::log("UPDATE: manifest ahead of the system clock and no consensus verdict in hand — requesting one, deferring");
                    #[cfg(not(target_os = "android"))]
                    if let Some(proxy) = self.event_proxy.clone() {
                        crate::network::spawn_clock_check(self.clock_check_tx.clone(), Some(proxy));
                    }
                    #[cfg(target_os = "android")]
                    crate::network::spawn_clock_check(self.clock_check_tx.clone(), None);
                    StampVerdict::ForwardDated
                }
            },
            v => v,
        };
        match verdict {
            StampVerdict::Stale => {
                crate::log("UPDATE: manifest stamp at/below this build's floor — replay or stale edge, ignored");
                return;
            }
            StampVerdict::ForwardDated => {
                crate::log(
                    "UPDATE: manifest is forward-dated — not yet (re-evaluated next cadence)",
                );
                return;
            }
            StampVerdict::Accept => {}
        }
        // A dev build never hops channels on its own, and Android can't self-install — both announce instead. patch == 0 IS the release-build predicate: the version scheme guarantees a dev build never wears .0 (deploy opens the dev line at .1; dev publishes are publish-current-then-bump).
        let desktop_release = cfg!(not(target_os = "android")) && ours.2 == 0;
        if desktop_release {
            crate::logf!(
                "UPDATE: auto-applying release {} (stamp window clear)",
                row.version_string()
            );
            self.update_release = ChannelCheck::Ready(Some(row));
            self.spawn_update_apply(crate::network::updates::Channel::Release);
        } else if self.update_toasted != Some(row.version) {
            self.update_toasted = Some(row.version);
            self.ready_toast = Some(format!(
                "Photon {} available \u{2014} Settings \u{2192} Updates",
                dozenal_version_tuple(row.version)
            ));
        }
    }

    pub(super) fn check_update_channels(&mut self) {
        use crate::network::updates::Channel;
        self.update_checked = true;
        for channel in [Channel::Release, Channel::Dev] {
            let slot = match channel {
                Channel::Release => &mut self.update_release,
                Channel::Dev => &mut self.update_dev,
            };
            *slot = ChannelCheck::Checking;
            let tx = self.update_sender();
            std::thread::spawn(move || {
                use crate::network::updates::{fetch_manifest_blocking, our_row};
                let result = fetch_manifest_blocking(channel).map(|rows| our_row(&rows));
                let _ = tx.send(UpdateEvent::Checked(channel, result));
            });
        }
    }

    /// Button click: install the KNOWN row for `channel` (download → verify → swap/stage). No-op if that channel isn't Ready with an installable row, or an apply is already in flight.
    pub(super) fn spawn_update_apply(&mut self, channel: crate::network::updates::Channel) {
        if self.update_busy {
            return;
        }
        let row = match channel {
            crate::network::updates::Channel::Release => &self.update_release,
            crate::network::updates::Channel::Dev => &self.update_dev,
        };
        let ChannelCheck::Ready(Some(row)) = row else {
            return;
        };
        if row.version == crate::network::updates::our_version() {
            return; // already on it — the button is inert ("Already on …")
        }
        let row = row.clone();
        self.update_busy = true;
        self.update_status = Some(format!(
            "Installing {} {}\u{2026}",
            channel.label(),
            dozenal_version_tuple(row.version)
        ));
        crate::logf!(
            "UPDATE: applying {} {}",
            channel.label(),
            row.version_string()
        );
        let tx = self.update_sender();
        let wake = self.event_proxy.clone();
        std::thread::spawn(move || {
            // Progress rides the same channel, throttled to whole-percent changes (~100 events for a 40 MiB binary) so the wake path isn't an event storm. The tick drain renders it as the download bar.
            let last_pct = std::sync::atomic::AtomicU64::new(u64::MAX);
            let txp = tx.clone();
            let progress = move |done: u64, total: u64| {
                let pct = if total > 0 {
                    done * 100 / total
                } else {
                    done >> 20
                };
                if last_pct.swap(pct, std::sync::atomic::Ordering::Relaxed) != pct {
                    let _ = txp.send(UpdateEvent::Progress(done, total));
                    #[cfg(not(target_os = "android"))]
                    if let Some(w) = wake.as_ref() {
                        let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
                    }
                }
            };
            #[cfg(target_os = "android")]
            let _ = &wake; // Android redraws via the Choreographer; the drain runs every tick regardless.
            #[cfg(not(target_os = "android"))]
            {
                match crate::network::updates::apply_desktop_blocking(&row, &progress) {
                    Ok(exe) => {
                        let _ = tx.send(UpdateEvent::Applied(exe));
                    }
                    Err(e) => {
                        let _ = tx.send(UpdateEvent::ApplyFailed(e));
                    }
                }
            }
            #[cfg(target_os = "android")]
            {
                match crate::network::updates::download_apk_blocking(&row, &progress) {
                    Ok(path) => {
                        let _ = tx.send(UpdateEvent::ApkReady(path.to_string_lossy().into_owned()));
                    }
                    Err(e) => {
                        let _ = tx.send(UpdateEvent::ApplyFailed(e));
                    }
                }
            }
        });
    }

    /// Drain the update channel (called from tick): per-channel version state, apply status, staged-APK hand-off, re-exec flag.
    pub(super) fn drain_update_events(&mut self) -> bool {
        let Some(rx) = self.update_rx.as_ref() else {
            return false;
        };
        let mut changed = false;
        // Auto-check verdicts defer past the loop — the policy handler needs &mut self while `rx` borrows it.
        let mut auto_checked: Option<(i64, Option<crate::network::updates::ManifestRow>)> = None;
        while let Ok(ev) = rx.try_recv() {
            changed = true;
            match ev {
                UpdateEvent::Checked(channel, result) => {
                    let state = match result {
                        Ok(row) => ChannelCheck::Ready(row),
                        Err(e) => {
                            // Read (log) the reason — otherwise the field is dead and the failure is invisible.
                            crate::logf!("UPDATE: {} check failed: {}", channel.label(), e);
                            let _ = e;
                            ChannelCheck::Failed
                        }
                    };
                    // Log WHAT settled, not just that it did. "check settled" alone made the field case undiagnosable: a device sat three releases behind while reporting up-to-date, and nothing recorded whether the manifest it fetched was stale, the row was missing for the platform, or the compare itself was wrong.
                    if let ChannelCheck::Ready(row_opt) = &state {
                        let ours = crate::network::updates::our_version();
                        match row_opt {
                            Some(row) => {
                                let manifest_v = format!(
                                    "{}.{}.{}",
                                    row.version.0, row.version.1, row.version.2
                                );
                                let ours_v = format!("{}.{}.{}", ours.0, ours.1, ours.2);
                                let verdict = if row.version > ours {
                                    " → UPDATE AVAILABLE"
                                } else {
                                    ""
                                };
                                crate::logf!(
                                    "UPDATE: {} check settled — manifest has {}, running {}{}",
                                    channel.label(),
                                    manifest_v,
                                    ours_v,
                                    verdict
                                );
                            }
                            None => crate::logf!(
                                "UPDATE: {} check settled — no artefact row for this platform",
                                channel.label()
                            ),
                        }
                    } else {
                        crate::logf!("UPDATE: {} check settled (failed)", channel.label());
                    }
                    match channel {
                        crate::network::updates::Channel::Release => self.update_release = state,
                        crate::network::updates::Channel::Dev => self.update_dev = state,
                    }
                }
                UpdateEvent::AutoChecked(stamp, row) => {
                    auto_checked = Some((stamp, row));
                }
                UpdateEvent::Progress(done, total) => {
                    // done == total (or the stream just ended on an unknown length) flips the label to the verify/swap phase.
                    self.update_progress = Some((done, total));
                    self.update_status = None; // the bar IS the status while a download runs
                }
                UpdateEvent::Applied(exe) => {
                    self.update_busy = false;
                    self.update_progress = None;
                    self.update_status = Some("Updated \u{221a} restarting\u{2026}".to_string());
                    self.update_reexec = Some(exe);
                }
                #[cfg(target_os = "android")]
                UpdateEvent::ApkReady(path) => {
                    self.update_busy = false;
                    self.update_progress = None;
                    self.update_status =
                        Some("Downloaded \u{221a} confirm the install prompt".to_string());
                    self.pending_apk_install = Some(path);
                }
                UpdateEvent::ApplyFailed(e) => {
                    self.update_busy = false;
                    self.update_progress = None;
                    self.update_status = Some(format!("Update failed (nothing changed): {e}"));
                    crate::logf!("UPDATE: apply failed: {}", e);
                }
            }
        }
        if let Some((stamp, row)) = auto_checked {
            self.on_auto_update_check(stamp, row);
        }
        changed
    }

    // (The MINTING ensure_avatar_pin is deliberately gone. It existed to guarantee a pin at publish time, and its authoritative-pull gate was still one drain-ordering hazard wide: inside ensure_fleet_settings' lazy load it minted 4ms before the merge delivered the fleet's real pin, which then lost LWW to the mint — every wiped boot, forever. A pin is born in exactly one place now: the avatar-set rotation, where an image actually stands behind it.)

    /// The avatar pin as it stands, WITHOUT minting one — the serve path runs inside the drain's immutable borrows, and a peer's request is never a reason to create a pin (no pin means no avatar to serve anyway).
    pub(super) fn ensure_avatar_pin_readonly(&self) -> Option<[u8; 64]> {
        let v = self
            .fleet_settings
            .as_ref()
            .and_then(|fs| fs.effective("profile.avatar_pin"))
            .and_then(crate::storage::fleet_settings::as_bytes)?;
        (v.len() == 64).then(|| {
            let mut p = [0u8; 64];
            p.copy_from_slice(&v);
            p
        })
    }

    /// Push our avatar pin into the status thread's pong slot, so friends receive the friend-gated avatar capability on their next ping cycle. Called on avatar set, on settings load, and when a sibling's merged edit lands.
    pub(super) fn publish_avatar_pin(&mut self) {
        // READONLY, deliberately: publishing must never mint. The minting variant ran inside ensure_fleet_settings' lazy load — 4ms BEFORE the fstate merge delivered the slot's real pin, which then lost LWW to the mint it had just enabled. Every wiped boot re-minted, the wall copy stayed one pin behind, and the avatar never recovered (caught by the FSTATE fingerprints, 2026-08-02). A pin now exists ONLY when an avatar set rotates one in — a pin with no image behind it was never worth announcing.
        if let Some(pin) = self.ensure_avatar_pin_readonly() {
            crate::network::status::set_avatar_pin(&pin);
        }
    }

    /// Rotate the avatar bearer pin WITHOUT a new image — the removal-heal follow-up (braid.md §14.2). The pin is a bearer credential (AES key ‖ wall lookup) held at rest by friends and the whole fleet, and it otherwise rotates only on an avatar CHANGE — so a departed device would keep fetching + decrypting the avatar forever. Mirrors `set_avatar_from_file`'s rotate-on-set half: mint + set + publish + stamp-bump on the UI thread (the stamp makes siblings refetch, since the old wall slot is about to die), then re-upload the vault-cached avatar under the new pin and delete the old slot off-thread. Skips — pin left standing — when there's no pin at rest (nothing to revoke) or no vault copy to re-upload (deleting the wall blob without a replacement would blank the avatar fleet-wide; the next avatar set/sync closes it). Same accepted ordering as set: the new pin is announced before the upload lands, so an upload failure leaves a dangling pin that heals on the next set.
    pub(super) fn rotate_avatar_pin(&mut self) {
        let Some(identity_seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            return;
        };
        let (Some(storage), Some(kp), Some(hp)) = (
            self.storage.clone(),
            self.device_keypair.clone(),
            self.our_handle_proof(),
        ) else {
            return;
        };
        let Some(old_pin) = self
            .fleet_settings
            .as_ref()
            .and_then(|fs| fs.effective("profile.avatar_pin"))
            .and_then(crate::storage::fleet_settings::as_bytes)
            .filter(|v| v.len() == 64)
            .map(|v| {
                let mut p = [0u8; 64];
                p.copy_from_slice(&v);
                p
            })
        else {
            return; // no pin at rest = no bearer credential to revoke
        };
        let have_local = matches!(
            storage.read_addr(&crate::storage::vault_key("avatar", &identity_seed)),
            Ok(Some(_))
        );
        if !have_local {
            crate::log("AVATAR: pin rotate skipped — no vault copy to re-upload yet (next avatar set/sync closes it)");
            return;
        }
        let mut new_pin = [0u8; 64];
        {
            use rand::RngCore;
            rand::thread_rng().fill_bytes(&mut new_pin);
        }
        self.settings_set("profile.avatar_pin", vsf::VsfType::hR(new_pin.to_vec()));
        self.publish_avatar_pin();
        self.settings_set(
            "profile.avatar_ts",
            vsf::VsfType::e(vsf::types::EtType::e6(vsf::eagle_time_oscillations())),
        );
        std::thread::spawn(move || {
            #[cfg(not(target_os = "redox"))]
            let _ =
                thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Min);
            match crate::ui::avatar::upload_avatar_from_seed(&kp.secret, &identity_seed, &new_pin, &hp, &storage) {
                Ok(_) => {
                    crate::log("AVATAR: re-uploaded under the rotated pin (removal heal)");
                    let sk = ed25519_dalek::SigningKey::from_bytes(kp.secret.as_bytes());
                    match crate::ui::avatar::delete_avatar_blocking(&sk, &identity_seed, &old_pin) {
                        Ok(()) => crate::log("AVATAR: old wall slot deleted — departed device's pin is dead"),
                        Err(e) => crate::logf!("AVATAR: old slot delete failed (orphan blob remains): {}", e),
                    }
                }
                Err(e) => crate::logf!("AVATAR: pin-rotate upload failed (old slot left serving, pin dangling until the next set): {}", e),
            }
        });
    }

    /// Push our `profile.name` into the status thread's pong slot, so every friend's next ping cycle carries the current name (the always-granted slot). Called on settings load, on Update, and when a sibling's merged edit lands.
    pub(super) fn publish_profile_name(&self) {
        let name = self
            .fleet_settings
            .as_ref()
            .and_then(|fs| fs.effective("profile.name"))
            .and_then(crate::storage::fleet_settings::as_text)
            .unwrap_or_default();
        crate::network::status::set_profile_name(&name);
    }

    /// Mirror the settings layer into the widgets that display it (after a load or an adopted fleet merge). updates.auto defaults ON until a value exists (the compiled default per docs/updates.md).
    pub(super) fn apply_settings_to_ui(&mut self) {
        let auto = self
            .fleet_settings
            .as_ref()
            .and_then(|fs| fs.effective("updates.auto"))
            .and_then(crate::storage::fleet_settings::as_bool)
            .unwrap_or(true);
        if let Some(cb) = self.settings_autoupdate_check.as_mut() {
            cb.set_checked(auto);
        }
        // Hard logs: DEVICE-LOCAL (an investigation concerns one piece of hardware — never the fleet global) and self-expiring; the stored value is the ARM TIME, the sink owns the 24h window, and the checkbox displays the sink's verdict so the two can't disagree.
        let armed_at = self
            .fleet_settings
            .as_ref()
            .and_then(|fs| fs.device_local("logs.hard"))
            .and_then(crate::storage::fleet_settings::as_osc)
            .filter(|t| *t > 0);
        crate::set_hard_logs(armed_at);
        if let Some(cb) = self.settings_hardlogs_check.as_mut() {
            cb.set_checked(crate::hard_logs_active());
        }
        // Restore THIS DEVICE'S persisted zoom (display.zoom, f32 LE bytes — binary at rest), device-local ONLY: never the fleet global. Zoom is monitor ergonomics, so a device that has never set one keeps the default rather than adopting another screen's value — reading it through `effective` is what made a fresh device jump to a 4K desktop's zoom seconds after launch. Handed to the host as a one-shot absolute request; applies exactly like a user zoom.
        // ONCE per process, and only at load. `apply_settings_to_ui` also runs after EVERY fleet merge that changed anything, and the fleet poll fires every ~15s -- so without this guard the stored zoom was re-applied on a timer, stomping whatever the window was actually at. That is the "scaling elements go half size a few moments after the contacts show up" report: the first pull after contacts load re-armed the restore, and every pull after it did so again. A restore is a startup action, not a steady-state one; the host applies it exactly like a user zoom and the user must stay in control after that.
        if !self.zoom_restored {
            if let Some(ru) = self
                .fleet_settings
                .as_ref()
                .and_then(|fs| fs.device_local("display.zoom"))
                .and_then(crate::storage::fleet_settings::as_f32)
                .filter(|ru| ru.is_finite() && *ru > 0.0)
            {
                self.pending_zoom_restore = Some(ru);
                crate::logf!("SETTINGS: restoring device zoom = {} (one-shot)", ru);
            }
            // Window geometry rides the same one-shot: two typed PAIRS (display.window.pos v_i5[x,y], .size v_u5[w,h] — fluor window_rect desktop units), device-local like zoom. Pos and size are atomic pairs (a move never changes just an x), so each is one value that reads as itself in the inspector. The host clamps into live surfaces at apply, so a rect from an unplugged monitor snaps back on-screen.
            {
                use crate::storage::fleet_settings::{as_i32_pair, as_u32_pair};
                let pos = self.fleet_settings.as_ref().and_then(|fs| fs.device_local("display.window.pos")).and_then(as_i32_pair);
                let size = self.fleet_settings.as_ref().and_then(|fs| fs.device_local("display.window.size")).and_then(as_u32_pair);
                if let (Some((x, y)), Some((w, h))) = (pos, size) {
                    if w > 0 && h > 0 {
                        self.pending_geometry_restore = Some((x, y, w, h));
                        crate::logf!("SETTINGS: restoring window geometry ({} , {}) {}x{} (one-shot)", x, y, w, h);
                    }
                }
            }
            // Armed even when nothing was stored: a device with no saved zoom must not have a LATER fleet merge start restoring one mid-session either.
            self.zoom_restored = true;
        }
    }

    /// Persist the settled window geometry as this DEVICE's two typed pairs — display.window.pos (v_i5 [x,y]) + .size (v_u5 [w,h]), fluor `window_rect` GLOBAL desktop units. Fed by the host's once-per-gesture settle hook (drag-move release / resize-drag end), so no dirty tracking exists. Device-local and UNLINKED like zoom: where a window sits is monitor ergonomics, never fleet-global — but still mirrored thru the fleet's device maps like every device setting.
    pub(super) fn save_window_geometry(&mut self, x: i32, y: i32, w: u32, h: u32) {
        if !self.ensure_fleet_settings() {
            return;
        }
        use vsf::types::tensor::Vector;
        use vsf::VsfType;
        let now = vsf::eagle_time_oscillations();
        let fs = self.fleet_settings.as_mut().unwrap();
        let mut changed = false;
        for (k, v) in [
            ("display.window.pos", VsfType::v_i5(Vector { data: vec![x, y] })),
            ("display.window.size", VsfType::v_u5(Vector { data: vec![w, h] })),
        ] {
            if fs.linked(k) {
                fs.set_link(k, false, now);
            }
            changed |= fs.set(k, v, now);
        }
        if changed {
            crate::logf!("SETTINGS: display.window = ({x},{y}) {w}x{h} (device-local)");
            self.persist_and_push_settings();
        }
    }

    /// Persist the settled zoom as this DEVICE's `display.zoom` (docs/global-vault.md model: per-device value, so it's UNLINKED — zoom is monitor ergonomics, never fleet-global — but still mirrored thru the fleet's device maps like every device setting). f32 LE bytes: binary at rest.
    pub(super) fn save_zoom_setting(&mut self, ru: f32) {
        if !self.ensure_fleet_settings() {
            return;
        }
        let now = vsf::eagle_time_oscillations();
        let fs = self.fleet_settings.as_mut().unwrap();
        if fs.linked("display.zoom") {
            fs.set_link("display.zoom", false, now);
        }
        if fs.set("display.zoom", vsf::VsfType::f5(ru), now) {
            crate::logf!("SETTINGS: display.zoom = {} (device-local)", ru);
            self.persist_and_push_settings();
        }
    }

    /// Set a setting from UI: writes the global (linked, the default) or our device map (unlinked), persists, and pushes to the fleet slot. Returns true if the value actually changed.
    pub(super) fn settings_set(&mut self, key: &str, value: vsf::VsfType) -> bool {
        if !self.ensure_fleet_settings() {
            return false;
        }
        let fs = self.fleet_settings.as_mut().unwrap();
        if !fs.set(key, value, vsf::eagle_time_oscillations()) {
            return false;
        }
        self.persist_and_push_settings();
        true
    }

    /// Encode a device's reaction-recency list as a TYPED VSF field — alternating x{glyph} e6{last_used_osc} value pairs, the same tagged-value discipline as the wire reference field. Never a separator-joined string with decimal stamps (the settings-value cousin of the forbidden `s{idx}_` shape).
    pub(super) fn encode_react_recent(stamps: &[(String, i64)]) -> Vec<u8> {
        use vsf::schema::section::FieldValue;
        let mut values = Vec::with_capacity(stamps.len() * 2);
        for (g, t) in stamps {
            values.push(vsf::VsfType::x(g.clone()));
            values.push(vsf::VsfType::e(vsf::EtType::e6(*t)));
        }
        FieldValue::new("recent", values).flatten()
    }

    /// Decode a reaction-recency blob — lenient: a malformed blob reads as empty (the strip falls back to defaults; the next stamp rewrites it whole).
    pub(super) fn decode_react_recent(bytes: &[u8]) -> Vec<(String, i64)> {
        let mut ptr = 0usize;
        let Ok(field) = vsf::file_format::VsfField::parse(bytes, &mut ptr) else {
            return Vec::new();
        };
        if field.name != "recent" {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut pending: Option<String> = None;
        for v in &field.values {
            match v {
                vsf::VsfType::x(g) => pending = Some(g.clone()),
                vsf::VsfType::e(vsf::EtType::e6(t)) => {
                    if let Some(g) = pending.take() {
                        out.push((g, *t));
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// Fleet-wide reaction RECENCY: each device keeps `glyph → last_used_osc` in its own single-writer key (`react.recent.<device>` — per-device because the settings layer is LWW per key: one shared key would drop concurrent stamps across devices, the `fleet.locked` race shape). The fleet view is the max stamp per glyph across keys. Recency, not tally, ON PURPOSE: all-time counts ossify (an old habit needs to be out-used to dethrone), while most-recent-first keeps the strip current and reshuffles the moment a new codepoint is used — the contacts list's float-to-top, derived from stamps because two devices' bare orders can't merge. Values are typed VSF (see encode_react_recent).
    pub(super) fn react_recency(&self) -> std::collections::HashMap<String, i64> {
        let mut out: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        let Some(fs) = self.fleet_settings.as_ref() else {
            return out;
        };
        for e in fs
            .global
            .iter()
            .filter(|e| !e.tombstone && e.key.starts_with("react.recent."))
        {
            let Some(bytes) = crate::storage::fleet_settings::as_bytes(&e.value) else {
                continue;
            };
            for (g, t) in Self::decode_react_recent(&bytes) {
                let e = out.entry(g).or_insert(t);
                *e = (*e).max(t);
            }
        }
        out
    }

    /// Stamp a just-used reaction NOW on OUR device's recency key (single-writer; see react_recency), pruning to the newest 24 so the blob stays bounded — an old one-off falls off the end instead of living forever. Rides the ordinary settings persist+push, so the order follows the fleet.
    pub(super) fn stamp_react_used(&mut self, glyph: &str) {
        let Some(pk) = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes()) else {
            return;
        };
        let key = format!("react.recent.{}", hex::encode(&pk[..8]));
        let mut stamps: Vec<(String, i64)> = self
            .fleet_settings
            .as_ref()
            .and_then(|fs| fs.effective(&key))
            .and_then(crate::storage::fleet_settings::as_bytes)
            .map(|b| Self::decode_react_recent(&b))
            .unwrap_or_default();
        let now = vsf::eagle_time_oscillations();
        match stamps.iter_mut().find(|(g, _)| g == glyph) {
            Some((_, t)) => *t = now,
            None => stamps.push((glyph.to_string(), now)),
        }
        stamps.sort_by_key(|(_, t)| std::cmp::Reverse(*t));
        stamps.truncate(24);
        let blob = Self::encode_react_recent(&stamps);
        self.settings_set(&key, vsf::VsfType::hR(blob));
    }

    /// The reaction strip, most-recent-first: defaults seeded at stamp zero (so unused ones hold the tail in default order — the sort is stable), every used glyph floats by its fleet-wide newest stamp, custom glyphs join the pool in sorted order for determinism.
    pub(super) fn ranked_reactions(&self) -> Vec<String> {
        let recency = self.react_recency();
        let mut pool: Vec<String> = DEFAULT_REACTIONS.iter().map(|s| s.to_string()).collect();
        let mut extras: Vec<String> = recency
            .keys()
            .filter(|g| !pool.iter().any(|p| p == *g) && !g.is_empty())
            .cloned()
            .collect();
        extras.sort();
        pool.extend(extras);
        pool.sort_by_key(|g| std::cmp::Reverse(recency.get(g).copied().unwrap_or(0)));
        pool
    }

    /// Flip a key's link on this device (unlink = set locally from now on; relink = follow the fleet). Persists + pushes on change.
    /// Not yet wired to a UI (the per-key link toggle is a designed settings-page control that hasn't landed) — kept as the API half so the storage layer's `set_link` has its app-side entry point.
    #[allow(dead_code)]
    pub(super) fn settings_set_link(&mut self, key: &str, linked: bool) -> bool {
        if !self.ensure_fleet_settings() {
            return false;
        }
        let fs = self.fleet_settings.as_mut().unwrap();
        if !fs.set_link(key, linked, vsf::eagle_time_oscillations()) {
            return false;
        }
        self.persist_and_push_settings();
        true
    }

    /// Build the You-page field boxes ONCE (HitId is a scarce u16 — never rebuild): the standard taxonomy, then any custom fields registered in `profile._custom` (one `id\tlabel` per line). Idempotent; values are loaded separately by [`Self::load_you_fields`]; expandable-field instances (addr2, email3, …) are appended by [`Self::sync_expandable_fields`].
    pub(super) fn build_you_fields(&mut self) {
        if !self.you_fields.is_empty() {
            return;
        }
        for &(id, label, tier) in STD_PROFILE_FIELDS {
            let tagged = EXPANDABLE_FIELDS.iter().any(|&(b, tag)| b == id && tag);
            let tb = Textbox::new(&mut self.hit_counter, 0., 0., 1., 1., 12.);
            let tag_tb = tagged.then(|| Textbox::new(&mut self.hit_counter, 0., 0., 1., 1., 12.));
            self.you_fields.push(ProfileField {
                field_id: id.to_string(),
                label: label.to_string(),
                tier,
                custom: false,
                tb,
                tag_tb,
                share_cb: None,
            });
        }
        let custom = self
            .fleet_settings
            .as_ref()
            .and_then(|fs| fs.effective("profile._custom"))
            .and_then(crate::storage::fleet_settings::as_text)
            .unwrap_or_default();
        for line in custom.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (id, label) = line.split_once('\t').unwrap_or((line, line));
            let id = id.trim().to_string();
            let label = label.trim().to_string();
            if id.is_empty() || self.you_fields.iter().any(|f| f.field_id == id) {
                continue;
            }
            let tb = Textbox::new(&mut self.hit_counter, 0., 0., 1., 1., 12.);
            self.you_fields.push(ProfileField {
                field_id: id,
                label,
                tier: "custom",
                custom: true,
                tb,
                tag_tb: None,
                share_cb: None,
            });
        }
    }

    /// Attach a default-share checkbox to every field that lacks one — except the display name, which is public and always shared (no box at all). Checked state loads from `share.<id>`; runs each frame on the You page so expansion/custom fields born after load get theirs too.
    pub(super) fn ensure_share_checkboxes(&mut self) {
        for pf in self.you_fields.iter_mut() {
            if pf.field_id == "name" || pf.share_cb.is_some() {
                continue;
            }
            let checked = self
                .fleet_settings
                .as_ref()
                .and_then(|fs| fs.effective(&format!("share.{}", pf.field_id)))
                .and_then(crate::storage::fleet_settings::as_bool)
                .unwrap_or(false);
            pf.share_cb = Some(crate::ui::settings_widgets::Checkbox::new(
                &mut self.hit_counter,
                "",
                0.,
                0.,
                1.,
                1.,
                12.,
                checked,
            ));
        }
    }

    /// The instances of an expandable base currently in `you_fields`, in order: index of the last one + how many there are. Instance ids are `base`, `base2`, `base3`, … (a bare-digit suffix — so `addr` matches `addr2` but never `addr_work` or a custom `address_notes`).
    pub(super) fn expandable_instances(&self, base: &str) -> (Option<usize>, usize) {
        let mut last = None;
        let mut count = 0;
        for (i, f) in self.you_fields.iter().enumerate() {
            let suffix = match f.field_id.strip_prefix(base) {
                Some(s) => s,
                None => continue,
            };
            if suffix.is_empty()
                || (!suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()))
            {
                last = Some(i);
                count += 1;
            }
        }
        (last, count)
    }

    /// Keep every expandable base one-empty-instance ahead: whenever the LAST instance of addr/email/phone/… is non-empty, insert a fresh empty next instance right after it (Address 2, Email 3, …). Called from load and from the per-frame You layout pass, so the next slot appears AS you fill the previous — and never before. Singletons (SSN, passport, licence, …) aren't in [`EXPANDABLE_FIELDS`] and never expand. Returns true if a field was added (row plan changed).
    pub(super) fn sync_expandable_fields(&mut self) -> bool {
        let mut added = false;
        for &(base, tagged) in EXPANDABLE_FIELDS {
            let (last_idx, count) = self.expandable_instances(base);
            let Some(li) = last_idx else { continue };
            if self.you_fields[li].tb.chars.is_empty() {
                continue;
            }
            let n = count + 1;
            let id = format!("{base}{n}");
            if self.you_fields.iter().any(|f| f.field_id == id) {
                continue;
            }
            let (base_label, tier) = {
                let bf = &self.you_fields[li];
                (
                    STD_PROFILE_FIELDS
                        .iter()
                        .find(|&&(fid, _, _)| fid == base)
                        .map(|&(_, l, _)| l.to_string())
                        .unwrap_or_else(|| bf.label.clone()),
                    bf.tier,
                )
            };
            let tb = Textbox::new(&mut self.hit_counter, 0., 0., 1., 1., 12.);
            let tag_tb = tagged.then(|| Textbox::new(&mut self.hit_counter, 0., 0., 1., 1., 12.));
            self.you_fields.insert(
                li + 1,
                ProfileField {
                    field_id: id,
                    label: format!("{base_label} {n}"),
                    tier,
                    custom: false,
                    tb,
                    tag_tb,
                    share_cb: None,
                },
            );
            added = true;
        }
        added
    }

    /// Reload each field box (and its tag box) from its stored `profile.<id>` / `profile.<id>_label` value (building the boxes first if needed), so the You page shows the fleet-synced state on open. Stored expandable instances (addr2, email3, …) are materialised first so a value saved on a sibling device gets a box here. Only overwrites a box when the stored value differs, so an in-progress edit survives a stray reload. Flips `you_fields_loaded` so the per-frame layout pass stops reloading.
    pub(super) fn load_you_fields(&mut self, text: &mut fluor::text::TextRenderer) {
        self.ensure_fleet_settings();
        self.build_you_fields();
        // Materialise boxes for every STORED instance of each expandable base (up thru the last n with a value — gap-tolerant, a cleared middle instance keeps its box).
        for &(base, tagged) in EXPANDABLE_FIELDS {
            let stored_max = (2..=24)
                .filter(|n| {
                    self.fleet_settings
                        .as_ref()
                        .and_then(|fs| fs.effective(&format!("profile.{base}{n}")))
                        .and_then(crate::storage::fleet_settings::as_text)
                        .is_some_and(|v| !v.is_empty())
                })
                .max()
                .unwrap_or(1);
            for n in 2..=stored_max {
                let id = format!("{base}{n}");
                if self.you_fields.iter().any(|f| f.field_id == id) {
                    continue;
                }
                let (li, _) = self.expandable_instances(base);
                let Some(li) = li else { continue };
                let base_label = STD_PROFILE_FIELDS
                    .iter()
                    .find(|&&(fid, _, _)| fid == base)
                    .map(|&(_, l, _)| l.to_string())
                    .unwrap_or_else(|| base.to_string());
                let tier = self.you_fields[li].tier;
                let tb = Textbox::new(&mut self.hit_counter, 0., 0., 1., 1., 12.);
                let tag_tb =
                    tagged.then(|| Textbox::new(&mut self.hit_counter, 0., 0., 1., 1., 12.));
                self.you_fields.insert(
                    li + 1,
                    ProfileField {
                        field_id: id,
                        label: format!("{base_label} {n}"),
                        tier,
                        custom: false,
                        tb,
                        tag_tb,
                        share_cb: None,
                    },
                );
            }
        }
        // Prefill every box (value + tag) from its stored setting. Gather first — insert_str needs `text` while `fleet_settings` reads borrow self.
        let stored: Vec<(String, Option<String>)> = self
            .you_fields
            .iter()
            .map(|f| {
                let get = |key: &str| {
                    self.fleet_settings
                        .as_ref()
                        .and_then(|fs| fs.effective(key))
                        .and_then(crate::storage::fleet_settings::as_text)
                        .unwrap_or_default()
                };
                (
                    get(&format!("profile.{}", f.field_id)),
                    f.tag_tb
                        .as_ref()
                        .map(|_| get(&format!("profile.{}_label", f.field_id))),
                )
            })
            .collect();
        for (f, (val, tag_val)) in self.you_fields.iter_mut().zip(stored) {
            let cur: String = f.tb.chars.iter().collect();
            if cur != val {
                f.tb.clear();
                f.tb.insert_str(&val, text);
            }
            if let (Some(tag_tb), Some(tv)) = (f.tag_tb.as_mut(), tag_val) {
                let cur: String = tag_tb.chars.iter().collect();
                if cur != tv {
                    tag_tb.clear();
                    tag_tb.insert_str(&tv, text);
                }
            }
        }
        // A stored last-instance value immediately earns its empty successor.
        self.sync_expandable_fields();
        self.you_fields_loaded = true;
    }

    /// "Update" → write every field's current text (and tag) to its `profile.<id>` / `profile.<id>_label` setting and push the whole batch to the fleet ONCE (not one network push per field). Empty fields are saved as empty (a cleared value, legal). Reports what happened via the status toast.
    pub(super) fn save_you_profile(&mut self) {
        if !self.ensure_fleet_settings() {
            return;
        }
        let now = vsf::eagle_time_oscillations();
        // Snapshot (key, value) first so we don't hold a you_fields borrow across the fleet_settings mutation.
        let mut pairs: Vec<(String, vsf::VsfType)> = Vec::new();
        for f in &self.you_fields {
            let v: String = f.tb.chars.iter().collect();
            pairs.push((
                format!("profile.{}", f.field_id),
                vsf::VsfType::x(v.trim().to_string()),
            ));
            if let Some(tag_tb) = &f.tag_tb {
                let t: String = tag_tb.chars.iter().collect();
                pairs.push((
                    format!("profile.{}_label", f.field_id),
                    vsf::VsfType::x(t.trim().to_string()),
                ));
            }
        }
        let fs = self.fleet_settings.as_mut().unwrap();
        let mut changed = false;
        for (key, val) in pairs {
            // Don't create an empty entry for a field that was never filled and is still blank — only write blanks that CLEAR an existing value.
            let absent = fs
                .effective(&key)
                .and_then(crate::storage::fleet_settings::as_text)
                .map_or(true, |c| c.is_empty());
            let val_empty = matches!(&val, vsf::VsfType::x(s) if s.is_empty());
            if val_empty && absent {
                continue;
            }
            if fs.set(&key, val, now) {
                changed = true;
            }
        }
        if changed {
            self.persist_and_push_settings();
            // Friends learn the new name on their next ping cycle (the pong carries it).
            self.publish_profile_name();
            self.publish_avatar_pin();
            self.ready_toast = Some("Profile saved \u{221a}".to_string());
        } else {
            self.ready_toast = Some("No changes".to_string());
        }
    }

    /// "Add" → register the label typed in the add box as a custom field (e.g. "Address 2" → id `address_2`), append its box, and persist the `profile._custom` registry so it reloads next launch. No-op on an empty label or a duplicate id.
    pub(super) fn add_custom_field(&mut self) {
        let raw: String = match self.you_add_textbox.as_ref() {
            Some(tb) => tb.chars.iter().collect(),
            None => return,
        };
        let label = raw.trim().to_string();
        if label.is_empty() {
            return;
        }
        // Sanitise the label to a field_id: lowercase ascii-alphanumeric, every other run → a single underscore, trimmed.
        let mut id = String::new();
        let mut pending_us = false;
        for c in label.chars() {
            if c.is_ascii_alphanumeric() {
                if pending_us && !id.is_empty() {
                    id.push('_');
                }
                pending_us = false;
                id.push(c.to_ascii_lowercase());
            } else {
                pending_us = true;
            }
        }
        if id.is_empty() {
            return;
        }
        if self.you_fields.iter().any(|f| f.field_id == id) {
            self.ready_toast = Some("That field already exists".to_string());
            if let Some(tb) = self.you_add_textbox.as_mut() {
                tb.clear();
            }
            return;
        }
        let tb = Textbox::new(&mut self.hit_counter, 0., 0., 1., 1., 12.);
        self.you_fields.push(ProfileField {
            field_id: id,
            label: label.clone(),
            tier: "custom",
            custom: true,
            tb,
            tag_tb: None,
            share_cb: None,
        });
        // Persist the whole custom registry (id\tlabel per line) so the field survives a relaunch.
        let reg = self
            .you_fields
            .iter()
            .filter(|f| f.custom)
            .map(|f| format!("{}\t{}", f.field_id, f.label))
            .collect::<Vec<_>>()
            .join("\n");
        self.settings_set("profile._custom", vsf::VsfType::x(reg));
        if let Some(tb) = self.you_add_textbox.as_mut() {
            tb.clear();
        }
        self.ready_toast = Some(format!("Added \u{201c}{label}\u{201d}"));
    }

    pub(super) fn persist_and_push_settings(&mut self) {
        if let (Some(fs), Some(storage)) = (self.fleet_settings.as_ref(), self.storage.as_ref()) {
            if let Err(e) = crate::storage::fleet_settings::save_fleet_settings(fs, storage) {
                crate::logf!("SETTINGS: persist failed: {}", e);
            }
        }
        self.spawn_settings_push();
    }

    /// Push our settings layers to the fleet slot (off-thread, best-effort). Pull-merge-push: the slot's current state folds in first, so a concurrent sibling write converges by CRDT instead of being clobbered — same doctrine as push_roster's roster-preserving pull.
    pub(super) fn spawn_settings_push(&self) {
        use crate::network::fgtw::fleet;
        let Some(fs) = self.fleet_settings.as_ref() else {
            return;
        };
        // The live ROSTER rides too — the mirror of the roster push carrying live settings: two pull-merge-push writers race, and a loser that carries every layer it holds can revert nothing it knows (the boot pin-mint vs reconcile race, field 2026-08-02).
        let ours = fgtw::fstate::FleetState {
            roster: self.current_roster(),
            global_settings: fs.global.clone(),
            device_settings: fs.devices.clone(),
        };
        let (Some(hp), Some(kp), Some(fleet_key)) = (
            self.our_handle_proof(),
            self.device_keypair.clone(),
            self.fleet_key_cached(),
        ) else {
            // TORCH the bail: this silent return ate every profile push made before the fleet key/hp settled — the worker slot sat at ONE global setting while the You page looked saved, and a nuked device restored to an empty profile (2026-07-26). The per-session re-push in tick retries once the keys exist.
            crate::log("SETTINGS: push skipped — hp/keypair/fleet key not ready yet (will re-push when settled)");
            return;
        };
        std::thread::spawn(move || {
            // A FAILED pull must not become a destructive push (same rule as fleet::push_roster). The comment below — empty ours.roster leaves the slot's roster untouched — holds ONLY when the slot actually pulled: rebasing on `default()` after an error unions an empty roster with an empty base, so a SETTINGS push would silently wipe the fleet's ROSTER. `Ok(None)` is different and safe: nothing is published yet, so there is nothing to lose.
            let slot = match fleet::pull_fstate(&hp, &fleet_key) {
                Ok(Some(s)) => s,
                Ok(None) => fgtw::fstate::FleetState::default(),
                Err(e) => {
                    crate::logf!("SETTINGS: push skipped — pull failed ({}), so the merge base is unknown; pushing now would overwrite the fleet's roster", e);
                    return;
                }
            };
            // Empty ours.roster merges to the slot's roster untouched (union) — settings pushes never disturb the roster.
            let merged = fgtw::fstate::merge_fstate(slot, ours);
            match fleet::push_fstate(&hp, &kp, &fleet_key, &merged) {
                Ok(()) => crate::log("SETTINGS: pushed to the fleet slot"),
                Err(e) => crate::logf!("SETTINGS: push failed: {}", e),
            }
        });
    }

    /// Publish this device's contact roster to the fleet slot (off-thread, best-effort). No-op if we have no contacts to share or lack the key/membership. COALESCED: one push in flight at a time — a request landing mid-flight queues exactly one follow-up that re-snapshots the roster on the completion edge, so back-to-back launch edges (re-push, weave claims, keepalive stamps, pong adoptions, reconciles) become one or two round trips instead of a concurrent racer per edge.
    pub(super) fn spawn_roster_push(&mut self) {
        use crate::network::fgtw::fleet;
        if self.roster_push_rx.is_some() {
            self.roster_push_queued = true;
            return;
        }
        let entries = self.current_roster();
        if entries.is_empty() {
            return;
        }
        let (Some(hp), Some(kp), Some(fleet_key)) = (
            self.our_handle_proof(),
            self.device_keypair.clone(),
            self.fleet_key_cached(),
        ) else {
            return;
        };
        // Live settings ride along so a race-losing push can't revert them (the boot pin-mint vs reconcile race).
        let live = self
            .fleet_settings
            .as_ref()
            .map(|fs| (fs.global.clone(), fs.devices.clone()));
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        self.roster_push_rx = Some(rx);
        let wake = self.event_proxy.clone();
        std::thread::spawn(move || {
            if let Err(e) = fleet::push_roster_with_settings(&hp, &kp, &fleet_key, &entries, live) {
                crate::logf!("FLEET: roster push failed: {}", e);
            }
            // Success or failure, the slot is done being written — release it and let a queued follow-up (which re-snapshots) carry anything newer.
            let _ = tx.send(());
            if let Some(w) = wake.as_ref() {
                let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
            }
        });
    }
}
