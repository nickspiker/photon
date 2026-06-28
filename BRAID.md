# The Braid — Chain Protocol Specification v0.1

**Protocol:** The Braid (Post-CLUTCH Rolling-Chain Encryption)
**Author:** Nick Spiker
**Status:** Draft — reflects the implementation as of the braid landing (commit 9bf1193)
**License:** MIT OR Apache-2.0
**Date:** June 2026 (supersedes CHAIN v0.0, December 2025)
**Dependency:** Requires a completed CLUTCH ceremony (see CLUTCH.md)
**Crypto primitives:** `spaghettify` and `smear_hash` are provided by the `ihi` crate (not defined locally)

---

## 0. Abstract

The braid is the rolling encryption protocol used for all communication after a CLUTCH ceremony completes.
It transforms the CLUTCH eggs into an evolving chain state that provides forward secrecy, self-authentication, and memory-hard advancement.

The braid is not a separate handshake — successful decryption *is* authentication.
Both parties derive identical chain states deterministically, and the chain advances with every message.
Compromise of current state reveals nothing about past messages.

### 0.1 Why "the braid" and not "a ratchet"

A double ratchet only advances forward and weaves depth-1: each step mixes in the immediately-preceding step and nothing older.
The braid reaches BACK into history and cross-weaves the *peer's* strand into our chain — bidirectional cross-entropy.
What makes it categorically a braid, not a ratchet or a single weave: **each step weaves TWO distinct prior peer messages**, chosen from a window of recent history.
Two strands crossing into each new link is a braid; one strand would be a weave; zero is the anchor.

The name is just "the braid". The window size (currently the last 256 messages) is a tunable PARAMETER, not part of the name — don't bake a number in.
"Confluent" describes a PROPERTY (the explicit eagle_time references make any delivery order converge to the same braid state), not part of the name.

---

## 1. Design Philosophy

### 1.0 Self-Authenticating Messages

No signatures, no certificates, no identity proofs at the chain layer.
If a message decrypts successfully, the sender must possess the chain state.
This proves continuous participation since the CLUTCH ceremony. The chain itself IS the credential.
(An outer VSF Ed25519 signature still provides standard transport integrity — see §5.)

### 1.1 Forward Secrecy by Default

Every advance evolves the chain state thru memory-hard mixing and destroys the old link.
Even if current state is compromised, past messages remain protected — the attacker cannot reverse the chain advancement.

### 1.2 Symmetric Efficiency

After the asymmetric CLUTCH ceremony, all subsequent encryption is symmetric.
Message encryption is fast, limited primarily by memory bandwidth for scratch generation.

### 1.3 Defense in Depth

Multiple independent security layers:
- ChaCha20 stream cipher (proven, fast)
- XOR with memory-hard scratch pad
- Smear hash authentication (BLAKE3 ⊕ SHA3 ⊕ SHA512)
- Device key encryption at rest

### 1.4 Explicit, Deterministic Weave

