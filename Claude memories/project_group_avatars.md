---
name: project_group_avatars
description: Group (molecule) avatars BUILT 2026-11-02 — AvatarRecord in the roster names an AV1-in-VSF blob members fetch like group files; Manage page big avatar
metadata:
  node_type: memory
  type: project
  originSessionId: d6c9db89-ccef-475b-b38b-64f51976930f
  modified: 2026-11-02T16:16:17.083Z
---

Group avatars BUILT 2026-11-02 (Nick: "molecule avatars, drag/drop onto the molecule info page with the big avatar with the hover hint to drag/drop or tap on adroid to choose"). Field test pending.

- Record: `AvatarRecord` (types/molecule.rs), newest wins, any member, `image: None` = removed; roster codec section `avatar` (storage/molecule.rs), older builds ignore it.
- Picture: avatar_plaintext (AV1 in VSF) stored with blob_store; members fetch it through replicate_sweep (group conv's hashes + roster avatar_image), any network.
- App: src/ui/photon_app/group_avatar.rs — set_molecule_avatar (worker encodes, record posted only after the blob is stored), remove, drain_group_faces, group_faces_tick (edge: persist_molecule / blob landing / presence answers), scale/face_at per diameter.
- UI: Manage page rows 1-5 big avatar (slot 7), hover hint row 6, Remove pill row 7 (slot 8); Bonds rows moved to slot 9. Ready row + conversation header wear it. Android tap → avatar_pick_for routes the picker result.
- Decoded picture must be AVATAR_SIZE square or it is refused (pie stays).

**Why:** groups looked alike (member pie only).
**How to apply:** when touching the Manage page or conv panel slots, keep SLOT_* in conv_panel.rs the one source; see [[project-molecules-build]].
