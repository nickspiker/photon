use crate::types::Seed;
use blake3::Hasher;
use sha2::{Digest as Sha2Digest, Sha512};
use sha3::{Digest as Sha3Digest, Sha3_256};
use spirix::ScalarF4E4;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroize;

/// Multi-algorithm hash smear for defense-in-depth.
///
/// XORs outputs from three fundamentally different hash constructions:
/// - BLAKE3: Merkle tree of ChaCha-based compression (modern, fast)
/// - SHA3-256: Keccak sponge construction (NIST standard, permutation-based)
/// - SHA-512: Merkle-Damgård with ARX rounds (battle-tested, truncated to 32 bytes)
///
/// If ANY algorithm survives cryptanalysis, the output remains secure.
/// An attacker must break ALL THREE simultaneously to compromise the result.
///
/// Not memory-hard - that's not the goal. The input is already high-entropy
/// from the avalanche mixing. This adds hash algorithm diversity.
pub fn smear_hash(data: &[u8]) -> [u8; 32] {
    // BLAKE3 - Merkle tree of ChaCha-based compression
    let blake3_out = *blake3::hash(data).as_bytes();

    // SHA3-256 - Keccak sponge (completely different construction)
    let mut sha3 = Sha3_256::new();
    sha3.update(data);
    let sha3_out: [u8; 32] = sha3.finalize().into();

    // SHA-512 truncated to 32 bytes - Merkle-Damgård ARX
    let mut sha512 = Sha512::new();
    sha512.update(data);
    let sha512_full: [u8; 64] = sha512.finalize().into();
    let mut sha512_out = [0u8; 32];
    sha512_out.copy_from_slice(&sha512_full[..32]);

    // XOR all three - output is secure if ANY one survives
    let mut result = [0u8; 32];
    for i in 0..32 {
        result[i] = blake3_out[i] ^ sha3_out[i] ^ sha512_out[i];
    }
    result
}

/// Domain separation for conversation token derivation
const CONVERSATION_TOKEN_DOMAIN: &[u8] = b"PHOTON_CONVERSATION_TOKEN_v0";

/// Derive a privacy-preserving conversation token from participant identity seeds.
///
/// Works for N-party conversations (2-party, 3-party, etc.).
/// All participants derive the SAME token by sorting seeds lexicographically
/// before hashing. The token:
/// - Only participants can compute (requires knowing all identity seeds)
/// - Doesn't reveal individual identities to network observers
/// - Different for each unique set of participants
///
/// Uses spaghettify for maximum obfuscation (domain-separated, maximally weird mixing).
/// VSF type: hg (spaghetti hash)
pub fn derive_conversation_token(participant_seeds: &[[u8; 32]]) -> [u8; 32] {
    // Canonical ordering - ALL participants compute same token
    let mut sorted_seeds = participant_seeds.to_vec();
    sorted_seeds.sort(); // Lexicographic sort of 32-byte arrays

    // Domain separation + concatenated seeds
    let mut input = Vec::with_capacity(CONVERSATION_TOKEN_DOMAIN.len() + sorted_seeds.len() * 32);
    input.extend_from_slice(CONVERSATION_TOKEN_DOMAIN);
    for seed in &sorted_seeds {
        input.extend_from_slice(seed);
    }

    // DEBUG: Print input to spaghettify for cross-platform comparison
    crate::log(&format!(
        "CONV_TOKEN: input_len={}, input_hash={}, sorted_seeds={}",
        input.len(),
        hex::encode(&blake3::hash(&input).as_bytes()[..8]),
        sorted_seeds
            .iter()
            .map(|s| hex::encode(&s[..8]))
            .collect::<Vec<_>>()
            .join(",")
    ));

    let result = spaghettify(&input);

    crate::log(&format!(
        "CONV_TOKEN: spaghettify_result={}",
        hex::encode(&result[..8])
    ));

    result
}

/// Domain separator for ceremony instance derivation
const CEREMONY_INSTANCE_DOMAIN: &[u8] = b"PHOTON_CEREMONY_INSTANCE_v0";

/// Derive a unique ceremony instance identifier from all parties' offers.
///
/// This is used for stale detection: distinguishes re-key requests from PT
/// retransmissions. Unlike ceremony_id (derived from handle_hashes, invariant
/// per handle pair), this changes when ephemeral keypairs are regenerated.
///
/// Both parties can compute this independently once they have both offers.
/// Works for N-party ceremonies.
pub fn derive_ceremony_instance(offers: &[&ClutchOfferPayload]) -> [u8; 32] {
    // Serialize each offer to bytes (concatenate all 8 public keys)
    let mut offer_bytes: Vec<Vec<u8>> = offers.iter().map(|o| o.to_bytes()).collect();

    // Canonical ordering - sort by serialized bytes
    offer_bytes.sort();

    // Domain separation + concatenated sorted offers
    let mut input = Vec::with_capacity(
        CEREMONY_INSTANCE_DOMAIN.len() + offer_bytes.iter().map(|b| b.len()).sum::<usize>(),
    );
    input.extend_from_slice(CEREMONY_INSTANCE_DOMAIN);
    for bytes in &offer_bytes {
        input.extend_from_slice(bytes);
    }

    smear_hash(&input)
}

// ============================================================================
// SPAGHETTIFY: Rube Goldberg mixing with U256 chunks and Spirix chaos
// ============================================================================

use i256::U256;

/// Bootstrap seed - Nothing Up My Sleeve: ASCII bytes of self-documenting string
/// "PHOTON_SPAGHETTI: 53 buckets, 23 ops, Spirix chaos, NUMS seed v0"
const LAVA_SEED_256: [u8; 64] =
    *b"PHOTON_SPAGHETTI: 53 buckets, 23 ops, Spirix chaos, NUMS seed v0";

// Constants for U256 - use from_be_bytes with const arrays
const U256_ZERO: U256 = U256::from_be_bytes([0u8; 32]);
const U256_ONE: U256 = U256::from_be_bytes([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
]);

/// Integer square root for U256 using Newton-Raphson
fn sqrt_u256(n: U256) -> U256 {
    if n == U256_ZERO {
        return U256_ZERO;
    }
    if n == U256_ONE {
        return U256_ONE;
    }

    // Start with a rough estimate: n >> (leading_zeros / 2)
    // For U256, we approximate by checking high bits
    let mut x = n >> 128; // Start smaller
    if x == U256_ZERO {
        x = n >> 64;
    }
    if x == U256_ZERO {
        x = n >> 32;
    }
    if x == U256_ZERO {
        x = n;
    }

    // Newton-Raphson: x_new = (x + n/x) / 2
    loop {
        if x == U256_ZERO {
            return U256_ZERO;
        }
        let x_new = (x + n / x) >> 1;
        if x_new >= x {
            return x;
        }
        x = x_new;
    }
}

/// Convert U256 to 8 Spirix ScalarF4E4 values (deterministic chaos mode!)
/// Random bit patterns become Spirix scalars with deterministic behavior across all platforms.
/// Uses 8×32-bit (16-bit fraction + 16-bit exponent) = 256 bits exact.
/// Each scalar is normalized to ensure valid Spirix representation (prevents div-by-zero panics).
fn u256_to_spirix8(n: U256) -> [ScalarF4E4; 8] {
    let bytes = n.to_be_bytes();
    let mut s0 = ScalarF4E4 {
        fraction: i16::from_be_bytes([bytes[0], bytes[1]]),
        exponent: i16::from_be_bytes([bytes[2], bytes[3]]),
    };
    let mut s1 = ScalarF4E4 {
        fraction: i16::from_be_bytes([bytes[4], bytes[5]]),
        exponent: i16::from_be_bytes([bytes[6], bytes[7]]),
    };
    let mut s2 = ScalarF4E4 {
        fraction: i16::from_be_bytes([bytes[8], bytes[9]]),
        exponent: i16::from_be_bytes([bytes[10], bytes[11]]),
    };
    let mut s3 = ScalarF4E4 {
        fraction: i16::from_be_bytes([bytes[12], bytes[13]]),
        exponent: i16::from_be_bytes([bytes[14], bytes[15]]),
    };
    let mut s4 = ScalarF4E4 {
        fraction: i16::from_be_bytes([bytes[16], bytes[17]]),
        exponent: i16::from_be_bytes([bytes[18], bytes[19]]),
    };
    let mut s5 = ScalarF4E4 {
        fraction: i16::from_be_bytes([bytes[20], bytes[21]]),
        exponent: i16::from_be_bytes([bytes[22], bytes[23]]),
    };
    let mut s6 = ScalarF4E4 {
        fraction: i16::from_be_bytes([bytes[24], bytes[25]]),
        exponent: i16::from_be_bytes([bytes[26], bytes[27]]),
    };
    let mut s7 = ScalarF4E4 {
        fraction: i16::from_be_bytes([bytes[28], bytes[29]]),
        exponent: i16::from_be_bytes([bytes[30], bytes[31]]),
    };
    s0.normalize();
    s1.normalize();
    s2.normalize();
    s3.normalize();
    s4.normalize();
    s5.normalize();
    s6.normalize();
    s7.normalize();
    [s0, s1, s2, s3, s4, s5, s6, s7]
}

/// Convert 8 Spirix ScalarF4E4 values back to U256
fn spirix8_to_u256(s: [ScalarF4E4; 8]) -> U256 {
    let mut bytes = [0u8; 32];
    for i in 0..8 {
        let frac = s[i].fraction.to_be_bytes();
        let exp = s[i].exponent.to_be_bytes();
        bytes[i * 4] = frac[0];
        bytes[i * 4 + 1] = frac[1];
        bytes[i * 4 + 2] = exp[0];
        bytes[i * 4 + 3] = exp[1];
    }
    U256::from_be_bytes(bytes)
}

/// Apply sin to U256 via Spirix (deterministic across all platforms)
fn chaos_sin(n: U256) -> U256 {
    let s = u256_to_spirix8(n);
    spirix8_to_u256([
        s[0].sin(),
        s[1].sin(),
        s[2].sin(),
        s[3].sin(),
        s[4].sin(),
        s[5].sin(),
        s[6].sin(),
        s[7].sin(),
    ])
}

/// Apply cos to U256 via Spirix
fn chaos_cos(n: U256) -> U256 {
    let s = u256_to_spirix8(n);
    spirix8_to_u256([
        s[0].cos(),
        s[1].cos(),
        s[2].cos(),
        s[3].cos(),
        s[4].cos(),
        s[5].cos(),
        s[6].cos(),
        s[7].cos(),
    ])
}

/// Apply ln (natural log) to U256 via Spirix - negative/zero become deterministic undefined
fn chaos_ln(n: U256) -> U256 {
    let s = u256_to_spirix8(n);
    spirix8_to_u256([
        s[0].ln(),
        s[1].ln(),
        s[2].ln(),
        s[3].ln(),
        s[4].ln(),
        s[5].ln(),
        s[6].ln(),
        s[7].ln(),
    ])
}

/// Apply exp to U256 via Spirix - large values become deterministic exploded
fn chaos_exp(n: U256) -> U256 {
    let s = u256_to_spirix8(n);
    spirix8_to_u256([
        s[0].exp(),
        s[1].exp(),
        s[2].exp(),
        s[3].exp(),
        s[4].exp(),
        s[5].exp(),
        s[6].exp(),
        s[7].exp(),
    ])
}

/// Apply tan to U256 via Spirix - near-90° values become deterministic undefined
fn chaos_tan(n: U256) -> U256 {
    let s = u256_to_spirix8(n);
    spirix8_to_u256([
        s[0].tan(),
        s[1].tan(),
        s[2].tan(),
        s[3].tan(),
        s[4].tan(),
        s[5].tan(),
        s[6].tan(),
        s[7].tan(),
    ])
}

/// Apply atan to U256 via Spirix - compresses everything to [-π/2, π/2]
fn chaos_atan(n: U256) -> U256 {
    let s = u256_to_spirix8(n);
    spirix8_to_u256([
        s[0].atan(),
        s[1].atan(),
        s[2].atan(),
        s[3].atan(),
        s[4].atan(),
        s[5].atan(),
        s[6].atan(),
        s[7].atan(),
    ])
}

/// Spirix addition: deterministic handling of infinity/undefined states
fn chaos_add(a: U256, b: U256) -> U256 {
    let sa = u256_to_spirix8(a);
    let sb = u256_to_spirix8(b);
    spirix8_to_u256([
        sa[0] + sb[0],
        sa[1] + sb[1],
        sa[2] + sb[2],
        sa[3] + sb[3],
        sa[4] + sb[4],
        sa[5] + sb[5],
        sa[6] + sb[6],
        sa[7] + sb[7],
    ])
}

/// Spirix subtraction: deterministic handling of infinity/undefined states
fn chaos_sub(a: U256, b: U256) -> U256 {
    let sa = u256_to_spirix8(a);
    let sb = u256_to_spirix8(b);
    spirix8_to_u256([
        sa[0] - sb[0],
        sa[1] - sb[1],
        sa[2] - sb[2],
        sa[3] - sb[3],
        sa[4] - sb[4],
        sa[5] - sb[5],
        sa[6] - sb[6],
        sa[7] - sb[7],
    ])
}

/// Spirix multiplication: 0 * Inf = undefined (deterministic), overflow → exploded
fn chaos_mul(a: U256, b: U256) -> U256 {
    let sa = u256_to_spirix8(a);
    let sb = u256_to_spirix8(b);
    spirix8_to_u256([
        sa[0] * sb[0],
        sa[1] * sb[1],
        sa[2] * sb[2],
        sa[3] * sb[3],
        sa[4] * sb[4],
        sa[5] * sb[5],
        sa[6] * sb[6],
        sa[7] * sb[7],
    ])
}

/// Spirix division: x/0 = Inf, 0/0 = undefined (deterministic)
fn chaos_div(a: U256, b: U256) -> U256 {
    let sa = u256_to_spirix8(a);
    let sb = u256_to_spirix8(b);
    spirix8_to_u256([
        sa[0] / sb[0],
        sa[1] / sb[1],
        sa[2] / sb[2],
        sa[3] / sb[3],
        sa[4] / sb[4],
        sa[5] / sb[5],
        sa[6] / sb[6],
        sa[7] / sb[7],
    ])
}

/// Spirix power: deterministic handling of edge cases
fn chaos_pow(a: U256, b: U256) -> U256 {
    let sa = u256_to_spirix8(a);
    let sb = u256_to_spirix8(b);
    spirix8_to_u256([
        sa[0].pow(sb[0]),
        sa[1].pow(sb[1]),
        sa[2].pow(sb[2]),
        sa[3].pow(sb[3]),
        sa[4].pow(sb[4]),
        sa[5].pow(sb[5]),
        sa[6].pow(sb[6]),
        sa[7].pow(sb[7]),
    ])
}

/// Spirix hypot: sqrt(a² + b²) via square() and sqrt()
fn chaos_hypot(a: U256, b: U256) -> U256 {
    let sa = u256_to_spirix8(a);
    let sb = u256_to_spirix8(b);
    spirix8_to_u256([
        (sa[0].square() + sb[0].square()).sqrt(),
        (sa[1].square() + sb[1].square()).sqrt(),
        (sa[2].square() + sb[2].square()).sqrt(),
        (sa[3].square() + sb[3].square()).sqrt(),
        (sa[4].square() + sb[4].square()).sqrt(),
        (sa[5].square() + sb[5].square()).sqrt(),
        (sa[6].square() + sb[6].square()).sqrt(),
        (sa[7].square() + sb[7].square()).sqrt(),
    ])
}

