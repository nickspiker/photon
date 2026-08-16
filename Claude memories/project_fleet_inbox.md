---
name: project_fleet_inbox
description: "Fleet inbox spec'd in docs/fleet-inbox.md: inbox/<hp>/ store-and-forward + hub/FCM wake; 3 authorship classes; v1 = bind-attempt alert; NOT built"
metadata: 
  node_type: memory
  type: project
  originSessionId: 9dd5621d-5f68-4b43-83bd-807531b898e2
---

Fleet-wide message channel DESIGNED 2026-07-12 in `docs/fleet-inbox.md` (nothing built).
Mechanism: `inbox/<handle_proof>/<ts>.vsf` on R2 (relay mechanics, identity-addressed), wake via existing PeerUpdateHub WS broadcast + FCM + attest/resume drain floor.

Three authorship classes, stamped STRUCTURALLY by the worker at write time (member writes can never render as worker events):
1. worker-observed events (bind attempt / `device_owned` refusal / pair_put) — advisory, display-only;
2. release-key-signed notices (product updates, global feed) — answers updates.md's poll-cadence open question with a push;
3. member notices — device-key signed, fleet-key sealed, ALWAYS attributed to the authoring device by name.

**User-named threat that shaped it: the stolen member.** A stolen device still in the fleet holds a valid key → it can author member notices + read everything until remove+rotate. So: the inbox is NEVER a control channel (no message can steer a sibling with authority — authority stays on fleet ops/CLUTCH/manifest signatures), attribution is mandatory, and the inbox's value against theft is VISIBILITY (bind attempts elsewhere trip alerts fleet-wide), not prevention.

v1 slice = bind-attempt alert end-to-end. Then re-key notification ([[project_rekey_attack_surface]]) and update push ([[project_update_flow]]) ride the same rails. Loaner-recall waits on [[project_device_loaners]].
