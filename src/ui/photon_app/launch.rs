//! The launch/attest flow — handle submit, the attest query, query/search result handling, and launch-screen focus helpers.

use super::*;

impl PhotonApp {
    /// Send the current textbox contents as an attestation query and transition Launch → Attesting. Called from Enter in the textbox path and from clicking the Attest button — same submit path. No-op if the textbox is empty, HandleQuery wasn't constructed (init failure path), or the launch sub-state forbids submission (`LaunchState::Attesting` — query already in flight; second submit would double-spend the ~5s memory-hard proof).
    pub(super) fn submit_handle(&mut self) {
        if let AppState::Launch(s) = &self.state {
            if !s.can_edit_handle() {
                return;
            }
        }
        // In JOIN mode the launch textbox feeds the pairing flow (handle then words), not a fresh attestation.
        if self.launch_add_mode {
            self.submit_join_step(None);
            return;
        }
        let handle: String = match self.textbox.as_ref() {
            Some(tb) => tb.chars.iter().collect(),
            None => return,
        };
        if handle.is_empty() {
            return;
        }
        // ONE IDENTITY PER DEVICE (docs/lifecycle.md D2): the binding marker names the identity this device carries; a different typed handle refuses HERE — before the ~1s memory-hard proof is spent. The check is cheap (the typed handle's party id derives without the proof — and the handle bytes go to the derivation RAW, byte-precise). Typing the BOUND identity's own handle passes and resumes normally; unbinding is a wipe (Panel → Security). The worker's one-owner index backstops a scrubbed marker.
        {
            if let Some(bound) = crate::storage::device_binding::bound_party_id() {
                let typed_pid = crate::crypto::clutch::identity_party_id(
                    &crate::types::Handle::to_identity_seed(&handle),
                );
                if typed_pid != bound {
                    crate::log("attest: DEVICE BUSY — this device is bound to another identity; refusing before the proof");
                    self.state = AppState::Launch(LaunchState::Error(
                        "this device already carries an identity \u{2014} type its handle to resume. To hand the device to someone else: put that identity on another device first, then Remove & shred (Settings \u{2192} Security)".to_string(),
                    ));
                    self.refocus_handle_select_all();
                    return;
                }
            }
        }
        // A press FROM the Confirm interstitial is the deliberate second act: claim the (probed-Fresh) handle with the roots the probe already derived — no second proof, no permanence warning re-shown. GUARD: fire the stashed roots ONLY if the box still holds the handle they were derived from. Every edit path tears Confirm down, but this is the invariant that survives a missed one — firing stale roots attests a DIFFERENT identity than the box shows (observed: probe handle A, retype to taken handle B, press → attested as A, user believes they claimed B). On mismatch the press falls thru to a fresh probe of the current text.
        if matches!(self.state, AppState::Launch(LaunchState::Confirm)) {
            if let Some(btn) = self.attest_btn.as_mut() {
                btn.set_label("Attest");
            }
            let matches_probe = self.probed_handle.as_deref()
                == Some(crate::types::Handle::canonical(&handle).as_str());
            let session = self.probed_session.take();
            self.probed_handle = None;
            if let (Some(session), true) = (session, matches_probe) {
                self.fire_attest_query_with_roots(session);
                return;
            }
            crate::log(
                "attest: confirm-press text no longer matches the probed handle — re-probing",
            );
        }
        // First press: PROBE the handle against the network before deciding anything. The ~1s proof runs here (once); the branch (permanence warning / add-this-device / resume / taken) is chosen in `on_query_result` from the probe outcome, so "forever" is never shown for a handle that already has a fleet.
        if let Some(hq) = self.handle_query.as_ref() {
            hq.probe(handle);
            self.state = AppState::Launch(LaunchState::Attesting);
            self.change_focus(None);
        }
    }

    pub(super) fn fire_attest_query_with_roots(&mut self, session: tohu::SessionIdentity) {
        if let Some(hq) = self.handle_query.as_ref() {
            hq.query_first_attest_with_roots(session);
            self.state = AppState::Launch(LaunchState::Attesting);
            self.change_focus(None);
        }
    }

