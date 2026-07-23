# The Braid — Chain Protocol Specification v0.2

**Protocol:** The Braid (Post-CLUTCH Rolling-Chain Encryption)
**Author:** Nick Spiker
**Status:** Draft — §1–§13 reflect the shipped friend-facing plane (commit 9bf1193). §0.2 + §14 add the fleet-internal multi-writer plane (design, pre-implementation).
**License:** MIT OR Apache-2.0
**Date:** June 2026 (supersedes CHAIN v0.0, December 2025)
**Dependency:** Requires a completed CLUTCH ceremony (see clutch.md)
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

### 0.2 Two planes: the friend-facing braid and the fleet-internal DAG (v0.2)

v0.1 (§1–§13) specifies the **friend-facing plane**: one linear braid strand per party, forward-secret, self-authenticating. That plane is **unchanged** in v0.2 — a friend's client runs exactly §1–§13, sees a single ordered strand, and learns nothing about how many devices you have.

v0.2 adds the **fleet-internal plane** (§14): your own devices — one identity, many devices, "the fleet" — replicate the conversation *among themselves* as a multi-writer set, sealed under the fleet key, so any device holds the full history and can continue the conversation. The fleet **linearizes** that set into the single strand the friend sees.

**Why two planes.** Making the friend fold a multi-writer structure would (a) destroy forward secrecy — a content-addressed key over a static root is recomputable by anyone holding that root, forever; (b) leak your device count via multi-writer merges; (c) force a bilateral wire flag-day on every friend's client. Confining the multi-writer machinery to the fleet keeps the friend on the forward-secret linear wire and puts all the complexity where the fleet key already gates it.

**Statelessness — the invariant §14 is built around.** A device stores **no durable unique secret but its `ihi`** (the born-in-silicon device key; today a software stand-in, see `fingerprint.rs`). Every other piece of state — reservoir, epoch keys, message log, roster — is fleet-key-sealed and re-fetchable, so a wiped device re-derives its `ihi`, recovers the current fleet key from the always-online slot (§14.2), re-fetches the sealed log, and recomputes. The vault is a disposable cache, never the source of truth.

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

**Why spaghettify?** Computationally chaotic (data-dependent branching over a 32-op integer ALU, path explosion), multi-algorithm (uses smear_hash internally), NOT memory-hard (~1.7KB state) — fast enough for per-message use. Domain-separated: `PHOTON_ADVANCE_v0` ≠ `PHOTON_ACK_v0`. **Pure integer math, zero floating point** — bit-identical on every architecture (ARM phone, x86 desktop, PIPE silicon), which is exactly what the braid's cross-device determinism demands. (`ihi`'s `F16E8` lane naming evokes a float's fraction/exponent but is two *integer* lanes, not IEEE — the whole reason ihi exists is that floating point is not bit-portable and would desync two peers.)

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

This table is the **friend-facing plane** and is unchanged in v0.2. The fleet-internal plane has its own, deliberately different FS/PCS semantics — reservoir-burn key-FS, a per-conversation content-FS dial, and a weaker (membership-rotation-bounded) post-compromise story — all stated out loud in §14.10.

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
- **Same-tick collision blocks fleet-plane ordering.** The `content_hash` tiebreak (stored, not yet on the wire) is *mandatory* for the fleet plane's strict total order — see §14.11 G6. The v2 gaps live in §14.11, not here.

---

## 14. The Fleet Plane — Multi-Writer Replication (v2)

> **Status:** design, pre-implementation. Today's code has a single static fleet key (`fleet.rs`), the roster CRDT (§`fstate`), and the pairing hand-off — none of §14 is built yet. Two items below are also *live* gaps to fix, flagged inline. This section was written after three adversarial red-team passes; the fixes those passes forced are folded in, not bolted on.

### 14.0 What the fleet is, and what a device keeps

A **fleet** is one identity's many devices, enumerated by the signed membership chain (`fleet.rs` `MembershipBlob` — the v1 keyring). The fleet plane replicates a friend-conversation *across those devices* so any one of them holds the whole thing and can carry it.

