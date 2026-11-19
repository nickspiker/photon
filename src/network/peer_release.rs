//! RELEASES TRAVEL PEER TO PEER (the owner 2026-11-19: "use the communications network we built to communicate, not cloudflare").
//! What travels is the signed release manifest itself, byte for byte as the deploy published it: a complete VSF document signed by the release key with every egg (fgtw::release), so a receiver checks it alone, exactly as its own poll of the update host would.
//! Each device holds the newest one it has verified and says which in every sealed pong tail (`rel`, protocol.rs PongTail); a device that hears a peer holding less hands it the manifest in a device-signed `release_tell` frame (protocol.rs).
//! The receiver verifies before it holds anything, holds only what is newer than what it holds, and passes on only what it holds: nothing unverified is ever relayed.
//! What a newer manifest makes due is the update check itself (ui/photon_app/settings.rs): tuple-forward only, then the stamp window, then the install path, which still fetches the binary from the update host for now (docs/updates.md "Peers carry the binaries" is the next step).

use crate::network::updates::{our_row, parse_manifest_signed_by, Channel, ManifestRow};

/// What a device's sealed pong tail says about the release manifests it holds.
/// Absent from a tail (an older build, or a tail-less pong) is no claim at all: such a device is never handed a manifest, since it could not read the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseClaim {
    /// It holds a verified release manifest signed at this stamp (eagle oscillations, the signed header's creation time).
    Holds(i64),
    /// It speaks release relay and holds no manifest yet.
    HoldsNone,
}

impl ReleaseClaim {
    /// Does a device making this claim hold less than a manifest signed at `stamp`?
    pub fn below(self, stamp: i64) -> bool {
        match self {
            ReleaseClaim::Holds(s) => s < stamp,
            ReleaseClaim::HoldsNone => true,
        }
    }
}

/// A release manifest this device has verified: the bytes to hand on, and what they say.
#[derive(Clone, Debug)]
pub struct HeldRelease {
    /// The manifest exactly as signed.
    pub bytes: Vec<u8>,
    /// Its signed creation stamp, the stamp window's `t`.
    pub stamp: i64,
    /// BLAKE3 of the bytes: a copy already held is known without verifying it again.
    pub hash: [u8; 32],
    /// Every release row it carries, one per platform and arch.
    pub rows: Vec<ManifestRow>,
}

/// Check a release manifest from anyone: every release egg verifies against the pinned key, it carries release rows, and its stamp is not ahead of `now`.
/// A forward-dated manifest is refused rather than held: held, its stamp would outrank every honest release and stop them travelling, so only a stamp at or before now is ever passed on.
pub fn verify(bytes: &[u8], now: i64) -> Result<HeldRelease, String> {
    verify_by(bytes, now, &fgtw::release::RELEASE_ED25519, &fgtw::release::release_bundle())
}

/// [`verify`] against any pinned key: the release key in every client, a test key in the tests.
pub fn verify_by(bytes: &[u8], now: i64, ed: &[u8; 32], bundle: &fgtw::pq::KeyBundle) -> Result<HeldRelease, String> {
    let (stamp, rows) = parse_manifest_signed_by(bytes, Channel::Release, ed, bundle)?;
    if stamp > now {
        return Err("the manifest is stamped ahead of our clock — not held, not passed on".to_string());
    }
    Ok(HeldRelease { bytes: bytes.to_vec(), stamp, hash: *blake3::hash(bytes).as_bytes(), rows })
}

/// A release manifest our own poll just verified (parse_manifest_stamped), held under the same rule as [`verify`]: never one stamped ahead of `now`. The poll keeps its one verification.
pub fn held_from_poll(bytes: Vec<u8>, stamp: i64, rows: Vec<ManifestRow>, now: i64) -> Option<HeldRelease> {
    (stamp <= now).then(|| HeldRelease { hash: *blake3::hash(&bytes).as_bytes(), bytes, stamp, rows })
}

/// What taking a verified manifest did.
#[derive(Debug, Default)]
pub struct Taken {
    /// It is newer than the one held and is held now: it is passed on to the peers that hold less.
    pub newer: bool,
    /// A peer handed it and it offers this build a newer version than it runs: the update check is due with it, (the signed stamp, our platform's row).
    pub check_due: Option<(i64, ManifestRow)>,
}

