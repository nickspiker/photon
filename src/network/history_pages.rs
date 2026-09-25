//! History-recovery page codec — the KEY-AGNOSTIC seal/open layer for conversation backfill.
//!
//! A page is a batch of plaintext conversation rows (newest-first cursor pagination) encoded as a complete VSF document (one section per row) and sealed with kete ChaCha20-Poly1305 under a bare 32-byte key. Phase 1 (friend recovery) seals under the friendship history key (`FriendshipChains::history_key`, spaghettify-derived at ceremony birth); phase 2 (fleet sync) reuses this codec verbatim under the fleet key — nothing in this module knows which. Page metadata (`oldest_osc`, `more`) lives INSIDE the seal so the wire leaks nothing beyond conversation token + blob size.

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
    /// The fleet's alert duty for this row is DISCHARGED (notification design 2026-07-23). Sibling pages carry it so a forwarded row never re-dings; absent on pre-feature pages ⇒ true (history is silent). FRIEND-route pages carry the FRIEND's flag, which the merge overrides to true — their alert state is not ours.
    pub notified: bool,
    /// Typed content marks (links) — page COLUMNS beside the content, never string-encoded into it. Absent on pre-feature pages ⇒ empty; every consumer re-validates against the row's content.
    pub marks: Vec<crate::types::MessageMark>,
    /// Wave row payload as page COLUMNS (raw wire outcome, live seconds) — absent on pre-feature pages ⇒ None.
    pub wave: Option<(u8, u32)>,
    /// Recording row envelope thumbnail as a native multi-value column — absent ⇒ empty.
    pub envelope: Vec<u8>,
    /// Typed attachment columns (2026-09-10): (raw wire kind, w, h, preview-blob hash) — absent on pre-feature pages ⇒ None.
    pub attach: Option<(u8, u32, u32, Option<[u8; 32]>)>,
    /// The row's micro preview as a native multi-value column — absent ⇒ empty.
    pub preview: Vec<u8>,
    /// Star stamp (signed eagle osc; positive = starred, negative = unstarred, 0 = never touched) — absent on pre-feature pages ⇒ 0. Merge = larger |osc| wins.
    pub star_osc: i64,
    /// The author's party id (groups, docs/molecules.md §5) — absent on pairwise/pre-feature pages ⇒ None (derive from direction).
    pub author: Option<[u8; 32]>,
    /// A hidden control row's kind and fields, as page COLUMNS (flag day 2026-09-24) — the content is empty on these.
    pub control: Option<crate::types::RowControl>,
    /// An attachment row's identity (hash, name, size, role), as page COLUMNS — the content is empty on these.
    pub file: Option<crate::types::AttachRef>,
}

