//! Rich message body — an ordered list of typed SPANS (the toka-bytecode / rich-format direction pinned in TICKETS.md).
//! Stage 1 (this file): the VSF span codec + scheme validation + KATs. No wire/storage/render yet — those are later stages.
//!
//! ## Links are NEVER inferred
//! A link span is only ever constructed by an EXPLICIT user action (Nick 2026-09-02): typing `passless.org` stays a plain text run forever. The compose UI (Stage 4) may SUGGEST converting a URL-looking run to a link, but nothing auto-resolves it — if someone wants a literal, dead `passless.org` in their message, that's their choice. This codec only encodes exactly the spans it is handed.
//!
//! ## The plaintext rides inside — no braid flag day
//! Every body carries its PLAINTEXT (the concatenation of each run's display text) as a bare VSF `x` element FIRST (Nick 2026-09-02: "backfill the regular vsf text type x with the text as well … could use that for the weave too"). Two payoffs:
//!  - A legacy or link-unaware reader (search, notification preview, an old build) reads that one `x` and has the whole message — nice and plaintexted.
//!  - The braid weaves message content into future keys; if it weaves the PLAINTEXT (not the rich framing), a plain message and its rich-framed twin weave IDENTICALLY, so adding link spans is not a chain flag day at all.
//! The load-bearing consequence, asserted by [`tests::plain_body_is_a_bare_x`]: a plain-text body encodes to the EXACT bytes of `VsfType::x(text)` — byte-identical to how a legacy flat-string row already serializes. The migration is free.
//!
//! ## Canonical
//! One `MessageBody` value has exactly one encoding: adjacent text runs are merged and empty text dropped ([`MessageBody::normalized`]), so a body with no link is always the bare-`x` form and a body with links is `x` + `u(count)` + runs. Determinism is what keeps both ends (and the weave) in agreement.

use vsf::VsfType;

/// One run of a message body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Span {
    /// Plain text.
    Text(String),
    /// A hyperlink. `href` is a validated https/mailto target; `text` is what's shown (which MAY differ from the href — the render-time confirm sheet shows the true href as the anti-spoof).
    Link { href: String, text: String },
    /// An INLINE attachment — the same blob an attachment row carries, but riding in a rich body interleaved with text/links. `hash` is the BLAKE3 content-hash (the blob key, reused verbatim by [`crate::storage::blob_load`] and the PT transfer); `name`/`size` are the filename + byte length. Its plaintext contribution is the FILENAME, so a legacy reader shows a readable `photo.jpg` inline and the woven bytes stay stable. Single-attachment and `call.audio` rows keep the legacy marker path ([`crate::types::attachment_content`]); this variant is only for the interleaved case.
    Attachment { hash: [u8; 32], name: String, size: u64 },
}

/// Run-kind tags on the wire. Additive: a new span kind takes the next integer, and an old reader that hits an unknown kind fails the decode loudly rather than guessing (see [`MessageBody::decode`]).
const KIND_TEXT: u64 = 0;
const KIND_LINK: u64 = 1;
const KIND_ATTACHMENT: u64 = 2;

impl Span {
    /// The display string of this run (what a link-unaware surface shows, and what feeds the plaintext + weave). An attachment's display is its filename.
    pub fn display(&self) -> &str {
        match self {
            Span::Text(t) => t,
            Span::Link { text, .. } => text,
            Span::Attachment { name, .. } => name,
        }
    }

    /// Construct a link span IFF `href` is an allowed scheme (https or mailto). Returns `None` otherwise — an `http`/`file`/`javascript`/`data` URL never becomes a link span, so a `Link` at rest is ALWAYS safe to hand to the opener. An empty display defaults to the href.
    pub fn link(href: impl Into<String>, text: impl Into<String>) -> Option<Span> {
        let href = href.into();
        if !href_allowed(&href) {
            return None;
        }
        let mut text = text.into();
        if text.is_empty() {
            text = href.clone();
        }
        Some(Span::Link { href, text })
    }
}

