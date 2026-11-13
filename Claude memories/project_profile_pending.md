---
name: project_profile_pending
description: 2026-11-10 why set names/avatars show "Pending…": no owner-signed revision, last-heard wins, nobody asks; findings reported, fix = fgtw Profile record (FLOWS.md stage 7)
metadata:
  type: project
---
Owner rule (2026-11-10): a profile field (name, avatar) is a record signed by its owner with a revision; any holder of a newer signed copy passes it on (owner's devices when older/missing, anyone showing Pending); a device with none asks people/devices it knows (BLE, LAN, WAN, fgtw.org last). "Pending" = no copy anywhere it asked.

Findings (traced, nothing fixed yet; no (b) store loss or (c) verify refusal proven):
0. Group name grants frozen/empty: molecules.rs own_name_grant reads profile.name only at join/found, avatar always [0;32]; never re-signed on change; render.rs:184/3302 don't fall back to the contact's name.
1. Roster merges whole entries on one `updated` clock that consent/ceremony/era/weave bumps too (fgtw fstate::merge_rosters, devices.rs ~1832): a blank/old name wins fleet-wide; fresh-device stubs copy blanks.
2. No revision anywhere: PongTail.display_name is a bare String; status.rs ~972 adopts any different name stamped now; avatar-blob decode overwrites published_name (conversation.rs ~3324) incl. cached copy each boot.
3. Nobody asks or forwards: only the person's own device's pong carries it; pongs answered by another device drop name/pin (status.rs ~3814; 1,345 such in one desktop log); no profile_req op exists; announce reaches one public_identity device; non-contact group members never pinged.
4. Friend avatars resolve only on the device that ran CLUTCH with that friend (scoped_avatar_readers + fanout_pairs per device); network fallback removed; failed fetch latched per session.
5. Own avatar re-stamped with write time on adopt (avatar.rs ~1239/1257): adopt/publish ping-pong (15+15 in one log).
6. Cloud contacts backup overwritten whole; restore fills only missing contacts, never blank names.

**How to apply:** the fix is the Profile machine/record in fgtw (FLOWS.md stage 7, owner-approved direction [[feedback_fgtw_out_of_photon]]): ProfileRecord (party, field, value, revision, signer, sig verified vs fold), newest-per-field merge, per-field roster, ask/answer ops, reader grant per folded device; Photon carries it (pong tail, member record, backup), forwards, asks, falls back. Wait for the owner's go after the findings.

**Owner go, 2026-11-13:** "Please. then once we get it resolved, run a full deploy.sh for all." Order: Brittany findings -> fix the own-fleet key/era split -> build the Profile record (fgtw first, per [[feedback_fgtw_out_of_photon]]) -> full deploy.sh (Nick asked in that message; the deploy waits until these are resolved and tested). Trigger seen: Brittany's flower avatar now the Mandelbrot placeholder on Nick's side; names/avatars disappearing generally.

**P1 landed 2026-11-13 (fgtw 3329870 + 39c516a, photon 9d7916f6, unverified in the field):** records for name and avatar pin, signed Ed25519-only (pong size; full eggs need another carrier), held in fleet settings profile_record.*, carried as `prof` rows in the pong tail beside bare fields, adopted only if newer and the device is in the contact's current fold, kept through pongs answered by another device, roster/avatar-blob never overwrite a recorded field, adopted avatar keeps its stamp, contacts persist records. Bare-field fallback pinned to 2027-02-01. Open (P2): records in the roster codec, profile_want/hand-on, friend avatars on every device, group names, backup restore; fold check ignores revision time.