The braid references the messages it weaves EXPLICITLY, by eagle_time, on the wire.
Nothing is inferred from "latest" or from ACK timing.
This is what makes random selection safe: the receiver never guesses which prior secret was mixed — it reads the references and looks them up.
Randomness helps the sender (an attacker with partial state can't predict which prior secret mixes next); the explicit reference makes it free for the receiver.

### 1.5 Eagle Time Ordering and Reference

Messages carry Eagle time (oscillations of the 21cm hydrogen line, 704ps granularity). It serves three roles at once:
- **Temporal ordering** — eagle_time is monotonic (a clock), so time-keyed storage is chronological for free.
- **Provable per-device uniqueness** — a single device physically cannot emit two messages at the same 704ps tick, so within one peer-device's stream the eagle_time is unique, not merely rarely-colliding.
- **Weave reference** — the braid names each woven message by its eagle_time (see §6).

A collision can only come from two devices on the SAME identity (the fleet case) emitting at the same instant — astronomically small, and if it ever happens it's almost certainly deliberate.
So a content-hash tiebreak is an adversarial guard for the contrived multi-device-same-tick case, not a routine collision path.
(That tiebreak is stored — see §8 `content_hash` — but is not yet carried on the wire; see §13 Known Gaps.)

---

## 2. Chain State Structure

### 2.0 Extended Chain (512 Links)

Each participant maintains a 512-link chain (16KB):
- **Links 0-255:** History window (initialized to zeros, fills as the chain advances)
- **Links 256-511:** Active chain (current encryption keys, derived from CLUTCH)

This layout is natural from truncate-and-append derivation: append-right fills [256..512],
and on each advance we shift-left, dropping oldest history at [0] and adding a new link at [511].

```rust
struct Chain {
    /// 512 links × 32 bytes = 16KB
    /// [0..256)   = history (zeros initially, fills on advance)
    /// [256..512) = active (derived, current key at [511])
    links: [[u8; 32]; 512],

    /// Last ACKed Eagle time for this participant. Fed into fresh_link
    /// derivation; if parties disagree on it, chains diverge immediately.
    last_ack_time: Option<EagleTime>,
}

struct FriendshipChains {
    friendship_id: FriendshipId,
    conversation_token: [u8; 32],
    /// One extended chain per participant (sorted by handle_hash)
    chains: Vec<Chain>,
    /// Participant handle_hashes (sorted)
    participants: Vec<[u8; 32]>,
    /// Sent messages awaiting ACK (see §10)
    pending_messages: Vec<PendingMessage>,
    /// Out-of-order arrivals awaiting their predecessor (see §6.3)
    gap_buffer: Vec<BufferedMessage>,
    // ... per-participant hash-chain + bidirectional-entropy bookkeeping
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

### 2.2 State Synchronization

Both parties maintain identical chain states. Deterministic:
- Same CLUTCH eggs → same initial chains
- Same processed messages with the same woven strands → same current state

### 2.3 Why Full 512 Links for All Chains?

Each participant stores full 512 links for ALL chains (their own + others'):
1. **Multi-device sync**: your laptop needs full state of your chain to continue a conversation started on your phone
2. **Cross-device message display**: all your devices need to decrypt/display your own sent messages
3. **Simplicity**: one data structure, one sync mechanism
4. **Future-proof**: new features may need history of your own chain

---

## 3. Chained Salt Generation

### 3.0 Salt Chaining via Spaghettify

Each message's salt is derived from the **previous message's plaintext**, forcing message ordering:

```rust
fn derive_salt(prev_plaintext: &[u8], chain: &Chain) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"PHOTON_SALT_v0");
    hasher.update(prev_plaintext);                       // empty for first message
    hasher.update(chain.links[500..512].as_flattened()); // last 12 links
    *spaghettify(hasher.finalize().as_bytes()).as_bytes()
}
```

### 3.1 Why Chained Salt?

- **Forced ordering**: can't derive S(n) without M(n-1)'s plaintext
- **Implicit gap detection**: missing message → can't derive next salt → chain breaks
- **Deterministic**: both parties derive the same sequence, no random generation
- **Memory-hard**: spaghettify prevents precomputation
- **Simple base case**: first message uses empty `prev_plaintext`

### 3.2 Salt NOT in Wire Format

Both sides derive it independently from `prev_plaintext` + chain links. No salt on the wire — saves 32 bytes per message.

---

## 4. L1 Scratch Pad Generation

### 4.0 Memory-Hard Scratch

```rust
const L1_SIZE: usize = 30_720;  // 30KB — fits in L1 cache
const L1_ROUNDS: usize = 3;     // sequential rounds

