# CHAIN Protocol Specification v0.0

**Protocol:** Rolling Chain Encryption (Post-CLUTCH Communication)
**Author:** Nick Spiker
**Status:** Draft
**License:** MIT OR Apache-2.0
**Date:** December 2025
**Dependency:** Requires completed CLUTCH ceremony (see CLUTCH.md)

---

## 0. Abstract

CHAIN is the rolling encryption protocol used for all communication after a CLUTCH ceremony completes. It transforms the CLUTCH eggs into an evolving chain state that provides forward secrecy, self-authentication, and memory-hard advancement.

CHAIN is not a separate handshake—successful decryption *is* authentication. Both parties derive identical chain states deterministically, and the chain advances with every ACKed message. Compromise of current state reveals nothing about past messages.

---

## 1. Design Philosophy

### 1.0 Self-Authenticating Messages

No signatures, no certificates, no identity proofs. If a message decrypts successfully and the smear proof matches, the sender must possess the chain state. This proves continuous participation since the CLUTCH ceremony. The chain itself IS the credential.

### 1.1 Forward Secrecy by Default

Every ACKed message advances the chain state thru memory-hard mixing. Even if current state is compromised, past messages remain protected—the attacker cannot reverse the chain advancement.

### 1.2 Symmetric Efficiency

After the asymmetric CLUTCH ceremony, all subsequent encryption is symmetric. Message encryption is fast, limited primarily by memory bandwidth for scratch generation.

### 1.3 Defense in Depth

Multiple independent security layers:
- ChaCha20 stream cipher (proven, fast)
- XOR with memory-hard scratch pad
- Smear hash authentication (BLAKE3 ⊕ SHA3 ⊕ SHA512)
- Device key encryption at rest

### 1.4 Eagle Time Ordering

No sequence numbers. Messages carry Eagle time (f6) which provides:
- Temporal ordering
- Implicit uniqueness (nanosecond precision)
- Consistency check with outer VSF header

---

## 2. Chain State Structure

### 2.0 Extended Chain (512 Links)

Each participant maintains a 512-link chain (16KB):
- **Links 0-255:** History window (initialized to zeros, fills as chain advances)
- **Links 256-511:** Active chain (current encryption keys, derived from CLUTCH)

This layout is natural from truncate-and-append derivation: append-right fills [256..512],
and on each advance we shift-left, dropping oldest history at [0] and adding new link at [511].

```rust
struct ParticipantChain {
    /// 512 links × 32 bytes = 16KB
    /// [0..256) = history (zeros initially, fills on advance)
    /// [256..512) = active (derived, current key at [511])
    links: [[u8; 32]; 512],

    /// Last ACKed Eagle time for this participant
    /// Included in fresh_link derivation - chains messages temporally
    /// If parties disagree on prev_time, chains diverge immediately
    last_ack_time: EagleTime,
}

struct FriendshipChains {
    /// Friendship identifier
    friendship_id: FriendshipId,

    /// One extended chain per participant (sorted by handle_hash)
    chains: Vec<ParticipantChain>,

    /// Participant handle_hashes (sorted)
    participants: Vec<[u8; 32]>,
}
```

### 2.1 Chain Initialization

From CLUTCH eggs via avalanche expansion:

```
CLUTCH eggs (640B)
    ↓
avalanche_expand_eggs() → 2MB mixed buffer
    ↓
For each participant (sorted):
    derive_chain_from_avalanche(avalanche, handle_hash) → 8KB
    ↓
    Place in links[256..512], set links[0..256] = zeros
```

The history window starts empty (zeros). It fills as messages are ACKed and the chain
advances - each advance shifts everything left, new link appends at [511], and the
oldest active link (now at [255]) becomes the newest history entry.

### 2.2 State Synchronization

Both parties maintain identical chain states. Deterministic:
- Same CLUTCH eggs → same initial chains
- Same ACKed messages → same current state

### 2.3 Why Full 512 Links for All Chains?

