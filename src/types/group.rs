//! Groups substrate (docs/groups.md): the stable id, the token, the roster of sovereign records and their merge, and the control-row grammar. No wire, no chains, no UI in this module — it is the vocabulary the later steps speak.
//!
//! A group is a conversation with a mutable member set and its own root secret; membership is signed records merged by union with subject-signed newest-wins per party (§4). Records ride inside the group as hidden control rows (`GROUP_PREFIX`, the `ERA_PREFIX` pattern), so they are history, re-servable, and fleet-replicated with no new plane.

use crate::types::PartyId;

/// Hidden control-row prefix for every roster record (the `ERA_PREFIX` shape — `is_control_content` hides it from every UI and digest).
pub const GROUP_PREFIX: &str = "\u{1}\u{2}photon-group\u{2}\u{1}";

/// A group's stable identity: 32 random bytes minted by the founder — deliberately NOT participant-derived, because the member set moves and the conversation must not (§2 D2).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct GroupId(pub [u8; 32]);

impl GroupId {
    /// Mint a fresh group id: `blake3("PHOTON_GROUP_v1" ‖ genesis_nonce)` over 32 random bytes.
    pub fn mint() -> Self {
        let nonce: [u8; 32] = rand::random();
        Self::from_nonce(&nonce)
    }
    /// The deterministic half of the mint, split out so tests can pin vectors.
    pub fn from_nonce(genesis_nonce: &[u8; 32]) -> Self {
        let mut h = blake3::Hasher::new();
        h.update(b"PHOTON_GROUP_v1");
        h.update(genesis_nonce);
        GroupId(*h.finalize().as_bytes())
    }
    /// The routing tag on every group frame (the `conversation_token` role): only members hold `group_id`, so only members can mint or match it. Stable across eras.
    pub fn token(&self) -> [u8; 32] {
        *blake3::Hasher::new_derive_key("photon.group.token.v1").update(&self.0).finalize().as_bytes()
    }
}

/// FOUNDER-SCOPED group proof (settled 2026-09-14): the group's registry face, unique PER FOUNDER — global squatting is impossible by construction ("I can't have three purple turtle groups but you could have one too"). Domain-separated from every personal proof so the knock router always knows what kind of thing it is routing. v1 registers nothing; this is the derivation the attach layer will use.
pub fn founder_scoped_proof(founder_proof: &[u8; 32], name: &str) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(b"PHOTON_GROUP_v1");
    h.update(founder_proof);
    h.update(name.as_bytes());
    *h.finalize().as_bytes()
}

/// One member's standing in the group — the subject-signed sovereign record (§4 Member). The device list is NEVER trusted from here: everyone folds the member's devices from the public membership chain under `handle_proof`.
#[derive(Clone, Debug, PartialEq)]
pub struct MemberRecord {
    pub party: PartyId,
    /// The member's handle proof — what the fold verifies their membership chain under, and what the registry is queried by for addressing (§8b).
    pub handle_proof: [u8; 32],
    /// The member's name grant, scoped to this group (zero trust; the pinned key is the trust).
    pub name: String,
    /// Avatar pin, scoped grant like the name; zeroes = none.
    pub avatar_pin: [u8; 32],
    /// When this record was signed — newest-wins per party in the merge.
    pub signed_osc: i64,
    /// Who sponsored this member in (the founder sponsors itself in genesis).
    pub sponsor: PartyId,
    /// Ed25519 by one of the member's own devices over [`Self::signing_bytes`]; verified at ingress against the folded device set.
    pub signature: [u8; 64],
    /// Which device signed (so verification is one lookup, not a trial over the fold).
    pub signer_device: [u8; 32],
}

impl MemberRecord {
    /// The canonical signed bytes: every field in fixed order, signature omitted. One derivation for sign and verify so the two can never disagree.
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(32 * 4 + 8 + self.name.len() + 16);
        b.extend_from_slice(b"PHOTON_GROUP_MEMBER_v1");
        b.extend_from_slice(&self.party);
        b.extend_from_slice(&self.handle_proof);
        b.extend_from_slice(&(self.name.len() as u32).to_le_bytes());
        b.extend_from_slice(self.name.as_bytes());
        b.extend_from_slice(&self.avatar_pin);
        b.extend_from_slice(&self.signed_osc.to_le_bytes());
        b.extend_from_slice(&self.sponsor);
        b.extend_from_slice(&self.signer_device);
        b
    }
}

/// A leave — the leaver's signed request (§4 Leave). The first survivor to observe it countersigns and mints the next era; the record itself stays as testimony (ostracism never erasure).
#[derive(Clone, Debug, PartialEq)]
pub struct LeaveRecord {
    pub party: PartyId,
    pub signed_osc: i64,
    pub signature: [u8; 64],
    pub signer_device: [u8; 32],
}

