//! Compose DRAFTS — what the human typed into a conversation's compose bar and has not sent (Nick 2026-09-25: "trackable structures for each conversation … when you navigate away or let it sit for more than 60s it writes to disk").
//!
//! Device-local by design: a draft is this device's unfinished sentence, never replicated to the fleet.
//! One vault entry holds every draft, at `vault_key("drafts", vault_seed)` — a complete VSF document with one `draft` section per conversation and one `draft_link` section per tagged link, each link naming its conversation by field.
//! Repeated same-name sections are first-class in VSF, so a draft's links are records of their own, never parallel columns.

use crate::storage::{FlatStorage, StorageError};
use crate::types::ConversationId;
use vsf::VsfType;

const DRAFT_SECTION: &str = "draft";
const LINK_SECTION: &str = "draft_link";

/// A tagged link in a draft: `start..end` in CHAR indices of the text, and where it points.
/// App-detected bare URLs are not stored — the text is their destination, and they are re-detected on restore.
#[derive(Clone, Debug, PartialEq)]
pub struct DraftLink {
    pub start: usize,
    pub end: usize,
    pub dest: String,
}

/// One conversation's unsent compose state.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Draft {
    pub text: String,
    pub links: Vec<DraftLink>,
    /// The row an armed reply, edit or reaction targets — the compose bar's mode is part of what was typed.
    pub reply_to: Option<i64>,
    pub edit_of: Option<i64>,
    pub react_to: Option<i64>,
}

impl Draft {
    /// Nothing worth keeping: no text and no armed mode. An empty draft is deleted, never stored.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.reply_to.is_none() && self.edit_of.is_none() && self.react_to.is_none()
    }
}

fn drafts_addr(storage: &FlatStorage) -> [u8; 32] {
    crate::storage::vault_key("drafts", &storage.vault_seed())
}

fn e6(osc: i64) -> VsfType {
    VsfType::e(vsf::types::EtType::e6(osc))
}

/// Encode every draft as one complete VSF document (header, provenance hash, one section per record).
pub fn drafts_to_vsf_bytes(drafts: &[(ConversationId, Draft)]) -> Result<Vec<u8>, StorageError> {
    let mut b = vsf::VsfBuilder::new().creation_time_oscillations(vsf::eagle_time_oscillations()).provenance_only();
    for (conv, d) in drafts {
        let mut fields: Vec<(String, VsfType)> = vec![
            ("conv".into(), VsfType::hb(conv.as_bytes().to_vec())),
            ("text".into(), VsfType::x(d.text.clone())),
        ];
        if let Some(t) = d.reply_to {
            fields.push(("reply_to".into(), e6(t)));
        }
        if let Some(t) = d.edit_of {
            fields.push(("edit_of".into(), e6(t)));
        }
        if let Some(t) = d.react_to {
            fields.push(("react_to".into(), e6(t)));
        }
        b = b.add_section(DRAFT_SECTION, fields);
        for l in &d.links {
            b = b.add_section(
                LINK_SECTION,
                vec![
                    ("conv".into(), VsfType::hb(conv.as_bytes().to_vec())),
                    ("start".into(), VsfType::u(l.start, false)),
                    ("end".into(), VsfType::u(l.end, false)),
                    ("dest".into(), VsfType::x(l.dest.clone())),
                ],
            );
        }
    }
    b.build().map_err(StorageError::Parse)
}

