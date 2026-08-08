//! History-recovery page codec — the KEY-AGNOSTIC seal/open layer for conversation backfill.
//!
//! A page is a batch of plaintext conversation rows (newest-first cursor pagination) encoded as a schema-validated VSF section and sealed with kete ChaCha20-Poly1305 under a bare 32-byte key. Phase 1 (friend recovery) seals under the friendship history key (`FriendshipChains::history_key`, spaghettify-derived at ceremony birth); phase 2 (fleet sync) reuses this codec verbatim under the fleet key — nothing in this module knows which. Page metadata (`oldest_osc`, `more`) lives INSIDE the seal so the wire leaks nothing beyond conversation token + blob size.

use vsf::schema::{SectionBuilder, SectionSchema, TypeConstraint};
use vsf::VsfType;

/// Max rows per served page. ~50 keeps a typical page at a few KB sealed (PT shards anything bigger).
pub const MAX_PAGE_ROWS: usize = 50;
/// Max summed plaintext content bytes per page — the serve-side byte budget before the seal.
pub const MAX_PAGE_BYTES: usize = 24 * 1024;

/// One conversation row as served — the SENDER'S OWN view (`sender_outgoing` = their `is_outgoing`); the requester flips direction on merge. `ack_hash` never travels (device-local reliability state).
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryRow {
    pub timestamp: i64,
    pub content: String,
    pub sender_outgoing: bool,
    pub delivered: bool,
    /// Tombstone flag — deleted-for-everyone, monotonic true-wins on merge. Content still rides (braid weave dependency; see ChatMessage::deleted).
    pub deleted: bool,
    /// Typed reference (raw wire kind, target eagle_time) — reply/edit/react metadata as page COLUMNS, never string-encoded into content. Absent on pre-feature pages ⇒ None (the m_tomb additive idiom).
    pub reference: Option<(u8, i64)>,
}

/// A decoded (pre-seal / post-open) history page.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryPagePlain {
    /// Rows in ascending time order.
    pub rows: Vec<HistoryRow>,
    /// The oldest timestamp in this page — the requester's next `before` cursor. When `rows` is empty this is the cursor the request asked for (no progress; `more` will be false).
    pub oldest_osc: i64,
    /// Whether rows older than `oldest_osc` remain on the server.
    pub more: bool,
}

/// The section name, shared by the document builder and the TOC lookup in `open_history_page` — `parse_document` finds the section by matching this against the header, so the two must never drift.
const PAGE_SECTION: &str = "hist_rows";

/// Schema for the sealed page plaintext. Rows are four parallel multi-value arrays zipped on decode (the `pending_*` idiom from friendship storage).
fn page_schema() -> SectionSchema {
    SectionSchema::new(PAGE_SECTION)
        .field("oldest", TypeConstraint::Any) // e6 eagle-time
        .field("more", TypeConstraint::AnyUnsigned) // bool
        .field("m_time", TypeConstraint::Any) // e6, one per row
        .field("m_text", TypeConstraint::Utf8Text) // x, one per row
        .field("m_out", TypeConstraint::AnyUnsigned) // bool, one per row (sender's is_outgoing)
        .field("m_del", TypeConstraint::AnyUnsigned) // bool, one per row
        .field("m_tomb", TypeConstraint::AnyUnsigned) // bool, one per row: the deleted-for-everyone tombstone (absent on pre-feature pages → all false)
        .field("m_refk", TypeConstraint::AnyUnsigned) // reference kind, one per row: 0 = none, else RefKind wire value (absent on pre-feature pages → all none)
        .field("m_reft", TypeConstraint::Any) // e6 reference target, one per row: 0 when kind is none
}

