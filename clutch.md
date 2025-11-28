# CLUTCH Protocol Specification v1.0

## Cryptographic Layered Universal Trust Commitment Handshake

**Author:** Nick Spiker  
**Status:** Draft  
**License:** MIT OR Apache-2.0  
**Date:** November 2025

---

## 1. Abstract

CLUTCH is a one-time key generation ceremony that combines multiple independent cryptographic primitives across diverse mathematical foundations, implementations, and origins into a single shared seed. This seed bootstraps a rolling-chain encrypted relationship between two parties.

CLUTCH is not a handshake protocol. It is a **key generation ceremony** performed once per relationship. All subsequent communication is authenticated by the rolling chain itself—successful decryption *is* authentication.

---

## 2. Design Philosophy

### 2.1 Defense in Parallel

Traditional cryptographic diversity uses fallback schemes—if one breaks, switch to another. CLUTCH instead **combines all schemes simultaneously**. An attacker must break every primitive to derive the shared seed. If any single primitive holds, the seed remains secure.

### 2.2 Pre-Shared Secret Integration

Both parties know each other's handles before the ceremony. Handles are never transmitted over the wire. The handles themselves become a pre-shared secret component mixed into the seed derivation, creating a dependency that cannot be satisfied by cryptanalysis alone.

### 2.3 Self-Authenticating Communication

After CLUTCH completes, no further handshakes or identity proofs are required. The rolling chain state is known only to the two participants. Successful decryption proves possession of the chain state, which proves continuous participation since the ceremony.

---

## 3. Cryptographic Primitives

CLUTCH employs eight key exchange primitives spanning four mathematical families, multiple structural approaches, and diverse origins.

### 3.1 Classical Elliptic Curve (3 primitives)

| Primitive | Curve | Field | Origin | Public Key | Shared Secret |
|-----------|-------|-------|--------|------------|---------------|
| X25519 | Curve25519 | Montgomery | djb (2006) | 32 B | 32 B |
| ECDH-P384 | P-384 | Weierstrass | NIST/NSA (2000) | 97 B | 48 B |
| ECDH-secp256k1 | secp256k1 | Koblitz | Certicom (2000) | 33 B | 32 B |

**Rationale:** Three curves with different constants, field representations, and origins. Provides coverage against curve-specific attacks, implementation bugs in any single curve, and potential undisclosed weaknesses in NIST parameters.

### 3.2 Structured Lattice (2 primitives)

| Primitive | Problem | Ring Structure | Origin | Public Key | Shared Secret |
|-----------|---------|----------------|--------|------------|---------------|
| ML-KEM-1024 | Module-LWE | Polynomial ring | IBM/European consortium (2017) | 1,568 B | 32 B |
| NTRU-HPS-4096-821 | NTRU | NTRU ring | Hoffstein/Pipher/Silverman (1996) | 1,230 B | 32 B |

**Rationale:** Both are lattice-based but use fundamentally different ring constructions and security reductions. ML-KEM is NIST standardized (FIPS 203). NTRU predates it by two decades and uses a different problem formulation.

### 3.3 Unstructured Lattice (1 primitive)

| Primitive | Problem | Structure | Origin | Public Key | Shared Secret |
|-----------|---------|-----------|--------|------------|---------------|
| FrodoKEM-976 | Plain LWE | None | Microsoft Research (2016) | 15,632 B | 24 B |

**Rationale:** No ring structure to exploit. If structured lattice assumptions fall due to ring-specific attacks, unstructured LWE provides a fallback within the lattice family.

### 3.4 Code-Based (2 primitives)

| Primitive | Problem | Code Type | Origin | Public Key | Shared Secret |
|-----------|---------|-----------|--------|------------|---------------|
| HQC-256 | Syndrome decoding | Quasi-cyclic | French academics (2017) | 7,245 B | 64 B |
| Classic McEliece-460896 | Syndrome decoding | Binary Goppa | Bernstein/Lange, after McEliece (1978) | 524,160 B | 32 B |

**Rationale:** Code-based cryptography relies on entirely different mathematics than lattice schemes. McEliece has withstood 47 years of cryptanalysis. HQC uses different code structure (quasi-cyclic vs. Goppa), providing diversity within the code-based family.

### 3.5 Summary

| Family | Primitives | Combined Public Key Size |
|--------|------------|--------------------------|
| Classical ECC | 3 | 162 B |
| Structured Lattice | 2 | 2,798 B |
| Unstructured Lattice | 1 | 15,632 B |
| Code-Based | 2 | 531,405 B |
| **Total** | **8** | **~550 KB** |