impl LeaveRecord {
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(32 * 2 + 8 + 24);
        b.extend_from_slice(b"PHOTON_GROUP_LEAVE_v1");
        b.extend_from_slice(&self.party);
        b.extend_from_slice(&self.signed_osc.to_le_bytes());
        b.extend_from_slice(&self.signer_device);
        b
    }
}

/// Genesis — founder-signed birth certificate (§4 Genesis). The newcomer-history policy is fixed at birth; from-join additionally mints an era at every join (§8b).
#[derive(Clone, Debug, PartialEq)]
pub struct GenesisRecord {
    pub group_id: GroupId,
    pub founder: PartyId,
    pub genesis_osc: i64,
    /// D5: true = newcomers may be served history from genesis (the family group); false = from join, and a join mints an era.
    pub history_from_genesis: bool,
    /// The founder's suggested title — a default for local petnames, zero trust.
    pub title: String,
    pub signature: [u8; 64],
    pub signer_device: [u8; 32],
}

impl GenesisRecord {
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(32 * 3 + 16 + self.title.len());
        b.extend_from_slice(b"PHOTON_GROUP_GENESIS_v1");
        b.extend_from_slice(&self.group_id.0);
        b.extend_from_slice(&self.founder);
        b.extend_from_slice(&self.genesis_osc.to_le_bytes());
        b.push(self.history_from_genesis as u8);
        b.extend_from_slice(&(self.title.len() as u32).to_le_bytes());
        b.extend_from_slice(self.title.as_bytes());
        b.extend_from_slice(&self.signer_device);
        b
    }
}

/// The roster: genesis plus the merged record sets. Merge is UNION with subject-signed newest-wins per party; a leave never deletes the member record (testimony), it changes standing.
#[derive(Clone, Debug, Default)]
pub struct Roster {
    pub genesis: Option<GenesisRecord>,
    pub members: std::collections::HashMap<PartyId, MemberRecord>,
    pub leaves: std::collections::HashMap<PartyId, LeaveRecord>,
}

impl Roster {
    /// Merge one member record in: newest `signed_osc` wins for its party; equal stamps keep the incumbent (deterministic both sides — ties can only be the same record or a same-instant re-sign, and refusing the swap keeps the merge commutative-with-itself).
    pub fn merge_member(&mut self, rec: MemberRecord) -> bool {
        match self.members.get(&rec.party) {
            Some(cur) if cur.signed_osc >= rec.signed_osc => false,
            _ => {
                self.members.insert(rec.party, rec);
                true
            }
        }
    }
    /// Merge a leave in — same newest-wins per party.
    pub fn merge_leave(&mut self, rec: LeaveRecord) -> bool {
        match self.leaves.get(&rec.party) {
            Some(cur) if cur.signed_osc >= rec.signed_osc => false,
            _ => {
                self.leaves.insert(rec.party, rec);
                true
            }
        }
    }
    /// Genesis merges once; a second genesis for the same group is refused (the founder signed exactly one birth).
    pub fn merge_genesis(&mut self, rec: GenesisRecord) -> bool {
        if self.genesis.is_some() {
            return false;
        }
        self.genesis = Some(rec);
        true
    }
    /// A party is STANDING when its member record outdates any leave it signed — a re-join is a fresh member record newer than the old leave.
    pub fn is_standing(&self, party: &PartyId) -> bool {
        match (self.members.get(party), self.leaves.get(party)) {
            (Some(m), Some(l)) => m.signed_osc > l.signed_osc,
            (Some(_), None) => true,
            _ => false,
        }
    }
    /// The standing set, sorted — what the next era wraps to, what the UI lists, what `Conversation::set_participants` mirrors.
    pub fn standing(&self) -> Vec<PartyId> {
        let mut v: Vec<PartyId> = self.members.keys().filter(|p| self.is_standing(p)).copied().collect();
        v.sort_unstable();
        v
    }
}

/// The device pubkey a seed signs as — fill `signer_device` with this BEFORE building signing bytes (the device is part of what's signed, so it must be in place first).
pub fn device_pubkey(device_seed: &[u8; 32]) -> [u8; 32] {
    ed25519_dalek::SigningKey::from_bytes(device_seed).verifying_key().to_bytes()
}