Each participant stores full 512 links for ALL chains (their own + others'), even though:
- Your own chain: only need active portion (256 links) for sending
- Their chains: need history for decrypting retries

**Reasons for uniform storage:**
1. **Multi-device sync**: Your laptop needs full state of your chain to continue conversations started on your phone
2. **Cross-device message display**: All your devices need to decrypt/display your own sent messages
3. **Device sync protocol**: Similar chain-based sync between your own devices
4. **Simplicity**: One data structure, one sync mechanism
5. **Future-proof**: New features may need history of your own chain

Messages stored locally are re-encrypted with device-specific keys. Chain state syncs between your devices via separate device-to-device protocol (see Device Sync spec).

---

## 3. Chained Salt Generation

### 3.0 Salt Chaining via Spaghettify

Each message's salt is derived from the **previous message's plaintext**. This creates
a cryptographic chain that forces message ordering:

```rust
fn derive_salt(prev_plaintext: &[u8], chain: &ParticipantChain) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"PHOTON_SALT_v0");
    hasher.update(prev_plaintext);  // Empty for first message
    hasher.update(chain.links[500..512].as_flattened());  // Last 12 links
    *spaghettify(hasher.finalize().as_bytes()).as_bytes()
}

// Usage:
// S1 = derive_salt(&[], chain)           // First message: empty prev
// S2 = derive_salt(&m1_plaintext, chain) // Second message
// S3 = derive_salt(&m2_plaintext, chain) // Third message
```

### 3.1 Why Chained Salt?

- **Forced ordering**: Can't decrypt M2 without M1's plaintext (need it to derive S2)
- **Implicit gap detection**: Missing message = can't derive next salt = chain breaks
- **ACK efficiency**: ACK for last message confirms entire chain received
- **Deterministic**: No random generation, both parties derive same sequence
- **Memory-hard**: spaghettify prevents precomputation attacks
- **Simple base case**: First message just uses empty prev_plaintext

### 3.2 Message Flow

```
Alice sends M1, M2, M3 without waiting for ACKs:
    S1 = derive_salt(&[], chain)            // First: empty prev
    Send M1 with S1
    S2 = derive_salt(&M1_plaintext, chain)  // S2 from M1
    Send M2 with S2
    S3 = derive_salt(&M2_plaintext, chain)  // S3 from M2
    Send M3 with S3

Bob receives out of order (M3, M1, M2):
    Receive M3 - can't decrypt (don't know S3)
    Receive M1 - decrypt with S1 ✓ → now can derive S2
    Receive M2 - decrypt with S2 ✓ → now can derive S3
    Receive M3 - decrypt with S3 ✓
    ACK(M3) → Alice knows entire chain received
```

### 3.3 Salt NOT in Wire Format

With chained salt, both sides derive it independently:
- Sender: `derive_salt(prev_plaintext, chain)` before encrypting
- Receiver: `derive_salt(prev_plaintext, chain)` after decrypting previous message

**No salt on the wire** - saves 32 bytes per message. The receiver already has prev_plaintext (they just decrypted it), so they can derive the same salt.

---

## 4. L1 Scratch Pad Generation

### 4.0 Memory-Hard Scratch

Background thread continuously precomputes scratch buffers:

```rust
const L1_SIZE: usize = 30_720;  // 30KB - fits in L1 cache
const L1_ROUNDS: usize = 3;      // Sequential rounds

fn generate_scratch(
    chain: &[[u8; 32]; 512],
    salt: &[u8; 32],
) -> Vec<u8> {
    let mut scratch = vec![0u8; L1_SIZE];

    // Initialize from current key (link[511]) XOR salt
    let mut state = [0u8; 32];
    for i in 0..32 {
        state[i] = chain[511][i] ^ salt[i];
    }
    scratch[0..32].copy_from_slice(&state);

    // Fill with sequential hashing
    for i in (32..L1_SIZE).step_by(32) {
        state = smear_hash(&scratch[i-32..i]);
        scratch[i..i+32].copy_from_slice(&state);
    }

    // Data-dependent mixing rounds
    for round in 0..L1_ROUNDS {
        for i in (32..L1_SIZE).step_by(32) {
            // Read position depends on current state
            let read_idx = (u32::from_le_bytes(scratch[i..i+4].try_into().unwrap()) as usize)
                % (i / 32) * 32;

            // Mix with data-dependent read
            let mut mix_input = [0u8; 64];
            mix_input[0..32].copy_from_slice(&scratch[i-32..i]);
            mix_input[32..64].copy_from_slice(&scratch[read_idx..read_idx+32]);

            let mixed = smear_hash(&mix_input);
            scratch[i..i+32].copy_from_slice(&mixed);
        }
    }

    scratch
}
```

### 4.1 Scratch Properties

| Property | Value | Rationale |
|----------|-------|-----------|
| Size | 30KB | Fits in L1 cache |
| Rounds | 3 | ~1-5ms generation |
| Sequential | Yes | Cannot parallelize |
| Data-dependent | Yes | Cache-hostile, ASIC-resistant |
| Deterministic | Yes | Same inputs → same scratch |

### 4.2 Scratch Precomputation

```rust
impl ParticipantChain {
    /// Precompute scratch for next message
    /// prev_plaintext is empty for first message, otherwise previous message content
    fn precompute_scratch(&self, prev_plaintext: &[u8]) -> ScratchPad {
        let salt = derive_salt(prev_plaintext, self);
        let data = generate_scratch(&self.links, &salt);
        ScratchPad { salt, data }
    }
}

struct ScratchPad {
    salt: [u8; 32],  // Derived, not transmitted - for internal verification
    data: Vec<u8>,   // 30KB scratch for XOR layer
}

// Usage:
// First message:  precompute_scratch(&[])
// After sending:  precompute_scratch(&sent_plaintext)
```

**Precomputation timing:**
- After sending M1, immediately start computing scratch for M2 (using M1's plaintext)
- spaghettify (~1s) can overlap with network latency waiting for ACK
- By the time user types next message, scratch is ready

---

## 5. Message Encryption

### 5.0 Encryption Layers

Three-layer inner encryption + standard VSF outer signature:

```
Message (text)
    ↓ Layer 1: Build VSF field (d{message}:x{text},hp{inc_hp},hR{pad})
    ↓ Layer 1b: Shuffle field values (enforces type-marker parsing)
    ↓ Layer 2: Encrypt field.flatten() with ChaCha20
    ↓ Layer 3: XOR with scratch pad
Inner ciphertext (opaque blob)
    ↓ Layer 4: Standard VSF Ed25519 signature (outer integrity)
VSF Document
```

**Plaintext is a VSF field with shuffled values:**
```
(d{message}:x{Hey man},hp{32 bytes...},hR{random padding})
```

Field components (order randomized before encryption):
- `d{message}` - Field name (always first, before colon)
- `x{text}` - UTF-8 user message (Huffman compressed)
- `hp{inc_hp}` - Incorporated hash pointer (32B, bidirectional weave)
- `hR{pad}` - Random padding (0-255 bytes, traffic analysis resistance)

This gives us:
- **Type-marker parsing**: Receiver matches on type prefix, not position
- **Bidirectional weave**: `hp` contains hash of peer's last message we incorporated
- **Traffic analysis resistance**: Random `hR` padding obscures message length
- **VSF-spec compliant**: Standard field syntax `(d{name}:val1,val2,...)`
- **Shuffle enforcement**: Randomized order ensures parsers can't rely on position

**Why shuffle field values?**
- Enforces correct parsing: receiver MUST use type markers (`x`, `hp`, `hR`)
- VSF-spec compliant: field values are order-independent by design
- Defense in depth: even if encryption broken, parser must handle any order

**Standard VSF structure:**
- Header contains: version, Created timestamp, provenance hash, signature, pubkey, labels
- Body contains: `[message v(vP, ciphertext)]` - Photon-wrapped encrypted content
- Any VSF tool can verify the signature given sender's device pubkey
- Body content is opaque `v` wrapped bytes to generic VSF tools

### 5.1 Encryption Process

```rust
fn encrypt_message(
    message_text: &str,
    chain: &ParticipantChain,
    their_last_hp: &[u8; 32],  // Hash of peer's last message we received
    scratch: &ScratchPad,
    eagle_time: EagleTime,
    device_key: &Ed25519Key,
) -> VsfDocument {
    use vsf::schema::section::FieldValue;
    use rand::seq::SliceRandom;

    // Layer 1: Build field values
    let mut values = vec![
        VsfType::x(message_text.to_string()),  // UTF-8 text
        VsfType::hp(their_last_hp.to_vec()),   // Bidirectional weave
    ];

    // Add random padding for traffic analysis resistance
    // Length = min of 3 random u8s → biased toward short (median ~53 bytes)
    let pad_len = rand::random::<u8>()
        .min(rand::random::<u8>())
        .min(rand::random::<u8>()) as usize;
    if pad_len > 0 {
        let random_bytes: Vec<u8> = (0..pad_len).map(|_| rand::random()).collect();
        values.push(VsfType::hR(random_bytes));
    }

    // Layer 1b: Shuffle field order - enforces type-marker parsing
    values.shuffle(&mut rand::thread_rng());

    // Build field: (d{message}:x{...},hp{...},hR{...})
    let field = FieldValue::new("message", values);
    let plaintext = field.flatten();

    // Derive ChaCha20 key from current link (rightmost = newest)
    let chacha_key = blake3::derive_key("photon.chain.chacha.v0", &chain.links[511]);

    // Derive nonce from eagle time (from VSF header)
    let nonce = derive_nonce(eagle_time);

    // Layer 2: ChaCha20 encryption
    let mut cipher = ChaCha20::new(&chacha_key.into(), &nonce.into());
    let mut ciphertext = plaintext.clone();
    cipher.apply_keystream(&mut ciphertext);

    // Layer 3: XOR with scratch pad (cycling if message > scratch)
    for (i, byte) in ciphertext.iter_mut().enumerate() {
        *byte ^= scratch.data[i % scratch.data.len()];
    }

    // Build body section with Photon-wrapped ciphertext
    let body_section = VsfSection::new_labeled("message")
        .with(VsfType::v(b'P', ciphertext));  // v(vP, ciphertext)

    // Layer 4: Create VSF document with standard header
    VsfDocument::new()
        .with_created(eagle_time)                    // Header: Created timestamp
        .with_signature(device_key, &body_section)   // Header: se(sig), ke(pubkey)
        .with_body(body_section)                     // Body: [message v(vP, ...)]
    // Note: salt not included - receiver derives it from prev_plaintext
}
```

**Note:** Outer integrity uses standard VSF Ed25519 signature. Inner integrity comes from chain-bound decryption (only holder of chain state can decrypt).

### 5.2 Decryption Process

```rust
fn decrypt_message(
    doc: &VsfDocument,
    sender_pubkey: &[u8; 32],  // Known from CLUTCH ceremony
    chain: &ParticipantChain,
    prev_plaintext: &[u8],  // Empty for first message, otherwise previous msg
) -> Result<String, ChainError> {
    // Layer 4 (first): Verify VSF signature (outer integrity, fast reject)
    // Signature is in header, covers body section
    if !doc.verify_signature(sender_pubkey) {
        return Err(ChainError::SignatureInvalid);
    }

    // Extract timestamp from VSF header (single source of truth)
    let eagle_time = doc.created();

    // Extract ciphertext from body: [message v(vP, ciphertext)]
    let message_section = doc.body.get_section("message")?;
    let (encoding, ciphertext) = message_section.get::<(u8, Vec<u8>)>()?;  // v type
    if encoding != b'P' {
        return Err(ChainError::InvalidEncoding);
    }

    // Derive salt from previous plaintext (same as sender did)
    let salt = derive_salt(prev_plaintext, chain);

    // Try current state first, then history if needed
    if let Ok(text) = try_decrypt_at_offset(&ciphertext, eagle_time, chain, &salt, 0) {
        return Ok(text);
    }

    // Try history window (shifted by 1..256 positions)
    for offset in 1..=256 {
        if let Ok(text) = try_decrypt_at_offset(&ciphertext, eagle_time, chain, &salt, offset) {
            return Ok(text);
        }
    }

    Err(ChainError::DecryptionFailed)
}

fn try_decrypt_at_offset(
    ciphertext: &[u8],
    eagle_time: EagleTime,
    chain: &ParticipantChain,
    salt: &[u8; 32],
    history_offset: usize,
) -> Result<(String, [u8; 32]), ChainError> {
    // Current key is at [511], history at [511 - offset]
    let key_index = 511 - history_offset;
    if key_index < 256 {
        return Err(ChainError::DecryptionFailed);
    }

    // Regenerate scratch from derived salt + historical key
    let scratch = generate_scratch_with_key(&chain.links[key_index], salt);

    // Layer 3 (reverse): XOR with scratch pad
    let mut intermediate = ciphertext.to_vec();
    for (i, byte) in intermediate.iter_mut().enumerate() {
        *byte ^= scratch[i % scratch.len()];
    }

    // Layer 2 (reverse): ChaCha20 decryption
    let chacha_key = blake3::derive_key("photon.chain.chacha.v0", &chain.links[key_index]);
    let nonce = derive_nonce(eagle_time);
    let mut cipher = ChaCha20::new(&chacha_key.into(), &nonce.into());
    cipher.apply_keystream(&mut intermediate);

    // Layer 1 (reverse): Parse VSF field (d{message}:x{...},hp{...},hR{...})
    // Uses type-marker parsing - values can appear in any order
    let mut ptr = 0usize;
    let mut message_text = String::new();
    let mut incorporated_hp = [0u8; 32];

    // Expect '(' to start field
    if intermediate.get(ptr) != Some(&b'(') {
        return Err(ChainError::DecryptionFailed);
    }
    ptr += 1;

    // Parse field name (d{message})
    match vsf::parse(&intermediate, &mut ptr) {
        Ok(VsfType::d(name)) if name == "message" => {}
        _ => return Err(ChainError::DecryptionFailed),
    }

    // Expect ':' separator
    if intermediate.get(ptr) != Some(&b':') {
        return Err(ChainError::DecryptionFailed);
    }
    ptr += 1;

    // Parse comma-separated values by type marker (not position)
    loop {
        match vsf::parse(&intermediate, &mut ptr) {
            Ok(VsfType::x(s)) => message_text = s,
            Ok(VsfType::hp(hash)) if hash.len() == 32 => {
                incorporated_hp.copy_from_slice(&hash);
            }
            Ok(VsfType::hR(_)) => {} // Random padding - ignore
            _ => break,
        }

        // Check for ',' (more values) or ')' (end of field)
        match intermediate.get(ptr) {
            Some(b',') => ptr += 1,
            Some(b')') => break,
            _ => break,
        }
    }

    if message_text.is_empty() {
        return Err(ChainError::DecryptionFailed);
    }

    Ok((message_text, incorporated_hp))
}
```

**Verification order:**
1. **Signature first** (fast reject) - O(1), no chain state needed
2. **Layers 3→2→1** - only if signature valid

---

## 6. Wire Format (VSF)

### 6.0 Encrypted Message Document

Standard VSF document - timestamp lives in header, not duplicated in body:

```
VSF Document: Encrypted Message
┌────────────────────────────────────────────────────────────┐
│ HEADER (standard VSF):                                     │
│   Version 5                                                │
│   Created e(eagle_time)     ← timestamp HERE ONLY          │
│   hp(provenance_hash)       - 32B BLAKE3 of body           │
│   se(signature)             - 64B Ed25519 signature        │
│   ke(pubkey)                - 32B sender device pubkey     │
│   1 labels: (message @offset N bytes 1 field)              │
├────────────────────────────────────────────────────────────┤
│ BODY SECTION [message]:                                    │
│   v(vP, ciphertext)         - Photon-wrapped encrypted     │
└────────────────────────────────────────────────────────────┘

Signature covers: BLAKE3(body.flatten())
```

**The `v(vP, ciphertext)` wrapper:**
- `v` = VSF wrapped data type
- `vP` = Photon encoding byte (application-specific)
- `ciphertext` = encrypted VSF field (plaintext below)

**Plaintext field (encrypted inside v):**
```
┌────────────────────────────────────────────────────────────┐
│ (d{message}:                - Field start with name        │
│   x{text},                  - UTF-8 user message (Huffman) │
│   hp{inc_hp},               - 32B incorporated hash        │
│   hR{pad}                   - 0-255B random padding        │
│ )                           - Field end                    │
└────────────────────────────────────────────────────────────┘
Note: Field values are shuffled before encryption.
      Receiver parses by type marker, not position.
```

**Wire overhead:**
- Field overhead: ~12 bytes (parens, d{message}:, commas)
- x header: ~3 bytes
- hp: 33 bytes (1 type + 32 hash)
- hR: 0-256 bytes (median ~53, biased short via min of 3 random u8s)
- Body section: ~4 (v header)
- Header: ~132 bytes (version, timestamp, hash, sig, pubkey, labels)
- Total: message + ~185 bytes + padding (no salt - derived)

**Why this structure?**
- **Single timestamp**: VSF header `Created` field, not duplicated
- **Standard VSF**: Any VSF tool can verify signature, inspect header
- **Clean wrapping**: `v(vP, ...)` marks Photon-specific content
- **Type-marker parsing**: Shuffle enforces correct VSF-spec parsing
- **Bidirectional weave**: `hp` links to peer's last message
- **Traffic analysis resistance**: Random `hR` padding obscures length

### 6.1 ACK Message Section

```
VSF Section: Message ACK
┌────────────────────────────────────────────────────────────┐
│ e(eagle_time)        - f6, which message we're ACKing      │
│ hb(ack_proof)        - 32B, domain-separated decrypt proof │
└────────────────────────────────────────────────────────────┘
```

**ACK Proof Generation (fast, domain-separated):**

```rust
fn generate_ack_proof(
    eagle_time: EagleTime,
    plaintext_hash: &[u8; 32],
    chain: &ParticipantChain,
) -> [u8; 32] {
    // Different structure than fresh_link:
    // - Different domain prefix
    // - Only last 5 links (not full active chain)
    // - Different field order
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"PHOTON_ACK_v0");           // Different domain
    hasher.update(plaintext_hash);              // Hash first (opposite order)
    hasher.update(&eagle_time.to_bytes());
    hasher.update(chain.links[507..512].as_flattened());  // Only last 5 links (160B)

    smear_hash(hasher.finalize().as_bytes())  // Fast, not memory-hard
}
```

**ACK is fast because:**
- smear_hash (~microseconds) not handle_proof (~1 second)
- Only 5 links, not 256 (160B vs 8KB)
- Message flow stays snappy

**Still secure because:**
- Domain separation: `PHOTON_ACK_v0` ≠ `PHOTON_ADVANCE_v0`
- Field order reversed (plaintext_hash before eagle_time)
- Different link range (last 5 vs full active)
- Attacker can't use ACK as fresh_link or vice versa

### 6.2 Full Message Envelope

```
VSF Document
┌────────────────────────────────────────────────────────────┐
│ HEADER (standard VSF):                                     │
│   Version 5                                                │
│   Created e(eagle_time)     ← single timestamp             │
│   hp(provenance_hash)       - 32B BLAKE3 of body           │
│   se(signature)             - 64B Ed25519 signature        │
│   ke(pubkey)                - 32B sender device pubkey     │
│   Labels: (message @...), (routing @...), (acks @...)      │
├────────────────────────────────────────────────────────────┤
│ BODY SECTIONS:                                             │
│   [message v(vP, ciphertext)]     - encrypted content      │
│   [routing                                                 │
│     hb(sender_handle_hash),       - 32B                    │
│     hb(friendship_id),            - 32B                    │
│     hp(prev_msg_hp)               - 32B, hash chain link   │
│   ]                                                        │
│   [acks                           - bundled ACKs (optional)│
│     (e, hb), (e, hb), ...         - time + proof pairs     │
│   ]                                                        │
└────────────────────────────────────────────────────────────┘
```

**Routing overhead:** 96 bytes constant (no sequence numbers, no growth)

**The `hp(prev_msg_hp)` field:**
- Links messages in a hash chain for ordering
- First message uses deterministic anchor (see Section 6.3)
- Subsequent messages reference previous message's `hp` (from header)
- Enables gap detection: unknown `prev_msg_hp` → request missing message

### 6.3 Message Ordering

#### 6.3.1 First Message Anchor

The first message in a conversation uses a deterministic anchor derived from the friendship ID:

```rust
const DOMAIN_FIRST_MSG: &[u8] = b"PHOTON_FIRST_MSG_v0";

fn first_message_anchor(friendship_id: &FriendshipId) -> [u8; 32] {
    *blake3::keyed_hash(
        blake3::hash(DOMAIN_FIRST_MSG).as_bytes(),
        friendship_id.as_bytes()
    ).as_bytes()
}
```

**Usage:**
- First message: `prev_msg_hp = first_message_anchor(friendship_id)`
- Subsequent: `prev_msg_hp = hp` from previous message's header

#### 6.3.2 Hash Domain Separation

| Hash | Computed Over | Purpose |
|------|---------------|---------|
| `hp` (header) | Encrypted body | Wire identifier, VSF standard |
| `plaintext_hash` | Decrypted content | Chain advancement, ACK verification |
| `network_id` | `spaghettify(BLAKE3(plaintext))` | Storage/request identifier |

**Key insight:** `hp` is over encrypted body (what's on wire before decryption), `plaintext_hash` is over decrypted content. They are naturally domain-separated by their inputs.

#### 6.3.3 Gap Detection Flow

```
Receive msg4:
  1. Check routing.prev_msg_hp → points to unknown hash
  2. Don't have that message → request it
  3. Request by network_id (spaghettified plaintext hash)
  4. Receive msg3, decrypt with history links
  5. Now can verify msg4's chain and process it
```

**History links enable recovery:** Even if your chain has advanced, the 256 history links let you decrypt late-arriving messages (up to 256 messages behind).

#### 6.3.4 Message Request Format

```
VSF Section: Message Request
┌────────────────────────────────────────────────────────────┐
│ hG(network_id)         - 32B spaghettified identifier      │
│ hb(friendship_id)      - 32B which conversation            │
└────────────────────────────────────────────────────────────┘
```

**Response:** Original chain-encrypted blob. Requester uses history links to decrypt.

**Why `hG` (spaghettify) for network_id?**
- Privacy: Observer can't correlate with raw `hp` or `plaintext_hash`
- Defense-in-depth: If BLAKE3 broken, spaghettify layer still protects
- Deterministic: Same plaintext → same network_id on all devices

---

## 7. Chain Advancement

### 7.0 Advancement Trigger

Chain advances ONLY on ACK confirmation:

```
Alice sends message M1 (eagle_time T1)
    Alice: encrypt with chain state S0
    Alice: store M1 locally, mark pending

Bob receives M1
    Bob: decrypt with chain state S0 ✓
    Bob: send ACK(T1, ack_smear)
    Bob: advance chain S0 → S1

Alice receives ACK
    Alice: verify ack_smear
    Alice: advance chain S0 → S1
    Alice: mark M1 as delivered
```

### 7.1 Advancement Algorithm

```rust
impl ParticipantChain {
    fn advance(&mut self, eagle_time: EagleTime, plaintext_hash: &[u8; 32]) {
        // Single left-shift: everything moves left, oldest history drops off [0]
        // Old [256] (oldest active) becomes [255] (newest history)
        self.links.copy_within(1..512, 0);

        // Derive fresh link via spaghettify
        let fresh_link = derive_fresh_link(eagle_time, plaintext_hash, &self.links);

        // Append at rightmost position
        self.links[511] = fresh_link;

        // Update last ack time
        self.last_ack_time = eagle_time;
    }
}

fn derive_fresh_link(
    eagle_time: EagleTime,
    plaintext_hash: &[u8; 32],
    chain: &[[u8; 32]; 512],
) -> [u8; 32] {
    // Domain-separated from ACK proof!
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"PHOTON_ADVANCE_v0");  // Different domain than ACK_PROOF
    hasher.update(&eagle_time.to_bytes());
    hasher.update(chain[256..511].as_flattened()); // Active chain (post-shift)
    hasher.update(plaintext_hash);

    // Spaghettify: computationally chaotic Rube Goldberg mixing
    *spaghettify(&hasher.finalize().as_bytes()).as_bytes()
}
```

**Why spaghettify for fresh_link?**
- Computationally chaotic: data-dependent ops, IEEE754 weirdness, path explosion
- NOT memory-hard (~1.7KB state) - fast enough for per-message use
- Multi-algorithm: uses smear_hash internally (BLAKE3 ⊕ SHA3 ⊕ SHA512)
- Domain separation: `PHOTON_ADVANCE_v0` ≠ `PHOTON_ACK_v0`

**Note:** Memory-hard operations are `handle_proof` (~25MB) and `avalanche_expand_eggs` (2MB).
Spaghettify is for chaos/mixing, not memory-hardness.

### 7.2 State Machine

```
┌─────────────────┐
│  Chain State S  │
└────────┬────────┘
         │
    Send Message
         │
         ▼
┌─────────────────┐
│ Pending (S, M)  │◄────────┐
└────────┬────────┘         │
         │                  │
    Receive ACK             │ Timeout/Retry
         │                  │
         ▼                  │
┌─────────────────┐         │
│ Verify ACK      │─────────┘
│ smear matches?  │    No
└────────┬────────┘
         │ Yes
         ▼
┌─────────────────┐
│ Advance → S'    │
│ Mark delivered  │
└─────────────────┘
```

---

## 8. Message Storage

### 8.0 Security Model

- **Assume:** Process isolation (OS enforced)
- **Assume:** No disk encryption (user may not enable)
- **Therefore:** Encrypt all messages at rest with device key

### 8.1 Device Key

```rust
struct DeviceKey {
    /// ChaCha20-Poly1305 key derived from device-specific entropy
    key: [u8; 32],

    /// Key derivation: platform-specific secure storage
    /// - Linux: libsecret / kernel keyring
    /// - macOS: Keychain
    /// - Windows: DPAPI
    /// - Android: Keystore
}

impl DeviceKey {
    fn encrypt(&self, plaintext: &[u8]) -> Vec<u8> {
        // Random nonce + ChaCha20-Poly1305 AEAD
        let nonce: [u8; 12] = rand::random();
        let cipher = ChaCha20Poly1305::new(&self.key.into());
        let ciphertext = cipher.encrypt(&nonce.into(), plaintext).unwrap();

        [nonce.as_slice(), &ciphertext].concat()
    }

    fn decrypt(&self, encrypted: &[u8]) -> Result<Vec<u8>, Error> {
        let (nonce, ciphertext) = encrypted.split_at(12);
        let cipher = ChaCha20Poly1305::new(&self.key.into());
        cipher.decrypt(nonce.into(), ciphertext)
    }
}
```

### 8.2 Message File Format

Each message stored as separate file, identified by **network_id** (spaghettified plaintext hash):

```
Path: ~/.photon/friendships/{friendship_id_base64}/messages/{network_id_base64}.vsf

File contents (device-key encrypted):
┌────────────────────────────────────────────────────────────┐
│ VSF Document                                               │
│   direction: u3(0=sent, 1=received)                        │
│   status: u3(pending/sent/delivered/read)                  │
│   eagle_time: e(...)                                       │
│   plaintext: t_u3(...)     ← original message content      │
│   wire_format: t_u3(...)   ← encrypted wire bytes (for re-send) │
│   prev_msg_hp: hb(...)     ← for chain reconstruction       │
└────────────────────────────────────────────────────────────┘
```

**Network ID derivation:**

```rust
const DOMAIN_NETWORK_ID: &[u8] = b"PHOTON_NETWORK_ID_v0";

/// Derive network identifier from plaintext.
/// Deterministic, device-independent, privacy-preserving.
fn derive_network_id(plaintext: &[u8]) -> [u8; 32] {
    let hp_plaintext = blake3::hash(plaintext);
    let mut hasher = blake3::Hasher::new();
    hasher.update(DOMAIN_NETWORK_ID);
    hasher.update(hp_plaintext.as_bytes());
    spaghettify(hasher.finalize().as_bytes())
}
```

**Why network_id instead of eagle_time for filename?**
- **Deterministic across devices**: Same plaintext → same filename everywhere
- **Content-addressable**: Can request by network_id without knowing timestamp
- **Privacy**: Spaghettified, can't reverse to plaintext hash
- **Sync-friendly**: Devices can compare network_ids to find missing messages

### 8.3 Benefits of Per-Message Files

1. **Easy enumeration:** List directory for message history
2. **Atomic writes:** Rename-based atomic save
3. **Selective sync:** Transfer specific messages
4. **Easy cleanup:** Delete old files
5. **Recovery-friendly:** Re-encrypt individual messages for new device

---

### 10.1 Pending Message Tracking

Sender stores pending messages for retry and ACK matching:

```rust
struct PendingMessage {
    eagle_time: EagleTime,
    plaintext: String,        // x type - need for next salt derivation
    plaintext_hash: [u8; 32], // For ACK verification and advancement
    wire_bytes: Vec<u8>,      // For retry
}

// Sender tracks all unACKed messages in send order
pending_messages: Vec<PendingMessage>
```

### 10.2 ACK Confirms Chain

When ACK arrives for message N, it confirms messages 1..N all received:

```
Alice receives ACK(T3):
    Verify ACK proof using T3, hash3, chain

    // Process all pending up to and including T3
    for msg in pending_messages where T <= T3:
        Advance chain with (msg.eagle_time, msg.plaintext_hash)
        Mark delivered
        Remove from pending
```

**Why this works:**
- Bob couldn't decrypt M3 without M1 and M2 plaintexts
- Bob couldn't generate valid ACK(T3) without successfully decrypting M3
- Therefore: ACK(T3) proves M1, M2, M3 all decrypted correctly

### 10.3 Receiver Processing

Bob queues until he can decrypt in order:

```rust
struct ReceivedMessage {
    eagle_time: EagleTime,
    encrypted: EncryptedMessage,
    decrypted: Option<Vec<u8>>,  // None until we can decrypt
}

// Queue of received messages awaiting decryption
received_queue: BTreeMap<EagleTime, ReceivedMessage>

fn process_received(msg: EncryptedMessage, chain: &ParticipantChain) {
    received_queue.insert(msg.eagle_time, ReceivedMessage {
        eagle_time: msg.eagle_time,
        encrypted: msg,
        decrypted: None,
    });

    // Try to decrypt in order
    try_decrypt_chain(chain);
}

fn try_decrypt_chain(chain: &mut ParticipantChain) {
    // Track previous plaintext for salt derivation
    // Empty for first message, then each decrypted message feeds the next
    let mut prev_plaintext: Vec<u8> = last_decrypted_plaintext.unwrap_or_default();

    for msg in received_queue.values_mut() {
        if msg.decrypted.is_some() {
            // Already decrypted, use as prev for next salt
            prev_plaintext = msg.decrypted.clone().unwrap();
            continue;
        }

        // Derive salt from previous plaintext (empty = first message)
        let salt = derive_salt(&prev_plaintext, chain);

        // Try decrypt with derived salt
        if let Ok(plaintext) = decrypt_with_salt(&msg.encrypted, chain, &salt) {
            msg.decrypted = Some(plaintext.clone());

            // Send ACK now that we verified decrypt
            send_ack(msg.eagle_time, hash(&plaintext), chain);

            // This plaintext becomes prev for next message
            prev_plaintext = plaintext;

            // Advance chain
            chain.advance(msg.eagle_time, &hash(&prev_plaintext));
        } else {
            // Can't decrypt - waiting for earlier message
            break;
        }
    }
}
```

### 10.4 Retry with History

If ACK was lost, sender retries. Receiver uses history window:

```
Alice sends M1, times out waiting for ACK
Alice retries M1 (same eagle_time, same wire bytes)

Bob already processed M1, advanced chain
Bob receives M1 retry:
    Decrypt with current salt fails (chain advanced)
    Try history: regenerate old salt from stored state
    Decrypt with history succeeds
    Re-send ACK(T1) (deterministic - same proof)
```

History window (256 links) allows decrypting messages up to 256 advances ago.

### 10.5 Gap Recovery

If M1 is permanently lost (network partition, etc.):

```
Alice sent M1, M2, M3
M1 lost forever
Bob has M2, M3 but can't decrypt (no S2 without M1)

Recovery options:
1. Alice re-sends M1 (if she still has pending state)
2. Out-of-band: Alice tells Bob plaintext of M1
3. Nuclear: New CLUTCH ceremony (fresh chains)
```

The chained salt design means: **no gaps allowed**. This is intentional - it prevents selective message suppression attacks and ensures conversation integrity.

---

## 11. Security Properties

### 11.0 Forward Secrecy

| Scenario | Past Messages | Future Messages |
|----------|---------------|-----------------|
| Current chain compromised | Protected | Compromised until re-CLUTCH |
| Device key compromised | Protected (chain-encrypted) | At risk on that device |
| Both compromised | At risk | At risk |

### 11.1 Authentication

| Property | Mechanism |
|----------|-----------|
| Outer integrity | Standard VSF Ed25519 signature (device-bound) |
| Inner integrity | confirmation_smear (chain-bound, encrypted) |
| Sender identity | Chain state + device key |
| Replay protection | Eagle time uniqueness |
| Ordering | Eagle time comparison |

### 11.2 Defense Layers

```
Layer 0: CLUTCH ceremony (20 eggs from 8 algorithms)
    ↓
Layer 1: Avalanche expansion (2MB memory-hard)
    ↓
Layer 2: Truncate-and-append chain derivation (smear_hash)
    ↓
Layer 3: L1 scratch pad (memory-hard, data-dependent)
    ↓
Layer 4: ChaCha20 stream cipher
    ↓
Layer 5: XOR with scratch pad
    ↓
Layer 6: Inner smear confirmation (BLAKE3 ⊕ SHA3 ⊕ SHA512)
    ↓
Layer 7: Standard VSF Ed25519 signature (outer integrity)
    ↓
Layer 8: Domain-separated ACK proof (fast smear)
    ↓
Layer 9: spaghettify chain advancement (memory-hard, async)
    ↓
Layer 10: Device key encryption at rest
```

Memory-hard operations (Layers 1, 3, 8) happen during setup or async after ACK.
Fast path (Layers 4-7) keeps message tx/rx snappy.

---

## 12. Implementation Checklist

### 12.0 Core Types

- [ ] `ParticipantChain` with 512 links ([0..256]=history, [256..512]=active)
- [ ] `ScratchPad` with chained salt
- [ ] `EncryptedMessage` VSF format
- [ ] `MessageAck` with fast smear proof
- [ ] `PendingMessage` for ACK matching (stores plaintext for salt chain)
- [ ] `ReceivedMessage` queue for out-of-order handling

### 12.1 Salt Chaining

- [ ] `derive_salt(prev_plaintext, chain)` - unified (empty = first message)
- [ ] Salt precomputation after send (overlaps with ACK wait)

### 12.2 Encryption

- [ ] `generate_scratch()` - L1 memory-hard from link[511] + salt
- [ ] `generate_confirmation_smear()` - inner integrity (last 3 links, hM type)
- [ ] `encrypt_message()` - 3-layer inner + VSF signature outer
- [ ] `decrypt_message()` - signature check first, then history fallback
- [ ] Standard VSF Ed25519 signature for outer integrity

### 12.3 Chain Management

- [ ] `advance()` - left-shift + spaghettify new link at [511]
- [ ] `derive_fresh_link()` - spaghettify with domain separation
- [ ] `generate_ack_proof()` - fast smear with domain separation (last 5 links)
- [ ] Receiver queue + ordered decryption via salt chain
- [ ] ACK-confirms-chain logic (ACK for M3 confirms M1, M2, M3)

### 12.4 Storage

- [ ] Device key derivation (per-platform)
- [ ] Per-message file encryption
- [ ] Chain state persistence (16KB per participant)

### 12.5 Recovery

- [ ] Recovery key derivation
- [ ] Export bundle format
- [ ] Import and re-key

---

## Appendix A: Constants

```rust
// Chain structure
const HISTORY_LINKS: usize = 256;   // links[0..256] - zeros initially
const ACTIVE_LINKS: usize = 256;    // links[256..512] - derived from CLUTCH
const TOTAL_LINKS: usize = 512;
const LINK_SIZE: usize = 32;
const CHAIN_SIZE: usize = TOTAL_LINKS * LINK_SIZE;  // 16KB

// Current key position
const CURRENT_KEY_INDEX: usize = 511;  // Rightmost = newest

// Scratch
const L1_SIZE: usize = 30_720;  // 30KB
const L1_ROUNDS: usize = 3;

// Wire format (VSF with standard header)
// Field: (d{message}:x{text},hp{inc_hp},hR{pad})
const FIELD_OVERHEAD: usize = 12;       // parens, d{message}:, commas
const X_HEADER: usize = 3;              // x header bytes
const HP_SIZE: usize = 33;              // hp type (1) + hash (32)
const HR_MEDIAN: usize = 53;            // hR padding median (min of 3 random u8s)
const BODY_OVERHEAD: usize = 4;         // v(vP, ...) header
const HEADER_SIZE: usize = 132;         // version, timestamp, hash, sig, pubkey, labels
const MSG_OVERHEAD: usize = 185;        // field + body + header (no salt - derived)
// Note: Add HR_MEDIAN (~53) for typical total overhead

// Domain separation strings
const DOMAIN_ADVANCE: &[u8] = b"PHOTON_ADVANCE_v0";    // Chain advancement (spaghettify)
const DOMAIN_ACK: &[u8] = b"PHOTON_ACK_v0";            // ACK proof (fast smear)
const DOMAIN_SALT: &[u8] = b"PHOTON_SALT_v0";          // Salt derivation (empty prev = first msg)
const DOMAIN_FIRST_MSG: &[u8] = b"PHOTON_FIRST_MSG_v0"; // First message anchor
const DOMAIN_NETWORK_ID: &[u8] = b"PHOTON_NETWORK_ID_v0"; // Storage/request identifier
// Note: Outer integrity uses standard VSF Ed25519 signature (no custom domain)
// Note: DOMAIN_CONFIRM removed - inner integrity via chain-bound decryption

// Each domain uses different link ranges for additional separation
const ACK_LINK_RANGE: Range<usize> = 507..512;      // 5 links (160B)
const SALT_LINK_RANGE: Range<usize> = 500..512;     // 12 links for salt derivation
// Note: Outer integrity is standard VSF header signature (device key, no chain links)
```

## Appendix C: VSF Type Extensions

Photon uses application-defined VSF types with **uppercase second character**:

```rust
// Standard VSF types (lowercase = VSF-defined)
d(String),       // Dictionary key (field name)
x(String),       // UTF-8 text (Huffman compressed)
hp(Vec<u8>),     // BLAKE3 provenance hash
hb(Vec<u8>),     // BLAKE3 rolling hash
hs(Vec<u8>),     // SHA hash family
ke(Vec<u8>),     // Ed25519 public key
se(Vec<u8>),     // Ed25519 signature
v(u8, Vec<u8>),  // Wrapped data with encoding byte

// Application-defined hash types (UPPERCASE = app-specific)
hR(Vec<u8>),  // Random padding (traffic analysis resistance)
hD(Vec<u8>),  // Handle proof output (memory-hard ~25MB, ~1s)
hG(Vec<u8>),  // Spaghettify output (chaotic Rube Goldberg, ~1.7KB state)
hM(Vec<u8>),  // Smear hash: BLAKE3 ⊕ SHA3 ⊕ SHA512 (multi-algorithm)

// Application-defined v encoding bytes
v(b'P', ...),  // Photon encrypted message (vP)
               // Body contains: encrypted (d{message}:x{},hp{},hR{}) field
```

**Convention:** `a-z` = reserved for VSF standard types, `A-Z` = application extensions.

**The `v` wrapper type:**
- First byte is the encoding identifier (application-defined meaning)
- `vP` = Photon-encrypted content (decrypt to get VSF field)
- Generic VSF tools see opaque wrapped bytes, Photon tools know to decrypt

**Message field format:**
```
(d{message}:x{Hey man},hp{32 bytes...},hR{random padding})
```
- Field values shuffled before encryption
- Receiver parses by type marker (`x`, `hp`, `hR`), not position
- Shuffle enforces VSF-spec-compliant parsing

## Appendix D: Test Vectors

TODO: Add test vectors for:
- Chain initialization from known eggs
- Salt derivation (empty prev = first message)
- Chained salt derivation from plaintext
- Scratch generation from known chain + salt
- Message encryption/decryption roundtrip
- Field parsing with shuffled order
- VSF signature verification (outer integrity)
- ACK proof generation
- Chain advancement
- History fallback decryption
- Multi-message salt chain (M1 → M2 → M3)
- Out-of-order receive + queue processing
- First message anchor derivation
- Network ID derivation from plaintext

## Appendix E: Storage Identifiers

### E.1 Identifier Hierarchy

Messages have three related identifiers, each serving a different purpose:

```
┌─────────────────────────────────────────────────────────────────┐
│ plaintext (decrypted message content)                           │
│     ↓                                                           │
│ hp_plaintext = BLAKE3(plaintext)                                │
│     • Device-independent                                        │
│     • Used for chain advancement (as plaintext_hash)            │
│     • Input to network_id derivation                            │
│     ↓                                                           │
│ network_id = spaghettify(DOMAIN_NETWORK_ID || hp_plaintext)     │
│     • Privacy-preserving (one-way from hp_plaintext)            │
│     • Used for storage filenames                                │
│     • Used for network message requests                         │
│     • Deterministic: same plaintext → same network_id everywhere│
└─────────────────────────────────────────────────────────────────┘
```

### E.2 hp (Header) vs hp_plaintext

| Property | `hp` (header provenance) | `hp_plaintext` |
|----------|--------------------------|----------------|
| Input | Encrypted body (wire format) | Decrypted plaintext |
| Computed | VSF standard, before decryption | After successful decryption |
| Device-dependent | Yes (different device keys) | No |
| Use case | Wire identifier | Storage/sync identifier |

**Critical distinction:** `hp` changes if you re-encrypt with different keys (e.g., re-encryption for different device). `hp_plaintext` stays the same regardless of encryption.

### E.3 Network ID Properties

```rust
fn derive_network_id(plaintext: &[u8]) -> [u8; 32] {
    let hp_plaintext = blake3::hash(plaintext);
    let mut hasher = blake3::Hasher::new();
    hasher.update(DOMAIN_NETWORK_ID);
    hasher.update(hp_plaintext.as_bytes());
    spaghettify(hasher.finalize().as_bytes())
}
```

**Security properties:**
1. **Preimage resistant**: Can't reverse spaghettify to get hp_plaintext
2. **Collision resistant**: Different plaintexts → different network_ids
3. **Deterministic**: Same plaintext → same network_id (required for sync)
4. **Domain separated**: `DOMAIN_NETWORK_ID` prevents cross-protocol attacks

### E.4 Storage Path Examples

```
Friendship between Alice and Bob:
  friendship_id = BLAKE3("PHOTON_FRIENDSHIP_v1" || sorted_handle_hashes)

Storage root: ~/.photon/friendships/{friendship_id_base64}/

Messages:
  messages/abc123...xyz.vsf  ← network_id of message 1
  messages/def456...uvw.vsf  ← network_id of message 2
  messages/ghi789...rst.vsf  ← network_id of message 3

Chain state:
  chain.bin  ← 16KB × 2 participants = 32KB

Metadata:
  metadata.vsf  ← friendship info, participants, etc.
```

### E.5 Cross-Device Sync

When syncing between your own devices:

```
Device A has: {net_id_1, net_id_2, net_id_3}
Device B has: {net_id_1, net_id_4}

Sync protocol:
  1. Exchange network_id sets
  2. A requests net_id_4 from B
  3. B requests net_id_2, net_id_3 from A
  4. Exchange device-encrypted blobs
  5. Each device re-encrypts with own device key
```

**Key insight:** network_id is the common identifier that works across devices with different device keys.