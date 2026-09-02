//! `check_status_updates` — the status-channel drain: every inbound ping/pong/chat/ACK/CLUTCH frame lands here and mutates contacts, conversations, and ceremonies.

use super::*;

impl PhotonApp {
    /// Drain `StatusUpdate`s from the checker and apply them to contacts. v1 (presence checkpoint) handles only `Online`: match the pong's pubkey to a contact, update its `ip` from the source address, and flip `is_online`. Returns true if any contact changed (→ redraw the list ring). The CLUTCH arms (offer/KEM/complete) land in the follow-up commit. Chat/ack/PT arms are intentionally ignored (messaging not yet ported).
    pub fn check_status_updates(&mut self) -> bool {
        use crate::crypto::clutch;
        use crate::network::status::StatusUpdate;
        // NOTE: ClutchRequest and ClutchRequestType imports removed - legacy v1 CLUTCH no longer used
        use crate::types::ClutchState;

        // Per-DRAIN stall attribution, the arm timer's sibling: the 2026-08-08 field log showed 400-1700ms ticks with almost no arm attribution — the time was in these drains, which nothing named. >50ms logs the drain, so the next log round points at code instead of at a guess.
        macro_rules! timed_drain {
            ($label:literal, $body:expr) => {{
                let __t = std::time::Instant::now();
                let __r = $body;
                let __ms = __t.elapsed().as_millis();
                if __ms > 50 {
                    crate::logf!("PERF: drain {} took {}ms (UI thread)", $label, __ms);
                }
                __r
            }};
        }
        // Region attribution (2026-08-15 field log): the OUTER timer showed 400-1794ms while the pass profile (>200ms) stayed SILENT — the cost lives outside the arm loop, in the pre-loop drains or the post-loop deferred section, which nothing named. Two coarse region timers pin the side; the guilty region gets fine-grained timers next round.
        let preloop_t = std::time::Instant::now();
        // Peer avatars: install any completed downloads, then kick a fetch (once/session/handle) for any contact still without one. Cache-first + dedup'd by avatar_dl_started, so this is cheap to run every tick — it spawns at most one thread per peer per session.
        // Express call signals FIRST — a doorbell outranks every other drain on the tick (rare + tiny; empty = one mutex).
        timed_drain!("call_express", self.drain_express_signals());
        // A finished audio calibration posts its profile here (rare; empty = one mutex).
        timed_drain!("audio_cal", self.drain_audio_cal());
        timed_drain!("avatar", self.drain_avatar_downloads());
        timed_drain!("attach", self.drain_attach_installed());
        // History pages the decrypt workers finished since last tick — merge before the arm loop so a walk's next request goes out on this tick's sweep, not the next.
        timed_drain!("history_pages", self.drain_history_pages());
        // Chain-sync blobs the open workers finished — adopt before the arm loop so this tick's replication push already carries the adopted heads.
        timed_drain!("chain_syncs", self.drain_chain_syncs());
        // Bridge command output the off-thread executor finished — reply over the chain (RefKind::BridgeOut). Zero shell work here; the exec thread did that.
        #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
        timed_drain!("bridge_out", self.drain_bridge_output());
        // Braid decrypts the workers finished — commit before the arm loop so a gap-refill replay re-enters THIS tick's gates.
        timed_drain!("braid_rx", self.drain_braid_rx());
        // Send encrypts the workers finished — commit + hand to the durable-transmit writer.
        timed_drain!("braid_tx", self.drain_braid_tx());
        // Durable verdicts from the message writer — self rows flip bright + release their sibling push only HERE, after the disk confirmed (the zero-remote write-confirm-then-send law, 2026-08-21 erasure ticket).
        timed_drain!("persist_done", self.drain_persist_done());

        // Our OWN just-picked avatar, arriving from the off-thread set pipeline (decode ran there too): install + repaint, then drop the channel — one avatar per pick.
        if let Some(rx) = self.avatar_set_rx.as_ref() {
            if let Ok(px) = rx.try_recv() {
                self.device_avatar_pixels = Some(px);
                self.device_avatar_scaled = None;
                self.device_avatar_scaled_diameter = 0;
                self.scene_dirty = true;
                self.avatar_set_rx = None;
                crate::log("avatar picker: display pixels installed");
                self.refresh_self_row_avatar();
            }
        }

        // Clock sanity: drain any completed nunc verdict, then (if the wall clock has grossly jumped since the last baseline) spawn a fresh re-check. Both are cheap — the jump check is two clock reads and a subtraction; a re-check only spawns on an actual jump.
        self.drain_clock_check();
        // Surface any fleet-inbox alerts pulled since the last tick (bind attempts on our devices).
        self.drain_fleet_inbox();
        if self.online && self.clock_jump.check_and_reset() {
            crate::log("Clock: wall clock jumped — re-verifying via nunc consensus");
            #[cfg(not(target_os = "android"))]
            if let Some(proxy) = self.event_proxy.clone() {
                crate::network::spawn_clock_check(self.clock_check_tx.clone(), Some(proxy));
            }
            #[cfg(target_os = "android")]
            crate::network::spawn_clock_check(self.clock_check_tx.clone(), None);
        }
        // Avatar acquisition policy (once/session/contact). A MUTUAL contact (CLUTCH Complete, which is impossible unless both added each other) gets a direct P2P AvatarRequest — a friend's avatar comes from the friend. We fall back to FGTW for that friend ONLY if no AvatarResponse has installed an avatar within AVATAR_P2P_FALLBACK_OSC (the friend is offline or avatar-less). A non-mutual contact never gets a direct request — it only ever pulls the public FGTW copy. Never blocks; each branch is dedup'd so the per-tick sweep is cheap.
        /// ~3 seconds (oscillations) before a mutual peer's silent P2P request falls back to FGTW.
        const AVATAR_P2P_FALLBACK_OSC: i64 = 3 * crate::OSC_PER_SEC;
        enum AvatarPlan {
            // Cached locally (the common launch case): just kick the local-first background load, which reads the vault and never touches the network. Keeps the P2P/FGTW escalation from firing a redundant request every launch when we already hold the avatar. `spawn_avatar_download`'s worker is cache-first, so this IS the "look local first" path — the caller states intent, the fetch layer serves it from the vault.
            LocalCached {
                ci: usize,
            },
            // Complete + addressable, NOT cached: try the peer directly; FGTW only after the timeout.
            P2pThenFgtw {
                peer_addr: std::net::SocketAddr,
                recipient_pubkey: [u8; 32],
                ci: usize,
            },
            // Non-mutual (or Complete-but-unaddressable) and not cached: public FGTW copy only.
            FgtwOnly {
                ci: usize,
            },
        }
        // Steady state: every contact already has an avatar → skip the sweep entirely (no timestamp read, no allocation) since this runs every tick. Only do the work when something's missing.
        if self.contacts.iter().any(|c| c.avatar_pixels.is_none()) {
            let now = vsf::eagle_time_oscillations();
            // Probe cache taken out for the closure (contacts holds the self borrow), restored after. The vault read behind a miss takes the VAULT MUTEX that persist workers hold thru whole table writes — per-tick probing turned every background write into a UI stall (2026-08-15 "status pre-loop" 400-1794ms). One probe per contact per session; avatar installs / pin adoptions clear the map.
            let mut probe_cache = std::mem::take(&mut self.avatar_probe_cache);
            let plans: Vec<AvatarPlan> = self
                .contacts
                .iter()
                .enumerate()
                .filter(|(_, c)| c.avatar_pixels.is_none())
                .map(|(ci, c)| {
                    // Local vault first — remembered from the one probe. If we have it, the network never runs. This is what stops the every-launch redundant P2P request: the friend's avatar is already cached, so we don't re-ask them for it.
                    let cached = *probe_cache.entry(c.handle_hash).or_insert_with(|| {
                        self.storage.as_ref().is_some_and(|s| {
                            crate::ui::avatar::has_cached_avatar_from_seed(&c.handle_hash, s)
                        })
                    });
                    if cached {
                        return AvatarPlan::LocalCached { ci };
                    }
                    if c.is_mutual() {
                        if let (Some((addr, _alt)), Some(recipient_key)) = (c.race_addrs(), c.device_key()) {
                            return AvatarPlan::P2pThenFgtw {
                                peer_addr: addr,
                                recipient_pubkey: recipient_key,
                                ci,
                            };
                        }
                    }
                    AvatarPlan::FgtwOnly { ci }
                })
                .collect();
            for plan in plans {
                match plan {
                    // Cache-first background load; never hits the network for an already-cached avatar.
                    AvatarPlan::LocalCached { ci } => self.spawn_avatar_download(ci),
                    AvatarPlan::FgtwOnly { ci } => self.spawn_avatar_download(ci),
                    AvatarPlan::P2pThenFgtw {
                        peer_addr,
                        recipient_pubkey,
                        ci,
                    } => match self.avatar_req_pending.get(&recipient_pubkey).copied() {
                        // Never asked this peer — send the P2P request now, record when.
                        None => {
                            self.spawn_avatar_request_p2p(peer_addr, recipient_pubkey, now);
                        }
                        // Asked, but the peer hasn't answered within the window — fall back to FGTW (dedup'd by avatar_dl_started, so this fires at most once per peer).
                        Some(sent_at) if now.saturating_sub(sent_at) > AVATAR_P2P_FALLBACK_OSC => {
                            self.spawn_avatar_download(ci);
                        }
                        // Asked recently — still waiting on the peer; do nothing this tick.
                        Some(_) => {}
                    },
                }
            }
            self.avatar_probe_cache = probe_cache;
        } // end avatar sweep (skipped when every contact already has an avatar)

        let checker = match &self.status_checker {
            Some(c) => c,
            None => return false,
        };

        // Our party id for CLUTCH: the identity PUBKEY (the value contacts pin at first-met). It rides CLUTCH offers for contact matching — public by design; the secret identity binding is the friendship-secret egg, never this id. (Was the raw identity seed, which also parked our seed in every peer's contact row — docs/identity-profile.md.)
        let our_handle_hash = match self
            .session
            .as_ref()
            .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
        {
            Some(h) => h,
            None => return false, // Can't do CLUTCH without our party id
        };

        // Alias kept for the keygen-spawn call below (same value — the party id).
        let our_identity_seed = our_handle_hash;

        // Our device pubkey, hoisted for the sibling party-id shadow inside the contacts loop (a &self method call there would conflict with the &mut contacts borrow).
        let our_device_pubkey = match self.device_keypair.as_ref() {
            Some(kp) => *kp.public.as_bytes(),
            None => return false,
        };

        let mut changed = false;
        let mut ceremony_completions: Vec<usize> = Vec::new(); // Contact indices to complete after loop
                                                               // Deferred KEM decapsulation spawns (spawn_clutch_kem_decap needs &self; the loop holds contact borrows) — same deferral discipline as ceremony_completions.
        let mut decap_spawns: Vec<(
            crate::types::ContactId,
            crate::crypto::clutch::ClutchKemResponsePayload,
            crate::crypto::clutch::ClutchAllKeypairs,
        )> = Vec::new();
        let mut lan_ping_indices: Vec<usize> = Vec::new(); // Contact indices to ping immediately on new LAN discovery
                                                           // Collect pending message retransmit requests (friendship_id, ip, handle, device_pubkey, last_received_ef6) to process after loop last_received_ef6 from pong tells us what they already have - only retransmit newer
        let mut retransmit_requests: Vec<(
            crate::types::FriendshipId,
            std::net::SocketAddr,
            Option<std::net::SocketAddr>, // alt address to race (public/LAN counterpart)
            String,
            [u8; 32], // Recipient device pubkey for relay fallback
            Option<i64>,
        )> = Vec::new();
        // Flag to update sync records after the loop (when borrows are released)

        // Deferred probe-before-generate verdict (maybe_generate_s needs &mut self; the loop holds the checker borrow) — set when a blind_srv miss lands while S is None.
        let mut check_s_genesis = false;

        // Chain-weave probe deferrals — the loop holds an immutable `checker` borrow of `self`, so the `&mut self` seal/probe helpers can't run inline; collect contact indices and process them after the loop, like ceremony_completions / lan_ping_indices already do.
        let mut chain_seal_indices: Vec<usize> = Vec::new(); // seal_chain_if_ready after loop
        let mut chain_reset_apply: Vec<(usize, [u8; 32], bool)> = Vec::new(); // (contact idx, nonce, echo_back) from ChainResetReceived — applied after loop
        let mut chain_probe_indices: Vec<usize> = Vec::new(); // maybe_send_chain_probe after loop
                                                              // Parked-ceremony offer re-fires on a path-up edge (resend_clutch_offer needs &mut self) — same deferral discipline.
        let mut offer_refire_indices: Vec<usize> = Vec::new();
        // Fleet history sweep deferral: a sibling coming online means it may hold conversation rows we don't — arm the per-conversation walk after the loop (the sweep needs &mut contacts).
        let mut fleet_sweep_due = false;
        // Fleet-attention moves + claim drop-sweeps, deferred past the checker borrow (the drain can't take &mut self). attn_adopt keeps only the LWW winner across a multi-frame drain; attn_void records the offline-verdict holder (applied before adopt, so a live frame in the same drain still wins); heal_due fires the stored-osc re-announce at a sibling's online edge.
        let mut attn_adopt: Option<([u8; 32], i64)> = None;
        let mut attn_void: Option<[u8; 32]> = None;
        let mut claim_sweeps: Vec<([u8; 32], i64)> = Vec::new();
        let mut heal_due = false;
        // Conversations whose message table changed in an arm below — persisted AFTER the loop via the async writer (the arms hold a &mut contact borrow; the inline save_messages here was the named 600ms-5.7s UI stall).
        let mut persist_hashes: Vec<[u8; 32]> = Vec::new();
        // Friendships whose chains changed in a SAFE-to-delay way (ACK pending-removal, chain-sync adopt) — persisted AFTER the loop via the coalescing chains writer. The `checker` borrow spans the loop, so a &mut self method can't run inside it (the same deferral every arm here uses).
        let mut chains_persist_fids: Vec<crate::types::friendship::FriendshipId> = Vec::new();

        // The braid / strict-ordering replay queue: when a committed decrypt fills a hash-chain gap, commit_braid_rx minted the now-contiguous buffered frames as synthetic ChatMessage updates on chat_replay_queue — seeded here so they re-enter the arm's full gates BEFORE the next channel item (a refilled N+1 processes ahead of anything newer, and can itself cascade). FIFO front-drain.
        let mut replay_queue: std::collections::VecDeque<StatusUpdate> =
            std::mem::take(&mut self.chat_replay_queue);

        // Wi-Fi Direct platform events (Kotlin/D-Bus up-calls, queued off-thread): converted here to StatusUpdates so they ride the same arms as everything else. Token matching happens here because it needs the contacts.
        for ev in crate::network::wfd::drain_events() {
            match ev {
                crate::network::wfd::WfdEvent::ServiceFound(txt) => {
                    for c in &self.contacts {
                        if c.is_sibling || c.wfd_cred.is_none() {
                            continue;
                        }
                        let Some(seed) = c.relationship_seed.as_ref() else {
                            continue;
                        };
                        let mut devs = c.fleet_members.clone();
                        if let Some(d) = c.device_key() {
                            devs.push(d);
                        }
                        if let Some(dev) = crate::network::wfd::match_txt_tokens(
                            &txt,
                            seed.as_bytes(),
                            &devs,
                        ) {
                            replay_queue
                                .push_back(StatusUpdate::WfdFriendNearby { device_pubkey: dev });
                            break;
                        }
                    }
                }
                crate::network::wfd::WfdEvent::GroupChanged {
                    formed,
                    is_go,
                    our_ip,
                    go_ip,
                } => {
                    replay_queue.push_back(if formed {
                        StatusUpdate::WfdGroupUp {
                            is_go,
                            our_ip,
                            go_ip,
                        }
                    } else {
                        StatusUpdate::WfdGroupDown
                    });
                }
                crate::network::wfd::WfdEvent::OpenHouseHeard { ssid, psk } => {
                    // Gated + tie-broken inside (only meaningful while OUR open house is armed too).
                    crate::network::wfd::open_house_join(&ssid, &psk);
                }
            }
        }

        // Off-thread woods-add PoW finished → arm the beacon matcher.
        if let Some(rx) = self.woods_add_rx.as_ref() {
            if let Ok((handle, hp)) = rx.try_recv() {
                crate::logf!("add-friend: proof derived for the off-grid add of {}", handle);
                self.pending_woods_add = Some((handle, hp));
                self.woods_add_rx = None;
            }
        }

        // Per-ARM stall attribution. PERF already showed check_status_updates blocking the UI thread for seconds, but not WHICH update type — this names it. Timed from loop top to loop top (arms `continue`, so an end-of-body probe would be skipped); the post-loop check closes the last iteration.
        fn arm_label(u: &StatusUpdate) -> &'static str {
            match u {
                StatusUpdate::Online { .. } => "Online",
                StatusUpdate::ChatMessage { .. } => "ChatMessage",
                StatusUpdate::ChainResetReceived { .. } => "ChainResetReceived",
                StatusUpdate::PongSealMissing { .. } => "PongSealMissing",
                StatusUpdate::ChainSyncReceived { .. } => "ChainSyncReceived",
                StatusUpdate::CkptRootReceived { .. } => "CkptRootReceived",
                StatusUpdate::CkptReqReceived { .. } => "CkptReqReceived",
                StatusUpdate::ChainPullReceived { .. } => "ChainPullReceived",
                StatusUpdate::ChainPullMissReceived { .. } => "ChainPullMissReceived",
                StatusUpdate::DepartReqReceived { .. } => "DepartReqReceived",
                StatusUpdate::FocusClaimReceived { .. } => "FocusClaimReceived",
                StatusUpdate::FriendKnockReceived { .. } => "FriendKnockReceived",
                StatusUpdate::AttentionReceived { .. } => "AttentionReceived",
                StatusUpdate::CkptStateReceived { .. } => "CkptStateReceived",
                StatusUpdate::AttachBlobReceived { .. } => "AttachBlobReceived",
                StatusUpdate::AttachProgress { .. } => "AttachProgress",
                StatusUpdate::AttachHaveReceived { .. } => "AttachHaveReceived",
                StatusUpdate::AttachReqReceived { .. } => "AttachReqReceived",
                StatusUpdate::MessageAck { .. } => "MessageAck",
                StatusUpdate::AvatarRequestReceived { .. } => "AvatarRequestReceived",
                StatusUpdate::AvatarReceived { .. } => "AvatarReceived",
                StatusUpdate::HistoryRequestReceived { .. } => "HistoryRequestReceived",
                StatusUpdate::HistoryPageReceived { .. } => "HistoryPageReceived",
                StatusUpdate::BlindFrameReceived { .. } => "BlindFrameReceived",
                StatusUpdate::PTReceived { .. } => "PTReceived",
                StatusUpdate::PTSendComplete { .. } => "PTSendComplete",
                StatusUpdate::ClutchOfferReceived { .. } => "ClutchOfferReceived",
                StatusUpdate::ClutchKemResponseReceived { .. } => "ClutchKemResponseReceived",
                StatusUpdate::ClutchCompleteReceived { .. } => "ClutchCompleteReceived",
                StatusUpdate::LanPeerDiscovered { .. } => "LanPeerDiscovered",
                StatusUpdate::OurLanAddrObserved { .. } => "OurLanAddrObserved",
                StatusUpdate::ReflexiveLearned { .. } => "ReflexiveLearned",
                StatusUpdate::PathValidated { .. } => "PathValidated",
                StatusUpdate::WfdFriendNearby { .. } => "WfdFriendNearby",
                StatusUpdate::WfdGroupUp { .. } => "WfdGroupUp",
                StatusUpdate::WfdGroupDown => "WfdGroupDown",
                StatusUpdate::WfdCredReceived { .. } => "WfdCredReceived",
            }
        }
        {
            let ms = preloop_t.elapsed().as_millis();
            if ms > 100 {
                crate::logf!("PERF: status pre-loop took {}ms (UI thread)", ms as u64);
            }
        }
        let mut arm_timer: Option<(&'static str, std::time::Instant)> = None;
        // Per-PASS accounting beside the per-arm timer: the 2026-08-11 desktop showed 1.8s passes with ZERO arms over 100ms — death by hundreds of moderate updates, invisible to a threshold that only names single offenders. The profile line names the cumulative eaters; the budget below bounds the stall.
        let pass_start = std::time::Instant::now();
        let mut pass_updates: u32 = 0;
        let mut pass_profile: std::collections::HashMap<&'static str, (u32, u128)> =
            std::collections::HashMap::new();
        macro_rules! close_arm_timer {
            () => {
                if let Some((label, t)) = arm_timer.take() {
                    let ms = t.elapsed().as_millis();
                    let slot = pass_profile.entry(label).or_insert((0, 0));
                    slot.0 += 1;
                    slot.1 += ms;
                    if ms > 100 {
                        crate::logf!(
                            "PERF: status arm {} took {}ms (UI thread)",
                            label,
                            ms as u64
                        );
                    }
                }
            };
        }

        // Friendships whose send lane the wedge heal rotated this pass — persisted and row-flushed after the drain loop, where &mut self is available again.
        let mut rotated_flush: Vec<crate::types::friendship::FriendshipId> = Vec::new();
        // Sender-side re-serve jobs (Nick's go, 2026-08-20): (contact idx, rows as (eagle_time, content, reference)) collected during the drain — chain_transmit needs &mut self, so execution waits for the checker borrow to release, same as rotated_flush.
        let mut reserve_jobs: Vec<(
            usize,
            (crate::types::friendship::FriendshipId, [u8; 32]),
            Vec<(i64, String, Option<(crate::types::RefKind, i64)>)>,
        )> = Vec::new();
        // Consent gate (2026-08-25): knocks to fire and the roster ride for a Mutual flip — both need &mut self, so they wait out the drain like the jobs above.
        let mut knock_after: Vec<crate::types::ContactId> = Vec::new();
        // chain_pull request/miss events, deferred past the checker borrow (their handling mutates watermarks / re-keys).
        let mut chain_pull_reqs_after: Vec<([u8; 32], [u8; 32])> = Vec::new();
        let mut chain_pull_misses_after: Vec<([u8; 32], [u8; 32])> = Vec::new();
        // Sibling departure requests (bilateral removal), deferred past the checker borrow.
        let mut depart_reqs_after: Vec<(i64, Vec<u8>, [u8; 32])> = Vec::new();
        // Wi-Fi Direct credential provisioning, collected on came-online edges (after releasing the checker borrow): the elected-GO side mints/re-offers the pair's group credential once per session (docs/offgrid.md).
        // Wi-Fi Direct beacon answer-back target, collected on the p2p learn edge (the reply teaches the peer OUR group address).
        let mut wfd_beacon_reply: Option<std::net::SocketAddr> = None;
        let mut consent_roster_push = false;
        // Complete-without-chains probe (field 2026-08-26, Emma's post-wipe Frankenstein): tokens whose inbound frames found NO friendship chains — checked post-drain against the contact's claimed ceremony state.
        let mut rekey_probe: Vec<[u8; 32]> = Vec::new();
        loop {
            // TIME BUDGET — the UI thread's stall is bounded whatever the storm size: past 250ms the rest of the backlog waits for the next tick (the channel holds it; un-replayed synthetic frames go back on chat_replay_queue below, order preserved). Unbounded, a churny catch-up pass measured 1.8s on the desktop — taps landed but nothing painted until the drain yielded (2026-08-11).
            if pass_start.elapsed().as_millis() > 250 {
                crate::logf!("PERF: status pass hit the 250ms budget after {} update(s) — deferring the rest to the next tick", pass_updates);
                break;
            }
            let update = match replay_queue.pop_front() {
                Some(u) => u,
                None => match checker.try_recv() {
                    Some(u) => u,
                    None => break,
                },
            };
            // OWN-FRAME GUARD — the one seam every receive path funnels thru. A frame we authored can loop back (relay echo, LAN multicast loopback, a send aimed at an endpoint already poisoned to our own address), and the arms below adopt endpoints/addresses/liveness from whatever sender they see: one echoed offer elected US the sibling's active device, every later send aimed at ourselves, and the ceremony parked for a day ringing its own doorbell (field, 2026-08-12). Nothing a device tells ITSELF over the network is information — drop it before any arm can act on it.
            if let (Some(sender), Some(kp)) = (update.sender_device(), self.device_keypair.as_ref())
            {
                if sender == kp.public.as_bytes() {
                    crate::logf!(
                        "ECHO: own {} frame back at us (relay echo / LAN loopback) — dropped",
                        arm_label(&update)
                    );
                    continue;
                }
            }
            pass_updates += 1;
            close_arm_timer!();
            arm_timer = Some((arm_label(&update), std::time::Instant::now()));
            match update {
                StatusUpdate::Online {
                    peer_pubkey,
                    is_online,
                    peer_addr,
                    sync_records,
                    display_name,
                    avatar_pin,
                    locked_reports,
                    about,
                } => {
                    // Stall recovery (runs EVERY ping that carries sync records, not just the offline→online edge): each record advertises the peer's contiguous head. Re-arm any pending of ours newer than the head for OUR lane AND already given up (exhausted attempts) — so a gap-filler the sender abandoned gets resent and a receiver stuck behind a permanently-lost message un-sticks. The staleness gate stays (a fresh send is left to normal backoff; only a given-up one is revived), which keeps a pong that merely raced ahead of the ACK from double-sending. collect_due_retransmits (the tick path) then actually sends the revived messages.
                    let now_osc = vsf::eagle_time_oscillations();
                    // Lanes rotated by the wedge heal below — flushed AFTER the record loop (the loop holds the chains borrow; resend_held_messages needs &mut self).
                    let mut rotated_fids: Vec<crate::types::friendship::FriendshipId> = Vec::new();
                    for record in &sync_records {
                        if let Some((fid, chains)) = self
                            .friendship_chains
                            .iter_mut()
                            .find(|(_, c)| c.conversation_token == record.conversation_token)
                        {
                            // The peer's head for the lane WE send on — exact when it carries per-lane heads, the max-across-lanes tip for a legacy peer. Absence of our lane among non-empty heads = they've received nothing on it → send from the anchor (tip 0).
                            let tip = if record.lane_heads.is_empty() {
                                record.last_received_osc
                            } else {
                                chains
                                    .our_label()
                                    .and_then(|l| {
                                        record
                                            .lane_heads
                                            .iter()
                                            .find(|(lab, _)| lab == l)
                                            .map(|(_, t)| *t)
                                    })
                                    .unwrap_or(0)
                            };
                            // LANE WEDGE HEAL — checked BEFORE stall recovery, because the wedge's terminal loop was exactly that re-arm: give-up → "past peer lane tip 0" revive → give-up, forever (2,913 retransmit lines in one overnight log, 2026-08-09, every frame gap-buffered by a peer that lost the lane state and expects the anchor — unlinkable by hash AND undecryptable at their position-0 key). Rotation retires the dead lane and its pendings; the post-loop flush re-serves every undelivered row thru chain_transmit on the fresh lane at the ORIGINAL eagle_times, so row identity — and history — converges instead of forking.
                            if chains.our_lane_wedged_at_peer_anchor(tip) {
                                if let Some((dead, fresh, retired)) = chains.rotate_our_lane() {
                                    crate::logf!("LANE: peer at the ANCHOR of {}... with {} unlinkable exhausted pending(s) — rotated to {}..., re-serving undelivered rows on the fresh lane", hex::encode(&dead[..4]), retired, hex::encode(&fresh[..4]));
                                    rotated_fids.push(*fid);
                                }
                            }
                            // Tip testimony clears BEFORE stall recovery re-arms: pendings at/below the peer's contiguous lane head are received-in-order facts, not in-flight sends — un-re-ACKable pre-fix rows sat there wedging the window shut against the very send whose ACK could have implied them away.
                            let cleared = chains.clear_pending_up_to(tip);
                            if cleared > 0 {
                                crate::logf!("CHAT: {} pending(s) at/below peer lane tip {} — implied-delivered by the sync record", cleared, tip);
                            }
                            let n = chains.rearm_pending_after(tip, now_osc);
                            if n > 0 {
                                crate::logf!("CHAT: re-armed {} given-up pending msg(s) past peer lane tip {} (stall recovery)", n, tip);
                                // STUCK-TIP WEDGE — the anchor wedge in disguise (round-9 field, 2026-08-17): the peer's head for our lane is NONZERO but never moves (its lane stalled exactly where ours wedged; the rows above it reached the fleet as forwards nobody can re-ACK), so exhaust → re-arm → exhaust loops forever below the tip-0 detector. Two FULL retry ladders re-armed at the very same advertised head = the peer provably cannot advance past it; rotate, and the re-serve converges rows by identity exactly like the anchor heal.
                                let ladders = {
                                    let e = self.lane_rearm_cycles.entry(*fid).or_insert((tip, 0));
                                    if e.0 == tip {
                                        e.1 = e.1.saturating_add(1);
                                    } else {
                                        *e = (tip, 1);
                                    }
                                    e.1
                                };
                                if ladders >= 2 {
                                    if let Some((dead, fresh, retired)) = chains.rotate_our_lane() {
                                        crate::logf!("LANE: peer's head for our lane STUCK at {} thru {} exhaust→re-arm ladders ({} pending(s)) — rotated {}... to {}..., re-serving on the fresh lane", tip, ladders, retired, hex::encode(&dead[..4]), hex::encode(&fresh[..4]));
                                        rotated_fids.push(*fid);
                                        self.lane_rearm_cycles.remove(fid);
                                    }
                                }
                            }
                            // ANTI-ENTROPY: the pong carries the peer's (row_count, XOR-fold) for this conversation. A digest mismatch means the two sides provably hold DIFFERENT message sets — the heuristic cursor walk left a hole (the greyed sends a peer never got, 2026-07-25) — so force a FULL recovery walk (early-stop disabled). Zero count+digest = legacy peer, no comparison. Cooldown per contact so a persistent mismatch (peer can't serve) re-fires at a polite cadence instead of every pong.
                            if record.row_count != 0 || record.row_digest != [0u8; 32] {
                                let fid = *fid;
                                // No chain_woven gate here: the wedge/rotation era leaves woven flags stale while the HISTORY KEY (the actual page-seal capability, checked at the route) is present — and gating the digest kick on woven left a receiving-fine device 100 rows behind with its walk never arming (field, 2026-08-11: 8 rows vs the peer's advertised 109, zero pull attempts all session).
                                let ci = self
                                    .contacts
                                    .iter()
                                    .position(|c| c.friendship_id == Some(fid))
                                    .filter(|&ci| !self.contacts[ci].is_sibling);
                                // Field-precise conversation lookup — `chains` above holds a borrow of `friendship_chains`, so no &mut self method fits here.
                                if let Some(ci) = ci {
                                    let cid = self.contacts[ci].conversation(&our_handle_hash).id();
                                    if let Some(conv) =
                                        self.conversations.iter_mut().find(|v| v.id() == cid)
                                    {
                                        // SAME digest the producer publishes — the cached, order-dependent rolling hash (Conversation::anti_entropy_digest). Both sides MUST compute it identically or every comparison false-mismatches.
                                        let (n_rows, digest) = conv.anti_entropy_digest();
                                        // REPLICATED edge (delivery ladder): a SIBLING pong whose (count, digest) EXACTLY matches ours is testimony that device holds every syncable row of this conversation — flip the fleet-replication tick on our outgoing rows. Runtime state; delivered outranks it, and a friend's matching pong proves nothing about our fleet so it's sibling-gated.
                                        if n_rows == record.row_count
                                            && digest == record.row_digest
                                            && self.contacts.iter().any(|c| {
                                                c.is_sibling
                                                    && c.device_key() == Some(peer_pubkey.key)
                                            })
                                        {
                                            for m in conv
                                                .messages
                                                .iter_mut()
                                                .filter(|m| m.is_outgoing && !m.replicated)
                                            {
                                                m.replicated = true;
                                            }
                                        }
                                        // A history walk PULLS rows FROM the peer, so it can only help when the peer has rows WE lack. When we already hold MORE than the peer (n_rows > theirs), a pull returns 0 new every time — it cannot deliver our extra row to THEM. That was a permanent loop: one undelivered message we hold (a lane wedged at the peer's anchor) kept ours = theirs+1 forever, re-walking every 120s and decrypting pages on the UI thread (the 2026-08-08 typing lag). Only walk when the peer is at-least-even (they may have rows we're missing, or an equal-count content divergence a walk can reconcile). When we're strictly ahead the fix is on the delivery side (re-serve / lane repair), not a pull.
                                        let we_might_be_behind = record.row_count >= n_rows;
                                        let mismatch = (n_rows != record.row_count
                                            || digest != record.row_digest)
                                            && we_might_be_behind;
                                        const DIGEST_KICK_COOLDOWN_OSC: i64 =
                                            120 * vsf::OSCILLATIONS_PER_SECOND as i64;
                                        let idle = conv
                                            .history_recovery
                                            .as_ref()
                                            .map_or(true, |r| r.complete);
                                        if mismatch
                                            && idle
                                            && now_osc
                                                .saturating_sub(self.contacts[ci].digest_kick_osc)
                                                > DIGEST_KICK_COOLDOWN_OSC
                                        {
                                            self.contacts[ci].digest_kick_osc = now_osc;
                                            crate::logf!("HISTORY: digest mismatch with {} (ours {} rows, theirs {}) — full resync walk", crate::fp(&self.contacts[ci].handle_proof), n_rows, record.row_count);
                                            conv.history_recovery =
                                                Some(crate::types::HistoryRecovery {
                                                    oldest_recovered_osc: i64::MAX,
                                                    complete: false,
                                                    in_flight: None,
                                                    next_request_osc: 0,
                                                    urgent: true,
                                                    was_complete_before: false,
                                                    decrypt_fail_streak: 0,
                    expire_streak: 0,
                                                    parked_key_fp: None,
                                                });
                                        }
                                        // SENDER-SIDE RE-SERVE (Nick's go, 2026-08-20) — the delivery-side fix the pull-gate comment above promises. The pending list only retransmits rows it still holds; a row implied-delivered by a FLEET ack that THIS peer device never received leaves their lane wedged forever — they gap-buffer every later row (twice on 2026-08-20 that hostage was a call ANSWER). The sealed tip is per-device cryptographic testimony of what they hold contiguously, and their row_count deficit is content-level evidence they lack rows — so when WE are strictly ahead, re-serve the oldest rows above the tip from the DURABLE store at their ORIGINAL stamps (the lane-rotation flush's proven semantics). The receiver's row-store dedup absorbs anything it already had and Re-ACKs it, clearing the fresh pending; a genuinely missing row processes normally and un-jams the gap cascade.
                                        if n_rows > record.row_count {
                                            // Cap = rows TRANSMITTED per testimony, not bursts attempted (2026-09-01): the serial-send gate lets ~1 row out per burst, so a burst-attempt cap of 2 parked any hole deeper than ~2 rows forever ('re-serving 8 → re-served 1' in the field log). 16 covers the deficit window; the deferred loop below charges actual transmits.
                                            const RESERVE_ROWS_PER_TESTIMONY: u8 = 16;
                                            const RESERVE_ROWS_PER_BURST: usize = 8;
                                            // Convergence rule (field 2026-08-21, the hours-long loop): the spend is against the peer device's EXACT testimony (tip, row_count, row_digest), and only a testimony CHANGE re-arms it — re-serving into unchanged testimony is evidence the peer already holds (and dedups) the rows, so the count deficit is a counting divergence, not a delivery hole. Device-keyed because sibling devices pong different lane views; the old fid-keyed tip slot reset on every alternation.
                                            let allowed = {
                                                let e = self
                                                    .lane_reserve_bursts
                                                    .entry((fid, peer_pubkey.key))
                                                    .or_insert((
                                                        tip,
                                                        record.row_count,
                                                        record.row_digest,
                                                        0,
                                                    ));
                                                if (e.0, e.1, e.2)
                                                    != (tip, record.row_count, record.row_digest)
                                                {
                                                    *e = (
                                                        tip,
                                                        record.row_count,
                                                        record.row_digest,
                                                        0,
                                                    );
                                                }
                                                if e.3 < RESERVE_ROWS_PER_TESTIMONY {
                                                    true
                                                } else {
                                                    if e.3 == RESERVE_ROWS_PER_TESTIMONY {
                                                        e.3 += 1;
                                                        crate::logf!("CHAT: re-serve cap spent at tip {} with peer testimony unchanged (theirs {} rows vs ours {}) — parked until their sync record moves", tip, record.row_count, n_rows);
                                                    }
                                                    false
                                                }
                                            };
                                            if allowed {
                                                // LIVE pendings only (2026-09-01, the head-gap last boss): a pending that exhausted MAX_SEND_ATTEMPTS never retransmits again — it sits in the list solely so a late ACK can clear it — yet the old whole-list exclusion treated it as "already retransmitting" and hid exactly the head row the peer's in-order gate was starving on (the MacBook's ringing-answered-silent call: the ANSWER buffered behind one exhausted pending forever). Given-up rows are re-servable; live ones still are not (their ladder covers them).
                                                let pending_times: std::collections::HashSet<i64> =
                                                    chains
                                                        .pending_messages
                                                        .iter()
                                                        .filter(|m| m.attempts < crate::types::friendship::MAX_SEND_ATTEMPTS)
                                                        .map(|m| m.eagle_time)
                                                        .collect();
                                                // Oldest-first: the OLDEST missing row is the one holding the peer's in-order gate shut; later holes fill on subsequent tip observations. LIVE pending rows are excluded (they already retransmit); deleted and friend-recovered rows never re-serve.
                                                let mut rows: Vec<(
                                                    i64,
                                                    String,
                                                    Option<(crate::types::RefKind, i64)>,
                                                )> = conv
                                                    .messages
                                                    .iter()
                                                    .filter(|m| {
                                                        m.is_outgoing
                                                            && !m.recovered
                                                            && !m.deleted
                                                            && m.timestamp > tip
                                                            && !pending_times.contains(&m.timestamp)
                                                    })
                                                    .map(|m| {
                                                        (
                                                            m.timestamp,
                                                            m.content.clone(),
                                                            m.reference,
                                                        )
                                                    })
                                                    .collect();
                                                rows.sort_by_key(|(t, _, _)| *t);
                                                rows.truncate(RESERVE_ROWS_PER_BURST);
                                                if !rows.is_empty() {
                                                    crate::logf!("CHAT: peer {} row(s) behind with our lane tip {} — re-serving {} row(s) from the durable store (the pending list called them delivered)", n_rows - record.row_count, tip, rows.len());
                                                    reserve_jobs.push((ci, (fid, peer_pubkey.key), rows));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // Rotated lanes flush AFTER the drain loop (the loop pins `checker` = &self.status_checker, so no &mut self method fits anywhere in this arm — same constraint the digest branch documents above).
                    rotated_flush.extend(rotated_fids);
                    // LOCKOUT gate: a locked device is still a fold member, so knows_device would happily honour its pong — presence, addresses, relay flags, all of it. Refuse the frame at the door instead; the lockout is precisely "stop listening".
                    if self.is_locked_device(&peer_pubkey.key) {
                        crate::logf!(
                            "Status: frame from LOCKED-OUT device {} — refused",
                            crate::fp(&peer_pubkey.key)
                        );
                        continue;
                    }
                    // §4.2 snapshot: taken per-update so verdicts drained earlier this pass are already reflected.
                    let our_device_pk = Some(our_device_pubkey);
                    let siblings = sibling_presence_snapshot(&self.contacts);
                    // Find matching contact and update status
                    for contact in &mut self.contacts {
                        if contact.knows_device(&peer_pubkey.key) {
                            // REPORTED-STOLEN intake (device-trust-and-recovery.md): the pong's sealed tail names the peer fleet's locked devices, and ONE report from a trusted fold member suffices. The authorization lives at lock CREATION, not here: writing fleet.locked demands the handle at a fresh attest, so a stock-client thief can't mint a false report — and a locked device's stale fleet key can't even read current fstate, so its reportable set is frozen at empty. The key-extraction tier defeats any threshold anyway (it can plant a fresh fold member as a second voucher — the trust doc's own observation), so requiring two only stranded the commonest fleet: two devices, one stolen, one survivor. A reporter still never counts toward refusing itself. Monotonic and persisted.
                            if !contact.is_sibling {
                                for reported in &locked_reports {
                                    let pair = (peer_pubkey.key, *reported);
                                    if !contact.locked_reports_seen.contains(&pair) {
                                        contact.locked_reports_seen.push(pair);
                                    }
                                    if contact.refused_devices.contains(reported) {
                                        continue;
                                    }
                                    let reporters = contact
                                        .locked_reports_seen
                                        .iter()
                                        .filter(|(rep, dev)| {
                                            dev == reported
                                                && rep != reported
                                                && !contact.refused_devices.contains(rep)
                                        })
                                        .map(|(rep, _)| *rep)
                                        .collect::<std::collections::HashSet<_>>();
                                    if !reporters.is_empty() {
                                        contact.refused_devices.push(*reported);
                                        changed = true;
                                        crate::logf!("FRIEND-REFUSE: {} device {} refused — reported stolen by {} fleet device(s)", crate::fp(&contact.handle_proof), crate::fp(reported), reporters.len());
                                        if let Some(storage) = self.storage.as_ref() {
                                            let _ = crate::storage::contacts::save_contact(
                                                contact, storage,
                                            );
                                        }
                                    }
                                }
                                // UN-REFUSE — the reversal half the intake never had: an UNLOCK (handle-confirmed at a fresh attest, worker-authoritative) clears the peer fleet's fleet.locked, but a friend who ingested the report kept refusing FOREVER — the 2026-08-16 field wedge: a lock-test on one device left it permanently deaf at every friend (pings silently dropped, contact stuck offline) with no path back. This pong is the SAME trusted testimony channel the refusal arrived on, so its locked set is authoritative for THIS reporter now: its stale (reporter, device) pairs retract, and a refused device is kept only while a standing reporter remains. Crucially the report ledger is RAM-ONLY while refused_devices persists — a restart ORPHANS every refusal (zero recorded reporters; the first un-refuse shipped gated on pair-removal and never fired in the field, round-2 soak) — so an orphaned refusal yields to this pong's live word directly: trusted fold member, current locked set, device absent → restored. Thief-safe: a locked/refused device's own pongs are dropped at the door (knows_device is false for it), so retraction testimony can only come from ANOTHER trusted device of that fleet — exactly who an unlock speaks thru; and a still-locked device re-refuses on the next report-carrying pong.
                                contact.locked_reports_seen.retain(|(rep, dev)| {
                                    *rep != peer_pubkey.key || locked_reports.contains(dev)
                                });
                                let keep: Vec<[u8; 32]> = contact
                                    .refused_devices
                                    .iter()
                                    .copied()
                                    .filter(|dev| {
                                        let actively_reported = contact
                                            .locked_reports_seen
                                            .iter()
                                            .any(|(rep, d)| d == dev && rep != dev);
                                        actively_reported || locked_reports.contains(dev)
                                    })
                                    .collect();
                                if keep.len() != contact.refused_devices.len() {
                                    let dropped: Vec<[u8; 32]> = contact
                                        .refused_devices
                                        .iter()
                                        .copied()
                                        .filter(|d| !keep.contains(d))
                                        .collect();
                                    contact.refused_devices = keep;
                                    changed = true;
                                    for dev in &dropped {
                                        crate::logf!("FRIEND-UNREFUSE: {} device {} restored — no standing reporter and {}'s current locked set doesn't name it (unlock propagated)", crate::fp(&contact.handle_proof), crate::fp(dev), crate::fp(&peer_pubkey.key));
                                    }
                                    if let Some(storage) = self.storage.as_ref() {
                                        let _ = crate::storage::contacts::save_contact(
                                            contact, storage,
                                        );
                                    }
                                }
                            }
                            // Party-id seam: sibling offers/tokens key on the device-derived pid, not the (shared) identity seed.
                            let our_handle_hash = if contact.is_sibling {
                                crate::crypto::clutch::sibling_party_id(&our_device_pubkey)
                            } else {
                                our_handle_hash
                            };
                            // Note: ceremony_id is now computed from offer_provenances, not ping provenances. Offer provenances are collected when ClutchOfferReceived messages arrive.

                            // Relay vs direct is the FIRST question a pong answers, and now it's answered from ground truth: a pong injected off the pipe carries the RELAY_ADDR sentinel because the pipe task already verified its authenticated relay envelope (peel_relay_envelope).
                            // A relayed pong proves the peer is reachable — but ONLY via the relay — so mark the link relay-only (→ lime-yellow) and DO NOT learn its address (storing 0.0.0.0:0 as an endpoint would poison direct sends, and a relayed pong carries no reachable address anyway). A direct pong clears the flag: a real UDP path always wins over the relay.
                            let via_relay = peer_addr == Some(crate::network::status::RELAY_ADDR);
                            // An UNSPECIFIED address (0.0.0.0 / ::) is never a reachable peer endpoint — it's the relay sentinel, OR a pong whose observed_addr echo is our own not-yet-learned reflexive (a sibling on a fresh device pongs back the 0.0.0.0 it saw). Adopting it as the contact's `ip` sends the next CLUTCH offer to 0.0.0.0 (a black hole), which is exactly why a freshly-paired sibling's weave never completes — the offer is fired at nowhere. Treat it like a relayed pong: proves liveness, carries no address to learn.
                            let addr_unspecified =
                                peer_addr.map_or(false, |a| a.ip().is_unspecified());
                            let learn_addr = !via_relay && !addr_unspecified;
                            // PER-DEVICE addressing: a DIRECT pong updates the SENDING device's endpoint (public/LAN split by source privacy), and only the ACTIVE device's pong may move the contact-level `ip`/`local_*` slot. A friend's other devices each keep their own endpoint — the old any-device-writes-the-one-slot rule made three-device fleets flip-flop the slot every cycle, which broke presence (pings chased the last ponger) AND cancelled mid-flight CLUTCH offer transfers ("address changed — cancelling"). First pong with no active device adopts the sender (bootstrap); inbound DATA (chat/CLUTCH) re-elects it (the device in their hand).
                            if let Some(addr) = peer_addr.filter(|_| learn_addr) {
                                let private = is_private_addr(&addr.ip());
                                {
                                    let ep = contact.endpoint_mut(&peer_pubkey.key);
                                    if private {
                                        ep.lan = Some(addr);
                                    } else {
                                        ep.public = Some(addr);
                                    }
                                }
                                if contact.active_device.is_none() {
                                    contact.active_device = Some(peer_pubkey.key);
                                }
                                if contact.active_device == Some(peer_pubkey.key) {
                                    if private {
                                        if let std::net::IpAddr::V4(v4) = addr.ip() {
                                            if contact.local_ip != Some(v4)
                                                || contact.local_port != Some(addr.port())
                                            {
                                                contact.local_ip = Some(v4);
                                                contact.local_port = Some(addr.port());
                                            }
                                        }
                                    } else if contact.ip != Some(addr) {
                                        crate::logf!("Status: Updated {} public IP from active-device pong: {} -> {}", crate::fp(&contact.handle_proof), format!("{:?}", contact.ip), addr);
                                        contact.ip = Some(addr);
                                    }
                                }
                            }
                            // Set the relay flag from THIS pong only when it's a positive report (is_online). A TIMEOUT flows thru this same arm with is_online=false and peer_addr=None — silence must not flip the flag either way; the last real pong's verdict stands until the next.
                            // A direct pong wins: if ANY device answered directly this cycle, the identity is direct (green). Relay-only when the only answer came over the pipe (yellow).
                            if is_online {
                                if via_relay {
                                    // Don't override a direct verdict already set this cycle by a sibling device's direct pong; only claim relay if we're not already known-direct-and-online.
                                    if !contact.is_online || contact.reached_via_relay {
                                        contact.reached_via_relay = true;
                                    }
                                } else {
                                    contact.reached_via_relay = false;
                                }
                            }

                            // Per-device About off the sealed tail — SIBLINGS ONLY, both directions (Nick's disclosure ruling 2026-08-31): we don't say it to friends, and we don't ADOPT it from a non-sibling either — a modified friend client volunteering an abt field changes nothing here.
                            if let Some(a) = about.as_ref().filter(|_| contact.is_sibling) {
                                if contact.device_about.as_deref() != Some(a.as_str()) {
                                    contact.device_about = Some(a.clone());
                                    changed = true;
                                }
                            }
                            // Always-granted name slot off the pong: adopt the friend's chosen display name. Persisted below via the state-save the name-change marks.
                            if let Some(name) = display_name.as_ref() {
                                if !contact.is_sibling && contact.published_name != *name {
                                    crate::logf!(
                                        "CONTACT: {} published name {} -> {}",
                                        crate::fp(&contact.handle_proof),
                                        format!("{:?}", contact.published_name),
                                        format!("{:?}", name)
                                    );
                                    contact.published_name = name.clone();
                                    contact.published_name_dirty = true;
                                    // Roster LWW clock: the published name is a synced identity field as of PRST4 — same rule as the pin adoption below.
                                    contact.roster_updated = vsf::eagle_time_oscillations();
                                    changed = true;
                                }
                            }
                            // Always-granted AVATAR slot off the pong: adopt the friend's avatar pin (random key ‖ lookup). This is the ONLY way a friend learns the pin — it's never handle-derivable — so it arrives here, gated by the pong only answering authenticated contacts. A new/changed pin marks the contact for a fresh avatar fetch.
                            if let Some(pin) = avatar_pin.as_ref() {
                                if !contact.is_sibling && contact.avatar_pin != *pin {
                                    crate::logf!(
                                        "CONTACT: {} avatar pin adopted",
                                        crate::fp(&contact.handle_proof)
                                    );
                                    contact.avatar_pin = *pin;
                                    contact.avatar_pin_dirty = true;
                                    // Roster LWW clock: the pin is a synced identity field, so this adoption must win the merge on every sibling (the post-drain sweep pushes the roster).
                                    contact.roster_updated = vsf::eagle_time_oscillations();
                                    changed = true;
                                }
                            }
                            // Per-device liveness: this pong/timeout is about the pinged DEVICE. The contact-level ring shows the IDENTITY reachable = any device online.
                            {
                                let ep = contact.endpoint_mut(&peer_pubkey.key);
                                ep.online = is_online;
                            }
                            // §4.2 takeover boot-race fix: a VERDICT landed for this contact — pong (is_online=true) or 3-consecutive-timeout (false, same arm). Until this is set, "owner absent" means nothing and takeover stays parked.
                            contact.presence_probed = true;
                            // Reachability clock: only the POSITIVE report counts (a TIMEOUT arrives thru this same arm with is_online=false — silence is exactly what the clock measures).
                            if is_online {
                                contact.last_heard = Some(std::time::Instant::now());
                                // They spoke: this contact matters right now, so collapse its presence backoff to the floor.
                                contact.ping_backoff = 0;
                            }
                            let identity_online = is_online || contact.any_device_online();
                            // True only on the offline→online EDGE, not every online ping/chat. Retransmit-of-pending (below) keys off this — without the edge gate it re-fired on every received chat (now that a chat marks the sender online), resending all pending messages in a storm.
                            let came_online = identity_online && !contact.is_online;
                            if contact.is_online != identity_online {
                                contact.is_online = identity_online;
                                changed = true;
                                crate::logf!(
                                    "Status: {} is now {} (device {} {})",
                                    crate::fp(&contact.handle_proof),
                                    if identity_online { "ONLINE" } else { "offline" },
                                    hex::encode(&peer_pubkey.key[..4]),
                                    if is_online { "up" } else { "down" }
                                );
                            }
                            // A sibling coming online is the fleet-history catch-up trigger: it may hold conversation rows written while we were apart. Deferred — the sweep needs &mut contacts. Also the heal edge: re-announce our STORED attention/claim so the reconnecting sibling sheds any stale holder beliefs.
                            if came_online && contact.is_sibling {
                                fleet_sweep_due = true;
                                heal_due = true;
                            }
                            // Presence VOIDS a dead holder's clearer claim (the design's crash/sleep coverage, no timers): the 3-strike offline verdict for the claim-holding device clears the claim, so the next friend message dings somewhere instead of being suppressed by a ghost. DROP-SWEEP (b): rows we suppressed under the now-dead claim are orphaned — chirp them (deferred).
                            if !is_online {
                                if let Some((tok, d, cosc)) = self.fleet_focus_claim {
                                    if d == peer_pubkey.key {
                                        self.fleet_focus_claim = None;
                                        crate::logf!(
                                            "FOCUS: clearer {} went offline — claim voided",
                                            crate::fp(&peer_pubkey.key)
                                        );
                                        claim_sweeps.push((tok, cosc));
                                    }
                                }
                                // The dead device can't hold the ball either — back to bootstrap until the human's next input somewhere.
                                if self
                                    .fleet_attention
                                    .map_or(false, |(d, _)| d == peer_pubkey.key)
                                {
                                    attn_void = Some(peer_pubkey.key);
                                }
                            }
                            // Bootstrap un-deadlock (docs/lifecycle.md aftermath, observed): a roster-merged contact starts with ONE bootstrap device (public_identity) as its ping target — if THAT device is asleep, pings chase a corpse forever while the friend's live devices sit in the fold. On the offline edge, rotate the ACTIVE device to the next fleet member with a known endpoint and retarget the contact-level address; the sweep pings it next cycle (round-robin until one answers — a pong or inbound DATA re-elects the real active device).
                            if !identity_online && !is_online {
                                let cur = contact.active_device;
                                let next = contact
                                    .device_endpoints
                                    .iter()
                                    .filter(|ep| {
                                        Some(ep.pubkey) != cur
                                            && (ep.public.is_some() || ep.lan.is_some())
                                    })
                                    .map(|ep| (ep.pubkey, ep.public, ep.lan))
                                    .next();
                                if let Some((pk, public, lan)) = next {
                                    crate::logf!("Status: active device {} unreachable — rotating to fleet member {}", cur.map(|d| hex::encode(&d[..4])).unwrap_or_default(), hex::encode(&pk[..4]));
                                    contact.active_device = Some(pk);
                                    if let Some(addr) = public {
                                        contact.ip = Some(addr);
                                    }
                                    if let Some(addr) = lan {
                                        if let std::net::IpAddr::V4(v4) = addr.ip() {
                                            contact.local_ip = Some(v4);
                                            contact.local_port = Some(addr.port());
                                        }
                                    }
                                    changed = true;
                                }
                            }

                            // Deadlock recovery: a queued KEM with no offer means their offer never arrived (lost in transit — their KEM landed but the larger offer transfer didn't). We can't derive ceremony_id or complete our slot without it, and there's no timeout on the queue, so this hangs Pending forever (only a restart, which forces a fresh offer exchange, recovers it). Self-heal: when we see a still-queued KEM on a pong AND we've already sent our offer (so we're genuinely stuck, not mid-initial-exchange), reset clutch_offer_sent so the offer-send block below re-fires this pong — our re-sent offer prompts them to re-send theirs (the same path a restart takes). Pong cadence rate-limits the re-request to one per pong. Only the "their offer was lost" case is recoverable here; if a peer genuinely never sends an offer, nothing we do fixes it.
                            if is_online
                                && contact.clutch_state == ClutchState::Pending
                                && contact.clutch_pending_kem.is_some()
                                && contact.clutch_offer_sent
                                && !ceremony_parked_by(contact, our_device_pk, &siblings)
                            {
                                crate::logf!("CLUTCH: still waiting for offer from {} (their KEM is queued) — re-requesting by re-sending our offer", crate::fp(&contact.handle_proof));
                                contact.clutch_offer_sent = false;
                            }

                            // THE CONSENT GATE at the wire (2026-08-25): no offer — no key material at all — leaves toward a contact who hasn't added us back. The knock travels instead, fired post-drain (the presence edge is the once-per-session re-knock trigger).
                            if is_online
                                && !contact.consent_mutual
                                && !contact.is_sibling
                                && !contact.knocked_session
                            {
                                knock_after.push(contact.id.clone());
                            }
                            // Send full offer when contact comes online and keys are ready Keys are pre-generated in background when contact is added Slot-based: send if Pending, have keypairs, haven't sent yet Note: ceremony_id is now computed AFTER offers are exchanged
                            if is_online
                                && contact.consent_mutual
                                && contact.clutch_state == ClutchState::Pending
                                && !contact.clutch_offer_sent
                                && !ceremony_parked_by(contact, our_device_pk, &siblings)
                            {
                                if let Some(ref keypairs) = contact.clutch_our_keypairs {
                                    use crate::network::fgtw::protocol::build_clutch_offer_vsf;
                                    use crate::network::status::ClutchOfferRequest;

                                    let payload =
                                        clutch::ClutchOfferPayload::from_keypairs(keypairs);

                                    // This is the send that a RELAYED pong triggers — and a relayed pong never sets `contact.ip`, so gating on it here meant the exact peer whose liveness the relay just proved could never receive our offer. The RELAY sentinel + relay_to fan-out is the delivery path that pong itself arrived on.
                                    {
                                        let ip = contact
                                            .ip
                                            .unwrap_or(crate::network::status::RELAY_ADDR);
                                        // Build VSF and capture our offer_provenance
                                        let conversation_token =
                                            clutch::derive_conversation_token(&[
                                                our_handle_hash,
                                                contact.handle_hash,
                                            ]);
                                        match build_clutch_offer_vsf(
                                            &conversation_token,
                                            &payload,
                                            self.device_keypair
                                                .as_ref()
                                                .expect("device_keypair set in init")
                                                .public
                                                .as_bytes(),
                                            self.device_keypair
                                                .as_ref()
                                                .expect("device_keypair set in init")
                                                .secret
                                                .as_bytes(),
                                            contact
                                                .clutch_round_started
                                                .unwrap_or_else(vsf::eagle_time_oscillations),
                                        ) {
                                            Ok((vsf_bytes, our_offer_provenance)) => {
                                                crate::logf!(
                                                    "CLUTCH: Sending full offer to {} (prov={}...)",
                                                    crate::fp(&contact.handle_proof),
                                                    hex::encode(&our_offer_provenance[..4])
                                                );

                                                // Store our offer provenance (for ceremony_id derivation)
                                                if !contact
                                                    .offer_provenances
                                                    .contains(&our_offer_provenance)
                                                {
                                                    contact
                                                        .offer_provenances
                                                        .push(our_offer_provenance);
                                                }

                                                // Persist provenance immediately
                                                if let Some(storage) = self.storage.as_ref() {
                                                    if let Err(e) =
                                                        crate::storage::contacts::save_clutch_slots(
                                                            &contact.clutch_slots,
                                                            &contact.offer_provenances,
                                                            contact.ceremony_id,
                                                            &contact.handle_hash,
                                                            storage,
                                                        )
                                                    {
                                                        crate::logf!("Failed to persist CLUTCH provenance: {}", e);
                                                    }
                                                }

                                                let (primary, alt) =
                                                    contact.race_addrs().unwrap_or((ip, None));
                                                checker.send_offer(ClutchOfferRequest {
                                                    peer_addr: primary,
                                                    alt_addr: alt,
                                                    vsf_bytes,
                                                    recipient_pubkey: contact.device_key().unwrap_or_default(), // unreachable-zero: this arm answers a frame-verified peer; Option-izing the request API is the follow-up
                                                    relay_to: contact.relay_device_list(),
                                                });
                                                contact.clutch_offer_sent = true;
                                                changed = true;
                                            }
                                            Err(e) => {
                                                crate::logf!(
                                                    "CLUTCH: Failed to build offer VSF: {}",
                                                    e
                                                );
                                            }
                                        }
                                    }
                                }
                            }

                            // Queue retransmit of pending messages only on the offline→online EDGE (not every online update) — otherwise every received chat would re-trigger a full pending resend.
                            if came_online {
                                if let (Some(fid), Some((primary, alt))) =
                                    (contact.friendship_id, contact.race_addrs())
                                {
                                    // Look up sync record for this friendship's conversation_token
                                    let last_received = if let Some((_, chains)) =
                                        self.friendship_chains.iter().find(|(id, _)| *id == fid)
                                    {
                                        sync_records
                                            .iter()
                                            .find(|r| {
                                                r.conversation_token == chains.conversation_token
                                            })
                                            .map(|r| r.last_received_osc)
                                    } else {
                                        None
                                    };
                                    retransmit_requests.push((
                                        fid,
                                        primary,
                                        alt,
                                        contact.display_name(),
                                        contact.device_key().unwrap_or_default(), // unreachable-zero: retransmit targets a woven friend
                                        last_received,
                                    ));
                                }
                            }

                            break;
                        }
                    }
                }
                // NOTE: ClutchOffer, ClutchInit, ClutchResponse, ClutchComplete handlers REMOVED Full 8-primitive CLUTCH uses ClutchOfferReceived and ClutchKemResponseReceived which are handled above (via TCP/PT transport).
                StatusUpdate::ChatMessage {
                    conversation_token,
                    lane,
                    prev_msg_hp,
                    ciphertext,
                    timestamp,
                    sender_addr,
                    sender_pubkey,
                } => {
                    // Get our handle_hash for chain lookups
                    let our_handle_hash = match self
                        .session
                        .as_ref()
                        .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
                    {
                        // PARTY ID: the friendship chain + slots are keyed on party ids (from_clutch + send both use our_party_id); matching on the raw seed here dropped every incoming message/probe as "not a participant" and hung the weave.
                        Some(h) => h,
                        None => {
                            crate::log("CHAT: No user_identity_seed - cannot decrypt");
                            continue;
                        }
                    };
                    // Sibling pid candidate, resolved BEFORE the chains borrow (a &self method call inside would conflict). Sibling chains carry the device-derived pid as our participant, not the identity seed.
                    let our_sibling_pid = self.our_sibling_pid();

                    // Find friendship by conversation_token
                    let chains_result = self
                        .friendship_chains
                        .iter_mut()
                        .find(|(_, c)| c.conversation_token == conversation_token);

                    if let Some((_, chains)) = chains_result {
                        // Party-id seam: our participant id is the identity PARTY id (friends) or the sibling pid (siblings) — whichever the chain actually holds. (The raw seed is never a chain participant post-pin-set.) The UNSHADOWED identity pid survives for the conversation resolution below.
                        let identity_hh = our_handle_hash;
                        let our_handle_hash = if chains.participants().contains(&our_handle_hash) {
                            our_handle_hash
                        } else if let Some(pid) =
                            our_sibling_pid.filter(|p| chains.participants().contains(p))
                        {
                            pid
                        } else {
                            crate::log("CHAT: we are not a participant in these chains");
                            continue;
                        };
                        // For 2-party chats, infer sender as the "other" participant
                        let from_handle_hash = match chains.other_participant(&our_handle_hash) {
                            Some(h) => *h,
                            None => {
                                crate::log("CHAT: Could not determine sender (not a 2-party chat or we're not a participant)");
                                continue;
                            }
                        };

                        // Materialize the SENDER'S LANE from root ‖ label (docs/lanes.md): any device holding the root decrypts any lane — receive-anywhere, no fold lookup, no trial decryption. A blob without a root predates lanes: the flag-day re-clutch is already sweeping it.
                        if chains.ensure_lane(&lane).is_none() {
                            crate::log("CHAT: frame for pre-lane chains (no root) — dropped; re-clutch re-mints");
                            continue;
                        }

                        // Find contact by their handle_hash
                        let contact_info = self.contacts.iter().enumerate().find_map(|(idx, c)| {
                            if c.handle_hash == from_handle_hash {
                                Some((idx, c.display_name()))
                            } else {
                                None
                            }
                        });

                        let (contact_idx, _handle) = match contact_info {
                            Some((idx, h)) => (idx, h),
                            None => {
                                crate::logf!(
                                    "CHAT: Contact not found for handle_hash {}...",
                                    hex::encode(&from_handle_hash[..8])
                                );
                                continue;
                            }
                        };

                        // KNOWN∧NOT-REFUSED, the same gate ChainSyncReceived applies (knows_device already excludes refused_devices; locked_out is the sibling case). The RX worker proved only "some contact knows this signing key"; here we prove the signer is a current device of THIS conversation's peer and not one the fold has refused or the fleet has locked — so a stolen/refused device cannot inject frames that drive the fork detectors.
                        if !self.contacts[contact_idx].knows_device(&sender_pubkey.key)
                            || self.contacts[contact_idx].locked_out
                        {
                            crate::logf!("CHAT: dropped frame from {} — signer {}... is not a trusted device of this conversation (refused/locked/unfolded)", crate::fp(&from_handle_hash), hex::encode(&sender_pubkey.key[..8]));
                            continue;
                        }

                        // The conversation this frame lands in — resolved THRU THE CONTACT (see the braid drain's SHADOW SEAM note: chains-derived resolution minted an unpersisted shadow object when the chains carry a stale-era participant set). Field-precise (`chains` pins `friendship_chains` for this whole block, so no &mut self method fits here).
                        let conv_pos = {
                            let conv_our_pid = if self.contacts[contact_idx].is_sibling {
                                match our_sibling_pid {
                                    Some(p) => p,
                                    None => continue,
                                }
                            } else {
                                identity_hh
                            };
                            let derived = self.contacts[contact_idx].conversation(&conv_our_pid);
                            let chains_id = crate::types::Conversation::new(
                                chains.participants().iter().copied(),
                            )
                            .id();
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

                        // Deduplication: we've already processed this exact message (UDP duplicate, or — the important case — the sender RETRANSMITTED because our ACK was lost). Don't re-process (that would double-advance), but DO re-send the ACK if this is the most recently acked message, so the lost-ACK case heals instead of the sender retrying until it gives up and its chain stays frozen.
                        // DURABLE second gate: is_duplicate lives inside the chain object, and a ceremony reset / braid-in mints a FRESH chain with last_received_times = None — so a frame arriving again post-reset (direct + relay dual-path, or an inbox-drain replay) sailed past it and was processed against the wrong chain state, forking the pair (the 2026-07-23 sibling desync). The rarangi row store keys on the same eagle_time, persists, and survives every chain reset — a stored inbound row at this timestamp means this exact frame was already processed, whatever the in-memory chain thinks.
                        // RECOVERED rows are excluded from the gate: a friend-attested backfill row was never chain-processed and carries no ack_hash, so treating it as "already processed" deadlocked the sender — recovery raced live delivery and the sender's retransmits were skipped un-ACKably forever while their chain waited (2026-07-24). The wire frame must process normally; insert_message_sorted upgrades the recovered row in place.
                        let row_dup = self.conversations[conv_pos]
                            .messages
                            .iter()
                            .any(|m| !m.is_outgoing && !m.recovered && m.timestamp == timestamp);
                        // TIMESTAMPS ARE NOT THE CHAIN: held messages carry composition-time stamps that can sit DAYS behind the lane's last-received time, and the timestamp verdict skipped them un-decrypted and un-ACKably — while every frame behind them gap-buffered forever (the stuck-grey four + the eternal gap, 2026-08-07). A frame whose prev links to our EXPECTED position is by definition the next message, never a duplicate; a true retransmit of a processed frame carries an older prev and still lands here.
                        let is_expected_next =
                            chains.verify_chain_link(&lane, &prev_msg_hp).is_ok();
                        if (chains.is_duplicate(&lane, timestamp) || row_dup) && !is_expected_next {
                            // Re-ACK from the stored message, looked up by its eagle_time. Unlike the old single-slot last_acked (which only remembered the MOST RECENT ack and so dropped any earlier duplicate → permanent sender stall), every received message persists its own ack_hash, so ANY duplicate self-heals a lost ACK.
                            let stored = self.conversations[conv_pos]
                                .messages
                                .iter()
                                .find(|m| !m.is_outgoing && m.timestamp == timestamp)
                                .and_then(|m| m.ack_hash)
                                .and_then(|ack| {
                                    self.contacts
                                        .get(contact_idx)
                                        .and_then(|c| c.device_key().map(|k| (ack, k)))
                                });
                            if let Some((ph, recipient_pubkey)) = stored {
                                if let Some(ref checker) = self.status_checker {
                                    // The ACK ALWAYS rides the relay alongside any direct leg. Gating on the sentinel missed the case that actually lagged in the field: a message received DIRECT whose reverse direction is dead — the direct ACK vanished, the sender retransmitted, and only the duplicate's re-ACK (relayed) landed. Acks are tiny and idempotent, so the duplicate delivery is free.
                                    let relay_to = self
                                        .contacts
                                        .get(contact_idx)
                                        .map(|c| c.relay_device_list())
                                        .unwrap_or_default();
                                    checker.send_ack(AckRequest {
                                        peer_addr: sender_addr,
                                        recipient_pubkey,
                                        conversation_token,
                                        acked_eagle_time: timestamp,
                                        plaintext_hash: ph,
                                        relay_to,
                                    });
                                    crate::logf!("CHAT: Re-ACKed duplicate from {} (eagle_time {}) — our earlier ACK was likely lost", crate::fp(&from_handle_hash), timestamp);
                                }
                            } else {
                                crate::logf!("CHAT: Skipping duplicate from {} (eagle_time {}) — no stored ack_hash (pre-fix message or outgoing)", crate::fp(&from_handle_hash), timestamp);
                            }
                            continue;
                        }

                        // Strict in-order processing (Layer 1). The receiver decrypts at CURRENT_KEY_INDEX, which is only correct when this message is the immediate successor of the last one we processed. So verify_chain_link is now HARD: on a mismatch the message is "ahead" (its predecessor hasn't arrived yet) — buffer it on the `prev_msg_hp` it awaits and SKIP decrypt. It gets replayed when that predecessor lands (see the gap-buffer drain after a successful advance below). "Behind"/duplicate is already handled by is_duplicate above; an unrelated stale prev_msg_hp simply waits in the buffer (and the retransmit path re-sends).
                        if let Err(expected) = chains.verify_chain_link(&lane, &prev_msg_hp) {
                            crate::logf!("CHAT: Hash chain gap from {} - expected prev {}..., got {}... — buffering (ahead of us)", crate::fp(&from_handle_hash), hex::encode(&expected[..8]), hex::encode(&prev_msg_hp[..8]));
                            // GAPS ARE TRANSPORT, NEVER FORK EVIDENCE: a missing predecessor means a frame is in flight, lost (anti-entropy re-serves it on the next pong edge), or a stale-era straggler — none of which a re-key repairs and all of which a re-key destroys (the ≥8-streak trigger here nuked healthy weaves all week, 2026-08-03→07, and masked the actual salt bug). Fork evidence lives solely in the decrypt-fail streak: a fill that arrives and still produces garbage is the only proof both heads committed differently.
                            {
                                let c = &mut self.contacts[contact_idx];
                                // The depth still rides the log-visible counter (cleared by a successful fill) so field logs show how far behind a lane is running.
                                let key = u64::from_le_bytes(expected[..8].try_into().unwrap())
                                    ^ u64::from_le_bytes(prev_msg_hp[..8].try_into().unwrap());
                                c.gap_streak = (key, c.gap_streak.1.saturating_add(1));
                                // A gap means the SENDER is missing our tip — and the tip travels in our ping's sync records, which an hour-deep presence backoff would sit on. A buffered frame is the loudest possible "this contact matters right now": collapse the backoff so the next sweep pings, the pong's tip re-arms their given-up retransmit, and the gap fills in seconds instead of an hour (the "some messages lag a very long time" of 2026-08-02).
                                c.ping_backoff = 0;
                                c.last_pinged = None;
                            }
                            // RECEIVER-DRIVEN GAP HEAL (2026-08-20): the backoff collapse above only works when the SENDER still holds the missing row as a pending — a row their side believes delivered (fleet-ACK'd via a sibling, or swept past ack_hash persistence) NEVER retransmits, and the in-order gate then holds every later row hostage forever. Field proof: a call ANSWER sat buffered behind one such hole while the caller rang out (a78c6f9b), and the chronic 53-buffered/4-filled stuck-message logs are the same class. The friend provably HOLDS the missing row (it is their own outgoing), so arm the urgent friend history walk — the same arm the strand-miss path uses in conversation.rs — which re-serves the hole from their store regardless of anyone's pending list. Gated on not-already-recovering so repeat buffering of the same frame doesn't re-arm a walk already in flight. Direct field access: `chains` pins friendship_chains for this block, and conversations is a disjoint field.
                            if !self.contacts[contact_idx].is_sibling {
                                let conv = &mut self.conversations[conv_pos];
                                if conv.history_recovery.as_ref().map_or(true, |r| r.complete) {
                                    crate::log("CHAT: gap arms the history walk — the friend holds the missing row(s)");
                                    conv.history_recovery = Some(crate::types::HistoryRecovery {
                                        oldest_recovered_osc: i64::MAX,
                                        complete: false,
                                        in_flight: None,
                                        next_request_osc: 0,
                                        urgent: true,
                                        was_complete_before: false,
                                        decrypt_fail_streak: 0,
                    expire_streak: 0,
                                        parked_key_fp: None,
                                    });
                                }
                            }
                            // The buffered entry carries the LANE label (the field predates lanes and keeps its name) — the replay re-enters the arm with it, resolving the same lane.
                            chains.buffer_for_gap(
                                prev_msg_hp,
                                lane,
                                timestamp,
                                ciphertext.clone(),
                                sender_addr,
                                sender_pubkey.key,
                            );
                            continue;
                        }

                        crate::logf!(
                            "CHAT: Received message from {} (eagle_time {}), {} bytes ciphertext",
                            crate::fp(&from_handle_hash),
                            timestamp,
                            ciphertext.len()
                        );

                        // OFF-THREAD: the braid decrypt (memory-hard scratch + layer peel) ran inline per frame — and the gap-cascade replays a burst of them in ONE tick. The worker gets the same lane snapshot the inline path cloned; commit_braid_rx re-gates against current state and runs everything after the decrypt. Until this frame commits, the next frame's chain-link verify fails and gap-buffers — the ordering the inline path enforced, kept for free.
                        let sender_chain = match chains.chain(&lane) {
                            Some(c) => c.clone(),
                            None => {
                                crate::log("CHAT: Sender chain not found");
                                continue;
                            }
                        };
                        let their_last_plaintext = chains.last_plaintext(&lane).to_vec();
                        let tx = self.braid_rx_tx.clone();
                        let wake = self.event_proxy.clone();
                        queue_job(&self.braid_job_tx, move || {
                            use crate::crypto::chain::{
                                decrypt_layers, derive_salt, generate_scratch, CURRENT_KEY_INDEX,
                            };
                            let salt = derive_salt(&their_last_plaintext, &sender_chain);
                            let scratch = generate_scratch(&sender_chain, &salt);
                            let et = vsf::EagleTime::from_oscillations(timestamp);
                            // Key/salt FINGERPRINTS (BLAKE3), never the bytes — mirrors CHAIN ENCRYPT so the two still diff for a chain divergence, without putting ratchet key material in a handle-readable log.
                            let key_fp = hex::encode(
                                &blake3::hash(&sender_chain.current_key()[..]).as_bytes()[..4],
                            );
                            let salt_fp = hex::encode(&blake3::hash(&salt[..]).as_bytes()[..4]);
                            crate::logf!("CHAIN DECRYPT: lane={}..., key#{}, salt#{}, eagle_time={}, ciphertext_len={}", hex::encode(&lane[..4]), key_fp, salt_fp, timestamp, ciphertext.len());
                            let plaintext = decrypt_layers(
                                &ciphertext,
                                &sender_chain,
                                CURRENT_KEY_INDEX,
                                &scratch,
                                &et,
                            );
                            let _ = tx.send(BraidRxDecrypted {
                                conversation_token,
                                lane,
                                prev_msg_hp,
                                timestamp,
                                sender_addr,
                                sender_pubkey,
                                plaintext,
                            });
                            if let Some(w) = wake.as_ref() {
                                let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
                            }
                        });
                    } else {
                        crate::logf!(
                            "CHAT: No friendship found for conversation_token {}...",
                            hex::encode(&conversation_token[..8])
                        );
                        // A frame we can't even ROUTE is evidence, not just noise: if this token's contact claims the ceremony is Complete, the state is lying (chains wiped, claim resurrected) and nothing else will ever trigger the repair. Recorded here, judged post-drain.
                        if !rekey_probe.contains(&conversation_token) {
                            rekey_probe.push(conversation_token);
                        }
                    }
                }
                StatusUpdate::MessageAck {
                    conversation_token,
                    acked_eagle_time,
                    plaintext_hash,
                } => {
                    // Get our handle_hash
                    let our_handle_hash = match self
                        .session
                        .as_ref()
                        .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
                    {
                        // PARTY ID: the friendship chain + slots are keyed on party ids (from_clutch + send both use our_party_id); matching on the raw seed here dropped every incoming message/probe as "not a participant" and hung the weave.
                        Some(h) => h,
                        None => {
                            crate::log("CHAT: No user_identity_seed - cannot process ACK");
                            continue;
                        }
                    };
                    // Sibling pid candidate, resolved BEFORE the chains borrow (see the ChatMessage arm).
                    let our_sibling_pid = self.our_sibling_pid();

                    // Find friendship by conversation_token
                    let chains_result = self
                        .friendship_chains
                        .iter_mut()
                        .find(|(_, c)| c.conversation_token == conversation_token);

                    // Contact index to seal AFTER the `chains` borrow ends (seal needs &mut self).
                    let mut ack_sealed_idx: Option<usize> = None;
                    // Friendship whose chains to persist AFTER the borrow — an ACK only removes a pending, so the save is safe to coalesce off-thread (a lost removal just re-transmits; the peer dedups).
                    let mut ack_persist_fid: Option<crate::types::friendship::FriendshipId> = None;
                    if let Some((fid_ref, chains)) = chains_result {
                        let ack_fid = *fid_ref;
                        // Party-id seam: whichever of (identity seed, sibling pid) is a participant is "us". The UNSHADOWED identity pid survives for the conversation resolution below.
                        let identity_hh = our_handle_hash;
                        let our_handle_hash = if chains.participants().contains(&our_handle_hash) {
                            our_handle_hash
                        } else if let Some(pid) =
                            our_sibling_pid.filter(|p| chains.participants().contains(p))
                        {
                            pid
                        } else {
                            crate::log("CHAT: we are not a participant in these chains (ACK)");
                            continue;
                        };
                        // For 2-party chats, the ACK sender is the "other" participant
                        let from_handle_hash = match chains.other_participant(&our_handle_hash) {
                            Some(h) => *h,
                            None => {
                                crate::log("CHAT: Could not determine ACK sender");
                                continue;
                            }
                        };

                        // Find contact by their handle_hash
                        let contact_info = self.contacts.iter().enumerate().find_map(|(idx, c)| {
                            if c.handle_hash == from_handle_hash {
                                Some((idx, c.display_name()))
                            } else {
                                None
                            }
                        });

                        let (contact_idx, _handle) = match contact_info {
                            Some((idx, h)) => (idx, h),
                            None => {
                                crate::logf!(
                                    "CHAT: Contact not found for ACK from handle_hash {}...",
                                    hex::encode(&from_handle_hash[..8])
                                );
                                continue;
                            }
                        };

                        crate::logf!(
                            "CHAT: ACK received from {} for eagle_time {} (hash: {}...)",
                            crate::fp(&from_handle_hash),
                            acked_eagle_time,
                            hex::encode(&plaintext_hash[..8])
                        );

                        // The conversation this ACK lands in — resolved THRU THE CONTACT (see the braid drain's SHADOW SEAM note: chains-derived resolution put delivered-flag flips on an unpersisted shadow object). Field-precise, same shape as the ChatMessage arm (`chains` pins `friendship_chains` here too).
                        let conv_pos = {
                            let conv_our_pid = if self.contacts[contact_idx].is_sibling {
                                match our_sibling_pid {
                                    Some(p) => p,
                                    None => continue,
                                }
                            } else {
                                identity_hh
                            };
                            let derived = self.contacts[contact_idx].conversation(&conv_our_pid);
                            let chains_id = crate::types::Conversation::new(
                                chains.participants().iter().copied(),
                            )
                            .id();
                            if chains_id != derived.id() {
                                crate::logf!("CHAT: SHADOW SEAM — chains derive conversation {} but the contact derives {} (ACK path); landing in the contact's", hex::encode(&chains_id.as_bytes()[..4]), hex::encode(&derived.id().as_bytes()[..4]));
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

                        // Process ACK: advance our chain and remove pending message
                        if chains.process_ack(acked_eagle_time, &plaintext_hash) {
                            crate::logf!(
                                "CHAT: Chain advanced for {} (ACK verified)",
                                crate::fp(&from_handle_hash)
                            );

                            // Our TX chain just advanced on a matching ACK — their RX is proven. Record it so the chain-weave can seal (sealing itself happens after the `chains` borrow ends, below). This is the "our TX / their RX" half of woven.
                            if let Some(contact) = self.contacts.get_mut(contact_idx) {
                                contact.chain_advanced_by_ack = true;
                                // A matching ACK is DEFINITIVE proof the peer holds our ceremony proof — they cannot run the chain that produced this ACK without it. So stop resending the proof HERE, independently of whether the full weave seals. Previously the budget only cleared inside seal_chain_if_ready, which needs BOTH directions, so a ceremony that was provably complete kept rebroadcasting: 5 retransmits × 3 relay devices ≈ 15 pointless relay sends per contact (Jon, 2026-07-27).
                                contact.clutch_proof_resends_left = 0;
                            }
                            ack_sealed_idx = Some(contact_idx);

                            // First ACK confirms both sides have working chains - safe to zeroize CLUTCH keypairs
                            if let Some(contact) = self.contacts.get_mut(contact_idx) {
                                if contact.clutch_our_keypairs.is_some() {
                                    let their_identity_seed = contact.handle_hash;
                                    crate::logf!(
                                        "CLUTCH: First ACK from {} - zeroizing ephemeral keypairs",
                                        crate::fp(&contact.handle_proof)
                                    );
                                    if let Some(ref mut keys) = contact.clutch_our_keypairs {
                                        keys.zeroize();
                                    }
                                    contact.clutch_our_keypairs = None;
                                    contact.clutch_round_started = None;
                                    for slot in &mut contact.clutch_slots {
                                        slot.offer = None;
                                        if let Some(ref mut s) = slot.kem_secrets_from_them {
                                            s.zeroize();
                                        }
                                        if let Some(ref mut s) = slot.kem_secrets_to_them {
                                            s.zeroize();
                                        }
                                        slot.kem_secrets_from_them = None;
                                        slot.kem_secrets_to_them = None;
                                    }

                                    // Delete persisted keypairs file (no longer needed)
                                    if let Some(storage) = self.storage.as_ref() {
                                        if let Err(e) =
                                            crate::storage::contacts::delete_clutch_keypairs(
                                                &their_identity_seed,
                                                storage,
                                            )
                                        {
                                            crate::logf!("CLUTCH: Failed to delete keypairs file for seed {}: {}", hex::encode(&their_identity_seed[..4]), e);
                                        }
                                    }
                                }
                            }

                            // Persist chains OFF-thread (coalesced): an ACK only removed a pending — not a commit point, so a delayed/lost write just costs a redundant retransmit the peer dedups. Deferred past the `chains` borrow.
                            ack_persist_fid = Some(ack_fid);
                        } else {
                            // No pending message matched. Two cases: (a) a DUPLICATE ACK — dual-path racing (P3) delivers the same ACK on both the LAN and public path, so the second copy arrives after the first already advanced + cleared the pending entry; (b) a genuinely UNKNOWN ACK. Tell them apart via the outgoing message: if it exists and is already `delivered`, this is the benign duplicate — log at DEBUG so it stops reading as a failure.
                            let is_dup = self.conversations[conv_pos].messages.iter().any(|m| {
                                m.is_outgoing && m.delivered && m.timestamp == acked_eagle_time
                            });
                            if is_dup {
                                crate::log_at(
                                    crate::LogLevel::Debug,
                                    &format!(
                                        "CHAT: Duplicate ACK from {} (eagle_time {}) — already delivered, dual-path echo",
                                        crate::fp(&from_handle_hash),
                                        acked_eagle_time
                                    ),
                                );
                            } else {
                                crate::logf!("CHAT: ACK verification failed for {} (no matching pending message)", crate::fp(&from_handle_hash));
                            }
                        }

                        // Mark message as delivered in UI
                        let mut delivered_row: Option<ChatMessage> = None;
                        {
                            let conv = &mut self.conversations[conv_pos];
                            // Find message by matching eagle_time (exact i64 oscillations)
                            let mut found_msg = false;
                            for msg in conv.messages.iter_mut().rev() {
                                if msg.is_outgoing && !msg.delivered {
                                    // Match by eagle_time (exact i64 match)
                                    if msg.timestamp == acked_eagle_time {
                                        msg.delivered = true;
                                        delivered_row = Some(msg.clone());
                                        found_msg = true;
                                        changed = true;
                                        break;
                                    }
                                }
                            }

                            // Persist delivered status (async writer — the inline save was the 5.7s MessageAck stall)
                            if found_msg {
                                persist_hashes.push(from_handle_hash);
                            }
                        }
                        // Live fleet propagation of the delivered tick (the sibling merge upgrades its copy monotonically).
                        if let Some(row) = delivered_row {
                            self.push_rows_to_siblings(
                                contact_idx,
                                std::slice::from_ref(&row),
                                None,
                            );
                        }
                    } else {
                        crate::logf!(
                            "CHAT: No friendship found for ACK conversation_token {}...",
                            hex::encode(&conversation_token[..8])
                        );
                    }

                    // Defer the chain-weave seal until after the loop (outer `checker` borrow blocks `&mut self` here). No-op later unless both directions are proven.
                    if let Some(idx) = ack_sealed_idx {
                        chain_seal_indices.push(idx);
                    }
                    // Coalesced off-thread chains persist for the ACK's pending-removal — deferred past the `checker` borrow like every other &mut-self action here.
                    if let Some(fid) = ack_persist_fid {
                        chains_persist_fids.push(fid);
                    }
                }

                // PT large transfer received (fallback - normally parsed in status.rs) This only fires if the PT data wasn't recognized as CLUTCH message
                StatusUpdate::PTReceived { peer_addr, data } => {
                    crate::logf!(
                        "PT: Received unknown {} bytes from {} (not CLUTCH)",
                        data.len(),
                        peer_addr
                    );
                }

                // PT outbound transfer completed
                StatusUpdate::PTSendComplete { peer_addr } => {
                    crate::logf!("PT: Outbound transfer to {} completed", peer_addr);
                    // TODO: Track completion for full CLUTCH flow
                }

                // Full CLUTCH offer received (~548KB with all 8 pubkeys) Payload is already parsed and signature verified by status.rs
                StatusUpdate::ClutchOfferReceived {
                    conversation_token,
                    offer_provenance, // Unique per offer (VSF hp field)
                    sender_pubkey,
                    payload,
                    sender_addr: raw_sender_addr,
                } => {
                    use crate::crypto::clutch::{derive_conversation_token, ClutchOfferPayload};
                    use crate::network::status::ClutchOfferRequest;
                    use crate::types::ClutchState;
                    // LOCKOUT gate: a locked-out device must never re-enter thru a ceremony — accepting its offer would weave fresh chains with hardware the fleet declared stolen.
                    if self.is_locked_device(&sender_pubkey) {
                        crate::logf!(
                            "CLUTCH: offer from LOCKED-OUT device {} — refused",
                            crate::fp(&sender_pubkey)
                        );
                        continue;
                    }

                    crate::logf!(
                        "CLUTCH: Processing ClutchOfferReceived from {} (contacts={})",
                        raw_sender_addr,
                        self.contacts.len()
                    );

                    // Normalize to port 4383 (TCP source port is ephemeral)
                    let sender_addr =
                        std::net::SocketAddr::new(raw_sender_addr.ip(), crate::PHOTON_PORT);

                    // Get our handle_hash
                    let our_handle_hash = match self
                        .session
                        .as_ref()
                        .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
                    {
                        // PARTY ID (not raw seed): the conversation token + slots key on party ids on the SEND side; the receive path must match or every friend ceremony stalls at "unknown conversation_token".
                        Some(h) => h,
                        None => {
                            #[cfg(feature = "development")]
                            #[cfg(feature = "development")]
                            crate::log("CLUTCH: No user_identity_seed available");
                            continue;
                        }
                    };
                    let our_sibling_pid = self.our_sibling_pid();

                    // Find contact by conversation_token (compute token for each contact and match). Party-id seam: sibling candidates token with the device-derived pid pair; the resolved "our" id shadows the seed for the whole arm. The matched INDEX travels with the pair — the trust gate below must judge THE ROW THE TOKEN NAMED, and a re-find by handle_hash first-match could bind a different row sharing the hash (the shadow-row class again: sibling vs debris vs self, and the gate then reads the wrong row's trust).
                    let (matched_ci, their_handle_hash, our_handle_hash) =
                        match self.contacts.iter().enumerate().find_map(|(ci, c)| {
                            let our = if c.is_sibling {
                                our_sibling_pid?
                            } else {
                                our_handle_hash
                            };
                            (derive_conversation_token(&[our, c.handle_hash]) == conversation_token)
                                .then_some((ci, c.handle_hash, our))
                        }) {
                            Some(hit) => hit,
                            None => {
                                crate::logf!(
                                    "CLUTCH: Received offer with unknown conversation_token {}",
                                    hex::encode(&conversation_token[..8])
                                );
                                continue;
                            }
                        };

                    crate::logf!(
                        "CLUTCH: Received full offer (VSF verified) from {} tok={}...",
                        sender_addr,
                        hex::encode(&conversation_token[..8])
                    );

                    // Gate: the sender must be a CURRENTLY-TRUSTED device of this contact (fold-respecting `knows_device`). Post-fold this widens to ANY current fleet member (a friend's 2nd device can now CLUTCH — was pinned to first-met only) AND revokes a removed device (it fails membership); pre-fold + siblings pin to the one known device exactly as before.
                    if !self.sender_trusted_for(&self.contacts[matched_ci], &sender_pubkey) {
                        crate::logf!(
                            "CLUTCH: offer from untrusted/removed device {} for {} (row: fp {} sib={} first-met {}) — dropping",
                            hex::encode(&sender_pubkey[..8]),
                            hex::encode(&their_handle_hash[..8]),
                            crate::fp(&self.contacts[matched_ci].handle_proof).as_str(),
                            self.contacts[matched_ci].is_sibling,
                            hex::encode(&self.contacts[matched_ci].device_key().unwrap_or_default()[..4])
                        );
                        continue;
                    }

                    // OFFER AS MUTUALITY EVIDENCE (consent gate, 2026-08-25): an old client never knocks — its opening move is still the full offer. A token-matched, trust-gated offer proves the sender holds both party ids, exactly what the knock proves, so it flips a WeAsked row Mutual (persist + roster ride) before normal processing arms the ceremony.
                    if !self.contacts[matched_ci].consent_mutual {
                        self.contacts[matched_ci].consent_mutual = true;
                        crate::logf!(
                            "CONSENT: mutual add confirmed with {} via their offer — arming the ceremony",
                            crate::fp(&self.contacts[matched_ci].handle_proof)
                        );
                        if let Some(storage) = self.storage.as_ref() {
                            let snapshot = self.contacts[matched_ci].clone();
                            if let Err(e) =
                                crate::storage::contacts::save_contact(&snapshot, storage)
                            {
                                crate::logf!("CONSENT: contact save failed: {}", e);
                            }
                        }
                        consent_roster_push = true;
                    }

                    // The payload is already parsed
                    let their_offer = payload;

                    // Find contact by handle_hash
                    let mut rekey_request: Option<(ContactId, [u8; 32])> = None;
                    let mut chains_to_remove: Vec<FriendshipId> = Vec::new();
                    // Deferred KEM encapsulation spawn (to avoid borrow conflict)
                    let mut kem_encap_spawn: Option<(
                        ContactId,
                        ClutchOfferPayload,
                        [u8; 32],
                        [u8; 32],
                        std::net::SocketAddr,
                    )> = None;

                    for (idx, contact) in self.contacts.iter_mut().enumerate() {
                        if contact.handle_hash == their_handle_hash {
                            // A relayed message (RELAY_ADDR sentinel) carries no reachable peer address — skip address-learning (storing the sentinel as contact.ip would poison direct sends) and mark the link relay-only, which lights the presence lime-yellow. A direct message clears the flag: direct always wins. Otherwise inbound DATA elects the sending device ACTIVE (the fleet reply-TX rule) and seeds its endpoint, so contact-level addressing follows the device actually talking to us.
                            if sender_addr == crate::network::status::RELAY_ADDR {
                                contact.reached_via_relay = true;
                            } else {
                                contact.reached_via_relay = false;
                                contact.ip = Some(sender_addr);
                                contact.active_device = Some(sender_pubkey);
                                let pub_src = !is_private_addr(&sender_addr.ip());
                                let ep = contact.endpoint_mut(&sender_pubkey);
                                if pub_src {
                                    ep.public = Some(sender_addr);
                                } else {
                                    ep.lan = Some(sender_addr);
                                }
                            }
                            // Authenticated CLUTCH traffic from them ⇒ reachable right now ⇒ show online immediately, don't wait for the next pong.
                            if !contact.is_online {
                                contact.is_online = true;
                                changed = true;
                                crate::logf!(
                                    "Status: {} is now ONLINE (CLUTCH)",
                                    crate::fp(&contact.handle_proof)
                                );
                            }

                            // Simple re-key logic: if stored keys don't match received keys, re-key. Same keys = duplicate/stale (ignore). Different/no keys = accept.
                            let stored_hqc_pub = contact
                                .get_slot(&their_handle_hash)
                                .and_then(|slot| slot.offer.as_ref())
                                .map(|o| o.hqc256_public.clone());
                            // PRE-ARRIVAL snapshot for the AwaitingProof branch far below: the fallthrough between here and there STORES this arrival's offer into the slot, so a late re-read compares the offer to ITSELF (always "same keys") — which is exactly how the AwaitingProof wedge ate fresh offers forever.
                            let pre_arrival_hqc = stored_hqc_pub.clone();

                            if let Some(stored_keys) = stored_hqc_pub {
                                if stored_keys == their_offer.hqc256_public {
                                    // Same keys - check if we already sent KEM response If so, peer didn't receive it - re-send!
                                    let already_sent_kem = contact
                                        .get_slot(&our_handle_hash)
                                        .map(|s| s.kem_secrets_to_them.is_some())
                                        .unwrap_or(false);

                                    if already_sent_kem {
                                        // We already sent KEM response but peer resent offer They didn't receive it - trigger re-send
                                        crate::logf!("CLUTCH: Re-sending KEM response to {} (peer resent same offer)", crate::fp(&contact.handle_proof));
                                        // The missing half of the deadlock recovery. The peer re-sending their offer means they're stuck, and the usual cause is that OUR offer never reached them — its one send may have gone to an address not yet confirmed reachable (e.g. their LAN IPv4 before their public/IPv6 was known) and been lost. Re-sending only our KEM can't help: they can't answer an offer they never received, so they keep re-sending theirs and queuing our KEMs forever. Re-arm our offer so the online/pong handler re-transmits it via race_addrs — now to the address their packets are actually arriving from. (Their side re-sends THEIR offer via the pending-KEM branch above; this is the symmetric OUR-offer resend.)
                                        if contact.clutch_state == ClutchState::Pending {
                                            contact.clutch_offer_sent = false;
                                        }
                                        // Don't continue - fall thru to re-send KEM below
                                    } else {
                                        // Same keys but no KEM sent yet - truly duplicate, ignore
                                        crate::logf!("CLUTCH: Ignoring duplicate offer from {} (same keys, no KEM sent yet)", crate::fp(&contact.handle_proof));
                                        continue;
                                    }
                                } else {
                                    // Different keys from them - but DON'T immediately nuke! This prevents infinite re-key loops where both sides keep regenerating.
                                    //
                                    // Strategy: If we have keypairs, just update their offer and continue. We'll send our existing offer, they'll either:
                                    // - Accept it (converge) if they're mid-ceremony
                                    // - Send KEM response (complete) if they're ahead
                                    //
                                    // Guard against a FALSE re-key: a peer we already completed with re-sends its offer (retransmit, or our slots got zeroized post- completion so stored_hqc_pub no longer matches). At completion we saved their HQC pubkey PREFIX precisely to recognize this. If the incoming offer matches what we completed with, it's the SAME peer — ignore it, do NOT nuke. Only a genuinely DIFFERENT key (they truly re-keyed / lost their chains) should trigger a re-key. Without this, a Complete↔Complete pair bounced back to Pending on a stray offer ("it completed, then went back to Pending after a message").
                                    if contact.clutch_state == ClutchState::Complete {
                                        let their_prefix: [u8; 8] = their_offer.hqc256_public[..8]
                                            .try_into()
                                            .unwrap_or_default();
                                        if contact.completed_their_hqc_prefix == Some(their_prefix)
                                        {
                                            crate::logf!("CLUTCH: Ignoring offer from {} — matches the key we already completed with (no re-key)", crate::fp(&contact.handle_proof));
                                            continue;
                                        }
                                        // Post-weave cooldown (see the twin guard in the no-keypairs branch below): a different-keyed offer arriving right after we wove is a crossed pre-completion re-offer from the peer's own racing ceremony, not a deliberate reset. Ignore it briefly so we don't nuke a just-woven chain into a divergent re-key. A genuine reset persists past the window.
                                        const REKEY_COOLDOWN: std::time::Duration =
                                            std::time::Duration::from_secs(10);
                                        if contact
                                            .clutch_completed_at
                                            .is_some_and(|t| t.elapsed() < REKEY_COOLDOWN)
                                        {
                                            crate::logf!("CLUTCH: Ignoring different-keyed offer from {} — completed {}ms ago (post-completion re-key cooldown)", crate::fp(&contact.handle_proof), contact.clutch_completed_at.map(|t| t.elapsed().as_millis()).unwrap_or(0));
                                            continue;
                                        }
                                        crate::logf!("CLUTCH: Re-key from {} - we're Complete, they have new keys, nuking for fresh ceremony", crate::fp(&contact.handle_proof));
                                        // Full re-key: nuke everything
                                        contact.clutch_our_keypairs = None;
                                        contact.clutch_round_started = None;
                                        contact.clutch_slots.clear();
                                        contact.ceremony_id = None;
                                        contact.offer_provenances.clear();
                                        contact.clutch_pending_kem = None;
                                        contact.clutch_offer_sent = false;
                                        contact.clutch_state = ClutchState::Pending;
                                        contact.completed_their_hqc_prefix = None;
                                        if let Some(old_friendship_id) =
                                            contact.friendship_id.take()
                                        {
                                            crate::logf!(
                                                "CLUTCH: Invalidating old chains for {}",
                                                crate::fp(&contact.handle_proof)
                                            );
                                            chains_to_remove.push(old_friendship_id);
                                        }
                                        rekey_request =
                                            Some((contact.id.clone(), contact.handle_hash));
                                    } else {
                                        // Not Complete and they minted NEW keys — their side is running a FRESH ceremony instance (their §4.2 ceremony owner changed, or they discarded and restarted). The old "keep our keys, swap their offer" splice welded half of OUR round onto half of THEIRS: the friend then held offers/completes from mixed instances and dropped the odd one out as "unknown conversation_token" forever. Adopt their new round wholesale instead — discard ours completely; the fallthrough below re-inits slots and stores their fresh offer + provenance; fresh keys of ours arrive via keygen and the drain sends our offer.
                                        // ADOPTION COOLDOWN: a peer that can't HEAR our responses (one-way reachability) re-offers with fresh keys every ~25s; unthrottled adoption re-ran keygen+encap per round (a UI-thread hitch storm, live-pair livelock 2026-07-25). Hold the recently-adopted round instead — our response to it is already in flight/on the relay, and the peer only needs one to land. A genuinely new ceremony attempt survives the ignore (it persists past the window).
                                        const ADOPTION_COOLDOWN: std::time::Duration =
                                            std::time::Duration::from_secs(60);
                                        if contact
                                            .clutch_last_adoption
                                            .is_some_and(|t| t.elapsed() < ADOPTION_COOLDOWN)
                                        {
                                            crate::logf!("CLUTCH: {} re-offered fresh keys {}s after the last adoption — holding our round (adoption cooldown; their receive path is likely down)", crate::fp(&contact.handle_proof), contact.clutch_last_adoption.map(|t| t.elapsed().as_secs()).unwrap_or(0));
                                            continue;
                                        }
                                        contact.clutch_last_adoption =
                                            Some(std::time::Instant::now());
                                        crate::logf!("CLUTCH: {} sent new keys mid-ceremony (state={}) — discarding our round and adopting theirs", crate::fp(&contact.handle_proof), format!("{:?}", contact.clutch_state));
                                        contact.discard_clutch_round();
                                        // GUARDED re-trigger: a keygen already in flight will complete this round (the drain stores + sends our offer) — spawning another here would ping-pong re-keys when both sides discard simultaneously.
                                        if !contact.clutch_keygen_in_progress {
                                            contact.clutch_keygen_in_progress = true;
                                            rekey_request =
                                                Some((contact.id.clone(), contact.handle_hash));
                                        }
                                    }
                                }
                            }
                            // No stored keys = fresh start, accept offer below

                            // Initialize slots if not already done
                            if contact.clutch_slots.is_empty() {
                                contact.init_clutch_slots(our_handle_hash);
                            }

                            // Store their offer in their slot, with its SIGNING device — the eggs bind the offer-origin device pair, never the pinned one (PartySlot::offer_device).
                            if let Some(slot) = contact.get_slot_mut(&their_handle_hash) {
                                slot.offer = Some(their_offer.clone());
                                slot.offer_device = Some(sender_pubkey);
                                crate::logf!(
                                    "CLUTCH: Stored offer from {} in slot",
                                    crate::fp(&contact.handle_proof)
                                );
                            }

                            // Store OUR offer in OUR slot too — every slot needs offer + a KEM contribution to be complete (PartySlot::is_complete). When their offer arrives first and we go straight to the KEM-response path, our own slot would otherwise keep offer=None forever, so all_slots_complete never fires and the ceremony never runs (the one-sided-nuke re-key stall: we have keys + sent a KEM, but our local offer was never recorded).
                            if contact
                                .get_slot(&our_handle_hash)
                                .map(|s| s.offer.is_none())
                                .unwrap_or(false)
                            {
                                if let Some(ref keypairs) = contact.clutch_our_keypairs {
                                    let our_offer =
                                        clutch::ClutchOfferPayload::from_keypairs(keypairs);
                                    if let Some(local_slot) = contact.get_slot_mut(&our_handle_hash)
                                    {
                                        local_slot.offer = Some(our_offer);
                                        crate::log(
                                            "CLUTCH: Stored our own offer in local slot (on offer-received)",
                                        );
                                    }
                                }
                            }

                            // Store their offer_provenance for ceremony_id derivation
                            if !contact.offer_provenances.contains(&offer_provenance) {
                                contact.offer_provenances.push(offer_provenance);
                                crate::logf!(
                                    "CLUTCH: Stored offer_provenance from {} (now have {})",
                                    crate::fp(&contact.handle_proof),
                                    contact.offer_provenances.len()
                                );
                            }

                            // Compute ceremony_id if we have all provenances (2 for DM)
                            let required_provenances = 2;
                            if contact.ceremony_id.is_none()
                                && contact.offer_provenances.len() >= required_provenances
                            {
                                use crate::types::CeremonyId;
                                let ceremony_id = *CeremonyId::derive(
                                    &[our_handle_hash, contact.handle_hash],
                                    &contact.offer_provenances,
                                )
                                .as_bytes();
                                contact.ceremony_id = Some(ceremony_id);
                                crate::logf!(
                                    "CLUTCH: Derived ceremony_id={}... from {} offer_provenances",
                                    hex::encode(&ceremony_id[..4]),
                                    contact.offer_provenances.len()
                                );

                                // A KEM response queued before ceremony_id existed can drain now — to the decap JOB, not inline (8 PQ opens off the UI thread, 2026-08-15). The drain stores the secrets and runs the completion check.
                                if contact.clutch_pending_kem.is_some()
                                    && !contact.clutch_kem_decap_in_progress
                                {
                                    if let Some(ref local_keys) = contact.clutch_our_keypairs {
                                        let pending_kem =
                                            contact.clutch_pending_kem.take().expect("checked");
                                        contact.clutch_kem_decap_in_progress = true;
                                        decap_spawns.push((
                                            contact.id.clone(),
                                            pending_kem,
                                            local_keys.clone(),
                                        ));
                                        crate::logf!("CLUTCH: Spawning decap for queued KEM from {} (ceremony_id now available)", crate::fp(&contact.handle_proof));
                                    }
                                }
                            }

                            // Persist slot state (offer, provenances, ceremony_id)
                            if let Some(storage) = self.storage.as_ref() {
                                if let Err(e) = crate::storage::contacts::save_clutch_slots(
                                    &contact.clutch_slots,
                                    &contact.offer_provenances,
                                    contact.ceremony_id,
                                    &contact.handle_hash,
                                    storage,
                                ) {
                                    crate::logf!(
                                        "CLUTCH: Failed to save slots for {}: {}",
                                        crate::fp(&contact.handle_proof),
                                        e
                                    );
                                }
                            }

                            // If we have keypairs, send our offer (if not sent) and KEM response
                            if let Some(ref keypairs) = contact.clutch_our_keypairs {
                                // Compute conversation_token once for this contact
                                let conv_token = derive_conversation_token(&[
                                    our_handle_hash,
                                    contact.handle_hash,
                                ]);

                                // Send our offer if not already sent
                                if !contact.clutch_offer_sent {
                                    use crate::network::fgtw::protocol::build_clutch_offer_vsf;

                                    let our_offer = ClutchOfferPayload::from_keypairs(keypairs);

                                    // Build VSF and capture our offer_provenance
                                    match build_clutch_offer_vsf(
                                        &conv_token,
                                        &our_offer,
                                        self.device_keypair
                                            .as_ref()
                                            .expect("device_keypair set in init")
                                            .public
                                            .as_bytes(),
                                        self.device_keypair
                                            .as_ref()
                                            .expect("device_keypair set in init")
                                            .secret
                                            .as_bytes(),
                                        contact
                                            .clutch_round_started
                                            .unwrap_or_else(vsf::eagle_time_oscillations),
                                    ) {
                                        Ok((vsf_bytes, our_offer_provenance)) => {
                                            // Store our offer provenance
                                            if !contact
                                                .offer_provenances
                                                .contains(&our_offer_provenance)
                                            {
                                                contact
                                                    .offer_provenances
                                                    .push(our_offer_provenance);
                                            }

                                            // The offer arrived from sender_addr, so that path is known-reachable — use it as primary and race the contact's other known address as the alternate.
                                            let alt = contact
                                                .race_addrs()
                                                .and_then(|(p, a)| a.or(Some(p)))
                                                .filter(|a| *a != sender_addr);
                                            checker.send_offer(ClutchOfferRequest {
                                                peer_addr: sender_addr,
                                                alt_addr: alt,
                                                vsf_bytes,
                                                recipient_pubkey: contact.device_key().unwrap_or_default(), // unreachable-zero: this arm answers a frame-verified peer; Option-izing the request API is the follow-up
                                                relay_to: contact.relay_device_list(),
                                            });
                                            contact.clutch_offer_sent = true;
                                            // Store local offer in local slot too
                                            if let Some(local_slot) =
                                                contact.get_slot_mut(&our_handle_hash)
                                            {
                                                local_slot.offer = Some(our_offer);
                                            }
                                            crate::logf!(
                                                "CLUTCH: Sent full offer to {} (prov={}...)",
                                                crate::fp(&contact.handle_proof),
                                                hex::encode(&our_offer_provenance[..4])
                                            );

                                            // Compute ceremony_id now that we have both provenances
                                            if contact.ceremony_id.is_none()
                                                && contact.offer_provenances.len()
                                                    >= required_provenances
                                            {
                                                use crate::types::CeremonyId;
                                                let ceremony_id = *CeremonyId::derive(
                                                    &[our_handle_hash, contact.handle_hash],
                                                    &contact.offer_provenances,
                                                )
                                                .as_bytes();
                                                contact.ceremony_id = Some(ceremony_id);
                                                crate::logf!("CLUTCH: Derived ceremony_id={}... after sending offer", hex::encode(&ceremony_id[..4]));
                                            }

                                            // Persist provenance/ceremony_id immediately
                                            if let Some(storage) = self.storage.as_ref() {
                                                if let Err(e) =
                                                    crate::storage::contacts::save_clutch_slots(
                                                        &contact.clutch_slots,
                                                        &contact.offer_provenances,
                                                        contact.ceremony_id,
                                                        &contact.handle_hash,
                                                        storage,
                                                    )
                                                {
                                                    crate::logf!(
                                                        "Failed to persist CLUTCH provenance: {}",
                                                        e
                                                    );
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            crate::logf!(
                                                "CLUTCH: Failed to build offer VSF: {}",
                                                e
                                            );
                                        }
                                    }
                                }

                                // Send KEM response (encapsulate to remote pubkeys) Check if we haven't already sent (kem_secrets_to_them in local slot) KEM response requires ceremony_id (for wire format verification)
                                let already_sent_kem = contact
                                    .get_slot(&our_handle_hash)
                                    .map(|s| s.kem_secrets_to_them.is_some())
                                    .unwrap_or(false);

                                // Check for re-send case: we have stored payload from previous send
                                let resend_payload = contact
                                    .get_slot(&our_handle_hash)
                                    .and_then(|s| s.kem_response_for_resend.clone());

                                if let Some(kem_response) = resend_payload {
                                    // Re-send using stored payload
                                    if let Some(ceremony_id) = contact.ceremony_id {
                                        use crate::network::status::ClutchKemResponseRequest;

                                        let alt = contact
                                            .race_addrs()
                                            .and_then(|(p, a)| a.or(Some(p)))
                                            .filter(|a| *a != sender_addr);
                                        checker.send_kem_response(ClutchKemResponseRequest {
                                            peer_addr: sender_addr,
                                            alt_addr: alt,
                                            conversation_token: conv_token,
                                            ceremony_id,
                                            payload: kem_response,
                                            device_pubkey: *self
                                                .device_keypair
                                                .as_ref()
                                                .expect("device_keypair set in init")
                                                .public
                                                .as_bytes(),
                                            device_secret: *self
                                                .device_keypair
                                                .as_ref()
                                                .expect("device_keypair set in init")
                                                .secret
                                                .as_bytes(),
                                            recipient_pubkey: contact.device_key().unwrap_or_default(), // unreachable-zero: this arm answers a frame-verified peer; Option-izing the request API is the follow-up
                                            relay_to: contact.relay_device_list(),
                                        });
                                        crate::logf!(
                                            "CLUTCH: Re-sent KEM response to {}",
                                            crate::fp(&contact.handle_proof)
                                        );
                                    }
                                } else if !already_sent_kem && !contact.clutch_kem_encap_in_progress
                                {
                                    if let Some(ceremony_id) = contact.ceremony_id {
                                        // Defer spawn for KEM encapsulation (to avoid borrow conflict) (PQ crypto is slow ~800ms, would block UI/network)
                                        contact.clutch_kem_encap_in_progress = true;
                                        kem_encap_spawn = Some((
                                            contact.id.clone(),
                                            their_offer.clone(),
                                            ceremony_id,
                                            conv_token,
                                            sender_addr,
                                        ));
                                        crate::logf!(
                                            "CLUTCH: Will spawn KEM encapsulation for {}",
                                            crate::fp(&contact.handle_proof)
                                        );
                                        changed = true;
                                    } else {
                                        crate::logf!("CLUTCH: Deferring KEM response to {} - waiting for ceremony_id", crate::fp(&contact.handle_proof));
                                    }
                                }

                                // Check if ceremony is complete (defer to after outer loop)
                                if contact.all_slots_complete() {
                                    ceremony_completions.push(idx);
                                    changed = true;
                                }
                            } else if contact.clutch_our_keypairs.is_none() {
                                if contact.clutch_keygen_in_progress {
                                    // Keygen already running - don't spawn another
                                    crate::logf!("CLUTCH: Received offer from {} but keygen already in progress - waiting", crate::fp(&contact.handle_proof));
                                } else {
                                    // No keypairs - need to respond (whether Complete or not) If Complete: peer lost their chains, accept re-key If not Complete: restart mid-ceremony or fresh re-key
                                    if contact.clutch_state == ClutchState::Complete {
                                        // POST-WEAVE RE-KEY COOLDOWN. Completion zeroizes our ephemeral keypairs (is_none here), so a peer's offer that was in flight just before they saw our completion lands right after we weave and would trip the re-key path below — a SPURIOUS re-key that, when both sides do it near-simultaneously, storms into divergent ceremonies (observed: two devices stuck at 5/8 and 7/8 forever). Within the cooldown, ignore the stray offer: a crossed leftover stops within ~1s (the peer completes too). A GENUINE reset peer keeps sending and re-keys once the window passes.
                                        const REKEY_COOLDOWN: std::time::Duration =
                                            std::time::Duration::from_secs(10);
                                        if contact
                                            .clutch_completed_at
                                            .is_some_and(|t| t.elapsed() < REKEY_COOLDOWN)
                                        {
                                            crate::logf!("CLUTCH: Ignoring offer from {} — completed {}ms ago (post-completion re-key cooldown; likely a crossed pre-completion offer, not a reset)", crate::fp(&contact.handle_proof), contact.clutch_completed_at.map(|t| t.elapsed().as_millis()).unwrap_or(0));
                                            continue;
                                        }
                                        // Peer is sending an offer while we think we're Complete. This means either:
                                        // 1. Same HQC prefix: peer missed our KEM response (can't re-send without keypairs)
                                        // 2. Different HQC prefix: peer lost chains, wants re-key
                                        //
                                        // Since we have NO keypairs here (we're in the is_none branch), we can't re-respond even to the same offer. Accept as re-key.
                                        //
                                        // Note: If peer keeps re-sending same offer, both sides will eventually converge on a fresh ceremony (peer will regenerate keys after timeout).
                                        crate::logf!("CLUTCH: Received offer from {} while Complete - peer lost chains, accepting re-key", crate::fp(&contact.handle_proof));
                                        // Delete our old chains - they're useless now
                                        if let Some(fid) = contact.friendship_id {
                                            chains_to_remove.push(fid);
                                        }
                                        // Reset ALL CLUTCH state for new ceremony (canonical discard + the Complete-rekey-only friendship clear)
                                        contact.discard_clutch_round();
                                        contact.friendship_id = None;
                                        // Re-initialize slots and store their offer (was stored earlier but we just cleared)
                                        contact.init_clutch_slots(our_handle_hash);
                                        if let Some(slot) = contact.get_slot_mut(&their_handle_hash)
                                        {
                                            slot.offer = Some(their_offer.clone());
                                            slot.offer_device = Some(sender_pubkey);
                                        }
                                        // Store their offer_provenance (was cleared, need to re-add)
                                        if !contact.offer_provenances.contains(&offer_provenance) {
                                            contact.offer_provenances.push(offer_provenance);
                                        }

                                        // Persist re-key state immediately
                                        if let Some(storage) = self.storage.as_ref() {
                                            if let Err(e) =
                                                crate::storage::contacts::save_clutch_slots(
                                                    &contact.clutch_slots,
                                                    &contact.offer_provenances,
                                                    contact.ceremony_id,
                                                    &contact.handle_hash,
                                                    storage,
                                                )
                                            {
                                                crate::logf!(
                                                    "Failed to persist re-key CLUTCH state: {}",
                                                    e
                                                );
                                            }
                                        }

                                        // Trigger keygen for fresh re-key ceremony
                                        contact.clutch_keygen_in_progress = true;
                                        rekey_request =
                                            Some((contact.id.clone(), contact.handle_hash));
                                    } else if contact.clutch_state == ClutchState::AwaitingProof {
                                        // We're waiting for their proof, but they sent an offer. Retransmit-vs-fresh MUST be judged against the PRE-ARRIVAL keys: the fallthrough above already stored this arrival's offer in the slot, so the old slot re-read here compared the offer to ITSELF — every offer (fresh keys included) logged "Ignoring retransmit" and the reset recovery below was unreachable dead code. That plus the give-up latch (proof destroyed at the lifetime cap) was the permanent field wedge.
                                        let is_same_keys = pre_arrival_hqc
                                            .as_ref()
                                            .map(|h| *h == their_offer.hqc256_public)
                                            .unwrap_or(false);
                                        // Zombie gate: no keypairs (this branch) AND no proof left to send (gave up / budget drained / lost at resume) = this round can never conclude on our side, and the peer still offering means theirs can't either. Any offer — same keys or fresh — is the exit ramp.
                                        let unrecoverable = contact.clutch_our_eggs_proof.is_none()
                                            && contact.clutch_proof_resends_left == 0;
                                        if is_same_keys && !unrecoverable {
                                            crate::logf!("CLUTCH: Ignoring retransmit from {} (already AwaitingProof, proof still in flight)", crate::fp(&contact.handle_proof));
                                            break;
                                        }
                                        crate::logf!("CLUTCH: {} offered while we're AwaitingProof {} — discarding the wedged round and adopting their offer", crate::fp(&contact.handle_proof), if is_same_keys { "and we hold no proof to send" } else { "with fresh keys (peer reset)" });
                                        contact.discard_clutch_round();
                                        contact.clutch_proof_retry_lifetime = 0;
                                        contact.clutch_proof_gave_up = false;
                                        contact.init_clutch_slots(our_handle_hash);
                                        if let Some(slot) = contact.get_slot_mut(&their_handle_hash)
                                        {
                                            slot.offer = Some(their_offer.clone());
                                            slot.offer_device = Some(sender_pubkey);
                                        }
                                        if !contact.offer_provenances.contains(&offer_provenance) {
                                            contact.offer_provenances.push(offer_provenance);
                                        }
                                        contact.clutch_keygen_in_progress = true;
                                        rekey_request =
                                            Some((contact.id.clone(), contact.handle_hash));
                                    } else {
                                        crate::logf!("CLUTCH: Received offer from {} but no keypairs (state={}) - triggering keygen", crate::fp(&contact.handle_proof), format!("{:?}", contact.clutch_state));
                                        contact.clutch_keygen_in_progress = true;
                                        rekey_request =
                                            Some((contact.id.clone(), contact.handle_hash));
                                    }
                                }
                            }
                            break;
                        }
                    }

                    // Remove invalidated chains from memory and disk
                    for old_id in chains_to_remove {
                        // Scrub the doomed chains' history key before dropping them (re-key path — the fresh ceremony derives its own).
                        for (id, chains) in self.friendship_chains.iter_mut() {
                            if *id == old_id {
                                chains.zeroize_history_key();
                                chains.zeroize_lane_root();
                            }
                        }
                        self.friendship_chains.retain(|(id, _)| *id != old_id);
                        // Delete from disk
                        if let Some(storage) = self.storage.as_ref() {
                            if let Err(e) = crate::storage::friendship::delete_friendship_chains(
                                &old_id, storage,
                            ) {
                                crate::logf!("CLUTCH: Failed to delete old chains: {}", e);
                            }
                        }
                    }

                    // Spawn re-key keygen after releasing mutable borrow
                    if let Some((contact_id, their_handle_hash)) = rekey_request {
                        self.spawn_clutch_keygen(contact_id, our_identity_seed, their_handle_hash);
                    }

                    // Spawn deferred KEM encapsulation after releasing mutable borrow
                    if let Some((contact_id, offer, ceremony_id, conv_token, peer_addr)) =
                        kem_encap_spawn
                    {
                        self.spawn_clutch_kem_encap(
                            contact_id,
                            offer,
                            ceremony_id,
                            conv_token,
                            peer_addr,
                        );
                    }
                }

                // CLUTCH KEM response received (~31KB with 4 ciphertexts) Payload is already parsed and signature verified by status.rs
                StatusUpdate::ClutchKemResponseReceived {
                    conversation_token,
                    ceremony_id: received_ceremony_id,
                    sender_pubkey,
                    payload,
                    sender_addr: raw_sender_addr,
                } => {
                    use crate::crypto::clutch::derive_conversation_token;

                    // Normalize to port 4383 (TCP source port is ephemeral)
                    let sender_addr =
                        std::net::SocketAddr::new(raw_sender_addr.ip(), crate::PHOTON_PORT);

                    // Get our handle_hash
                    let our_handle_hash = match self
                        .session
                        .as_ref()
                        .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
                    {
                        // PARTY ID (not raw seed): the conversation token + slots key on party ids on the SEND side; the receive path must match or every friend ceremony stalls at "unknown conversation_token".
                        Some(h) => h,
                        None => {
                            #[cfg(feature = "development")]
                            #[cfg(feature = "development")]
                            crate::log("CLUTCH: No user_identity_seed available");
                            continue;
                        }
                    };
                    let our_sibling_pid = self.our_sibling_pid();

                    // Find contact by conversation_token. Party-id seam: sibling candidates token with the device-derived pid pair; the resolved "our" id shadows the seed for the whole arm.
                    let (their_handle_hash, our_handle_hash) =
                        match self.contacts.iter().find_map(|c| {
                            let our = if c.is_sibling {
                                our_sibling_pid?
                            } else {
                                our_handle_hash
                            };
                            (derive_conversation_token(&[our, c.handle_hash]) == conversation_token)
                                .then_some((c.handle_hash, our))
                        }) {
                            Some(pair) => pair,
                            None => {
                                crate::logf!(
                                "CLUTCH: Received KEM response with unknown conversation_token {}",
                                hex::encode(&conversation_token[..8])
                            );
                                continue;
                            }
                        };

                    crate::logf!(
                        "CLUTCH: Received KEM response (VSF verified) from {} tok={}...",
                        sender_addr,
                        hex::encode(&conversation_token[..8])
                    );

                    // Gate: sender must be a currently-trusted device of this contact (fold-respecting). See the offer gate for the widen/revoke rationale.
                    let sender_known = self
                        .contacts
                        .iter()
                        .find(|c| c.handle_hash == their_handle_hash)
                        .map(|c| self.sender_trusted_for(c, &sender_pubkey));
                    match sender_known {
                        None => {
                            #[cfg(feature = "development")]
                            #[cfg(feature = "development")]
                            crate::log("CLUTCH: Received KEM response from unknown contact");
                            continue;
                        }
                        Some(false) => {
                            crate::logf!(
                                "CLUTCH: KEM from untrusted/removed device {} — dropping",
                                hex::encode(&sender_pubkey[..8])
                            );
                            continue;
                        }
                        Some(true) => {}
                    }

                    // The payload is already parsed
                    let their_kem = payload;

                    // Find contact by handle_hash
                    for (_idx, contact) in self.contacts.iter_mut().enumerate() {
                        if contact.handle_hash == their_handle_hash {
                            // A relayed message (RELAY_ADDR sentinel) carries no reachable peer address — skip address-learning (storing the sentinel as contact.ip would poison direct sends) and mark the link relay-only, which lights the presence lime-yellow. A direct message clears the flag: direct always wins. Otherwise inbound DATA elects the sending device ACTIVE (the fleet reply-TX rule) and seeds its endpoint, so contact-level addressing follows the device actually talking to us.
                            if sender_addr == crate::network::status::RELAY_ADDR {
                                contact.reached_via_relay = true;
                            } else {
                                contact.reached_via_relay = false;
                                contact.ip = Some(sender_addr);
                                contact.active_device = Some(sender_pubkey);
                                let pub_src = !is_private_addr(&sender_addr.ip());
                                let ep = contact.endpoint_mut(&sender_pubkey);
                                if pub_src {
                                    ep.public = Some(sender_addr);
                                } else {
                                    ep.lan = Some(sender_addr);
                                }
                            }
                            // Authenticated CLUTCH traffic from them ⇒ reachable right now ⇒ show online immediately, don't wait for the next pong.
                            if !contact.is_online {
                                contact.is_online = true;
                                changed = true;
                                crate::logf!(
                                    "Status: {} is now ONLINE (CLUTCH)",
                                    crate::fp(&contact.handle_proof)
                                );
                            }

                            // Verify ceremony_id matches (if we have one)
                            if let Some(our_ceremony_id) = contact.ceremony_id {
                                if received_ceremony_id != our_ceremony_id {
                                    crate::logf!("CLUTCH: ceremony_id mismatch! Received {:02x}{:02x}..., expected {:02x}{:02x}...", received_ceremony_id[0], received_ceremony_id[1], our_ceremony_id[0], our_ceremony_id[1]);
                                    continue;
                                }
                            } else {
                                // No ceremony_id yet - check if we have keypairs and if KEM targets them This happens when keypairs are loaded from disk but offers not yet exchanged
                                if let Some(our_keys_cloned) = contact.clutch_our_keypairs.clone() {
                                    let our_hqc_prefix: [u8; 8] =
                                        our_keys_cloned.hqc256_public[..8].try_into().unwrap();
                                    let all_zeros = their_kem.target_hqc_pub_prefix == [0u8; 8];
                                    if !all_zeros
                                        && their_kem.target_hqc_pub_prefix != our_hqc_prefix
                                    {
                                        // KEM targets different keys - truly stale, discard
                                        crate::logf!("CLUTCH: KEM response from {} targets old keys (HQC {}) - discarding", crate::fp(&contact.handle_proof), hex::encode(&their_kem.target_hqc_pub_prefix));
                                        break;
                                    }
                                    // KEM targets our current keys but we don't have ceremony_id yet — which means we haven't processed THEIR offer yet (ceremony_id derives from both offers). We can't complete without their offer (no offer → can't encapsulate our KEM → our slot never completes). QUEUE the KEM and wait for their offer to arrive; the ClutchOfferReceived path drains clutch_pending_kem once both offers are in and ceremony_id is derived. Their offer is a reliable PT stream now, so it WILL arrive — no deadlock. (The old "adopt ceremony_id + decapsulate + break" shortcut left our own slot incomplete and hung CLUTCH Pending forever.)
                                    let _ = (our_keys_cloned, received_ceremony_id);
                                    crate::logf!("CLUTCH: KEM from {} arrived before their offer/ceremony_id - queuing until offer arrives", crate::fp(&contact.handle_proof));
                                    contact.clutch_pending_kem = Some(their_kem.clone());
                                    break;
                                } else {
                                    // No keypairs at all - stale KEM encrypted to unknown keys
                                    crate::logf!("CLUTCH: KEM response from {} arrived before keygen - discarding (encrypted to old keys)", crate::fp(&contact.handle_proof));
                                    break;
                                }
                            }

                            // Initialize slots if needed
                            if contact.clutch_slots.is_empty() {
                                contact.init_clutch_slots(our_handle_hash);
                            }

                            // Verify KEM response targets our CURRENT HQC public key This prevents panics from stale KEM responses encrypted to old keys
                            if let Some(ref our_keys) = contact.clutch_our_keypairs {
                                let our_hqc_prefix: [u8; 8] =
                                    our_keys.hqc256_public[..8].try_into().unwrap();
                                let all_zeros = their_kem.target_hqc_pub_prefix == [0u8; 8];
                                if !all_zeros && their_kem.target_hqc_pub_prefix != our_hqc_prefix {
                                    crate::logf!("CLUTCH: Stale KEM response from {} - target HQC {} != our HQC {} (discarding)", crate::fp(&contact.handle_proof), hex::encode(&their_kem.target_hqc_pub_prefix), hex::encode(&our_hqc_prefix));
                                    break;
                                }
                            }

                            // Duplicate KEM response (peer retransmit): the slot already holds their secrets — drop before spending anything. Pre-2026-08-15 every duplicate re-ran all 8 decapsulations INLINE, so a retransmit storm compounded the very UI freeze that was stalling our reply.
                            if contact
                                .get_slot(&their_handle_hash)
                                .map(|s| s.kem_secrets_from_them.is_some())
                                .unwrap_or(false)
                            {
                                crate::logf!(
                                    "CLUTCH: duplicate KEM response from {} — slot already decapped, dropped",
                                    crate::fp(&contact.handle_proof)
                                );
                                break;
                            }

                            // Hand the KEM response to the decap job — 8 PQ opens are NOT UI-thread work (2026-08-15). The drain (check_clutch_kem_decaps) stores the secrets, backfills our offer, and fires the completion check. An in-flight decap parks the payload in clutch_pending_kem; the keygen tick re-offers it once the flag clears.
                            if let Some(ref local_keys) = contact.clutch_our_keypairs {
                                if contact.clutch_kem_decap_in_progress {
                                    contact.clutch_pending_kem = Some(their_kem.clone());
                                    crate::logf!(
                                        "CLUTCH: decap already in flight for {} — KEM response parked",
                                        crate::fp(&contact.handle_proof)
                                    );
                                } else {
                                    contact.clutch_kem_decap_in_progress = true;
                                    decap_spawns.push((
                                        contact.id.clone(),
                                        their_kem.clone(),
                                        local_keys.clone(),
                                    ));
                                    changed = true;
                                }
                            } else {
                                crate::logf!(
                                    "CLUTCH: Received KEM response but no keypairs for {}",
                                    crate::fp(&contact.handle_proof)
                                );
                            }
                            break;
                        }
                    }
                }

                // CLUTCH complete proof received (~200 bytes with eggs_proof) Both parties exchange this to verify they derived identical eggs
                StatusUpdate::ClutchCompleteReceived {
                    conversation_token,
                    ceremony_id: received_ceremony_id,
                    sender_pubkey,
                    payload,
                    sender_addr: raw_sender_addr,
                } => {
                    use crate::crypto::clutch::derive_conversation_token;
                    use crate::types::ClutchState;

                    // Normalize to port 4383 (TCP source port is ephemeral)
                    let sender_addr =
                        std::net::SocketAddr::new(raw_sender_addr.ip(), crate::PHOTON_PORT);

                    crate::logf!(
                        "CLUTCH: Received complete proof (VSF verified) from {} proof={}...",
                        sender_addr,
                        hex::encode(&payload.eggs_proof[..8])
                    );

                    // Find contact by conversation_token. Party-id seam: BOTH participants token on their PARTY IDS (send derives ours via identity_party_id; matching on the raw seed here was the "unknown conversation_token" stall). Sibling candidates token with the device-derived pid pair. (Our id isn't needed downstream — completion derives it internally — so it's discarded.)
                    let our_sibling_pid = self.our_sibling_pid();
                    let our_handle_hash = match self
                        .session
                        .as_ref()
                        .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
                    {
                        Some(h) => h,
                        None => continue,
                    };
                    let (their_handle_hash, _our_handle_hash) = match self.contacts.iter().find_map(
                        |c| {
                            let our = if c.is_sibling {
                                our_sibling_pid?
                            } else {
                                our_handle_hash
                            };
                            (derive_conversation_token(&[our, c.handle_hash]) == conversation_token)
                                .then_some((c.handle_hash, our))
                        },
                    ) {
                        Some(pair) => pair,
                        None => {
                            crate::logf!("CLUTCH: Received complete proof with unknown conversation_token {}", hex::encode(&conversation_token[..8]));
                            continue;
                        }
                    };

                    // Gate: sender must be a currently-trusted device of this contact (fold-respecting). See the offer gate for the widen/revoke rationale.
                    let sender_known = self
                        .contacts
                        .iter()
                        .find(|c| c.handle_hash == their_handle_hash)
                        .map(|c| self.sender_trusted_for(c, &sender_pubkey));
                    match sender_known {
                        None => {
                            #[cfg(feature = "development")]
                            #[cfg(feature = "development")]
                            crate::log("CLUTCH: Received proof from unknown contact");
                            continue;
                        }
                        Some(false) => {
                            crate::logf!(
                                "CLUTCH: proof from untrusted/removed device {} — dropping",
                                hex::encode(&sender_pubkey[..8])
                            );
                            continue;
                        }
                        Some(true) => {}
                    }

                    // Find contact and process proof
                    let mut newly_complete_idx: Option<usize> = None;
                    for (contact_idx, contact) in self.contacts.iter_mut().enumerate() {
                        if contact.handle_hash == their_handle_hash {
                            // A relayed message (RELAY_ADDR sentinel) carries no reachable peer address — skip address-learning (storing the sentinel as contact.ip would poison direct sends) and mark the link relay-only, which lights the presence lime-yellow. A direct message clears the flag: direct always wins. Otherwise inbound DATA elects the sending device ACTIVE (the fleet reply-TX rule) and seeds its endpoint, so contact-level addressing follows the device actually talking to us.
                            if sender_addr == crate::network::status::RELAY_ADDR {
                                contact.reached_via_relay = true;
                            } else {
                                contact.reached_via_relay = false;
                                contact.ip = Some(sender_addr);
                                contact.active_device = Some(sender_pubkey);
                                let pub_src = !is_private_addr(&sender_addr.ip());
                                let ep = contact.endpoint_mut(&sender_pubkey);
                                if pub_src {
                                    ep.public = Some(sender_addr);
                                } else {
                                    ep.lan = Some(sender_addr);
                                }
                            }
                            // Authenticated CLUTCH traffic from them ⇒ reachable right now ⇒ show online immediately, don't wait for the next pong.
                            if !contact.is_online {
                                contact.is_online = true;
                                changed = true;
                                crate::logf!(
                                    "Status: {} is now ONLINE (CLUTCH)",
                                    crate::fp(&contact.handle_proof)
                                );
                            }
                            // We just received + VSF-verified an authenticated message from them — they're reachable NOW, so reflect online immediately instead of waiting for the next pong (fixes "CLUTCH completed but still shows offline").
                            if !contact.is_online {
                                contact.is_online = true;
                                changed = true;
                                crate::logf!(
                                    "Status: {} is now ONLINE (CLUTCH complete)",
                                    crate::fp(&contact.handle_proof)
                                );
                            }

                            // ROUND SCOPING (a permanent-Pending stall): a proof is only meaningful within ITS ceremony round — the wire carries ceremony_id for exactly this, but it was parsed and discarded, so a proof from a superseded round (offer churn, address change, an unwiped peer replaying old state) got compared against OUR round and manufactured "PROOF MISMATCH" out of ordinary echo. If we know our round and theirs differs, drop it here: never stored, never compared. Their CURRENT round's proof rides the resend budget and arrives on its own.
                            if let Some(our_cid) = contact.ceremony_id {
                                if received_ceremony_id != our_cid {
                                    crate::logf!("CLUTCH: proof from {} is for round {}… ours is {}… — cross-round echo dropped", crate::fp(&contact.handle_proof), hex::encode(&received_ceremony_id[..4]), hex::encode(&our_cid[..4]));
                                    break;
                                }
                            }

                            match contact.clutch_state {
                                ClutchState::AwaitingProof => {
                                    // We have our proof - verify theirs matches
                                    if let Some(our_proof) = contact.clutch_our_eggs_proof {
                                        if payload.eggs_proof == our_proof {
                                            // SUCCESS! Both parties computed same eggs
                                            crate::logf!(
                                                "CLUTCH: Proof verified with {}! ✓ proof={}...",
                                                crate::fp(&contact.handle_proof),
                                                hex::encode(&our_proof[..8])
                                            );
                                            contact.clutch_state = ClutchState::Complete;
                                            contact.clutch_mismatch_streak = 0;
                                            contact.clutch_completed_at =
                                                Some(std::time::Instant::now()); // arm the post-completion re-key cooldown (before the ~1s-later weave)
                                                                                 // Fresh ceremony = fresh chain: void any prior weave seal so the probe refires (see the twin reset at the Early-proof-verified site for the full deadlock story).
                                            contact.chain_woven = false;
                                            contact.probe_sent = false;
                                            contact.void_weave_seal_from_previous_chain();
                                            contact.chain_advanced_by_ack = false;
                                            newly_complete_idx = Some(contact_idx);
                                            // Store their HQC pub prefix to detect stale offers after restart
                                            if let Some(their_slot) =
                                                contact.get_slot(&contact.handle_hash)
                                            {
                                                if let Some(ref their_offer) = their_slot.offer {
                                                    let prefix: [u8; 8] = their_offer.hqc256_public
                                                        [..8]
                                                        .try_into()
                                                        .unwrap_or_default();
                                                    contact.completed_their_hqc_prefix =
                                                        Some(prefix);
                                                }
                                            }
                                            // Keep our proof + resend budget: we just verified theirs, but ours may still be in flight or dropped. ping_contacts drains the budget over the next few cycles, then clears it — so neither side strands.
                                            contact.clutch_their_eggs_proof = None;
                                            changed = true;

                                            // NOTE: Don't clear PT sends here - our ClutchComplete proof might still be in flight to them. Let it finish.

                                            // Save Complete state to disk immediately
                                            if let Some(storage) = self.storage.as_ref() {
                                                if let Err(e) =
                                                    crate::storage::contacts::save_contact(
                                                        contact, storage,
                                                    )
                                                {
                                                    crate::logf!(
                                                        "Failed to save Complete state: {}",
                                                        e
                                                    );
                                                } else {
                                                    crate::logf!(
                                                        "CLUTCH: Saved {} Complete state to disk",
                                                        crate::fp(&contact.handle_proof)
                                                    );
                                                }
                                            }
                                        } else {
                                            // Proof mismatch — NEVER panic; not an attack signal (a forged proof can't pass the read_verified gate). We NO LONGER torch here — the clutch does not rotate (see the matching handler in check_clutch_ceremonies). Torching re-keyed → new time-based provenance → new ceremony_id the peer chased, and over the relay both sides torched faster than the round-trip, staying one generation apart forever. With the clutch pinned, reaching here means a TRANSIENT (eggs computed before the full KEM exchange, or a proof crossed in flight). Keep every ceremony input; the resend redelivers and the next completion recomputes matching eggs from the stable slots.
                                            crate::logf!("CLUTCH: ⚠ PROOF MISMATCH with {} (same round) ours={}... theirs={}... — NOT re-keying (clutch pinned); awaiting their correct proof", crate::fp(&contact.handle_proof), hex::encode(&our_proof[..8]), hex::encode(&payload.eggs_proof[..8]));
                                            // One crossed-in-flight proof is a transient; a STREAK of same-round mismatches is a COMPETING INSTANCE — a third device muxing the shared slots computed different eggs under this round (the 11/12 deadlock: each side reads the other's re-sent foreign proof as "my ClutchComplete was lost" and they re-arm each other forever). §4.2 arbitration: the in-flight side YIELDS — discard this round and stand down for one TTL, so chain replication (adopt-iff-newer) or the next single-owner claimed round converges the friendship. A completed instance always outranks an in-flight one; simultaneous double-yield lands both in Pending where the ceremony-claim LWW serializes the re-run.
                                            contact.clutch_mismatch_streak += 1;
                                            const MISMATCH_YIELD_STREAK: u16 = 3;
                                            if contact.clutch_mismatch_streak >= MISMATCH_YIELD_STREAK {
                                                crate::logf!("CLUTCH: competing ceremony instance with {} ({} same-round mismatches) — yielding this round; replication or a fresh claimed round converges", crate::fp(&contact.handle_proof), contact.clutch_mismatch_streak);
                                                contact.clutch_mismatch_streak = 0;
                                                contact.discard_clutch_round();
                                                // Stand down for a JITTERED slice of the round TTL (50-100%): the yield exists to make room for adoption, not to race a fresh keygen into the same collision — and two devices yielding on identical fixed timers would re-collide on the same beat forever.
                                                contact.clutch_round_started =
                                                    Some(vsf::eagle_time_oscillations()
                                                        - super::CLUTCH_ROUND_TTL_OSC
                                                        + crate::jitter(
                                                            super::CLUTCH_ROUND_TTL_OSC,
                                                        ));
                                            }
                                            changed = true;
                                        }
                                    } else {
                                        // Race condition: proof arrived before check_clutch_ceremonies processed our ceremony result. Store theirs for when we're ready.
                                        crate::logf!("CLUTCH: Storing early proof from {} (AwaitingProof but our result not processed yet)", crate::fp(&contact.handle_proof));
                                        contact.clutch_their_eggs_proof = Some(payload.eggs_proof);
                                        contact.clutch_their_proof_ceremony =
                                            Some(received_ceremony_id);
                                        changed = true;
                                    }
                                }
                                ClutchState::Pending => {
                                    // We haven't computed our proof yet - store theirs for later
                                    crate::logf!("CLUTCH: Storing early proof from {} (we're still in Pending)", crate::fp(&contact.handle_proof));
                                    contact.clutch_their_eggs_proof = Some(payload.eggs_proof);
                                    contact.clutch_their_proof_ceremony =
                                        Some(received_ceremony_id);
                                    changed = true;
                                }
                                ClutchState::Complete => {
                                    // We're Complete but the peer is STILL sending its proof — that means our ClutchComplete never reached them (a dropped proof strands them in AwaitingProof forever, since we'd otherwise ignore the duplicate). Treat the duplicate as an implicit re-request: re-arm our proof-resend budget so the next ping cycle re-sends our ClutchComplete. This is the recovery half of the asymmetric-completion bug (the other half is the AwaitingProof side re-sending its proof while the peer is online).
                                    if contact.chain_woven {
                                        // The chain is proven end-to-end (probe exchanged + ACKed), so a duplicate proof is just late network echo — stop rebroadcasting.
                                        crate::logf!("CLUTCH: Ignoring duplicate proof from {} — chain already woven, rebroadcast retired", crate::fp(&contact.handle_proof));
                                    } else if contact.clutch_proof_gave_up {
                                        // We already gave up on this peer (lifetime cap): a mismatched ghost re-sending its proof forever must NOT re-arm us — that's the mirror of the AwaitingProof storm and the whole reason for the cap. Swallow it silently (no relay spew back).
                                    } else if contact
                                        .clutch_our_eggs_proof
                                        .is_some_and(|ours| ours != payload.eggs_proof)
                                    {
                                        // A completed ceremony's verified proof EQUALS ours ("both parties computed same eggs") — so a duplicate carrying a DIFFERENT value is not our lost ClutchComplete, it's a COMPETING INSTANCE (a third device's eggs under the muxed slots: the 11/12 deadlock's Complete half). Re-arming answers a foreign instance forever; refuse instead. The completed instance outranks — the sender yields on its own mismatch streak and converges via replication or a fresh claimed round.
                                        crate::logf!("CLUTCH: proof from {} belongs to a competing instance (ours={}... theirs={}...) — Complete stands, not re-arming", crate::fp(&contact.handle_proof), hex::encode(&contact.clutch_our_eggs_proof.unwrap()[..8]), hex::encode(&payload.eggs_proof[..8]));
                                    } else if contact.clutch_our_eggs_proof.is_some()
                                        && contact.ceremony_id.is_some()
                                    {
                                        // Count each re-arm toward the same lifetime cap (do NOT reset it — this is not a fresh round, it's the peer re-requesting). Past the cap, latch gave-up so the ping-and-answer storm terminates.
                                        const PROOF_RETRY_LIFETIME_CAP: u16 = 40;
                                        if contact.clutch_proof_retry_lifetime
                                            >= PROOF_RETRY_LIFETIME_CAP
                                        {
                                            contact.clutch_proof_gave_up = true;
                                            contact.clutch_our_eggs_proof = None;
                                            changed = true;
                                            crate::logf!("CLUTCH: giving up proof re-arm to {} — duplicate storm past the lifetime cap (peer can't place our proof); remove & re-add", crate::fp(&contact.handle_proof));
                                        } else {
                                            contact.clutch_proof_resends_left = 5;
                                            contact.clutch_proof_retry_lifetime += 1;
                                            changed = true;
                                            crate::logf!("CLUTCH: Re-arming proof resend to {} — they re-sent their proof (our ClutchComplete was likely lost)", crate::fp(&contact.handle_proof));
                                        }
                                    } else {
                                        crate::logf!("CLUTCH: Duplicate proof from {} but our proof/ceremony cleared — cannot re-send", crate::fp(&contact.handle_proof));
                                    }
                                }
                            }
                            break;
                        }
                    }
                    // If this proof took the contact to Complete, fire the one hidden chain-weave probe — deferred past the outer `checker` borrow like the other helpers.
                    if let Some(idx) = newly_complete_idx {
                        chain_probe_indices.push(idx);
                    }
                }

                // LAN peer discovered via broadcast (NAT hairpinning workaround)
                StatusUpdate::LanPeerDiscovered {
                    device_pubkey,
                    handle_proof,
                    local_ip,
                    port,
                } => {
                    // An OWN-handle beacon from another device = a machine on this network claiming to be part of (or asking to join) OUR fleet — the proximity evidence the AddDevice tap list runs on. Record it and light any matching candidate immediately; the registry entry it must match is still signature-verified, so a LAN squatter with no valid bindreq never becomes tappable.
                    let own_hp = self.session.as_ref().map(|s| s.handle_proof);
                    if Some(handle_proof) == own_hp {
                        if let Some(dk) = device_pubkey {
                            let now = std::time::Instant::now();
                            self.lan_heard
                                .retain(|(_, t)| now.duration_since(*t) < LAN_HEARD_FRESH);
                            match self.lan_heard.iter_mut().find(|(k, _)| *k == dk) {
                                Some(e) => e.1 = now,
                                None => {
                                    self.lan_heard.push((dk, now));
                                    crate::logf!(
                                        "LAN: own-handle device {} broadcasting on this network",
                                        hex::encode(&dk[..4])
                                    );
                                }
                            }
                            if matches!(self.state, AppState::AddDevice) {
                                for c in self.add_device_candidates.iter_mut() {
                                    if c.req.device_pubkey == dk && !c.heard_lan {
                                        c.heard_lan = true;
                                        changed = true;
                                    }
                                }
                            }
                        }
                    }
                    // WOODS ADD (docs/offgrid.md open house): a pending off-grid add whose derived proof matches this beacon — the beacon IS the registry record (hp in the provenance, device key in `ke`, address in the source). Create the contact and the normal machinery takes over: ping → pong → CLUTCH over the group.
                    if let Some((handle, want_hp)) = self.pending_woods_add.as_ref() {
                        if *want_hp == handle_proof
                            && !self.contacts.iter().any(|c| c.handle_proof == handle_proof)
                        {
                            if let Some(dk) = device_pubkey {
                                let handle = handle.clone();
                                crate::logf!(
                                    "add-friend: {} found NEARBY (off-grid) — contact created from the beacon",
                                    handle
                                );
                                let ht = crate::types::HandleText::new(&handle);
                                let mut contact = crate::types::Contact::new(
                                    ht,
                                    handle_proof,
                                    crate::types::DevicePubkey::from_bytes(dk),
                                );
                                let addr = std::net::SocketAddr::new(
                                    std::net::IpAddr::V4(local_ip),
                                    port,
                                );
                                if crate::network::traverse::gather::is_wfd_subnet(local_ip) {
                                    contact.p2p_addr = Some(addr);
                                } else {
                                    contact.local_ip = Some(local_ip);
                                    contact.local_port = Some(port);
                                }
                                self.contacts.push(contact);
                                if let (Some(storage), Some(c)) =
                                    (self.storage.as_ref(), self.contacts.last())
                                {
                                    if let Err(e) =
                                        crate::storage::contacts::save_contact(c, storage)
                                    {
                                        crate::logf!("add-friend: woods contact save failed: {}", e);
                                    }
                                }
                                self.ready_toast = Some(format!("found {handle} nearby"));
                                self.pending_woods_add = None;
                                // Found them: quiet the cleartext beacon; the group stays for the ceremony (DRAINED tears it down later).
                                crate::network::wfd::stop_open_house(true);
                                lan_ping_indices.push(self.contacts.len() - 1);
                                wfd_beacon_reply = Some(addr);
                                changed = true;
                                continue;
                            }
                        }
                    }
                    // Find contact by handle_proof and store their LAN IP + port. Siblings AND the self-contact are skipped — an own-hp broadcast carries only (hp, port) with no device disambiguation, so it can't say WHICH of our devices it came from; sibling addresses flow via FGTW peer rows + pong source addresses instead.
                    // A source inside the Wi-Fi Direct group subnet routes to `p2p_addr`, NOT `local_ip` — a p2p address must never masquerade as an infra-LAN address (it would hit the foreign-/24 gate and vanish, or survive teardown as a black hole).
                    let is_p2p = crate::network::traverse::gather::is_wfd_subnet(local_ip);
                    for (idx, contact) in self.contacts.iter_mut().enumerate() {
                        if !contact.is_sibling
                            && contact.remote_count(&our_handle_hash) > 0
                            && contact.handle_proof == handle_proof
                        {
                            if is_p2p {
                                let addr = std::net::SocketAddr::new(
                                    std::net::IpAddr::V4(local_ip),
                                    port,
                                );
                                if contact.p2p_addr != Some(addr) {
                                    crate::logf!(
                                        "WFD: {} on the group at {}",
                                        crate::fp(&contact.handle_proof),
                                        addr
                                    );
                                    contact.p2p_addr = Some(addr);
                                    lan_ping_indices.push(idx);
                                    changed = true;
                                    // Answer-back: our reply beacon teaches THEM our group address. Gated on the learn edge (a repeat beacon changes nothing), which quenches the beacon ping-pong after one round trip each way.
                                    wfd_beacon_reply = Some(addr);
                                }
                                break;
                            }
                            let old_local = contact.local_ip;
                            let old_port = contact.local_port;
                            contact.local_ip = Some(local_ip);
                            contact.local_port = Some(port);
                            if old_local != Some(local_ip) || old_port != Some(port) {
                                crate::logf!(
                                    "LAN: Discovered {} at local {}:{}",
                                    crate::fp(&contact.handle_proof),
                                    local_ip,
                                    port
                                );
                                // Ping immediately so we don't wait for next scheduled cycle
                                lan_ping_indices.push(idx);
                                changed = true;
                            }
                            break;
                        }
                    }
                }
                // A peer asked for our avatar. Policy: reply ONLY if they are a MUTUAL contact — i.e. a completed CLUTCH ceremony, which is cryptographically impossible unless both added each other. A friend gets our avatar straight from us; anyone else is ignored (they fall back to FGTW, or get nothing). We reply with our OWN avatar VSF bytes.
                StatusUpdate::AvatarRequestReceived {
                    sender_pubkey,
                    sender_addr,
                } => {
                    let is_mutual = self
                        .contacts
                        .iter()
                        .any(|c| c.knows_device(&sender_pubkey.key) && c.is_mutual());
                    if !is_mutual {
                        crate::log(
                            "Avatar: ignoring avatar request from a non-mutual peer (not Complete)",
                        );
                    } else if let (Some(session), Some(storage), Some(checker)) = (
                        self.session.as_ref(),
                        self.storage.as_ref(),
                        self.status_checker.as_ref(),
                    ) {
                        // Serve the PIN-keyed copy, never the vault bytes: the vault holds the avatar under a seed-derived key that no friend can ever have, so shipping it verbatim hands them a blob that verifies and then fails to decrypt. `avatar_vsf_for_friend` re-encrypts under the pin we handed them, which is byte-for-byte what the FGTW wall serves — including the preferred-name slot.
                        let our_name = self
                            .fleet_settings
                            .as_ref()
                            .and_then(|fs| fs.effective("profile.name"))
                            .and_then(crate::storage::fleet_settings::as_text)
                            .filter(|n| !n.is_empty());
                        let served = self
                            .ensure_avatar_pin_readonly()
                            .ok_or_else(|| "no avatar pin yet".to_string())
                            .and_then(|pin| {
                                self.device_keypair
                                    .as_ref()
                                    .ok_or_else(|| "no device key".to_string())
                                    .and_then(|kp| {
                                        crate::ui::avatar::avatar_vsf_for_friend(
                                            &kp.secret,
                                            &session.identity_seed,
                                            &pin,
                                            our_name.as_deref(),
                                            storage,
                                        )
                                    })
                            });
                        match served {
                            Ok(avatar_vsf) => {
                                crate::logf!(
                                    "Avatar: sending our avatar to mutual peer ({} bytes, pin-keyed)",
                                    avatar_vsf.len()
                                );
                                checker.send_avatar_response(
                                    crate::network::status::AvatarResponseSend {
                                        peer_addr: sender_addr,
                                        recipient_pubkey: *sender_pubkey.as_bytes(),
                                        avatar_vsf,
                                    },
                                );
                            }
                            Err(e) => {
                                crate::logf!(
                                    "Avatar: mutual peer requested avatar — not serving: {}",
                                    e
                                )
                            }
                        }
                    }
                }
                // A peer sent us their avatar. Policy: install ONLY if the responder is a MUTUAL (Complete) contact — otherwise anyone could push us an arbitrary avatar. The wire layer already verified the bytes are signed by responder_pubkey; here we bind that pubkey to a friendship before trusting it. Decode + cache + install on that contact.
                StatusUpdate::AvatarReceived {
                    responder_pubkey,
                    avatar_vsf,
                    sender_addr: _,
                } => {
                    let target = self
                        .contacts
                        .iter()
                        .position(|c| c.knows_device(&responder_pubkey.key) && c.is_mutual());
                    match target {
                        None => crate::log(
                            "Avatar: ignoring avatar from a non-mutual peer (not a Complete contact)",
                        ),
                        Some(idx) => {
                            let party_id = self.contacts[idx].handle_hash;
                            let owner_hp = self.contacts[idx].handle_proof;
                            let mut pin_key = [0u8; 32];
                            pin_key.copy_from_slice(&self.contacts[idx].avatar_pin[..32]);
                            // OFF-THREAD: the AVIF-in-VSF decode (dav1d) + the cache write ran inline on the render thread. Small avatars are cheap, but a large one hitches the frame — mirror the FGTW download path: a worker decodes with the PINNED key and caches (party-id scope, so a restart shows it without a round-trip), then delivers the display pixels back over avatar_dl_tx keyed by owner handle_proof, which the existing drain installs on the matching contact next tick.
                            let storage = self.storage.as_ref().map(std::sync::Arc::clone);
                            let tx = self.avatar_dl_tx.clone();
                            std::thread::spawn(move || {
                                // The direct-served VSF carries the friend's preferred name beside the pixels (same pin).
                                let name =
                                    crate::ui::avatar::avatar_name_with_key(&avatar_vsf, &pin_key);
                                let pixels = crate::ui::avatar::load_avatar_from_bytes_with_key(
                                    &avatar_vsf,
                                    &pin_key,
                                )
                                .map(|(_, vsf_rgb)| {
                                    if let Some(st) = storage.as_ref() {
                                        let _ = crate::ui::avatar::save_avatar_to_cache_from_seed(
                                            &party_id, &avatar_vsf, st,
                                        );
                                    }
                                    vsf_rgb
                                });
                                if pixels.is_none() {
                                    crate::log("Avatar: failed to decode peer avatar bytes");
                                }
                                let _ = tx.send(crate::ui::avatar::AvatarDownloadResult {
                                    owner: Some(owner_hp),
                                    pixels,
                                    name,
                                });
                            });
                        }
                    }
                }

                // A friend (post-reset / new device) is asking for conversation history. Serve one newest-first page from our rārangi rows, sealed under the friendship history key. Authorization is OURS to do (the RX worker only verified the signature): the signer must be a known device of the contact this conversation belongs to, and mutual.
                StatusUpdate::HistoryRequestReceived {
                    conversation_token,
                    before_osc,
                    limit,
                    request_id,
                    sent_osc,
                    sender_pubkey,
                    sender_addr,
                } => {
                    let now = vsf::eagle_time_oscillations();
                    // Staleness cap: a hist_req older than ~10 min is a replay or a badly delayed duplicate — pages are useless to an attacker (sealed) but serving costs us I/O.
                    const HIST_STALE_OSC: i64 = 600 * crate::OSC_PER_SEC;
                    let stale = sent_osc != 0 && now.saturating_sub(sent_osc) > HIST_STALE_OSC;

                    // Per-conversation dedup (rid) + cadence cap (≥500ms between served pages).
                    let entry = self
                        .history_serve
                        .entry(conversation_token)
                        .or_insert_with(|| (0, std::collections::VecDeque::new()));
                    let duplicate = entry.1.contains(&request_id);
                    let too_fast = now.saturating_sub(entry.0) < crate::OSC_PER_SEC / 2;

                    if !stale && !duplicate && !too_fast {
                        entry.0 = now;
                        entry.1.push_back(request_id);
                        while entry.1.len() > 8 {
                            entry.1.pop_front();
                        }

                        // FRIEND route: bind token → chains (history key) → the OTHER participant → contact, and require the requesting device to belong to that exact contact + be mutual. Participants are PARTY IDS (chains key on them since the pin-set migration), so "other" resolves against OUR party id — comparing against the raw seed matched nothing and made `other` sort-order dependent.
                        let our_pid = self
                            .session
                            .as_ref()
                            .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed));
                        let key_and_other = self
                            .friendship_chains
                            .iter()
                            .find(|(_, c)| c.conversation_token == conversation_token)
                            .and_then(|(_, c)| {
                                let key = c.history_key().copied()?;
                                let other = c
                                    .participants()
                                    .iter()
                                    .find(|p| Some(**p) != our_pid)
                                    .copied()?;
                                Some((key, other))
                            });
                        let friend_route: Option<(usize, [u8; 32], Option<u64>)> = key_and_other
                            .and_then(|(key, other)| {
                                self.contacts
                                    .iter()
                                    .position(|c| {
                                        // Friend chains only — a sibling chain's "other ≠ our pid" resolution is ambiguous (both participant pids differ); sibling requests take the FLEET route below.
                                        !c.is_sibling
                                            && c.handle_hash == other
                                            && c.knows_device(&sender_pubkey.key)
                                            && c.is_mutual()
                                    })
                                    .map(|idx| (idx, key, None))
                            });
                        // FLEET route (fleet history sync): the requester is one of OUR OWN devices — fold-trusted sibling — asking for any conversation we hold. Sealed under the EPOCH hist_page key (the B-arc re-seal), with the epoch on the frame; no spine yet = don't serve — the requester re-requests after our 45s-bounded bootstrap lands, exactly the chain_sync hold rule. The token resolves by DERIVATION from party ids (no chain needed), so a conversation the sibling only knows from the roster — or the self notes conversation — still serves.
                        let route = friend_route.or_else(|| {
                            let sender_is_sibling = self.contacts.iter().any(|c| {
                                c.is_sibling && !c.locked_out && c.knows_device(&sender_pubkey.key)
                            });
                            if !sender_is_sibling {
                                return None;
                            }
                            let (k, epoch) = self.fleet_epoch?;
                            let key =
                                crate::crypto::clutch::fleet_epoch_seal_key(&epoch, b"hist_page");
                            let idx =
                                self.contact_idx_for_conversation_token(&conversation_token)?;
                            Some((idx, key, Some(k)))
                        });

                        if let (Some((idx, key, serve_ek)), Some(storage), Some(checker)) =
                            (route, self.storage.as_ref(), self.status_checker.as_ref())
                        {
                            // OFF THE RENDER THREAD. Serving a page reads and decrypts up to 50 vault rows and then seals them — measured at 2195ms inline, which is what a peer's backfill felt like from inside our own UI. Everything the work needs is copied here (ids, keys, an Arc of storage, a cloned dispatch sender) and the whole read-seal-send runs on a worker; nothing it produces touches app state, so there is no result to drain back.
                            let their_seed = self.contacts[idx].handle_hash;
                            let page_limit = (limit as usize)
                                .clamp(1, crate::network::history_pages::MAX_PAGE_ROWS);
                            let storage = Arc::clone(storage);
                            let dispatch = checker.history_dispatch();
                            let kp = self
                                .device_keypair
                                .as_ref()
                                .expect("device_keypair set in init");
                            let device_pubkey = *kp.public.as_bytes();
                            let device_secret = *kp.secret.as_bytes();
                            let recipient = *sender_pubkey.as_bytes();
                            queue_job(&self.seal_job_tx, move || {
                                use crate::network::history_pages::{
                                    seal_history_page, HistoryPagePlain, HistoryRow,
                                };
                                match crate::storage::contacts::load_message_page_before(
                                    &their_seed,
                                    before_osc,
                                    page_limit,
                                    crate::network::history_pages::MAX_PAGE_BYTES,
                                    &storage,
                                ) {
                                    Ok((rows, more)) => {
                                        // Cursor progresses over ALL returned rows (probe rows included) so a probe-heavy stretch can't stall the walk; the probe rows themselves are filtered out of what we ship.
                                        let oldest_osc =
                                            rows.first().map(|m| m.timestamp).unwrap_or(before_osc);
                                        let hist_rows: Vec<HistoryRow> = rows
                                            .iter()
                                            .filter(|m| {
                                                !crate::types::is_control_content(&m.content)
                                            })
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
                                        let page = HistoryPagePlain {
                                            rows: hist_rows,
                                            oldest_osc,
                                            more,
                                        };
                                        match seal_history_page(&page, &key).and_then(|sealed| {
                                            let skf = u32::from_le_bytes(blake3::hash(&key).as_bytes()[..4].try_into().unwrap()) as u64;
                                            crate::network::fgtw::protocol::build_history_page_vsf(
                                                &conversation_token,
                                                &request_id,
                                                serve_ek,
                                                Some(skf),
                                                sealed,
                                                &device_pubkey,
                                                &device_secret,
                                            )
                                        }) {
                                            Ok(vsf_bytes) => {
                                                crate::logf!(
                                                    "HISTORY: serving page ({} rows, more={}) to {}",
                                                    page.rows.len(),
                                                    page.more,
                                                    sender_addr
                                                );
                                                let _ = dispatch.send(
                                                    crate::network::status::HistorySendRequest {
                                                        peer_addr: sender_addr,
                                                        alt_addr: None,
                                                        recipient_pubkey: recipient,
                                                        // The response ALWAYS carries its one-device relay copy: requests arrive fine while responses die on one-directional reverse paths (2322 re-requests in one field session) — one relayed page is cheaper than the re-request storm.
                                                        relay_to: vec![recipient],
                                                        vsf_bytes,
                                                    },
                                                );
                                            }
                                            Err(e) => {
                                                crate::logf!("HISTORY: page build failed: {}", e)
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        crate::logf!("HISTORY: page read failed: {}", e)
                                    }
                                }
                            });
                        } else {
                            // WHICH gate refused (2026-09-01: 528 collapsed rejections in Emma's log couldn't say) — walk the same conditions and name the first failure + the asking device, so a field log convicts the era-split/consent lesion directly.
                            let why = {
                                let by_token = self
                                    .friendship_chains
                                    .iter()
                                    .find(|(_, c)| c.conversation_token == conversation_token);
                                match by_token {
                                    None => "no chain for token".to_string(),
                                    Some((_, c)) if c.history_key().is_none() => {
                                        "chain has no history key".to_string()
                                    }
                                    Some(_) => {
                                        let holder = self.contacts.iter().find(|c| {
                                            !c.is_sibling && c.knows_device(&sender_pubkey.key)
                                        });
                                        match holder {
                                            None => format!(
                                                "device {} unknown to every friend contact (stale fold / era split)",
                                                crate::fp(&sender_pubkey.key)
                                            ),
                                            Some(c) if !c.is_mutual() => format!(
                                                "device {} belongs to {} but the contact is NOT mutual",
                                                crate::fp(&sender_pubkey.key),
                                                crate::fp(&c.handle_proof)
                                            ),
                                            Some(c) => format!(
                                                "device {} known to {} but not bound to this token's chain",
                                                crate::fp(&sender_pubkey.key),
                                                crate::fp(&c.handle_proof)
                                            ),
                                        }
                                    }
                                }
                            };
                            crate::logf!("HISTORY: request rejected — {}", why);
                        }
                    }
                }

                // Sibling fork repair: a chain_reset frame arrived. Trust gates: outer signature already verified in the RX worker; here the sender must be a known SIBLING device and the sealed nonce must open under OUR fleet key (only fleet members can mint one). Application + echo are deferred past the drain (the repair rebuilds chains and sends frames — both blocked by live borrows here).
                StatusUpdate::PongSealMissing { device } => {
                    // Reseed the pong-seal map (rate-limited): the sender's tail will open on its next pong. Also retries the failed-open dedup by virtue of the RX worker clearing it on success.
                    const RESEED_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(10);
                    if self
                        .last_seal_reseed
                        .is_none_or(|t| t.elapsed() > RESEED_COOLDOWN)
                    {
                        self.last_seal_reseed = Some(Instant::now());
                        crate::logf!(
                            "Status: reseeding pong-seal keys (tail from {} unopenable)",
                            crate::fp(&device.key)
                        );
                        self.reseed_contact_pubkeys();
                    }
                }
                // Attachment blob arrived: authorize the sender (known device), pick the wire key by relationship (sibling → fleet, friend → history), open, verify the content hash, seal to the local blob store. The pill flips from "fetching" to present on the next frame (wrap cache dropped).
                StatusUpdate::AttachBlobReceived {
                    conversation_token,
                    content_hash,
                    sealed,
                    sender_pubkey,
                    sender_addr,
                } => {
                    let known = self
                        .contacts
                        .iter()
                        .any(|c| c.knows_device(&sender_pubkey.key));
                    let wire_key = self.attach_wire_key(&sender_pubkey.key, &conversation_token);
                    let seed = self.session.as_ref().map(|s| s.identity_seed);
                    if !known {
                        crate::log("ATTACH: blob from unknown device — dropped");
                    } else if let (Some(wire_key), Some(seed)) = (wire_key, seed) {
                        // OFF-THREAD: an attachment blob is arbitrary-size, and the AEAD open + blake3-over-the-whole-blob + disk store all ran inline on the render thread. A worker does the three, then posts back so the drain (which holds the keypair + checker) sends the attach_have confirm and clears the compose wrap. A hash mismatch or store failure logs and posts nothing.
                        let tx = self.attach_installed_tx.clone();
                        queue_job(&self.seal_job_tx, move || {
                            match kete::decrypt_bytes(&sealed, &wire_key) {
                                Ok(plain) if *blake3::hash(&plain).as_bytes() == content_hash => {
                                    match crate::storage::blob_store(&seed, &content_hash, &plain) {
                                        Ok(()) => {
                                            let _ = tx.send(AttachInstalled {
                                                conversation_token,
                                                content_hash,
                                                sender_pubkey,
                                                sender_addr,
                                                len: plain.len(),
                                            });
                                        }
                                        Err(e) => crate::logf!("ATTACH: blob store failed: {}", e),
                                    }
                                }
                                Ok(_) => crate::log("ATTACH: blob hash mismatch — dropped"),
                                Err(e) => crate::logf!("ATTACH: blob seal open failed: {}", e),
                            }
                        });
                    } else {
                        crate::log("ATTACH: no wire key / no session for the blob's conversation — dropped");
                    }
                }
                // Throttled PT transfer progress — drives the pill progress bars.
                StatusUpdate::AttachProgress(snap) => {
                    self.attach_progress = snap;
                    if matches!(self.state, AppState::Conversation) {
                        self.scene_dirty = true;
                        changed = true;
                    }
                }
                // Blob-landed confirmation from the receiver: the sender's pill flips to delivered.
                StatusUpdate::AttachHaveReceived {
                    content_hash,
                    sender_pubkey,
                } => {
                    if self
                        .contacts
                        .iter()
                        .any(|c| c.knows_device(&sender_pubkey.key))
                    {
                        self.attach_confirmed.insert(content_hash);
                        self.msg_wrap = None;
                        self.scene_dirty = true;
                        changed = true;
                    }
                }
                // A peer wants a blob we may hold: authorize, seal under the requester-relationship key, answer over PT (relay reply when the request came thru the pipe).
                StatusUpdate::AttachReqReceived {
                    conversation_token,
                    content_hash,
                    sender_pubkey,
                    sender_addr,
                } => {
                    let known = self
                        .contacts
                        .iter()
                        .any(|c| c.knows_device(&sender_pubkey.key));
                    let seed = self.session.as_ref().map(|s| s.identity_seed);
                    if !known {
                        crate::log("ATTACH: request from unknown device — ignored");
                    } else if let (Some(seed), Some(wire_key), Some(kp)) = (
                        seed,
                        self.attach_wire_key(&sender_pubkey.key, &conversation_token),
                        self.device_keypair.as_ref(),
                    ) {
                        // OFF-THREAD: serving a blob is a vault read + a kete seal over an ARBITRARY-size file — both ran inline on the render thread. Authorization stays here; the worker loads, seals, builds, and dispatches.
                        let kp_pub = *kp.public.as_bytes();
                        let kp_sec = *kp.secret.as_bytes();
                        let dispatch = checker.history_dispatch();
                        queue_job(&self.seal_job_tx, move || {
                            let Some(plain) = crate::storage::blob_load(&seed, &content_hash)
                            else {
                                crate::log("ATTACH: requested blob not held here");
                                return;
                            };
                            match kete::encrypt_bytes(&plain, &wire_key).and_then(|sealed| {
                                crate::network::fgtw::protocol::build_attach_blob_vsf(
                                    &conversation_token,
                                    &content_hash,
                                    sealed,
                                    &kp_pub,
                                    &kp_sec,
                                )
                            }) {
                                Ok(vsf_bytes) => {
                                    // A relay-injected request has no routable src addr — the always-carried relay copy answers back thru the pipe.
                                    let _ =
                                        dispatch.send(crate::network::status::HistorySendRequest {
                                            peer_addr: sender_addr,
                                            alt_addr: None,
                                            recipient_pubkey: sender_pubkey.key,
                                            vsf_bytes,
                                            relay_to: vec![sender_pubkey.key], // always the one-device relay copy — see the page-serve site: responses die on one-directional reverse paths
                                        });
                                    crate::log("ATTACH: served blob request");
                                }
                                Err(e) => {
                                    crate::logf!("ATTACH: serve frame build failed: {}", e)
                                }
                            }
                        });
                    }
                }
                StatusUpdate::ChainSyncReceived {
                    conversation_token,
                    epoch_k,
                    sealed,
                    sender_pubkey,
                } => {
                    // Trust gate: only a fold-verified sibling device may replace chain state. Re-checked at the drain — lockout can land between dispatch and the worker finishing.
                    if !self.contacts.iter().any(|c| {
                        c.is_sibling && !c.locked_out && c.knows_device(&sender_pubkey.key)
                    }) {
                        crate::log(
                            "CHAIN-SYNC: frame from a non-sibling or unknown device — dropped",
                        );
                        continue;
                    }
                    // The B3 re-seal: the blob opens under the chain_sync key of the epoch it names — current, or prev across a checkpoint crossing. A frame ahead of our spine means we lag; ask the sender to serve its state and drop (the chain re-pushes on its next advance).
                    let epoch = match (self.fleet_epoch, self.fleet_epoch_prev) {
                        (Some((k, e)), _) if k == epoch_k => Some(e),
                        (_, Some((k, e))) if k == epoch_k => Some(e),
                        _ => None,
                    };
                    let Some(epoch) = epoch else {
                        let our_k = self.fleet_epoch.map(|(k, _)| k).unwrap_or(0);
                        crate::logf!("CKPT: chain_sync sealed at k={} but our spine is at k={} — requesting state from {}", epoch_k, our_k, crate::fp(&sender_pubkey.key));
                        if let Some(kp) = self.device_keypair.as_ref() {
                            if let Ok(frame) = crate::network::fgtw::protocol::build_ckpt_req_vsf(
                                our_k,
                                kp.public.as_bytes(),
                                kp.secret.as_bytes(),
                            ) {
                                self.dispatch_frame_to_siblings(frame);
                            }
                        }
                        continue;
                    };
                    let seal_key =
                        crate::crypto::clutch::fleet_epoch_seal_key(&epoch, b"chain_sync");
                    // OFF-THREAD: the kete open + VSF decode ran inline — 17KB+ per lane, and a fresh sibling join repushes EVERY friendship at once (an adopt storm on the render thread). The worker opens and decodes; the adopt (cheap position compares) runs in drain_chain_syncs.
                    let tx = self.chain_sync_opened_tx.clone();
                    let wake = self.event_proxy.clone();
                    queue_job(&self.seal_job_tx, move || {
                        let Ok(plain) = kete::decrypt_bytes(&sealed, &seal_key) else {
                            crate::log("CHAIN-SYNC: blob failed to open under the epoch chain_sync key — dropped (stale epoch?)");
                            return;
                        };
                        let incoming =
                            match crate::storage::friendship::chains_from_vsf_bytes(&plain) {
                                Ok(c) => c,
                                Err(e) => {
                                    crate::logf!("CHAIN-SYNC: decode failed: {} — dropped", e);
                                    return;
                                }
                            };
                        if incoming.conversation_token != conversation_token {
                            crate::log("CHAIN-SYNC: inner token mismatch — dropped");
                            return;
                        }
                        let _ = tx.send(ChainSyncOpened {
                            conversation_token,
                            sender_pubkey,
                            incoming,
                        });
                        if let Some(w) = wake.as_ref() {
                            let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
                        }
                    });
                }

                // A checkpoint minter's root hand-off: open-success under the k−1 ckpt_root key is member-grade authentication (only fleet devices past epoch k−1 hold it); the chain's public commitment reconciles on the next refold as defence-in-depth, never as the liveness gate.
                StatusUpdate::CkptRootReceived {
                    k,
                    fanout_epoch,
                    sealed,
                    sender_pubkey,
                } => {
                    if !self.contacts.iter().any(|c| {
                        c.is_sibling && !c.locked_out && c.knows_device(&sender_pubkey.key)
                    }) {
                        crate::log(
                            "CKPT: root frame from a non-sibling or unknown device — dropped",
                        );
                        continue;
                    }
                    let (Some((our_k, our_epoch)), Some(fleet_key)) =
                        (self.fleet_epoch, self.fleet_key_cached())
                    else {
                        continue;
                    };
                    if k != our_k + 1 {
                        if k > our_k + 1 {
                            // More than one checkpoint ahead — sequential root-opening can't bridge it; jump via state serve.
                            if let Some(kp) = self.device_keypair.as_ref() {
                                if let Ok(frame) =
                                    crate::network::fgtw::protocol::build_ckpt_req_vsf(
                                        our_k,
                                        kp.public.as_bytes(),
                                        kp.secret.as_bytes(),
                                    )
                                {
                                    self.dispatch_frame_to_siblings(frame);
                                }
                            }
                        }
                        continue;
                    }
                    let open_key =
                        crate::crypto::clutch::fleet_epoch_seal_key(&our_epoch, b"ckpt_root");
                    let Ok(root_bytes) = kete::decrypt_bytes(&sealed, &open_key) else {
                        crate::logf!("CKPT: root for k={} failed to open under our k={} key — spines diverged, requesting state", k, our_k);
                        if let Some(kp) = self.device_keypair.as_ref() {
                            if let Ok(frame) = crate::network::fgtw::protocol::build_ckpt_req_vsf(
                                our_k,
                                kp.public.as_bytes(),
                                kp.secret.as_bytes(),
                            ) {
                                self.dispatch_frame_to_siblings(frame);
                            }
                        }
                        continue;
                    };
                    let Ok(root) = <[u8; 32]>::try_from(root_bytes.as_slice()) else {
                        crate::log("CKPT: root frame carried a malformed root — dropped");
                        continue;
                    };
                    // Divergence diagnostic, not a gate: a differing local root means we hold a different settled set — repair is convergence (the history sweep), never a wedge.
                    let local_root = self.settled_root_now();
                    if local_root != root {
                        crate::logf!("CKPT DIVERGENCE: minter's settled root for k={} differs from ours — adopting theirs, the history sweep reconciles rows", k);
                    }
                    let epoch = crate::crypto::clutch::advance_fleet_epoch(
                        &our_epoch,
                        &root,
                        &fleet_key,
                        fanout_epoch,
                        k,
                    );
                    self.fleet_epoch_prev = Some((our_k, our_epoch));
                    self.fleet_epoch = Some((k, epoch));
                    self.fleet_epoch_store();
                    // Pong tails ride the epoch key — flip on the same edge as the spine.
                    self.reseed_pong_seal_keys();
                    crate::logf!(
                        "CKPT: advanced to k={} from {}'s root hand-off",
                        k,
                        crate::fp(&sender_pubkey.key)
                    );
                }

                StatusUpdate::FriendKnockReceived {
                    conversation_token,
                    sender_pubkey,
                    sender_addr,
                } => {
                    // THE MUTUALITY EDGE (consent gate, 2026-08-25): a knock resolvable against OUR roster means the knocker is someone WE added — both humans have now entered each other's handles, and the ceremony may finally arm. An unmatched token is a stranger: drop silently, remember nothing (they get no storage, no reply, no evidence they were heard). The knock's whole job ends here; the ceremony pipeline from keygen onward is unchanged.
                    let our_pid = self.session.as_ref().map(|s| {
                        crate::crypto::clutch::identity_party_id(&s.identity_seed)
                    });
                    let matched = our_pid.and_then(|us| {
                        self.contacts.iter().position(|c| {
                            !c.is_sibling
                                && crate::crypto::clutch::derive_conversation_token(&[
                                    us,
                                    c.handle_hash,
                                ]) == conversation_token
                        })
                    });
                    let Some(ci) = matched else {
                        continue; // stranger's knock: unresolvable by construction — silence
                    };
                    if !self.sender_trusted_for(&self.contacts[ci], &sender_pubkey.key) {
                        continue; // matched row but an unknown/refused device — same silence
                    }
                    let _ = sender_addr;
                    if !self.contacts[ci].consent_mutual {
                        self.contacts[ci].consent_mutual = true;
                        crate::logf!(
                            "CONSENT: mutual add confirmed with {} — arming the ceremony",
                            crate::fp(&self.contacts[ci].handle_proof)
                        );
                        let id = self.contacts[ci].id.clone();
                        let (Some(us), them) = (our_pid, self.contacts[ci].handle_hash) else {
                            continue;
                        };
                        if self.contacts[ci].clutch_our_keypairs.is_none()
                            && !self.contacts[ci].clutch_keygen_in_progress
                        {
                            self.contacts[ci].clutch_keygen_in_progress = true;
                            self.spawn_clutch_keygen(id, us, them);
                        }
                        if let Some(storage) = self.storage.as_ref() {
                            let snapshot = self.contacts[ci].clone();
                            if let Err(e) =
                                crate::storage::contacts::save_contact(&snapshot, storage)
                            {
                                crate::logf!("CONSENT: contact save failed: {}", e);
                            }
                        }
                        consent_roster_push = true;
                    }
                }
                // A sibling's active-clearer claim/retraction (notification design 2026-07-23): newest osc wins, so a device the user just sat down at displaces the old holder; a retraction only clears the claim it matches. Fold-trust gated like every sibling frame.
                StatusUpdate::FocusClaimReceived {
                    conversation_token,
                    osc,
                    active,
                    sender_pubkey,
                } => {
                    let is_sib = self.contacts.iter().any(|c| {
                        c.is_sibling && !c.locked_out && c.knows_device(&sender_pubkey.key)
                    });
                    if is_sib {
                        let newer = self.fleet_focus_claim.map_or(true, |(_, _, cur)| osc > cur);
                        if active && newer {
                            self.fleet_focus_claim =
                                Some((conversation_token, sender_pubkey.key, osc));
                            crate::logf!(
                                "FOCUS: sibling {} claims the clearer role for tok {}…",
                                crate::fp(&sender_pubkey.key),
                                hex::encode(&conversation_token[..4])
                            );
                            // A claim IS attention (the open was human input THERE) — one frame moves both slots, mirroring the sender's local adopt.
                            let a_newer = attn_adopt
                                .or(self.fleet_attention)
                                .map_or(true, |(d, cur)| {
                                    osc > cur || (osc == cur && sender_pubkey.key > d)
                                });
                            if a_newer {
                                attn_adopt = Some((sender_pubkey.key, osc));
                            }
                        } else if !active {
                            if let Some((t, d, cur)) = self.fleet_focus_claim {
                                if t == conversation_token && d == sender_pubkey.key && osc >= cur {
                                    self.fleet_focus_claim = None;
                                    crate::logf!(
                                        "FOCUS: sibling {} retracted its clearer claim",
                                        crate::fp(&sender_pubkey.key)
                                    );
                                    // DROP-SWEEP (a): the retraction is the ball-drop edge — anything THIS device suppressed while honoring that claim is still undischarged; one summary chirp covers the crossing window (message in flight vs retraction in flight). Bounded below by the claim's own osc — suppression can only have happened while it stood.
                                    claim_sweeps.push((conversation_token, cur));
                                }
                            }
                        }
                    }
                }

                // A sibling announcing the human's newest input is THERE — fleet attention moves (2026-08-18). LWW by osc with device-byte tie-break, same fold-trust gate as the focus arm. If the move displaces a device that holds the active-clearer claim, the claim goes dishonored at the ding gate the same instant — sweep the rows we suppressed while it stood.
                StatusUpdate::AttentionReceived { osc, sender_pubkey } => {
                    let is_sib = self.contacts.iter().any(|c| {
                        c.is_sibling && !c.locked_out && c.knows_device(&sender_pubkey.key)
                    });
                    if is_sib {
                        let effective = attn_adopt.or(self.fleet_attention);
                        let newer = effective.map_or(true, |(d, cur)| {
                            osc > cur || (osc == cur && sender_pubkey.key > d)
                        });
                        if newer {
                            crate::logf!(
                                "ATTN: sibling {} holds the ball (osc {})",
                                crate::fp(&sender_pubkey.key),
                                osc
                            );
                            // DROP-SWEEP (c): the claim holder just lost attention — its claim no longer suppresses anyone; rows we muted while it stood are orphaned.
                            if let Some((tok, cd, cosc)) = self.fleet_focus_claim {
                                if cd != sender_pubkey.key {
                                    claim_sweeps.push((tok, cosc));
                                }
                            }
                            attn_adopt = Some((sender_pubkey.key, osc));
                        }
                    }
                }

                // A sibling's catch-up ask: serve our whole spine state fleet-key-sealed if we are ahead — the fgtw-independent jump path.
                StatusUpdate::ChainPullReceived {
                    conversation_token,
                    sender_pubkey,
                } => {
                    // Sibling authorization, exactly the ckpt_req gate; the serve/miss work mutates watermarks so it defers past the checker borrow.
                    if self.contacts.iter().any(|c| {
                        c.is_sibling && !c.locked_out && c.knows_device(&sender_pubkey.key)
                    }) {
                        chain_pull_reqs_after.push((conversation_token, sender_pubkey.key));
                    }
                }
                StatusUpdate::ChainPullMissReceived {
                    conversation_token,
                    sender_pubkey,
                } => {
                    if self.contacts.iter().any(|c| {
                        c.is_sibling && !c.locked_out && c.knows_device(&sender_pubkey.key)
                    }) {
                        chain_pull_misses_after.push((conversation_token, sender_pubkey.key));
                    }
                }
                StatusUpdate::DepartReqReceived {
                    consent_t,
                    consent_sig,
                    sender_pubkey,
                } => {
                    // Sibling gate: only a live member of OUR fleet may ask us to countersign its exit.
                    if self.contacts.iter().any(|c| {
                        c.is_sibling && !c.locked_out && c.knows_device(&sender_pubkey.key)
                    }) {
                        depart_reqs_after.push((consent_t, consent_sig, sender_pubkey.key));
                    }
                }
                StatusUpdate::CkptReqReceived {
                    have_k,
                    sender_pubkey,
                    sender_addr,
                } => {
                    if !self.contacts.iter().any(|c| {
                        c.is_sibling && !c.locked_out && c.knows_device(&sender_pubkey.key)
                    }) {
                        continue;
                    }
                    let (Some((our_k, our_epoch)), Some(fleet_key), Some(kp), Some(checker)) = (
                        self.fleet_epoch,
                        self.fleet_key_cached(),
                        self.device_keypair.as_ref(),
                        self.status_checker.as_ref(),
                    ) else {
                        continue;
                    };
                    if our_k <= have_k {
                        continue;
                    }
                    let state = crate::network::fgtw::fleet::ckpt_state_bytes(
                        our_k,
                        &our_epoch,
                        self.fleet_epoch_prev,
                    );
                    let custody_sealed = {
                        // The same custody sealing the slot uses — one key derivation, one ciphertext shape, whichever bearer serves it.
                        let mut h = blake3::Hasher::new();
                        h.update(b"PHOTON_FLEET_EPOCH_CUSTODY_v\x01");
                        h.update(&fleet_key);
                        kete::encrypt_bytes(&state, h.finalize().as_bytes())
                    };
                    let Ok(sealed) = custody_sealed else {
                        continue;
                    };
                    if let Ok(frame) = crate::network::fgtw::protocol::build_ckpt_state_vsf(
                        our_k,
                        sealed,
                        kp.public.as_bytes(),
                        kp.secret.as_bytes(),
                    ) {
                        let _ = checker.history_dispatch().send(
                            crate::network::status::HistorySendRequest {
                                peer_addr: sender_addr,
                                alt_addr: None,
                                recipient_pubkey: sender_pubkey.key,
                                relay_to: vec![sender_pubkey.key],
                                vsf_bytes: frame,
                            },
                        );
                        crate::logf!(
                            "CKPT: served spine state k={} to {} (they were at k={})",
                            our_k,
                            crate::fp(&sender_pubkey.key),
                            have_k
                        );
                    }
                }

                // A sibling's spine state: adopt if ahead. Fleet-key seal = the custody trust boundary, sibling-signed outer = the transport gate.
                StatusUpdate::CkptStateReceived {
                    k,
                    sealed,
                    sender_pubkey,
                } => {
                    if !self.contacts.iter().any(|c| {
                        c.is_sibling && !c.locked_out && c.knows_device(&sender_pubkey.key)
                    }) {
                        continue;
                    }
                    let Some(fleet_key) = self.fleet_key_cached() else {
                        continue;
                    };
                    let our_k = self.fleet_epoch.map(|(x, _)| x).unwrap_or(0);
                    if k <= our_k {
                        continue;
                    }
                    let custody_key = {
                        let mut h = blake3::Hasher::new();
                        h.update(b"PHOTON_FLEET_EPOCH_CUSTODY_v\x01");
                        h.update(&fleet_key);
                        *h.finalize().as_bytes()
                    };
                    let Ok(plain) = kete::decrypt_bytes(&sealed, &custody_key) else {
                        crate::log(
                            "CKPT: state frame failed to open under the custody key — dropped",
                        );
                        continue;
                    };
                    let Some((sk, epoch, prev)) =
                        crate::network::fgtw::fleet::ckpt_state_decode(&plain)
                    else {
                        crate::log("CKPT: state frame malformed — dropped");
                        continue;
                    };
                    if sk <= our_k {
                        continue;
                    }
                    self.fleet_epoch_prev = prev;
                    self.fleet_epoch = Some((sk, epoch));
                    self.fleet_epoch_store();
                    // Pong tails ride the epoch key — flip on the same edge as the spine.
                    self.reseed_pong_seal_keys();
                    crate::logf!(
                        "CKPT: adopted spine state k={} from {}",
                        sk,
                        crate::fp(&sender_pubkey.key)
                    );
                }

                StatusUpdate::ChainResetReceived {
                    conversation_token,
                    sealed,
                    sender_pubkey,
                    sender_addr: _,
                } => {
                    let Some(idx) = self.contacts.iter().position(|c| {
                        c.is_sibling && !c.locked_out && c.knows_device(&sender_pubkey.key)
                    }) else {
                        crate::log(
                            "CHAIN-RESET: frame from a non-sibling or unknown device — dropped",
                        );
                        continue;
                    };
                    let Some(fleet_key) = self.fleet_key_cached() else {
                        crate::log("CHAIN-RESET: no fleet key in hand — dropped (will heal on a later frame)");
                        continue;
                    };
                    let nonce: [u8; 32] = match kete::decrypt_bytes(&sealed, &fleet_key)
                        .ok()
                        .and_then(|p| p.try_into().ok())
                    {
                        Some(n) => n,
                        None => {
                            crate::log("CHAIN-RESET: sealed nonce failed to open under the fleet key — dropped");
                            continue;
                        }
                    };
                    // Token sanity: the frame must name OUR sibling 1:1 with this device, not some other conversation.
                    let expected_token = self.device_keypair.as_ref().map(|kp| {
                        let our_pid = crate::crypto::clutch::sibling_party_id(kp.public.as_bytes());
                        crate::crypto::clutch::derive_conversation_token(&[
                            our_pid,
                            self.contacts[idx].handle_hash,
                        ])
                    });
                    if expected_token != Some(conversation_token) {
                        crate::log("CHAIN-RESET: token mismatch — dropped");
                        continue;
                    }
                    if self.contacts[idx].last_chain_reset_nonce == Some(nonce) {
                        // The echo of a reset we already applied (or a retransmit) — converged, stop the ping-pong.
                        continue;
                    }
                    chain_reset_apply.push((idx, nonce, true));
                }

                // A history page arrived. Route by SENDER: a page from one of our own fleet devices opens under the FLEET key and merges VERBATIM (the sibling's view IS our view — same identity, no direction flip); a page from the friend opens under the friendship history key with direction flipped to our perspective. Friend pages must match an in-flight request; sibling pages that don't are the LIVE PUSH — a conversation advancing on another of our devices.
                StatusUpdate::HistoryPageReceived {
                    conversation_token,
                    request_id,
                    epoch_k,
                    sealer_key_fp,
                    sealed,
                    sender_pubkey,
                    sender_addr: _,
                } => {
                    let from_sibling = self.contacts.iter().any(|c| {
                        c.is_sibling && !c.locked_out && c.knows_device(&sender_pubkey.key)
                    });
                    let (key, contact_idx) = if from_sibling {
                        // Fleet-route pages seal under the EPOCH hist_page key (the B-arc re-seal): `ek` names the sealing epoch; we open at our k or k−1 across a checkpoint crossing. Behind the sender → request spine state (the chain_sync catch-up move) and drop; the request re-fires and self-heals. `ek` absent from a sibling = a pre-epoch build — flag-day drop, loudly.
                        let key = match (epoch_k, self.fleet_epoch, self.fleet_epoch_prev) {
                            (Some(ek), Some((k, e)), _) if ek == k => Some(
                                crate::crypto::clutch::fleet_epoch_seal_key(&e, b"hist_page"),
                            ),
                            (Some(ek), _, Some((pk, pe))) if ek == pk => Some(
                                crate::crypto::clutch::fleet_epoch_seal_key(&pe, b"hist_page"),
                            ),
                            (Some(ek), cur, _) => {
                                let our_k = cur.map(|(k, _)| k).unwrap_or(0);
                                crate::logf!("HISTORY: fleet page sealed at k={} but our spine is at k={} — requesting state from the fleet", ek, our_k);
                                if let Some(kp) = self.device_keypair.as_ref() {
                                    if let Ok(frame) =
                                        crate::network::fgtw::protocol::build_ckpt_req_vsf(
                                            our_k,
                                            kp.public.as_bytes(),
                                            kp.secret.as_bytes(),
                                        )
                                    {
                                        self.dispatch_frame_to_siblings(frame);
                                    }
                                }
                                None
                            }
                            (None, _, _) => {
                                crate::logf!("HISTORY: sibling page from {} carries no epoch — pre-epoch build, dropped (flag-day)", crate::fp(&sender_pubkey.key));
                                None
                            }
                        };
                        (
                            key,
                            self.contact_idx_for_conversation_token(&conversation_token),
                        )
                    } else {
                        // Participants are PARTY IDS (chains key on them since the pin-set migration) — resolve "other" against OUR party id, never the raw seed.
                        let our_pid = self
                            .session
                            .as_ref()
                            .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed));
                        let key_and_other = self
                            .friendship_chains
                            .iter()
                            .find(|(_, c)| c.conversation_token == conversation_token)
                            .and_then(|(_, c)| {
                                let key = c.history_key().copied()?;
                                let other = c
                                    .participants()
                                    .iter()
                                    .find(|p| Some(**p) != our_pid)
                                    .copied()?;
                                Some((key, other))
                            });
                        (
                            key_and_other.map(|(k, _)| k),
                            key_and_other.and_then(|(_, other)| {
                                self.contacts.iter().position(|c| c.handle_hash == other)
                            }),
                        )
                    };

                    if key.is_none() || contact_idx.is_none() {
                        // Torch the drop: a page dying here is indistinguishable from "sync doesn't work" in the field ("fleet sync with self is not working", 2026-07-26 — every drop in this arm was silent).
                        crate::logf!(
                            "HISTORY: page from {} DROPPED — {} (from_sibling={})",
                            crate::fp(&sender_pubkey.key),
                            if key.is_none() {
                                "no key (fleet key missing, or no chain/history_key for this token)"
                            } else {
                                "token resolves to no contact"
                            },
                            from_sibling
                        );
                    }
                    if let (Some(key), Some(_)) = (key, contact_idx) {
                        // OFF-THREAD: opening the sealed page (kete over up to MAX_PAGE_BYTES) ran inline here — 210-485ms per page in the field (2026-08-08), the largest single status-arm stall. The worker only decrypts and posts back; EVERY gate that reads mutable state (in-flight rid match, sibling trust, contact indexes) lives in drain_history_pages, evaluated against current state instead of a dispatch-time snapshot.
                        let tx = self.hist_opened_tx.clone();
                        let wake = self.event_proxy.clone();
                        let open_key_fp: [u8; 4] =
                            blake3::hash(&key).as_bytes()[..4].try_into().unwrap();
                        let sealer_fp_str = sealer_key_fp
                            .map(|f| format!("{:08x}", f as u32))
                            .unwrap_or_else(|| "-".into());
                        queue_job(&self.seal_job_tx, move || {
                            match crate::network::history_pages::open_history_page(&sealed, &key) {
                                Ok(page) => {
                                    let _ = tx.send(HistPageOpened {
                                        conversation_token,
                                        request_id,
                                        sender_pubkey,
                                        page: Some(page),
                                        open_key_fp,
                                    });
                                    if let Some(w) = wake.as_ref() {
                                        let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
                                    }
                                }
                                Err(e) => {
                                    // Named failure: the anonymous form of this line stalled the 2026-08-21 era-wedge diagnosis (161 drops, zero attribution). The failure ALSO rides the channel so the drain can count it toward the divergence park — silently re-requesting the same undecryptable page re-downloads 17KB per cycle forever.
                                    // The whole era-divergence diagnosis in one line: OUR key fp vs the SEALER's declared fp (wire skf). Matching fps + still failing = corruption; differing fps = the two fleets provably derive different history keys for the same token — the 2026-08-21 wedge, finally attributed.
                                    crate::logf!(
                                        "HISTORY: page open failed ({}) from {} token {} ours key#{} theirs key#{} — dropped",
                                        e,
                                        crate::fp(&sender_pubkey.key),
                                        hex::encode(&conversation_token[..4]),
                                        hex::encode(open_key_fp),
                                        sealer_fp_str
                                    );
                                    let _ = tx.send(HistPageOpened {
                                        conversation_token,
                                        request_id,
                                        sender_pubkey,
                                        page: None,
                                        open_key_fp,
                                    });
                                    if let Some(w) = wake.as_ref() {
                                        let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
                                    }
                                }
                            }
                        });
                    }
                }

                StatusUpdate::BlindFrameReceived {
                    kind,
                    conversation_token,
                    request_id,
                    blob,
                    found,
                    sent_osc,
                    sender_pubkey,
                    sender_addr,
                } => {
                    use crate::network::fgtw::protocol::BlindFrameKind;

                    // Staleness: an old frame is a replay/duplicate — drop before any state change.
                    let now = vsf::eagle_time_oscillations();
                    const BLIND_STALE_OSC: i64 = 600 * crate::OSC_PER_SEC;
                    if sent_osc != 0 && now.saturating_sub(sent_osc) > BLIND_STALE_OSC {
                        continue;
                    }
                    let Some(our_seed) = self.session.as_ref().map(|s| s.identity_seed) else {
                        continue;
                    };

                    match kind {
                        // A friend's device deposits its blind with us (or asks for it back) — or a FLEET SIBLING asks for S over its own token. Authorization for all: the token must resolve to a contact AND the signer must be a device we trust for it AND the relationship must be mutual (for a sibling that means the exact device + Complete ceremony).
                        BlindFrameKind::Put | BlindFrameKind::Get => {
                            let our_sibling_pid = self.our_sibling_pid();
                            // Friend tokens derive from the identity PARTY IDS (never the raw seed — the peer derives with pids, so a seed here rejects every legitimate frame as unknown-token).
                            let our_friend_pid =
                                crate::crypto::clutch::identity_party_id(&our_seed);
                            let cidx = self.contacts.iter().position(|c| {
                                let our = if c.is_sibling {
                                    match our_sibling_pid {
                                        Some(p) => p,
                                        None => return false,
                                    }
                                } else {
                                    our_friend_pid
                                };
                                c.is_mutual()
                                    && c.knows_device(&sender_pubkey.key)
                                    && crate::crypto::clutch::derive_conversation_token(&[
                                        our,
                                        c.handle_hash,
                                    ]) == conversation_token
                            });
                            let Some(idx) = cidx else {
                                crate::log("BLIND: put/get REJECTED (unknown token or unauthorized device)");
                                continue;
                            };

                            if kind == BlindFrameKind::Put && self.contacts[idx].is_sibling {
                                // Siblings never deposit OTP blinds (they serve S directly) — a put on a sibling token is a protocol violation.
                                crate::log("BLIND: put on a sibling token REJECTED");
                                continue;
                            }
                            if kind == BlindFrameKind::Get && self.contacts[idx].is_sibling {
                                // Sibling S-transfer: serve S sealed under the sibling chains' history key — only when OUR S is Live (a provisional S has no durable recovery anchor yet; the sibling keeps probing and adopts once we're Live, or generates if everyone misses).
                                let blob_opt: Option<Vec<u8>> =
                                    self.private_s.live().and_then(|(s, _)| {
                                        let fid = self.contacts[idx].friendship_id?;
                                        let (_, chains) = self
                                            .friendship_chains
                                            .iter()
                                            .find(|(id, _)| *id == fid)?;
                                        let key = chains.history_key().copied()?;
                                        crate::crypto::blind::seal_sibling_s(s, &key).ok()
                                    });
                                if let (Some(kp), Some(checker)) =
                                    (self.device_keypair.as_ref(), self.status_checker.as_ref())
                                {
                                    match crate::network::fgtw::protocol::build_blind_srv_vsf(
                                        &conversation_token,
                                        &request_id,
                                        blob_opt.as_deref(),
                                        kp.public.as_bytes(),
                                        kp.secret.as_bytes(),
                                    ) {
                                        Ok(vsf_bytes) => {
                                            let (primary, alt) = self.contacts[idx]
                                                .race_addrs()
                                                .unwrap_or((sender_addr, None));
                                            checker.send_history(
                                                crate::network::status::HistorySendRequest {
                                                    peer_addr: primary,
                                                    alt_addr: alt,
                                                    recipient_pubkey: sender_pubkey.key,
                                                    relay_to: self.contacts[idx]
                                                        .relay_device_list(), // BLIND frames always ride the relay — a validated path can be one-directional, and a lost answer stalls S-recovery silently (see drive_blind_ops)
                                                    vsf_bytes,
                                                },
                                            );
                                            crate::logf!(
                                                "BLIND: served {} to sibling device {}",
                                                if blob_opt.is_some() {
                                                    "sealed S"
                                                } else {
                                                    "found=0 (no live S)"
                                                },
                                                hex::encode(&sender_pubkey.key[..4])
                                            );
                                        }
                                        Err(e) => {
                                            crate::logf!("BLIND: sibling srv build failed: {}", e)
                                        }
                                    }
                                }
                                continue;
                            }

                            if kind == BlindFrameKind::Put {
                                if blob.len() != crate::crypto::blind::BLIND_BLOB_LEN {
                                    crate::log("BLIND: put REJECTED (bad blob length)");
                                    continue;
                                }
                                // BYTE-IDENTICAL redeposit: the stored copy is ALREADY durable (its commit gated the original ack) — pure lost-ack heal, re-ack with ZERO vault writes. Also the war-stability signal: a repeating value means the depositor settled, reset the flip detector.
                                let current = self.contacts[idx]
                                    .deposited_blinds
                                    .iter()
                                    .find(|(d, _, _)| *d == sender_pubkey.key)
                                    .map(|(_, b, _)| b.clone());
                                let flip_key = (self.contacts[idx].handle_proof, sender_pubkey.key);
                                if current.as_ref() == Some(&blob) {
                                    self.blind_flip.remove(&flip_key);
                                    if let (Some(kp), Some(checker)) =
                                        (self.device_keypair.as_ref(), self.status_checker.as_ref())
                                    {
                                        if let Ok(vsf_bytes) =
                                            crate::network::fgtw::protocol::build_blind_ack_vsf(
                                                &conversation_token,
                                                &request_id,
                                                kp.public.as_bytes(),
                                                kp.secret.as_bytes(),
                                            )
                                        {
                                            let (primary, alt) = self.contacts[idx]
                                                .race_addrs()
                                                .unwrap_or((sender_addr, None));
                                            let _ = checker.history_dispatch().send(
                                                crate::network::status::HistorySendRequest {
                                                    peer_addr: primary,
                                                    alt_addr: alt,
                                                    recipient_pubkey: sender_pubkey.key,
                                                    relay_to: self.contacts[idx]
                                                        .relay_device_list(),
                                                    vsf_bytes,
                                                },
                                            );
                                        }
                                    }
                                    continue;
                                }
                                // FLIP-FLOP WAR BRAKE: a REPLACING deposit whose bytes equal the value it replaced LAST TIME is the A-B-A signature of two installs sharing one device key, each overwriting the other forever (field 2026-08-21: 400 rounds in one log, every round a ~1.5s durable commit — the vault spent the afternoon on one contact). At 3 consecutive flips, commits decimate 8:1 — dropped rounds are UNACKED (no durability lie; the depositor just retries), a genuine re-key still lands within 8 rounds, and the byte-identical arm above resets the detector the moment the value stabilizes.
                                if let Some(cur) = current.as_ref() {
                                    let cur_hash = *blake3::hash(cur).as_bytes();
                                    let incoming_hash = *blake3::hash(&blob).as_bytes();
                                    let (prev_hash, flips) = self
                                        .blind_flip
                                        .get(&flip_key)
                                        .copied()
                                        .unwrap_or(([0u8; 32], 0));
                                    let flips = if prev_hash == incoming_hash {
                                        flips + 1
                                    } else {
                                        0
                                    };
                                    self.blind_flip.insert(flip_key, (cur_hash, flips));
                                    if flips >= 3 && flips % 8 != 0 {
                                        if flips == 3 {
                                            crate::logf!("BLIND: deposit WAR from {} device {} — alternating values (two installs on one device key?); decimating commits 8:1 until it stabilizes", crate::fp(&flip_key.0), crate::fp(&sender_pubkey.key));
                                        }
                                        continue;
                                    }
                                }
                                // Upsert by depositor device — a redeposit (re-key, S regen) replaces. Idempotent, so a duplicate put just re-acks (lost-ack heal).
                                let c = &mut self.contacts[idx];
                                if let Some(entry) = c
                                    .deposited_blinds
                                    .iter_mut()
                                    .find(|(d, _, _)| *d == sender_pubkey.key)
                                {
                                    entry.1 = blob.clone();
                                    entry.2 = now;
                                } else {
                                    c.deposited_blinds
                                        .push((sender_pubkey.key, blob.clone(), now));
                                }
                                // DISK COMMIT BEFORE THE ACK — the ack is the depositor's Provisional→Live edge, so it must attest durable storage, not RAM — but OFF the UI thread: this save is a dual-ring fsync that clocked 400-625ms per deposit in the field (2026-08-18 'massive lag', each friend-fleet deposit = one render-thread stall). Durable-then-signal, the chains-writer pattern: snapshot the contact, a worker persists it, and ONLY the worker sends the ack after the write lands. A crash between RAM-upsert and the worker write loses nothing — no ack left, the depositor retries, the upsert is idempotent.
                                let (Some(storage), Some(kp), Some(checker)) = (
                                    self.storage.as_ref().cloned(),
                                    self.device_keypair.clone(),
                                    self.status_checker.as_ref(),
                                ) else {
                                    continue;
                                };
                                let snapshot = self.contacts[idx].clone();
                                let (primary, alt) = self.contacts[idx]
                                    .race_addrs()
                                    .unwrap_or((sender_addr, None));
                                let relay_to = self.contacts[idx].relay_device_list(); // BLIND frames always ride the relay — a validated path can be one-directional, and a lost answer stalls S-recovery silently (see drive_blind_ops)
                                let dispatch = checker.history_dispatch();
                                queue_job(&self.seal_job_tx, move || {
                                    if let Err(e) = crate::storage::contacts::save_contact_state(
                                        &snapshot, &storage,
                                    ) {
                                        crate::logf!("BLIND: deposit persist failed: {}", e);
                                        return;
                                    }
                                    match crate::network::fgtw::protocol::build_blind_ack_vsf(
                                        &conversation_token,
                                        &request_id,
                                        kp.public.as_bytes(),
                                        kp.secret.as_bytes(),
                                    ) {
                                        Ok(vsf_bytes) => {
                                            let _ = dispatch.send(
                                                crate::network::status::HistorySendRequest {
                                                    peer_addr: primary,
                                                    alt_addr: alt,
                                                    recipient_pubkey: sender_pubkey.key,
                                                    relay_to,
                                                    vsf_bytes,
                                                },
                                            );
                                            crate::logf!("BLIND: stored deposit from {} device {} — acked (disk-committed, off-thread)", crate::fp(&snapshot.handle_proof), hex::encode(&sender_pubkey.key[..4]));
                                        }
                                        Err(e) => {
                                            crate::logf!("BLIND: ack build failed: {}", e)
                                        }
                                    }
                                });
                            } else {
                                // Get: serve THE SIGNER's deposit back (or an explicit miss — the probe-before-generate signal).
                                let blob_opt = self.contacts[idx]
                                    .deposited_blinds
                                    .iter()
                                    .find(|(d, _, _)| *d == sender_pubkey.key)
                                    .map(|(_, b, _)| b.clone());
                                if let (Some(kp), Some(checker)) =
                                    (self.device_keypair.as_ref(), self.status_checker.as_ref())
                                {
                                    match crate::network::fgtw::protocol::build_blind_srv_vsf(
                                        &conversation_token,
                                        &request_id,
                                        blob_opt.as_deref(),
                                        kp.public.as_bytes(),
                                        kp.secret.as_bytes(),
                                    ) {
                                        Ok(vsf_bytes) => {
                                            let (primary, alt) = self.contacts[idx]
                                                .race_addrs()
                                                .unwrap_or((sender_addr, None));
                                            checker.send_history(
                                                crate::network::status::HistorySendRequest {
                                                    peer_addr: primary,
                                                    alt_addr: alt,
                                                    recipient_pubkey: sender_pubkey.key,
                                                    relay_to: self.contacts[idx]
                                                        .relay_device_list(), // BLIND frames always ride the relay — a validated path can be one-directional, and a lost answer stalls S-recovery silently (see drive_blind_ops)
                                                    vsf_bytes,
                                                },
                                            );
                                            crate::logf!(
                                                "BLIND: served {} to {} device {}",
                                                if blob_opt.is_some() {
                                                    "deposit"
                                                } else {
                                                    "found=0 (no deposit)"
                                                },
                                                crate::fp(&self.contacts[idx].handle_proof),
                                                hex::encode(&sender_pubkey.key[..4])
                                            );
                                        }
                                        Err(e) => crate::logf!("BLIND: srv build failed: {}", e),
                                    }
                                }
                            }
                        }

                        // Our deposit is disk-confirmed at the friend: rid must match our in-flight put.
                        BlindFrameKind::Ack => {
                            let Some(idx) = self.contacts.iter().position(|c| {
                                c.blind_in_flight
                                    .map_or(false, |(r, _, is_get)| r == request_id && !is_get)
                            }) else {
                                continue; // not ours / already resolved — duplicate ack, harmless
                            };
                            self.contacts[idx].blind_in_flight = None;
                            self.contacts[idx].blind_deposited = true;
                            // OFF-THREAD, snapshot pattern: this inline save was the `BlindFrameReceived 890ms (UI thread)` arm in the 2026-08-21 hang capture — a full vault write on the render thread for a flag only WE read (no ack depends on it, unlike the deposit path above; a crash before the write just re-runs the confirm, idempotent).
                            if let Some(storage) = self.storage.as_ref().cloned() {
                                let snapshot = self.contacts[idx].clone();
                                queue_job(&self.seal_job_tx, move || {
                                    if let Err(e) = crate::storage::contacts::save_contact_state(
                                        &snapshot, &storage,
                                    ) {
                                        crate::logf!("BLIND: deposited-flag persist failed: {}", e);
                                    }
                                });
                            }
                            crate::logf!(
                                "BLIND: deposit confirmed at {}",
                                crate::fp(&self.contacts[idx].handle_proof)
                            );
                            // First confirmation flips Provisional → Live: from here S may author tags, because at least one friend durably holds the recovery blind.
                            if matches!(
                                self.private_s,
                                crate::crypto::blind::PrivateS::Provisional(_)
                            ) {
                                if let crate::crypto::blind::PrivateS::Provisional(s) =
                                    std::mem::take(&mut self.private_s)
                                {
                                    let sid = crate::crypto::blind::s_id(&s);
                                    crate::logf!("S: live (s_id={})", hex::encode(sid));
                                    self.private_s =
                                        crate::crypto::blind::PrivateS::Live { s, s_id: sid };
                                }
                            }
                        }

                        // Answer to OUR probe: rid must match the in-flight get.
                        BlindFrameKind::Srv => {
                            let Some(idx) = self.contacts.iter().position(|c| {
                                c.blind_in_flight
                                    .map_or(false, |(r, _, is_get)| r == request_id && is_get)
                            }) else {
                                continue; // unsolicited/expired — drop
                            };
                            self.contacts[idx].blind_in_flight = None;

                            if found && self.contacts[idx].is_sibling {
                                // Sibling served S sealed under the sibling chains' history key. Adopt it; on a live-vs-live epoch clash both sides converge on the LOWER s_id deterministically (split-brain healing: only possible when two fresh devices genesised with zero shared friends).
                                let opened = {
                                    let fid = self.contacts[idx].friendship_id;
                                    fid.and_then(|fid| {
                                        self.friendship_chains
                                            .iter()
                                            .find(|(id, _)| *id == fid)
                                            .and_then(|(_, chains)| chains.history_key().copied())
                                    })
                                    .and_then(|key| {
                                        crate::crypto::blind::open_sibling_s(&blob, &key)
                                    })
                                };
                                match opened {
                                    Some(s) => {
                                        let sid = crate::crypto::blind::s_id(&s);
                                        match &self.private_s {
                                            crate::crypto::blind::PrivateS::Live {
                                                s_id, ..
                                            } if *s_id != sid => {
                                                if sid < *s_id {
                                                    crate::logf!("S: CRITICAL — divergent epochs across the fleet; ADOPTING the lower ({} < {}) and redepositing everywhere", hex::encode(sid), hex::encode(s_id));
                                                    self.private_s =
                                                        crate::crypto::blind::PrivateS::Live {
                                                            s,
                                                            s_id: sid,
                                                        };
                                                    for c in self.contacts.iter_mut() {
                                                        if !c.is_sibling {
                                                            c.blind_deposited = false;
                                                        }
                                                    }
                                                } else {
                                                    crate::logf!("S: CRITICAL — divergent epochs across the fleet; keeping the lower ({} < {}), sibling converges on its next probe", hex::encode(s_id), hex::encode(sid));
                                                }
                                            }
                                            crate::crypto::blind::PrivateS::Live { .. } => {
                                                crate::log(
                                                    "S: sibling cross-check OK (same epoch)",
                                                );
                                            }
                                            _ => {
                                                crate::logf!("S: adopted from fleet sibling (check OK, s_id={})", hex::encode(sid));
                                                self.private_s =
                                                    crate::crypto::blind::PrivateS::Live {
                                                        s,
                                                        s_id: sid,
                                                    };
                                            }
                                        }
                                    }
                                    None => {
                                        crate::log(
                                            "BLIND: CRITICAL — sibling-served S failed AEAD/check; treating as miss",
                                        );
                                        self.contacts[idx].blind_probe_missed = true;
                                        check_s_genesis = true;
                                    }
                                }
                                continue;
                            }

                            if found && blob.len() == crate::crypto::blind::BLIND_BLOB_LEN {
                                let Some(kp) = self.device_keypair.as_ref() else {
                                    continue;
                                };
                                let device_secret = *kp.secret.as_bytes();
                                let pad = crate::crypto::blind::derive_blind_pad(
                                    &device_secret,
                                    &self.contacts[idx].handle_hash,
                                );
                                match crate::crypto::blind::open_blind_blob(&blob, &pad) {
                                    Some(s) => {
                                        let sid = crate::crypto::blind::s_id(&s);
                                        match &self.private_s {
                                            crate::crypto::blind::PrivateS::Live {
                                                s_id, ..
                                            } if *s_id != sid => {
                                                // Split-brain: a friend holds a DIFFERENT epoch than the S we're running. Keep ours (it has live confirmations); the redeposit driver will overwrite theirs.
                                                crate::logf!("BLIND: CRITICAL — divergent S epoch from {} (theirs {}, ours {}); keeping ours + redepositing", crate::fp(&self.contacts[idx].handle_proof), hex::encode(sid), hex::encode(s_id));
                                                self.contacts[idx].blind_deposited = false;
                                            }
                                            crate::crypto::blind::PrivateS::Live { .. } => {
                                                crate::log("BLIND: cross-check OK (same S epoch)");
                                            }
                                            _ => {
                                                crate::logf!("S: reconstituted from friend blind (check OK, s_id={})", hex::encode(sid));
                                                self.private_s =
                                                    crate::crypto::blind::PrivateS::Live {
                                                        s,
                                                        s_id: sid,
                                                    };
                                                // A served deposit IS a confirmed deposit at this friend. Persist OFF-THREAD (snapshot pattern) — the sibling inline save was the 890ms UI-thread arm; this is the same flag on the reconstitute path.
                                                self.contacts[idx].blind_deposited = true;
                                                if let Some(storage) =
                                                    self.storage.as_ref().cloned()
                                                {
                                                    let snapshot = self.contacts[idx].clone();
                                                    queue_job(&self.seal_job_tx, move || {
                                                        let _ = crate::storage::contacts::save_contact_state(
                                                            &snapshot, &storage,
                                                        );
                                                    });
                                                }
                                            }
                                        }
                                    }
                                    None => {
                                        // Tampered blob or a foreign device's deposit under our key — treat as a miss for THIS friend, loudly (a valid deposit at another friend must still win over genesis).
                                        crate::logf!("BLIND: CRITICAL — served blob failed the check from {} (tampered?); treating as miss", crate::fp(&self.contacts[idx].handle_proof));
                                        self.contacts[idx].blind_probe_missed = true;
                                        check_s_genesis = true;
                                    }
                                }
                            } else {
                                crate::logf!(
                                    "BLIND: no deposit at {} (found=0)",
                                    crate::fp(&self.contacts[idx].handle_proof)
                                );
                                self.contacts[idx].blind_probe_missed = true;
                                check_s_genesis = true;
                            }
                        }
                    }
                }

                StatusUpdate::ReflexiveLearned { addr } => {
                    // Our own public address, learned via peer-echoed reflection on the live UDP data socket. Store it for candidate gathering and the announce to publish (so our `PeerRecord.ip` is the real data-socket address, not fgtw.org's cone-only TLS view).
                    if self.our_reflexive != Some(addr) {
                        self.our_reflexive = Some(addr);
                        crate::logf!("TRAVERSE: our reflexive address = {}", addr);
                        // The re-publish happens in tick, not here: this arm sits inside a borrow of `status_checker`, and publishing also needs `handle_proof`, which can arrive AFTER the first reflexive echo. Comparing against `self_record_published_for` there makes it idempotent and self-retrying instead of a one-shot that could fire too early.
                    }
                    // A UDP-observed mapping is now in hand — stop carrying Reflects beside pings (the bootstrap is self-extinguishing; the LAN-change edge below re-arms it).
                    checker.set_reflect_needed(false);
                }

                StatusUpdate::OurLanAddrObserved { ip } => {
                    // Our own LAN address, from our looped-back beacon's source — the interface the beacon actually left on, not the one that routes to the internet. On change, clear `self_record_published_for` so the same tick edge that handles a reflexive change re-signs and re-publishes the record WITH the LAN entry (same idempotent re-publish path, same reason it isn't done inline here).
                    //
                    // STICKY across interfaces: a multi-homed device loops a beacon back on EVERY interface each round, so a naive `our_lan_ip != Some(ip)` flipped the published address between them every beacon — and each flip re-signed, re-published, and re-persisted the record on a loop (a measured 0.5–4.4s phonebook-persist freeze, repeating). Track the observed set with last-seen times, keep the current address while it's still observed, and only re-pick when it ages out. The record's single LAN slot then holds one stable address instead of thrashing.
                    let now = std::time::Instant::now();
                    self.our_lan_ips.insert(ip, now);
                    // Age out an interface that stopped beaconing (unplugged / roamed off). Generous TTL — a couple of missed beacon rounds must not drop a live interface.
                    const OWN_LAN_TTL: std::time::Duration = std::time::Duration::from_secs(120);
                    self.our_lan_ips
                        .retain(|_, seen| now.duration_since(*seen) < OWN_LAN_TTL);
                    let keep_current = self
                        .our_lan_ip
                        .is_some_and(|cur| self.our_lan_ips.contains_key(&cur));
                    // Deterministic tie-break (lowest address) only when we must actually choose — so two devices, or the same device twice, never disagree on which of several live addresses to publish.
                    let chosen = if keep_current {
                        self.our_lan_ip
                    } else {
                        self.our_lan_ips.keys().copied().min()
                    };
                    if self.our_lan_ip != chosen {
                        self.our_lan_ip = chosen;
                        if let Some(chosen_ip) = chosen {
                            crate::logf!(
                                "TRAVERSE: our LAN address = {} (from our own looped-back beacon)",
                                chosen_ip
                            );
                        }
                        self.self_record_published_for = None;
                        // Interface change = our NAT mapping likely changed too — re-arm the reflect-beside-pings bootstrap so the published record re-learns the TRUE mapping.
                        checker.set_reflect_needed(true);
                    }
                }

                StatusUpdate::PathValidated {
                    peer_pubkey,
                    remote,
                } => {
                    // A hole-punch (or keepalive) round-tripped. Record/refresh it on the matching contact (any device in the friend's fleet) so `race_addrs` prefers this direct path, keeping the public/LAN as the alternate. First-wins on the address (we stop full-punching once a path is set, so among a single cycle's candidates the first to round-trip — ≈ the lowest-latency path — wins); the timestamp is refreshed on every ack for that same path (keepalive liveness). Any validation clears the graceful-failure counter.
                    let now = std::time::Instant::now();
                    let mut refire: Option<usize> = None;
                    if let Some((idx, contact)) = self
                        .contacts
                        .iter_mut()
                        .enumerate()
                        .find(|(_, c)| c.knows_device(&peer_pubkey.key))
                    {
                        // Never validate the unspecified sentinel (0.0.0.0 / ::) — that's the RELAY_ADDR a relayed message carries, and a punch to it round-trips locally. Validating it poisons addressing: sends go nowhere and relay_to empties out because validated_path looks Some (a peer's proof vanished exactly this way). Bail before touching any state.
                        if remote.ip().is_unspecified() {
                            continue;
                        }
                        contact.punch_unvalidated_cycles = 0;
                        // A direct path just proved out — this contact is no longer relay-only, so drop the lime-yellow and show normal green.
                        contact.reached_via_relay = false;
                        // Reachability clock: a signed punch ack = the guard's eyes are open.
                        contact.last_heard = Some(now);
                        // The same-LAN judgment, made ONCE here at the edge (get_local_ip binds a socket — never per frame): private address on OUR subnet (or WFD group) = LAN; private-but-foreign (carrier CGNAT 10.x, colliding home /24s) = a real direct path that is NOT "same room" (the 2026-08-30 cyan lie: a Verizon-CGNAT path to a peer hundreds of miles away rang LAN). Canonical form first — punch acks arrive v4-mapped (::ffff:a.b.c.d).
                        let canon = crate::network::udp::canon_socketaddr(remote);
                        let remote_is_lan = crate::ui::photon_app::is_private_addr(&canon.ip())
                            && !crate::network::traverse::gather::is_foreign_peer_lan(
                                &canon,
                                crate::network::udp::get_local_ip(),
                            );
                        match contact.validated_path {
                            None => {
                                crate::logf!(
                                    "TRAVERSE: path validated to {} = {}",
                                    crate::fp(&contact.handle_proof).as_str(),
                                    remote
                                );
                                contact.validated_path = Some((remote, now));
                                contact.validated_path_lan = remote_is_lan;
                                // Path-up EDGE doubles as the parked ceremony's second chance: the one offer send may have raced only dead records (carrier-NAT LAN + a stale registry address) before this path proved out — and the pong-driven re-send never fires for a peer whose pongs don't flow. Only when the peer's own offer hasn't arrived either (a present offer means the exchange is moving; duplicates would just burn a half-MB transfer).
                                if !contact.is_sibling
                                    && contact.clutch_state == crate::types::ClutchState::Pending
                                    && contact.clutch_offer_sent
                                    && contact
                                        .get_slot(&contact.handle_hash)
                                        .map_or(true, |s| s.offer.is_none())
                                {
                                    refire = Some(idx);
                                }
                            }
                            Some((existing, _)) if existing == remote => {
                                // Keepalive ack for the current path — refresh liveness (the LAN judgment stands; same address, same verdict).
                                contact.validated_path = Some((remote, now));
                            }
                            Some((existing, _))
                                if remote_is_lan && !is_private_addr(&existing.ip()) =>
                            {
                                // A LAN path acked while we're pinned to a public/cell one — UPGRADE. First-wins normally holds, but a genuinely-OUR-subnet path is categorically better than a carrier one: it never rotates (the cell IPv6 privacy address churns — five in one field log — and each rotation strands the pinned path until TTL), no NAT, lowest latency. Two devices on one LAN must ride the LAN, not race a dying cell mapping. Gated on the same-LAN judgment, not bare is_private (a foreign CGNAT 10.x acking must NOT displace a working public path — 2026-08-30). (Only LAN-supplants-public; a second public path never displaces the first-won.)
                                crate::logf!(
                                    "TRAVERSE: {} LAN path {} supplants pinned public {}",
                                    crate::fp(&contact.handle_proof).as_str(),
                                    remote,
                                    existing
                                );
                                contact.validated_path = Some((remote, now));
                                contact.validated_path_lan = true;
                            }
                            Some(_) => { /* a different candidate acked; keep the first-won path */
                            }
                        }
                    }
                    if let Some(idx) = refire {
                        offer_refire_indices.push(idx);
                    }
                }
                StatusUpdate::WfdCredReceived { .. } => {
                    // RETIRED (universal-token mode, 2026-09-01): per-pair provisioned credentials are gone — discovery rides blake3("Photon WiFi direct v0") and cleartext open-house creds; auth is the chain layer. An old build's cred frame is harmless noise.
                    crate::log("WFD: per-pair cred frame from an old build — retired, dropped");
                }
                StatusUpdate::WfdFriendNearby { device_pubkey } => {
                    // FRIEND-HEARD edge: a provisioned friend's token is in the air and we hold no path to them — form the group (the bearer decides create-vs-connect from the credential's designated GO).
                    crate::logf!(
                        "WFD: provisioned friend device {} heard nearby",
                        crate::fp(&device_pubkey)
                    );
                    if let Some(kp) = self.device_keypair.as_ref() {
                        let our_dev = *kp.public.as_bytes();
                        if let Some(c) = self
                            .contacts
                            .iter()
                            .find(|c| !c.is_sibling && c.knows_device(&device_pubkey))
                        {
                            if c.validated_path.is_none() {
                                if let Some(cred) = c.wfd_cred.as_ref() {
                                    crate::network::wfd::friend_heard(&our_dev, cred);
                                }
                            }
                        }
                    }
                }
                StatusUpdate::WfdGroupUp {
                    is_go,
                    our_ip,
                    go_ip,
                } => {
                    crate::logf!(
                        "WFD: group up — we are {} (our {} / GO {})",
                        if is_go { "GO" } else { "client" },
                        our_ip,
                        go_ip
                    );
                    crate::network::wfd::group_up();
                    crate::network::wfd::note_group(Some((is_go, our_ip, go_ip)));
                    // The joiner beacons the GO unicast with the existing pt_disc shape; the GO learns the joiner from that frame's source and beacons back (both land in the LanPeerDiscovered arm, which routes 192.168.49/24 sources into p2p_addr).
                    if !is_go {
                        if let (Some(session), Some(hq), Some(checker)) = (
                            self.session.as_ref(),
                            self.handle_query.as_ref(),
                            self.status_checker.as_ref(),
                        ) {
                            checker.send_lan_unicast(
                                session.handle_proof,
                                hq.port(),
                                std::net::SocketAddr::new(
                                    std::net::IpAddr::V4(go_ip),
                                    crate::PHOTON_PORT,
                                ),
                            );
                        }
                    }
                }
                StatusUpdate::WfdGroupDown => {
                    crate::network::wfd::iface_lost();
                    crate::network::wfd::note_group(None);
                    // Clear every p2p address AND any validated path inside the group subnet so sends fall back immediately instead of black-holing (same lesson as is_bogus_addr).
                    let mut cleared = 0;
                    for c in self.contacts.iter_mut() {
                        if c.p2p_addr.take().is_some() {
                            cleared += 1;
                        }
                        if let Some((addr, _)) = c.validated_path {
                            if let std::net::IpAddr::V4(v4) = addr.ip() {
                                if crate::network::traverse::gather::is_wfd_subnet(v4) {
                                    c.validated_path = None;
                                }
                            }
                        }
                    }
                    if cleared > 0 {
                        crate::logf!("WFD: group down — cleared {} p2p address(es)", cleared);
                    }
                }
            }
        }
        close_arm_timer!();
        // Budget-deferred synthetic replays go back on the FRONT so strict ordering holds across ticks (anything newly minted this pass queues behind them).
        if !replay_queue.is_empty() {
            replay_queue.extend(std::mem::take(&mut self.chat_replay_queue));
            self.chat_replay_queue = replay_queue;
        }
        // The pass profile — logged for any heavy pass so the field log names the CUMULATIVE eaters, not just single >100ms offenders.
        let pass_ms = pass_start.elapsed().as_millis();
        if pass_ms > 200 {
            let mut top: Vec<(&'static str, (u32, u128))> = pass_profile.into_iter().collect();
            top.sort_by_key(|&(_, (_, ms))| std::cmp::Reverse(ms));
            let summary = top
                .iter()
                .take(3)
                .map(|(label, (n, ms))| format!("{label} {ms}ms x{n}"))
                .collect::<Vec<_>>()
                .join(", ");
            crate::logf!(
                "PERF: status pass {}ms over {} update(s) — top arms: {}",
                pass_ms as u64,
                pass_updates,
                summary
            );
        }
        // The post-loop deferred section (ceremony completions, pings, seals, persists, adoptions) — the second untimed region the 2026-08-15 stalls could hide in.
        let deferred_t = std::time::Instant::now();

        // Async-persist every conversation an arm touched (deduped — one snapshot per conversation per drain).
        persist_hashes.dedup();
        for hh in persist_hashes {
            if let Some(idx) = self.contacts.iter().position(|c| c.handle_hash == hh) {
                self.persist_messages_async(idx);
            }
        }

        // Rotated-lane flush (after releasing the checker borrow): persist the rotation — a crash before the write just re-detects and re-rotates, the wedge evidence is durable on the peer — then re-serve every undelivered row thru the normal send path on the fresh lane. chain_transmit rebuilds fresh frames at the rows' ORIGINAL eagle_times; the in-flight window paces the burst and the ACK edges keep flushing the rest.
        for fid in rotated_flush {
            self.persist_chains_async(&fid);
            if let Some(ci) = self
                .contacts
                .iter()
                .position(|c| c.friendship_id == Some(fid))
            {
                self.resend_held_messages(ci);
            }
        }

        // Sender-side re-serve execution (checker borrow released): rebuild each missing row's wire frame at its ORIGINAL eagle_time. chain_transmit's already-in-flight guard keeps this idempotent, and its in-flight window paces the burst — a refused row just waits for the next tip observation.
        for (ci, cap_key, rows) in reserve_jobs {
            let mut served = 0usize;
            for (ts, content, reference) in rows {
                let bw = self.bridge_wire_for_row(ci, ts);
                if self.chain_transmit(ci, &content, ts, reference, bw.as_ref()) {
                    served += 1;
                }
            }
            // Charge the cap for what actually LEFT — attempts the serial-send gate swallowed cost nothing, so the deficit keeps draining across pongs instead of parking two rows in. A ZERO-served burst still charges 1: chain_transmit refusing every row (no chain / no address / stale-era token lane) repeated forever otherwise — the tip-0 're-serving 8' spam every ~45s, 2026-09-01 — and a lane that cannot transmit at all is exactly what the park exists for.
            if let Some(e) = self.lane_reserve_bursts.get_mut(&cap_key) {
                e.3 = e.3.saturating_add((served as u8).max(1));
            }
            if served > 0 {
                crate::logf!(
                    "CHAT: re-served {} row(s) to {} from the durable store",
                    served,
                    crate::fp(&self.contacts[ci].handle_proof)
                );
            }
        }

        // COMPLETE-WITHOUT-CHAINS SELF-HEAL (field 2026-08-26, Emma's device after the pre-fix crypto wipe): the wipe cleared her friendship chains, the RAM resurrection restored a contact CLAIMING Complete — so every inbound frame dropped "No friendship found" forever and nothing ever triggered a repair, because the state said there was nothing to fix. The evidence is the frame we couldn't route: a probed token whose contact claims Complete while its chains are ABSENT gets the same reset the offer-while-Complete re-key path runs, and the fresh offer walks the peer thru the normal accepting-re-key flow. Evidence-driven (never fires on healthy or mid-ceremony state), once per contact per session via the keygen-in-progress latch.
        for token in rekey_probe {
            let our_pid = self
                .session
                .as_ref()
                .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed));
            let Some(us) = our_pid else { break };
            let Some(ci) = self.contacts.iter().position(|c| {
                !c.is_sibling
                    && crate::crypto::clutch::derive_conversation_token(&[us, c.handle_hash])
                        == token
            }) else {
                continue;
            };
            // TOKEN-matched, not fid-matched (2026-09-01, Emma's era-split residue): her contact held A chain — the stale-era one — so the fid check said "chains present" and the heal never fired, while every current-era frame bounced off "no chain for token" forever. The evidence that matters is a chain FOR THIS TOKEN.
            let chains_present = self
                .friendship_chains
                .iter()
                .any(|(_, c)| c.conversation_token == token);
            if self.contacts[ci].clutch_state != crate::types::ClutchState::Complete
                || chains_present
                || self.contacts[ci].clutch_keygen_in_progress
            {
                continue;
            }
            // ASK THE FLEET FIRST (field 2026-08-31, Nick's device add): on a freshly-added device this exact state — roster says Complete, chains not yet replicated — is EXPECTED for every friend, and friend traffic reliably beats the sibling join-edge re-push. Declaring wipe debris here mass-re-keyed the whole roster, and the fresh chains then clobbered the fleet's established ones via adopt-iff-newer. So with any live sibling, fire ONE chain_pull (per token per session) and hold; a sibling holding the chains re-pushes (watermark clear), and the re-key runs only on the true-negative edge — every live sibling answered miss (drained in ChainPullMissReceived). No siblings = the original Emma single-device wipe, heal immediately.
            let have_live_sibling = self
                .contacts
                .iter()
                .any(|c| c.is_sibling && !c.locked_out);
            if have_live_sibling {
                if self.chain_pull_sent.insert(token) {
                    crate::logf!(
                        "CLUTCH: {} claims Complete but holds NO chains — asking the fleet (chain_pull) before any re-key",
                        crate::fp(&self.contacts[ci].handle_proof)
                    );
                    if let Some(kp) = self.device_keypair.as_ref() {
                        if let Ok(frame) = crate::network::fgtw::protocol::build_chain_pull_vsf(
                            &token,
                            kp.public.as_bytes(),
                            kp.secret.as_bytes(),
                        ) {
                            self.dispatch_frame_to_siblings(frame);
                        }
                    }
                }
                continue;
            }
            crate::logf!(
                "CLUTCH: {} claims Complete but holds NO chains and there are no siblings to ask (wipe debris) — discarding the lie and re-keying",
                crate::fp(&self.contacts[ci].handle_proof)
            );
            self.rekey_without_chains(ci);
        }

        // chain_pull serves (deferred past the checker borrow): a sibling asked for chains it lacks. Holding them = clear this friendship's push watermarks so drive_chain_replication re-pushes every lane checkpoint fleet-wide (the asker adopts, everyone else no-ops). Not holding them = answer miss.
        for (token, sender_key) in chain_pull_reqs_after {
            // TOKEN-matched (same 2026-09-01 rule as the heal): serve iff we hold a chain FOR THIS TOKEN — a stale-era chain under the same contact must answer miss, or the asking sibling re-keys against a chain we can't actually give it.
            let have_fid = self
                .friendship_chains
                .iter()
                .find(|(_, c)| c.conversation_token == token)
                .map(|(id, _)| *id);
            if let Some(fid) = have_fid {
                let fb = *fid.as_bytes();
                self.chain_pushed_osc.remove(&fb);
                self.lane_pushed_pos.retain(|k, _| k[..32] != fb);
                crate::logf!(
                    "CHAIN-PULL: sibling {} lacks chains for a friendship we hold — watermarks cleared, full re-push next tick",
                    crate::fp(&sender_key)
                );
            } else if let Some(kp) = self.device_keypair.as_ref() {
                if let Ok(frame) = crate::network::fgtw::protocol::build_chain_pull_miss_vsf(
                    &token,
                    kp.public.as_bytes(),
                    kp.secret.as_bytes(),
                ) {
                    self.dispatch_frame_to_siblings(frame);
                }
            }
        }
        // chain_pull miss verdicts (deferred): re-key ONLY when every live sibling contact has answered miss AND the evidence still stands. Offline siblings never answer, so the hold persists until one wakes — correct, its chains would have been clobbered by a premature re-key.
        for (token, sender_key) in chain_pull_misses_after {
            // Only meaningful if WE asked (the miss broadcast reaches every sibling; non-askers drop here).
            if !self.chain_pull_sent.contains(&token) {
                continue;
            }
            self.chain_pull_misses
                .entry(token)
                .or_default()
                .insert(sender_key);
            let all_missed = {
                let misses = &self.chain_pull_misses[&token];
                self.contacts
                    .iter()
                    .filter(|c| c.is_sibling && !c.locked_out)
                    .all(|c| misses.iter().any(|d| c.knows_device(d)))
            };
            if !all_missed {
                continue;
            }
            let us = self
                .session
                .as_ref()
                .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed));
            let Some(ci) = us.and_then(|us| {
                self.contacts.iter().position(|c| {
                    !c.is_sibling
                        && crate::crypto::clutch::derive_conversation_token(&[us, c.handle_hash])
                            == token
                })
            }) else {
                continue;
            };
            // Re-verify the evidence still stands — a chain_sync may have raced the misses in.
            // TOKEN-matched, not fid-matched (2026-09-01, Emma's era-split residue): her contact held A chain — the stale-era one — so the fid check said "chains present" and the heal never fired, while every current-era frame bounced off "no chain for token" forever. The evidence that matters is a chain FOR THIS TOKEN.
            let chains_present = self
                .friendship_chains
                .iter()
                .any(|(_, c)| c.conversation_token == token);
            if self.contacts[ci].clutch_state != crate::types::ClutchState::Complete
                || chains_present
                || self.contacts[ci].clutch_keygen_in_progress
            {
                self.chain_pull_misses.remove(&token);
                continue;
            }
            crate::logf!(
                "CLUTCH: {} — every live sibling answered chain_pull_miss; the fleet holds no chains (wipe debris confirmed) — re-keying",
                crate::fp(&self.contacts[ci].handle_proof)
            );
            self.chain_pull_misses.remove(&token);
            self.rekey_without_chains(ci);
        }

        // Sibling departure requests (deferred): verify the leaver's signature over its own departure request, then surface the approval to THIS user (fleet page pill + toast). The countersign happens only on the human's two-tap approve — never automatically, that's the whole point of bilateral.
        for (t, sig, leaver) in depart_reqs_after {
            let Some(hp) = self.our_handle_proof() else { continue };
            let msg = fgtw::fleet::departreq_signing_bytes(&hp, &leaver, t);
            let valid = ed25519_dalek::VerifyingKey::from_bytes(&leaver)
                .ok()
                .and_then(|vk| {
                    let s: [u8; 64] = sig.as_slice().try_into().ok()?;
                    use ed25519_dalek::Verifier;
                    vk.verify(&msg, &ed25519_dalek::Signature::from_bytes(&s)).ok()
                })
                .is_some();
            if !valid {
                crate::logf!("SECURITY: depart_req from {} carries an INVALID request signature — dropped", crate::fp(&leaver));
                continue;
            }
            let name = self
                .contacts
                .iter()
                .find(|c| c.is_sibling && c.knows_device(&leaver))
                .map(|c| c.display_name())
                .unwrap_or_else(|| "a device".to_string());
            crate::logf!("SECURITY: {} requests removal from the fleet — approve on the Fleet page", name);
            self.pending_depart_req = Some((leaver, t, sig));
            self.ready_toast = Some(format!("{name} asks to sign out of your fleet — approve it in Settings → Fleet."));
            changed = true;
        }

        // Wi-Fi Direct beacon answer-back (the group-up pt_disc exchange's second leg).
        if let (Some(target), Some(session), Some(hq), Some(checker)) = (
            wfd_beacon_reply,
            self.session.as_ref(),
            self.handle_query.as_ref(),
            self.status_checker.as_ref(),
        ) {
            checker.send_lan_unicast(session.handle_proof, hq.port(), target);
        }

        // Consent knocks collected on pong edges (after releasing the checker borrow), plus the roster ride for any Mutual flip this drain confirmed — siblings learn the flip so whichever device is in hand completes the handshake.
        for id in knock_after {
            if let Some(ci) = self.contacts.iter().position(|c| c.id == id) {
                self.send_friend_knock(ci);
            }
        }
        if consent_roster_push {
            self.spawn_roster_push();
        }

        // Parked-ceremony offer re-fires collected on path-up edges (after releasing the checker borrow).
        for idx in offer_refire_indices {
            crate::logf!("CLUTCH: {} direct path came up while the ceremony is parked — re-firing our offer on it", crate::fp(&self.contacts[idx].handle_proof));
            self.contacts[idx].clutch_offer_sent = false;
            self.resend_clutch_offer(idx);
        }

        // Spawn deferred KEM decapsulations (after releasing checker borrow)
        for (contact_id, kem, keypairs) in decap_spawns {
            self.spawn_clutch_kem_decap(contact_id, kem, keypairs);
        }

        // Process deferred ceremony completions (after releasing checker borrow)
        for idx in ceremony_completions {
            self.complete_clutch_ceremony_by_idx(idx);
            changed = true;
        }

        // Deferred probe-before-generate verdict (a blind_srv miss landed while S was None).
        if check_s_genesis {
            self.maybe_generate_s();
        }

        // Ping contacts immediately when a new LAN address is discovered Fixes timing gap: startup ping fires before first LAN discovery arrives
        for idx in lan_ping_indices {
            self.ping_contact(idx);
        }

        // Chain-weave probe (deferred past the checker borrow): fire the one hidden probe for any contact that just reached CLUTCH Complete, then seal any contact whose chain is now proven both ways (their probe seen + our TX ACK-advanced). Order: probe first, then seal, so a probe+ACK that both landed in this same drain still seals in the same pass.
        for idx in chain_probe_indices {
            self.maybe_send_chain_probe(idx);
        }
        for idx in chain_seal_indices {
            self.seal_chain_if_ready(idx);
            // ACK-ADVANCE FLUSH: the serial-send gate holds every message behind the one in flight, so the ACK that just advanced the lane is the edge that releases the next held row at the fresh chain position. No-op when nothing is held.
            self.resend_held_messages(idx);
        }
        // Coalesced off-thread chains persists (ACK pending-removals, chain-sync adopts) — the safe-to-delay saves, now that the checker borrow has ended.
        for fid in chains_persist_fids {
            self.persist_chains_async(&fid);
        }
        // Sibling fork repair (deferred past the checker borrow): apply inbound resets first (each echoes once so the initiator converges), then fire any detector-initiated resets (mint nonce + apply + send).
        for (idx, nonce, echo) in chain_reset_apply {
            self.apply_sibling_chain_reset(idx, nonce, echo);
            changed = true;
        }
        if fleet_sweep_due {
            self.kick_fleet_history_sweep("sibling online");
            self.reserve_fleet_forwards();
        }
        // Fleet-attention state changes (deferred past the checker borrow): void the dead holder first, then adopt the drain's LWW winner, then run the sweeps against the settled slots, then heal.
        if let Some(dead) = attn_void {
            if self.fleet_attention.map_or(false, |(d, _)| d == dead) {
                self.set_fleet_attention(None);
                crate::logf!(
                    "ATTN: holder {} went offline — ball voided",
                    crate::fp(&dead)
                );
            }
        }
        if let Some(v) = attn_adopt {
            self.set_fleet_attention(Some(v));
        }
        for (tok, min_osc) in claim_sweeps {
            self.sweep_undischarged(tok, min_osc);
        }
        if heal_due {
            self.reannounce_attention_state();
        }

        // Retransmit pending messages to contacts that just came online Use last_received_ef6 from pong to only retransmit messages they don't have
        for (fid, peer_addr, alt_addr, handle, recipient_pubkey, last_received_ef6) in
            retransmit_requests
        {
            if let Some((_, chains)) = self.friendship_chains.iter().find(|(id, _)| *id == fid) {
                let pending = chains.pending_messages();
                if !pending.is_empty() {
                    // Filter to only messages newer than what peer has received
                    let to_retransmit: Vec<_> = pending
                        .iter()
                        .filter(|msg| {
                            if let Some(their_last) = last_received_ef6 {
                                msg.eagle_time > their_last
                            } else {
                                // No sync info from peer - retransmit all
                                true
                            }
                        })
                        .collect();

                    if !to_retransmit.is_empty() {
                        crate::logf!("CHAT: Retransmitting {} of {} pending message(s) to {} (came online, last_received={})", to_retransmit.len(), pending.len(), handle, format!("{:?}", last_received_ef6));
                        let conversation_token = chains.conversation_token;
                        let our_lane = chains.our_label().copied().unwrap_or([0u8; 32]);
                        // Came online via relay (no direct path) → retransmit over the pipe too.
                        let relay_to = self
                            .contacts
                            .iter()
                            .find(|c| c.friendship_id == Some(fid))
                            .filter(|c| c.validated_path.is_none())
                            .map(|c| c.relay_device_list())
                            .unwrap_or_default();
                        for msg in to_retransmit {
                            if let Some(ref checker) = self.status_checker {
                                checker.send_message(crate::network::status::MessageRequest {
                                    peer_addr,
                                    alt_addr,
                                    recipient_pubkey,
                                    conversation_token,
                                    lane: our_lane,
                                    prev_msg_hp: msg.prev_msg_hp,
                                    ciphertext: msg.ciphertext.clone(),
                                    eagle_time: msg.eagle_time,
                                    relay_to: relay_to.clone(),
                                });
                                crate::logf!(
                                    "CHAT: Retransmitted msg with eagle_time {} to {}",
                                    msg.eagle_time,
                                    handle
                                );
                            }
                        }
                    } else if !pending.is_empty() {
                        crate::logf!("CHAT: {} pending messages but peer already has them (last_received={})", pending.len(), format!("{:?}", last_received_ef6));
                    }
                }
            }
        }

        // Reliability: per-message retransmit with exponential backoff. The came-online loop above only fires on the offline→online EDGE, so a message (or its ACK) dropped while the peer was already online would otherwise never be resent — the exact desync seen live (msg 1 ACKed, msg 2 garbage because the sender's chain never advanced on a lost ACK). This sweep runs every tick and resends any unacked pending whose backoff deadline has passed, until an ACK clears it or it exhausts its attempts.
        self.retransmit_due_messages();

        // History recovery: fire the next backfill page request for any contact mid-recovery (newest-first cursor; urgent jumps the trickle interval; in-flight expiry re-requests lost pages).
        self.drive_history_recovery();

        // Bridge stream-loss watch: a host that went dark mid-command gets its streamed row stamped closed (idempotent — the stamp ends the in-flight state it keys on).
        self.bridge_watch_stream_loss();

        // Private-identity-secret S: probe/reconstitute/deposit blinds toward whichever friends need an op (no-op at steady state).
        self.drive_blind_ops();

        // Automatic update poll (release channel, ~6–8h jittered, updates.auto-gated): desktop release builds self-apply thru the stamp window; dev builds + Android toast once per version.
        self.drive_auto_update();

        // NOTE: Proactive CLUTCH initiation is now handled via background keygen:
        // 1. spawn_clutch_keygen() is called when contact is added (background thread)
        // 2. check_clutch_keygens() processes results, stores keypairs + ceremony_id
        // 3. Offers are sent from check_clutch_keygens or the KeysGenerated handler above
        // This avoids UI freeze from synchronous McEliece keygen (~100ms) and handle_proof (~1s)

        // Persist any published-name adoptions from this drain (deferred: saving inside the loop would fight the contacts borrow). The name lives in the per-contact STATE entry, not the index. OFF-THREAD, snapshot pattern — these fired inline on the UI thread at ~900ms per vault write in the 2026-08-21 field capture; the batch of snapshots rides ONE worker job (and the librarian's group commit makes the burst one flush).
        let name_adopted = self.contacts.iter().any(|c| c.published_name_dirty);
        if name_adopted {
            if let Some(storage) = self.storage.as_ref().cloned() {
                let snapshots: Vec<crate::types::Contact> = self
                    .contacts
                    .iter_mut()
                    .filter(|c| c.published_name_dirty)
                    .map(|c| {
                        c.published_name_dirty = false;
                        c.clone()
                    })
                    .collect();
                queue_job(&self.seal_job_tx, move || {
                    for snapshot in &snapshots {
                        if let Err(e) =
                            crate::storage::contacts::save_contact_state(snapshot, &storage)
                        {
                            crate::logf!("CONTACT: published-name persist failed: {}", e);
                        }
                    }
                });
            }
        }

        // Persist + fetch any avatar-pin adoptions from this drain: the pin lives in the contact-list INDEX (save_contact_list rewrites it), and a fresh pin means a fresh avatar to pull.
        let avatar_adopted: Vec<usize> = self
            .contacts
            .iter()
            .enumerate()
            .filter(|(_, c)| c.avatar_pin_dirty)
            .map(|(i, _)| i)
            .collect();
        let avatar_changed = !avatar_adopted.is_empty();
        if avatar_changed {
            for c in self.contacts.iter_mut() {
                c.avatar_pin_dirty = false;
            }
            // A fresh pin means a fresh avatar to pull — the sweep's remembered probes are stale.
            self.avatar_probe_cache.clear();
            // OFF-THREAD (snapshot): the index rewrite is a full vault write — ~900ms inline on the UI thread in the 2026-08-21 field capture, and pin adoptions arrive with every fstate merge cycle.
            if let Some(storage) = self.storage.as_ref().cloned() {
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
                queue_job(&self.seal_job_tx, move || {
                    if let Err(e) = crate::storage::contacts::save_contact_list(&index, &storage) {
                        crate::logf!("CONTACT: avatar-pin persist failed: {}", e);
                    }
                });
            }
            for i in avatar_adopted {
                // A rotated pin names a NEW avatar: drop the once-per-session latch first, or the fetch dedups itself into a no-op and the new picture never arrives until a restart (one device picked, the peer kept the old face — 2026-08-02).
                if let Some(hp) = self.contacts.get(i).map(|c| c.handle_proof) {
                    self.avatar_dl_started.remove(&hp);
                }
                // Evict the party-id avatar cache too — the fetch worker is local-first now, and a raw-AV1 cache entry decodes fine under ANY pin, so without this eviction the old face would be re-served forever.
                if let (Some(c), Some(storage)) = (self.contacts.get(i), self.storage.as_ref()) {
                    let _ =
                        storage.delete_addr(&crate::storage::vault_key("avatar", &c.handle_hash));
                }
                self.spawn_avatar_download(i);
            }
        }
        // A fresh pin or published name is roster state (PRST4) — push so offline-at-the-time siblings still converge (merge-idempotent, so a field that ARRIVED via roster merge just round-trips a no-op).
        if name_adopted || avatar_changed {
            self.spawn_roster_push();
        }

        // RE-PROBE ON PRESENCE: the weave probe fired only on ceremony edges, so a completed-but-unwoven chain whose one-shot never dispatched — the pre-relay-fallback address bail, or a restart before the ACK landed — had NO edge left to fire it and both sides sat at "testing the secure channel" forever (live pair, 2026-08-06). Any drain pass with the contact online re-arms it; the maybe_ gate (Complete, !probe_sent, has remote) keeps it one frame per session until the seal re-arm cycles it, and the seal call right after catches the half-proven shapes the moment the missing half lands.
        let reprobe: Vec<usize> = self
            .contacts
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                !c.is_sibling
                    && c.is_online
                    && c.clutch_state == crate::types::ClutchState::Complete
                    && !c.chain_woven
                    && !c.probe_sent
            })
            .map(|(i, _)| i)
            .collect();
        for idx in reprobe {
            self.maybe_send_chain_probe(idx);
            self.seal_chain_if_ready(idx);
        }

        // Content marks the scene dirty HERE, not via the caller's return: the Android foreground SERVICE also runs this drain headless (nativeServiceTick → advance_protocol) and drops the returned bool — so a presence flip or name/pin adoption that landed while backgrounded painted nothing on resume (the field "online ring is stale until you click thru", 2026-08-08). scene_dirty is app state, so marking it at the mutation site survives the headless window and the first visible frame repaints.
        self.scene_dirty |= changed;
        {
            let ms = deferred_t.elapsed().as_millis();
            if ms > 100 {
                crate::logf!("PERF: status deferred took {}ms (UI thread)", ms as u64);
            }
        }
        changed
    }
}
