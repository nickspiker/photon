//! FGTW client adapter — photon's binding of the shared `fgtw` crate to its own HTTP + storage stack.
//!
//! All the substrate now lives in the crate, re-exported here so photon's `crate::network::fgtw::fleet::*` call sites are unchanged:
//! `fgtw::fleet` (the membership chain), `fgtw::fanout` (fan-out crypto), `fgtw::fstate` (roster codec), `fgtw::pair` (pairing words), and `fgtw::client` (the fetch-then-sign oracle).
//! What's left here is the *binding*: [`PhotonTransport`] (FGTW's HTTP over photon's warm-TLS pool + short error UX) and [`PhotonSealer`] (roster AEAD over `kete`), plus thin same-signature wrappers that inject them — so the crate stays reqwest-free and photon keeps its own network stack.

// ── Sunset tripwire for the v1 fleet-op verify path (docs/identity-succession.md) ──
// New fleets found under v2 (no identity_sig); the v1 path in fgtw/src/fleet.rs (verify_identity_binding, the fold genesis arm accepting a present identity_sig, and its encode/parse) stays ONLY to fold chains founded before the cutover. A chain is immutable and append-only, so it becomes v2 only by re-founding via the succession re-pin flow, never on its own — once every peer's chain is v2, the v1 path is dead weight. This const FAILS THE BUILD at the sunset version unless someone confirmed all peers are v2 and deleted it (flip V1_FLEET_VERIFY_PRESENT to false), or consciously bumped the deadline. There is no other way to forget it.
const fn parse_u(s: &str) -> usize {
    let (b, mut n, mut i) = (s.as_bytes(), 0usize, 0usize);
    while i < b.len() {
        n = n * 10 + (b[i] - b'0') as usize;
        i += 1;
    }
    n
}
const CURRENT_VERSION: (usize, usize, usize) = (
    parse_u(env!("CARGO_PKG_VERSION_MAJOR")),
    parse_u(env!("CARGO_PKG_VERSION_MINOR")),
    parse_u(env!("CARGO_PKG_VERSION_PATCH")),
);
/// Releases bump the MINOR (deploy.sh), so this is ~twelve releases past 0.58.0. A knob — bump it only after a conscious "the fleet's v2 migration isn't done yet" decision.
const V1_FLEET_VERIFY_SUNSET: (usize, usize, usize) = (0, 70, 0);
/// Flip to `false` in the SAME change that deletes the v1 fleet-op verify path from fgtw/src/fleet.rs.
const V1_FLEET_VERIFY_PRESENT: bool = true;
const fn ver_ge(a: (usize, usize, usize), b: (usize, usize, usize)) -> bool {
    a.0 > b.0 || (a.0 == b.0 && (a.1 > b.1 || (a.1 == b.1 && a.2 >= b.2)))
}
const _: () = assert!(
    !(V1_FLEET_VERIFY_PRESENT && ver_ge(CURRENT_VERSION, V1_FLEET_VERIFY_SUNSET)),
    "v1 fleet-op verify path reached its sunset version: confirm every peer's chain is v2 and delete it from fgtw/src/fleet.rs (set V1_FLEET_VERIFY_PRESENT = false), or consciously bump V1_FLEET_VERIFY_SUNSET."
);

pub use fgtw::fanout::{
    fanout_from_bytes, fanout_needs_rotation, fanout_open, fanout_seal, fanout_to_bytes,
    new_fleet_key, FanoutWrap,
};
pub use fgtw::fleet::{
    bindreq_signing_bytes, et_to_osc, scheme, BindRequest, Egg, FleetOp, FoldError, MembershipBlob,
    OpKind, SuccessorRecord, BINDREQ_FRESH_OSC, CONSENT_WINDOW_OSC,
};
pub use fgtw::fstate::{merge_rosters, roster_from_bytes, roster_to_bytes, RosterEntry};
pub use fgtw::pair::{
    device_name_default, first_bad_pair_word, keyed_pseudonym, masked_device_words, pair_word_list,
    pair_word_tokens, pair_words, parse_pair_event, word_mask, PAIR_WORD_COUNT,
};

use crate::network::fgtw::Keypair;
use fgtw::client::{FgtwResponse, FgtwTransport, FleetSealer};

use crate::network::http::SEED_HTTPS as FGTW_URL;

// ── Transport injection: the crate owns the FGTW protocol; photon supplies the raw HTTP (pooled reqwest, warm TLS, short "No connection to FGTW" errors) and the roster AEAD (kete). ──

/// Photon's HTTP reach to FGTW: POST via the shared pooled client, hand the crate back `{status, body}` so it owns the `error`-frame reason / success interpretation.
struct PhotonTransport;
impl FgtwTransport for PhotonTransport {
    fn post(&self, body: Vec<u8>) -> Result<FgtwResponse, String> {
        let resp = crate::network::http::blocking()
            .post(FGTW_URL)
            .timeout(std::time::Duration::from_secs(15))
            .header("Content-Type", "application/octet-stream")
            .body(body)
            .send()
            .map_err(|e| crate::network::http::short_send_error("reach FGTW", &e))?;
        let status = resp.status().as_u16();
        let body = resp
            .bytes()
            .map_err(|e| crate::network::http::short_send_error("reach FGTW", &e))?
            .to_vec();
        Ok(FgtwResponse { status, body })
    }
}

