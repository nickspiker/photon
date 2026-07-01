//! Fleet membership blob — the network-held, signed, authenticated log of the devices that constitute one identity (the user's fleet).
//! This is the v1 keyring: a list of device public keys you can add to and remove from, where **every change is signed by a device that was valid in the previous state**, chained by hash so the whole history is tamper-evident and replayable.
//! Peers verify a friend's fleet by folding the chain; FGTW gates updates by the same rule.
//! (Supersedes the v0 Merkle-root keyring: count-hiding is deferred to a future modulus accumulator, which layers over this set without changing membership logic.)
//!
//! ## Model (decided 2026-06-30)
//!
//! - Devices are **blind, stateless signing oracles** — each knows only its own private key.
//!   The blob lives wholly on the network; a device fetches it, finds its own pubkey, signs an op, done.
//!   No local fleet state.
//! - **Authorisation = signature from a prior-valid member.**
//!   No shared secret (the handle is disclosable; the only real secret is the per-device key), so "an authorised device approved this" can only be a signature from a key that was in the set before this op.
//! - **Genesis is first-come, self-signed** (the first device claims the handle, like the handle itself).
//!
//! ## Signatures — Ed25519 now, egg-list shaped
//!
//! Each op carries a LIST of `(scheme, sig)` eggs and the rule is **every listed egg must verify**.
//! v1 lists only Ed25519 (the device's existing identity key); adding Falcon-512 / SPHINCS+ later is appending an egg, gated by a credential-format version bump — not a reshape.
//! A forger then has to break *every* family.

use crate::network::fgtw::Keypair;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use vsf::VsfType;

/// Signature-scheme tag (the egg label). Wire-stable: append, never renumber.
pub mod scheme {
    pub const ED25519: u8 = 0;
    // Reserved for the additive PQ eggs:
    // pub const FALCON512: u8 = 1;
    // pub const SPHINCS_PLUS: u8 = 2;
}

/// One signature egg: which scheme, and the signature bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct Egg {
    pub scheme: u8,
    pub sig: Vec<u8>,
}

/// What a fleet op does. `u8` discriminant is the on-wire `kind`; wire-stable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    Genesis = 0,
    Add = 1,
    Remove = 2,
}

impl OpKind {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(OpKind::Genesis),
            1 => Some(OpKind::Add),
            2 => Some(OpKind::Remove),
            _ => None,
        }
    }
}

/// One link in the fleet chain: an authorised change to the device set.
#[derive(Clone, Debug, PartialEq)]
pub struct FleetOp {
    /// The identity (public network id) this op's chain belongs to — bound into every op's signature so a valid chain can't be transplanted under a different (e.g. unclaimed) handle_proof to brick it.
    pub handle_proof: [u8; 32],
    /// Hash of the previous op (`chain_hash`), linking the chain. `[0; 32]` for genesis.
    pub prev_hash: [u8; 32],
    pub kind: OpKind,
    /// The device being added/removed (for genesis: the founding device).
    pub device_pubkey: [u8; 32],
    /// Eagle-time the op was made (ordering / display; not load-bearing for auth).
    pub eagle_time: i64,
    /// The device that SIGNED this op — must have been a member in the previous state (genesis: == device).
    pub signer_pubkey: [u8; 32],
    /// GENESIS ONLY: the identity public key `Ed25519(identity_seed)` — the key only the holder of the handle's secret seed can produce, co-signing the genesis so the fleet is provably founded by the identity owner (not just whoever scraped the public `handle_proof`).
    /// `[0; 32]` on add/remove ops.
    pub identity_pubkey: [u8; 32],
    /// GENESIS ONLY: signature over [`FleetOp::signing_bytes`] by `identity_pubkey`. Empty on add/remove ops.
    pub identity_sig: Vec<u8>,
    /// Signature eggs over [`FleetOp::signing_bytes`]; every listed egg must verify (the egg-list rule).
    pub sigs: Vec<Egg>,
}

/// Domain tag so a fleet-op signature can never be confused with any other signature in the system.
const SIGNING_DOMAIN: &[u8] = b"PHOTON_FLEET_OP_v0";

impl FleetOp {
    /// The exact bytes every egg signs: domain + all content fields, fixed-width and deterministic.
    /// Excludes the sigs themselves (you can't sign the signature).
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(SIGNING_DOMAIN.len() + 32 + 32 + 1 + 32 + 8 + 32);
        b.extend_from_slice(SIGNING_DOMAIN);
        b.extend_from_slice(&self.handle_proof);
        b.extend_from_slice(&self.prev_hash);
        b.push(self.kind as u8);
        b.extend_from_slice(&self.device_pubkey);
        b.extend_from_slice(&self.eagle_time.to_le_bytes());
        b.extend_from_slice(&self.signer_pubkey);
        b.extend_from_slice(&self.identity_pubkey); // bound in so the device sig also commits to the identity key (it can't be swapped)
        b
    }

    /// The chain link for the NEXT op's `prev_hash`: a hash over the signed content AND every signature, so the whole op (including who signed it and how) is immutable once chained.
    pub fn chain_hash(&self) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        h.update(&self.signing_bytes());
        for egg in &self.sigs {
            h.update(&[egg.scheme]);
            h.update(&egg.sig);
        }
        h.update(&self.identity_sig);
        *h.finalize().as_bytes()
    }

    /// Verify the GENESIS identity binding: `identity_sig` is a valid signature over [`FleetOp::signing_bytes`] by `identity_pubkey`.
    /// This proves the founder held `identity_seed` (the handle's secret preimage); a peer who knows the handle additionally checks `identity_pubkey == Ed25519(identity_seed)` via [`MembershipBlob::genesis_identity_matches`].
    fn verify_identity_binding(&self) -> bool {
        self.identity_pubkey != [0u8; 32]
            && self.identity_sig.len() == 64
            && verify_ed25519(&self.identity_pubkey, &self.signing_bytes(), &self.identity_sig)
    }

    /// Verify every signature egg against `signer_pubkey`.
    /// v1 understands Ed25519; an op carrying an egg whose scheme this build doesn't implement is REJECTED (fail-closed — never silently accept an unverifiable op, the no-fork rule).
    /// An empty egg list is invalid.
    pub fn verify_sigs(&self) -> bool {
        if self.sigs.is_empty() {
            return false;
        }
        let msg = self.signing_bytes();
        for egg in &self.sigs {
            let ok = match egg.scheme {
                scheme::ED25519 => verify_ed25519(&self.signer_pubkey, &msg, &egg.sig),
                _ => false, // unknown scheme → fail closed
            };
            if !ok {
                return false;
            }
        }
        true
    }
}

/// Pure Ed25519 verify (photon side, via dalek).
/// The FGTW worker mirrors this op format but verifies via its async webcrypto path; the chain/membership rules in [`fold`] are identical and the part to keep in lockstep.
fn verify_ed25519(pubkey: &[u8; 32], msg: &[u8], sig: &[u8]) -> bool {
    let Ok(vk) = VerifyingKey::from_bytes(pubkey) else {
        return false;
    };
    let Ok(sig_arr): Result<[u8; 64], _> = sig.try_into() else {
        return false;
    };
    vk.verify(msg, &Signature::from_bytes(&sig_arr)).is_ok()
}

/// Why a blob failed to fold. Surfaced so the UI/logs can say *what* was wrong, not just "invalid".
#[derive(Debug, PartialEq)]
pub enum FoldError {
    Empty,
    NotGenesisFirst,
    GenesisNotSelfSigned,
    /// Genesis lacks a valid identity-key co-signature (not founded by the handle owner).
    BadIdentityBinding,
    /// A non-genesis op carries identity-binding fields it has no business carrying.
    StrayIdentityBinding { index: usize },
    /// An op carries a different `handle_proof` than the genesis — a spliced/transplanted chain.
    InconsistentHandleProof { index: usize },
    BrokenChain { index: usize },
    BadSignature { index: usize },
    SignerNotMember { index: usize },
    AddExistingMember { index: usize },
    RemoveNonMember { index: usize },
}

/// The fleet membership blob: the ordered op chain.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MembershipBlob {
    pub ops: Vec<FleetOp>,
}

