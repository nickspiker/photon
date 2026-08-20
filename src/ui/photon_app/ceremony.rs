//! The CLUTCH ceremony — keygen/encap/ceremony spawns, their check drains, and ceremony completion.

use super::*;

impl PhotonApp {
    pub fn spawn_clutch_keygen(
        &self,
        contact_id: ContactId,
        _our_handle_hash: [u8; 32],
        _their_handle_hash: [u8; 32],
    ) {
        use crate::crypto::clutch::generate_all_ephemeral_keypairs;

        let tx = self.clutch_keygen_tx.clone();
        #[cfg(not(target_os = "android"))]
        let proxy = self.event_proxy.clone();

        // Keypair generation includes McEliece460896 — very CPU-heavy (large matrix build). On resume every Pending contact re-keys at once (two contacts = two McEliece keygens in parallel), so this MUST run at Min priority or it starves the UI render thread and the window freezes until keygen finishes — the "GUI loads but you can't do anything until it syncs" symptom. Matches the Min-priority KEM-encap and ceremony-expand threads.
        let thread_body = move || {
            #[cfg(feature = "development")]
            crate::log("CLUTCH: Background keypair generation started...");
            let keypairs = generate_all_ephemeral_keypairs();
            crate::log(
                "CLUTCH: Keypairs ready (ceremony_id computed when ping provenances available)",
            );

            let _ = tx.send(ClutchKeygenResult {
                contact_id,
                keypairs,
            });

            // Wake the event loop so it processes the result
            #[cfg(not(target_os = "android"))]
            if let Some(p) = proxy.as_ref() {
                let _ = p.send(crate::ui::PhotonEvent::ClutchKeygenComplete);
            }
        };

        #[cfg(not(target_os = "redox"))]
        {
            use thread_priority::{ThreadBuilderExt, ThreadPriority};
            std::thread::Builder::new()
                .name("clutch-keygen".to_string())
                .spawn_with_priority(ThreadPriority::Min, move |_| thread_body())
                .expect("Failed to spawn CLUTCH keygen thread");
        }
        #[cfg(target_os = "redox")]
        {
            std::thread::Builder::new()
                .name("clutch-keygen".to_string())
                .spawn(thread_body)
                .expect("Failed to spawn CLUTCH keygen thread");
        }
    }

    /// Spawn background thread to perform CLUTCH KEM encapsulation. The PQ KEMs (~800ms total) are slow, so we do them off the main thread. Results are received via clutch_kem_encap_rx and processed in check_clutch_kem_encaps().
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_clutch_kem_encap(
        &self,
        contact_id: ContactId,
        their_offer: crate::crypto::clutch::ClutchOfferPayload,
        ceremony_id: [u8; 32],
        conversation_token: [u8; 32],
        peer_addr: std::net::SocketAddr,
    ) {
        use crate::crypto::clutch::ClutchKemResponsePayload;

        let tx = self.clutch_kem_encap_tx.clone();
        #[cfg(not(target_os = "android"))]
        let proxy = self.event_proxy.clone();

        let thread_body = move || {
            #[cfg(feature = "development")]
            #[cfg(feature = "development")]
            crate::log("CLUTCH: Background KEM encapsulation started (low priority)...");
            let Some((kem_response, local_secrets)) =
                ClutchKemResponsePayload::encapsulate_to_peer(&their_offer)
            else {
                // Malformed key material (old-build or hostile offer) — DROP, never panic: this exact shape was crashing a field peer's app on every received offer (2026-08-02).
                crate::log(
                    "CLUTCH: offer carries malformed key material (version skew?) — dropped",
                );
                return;
            };
            #[cfg(feature = "development")]
            #[cfg(feature = "development")]
            crate::log("CLUTCH: KEM encapsulation complete");

            let _ = tx.send(ClutchKemEncapResult {
                contact_id,
                kem_response,
                local_secrets,
                ceremony_id,
                conversation_token,
                peer_addr,
            });

            // Wake the event loop so it processes the result
            #[cfg(not(target_os = "android"))]
            if let Some(p) = proxy.as_ref() {
                let _ = p.send(crate::ui::PhotonEvent::ClutchKemEncapComplete);
            }
        };

        #[cfg(not(target_os = "redox"))]
        {
            use thread_priority::{ThreadBuilderExt, ThreadPriority};
            std::thread::Builder::new()
                .name("clutch-kem-encap".to_string())
                .spawn_with_priority(ThreadPriority::Min, move |_| thread_body())
                .expect("Failed to spawn KEM encap thread");
        }
        #[cfg(target_os = "redox")]
        {
            std::thread::Builder::new()
                .name("clutch-kem-encap".to_string())
                .spawn(thread_body)
                .expect("Failed to spawn KEM encap thread");
        }
    }

    /// Spawn background thread to decapsulate a peer's KEM response — 8 PQ decapsulations against our secret keys, the fourth CLUTCH job stage. Ran inline in three drain arms until 2026-08-15 (the last of the 2026-08-08 "deliberately inline" residue); on a throttled phone the inline open was the visible "weaving the chain" UI freeze. Results drain in check_clutch_kem_decaps(); the caller sets clutch_kem_decap_in_progress to serialize.
    pub fn spawn_clutch_kem_decap(
        &self,
        contact_id: ContactId,
        kem: crate::crypto::clutch::ClutchKemResponsePayload,
        keypairs: crate::crypto::clutch::ClutchAllKeypairs,
    ) {
        use crate::crypto::clutch::ClutchKemSharedSecrets;
        use crate::network::ClutchKemDecapResult;

        let tx = self.clutch_kem_decap_tx.clone();
        #[cfg(not(target_os = "android"))]
        let proxy = self.event_proxy.clone();

        let thread_body = move || {
            #[cfg(feature = "development")]
            crate::log("CLUTCH: Background KEM decapsulation started (low priority)...");
            let keypair_hqc_prefix: [u8; 8] = keypairs.hqc256_public[..8]
                .try_into()
                .expect("hqc public >= 8 bytes");
            let remote_secrets = ClutchKemSharedSecrets::decapsulate_from_peer(&kem, &keypairs);
            #[cfg(feature = "development")]
            crate::log("CLUTCH: KEM decapsulation complete");

            let _ = tx.send(ClutchKemDecapResult {
                contact_id,
                remote_secrets,
                keypair_hqc_prefix,
            });

            // Wake the event loop so it processes the result
            #[cfg(not(target_os = "android"))]
            if let Some(p) = proxy.as_ref() {
                let _ = p.send(crate::ui::PhotonEvent::ClutchKemDecapComplete);
            }
        };

        #[cfg(not(target_os = "redox"))]
        {
            use thread_priority::{ThreadBuilderExt, ThreadPriority};
            std::thread::Builder::new()
                .name("clutch-kem-decap".to_string())
                .spawn_with_priority(ThreadPriority::Min, move |_| thread_body())
                .expect("Failed to spawn KEM decap thread");
        }
        #[cfg(target_os = "redox")]
        {
            std::thread::Builder::new()
                .name("clutch-kem-decap".to_string())
                .spawn(thread_body)
                .expect("Failed to spawn KEM decap thread");
        }
    }

