//! A row's KIND is a field, never a content prefix (flag day 2026-09-24).
//!
//! Until this day five kinds of row smuggled their structure through `content`: a `\x01\x02photon-<tag>\x02\x01` marker, a kind word, then `\x02`-split hex or decimal fields.
//! Renaming one marker (`photon-call` → `photon-wave`, the v103 flag day) silently reclassified every stored wave signal as a visible chat bubble and let those rows ring the notifier, because what a row WAS lived inside a renameable string.
//! Now a control row carries [`RowControl`] and an attachment row carries [`AttachRef`], each written by every carrier (live package, history page, durable record) as named typed fields, and `content` holds only human text.
//!
//! [`ControlSlots`] is the one flat, typed shape every carrier writes: each carrier names the slots its own way (`ck`/`cs`… on the wire, `m_ck`… on a page, `ctl_kind`… in the vault), and the enum↔slot mapping lives here once, so no two carriers can disagree about what a slot means.

use crate::crypto::era::EraSignal;
use crate::types::molecule::MoleculeSignal;
use crate::wave::signal::WaveSignal;

/// What a hidden machinery row is. Every variant is invisible to every surface, never notifies, and never enters a weave window or a history page's visible set.
#[derive(Clone, Debug, PartialEq)]
pub enum RowControl {
    /// The one hidden chain-weave probe after a CLUTCH completes: validates the ratchet end to end, suppresses its bubble.
    Probe,
    /// Tombstone instruction: the peer deletes-for-everyone the row stamped `target`.
    Delete { target: i64 },
    /// Wave signaling (offer/answer/decline/busy/hangup/taken; anchor is express-only).
    Wave(WaveSignal),
    /// Era-ratchet rows (Init/Resp/Nudge); the KEM material rides the package's `ekn`/`ekx`/`ekh` beside these.
    Era(EraSignal),
    /// Group control rows; every payload already rides the package's typed `g*` fields beside this.
    Molecule(MoleculeSignal),
}

/// The flat typed shape a carrier writes. `kind` 0 never appears on a control row; absent optionals are simply not written.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ControlSlots {
    pub kind: u8,
    pub sub: u8,
    /// Delete: the target row's eagle time.
    pub ts: Option<i64>,
    /// Wave: the 16-byte wave id.
    pub id: Option<Vec<u8>>,
    /// Wave offer/answer, era init/resp: the 32-byte nonce.
    pub nonce: Option<[u8; 32]>,
    /// Wave offer/answer: the originating or answering device's pubkey.
    pub dev: Option<[u8; 32]>,
    /// Era init/resp: `era_next`.
    pub num: Option<u64>,
    /// Era: `prior_tag`.
    pub tag: Option<u32>,
    /// Era init: `kem_set`.
    pub set: Option<u8>,
}

pub const CTL_PROBE: u8 = 1;
pub const CTL_DELETE: u8 = 2;
pub const CTL_WAVE: u8 = 3;
pub const CTL_ERA: u8 = 4;
pub const CTL_MOLECULE: u8 = 5;

impl RowControl {
    /// The flat slots every carrier writes.
    pub fn slots(&self) -> ControlSlots {
        match self {
            RowControl::Probe => ControlSlots { kind: CTL_PROBE, ..Default::default() },
            RowControl::Delete { target } => ControlSlots { kind: CTL_DELETE, ts: Some(*target), ..Default::default() },
            RowControl::Wave(w) => {
                let (sub, nonce, dev) = match w {
                    WaveSignal::Offer { nonce, device, .. } => (1, Some(*nonce), *device),
                    WaveSignal::Answer { nonce, device, .. } => (2, Some(*nonce), *device),
                    WaveSignal::Decline { .. } => (3, None, None),
                    WaveSignal::Busy { .. } => (4, None, None),
                    WaveSignal::Hangup { .. } => (5, None, None),
                    WaveSignal::Taken { .. } => (6, None, None),
                    WaveSignal::Anchor { .. } => (7, None, None),
                };
                ControlSlots { kind: CTL_WAVE, sub, id: Some(w.wave_id().to_vec()), nonce, dev, ..Default::default() }
            }
            RowControl::Era(e) => match e {
                EraSignal::Init { era_next, nonce, prior_tag, kem_set } => ControlSlots {
                    kind: CTL_ERA,
                    sub: 1,
                    num: Some(*era_next),
                    nonce: Some(*nonce),
                    tag: Some(*prior_tag),
                    set: Some(*kem_set),
                    ..Default::default()
                },
                EraSignal::Resp { era_next, nonce, prior_tag } => ControlSlots {
                    kind: CTL_ERA,
                    sub: 2,
                    num: Some(*era_next),
                    nonce: Some(*nonce),
                    tag: Some(*prior_tag),
                    ..Default::default()
                },
                EraSignal::Nudge { prior_tag } => ControlSlots { kind: CTL_ERA, sub: 3, tag: Some(*prior_tag), ..Default::default() },
            },
            RowControl::Molecule(m) => {
                let sub = match m {
                    MoleculeSignal::Offer => 1,
                    MoleculeSignal::Join => 2,
                    MoleculeSignal::Wrap => 3,
                    MoleculeSignal::Records => 4,
                };
                ControlSlots { kind: CTL_MOLECULE, sub, ..Default::default() }
            }
        }
    }

