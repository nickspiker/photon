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
    let _ = publish(&blob);
    // Trust the network, not ourselves: re-fetch the canonical chain and accept ONLY if it actually names this device. The fleet slot has no compare-and-set, so if two devices attest the same fresh handle inside the fetch→publish window they both "publish" but the slot settles on ONE genesis — the loser re-reads it here, finds it isn't a member, and fails cleanly instead of announcing as a phantom founder (which is what leaves a zombie peer row).
    match fetch(handle_proof)? {
        Some(b) if b.fold().map(|m| m.contains(&me)).unwrap_or(false) => Ok(()),
        Some(_) => {
            Err("this device is not in the fleet — enroll it from an existing device first".into())
        }
        None => Err("failed to establish fleet membership for this device".into()),
    }
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

// ── Pairing v1: the NEW device generates a fresh 256-bit pairing keypair and DISPLAYS its public half as words; the user types them into an EXISTING device, which matches them against the posted request and binds. ──
// The words are a public key, not a bearer secret: the request is SIGNED by the pairing private key, so a shoulder-surfer who reads the words can find the request but can never forge a rival one for their own device — stealing the invite requires stealing the new device itself.
// 256-bit because the value is matched on the network: birthday-bounded to 128-bit security, per the count that matters.

/// Fixed word count for a 256-bit pairing key: voca's FULL base is 3177 (~11.63 bits/word), and 22 words is 255.94 bits — just short — so 23 covers every key. Fixed-width (leading-zero-padded) so the typing side always knows when the entry is complete.
pub const PAIR_WORD_COUNT: usize = 23;

/// Fresh pairing identity for one add attempt (the seed IS the 256-bit value the words carry — the keypair is derived from it).
pub fn new_pairing_id() -> Keypair {
    Keypair::from_seed(&rand::random::<[u8; 32]>())
}

/// The zero word (digit 0), capitalised to match voca's camelCase encode — the left-pad for keys with leading zeros, so the word count never shrinks and the completeness check stays exact.
fn zero_word() -> String {
    let w = std::str::from_utf8(voca::FULL.alphabet[0]).expect("voca words are ASCII");
    let mut s = String::with_capacity(w.len());
    let mut chars = w.chars();
    if let Some(c) = chars.next() {
        s.push(c.to_ascii_uppercase());
        s.extend(chars);
    }
    s
}

/// The pairing pubkey as EXACTLY `PAIR_WORD_COUNT` camelCase words, left-padded with the zero word. Positional base-3177: leading zero-digits don't change the decoded value, so padding is free.
pub fn pair_words(pairing_pubkey: &[u8; 32]) -> String {
    let encoded = voca::encode(num_bigint::BigUint::from_bytes_be(pairing_pubkey));
    let have = pair_word_tokens(&encoded);
    let mut s = String::new();
    for _ in have..PAIR_WORD_COUNT {
        s.push_str(&zero_word());
    }
    s.push_str(&encoded);
    s
}

/// Count the words in a typed string, mirroring voca's tokenizer: whitespace-separated if any whitespace, else camelCase boundaries. Drives the live n/23 counter and the completeness gate.
pub fn pair_word_tokens(s: &str) -> usize {
    let t = s.trim();
    if t.is_empty() {
        return 0;
    }
    if t.bytes().any(|b| b.is_ascii_whitespace()) {
        return t.split_ascii_whitespace().count();
    }
    let mut count = 1;
    for c in t.chars().skip(1) {
        if c.is_ascii_uppercase() {
            count += 1;
        }
    }
    count
}

/// Lazy index over the voca FULL alphabet for live spell-checking: a hash set for exact membership plus a sorted copy for prefix tests. Built once, ~3177 entries.
static WORD_INDEX: std::sync::OnceLock<(
    std::collections::HashSet<&'static [u8]>,
    Vec<&'static [u8]>,
)> = std::sync::OnceLock::new();
fn word_index() -> &'static (std::collections::HashSet<&'static [u8]>, Vec<&'static [u8]>) {
    WORD_INDEX.get_or_init(|| {
        let set: std::collections::HashSet<_> = voca::FULL.alphabet.iter().copied().collect();
        let mut sorted: Vec<_> = voca::FULL.alphabet.to_vec();
        sorted.sort_unstable();
        (set, sorted)
    })
}

