//! The era ratchet's decision function (docs/lanes.md "Eras", plan logical-brewing-creek §5): every trigger that used to start its own destructive re-key routes here, and the verdict set has NO destructive member by construction.
//! Stage 2 wires the two observations that need no new crypto — a peer's advertised era differing from ours, and the fleet answering an era_pull with a miss. Verdicts that need the light ratchet, the woven CLUTCH or consent (stages 3-5) are returned but not yet acted on: they log, and nothing else.

use super::PhotonApp;

/// What was observed. Each variant names its evidence; none of them is "a timer fired".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RepairTrigger {
    /// A peer's pong advertised an era for this friendship that is not ours.
    StaleEraObserved { peer_index: u64, peer_tag: u32 },
    /// Every live sibling answered an era_pull with a miss: no newer era exists in the fleet.
    FleetAllMissed,
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
    /// Stage 3: propose an in-band light ratchet (owner only, era-capable peer only).
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
            PeerEra::Foreign => {
                if i.we_own { ConsentFresh } else { Hold("not the era owner") }
            }
        },
        RepairTrigger::FleetAllMissed => {
            if i.we_own { mint(i.peer_era_capable) } else { Hold("not the era owner") }
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

impl PhotonApp {
    /// Build the input, decide, log one line, and act on the verdicts stage 2 can act on. The rest log their intent and nothing else — never a destructive fallback.
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
        // Ownership stays the advisory ceremony_owner until the computed owner lands (stage 3). Unclaimed counts as ours, matching the park predicate.
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
            RepairVerdict::LightRatchet => crate::logf!("ERA: {} wants a light ratchet — not built yet (stage 3); nothing done", fp),
            RepairVerdict::HeavyWeave => crate::logf!("ERA: {} wants a woven full CLUTCH — not built yet (stage 4); nothing done", fp),
            RepairVerdict::ConsentFresh => crate::logf!("ERA: {} is on a channel we cannot order against ours — the consent-gated fresh channel is not built yet (stage 5); nothing done", fp),
        }
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
                        if peer == PeerEra::Foreign && own {
                            assert_eq!(v, RepairVerdict::ConsentFresh);
                        }
                    }
                }
            }
        }
        assert_eq!(friendship_repair(&input(RepairTrigger::FleetAllMissed, PeerEra::Ahead, SiblingVerdict::AllMissed, true, true)), RepairVerdict::LightRatchet);
        assert!(matches!(friendship_repair(&input(RepairTrigger::FleetAllMissed, PeerEra::Ahead, SiblingVerdict::AllMissed, false, true)), RepairVerdict::Hold(_)));
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
}
