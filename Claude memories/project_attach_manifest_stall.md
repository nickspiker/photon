---
name: project_attach_manifest_stall
description: "2026-11-03 CONVICTED + FIXED: a fleet replication fetch stood 'waiting on it' forever after the asked sibling served a manifest with 0 chunks (it was still receiving the blob itself); the pong rule now ends a stalled stream and re-asks with a resume bitmap"
metadata:
  node_type: memory
  type: project
  originSessionId: 17b90e8e-8b1a-448e-8ac7-22b057e9a0d7
  modified: 2026-11-03T15:21:03.522Z
---

Nick 2026-11-03: "Our fleet pigeon replication didn't replicate. it says fetching from devices on my mac and my linux has it." Logs pulled with `photonlog --pull --session` (both devices in one pull).

CONVICTION (one photo, 3f1cd47e, 3.5 MB, 14 chunks, PXL_20261103_150951 from the Pixel): the Mac asked friend devices (one quiet, one "holds none"), then the desktop at 15:10:09.048. The desktop had stored the MANIFEST from the phone at 09.237, served the Mac "0 of 14 chunk(s) + manifest" at 09.238 (`blob_chunk_load` None for every chunk — its own first chunk landed at 09.384, complete at 10.963). The Mac stored the manifest (progressed = true, attach_heard updated) and then: `fetch_pong_from` skipped progressed asks and asks with `heard > f.at`; `attach_fetch_conversation`'s "a streaming device is never abandoned" rule saw not-quiet + heard-since → "already asked of device 90e571bf, which is not quiet — waiting on it", forever. Nothing re-asked the desktop, which had every chunk 2 s later.

FIX (photon attachments.rs + photon_app.rs FetchAsk): `last_heard: Instant` per ask (init = at); every pong from the asked device: if attach_heard[dev] > last_heard → the device streamed since the last pong (any blob) → last_heard = it, pongs_since = 0; else pongs_since += 1, and at 2 the ask ENDS (progressed: "its stream of … stopped — asking again for the rest"; not: "never came — asking the next"). Progress (drain_attach_installed) resets pongs_since. The next tick re-asks via the normal rotation; a resume ask carries the want bitmap and any holder answers it (a holder with none answers attach_none). Edges only, no timer: two pongs with nothing heard since the last one.

NOT changed (option considered): the server could refuse to serve a manifest while it holds zero chunks; left as is, the manifest is useful and the asker-side rule covers every partial-then-silent case.

Related: [[project_pigeon_fetch_fanout]], [[project_group_pigeon_stuck_fetch]], [[edges-not-timers]].
