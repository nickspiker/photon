//! Keyring — client side of the multi-device identity credential.
//!
//! One identity (handle) binds many devices (the fleet). The PUBLIC credential is a single constant-size 32-byte Merkle root over the fleet's leaves (`ihi::keyring`), so the device COUNT is hidden from the public and from FGTW — the root is the same size for a 1-device fleet or a billion-device fleet. The per-device leaf list is PRIVATE to the fleet (synced device-to-device later via the deferred CLUTCH path); a device proves membership with a fixed-size inclusion proof against the published root, revealing only "a member," never the count.
//!
//! ## Pubkey-based leaves (v0)
//!
//! `leaf = ihi::keyring::leaf(kind, device_pubkey, handle_proof)`. The leaf is keyed on the device's PUBLIC key (not its secret) so the VERIFIER (FGTW) can RECOMPUTE the leaf from the pubkey that signed an op and check its inclusion against the root — binding the signer to the leaf. (A secret-based leaf would be unguessable but unverifiable by recompute, and a bare inclusion proof wouldn't bind it to the signer; that
//! returns with the deferred ZK/accumulator egg.) `spaghettify`'s lossiness still makes leaves mutually
//! unlinkable across identities (different `handle_proof`), so cross-handle correlation is impossible; the residual is that someone who knows your handle AND a device's pubkey can test that device's membership.
//!
//! Each device contributes one leaf PER KIND it's authorised for — a DEVICE leaf (fleet membership / add-remove authority), an AVATAR leaf (avatar-write), a BLOB leaf (blob-write) — all in the SAME tree, so one root commits to every authority. A write proves the matching kind's leaf; an add/remove op proves the publisher's DEVICE leaf against the *previous* root ("any current member may add/remove").
//!
//! ## Versioning: forensic, NOT a fork (AGENT.md "No Fork Bullshit")
//!
//! [`KR_VER`] stamps the format for forensics. NOT a branch: a client/worker that sees a `kr_ver` it doesn't implement FAILS LOUD. Evolving the format (the Ristretto / accumulator eggs later) = a new `kr_ver` shipped to client AND worker atomically, registry nuked, move on.
//!
//! ## Wire contract (client builds, FGTW `handle_keyring_op` verifies — keep both sides identical)
//!
//! A `keyring` op is a complete signed VSF file (header `ke` = device pubkey, `ge` = file signature via `sign_file`, same integrity shape as `announce`), section `keyring`:
//! - `op`     `d`      one of `genesis` / `add` / `remove`.
//! - `hp`     `hP[32]` the handle_proof (the PUBLIC network id) — which keyring this targets, and the identity-binding component of every leaf. (NOT identity_seed: that's semi-private and FGTW doesn't have it; handle_proof is what FGTW already holds and can recompute leaves from.)
//! - `root`   `hb[32]` the NEW Merkle root being published.
//! - `kr_ver` `u`      the format version ([`KR_VER`]); a mismatch is a hard reject.
//! - `chal`   `hb[32]` the FGTW challenge hash this op answers (anti-replay; covered by the file signature).
//! - add/remove only: `pidx` `u` + `pnode` (multi `hb[32]`) — inclusion proof that the PUBLISHING device's DEVICE leaf was in the *previous* root. genesis carries no proof (first-come claim, like the handle).
//!
//! FGTW stores only `{kr_ver, root, root_sig (= the file sig), root_ts}` at `keyring/<handle_proof_b64>` — no list, no count. Rollback/replay guard: FGTW rejects a `root_ts` ≤ the stored one (same monotonic pattern as `avatar_put`). FGTW recomputes the publisher's DEVICE leaf from the header pubkey + `hp` to verify the proof.

use crate::network::fgtw::Keypair;
use ihi::keyring::{self as kr, InclusionProof};
use vsf::VsfType;

/// Credential format version. Forensic stamp; a mismatch is a hard error, never a branch (see module docs).
pub const KR_VER: u64 = 0;

const FGTW_URL: &str = "https://fgtw.org";

/// Derive a fleet leaf of `kind` for `binding_pubkey` under `handle_proof`. v0 feeds the device/avatar/blob PUBLIC key (pubkey-based, so FGTW can recompute-and-verify). Pass-thru to `ihi::keyring::leaf` so callers needn't import ihi.
pub fn leaf(kind: u8, binding_pubkey: &[u8; 32], handle_proof: &[u8; 32]) -> [u8; 32] {
    kr::leaf(kind, binding_pubkey, handle_proof)
}

/// The Merkle root over an ordered leaf set — the constant-size public credential. Pass-thru.
pub fn root_of(leaves: &[[u8; 32]]) -> [u8; 32] {
    kr::merkle_root(leaves)
}

/// Inclusion proof for the leaf at `index`. Pass-thru.
pub fn proof_for(leaves: &[[u8; 32]], index: usize) -> InclusionProof {
    kr::inclusion_proof(leaves, index)
}

