//! The LIGHT ERA RATCHET (docs/lanes.md "Eras", plan logical-brewing-creek §1-§2): re-key an existing friendship IN BAND, without a ceremony and without destroying anything.
//!
//! The next era's secrets are `KDF(old lane_root ‖ old history_key ‖ FRESH ‖ transcript)`. The old root buys continuity and splice-resistance (nobody without it computes the next era, whatever fresh material they hold); confidentiality of the new era comes ENTIRELY from the fresh secret, a hybrid of ML-KEM-1024, X25519 and HQC-256 — two post-quantum families plus classical, ≈9 KB of public keys on the Init row and ≈16 KB of ciphertexts on the Resp row, no PT transfer, no McEliece. A departed device that captured the whole old era lacks the initiator's ephemeral decapsulation keys (RAM only, zeroized at derive) and the responder's encapsulation randomness, so it cannot follow.
//!
//! Wire grammar (`EraSignal`) follows the call-signal STX convention: a hidden control row whose content is `ERA_PREFIX kind ‖ fields`, with the KEM material riding the message package's typed `ekn`/`ekx`/`ekh` fields beside it — never inside the text.

use blake3::Hasher;
use ihi::spaghettify;
use zeroize::Zeroize;

use super::clutch::{
    generate_hqc256_keypair, generate_mlkem1024_keypair, generate_x25519_ephemeral, hqc256_decapsulate, hqc256_encapsulate, mlkem1024_decapsulate, mlkem1024_encapsulate, x25519_ecdh,
};
use crate::types::ERA_PREFIX;

/// KEM set bitmask — the set can change without a flag day; a Resp must echo the Init's set.
pub const KEM_MLKEM1024: u8 = 1;
pub const KEM_X25519: u8 = 2;
pub const KEM_HQC256: u8 = 4;
pub const KEM_SET_DEFAULT: u8 = KEM_MLKEM1024 | KEM_X25519 | KEM_HQC256;

/// The standing cadence (decision 5, 2026-09-08): a light ratchet fires on the row-count edge every 256 peer rows per friendship — the braid's history window, a constant, never a timer.
pub const LIGHT_RATCHET_CADENCE_ROWS: u32 = 256;

const ERA_FRESH_DOMAIN: &[u8] = b"PHOTON_ERA_FRESH_v\x01";
const ERA_LANE_ROOT_DOMAIN: &[u8] = b"PHOTON_ERA_LANE_ROOT_v\x01";
const ERA_HISTORY_KEY_DOMAIN: &[u8] = b"PHOTON_ERA_HISTORY_KEY_v\x01";
const ERA_TRANSCRIPT_DOMAIN: &[u8] = b"PHOTON_ERA_TRANSCRIPT_v\x01";

/// The KEM material one ratchet row carries: PUBLIC KEYS on the Init, CIPHERTEXTS on the Resp (the X25519 slot holds the responder's ephemeral public key there). A field the set excludes is empty.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EraKemWire {
    pub mlkem: Vec<u8>,
    pub x25519: Vec<u8>,
    pub hqc: Vec<u8>,
}

/// The initiator's in-flight ratchet: ephemeral decapsulation keys plus the parameters a Resp must echo (the nonce CAS). RUNTIME ONLY — never persisted; a restart aborts the ratchet and a late Resp is dropped by nonce mismatch. Zeroized on drop.
#[derive(Clone)]
pub struct EraEphemeral {
    pub era_next: u64,
    pub prior_tag: u32,
    pub nonce: [u8; 32],
    pub kem_set: u8,
    pub init_wire: EraKemWire,
    mlkem_sk: Vec<u8>,
    x_sk: [u8; 32],
    hqc_sk: Vec<u8>,
}

impl std::fmt::Debug for EraEphemeral {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EraEphemeral(era#{} prior {:08x} nonce {}… set {})", self.era_next, self.prior_tag, hex::encode(&self.nonce[..4]), self.kem_set)
    }
}