/// The scheme allowlist — `https://` and `mailto:` only (Nick's call 2026-09-02). Case-insensitive on the scheme, byte-verbatim on the rest; a bare scheme with no target is rejected. `http`, `file`, `javascript`, `data`, custom schemes: all refused, so a link field can never smuggle a dangerous target past the type.
pub fn href_allowed(href: &str) -> bool {
    let lower = href.to_ascii_lowercase();
    (lower.starts_with("https://") && href.len() > "https://".len())
        || (lower.starts_with("mailto:") && href.len() > "mailto:".len())
}

/// A message body: an ordered list of spans. `Default` is empty (renders as nothing).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct MessageBody {
    pub spans: Vec<Span>,
}

impl MessageBody {
    /// A plain-text body — one text span. This is what a legacy flat-string row becomes on read.
    pub fn plain(text: impl Into<String>) -> Self {
        Self { spans: vec![Span::Text(text.into())] }
    }

    /// The plaintext — every run's display text, concatenated. The legacy/preview view AND the braid weave source.
    pub fn plaintext(&self) -> String {
        let mut s = String::new();
        for span in &self.spans {
            s.push_str(span.display());
        }
        s
    }

    /// Whether any run carries something the plaintext doesn't — a link or an attachment. A pure-text body has none, and encodes to the bare-`x` form.
    pub fn has_rich(&self) -> bool {
        self.spans.iter().any(|s| !matches!(s, Span::Text(_)))
    }

    /// Canonicalize: merge adjacent text runs, drop empty text runs. One `MessageBody` value → one span vector → one encoding (the determinism the weave and cross-device agreement need).
    pub fn normalized(&self) -> MessageBody {
        let mut out: Vec<Span> = Vec::with_capacity(self.spans.len());
        for span in &self.spans {
            match span {
                Span::Text(t) if t.is_empty() => {}
                Span::Text(t) => {
                    if let Some(Span::Text(prev)) = out.last_mut() {
                        prev.push_str(t);
                    } else {
                        out.push(Span::Text(t.clone()));
                    }
                }
                Span::Link { .. } | Span::Attachment { .. } => out.push(span.clone()),
            }
        }
        MessageBody { spans: out }
    }

    /// Canonical VSF encoding. Plaintext-first, always:
    ///  - a body with NO link → the bare `x(plaintext)` element, byte-identical to a legacy flat-string row (the free migration — see the module docs).
    ///  - a body WITH a link → `x(plaintext)` then `u(run_count)` then, per run, `u(kind)` + typed fields.
    /// Runs the [`normalized`](Self::normalized) form first so the output is deterministic.
    pub fn encode(&self) -> Vec<u8> {
        let body = self.normalized();
        let mut out = VsfType::x(body.plaintext()).flatten();
        if !body.has_rich() {
            return out; // bare plaintext — no runs section
        }
        out.extend(VsfType::u(body.spans.len(), false).flatten());
        for span in &body.spans {
            match span {
                Span::Text(t) => {
                    out.extend(VsfType::u(KIND_TEXT as usize, false).flatten());
                    out.extend(VsfType::x(t.clone()).flatten());
                }
                Span::Link { href, text } => {
                    out.extend(VsfType::u(KIND_LINK as usize, false).flatten());
                    out.extend(VsfType::x(href.clone()).flatten());
                    out.extend(VsfType::x(text.clone()).flatten());
                }
                Span::Attachment { hash, name, size } => {
                    out.extend(VsfType::u(KIND_ATTACHMENT as usize, false).flatten());
                    out.extend(VsfType::hb(hash.to_vec()).flatten());
                    out.extend(VsfType::x(name.clone()).flatten());
                    out.extend(VsfType::u(*size as usize, false).flatten());
                }
            }
        }
        out
    }

