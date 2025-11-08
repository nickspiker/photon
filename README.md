# Photon

**Decentralized messenger with passless authentication and rolling-chain encryption**

No servers. No passwords. No phone numbers. No corporate data harvesting.

## What This Is

Photon is a peer-to-peer messaging application that replaces traditional authentication (passwords, PINs, biometrics, recovery emails) with **social attestation**—your identity is verified by trusted humans, not by servers or credentials. Messages use **rolling-chain encryption**, where each message cryptographically depends on all previous messages, preventing replay attacks, reordering, and tampering while enforcing message immutability thru acknowledgment-based state advancement.

## Current Status: Early Development

**What works:**
- Cross-platform GUI (Windows, Linux)
- Text input, selection, editing
- Window management, rendering pipeline
- Handle attestation UI (mock DHT queries)
- Cryptographic type system (identities, seeds, shards)
- Rolling-chain encryption framework
- Android build pipeline (close to complete)

**What doesn't work yet:**
- Actual peer-to-peer messaging (1-2 months out)
- Network transport (DHT integration stubbed)
- Identity validation and key recovery
- Message persistence (database layer empty)
- Social attestation flow (design complete in [AUTH.md](AUTH.md), implementation pending)

**Platform support:**
- ✅ **Linux** (X11/Wayland)
- ✅ **Windows** (DirectDraw)
- ⚠️ **Android** (NDK bindings ready, testing in progress)
- 🟡 **macOS** (possible, not tested)
- ❌ **iOS** (unlikely—Apple's encryption restrictions incompatible with design)

## Distribution

We will not distribute via Google Play Store, Apple App Store, or Microsoft Store. These platforms create barriers incompatible with our security model and decentralized architecture. Ready-to-run builds are provided in `bin/`—installation instructions below.

**Why not use app stores?**

- **Encryption reporting requirements**: US Export Administration Regulations require registration with the Bureau of Industry and Security and disclosure of encryption implementation details before international distribution. We decline to notify the government about our cryptographic systems.

- **Review process uncertainty**: App stores can reject applications with strong encryption arbitrarily or demand explanations of security implementations that we consider proprietary.

- **Corporate intermediaries**: Distribution thru stores requires trusting corporations to maintain access to software we built. Yeahno.

- **Sideloading restrictions**: iOS requires annual re-signing (99USD/year developer account), and free accounts must reinstall every 7 days. Android allows direct APK installation but shows warnings designed to discourage it.

We provide cryptographically signed binaries with published checksums. Verify the signature, verify the source, run it yourself. That's the security model that actually works.

### Why No iOS?

Apple's iOS platform is **architecturally incompatible** with Photon's design:

**Distribution barriers:**
- App Store requires $99/year developer account (no anonymous distribution)
- Code signing mandatory (all binaries must be Apple-signed)
- Sideloading requires re-installation every 7 days (free accounts) or yearly (paid)
- Enterprise distribution violates terms if used publicly

**Technical limitations:**
- **No raw socket access** - Cannot connect to DHT peers directly
- **No background processes** - Apps terminated after ~30 seconds in background
- **Sandbox restrictions** - Cannot run persistent TOKEN daemon
- **Entitlement gatekeeping** - Network access requires Apple approval
- **No system-level services** - Architecture assumes apps are foreground-only

Photon requires long-running background connections, direct peer-to-peer networking, and system-level cryptographic services. iOS prohibits all of these - not because the hardware can't do it, but because Apple spent decades building walls specifically to prevent applications like Photon from existing.

**Could this change?** Unlikely. The EU's Digital Markets Act may force sideloading in Europe by 2026, but it won't be truly keyless and won't affect most of the world.

## Installation

### Quick Install (Recommended)

Download pre-built, cryptographically signed binaries:

**Linux/macOS:**
```bash
curl -sSfL https://holdmyoscilloscope.com/photon/install.sh | sh
```

**Windows (PowerShell):**
```powershell
iwr -useb https://holdmyoscilloscope.com/photon/install.ps1 | iex
```

These scripts will:
0. Download a pre-built, pre-signed binary from holdmyoscilloscope.com
1. Install to `~/.local/bin` (Linux/macOS) or `%LOCALAPPDATA%\Programs\PhotonMessenger` (Windows)
2. Create a desktop/Start Menu shortcut automatically
3. Add the binary to your PATH

**Security:** Every binary is signed with Ed25519 by Nick Spiker (fractaldecoder@proton.me) and self-verifies on startup. This protects against data corruption (bit flips, incomplete downloads, storage failures) and tampering. If verification fails, the binary won't run.

After installation, find **Photon Messenger** in your application menu (Start Menu on Windows, app launcher on Linux/macOS), or run from terminal:

```bash
photon-messenger
```

### Building from Source

**WARNING:** Building from source requires generating your own signing keys.

**Just use the installer above unless you know what you're doing.**

If you still want to build from source:

0. **Clone the repository**:
   ```bash
   git clone https://github.com/nickspiker/photon
   cd photon
   ```

1. **Generate signing keys**:
   ```bash
   # Edit src/bin/photon-keygen.rs to set your key path
   cargo run --bin photon-keygen
   ```

2. **Update the public key** in `src/self_verify.rs` with your generated key

3. **Build and sign**:
   ```bash
   cargo build --release
   ./sign-after-build.sh release
   ```

See [src/self_verify.rs](src/self_verify.rs) for complete signing documentation.

**Android:**
```bash
rustup target add aarch64-linux-android
cargo build --target aarch64-linux-android --release
./sign-after-build.sh release aarch64-linux-android
```

## How It Works

### Rolling-Chain Encryption

Traditional messaging (Signal, WhatsApp) uses **Double Ratchet**: sender advances keys immediately, receiver stores "skipped message keys" for out-of-order delivery. This allows asynchronous messaging but makes message ordering non-deterministic.

Photon uses **rolling-chain encryption**: the sender does **not** advance the chain state until receiving confirmation that the message was successfully received and decrypted. This creates a **synchronization loop**:

```
Eve (state₀)  ──message encrypted with state₀──→  Norman (state₀)
                                                     Norman decrypts, advances to state₁
Eve (state₀)  ←──────ACK or reply──────────────  Norman (state₁)
Eve advances to state₁
[Both now at state₁, loop complete]
```

**Why wait for acknowledgment?**

**0. Prevents desynchronization**: If a message is lost, both parties remain at the same state—no "skipped keys" needed

**1. Enforces ordering**: Messages must be processed sequentially (sequence numbers verified)

**2. Enables immutability**: Deleting or editing a message breaks all subsequent hashes—cryptographic proof of tampering

**3. Simplifies recovery**: New devices can replay message history from checkpoints without complex key management

**How the hash chain works:**

```rust
// Initial state from shared seed (exchanged out-of-band)
state₀ = BLAKE3(seed)

// Encrypt message
ciphertext = plaintext ⊕ state₀  // XOR with current state

// Advance ONLY after receiving ACK
stateᵢ = BLAKE3(stateᵢ₋₁ ‖ ciphertext)
```

Each message's ciphertext is hashed with the previous state to produce the next state. Breaking one message doesn't reveal others (forward secrecy via BLAKE3 preimage resistance). Modifying message `i` changes all subsequent states—tampering is cryptographically detectable.

**ACK timing and state branches:**

Messages include a `parent_sequence` field indicating which chain state was used for encryption. If the sender receives an ACK and advances the chain while messages are still in flight, the receiver can detect the branch:

```rust
// Eve sends msg₅, msg₆ (both with state₄)
// ACK for msg₅ arrives → Eve advances to state₅
// Eve sends msg₇ with parent_sequence=5 (encrypted with state₅)

// Norman receives msg₇ before msg₆
// Checks: parent_sequence=5, my last ACK'd=4
// Queues msg₇ until msg₅ arrives and is processed
```

This prevents race conditions where sender and receiver have different views of which state to use. Orphaned branches (messages encrypted with a state the receiver has moved past) are detected and trigger retransmission requests.

**Multiple messages in flight:**

Eve can send messages 100, 101, 102 all encrypted with `state₉₉` while waiting for ACKs. Once ACKs arrive, states advance in sequence order:

```
msg₁₀₀ sent with state₉₉
msg₁₀₁ sent with state₉₉ (still waiting)
msg₁₀₂ sent with state₉₉ (still waiting)

ACK for msg₁₀₀ → state₉₉ → state₁₀₀
ACK for msg₁₀₁ → state₁₀₀ → state₁₀₁
ACK for msg₁₀₂ → state₁₀₁ → state₁₀₂
```

This is secure because:
- Each message has unique nonce (64-bit random)
- Each message has unique timestamp (128-bit microsecond precision)
- Combined serialized plaintext is always unique
- State advances once ACK received, limiting exposure window
- Known-plaintext attack on one message doesn't reveal past/future states (BLAKE3 preimage resistance)

**Traditional encryption on top:**

The XOR with chain state is not the only encryption layer. Final messages use **ChaCha20-Poly1305 AEAD** with keys derived from chain state:

```rust
encryption_key = BLAKE3_KDF(chain_state, "photon.encryption.v1")
encrypted_message = ChaCha20Poly1305::encrypt(key, nonce, plaintext)
```

Rolling-chain provides immutability and ordering guarantees; ChaCha20-Poly1305 provides standard cryptographic security.

**Trade-offs:**

| Double Ratchet (Signal) | Rolling-Chain (Photon) |
|------------------------|------------------------|
| Async: sender advances immediately | Sync: sender waits for ACK |
| Out-of-order delivery supported | Sequential processing enforced |
| Skipped message keys stored | No skipped keys |
| Message deletion undetectable | Deletion breaks chain (detectable) |
| Message editing undetectable | Editing breaks chain (detectable) |

### Passless Authentication

No passwords. No PINs. No biometrics unlocking a password. No "passkeys" that are passwords with extra steps.

**Authentication count: A = 1**

You authenticate **once** when creating your identity. All subsequent access uses cryptographic proofs derived from that single authentication event. New devices receive identity thru:

**0. Proximity transfer**: Authorized device transfers identity to new device via Bluetooth LE with 3-word visual verification (or manual entry if BLE unavailable)

**1. Social recovery**: Lose all devices? Trusted contacts hold encrypted shards of your private key—threshold reconstruction (typically 5 friends required)

**Handle attestation:**

Identity is tied to your handle. Handles can be **any Unicode string of any length** including zero (e.g., `fractaldecoder`, `🚀`, `∫∂x`, or even empty string `""` if unclaimed). The handle is hashed with BLAKE3 to derive a unique network address—if it can be hashed, it's valid. Claiming a handle requires **two human attestations**—existing users vouch for your identity. This is invite-only by design. No bots, no spam, no anonymous harassment.

Attestation flow (see [AUTH.md](AUTH.md) for full specification):

**0. User requests handle** (e.g., `Wayne`)

**1. System queries DHT**: is `Wayne` already claimed?

**2. If unclaimed**, user requests attestations from 2 trusted people

**3. Attesters see**: device type, approximate location, timestamp

**4. Attesters verify out-of-band** (phone call, video, in-person)

**5. Both attestations required** within 24 hours

**6. Handle bound** to user's public key cryptographically

**Why attestation?**

Password reset flows prove identity doesn't exist—anyone with access to your email/phone can take your account. Photon uses the security model humans have used for millennia: **trusted relationships**. Your friends vouch for you. They hold shards of your identity. They verify recovery requests. No company, no customer support, no "click this link to reset."

See [AUTH.md](AUTH.md) for detailed specification (1,350 lines covering attestation, reputation, social recovery, verification phrases, rate limiting, attack resistance).

### Network Architecture

**Peer discovery:** Mainline DHT (BitTorrent's distributed hash table)—handles are resolved like magnet links, all traffic looks like torrenting to network observers.

**Transport:** TLS 1.3 over TCP + WebSocket upgrade—encrypted connections look like HTTPS web traffic.

**Message routing:**
- Small messages (<1KB, no expiration): Stored across your social graph with fractal gradient mesh distribution (closer friends store more copies)
- Large messages (>1KB): Direct peer-to-peer transfer
- Ephemeral messages (expiration <7 days): Direct-only, auto-deleted

**No servers.** Messages persist as long as **you** want them to, distributed across devices you control and friends who've agreed to store encrypted backups. No company can read, delete, or subpoena your messages—they don't have them.

## Architecture

### Module Structure

```
src/
├── main.rs              - Winit event loop, window management
├── lib.rs               - Module exports, debug utilities
├── crypto/
│   ├── chain.rs         - Rolling-chain encryption (IMPLEMENTED)
│   ├── keys.rs          - Identity key management (TODO)
│   └── shards.rs        - Social recovery key sharding (TODO)
├── network/
│   └── handle_query.rs  - DHT attestation status queries (STUBBED)
├── ui/
│   ├── app.rs           - Application state machine
│   ├── text_rasterizing.rs - Font rendering (cosmic-text)
│   ├── renderer_linux.rs   - X11/Wayland rendering
│   ├── renderer_windows.rs - DirectDraw rendering
│   ├── keyboard.rs      - Input handling
│   ├── mouse.rs         - Mouse tracking
│   ├── text_editing.rs  - Text input state
│   ├── theme.rs         - Color palette
│   └── compositing.rs   - Layer blending
├── types/
│   ├── identity.rs      - Public/private keys (X25519)
│   ├── message.rs       - Message structure, status, expiration
│   ├── contact.rs       - Contact info, trust levels
│   ├── peer.rs          - Network peer info
│   ├── seed.rs          - Cryptographic seed generation
│   └── shard.rs         - Key shard structures
├── storage/             - SQLite integration (EMPTY - TODO)
├── logic/               - Business logic orchestration (EMPTY - TODO)
└── platform/            - Platform-specific code (EMPTY)
```

### What's Implemented

| Component | Status | Notes |
|-----------|--------|-------|
| UI Framework | ✅ Complete | Custom winit-based GUI, differential rendering |
| Text Rendering | ✅ Complete | cosmic-text with multiple fonts, selection, editing |
| Window Management | ✅ Complete | Resize, maximize, fullscreen, transparency |
| Input Handling | ✅ Complete | Keyboard, mouse, clipboard integration |
| Crypto Types | ✅ Complete | Identity, seed, shard, message structures |
| Rolling-Chain Crypto | ⚠️ Framework | Core logic implemented, needs ChaCha20 integration |
| Handle Attestation UI | ⚠️ Mock | UI complete, DHT queries simulated (8s delay) |
| Network Transport | ❌ Stubbed | DHT/TLS/WebSocket integration pending |
| Message Persistence | ❌ Empty | SQLite schema and storage layer not implemented |
| Social Recovery | ❌ Stubbed | Shard distribution/reconstruction TODO |
| Peer Messaging | ❌ Not Started | End-to-end message flow not implemented |

### Technology Stack

**Core:**
- `winit` - Cross-platform windowing
- `softbuffer` - Software rendering
- `cosmic-text` - Font rendering and text layout
- `arboard` - Clipboard access

**Crypto:**
- `blake3` - Cryptographic hashing (chain state, KDF)
- `chacha20poly1305` - AEAD encryption
- `x25519-dalek` - Elliptic curve Diffie-Hellman
- `zeroize` - Secure memory wiping

**Network:**
- `tokio` - Async runtime
- `mainline` - DHT client
- `tokio-tungstenite` - WebSocket
- `tokio-native-tls` - TLS 1.3

**Storage:**
- `rusqlite` - SQLite with bundled library
- `bincode` - Binary serialization

**Build:**
- LTO enabled (release)
- Symbols stripped
- Parallel codegen (dev)
- `opt-level=2` (dev for fast iteration)

## File Format: VSF (Versatile Storage Format)

Messages and identity data use **VSF**—a self-describing binary format with cryptographic integrity. Implementation is functional but needs updates for messaging (1-2 months estimated).

VSF provides:
- Type-length-value encoding
- Embedded schemas
- Versioning and forward compatibility
- Cryptographic signatures per record
- Compression (optional)

See `tools/` directory for VSF utilities (format inspection, validation).

## Design Philosophy

Drawn from [AGENT.md](https://github.com/yourusername/photon/blob/main/AGENT.md):

**0. Trust the math**: If loop bounds guarantee safety, don't add runtime checks

**1. Fail fast, fail loud**: Panics expose bugs; bounds checks hide them

**2. No fixed pixels**: Everything scales relative to screen dimensions (`min_dim`, `perimeter`, `diagonal_sq`)

**3. Explicit over "safe"**: `pixels[idx] = color` not `pixels.get_mut(idx).map(...)`

**4. Bounds checks require proof**: State why, prove necessity, explain what undefined behavior is prevented

If you add a bounds check or saturating arithmetic without justification, expect rejection. See [AGENT.md](AGENT.md) for full rules.

## Contributing

Photon is in early development. Contributions welcome, but read the architecture docs first:

- [AUTH.md](AUTH.md) - Attestation system specification (1,350 lines)
- [AGENT.md](AGENT.md) - Code generation rules (bounds checks, scaling, philosophy)
- This README - Architecture and current status

**High-priority areas:**

**0. Network transport** (DHT queries, peer connections, WebSocket)

**1. Message persistence** (SQLite schema, storage layer)

**2. Social recovery** (shard distribution, threshold reconstruction)

**3. Android testing** (NDK integration works, needs real-device testing)

**4. ChaCha20-Poly1305 integration** (replace XOR placeholder in rolling-chain)

**Testing:**
```bash
cargo test
cargo bench  # Crypto benchmarks
```

No tests currently—this is early-stage development. Write tests for any new crypto code.

## Why Not Use Signal/WhatsApp/Matrix?

| Property | Signal | WhatsApp | Telegram | Matrix | Photon |
|----------|--------|----------|----------|--------|--------|
| Authentication count | >1 | >1 | >1 | >1 | **1** |
| Decentralized | No | No | No | Yes | Yes |
| Social recovery | No | No | No | No | **Yes** |
| Metadata privacy | Partial | No | No | Partial | **Yes** |
| Self-sovereign data | No | No | No | Partial | **Yes** |
| Message immutability | No | No | No | No | **Yes** |
| Phone number required | Yes | Yes | Yes | No | No |
| Can be shut down | Yes | Yes | Yes | No | No |

**Signal/WhatsApp:**
- Centralized servers (can be subpoenaed, shut down)
- Phone number required (ties identity to carrier)
- No social recovery (lose device → lose account)
- Message deletion undetectable (sender can unsend)

**Matrix:**
- Federation, not true P2P (still relies on homeservers)
- Authentication per device/homeserver
- No social key recovery
- Metadata leaked to homeserver operators

**Photon:**
- True P2P (no servers, just DHT peer discovery)
- Passless authentication (A = 1)
- Social recovery (friends hold key shards)
- Message immutability (rolling-chain makes tampering detectable)
- Metadata privacy (traffic looks like BitTorrent + HTTPS)

## Security Properties

**Guaranteed by rolling-chain encryption:**

**0. Forward secrecy** (compromising `stateₙ` doesn't reveal `stateₙ₋₁`)

**1. Replay resistance** (sequence numbers prevent replayed messages)

**2. Reorder detection** (out-of-order messages fail sequence validation)

**3. Tamper evidence** (modifying message `i` breaks all subsequent hashes)

**4. Message immutability** (deletion/editing cryptographically detectable)

**Guaranteed by attestation system:**

**0. Human identity verification** (bots can't pass two attestations)

**1. Rate limiting** (1 attestation request per hour per device)

**2. Collusion detection** (both attesters see each other's approval)

**3. Reputation staking** (attesters risk reputation if they vouch for abusers)

**Not protected against:**
- Physical device theft (if unlocked)
- `k` or more friends colluding (threshold is your security boundary)
- Screenshots or cameras pointed at screen
- Quantum computers breaking X25519 (future threat—would require protocol upgrade)

## Related Projects

Photon is the first implementation of **TOKEN**—a universal digital identity system where authentication happens once. Other TOKEN applications (planned):

- `ferros` - Kill-switch ready OS

Once you authenticate with TOKEN (A = 1), all applications use that single identity. Install TOKEN on new device → everything appears automatically (apps, settings, messages, contacts, files). No passwords, no setup wizards, no per-app logins.

Photon proves the social attestation and recovery model works before applying it to high-stakes use cases (property deeds, medical records, financial assets).

## License

MIT License - See [LICENSE](LICENSE)

## Contact

**Nick Spiker** - fractaldecoder@proton.me

---

**Project Status:** 🟡 Early Development (GUI functional, messaging implementation 1-2 months out)

**Platform Support:** Linux ✅ | Windows ✅ | Android ⚠️ | macOS 🟡 | iOS ❌🟥❌

**Last Updated:** 2025-11-04
