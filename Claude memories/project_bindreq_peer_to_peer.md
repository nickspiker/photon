---
name: project-bindreq-peer-to-peer
description: 2026-10-17 Nick: FGTW is ONLY the phonebook — the bind-request registry (consent eggs + PQ bundle held on the worker) violates that; the request should go device to device. Triggered by Emma's "bindreq_put too_large"
metadata:
  type: project
---

2026-10-17, Emma could not bind a new device: "fgtw bindreq_put too_large".
Cause: fgtw-bootstrap `handle_bindreq_put` caps the body at 4096 bytes, but since fgtw 3da07b4 (2026-09-19, "declaring a key is proving it") the request carries one egg per scheme the joiner holds — Ed25519 + Falcon-512 + SPHINCS+ (SLH-DSA-SHA2-128s sig = 7856 bytes) — plus the `kb` key bundle, ~10 KB; every device holding a full bundle has been unable to post a bind request since.
Nick, asked why the server is in the path at all: "We're just using that for the address book." (his word: the PHONEBOOK)
The registry design (docs/pairing-v2.md, July 2026) predates the peers-are-FGTW doctrine ([[project_peers_are_fgtw]]): the worker stores and verifies the request; words/BLE/NFC only select.

**Why:** FGTW's job is resolving where a device is; anything it stores beyond that is a server dependency, a size/availability failure point and a trust surface (today's outage was exactly that).
**How to apply:** the selector (words/BLE/NFC) yields the candidate device key → the old device resolves its address in the phonebook → pulls consent + bundle straight from the candidate over PT/relay and verifies there; retire bindreq_put/list/withdraw from the worker. Until that lands, the worker cap must fit a MASK_ALL request.

**Second wall, same day (green confirm never finishes):** after the cap fix Emma's bind landed (sibling 0a08c6e4 reconciled) but every "Yes, green — finish" tap came back Failed, unlogged.
Cause: fgtw `mint_fleet_key` / `grow_fleet_wraps` took a bare `&Keypair`, so photon's wrappers published the fan-out Ed25519-only; once every member declared Falcon (log: "floor Ed25519 · Falcon · SPHINCS+, 2/2 declared the basket") the worker's `envelope_refusal` bounces it `below_floor`. Every other fleet writer already went thru `signer_for`; these two could not.
Fixed 2026-10-17: both take `&impl FleetSigner`, photon passes `signer_for(device_key)`; test a_fanout_publish_by_a_full_bundle_device_clears_the_envelope_floor; the confirm now logs ADD-DEVICE lines and a missing input no longer strands `add_device_checking`. Needs a photon build on the CONFIRMING (old) device — the server was never wrong here.