impl Drop for EraEphemeral {
    fn drop(&mut self) {
        self.mlkem_sk.zeroize();
        self.x_sk.zeroize();
        self.hqc_sk.zeroize();
    }
}

/// Mint the initiator's ephemerals for era `era_next`, ratcheting from the era whose public tag is `prior_tag`.
pub fn era_keygen(era_next: u64, prior_tag: u32, kem_set: u8) -> EraEphemeral {
    let mut nonce = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut nonce);
    let (mlkem_sk, mlkem_pk) = if kem_set & KEM_MLKEM1024 != 0 { generate_mlkem1024_keypair() } else { (Vec::new(), Vec::new()) };
    let (x_sk, x_pk) = if kem_set & KEM_X25519 != 0 { generate_x25519_ephemeral() } else { ([0u8; 32], [0u8; 32]) };
    let (hqc_sk, hqc_pk) = if kem_set & KEM_HQC256 != 0 { generate_hqc256_keypair() } else { (Vec::new(), Vec::new()) };
    EraEphemeral {
        era_next,
        prior_tag,
        nonce,
        kem_set,
        init_wire: EraKemWire { mlkem: mlkem_pk, x25519: if kem_set & KEM_X25519 != 0 { x_pk.to_vec() } else { Vec::new() }, hqc: hqc_pk },
        mlkem_sk,
        x_sk,
        hqc_sk,
    }
}