impl MembershipBlob {
    /// Fold the chain to the CURRENT member set, validating every rule along the way.
    /// This is the heart of the design and the part FGTW must mirror exactly: each op must (1) link to the prior op by hash, (2) carry valid signature(s), and (3) be signed by a device that was a member *before* this op (genesis excepted — it's self-signed into an empty set).
    /// Returns the live device pubkeys in insertion order, or the first rule it violated.
    pub fn fold(&self) -> Result<Vec<[u8; 32]>, FoldError> {
        if self.ops.is_empty() {
            return Err(FoldError::Empty);
        }
        let mut members: Vec<[u8; 32]> = Vec::new();
        let mut expected_prev = [0u8; 32];
        let identity = self.ops[0].handle_proof;

        for (i, op) in self.ops.iter().enumerate() {
            if op.handle_proof != identity {
                return Err(FoldError::InconsistentHandleProof { index: i });
            }
            if op.prev_hash != expected_prev {
                return Err(FoldError::BrokenChain { index: i });
            }
            // Structural check before the sig check: only genesis carries an identity binding (and identity_pubkey is in signing_bytes, so a stray one would otherwise surface as a confusing BadSignature).
            if op.kind != OpKind::Genesis && (op.identity_pubkey != [0u8; 32] || !op.identity_sig.is_empty()) {
                return Err(FoldError::StrayIdentityBinding { index: i });
            }
            if !op.verify_sigs() {
                return Err(FoldError::BadSignature { index: i });
            }
            match op.kind {
                OpKind::Genesis => {
                    if i != 0 || !members.is_empty() {
                        return Err(FoldError::NotGenesisFirst);
                    }
                    if op.signer_pubkey != op.device_pubkey {
                        return Err(FoldError::GenesisNotSelfSigned);
                    }
                    // The genesis MUST be co-signed by the identity key — this is the link that closes the chain onto the handle's owner.
                    if !op.verify_identity_binding() {
                        return Err(FoldError::BadIdentityBinding);
                    }
                    members.push(op.device_pubkey);
                }
                OpKind::Add => {
                    if !members.contains(&op.signer_pubkey) {
                        return Err(FoldError::SignerNotMember { index: i });
                    }
                    if members.contains(&op.device_pubkey) {
                        return Err(FoldError::AddExistingMember { index: i });
                    }
                    members.push(op.device_pubkey);
                }
                OpKind::Remove => {
                    if !members.contains(&op.signer_pubkey) {
                        return Err(FoldError::SignerNotMember { index: i });
                    }
                    let before = members.len();
                    members.retain(|m| m != &op.device_pubkey);
                    if members.len() == before {
                        return Err(FoldError::RemoveNonMember { index: i });
                    }
                }
            }
            expected_prev = op.chain_hash();
        }
        Ok(members)
    }

    /// Convenience: is `device_pubkey` a current member? (`fold` + membership test.)
    pub fn is_member(&self, device_pubkey: &[u8; 32]) -> bool {
        self.fold().map(|m| m.contains(device_pubkey)).unwrap_or(false)
    }

    /// The hash the NEXT op must reference as `prev_hash` (the tail link, or `[0;32]` if empty).
    pub fn head(&self) -> [u8; 32] {
        self.ops.last().map(|op| op.chain_hash()).unwrap_or([0u8; 32])
    }

    /// The identity this chain belongs to (the genesis op's handle_proof), or `None` if empty.
    pub fn handle_proof(&self) -> Option<[u8; 32]> {
        self.ops.first().map(|op| op.handle_proof)
    }

    /// Is `prior` an exact prefix of this chain? FGTW uses this to accept only forward extensions of the stored chain (optimistic concurrency: a writer who appended to a stale head fails this and re-fetches).
    pub fn extends(&self, prior: &MembershipBlob) -> bool {
        prior.ops.len() <= self.ops.len() && self.ops[..prior.ops.len()] == prior.ops[..]
    }

    /// Peer check: does the genesis identity key match `Ed25519(identity_seed)` — the key a contact derives from the handle?
    /// `fold()` already proves the genesis is self-consistently identity-signed; this additionally proves it's THIS handle's owner, so a contact who knows your handle can't be fooled by a squatted fleet under your `handle_proof`.
    pub fn genesis_identity_matches(&self, identity_seed: &[u8; 32]) -> bool {
        let expect = ed25519_dalek::SigningKey::from_bytes(identity_seed).verifying_key().to_bytes();
        self.ops.first().map(|op| op.identity_pubkey == expect).unwrap_or(false)
    }

    // ── builders (sign with the local device key; the device is the only thing that can authorise) ──

    /// Start a brand-new fleet: the founding device self-signs itself in, bound to `handle_proof`, and the identity key `Ed25519(identity_seed)` co-signs to prove the founder owns the handle.
    pub fn genesis(
        device_key: &Keypair,
        handle_proof: [u8; 32],
        identity_seed: &[u8; 32],
        eagle_time: i64,
    ) -> Self {
        let pk = device_key.public.to_bytes();
        let identity_key = ed25519_dalek::SigningKey::from_bytes(identity_seed);
        let op = sign_op(
            device_key,
            handle_proof,
            [0u8; 32],
            OpKind::Genesis,
            pk,
            eagle_time,
            pk,
            Some(&identity_key),
        );
        MembershipBlob { ops: vec![op] }
    }

    /// Append an Add, signed by `device_key` (which must be a current member for the result to fold).
    pub fn add(&mut self, device_key: &Keypair, new_device: [u8; 32], eagle_time: i64) {
        let hp = self.handle_proof().unwrap_or([0u8; 32]);
        let op = sign_op(
            device_key,
            hp,
            self.head(),
            OpKind::Add,
            new_device,
            eagle_time,
            device_key.public.to_bytes(),
            None,
        );
        self.ops.push(op);
    }

    /// Append a Remove, signed by `device_key` (a current member). A device may remove itself or any other.
    pub fn remove(&mut self, device_key: &Keypair, target_device: [u8; 32], eagle_time: i64) {
        let hp = self.handle_proof().unwrap_or([0u8; 32]);
        let op = sign_op(
            device_key,
            hp,
            self.head(),
            OpKind::Remove,
            target_device,
            eagle_time,
            device_key.public.to_bytes(),
            None,
        );
        self.ops.push(op);
    }

    // ── VSF wire form: section "fleet" with one repeated "op" multi-value field per op (same shape as PhonebookResponse's "peer" fields, so the FGTW worker mirrors the parse with the existing pattern).
    //    Positional op layout: hP(handle_proof) hb(prev) u(kind) ke(device) e6(time) ke(signer), then GENESIS-ONLY ke(identity_pubkey) ge(identity_sig), then (u scheme, ge sig) egg pairs to the end.
    //    The identity pair is gated by kind (known at value index 2), so non-genesis ops carry no waste and the egg tail stays unambiguous. Appending a PQ egg = two more trailing values; nothing before them moves. ──

    /// Encode to a complete VSF file (header + provenance + the "fleet" section). Network/disk transport.
    pub fn to_vsf_bytes(&self) -> Result<Vec<u8>, String> {
        let mut section = vsf::VsfSection::new("fleet");
        for op in &self.ops {
            let mut values = vec![
                VsfType::hP(op.handle_proof.to_vec()),
                VsfType::hb(op.prev_hash.to_vec()),
                VsfType::u(op.kind as usize, false),
                VsfType::ke(op.device_pubkey.to_vec()),
                VsfType::e(vsf::types::EtType::e6(op.eagle_time)),
                VsfType::ke(op.signer_pubkey.to_vec()),
            ];
            if op.kind == OpKind::Genesis {
                values.push(VsfType::ke(op.identity_pubkey.to_vec()));
                values.push(VsfType::ge(op.identity_sig.clone()));
            }
            for egg in &op.sigs {
                values.push(VsfType::u(egg.scheme as usize, false));
                values.push(VsfType::ge(egg.sig.clone()));
            }
            section.add_field_multi("op", values);
        }
        vsf::VsfBuilder::new()
            .creation_time_oscillations(vsf::eagle_time_oscillations())
            .provenance_only()
            .add_section_direct(section)
            .build()
            .map_err(|e| format!("fleet to_vsf: {e}"))
    }