/// What one peer device is known to hold, and what was last handed to it.
#[derive(Clone, Debug)]
struct Peer {
    device: [u8; 32],
    /// Its last claim: from its pong tail, or the stamp it handed us.
    claim: ReleaseClaim,
    /// The newest stamp it handed us: never handed back to it, however far its next pong lags behind its own gift.
    gave: Option<i64>,
    /// The manifest stamp handed to it and the claim it stood at when we did: the same pair again is the same link already used, so nothing goes twice.
    told: Option<(i64, ReleaseClaim)>,
}

/// The relay's state on one device: the newest manifest held, and each peer's claim.
#[derive(Default)]
pub struct Relay {
    held: Option<HeldRelease>,
    peers: Vec<Peer>,
}

impl Relay {
    /// The manifest held, if any.
    pub fn held(&self) -> Option<&HeldRelease> {
        self.held.as_ref()
    }

    /// Are these exact bytes the manifest held? A copy we hold is not verified a second time.
    pub fn holds_hash(&self, hash: &[u8; 32]) -> bool {
        self.held.as_ref().is_some_and(|h| h.hash == *hash)
    }

    /// Take a verified manifest, from our own poll (`from` None) or handed by device `from`.
    /// True when it is newer than the one held and is now the one held; the giver is noted as holding it either way, so it is never handed back.
    pub fn offer(&mut self, rel: HeldRelease, from: Option<[u8; 32]>) -> bool {
        if let Some(dev) = from {
            let p = self.peer_mut(dev);
            if p.claim.below(rel.stamp) {
                p.claim = ReleaseClaim::Holds(rel.stamp);
            }
            if p.gave.is_none_or(|g| g < rel.stamp) {
                p.gave = Some(rel.stamp);
            }
        }
        if self.held.as_ref().is_some_and(|h| h.stamp >= rel.stamp) {
            return false;
        }
        self.held = Some(rel);
        true
    }

    /// Take a verified manifest and say what it makes due, for a build running `ours`.
    /// Only a peer's manifest makes the check due here: our own poll ran the check on the same manifest already.
    pub fn take(&mut self, rel: HeldRelease, from: Option<[u8; 32]>, ours: (usize, usize, usize)) -> Taken {
        let due = from.and(our_row(&rel.rows)).filter(|row| row.version > ours).map(|row| (rel.stamp, row));
        let newer = self.offer(rel, from);
        Taken { newer, check_due: due.filter(|_| newer) }
    }

    /// A device's pong said `claim`: true when it holds less than we do and this link has not carried our manifest at that claim yet, so it is handed over now (and marked handed).
    pub fn heard(&mut self, device: [u8; 32], claim: ReleaseClaim) -> bool {
        let held = self.held.as_ref().map(|h| h.stamp);
        let p = self.peer_mut(device);
        p.claim = claim;
        let Some(stamp) = held else {
            return false;
        };
        Self::owe(p, stamp)
    }

    /// The devices among `candidates`, in their order, that hold less than our manifest and have not been handed it: each is marked handed.
    /// Only a device we have heard a claim from is a candidate at all (an older build never claims, and could not read the frame).
    pub fn owed(&mut self, candidates: &[[u8; 32]]) -> Vec<[u8; 32]> {
        let Some(stamp) = self.held.as_ref().map(|h| h.stamp) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for dev in candidates {
            if let Some(p) = self.peers.iter_mut().find(|p| p.device == *dev) {
                if Self::owe(p, stamp) {
                    out.push(*dev);
                }
            }
        }
        out
    }

    /// The hand-over to `device` found no path: it is owed again, and its next pong (which proves a path) carries it.
    pub fn untold(&mut self, device: &[u8; 32]) {
        if let Some(p) = self.peers.iter_mut().find(|p| p.device == *device) {
            p.told = None;
        }
    }

    /// A device went offline: what it holds when it is back is told by its next pong, and a link it used is fresh again (it may have restarted holding nothing).
    pub fn forget(&mut self, device: &[u8; 32]) {
        self.peers.retain(|p| p.device != *device);
    }

    fn owe(p: &mut Peer, stamp: i64) -> bool {
        if !p.claim.below(stamp) || p.gave.is_some_and(|g| g >= stamp) || p.told == Some((stamp, p.claim)) {
            return false;
        }
        p.told = Some((stamp, p.claim));
        true
    }

