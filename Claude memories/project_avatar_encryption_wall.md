---
name: project_avatar_encryption_wall
description: "Avatars on FGTW are sealed under a 64-byte bearer pin (key ‖ lookup) only friends and the fleet hold; FGTW/admin can never decrypt them; pin rotates on avatar change and fleet shrink"
metadata:
  type: project
---

An avatar on FGTW is AV1-in-VSF sealed under the 64-byte avatar pin: `avatar_pin[..32]` = the key, `avatar_pin[32..]` = the keyless storage lookup (src/ui/avatar.rs).
The pin reaches friends via pong and the fleet via fstate `profile.avatar_pin`; FGTW stores ciphertext it cannot read (server compromise leaks no faces).
Rotation (`rotate_avatar_pin`, settings.rs): on avatar change, and on fleet-membership shrink by the heal winner (braid.md §14.12). OPEN: an ex-FRIEND keeps a pin until the next rotation (rides the re-key layer, [[project_rekey_attack_surface]]).
Fixed 2026-07-23 (b5fe9c6): the mint waits for the first fleet-state pull (probe-then-generate), else each device minted its own pin, LWW never converged and every pin change re-downloaded avatars.

**How to apply:** the octopus admin console can never show avatars, and must never be given keys; for a per-row visual there, use identicons from public data (device_pubkey / handle_proof).
Built 2026-07 and deployed anyway: a browser AV1 path (vsf `image-decode` feature over rav1d, vendored rav1d + libc shim at toka/vendor/ for wasm32-unknown-unknown, fluor::paint::draw_image, toka `draw_image` opcode + `/resource_get`) — reusable by pointing draw_image at decryptable bytes.