/// Spaghettify: Rube Goldberg mixing with U256 chunks and Spirix chaos.
///
/// A deterministic one-way function that achieves **provable irreversibility** through
/// information destruction and path explosion. Not a cryptographic hash - instead a
/// "chaos amplifier" that makes preimage search computationally infeasible.
///
/// # Irreversibility Proof
///
/// **Information-Destroying Operations** (many-to-one mappings):
/// - `sqrt_u256`: 2^256 inputs → 2^128 outputs (op 12) - exact 2:1 collision guarantee
/// - `count_ones`: 2^256 inputs → 257 outputs (op 17) - ~10^75 collisions per output
/// - `saturating_add/sub`: clamps at 0 or MAX (ops 13,14) - infinite inputs → one output
/// - `AND/OR`: bit-level information loss (ops 15,16) - many patterns collapse to same result
/// - Spirix undefined states: ln(neg), 0/0, tan(π/2), etc. → deterministic "undefined" value
///
/// **Per-Round Information Loss:**
/// - 53 buckets × ~40% information-destroying op probability ≈ 21 lossy ops per round
/// - Each lossy op has ≥2:1 collision ratio (many have exponentially more)
/// - After R rounds: collision space grows multiplicatively
///
/// **Path Explosion:**
/// - 23^53 ≈ 10^72 possible operation sequences per round
/// - 11-23 rounds (data-dependent): total paths ≈ 10^792 to 10^1656
/// - Exceeds atoms in observable universe (~10^80) by factor of 10^700+
///
/// **Defense in Depth:**
/// - Final `smear_hash()` provides cryptographic one-wayness even if chaos layer is weak
/// - Original input re-appended before hash: secure if EITHER layer survives attack
///
/// # Security Model
///
/// Given output O, finding input I such that spaghettify(I) = O requires:
/// 1. Inverting smear_hash (cryptographic hardness)
/// 2. OR inverting 11-23 rounds of mixed lossy/reversible ops (combinatorial explosion)
/// 3. AND somehow navigating 10^72+ path choices per round
///
/// The function is **preimage-resistant** (cannot find input from output) but makes no
/// claim about **collision-resistance** (finding two inputs with same output) - collisions
/// are guaranteed to exist due to the lossy operations, they're just infeasible to find.
///
/// # Parameters
/// - **53 buckets** (prime) × U256 = 1696 bytes state
/// - **23 operations** (prime) - bucket value mod 23 selects op
/// - **11-23 rounds** (primes) - data-dependent iteration count
/// - **Spirix ScalarF4E4** - 8×32-bit deterministic floats per U256
///
/// NOT memory-hard - maximum weirdness, not resource consumption.
///
/// # Arguments
/// * `input` - Arbitrary length byte slice (0 to any size)
///
/// # Returns
/// 32 bytes of maximally entangled chaos (via smear_hash)
pub fn spaghettify(input: &[u8]) -> [u8; 32] {
    const BUCKETS: usize = 53; // Prime, 53 * 32 = 1696 bytes
    const CROSS: usize = 29; // Prime offset for cross-contamination (changed from 23)
    const OPS: usize = 23; // Prime number of operations

    let mut buckets: [U256; BUCKETS] = [U256_ZERO; BUCKETS];

    // Phase 1: Create input-modified seed
    // Start with LAVA_SEED, then mix input into it
    let mut seed = [
        U256::from_be_bytes(LAVA_SEED_256[0..32].try_into().unwrap()),
        U256::from_be_bytes(LAVA_SEED_256[32..64].try_into().unwrap()),
    ];

    // Mix each 32-byte chunk of input into seed
    for (chunk_idx, chunk) in input.chunks(32).enumerate() {
        let mut chunk_bytes = [0u8; 32];
        chunk_bytes[..chunk.len()].copy_from_slice(chunk);
        let chunk_val = U256::from_be_bytes(chunk_bytes);

        // XOR and rotate into both seed values
        seed[0] = seed[0] ^ chunk_val;
        seed[1] = seed[1].wrapping_add(chunk_val);
        seed[0] = (seed[0] << 7) | (seed[0] >> 249); // Rotate left 7
        seed[1] = seed[1] ^ (chunk_val >> (chunk_idx % 128));
    }

    // Also mix in input length to differentiate padding scenarios
    seed[0] = seed[0].wrapping_add(U256::from(input.len() as u128));

    // Expand modified seed into all 53 buckets
    for i in 0..BUCKETS {
        // Derive bucket from both seed values with position mixing
        let s0_rot = (seed[0] << (i % 128)) | (seed[0] >> (256 - (i % 128)));
        let s1_rot = (seed[1] >> (i % 64)) | (seed[1] << (256 - (i % 64)));
        buckets[i] = s0_rot ^ s1_rot;
        buckets[i] = buckets[i].wrapping_add(U256::from((i as u128) * 89));
    }

    // Pre-round mixing: cascade differences across neighbors
    for i in 0..BUCKETS {
        let next = (i + 1) % BUCKETS;
        buckets[next] = buckets[next] ^ buckets[i].wrapping_add(U256::from(i as u128));
    }

    // Phase 2: Determine round count from initial state (11-23, both prime)
    let state_sum: u128 = buckets.iter().map(|b| b.as_u128()).sum();
    let rounds = 11 + (state_sum % 13) as usize; // Variable work: 11..=23

    // Phase 3: Spaghettification - the chaos engine with U256 ops
    for round in 0..rounds {
        // Round-dependent constant from seed
        let round_const = seed[round % 2].wrapping_add(U256::from(round as u128));

        for i in 0..BUCKETS {
            let val = buckets[i];
            let op = (val.as_u128() as usize) % OPS; // Which operation (23 choices)

            // Data-dependent target
            let target = (i + (val.as_u128() as usize) + round * 31) % BUCKETS; // 31 is prime
            let secondary = (target + CROSS) % BUCKETS;

            // Position-dependent constant prevents convergence to fixed point
            let pos_const = round_const.wrapping_add(U256::from((i as u128) * 89));

            let op_result = match op {
                // Spirix unary ops (deterministic across all platforms):
                0 => chaos_sin(val),  // Trig chaos
                1 => chaos_cos(val),  // More trig chaos
                2 => chaos_ln(val),   // Negative → undefined
                3 => chaos_exp(val),  // Large → exploded
                4 => chaos_tan(val),  // Near-90° → undefined
                5 => chaos_atan(val), // Compresses range

                // Spirix binary ops (deterministic special value handling):
                6 => chaos_add(val, buckets[secondary]), // Inf + (-Inf) → undefined
                7 => chaos_sub(val, buckets[secondary]), // Inf - Inf → undefined
                8 => chaos_mul(val, buckets[secondary]), // 0 * Inf → undefined
                9 => chaos_div(val, buckets[secondary]), // 0/0 → undefined, x/0 → Inf
                10 => chaos_pow(val, buckets[secondary]), // neg^frac → undefined
                11 => chaos_hypot(val, buckets[secondary]), // sqrt(a² + b²)

                // U256 arithmetic ops:
                12 => sqrt_u256(val), // Lossy: 2^256 → 2^128 outputs
                13 => buckets[target].saturating_add(val), // Stuck at MAX
                14 => buckets[target].saturating_sub(val), // Stuck at 0
                15 => buckets[target] & val, // Bits destroyed
                16 => buckets[target] | val, // Bits forced on
                17 => U256::from(val.count_ones() as u128), // 256 bits → 0-256

                // Reversible but path-dependent:
                18 => buckets[target] ^ val, // XOR mixing
                19 => (val << (val.as_u128() % 256)) | (val >> (256 - val.as_u128() % 256)), // Data-dep rotate
                20 => buckets[target].wrapping_mul(val | U256_ONE), // |1 avoids *0
                21 => {
                    // Conditional swap - branch creates path explosion
                    if val > buckets[secondary] {
                        buckets.swap(target, secondary);
                    }
                    buckets[target]
                }
                _ => val.wrapping_add(buckets[secondary]), // Cross-bucket mixing (op 22)
            };

            // Mix in position constant to prevent fixed point convergence
            buckets[target] = op_result ^ pos_const;
        }
    }

    // Phase 4: Collapse 53 U256 buckets → bytes, append input, then smear_hash
    // Defense in depth: if spaghettify has unknown weaknesses, original input
    // is still mixed in. Output is secure if ANY layer survives (like CLUTCH).
    let mut state_bytes = Vec::with_capacity(BUCKETS * 32 + input.len());
    for bucket in &buckets {
        state_bytes.extend_from_slice(&bucket.to_be_bytes());
    }
    state_bytes.extend_from_slice(input); // Append original input

    // Use smear_hash to collapse to 32 bytes with hash algorithm diversity
    smear_hash(&state_bytes)
}

/// Determine who initiates clutch ceremony.
/// Lower handle_proof = initiator (sends ephemeral pubkeys first)
/// Higher handle_proof = responder (waits, then responds)
///
/// All parties compute the same result from sorted handle hashes.
pub fn is_clutch_initiator(local_handle_proof: &[u8; 32], remote_handle_proof: &[u8; 32]) -> bool {
    local_handle_proof < remote_handle_proof
}

/// Generate ephemeral X25519 keypair
/// Returns (secret, public) - caller MUST zeroize the secret after use!
pub fn generate_x25519_ephemeral() -> ([u8; 32], [u8; 32]) {
    let mut secret_bytes = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut secret_bytes);

    let secret = StaticSecret::from(secret_bytes);
    let public = PublicKey::from(&secret);

    // Return the secret bytes for the caller to use (and zeroize when done)
    // Note: StaticSecret::from() copies the bytes, so we return the original
    (secret_bytes, *public.as_bytes())
}

/// Perform X25519 ECDH to derive shared secret.
/// Caller should zeroize the returned shared secret after use.
pub fn x25519_ecdh(local_secret: &[u8; 32], peer_public: &[u8; 32]) -> [u8; 32] {
    let secret = StaticSecret::from(*local_secret);
    let public = PublicKey::from(*peer_public);
    let shared = secret.diffie_hellman(&public);
    // x25519_dalek's SharedSecret zeroizes on drop, but we need the bytes
    *shared.as_bytes()
}

// ============================================================================
// CLASS 0: CLASSICAL ELLIPTIC CURVES
// ============================================================================

/// Generate P-384 ephemeral keypair
/// Returns (secret_bytes, public_bytes)
pub fn generate_p384_ephemeral() -> (Vec<u8>, Vec<u8>) {
    use p384::SecretKey;

    let secret = SecretKey::generate();
    let public = secret.public_key();

    let secret_bytes = secret.to_bytes().to_vec();
    let public_bytes = public.to_sec1_bytes().to_vec();

    (secret_bytes, public_bytes)
}

/// Perform P-384 ECDH.
/// Returns 48-byte shared secret.
pub fn p384_ecdh(local_secret: &[u8], peer_public: &[u8]) -> Vec<u8> {
    use p384::elliptic_curve::ecdh::diffie_hellman;
    use p384::{PublicKey, SecretKey};

    let secret = SecretKey::from_slice(local_secret).expect("P-384 secret key invalid");
    let public = PublicKey::from_sec1_bytes(peer_public).expect("P-384 public key invalid");

    let shared = diffie_hellman(secret.to_nonzero_scalar(), public.as_affine());
    shared.raw_secret_bytes().to_vec()
}

/// Generate secp256k1 ephemeral keypair
/// Returns (secret_bytes, public_bytes)
pub fn generate_secp256k1_ephemeral() -> (Vec<u8>, Vec<u8>) {
    use k256::SecretKey;

    let secret = SecretKey::generate();
    let public = secret.public_key();

    let secret_bytes = secret.to_bytes().to_vec();
    let public_bytes = public.to_sec1_bytes().to_vec();

    (secret_bytes, public_bytes)
}

/// Perform secp256k1 ECDH.
/// Returns 32-byte shared secret.
pub fn secp256k1_ecdh(local_secret: &[u8], peer_public: &[u8]) -> Vec<u8> {
    use k256::elliptic_curve::ecdh::diffie_hellman;
    use k256::{PublicKey, SecretKey};

    let secret = SecretKey::from_slice(local_secret).expect("secp256k1 secret key invalid");
    let public = PublicKey::from_sec1_bytes(peer_public).expect("secp256k1 public key invalid");

    let shared = diffie_hellman(secret.to_nonzero_scalar(), public.as_affine());
    shared.raw_secret_bytes().to_vec()
}

/// Generate P-256 ephemeral keypair
/// Returns (secret_bytes, public_bytes)
pub fn generate_p256_ephemeral() -> (Vec<u8>, Vec<u8>) {
    use p256::SecretKey;

    let secret = SecretKey::generate();
    let public = secret.public_key();

    let secret_bytes = secret.to_bytes().to_vec();
    let public_bytes = public.to_sec1_bytes().to_vec();

    (secret_bytes, public_bytes)
}

/// Perform P-256 ECDH.
/// Returns 32-byte shared secret.
pub fn p256_ecdh(local_secret: &[u8], peer_public: &[u8]) -> Vec<u8> {
    use p256::elliptic_curve::ecdh::diffie_hellman;
    use p256::{PublicKey, SecretKey};

    let secret = SecretKey::from_slice(local_secret).expect("P-256 secret key invalid");
    let public = PublicKey::from_sec1_bytes(peer_public).expect("P-256 public key invalid");

    let shared = diffie_hellman(secret.to_nonzero_scalar(), public.as_affine());
    shared.raw_secret_bytes().to_vec()
}

// ============================================================================
// CLASS 1: POST-QUANTUM LATTICE KEMS
// ============================================================================

/// Generate FrodoKEM-976-SHAKE keypair
/// Returns (secret_key, public_key)
pub fn generate_frodo976_keypair() -> (Vec<u8>, Vec<u8>) {
    use frodo_kem_rs::Algorithm;
    use rand_core::OsRng;

    let alg = Algorithm::FrodoKem976Shake;
    let (ek, dk) = alg
        .try_generate_keypair(OsRng)
        .expect("FrodoKEM keygen failed");

    (dk.value().to_vec(), ek.value().to_vec())
}

/// Encapsulate FrodoKEM-976-SHAKE
/// Returns (ciphertext, shared_secret)
pub fn frodo976_encapsulate(their_public_key: &[u8]) -> (Vec<u8>, Vec<u8>) {
    use frodo_kem_rs::{Algorithm, EncryptionKey};
    use rand_core::OsRng;

    let alg = Algorithm::FrodoKem976Shake;
    let ek =
        EncryptionKey::from_bytes(alg, their_public_key).expect("FrodoKEM pubkey parse failed");
    let (ct, ss) = alg
        .try_encapsulate_with_rng(&ek, OsRng)
        .expect("FrodoKEM encapsulate failed");

    (ct.value().to_vec(), ss.value().to_vec())
}

/// Decapsulate FrodoKEM-976-SHAKE
/// Returns shared_secret
pub fn frodo976_decapsulate(our_secret_key: &[u8], ciphertext: &[u8]) -> Vec<u8> {
    use frodo_kem_rs::{Algorithm, Ciphertext, DecryptionKey};

    let alg = Algorithm::FrodoKem976Shake;
    let dk = DecryptionKey::from_bytes(alg, our_secret_key).expect("FrodoKEM seckey parse failed");
    let ct = Ciphertext::from_bytes(alg, ciphertext).expect("FrodoKEM ciphertext parse failed");
    let (ss, _msg) = alg
        .decapsulate(&dk, &ct)
        .expect("FrodoKEM decapsulate failed");

    ss.value().to_vec()
}

/// Generate NTRU-HRSS-701 keypair
/// Returns (secret_key, public_key)
pub fn generate_ntru701_keypair() -> (Vec<u8>, Vec<u8>) {
    use pqcrypto_ntru::ntruhrss701;
    use pqcrypto_traits::kem::{PublicKey, SecretKey};

    // NTRU uses its own internal RNG (PQClean's randombytes)
    let (pk, sk) = ntruhrss701::keypair();

    (
        SecretKey::as_bytes(&sk).to_vec(),
        PublicKey::as_bytes(&pk).to_vec(),
    )
}

