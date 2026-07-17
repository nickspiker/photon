//! FGTW client adapter — photon's binding of the shared `fgtw` crate to its own HTTP + storage stack.
//!
//! All the substrate now lives in the crate, re-exported here so photon's `crate::network::fgtw::fleet::*` call sites are unchanged:
//! `fgtw::fleet` (the membership chain), `fgtw::fanout` (fan-out crypto), `fgtw::fstate` (roster codec), `fgtw::pair` (pairing words), and `fgtw::client` (the fetch-then-sign oracle).
//! What's left here is the *binding*: [`PhotonTransport`] (FGTW's HTTP over photon's warm-TLS pool + short error UX) and [`PhotonSealer`] (roster AEAD over `kete`), plus thin same-signature wrappers that inject them — so the crate stays reqwest-free and photon keeps its own network stack.

pub use fgtw::fanout::{
    fanout_from_bytes, fanout_open, fanout_seal, fanout_to_bytes, new_fleet_key, FanoutWrap,
};
pub use fgtw::fleet::{
    bindreq_signing_bytes, et_to_osc, scheme, BindRequest, Egg, FleetOp, FoldError, MembershipBlob,
    OpKind, BINDREQ_FRESH_OSC, CONSENT_WINDOW_OSC,
};
pub use fgtw::fstate::{merge_rosters, roster_from_bytes, roster_to_bytes, RosterEntry};
pub use fgtw::pair::{
    device_name_default, first_bad_pair_word, keyed_pseudonym, masked_device_words,
    pair_word_list, pair_word_tokens, pair_words, parse_pair_event, word_mask, PAIR_WORD_COUNT,
};

use crate::network::fgtw::Keypair;
use fgtw::client::{FgtwResponse, FgtwTransport, FleetSealer};

const FGTW_URL: &str = "https://fgtw.org";

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

/// The current member set for OUR OWN fleet, refusing a chain whose genesis isn't co-signed by `Ed25519(identity_seed)` — the every-fetch genesis check (docs/pairing-v2.md). Use this wherever the fetch feeds a trust decision about our own fleet; `current_members` stays for contact chains.
pub fn current_members_verified(handle_proof: &[u8; 32], identity_seed: &[u8; 32]) -> Result<Vec<[u8; 32]>, String> {
    fgtw::client::current_members_verified(&PhotonTransport, handle_proof, identity_seed)
}