    fn peer_mut(&mut self, device: [u8; 32]) -> &mut Peer {
        if let Some(i) = self.peers.iter().position(|p| p.device == device) {
            return &mut self.peers[i];
        }
        self.peers.push(Peer { device, claim: ReleaseClaim::HoldsNone, gave: None, told: None });
        self.peers.last_mut().unwrap()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use vsf::VsfType;

    /// The seed of the key the tests sign with, standing in for the release key (whose secret never leaves the maintainer's cold store).
    pub(crate) const TEST_SEED: [u8; 32] = [0x42; 32];

    /// The test key's pins: its Ed25519 public half and its public bundle, every scheme.
    pub(crate) fn test_pins() -> ([u8; 32], fgtw::pq::KeyBundle) {
        let ed = ed25519_dalek::SigningKey::from_bytes(&TEST_SEED).verifying_key().to_bytes();
        (ed, fgtw::pq::SigningBundle::release(&TEST_SEED).public())
    }

    /// A release manifest signed by the key of `seed` (every release egg) at `stamp`, carrying one release row for this build's platform at `version`.
    pub(crate) fn manifest(seed: &[u8; 32], stamp: i64, version: (usize, usize, usize)) -> Vec<u8> {
        let (platform, arch) = crate::network::updates::our_platform();
        let mut section = vsf::VsfSection::new(Channel::Release.section_name());
        section.add_field("platform", VsfType::x(platform.to_string()));
        section.add_field("arch", VsfType::x(arch.to_string()));
        section.add_field("minor", VsfType::z(version.1));
        if version.2 != 0 {
            section.add_field("patch", VsfType::z(version.2));
        }
        section.add_field("url", VsfType::nu("https://example.invalid/photon".to_string()));
        section.add_field("hash", VsfType::hb(vec![7u8; 32]));
        let signer = ed25519_dalek::SigningKey::from_bytes(seed).verifying_key().to_bytes();
        let unsigned = vsf::VsfBuilder::new()
            .creation_time_oscillations(stamp)
            .signed_only_eggs(VsfType::ke(signer.to_vec()), &fgtw::release::release_slots())
            .add_section_direct(section)
            .build()
            .unwrap();
        fgtw::release::sign_release(unsigned, &fgtw::pq::SigningBundle::release(seed)).unwrap()
    }

    /// Verify as a client does, with the test key standing in for the release key.
    pub(crate) fn verify_test(bytes: &[u8], now: i64) -> Result<HeldRelease, String> {
        let (ed, bundle) = test_pins();
        verify_by(bytes, now, &ed, &bundle)
    }

    fn held(stamp: i64) -> HeldRelease {
        HeldRelease { bytes: stamp.to_le_bytes().to_vec(), stamp, hash: *blake3::hash(&stamp.to_le_bytes()).as_bytes(), rows: Vec::new() }
    }

    /// A newer version than any build runs.
    const AHEAD: (usize, usize, usize) = (0, 1 << 20, 0);

    /// A genuine manifest verifies with its stamp, rows and hash; the real client pin refuses it, since it is not the release key's.
    #[test]
    fn a_genuine_manifest_verifies_against_its_pin_only() {
        let now = vsf::eagle_time_oscillations();
        let bytes = manifest(&TEST_SEED, now - 1, AHEAD);
        let rel = verify_test(&bytes, now).expect("a manifest under the pinned key verifies");
        assert_eq!(rel.stamp, now - 1);
        assert_eq!(our_row(&rel.rows).map(|r| r.version), Some(AHEAD));
        assert_eq!(rel.hash, *blake3::hash(&bytes).as_bytes());
        assert!(verify(&bytes, now).is_err(), "the client pins the release key, and this is not it");
    }

    /// A forged manifest is refused: another key signing the way the release key does, a byte touched after signing, a stamp ahead of now.
    #[test]
    fn a_forged_manifest_is_refused() {
        let now = vsf::eagle_time_oscillations();
        let forged = manifest(&[0x17; 32], now - 1, AHEAD);
        assert!(verify_test(&forged, now).is_err(), "another key is refused");
        let mut touched = manifest(&TEST_SEED, now - 1, AHEAD);
        let at = touched.len() - 3;
        touched[at] ^= 1;
        assert!(verify_test(&touched, now).is_err(), "a byte changed after signing is refused");
        let ahead = manifest(&TEST_SEED, now + (1 << 40), AHEAD);
        assert!(verify_test(&ahead, now).is_err(), "a stamp ahead of now is never held, so it can never outrank an honest release");
    }

    /// A verified manifest from a peer that offers a newer version makes the update check due, with the signed stamp and our platform's row.
    #[test]
    fn a_verified_manifest_from_a_peer_makes_the_check_due() {
        let now = vsf::eagle_time_oscillations();
        let rel = verify_test(&manifest(&TEST_SEED, now - 1, AHEAD), now).unwrap();
        let mut r = Relay::default();
        let t = r.take(rel, Some([5; 32]), crate::network::updates::our_version());
        assert!(t.newer, "it is held");
        let (stamp, row) = t.check_due.expect("the check is due");
        assert_eq!((stamp, row.version), (now - 1, AHEAD));
        assert_eq!(r.held().map(|h| h.stamp), Some(now - 1));
    }

    /// An older manifest makes nothing due: one older than the one held is not taken, and one whose version is not ahead of the running build is held (to pass on) without a check.
    #[test]
    fn an_older_manifest_makes_nothing_due() {
        let now = vsf::eagle_time_oscillations();
        let ours = crate::network::updates::our_version();
        let mut r = Relay::default();
        let newer = verify_test(&manifest(&TEST_SEED, now - 1, AHEAD), now).unwrap();
        assert!(r.take(newer, None, ours).check_due.is_none(), "our own poll ran its own check");
        let older = verify_test(&manifest(&TEST_SEED, now - 2, AHEAD), now).unwrap();
        let t = r.take(older, Some([6; 32]), ours);
        assert!(!t.newer && t.check_due.is_none(), "older than the one held: nothing");
        let mut fresh = Relay::default();
        let same = verify_test(&manifest(&TEST_SEED, now - 1, ours), now).unwrap();
        let t = fresh.take(same, Some([6; 32]), ours);
        assert!(t.newer && t.check_due.is_none(), "the version we run: held to pass on, no check");
    }

    /// Only a newer manifest replaces the one held; an older or equal one changes nothing.
    #[test]
    fn only_a_newer_manifest_is_held() {
        let mut r = Relay::default();
        assert!(r.offer(held(20), None), "the first manifest is held");
        assert!(!r.offer(held(10), Some([1; 32])), "an older one is not");
        assert!(!r.offer(held(20), Some([2; 32])), "the same stamp is not");
        assert_eq!(r.held().map(|h| h.stamp), Some(20));
        assert!(r.offer(held(30), Some([3; 32])), "a newer one is");
        assert_eq!(r.held().map(|h| h.stamp), Some(30));
        assert!(r.holds_hash(&held(30).hash) && !r.holds_hash(&held(20).hash));
    }

    /// Each release travels each link once: a peer behind us is handed the manifest on its first claim and not on the same claim again, a giver is never handed back what it gave, a peer at or past us is handed nothing, and a newer manifest is owed to the peers still behind, each once.
    #[test]
    fn each_release_travels_each_link_once() {
        let mut r = Relay::default();
        assert!(!r.heard([1; 32], ReleaseClaim::HoldsNone), "nothing held, nothing to hand");
        r.offer(held(20), None);
        assert!(r.heard([1; 32], ReleaseClaim::HoldsNone), "a peer holding nothing is handed ours");
        assert!(!r.heard([1; 32], ReleaseClaim::HoldsNone), "the same claim again is the same link: not twice");
        assert!(!r.heard([1; 32], ReleaseClaim::Holds(20)), "it holds ours now");
        assert!(!r.heard([2; 32], ReleaseClaim::Holds(25)), "a peer ahead of us is handed nothing");
        assert!(r.heard([3; 32], ReleaseClaim::Holds(10)), "a peer behind is handed ours");
        assert!(r.offer(held(30), Some([4; 32])), "a peer hands us a newer one");
        assert!(!r.heard([4; 32], ReleaseClaim::Holds(20)), "its pong lagging behind its own gift: the giver is never handed back what it gave");
        assert_eq!(r.owed(&[[3; 32], [1; 32], [2; 32], [4; 32], [9; 32]]), vec![[3; 32], [1; 32], [2; 32]], "the newer manifest is owed to the peers behind, in the order asked; a device never heard from is no candidate");
        assert!(r.owed(&[[3; 32], [1; 32], [2; 32]]).is_empty(), "owed once");
        r.forget(&[1; 32]);
        assert!(r.heard([1; 32], ReleaseClaim::HoldsNone), "after its offline edge it is a fresh link");
        assert!(r.heard([3; 32], ReleaseClaim::HoldsNone), "a peer whose claim moved, still behind, restarted or lost it: handed again");
    }
}