---

## 4. Handle Structure

A handle is a human-readable identifier cryptographically bound to a public key bundle containing all eight primitives. Handles are encoded using VSF (Versatile Storage Format).

### 4.2 Handle Properties

- **Human-readable:** Users reference handles by name (e.g., `fractal decoder`)
- **Self-authenticating:** Handle name is cryptographically bound to key material
- **Never transmitted:** Handles are exchanged out-of-band only, NEVER over the wire
- **Canonical encoding:** Handle proof is computed and used for initiating communication between clients

### 4.3 Handle Secret

The handle secret is the seed from which all keypairs are derived. It is:
- **Never stored** on any device
- **Never transmitted** over any wire
- Regenerated from user memory or social recovery when needed

The public handle proof is derived deterministically from the handle secret.

---

## 5. CLUTCH Ceremony

### 5.1 Prerequisites

Before CLUTCH can occur:
1. Alice knows Bob's handle (obtained out-of-band)
2. Bob knows Alice's handle (obtained out-of-band)
3. Both parties possess their own client secrets
4. Obviously they know their own handle

### 5.2 Ceremony Initiation

No network communication is required to initiate. Each party independently:
1. Enters the other party's human-readable handle name
2. System resolves name to full handle (from local storage)
3. System computes shared seed locally

### 5.3 Shared Seed Derivation

Both parties compute identical shared seeds using the following algorithm:

```
function derive_clutch_seed(my_handle, my_secrets, their_handle):
    
    // Canonicalize handle ordering (lexicographic by public key hash)
    handles_ordered = sort_by_hash([my_handle, their_handle])
    handle_component = handles_ordered[0] || handles_ordered[1]
    
    // Perform all key exchanges over the wire via VSF
    // Combine all components
    shared_seed = BLAKE3(
        handle_component ||
        x25519_shared ||
        p384_shared ||
        secp256k1_shared ||
        ml_kem_shared ||
        ntru_shared ||
        frodo_shared ||
        hqc_shared ||
        mceliece_shared
    )
    
    return shared_seed
```

### 5.5 Chain Initialization

Upon successful seed derivation:

```
state_0 = BLAKE3(shared_seed || "CLUTCH_v1_chain_init")
```

The rolling chain is now active. CLUTCH ceremony is complete.

---

## 6. Rolling Chain Integration

TBD

### 6.4 Chain Advancement

State advances **only after confirmed receipt by decrypt and matching proof inside encrypted message**:

```
function advance_chain(current_state, plaintext):
    plaintext_hash = BLAKE3(plaintext||BLAKE3(chain count||fixed salt))
    new_state = BLAKE3(current_state || plaintext_hash)
    return new_state
```

Advancement sequence:
1. Alice encrypts with `state_sending`, sends to Bob
2. Bob decrypts with `state_receiving`, verifies signature
3. Bob sends acknowledgment (or reply)
4. Alice receives acknowledgment
5. **Each party advances their respective states once they know the other party has the state**

### 6.5 Authentication Model

- **If decryption succeeds** and **decrypted proof matches**: Sender possesses correct chain state and signing key, therefore sender is the original ceremony participant
- **If decryption fails**: Wrong chain state, sender is not authenticated, message rejected

---

## 9. Attack Resistance

### 9.1 Requirements to Break One Friendship

An attacker must **simultaneously**:

| Requirement | Difficulty |
|-------------|------------|
| Obtain Alice's handle | Never transmitted, must compromise Alice or her social recovery circle or reverse handle proof |
| Obtain Bob's handle | Never transmitted, must compromise Bob or his social recovery circle or reverse handle proof|
| Break X25519 | Solve ECDLP on Curve25519 |
| Break ECDH-P384 | Solve ECDLP on P-384 |
| Break ECDH-secp256k1 | Solve ECDLP on secp256k1 |
| Break ML-KEM-1024 | Solve Module-LWE |
| Break NTRU-HPS-4096-821 | Solve NTRU problem |
| Break FrodoKEM-976 | Solve unstructured LWE |
| Break HQC-256 | Solve syndrome decoding (quasi-cyclic) |
| Break McEliece-460896 | Solve syndrome decoding (Goppa) — unbroken since 1978 |

**All ten conditions must be satisfied.** Failure of any single condition preserves security.