    // (fire_attest_query — the string-based attest that BYPASSED the permanence interstitial — is deliberately gone: every launch-screen claim now flows probe → Confirm → roots-verified fire, so no path can attest a string the user didn't just confirm.)

    /// Handle a [`QueryResult`] arriving from HandleQuery's background worker. On success, stashes the proof, loads the device avatar + contacts, and transitions to the Ready screen; on rejection/error, drops to `LaunchState::Error` and refocuses the handle field.
    pub(super) fn on_query_result(&mut self, result: QueryResult) {
        use num_bigint::BigUint;
        // Resume painted Ready optimistically from local state, so a result arriving while we're already past Launch is a background refresh (presence / contacts / cloud-merge), NOT a first attest. This gates the bailouts below: a transient network error must not knock a valid local session off Ready.
        let in_app = !matches!(self.state, AppState::Launch(_));
        match result {
            QueryResult::Probe { outcome, session } => {
                use crate::network::handle_query::ProbeOutcome;
                // The attest three-way branch, chosen from the network probe (see submit_handle). A probe result arriving while already in-app is stale (we navigated on) — ignore it.
                if in_app {
                    return;
                }
                match outcome {
                    ProbeOutcome::Fresh => {
                        // Genuine fresh claim — NOW show the permanence warning, stashing the probed roots (and the canonical handle they belong to) so the confirm press claims without re-deriving the proof.
                        self.probed_session = Some(session);
                        self.probed_handle = self.textbox.as_ref().map(|tb| {
                            crate::types::Handle::canonical(&tb.chars.iter().collect::<String>())
                        });
                        self.state = AppState::Launch(LaunchState::Confirm);
                        if let Some(btn) = self.attest_btn.as_mut() {
                            btn.set_label("Yes — forever");
                        }
                    }
                    ProbeOutcome::Member => {
                        // Already in the fleet — just attest (resume). No warning; the announce passes the membership gate.
                        self.fire_attest_query_with_roots(session);
                    }
                    ProbeOutcome::JoinOurs => {
                        // A fleet exists whose genesis binds to this handle's identity — EITHER the user's own other device OR a collider who typed the same name; the derivation makes them indistinguishable (docs/lifecycle.md D1). KnownHandle presents both readings; NOTHING posts to the network (no bind request, no beacon) until "it's mine" is pressed — the old straight-to-join route made every collision read as device pairing and spammed the owner's registry with stranger requests.
                        crate::log("attest: handle already has a fleet → KnownHandle fork");
                        self.probed_session = Some(session);
                        self.state = AppState::Launch(LaunchState::KnownHandle);
                    }
                    ProbeOutcome::Taken => {
                        self.state = AppState::Launch(LaunchState::Error(
                            "this handle is taken".to_string(),
                        ));
                        self.refocus_handle_select_all();
                    }
                }
            }
            QueryResult::Success(data) => {
                if let Some(hq) = self.handle_query.as_ref() {
                    hq.set_handle_proof(data.handle_proof);
                }
                // Sync our own avatar with FGTW now that the handle_proof is set — newest-wins, so a copy this identity set on another device propagates here, and ours propagates out, without either clobbering a fresher one. (Was a blind one-way upload, which could overwrite a newer server copy with a stale local one.)
                self.spawn_avatar_sync();
                // Live fleet propagation: subscribe to hub events for this identity (idempotent across resumes/re-attests in one run).
                self.spawn_fleet_event_sub();
                // Pubkey emitted as voca-encoded camelCase so a user reading the log can double-click + paste the value as a single word (matches `Development:` key lines from handle_query.rs). The handle is deliberately NOT logged — Photon never surfaces the plaintext handle.
                crate::logf!(
                    "attestation success: pubkey = {}",
                    voca::encode(BigUint::from_bytes_be(&data.handle_proof))
                );
                // Adopt the session roots the worker just derived + persisted (register-shaped, no handle string). Shared across the user's TOKEN apps, gone at logout; a close/reopen resumes from these without re-typing or recomputing the proof. Fall back to the roots carried in the attest result if the tohu READ-BACK comes up empty (a persist failure must not leave THIS RUN sessionless — that made the avatar picker report "not attested" seconds after a successful attest). vault_seed == identity_seed mirrors the worker's derivation (handle_query FirstAttest).
                self.session = tohu::session().or(Some(tohu::SessionIdentity {
                    identity_seed: data.identity_seed,
                    vault_seed: data.identity_seed,
                    handle_proof: data.handle_proof,
                }));
                // Blob name key (keyed filenames — the possession-oracle close) as soon as the attested seed exists.
                if let Some(s) = &self.session {
                    crate::storage::blob_init_names(&s.identity_seed);
                }
                // An armed lock-out fires HERE and only here — the handle just proved itself. A different identity attesting (thief typing their own name) discards the arm instead: their lock was never this fleet's to execute.
                if let Some((pk, armer_hp, name)) = self.pending_lock.take() {
                    if data.handle_proof == armer_hp {
                        self.lock_out_device(pk);
                        crate::logf!(
                            "FLEET: {} locked out — handle confirmed, key rotating away",
                            name
                        );
                        self.ready_toast = Some(format!(
                            "{name} locked out \u{2014} it can no longer act in this fleet."
                        ));
                    } else {
                        crate::logf!(
                            "FLEET: armed lock-out of {} DISCARDED — a different identity attested",
                            name
                        );
                    }
                }
                // The armed UNLOCK fires under the same proof: the handle just re-proved the owner. A different identity attesting discards it identically.
                if let Some((pk, armer_hp, name)) = self.pending_unlock.take() {
                    if data.handle_proof == armer_hp {
                        crate::logf!("FLEET: {} unlock confirmed — clearing the worker verdict + fleet marker", name);
                        self.unlock_fleet_device(pk, &name);
                    } else {
                        crate::logf!(
                            "FLEET: armed unlock of {} DISCARDED — a different identity attested",
                            name
                        );
                    }
                }
                // Durable reconcile: re-push every locally-locked device to the worker now we're freshly online, so a lock whose immediate push failed (offline at lock time, stale-chain race) still reaches the worker before the stolen device could be wiped+reattested. Idempotent puts, and usually a no-op empty loop (most fleets lock nothing).
                self.reconcile_worker_locks();
                self.reconcile_worker_unlocks();
                // Bind the device to this identity (docs/lifecycle.md D2): the marker refuses a second identity at the NEXT submit, before its proof is spent. Idempotent on resume; cleared only by a wipe.
                crate::storage::device_binding::bind(&crate::crypto::clutch::identity_party_id(
                    &data.identity_seed,
                ));
                // UNATTENDED MODE: if the operator has opted this box into auto-attest-on-reboot, refresh the reboot capsule now that we hold a live session, so the NEXT reboot resumes without a handle. No-op (and the capsule is cleared) when the toggle is off — see refresh_reboot_capsule.
                self.refresh_reboot_capsule();
                self.pending_broadcast_signal = 1;
                // Re-anchor the sticky-freshness timer off this fresh post so the periodic ensure doesn't immediately double-fire (and so a re-attest after a logout re-schedules cleanly rather than riding a stale pre-logout deadline).
                self.next_session_broadcast = Some(
                    Instant::now()
                        + std::time::Duration::from_secs(crate::jitter(3600).max(60) as u64),
                );
                self.vault_degraded = data.vault_degraded;
                // The worker already loaded this device's avatar (keyed on identity_seed) into `data.avatar_pixels`; colour-convert it to BT.2020 γ=2.0 for the Ready screen. `None` = storage-miss → grey placeholder.
                if let Some(vsf_rgb) = &data.avatar_pixels {
                    self.device_avatar_pixels =
                        Some(crate::ui::colour_convert::vsf_rgb_to_bt2020(vsf_rgb));
                    self.device_avatar_scaled = None;
                    self.device_avatar_scaled_diameter = 0;
                }
                // Initialize local encrypted storage from the session's vault_seed + device secret. open_session_vault: on a resume this returns the SAME engine the resume path already opened (and the attest worker holds) — a second independent engine on the live vault is the corruption class, not a refresh.
                if let Some(session) = &self.session {
                    if let Some(kp) = &self.device_keypair {
                        let device_secret = *kp.secret.as_bytes();
                        match crate::storage::open_session_vault(
                            session.identity_seed,
                            session.vault_seed,
                            device_secret,
                        ) {
                            Ok(s) => self.storage = Some(s),
                            Err(e) => {
                                crate::logf!("STORAGE: init failed: {}", e);
                                // Hard vault-open failure → surface the red banner (overrides any `false` from `data.vault_degraded` set just above — a local open failure is worse).
                                self.vault_degraded = true;
                            }
                        }
                    }
                }
                // If we just joined a fleet, the vault is now open — persist the fleet key we recovered from the fan-out during pairing so this device shares the fleet's private state.
                if let Some(k) = self.pending_fleet_key.take() {
                    // A fresh fleet key is an epoch-worthy edge: the next checkpoint folds it (rotation = the PCS boundary).
                    self.ckpt_mint_due = true;
                    self.fleet_key_store(&k);
                    crate::log("FLEET: stored fleet key recovered during pairing");
                }
                // Establish (genesis founder) or refresh (existing device, picks up a rotation) the fleet key from the fan-out and cache it, so the roster/state seal uses the current key.
                self.spawn_fleet_key_sync();
                // Sync the fleet's shared contact roster. The pull is DEFERRED to tick (via the flag) because the fleet key is written by the async sync above — pulling here would race it and read an empty/stale key. On the INITIAL attest also push our existing set so a fleet formed before roster-sync existed seeds FGTW for newly-joined devices. Pulled contacts merge in as Pending stubs and re-CLUTCH on this device's own key (drained in tick).
                self.needs_initial_roster_pull = true;
                // ~8 attempts × the pull's ~150ms round-trip ≈ 1.2s — enough to outlast the fan-out key sync writing the current (post-rotation) key, after which the retry decrypts the roster cleanly.
                self.roster_pull_retries_left = 8;
                if !in_app {
                    self.spawn_roster_push();
                }
                // Merge incoming contacts with any already loaded locally — union by handle_proof so contacts added on another device (via FGTW/cloud) appear without losing locally-added ones. Siblings are excluded from domination: they carry OUR handle_proof and must not suppress a merged self-contact.
                let mut added = 0usize;
                let mut merged_ids: Vec<(ContactId, [u8; 32])> = Vec::new();
                for incoming in &data.contacts {
                    let dominated = self
                        .contacts
                        .iter()
                        .any(|c| !c.is_sibling && c.handle_proof == incoming.handle_proof);
                    if !dominated {
                        merged_ids.push((incoming.id.clone(), incoming.handle_hash));
                        self.contacts.push(incoming.clone());
                        added += 1;
                    }
                }
                if added > 0 {
                    crate::logf!(
                        "UI: merged {} new contact(s) from FGTW (total: {})",
                        added,
                        self.contacts.len()
                    );
                    // Register the merged contacts' pubkeys so the checker answers their pings, and kick CLUTCH keygen for any that arrived Pending without keypairs. The resume path (load_all_contacts) already does this for locally-stored contacts, but cloud/FGTW-merged contacts land here AFTER that ran — without this they sit Pending forever with no keypairs, no offer, no connection (exactly what broke after a []n nuke wiped the local vault and contacts came back only via cloud).
                    self.reseed_contact_pubkeys();
                    // Kick at most ONE keygen now; the rest are serialized by spawn_next_pending_keygen (called each tick) so we never run two McEliece keygens at once — two in parallel on launch starved the UI thread (the "first launch hangs" symptom). The Pending + keyless contacts are picked up one at a time as each keygen completes.
                    self.spawn_next_pending_keygen();
                }
                // Merge the friendship chains the worker loaded from disk into the live map. Without this, resumed contacts have no chains in self.friendship_chains and sending fails with "friendship chains missing" — even though storage loaded them fine. Only add ids we don't already hold; an in-session chain (built at ceremony completion) is fresher than a disk copy, so never clobber it.
                let mut merged_chains = 0usize;
                for (fid, chains) in data.friendships {
                    if !self.friendship_chains.iter().any(|(id, _)| *id == fid) {
                        self.friendship_chains.push((fid, chains));
                        merged_chains += 1;
                    }
                }
                if merged_chains > 0 {
                    crate::logf!(
                        "UI: merged {} friendship chain(s) from disk (total: {})",
                        merged_chains,
                        self.friendship_chains.len()
                    );
                    self.update_sync_records();
                }
                // Same flag-day edge as the resume path: contacts whose pre-v8 chains the worker's loader rejected re-clutch here.
                self.reclutch_chainless_contacts("attest load");
                // Conversations the load worker read from disk: adopt any we don't hold. Never clobber — an in-session conversation (rows landed while the worker ran) is fresher than its disk copy.
                let mut merged_convs = 0usize;
                for conv in data.conversations {
                    if !self.conversations.iter().any(|v| v.id() == conv.id()) {
                        self.conversations.push(conv);
                        merged_convs += 1;
                    }
                }
                if merged_convs > 0 {
                    crate::logf!(
                        "UI: merged {} conversation(s) from disk (total: {})",
                        merged_convs,
                        self.conversations.len()
                    );
                }
                // Seed `our_reflexive` from FGTW's signed observation if we have nothing better yet. This is what lets `publish_self_peer_record` sign a record on a FIRST launch: until now it needed a peer-echoed pong, but a pong needs a reachable path, which needs an address a peer could learn, which needs a published record -- a cycle nothing could enter, which is why `PHONEBOOK:` never appeared in a log and gossip carried zero records.
                //
                // Deliberately does NOT overwrite an address we already hold: a peer echo comes off the live UDP data socket and is quorum-corroborated, while the seed sees a TLS flow and is only exactly right for a cone NAT. Seed first, correct later.
                if self.our_reflexive.is_none() {
                    if let Some(addr) = data.observed_addr {
                        self.our_reflexive = Some(addr);
                        crate::logf!("TRAVERSE: reflexive address seeded from the FGTW ack = {} (a peer echo will refine it)", addr);
                    }
                }
                if data.seed_identity_count > 0 {
                    self.seed_identity_count = data.seed_identity_count;
                }
                // Fleet weave: load persisted siblings if this attest path didn't come thru the resume loader (e.g. JOIN-flow first attest, or []u→re-attest), then re-fold OUR OWN chain — the members drain routes our hp to reconcile_fleet_siblings, which creates Pending sibling contacts for any member device we don't hold yet. Fires on every background refresh too, giving a ~30s catch-up cadence for fleet changes we missed.
                if let Some(storage) = self.storage.as_ref().cloned() {
                    if !self.contacts.iter().any(|c| c.is_sibling) {
                        let siblings = crate::storage::contacts::load_all_siblings(
                            data.handle_proof,
                            &storage,
                        );
                        if !siblings.is_empty() {
                            crate::logf!(
                                "SIBLING: loaded {} sibling(s) from local vault on attest",
                                siblings.len()
                            );
                            let fids: Vec<crate::types::FriendshipId> =
                                siblings.iter().filter_map(|c| c.friendship_id).collect();
                            for (fid, chains) in
                                crate::storage::friendship::load_all_friendships(&fids, &storage)
                            {
                                if !self.friendship_chains.iter().any(|(id, _)| *id == fid) {
                                    self.friendship_chains.push((fid, chains));
                                }
                            }
                            self.contacts.extend(siblings);
                            self.reseed_contact_pubkeys();
                        }
                    }
                    self.spawn_contact_fleet_refresh(vec![data.handle_proof]);
                }
                // Notes-to-self is not created here either — see the resume path. Adding yourself is a deliberate act.
                self.hints_dismissed = false;
                // Only flip to Ready on the INITIAL attest (we were still on the Launch screen). This Success branch also fires on every recurring background resume refresh — if the user has already navigated in-app (Ready, or inside a Conversation), forcing Ready here would yank them out of an open chat back to the contact list each sweep.
                if !in_app {
                    self.state = AppState::Ready;
                }
            }
            QueryResult::AlreadyAttested(peer) => {
                let msg = format!(
                    "handle already attested by another device (pubkey {})",
                    voca::encode(BigUint::from_bytes_be(peer.device_pubkey.as_bytes()))
                );
                crate::log_at(
                    crate::LogLevel::Error,
                    &format!("attestation rejected: {msg}"),
                );
                // AlreadyAttested is now sent ONLY on a CHAIN-PROVEN takeover: the worker fold-verified a fleet chain whose genesis identity is not ours (handle_query.rs verdict). This is the genuine takeover case, so clearing the contested roots is correct — an indeterminate result (fold/parse/transport error) arrives as QueryResult::Error below, which does NOT clear the session. Clear so the next launch can't auto-resume into the same rejection, and bail to the attest screen (even from an optimistic Ready).
                tohu::clear_session();
                self.session = None;
                self.state = AppState::Launch(LaunchState::Error(msg));
                self.refocus_handle_select_all();
            }
            QueryResult::Locked => {
                // The worker refused this device's announce as fleet-locked (treat-as-stolen). Terminal brick: clear the session so nothing auto-resumes, drop to the dead-end Locked screen, and DON'T bind or persist anything — because the worker is the authority, this same verdict returns on every attest even after a local wipe, until the fleet owner unlocks the device.
                crate::log_at(
                    crate::LogLevel::Error,
                    "attestation refused: this device is fleet-locked",
                );
                tohu::clear_session();
                self.session = None;
                self.private_s = crate::crypto::blind::PrivateS::None;
                self.pending_broadcast_signal = -1;
                self.state = AppState::Launch(LaunchState::Locked);
                // A locked device must not keep the handle in its field: the retained text would auto-populate the attest screen the retry pill returns to.
                self.clear_handle_for_reproof();
            }
            QueryResult::Error(e) => {
                crate::log_at(crate::LogLevel::Error, &format!("attestation error: {e}"));
                if in_app {
                    // Transient network failure on a resume refresh — the local session is still valid. Stay on Ready; the next presence cycle retries. Do NOT drop the user back to the attest screen.
                    crate::log("UI: background refresh failed (network); staying on local session");
                } else if e.contains("not in the fleet") && !self.launch_add_mode {
                    // Safety net for a probe→announce race (device removed from the fleet in the gap): the announce says "not in the fleet". The probe normally routes this to add-this-device UP FRONT, but if it slips thru, catch it here too.
                    self.launch_add_mode = true;
                    self.state = AppState::Launch(LaunchState::Fresh);
                    self.add_join_handle = None;
                    self.submit_join_step(None);
                } else {
                    self.state = AppState::Launch(LaunchState::Error(e));
                    self.refocus_handle_select_all();
                }
            }
        }
    }

