# PT.md - Photon Transport Protocol

## Overview

PT is a reliable UDP transport for VSF payloads. It handles:
- Sharding large VSF files into 1KB chunks
- Reliable delivery with ACKs and retransmits
- Congestion control (blast-then-pipeline)
- TCP fallback when UDP repeatedly fails

## Spooled receive (2026-09-17, designed — the zero-bitmap spool is PT's buffer done right)

`ReceiveBuffer` today is a pre-allocated RAM `Vec` PLUS a `BitVec` of received packets — a sidecar bitmap beside the data, held in memory, lost on restart. For SEALED payloads both halves are wrong by one insight each (Nick 2026-09-17):

- The bitmap is redundant: a 256-bit window of ciphertext is all-zero with probability 2^-256, so a pre-zeroed buffer IS its own receipt bitmap (`storage/spool.rs`, the proof in its header). The data is the bookkeeping, atomically — "bitmap says held, data missing" becomes unrepresentable rather than avoided.
- The RAM residence is why transfers are size-bound and die with the process: spool-backed, the buffer is a sparse file, the bound is disk, and an interrupted transfer resumes across restart from the file alone — the five-scalar VSF prelude self-verifies (parse success IS the presence test), then the zero-scan reconstructs the have-set with no other state.

Verification decouples from transfer entirely: land + ACK on the hot path with no per-packet checking, ONE whole-payload blake3 at finalize (the SPEC already carries the hash; AEAD tags verify each sealed chunk as a side effect of decrypting). Fault localization on the rare mismatch is an interactive window-hash dialogue at link-tuned granularity — a megabyte on a slow link, a gigabyte for a terabyte on a fast one — computed on demand by both sides, never stored.

The boundary, stated honestly: the zero rule is airtight only for payloads whose every window is ciphertext. Sealed blobs (attachment chunks, bridge pigeons, history bulk) qualify — that is the class that is large, resumable and worth spooling. Complete VSF files with plaintext framing (CLUTCH offers, control payloads) may contain honest zero windows in their structure, so they keep the RAM buffer — they are small, and their retry story is their own machinery's.

Consolidation this implies (not yet built): the attachment wire's parallel reliability layer (`attach_manifest`/`attach_chunk`/WANT bitmaps) and the bridge pigeon should ride PT streams with spool-backed receive, so photon has ONE bulk-transfer story: PT moves packets, the spool is custody and resume, the prelude is the manifest, and the repair dialogue is the only verification protocol.

## Stream IDs

Each transfer gets a stream ID from 'a'-'z' (26 concurrent streams per peer).

```
'a' = 0x61  first transfer
'b' = 0x62  second transfer
...
'z' = 0x7A  26th transfer, then wraps to 'a'
```

Stream IDs route packets to the correct transfer state.

### The send window (2026-09-21)

At most **thirteen** large transfers are live per peer — `PTManager::STREAMS_IN_FLIGHT`, half the alphabet — and the ids rotate `a..z` by modulo **per peer**. The rest queue FIFO in `pending_outbound` and release in `tick()` on the completion or failure edge of a live one (a completed outbound transfer is swept by that same tick; it used to linger with two copies of its payload until the peer was cleared).

Why half: the receiver evicts an incomplete inbound transfer when a new SPEC arrives on its stream id, and a late packet from an old holder must find no live twin. With thirteen live and twenty-six ids, an id comes back around only after its previous holder was retired at least thirteen releases earlier. The field case that forced it: a 345 MB bridge pigeon (1383 chunks) fired every chunk at once, wrapped the alphabet fifty times over, and the chunks killed each other mid-flight — 62 of 1383 landed, 1609 `Transfer FAILED`. Every bulk sender rides the window without knowing it (pigeons, attachment serves, history pages, offers). A retarget re-aims queued transfers too; `clear_outbound` (relay says offline, CLUTCH completion) drops the queue with the ladder.

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

### Built 2026-09-17: the bridge pigeon as the first spooled tenant (stages 1–3)

