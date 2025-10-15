```markdown
        })?
        .collect::<Result<Vec<_>, _>>()?;
        
        Ok(messages)
    }
    
    pub fn delete_expired_messages(&mut self) -> Result<usize> {
        let now = current_timestamp();
        let deleted = self.db.execute(
            "DELETE FROM messages WHERE expiration IS NOT NULL AND expiration < ?",
            params![now],
        )?;
        Ok(deleted)
    }
    
    pub fn update_message_status(&mut self, id: &MessageId, status: MessageStatus) -> Result<()> {
        self.db.execute(
            "UPDATE messages SET status = ? WHERE id = ?",
            params![status as i32, id.as_bytes()],
        )?;
        Ok(())
    }
}
```

---

## Development Roadmap

### Phase 1: Foundation (Weeks 1-3)

**Goal**: Core cryptography working in isolation

```
Week 1: Project Setup
├─ Create Cargo workspace
├─ Set up directory structure
├─ Add dependencies (iced, blake3, chacha20poly1305, etc.)
├─ Write types/ module (Message, Contact, Identity structs)
└─ Basic tests compile

Week 2: Rolling Chain Implementation
├─ Implement MessageChain struct
├─ encrypt() and decrypt() methods
├─ State advancement logic
├─ Read receipt tracking
├─ Unit tests proving correctness
└─ Test vectors from whitepaper

Week 3: Key Management
├─ Identity generation (X25519 keypair)
├─ Secure storage (encrypted at rest)
├─ Key shard generation logic
├─ Shard distribution algorithm
└─ Recovery reconstruction algorithm
```

**Success Criteria**:
- Can encrypt/decrypt message sequence correctly
- Chain state advances properly
- Replay/reorder attacks detected
- Key reconstruction from shards works
- All tests pass

**Deliverables**:
- `crypto/` module fully implemented
- 100% test coverage on crypto code
- No UI yet (pure library)

---

### Phase 2: UI Skeleton (Weeks 4-5)

**Goal**: Iced app displays messages (no network)

```
Week 4: Basic Iced App
├─ main.rs initializes Iced Application
├─ App struct with dummy state
├─ Contact list view (hardcoded contacts)
├─ Chat view (hardcoded messages)
├─ Text input and send button
└─ Navigation between views works

Week 5: Custom Widgets
├─ Message bubble widget
├─ Phase indicator (rotating circles)
├─ Hash display (rolling digits)
├─ Contact avatar
└─ Styling/theme
```

**Success Criteria**:
- App launches and displays UI
- Can type in input field
- Can click send button (no-op for now)
- Custom widgets render correctly
- UI is responsive (no lag)

**Deliverables**:
- `ui/` module with views and widgets
- App compiles and runs on Linux
- Screenshots of UI

---

### Phase 3: Network Layer (Weeks 6-9)

**Goal**: Two instances can send messages over network

```
Week 6: DHT Integration
├─ Integrate mainline-dht crate
├─ Compute InfoHash from public key
├─ Announce presence to DHT
├─ Query DHT for peers
└─ Test peer discovery on local network

Week 7: Transport Layer
├─ TLS connection establishment
├─ WebSocket upgrade
├─ Peer identity verification
├─ Connection management (reconnect on drop)
└─ Keepalive/heartbeat

Week 8: Protocol Implementation
├─ Frame encoding/decoding
├─ Message transmission
├─ ACK handling
├─ Error handling (timeout, retry)
└─ Network event loop

Week 9: Integration
├─ Wire network layer to crypto layer
├─ Network thread communicates with UI via channels
├─ End-to-end message flow works
└─ Two instances on same machine can chat
```

**Success Criteria**:
- DHT peer discovery works
- TLS connections establish successfully
- Messages transmit and decrypt correctly
- ACKs received and chain advances
- Can run two tmessage instances and chat between them

**Deliverables**:
- `network/` module fully functional
- Demo video: two terminals chatting
- Network protocol documented

---

### Phase 4: Full Integration (Weeks 10-11)

