//! Wave-key derivation — the DOZEN-EGG basket, no lone ECDH (docs/waves.md).
//!
//! A single ephemeral exchange would be one egg — and the only quantum-soft link in the whole system (everything else here is symmetric/hash-based; a record-now-decrypt-later adversary gets nothing from the lanes). So the wave secret derives from what both fleets already hold, three independent lineages deep:
//! - `lane_root` — the era secret, ceremony-born, every device of both fleets (it's what makes receive-anywhere work at all);
//! - `history_key` — born at the same ceremony but derived OUTSIDE the ratchet (spaghettify over the pristine chains) — breaking one lineage says nothing about the other;
//! - the **lane key the wave-offer was sealed under** — doomed material: the ratchet destroys it as the lane advances past, which is the per-wave forward-secrecy egg, and any device able to decrypt the offer necessarily held it (the origin captures it at `prepare_send_encrypt`, each answering device at decrypt).
//!
//! Public uniquifiers (`wave_id`, both nonces) ride the offer/answer rows. Per-direction subkeys keep the two streams off one keystream; the packet nonce is the global sequence number.
//!
//! **Intra-wave ratchet**: the step key advances every [`PACKETS_PER_STEP`] packets — count-based, never a timer — and the old step is zeroized. A device compromised at minute ten cannot decrypt minute one of a recording; hangup zeroizes the whole chain and the wave is cryptographically GONE (§14.8 doctrine on media: destruction = key death, never "we deleted the recording we never made" — the deliberate local spool in `spool.rs` is endpoint memory, a different object with its own keys).

use zeroize::Zeroize;

/// Packets per ratchet step — 100 × 10ms frames = one step every ~2s of talk, cheap enough to be free.
pub const PACKETS_PER_STEP: u32 = 100;

/// How far ahead of the chain an unverified packet may ask the receiver to look: 2^16 steps is 6.5 million packets, more than a day of media at the fastest rung, so a wave that waited out an outage still opens; past it the walk itself would be the attack (an unauthenticated header buying a million hashes).
pub const MAX_LOOKAHEAD_STEPS: u32 = 1 << 16;

/// Media stream direction: origin→answer or answer→origin. Wire value = the `dir` header byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    OriginToAnswer,
    AnswerToOrigin,
}

impl Direction {
    pub fn wire(self) -> u8 {
        match self {
            Direction::OriginToAnswer => 0,
            Direction::AnswerToOrigin => 1,
        }
    }

    pub fn from_wire(b: u8) -> Option<Self> {
        match b {
            0 => Some(Direction::OriginToAnswer),
            1 => Some(Direction::AnswerToOrigin),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Direction::OriginToAnswer => "o>a",
            Direction::AnswerToOrigin => "a>o",
        }
    }
}

/// Derive the per-wave root secret from the basket. Every input is fixed-width or length-prefixed by position, so the concatenation is injective; blake3's derive_key binds the domain.
/// The basket's second cut (2026-09-10; the context string reset to `PHOTON_WAVE_v1` on the 2026-09-23 flag day, so the label no longer carries that history): the friendship's CURRENT lane_root + history_key are NOT ingredients any more. They were, and one era of skew between the two fleets (Emma re-keyed, Nick's phone had not) gave the two ends two different secrets with a perfectly good offer/answer exchange — "seen 293, open-drop 293, decoded 0" on BOTH sides. The offer lane key is already a per-era lane secret the origin sealed and the answering side captured at decrypt, so the secret stays bound to the friendship's key material thru it, era-consistently by construction.
pub fn derive_wave_secret(
    offer_lane_key: &[u8; 32],
    wave_id: &[u8; 16],
    origin_nonce: &[u8; 32],
    answer_nonce: &[u8; 32],
) -> [u8; 32] {
    let mut material = Vec::with_capacity(32 * 3 + 16);
    material.extend_from_slice(offer_lane_key);
    material.extend_from_slice(wave_id);
    material.extend_from_slice(origin_nonce);
    material.extend_from_slice(answer_nonce);
    let out = blake3::derive_key("PHOTON_WAVE_v1 wave secret", &material);
    material.zeroize();
    out
}

