# CLUTCH Protocol Specification v3.0

**Protocol:** CLUTCH (Device-Bound Parallel Key Ceremony) + Rolling Chain Encryption
**Author:** Nick Spiker
**Status:** Draft
**License:** MIT OR Apache-2.0
**Date:** December 2025

---

## 0. Abstract

CLUTCH is a one-time, device-bound key generation ceremony combining eight independent cryptographic primitives across diverse mathematical foundations into a single shared seed. This seed bootstraps a rolling-chain encrypted relationship between two parties for text, voice, and video communication.

CLUTCH is not a handshake protocol. It is a **key generation ceremony** performed once per relationship per device pair. Relationship seeds are encrypted at rest using the device's private key, ensuring seeds cannot be extracted even if storage is compromised. All subsequent communication is authenticated by the rolling chain itself—successful decryption *is* authentication.

---

## 1. Design Philosophy

### 1.0 Defense in Parallel

Traditional cryptographic diversity uses fallback schemes—if one breaks, switch to another. CLUTCH instead **combines all schemes simultaneously**. An attacker must break every primitive to derive the shared seed. If any single primitive holds, the seed remains secure.

### 1.1 Pre-Shared Secret Integration

Both parties know each other's handles before the ceremony. Handles are never transmitted over the wire. The handles themselves become a pre-shared secret component mixed into the seed derivation, creating a dependency that cannot be satisfied by cryptanalysis alone.

### 1.2 Self-Authenticating Communication

After CLUTCH completes, no further handshakes or identity proofs are required. The rolling chain state is known only to the two participants. Successful decryption proves possession of the chain state, which proves continuous participation since the ceremony.

### 1.3 Pure P2P with Optional Infrastructure

All communication is peer-to-peer UDP by default. No central servers. Optional infrastructure (FGTW signaling for IP mobility) requires explicit user consent and is clearly documented regarding metadata implications.

---

## 2. Cryptographic Primitives

### 2.0 Eight-Primitive CLUTCH Bundle

CLUTCH employs eight key exchange primitives spanning four mathematical families:

| Family | Primitives | Combined Pubkey Size |
|--------|------------|---------------------|
| Classical ECC | 3 | 162 B |
| Structured Lattice | 2 | 2,798 B |
| Unstructured Lattice | 1 | 15,632 B |
| Code-Based | 2 | 531,405 B |
| **Total** | **8** | **~550 KB** |

### 2.1 Classical Elliptic Curve (3 primitives)

| Primitive | Curve | Field | Origin | Pubkey | Shared |
|-----------|-------|-------|--------|--------|--------|
| X25519 | Curve25519 | Montgomery | djb (2006) | 32 B | 32 B |
| ECDH-P384 | P-384 | Weierstrass | NIST/NSA (2000) | 97 B | 48 B |
| ECDH-secp256k1 | secp256k1 | Koblitz | Certicom (2000) | 33 B | 32 B |

**Rationale:** Three curves with different constants, field representations, and origins. Covers curve-specific attacks, implementation bugs, and potential NIST backdoors.

### 2.2 Structured Lattice (2 primitives)

| Primitive | Problem | Ring | Origin | Pubkey | Shared |
|-----------|---------|------|--------|--------|--------|
| ML-KEM-1024 | Module-LWE | Polynomial | IBM/EU (2017) | 1,568 B | 32 B |
| NTRU-HPS-4096-821 | NTRU | NTRU | HPS (1996) | 1,230 B | 32 B |

**Rationale:** Different ring structures and security reductions. ML-KEM is NIST FIPS 203 standardized. NTRU predates it by 21 years with different problem formulation.

### 2.3 Unstructured Lattice (1 primitive)

| Primitive | Problem | Structure | Origin | Pubkey | Shared |
|-----------|---------|-----------|--------|--------|--------|
| FrodoKEM-976 | Plain LWE | None | MSR (2016) | 15,632 B | 24 B |

**Rationale:** No ring structure to exploit. If structured lattices fall to ring-specific attacks, unstructured LWE provides fallback.

### 2.4 Code-Based (2 primitives)

| Primitive | Problem | Code Type | Origin | Pubkey | Shared |
|-----------|---------|-----------|--------|--------|--------|
| HQC-256 | Syndrome | Quasi-cyclic | French (2017) | 7,245 B | 64 B |
| McEliece-460896 | Syndrome | Binary Goppa | McEliece (1978) | 524,160 B | 32 B |

