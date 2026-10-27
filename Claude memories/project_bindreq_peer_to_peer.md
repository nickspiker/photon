---
name: project-bindreq-peer-to-peer
description: "2026-10-17 Nick: FGTW is ONLY the phonebook — the bind-request registry (consent eggs + PQ bundle held on the worker) violates that; the request should go device to device. Triggered by Emma's 'bindreq_put too_large'"
metadata:
  type: project
---

2026-10-17, Emma could not bind a new device: "fgtw bindreq_put too_large".
Cause: fgtw-bootstrap `handle_bindreq_put` capped the body at 4096 bytes, but since fgtw 3da07b4 (2026-09-19, "declaring a key is proving it") the request carries one egg per scheme the joiner holds — Ed25519 + Falcon-512 + SPHINCS+ (7856-byte sig) — plus the `kb` key bundle, ~10 KB; every full-bundle device had been unable to post a bind request since.
Nick, asked why the server is in the path at all: "We're just using that for the address book." (his word: the PHONEBOOK)
The registry design (docs/pairing-v2.md, July 2026) predates the peers-are-FGTW doctrine ([[project_peers_are_fgtw]]): the worker stores and verifies the request; words/BLE/NFC only select.

**Why:** FGTW's job is resolving where a device is; anything it stores beyond that is a server dependency, a size/availability failure point and a trust surface (this outage was exactly that).
**BUILT 2026-10-26** ([[project_fleet_over_mesh]]): the request goes joiner → sponsor as a `bind_req` frame over the LAN (the registry kept as a mirror), the chain and the fan-out come back over the mesh; the bindreq routes are not yet retired from the worker.
**How to apply:** the selector (words/BLE/NFC) yields the candidate device key → the old device resolves its address in the phonebook → pulls consent + bundle straight from the candidate over PT/relay and verifies there; retire bindreq_put/list/withdraw from the worker. Until then the worker cap must fit a MASK_ALL request.

**Second wall, same day (green confirm never finished):** after the cap fix every "Yes, green — finish" tap came back Failed, unlogged.
Cause: fgtw `mint_fleet_key` / `grow_fleet_wraps` took a bare `&Keypair`, so the fan-out published Ed25519-only and the worker's `envelope_refusal` bounced it `below_floor` once every member declared Falcon.
Fixed 2026-10-17: both take `&impl FleetSigner`, photon passes `signer_for(device_key)` (like every other fleet writer); test a_fanout_publish_by_a_full_bundle_device_clears_the_envelope_floor; the confirm logs ADD-DEVICE lines and a missing input no longer strands `add_device_checking`. Needs a photon build on the CONFIRMING (old) device.

**Third round (2026-10-17 evening, phone on 0.120.1 dev; Mac + Emma's new Arch box 0a08c6e4 on v0.120.0), open:**
- Add: the phone's app update wiped the in-RAM two-phase confirm (`add_device_bound`); the new device got its wrap only via the UNLOCK grow after a revoke+reinstate. Gap: a bound-but-unwrapped member has no path to its wrap after the sponsor restarts — persist the pending confirm or offer "finish" on the Fleet page.
- Wave: the Arch box logged "no capture device" + "no render device" (cpal ALSA host, likely PipeWire without pipewire-alsa); the render watchdog then rebuilt streams every 2 s for a device that never existed.
- Stuck message: the revoke/unlock armed a heavy weave on Emma's side; Nick's Mac parked her offers to the ceremony owner (desktop 90e571bf), discarded KEM responses "encrypted to old keys", era_pull → Hold("not the era owner"), and caught up only when Photon relaunched (messages 20–40 s late).