    /// On an attestation error, return the user to an editable handle field with the whole handle selected. The submit path dropped focus into the frozen Attesting state; coming back to `Error` (which `can_edit_handle()` allows) we refocus the textbox and select-all so the most common fix — the handle is claimed, retype a different one — is one keystroke: the first character typed replaces the selection. On Android, `change_focus` into a textbox also re-raises the soft keyboard via the pending-keyboard signal.
    /// Wipe + refocus the handle field — for every teardown whose POINT is re-proving the handle (the lock/unlock arms, the Security self-lock, a Locked verdict, a de-attest). The convenience retention elsewhere (attest errors keep the text selected for a quick retype) is exactly wrong here: a retained handle turns "re-prove the owner" into "press the button" (field report 2026-08-15 — the unlock confirmation auto-populated).
    pub(super) fn clear_handle_for_reproof(&mut self) {
        if let Some(tb) = self.textbox.as_mut() {
            tb.clear();
        }
        self.refocus_handle_select_all();
    }

    pub(super) fn refocus_handle_select_all(&mut self) {
        let Some(id) = self.textbox.as_ref().map(|t| t.hit_id()) else {
            return;
        };
        self.change_focus(Some(id));
        if let Some(tb) = self.textbox.as_mut() {
            tb.select_all();
        }
    }