**Rationale:** Entirely different mathematics from lattices. McEliece unbroken for 47 years. HQC provides diversity within code-based family.

### 2.5 Rolling Chain Primitives (per-message)

| Primitive | Purpose | Family |
|-----------|---------|--------|
| BLAKE3 | Hash/KDF | Hash function |
| ChaCha20-Poly1305 | AEAD encryption | Stream cipher |
| L1 Memory-Hard Scratch | Chain advancement | Memory-hard function |
| ChaCha20Rng | PRNG salt source 0 | CSPRNG |
| Pcg64 | PRNG salt source 1 | Fast PRNG |

**Total: 13 independent cryptographic elements** (8 in CLUTCH + 5 in rolling chain)

---

## 3. Handle Structure

### 3.0 Handle Definition

A handle is a human-readable identifier cryptographically bound to a public key bundle containing all eight CLUTCH primitives. Handles are encoded using VSF (Versatile Storage Format).

### 3.1 Handle Properties

- **Human-readable:** Users reference handles by name (e.g., `fractal decoder`)
- **Self-authenticating:** Handle name is cryptographically bound to key material via memory-hard proof
- **Never transmitted:** Handles are exchanged out-of-band only, NEVER over the wire
- **Deterministic proof:** Same handle → same handle proof (enables decentralized verification)

### 3.2 Handle Secret

The handle secret is the seed from which all keypairs are derived:
- **Never stored** on any device
- **Never transmitted** over any wire  
- Regenerated from user memory or social recovery when needed
- Protected by memory-hard proof-of-work (~1 second computation)

### 3.3 Handle Proof

```rust
const SIZE: usize = 24_873_856; // 24MB scratch buffer
const ROUNDS: usize = 17;       // ~1 second on 2025 hardware

pub fn handle_proof(handle_hash: &blake3::Hash) -> blake3::Hash {
    let mut scratch = vec![0u8; SIZE];
    let mut round_hash = *handle_hash;
    
    for round in 0..ROUNDS {
        // Variable fill (25-75% of buffer, prevents precomputation)
        let fill_size = determine_fill_size(&round_hash, SIZE);
        
        // Sequential hash chain (memory-hard, non-seekable)
        fill_sequential(&mut scratch, fill_size, &round_hash);
        
        // Data-dependent reads (cache-hostile, ASIC-resistant)
        fill_random_reads(&mut scratch, fill_size, SIZE, &round_hash);
        
        // Advance to next round
        round_hash = blake3::hash(&scratch);
    }
    
    round_hash // Public handle proof
}
```

**Security properties:**
- Anti-squatting: ~1 second per handle makes bulk registration expensive
- ASIC-resistant: Variable fill and data-dependent reads
- Deterministic: Same handle always produces same proof
- Verifiable: Anyone can recompute to verify ownership

---

## 4. CLUTCH Ceremony

### 4.0 Prerequisites

Before CLUTCH:
0. Alice knows Bob's handle (obtained out-of-band)
1. Bob knows Alice's handle (obtained out-of-band)
2. Both parties possess their own handle secrets
3. Both have computed their own handle proofs

### 4.1 Initiator Determination

Deterministic, no negotiation required:

```rust
fn is_initiator(
    our_handle_proof: &[u8; 32], 
    their_handle_proof: &[u8; 32]
) -> bool {
    our_handle_proof < their_handle_proof  // Lexicographic
}
```

Lower handle proof = initiator. Both parties compute same result independently.

### 4.2 Message Exchange (P2P UDP)

CLUTCH v3 uses **parallel key exchange** where both parties generate and send ephemeral
keys simultaneously. Both parties' ephemeral pubkeys contribute entropy to the final seed.
The protocol is device-bound—seeds are encrypted at rest with the device's private key.

**Parallel Exchange Flow:**

```
Alice                              Bob
  | generate ephemeral              | generate ephemeral
  |--- ClutchOffer (alice_pub) --->|
  |<-- ClutchOffer (bob_pub) ------|  (simultaneous)
  |                                 |
  | Both compute same seed:
  | Seed = BLAKE3(sorted_handles || sorted_pubkeys || device_secrets || ECDH_shared)
  |                                 |
  |--- ClutchComplete (proof) ---->|  (lower handle_proof sends)
```

**Message 0 & 1: ClutchOffer** (Both parties, parallel)