    /// Drain background KEM-decap results: the one place decapped secrets enter a slot, consolidating what three inline arms each half-did (store, offer backfill, encap trigger, completion check).
    /// CAS discipline per the 2026-08-08 offload laws: the result carries the HQC prefix of the keypairs that decapped; a contact whose CURRENT keypairs differ (round torched + re-keyed mid-flight) drops the result untouched — same staleness identity the wire uses.
    pub fn check_clutch_kem_decaps(&mut self) -> bool {
        use crate::crypto::clutch::derive_conversation_token;

        let mut changed = false;
        let our_handle_hash = match self
            .session
            .as_ref()
            .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
        {
            Some(h) => h,
            None => return changed,
        };

        let mut kem_encap_spawn: Option<(
            ContactId,
            crate::crypto::clutch::ClutchOfferPayload,
            [u8; 32],
            [u8; 32],
            std::net::SocketAddr,
        )> = None;
        let mut ceremony_completions: Vec<usize> = Vec::new();

        while let Ok(result) = self.clutch_kem_decap_rx.try_recv() {
            let Some(idx) = self.contacts.iter().position(|c| c.id == result.contact_id) else {
                crate::log("CLUTCH: decap result for a contact that no longer exists — dropped");
                continue;
            };
            let contact = &mut self.contacts[idx];
            contact.clutch_kem_decap_in_progress = false;

            // CAS: only install into the keypair generation that decapped. A torch mid-flight minted new keypairs; these secrets belong to a dead round.
            let current_prefix: Option<[u8; 8]> = contact.clutch_our_keypairs.as_ref().map(|k| {
                k.hqc256_public[..8]
                    .try_into()
                    .expect("hqc public >= 8 bytes")
            });
            if current_prefix != Some(result.keypair_hqc_prefix) {
                crate::logf!(
                    "CLUTCH: decap result is for a superseded keypair generation of {} — dropped",
                    crate::fp(&contact.handle_proof)
                );
                continue;
            }
            let Some(remote_secrets) = result.remote_secrets else {
                crate::log(
                    "CLUTCH: KEM response carries malformed material — dropped (version skew?)",
                );
                continue;
            };

            let remote_hash = contact.handle_hash;
            if let Some(remote_slot) = contact.get_slot_mut(&remote_hash) {
                if remote_slot.kem_secrets_from_them.is_some() {
                    crate::logf!(
                        "CLUTCH: duplicate decap for {} — slot already holds their secrets, dropped",
                        crate::fp(&contact.handle_proof)
                    );
                    continue;
                }
                remote_slot.kem_secrets_from_them = Some(remote_secrets);
                crate::logf!(
                    "CLUTCH: Decapsulated KEM from {} - stored in slot",
                    crate::fp(&contact.handle_proof)
                );
            } else {
                continue;
            }
            changed = true;

            // Backfill OUR offer in OUR slot if missing — guarantees all_slots_complete can fire here. Covers the stall where our own offer was never recorded (offer arrived before our keygen, or the offer-received path didn't store it), leaving our slot offer=None forever even though we have keys + KEM secrets.
            if contact
                .get_slot(&our_handle_hash)
                .map(|s| s.offer.is_none())
                .unwrap_or(false)
            {
                if let Some(ref keypairs) = contact.clutch_our_keypairs {
                    let our_offer =
                        crate::crypto::clutch::ClutchOfferPayload::from_keypairs(keypairs);
                    if let Some(local_slot) = contact.get_slot_mut(&our_handle_hash) {
                        local_slot.offer = Some(our_offer);
                        crate::log("CLUTCH: Backfilled our own offer in local slot (on decap)");
                    }
                }
            }

            // If we haven't sent our own KEM encap yet, do it now. This covers the case where their KEM arrived before we had ceremony_id, so the normal encap-trigger was skipped.
            let already_sent_kem = contact
                .get_slot(&our_handle_hash)
                .map(|s| s.kem_secrets_to_them.is_some())
                .unwrap_or(false);
            if !already_sent_kem
                && !contact.clutch_kem_encap_in_progress
                && kem_encap_spawn.is_none()
            {
                if let Some(ceremony_id) = contact.ceremony_id {
                    // Same relay fallback as the keygen arm — a queued KEM must drain even with no address.
                    let ip = contact.ip.unwrap_or(crate::network::status::RELAY_ADDR);
                    let conv_token =
                        derive_conversation_token(&[our_handle_hash, contact.handle_hash]);
                    let remote_offer = contact
                        .get_slot(&contact.handle_hash)
                        .and_then(|s| s.offer.clone());
                    if let Some(remote_offer) = remote_offer {
                        contact.clutch_kem_encap_in_progress = true;
                        kem_encap_spawn = Some((
                            contact.id.clone(),
                            remote_offer,
                            ceremony_id,
                            conv_token,
                            ip,
                        ));
                        crate::logf!(
                            "CLUTCH: Spawning KEM encap for {} after decap",
                            crate::fp(&contact.handle_proof)
                        );
                    }
                }
            }

            if contact.all_slots_complete() {
                crate::logf!(
                    "CLUTCH: All slots complete for {} after decap - triggering ceremony completion",
                    crate::fp(&contact.handle_proof)
                );
                ceremony_completions.push(idx);
            }
        }

        // Spawn deferred KEM encapsulation after releasing contacts borrow
        if let Some((contact_id, offer, ceremony_id, conv_token, peer_addr)) = kem_encap_spawn {
            self.spawn_clutch_kem_encap(contact_id, offer, ceremony_id, conv_token, peer_addr);
        }

        // Process deferred ceremony completions (after releasing contacts borrow)
        for idx in ceremony_completions {
            self.complete_clutch_ceremony_by_idx(idx);
            changed = true;
        }

        changed
    }

    /// Spawn background thread to complete CLUTCH ceremony (avalanche_expand). The 2MB memory-hard expansion (~850ms) is slow, so we do it off the main thread. Results are received via clutch_ceremony_rx and processed in check_clutch_ceremonies().
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_clutch_ceremony(
        &self,
        contact_id: ContactId,
        our_handle_hash: [u8; 32],
        their_handle_hash: [u8; 32],
        our_device_pub: [u8; 32],
        their_device_pub: [u8; 32],
        friendship_secret: [u8; 32],
        secrets: crate::crypto::clutch::ClutchSharedSecrets,
        ceremony_id: [u8; 32],
        conversation_token: [u8; 32],
        peer_addr: std::net::SocketAddr,
        their_hqc_prefix: [u8; 8],
    ) {
        use crate::crypto::clutch::clutch_complete_full;

        let tx = self.clutch_ceremony_tx.clone();
        #[cfg(not(target_os = "android"))]
        let proxy = self.event_proxy.clone();

        let thread_body = move || {
            // A ceremony is several round trips over ~570KB with no mailbox behind it: if the machine idles out mid-exchange the in-flight frames are simply discarded and the two sides end up holding different halves. The guard drops with this closure, so it protects the ceremony and not a minute longer.
            let _awake = crate::platform::stay_awake::SleepGuard::hold("photon: CLUTCH ceremony");
            #[cfg(feature = "development")]
            #[cfg(feature = "development")]
            crate::log("CLUTCH: Background ceremony completion started (low priority)...");

            // Phase 1: Compute eggs (moderately fast)
            let result = clutch_complete_full(
                &our_device_pub,
                &their_device_pub,
                &our_handle_hash,
                &their_handle_hash,
                &friendship_secret,
                &secrets,
            );

            // The fan-out pair secret rides the SAME eggs (Phase A) — derive it here, while they're alive, so a sibling wrap can be sealed post-quantum. Cheap next to the expansion below.
            let fanout_pair_secret = crate::crypto::clutch::derive_fanout_pair_secret(
                &our_device_pub,
                &their_device_pub,
                &result.eggs,
            );

            // Phase 2: Expand to 2MB and derive chains (slow - avalanche_expand)
            let friendship_chains = FriendshipChains::from_clutch(
                &[our_handle_hash, their_handle_hash],
                result.eggs.as_slice(),
            );

            #[cfg(feature = "development")]
            #[cfg(feature = "development")]
            crate::log("CLUTCH: Ceremony completion finished");

            let _ = tx.send(ClutchCeremonyResult {
                contact_id,
                friendship_chains,
                eggs_proof: result.proof,
                their_handle_hash,
                ceremony_id,
                conversation_token,
                peer_addr,
                their_hqc_prefix,
                fanout_pair_secret,
            });

            // Wake the event loop so it processes the result
            #[cfg(not(target_os = "android"))]
            if let Some(p) = proxy.as_ref() {
                let _ = p.send(crate::ui::PhotonEvent::ClutchCeremonyComplete);
            }
        };

        #[cfg(not(target_os = "redox"))]
        {
            use thread_priority::{ThreadBuilderExt, ThreadPriority};
            std::thread::Builder::new()
                .name("clutch-ceremony".to_string())
                .spawn_with_priority(ThreadPriority::Min, move |_| thread_body())
                .expect("Failed to spawn ceremony thread");
        }
        #[cfg(target_os = "redox")]
        {
            std::thread::Builder::new()
                .name("clutch-ceremony".to_string())
                .spawn(thread_body)
                .expect("Failed to spawn ceremony thread");
        }
    }

