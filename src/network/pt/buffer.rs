//! PT Receive Buffer
//!
//! Manages reassembly of incoming DATA packets:
//! - Pre-allocates buffer based on SPEC
//! - Tracks received packets with bitmap
//! - Detects gaps for NAK generation
//! - Computes final hash for verification

use bitvec::prelude::*;

/// Receive buffer for reassembling incoming transfer
pub struct ReceiveBuffer {
    /// Pre-allocated data buffer
    data: Vec<u8>,
    /// Bitmap of received packets (true = received)
    received: BitVec,
    /// Expected packet size
    packet_size: u16,
    /// Total packets expected
    total_packets: u32,
    /// Total size expected
    total_size: u32,
    /// Expected hash from SPEC
    expected_hash: [u8; 32],
    /// Count of received packets
    received_count: u32,
}

impl ReceiveBuffer {
    /// Create new receive buffer from SPEC
    pub fn new(
        total_packets: u32,
        packet_size: u16,
        total_size: u32,
        expected_hash: [u8; 32],
    ) -> Self {
        // Pre-allocate data buffer
        let data = vec![0u8; total_size as usize];

        // Create bitmap for tracking
        let received = bitvec![0; total_packets as usize];

        Self {
            data,
            received,
            packet_size,
            total_packets,
            total_size,
            expected_hash,
            received_count: 0,
        }
    }

    /// Insert received packet, returns true if new (not duplicate)
    pub fn insert(&mut self, sequence: u32, payload: &[u8]) -> bool {
        if sequence >= self.total_packets {
            return false; // Out of range
        }

        let idx = sequence as usize;
        if self.received[idx] {
            return false; // Duplicate
        }

        // Calculate offset in data buffer
        let offset = idx * self.packet_size as usize;
        let end = (offset + payload.len()).min(self.total_size as usize);

        // Copy payload to buffer
        if offset < self.data.len() {
            let copy_len = end - offset;
            self.data[offset..offset + copy_len].copy_from_slice(&payload[..copy_len]);
        }

        // Mark as received
        self.received.set(idx, true);
        self.received_count += 1;

        true
    }

    /// Check if transfer is complete (all packets received)
    pub fn is_complete(&self) -> bool {
        self.received_count == self.total_packets
    }

    /// Get list of missing sequence numbers
    pub fn missing_sequences(&self) -> Vec<u32> {
        self.received
            .iter()
            .enumerate()
            .filter(|(_, received)| !**received)
            .map(|(i, _)| i as u32)
            .collect()
    }

    /// Get count of missing packets
    pub fn missing_count(&self) -> u32 {
        self.total_packets - self.received_count
    }

    /// Verify final hash matches expected
    pub fn verify(&self) -> bool {
        if !self.is_complete() {
            return false;
        }

        let actual_hash = blake3::hash(&self.data[..self.total_size as usize]);
        actual_hash.as_bytes() == &self.expected_hash
    }

    /// Get final data (only valid if complete and verified)
    pub fn take_data(self) -> Vec<u8> {
        let mut data = self.data;
        data.truncate(self.total_size as usize);
        data
    }

    /// Get computed hash of received data
    pub fn compute_hash(&self) -> [u8; 32] {
        *blake3::hash(&self.data[..self.total_size as usize]).as_bytes()
    }

    /// Get expected hash
    pub fn expected_hash(&self) -> [u8; 32] {
        self.expected_hash
    }

    /// Get progress as (received, total)
    pub fn progress(&self) -> (u32, u32) {
        (self.received_count, self.total_packets)
    }

    /// Get total expected size in bytes
    pub fn total_size(&self) -> u32 {
        self.total_size
    }

    /// Get total expected packets
    pub fn total_packets(&self) -> u32 {
        self.total_packets
    }
}

/// Send buffer - tracks what we're sending and what's been ACK'd
pub struct SendBuffer {
    /// Data being sent
    data: Vec<u8>,
    /// Packet size
    packet_size: u16,
    /// Total packets
    total_packets: u32,
    /// Bitmap of ACK'd packets
    acked: BitVec,
    /// Next sequence to send (for initial send)
    next_send: u32,
    /// Count of ACK'd packets
    acked_count: u32,
    /// Data hash
    data_hash: [u8; 32],
}

impl SendBuffer {
    /// Create send buffer from data
    pub fn new(data: Vec<u8>, packet_size: u16) -> Self {
        let total_packets = ((data.len() + packet_size as usize - 1) / packet_size as usize) as u32;
        let acked = bitvec![0; total_packets as usize];
        let data_hash = *blake3::hash(&data).as_bytes();

        Self {
            data,
            packet_size,
            total_packets,
            acked,
            next_send: 0,
            acked_count: 0,
            data_hash,
        }
    }

