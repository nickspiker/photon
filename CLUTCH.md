# CLUTCH Protocol Specification v3.0

**Protocol:** CLUTCH (Device-Bound Parallel Key Ceremony)
**Related:** CHAIN.md (Rolling Chain Encryption)
**Author:** Nick Spiker
**Status:** Draft
**License:** MIT OR Apache-2.0
**Date:** December 2025

---

## 0. Abstract

CLUTCH is a one-time, device-bound key generation ceremony combining eight independent cryptographic primitives across diverse mathematical foundations into a single 256-byte shared seed. This seed bootstraps the CHAIN protocol (see CHAIN.md) for text, voice, and video communication.

CLUTCH is not a handshake protocol. It is a **key generation ceremony** performed once per relationship per device pair. Relationship seeds are encrypted at rest using the device's private key, ensuring seeds are difficult to extract even if storage is compromised. All subsequent communication uses the CHAIN protocol—successful decryption *is* authentication.

---

## 1. Design Philosophy

### 1.0 Defense in Parallel

Traditional cryptographic diversity uses fallback schemes—if one breaks, the system is compromised. CLUTCH instead **combines all schemes simultaneously**. An attacker must break every primitive to derive the shared seed. If any single primitive holds, the seed remains secure.

### 1.1 Pre-Shared Secret Integration

All parties know each other's handles before the ceremony. Handles are never transmitted over the wire. The handles themselves become a pre-shared secret component mixed into the seed derivation, creating a dependency that cannot be satisfied by cryptanalysis alone.

### 1.2 Self-Authenticating Communication

After CLUTCH completes, no further handshakes or identity proofs are required. The CHAIN protocol state is known only to the two participants. Successful decryption proves possession of the chain state, which proves continuous participation since the ceremony. See CHAIN.md for rolling encryption details.

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

See CHAIN.md for full specification. Summary:

| Primitive | Purpose | Family |
|-----------|---------|--------|
| BLAKE3 | Hash/KDF | Hash function |
| ChaCha20-Poly1305 | AEAD encryption | Stream cipher |
| L1 Memory-Hard Scratch | Chain advancement | Memory-hard function |
| ChaCha20Rng | PRNG salt source 0 | CSPRNG |
| Pcg64 | PRNG salt source 1 | Fast PRNG |

**Total: 14 cryptographic elements** (8 in CLUTCH + 1 handle PSK + 5 in CHAIN)

### 2.6 Hardware-Attested Contextual KDF (HAC-KDF)

Device-bound key derivation without storing secrets on disk:

```rust
fn hac_kdf(input: &[u8]) -> [u8; 32]
```

**Ideal (TPM-backed):**
```rust
output = BLAKE3(
    app_signing_pubkey ||
    tpm_secret ||           // Hardware-unique, changes on factory reset
    user_number ||          // OS user profile ID
    input
)
```

**Current Android Workaround:**
```rust
let android_id = get_android_id();  // 64-bit, scoped to app signing key
let user_num = get_user_number();   // Android multi-user profile ID
output = BLAKE3(android_id || user_num || input)
```

**Properties:**
- **Hardware-bound:** Keys derivable only on specific device
- **App-scoped:** Different apps get different outputs
- **User-isolated:** Different OS users get different outputs
- **Reset boundary:** Factory reset generates new `tpm_secret`
- **Deterministic:** Same inputs always produce same output
- **Non-extractable** (ideal): Secret never leaves TPM hardware

