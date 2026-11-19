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
    /// A FRIEND on our retired era where a full ceremony moved us on: it cannot catch up by itself (RetiredEra::heavy).
    pub peer_stranded: bool,
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
            // Behind on an era a light ratchet retired: the peer holds the pending era and its own cutover edge moves it. Behind on an era a CEREMONY retired: nothing it holds reaches ours — the owner re-keys it (fresh, since its prior claim cannot match ours).
            PeerEra::Behind if i.peer_stranded => {
                if i.we_own { ConsentFresh } else { Hold("not the era owner") }
            }
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
        // The fleet holds nothing newer. A peer AHEAD of us takes a ratchet from our era; a FOREIGN peer (a channel we cannot order — the same index under a different root) cannot decapsulate anything we mint from ours, so only a fresh channel reaches it (2026-09-10, Esme/Nick: the light ratchet here would have been sealed to an era Esme no longer held).
        RepairTrigger::FleetAllMissed => match i.peer {
            PeerEra::Foreign => {
                if i.we_own { ConsentFresh } else { Hold("not the era owner") }
            }
            _ => {
                if i.we_own { mint(i.peer_era_capable) } else { Hold("not the era owner") }
            }
        },
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

/// Is `d` a device that can carry a ceremony right now: ours, or a fold member (the fold when known, else the sibling list) that is not locked and not probed-offline. "Unprobed" counts as present (the boot-race rule: a freshly booted device must not take over from a live owner it has not heard from yet).
pub(crate) fn device_live(d: [u8; 32], ours: [u8; 32], fold: &[[u8; 32]], locked: &[[u8; 32]], siblings: &[super::SiblingPresence], locked_out: &[[u8; 32]]) -> bool {
    if locked.contains(&d) || locked_out.contains(&d) {
        return false;
    }
    if d == ours {
        return true;
    }
    if fold.is_empty() {
        if !siblings.iter().any(|(k, _, _)| *k == d) {
            return false;
        }
    } else if !fold.contains(&d) {
        return false;
    }
    match siblings.iter().find(|(k, _, _)| *k == d) {
        Some((_, online, probed)) => !(*probed && !*online),
        None => true,
    }
}

/// The fleet's FALLBACK ceremony owner, computed (plan §4): the lowest device pubkey among the live devices (`device_live`). Every device computes the same answer from the same evidence. It is the owner only for a friendship whose CLAIM is dead or absent — see `recompute_ceremony_owners`.
pub(crate) fn era_owner(ours: [u8; 32], fold: &[[u8; 32]], locked: &[[u8; 32]], siblings: &[super::SiblingPresence], locked_out: &[[u8; 32]]) -> [u8; 32] {
    let mut candidates: Vec<[u8; 32]> = if fold.is_empty() {
        let mut v = vec![ours];
        v.extend(siblings.iter().map(|(k, _, _)| *k));
        v
    } else {
        fold.to_vec()
    };
    candidates.retain(|d| device_live(*d, ours, fold, locked, siblings, locked_out));
    candidates.into_iter().min().unwrap_or(ours)
}

/// The lockout arming rule, pure: every friendship (mutual, with a bond on a live era, not locked) owes one re-key due at `due`; `tags[i]` is row i's bond era tag. Siblings, one-sided contacts and friendships with no era yet owe none.
/// A row that already owes one keeps the later due, and is measured from the era it stands on now, so the second of two lockouts is not satisfied by a re-key minted before its own rotation began.
/// Returns the rows armed.
pub(crate) fn arm_lockout_rekeys(contacts: &mut [crate::types::Contact], tags: &[Option<u32>], due: u64) -> Vec<usize> {
    let mut armed = Vec::new();
    for (i, c) in contacts.iter_mut().enumerate() {
        if c.is_sibling || !c.consent_mutual || c.friendship_id.is_none() || c.locked_out {
            continue;
        }
        let Some(tag) = tags[i] else { continue };
        if due > c.era_lockout_due {
            c.era_lockout_due = due;
        }
        c.era_lockout_from = tag;
        armed.push(i);
    }
    armed
}

/// A member's lockout armed before any key sync has said where the fan-out stands: no epoch is due until one does (`adopt_fanout_seen`).
pub(crate) const LOCKOUT_FANOUT_UNKNOWN: u64 = u64::MAX;

/// The fan-out as one key sync saw it (devices.rs spawn_fleet_key_sync): its revision, whether it still reaches a locked device, and the locked set it measured against.
/// "Reaches" is the rotation code's own criterion (fgtw fanout_needs_rotation): the fan-out wraps more devices than the fold minus the locked, so a locked device's wrap lingers; the wraps are unlabelled, so neither the chain nor the slot says which devices a fan-out reached, and this count is the one test the rotation itself acts on.
pub(crate) struct FanoutSeen {
    pub revision: u64,
    pub reaches_locked: bool,
    pub locked: Vec<[u8; 32]>,
}

/// Must the key sync mint: the count test on every edge (fgtw fanout_needs_rotation: more wraps than the fold minus the locked), OR a member's lockout this device owes a mint for, while the live fan-out is still under the key it held at the lock (`owed`, that key's fingerprint; `live_kfp`, the fan-out's).
/// The count alone misses a grow between the lock and the rotation: it rewraps the fold minus the locked under the OLD key, the counts read equal, and the locked member keeps a live key (the grow gap, 2026-11-18). A fan-out under any other key is a mint since the lock, which wrapped the fold minus the locked alone, and pays the debt.
pub(crate) fn rotation_due(wraps: usize, desired: usize, owed: Option<[u8; 32]>, live_kfp: [u8; 32]) -> bool {
    fgtw::fanout::fanout_needs_rotation(wraps, desired) || owed == Some(live_kfp)
}

/// The fan-out revision a member's lockout re-key waits for: the first whose key the locked device cannot hold.
/// A fan-out that still reaches it wraps a key it holds, so the wait is for the next revision, which is the rotation the lock starts (a mint wraps the fold minus the locked alone, and every later one does the same); one that no longer reaches it is that revision already.
/// Never a checkpoint counted from the lock: a routine checkpoint between the lock and the rotation folds the old key, and an era sealed under it would reach the locked device (the leak, 2026-11-18).
/// The value 1 doubles as "at once" (a device that never held the fleet key): a revision-1 fan-out that reaches no locked device means every key there ever was is past the lock anyway.
pub(crate) fn lockout_fanout_due(seen: &FanoutSeen) -> u64 {
    if seen.reaches_locked {
        seen.revision + 1
    } else {
        seen.revision
    }
}

