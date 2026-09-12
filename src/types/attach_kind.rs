//! Attachment KINDS (2026-09-10, the typed-attachments round): what a file IS, sniffed from its bytes — never trusted from a name or from the wire.
//! The kind decides the bubble's glyph, whether a preview exists, and what a tap does (image → viewer, text → reader, everything else → save). The bytes themselves are untouched by all of it: an attachment travels byte-exact, and the only thing photon ever encodes is its own house-format preview.
//! A receiver re-sniffs at install and keeps the stricter verdict, so a peer can never dress a program up as a picture.

/// What an attachment is. Wire values are stable (page columns, vault fields, the friend package); `Unknown` is the pre-feature default and the honest answer for bytes nothing recognizes.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachKind {
    Unknown = 0,
    /// A display-referred image the `image` crate (or jxl-oxide) decodes: JPEG, PNG, WebP, TIFF, GIF, JXL, BMP.
    Image = 1,
    /// A sensor-truth image: camera RAW, DNG, or a VSF spectral image — native-depth counts, decoded thru limbus/opsin, never re-encoded.
    RawImage = 2,
    /// A video container. Foreign video travels byte-exact with no in-app decode (house-only doctrine); a VSF video container is the later design.
    Video = 3,
    /// Audio: the house PHCALL4 recording container (the wave card) or a foreign audio file (save-only until the decode-only ingest lands).
    Audio = 4,
    /// Plain UTF-8 text.
    Text = 5,
    /// UTF-8 source code (the language comes from the extension — a tiebreak, never a verdict).
    Code = 6,
    /// An archive: zip, 7z, rar, tar, gzip, xz, zstd.
    Archive = 7,
    /// An executable or script: ELF, PE, Mach-O, APK, shebang. Photon never runs one.
    Program = 8,
    /// A document (PDF today).
    Document = 9,
}

impl AttachKind {
    pub fn from_wire(v: u8) -> Option<Self> {
        Some(match v {
            0 => Self::Unknown,
            1 => Self::Image,
            2 => Self::RawImage,
            3 => Self::Video,
            4 => Self::Audio,
            5 => Self::Text,
            6 => Self::Code,
            7 => Self::Archive,
            8 => Self::Program,
            9 => Self::Document,
            _ => return None,
        })
    }

    /// The bubble glyph. Geometric shapes from the bundled text faces (with FE0E to pin the text presentation), never emoji codepoints the bubble font may tofu.
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Unknown => "\u{1F4CE}",
            Self::Image => "\u{25A3}\u{FE0E}",
            Self::RawImage => "\u{25A3}\u{FE0E} RAW",
            Self::Video => "\u{25B7}\u{FE0E}",
            Self::Audio => "\u{25B6}\u{FE0E}",
            Self::Text => "\u{2261}",
            Self::Code => "\u{2261}",
            Self::Archive => "\u{25A4}\u{FE0E}",
            Self::Program => "\u{25AA}\u{FE0E}",
            Self::Document => "\u{25AB}\u{FE0E}",
        }
    }

    /// Kinds whose row carries a picture preview and whose tap opens the image viewer.
    pub fn is_image(self) -> bool {
        matches!(self, Self::Image | Self::RawImage)
    }

    /// Kinds whose row carries a text preview and whose tap opens the reader.
    pub fn is_text(self) -> bool {
        matches!(self, Self::Text | Self::Code)
    }

    /// Ordering for the receiver's "stricter verdict" rule: a sniff that says Program outranks a wire claim of Image; a wire claim of RawImage outranks a local sniff of Image (the peer decoded what we cannot).
    fn strictness(self) -> u8 {
        match self {
            Self::Program => 9,
            Self::Archive => 8,
            Self::Unknown => 0,
            Self::RawImage => 4,
            Self::Image => 3,
            Self::Video => 3,
            Self::Audio => 3,
            Self::Document => 3,
            Self::Code => 2,
            Self::Text => 1,
        }
    }

    /// The receiver's rule: our own sniff wins when it is stricter (or the wire said nothing useful); the wire's verdict stands only where it names something our sniff could not tell apart from a plainer kind.
    pub fn reconcile(local: Self, wire: Self) -> Self {
        if local == wire || wire == Self::Unknown {
            return local;
        }
        if local.strictness() >= wire.strictness() {
            local
        } else if local == Self::Unknown && matches!(wire, Self::Program | Self::Archive) {
            // The peer's stricter claim about bytes we could not place is worth keeping — it only makes the pill more cautious.
            wire
        } else if local == Self::Image && wire == Self::RawImage {
            wire
        } else {
            local
        }
    }
}

