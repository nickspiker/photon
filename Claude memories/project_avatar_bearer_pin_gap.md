---
name: project-avatar-bearer-pin-gap
description: avatar on FGTW = 64-byte bearer pin (key‖lookup) rotated only on avatar CHANGE — removal never yanks it; fix rides removal-rotates step 1
metadata: 
  node_type: memory
  type: project
  originSessionId: f4272721-c713-4a82-a97a-db8106029756
---

User-reported 2026-07-23, confirmed in code: the avatar blob on FGTW is sealed under a 64-byte bearer pin — `avatar_pin[..32]` = AES key, `avatar_pin[32..]` = keyless wall lookup (avatar.rs `download_avatar_pinned`).
The pin rotates ONLY when the user changes the avatar (`ensure_avatar_pin` rotate-on-set, photon_app.rs ~1262: fresh pin, re-upload, delete old slot, stamp bump); it is distributed to friends via pong and to the fleet via fstate settings `profile.avatar_pin`.
So it sits outside the wairua/rotating-key hierarchy: a removed device (or ex-friend) that ever held the pin keeps fetching+decrypting the avatar forever — "an old key that can't get yanked."
Fix = rotate the pin on fleet-membership shrink by invoking the existing rotate-on-set machinery without a new image; planned as step 5 of the rotation flow in plans/fleet-plane-step1-removal-rotates.md.
Ex-FRIEND revocation of the pin is a separate open (rides the re-key/friendship layer, [[project-rekey-attack-surface]]).

Related: [[project-fleet-braid-plane]], [[project_avatar_encryption_wall]].

**CLOSED:** pin-rotate on membership shrink shipped inside removal-rotates step 1 (2026-07-23, braid.md §14.12: "avatar bearer pin rotated by the winner").
