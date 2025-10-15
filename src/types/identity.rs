use blake3::Hasher;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
pub struct PublicIdentity {
    pub key: [u8; 32],
}

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct PrivateIdentity {
    secret: [u8; 32],
}

#[derive(Clone)]
pub struct Identity {
    pub public: PublicIdentity,
    private: PrivateIdentity,
}

impl PublicIdentity {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self { key: bytes }
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.key
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

impl PrivateIdentity {
    pub fn generate() -> Self {
        let mut secret = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut secret);
        Self { secret }
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self { secret: bytes }
    }

    pub fn to_public(&self) -> PublicIdentity {
        let scalar = x25519_dalek::EphemeralSecret::random_from_rng(&mut rand::thread_rng());
        let public = x25519_dalek::PublicKey::from(&scalar);
        // For now, we'll use the ephemeral key. In production, we'd derive from secret.
        PublicIdentity {
            key: *public.as_bytes(),
        }
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.secret
    }
}

impl Identity {
    pub fn generate() -> Self {
        let private = PrivateIdentity::generate();
        let public = private.to_public();
        Self { public, private }
    }

    pub fn from_private_key(bytes: [u8; 32]) -> Self {
        let private = PrivateIdentity::from_bytes(bytes);
        let public = private.to_public();
        Self { public, private }
    }

    pub fn private_key(&self) -> &[u8; 32] {
        self.private.as_bytes()
    }

    pub fn compute_shared_secret(&self, their_public: &PublicIdentity) -> [u8; 32] {
        // For now, return a placeholder. In production, implement proper DH.
        let mut shared = [0u8; 32];
        shared[..16].copy_from_slice(&self.private.secret[..16]);
        shared[16..].copy_from_slice(&their_public.key[..16]);
        blake3::hash(&shared).into()
    }
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("public", &self.public)
            .field("private", &"[REDACTED]")
            .finish()
    }
}
