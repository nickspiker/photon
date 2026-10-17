---
name: project-pairing-v2
description: "Pairing v2 words-first BUILT 2026-07-13: masked words, consent egg, proximity-populates-the-list, two-phase green confirm; the worker bindreq registry is being retired (2026-10-17, device to device)"
metadata:
  type: project
---

Canonical: docs/pairing-v2.md (opens with the [[project_device_sovereignty]] manifesto).
Reframe: words/NFC/BLE are three SELECTORS on one candidate-set machine; the WAN never selects.
- **Masked words:** `pair_words(device_pubkey XOR blake3::derive_key("photon pair words v1", identity_seed))`, 23 words, no checksum — the live matcher prefix-matches each keystroke per candidate and flags the exact diverging word.
- **Consent egg:** an Add carries consent_t + consent_sig (the joiner's device signature) bound into signing_bytes; fold rejects |eagle_time − consent_t| > 1 h. Conscription is structurally impossible.
- **Proximity populates the list (load-bearing):** tappable candidates come ONLY from proximity (BLE heard, NFC tap), never from the WAN registry — a remote attacker holding the handle could flood it with decoys. No BLE/NFC → type the words (reading them off the physical screen IS the proximity check).
- **Two-phase:** bind → green lamp → human confirm → rotate. Only the sponsor's confirm establishes the key (a freshly bound device must not rotate itself). A wrong bind = a permanent testimony entry + rotate-around.
- Removal = self-signed departure only ([[self-only-removal]]).
- Registry (worker bindreq_put/list/withdraw, keyed (hp, device_pubkey), dual-sig write, member-gated read): **being retired** — Nick 2026-10-17, FGTW is the phonebook only; the request goes device to device ([[project_bindreq_peer_to_peer]]).
Pending: two-device on-screen E2E incl. Android; NFC then BLE transports.

**Why:** v1 had a clobberable pair slot, a WAN-delivered word burden, squat-by-listing and no device consent.
**How to apply:** never let a WAN-sourced candidate become tappable; keep the green confirm as the only key-establishing step.