**Goal**: Complete app with persistence and recovery

```
Week 10: Storage Layer
├─ SQLite database setup
├─ Message persistence
├─ Contact storage
├─ Settings storage
├─ Identity storage (encrypted)
└─ Migration system

Week 11: Polish
├─ Message expiration (auto-delete)
├─ Notification system
├─ Online/offline status detection
├─ Error messages in UI
├─ Loading states
└─ Bug fixes
```

**Success Criteria**:
- Messages persist across restarts
- Contacts saved to database
- App handles network failures gracefully
- UI shows appropriate loading/error states
- No crashes, no data loss

**Deliverables**:
- `storage/` module complete
- App is usable for daily messaging
- Documentation for users

---

### Phase 5: Social Recovery (Weeks 12-14)

**Goal**: Key shard distribution and recovery working

```
Week 12: Shard Distribution
├─ Automatic weight calculation
├─ Shard generation from private key
├─ Encrypt and send shards to contacts
├─ Store received shards
└─ Update weights over time

Week 13: Recovery Protocol
├─ Recovery initiation flow
├─ Custodian notification
├─ Verification phrase generation
├─ Out-of-band verification UI
├─ Threshold validation
└─ Key reconstruction

Week 14: Recovery Testing
├─ Test full recovery flow
├─ Edge cases (offline custodians, insufficient shards)
├─ Security testing (rate limiting, attack scenarios)
└─ UI polish for recovery screens
```

**Success Criteria**:
- Shards automatically distributed to contacts
- Can recover identity by entering phrases from friends
- Threshold logic works correctly
- Attack scenarios are blocked (rate limits, etc.)

**Deliverables**:
- Social recovery fully functional
- Recovery documentation for users
- Demo video of recovery process

---

### Phase 6: Android Port (Weeks 15-18)

**Goal**: tmessage running on Android phones

```
Week 15: Android Build Setup
├─ Cross-compilation to aarch64-linux-android
├─ JNI bridge from existing Android code
├─ Iced integration with Android NDK
├─ APK builds successfully
└─ App launches on emulator

Week 16: Platform Integration
├─ Touch input handling
├─ Android permissions (network, storage)
├─ Notifications (FCM alternative needed)
├─ Battery optimization exemption
└─ VpnService API for DHT (optional)

Week 17: Mobile UI Adjustments
├─ Responsive layouts for small screens
├─ Keyboard handling
├─ Orientation changes
├─ Pull-to-refresh
└─ Swipe gestures

Week 18: Testing and Polish
├─ Test on real Android devices
├─ Performance optimization (battery, memory)
├─ Play Store preparation (if desired)
└─ Bug fixes
```

**Success Criteria**:
- APK installs and runs on Android 8.0+
- Can send/receive messages over cellular and WiFi
- Battery life acceptable (not constantly draining)
- UI works on various screen sizes

**Deliverables**:
- Android APK
- Installation instructions
- Android-specific documentation

---

### Phase 7: Additional Platforms (Weeks 19+)

**Windows (Week 19-20)**:
- Should mostly work from Linux codebase
- Windows-specific installer
- Test on Windows 10/11

**macOS (Week 21-22)**:
- Should mostly work from Linux codebase
- macOS bundle creation
- Test on macOS 12+

**Ferros Integration (Ongoing)**:
- Port to Redox OS
- Test on Redox
- Integrate with Ferros kill-switch architecture

---

## Platform Support

### Tier 1: Desktop (Production Ready)

**Linux**
- Native development platform
- Full feature support
- Best performance
- Target: Ubuntu 22.04+, Fedora 40+, Arch

**Windows**
- Full P2P support
- Desktop integration
- Target: Windows 10/11

**macOS**
- Full P2P support
- Native UI (Iced Metal backend)
- Target: macOS 12+

### Tier 2: Mobile (Functional with Constraints)

**Android**
- Works best on WiFi
- Battery optimization challenges
- VpnService for full P2P (optional)
- Background limitations
- Target: Android 8.0+