A device keeps exactly one durable secret: its **`ihi`** (device key). Everything else lives **fleet-key-sealed in an always-online network slot** (per-conversation, on FGTW) and is re-fetchable. This is the statelessness invariant (§0.2): a wiped device resurrects from `ihi` alone by recovering the fleet key (§14.2) and re-fetching.

The friend-facing braid (§1–§13) is untouched. The fleet plane is a *replication + forward-secrecy* layer beneath it, plus a *linearizer* (§14.6) that emits the single strand the friend consumes.

### 14.1 Message identity — self-recognition without attribution

Every message carries a **tag**:

```
T = blake3(DOMAIN_MSG_TAG ‖ device_private ‖ eagle_time)
```

- **Self-recognition, stored nowhere.** A device recomputes `T` from its own `device_private` and a message's `eagle_time` and checks equality — so it recognizes *its own* messages without storing any "sent-by" column (statelessness). It also learns "not mine" for everyone else's, and *nothing more*.
- **Unlinkable.** Any other party — sibling **or** friend — sees an opaque per-message value and cannot attribute it to a device or even count devices. Device identity and count leak to *no one*. (Caveat → §14.11 G-tag.)
- **Nonce + uniqueness.** `T` folds into the message key seed (§14.3) as the per-message uniquifier, so two identical plaintexts can never reuse a keystream. It is unique even in the same-704ps-tick fleet case (distinct `device_private` → distinct `T`).

The **node id** used for the DAG (parent refs, dedup, content-addressing) is:

```
id = blake3(DOMAIN_NODE_ID ‖ T ‖ content_hash)
```

- Content-bound (a fabricated ref whose preimage never arrives is dropped after a bounded buffer, not chased forever), device-hiding (`T` carries no recoverable pubkey), and — because fleet nodes are all *your own* devices sealed under the fleet key — **not per-device signed** (a signature would re-attribute). A misbehaving device is a *compromised* device, handled by removal + rotation (§14.2), not by per-message fingerprinting.

