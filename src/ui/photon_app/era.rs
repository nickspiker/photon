//! The era ratchet's decision function and its in-band mechanics (docs/lanes.md "Eras", plan logical-brewing-creek §4-§5): every trigger that used to start its own destructive re-key routes here, and the verdict set has NO destructive member by construction.
//! Stage 2 wired the two observations that need no new crypto — a peer's advertised era differing from ours, and the fleet answering an era_pull with a miss. Stage 3 adds the LIGHT RATCHET (crypto/era.rs): the computed ceremony owner, the Init/Resp/Nudge rows, the two cutover edges, and the 256-row cadence. Verdicts that need the woven CLUTCH or consent (stages 4-5) still log and nothing else.

use super::PhotonApp;
use crate::crypto::era::{era_decapsulate, era_encapsulate, era_keygen, derive_era_keys, derive_era_transcript, EraKemWire, EraSignal, KEM_SET_DEFAULT};
use crate::types::friendship::PendingEra;

/// What was observed. Each variant names its evidence; none of them is "a timer fired".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RepairTrigger {
    /// A peer's pong advertised an era for this friendship that is not ours.
    StaleEraObserved { peer_index: u64, peer_tag: u32 },
    /// Every live sibling answered an era_pull with a miss: no newer era exists in the fleet.
    FleetAllMissed,
    /// The peer's rows since the last ratchet crossed a multiple of the cadence (crypto/era.rs LIGHT_RATCHET_CADENCE_ROWS).
    CadenceReached,
    /// The peer (the identity that may not initiate) asked us to ratchet from our current era.
    NudgeReceived,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PeerEra {
    NoRecord,
    Same,
    Ahead,
    Behind,
    /// Same index with a different tag, or otherwise unorderable against ours: a channel we cannot place.
    Foreign,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SiblingVerdict {
    NoSiblings,
    Unasked,
    Asked,
    AllMissed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RepairInput {
    pub trigger: RepairTrigger,
    pub peer: PeerEra,
    pub siblings: SiblingVerdict,
    pub we_own: bool,
    pub peer_era_capable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RepairVerdict {
    Hold(&'static str),
    /// Ask the fleet (era_pull) whether a sibling holds a newer era.
    PullFleet,
    /// Propose an in-band light ratchet (owner only, era-capable peer only).
    LightRatchet,
    /// Stage 4: a full CLUTCH with the current roots woven in (owner only).
    HeavyWeave,
    /// Stage 5: a from-scratch channel, consent-gated.
    ConsentFresh,
}

/// Pure and table-tested. Reads only what it is given; never touches state.
pub(crate) fn friendship_repair(i: &RepairInput) -> RepairVerdict {
    use RepairVerdict::*;
    let mint = |capable: bool| if capable { LightRatchet } else { HeavyWeave };
    match i.trigger {
        RepairTrigger::StaleEraObserved { .. } => match i.peer {
            PeerEra::NoRecord | PeerEra::Same => Hold("peer era matches"),
            PeerEra::Behind => Hold("the peer is behind — its own repair moves it"),
            PeerEra::Ahead => match i.siblings {
                SiblingVerdict::Unasked => PullFleet,
                SiblingVerdict::Asked => Hold("era_pull in flight"),
                SiblingVerdict::NoSiblings | SiblingVerdict::AllMissed => {
                    if i.we_own { mint(i.peer_era_capable) } else { Hold("not the era owner") }
                }
            },
            // A channel we cannot order against ours: ask the fleet FIRST — a sibling may hold the era the peer is on (two ceremonies out of one fleet, the 2026-09-09 phone/desktop split) — and only then is it a stranger's channel for consent.
            PeerEra::Foreign => match i.siblings {
                SiblingVerdict::Unasked => PullFleet,
                SiblingVerdict::Asked => Hold("era_pull in flight"),
                SiblingVerdict::NoSiblings | SiblingVerdict::AllMissed => {
                    if i.we_own { ConsentFresh } else { Hold("not the era owner") }
                }
            },
        },
        RepairTrigger::FleetAllMissed => {
            if i.we_own { mint(i.peer_era_capable) } else { Hold("not the era owner") }
        }
        // The standing cadence and a nudge both ask for the LIGHT ratchet and nothing heavier: a peer that cannot ratchet in band simply keeps its era (the cadence is hygiene, not a repair).
        RepairTrigger::CadenceReached | RepairTrigger::NudgeReceived => {
            if !i.we_own {
                Hold("not the era owner")
            } else if !i.peer_era_capable {
                Hold("peer is not era-capable — the cadence waits for its first tagged frame")
            } else {
                LightRatchet
            }
        }
    }
}

/// Classify the peer's advertised era against ours.
pub(crate) fn classify_peer_era(ours_index: u64, ours_tag: Option<u32>, peer_index: u64, peer_tag: u32, retired_tag: Option<u32>, pending_tag: Option<u32>) -> PeerEra {
    if peer_tag == 0 {
        return PeerEra::NoRecord;
    }
    if Some(peer_tag) == ours_tag {
        return PeerEra::Same;
    }
    if Some(peer_tag) == retired_tag || peer_index < ours_index {
        return PeerEra::Behind;
    }
    if Some(peer_tag) == pending_tag || peer_index > ours_index {
        return PeerEra::Ahead;
    }
    PeerEra::Foreign
}

/// The fleet's ceremony owner, COMPUTED (plan §4, replacing claim-on-pickup): the lowest device pubkey among fold members that are not locked and not probed-offline. "Unprobed" counts as present (the boot-race rule: a freshly booted device must not take over from a live owner it has not heard from yet). Every device computes the same answer from the same evidence; divergent presence views are arbitrated at the peer (first Init wins) and the loser converges by chain-sync.
pub(crate) fn era_owner(ours: [u8; 32], fold: &[[u8; 32]], locked: &[[u8; 32]], siblings: &[super::SiblingPresence], locked_out: &[[u8; 32]]) -> [u8; 32] {
    let mut candidates: Vec<[u8; 32]> = if fold.is_empty() {
        let mut v = vec![ours];
        v.extend(siblings.iter().map(|(k, _, _)| *k));
        v
    } else {
        fold.to_vec()
    };
    candidates.retain(|d| {
        if locked.contains(d) || locked_out.contains(d) {
            return false;
        }
        if *d == ours {
            return true;
        }
        match siblings.iter().find(|(k, _, _)| k == d) {
            Some((_, online, probed)) => !(*probed && !*online),
            None => true,
        }
    });
    candidates.into_iter().min().unwrap_or(ours)
}

impl PhotonApp {
    /// Recompute the ceremony owner from fold + presence and re-point every friendship at it. Called on the evidence EDGES (a sibling's presence verdict, a fold adopt, a locked-set change, a roster adopt) — never on a timer. A round this device holds for a friendship that just moved to another owner is discarded (fleet-sync.md §4.2 discard-on-park), so the friend never sees two instances from one fleet.
    pub(super) fn recompute_ceremony_owners(&mut self, why: &str) {
        let Some(ours) = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes()) else { return };
        let locked = self.locked_devices();
        let siblings = super::sibling_presence_snapshot(&self.contacts);
        let locked_out: Vec<[u8; 32]> = self.contacts.iter().filter(|c| c.is_sibling && c.locked_out).filter_map(|c| c.device_key()).collect();
        let owner = era_owner(ours, &self.registry_converged_fold, &locked, &siblings, &locked_out);
        let mut moved = 0usize;
        let mut discarded = 0usize;
        for c in self.contacts.iter_mut().filter(|c| !c.is_sibling) {
            if c.ceremony_owner == Some(owner) {
                continue;
            }
            let holds_round = c.clutch_our_keypairs.is_some() || c.clutch_offer_sent || !c.clutch_slots.is_empty() || c.ceremony_id.is_some();
            if owner != ours && c.clutch_state != crate::types::ClutchState::Complete && holds_round {
                crate::logf!("CLUTCH §4.2: discarding our round for {} — the fleet's ceremony owner is {}", crate::fp(&c.handle_proof).as_str(), hex::encode(&owner[..4]));
                c.discard_clutch_round();
                discarded += 1;
            }
            // DERIVED STATE, never written here: the owner is recomputed on every edge and at the keygen pickup after a load, so persisting it bought nothing — and 13 synchronous vault writes on a presence verdict were two 1.8 s UI hangs in the middle of a call (Nick's phone 2026-09-09 01:55, 46 windows lost).
            c.ceremony_owner = Some(owner);
            moved += 1;
        }
        if moved > 0 {
            crate::logf!(
                "ERA: ceremony owner → {}{} ({}) — {} friendship(s) re-pointed, {} parked round(s) discarded",
                hex::encode(&owner[..4]),
                if owner == ours { " (this device)" } else { "" },
                why,
                moved,
                discarded
            );
        }
    }

    /// Build the input, decide, log one line, and act on the verdicts built so far. The rest log their intent and nothing else — never a destructive fallback.
    pub(super) fn repair_dispatch(&mut self, ci: usize, trigger: RepairTrigger) {
        let Some(contact) = self.contacts.get(ci) else { return };
        let Some(fid) = contact.friendship_id else { return };
        let Some((_, chains)) = self.friendship_chains.iter().find(|(id, _)| *id == fid) else { return };
        let peer = match trigger {
            RepairTrigger::StaleEraObserved { peer_index, peer_tag } => classify_peer_era(
                chains.era_index,
                chains.era_tag(),
                peer_index,
                peer_tag,
                chains.retired_era().map(|r| r.tag),
                chains.pending_era().map(|p| p.tag),
            ),
            RepairTrigger::FleetAllMissed => PeerEra::Ahead,
            RepairTrigger::CadenceReached | RepairTrigger::NudgeReceived => PeerEra::Same,
        };
        let token = chains.conversation_token;
        let held = chains.era_index;
        let has_siblings = self.contacts.iter().any(|c| c.is_sibling && !c.locked_out);
        let siblings = if !has_siblings {
            SiblingVerdict::NoSiblings
        } else if matches!(trigger, RepairTrigger::FleetAllMissed) {
            SiblingVerdict::AllMissed
        } else if self.era_pull_sent.contains_key(&token) {
            SiblingVerdict::Asked
        } else {
            SiblingVerdict::Unasked
        };
        // Ownership is the computed owner (recompute_ceremony_owners); a contact not yet re-pointed reads as unclaimed = ours, matching the park predicate.
        let ours = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes());
        let we_own = contact.ceremony_owner.map_or(true, |o| Some(o) == ours);
        let capable = contact.peer_era_capable;
        let fp = crate::fp(&contact.handle_proof);
        let verdict = friendship_repair(&RepairInput { trigger, peer, siblings, we_own, peer_era_capable: capable });
        let line = format!("{trigger:?} peer={peer:?} siblings={siblings:?} own={we_own} capable={capable} → {verdict:?}");
        crate::logf!("ERA: {} {}", fp, line);
        match verdict {
            RepairVerdict::PullFleet => {
                if let Some(kp) = self.device_keypair.as_ref() {
                    if let Ok(frame) = crate::network::fgtw::protocol::build_chain_pull_vsf(&token, kp.public.as_bytes(), kp.secret.as_bytes(), Some(held)) {
                        self.era_pull_sent.insert(token, held);
                        self.dispatch_frame_to_siblings(frame);
                        crate::logf!("ERA: era_pull for {} — asking the fleet for an era newer than #{}", fp, held);
                    }
                }
            }
            RepairVerdict::Hold(why) => crate::logf!("ERA: {} holding — {}", fp, why),
            RepairVerdict::LightRatchet => {
                self.propose_light_ratchet(ci);
            }
            RepairVerdict::HeavyWeave => {
                self.arm_heavy_weave_for(ci, "repair verdict");
            }
            RepairVerdict::ConsentFresh => crate::logf!("ERA: {} is on a channel we cannot order against ours — the consent-gated fresh channel is not built yet (stage 5); nothing done", fp),
        }
    }

    /// PROPOSE (plan §2 light flow, step 1). The initiating IDENTITY is the lower party id (participants[0], the is_clutch_initiator rule); its fleet's owner device mints ephemerals and sends the Init on the CURRENT era, then keeps chatting on it. The other identity's owner sends a Nudge instead, asking the initiator to start. Returns whether a row left.
    pub(super) fn propose_light_ratchet(&mut self, ci: usize) -> bool {
        let Some(ours) = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes()) else { return false };
        let Some(contact) = self.contacts.get(ci) else { return false };
        let fp = crate::fp(&contact.handle_proof);
        if contact.is_sibling {
            return false;
        }
        if contact.ceremony_owner.is_some_and(|o| o != ours) {
            crate::logf!("ERA: {} — not the ceremony owner ({}); not proposing", fp, hex::encode(&contact.ceremony_owner.unwrap_or_default()[..4]));
            return false;
        }
        if !contact.peer_era_capable {
            crate::logf!("ERA: {} is not era-capable yet — no light ratchet toward it", fp);
            return false;
        }
        if let Some(e) = contact.era_ephemeral.as_ref() {
            crate::logf!("ERA: {} ratchet already in flight ({}) — waiting for the Resp", fp, format!("{e:?}"));
            return false;
        }
        let Some(fid) = contact.friendship_id else { return false };
        let Some(our_pid) = self.our_party_id(contact) else { return false };
        let Some((_, chains)) = self.friendship_chains.iter().find(|(id, _)| *id == fid) else { return false };
        let Some(tag) = chains.era_tag() else { return false };
        if let Some(p) = chains.pending_era() {
            crate::logf!("ERA: {} already holds era#{} ({:08x}) pending — the cutover edge finishes it", fp, p.era_index, p.tag);
            return false;
        }
        let era_next = chains.era_index + 1;
        let initiator = chains.participants().first() == Some(&our_pid);
        let ts = vsf::eagle_time_oscillations();
        if !initiator {
            let content = EraSignal::Nudge { prior_tag: tag }.to_content();
            let sent = self.chain_transmit_with(ci, &content, ts, None, None, None);
            crate::logf!("ERA: {} is the initiating identity — Nudge {} from era#{} ({:08x})", fp, if sent { "sent" } else { "NOT sent" }, era_next - 1, tag);
            return sent;
        }
        // Keygen inline: ML-KEM-1024 + X25519 + HQC-256 keypairs cost a few milliseconds — sub-frame, once per 256 rows. (McEliece was the slow one; it is not in this set.)
        let eph = era_keygen(era_next, tag, KEM_SET_DEFAULT);
        let wire = eph.init_wire.clone();
        let content = EraSignal::Init { era_next, nonce: eph.nonce, prior_tag: tag, kem_set: eph.kem_set }.to_content();
        let bytes = wire.mlkem.len() + wire.x25519.len() + wire.hqc.len();
        let nonce_s = hex::encode(&eph.nonce[..4]);
        self.contacts[ci].era_ephemeral = Some(eph);
        let sent = self.chain_transmit_with(ci, &content, ts, None, None, Some(&wire));
        if sent {
            crate::logf!("ERA: {} era#{} → era#{} Init sent on {:08x} (nonce {}…, {} B of public keys) — chatting on the current era until the Resp lands", fp, era_next - 1, era_next, tag, nonce_s, bytes);
        } else {
            self.contacts[ci].era_ephemeral = None;
            crate::logf!("ERA: {} Init could not be sent — ephemerals dropped; the next edge retries", fp);
        }
        sent
    }

    /// An era row landed (conversation.rs control ladder, post-decrypt, post-commit). `kem` is the package's typed KEM material; `ts` the row's eagle time.
    pub(super) fn on_era_signal(&mut self, ci: usize, sig: EraSignal, kem: Option<EraKemWire>, ts: i64) {
        let Some(contact) = self.contacts.get(ci) else { return };
        let fp = crate::fp(&contact.handle_proof);
        let Some(fid) = contact.friendship_id else { return };
        let Some(pos) = self.friendship_chains.iter().position(|(id, _)| *id == fid) else { return };
        match sig {
            EraSignal::Init { era_next, nonce, prior_tag, kem_set } => {
                let Some(kem) = kem else {
                    crate::logf!("ERA: {} Init without KEM material — dropped", fp);
                    return;
                };
                // Duplicate Init (retransmit, or our Resp was lost and they re-asked): the SAME ciphertexts go back — a fresh encapsulation would derive a second era for one nonce.
                if let Some((n, cached)) = self.contacts[ci].era_resp_cache.clone() {
                    if n == nonce {
                        let content = EraSignal::Resp { era_next, nonce, prior_tag }.to_content();
                        let resp_ts = self.friendship_chains[pos].1.pending_era().and_then(|p| p.resp_osc).unwrap_or_else(vsf::eagle_time_oscillations);
                        let sent = self.chain_transmit_with(ci, &content, resp_ts, None, None, Some(&cached));
                        crate::logf!("ERA: {} duplicate Init (nonce {}…) — cached Resp {}", fp, hex::encode(&nonce[..4]), if sent { "re-sent" } else { "not re-sent (in flight or window full)" });
                        return;
                    }
                }
                let resp_ts = vsf::eagle_time_oscillations();
                let (resp, new_tag, replaced) = {
                    let chains = &mut self.friendship_chains[pos].1;
                    let Some(our_tag) = chains.era_tag() else { return };
                    if prior_tag != our_tag {
                        let behind = chains.retired_era().is_some_and(|r| r.tag == prior_tag);
                        crate::logf!("ERA: {} Init ratchets from {:08x} but we are on {:08x} — {}; dropped", fp, prior_tag, our_tag, if behind { "the peer is on our RETIRED era (its pong will show it)" } else { "unknown prior" });
                        return;
                    }
                    if era_next != chains.era_index + 1 {
                        crate::logf!("ERA: {} Init names era#{} but we hold era#{} — dropped", fp, era_next, chains.era_index);
                        return;
                    }
                    let replaced = chains.pending_era().map(|p| p.tag);
                    let Some((resp, fresh)) = era_encapsulate(&kem, kem_set) else {
                        crate::logf!("ERA: {} Init KEM material malformed (set {}) — dropped", fp, kem_set);
                        return;
                    };
                    let transcript = derive_era_transcript(&chains.conversation_token, era_next, &nonce, kem_set, &kem, &resp);
                    let Some(old_root) = chains.lane_root().copied() else { return };
                    let old_hk = chains.history_key().copied();
                    let (root, hk) = derive_era_keys(fid.as_bytes(), era_next, &old_root, old_hk.as_ref(), &fresh, &transcript);
                    let new_tag = crate::crypto::clutch::era_tag(&root);
                    // A pending era from an earlier Init can never complete once the initiator minted new ephemerals (its old ones are gone) — the newer Init replaces it.
                    if replaced.is_some() {
                        chains.clear_pending();
                    }
                    chains.install_pending(PendingEra { era_index: era_next, lane_root: root, history_key: Some(hk), tag: new_tag, resp_osc: Some(resp_ts) });
                    (resp, new_tag, replaced)
                };
                self.persist_chains_async(&fid);
                self.contacts[ci].era_resp_cache = Some((nonce, resp.clone()));
                let content = EraSignal::Resp { era_next, nonce, prior_tag }.to_content();
                let sent = self.chain_transmit_with(ci, &content, resp_ts, None, None, Some(&resp));
                crate::logf!(
                    "ERA: {} era#{} → era#{} Resp {} — {:08x} installed PENDING{}; cut over on its ACK or the peer's first tagged frame",
                    fp,
                    era_next - 1,
                    era_next,
                    if sent { "sent" } else { "NOT sent (window full — the held-row sweep retries)" },
                    new_tag,
                    replaced.map(|t| format!(" (replacing stale pending {t:08x})")).unwrap_or_default()
                );
            }
            EraSignal::Resp { era_next, nonce, prior_tag } => {
                let Some(kem) = kem else {
                    crate::logf!("ERA: {} Resp without KEM material — dropped", fp);
                    return;
                };
                let Some(eph) = self.contacts[ci].era_ephemeral.take() else {
                    crate::logf!("ERA: {} Resp with no ratchet in flight (a restart aborted it) — dropped; the next edge proposes afresh", fp);
                    return;
                };
                if eph.nonce != nonce || eph.era_next != era_next || eph.prior_tag != prior_tag {
                    crate::logf!("ERA: {} Resp does not match the ratchet in flight ({} vs era#{} nonce {}… prior {:08x}) — dropped", fp, format!("{eph:?}"), era_next, hex::encode(&nonce[..4]), prior_tag);
                    self.contacts[ci].era_ephemeral = Some(eph);
                    return;
                }
                let eph = &eph;
                let cut = {
                    let chains = &mut self.friendship_chains[pos].1;
                    if chains.era_tag() != Some(prior_tag) {
                        crate::logf!("ERA: {} our era moved under the ratchet (now {:08x}) — Resp dropped, ephemerals discarded", fp, chains.era_tag().unwrap_or(0));
                        return;
                    }
                    let Some(fresh) = era_decapsulate(eph, &kem) else {
                        crate::logf!("ERA: {} Resp KEM material malformed — dropped", fp);
                        return;
                    };
                    let transcript = derive_era_transcript(&chains.conversation_token, era_next, &nonce, eph.kem_set, &eph.init_wire, &kem);
                    let Some(old_root) = chains.lane_root().copied() else { return };
                    let old_hk = chains.history_key().copied();
                    let (root, hk) = derive_era_keys(fid.as_bytes(), era_next, &old_root, old_hk.as_ref(), &fresh, &transcript);
                    let tag = crate::crypto::clutch::era_tag(&root);
                    chains.clear_pending();
                    chains.install_pending(PendingEra { era_index: era_next, lane_root: root, history_key: Some(hk), tag, resp_osc: None });
                    // The Resp's existence proves the responder holds era_next: cut over NOW. Its ACK rides after our durable write, so it provably post-dates our cutover.
                    chains.cut_over_to_pending()
                };
                match cut {
                    Some((old_tag, new_tag, retired)) => {
                        crate::logf!("ERA: {} cut over {:08x} → {:08x} (era#{}) on the peer's Resp — {} pending(s) re-serve on the fresh lane; old era retired for the straggler window", fp, old_tag, new_tag, era_next, retired);
                        self.era_cutover_flush(ci, &fid);
                    }
                    None => crate::logf!("ERA: {} Resp accepted but nothing to cut over — no current root?", fp),
                }
                let _ = ts;
            }
            EraSignal::Nudge { prior_tag } => {
                let ours = self.friendship_chains[pos].1.era_tag();
                if ours == Some(prior_tag) {
                    self.repair_dispatch(ci, RepairTrigger::NudgeReceived);
                } else {
                    crate::logf!("ERA: {} nudged from {:08x} but we are on {} — ignored (its pong or our chain-sync converges it)", fp, prior_tag, ours.map(|t| format!("{t:08x}")).unwrap_or_else(|| "none".into()));
                }
            }
        }
    }

    /// SHRINK EDGE (stage 4, decision 1): every friendship with a live era re-keys with the old roots woven in. Armed for the epoch AFTER the rotation the shrink just started (`spawn_fleet_key_sync`), so the keygen pickup waits until the cached fleet key is one the leaver never held — chain_sync opens k−1, so a woven era pushed under the old epoch would still reach it. Persisted per contact: a restart cannot lose a shrink's re-key.
    pub(super) fn arm_heavy_weaves(&mut self, why: &str) {
        let due = self.fleet_epoch.map(|(e, _)| e).unwrap_or(0) + 1;
        let mut armed = 0usize;
        let mut dirty: Vec<crate::types::Contact> = Vec::new();
        for c in self.contacts.iter_mut().filter(|c| !c.is_sibling && c.consent_mutual && c.friendship_id.is_some() && !c.locked_out) {
            if c.era_weave_due >= due {
                continue;
            }
            c.era_weave_due = due;
            armed += 1;
            dirty.push(c.clone());
        }
        self.save_contacts_off_thread(dirty);
        crate::logf!("ERA: heavy weave armed for {} friendship(s) at epoch ≥ {} — {}", armed, due, why);
    }

    /// One friendship, now (no rotation to wait for): a repair verdict that resolved to HeavyWeave.
    pub(super) fn arm_heavy_weave_for(&mut self, ci: usize, why: &str) {
        let Some(c) = self.contacts.get_mut(ci) else { return };
        if c.is_sibling || c.friendship_id.is_none() {
            return;
        }
        if c.era_weave_due == 0 {
            c.era_weave_due = 1;
            crate::logf!("ERA: heavy weave armed for {} — {}", crate::fp(&c.handle_proof), why);
            let snapshot = c.clone();
            self.save_contacts_off_thread(vec![snapshot]);
        }
    }

    /// Persist contact rows off the UI thread (the vault commit is ~0.7 s under load; one per contact on this thread was a multi-second hang). Snapshots ride the worker; a later edit simply re-saves.
    pub(super) fn save_contacts_off_thread(&self, contacts: Vec<crate::types::Contact>) {
        if contacts.is_empty() {
            return;
        }
        let Some(storage) = self.storage.clone() else { return };
        let _ = std::thread::Builder::new().name("contacts-save".into()).spawn(move || {
            for c in &contacts {
                if let Err(e) = crate::storage::contacts::save_contact(c, &storage) {
                    crate::logf!("ERA: contact save failed off-thread: {}", e);
                }
            }
        });
    }

    /// After a cutover: persist the blob (the replication sweep pushes it on the mutated_osc edge) and re-serve undelivered rows on the fresh lane — the rotated_flush shape.
    pub(super) fn era_cutover_flush(&mut self, ci: usize, fid: &crate::types::friendship::FriendshipId) {
        self.persist_chains_async(fid);
        self.resend_held_messages(ci);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(trigger: RepairTrigger, peer: PeerEra, siblings: SiblingVerdict, we_own: bool, capable: bool) -> RepairInput {
        RepairInput { trigger, peer, siblings, we_own, peer_era_capable: capable }
    }

    /// The whole table: non-owners never mint, an unasked fleet is asked first, matching/behind peers always hold, a foreign channel goes to consent — and the verdict enum has no destructive member to reach.
    #[test]
    fn repair_table_never_destroys_and_owner_gates_every_mint() {
        let stale = RepairTrigger::StaleEraObserved { peer_index: 2, peer_tag: 0x1234 };
        for &peer in &[PeerEra::NoRecord, PeerEra::Same, PeerEra::Ahead, PeerEra::Behind, PeerEra::Foreign] {
            for &sib in &[SiblingVerdict::NoSiblings, SiblingVerdict::Unasked, SiblingVerdict::Asked, SiblingVerdict::AllMissed] {
                for &own in &[true, false] {
                    for &cap in &[true, false] {
                        let v = friendship_repair(&input(stale, peer, sib, own, cap));
                        if !own {
                            assert!(!matches!(v, RepairVerdict::LightRatchet | RepairVerdict::HeavyWeave | RepairVerdict::ConsentFresh), "non-owner minted: {peer:?} {sib:?} → {v:?}");
                        }
                        if matches!(peer, PeerEra::Same | PeerEra::NoRecord | PeerEra::Behind) {
                            assert!(matches!(v, RepairVerdict::Hold(_)), "{peer:?} must hold, got {v:?}");
                        }
                        if peer == PeerEra::Ahead && sib == SiblingVerdict::Unasked {
                            assert_eq!(v, RepairVerdict::PullFleet, "ask the fleet before minting");
                        }
                        if peer == PeerEra::Ahead && sib == SiblingVerdict::AllMissed && own {
                            assert_eq!(v, if cap { RepairVerdict::LightRatchet } else { RepairVerdict::HeavyWeave });
                        }
                        if peer == PeerEra::Foreign && sib == SiblingVerdict::Unasked {
                            assert_eq!(v, RepairVerdict::PullFleet, "a foreign era asks the fleet before consent");
                        }
                        if peer == PeerEra::Foreign && own && matches!(sib, SiblingVerdict::NoSiblings | SiblingVerdict::AllMissed) {
                            assert_eq!(v, RepairVerdict::ConsentFresh);
                        }
                    }
                }
            }
        }
        assert_eq!(friendship_repair(&input(RepairTrigger::FleetAllMissed, PeerEra::Ahead, SiblingVerdict::AllMissed, true, true)), RepairVerdict::LightRatchet);
        assert!(matches!(friendship_repair(&input(RepairTrigger::FleetAllMissed, PeerEra::Ahead, SiblingVerdict::AllMissed, false, true)), RepairVerdict::Hold(_)));
    }

    /// The cadence and a nudge ask for the light ratchet only: owner + capable ⇒ LightRatchet, anything else holds, and neither ever escalates to a heavy weave.
    #[test]
    fn cadence_and_nudge_are_light_only() {
        for t in [RepairTrigger::CadenceReached, RepairTrigger::NudgeReceived] {
            for &sib in &[SiblingVerdict::NoSiblings, SiblingVerdict::Unasked, SiblingVerdict::Asked, SiblingVerdict::AllMissed] {
                assert_eq!(friendship_repair(&input(t, PeerEra::Same, sib, true, true)), RepairVerdict::LightRatchet);
                assert!(matches!(friendship_repair(&input(t, PeerEra::Same, sib, false, true)), RepairVerdict::Hold(_)));
                assert!(matches!(friendship_repair(&input(t, PeerEra::Same, sib, true, false)), RepairVerdict::Hold(_)));
            }
        }
    }

    #[test]
    fn peer_era_classification() {
        assert_eq!(classify_peer_era(1, Some(10), 1, 10, None, None), PeerEra::Same);
        assert_eq!(classify_peer_era(1, Some(10), 0, 0, None, None), PeerEra::NoRecord);
        assert_eq!(classify_peer_era(1, Some(10), 0, 9, Some(9), None), PeerEra::Behind);
        assert_eq!(classify_peer_era(1, Some(10), 2, 11, None, None), PeerEra::Ahead);
        assert_eq!(classify_peer_era(1, Some(10), 2, 11, None, Some(11)), PeerEra::Ahead);
        assert_eq!(classify_peer_era(1, Some(10), 1, 77, None, None), PeerEra::Foreign, "same index, different tag = a channel we cannot order");
    }

    /// The computed owner: lowest wins, locked and probed-offline devices fall through, an UNPROBED sibling still holds (boot race), and an empty fold falls back to us + known siblings.
    #[test]
    fn era_owner_is_lowest_live_unlocked_device() {
        let a = [1u8; 32];
        let b = [2u8; 32];
        let c = [3u8; 32];
        let fold = [c, a, b];
        // Everyone unprobed: the lowest key owns, whether or not it is us.
        assert_eq!(era_owner(c, &fold, &[], &[(a, false, false), (b, false, false)], &[]), a);
        // a probed offline → b; b locked too → c (us).
        assert_eq!(era_owner(c, &fold, &[], &[(a, false, true), (b, true, true)], &[]), b);
        assert_eq!(era_owner(c, &fold, &[b], &[(a, false, true), (b, true, true)], &[]), c);
        // locked_out sibling rows are excluded like the locked set.
        assert_eq!(era_owner(c, &fold, &[], &[(a, true, true), (b, true, true)], &[a]), b);
        // We are never excluded by presence (we are here).
        assert_eq!(era_owner(a, &fold, &[], &[(b, false, true), (c, false, true)], &[]), a);
        // Empty fold: us + siblings.
        assert_eq!(era_owner(b, &[], &[], &[(a, true, true)], &[]), a);
        assert_eq!(era_owner(b, &[], &[], &[(a, false, true)], &[]), b);
        // A fold member with no contact row yet counts as present.
        assert_eq!(era_owner(c, &fold, &[], &[(b, false, true)], &[]), a);
    }
}
