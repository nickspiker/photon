use blake3;
use i256::U256;
/// Proves computational work for handle registration.
///
/// Computes a deterministic proof-of-work for a given handle hash using memory-hard sequential processing. The same handle always produces the same public ID, enabling decentralized verification without coordination.
///
/// # Design Goals
///
/// - **Anti-squatting**: ~1 second per handle makes bulk registration expensive
/// - **Rate limiting**: Sequential rounds prevent parallelization across handles
/// - **ASIC resistance**: Data-dependent reads and variable memory usage resist hardware optimization
/// - **Deterministic**: Same handle → same ID (no salt, no randomization)
/// - **Verifiable**: Anyone can recompute to verify claimed handle ownership
///
/// # Algorithm
///
/// Each of 17 rounds consists of:
///
/// 0. **Variable fill determination**: Hash determines buffer fill (25-75% of 24MB)
/// 1. **Sequential hash chain**: Each chunk depends on previous chunk (non-seekable)
/// 2. **Data-dependent reads**: Random reads from earlier chunks (cache-hostile)
/// 3. **State advancement**: Round output becomes input to next round
///
/// # Security Properties
///
/// - **Memory-hard**: 24MB scratch buffer makes bulk parallelization expensive
/// - **Time-hard**: 17 sequential rounds, each feeding into the next
/// - **Unpredictable work**: Variable fill (25-75%) prevents precomputation attacks
/// - **Cache-hostile**: Data-dependent reads prevent efficient caching/ASICs
/// - **Forward secrecy**: Breaking round N doesn't reveal earlier rounds
///
/// # Performance
///
/// Tuned for approximately one second on 2025 desktop CPUs. Hardware improvements will reduce wall time but maintain economic cost (electricity, opportunity cost of CPU time).
///
/// # Arguments
///
/// * `hash` - BLAKE3 hash of the plaintext handle
///
/// # Returns
///
/// Final BLAKE3 hash after 17 rounds - this becomes the public ID (32 bytes)
///
/// # Examples
///
/// ```rust,ignore
/// let handle = "fractal decoder";
/// let vsf_bytes = vsf::VsfType::x(handle.to_string()).flatten();
/// let handle_hash = blake3::hash(&vsf_bytes);
/// let public_id = handle_proof(&handle_hash);
/// ```
const SIZE: usize = 24_873_856; // 24MB - fits in L3 cache, prevents bulk parallelization
const CHUNK_SIZE: usize = 32; // BLAKE3 output size
const ROUNDS: usize = 17; // Tuned for ~1s on 2025 hardware

pub fn handle_proof(hash: &blake3::Hash) -> blake3::Hash {
    // Allocate scratch buffer without initialization (for performance)
    let mut scratch = Vec::with_capacity(SIZE);
    // SAFETY: Buffer is completely filled by Phase 1 and Phase 2 before the final hash.
    // Phase 1 fills [0..fill_size], Phase 2 fills [fill_size..SIZE].
    // SIZE is exactly divisible by CHUNK_SIZE (24_873_856 / 32 = 777_308).
    unsafe {
        scratch.set_len(SIZE);
    }

    let mut round_hash = *hash;

    for round in 0..ROUNDS {
        // Mix round number into hash to prevent cross-round collisions
        let hash_num =
            U256::from_be_bytes(*round_hash.as_bytes()).wrapping_add(U256::from(round as u128));

        // Phase 0: Determine fill size (25-75% of buffer), aligned to CHUNK_SIZE
        // Alignment ensures Phase 1 fills exactly up to where Phase 2 starts (no gaps)
        let min_fill = SIZE / 4;
        let max_fill = SIZE * 3 / 4;
        let fill_range = max_fill - min_fill;
        let fill_size_raw =
            min_fill + ((hash_num % U256::from(fill_range as u128)).as_u128() as usize);
        let fill_size = (fill_size_raw / CHUNK_SIZE) * CHUNK_SIZE; // Align down to 32-byte boundary

        // Phase 1: Sequential hash chain (memory-hard, non-seekable)
        // Each chunk must be computed in order - no parallelization or seeking possible
        scratch[..CHUNK_SIZE].copy_from_slice(round_hash.as_bytes());

        for i in 1..(fill_size / CHUNK_SIZE) {
            let prev_start = (i - 1) * CHUNK_SIZE;
            let curr_start = i * CHUNK_SIZE;

            // Hash previous chunk
            let prev_hash = blake3::hash(&scratch[prev_start..prev_start + CHUNK_SIZE]);

            // Mix with round hash and position to ensure uniqueness
            let hash_num_out = U256::from_be_bytes(*prev_hash.as_bytes())
                .wrapping_add(hash_num)
                .wrapping_add(U256::from(i as u128));

            scratch[curr_start..curr_start + CHUNK_SIZE]
                .copy_from_slice(&hash_num_out.to_be_bytes());
        }

        // Phase 2: Data-dependent random reads (cache-hostile, ASIC-resistant)
        // Read location depends on previous hash value - unpredictable memory access pattern
        let mut curr_start = fill_size;

        while curr_start + CHUNK_SIZE <= SIZE {
            // Previous chunk determines where we read from
            let prev_hash_num = U256::from_be_bytes(
                scratch[curr_start - CHUNK_SIZE..curr_start]
                    .try_into()
                    .unwrap(),
            );

            // Random read from earlier in buffer
            let read_idx =
                (prev_hash_num % U256::from((curr_start - CHUNK_SIZE) as u128)).as_u128() as usize;

            // Hash the randomly-selected chunk
            let prev_hash = blake3::hash(&scratch[read_idx..read_idx + CHUNK_SIZE]);

            // Mix with round hash and position
            let new_val = U256::from_be_bytes(*prev_hash.as_bytes())
                .wrapping_add(hash_num)
                .wrapping_add(U256::from(curr_start as u128));

            scratch[curr_start..curr_start + CHUNK_SIZE].copy_from_slice(&new_val.to_be_bytes());
            curr_start += CHUNK_SIZE;
        }

        // Advance to next round: output of this round becomes input to next
        // Forces sequential processing of all 17 rounds
        round_hash = blake3::hash(&scratch);
    }

    // Final hash is the public ID - deterministic, verifiable, expensive to compute
    round_hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn bench_handle_proof() {
        let input = blake3::hash(b"handle");

        let start = Instant::now();
        let result = handle_proof(&input);
        let elapsed = start.elapsed();

        println!("handle_proof took: {:?}", elapsed);
        println!("Result: {}", result.to_hex());
    }
}
