use blake3::Hasher;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use vsf::VsfType;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Public identity (X25519 public key)
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PublicIdentity {
    pub key: [u8; 32],
}

/// Private identity (X25519 secret key)
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct PrivateIdentity {
    secret: [u8; 32],
}

/// Complete identity (public + private keys)
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

    /// Convert to VSF X25519 key type
    pub fn to_vsf(&self) -> VsfType {
        VsfType::kx(self.key.to_vec())
    }

    /// Create from VSF X25519 key type
    pub fn from_vsf(vsf: VsfType) -> Option<Self> {
        match vsf {
            VsfType::kx(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                Some(Self { key: arr })
            }
            _ => None,
        }
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

// Serde implementations for PublicIdentity (serialize as hex string)
impl Serialize for PublicIdentity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for PublicIdentity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let hex_str = String::deserialize(deserializer)?;
        PublicIdentity::from_hex(&hex_str).map_err(serde::de::Error::custom)
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
        // Properly derive X25519 public key from secret
        let secret = x25519_dalek::StaticSecret::from(self.secret);
        let public = x25519_dalek::PublicKey::from(&secret);
        PublicIdentity {
            key: *public.as_bytes(),
        }
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.secret
    }

    /// Convert to VSF X25519 key type (CAREFUL: this exposes the private key!)
    pub fn to_vsf(&self) -> VsfType {
        VsfType::kx(self.secret.to_vec())
    }

    /// Create from VSF X25519 key type
    pub fn from_vsf(vsf: VsfType) -> Option<Self> {
        match vsf {
            VsfType::kx(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                Some(Self { secret: arr })
            }
            _ => None,
        }
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

    /// Compute X25519 Diffie-Hellman shared secret
    pub fn compute_shared_secret(&self, their_public: &PublicIdentity) -> [u8; 32] {
        let our_secret = x25519_dalek::StaticSecret::from(self.private.secret);
        let their_pubkey = x25519_dalek::PublicKey::from(their_public.key);
        let shared = our_secret.diffie_hellman(&their_pubkey);
        *shared.as_bytes()
    }

    /// Serialize identity to bare VSF bytes (includes private key - handle carefully!)
    pub fn to_vsf_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(self.public.to_vsf().flatten());
        bytes.extend(self.private.to_vsf().flatten());
        bytes
    }

    /// Deserialize identity from bare VSF bytes
    pub fn from_vsf_bytes(bytes: &[u8]) -> Result<Self, String> {
        use vsf::parse;

        let mut ptr = 0;

        // Parse public key
        let public_vsf =
            parse(bytes, &mut ptr).map_err(|e| format!("Parse public key error: {}", e))?;
        let public = PublicIdentity::from_vsf(public_vsf).ok_or("Invalid public key type")?;

        // Parse private key
        let private_vsf =
            parse(bytes, &mut ptr).map_err(|e| format!("Parse private key error: {}", e))?;
        let private = PrivateIdentity::from_vsf(private_vsf).ok_or("Invalid private key type")?;

        Ok(Identity { public, private })
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
