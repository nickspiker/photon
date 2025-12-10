# CHAIN Protocol Specification v1.0

**Protocol:** Rolling Chain Encryption (Post-CLUTCH Communication)
**Author:** Nick Spiker
**Status:** Draft
**License:** MIT OR Apache-2.0
**Date:** December 2025
**Dependency:** Requires completed CLUTCH ceremony (see CLUTCH.md)

---

## 0. Abstract

CHAIN is the rolling encryption protocol used for all communication after a CLUTCH ceremony completes. It transforms the 256-byte CLUTCH seed into an evolving chain state that provides forward secrecy, self-authentication, and memory-hard advancement.

CHAIN is not a separate handshake—successful decryption *is* authentication. Both parties derive identical chain states deterministically, and the chain advances with every message. Compromise of current state reveals nothing about past messages.

---

## 1. Design Philosophy

### 1.0 Self-Authenticating Messages

No signatures, no certificates, no identity proofs. If a message decrypts successfully, the sender must possess the chain state, which proves continuous participation since the CLUTCH ceremony. The chain itself is the credential.

### 1.1 Forward Secrecy by Default

Every message advances the chain state through a memory-hard function. Even if current state is compromised, past messages remain protected—the attacker cannot reverse the chain advancement.

### 1.2 Symmetric Efficiency

After the asymmetric CLUTCH ceremony (~500ms), all subsequent encryption is symmetric (ChaCha20-Poly1305). Message encryption is effectively instant, limited only by XOR bandwidth.

### 1.3 Dual PRNG Defense

Salt generation uses two independent PRNGs (ChaCha20Rng + Pcg64). An attacker must break both simultaneously to predict salt values.

---

## 2. Chain State Structure

### 2.0 State Definition

```rust
struct ChainState {
    /// Current chain hash (32 bytes)
    /// Advances with every message via memory-hard function
    current: [u8; 32],

    /// Message sequence number (monotonic)
    /// Prevents replay, provides nonce uniqueness
    sequence: u64,

    /// CLUTCH seed backup (for PRNG initialization)
    /// Only used once (on first message)
    clutch_seed: [u8; 256],

    /// Dual PRNG state (initialized on first message)
    prng: Option<DualPRNG>,

    /// Precomputed L1 scratch for next message
    /// Background thread generates this continuously
    next_scratch: Option<Vec<u8>>,
}

struct DualPRNG {
    chacha_rng: ChaCha20Rng,  // CSPRNG - cryptographically secure
    pcg_rng: Pcg64,           // Fast PRNG - different mathematical structure
}
```

### 2.1 State Synchronization

Both parties maintain identical chain states. The chain is **deterministic**:
- Same CLUTCH seed → same initial state
- Same messages processed → same current state
- Same sequence number → synchronized

If states desynchronize (missed message, corruption), decryption fails and resynchronization is required (see Section 7).

---

## 3. Chain Initialization

### 3.0 From CLUTCH Seed

Upon CLUTCH ceremony completion:

```rust
fn init_chain_state(clutch_seed: &[u8; 256]) -> ChainState {
    // Domain-separated derivation of initial chain hash
    let initial_hash = blake3::keyed_hash(
        b"PHOTON_CHAIN_v1_init____________",  // 32-byte key
        clutch_seed
    );

    ChainState {
        current: *initial_hash.as_bytes(),
        sequence: 0,
        clutch_seed: *clutch_seed,
        prng: None,  // Initialized on first message
        next_scratch: None,  // Background thread starts computing
    }
}
```

### 3.1 PRNG Initialization

Triggered by the first message in either direction:

```rust
fn init_dual_prng(clutch_seed: &[u8; 256], first_message_hash: &[u8; 32]) -> DualPRNG {
    // ChaCha20Rng seed (32 bytes)
    let chacha_seed = blake3::keyed_hash(
        b"PHOTON_CHAIN_chacha_seed________",
        &[clutch_seed.as_slice(), first_message_hash].concat()
    );

    // Pcg64 seed (16 bytes -> 2x u64)
    let pcg_seed = blake3::keyed_hash(
        b"PHOTON_CHAIN_pcg_seed___________",
        &[clutch_seed.as_slice(), first_message_hash].concat()
    );

    let pcg_state = u64::from_le_bytes(pcg_seed.as_bytes()[0..8].try_into().unwrap());
    let pcg_stream = u64::from_le_bytes(pcg_seed.as_bytes()[8..16].try_into().unwrap());

    DualPRNG {
        chacha_rng: ChaCha20Rng::from_seed(*chacha_seed.as_bytes()),
        pcg_rng: Pcg64::new(pcg_state, pcg_stream),
    }
}
```