```rust
struct ClutchOffer {
    from_handle_proof: [u8; 32],
    to_handle_proof: [u8; 32],
    ephemeral_pubkeys: EphemeralBundle,
    signature: [u8; 64],  // Ed25519(provenance_hash)
}

struct EphemeralBundle {
    x25519: [u8; 32],           // 0
    p384: [u8; 97],             // 1
    secp256k1: [u8; 33],        // 2
    ml_kem_1024: [u8; 1568],    // 3
    ntru: [u8; 1230],           // 4
    frodo: [u8; 15632],         // 5
    hqc: [u8; 7245],            // 6
    mceliece: [u8; 524160],     // 7
}
// Total: ~550KB per offer
```

Both parties send ClutchOffer as soon as they come online. No initiator/responder
distinction for the key exchange itself - both generate and send immediately.

**Message 2: ClutchComplete** (Lower handle_proof party)

```rust
struct ClutchComplete {
    from_handle_proof: [u8; 32],
    to_handle_proof: [u8; 32],
    proof: [u8; 32],  // BLAKE3(shared_seed || "CLUTCH_v1_complete")
}
```

The party with the lower handle_proof sends ClutchComplete to confirm the ceremony.
The other party verifies the proof matches their derived seed.

### 4.2.1 Legacy v1 Protocol (Backwards Compatibility)

The sequential v1 protocol is still supported for interoperability:

```
Alice (initiator)                  Bob (responder)
     |                                  |
     |--- ClutchInit (alice_pub) --->   |
     |                                  | generate bob_pub
     |<-- ClutchResponse (bob_pub) -----|
     |--- ClutchComplete (proof) --->   |
```

Clients receiving ClutchInit respond with ClutchResponse and use v1 seed derivation.
New clients prefer ClutchOffer for parallel exchange.

### 4.3 Shared Seed Derivation

Both parties compute identical 256-byte seed (2048 bits of entropy).

**Parallel v2 derivation** (both pubkeys contribute entropy):

```rust
fn derive_clutch_seed_parallel(
    our_handle_hash: &[u8; 32],    // BLAKE3(handle) - PRIVATE
    their_handle_hash: &[u8; 32],
    our_ephemeral_pub: &[u8; 32],
    their_ephemeral_pub: &[u8; 32],
    shared_secrets: &SharedSecrets,  // All 8 ECDH/KEM results
) -> [u8; 256] {
    // Sort handles canonically (lower first)
    let (first_handle, second_handle) = sort_pair(our_handle_hash, their_handle_hash);

    // Sort ephemeral pubkeys canonically (both contribute entropy!)
    let (first_pub, second_pub) = sort_pair(our_ephemeral_pub, their_ephemeral_pub);

    let mut hasher = blake3::Hasher::new();
    hasher.update(b"CLUTCH_v2_parallel");
    hasher.update(first_handle);              // Out-of-band secret
    hasher.update(second_handle);
    hasher.update(first_pub);                 // Both parties' randomness
    hasher.update(second_pub);
    hasher.update(&shared_secrets.x25519);    // 32 B
    hasher.update(&shared_secrets.p384);      // 48 B
    hasher.update(&shared_secrets.secp256k1); // 32 B
    hasher.update(&shared_secrets.ml_kem);    // 32 B
    hasher.update(&shared_secrets.ntru);      // 32 B
    hasher.update(&shared_secrets.frodo);     // 24 B
    hasher.update(&shared_secrets.hqc);       // 64 B
    hasher.update(&shared_secrets.mceliece);  // 32 B

    // BLAKE3 XOF: extend output to 256 bytes
    let mut output = [0u8; 256];
    hasher.finalize_xof().fill(&mut output);
    output
}

fn sort_pair<'a>(a: &'a [u8; 32], b: &'a [u8; 32]) -> (&'a [u8; 32], &'a [u8; 32]) {
    if a < b { (a, b) } else { (b, a) }
}
```

**Security note:** Uses private `handle_hash = BLAKE3(handle)`, NOT the public `handle_proof`.
The handle_proof is publicly announced to FGTW; handle_hash is only known to parties who
know the plaintext handle (the out-of-band shared secret).

**Performance:** ~100-500ms total (acceptable for one-time ceremony)

---

## 5. Rolling Chain Initialization

### 5.0 Initial Chain State

Upon CLUTCH completion:

