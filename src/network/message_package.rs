//! The chat message package — a PROPERLY FRAMED VSF document, not a bare field with appendations.
//!
//! Every other transport in the system already obeys AGENT.md's "COMPLETE VSF FILES ONLY" rule (history pages, chain_sync, fstate); the chat plaintext was the last bare-field holdout, and each new capability was being smuggled in as a string marker or a trailing appended field. This module ends that: the plaintext inside the braid seal is a schema-validated section in a complete VSF document (header, TOC, provenance), and describing something new about a message is ADDING A FIELD — named, typed, order-independent, verifiable — never an encoding trick.
//!
//! The braid neither knows nor cares: it encrypts these bytes and hashes them for msg_hp/ACK exactly as before, and the chain ingredient stays the bare body text on both sides. Atomic swap, no compat branch (the codebase's own doctrine): a version-skewed peer garbage-parses, which is the fork detector's territory, and the standing repair ladder converges it — the same story as every flag-day.

use vsf::schema::{SectionBuilder, SectionSchema, TypeConstraint};
use vsf::VsfType;

/// The section name, shared by the builder and the parse lookup.
const MSG_SECTION: &str = "msg";

/// A decoded (pre-seal / post-open) message package.
#[derive(Clone, Debug, PartialEq)]
pub struct MessagePackage {
    /// The bare body text — the ONLY chain ingredient (salt source + braid `our_plaintext`), and the only part a bubble renders. Empty is legal (a reaction retract).
    pub body: String,
    /// The sender's last incorporated hash pointer (bidirectional-entropy bookkeeping; zeroes when none).
    pub incorporated_hp: [u8; 32],
    /// Eagle times naming the woven peer-message strands this step folds — 0, 1, or 2 (the braid's explicit references).
    pub woven_times: Vec<i64>,
    /// Typed reference: (RefKind wire value, target eagle_time) — reply/edit/react metadata as a FIELD. None = a plain message.
    pub reference: Option<(u8, i64)>,
    /// Typed bridge extras (locus / stream state / interrupt signal) — fields, never content markers. None = not a bridge frame.
    pub bridge: Option<BridgeWire>,
    /// Typed content marks: (kind, byte_start, byte_len, dest) per entry — the link elements layered beside the body (Nick 2026-09-04). Parallel multi-value fields on the wire, zipped at parse; a count mismatch drops ALL marks (fail-safe to plain text). Old parsers discard the unknown names — no flag day.
    pub marks: Vec<(u8, usize, usize, String)>,
    /// Era-ratchet KEM material (crypto/era.rs): present only on an Init/Resp control row. Typed fields beside the text, never inside it.
    pub era_kem: Option<crate::crypto::era::EraKemWire>,
    /// Typed attachment fields (2026-09-10): the sender's sniffed kind, dims, preview-blob hash and the row's micro preview. None on every non-attachment row.
    pub attach: Option<AttachWire>,
    /// Typed group extras (docs/molecules.md §3/§4): attribution, weave authors, and the record payload. None on every friendship row.
    pub molecule: Option<MoleculeWire>,
    /// The row's KIND when it is hidden machinery (probe, delete, wave, era, group control) — typed fields, never a body prefix. The body is empty on these.
    pub control: Option<crate::types::RowControl>,
    /// The attachment row's identity (hash, name, size, role) — typed fields; the body is empty on these.
    pub file: Option<crate::types::AttachRef>,
}

/// The group frame's typed extras (docs/molecules.md §3 Frames): `from` is the sender's party id — implicit in a friendship, attribution in a group, verified against the roster's folded devices at ingress; `woven_authors` pairs with the package's `woven_times` to make each weave reference `(author, eagle_time)` (eagle times are unique per device, not per group); `blob` is the roster-codec record payload on a MOLECULE_PREFIX control row (offer snapshot, join records, record posting); `wrap` is the sealed era secret on a WRAP row — typed fields, consumed at ingress, never part of the row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MoleculeWire {
    pub from: [u8; 32],
    pub woven_authors: Vec<[u8; 32]>,
    pub blob: Option<Vec<u8>>,
    pub wrap: Option<BondWrapWire>,
}

/// A group WRAP's typed fields (docs/molecules.md §3 Eras, D10 "secrets only ever move as wraps"): one era secret sealed to ONE device's published KEM bundle. The KEM ciphertexts ride the existing `ekn`/`ekx`/`ekh` fields beside these; `sealed` is the secret under the KEM-derived key. Read once by the wrap handler and dropped; the row that persists is the bare kind marker. Zeroized on drop. Never in the text — the text is the row, and the row persists, replicates and re-serves.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BondWrapWire {
    /// The device this wrap opens for — every other device ignores the row.
    pub recipient_device: [u8; 32],
    /// The published bundle it was sealed to (`EraKemWire::bundle_id`) — matched before any decapsulation runs.
    pub bundle_id: [u8; 32],
    /// The era the sealed secret installs.
    pub era: u64,
    pub era_lineage: [u8; 32],
    /// The minter's nonce — folded into the transcript so two mints of the same index never derive alike.
    pub nonce: [u8; 32],
    /// The sealed payload: `fresh` (a mint: the recipient derives N+1 from its own old era) or `root ‖ history_key` (a join answer: the recipient installs the era whole).
    pub sealed: Vec<u8>,
}