/// Attach a membership inclusion proof (`pidx` + `pnode` multi-value) to a write's VSF section, so FGTW can recompute the writer's leaf and verify it against the published root. Same field shape the `keyring` add/remove op uses, so the worker reads one proof format everywhere.
pub fn add_proof_fields(section: &mut vsf::VsfSection, idx: usize, proof: &InclusionProof) {
    section.add_field("pidx", VsfType::u(idx, false));
    section.add_field_multi(
        "pnode",
        proof.iter().map(|n| VsfType::hb(n.to_vec())).collect(),
    );
}

/// Re-export the leaf-kind tags so callers use one vocabulary.
pub use ihi::keyring::kind;

/// Homogeneous device-leaf model (v0): the keyring tree commits ONLY to per-device DEVICE leaves. Every write (avatar, blob, keyring add/remove) is signed by the DEVICE key and proves the SAME device leaf — authorisation is uniformly "is the signing device in the fleet?". An avatar keeps its own keypair only
/// for the avatar VSF's self-contained content signature, NOT for authorisation. This keeps the tree
/// homogeneous, so any write path rebuilds it from just (device_pubkey, handle_proof) — no avatar pubkey, no identity_seed coupling. A single-device fleet's tree is `[device_leaf]` at index 0.
const IDX_DEVICE: usize = 0;

/// This device's DEVICE leaf — its fleet-membership commitment. Deterministic from its public key + handle_proof, so a single-device fleet needs no persisted leaf list. (Multi-device fleets hold the full ordered device-leaf list, synced via the deferred CLUTCH path.)
fn solo_leaves(device_pubkey: &[u8; 32], handle_proof: &[u8; 32]) -> [[u8; 32]; 1] {
    [kr::leaf(kind::DEVICE, device_pubkey, handle_proof)]
}

/// Process-lifetime cache of handle_proofs whose keyring genesis we've already ensured, so frequent writes (e.g. contacts blob sync) don't re-attempt genesis every call. A tiny set (one entry per identity this process runs) — a linear-scanned Vec, not a HashMap.
fn genesis_seen() -> &'static std::sync::Mutex<Vec<[u8; 32]>> {
    static SEEN: std::sync::OnceLock<std::sync::Mutex<Vec<[u8; 32]>>> = std::sync::OnceLock::new();
    SEEN.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// Ensure this device's single-device keyring root is published, then return the inclusion proof a device-signed write attaches to prove fleet membership (always the DEVICE leaf at index 0).
///
/// v0 single-device: the device leaf is deterministic, so we (best-effort, once per process per handle_proof) publish genesis over `[device_leaf]` and recompute the proof on demand. Genesis is idempotent in effect — if the keyring already exists with this same deterministic root, the proof still verifies; a "genesis already exists" rejection is therefore success, not failure. Returns the `(index, proof)` to attach, or an error only if NO usable root could be ensured (genuine network failure with no prior keyring).
pub fn ensure_keyring_and_prove(
    device_key: &Keypair,
    handle_proof: &[u8; 32],
) -> Result<(usize, InclusionProof), String> {
    let device_pubkey = device_key.public.to_bytes();
    let leaves = solo_leaves(&device_pubkey, handle_proof);

    // Publish genesis once per process per identity (best-effort: a prior root with the same deterministic value is fine — the proof below verifies against it either way).
    let already = genesis_seen().lock().map(|s| s.contains(handle_proof)).unwrap_or(false);
    if !already {
        match genesis(device_key, handle_proof, &leaves) {
            KeyringOpResult::Ok | KeyringOpResult::Rejected(_) => {
                if let Ok(mut s) = genesis_seen().lock() {
                    if !s.contains(handle_proof) {
                        s.push(*handle_proof);
                    }
                }
            }
            KeyringOpResult::Error(e) => return Err(format!("keyring genesis: {e}")),
        }
    }

    Ok((IDX_DEVICE, proof_for(&leaves, IDX_DEVICE)))
}

/// Outcome of a keyring op round-trip.
#[derive(Debug)]
pub enum KeyringOpResult {
    Ok,
    /// Rejected by FGTW (not a current member, stale root_ts, bad signature, version mismatch).
    Rejected(String),
    /// Network / build error before a verdict.
    Error(String),
}