/// The typed entry's tokens, lowercased, split exactly the way [`pair_word_tokens`] counts them: whitespace-separated if the entry contains any whitespace, else camelCase boundaries.
pub fn pair_word_list(s: &str) -> Vec<String> {
    let t = s.trim();
    if t.is_empty() {
        return Vec::new();
    }
    if t.bytes().any(|b| b.is_ascii_whitespace()) {
        return t.split_ascii_whitespace().map(|w| w.to_ascii_lowercase()).collect();
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in t.chars() {
        if c.is_ascii_uppercase() && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(c.to_ascii_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Live spell-check of a (possibly partial) pairing entry against the voca FULL list. Every completed word must be an exact list member; the final, still-being-typed word passes while it's still a PREFIX of some list word, so nothing flashes red mid-word — but the instant it can't become any list word ("contrav…", "spontani…") it's flagged, and a full 23-word entry (or a trailing space) demands exactness from every token. Case-insensitive. Returns the first offender for the status line.
pub fn first_bad_pair_word(s: &str) -> Option<String> {
    let words = pair_word_list(s);
    if words.is_empty() {
        return None;
    }
    let (set, sorted) = word_index();
    // The last token is "complete" (must match exactly) only once a separator follows it — a full-width count is NOT completion, because the 23rd token exists from its first typed character (an "at 23 words check everything" rule would flag the last word mid-type). A valid full word passes the prefix test anyway (exact match is its own prefix), so the lenient last-token rule never rejects a correct entry.
    let last_complete = s != s.trim_end();
    let n = words.len();
    for (i, w) in words.iter().enumerate() {
        let wb = w.as_bytes();
        let ok = if i + 1 < n || last_complete {
            set.contains(wb)
        } else {
            let idx = sorted.partition_point(|&cand| cand < wb);
            idx < sorted.len() && sorted[idx].starts_with(wb)
        };
        if !ok {
            return Some(w.clone());
        }
    }
    None
}

/// Parse a hub-pushed pairing event — section `pair_evt` {k: kind, hp} — into (kind, handle_proof). Returns `None` for every other frame: the hub also carries dashboard-capsule broadcasts, which subscribers skip cheaply on the header/section decode. Kinds today: "matched" (a member posted the matched flag) and "fleet" (the membership chain extended).
pub fn parse_pair_event(bytes: &[u8]) -> Option<(String, [u8; 32])> {
    let (_, header_end) = vsf::VsfHeader::decode(bytes).ok()?;
    let mut ptr = header_end;
    let section = vsf::VsfSection::parse(bytes, &mut ptr).ok()?;
    if section.name != "pair_evt" {
        return None;
    }
    let kind = match section.get_field("k").and_then(|f| f.values.first()) {
        // `a` is what the worker sends (its vsf build has no `text` feature, so `x` would panic there); accept `x` too for forward-compat.
        Some(VsfType::a(s)) | Some(VsfType::x(s)) => s.clone(),
        _ => return None,
    };
    let hp = match section.get_field("hp").and_then(|f| f.values.first()) {
        Some(VsfType::hP(b)) if b.len() == 32 => {
            let mut a = [0u8; 32];
            a.copy_from_slice(b);
            a
        }
        _ => return None,
    };
    Some((kind, hp))
}

/// True when the entry is fully typed: exactly `PAIR_WORD_COUNT` tokens and EVERY token an exact list member. This is the completeness gate for the network match-check: a bare token count trips on the 23rd word's first character (the token exists from its first letter), firing a decode that then complains "unrecognised word" about a word the user simply hasn't finished typing.
pub fn pair_entry_complete(s: &str) -> bool {
    let words = pair_word_list(s);
    words.len() == PAIR_WORD_COUNT && {
        let (set, _) = word_index();
        words.iter().all(|w| set.contains(w.as_bytes()))
    }
}

/// Decode a complete word entry back to the pairing pubkey. Strict: exactly `PAIR_WORD_COUNT` words, value < 2^256. A wrong word fails the decode; the right words of the wrong device fail the match downstream.
pub fn words_to_pair_pubkey(words: &str) -> Result<[u8; 32], String> {
    if pair_word_tokens(words) != PAIR_WORD_COUNT {
        return Err(format!("expected {PAIR_WORD_COUNT} words"));
    }
    let n = voca::decode(words.trim()).map_err(|e| format!("unrecognised word: {e:?}"))?;
    let bytes = n.to_bytes_be();
    if bytes.len() > 32 {
        return Err("words don't decode to a key".into());
    }
    let mut out = [0u8; 32];
    out[32 - bytes.len()..].copy_from_slice(&bytes);
    Ok(out)
}

/// Deterministic default device label: exactly TWO voca words derived one-way from the device secret. blake3 keeps the secret unrecoverable from the label; the fingerprint-deterministic secret makes the label survive a wipe-and-reinstall (the "same device, same name" resume story). Label space is 3177² ≈ 10.1 M, so even a 12-device fleet collides with p ≈ 7×10⁻⁶. camelCase per the voca display convention. The owner-edited override (devices page) supersedes this — it is only the shipped default.
pub fn device_name_default(device_secret: &[u8; 32]) -> String {
    let mut input = Vec::with_capacity(24 + 32);
    input.extend_from_slice(b"PHOTON_DEVICE_NAME_v1");
    input.extend_from_slice(device_secret);
    let digest = blake3::hash(&input);
    let mut n8 = [0u8; 8];
    n8.copy_from_slice(&digest.as_bytes()[..8]);
    let base = voca::FULL.alphabet.len() as u64;
    let n = u64::from_le_bytes(n8) % (base * base);
    let encoded = voca::encode(num_bigint::BigUint::from(n));
    // Left-pad to exactly two words (a value < base encodes as one) — fixed width like pair_words, so the label always reads as a two-word name.
    let mut s = String::new();
    for _ in pair_word_tokens(&encoded)..2 {
        s.push_str(&zero_word());
    }
    s.push_str(&encoded);
    s
}

/// A pairing request the existing device matched against the typed words: the device to bind, proven owned by the pairing key the words name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairRequest {
    pub pairing_pubkey: [u8; 32],
    pub device_pubkey: [u8; 32],
}

/// Pairing slots older than this are ignored (stale inbox).
const PAIR_FRESH_OSC: i64 = 300 * vsf::OSCILLATIONS_PER_SECOND as i64; // 5 minutes

fn pair_request_signing_bytes(handle_proof: &[u8; 32], device_pubkey: &[u8; 32], t: i64) -> Vec<u8> {
    let mut v = Vec::with_capacity(24 + 64 + 8);
    v.extend_from_slice(b"PHOTON_PAIR_REQ_v1");
    v.extend_from_slice(handle_proof);
    v.extend_from_slice(device_pubkey);
    v.extend_from_slice(&t.to_le_bytes());
    v
}

fn pair_matched_signing_bytes(handle_proof: &[u8; 32], pairing_pubkey: &[u8; 32], t: i64) -> Vec<u8> {
    let mut v = Vec::with_capacity(28 + 64 + 8);
    v.extend_from_slice(b"PHOTON_PAIR_MATCHED_v1");
    v.extend_from_slice(handle_proof);
    v.extend_from_slice(pairing_pubkey);
    v.extend_from_slice(&t.to_le_bytes());
    v
}

// ── Pairing transport (FGTW is a dumb relay; the pairing key's signature authenticates ownership, the member's signature authenticates the match). ──

/// NEW device: post its pairing request — `{device_pubkey, pairing_pubkey, t, sig}` where `sig` is the PAIRING key signing the (identity, device, time) tuple.
/// The signature is the ownership proof: only the holder of the displayed words' private half can produce a valid request, so the words on the screen can't be hijacked for a different device.
pub fn post_pairing_request(
    pairing: &Keypair,
    new_device_pubkey: &[u8; 32],
    handle_proof: &[u8; 32],
) -> Result<(), String> {
    let t = vsf::eagle_time_oscillations();
    let sig = pairing.sign(&pair_request_signing_bytes(handle_proof, new_device_pubkey, t));
    let mut section = vsf::VsfSection::new("pair_put");
    section.add_field("hp", VsfType::hP(handle_proof.to_vec()));
    section.add_field("dk", VsfType::ke(new_device_pubkey.to_vec()));
    section.add_field("pp", VsfType::ke(pairing.public.to_bytes().to_vec()));
    section.add_field("t", VsfType::e(vsf::types::EtType::e6(t)));
    section.add_field("sig", VsfType::ge(sig.to_bytes().to_vec()));
    let req = vsf::VsfBuilder::new()
        .creation_time_oscillations(t)
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

/// EXISTING device: fetch the pending pairing request for this identity, validating freshness and the pairing key's ownership signature.
/// Returns `None` when there's no fresh valid request; the caller compares `pairing_pubkey` against the typed words to decide "match" vs "words don't match".
pub fn fetch_pairing_request(handle_proof: &[u8; 32]) -> Result<Option<PairRequest>, String> {
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
    let field32 = |name: &str| -> Option<[u8; 32]> {
        match stored.get_field(name).and_then(|f| f.values.first()) {
            Some(VsfType::ke(b)) if b.len() == 32 => {
                let mut a = [0u8; 32];
                a.copy_from_slice(b);
                Some(a)
            }
            _ => None,
        }
    };
    let (Some(device_pubkey), Some(pairing_pubkey)) = (field32("dk"), field32("pp")) else {
        return Ok(None);
    };
    let t = match stored.get_field("t").and_then(|f| f.values.first()) {
        Some(VsfType::e(et)) => et_to_osc(et),
        _ => return Ok(None),
    };
    let sig = match stored.get_field("sig").and_then(|f| f.values.first()) {
        Some(VsfType::ge(s)) if s.len() == 64 => Signature::from_bytes(s.as_slice().try_into().unwrap()),
        _ => return Ok(None),
    };
    // Stale, or an ownership signature that doesn't verify under the request's own pairing key → not a usable request; ignore.
    if (vsf::eagle_time_oscillations() - t) > PAIR_FRESH_OSC {
        return Ok(None);
    }
    let Ok(vk) = VerifyingKey::from_bytes(&pairing_pubkey) else {
        return Ok(None);
    };
    if vk.verify(&pair_request_signing_bytes(handle_proof, &device_pubkey, t), &sig).is_err() {
        return Ok(None);
    }
    Ok(Some(PairRequest { pairing_pubkey, device_pubkey }))
}

/// EXISTING device: after the typed words matched the request, post the signed "matched" flag so the new device's screen flips to ready. Cosmetic but authenticated: signed by this (member) device, verified by the new device against the current member set.
pub fn post_pair_matched(
    member_key: &Keypair,
    handle_proof: &[u8; 32],
    pairing_pubkey: &[u8; 32],
) -> Result<(), String> {
    let t = vsf::eagle_time_oscillations();
    let sig = member_key.sign(&pair_matched_signing_bytes(handle_proof, pairing_pubkey, t));
    let mut section = vsf::VsfSection::new("pack_put");
    section.add_field("hp", VsfType::hP(handle_proof.to_vec()));
    section.add_field("pp", VsfType::ke(pairing_pubkey.to_vec()));
    section.add_field("dk", VsfType::ke(member_key.public.to_bytes().to_vec()));
    section.add_field("t", VsfType::e(vsf::types::EtType::e6(t)));
    section.add_field("sig", VsfType::ge(sig.to_bytes().to_vec()));
    let req = vsf::VsfBuilder::new()
        .creation_time_oscillations(t)
        .provenance_only()
        .add_section_direct(section)
        .build()
        .map_err(|e| format!("pack_put build: {e}"))?;
    let resp = crate::network::http::blocking()
        .post(FGTW_URL)
        .timeout(std::time::Duration::from_secs(15))
        .header("Content-Type", "application/octet-stream")
        .body(req)
        .send()
        .map_err(|e| format!("pack_put send: {e}"))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("pack_put http {}", resp.status()))
    }
}

/// NEW device: has an existing member matched OUR words? True only for a fresh flag naming OUR pairing pubkey, signed by a device in `members` — so a stranger can't flip the ready light.
pub fn poll_pair_matched(
    handle_proof: &[u8; 32],
    pairing_pubkey: &[u8; 32],
    members: &[[u8; 32]],
) -> Result<bool, String> {
    let mut section = vsf::VsfSection::new("pack_get");
    section.add_field("hp", VsfType::hP(handle_proof.to_vec()));
    let req = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_section_direct(section)
        .build()
        .map_err(|e| format!("pack_get build: {e}"))?;
    let resp = crate::network::http::blocking()
        .post(FGTW_URL)
        .timeout(std::time::Duration::from_secs(15))
        .header("Content-Type", "application/octet-stream")
        .body(req)
        .send()
        .map_err(|e| format!("pack_get send: {e}"))?;
    if resp.status().as_u16() == 404 {
        return Ok(false);
    }
    if !resp.status().is_success() {
        return Err(format!("pack_get http {}", resp.status()));
    }
    let bytes = resp.bytes().map_err(|e| format!("pack_get read: {e}"))?;
    let (_, header_end) = vsf::VsfHeader::decode(&bytes).map_err(|e| format!("pack header: {e}"))?;
    let mut ptr = header_end;
    let stored =
        vsf::VsfSection::parse(&bytes, &mut ptr).map_err(|e| format!("pack section: {e}"))?;
    let field32 = |name: &str| -> Option<[u8; 32]> {
        match stored.get_field(name).and_then(|f| f.values.first()) {
            Some(VsfType::ke(b)) if b.len() == 32 => {
                let mut a = [0u8; 32];
                a.copy_from_slice(b);
                Some(a)
            }
            _ => None,
        }
    };
    let (Some(pp), Some(dk)) = (field32("pp"), field32("dk")) else {
        return Ok(false);
    };
    let t = match stored.get_field("t").and_then(|f| f.values.first()) {
        Some(VsfType::e(et)) => et_to_osc(et),
        _ => return Ok(false),
    };
    let sig = match stored.get_field("sig").and_then(|f| f.values.first()) {
        Some(VsfType::ge(s)) if s.len() == 64 => Signature::from_bytes(s.as_slice().try_into().unwrap()),
        _ => return Ok(false),
    };
    if pp != *pairing_pubkey
        || (vsf::eagle_time_oscillations() - t) > PAIR_FRESH_OSC
        || !members.contains(&dk)
    {
        return Ok(false);
    }
    let Ok(vk) = VerifyingKey::from_bytes(&dk) else {
        return Ok(false);
    };
    Ok(vk.verify(&pair_matched_signing_bytes(handle_proof, &pp, t), &sig).is_ok())
}

// ── The fleet key ──
// A single high-entropy symmetric secret shared by every device in a fleet — what lets a second device decrypt the fleet's PRIVATE state (contacts, chains, preferences) that per-device vault keys can't share. Delivered per-member via the fan-out below (sealed to each device's own key, epoch-rotated on membership change); the old pairing-secret-wrapped hand-off is gone.

/// A fresh random fleet key — minted per epoch by `rotate_fleet_key`; devices RECEIVE the current one from the fan-out.
pub fn new_fleet_key() -> [u8; 32] {
    rand::random()
}

// ── Fleet key fan-out: per-member sealed delivery of the current fleet key (BRAID.md §14.2) ──
// The steady-state replacement for the pairing hand-off. Each epoch mints a FRESH fleet key and seals it SEPARATELY to every CURRENT member device — a sealed box to the device's X25519 key, converted from the Ed25519 device key already in the membership chain (no chain-format change). A device recovers the current key by trial-decrypting its own wrap with its `ihi` — no live sibling. A removed device is simply not a wrap target next epoch, so the new key is unreadable to it: removal removes, and there is NO seal-under-the-prior-key chain (that would be a skeleton key).

const FANOUT_DOMAIN: &[u8] = b"PHOTON_FLEET_FANOUT_v0";
const FANOUT_TAG: &[u8; 4] = b"PFO0";

/// One sealed copy of the fleet key for one (unlabelled) member. `epk` is a per-wrap ephemeral X25519 public; `commit` binds the ciphertext to the exact derived key (KEY-COMMITTING — so a malicious member can't craft one `ct` that opens to different keys for two devices, the invisible-salamander split); `ct` is ChaCha20-Poly1305(fleet_key) under the ECDH-derived key. No recipient label — a device recomputes `commit` to find its own — so the slot carries only a count, never pubkeys.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FanoutWrap {
    pub epk: [u8; 32],
    pub commit: [u8; 32],
    pub ct: Vec<u8>,
}

/// Ed25519 device pubkey → its X25519 (Montgomery) counterpart, so we can seal to a key already in the membership chain. The matching secret side is `SigningKey::to_scalar_bytes` (§`fanout_open`); `to_montgomery` and the clamped scalar agree on the same point.
fn ed_to_x25519_public(ed_pubkey: &[u8; 32]) -> Option<[u8; 32]> {
    Some(VerifyingKey::from_bytes(ed_pubkey).ok()?.to_montgomery().to_bytes())
}

/// Derive the per-wrap AEAD key AND its key-commitment from the ECDH shared secret.
/// Binds the FLEET (`handle_proof`) and `epoch`, so a wrap is valid only for (this fleet, this epoch, this recipient) — no cross-fleet or cross-epoch splicing (a device key is the same across fleets, so without this a wrap lifts between them). `epk` MUST stay in this hash: it is what makes each wrap's key unique, which is what makes the fixed AEAD nonce safe — never derive the key from `shared` alone. The 64-byte XOF splits into `(aead_key, commit)`; `commit` binds `ct` to this exact key (defeats the partitioning-oracle / invisible-salamander attack that Poly1305 alone allows) and doubles as the recipient selector.
fn fanout_keys(
    handle_proof: &[u8; 32],
    epoch: u64,
    recipient_ed: &[u8; 32],
    shared: &[u8; 32],
    epk: &[u8; 32],
    recipient_xpk: &[u8; 32],
) -> ([u8; 32], [u8; 32]) {
    let mut h = blake3::Hasher::new();
    h.update(FANOUT_DOMAIN);
    h.update(handle_proof);
    h.update(&epoch.to_le_bytes());
    // Bind the canonical Ed25519 device pubkey too: to_montgomery drops the sign bit, so two distinct Ed25519 keys can share a Montgomery u — this disambiguates them.
    h.update(recipient_ed);
    h.update(epk);
    h.update(recipient_xpk);
    h.update(shared);
    let mut out = [0u8; 64];
    h.finalize_xof().fill(&mut out);
    let mut ak = [0u8; 32];
    let mut cm = [0u8; 32];
    ak.copy_from_slice(&out[..32]);
    cm.copy_from_slice(&out[32..]);
    (ak, cm)
}

/// Seal `fleet_key` separately to each current member (Ed25519 device pubkeys, e.g. from a folded `MembershipBlob`) for a given `(handle_proof, epoch)`. A device not in `members` gets no wrap and cannot recover the key.
pub fn fanout_seal(
    handle_proof: &[u8; 32],
    epoch: u64,
    fleet_key: &[u8; 32],
    members: &[[u8; 32]],
) -> Result<Vec<FanoutWrap>, String> {
    use chacha20poly1305::{aead::Aead, ChaCha20Poly1305, KeyInit, Nonce};
    use x25519_dalek::{PublicKey as XPublic, StaticSecret};
    let mut wraps = Vec::with_capacity(members.len());
    for member_ed in members {
        let recipient_xpk =
            ed_to_x25519_public(member_ed).ok_or_else(|| "fanout: bad member pubkey".to_string())?;
        // Fresh ephemeral per wrap → the key is unique per wrap → a zero nonce is safe (no reuse).
        let esk = StaticSecret::from(rand::random::<[u8; 32]>());
        let epk = XPublic::from(&esk).to_bytes();
        let ss = esk.diffie_hellman(&XPublic::from(recipient_xpk));
        // Reject a low-order member pubkey (a zero/small-order shared secret would be attacker-predictable).
        if !ss.was_contributory() {
            return Err("fanout: member pubkey is low-order".into());
        }
        let shared = ss.to_bytes();
        let (ak, commit) = fanout_keys(handle_proof, epoch, member_ed, &shared, &epk, &recipient_xpk);
        let ct = ChaCha20Poly1305::new((&ak).into())
            .encrypt(Nonce::from_slice(&[0u8; 12]), fleet_key.as_slice())
            .map_err(|_| "fanout: seal failed".to_string())?;
        wraps.push(FanoutWrap { epk, commit, ct });
    }
    Ok(wraps)
}

/// Recover the fleet key for `(handle_proof, epoch)` by finding this device's wrap (via the key-commitment) and decrypting. `None` if this device is not a recipient (removed, or a stale epoch it was never in) — and, because the key is bound to `(handle_proof, epoch)`, a wrap from a different fleet or epoch simply won't match.
pub fn fanout_open(
    handle_proof: &[u8; 32],
    epoch: u64,
    wraps: &[FanoutWrap],
    device_key: &Keypair,
) -> Option<[u8; 32]> {
    use chacha20poly1305::{aead::Aead, ChaCha20Poly1305, KeyInit, Nonce};
    use x25519_dalek::{PublicKey as XPublic, StaticSecret};
    let my_xsk = StaticSecret::from(device_key.secret.to_scalar_bytes());
    let my_xpk = device_key.public.to_montgomery().to_bytes();
    let my_ed = device_key.public.to_bytes();
    for w in wraps {
        let ss = my_xsk.diffie_hellman(&XPublic::from(w.epk));
        // Reject a low-order/attacker-chosen epk (a zero shared secret would let a malicious member install a chosen key).
        if !ss.was_contributory() {
            continue;
        }
        let shared = ss.to_bytes();
        let (ak, commit) = fanout_keys(handle_proof, epoch, &my_ed, &shared, &w.epk, &my_xpk);
        // Key-commitment gate: accept only a wrap bound to THIS exact derived key (defeats a crafted ct that opens under two keys), which doubles as the recipient selector.
        if commit != w.commit {
            continue;
        }
        if let Ok(pt) = ChaCha20Poly1305::new((&ak).into())
            .decrypt(Nonce::from_slice(&[0u8; 12]), w.ct.as_slice())
        {
            if let Ok(k) = <[u8; 32]>::try_from(pt.as_slice()) {
                return Some(k);
            }
        }
    }
    None
}

/// Serialize a fan-out (epoch + wraps) for the always-online slot. Opaque per-wrap ciphertext, so a plain length-framed layout; the envelope on the wire stays VSF.
pub fn fanout_to_bytes(epoch: u64, wraps: &[FanoutWrap]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(FANOUT_TAG);
    out.extend_from_slice(&epoch.to_be_bytes());
    out.extend_from_slice(&(wraps.len() as u32).to_be_bytes());
    for w in wraps {
        out.extend_from_slice(&w.epk);
        out.extend_from_slice(&w.commit);
        out.extend_from_slice(&(w.ct.len() as u32).to_be_bytes());
        out.extend_from_slice(&w.ct);
    }
    out
}

/// Parse a fan-out blob. Bounds-checked — a truncated or corrupt blob fails rather than panicking.
pub fn fanout_from_bytes(bytes: &[u8]) -> Result<(u64, Vec<FanoutWrap>), String> {
    let mut p = 0usize;
    let take = |p: &mut usize, n: usize| -> Result<&[u8], String> {
        if *p + n > bytes.len() {
            return Err("fanout: truncated".into());
        }
        let s = &bytes[*p..*p + n];
        *p += n;
        Ok(s)
    };
    if take(&mut p, 4)? != FANOUT_TAG {
        return Err("fanout: bad tag".into());
    }
    let epoch = u64::from_be_bytes(take(&mut p, 8)?.try_into().unwrap());
    let count = u32::from_be_bytes(take(&mut p, 4)?.try_into().unwrap()) as usize;
    // A fleet is a person's devices — a four-figure count is adversarial. Reject before allocating/looping.
    if count > 1024 {
        return Err("fanout: implausible wrap count".into());
    }
    let mut wraps = Vec::with_capacity(count);
    for _ in 0..count {
        let epk: [u8; 32] = take(&mut p, 32)?.try_into().unwrap();
        let commit: [u8; 32] = take(&mut p, 32)?.try_into().unwrap();
        let ct_len = u32::from_be_bytes(take(&mut p, 4)?.try_into().unwrap()) as usize;
        let ct = take(&mut p, ct_len)?.to_vec();
        wraps.push(FanoutWrap { epk, commit, ct });
    }
    Ok((epoch, wraps))
}

// ── Fan-out transport + rotation ──

/// Publish a fan-out to the always-online slot. Device-signed envelope (ke/ge) so FGTW checks the writer against the folded fleet chain; the epoch inside the blob drives the worker's monotonic guard.
pub fn post_fanout(
    handle_proof: &[u8; 32],
    device_key: &Keypair,
    epoch: u64,
    wraps: &[FanoutWrap],
) -> Result<(), String> {
    let mut section = vsf::VsfSection::new("fanout_put");
    section.add_field("hp", VsfType::hP(handle_proof.to_vec()));
    section.add_field("bl", VsfType::ge(fanout_to_bytes(epoch, wraps)));
    let unsigned = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .signed_only(VsfType::ke(device_key.public.to_bytes().to_vec()))
        .add_section_direct(section)
        .build()
        .map_err(|e| format!("fanout_put build: {e}"))?;
    let signed = vsf::verification::sign_file(unsigned, device_key.secret.as_bytes())
        .map_err(|e| format!("fanout_put sign: {e}"))?;
    let resp = crate::network::http::blocking()
        .post(FGTW_URL)
        .timeout(std::time::Duration::from_secs(15))
        .header("Content-Type", "application/octet-stream")
        .body(signed)
        .send()
        .map_err(|e| format!("fanout_put send: {e}"))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("fanout_put http {}", resp.status()))
    }
}