/// The typed attachment metadata carried ON THE ROW beside the content string (which keeps hash + name + size, the row's identity): kind, pixel dims when known, and the hash of the eagerly replicated preview blob (Phase 2). Persisted, fleet-synced as page columns, and sent to the friend as typed package fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttachMeta {
    pub kind: AttachKind,
    pub dims: Option<(u32, u32)>,
    pub preview_hash: Option<[u8; 32]>,
}

/// Longest edge of the MICRO preview that rides on the row itself (a 24×24 gamma-2 VSF RGB thumb is 1728 bytes — small enough for a page column, enough for colour and shape before any blob exists).
pub const MICRO_PREVIEW_MAX_EDGE: usize = 24;
/// Bytes of a text/code file that ride on the row as its micro preview.
pub const MICRO_PREVIEW_TEXT_BYTES: usize = 240;
/// Hard cap any reader applies to a preview column/field before trusting it.
pub const MICRO_PREVIEW_MAX_BYTES: usize = 2 + MICRO_PREVIEW_MAX_EDGE * MICRO_PREVIEW_MAX_EDGE * 3;

/// Encode an image micro preview: `[w u8][h u8]` then `w × h × 3` bytes of gamma-2 VSF RGB, row-major.
pub fn encode_micro_image(w: usize, h: usize, vsf_gamma2: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + vsf_gamma2.len());
    out.push(w.min(255) as u8);
    out.push(h.min(255) as u8);
    out.extend_from_slice(vsf_gamma2);
    out
}

/// Decode an image micro preview → (w, h, gamma-2 VSF RGB bytes). None when the bytes are not a well-formed thumb (a text preview, or junk from the wire).
pub fn parse_micro_image(bytes: &[u8]) -> Option<(usize, usize, &[u8])> {
    if bytes.len() < 2 {
        return None;
    }
    let (w, h) = (bytes[0] as usize, bytes[1] as usize);
    if w == 0 || h == 0 || w > MICRO_PREVIEW_MAX_EDGE || h > MICRO_PREVIEW_MAX_EDGE {
        return None;
    }
    let px = &bytes[2..];
    (px.len() == w * h * 3).then_some((w, h, px))
}

/// The text micro preview: the first [`MICRO_PREVIEW_TEXT_BYTES`] of a UTF-8 file, cut on a character boundary, CR/NUL stripped.
pub fn micro_text(bytes: &[u8]) -> Vec<u8> {
    let s = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => match std::str::from_utf8(&bytes[..e.valid_up_to()]) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        },
    };
    let mut end = s.len().min(MICRO_PREVIEW_TEXT_BYTES);
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].replace(['\r', '\0'], "").into_bytes()
}

/// The file extension the bytes' own magic names — for the viewer temp of a NAMELESS attachment (images travel without filenames), so opsin's extension-dispatched ingest picks the right reader. TIFF-shaped bytes say "dng" (limbus reads the family); unknown magic says "bin".
pub fn sniff_ext(bytes: &[u8]) -> &'static str {
    let starts = |m: &[u8]| bytes.len() >= m.len() && &bytes[..m.len()] == m;
    let at = |off: usize, m: &[u8]| bytes.len() >= off + m.len() && &bytes[off..off + m.len()] == m;
    if starts(&[0xFF, 0xD8, 0xFF]) {
        return "jpg";
    }
    if starts(&[0xFF, 0x0A]) || starts(b"\0\0\0\x0cJXL ") {
        return "jxl";
    }
    if starts(b"\x89PNG\r\n\x1a\n") {
        return "png";
    }
    if starts(b"GIF8") {
        return "gif";
    }
    if starts(b"RIFF") && at(8, b"WEBP") {
        return "webp";
    }
    if starts(b"II*\0") || starts(b"MM\0*") || starts(b"IIRO") || starts(b"IIRS") {
        return "dng";
    }
    if starts(b"R") && bytes.len() >= 4 && bytes[..bytes.len().min(4096)].windows(14).any(|w| w == b"spectral_image") {
        return "vsf";
    }
    if starts(b"ID3") || starts(&[0xFF, 0xFB]) || starts(&[0xFF, 0xF3]) || starts(&[0xFF, 0xF2]) {
        return "mp3";
    }
    if starts(b"OggS") {
        return "ogg";
    }
    if starts(b"fLaC") {
        return "flac";
    }
    if starts(b"RIFF") && at(8, b"WAVE") {
        return "wav";
    }
    if at(4, b"ftyp") {
        return "m4a";
    }
    "bin"
}