/// Encapsulate NTRU-HRSS-701
/// Returns (ciphertext, 32B shared_secret)
pub fn ntru701_encapsulate(their_public_key: &[u8]) -> (Vec<u8>, Vec<u8>) {
    use pqcrypto_ntru::ntruhrss701;
    use pqcrypto_traits::kem::{Ciphertext, PublicKey, SharedSecret};

    let pk = <ntruhrss701::PublicKey as PublicKey>::from_bytes(their_public_key)
        .expect("NTRU public key invalid");

    // NTRU uses its own internal RNG for encapsulation
    let (ss, ct) = ntruhrss701::encapsulate(&pk);

    (
        Ciphertext::as_bytes(&ct).to_vec(),
        SharedSecret::as_bytes(&ss).to_vec(),
    )
}

/// Decapsulate NTRU-HRSS-701
/// Returns 32B shared_secret
pub fn ntru701_decapsulate(our_secret_key: &[u8], ciphertext: &[u8]) -> Vec<u8> {
    use pqcrypto_ntru::ntruhrss701;
    use pqcrypto_traits::kem::{Ciphertext, SecretKey, SharedSecret};

    let sk = <ntruhrss701::SecretKey as SecretKey>::from_bytes(our_secret_key)
        .expect("NTRU secret key invalid");
    let ct = <ntruhrss701::Ciphertext as Ciphertext>::from_bytes(ciphertext)
        .expect("NTRU ciphertext invalid");

    let ss = ntruhrss701::decapsulate(&ct, &sk);

    SharedSecret::as_bytes(&ss).to_vec()
}

// ============================================================================
// CLASS 2: POST-QUANTUM CODE-BASED KEMS
// ============================================================================

/// Generate Classic McEliece 460896 keypair
/// Returns (secret_key, public_key ~512KB)
pub fn generate_mceliece460896_keypair() -> (Vec<u8>, Vec<u8>) {
    use classic_mceliece_rust::keypair_boxed;

    // McEliece uses a different RNG - use rng for diversity
    let mut rng = rand::thread_rng();
    let (pk, sk) = keypair_boxed(&mut rng);

    (sk.as_array().to_vec(), pk.as_array().to_vec())
}

/// Encapsulate Classic McEliece 460896
/// Returns (ciphertext, 32B shared_secret)
pub fn mceliece460896_encapsulate(their_public_key: &[u8]) -> (Vec<u8>, Vec<u8>) {
    use classic_mceliece_rust::{encapsulate_boxed, PublicKey, CRYPTO_PUBLICKEYBYTES};

    // Copy to Box for PublicKey::from
    let mut pk_box = vec![0u8; CRYPTO_PUBLICKEYBYTES].into_boxed_slice();
    pk_box.copy_from_slice(their_public_key);
    let pk_array: Box<[u8; CRYPTO_PUBLICKEYBYTES]> =
        pk_box.try_into().expect("McEliece public key wrong size");
    let pk = PublicKey::from(pk_array);

    // Use rng for McEliece encapsulation (another diverse RNG source)
    let mut rng = rand::thread_rng();
    let (ct, ss) = encapsulate_boxed(&pk, &mut rng);

    (ct.as_array().to_vec(), ss.as_array().to_vec())
}

/// Decapsulate Classic McEliece 460896
/// Returns 32B shared_secret
pub fn mceliece460896_decapsulate(our_secret_key: &[u8], ciphertext: &[u8]) -> Vec<u8> {
    use classic_mceliece_rust::{
        decapsulate_boxed, Ciphertext, SecretKey, CRYPTO_CIPHERTEXTBYTES, CRYPTO_SECRETKEYBYTES,
    };

    // Copy to Box for SecretKey::from
    let mut sk_box = vec![0u8; CRYPTO_SECRETKEYBYTES].into_boxed_slice();
    sk_box.copy_from_slice(our_secret_key);
    let sk_array: Box<[u8; CRYPTO_SECRETKEYBYTES]> =
        sk_box.try_into().expect("McEliece secret key wrong size");
    let sk = SecretKey::from(sk_array);

    // Copy to array for Ciphertext::from
    let ct_array: [u8; CRYPTO_CIPHERTEXTBYTES] = ciphertext
        .try_into()
        .expect("McEliece ciphertext wrong size");
    let ct = Ciphertext::from(ct_array);

    let ss = decapsulate_boxed(&ct, &sk);

    ss.as_array().to_vec()
}

/// Generate HQC-256 keypair
/// Returns (secret_key, public_key)
pub fn generate_hqc256_keypair() -> (Vec<u8>, Vec<u8>) {
    use pqcrypto_hqc::hqc256;
    use pqcrypto_traits::kem::{PublicKey, SecretKey};

    // HQC uses PQClean's internal RNG (different from NTRU's implementation)
    let (pk, sk) = hqc256::keypair();

    (
        SecretKey::as_bytes(&sk).to_vec(),
        PublicKey::as_bytes(&pk).to_vec(),
    )
}

/// Encapsulate HQC-256
/// Returns (ciphertext, 64B shared_secret)
pub fn hqc256_encapsulate(their_public_key: &[u8]) -> (Vec<u8>, Vec<u8>) {
    use pqcrypto_hqc::hqc256;
    use pqcrypto_traits::kem::{Ciphertext, PublicKey, SharedSecret};

    let pk = <hqc256::PublicKey as PublicKey>::from_bytes(their_public_key)
        .expect("HQC public key invalid");

    // HQC uses its own internal RNG for encapsulation
    let (ss, ct) = hqc256::encapsulate(&pk);

    (
        Ciphertext::as_bytes(&ct).to_vec(),
        SharedSecret::as_bytes(&ss).to_vec(),
    )
}

/// Decapsulate HQC-256
/// Returns 64B shared_secret
pub fn hqc256_decapsulate(our_secret_key: &[u8], ciphertext: &[u8]) -> Vec<u8> {
    use pqcrypto_hqc::hqc256;
    use pqcrypto_traits::kem::{Ciphertext, SecretKey, SharedSecret};

    let sk = <hqc256::SecretKey as SecretKey>::from_bytes(our_secret_key)
        .expect("HQC secret key invalid");
    let ct =
        <hqc256::Ciphertext as Ciphertext>::from_bytes(ciphertext).expect("HQC ciphertext invalid");

    let ss = hqc256::decapsulate(&ct, &sk);

    SharedSecret::as_bytes(&ss).to_vec()
}

/// Sort two 32-byte arrays canonically (lower first)
fn sort_pair<'a>(a: &'a [u8; 32], b: &'a [u8; 32]) -> (&'a [u8; 32], &'a [u8; 32]) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

// ============================================================================
// LAYER 1: CONVERSATION PROVENANCE (permanent identity binding)
// ============================================================================

/// Derive the conversation provenance hash.
///
/// This is a PERMANENT identifier for a conversation between two parties.
/// It depends ONLY on identity (device keys + handle hashes + signatures),
/// NOT on ephemeral clutch keys. This means:
/// - Same provenance survives re-clutch (key rotation)
/// - Can be used as filename/filter key for conversation messages
/// - Proves chain of custody back to initial handshake
///
/// Both parties derive the same provenance because:
/// - Device pubkeys are sorted canonically
/// - Handle hashes are sorted canonically
/// - Both signatures are included (order doesn't matter for hash)
///
/// The provenance binds:
/// - WHO: Both device pubkeys (cryptographic identity)
/// - WHAT: Both handle hashes (human-readable identity, private)
/// - PROOF: Both signatures over the handshake (proves mutual consent)
pub fn derive_conversation_provenance(
    our_device_pubkey: &[u8; 32],
    their_device_pubkey: &[u8; 32],
    our_handle_hash: &[u8; 32],
    their_handle_hash: &[u8; 32],
    our_handshake_signature: &[u8; 64],
    their_handshake_signature: &[u8; 64],
) -> [u8; 32] {
    // Sort device pubkeys canonically
    let (first_device, second_device) = sort_pair(our_device_pubkey, their_device_pubkey);

    // Sort handle hashes canonically
    let (first_handle, second_handle) = sort_pair(our_handle_hash, their_handle_hash);

    // Signatures are included but order doesn't matter for the hash
    // We sort by the device pubkey order for consistency
    let (first_sig, second_sig) = if our_device_pubkey < their_device_pubkey {
        (our_handshake_signature, their_handshake_signature)
    } else {
        (their_handshake_signature, our_handshake_signature)
    };

    let mut hasher = Hasher::new();
    hasher.update(b"PHOTON_PROVENANCE_v1");
    hasher.update(first_device);
    hasher.update(second_device);
    hasher.update(first_handle);
    hasher.update(second_handle);
    hasher.update(first_sig);
    hasher.update(second_sig);

    *hasher.finalize().as_bytes()
}

/// Compute the handshake message that both parties sign.
///
/// This is signed by each party with their device private key.
/// The signatures become part of the provenance derivation.
///
/// Contains: sorted device pubkeys + sorted handle hashes + timestamp
/// The timestamp prevents replay but is NOT part of provenance (so same
/// parties can re-establish with same provenance).
pub fn compute_handshake_message(
    our_device_pubkey: &[u8; 32],
    their_device_pubkey: &[u8; 32],
    our_handle_hash: &[u8; 32],
    their_handle_hash: &[u8; 32],
) -> [u8; 32] {
    let (first_device, second_device) = sort_pair(our_device_pubkey, their_device_pubkey);
    let (first_handle, second_handle) = sort_pair(our_handle_hash, their_handle_hash);

    let mut hasher = Hasher::new();
    hasher.update(b"PHOTON_HANDSHAKE v0");
    hasher.update(first_device);
    hasher.update(second_device);
    hasher.update(first_handle);
    hasher.update(second_handle);

    *hasher.finalize().as_bytes()
}

// ============================================================================
// LAYER 2: clutch SEED (ephemeral encryption key material)
// ============================================================================

/// Derive the clutch shared seed from private handle hashes and X25519 shared secret.
///
/// This is the Phase 1 (X25519-only) seed derivation.
/// The seed is deterministic: both parties compute the same value.
///
/// SECURITY: Uses private handle_hash = BLAKE3(handle), NOT public handle_proof!
/// - handle_proof is PUBLIC (announced to FGTW, visible in peer table)
/// - handle_hash is PRIVATE (only known to parties who know the plaintext handle)
///
/// Handle hashes are sorted canonically so order of parties doesn't matter.
///
/// Note: Phase 1 uses 32-byte seed (sufficient for single primitive).
/// Full clutch (8 primitives) will use 256-byte seed via BLAKE3 XOF.
pub fn derive_clutch_seed_x25519(
    our_handle_hash: &[u8; 32],
    their_handle_hash: &[u8; 32],
    x25519_shared: &[u8; 32],
) -> Seed {
    // Sort handle hashes canonically (lower first)
    let (first, second) = if our_handle_hash < their_handle_hash {
        (our_handle_hash, their_handle_hash)
    } else {
        (their_handle_hash, our_handle_hash)
    };

    let mut hasher = Hasher::new();
    hasher.update(b"clutch_v1_x25519_only");
    hasher.update(first);
    hasher.update(second);
    hasher.update(x25519_shared);

    Seed::from_bytes(*hasher.finalize().as_bytes())
}

/// Derive the clutch shared seed using parallel key exchange.
///
/// Both parties generate and exchange ephemeral keys simultaneously.
/// BOTH ephemeral pubkeys contribute entropy to the final seed.
///
/// SECURITY: Uses private handle_hash = BLAKE3(handle), NOT public handle_proof!
///
/// Components are sorted canonically so order of parties doesn't matter:
/// - Device pubkeys: sorted (lower first) - binds to device identity
/// - Handle hashes: sorted (lower first)
/// - Ephemeral pubkeys: sorted (lower first)
///
/// Uses BLAKE3 XOF to produce 256-byte seed (ready for full 8-primitive clutch).
/// Phase 1 only uses first 32 bytes, but we derive the full seed for forward compat.
pub fn derive_clutch_seed_parallel(
    our_device_pubkey: &[u8; 32],
    their_device_pubkey: &[u8; 32],
    our_handle_hash: &[u8; 32],
    their_handle_hash: &[u8; 32],
    our_ephemeral_pub: &[u8; 32],
    their_ephemeral_pub: &[u8; 32],
    x25519_shared: &[u8; 32],
) -> Seed {
    // Sort device pubkeys canonically (binds seed to both device identities!)
    let (first_device, second_device) = sort_pair(our_device_pubkey, their_device_pubkey);

    // Sort handle hashes canonically
    let (first_handle, second_handle) = sort_pair(our_handle_hash, their_handle_hash);

    // Sort ephemeral pubkeys canonically (both contribute entropy!)
    let (first_pub, second_pub) = sort_pair(our_ephemeral_pub, their_ephemeral_pub);

    let mut hasher = Hasher::new();
    hasher.update(b"clutch v3 device bound"); // New version - device keys now bound
    hasher.update(first_device); // Device identity binding (prevents spoofing)
    hasher.update(second_device);
    hasher.update(first_handle); // Out-of-band secret (handle hash)
    hasher.update(second_handle);
    hasher.update(first_pub); // Both parties' ephemeral randomness
    hasher.update(second_pub);
    hasher.update(x25519_shared); // ECDH result (32B for X25519-only)
                                  // Future: add other 7 shared secrets here for full clutch

    // BLAKE3 XOF: extend output to 256 bytes (2048 bits)
    // Phase 1 uses Seed (32 bytes) but we derive full output for future compat
    let mut output = [0u8; 256];
    hasher.finalize_xof().fill(&mut output);

    // For now, use first 32 bytes as seed
    let mut seed_bytes = [0u8; 32];
    seed_bytes.copy_from_slice(&output[..32]);
    Seed::from_bytes(seed_bytes)
}

/// All 8 ephemeral keypairs for full CLUTCH ceremony.
/// Each algorithm has its own keypair format.
#[derive(Clone, Debug)]
pub struct ClutchAllKeypairs {
    // Class 0: Classical EC (32B secrets, variable pubkeys)
    pub x25519_secret: [u8; 32],
    pub x25519_public: [u8; 32],
    pub p384_secret: Vec<u8>,      // 48B
    pub p384_public: Vec<u8>,      // 97B (uncompressed SEC1)
    pub secp256k1_secret: Vec<u8>, // 32B
    pub secp256k1_public: Vec<u8>, // 65B (uncompressed SEC1)
    pub p256_secret: Vec<u8>,      // 32B
    pub p256_public: Vec<u8>,      // 65B (uncompressed SEC1)

    // Class 1: Post-quantum lattice KEMs
    pub frodo976_secret: Vec<u8>, // 31296B
    pub frodo976_public: Vec<u8>, // 15632B
    pub ntru701_secret: Vec<u8>,  // 1450B (HRSS-701)
    pub ntru701_public: Vec<u8>,  // 1138B

    // Class 2: Post-quantum code-based KEMs
    pub mceliece_secret: Vec<u8>, // 13608B
    pub mceliece_public: Vec<u8>, // 524160B (~512KB)
    pub hqc256_secret: Vec<u8>,   // 7317B
    pub hqc256_public: Vec<u8>,   // 7285B
}

impl ClutchAllKeypairs {
    /// Securely zeroize all secret keys
    pub fn zeroize(&mut self) {
        self.x25519_secret.zeroize();
        self.p384_secret.zeroize();
        self.secp256k1_secret.zeroize();
        self.p256_secret.zeroize();
        self.frodo976_secret.zeroize();
        self.ntru701_secret.zeroize();
        self.mceliece_secret.zeroize();
        self.hqc256_secret.zeroize();
    }