/// Is a lockout's wait over: at once, or the fan-out revision folded into this device's current fleet epoch (the checkpoint's `ckpt_fanout_epoch`) is the one the wait names or later, so every chain_sync sealed from here is under a key the locked device never held.
pub(crate) fn lockout_wait_over(due: u64, folded: Option<u64>) -> bool {
    due == 1 || (due != LOCKOUT_FANOUT_UNKNOWN && folded.is_some_and(|r| r >= due))
}

/// The fan-out revision the checkpoint `k` folded, as the chain records it.
pub(crate) fn folded_fanout_revision(chain: &fgtw::fleet::MembershipBlob, k: u64) -> Option<u64> {
    chain.ops.iter().find(|op| op.kind == fgtw::fleet::OpKind::Checkpoint && op.ckpt_k == k).map(|op| op.ckpt_fanout_epoch)
}

/// Arm one group's lockout re-key, the friendships' rule: the later due wins, measured from the era the group stands on now.
pub(crate) fn arm_group_lockout(local: &mut crate::storage::molecule::MoleculeLocal, tag: u32, due: u64) {
    if due > local.lockout_due {
        local.lockout_due = due;
    }
    local.lockout_from = tag;
}

/// May this device mint a group's lockout era now: it is owed, its wait is over (`lockout_wait_over`, against `folded`, the fan-out revision this device's epoch folded), this device is the group's minter, and no era of the group is pending (a mint in flight, or a cutover not yet taken), so one group at a time and never two mints from one era.
pub(crate) fn group_lockout_ready(due: u64, folded: Option<u64>, we_mint: bool, pending: bool) -> bool {
    due != 0 && lockout_wait_over(due, folded) && we_mint && !pending
}

/// A lockout re-key is done once the bond stands on an era other than the one it was armed on.
pub(crate) fn lockout_rekey_done(c: &crate::types::Contact, tag: Option<u32>) -> bool {
    c.era_lockout_due != 0 && tag.is_some_and(|t| t != c.era_lockout_from)
}

/// May this device start the lockout re-key of `c` now: it is owed and its wait is over (`lockout_wait_over`), the friend is online, the friendship is complete with nothing of its own in flight, and this device is its ceremony owner (unclaimed reads as ours, the park rule).
pub(crate) fn lockout_rekey_ready(c: &crate::types::Contact, folded: Option<u64>, ours: Option<[u8; 32]>) -> bool {
    c.era_lockout_due != 0
        && lockout_wait_over(c.era_lockout_due, folded)
        && !c.is_sibling
        && !c.locked_out
        && c.is_online
        && c.clutch_state == crate::types::ClutchState::Complete
        && !c.clutch_keygen_in_progress
        && c.era_ephemeral.is_none()
        && c.era_weave_due == 0
        && c.ceremony_owner.map_or(true, |o| Some(o) == ours)
}

/// Which sibling rows the locked set newly locks: rows of ours not yet flagged whose device the set names. Only these arm a re-key; a sibling that left by signing itself out is in no locked set, so its leaving arms nothing.
pub(crate) fn newly_locked_rows(contacts: &[crate::types::Contact], locked: &[[u8; 32]]) -> Vec<usize> {
    contacts
        .iter()
        .enumerate()
        .filter(|(_, c)| c.is_sibling && !c.locked_out && c.device_key().is_some_and(|k| locked.contains(&k)))
        .map(|(i, _)| i)
        .collect()
}

