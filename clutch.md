# CLUTCH Protocol Specification (Draft v0.1)

**Cryptographic Layered Universal Trust Commitment Handshake**

## Overview

CLUTCH is a multi-primitive key exchange protocol that combines multiple post-quantum and classical cryptographic primitives with timing-based authentication to establish secure communication channels. Security is achieved through cryptographic diversity - an attacker must break ALL primitives simultaneously to compromise the session.

---

## Core Principles

1. **Cryptographic Diversity**: Multiple mathematically distinct hard problems
2. **Sequential Ratcheting**: Each exchange builds on previous exchanges
3. **Timing Authentication**: RTT measurements provide physical-layer verification
4. **Graceful Degradation**: Failed exchanges can be skipped if both parties agree
5. **Obfuscation Layer**: Partial exchanges and interleaving resist traffic analysis

---

## Protocol Primitives (The Eggs)

### Required Primitives
- **Classical**: ECDH (X25519 or P-256)
- **Lattice-based**: ML-KEM (Kyber)
- **Code-based**: Classic McEliece or HQC
- **Isogeny-based**: CSIDH
- **Hash-based**: SPHINCS+ (for signatures)
- **Spirix-based**: Custom Spirix arithmetic key exchange

### Optional/Extended Primitives
- Multivariate: Rainbow or similar
- Additional lattice schemes: NTRU
- Additional isogeny schemes: SIDH variants
- Custom primitives specific to implementation

---

## Protocol Phases

### Phase 0: Initial Commitment

```
Initiator → Responder: HELLO
  - Protocol version
  - Supported primitives (bitmask)
  - Nonce_I
  - Timestamp_I

Responder → Initiator: HELLO_ACK
  - Selected primitives (bitmask)
  - Nonce_R  
  - Timestamp_R
  - RTT_estimate

Both parties:
  - Agree on primitive set S = {P₁, P₂, ..., Pₙ}
  - Initialize state: State₀ = KDF(Nonce_I || Nonce_R)
```

### Phase 1: Sequential Exchanges

For each primitive Pᵢ in agreed set:

```
Exchange Pᵢ:
  T_start = timestamp()
  
  Initiator → Responder: EXCHANGE_i
    - Primitive_ID = i
    - Key_material_i (public key or first message)
    - HMAC(State_{i-1}, Exchange_i)
    - Encryption: encrypted under State_{i-1} (for i > 1)
  
  Responder → Initiator: EXCHANGE_i_RESPONSE
    - Primitive_ID = i
    - Key_material_i_response
    - HMAC(State_{i-1}, Response_i)
    - Encryption: encrypted under State_{i-1}
  
  T_end = timestamp()
  RTT_i = T_end - T_start
  
  Both parties compute:
    - shared_secret_i = DeriveSecret(Pᵢ, key_material_i, key_material_i_response)
    - State_i = KDF(State_{i-1} || shared_secret_i || RTT_i || Timestamp)
  
  Initiator → Responder: CONFIRM_i
    - HMAC(State_i, "CONFIRM")
    
  If CONFIRM fails or RTT anomalous:
    → Mark Pᵢ as FAILED
    → Continue to next primitive
```

### Phase 2: Validation & Key Derivation

```
Both parties:
  - Collect successful exchanges: Success = {P₁, P₃, P₄, ...}
  - Verify minimum threshold met (e.g., 3 out of 6 primitives)
  
  Final_Key = KDF(
    State_n ||
    Success_bitmask ||
    RTT₁ || RTT₂ || ... || RTTₙ ||
    Nonce_I || Nonce_R
  )

Initiator → Responder: COMMIT
  - HMAC(Final_Key, "INITIATOR_COMMIT")
  - List of successful exchanges
  
Responder → Initiator: COMMIT_ACK  
  - HMAC(Final_Key, "RESPONDER_COMMIT")
  - Verification of success list

If both match:
  ✓ CLUTCH complete
  Use Final_Key for session encryption
```

---

## Variants

### Variant A: Synchronous CLUTCH
- All exchanges sequential, blocking
- Wait for each to complete before starting next
- **Use case**: High-security, can tolerate latency
- **Latency**: Sum of all RTTs × number of primitives

### Variant B: Pipelined CLUTCH  
- Start exchange i+1 before confirming i
- Multiple exchanges in flight
- **Use case**: Lower latency, still secure
- **Latency**: Max RTT + processing time