**Why first message hash?** Adds unpredictable entropy to PRNG seeding. Even if CLUTCH seed is somehow predicted, the first message content provides additional randomness.

---

## 4. Memory-Hard Scratch Generation

### 4.0 L1 Cache-Optimized Scratch

Background thread continuously precomputes scratch buffers:

```rust
const L1_SIZE: usize = 32_768;  // 32KB - fits in L1 cache
const L1_ROUNDS: usize = 3;      // 3 rounds, ~1-10ms

fn generate_l1_scratch(chain_state: &[u8; 32]) -> Vec<u8> {
    let mut scratch = vec![0u8; L1_SIZE];

    // Seed with chain state
    scratch[..32].copy_from_slice(chain_state);

    for round in 0..L1_ROUNDS {
        // Sequential fill with data-dependent reads
        for i in (32..L1_SIZE).step_by(32) {
            // Hash previous 32 bytes
            let prev_hash = blake3::hash(&scratch[i - 32..i]);

            // Data-dependent read position (cache-hostile)
            let read_pos = u32::from_le_bytes(
                prev_hash.as_bytes()[0..4].try_into().unwrap()
            ) as usize % i;

            // Ensure 32-byte aligned read
            let aligned_pos = read_pos & !31;
            let chunk = &scratch[aligned_pos..aligned_pos + 32];

            // Mix: prev_hash XOR chunk XOR round XOR position
            let mixed = blake3::hash(&[
                prev_hash.as_bytes(),
                chunk,
                &(round as u64).to_le_bytes(),
                &(i as u64).to_le_bytes(),
            ].concat());

            scratch[i..i + 32].copy_from_slice(mixed.as_bytes());
        }
    }

    scratch
}
```

### 4.1 Scratch Properties

- **Memory-hard:** Must hold 32KB in fast memory
- **Sequential:** Cannot parallelize within a round
- **Data-dependent:** Read positions depend on computed values
- **Deterministic:** Same chain_state → same scratch
- **Fast:** 1-10ms (precomputed in background, never on hot path)

### 4.2 Background Precomputation

```rust
impl ChainState {
    /// Called by background thread after each message
    fn precompute_next_scratch(&mut self) {
        self.next_scratch = Some(generate_l1_scratch(&self.current));
    }

    /// Returns precomputed scratch, or generates if not ready
    fn get_scratch(&mut self) -> Vec<u8> {
        self.next_scratch.take()
            .unwrap_or_else(|| generate_l1_scratch(&self.current))
    }
}
```

---

## 5. Text Message Encryption

### 5.0 Wire Format

```rust
struct EncryptedMessage {
    /// Sequence number (prevents replay, provides nonce)
    sequence: u64,

    /// Dual PRNG salt (64 bytes: 32 ChaCha20 + 32 Pcg64)
    salt: [u8; 64],

    /// ChaCha20-Poly1305 ciphertext (includes 16-byte auth tag)
    ciphertext: Vec<u8>,
}
```

**Wire overhead:** 8 (sequence) + 64 (salt) + 16 (Poly1305 tag) = 88 bytes per message

### 5.1 Send Flow