/// Photon's roster AEAD: the same `kete` per-key ChaCha20-Poly1305 the vault uses, so fleet-state ciphertext stays byte-identical.
struct PhotonSealer;
impl FleetSealer for PhotonSealer {
    fn seal(&self, plaintext: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, String> {
        kete::encrypt_bytes(plaintext, key)
    }
    fn open(&self, sealed: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, String> {
        kete::decrypt_bytes(sealed, key)
    }
}

// ── Oracle wrappers: identical signatures to the pre-migration free functions, transport/sealer injected. ──

/// Fetch the identity's stored fleet chain, or `None` if none exists yet. Parsed but not trusted until `fold()`.
pub fn fetch(handle_proof: &[u8; 32]) -> Result<Option<MembershipBlob>, String> {
    fgtw::client::fetch(&PhotonTransport, handle_proof)
}

/// Publish a new (or extended) chain (`stale` reason → `"fleet: stale"` for the retry loop).
pub fn publish(blob: &MembershipBlob) -> Result<(), String> {
    fgtw::client::publish(&PhotonTransport, blob)
}

/// Fetch a contact's published succession record, or `None` if none exists (docs/identity-succession.md). Structural only — the caller runs `SuccessorRecord::verify_for_pin` against its own pin.
pub fn fetch_successor(handle_proof: &[u8; 32]) -> Result<Option<SuccessorRecord>, String> {
    fgtw::client::fetch_successor(&PhotonTransport, handle_proof)
}

/// Publish OUR succession record when we re-found this identity — member-gated (this device must fold as a current member of the chain the worker holds for `handle_proof`).
pub fn publish_successor(device_key: &Keypair, record: &SuccessorRecord) -> Result<(), String> {
    fgtw::client::publish_successor(&PhotonTransport, device_key, record)
}

/// Ensure this device is a current fleet member before an authorised write (genesis-claim if no fleet yet).
pub fn ensure_member(
    device_key: &Keypair,
    handle_proof: &[u8; 32],
    identity_seed: &[u8; 32],
) -> Result<(), String> {
    fgtw::client::ensure_member(&PhotonTransport, device_key, handle_proof, identity_seed)
}

/// The current device-pubkey member set (empty if no fleet yet).
pub fn current_members(handle_proof: &[u8; 32]) -> Result<Vec<[u8; 32]>, String> {
    fgtw::client::current_members(&PhotonTransport, handle_proof)
}

/// The current member set for OUR OWN fleet, refusing a chain whose genesis `handle_proof` isn't the slot we queried — the every-fetch anti-swap check (docs/fleet-identity-remediation.md). A slot-consistency check, not an ownership proof (identity is handle-derived). Use this wherever the fetch feeds a trust decision about our own fleet; `current_members` stays for contact chains.
pub fn current_members_verified(handle_proof: &[u8; 32]) -> Result<Vec<[u8; 32]>, String> {
    fgtw::client::current_members_verified(&PhotonTransport, handle_proof)
}

/// The current member set + chain-tip eagle time (monotonic freshness guard for the fold-respecting trust rule).
/// Members + tip + generation id (genesis hash) + existed — the contact-refresh read (docs/lifecycle.md genesis pin).
pub fn current_members_full(
    handle_proof: &[u8; 32],
) -> Result<(Vec<[u8; 32]>, i64, [u8; 32], bool), String> {
    fgtw::client::current_members_full(&PhotonTransport, handle_proof)
}

pub fn current_members_with_ts(handle_proof: &[u8; 32]) -> Result<(Vec<[u8; 32]>, i64), String> {
    fgtw::client::current_members_with_ts(&PhotonTransport, handle_proof)
}

/// Existing-device side of device-ADD: bind the device a verified binding request names, carrying its consent into the Add op.
pub fn bind_device(
    member_key: &Keypair,
    handle_proof: &[u8; 32],
    req: &BindRequest,
) -> Result<(), String> {
    fgtw::client::bind_device(&PhotonTransport, member_key, handle_proof, req)
}

/// This device's own self-signed departure — the only chain remove that exists. Not yet wired to UI (self-retire arrives with the device-trust bundle).
pub fn depart_device(device_key: &Keypair, handle_proof: &[u8; 32]) -> Result<(), String> {
    fgtw::client::depart_device(&PhotonTransport, device_key, handle_proof)
}

/// Devices the chain shows were once ours but are no longer current members — signed out, hardware brand still held (brands survive departure; identity never dies). The chain is the only truth source the client has: a brand the owner already released still lists here, and re-releasing it is an idempotent ack — so these rows are "retired" whether or not the registry claim is technically gone.
pub fn retired_devices(handle_proof: &[u8; 32]) -> Result<Vec<[u8; 32]>, String> {
    let Some(blob) = fetch(handle_proof)? else {
        return Ok(Vec::new());
    };
    let current = blob.fold().map_err(|e| format!("fleet fold: {e:?}"))?;
    let mut out: Vec<[u8; 32]> = Vec::new();
    for op in &blob.ops {
        if matches!(op.kind, OpKind::Genesis | OpKind::Add)
            && !current.contains(&op.device_pubkey)
            && !out.contains(&op.device_pubkey)
        {
            out.push(op.device_pubkey);
        }
    }
    Ok(out)
}

/// OWNER frees a retired device's hardware brand — the second signature of the two-signature retire (the first was the device's own departure). `member_key` must be a current fleet member; the worker refuses releasing a device still in the fold.
pub fn release_device(
    member_key: &Keypair,
    handle_proof: &[u8; 32],
    released: &[u8; 32],
) -> Result<(), String> {
    fgtw::client::device_release(&PhotonTransport, member_key, handle_proof, released)
}

/// Lock a device out of its fleet at the worker (treat-as-stolen) — the worker-authoritative brick that refuses the device at announce forever, surviving any wipe of its local lock cache. `member_key` must be a current fleet member.
pub fn lock_device(
    member_key: &Keypair,
    handle_proof: &[u8; 32],
    locked: &[u8; 32],
) -> Result<(), String> {
    fgtw::client::device_lock(&PhotonTransport, member_key, handle_proof, locked)
}

/// Unlock a device the fleet previously locked (the owner's deliberate reversal) — the worker deletes the lock so the device announces normally again. Same member-gated auth.
pub fn unlock_device(
    member_key: &Keypair,
    handle_proof: &[u8; 32],
    locked: &[u8; 32],
) -> Result<(), String> {
    fgtw::client::device_unlock(&PhotonTransport, member_key, handle_proof, locked)
}

/// NEW device: post (or refresh) its binding request — device-signed + identity-co-signed consent to join. Returns the published `eagle_time` stamp (oscillations) so the caller can derive the proximity beacon from the exact offer the sponsor reads back.
pub fn bindreq_put(
    device_key: &Keypair,
    identity_seed: &[u8; 32],
    handle_proof: &[u8; 32],
    nfc_secret: &[u8; 32],
) -> Result<i64, String> {
    fgtw::client::bindreq_put(
        &PhotonTransport,
        device_key,
        identity_seed,
        handle_proof,
        nfc_secret,
    )
}

/// NEW device: withdraw its own request (on green, or on ceremony cancel). Best-effort — the stamp lapses anyway.
pub fn bindreq_withdraw(device_key: &Keypair, handle_proof: &[u8; 32]) -> Result<(), String> {
    fgtw::client::bindreq_withdraw(&PhotonTransport, device_key, handle_proof)
}

/// EXISTING device: the fresh, signature-verified binding requests for OUR fleet — the matcher's candidate set.
pub fn bindreq_list(
    member_key: &Keypair,
    handle_proof: &[u8; 32],
    identity_seed: &[u8; 32],
) -> Result<Vec<BindRequest>, String> {
    fgtw::client::bindreq_list(&PhotonTransport, member_key, handle_proof, identity_seed)
}

/// Publish a fan-out to the always-online slot (device-signed envelope).
pub fn post_fanout(
    handle_proof: &[u8; 32],
    device_key: &Keypair,
    epoch: u64,
    wraps: &[FanoutWrap],
) -> Result<(), String> {
    fgtw::client::post_fanout(&PhotonTransport, handle_proof, device_key, epoch, wraps)
}

/// Fetch the current fan-out (epoch + rotator + wraps), or None if none published yet.
pub fn fetch_fanout(
    handle_proof: &[u8; 32],
) -> Result<Option<(u64, [u8; 32], Vec<FanoutWrap>)>, String> {
    fgtw::client::fetch_fanout(&PhotonTransport, handle_proof)
}

/// The pair-secret lookup every fan-out call needs: our OWN device answers from its self secret (no ceremony with oneself), any sibling from the vault entry its CLUTCH minted. `None` = that device is NOT COMPLIANT — it gets no wrap and cannot open ours.
fn pair_lookup<'a>(
    device_key: &'a Keypair,
    storage: Option<&'a crate::storage::FlatStorage>,
) -> impl Fn(&[u8; 32]) -> Option<[u8; 32]> + 'a {
    let ours = device_key.public.to_bytes();
    move |peer: &[u8; 32]| {
        if *peer == ours {
            return Some(crate::storage::fanout_pairs::self_secret(
                device_key.secret.as_bytes(),
                &ours,
            ));
        }
        crate::storage::fanout_pairs::load(&ours, peer, storage?)
    }
}