### Variant C: Parallel CLUTCH
- Send ALL exchange requests simultaneously
- Collect responses as they arrive
- **Use case**: Minimum latency
- **Latency**: Max(RTT₁, RTT₂, ..., RTTₙ)
- **Caveat**: Can't encrypt later exchanges under earlier keys

### Variant D: Spaghetti CLUTCH (Obfuscated)

```
Partial exchanges spread across multiple rounds:

Round 1:
  - 40% of ECDH key
  - Decoy data labeled "Kyber"
  - 100% of SPHINCS+ signature
  
Round 2:  
  - 100% of McEliece key
  - 30% of Kyber key
  - Decoy data labeled "Multivariate"

Round 3:
  - Remaining 60% of ECDH key
  - Remaining 70% of Kyber key
  - 100% of CSIDH key

Assembly Map (shared secret):
  - Map specifies which rounds contribute to which primitives
  - Map derived from initial nonces
  - Makes traffic analysis difficult
```

### Variant E: Adaptive CLUTCH

```
Start with fast primitives, add more based on threat level:

Security Level 1 (Low): 
  - ECDH + Kyber (2 primitives)

Security Level 2 (Medium):
  - Add McEliece + CSIDH (4 primitives)

Security Level 3 (High):  
  - Add Spirix + SPHINCS+ (6 primitives)

Security Level 4 (Paranoid):
  - Add NTRU + Multivariate + decoys (8+ primitives)
  - Enable Spaghetti mode
```

---

## Timing Integration

### Timing as Authentication

```
During exchange i:
  Expected_RTT = Average(RTT₁, ..., RTT_{i-1})
  Tolerance = StdDev(RTT₁, ..., RTT_{i-1}) × 3
  
  If |RTT_i - Expected_RTT| > Tolerance:
    → Possible MITM/relay attack
    → Log anomaly
    → Consider aborting or downgrading trust
```

### Timing as Key Material

```
RTT_quantized = floor(RTT_microseconds / quantum)
quantum = 100μs (adjustable)

RTT contributes to key derivation:
  State_i = KDF(State_{i-1} || shared_secret_i || RTT_quantized_i)

This means:
  - Keys are unique to this session at this time
  - Replay attacks from different network position fail
  - RTT acts as implicit freshness token
```

### Timing Fingerprinting

```
Record RTT profile for each primitive:
  Profile = {
    ECDH: avg=2ms, stddev=0.5ms,
    Kyber: avg=3ms, stddev=0.8ms,
    McEliece: avg=150ms, stddev=20ms  // Large keys!
  }

On subsequent connections:
  - Compare RTT profile to baseline
  - Detect if primitive implementations changed
  - Detect if network path changed significantly
```

---

## Graceful Degradation

### Handling Failed Exchanges

```
Minimum threshold: T = 3 primitives (configurable)

If exchange P_i fails:
  1. Log failure reason
  2. Continue to next primitive
  3. Mark P_i as unavailable in Success bitmask
  
After all exchanges:
  If |Success| >= T:
    ✓ Proceed with Final_Key from successful primitives
  Else:
    ✗ Abort connection (insufficient diversity)
```

### Real Key Validation

**Your brilliant addition:** Each party validates received key material before committing.

```
On receiving key_material_i:
  1. Validate format (correct size, structure)
  2. Validate cryptographic properties:
     - Public key on correct curve/group
     - Non-trivial element (not identity/zero)
     - Passes weak key checks
  3. Attempt to derive shared_secret_i
  4. If ANY validation fails:
     → Send EXCHANGE_i_INVALID
     → Skip this primitive
     → Continue to next

Responder → Initiator: EXCHANGE_i_INVALID
  - Reason code
  - "Let's use next primitive"
  
Both parties mark P_i as FAILED, increment i, continue.
```

### Example: Corrupted McEliece Key

```
Exchange 3 (McEliece):
  Initiator → Responder: McEliece public key (1MB)
  [Network corruption flips some bits]
  
  Responder checks:
    - Size: ✓ 1MB
    - Format: ✗ Not a valid generator matrix
    
  Responder → Initiator: EXCHANGE_3_INVALID
    - Reason: "Invalid generator matrix"
    - "Skipping McEliece, proceeding to CSIDH"
    
  Both parties:
    - Mark McEliece as FAILED
    - Proceed to Exchange 4 (CSIDH)
    - Final key will not include McEliece contribution
```