### Tier 3: Future Consideration

**iOS**
- Apple's sandbox hostile to P2P
- DHT blocked by App Sandbox
- Would require relay servers (defeats design)
- Not recommended unless architecture changes

**Web/WASM**
- No raw socket access
- Can't run DHT node
- Would require centralized relay
- Not viable for decentralized design

### Ferros (Ultimate Target)

**Redox OS Integration**:
- Pure Rust OS (perfect fit)
- Orbital display server
- Native kill-switch support (0ms shutdown)
- Full control over network stack
- This is the long-term goal

---

## Getting Started

### Prerequisites

```bash
# Linux
sudo apt install build-essential pkg-config libssl-dev sqlite3

# Rust (latest stable)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# For Android (optional)
rustup target add aarch64-linux-android
```

### Project Setup

```bash
# Create project
cargo new tmessage --bin
cd tmessage

# Set up directory structure
mkdir -p src/{ui,logic,crypto,network,storage,types,platform}
mkdir -p src/ui/{views,widgets}
mkdir -p tests/{integration,unit}
mkdir -p docs assets benches

# Initialize git
git init
echo "target/" > .gitignore
echo "Cargo.lock" >> .gitignore

# Add dependencies to Cargo.toml
```

### Cargo.toml

```toml
[package]
name = "tmessage"
version = "0.1.0"
edition = "2021"
authors = ["Nick Spiker <fractaldecoder@proton.me>"]
license = "MIT"
description = "Decentralized messenger with rolling-chain encryption"

[dependencies]
# UI
iced = { version = "0.12", features = ["tokio", "advanced"] }

# Crypto
blake3 = "1.5"
chacha20poly1305 = "0.10"
x25519-dalek = "2.0"
rand = "0.8"
zeroize = { version = "1.7", features = ["derive"] }

# Network
tokio = { version = "1", features = ["full"] }
tokio-tungstenite = "0.21"
tokio-native-tls = "0.3"
mainline = "2.0"  # DHT

# Serialization
bincode = "1.3"
serde = { version = "1.0", features = ["derive"] }

# Storage
rusqlite = { version = "0.31", features = ["bundled"] }

# Utilities
thiserror = "1.0"
anyhow = "1.0"
log = "0.4"
env_logger = "0.11"

[target.'cfg(target_os = "android")'.dependencies]
jni = "0.21"
ndk = "0.8"
ndk-glue = "0.7"

[dev-dependencies]
criterion = "0.5"

[[bench]]
name = "crypto_bench"
harness = false

[profile.release]
lto = true
codegen-units = 1
opt-level = 3
strip = true
```

### First Milestone: Crypto Foundation

**Create types/message.rs**:

```rust
// src/types/message.rs

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub nonce: u64,
    pub sequence: u64,
    pub payload: Vec<u8>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedMessage {
    pub sequence: u64,
    pub ciphertext: Vec<u8>,
}

impl Message {
    pub fn new(sequence: u64, payload: Vec<u8>) -> Self {
        Self {
            nonce: rand::random(),
            sequence,
            payload,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }
}
```

**Create crypto/chain.rs** (see Core Systems section above)

**Write tests**:

```rust
// tests/integration/crypto_test.rs

use tmessage::crypto::chain::MessageChain;
use tmessage::types::seed::Seed;

#[test]
fn test_rolling_chain_encryption() {
    let seed = Seed::generate();
    
    let mut alice_chain = MessageChain::new(seed.clone());
    let mut bob_chain = MessageChain::new(seed);
    
    // Alice sends message to Bob
    let plaintext = b"Hello Bob!";
    let encrypted = alice_chain.encrypt(plaintext);
    
    // Bob receives and decrypts
    let decrypted = bob_chain.decrypt(&encrypted).unwrap();
    
    assert_eq!(decrypted.payload, plaintext);
    
    // Chain states should be identical after ACK
    alice_chain.receive_ack(encrypted.sequence);
    assert_eq!(alice_chain.state, bob_chain.state);
}

#[test]
fn test_replay_attack_prevented() {
    let seed = Seed::generate();
    
    let mut alice_chain = MessageChain::new(seed.clone());
    let mut bob_chain = MessageChain::new(seed);
    
    let encrypted = alice_chain.encrypt(b"Message 1");
    bob_chain.decrypt(&encrypted).unwrap();
    
    // Try to replay the same message
    let result = bob_chain.decrypt(&encrypted);
    assert!(result.is_err()); // Should fail (sequence mismatch)
}

#[test]
fn test_message_reordering_detected() {
    let seed = Seed::generate();
    
    let mut alice_chain = MessageChain::new(seed.clone());
    let mut bob_chain = MessageChain::new(seed);
    
    let msg1 = alice_chain.encrypt(b"First");
    let msg2 = alice_chain.encrypt(b"Second");
    
    // Bob receives messages out of order
    let result = bob_chain.decrypt(&msg2);
    assert!(result.is_err()); // Should fail (expects sequence 0, got 1)
}
```

**Run tests**:

```bash
cargo test
```

---

## Critical Decisions Log

This section tracks major architectural decisions made during development. Update as you go.

### Decision 1: DHT Implementation

**Date**: 2025-01-XX

**Question**: Which DHT implementation to use?

**Options Considered**:
1. `mainline` crate (pure Rust, Mainline DHT)
2. `libp2p-kad` (feature-rich but heavier)
3. Custom implementation

**Decision**: Use `mainline` crate

**Rationale**:
- Mainline DHT has millions of existing nodes (BitTorrent)
- Pure Rust (no FFI)
- Proven at scale
- Simplest integration
- Already looks like torrent traffic (camouflage goal)

**Consequences**:
- Tied to BitTorrent DHT network
- Limited to Kademlia routing
- No built-in NAT traversal (need separate solution)

---

### Decision 2: Message Storage

**Date**: 2025-01-XX

**Question**: How to persist messages?

**Options Considered**:
1. SQLite database (encrypted)
2. Custom binary format (like Signal)
3. Plain files per conversation
4. In-memory only (no persistence)

**Decision**: SQLite with encryption at rest

**Rationale**:
- SQL queries useful for features (search, filters)
- Well-tested, reliable
- Cross-platform
- Easy backup/export
- Encryption via SQLCipher or custom wrapper

**Consequences**:
- Need database migrations
- Larger binary size
- Potential performance bottleneck (mitigated by indexes)

---

### Decision 3: Notification System

**Date**: 2025-01-XX

**Question**: How to notify users of new messages when app is in background?

**Options Considered**:
1. OS-level notifications (Linux: libnotify, Android: local notifications)
2. No background support (app must be open)
3. Persistent background service

**Decision**: OS-level notifications with persistent background service

**Rationale**:
- Users expect message apps to work in background
- Linux: systemd user service or background thread
- Android: Foreground Service (required for persistent network)
- No push servers needed (decentralized design)

**Consequences**:
- Battery impact on mobile
- Android: Need foreground notification
- May conflict with battery optimization
- Users must whitelist app

---

### Decision 4: Seed Exchange Method

**Date**: 2025-01-XX

**Question**: How do users exchange relationship seeds?

**Options Considered**:
1. QR code only (in-person)
2. Manual text entry (copy-paste)
3. NFC tap (Android)
4. All of the above

**Decision**: All of the above (QR primary, text fallback, NFC bonus)

**Rationale**:
- QR codes work for most users (easy, visual)
- Text entry for remote setup (Signal safety number pattern)
- NFC for Android users (fast, cool factor)
- More options = better UX

**Consequences**:
- Need QR code generation/scanning (use `qrcode` crate)
- Need NFC integration (Android NDK)
- More UI complexity

---

### Decision 5: Key Derivation Function

**Date**: 2025-01-XX

**Question**: How to derive keys from master secret?

**Options Considered**:
1. BLAKE3 KDF
2. Argon2
3. HKDF-SHA256