/// One LINK of a group wave (docs/group-waves.md): every joined device pair runs its own media plane, keyed from the same offer egg as a 1:1 wave but bound to both devices and both join nonces.
/// The pair is ordered by device key, so both ends derive the same secret, and the lower device plays the origin's direction (`group_link_origin`).
/// Its own context string keeps a link's secret apart from any 1:1 wave's, whatever the inputs.
pub fn derive_group_link_secret(offer_lane_key: &[u8; 32], wave_id: &[u8; 16], a: (&[u8; 32], &[u8; 32]), b: (&[u8; 32], &[u8; 32])) -> [u8; 32] {
    let (lo, hi) = if a.0 <= b.0 { (a, b) } else { (b, a) };
    let mut material = Vec::with_capacity(32 * 5 + 16);
    material.extend_from_slice(offer_lane_key);
    material.extend_from_slice(wave_id);
    material.extend_from_slice(lo.0);
    material.extend_from_slice(lo.1);
    material.extend_from_slice(hi.0);
    material.extend_from_slice(hi.1);
    let out = blake3::derive_key("PHOTON_WAVE_v1 group link", &material);
    material.zeroize();
    out
}

/// Whether `ours` takes the origin's direction on its link with `theirs` (the lower device key does).
pub fn group_link_origin(ours: &[u8; 32], theirs: &[u8; 32]) -> bool {
    ours < theirs
}

/// One direction's stepping key chain. `advance_to` walks forward (zeroizing each superseded step) and refuses to walk back — the whole point is that earlier steps no longer exist anywhere.
/// The recording-fill plane's root: a domain-separated child of the wave secret, so FILL datagrams run their own StepChains and seq space beside the media ones (packet.rs FILL_MAGIC) without ever sharing a key+nonce with an audio window.
pub fn fill_secret(wave_secret: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key("PHOTON_WAVE_v1 recording fill", wave_secret)
}

pub struct StepChain {
    key: [u8; 32],
    step: u32,
}

impl StepChain {
    /// Step 0 key for a direction, from the wave secret.
    pub fn new(wave_secret: &[u8; 32], dir: Direction) -> Self {
        let mut material = Vec::with_capacity(32 + 3);
        material.extend_from_slice(wave_secret);
        material.extend_from_slice(dir.label().as_bytes());
        let key = blake3::derive_key("PHOTON_WAVE_v1 direction step0", &material);
        material.zeroize();
        Self { key, step: 0 }
    }

    pub fn step(&self) -> u32 {
        self.step
    }

    /// The current step key (send side seals under this; receive side must be AT the packet's step).
    pub fn key(&self) -> &[u8; 32] {
        &self.key
    }

    /// Which step a global packet sequence number lives in.
    pub fn step_for_seq(seq: u32) -> u32 {
        seq / PACKETS_PER_STEP
    }

    /// The key `target` steps along WITHOUT moving the chain: a receiver opens a packet against this copy and only advances once the tag holds.
    /// Walking the real chain on an unverified header let any datagram ratchet a live wave past its own keys — one forged sequence number and every later packet failed to open (2026-10-25, found sizing the group mesh, where every engine sees every link's packets).
    /// `None` behind the chain (those keys are gone) or more than [`MAX_LOOKAHEAD_STEPS`] ahead.
    pub fn key_at(&self, target: u32) -> Option<[u8; 32]> {
        if target < self.step || target - self.step > MAX_LOOKAHEAD_STEPS {
            return None;
        }
        let mut key = self.key;
        for _ in self.step..target {
            let next = blake3::derive_key("PHOTON_WAVE_v1 step", &key);
            key.zeroize();
            key = next;
        }
        Some(key)
    }

    /// Walk forward to `target` step, zeroizing every superseded key. Returns false (untouched) on a backward ask — past steps are destroyed by design; a reordered packet from a dead step is silence, not a rewind.
    pub fn advance_to(&mut self, target: u32) -> bool {
        if target < self.step {
            return false;
        }
        while self.step < target {
            let next = blake3::derive_key("PHOTON_WAVE_v1 step", &self.key);
            self.key.zeroize();
            self.key = next;
            self.step += 1;
        }
        true
    }
}

