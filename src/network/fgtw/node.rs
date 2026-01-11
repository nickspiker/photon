use crate::types::DevicePubkey;
use std::net::SocketAddr;

/// Get current Eagle Time (seconds since Apollo 11 landing)
fn eagle_time() -> f64 {
    vsf::eagle_time_nanos()
}

/// Node identifier for FGTW routing
/// Wraps the device's X25519 pubkey for use in Kademlia XOR distance calculations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId([u8; 32]);

impl NodeId {
    /// Create NodeId from public identity (directly uses pubkey bytes)
    pub fn from_pubkey(pubkey: &DevicePubkey) -> Self {
        Self(*pubkey.as_bytes())
    }

    /// Create from raw bytes
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Get raw bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Calculate XOR distance to another node (for Kademlia routing)
    pub fn distance(&self, other: &NodeId) -> [u8; 32] {
        let mut dist = [0u8; 32];
        for i in 0..32 {
            dist[i] = self.0[i] ^ other.0[i];
        }
        dist
    }

    /// Count leading zero bits in this NodeId (for bucket index calculation)
    pub fn leading_zeros(&self) -> usize {
        for (i, &byte) in self.0.iter().enumerate() {
            if byte != 0 {
                return i * 8 + byte.leading_zeros() as usize;
            }
        }
        256 // All zeros
    }

    /// Calculate bucket index for another node relative to this node
    /// Returns which of the 256 buckets the other node belongs in
    pub fn bucket_index(&self, other: &NodeId) -> usize {
        let distance = self.distance(other);
        let mut leading_zeros = 0;
        for &byte in &distance {
            if byte != 0 {
                leading_zeros += byte.leading_zeros() as usize;
                break;
            }
            leading_zeros += 8;
        }
        255_usize.saturating_sub(leading_zeros) // Bucket 0 = farthest, bucket 255 = closest
    }

    /// Convert to hex string for display
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.to_hex()[..16]) // Show first 16 chars
    }
}

/// Contact information for a node in the routing table
#[derive(Debug, Clone)]
pub struct NodeContact {
    pub node_id: NodeId,
    pub pubkey: DevicePubkey,
    pub addr: SocketAddr,
    pub last_seen: f64,
}

impl NodeContact {
    pub fn new(pubkey: DevicePubkey, addr: SocketAddr) -> Self {
        Self {
            node_id: NodeId::from_pubkey(&pubkey),
            pubkey,
            addr,
            last_seen: eagle_time(),
        }
    }

    pub fn update_last_seen(&mut self) {
        self.last_seen = eagle_time();
    }

    pub fn is_stale(&self, max_age_secs: f64) -> bool {
        let now = eagle_time();
        now - self.last_seen > max_age_secs
    }
}

/// K-bucket for Kademlia routing table
/// Each bucket stores up to K nodes at a specific XOR distance range
#[derive(Debug, Clone)]
pub struct KBucket {
    entries: Vec<NodeContact>,
    max_size: usize,
}

impl KBucket {
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: Vec::with_capacity(max_size),
            max_size,
        }
    }

    /// Try to insert a node contact
    /// Returns true if inserted, false if bucket is full
    pub fn insert(&mut self, contact: NodeContact) -> bool {
        // Check if node already exists
        if let Some(pos) = self
            .entries
            .iter()
            .position(|c| c.node_id == contact.node_id)
        {
            // Update existing entry and move to end (most recently seen)
            self.entries.remove(pos);
            self.entries.push(contact);
            return true;
        }

        // If bucket not full, add new entry
        if self.entries.len() < self.max_size {
            self.entries.push(contact);
            return true;
        }

        // Bucket is full - check if we can evict a stale entry
        if let Some(pos) = self.entries.iter().position(|c| c.is_stale(3600.0)) {
            // Evict stale entry (older than 1 hour)
            self.entries.remove(pos);
            self.entries.push(contact);
            return true;
        }

        false // Bucket full, no stale entries
    }

    /// Get all entries in this bucket
    pub fn entries(&self) -> &[NodeContact] {
        &self.entries
    }

    /// Check if bucket is full
    pub fn is_full(&self) -> bool {
        self.entries.len() >= self.max_size
    }

    /// Get number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if bucket is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Remove a node by NodeId
    pub fn remove(&mut self, node_id: &NodeId) -> Option<NodeContact> {
        if let Some(pos) = self.entries.iter().position(|c| &c.node_id == node_id) {
            Some(self.entries.remove(pos))
        } else {
            None
        }
    }

    /// Get the least recently seen entry
    pub fn get_lru(&self) -> Option<&NodeContact> {
        self.entries.first()
    }
}

/// Kademlia routing table with 256 buckets
/// Each bucket stores nodes at a specific XOR distance range
pub struct RoutingTable {
    local_id: NodeId,
    buckets: Vec<KBucket>,
}

impl RoutingTable {
    /// Create a new routing table for the local node
    pub fn new(local_pubkey: &DevicePubkey) -> Self {
        let local_id = NodeId::from_pubkey(local_pubkey);
        let buckets = (0..256).map(|_| KBucket::new(256)).collect();

        Self { local_id, buckets }
    }

    /// Insert or update a node in the routing table
    pub fn insert(&mut self, contact: NodeContact) -> bool {
        // Don't insert ourselves
        if contact.node_id == self.local_id {
            return false;
        }

        let bucket_idx = self.local_id.bucket_index(&contact.node_id);
        self.buckets[bucket_idx].insert(contact)
    }

    /// Remove a node from the routing table
    pub fn remove(&mut self, node_id: &NodeId) -> Option<NodeContact> {
        let bucket_idx = self.local_id.bucket_index(node_id);
        self.buckets[bucket_idx].remove(node_id)
    }

    /// Find the K closest nodes to a target ID
    pub fn find_closest(&self, target: &NodeId, count: usize) -> Vec<NodeContact> {
        let mut all_nodes: Vec<(NodeContact, [u8; 32])> = Vec::new();

        // Collect all nodes with their distances to target
        for bucket in &self.buckets {
            for contact in bucket.entries() {
                let distance = target.distance(&contact.node_id);
                all_nodes.push((contact.clone(), distance));
            }
        }

        // Sort by distance (closest first)
        all_nodes.sort_by(|a, b| a.1.cmp(&b.1));

        // Return top K
        all_nodes
            .into_iter()
            .take(count)
            .map(|(contact, _)| contact)
            .collect()
    }

    /// Get all nodes in the routing table
    pub fn all_nodes(&self) -> Vec<NodeContact> {
        let mut nodes = Vec::new();
        for bucket in &self.buckets {
            nodes.extend_from_slice(bucket.entries());
        }
        nodes
    }

    /// Get total number of nodes in routing table
    pub fn node_count(&self) -> usize {
        self.buckets.iter().map(|b| b.len()).sum()
    }

    /// Get reference to local NodeId
    pub fn local_id(&self) -> &NodeId {
        &self.local_id
    }

    /// Get statistics about bucket fill rates
    pub fn bucket_stats(&self) -> Vec<(usize, usize)> {
        self.buckets
            .iter()
            .enumerate()
            .map(|(idx, bucket)| (idx, bucket.len()))
            .filter(|(_, len)| *len > 0)
            .collect()
    }
}