    /// Process background CLUTCH key generation results.
    ///
    /// Slot-based design: keypairs stored once, slots filled as messages arrive. Ceremony completes when all slots have offer + both KEM secret directions.
    pub fn check_clutch_keygens(&mut self) -> bool {
        use crate::crypto::clutch::{derive_conversation_token, ClutchOfferPayload};
        use crate::network::status::ClutchOfferRequest;
        use crate::types::CeremonyId;

        let mut changed = false;
        let mut ceremony_completions: Vec<usize> = Vec::new();
        // Deferred KEM encapsulation spawn (to avoid borrow conflict)
        let mut kem_encap_spawn: Option<(
            ContactId,
            ClutchOfferPayload,
            [u8; 32],
            [u8; 32],
            std::net::SocketAddr,
        )> = None;
        // Deferred KEM decapsulation spawns (same borrow-conflict deferral as the encap)
        let mut decap_spawns: Vec<(
            ContactId,
            crate::crypto::clutch::ClutchKemResponsePayload,
            crate::crypto::clutch::ClutchAllKeypairs,
        )> = Vec::new();

        // Our party id for CLUTCH: the identity pubkey (public; contacts pin it — never the seed).
        let our_handle_hash = match self
            .session
            .as_ref()
            .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
        {
            Some(h) => h,
            None => return changed,
        };
        let device_pubkey = *self
            .device_keypair
            .as_ref()
            .expect("device_keypair set in init")
            .public
            .as_bytes();
        let device_secret = *self
            .device_keypair
            .as_ref()
            .expect("device_keypair set in init")
            .secret
            .as_bytes();

        let mut claimed_ownership = false;
        while let Ok(result) = self.clutch_keygen_rx.try_recv() {
            let result_id_hex = hex::encode(&result.contact_id.as_bytes()[..4]);
            crate::logf!(
                "CLUTCH: Processing keygen result for contact_id {}...",
                result_id_hex
            );

            let siblings = sibling_presence_snapshot(&self.contacts);
            let mut found = false;
            for (idx, contact) in self.contacts.iter_mut().enumerate() {
                if contact.id == result.contact_id {
                    found = true;

                    // Party-id seam: shadow the hoisted seed with THIS contact's "our" id — the device-derived sibling pid for fleet siblings (same handle ⇒ the seed would collide), the identity seed for friends. Every slot lookup / token / ceremony-id below then keys correctly with no further edits.
                    let our_handle_hash = if contact.is_sibling {
                        crate::crypto::clutch::sibling_party_id(&device_pubkey)
                    } else {
                        our_handle_hash
                    };

                    // Clear the in-progress flag now that keygen is complete
                    contact.clutch_keygen_in_progress = false;

                    // ZERO-REMOTE GUARD: a conversation with no remote participants has no ceremony to run, so keys landing here are MIS-ROUTED — the notes row shared its ContactId with the sibling row for its first-met device (both derived blake3 of the same pubkey), this first-match scan installed the sibling's re-key onto it, and the row then offered 573KB at our own fleet on the self-pair token forever (field, 2026-08-13). The sibling's own keygen re-fires from its ceremony edges; nothing zero-remote may ever hold a round.
                    if contact.remote_count(&our_handle_hash) == 0 {
                        crate::logf!(
                            "CLUTCH: keygen result for zero-remote conversation {} — discarded (nothing to exchange)",
                            crate::fp(&contact.handle_proof).as_str()
                        );
                        break;
                    }

                    // FLEET-FIRST belt-and-braces: chains adopted while this keygen ground (replication flipped the contact Complete) — the ceremony is already unnecessary; installing keys would re-arm a round on a finished friendship and offer at a friend who long since has the chain.
                    if contact.clutch_state == crate::types::ClutchState::Complete {
                        crate::logf!(
                            "CLUTCH: keygen result for {} — discarded, chains already adopted (Complete)",
                            crate::fp(&contact.handle_proof).as_str()
                        );
                        break;
                    }

                    // §4.2: a claim landed while keygen was running (roster merge parked this contact mid-flight) — installing the result would resurrect the parked round and re-send a competing offer. Drop it on the floor.
                    if ceremony_parked_by(contact, Some(device_pubkey), &siblings) {
                        // RESPONDER EXCEPTION, gated on a STALE owner: the friend's offer sitting in their slot deadlocks a parked device when their build offers to one device only (observed live against a v0.40 peer: the drain dropped the responding keygen, their KEM/proofs then fell on a keyless Pending contact forever, both sides stalled) — so ownership may FOLLOW the friend's offer. But a friend's offer is NOT a choice of device: a responder's offer fans out over the relay to the whole fleet, so every sibling receives it — and ungated, each one "followed the choice", stole the ceremony from a sibling actively running it, and minted its own round (three concurrent rounds, cross-round proof drops on every side, the ceremony stuck "testing the secure channel" — live pair, 2026-08-03). The roster clock separates the two shapes: a live owner claimed within the round TTL, a deadlocked one has been silent past it. Recent claim → park and drop the keygen below, the owner's round completes and fleet sync carries the chains; stale claim → the original rescue: claim, bump the LWW clock so siblings adopt + discard-on-park, and install the keys.
                        let their_offer_waiting = contact
                            .get_slot(&contact.handle_hash)
                            .map_or(false, |s| s.offer.is_some());
                        let owner_stale = vsf::eagle_time_oscillations()
                            .saturating_sub(contact.roster_updated)
                            > CLUTCH_ROUND_TTL_OSC;
                        if their_offer_waiting && owner_stale {
                            // FAN-OUT TIE-BREAK, no sync channel required: the offer that "chose this device" also chose every sibling — it fanned out over the relay — and with the roster clock stale on BOTH siblings (mid-session, no fstate edge between them), each read "owner silent", claimed, and minted competing rounds that adoption-cooldowns then locked in place on all three parties (live pair, 2026-08-05). Same doctrine as the sibling initiator rule: the lowest ONLINE fleet device claims at the TTL; a higher device defers, time-boxed to one more TTL so a winner that dies mid-round still gets rescued.
                            // Defer unless the lower device is PROBED-OFFLINE: requiring online-and-probed meant a boot or one flapped probe read as "no lower device" and both siblings claimed — the presence-luck dual-claim this tie-break exists to end (live pair, 2026-08-06). Unknown presence defers; only a confirmed-dead winner forfeits its turn (and the one-TTL deference cap still rescues a silently dead one).
                            let lower_sibling_online =
                                siblings.iter().any(|(k, online, probed)| {
                                    (*online || !*probed) && k < &device_pubkey
                                });
                            let deference_expired =
                                contact.clutch_claim_deferred.map_or(false, |t| {
                                    t.elapsed().as_secs() as i64
                                        > CLUTCH_ROUND_TTL_OSC / vsf::OSCILLATIONS_PER_SECOND as i64
                                });
                            if lower_sibling_online && !deference_expired {
                                if contact.clutch_claim_deferred.is_none() {
                                    contact.clutch_claim_deferred = Some(std::time::Instant::now());
                                }
                                crate::logf!("CLUTCH §4.2: {} offered here but a lower fleet device is online — deferring the claim one TTL, its round carries the ceremony", crate::fp(&contact.handle_proof));
                                contact.discard_clutch_round();
                                changed = true;
                                break;
                            }
                            contact.clutch_claim_deferred = None;
                            crate::logf!("CLUTCH §4.2: {} offered at THIS device and the named owner has been silent past the round TTL — claiming the ceremony", crate::fp(&contact.handle_proof));
                            contact.ceremony_owner = Some(device_pubkey);
                            contact.owner_woven = false;
                            contact.roster_updated = vsf::eagle_time_oscillations();
                            claimed_ownership = true;
                        } else {
                            if their_offer_waiting {
                                crate::logf!("CLUTCH §4.2: {} offered here but the named owner claimed recently — parking, the owner's round carries the ceremony", crate::fp(&contact.handle_proof));
                            }
                            crate::logf!("CLUTCH §4.2: dropping keygen result for {} — round was parked while keygen ran", crate::fp(&contact.handle_proof));
                            contact.discard_clutch_round();
                            changed = true;
                            break;
                        }
                    }

                    // Store keypairs (ceremony_id computed on-demand when provenances available)
                    contact.clutch_our_keypairs = Some(result.keypairs);
                    // Stamp the round start (eagle time): this is the moment a round's keys exist. A resume that reloads contacts from disk wipes these ephemeral keys — a fresh stamp lets the resume RESTORE the round instead of the sweep minting a divergent one, and gates re-key on real staleness (see Contact::clutch_round_started).
                    contact.clutch_round_started = Some(vsf::eagle_time_oscillations());
                    // OWNER KEEPALIVE: takeover reads the roster entry's LWW clock, and nothing else bumps it while the owner grinds — after one quiet TTL every sibling read "owner silent", claimed, and the fleet re-entered dual-writer churn (live pair, 2026-08-06). A working owner re-stamps its claim at each round mint; the push after the drain carries it, and siblings' owner_stale stays honest.
                    if !contact.is_sibling && contact.ceremony_owner == Some(device_pubkey) {
                        contact.roster_updated = vsf::eagle_time_oscillations();
                        claimed_ownership = true;
                    }
                    changed = true;

                    // Persist keypairs to disk immediately (crash recovery)
                    if let (Some(ref keypairs), Some(storage)) =
                        (&contact.clutch_our_keypairs, self.storage.as_ref())
                    {
                        if let Err(e) = crate::storage::contacts::save_clutch_keypairs(
                            keypairs,
                            &contact.handle_hash,
                            storage,
                        ) {
                            crate::logf!(
                                "CLUTCH: Failed to save keypairs for {}: {}",
                                crate::fp(&contact.handle_proof),
                                e
                            );
                        }
                    }

                    // Initialize slots if not done yet (sorted by handle_hash)
                    if contact.clutch_slots.is_empty() {
                        contact.init_clutch_slots(our_handle_hash);
                    }

                    // Check if their slot has an offer (received before keygen completed)
                    let their_slot_has_offer = contact
                        .get_slot(&contact.handle_hash)
                        .map(|s| s.offer.is_some())
                        .unwrap_or(false);

                    // Store local offer in local slot
                    if let Some(ref keypairs) = contact.clutch_our_keypairs {
                        let our_offer = ClutchOfferPayload::from_keypairs(keypairs);
                        if let Some(local_slot) = contact.get_slot_mut(&our_handle_hash) {
                            local_slot.offer = Some(our_offer);
                            crate::logf!(
                                "CLUTCH: Stored local offer in local slot for {}",
                                crate::fp(&contact.handle_proof)
                            );
                        } else {
                            crate::logf!(
                                "CLUTCH: Could not find local slot for {} - handle_hash mismatch?",
                                crate::fp(&contact.handle_proof)
                            );
                        }
                    }

                    // Send our offer if not already sent (don't wait for ceremony_id - that comes later)
                    if !contact.clutch_offer_sent {
                        // No address → the RELAY sentinel; the relay_to fan-out below is the real delivery. This gate silently skipping on ip=None (relayed pongs never set it) is what held offers hostage to address discovery — the weave stalled Pending while the relay carried every other message fine.
                        {
                            let ip = contact.ip.unwrap_or(crate::network::status::RELAY_ADDR);
                            if let Some(ref keypairs) = contact.clutch_our_keypairs {
                                use crate::network::fgtw::protocol::build_clutch_offer_vsf;

                                let offer = ClutchOfferPayload::from_keypairs(keypairs);
                                let conv_token = derive_conversation_token(&[
                                    our_handle_hash,
                                    contact.handle_hash,
                                ]);

                                // Build VSF and capture our offer_provenance. The pinned send-time (clutch_round_started) makes the provenance stable across re-sends so the clutch never rotates.
                                match build_clutch_offer_vsf(
                                    &conv_token,
                                    &offer,
                                    &device_pubkey,
                                    &device_secret,
                                    contact
                                        .clutch_round_started
                                        .unwrap_or_else(vsf::eagle_time_oscillations),
                                ) {
                                    Ok((vsf_bytes, our_offer_provenance)) => {
                                        // Store our offer provenance (for ceremony_id derivation)
                                        if !contact
                                            .offer_provenances
                                            .contains(&our_offer_provenance)
                                        {
                                            contact.offer_provenances.push(our_offer_provenance);
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
                                                crate::logf!(
                                                    "Failed to persist CLUTCH provenance: {}",
                                                    e
                                                );
                                            }
                                        }

                                        if let Some(ref checker) = self.status_checker {
                                            let (primary, alt) =
                                                contact.race_addrs().unwrap_or((ip, None));
                                            checker.send_offer(ClutchOfferRequest {
                                                peer_addr: primary,
                                                alt_addr: alt,
                                                vsf_bytes,
                                                recipient_pubkey: contact.public_identity.key,
                                                // CEREMONY frames always carry the FULL relay fan-out, never the direct-trust heuristic: a validated path proves ONE device of the identity is reachable, but the ceremony's owner can be a different device with no direct path at all — the reply rode direct to the reachable sibling, the relay stayed suppressed, and the owner starved awaiting it (live pair, 2026-08-05). Ceremony frames are rare; receivers dedup; the relay copy is cheap insurance that the one device that NEEDS the frame gets it.
                                                relay_to: contact.relay_device_list(),
                                            });
                                            contact.clutch_offer_sent = true;
                                            crate::logf!(
                                                "CLUTCH: Sent offer to {} (prov={}...)",
                                                crate::fp(&contact.handle_proof),
                                                hex::encode(&our_offer_provenance[..4])
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        crate::logf!(
                                            "CLUTCH: Failed to build offer VSF for {}: {}",
                                            crate::fp(&contact.handle_proof),
                                            e
                                        );
                                    }
                                }
                            }
                        }
                    }

                    // Compute ceremony_id if we have enough offer provenances (2 for DM)
                    let required_provenances = 2;
                    if contact.ceremony_id.is_none()
                        && contact.offer_provenances.len() >= required_provenances
                    {
                        let ceremony_id = *CeremonyId::derive(
                            &[our_handle_hash, contact.handle_hash],
                            &contact.offer_provenances,
                        )
                        .as_bytes();
                        contact.ceremony_id = Some(ceremony_id);
                        crate::logf!(
                            "CLUTCH: Computed ceremony_id for {} from {} offer provenances",
                            crate::fp(&contact.handle_proof),
                            contact.offer_provenances.len()
                        );
                    }

                    // Send KEM response if we have ceremony_id and their offer
                    if their_slot_has_offer {
                        let already_sent_kem = contact
                            .get_slot(&our_handle_hash)
                            .map(|s| s.kem_secrets_to_them.is_some())
                            .unwrap_or(false);

                        if !already_sent_kem && !contact.clutch_kem_encap_in_progress {
                            if let Some(ceremony_id) = contact.ceremony_id {
                                // No address → RELAY sentinel; the KEM response fans out over relay_to like every ceremony message. Gating on ip stalled the middle of the weave the same way it stalled the offer.
                                {
                                    let ip =
                                        contact.ip.unwrap_or(crate::network::status::RELAY_ADDR);
                                    let conv_token = derive_conversation_token(&[
                                        our_handle_hash,
                                        contact.handle_hash,
                                    ]);
                                    let remote_offer = contact
                                        .get_slot(&contact.handle_hash)
                                        .and_then(|s| s.offer.clone());

                                    if let Some(remote_offer) = remote_offer {
                                        // Defer spawn for KEM encapsulation (to avoid borrow conflict) (PQ crypto is slow ~800ms, would block UI/network)
                                        contact.clutch_kem_encap_in_progress = true;
                                        kem_encap_spawn = Some((
                                            contact.id.clone(),
                                            remote_offer,
                                            ceremony_id,
                                            conv_token,
                                            ip,
                                        ));
                                        crate::logf!("CLUTCH: Will spawn KEM encapsulation for {} (post-keygen)", crate::fp(&contact.handle_proof));
                                    }
                                }
                            } else {
                                crate::logf!("CLUTCH: Keypairs ready for {} - need ceremony_id for KEM response (have {} offer provenances)", crate::fp(&contact.handle_proof), contact.offer_provenances.len());
                            }
                        }
                    }

                    // Process any pending KEM response that arrived before keygen completed. Also compute ceremony_id here if provenances are ready — the KEM may have arrived in the network thread between when we added our provenance and when the main loop got here to run the ceremony_id derivation above.
                    if contact.clutch_pending_kem.is_some() {
                        if contact.ceremony_id.is_none() && contact.offer_provenances.len() >= 2 {
                            let ceremony_id = *CeremonyId::derive(
                                &[our_handle_hash, contact.handle_hash],
                                &contact.offer_provenances,
                            )
                            .as_bytes();
                            contact.ceremony_id = Some(ceremony_id);
                            crate::logf!(
                                "CLUTCH: Computed ceremony_id for {} while draining queued KEM",
                                crate::fp(&contact.handle_proof)
                            );
                        }
                    }

                    // Queued KEM response: hand it to the decap job (8 PQ opens are NOT UI-thread work — 2026-08-15). The drain (check_clutch_kem_decaps) stores the secrets, triggers our encap if still unsent, and fires the completion check; this arm only launches the job. An in-flight decap re-queues the payload — the flag serializes, the next tick re-offers it.
                    if contact.clutch_pending_kem.is_some() && !contact.clutch_kem_decap_in_progress
                    {
                        if let Some(ref local_keys) = contact.clutch_our_keypairs {
                            let pending_kem = contact.clutch_pending_kem.take().expect("checked");
                            contact.clutch_kem_decap_in_progress = true;
                            decap_spawns.push((
                                contact.id.clone(),
                                pending_kem,
                                local_keys.clone(),
                            ));
                            crate::logf!(
                                "CLUTCH: Spawning decap for queued KEM from {}",
                                crate::fp(&contact.handle_proof)
                            );
                        }
                    }

                    // Check if ceremony can complete
                    if contact.all_slots_complete() {
                        crate::logf!("CLUTCH: All slots complete for {} after keygen - triggering ceremony completion", crate::fp(&contact.handle_proof));
                        ceremony_completions.push(idx);
                    }

                    break;
                }
            }

            if !found {
                crate::logf!(
                    "CLUTCH: Keygen result contact_id {}... not found in contacts!",
                    result_id_hex
                );
            }
        }

        // Spawn deferred KEM encapsulation after releasing contacts borrow
        if let Some((contact_id, offer, ceremony_id, conv_token, peer_addr)) = kem_encap_spawn {
            self.spawn_clutch_kem_encap(contact_id, offer, ceremony_id, conv_token, peer_addr);
        }

        // Spawn deferred KEM decapsulations after releasing contacts borrow
        for (contact_id, kem, keypairs) in decap_spawns {
            self.spawn_clutch_kem_decap(contact_id, kem, keypairs);
        }

        // Process deferred ceremony completions (after releasing contacts borrow)
        for idx in ceremony_completions {
            self.complete_clutch_ceremony_by_idx(idx);
            changed = true;
        }

        // A claim is a ROSTER EDGE: without an immediate push the claim sits local, every sibling's TTL gate keeps reading "owner silent", and competing rounds run until some unrelated sync fires — which can be never in a long-lived session. Push now so discard-on-park reaches the fleet while the round is young.
        if claimed_ownership {
            self.spawn_settings_push();
        }
        changed
    }

    /// Process background CLUTCH KEM encapsulation results. When KEM encap completes, store the secrets and send the KEM response.
    pub fn check_clutch_kem_encaps(&mut self) -> bool {
        use crate::network::status::ClutchKemResponseRequest;

        let mut changed = false;
        let mut ceremony_completions: Vec<usize> = Vec::new();
        let our_handle_hash = match self
            .session
            .as_ref()
            .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
        {
            Some(h) => h,
            None => return changed,
        };
        let device_pubkey = *self
            .device_keypair
            .as_ref()
            .expect("device_keypair set in init")
            .public
            .as_bytes();
        let device_secret = *self
            .device_keypair
            .as_ref()
            .expect("device_keypair set in init")
            .secret
            .as_bytes();

        while let Ok(result) = self.clutch_kem_encap_rx.try_recv() {
            let result_id_hex = hex::encode(&result.contact_id.as_bytes()[..4]);
            crate::logf!(
                "CLUTCH: Processing KEM encap result for contact_id {}...",
                result_id_hex
            );

            // Same round scoping as the ceremony drain below: a KEM encapsulated under a discarded round must not send, and must not park its secrets in the slot the successor round is now using.
            if let Some(idx) = self.contacts.iter().position(|c| c.id == result.contact_id) {
                if self.contacts[idx].ceremony_id != Some(result.ceremony_id) {
                    let cur = self.contacts[idx]
                        .ceremony_id
                        .map(|c| hex::encode(&c[..4]))
                        .unwrap_or_else(|| "none".into());
                    crate::logf!("CLUTCH: KEM encap result is for round {}… but the slot is on {} — stale round, dropped", hex::encode(&result.ceremony_id[..4]), cur);
                    self.contacts[idx].clutch_kem_encap_in_progress = false;
                    continue;
                }
            }

            // Find the contact and update state
            let mut found_idx = None;
            for (idx, contact) in self.contacts.iter_mut().enumerate() {
                if contact.id == result.contact_id {
                    found_idx = Some(idx);
                    contact.clutch_kem_encap_in_progress = false;

                    // Party-id seam: sibling ceremonies key our slot on the device-derived pid, not the (shared) identity seed.
                    let our_handle_hash = if contact.is_sibling {
                        crate::crypto::clutch::sibling_party_id(&device_pubkey)
                    } else {
                        our_handle_hash
                    };

                    // Store local encapsulation secrets in local slot (local contribution) Also store the KEM response payload for re-send
                    if let Some(slot) = contact.get_slot_mut(&our_handle_hash) {
                        slot.kem_secrets_to_them = Some(result.local_secrets);
                        slot.kem_response_for_resend = Some(result.kem_response.clone());
                    }

                    // Persist slot state before sending KEM
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

                    // Send the KEM response
                    if let Some(ref checker) = self.status_checker {
                        let (primary, alt) =
                            contact.race_addrs().unwrap_or((result.peer_addr, None));
                        checker.send_kem_response(ClutchKemResponseRequest {
                            peer_addr: primary,
                            alt_addr: alt,
                            conversation_token: result.conversation_token,
                            ceremony_id: result.ceremony_id,
                            payload: result.kem_response,
                            device_pubkey,
                            device_secret,
                            recipient_pubkey: contact.public_identity.key,
                            relay_to: contact.relay_device_list(),
                        });
                        crate::logf!(
                            "CLUTCH: Sent KEM response to {}",
                            crate::fp(&contact.handle_proof)
                        );
                    }

                    // Check if all slots are complete after storing our KEM encap secrets
                    if contact.all_slots_complete() {
                        crate::logf!("CLUTCH: All slots complete for {} after KEM encap - triggering ceremony", crate::fp(&contact.handle_proof));
                        ceremony_completions.push(idx);
                    }

                    changed = true;
                    break;
                }
            }

            if found_idx.is_none() {
                crate::logf!(
                    "CLUTCH: KEM encap result contact_id {}... not found in contacts!",
                    result_id_hex
                );
            }
        }

        // Process deferred ceremony completions (after releasing contacts borrow)
        for idx in ceremony_completions {
            self.complete_clutch_ceremony_by_idx(idx);
            changed = true;
        }

        if changed {}
        changed
    }

    /// Process background CLUTCH ceremony completion results. When ceremony completes, store the friendship chains and send proof.
    pub fn check_clutch_ceremonies(&mut self) -> bool {
        use crate::crypto::clutch::ClutchCompletePayload;
        use crate::network::status::ClutchCompleteRequest;
        use crate::types::ClutchState;

        let mut changed = false;
        let _our_handle_hash = match self
            .session
            .as_ref()
            .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
        {
            Some(h) => h,
            None => return changed,
        };
        let device_pubkey = *self
            .device_keypair
            .as_ref()
            .expect("device_keypair set in init")
            .public
            .as_bytes();
        let device_secret = *self
            .device_keypair
            .as_ref()
            .expect("device_keypair set in init")
            .secret
            .as_bytes();

        while let Ok(result) = self.clutch_ceremony_rx.try_recv() {
            let result_id_hex = hex::encode(&result.contact_id.as_bytes()[..4]);
            crate::logf!(
                "CLUTCH: Processing ceremony result for contact_id {}...",
                result_id_hex
            );

            // ROUND SCOPING for the LOCAL pipeline — the mirror of the wire-side cross-round drop: this completion was spawned under the round it carries, and a discard-and-adopt can land while the job is still in flight. Installing the late result resurrects a round no other device holds, and the proof it just minted gets defended with retransmits for the rest of the session (the 35-minute cross-round wedge, live pair 2026-08-04). Compared against the slot's CURRENT round, not spawn order: the id is deterministic over both provenances, so a re-derived identical round still installs.
            if let Some(idx) = self.contacts.iter().position(|c| c.id == result.contact_id) {
                if self.contacts[idx].ceremony_id != Some(result.ceremony_id) {
                    let cur = self.contacts[idx]
                        .ceremony_id
                        .map(|c| hex::encode(&c[..4]))
                        .unwrap_or_else(|| "none".into());
                    crate::logf!("CLUTCH: ceremony result is for round {}… but the slot is on {} — stale round, dropped", hex::encode(&result.ceremony_id[..4]), cur);
                    self.contacts[idx].clutch_ceremony_in_progress = false;
                    let mut stale = result;
                    use zeroize::Zeroize;
                    stale.fanout_pair_secret.zeroize();
                    stale.friendship_chains.zeroize_history_key();
                    continue;
                }
            }

            let friendship_id = *result.friendship_chains.id();

            // The chains write rides the durable writer (persist_chains_then, below) with the proof send attached — persist-before-proof kept, zero encrypt+IO on the UI thread (2026-08-15). Cloned because the in-memory cache takes the original; the writer coalesces per fid, so a queued older save of this friendship is superseded by these fresh chains, never the reverse.
            let chains_for_writer = result.friendship_chains.clone();
            let mut proof_action: Option<super::ChainsPostDurable> = None;

            // EVERY completed ceremony mints a durable pair secret with the other DEVICE, sibling or friend, and it is stored before anything else can fail. A sibling's is what lets it be wrapped into the egged fan-out; a FRIEND's is the key their scoped-blob slot is addressed and sealed with (docs/scoped-blobs.md), so without it there is no way to share a file with them at all.
            let peer = self
                .contacts
                .iter()
                .find(|c| c.id == result.contact_id)
                .map(|c| (*c.public_identity.as_bytes(), c.is_sibling));
            if let (Some((their_device, is_sibling)), Some(storage)) = (peer, self.storage.as_ref())
            {
                match crate::storage::fanout_pairs::store(
                    &device_pubkey,
                    &their_device,
                    &result.fanout_pair_secret,
                    storage,
                ) {
                    Ok(()) => {
                        crate::logf!(
                            "PAIR: secret minted with {} {}",
                            if is_sibling { "sibling" } else { "friend" },
                            crate::fp(&their_device)
                        );
                        // The Fleet page's remembered egged probe is stale the moment a pair mints.
                        self.egged_cache.remove(&their_device);
                        // A scoped slot's ADDRESS is derived from this secret, so re-minting it moves the reader to an address nothing was ever written to — their avatar silently stops resolving. Re-grant on the mint edge: one ~80 byte slot write against the blob we already published, no re-upload.
                        self.scoped_regrant_pending.push(result.fanout_pair_secret);
                        // A newly egged SIBLING just completed its consent ceremony — GROW the fan-out so its wrap exists (same key, docs/fleet-key.md). Idempotent by the worker's monotonic revision guard; a race loser refetches. A friend's secret changes no fleet state, so no publish.
                        if is_sibling {
                            self.fanout_grow_pending = true;
                        }
                    }
                    Err(e) => crate::logf!("PAIR: secret store failed: {}", e),
                }
            }

            // Cache chains in memory
            if let Some(entry) = self
                .friendship_chains
                .iter_mut()
                .find(|(id, _)| *id == friendship_id)
            {
                // Supersede: scrub the OLD chains' history key + lane root before the re-keyed chains replace them (the fresh chains carry their own newly derived pair).
                entry.1.zeroize_history_key();
                entry.1.zeroize_lane_root();
                entry.1 = result.friendship_chains;
            } else {
                self.friendship_chains
                    .push((friendship_id, result.friendship_chains));
            }

            // Update sync records for new friendship
            self.update_sync_records();

            // Find the contact and update state
            if let Some(contact) = self.contacts.iter_mut().find(|c| c.id == result.contact_id) {
                // Pseudonymous log label — the display name is a REAL name in the field ("Theresa" in a submitted log, 2026-08-08); fp is the doctrine.
                let contact_handle = crate::fp(&contact.handle_proof);
                contact.clutch_ceremony_in_progress = false;
                contact.friendship_id = Some(friendship_id);

                crate::logf!(
                    "CLUTCH: Eggs computed with {}! (proof: {}...)",
                    contact_handle,
                    hex::encode(&result.eggs_proof[..8])
                );

                // Store our proof for later verification
                contact.clutch_our_eggs_proof = Some(result.eggs_proof);
                // Budget a handful of proof retransmits — the proof is a single unreliable UDP packet, so ping_contacts re-sends it until this drains, guaranteeing the peer gets it even on a lossy or freshly-changed path.
                contact.clutch_proof_resends_left = 5;
                contact.clutch_proof_retry_lifetime = 0; // fresh round — reset the give-up lifetime
                contact.clutch_proof_gave_up = false;

                // Check if we already received their proof (fast party case)
                let their_early_proof = contact.clutch_their_eggs_proof;

                // The ClutchComplete proof rides the chains writer as a gated signal: it leaves the machine only after the chains that back it are durable (the same persist-before-signal the save-then-send inline order used to give). The retransmit ladder (clutch_proof_resends_left, ping cadence) arms immediately — its earliest re-fire trails the writer's ms-scale write by seconds, and losing that race costs one recoverable re-offer.
                if let Some(ref checker) = self.status_checker {
                    let payload = ClutchCompletePayload {
                        eggs_proof: result.eggs_proof,
                    };

                    let (primary, alt) = contact.race_addrs().unwrap_or((result.peer_addr, None));
                    proof_action = Some(super::ChainsPostDurable::CeremonyProof(
                        checker.complete_proof_sender(),
                        ClutchCompleteRequest {
                            peer_addr: primary,
                            alt_addr: alt,
                            conversation_token: result.conversation_token,
                            ceremony_id: result.ceremony_id,
                            payload,
                            device_pubkey,
                            device_secret,
                            recipient_pubkey: contact.public_identity.key,
                            relay_to: contact.relay_device_list(),
                        },
                    ));

                    crate::logf!(
                        "CLUTCH: proof queued behind the durable chains write for {}",
                        contact_handle
                    );
                }

                // Check if they already sent us their proof — but only a proof from OUR round counts. An early proof stored under a different ceremony_id is echo of a superseded attempt (offer churn / an unwiped peer replaying old state): discard it and await their current-round proof instead of manufacturing a mismatch (a permanent-Pending stall).
                let round_ok = contact.clutch_their_proof_ceremony == Some(result.ceremony_id);
                let their_early_proof = match (their_early_proof, round_ok) {
                    (Some(p), true) => Some(p),
                    (Some(_), false) => {
                        let stored_round = contact
                            .clutch_their_proof_ceremony
                            .map(|c| hex::encode(&c[..4]))
                            .unwrap_or_else(|| "none".to_string());
                        crate::logf!("CLUTCH: stored early proof from {} is cross-round (theirs {}… vs ours {}…) — discarded, awaiting their current-round proof", contact_handle, stored_round, hex::encode(&result.ceremony_id[..4]));
                        contact.clutch_their_eggs_proof = None;
                        contact.clutch_their_proof_ceremony = None;
                        None
                    }
                    (None, _) => None,
                };
                if let Some(their_proof) = their_early_proof {
                    if their_proof == result.eggs_proof {
                        // SUCCESS! Both parties computed same eggs
                        crate::logf!(
                            "CLUTCH: Early proof verified with {}! ✓ proof={}...",
                            contact_handle,
                            hex::encode(&result.eggs_proof[..8])
                        );
                        contact.clutch_state = ClutchState::Complete;
                        contact.clutch_completed_at = Some(std::time::Instant::now()); // arm the post-completion re-key cooldown (before the ~1s-later weave)
                                                                                       // A FRESH ceremony just completed = a brand-new chain — any prior weave seal is void. Reset the double-toggle state so the hidden probe REFIRES for this chain. Without this, a peer that client-reset and re-CLUTCHed hits a deadlock: our persisted chain_woven=true (load latches all probe flags true) suppresses our probe, the reset peer waits forever for it ("weaving the chain"), and we dismiss their re-sent proofs as woven-duplicates. First-ceremony case: flags already false, no-op.
                        contact.chain_woven = false;
                        contact.probe_sent = false;
                        contact.void_weave_seal_from_previous_chain();
                        contact.chain_advanced_by_ack = false;
                        // Store their HQC pub prefix to detect stale offers after restart
                        contact.completed_their_hqc_prefix = Some(result.their_hqc_prefix);
                        // We're Complete, but the peer may not have our proof yet — we got theirs first, and our single send (just above) might have dropped. Keep the proof and the resend budget so ping_contacts keeps delivering it for a few more cycles; that's exactly what stops the peer from hanging in AwaitingProof.
                        contact.clutch_their_eggs_proof = None;
                    } else {
                        // SAME round, different eggs. We NO LONGER torch (re-key) here — the clutch does not rotate. Torching re-minted keys → a new time-based provenance → a new ceremony_id the peer had to chase, and over the relay both sides torched on the same transient faster than the round-trip, so they stayed one generation apart forever (the "clutch toggling"). With the clutch pinned (stable keys + pinned send-time), the ONLY reason to reach here is a TRANSIENT: we computed eggs before the full KEM exchange landed, or a proof crossed in flight. Keep every ceremony input intact; the resend machinery redelivers the peer's KEM/proof and the next completion recomputes matching eggs from the stable slots. If a genuine deterministic mismatch survives a full stable exchange, that's a real bug to chase in the log — not a re-key trigger.
                        let our_hex = hex::encode(&result.eggs_proof);
                        let their_hex = hex::encode(&their_proof);
                        crate::logf!("CLUTCH: ⚠ PROOF MISMATCH with {} (same round {}…) ours={}... theirs={}... — NOT re-keying (clutch pinned); awaiting their correct proof", contact_handle, hex::encode(&result.ceremony_id[..4]), &our_hex[..16], &their_hex[..16]);
                        // Discard THEIR mismatched early proof (a transient — crossed in flight, or computed before the full stable KEM exchange). Keep OUR proof + all keys/slots/provenances/ceremony_id pinned, go AwaitingProof: we keep re-sending our stable proof and wait for theirs to land correct. Deterministic stable inputs → their proof matches ours once the exchange completes.
                        contact.clutch_their_eggs_proof = None;
                        contact.clutch_their_proof_ceremony = None;
                        contact.clutch_state = ClutchState::AwaitingProof;
                    }
                } else {
                    // Set state to AwaitingProof - wait for their proof
                    contact.clutch_state = ClutchState::AwaitingProof;
                    crate::logf!(
                        "CLUTCH: Awaiting proof from {} (we sent ours)",
                        contact_handle
                    );
                }

                // Save contact to persist friendship_id and clutch_state
                if let Some(storage) = self.storage.as_ref() {
                    if let Err(e) = crate::storage::contacts::save_contact(contact, storage) {
                        crate::logf!("Failed to save contact after CLUTCH: {}", e);
                    } else {
                        #[cfg(feature = "development")]
                        #[cfg(feature = "development")]
                        crate::logf!("CLUTCH: Saved {} state to disk", contact_handle);
                    }

                    // Delete slots file - ceremony is complete, slots no longer needed
                    if let Err(e) =
                        crate::storage::contacts::delete_clutch_slots(&contact.handle_hash, storage)
                    {
                        crate::logf!("Failed to delete CLUTCH slots: {}", e);
                    }
                }
                changed = true;
            } else {
                crate::logf!(
                    "CLUTCH: Ceremony result contact_id {}... not found in contacts!",
                    result_id_hex
                );
            }

            // Durable chains write + the gated proof send, now that the contact borrow is released.
            self.persist_chains_then(chains_for_writer, proof_action.into_iter().collect());

            // If the early-proof branch just took this contact to Complete, fire the hidden chain-weave probe (once). Done after the mutable-borrow block above releases.
            if let Some(idx) = self.contacts.iter().position(|c| c.id == result.contact_id) {
                self.maybe_send_chain_probe(idx);
            }
        }

        if changed {}
        changed
    }

    /// Spawn background CLUTCH ceremony completion when all slots are filled. Extracts data from contact and spawns background thread for heavy crypto.
    ///
    /// Takes contact index to avoid borrow conflicts in the event loop. Derives OUR party id internally (identity seed for friends, device-derived pid for fleet siblings) — callers used to pass a hoisted seed, which was wrong for sibling ceremonies.
    pub(super) fn complete_clutch_ceremony_by_idx(&mut self, contact_idx: usize) {
        use crate::crypto::clutch::{derive_conversation_token, ClutchSharedSecrets};

        let our_handle_hash = match self
            .contacts
            .get(contact_idx)
            .and_then(|c| self.our_party_id(c))
        {
            Some(pid) => pid,
            None => {
                crate::log("CLUTCH: No party id available for ceremony completion");
                return;
            }
        };

        // Extract data from contact to avoid borrow issues
        let contact = match self.contacts.get_mut(contact_idx) {
            Some(c) => c,
            None => {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::log("CLUTCH: Invalid contact index");
                return;
            }
        };

        // Check if ceremony already in progress
        if contact.clutch_ceremony_in_progress {
            crate::logf!(
                "CLUTCH: Ceremony already in progress for {}",
                crate::fp(&contact.handle_proof)
            );
            return;
        }

        // Get their slot (the other party)
        let their_handle_hash = contact.handle_hash;
        let contact_is_sibling = contact.is_sibling;
        let contact_hp = contact.handle_proof;
        let contact_id = contact.id.clone();
        // Pseudonymous log label (see the ceremony-completion twin above).
        let contact_handle = crate::fp(&contact.handle_proof);
        // The eggs bind a device-pubkey pair: use the device that SIGNED their offer (the ceremony's actual participant), never the pinned public_identity — pongs re-elect the pin, so a multi-device friend answering from an unpinned device desynced one egg and the proofs mismatched on a perfect round (live pair 2026-07-24). Legacy-persisted slots lack the signer → fall back to the pin, which is exact for single-device friends.
        let their_device_pub = contact
            .get_slot(&contact.handle_hash)
            .and_then(|s| s.offer_device)
            .unwrap_or(*contact.public_identity.as_bytes());

        // Extract all needed data from slots (cloning to release borrow)
        let our_slot = match contact.get_slot(&our_handle_hash) {
            Some(s) => s,
            None => {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::log("CLUTCH: No slot for local party");
                return;
            }
        };
        let their_slot = match contact.get_slot(&their_handle_hash) {
            Some(s) => s,
            None => {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::log("CLUTCH: No slot for remote party");
                return;
            }
        };

        // Local encapsulation secrets from local slot
        let our_kem_secrets = match &our_slot.kem_secrets_to_them {
            Some(s) => s.clone(),
            None => {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::log("CLUTCH: No kem_secrets_to_them in local slot");
                return;
            }
        };
        // Remote encapsulation secrets from remote slot
        let their_kem_secrets = match &their_slot.kem_secrets_from_them {
            Some(s) => s.clone(),
            None => {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::log("CLUTCH: No kem_secrets_from_them in remote slot");
                return;
            }
        };

        // Get their HQC prefix for stale detection
        let their_hqc_prefix: [u8; 8] = their_slot
            .offer
            .as_ref()
            .map(|o| o.hqc256_public[..8].try_into().unwrap_or_default())
            .unwrap_or_default();

        // Get peer address and ceremony_id
        let peer_addr = match contact.ip {
            Some(ip) => ip,
            None => {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::logf!("CLUTCH: No IP for {}", crate::fp(&contact.handle_proof));
                return;
            }
        };
        let ceremony_id = match contact.ceremony_id {
            Some(c) => c,
            None => {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::logf!(
                    "CLUTCH: No ceremony_id for {}",
                    crate::fp(&contact.handle_proof)
                );
                return;
            }
        };

        let conversation_token = derive_conversation_token(&[our_handle_hash, their_handle_hash]);

        crate::logf!(
            "CLUTCH: Spawning ceremony completion for {}",
            contact_handle
        );

        // Determine low/high ordering by handle hash
        let we_are_low = our_handle_hash < their_handle_hash;

        // Pick the low/high sides ONCE, then list every algorithm once. The old form wrote the whole field list twice, mirrored — 24 lines that had to stay exact transpositions of each other, where a single mis-swap would produce a valid-but-different pad on one side only and fail as an unexplainable proof mismatch.
        let (lo, hi) = if we_are_low {
            (&our_kem_secrets, &their_kem_secrets)
        } else {
            (&their_kem_secrets, &our_kem_secrets)
        };
        let secrets = ClutchSharedSecrets {
            low_x25519: lo.x25519,
            high_x25519: hi.x25519,
            low_p384: lo.p384.clone(),
            high_p384: hi.p384.clone(),
            low_secp256k1: lo.secp256k1.clone(),
            high_secp256k1: hi.secp256k1.clone(),
            low_p256: lo.p256.clone(),
            high_p256: hi.p256.clone(),
            low_p521: lo.p521.clone(),
            high_p521: hi.p521.clone(),
            low_frodo: lo.frodo.clone(),
            high_frodo: hi.frodo.clone(),
            low_frodo1344: lo.frodo1344.clone(),
            high_frodo1344: hi.frodo1344.clone(),
            low_ntru: lo.ntru.clone(),
            high_ntru: hi.ntru.clone(),
            low_sntrup: lo.sntrup.clone(),
            high_sntrup: hi.sntrup.clone(),
            low_mlkem: lo.mlkem.clone(),
            high_mlkem: hi.mlkem.clone(),
            low_mceliece: lo.mceliece.clone(),
            high_mceliece: hi.mceliece.clone(),
            low_hqc: lo.hqc.clone(),
            high_hqc: hi.hqc.clone(),
        };

        // Mark ceremony in progress and spawn background thread
        contact.clutch_ceremony_in_progress = true;

        let our_device_pub = *self
            .device_keypair
            .as_ref()
            .expect("device_keypair set in init")
            .public
            .as_bytes();
        // The SECRET identity binding for the eggs (docs/identity-profile.md): friends = static identity DH against their pinned identity pubkey (the party id); siblings share the identity seed itself (their party ids aren't curve points). A pin that isn't a valid point is an old-format contact row — flag-day: fail loudly, re-add the friend.
        let Some(our_seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            crate::log("CLUTCH: no session — cannot derive friendship secret");
            return;
        };
        let friendship_secret = if contact_is_sibling {
            our_seed
        } else {
            match crate::crypto::clutch::identity_friendship_secret(&our_seed, &their_handle_hash) {
                Some(fs) => fs,
                None => {
                    crate::logf!("CLUTCH: pinned identity for {} is not a curve point (old-format contact row) — re-add this friend", crate::fp(&contact_hp));
                    return;
                }
            }
        };
        self.spawn_clutch_ceremony(
            contact_id,
            our_handle_hash,
            their_handle_hash,
            our_device_pub,
            their_device_pub,
            friendship_secret,
            secrets,
            ceremony_id,
            conversation_token,
            peer_addr,
            their_hqc_prefix,
        );
    }
}