**Purpose:** Derive device-specific cryptographic material for:
0. Device identity (Ed25519 keypair per device)
1. Multi-device federation (each device has independent keys)
2. Device Quorum (collective authorization across user's devices)

**Limitations (Android Workaround):**
- `ANDROID_ID` is 64-bit (reduced entropy vs 256-bit ideal)
- Software-stored (root can extract vs hardware-isolated)

**Security Model:**
- ✅ Remote attacks (requires physical device access)
- ✅ App cloning (different signing key → different output)
- ✅ Cross-user leakage (user_number scoping)
- ⚠️ Root access (workaround only - ideal TPM resists)
- ❌ Physical device theft while unlocked or unencrypted filesystem

**Potential Integration with TOKEN Device Quorum:**
```rust
// Device initialization
let seed = hac_kdf(b"photon_device_identity_v0");
let keypair = Ed25519::from_seed(&seed);

// Device Quorum revocation (threshold vote from other devices)
if watch.vote_revoke(phone) && car.vote_revoke(phone) {
    publish_revocation(phone.pubkey, vec![watch.sig, car.sig]);
}
```

---

## 3. Handle Structure

### 3.0 Handle Definition

A handle is a human-readable identifier cryptographically bound to a public key bundle containing all eight CLUTCH primitives. Handles are encoded using VSF (Versatile Storage Format).

### 3.1 Handle Properties

- **Human-readable:** Users reference handles by name (e.g., `fractal decoder`)
- **Self-authenticating:** Handle name is cryptographically bound to key material via memory-hard proof
- **Never transmitted:** Handles are exchanged out-of-band only, NEVER over the wire
- **Deterministic proof:** Same handle → same handle proof (enables decentralized verification)

### 3.2 Handle Identity Terms

CLUTCH distinguishes between three levels of handle identity:

| Term | Formula | Visibility | Purpose |
|------|---------|------------|---------|
| **handle** | User-chosen text | Out-of-band only | Human reference |
| **handle_hash** | `BLAKE3(handle)` | CLUTCH messages only | Private identity seed |
| **handle_proof** | `memory_hard(handle_hash)` | Public (FGTW) | Anti-squatting, verification |

**handle_hash** is the PRIVATE identity seed:
- Computed instantly: `BLAKE3(VsfType::x(handle).flatten())`
- Only known to parties who know the plaintext handle
- Used for ceremony_id derivation and CLUTCH offer matching
- Sent in CLUTCH messages but only matchable by contacts who know you

**handle_proof** is the PUBLIC identity:
- Expensive to compute (~1 second memory-hard)
- Announced to FGTW for presence/lookup
- Anyone can verify by recomputing from handle
- Not used in CLUTCH messages (too slow, public anyway)

### 3.3 Handle Proof Computation

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

### 3.4 Ceremony Identity (ceremony_id)

The **ceremony_id** is a deterministic identifier for a specific CLUTCH ceremony between parties.
All parties compute the identical ceremony_id independently, enabling offer matching without
prior coordination.

```rust
/// Compute deterministic ceremony identity from sorted handle_hashes
/// Uses memory-hard function to slow brute-force matching attacks
pub fn compute_ceremony_id(handle_hashes: &[[u8; 32]]) -> [u8; 32] {
    // Sort handle_hashes lexicographically (canonical ordering)
    let mut sorted = handle_hashes.to_vec();
    sorted.sort();

    // Domain separation + concatenated sorted hashes
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"PHOTON_CEREMONY_v1");
    for hash in &sorted {
        hasher.update(hash);
    }
    let combined = hasher.finalize();

    // Harden with memory-hard function (~1 second)
    // This prevents brute-force matching by MITM attackers
    *handle_proof(combined.as_bytes()).as_bytes()
}
```

**Properties:**
- **Deterministic:** Same parties → same ceremony_id (no negotiation needed)
- **Scalable:** Works for 2+ party ceremonies (future group CLUTCH)
- **MITM-resistant:** Memory-hard computation prevents brute-force matching
- **Privacy-preserving:** ceremony_id reveals nothing about handles without prior knowledge

**Example (2-party):**
```
Alice's handle_hash: 0x646B83DF...
Bob's handle_hash:   0x7583B495...

Sorted: [0x646B83DF..., 0x7583B495...]
Combined: BLAKE3("PHOTON_CEREMONY_v1" || sorted_hashes)
ceremony_id: handle_proof(combined) // ~1 second
```

**Example (5-party group):**
```
handle_hashes: [0x1A..., 0x3F..., 0x7B..., 0x8C..., 0xE2...]  // Already sorted
Combined: BLAKE3("PHOTON_CEREMONY_v1" || all 5 hashes)
ceremony_id: handle_proof(combined) // Same ~1 second regardless of party count
```

All parties compute identical ceremony_id independently from their shared knowledge of handles.

---

## 4. CLUTCH Ceremony

### 4.0 Prerequisites

Before CLUTCH:
0. Alice knows Bob's handle (obtained out-of-band)
1. Bob knows Alice's handle (obtained out-of-band)
2. All parties have computed each other's handle_hash (from known handles)
3. All have computed the shared ceremony_id (from sorted handle_hashes)

### 4.1 Slot-Based Ceremony (No Initiator)

There is no "initiator" in CLUTCH. The ceremony has N slots (one per handle_hash in sorted order).
Each party fills their slot when ready. Order of arrival doesn't matter—ceremony completes
when all slots are filled.

```rust
struct CeremonyState {
    ceremony_id: [u8; 32],
    slots: Vec<Option<PartyMaterial>>,  // Indexed by sorted handle_hash position
}

struct PartyMaterial {
    handle_hash: [u8; 32],
    offer: ClutchFullOffer,       // Their 8 pubkeys
    response: Option<ClutchKemResponse>,  // Their KEM ciphertexts to us
}
```

**Completion condition:** All N slots have offers AND all N slots have responses addressed to us.

### 4.2 Message Exchange (TCP + PT/UDP)

CLUTCH uses **organic slot-filling** where parties generate and broadcast ephemeral keys
whenever ready. No coordination, no initiator, no linear flow. All parties' ephemeral
pubkeys contribute entropy to the final seed.

**Transport:** Full offers are ~550KB, too large for UDP. We use:
- **Primary:** PT (Photon Transport) - reliable UDP with acknowledgments
- **Fallback:** TCP direct connection to peer's PHOTON_PORT

**Organic Slot-Filling (2-party example):**

```
Alice                              Bob
  | compute ceremony_id             | compute ceremony_id
  | generate ephemeral keys         |
  |--- ClutchFullOffer ----------->|  (fills Alice's slot)
  |                                 | generate ephemeral keys
  |<-- ClutchFullOffer ------------|  (fills Bob's slot)
  |                                 |  (Bob sees Alice's offer, encapsulates)
  |<-- ClutchKemResponse ----------|  (Bob's response to Alice)
  | (Alice sees Bob's offer)        |
  |--- ClutchKemResponse --------->|  (Alice's response to Bob)
  |                                 |
  | All slots filled → derive shared_seed
```

**N-party example (5 people):**
```
ceremony_id = hash(sorted [A, B, C, D, E])
Slots: [_, _, _, _, _]  (5 empty slots)

C comes online first:  [_, _, C, _, _]
A and E join:          [A, _, C, _, E]
D joins:               [A, _, C, D, E]
B finally joins:       [A, B, C, D, E]  ← All offers received

Now each party sends KemResponses to all others.
Ceremony completes when each party has responses from all others.
```

**No ordering dependency.** Parties can join in any order, go offline and rejoin,
or have asymmetric network conditions. The ceremony state is just a set of slots.

**Message 0..N-1: ClutchFullOffer** (All parties, ~550KB each)

VSF-formatted message with Ed25519 signature:

```rust
// VSF section: "clutch_full_offer"
// Header provenance (hp): ceremony_id
struct ClutchFullOffer {
    // Sorted list of all handle_hashes in ceremony (1 to N parties)
    handle_hashes: Vec<[u8; 32]>,  // v(b'h') - sorted lexicographically

    // Sender's public keys for all 8 primitives
    x25519: [u8; 32],         // kx - X25519 public key
    p384: [u8; 97],           // kp - P-384 public key
    secp256k1: [u8; 33],      // kk - secp256k1 public key
    p256: [u8; 65],           // kp - P-256 public key (for iOS interop)
    frodo: [u8; 15632],       // kf - FrodoKEM-976 public key
    ntru: [u8; 1230],         // kn - NTRU-HPS-4096-821 public key
    mceliece: [u8; 524160],   // kl - McEliece-460896 public key
    hqc: [u8; 7245],          // kh - HQC-256 public key
}
// Total: ~550KB (McEliece dominates)
// handle_hashes overhead: 32 bytes per party (negligible vs McEliece)
```

**Contact matching:** Receiver checks if ALL handle_hashes are in their contact list.
A ceremony only proceeds if every party knows every other party's handle.

**Message N..2N-1: ClutchKemResponse** (All parties, ~31KB each)

After receiving all other parties' offers, encapsulate to each party's public keys:

```rust
// VSF section: "clutch_kem_response"
// Header provenance (hp): ceremony_id (must match offers)
struct ClutchKemResponse {
    handle_hashes: Vec<[u8; 32]>,  // v(b'h') - same sorted list

    // KEM ciphertexts for each recipient (indexed by sorted handle_hash position)
    // Each recipient gets their own set of 4 ciphertexts
    kem_responses: Vec<KemCiphertexts>,  // v(b'k') - one per other party
}

struct KemCiphertexts {
    recipient_hash: [u8; 32],     // hb - which party this is for
    frodo_ct: Vec<u8>,            // v(b'f') - FrodoKEM ciphertext
    ntru_ct: Vec<u8>,             // v(b'n') - NTRU ciphertext
    mceliece_ct: Vec<u8>,         // v(b'l') - McEliece ciphertext
    hqc_ct: Vec<u8>,              // v(b'h') - HQC ciphertext
}
// Total: ~31KB per recipient
```

Each party decapsulates the KEM responses addressed to them, yielding shared secrets
with every other party. Final seed combines ALL pairwise shared secrets.

### 4.3 Shared Seed Derivation

All parties compute identical 256-byte seed (2048 bits of entropy).

**N-party seed derivation:**

```rust
/// Pairwise shared secrets with one other party (8 KEM/ECDH results)
struct PairwiseSecrets {
    peer_hash: [u8; 32],          // Who these secrets are with
    x25519: [u8; 32],
    p384: [u8; 48],
    secp256k1: [u8; 32],
    ml_kem: [u8; 32],
    ntru: [u8; 32],
    frodo: [u8; 24],
    hqc: [u8; 64],
    mceliece: [u8; 32],
}

fn derive_clutch_seed(
    handle_hashes: &[[u8; 32]],           // All parties, sorted
    ephemeral_pubkeys: &[[u8; 32]],       // All X25519 pubkeys, sorted by handle_hash
    pairwise_secrets: &[PairwiseSecrets], // Our secrets with each other party
) -> [u8; 256] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"CLUTCH_v3_nparty");

    // All handle_hashes (already sorted)
    for hash in handle_hashes {
        hasher.update(hash);
    }

    // All ephemeral pubkeys (sorted same order as handle_hashes)
    for pubkey in ephemeral_pubkeys {
        hasher.update(pubkey);
    }

    // All pairwise shared secrets (sorted by peer_hash for determinism)
    let mut sorted_secrets = pairwise_secrets.to_vec();
    sorted_secrets.sort_by_key(|s| s.peer_hash);

    for secrets in &sorted_secrets {
        hasher.update(&secrets.peer_hash);
        hasher.update(&secrets.x25519);
        hasher.update(&secrets.p384);
        hasher.update(&secrets.secp256k1);
        hasher.update(&secrets.ml_kem);
        hasher.update(&secrets.ntru);
        hasher.update(&secrets.frodo);
        hasher.update(&secrets.hqc);
        hasher.update(&secrets.mceliece);
    }

    // BLAKE3 XOF: extend output to 256 bytes
    let mut output = [0u8; 256];
    hasher.finalize_xof().fill(&mut output);
    output
}
```

**Example (3-party: Alice, Bob, Carol):**
```
handle_hashes: [A, B, C]  (sorted)
ephemeral_pubkeys: [pk_A, pk_B, pk_C]  (same order)

Alice computes secrets with: Bob (8 values), Carol (8 values)
Bob computes secrets with: Alice (8 values), Carol (8 values)
Carol computes secrets with: Alice (8 values), Bob (8 values)

All three hash the same:
  - 3 handle_hashes
  - 3 ephemeral pubkeys
  - 2 sets of 8 pairwise secrets each (sorted by peer)

Result: Identical 256-byte seed for all parties
```

**Security note:** Uses private `handle_hash = BLAKE3(handle)`, NOT the public `handle_proof`.
The handle_proof is publicly announced to FGTW; handle_hash is only known to parties who
know the plaintext handle (the out-of-band shared secret).

**Performance:** ~100-5,000ms total (acceptable for one-time ceremony)

---

## 5. Post-Ceremony: CHAIN Protocol

After CLUTCH ceremony completes successfully, each party has identical 256-byte seeds. All subsequent communication uses the CHAIN protocol (see **CHAIN.md** for full specification).

**Summary:**
- Rolling chain state advances with every message (forward secrecy)
- Memory-hard L1 scratch generation (32KB, 1-10ms precomputed)
- Diverse PRNG salt (ChaCha20Rng + Pcg64)
- ChaCha20-Poly1305 AEAD encryption
- 0ms perceived latency (scratch precomputed in background)
- Media uses 1-second batched chain advancement

---

## 6. Network Architecture

### 6.0 Pure P2P Default

All communication is direct peer-to-peer UDP:
- No central servers
- No relay infrastructure
- No metadata collection
- IP changes drop connection (explicit reconnect required)
- Seeds to find initial peers are hardcoded into client

### 6.1 Optional FGTW Call Forwarding

**User consent required. Off by default.**

```
Settings:
  ☐ Allow call forwarding
    Use FGTW signaling for invisible IP changes during calls.
    Prevents dropped calls when switching networks.

    Metadata shared with FGTW:
    - Your IP address when it changes
    - Eagle Timestamp of IP changes
    - Device pubkeys of all parties in call

    FGTW cannot:
    - Read message content (rolling chain encrypted)
    - Impersonate you (requires device signatures)
    - Link device pubkey to your handle
```

### 6.2 FGTW Signaling Protocol

When enabled:

```rust
// User's IP changes (WiFi → Cellular)
fn handle_ip_change(
    new_ip: IpAddr,
    call_state: &CallState,
    fgtw_endpoint: &str
) {
    // 0. Sign IP update with our device key (from HAC-KDF)
    let update = IpUpdate {
        ceremony_id: call_state.ceremony_id,     // Registry lookup key
        device_pubkey: our_device_pubkey(),      // Ed25519 from HAC-KDF
        new_ip,
        timestamp: now(),
        signature: sign_with_device_key(&update_bytes),
    };

    // 1. Send to FGTW (Cloudflare Worker, Rust)
    send_to_fgtw(fgtw_endpoint, &update);

    // 2. FGTW broadcasts to all peers in registry
    // 3. Peers update UDP destination
    // 4. Call continues

    // Total interruption: <16ms (sub-frame)
}
```

**Why device identity, not handle:**
- A user may have multiple devices in the same conversation
- Each device has a unique IP address
- FGTW broadcasts to all devices in the call registry
- Device pubkey from HAC-KDF is already unique per-device

**FGTW Worker (edge.fgtw.org):**

```rust
#[worker::event(fetch)]
async fn handle_ip_update(req: Request, env: Env) -> Result<Response> {
    // 0. Parse and verify signature
    let update: IpUpdate = req.json().await?;
    verify_device_signature(&update)?;

    // 1. Get all devices in this call registry
    let registry_key = &update.ceremony_id;
    let devices: Vec<DeviceEntry> = env.kv("CALL_REGISTRY")
        .get(registry_key)
        .await?;

    // 2. Broadcast IP change to all other devices
    let notification = IpChangeNotification {
        device_pubkey: update.device_pubkey,
        new_ip: update.new_ip,
    };
    for device in devices {
        if device.device_pubkey != update.device_pubkey {
            send_udp_to(device.ip, &notification)?;
        }
    }

    // 3. Update our entry in registry
    update_registry_entry(registry_key, &update.device_pubkey, &update.new_ip).await?;

    Ok(Response::ok("broadcasted"))
    // < 5ms total latency
}
```

**Security properties:**
- FGTW learns: Which devices are in call, their IPs, timing
- FGTW cannot: Read content, impersonate devices, link device to user
- Device pubkeys are ephemeral per-CLUTCH (not persistent identity)
- User explicitly consents to metadata tradeoff
- Fallback: If FGTW unavailable, call drops (same as default behavior)

---

## 7. Wire Format (VSF)

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

## 8. Security Analysis

### 8.0 Break-in Requirements

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

### 8.1 Primitives Summary

**Foundation (one-time ceremony):**
- 8 key exchange primitives (CLUTCH bundle)
- Plaintext handles as pre-shared secret (user types "Joe Walker", we hash it)

**Ongoing (per-message):**
- BLAKE3 (hash/KDF)
- ChaCha20-Poly1305 (AEAD encryption)
- L1 memory-hard scratch (chain advancement)
- ChaCha20Rng (PRNG salt source 0)
- Pcg64 (PRNG salt source 1)

**Total: 14 cryptographic elements** (8 CLUTCH + 1 handle PSK + 5 CHAIN)

### 8.2 Known Limitations

**Not defended against:**
- Physical device compromise
- Side-channel attacks on implementation
- Social engineering for handles
- Targeted malware with keylogger

**Metadata leakage:**
- Default (P2P): IP addresses visible to peer only
- With FGTW forwarding: IP changes visible to FGTW infrastructure
- Packet timing and sizes (unavoidable without padding)

---

## 9. Performance Targets

| Operation | Target | Actual |
|-----------|--------|--------|
| CLUTCH ceremony | <500ms | 100-5,000ms |
| Handle proof generation | ~1s | ~1s (17 rounds, 24MB) |
| L1 scratch generation | <10ms | 1-100ms (3 rounds, 32KB) |
| Message encryption | =0ms | =0ms (XOR with precomputed pad) |
| Message send latency | 0ms perceived | 0ms (precomputed ready) |
| Video frame encryption | <100μs | ~50μs (ChaCha20 at GB/s) |
| Video chain advancement | <10ms/s | ~1% overhead |
| FGTW IP update | <16ms | <5ms (Cloudflare edge) |

---

## 10. Implementation Status

**Working:**
- ✅ Full 8-primitive CLUTCH ceremony (all algorithms)
- ✅ ClutchFullOffer VSF messages (~550KB with all pubkeys)
- ✅ ClutchKemResponse VSF messages (~31KB with ciphertexts)
- ✅ Contact matching by handle_hash (PRIVATE identity)
- ✅ PT (Photon Transport) for reliable large transfers
- ✅ TCP fallback for CLUTCH messages
- ✅ Rolling chain encryption (text messages)
- ✅ L1 memory-hard scratch
- ✅ P2P UDP communication
- ✅ One-frame UI load
- ✅ CLUTCH state persistence (Complete state survives app restart)
- ✅ Keygen race condition guard (prevents parallel keypair generation)

**Needs Migration (wire format uses 2-party lower/higher, should use N-party handle_hashes vector):**
- 🔄 Wire format: lower/higher fields → handle_hashes: Vec<[u8; 32]>
- 🔄 StatusUpdate enums: from_handle_hash/to_handle_hash → handle_hashes vector
- 🔄 ceremony_id derivation uses sorted handle_hashes internally, but wire format doesn't match

**In Development:**
- 🚧 Friendship-based ceremony chains (friendship.rs scaffolded)
- 🚧 FGTW call forwarding (optional, requires consent)

**Future:**
- ⏳ ferros OS integration
- ⏳ Hardware encoder optimization (mobile)
- ⏳ Social key recovery for handles

---

## 11. License

MIT OR Apache-2.0 (dual licensed)

---

## Appendix A: Zero-Index Philosophy

This specification uses zero-indexing throughout:
- Sections: 0-11
- Phases: 0-N
- Array indices: 0-N
- Primitives in bundle: 0-7

**Rationale:** Aligns with hardware reality, Rust conventions, and mathematical foundations. Humans count from 1, computers count from 0. This is a computer protocol.

---

**End of Specification**