    /// Rebuild from a carrier's slots. None for an unknown kind or a missing required slot — malformed is dropped, never guessed.
    pub fn from_slots(s: &ControlSlots) -> Option<RowControl> {
        match s.kind {
            CTL_PROBE => Some(RowControl::Probe),
            CTL_DELETE => Some(RowControl::Delete { target: s.ts? }),
            CTL_WAVE => {
                let wave_id: [u8; 16] = s.id.as_deref()?.try_into().ok()?;
                let w = match s.sub {
                    1 => WaveSignal::Offer { wave_id, nonce: s.nonce?, device: s.dev },
                    2 => WaveSignal::Answer { wave_id, nonce: s.nonce?, device: s.dev },
                    3 => WaveSignal::Decline { wave_id },
                    4 => WaveSignal::Busy { wave_id },
                    5 => WaveSignal::Hangup { wave_id },
                    6 => WaveSignal::Taken { wave_id },
                    7 => WaveSignal::Anchor { wave_id },
                    _ => return None,
                };
                Some(RowControl::Wave(w))
            }
            CTL_ERA => {
                let e = match s.sub {
                    1 => EraSignal::Init { era_next: s.num?, nonce: s.nonce?, prior_tag: s.tag?, kem_set: s.set? },
                    2 => EraSignal::Resp { era_next: s.num?, nonce: s.nonce?, prior_tag: s.tag? },
                    3 => EraSignal::Nudge { prior_tag: s.tag? },
                    _ => return None,
                };
                Some(RowControl::Era(e))
            }
            CTL_MOLECULE => {
                let m = match s.sub {
                    1 => MoleculeSignal::Offer,
                    2 => MoleculeSignal::Join,
                    3 => MoleculeSignal::Wrap,
                    4 => MoleculeSignal::Records,
                    _ => return None,
                };
                Some(RowControl::Molecule(m))
            }
            _ => None,
        }
    }

    /// Canonical identity bytes (row key, dedup, digest, braid ingredient) — see [`ident_typed`].
    pub fn ident_bytes(&self) -> Vec<u8> {
        let s = self.slots();
        let mut out = vec![IDENT_TYPED, IDENT_CONTROL, s.kind, s.sub];
        push_opt(&mut out, s.ts.map(|t| t.to_le_bytes().to_vec()));
        push_opt(&mut out, s.id);
        push_opt(&mut out, s.nonce.map(|n| n.to_vec()));
        push_opt(&mut out, s.dev.map(|d| d.to_vec()));
        push_opt(&mut out, s.num.map(|n| n.to_le_bytes().to_vec()));
        push_opt(&mut out, s.tag.map(|t| t.to_le_bytes().to_vec()));
        push_opt(&mut out, s.set.map(|v| vec![v]));
        out
    }

    pub fn wave(&self) -> Option<&WaveSignal> {
        match self {
            RowControl::Wave(w) => Some(w),
            _ => None,
        }
    }

    pub fn era(&self) -> Option<&EraSignal> {
        match self {
            RowControl::Era(e) => Some(e),
            _ => None,
        }
    }

    pub fn molecule(&self) -> Option<MoleculeSignal> {
        match self {
            RowControl::Molecule(m) => Some(*m),
            _ => None,
        }
    }
}

