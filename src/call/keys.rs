//! Call-key derivation — the DOZEN-EGG basket, no lone ECDH (docs/calls.md).
//!
//! A single ephemeral exchange would be one egg — and the only quantum-soft link in the whole system (everything else here is symmetric/hash-based; a record-now-decrypt-later adversary gets nothing from the lanes). So the call secret derives from what both fleets already hold, three independent lineages deep:
//! - `lane_root` — the era secret, ceremony-born, every device of both fleets (it's what makes receive-anywhere work at all);
//! - `history_key` — born at the same ceremony but derived OUTSIDE the ratchet (spaghettify over the pristine chains) — breaking one lineage says nothing about the other;
//! - the **lane key the call-offer was sealed under** — doomed material: the ratchet destroys it as the lane advances past, which is the per-call forward-secrecy egg, and any device able to decrypt the offer necessarily held it (the caller captures it at `prepare_send_encrypt`, each callee device at decrypt).
//!
//! Public uniquifiers (`call_id`, both nonces) ride the offer/answer rows. Per-direction subkeys keep the two streams off one keystream; the packet nonce is the global sequence number.
//!
//! **Intra-call ratchet**: the step key advances every [`PACKETS_PER_STEP`] packets — count-based, never a timer — and the old step is zeroized. A device compromised at minute ten cannot decrypt minute one of a recording; hangup zeroizes the whole chain and the call is cryptographically GONE (§14.8 doctrine on media: destruction = key death, never "we deleted the recording we never made" — the deliberate local spool in `spool.rs` is endpoint memory, a different object with its own keys).

use zeroize::Zeroize;

/// Packets per ratchet step — 100 × 10ms frames = one step every ~2s of talk, cheap enough to be free.
pub const PACKETS_PER_STEP: u32 = 100;

/// Media stream direction: caller→callee or callee→caller. Wire value = the `dir` header byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    CallerToCallee,
    CalleeToCaller,
}

impl Direction {
    pub fn wire(self) -> u8 {
        match self {
            Direction::CallerToCallee => 0,
            Direction::CalleeToCaller => 1,
        }
    }

    pub fn from_wire(b: u8) -> Option<Self> {
        match b {
            0 => Some(Direction::CallerToCallee),
            1 => Some(Direction::CalleeToCaller),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Direction::CallerToCallee => "c>e",
            Direction::CalleeToCaller => "e>c",
        }
    }
}

/// Derive the per-call root secret from the basket. Every input is fixed-width or length-prefixed by position, so the concatenation is injective; blake3's derive_key binds the domain.
pub fn derive_call_secret(
    lane_root: &[u8; 32],
    history_key: &[u8; 32],
    offer_lane_key: &[u8; 32],
    call_id: &[u8; 16],
    caller_nonce: &[u8; 32],
    callee_nonce: &[u8; 32],
) -> [u8; 32] {
    let mut material = Vec::with_capacity(32 * 5 + 16);
    material.extend_from_slice(lane_root);
    material.extend_from_slice(history_key);
    material.extend_from_slice(offer_lane_key);
    material.extend_from_slice(call_id);
    material.extend_from_slice(caller_nonce);
    material.extend_from_slice(callee_nonce);
    let out = blake3::derive_key("PHOTON_CALL_v1 call secret", &material);
    material.zeroize();
    out
}

/// One direction's stepping key chain. `advance_to` walks forward (zeroizing each superseded step) and refuses to walk back — the whole point is that earlier steps no longer exist anywhere.
pub struct StepChain {
    key: [u8; 32],
    step: u32,
}

impl StepChain {
    /// Step 0 key for a direction, from the call secret.
    pub fn new(call_secret: &[u8; 32], dir: Direction) -> Self {
        let mut material = Vec::with_capacity(32 + 3);
        material.extend_from_slice(call_secret);
        material.extend_from_slice(dir.label().as_bytes());
        let key = blake3::derive_key("PHOTON_CALL_v1 direction step0", &material);
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

    /// Walk forward to `target` step, zeroizing every superseded key. Returns false (untouched) on a backward ask — past steps are destroyed by design; a reordered packet from a dead step is silence, not a rewind.
    pub fn advance_to(&mut self, target: u32) -> bool {
        if target < self.step {
            return false;
        }
        while self.step < target {
            let next = blake3::derive_key("PHOTON_CALL_v1 step", &self.key);
            self.key.zeroize();
            self.key = next;
            self.step += 1;
        }
        true
    }
}

impl Drop for StepChain {
    fn drop(&mut self) {
        // Hangup (or any teardown path) destroys the chain — the call becomes undecryptable everywhere, forever.
        self.key.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basket() -> [u8; 32] {
        derive_call_secret(
            &[0x11; 32],
            &[0x22; 32],
            &[0x33; 32],
            &[0x44; 16],
            &[0x55; 32],
            &[0x66; 32],
        )
    }

    /// Frozen contract: if this ever changes, an updated device cannot talk to a not-yet-updated one mid-rollout — the KAT is the tripwire, same doctrine as the device-keypair KAT in fgtw. The literal was computed by an INDEPENDENT program (blake3 1.5 direct, outside this crate) on 2026-08-18; regenerate ONLY with a version bump on the context string.
    #[test]
    fn call_secret_known_answer() {
        assert_eq!(
            hex::encode(basket()),
            "6f12ef0cdc0cbb011d795c6645de39a8a2f62336b4bf45ad4ce08d57329637ce"
        );
    }

    #[test]
    fn every_basket_ingredient_changes_the_secret() {
        let base = basket();
        let variants = [
            derive_call_secret(&[0x12; 32], &[0x22; 32], &[0x33; 32], &[0x44; 16], &[0x55; 32], &[0x66; 32]),
            derive_call_secret(&[0x11; 32], &[0x23; 32], &[0x33; 32], &[0x44; 16], &[0x55; 32], &[0x66; 32]),
            derive_call_secret(&[0x11; 32], &[0x22; 32], &[0x34; 32], &[0x44; 16], &[0x55; 32], &[0x66; 32]),
            derive_call_secret(&[0x11; 32], &[0x22; 32], &[0x33; 32], &[0x45; 16], &[0x55; 32], &[0x66; 32]),
            derive_call_secret(&[0x11; 32], &[0x22; 32], &[0x33; 32], &[0x44; 16], &[0x56; 32], &[0x66; 32]),
            derive_call_secret(&[0x11; 32], &[0x22; 32], &[0x33; 32], &[0x44; 16], &[0x55; 32], &[0x67; 32]),
        ];
        for (i, v) in variants.iter().enumerate() {
            assert_ne!(&base, v, "ingredient {} must reach the KDF", i);
        }
    }

    #[test]
    fn directions_never_share_a_keystream() {
        let s = basket();
        let a = StepChain::new(&s, Direction::CallerToCallee);
        let b = StepChain::new(&s, Direction::CalleeToCaller);
        assert_ne!(a.key(), b.key());
    }

    #[test]
    fn ratchet_is_forward_only_and_convergent() {
        let s = basket();
        // Two independent walkers (sender and a mid-call joiner) converge at the same step.
        let mut sender = StepChain::new(&s, Direction::CallerToCallee);
        let mut joiner = StepChain::new(&s, Direction::CallerToCallee);
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