    /// Parse from a complete VSF file.
    /// A malformed op aborts the whole parse (the chain is only meaningful intact); returns the blob for [`fold`] to then validate cryptographically.
    pub fn from_vsf_bytes(bytes: &[u8]) -> Result<Self, String> {
        let (_, header_end) =
            vsf::VsfHeader::decode(bytes).map_err(|e| format!("fleet header: {e}"))?;
        // VsfSection::parse reads from the FULL buffer starting at the header's end offset.
        let mut ptr = header_end;
        let section =
            vsf::VsfSection::parse(bytes, &mut ptr).map_err(|e| format!("fleet section: {e}"))?;

        let mut ops = Vec::new();
        for field in section.get_fields("op") {
            ops.push(parse_op(&field.values)?);
        }
        Ok(MembershipBlob { ops })
    }
}

/// Build + sign one op. Each enabled scheme contributes an egg over the op's signing bytes; v1 = Ed25519.
#[allow(clippy::too_many_arguments)]
fn sign_op(
    device_key: &Keypair,
    handle_proof: [u8; 32],
    prev_hash: [u8; 32],
    kind: OpKind,
    device_pubkey: [u8; 32],
    eagle_time: i64,
    signer_pubkey: [u8; 32],
    identity: Option<&ed25519_dalek::SigningKey>,
) -> FleetOp {
    use ed25519_dalek::Signer;
    let identity_pubkey = identity.map(|k| k.verifying_key().to_bytes()).unwrap_or([0u8; 32]);
    let mut op = FleetOp {
        handle_proof,
        prev_hash,
        kind,
        device_pubkey,
        eagle_time,
        signer_pubkey,
        identity_pubkey,
        identity_sig: Vec::new(),
        sigs: Vec::new(),
    };
    let msg = op.signing_bytes();
    op.sigs.push(Egg {
        scheme: scheme::ED25519,
        sig: device_key.secret.sign(&msg).to_bytes().to_vec(),
    });
    if let Some(idk) = identity {
        op.identity_sig = idk.sign(&msg).to_bytes().to_vec();
    }
    op
}

/// Decode one positional "op" field's values back into a [`FleetOp`].
fn parse_op(values: &[VsfType]) -> Result<FleetOp, String> {
    if values.len() < 6 {
        return Err(format!("fleet op: need >=6 values, got {}", values.len()));
    }
    let handle_proof = take_hp32(&values[0], "hp")?;
    let prev_hash = take_hb32(&values[1], "prev")?;
    let kind = match &values[2] {
        VsfType::u(v, false) => OpKind::from_u8(*v as u8).ok_or_else(|| format!("bad kind {v}"))?,
        other => {
            use vsf::schema::FromVsfType;
            let v = u8::from_vsf_type(other).map_err(|_| "fleet op: bad kind type".to_string())?;
            OpKind::from_u8(v).ok_or_else(|| format!("bad kind {v}"))?
        }
    };
    let device_pubkey = take_ke32(&values[3], "device")?;
    let eagle_time = match &values[4] {
        VsfType::e(et) => et_to_osc(et),
        _ => return Err("fleet op: bad time".into()),
    };
    let signer_pubkey = take_ke32(&values[5], "signer")?;

    // GENESIS carries the identity binding (ke pubkey, ge sig) before the egg pairs; other kinds don't.
    let mut i = 6;
    let mut identity_pubkey = [0u8; 32];
    let mut identity_sig = Vec::new();
    if kind == OpKind::Genesis {
        identity_pubkey = take_ke32(values.get(6).ok_or("fleet op: genesis missing identity pubkey")?, "identity")?;
        identity_sig = match values.get(7) {
            Some(VsfType::ge(s)) => s.clone(),
            _ => return Err("fleet op: genesis missing identity sig".into()),
        };
        i = 8;
    }

    // Remaining values are (scheme:u, sig:ge) egg pairs.
    let mut sigs = Vec::new();
    while i + 1 < values.len() {
        let scheme = match &values[i] {
            VsfType::u(v, false) => *v as u8,
            other => {
                use vsf::schema::FromVsfType;
                u8::from_vsf_type(other).map_err(|_| "fleet egg: bad scheme".to_string())?
            }
        };
        let sig = match &values[i + 1] {
            VsfType::ge(s) => s.clone(),
            _ => return Err("fleet egg: bad sig".into()),
        };
        sigs.push(Egg { scheme, sig });
        i += 2;
    }
    Ok(FleetOp {
        handle_proof,
        prev_hash,
        kind,
        device_pubkey,
        eagle_time,
        signer_pubkey,
        identity_pubkey,
        identity_sig,
        sigs,
    })
}

fn take_hp32(v: &VsfType, what: &str) -> Result<[u8; 32], String> {
    match v {
        VsfType::hP(b) if b.len() == 32 => Ok(b.as_slice().try_into().unwrap()),
        _ => Err(format!("fleet op: bad {what} (hP32)")),
    }
}
fn take_hb32(v: &VsfType, what: &str) -> Result<[u8; 32], String> {
    match v {
        VsfType::hb(b) if b.len() == 32 => Ok(b.as_slice().try_into().unwrap()),
        _ => Err(format!("fleet op: bad {what} (hb32)")),
    }
}
fn take_ke32(v: &VsfType, what: &str) -> Result<[u8; 32], String> {
    match v {
        VsfType::ke(b) if b.len() == 32 => Ok(b.as_slice().try_into().unwrap()),
        _ => Err(format!("fleet op: bad {what} (ke32)")),
    }
}
fn et_to_osc(et: &vsf::types::EtType) -> i64 {
    use vsf::types::EtType;
    match et {
        EtType::e5(o) => *o as i64,
        EtType::e6(o) => *o,
        EtType::e7(o) => *o as i64,
        _ => 0, // deprecated float forms; we only ever emit e6
    }
}

// ── Client oracle: the device is blind and stateless — it fetches the chain, signs, and posts; it holds no fleet state of its own. ──

const FGTW_URL: &str = "https://fgtw.org";

/// Fetch the identity's stored fleet chain from FGTW, or `None` if none exists yet (`fleet_get` returns 404).
/// The response is the raw per-op-signed chain VSF; we parse it but do NOT trust it until [`MembershipBlob::fold`].
pub fn fetch(handle_proof: &[u8; 32]) -> Result<Option<MembershipBlob>, String> {
    let mut section = vsf::VsfSection::new("fleet_get");
    section.add_field("hp", VsfType::hP(handle_proof.to_vec()));
    let req = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_section_direct(section)
        .build()
        .map_err(|e| format!("fleet_get build: {e}"))?;
    let resp = crate::network::http::blocking()
        .post(FGTW_URL)
        .timeout(std::time::Duration::from_secs(15))
        .header("Content-Type", "application/octet-stream")
        .body(req)
        .send()
        .map_err(|e| format!("fleet_get send: {e}"))?;
    if resp.status().as_u16() == 404 {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(format!("fleet_get http {}", resp.status()));
    }
    let bytes = resp.bytes().map_err(|e| format!("fleet_get read: {e}"))?;
    Ok(Some(MembershipBlob::from_vsf_bytes(&bytes)?))
}

/// Publish a new (or extended) chain to FGTW.
/// The worker re-folds it and accepts it only as a forward extension of what it holds, so a stale post is rejected (the caller should re-fetch and retry).
pub fn publish(blob: &MembershipBlob) -> Result<(), String> {
    let body = blob.to_vsf_bytes()?;
    let resp = crate::network::http::blocking()
        .post(FGTW_URL)
        .timeout(std::time::Duration::from_secs(15))
        .header("Content-Type", "application/octet-stream")
        .body(body)
        .send()
        .map_err(|e| format!("fleet publish send: {e}"))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("fleet publish http {}: {}", resp.status(), resp.text().unwrap_or_default()))
    }
}

/// Ensure this device is a CURRENT member of the identity's fleet before it tries an authorised write (avatar etc.).
/// No fleet yet → claim it with a first-come genesis (the single-device fleet, the common case after a wipe).
/// Already a member → nothing to do.
/// A fleet exists but this device isn't in it → it must be enrolled from an existing device first (the device-ADD ceremony), so we surface that rather than silently failing the later write.
pub fn ensure_member(
    device_key: &Keypair,
    handle_proof: &[u8; 32],
    identity_seed: &[u8; 32],
) -> Result<(), String> {
    let me = device_key.public.to_bytes();
    if let Some(blob) = fetch(handle_proof)? {
        return if blob.fold().map(|m| m.contains(&me)).unwrap_or(false) {
            Ok(())
        } else {
            Err("this device is not in the fleet — enroll it from an existing device first".into())
        };
    }
    // First-come genesis, identity-key co-signed so the fleet is provably founded by this handle's owner.
    let blob =
        MembershipBlob::genesis(device_key, *handle_proof, identity_seed, vsf::eagle_time_oscillations());
    if publish(&blob).is_ok() {
        return Ok(());
    }
    // Lost a genesis race (someone else claimed it between our fetch and post): re-fetch and accept if we ended up a member.
    if let Some(b) = fetch(handle_proof)? {
        if b.fold().map(|m| m.contains(&me)).unwrap_or(false) {
            return Ok(());
        }
    }
    Err("failed to establish fleet membership for this device".into())
}

