# NETWORK.md - Photon Network Architecture

## Transport Stack

```
┌─────────────────────────────────────────────────────────┐
│                    Application Layer                   │
│  CLUTCH offers, KEM responses, chat, pings, attachments│
└─────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────┐
│                      VSF Layer                         │
│         Everything is self-describing VSF bytes        │
│         Magic: RÅ< (0x52 0xC3 0x85 0x3C)               │
└─────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────┐
│                  PT (Photon Transport)                 │
│     Reliable delivery over UDP with 'a'-'z' streams    │
└─────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────┐
│ Raw Transport (private, ONLY PT CAN CALL INTERNALLY!!!! │
│              UDP port 4383 (primary)                   │
│              TCP port 4383 (fallback)                  │
│              fgtw.org Cloudflare (fallback)            │
└─────────────────────────────────────────────────────────┘
```

## Port Usage

**Port 4383** - All Photon Transport (UDP + TCP dual-stack), fallback port 3546 if unable to bind to 4383, last case, AUTO!

- PT/UDP: Primary transport, PT streams, LAN discovery broadcasts
- PT/TCP: Fallback when UDP fails after retries

## Packet Discrimination (RX)

When bytes arrive on UDP port 4383:

```
Incoming bytes:
  │
  ├─ First byte 'a'-'z'? ──────────► PT DATA packet (stream chunk)
  │
  ├─ Starts with "RÅ<"? ──────────► Complete VSF packet, parse header:
  │     │
  │     ├─ Section "pt_spec" ─────► New inbound PT stream (SPEC)
  │     ├─ Section "pt_ack" ──────► ACK for our outbound stream
  │     ├─ Section "pt_nak" ──────► NAK, retransmit requested seqs
  │     ├─ Section "pt_ctrl" ─────► Flow control
  │     ├─ Section "pt_done" ─────► Transfer complete
  │     ├─ Section "pt_disc" ─────► LAN peer announcement
  │     └─ Other sections ────────► Application message (ping, chat, CLUTCH, etc.)
  │
  └─ Otherwise ───────────────────► Unknown, ignore
```

## LAN Discovery

Broadcast to `255.255.255.255:4383` for local peer discovery.

**Format**: Complete VSF file
- `hp` (provenance hash) = handle_proof (identity)
- Section `pt_disc` with `port` field

```rust
// Build
let packet = udp::build_lan_discovery(handle_proof, port);

// Parse
if let Some((handle_proof, ip, port)) = parse_lan_discovery(bytes, src_addr) {
    // Found local peer
}
```

**Example parsed pt_disc packet:**
```
 Version 5
 Backward compat 5
 Created 2025-DEC-13 8:53:42.284 PM
 Header size: 62 Bytes
 32-Byte BLAKE3 provenance hash hex
    A1B2C3D4E5F6071889ABCDEF01234567
    89ABCDEF0123456789ABCDEF01234567
 (pt_disc @62 6 Bytes 1 field)
>┓
 ┗━[
   ┗━ (u4 port : 4383)
   ]

Valid
```

## Message Types (All VSF)

| Message | Section Name | Typical Size | Notes |
|---------|--------------|--------------|-------|
| CLUTCH Full Offer | `clutch_offer` | ~548KB | Contains all KEM public keys |
| CLUTCH KEM Response | `clutch_kem` | ~17KB | Encapsulated shared secrets |
| CLUTCH Complete | `clutch_complete` | ~300B | Ceremony completion proof |
| Ping | `ping` | ~100B | Keepalive with provenance |
| Pong | `pong` | ~100B | Ping response |
| Chat Message | `msg` | Variable | Encrypted message content |
| LAN Discovery | `pt_disc` | ~158B | Local network broadcast |

## Sending Data

**One interface for everything:**

```rust
// Small or large, doesn't matter - PT handles ALL of it!
let spec_bytes = pt_manager.send(peer_addr, vsf_bytes);
udp::send(&socket, &spec_bytes, peer_addr).await;
```

PT automatically:
- Assigns stream ID ('a'-'z')
- Shards into 1KB DATA packets if needed
- Handles ACKs, retries, exponential backoff
- Falls back to TCP after repeated failures

## Receiving Data

PT reassembles streams automatically. When complete:

```rust
if let Some(vsf_bytes) = pt_manager.take_inbound_data(peer_addr) {
    // vsf_bytes is the complete, verified VSF file
    // Parse and handle based on section name
}
```

## Integrity Chain

0. **Per-chunk**: Each DATA packet ACK'd with BLAKE3(chunk)
1. **Per-transfer**: COMPLETE packet contains BLAKE3(reassembled)
2. **VSF verification**: Magic bytes, provenance hash, signature (if signed)

Corruption at any layer is detected and corrected via retransmit.

## TCP Fallback

After `SPEC_MAX_RETRIES` (5) UDP failures with exponential backoff:

```
1s → 2s → 4s → 8s → 16s → TCP fallback
```

TCP uses length-prefixed framing: `[4-byte BE length][payload]`, needs converted to VSF spec with optional L prefix indicating message size