/// Rotate (or first-establish) the fleet key: mint fresh, seal to the COMPLIANT subset of `members`, publish at `stored_epoch + 1`. A member with no pair secret toward us is skipped — dark until its ceremony completes and the next rotation includes it.
pub fn rotate_fleet_key(
    handle_proof: &[u8; 32],
    device_key: &Keypair,
    members: &[[u8; 32]],
    storage: Option<&crate::storage::FlatStorage>,
) -> Result<(u64, [u8; 32]), String> {
    let lookup = pair_lookup(device_key, storage);
    let compliant: Vec<([u8; 32], [u8; 32])> = members
        .iter()
        .filter_map(|m| lookup(m).map(|ps| (*m, ps)))
        .collect();
    let skipped = members.len().saturating_sub(compliant.len());
    if skipped > 0 {
        crate::logf!(
            "FANOUT: {} member device(s) not yet egged — no wrap this epoch (they re-clutch, next rotation includes them)",
            skipped
        );
    }
    if compliant.is_empty() {
        return Err("fanout: no compliant members to wrap".into());
    }
    fgtw::client::rotate_fleet_key(&PhotonTransport, handle_proof, device_key, &compliant)
}

/// The oracle-rooted kek for OUR fleet-key recovery slot (braid.md §14 statelessness, restored). Derived from the device ORACLE + the identity seed: the oracle is the one secret a wipe cannot destroy, and the identity binding keeps two identities on one machine from sharing a slot. Pure hashing, no curve — nothing here for a quantum harvester. `None` when the platform oracle is unreadable.
fn recovery_kek(identity_seed: &[u8; 32]) -> Option<[u8; 32]> {
    let dev = tohu::device::device_secret().ok()?;
    let mut h = blake3::Hasher::new();
    h.update(b"PHOTON_FLEET_RECOVERY_v\x01");
    h.update(&dev);
    h.update(identity_seed);
    Some(*h.finalize().as_bytes())
}

/// The slot purpose: base tag ‖ publisher pid (the scoped-blob publisher-in-purpose rule — one derivation shape everywhere, even where the publisher is always us).
fn recovery_purpose(identity_seed: &[u8; 32]) -> Vec<u8> {
    let pid = crate::crypto::clutch::identity_party_id(identity_seed);
    let mut p = Vec::with_capacity(9 + 32);
    p.extend_from_slice(b"fleet-key");
    p.extend_from_slice(&pid);
    p
}

fn recovery_addr(kek: &[u8; 32], purpose: &[u8]) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.encode(fgtw::scoped_blob::slot_address(kek, purpose))
}

/// Publish OUR fleet-key recovery slot (blocking — call off-thread, on every edge where this device gains the current key: genesis, rotation, fan-out sync, pairing hand-off). This is what lets a WIPED device, alone, its siblings off, get its contacts and avatar back: the wrap Phase A gated on a vault-held pair secret is bypassed by a slot only this machine can find or open.
pub fn publish_recovery_slot(
    fleet_key: &[u8; 32],
    identity_seed: &[u8; 32],
    device_keypair: &Keypair,
    handle_proof: &[u8; 32],
) {
    let Some(kek) = recovery_kek(identity_seed) else {
        return;
    };
    let purpose = recovery_purpose(identity_seed);
    let Ok(sealed) = fgtw::scoped_blob::seal_value(&kek, &purpose, fleet_key) else {
        return;
    };
    match crate::network::fgtw::blob::put_blob_blocking(
        &recovery_addr(&kek, &purpose),
        &sealed,
        device_keypair,
        handle_proof,
    ) {
        Ok(()) => crate::log("RECOVERY: fleet-key slot refreshed (oracle-derived, wipe-proof)"),
        Err(e) => crate::logf!("RECOVERY: fleet-key slot publish failed: {}", e),
    }
}