The consolidation landed STAGED, because the receive map settled one fact: attachments ALREADY survive restart (chunks land as content-addressed vault blobs; `attach_fetch` rebuilds the WANT set from `blob_chunks_held`), so migrating the working attach path onto the spool is field risk with no functional gain until the new path is proven. What is live:

- **Wire** — `pigeon_chunk` (protocol.rs) is byte-identical in shape to `attach_chunk` with ONE difference, the section name, and that name is load-bearing: the receiver's sniff-dispatch ladder routes an attach_chunk to the vault-install worker, which is exactly wrong for an ephemeral one-device drop. A test pins that the two parsers reject each other's frames. The announcement is a typed `RefKind::BridgePigeon` row whose content is the file name (a visible bubble both sides) carrying `BridgeWire.pigeon` (name, whole-file hash, size); the host's run gate matches `BridgeCmd` only, so it can never execute.
- **Receive** — `network::pigeon::PigeonReceiver`: a zero-bitmap spool per whole-file hash in `runtime_dir()/pigeons`, opened (or resumed) on the announcement under the FLEET key and keyed to the announcing device (a valid frame from any other signer is refused); `chunk` is one `pwrite` into the slot; completion is the zero-window scan; `take_complete` detaches the whole spool so `finalize_inflight` runs the decrypt-walk + whole-file hash + landing on the seal worker, never the UI thread. A chunk that will not open keeps the spool (resumable); a spool that completes to the WRONG hash is shed on the spot — poison, not a resumable state.
- **Landing** — `storage::land_blob`, shared with attachment Save: streamed from the vault under EXACTLY the name sent, replacing whatever had that name. No `(2)` suffix, no backup copy — a transfer that changes the name it was given has not delivered the file; rollback is the receiving host's snapshots (BTRFS + snapper on leviathan), never a name the writer invented. The directory is the sibling's shell cwd as of its last command (`bridge::BridgeCwdMap`, written by the shell worker from the command sentinel), else home. The host answers the operator with a `BridgeOut` row naming the landed path (or `Msg::PigeonLandFailed`).
- **Ephemeral both ends** — the host sheds the spool and its vault copy once the file lands. (SUPERSEDED 2026-10-07, "Pigeons, v2": the client used to shed its vault copy once the chunks were dispatched; it now keeps it until the host reports every slot held.)
- **Progress (2026-09-20)** — the host answers the landing edge of a chunk with a `pigeon_ack` frame (`tok`, `hash`, `got`, `of`; signed, small-packet lane with the mandatory packet-ack, direct or off the pipe alike), thinned to the first chunk, the last, and every crossing of one sixty-fourth — at most ~65 frames per pigeon however large. The sender's row draws its bar from these, the host's row from its own spool count; the count is the receiver's RAM presence bitmap (seeded from the spool's zero-scan at announce, so a resumed spool starts part-way), which also gates the whole-spool zero-scan to run once at completion instead of after every chunk. A whole pigeon drops its bar and the `BridgeOut` landing row follows.

Cross-device leg is FIELD-PENDING (compile + unit only here), like the bridge itself shipped. Deferred, field-proven-first: stage 4 makes PT's `ReceiveBuffer` itself spool-backed for sealed payloads (the hot `insert` runs under the PT mutex in the tokio recv task, so the disk write must be offloaded exactly as the attach path's seal_job already does) and migrates attachments onto it — one bulk-transfer story. Also not yet built: the window-hash repair dialogue (today a failed finalize is a re-drop away from a retry).

### Pigeons, v2 (built 2026-10-07): self-describing, repaired, bitmapless

The field failure that forced it: a 1 MB drop over a fast LAN sent four chunk streams that finished before the announcement row was decrypted on the host — three were dropped as "a chunk for a pigeon nobody announced", the client had already shed its copy, and nothing ever asked again. The host restarted for an update between drops, and the second drop lost chunk 0 the same way. The stage 1–3 pigeon had no handshake: data and its announcement rode two channels with no ordering, and the sender's copy was gone before anyone could say what was missing.

What v2 changes, and nothing else (PT, the lettered streams and the send window, the spool, the landing are untouched):

