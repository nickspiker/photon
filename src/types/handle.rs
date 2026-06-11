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

    /// Generate handle proof for DHT lookup. Delegates to [`ihi::handle_to_proof`] — the canonical entry point per ihi's consolidation doc-comment ("every component in the stack should call this rather than rolling its own pre-hash step"). The username is encoded via `VsfType::x` (NFC normalization + Huffman codebook), BLAKE3-hashed, then fed through the memory-hard PoW.
    pub fn to_handle_proof(&self) -> [u8; 32] {
        Self::username_to_handle_proof(&self.text)
    }

    /// Generate handle proof from a username string via [`ihi::handle_to_proof`]. Memory-hard PoW (24MB scratch, 17 rounds, ~1s on 2025 hardware) — anti-squatting + ASIC-resistant. Returns the 32-byte proof.
    pub fn username_to_handle_proof(username: &str) -> [u8; 32] {
        *ihi::handle_to_proof(username).as_bytes()
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