/// Responder: encapsulate to the Init's public keys. Returns the Resp wire (ciphertexts) and the fresh secret F. None on malformed material.
pub fn era_encapsulate(init: &EraKemWire, kem_set: u8) -> Option<(EraKemWire, [u8; 32])> {
    let mut resp = EraKemWire::default();
    let mut secrets: Vec<(&'static str, Vec<u8>)> = Vec::new();
    if kem_set & KEM_MLKEM1024 != 0 {
        let (ct, ss) = mlkem1024_encapsulate(&init.mlkem)?;
        resp.mlkem = ct;
        secrets.push(("mlkem1024", ss));
    }
    if kem_set & KEM_X25519 != 0 {
        let their: [u8; 32] = init.x25519.as_slice().try_into().ok()?;
        let (mut sk, pk) = generate_x25519_ephemeral();
        let ss = x25519_ecdh(&sk, &their);
        sk.zeroize();
        resp.x25519 = pk.to_vec();
        secrets.push(("x25519", ss.to_vec()));
    }
    if kem_set & KEM_HQC256 != 0 {
        let (ct, ss) = hqc256_encapsulate(&init.hqc)?;
        resp.hqc = ct;
        secrets.push(("hqc256", ss));
    }
    if secrets.is_empty() {
        return None;
    }
    let fresh = derive_era_fresh(&secrets);
    for (_, s) in secrets.iter_mut() {
        s.zeroize();
    }
    Some((resp, fresh))
}

/// Initiator: decapsulate the Resp against our ephemerals. Returns F. None on malformed material.
pub fn era_decapsulate(eph: &EraEphemeral, resp: &EraKemWire) -> Option<[u8; 32]> {
    let mut secrets: Vec<(&'static str, Vec<u8>)> = Vec::new();
    if eph.kem_set & KEM_MLKEM1024 != 0 {
        secrets.push(("mlkem1024", mlkem1024_decapsulate(&eph.mlkem_sk, &resp.mlkem)?));
    }
    if eph.kem_set & KEM_X25519 != 0 {
        let their: [u8; 32] = resp.x25519.as_slice().try_into().ok()?;
        secrets.push(("x25519", x25519_ecdh(&eph.x_sk, &their).to_vec()));
    }
    if eph.kem_set & KEM_HQC256 != 0 {
        secrets.push(("hqc256", hqc256_decapsulate(&eph.hqc_sk, &resp.hqc)?));
    }
    if secrets.is_empty() {
        return None;
    }
    let fresh = derive_era_fresh(&secrets);
    for (_, s) in secrets.iter_mut() {
        s.zeroize();
    }
    Some(fresh)
}

/// Fold the labeled KEM secrets into one fresh secret — the labeled-egg discipline: `DOMAIN ‖ count ‖ (label_len ‖ label ‖ secret_len ‖ secret)*`, injective framing, thru spaghettify. Input zeroized.
pub fn derive_era_fresh(secrets: &[(&str, Vec<u8>)]) -> [u8; 32] {
    let mut input = Vec::with_capacity(ERA_FRESH_DOMAIN.len() + 4 + secrets.iter().map(|(l, s)| 8 + l.len() + s.len()).sum::<usize>());
    input.extend_from_slice(ERA_FRESH_DOMAIN);
    input.extend_from_slice(&(secrets.len() as u32).to_le_bytes());
    for (label, secret) in secrets {
        input.extend_from_slice(&(label.len() as u32).to_le_bytes());
        input.extend_from_slice(label.as_bytes());
        input.extend_from_slice(&(secret.len() as u32).to_le_bytes());
        input.extend_from_slice(secret);
    }
    let out = spaghettify(&input);
    input.zeroize();
    out
}

/// The transcript both sides bind the new era to: every PUBLIC input of the exchange (token, era, nonce, set, both wires). blake3 — nothing secret here, bit-portable.
pub fn derive_era_transcript(token: &[u8; 32], era_next: u64, nonce: &[u8; 32], kem_set: u8, init: &EraKemWire, resp: &EraKemWire) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(ERA_TRANSCRIPT_DOMAIN);
    h.update(token);
    h.update(&era_next.to_le_bytes());
    h.update(nonce);
    h.update(&[kem_set]);
    for w in [init, resp] {
        for part in [&w.mlkem, &w.x25519, &w.hqc] {
            h.update(&(part.len() as u32).to_le_bytes());
            h.update(part);
        }
    }
    *h.finalize().as_bytes()
}

/// Derive the next era's (lane_root, history_key). body = friendship_id ‖ era_index ‖ old_lane_root ‖ hk_present ‖ old_history_key|zeros ‖ fresh_len ‖ fresh ‖ transcript — injective, both keys from one body under two domains, the body zeroized after.
pub fn derive_era_keys(friendship_id: &[u8; 32], era_index: u64, old_lane_root: &[u8; 32], old_history_key: Option<&[u8; 32]>, fresh: &[u8; 32], transcript: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let mut body = Vec::with_capacity(32 + 8 + 32 + 1 + 32 + 4 + 32 + 32);
    body.extend_from_slice(friendship_id);
    body.extend_from_slice(&era_index.to_le_bytes());
    body.extend_from_slice(old_lane_root);
    match old_history_key {
        Some(hk) => {
            body.push(1);
            body.extend_from_slice(hk);
        }
        None => {
            body.push(0);
            body.extend_from_slice(&[0u8; 32]);
        }
    }
    body.extend_from_slice(&(fresh.len() as u32).to_le_bytes());
    body.extend_from_slice(fresh);
    body.extend_from_slice(transcript);
    let mut root_in = Vec::with_capacity(ERA_LANE_ROOT_DOMAIN.len() + body.len());
    root_in.extend_from_slice(ERA_LANE_ROOT_DOMAIN);
    root_in.extend_from_slice(&body);
    let mut hk_in = Vec::with_capacity(ERA_HISTORY_KEY_DOMAIN.len() + body.len());
    hk_in.extend_from_slice(ERA_HISTORY_KEY_DOMAIN);
    hk_in.extend_from_slice(&body);
    let root = spaghettify(&root_in);
    let hk = spaghettify(&hk_in);
    root_in.zeroize();
    hk_in.zeroize();
    body.zeroize();
    (root, hk)
}

/// The three ratchet rows. Init and Resp carry KEM material in the package's typed fields; the text carries only the parameters both sides must agree on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EraSignal {
    /// Initiator → responder: "ratchet from the era tagged prior_tag to era_next; here are my public keys".
    Init { era_next: u64, nonce: [u8; 32], prior_tag: u32, kem_set: u8 },
    /// Responder → initiator: "encapsulated; here are the ciphertexts" — the responder holds era_next as PENDING from this moment.
    Resp { era_next: u64, nonce: [u8; 32], prior_tag: u32 },
    /// Either way: "please initiate a ratchet from prior_tag" — sent by the identity that may not initiate (the higher party id) toward the one that may.
    Nudge { prior_tag: u32 },
}

