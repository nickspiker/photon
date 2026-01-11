use crate::crypto::handle_proof::handle_proof;
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

    /// Generate handle proof for DHT lookup
    /// VSF normalizes Unicode, then runs handle_proof (memory-hard PoW, ~1s)
    pub fn to_handle_proof(&self) -> [u8; 32] {
        Self::username_to_handle_proof(&self.text)
    }

    /// Generate handle proof from a username string
    /// Memory-hard PoW (24MB, 17 rounds, ~1s) to prevent handle squatting
    pub fn username_to_handle_proof(username: &str) -> [u8; 32] {
        let vsf_bytes = vsf::VsfType::x(username.to_string()).flatten();
        let initial_hash = blake3::hash(&vsf_bytes);
        *handle_proof(&initial_hash).as_bytes()
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