/// What an attachment row's blob IS — the typed replacement for reading the filename as a tag (`"wave.audio"`, `"wave.env"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachRole {
    /// An ordinary file or image the human sent.
    File,
    /// A kept wave recording (PHWAVE9 container): plays, never saves.
    WaveAudio,
    /// A wave's envelope pyramid: drawn on the card, never shown as a pill.
    WaveEnv,
}

impl AttachRole {
    pub fn code(self) -> u8 {
        match self {
            AttachRole::File => 1,
            AttachRole::WaveAudio => 2,
            AttachRole::WaveEnv => 3,
        }
    }

    pub fn from_code(c: u8) -> Option<AttachRole> {
        match c {
            1 => Some(AttachRole::File),
            2 => Some(AttachRole::WaveAudio),
            3 => Some(AttachRole::WaveEnv),
            _ => None,
        }
    }
}

/// An attachment row's identity: which bytes (the content hash the blob transport serves under), what the sender called them, how many, and what they are for. The blob itself never rides a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachRef {
    pub hash: [u8; 32],
    /// The sender's filename; empty for images and audio, which travel nameless.
    pub name: String,
    pub size: u64,
    pub role: AttachRole,
}

impl AttachRef {
    pub fn file(hash: [u8; 32], name: &str, size: u64) -> Self {
        AttachRef { hash, name: name.to_string(), size, role: AttachRole::File }
    }

    /// Canonical identity bytes — see [`ident_typed`].
    pub fn ident_bytes(&self) -> Vec<u8> {
        let mut out = vec![IDENT_TYPED, IDENT_ATTACH, self.role.code()];
        out.extend_from_slice(&self.hash);
        out.extend_from_slice(&self.size.to_le_bytes());
        out.extend_from_slice(self.name.as_bytes());
        out
    }
}

/// A PRE-FLAG-DAY row: its kind smuggled through a `\x01\x02photon-` content prefix. Nick (2026-09-24): no backwards compatibility — such a row is never parsed, never shown and never stored; this predicate exists only so every ingress (live frame, page merge, vault load) can refuse it the same way.
pub fn is_legacy_prefixed(content: &str) -> bool {
    content.starts_with("\u{1}\u{2}photon-")
}

/// Leading byte of every typed row's identity. 0xFF can never begin valid UTF-8, so a typed row's identity can never equal any text row's, whatever the human types.
pub const IDENT_TYPED: u8 = 0xFF;
const IDENT_CONTROL: u8 = 1;
const IDENT_ATTACH: u8 = 2;

fn push_opt(out: &mut Vec<u8>, v: Option<Vec<u8>>) {
    match v {
        Some(b) => {
            out.push(1);
            out.extend_from_slice(&(b.len() as u32).to_le_bytes());
            out.extend_from_slice(&b);
        }
        None => out.push(0),
    }
}

/// The ONE identity function for a row's payload, used by the row key, dedup, sort tiebreak, anti-entropy digest, checkpoint leaf and braid ingredient alike — so sender and receiver, vault and page, can never disagree.
/// A text row is its content bytes, exactly as before this flag day, so text rows keep their keys and the braid is untouched for them; a typed row is its canonical field bytes behind [`IDENT_TYPED`].
pub fn ident_typed(content: &str, control: Option<&RowControl>, file: Option<&AttachRef>) -> Vec<u8> {
    if let Some(c) = control {
        return c.ident_bytes();
    }
    if let Some(f) = file {
        return f.ident_bytes();
    }
    content.as_bytes().to_vec()
}

// ---------------------------------------------------------------------------
// SECTION FIELDS — the one writer/reader pair for every schema'd VSF section that carries a row (the live message package and the express wave frame).
// Each value takes its honest VSF type: random ids and nonces are `hR`, a device is an Ed25519 key `ke`, a content hash `hb`, a stamp `e6`, counts and codes unsigned, a name text.
// ---------------------------------------------------------------------------

use vsf::schema::{SectionBuilder, SectionSchema, TypeConstraint};
use vsf::VsfType;