impl HistoryRow {
    /// The page view of a stored row — the ONE conversion both the served history page and the fleet push use, so the two can never drift in which fields ride.
    pub fn from_message(m: &crate::types::ChatMessage) -> Self {
        HistoryRow {
            author: m.author,
            star_osc: m.star_osc,
            timestamp: m.timestamp,
            content: m.content.clone(),
            sender_outgoing: m.is_outgoing,
            delivered: m.delivered,
            deleted: m.deleted,
            reference: m.reference.map(|(k, t)| (k as u8, t)),
            notified: m.notified,
            marks: m.marks.clone(),
            wave: m.wave.map(|w| (w.outcome as u8, w.secs)),
            envelope: m.envelope.clone(),
            attach: m.attach.map(|a| (a.kind as u8, a.dims.map_or(0, |d| d.0), a.dims.map_or(0, |d| d.1), a.preview_hash)),
            preview: m.preview.clone(),
            control: m.control.clone(),
            file: m.file.clone(),
        }
    }
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

/// The page document's section names, shared by the sealer and the opener — the two must never drift.
/// One `hist_page` section carries the cursor; every row is its OWN `row` section and every link mark its own `mark` section naming its row by index (the 2026-09-25 flag day — the parallel columns and their count/mask bookkeeping are gone). An optional part of a row is simply a field that is present or absent.
const PAGE_SECTION: &str = "hist_page";
const ROW_SECTION: &str = "row";
const MARK_SECTION: &str = "mark";

/// Encode + AEAD-seal a page under `key`. Key-agnostic: friendship history key today, fleet key later.
pub fn seal_history_page(page: &HistoryPagePlain, key: &[u8; 32]) -> Result<Vec<u8>, String> {
    let e6 = |v: i64| VsfType::e(vsf::types::EtType::e6(v));
    let uint = |v: u64| VsfType::u(v as usize, false); // WHY/PROOF: every photon target is 64-bit, so usize holds a u64 whole
    let f = |name: &str, v: VsfType| (name.to_string(), v);
    let mut doc = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .provenance_only()
        .add_section(PAGE_SECTION, vec![f("oldest", e6(page.oldest_osc)), f("more", uint(page.more as u64))]);
    for (i, row) in page.rows.iter().enumerate() {
        let mut r = vec![
            f("time", e6(row.timestamp)),
            f("text", VsfType::x(row.content.clone())),
            f("out", uint(row.sender_outgoing as u64)),
            f("delivered", uint(row.delivered as u64)),
            f("deleted", uint(row.deleted as u64)),
            f("notified", uint(row.notified as u64)),
        ];
        if row.star_osc != 0 {
            r.push(f("star", e6(row.star_osc)));
        }
        if let Some(a) = row.author {
            r.push(f("author", VsfType::hb(a.to_vec())));
        }
        if let Some((k, t)) = row.reference {
            r.push(f("ref_kind", uint(k as u64)));
            r.push(f("ref_target", e6(t)));
        }
        if let Some((o, secs)) = row.wave {
            r.push(f("wave_outcome", uint(o as u64)));
            r.push(f("wave_secs", uint(secs as u64)));
        }
        // Thumbnails are opaque bytes — one value, never a number per byte.
        if !row.envelope.is_empty() {
            r.push(f("envelope", VsfType::hR(row.envelope.clone())));
        }
        if let Some((k, w, h, ph)) = row.attach {
            r.push(f("attach_kind", uint(k as u64)));
            r.push(f("attach_w", uint(w as u64)));
            r.push(f("attach_h", uint(h as u64)));
            if let Some(ph) = ph {
                r.push(f("attach_preview_hash", VsfType::hb(ph.to_vec())));
            }
        }
        if !row.preview.is_empty() {
            r.push(f("preview", VsfType::hR(row.preview.clone())));
        }
        if let Some(c) = row.control.as_ref() {
            let s = c.slots();
            r.push(f("ctl_kind", uint(s.kind as u64)));
            r.push(f("ctl_sub", uint(s.sub as u64)));
            if let Some(t) = s.ts {
                r.push(f("ctl_ts", e6(t)));
            }
            if let Some(id) = s.id {
                r.push(f("ctl_id", VsfType::hR(id)));
            }
            if let Some(n) = s.nonce {
                r.push(f("ctl_nonce", VsfType::hR(n.to_vec())));
            }
            if let Some(d) = s.dev {
                r.push(f("ctl_dev", VsfType::ke(d.to_vec())));
            }
            if let Some(n) = s.num {
                r.push(f("ctl_num", uint(n)));
            }
            if let Some(t) = s.tag {
                r.push(f("ctl_tag", uint(t as u64)));
            }
            if let Some(v) = s.set {
                r.push(f("ctl_set", uint(v as u64)));
            }
        }
        if let Some(fr) = row.file.as_ref() {
            r.push(f("file_role", uint(fr.role.code() as u64)));
            r.push(f("file_hash", VsfType::hb(fr.hash.to_vec())));
            r.push(f("file_name", VsfType::x(fr.name.clone())));
            r.push(f("file_size", uint(fr.size)));
        }
        doc = doc.add_section(ROW_SECTION, r);
        for m in &row.marks {
            doc = doc.add_section(
                MARK_SECTION,
                vec![f("row", uint(i as u64)), f("kind", uint(m.kind as u64)), f("start", uint(m.start as u64)), f("len", uint(m.len as u64)), f("dest", VsfType::x(m.dest.clone()))],
            );
        }
    }
    // A COMPLETE VSF FILE inside the seal (AGENT.md: "VSF Transport Rule: COMPLETE FILES ONLY"). These pages ride the FLEET key to every sibling device, so "it decrypted" proves only that SOMEONE IN THE FLEET wrote it — the provenance hash is what makes the payload itself self-consistent.
    let doc = doc.build()?;
    kete::encrypt_bytes(&doc, key)
}

/// AEAD-open + decode a page. Fails on wrong key, tamper, or malformed plaintext.
/// Verified read before a single field is trusted; the retired column shape fails here, which is correct: pages are a resyncable cache, so the requester re-fetches once both sides run the record shape. No compat branch.
pub fn open_history_page(sealed: &[u8], key: &[u8; 32]) -> Result<HistoryPagePlain, String> {
    use crate::storage::record::Rec;
    let plain = kete::decrypt_bytes(sealed, key)?;
    let sections = crate::storage::record::verified_sections(&plain, None).map_err(|e| format!("page failed verified read: {e}"))?;
    let head = sections.iter().find(|s| s.name == PAGE_SECTION).map(Rec).ok_or("page has no hist_page section")?;
    let oldest_osc = head.osc("oldest").ok_or("page missing oldest")?;
    let more = head.uint("more").is_some_and(|v| v != 0);
    let flag = |r: &Rec, name: &str| r.uint(name).is_some_and(|v| v != 0);

    let mut rows: Vec<HistoryRow> = Vec::new();
    for r in sections.iter().filter(|s| s.name == ROW_SECTION).map(Rec) {
        // A row without its time or text is torn — dropped alone, never shifting another row's fields onto it.
        let (Some(timestamp), Some(content)) = (r.osc("time"), r.text("text")) else {
            continue;
        };
        let control = r.uint("ctl_kind").and_then(|k| {
            crate::types::RowControl::from_slots(&crate::types::ControlSlots {
                kind: u8::try_from(k).ok()?,
                sub: r.uint("ctl_sub").and_then(|v| u8::try_from(v).ok()).unwrap_or(0),
                ts: r.osc("ctl_ts"),
                id: r.bytes("ctl_id"),
                nonce: r.bytes("ctl_nonce").and_then(|b| b.try_into().ok()),
                dev: r.key32("ctl_dev"),
                num: r.uint("ctl_num"),
                tag: r.uint("ctl_tag").and_then(|v| u32::try_from(v).ok()),
                set: r.uint("ctl_set").and_then(|v| u8::try_from(v).ok()),
            })
        });
        let file = (|| {
            Some(crate::types::AttachRef {
                role: crate::types::AttachRole::from_code(u8::try_from(r.uint("file_role")?).ok()?)?,
                hash: r.h32("file_hash")?,
                name: r.text("file_name")?,
                size: r.uint("file_size")?,
            })
        })();
        let attach = (|| {
            let k = u8::try_from(r.uint("attach_kind")?).ok()?;
            // WHY/PROOF: pixel dimensions are u32 on write — a wider value from a peer's page reads as unknown (0), never wrapped into a small size.
            let dim = |name: &str| r.uint(name).and_then(|v| u32::try_from(v).ok()).unwrap_or(0);
            Some((k, dim("attach_w"), dim("attach_h"), r.h32("attach_preview_hash")))
        })();
        rows.push(HistoryRow {
            timestamp,
            content,
            sender_outgoing: flag(&r, "out"),
            delivered: flag(&r, "delivered"),
            deleted: flag(&r, "deleted"),
            // History never re-dings: a row that does not say otherwise is notified.
            notified: r.uint("notified").is_none_or(|v| v != 0),
            star_osc: r.osc("star").unwrap_or(0),
            author: r.h32("author"),
            reference: r.uint("ref_kind").and_then(|k| Some((u8::try_from(k).ok()?, r.osc("ref_target")?))),
            marks: Vec::new(),
            wave: r.uint("wave_outcome").and_then(|o| Some((u8::try_from(o).ok()?, u32::try_from(r.uint("wave_secs")?).ok()?))),
            envelope: r.bytes("envelope").unwrap_or_default(),
            attach,
            preview: r.bytes("preview").filter(|p| p.len() <= crate::types::MICRO_PREVIEW_MAX_BYTES).unwrap_or_default(),
            control,
            file,
        });
    }
    // Marks name their row by index; each row's marks re-validate against its own content.
    let mut marks: Vec<Vec<crate::types::MessageMark>> = vec![Vec::new(); rows.len()];
    for m in sections.iter().filter(|s| s.name == MARK_SECTION).map(Rec) {
        let (Some(row), Some(kind), Some(start), Some(len), Some(dest)) = (m.uint("row"), m.uint("kind"), m.uint("start"), m.uint("len"), m.text("dest")) else {
            continue;
        };
        let (Ok(row), Ok(kind), Ok(start), Ok(len)) = (usize::try_from(row), u8::try_from(kind), usize::try_from(start), usize::try_from(len)) else {
            continue;
        };
        // WHY/PROOF: a peer's page names the row — an index past the page's rows is a malformed mark, dropped.
        if let Some(v) = marks.get_mut(row) {
            v.push(crate::types::MessageMark { kind, start, len, dest });
        }
    }
    for (row, m) in rows.iter_mut().zip(marks) {
        row.marks = crate::types::valid_marks(&row.content, &m);
    }
    Ok(HistoryPagePlain { rows, oldest_osc, more })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_page() -> HistoryPagePlain {
        HistoryPagePlain {
            rows: vec![
                HistoryRow {
                    control: None,
                    file: None,
                    author: Some([0xAB; 32]),
                    star_osc: 777,
                    timestamp: 1_000,
                    content: "oldest in page 👋 unicode".to_string(),
                    sender_outgoing: true,
                    delivered: true,
                    deleted: false,
                    reference: None,
                    // A link mark rides the page: range covers "oldest " — validation checks bounds + dest scheme, and the round trip must return it intact.
                    marks: vec![crate::types::MessageMark { kind: 1, start: 0, len: 7, dest: "https://x.example/".into() }],
                    wave: Some((4, 61)),
                    envelope: (0..288u32).map(|b| (b % 256) as u8).collect(),
                    // Typed attachment columns ride too: kind, dims, a preview-blob hash and a micro preview, all back byte-exact.
                    attach: Some((2, 6000, 4000, Some([9u8; 32]))),
                    preview: vec![3, 2, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18],
                    notified: true,
                },
                HistoryRow {
                    control: None,
                    file: None,
                    author: None,
                    star_osc: 0,
                    timestamp: 2_000,
                    content: "".to_string(), // empty content is a legal row
                    sender_outgoing: false,
                    delivered: false,
                    deleted: false,
                    reference: Some((1, 1_000)), // a reply column rides the page
                    marks: Vec::new(),
                    wave: None,
                    envelope: Vec::new(),
                    attach: None,
                    preview: Vec::new(),
                    notified: true,
                },
                HistoryRow {
                    control: None,
                    file: None,
                    author: None,
                    star_osc: 0,
                    timestamp: 3_000,
                    content: "newest".to_string(),
                    sender_outgoing: true,
                    delivered: false,
                    deleted: false,
                    reference: None,
                    marks: Vec::new(),
                    wave: None,
                    envelope: Vec::new(),
                    attach: None,
                    preview: Vec::new(),
                    notified: true,
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

    /// Control and attachment rows ride as typed columns: a page mixing every control kind, attachment roles and plain text round-trips exactly, and each row's slots stay attached to that row (the per-column cursors never drift).
    #[test]
    fn control_and_file_rows_ride_typed_columns() {
        use crate::types::{AttachRef, AttachRole, RowControl};
        use crate::wave::signal::WaveSignal;
        let plain = |t: i64, text: &str| HistoryRow::from_message(&crate::types::ChatMessage::new_with_timestamp(text.to_string(), true, t));
        let ctl = |t: i64, c: RowControl| HistoryRow::from_message(&crate::types::ChatMessage::control(c, false, t));
        let file = |t: i64, f: AttachRef| HistoryRow::from_message(&crate::types::ChatMessage::attachment(f, true, t));
        let rows = vec![
            plain(1, "before"),
            ctl(2, RowControl::Wave(WaveSignal::Offer { wave_id: [1; 16], nonce: [2; 32], device: Some([3; 32]) })),
            file(3, AttachRef::file([4; 32], "notes.txt", 12)),
            ctl(4, RowControl::Delete { target: 3 }),
            ctl(5, RowControl::Wave(WaveSignal::Hangup { wave_id: [1; 16] })),
            file(6, AttachRef { hash: [5; 32], name: String::new(), size: 99, role: AttachRole::WaveAudio }),
            ctl(7, RowControl::Era(crate::crypto::era::EraSignal::Init { era_next: 2, nonce: [6; 32], prior_tag: 7, kem_set: 3 })),
            ctl(8, RowControl::Probe),
            ctl(9, RowControl::Wave(WaveSignal::Answer { wave_id: [8; 16], nonce: [9; 32], device: None })),
            plain(10, "after"),
        ];
        let page = HistoryPagePlain { rows, oldest_osc: 1, more: false };
        let key = [0x11u8; 32];
        let opened = open_history_page(&seal_history_page(&page, &key).unwrap(), &key).unwrap();
        assert_eq!(opened, page);
        assert!(opened.rows.iter().filter(|r| r.control.is_some() || r.file.is_some()).all(|r| r.content.is_empty()), "typed rows carry no text");
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
                control: None,
                file: None,
                author: None,
                star_osc: 0,
                timestamp: 7,
                content: "hi".to_string(),
                sender_outgoing: true,
                delivered: false,
                deleted: false,
                reference: None,
                marks: Vec::new(),
                wave: None,
                envelope: Vec::new(),
                attach: None,
                preview: Vec::new(),
                notified: true,
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
        // The head section's bytes alone, cut out of a real page by its TOC entry — a headerless blob.
        let plain = kete::decrypt_bytes(&seal_history_page(&sample_page(), &key).expect("seal"), &key).expect("open");
        let (header, _) = vsf::verification::read_verified(&plain, None).expect("verifies");
        let f = header.fields.iter().find(|f| f.name == PAGE_SECTION).expect("head in the TOC");
        let bare = plain[f.offset_bytes..f.offset_bytes + f.size_bytes].to_vec();
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
