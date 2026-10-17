---
name: project-vsf-canonical-signing
description: "VSF signing converged on ONE canonical scheme (ge over BLAKE3(file, ge zeroed)); hp-value signing retired 2026-07-06; hp alone verifies an unsigned doc"
metadata:
  type: project
---

Two Ed25519 schemes once coexisted (canonical whole-file vs hand-rolled sig over the bare hp value); the mismatch silently killed the relay. On 2026-07-06 everything converged on the canonical scheme (photon b0f18c3, worker 5a35e54, vsf `read_verified`).

**Why:** `read_verified` is the single un-skippable verification door; a second scheme is a second door.

**How to apply:** never sign the hp value; write with `vsf::verification::sign_file` (or `build_signed`), read with `read_verified(bytes, Some(pinned_key))`.
Nick's ruling (2026-07-06): hp alone verifies an UNSIGNED document (hp and hb are equally self-attesting); pinned-signer reads still demand a signature.
Known deviation: ping/pong/chat use hp = chain linkage and CLUTCH KEM/complete use hp = ceremony_id — they fail `is_original` by design while the whole-file signature verifies.