/// Declare the control and file fields on a schema.
pub fn declare_row_fields(s: SectionSchema) -> SectionSchema {
    s.field("ck", TypeConstraint::AnyUnsigned) // control kind (CTL_*); absent = not a control row
        .field("cs", TypeConstraint::AnyUnsigned) // control sub-kind within its family
        .field("cts", TypeConstraint::Any) // e6 delete target
        .field("cid", TypeConstraint::Any) // hR wave id (16 bytes)
        .field("cn", TypeConstraint::Any) // hR nonce (32 bytes)
        .field("cd", TypeConstraint::AnyKey) // ke originating / answering device
        .field("cnum", TypeConstraint::AnyUnsigned) // era_next
        .field("ctag", TypeConstraint::AnyUnsigned) // era prior_tag
        .field("cset", TypeConstraint::AnyUnsigned) // era kem_set
        .field("fr", TypeConstraint::AnyUnsigned) // attachment role (AttachRole code); absent = not an attachment row
        .field("fh", TypeConstraint::AnyHash) // hb attachment content hash
        .field("fnm", TypeConstraint::Utf8Text) // attachment filename (may be empty)
        .field("fz", TypeConstraint::AnyUnsigned) // attachment size in bytes
}

/// Write a control row's fields.
pub fn put_control(b: SectionBuilder, c: &RowControl) -> Result<SectionBuilder, String> {
    let s = c.slots();
    let e = |x: vsf::schema::ValidationError| x.to_string();
    let mut b = b.set("ck", VsfType::u(s.kind as usize, false)).map_err(e)?.set("cs", VsfType::u(s.sub as usize, false)).map_err(e)?;
    if let Some(t) = s.ts {
        b = b.set("cts", VsfType::e(vsf::types::EtType::e6(t))).map_err(e)?;
    }
    if let Some(id) = s.id {
        b = b.set("cid", VsfType::hR(id)).map_err(e)?;
    }
    if let Some(n) = s.nonce {
        b = b.set("cn", VsfType::hR(n.to_vec())).map_err(e)?;
    }
    if let Some(d) = s.dev {
        b = b.set("cd", VsfType::ke(d.to_vec())).map_err(e)?;
    }
    if let Some(n) = s.num {
        b = b.set("cnum", VsfType::u(n as usize, false)).map_err(e)?;
    }
    if let Some(t) = s.tag {
        b = b.set("ctag", VsfType::u(t as usize, false)).map_err(e)?;
    }
    if let Some(v) = s.set {
        b = b.set("cset", VsfType::u(v as usize, false)).map_err(e)?;
    }
    Ok(b)
}

/// Write an attachment row's identity fields.
pub fn put_file(b: SectionBuilder, f: &AttachRef) -> Result<SectionBuilder, String> {
    let e = |x: vsf::schema::ValidationError| x.to_string();
    b.set("fr", VsfType::u(f.role.code() as usize, false))
        .map_err(e)?
        .set("fh", VsfType::hb(f.hash.to_vec()))
        .map_err(e)?
        .set("fnm", VsfType::x(f.name.clone()))
        .map_err(e)?
        .set("fz", VsfType::u(f.size as usize, false))
        .map_err(e)
}

fn first<'a>(section: &'a SectionBuilder, name: &str) -> Option<&'a VsfType> {
    section.get_fields(name).first().and_then(|f| f.values.first())
}

fn bytes_of(v: &VsfType) -> Option<&[u8]> {
    match v {
        VsfType::hR(b) | VsfType::hb(b) | VsfType::ke(b) => Some(b.as_slice()),
        _ => None,
    }
}

/// Read a control row's fields. None when `ck` is absent (not a control row) or the set is malformed.
pub fn get_control(section: &SectionBuilder) -> Option<RowControl> {
    let kind = u8::try_from(first(section, "ck")?.as_u64()?).ok()?;
    let arr32 = |name: &str| first(section, name).and_then(bytes_of).and_then(|b| <[u8; 32]>::try_from(b).ok());
    let slots = ControlSlots {
        kind,
        sub: first(section, "cs").and_then(|v| v.as_u64()).and_then(|v| u8::try_from(v).ok()).unwrap_or(0),
        ts: first(section, "cts").and_then(|v| match v {
            VsfType::e(vsf::types::EtType::e6(t)) => Some(*t),
            _ => None,
        }),
        id: first(section, "cid").and_then(bytes_of).map(|b| b.to_vec()),
        nonce: arr32("cn"),
        dev: arr32("cd"),
        num: first(section, "cnum").and_then(|v| v.as_u64()),
        tag: first(section, "ctag").and_then(|v| v.as_u64()).and_then(|v| u32::try_from(v).ok()),
        set: first(section, "cset").and_then(|v| v.as_u64()).and_then(|v| u8::try_from(v).ok()),
    };
    RowControl::from_slots(&slots)
}

