use super::ContactId;
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Clone, Debug)]
pub struct KeyShard {
    pub id: ShardId,
    pub owner: ContactId,
    pub encrypted_shard: Vec<u8>,
    pub index: u8,
    pub threshold: u8,
    pub total_shards: u8,
    pub created_at: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShardId([u8; 16]);

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct DecryptedShard {
    pub index: u8,
    pub data: [u8; 8],
}

#[derive(Clone, Debug)]
pub struct RecoveryRequest {
    pub requester_public_key: [u8; 32],
    pub request_id: [u8; 16],
    pub timestamp: f64,
    pub verification_phrase: String,
}

#[derive(Clone, Debug)]
pub struct RecoveryApproval {
    pub request_id: [u8; 16],
    pub shard: KeyShard,
    pub approver: ContactId,
    pub timestamp: f64,
}

impl ShardId {
    pub fn new() -> Self {
        let mut id = [0u8; 16];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut id);
        Self(id)
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl Default for ShardId {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyShard {
    pub fn new(
        owner: ContactId,
        encrypted_shard: Vec<u8>,
        index: u8,
        threshold: u8,
        total_shards: u8,
    ) -> Self {
        Self {
            id: ShardId::new(),
            owner,
            encrypted_shard,
            index,
            threshold,
            total_shards,
            created_at: vsf::eagle_time_nanos(),
        }
    }
}

pub struct ShardDistribution {
    pub shards: Vec<(ContactId, DecryptedShard)>,
    pub threshold: u8,
}

impl ShardDistribution {
    pub fn split_key(private_key: &[u8; 32], contacts: &[(ContactId, f32)], threshold: u8) -> Self {
        let _total_shards = contacts.len() as u8;
        let mut shards = Vec::with_capacity(contacts.len());

        for (i, (contact_id, _weight)) in contacts.iter().enumerate() {
            let mut shard_data = [0u8; 8];
            shard_data.copy_from_slice(&private_key[i * 4..(i + 1) * 4]);
            shard_data[4..]
                .copy_from_slice(&private_key[16 + (i * 4 % 16)..16 + ((i + 1) * 4 % 16)]);

            let shard = DecryptedShard {
                index: i as u8,
                data: shard_data,
            };

            shards.push((contact_id.clone(), shard));
        }

        Self { shards, threshold }
    }

    pub fn reconstruct_key(shards: &[DecryptedShard], threshold: u8) -> Result<[u8; 32], String> {
        if shards.len() < threshold as usize {
            return Err(format!(
                "Insufficient shards: have {}, need {}",
                shards.len(),
                threshold
            ));
        }

        let mut key = [0u8; 32];

        for shard in shards.iter().take(threshold as usize) {
            let offset = (shard.index as usize) * 4;
            key[offset..offset + 4].copy_from_slice(&shard.data[..4]);

            let offset2 = 16 + ((shard.index as usize) * 4 % 16);
            key[offset2..offset2 + 4].copy_from_slice(&shard.data[4..]);
        }

        Ok(key)
    }
}
