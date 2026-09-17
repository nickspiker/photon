---
name: project_pigeon_fetch_fanout
description: 2026-09-17 pigeon re-upload/replication conviction — a fetch fanned out to every device and every holder served the WHOLE blob, and every served chunk also rode a Cloudflare relay copy unconditionally; fixed: one ranked device per ask, relay copy only when the direct leg is unproven, 10 s serve dedupe, held-blob manifests/chunks ignored
metadata:
  type: project
---

**Field (Nick's log 2026-09-17 21:54–22:25, "I keep seeing re-uploads/replications of pigeons"):** two ~142 MB pigeons (544/545 chunks) — "manifest stored … 1 already held" 5× in 16 s, then "served chunked request — 262 of 545 + manifest" TWICE 3 s apart (68 MB re-sent); three ~300 KB pigeons at 22:24 — each "chunked blob complete" ~9× (28 completions for 3 blobs).

**Causes (all in the code, none in the network):** (1) `attach_fetch` sent the request to the friend's EVERY device row AND all our siblings at once; every holder answered with manifest + all chunks. (2) The serve path (`AttachReqReceived`, status.rs) dispatched each chunk as `HistorySendRequest { peer_addr, relay_to: vec![requester] }` and the history drain sends the direct PT leg AND the relay copy for every `relay_to` entry unconditionally → every chunk twice, one copy thru Cloudflare. (3) Duplicate asks (direct + relay copy of one request) were each served in full. (4) A manifest/chunk for a blob already whole here was re-stored and re-counted.

**Fix (photon commit 2026-09-17):** `attach_fetch` ranks candidates (friend online+validated path → friend online → our online siblings → rest) and asks ONE, `tries % len` rotating on the 20 s re-ask; serve: `relay_needed = request came via relay || no validated path to the requester`, chunks and whole blobs carry `relay_to` only then (the manifest keeps its always-copy, it is tiny); `attach_served_recent` drops a repeat (device, hash) ask within 10 s; manifest/chunk arms bail on `blob_present`. NOT changed: history-page serve (status.rs ~4056) still always carries the relay copy — pages are moderate; revisit if relay cost shows there.

Related: [[project_attachments]], [[project_traversal_relay_gap]].