```rust
fn send_message(plaintext: &[u8], state: &mut ChainState) -> EncryptedMessage {
    // 0. Get precomputed scratch (or generate)
    let scratch = state.get_scratch();
    let scratch_hash = blake3::hash(&scratch);

    // 1. Derive message key from chain state + scratch
    let message_key = blake3::keyed_hash(
        b"PHOTON_CHAIN_message_key________",
        &[&state.current[..], &state.sequence.to_le_bytes(), scratch_hash.as_bytes()].concat()
    );

    // 2. Generate or initialize PRNG, get dual salt
    let salt = match state.prng.as_mut() {
        Some(prng) => generate_dual_salt(prng),
        None => {
            // First message: initialize PRNGs with message hash
            let msg_hash = blake3::hash(plaintext);
            state.prng = Some(init_dual_prng(&state.clutch_seed, msg_hash.as_bytes()));
            generate_dual_salt(state.prng.as_mut().unwrap())
        }
    };

    // 3. Derive encryption key (message_key + salt)
    let encryption_key = blake3::keyed_hash(
        b"PHOTON_CHAIN_encryption_________",
        &[message_key.as_bytes(), &salt].concat()
    );

    // 4. Encrypt with ChaCha20-Poly1305
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&encryption_key.as_bytes()[..32]));
    let nonce = derive_nonce(state.sequence);
    let ciphertext = cipher.encrypt(&nonce, plaintext).expect("encryption should not fail");

    // 5. Advance chain state (includes memory-hard scratch)
    let message_hash = blake3::hash(plaintext);
    state.current = blake3::keyed_hash(
        b"PHOTON_CHAIN_advance____________",
        &[&state.current[..], message_hash.as_bytes(), scratch_hash.as_bytes()].concat()
    ).as_bytes().clone();
    state.sequence += 1;

    // 6. Trigger background precomputation for next message
    state.precompute_next_scratch();

    EncryptedMessage {
        sequence: state.sequence - 1,
        salt,
        ciphertext,
    }
}

fn generate_dual_salt(prng: &mut DualPRNG) -> [u8; 64] {
    let mut salt = [0u8; 64];
    prng.chacha_rng.fill_bytes(&mut salt[..32]);  // Cryptographic randomness
    prng.pcg_rng.fill_bytes(&mut salt[32..]);     // Different PRNG family
    salt
}

fn derive_nonce(sequence: u64) -> Nonce {
    let mut nonce_bytes = [0u8; 12];
    nonce_bytes[..8].copy_from_slice(&sequence.to_le_bytes());
    // Remaining 4 bytes are zero (domain separation)
    Nonce::from_slice(&nonce_bytes).clone()
}
```

### 5.2 Receive Flow

```rust
fn receive_message(
    encrypted: &EncryptedMessage,
    state: &mut ChainState
) -> Result<Vec<u8>, ChainError> {
    // 0. Verify sequence number
    if encrypted.sequence != state.sequence {
        return Err(ChainError::SequenceMismatch {
            expected: state.sequence,
            received: encrypted.sequence,
        });
    }

    // 1. Generate same scratch (deterministic)
    let scratch = state.get_scratch();
    let scratch_hash = blake3::hash(&scratch);

    // 2. Derive same message key
    let message_key = blake3::keyed_hash(
        b"PHOTON_CHAIN_message_key________",
        &[&state.current[..], &state.sequence.to_le_bytes(), scratch_hash.as_bytes()].concat()
    );

    // 3. Derive encryption key with received salt
    let encryption_key = blake3::keyed_hash(
        b"PHOTON_CHAIN_encryption_________",
        &[message_key.as_bytes(), &encrypted.salt].concat()
    );

    // 4. Decrypt
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&encryption_key.as_bytes()[..32]));
    let nonce = derive_nonce(state.sequence);
    let plaintext = cipher.decrypt(&nonce, encrypted.ciphertext.as_slice())
        .map_err(|_| ChainError::DecryptionFailed)?;

    // 5. Advance chain (must match sender exactly)
    let message_hash = blake3::hash(&plaintext);
    state.current = blake3::keyed_hash(
        b"PHOTON_CHAIN_advance____________",
        &[&state.current[..], message_hash.as_bytes(), scratch_hash.as_bytes()].concat()
    ).as_bytes().clone();
    state.sequence += 1;

    // 6. Advance PRNG to stay synchronized
    match state.prng.as_mut() {
        Some(prng) => { let _ = generate_dual_salt(prng); },
        None => {
            // First message received: initialize PRNGs
            state.prng = Some(init_dual_prng(&state.clutch_seed, message_hash.as_bytes()));
        }
    }

    // 7. Precompute next scratch
    state.precompute_next_scratch();

    Ok(plaintext)
}
```

---

## 6. Media Encryption (Voice/Video)

### 6.0 Batched Chain Advancement

Video at 30fps would require 30 chain advancements per second, which is excessive. Instead, we batch:

```rust
const MEDIA_BATCH_DURATION_MS: u64 = 1000;  // 1 second batches

struct MediaChainState {
    /// Text chain (shared with text messages)
    text_chain: ChainState,

    /// Current media batch key (derived from text chain)
    batch_key: [u8; 32],

    /// Frame counter within current batch
    batch_frame: u32,

    /// Batch start timestamp
    batch_start: Instant,
}
```

### 6.1 Batch Key Derivation

```rust
fn derive_batch_key(chain_state: &[u8; 32], batch_number: u64) -> [u8; 32] {
    *blake3::keyed_hash(
        b"PHOTON_CHAIN_media_batch________",
        &[chain_state, &batch_number.to_le_bytes()].concat()
    ).as_bytes()
}
```

### 6.2 Frame Encryption

```rust
fn encrypt_media_frame(
    frame_data: &[u8],
    state: &mut MediaChainState
) -> EncryptedFrame {
    // Check if we need new batch
    if state.batch_start.elapsed().as_millis() >= MEDIA_BATCH_DURATION_MS as u128 {
        // Advance text chain (triggers memory-hard computation)
        advance_chain_for_media(&mut state.text_chain);

        // Derive new batch key
        state.batch_key = derive_batch_key(
            &state.text_chain.current,
            state.text_chain.sequence
        );
        state.batch_frame = 0;
        state.batch_start = Instant::now();
    }

    // Derive per-frame key (fast, no memory-hard)
    let frame_key = blake3::keyed_hash(
        b"PHOTON_CHAIN_frame_key__________",
        &[&state.batch_key[..], &state.batch_frame.to_le_bytes()].concat()
    );

    // Encrypt frame with ChaCha20 (no Poly1305 auth - UDP may lose frames)
    let mut ciphertext = frame_data.to_vec();
    let mut cipher = ChaCha20::new(
        Key::from_slice(&frame_key.as_bytes()[..32]),
        &derive_nonce(state.batch_frame as u64)
    );
    cipher.apply_keystream(&mut ciphertext);

    state.batch_frame += 1;

    EncryptedFrame {
        batch_sequence: state.text_chain.sequence,
        frame_number: state.batch_frame - 1,
        ciphertext,
    }
}
```

### 6.3 Frame Decryption

```rust
fn decrypt_media_frame(
    encrypted: &EncryptedFrame,
    state: &mut MediaChainState
) -> Result<Vec<u8>, ChainError> {
    // Sync batch if needed
    if encrypted.batch_sequence != state.text_chain.sequence {
        // Advance chain to match (may need to skip batches)
        while state.text_chain.sequence < encrypted.batch_sequence {
            advance_chain_for_media(&mut state.text_chain);
        }
        state.batch_key = derive_batch_key(
            &state.text_chain.current,
            state.text_chain.sequence
        );
    }

    // Derive frame key
    let frame_key = blake3::keyed_hash(
        b"PHOTON_CHAIN_frame_key__________",
        &[&state.batch_key[..], &encrypted.frame_number.to_le_bytes()].concat()
    );

    // Decrypt
    let mut plaintext = encrypted.ciphertext.clone();
    let mut cipher = ChaCha20::new(
        Key::from_slice(&frame_key.as_bytes()[..32]),
        &derive_nonce(encrypted.frame_number as u64)
    );
    cipher.apply_keystream(&mut plaintext);

    Ok(plaintext)
}
```

**Note:** Media frames use ChaCha20 without Poly1305 authentication. UDP may drop or reorder frames, and authenticating every frame is expensive. The batch key provides implicit authentication—only someone with the chain state can derive valid frame keys.

---

## 7. Error Handling and Resynchronization

### 7.0 Sequence Mismatch

If received sequence doesn't match expected:

```rust
enum SyncStrategy {
    /// Receiver is behind: skip forward
    SkipForward { frames_to_skip: u64 },

    /// Receiver is ahead: sender may have restarted
    WaitForRetransmit,

    /// Gap too large: requires full resync
    RequiresResync,
}

fn handle_sequence_mismatch(
    expected: u64,
    received: u64
) -> SyncStrategy {
    if received > expected && received - expected < 100 {
        // Small gap: skip forward (lost packets)
        SyncStrategy::SkipForward {
            frames_to_skip: received - expected
        }
    } else if expected > received && expected - received < 10 {
        // Duplicate or out-of-order: ignore
        SyncStrategy::WaitForRetransmit
    } else {
        // Large gap: chain state irrecoverable
        SyncStrategy::RequiresResync
    }
}
```