This is CRITICAL because:
- Real networks are messy
- Large keys (McEliece) more prone to corruption  
- Prevents hanging on broken exchanges
- Maintains security through remaining primitives

---

## Security Properties

### Threat Model

**CLUTCH provides security against:**
- Passive eavesdropping (Eve observing all traffic)
- Active MITM (Eve intercepting and modifying)
- Relay attacks (Eve forwarding traffic with added latency)
- Future quantum computers (multiple PQ primitives)
- Single primitive compromise (cryptographic diversity)
- Traffic analysis (obfuscation variants)

**CLUTCH assumes:**
- Initial connection authenticity (first HELLO verified somehow)
- No compromise of both endpoints
- Honest implementation (no backdoored crypto libraries)

### Security Proof Sketch

**Theorem**: Breaking CLUTCH requires breaking ALL successful primitives simultaneously.

**Proof sketch**:
```
Final_Key = KDF(State_n) where State_n depends on all State_i

State_i = KDF(State_{i-1} || shared_secret_i || RTT_i)

Therefore:
  State_n = KDF(
    KDF(State_{n-1} || secret_n || RTT_n)
  ) = KDF(
    KDF(... KDF(State_0 || secret_1 || RTT_1) ...) || secret_n || RTT_n
  )

To compute Final_Key, attacker must know:
  - secret_1 (requires breaking P₁) AND
  - secret_2 (requires breaking P₂) AND  
  - ... AND
  - secret_n (requires breaking Pₙ) AND
  - All RTT_i values (requires observing timing)

Probability of breaking all:
  P(break) = P(break P₁) × P(break P₂) × ... × P(break Pₙ)
  
With diverse primitives (lattice, code, isogeny, etc.):
  P(break all) << P(break weakest)
```

---

## Implementation Considerations

### Message Format

```rust
struct ClutchMessage {
    version: u8,
    message_type: MessageType,
    primitive_id: u8,
    sequence: u32,
    payload: Vec<u8>,
    hmac: [u8; 32],
}

enum MessageType {
    Hello,
    HelloAck,
    Exchange,
    ExchangeResponse,
    ExchangeInvalid,
    Confirm,
    Commit,
    CommitAck,
}
```

### State Machine

```rust
enum ClutchState {
    Initial,
    HelloSent { nonce: [u8; 32] },
    HelloReceived { agreed_primitives: Vec<PrimitiveId> },
    ExchangeInProgress { 
        current_primitive: usize,
        state: [u8; 32],
        rtts: Vec<Duration>,
        successes: BitSet,
    },
    ValidationPending,
    Complete { final_key: [u8; 32] },
    Failed { reason: String },
}
```

### Error Handling

```rust
enum ClutchError {
    UnsupportedPrimitive(PrimitiveId),
    InvalidKeyMaterial { primitive: PrimitiveId, reason: String },
    TimingAnomaly { expected_rtt: Duration, actual_rtt: Duration },
    InsufficientSuccesses { required: usize, actual: usize },
    HmacVerificationFailed,
    TimeoutExceeded,
}
```

---

## Timing Details: The Missing Piece

### Timing Windows

```
For each exchange i, measure:
  T_send = timestamp of sending request
  T_recv = timestamp of receiving response  
  RTT_i = T_recv - T_send
  
  Processing_time = responder's reported processing duration
  Network_latency = RTT_i - Processing_time
```

### Timing Anomaly Detection

```rust
struct TimingProfile {
    rtts: Vec<Duration>,
    mean: Duration,
    stddev: Duration,
    min: Duration,
    max: Duration,
}

impl TimingProfile {
    fn is_anomalous(&self, rtt: Duration) -> bool {
        let z_score = (rtt - self.mean).abs() / self.stddev;
        z_score > 3.0  // More than 3 standard deviations
    }
    
    fn update(&mut self, rtt: Duration) {
        self.rtts.push(rtt);
        self.recalculate_stats();
    }
}
```

### Timing as Entropy

```
Timing provides ~10-12 bits of entropy per exchange:
  - RTT measured in microseconds: 1μs precision
  - Typical internet RTT: 1ms - 300ms = 10^3 to 10^6 microseconds
  - log₂(10^6) ≈ 20 bits theoretical
  - But predictable within ~100μs due to network consistency
  - Effective entropy: ~10-12 bits

With 6 exchanges:
  Total timing entropy: 60-72 bits
  
Not sufficient alone, but excellent supplement to cryptographic material.
```

