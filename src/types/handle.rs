use crate::types::DevicePubkey;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handle {
    pub text: String,      // handle
    pub key: DevicePubkey, // X25519 public key
}

impl Handle {
    pub fn new(username: String, identity: DevicePubkey) -> Self {
        Self {
            text: username,
            key: identity,
        }
    }

    /// Generate handle proof for DHT lookup. Delegates to [`ihi::handle_to_proof`] — the canonical entry point per ihi's consolidation doc-comment ("every component in the stack should call this rather than rolling its own pre-hash step"). The username is encoded via `VsfType::x` (NFC normalization + Huffman codebook), BLAKE3-hashed, then fed thru the memory-hard PoW.
    pub fn to_handle_proof(&self) -> [u8; 32] {
        Self::username_to_handle_proof(&self.text)
    }

    /// Generate handle proof from a username string via [`ihi::handle_to_proof`], after [`Self::canonical`] normalization. Memory-hard PoW (24MB scratch, 17 rounds, ~1s on 2025 hardware) — anti-squatting + ASIC-resistant. Returns the 32-byte proof.
    pub fn username_to_handle_proof(username: &str) -> [u8; 32] {
        *ihi::handle_to_proof(&Self::canonical(username)).as_bytes()
    }

    /// Identity seed from a handle string, [`Self::canonical`]-normalized — the ONE "handle string → identity_seed" entry point. Every call site (attest, contacts, avatars) must come thru here; a raw `ihi::handle_to_hash(typed_string)` derives a different identity for every typo-variant of the same handle.
    pub fn to_identity_seed(handle: &str) -> [u8; 32] {
        *ihi::handle_to_hash(&Self::canonical(handle)).as_bytes()
    }

    /// The ONE canonical spelling of a handle, applied before EVERY derivation (proof + identity seed). ihi only does Unicode NFC, so without this the same handle typed with different case, spacing, or camelCase concatenation derives a DIFFERENT identity — the observed "double handle proof": one device attests `FractalDecoder`, another types `fractal decoder`, the probe finds no chain, and a second genesis forks the identity.
    /// Rules: split on whitespace AND lower→Upper camelCase boundaries, lowercase every word, join with single spaces. `"FractalDecoder"`, `" Fractal  Decoder "`, and `"fractal decoder"` all canonicalize to `"fractal decoder"`.
    pub fn canonical(handle: &str) -> String {
        let mut words: Vec<String> = Vec::new();
        for token in handle.split_whitespace() {
            let mut cur = String::new();
            let mut prev_lower = false;
            for c in token.chars() {
                if c.is_uppercase() && prev_lower && !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
                prev_lower = c.is_lowercase();
                cur.extend(c.to_lowercase());
            }
            if !cur.is_empty() {
                words.push(cur);
            }
        }
        words.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_proof_generation() {
        let identity = DevicePubkey::from_bytes([1u8; 32]);
        let handle = Handle::new("alice".to_string(), identity);

        let proof1 = handle.to_handle_proof();
        let proof2 = Handle::username_to_handle_proof("alice");
        assert_eq!(proof1, proof2);
    }

    #[test]
    fn test_any_unicode_valid() {
        let identity = DevicePubkey::from_bytes([1u8; 32]);

        let _h1 = Handle::new("alice".to_string(), identity.clone());
        let _h2 = Handle::new("🚀".to_string(), identity.clone());
        let _h3 = Handle::new("".to_string(), identity.clone());
        let _h4 = Handle::new("∫∂x".to_string(), identity.clone());
    }

    #[test]
    fn canonical_folds_case_spacing_and_camel() {
        assert_eq!(Handle::canonical("FractalDecoder"), "fractal decoder");
        assert_eq!(Handle::canonical(" Fractal  Decoder "), "fractal decoder");
        assert_eq!(Handle::canonical("fractal decoder"), "fractal decoder");
        assert_eq!(Handle::canonical("nem"), "nem");
        // ALL-CAPS is a single word (no lower→Upper boundary), not per-letter splits.
        assert_eq!(Handle::canonical("NASA"), "nasa");
        // Same canonical string → same proof, whatever the typist did.
        assert_eq!(
            Handle::username_to_handle_proof("FractalDecoder"),
            Handle::username_to_handle_proof("  fractal   Decoder ")
        );
    }

    #[test]
    fn test_handle_proof_deterministic() {
        // Run multiple times and verify same result
        let proof1 = Handle::username_to_handle_proof("fractal decoder");
        let proof2 = Handle::username_to_handle_proof("fractal decoder");
        let proof3 = Handle::username_to_handle_proof("fractal decoder");

        println!("Proof 1: {}", hex::encode(&proof1));
        println!("Proof 2: {}", hex::encode(&proof2));
        println!("Proof 3: {}", hex::encode(&proof3));

        assert_eq!(
            proof1, proof2,
            "Proof should be deterministic between calls"
        );
        assert_eq!(
            proof2, proof3,
            "Proof should be deterministic between calls"
        );
    }

    #[test]
    fn test_nem_handle_proof() {
        // Test the specific handle "nem" that's showing different proofs
        let vsf_bytes = vsf::VsfType::x("nem".to_string()).flatten();
        println!("VSF bytes for 'nem': {:02x?}", vsf_bytes);
        println!("VSF bytes hex: {}", hex::encode(&vsf_bytes));

        let proof = Handle::username_to_handle_proof("nem");
        println!("Handle proof for 'nem': {}", hex::encode(&proof));

        // Run again to verify determinism
        let proof2 = Handle::username_to_handle_proof("nem");
        println!("Handle proof for 'nem' (2nd): {}", hex::encode(&proof2));

        assert_eq!(proof, proof2, "nem handle proof should be deterministic");
    }
}