/// Read an attachment row's identity. None when `fr` is absent or any of the four is missing — a partial set is malformed, never a guess.
pub fn get_file(section: &SectionBuilder) -> Option<AttachRef> {
    let role = AttachRole::from_code(u8::try_from(first(section, "fr")?.as_u64()?).ok()?)?;
    let hash = <[u8; 32]>::try_from(first(section, "fh").and_then(bytes_of)?).ok()?;
    let name = match first(section, "fnm")? {
        VsfType::x(s) => s.clone(),
        _ => return None,
    };
    let size = first(section, "fz")?.as_u64()?;
    Some(AttachRef { hash, name, size, role })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_control() -> Vec<RowControl> {
        let id = [0xAB; 16];
        let n = [0xCD; 32];
        vec![
            RowControl::Probe,
            RowControl::Delete { target: -5 },
            RowControl::Wave(WaveSignal::Offer { wave_id: id, nonce: n, device: Some([0xEE; 32]) }),
            RowControl::Wave(WaveSignal::Offer { wave_id: id, nonce: n, device: None }),
            RowControl::Wave(WaveSignal::Answer { wave_id: id, nonce: n, device: Some([0xEF; 32]) }),
            RowControl::Wave(WaveSignal::Decline { wave_id: id }),
            RowControl::Wave(WaveSignal::Busy { wave_id: id }),
            RowControl::Wave(WaveSignal::Hangup { wave_id: id }),
            RowControl::Wave(WaveSignal::Taken { wave_id: id }),
            RowControl::Wave(WaveSignal::Anchor { wave_id: id }),
            RowControl::Era(EraSignal::Init { era_next: 7, nonce: n, prior_tag: 0xDEAD_BEEF, kem_set: 3 }),
            RowControl::Era(EraSignal::Resp { era_next: 7, nonce: n, prior_tag: 1 }),
            RowControl::Era(EraSignal::Nudge { prior_tag: 2 }),
            RowControl::Molecule(MoleculeSignal::Offer),
            RowControl::Molecule(MoleculeSignal::Join),
            RowControl::Molecule(MoleculeSignal::Wrap),
            RowControl::Molecule(MoleculeSignal::Records),
        ]
    }

    /// Every variant survives the slot mapping every carrier uses, and every variant has a distinct identity.
    #[test]
    fn every_control_round_trips_and_idents_are_distinct() {
        let all = every_control();
        for c in &all {
            assert_eq!(RowControl::from_slots(&c.slots()).as_ref(), Some(c), "{c:?}");
        }
        let mut idents: Vec<Vec<u8>> = all.iter().map(|c| c.ident_bytes()).collect();
        idents.sort();
        idents.dedup();
        assert_eq!(idents.len(), all.len());
    }

    /// A text row's identity is its bytes (unchanged by the flag day); a typed row's can never collide with any text, whatever the human types.
    #[test]
    fn typed_identity_never_collides_with_text() {
        assert_eq!(ident_typed("hi", None, None), b"hi".to_vec());
        let probe = ident_typed("", Some(&RowControl::Probe), None);
        assert_eq!(probe[0], IDENT_TYPED);
        assert!(std::str::from_utf8(&probe).is_err());
        let f = AttachRef::file([1; 32], "a.txt", 9);
        assert_ne!(f.ident_bytes(), AttachRef { role: AttachRole::WaveAudio, ..f.clone() }.ident_bytes());
    }

    /// A malformed slot set is dropped, never guessed into a signal.
    #[test]
    fn malformed_slots_are_refused() {
        assert_eq!(RowControl::from_slots(&ControlSlots { kind: CTL_WAVE, sub: 1, id: Some(vec![0; 16]), ..Default::default() }), None);
        assert_eq!(RowControl::from_slots(&ControlSlots { kind: CTL_WAVE, sub: 5, id: Some(vec![0; 15]), ..Default::default() }), None);
        assert_eq!(RowControl::from_slots(&ControlSlots { kind: 99, ..Default::default() }), None);
        assert_eq!(RowControl::from_slots(&ControlSlots { kind: CTL_DELETE, ..Default::default() }), None);
    }
}