```rust
fn init_chain_state(clutch_seed: &[u8; 32]) -> ChainState {
    let state_0 = blake3::hash(&[
        clutch_seed,
        b"PHOTON_v0_chain_init"
    ].concat());
    
    ChainState {
        current: *state_0.as_bytes(),
        sequence: 0,
        prng: None,  // Initialized on first message
    }
}
```

### 5.1 Dual PRNG Initialization

Triggered by **first message** in either direction:

```rust
struct DualPRNG {
    chacha_rng: ChaCha20Rng,  // CSPRNG
    pcg_rng: Pcg64,           // Fast, different structure
}

fn init_dual_prng(
    clutch_seed: &[u8; 32], 
    first_message_hash: &[u8; 32]
) -> DualPRNG {
    // Separate seeds for each PRNG
    let chacha_seed = blake3::hash(&[
        clutch_seed,
        first_message_hash,
        b"chacha_rng_seed"
    ].concat());
    
    let pcg_seed = blake3::hash(&[
        clutch_seed,
        first_message_hash,
        b"pcg_rng_seed"
    ].concat());
    
    DualPRNG {
        chacha_rng: ChaCha20Rng::from_seed(*chacha_seed.as_bytes()),
        pcg_rng: Pcg64::from_seed(
            u64::from_le_bytes(pcg_seed.as_bytes()[0..8].try_into().unwrap())
        ),
    }
}
```

Both parties compute identical PRNG states deterministically.

---

## 6. Message Encryption (Text)

### 6.0 L1 Memory-Hard Scratch Generation

Background precomputation for chain advancement:

```rust
const L1_SIZE: usize = 32_768;  // 32KB - fits in L1 cache
const ROUNDS: usize = 3;        // ~1-10ms total

fn generate_l1_scratch(
    chain_state: &[u8; 32], 
    size: usize, 
    rounds: usize
) -> Vec<u8> {
    let mut scratch = vec![0u8; size];
    scratch[..32].copy_from_slice(chain_state);
    
    for round in 0..rounds {
        for i in (32..size).step_by(32) {
            // Data-dependent read (cache-hostile)
            let prev_hash = blake3::hash(&scratch[i-32..i]);
            let read_pos = (
                u32::from_le_bytes(
                    prev_hash.as_bytes()[0..4].try_into().unwrap()
                ) as usize % i
            );
            
            // Hash from random earlier position
            let chunk_hash = blake3::hash(&scratch[read_pos..read_pos+32]);
            
            // Mix with round and position
            let mixed = blake3::hash(&[
                chunk_hash.as_bytes(),
                &(round as u64).to_le_bytes(),
                &(i as u64).to_le_bytes()
            ].concat());
            
            scratch[i..i+32].copy_from_slice(mixed.as_bytes());
        }
    }
    
    scratch
}
```

### 6.1 Send Flow

```rust
fn send_message(plaintext: &[u8], state: &mut ChainState) -> Vec<u8> {
    // 0. Generate memory-hard scratch (precomputed in background)
    let scratch = generate_l1_scratch(&state.current, L1_SIZE, ROUNDS);
    
    // 1. Derive message key from scratch
    let message_key = blake3::hash(&[
        &scratch,
        &state.sequence.to_le_bytes(),
        b"message key"
    ].concat());
    
    // 2. Generate dual PRNG salt (64 bytes total)
    let salt = if let Some(ref mut prng) = state.prng {
        generate_dual_salt(prng)
    } else {
        // First message: initialize PRNGs
        let msg_hash = blake3::hash(plaintext);
        state.prng = Some(init_dual_prng(&state.clutch_seed, msg_hash.as_bytes()));
        generate_dual_salt(state.prng.as_mut().unwrap())
    };
    
    // 3. Derive encryption key with salt
    let encryption_key = blake3::hash(&[
        message_key.as_bytes(),
        &salt,
        b"encryption"
    ].concat());
    
    // 4. Encrypt with ChaCha20-Poly1305
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&encryption_key[..32]));
    let nonce = Nonce::from_slice(&state.sequence.to_le_bytes()[..12]);
    let ciphertext = cipher.encrypt(nonce, plaintext).expect("encryption");
    
    // 5. Advance chain state
    let message_hash = blake3::hash(plaintext);
    state.current = blake3::hash(&[
        &state.current,
        message_hash.as_bytes(),
        blake3::hash(&scratch).as_bytes()
    ].concat());
    state.sequence += 1;
    
    // 6. Encode for wire (VSF format)
    EncryptedMessage {
        sequence: state.sequence - 1,
        salt,
        ciphertext,
    }.to_vsf()
}

fn generate_dual_salt(prng: &mut DualPRNG) -> [u8; 64] {
    let mut salt = [0u8; 64];
    prng.chacha_rng.fill_bytes(&mut salt[..32]);  // Bytes 0-31
    prng.pcg_rng.fill_bytes(&mut salt[32..]);     // Bytes 32-63
    salt
}
```