impl PhotonApp {
    /// THE BALL STAYS WITH WHOEVER GRABBED IT (Nick 2026-10-01: "pin that to the first device to grab the ball, rather than a low hash … for a device that may not be online very often"). Per friendship: the CLAIM on the roster entry (`ceremony_owner`, set by the adding device, synced LWW thru the roster) stands while that device is live (`device_live`); only a dead or absent claim falls back to the computed lowest-live device — and when the fallback is THIS device it TAKES OVER by writing a fresh claim and pushing the roster, so the sleepy claimant does not reclaim the ball when it wakes (it adopts the newer entry instead). Deterministic across the fleet because every device reads the same claim; the two places presence views can still disagree are arbitrated as before — the peer takes the first Init, and the loser discards on park.
    /// Called on the evidence EDGES (a sibling's presence verdict, a fold adopt, a locked-set change, a roster adopt) — never on a timer. A round this device holds for a friendship that just moved to another owner is discarded (fleet-sync.md §4.2 discard-on-park), so the friend never sees two instances from one fleet.
    pub(super) fn recompute_ceremony_owners(&mut self, why: &str) {
        // Photon on the alias alone (ferros) runs no friend ceremony, so it never takes the ball: the claims stay as its siblings' roster carries them.
        // Until 2026-11-18 the Pro took all fifteen at each launch (its claimants not yet heard), stamped each row's roster clock with now, pushed that claim to the siblings, and then skipped every sibling entry as older, the names they carried included.
        if self.alias_alone() {
            return;
        }
        let Some(ours) = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes()) else { return };
        let locked = self.locked_devices();
        let siblings = super::sibling_presence_snapshot(&self.contacts);
        let locked_out: Vec<[u8; 32]> = self.contacts.iter().filter(|c| c.is_sibling && c.locked_out).filter_map(|c| c.device_key()).collect();
        let fold = self.registry_converged_fold.clone();
        let fallback = era_owner(ours, &fold, &locked, &siblings, &locked_out);
        let mut moved = 0usize;
        let mut discarded = 0usize;
        let mut took_over = 0usize;
        let now = vsf::eagle_time_oscillations();
        let mut to_persist: Vec<usize> = Vec::new();
        for (i, c) in self.contacts.iter_mut().enumerate().filter(|(_, c)| !c.is_sibling) {
            let owner = match c.ceremony_owner {
                Some(d) if device_live(d, ours, &fold, &locked, &siblings, &locked_out) => d,
                _ => fallback,
            };
            if c.ceremony_owner == Some(owner) {
                continue;
            }
            let holds_round = c.clutch_our_keypairs.is_some() || c.clutch_offer_sent || !c.clutch_slots.is_empty() || c.ceremony_id.is_some();
            if owner != ours && c.clutch_state != crate::types::ClutchState::Complete && holds_round {
                crate::logf!("CLUTCH §4.2: discarding our round for {} — the fleet's ceremony owner is {}", crate::fp(&c.handle_proof).as_str(), hex::encode(&owner[..4]));
                c.discard_clutch_round();
                discarded += 1;
            }
            let from = c.ceremony_owner;
            c.ceremony_owner = Some(owner);
            moved += 1;
            // The takeover IS a claim: it rides the roster with a fresh LWW clock, so every sibling (the dead claimant included, when it wakes) adopts it.
            if owner == ours {
                c.roster_updated = now;
                c.owner_woven = false;
                took_over += 1;
                to_persist.push(i);
                crate::logf!("CLUTCH §4.2: taking the ball for {} — its claimant {} is not live", crate::fp(&c.handle_proof).as_str(), from.map_or("none".to_string(), |d| hex::encode(&d[..4])));
            }
        }
        if !to_persist.is_empty() {
            if let Some(storage) = self.storage.as_ref() {
                for i in to_persist {
                    crate::log_err!(crate::storage::contacts::save_contact(&self.contacts[i], storage), "the contact save after an era change");
                }
            }
            self.spawn_roster_push();
        }
        if moved > 0 {
            crate::logf!(
                "ERA: ceremony owner moved for {} friendship(s) ({}) — fallback {}{}, {} taken over here, {} parked round(s) discarded",
                moved,
                why,
                hex::encode(&fallback[..4]),
                if fallback == ours { " (this device)" } else { "" },
                took_over,
                discarded
            );
        }
    }

    /// Build the input, decide, log one line, and act on the verdicts built so far. The rest log their intent and nothing else — never a destructive fallback.
    pub(super) fn repair_dispatch(&mut self, ci: usize, trigger: RepairTrigger) {
        let Some(contact) = self.contacts.get(ci) else { return };
        // The era machinery runs on the BOND (docs/conversations.md §7); a conversation's eras are minted and delivered over bonds.
        let Some(fid) = self.bond_chains_id(contact) else { return };
        let Some((_, chains)) = self.friendship_chains.iter().find(|(id, _)| *id == fid) else { return };
        let classify = |peer_index: u64, peer_tag: u32| classify_peer_era(
            chains.era_index,
            chains.era_tag(),
            peer_index,
            peer_tag,
            chains.retired_era().map(|r| r.tag),
            chains.pending_era().map(|p| p.tag),
        );
        let token = chains.conversation_token;
        let peer = match trigger {
            RepairTrigger::StaleEraObserved { peer_index, peer_tag } => classify(peer_index, peer_tag),
            // The fleet's answer arrives long after the observation that asked — classify against the era the peer LAST advertised, not a blanket "ahead" (a foreign era needs the fresh channel, never a ratchet from ours).
            // With no observation recorded here (the era-mismatch heal, bonds.rs, asked the fleet itself — 2026-11-01, Emma's era read as "ahead" and the verdict was a ratchet from an era she never held), the friend's own devices' advertisements are the evidence: an era of theirs we cannot place is foreign.
            RepairTrigger::FleetAllMissed => match self.era_peer_seen.get(&token) {
                Some((idx, tag)) => classify(*idx, *tag),
                None => self
                    .peer_era_seen
                    .iter()
                    .filter(|((f, dev), _)| *f == fid && contact.knows_device(dev))
                    .map(|(_, (idx, tag))| classify(*idx, *tag))
                    .find(|p| !matches!(p, PeerEra::Same | PeerEra::NoRecord))
                    .unwrap_or(PeerEra::Ahead),
            },
            RepairTrigger::CadenceReached | RepairTrigger::NudgeReceived => PeerEra::Same,
        };
        let seen_tag = match trigger {
            RepairTrigger::StaleEraObserved { peer_tag, .. } => Some(peer_tag),
            _ => self.era_peer_seen.get(&token).map(|(_, t)| *t),
        };
        let peer_stranded = peer == PeerEra::Behind && !contact.is_sibling && chains.retired_era().is_some_and(|r| r.heavy && Some(r.tag) == seen_tag);
        let held = chains.era_index;
        let held_tag = chains.era_tag();
        if let RepairTrigger::StaleEraObserved { peer_index, peer_tag } = trigger {
            self.era_peer_seen.insert(token, (peer_index, peer_tag));
        }
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
        let verdict = friendship_repair(&RepairInput { trigger, peer, siblings, we_own, peer_era_capable: capable, peer_stranded });
        let line = format!("{trigger:?} peer={peer:?} stranded={peer_stranded} siblings={siblings:?} own={we_own} capable={capable} → {verdict:?}");
        crate::logf!("ERA: {} {}", fp, line);
        match verdict {
            RepairVerdict::PullFleet => {
                if let Some(kp) = self.device_keypair.as_ref() {
                    if let Ok(frame) = crate::network::fgtw::protocol::build_chain_pull_vsf(&token, kp.public.as_bytes(), kp.secret.as_bytes(), Some(held), held_tag) {
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
            // FRESH CHANNEL (2026-09-10): the peer is on an era neither we nor any live sibling hold, so nothing minted from ours can reach it — the owner re-runs the CLUTCH (the heavy-weave pickup, whose prior claim will not match the peer's, so it completes as a fresh channel). The stage-5 consent gate is still to come; today a restart already re-runs ceremonies unasked, so this adds no new consent surface, only a repair that ends instead of holding forever. One round at a time: a friendship mid-ceremony is left to finish.
            RepairVerdict::ConsentFresh => {
                let mid_round = self.contacts.get(ci).is_some_and(|c| c.clutch_state != crate::types::ClutchState::Complete || c.clutch_ceremony_in_progress || c.clutch_keygen_in_progress);
                if mid_round {
                    crate::logf!("ERA: {} is on a channel we cannot order against ours — a ceremony is already running, leaving it to finish", fp);
                } else {
                    self.arm_heavy_weave_for(ci, "peer on a channel we cannot order and no live sibling holds it — fresh channel (consent gate not yet built)");
                }
            }
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
        let Some(fid) = self.bond_chains_id(contact) else { return false };
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
            let ctl = crate::types::RowControl::Era(EraSignal::Nudge { prior_tag: tag });
            let sent = self.bond_transmit_with(ci, ts, None, None, &ctl);
            crate::logf!("ERA: {} is the initiating identity — Nudge {} from era#{} ({:08x})", fp, if sent { "sent" } else { "NOT sent" }, era_next - 1, tag);
            return sent;
        }
        // Keygen inline: ML-KEM-1024 + X25519 + HQC-256 keypairs cost a few milliseconds — sub-frame, once per 256 rows. (McEliece was the slow one; it is not in this set.)
        let eph = era_keygen(era_next, tag, KEM_SET_DEFAULT);
        let wire = eph.init_wire.clone();
        let ctl = crate::types::RowControl::Era(EraSignal::Init { era_next, nonce: eph.nonce, prior_tag: tag, kem_set: eph.kem_set });
        let bytes = wire.mlkem.len() + wire.x25519.len() + wire.hqc.len();
        let nonce_s = hex::encode(&eph.nonce[..4]);
        self.contacts[ci].era_ephemeral = Some(eph);
        let sent = self.bond_transmit_with(ci, ts, Some(&wire), None, &ctl);
        if sent {
            crate::logf!("ERA: {} era#{} → era#{} Init sent on {:08x} (nonce {}…, {} B of public keys) — chatting on the current era until the Resp lands", fp, era_next - 1, era_next, tag, nonce_s, bytes);
        } else {
            self.contacts[ci].era_ephemeral = None;
            crate::logf!("ERA: {} Init could not be sent — ephemerals dropped; the next edge retries", fp);
        }
        sent
    }

    /// An era row landed (conversation.rs control ladder, post-decrypt, post-commit). `kem` is the package's typed KEM material; `ts` the row's eagle time.
    pub(super) fn on_era_signal(&mut self, ci: usize, sig: EraSignal, kem: Option<EraKemWire>, _ts: i64) {
        let Some(contact) = self.contacts.get(ci) else { return };
        let fp = crate::fp(&contact.handle_proof);
        // The era machinery runs on the BOND (docs/conversations.md §7); a conversation's eras are minted and delivered over bonds.
        let Some(fid) = self.bond_chains_id(contact) else { return };
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
                        let ctl = crate::types::RowControl::Era(EraSignal::Resp { era_next, nonce, prior_tag });
                        let resp_ts = self.friendship_chains[pos].1.pending_era().and_then(|p| p.resp_osc).unwrap_or_else(vsf::eagle_time_oscillations);
                        let sent = self.bond_transmit_with(ci, resp_ts, Some(&cached), None, &ctl);
                        crate::logf!("ERA: {} duplicate Init (nonce {}…) — cached Resp {}", fp, hex::encode(&nonce[..4]), if sent { "re-sent" } else { "not re-sent (in flight or window full)" });
                        return;
                    }
                }
                // THE STRANDED PEER (Emma ↔ Nick 2026-10-24, the molecule join that never landed): we are a step ahead, and the peer ratchets from our RETIRED era.
                // A behind peer sends that only after its own fleet came up empty (FleetAllMissed → LightRatchet), so no device of theirs holds our era — and our rule ("the peer is behind — its own repair moves it") and theirs (that very Init, which we drop) wait on each other for ever, every frame dropped both ways.
                // Heal with one fresh ceremony: a new root supersedes both eras on every device (the era supersede path), once per contact per session so it can never loop.
                let stranded = {
                    let chains = &self.friendship_chains[pos].1;
                    chains.era_tag().is_some_and(|our| our != prior_tag) && chains.retired_era().is_some_and(|r| r.tag == prior_tag)
                };
                if stranded {
                    let them = self.contacts[ci].handle_hash;
                    if self.era_reclutched.insert(them) {
                        crate::logf!("ERA: {} Init ratchets from our RETIRED era {:08x} — their fleet lost our era; healing with a fresh ceremony", fp, prior_tag);
                        self.rekey_without_chains(ci);
                    } else {
                        crate::logf!("ERA: {} Init from our retired era again — the fresh ceremony is already under way", fp);
                    }
                    return;
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
                let ctl = crate::types::RowControl::Era(EraSignal::Resp { era_next, nonce, prior_tag });
                let sent = self.bond_transmit_with(ci, resp_ts, Some(&resp), None, &ctl);
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
            }
            EraSignal::Nudge { prior_tag } => {
                let ours = self.friendship_chains[pos].1.era_tag();
                if ours == Some(prior_tag) {
                    self.repair_dispatch(ci, RepairTrigger::NudgeReceived);
                } else if let Some(r) = self.friendship_chains[pos].1.retired_era().filter(|r| r.heavy && r.tag == prior_tag && !self.contacts[ci].is_sibling).map(|r| r.era_index) {
                    // A STRANDED friend nudging from the era a ceremony retired (field 2026-09-28: Emma nudged all evening, ignored): its nudge is the edge that it is up and asking. A round already pending re-fires our offer now (its earlier sends may have landed on a build that could not answer); none pending starts the re-key.
                    let pending = self.contacts[ci].clutch_state != crate::types::ClutchState::Complete && self.contacts[ci].clutch_our_keypairs.is_some();
                    if pending {
                        crate::logf!("ERA: {} nudged from our retired era {:08x} — stranded; re-firing our pending offer", fp, prior_tag);
                        self.contacts[ci].clutch_offer_sent = false;
                        self.resend_clutch_offer(ci);
                    } else {
                        crate::logf!("ERA: {} nudged from our retired era {:08x} — stranded; re-keying", fp, prior_tag);
                        self.repair_dispatch(ci, RepairTrigger::StaleEraObserved { peer_index: r, peer_tag: prior_tag });
                    }
                } else {
                    crate::logf!("ERA: {} nudged from {:08x} but we are on {} — ignored (its pong or our chain-sync converges it)", fp, prior_tag, ours.map(|t| format!("{t:08x}")).unwrap_or_else(|| "none".into()));
                }
            }
        }
    }

    /// LOCKOUT EDGE (the owner 2026-11-18: "one of which triggers a re-key and locks out that device"): every friendship with a live era owes one re-key, by the light era ratchet, started one friend at a time as each comes online (`lockout_rekey_step`).
    /// `due` is the fan-out revision the re-key waits for (`lockout_fanout_due`): for a member, the first whose key the locked device cannot hold, unknown until a key sync reports (`LOCKOUT_FANOUT_UNKNOWN`, resolved by `adopt_fanout_seen`), so the new era's chain_sync seals only under a key it never held; 1 for a device that never held the fleet key (a Photon on the alias alone), which waits for nothing.
    /// A device that signed itself out arms nothing: it asserted it wiped its keys (the other path). Persisted per contact, so a restart cannot lose a lockout's re-key.
    pub(super) fn arm_lockout_rekeys(&mut self, due: u64, why: &str) {
        let tags: Vec<Option<u32>> = self.contacts.iter().map(|c| self.bond_era_tag(c)).collect();
        let armed = arm_lockout_rekeys(&mut self.contacts, &tags, due);
        let dirty: Vec<crate::types::Contact> = armed.iter().map(|&i| self.contacts[i].clone()).collect();
        self.save_contacts_off_thread(dirty);
        let groups = self.arm_group_lockout_rekeys(due);
        crate::logf!("ERA: lockout re-key owed by {} friendship(s) and {} group(s), {} ({})", armed.len(), groups, if due == LOCKOUT_FANOUT_UNKNOWN { "waiting for a fan-out without the locked device".to_string() } else { format!("due at fan-out revision {due}") }, why);
        self.lockout_rekey_step(why);
    }

    /// Every group this device stands in owes one fresh era too (the owner 2026-11-18: a locked device holds every group's keys as well): armed on the group's device-local state, so a restart cannot lose it.
    /// Only the group's minter (its lowest standing party, on the one device of that fleet that acts, molecules.rs) can mint; a group another member mints is left armed here and done when that member's next mint lands, which wraps nothing to our locked device once our fold or our pongs name it locked.
    fn arm_group_lockout_rekeys(&mut self, due: u64) -> usize {
        let groups: Vec<(crate::types::molecule::MoleculeId, u32)> = self
            .molecule_rosters
            .iter()
            .map(|(g, _)| *g)
            .filter(|g| !self.molecule_is_left(g))
            .filter_map(|g| {
                let fid = crate::types::FriendshipId::from_bytes(g.0);
                self.friendship_chains.iter().find(|(id, _)| *id == fid).and_then(|(_, c)| c.era_tag()).map(|t| (g, t))
            })
            .collect();
        for (g, tag) in &groups {
            self.set_molecule_local(g, |l| arm_group_lockout(l, *tag, due));
        }
        groups.len()
    }

    /// Start at most ONE group's lockout mint: a group whose era moved off the one it was armed on is done; while one this device minted is still pending (its era not cut over), the next waits; the next goes only where this device is the group's minter.
    fn group_lockout_step(&mut self, why: &str) {
        let armed: Vec<(crate::types::molecule::MoleculeId, u64, u32)> = self.molecule_locals.iter().filter(|(_, l)| l.lockout_due != 0).map(|(g, l)| (*g, l.lockout_due, l.lockout_from)).collect();
        if armed.is_empty() {
            self.lockout_group_busy = None;
            return;
        }
        let chains_of = |app: &Self, g: &crate::types::molecule::MoleculeId| {
            let fid = crate::types::FriendshipId::from_bytes(g.0);
            app.friendship_chains.iter().find(|(id, _)| *id == fid).map(|(_, c)| (c.era_tag(), c.pending_era().is_some()))
        };
        for (g, _, from) in &armed {
            if chains_of(self, g).and_then(|(t, _)| t).is_some_and(|t| t != *from) {
                crate::logf!("MOLECULE: {} re-keyed since the lockout", hex::encode(&g.0[..4]));
                self.set_molecule_local(g, |l| {
                    l.lockout_due = 0;
                    l.lockout_from = 0;
                });
            }
        }
        if let Some(g) = self.lockout_group_busy {
            if self.molecule_locals.iter().any(|(id, l)| *id == g && l.lockout_due != 0) && chains_of(self, &g).is_some_and(|(_, pending)| pending) {
                return;
            }
            self.lockout_group_busy = None;
        }
        let folded = self.folded_fanout_now();
        let us = self.own_party_id();
        let acts = self.fleet_device_acts(None);
        let pick = self.molecule_locals.iter().filter(|(_, l)| l.lockout_due != 0).map(|(g, l)| (*g, l.lockout_due)).find(|(g, due)| {
            let minter = self.molecule_rosters.iter().find(|(id, _)| id == g).and_then(|(_, r)| r.standing().first().copied());
            let pending = chains_of(self, g).is_some_and(|(_, p)| p);
            group_lockout_ready(*due, folded, us.is_some() && us == minter && acts, pending)
        });
        if let Some((g, _)) = pick {
            crate::logf!("MOLECULE: {} lockout re-key: minting the next era without the locked device ({})", hex::encode(&g.0[..4]), why);
            self.lockout_group_busy = Some(g);
            self.mint_molecule_era(g, None);
        }
    }

    /// The fan-out revision folded into this device's current fleet epoch, as our held chain's checkpoint records it; None before an epoch or the chain is held. Read only while a lockout re-key waits.
    fn folded_fanout_now(&self) -> Option<u64> {
        let k = self.fleet_epoch.map(|(k, _)| k)?;
        let chain = crate::network::fgtw::fleet::held_chain(&self.our_handle_proof()?)?;
        folded_fanout_revision(&chain, k)
    }

    /// A key sync said where the fan-out stands: every lockout re-key still waiting to learn its revision (`LOCKOUT_FANOUT_UNKNOWN`) learns it here, when the sync measured against every member lock this device knows (one begun before a lock would count the locked device as wanted).
    pub(super) fn adopt_fanout_seen(&mut self, seen: FanoutSeen) {
        if !self.fleet_locked_members().iter().all(|d| seen.locked.contains(d)) {
            return;
        }
        let due = lockout_fanout_due(&seen);
        let mut dirty: Vec<crate::types::Contact> = Vec::new();
        for c in self.contacts.iter_mut().filter(|c| c.era_lockout_due == LOCKOUT_FANOUT_UNKNOWN) {
            c.era_lockout_due = due;
            dirty.push(c.clone());
        }
        let groups: Vec<crate::types::molecule::MoleculeId> = self.molecule_locals.iter().filter(|(_, l)| l.lockout_due == LOCKOUT_FANOUT_UNKNOWN).map(|(g, _)| *g).collect();
        for g in &groups {
            self.set_molecule_local(g, |l| l.lockout_due = due);
        }
        if dirty.is_empty() && groups.is_empty() {
            return;
        }
        crate::logf!("ERA: the fan-out is at revision {} and {} a locked device; {} friendship(s) and {} group(s) re-key once this device's epoch folds revision {}", seen.revision, if seen.reaches_locked { "still reaches" } else { "no longer reaches" }, dirty.len(), groups.len(), due);
        self.save_contacts_off_thread(dirty);
        self.lockout_rekey_step("the fan-out was seen");
    }

    /// The era tag of the bond a friendship's eras run on, as this device holds it; None without chains.
    fn bond_era_tag(&self, c: &crate::types::Contact) -> Option<u32> {
        let fid = self.bond_chains_id(c)?;
        self.friendship_chains.iter().find(|(id, _)| *id == fid).and_then(|(_, ch)| ch.era_tag())
    }

    /// Start at most ONE lockout re-key, never a storm: called on the edges that can let one go (the lockout itself, a friend coming online, an era cutting over) and by the keygen pickup scan.
    /// A friendship whose bond moved off the era it was armed on is done, whichever device moved it (the owner's ratchet replicates to every sibling).
    /// While one friend's re-key is in flight and that friend is online, the next waits; one going offline lets the next go, and it is tried again at its own next online edge.
    /// Only the friendship's ceremony owner starts one: the light ratchet when the friend can ratchet in band, else the woven CLUTCH (`arm_heavy_weave_for`), whose own pickup runs one at a time.
    pub(super) fn lockout_rekey_step(&mut self, why: &str) {
        if self.alias_alone() {
            return;
        }
        self.group_lockout_step(why);
        if self.contacts.iter().all(|c| c.era_lockout_due == 0) {
            return;
        }
        let tags: Vec<Option<u32>> = self.contacts.iter().map(|c| self.bond_era_tag(c)).collect();
        let mut dirty: Vec<crate::types::Contact> = Vec::new();
        for (i, c) in self.contacts.iter_mut().enumerate() {
            if lockout_rekey_done(c, tags[i]) {
                crate::logf!("ERA: {} re-keyed since the lockout ({:08x} → {:08x})", crate::fp(&c.handle_proof), c.era_lockout_from, tags[i].unwrap_or(0));
                c.era_lockout_due = 0;
                c.era_lockout_from = 0;
                dirty.push(c.clone());
            }
        }
        self.save_contacts_off_thread(dirty);
        if let Some(id) = self.lockout_rekey_busy {
            if self.contacts.iter().any(|c| c.id == id && c.era_lockout_due != 0 && c.is_online) {
                return;
            }
            self.lockout_rekey_busy = None;
        }
        let ours = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes());
        let folded = self.folded_fanout_now();
        let skip = &self.lockout_rekey_skip;
        let Some(ci) = self.contacts.iter().position(|c| !skip.contains(&c.id) && lockout_rekey_ready(c, folded, ours)) else {
            return;
        };
        let id = self.contacts[ci].id;
        let fp = crate::fp(&self.contacts[ci].handle_proof);
        if self.contacts[ci].peer_era_capable {
            if self.propose_light_ratchet(ci) {
                self.lockout_rekey_busy = Some(id);
                crate::logf!("ERA: {} lockout re-key by the light ratchet: proposed ({})", fp, why);
            } else {
                // Passed over until this friend's next online edge (status.rs clears the entry), so the next friend is not held behind it.
                self.lockout_rekey_skip.push(id);
                crate::logf!("ERA: {} lockout re-key not proposed; tried again at its next online edge ({})", fp, why);
            }
        } else {
            self.lockout_rekey_busy = Some(id);
            self.arm_heavy_weave_for(ci, "a device was locked out and this friend cannot ratchet in band");
        }
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
        crate::log_err!(std::thread::Builder::new().name("contacts-save".into()).spawn(move || {
            for c in &contacts {
                if let Err(e) = crate::storage::contacts::save_contact(c, &storage) {
                    crate::logf!("ERA: contact save failed off-thread: {}", e);
                }
            }
        }), "the contacts-save thread spawn");
    }

    /// One sibling's era_pull answer was a MISS (an explicit chain_pull_miss, or a served blob that did not move our era — 2026-09-10). When every LIVE sibling has missed, the pull concludes and the repair table decides from FleetAllMissed.
    pub(super) fn era_pull_miss_from(&mut self, token: [u8; 32], sender_key: [u8; 32]) {
        if !self.era_pull_sent.contains_key(&token) {
            return;
        }
        let set = self.chain_pull_misses.entry(token).or_default();
        if !set.contains(&sender_key) {
            set.push(sender_key);
        }
        self.conclude_era_pull(token);
    }

    /// A sibling's presence verdict landed: every open era_pull is checked again (2026-11-01, the Mac's pull: the phone counted as present until its probe failed, the desktop's miss had already arrived, and with no further miss to come the pull waited forever).
    pub(super) fn conclude_era_pulls(&mut self) {
        let open: Vec<[u8; 32]> = self.era_pull_sent.iter().map(|(t, _)| *t).collect();
        for token in open {
            self.conclude_era_pull(token);
        }
    }

    /// The pull concludes when every LIVE sibling has missed, and at least one answered — a fleet that is all away has said nothing, and the device waits for one to come back.
    fn conclude_era_pull(&mut self, token: [u8; 32]) {
        let Some(&held) = self.era_pull_sent.get(&token) else { return };
        let all_missed = {
            let Some(set) = self.chain_pull_misses.get(&token).filter(|s| !s.is_empty()) else { return };
            // The quorum is the LIVE fleet: a sibling probed offline (a phone in a desk for a week, or a wiped device never released) cannot answer and needs nothing from this answer — replication carries the era to it on its return. Counting it froze every repair on "era_pull in flight" for as long as it stayed dark (2026-09-09).
            self.contacts.iter().filter(|c| c.is_sibling && !c.locked_out && !(c.presence_probed && !c.is_online)).all(|c| set.iter().any(|d| c.knows_device(d)))
        };
        if !all_missed {
            return;
        }
        self.chain_pull_misses.remove(&token);
        self.era_pull_sent.remove(&token);
        crate::logf!("ERA: every live sibling answered the era_pull with a miss — no era newer than #{} in the fleet", held);
        // The token names a friend's BOND — the chains the era machinery runs on (docs/conversations.md §7).
        let ci = self.contacts.iter().position(|c| {
            !c.is_sibling
                && self.bond_chains_id(c).is_some_and(|fid| self.friendship_chains.iter().any(|(id, ch)| *id == fid && ch.conversation_token == token))
        });
        if let Some(ci) = ci {
            self.repair_dispatch(ci, RepairTrigger::FleetAllMissed);
        }
    }

    /// After a cutover: persist the blob (the replication sweep pushes it on the mutated_osc edge) and re-serve undelivered rows on the fresh lane — the rotated_flush shape.
    pub(super) fn era_cutover_flush(&mut self, ci: usize, fid: &crate::types::friendship::FriendshipId) {
        self.persist_chains_async(fid);
        // A GROUP cutover (docs/molecules.md §10.4): undelivered rows re-serve on the fresh lane and this device publishes a fresh bundle for the new era; a friendship flushes its held rows.
        if self.molecule_rosters.iter().any(|(g, _)| g.0 == *fid.as_bytes()) {
            let gid = crate::types::molecule::MoleculeId(*fid.as_bytes());
            self.after_molecule_cutover(gid);
        } else {
            self.resend_held_messages(ci);
        }
        // A cutover may finish a lockout re-key: the next friend's may go.
        self.lockout_rekey_step("an era cut over");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(trigger: RepairTrigger, peer: PeerEra, siblings: SiblingVerdict, we_own: bool, capable: bool) -> RepairInput {
        RepairInput { trigger, peer, siblings, we_own, peer_era_capable: capable, peer_stranded: false }
    }

    /// A friend stranded on an era a ceremony retired is re-keyed by the owner (fresh), never left holding; a non-owner still holds; a peer merely behind a light ratchet still holds.
    #[test]
    fn a_stranded_friend_is_re_keyed_by_the_owner() {
        let stale = RepairTrigger::StaleEraObserved { peer_index: 0, peer_tag: 0x1748_4aa3 };
        let mut i = input(stale, PeerEra::Behind, SiblingVerdict::Unasked, true, true);
        assert!(matches!(friendship_repair(&i), RepairVerdict::Hold(_)), "behind a light ratchet: hold");
        i.peer_stranded = true;
        assert_eq!(friendship_repair(&i), RepairVerdict::ConsentFresh);
        i.we_own = false;
        assert!(matches!(friendship_repair(&i), RepairVerdict::Hold(_)), "only the owner re-keys");
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
        // A foreign peer after the fleet missed: nothing minted from our era can reach it — the fresh channel, owner only.
        assert_eq!(friendship_repair(&input(RepairTrigger::FleetAllMissed, PeerEra::Foreign, SiblingVerdict::AllMissed, true, true)), RepairVerdict::ConsentFresh);
        assert!(matches!(friendship_repair(&input(RepairTrigger::FleetAllMissed, PeerEra::Foreign, SiblingVerdict::AllMissed, false, true)), RepairVerdict::Hold(_)));
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

    fn friend(n: u8) -> crate::types::Contact {
        let mut c = crate::types::Contact::new(crate::types::HandleText::new("f"), [n; 32], crate::types::DevicePubkey::from_bytes([n | 0x80; 32]));
        c.friendship_id = Some(crate::types::FriendshipId::from_bytes([n; 32]));
        c.consent_mutual = true;
        c.clutch_state = crate::types::ClutchState::Complete;
        c
    }

    fn sibling(n: u8) -> crate::types::Contact {
        crate::types::Contact::new_sibling([0x48; 32], crate::types::DevicePubkey::from_bytes([n; 32]))
    }

    /// The owner's two paths, side by side: a sibling that signed itself out leaves the rows and is in no locked set, so nothing arms; a sibling locked out flips its row and arms exactly one re-key per friendship, none for siblings, a one-sided contact or a friendship with no era yet.
    #[test]
    fn a_sign_out_arms_no_rekey_and_a_lockout_arms_one_per_friendship() {
        let gone = [0x31u8; 32];
        let stays = [0x32u8; 32];
        let mut one_sided = friend(4);
        one_sided.consent_mutual = false;
        let no_era = friend(5);
        let mut rows = vec![sibling(0x32), friend(1), friend(2), friend(3), one_sided, no_era];
        let tags = vec![None, Some(0xA1), Some(0xA2), Some(0xA3), Some(0xA4), None];

        // SIGN-OUT: the departed sibling's row is gone and the locked set does not name it.
        assert!(newly_locked_rows(&rows, &[]).is_empty(), "a self-signed departure locks no row, so it arms no re-key");
        assert!(rows.iter().all(|c| c.era_lockout_due == 0));

        // LOCKOUT: the sibling still listed is locked; one row flips and every friendship owes one re-key.
        rows.push(sibling(0x31));
        let mut tags = tags;
        tags.push(None);
        let flipped = newly_locked_rows(&rows, &[gone]);
        assert_eq!(flipped, vec![6]);
        assert!(!flipped.contains(&0), "the sibling that stays is not locked: {:?}", stays);
        let armed = arm_lockout_rekeys(&mut rows, &tags, 1);
        assert_eq!(armed, vec![1, 2, 3], "one per friendship");
        assert!(armed.iter().all(|&i| rows[i].era_lockout_due == 1 && Some(rows[i].era_lockout_from) == tags[i]));
        // A member's lockout later waits for its rotation: the later due wins, measured from the era it stands on then.
        let again = arm_lockout_rekeys(&mut rows, &[None, Some(0xB1), Some(0xA2), Some(0xA3), Some(0xA4), None, None], 9);
        assert_eq!(again, vec![1, 2, 3]);
        assert_eq!((rows[1].era_lockout_due, rows[1].era_lockout_from), (9, 0xB1));
    }

    /// Groups after a lockout: every group armed, and only the group's minter mints, one group at a time, after the lock's epoch, never over a pending era.
    #[test]
    fn a_lockout_arms_every_group_and_only_the_minter_mints() {
        let mut local = crate::storage::molecule::MoleculeLocal::default();
        arm_group_lockout(&mut local, 0xC1, 5);
        assert_eq!((local.lockout_due, local.lockout_from), (5, 0xC1));
        arm_group_lockout(&mut local, 0xC2, 3);
        assert_eq!((local.lockout_due, local.lockout_from), (5, 0xC2), "the later due wins, from the era it stands on now");
        assert!(group_lockout_ready(5, Some(5), true, false));
        assert!(!group_lockout_ready(5, Some(4), true, false), "waits for the fan-out without the locked device");
        assert!(!group_lockout_ready(5, Some(5), false, false), "another member mints this group");
        assert!(!group_lockout_ready(5, Some(5), true, true), "an era pending: one mint at a time");
        assert!(group_lockout_ready(1, None, true, false), "a device that never held the fleet key: at once");
        assert!(!group_lockout_ready(LOCKOUT_FANOUT_UNKNOWN, Some(9), true, false), "no key sync has said where the fan-out stands");
        assert!(!group_lockout_ready(0, Some(9), true, false), "nothing owed");
    }

    /// The leak, 2026-11-18: a routine checkpoint minted between the lock and the rotation folds the old fleet key, which the locked member still holds, so the re-key must not go on it; it goes on the first checkpoint that folds the fan-out the rotation minted without the locked device.
    #[test]
    fn a_routine_checkpoint_between_the_lock_and_the_rotation_carries_no_rekey() {
        use fgtw::fleet::{bindreq_signing_bytes, scheme, MembershipBlob};
        use fgtw::pq::FleetSigner;
        let hp = [0x48u8; 32];
        let a = fgtw::keys::Keypair::from_seed(&[1; 32]);
        let b = fgtw::keys::Keypair::from_seed(&[2; 32]);
        let bpk = b.public.to_bytes();
        let mut chain = MembershipBlob::genesis(&a, hp, &[7; 32], 100);
        chain.add(&a, bpk, 101, 101, b.eggs(&bindreq_signing_bytes(&hp, &bpk, 101), scheme::MASK_BASE));
        chain.checkpoint(&a, 102, 1, [0x11; 32], 3);
        chain.lock(&a, bpk, 103);
        // A key sync right after the lock: fan-out revision 3 still wraps the locked device, so the wait is for the rotation's revision 4.
        let due = lockout_fanout_due(&FanoutSeen { revision: 3, reaches_locked: true, locked: vec![bpk] });
        assert_eq!(due, 4);
        // A routine checkpoint (k 2) between the lock and the rotation folds revision 3, the old key.
        chain.checkpoint(&a, 104, 2, [0x22; 32], 3);
        assert!(chain.fold().is_ok());
        let mut c = friend(1);
        c.is_online = true;
        c.era_lockout_due = due;
        c.era_lockout_from = 0xA1;
        let me = [7u8; 32];
        assert_eq!(folded_fanout_revision(&chain, 2), Some(3));
        assert!(!lockout_rekey_ready(&c, folded_fanout_revision(&chain, 2), Some(me)), "the routine checkpoint after the lock folds a key the locked device holds");
        // The rotation mints revision 4 without the locked device, and the checkpoint after it (k 3) folds it.
        chain.checkpoint(&a, 105, 3, [0x33; 32], 4);
        assert!(lockout_rekey_ready(&c, folded_fanout_revision(&chain, 3), Some(me)), "the first epoch folding the rotation's key");
        // A device that learns of the lock only after the rotation sees a fan-out that no longer reaches it: its wait is that revision, and an epoch already folding it re-keys at once.
        let late = lockout_fanout_due(&FanoutSeen { revision: 4, reaches_locked: false, locked: vec![bpk] });
        assert_eq!(late, 4);
        assert!(lockout_wait_over(late, folded_fanout_revision(&chain, 3)));
        assert!(!lockout_wait_over(LOCKOUT_FANOUT_UNKNOWN, Some(9)), "nothing goes before a key sync has said where the fan-out stands");
        assert!(lockout_wait_over(1, None), "a device that never held the fleet key: at once");
    }

    /// The grow gap, 2026-11-18: a grow between the lock and the rotation rewraps the fold minus the locked under the OLD key, so the counts read equal; the lock's debt still forces the mint, the key sync reports the fan-out as still reaching the locked device, and the re-key waits for the mint's revision, not the grow's.
    #[test]
    fn a_grow_between_the_lock_and_the_rotation_still_mints_and_the_rekey_waits_for_it() {
        let old_key = [0x0Au8; 32];
        let new_key = [0x0Bu8; 32];
        let k0 = fgtw::fanout::fleet_key_fingerprint(&old_key);
        let k1 = fgtw::fanout::fleet_key_fingerprint(&new_key);
        // Three members, one locked: two wanted. Before the grow the old fan-out wraps all three, and the count alone mints.
        assert!(rotation_due(3, 2, Some(k0), k0));
        // A grow at revision 6 rewraps the two under the old key: the count reads equal, and the count alone would never mint.
        assert!(!fgtw::fanout::fanout_needs_rotation(2, 2), "the gap: the count test sees nothing to rotate");
        let reaches = rotation_due(2, 2, Some(k0), k0);
        assert!(reaches, "the lock's debt forces the mint under the old key");
        let due = lockout_fanout_due(&FanoutSeen { revision: 6, reaches_locked: reaches, locked: vec![[2; 32]] });
        assert_eq!(due, 7, "the re-key waits for the mint after the grow");
        let mut c = friend(1);
        c.is_online = true;
        c.era_lockout_due = due;
        c.era_lockout_from = 0xA1;
        assert!(!lockout_rekey_ready(&c, Some(6), Some([7u8; 32])), "an epoch folding the grow is under the old key");
        // The mint at revision 7 is under a new key: the debt is paid, nothing mints again, and the re-key goes once an epoch folds it.
        assert!(!rotation_due(2, 2, Some(k0), k1));
        assert!(lockout_rekey_ready(&c, Some(7), Some([7u8; 32])));
        // Every other edge keeps the count test alone.
        assert!(!rotation_due(2, 2, None, k0));
        assert!(rotation_due(3, 2, None, k0));
    }

    /// One at a time and only when it can go: due, online, complete, nothing in flight, and ours to run; done once the bond moved off the armed era.
    #[test]
    fn a_lockout_rekey_goes_only_to_an_online_friend_this_device_owns() {
        let me = [7u8; 32];
        let mut c = friend(1);
        c.era_lockout_due = 1;
        c.era_lockout_from = 0xA1;
        assert!(!lockout_rekey_ready(&c, None, Some(me)), "offline: waits for its online edge");
        c.is_online = true;
        assert!(lockout_rekey_ready(&c, None, Some(me)));
        c.ceremony_owner = Some([6u8; 32]);
        assert!(!lockout_rekey_ready(&c, None, Some(me)), "another device of ours owns this friendship's ceremonies");
        c.ceremony_owner = Some(me);
        c.era_lockout_due = 5;
        assert!(!lockout_rekey_ready(&c, Some(4), Some(me)), "a member's lockout waits for the fan-out without it");
        assert!(lockout_rekey_ready(&c, Some(5), Some(me)));
        c.era_weave_due = 1;
        assert!(!lockout_rekey_ready(&c, Some(5), Some(me)), "a woven CLUTCH already owed carries it");
        assert!(!lockout_rekey_done(&c, Some(0xA1)));
        assert!(!lockout_rekey_done(&c, None));
        assert!(lockout_rekey_done(&c, Some(0xA2)));
    }
}