/// Encode + AEAD-seal a page under `key`. Key-agnostic: friendship history key today, fleet key later.
pub fn seal_history_page(page: &HistoryPagePlain, key: &[u8; 32]) -> Result<Vec<u8>, String> {
    let mut builder = page_schema()
        .build()
        .set(
            "oldest",
            VsfType::e(vsf::types::EtType::e6(page.oldest_osc)),
        )
        .map_err(|e| e.to_string())?
        .set("more", page.more)
        .map_err(|e| e.to_string())?;
    for row in &page.rows {
        builder = builder
            .append_multi(
                "m_time",
                vec![VsfType::e(vsf::types::EtType::e6(row.timestamp))],
            )
            .map_err(|e| e.to_string())?
            .append_multi("m_text", vec![VsfType::x(row.content.clone())])
            .map_err(|e| e.to_string())?
            .append_multi("m_out", vec![VsfType::u3(row.sender_outgoing as u8)])
            .map_err(|e| e.to_string())?
            .append_multi("m_del", vec![VsfType::u3(row.delivered as u8)])
            .map_err(|e| e.to_string())?
            .append_multi("m_tomb", vec![VsfType::u3(row.deleted as u8)])
            .map_err(|e| e.to_string())?
            .append_multi(
                "m_refk",
                vec![VsfType::u3(row.reference.map(|(k, _)| k).unwrap_or(0))],
            )
            .map_err(|e| e.to_string())?
            .append_multi(
                "m_reft",
                vec![VsfType::e(vsf::types::EtType::e6(
                    row.reference.map(|(_, t)| t).unwrap_or(0),
                ))],
            )
            .map_err(|e| e.to_string())?;
    }
    let section_bytes = builder.encode().map_err(|e| e.to_string())?;

    // A COMPLETE VSF FILE inside the seal, not a bare section (AGENT.md: "VSF Transport Rule: COMPLETE FILES ONLY"). `.encode()` alone emits the section body — no header, no creation time, no TOC, no BLAKE3 provenance hash — so the open side had nothing to verify and fell back to a raw schema parse.
    // The AEAD is not a substitute here. These pages ride the FLEET key to every sibling device, so "it decrypted" proves only that SOMEONE IN THE FLEET wrote it — exactly what the signed outer frame already proved. The provenance hash is what makes the payload itself self-consistent, and it costs one builder call.
    let doc = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_unboxed(PAGE_SECTION, section_bytes)
        .build()
        .map_err(|e| e.to_string())?;

    kete::encrypt_bytes(&doc, key)
}

/// AEAD-open + decode a page. Fails on wrong key, tamper, or malformed plaintext.
pub fn open_history_page(sealed: &[u8], key: &[u8; 32]) -> Result<HistoryPagePlain, String> {
    let plain = kete::decrypt_bytes(sealed, key)?;
    // Verified read — `parse_document` runs `read_verified` (header decode + provenance self-consistency) before a single field is trusted. A pre-document page is a bare section with no TOC and fails here, which is correct: pages are a resyncable cache, so the requester simply re-fetches and the server re-seals in document form. No compat branch (AGENT.md: atomic updates, no protocol forks).
    let section = SectionBuilder::parse_document(page_schema(), &plain, None)
        .map_err(|e| format!("page failed verified read: {e}"))?;

    let oldest_osc = section
        .get_fields("oldest")
        .first()
        .and_then(|f| f.values.first())
        .and_then(|v| match v {
            VsfType::e(vsf::types::EtType::e6(osc)) => Some(*osc),
            _ => None,
        })
        .ok_or("page missing oldest")?;
    let more = section.get_value::<bool>("more").unwrap_or(false);

    let times: Vec<i64> = section
        .get_fields("m_time")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::e(vsf::types::EtType::e6(osc)) => Some(*osc),
            _ => None,
        })
        .collect();
    let texts: Vec<String> = section
        .get_fields("m_text")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::x(s) => Some(s.clone()),
            _ => None,
        })
        .collect();
    let outs: Vec<bool> = section
        .get_fields("m_out")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(vsf_bool)
        .collect();
    let dels: Vec<bool> = section
        .get_fields("m_del")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(vsf_bool)
        .collect();

    // Tombstones (optional field — absent on pre-feature pages ⇒ all false).
    let tombs: Vec<bool> = section
        .get_fields("m_tomb")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(vsf_bool)
        .collect();
    // Typed references (optional parallel columns — absent on pre-feature pages ⇒ all none).
    let ref_kinds: Vec<u8> = section
        .get_fields("m_refk")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::u3(n) => Some(*n),
            VsfType::u4(n) => Some(*n as u8),
            _ => None,
        })
        .collect();
    let ref_targets: Vec<i64> = section
        .get_fields("m_reft")
        .iter()
        .filter_map(|f| f.values.first())
        .filter_map(|v| match v {
            VsfType::e(vsf::types::EtType::e6(osc)) => Some(*osc),
            _ => None,
        })
        .collect();

    // Zip the parallel arrays; a malformed page (mismatched lengths) yields the common prefix.
    let n = times.len().min(texts.len()).min(outs.len()).min(dels.len());
    let mut rows = Vec::with_capacity(n);
    for i in 0..n {
        rows.push(HistoryRow {
            timestamp: times[i],
            content: texts[i].clone(),
            sender_outgoing: outs[i],
            delivered: dels[i],
            deleted: tombs.get(i).copied().unwrap_or(false),
            reference: match ref_kinds.get(i).copied().unwrap_or(0) {
                0 => None,
                k => Some((k, ref_targets.get(i).copied().unwrap_or(0))),
            },
        });
    }
    Ok(HistoryPagePlain {
        rows,
        oldest_osc,
        more,
    })
}

