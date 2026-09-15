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
    /// Mint a fresh group id: `blake3("PHOTON_GROUP_v" ‖ 0x01 ‖ genesis_nonce)` over 32 random bytes (the version is a binary numeral after the text — the numbers-binary-at-rest doctrine).
    pub fn mint() -> Self {
        let nonce: [u8; 32] = rand::random();
        Self::from_nonce(&nonce)
    }
    /// The deterministic half of the mint, split out so tests can pin vectors.
    pub fn from_nonce(genesis_nonce: &[u8; 32]) -> Self {
        let mut h = blake3::Hasher::new();
        h.update(b"PHOTON_GROUP_v\x01");
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
    h.update(b"PHOTON_GROUP_v\x01");
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
        b.extend_from_slice(b"PHOTON_GROUP_MEMBER_v\x01");
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
        b.extend_from_slice(b"PHOTON_GROUP_LEAVE_v\x01");
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
        b.extend_from_slice(b"PHOTON_GROUP_GENESIS_v\x01");
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

/// The group's TITLE — one more roster record (§10, D7): any standing member signs a new one, newest wins, the genesis suggestion seeds the first. A shared petname, zero trust.
#[derive(Clone, Debug, PartialEq)]
pub struct TitleRecord {
    pub party: PartyId,
    pub title: String,
    pub signed_osc: i64,
    pub signature: [u8; 64],
    pub signer_device: [u8; 32],
}

impl TitleRecord {
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(32 * 2 + 8 + 4 + self.title.len() + 24);
        b.extend_from_slice(b"PHOTON_GROUP_TITLE_v\x01");
        b.extend_from_slice(&self.party);
        b.extend_from_slice(&(self.title.len() as u32).to_le_bytes());
        b.extend_from_slice(self.title.as_bytes());
        b.extend_from_slice(&self.signed_osc.to_le_bytes());
        b.extend_from_slice(&self.signer_device);
        b
    }
}

/// One DEVICE's published KEM bundle (§3 Eras, D11): the public half of the hybrid keypair every era secret is wrapped to. Signed by the device that owns the decapsulation keys, newest wins per (party, device); a device publishes a fresh one on its first frame of each era. The private half is `EraDecapKeys` in that device's chains blob, nowhere else.
#[derive(Clone, Debug, PartialEq)]
pub struct BundleRecord {
    pub party: PartyId,
    pub device: [u8; 32],
    /// The era the device was IN when it published — the minter always wraps to a device's newest bundle whatever index it mints.
    pub published_era: u64,
    pub kem_set: u8,
    pub mlkem_pk: Vec<u8>,
    pub x_pk: Vec<u8>,
    pub hqc_pk: Vec<u8>,
    pub signed_osc: i64,
    pub signature: [u8; 64],
}

impl BundleRecord {
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(32 * 2 + 8 + 1 + 12 + self.mlkem_pk.len() + self.x_pk.len() + self.hqc_pk.len() + 8 + 24);
        b.extend_from_slice(b"PHOTON_GROUP_BUNDLE_v\x01");
        b.extend_from_slice(&self.party);
        b.extend_from_slice(&self.device);
        b.extend_from_slice(&self.published_era.to_le_bytes());
        b.push(self.kem_set);
        for k in [&self.mlkem_pk, &self.x_pk, &self.hqc_pk] {
            b.extend_from_slice(&(k.len() as u32).to_le_bytes());
            b.extend_from_slice(k);
        }
        b.extend_from_slice(&self.signed_osc.to_le_bytes());
        b
    }
    /// The public keys as the era-KEM wire shape the wrap helpers take.
    pub fn pubkeys(&self) -> crate::crypto::era::EraKemWire {
        crate::crypto::era::EraKemWire { mlkem: self.mlkem_pk.clone(), x25519: self.x_pk.clone(), hqc: self.hqc_pk.clone() }
    }
}