/// Recover the fleet key from OUR oracle slot (blocking) — the wiped-device path: no vault, no siblings, just this machine and the wall. `None` = no slot (never published, or a different machine).
pub fn recover_fleet_key_from_oracle(identity_seed: &[u8; 32]) -> Option<[u8; 32]> {
    let kek = recovery_kek(identity_seed)?;
    let purpose = recovery_purpose(identity_seed);
    let sealed = crate::network::fgtw::blob::get_blob_blocking(&recovery_addr(&kek, &purpose))
        .ok()
        .flatten()?;
    fgtw::scoped_blob::open_value(&kek, &purpose, &sealed)
}

// ── The epoch-spine custody slot: {k ‖ epoch_k ‖ prev_k ‖ prev_epoch} sealed under a fleet-key-derived custody key, addressed by the fleet key — the wiped-device jump-to-head, refreshed by each checkpoint winner and re-sealed across rotations. Live siblings never need it (ckpt_state frames serve device-to-device). ──

/// Custody domain — binary version numeral per convention.
const CKPT_CUSTODY_PURPOSE: &[u8] = b"PHOTON_FLEET_EPOCH_STATE_v\x01";

fn ckpt_custody_key(fleet_key: &[u8; 32]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(b"PHOTON_FLEET_EPOCH_CUSTODY_v\x01");
    h.update(fleet_key);
    *h.finalize().as_bytes()
}

fn ckpt_custody_addr(fleet_key: &[u8; 32]) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.encode(fgtw::scoped_blob::slot_address(
        fleet_key,
        CKPT_CUSTODY_PURPOSE,
    ))
}

/// Encode the epoch spine state for custody/serve: `k ‖ epoch ‖ prev_k ‖ prev` (prev zeroed when absent).
pub fn ckpt_state_bytes(k: u64, epoch: &[u8; 32], prev: Option<(u64, [u8; 32])>) -> Vec<u8> {
    let (pk, pe) = prev.unwrap_or((0, [0u8; 32]));
    let mut v = Vec::with_capacity(80);
    v.extend_from_slice(&k.to_le_bytes());
    v.extend_from_slice(epoch);
    v.extend_from_slice(&pk.to_le_bytes());
    v.extend_from_slice(&pe);
    v
}

/// Decode [`ckpt_state_bytes`]. `None` on wrong length or a void head.
pub fn ckpt_state_decode(bytes: &[u8]) -> Option<(u64, [u8; 32], Option<(u64, [u8; 32])>)> {
    if bytes.len() != 80 {
        return None;
    }
    let k = u64::from_le_bytes(bytes[..8].try_into().ok()?);
    let epoch: [u8; 32] = bytes[8..40].try_into().ok()?;
    if k == 0 || epoch == [0u8; 32] {
        return None;
    }
    let pk = u64::from_le_bytes(bytes[40..48].try_into().ok()?);
    let pe: [u8; 32] = bytes[48..80].try_into().ok()?;
    let prev = if pk == 0 || pe == [0u8; 32] {
        None
    } else {
        Some((pk, pe))
    };
    Some((k, epoch, prev))
}

/// Write the custody slot (blocking — call off-thread). The winner of each checkpoint refreshes it; a rotation's re-seal is a rewrite under the new fleet key by whoever rotates next checkpoint.
pub fn ckpt_custody_write(
    fleet_key: &[u8; 32],
    state: &[u8],
    device_keypair: &Keypair,
    handle_proof: &[u8; 32],
) -> Result<(), String> {
    let sealed = kete::encrypt_bytes(state, &ckpt_custody_key(fleet_key))?;
    crate::network::fgtw::blob::put_blob_blocking(
        &ckpt_custody_addr(fleet_key),
        &sealed,
        device_keypair,
        handle_proof,
    )
    .map_err(|e| format!("epoch custody put: {e:?}"))
}

/// Read + open the custody slot (blocking). `None` = absent or sealed under a different fleet-key epoch.
pub fn ckpt_custody_read(fleet_key: &[u8; 32]) -> Option<Vec<u8>> {
    let sealed = crate::network::fgtw::blob::get_blob_blocking(&ckpt_custody_addr(fleet_key))
        .ok()
        .flatten()?;
    kete::decrypt_bytes(&sealed, &ckpt_custody_key(fleet_key)).ok()
}

/// Append + publish a Checkpoint op (blocking). `Ok(None)` = we won k; `Ok(Some(winner))` = a competing checkpoint rides the chain; `Err` = transport/raced-membership, re-arm on the next edge.
pub fn push_checkpoint(
    handle_proof: &[u8; 32],
    device_key: &Keypair,
    k: u64,
    commit: [u8; 32],
    fanout_epoch: u64,
) -> Result<Option<(u64, [u8; 32], u64)>, String> {
    fgtw::client::push_checkpoint(
        &PhotonTransport,
        handle_proof,
        device_key,
        k,
        commit,
        fanout_epoch,
    )
}

/// Recover the current fleet key WITH its fan-out epoch — derivation callers need the pair (the epoch spine folds the key and names the epoch).
pub fn recover_fleet_key_with_epoch(
    handle_proof: &[u8; 32],
    device_key: &Keypair,
    storage: Option<&crate::storage::FlatStorage>,
) -> Result<Option<(u64, [u8; 32])>, String> {
    fgtw::client::recover_fleet_key_with_epoch(
        &PhotonTransport,
        handle_proof,
        device_key,
        &pair_lookup(device_key, storage),
    )
}