**Decision**: BLAKE3 for chain state, Argon2 for password-based (if needed)

**Rationale**:
- BLAKE3 is fast, secure, already used for chain
- Argon2 only needed if we add optional password layer
- Consistency with existing crypto choices

**Consequences**:
- Not using "standard" KDF (HKDF)
- BLAKE3 is newer, less reviewed (but Signal uses it)
- Need clear documentation of key derivation

---

### Decision 6: UI Framework

**Date**: 2025-01-XX (Already decided in conversation)

**Question**: Which UI framework?

**Options Considered**:
1. Iced (declarative, pure Rust)
2. egui (immediate mode)
3. GTK via gtk-rs
4. Qt via CXX
5. Flutter (Dart)

**Decision**: Iced

**Rationale**:
- Pure Rust (no FFI)
- Declarative (easy to reason about)
- GPU-accelerated (smooth animations)
- Cross-platform (desktop + mobile)
- COSMIC uses it (proven for chat-like apps)
- Matches project's Rust-first philosophy

**Consequences**:
- Iced is still maturing (may hit rough edges)
- Custom widgets needed for unique visuals
- Android support requires extra work (winit + NDK)

---

### Decision 7: Network Protocol Format

**Date**: 2025-01-XX

**Question**: How to serialize messages on the wire?

**Options Considered**:
1. Bincode (Rust-native binary)
2. Protocol Buffers
3. JSON
4. Custom binary format

**Decision**: Bincode with length prefixing

**Rationale**:
- Simplest for Rust (serde support)
- Compact binary format
- No schema files needed
- Fast serialization