/// A VOUCH — the revocation model (the A5 branch, settled 2026-09-15): "I, voucher, want subject in." Any standing member may vouch for anyone; a voucher may withdraw only its OWN vouch (a newer record with `withdrawn = true`); LWW per (voucher, subject). Standing is DERIVED from vouches, so nobody ever edits another's standing — the people who wanted you in change their minds. The founder's genesis self-vouch is the only self-vouch that counts.
#[derive(Clone, Debug, PartialEq)]
pub struct VouchRecord {
    pub voucher: PartyId,
    pub subject: PartyId,
    pub signed_osc: i64,
    pub withdrawn: bool,
    pub signature: [u8; 64],
    pub signer_device: [u8; 32],
}

impl VouchRecord {
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(32 * 3 + 8 + 1 + 24);
        b.extend_from_slice(b"PHOTON_GROUP_VOUCH_v\x01");
        b.extend_from_slice(&self.voucher);
        b.extend_from_slice(&self.subject);
        b.extend_from_slice(&self.signed_osc.to_le_bytes());
        b.push(self.withdrawn as u8);
        b.extend_from_slice(&self.signer_device);
        b
    }
}

/// The roster: genesis plus the merged record sets. Merge is UNION with subject-signed newest-wins per key; a leave never deletes the member record (testimony), it changes standing.
#[derive(Clone, Debug, Default)]
pub struct Roster {
    pub genesis: Option<GenesisRecord>,
    pub members: std::collections::HashMap<PartyId, MemberRecord>,
    pub leaves: std::collections::HashMap<PartyId, LeaveRecord>,
    pub title: Option<TitleRecord>,
    /// Keyed by (party, device) — every device of every member publishes its own.
    pub bundles: std::collections::HashMap<(PartyId, [u8; 32]), BundleRecord>,
    /// Keyed by (voucher, subject): EVERY record ever merged, newest last (testimony; a departed voucher's late withdrawal must not erase its earlier vouch, so nothing is replaced).
    pub vouches: std::collections::HashMap<(PartyId, PartyId), Vec<VouchRecord>>,
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
    /// Title: newest wins, whoever signed it.
    pub fn merge_title(&mut self, rec: TitleRecord) -> bool {
        match self.title.as_ref() {
            Some(cur) if cur.signed_osc >= rec.signed_osc => false,
            _ => {
                self.title = Some(rec);
                true
            }
        }
    }
    /// Bundle: newest wins per (party, device).
    pub fn merge_bundle(&mut self, rec: BundleRecord) -> bool {
        let key = (rec.party, rec.device);
        match self.bundles.get(&key) {
            Some(cur) if cur.signed_osc >= rec.signed_osc => false,
            _ => {
                self.bundles.insert(key, rec);
                true
            }
        }
    }
    /// Vouch: every distinct record (by stamp) is kept per (voucher, subject); standing reads the newest one that COUNTS. A self-vouch is refused unless it is the founder's (the genesis self-vouch is the root of every standing chain).
    pub fn merge_vouch(&mut self, rec: VouchRecord) -> bool {
        if rec.voucher == rec.subject && self.genesis.as_ref().map_or(true, |g| g.founder != rec.voucher) {
            return false;
        }
        let list = self.vouches.entry((rec.voucher, rec.subject)).or_default();
        if list.iter().any(|v| v.signed_osc == rec.signed_osc) {
            return false;
        }
        list.push(rec);
        list.sort_unstable_by_key(|v| v.signed_osc);
        true
    }
    /// The live vouch for (voucher, subject): the newest record that counts, if it is not a withdrawal.
    fn live_vouch(&self, voucher: &PartyId, subject: &PartyId) -> bool {
        self.vouches.get(&(*voucher, *subject)).map_or(false, |list| list.iter().rev().find(|v| self.vouch_counts(v)).map_or(false, |v| !v.withdrawn))
    }
    /// The title everyone shares: the newest title record, else the genesis suggestion, else empty.
    pub fn title(&self) -> String {
        self.title.as_ref().map(|t| t.title.clone()).or_else(|| self.genesis.as_ref().map(|g| g.title.clone())).unwrap_or_default()
    }
    /// A party has JOINED when its member record outdates any leave it signed — a re-join is a fresh member record newer than the old leave.
    pub fn has_joined(&self, party: &PartyId) -> bool {
        match (self.members.get(party), self.leaves.get(party)) {
            (Some(m), Some(l)) => m.signed_osc > l.signed_osc,
            (Some(_), None) => true,
            _ => false,
        }
    }
    /// Does a vouch record COUNT: signed by a party that had joined and had not yet left when it signed (a departed voucher's later withdrawal is ignored the same way — the record is judged against the voucher's own leave stamp, so order of arrival never matters).
    fn vouch_counts(&self, v: &VouchRecord) -> bool {
        if !self.members.contains_key(&v.voucher) {
            return false;
        }
        match self.leaves.get(&v.voucher) {
            Some(l) if v.signed_osc > l.signed_osc => {
                // Signed after the voucher's leave: counts only if the voucher re-joined before signing.
                self.members.get(&v.voucher).map_or(false, |m| m.signed_osc > l.signed_osc && v.signed_osc >= m.signed_osc)
            }
            _ => true,
        }
    }
    /// STANDING (the A5 rule): joined, not left, and at least one unwithdrawn vouch that counts. The founder stands on its genesis self-vouch.
    pub fn is_standing(&self, party: &PartyId) -> bool {
        if !self.has_joined(party) {
            return false;
        }
        self.vouches.keys().any(|(voucher, subject)| subject == party && self.live_vouch(voucher, subject))
    }
    /// The standing set, sorted — what the next era wraps to, what the UI lists, what `Conversation::set_participants` mirrors.
    pub fn standing(&self) -> Vec<PartyId> {
        let mut v: Vec<PartyId> = self.members.keys().filter(|p| self.is_standing(p)).copied().collect();
        v.sort_unstable();
        v
    }
    /// The newest published bundle for a device, if any member's device published one.
    pub fn newest_bundle(&self, device: &[u8; 32]) -> Option<&BundleRecord> {
        self.bundles.values().filter(|b| b.device == *device).max_by_key(|b| b.signed_osc)
    }
    /// Every bundle belonging to a standing party — the wrap set of the next mint, one per device.
    pub fn standing_bundles(&self) -> Vec<&BundleRecord> {
        let standing = self.standing();
        let mut out: Vec<&BundleRecord> = self.bundles.values().filter(|b| standing.binary_search(&b.party).is_ok()).collect();
        out.sort_unstable_by_key(|b| (b.party, b.device));
        out
    }
    /// Merge every record of another roster in (a snapshot, a records posting, a sibling push) — order-free, so callers never sequence anything. Signatures are the caller's business (verified before this is reached). Returns whether anything changed.
    pub fn merge_all(&mut self, other: Roster) -> bool {
        let mut changed = false;
        if let Some(g) = other.genesis {
            changed |= self.merge_genesis(g);
        }
        for (_, m) in other.members {
            changed |= self.merge_member(m);
        }
        for (_, list) in other.vouches {
            for v in list {
                changed |= self.merge_vouch(v);
            }
        }
        for (_, b) in other.bundles {
            changed |= self.merge_bundle(b);
        }
        if let Some(t) = other.title {
            changed |= self.merge_title(t);
        }
        for (_, l) in other.leaves {
            changed |= self.merge_leave(l);
        }
        changed
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

/// A group weave reference: (author party id, eagle_time) — §3 The weave. Eagle times are unique per device, not per group, so the author disambiguates.
pub type StrandRef = (PartyId, i64);

/// What one offer/serve edge produced: rows now applicable (the offered row first when it applied directly, then the unpark cascade in discovery order), and the strands to PULL — addressed to the offered row's SENDER, who necessarily holds what it wove.
#[derive(Default, Debug, PartialEq)]
pub struct StrandOutcome {
    pub applied: Vec<StrandRef>,
    pub pulls: Vec<StrandRef>,
}

/// The receive-side confluence engine for a group stream (§3 The weave): a row applies when every strand it weaves is already held; a row missing one PARKS and pulls it from its own sender. Weave references point strictly backwards in causality, so the pull graph is a DAG and parking always terminates in applies, whatever the delivery order — braid §1.4's confluence promise, kept in groups. Edge-driven: state moves only on offer/serve edges, never a timer. The wiring seeds `applied` from the conversation DB at load; a served strand is just another offer.
#[derive(Default, Clone, Debug)]
pub struct StrandLedger {
    applied: std::collections::HashSet<StrandRef>,
    /// Parked rows with their unmet refs; a row parks once and unparks when its last ref lands.
    parked: Vec<(StrandRef, Vec<StrandRef>)>,
    /// Strands already asked for — one pull per missing edge; a re-park of the same gap stays quiet until served (retransmit pressure lives in the pending ledger, not here).
    requested: std::collections::HashSet<StrandRef>,
}

impl StrandLedger {
    /// Seed a strand as already held (conversation DB rows at load; our own sends).
    pub fn seed(&mut self, row: StrandRef) {
        self.applied.insert(row);
    }

    /// True when the strand has been applied.
    pub fn holds(&self, row: &StrandRef) -> bool {
        self.applied.contains(row)
    }

    /// Rows currently parked (missing strands outstanding).
    pub fn parked_count(&self) -> usize {
        self.parked.len()
    }

    /// Offer a row (fresh arrival OR a served pull — the two are one edge). Duplicate offers are no-ops.
    pub fn offer(&mut self, row: StrandRef, woven: &[StrandRef]) -> StrandOutcome {
        let mut out = StrandOutcome::default();
        if self.applied.contains(&row) {
            return out;
        }
        let missing: Vec<StrandRef> = woven.iter().filter(|r| !self.applied.contains(*r)).copied().collect();
        if missing.is_empty() {
            self.apply_cascade(row, &mut out);
            return out;
        }
        for m in &missing {
            if self.requested.insert(*m) {
                out.pulls.push(*m);
            }
        }
        if !self.parked.iter().any(|(r, _)| *r == row) {
            self.parked.push((row, missing));
        }
        out
    }

    /// Apply one row, then drain every parked row it (transitively) completes.
    fn apply_cascade(&mut self, row: StrandRef, out: &mut StrandOutcome) {
        let mut worklist = vec![row];
        while let Some(r) = worklist.pop() {
            if !self.applied.insert(r) {
                continue;
            }
            out.applied.push(r);
            self.requested.remove(&r);
            let mut i = 0;
            while i < self.parked.len() {
                self.parked[i].1.retain(|m| *m != r);
                if self.parked[i].1.is_empty() {
                    worklist.push(self.parked.swap_remove(i).0);
                } else {
                    i += 1;
                }
            }
        }
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

/// The group control-row grammar (§4, the `EraSignal` shape): a hidden text row `GROUP_PREFIX kind`, with EVERYTHING else typed on the message package beside it — roster records in `gpl` (a roster-codec blob), a wrap's KEM ciphertexts on `ekn`/`ekx`/`ekh` and its sealed secret on the typed wrap fields (`GroupWrapWire`). Nothing binary is ever encoded into the text: the text is the row, and the row persists, replicates and re-serves — a secret in it would outlive its era (D10: secrets only ever move as wraps).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GroupSignal {
    /// The sponsor's OFFER, over the friendship braid: a roster snapshot in `gpl` so the invitee sees who is in it before consenting. Carries no secret. Refreshed by the sponsor on every roster change; the invitee's consent is the Join.
    Offer,
    /// The invitee's JOIN, back over the friendship braid: its member record + its device's KEM bundle in `gpl`. The sponsor posts them into the group, vouches, and answers with a Wrap.
    Join,
    /// A WRAP: an era secret sealed to one device's published bundle — the join answer over the friendship braid (era N re-wrapped under from-genesis, N+1 under from-join), or a mint's fan-out inside the group, one row per recipient device on the old era.
    Wrap,
    /// Records posted INSIDE the group — genesis, member, vouch, bundle, title, leave, in any mix — as a roster-codec blob in `gpl`. History re-serve REBUILDS the blob from the roster at serve time (records never delete, so the roster always holds them).
    Records,
}

impl GroupSignal {
    pub fn to_content(&self) -> String {
        match self {
            GroupSignal::Offer => format!("{}offer", GROUP_PREFIX),
            GroupSignal::Join => format!("{}join", GROUP_PREFIX),
            GroupSignal::Wrap => format!("{}wrap", GROUP_PREFIX),
            GroupSignal::Records => format!("{}records", GROUP_PREFIX),
        }
    }

    pub fn parse(content: &str) -> Option<GroupSignal> {
        match content.strip_prefix(GROUP_PREFIX)? {
            "offer" => Some(GroupSignal::Offer),
            "join" => Some(GroupSignal::Join),
            "wrap" => Some(GroupSignal::Wrap),
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

/// Mint a vouch (or a withdrawal of one) signed by this device on behalf of `voucher`.
pub fn vouch_record(voucher: PartyId, subject: PartyId, withdrawn: bool, device_seed: &[u8; 32]) -> VouchRecord {
    let mut rec = VouchRecord { voucher, subject, signed_osc: vsf::eagle_time_oscillations(), withdrawn, signature: [0u8; 64], signer_device: device_pubkey(device_seed) };
    rec.signature = sign_record(&rec.signing_bytes(), device_seed);
    rec
}

/// Mint a title record signed by this device on behalf of `party`.
pub fn title_record(party: PartyId, title: &str, device_seed: &[u8; 32]) -> TitleRecord {
    let mut rec = TitleRecord { party, title: title.to_string(), signed_osc: vsf::eagle_time_oscillations(), signature: [0u8; 64], signer_device: device_pubkey(device_seed) };
    rec.signature = sign_record(&rec.signing_bytes(), device_seed);
    rec
}

/// Publish this device's KEM bundle: the public half of a fresh `era_keygen` — the caller keeps `export_decaps` in the chains blob (`push_group_kem`).
pub fn bundle_record(party: PartyId, published_era: u64, eph: &crate::crypto::era::EraEphemeral, device_seed: &[u8; 32]) -> BundleRecord {
    let mut rec = BundleRecord {
        party,
        device: device_pubkey(device_seed),
        published_era,
        kem_set: eph.kem_set,
        mlkem_pk: eph.init_wire.mlkem.clone(),
        x_pk: eph.init_wire.x25519.clone(),
        hqc_pk: eph.init_wire.hqc.clone(),
        signed_osc: vsf::eagle_time_oscillations(),
        signature: [0u8; 64],
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
    /// The founder's genesis self-vouch — the root every standing chain hangs from.
    pub founder_vouch: VouchRecord,
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
    let mut founder_vouch = VouchRecord { voucher: founder, subject: founder, signed_osc: now, withdrawn: false, signature: [0u8; 64], signer_device };
    founder_vouch.signature = sign_record(&founder_vouch.signing_bytes(), device_seed);
    GroupBirth {
        group_id,
        group_root,
        group_history_key,
        era_lineage: crate::crypto::clutch::era_lineage(&group_root),
        genesis,
        founder_member,
        founder_vouch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A roster whose founder [1;32] stands on its genesis self-vouch, so tests can vouch others in from it.
    fn founded() -> Roster {
        let mut r = Roster::default();
        r.merge_genesis(GenesisRecord { group_id: GroupId::from_nonce(&[0; 32]), founder: [1; 32], genesis_osc: 1, history_from_genesis: false, title: "t".into(), signature: [0; 64], signer_device: [0; 32] });
        r.merge_member(rec(1, 1));
        r.merge_vouch(vouch(1, 1, 1, false));
        r
    }

    fn vouch(voucher: u8, subject: u8, osc: i64, withdrawn: bool) -> VouchRecord {
        VouchRecord { voucher: [voucher; 32], subject: [subject; 32], signed_osc: osc, withdrawn, signature: [0; 64], signer_device: [0; 32] }
    }

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
        let mut r = founded();
        assert!(r.merge_member(rec(2, 100)));
        assert!(!r.merge_member(rec(2, 90)), "older record must lose");
        assert!(r.merge_member(rec(3, 50)));
        assert!(!r.is_standing(&[2; 32]), "joined but nobody vouched — not standing");
        assert!(r.merge_vouch(vouch(1, 2, 101, false)));
        assert!(r.merge_vouch(vouch(1, 3, 101, false)));
        assert_eq!(r.standing().len(), 3);
        // A leave newer than the member record ends standing; the records both remain (testimony).
        assert!(r.merge_leave(LeaveRecord { party: [3; 32], signed_osc: 60, signature: [0; 64], signer_device: [0; 32] }));
        assert!(!r.is_standing(&[3; 32]));
        assert_eq!(r.standing(), vec![[1u8; 32], [2u8; 32]]);
        assert!(r.members.contains_key(&[3; 32]), "ostracism, never erasure");
        // A re-join is a fresh member record newer than the leave; the old vouch still counts (it was never withdrawn).
        assert!(r.merge_member(rec(3, 70)));
        assert!(r.is_standing(&[3; 32]));
        // Merge is idempotent and order-free for distinct parties: replay changes nothing.
        let snapshot = r.standing();
        assert!(!r.merge_member(rec(2, 100)));
        assert_eq!(r.standing(), snapshot);
    }

    /// The A5 standing table (the revocation model, 2026-09-15): standing is derived from vouches, nobody edits another's standing, and every branch walked in the plan holds.
    #[test]
    fn standing_is_vouch_and_withdraw() {
        let mut r = founded();
        // Founder stands alone on the genesis self-vouch; a non-founder self-vouch is refused.
        assert_eq!(r.standing(), vec![[1u8; 32]]);
        r.merge_member(rec(2, 10));
        assert!(!r.merge_vouch(vouch(2, 2, 11, false)), "only the founder may self-vouch");
        assert!(!r.is_standing(&[2; 32]));
        // The founder vouches 2 in; 2 vouches 3 in.
        assert!(r.merge_vouch(vouch(1, 2, 12, false)));
        assert!(r.is_standing(&[2; 32]));
        r.merge_member(rec(3, 20));
        assert!(r.merge_vouch(vouch(2, 3, 21, false)));
        assert!(r.is_standing(&[3; 32]));
        // Withdrawal: newest wins per (voucher, subject); the sole vouch withdrawn ends standing.
        assert!(r.merge_vouch(vouch(2, 3, 30, true)));
        assert!(!r.is_standing(&[3; 32]), "the last vouch withdrawn ⇒ standing false");
        assert!(r.merge_vouch(vouch(2, 3, 25, false)), "every record is kept as testimony …");
        assert!(!r.is_standing(&[3; 32]), "… but an older re-vouch does not outrank the newer withdrawal");
        // A second voucher keeps 3 standing thru 2's withdrawal.
        assert!(r.merge_vouch(vouch(1, 3, 31, false)));
        assert!(r.is_standing(&[3; 32]));
        // 1 withdraws too ⇒ gone; anyone standing can re-vouch at once.
        assert!(r.merge_vouch(vouch(1, 3, 40, true)));
        assert!(!r.is_standing(&[3; 32]));
        assert!(r.merge_vouch(vouch(2, 3, 41, false)));
        assert!(r.is_standing(&[3; 32]));
        // A departed voucher's vouches persist; its LATER withdrawal is ignored (signed after its leave).
        r.merge_leave(LeaveRecord { party: [2; 32], signed_osc: 50, signature: [0; 64], signer_device: [0; 32] });
        assert!(!r.is_standing(&[2; 32]));
        assert!(r.is_standing(&[3; 32]), "2's vouch for 3 was signed before 2 left — it stands as testimony");
        assert!(r.merge_vouch(vouch(2, 3, 60, true)), "the record merges (newest) …");
        assert!(r.is_standing(&[3; 32]), "… but does not count: signed after the voucher's leave");
        // The founder leaving cascades nothing: 3 still stands on 2's pre-leave vouch.
        r.merge_leave(LeaveRecord { party: [1; 32], signed_osc: 70, signature: [0; 64], signer_device: [0; 32] });
        assert_eq!(r.standing(), vec![[3u8; 32]]);
        // Title: newest wins, whoever signed; genesis title until then.
        assert_eq!(r.title(), "t");
        assert!(r.merge_title(TitleRecord { party: [3; 32], title: "taco".into(), signed_osc: 80, signature: [0; 64], signer_device: [0; 32] }));
        assert!(!r.merge_title(TitleRecord { party: [1; 32], title: "old".into(), signed_osc: 79, signature: [0; 64], signer_device: [0; 32] }));
        assert_eq!(r.title(), "taco");
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
        for sig in [GroupSignal::Offer, GroupSignal::Join, GroupSignal::Wrap, GroupSignal::Records] {
            assert_eq!(GroupSignal::parse(&sig.to_content()), Some(sig));
            assert!(crate::types::is_control_content(&sig.to_content()));
            assert!(!sig.to_content().trim_start_matches(GROUP_PREFIX).contains('\u{2}'), "bare kind markers — nothing rides the text");
        }
        assert_eq!(GroupSignal::parse(&format!("{}bogus\u{2}1", GROUP_PREFIX)), None);
        assert_eq!(GroupSignal::parse("plain text"), None);
    }

    /// The full join arc (§10.1, D10): founder mints; the OFFER carries only a snapshot; the joiner's JOIN carries its member record + bundle; the sponsor vouches and answers with a WRAP sealed to that bundle; the joiner installs the era and both sides hold the same token, lanes and standing set — and the joiner never held a secret before the wrap.
    #[test]
    fn join_arc_founder_to_joiner() {
        use crate::types::friendship::FriendshipChains;
        let f_seed = [0x01u8; 32];
        let j_seed = [0x02u8; 32];
        let founder_party = [0xF0u8; 32];
        let joiner_party = [0x10u8; 32];
        let birth = found_group(founder_party, [0xF1; 32], "founder", [0; 32], "turtles", true, &f_seed);
        let mut f_roster = Roster::default();
        f_roster.merge_genesis(birth.genesis.clone());
        f_roster.merge_member(birth.founder_member.clone());
        f_roster.merge_vouch(birth.founder_vouch.clone());
        let mut f_chains = FriendshipChains::from_group_root(birth.group_id, &f_roster.standing(), birth.group_root, birth.group_history_key, 0, birth.era_lineage);

        // OFFER: the snapshot blob in gpl, nothing else.
        let offer_blob = crate::storage::group::roster_to_vsf_bytes(&birth.group_id, &f_roster).expect("snapshot");
        assert!(!offer_blob.windows(32).any(|w| w == birth.group_root), "no secret in the offer");
        let (gid, snapshot) = crate::storage::group::roster_from_vsf_bytes(&offer_blob).expect("snapshot decodes");
        assert_eq!(gid, birth.group_id);
        let g = snapshot.genesis.as_ref().expect("the invitee sees the birth certificate");
        assert!(verify_record(&g.signing_bytes(), &g.signature, &g.signer_device));
        assert!(snapshot.is_standing(&founder_party), "the sponsor stands in its own snapshot");

        // JOIN: the joiner mints a bundle, keeps the decaps, sends record + bundle.
        let eph = crate::crypto::era::era_keygen(0, 0, crate::crypto::era::KEM_SET_DEFAULT);
        let decaps = eph.export_decaps(0);
        let bundle = bundle_record(joiner_party, 0, &eph, &j_seed);
        let member = join_record(joiner_party, [0x11; 32], "cousin", [0; 32], founder_party, &j_seed);
        let mut join = Roster::default();
        join.merge_member(member.clone());
        join.merge_bundle(bundle.clone());
        let join_blob = crate::storage::group::roster_to_vsf_bytes(&gid, &join).expect("join encodes");

        // The sponsor: verify, vouch, merge, wrap the current era whole (from-genesis).
        let (_, join_records) = crate::storage::group::roster_from_vsf_bytes(&join_blob).expect("join decodes");
        let m = join_records.members.get(&joiner_party).expect("member record");
        assert!(verify_record(&m.signing_bytes(), &m.signature, &m.signer_device));
        let b = join_records.newest_bundle(&device_pubkey(&j_seed)).expect("bundle");
        assert!(verify_record(&b.signing_bytes(), &b.signature, &b.device));
        f_roster.merge_member(m.clone());
        f_roster.merge_bundle(b.clone());
        f_roster.merge_vouch(vouch_record(founder_party, joiner_party, false, &f_seed));
        assert!(f_roster.is_standing(&joiner_party));
        let mut payload = Vec::new();
        payload.extend_from_slice(&birth.group_root);
        payload.extend_from_slice(&birth.group_history_key);
        let nonce = [7u8; 32];
        let (cts, sealed) = crate::crypto::era::wrap_to_bundle(&b.pubkeys(), b.kem_set, &b.device, 0, &nonce, &payload).expect("wrap");

        // WRAP: the joiner opens it against its persisted decaps and installs the era.
        let opened = crate::crypto::era::unwrap_from_bundle(&decaps, &b.pubkeys().bundle_id(), &cts, &device_pubkey(&j_seed), 0, &nonce, &sealed).expect("opens");
        assert_eq!(opened, payload);
        let root: [u8; 32] = opened[..32].try_into().unwrap();
        let hk: [u8; 32] = opened[32..].try_into().unwrap();
        let mut j_roster = snapshot;
        j_roster.merge_member(member);
        j_roster.merge_bundle(bundle);
        j_roster.merge_vouch(f_roster.vouches[&(founder_party, joiner_party)][0].clone());
        let mut j_chains = FriendshipChains::from_group_root(gid, &j_roster.standing(), root, hk, 0, birth.era_lineage);
        assert_eq!(j_chains.conversation_token, f_chains.conversation_token, "one token, minted from the id");
        assert_eq!(f_roster.standing(), j_roster.standing());
        assert_eq!(f_roster.standing().len(), 2);

        // The founder speaks; the joiner derives the lane from the label alone.
        let f_lane = f_chains.mint_our_lane().expect("founder's lane");
        j_chains.ensure_lane(&f_lane).expect("derived from root ‖ label");
        assert_eq!(f_chains.current_key(&f_lane), j_chains.current_key(&f_lane));
    }

    /// The step-1 unit (§9): strand-pull on a simulated three-member stream. Weave refs point strictly backwards, so the pull graph is a DAG — every delivery order (in-order, fully reversed, deterministically shuffled) converges to the same fully-applied stream, pulls only ever name earlier rows, one pull per missing edge, and duplicate offers are no-ops. Braid §1.4 confluence, kept in groups.
    #[test]
    fn strand_pull_dag_three_member_stream() {
        let members: [PartyId; 3] = [[0xA0; 32], [0xB0; 32], [0xC0; 32]];
        // A deterministic LCG (no wall-clock randomness in tests) drives both the weave choices and the shuffle.
        let mut lcg: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move || {
            lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            lcg >> 33
        };
        // The stream: 30 rows, author round-robin, each weaving up to two strictly-earlier rows.
        let mut stream: Vec<(StrandRef, Vec<StrandRef>)> = Vec::new();
        for i in 0..30u64 {
            let row: StrandRef = (members[(i % 3) as usize], 1000 + i as i64);
            let mut woven = Vec::new();
            if i > 0 {
                for _ in 0..(next() % 3) {
                    woven.push(stream[(next() % i) as usize].0);
                }
                woven.dedup();
            }
            stream.push((row, woven));
        }
        let orders: Vec<Vec<usize>> = vec![
            (0..30).collect(),
            (0..30).rev().collect(),
            {
                let mut v: Vec<usize> = (0..30).collect();
                for i in (1..30).rev() {
                    v.swap(i, (next() % (i as u64 + 1)) as usize);
                }
                v
            },
        ];
        for order in orders {
            let mut ledger = StrandLedger::default();
            let mut pulls_seen = 0usize;
            for &i in &order {
                let (row, woven) = &stream[i];
                let out = ledger.offer(*row, woven);
                // Serve every pull immediately from the stream (the sender necessarily holds it) — served strands are just offers, and their own missing refs pull recursively.
                let mut queue = out.pulls;
                while let Some(want) = queue.pop() {
                    pulls_seen += 1;
                    assert!(want.1 < row.1 || want.0 != row.0, "a pull can only name a causally earlier strand");
                    let (prow, pwoven) = stream.iter().find(|(r, _)| *r == want).expect("the sender holds what it wove");
                    assert!(prow.1 <= row.1, "weave refs point strictly backwards — the DAG never asks forward");
                    queue.extend(ledger.offer(*prow, pwoven).pulls);
                }
            }
            assert_eq!(ledger.parked_count(), 0, "every delivery order converges — nothing stays parked");
            for (row, _) in &stream {
                assert!(ledger.holds(row));
            }
            // Duplicate offers after convergence are no-ops.
            let out = ledger.offer(stream[7].0, &stream[7].1);
            assert!(out.applied.is_empty() && out.pulls.is_empty());
            // Reverse delivery must actually exercise the parking machinery.
            if order[0] == 29 {
                assert!(pulls_seen > 0, "reverse order parks and pulls");
            }
        }
    }
}