/// Sign a record's canonical bytes with a device key — one helper so every record signs identically. The bytes must already carry the signer's device pubkey.
pub fn sign_record(signing_bytes: &[u8], device_seed: &[u8; 32]) -> [u8; 64] {
    use ed25519_dalek::Signer;
    let kp = ed25519_dalek::SigningKey::from_bytes(device_seed);
    kp.sign(&blake3::hash(signing_bytes).as_bytes()[..]).to_bytes()
}

/// Verify a record signature against one claimed device pubkey. The CALLER is responsible for checking that device sits in the subject's folded device set — this only proves the bytes were signed by that key.
pub fn verify_record(signing_bytes: &[u8], signature: &[u8; 64], signer_device: &[u8; 32]) -> bool {
    use ed25519_dalek::Verifier;
    let Ok(vk) = ed25519_dalek::VerifyingKey::from_bytes(signer_device) else {
        return false;
    };
    let sig = ed25519_dalek::Signature::from_bytes(signature);
    vk.verify(&blake3::hash(signing_bytes).as_bytes()[..], &sig).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(party: u8, osc: i64) -> MemberRecord {
        MemberRecord {
            party: [party; 32],
            handle_proof: [0x11; 32],
            name: format!("m{party}"),
            avatar_pin: [0; 32],
            signed_osc: osc,
            sponsor: [1; 32],
            signature: [0; 64],
            signer_device: [0; 32],
        }
    }

    #[test]
    fn group_id_is_stable_and_token_derives() {
        let a = GroupId::from_nonce(&[7; 32]);
        let b = GroupId::from_nonce(&[7; 32]);
        assert_eq!(a, b);
        assert_ne!(a.0, a.token(), "the token is a derivation, never the id itself");
        assert_ne!(GroupId::from_nonce(&[8; 32]), a);
    }

    #[test]
    fn founder_scoping_separates_namespaces() {
        let nick = founder_scoped_proof(&[1; 32], "purple turtle");
        let other = founder_scoped_proof(&[2; 32], "purple turtle");
        assert_ne!(nick, other, "same name, different founders, different proofs");
        assert_ne!(nick, founder_scoped_proof(&[1; 32], "purple turtles"));
    }

    #[test]
    fn roster_merge_is_union_newest_wins_and_standing_tracks_leaves() {
        let mut r = Roster::default();
        assert!(r.merge_member(rec(2, 100)));
        assert!(!r.merge_member(rec(2, 90)), "older record must lose");
        assert!(r.merge_member(rec(3, 50)));
        assert_eq!(r.standing().len(), 2);
        // A leave newer than the member record ends standing; the records both remain (testimony).
        assert!(r.merge_leave(LeaveRecord { party: [3; 32], signed_osc: 60, signature: [0; 64], signer_device: [0; 32] }));
        assert!(!r.is_standing(&[3; 32]));
        assert_eq!(r.standing(), vec![[2u8; 32]]);
        assert!(r.members.contains_key(&[3; 32]), "ostracism, never erasure");
        // A re-join is a fresh member record newer than the leave.
        assert!(r.merge_member(rec(3, 70)));
        assert!(r.is_standing(&[3; 32]));
        // Merge is idempotent and order-free for distinct parties: replay changes nothing.
        let snapshot = r.standing();
        assert!(!r.merge_member(rec(2, 100)));
        assert_eq!(r.standing(), snapshot);
    }

    #[test]
    fn records_sign_and_verify_and_tampering_fails() {
        let seed = [9u8; 32];
        let mut m = rec(5, 1000);
        m.signer_device = device_pubkey(&seed);
        m.signature = sign_record(&m.signing_bytes(), &seed);
        assert!(verify_record(&m.signing_bytes(), &m.signature, &m.signer_device));
        let mut tampered = m.clone();
        tampered.name = "impostor".into();
        assert!(!verify_record(&tampered.signing_bytes(), &m.signature, &m.signer_device));
        let mut wrong_dev = m.clone();
        wrong_dev.signer_device = [3; 32];
        assert!(!verify_record(&wrong_dev.signing_bytes(), &wrong_dev.signature, &wrong_dev.signer_device));
    }

    #[test]
    fn lanes_derive_from_a_group_root_exactly_as_a_friendship() {
        // The doc's central reuse claim (§3): derive_lane_active is root-agnostic — a group root produces a full lane keystream just as a friendship root does, and distinct labels give distinct lanes.
        let group_root = [0x42u8; 32];
        let a = crate::crypto::clutch::derive_lane_active(&group_root, &[1; 32]);
        let b = crate::crypto::clutch::derive_lane_active(&group_root, &[2; 32]);
        assert_eq!(a.len(), 8192);
        assert_ne!(a, b);
        assert_eq!(a, crate::crypto::clutch::derive_lane_active(&group_root, &[1; 32]));
    }
}