/// Fetch the current fan-out (epoch + wraps), or None if none published yet.
pub fn fetch_fanout(handle_proof: &[u8; 32]) -> Result<Option<(u64, Vec<FanoutWrap>)>, String> {
    let mut section = vsf::VsfSection::new("fanout_get");
    section.add_field("hp", VsfType::hP(handle_proof.to_vec()));
    let req = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_section_direct(section)
        .build()
        .map_err(|e| format!("fanout_get build: {e}"))?;
    let resp = crate::network::http::blocking()
        .post(FGTW_URL)
        .timeout(std::time::Duration::from_secs(15))
        .header("Content-Type", "application/octet-stream")
        .body(req)
        .send()
        .map_err(|e| format!("fanout_get send: {e}"))?;
    if resp.status().as_u16() == 404 {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(format!("fanout_get http {}", resp.status()));
    }
    let bytes = resp.bytes().map_err(|e| format!("fanout_get read: {e}"))?;
    let (_, header_end) =
        vsf::VsfHeader::decode(&bytes).map_err(|e| format!("fanout header: {e}"))?;
    let mut ptr = header_end;
    let stored =
        vsf::VsfSection::parse(&bytes, &mut ptr).map_err(|e| format!("fanout section: {e}"))?;
    match stored.get_field("bl").and_then(|f| f.values.first()) {
        Some(VsfType::ge(b)) => Ok(Some(fanout_from_bytes(b)?)),
        _ => Ok(None),
    }
}

