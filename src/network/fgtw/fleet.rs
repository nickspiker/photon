//! FGTW client adapter — photon's binding of the shared `fgtw` crate to its own HTTP + storage stack.
//!
//! All the substrate now lives in the crate, re-exported here so photon's `crate::network::fgtw::fleet::*` call sites are unchanged:
//! `fgtw::fleet` (the membership chain), `fgtw::fanout` (fan-out crypto), `fgtw::fstate` (roster codec), `fgtw::pair` (pairing words), and `fgtw::client` (the fetch-then-sign oracle).
//! What's left here is the *binding*: [`PhotonTransport`] (FGTW's HTTP over photon's warm-TLS pool + short error UX) and [`PhotonSealer`] (roster AEAD over `kete`), plus thin same-signature wrappers that inject them — so the crate stays reqwest-free and photon keeps its own network stack.

pub use fgtw::fanout::{
    fanout_from_bytes, fanout_open, fanout_seal, fanout_to_bytes, new_fleet_key, FanoutWrap,
};
pub use fgtw::fleet::{et_to_osc, scheme, Egg, FleetOp, FoldError, MembershipBlob, OpKind};
pub use fgtw::fstate::{merge_rosters, roster_from_bytes, roster_to_bytes, RosterEntry};
pub use fgtw::pair::{
    device_name_default, first_bad_pair_word, new_pairing_id, pair_entry_complete,
    pair_matched_signing_bytes, pair_request_signing_bytes, pair_word_list, pair_word_tokens,
    pair_words, parse_pair_event, words_to_pair_pubkey, PairRequest, PAIR_WORD_COUNT,
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

/// Existing-device side of device-ADD: add `new_pubkey`, signed by this member device.
pub fn bind_device(
    member_key: &Keypair,
    handle_proof: &[u8; 32],
    new_pubkey: [u8; 32],
) -> Result<(), String> {
    fgtw::client::bind_device(&PhotonTransport, member_key, handle_proof, new_pubkey)
}

/// Existing-device side of device removal: remove `target_pubkey`, signed by this member device.
pub fn unbind_device(
    member_key: &Keypair,
    handle_proof: &[u8; 32],
    target_pubkey: [u8; 32],
) -> Result<(), String> {
    fgtw::client::unbind_device(&PhotonTransport, member_key, handle_proof, target_pubkey)
}

/// NEW device: post its pairing request (signed by the pairing key).
pub fn post_pairing_request(
    pairing: &Keypair,
    new_device_pubkey: &[u8; 32],
    handle_proof: &[u8; 32],
) -> Result<(), String> {
    fgtw::client::post_pairing_request(&PhotonTransport, pairing, new_device_pubkey, handle_proof)
}

/// EXISTING device: fetch the pending pairing request (freshness + ownership-signature checked).
pub fn fetch_pairing_request(handle_proof: &[u8; 32]) -> Result<Option<PairRequest>, String> {
    fgtw::client::fetch_pairing_request(&PhotonTransport, handle_proof)
}

/// EXISTING device: post the signed "matched" flag so the new device's screen flips to ready.
pub fn post_pair_matched(
    member_key: &Keypair,
    handle_proof: &[u8; 32],
    pairing_pubkey: &[u8; 32],
) -> Result<(), String> {
    fgtw::client::post_pair_matched(&PhotonTransport, member_key, handle_proof, pairing_pubkey)
}

/// NEW device: has an existing member matched OUR words? (Verified against the member set.)
pub fn poll_pair_matched(
    handle_proof: &[u8; 32],
    pairing_pubkey: &[u8; 32],
    members: &[[u8; 32]],
) -> Result<bool, String> {
    fgtw::client::poll_pair_matched(&PhotonTransport, handle_proof, pairing_pubkey, members)
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

/// Publish the fleet roster: seal under the fleet key (kete) and PUT to the membership-gated slot.
pub fn push_roster(
    handle_proof: &[u8; 32],
    device_key: &Keypair,
    fleet_key: &[u8; 32],
    entries: &[RosterEntry],
) -> Result<(), String> {
    fgtw::client::push_roster(&PhotonTransport, &PhotonSealer, handle_proof, device_key, fleet_key, entries)
}

/// Fetch + open the fleet roster (None if none published yet).
pub fn pull_roster(
    handle_proof: &[u8; 32],
    fleet_key: &[u8; 32],
) -> Result<Option<Vec<RosterEntry>>, String> {
    fgtw::client::pull_roster(&PhotonTransport, &PhotonSealer, handle_proof, fleet_key)
}

#[cfg(test)]
mod tests {
    use super::*;

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