/// Sniff the kind from the bytes, with the name as a tiebreak only where the container cannot tell (TIFF-shaped camera RAWs, a zip that is an APK, code vs text).
pub fn sniff(bytes: &[u8], name: &str) -> AttachKind {
    use AttachKind::*;
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let starts = |m: &[u8]| bytes.len() >= m.len() && &bytes[..m.len()] == m;
    let at = |off: usize, m: &[u8]| bytes.len() >= off + m.len() && &bytes[off..off + m.len()] == m;
    if bytes.is_empty() {
        return Unknown;
    }
    // Programs first: nothing else may claim these bytes.
    if starts(b"\x7fELF") || starts(b"MZ") || starts(b"#!") || starts(&[0xFE, 0xED, 0xFA, 0xCE]) || starts(&[0xFE, 0xED, 0xFA, 0xCF]) || starts(&[0xCF, 0xFA, 0xED, 0xFE]) || starts(&[0xCA, 0xFE, 0xBA, 0xBE]) {
        return Program;
    }
    if starts(b"PK\x03\x04") {
        return if matches!(ext.as_str(), "apk" | "jar" | "xapk" | "aab") { Program } else { Archive };
    }
    if starts(b"7z\xBC\xAF\x27\x1C") || starts(b"Rar!") || starts(&[0x1F, 0x8B]) || starts(&[0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00]) || starts(&[0x28, 0xB5, 0x2F, 0xFD]) || at(257, b"ustar") || starts(b"BZh") {
        return Archive;
    }
    if starts(b"%PDF") {
        return Document;
    }
    // House recording container.
    if starts(crate::call::record::CONTAINER_MAGIC_V6) {
        return Audio;
    }
    // Images. TIFF-shaped bytes are a RAW when the name says so (DNG/NEF/ARW/CR2/PEF/ORF/RW2/SRW carry TIFF headers).
    let raw_ext = matches!(ext.as_str(), "dng" | "nef" | "nrw" | "cr2" | "cr3" | "arw" | "srf" | "sr2" | "raf" | "orf" | "rw2" | "pef" | "srw" | "x3f" | "3fr" | "fff" | "iiq" | "mos" | "erf" | "mef" | "dcr" | "kdc" | "raw" | "rwl");
    if starts(b"II*\0") || starts(b"MM\0*") || starts(b"IIRO") || starts(b"IIRS") {
        return if raw_ext { RawImage } else { Image };
    }
    if starts(b"FUJIFILMCCD-RAW") || starts(b"FOVb") {
        return RawImage;
    }
    if starts(&[0xFF, 0xD8, 0xFF]) || starts(b"\x89PNG\r\n\x1a\n") || starts(b"GIF8") || starts(b"BM") || (starts(b"RIFF") && at(8, b"WEBP")) || starts(&[0xFF, 0x0A]) || starts(b"\0\0\0\x0cJXL ") {
        return Image;
    }
    // VSF: a spectral image names its primary section in the header; anything else in VSF is a document.
    if starts(b"R") && ext == "vsf" {
        let head = &bytes[..bytes.len().min(4096)];
        return if head.windows(14).any(|w| w == b"spectral_image") { RawImage } else { Document };
    }
    // Video containers.
    if at(4, b"ftyp") {
        let brand = &bytes[8..bytes.len().min(12)];
        return if brand.starts_with(b"M4A") || brand.starts_with(b"M4B") { Audio } else { Video };
    }
    if starts(&[0x1A, 0x45, 0xDF, 0xA3]) || (starts(b"RIFF") && at(8, b"AVI ")) || starts(b"FLV") || (bytes.len() > 188 && bytes[0] == 0x47 && bytes[188] == 0x47) {
        return Video;
    }
    // Audio containers.
    if starts(b"OggS") || starts(b"fLaC") || (starts(b"RIFF") && at(8, b"WAVE")) || starts(b"ID3") || starts(&[0xFF, 0xFB]) || starts(&[0xFF, 0xF3]) || starts(&[0xFF, 0xF2]) || starts(b"FORM") {
        return Audio;
    }
    if raw_ext {
        return RawImage;
    }
    // Text vs code: valid UTF-8 without NUL in the first 8 KB is text; the extension names code.
    let probe = &bytes[..bytes.len().min(8192)];
    let text_like = !probe.contains(&0) && match std::str::from_utf8(probe) {
        Ok(_) => true,
        Err(e) => e.error_len().is_none() && e.valid_up_to() + 4 >= probe.len(),
    };
    if text_like {
        return if code_language(name).is_some() { Code } else { Text };
    }
    Unknown
}