/// Rotate (or first-establish) the fleet key: mint a FRESH key, seal it to the current `members`, and publish at `stored_epoch + 1`. Returns `(new_epoch, new_key)`. This is the one operation for both genesis-establish and every membership-change rotation — a removed device just isn't in `members`.
pub fn rotate_fleet_key(
    handle_proof: &[u8; 32],
    device_key: &Keypair,
    members: &[[u8; 32]],
) -> Result<(u64, [u8; 32]), String> {
    let current = fetch_fanout(handle_proof)?.map(|(e, _)| e).unwrap_or(0);
    let epoch = current + 1;
    let key = new_fleet_key();
    let wraps = fanout_seal(handle_proof, epoch, &key, members)?;
    post_fanout(handle_proof, device_key, epoch, &wraps)?;
    Ok((epoch, key))
}

/// Recover the current fleet key from the always-online fan-out with this device's key alone (no live sibling). None if this device isn't in the current member set (removed, or never joined), or no fan-out exists yet.
pub fn recover_fleet_key(
    handle_proof: &[u8; 32],
    device_key: &Keypair,
) -> Result<Option<[u8; 32]>, String> {
    match fetch_fanout(handle_proof)? {
        Some((epoch, wraps)) => Ok(fanout_open(handle_proof, epoch, &wraps, device_key)),
        None => Ok(None),
    }
}

