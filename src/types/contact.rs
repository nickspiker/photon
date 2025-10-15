use super::{PublicIdentity, Seed};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Contact {
    pub id: ContactId,
    pub name: String,
    pub public_identity: PublicIdentity,
    pub relationship_seed: Option<Seed>,
    pub trust_level: TrustLevel,
    pub added_timestamp: u64,
    pub last_seen: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContactId([u8; 16]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrustLevel {
    Stranger,
    Known,
    Trusted,
    Inner,
}

impl ContactId {
    pub fn new() -> Self {
        let mut id = [0u8; 16];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut id);
        Self(id)
    }

    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl Default for ContactId {
    fn default() -> Self {
        Self::new()
    }
}

impl Contact {
    pub fn new(name: String, public_identity: PublicIdentity) -> Self {
        Self {
            id: ContactId::new(),
            name,
            public_identity,
            relationship_seed: None,
            trust_level: TrustLevel::Stranger,
            added_timestamp: crate::types::message::current_timestamp(),
            last_seen: None,
        }
    }

    pub fn with_seed(mut self, seed: Seed) -> Self {
        self.relationship_seed = Some(seed);
        self
    }

    pub fn with_trust_level(mut self, level: TrustLevel) -> Self {
        self.trust_level = level;
        self
    }

    pub fn update_last_seen(&mut self, timestamp: u64) {
        self.last_seen = Some(timestamp);
    }

    pub fn can_be_custodian(&self) -> bool {
        matches!(self.trust_level, TrustLevel::Trusted | TrustLevel::Inner)
    }
}