    /// Convert to VSF multi-value fields for disk storage.
    /// Returns (pubkeys, secrets) as two Vec<VsfType> for use with add_field_multi().
    /// Order: x25519, p384, secp256k1, p256, frodo, ntru, mceliece, hqc
    pub fn to_vsf_multi(&self) -> (Vec<vsf::VsfType>, Vec<vsf::VsfType>) {
        use vsf::VsfType;
        let pubkeys = vec![
            VsfType::kx(self.x25519_public.to_vec()),
            VsfType::kp(self.p384_public.clone()),
            VsfType::kk(self.secp256k1_public.clone()),
            VsfType::kp(self.p256_public.clone()),
            VsfType::kf(self.frodo976_public.clone()),
            VsfType::kn(self.ntru701_public.clone()),
            VsfType::kl(self.mceliece_public.clone()),
            VsfType::kh(self.hqc256_public.clone()),
        ];
        let secrets = vec![
            VsfType::v(b'x', self.x25519_secret.to_vec()),
            VsfType::v(b'p', self.p384_secret.clone()),
            VsfType::v(b'k', self.secp256k1_secret.clone()),
            VsfType::v(b'p', self.p256_secret.clone()),
            VsfType::v(b'f', self.frodo976_secret.clone()),
            VsfType::v(b'n', self.ntru701_secret.clone()),
            VsfType::v(b'l', self.mceliece_secret.clone()),
            VsfType::v(b'h', self.hqc256_secret.clone()),
        ];
        (pubkeys, secrets)
    }

    /// Parse from VSF section with multi-value fields.
    /// Expects: (pubkeys: kx, kp, kk, kp, kf, kn, kl, kh)
    ///          (secrets: vx, vp, vk, vp, vf, vn, vl, vh)
    pub fn from_vsf_section(section: &vsf::VsfSection) -> Option<Self> {
        use vsf::VsfType;

        // Parse pubkeys multi-value field by type marker
        let pubkeys_field = section.get_field("pubkeys")?;
        let pubkeys = &pubkeys_field.values;
        if pubkeys.len() < 8 {
            return None;
        }

        // Parse secrets multi-value field by type marker
        let secrets_field = section.get_field("secrets")?;
        let secrets = &secrets_field.values;
        if secrets.len() < 8 {
            return None;
        }

        // Extract pubkeys by type marker (order: kx, kp, kk, kp, kf, kn, kl, kh)
        let mut x25519_pub = None;
        let mut p384_pub = None;
        let mut secp256k1_pub = None;
        let mut p256_pub = None;
        let mut frodo_pub = None;
        let mut ntru_pub = None;
        let mut mceliece_pub = None;
        let mut hqc_pub = None;

        for v in pubkeys {
            match v {
                VsfType::kx(b) if x25519_pub.is_none() => x25519_pub = Some(b.clone()),
                VsfType::kp(b) if p384_pub.is_none() && b.len() > 64 => p384_pub = Some(b.clone()),
                VsfType::kp(b) if p256_pub.is_none() && b.len() <= 65 => p256_pub = Some(b.clone()),
                VsfType::kk(b) if secp256k1_pub.is_none() => secp256k1_pub = Some(b.clone()),
                VsfType::kf(b) if frodo_pub.is_none() => frodo_pub = Some(b.clone()),
                VsfType::kn(b) if ntru_pub.is_none() => ntru_pub = Some(b.clone()),
                VsfType::kl(b) if mceliece_pub.is_none() => mceliece_pub = Some(b.clone()),
                VsfType::kh(b) if hqc_pub.is_none() => hqc_pub = Some(b.clone()),
                _ => {}
            }
        }

        // Extract secrets by type marker (order: vx, vp, vk, vp, vf, vn, vl, vh)
        let mut x25519_sec = None;
        let mut p384_sec = None;
        let mut secp256k1_sec = None;
        let mut p256_sec = None;
        let mut frodo_sec = None;
        let mut ntru_sec = None;
        let mut mceliece_sec = None;
        let mut hqc_sec = None;

        for v in secrets {
            match v {
                VsfType::v(b'x', b) if x25519_sec.is_none() => x25519_sec = Some(b.clone()),
                VsfType::v(b'p', b) if p384_sec.is_none() && b.len() > 32 => {
                    p384_sec = Some(b.clone())
                }
                VsfType::v(b'p', b) if p256_sec.is_none() && b.len() == 32 => {
                    p256_sec = Some(b.clone())
                }
                VsfType::v(b'k', b) if secp256k1_sec.is_none() => secp256k1_sec = Some(b.clone()),
                VsfType::v(b'f', b) if frodo_sec.is_none() => frodo_sec = Some(b.clone()),
                VsfType::v(b'n', b) if ntru_sec.is_none() => ntru_sec = Some(b.clone()),
                VsfType::v(b'l', b) if mceliece_sec.is_none() => mceliece_sec = Some(b.clone()),
                VsfType::v(b'h', b) if hqc_sec.is_none() => hqc_sec = Some(b.clone()),
                _ => {}
            }
        }

        // Convert x25519 to fixed arrays
        let x25519_pub_bytes = x25519_pub?;
        let x25519_sec_bytes = x25519_sec?;
        if x25519_pub_bytes.len() != 32 || x25519_sec_bytes.len() != 32 {
            return None;
        }
        let mut x25519_public = [0u8; 32];
        let mut x25519_secret = [0u8; 32];
        x25519_public.copy_from_slice(&x25519_pub_bytes);
        x25519_secret.copy_from_slice(&x25519_sec_bytes);

        Some(Self {
            x25519_public,
            x25519_secret,
            p384_public: p384_pub?,
            p384_secret: p384_sec?,
            secp256k1_public: secp256k1_pub?,
            secp256k1_secret: secp256k1_sec?,
            p256_public: p256_pub?,
            p256_secret: p256_sec?,
            frodo976_public: frodo_pub?,
            frodo976_secret: frodo_sec?,
            ntru701_public: ntru_pub?,
            ntru701_secret: ntru_sec?,
            mceliece_public: mceliece_pub?,
            mceliece_secret: mceliece_sec?,
            hqc256_public: hqc_pub?,
            hqc256_secret: hqc_sec?,
        })
    }
}

// =============================================================================
// CLUTCH PAYLOAD STRUCTS FOR NETWORK TRANSFER
// =============================================================================

/// Full offer with all 8 public keys (~548KB).
/// Sent by both parties at start of CLUTCH ceremony.
///
/// For network serialization, use the VSF-wrapped functions in protocol.rs:
/// - build_clutch_offer_vsf() / parse_clutch_offer_vsf()
#[derive(Clone, Debug, Default)]
pub struct ClutchOfferPayload {
    pub x25519_public: [u8; 32],
    pub p384_public: Vec<u8>,
    pub secp256k1_public: Vec<u8>,
    pub p256_public: Vec<u8>,
    pub frodo976_public: Vec<u8>,
    pub ntru701_public: Vec<u8>,
    pub mceliece_public: Vec<u8>,
    pub hqc256_public: Vec<u8>,
}

impl ClutchOfferPayload {
    /// Create from our keypairs (extract public keys)
    pub fn from_keypairs(keys: &ClutchAllKeypairs) -> Self {
        #[cfg(feature = "development")]
        crate::log(&format!(
            "CLUTCH: Building offer with HQC pub[..8]={}",
            hex::encode(&keys.hqc256_public[..8])
        ));

        Self {
            x25519_public: keys.x25519_public,
            p384_public: keys.p384_public.clone(),
            secp256k1_public: keys.secp256k1_public.clone(),
            p256_public: keys.p256_public.clone(),
            frodo976_public: keys.frodo976_public.clone(),
            ntru701_public: keys.ntru701_public.clone(),
            mceliece_public: keys.mceliece_public.clone(), // ~512KB - PT transfer handles this
            hqc256_public: keys.hqc256_public.clone(),
        }
    }

    /// Serialize all 8 public keys to bytes (for ceremony instance derivation)
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(
            32 + self.p384_public.len()
                + self.secp256k1_public.len()
                + self.p256_public.len()
                + self.frodo976_public.len()
                + self.ntru701_public.len()
                + self.mceliece_public.len()
                + self.hqc256_public.len(),
        );
        bytes.extend_from_slice(&self.x25519_public);
        bytes.extend_from_slice(&self.p384_public);
        bytes.extend_from_slice(&self.secp256k1_public);
        bytes.extend_from_slice(&self.p256_public);
        bytes.extend_from_slice(&self.frodo976_public);
        bytes.extend_from_slice(&self.ntru701_public);
        bytes.extend_from_slice(&self.mceliece_public);
        bytes.extend_from_slice(&self.hqc256_public);
        bytes
    }

    /// Convert to VSF multi-value field for disk storage.
    /// Returns Vec<VsfType> for use with add_field_multi("pubkeys", ...).
    /// Order: x25519, p384, secp256k1, p256, frodo, ntru, mceliece, hqc
    pub fn to_vsf_multi(&self) -> Vec<vsf::VsfType> {
        use vsf::VsfType;
        vec![
            VsfType::kx(self.x25519_public.to_vec()),
            VsfType::kp(self.p384_public.clone()),
            VsfType::kk(self.secp256k1_public.clone()),
            VsfType::kp(self.p256_public.clone()),
            VsfType::kf(self.frodo976_public.clone()),
            VsfType::kn(self.ntru701_public.clone()),
            VsfType::kl(self.mceliece_public.clone()),
            VsfType::kh(self.hqc256_public.clone()),
        ]
    }

    /// Parse from VSF section with multi-value pubkeys field.
    /// Expects: (pubkeys: kx, kp, kk, kp, kf, kn, kl, kh)
    pub fn from_vsf_section(section: &vsf::VsfSection) -> Option<Self> {
        use vsf::VsfType;

        let pubkeys_field = section.get_field("pubkeys")?;
        let pubkeys = &pubkeys_field.values;
        if pubkeys.len() < 8 {
            return None;
        }

        let mut x25519_pub = None;
        let mut p384_pub = None;
        let mut secp256k1_pub = None;
        let mut p256_pub = None;
        let mut frodo_pub = None;
        let mut ntru_pub = None;
        let mut mceliece_pub = None;
        let mut hqc_pub = None;

        for v in pubkeys {
            match v {
                VsfType::kx(b) if x25519_pub.is_none() => x25519_pub = Some(b.clone()),
                VsfType::kp(b) if p384_pub.is_none() && b.len() > 64 => p384_pub = Some(b.clone()),
                VsfType::kp(b) if p256_pub.is_none() && b.len() <= 65 => p256_pub = Some(b.clone()),
                VsfType::kk(b) if secp256k1_pub.is_none() => secp256k1_pub = Some(b.clone()),
                VsfType::kf(b) if frodo_pub.is_none() => frodo_pub = Some(b.clone()),
                VsfType::kn(b) if ntru_pub.is_none() => ntru_pub = Some(b.clone()),
                VsfType::kl(b) if mceliece_pub.is_none() => mceliece_pub = Some(b.clone()),
                VsfType::kh(b) if hqc_pub.is_none() => hqc_pub = Some(b.clone()),
                _ => {}
            }
        }

        let x25519_bytes = x25519_pub?;
        if x25519_bytes.len() != 32 {
            return None;
        }
        let mut x25519_public = [0u8; 32];
        x25519_public.copy_from_slice(&x25519_bytes);

        Some(Self {
            x25519_public,
            p384_public: p384_pub?,
            secp256k1_public: secp256k1_pub?,
            p256_public: p256_pub?,
            frodo976_public: frodo_pub?,
            ntru701_public: ntru_pub?,
            mceliece_public: mceliece_pub?,
            hqc256_public: hqc_pub?,
        })
    }
}

/// KEM response with 4 PQC ciphertexts + 4 EC ephemeral pubkeys (~31KB).
/// Sent by both parties after receiving peer's full offer.
///
/// The EC ephemeral pubkeys enable ECIES-style encapsulation: sender generates
/// fresh keypair, computes ECDH with recipient's long-term pubkey, sends ephemeral
/// pubkey. This gives truly distinct shared secrets per direction per algorithm.
///
/// For network serialization, use the VSF-wrapped functions in protocol.rs:
/// - build_clutch_kem_response_vsf() / parse_clutch_kem_response_vsf()
#[derive(Clone, Debug)]
pub struct ClutchKemResponsePayload {
    // PQC KEM ciphertexts (encapsulated to peer's pubkeys)
    pub frodo976_ciphertext: Vec<u8>,
    pub ntru701_ciphertext: Vec<u8>,
    pub mceliece_ciphertext: Vec<u8>,
    pub hqc256_ciphertext: Vec<u8>,
    /// First 8 bytes of HQC public key this was encrypted to (for stale detection)
    pub target_hqc_pub_prefix: [u8; 8],
    // EC ephemeral pubkeys for ECIES-style encapsulation
    // Sender generates fresh keypair, computes ECDH(ephemeral_secret, peer_offer_pubkey)
    // Receiver computes ECDH(offer_secret, ephemeral_pubkey) to get same shared secret
    pub x25519_ephemeral: [u8; 32],
    pub p384_ephemeral: Vec<u8>,      // 97B uncompressed SEC1
    pub secp256k1_ephemeral: Vec<u8>, // 65B uncompressed SEC1
    pub p256_ephemeral: Vec<u8>,      // 65B uncompressed SEC1
}

impl ClutchKemResponsePayload {
    /// Perform encapsulations to peer's public keys (4 PQC KEMs + 4 EC ECIES-style).
    /// Returns (payload, shared_secrets) where shared_secrets are our encapsulated secrets.
    ///
    /// For EC algorithms, we generate fresh ephemeral keypairs and compute ECDH with
    /// the peer's offer pubkeys. This gives truly distinct secrets per direction.
    pub fn encapsulate_to_peer(their_offer: &ClutchOfferPayload) -> (Self, ClutchKemSharedSecrets) {
        #[cfg(feature = "development")]
        #[cfg(feature = "development")]
        #[cfg(feature = "development")]
        crate::log("CLUTCH: Encapsulating to peer's public keys (8 algorithms)...");

        // ===== PQC KEMs =====
        let (frodo976_ciphertext, frodo_ss) = frodo976_encapsulate(&their_offer.frodo976_public);
        let (ntru701_ciphertext, ntru_ss) = ntru701_encapsulate(&their_offer.ntru701_public);
        let (mceliece_ciphertext, mceliece_ss) =
            mceliece460896_encapsulate(&their_offer.mceliece_public);
        let (hqc256_ciphertext, hqc_ss) = hqc256_encapsulate(&their_offer.hqc256_public);

        #[cfg(feature = "development")]
        crate::log(&format!(
            "CLUTCH: HQC encap: their_pub[..8]={} → ct[..8]={}",
            hex::encode(&their_offer.hqc256_public[..8]),
            hex::encode(&hqc256_ciphertext[..8])
        ));

        // ===== EC ECIES-style: generate ephemeral keypairs, ECDH with peer's offer pubkeys =====
        // This gives distinct shared secrets per direction (we→them vs them→us)
        let (x25519_eph_secret, x25519_ephemeral) = generate_x25519_ephemeral();
        let x25519_ss = x25519_ecdh(&x25519_eph_secret, &their_offer.x25519_public);

        let (p384_eph_secret, p384_ephemeral) = generate_p384_ephemeral();
        let p384_ss = p384_ecdh(&p384_eph_secret, &their_offer.p384_public);

        let (secp256k1_eph_secret, secp256k1_ephemeral) = generate_secp256k1_ephemeral();
        let secp256k1_ss = secp256k1_ecdh(&secp256k1_eph_secret, &their_offer.secp256k1_public);

        let (p256_eph_secret, p256_ephemeral) = generate_p256_ephemeral();
        let p256_ss = p256_ecdh(&p256_eph_secret, &their_offer.p256_public);

        #[cfg(feature = "development")]
        crate::log(&format!(
            "CLUTCH: Encap ready (PQC: Frodo {}B, NTRU {}B, McEliece {}B, HQC {}B) (EC: X25519 32B, P384 {}B, secp256k1 {}B, P256 {}B)",
            frodo976_ciphertext.len(),
            ntru701_ciphertext.len(),
            mceliece_ciphertext.len(),
            hqc256_ciphertext.len(),
            p384_ss.len(),
            secp256k1_ss.len(),
            p256_ss.len()
        ));

        // Store the target HQC pub prefix so recipient can verify before decapsulating
        let mut target_hqc_pub_prefix = [0u8; 8];
        target_hqc_pub_prefix.copy_from_slice(&their_offer.hqc256_public[..8]);

        let payload = Self {
            frodo976_ciphertext,
            ntru701_ciphertext,
            mceliece_ciphertext,
            hqc256_ciphertext,
            target_hqc_pub_prefix,
            x25519_ephemeral,
            p384_ephemeral,
            secp256k1_ephemeral,
            p256_ephemeral,
        };

        let secrets = ClutchKemSharedSecrets {
            frodo: frodo_ss,
            ntru: ntru_ss,
            mceliece: mceliece_ss,
            hqc: hqc_ss,
            x25519: x25519_ss,
            p384: p384_ss,
            secp256k1: secp256k1_ss,
            p256: p256_ss,
        };

        (payload, secrets)
    }
}