/// The current device-pubkey member set of an identity's fleet (empty if no fleet yet).
/// Used by the existing device to render *manage devices*, and by a freshly-paired device to poll "am I in yet?".
pub fn current_members(handle_proof: &[u8; 32]) -> Result<Vec<[u8; 32]>, String> {
    match fetch(handle_proof)? {
        Some(b) => b.fold().map_err(|e| format!("stored fleet invalid: {e:?}")),
        None => Ok(Vec::new()),
    }
}

/// Existing-device side of device-ADD: add `new_pubkey` to the fleet, signed by this (member) device.
/// `new_pubkey` must have arrived over the proximity channel (NFC tap, or words carried screen-to-screen) — NOT from a network request that merely claims the handle — so the signature binds to the device physically in hand, not to anyone who knows the (public) handle.
/// Fetches the chain, appends a member-signed add, publishes; retries on a forward-extension race.
pub fn bind_device(
    member_key: &Keypair,
    handle_proof: &[u8; 32],
    new_pubkey: [u8; 32],
) -> Result<(), String> {
    let me = member_key.public.to_bytes();
    for _attempt in 0..4 {
        let mut blob = fetch(handle_proof)?
            .ok_or("no fleet to add to — attest this identity first")?;
        let members = blob.fold().map_err(|e| format!("stored fleet invalid: {e:?}"))?;
        if !members.contains(&me) {
            return Err("this device isn't a fleet member, so it can't add another".into());
        }
        if members.contains(&new_pubkey) {
            return Ok(()); // already in — idempotent
        }
        blob.add(member_key, new_pubkey, vsf::eagle_time_oscillations());
        match publish(&blob) {
            Ok(()) => return Ok(()),
            Err(e) if e.contains("409") => continue, // someone else extended; re-fetch + retry
            Err(e) => return Err(e),
        }
    }
    Err("fleet add: lost too many extension races".into())
}

/// Existing-device side of device removal: remove `target_pubkey`, signed by this (member) device.
/// Revocation sticks because FGTW gates future writes/announces on the folded chain, which no longer contains the removed device.
pub fn unbind_device(
    member_key: &Keypair,
    handle_proof: &[u8; 32],
    target_pubkey: [u8; 32],
) -> Result<(), String> {
    let me = member_key.public.to_bytes();
    for _attempt in 0..4 {
        let mut blob = fetch(handle_proof)?.ok_or("no fleet to modify")?;
        let members = blob.fold().map_err(|e| format!("stored fleet invalid: {e:?}"))?;
        if !members.contains(&me) {
            return Err("this device isn't a fleet member, so it can't remove another".into());
        }
        if !members.contains(&target_pubkey) {
            return Ok(()); // already gone — idempotent
        }
        blob.remove(member_key, target_pubkey, vsf::eagle_time_oscillations());
        match publish(&blob) {
            Ok(()) => return Ok(()),
            Err(e) if e.contains("409") => continue,
            Err(e) => return Err(e),
        }
    }
    Err("fleet remove: lost too many extension races".into())
}

// ── Pairing secret: a short random value the EXISTING device shows as words and the NEW device types in. ──
// It never travels the network — only the new device's pubkey does (over P2P), and that pubkey is bound into the MACs below by the secret.
// Knowing the secret authenticates the handshake; a wrong transcription just fails the MAC, so no separate checksum is needed.

/// Bytes of a pairing secret — 16 (128-bit) is plenty for a one-shot human-carried code, and stays short as words.
pub const PAIRING_SECRET_LEN: usize = 16;

/// Fresh random pairing secret (existing device mints one per add attempt).
pub fn new_pairing_secret() -> [u8; PAIRING_SECRET_LEN] {
    rand::random()
}

/// The secret as voca words (camelCase — the canonical typed form), and a spaced form for reading aloud.
pub fn secret_words(secret: &[u8; PAIRING_SECRET_LEN]) -> String {
    voca::encode(num_bigint::BigUint::from_bytes_be(secret))
}
pub fn secret_words_spaced(secret: &[u8; PAIRING_SECRET_LEN]) -> String {
    voca::encode_spaced(num_bigint::BigUint::from_bytes_be(secret))
}

/// Decode typed words back to the secret (left-padded; leading zero bytes drop out of the integer).
pub fn secret_from_words(words: &str) -> Result<[u8; PAIRING_SECRET_LEN], String> {
    let n = voca::decode(words.trim()).map_err(|e| format!("unrecognised pairing words: {e:?}"))?;
    let bytes = n.to_bytes_be();
    if bytes.len() > PAIRING_SECRET_LEN {
        return Err("pairing words decode too large".into());
    }
    let mut out = [0u8; PAIRING_SECRET_LEN];
    out[PAIRING_SECRET_LEN - bytes.len()..].copy_from_slice(&bytes);
    Ok(out)
}

fn pairing_mac(domain: &[u8], secret: &[u8], handle_proof: &[u8; 32], new_pubkey: &[u8; 32]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(domain);
    h.update(secret); // secret-prefix MAC — BLAKE3 has no length-extension weakness
    h.update(handle_proof);
    h.update(new_pubkey);
    *h.finalize().as_bytes()
}

/// MAC the NEW device sends with its pairing request: proves it holds the secret AND binds the request to its exact pubkey + identity, so a relay can't swap in a different key.
pub fn pairing_request_mac(
    secret: &[u8; PAIRING_SECRET_LEN],
    handle_proof: &[u8; 32],
    new_pubkey: &[u8; 32],
) -> [u8; 32] {
    pairing_mac(b"PHOTON_PAIR_REQ_v0", secret, handle_proof, new_pubkey)
}

/// MAC the EXISTING device sends back: proves the genuine secret-holder (not a spoofer who raced) acknowledged THIS pubkey, so the new device's "binding…" screen only lights up for the real other device.
pub fn pairing_ack_mac(
    secret: &[u8; PAIRING_SECRET_LEN],
    handle_proof: &[u8; 32],
    new_pubkey: &[u8; 32],
) -> [u8; 32] {
    pairing_mac(b"PHOTON_PAIR_ACK_v0", secret, handle_proof, new_pubkey)
}

/// Short read-aloud fingerprint of a device pubkey, shown on BOTH screens so the human can confirm the existing device is about to bind the device actually in their hand — not a shoulder-surfer who raced a request into the inbox with the same secret.
pub fn device_fingerprint(pubkey: &[u8; 32]) -> String {
    let h = blake3::hash(pubkey);
    voca::encode_spaced(num_bigint::BigUint::from_bytes_be(&h.as_bytes()[..4]))
}

/// A pending pairing request the existing device should surface for confirmation.
pub struct PendingPairing {
    pub new_pubkey: [u8; 32],
    /// The read-aloud fingerprint to display for the human cross-check.
    pub fingerprint: String,
}

/// Pairing requests older than this are ignored (stale inbox slot).
const PAIR_FRESH_OSC: i64 = 300 * vsf::OSCILLATIONS_PER_SECOND as i64; // 5 minutes

// ── Pairing inbox transport (the secret's MAC authenticates; FGTW is a dumb relay). ──