impl Drop for BondWrapWire {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.sealed.zeroize();
    }
}

/// The attachment row's typed extras on the friend wire — kind (AttachKind wire value), pixel dims (0 = unknown), the preview-blob hash, and the micro preview bytes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AttachWire {
    pub kind: u8,
    pub w: u32,
    pub h: u32,
    pub preview_hash: Option<[u8; 32]>,
    pub preview: Vec<u8>,
}

/// The bridge's typed wire extras, riding the inner package as named optional fields: the locus that ends blind-cwd operation (field 2026-08-23), the snapshot sequence + final exit that make streamed output loss-proof (every partial is the FULL accumulated text; newest seq wins), and the interrupt signal. An old peer's parser discards the names it doesn't know; a new peer reading an old frame sees all-None — no flag day.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BridgeWire {
    /// Host machine name (once per shell, repeated on every output frame — the frame is self-sufficient).
    pub host: Option<String>,
    /// The shell's working directory — post-command truth from the host, last-known on partials.
    pub cwd: Option<String>,
    /// Snapshot sequence per command, monotonically increasing; the client keeps the newest.
    pub seq: Option<u64>,
    /// Exit code — present exactly on the FINAL frame of a command.
    pub exit: Option<i64>,
    /// Interrupt signal number (SIGINT/SIGTERM/SIGKILL) on a BridgeCtl row.
    pub sig: Option<u64>,
    /// APPEND semantics (Nick 2026-09-03): this frame carries only what's NEW since the previous frame — the client appends to the command's row instead of replacing it, the chain's hash links already guarantee order, and "finished" is simply the frame where `exit` is present (no special end message). Absent = legacy whole-snapshot replace.
    pub delta: bool,
    /// A PIGEON DROPPED INTO THE BRIDGE (Nick 2026-09-17): the operator dropped a file on an open session and it lands in whatever directory the host's shell is standing in.
    /// Name and content hash only — deliberately NO PATH. The host is the only side that knows where its shell actually stands (the client's `cwd` is a snapshot off the last output frame and goes stale the moment a command cds), and a wire that cannot name a destination cannot be used to write outside the directory the operator is already looking at.
    /// The bytes themselves ride the ordinary chunked blob transport under the same relationship key; this is the announcement, not the payload.
    pub pigeon: Option<BridgePigeon>,
}

/// A file dropped into an open bridge session: what it is called and which bytes it is. Ephemeral by construction — the host writes it to disk and sheds the blob, the client sheds its copy once the landing confirms, and the row carrying this is a bridge row, wiped at the next open.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BridgePigeon {
    pub name: String,
    pub hash: [u8; 32],
    pub size: u64,
}

impl BridgeWire {
    pub fn is_empty(&self) -> bool {
        self.host.is_none()
            && self.cwd.is_none()
            && self.seq.is_none()
            && self.exit.is_none()
            && self.sig.is_none()
            && !self.delta
            && self.pigeon.is_none()
    }
}

