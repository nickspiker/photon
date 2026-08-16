---
name: project_lifecycle_flows
description: "Identity/device lifecycle DESIGNED in docs/lifecycle.md 2026-07-17 (flow tree, canonical screen names, flood conventions); punch list in TICKETS; NOT built"
metadata: 
  node_type: memory
  type: project
  originSessionId: 5e7ea89e-050d-402f-8393-9069280cb926
---

docs/lifecycle.md is THE canonical map of the identity/device lifecycle (@ d173bbf): state model (device × identity × session), full flow tree, screen names (Launch/Claim/Attesting/KnownHandle/JoinerWords/JoinerSelected/SponsorAdd/SponsorConfirm/Ready/Panel/LastRites), conventions (flood states: amber=dev, green=Selected, red=LastRites; the interstitial pattern = press-again-proceeds/anything-else-cancels; event-shown notices per [[feedback_no_time_based_ui]]).

The three defects it grounds (all observed live 2026-07-17):
- **D1 collision**: a handle collider derives the SAME identity seed (handle→seed), so ProbeOutcome::JoinOurs fires for strangers too — undetectable by construction. Fix = KnownHandle screen speaking to both readers (taken-first), and NO bind request/beacon until "It's mine" (today it posts immediately = collision looks like pairing + spams the owner's registry).
- **D2 double-attest**: one device attested two handles. Device keys are fingerprint-only (NOT handle-salted) so the worker's one-owner-per-device gate (exists in fgtw-bootstrap source, device_owned + ownership index) WOULD catch it — the live fgtw.org worker predates it. Fix = redeploy worker (standing-authorized) + client device-binding marker at vault_key("device_binding", device_pubkey) sealed under device key, consulted by the probe BEFORE spending the proof (DEVICE BUSY line on Launch). Marker cleared only by wipe, not takeover.
- **D3 last-device exit — SUPERSEDED 2026-07-17 same-day by [[project_identity_never_dies]]**: the free-on-zero + total-purge ruling was reversed and LastRites CUT (shipped): the worker refuses zero-member folds, the client refuses last-device sign-out, brands survive departure, owner-consent device_release frees hardware. The genesis-hash pin SURVIVES the reversal (custodian supersession + defensive rendering still need it); friend-side not_found = ENDED-but-frozen rendering also stays. Backup NFC tag = a $1 fleet member (phone mints keypair, signs both add-halves, flashes seed, zeroizes; bearer instrument, no passphrase).

Build order (TICKETS punch list): worker redeploy → binding marker → KnownHandle → JoinerSelected green flood → collision counter/notifications (LastRites dropped from the list; Security-page rework added per the never-dies ruling).

Also that session: scripts/lib/snapbuild.sh — dev.sh builds from a btrfs reflink snapshot (Code/.build-snap, stable path, dynamic path-dep closure from Cargo.tomls, real target dir shared, destroyed on every exit); edits during a build can't tear it.