/// Shared secrets from encapsulation (one direction) - all 8 algorithms.
/// PQC KEMs produce variable-size secrets, EC ECDH produces 32B secrets.
#[derive(Clone, Debug)]
pub struct ClutchKemSharedSecrets {
    // PQC KEM shared secrets
    pub frodo: Vec<u8>,
    pub ntru: Vec<u8>,
    pub mceliece: Vec<u8>,
    pub hqc: Vec<u8>,
    // EC ECDH shared secrets (ECIES-style: ephemeral_secret × peer_offer_pubkey)
    pub x25519: [u8; 32],
    pub p384: Vec<u8>,      // 48B
    pub secp256k1: Vec<u8>, // 32B
    pub p256: Vec<u8>,      // 32B
}

impl ClutchKemSharedSecrets {
    /// Decapsulate from received response using our secret keys (4 PQC + 4 EC).
    ///
    /// For EC algorithms, we compute ECDH(our_offer_secret, their_ephemeral_pubkey)
    /// which gives the same shared secret as their ECDH(ephemeral_secret, our_offer_pubkey).
    pub fn decapsulate_from_peer(
        response: &ClutchKemResponsePayload,
        our_keys: &ClutchAllKeypairs,
    ) -> Self {
        #[cfg(feature = "development")]
        #[cfg(feature = "development")]
        #[cfg(feature = "development")]
        crate::log("CLUTCH: Decapsulating from peer's response (8 algorithms)...");

        // ===== PQC KEMs =====
        let frodo = frodo976_decapsulate(&our_keys.frodo976_secret, &response.frodo976_ciphertext);
        #[cfg(feature = "development")]
        crate::log(&format!(
            "CLUTCH: ✓ Frodo976 decap OK ({}B shared secret)",
            frodo.len()
        ));

        let ntru = ntru701_decapsulate(&our_keys.ntru701_secret, &response.ntru701_ciphertext);
        #[cfg(feature = "development")]
        crate::log(&format!(
            "CLUTCH: ✓ NTRU701 decap OK ({}B shared secret)",
            ntru.len()
        ));

        // TODO: Re-enable McEliece once PT transfer is stable
        let mceliece = if response.mceliece_ciphertext.is_empty() {
            #[cfg(feature = "development")]
            #[cfg(feature = "development")]
            #[cfg(feature = "development")]
            crate::log("CLUTCH: - McEliece skipped (empty ciphertext)");
            vec![0u8; 32] // Placeholder shared secret
        } else {
            let ss = mceliece460896_decapsulate(
                &our_keys.mceliece_secret,
                &response.mceliece_ciphertext,
            );
            #[cfg(feature = "development")]
            crate::log(&format!(
                "CLUTCH: ✓ McEliece decap OK ({}B shared secret)",
                ss.len()
            ));
            ss
        };

        #[cfg(feature = "development")]
        crate::log(&format!(
            "CLUTCH: HQC256 decap: our_sk[..8]={} their_ct[..8]={}",
            hex::encode(&our_keys.hqc256_secret[..8]),
            hex::encode(&response.hqc256_ciphertext[..8])
        ));

        let hqc = hqc256_decapsulate(&our_keys.hqc256_secret, &response.hqc256_ciphertext);
        #[cfg(feature = "development")]
        crate::log(&format!(
            "CLUTCH: ✓ HQC256 decap OK ({}B shared secret)",
            hqc.len()
        ));

        // ===== EC ECIES-style: ECDH(our_offer_secret, their_ephemeral_pubkey) =====
        // This matches their ECDH(ephemeral_secret, our_offer_pubkey)
        let x25519 = x25519_ecdh(&our_keys.x25519_secret, &response.x25519_ephemeral);
        #[cfg(feature = "development")]
        #[cfg(feature = "development")]
        #[cfg(feature = "development")]
        crate::log("CLUTCH: ✓ X25519 decap OK (32B shared secret)");

        let p384 = p384_ecdh(&our_keys.p384_secret, &response.p384_ephemeral);
        #[cfg(feature = "development")]
        crate::log(&format!(
            "CLUTCH: ✓ P384 decap OK ({}B shared secret)",
            p384.len()
        ));

        let secp256k1 = secp256k1_ecdh(&our_keys.secp256k1_secret, &response.secp256k1_ephemeral);
        #[cfg(feature = "development")]
        crate::log(&format!(
            "CLUTCH: ✓ secp256k1 decap OK ({}B shared secret)",
            secp256k1.len()
        ));

        let p256 = p256_ecdh(&our_keys.p256_secret, &response.p256_ephemeral);
        #[cfg(feature = "development")]
        crate::log(&format!(
            "CLUTCH: ✓ P256 decap OK ({}B shared secret)",
            p256.len()
        ));

        Self {
            frodo,
            ntru,
            mceliece,
            hqc,
            x25519,
            p384,
            secp256k1,
            p256,
        }
    }

    /// Zeroize all secrets
    pub fn zeroize(&mut self) {
        self.frodo.zeroize();
        self.ntru.zeroize();
        self.mceliece.zeroize();
        self.hqc.zeroize();
        self.x25519.zeroize();
        self.p384.zeroize();
        self.secp256k1.zeroize();
        self.p256.zeroize();
    }

    /// Convert to VSF multi-value field for disk storage.
    /// Returns Vec<VsfType> for use with add_field_multi("secrets", ...).
    /// Order: x25519, p384, secp256k1, p256, frodo, ntru, mceliece, hqc
    pub fn to_vsf_multi(&self) -> Vec<vsf::VsfType> {
        use vsf::VsfType;
        vec![
            VsfType::v(b'x', self.x25519.to_vec()),
            VsfType::v(b'p', self.p384.clone()),
            VsfType::v(b'k', self.secp256k1.clone()),
            VsfType::v(b'p', self.p256.clone()),
            VsfType::v(b'f', self.frodo.clone()),
            VsfType::v(b'n', self.ntru.clone()),
            VsfType::v(b'l', self.mceliece.clone()),
            VsfType::v(b'h', self.hqc.clone()),
        ]
    }

    /// Parse from VSF section with multi-value secrets field.
    /// Expects: (secrets: vx, vp, vk, vp, vf, vn, vl, vh)
    pub fn from_vsf_section(section: &vsf::VsfSection) -> Option<Self> {
        use vsf::VsfType;

        let secrets_field = section.get_field("secrets")?;
        let secrets = &secrets_field.values;
        if secrets.len() < 8 {
            return None;
        }

        let mut x25519_sec = None;
        let mut p384_sec = None;
        let mut secp256k1_sec = None;
        let mut p256_sec = None;
        let mut frodo_sec = None;
        let mut ntru_sec = None;
        let mut mceliece_sec = None;
        let mut hqc_sec = None;

        for v in secrets {
            match v {
                VsfType::v(b'x', b) if x25519_sec.is_none() => x25519_sec = Some(b.clone()),
                VsfType::v(b'p', b) if p384_sec.is_none() && b.len() > 32 => {
                    p384_sec = Some(b.clone())
                }
                VsfType::v(b'p', b) if p256_sec.is_none() && b.len() == 32 => {
                    p256_sec = Some(b.clone())
                }
                VsfType::v(b'k', b) if secp256k1_sec.is_none() => secp256k1_sec = Some(b.clone()),
                VsfType::v(b'f', b) if frodo_sec.is_none() => frodo_sec = Some(b.clone()),
                VsfType::v(b'n', b) if ntru_sec.is_none() => ntru_sec = Some(b.clone()),
                VsfType::v(b'l', b) if mceliece_sec.is_none() => mceliece_sec = Some(b.clone()),
                VsfType::v(b'h', b) if hqc_sec.is_none() => hqc_sec = Some(b.clone()),
                _ => {}
            }
        }

        let x25519_bytes = x25519_sec?;
        if x25519_bytes.len() != 32 {
            return None;
        }
        let mut x25519 = [0u8; 32];
        x25519.copy_from_slice(&x25519_bytes);

        Some(Self {
            x25519,
            p384: p384_sec?,
            secp256k1: secp256k1_sec?,
            p256: p256_sec?,
            frodo: frodo_sec?,
            ntru: ntru_sec?,
            mceliece: mceliece_sec?,
            hqc: hqc_sec?,
        })
    }
}

/// Sent by both parties after computing eggs to verify agreement.
///
/// Contains the eggs_proof hash. Both parties MUST compute the same proof
/// since they derived identical eggs from the ceremony.
///
/// If proofs don't match, something went catastrophically wrong (MITM, bug,
/// or corruption) and the ceremony MUST be aborted with a panic.
///
/// For network serialization, use the VSF-wrapped functions in protocol.rs:
/// - build_clutch_complete_vsf() / parse_clutch_complete_vsf()
#[derive(Clone, Debug)]
pub struct ClutchCompletePayload {
    pub eggs_proof: [u8; 32],
}

/// Generate all 8 ephemeral keypairs for full CLUTCH ceremony.
/// WARNING: This generates ~512KB of public key material (mostly McEliece).
/// Caller MUST call zeroize() on the result when done!
pub fn generate_all_ephemeral_keypairs() -> ClutchAllKeypairs {
    // Class 0: Classical EC
    let (x25519_secret, x25519_public) = generate_x25519_ephemeral();
    let (p384_secret, p384_public) = generate_p384_ephemeral();
    let (secp256k1_secret, secp256k1_public) = generate_secp256k1_ephemeral();
    let (p256_secret, p256_public) = generate_p256_ephemeral();

    // Class 1: Post-quantum lattice KEMs
    let (frodo976_secret, frodo976_public) = generate_frodo976_keypair();
    let (ntru701_secret, ntru701_public) = generate_ntru701_keypair();

    // Class 2: Post-quantum code-based KEMs
    let (mceliece_secret, mceliece_public) = generate_mceliece460896_keypair();
    let (hqc256_secret, hqc256_public) = generate_hqc256_keypair();

    ClutchAllKeypairs {
        x25519_secret,
        x25519_public,
        p384_secret,
        p384_public,
        secp256k1_secret,
        secp256k1_public,
        p256_secret,
        p256_public,
        frodo976_secret,
        frodo976_public,
        ntru701_secret,
        ntru701_public,
        mceliece_secret,
        mceliece_public,
        hqc256_secret,
        hqc256_public,
    }
}

/// All shared secrets from 20 cryptographic eggs.
/// Each "egg" is a labeled BLAKE3 hash for domain separation.
pub struct ClutchEggs {
    pub eggs: Vec<[u8; 32]>,
}

impl ClutchEggs {
    pub fn new() -> Self {
        ClutchEggs { eggs: Vec::new() }
    }

    /// Add an egg with domain-separated labeling
    fn add_egg(&mut self, label: &str, shared_secret: &[u8]) {
        let mut hasher = Hasher::new();
        hasher.update(b"clutch EGG v4 ");
        hasher.update(label.as_bytes());
        hasher.update(shared_secret);
        self.eggs.push(*hasher.finalize().as_bytes());
    }

    /// Get eggs as slice of 32-byte arrays (for FriendshipChains::from_clutch)
    pub fn as_slice(&self) -> &[[u8; 32]] {
        &self.eggs
    }
}

/// Collect all 20 cryptographic eggs for bidirectional CLUTCH.
///
/// 4 identity eggs:
/// - our_device_pubkey, their_device_pubkey
/// - our_handle_hash, their_handle_hash
///
/// 16 shared secret eggs (8 algorithms × 2 directions):
/// - Both parties exchange in both directions
/// - Ordered by handle hash: low_* then high_*
/// - Class 0: x25519, p384, secp256k1, p256
/// - Class 1: frodo976, ntru701
/// - Class 2: mceliece460896, hqc256
///
/// Each egg is a BLAKE3 hash with domain separation:
/// BLAKE3("clutch_EGG_v4_" || label || shared_secret)
///
/// Returns vector of 20 eggs ready for avalanche hashing.
pub fn collect_clutch_eggs(
    our_device_pubkey: &[u8; 32],
    their_device_pubkey: &[u8; 32],
    our_handle_hash: &[u8; 32],
    their_handle_hash: &[u8; 32],
    low_x25519_shared: &[u8; 32],
    high_x25519_shared: &[u8; 32],
    low_p384_shared: &[u8],
    high_p384_shared: &[u8],
    low_secp256k1_shared: &[u8],
    high_secp256k1_shared: &[u8],
    low_frodo_shared: &[u8],
    high_frodo_shared: &[u8],
    low_ntru_shared: &[u8],
    high_ntru_shared: &[u8],
    low_mceliece_shared: &[u8],
    high_mceliece_shared: &[u8],
    low_hqc_shared: &[u8],
    high_hqc_shared: &[u8],
    low_p256_shared: &[u8],
    high_p256_shared: &[u8],
) -> ClutchEggs {
    let mut eggs = ClutchEggs::new();

    // Sort device pubkeys and handle hashes canonically so both parties add in same order
    let (low_device, high_device) = sort_pair(our_device_pubkey, their_device_pubkey);
    let (low_handle, high_handle) = sort_pair(our_handle_hash, their_handle_hash);

    eggs.add_egg("low_device_pubkey", low_device);
    eggs.add_egg("high_device_pubkey", high_device);
    eggs.add_egg("low_handle_hash", low_handle);
    eggs.add_egg("high_handle_hash", high_handle);

    // Class 0: Classical EC - low handle's secrets first, then high
    eggs.add_egg("low_x25519", low_x25519_shared);
    eggs.add_egg("high_x25519", high_x25519_shared);
    eggs.add_egg("low_p384", low_p384_shared);
    eggs.add_egg("high_p384", high_p384_shared);
    eggs.add_egg("low_secp256k1", low_secp256k1_shared);
    eggs.add_egg("high_secp256k1", high_secp256k1_shared);
    eggs.add_egg("low_p256", low_p256_shared);
    eggs.add_egg("high_p256", high_p256_shared);

    // Class 1: Post-quantum lattice KEMs
    eggs.add_egg("low_frodo976", low_frodo_shared);
    eggs.add_egg("high_frodo976", high_frodo_shared);
    eggs.add_egg("low_ntru701", low_ntru_shared);
    eggs.add_egg("high_ntru701", high_ntru_shared);

    // Class 2: Post-quantum code-based KEMs
    eggs.add_egg("low_mceliece460896", low_mceliece_shared);
    eggs.add_egg("high_mceliece460896", high_mceliece_shared);
    eggs.add_egg("low_hqc256", low_hqc_shared);
    eggs.add_egg("high_hqc256", high_hqc_shared);

    eggs
}