    /// Decode a body blob. `None` on a malformed/truncated element or an unknown run kind — never a guess (a body that can't be read cleanly is an error, not a best-effort render).
    /// A link run whose href fails validation is DEGRADED to a text run of its display: at-rest bytes are untrusted, so a `Link` we return is always openable-safe, and a poisoned href can never reach the opener.
    /// A bare `x` (no runs section) is a plain-text body — the legacy/minimal form.
    pub fn decode(bytes: &[u8]) -> Option<MessageBody> {
        let mut ptr = 0usize;
        let plaintext = match vsf::parse(bytes, &mut ptr).ok()? {
            VsfType::x(s) | VsfType::a(s) => s,
            _ => return None, // the body MUST lead with its plaintext
        };
        if ptr == bytes.len() {
            // Bare plaintext — one text span (empty string → empty body).
            return Some(if plaintext.is_empty() {
                MessageBody::default()
            } else {
                MessageBody::plain(plaintext)
            });
        }
        let count = vsf::parse(bytes, &mut ptr).ok()?.as_u64()?;
        let mut spans = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let kind = vsf::parse(bytes, &mut ptr).ok()?.as_u64()?;
            match kind {
                KIND_TEXT => {
                    let t = parse_x(bytes, &mut ptr)?;
                    spans.push(Span::Text(t));
                }
                KIND_LINK => {
                    let href = parse_x(bytes, &mut ptr)?;
                    let text = parse_x(bytes, &mut ptr)?;
                    // Untrusted href → degrade to text; never surface an un-vetted link.
                    match Span::link(href, text.clone()) {
                        Some(s) => spans.push(s),
                        None => spans.push(Span::Text(text)),
                    }
                }
                KIND_ATTACHMENT => {
                    let hash = parse_hash(bytes, &mut ptr)?;
                    let name = parse_x(bytes, &mut ptr)?;
                    let size = vsf::parse(bytes, &mut ptr).ok()?.as_u64()?;
                    spans.push(Span::Attachment { hash, name, size });
                }
                _ => return None, // unknown run kind — malformed, not guessed
            }
        }
        if ptr != bytes.len() {
            return None; // trailing bytes — malformed
        }
        Some(MessageBody { spans }.normalized())
    }
}

/// Parse one VSF element and require it to be text (`x`/`a`).
fn parse_x(bytes: &[u8], ptr: &mut usize) -> Option<String> {
    match vsf::parse(bytes, ptr).ok()? {
        VsfType::x(s) | VsfType::a(s) => Some(s),
        _ => None,
    }
}