**Consequences**:
- Not cross-language compatible (doesn't matter for now)
- Need version handling (frame type field)
- Can't inspect frames with Wireshark easily (feature, not bug)

---

### Decision 8: Testing Strategy

**Date**: 2025-01-XX

**Question**: How to test cryptographic correctness?

**Decision**:
1. Unit tests in each module (inline with `#[cfg(test)]`)
2. Integration tests in `tests/` directory
3. Test vectors from whitepaper
4. Property-based testing with `proptest` for crypto
5. Manual end-to-end testing (two instances chatting)

**Rationale**:
- Crypto bugs are critical (need thorough testing)
- Unit tests catch regressions
- Property tests find edge cases
- Manual testing ensures UX works

**Consequences**:
- Test suite will be large
- CI needed (GitHub Actions)
- May slow down development initially (worth it)

---

### Decision 9: Error Handling

**Date**: 2025-01-XX

**Question**: How to handle errors across layers?

**Options Considered**:
1. `anyhow` everywhere (quick and dirty)
2. Custom error types per module
3. `thiserror` for defining errors, `anyhow` for propagation

**Decision**: `thiserror` for library code, `anyhow` in app/main

**Rationale**:
- Library modules (crypto, network) need specific error types
- App code can use `anyhow` for convenience
- Best of both worlds

**Consequences**:
- More boilerplate (error enum per module)
- Better error messages for users
- Easier debugging

---

### Decision 10: Expiring Messages Implementation

**Date**: 2025-01-XX

**Question**: How to delete expired messages?

**Options Considered**:
1. Background thread checks every N seconds
2. Delete on app launch
3. Delete lazily when loading conversation
4. Both 1 and 2

**Decision**: Background thread + on-launch cleanup

**Rationale**:
- Background thread ensures timely deletion (privacy goal)
- On-launch as backup (if app was closed during expiration)
- User expects expired messages to disappear

**Consequences**:
- Another background thread (minimal overhead)
- Need to handle deletion while conversation is open (UI update)

---

## Next Steps

### You're starting development now. Here's your checklist:

**Today (Day 1)**:
- [ ] Create project: `cargo new tmessage --bin`
- [ ] Set up directory structure (see above)
- [ ] Copy this README.md into project
- [ ] Add dependencies to Cargo.toml
- [ ] Create `docs/architecture.md` (copy architecture section from this file)
- [ ] Initialize git repository

**This Week (Week 1)**:
- [ ] Define all types in `src/types/`
  - [ ] `message.rs`
  - [ ] `contact.rs`
  - [ ] `identity.rs`
  - [ ] `seed.rs`
  - [ ] `shard.rs`
  - [ ] `peer.rs`
- [ ] Get types compiling
- [ ] Write basic tests for type serialization

**Next Week (Week 2)**:
- [ ] Implement `crypto/chain.rs` (rolling-chain encryption)
- [ ] Write comprehensive tests for MessageChain
- [ ] Test with vectors from tmessage.tex
- [ ] Verify forward secrecy, replay protection, reorder detection

**Week 3**:
- [ ] Implement `crypto/keys.rs` (identity generation, storage)
- [ ] Implement `crypto/shards.rs` (shard distribution, reconstruction)
- [ ] Test key recovery with various shard combinations
- [ ] Milestone 1 complete: Crypto foundation working ✓

---

## Communication with Claude (Important!)

When you resume working with Claude on this project:

### What to Include in Each Session

```
I'm working on tmessage, a decentralized messenger in Rust.

Current Status:
- Milestone: [X] (e.g., "Week 2: Rolling Chain Implementation")
- Last completed: [describe what works]
- Current task: [what you're working on now]
- Stuck on: [if applicable]

Context:
- See README.md section [X] for architecture
- Working in file: src/[path/to/file.rs]
- Relevant code: [paste snippet if needed]

Question:
[Your actual question]
```

### What Claude Needs to Know

1. **Which milestone you're on** (so I know what should be working)
2. **Current file structure** (if you've deviated from README)
3. **What's already implemented** (so I don't suggest redoing things)
4. **What's broken** (error messages, unexpected behavior)
5. **Your goal for this session** (fix bug? implement feature? refactor?)

### Keeping This README Updated

As you make decisions:
- Add to "Critical Decisions Log"
- Update directory structure if you change it
- Mark milestones complete with dates
- Note any architecture changes

---

## Resources

### Crates Documentation

- **Iced**: https://docs.rs/iced/
- **BLAKE3**: https://docs.rs/blake3/
- **ChaCha20-Poly1305**: https://docs.rs/chacha20poly1305/
- **X25519**: https://docs.rs/x25519-dalek/
- **Tokio**: https://docs.rs/tokio/
- **Mainline DHT**: https://docs.rs/mainline/

### Relevant Papers & Specs

- tmessage.tex (your whitepaper)
- Signal Protocol: https://signal.org/docs/
- Mainline DHT (BEP 5): https://www.bittorrent.org/beps/bep_0005.html
- TLS 1.3 RFC: https://datatracker.ietf.org/doc/html/rfc8446

### Similar Projects (Learn From)

- Signal Desktop (Electron + crypto)
- Briar (Android P2P messenger)
- Jami (formerly Ring, decentralized)
- Tox (P2P messenger protocol)

### Rust Patterns

- Async Rust: https://rust-lang.github.io/async-book/
- Error Handling: https://rust-lang-nursery.github.io/rust-cookbook/errors.html
- Crypto Best Practices: https://github.com/RustCrypto

---

## License

MIT License - See LICENSE file

---

## Contact

**Nick Spiker** - fractaldecoder@proton.me

---

## Final Notes

### Remember the Goal

tmessage is not just another messenger. It's the first application proving TOKEN's A=1 authentication model works. Every design decision should support:

1. **Decentralization** (no servers, no central authority)
2. **True single sign-on** (authenticate once, never again)
3. **Social trust** (key recovery via friends, not corporations)
4. **Privacy** (encrypted, metadata-resistant, opt-in features)
5. **Sovereignty** (users own their data, cryptographically)

### Development Philosophy

- **Rust First**: No compromises. If a library isn't in Rust, write it yourself or find an alternative.
- **Test Everything**: Crypto bugs are unacceptable. Test obsessively.
- **Document as You Go**: Future you (and future contributors) will thank you.
- **Fail Fast**: If something doesn't work, don't hack around it. Fix the root cause.
- **User Experience Matters**: Crypto should be invisible. UX should be delightful.

### When You Get Stuck

1. **Read the Error**: Rust's error messages are amazing. Actually read them.
2. **Check the Tests**: What do your tests say should happen?
3. **Simplify**: Strip away complexity until it works, then add back piece by piece.
4. **Ask Claude**: But give context (see "Communication with Claude" section).
5. **Take a Break**: Fresh eyes solve problems.

### This is a Cathedral

You're not building a quick hack. You're architecting a foundational system that could replace passwords, server-based identity, and centralized trust. Take your time. Do it right.

The peasants will use the cathedral even if they don't understand the buttresses.

---

**Project Started**: [Date]

**Last Updated**: [Date]

**Current Milestone**: Phase 1, Week 1

**Status**: 🟢 Active Development

---

## Appendix A: Useful Commands

```bash
# Build (debug)
cargo build

# Build (release, optimized)
cargo build --release

# Run
cargo run

# Run tests
cargo test

# Run specific test
cargo test test_rolling_chain_encryption

# Run tests with output
cargo test -- --nocapture

# Check without building (fast)
cargo check

# Format code
cargo fmt

# Lint
cargo clippy

# Generate documentation
cargo doc --open

# Run benchmarks
cargo bench

# Clean build artifacts
cargo clean

# Cross-compile for Android
cargo build --target aarch64-linux-android --release

# Profile (Linux)
perf record -g target/release/tmessage
perf report

# Memory leak check
valgrind --leak-check=full target/debug/tmessage
```

---

## Appendix B: Debugging Tips

### Crypto Issues

```rust
// Add debug output to chain state
println!("Chain state after encrypt: {:02x?}", &self.state[..8]);

// Verify serialization round-trip
let msg = Message::new(0, b"test".to_vec());
let serialized = bincode::serialize(&msg).unwrap();
let deserialized: Message = bincode::deserialize(&serialized).unwrap();
assert_eq!(msg.payload, deserialized.payload);
```

### Network Issues

```bash
# Monitor DHT traffic
tcpdump -i any -n 'udp port 6881'

# Check if port is open
nc -zv localhost 6881

# Test TLS connection
openssl s_client -connect peer_ip:port

# Wireshark filter for tmessage
tcp.port == 6881 || udp.port == 6881
```

### UI Issues

```rust
// Add debug borders to layouts
container(content).style(|_theme| {
    container::Style {
        border: Border {
            width: 1.0,
            color: Color::from_rgb(1.0, 0.0, 0.0),
            ..Default::default()
        },
        ..Default::default()
    }
})

// Print widget bounds
fn layout(&self, renderer: &Renderer, limits: &Limits) -> Node {
    let node = Node::new(Size::new(100.0, 50.0));
    println!("Widget bounds: {:?}", node.bounds());
    node
}
```

---

## Appendix C: Performance Targets

Track these metrics as you develop:

**Crypto**:
- Message encryption: < 1ms
- Message decryption: < 1ms
- Key generation: < 100ms
- Shard reconstruction: < 500ms

**Network**:
- DHT peer lookup: < 2s
- TLS handshake: < 1s
- Message transmission: < 100ms (local network)
- ACK round-trip: < 200ms (local network)

**UI**:
- Frame rate: 60 FPS minimum
- Input latency: < 16ms
- App launch time: < 2s
- Message list scroll: smooth (no jank)

**Storage**:
- Message save: < 10ms
- Conversation load: < 50ms (1000 messages)
- Database query: < 5ms

**Memory**:
- Idle: < 50MB RAM
- Active conversation: < 100MB RAM
- 10 conversations: < 200MB RAM

**Battery (Android)**:
- Background drain: < 2% per hour
- Active usage: < 10% per hour

---

**END OF README**

This README should be your single source of truth for the project. Print it, keep it open in a tab, refer to it constantly. Update it as decisions are made. It will save you hours of confusion.

Good luck, and remember: A = 1. 🚀
```