/// Recover the current fleet key, or ESTABLISH epoch 1 if no fan-out exists yet (the genesis founder). Handles the establish race: if another device published epoch 1 first, recover theirs instead. Returns None on network failure or if this device can't recover (not a current member / no fleet yet).
pub fn recover_or_establish_fleet_key(
    handle_proof: &[u8; 32],
    device_key: &Keypair,
) -> Result<Option<[u8; 32]>, String> {
    if let Some(k) = recover_fleet_key(handle_proof, device_key)? {
        return Ok(Some(k));
    }
    // No fan-out yet — establish it, sealed to the CURRENT member set (just the founder at genesis).
    let members = current_members(handle_proof)?;
    if members.is_empty() {
        return Ok(None);
    }
    match rotate_fleet_key(handle_proof, device_key, &members) {
        Ok((_, k)) => Ok(Some(k)),
        // Lost the establish race → recover the key the winner published.
        Err(_) => recover_fleet_key(handle_proof, device_key),
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

    #[test]
    fn live_word_check_flags_typos_but_tolerates_prefixes() {
        // A real entry (generated words) passes at every truncation point.
        let words = pair_words(&[0xA5; 32]);
        assert_eq!(first_bad_pair_word(&words), None);
        for cut in 1..words.len() {
            if words.is_char_boundary(cut) {
                assert_eq!(first_bad_pair_word(&words[..cut]), None, "prefix at {cut} flagged");
            }
        }
        // Classic misspellings flag as soon as they're impossible prefixes, in either entry mode.
        assert_eq!(first_bad_pair_word("contraversy "), Some("contraversy".into()));
        assert_eq!(first_bad_pair_word("SpontaniousAble"), Some("spontanious".into()));
        // An in-progress word that is still a valid prefix stays green.
        let first = std::str::from_utf8(voca::FULL.alphabet[100]).unwrap();
        assert_eq!(first_bad_pair_word(&first[..2]), None);
        // Completeness gate: a full generated entry is complete; the same entry cut mid-last-word is NOT, even tho the token count already reads 23.
        assert!(pair_entry_complete(&words));
        let cut = words.len() - 2;
        assert!(!pair_entry_complete(&words[..cut]));
        assert_eq!(pair_word_tokens(&words[..cut]), PAIR_WORD_COUNT);
    }

    #[test]
    fn device_name_default_is_two_stable_words() {
        let a = device_name_default(&[7u8; 32]);
        assert_eq!(a, device_name_default(&[7u8; 32]), "deterministic");
        assert_eq!(pair_word_tokens(&a), 2, "always exactly two words: {a}");
        assert_ne!(a, device_name_default(&[8u8; 32]), "distinct secrets, distinct names");
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
    fn pair_words_fixed_width_round_trip() {
        // A normal key, an all-zero key (maximum padding), and a leading-zero key (partial padding) all render EXACTLY PAIR_WORD_COUNT words and decode back byte-identical.
        let mut leading_zero = [0x42u8; 32];
        leading_zero[0] = 0;
        leading_zero[1] = 0;
        for key in [[0x9au8; 32], [0u8; 32], leading_zero, rand::random()] {
            let words = pair_words(&key);
            assert_eq!(pair_word_tokens(&words), PAIR_WORD_COUNT, "fixed width");
            assert_eq!(words_to_pair_pubkey(&words).unwrap(), key, "round trip");
        }
        // The counter mirrors voca's tokenizer for both entry styles.
        let words = pair_words(&[7u8; 32]);
        let spaced: Vec<String> = {
            let mut v = Vec::new();
            let mut cur = String::new();
            for c in words.chars() {
                if c.is_ascii_uppercase() && !cur.is_empty() {
                    v.push(std::mem::take(&mut cur));
                }
                cur.push(c);
            }
            v.push(cur);
            v
        };
        assert_eq!(spaced.len(), PAIR_WORD_COUNT);
        assert_eq!(pair_word_tokens(&spaced.join(" ")), PAIR_WORD_COUNT);
        assert_eq!(words_to_pair_pubkey(&spaced.join(" ")).unwrap(), [7u8; 32]);
    }

    #[test]
    fn words_to_pair_pubkey_rejects_bad_entries() {
        // Wrong word count (incomplete entry) is rejected before any decode.
        assert!(words_to_pair_pubkey("justOneWord").is_err());
        // 23 copies of the LAST alphabet word decode above 2^256 — a valid-looking entry that isn't a key.
        let last = std::str::from_utf8(voca::FULL.alphabet[voca::FULL.base() - 1]).unwrap();
        let too_big = vec![last; PAIR_WORD_COUNT].join(" ");
        assert!(words_to_pair_pubkey(&too_big).is_err());
        // A garbage token fails the decode loudly.
        let mut words = pair_words(&[1u8; 32]);
        words.push_str("Zzzqx");
        assert!(words_to_pair_pubkey(&words).is_err());
    }

    #[test]
    fn pair_request_and_matched_signatures_verify_and_bind() {
        let pairing = new_pairing_id();
        let member = key(4);
        let dk = pk(&key(5));
        let t = 12345i64;
        // Ownership proof: verifies under the pairing pubkey, breaks under a different device or identity.
        let sig = pairing.sign(&pair_request_signing_bytes(&HP, &dk, t));
        let vk = VerifyingKey::from_bytes(&pairing.public.to_bytes()).unwrap();
        assert!(vk.verify(&pair_request_signing_bytes(&HP, &dk, t), &sig).is_ok());
        assert!(vk.verify(&pair_request_signing_bytes(&HP, &pk(&key(6)), t), &sig).is_err());
        assert!(vk.verify(&pair_request_signing_bytes(&[9u8; 32], &dk, t), &sig).is_err());
        // Matched flag: verifies under the member's device key, breaks for a different pairing pubkey.
        let pp = pairing.public.to_bytes();
        let msig = member.sign(&pair_matched_signing_bytes(&HP, &pp, t));
        let mvk = VerifyingKey::from_bytes(&pk(&member)).unwrap();
        assert!(mvk.verify(&pair_matched_signing_bytes(&HP, &pp, t), &msig).is_ok());
        assert!(mvk.verify(&pair_matched_signing_bytes(&HP, &[8u8; 32], t), &msig).is_err());
    }

    /// End-to-end against LIVE fgtw.org: genesis a fresh fleet, run the full v1 device-ADD handshake (words → request → match → matched flag → bind → rotate → recover), and confirm the new device folds in with the fleet key.
    /// Ignored by default (hits the network + leaves ephemeral random-key objects); run with `--ignored`.
    #[test]
    #[ignore = "hits live fgtw.org"]
    fn live_device_add_round_trip() {
        let handle_proof: [u8; 32] = rand::random();
        let identity_seed: [u8; 32] = rand::random();
        let member = Keypair::from_seed(&rand::random::<[u8; 32]>());
        let newdev = Keypair::from_seed(&rand::random::<[u8; 32]>());

        // Existing device claims the fleet (identity-signed genesis) and establishes the fan-out.
        ensure_member(&member, &handle_proof, &identity_seed).expect("genesis");
        assert_eq!(current_members(&handle_proof).unwrap(), vec![member.public.to_bytes()]);
        let (_, k1) = rotate_fleet_key(&handle_proof, &member, &[member.public.to_bytes()]).expect("establish");

        // New device: mint a pairing identity, display its words, post the signed request.
        let pairing = new_pairing_id();
        let words = pair_words(&pairing.public.to_bytes());
        post_pairing_request(&pairing, &newdev.public.to_bytes(), &handle_proof).expect("post request");

        // Existing device: the user types the words; decode → fetch → the request matches and its ownership signature verifies.
        let typed = words_to_pair_pubkey(&words).expect("decode typed words");
        let req = fetch_pairing_request(&handle_proof).expect("fetch").expect("a pending request");
        assert_eq!(req.pairing_pubkey, typed);
        assert_eq!(req.device_pubkey, newdev.public.to_bytes());

        // Existing device posts the matched flag; the new device's ready light verifies it against the member set.
        post_pair_matched(&member, &handle_proof, &req.pairing_pubkey).expect("post matched");
        let members = current_members(&handle_proof).unwrap();
        assert!(poll_pair_matched(&handle_proof, &pairing.public.to_bytes(), &members).unwrap());
        // A different pairing key sees no match (a stranger can't flip the light).
        let other = new_pairing_id();
        assert!(!poll_pair_matched(&handle_proof, &other.public.to_bytes(), &members).unwrap());

        // Bind + rotate: the new device is a member and recovers the NEW epoch key with its own device key.
        bind_device(&member, &handle_proof, req.device_pubkey).expect("bind");
        let members2 = current_members(&handle_proof).unwrap();
        assert!(members2.contains(&newdev.public.to_bytes()));
        let (_, k2) = rotate_fleet_key(&handle_proof, &member, &members2).expect("rotate");
        assert_ne!(k2, k1);
        assert_eq!(recover_fleet_key(&handle_proof, &newdev).unwrap().unwrap(), k2);
    }

    #[test]
    fn fanout_seals_to_each_member_and_excludes_removed() {
        let a = key(1);
        let b = key(2);
        let c = key(3);
        let removed = key(9);
        let members = vec![pk(&a), pk(&b), pk(&c)];
        let hp = [0x11u8; 32];
        let epoch = 5u64;
        let fleet_key = new_fleet_key();
        let wraps = fanout_seal(&hp, epoch, &fleet_key, &members).unwrap();
        assert_eq!(wraps.len(), 3);
        // Every current member recovers the exact key with its own device key (no live sibling).
        for kp in [&a, &b, &c] {
            assert_eq!(fanout_open(&hp, epoch, &wraps, kp).expect("member opens"), fleet_key);
        }
        // A device not in the member set (removed, or never joined) cannot — removal removes.
        assert!(fanout_open(&hp, epoch, &wraps, &removed).is_none());
        // Bound to (fleet, epoch): a wrap won't open under a different handle_proof or epoch (no cross-fleet / cross-epoch splicing).
        assert!(fanout_open(&[0x22u8; 32], epoch, &wraps, &a).is_none());
        assert!(fanout_open(&hp, epoch + 1, &wraps, &a).is_none());
        // Serialize round-trips and the recovered blob still opens.
        let bytes = fanout_to_bytes(epoch, &wraps);
        let (got_epoch, back) = fanout_from_bytes(&bytes).unwrap();
        assert_eq!(got_epoch, epoch);
        assert_eq!(back, wraps);
        assert_eq!(fanout_open(&hp, epoch, &back, &a).unwrap(), fleet_key);
        assert!(fanout_from_bytes(&bytes[..bytes.len() - 5]).is_err());
        // A tampered wrap fails its AEAD tag (no silent wrong key).
        let mut tampered = wraps.clone();
        *tampered[0].ct.last_mut().unwrap() ^= 1;
        assert!(fanout_open(&hp, epoch, &tampered[..1], &a).is_none());
        // A low-order (all-zero) epk is rejected by the contributory-DH check, not opened.
        let mut loword = wraps.clone();
        loword[0].epk = [0u8; 32];
        assert!(fanout_open(&hp, epoch, &loword[..1], &a).is_none());
        // Wrap-count sanity: an implausible count is rejected before allocation.
        let mut huge = fanout_to_bytes(epoch, &wraps);
        huge[12..16].copy_from_slice(&2000u32.to_be_bytes());
        assert!(fanout_from_bytes(&huge).is_err());
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
    fn live_fanout_rotation_round_trip() {
        let handle_proof: [u8; 32] = rand::random();
        let identity_seed: [u8; 32] = rand::random();
        let a = Keypair::from_seed(&rand::random::<[u8; 32]>());
        let b = Keypair::from_seed(&rand::random::<[u8; 32]>());

        // A claims the fleet and establishes the first fan-out (epoch 1, sealed to [A]).
        ensure_member(&a, &handle_proof, &identity_seed).expect("genesis");
        let (e1, k1) = rotate_fleet_key(&handle_proof, &a, &[pk(&a)]).expect("establish");
        assert_eq!(e1, 1);
        assert_eq!(recover_fleet_key(&handle_proof, &a).unwrap().unwrap(), k1);
        // B isn't a member yet → cannot recover.
        assert!(recover_fleet_key(&handle_proof, &b).unwrap().is_none());

        // A adds B, then rotates to [A, B]: a fresh key both can open.
        bind_device(&a, &handle_proof, pk(&b)).expect("bind B");
        let members2 = current_members(&handle_proof).unwrap();
        let (e2, k2) = rotate_fleet_key(&handle_proof, &a, &members2).expect("rotate to A,B");
        assert_eq!(e2, 2);
        assert_ne!(k2, k1);
        assert_eq!(recover_fleet_key(&handle_proof, &a).unwrap().unwrap(), k2);
        assert_eq!(recover_fleet_key(&handle_proof, &b).unwrap().unwrap(), k2);

        // A removes B, rotates to [A]: A gets the new key, B (removed) cannot — removal removes.
        unbind_device(&a, &handle_proof, pk(&b)).expect("remove B");
        let members3 = current_members(&handle_proof).unwrap();
        let (e3, k3) = rotate_fleet_key(&handle_proof, &a, &members3).expect("rotate to A");
        assert_eq!(e3, 3);
        assert_eq!(recover_fleet_key(&handle_proof, &a).unwrap().unwrap(), k3);
        assert!(recover_fleet_key(&handle_proof, &b).unwrap().is_none());

        // A stale rotation (epoch ≤ stored) is rejected by the worker's monotonic guard.
        let stale = fanout_seal(&handle_proof, 3, &new_fleet_key(), &members3).unwrap();
        assert!(post_fanout(&handle_proof, &a, 3, &stale).is_err());
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

#[cfg(test)]
mod live_smoke {
    use super::*;
    #[test]
    #[ignore = "hits live fgtw.org — run explicitly"]
    fn pack_put_smoke() {
        let member = Keypair::from_seed(&[0xEE; 32]);
        let r = post_pair_matched(&member, &[0xDD; 32], &[0xCC; 32]);
        eprintln!("pack_put smoke: {r:?}");
        assert!(r.is_ok(), "{r:?}");
    }
}