### Timing-Based Session Binding

```
Session_ID = hash(
    Final_Key ||
    Timing_fingerprint ||
    Timestamp
)

Timing_fingerprint = hash(
    RTT₁ || RTT₂ || ... || RTTₙ ||
    Sequence_of_primitives
)

This binds the session to:
  - The cryptographic material (Final_Key)
  - The physical network path (RTT values)
  - The temporal moment (Timestamp)
  
Replay from different location/time fails verification.
```

---

## Example Session

```
Initiator (You) ←→ Responder (Alice)

[Phase 0: Initial]
You → Alice: HELLO
  primitives: [ECDH, Kyber, McEliece, CSIDH, Spirix, SPHINCS+]
  nonce: 0xdeadbeef...
  
Alice → You: HELLO_ACK  
  selected: [ECDH, Kyber, CSIDH, Spirix] // Alice doesn't support McEliece
  nonce: 0xcafebabe...

[Phase 1: Exchanges]

Exchange 1 (ECDH) - T=0ms:
  You → Alice: X25519 public key
  RTT₁ = 45ms
  ✓ Success → State₁

Exchange 2 (Kyber) - T=50ms:
  You → Alice: Kyber public key (encrypted under State₁)
  RTT₂ = 48ms  
  ✓ Success → State₂

Exchange 3 (CSIDH) - T=105ms:
  You → Alice: CSIDH public key (encrypted under State₂)
  RTT₃ = 52ms
  ✓ Success → State₃

Exchange 4 (Spirix) - T=165ms:
  You → Alice: Spirix key in custom encoding (encrypted under State₃)
  [Alice validates: checks Spirix format, non-zero, reasonable range]
  RTT₄ = 47ms
  ✓ Success → State₄

[Phase 2: Finalization]

Final_Key = KDF(
  State₄ ||
  Success[ECDH, Kyber, CSIDH, Spirix] ||
  RTT[45ms, 48ms, 52ms, 47ms]
)

You → Alice: COMMIT
  HMAC(Final_Key, "INITIATOR_COMMIT")
  
Alice → You: COMMIT_ACK
  HMAC(Final_Key, "RESPONDER_COMMIT")

✓ CLUTCH Complete
Total time: ~220ms
Session established with 4 primitives
```

---

## TOKEN Integration

### Use in TOKEN Identity System

```
Token_Handshake = CLUTCH(
  initiator: Token_ID_A,
  responder: Token_ID_B,
  primitives: [TOKEN_STANDARD_SET],
  mode: AdaptiveCLUTCH,
  security_level: based_on_transaction_value
)

Derived keys used for:
  - Message encryption
  - Transaction signing  
  - Channel authentication
  - Subsequent CLUTCH seed material
```

### TOKEN Standard Primitive Set

```
TOKEN_CLUTCH_V1 = {
  ECDH (X25519),
  ML-KEM-768 (Kyber),
  CSIDH-512,
  Spirix-256,
  SPHINCS+-128s (signatures),
}

Minimum threshold: 3 of 5
Target time: <500ms
```

---

## Open Questions & Future Work

1. **Optimal primitive ordering**: Which order minimizes latency while maximizing security?

2. **Quantum resistance proof**: Formal security proof against quantum adversaries

3. **Timing oracle attacks**: Can RTT measurements leak information about key material?

4. **Decoy effectiveness**: How many decoy exchanges needed to meaningfully impact traffic analysis?

5. **Mobile/NAT scenarios**: How does CLUTCH behave through NAT, cellular handoffs, VPNs?

6. **Rekeying protocol**: How to efficiently re-key using previous CLUTCH state?

7. **Group CLUTCH**: Extending to N-party key exchange

---

## Conclusion

CLUTCH provides cryptographic diversity through multiple independent primitives, each based on different hard problems. Combined with timing-based authentication and optional obfuscation, it offers defense-in-depth against both current and future cryptographic attacks.

The protocol is practical (works over standard TCP/IP), flexible (multiple variants for different use cases), and robust (gracefully handles failed exchanges while maintaining security).

---

**Status**: Draft specification v0.1
**Author**: Nick Spiker
**Date**: 2025-11-10
**License**: None?

---