/// Decode the drafts document — a verified read; a link whose conversation has no draft, or whose range falls outside its text, is dropped.
pub fn drafts_from_vsf_bytes(bytes: &[u8]) -> Result<Vec<(ConversationId, Draft)>, StorageError> {
    let (header, header_end) = vsf::verification::read_verified(bytes, None).map_err(|e| StorageError::Parse(format!("drafts failed verified read: {e}")))?;
    let sections = header.sections(bytes, header_end).map_err(|e| StorageError::Parse(format!("drafts sections: {e}")))?;
    let first = |s: &vsf::file_format::VsfSection, name: &str| -> Option<VsfType> { s.get_fields(name).first().and_then(|f| f.values.first()).cloned() };
    let conv_of = |s: &vsf::file_format::VsfSection| -> Option<ConversationId> {
        match first(s, "conv")? {
            VsfType::hb(b) => <[u8; 32]>::try_from(b.as_slice()).ok().map(ConversationId::from_bytes),
            _ => None,
        }
    };
    let text_of = |s: &vsf::file_format::VsfSection, name: &str| -> Option<String> {
        match first(s, name)? {
            VsfType::x(t) => Some(t),
            _ => None,
        }
    };
    // Width-agnostic numeral reads: the writer auto-sizes, so a parsed integer never variant-matches its written width.
    let uint_of = |s: &vsf::file_format::VsfSection, name: &str| -> Option<usize> { first(s, name)?.as_usize() };
    let osc_of = |s: &vsf::file_format::VsfSection, name: &str| -> Option<i64> {
        match first(s, name)? {
            VsfType::e(vsf::types::EtType::e6(o)) => Some(o),
            other => other.as_i64(),
        }
    };
    let mut out: Vec<(ConversationId, Draft)> = Vec::new();
    for s in sections.iter().filter(|s| s.name == DRAFT_SECTION) {
        let (Some(conv), Some(text)) = (conv_of(s), text_of(s, "text")) else {
            continue;
        };
        let d = Draft {
            text,
            links: Vec::new(),
            reply_to: osc_of(s, "reply_to"),
            edit_of: osc_of(s, "edit_of"),
            react_to: osc_of(s, "react_to"),
        };
        out.push((conv, d));
    }
    for s in sections.iter().filter(|s| s.name == LINK_SECTION) {
        let (Some(conv), Some(start), Some(end), Some(dest)) = (conv_of(s), uint_of(s, "start"), uint_of(s, "end"), text_of(s, "dest")) else {
            continue;
        };
        let Some((_, d)) = out.iter_mut().find(|(c, _)| *c == conv) else {
            continue;
        };
        if start < end && end <= d.text.chars().count() {
            d.links.push(DraftLink { start, end, dest });
        }
    }
    Ok(out)
}

/// Write every draft (or delete the entry when there are none).
pub fn save_drafts(drafts: &[(ConversationId, Draft)], storage: &FlatStorage) -> Result<(), StorageError> {
    if drafts.is_empty() {
        return storage.delete_addr(&drafts_addr(storage));
    }
    storage.write_addr(&drafts_addr(storage), &drafts_to_vsf_bytes(drafts)?)
}

/// Every stored draft; none stored reads as an empty list.
pub fn load_drafts(storage: &FlatStorage) -> Result<Vec<(ConversationId, Draft)>, StorageError> {
    match storage.read_addr(&drafts_addr(storage))? {
        Some(bytes) => drafts_from_vsf_bytes(&bytes),
        None => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drafts round-trip whole — text, caret, armed modes and each link on its own conversation — and a link never crosses to another conversation's draft.
    #[test]
    fn drafts_round_trip_with_links_on_their_own_conversation() {
        let a = ConversationId::from_bytes([1; 32]);
        let b = ConversationId::from_bytes([2; 32]);
        let drafts = vec![
            (
                a,
                Draft {
                    text: "see the docs here — ok".into(),
                    links: vec![DraftLink { start: 8, end: 12, dest: "https://example.org/docs".into() }],
                    reply_to: Some(2_561_124_741_904_843_777),
                    edit_of: None,
                    react_to: None,
                },
            ),
            (b, Draft { text: "\t".into(), links: Vec::new(), reply_to: None, edit_of: Some(7), react_to: None }),
        ];
        let bytes = drafts_to_vsf_bytes(&drafts).unwrap();
        assert_eq!(drafts_from_vsf_bytes(&bytes).unwrap(), drafts);
    }

    /// A tampered document is refused whole — the provenance hash covers every section.
    #[test]
    fn tampered_drafts_are_refused() {
        let drafts = vec![(ConversationId::from_bytes([3; 32]), Draft { text: "hello".into(), ..Default::default() })];
        let mut bytes = drafts_to_vsf_bytes(&drafts).unwrap();
        let n = bytes.len();
        bytes[n - 3] ^= 0x01;
        assert!(drafts_from_vsf_bytes(&bytes).is_err());
    }
}