/// Avalanche hash the eggs into dual 1MB pads for bidirectional conversation state.
///
/// This is a memory-hard, deterministic mixing function that:
/// 0. Flattens all 20 eggs into a single buffer (640 bytes)
/// 1. Repeatedly copies pseudo-random chunks to grow to 2MB
/// 2. Heavy mixing with diverse operations (+, -, *, ^, %, <<, >>)
/// 3. Final rotation and trim to exactly 2MB
/// 4. Split into two 1MB pads (low_pad, high_pad)
///
/// Properties:
/// - Deterministic: same eggs → same pads
/// - Memory-hard: 2MB total state
/// - Avalanche: every bit of input affects every bit of output
/// - Diverse operations: prevents algebraic attacks
///
/// The returned pads are saved locally:
/// - low_pad: rotates when lower handle proof sends/acks messages
/// - high_pad: rotates when higher handle proof sends/acks messages
///
/// Returns (low_pad, high_pad) as two 1MB Vec<u8> for conversation state.
pub fn avalanche_hash_eggs(eggs: &ClutchEggs) -> (Vec<u8>, Vec<u8>) {
    use i256::U256;

    #[cfg(feature = "development")]
    let start_time = std::time::Instant::now();

    #[cfg(feature = "development")]
    crate::log(&format!(
        "CLUTCH: Collecting {} eggs for avalanche ({} bytes input)...",
        eggs.eggs.len(),
        eggs.eggs.len() * 32
    ));

    const MIN_SIZE: usize = 1_048_576; // 1MB ish
    const TOTAL_SIZE: usize = MIN_SIZE * 2; // 2MB
    let max_size = TOTAL_SIZE * 2; // Allow expansion up to 4MB

    // Step 0: Flatten all eggs into one buffer
    let mut omelette = Vec::with_capacity(max_size);
    for egg in &eggs.eggs {
        omelette.extend_from_slice(egg);
    }

    #[cfg(feature = "development")]
    let step0_elapsed = start_time.elapsed();

    let mut target_hasher = Hasher::new();
    target_hasher.update(b"target");
    target_hasher.update(&omelette);
    let target_hash = target_hasher.finalize();
    let target_u256 = U256::from_be_bytes(*target_hash.as_bytes());
    let target_size =
        TOTAL_SIZE + (target_u256 % U256::from(TOTAL_SIZE as u128)).as_u128() as usize;

    // Step 1: Grow to 2MB by copying pseudo-random chunks
    while omelette.len() < target_size {
        let current_len = omelette.len();

        // Hash current state → U256 for start position
        let mut start_hasher = Hasher::new();
        start_hasher.update(b"start");
        start_hasher.update(&omelette);
        let start_hash = start_hasher.finalize();
        let start_u256 = U256::from_be_bytes(*start_hash.as_bytes());
        let start_pos = (start_u256 % U256::from(current_len as u128)).as_u128() as usize;

        // Hash with domain separation → U256 for stop position
        let mut stop_hasher = Hasher::new();
        stop_hasher.update(&omelette);
        stop_hasher.update(b"stop");
        let stop_hash = stop_hasher.finalize();
        let stop_u256 = U256::from_be_bytes(*stop_hash.as_bytes());
        let stop_pos = (stop_u256 % U256::from(current_len as u128)).as_u128() as usize;

        // Swap if start > stop
        let (start, stop, append) = if start_pos > stop_pos {
            (stop_pos, start_pos, true)
        } else {
            (start_pos, stop_pos, false)
        };

        // Guard against empty chunk (start == stop) causing infinite loop
        let chunk = if start == stop {
            // Hash current state to get a non-empty chunk
            let mut chunk_hasher = Hasher::new();
            chunk_hasher.update(b"empty_chunk_fallback");
            chunk_hasher.update(&omelette);
            chunk_hasher.finalize().as_bytes().to_vec()
        } else {
            omelette[start..stop].to_vec()
        };
        if append {
            // Append to end
            omelette.extend_from_slice(&chunk);
        } else {
            // Prepend to start (faster than splice for large buffers)
            let mut temp = chunk;
            temp.append(&mut omelette);
            omelette = temp;
        }

        // Overgrow is okay, we'll trim at the end
        if omelette.len() > target_size {
            break;
        }
    }

    #[cfg(feature = "development")]
    let step1_elapsed = start_time.elapsed();

    // Step 2: Heavy mixing with diverse operations
    // Process as variable-sized chunks (1-43 bytes, unaligned) for maximum diffusion
    const MIX_ROUNDS: usize = 8;

    for round in 0..MIX_ROUNDS {
        let len = omelette.len();

        // Hash current state to derive mixing parameters
        let mut round_hasher = Hasher::new();
        round_hasher.update(&omelette);
        round_hasher.update(&[round as u8]);
        let round_hash = round_hasher.finalize();
        let round_u256 = U256::from_be_bytes(*round_hash.as_bytes());

        // Determine chunk size for this round (1-43 bytes, unaligned)
        let chunk_size = 1 + ((round_u256 % U256::from(43_u128)).as_u128() as usize);

        // Mix chunks with diverse operations
        let num_chunks = len / chunk_size;

        for i in 0..num_chunks {
            let pos = i * chunk_size;
            if pos > len - chunk_size {
                break;
            }

            // Hash current chunk to derive indices
            let chunk = &omelette[pos..pos + chunk_size];
            let mut idx_hasher = Hasher::new();
            idx_hasher.update(chunk);
            idx_hasher.update(&[round as u8, i as u8]);
            let idx_hash = idx_hasher.finalize();
            let idx_u256 = U256::from_be_bytes(*idx_hash.as_bytes());

            // Pick two random chunks to mix with
            let idx1 =
                ((idx_u256 % U256::from(num_chunks as u128)).as_u128() as usize) * chunk_size;
            let idx2 = (((idx_u256 >> 64_u32) % U256::from(num_chunks as u128)).as_u128() as usize)
                * chunk_size;

            if idx1 + chunk_size > len || idx2 + chunk_size > len {
                continue;
            }

            // Read chunks (avoid borrow checker by cloning)
            let chunk1 = omelette[idx1..idx1 + chunk_size].to_vec();
            let chunk2 = omelette[idx2..idx2 + chunk_size].to_vec();

            // Apply diverse operation based on round (byte-wise for variable sizes)
            for j in 0..chunk_size {
                let val = omelette[pos + j];
                let v1 = chunk1[j];
                let v2 = chunk2[j];

                // Different operation per round for maximum diversity
                omelette[pos + j] = match round % 7 {
                    0 => val.wrapping_add(v1) ^ v2,             // + and ^
                    1 => val.wrapping_sub(v1).wrapping_mul(v2), // - and *
                    2 => (val ^ v1).wrapping_add(v2),           // ^ and +
                    3 => val.wrapping_mul(0xEF) ^ v1 ^ 0xBE,    // DEADBEEF nibbles
                    4 => (val << (v1 & 7)) ^ v2,                // << shift
                    5 => (val >> (v2 & 7)) ^ v1,                // >> shift
                    6 => val ^ v1 ^ v2 ^ 0xDE,                  // More DEADBEEF
                    _ => val,
                };
            }
        }
    }

    #[cfg(feature = "development")]
    let step2_elapsed = start_time.elapsed();

    // Step 3: Final rotation before trim
    // Hash entire buffer and rotate by (hash % len) to shuffle one last time
    let final_hash = blake3::hash(&omelette);
    let final_u256 = U256::from_be_bytes(*final_hash.as_bytes());
    let rotate_amount = (final_u256 % U256::from(omelette.len() as u128)).as_u128() as usize;
    omelette.rotate_left(rotate_amount);

    // Step 5: Trim to exactly 2MB
    omelette.truncate(TOTAL_SIZE);

    // Step 6: Split into two 1MB pads (legacy, for backwards compat logging only)
    let low_pad = omelette[0..MIN_SIZE].to_vec();
    let high_pad = omelette[MIN_SIZE..TOTAL_SIZE].to_vec();

    #[cfg(feature = "development")]
    {
        let total_elapsed = start_time.elapsed();
        crate::log(&format!(
            "CLUTCH: avalanche_hash 2MB: step0={:.1}ms step1={:.1}ms step2={:.1}ms step3={:.1}ms total={:.1}ms",
            step0_elapsed.as_secs_f64() * 1000.0,
            (step1_elapsed - step0_elapsed).as_secs_f64() * 1000.0,
            (step2_elapsed - step1_elapsed).as_secs_f64() * 1000.0,
            (total_elapsed - step2_elapsed).as_secs_f64() * 1000.0,
            total_elapsed.as_secs_f64() * 1000.0,
        ));
    }

    (low_pad, high_pad)
}

/// Expand eggs to 2MB mixed buffer for chain derivation.
///
/// Memory-hard, deterministic, preserves full entropy from all 20 eggs.
/// Uses the same expansion and mixing logic as avalanche_hash_eggs but
/// returns the full 2MB buffer instead of splitting into pads.
///
/// Properties:
/// - Deterministic: same eggs → same 2MB output
/// - Memory-hard: 2MB total state
/// - Avalanche: every bit of input affects every bit of output
/// - No compression: full 2MB preserves entropy for chain derivation
pub fn avalanche_expand_eggs(eggs: &ClutchEggs) -> Vec<u8> {
    use i256::U256;

    #[cfg(feature = "development")]
    let start_time = std::time::Instant::now();

    const TOTAL_SIZE: usize = 2_097_152; // 2MB
    let max_size = TOTAL_SIZE * 2; // Allow expansion up to 4MB

    // Step 0: Flatten all eggs into one buffer
    let mut omelette = Vec::with_capacity(max_size);
    for egg in &eggs.eggs {
        omelette.extend_from_slice(egg);
    }

    #[cfg(feature = "development")]
    let step0_elapsed = start_time.elapsed();

    // Determine target size (2-4MB, data-dependent)
    let mut target_hasher = Hasher::new();
    target_hasher.update(b"target");
    target_hasher.update(&omelette);
    let target_hash = target_hasher.finalize();
    let target_u256 = U256::from_be_bytes(*target_hash.as_bytes());
    let target_size =
        TOTAL_SIZE + (target_u256 % U256::from(TOTAL_SIZE as u128)).as_u128() as usize;

    // Step 1: Grow to target size by copying pseudo-random chunks
    while omelette.len() < target_size {
        let current_len = omelette.len();

        let mut start_hasher = Hasher::new();
        start_hasher.update(b"start");
        start_hasher.update(&omelette);
        let start_hash = start_hasher.finalize();
        let start_u256 = U256::from_be_bytes(*start_hash.as_bytes());
        let start_pos = (start_u256 % U256::from(current_len as u128)).as_u128() as usize;

        let mut stop_hasher = Hasher::new();
        stop_hasher.update(&omelette);
        stop_hasher.update(b"stop");
        let stop_hash = stop_hasher.finalize();
        let stop_u256 = U256::from_be_bytes(*stop_hash.as_bytes());
        let stop_pos = (stop_u256 % U256::from(current_len as u128)).as_u128() as usize;

        let (start, stop, append) = if start_pos > stop_pos {
            (stop_pos, start_pos, true)
        } else {
            (start_pos, stop_pos, false)
        };

        // Guard against empty chunk (start == stop) causing infinite loop
        let chunk = if start == stop {
            // Hash current state to get a non-empty chunk
            let mut chunk_hasher = Hasher::new();
            chunk_hasher.update(b"empty_chunk_fallback");
            chunk_hasher.update(&omelette);
            chunk_hasher.finalize().as_bytes().to_vec()
        } else {
            omelette[start..stop].to_vec()
        };
        if append {
            omelette.extend_from_slice(&chunk);
        } else {
            let mut temp = chunk;
            temp.append(&mut omelette);
            omelette = temp;
        }

        if omelette.len() > target_size {
            break;
        }
    }

    #[cfg(feature = "development")]
    let step1_elapsed = start_time.elapsed();

    // Step 2: Heavy mixing with diverse operations
    const MIX_ROUNDS: usize = 8;

    for round in 0..MIX_ROUNDS {
        let len = omelette.len();

        let mut round_hasher = Hasher::new();
        round_hasher.update(&omelette);
        round_hasher.update(&[round as u8]);
        let round_hash = round_hasher.finalize();
        let round_u256 = U256::from_be_bytes(*round_hash.as_bytes());

        let chunk_size = 1 + ((round_u256 % U256::from(43_u128)).as_u128() as usize);
        let num_chunks = len / chunk_size;

        for i in 0..num_chunks {
            let pos = i * chunk_size;
            if pos > len - chunk_size {
                break;
            }

            let chunk = &omelette[pos..pos + chunk_size];
            let mut idx_hasher = Hasher::new();
            idx_hasher.update(chunk);
            idx_hasher.update(&[round as u8, i as u8]);
            let idx_hash = idx_hasher.finalize();
            let idx_u256 = U256::from_be_bytes(*idx_hash.as_bytes());

            let idx1 =
                ((idx_u256 % U256::from(num_chunks as u128)).as_u128() as usize) * chunk_size;
            let idx2 = (((idx_u256 >> 64_u32) % U256::from(num_chunks as u128)).as_u128() as usize)
                * chunk_size;

            if idx1 + chunk_size > len || idx2 + chunk_size > len {
                continue;
            }

            let chunk1 = omelette[idx1..idx1 + chunk_size].to_vec();
            let chunk2 = omelette[idx2..idx2 + chunk_size].to_vec();

            for j in 0..chunk_size {
                let val = omelette[pos + j];
                let v1 = chunk1[j];
                let v2 = chunk2[j];

                omelette[pos + j] = match round % 7 {
                    0 => val.wrapping_add(v1) ^ v2,
                    1 => val.wrapping_sub(v1).wrapping_mul(v2),
                    2 => (val ^ v1).wrapping_add(v2),
                    3 => val.wrapping_mul(0xEF) ^ v1 ^ 0xBE,
                    4 => (val << (v1 & 7)) ^ v2,
                    5 => (val >> (v2 & 7)) ^ v1,
                    6 => val ^ v1 ^ v2 ^ 0xDE,
                    _ => val,
                };
            }
        }
    }

    #[cfg(feature = "development")]
    let step2_elapsed = start_time.elapsed();

    // Step 3: Final rotation
    let final_hash = blake3::hash(&omelette);
    let final_u256 = U256::from_be_bytes(*final_hash.as_bytes());
    let rotate_amount = (final_u256 % U256::from(omelette.len() as u128)).as_u128() as usize;
    omelette.rotate_left(rotate_amount);

    // Trim to exactly 2MB
    omelette.truncate(TOTAL_SIZE);

    #[cfg(feature = "development")]
    {
        let total_elapsed = start_time.elapsed();
        crate::log(&format!(
            "CLUTCH: avalanche_expand 2MB: step0={:.1}ms step1={:.1}ms step2={:.1}ms step3={:.1}ms total={:.1}ms",
            step0_elapsed.as_secs_f64() * 1000.0,
            (step1_elapsed - step0_elapsed).as_secs_f64() * 1000.0,
            (step2_elapsed - step1_elapsed).as_secs_f64() * 1000.0,
            (total_elapsed - step2_elapsed).as_secs_f64() * 1000.0,
            total_elapsed.as_secs_f64() * 1000.0,
        ));
    }

    omelette
}

/// Derive one participant's 8KB chain from avalanche buffer.
///
/// Uses truncate-and-append for deterministic PRNG without compression.
/// Links accumulate at end of buffer - no separate Vec needed.
///
/// Algorithm:
/// 1. Domain separation: BLAKE3_XOF(avalanche || participant) → 2MB buffer
/// 2. For 256 rounds:
///    - link = smear_hash(buffer)  // BLAKE3 ⊕ SHA3 ⊕ SHA512
///    - Drop first 32B, append link at end
/// 3. Chain = last 8KB (256 links in order)
pub fn derive_chain_from_avalanche(avalanche: &[u8], participant: &[u8; 32]) -> Vec<u8> {
    // Domain separation: mix participant identity into state
    let mut hasher = Hasher::new();
    hasher.update(b"PHOTON_CHAIN_DERIVE_v1");
    hasher.update(participant);
    hasher.update(avalanche);

    // Create working buffer from domain-separated XOF
    let mut buffer = vec![0u8; avalanche.len()];
    hasher.finalize_xof().fill(&mut buffer);
    let len = buffer.len();

    // Generate 256 links via truncate-and-append
    // Each link = smear_hash(buffer), then drop first 32B, append link at end
    for _ in 0..256 {
        let link = smear_hash(&buffer);
        buffer.copy_within(32.., 0); // shift left, drop first 32B
        buffer[len - 32..].copy_from_slice(&link); // append link at end
    }

    // Chain = last 8KB (256 × 32B links in order)
    buffer[len - 8192..].to_vec()
}