/// Schema for the package section. `pad` is random bytes for traffic-analysis size jitter — schema'd sections serialize fields in schema order, so the old shuffled-field trick is gone; parsing is by NAME, which is the stronger version of what the shuffle enforced.
fn msg_schema() -> SectionSchema {
    crate::types::row_control::declare_row_fields(SectionSchema::new(MSG_SECTION))
        .field("body", TypeConstraint::Utf8Text)
        .field("ihp", TypeConstraint::Any) // hp, 32 bytes
        .field("wt", TypeConstraint::Any) // e6, zero to two entries
        .field("refk", TypeConstraint::AnyUnsigned) // reference kind: 0 = none
        .field("reft", TypeConstraint::Any) // e6 reference target; 0 when kind is none
        .field("bhost", TypeConstraint::Utf8Text) // bridge locus: host machine name
        .field("bcwd", TypeConstraint::Utf8Text) // bridge locus: shell working directory
        .field("bseq", TypeConstraint::AnyUnsigned) // bridge snapshot sequence
        .field("bexit", TypeConstraint::Any) // i6 bridge exit code; present = final frame
        .field("bsig", TypeConstraint::AnyUnsigned) // bridge interrupt signal number
        .field("bdelta", TypeConstraint::AnyUnsigned) // u0 append-semantics flag; absent = legacy whole-snapshot replace (old parsers discard unknown fields, so this is forward-compatible)
        .field("bpn", TypeConstraint::Utf8Text) // bridge pigeon: the dropped file's name (no path — the host chooses the directory)
        .field("bph", TypeConstraint::AnyHash) // bridge pigeon: content hash, the key the chunked transport serves under
        .field("bps", TypeConstraint::AnyUnsigned) // bridge pigeon: size in bytes, so the far side can show progress before a manifest lands
        .field("mk", TypeConstraint::AnyUnsigned) // mark kind per entry (1 = link)
        .field("ms", TypeConstraint::AnyUnsigned) // mark byte start per entry
        .field("ml", TypeConstraint::AnyUnsigned) // mark byte len per entry
        .field("md", TypeConstraint::Utf8Text) // mark dest per entry (link destination URL)
        .field("pad", TypeConstraint::Any) // hR random jitter; length is the only meaning
        .field("ekn", TypeConstraint::Any) // hR era-ratchet ML-KEM-1024 material (public key on Init, ciphertext on Resp)
        .field("ekx", TypeConstraint::Any) // hR era-ratchet X25519 ephemeral public key
        .field("ekh", TypeConstraint::Any) // hR era-ratchet HQC-256 material (public key on Init, ciphertext on Resp)
        // Typed attachment extras (2026-09-10).
        .field("ak", TypeConstraint::AnyUnsigned)
        .field("aw", TypeConstraint::AnyUnsigned)
        .field("ah", TypeConstraint::AnyUnsigned)
        .field("aph", TypeConstraint::Any)
        .field("apv", TypeConstraint::Any)
        .field("gpl", TypeConstraint::Any) // hR group record payload (roster-codec blob) on a MOLECULE_PREFIX row; old parsers discard the unknown name
        .field("gfrom", TypeConstraint::AnyHash) // hb 32 group attribution: the sender's party id (docs/molecules.md §3); presence marks a group frame
        .field("gwa", TypeConstraint::AnyHash) // hb 32 weave author per wt entry (group weave refs are (author, eagle_time)); count must match wt
        .field("grcpt", TypeConstraint::AnyHash) // hb 32 wrap: the recipient device
        .field("gbid", TypeConstraint::AnyHash) // hb 32 wrap: the bundle it was sealed to
        .field("gera", TypeConstraint::AnyUnsigned) // wrap: the era the sealed secret installs (auto-sized; readers widen)
        .field("glin", TypeConstraint::AnyHash) // hb 32 wrap: the era lineage (public — the one-way image of the era-0 root)
        .field("gnonce", TypeConstraint::AnyHash) // hb 32 wrap: the minter's nonce
        .field("gwrap", TypeConstraint::Any) // hR wrap: the sealed secret — consumed at ingress, never stored in the row
}

/// Encode a message package as a complete VSF document. The caller supplies the pad (already random) so this layer stays deterministic-in, deterministic-out.
pub fn build_message_package(
    body: &str,
    incorporated_hp: &[u8; 32],
    woven_times: &[i64],
    reference: Option<(u8, i64)>,
    bridge: Option<&BridgeWire>,
    marks: &[(u8, usize, usize, String)],
    pad: &[u8],
) -> Result<Vec<u8>, String> {
    build_message_package_era(body, incorporated_hp, woven_times, reference, bridge, marks, pad, None, None, None, None, None)
}

