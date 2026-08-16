---
name: project-vsf-canonical-signing
description: "VSF signing converged on ONE canonical scheme (ge over BLAKE3(file, ge zeroed)); hp-value signing retired; lockstep worker deploy 2026-07-06"
metadata: 
  node_type: memory
  type: project
  originSessionId: 71fe9349-599c-4d2c-9970-3a988ee9a08f
---

Two Ed25519 signing schemes coexisted in the FGTW stack: canonical (`sign_file`/`verify_file_signature`/`read_verified`: ge over BLAKE3(file, hp filled, ge zeroed)) and hand-rolled (ge over the bare 32-byte hp value: worker challenge, photon avatar content docs, relay/fetch requests).
Relay was silently dead because of the mismatch (scheme-2 TX, scheme-1 verify at the worker, non-fatal fallback masked it).
On 2026-07-06 everything converged on the canonical scheme: photon b0f18c3, worker 5a35e54 (deployed, version 8646a6bc), on vsf 15a27c3's `read_verified`/`parse_document`.

**Why:** `read_verified` is the single un-skippable verification door; a second scheme means a second door.

**How to apply:** never sign the hp value; always `vsf::verification::sign_file` (or `build_signed`) to write, `read_verified(bytes, Some(pinned_key))` to read.
Nick's ruling (2026-07-06, vsf dd273aa): **hp alone verifies an unsigned document** — hp and hb are equally self-attesting; requiring hb caused four hp-only frame moles in one day and added no security. Pinned-signer reads still demand a signature; error-frame detection stays a separate FIRST step.
KNOWN DEVIATION: ping/pong/chat use hp = chain-linkage, CLUTCH KEM/complete use hp = ceremony_id — application semantics that fail `is_original` by design; their whole-file signature still verifies. Resolves in the messaging rework ([[project-fgtw-migration-state]]).