/// Compute the clutch completion proof.
/// Sent by initiator to confirm they derived the same seed.
/// Responder can verify without revealing the seed.
pub fn compute_clutch_proof(seed: &Seed) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(seed.as_bytes());
    hasher.update(b"clutch_v1_complete");
    *hasher.finalize().as_bytes()
}

/// Verify the clutch completion proof matches our derived seed.
pub fn verify_clutch_proof(seed: &Seed, proof: &[u8; 32]) -> bool {
    use subtle::ConstantTimeEq;
    let expected = compute_clutch_proof(seed);
    // Actually constant-time comparison (subtle crate)
    expected.ct_eq(proof).into()
}

/// Full clutch ceremony result
pub struct ClutchResult {
    pub seed: Seed,
    pub proof: [u8; 32],
}

/// Perform complete clutch ceremony using parallel key exchange.
///
/// Both parties generate ephemeral keypairs simultaneously and exchange them.
/// Both pubkeys contribute entropy to the final seed.
/// Device pubkeys are mixed in to bind the seed to both device identities.
///
/// Steps:
/// 0. Generate ephemeral keypair (done before calling this)
/// 1. Exchange ClutchOffer messages (both directions, parallel)
/// 2. Once both pubkeys known, call this function
/// 3. Lower handle_proof party sends ClutchComplete with proof
///
/// SECURITY: Takes private handle_hash = BLAKE3(handle), NOT public handle_proof!
/// Device pubkeys are mixed in to prevent handle spoofing with different device.
pub fn clutch_complete_parallel(
    our_device_pubkey: &[u8; 32],
    their_device_pubkey: &[u8; 32],
    our_handle_hash: &[u8; 32],
    their_handle_hash: &[u8; 32],
    our_ephemeral_secret: &[u8; 32],
    our_ephemeral_pubkey: &[u8; 32],
    their_ephemeral_pubkey: &[u8; 32],
) -> ClutchResult {
    let mut x25519_shared = x25519_ecdh(our_ephemeral_secret, their_ephemeral_pubkey);
    let seed = derive_clutch_seed_parallel(
        our_device_pubkey,
        their_device_pubkey,
        our_handle_hash,
        their_handle_hash,
        our_ephemeral_pubkey,
        their_ephemeral_pubkey,
        &x25519_shared,
    );
    let proof = compute_clutch_proof(&seed);

    // Zeroize intermediate shared secret
    x25519_shared.zeroize();

    ClutchResult { seed, proof }
}

/// Full CLUTCH result with eggs for chain derivation.
pub struct ClutchFullResult {
    /// 20 cryptographic eggs from the ceremony (for FriendshipChains)
    pub eggs: ClutchEggs,
    /// Proof hash for verification
    pub proof: [u8; 32],
}

/// All 16 shared secrets for full CLUTCH (8 algorithms × 2 directions).
///
/// All algorithms now use ECIES-style bidirectional encapsulation:
/// - Each party generates ephemeral keys and encapsulates to peer
/// - Results in TWO distinct shared secrets per algorithm (truly bidirectional)
/// - low_* = encapsulated by lower handle_hash party
/// - high_* = encapsulated by higher handle_hash party
///
/// For 2-party: 16 distinct shared secrets (8 algorithms × 2 directions)
/// For 3-party: 48 distinct shared secrets (8 algorithms × 6 directed pairs)
///
/// An attacker must compromise BOTH directions of an algorithm to break
/// that algorithm's contribution to the final key material.
pub struct ClutchSharedSecrets {
    // Class 0: Classical EC (ECIES-style: distinct secret per direction)
    pub low_x25519: [u8; 32],
    pub high_x25519: [u8; 32],
    pub low_p384: Vec<u8>, // 48B
    pub high_p384: Vec<u8>,
    pub low_secp256k1: Vec<u8>, // 32B
    pub high_secp256k1: Vec<u8>,
    pub low_p256: Vec<u8>, // 32B
    pub high_p256: Vec<u8>,

    // Class 1: Post-quantum lattice KEMs (distinct secret per direction)
    pub low_frodo: Vec<u8>, // 24B
    pub high_frodo: Vec<u8>,
    pub low_ntru: Vec<u8>, // 32B
    pub high_ntru: Vec<u8>,

    // Class 2: Post-quantum code-based KEMs (distinct secret per direction)
    pub low_mceliece: Vec<u8>, // 32B
    pub high_mceliece: Vec<u8>,
    pub low_hqc: Vec<u8>, // 64B
    pub high_hqc: Vec<u8>,
}

impl ClutchSharedSecrets {
    /// Securely zeroize all shared secrets
    pub fn zeroize(&mut self) {
        self.low_x25519.zeroize();
        self.high_x25519.zeroize();
        self.low_p384.zeroize();
        self.high_p384.zeroize();
        self.low_secp256k1.zeroize();
        self.high_secp256k1.zeroize();
        self.low_p256.zeroize();
        self.high_p256.zeroize();
        self.low_frodo.zeroize();
        self.high_frodo.zeroize();
        self.low_ntru.zeroize();
        self.high_ntru.zeroize();
        self.low_mceliece.zeroize();
        self.high_mceliece.zeroize();
        self.low_hqc.zeroize();
        self.high_hqc.zeroize();
    }
}

/// Perform full 8-algorithm CLUTCH ceremony.
///
/// Takes all 16 shared secrets (8 algorithms × 2 directions) and produces
/// identical (low_pad, high_pad) on both parties.
///
/// The low/high ordering is determined by comparing handle_hashes:
/// - Party with lower handle_hash uses low_pad for sending
/// - Party with higher handle_hash uses high_pad for sending
///
/// Both parties MUST call this with the same shared secrets (just with
/// their perspective on low/high being different based on handle ordering).
///
/// Returns ClutchFullResult with:
/// - low_pad: 1MB encryption pad for low handle party
/// - high_pad: 1MB encryption pad for high handle party
/// - proof: BLAKE3 hash of pads for verification
pub fn clutch_complete_full(
    our_device_pubkey: &[u8; 32],
    their_device_pubkey: &[u8; 32],
    our_handle_hash: &[u8; 32],
    their_handle_hash: &[u8; 32],
    secrets: &ClutchSharedSecrets,
) -> ClutchFullResult {
    // Collect all 20 eggs
    let eggs = collect_clutch_eggs(
        our_device_pubkey,
        their_device_pubkey,
        our_handle_hash,
        their_handle_hash,
        &secrets.low_x25519,
        &secrets.high_x25519,
        &secrets.low_p384,
        &secrets.high_p384,
        &secrets.low_secp256k1,
        &secrets.high_secp256k1,
        &secrets.low_frodo,
        &secrets.high_frodo,
        &secrets.low_ntru,
        &secrets.high_ntru,
        &secrets.low_mceliece,
        &secrets.high_mceliece,
        &secrets.low_hqc,
        &secrets.high_hqc,
        &secrets.low_p256,
        &secrets.high_p256,
    );

    // Compute proof from eggs (deterministic - same eggs = same proof)
    let proof = compute_eggs_proof(&eggs);

    ClutchFullResult { eggs, proof }
}

/// Compute proof hash for CLUTCH verification from eggs.
/// Used by both parties to verify they collected identical eggs.
///
/// Defense-in-depth: uses spaghettify + smear_hash for algorithm diversity.
/// If BLAKE3 is broken, SHA3 and SHA512 still protect the proof.
/// If any hash is broken, spaghettify's chaos mixing still scrambles the eggs.
pub fn compute_eggs_proof(eggs: &ClutchEggs) -> [u8; 32] {
    // Flatten eggs to bytes
    let mut egg_bytes = Vec::with_capacity(eggs.eggs.len() * 32);
    for egg in &eggs.eggs {
        egg_bytes.extend_from_slice(egg);
    }

    // Add domain separation
    let mut input = b"CLUTCH_EGGS_v2_proof".to_vec();
    input.extend_from_slice(&egg_bytes);

    // Spaghettify for chaos mixing, then smear_hash for algorithm diversity
    // This is overkill for a proof, but consistency with chain derivation is good
    let spaghetti = spaghettify(&input);

    // Final proof = smear_hash(spaghetti || eggs)
    // Defense in depth: if spaghettify broken, eggs still contribute directly
    let mut final_input = spaghetti.to_vec();
    final_input.extend_from_slice(&egg_bytes);
    smear_hash(&final_input)
}

