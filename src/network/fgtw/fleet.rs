//! FGTW client layer — the std HTTP oracle over the shared fleet core, plus the fan-out crypto, roster codec, and pairing words.
//!
//! The membership chain itself (`FleetOp` / `MembershipBlob` / `fold` / the VSF op codec / the device-signed builders) now lives in the `fgtw` crate (`fgtw::fleet`), shared verbatim by every TOKEN app and the FGTW worker — no more hand-mirrored copies kept in lockstep.
//! Re-exported below so photon's `crate::network::fgtw::fleet::*` call sites are unchanged.
//! What remains here is the client half that still depends on photon's std stack (reqwest via `crate::network::http`, `kete`) — being lifted into `fgtw::client` / `fgtw::fanout` / `fgtw::fstate` in later migration steps.

pub use fgtw::fleet::{et_to_osc, scheme, Egg, FleetOp, FoldError, MembershipBlob, OpKind};

use crate::network::fgtw::Keypair;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use vsf::VsfType;

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
        .map_err(|e| crate::network::http::short_send_error("reach FGTW", &e))?;
    if resp.status().as_u16() == 404 {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err("FGTW rejected the lookup".to_string());
    }
    let bytes = resp.bytes().map_err(|e| crate::network::http::short_send_error("reach FGTW", &e))?;
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
        .map_err(|e| crate::network::http::short_send_error("reach FGTW", &e))?;
    if resp.status().is_success() {
        Ok(())
    } else if resp.status().as_u16() == 409 {
        // Forward-extension conflict — a real, actionable state (someone extended the chain first), kept distinct so the retry loop can match on it.
        Err("fleet: 409".to_string())
    } else {
        Err("FGTW rejected the fleet update".to_string())
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

// ── Pairing v1 word codec moved to fgtw::pair (voca words ↔ pairing pubkey, spell-check, signing-bytes); re-exported so call sites are unchanged. The FGTW relay transport below stays here (M3). ──
pub use fgtw::pair::{
    device_name_default, first_bad_pair_word, new_pairing_id, pair_entry_complete,
    pair_matched_signing_bytes, pair_request_signing_bytes, pair_word_list, pair_word_tokens,
    pair_words, parse_pair_event, words_to_pair_pubkey, PairRequest, PAIR_WORD_COUNT,
};

/// Pairing slots older than this are ignored (stale inbox).
const PAIR_FRESH_OSC: i64 = 300 * vsf::OSCILLATIONS_PER_SECOND as i64; // 5 minutes

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
        .map_err(|e| crate::network::http::short_send_error("reach FGTW", &e))?;
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
        .map_err(|e| crate::network::http::short_send_error("reach FGTW", &e))?;
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
        .map_err(|e| crate::network::http::short_send_error("reach FGTW", &e))?;
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
        .map_err(|e| crate::network::http::short_send_error("reach FGTW", &e))?;
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

// ── Fleet-key fan-out crypto moved to fgtw::fanout (seal/open + FanoutWrap + codec); re-exported. The always-online transport + epoch rotation below stays here (M4). ──
pub use fgtw::fanout::{
    fanout_from_bytes, fanout_open, fanout_seal, fanout_to_bytes, new_fleet_key, FanoutWrap,
};

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
        .map_err(|e| crate::network::http::short_send_error("reach FGTW", &e))?;
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
        .map_err(|e| crate::network::http::short_send_error("reach FGTW", &e))?;
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

// ── Fleet shared-state roster codec moved to fgtw::fstate (RosterEntry + serialize + CRDT merge); re-exported. The seal-and-push transport below stays here (M4). ──
pub use fgtw::fstate::{merge_rosters, roster_from_bytes, roster_to_bytes, RosterEntry};

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
        .map_err(|e| crate::network::http::short_send_error("reach FGTW", &e))?;
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
        .map_err(|e| crate::network::http::short_send_error("reach FGTW", &e))?;
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

    fn key(seed: u8) -> Keypair {
        Keypair::from_seed(&[seed; 32])
    }

    fn pk(k: &Keypair) -> [u8; 32] {
        k.public.to_bytes()
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