impl Drop for StepChain {
    fn drop(&mut self) {
        // Hangup (or any teardown path) destroys the chain — the wave becomes undecryptable everywhere, forever.
        self.key.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basket() -> [u8; 32] {
        derive_wave_secret(&[0x33; 32], &[0x44; 16], &[0x55; 32], &[0x66; 32])
    }

    /// Frozen contract: if this ever changes, an updated device cannot talk to a not-yet-updated one mid-rollout — the KAT is the tripwire, same doctrine as the device-keypair KAT in fgtw. The literal was computed by an INDEPENDENT program (b3sum --derive-key, outside this crate) on 2026-09-23, the wave flag day that retired the PHOTON_CALL_* context strings; regenerate ONLY with a version bump on the context string.
    /// Both ends of a group link derive the same secret whichever side computes it, exactly one of them takes the origin's direction, and the secret is nothing a 1:1 wave could derive.
    #[test]
    fn a_group_link_is_symmetric_and_its_own() {
        let (lane, id) = ([0x33; 32], [0x44; 16]);
        let (dev_a, nonce_a, dev_b, nonce_b) = ([1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32]);
        let from_a = derive_group_link_secret(&lane, &id, (&dev_a, &nonce_a), (&dev_b, &nonce_b));
        let from_b = derive_group_link_secret(&lane, &id, (&dev_b, &nonce_b), (&dev_a, &nonce_a));
        assert_eq!(from_a, from_b);
        assert!(group_link_origin(&dev_a, &dev_b) ^ group_link_origin(&dev_b, &dev_a));
        assert_ne!(from_a, derive_wave_secret(&lane, &id, &nonce_a, &nonce_b));
        // Another member's link in the same wave is a different secret.
        let other = derive_group_link_secret(&lane, &id, (&dev_a, &nonce_a), (&[5u8; 32], &[6u8; 32]));
        assert_ne!(from_a, other);
    }

    #[test]
    fn wave_secret_known_answer() {
        assert_eq!(
            hex::encode(basket()),
            "48be713c8a80427eaf77732d8940b937229818d8e7370236625371f3cd04132b"
        );
    }

    #[test]
    fn every_basket_ingredient_changes_the_secret() {
        let base = basket();
        let variants = [
            derive_wave_secret(&[0x34; 32], &[0x44; 16], &[0x55; 32], &[0x66; 32]),
            derive_wave_secret(&[0x33; 32], &[0x45; 16], &[0x55; 32], &[0x66; 32]),
            derive_wave_secret(&[0x33; 32], &[0x44; 16], &[0x56; 32], &[0x66; 32]),
            derive_wave_secret(&[0x33; 32], &[0x44; 16], &[0x55; 32], &[0x67; 32]),
        ];
        for (i, v) in variants.iter().enumerate() {
            assert_ne!(&base, v, "ingredient {} must reach the KDF", i);
        }
    }

    #[test]
    fn directions_never_share_a_keystream() {
        let s = basket();
        let a = StepChain::new(&s, Direction::OriginToAnswer);
        let b = StepChain::new(&s, Direction::AnswerToOrigin);
        assert_ne!(a.key(), b.key());
    }

    #[test]
    fn ratchet_is_forward_only_and_convergent() {
        let s = basket();
        // Two independent walkers (sender and a mid-wave joiner) converge at the same step.
        let mut sender = StepChain::new(&s, Direction::OriginToAnswer);
        let mut joiner = StepChain::new(&s, Direction::OriginToAnswer);
        assert!(sender.advance_to(3));
        assert!(joiner.advance_to(1));
        assert!(joiner.advance_to(3));
        assert_eq!(sender.key(), joiner.key());
        assert_eq!(sender.step(), 3);
        // Backward is refused, state untouched — the past is destroyed, not hidden.
        let held = *sender.key();
        assert!(!sender.advance_to(2));
        assert_eq!(*sender.key(), held);
        assert_eq!(sender.step(), 3);
    }

    #[test]
    fn step_boundaries_follow_packet_count() {
        assert_eq!(StepChain::step_for_seq(0), 0);
        assert_eq!(StepChain::step_for_seq(PACKETS_PER_STEP - 1), 0);
        assert_eq!(StepChain::step_for_seq(PACKETS_PER_STEP), 1);
        assert_eq!(StepChain::step_for_seq(PACKETS_PER_STEP * 7 + 3), 7);
    }
}
