use crate::types::PublicIdentity;
use blake3::Hasher;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handle {
    pub text: String,        // handle
    pub key: PublicIdentity, // X25519 public key
}

impl Handle {
    pub fn new(username: String, identity: PublicIdentity) -> Self {
        Self {
            text: username,
            key: identity,
        }
    }

    /// Generate hash for DHT lookup
    /// VSF normalizes Unicode, then BLAKE3 hash
    pub fn to_infohash(&self) -> [u8; 32] {
        let vsf_bytes = vsf::VsfType::x(self.text.clone()).flatten();
        let mut hasher = Hasher::new();
        hasher.update(&vsf_bytes);
        *hasher.finalize().as_bytes()
    }

    /// Generate infohash from a username string
    pub fn username_to_infohash(username: &str) -> [u8; 32] {
        let vsf_bytes = vsf::VsfType::x(username.to_string()).flatten();
        let mut hasher = Hasher::new();
        hasher.update(&vsf_bytes);
        *hasher.finalize().as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infohash_generation() {
        let identity = PublicIdentity::from_bytes([1u8; 32]);
        let handle = Handle::new("alice".to_string(), identity);

        let infohash1 = handle.to_infohash();
        let infohash2 = Handle::username_to_infohash("alice");
        assert_eq!(infohash1, infohash2);
    }

    #[test]
    fn test_any_unicode_valid() {
        let identity = PublicIdentity::from_bytes([1u8; 32]);

        let _h1 = Handle::new("alice".to_string(), identity.clone());
        let _h2 = Handle::new("🚀".to_string(), identity.clone());
        let _h3 = Handle::new("".to_string(), identity.clone());
        let _h4 = Handle::new("∫∂x".to_string(), identity.clone());
    }
}