impl EraSignal {
    pub fn kind(&self) -> &'static str {
        match self {
            EraSignal::Init { .. } => "init",
            EraSignal::Resp { .. } => "resp",
            EraSignal::Nudge { .. } => "nudge",
        }
    }

    pub fn to_content(&self) -> String {
        match self {
            EraSignal::Init { era_next, nonce, prior_tag, kem_set } => format!("{}init\u{2}{}\u{2}{}\u{2}{:08x}\u{2}{}", ERA_PREFIX, era_next, hex::encode(nonce), prior_tag, kem_set),
            EraSignal::Resp { era_next, nonce, prior_tag } => format!("{}resp\u{2}{}\u{2}{}\u{2}{:08x}", ERA_PREFIX, era_next, hex::encode(nonce), prior_tag),
            EraSignal::Nudge { prior_tag } => format!("{}nudge\u{2}{:08x}", ERA_PREFIX, prior_tag),
        }
    }

    /// None for non-era content or a malformed record (malformed = dropped, never guessed).
    pub fn parse(content: &str) -> Option<EraSignal> {
        let rest = content.strip_prefix(ERA_PREFIX)?;
        let mut parts = rest.split('\u{2}');
        let kind = parts.next()?;
        let tag = |s: &str| u32::from_str_radix(s, 16).ok();
        match kind {
            "init" => {
                let era_next: u64 = parts.next()?.parse().ok()?;
                let nonce: [u8; 32] = hex::decode(parts.next()?).ok()?.try_into().ok()?;
                let prior_tag = tag(parts.next()?)?;
                let kem_set: u8 = parts.next()?.parse().ok()?;
                Some(EraSignal::Init { era_next, nonce, prior_tag, kem_set })
            }
            "resp" => {
                let era_next: u64 = parts.next()?.parse().ok()?;
                let nonce: [u8; 32] = hex::decode(parts.next()?).ok()?.try_into().ok()?;
                let prior_tag = tag(parts.next()?)?;
                Some(EraSignal::Resp { era_next, nonce, prior_tag })
            }
            "nudge" => Some(EraSignal::Nudge { prior_tag: tag(parts.next()?)? }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One full exchange over the default set: both sides land on the same fresh secret and the same era keys; the old root is load-bearing; every public input moves the result.
    #[test]
    fn hybrid_ratchet_agrees_and_binds_everything() {
        let fid = [7u8; 32];
        let token = [9u8; 32];
        let old_root = [1u8; 32];
        let old_hk = [2u8; 32];
        let eph = era_keygen(1, 0xABCD_1234, KEM_SET_DEFAULT);
        assert!(!eph.init_wire.mlkem.is_empty() && eph.init_wire.x25519.len() == 32 && !eph.init_wire.hqc.is_empty());
        let (resp, f_resp) = era_encapsulate(&eph.init_wire, KEM_SET_DEFAULT).expect("encapsulate");
        let f_init = era_decapsulate(&eph, &resp).expect("decapsulate");
        assert_eq!(f_init, f_resp, "both sides hold one fresh secret");
        let t = derive_era_transcript(&token, 1, &eph.nonce, KEM_SET_DEFAULT, &eph.init_wire, &resp);
        let a = derive_era_keys(&fid, 1, &old_root, Some(&old_hk), &f_init, &t);
        let b = derive_era_keys(&fid, 1, &old_root, Some(&old_hk), &f_resp, &t);
        assert_eq!(a, b);
        assert_ne!(a.0, a.1, "root and history key are distinct");
        // Old-root necessity: a snapshot holding everything but the old root derives nothing useful.
        let c = derive_era_keys(&fid, 1, &[3u8; 32], Some(&old_hk), &f_init, &t);
        assert_ne!(a, c);
        // Transcript sensitivity: a different nonce on the wire is a different era.
        let t2 = derive_era_transcript(&token, 1, &[0u8; 32], KEM_SET_DEFAULT, &eph.init_wire, &resp);
        assert_ne!(t, t2);
        assert_ne!(a, derive_era_keys(&fid, 1, &old_root, Some(&old_hk), &f_init, &t2));
        // Index sensitivity and history-key presence are both bound.
        assert_ne!(a, derive_era_keys(&fid, 2, &old_root, Some(&old_hk), &f_init, &t));
        assert_ne!(a, derive_era_keys(&fid, 1, &old_root, None, &f_init, &t));
    }

    /// A tampered ciphertext never agrees — and the HQC-256 ciphertext size is pinned so a library bump that changes it fails loudly here rather than on a phone.
    #[test]
    fn tamper_rejects_and_sizes_are_pinned() {
        let eph = era_keygen(3, 1, KEM_SET_DEFAULT);
        let (mut resp, f) = era_encapsulate(&eph.init_wire, KEM_SET_DEFAULT).unwrap();
        assert_eq!(resp.hqc.len(), 14421, "HQC-256 ciphertext");
        assert_eq!(resp.mlkem.len(), 1568, "ML-KEM-1024 ciphertext");
        assert_eq!(eph.init_wire.mlkem.len(), 1568, "ML-KEM-1024 public key");
        assert_eq!(eph.init_wire.hqc.len(), 7245, "HQC-256 public key");
        resp.mlkem[10] ^= 0x55;
        let f2 = era_decapsulate(&eph, &resp);
        assert!(f2.map_or(true, |x| x != f), "a flipped ML-KEM byte must not reproduce the fresh secret");
        let mut resp2 = resp.clone();
        resp2.mlkem[10] ^= 0x55;
        resp2.x25519 = vec![0u8; 31];
        assert!(era_decapsulate(&eph, &resp2).is_none(), "a malformed X25519 field is a parse failure, not a guess");
    }

    /// The set is a bitmask: ML-KEM + X25519 without HQC still agrees, and an empty set is refused.
    #[test]
    fn kem_set_is_a_bitmask() {
        let set = KEM_MLKEM1024 | KEM_X25519;
        let eph = era_keygen(1, 0, set);
        assert!(eph.init_wire.hqc.is_empty());
        let (resp, f) = era_encapsulate(&eph.init_wire, set).unwrap();
        assert!(resp.hqc.is_empty());
        assert_eq!(era_decapsulate(&eph, &resp), Some(f));
        assert!(era_encapsulate(&eph.init_wire, 0).is_none());
    }

    /// Labeled-egg framing is injective: moving bytes between eggs changes the fold.
    #[test]
    fn fresh_fold_is_injective_in_framing() {
        let a = derive_era_fresh(&[("x", vec![1, 2, 3]), ("y", vec![4])]);
        let b = derive_era_fresh(&[("x", vec![1, 2]), ("y", vec![3, 4])]);
        let c = derive_era_fresh(&[("xy", vec![1, 2, 3]), ("", vec![4])]);
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(a, derive_era_fresh(&[("x", vec![1, 2, 3]), ("y", vec![4])]));
    }

    #[test]
    fn signals_round_trip_and_are_control() {
        let sigs = [
            EraSignal::Init { era_next: 5, nonce: [0xAB; 32], prior_tag: 0x0000_00FF, kem_set: 7 },
            EraSignal::Resp { era_next: 5, nonce: [0xCD; 32], prior_tag: 0xDEAD_BEEF },
            EraSignal::Nudge { prior_tag: 1 },
        ];
        for s in sigs {
            let c = s.to_content();
            assert!(crate::types::is_control_content(&c), "era rows are hidden control rows");
            assert_eq!(EraSignal::parse(&c), Some(s));
        }
        assert_eq!(EraSignal::parse("hello"), None);
        assert_eq!(EraSignal::parse(&format!("{}init\u{2}x", ERA_PREFIX)), None, "malformed is dropped, never guessed");
    }
}
