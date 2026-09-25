//! Typed reads from ONE record section — the decoder half of "one VSF section per record" (AGENT.md: nested structures, never parallel columns).
//! A document that holds many records of a kind carries one same-name section per record; each record's fields sit together, so a torn record is one missing field in one section, never columns of different lengths zipped by index.

use crate::storage::StorageError;
use vsf::file_format::VsfSection;
use vsf::VsfType;

/// Every section of a complete VSF document, after the verified read (provenance hash, or signature when `signer` is named).
pub fn verified_sections(bytes: &[u8], signer: Option<[u8; 32]>) -> Result<Vec<VsfSection>, StorageError> {
    let (header, header_end) = vsf::verification::read_verified(bytes, signer).map_err(|e| StorageError::Parse(format!("verified read: {e}")))?;
    header.sections(bytes, header_end).map_err(|e| StorageError::Parse(format!("sections: {e}")))
}

/// One record: typed accessors over a section's single-valued fields.
pub struct Rec<'a>(pub &'a VsfSection);

impl Rec<'_> {
    fn first(&self, name: &str) -> Option<&VsfType> {
        self.0.get_fields(name).first().and_then(|f| f.values.first())
    }

    /// An opaque 32-byte value (`hb`).
    pub fn h32(&self, name: &str) -> Option<[u8; 32]> {
        match self.first(name)? {
            VsfType::hb(b) => <[u8; 32]>::try_from(b.as_slice()).ok(),
            _ => None,
        }
    }

    /// An opaque 64-byte value (`hb`) — a signature.
    pub fn h64(&self, name: &str) -> Option<[u8; 64]> {
        match self.first(name)? {
            VsfType::hb(b) => <[u8; 64]>::try_from(b.as_slice()).ok(),
            _ => None,
        }
    }

    /// A 32-byte provenance hash (`hp`).
    pub fn hp32(&self, name: &str) -> Option<[u8; 32]> {
        match self.first(name)? {
            VsfType::hp(b) => <[u8; 32]>::try_from(b.as_slice()).ok(),
            _ => None,
        }
    }

    /// An application-wrapped value (`v` with its tag byte — `C` a chain, `X` ciphertext, `K` a decapsulation key).
    pub fn wrapped(&self, name: &str, tag: u8) -> Option<Vec<u8>> {
        match self.first(name)? {
            VsfType::v(t, d) if *t == tag => Some(d.clone()),
            _ => None,
        }
    }

    /// Every value of a repeated single-valued field, in order — a LIST of one kind (participants), never columns zipped against another.
    pub fn all(&self, name: &str) -> Vec<&VsfType> {
        self.0.get_fields(name).into_iter().filter_map(|f| f.values.first()).collect()
    }

    /// A 32-byte Ed25519 device key (`ke`).
    pub fn key32(&self, name: &str) -> Option<[u8; 32]> {
        match self.first(name)? {
            VsfType::ke(b) => <[u8; 32]>::try_from(b.as_slice()).ok(),
            _ => None,
        }
    }

    /// Opaque bytes of any length (`hR` raw, or `hb`).
    pub fn bytes(&self, name: &str) -> Option<Vec<u8>> {
        match self.first(name)? {
            VsfType::hR(b) | VsfType::hb(b) => Some(b.clone()),
            _ => None,
        }
    }

    /// Human text (`x`).
    pub fn text(&self, name: &str) -> Option<String> {
        match self.first(name)? {
            VsfType::x(s) => Some(s.clone()),
            _ => None,
        }
    }

    /// An unsigned integer, width-agnostic (writers auto-size, so a parsed value never variant-matches its written width).
    pub fn uint(&self, name: &str) -> Option<u64> {
        self.first(name)?.as_u64()
    }

    /// An Eagle-time stamp in oscillations (`e6`), or any signed integer.
    pub fn osc(&self, name: &str) -> Option<i64> {
        match self.first(name)? {
            VsfType::e(vsf::types::EtType::e6(o)) => Some(*o),
            other => other.as_i64(),
        }
    }
}