**Total order.** Any deterministic derivation across the fleet (linearization, checkpoint sealing) orders nodes by the strict triple **`(eagle_time, content_hash, device_tag)`** — never `eagle_time` alone. This closes the §13 same-tick collision *for the fleet plane* and requires `content_hash` on the wire (v0.1 stored it but didn't carry it — §14.11 G6).

### 14.2 Fleet keys — per-member re-encryption fan-out

The fleet key is **not** a chain of keys each sealed under the previous — that was a skeleton key (any one historical key unrolls all future ones, so a *removed* device could read everything forever). Instead:

- Each epoch mints a **fresh CSPRNG fleet key**, wrapped **separately to each current member device's public key** (X25519 derived from the same `ihi`; the pubkeys are already in the folded `MembershipBlob`). The wraps sit in the always-online slot.
- **Recovery needs no live sibling.** A returning/wiped device unwraps *its own* copy with its `ihi` — so "always-online" finally means "always-**recoverable** for current members." This is the single fix that dissolves the stranding cases in §14.7.
- **Removal removes.** On a device Remove, the fleet rotates (new fresh key, re-fanned-out to the *remaining* members, and the current slot content re-sealed under it). The removed device is simply not a wrap target. **Shipped 2026-07-23:** any surviving member's key sync doubles as the removal sentinel (`fanout_needs_rotation`: the fan-out wrapping MORE devices than the fold holds = a leaver's wrap lingers) and heals — pull the fstate slot under the old key, rotate to the survivors, re-push the CRDT merge under the new epoch, then rotate the avatar bearer pin. Sibling-triggered only (the leaver minting the next key would know it); strictly shrink-triggered (the wraps<members state is the two-phase ADD window, where auto-rotating would release the key before the sponsor's confirm). Rotation cuts FORWARD access only — pre-rotation reads are already in the leaver's hands — and waits for a surviving member to be online (the §14.7 coalescing doctrine).
- **The pairing hand-off (`fkey`) is the *first-join* case only, and is single-use + expiring** (shipped: `fkey_ack` + 5-minute GET expiry) so the pairing-secret-wrapped key never lingers as an escrow. Steady-state key delivery is the fan-out, not the pairing wrap.

This is the MLS / sender-keys shape: fan-out to current members, rotate on membership change.

### 14.3 The reservoir burn — forward secrecy, by checkpoint

FS comes from **burning the avalanche reservoir forward** (the 2MB pad, `clutch.rs` `avalanche_expand_eggs`), not from destroying per-message links (that's the friend-facing plane's mechanism). Rules:

- **The eggs are dropped after the initial seed.** Keeping them would make the reservoir re-derivable = no FS. The reservoir (burning) is what's distributed, never the eggs.
- **Epoch index advances per *sealed checkpoint*, never per message.** "Per message with no total order" is self-contradictory — two devices sending concurrently would fork the reservoir irrecoverably (nothing folded to reconcile from). So:

  ```
  epoch_k = KDF^k(seed)            // k = the sealed checkpoint number (§14.4), a scalar
  ```

  A returner derives `epoch_k` in one shot from `(seed, k)` — zero dependence on how many messages it replayed or in what order.
- **The burn folds NO message content** — it's a pure reservoir ratchet, so resync needs only reservoir state, not a complete/ordered message log.
- **Re-expansion, when the reservoir runs low, is deterministic *and* mixes the current fleet key:**

  ```
  seed_{next} = KDF(reservoir_tail_consumed ‖ DOMAIN_REEXPAND ‖ k ‖ fleet_key_epoch)
  ```

  Folding the rotating fleet key means every **membership change forces a post-compromise recovery boundary** (a leaked reservoir stops predicting future epochs once the fleet re-keys) — the G4 decision. No fresh entropy otherwise, so all devices re-expand identically (fork-free).
- **Granularity rule for the lag.** The reservoir may lag (one KDF step per checkpoint) *because* it is coarse epoch key material — no two messages ever encrypt under raw reservoir output; each derives its own key with its per-message tag `T` (§14.1) folded in. The per-message L1 scratch (§4) may **not** lag: it *is* the per-message keystream, so reusing it across a settled position is straight keystream reuse (`C1⊕C2 = P1⊕P2`). The rule of thumb: lag the coarse secret, never lag the fine keystream.

### 14.4 Checkpoints — the totally-ordered spine

Messages are an unordered union set (§14.5). **Checkpoints are not** — a checkpoint advances the burn horizon and *zeroizes keys*, so it MUST be totally ordered:

- A checkpoint `C_k` is a signed record over the merged prefix, carrying `k` (a monotonic sequence number *inside* the signed body — never worker-receipt time) and the merkle root of the settled nodes at/below it.
- Committed to the slot via **compare-and-set** (R2 `onlyIf`/If-Match): exactly one `C_k` wins; a loser gets 412 and re-derives against the winner. Sealing is an **idempotent pure function of the merged prefix**, so concurrent sealers *converge* rather than race.
- The checkpoint sequence is the one place the fleet plane keeps a signed chain; it rides the same `extends()` discipline as the membership chain.

### 14.5 The message set + the fleet-sync channel

- **Substrate:** a **grow-only, content-addressed, fleet-key-sealed set** of nodes, one slot per `friendship_id` on FGTW, **union-merge** (never last-writer-wins-on-blob — that would clobber a concurrent sibling append; note the roster `fstate` slot *is* LWW and is deliberately a different, single-writer-per-value thing).
- **Lockstep = anti-entropy.** Two devices exchange a compact have-set digest (id list / bloom of the frontier), diff, fetch missing ids. Concurrency can't fork it (union). No message-level chain.
- **Retrieval of a specific message:** check local set → gossip "who has `id`?" → a source serves it sealed under the current epoch key → verify by content-id + AEAD. Below the horizon (burned) → §14.9 recovery ladder.
- **Nodes carry the friend-facing braid metadata** (woven strand refs, slot) so any device can reconstruct and emit the external strand (§14.6). Carrying it is not securing the channel with it — the fleet key + reservoir do that.
- **The fleet seal has no scratch.** A fleet node is a plain AEAD under the high-entropy epoch key (reservoir + fleet key) — brute-force isn't on the table, so the memory-hard L1 scratch (§4) earns nothing here. That scratch exists on the *friend-facing* plane to harden a lower-entropy evolving chain key against offline attack; the fleet key is already strong. The only memory-hard work on the fleet plane is the per-checkpoint reservoir re-expand (§14.3) — the one operation that legitimately lags.

### 14.6 The linearizer — one strand to the friend

The friend must see a single v0.1 strand, so **exactly one device advances the friend-facing chain per position**, for both send and receive. Because it's one human, concurrent external emission is rare, so:

- **Soft speaker-token that follows the active device.** Whichever device you're using holds the token and emits externally; it hands off on device switch. The token is a claim in the fleet log on the *next external sequence position*.
- **Rare-case rebase.** If two devices do race the position, the fleet-internal total order (§14.1) picks the winner; the loser re-bases its external send onto the winner's new head *before* it reaches the friend. A device must not emit to the friend until it has won the position internally — otherwise the friend forks (the v0.1 desync, now on their side).
- Receiving is the mirror: one device processes-and-advances the friend-facing chain for a given inbound message; siblings learn the advanced state via the sealed log.

### 14.7 Resync — the CAN guarantee, cursors, and the return window

We never confirm devices *have* synced (they may be offline); we guarantee they *can*, as a property of **data availability**, not device liveness:

- **The current epoch key is always recoverable** from the always-online slot by any current member using its `ihi` alone (§14.2 fan-out). This is what makes "can resync" true without a live sibling.
- **The prune/burn horizon advances to `min(per-member signed sync-cursor)`** — each device publishes a signed "last-synced checkpoint" cursor to the slot — **except** past a hard **wall-clock grace `W`**, after which advancing past a silent member is a deliberate, **logged, UI-surfaced** "this device will need to re-pair" decision. Never silent, never ACK-gated on an indefinitely-offline device (a lost device can't hold FS hostage).
- **Convergence gate:** before any key-zeroize, a read-back confirms the slot actually holds every node referenced at/below `C_k` — this waits only on the always-online slot containing the bytes, not on offline ACKs.
- **One window to rule them all.** Rotation records, epoch keys, content, and checkpoints share a single user-facing **return window `W`** (wall-clock, months — never a *count*, since churn could burn N rotations in minutes). So "can open but nothing to read" and "can read but can't open" are impossible by construction. Membership-change rotations are rate-limited/coalesced so churn can't outrun `W`.

**Guarantee, stated precisely:** any current-member device offline **≤ W** resyncs deterministically from the slot. Offline **> W** → it re-pairs (a sibling), losing only unreadable pre-horizon history — never live participation.

### 14.8 Crypto-shredding — why "delete the slot" is not destruction

R2 delete/overwrite is best-effort; a provider can retain overwritten bytes, and an attacker can cheaply record the (unauthenticated-GET) ciphertext. So **below-horizon FS never rests on the object being gone.** Instead:

- Slot content is encrypted under **per-checkpoint content keys that live only on-device** and are **zeroized on-device** at the horizon. The standing fleet key alone must *not* open pre-horizon content.
- The R2 delete is defence-in-depth. Document (and mean) that FS = on-device key destruction, full stop.

### 14.9 The recovery ladder

1. **Within `W`, some devices alive** → anti-entropy from a sibling or the slot.
2. **Returner past its cursor but ≤ `W`** → recover current epoch key from the fan-out slot with `ihi`; catch up.
3. **Returner > `W`** → re-pair (needs one live sibling); lose pre-horizon history only.
4. **Whole fleet lost** → mint fresh eggs, re-CLUTCH friends, friends re-serve *their* retained history. The synced contact list says whom to ask. Re-CLUTCH (needs the friend online) is the whole-fleet-loss backstop **only**, never a single-returning-device path.

### 14.10 Security semantics, stated out loud

- **Key-FS (friend-facing):** unchanged from v0.1 — links are destroyed on advance.
- **Key-FS (fleet-internal):** the reservoir burns forward and per-checkpoint content keys are crypto-shredded → past epochs unrecoverable after the horizon, even by a fleet-key holder.
- **Content-FS:** a **per-conversation dial** — keep-forever (max history sync, no content-FS) vs a retention horizon (content-FS past it; a new device backfills only *to* the horizon). Because history sync retains plaintext *by design*, "FS against a compromised device" is retention-bounded — same as every synced messenger. Key-FS and content-FS are a single dial over the {key + content} pair; a retained node whose key is gone is a *bug to surface*, never silently shown as unopenable history.
- **Post-compromise security (fleet-internal):** weaker than the friend-facing braid — a reservoir leaked while a device is online predicts future epochs *until the next membership rotation re-keys the re-expand* (§14.3). That rotation is the PCS boundary; a long-lived fleet with no membership change has a correspondingly long PCS exposure. Stated, not hidden.
- **Operator visibility:** FGTW sees ciphertext + the coarse retention schedule (which epochs still exist). Acceptable *because* crypto-shredding makes "the slot still holds epoch e" convey no decryptability.

### 14.11 Known gaps (v2)

- **G-tag — post-compromise deanonymization.** `T = blake3(device_private ‖ eagle_time)` is unlinkable until `device_private` leaks, after which an attacker recomputes past `T`s and re-attributes messages to that device. Acceptable (device-secret compromise breaks more), stated.
- **G1 — no fleet-only backstop.** Below-horizon *friend-facing* content is friend-recoverable; fleet-internal-only state (own notes, prefs, or a conversation the friend also pruned) has no backstop past `W`.
- **G2 — bounded, not unconditional.** The slot is FS-preserving durability with an explicit window, not infinite availability. Long-return stranding is first-class: a device detects it is below-horizon and *triggers re-pair*, never silently missing history.
- **G4 — decided:** the rotating fleet key is folded into re-expand (§14.3) so membership change is a PCS boundary. Chosen over silently accepting the downgrade.
- **G5 — trailing-K exposure.** The since-checkpoint tail (≤ one checkpoint of messages) is decryptable-if-key-obtained by construction; keep the checkpoint cadence tight; resync is fetch-then-shred.
- **G6 — same-tick on the wire.** The strict total order `(eagle_time, content_hash, device_tag)` requires `content_hash` carried on the wire (§13 stored it but didn't carry it). Mandatory for the fleet plane; the friend-facing sort may stay `eagle_time`-only.
- **G7 — equal-counts shadow.** Fan-out wraps carry no plaintext target (recipients self-select by key-commitment), so the removal sentinel is a count comparison — a simultaneous depart+bind leaves wraps == members with a leaver's wrap still inside, invisible until the next shrink or the sponsor's confirm rotation heals it. Narrow window, self-healing, stated.

### 14.12 Implementation status

Live today: per-member fan-out key slot + `ihi`-recovery (items 1–2), roster + settings CRDT, pairing hand-off (single-use + expiring). The rest of §14 is unbuilt. Ordered build:

1. **Removal rotates** (§14.2 fan-out) — ✅ shipped 2026-07-23: sibling-triggered shrink sentinel in the key sync, fstate preserved across the re-seal, avatar bearer pin rotated by the winner.
2. Per-member fan-out key slot + `ihi`-recovery — ✅ live (shipped with device-ADD v1: `post_fanout`/`recover_fleet_key`, epoch-monotonic worker guard, recovery on attest + every `fleet` bump).
3. Union-merge per-conversation sync channel (§14.5) with anti-entropy.
4. Checkpoint spine (§14.4, CAS) + the reservoir burn (§14.3).
5. Cursor-based horizon + crypto-shred (§14.7–14.8).
6. The linearizer (§14.6) — last, since it assumes the log exists.

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

// Domain separation — v2 fleet plane (§14)
const DOMAIN_MSG_TAG:  &[u8] = b"PHOTON_MSG_TAG_v2";  // self-recognition tag  T  = blake3(· ‖ device_private ‖ eagle_time)
const DOMAIN_NODE_ID:  &[u8] = b"PHOTON_NODE_ID_v2";  // content-addressed id  id = blake3(· ‖ T ‖ content_hash)
const DOMAIN_REEXPAND: &[u8] = b"PHOTON_REEXPAND_v2"; // reservoir re-expansion (folds fleet_key_epoch → PCS boundary)
```

**v2 parameters (tunable, unset pending real numbers):** the return window `W` (wall-clock, target months — §14.7), the checkpoint cadence (messages per `C_k`, bounds the trailing-K exposure G5 — §14.4/§14.11), and `T_grace` for a silent member before the horizon steps past it (§14.7). Sized against real bandwidth-delay and usage once the plane is built; deliberately not baked in here.

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