    /// Refocus the contacts textbox and select all text — used on search failure so the user can immediately retype.
    pub(super) fn refocus_contacts_select_all(&mut self) {
        let Some(id) = self.contacts_textbox.as_ref().map(|t| t.hit_id()) else {
            return;
        };
        self.change_focus(Some(id));
        if let Some(tb) = self.contacts_textbox.as_mut() {
            tb.select_all();
        }
    }

    /// Handle a [`SearchResult`] from `HandleQuery::search`. On `Found`, build a `Contact` from the peer and append to `self.contacts` (skip if a contact with the same handle already exists; should be rare given `submit_add_friend` pre-checks, but the search races against attestation worker's contact load). Ends the in-flight hourglass and sets the result text shown below the search box: green "added {h}", red "not found" / "error: …".
    pub(super) fn on_search_result(&mut self, result: crate::ui::state::SearchResult) {
        use crate::ui::state::SearchResult;
        // Search resolved — drop the hourglass regardless of which branch we take below.
        self.add_in_flight = false;
        match result {
            SearchResult::Found(peer) => {
                // peer.handle is the user's TYPED search input riding along locally — the first-met seam. Dedup by the party id it derives, never by a stored string.
                let handle = peer.handle.as_str().to_string();
                let typed_pid = crate::crypto::clutch::identity_party_id(
                    &crate::types::Handle::to_identity_seed(&handle),
                );
                let already = self.contacts.iter().any(|c| c.handle_hash == typed_pid);
                if already {
                    crate::logf!(
                        "search-result: '{}' already in contacts — skipping add",
                        handle
                    );
                    self.search_status = Some((
                        format!("{handle} already added"),
                        (*theme::SEARCH_FOUND_COLOUR),
                    ));
                    return;
                }
                let mut contact = crate::types::Contact::new(
                    peer.handle.clone(),
                    peer.handle_proof,
                    peer.device_pubkey.clone(),
                )
                .with_ip(peer.ip)
                .with_local_ip(peer.local_ip, peer.ip.port());
                // §4.2 one-ceremony claim: the ADDING device owns this friendship's CLUTCH. The claim rides the roster entry this add is about to push, so siblings park instead of racing their own rounds at the friend.
                contact.ceremony_owner =
                    self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes());
                // Self-contact: same identity, no key exchange needed.
                let is_self = self
                    .session
                    .as_ref()
                    .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
                    == Some(contact.handle_hash);
                if is_self {
                    contact.clutch_state = crate::types::ClutchState::Complete;
                }
                let contact_id = contact.id.clone();
                let their_handle_hash = contact.handle_hash;
                let their_handle_proof = contact.handle_proof;
                // Mark keygen in flight BEFORE spawning (race guard) for non-self contacts.
                if !is_self {
                    contact.clutch_keygen_in_progress = true;
                }
                crate::logf!(
                    "search-result: added contact '{}' (total: {})",
                    crate::fp(&contact.handle_proof).as_str(),
                    self.contacts.len() + 1
                );
                self.contacts.push(contact);
                // Register the new contact (and its fleet, once refreshed) so the checker answers pings/offers from any of its devices, and kick CLUTCH keypair generation so the contact becomes offer-ready when it comes online.
                self.reseed_contact_pubkeys();
                self.spawn_contact_fleet_refresh(vec![their_handle_proof]);
                if !is_self {
                    let our_handle_hash = self
                        .session
                        .as_ref()
                        .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
                        .unwrap_or([0u8; 32]);
                    self.spawn_clutch_keygen(contact_id, our_handle_hash, their_handle_hash);
                }
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
                // Propagate the new friend to the rest of the fleet's devices.
                self.spawn_roster_push();
            }
            SearchResult::NotFound => {
                crate::log("search-result: handle not found on FGTW");
                self.search_status = Some(("not found".to_string(), (*theme::SEARCH_FAIL_COLOUR)));
                self.refocus_contacts_select_all();
            }
            SearchResult::Error(e) => {
                crate::logf!("search-result: error '{}'", e);
                self.search_status = Some((format!("error: {e}"), (*theme::SEARCH_FAIL_COLOUR)));
                self.refocus_contacts_select_all();
            }
        }
    }
}