- **Every chunk self-describes.** A `pigeon_chunk` carries `sz` (the whole file's plaintext size) and `snm` (its name, sealed under the fleet key — never in clear). The first frame to arrive, chunk or announcement row, opens the spool; the other only fills in what is missing (a name). A whole spool that is still nameless waits for its row before landing. A frame from an older build has no `sz` and still needs its announcement first.
- **The host repairs; the sender serves.** `pigeon_want` (tok, hash, one `want` value per missing slot) is the host's ask, sent when a resumed spool is incomplete and whenever a pigeon stops moving for `REPAIR_AFTER` (20 s), up to `MAX_REPAIR_ASKS` (8) — then the transcript says it stopped and the spool stays on disk. The sender seals exactly the named slots. A want too large for one datagram travels as a whole PT transfer; both paths dispatch it.
- **The sender keeps its copy until the host holds every slot** (`pigeon_ack` with got = of). It used to shed the moment the chunks were dispatched. A file that was already in the vault before the drop is never shed. After a sender restart the vault's own manifest still serves a repair.
- **Restart on the host is free.** The spool prelude now records the sending device (`from`), so a fresh session rediscovers every spool in `runtime_dir()/pigeons`, counts what it holds from the file, and asks that device for the rest.
- **No bitmap, anywhere** (Nick 2026-10-07: "no bitmap is needed since we store it encrypted"). The receiver's RAM presence list is gone: it keeps a count, derived once from the zero-scan at open and bumped only when a frame fills a slot that read empty (the ONE slot is read before the write, so a duplicate never counts twice and the file is never rescanned per chunk). The repair ask is computed from the zero-scan at the moment of asking and stored nowhere. Completion is the count reaching the total, confirmed by one full scan.
- **The drop no longer stalls the window**: the file is hashed and stored on the seal worker; the tick announces it.

The attachment path got the matching fix the same day: a chunk that overtakes its manifest is stored at its own content hash (chunks are content-addressed and authenticated, so there is nothing to park), and a manifest that finds every chunk already held installs the blob at once. It used to drop the early chunk and wait twenty seconds for the re-ask.

### The bitmapless transfer — the general shape (draft, 2026-10-07)

Pigeons v2 is the first tenant of one rule for every SEALED bulk transfer in photon. Stated once, so attachments, history bulk and beams can be moved onto it:

1. **The receiver's store is the only state.** Slots hold ciphertext; an unwritten slot is zeros (a spool) or absent (a content-addressed vault value). "What do I hold" is always a read of the store. Nothing beside it — no bitmap, no want set, no persisted progress — can disagree with it, because nothing beside it exists. A count in RAM is a cache of a scan, never a record.
2. **Every data frame self-describes** enough to open the receiver's store: the transfer's identity (the whole-plaintext hash), its geometry (size; chunk size is a protocol constant) and anything the landing needs, sealed. Arrival order is irrelevant: whichever frame lands first opens the store.
3. **Blast, then repair.** The sender sends everything once; PT's streams, letters and window carry it. The receiver asks for exactly what its store lacks, computed at the moment of asking, on the edges that matter — a resume, a transfer that stopped moving — and the sender serves exactly that.
4. **Custody ends at the receiver's word.** The sender holds its copy until the receiver reports the whole held; per-chunk AEAD tags verify on arrival or at finalize, and one whole-plaintext hash verifies the result.
5. **Any holder can serve.** Because slots are addressed by content and verified by the receiver, the repair ask can go to any device that holds the bytes, not only the one that started the transfer — the fleet-holds-everything replication is this same ask aimed at whoever is easiest.

What remains to build on it (stage 4, unchanged in intent from "Spooled receive" above): make PT's own `ReceiveBuffer` spool-backed for sealed payloads so ONE PT stream can carry a whole file (the SPEC is then the self-description, PT's NAK is the repair ask read off the zero-scan, and the hot write is offloaded from the recv task); seal deterministically (nonce from the chunk's hash) so a sender that restarts mid-stream produces the same ciphertext and the receiver's slots stay valid; then migrate attachments off their WANT bitmaps onto it.

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
