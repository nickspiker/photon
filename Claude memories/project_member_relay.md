---
name: project_member_relay
description: "BUILT 2026-10-25 (d4a2ffa8): group members hand each other's ORIGINAL signed frames (grp_relay) to member devices that are behind while the author is away"
metadata:
  type: project
---

Nick 2026-10-25: Steve sends, only Mike online; Steve offline, Jan online → Mike relays; Mike offline, Frank online → Jan relays.
Built: every device keeps received group frames as signed (`raw` on StatusUpdate::ChatMessage; `GroupFrame`, 256/group, RAM only). A member device's sync record (lane heads per label) behind a frame whose author's devices are not online → `relay_group_frames` sends it inside a `grp_relay` wrapper signed by the relayer (oldest first, 16 per record, 3 tries per frame per device, only on the peer's era). Receiver: `relayed_group_frame` verifies relayer + author and emits the chat update with NO presence flip / address adoption for the author.
OPEN: frames are not persisted (a relayer that restarts forgets them); gap-buffer replays lose `raw`; the chat signature covers provenance (token + prev hash), not the ciphertext, so any member holding the root could forge on another's lane — pre-existing, not widened by relaying.

**Why:** fleet-holds-everything for groups: a message must reach every member whoever is online.
**How to apply:** relay only ORIGINAL frames (never rows); any new group-frame path must keep `raw`.