### 7.1 Resynchronization Protocol

When chains desync beyond recovery:

```rust
/// Sent by party detecting desync
struct ResyncRequest {
    /// Hash of our current chain state (proves we had valid state)
    state_hash: [u8; 32],

    /// Our current sequence number
    sequence: u64,

    /// Signed with device key (proves identity)
    signature: [u8; 64],
}

/// Response with enough info to resync
struct ResyncResponse {
    /// Agreed sequence to resume from
    resume_sequence: u64,

    /// Chain state at resume point (encrypted with shared secret)
    encrypted_state: Vec<u8>,

    signature: [u8; 64],
}
```

**Security consideration:** Resync requires proving identity (device signature) to prevent attackers from forcing resync to known state.

---

## 8. Security Analysis

### 8.0 Forward Secrecy

Each message advances the chain through a memory-hard function:

```
state_n+1 = BLAKE3(state_n || message_hash || scratch_hash)
```

Reversing this requires:
0. Inverting BLAKE3 (computationally infeasible)
1. OR: Possessing all previous messages to replay the chain

Compromise of `state_n` reveals nothing about `state_0..n-1`.

### 8.1 Break Requirements

To decrypt a message, attacker must have:

| Layer | Requirement |
|-------|-------------|
| 0 | CLUTCH seed (break 8 primitives or obtain handles) |
| 1 | All previous message hashes (to derive current state) |
| 2 | Memory-hard scratch for that state |
| 3 | Correct sequence number |
| 4 | Salt value (from wire, or break dual PRNG) |

**Minimum: 3 independent compromises** (seed + chain history + network position)

### 8.2 PRNG Security

Dual PRNG provides defense-in-depth:

| PRNG | Type | Strength |
|------|------|----------|
| ChaCha20Rng | Stream cipher CSPRNG | 256-bit security, proven secure |
| Pcg64 | Permuted congruential generator | Statistical quality, different math |

Attacker must break **both** to predict salt values.

### 8.3 Replay Protection

- Sequence numbers are monotonic and verified
- Same sequence + different message = different chain advancement
- Replaying old message fails because receiver's chain has advanced

---

## 9. Performance

| Operation | Time | Notes |
|-----------|------|-------|
| L1 scratch generation | 1-10ms | Precomputed in background |
| Message key derivation | <1μs | BLAKE3 is fast |
| ChaCha20-Poly1305 encrypt | ~1μs/KB | ~4 GB/s on modern CPUs |
| Chain advancement | <1μs | Single BLAKE3 call |
| Total send latency | **0ms perceived** | Scratch precomputed |
| Total receive latency | **<1ms** | Scratch + decrypt |
| Media frame encrypt | <100μs | ChaCha20 only, no auth |
| Media batch advance | ~10ms | Once per second |

---

## 10. Implementation Checklist

**Core:**
- [ ] ChainState struct with all fields
- [ ] init_chain_state from CLUTCH seed
- [ ] init_dual_prng with first message hash
- [ ] generate_l1_scratch with data-dependent reads
- [ ] Background scratch precomputation thread

**Text Messages:**
- [ ] send_message with full flow
- [ ] receive_message with validation
- [ ] generate_dual_salt from both PRNGs
- [ ] derive_nonce from sequence

**Media:**
- [ ] MediaChainState with batching
- [ ] derive_batch_key for 1-second batches
- [ ] encrypt_media_frame / decrypt_media_frame
- [ ] ChaCha20-only encryption (no Poly1305)

**Error Handling:**
- [ ] Sequence mismatch detection
- [ ] Skip-forward for small gaps
- [ ] Resync protocol for large gaps
- [ ] Device signature verification for resync

---

## 11. Relationship to CLUTCH

CHAIN depends on CLUTCH but is conceptually separate:

| Aspect | CLUTCH | CHAIN |
|--------|--------|-------|
| Purpose | Generate shared seed | Encrypt messages |
| Frequency | Once per relationship | Every message |
| Primitives | 8 asymmetric | BLAKE3 + ChaCha20 symmetric |
| Latency | 100-500ms | 0ms perceived |
| Forward secrecy | N/A (one-time) | Yes (per-message) |

CLUTCH creates the seed. CHAIN uses it forever.

---

**End of Specification**
