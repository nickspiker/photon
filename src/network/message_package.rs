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
}

/// Schema for the package section. `pad` is random bytes for traffic-analysis size jitter — schema'd sections serialize fields in schema order, so the old shuffled-field trick is gone; parsing is by NAME, which is the stronger version of what the shuffle enforced.
fn msg_schema() -> SectionSchema {
    SectionSchema::new(MSG_SECTION)
        .field("body", TypeConstraint::Utf8Text)
        .field("ihp", TypeConstraint::Any) // hp, 32 bytes
        .field("wt", TypeConstraint::Any) // e6, zero to two entries
        .field("refk", TypeConstraint::AnyUnsigned) // reference kind: 0 = none
        .field("reft", TypeConstraint::Any) // e6 reference target; 0 when kind is none
        .field("pad", TypeConstraint::Any) // hR random jitter; length is the only meaning
}

/// Encode a message package as a complete VSF document. The caller supplies the pad (already random) so this layer stays deterministic-in, deterministic-out.
pub fn build_message_package(
    body: &str,
    incorporated_hp: &[u8; 32],
    woven_times: &[i64],
    reference: Option<(u8, i64)>,
    pad: &[u8],
) -> Result<Vec<u8>, String> {
    let mut builder = msg_schema()
        .build()
        .set("body", VsfType::x(body.to_string()))
        .map_err(|e| e.to_string())?
        .set("ihp", VsfType::hp(incorporated_hp.to_vec()))
        .map_err(|e| e.to_string())?
        .set(
            "refk",
            VsfType::u3(reference.map(|(k, _)| k).unwrap_or(0)),
        )
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
    if !pad.is_empty() {
        builder = builder
            .set("pad", VsfType::hR(pad.to_vec()))
            .map_err(|e| e.to_string())?;
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
        .and_then(|v| match v {
            VsfType::u3(k) => Some(*k),
            VsfType::u4(k) => Some(*k as u8),
            _ => None,
        })
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
    Ok(MessagePackage {
        body,
        incorporated_hp,
        woven_times,
        reference: match ref_kind {
            0 => None,
            k => Some((k, ref_target)),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip: every field survives, the reference travels typed, empty body and zero wovens are legal, and garbage is ONE clean error (fork-detector food, never a panic).
    #[test]
    fn package_round_trips_and_garbage_is_one_error() {
        let built = build_message_package(
            "hello \u{1F30D}",
            &[7u8; 32],
            &[111, 222],
            Some((1, 987_654_321)),
            &[0xAA; 17],
        )
        .unwrap();
        let parsed = parse_message_package(&built).unwrap();
        assert_eq!(parsed.body, "hello \u{1F30D}");
        assert_eq!(parsed.incorporated_hp, [7u8; 32]);
        assert_eq!(parsed.woven_times, vec![111, 222]);
        assert_eq!(parsed.reference, Some((1, 987_654_321)));

        // A reaction retract: empty body, no wovens, no pad.
        let retract =
            build_message_package("", &[0u8; 32], &[], Some((3, 42)), &[]).unwrap();
        let parsed = parse_message_package(&retract).unwrap();
        assert_eq!(parsed.body, "");
        assert_eq!(parsed.reference, Some((3, 42)));

        // A plain message carries no reference.
        let plain = build_message_package("hi", &[0u8; 32], &[5], None, &[1, 2, 3]).unwrap();
        assert_eq!(parse_message_package(&plain).unwrap().reference, None);

        assert!(parse_message_package(b"not a vsf document").is_err());
    }

    /// Two packages with identical inputs but different pads differ on the wire (size jitter works) yet parse identically — the pad carries no meaning.
    #[test]
    fn pad_jitters_the_wire_without_touching_meaning() {
        let a = build_message_package("x", &[1u8; 32], &[9], None, &[0u8; 4]).unwrap();
        let b = build_message_package("x", &[1u8; 32], &[9], None, &[0u8; 40]).unwrap();
        assert_ne!(a.len(), b.len());
        assert_eq!(
            parse_message_package(&a).unwrap(),
            parse_message_package(&b).unwrap()
        );
    }
}
