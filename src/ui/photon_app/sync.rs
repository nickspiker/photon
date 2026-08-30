//! Steady-state sync drivers — presence pings, CLUTCH/message retransmits, chain replication to siblings, history recovery, blind ops, and S generation.

use super::*;

impl PhotonApp {
    /// Boot the open contact — THE first roster-tombstone writer (the receive side has honoured tombstones since the roster CRDT shipped; nothing ever minted one until now). Ostracism, not erasure: WE drop the contact + chains locally and push a sticky tombstone so every device of OUR fleet drops it too — the other side is never signalled and keeps its own records (device-sovereignty doctrine). A tombstone outranks any concurrent re-add by LWW stamp, and re-adding later mints a fresh entry with a newer stamp, so boot→re-add works.
    pub(super) fn boot_active_contact(&mut self) {
        let Some(ci) = self.active_contact() else {
            return;
        };
        if self.contacts[ci].is_sibling {
            return; // device removal is chain consent (self-departure), never a contact boot
        }
        // Sticky tombstone into the fleet roster slot — off-thread, same shape as every roster push. push_roster pull-merges, so the tombstone joins the slot without clobbering concurrent sibling writes.
        if let (Some(hp), Some(kp), Some(fleet_key)) = (
            self.our_handle_proof(),
            self.device_keypair.clone(),
            self.fleet_key_cached(),
        ) {
            let c = &self.contacts[ci];
            let entry = crate::network::fgtw::fleet::RosterEntry {
                handle_proof: c.handle_proof,
                handle_hash: c.handle_hash,
                public_identity: *c.public_identity.as_bytes(),
                published_name: String::new(),
                avatar_pin: [0u8; 64],
                added: 0,
                updated: vsf::eagle_time_oscillations(),
                tombstone: true,
                ceremony_owner: [0u8; 32],
                woven: false,
                // A tombstone carries no identity payload — name, pin and owner are already blanked above for the same reason. Stranger (0) is the least-privileged value, so a tombstone that somehow lost its flag downgrades rather than promotes.
                trust_level: 0,
            };
            let live = self
                .fleet_settings
                .as_ref()
                .map(|fs| (fs.global.clone(), fs.devices.clone()));
            std::thread::spawn(move || {
                match crate::network::fgtw::fleet::push_roster_with_settings(
                    &hp,
                    &kp,
                    &fleet_key,
                    &[entry],
                    live,
                ) {
                    Ok(()) => crate::log("BOOT: roster tombstone pushed — every fleet device drops the contact"),
                    Err(e) => crate::logf!("BOOT: tombstone push failed ({}); local removal stands, the tombstone rides the next roster push", e),
                }
            });
        }
        // Local removal, mirroring the tombstone-receive path, plus chain cleanup.
        let gone = self.contacts.remove(ci);
        if let Some(storage) = self.storage.as_ref() {
            if let Err(e) = crate::storage::contacts::delete_contact(&gone.handle_hash, storage) {
                crate::logf!("BOOT: contact state delete failed: {}", e);
            }
            if let Some(fid) = gone.friendship_id {
                self.friendship_chains.retain(|(id, _)| *id != fid);
                if let Err(e) = crate::storage::friendship::delete_friendship_chains(&fid, storage)
                {
                    crate::logf!("BOOT: chain delete failed: {}", e);
                }
            }
        }
        crate::logf!(
            "BOOT: contact {} removed (ostracism, not erasure — their side keeps its own records)",
            crate::fp(&gone.handle_proof).as_str()
        );
        // Rewrite the contact index too (same as the tombstone-receive path) — or the next launch resurrects the row from the list until the next roster pull re-tombstones it.
        if let Some(storage) = self.storage.as_ref() {
            let index: Vec<crate::storage::contacts::ContactIdentity> = self
                .contacts
                .iter()
                .filter(|c| !c.is_sibling)
                .map(|c| crate::storage::contacts::ContactIdentity {
                    handle_proof: c.handle_proof,
                    party_id: c.handle_hash,
                    avatar_pin: c.avatar_pin,
                })
                .collect();
            if let Err(e) = crate::storage::contacts::save_contact_list(&index, storage) {
                crate::logf!("BOOT: index rewrite failed: {}", e);
            }
        }
        self.active_conversation = None;
        self.reseed_contact_pubkeys();
        self.update_sync_records();
        self.state = AppState::Ready;
    }

    /// A NEW own avatar just landed (picked, or recovered after a wipe): push it onto every zero-remote row NOW. Edge-driven — `settle_self_display` only fills an EMPTY slot, deliberately, because noticing staleness there would mean comparing full pixel buffers every pass; the install edge is the one place that KNOWS the picture changed (live 2026-08-02: a new avatar picked, notes-to-self kept the old face).
    pub(super) fn refresh_self_row_avatar(&mut self) {
        let Some(our_pid) = self
            .session
            .as_ref()
            .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
        else {
            return;
        };
        let ours = self.device_avatar_pixels.clone();
        for c in self
            .contacts
            .iter_mut()
            .filter(|c| !c.is_sibling && c.remote_count(&our_pid) == 0)
        {
            c.avatar_pixels = ours.clone();
            c.avatar_scaled = None;
            c.avatar_scaled_diameter = 0;
        }
    }

