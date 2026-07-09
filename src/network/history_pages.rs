//! History-recovery page codec — the KEY-AGNOSTIC seal/open layer for conversation backfill.
//!
//! A page is a batch of plaintext conversation rows (newest-first cursor pagination) encoded as a
//! schema-validated VSF section and sealed with kete ChaCha20-Poly1305 under a bare 32-byte key.
//! Phase 1 (friend recovery) seals under the friendship history key (`FriendshipChains::history_key`,
//! spaghettify-derived at ceremony birth); phase 2 (fleet sync) reuses this codec verbatim under the
//! fleet key — nothing in this module knows which. Page metadata (`oldest_osc`, `more`) lives INSIDE
//! the seal so the wire leaks nothing beyond conversation token + blob size.

use vsf::schema::{SectionBuilder, SectionSchema, TypeConstraint};
use vsf::VsfType;

/// Max rows per served page. ~50 keeps a typical page at a few KB sealed (PT shards anything bigger).
pub const MAX_PAGE_ROWS: usize = 50;
/// Max summed plaintext content bytes per page — the serve-side byte budget before the seal.
pub const MAX_PAGE_BYTES: usize = 24 * 1024;

/// One conversation row as served — the SENDER'S OWN view (`sender_outgoing` = their `is_outgoing`);
/// the requester flips direction on merge. `ack_hash` never travels (device-local reliability state).
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryRow {
    pub timestamp: i64,
    pub content: String,
    pub sender_outgoing: bool,
    pub delivered: bool,
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

/// Schema for the sealed page plaintext. Rows are four parallel multi-value arrays zipped on decode
/// (the `pending_*` idiom from friendship storage).
fn page_schema() -> SectionSchema {
    SectionSchema::new("hist_rows")
        .field("oldest", TypeConstraint::Any) // e6 eagle-time
        .field("more", TypeConstraint::AnyUnsigned) // bool
        .field("m_time", TypeConstraint::Any) // e6, one per row
        .field("m_text", TypeConstraint::Utf8Text) // x, one per row
        .field("m_out", TypeConstraint::AnyUnsigned) // bool, one per row (sender's is_outgoing)
        .field("m_del", TypeConstraint::AnyUnsigned) // bool, one per row
}

/// Encode + AEAD-seal a page under `key`. Key-agnostic: friendship history key today, fleet key later.
pub fn seal_history_page(page: &HistoryPagePlain, key: &[u8; 32]) -> Result<Vec<u8>, String> {
    let mut builder = page_schema()
        .build()
        .set("oldest", VsfType::e(vsf::types::EtType::e6(page.oldest_osc)))
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
            .map_err(|e| e.to_string())?;
    }
    let plain = builder.encode().map_err(|e| e.to_string())?;
    kete::encrypt_bytes(&plain, key)
}

/// AEAD-open + decode a page. Fails on wrong key, tamper, or malformed plaintext.
pub fn open_history_page(sealed: &[u8], key: &[u8; 32]) -> Result<HistoryPagePlain, String> {
    let plain = kete::decrypt_bytes(sealed, key)?;
    let section =
        SectionBuilder::parse(page_schema(), &plain).map_err(|e| format!("page parse: {e}"))?;

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

    // Zip the parallel arrays; a malformed page (mismatched lengths) yields the common prefix.
    let n = times.len().min(texts.len()).min(outs.len()).min(dels.len());
    let mut rows = Vec::with_capacity(n);
    for i in 0..n {
        rows.push(HistoryRow {
            timestamp: times[i],
            content: texts[i].clone(),
            sender_outgoing: outs[i],
            delivered: dels[i],
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
                },
                HistoryRow {
                    timestamp: 2_000,
                    content: "".to_string(), // empty content is a legal row
                    sender_outgoing: false,
                    delivered: false,
                },
                HistoryRow {
                    timestamp: 3_000,
                    content: "newest".to_string(),
                    sender_outgoing: true,
                    delivered: false,
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