/// The full builder: an era-ratchet row also carries its KEM material as typed fields.
#[allow(clippy::too_many_arguments)]
pub fn build_message_package_era(
    body: &str,
    incorporated_hp: &[u8; 32],
    woven_times: &[i64],
    reference: Option<(u8, i64)>,
    bridge: Option<&BridgeWire>,
    marks: &[(u8, usize, usize, String)],
    pad: &[u8],
    era_kem: Option<&crate::crypto::era::EraKemWire>,
    attach: Option<&AttachWire>,
    molecule: Option<&MoleculeWire>,
    control: Option<&crate::types::RowControl>,
    file: Option<&crate::types::AttachRef>,
) -> Result<Vec<u8>, String> {
    let mut builder = msg_schema()
        .build()
        .set("body", VsfType::x(body.to_string()))
        .map_err(|e| e.to_string())?
        .set("ihp", VsfType::hp(incorporated_hp.to_vec()))
        .map_err(|e| e.to_string())?
        .set("refk", VsfType::u(reference.map(|(k, _)| k).unwrap_or(0) as usize, false))
        .map_err(|e| e.to_string())?
        .set(
            "reft",
            VsfType::e(vsf::types::EtType::e6(
                reference.map(|(_, t)| t).unwrap_or(0),
            )),
        )
        .map_err(|e| e.to_string())?;
    for &t in woven_times {
        builder = builder
            .append_multi("wt", vec![VsfType::e(vsf::types::EtType::e6(t))])
            .map_err(|e| e.to_string())?;
    }
    if let Some(b) = bridge {
        if let Some(h) = &b.host {
            builder = builder
                .set("bhost", VsfType::x(h.clone()))
                .map_err(|e| e.to_string())?;
        }
        if let Some(c) = &b.cwd {
            builder = builder
                .set("bcwd", VsfType::x(c.clone()))
                .map_err(|e| e.to_string())?;
        }
        if let Some(s) = b.seq {
            builder = builder
                .set("bseq", VsfType::u(s as usize, false))
                .map_err(|e| e.to_string())?;
        }
        if let Some(x) = b.exit {
            builder = builder
                .set("bexit", VsfType::i(x as isize))
                .map_err(|e| e.to_string())?;
        }
        if let Some(s) = b.sig {
            builder = builder
                .set("bsig", VsfType::u(s as usize, false))
                .map_err(|e| e.to_string())?;
        }
        if b.delta {
            builder = builder
                .set("bdelta", VsfType::u0(true))
                .map_err(|e| e.to_string())?;
        }
        if let Some(p) = b.pigeon.as_ref() {
            builder = builder
                .set("bpn", VsfType::x(p.name.clone()))
                .map_err(|e| e.to_string())?
                .set("bph", VsfType::hb(p.hash.to_vec()))
                .map_err(|e| e.to_string())?
                .set("bps", VsfType::u(p.size as usize, false))
                .map_err(|e| e.to_string())?;
        }
    }
    for (k, st, ln, dest) in marks {
        builder = builder
            .append_multi("mk", vec![VsfType::u(*k as usize, false)])
            .map_err(|e| e.to_string())?
            .append_multi("ms", vec![VsfType::u(*st, false)])
            .map_err(|e| e.to_string())?
            .append_multi("ml", vec![VsfType::u(*ln, false)])
            .map_err(|e| e.to_string())?
            .append_multi("md", vec![VsfType::x(dest.clone())])
            .map_err(|e| e.to_string())?;
    }
    if !pad.is_empty() {
        builder = builder
            .set("pad", VsfType::hR(pad.to_vec()))
            .map_err(|e| e.to_string())?;
    }
    if let Some(k) = era_kem {
        for (name, bytes) in [("ekn", &k.mlkem), ("ekx", &k.x25519), ("ekh", &k.hqc)] {
            if !bytes.is_empty() {
                builder = builder.set(name, VsfType::hR(bytes.clone())).map_err(|e| e.to_string())?;
            }
        }
    }
    if let Some(a) = attach {
        builder = builder
            .set("ak", VsfType::u(a.kind as usize, false))
            .map_err(|e| e.to_string())?
            .set("aw", VsfType::u(a.w as usize, false))
            .map_err(|e| e.to_string())?
            .set("ah", VsfType::u(a.h as usize, false))
            .map_err(|e| e.to_string())?;
        if let Some(ph) = a.preview_hash {
            builder = builder.set("aph", VsfType::hb(ph.to_vec())).map_err(|e| e.to_string())?;
        }
        if !a.preview.is_empty() {
            builder = builder.set("apv", VsfType::hR(a.preview.clone())).map_err(|e| e.to_string())?;
        }
    }
    if let Some(g) = molecule {
        builder = builder.set("gfrom", VsfType::hb(g.from.to_vec())).map_err(|e| e.to_string())?;
        for a in &g.woven_authors {
            builder = builder.append_multi("gwa", vec![VsfType::hb(a.to_vec())]).map_err(|e| e.to_string())?;
        }
        if let Some(b) = g.blob.as_ref() {
            if !b.is_empty() {
                builder = builder.set("gpl", VsfType::hR(b.clone())).map_err(|e| e.to_string())?;
            }
        }
        if let Some(w) = g.wrap.as_ref() {
            builder = builder.set("grcpt", VsfType::hb(w.recipient_device.to_vec())).map_err(|e| e.to_string())?;
            builder = builder.set("gbid", VsfType::hb(w.bundle_id.to_vec())).map_err(|e| e.to_string())?;
            builder = builder.set("gera", VsfType::u(w.era as usize, false)).map_err(|e| e.to_string())?;
            builder = builder.set("glin", VsfType::hb(w.era_lineage.to_vec())).map_err(|e| e.to_string())?;
            builder = builder.set("gnonce", VsfType::hb(w.nonce.to_vec())).map_err(|e| e.to_string())?;
            builder = builder.set("gwrap", VsfType::hR(w.sealed.clone())).map_err(|e| e.to_string())?;
        }
    }
    if let Some(c) = control {
        builder = crate::types::row_control::put_control(builder, c)?;
    }
    if let Some(f) = file {
        builder = crate::types::row_control::put_file(builder, f)?;
    }
    let section_bytes = builder.encode().map_err(|e| e.to_string())?;

    // A COMPLETE VSF FILE (AGENT.md transport rule): header, creation time, TOC, provenance hash — the same framing every other transport carries, so the payload is self-consistent before a single field is trusted.
    vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_unboxed(MSG_SECTION, section_bytes)
        .build()
        .map_err(|e| e.to_string())
}