/// Recover the current fleet key from the fan-out with this device's key + its pair secret toward the rotator (None if not a current member, or not yet egged with whoever rotated).
pub fn recover_fleet_key(
    handle_proof: &[u8; 32],
    device_key: &Keypair,
    storage: Option<&crate::storage::FlatStorage>,
) -> Result<Option<[u8; 32]>, String> {
    fgtw::client::recover_fleet_key(
        &PhotonTransport,
        handle_proof,
        device_key,
        &pair_lookup(device_key, storage),
    )
}

/// Recover the current fleet key, or ESTABLISH epoch 1 if no fan-out exists yet (the genesis founder).
pub fn recover_or_establish_fleet_key(
    handle_proof: &[u8; 32],
    device_key: &Keypair,
    storage: Option<&crate::storage::FlatStorage>,
) -> Result<Option<[u8; 32]>, String> {
    fgtw::client::recover_or_establish_fleet_key(
        &PhotonTransport,
        handle_proof,
        device_key,
        &pair_lookup(device_key, storage),
    )
}

/// Publish the FULL fleet-shared state (roster + settings layers): seal under the fleet key (kete) and PUT to the membership-gated slot. The settings sync layer calls this with its cached state.
pub fn push_fstate(
    handle_proof: &[u8; 32],
    device_key: &Keypair,
    fleet_key: &[u8; 32],
    state: &fgtw::fstate::FleetState,
) -> Result<(), String> {
    // Fingerprint every write: the vanishing-avatar-pin hunt (2026-08-02) burned three sessions inferring what a push carried. Key NAMES are internal labels, not secrets; values never log.
    crate::logf!(
        "FSTATE→ push: {} roster, globals [{}], {} device map(s), key {}",
        state.roster.len(),
        state
            .global_settings
            .iter()
            .map(|e| e.key.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        state.device_settings.len(),
        crate::fp(fleet_key).as_str()
    );
    fgtw::client::push_fstate(
        &PhotonTransport,
        &PhotonSealer,
        handle_proof,
        device_key,
        fleet_key,
        state,
    )
}

/// Fetch + open the fleet-shared state (None if none published yet; a pre-settings roster-only blob reads as settings-empty).
pub fn pull_fstate(
    handle_proof: &[u8; 32],
    fleet_key: &[u8; 32],
) -> Result<Option<fgtw::fstate::FleetState>, String> {
    let r = fgtw::client::pull_fstate(&PhotonTransport, &PhotonSealer, handle_proof, fleet_key);
    // The pull side of the write fingerprint above — reading both lines in one log answers "did the slot keep what we sent" without inference.
    if let Ok(Some(s)) = &r {
        crate::logf!(
            "FSTATE← pull: {} roster, globals [{}], {} device map(s), key {}",
            s.roster.len(),
            s.global_settings
                .iter()
                .map(|e| e.key.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            s.device_settings.len(),
            crate::fp(fleet_key).as_str()
        );
    }
    r
}

/// Publish the fleet roster. Roster-shaped wrapper over [`push_fstate`]: pulls the current slot first so the settings layers ride along untouched AND the roster converges by CRDT — union by handle_proof, per-entry LWW on `updated`, sticky tombstones — instead of last-pusher-wins clobbering a sibling's concurrent add (or resurrecting a removal we never held locally). A pull failure falls back to our-entries-only (nothing preservable: not_found = empty slot, AEAD failure = stale-epoch blob that this push is re-sealing anyway).
pub fn push_roster(
    handle_proof: &[u8; 32],
    device_key: &Keypair,
    fleet_key: &[u8; 32],
    entries: &[RosterEntry],
) -> Result<(), String> {
    push_roster_with_settings(handle_proof, device_key, fleet_key, entries, None)
}

/// [`push_roster`] carrying this device's LIVE settings layers alongside. Two concurrent pull-merge-push writers race: the loser's pulled base predates the winner's write, so a roster push that carried only stale pulled settings REVERTED them — a freshly-minted avatar pin lost the race to the boot reconcile push on every single launch, and the slot sat pinless forever ("avatar still sticks", 2026-08-02). A pusher that includes every layer it holds can never revert a value it already knows.
pub fn push_roster_with_settings(
    handle_proof: &[u8; 32],
    device_key: &Keypair,
    fleet_key: &[u8; 32],
    entries: &[RosterEntry],
    live_settings: Option<(
        Vec<fgtw::fstate::SettingEntry>,
        Vec<fgtw::fstate::DeviceSettings>,
    )>,
) -> Result<(), String> {
    // A FAILED pull must never become a destructive push. This is pull-merge-push, so the pulled state is the merge base for BOTH layers — and `Err` (network blip, AEAD failure across a key rotation, a tag bump the reader doesn't know) is not the same fact as `Ok(None)` (nothing published yet). Collapsing them into `default()` meant any transient error rebased the fleet on EMPTY and the push overwrote everyone's settings and roster with this device's local view.
    // Observed live on the PRST2→PRST3 roster bump: "state pulled — 8 roster entries, 0 global settings, 0 device maps" — the settings layer was gone from FGTW.
    // Ok(None) still starts from empty: that is a genuine first publish, and there is nothing to lose.
    let mut state = match pull_fstate(handle_proof, fleet_key) {
        Ok(Some(s)) => s,
        Ok(None) => fgtw::fstate::FleetState::default(),
        // A slot we cannot DECRYPT is not the same fact as a pull we could not COMPLETE. The guard below exists to stop an empty merge base clobbering good data after a transient failure — but bytes sealed under a superseded fleet key are already unreadable to every device in the fleet, including the ones that wrote them. Refusing there is not caution; it is a permanent deadlock, and it is the one Nick's log shows: 29 aead failures against 26 refused pushes, the roster never loading on any device. Re-sealing our local view is the only way the slot ever becomes readable again, and local state is strictly better than state nobody can open.
        Err(e) if e.contains("aead") || e.contains("decrypt") => {
            crate::logf!(
                "FLEET: slot is sealed under a superseded key ({}) — re-sealing from local state, since bytes no device can open are already lost",
                e
            );
            fgtw::fstate::FleetState::default()
        }
        Err(e) => {
            return Err(format!(
                "refusing to push fleet state: the pull failed ({e}), so the merge base is unknown — pushing now would overwrite the fleet's roster and settings with this device's view alone"
            ))
        }
    };
    state.roster = fgtw::fstate::merge_rosters(std::mem::take(&mut state.roster), entries.to_vec());
    if let Some((global, devices)) = live_settings {
        state = fgtw::fstate::merge_fstate(
            state,
            fgtw::fstate::FleetState {
                roster: Vec::new(),
                global_settings: global,
                device_settings: devices,
            },
        );
    }
    push_fstate(handle_proof, device_key, fleet_key, &state)
}

/// Fetch + open the fleet roster (None if none published yet).
pub fn pull_roster(
    handle_proof: &[u8; 32],
    fleet_key: &[u8; 32],
) -> Result<Option<Vec<RosterEntry>>, String> {
    Ok(pull_fstate(handle_proof, fleet_key)?.map(|s| s.roster))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pk(k: &Keypair) -> [u8; 32] {
        k.public.to_bytes()
    }

    /// A throwaway vault holding the pair secrets a live fan-out test needs. Phase A wraps require an egged pair between rotator and recipient, so each simulated device gets its own vault carrying the shared secret for every pair it participates in — exactly what a completed sibling CLUTCH would have stored.
    fn egged_vault(tag: &str, ours: &Keypair, peers: &[&Keypair]) -> crate::storage::FlatStorage {
        let vault_seed = *ihi::handle_to_hash(tag).as_bytes();
        let storage =
            crate::storage::FlatStorage::new(crate::storage::APP, vault_seed, rand::random())
                .expect("test vault");
        for peer in peers {
            // Symmetric stand-in for the ceremony-derived secret: both devices derive the same bytes from the sorted pair.
            let (lo, hi) = if pk(ours) <= pk(peer) {
                (pk(ours), pk(peer))
            } else {
                (pk(peer), pk(ours))
            };
            let mut h = blake3::Hasher::new();
            h.update(b"test pair");
            h.update(&lo);
            h.update(&hi);
            crate::storage::fanout_pairs::store(
                &pk(ours),
                &pk(peer),
                h.finalize().as_bytes(),
                &storage,
            )
            .expect("store pair secret");
        }
        storage
    }

    /// End-to-end against LIVE fgtw.org: genesis a fresh fleet, run the full words-first device-ADD ceremony (binding request → member-gated list → matcher words → consent-carrying bind → rotate → recover → withdraw), and confirm the new device folds in with the fleet key.
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
        assert_eq!(
            current_members(&handle_proof).unwrap(),
            vec![member.public.to_bytes()]
        );
        let member_vault = egged_vault("fanout-live-member", &member, &[&newdev]);
        let newdev_vault = egged_vault("fanout-live-newdev", &newdev, &[&member]);
        let (_, k1) = rotate_fleet_key(
            &handle_proof,
            &member,
            &[member.public.to_bytes()],
            Some(&member_vault),
        )
        .expect("establish");

        // New device: post its binding request (device-signed + identity-co-signed) and display its masked words.
        bindreq_put(&newdev, &identity_seed, &handle_proof, &[0u8; 32]).expect("post request");
        let shown = masked_device_words(&newdev.public.to_bytes(), &identity_seed);

        // Existing device: pull the member-gated candidate set — the request is there, verified, and its expected words match what the new device is showing (the matcher's full-match condition).
        let reqs = bindreq_list(&member, &handle_proof, &identity_seed).expect("list");
        let req = reqs
            .iter()
            .find(|r| r.device_pubkey == newdev.public.to_bytes())
            .expect("our request in the set");
        assert_eq!(
            masked_device_words(&req.device_pubkey, &identity_seed),
            shown
        );

        // Bind (carrying the request's consent) + rotate: the new device is a member and recovers the NEW epoch key with its own device key.
        bind_device(&member, &handle_proof, req).expect("bind");
        let members2 = current_members(&handle_proof).unwrap();
        assert!(members2.contains(&newdev.public.to_bytes()));
        let (_, k2) = rotate_fleet_key(&handle_proof, &member, &members2, Some(&member_vault))
            .expect("rotate");
        assert_ne!(k2, k1);
        assert_eq!(
            recover_fleet_key(&handle_proof, &newdev, Some(&newdev_vault))
                .unwrap()
                .unwrap(),
            k2
        );

        // The author withdraws its request (the exit act) — the set reads empty afterwards.
        bindreq_withdraw(&newdev, &handle_proof).expect("withdraw");
        assert!(bindreq_list(&member, &handle_proof, &identity_seed)
            .unwrap()
            .is_empty());

        // A non-member can't read the registry (the member gate).
        let stranger = Keypair::from_seed(&rand::random::<[u8; 32]>());
        assert!(bindreq_list(&stranger, &handle_proof, &identity_seed).is_err());
    }

    fn roster_entry(hp: u8, updated: i64, tombstone: bool) -> RosterEntry {
        RosterEntry {
            handle_proof: [hp; 32],
            handle_hash: [hp ^ 0xff; 32],
            public_identity: [hp.wrapping_add(1); 32],
            published_name: format!("Chosen{hp}"),
            avatar_pin: [hp ^ 0x55; 64],
            added: 100,
            updated,
            tombstone,
            ceremony_owner: [hp.wrapping_add(2); 32],
            woven: false,
            trust_level: hp % 4,
        }
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
        let a_vault = egged_vault("fanout-rot-a", &a, &[&b]);
        let b_vault = egged_vault("fanout-rot-b", &b, &[&a]);
        let (e1, k1) =
            rotate_fleet_key(&handle_proof, &a, &[pk(&a)], Some(&a_vault)).expect("establish");
        assert_eq!(e1, 1);
        assert_eq!(
            recover_fleet_key(&handle_proof, &a, Some(&a_vault))
                .unwrap()
                .unwrap(),
            k1
        );
        // B isn't a member yet → cannot recover.
        assert!(recover_fleet_key(&handle_proof, &b, Some(&b_vault))
            .unwrap()
            .is_none());

        // A sponsors B (B's request carries its consent), then rotates to [A, B]: a fresh key both can open.
        bindreq_put(&b, &identity_seed, &handle_proof, &[0u8; 32]).expect("B posts request");
        let reqs = bindreq_list(&a, &handle_proof, &identity_seed).expect("list");
        let req_b = reqs
            .iter()
            .find(|r| r.device_pubkey == pk(&b))
            .expect("B's request");
        bind_device(&a, &handle_proof, req_b).expect("bind B");
        let members2 = current_members(&handle_proof).unwrap();
        let (e2, k2) =
            rotate_fleet_key(&handle_proof, &a, &members2, Some(&a_vault)).expect("rotate to A,B");
        assert_eq!(e2, 2);
        assert_ne!(k2, k1);
        assert_eq!(
            recover_fleet_key(&handle_proof, &a, Some(&a_vault))
                .unwrap()
                .unwrap(),
            k2
        );
        assert_eq!(
            recover_fleet_key(&handle_proof, &b, Some(&b_vault))
                .unwrap()
                .unwrap(),
            k2
        );

        // B departs (self-signed — the only remove there is); A rotates to [A]: A gets the new key, B cannot — departure + rotation withhold.
        depart_device(&b, &handle_proof).expect("B departs");
        let members3 = current_members(&handle_proof).unwrap();
        assert_eq!(members3, vec![pk(&a)]);
        let (e3, k3) =
            rotate_fleet_key(&handle_proof, &a, &members3, Some(&a_vault)).expect("rotate to A");
        assert_eq!(e3, 3);
        assert_eq!(
            recover_fleet_key(&handle_proof, &a, Some(&a_vault))
                .unwrap()
                .unwrap(),
            k3
        );
        assert!(recover_fleet_key(&handle_proof, &b, Some(&b_vault))
            .unwrap()
            .is_none());

        // A stale rotation (epoch ≤ stored) is rejected by the worker's monotonic guard.
        let stale_members: Vec<([u8; 32], [u8; 32])> =
            members3.iter().map(|m| (*m, [0u8; 32])).collect();
        let stale =
            fanout_seal(&handle_proof, 3, &new_fleet_key(), &pk(&a), &stale_members).unwrap();
        assert!(post_fanout(&handle_proof, &a, 3, &stale).is_err());
    }

    /// End-to-end removal HEAL against LIVE fgtw.org — the §14.12-1 recipe `spawn_fleet_key_sync` runs when a departure lands: the shrink sentinel fires (fan-out wraps > folded members), the survivor preserves the fstate slot under the OLD key, rotates, re-pushes the merge under the NEW epoch — settings intact, the leaver locked out of both the fan-out and the re-sealed slot.
    #[test]
    #[ignore = "hits live fgtw.org"]
    fn live_removal_heal_round_trip() {
        use fgtw::fstate::{merge_fstate, FleetState, SettingEntry};
        let handle_proof: [u8; 32] = rand::random();
        let identity_seed: [u8; 32] = rand::random();
        let a = Keypair::from_seed(&rand::random::<[u8; 32]>());
        let b = Keypair::from_seed(&rand::random::<[u8; 32]>());

        // Two-member fleet at epoch 2, with a settings marker sealed into the slot under k2.
        ensure_member(&a, &handle_proof, &identity_seed).expect("genesis");
        let a_vault = egged_vault("fanout-heal-a", &a, &[&b]);
        let b_vault = egged_vault("fanout-heal-b", &b, &[&a]);
        let (e1, _) =
            rotate_fleet_key(&handle_proof, &a, &[pk(&a)], Some(&a_vault)).expect("establish");
        assert_eq!(e1, 1);
        bindreq_put(&b, &identity_seed, &handle_proof, &[0u8; 32]).expect("B posts request");
        let reqs = bindreq_list(&a, &handle_proof, &identity_seed).expect("list");
        let req_b = reqs
            .iter()
            .find(|r| r.device_pubkey == pk(&b))
            .expect("B's request");
        bind_device(&a, &handle_proof, req_b).expect("bind B");
        let (e2, k2) = rotate_fleet_key(
            &handle_proof,
            &a,
            &current_members(&handle_proof).unwrap(),
            Some(&a_vault),
        )
        .expect("rotate to A,B");
        assert_eq!(e2, 2);
        let marker = SettingEntry {
            key: "test.heal_marker".into(),
            value: vsf::VsfType::hR(b"survives".to_vec()),
            updated: 700,
            tombstone: false,
        };
        let state = FleetState {
            roster: vec![roster_entry(7, 500, false)],
            global_settings: vec![marker],
            device_settings: Vec::new(),
        };
        push_fstate(&handle_proof, &a, &k2, &state).expect("seed the slot under k2");

        // B departs. The sentinel condition A's next key sync sees: the fan-out still wraps 2 devices, the fold holds 1.
        depart_device(&b, &handle_proof).expect("B departs");
        let members = current_members(&handle_proof).unwrap();
        assert_eq!(members, vec![pk(&a)]);
        let (_, _, wraps) = fetch_fanout(&handle_proof)
            .expect("fetch")
            .expect("a fan-out");
        assert!(
            fanout_needs_rotation(wraps.len(), members.len()),
            "shrink must trip the sentinel"
        );

        // The heal, in spawn_fleet_key_sync's exact order: preserve under the old key, rotate to the survivors, re-push the merge under the new epoch.
        let preserved = pull_fstate(&handle_proof, &k2)
            .expect("pull")
            .expect("slot readable under the old key");
        let (e3, k3) =
            rotate_fleet_key(&handle_proof, &a, &members, Some(&a_vault)).expect("heal rotation");
        assert_eq!(e3, 3);
        push_fstate(
            &handle_proof,
            &a,
            &k3,
            &merge_fstate(preserved, FleetState::default()),
        )
        .expect("re-seal under k3");

        // The survivor recovers the new epoch and the marker survived the re-seal; the leaver recovers nothing and its old key no longer opens the slot.
        assert_eq!(
            recover_fleet_key(&handle_proof, &a, Some(&a_vault))
                .unwrap()
                .unwrap(),
            k3
        );
        let healed = pull_fstate(&handle_proof, &k3)
            .expect("pull under k3")
            .expect("re-sealed slot");
        assert!(
            healed
                .global_settings
                .iter()
                .any(|s| s.key == "test.heal_marker" && s.value == vsf::VsfType::hR(b"survives".to_vec())),
            "settings must survive the re-seal"
        );
        assert_eq!(healed.roster.len(), 1, "roster must survive the re-seal");
        assert!(
            recover_fleet_key(&handle_proof, &b, Some(&b_vault))
                .unwrap()
                .is_none(),
            "the leaver must not recover the healed epoch"
        );
        assert!(
            pull_fstate(&handle_proof, &k2).is_err(),
            "the leaver's cached key must not open the re-sealed slot"
        );
        // Post-heal steady state: the sentinel is quiet again.
        let (_, _, wraps) = fetch_fanout(&handle_proof)
            .expect("fetch")
            .expect("a fan-out");
        assert!(!fanout_needs_rotation(wraps.len(), members.len()));
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
        let pulled = pull_roster(&handle_proof, &fleet_key)
            .expect("pull")
            .expect("a roster");
        assert_eq!(pulled, entries);

        // A non-member can't publish (fold gate rejects the write).
        let stranger = Keypair::from_seed(&rand::random::<[u8; 32]>());
        assert!(push_roster(&handle_proof, &stranger, &fleet_key, &entries).is_err());
    }

    /// End-to-end fleet-inbox bind-attempt alert against LIVE fgtw.org: device D belongs to identity A; identity B tries to enrol the SAME device D → worker rejects device_owned and drops a bind_attempt alert into A's inbox; A drains it (member-gated), sees B as the attempted-by, and a second drain is empty (consume semantics). See docs/fleet-inbox.md.
    #[test]
    #[ignore = "hits live fgtw.org"]
    fn live_bind_attempt_alert() {
        let a_hp: [u8; 32] = rand::random();
        let a_seed: [u8; 32] = rand::random();
        let b_hp: [u8; 32] = rand::random();
        let b_seed: [u8; 32] = rand::random();
        let device = Keypair::from_seed(&rand::random::<[u8; 32]>());

        // A claims the fleet with device D.
        ensure_member(&device, &a_hp, &a_seed).expect("A genesis");
        assert_eq!(
            current_members(&a_hp).unwrap(),
            vec![device.public.to_bytes()]
        );

        // B tries to enrol the SAME device D — rejected (device_owned, wrapped by ensure_member's establish-membership message); B's fleet stays empty.
        ensure_member(&device, &b_hp, &b_seed).expect_err("B enrol must be rejected");
        assert!(
            current_members(&b_hp).unwrap().is_empty(),
            "B must not have claimed the device"
        );

        // A drains its inbox: a bind_attempt naming B's handle_proof.
        let events = crate::network::fgtw::inbox_drain_blocking(&device, &a_hp).expect("drain");
        assert!(
            events
                .iter()
                .any(|e| e.kind == "bind_attempt" && e.attempted_by == b_hp),
            "expected a bind_attempt alert naming B; got {events:?}"
        );

        // Consume semantics: a second drain is empty.
        let again = crate::network::fgtw::inbox_drain_blocking(&device, &a_hp).expect("drain2");
        assert!(
            again.is_empty(),
            "inbox should be empty after drain; got {again:?}"
        );

        // A non-member device can't drain A's inbox (member gate).
        let stranger = Keypair::from_seed(&rand::random::<[u8; 32]>());
        assert!(
            crate::network::fgtw::inbox_drain_blocking(&stranger, &a_hp)
                .map(|v| v.is_empty())
                .unwrap_or(true),
            "a non-member must not read A's inbox"
        );
    }
}

#[cfg(test)]
mod pin_live_tests {
    use super::*;

    /// End-to-end against LIVE fgtw.org: a 4-global state whose fourth entry is a 64-byte avatar pin must survive push → worker → pull byte-for-byte. Every local link (codec, merge, race carriage) is unit-proven; this is the only test that can catch the slot itself lying (the field pin regression, 2026-08-02).
    #[test]
    #[ignore = "hits live fgtw.org"]
    fn live_pin_survives_the_slot() {
        use fgtw::fstate::{FleetState, SettingEntry};
        let handle_proof: [u8; 32] = rand::random();
        let identity_seed: [u8; 32] = rand::random();
        let a = Keypair::from_seed(&rand::random::<[u8; 32]>());
        ensure_member(&a, &handle_proof, &identity_seed).expect("genesis");
        let fleet_key: [u8; 32] = rand::random();
        let mk = |key: &str, val: Vec<u8>, at: i64| SettingEntry {
            key: key.into(),
            value: vsf::VsfType::hR(val),
            updated: at,
            tombstone: false,
        };
        let state = FleetState {
            roster: Vec::new(),
            global_settings: vec![
                mk("logs.hard", vec![1], 5),
                mk("profile.avatar_pin", vec![0xEE; 64], 8),
                mk("profile.avatar_ts", 42i64.to_le_bytes().to_vec(), 6),
                mk("profile.name", b"PinProbe".to_vec(), 7),
            ],
            device_settings: Vec::new(),
        };
        push_fstate(&handle_proof, &a, &fleet_key, &state).expect("push");
        let back = pull_fstate(&handle_proof, &fleet_key)
            .expect("pull")
            .expect("stored");
        assert_eq!(
            back.global_settings.len(),
            4,
            "slot dropped entries: {:?}",
            back.global_settings
                .iter()
                .map(|e| &e.key)
                .collect::<Vec<_>>()
        );
        let pin = back
            .global_settings
            .iter()
            .find(|e| e.key == "profile.avatar_pin")
            .expect("pin entry");
        assert_eq!(pin.value, vsf::VsfType::hR(vec![0xEE; 64]));
    }
}
