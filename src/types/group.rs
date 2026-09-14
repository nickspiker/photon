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

/// A member seen thru the group's SECOND trust source (§2 "transport trust, scoped"): party id → folded devices, consulted ONLY for frames carrying that group's token. Deliberately NOT a Contact — no presence standing, no address-book row; a non-friend member's pings still drop, membership grants nothing outside the group.
#[derive(Clone, Debug, Default)]
pub struct GroupPeer {
    pub party: PartyId,
    /// The proof the fold verifies their public membership chain under, and the registry key for addressing.
    pub handle_proof: [u8; 32],
    /// Folded from the public membership chain — never trusted from a roster record.
    pub devices: Vec<[u8; 32]>,
    /// The fold's tip eagle time — the same freshness anchor a Contact's fleet_members_ts carries, so the ftip tripwire generalizes.
    pub fold_ts: i64,
}

impl GroupPeer {
    /// The group-scoped device gate — the `knows_device` of this trust source.
    pub fn knows_device(&self, dev: &[u8; 32]) -> bool {
        self.devices.iter().any(|d| d == dev)
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

/// The group control-row grammar (§4, the `EraSignal` shape): a hidden text row `GROUP_PREFIX kind ‖ ␂-separated fields`, with the record payload — a roster-codec VSF blob — riding the message package's typed `gpl` field beside it, never inside the text. Small fixed-width values (the invite's era-pinned secrets) ride the text hex-encoded: the row travels sealed inside the pairwise braid, so the text IS end-to-end encrypted; hex is framing, not protection.
#[derive(Clone, Debug, PartialEq)]
pub enum GroupSignal {
    /// The sponsor's pairwise invite (§4 Invite): era-pinned secrets + lineage in the text, the roster snapshot in `gpl` so the invitee sees who is in it before consenting. Refreshed by the sponsor on every era mint (§8b); consent IS the invitee's member record, posted into the group as its first frame.
    Invite {
        era_index: u64,
        group_root: [u8; 32],
        group_history_key: [u8; 32],
        era_lineage: [u8; 32],
    },
    /// Records posted INSIDE the group — genesis, member, leave, in any mix — as a roster-codec blob in `gpl`. History re-serve REBUILDS the blob from the roster at serve time (records never delete, so the roster always holds them — the plaid-fill doctrine: derived bytes are reconstructed, never archived twice).
    Records,
}

impl GroupSignal {
    pub fn to_content(&self) -> String {
        match self {
            GroupSignal::Invite { era_index, group_root, group_history_key, era_lineage } => format!(
                "{}invite\u{2}{}\u{2}{}\u{2}{}\u{2}{}",
                GROUP_PREFIX,
                era_index,
                hex::encode(group_root),
                hex::encode(group_history_key),
                hex::encode(era_lineage)
            ),
            GroupSignal::Records => format!("{}records", GROUP_PREFIX),
        }
    }

    pub fn parse(content: &str) -> Option<GroupSignal> {
        let rest = content.strip_prefix(GROUP_PREFIX)?;
        let mut parts = rest.split('\u{2}');
        match parts.next()? {
            "invite" => {
                let era_index: u64 = parts.next()?.parse().ok()?;
                let dec32 = |s: &str| -> Option<[u8; 32]> {
                    let v = hex::decode(s).ok()?;
                    <[u8; 32]>::try_from(v.as_slice()).ok()
                };
                Some(GroupSignal::Invite {
                    era_index,
                    group_root: dec32(parts.next()?)?,
                    group_history_key: dec32(parts.next()?)?,
                    era_lineage: dec32(parts.next()?)?,
                })
            }
            "records" => Some(GroupSignal::Records),
            _ => None,
        }
    }
}

/// Mint a member record for a JOIN (§4: consent IS this record, posted into the group as the joiner's first frame) or a re-grant (name/avatar change re-signs at a newer stamp — newest-wins in the merge).
pub fn join_record(party: PartyId, handle_proof: [u8; 32], name_grant: &str, avatar_pin: [u8; 32], sponsor: PartyId, device_seed: &[u8; 32]) -> MemberRecord {
    let mut rec = MemberRecord {
        party,
        handle_proof,
        name: name_grant.to_string(),
        avatar_pin,
        signed_osc: vsf::eagle_time_oscillations(),
        sponsor,
        signature: [0u8; 64],
        signer_device: device_pubkey(device_seed),
    };
    rec.signature = sign_record(&rec.signing_bytes(), device_seed);
    rec
}

/// Everything a founder mints at birth (§4 Genesis): the id, the era-0 secrets, the signed birth certificate, and the founder's own member record (the founder sponsors itself). The caller builds `FriendshipChains::from_group_root` with the secrets, persists the roster, and posts both records as the group's first control rows. The secrets are here transiently — they live on in the chains blob, nowhere else.
pub struct GroupBirth {
    pub group_id: GroupId,
    pub group_root: [u8; 32],
    pub group_history_key: [u8; 32],
    pub era_lineage: [u8; 32],
    pub genesis: GenesisRecord,
    pub founder_member: MemberRecord,
}

/// Found a group: mint the nonce, the era-0 root and history key (fresh randomness — no ceremony, nothing derived from any friendship), and sign genesis + the founder's member record with this device's key.
pub fn found_group(founder: PartyId, founder_proof: [u8; 32], name_grant: &str, avatar_pin: [u8; 32], title: &str, history_from_genesis: bool, device_seed: &[u8; 32]) -> GroupBirth {
    use rand::RngCore;
    let mut nonce = [0u8; 32];
    let mut group_root = [0u8; 32];
    let mut group_history_key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut nonce);
    rand::thread_rng().fill_bytes(&mut group_root);
    rand::thread_rng().fill_bytes(&mut group_history_key);
    let group_id = GroupId::from_nonce(&nonce);
    let signer_device = device_pubkey(device_seed);
    let now = vsf::eagle_time_oscillations();
    let mut genesis = GenesisRecord {
        group_id,
        founder,
        genesis_osc: now,
        history_from_genesis,
        title: title.to_string(),
        signature: [0u8; 64],
        signer_device,
    };
    genesis.signature = sign_record(&genesis.signing_bytes(), device_seed);
    let mut founder_member = MemberRecord {
        party: founder,
        handle_proof: founder_proof,
        name: name_grant.to_string(),
        avatar_pin,
        signed_osc: now,
        sponsor: founder,
        signature: [0u8; 64],
        signer_device,
    };
    founder_member.signature = sign_record(&founder_member.signing_bytes(), device_seed);
    GroupBirth {
        group_id,
        group_root,
        group_history_key,
        era_lineage: crate::crypto::clutch::era_lineage(&group_root),
        genesis,
        founder_member,
    }
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

    /// Writer discipline in a GROUP (§3, the step-1 unit): two member devices holding only the delivered root agree on any lane from its label alone, stay in lockstep thru an advance, and each writes its OWN lane — the same one-writer-per-lane invariant a friendship holds, off a root no ceremony minted.
    #[test]
    fn writer_discipline_two_members_one_root() {
        use crate::types::friendship::FriendshipChains;
        let gid = GroupId::from_nonce(&[9u8; 32]);
        let root = [0x33u8; 32];
        let hk = [0x44u8; 32];
        let lineage = crate::crypto::clutch::era_lineage(&root);
        let members = [[1u8; 32], [2u8; 32], [3u8; 32]];
        let mut a = FriendshipChains::from_group_root(gid, &members, root, hk, 0, lineage);
        let mut b = FriendshipChains::from_group_root(gid, &members, root, hk, 0, lineage);
        assert!(a.group);
        assert_eq!(a.conversation_token, gid.token());
        assert_eq!(a.id().as_bytes(), &gid.0, "the conversation id IS the group id");
        let a_label = a.mint_our_lane().expect("root present — the lane mints");
        b.ensure_lane(&a_label).expect("the label alone derives the lane on any member device");
        assert_eq!(a.current_key(&a_label), b.current_key(&a_label), "receive-anywhere off the group root");
        let et = vsf::EagleTime::from_oscillations(vsf::eagle_time_oscillations());
        a.advance(&a_label, &et, &[1u8; 16], &[]);
        b.advance(&a_label, &et, &[1u8; 16], &[]);
        assert_eq!(a.current_key(&a_label), b.current_key(&a_label), "advance is a pure function of the row — writer and reader stay in lockstep");
        let b_label = b.mint_our_lane().expect("b mints its own");
        assert_ne!(a_label, b_label, "one writer per lane: b's sends never touch a's ratchet");
    }

    /// The control-row grammar round-trips and stays hidden; a bogus kind is None, never a panic.
    #[test]
    fn group_signals_round_trip_and_are_control() {
        let inv = GroupSignal::Invite { era_index: 3, group_root: [1; 32], group_history_key: [2; 32], era_lineage: [3; 32] };
        assert_eq!(GroupSignal::parse(&inv.to_content()), Some(inv.clone()));
        assert_eq!(GroupSignal::parse(&GroupSignal::Records.to_content()), Some(GroupSignal::Records));
        assert!(crate::types::is_control_content(&inv.to_content()));
        assert!(crate::types::is_control_content(&GroupSignal::Records.to_content()));
        assert_eq!(GroupSignal::parse(&format!("{}bogus\u{2}1", GROUP_PREFIX)), None);
        assert_eq!(GroupSignal::parse("plain text"), None);
    }

    /// The full pairwise-invite arc (§4): founder mints, the invite signal + roster snapshot travel the braid, the invitee adopts — same token, same lanes off the delivered root — and its consent record merges both sides to the same standing set.
    #[test]
    fn invite_arc_founder_to_joiner() {
        use crate::types::friendship::FriendshipChains;
        let f_seed = [0x01u8; 32];
        let j_seed = [0x02u8; 32];
        let founder_party = [0xF0u8; 32];
        let joiner_party = [0x10u8; 32];
        let birth = found_group(founder_party, [0xF1; 32], "founder", [0; 32], "turtles", false, &f_seed);
        let mut f_roster = Roster::default();
        f_roster.merge_genesis(birth.genesis.clone());
        f_roster.merge_member(birth.founder_member.clone());
        let mut f_chains = FriendshipChains::from_group_root(birth.group_id, &f_roster.standing(), birth.group_root, birth.group_history_key, 0, birth.era_lineage);

        // The wire: the signal text + the roster snapshot blob (rides the package's gpl field).
        let sig = GroupSignal::Invite { era_index: 0, group_root: birth.group_root, group_history_key: birth.group_history_key, era_lineage: birth.era_lineage };
        let content = sig.to_content();
        let blob = crate::storage::group::roster_to_vsf_bytes(&birth.group_id, &f_roster).expect("snapshot");

        // The invitee's side: parse, inspect who is in it, verify the birth, consent.
        let Some(GroupSignal::Invite { era_index, group_root, group_history_key, era_lineage }) = GroupSignal::parse(&content) else {
            panic!("invite parses");
        };
        let (gid, mut j_roster) = crate::storage::group::roster_from_vsf_bytes(&blob).expect("snapshot decodes");
        assert_eq!(gid, birth.group_id);
        let g = j_roster.genesis.as_ref().expect("the invitee sees the birth certificate");
        assert!(verify_record(&g.signing_bytes(), &g.signature, &g.signer_device));
        let mut j_chains = FriendshipChains::from_group_root(gid, &j_roster.standing(), group_root, group_history_key, era_index, era_lineage);
        assert_eq!(j_chains.conversation_token, f_chains.conversation_token, "one token, minted from the id");

        // The founder speaks; the joiner derives the lane from the label alone.
        let f_lane = f_chains.mint_our_lane().expect("founder's lane");
        j_chains.ensure_lane(&f_lane).expect("derived from root ‖ label");
        assert_eq!(f_chains.current_key(&f_lane), j_chains.current_key(&f_lane));

        // Consent IS the member record (the joiner's first frame); both rosters converge.
        let rec = join_record(joiner_party, [0x11; 32], "cousin", [0; 32], founder_party, &j_seed);
        assert!(verify_record(&rec.signing_bytes(), &rec.signature, &rec.signer_device));
        assert!(j_roster.merge_member(rec.clone()));
        assert!(f_roster.merge_member(rec));
        assert_eq!(f_roster.standing(), j_roster.standing());
        assert_eq!(f_roster.standing().len(), 2);
    }
}