fn generate_scratch(chain: &[[u8; 32]; 512], salt: &[u8; 32]) -> Vec<u8> {
    let mut scratch = vec![0u8; L1_SIZE];

    // Initialize from current key (link[511]) XOR salt
    let mut state = [0u8; 32];
    for i in 0..32 { state[i] = chain[511][i] ^ salt[i]; }
    scratch[0..32].copy_from_slice(&state);

    // Fill with sequential hashing
    for i in (32..L1_SIZE).step_by(32) {
        state = smear_hash(&scratch[i-32..i]);
        scratch[i..i+32].copy_from_slice(&state);
    }

    // Data-dependent mixing rounds (cache-hostile, ASIC-resistant)
    for _round in 0..L1_ROUNDS {
        for i in (32..L1_SIZE).step_by(32) {
            let read_idx = (u32::from_le_bytes(scratch[i..i+4].try_into().unwrap()) as usize)
                % (i / 32) * 32;
            let mut mix_input = [0u8; 64];
            mix_input[0..32].copy_from_slice(&scratch[i-32..i]);
            mix_input[32..64].copy_from_slice(&scratch[read_idx..read_idx+32]);
            scratch[i..i+32].copy_from_slice(&smear_hash(&mix_input));
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

---

## 5. Message Encryption

### 5.0 Encryption Layers

```
Message (text)
    ↓ Layer 1:  Build VSF field (message: x{text}, hp{inc_hp}, e6{woven}…, hR{pad})
    ↓ Layer 1b: Shuffle field values (enforces type-marker parsing)
    ↓ Layer 2:  Encrypt field.flatten() with ChaCha20
    ↓ Layer 3:  XOR with scratch pad
Inner ciphertext (opaque blob)
    ↓ Layer 4:  Standard VSF Ed25519 signature (outer integrity)
VSF Document
```

**Plaintext is a VSF field with shuffled values:**
```
(message: x{Hey man}, hp{32 bytes…}, e6{woven_time}…, hR{random padding})
```

Field components (order randomized before encryption):
- `message` — field name (always first, before the colon)
- `x{text}` — UTF-8 user message (Huffman compressed). **This is the braid's weave ingredient** (see §6).
- `hp{inc_hp}` — incorporated hash pointer (32B). Legacy implicit-ACK signal; NO LONGER the weave reference.
- `e6{woven_time}` — **the braid references**: 0, 1, or 2 eagle_times naming the woven peer messages (see §6).
- `hR{pad}` — random padding (0-255 bytes, traffic-analysis resistance).

**Why shuffle field values?** Enforces type-marker parsing (receiver matches `x`/`hp`/`e6`/`hR`, not position); VSF field values are order-independent by design; defense in depth.

### 5.1 Encryption Process

```rust
// Layer 1: build field values
let mut values = vec![
    VsfType::x(message_text.to_string()), // UTF-8 text (= weave ingredient)
    VsfType::hp(incorporated_hp.to_vec()), // legacy implicit-ACK
];
// The braid: name each woven peer message by its eagle_time. 0, 1, or 2.
for &t in &woven_times {
    values.push(VsfType::e(EtType::e6(t)));
}
// Random pad: length = min of 3 random u8s → biased short (median ~53B)
let pad_len = rand::random::<u8>().min(rand::random()).min(rand::random()) as usize;
if pad_len > 0 { values.push(VsfType::hR((0..pad_len).map(|_| rand::random()).collect())); }

// Layer 1b: shuffle (enforces type-marker parsing)
values.shuffle(&mut rand::thread_rng());
let plaintext = FieldValue::new("message", values).flatten();

// Layer 2: ChaCha20 (key from current link [511]), nonce from eagle_time
// Layer 3: XOR with scratch pad
// Layer 4: standard VSF Ed25519 signature over the body section
```

Outer integrity uses the standard VSF Ed25519 signature.
Inner integrity comes from chain-bound decryption (only a holder of the chain state can decrypt).

---

## 6. The Braid: Weave Selection and Derivation

### 6.0 Ingredient vs. Reference

Settled model: **the eagle_time on the wire is the POINTER naming which message is woven; the literal x-text of that message is the INGREDIENT mixed into the chain.**

- The plaintext is NEVER sent — both sides already hold the woven message (one authored it, the other received-and-ACKed it). The eagle_time carries no key entropy; it only lets both sides agree WHICH text to feed.
- The ingredient is **just the x-text** (`content`), NOT the full flattened payload. The random pad is traffic-analysis padding, never key material; `hp` is public. Dropping them loses nothing cryptographically AND makes the ingredient recoverable from storage: the message DB already holds `content` keyed by eagle_time (see §8), so `eagle_time → row → content` resolves a woven message with zero new storage. The message DB *is* the weave window.

### 6.1 Selecting the strands (sender side)

When sending a message, choose up to TWO distinct prior PEER messages to weave:

- Eligible set = **incoming messages** (`is_outgoing == false`) in the last ≤256 of this conversation. Any stored incoming row was already ACKed by the receive path, so the sender knows the peer holds it → both-held → identical strands → lockstep.
- Degenerate ramp:
  - **0 eligible** (brand-new conversation) → weave nothing (the anchor case).
  - **exactly 1** → weave that one (single strand; the braid can't form yet).
  - **≥2** → pick TWO DISTINCT messages.
- Pick with a CSPRNG `gen_range(0..window)` (bounded, bias-free). For the second pick use `gen_range(0..window-1)` and skip the first index, so the two are distinct and uniform. **Never `random % 256`** — that indexes past a small window and reintroduces modulo bias.
- Sort the chosen strands by eagle_time. Put each chosen message's eagle_time on the wire as an `e6` value; freeze the (sorted) strand bytes on the `PendingMessage` so the later ACK-advance uses the exact same bytes (see §10).

### 6.2 Resolving the strands (receiver side)

Read the `e6` woven references from the decrypted field. The peer wove messages IT received — i.e. messages WE authored — so resolve each eagle_time against OUR **outgoing** rows (`is_outgoing == true`).
Both sides hold identical `content` for any such message, so the resolved strands are byte-identical.
Sort by eagle_time (matching the sender) and feed them to `advance`.

A single device cannot emit two messages at the same 704ps tick, so the eagle_time uniquely identifies one of our outgoing messages.
The adversarial same-tick collision (two fleet devices) is not yet disambiguated on the wire — see §13.

### 6.3 Strict in-order processing and the gap buffer

The receiver decrypts at `CURRENT_KEY_INDEX` (link [511]), which is only the correct decrypt position when the message is the immediate successor of the last one processed.
So hash-chain verification is HARD:

```
Receive a message:
  1. is_duplicate(eagle_time)?        → skip (UDP dup / retransmit)
  2. verify_chain_link(prev_msg_hp):
       Ok    → it is contiguous; decrypt at [511], advance, update last_received_hash
       Err   → it is AHEAD of us (predecessor unseen). Buffer it on the prev_msg_hp
               it awaits, and SKIP decrypt. Do NOT soft-decrypt at the wrong link.
  3. After a successful advance, the new msg_hp becomes last_received_hash, so any
     buffered message waiting on THIS as its predecessor is now contiguous:
     drain take_buffered_for(msg_hp) and REPLAY them (front of the work queue),
     in order — each can cascade to fill the next gap.
```

The gap buffer holds ciphertext + the awaited `prev_msg_hp` + sender info, keyed purely on `prev_msg_hp` (the message's own `msg_hp` is unknown before decrypt). Buffered entries are deduped on (sender, eagle_time). The buffer is transient — the existing retransmit path re-sends anything that is lost rather than buffered. No schema change.

```rust
struct BufferedMessage {
    prev_msg_hp: [u8; 32],        // the predecessor this message waits on
    sender_handle_hash: [u8; 32],
    eagle_time: i64,              // oscillations
    ciphertext: Vec<u8>,
    sender_addr: SocketAddr,      // so the replay path ACKs exactly like the live path
}
```

This replaces the older "request the missing message by network_id" recovery: out-of-order arrivals are buffered and replayed locally; only genuinely-lost messages rely on retransmit.

---

## 7. Chain Advancement

### 7.0 Advancement Triggers

Two independent advance points, and they must produce the SAME fresh link for a given message:

- **Receiver:** advances per received message, in strict order (§6.3), immediately after a successful decrypt — NOT gated on ACK timing. Back-to-back messages each advance correctly instead of reusing a stale key.
- **Sender:** advances its own copy of its chain on ACK (`process_ack`), using the woven strands frozen on the `PendingMessage`. Sender-advance-on-ACK is load-bearing for reliability (an ACK proves the receiver advanced its copy) and for the CLUTCH-keypair-zeroize-on-first-ACK forward-secrecy step. Do NOT move it to send time — that would diverge the chains irrecoverably.

### 7.1 Advancement Algorithm

```rust
impl Chain {
    fn advance(&mut self, eagle_time: &EagleTime,
               our_plaintext: &[u8], their_plaintexts: &[&[u8]]) {
        // Single left-shift: oldest history drops off [0]; old [256] becomes [255]
        self.links.copy_within(1..512, 0);
        let fresh = derive_fresh_link(eagle_time, our_plaintext, their_plaintexts, &self.links);
        self.links[511] = fresh;        // append at rightmost
        self.last_ack_time = Some(*eagle_time);
    }
}
```

### 7.2 derive_fresh_link (THE BRAID CORE — both peers must run this identically)

```rust
fn derive_fresh_link(
    eagle_time: &EagleTime,
    our_plaintext: &[u8],
    their_plaintexts: &[&[u8]],   // 0, 1, or 2 strands, SORTED by eagle_time
    chain: &[[u8; 32]; CHAIN_LINKS],
) -> [u8; 32] {
    let chain_portion = chain[HISTORY_LINKS..CURRENT_KEY_INDEX].as_flattened();

    // Layout (peer entropy FIRST, injective framing):
    //   DOMAIN_ADVANCE
    //   eagle_time            (8 bytes, LE)
    //   strand_count          (4 bytes, LE)
    //   [ strand_len(4 LE) + strand_bytes ] * count   ← the woven peer x-texts
    //   chain_portion         (links [256..511], post-shift)
    //   our_len(4 LE) + our_plaintext
    let mut input = Vec::new();
    input.extend_from_slice(DOMAIN_ADVANCE);
    input.extend_from_slice(&eagle_time.oscillations().unwrap_or(0).to_le_bytes());
    input.extend_from_slice(&(their_plaintexts.len() as u32).to_le_bytes());
    for strand in their_plaintexts {
        input.extend_from_slice(&(strand.len() as u32).to_le_bytes());
        input.extend_from_slice(strand);
    }
    input.extend_from_slice(chain_portion);
    input.extend_from_slice(&(our_plaintext.len() as u32).to_le_bytes());
    input.extend_from_slice(our_plaintext);

    spaghettify(&input)   // raw bytes straight in — no pre-hash bottleneck
}
```

**Why peer strands first?** The advance feeds the custom `spaghettify` mixer (not BLAKE3 directly). Leading with the highest-entropy, hardest-to-predict input (the peer's woven plaintext) avalanches the fixed/known portion (our plaintext, domain, chain links) rather than letting predictable bytes set a known prefix — the same principle as salt-before-password in a KDF.

**Why length prefixes?** Injective/canonical framing prevents concatenation collisions (so `"AB"+"C"` and `"A"+"BC"` can't hash identically) and lets 0/1/2 strands be unambiguous. Order is a free choice; unambiguous framing is not.

**Why spaghettify?** Computationally chaotic (data-dependent ops, IEEE754 weirdness, path explosion), multi-algorithm (uses smear_hash internally), NOT memory-hard (~1.7KB state) — fast enough for per-message use. Domain-separated: `PHOTON_ADVANCE_v0` ≠ `PHOTON_ACK_v0`.

> ⚠️ **Compatibility:** changing this layout — order, framing, domain, or which links are included — changes every fresh link and totally desyncs the two chains. Both peers must ship the identical derivation. The braid landing already changed it, so chains predating it are incompatible (dev: nuke + re-key; release: version bump).

---

## 8. Message Storage

### 8.0 Layering

Chain STATE (the ratchet machinery) and conversation CONTENT (messages) live in different stores:

- **Chain state** → per-friendship blob in the flat vault at `vault_key("chains", friendship_id)`.
- **Messages** → rows in the rārangi conversation DB. Each conversation is one byte-keyed table addressed by its `friendship_id` (deterministic from the sorted participant seeds, so the same conversation resolves to the same table on every participant's — and every fleet — device).

### 8.1 Message rows keyed by eagle_time

Each message is one row keyed by its **eagle_time** (`Pk::Int(eagle_time as u64)`), NOT a local counter:

```rust
let content_hash = blake3::hash(msg.content.as_bytes());
let rec = Record::new()
    .set("content",      msg.content.clone())     // the x-text (= weave ingredient)
    .set("timestamp",    Value::Time(msg.timestamp))
    .set("is_outgoing",  msg.is_outgoing as u64)
    .set("delivered",    msg.delivered as u64)
    .set("content_hash", content_hash.as_bytes().to_vec());
db.put_row_in(&table, Pk::Int(msg.timestamp as u64), &rec)?;
```

One value does triple duty:
- **Ordering** — eagle_time is monotonic, and `Pk::Int` encodes big-endian, so key order == chronological. `list_in` over the table is time-sorted for free.
- **Weave reference** — the same eagle_time the braid puts on the wire (§6). `eagle_time → row → content` resolves a woven strand with no extra storage.
- **Identity** — stable and shared across both devices, killing the renumber-on-insert hazard of a local index key.

eagle_time is `i64` but always positive (oscillations since Apollo 11), so `as u64` is safe and order-preserving.
`content_hash` is stored so the eagle_time→text resolution has an integrity check and so the adversarial same-tick collision has a tiebreak available (not yet wired to the wire — §13).

### 8.2 The weave window = the DB tail

The braid's "last ≤256 eligible" is a tail slice of the message table filtered to incoming (`is_outgoing == false`) — no separate ring structure. Reading ≤256 rows per send is cheap. Eligibility ("both-held") is automatic: any stored incoming row is one the receive path already ACKed.

### 8.3 At-rest encryption

The vault and rārangi stores are encrypted at rest with the device key (per-platform secure storage: libsecret/keyring, Keychain, DPAPI, Android Keystore). Process isolation is assumed; disk encryption is not.

---

## 9. Wire Format (VSF)

### 9.0 Encrypted Message Document

Standard VSF document — timestamp lives in the header, not duplicated in the body:

```
HEADER (standard VSF):
  Version 5
  Created e(eagle_time)     ← message timestamp HERE ONLY
  hp(provenance_hash)       - 32B BLAKE3 of body
  se(signature)             - 64B Ed25519 signature
  ke(pubkey)                - 32B sender device pubkey
BODY [message]:
  v(vP, ciphertext)         - Photon-wrapped encrypted field
```

**Plaintext field (encrypted inside `v`):**
```
(message: x{text}, hp{inc_hp}, e6{woven_time}…, hR{pad})
   x{text}        - UTF-8 user message; the braid's weave ingredient
   hp{inc_hp}     - 32B legacy implicit-ACK pointer (not the weave reference)
   e6{woven_time} - 0, 1, or 2 eagle_times naming the woven peer messages
   hR{pad}        - 0-255B random padding
Field values are shuffled before encryption; parse by type marker, not position.
```

### 9.1 Routing section

```
[routing
  hb(sender_handle_hash)   - 32B
  hb(friendship_id)        - 32B
  hp(prev_msg_hp)          - 32B hash-chain link (ordering; first msg uses the anchor)
]
```

`prev_msg_hp` links messages into a hash chain. The first message uses a deterministic anchor derived from the friendship ID. An unexpected `prev_msg_hp` means "ahead" → buffer (§6.3).

### 9.2 ACK section

```
[ack
  e(eagle_time)   - which message we're ACKing
  hb(ack_proof)   - 32B domain-separated fast decrypt proof
]
```

ACK proof is a fast `smear_hash` (microseconds, not memory-hard), domain-separated (`PHOTON_ACK_v0`) and over a different link range than the advance, so it can't be reused as a fresh link or vice versa.

---

## 10. Reliability and Ordering State

### 10.1 Pending messages (sender)

```rust
struct PendingMessage {
    eagle_time: i64,
    plaintext: Vec<u8>,        // our flattened payload (for our_plaintext on advance)
    plaintext_hash: [u8; 32],  // ACK verification + advancement
    prev_msg_hp: [u8; 32],
    msg_hp: [u8; 32],
    ciphertext: Vec<u8>,       // for retransmit
    woven_strands: Vec<Vec<u8>>, // the braid strands FROZEN at send time (0/1/2, sorted)
}
```

`woven_strands` is the load-bearing braid field: it freezes the exact strand bytes this message braided, so the matching `process_ack` advances the sender's chain with the SAME bytes the receiver used — regardless of what messages arrive between send and ACK.

### 10.2 ACK advances the sender's chain

```
On ACK(eagle_time, plaintext_hash):
  find the matching PendingMessage
  advance(our_handle_hash, eagle_time, pending.plaintext, &pending.woven_strands)
  update last_plaintext (for the next salt)
  remove from pending
```

### 10.3 Receiver processing

Strict in-order with the gap buffer and replay queue (§6.3). After a successful decrypt+advance the receiver persists chain state to disk BEFORE sending the ACK (disk is the commit point; the ACK is just notification), then appends the message to the conversation and ACKs.

---

## 11. Security Properties

### 11.0 Forward Secrecy

| Scenario | Past Messages | Future Messages |
|----------|---------------|-----------------|
| Current chain compromised | Protected | Compromised until re-CLUTCH |
| Device key compromised | Protected (chain-encrypted) | At risk on that device |
| Both compromised | At risk | At risk |

The braid's reach-back adds bidirectional cross-entropy: an attacker who recovers partial state still cannot predict which prior peer secrets mix into the next link, because selection is CSPRNG-random over the window and named only on the (encrypted) wire.

### 11.1 Authentication

| Property | Mechanism |
|----------|-----------|
| Outer integrity | Standard VSF Ed25519 signature (device-bound) |
| Inner integrity | Chain-bound decryption (only a chain holder can decrypt) |
| Sender identity | Chain state + device key |
| Replay protection | Eagle time uniqueness + is_duplicate |
| Ordering | prev_msg_hp hash chain + strict in-order processing |

### 11.2 Defense Layers

```
Layer 0:  CLUTCH ceremony (eggs from 8 algorithms)
Layer 1:  Avalanche expansion (2MB memory-hard)
Layer 2:  Truncate-and-append chain derivation (smear_hash)
Layer 3:  L1 scratch pad (memory-hard, data-dependent)
Layer 4:  ChaCha20 stream cipher
Layer 5:  XOR with scratch pad
Layer 6:  Standard VSF Ed25519 signature (outer integrity)
Layer 7:  Domain-separated ACK proof (fast smear)
Layer 8:  The braid — spaghettify advancement weaving two peer strands (per message)
Layer 9:  Device key encryption at rest
```

Memory-hard operations (Layers 1, 3) happen at setup; the fast path keeps tx/rx snappy. The braid advance (Layer 8) is per-message but spaghettify is chaos-hard, not memory-hard.

---

## 12. Implementation Checklist

### 12.0 Core types
- [x] `Chain` with 512 links ([0..256]=history, [256..512]=active)
- [x] `FriendshipChains` (chains + participants + pending + gap_buffer)
- [x] `PendingMessage` with frozen `woven_strands`
- [x] `BufferedMessage` keyed on `prev_msg_hp` (gap buffer)

### 12.1 The braid
- [x] Send-side: select up to 2 distinct incoming messages from the last ≤256 (`gen_range`, not modulo), sorted by eagle_time
- [x] Wire: carry the chosen eagle_times as `e6` values
- [x] Receive-side: collect `e6` refs, resolve to OUR outgoing `content` by eagle_time, sort, feed to `advance`
- [x] `derive_fresh_link(eagle_time, our_plaintext, their_plaintexts, chain)` — peer-first, length-prefixed
- [ ] Same-tick collision tiebreak carried on the wire (content_hash) — see §13

### 12.2 Strict ordering
- [x] HARD `verify_chain_link` (ahead → buffer + skip)
- [x] `buffer_for_gap` / `take_buffered_for` wired into the receive path
- [x] Replay queue drains buffered messages in order, cascading

### 12.3 Encryption / advancement
- [x] `generate_scratch()` (L1 memory-hard), `derive_salt()` (chained), ChaCha20 + XOR layers
- [x] `advance()` — left-shift + spaghettify new link at [511]
- [x] `generate_ack_proof()` / `verify_ack_proof()` (fast smear, domain-separated)
- [x] Receiver advances per-message in order; sender advances on ACK with frozen strands

### 12.4 Storage
- [x] rārangi rows keyed by eagle_time, with `content_hash`
- [x] Chain state persistence (16KB per participant) in the flat vault

---

## 13. Known Gaps

- **Same-tick eagle_time collision on the wire.** Two fleet devices (same identity) emitting at the same 704ps tick would produce duplicate woven references; the receiver currently resolves to the first match. The `content_hash` is stored (§8.1) but not yet carried on the wire as a tiebreak. Adversarial/contrived only — not a routine path.
- **Restart mid-flight with a non-empty braid.** A `PendingMessage` reloaded after restart weaves no strands (the frozen `woven_strands` are runtime-only, not persisted). Pending messages are short-lived (cleared on ACK), so this only bites if the app restarts with an unacked, non-empty-braid message in flight.
- **Legacy weave machinery still present.** `incorporated_hp` / `last_incorporated_hp` / `update_received_for_mixing` remain as the implicit-ACK signal but are no longer the weave reference; they can be retired once the implicit-ACK role is folded elsewhere.

---

## Appendix A: Constants

```rust
const HISTORY_LINKS: usize = 256;   // links[0..256] — zeros initially
const ACTIVE_LINKS:  usize = 256;   // links[256..512] — derived from CLUTCH
const CHAIN_LINKS:   usize = 512;
const LINK_SIZE:     usize = 32;
const CHAIN_SIZE:    usize = CHAIN_LINKS * LINK_SIZE;  // 16KB
const CURRENT_KEY_INDEX: usize = 511;                 // rightmost = newest

const L1_SIZE:   usize = 30_720;    // 30KB
const L1_ROUNDS: usize = 3;

const BRAID_WINDOW: usize = 256;    // tunable: how many recent messages are weave-eligible
const BRAID_STRANDS_MAX: usize = 2; // two distinct strands = a braid

// Domain separation
const DOMAIN_ADVANCE:   &[u8] = b"PHOTON_ADVANCE_v0";   // chain advancement (spaghettify)
const DOMAIN_ACK:       &[u8] = b"PHOTON_ACK_v0";       // ACK proof (fast smear)
const DOMAIN_SALT:      &[u8] = b"PHOTON_SALT_v0";      // salt derivation
const DOMAIN_FIRST_MSG: &[u8] = b"PHOTON_FIRST_MSG_v0"; // first-message anchor
```

## Appendix B: VSF Type Reference (Photon extensions)

```rust
// Standard VSF types (lowercase)
d(String),       // field name
x(String),       // UTF-8 text (Huffman) — the braid's weave ingredient
e(EtType),       // Eagle time; e5(i32)/e6(i64)/e7(i128). The braid uses e6.
hp(Vec<u8>),     // BLAKE3 provenance hash
hb(Vec<u8>),     // BLAKE3 rolling hash
ke(Vec<u8>),     // Ed25519 public key
se(Vec<u8>),     // Ed25519 signature
v(u8, Vec<u8>),  // wrapped data with encoding byte; vP = Photon encrypted

// Application-defined (UPPERCASE second char)
hR(Vec<u8>),  // random padding (traffic-analysis resistance)
hG(Vec<u8>),  // spaghettify output
hM(Vec<u8>),  // smear hash: BLAKE3 ⊕ SHA3 ⊕ SHA512
```
