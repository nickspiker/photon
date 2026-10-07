//! THE ATTACHMENT HEAD (flag day 2026-10-07; docs/attachments.md "Target shape"). Nick: *"keep the chain simply text, text formatting, links, pointers to waves and beams and pigeons and other blobs… store the pdf or tiff or whatever in a wrapped vsf blob with a thumbnail but I'd keep that off chain."*
//!
//! A file photon on the chain is two pointers: the ORIGINAL's hash (byte-exact, chunked, what Save exports and what de-duplication and arrival verification key on) and this HEAD's hash. The head is a small off-chain VSF blob holding everything a row used to carry inline — the sender's filename, the sniffed kind, the pixel or page dims, the micro thumb and the AV1 preview — so a conversation opens on text and pointers alone and each row fills in when its head is read (held) or fetched (pushed ahead of the original by the sender, replicated on any network: a head is a few KB).
//!
//! Rows minted before the flag day carry no head and keep their inline fields; both shapes render through the same row fields, because a head HYDRATES its rows at runtime (`attach`, `preview`, `head_name`) and the writers never persist or transmit hydrated fields on a headed row.

use super::attach_kind::{AttachKind, AttachMeta};

/// The VSF section a head lives in.
const SECTION: &str = "attach_head";

/// Everything about an attachment except its bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AttachHead {
    /// The sender's filename (may be empty — images and audio have travelled nameless).
    pub name: String,
    pub kind: AttachKind,
    /// Pixel dims for an image, the page size in points for a PDF.
    pub dims: Option<(u32, u32)>,
    /// The micro tier (≤24-px gamma-2 VSF RGB thumb, or the first bytes of a text file).
    pub micro: Vec<u8>,
    /// The preview tier: the AV1-in-VSF image bytes, whole (never a pointer to another blob).
    pub preview: Option<Vec<u8>>,
}

impl AttachHead {
    /// The head as a complete VSF document (the transport and the vault both want whole files).
    pub fn encode(&self) -> Option<Vec<u8>> {
        let mut sec = vsf::file_format::VsfSection::new(SECTION);
        sec.add_field("name", vsf::VsfType::x(self.name.clone()));
        sec.add_field("kind", vsf::VsfType::u(self.kind as usize, false));
        if let Some((w, h)) = self.dims {
            sec.add_field("w", vsf::VsfType::u(w as usize, false));
            sec.add_field("h", vsf::VsfType::u(h as usize, false));
        }
        if !self.micro.is_empty() {
            sec.add_field("micro", vsf::VsfType::hR(self.micro.clone()));
        }
        if let Some(p) = self.preview.as_ref() {
            sec.add_field("preview", vsf::VsfType::hR(p.clone()));
        }
        vsf::VsfBuilder::new()
            .creation_time_oscillations(vsf::eagle_time_oscillations())
            .add_section_direct(sec)
            .build()
            .ok()
    }

    /// Read a head (schema-validated: the VSF trust gate). None for bytes that are not a head — a pre-flag-day preview blob, a corrupt value.
    pub fn parse(bytes: &[u8]) -> Option<AttachHead> {
        use vsf::schema::{SectionBuilder, SectionSchema, TypeConstraint};
        let schema = SectionSchema::new(SECTION)
            .field("name", TypeConstraint::Utf8Text)
            .field("kind", TypeConstraint::AnyUnsigned)
            .field("w", TypeConstraint::AnyUnsigned)
            .field("h", TypeConstraint::AnyUnsigned)
            .field("micro", TypeConstraint::Any)
            .field("preview", TypeConstraint::Any);
        let sec = SectionBuilder::parse_document(schema, bytes, None).ok()?;
        let first = |n: &str| sec.get_fields(n).first().and_then(|f| f.values.first()).cloned();
        let bytes_of = |n: &str| match first(n) {
            Some(vsf::VsfType::hR(b)) | Some(vsf::VsfType::hb(b)) => Some(b),
            _ => None,
        };
        let name = match first("name") {
            Some(vsf::VsfType::x(s)) => s,
            _ => String::new(),
        };
        let kind = first("kind").and_then(|v| v.as_u64()).and_then(|k| u8::try_from(k).ok()).and_then(AttachKind::from_wire).unwrap_or(AttachKind::Unknown);
        // WHY/PROOF: dims are the sender's claim — a width or height past u32 reads as unknown, never wrapped.
        let dim = |n: &str| first(n).and_then(|v| v.as_u64()).and_then(|v| u32::try_from(v).ok());
        let dims = match (dim("w"), dim("h")) {
            (Some(w), Some(h)) if w > 0 && h > 0 => Some((w, h)),
            _ => None,
        };
        Some(AttachHead { name, kind, dims, micro: bytes_of("micro").unwrap_or_default(), preview: bytes_of("preview") })
    }

    /// The row metadata this head hydrates: kind and dims, with the preview addressed at the HEAD itself (the preview bytes live inside it).
    pub fn meta(&self, head_hash: [u8; 32]) -> AttachMeta {
        AttachMeta { kind: self.kind, dims: self.dims, preview_hash: self.preview.as_ref().map(|_| head_hash) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_head_round_trips() {
        let h = AttachHead { name: "provisional.pdf".into(), kind: AttachKind::Document, dims: Some((612, 792)), micro: vec![1, 2, 3], preview: Some(vec![9; 40]) };
        let bytes = h.encode().expect("encodes");
        assert_eq!(AttachHead::parse(&bytes), Some(h.clone()));
        let meta = h.meta([5u8; 32]);
        assert_eq!(meta.preview_hash, Some([5u8; 32]), "the preview is addressed at the head");
        let bare = AttachHead { name: String::new(), kind: AttachKind::Archive, ..Default::default() };
        assert_eq!(AttachHead::parse(&bare.encode().unwrap()), Some(bare));
        assert_eq!(AttachHead::parse(b"not a head"), None);
    }
}