/// Parse one VSF element and require it to be a 32-byte hash (`hb`).
fn parse_hash(bytes: &[u8], ptr: &mut usize) -> Option<[u8; 32]> {
    match vsf::parse(bytes, ptr).ok()? {
        VsfType::hb(b) if b.len() == 32 => b.try_into().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn href_allowlist() {
        assert!(href_allowed("https://passless.org/"));
        assert!(href_allowed("mailto:hello@passless.org"));
        assert!(!href_allowed("http://passless.org/")); // http is not https
        assert!(!href_allowed("https://")); // bare scheme, no target
        assert!(!href_allowed("javascript:alert(1)"));
        assert!(!href_allowed("file:///etc/passwd"));
        assert!(!href_allowed("passless.org")); // schemeless never auto-qualifies
        // Case-insensitive scheme.
        assert!(href_allowed("HTTPS://passless.org/"));
        // A link span refuses a bad scheme at construction.
        assert!(Span::link("http://x", "x").is_none());
        assert_eq!(
            Span::link("https://passless.org/", ""),
            Some(Span::Link {
                href: "https://passless.org/".into(),
                text: "https://passless.org/".into() // empty display defaults to href
            })
        );
    }

    #[test]
    fn plain_body_is_a_bare_x() {
        // THE migration invariant: a plain body's bytes are byte-identical to a legacy flat-string row's `x` encoding, so weaving the plaintext means adding link framing is not a braid flag day.
        let s = "see passless.org, it's good";
        assert_eq!(MessageBody::plain(s).encode(), VsfType::x(s.to_string()).flatten());
    }

    #[test]
    fn plain_round_trip() {
        let b = MessageBody::plain("hello world");
        let wire = b.encode();
        assert_eq!(MessageBody::decode(&wire), Some(b));
        // Empty.
        assert_eq!(MessageBody::decode(&MessageBody::default().encode()), Some(MessageBody::default()));
    }

    #[test]
    fn rich_round_trip() {
        let b = MessageBody {
            spans: vec![
                Span::Text("learn more at ".into()),
                Span::link("https://passless.org/", "passless").unwrap(),
                Span::Text(" today".into()),
            ],
        };
        let wire = b.encode();
        let back = MessageBody::decode(&wire).unwrap();
        assert_eq!(back, b);
        // The plaintext threads through unchanged (the weave/legacy view).
        assert_eq!(back.plaintext(), "learn more at passless today");
        // And a rich body's plaintext element is still the FIRST thing on the wire — a legacy x-reader gets it.
        let mut ptr = 0;
        let lead = match vsf::parse(&wire, &mut ptr).unwrap() {
            VsfType::x(s) | VsfType::a(s) => s,
            _ => panic!("body must lead with its plaintext"),
        };
        assert_eq!(lead, "learn more at passless today");
    }

    #[test]
    fn attachment_round_trip() {
        let hash = [0x5au8; 32];
        let b = MessageBody {
            spans: vec![
                Span::Text("here's the deck ".into()),
                Span::Attachment { hash, name: "slides.pdf".into(), size: 2_097_152 },
                Span::Text(" and a link ".into()),
                Span::link("https://passless.org/", "passless").unwrap(),
            ],
        };
        let wire = b.encode();
        assert_eq!(MessageBody::decode(&wire), Some(b.clone()));
        // Plaintext = every run's display concatenated, attachment contributing its filename.
        assert_eq!(b.plaintext(), "here's the deck slides.pdf and a link passless");
        // An attachment makes the body rich (carries a hash the plaintext doesn't).
        assert!(b.has_rich());
        // And the plaintext still leads the wire for a legacy reader.
        let mut ptr = 0;
        let lead = match vsf::parse(&wire, &mut ptr).unwrap() {
            VsfType::x(s) | VsfType::a(s) => s,
            _ => panic!("body must lead with its plaintext"),
        };
        assert_eq!(lead, "here's the deck slides.pdf and a link passless");
    }

    #[test]
    fn attachment_truncated_is_none() {
        // A KIND_ATTACHMENT run missing its size field is malformed, not a lesser span.
        let mut wire = VsfType::x("f".to_string()).flatten();
        wire.extend(VsfType::u(1usize, false).flatten());
        wire.extend(VsfType::u(KIND_ATTACHMENT as usize, false).flatten());
        wire.extend(VsfType::hb([1u8; 32].to_vec()).flatten());
        wire.extend(VsfType::x("f".to_string()).flatten());
        // (no size element)
        assert_eq!(MessageBody::decode(&wire), None);
    }

    #[test]
    fn canonical_merges_adjacent_text() {
        let split = MessageBody { spans: vec![Span::Text("ab".into()), Span::Text("cd".into())] };
        let merged = MessageBody::plain("abcd");
        assert_eq!(split.encode(), merged.encode()); // one value → one encoding
        assert_eq!(split.normalized(), merged);
    }

    #[test]
    fn poisoned_href_degrades_to_text_on_decode() {
        // Hand-craft a link run carrying a disallowed href (a malicious/corrupt blob a trusted encoder would never emit).
        let mut wire = VsfType::x("click me".to_string()).flatten();
        wire.extend(VsfType::u(1usize, false).flatten());
        wire.extend(VsfType::u(KIND_LINK as usize, false).flatten());
        wire.extend(VsfType::x("javascript:alert(1)".to_string()).flatten());
        wire.extend(VsfType::x("click me".to_string()).flatten());
        let back = MessageBody::decode(&wire).unwrap();
        // Degraded to plain text — no Link reaches the caller.
        assert_eq!(back, MessageBody::plain("click me"));
    }

    #[test]
    fn malformed_is_none_not_a_guess() {
        assert_eq!(MessageBody::decode(&[]), None); // no leading plaintext
        // Unknown run kind.
        let mut wire = VsfType::x("x".to_string()).flatten();
        wire.extend(VsfType::u(1usize, false).flatten());
        wire.extend(VsfType::u(99usize, false).flatten()); // bogus kind
        wire.extend(VsfType::x("x".to_string()).flatten());
        assert_eq!(MessageBody::decode(&wire), None);
    }
}