/// Width-tolerant VSF unsigned → bool (writers may emit u3 or wider).
fn vsf_bool(v: &VsfType) -> Option<bool> {
    match v {
        VsfType::u3(n) => Some(*n != 0),
        VsfType::u4(n) => Some(*n != 0),
        VsfType::u5(n) => Some(*n != 0),
        VsfType::u6(n) => Some(*n != 0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_page() -> HistoryPagePlain {
        HistoryPagePlain {
            rows: vec![
                HistoryRow {
                    timestamp: 1_000,
                    content: "oldest in page 👋 unicode".to_string(),
                    sender_outgoing: true,
                    delivered: true,
                    deleted: false,
                    reference: None,
                },
                HistoryRow {
                    timestamp: 2_000,
                    content: "".to_string(), // empty content is a legal row
                    sender_outgoing: false,
                    delivered: false,
                    deleted: false,
                    reference: Some((1, 1_000)), // a reply column rides the page
                },
                HistoryRow {
                    timestamp: 3_000,
                    content: "newest".to_string(),
                    sender_outgoing: true,
                    delivered: false,
                    deleted: false,
                    reference: None,
                },
            ],
            oldest_osc: 1_000,
            more: true,
        }
    }

    #[test]
    fn seal_open_round_trip() {
        let key = [0x42u8; 32];
        let page = sample_page();
        let sealed = seal_history_page(&page, &key).unwrap();
        let opened = open_history_page(&sealed, &key).unwrap();
        assert_eq!(opened, page);
    }

    #[test]
    fn empty_page_round_trip() {
        let key = [0x42u8; 32];
        let page = HistoryPagePlain {
            rows: Vec::new(),
            oldest_osc: i64::MAX,
            more: false,
        };
        let sealed = seal_history_page(&page, &key).unwrap();
        let opened = open_history_page(&sealed, &key).unwrap();
        assert_eq!(opened, page);
    }

    #[test]
    fn wrong_key_fails() {
        let page = sample_page();
        let sealed = seal_history_page(&page, &[0x42u8; 32]).unwrap();
        assert!(open_history_page(&sealed, &[0x43u8; 32]).is_err());
    }

    /// The sealed plaintext must be a COMPLETE VSF FILE (AGENT.md: "VSF Transport Rule: COMPLETE FILES ONLY"), not a bare section. These pages ride the fleet key to every sibling, so the payload needs its own integrity anchor — the AEAD only proves "someone in the fleet wrote this", which the signed outer frame already proved.
    #[test]
    fn sealed_page_is_a_complete_vsf_document() {
        let key = [4u8; 32];
        let page = HistoryPagePlain {
            rows: vec![HistoryRow {
                timestamp: 7,
                content: "hi".to_string(),
                sender_outgoing: true,
                delivered: false,
                deleted: false,
                reference: None,
            }],
            oldest_osc: 7,
            more: false,
        };
        let sealed = seal_history_page(&page, &key).expect("seal");
        let plain = kete::decrypt_bytes(&sealed, &key).expect("open");
        assert!(
            plain.starts_with(b"R\xc3\x85<"),
            "must carry the R\u{c5}< magic, got {:?}",
            &plain[..plain.len().min(8)]
        );
        let (header, _) =
            vsf::verification::read_verified(&plain, None).expect("read_verified must accept it");
        assert!(
            matches!(header.provenance_hash, vsf::VsfType::hp(ref h) if h.len() == 32),
            "a page without a 32-byte hp has nothing to verify"
        );
    }

    /// A pre-document page (bare section) must be REJECTED, not parsed on faith. Pages are a resyncable cache — the requester just re-fetches and the server re-seals in document form.
    #[test]
    fn pre_document_page_is_rejected() {
        let key = [6u8; 32];
        let bare = page_schema()
            .build()
            .set("oldest", VsfType::e(vsf::types::EtType::e6(0)))
            .expect("oldest")
            .set("more", VsfType::u3(0))
            .expect("more")
            .encode()
            .expect("bare section");
        let sealed = kete::encrypt_bytes(&bare, &key).expect("seal");
        assert!(
            open_history_page(&sealed, &key).is_err(),
            "a headerless page has no provenance hash — it must fail the verified read"
        );
    }

    #[test]
    fn tampered_blob_fails() {
        let key = [0x42u8; 32];
        let page = sample_page();
        let mut sealed = seal_history_page(&page, &key).unwrap();
        let mid = sealed.len() / 2;
        sealed[mid] ^= 0x01;
        assert!(open_history_page(&sealed, &key).is_err());
    }
}