/// Fetch a fresh challenge hash from FGTW (anti-replay nonce, bound into the op's file signature). Mirrors the announce flow in `bootstrap.rs`: POST a `challenge` section, read the signed response's provenance.
fn fetch_challenge() -> Result<[u8; 32], String> {
    let req = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .add_section("challenge", vec![])
        .build()
        .map_err(|e| format!("build challenge req: {e}"))?;
    let resp = crate::network::http::blocking()
        .post(FGTW_URL)
        .timeout(std::time::Duration::from_secs(10))
        .header("Content-Type", "application/octet-stream")
        .body(req)
        .send()
        .map_err(|e| format!("challenge send: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("challenge http {}", resp.status()));
    }
    let bytes = resp.bytes().map_err(|e| format!("challenge read: {e}"))?;
    let (header, _) =
        vsf::file_format::VsfHeader::decode(&bytes).map_err(|e| format!("challenge decode: {e}"))?;
    match &header.provenance_hash {
        VsfType::hp(h) if h.len() == 32 => Ok(h.as_slice().try_into().unwrap()),
        _ => Err("challenge missing provenance hash".into()),
    }
}

/// Build a signed `keyring` op VSF per the module's wire contract, then POST it. `proof` is `None` for genesis, `Some((index, nodes))` for add/remove (the publisher's DEVICE-leaf inclusion proof vs the previous root).
fn build_and_send(
    op: &str,
    device_key: &Keypair,
    handle_proof: &[u8; 32],
    new_root: &[u8; 32],
    challenge: [u8; 32],
    proof: Option<(usize, &InclusionProof)>,
) -> KeyringOpResult {
    // Common section: build it directly (add_section takes single-value fields, but add/remove needs the repeated `pnode` multi-value field, so use VsfSection for both paths to keep one code shape).
    let mut section = vsf::VsfSection::new("keyring");
    section.add_field("op", VsfType::d(op.to_string()));
    section.add_field("hp", VsfType::hP(handle_proof.to_vec()));
    section.add_field("root", VsfType::hb(new_root.to_vec()));
    section.add_field("kr_ver", VsfType::u(KR_VER as usize, false));
    section.add_field("chal", VsfType::hb(challenge.to_vec()));
    if let Some((idx, nodes)) = proof {
        section.add_field("pidx", VsfType::u(idx, false));
        section.add_field_multi(
            "pnode",
            nodes.iter().map(|n| VsfType::hb(n.to_vec())).collect(),
        );
    }

    let unsigned = match vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .signed_only(VsfType::ke(device_key.public.to_bytes().to_vec()))
        .add_section_direct(section)
        .build()
    {
        Ok(b) => b,
        Err(e) => return KeyringOpResult::Error(format!("build {op}: {e}")),
    };

    let signed = match vsf::verification::sign_file(unsigned, device_key.secret.as_bytes()) {
        Ok(b) => b,
        Err(e) => return KeyringOpResult::Error(format!("sign {op}: {e}")),
    };

    #[cfg(feature = "development")]
    crate::log(&crate::network::inspect::vsf_inspect(&signed, "FGTW", "TX", op));

    let resp = match crate::network::http::blocking()
        .post(FGTW_URL)
        .timeout(std::time::Duration::from_secs(15))
        .header("Content-Type", "application/octet-stream")
        .body(signed)
        .send()
    {
        Ok(r) => r,
        Err(e) => return KeyringOpResult::Error(format!("{op} send: {e}")),
    };
    if resp.status().is_success() {
        KeyringOpResult::Ok
    } else {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        KeyringOpResult::Rejected(format!("{op} http {status}: {body}"))
    }
}

/// Publish the genesis root for a new identity (first device claims the handle — first-come, like the handle). `leaves` is the initial fleet's leaf set in tree order (v0: this one device's DEVICE/AVATAR/BLOB leaves).
pub fn genesis(device_key: &Keypair, handle_proof: &[u8; 32], leaves: &[[u8; 32]]) -> KeyringOpResult {
    let challenge = match fetch_challenge() {
        Ok(c) => c,
        Err(e) => return KeyringOpResult::Error(e),
    };
    let root = kr::merkle_root(leaves);
    build_and_send("genesis", device_key, handle_proof, &root, challenge, None)
}

/// Publish a new root that ADDs a device (its leaves already appended to `new_leaves`). `my_index`/`proof` is the publishing device's DEVICE-leaf inclusion proof against the PREVIOUS root.
pub fn add(
    device_key: &Keypair,
    handle_proof: &[u8; 32],
    new_leaves: &[[u8; 32]],
    my_index: usize,
    proof: &InclusionProof,
) -> KeyringOpResult {
    let challenge = match fetch_challenge() {
        Ok(c) => c,
        Err(e) => return KeyringOpResult::Error(e),
    };
    let root = kr::merkle_root(new_leaves);
    build_and_send("add", device_key, handle_proof, &root, challenge, Some((my_index, proof)))
}

/// Publish a new root that REMOVEs a device (its leaves dropped from `new_leaves`). `my_index`/`proof` is the publishing device's DEVICE-leaf inclusion proof against the PREVIOUS root. Revocation sticks: FGTW checks future writes against THIS root, which no longer contains the removed device's leaves.
pub fn remove(
    device_key: &Keypair,
    handle_proof: &[u8; 32],
    new_leaves: &[[u8; 32]],
    my_index: usize,
    proof: &InclusionProof,
) -> KeyringOpResult {
    let challenge = match fetch_challenge() {
        Ok(c) => c,
        Err(e) => return KeyringOpResult::Error(e),
    };
    let root = kr::merkle_root(new_leaves);
    build_and_send("remove", device_key, handle_proof, &root, challenge, Some((my_index, proof)))
}