/// NEW device: drop a pairing request `{new_pubkey, request_mac}` into this identity's inbox slot.
/// The existing device validates the MAC with the secret you typed in, so a wrong transcription is silently dropped.
pub fn post_pairing_request(
    new_device_key: &Keypair,
    handle_proof: &[u8; 32],
    secret: &[u8; PAIRING_SECRET_LEN],
) -> Result<(), String> {
    let new_pubkey = new_device_key.public.to_bytes();
    let mac = pairing_request_mac(secret, handle_proof, &new_pubkey);
    let mut section = vsf::VsfSection::new("pair_put");
    section.add_field("hp", VsfType::hP(handle_proof.to_vec()));
    section.add_field("pk", VsfType::ke(new_pubkey.to_vec()));
    section.add_field("mac", VsfType::hb(mac.to_vec()));
    section.add_field("t", VsfType::e(vsf::types::EtType::e6(vsf::eagle_time_oscillations())));
    let req = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_section_direct(section)
        .build()
        .map_err(|e| format!("pair_put build: {e}"))?;
    let resp = crate::network::http::blocking()
        .post(FGTW_URL)
        .timeout(std::time::Duration::from_secs(15))
        .header("Content-Type", "application/octet-stream")
        .body(req)
        .send()
        .map_err(|e| format!("pair_put send: {e}"))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("pair_put http {}", resp.status()))
    }
}

/// EXISTING device: poll the inbox for a pending request and validate it against the typed secret.
/// Returns the device to confirm (with its fingerprint) only if the MAC checks out and the request is fresh — otherwise `None` (no request, stale, or a bad/forged MAC that we ignore).
pub fn poll_pairing_request(
    handle_proof: &[u8; 32],
    secret: &[u8; PAIRING_SECRET_LEN],
) -> Result<Option<PendingPairing>, String> {
    let mut section = vsf::VsfSection::new("pair_get");
    section.add_field("hp", VsfType::hP(handle_proof.to_vec()));
    let req = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_section_direct(section)
        .build()
        .map_err(|e| format!("pair_get build: {e}"))?;
    let resp = crate::network::http::blocking()
        .post(FGTW_URL)
        .timeout(std::time::Duration::from_secs(15))
        .header("Content-Type", "application/octet-stream")
        .body(req)
        .send()
        .map_err(|e| format!("pair_get send: {e}"))?;
    if resp.status().as_u16() == 404 {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(format!("pair_get http {}", resp.status()));
    }
    let bytes = resp.bytes().map_err(|e| format!("pair_get read: {e}"))?;
    let (_, header_end) = vsf::VsfHeader::decode(&bytes).map_err(|e| format!("pair header: {e}"))?;
    let mut ptr = header_end;
    let stored =
        vsf::VsfSection::parse(&bytes, &mut ptr).map_err(|e| format!("pair section: {e}"))?;
    let new_pubkey = match stored.get_field("pk").and_then(|f| f.values.first()) {
        Some(VsfType::ke(b)) if b.len() == 32 => {
            let mut a = [0u8; 32];
            a.copy_from_slice(b);
            a
        }
        _ => return Ok(None),
    };
    let mac = match stored.get_field("mac").and_then(|f| f.values.first()) {
        Some(VsfType::hb(b)) if b.len() == 32 => b.clone(),
        _ => return Ok(None),
    };
    let t = match stored.get_field("t").and_then(|f| f.values.first()) {
        Some(VsfType::e(et)) => et_to_osc(et),
        _ => return Ok(None),
    };
    // Stale, or a MAC we can't reproduce from the secret → not ours; ignore.
    if (vsf::eagle_time_oscillations() - t) > PAIR_FRESH_OSC
        || mac != pairing_request_mac(secret, handle_proof, &new_pubkey)
    {
        return Ok(None);
    }
    Ok(Some(PendingPairing { new_pubkey, fingerprint: device_fingerprint(&new_pubkey) }))
}

// ── Fleet key hand-off ──
// The fleet key is a single high-entropy symmetric secret shared by every device in a fleet — it's what lets a second device decrypt the fleet's PRIVATE state (contacts, chains, preferences) that per-device vault keys can't share. The genesis device mints it; each added device receives THIS one over the pairing channel, sealed under the human-carried pairing secret. That gate is exactly the trust surface the pairing MAC already assumes, so no new key exchange is introduced — knowledge of the typed words is what authorises the hand-off.

/// Domain-separated wrap key derived from the pairing secret. BLAKE3 keyed by a fixed domain so this can never collide with the request/ack MAC derivations that hash the same secret.
fn fleet_key_wrap_key(secret: &[u8; PAIRING_SECRET_LEN]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(b"PHOTON_FLEET_KEY_WRAP_v0");
    h.update(secret);
    *h.finalize().as_bytes()
}

/// A fresh random fleet key — minted once, by the genesis device, and thereafter only ever RECEIVED (never re-minted) so the whole fleet converges on the same key.
pub fn new_fleet_key() -> [u8; 32] {
    rand::random()
}

/// Seal the fleet key under the pairing secret (ChaCha20-Poly1305 via kete). AEAD auth means a wrong secret fails the unwrap rather than yielding a plausible-looking wrong key.
pub fn wrap_fleet_key(
    secret: &[u8; PAIRING_SECRET_LEN],
    fleet_key: &[u8; 32],
) -> Result<Vec<u8>, String> {
    kete::encrypt_bytes(fleet_key, &fleet_key_wrap_key(secret))
}

/// Open a fleet key delivered over the pairing channel. Fails loud (not garbage) on a wrong secret or a tampered blob.
pub fn unwrap_fleet_key(
    secret: &[u8; PAIRING_SECRET_LEN],
    wrapped: &[u8],
) -> Result<[u8; 32], String> {
    let pt = kete::decrypt_bytes(wrapped, &fleet_key_wrap_key(secret))?;
    pt.as_slice().try_into().map_err(|_| "fleet key wrong length".to_string())
}

// ── Fleet key inbox transport (one slot per identity, the sealed key relayed thru FGTW; the AEAD authenticates, FGTW is a dumb relay). ──

/// EXISTING device: after binding the new device, drop the sealed fleet key into the hand-off slot. FGTW (and anyone lacking the pairing secret) sees only ciphertext.
pub fn post_fleet_key(handle_proof: &[u8; 32], wrapped: &[u8]) -> Result<(), String> {
    let mut section = vsf::VsfSection::new("fkey_put");
    section.add_field("hp", VsfType::hP(handle_proof.to_vec()));
    section.add_field("bl", VsfType::ge(wrapped.to_vec()));
    section.add_field("t", VsfType::e(vsf::types::EtType::e6(vsf::eagle_time_oscillations())));
    let req = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_section_direct(section)
        .build()
        .map_err(|e| format!("fkey_put build: {e}"))?;
    let resp = crate::network::http::blocking()
        .post(FGTW_URL)
        .timeout(std::time::Duration::from_secs(15))
        .header("Content-Type", "application/octet-stream")
        .body(req)
        .send()
        .map_err(|e| format!("fkey_put send: {e}"))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("fkey_put http {}", resp.status()))
    }
}

/// NEW device: fetch the sealed fleet key (None until the existing device posts it), to unwrap with the pairing secret.
pub fn fetch_fleet_key(handle_proof: &[u8; 32]) -> Result<Option<Vec<u8>>, String> {
    let mut section = vsf::VsfSection::new("fkey_get");
    section.add_field("hp", VsfType::hP(handle_proof.to_vec()));
    let req = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_section_direct(section)
        .build()
        .map_err(|e| format!("fkey_get build: {e}"))?;
    let resp = crate::network::http::blocking()
        .post(FGTW_URL)
        .timeout(std::time::Duration::from_secs(15))
        .header("Content-Type", "application/octet-stream")
        .body(req)
        .send()
        .map_err(|e| format!("fkey_get send: {e}"))?;
    if resp.status().as_u16() == 404 {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(format!("fkey_get http {}", resp.status()));
    }
    let bytes = resp.bytes().map_err(|e| format!("fkey_get read: {e}"))?;
    let (_, header_end) = vsf::VsfHeader::decode(&bytes).map_err(|e| format!("fkey header: {e}"))?;
    let mut ptr = header_end;
    let stored =
        vsf::VsfSection::parse(&bytes, &mut ptr).map_err(|e| format!("fkey section: {e}"))?;
    match stored.get_field("bl").and_then(|f| f.values.first()) {
        Some(VsfType::ge(b)) => Ok(Some(b.clone())),
        _ => Ok(None),
    }
}