/// The language tag a code file's extension implies (shown in the pill; syntax colouring is a later round).
pub fn code_language(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
    Some(match ext.as_str() {
        "rs" => "Rust",
        "py" => "Python",
        "js" | "mjs" | "cjs" => "JavaScript",
        "ts" | "tsx" => "TypeScript",
        "jsx" => "JSX",
        "c" | "h" => "C",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => "C++",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "swift" => "Swift",
        "go" => "Go",
        "rb" => "Ruby",
        "php" => "PHP",
        "sh" | "bash" | "zsh" => "Shell",
        "html" | "htm" => "HTML",
        "css" => "CSS",
        "json" => "JSON",
        "toml" => "TOML",
        "yaml" | "yml" => "YAML",
        "xml" => "XML",
        "sql" => "SQL",
        "lua" => "Lua",
        "zig" => "Zig",
        "cs" => "C#",
        "m" | "mm" => "Objective-C",
        "glsl" | "wgsl" | "hlsl" => "Shader",
        "cmake" => "CMake",
        "gradle" => "Gradle",
        "mk" => "Make",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniff_places_each_kind_and_a_spoofed_extension_loses() {
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0], "a.jpg"), AttachKind::Image);
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\nxxxx", "a.png"), AttachKind::Image);
        assert_eq!(sniff(b"II*\0xxxxxxxx", "shot.dng"), AttachKind::RawImage);
        assert_eq!(sniff(b"II*\0xxxxxxxx", "scan.tif"), AttachKind::Image);
        assert_eq!(sniff(b"\0\0\0\x18ftypisom\0\0\0\0", "clip.mp4"), AttachKind::Video);
        assert_eq!(sniff(b"\0\0\0\x18ftypM4A \0\0\0\0", "song.m4a"), AttachKind::Audio);
        assert_eq!(sniff(b"OggS\0\0", "v.opus"), AttachKind::Audio);
        assert_eq!(sniff(b"PK\x03\x04junk", "app.apk"), AttachKind::Program);
        assert_eq!(sniff(b"PK\x03\x04junk", "docs.zip"), AttachKind::Archive);
        assert_eq!(sniff(b"\x7fELF\x02\x01\x01", "photo.jpg"), AttachKind::Program);
        assert_eq!(sniff(b"#!/bin/sh\necho hi\n", "cute.png"), AttachKind::Program);
        assert_eq!(sniff(b"%PDF-1.7\n", "x.pdf"), AttachKind::Document);
        assert_eq!(sniff(b"fn main() {}\n", "main.rs"), AttachKind::Code);
        assert_eq!(sniff("just words, with ünïcode\n".as_bytes(), "notes.txt"), AttachKind::Text);
        assert_eq!(sniff(&[0x00, 0x01, 0x02, 0x03, 0xFF], "blob.bin"), AttachKind::Unknown);
        assert_eq!(sniff(crate::call::record::CONTAINER_MAGIC_V6, "call.audio"), AttachKind::Audio);
    }

    #[test]
    fn micro_previews_are_bounded_and_round_trip() {
        let px: Vec<u8> = (0..24 * 16 * 3).map(|i| (i % 251) as u8).collect();
        let enc = encode_micro_image(24, 16, &px);
        assert!(enc.len() <= MICRO_PREVIEW_MAX_BYTES);
        let (w, h, back) = parse_micro_image(&enc).unwrap();
        assert_eq!((w, h), (24, 16));
        assert_eq!(back, &px[..]);
        assert!(parse_micro_image(&[24, 24, 0, 0]).is_none());
        let long: String = "é".repeat(300);
        let t = micro_text(long.as_bytes());
        assert!(t.len() <= MICRO_PREVIEW_TEXT_BYTES);
        assert!(std::str::from_utf8(&t).is_ok());
        assert_eq!(micro_text(b"a\r\nb\0c"), b"a\nbc".to_vec());
    }

    #[test]
    fn reconcile_keeps_the_stricter_verdict() {
        use AttachKind::*;
        assert_eq!(AttachKind::reconcile(Program, Image), Program);
        assert_eq!(AttachKind::reconcile(Image, Program), Image);
        assert_eq!(AttachKind::reconcile(Image, RawImage), RawImage);
        assert_eq!(AttachKind::reconcile(Unknown, Archive), Archive);
        assert_eq!(AttachKind::reconcile(Text, Code), Text);
        assert_eq!(AttachKind::reconcile(Video, Unknown), Video);
    }
}
