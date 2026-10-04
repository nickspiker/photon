---
name: project_ideas_gripes
description: "Ideas & fixes page (Settings → Ideas) BUILT 2026-10-03: anonymous gripes with id = blake3(text ‖ nonce), stored photon-gripes/<id>.vsf + .status on FGTW R2; sender keeps ids device-locally (gripes.mine) and sees each status; developer side = photonlog --gripes / --gripe-status with keys/gripes.token (worker secret GRIPES_TOKEN)"
metadata:
  type: project
---

Canonical: docs/ideas.md. Client: src/ui/photon_app/ideas.rs (+ blob.rs gripe_*_blocking, state.rs SettingsPage::Ideas, render arm, lang Msg::Ideas*). Worker: fgtw-bootstrap handle_gripe_put/get/list/status (token-gated list/status), DATA_PREFIXES photon-gripes/.

**Decisions:** no identity on a gripe (no hp, no device key, no signature); the id is provenance of its own text only; statuses readable by anyone holding an id; developer token = keys/gripes.token (minted 2026-10-03, set as the wrangler secret); the entry box is single-line (a second MultiTextbox would need the compose box's whole key-routing — deferred). Worker tree had drifted 5 commits behind origin on this machine (lock-out/pq ports); fast-forwarded before deploy.

**How to apply:** set a status with `photonlog --gripe-status <id> <state> [note] [version]`; list with `photonlog --gripes`. Every VSF read on the client goes thru `SectionBuilder::parse_document` (the trust gate blocks hand parses). See [[project_update_flow]] for the release notes path.
