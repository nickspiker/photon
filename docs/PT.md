# PT.md - Photon Transport Protocol

## Overview

PT is a reliable UDP transport for VSF payloads. It handles:
- Sharding large VSF files into 1KB chunks
- Reliable delivery with ACKs and retransmits
- Congestion control (blast-then-pipeline)
- TCP fallback when UDP repeatedly fails

## Stream IDs

Each transfer gets a stream ID from 'a'-'z' (26 concurrent streams per peer).

```
'a' = 0x61  first transfer
'b' = 0x62  second transfer
...
'z' = 0x7A  26th transfer, then wraps to 'a'
```

Stream IDs route packets to the correct transfer state.

## Packet Types

### DATA Packet (Binary, minimal overhead)

```
[stream_id:1][seq_vsf:1-4][payload:≤1024]
```

- `stream_id`: 'a'-'z' (0x61-0x7A)
- `seq_vsf`: VSF variable-length uint (1-4 bytes depending on total_packets)
- `payload`: Raw chunk data

**Detection**: First byte in range 0x61-0x7A

### Control Packets (VSF format)

All control packets are complete VSF files starting with `RÅ<`:

#### SPEC (Transfer initiation)
```
Section: pt_spec
Fields:
  - sid: stream_id ('a'-'z')
  - count: total packet count
  - psize: payload size per packet (typically 1024)
  - total: total transfer size in bytes
  - hash: BLAKE3 of complete data
```

#### ACK (Chunk acknowledgment)
```
Header-only VSF with inline field:
  - provenance_hash = BLAKE3(chunk payload)  ← IS the integrity proof
  - (pt_ack: stream_id, sequence)
```

#### NAK (Retransmit request)
```
Header-only VSF with inline field:
  - (pt_nak: seq0, seq1, seq2, ...)
```

#### CONTROL (Flow control)
```
Header-only VSF with inline field:
  - (pt_ctrl: command)
  - Commands: 0=Pause, 1=Resume, 2=SlowDown, 3=Abort
```

#### COMPLETE (Transfer verification)
```
Header-only VSF with inline field:
  - provenance_hash = BLAKE3(reassembled data)  ← IS the final verification
  - (pt_done: success_flag)
```

## Transfer Flow

### Sender (Outbound)

```
1. start_send(addr, vsf_bytes)
   ├─ Allocate stream_id
   ├─ Create SendBuffer (shard into 1KB chunks)
   ├─ Compute data_hash = BLAKE3(vsf_bytes)
   └─ Send SPEC packet

2. Receive SPEC ACK (seq=MAX marker)
   └─ Enter blast phase: send INITIAL_BLAST packets immediately

3. For each ACK received:
   ├─ Verify chunk_hash matches
   ├─ Mark sequence as ACK'd
   ├─ Update RTT estimate
   └─ Send packets_per_ack() new packets (pipelining)

4. On timeout:
   ├─ Retransmit unACK'd packets
   └─ Exponential backoff on SPEC retries

5. Receive COMPLETE
   ├─ Verify final_hash matches our data_hash
   └─ Transfer done
```

### Receiver (Inbound)

```
1. Receive SPEC
   ├─ Create ReceiveBuffer
   ├─ Store expected data_hash
   └─ Send SPEC ACK

2. For each DATA packet:
   ├─ Insert chunk into buffer
   ├─ Compute chunk_hash = BLAKE3(payload)
   └─ Send ACK with chunk_hash as provenance

3. All chunks received:
   ├─ Reassemble data
   ├─ Compute final_hash = BLAKE3(reassembled)
   ├─ Verify matches expected hash
   └─ Send COMPLETE

4. Return complete VSF bytes to application
```

## Congestion Control

### Blast Phase
Initial burst of `INITIAL_BLAST` packets (256) without waiting for ACKs.
Floods the pipe to quickly fill buffers.

### Pipelining Phase
After blast, send `packets_per_ack()` new packets for each ACK received.
`send_ratio` adapts based on:
- ACKs received → increase ratio (additive)
- Loss/timeout → decrease ratio (multiplicative)

### RTT Estimation
Smoothed RTT (SRTT) with exponential weighted moving average.
RTO = SRTT * 2 with backoff on repeated timeouts.

## Retry Logic

### SPEC Retries
```
Attempt 1: wait 1s
Attempt 2: wait 2s
Attempt 3: wait 4s
Attempt 4: wait 8s
Attempt 5: wait 16s
Attempt 6+: TCP fallback
```

### DATA Retries
Based on RTO (retransmission timeout) derived from RTT measurements.
Packets not ACK'd within RTO are retransmitted.

## Integrity Verification

```
Layer 1: Chunk Level
  └─ Each ACK contains BLAKE3(chunk payload)
  └─ Sender verifies chunk arrived intact
  └─ Mismatch → retransmit

Layer 2: Transfer Level
  └─ COMPLETE contains BLAKE3(reassembled data)
  └─ Must match SPEC's data_hash
  └─ Mismatch → transfer failed

Layer 3: VSF Level
  └─ Reassembled bytes are complete VSF file
  └─ VSF self-verifies: magic, provenance, signature
  └─ Invalid VSF → rejected
```

## API

### Sending

```rust
// Queue VSF for reliable delivery
let spec_bytes = pt_manager.start_send(peer_addr, vsf_bytes);
udp::send(&socket, &spec_bytes, peer_addr).await;
```

### Receiving

```rust
// Handle incoming DATA
if let Some(ack_bytes) = pt_manager.handle_data(src_addr, data_packet) {
    udp::send(&socket, &ack_bytes, src_addr).await;
}

// Check for complete transfers
if let Some(vsf_bytes) = pt_manager.take_inbound_data(peer_addr) {
    // Process complete VSF
}
```

### Periodic Tick

```rust
// Call regularly to handle timeouts and retries
let to_send = pt_manager.tick();
for (addr, pkt, use_tcp) in to_send {
    if use_tcp {
        tcp::send_tcp(&pkt, addr).await;
    } else {
        udp::send(&socket, &pkt, addr).await;
    }
}
```

## File Structure

```
src/network/pt/
├── mod.rs      PTManager - coordinates all transfers
├── packets.rs  Packet types: PTSpec, PTData, PTAck, PTNak, PTControl, PTComplete
├── state.rs    Transfer state machines: OutboundTransfer, InboundTransfer
├── buffer.rs   SendBuffer, ReceiveBuffer for chunk management
├── window.rs   WindowController, RTTEstimator, FlightTracker
└── transport.rs  Low-level send/recv helpers
```