/// Decode a message package. Any failure — not a document, wrong section, missing body — is ONE error to the caller, which treats it as the fork detector's evidence exactly as a garbage decrypt always was.
pub fn parse_message_package(plain: &[u8]) -> Result<MessagePackage, String> {
    let section = SectionBuilder::parse_document(msg_schema(), plain, None)
        .map_err(|e| format!("message package failed verified read: {e}"))?;

    let body = section
        .get_fields("body")
        .first()
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::x(s) => Some(s.clone()),
            _ => None,
        })
        .ok_or("package missing body")?;
    let incorporated_hp: [u8; 32] = section
        .get_fields("ihp")
        .first()
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::hp(h) if h.len() == 32 => <[u8; 32]>::try_from(h.as_slice()).ok(),
            _ => None,
        })
        .unwrap_or([0u8; 32]);
    let woven_times: Vec<i64> = section
        .get_fields("wt")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::e(vsf::types::EtType::e6(t)) => Some(*t),
            _ => None,
        })
        .collect();
    let ref_kind = section
        .get_fields("refk")
        .first()
        .and_then(|f| f.values.first())
        .and_then(|v| v.as_u64().and_then(|k| u8::try_from(k).ok()))
        .unwrap_or(0);
    let ref_target = section
        .get_fields("reft")
        .first()
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::e(vsf::types::EtType::e6(t)) => Some(*t),
            _ => None,
        })
        .unwrap_or(0);
    // Bridge extras — every reader width-agnostic (as_u64/as_i64, never exact-width matches) and every absence a clean None.
    let text_field = |name: &str| -> Option<String> {
        section
            .get_fields(name)
            .first()
            .and_then(|f| f.values.first())
            .and_then(|v| match v {
                VsfType::x(s) => Some(s.clone()),
                _ => None,
            })
    };
    let bridge = BridgeWire {
        host: text_field("bhost"),
        cwd: text_field("bcwd"),
        seq: section
            .get_fields("bseq")
            .first()
            .and_then(|f| f.values.first())
            .and_then(|v| v.as_u64()),
        exit: section
            .get_fields("bexit")
            .first()
            .and_then(|f| f.values.first())
            .and_then(|v| v.as_i64()),
        sig: section
            .get_fields("bsig")
            .first()
            .and_then(|f| f.values.first())
            .and_then(|v| v.as_u64()),
        delta: section
            .get_fields("bdelta")
            .first()
            .and_then(|f| f.values.first())
            .and_then(|v| v.as_u64())
            .map_or(false, |v| v != 0),
        // ALL THREE or none: a pigeon without its hash is an announcement of bytes nobody can fetch, and a hash without a name has nowhere to land. A partial set is not a smaller pigeon, it is a malformed frame.
        pigeon: {
            let name = section.get_fields("bpn").first().and_then(|f| f.values.first()).and_then(|v| match v {
                VsfType::x(t) => Some(t.clone()),
                _ => None,
            });
            let hash = section.get_fields("bph").first().and_then(|f| f.values.first()).and_then(|v| match v {
                VsfType::hb(b) => <[u8; 32]>::try_from(b.as_slice()).ok(),
                _ => None,
            });
            let size = section.get_fields("bps").first().and_then(|f| f.values.first()).and_then(|v| v.as_u64());
            match (name, hash, size) {
                (Some(name), Some(hash), Some(size)) if !name.is_empty() => Some(BridgePigeon { name, hash, size }),
                _ => None,
            }
        },
    };
    // Marks: four order-correlated multi-fields zipped back into tuples; any length mismatch = drop all (the text stands, links degrade).
    let u_list = |name: &str| -> Vec<u64> {
        section
            .get_fields(name)
            .iter()
            .filter_map(|f| f.values.first())
            .filter_map(|v| v.as_u64())
            .collect()
    };
    let mk = u_list("mk");
    let ms = u_list("ms");
    let ml = u_list("ml");
    let md: Vec<String> = section
        .get_fields("md")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::x(s) => Some(s.clone()),
            _ => None,
        })
        .collect();
    // Era-ratchet KEM material: any of the three present = an era row; absent on every ordinary message.
    let bytes_field = |name: &str| -> Vec<u8> {
        section
            .get_fields(name)
            .first()
            .and_then(|f| f.values.first())
            .and_then(|v| match v {
                VsfType::hR(b) => Some(b.clone()),
                _ => None,
            })
            .unwrap_or_default()
    };
    let era_kem = {
        let k = crate::crypto::era::EraKemWire { mlkem: bytes_field("ekn"), x25519: bytes_field("ekx"), hqc: bytes_field("ekh") };
        (!k.mlkem.is_empty() || !k.x25519.is_empty() || !k.hqc.is_empty()).then_some(k)
    };
    let marks = if mk.len() == ms.len() && mk.len() == ml.len() && mk.len() == md.len() {
        mk.iter()
            .zip(&ms)
            .zip(&ml)
            .zip(&md)
            .filter_map(|(((k, s), l), d)| {
                Some((u8::try_from(*k).ok()?, *s as usize, *l as usize, d.clone()))
            })
            .collect()
    } else {
        Vec::new()
    };
    // Attachment extras: a non-zero kind is the presence flag; the rest are width-agnostic optionals.
    let u_field = |name: &str| -> Option<u64> {
        section.get_fields(name).first().and_then(|f| f.values.first()).and_then(|v| v.as_u64())
    };
    let attach = match u_field("ak").unwrap_or(0) {
        0 => None,
        k => Some(AttachWire {
            kind: u8::try_from(k).unwrap_or(0),
            w: u_field("aw").unwrap_or(0) as u32,
            h: u_field("ah").unwrap_or(0) as u32,
            preview_hash: section.get_fields("aph").first().and_then(|f| f.values.first()).and_then(|v| match v {
                VsfType::hb(h) => <[u8; 32]>::try_from(h.as_slice()).ok(),
                _ => None,
            }),
            preview: bytes_field("apv"),
        }),
    };
    // Group extras: `gfrom` is the presence flag (a group frame always attributes). Weave authors must pair 1:1 with woven times — a mismatch drops the AUTHORS (the times still drive the braid advance; the receive path treats authorless refs as unresolvable and parks nothing on them).
    let molecule = section
        .get_fields("gfrom")
        .first()
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::hb(b) => <[u8; 32]>::try_from(b.as_slice()).ok(),
            _ => None,
        })
        .map(|from| {
            let authors: Vec<[u8; 32]> = section
                .get_fields("gwa")
                .iter()
                .filter_map(|f| f.values.first())
                .filter_map(|v| match v {
                    VsfType::hb(b) => <[u8; 32]>::try_from(b.as_slice()).ok(),
                    _ => None,
                })
                .collect();
            let blob = bytes_field("gpl");
            // The wrap set is all-or-nothing: recipient, era, lineage, nonce and the sealed bytes, or no wrap at all (a partial set is malformed, never a guess).
            let hash32 = |name: &str| -> Option<[u8; 32]> {
                section.get_fields(name).first().and_then(|f| f.values.first()).and_then(|v| match v {
                    VsfType::hb(b) => <[u8; 32]>::try_from(b.as_slice()).ok(),
                    _ => None,
                })
            };
            let sealed = bytes_field("gwrap");
            let wrap = match (hash32("grcpt"), hash32("gbid"), u_field("gera"), hash32("glin"), hash32("gnonce"), !sealed.is_empty()) {
                (Some(recipient_device), Some(bundle_id), Some(era), Some(era_lineage), Some(nonce), true) => Some(BondWrapWire { recipient_device, bundle_id, era, era_lineage, nonce, sealed }),
                _ => None,
            };
            MoleculeWire {
                from,
                woven_authors: if authors.len() == woven_times.len() { authors } else { Vec::new() },
                blob: (!blob.is_empty()).then_some(blob),
                wrap,
            }
        });
    let control = crate::types::row_control::get_control(&section);
    let file = crate::types::row_control::get_file(&section);
    Ok(MessagePackage {
        body,
        incorporated_hp,
        woven_times,
        reference: match ref_kind {
            0 => None,
            k => Some((k, ref_target)),
        },
        bridge: (!bridge.is_empty()).then_some(bridge),
        marks,
        era_kem,
        attach,
        molecule,
        control,
        file,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The era-ratchet KEM material rides as typed fields: present iff any of the three is non-empty, absent on an ordinary message, and a set that excludes an algorithm leaves its field off the wire.
    #[test]
    fn era_kem_fields_round_trip_and_are_absent_on_plain_rows() {
        let wire = crate::crypto::era::EraKemWire { mlkem: vec![1u8; 1568], x25519: vec![2u8; 32], hqc: Vec::new() };
        let built = build_message_package_era("", &[0u8; 32], &[], None, None, &[], &[], Some(&wire), None, None, Some(&crate::types::RowControl::Era(crate::crypto::era::EraSignal::Init { era_next: 1, nonce: [0; 32], prior_tag: 10, kem_set: 3 })), None).unwrap();
        let pkg = parse_message_package(&built).unwrap();
        assert_eq!(pkg.era_kem, Some(wire));
        let plain = build_message_package("hi", &[0u8; 32], &[], None, None, &[], &[]).unwrap();
        assert_eq!(parse_message_package(&plain).unwrap().era_kem, None);
    }

    /// The group extras ride typed beside the text (the era-KEM doctrine): attribution, paired weave authors, the record blob — absent on every friendship row, and a wa/wt count mismatch drops the authors while the times keep driving the braid.
    #[test]
    fn group_wire_rides_and_is_absent_on_plain_rows() {
        let g = MoleculeWire { from: [0xAA; 32], woven_authors: vec![[0xB1; 32], [0xB2; 32]], blob: Some(vec![7u8; 300]), wrap: None };
        let built = build_message_package_era("", &[0u8; 32], &[5, 9], None, None, &[], &[], None, None, Some(&g), Some(&crate::types::RowControl::Molecule(crate::types::molecule::MoleculeSignal::Records)), None).unwrap();
        let pkg = parse_message_package(&built).unwrap();
        assert_eq!(pkg.molecule, Some(g.clone()));
        assert_eq!(pkg.woven_times, vec![5, 9]);
        let plain = build_message_package("hi", &[0u8; 32], &[], None, None, &[], &[]).unwrap();
        assert_eq!(parse_message_package(&plain).unwrap().molecule, None);
        // Mismatched pairing: one author for two times — authors drop, attribution and blob stay.
        let bad = MoleculeWire { from: [0xAA; 32], woven_authors: vec![[0xB1; 32]], blob: None, wrap: None };
        let built = build_message_package_era("x", &[0u8; 32], &[5, 9], None, None, &[], &[], None, None, Some(&bad), None, None).unwrap();
        let pkg = parse_message_package(&built).unwrap();
        assert_eq!(pkg.molecule.as_ref().unwrap().from, [0xAA; 32]);
        assert!(pkg.molecule.as_ref().unwrap().woven_authors.is_empty());
        assert_eq!(pkg.woven_times, vec![5, 9]);
    }

    /// A wrap's sealed secret rides as TYPED fields (all five, or none) beside the KEM ciphertexts and never appears in the text — the row that persists is the bare kind marker.
    #[test]
    fn group_wrap_rides_typed_never_in_text() {
        let w = BondWrapWire { recipient_device: [0x11; 32], bundle_id: [0x22; 32], era: 300, era_lineage: [0x33; 32], nonce: [0x44; 32], sealed: vec![0x55; 80] };
        let kem = crate::crypto::era::EraKemWire { mlkem: vec![1; 8], x25519: vec![2; 32], hqc: vec![3; 8] };
        let g = MoleculeWire { from: [0xAA; 32], woven_authors: Vec::new(), blob: None, wrap: Some(w.clone()) };
        let ctl = crate::types::RowControl::Molecule(crate::types::molecule::MoleculeSignal::Wrap);
        let built = build_message_package_era("", &[0u8; 32], &[], None, None, &[], &[], Some(&kem), None, Some(&g), Some(&ctl), None).unwrap();
        let pkg = parse_message_package(&built).unwrap();
        assert_eq!(pkg.molecule.as_ref().unwrap().wrap, Some(w));
        assert_eq!(pkg.era_kem, Some(kem));
        assert_eq!(pkg.body, "", "a control row carries no text");
        assert_eq!(pkg.control, Some(ctl.clone()));
        // A partial set (attribution without the wrap fields) is no wrap at all.
        let plain = build_message_package_era("", &[0u8; 32], &[], None, None, &[], &[], None, None, Some(&MoleculeWire { from: [0xAA; 32], woven_authors: Vec::new(), blob: None, wrap: None }), Some(&ctl), None).unwrap();
        assert_eq!(parse_message_package(&plain).unwrap().molecule.as_ref().unwrap().wrap, None);
    }

    /// Round-trip: every field survives, the reference travels typed, empty body and zero wovens are legal, and garbage is ONE clean error (fork-detector food, never a panic).
    #[test]
    fn package_round_trips_and_garbage_is_one_error() {
        let built = build_message_package(
            "hello \u{1F30D}",
            &[7u8; 32],
            &[111, 222],
            Some((1, 987_654_321)),
            None,
            &[],
            &[0xAA; 17],
        )
        .unwrap();
        let parsed = parse_message_package(&built).unwrap();
        assert_eq!(parsed.body, "hello \u{1F30D}");
        assert_eq!(parsed.incorporated_hp, [7u8; 32]);
        assert_eq!(parsed.woven_times, vec![111, 222]);
        assert_eq!(parsed.reference, Some((1, 987_654_321)));
        assert_eq!(parsed.bridge, None);

        // A reaction retract: empty body, no wovens, no pad.
        let retract = build_message_package("", &[0u8; 32], &[], Some((3, 42)), None, &[], &[]).unwrap();
        let parsed = parse_message_package(&retract).unwrap();
        assert_eq!(parsed.body, "");
        assert_eq!(parsed.reference, Some((3, 42)));

        // A plain message carries no reference.
        let plain = build_message_package("hi", &[0u8; 32], &[5], None, None, &[], &[1, 2, 3]).unwrap();
        assert_eq!(parse_message_package(&plain).unwrap().reference, None);

        assert!(parse_message_package(b"not a vsf document").is_err());
    }

    /// The attachment extras ride as typed fields and zip back losslessly; a plain package parses to None.
    #[test]
    fn attach_fields_round_trip_typed() {
        let a = AttachWire { kind: 1, w: 4000, h: 3000, preview_hash: Some([7u8; 32]), preview: vec![2, 2, 9, 9, 9, 8, 8, 8, 7, 7, 7, 6, 6, 6] };
        let f = crate::types::AttachRef::file([9u8; 32], "photo.jpg", 123_456);
        let built = build_message_package_era("", &[0u8; 32], &[], None, None, &[], &[], None, Some(&a), None, None, Some(&f)).unwrap();
        let pkg = parse_message_package(&built).unwrap();
        assert_eq!(pkg.attach, Some(a));
        assert_eq!(pkg.file, Some(f), "the attachment's identity rides typed, not in the body");
        assert_eq!(pkg.body, "");
        let plain = build_message_package("hi", &[0u8; 32], &[], None, None, &[], &[]).unwrap();
        assert!(parse_message_package(&plain).unwrap().attach.is_none());
    }

    /// Every control kind rides as typed fields and a plain package carries none.
    #[test]
    fn control_rows_ride_typed() {
        let id = [0x31; 16];
        for c in [
            crate::types::RowControl::Probe,
            crate::types::RowControl::Delete { target: 77 },
            crate::types::RowControl::Wave(crate::wave::signal::WaveSignal::Offer { wave_id: id, nonce: [2; 32], device: Some([3; 32]) }),
            crate::types::RowControl::Wave(crate::wave::signal::WaveSignal::Hangup { wave_id: id }),
            crate::types::RowControl::Era(crate::crypto::era::EraSignal::Nudge { prior_tag: 9 }),
        ] {
            let built = build_message_package_era("", &[0u8; 32], &[], None, None, &[], &[], None, None, None, Some(&c), None).unwrap();
            assert_eq!(parse_message_package(&built).unwrap().control, Some(c));
        }
        let plain = build_message_package("hi", &[0u8; 32], &[], None, None, &[], &[]).unwrap();
        let pkg = parse_message_package(&plain).unwrap();
        assert!(pkg.control.is_none() && pkg.file.is_none());
    }

    /// Marks ride as four correlated multi-fields and zip back losslessly; a plain package parses to zero marks.
    #[test]
    fn marks_round_trip_typed() {
        let marks = vec![
            (1u8, 4usize, 8usize, "https://passless.org/".to_string()),
            (1u8, 20usize, 5usize, "http://example.com".to_string()),
        ];
        let built = build_message_package("see passless and more", &[0u8; 32], &[], None, None, &marks, &[]).unwrap();
        assert_eq!(parse_message_package(&built).unwrap().marks, marks);
        let plain = build_message_package("no links", &[0u8; 32], &[], None, None, &[], &[]).unwrap();
        assert!(parse_message_package(&plain).unwrap().marks.is_empty());
    }

    /// Two packages with identical inputs but different pads differ on the wire (size jitter works) yet parse identically — the pad carries no meaning.
    #[test]
    fn pad_jitters_the_wire_without_touching_meaning() {
        let a = build_message_package("x", &[1u8; 32], &[9], None, None, &[], &[0u8; 4]).unwrap();
        let b = build_message_package("x", &[1u8; 32], &[9], None, None, &[], &[0u8; 40]).unwrap();
        assert_ne!(a.len(), b.len());
        assert_eq!(
            parse_message_package(&a).unwrap(),
            parse_message_package(&b).unwrap()
        );
    }

    /// Bridge extras ride as named typed fields — locus + stream state + interrupt all survive the round trip, absence parses as None, and a partial (no exit) is distinguishable from a final.
    #[test]
    fn bridge_extras_round_trip_typed() {
        let wire = BridgeWire {
            host: Some("leviathan".into()),
            cwd: Some("/mnt/Harbor/Code/photon".into()),
            seq: Some(7),
            exit: None,
            sig: None,
            delta: true,
            pigeon: None,
        };
        let partial =
            build_message_package("out so far", &[0u8; 32], &[], Some((4, 999)), Some(&wire), &[], &[])
                .unwrap();
        let parsed = parse_message_package(&partial).unwrap();
        let b = parsed.bridge.expect("bridge extras present");
        assert_eq!(b.host.as_deref(), Some("leviathan"));
        assert_eq!(b.cwd.as_deref(), Some("/mnt/Harbor/Code/photon"));
        assert_eq!(b.seq, Some(7));
        assert_eq!(b.exit, None);
        assert!(b.delta, "append-semantics flag survives the round trip");

        let fin = BridgeWire { exit: Some(-1), seq: Some(8), ..wire };
        let final_frame =
            build_message_package("done", &[0u8; 32], &[], Some((4, 999)), Some(&fin), &[], &[])
                .unwrap();
        assert_eq!(parse_message_package(&final_frame).unwrap().bridge.unwrap().exit, Some(-1));

        let ctl = BridgeWire { sig: Some(2), ..Default::default() };
        let ctl_frame =
            build_message_package(" ", &[0u8; 32], &[], Some((5, 999)), Some(&ctl), &[], &[]).unwrap();
        assert_eq!(parse_message_package(&ctl_frame).unwrap().bridge.unwrap().sig, Some(2));
    }
}