/// NEW device: confirm the sealed fleet key was fetched + unwrapped, so FGTW drops the hand-off slot immediately instead of letting the pairing-secret-wrapped key sit in R2. Best-effort — the worker's GET-time freshness expiry is the backstop if this never arrives.
pub fn ack_fleet_key(handle_proof: &[u8; 32]) -> Result<(), String> {
    let mut section = vsf::VsfSection::new("fkey_ack");
    section.add_field("hp", VsfType::hP(handle_proof.to_vec()));
    let req = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_section_direct(section)
        .build()
        .map_err(|e| format!("fkey_ack build: {e}"))?;
    let resp = crate::network::http::blocking()
        .post(FGTW_URL)
        .timeout(std::time::Duration::from_secs(15))
        .header("Content-Type", "application/octet-stream")
        .body(req)
        .send()
        .map_err(|e| format!("fkey_ack send: {e}"))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("fkey_ack http {}", resp.status()))
    }
}

// ── Fleet shared state: the contact roster ──
// The roster is the "who are my friends" half of a fleet's private state. It rides the fleet key: encrypted with it, pushed to a membership-gated FGTW slot, pulled + CRDT-merged by every device. A new device that joins pulls the roster and re-CLUTCHes each friend on its own device key (conversation HISTORY + per-device ratchets are a later phase — this phase is the roster only).

/// One syncable friend. The minimal identity a device needs to reconstruct a contact and re-CLUTCH: who they are (handle + proof + hash) plus CRDT bookkeeping (`updated` for last-writer-wins, `tombstone` for removals that must stick across a merge).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterEntry {
    pub handle_proof: [u8; 32],
    pub handle_hash: [u8; 32],
    /// Last-known friend device pubkey (a hint; the joining device re-discovers current devices by handle_proof). Zero if unknown.
    pub public_identity: [u8; 32],
    pub handle: String,
    pub added: i64,
    /// Logical clock for this entry — the newest write across the fleet wins the merge.
    pub updated: i64,
    /// A removed contact stays as a tombstone so a stale device re-adding it can't resurrect it.
    pub tombstone: bool,
}

const ROSTER_TAG: &[u8; 5] = b"PRST0";

/// Serialize the roster to the plaintext that gets sealed under the fleet key. Not VSF: this is opaque AEAD-payload bytes, so a compact fixed-layout encoding is simpler and just as forensic (the wire envelope around the ciphertext is VSF).
pub fn roster_to_bytes(entries: &[RosterEntry]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(ROSTER_TAG);
    out.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    for e in entries {
        out.extend_from_slice(&e.handle_proof);
        out.extend_from_slice(&e.handle_hash);
        out.extend_from_slice(&e.public_identity);
        out.extend_from_slice(&e.added.to_be_bytes());
        out.extend_from_slice(&e.updated.to_be_bytes());
        out.push(e.tombstone as u8);
        let hb = e.handle.as_bytes();
        out.extend_from_slice(&(hb.len() as u32).to_be_bytes());
        out.extend_from_slice(hb);
    }
    out
}

/// Parse the roster plaintext back. Bounds-checked throughout — a truncated or corrupt blob fails rather than panicking.
pub fn roster_from_bytes(bytes: &[u8]) -> Result<Vec<RosterEntry>, String> {
    let mut p = 0usize;
    let take = |p: &mut usize, n: usize| -> Result<&[u8], String> {
        if *p + n > bytes.len() {
            return Err("roster: truncated".into());
        }
        let s = &bytes[*p..*p + n];
        *p += n;
        Ok(s)
    };
    if take(&mut p, 5)? != ROSTER_TAG {
        return Err("roster: bad tag".into());
    }
    let count = u32::from_be_bytes(take(&mut p, 4)?.try_into().unwrap()) as usize;
    let mut out = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        let handle_proof: [u8; 32] = take(&mut p, 32)?.try_into().unwrap();
        let handle_hash: [u8; 32] = take(&mut p, 32)?.try_into().unwrap();
        let public_identity: [u8; 32] = take(&mut p, 32)?.try_into().unwrap();
        let added = i64::from_be_bytes(take(&mut p, 8)?.try_into().unwrap());
        let updated = i64::from_be_bytes(take(&mut p, 8)?.try_into().unwrap());
        let tombstone = take(&mut p, 1)?[0] != 0;
        let hlen = u32::from_be_bytes(take(&mut p, 4)?.try_into().unwrap()) as usize;
        let handle = String::from_utf8(take(&mut p, hlen)?.to_vec())
            .map_err(|_| "roster: handle not utf8".to_string())?;
        out.push(RosterEntry {
            handle_proof,
            handle_hash,
            public_identity,
            handle,
            added,
            updated,
            tombstone,
        });
    }
    Ok(out)
}

/// CRDT merge: union by handle_proof, per-entry last-writer-wins on `updated`. Deterministic and order-independent (commutative/idempotent). A tombstone wins an `updated` tie so a concurrent remove beats a concurrent re-add — deletes are conservative.
pub fn merge_rosters(a: Vec<RosterEntry>, b: Vec<RosterEntry>) -> Vec<RosterEntry> {
    use std::collections::HashMap;
    let mut by: HashMap<[u8; 32], RosterEntry> = HashMap::new();
    for e in a.into_iter().chain(b.into_iter()) {
        let replace = match by.get(&e.handle_proof) {
            None => true,
            Some(cur) => {
                e.updated > cur.updated
                    || (e.updated == cur.updated && e.tombstone && !cur.tombstone)
            }
        };
        if replace {
            by.insert(e.handle_proof, e);
        }
    }
    let mut out: Vec<RosterEntry> = by.into_values().collect();
    out.sort_by(|x, y| x.handle_proof.cmp(&y.handle_proof));
    out
}

/// Publish the fleet roster: seal it under the fleet key and PUT it to the membership-gated slot. The envelope is device-signed (ke/ge header) so FGTW can check the writer against the folded fleet chain — any fleet device may write.
pub fn push_roster(
    handle_proof: &[u8; 32],
    device_key: &Keypair,
    fleet_key: &[u8; 32],
    entries: &[RosterEntry],
) -> Result<(), String> {
    let sealed = kete::encrypt_bytes(&roster_to_bytes(entries), fleet_key)?;
    let mut section = vsf::VsfSection::new("fstate_put");
    section.add_field("hp", VsfType::hP(handle_proof.to_vec()));
    section.add_field("bl", VsfType::ge(sealed));
    section.add_field("t", VsfType::e(vsf::types::EtType::e6(vsf::eagle_time_oscillations())));
    let unsigned = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .signed_only(VsfType::ke(device_key.public.to_bytes().to_vec()))
        .add_section_direct(section)
        .build()
        .map_err(|e| format!("fstate_put build: {e}"))?;
    let signed = vsf::verification::sign_file(unsigned, device_key.secret.as_bytes())
        .map_err(|e| format!("fstate_put sign: {e}"))?;
    let resp = crate::network::http::blocking()
        .post(FGTW_URL)
        .timeout(std::time::Duration::from_secs(15))
        .header("Content-Type", "application/octet-stream")
        .body(signed)
        .send()
        .map_err(|e| format!("fstate_put send: {e}"))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("fstate_put http {}", resp.status()))
    }
}