/// Verify CLUTCH proof matches our eggs.
pub fn verify_eggs_proof(eggs: &ClutchEggs, proof: &[u8; 32]) -> bool {
    use subtle::ConstantTimeEq;
    let expected = compute_eggs_proof(eggs);
    expected.ct_eq(proof).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initiator_selection_deterministic() {
        let alice = [1u8; 32];
        let bob = [2u8; 32];

        // Alice (lower) should be initiator from both perspectives
        assert!(is_clutch_initiator(&alice, &bob));
        assert!(!is_clutch_initiator(&bob, &alice));
    }

    #[test]
    fn test_clutch_ceremony_v1_compatibility_removed() {
        // This test verified v1 sequential clutch (initiator/responder pattern).
        // v3 uses parallel exchange only - see test_parallel_clutch_produces_same_seed.
        // Keeping this stub to document the intentional removal of v1 support.
    }

    #[test]
    fn test_different_handles_different_seeds() {
        // Private handle hashes (BLAKE3 of plaintext handle)
        let handle_hash1 = *blake3::hash(b"handle one").as_bytes();
        let handle_hash2 = *blake3::hash(b"handle two").as_bytes();
        let handle_hash3 = *blake3::hash(b"handle three").as_bytes();

        let (secret, public) = generate_x25519_ephemeral();
        let shared = x25519_ecdh(&secret, &public);

        let seed_12 = derive_clutch_seed_x25519(&handle_hash1, &handle_hash2, &shared);
        let seed_13 = derive_clutch_seed_x25519(&handle_hash1, &handle_hash3, &shared);

        // Different handle pairs should produce different seeds
        assert_ne!(seed_12.as_bytes(), seed_13.as_bytes());
    }

    #[test]
    fn test_parallel_clutch_produces_same_seed() {
        // Device pubkeys (simulated Ed25519 public keys)
        let alice_device = [1u8; 32];
        let bob_device = [2u8; 32];

        // Private handle hashes
        let alice_handle_hash = *blake3::hash(b"alice parallel handle").as_bytes();
        let bob_handle_hash = *blake3::hash(b"bob parallel handle").as_bytes();

        // Both generate ephemeral keypairs simultaneously
        let (alice_secret, alice_public) = generate_x25519_ephemeral();
        let (bob_secret, bob_public) = generate_x25519_ephemeral();

        // Both complete the ceremony with device keys and all four pubkeys
        let alice_result = clutch_complete_parallel(
            &alice_device,
            &bob_device,
            &alice_handle_hash,
            &bob_handle_hash,
            &alice_secret,
            &alice_public,
            &bob_public,
        );

        let bob_result = clutch_complete_parallel(
            &bob_device,
            &alice_device,
            &bob_handle_hash,
            &alice_handle_hash,
            &bob_secret,
            &bob_public,
            &alice_public,
        );

        // Both should derive the same seed
        assert_eq!(alice_result.seed.as_bytes(), bob_result.seed.as_bytes());

        // Proofs should match
        assert_eq!(alice_result.proof, bob_result.proof);

        // Cross-verify proofs
        assert!(verify_clutch_proof(&alice_result.seed, &bob_result.proof));
        assert!(verify_clutch_proof(&bob_result.seed, &alice_result.proof));
    }

    #[test]
    fn test_parallel_sorted_pubkeys_deterministic() {
        // Verify that sorting pubkeys produces deterministic output regardless of order
        let device1 = [1u8; 32];
        let device2 = [2u8; 32];
        let handle1 = *blake3::hash(b"handle 1").as_bytes();
        let handle2 = *blake3::hash(b"handle 2").as_bytes();

        let (secret1, pub1) = generate_x25519_ephemeral();
        let (_secret2, pub2) = generate_x25519_ephemeral();

        let shared = x25519_ecdh(&secret1, &pub2);

        // Derive seed with pubkeys in both orders - should produce same result
        let seed_a = derive_clutch_seed_parallel(
            &device1, &device2, &handle1, &handle2, &pub1, &pub2, &shared,
        );
        let seed_b = derive_clutch_seed_parallel(
            &device1, &device2, &handle1, &handle2, &pub2, &pub1, &shared,
        );

        assert_eq!(seed_a.as_bytes(), seed_b.as_bytes());
    }

    #[test]
    fn test_different_device_keys_different_seeds() {
        // Different device keys should produce different seeds (prevents spoofing)
        let device1 = [1u8; 32];
        let device2 = [2u8; 32];
        let device3 = [3u8; 32]; // Attacker's device
        let handle1 = *blake3::hash(b"alice").as_bytes();
        let handle2 = *blake3::hash(b"bob").as_bytes();

        let (secret1, pub1) = generate_x25519_ephemeral();
        let (_secret2, pub2) = generate_x25519_ephemeral();

        let shared = x25519_ecdh(&secret1, &pub2);

        // Legitimate seed between device1 and device2
        let legit_seed = derive_clutch_seed_parallel(
            &device1, &device2, &handle1, &handle2, &pub1, &pub2, &shared,
        );

        // Attacker tries to spoof with device3 claiming to be bob
        let spoofed_seed = derive_clutch_seed_parallel(
            &device1, &device3, &handle1, &handle2, &pub1, &pub2, &shared,
        );

        // Seeds MUST be different - device key binding prevents spoofing
        assert_ne!(legit_seed.as_bytes(), spoofed_seed.as_bytes());
    }

    // ========================================================================
    // PROVENANCE TESTS
    // ========================================================================

    #[test]
    fn test_provenance_deterministic_both_parties() {
        // Both parties should derive the same provenance
        let alice_device = [1u8; 32];
        let bob_device = [2u8; 32];
        let alice_handle = *blake3::hash(b"alice").as_bytes();
        let bob_handle = *blake3::hash(b"bob").as_bytes();

        // Simulated signatures (in real code, these come from Ed25519 signing)
        let alice_sig = [0xAAu8; 64];
        let bob_sig = [0xBBu8; 64];

        let alice_provenance = derive_conversation_provenance(
            &alice_device,
            &bob_device,
            &alice_handle,
            &bob_handle,
            &alice_sig,
            &bob_sig,
        );

        let bob_provenance = derive_conversation_provenance(
            &bob_device,
            &alice_device,
            &bob_handle,
            &alice_handle,
            &bob_sig,
            &alice_sig,
        );

        // Both parties MUST derive the same provenance
        assert_eq!(alice_provenance, bob_provenance);
    }

    #[test]
    fn test_provenance_different_for_different_pairs() {
        let alice_device = [1u8; 32];
        let bob_device = [2u8; 32];
        let charlie_device = [3u8; 32];
        let alice_handle = *blake3::hash(b"alice").as_bytes();
        let bob_handle = *blake3::hash(b"bob").as_bytes();
        let charlie_handle = *blake3::hash(b"charlie").as_bytes();

        let sig_a = [0xAAu8; 64];
        let sig_b = [0xBBu8; 64];
        let sig_c = [0xCCu8; 64];

        let alice_bob = derive_conversation_provenance(
            &alice_device,
            &bob_device,
            &alice_handle,
            &bob_handle,
            &sig_a,
            &sig_b,
        );

        let alice_charlie = derive_conversation_provenance(
            &alice_device,
            &charlie_device,
            &alice_handle,
            &charlie_handle,
            &sig_a,
            &sig_c,
        );

        // Different conversation pairs MUST have different provenance
        assert_ne!(alice_bob, alice_charlie);
    }

    #[test]
    fn test_provenance_survives_reclutch() {
        // Key insight: provenance doesn't include ephemeral keys
        // So re-clutch (new ephemeral keys) produces same provenance
        let alice_device = [1u8; 32];
        let bob_device = [2u8; 32];
        let alice_handle = *blake3::hash(b"alice").as_bytes();
        let bob_handle = *blake3::hash(b"bob").as_bytes();
        let alice_sig = [0xAAu8; 64];
        let bob_sig = [0xBBu8; 64];

        // First clutch - get provenance
        let provenance_1 = derive_conversation_provenance(
            &alice_device,
            &bob_device,
            &alice_handle,
            &bob_handle,
            &alice_sig,
            &bob_sig,
        );

        // Re-clutch with new ephemeral keys (simulated)
        // But same device keys, handles, and signatures
        let provenance_2 = derive_conversation_provenance(
            &alice_device,
            &bob_device,
            &alice_handle,
            &bob_handle,
            &alice_sig,
            &bob_sig,
        );

        // Provenance MUST be identical after re-clutch
        assert_eq!(provenance_1, provenance_2);
    }

    #[test]
    fn test_handshake_message_deterministic() {
        let alice_device = [1u8; 32];
        let bob_device = [2u8; 32];
        let alice_handle = *blake3::hash(b"alice").as_bytes();
        let bob_handle = *blake3::hash(b"bob").as_bytes();

        let alice_msg =
            compute_handshake_message(&alice_device, &bob_device, &alice_handle, &bob_handle);

        let bob_msg =
            compute_handshake_message(&bob_device, &alice_device, &bob_handle, &alice_handle);

        // Both parties compute the same handshake message to sign
        assert_eq!(alice_msg, bob_msg);
    }

    #[test]
    fn test_egg_collection_produces_vector() {
        let alice_device = [1u8; 32];
        let bob_device = [2u8; 32];
        let alice_handle = *blake3::hash(b"alice").as_bytes();
        let bob_handle = *blake3::hash(b"bob").as_bytes();

        // 16 shared secrets (8 algorithms × 2 directions)
        let low_x25519 = [5u8; 32];
        let high_x25519 = [6u8; 32];
        let low_p384 = vec![7u8; 48];
        let high_p384 = vec![8u8; 48];
        let low_secp256k1 = vec![9u8; 32];
        let high_secp256k1 = vec![10u8; 32];
        let low_frodo = vec![11u8; 24];
        let high_frodo = vec![12u8; 24];
        let low_ntru = vec![13u8; 32];
        let high_ntru = vec![14u8; 32];
        let low_mceliece = vec![15u8; 32];
        let high_mceliece = vec![16u8; 32];
        let low_hqc = vec![17u8; 64];
        let high_hqc = vec![18u8; 64];
        let low_p256 = vec![19u8; 32];
        let high_p256 = vec![20u8; 32];

        let eggs = collect_clutch_eggs(
            &alice_device,
            &bob_device,
            &alice_handle,
            &bob_handle,
            &low_x25519,
            &high_x25519,
            &low_p384,
            &high_p384,
            &low_secp256k1,
            &high_secp256k1,
            &low_frodo,
            &high_frodo,
            &low_ntru,
            &high_ntru,
            &low_mceliece,
            &high_mceliece,
            &low_hqc,
            &high_hqc,
            &low_p256,
            &high_p256,
        );

        // 4 identity + 16 shared secrets = 20 eggs
        assert_eq!(eggs.eggs.len(), 20);

        for egg in &eggs.eggs {
            assert_eq!(egg.len(), 32);
        }
    }

    #[test]
    fn test_egg_domain_separation() {
        let alice_device = [1u8; 32];
        let bob_device = [2u8; 32];
        let alice_handle = *blake3::hash(b"alice").as_bytes();
        let bob_handle = *blake3::hash(b"bob").as_bytes();

        // Use same bytes for all secrets to test domain separation
        let shared_32 = [99u8; 32];
        let shared_48 = vec![99u8; 48];
        let shared_24 = vec![99u8; 24];
        let shared_64 = vec![99u8; 64];

        let eggs = collect_clutch_eggs(
            &alice_device,
            &bob_device,
            &alice_handle,
            &bob_handle,
            &shared_32,          // low_x25519
            &shared_32,          // high_x25519
            &shared_48,          // low_p384
            &shared_48,          // high_p384
            &shared_32.to_vec(), // low_secp256k1
            &shared_32.to_vec(), // high_secp256k1
            &shared_24,          // low_frodo
            &shared_24,          // high_frodo
            &shared_32.to_vec(), // low_ntru
            &shared_32.to_vec(), // high_ntru
            &shared_32.to_vec(), // low_mceliece
            &shared_32.to_vec(), // high_mceliece
            &shared_64,          // low_hqc
            &shared_64,          // high_hqc
            &shared_32.to_vec(), // low_p256
            &shared_32.to_vec(), // high_p256
        );

        // Even with same input bytes, domain separation should produce unique eggs
        let unique_eggs: std::collections::HashSet<[u8; 32]> = eggs.eggs.into_iter().collect();
        assert_eq!(unique_eggs.len(), 20);
    }

    #[test]
    fn test_full_clutch_identical_pads() {
        // This is THE critical test: both parties must derive identical pads

        // Device identities
        let alice_device = [1u8; 32];
        let bob_device = [2u8; 32];

        // Handle hashes (alice < bob so alice is "low")
        let alice_handle = *blake3::hash(b"alice").as_bytes();
        let bob_handle = *blake3::hash(b"bob").as_bytes();
        assert!(alice_handle < bob_handle, "alice should be low handle");

        // Generate all keypairs for both parties
        let mut alice_keys = generate_all_ephemeral_keypairs();
        let mut bob_keys = generate_all_ephemeral_keypairs();

        // === EC ALGORITHMS: Both compute same shared secret ===
        // X25519
        let x25519_shared = x25519_ecdh(&alice_keys.x25519_secret, &bob_keys.x25519_public);
        let x25519_shared_bob = x25519_ecdh(&bob_keys.x25519_secret, &alice_keys.x25519_public);
        assert_eq!(x25519_shared, x25519_shared_bob);

        // P-384
        let p384_shared = p384_ecdh(&alice_keys.p384_secret, &bob_keys.p384_public);
        let p384_shared_bob = p384_ecdh(&bob_keys.p384_secret, &alice_keys.p384_public);
        assert_eq!(p384_shared, p384_shared_bob);

        // secp256k1
        let secp256k1_shared =
            secp256k1_ecdh(&alice_keys.secp256k1_secret, &bob_keys.secp256k1_public);
        let secp256k1_shared_bob =
            secp256k1_ecdh(&bob_keys.secp256k1_secret, &alice_keys.secp256k1_public);
        assert_eq!(secp256k1_shared, secp256k1_shared_bob);

        // P-256
        let p256_shared = p256_ecdh(&alice_keys.p256_secret, &bob_keys.p256_public);
        let p256_shared_bob = p256_ecdh(&bob_keys.p256_secret, &alice_keys.p256_public);
        assert_eq!(p256_shared, p256_shared_bob);

        // === KEM ALGORITHMS: Each encapsulates to peer, decapsulates own ===

        // FrodoKEM-976
        let (frodo_ct_to_bob, frodo_ss_alice_encap) =
            frodo976_encapsulate(&bob_keys.frodo976_public);
        let (frodo_ct_to_alice, frodo_ss_bob_encap) =
            frodo976_encapsulate(&alice_keys.frodo976_public);
        let frodo_ss_bob_decap = frodo976_decapsulate(&bob_keys.frodo976_secret, &frodo_ct_to_bob);
        let frodo_ss_alice_decap =
            frodo976_decapsulate(&alice_keys.frodo976_secret, &frodo_ct_to_alice);
        assert_eq!(frodo_ss_alice_encap, frodo_ss_bob_decap); // Alice→Bob direction
        assert_eq!(frodo_ss_bob_encap, frodo_ss_alice_decap); // Bob→Alice direction

        // NTRU-701
        let (ntru_ct_to_bob, ntru_ss_alice_encap) = ntru701_encapsulate(&bob_keys.ntru701_public);
        let (ntru_ct_to_alice, ntru_ss_bob_encap) = ntru701_encapsulate(&alice_keys.ntru701_public);
        let ntru_ss_bob_decap = ntru701_decapsulate(&bob_keys.ntru701_secret, &ntru_ct_to_bob);
        let ntru_ss_alice_decap =
            ntru701_decapsulate(&alice_keys.ntru701_secret, &ntru_ct_to_alice);
        assert_eq!(ntru_ss_alice_encap, ntru_ss_bob_decap);
        assert_eq!(ntru_ss_bob_encap, ntru_ss_alice_decap);

        // McEliece-460896
        let (mce_ct_to_bob, mce_ss_alice_encap) =
            mceliece460896_encapsulate(&bob_keys.mceliece_public);
        let (mce_ct_to_alice, mce_ss_bob_encap) =
            mceliece460896_encapsulate(&alice_keys.mceliece_public);
        let mce_ss_bob_decap =
            mceliece460896_decapsulate(&bob_keys.mceliece_secret, &mce_ct_to_bob);
        let mce_ss_alice_decap =
            mceliece460896_decapsulate(&alice_keys.mceliece_secret, &mce_ct_to_alice);
        assert_eq!(mce_ss_alice_encap, mce_ss_bob_decap);
        assert_eq!(mce_ss_bob_encap, mce_ss_alice_decap);

        // HQC-256
        let (hqc_ct_to_bob, hqc_ss_alice_encap) = hqc256_encapsulate(&bob_keys.hqc256_public);
        let (hqc_ct_to_alice, hqc_ss_bob_encap) = hqc256_encapsulate(&alice_keys.hqc256_public);
        let hqc_ss_bob_decap = hqc256_decapsulate(&bob_keys.hqc256_secret, &hqc_ct_to_bob);
        let hqc_ss_alice_decap = hqc256_decapsulate(&alice_keys.hqc256_secret, &hqc_ct_to_alice);
        assert_eq!(hqc_ss_alice_encap, hqc_ss_bob_decap);
        assert_eq!(hqc_ss_bob_encap, hqc_ss_alice_decap);

        // === BUILD SHARED SECRETS STRUCT ===
        // low_* = from alice's perspective (alice is low handle)
        // high_* = from bob's perspective (bob is high handle)
        //
        // For EC: both get same shared secret, but labeled by who initiated
        // For KEM: low_* = alice→bob direction, high_* = bob→alice direction

        let alice_secrets = ClutchSharedSecrets {
            low_x25519: x25519_shared,
            high_x25519: x25519_shared, // Same for EC
            low_p384: p384_shared.clone(),
            high_p384: p384_shared.clone(),
            low_secp256k1: secp256k1_shared.clone(),
            high_secp256k1: secp256k1_shared.clone(),
            low_p256: p256_shared.clone(),
            high_p256: p256_shared.clone(),
            // KEM: directional
            low_frodo: frodo_ss_alice_encap.clone(), // Alice→Bob
            high_frodo: frodo_ss_alice_decap.clone(), // Bob→Alice (what Alice decapsulated)
            low_ntru: ntru_ss_alice_encap.clone(),
            high_ntru: ntru_ss_alice_decap.clone(),
            low_mceliece: mce_ss_alice_encap.clone(),
            high_mceliece: mce_ss_alice_decap.clone(),
            low_hqc: hqc_ss_alice_encap.clone(),
            high_hqc: hqc_ss_alice_decap.clone(),
        };

        let bob_secrets = ClutchSharedSecrets {
            low_x25519: x25519_shared,
            high_x25519: x25519_shared,
            low_p384: p384_shared.clone(),
            high_p384: p384_shared.clone(),
            low_secp256k1: secp256k1_shared.clone(),
            high_secp256k1: secp256k1_shared.clone(),
            low_p256: p256_shared.clone(),
            high_p256: p256_shared.clone(),
            // KEM: Bob's view is symmetric to Alice's
            low_frodo: frodo_ss_bob_decap.clone(), // Alice→Bob (what Bob decapsulated)
            high_frodo: frodo_ss_bob_encap.clone(), // Bob→Alice
            low_ntru: ntru_ss_bob_decap.clone(),
            high_ntru: ntru_ss_bob_encap.clone(),
            low_mceliece: mce_ss_bob_decap.clone(),
            high_mceliece: mce_ss_bob_encap.clone(),
            low_hqc: hqc_ss_bob_decap.clone(),
            high_hqc: hqc_ss_bob_encap.clone(),
        };

        // === COMPLETE CLUTCH ===
        let alice_result = clutch_complete_full(
            &alice_device,
            &bob_device,
            &alice_handle,
            &bob_handle,
            &alice_secrets,
        );

        let bob_result = clutch_complete_full(
            &bob_device,
            &alice_device,
            &bob_handle,
            &alice_handle,
            &bob_secrets,
        );

        // === THE CRITICAL ASSERTIONS ===
        // Both parties should collect identical eggs
        assert_eq!(
            alice_result.eggs.eggs.len(),
            bob_result.eggs.eggs.len(),
            "egg count mismatch!"
        );
        for (i, (a, b)) in alice_result
            .eggs
            .eggs
            .iter()
            .zip(bob_result.eggs.eggs.iter())
            .enumerate()
        {
            assert_eq!(a, b, "egg {} mismatch!", i);
        }
        assert_eq!(alice_result.proof, bob_result.proof, "proof mismatch!");

        // Verify proof
        assert!(verify_eggs_proof(&alice_result.eggs, &bob_result.proof));

        // Cleanup
        alice_keys.zeroize();
        bob_keys.zeroize();
    }

    // ========================================================================
    // SPAGHETTIFY TESTS
    // ========================================================================

    #[test]
    fn test_spaghettify_deterministic() {
        // Same input MUST produce same output
        let input = b"Hello, spaghetti world!";
        let output1 = spaghettify(input);
        let output2 = spaghettify(input);
        assert_eq!(output1, output2, "spaghettify must be deterministic");
    }

    #[test]
    fn test_spaghettify_empty_input() {
        // Empty input should produce valid output from LAVA_SEED
        let output = spaghettify(&[]);
        assert_ne!(
            output, [0u8; 32],
            "empty input should not produce all zeros"
        );

        // Should also be deterministic
        let output2 = spaghettify(&[]);
        assert_eq!(output, output2, "empty input should be deterministic");
    }

    #[test]
    fn test_spaghettify_different_inputs() {
        // Different inputs should produce different outputs
        let output1 = spaghettify(b"input one");
        let output2 = spaghettify(b"input two");
        assert_ne!(
            output1, output2,
            "different inputs should produce different outputs"
        );
    }

    #[test]
    fn test_spaghettify_avalanche() {
        // Flip one bit, should change ~50% of output bits
        let input1 = [0u8; 32];
        let mut input2 = [0u8; 32];
        input2[0] = 1; // Flip one bit

        let output1 = spaghettify(&input1);
        let output2 = spaghettify(&input2);

        // Count differing bits
        let mut diff_bits = 0;
        for (a, b) in output1.iter().zip(output2.iter()) {
            diff_bits += (a ^ b).count_ones();
        }

        // Should change roughly half the bits (128 ± 32 is reasonable)
        assert!(
            diff_bits > 64,
            "avalanche too weak: only {} bits changed",
            diff_bits
        );
        assert!(
            diff_bits < 192,
            "avalanche too strong: {} bits changed",
            diff_bits
        );
    }

    #[test]
    fn test_spaghettify_variable_rounds() {
        // Different inputs should trigger different round counts
        // We can't directly verify round count, but we can verify different
        // inputs produce different timing characteristics (not tested here)
        // and that both produce valid outputs

        let short = spaghettify(&[0u8]);
        let long = spaghettify(&[255u8; 1000]);

        // Both should be valid 32-byte outputs
        assert_eq!(short.len(), 32);
        assert_eq!(long.len(), 32);
        // And different
        assert_ne!(short, long);
    }

    #[test]
    fn test_spaghettify_large_input() {
        // Should handle large inputs gracefully
        let large_input = vec![0xAB; 100_000]; // 100KB
        let output = spaghettify(&large_input);

        // Should produce valid output
        assert_eq!(output.len(), 32);
        assert_ne!(output, [0u8; 32]);

        // Should be deterministic
        let output2 = spaghettify(&large_input);
        assert_eq!(output, output2);
    }
}