    /// Get packet payload for given sequence
    pub fn get_packet(&self, sequence: u32) -> Option<&[u8]> {
        if sequence >= self.total_packets {
            return None;
        }

        let start = sequence as usize * self.packet_size as usize;
        let end = ((sequence as usize + 1) * self.packet_size as usize).min(self.data.len());

        if start < self.data.len() {
            Some(&self.data[start..end])
        } else {
            None
        }
    }

    /// Get next sequence to send for initial send pass
    pub fn next_to_send(&mut self) -> Option<u32> {
        if self.next_send < self.total_packets {
            let seq = self.next_send;
            self.next_send += 1;
            Some(seq)
        } else {
            None
        }
    }

    /// Mark packet as ACK'd, returns true if new ACK
    pub fn mark_acked(&mut self, sequence: u32) -> bool {
        if sequence >= self.total_packets {
            return false;
        }

        let idx = sequence as usize;
        if self.acked[idx] {
            return false; // Duplicate ACK
        }

        self.acked.set(idx, true);
        self.acked_count += 1;
        true
    }

    /// Check if all packets have been ACK'd
    pub fn is_complete(&self) -> bool {
        self.acked_count == self.total_packets
    }

    /// Get list of un-ACK'd sequences (for retransmit)
    pub fn unacked_sequences(&self) -> Vec<u32> {
        self.acked
            .iter()
            .enumerate()
            .filter(|(_, acked)| !**acked)
            .map(|(i, _)| i as u32)
            .collect()
    }

    /// Get total packets
    pub fn total_packets(&self) -> u32 {
        self.total_packets
    }

    /// Get packet size
    pub fn packet_size(&self) -> u16 {
        self.packet_size
    }

    /// Get data hash
    pub fn data_hash(&self) -> [u8; 32] {
        self.data_hash
    }

    /// Get total data size
    pub fn total_size(&self) -> u32 {
        self.data.len() as u32
    }

    /// Get progress as (acked, total)
    pub fn progress(&self) -> (u32, u32) {
        (self.acked_count, self.total_packets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_receive_buffer_basic() {
        let data = vec![0xAB; 3000]; // 3 packets of 1000 bytes
        let hash = *blake3::hash(&data).as_bytes();

        let mut buf = ReceiveBuffer::new(3, 1000, 3000, hash);

        assert!(!buf.is_complete());
        assert_eq!(buf.missing_count(), 3);

        // Insert packet 0
        assert!(buf.insert(0, &data[0..1000]));
        assert!(!buf.is_complete());
        assert_eq!(buf.missing_count(), 2);

        // Duplicate should return false
        assert!(!buf.insert(0, &data[0..1000]));

        // Insert remaining packets
        assert!(buf.insert(1, &data[1000..2000]));
        assert!(buf.insert(2, &data[2000..3000]));

        assert!(buf.is_complete());
        assert!(buf.verify());

        let received = buf.take_data();
        assert_eq!(received, data);
    }

    #[test]
    fn test_receive_buffer_out_of_order() {
        let data = vec![0xCD; 5000];
        let hash = *blake3::hash(&data).as_bytes();

        let mut buf = ReceiveBuffer::new(5, 1000, 5000, hash);

        // Insert out of order
        buf.insert(4, &data[4000..5000]);
        buf.insert(0, &data[0..1000]);
        buf.insert(2, &data[2000..3000]);

        assert_eq!(buf.missing_sequences(), vec![1, 3]);

        buf.insert(1, &data[1000..2000]);
        buf.insert(3, &data[3000..4000]);

        assert!(buf.is_complete());
        assert!(buf.verify());
    }

    #[test]
    fn test_send_buffer() {
        let data = vec![0xEF; 2500]; // 3 packets
        let mut buf = SendBuffer::new(data.clone(), 1000);

        assert_eq!(buf.total_packets(), 3);
        assert!(!buf.is_complete());

        // Get packets
        assert_eq!(buf.get_packet(0).unwrap().len(), 1000);
        assert_eq!(buf.get_packet(1).unwrap().len(), 1000);
        assert_eq!(buf.get_packet(2).unwrap().len(), 500); // Last packet smaller

        // Next to send
        assert_eq!(buf.next_to_send(), Some(0));
        assert_eq!(buf.next_to_send(), Some(1));
        assert_eq!(buf.next_to_send(), Some(2));
        assert_eq!(buf.next_to_send(), None);

        // Mark ACKs
        assert!(buf.mark_acked(0));
        assert!(!buf.mark_acked(0)); // Duplicate
        buf.mark_acked(1);
        buf.mark_acked(2);

        assert!(buf.is_complete());
    }
}