### 6.2 Receive Flow

```rust
fn receive_message(
    encrypted: &EncryptedMessage, 
    state: &mut ChainState
) -> Result<Vec<u8>, Error> {
    // 0. Verify sequence
    if encrypted.sequence != state.sequence {
        return Err(Error::SequenceMismatch);
    }
    
    // 1. Generate same scratch (deterministic from chain state)
    let scratch = generate_l1_scratch(&state.current, L1_SIZE, ROUNDS);
    
    // 2. Derive message key
    let message_key = blake3::hash(&[
        &scratch,
        &state.sequence.to_le_bytes(),
        b"message_key"
    ].concat());
    
    // 3. Derive encryption key with received salt
    let encryption_key = blake3::hash(&[
        message_key.as_bytes(),
        &encrypted.salt,
        b"encryption"
    ].concat());
    
    // 4. Decrypt
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&encryption_key[..32]));
    let nonce = Nonce::from_slice(&state.sequence.to_le_bytes()[..12]);
    let plaintext = cipher.decrypt(nonce, &encrypted.ciphertext)
        .map_err(|_| Error::DecryptionFailed)?;
    
    // 5. Advance chain (identical to sender)
    let message_hash = blake3::hash(&plaintext);
    state.current = blake3::hash(&[
        &state.current,
        message_hash.as_bytes(),
        blake3::hash(&scratch).as_bytes()
    ].concat());
    state.sequence += 1;
    
    // 6. Advance PRNG to maintain sync
    if let Some(ref mut prng) = state.prng {
        let _ = generate_dual_salt(prng);
    } else {
        // First message received: initialize PRNGs
        state.prng = Some(init_dual_prng(&state.clutch_seed, message_hash.as_bytes()));
    }
    
    Ok(plaintext)
}
```

**Latency:** 0ms perceived (scratch precomputed, XOR instant)

---

## 7. Video Encryption

### 7.0 Codec Selection

**Recommended: H.264 with x264, zerolatency preset**

```rust
// x264 configuration for real-time P2P
x264_param_default_preset(&mut params, "ultrafast", "zerolatency");

// Key settings:
params.b_repeat_headers = 1;     // SPS/PPS in every keyframe
params.b_vfr_input = 0;           // Constant frame rate
params.rc.i_rc_method = X264_RC_ABR;  // Average bitrate
params.rc.i_bitrate = 3000;       // 3 Mbps for 1080p
params.i_keyint_max = 60;         // Keyframe every 2 seconds
params.i_bframe = 0;              // No B-frames (low latency)
params.i_slice_count = 4;         // Enable partial frame sending
```

**Rationale:**
- 10-30ms encode latency with zerolatency tune
- Hardware support everywhere (phones, laptops, embedded)
- Mature tooling (x264 extremely well-optimized)
- Better packet loss recovery than VP8/VP9
- Patent concerns mostly resolved (expired in many regions)

**Quality expectations:**
- Good conditions (WiFi): 720p-1080p @ 30fps
- Typical cellular: 480p @ 24-30fps  
- Bad conditions: 360p @ 15-24fps or audio-only

### 7.1 Video Chain Advancement

Video uses batched advancement (1 second intervals):

```rust
fn video_chain_advancement(
    frames: &[VideoFrame],  // ~30-60 frames
    state: &mut ChainState
) {
    // 0. Hash all frames in this batch
    let frame_hashes: Vec<[u8; 32]> = frames
        .iter()
        .map(|f| *blake3::hash(&f.data).as_bytes())
        .collect();
    
    // 1. Combine frame hashes
    let batch_hash = blake3::hash(&frame_hashes.concat());
    
    // 2. Generate scratch (has 1 second to compute)
    let scratch = generate_l1_scratch(&state.current, L1_SIZE, ROUNDS);
    
    // 3. Advance chain once per second
    state.current = blake3::hash(&[
        &state.current,
        batch_hash.as_bytes(),
        blake3::hash(&scratch).as_bytes()
    ].concat());
    state.sequence += 1;
}
```