    /// A notes-to-self row displays OUR OWN name and avatar, because there is no peer to pong them: `published_name` and `avatar_pin` are populated by a friend answering our ping, and we never ping ourselves. Left alone the row reads "Pending…" with a placeholder picture forever — the identity you are logged in as, rendered as a stranger.
    pub(super) fn settle_self_display(&mut self) {
        let (Some(our_pid), Some(name)) = (
            self.session
                .as_ref()
                .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)),
            self.fleet_settings
                .as_ref()
                .and_then(|fs| fs.effective("profile.name"))
                .and_then(crate::storage::fleet_settings::as_text),
        ) else {
            return;
        };
        if name.is_empty() {
            return;
        }
        let ours = self.device_avatar_pixels.clone();
        for c in self
            .contacts
            .iter_mut()
            .filter(|c| !c.is_sibling && c.remote_count(&our_pid) == 0)
        {
            if c.published_name != name {
                c.published_name = name.clone();
            }
            // Our own avatar pixels, already decoded for the profile header — no fetch, no pin, no slot: it is the same image one struct field away.
            if c.avatar_pixels.is_none() {
                if let Some(px) = ours.clone() {
                    c.avatar_pixels = Some(px);
                    c.avatar_scaled = None;
                }
            }
        }
    }

    /// A conversation with zero remote participants can hold NO ceremony — nothing to exchange, nobody to offer at. Yet the field found the notes row Pending with a sent offer (the ContactId collision routed a sibling's keygen onto it, 2026-08-13), which rings the parked-ceremony doorbell every ~5min and re-arms 573KB offers at our own fleet on every path-up edge. Scrub round debris off zero-remote rows and settle them Complete (zero-remote is keyed-on-arrival by doctrine).
    pub(super) fn scrub_zero_remote_rounds(&mut self) {
        for ci in 0..self.contacts.len() {
            let c = &self.contacts[ci];
            if !self.is_zero_remote(c) {
                continue;
            }
            let has_round = c.clutch_state != crate::types::ClutchState::Complete
                || c.clutch_our_keypairs.is_some()
                || !c.clutch_slots.is_empty()
                || c.ceremony_id.is_some()
                || c.clutch_offer_sent
                || c.clutch_round_started.is_some();
            if !has_round {
                continue;
            }
            crate::logf!(
                "CLUTCH: scrubbed a parked round off zero-remote conversation {} — settled Complete",
                crate::fp(&self.contacts[ci].handle_proof).as_str()
            );
            let c = &mut self.contacts[ci];
            c.discard_clutch_round();
            c.clutch_state = crate::types::ClutchState::Complete;
            if let Some(storage) = self.storage.as_ref() {
                let _ = crate::storage::contacts::save_contact(&self.contacts[ci], storage);
            }
        }
    }

    /// Collapse the ACTIVE contact's presence backoff — called when its conversation opens. Looking at someone is the clearest possible signal that their presence matters now, and it is the escape hatch that makes an hour-long backoff safe to have at all.
    pub(super) fn reset_contact_ping_backoff(&mut self) {
        if let Some(ci) = self.active_contact() {
            if let Some(c) = self.contacts.get_mut(ci) {
                c.ping_backoff = 0;
                c.last_pinged = None; // due immediately, so the ring is fresh by the time it renders
            }
        }
    }

    /// Current presence-sweep interval, chosen by how long since the user last interacted. Active (5s) while engaged → idle (1min) → deep-idle (15min). `now` is the tick's clock. Jittered to 50–100% of the tier so a roomful of devices doesn't ping their contacts in lockstep (a synchronised presence sweep is a self-inflicted DDoS). Presence timing is soft, so the fuzziness is free.
    pub(super) fn presence_ping_interval(&self, now: Instant) -> std::time::Duration {
        let idle = self
            .last_interaction
            .map_or(std::time::Duration::ZERO, |last| now.duration_since(last));
        let mut tier = if idle < PRESENCE_IDLE_NEAR {
            PRESENCE_PING_ACTIVE
        } else if idle < PRESENCE_IDLE_FAR {
            PRESENCE_PING_IDLE
        } else {
            PRESENCE_PING_DEEP
        };
        // A held direct path is kept open only by traffic on it, and the presence sweep IS that keepalive — so while any validated path exists, don't let the idle/deep taper starve it below the NAT-safe interval, or the mapping dies mid-session.
        if tier > VALIDATED_PATH_KEEPALIVE
            && self.contacts.iter().any(|c| c.validated_path.is_some())
        {
            tier = VALIDATED_PATH_KEEPALIVE;
        }
        crate::jitter_dur(tier)
    }

    /// Ping all contacts that have IP addresses (call periodically)
    pub(super) fn ping_contacts(&mut self) {
        use crate::network::traverse::session::PATH_TTL;
        use crate::types::contact::PUNCH_UNREACHABLE_THRESHOLD;

        // Cycles a Pending ceremony may sit offer-sent with a validated path up and no peer offer before we re-fire ours (see Contact::clutch_offer_stall_cycles).
        const OFFER_STALL_CYCLES: u8 = 6;

        // Expire stale validated paths (no keepalive ack within TTL → the NAT mapping is likely dead): clear so `race_addrs` falls back to LAN/public and this cycle re-punches. Track the symmetric↔symmetric case: an online contact we keep punching but never validate is direct-unreachable — bump the graceful-failure counter (the hook M2's relay reads) and log the state once at the threshold.
        let mut stalled_offers: Vec<usize> = Vec::new();
        let mut dozed_rings: Vec<usize> = Vec::new();
        let our_device = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes());
        let siblings = sibling_presence_snapshot(&self.contacts);
        for (i, c) in self.contacts.iter_mut().enumerate() {
            // LOCKOUT: no pings, no punches, no stall re-fires, no doorbell rings toward a locked device — the fleet stopped talking to it, both directions.
            if c.locked_out {
                continue;
            }
            if let Some((_, at)) = c.validated_path {
                if at.elapsed() >= PATH_TTL {
                    c.validated_path = None;
                }
            }
            if c.is_online && c.validated_path.is_none() {
                c.punch_unvalidated_cycles = c.punch_unvalidated_cycles.saturating_add(1);
                if c.punch_unvalidated_cycles == PUNCH_UNREACHABLE_THRESHOLD {
                    crate::logf!("TRAVERSE: {} online but no direct path after {} cycles — pending relay (M2)", crate::fp(&c.handle_proof).as_str(), PUNCH_UNREACHABLE_THRESHOLD);
                }
            }
            // Parked-ceremony safety net: our offer went out, a direct path is PROVEN up, and the peer's offer still hasn't arrived — so ours (or theirs) died in transit and nothing pong-driven will ever retry it. Re-fire ours every few cycles until the exchange moves; bounded to one half-MB transfer per threshold-crossing, self-terminating the moment their offer lands.
            // Reachable by ANY path counts — not just a validated direct one. A relay-online peer with a lost offer had NO re-send edge at all: the stall arm required a validated path, the dozed doorbell requires 90s of SILENCE (relay pongs keep last_heard fresh forever), and the one 573KB send was "delivered" into a dead pipe socket while the recipient slept through a network change (field 2026-08-13: the sibling pair deadlocked cross-round — one side at proof of round A, the other awaiting a KEM to round B the peer never saw). Siblings are included: their re-fire is gated by the same no-peer-offer slot check, so only the side whose answer is genuinely missing re-sends, and ceremony_parked_by never parks a sibling by design.
            let stalled = c.clutch_state == crate::types::ClutchState::Pending
                && c.clutch_offer_sent
                && (c.validated_path.is_some() || c.is_online)
                && c.get_slot(&c.handle_hash)
                    .map_or(true, |s| s.offer.is_none())
                && !ceremony_parked_by(c, our_device, &siblings);
            if stalled {
                c.clutch_offer_stall_cycles = c.clutch_offer_stall_cycles.saturating_add(1);
                if c.clutch_offer_stall_cycles >= OFFER_STALL_CYCLES {
                    c.clutch_offer_stall_cycles = 0;
                    stalled_offers.push(i);
                }
            } else {
                c.clutch_offer_stall_cycles = 0;
            }
            // The DOZED flavour of a parked ceremony: offer sent, NO validated path, and total silence past the dozed threshold — their process probably isn't scheduled at all (phone in a pocket), so no amount of re-sending lands. Ring the doorbell; the woken phone re-punches, traffic flows, the ceremony drivers take it from there. Same double debounce as the chat ring.
            if c.clutch_state == crate::types::ClutchState::Pending
                && c.clutch_offer_sent
                && c.validated_path.is_none()
                && c.last_heard
                    .map_or(true, |t| t.elapsed() >= std::time::Duration::from_secs(90))
                && c.last_ring
                    .map_or(true, |t| t.elapsed() >= std::time::Duration::from_secs(300))
                && !ceremony_parked_by(c, our_device, &siblings)
            {
                c.last_ring = Some(std::time::Instant::now());
                dozed_rings.push(i);
            }
        }
        if !dozed_rings.is_empty() {
            if let Some(secret) = self.device_keypair.as_ref().map(|kp| *kp.secret.as_bytes()) {
                for i in dozed_rings {
                    crate::logf!(
                        "DOORBELL: {} ceremony parked with no path and no traffic — ringing",
                        crate::fp(&self.contacts[i].handle_proof)
                    );
                    crate::network::doorbell::spawn_ring(secret, self.contacts[i].handle_proof);
                }
            }
        }
        for i in stalled_offers {
            crate::logf!("CLUTCH: {} still has no offer from the peer after {} reachable ping cycles — re-firing ours", crate::fp(&self.contacts[i].handle_proof), OFFER_STALL_CYCLES);
            self.contacts[i].clutch_offer_sent = false;
            self.resend_clutch_offer(i);
        }

        let Some(checker) = self.status_checker.as_ref() else {
            return;
        };
        let mut pinged = 0;
        let now = std::time::Instant::now();
        let mut due: Vec<usize> = Vec::new();
        for (i, contact) in self.contacts.iter().enumerate() {
            // Presence is a question about OTHER people. A conversation with no remote participants has nobody to ask, so it is not skipped by a self-check — there is simply no one in the loop. (Pinging it used to storm our own addresses: probe spam at ourselves, wrong-responder pongs, mobile radio burnt for nothing.)
            let has_remote = self
                .our_party_id(contact)
                .is_some_and(|us| contact.remote_count(&us) > 0);
            if has_remote && contact_ping_due(contact, now) {
                due.push(i);
            }
        }
        // Stamp before sending: a contact is "pinged this round" whether or not the send succeeds, or an unreachable address would be retried at the floor rate forever.
        for &i in &due {
            if let Some(c) = self.contacts.get_mut(i) {
                c.last_pinged = Some(now);
                c.ping_backoff = c.ping_backoff.saturating_add(1).min(PING_BACKOFF_MAX);
            }
        }
        for contact in due.iter().filter_map(|&i| self.contacts.get(i)) {
            // Ping the LAN address AND the public address (when both are known) rather than preferring LAN and never falling back. Two devices that once shared a LAN have a stored `local_ip`; the moment one moves to a different network (e.g. phone → cellular) that LAN address is stale and unreachable, but the public address in the registry is correct — pinging only LAN strands them offline forever. Each ping is tracked by a unique provenance hash and a single pong clears the whole per-contact failure counter (see status.rs StatusPong handler), so the unreachable address simply times out harmlessly while the reachable one keeps the contact online. On-LAN the LAN ping wins (no router hairpin / AP isolation); off-LAN the public ping wins.
            let lan_addr = match (contact.local_ip, contact.local_port) {
                (Some(ip), Some(port)) => {
                    Some(std::net::SocketAddr::new(std::net::IpAddr::V4(ip), port))
                }
                _ => None,
            };
            // Punch candidates, fired alongside the first ping (stale paths were cleared above):
            // - validated → keepalive: probe just the validated remote to keep its NAT mapping warm; its ack refreshes liveness so the path never expires while the contact stays reachable.
            // - unvalidated → (re)punch: probe all the peer's addresses, best-first, so the first to round-trip wins.
            let mut punch: Vec<std::net::SocketAddr> = match contact.validated_path {
                Some((remote, _)) => vec![remote],
                None => crate::network::traverse::gather::gather_peer_candidates(contact)
                    .sorted()
                    .into_iter()
                    .map(|c| c.addr)
                    .collect(),
            };
            // Belt-and-suspenders: the peer's public address MUST be probed even if the candidate gather missed it (two-device fleet keying, a stale endpoint set). For a remote peer behind NAT — reached via 464XLAT when we're IPv6-only — this WAN address is frequently the ONLY viable path, and a bug that dropped it stranded the whole ceremony (the IPv6-only side never sent one packet to the other's real public v4). Log the exact probe set so a stuck ceremony is diagnosable at a glance.
            if contact.validated_path.is_none() {
                if let Some(ip) = contact.ip {
                    if !punch.contains(&ip) {
                        punch.push(ip);
                    }
                }
                // No bogus candidate ever reaches the wire: the belt-and-suspenders ip push above has no filter of its own, and a poisoned-era contact.ip put 0.0.0.0:4383 in a live probe set (field, 2026-08-13).
                punch.retain(|a| !crate::network::traverse::gather::is_bogus_addr(a));
                if !punch.is_empty() {
                    let set = punch
                        .iter()
                        .map(|a| a.to_string())
                        .collect::<Vec<_>>()
                        .join(" , ");
                    crate::logf!(
                        "PUNCH: {} probing {}",
                        crate::fp(&contact.handle_proof),
                        set
                    );
                }
            }
            let mut sent = false;
            // No direct path proven → also ping over the relay pipe so PRESENCE works for a relay-only peer. Taken once per cycle so we don't relay the same ping three times (once per candidate address). A validated path means direct pings suffice — no relay ping needed.
            let mut relay_ping =
                relay_unless_direct_trusted(&contact, crate::network::udp::get_local_ip());
            // The punch-validated path is the one address PROVEN reachable — when it matches neither stored record (a reflexive-learned mapping can differ from both the registry ip and the LAN row), ping it too, or presence sits TIMEOUT on two dead addresses while the keepalive acks flow.
            if let Some((vpath, _)) = contact.validated_path {
                if Some(vpath) != lan_addr && Some(vpath) != contact.ip {
                    checker.ping(
                        vpath,
                        contact.public_identity.clone(),
                        std::mem::take(&mut punch),
                        std::mem::take(&mut relay_ping),
                    );
                    sent = true;
                }
            }
            if let Some(addr) = lan_addr {
                checker.ping(
                    addr,
                    contact.public_identity.clone(),
                    std::mem::take(&mut punch),
                    std::mem::take(&mut relay_ping),
                );
                sent = true;
            }
            // Public address — skip only if it's identical to the LAN address we already pinged.
            if let Some(public) = contact.ip {
                if Some(public) != lan_addr {
                    checker.ping(
                        public,
                        contact.public_identity.clone(),
                        std::mem::take(&mut punch),
                        std::mem::take(&mut relay_ping),
                    );
                    sent = true;
                }
            }
            // If neither address fired (relay-only peer with no stored endpoint at all), still send the presence ping over the relay so the pipe keepalive keeps them yellow. peer_addr is a sentinel the send drain will UDP-send to harmlessly; the relay_to fan-out is what matters.
            if !relay_ping.is_empty() {
                checker.ping(
                    crate::network::status::RELAY_ADDR,
                    contact.public_identity.clone(),
                    Vec::new(),
                    std::mem::take(&mut relay_ping),
                );
                sent = true;
            }
            if sent {
                pinged += 1;
            }
            // PER-DEVICE presence: every OTHER fleet device with a discovered endpoint gets its own ping at ITS OWN address(es), tracked by ITS pubkey — so each device answers for itself and the identity ring is "any device up". (The contact-level pings above already cover the active/first-met device; skip its endpoint to avoid a doubled ping.)
            for ep in &contact.device_endpoints {
                if Some(ep.pubkey) == contact.active_device
                    || ep.pubkey == contact.public_identity.key
                {
                    continue;
                }
                let dev = crate::types::DevicePubkey::from_bytes(ep.pubkey);
                for addr in [ep.lan, ep.public].into_iter().flatten() {
                    checker.ping(addr, dev.clone(), Vec::new(), Vec::new());
                }
            }
        }
        if pinged > 0 {
            crate::logf!("Status: pinged {} contact(s)", pinged);
        }
        // LAN broadcast for same-network local-IP discovery (hairpin-NAT workaround).
        if let (Some(session), Some(hq)) = (self.session.as_ref(), self.handle_query.as_ref()) {
            checker.send_lan_broadcast(session.handle_proof, hq.port());
        }

        // Bell publish (Android only — desktops don't doze, so they publish nothing and are never rung): once Kotlin has handed over the FCM token, publish `fcm:<project>:<token>` under OUR handle_proof, and re-publish whenever the token rotates. Piggybacks the ping cadence so a late token or a rotation heals without dedicated machinery.
        #[cfg(target_os = "android")]
        if let Some((project, token)) = crate::platform::jni_android::fcm_bell() {
            let bell = format!("fcm:{}:{}", project, token);
            if self.published_bell.as_deref() != Some(bell.as_str()) {
                if let (Some(kp), Some(session)) =
                    (self.device_keypair.as_ref(), self.session.as_ref())
                {
                    crate::network::doorbell::spawn_publish_bells(
                        *kp.secret.as_bytes(),
                        session.handle_proof,
                        vec![bell.clone()],
                    );
                    self.published_bell = Some(bell);
                }
            }
        }

        // Recovery for a side stranded in AwaitingProof: while the peer is ONLINE and we still hold our computed proof, keep the resend budget topped up so we keep re-sending our proof every few cycles. The peer — already Complete — now treats our repeated proof as an implicit re-request and re-sends its ClutchComplete (see the Complete-state duplicate handler). So a ClutchComplete dropped during the original ceremony (e.g. before the v4-mapped-v6 send fix, or any single UDP loss) self-heals once both sides are online, instead of leaving us AwaitingProof forever with the peer already Complete. Bounded per-cycle so an offline peer doesn't spin; it only tops up when we actually have the peer online with a proof to send.
        // LIFETIME CAP: the re-arm was unbounded — a peer that answers but can never PLACE our proof (token mismatch: a stale-identity ghost, an interrupted re-genesis) kept us re-sending every cycle forever, each a relay request (the two-era ghost storm, ~13s cadence). Past the cap we latch `clutch_proof_gave_up` and stop: no more re-arms, no more relay spew. A fresh session re-tries once (retry_lifetime is runtime-only) in case the peer re-attested correctly.
        const PROOF_RETRY_LIFETIME_CAP: u16 = 40; // ~40 recovery cycles (minutes of online-together) before declaring the peer unable to place our proof
        for contact in self.contacts.iter_mut() {
            if contact.is_online
                && contact.clutch_state == crate::types::ClutchState::AwaitingProof
                && contact.clutch_our_eggs_proof.is_some()
                && contact.ceremony_id.is_some()
                && contact.clutch_proof_resends_left == 0
                && !contact.clutch_proof_gave_up
            {
                if contact.clutch_proof_retry_lifetime >= PROOF_RETRY_LIFETIME_CAP {
                    contact.clutch_proof_gave_up = true;
                    contact.clutch_our_eggs_proof = None; // stop holding it; nothing more to send
                    crate::logf!("CLUTCH: giving up proof retransmit to {} — peer answers but can't place our proof (stale identity / re-genesis?); remove & re-add to fix", crate::fp(&contact.handle_proof).as_str());
                } else {
                    contact.clutch_proof_retry_lifetime += 1;
                    contact.clutch_proof_resends_left = 1; // one re-send this cycle; re-armed next ping while still stuck (until the lifetime cap)
                }
            }
        }

        // PROACTIVE zombie-round expiry: AwaitingProof with NOTHING left to send — the proof was destroyed (give-up latch), drained, or lost at resume. The offer-arrival exit ramp can't save this shape when the peer has MOVED ON (completed its side and stopped offering — observed live 2026-07-24: one side Complete and silent, the other AwaitingProof holding fresh idle keys that no send path fires because every offer-send gates on Pending). While the peer is online, a provably-unrecoverable round (gave up outright, or empty-handed AND stale) discards to Pending; the keygen queue then mints a fresh round and our new offer goes out — a Complete peer accepts it as a re-key, an in-flight peer adopts it wholesale. Staleness guards the normal post-completion window where the proof budget has drained but the peer's proof is seconds away; the fresh round_started restamp is the natural rate limit.
        {
            const ZOMBIE_ROUND_STALE_OSC: i64 = 600 * vsf::OSCILLATIONS_PER_SECOND as i64;
            let now_osc = vsf::eagle_time_oscillations();
            let mut expired: Vec<usize> = Vec::new();
            for (i, contact) in self.contacts.iter().enumerate() {
                if !contact.is_online
                    || contact.clutch_state != crate::types::ClutchState::AwaitingProof
                {
                    continue;
                }
                if ceremony_parked_by(contact, our_device, &siblings) {
                    continue;
                }
                let empty_handed = contact.clutch_our_eggs_proof.is_none()
                    && contact.clutch_proof_resends_left == 0;
                let stale = contact
                    .clutch_round_started
                    .map_or(true, |t| now_osc.saturating_sub(t) > ZOMBIE_ROUND_STALE_OSC);
                if contact.clutch_proof_gave_up || (empty_handed && stale) {
                    expired.push(i);
                }
            }
            for i in expired {
                crate::logf!("CLUTCH: {} zombie round expired — AwaitingProof with nothing left to send and the peer silent; discarding for a fresh ceremony", crate::fp(&self.contacts[i].handle_proof));
                let c = &mut self.contacts[i];
                c.discard_clutch_round();
                c.clutch_proof_retry_lifetime = 0;
                c.clutch_proof_gave_up = false;
                if let Some(storage) = self.storage.as_ref() {
                    let _ = crate::storage::contacts::save_contact(&self.contacts[i], storage);
                }
            }
        }

        // Retransmit the ClutchComplete proof for any contact with budget left. The proof is a lone unreliable UDP packet, so a single drop (or a send to a since-refreshed address) would strand the peer in AwaitingProof. Re-sending it for a few ping cycles converges both sides regardless of which completed first or which packet was lost. Self-terminates as the budget drains; a peer already Complete re-arms its own resend on the duplicate.
        self.retransmit_pending_clutch_proofs();
    }

    /// Re-send the ClutchComplete proof to every contact whose retransmit budget (`clutch_proof_resends_left`) is non-zero, decrementing each. See [`ping_contacts`] for why this exists. Clears our held proof once the budget reaches zero so it isn't kept forever.
    pub(super) fn retransmit_pending_clutch_proofs(&mut self) {
        use crate::crypto::clutch::{derive_conversation_token, ClutchCompletePayload};
        use crate::network::status::ClutchCompleteRequest;

        // PARTY ID (not raw seed): the last un-migrated seam site. First-completion sends derived the token from party ids; this retransmit path used the raw identity seed, so every friend-proof RETRANSMIT rode a token no receiver recognizes ("unknown conversation_token" forever at the peer) — a proof lost once could never land, and stalls that needed a proof retransmit never converged (live 2026-07-24: proofs for round 933db663 arriving under acbaf3c9 instead of 8586b07e).
        let Some(our_handle_hash) = self
            .session
            .as_ref()
            .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
        else {
            return;
        };
        let Some(kp) = self.device_keypair.as_ref() else {
            return;
        };
        let device_pubkey = *kp.public.as_bytes();
        let device_secret = *kp.secret.as_bytes();
        let Some(checker) = self.status_checker.as_ref() else {
            return;
        };

        for contact in self.contacts.iter_mut() {
            if contact.clutch_proof_resends_left == 0 {
                continue;
            }
            let (Some(eggs_proof), Some(ceremony_id)) =
                (contact.clutch_our_eggs_proof, contact.ceremony_id)
            else {
                // Nothing to resend (proof/ceremony cleared) — drop the budget.
                contact.clutch_proof_resends_left = 0;
                continue;
            };
            let Some((primary, alt)) = contact.race_addrs() else {
                continue;
            };
            // Party-id seam: sibling tokens derive from the device pids, not the shared seed.
            let our_pid = if contact.is_sibling {
                crate::crypto::clutch::sibling_party_id(&device_pubkey)
            } else {
                our_handle_hash
            };
            let conv_token = derive_conversation_token(&[our_pid, contact.handle_hash]);
            checker.send_complete_proof(ClutchCompleteRequest {
                peer_addr: primary,
                alt_addr: alt,
                conversation_token: conv_token,
                ceremony_id,
                payload: ClutchCompletePayload { eggs_proof },
                device_pubkey,
                device_secret,
                recipient_pubkey: contact.public_identity.key,
                relay_to: contact.relay_device_list(),
            });
            contact.clutch_proof_resends_left -= 1;
            crate::logf!(
                "CLUTCH: Retransmitted proof to {} ({} resends left)",
                crate::fp(&contact.handle_proof),
                contact.clutch_proof_resends_left
            );
            // Budget exhausted — stop holding the proof.
            if contact.clutch_proof_resends_left == 0
                && contact.clutch_state == crate::types::ClutchState::Complete
            {
                contact.clutch_our_eggs_proof = None;
            }
        }
    }

    /// Send the consent KNOCK at a contact we added (2026-08-25): the few-hundred-byte signed intent frame that replaced the 548KB offer as the opening move — no key material travels until the add is reciprocated. Direct + relay like every small ceremony frame; loss is covered by the once-per-session re-knock on presence edges and, ultimately, by the peer's own add (the later adder initiates).
    pub(super) fn send_friend_knock(&mut self, ci: usize) {
        let (token, primary, alt, recipient, relay) = {
            let Some(contact) = self.contacts.get(ci) else {
                return;
            };
            if contact.consent_mutual || contact.is_sibling || contact.knocked_session {
                return;
            }
            let Some(us) = self
                .session
                .as_ref()
                .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
            else {
                return;
            };
            let token =
                crate::crypto::clutch::derive_conversation_token(&[us, contact.handle_hash]);
            // No address is no reason to hold: with no validated path the send fans out over relay_to, same as the offer re-fire below.
            let primary = contact.ip.unwrap_or(crate::network::status::RELAY_ADDR);
            let alt = match (contact.local_ip, contact.local_port) {
                (Some(lan), Some(port)) => Some(std::net::SocketAddr::from((lan, port))),
                _ => None,
            };
            (token, primary, alt, contact.public_identity.key, contact.relay_device_list())
        };
        let Some(kp) = self.device_keypair.as_ref() else {
            return;
        };
        let device_pubkey = *kp.public.as_bytes();
        let device_secret = *kp.secret.as_bytes();
        match crate::network::fgtw::protocol::build_friend_knock_vsf(
            &token,
            &device_pubkey,
            &device_secret,
        ) {
            Ok(bytes) => {
                if let Some(checker) = self.status_checker.as_ref() {
                    checker.send_history(crate::network::status::HistorySendRequest {
                        peer_addr: primary,
                        alt_addr: alt,
                        recipient_pubkey: recipient,
                        vsf_bytes: bytes,
                        relay_to: relay,
                    });
                    if let Some(c) = self.contacts.get_mut(ci) {
                        c.knocked_session = true;
                        crate::logf!(
                            "CONSENT: knock sent to {} — ceremony waits for their add",
                            crate::fp(&c.handle_proof)
                        );
                    }
                }
            }
            Err(e) => crate::logf!("CONSENT: knock build failed: {}", e),
        }
    }

    /// Re-fire our full CLUTCH offer to `self.contacts[idx]`, outside the pong-driven send block. The normal driver re-sends only when a pong flips the contact online — useless when the peer's pongs don't flow (observed: presence sat TIMEOUT for twenty minutes while punch keepalives validated a perfectly good direct path, and the ceremony stayed parked in Pending because the offer's single PT transfer had died racing a dead carrier-NAT address). `race_addrs` routes the re-send over the validated path first; the receiver's ceremony-round scoping makes a crossed duplicate free. Callers reset `clutch_offer_sent` first — this sets it back on a successful hand-off to the checker.
    pub(super) fn resend_clutch_offer(&mut self, idx: usize) {
        use crate::network::fgtw::protocol::build_clutch_offer_vsf;
        use crate::network::status::ClutchOfferRequest;

        // §4.2: a parked ceremony never re-fires its offer — the owner drives; our re-send would hand the friend a competing instance.
        if self.ceremony_parked(&self.contacts[idx]) {
            crate::logf!(
                "CLUTCH: not re-sending offer to {} — ceremony is parked (owner drives)",
                crate::fp(&self.contacts[idx].handle_proof)
            );
            return;
        }
        // PARTY ID (not raw seed) + the sibling seam: the fresh-send paths derive the token from party ids, but this RE-SEND path used the raw identity seed — every re-fired offer (stall re-fire, queued-KEM recovery, addr-change re-arm) rode a token the peer can't place, which is exactly the "offers under an unknown token" storms in the field logs (the 0d9b7fc0 flood in the field logs was OUR re-sends, not a ghost device).
        let Some(our_handle_hash) = self
            .session
            .as_ref()
            .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
        else {
            return;
        };
        let our_handle_hash = if self.contacts[idx].is_sibling {
            match self.device_keypair.as_ref() {
                Some(kp) => crate::crypto::clutch::sibling_party_id(kp.public.as_bytes()),
                None => return,
            }
        } else {
            our_handle_hash
        };
        let Some((device_pubkey, device_secret)) = self
            .device_keypair
            .as_ref()
            .map(|kp| (*kp.public.as_bytes(), *kp.secret.as_bytes()))
        else {
            return;
        };
        let Some(checker) = self.status_checker.as_ref() else {
            return;
        };
        let contact = &mut self.contacts[idx];
        let Some(ref keypairs) = contact.clutch_our_keypairs else {
            // Keygen hasn't (re)filled the ephemerals — the serialized keygen worker will, and its own completion path sends the offer.
            return;
        };
        // No known address is NOT a reason to hold the offer: with no validated path the send already fans out over the relay (relay_to below), and the relay demonstrably carries traffic exactly when addressing is broken. This gate used to return here, which is why a contact whose relayed pongs proved it ALIVE still sat Pending forever — relayed pongs never set `contact.ip` (they carry the RELAY sentinel), so the offer waited on an address it never needed. The sentinel peer_addr is harmless to the send drain; the relay_to fan-out is the real delivery.
        let ip = contact.ip.unwrap_or(crate::network::status::RELAY_ADDR);

        let payload = crate::crypto::clutch::ClutchOfferPayload::from_keypairs(keypairs);
        let conversation_token = crate::crypto::clutch::derive_conversation_token(&[
            our_handle_hash,
            contact.handle_hash,
        ]);
        match build_clutch_offer_vsf(
            &conversation_token,
            &payload,
            &device_pubkey,
            &device_secret,
            contact
                .clutch_round_started
                .unwrap_or_else(vsf::eagle_time_oscillations),
        ) {
            Ok((vsf_bytes, our_offer_provenance)) => {
                crate::logf!(
                    "CLUTCH: re-sending full offer to {} (prov={}...)",
                    crate::fp(&contact.handle_proof),
                    hex::encode(&our_offer_provenance[..4])
                );
                if !contact.offer_provenances.contains(&our_offer_provenance) {
                    contact.offer_provenances.push(our_offer_provenance);
                }
                let (primary, alt) = contact.race_addrs().unwrap_or((ip, None));
                checker.send_offer(ClutchOfferRequest {
                    peer_addr: primary,
                    alt_addr: alt,
                    vsf_bytes,
                    recipient_pubkey: contact.public_identity.key,
                    relay_to: contact.relay_device_list(),
                });
                contact.clutch_offer_sent = true;
                if let Some(storage) = self.storage.as_ref() {
                    let c = &self.contacts[idx];
                    if let Err(e) = crate::storage::contacts::save_clutch_slots(
                        &c.clutch_slots,
                        &c.offer_provenances,
                        c.ceremony_id,
                        &c.handle_hash,
                        storage,
                    ) {
                        crate::logf!("Failed to persist CLUTCH provenance: {}", e);
                    }
                }
            }
            Err(e) => {
                crate::logf!(
                    "CLUTCH: Failed to build offer VSF for {}: {}",
                    crate::fp(&self.contacts[idx].handle_proof),
                    e
                );
            }
        }
    }

    /// Reliability sweep (every tick): resend any unacked outgoing message whose backoff deadline has passed, with exponential backoff, until an ACK clears it or it exhausts its attempts. This is the per-message retry the protocol was missing — without it, a single dropped message OR a single dropped ACK desyncs the chain permanently (the sender advances on ACK, so a lost ACK freezes its chain while the receiver's has moved on → every later message decrypts as garbage). Resending is safe: the receiver dedupes by eagle_time and its ACK is deterministic, so a redelivered message just yields a free re-ACK. Uses the same LAN-preferring `race_addrs()` as the live send.
    pub(super) fn retransmit_due_messages(&mut self) {
        let now_osc = vsf::eagle_time_oscillations();

        // Snapshot (friendship_id → primary + alt addr + recipient pubkey) from contacts so we don't hold a contacts borrow across the mutable chains sweep. Only Complete contacts with a known address. Carry BOTH addresses — a retransmit that only re-hit the primary would keep blackholing an off-LAN peer for the whole retry budget (observed: 8 attempts all to a dead LAN IPv4).
        // OUR own LAN v4 (if any) decides whether a peer's private-v4 address is a same-subnet fast path or a foreign black hole. Computed ONCE for the whole sweep — it's a syscall.
        let our_lan_v4 = crate::network::udp::get_local_ip();
        let routes: Vec<(crate::types::FriendshipId, std::net::SocketAddr, Option<std::net::SocketAddr>, [u8; 32], Vec<[u8; 32]>)> = self
            .contacts
            .iter()
            .filter_map(|c| {
                let fid = c.friendship_id?;
                let (mut primary, mut alt) = c.race_addrs()?;
                // Drop a FOREIGN peer LAN — a peer's private address on a subnet that isn't ours, which PT retransmits into a black hole forever. If the primary is foreign, promote a reachable alt into its place; if both are foreign, this route has NO direct target and survives ONLY on the relay fan-out below (relay_to). A same-subnet peer LAN is kept — that's a real fast path.
                use crate::network::traverse::gather::is_foreign_peer_lan;
                if is_foreign_peer_lan(&primary, our_lan_v4) {
                    match alt.take().filter(|a| !is_foreign_peer_lan(a, our_lan_v4)) {
                        Some(reachable) => primary = reachable,
                        None => {
                            // No reachable direct address at all. Keep the route only if the relay can carry it.
                            let relay_to = c.relay_device_list();
                            if relay_to.is_empty() {
                                crate::logf!("CHAT: {} retransmit has no reachable path (foreign LAN {}, no relay) — skipping", crate::fp(&c.handle_proof), primary);
                                return None;
                            }
                            // peer_addr is unused for delivery here (both direct addrs were foreign); hand the sentinel so the send drain UDP-sends nowhere harmlessly and the relay_to carries it.
                            return Some((fid, crate::network::status::RELAY_ADDR, None, *c.public_identity.as_bytes(), relay_to));
                        }
                    }
                } else if alt.map_or(false, |a| is_foreign_peer_lan(&a, our_lan_v4)) {
                    alt = None; // primary reachable, but drop a foreign alt so PT doesn't race a black hole
                }
                // No direct path → carry the peer's relay device list so the retransmit also rides the pipe.
                let relay_to = c.relay_device_list();
                Some((fid, primary, alt, *c.public_identity.as_bytes(), relay_to))
            })
            .collect();
        if routes.is_empty() {
            return;
        }

        let Some(checker) = self.status_checker.as_ref() else {
            return;
        };

        let mut undelivered_fids: Vec<crate::types::FriendshipId> = Vec::new();
        let mut gave_up_fids: Vec<crate::types::FriendshipId> = Vec::new();
        let mut gave_up_rows: Vec<(crate::types::FriendshipId, i64)> = Vec::new();
        for (fid, peer_addr, alt_addr, recipient_pubkey, relay_to) in routes {
            let Some((_, chains)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == fid)
            else {
                continue;
            };
            let conversation_token = chains.conversation_token;
            // Pendings exist only on the lane WE minted, so a retransmit always rides our label.
            let Some(lane) = chains.our_label().copied() else {
                continue;
            };
            let mut any_due = false;
            for (eagle_time, prev_msg_hp, ciphertext, attempts, exhausted) in
                chains.collect_due_retransmits(now_osc)
            {
                any_due = true;
                checker.send_message(crate::network::status::MessageRequest {
                    peer_addr,
                    alt_addr,
                    recipient_pubkey,
                    conversation_token,
                    lane,
                    prev_msg_hp,
                    ciphertext,
                    eagle_time,
                    relay_to: relay_to.clone(),
                });
                if exhausted {
                    crate::logf!("CHAT: retransmit GAVE UP on msg eagle_time {} after {} attempts (undelivered)", eagle_time, attempts);
                    gave_up_fids.push(fid);
                    // The give-up is a VERDICT the bridge must hear (field 2026-08-27): a wedged command from a dead session kept the prompt gate held on a "running" command whose 8 attempts had all failed — zombie in-flight forever, fresh commands gated behind ghosts. Collected here, stamped released after the loop.
                    gave_up_rows.push((fid, eagle_time));
                } else {
                    crate::logf!(
                        "CHAT: retransmit msg eagle_time {} (attempt {})",
                        eagle_time,
                        attempts
                    );
                }
            }
            if any_due {
                undelivered_fids.push(fid);
            }
        }
        // BRIDGE: a given-up row on a TERMINAL lane also ROTATES the lane — the ephemeral-terminal doctrine's sanctioned abandon. A dead pending can't be removed alone (mid-chain hash hole), so it used to cycle give-up → re-arm forever: the field zombies survived days and every bridge reopen because they lived in whichever sibling conversation was NOT being reopened. Rotation retires the lane wholesale (pendings legally dropped — for a terminal, stale is dead by definition); the next command mints fresh. Chat lanes are untouched: their give-up/revive dance is the durable design.
        {
            let mut rotated: Vec<crate::types::friendship::FriendshipId> = Vec::new();
            for (fid, _) in &gave_up_rows {
                if rotated.contains(fid) {
                    continue;
                }
                let is_sib = self
                    .contacts
                    .iter()
                    .any(|c| c.is_sibling && c.friendship_id == Some(*fid));
                if !is_sib {
                    continue;
                }
                if let Some((_, chains)) = self
                    .friendship_chains
                    .iter_mut()
                    .find(|(id, _)| *id == *fid)
                {
                    if let Some((dead, fresh, retired)) = chains.rotate_our_lane() {
                        crate::logf!("BRIDGE: give-up on a terminal lane — rotated {}... to {}... ({} dead pending(s) dropped for good)", hex::encode(&dead[..4]), hex::encode(&fresh[..4]), retired);
                        rotated.push(*fid);
                    }
                }
            }
            for fid in rotated {
                self.persist_chains_async(&fid);
            }
        }
        for (fid, t) in gave_up_rows {
            let Some(ci) = self
                .contacts
                .iter()
                .position(|c| c.is_sibling && c.friendship_id == Some(fid))
            else {
                continue;
            };
            let is_cmd = self.conv_of(ci).map_or(false, |conv| {
                conv.messages.iter().any(|m| {
                    m.is_outgoing
                        && m.timestamp == t
                        && matches!(m.reference, Some((crate::types::RefKind::BridgeCmd, _)))
                })
            });
            if !is_cmd {
                continue;
            }
            let already_done = self.conv_of(ci).map_or(false, |conv| {
                conv.messages.iter().any(|m| {
                    m.reference == Some((crate::types::RefKind::BridgeOut, t))
                        && m.bridge_exit.is_some()
                })
            });
            if already_done {
                continue;
            }
            if let Some(conv) = self.conv_mut_of(ci) {
                let mut msg = crate::types::ChatMessage::new_with_timestamp(
                    "…(command undeliverable — the host never acknowledged it; prompt released)"
                        .to_string(),
                    false,
                    vsf::eagle_time_oscillations(),
                );
                msg.reference = Some((crate::types::RefKind::BridgeOut, t));
                msg.bridge_exit = Some(-1);
                msg.bridge_seq = u64::MAX;
                conv.insert_message_sorted(msg);
                self.scene_dirty = true;
                crate::logf!("BRIDGE: command at eagle_time {} given up — prompt released", t);
            }
            if self.bridge_int.map_or(false, |(t0, _)| t0 == t) {
                self.bridge_int = None;
            }
        }

        // A give-up's only revival is the peer's tip in a pong's sync records — and an hour-deep presence backoff sits on exactly that exchange. Giving up IS the "this contact matters right now" edge: collapse the backoff so the tip flows on the next sweep instead of next hour.
        for fid in gave_up_fids {
            if let Some(c) = self
                .contacts
                .iter_mut()
                .find(|c| c.friendship_id == Some(fid))
            {
                c.ping_backoff = 0;
                c.last_pinged = None;
            }
        }

        // The doorbell cascade (docs/reachability-doorbell.md): a due retransmit IS "I have something for this peer and direct isn't landing". If we also haven't heard ANY signed traffic from them past the dozed threshold, their process likely isn't scheduled — ring the bell once. Double-debounced: `last_ring` here, the per-target guard on the worker. Under-ringing is the design bias: a brief packet-loss blip on a live conversation never wakes anyone (their pongs/acks keep last_heard fresh).
        const DOZED_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(90);
        const RE_RING_MIN: std::time::Duration = std::time::Duration::from_secs(300);
        if !undelivered_fids.is_empty() {
            if let Some(secret) = self.device_keypair.as_ref().map(|kp| *kp.secret.as_bytes()) {
                for c in self.contacts.iter_mut() {
                    let Some(fid) = c.friendship_id else { continue };
                    if !undelivered_fids.contains(&fid) {
                        continue;
                    }
                    let dozed = c
                        .last_heard
                        .map_or(true, |t| t.elapsed() >= DOZED_THRESHOLD);
                    let ring_ok = c.last_ring.map_or(true, |t| t.elapsed() >= RE_RING_MIN);
                    if dozed && ring_ok {
                        c.last_ring = Some(std::time::Instant::now());
                        crate::logf!(
                            "DOORBELL: {} has undelivered traffic and {}s+ of silence — ringing",
                            crate::fp(&c.handle_proof),
                            DOZED_THRESHOLD.as_secs()
                        );
                        crate::network::doorbell::spawn_ring(secret, c.handle_proof);
                    }
                }
            }
        }
    }

    /// Resolve a conversation token to the FRIEND (or self) contact it belongs to by DERIVING each contact's token from the participant party ids — no chain needed, so a fresh device can serve/merge fleet history before any CLUTCH completes. The self notes conversation derives from [our_pid, our_pid]. Chains, when they exist, derive the identical token (same participant set), so this agrees with the chain-bound friend route.
    pub(super) fn contact_idx_for_conversation_token(&self, token: &[u8; 32]) -> Option<usize> {
        let our_pid =
            crate::crypto::clutch::identity_party_id(&self.session.as_ref()?.identity_seed);
        self.contacts.iter().position(|c| {
            !c.is_sibling
                && crate::crypto::clutch::derive_conversation_token(&[our_pid, c.handle_hash])
                    == *token
        })
    }

    /// Fleet chain replication, the PUSH half ("if another device is ahead, I just catch up" — the catch-up is the ChainSyncReceived adopt arm). Per tick: any FRIEND chain whose mutated_osc is newer than the last push ships to EVERY sibling as a fleet-sealed chain_sync frame (canonical chains VSF bytes, kete-sealed under the fleet key, device-signed). Sibling 1:1 chains never replicate (device-pair-local by definition). The receiving sibling adopts iff the stamp is newer than its copy — so after any device advances a friendship (send ACK, receive), the whole fleet converges on the new head within transport latency, and any device can transmit next. Concurrent same-instant sends from two devices can still fork the braid (the §14 linearizer is the real serializer); catch-up shrinks that window to transport latency, and the fork-repair machinery (reset + re-key streak) is the backstop.
    pub(super) fn drive_chain_replication(&mut self) {
        // The B3 re-seal: chain_sync frames seal under the EPOCH chain_sync key, never the raw fleet key. No spine yet = the bounded bootstrap window — hold the push (the lane re-pushes the moment it next advances) rather than fork the seal.
        let Some((epoch_k, epoch)) = self.fleet_epoch else {
            return;
        };
        let chain_seal_key = crate::crypto::clutch::fleet_epoch_seal_key(&epoch, b"chain_sync");
        let Some(_fleet_key) = self.fleet_key_cached() else {
            return;
        };
        let (Some(kp), Some(checker)) =
            (self.device_keypair.as_ref(), self.status_checker.as_ref())
        else {
            return;
        };
        let has_sibling = self.contacts.iter().any(|c| c.is_sibling);
        if !has_sibling {
            return;
        }
        // Sibling 1:1 chains stay pair-local.
        let sibling_fids: std::collections::HashSet<[u8; 32]> = self
            .contacts
            .iter()
            .filter(|c| c.is_sibling && !c.locked_out)
            .filter_map(|c| c.friendship_id.map(|f| *f.as_bytes()))
            .collect();
        // Collect due frames first (read-only pass over the chains); the encode + seal + frame build ride ONE worker — kete over 17KB+ per changed friendship ran inline on the render thread, and an adopt storm (fresh sibling join repushes everything) multiplied it.
        let mut frames: Vec<(
            [u8; 32],
            i64,
            Vec<([u8; 32], u64)>,
            Option<crate::types::friendship::FriendshipChains>,
        )> = Vec::new();
        for (fid, chains) in &self.friendship_chains {
            let fid_bytes = *fid.as_bytes();
            if sibling_fids.contains(&fid_bytes) {
                continue;
            }
            let pushed = self.chain_pushed_osc.get(&fid_bytes).copied().unwrap_or(0);
            if chains.mutated_osc <= pushed {
                continue; // coarse gate: nothing on this friendship moved since the last push
            }
            // PER-LANE: send only the lanes whose position advanced past what we last pushed — a per-lane checkpoint subset, not the whole blob. A key of fid ‖ label.
            let changed: Vec<([u8; 32], u64)> = chains
                .lane_summary()
                .into_iter()
                .filter(|(label, pos)| {
                    let mut key = [0u8; 64];
                    key[..32].copy_from_slice(&fid_bytes);
                    key[32..].copy_from_slice(label);
                    self.lane_pushed_pos.get(&key).copied().unwrap_or(0) < *pos
                })
                .collect();
            if changed.is_empty() {
                // mutated_osc moved but no lane position did (e.g. a last_plaintext-only touch); record the coarse stamp so we don't re-scan every tick.
                frames.push((fid_bytes, chains.mutated_osc, Vec::new(), None));
                continue;
            }
            let changed_labels: Vec<[u8; 32]> = changed.iter().map(|(l, _)| *l).collect();
            let subset = chains.replication_subset(&changed_labels);
            frames.push((fid_bytes, chains.mutated_osc, changed, Some(subset)));
        }
        if frames.is_empty() {
            return;
        }
        // Sibling targets once — identical for every frame this tick. EVERY sibling, reachable or not: direct legs race, the relay covers the rest.
        let unspecified = std::net::SocketAddr::from(([0, 0, 0, 0], 0));
        let mut targets: Vec<(
            std::net::SocketAddr,
            Option<std::net::SocketAddr>,
            [u8; 32],
            Vec<[u8; 32]>,
        )> = Vec::new();
        for sib in self
            .contacts
            .iter()
            .filter(|c| c.is_sibling && !c.locked_out)
        {
            let (primary, alt, relay_to) = match sib.race_addrs() {
                Some((p, a)) => (
                    p,
                    a,
                    relay_unless_direct_trusted(&sib, crate::network::udp::get_local_ip()),
                ),
                None => {
                    let relays = sib.relay_device_list();
                    if relays.is_empty() {
                        continue;
                    }
                    (unspecified, None, relays)
                }
            };
            targets.push((primary, alt, *sib.public_identity.as_bytes(), relay_to));
        }
        let kp_pub = *kp.public.as_bytes();
        let kp_sec = *kp.secret.as_bytes();
        let dispatch = checker.history_dispatch();
        let mut work: Vec<([u8; 32], crate::types::friendship::FriendshipChains, usize)> =
            Vec::new();
        for (fid_bytes, osc, changed, subset) in frames {
            // A subset-less entry is the "stamp moved but no lane did" case: record the coarse stamp and move on, no transmit.
            let Some(subset) = subset else {
                self.chain_pushed_osc.insert(fid_bytes, osc);
                continue;
            };
            crate::logf!("CHAIN-SYNC: pushing {} changed lane(s) for {} to {} sibling(s) (per-lane checkpoint)", changed.len(), crate::fp(&fid_bytes), targets.len());
            // Positions record at DISPATCH — the same optimism as before this moved off-thread (send_history only ever queued; delivery was never confirmed here). A worker failure is loud and impossible-class (deterministic encode + seal with a held key); the lane re-pushes the moment it advances again.
            self.chain_pushed_osc.insert(fid_bytes, osc);
            for (label, pos) in &changed {
                let mut key = [0u8; 64];
                key[..32].copy_from_slice(&fid_bytes);
                key[32..].copy_from_slice(label);
                self.lane_pushed_pos.insert(key, *pos);
            }
            work.push((fid_bytes, subset, changed.len()));
        }
        if work.is_empty() || targets.is_empty() {
            return;
        }
        queue_job(&self.seal_job_tx, move || {
            for (fid_bytes, subset, lane_count) in work {
                let bytes = match crate::storage::friendship::chains_to_vsf_bytes(&subset) {
                    Ok(b) => b,
                    Err(e) => {
                        crate::logf!("CHAIN-SYNC CRITICAL: encode failed off-thread: {} — {} lane(s) for {} unpushed until the lane next advances", e, lane_count, crate::fp(&fid_bytes));
                        continue;
                    }
                };
                let sealed = match kete::encrypt_bytes(&bytes, &chain_seal_key) {
                    Ok(s) => s,
                    Err(e) => {
                        crate::logf!("CHAIN-SYNC CRITICAL: seal failed off-thread: {} — {} lane(s) for {} unpushed until the lane next advances", e, lane_count, crate::fp(&fid_bytes));
                        continue;
                    }
                };
                let frame = match crate::network::fgtw::protocol::build_chain_sync_vsf(
                    &subset.conversation_token,
                    epoch_k,
                    sealed,
                    &kp_pub,
                    &kp_sec,
                ) {
                    Ok(f) => f,
                    Err(e) => {
                        crate::logf!("CHAIN-SYNC CRITICAL: frame build failed off-thread: {} — {} lane(s) for {} unpushed until the lane next advances", e, lane_count, crate::fp(&fid_bytes));
                        continue;
                    }
                };
                for (primary, alt, pk, relay_to) in &targets {
                    let _ = dispatch.send(crate::network::status::HistorySendRequest {
                        peer_addr: *primary,
                        alt_addr: *alt,
                        recipient_pubkey: *pk,
                        relay_to: relay_to.clone(),
                        vsf_bytes: frame.clone(),
                    });
                }
            }
        });
    }

    /// Live fleet propagation: push just-written conversation rows for the friend/self contact at `idx` to EVERY sibling (reachable-or-not — direct legs race, relay covers the rest) as an unsolicited hist_page under the FLEET key. The receiving sibling merges them verbatim (an unmatched rid from a sibling IS the push signature) and re-pushes anything genuinely fresh, so a message hops the whole fleet even when only one device can reach its origin. Probe rows are filtered; a lost push self-heals via the sibling-online history sweep. `exclude_device` keeps a gossip hop from echoing straight back at its sender.
    pub(super) fn push_rows_to_siblings(
        &self,
        idx: usize,
        rows: &[crate::types::ChatMessage],
        exclude_device: Option<[u8; 32]>,
    ) {
        use crate::network::history_pages::{seal_history_page, HistoryPagePlain, HistoryRow};
        let contact = &self.contacts[idx];
        if contact.is_sibling {
            return; // sibling↔sibling chatter stays device-pair-local
        }
        // The B-arc re-seal: live fleet pages seal under the EPOCH hist_page key, never the raw fleet key. No spine yet = the bounded bootstrap window — hold the push (the sibling-online history sweep re-covers these rows) rather than fork the seal, exactly the chain_sync rule.
        let (Some((epoch_k, epoch)), Some(kp), Some(checker), Some(session)) = (
            self.fleet_epoch,
            self.device_keypair.as_ref(),
            self.status_checker.as_ref(),
            self.session.as_ref(),
        ) else {
            return;
        };
        let page_key = crate::crypto::clutch::fleet_epoch_seal_key(&epoch, b"hist_page");
        let hist_rows: Vec<HistoryRow> = rows
            .iter()
            .filter(|m| !crate::types::is_control_content(&m.content))
            .map(|m| HistoryRow {
                timestamp: m.timestamp,
                content: m.content.clone(),
                sender_outgoing: m.is_outgoing,
                delivered: m.delivered,
                deleted: m.deleted,
                reference: m.reference.map(|(k, t)| (k as u8, t)),
                notified: m.notified,
            })
            .collect();
        if hist_rows.is_empty() {
            return;
        }
        let our_pid = crate::crypto::clutch::identity_party_id(&session.identity_seed);
        let token =
            crate::crypto::clutch::derive_conversation_token(&[our_pid, contact.handle_hash]);
        let page = HistoryPagePlain {
            oldest_osc: hist_rows.iter().map(|r| r.timestamp).min().unwrap_or(0),
            more: false,
            rows: hist_rows,
        };
        let rid: [u8; 32] = rand::random();
        // Targets first (needs &self); the kete seal + frame build + send ride a worker — sealing a full page inline was another render-thread cost on every live push and every gossip hop.
        let unspecified = std::net::SocketAddr::from(([0, 0, 0, 0], 0));
        let mut targets: Vec<(
            std::net::SocketAddr,
            Option<std::net::SocketAddr>,
            [u8; 32],
            Vec<[u8; 32]>,
        )> = Vec::new();
        let mut skipped = 0usize;
        // EVERY sibling, not just is_online ones: sibling presence has proven unreliable (pong-provenance drops kept siblings "offline" for whole sessions), and gating delivery on it turned the entire fleet plane into a silent no-op — zero FLEET-HIST lines in a full day's desktop log while messages flowed. Direct legs race as before; a sibling with no validated path (or no address at all) rides the relay, which delivers whenever that device next drains its pipe.
        for sib in self
            .contacts
            .iter()
            .filter(|c| c.is_sibling && !c.locked_out)
        {
            if exclude_device.is_some_and(|d| *sib.public_identity.as_bytes() == d) {
                continue;
            }
            let (primary, alt, relay_to) = match sib.race_addrs() {
                Some((p, a)) => (
                    p,
                    a,
                    relay_unless_direct_trusted(&sib, crate::network::udp::get_local_ip()),
                ),
                // No known address: relay-only (the unspecified primary skips the direct legs in the send worker).
                None => {
                    let relays = sib.relay_device_list();
                    if relays.is_empty() {
                        skipped += 1;
                        continue;
                    }
                    (unspecified, None, relays)
                }
            };
            targets.push((primary, alt, *sib.public_identity.as_bytes(), relay_to));
        }
        // ALWAYS log, zero included — a silently-no-op fleet push is exactly how this path shipped broken.
        crate::logf!(
            "FLEET-HIST: live push {} row(s) for {} → {} sibling(s) ({} unreachable)",
            page.rows.len(),
            crate::fp(&self.contacts[idx].handle_proof),
            targets.len(),
            skipped
        );
        if targets.is_empty() {
            return;
        }
        let kp_pub = *kp.public.as_bytes();
        let kp_sec = *kp.secret.as_bytes();
        let dispatch = checker.history_dispatch();
        queue_job(&self.seal_job_tx, move || {
            let skf = u32::from_le_bytes(blake3::hash(&page_key).as_bytes()[..4].try_into().unwrap()) as u64;
            let vsf_bytes = match seal_history_page(&page, &page_key).and_then(|sealed| {
                crate::network::fgtw::protocol::build_history_page_vsf(
                    &token,
                    &rid,
                    Some(epoch_k),
                    Some(skf),
                    sealed,
                    &kp_pub,
                    &kp_sec,
                )
            }) {
                Ok(b) => b,
                Err(e) => {
                    crate::logf!("FLEET-HIST: live push build failed: {}", e);
                    return;
                }
            };
            for (primary, alt, pk, relay_to) in targets {
                let _ = dispatch.send(crate::network::status::HistorySendRequest {
                    peer_addr: primary,
                    alt_addr: alt,
                    recipient_pubkey: pk,
                    relay_to,
                    vsf_bytes: vsf_bytes.clone(),
                });
            }
        });
    }

    /// Sibling fork repair, the APPLY half: deterministically rebuild the sibling 1:1 chains from `nonce`, reset the weave, persist, optionally echo the frame (once — the nonce dedup in the drain stops the ping-pong), and re-probe. Both sides run exactly this from the same nonce and land on byte-identical chains: synthetic "eggs" = BLAKE3-XOF(domain ‖ fleet_key ‖ nonce ‖ sorted sibling pids), fed thru the SAME from_clutch path a real ceremony uses. Fleet-key-derived is sound here because a sibling 1:1 is between two devices of ONE owner — the chain provides transport integrity, not inter-device secrecy. Rarangi rows are untouched: history survives, only chain state re-anchors. The 2MB avalanche expand runs inline (~1s, rare repair event — same UI-thread cost as the known ceremony hitch).
    pub(super) fn apply_sibling_chain_reset(&mut self, idx: usize, nonce: [u8; 32], echo: bool) {
        let (Some(fleet_key), Some(kp)) = (self.fleet_key_cached(), self.device_keypair.as_ref())
        else {
            crate::log("CHAIN-RESET: missing fleet key or device key — cannot apply");
            return;
        };
        let our_pid = crate::crypto::clutch::sibling_party_id(kp.public.as_bytes());
        let device_pub = *kp.public.as_bytes();
        let device_sec = *kp.secret.as_bytes();
        let their_pid = self.contacts[idx].handle_hash;
        let mut sorted = [our_pid, their_pid];
        sorted.sort();
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"PHOTON_SIBLING_CHAIN_RESET_v0");
        hasher.update(&fleet_key);
        hasher.update(&nonce);
        hasher.update(&sorted[0]);
        hasher.update(&sorted[1]);
        let mut egg_bytes = [0u8; 640];
        hasher.finalize_xof().fill(&mut egg_bytes);
        let eggs: Vec<[u8; 32]> = egg_bytes
            .chunks_exact(32)
            .map(|c| {
                let mut e = [0u8; 32];
                e.copy_from_slice(c);
                e
            })
            .collect();
        crate::logf!(
            "CHAIN-RESET: rebuilding sibling chains with {} (nonce {})",
            crate::fp(&self.contacts[idx].handle_proof),
            hex::encode(&nonce[..4])
        );
        let chains =
            crate::types::friendship::FriendshipChains::from_clutch(&[our_pid, their_pid], &eggs);
        let fid = chains.friendship_id;
        let conversation_token = chains.conversation_token;
        self.friendship_chains.retain(|(id, _)| *id != fid);
        self.friendship_chains.push((fid, chains));
        if let Some(storage) = self.storage.as_ref() {
            if let Some((_, c)) = self.friendship_chains.iter().find(|(id, _)| *id == fid) {
                if let Err(e) = crate::storage::friendship::save_friendship_chains(c, storage) {
                    crate::logf!("CHAIN-RESET: persist failed: {}", e);
                }
            }
        }
        {
            let contact = &mut self.contacts[idx];
            contact.friendship_id = Some(fid);
            contact.probe_sent = false;
            contact.their_probe_seen = false;
            contact.their_probe_ceremony = None; // the chain itself is gone — drop the attribution so a stale id can't match a future ceremony
            contact.chain_advanced_by_ack = false;
            contact.chain_woven = false;
            contact.chain_fail_streak = 0;
            contact.last_chain_reset_nonce = Some(nonce);
            if let Some(storage) = self.storage.as_ref() {
                let _ = crate::storage::contacts::save_contact(contact, storage);
            }
        }
        if echo {
            let sealed = match kete::encrypt_bytes(&nonce, &fleet_key) {
                Ok(s) => s,
                Err(e) => {
                    crate::logf!("CHAIN-RESET: seal failed: {}", e);
                    return;
                }
            };
            match crate::network::fgtw::protocol::build_chain_reset_vsf(
                &conversation_token,
                sealed,
                &device_pub,
                &device_sec,
            ) {
                Ok(vsf_bytes) => {
                    if let (Some(checker), Some((primary, alt))) = (
                        self.status_checker.as_ref(),
                        self.contacts[idx].race_addrs(),
                    ) {
                        checker.send_history(crate::network::status::HistorySendRequest {
                            peer_addr: primary,
                            alt_addr: alt,
                            recipient_pubkey: *self.contacts[idx].public_identity.as_bytes(),
                            relay_to: if self.contacts[idx].validated_path.is_none() {
                                self.contacts[idx].relay_device_list()
                            } else {
                                Vec::new()
                            },
                            vsf_bytes,
                        });
                    }
                }
                Err(e) => crate::logf!("CHAIN-RESET: frame build failed: {}", e),
            }
        }
        // Fresh chain, fresh weave: fire the hidden probe so the repaired pair proves itself end-to-end and seals.
        self.maybe_send_chain_probe(idx);
    }

    /// Friend fork repair: a woven pair whose chains diverged has no shared key to rebuild from, but a fresh ceremony is always legal — nuke our chains + round, claim the ceremony (§4.2, so our siblings park), and let the keygen queue mint the new offer; the friend's Complete-rekey path accepts it, both weave fresh, history rows survive, and recovery backfills anything the fork swallowed. Rate-limited on the same cooldown slot as the sibling reset.
    pub(super) fn initiate_friend_rekey(&mut self, idx: usize) {
        const RESET_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(60);
        if self.contacts[idx]
            .last_chain_reset_sent
            .is_some_and(|t| t.elapsed() < RESET_COOLDOWN)
        {
            return;
        }
        self.contacts[idx].last_chain_reset_sent = Some(std::time::Instant::now());
        crate::logf!(
            "CHAIN-REKEY: {} — forked woven chain; discarding for a fresh ceremony",
            crate::fp(&self.contacts[idx].handle_proof)
        );
        if let Some(fid) = self.contacts[idx].friendship_id.take() {
            for (id, chains) in self.friendship_chains.iter_mut() {
                if *id == fid {
                    chains.zeroize_history_key();
                    chains.zeroize_lane_root();
                }
            }
            self.friendship_chains.retain(|(id, _)| *id != fid);
            if let Some(storage) = self.storage.as_ref() {
                let _ = crate::storage::friendship::delete_friendship_chains(&fid, storage);
            }
        }
        let our_device = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes());
        let c = &mut self.contacts[idx];
        c.discard_clutch_round();
        c.chain_fail_streak = 0;
        c.chain_woven = false;
        c.probe_sent = false;
        c.their_probe_seen = false;
        c.their_probe_ceremony = None; // ditto: the round was discarded, so nothing may inherit its seal
        c.chain_advanced_by_ack = false;
        c.clutch_proof_retry_lifetime = 0;
        c.clutch_proof_gave_up = false;
        if let Some(dev) = our_device {
            c.ceremony_owner = Some(dev);
            c.owner_woven = false;
            c.roster_updated = vsf::eagle_time_oscillations();
        }
        if let Some(storage) = self.storage.as_ref() {
            let _ = crate::storage::contacts::save_contact(&self.contacts[idx], storage);
        }
    }

    /// Sibling fork repair, the INITIATE half: rate-limited nonce mint + local apply + frame send (the apply's echo path IS the send). The responder applies on receipt and echoes once; the initiator's nonce dedup swallows the echo. A lost frame self-heals: the fork persists, the detector re-fires past the rate-limit window with a fresh nonce.
    pub(super) fn initiate_sibling_chain_reset(&mut self, idx: usize) {
        const RESET_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(30);
        if self.contacts[idx]
            .last_chain_reset_sent
            .is_some_and(|t| t.elapsed() < RESET_COOLDOWN)
        {
            return;
        }
        self.contacts[idx].last_chain_reset_sent = Some(std::time::Instant::now());
        let nonce: [u8; 32] = rand::random();
        self.apply_sibling_chain_reset(idx, nonce, true);
    }

    /// Fleet history sweep: (re-)arm history recovery for every friend/self conversation so the driver walks each one from the head — served by a sibling when the friend can't. Early-stop makes a re-sweep of an already-complete conversation cost ONE page (zero new rows → complete again), so this fires freely on sibling-online edges and roster merges; conversations mid-walk are left alone.
    /// DURABLE FLEET-FORWARD (the clamshell lesson, 2026-08-30): the live push in `drain_pending_chain_sends` is one shot into possibly-dead pipes — a sibling asleep thru the relay delivery never sees the row, and a chainless origin has no retransmit ladder of its own (chain_transmit never ran), so the message sat undelivered forever while the log said "delivered". Re-offer every outgoing undelivered row to the fleet on the backstop + sibling-online edges until some device transmits and the delivered upgrade replicates back. Receiving siblings dedup known rows (the merge is (timestamp, content)-keyed and only FRESH rows drain to the wire), so a re-push is merge-noise at worst.
    pub(super) fn reserve_fleet_forwards(&self) {
        const FWD_BURST: usize = 8; // oldest-first, same burst shape as the friend re-serve
        if !self.contacts.iter().any(|c| c.is_sibling && !c.locked_out) {
            return;
        }
        for ci in 0..self.contacts.len() {
            let c = &self.contacts[ci];
            if c.is_sibling {
                continue;
            }
            // A device that can transmit itself owns delivery thru its own retransmit ladder — re-serving to siblings is the CHAINLESS origin's only path.
            if c.chain_woven || self.lane_transmit_capable(ci) {
                continue;
            }
            let Some(conv) = self.conv_of(ci) else { continue };
            let mut rows: Vec<crate::types::ChatMessage> = conv
                .messages
                .iter()
                // Same sendable shape as resend_held_messages: a reaction RETRACT is a legal empty-content row; a plain empty row stays filtered.
                .filter(|m| {
                    m.is_outgoing
                        && !m.delivered
                        && !m.deleted
                        && (!m.content.is_empty() || m.reference.is_some())
                })
                .cloned()
                .collect();
            if rows.is_empty() {
                continue;
            }
            rows.sort_by_key(|m| m.timestamp);
            rows.truncate(FWD_BURST);
            crate::logf!(
                "FLEET-HIST: re-serving {} undelivered forward(s) for {} (chainless origin)",
                rows.len(),
                crate::fp(&c.handle_proof)
            );
            self.push_rows_to_siblings(ci, &rows, None);
        }
    }

    pub(super) fn kick_fleet_history_sweep(&mut self, reason: &str) {
        let mut kicked = 0usize;
        for ci in 0..self.contacts.len() {
            if self.contacts[ci].is_sibling {
                continue;
            }
            let Some(conv) = self.conv_mut_of(ci) else {
                continue;
            };
            if conv.history_recovery.as_ref().is_some_and(|r| !r.complete) {
                continue; // mid-walk — the driver is already on it
            }
            let was_complete_before = conv
                .history_recovery
                .as_ref()
                .map(|r| r.complete)
                .unwrap_or(false);
            conv.history_recovery = Some(crate::types::HistoryRecovery {
                oldest_recovered_osc: i64::MAX,
                complete: false,
                in_flight: None,
                next_request_osc: 0,
                urgent: false, // background catch-up rides the trickle
                was_complete_before,
                decrypt_fail_streak: 0,
                parked_key_fp: None,
            });
            kicked += 1;
        }
        if kicked > 0 {
            crate::logf!(
                "HISTORY: fleet sweep armed {} conversation(s) ({})",
                kicked,
                reason
            );
        }
    }

    /// History-recovery driver (every tick): for each contact mid-backfill, expire a lost in-flight request and fire the next page request when due. Newest-first cursor pagination — `urgent` (weave-seal kickoff / scrollback) jumps the trickle interval; otherwise pages are rate-limited to one per HIST_TRICKLE_OSC so a 10-year backfill hums along in the background without competing with live traffic. Requests are idempotent (rid-correlated, merge dedups), so an expiry + re-request after a lost page is always safe. Each request routes to whichever SOURCE is available right now: the friend (woven chain, history key) or a fleet sibling (fleet key) — the cursor is conversation-level, so the walk continues seamlessly across sources.
    pub(super) fn drive_history_recovery(&mut self) {
        const HIST_TRICKLE_OSC: i64 = 2 * crate::OSC_PER_SEC; // one page per ~2s in background
        const HIST_INFLIGHT_TIMEOUT_OSC: i64 = 45 * crate::OSC_PER_SEC; // lost request/page — longer than PT's ~31s ladder-then-relay so the fallback can actually fire before the request is abandoned (15s starved it forever)

        let now_osc = vsf::eagle_time_oscillations();

        // Snapshot device keys once (frame building signs on this thread).
        let Some(kp) = self.device_keypair.as_ref() else {
            return;
        };
        let device_pubkey = *kp.public.as_bytes();
        let device_secret = *kp.secret.as_bytes();

        // Route material computed once: a sibling serves fleet-route requests, and the fleet key gates them (the sibling seals under it). Presence discipline (2026-08-26): prefer an online sibling with a direct address; the fallback arms accept UNPROBED siblings (a hard is_online gate starved the fleet backfill during the boot race) but exclude a POSITIVE offline verdict — the relay already said that pipe is closed, and re-asking every expiry cycle was the 497-retry storm.
        let sibling_target: Option<(
            std::net::SocketAddr,
            Option<std::net::SocketAddr>,
            [u8; 32],
            Vec<[u8; 32]>,
        )> = if self.fleet_key_cached().is_some() {
            let unspecified = std::net::SocketAddr::from(([0, 0, 0, 0], 0));
            self.contacts
                .iter()
                .filter(|c| c.is_sibling && c.is_online)
                .find_map(|c| {
                    c.race_addrs().map(|(p, a)| {
                        (
                            p,
                            a,
                            *c.public_identity.as_bytes(),
                            relay_unless_direct_trusted(&c, crate::network::udp::get_local_ip()),
                        )
                    })
                })
                .or_else(|| {
                    // Fallback arms exclude only a POSITIVE offline verdict (probed + 3-timeout), never the unprobed boot state — the ungated version picked the offline phone as the relay-only target and fired a request into its closed pipe every expiry cycle forever (497 retries + ~470 dropped 17KB frames in one 35-min field log). The came-online edge re-includes the device naturally.
                    self.contacts
                        .iter()
                        .filter(|c| c.is_sibling && !(c.presence_probed && !c.is_online))
                        .find_map(|c| {
                            c.race_addrs().map(|(p, a)| {
                                (p, a, *c.public_identity.as_bytes(), c.relay_device_list())
                            })
                        })
                })
                .or_else(|| {
                    self.contacts
                        .iter()
                        .filter(|c| c.is_sibling && !(c.presence_probed && !c.is_online))
                        .find_map(|c| {
                            let relays = c.relay_device_list();
                            if relays.is_empty() {
                                None
                            } else {
                                Some((unspecified, None, *c.public_identity.as_bytes(), relays))
                            }
                        })
                })
        } else {
            None
        };
        let our_pid = self
            .session
            .as_ref()
            .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed));

        // Candidate pass (read-only): eligible contacts + the best available route. FRIEND route (woven keyed chain, friend online) is preferred — it's the authoritative two-party copy; otherwise the FLEET route asks an online sibling under the fleet key, which needs no chain at all (a roster-merged contact backfills before its first CLUTCH, and the self notes conversation only ever has this route).
        let candidates: Vec<(
            usize,
            [u8; 32],
            std::net::SocketAddr,
            Option<std::net::SocketAddr>,
            [u8; 32],
            Vec<[u8; 32]>,
        )> = self
            .contacts
            .iter()
            .enumerate()
            .filter_map(|(idx, c)| {
                if c.is_sibling {
                    return None;
                }
                let rec = dm_conversation(&self.conversations, &our_pid?, c)?
                    .history_recovery
                    .as_ref()?;
                if rec.complete {
                    return None;
                }
                // Friend route: token from the chain (identical to the derived one — same participant set). No chain_woven gate — the page-seal capability is the HISTORY KEY (required just below), and stale woven flags from the wedge/rotation era stranded a behind device with an armed walk it could never drive (2026-08-11).
                if c.is_online {
                    if let Some(fid) = c.friendship_id {
                        if let Some((_, chains)) =
                            self.friendship_chains.iter().find(|(id, _)| *id == fid)
                        {
                            if chains.history_key().is_some() {
                                if let Some((primary, alt)) = c.race_addrs() {
                                    // No validated direct path → the request ALSO rides the relay immediately (chat's relay_to rule); PT's own ladder-then-relay takes longer than the requester's expiry, so relay-only friends starved on it forever.
                                    let relay_to = relay_unless_direct_trusted(
                                        &c,
                                        crate::network::udp::get_local_ip(),
                                    );
                                    return Some((
                                        idx,
                                        chains.conversation_token,
                                        primary,
                                        alt,
                                        *c.public_identity.as_bytes(),
                                        relay_to,
                                    ));
                                }
                            }
                        }
                    }
                }
                // Fleet route: derive the token from the participant party ids; the relay list rides from the target pick so an unreachable-direct sibling still serves via its pipe.
                let (primary, alt, sib_pk, sib_relay) = sibling_target.clone()?;
                let pid = our_pid?;
                Some((
                    idx,
                    crate::crypto::clutch::derive_conversation_token(&[pid, c.handle_hash]),
                    primary,
                    alt,
                    sib_pk,
                    sib_relay,
                ))
            })
            .collect();
        if candidates.is_empty() {
            return;
        }
        let Some(checker) = self.status_checker.as_ref() else {
            return;
        };

        for (idx, token, primary, alt, recipient_pubkey, relay_to) in candidates {
            // Field-precise lookup (`checker` holds a field borrow of self across this loop, so no &mut self method fits here).
            let Some(pid) = our_pid else {
                continue;
            };
            let cid = self.contacts[idx].conversation(&pid).id();
            let Some(rec) = self
                .conversations
                .iter_mut()
                .find(|v| v.id() == cid)
                .and_then(|v| v.history_recovery.as_mut())
            else {
                continue;
            };
            // DIVERGENCE PARK: while the conversation's history key still fingerprints as the one that kept failing to open pages, a re-request just re-downloads the same undecryptable 17KB — skip until the key CHANGES (re-key completed / era adopted), which is the resume edge. State comparison per sweep, no timer.
            if let Some(parked) = rec.parked_key_fp {
                let current_fp: Option<[u8; 4]> = self.contacts[idx]
                    .friendship_id
                    .and_then(|fid| self.friendship_chains.iter().find(|(id, _)| *id == fid))
                    .and_then(|(_, c)| c.history_key().copied())
                    .map(|k| blake3::hash(&k).as_bytes()[..4].try_into().unwrap());
                if current_fp == Some(parked) {
                    continue;
                }
                rec.parked_key_fp = None;
                rec.decrypt_fail_streak = 0;
                crate::logf!(
                    "HISTORY: walk RESUMED — history key changed (was key#{})",
                    hex::encode(parked)
                );
            }
            // Expire a lost in-flight request so the walk resumes.
            if let Some((_, sent_osc, _)) = rec.in_flight {
                if now_osc.saturating_sub(sent_osc) > HIST_INFLIGHT_TIMEOUT_OSC {
                    crate::log("HISTORY: in-flight request expired — re-requesting");
                    rec.in_flight = None;
                } else {
                    continue; // one request at a time per conversation
                }
            }
            if !rec.urgent && now_osc < rec.next_request_osc {
                continue; // trickle interval not up yet
            }

            let rid: [u8; 32] = rand::random();
            let before = rec.oldest_recovered_osc;
            match crate::network::fgtw::protocol::build_history_request_vsf(
                &token,
                before,
                crate::network::history_pages::MAX_PAGE_ROWS as u32,
                &rid,
                &device_pubkey,
                &device_secret,
            ) {
                Ok(vsf_bytes) => {
                    rec.in_flight = Some((rid, now_osc, before));
                    rec.next_request_osc = now_osc + HIST_TRICKLE_OSC;
                    rec.urgent = false;
                    // Authoritative rid registry (see hist_rid_map): sweep stale entries, then register this request so its page merges even if another contact resolving the same peer re-arms this conversation's in_flight before the answer lands.
                    self.hist_rid_map.retain(|_, (_, sent)| {
                        now_osc.saturating_sub(*sent) <= HIST_INFLIGHT_TIMEOUT_OSC
                    });
                    self.hist_rid_map.insert(rid, (cid, now_osc));
                    crate::logf!(
                        "HISTORY: requesting page before {} from {}",
                        if before == i64::MAX {
                            "HEAD".to_string()
                        } else {
                            before.to_string()
                        },
                        primary
                    );
                    checker.send_history(crate::network::status::HistorySendRequest {
                        peer_addr: primary,
                        alt_addr: alt,
                        recipient_pubkey,
                        vsf_bytes,
                        relay_to: relay_to.clone(),
                    });
                }
                Err(e) => crate::logf!("HISTORY: request build failed: {}", e),
            }
        }
    }

    /// Blind-ops driver (every tick, beside `drive_history_recovery`): keeps the friend-blinded private-identity-secret machinery converged. Per eligible friend (online, woven, mutual, not a sibling): expire a lost in-flight op (~15s), then fire the ONE op this contact needs — a `blind_get` while S is unknown and this friend hasn't answered `found=0` yet (probe and reconstitute are the SAME op), or a `blind_put` while S exists and this friend hasn't disk-confirmed our deposit. One op in flight per contact; responses land in the `BlindFrameReceived` arm. Steady state (S live, deposits confirmed everywhere) is a pure no-op.
    pub(super) fn drive_blind_ops(&mut self) {
        use crate::crypto::blind::PrivateS;
        const BLIND_INFLIGHT_TIMEOUT_OSC: i64 = 15 * crate::OSC_PER_SEC;
        let now_osc = vsf::eagle_time_oscillations();

        let Some(our_seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            return;
        };
        let Some(kp) = self.device_keypair.as_ref() else {
            return;
        };
        let device_pubkey = *kp.public.as_bytes();
        let device_secret = *kp.secret.as_bytes();
        let s_known = !matches!(self.private_s, PrivateS::None);
        // A stack copy for blob building inside the contacts borrow; lives only this call.
        let s_copy: Option<zeroize::Zeroizing<[u8; 32]>> = self
            .private_s
            .secret()
            .map(|s| zeroize::Zeroizing::new(**s));
        let Some(checker) = self.status_checker.as_ref() else {
            return;
        };

        for contact in self.contacts.iter_mut() {
            if !contact.is_online || !contact.chain_woven || !contact.is_mutual() {
                continue;
            }
            // Expire a lost op so the machinery retries.
            if let Some((_, sent_osc, _)) = contact.blind_in_flight {
                if now_osc.saturating_sub(sent_osc) > BLIND_INFLIGHT_TIMEOUT_OSC {
                    crate::log("BLIND: in-flight op expired — retrying");
                    contact.blind_in_flight = None;
                } else {
                    continue; // one op at a time per contact
                }
            }
            // Which op does this contact need? Siblings are PROBE-only — an S-less device pulls S over the sealed sibling channel (blind_get → AEAD-sealed srv); deposits go to friends only (a sibling holding our OTP blind would be pointless — it serves S itself when it has one).
            let want_probe = !s_known && !contact.blind_probe_missed;
            let want_put = s_known && !contact.blind_deposited && !contact.is_sibling;
            if !want_probe && !want_put {
                continue;
            }
            let Some((primary, alt)) = contact.race_addrs() else {
                continue;
            };
            // Party-id seam: sibling tokens derive from the device pids (fleet weave), friend tokens from the identity PARTY IDS — the same inputs the peer derives with. (Was the raw seed: my {seed, their_pid} vs their {their_seed, my_pid} never agreed, so every blind put/get bounced off the peer's unknown-token gate — the exact seam class that hung chat/CLUTCH/history.)
            let our_pid = if contact.is_sibling {
                crate::crypto::clutch::sibling_party_id(&device_pubkey)
            } else {
                crate::crypto::clutch::identity_party_id(&our_seed)
            };
            let token =
                crate::crypto::clutch::derive_conversation_token(&[our_pid, contact.handle_hash]);
            let rid: [u8; 32] = rand::random();
            let built = if want_probe {
                crate::network::fgtw::protocol::build_blind_get_vsf(
                    &token,
                    &rid,
                    &device_pubkey,
                    &device_secret,
                )
            } else {
                let Some(s) = s_copy.as_ref() else { continue };
                let pad =
                    crate::crypto::blind::derive_blind_pad(&device_secret, &contact.handle_hash);
                let blob = crate::crypto::blind::make_blind_blob(s, &pad);
                crate::network::fgtw::protocol::build_blind_put_vsf(
                    &token,
                    &rid,
                    &blob,
                    &device_pubkey,
                    &device_secret,
                )
            };
            match built {
                Ok(vsf_bytes) => {
                    contact.blind_in_flight = Some((rid, now_osc, want_probe));
                    crate::logf!(
                        "BLIND: {} {}",
                        if want_probe {
                            "probing for our deposit at"
                        } else {
                            "depositing our blind with"
                        },
                        crate::fp(&contact.handle_proof)
                    );
                    // BLIND frames ALWAYS ride the relay alongside any direct path. A validated path can be one-directional (their probes reach us, our answers vanish — observed live: one side served found=0 every 15s while the peer's probe expired every 15s, forever), and a lost blind frame stalls S-recovery silently. The frames are tiny and idempotent by request id, so the duplicate costs nothing.
                    checker.send_history(crate::network::status::HistorySendRequest {
                        peer_addr: primary,
                        alt_addr: alt,
                        recipient_pubkey: *contact.public_identity.as_bytes(),
                        relay_to: contact.relay_device_list(),
                        vsf_bytes,
                    });
                }
                Err(e) => crate::logf!("BLIND: frame build failed: {}", e),
            }
        }
    }

    /// Probe-before-generate verdict, called when a `blind_srv` miss lands while S is None. Generates a fresh S ONLY when no probe is still in flight and EVERY eligible online+woven friend has answered `found=0` — i.e. the network reachable right now provably holds no deposit for this device. A single hit anywhere reconstitutes instead (handled at the srv arrival). This asymmetry is the whole point: a `[]n`-reset device must RECOVER its S, never mint a second one while a deposit is reachable.
    pub(super) fn maybe_generate_s(&mut self) {
        use crate::crypto::blind::PrivateS;
        if !matches!(self.private_s, PrivateS::None) {
            return;
        }
        let mut any_eligible = false;
        for c in &self.contacts {
            // Siblings count: a woven sibling holding S serves it (a hit); one without answers found=0 like a friend, so the all-missed rule still converges. (Two FRESH siblings with zero friends can both generate — the deterministic lower-s_id tie-break at srv-adoption converges them after.)
            if !c.is_online || !c.chain_woven || !c.is_mutual() {
                continue;
            }
            any_eligible = true;
            if c.blind_in_flight.map_or(false, |(_, _, is_get)| is_get) {
                return; // a probe is still out — its answer decides
            }
            if !c.blind_probe_missed {
                return; // not asked/answered yet — the driver will probe it
            }
        }
        if !any_eligible {
            return; // nobody reachable to attest a miss — keep waiting
        }
        let mut s = zeroize::Zeroizing::new([0u8; 32]);
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), s.as_mut());
        crate::log("S: generated (provisional) — no deposit found at any reachable friend");
        self.private_s = PrivateS::Provisional(s);
        for c in self.contacts.iter_mut() {
            c.blind_deposited = false;
        }
        // Deposit immediately (the answering friend is online right now); Provisional→Live flips on the first blind_ack.
        self.drive_blind_ops();
    }

    /// Ping a single contact (on conversation-enter) so its presence refreshes promptly. Same LAN-IPv4-preferring address selection as `ping_contacts`.
    pub(super) fn ping_contact(&mut self, idx: usize) {
        let Some(checker) = self.status_checker.as_ref() else {
            return;
        };
        let Some(contact) = self.contacts.get(idx) else {
            return;
        };
        // The SELF contact is this fleet, not a network peer: pinging it makes our own devices answer as "wrong responder", and the punch machinery then storms our own addresses forever (the 7ff3835f probe spam in every 2026-07-26 log). Presence for our devices rides the SIBLING contacts.
        if self
            .session
            .as_ref()
            .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
            == Some(contact.handle_hash)
        {
            return;
        }
        let addr = match (contact.local_ip, contact.local_port) {
            (Some(ip), Some(port)) => {
                Some(std::net::SocketAddr::new(std::net::IpAddr::V4(ip), port))
            }
            _ => contact.ip,
        };
        if let Some(ip) = addr {
            let punch: Vec<std::net::SocketAddr> = match contact.validated_path {
                Some((remote, _)) => vec![remote], // keepalive the validated path
                None => crate::network::traverse::gather::gather_peer_candidates(contact)
                    .sorted()
                    .into_iter()
                    .map(|c| c.addr)
                    .collect(),
            };
            let relay_to =
                relay_unless_direct_trusted(&contact, crate::network::udp::get_local_ip());
            checker.ping(ip, contact.public_identity.clone(), punch, relay_to);
        }
    }
}
