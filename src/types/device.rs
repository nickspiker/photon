use blake3::Hasher;
use vsf::VsfType;

/// Ed25519 public key - the device's identity
///
/// This is an Ed25519 verifying key used for:
/// - Signing messages (directly)
/// - Converting to X25519 for DHE when needed (CLUTCH)
///
/// Serializes as VSF `ke` (Ed25519 key)
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DevicePubkey {
    pub key: [u8; 32],
}

impl DevicePubkey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self { key: bytes }
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.key
    }

    /// Convert to VSF Ed25519 key type
    pub fn to_vsf(&self) -> VsfType {
        VsfType::ke(self.key.to_vec())
    }

    /// Create from VSF Ed25519 key type
    pub fn from_vsf(vsf: VsfType) -> Option<Self> {
        match vsf {
            VsfType::ke(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                Some(Self { key: arr })
            }
            _ => None,
        }
    }

    /// Convert Ed25519 public key to X25519 for Diffie-Hellman
    ///
    /// Uses the standard birational map from Ed25519 to Curve25519.
    /// This allows using a single Ed25519 identity for both signing and DHE.
    pub fn to_x25519(&self) -> x25519_dalek::PublicKey {
        // Ed25519 public keys can be converted to X25519 using montgomery_point
        // The ed25519-dalek VerifyingKey has to_montgomery() but we have raw bytes
        // Use curve25519-dalek's CompressedEdwardsY directly
        use curve25519_dalek::edwards::CompressedEdwardsY;
        let compressed = CompressedEdwardsY::from_slice(&self.key).unwrap();
        let edwards = compressed.decompress().expect("invalid Ed25519 point");
        let montgomery = edwards.to_montgomery();
        x25519_dalek::PublicKey::from(*montgomery.as_bytes())
    }

    pub fn to_dht_infohash(&self) -> [u8; 20] {
        let mut hasher = Hasher::new();
        hasher.update(b"tmessage-dht-v1");
        hasher.update(&self.key);
        let hash = hasher.finalize();
        let mut infohash = [0u8; 20];
        infohash.copy_from_slice(&hash.as_bytes()[..20]);
        infohash
    }

    pub fn to_hex(&self) -> String {
        hex::encode(&self.key)
    }

    pub fn from_hex(s: &str) -> Result<Self, hex::FromHexError> {
        let bytes = hex::decode(s)?;
        if bytes.len() != 32 {
            return Err(hex::FromHexError::InvalidStringLength);
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self { key: arr })
    }
}

/// Convert Ed25519 signing key to X25519 secret for Diffie-Hellman
///
/// This is the secret-key counterpart to DevicePubkey::to_x25519().
/// Used when we need to do DHE with our Ed25519 identity.
pub fn ed25519_secret_to_x25519(
    ed_secret: &ed25519_dalek::SigningKey,
) -> x25519_dalek::StaticSecret {
    // Ed25519 secret keys are hashed before use; the first 32 bytes of SHA-512(secret)
    // become the scalar. For X25519, we need that scalar clamped.
    // ed25519-dalek's SigningKey stores the seed, so we hash it like Ed25519 does.
    use sha2::{Digest, Sha512};
    let mut hasher = Sha512::new();
    hasher.update(ed_secret.as_bytes());
    let hash = hasher.finalize();

    // First 32 bytes, clamped per X25519 spec
    let mut scalar = [0u8; 32];
    scalar.copy_from_slice(&hash[..32]);
    scalar[0] &= 248;
    scalar[31] &= 127;
    scalar[31] |= 64;

    x25519_dalek::StaticSecret::from(scalar)
}