**Performance:** ~1% overhead (10ms advancement per 1 second of video)

### 7.2 Video Frame Encryption

```rust
fn encrypt_video_frame(
    frame: &VideoFrame,
    pad: &[u8]  // Precomputed from chain state
) -> Vec<u8> {
    // XOR encryption (instant, wire-speed)
    frame.data
        .iter()
        .zip(pad.iter())
        .map(|(a, b)| a ^ b)
        .collect()
}
```

**Latency:** Microseconds (ChaCha20 runs at 4-8 GB/s, video is 3-5 Mbps)

---

## 8. Network Architecture

### 8.0 Pure P2P Default

All communication is direct peer-to-peer UDP:
- No central servers
- No relay infrastructure
- No metadata collection
- IP changes drop connection (explicit reconnect required)

### 8.1 Optional FGTW Call Forwarding

**User consent required. Off by default.**

```
Settings:
  ☐ Allow call forwarding
    Use FGTW signaling for seamless IP changes during calls.
    Prevents dropped calls when switching networks.
    
    Metadata shared with FGTW:
    - Your IP address when it changes
    - Timestamp of IP changes
    - Handle proofs of both parties in call
    
    FGTW cannot:
    - Read message content (rolling chain encrypted)
    - Impersonate you (requires handle signatures)
    - Access your handle secret
```

### 8.2 FGTW Signaling Protocol

When enabled:

```rust
// User's IP changes (WiFi → Cellular)
fn handle_ip_change(
    new_ip: IpAddr,
    call_state: &CallState,
    fgtw_endpoint: &str
) {
    // 0. Sign IP update with our handle
    let update = IpUpdate {
        handle_proof: our_handle_proof(),
        peer_handle_proof: call_state.peer_handle_proof,
        new_ip,
        timestamp: now(),
        signature: sign_with_handle(&update_bytes),
    };
    
    // 1. Send to FGTW (Cloudflare Worker, Rust)
    send_to_fgtw(fgtw_endpoint, &update);
    
    // 2. FGTW forwards to peer's current IP
    // 3. Peer updates UDP destination
    // 4. Call continues
    
    // Total interruption: <16ms (sub-frame)
}
```

**FGTW Worker (edge.fgtw.org):**

```rust
#[worker::event(fetch)]
async fn handle_ip_update(req: Request, env: Env) -> Result<Response> {
    // 0. Parse and verify signature
    let update: IpUpdate = req.json().await?;
    verify_handle_signature(&update)?;
    
    // 1. Look up peer's current IP (KV store)
    let peer_ip = env.kv("IP_MAPPINGS")
        .get(&update.peer_handle_proof)
        .await?;
    
    // 2. Forward notification to peer
    let notification = IpChangeNotification {
        peer_handle_proof: update.handle_proof,
        new_ip: update.new_ip,
    };
    send_udp_to(peer_ip, &notification)?;
    
    // 3. Update our mapping
    env.kv("IP_MAPPINGS")
        .put(&update.handle_proof, &update.new_ip)
        .await?;
    
    Ok(Response::ok("forwarded"))
    // < 5ms total latency
}
```

**Security properties:**
- FGTW learns: Two handles are in call, IPs, timing
- FGTW cannot: Read content, impersonate users, access handles
- User explicitly consents to metadata tradeoff
- Fallback: If FGTW unavailable, call drops (same as default behavior)

---

## 9. Wire Format (VSF)

All messages encoded with VSF (Versatile Storage Format):

```
EncryptedMessage (VSF):
  sequence: u64           // Message sequence number
  salt: [u8; 64]         // Dual PRNG salt (32 + 32)
  ciphertext: Vec<u8>    // ChaCha20-Poly1305 output
```

**Network observers see:** Generic VSF traffic (indistinguishable from other VSF applications after 40-year adoption timeline)

**Overhead:** 64 bytes salt + 16 bytes Poly1305 tag = 80 bytes per message

---

## 10. Security Analysis

### 10.0 Break-in Requirements

An attacker must compromise **ALL** of:

**Layer 0: CLUTCH Seed**
- Break all 8 primitives simultaneously:
  - X25519 (Curve25519 ECDLP)
  - ECDH-P384 (P-384 ECDLP)  
  - ECDH-secp256k1 (secp256k1 ECDLP)
  - ML-KEM-1024 (Module-LWE)
  - NTRU (NTRU problem)
  - FrodoKEM (unstructured LWE)
  - HQC (quasi-cyclic syndrome decoding)
  - McEliece (binary Goppa syndrome decoding - unbroken 47 years)

**OR**

- Obtain both handles (never transmitted, out-of-band only)
- Reverse handle proof (memory-hard, 24MB, 17 rounds)

**Layer 1: Chain State**
- Possess all previous message hashes
- Compute memory-hard L1 scratch for each advancement
- Break BLAKE3 hash function

**Layer 2: PRNG Salt**
- Break ChaCha20Rng **AND** Pcg64 simultaneously
- Obtain first message hash (for PRNG seeding)

**Layer 3: Message Encryption**
- Break ChaCha20-Poly1305 AEAD
- Break BLAKE3 key derivation

**Layer 4: Network Position**
- Intercept P2P UDP traffic (no central server to compromise)

**Total: Minimum 3 independent compromises required** (CLUTCH + chain + network position)

### 10.1 Primitives Summary

**Foundation (one-time ceremony):**
- 8 key exchange primitives (CLUTCH bundle)
- 2 handle proofs (pre-shared secret, memory-hard)

**Ongoing (per-message):**
- BLAKE3 (hash/KDF)
- ChaCha20-Poly1305 (AEAD encryption)
- L1 memory-hard scratch (chain advancement)
- ChaCha20Rng (PRNG salt source 0)
- Pcg64 (PRNG salt source 1)

**Total: 13 independent cryptographic elements**

### 10.2 Known Limitations

**Not defended against:**
- Physical device compromise (mitigated by Ferros 0ms kill-switch)
- Side-channel attacks on implementation
- Social engineering for handles
- Targeted malware with keylogger

**Metadata leakage:**
- Default (P2P): IP addresses visible to peer only
- With FGTW forwarding: IP changes visible to FGTW infrastructure
- Packet timing and sizes (unavoidable without padding)

---

## 11. Performance Targets

| Operation | Target | Actual |
|-----------|--------|--------|
| CLUTCH ceremony | <500ms | 100-500ms |
| Handle proof generation | ~1s | ~1s (17 rounds, 24MB) |
| L1 scratch generation | <10ms | 1-10ms (3 rounds, 32KB) |
| Message encryption | =0ms | =0ms (XOR with precomputed pad) |
| Message send latency | 0ms perceived | 0ms (precomputed ready) |
| Video frame encryption | <100μs | ~50μs (ChaCha20 at GB/s) |
| Video chain advancement | <10ms/s | ~1% overhead |
| FGTW IP update | <16ms | <5ms (Cloudflare edge) |

---

## 12. Implementation Status

**Working:**
- ✅ CLUTCH ceremony (X25519-only MVP)
- ✅ Rolling chain encryption (text messages)
- ✅ L1 memory-hard scratch
- ✅ Dual PRNG salt generation
- ✅ P2P UDP communication
- ✅ One-frame UI load
- ✅ Zero-latency send (precomputed pads)
- ✅ Video streaming with H.264

**In Development:**
- 🚧 Full multi-primitive CLUTCH (12 primitives across 4 classes)
  - ✅ Egg collection structure (ClutchEggs with domain separation)
  - ✅ Function stubs for all 12 primitives
  - ✅ Test infrastructure (15 eggs: 6 context + 9 KEMs)
  - ⏳ Real implementations (currently placeholder stubs)
  - ⏳ Wire format extension for ~550KB ClutchOffer
  - ⏳ Performance optimization (<500ms target)
- 🚧 FGTW call forwarding (optional)
- 🚧 VSF wire format (currently using interim format)

**Future:**
- ⏳ Avalanche shuffle (prepend hashes, generate 1MB cascade)
- ⏳ Ferros OS integration
- ⏳ Hardware encoder optimization (mobile)
- ⏳ Social key recovery for handles

---

## 13. License

MIT OR Apache-2.0 (dual licensed)

---

## Appendix A: Zero-Index Philosophy

This specification uses zero-indexing throughout:
- Sections: 0-13
- Phases: 0-N
- Array indices: 0-N
- Primitives in bundle: 0-7

**Rationale:** Aligns with hardware reality, Rust conventions, and mathematical foundations. Humans count from 1, computers count from 0. This is a computer protocol.

---

**End of Specification**