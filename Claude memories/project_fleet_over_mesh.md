---
name: project_fleet_over_mesh
description: "BUILT 2026-10-26: the membership chain, the bind request and the fan-out travel device to device (fleet_req/fleet_resp, bind_req, fanout_resp); every device holds its fleet's chain; fgtw.org is a mirror asked last. Next: attest degrades to the held chain"
metadata:
  type: project
---

Nick, 2026-10-26, on why a fleet join needed fgtw.org: "that's where the kamadilla routing comes in... not sure why we have to rely on this fgtw.org unless everyone our list does not respond."
Built the same day (photon 864b23ee + the join commit; canonical: docs/peers-are-fgtw.md Phase A′, docs/pairing-v2.md "The ceremony over the mesh"):
- `fleet.rs` holds every membership chain that folds (ours in the vault as `fleet_chain`, reloaded at attest); `fetch()` serves the held copy when the seed is down or behind; forks order longest → newest tip → lower head hash.
- Frames: `fleet_req` / `fleet_resp` (a handle's chain, asked or pushed; answered only to a device some fold knows), `bind_req` (the joiner's request straight to a LAN sponsor), `fanout_resp` (the grow to the newcomer + siblings). All device-signed envelopes, PT-carried like depart_req.
- Contact fold refresh is peers-first: the contact's own devices are asked; the seed only when none is reachable or an earlier ask stands unanswered on a re-fire (edge, no timer).
- The joiner mints ONE request (`make_bind_request`) and sends it to the seed (`bindreq_post`, best-effort; `device_owned` stays a verdict) and to every sponsor beacon heard (`JoinUpdate::Aimed` → the LAN arm). The sponsor merges registry + LAN candidates (`rebuild_add_device_candidates`), binds into the held chain (`bind_device_held`), pushes the chain (Bound arm) and the grow (`fanout_grow_held` → Rotated(blob) → `fanout_push`).
- NOT yet off the seed: the attest's announce (a joiner on a seed-less LAN goes green and holds the key but attest still needs fgtw.org), device→owner index, first-come genesis, relay/pipe, doorbell, inbox, avatars.
- Untested on two devices as of the build; field test pending.

**Why:** the seed was a dependency, an outage (Emma 2026-10-17) and a trust surface in the one flow that puts a device on the air under an identity; the chain is self-verifying so no server needs to vouch for it.
**How to apply:** new fleet state goes device to device first and to the seed as a mirror; never make a seed call load-bearing in the join; the ferros phone's Photon client joins over the LAN through exactly these frames ([[project_bindreq_peer_to_peer]], [[project_peers_are_fgtw]]).