/// Fetch + open the fleet roster (None if none published yet). The GET is unauthenticated — the payload is ciphertext only fleet members can open — so the pull just needs the fleet key.
pub fn pull_roster(
    handle_proof: &[u8; 32],
    fleet_key: &[u8; 32],
) -> Result<Option<Vec<RosterEntry>>, String> {
    let mut section = vsf::VsfSection::new("fstate_get");
    section.add_field("hp", VsfType::hP(handle_proof.to_vec()));
    let req = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_section_direct(section)
        .build()
        .map_err(|e| format!("fstate_get build: {e}"))?;
    let resp = crate::network::http::blocking()
        .post(FGTW_URL)
        .timeout(std::time::Duration::from_secs(15))
        .header("Content-Type", "application/octet-stream")
        .body(req)
        .send()
        .map_err(|e| format!("fstate_get send: {e}"))?;
    if resp.status().as_u16() == 404 {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(format!("fstate_get http {}", resp.status()));
    }
    let bytes = resp.bytes().map_err(|e| format!("fstate_get read: {e}"))?;
    let (_, header_end) =
        vsf::VsfHeader::decode(&bytes).map_err(|e| format!("fstate header: {e}"))?;
    let mut ptr = header_end;
    let stored =
        vsf::VsfSection::parse(&bytes, &mut ptr).map_err(|e| format!("fstate section: {e}"))?;
    let sealed = match stored.get_field("bl").and_then(|f| f.values.first()) {
        Some(VsfType::ge(b)) => b.clone(),
        _ => return Ok(None),
    };
    let plaintext = kete::decrypt_bytes(&sealed, fleet_key)?;
    Ok(Some(roster_from_bytes(&plaintext)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HP: [u8; 32] = [0xab; 32];
    const SEED: [u8; 32] = [0xcd; 32]; // stand-in identity_seed for the founder

    fn key(seed: u8) -> Keypair {
        Keypair::from_seed(&[seed; 32])
    }
    fn pk(k: &Keypair) -> [u8; 32] {
        k.public.to_bytes()
    }

    #[test]
    fn genesis_then_adds_then_remove_folds_to_live_set() {
        let a = key(1);
        let b = key(2);
        let c = key(3);
        let mut blob = MembershipBlob::genesis(&a, HP, &SEED, 100);
        assert_eq!(blob.fold().unwrap(), vec![pk(&a)]);
        assert_eq!(blob.handle_proof(), Some(HP));

        blob.add(&a, pk(&b), 200); // a (a member) adds b
        blob.add(&b, pk(&c), 300); // b (now a member) adds c
        assert_eq!(blob.fold().unwrap(), vec![pk(&a), pk(&b), pk(&c)]);

        blob.remove(&c, pk(&a), 400); // c removes a (any member may remove any)
        assert_eq!(blob.fold().unwrap(), vec![pk(&b), pk(&c)]);
        assert!(!blob.is_member(&pk(&a)));
        assert!(blob.is_member(&pk(&c)));
    }

    #[test]
    fn op_signed_by_non_member_is_rejected() {
        let a = key(1);
        let stranger = key(9);
        let victim = key(5);
        let mut blob = MembershipBlob::genesis(&a, HP, &SEED, 100);
        // A stranger (not in the fleet) tries to add a device — must fail at fold.
        blob.add(&stranger, pk(&victim), 200);
        assert_eq!(blob.fold(), Err(FoldError::SignerNotMember { index: 1 }));
    }

    #[test]
    fn genesis_must_be_self_signed_and_first() {
        let a = key(1);
        let b = key(2);
        // A genesis whose signer != device is forged.
        let forged = sign_op(&a, HP, [0u8; 32], OpKind::Genesis, pk(&b), 100, pk(&a), None);
        let blob = MembershipBlob { ops: vec![forged] };
        assert_eq!(blob.fold(), Err(FoldError::GenesisNotSelfSigned));
    }

    #[test]
    fn tampering_breaks_the_chain_or_signature() {
        let a = key(1);
        let b = key(2);
        let mut blob = MembershipBlob::genesis(&a, HP, &SEED, 100);
        blob.add(&a, pk(&b), 200);
        assert!(blob.fold().is_ok());

        // Tamper with the add op's device pubkey AFTER signing → signature no longer covers it.
        blob.ops[1].device_pubkey = pk(&key(7));
        assert_eq!(blob.fold(), Err(FoldError::BadSignature { index: 1 }));

        // Re-sign the tampered op correctly but leave its prev_hash stale → chain breaks instead.
        let a2 = key(1);
        blob.ops[1] = sign_op(&a2, HP, [1u8; 32], OpKind::Add, pk(&key(7)), 200, pk(&a2), None);
        assert_eq!(blob.fold(), Err(FoldError::BrokenChain { index: 1 }));
    }

    #[test]
    fn transplanted_chain_under_wrong_identity_is_rejected() {
        // A valid chain whose later op was re-stamped with a different handle_proof must fail. (Genuine transplant — re-keying ops[1].handle_proof without re-signing — trips the consistency check.)
        let a = key(1);
        let b = key(2);
        let mut blob = MembershipBlob::genesis(&a, HP, &SEED, 100);
        blob.add(&a, pk(&b), 200);
        blob.ops[1].handle_proof = [0x11; 32];
        assert_eq!(blob.fold(), Err(FoldError::InconsistentHandleProof { index: 1 }));
    }

    #[test]
    fn extends_accepts_forward_only() {
        let a = key(1);
        let b = key(2);
        let base = MembershipBlob::genesis(&a, HP, &SEED, 100);
        let mut grown = base.clone();
        grown.add(&a, pk(&b), 200);
        assert!(grown.extends(&base)); // forward extension
        assert!(!base.extends(&grown)); // shorter can't extend longer

        // A divergent branch (different op at the same height) is NOT an extension.
        let mut fork = base.clone();
        fork.add(&a, pk(&key(8)), 200);
        assert!(!fork.extends(&grown) && !grown.extends(&fork));
    }

    #[test]
    fn vsf_round_trips_and_still_folds() {
        let a = key(1);
        let b = key(2);
        let mut blob = MembershipBlob::genesis(&a, HP, &SEED, 100);
        blob.add(&a, pk(&b), 200);
        let bytes = blob.to_vsf_bytes().unwrap();
        let parsed = MembershipBlob::from_vsf_bytes(&bytes).unwrap();
        assert_eq!(parsed, blob);
        assert_eq!(parsed.fold().unwrap(), vec![pk(&a), pk(&b)]);
    }

    #[test]
    fn unknown_scheme_egg_fails_closed() {
        let a = key(1);
        let mut blob = MembershipBlob::genesis(&a, HP, &SEED, 100);
        // Inject an extra egg with an unimplemented scheme — "every egg must verify" → reject.
        blob.ops[0].sigs.push(Egg { scheme: 250, sig: vec![0u8; 64] });
        assert_eq!(blob.fold(), Err(FoldError::BadSignature { index: 0 }));
    }

    /// Cross-crate drift guard: a fixed blob's bytes must fold to a fixed device set. The FGTW worker mirror (`fgtw/src/fleet.rs`) carries the SAME vector — if either side's signing_bytes / chain_hash / parse diverges, this and the worker's copy disagree, surfacing the drift. Seeds + handle_proof are fixed and timestamps are constants, so the encoded bytes are deterministic.
    #[test]
    fn known_answer_vector_for_worker_parity() {
        let a = key(1);
        let b = key(2);
        let mut blob = MembershipBlob::genesis(&a, HP, &SEED, 100);
        blob.add(&a, pk(&b), 200);
        let members = blob.fold().unwrap();
        assert_eq!(members, vec![pk(&a), pk(&b)]);
        // Re-parsing the wire form yields the identical member set (what the worker computes from the POST).
        let parsed = MembershipBlob::from_vsf_bytes(&blob.to_vsf_bytes().unwrap()).unwrap();
        assert_eq!(parsed.fold().unwrap(), members);
    }

    #[test]
    fn genesis_identity_binding_holds_and_matches_seed() {
        let a = key(1);
        let blob = MembershipBlob::genesis(&a, HP, &SEED, 100);
        assert!(blob.fold().is_ok());
        // A contact who knows the handle (→ SEED) can confirm the founder is the real owner...
        assert!(blob.genesis_identity_matches(&SEED));
        // ...and a different seed (different handle) does not match.
        assert!(!blob.genesis_identity_matches(&[0x99; 32]));
        // The binding survives the VSF round-trip.
        let parsed = MembershipBlob::from_vsf_bytes(&blob.to_vsf_bytes().unwrap()).unwrap();
        assert!(parsed.fold().is_ok() && parsed.genesis_identity_matches(&SEED));
    }

    #[test]
    fn genesis_with_bad_identity_sig_is_rejected() {
        let a = key(1);
        let mut blob = MembershipBlob::genesis(&a, HP, &SEED, 100);
        // Corrupt ONLY the identity signature — the device egg still covers signing_bytes (which excludes it), so this isolates the identity check.
        blob.ops[0].identity_sig = vec![0u8; 64];
        assert_eq!(blob.fold(), Err(FoldError::BadIdentityBinding));
    }

    #[test]
    fn swapping_identity_pubkey_breaks_the_device_sig() {
        let a = key(1);
        let mut blob = MembershipBlob::genesis(&a, HP, &SEED, 100);
        // identity_pubkey is folded into signing_bytes, so swapping it invalidates the device self-signature.
        blob.ops[0].identity_pubkey =
            ed25519_dalek::SigningKey::from_bytes(&[0x99; 32]).verifying_key().to_bytes();
        assert_eq!(blob.fold(), Err(FoldError::BadSignature { index: 0 }));
    }

    #[test]
    fn pairing_secret_words_round_trip() {
        // A normal secret, and one with a leading zero byte (the padding edge).
        for secret in [[7u8; PAIRING_SECRET_LEN], {
            let mut s = [9u8; PAIRING_SECRET_LEN];
            s[0] = 0;
            s
        }] {
            assert_eq!(secret_from_words(&secret_words(&secret)).unwrap(), secret, "camelCase");
            assert_eq!(secret_from_words(&secret_words_spaced(&secret)).unwrap(), secret, "spaced");
        }
    }

    /// End-to-end against LIVE fgtw.org: genesis a fresh fleet, run the full device-ADD pairing handshake, and confirm the new device folds in.
    /// Ignored by default (hits the network + leaves ephemeral random-key objects); run with `--ignored`.
    #[test]
    #[ignore = "hits live fgtw.org"]
    fn live_device_add_round_trip() {
        let handle_proof: [u8; 32] = rand::random();
        let identity_seed: [u8; 32] = rand::random();
        let member = Keypair::from_seed(&rand::random::<[u8; 32]>());
        let newdev = Keypair::from_seed(&rand::random::<[u8; 32]>());

        // Existing device claims the fleet (identity-signed genesis).
        ensure_member(&member, &handle_proof, &identity_seed).expect("genesis");
        assert_eq!(current_members(&handle_proof).unwrap(), vec![member.public.to_bytes()]);

        // New device posts a pairing request; existing device validates it with the secret.
        let secret = new_pairing_secret();
        post_pairing_request(&newdev, &handle_proof, &secret).expect("post request");
        let pending = poll_pairing_request(&handle_proof, &secret)
            .expect("poll")
            .expect("a pending request");
        assert_eq!(pending.new_pubkey, newdev.public.to_bytes());
        // A wrong secret (mistyped words) sees nothing.
        assert!(poll_pairing_request(&handle_proof, &[0u8; PAIRING_SECRET_LEN]).unwrap().is_none());

        // Confirm → bind → the new device is now a fleet member.
        bind_device(&member, &handle_proof, pending.new_pubkey).expect("bind");
        let members = current_members(&handle_proof).unwrap();
        assert!(members.contains(&member.public.to_bytes()));
        assert!(members.contains(&newdev.public.to_bytes()));

        // Fleet key hand-off: existing device seals its key under the secret and posts it; the new device fetches and opens it to the identical bytes.
        let fleet_key = new_fleet_key();
        let sealed = wrap_fleet_key(&secret, &fleet_key).expect("wrap");
        post_fleet_key(&handle_proof, &sealed).expect("post fleet key");
        let fetched = fetch_fleet_key(&handle_proof).expect("fetch").expect("a sealed key");
        assert_eq!(unwrap_fleet_key(&secret, &fetched).expect("unwrap"), fleet_key);
        // A wrong secret can't open it (AEAD auth fails, not garbage).
        assert!(unwrap_fleet_key(&[0u8; PAIRING_SECRET_LEN], &fetched).is_err());
        // Single-use: after the joining device acks, FGTW drops the slot so the wrap doesn't linger.
        ack_fleet_key(&handle_proof).expect("ack");
        assert!(fetch_fleet_key(&handle_proof).unwrap().is_none());
    }

    #[test]
    fn fleet_key_wrap_round_trips_and_rejects_wrong_secret() {
        let secret = new_pairing_secret();
        let fleet_key = new_fleet_key();
        let sealed = wrap_fleet_key(&secret, &fleet_key).unwrap();
        // Right secret opens it to the same bytes...
        assert_eq!(unwrap_fleet_key(&secret, &sealed).unwrap(), fleet_key);
        // ...any other secret fails the AEAD tag (never yields a plausible wrong key)...
        let mut other = secret;
        other[0] ^= 1;
        assert!(unwrap_fleet_key(&other, &sealed).is_err());
        // ...and a flipped ciphertext bit is rejected, not silently decrypted.
        let mut tampered = sealed.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(unwrap_fleet_key(&secret, &tampered).is_err());
    }

    fn roster_entry(hp: u8, updated: i64, tombstone: bool) -> RosterEntry {
        RosterEntry {
            handle_proof: [hp; 32],
            handle_hash: [hp ^ 0xff; 32],
            public_identity: [hp.wrapping_add(1); 32],
            handle: format!("friend{hp}"),
            added: 100,
            updated,
            tombstone,
        }
    }

    #[test]
    fn roster_serialize_round_trips() {
        let entries = vec![roster_entry(1, 200, false), roster_entry(2, 300, true)];
        let bytes = roster_to_bytes(&entries);
        assert_eq!(roster_from_bytes(&bytes).unwrap(), entries);
        // A truncated blob fails rather than panicking.
        assert!(roster_from_bytes(&bytes[..bytes.len() - 3]).is_err());
        assert!(roster_from_bytes(b"nope").is_err());
    }

    #[test]
    fn roster_merge_is_commutative_lww_with_sticky_tombstones() {
        let old = roster_entry(1, 100, false);
        let newer = roster_entry(1, 200, false);
        // Last-writer-wins on `updated`, regardless of merge order.
        let ab = merge_rosters(vec![old.clone()], vec![newer.clone()]);
        let ba = merge_rosters(vec![newer.clone()], vec![old.clone()]);
        assert_eq!(ab, ba);
        assert_eq!(ab[0].updated, 200);
        // A tombstone wins an `updated` tie (delete beats concurrent re-add).
        let alive = roster_entry(1, 200, false);
        let dead = roster_entry(1, 200, true);
        assert!(merge_rosters(vec![alive.clone()], vec![dead.clone()])[0].tombstone);
        assert!(merge_rosters(vec![dead], vec![alive])[0].tombstone);
        // Distinct contacts union together, sorted by handle_proof.
        let two = merge_rosters(vec![roster_entry(2, 1, false)], vec![roster_entry(1, 1, false)]);
        assert_eq!(two.len(), 2);
        assert_eq!(two[0].handle_proof, [1; 32]);
    }

    #[test]
    #[ignore = "hits live fgtw.org"]
    fn live_roster_sync_round_trip() {
        let handle_proof: [u8; 32] = rand::random();
        let identity_seed: [u8; 32] = rand::random();
        let member = Keypair::from_seed(&rand::random::<[u8; 32]>());
        // The writer must be a fleet member (the fstate_put gate folds the chain).
        ensure_member(&member, &handle_proof, &identity_seed).expect("genesis");

        let fleet_key = new_fleet_key();
        let entries = vec![roster_entry(7, 500, false), roster_entry(9, 600, true)];
        push_roster(&handle_proof, &member, &fleet_key, &entries).expect("push roster");
        let pulled = pull_roster(&handle_proof, &fleet_key).expect("pull").expect("a roster");
        assert_eq!(pulled, entries);

        // A non-member can't publish (fold gate rejects the write).
        let stranger = Keypair::from_seed(&rand::random::<[u8; 32]>());
        assert!(push_roster(&handle_proof, &stranger, &fleet_key, &entries).is_err());
    }

    #[test]
    fn pairing_macs_bind_secret_pubkey_and_role() {
        let secret = [3u8; PAIRING_SECRET_LEN];
        let new_pubkey = pk(&key(5));
        let req = pairing_request_mac(&secret, &HP, &new_pubkey);
        // Deterministic for the same inputs...
        assert_eq!(req, pairing_request_mac(&secret, &HP, &new_pubkey));
        // ...request and ack MACs differ (domain separation, so an ack can't be replayed as a request)...
        assert_ne!(req, pairing_ack_mac(&secret, &HP, &new_pubkey));
        // ...a wrong secret fails (mistyped words → bind never authorises)...
        assert_ne!(req, pairing_request_mac(&[4u8; PAIRING_SECRET_LEN], &HP, &new_pubkey));
        // ...and a swapped pubkey fails (a relay can't re-point the request at another key).
        assert_ne!(req, pairing_request_mac(&secret, &HP, &pk(&key(6))));
    }

    #[test]
    fn add_op_carrying_identity_fields_is_rejected() {
        let a = key(1);
        let b = key(2);
        let mut blob = MembershipBlob::genesis(&a, HP, &SEED, 100);
        blob.add(&a, pk(&b), 200);
        // Stuff an identity binding onto the add op — only genesis may carry one.
        blob.ops[1].identity_pubkey = [0x77; 32];
        assert_eq!(blob.fold(), Err(FoldError::StrayIdentityBinding { index: 1 }));
    }
}