/// The current member set + chain-tip eagle time (monotonic freshness guard for the fold-respecting trust rule).
/// Members + tip + generation id (genesis hash) + existed — the contact-refresh read (docs/lifecycle.md genesis pin).
pub fn current_members_full(handle_proof: &[u8; 32]) -> Result<(Vec<[u8; 32]>, i64, [u8; 32], bool), String> {
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

/// NEW device: post (or refresh) its binding request — device-signed + identity-co-signed consent to join. Returns the published `eagle_time` stamp (oscillations) so the caller can derive the proximity beacon from the exact offer the sponsor reads back.
pub fn bindreq_put(
    device_key: &Keypair,
    identity_seed: &[u8; 32],
    handle_proof: &[u8; 32],
) -> Result<i64, String> {
    fgtw::client::bindreq_put(&PhotonTransport, device_key, identity_seed, handle_proof)
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

/// Fetch the current fan-out (epoch + wraps), or None if none published yet.
pub fn fetch_fanout(handle_proof: &[u8; 32]) -> Result<Option<(u64, Vec<FanoutWrap>)>, String> {
    fgtw::client::fetch_fanout(&PhotonTransport, handle_proof)
}

/// Rotate (or first-establish) the fleet key: mint fresh, seal to `members`, publish at `stored_epoch + 1`.
pub fn rotate_fleet_key(
    handle_proof: &[u8; 32],
    device_key: &Keypair,
    members: &[[u8; 32]],
) -> Result<(u64, [u8; 32]), String> {
    fgtw::client::rotate_fleet_key(&PhotonTransport, handle_proof, device_key, members)
}

/// Recover the current fleet key from the fan-out with this device's key alone (None if not a current member).
pub fn recover_fleet_key(
    handle_proof: &[u8; 32],
    device_key: &Keypair,
) -> Result<Option<[u8; 32]>, String> {
    fgtw::client::recover_fleet_key(&PhotonTransport, handle_proof, device_key)
}

/// Recover the current fleet key, or ESTABLISH epoch 1 if no fan-out exists yet (the genesis founder).
pub fn recover_or_establish_fleet_key(
    handle_proof: &[u8; 32],
    device_key: &Keypair,
) -> Result<Option<[u8; 32]>, String> {
    fgtw::client::recover_or_establish_fleet_key(&PhotonTransport, handle_proof, device_key)
}

/// Publish the FULL fleet-shared state (roster + settings layers): seal under the fleet key (kete) and PUT to the membership-gated slot. The settings sync layer calls this with its cached state.
pub fn push_fstate(
    handle_proof: &[u8; 32],
    device_key: &Keypair,
    fleet_key: &[u8; 32],
    state: &fgtw::fstate::FleetState,
) -> Result<(), String> {
    fgtw::client::push_fstate(&PhotonTransport, &PhotonSealer, handle_proof, device_key, fleet_key, state)
}

/// Fetch + open the fleet-shared state (None if none published yet; a pre-settings roster-only blob reads as settings-empty).
pub fn pull_fstate(
    handle_proof: &[u8; 32],
    fleet_key: &[u8; 32],
) -> Result<Option<fgtw::fstate::FleetState>, String> {
    fgtw::client::pull_fstate(&PhotonTransport, &PhotonSealer, handle_proof, fleet_key)
}

/// Publish the fleet roster. Roster-shaped wrapper over [`push_fstate`]: pulls the current slot first so the settings layers ride along untouched AND the roster converges by CRDT — union by handle_proof, per-entry LWW on `updated`, sticky tombstones — instead of last-pusher-wins clobbering a sibling's concurrent add (or resurrecting a removal we never held locally). A pull failure falls back to our-entries-only (nothing preservable: not_found = empty slot, AEAD failure = stale-epoch blob that this push is re-sealing anyway).
pub fn push_roster(
    handle_proof: &[u8; 32],
    device_key: &Keypair,
    fleet_key: &[u8; 32],
    entries: &[RosterEntry],
) -> Result<(), String> {
    let mut state = match pull_fstate(handle_proof, fleet_key) {
        Ok(Some(s)) => s,
        _ => fgtw::fstate::FleetState::default(),
    };
    state.roster = fgtw::fstate::merge_rosters(std::mem::take(&mut state.roster), entries.to_vec());
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
        assert_eq!(current_members(&handle_proof).unwrap(), vec![member.public.to_bytes()]);
        let (_, k1) = rotate_fleet_key(&handle_proof, &member, &[member.public.to_bytes()]).expect("establish");

        // New device: post its binding request (device-signed + identity-co-signed) and display its masked words.
        bindreq_put(&newdev, &identity_seed, &handle_proof).expect("post request");
        let shown = masked_device_words(&newdev.public.to_bytes(), &identity_seed);

        // Existing device: pull the member-gated candidate set — the request is there, verified, and its expected words match what the new device is showing (the matcher's full-match condition).
        let reqs = bindreq_list(&member, &handle_proof, &identity_seed).expect("list");
        let req = reqs.iter().find(|r| r.device_pubkey == newdev.public.to_bytes()).expect("our request in the set");
        assert_eq!(masked_device_words(&req.device_pubkey, &identity_seed), shown);

        // Bind (carrying the request's consent) + rotate: the new device is a member and recovers the NEW epoch key with its own device key.
        bind_device(&member, &handle_proof, req).expect("bind");
        let members2 = current_members(&handle_proof).unwrap();
        assert!(members2.contains(&newdev.public.to_bytes()));
        let (_, k2) = rotate_fleet_key(&handle_proof, &member, &members2).expect("rotate");
        assert_ne!(k2, k1);
        assert_eq!(recover_fleet_key(&handle_proof, &newdev).unwrap().unwrap(), k2);

        // The author withdraws its request (the exit act) — the set reads empty afterwards.
        bindreq_withdraw(&newdev, &handle_proof).expect("withdraw");
        assert!(bindreq_list(&member, &handle_proof, &identity_seed).unwrap().is_empty());

        // A non-member can't read the registry (the member gate).
        let stranger = Keypair::from_seed(&rand::random::<[u8; 32]>());
        assert!(bindreq_list(&stranger, &handle_proof, &identity_seed).is_err());
    }

    fn roster_entry(hp: u8, updated: i64, tombstone: bool) -> RosterEntry {
        RosterEntry {
            handle_proof: [hp; 32],
            handle_hash: [hp ^ 0xff; 32],
            public_identity: [hp.wrapping_add(1); 32],
            name: format!("friend{hp}"),
            avatar_pin: [hp ^ 0x55; 64],
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

        // A sponsors B (B's request carries its consent), then rotates to [A, B]: a fresh key both can open.
        bindreq_put(&b, &identity_seed, &handle_proof).expect("B posts request");
        let reqs = bindreq_list(&a, &handle_proof, &identity_seed).expect("list");
        let req_b = reqs.iter().find(|r| r.device_pubkey == pk(&b)).expect("B's request");
        bind_device(&a, &handle_proof, req_b).expect("bind B");
        let members2 = current_members(&handle_proof).unwrap();
        let (e2, k2) = rotate_fleet_key(&handle_proof, &a, &members2).expect("rotate to A,B");
        assert_eq!(e2, 2);
        assert_ne!(k2, k1);
        assert_eq!(recover_fleet_key(&handle_proof, &a).unwrap().unwrap(), k2);
        assert_eq!(recover_fleet_key(&handle_proof, &b).unwrap().unwrap(), k2);

        // B departs (self-signed — the only remove there is); A rotates to [A]: A gets the new key, B cannot — departure + rotation withhold.
        depart_device(&b, &handle_proof).expect("B departs");
        let members3 = current_members(&handle_proof).unwrap();
        assert_eq!(members3, vec![pk(&a)]);
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
        assert_eq!(current_members(&a_hp).unwrap(), vec![device.public.to_bytes()]);

        // B tries to enrol the SAME device D — rejected (device_owned, wrapped by ensure_member's establish-membership message); B's fleet stays empty.
        ensure_member(&device, &b_hp, &b_seed).expect_err("B enrol must be rejected");
        assert!(current_members(&b_hp).unwrap().is_empty(), "B must not have claimed the device");

        // A drains its inbox: a bind_attempt naming B's handle_proof.
        let events = crate::network::fgtw::inbox_drain_blocking(&device, &a_hp).expect("drain");
        assert!(
            events.iter().any(|e| e.kind == "bind_attempt" && e.attempted_by == b_hp),
            "expected a bind_attempt alert naming B; got {events:?}"
        );

        // Consume semantics: a second drain is empty.
        let again = crate::network::fgtw::inbox_drain_blocking(&device, &a_hp).expect("drain2");
        assert!(again.is_empty(), "inbox should be empty after drain; got {again:?}");

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

