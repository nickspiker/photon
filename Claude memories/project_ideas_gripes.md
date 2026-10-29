---
name: project_ideas_gripes
description: "Ideas & fixes page (Settings → Ideas) BUILT 2026-10-03; PUBLIC list + likeness-ranked bucket check + ideas.ticker (device-local, off) 2026-10-28: anonymous gripes with id = blake3(text ‖ nonce), stored photon-gripes/<id>.vsf + .status on FGTW R2; sender keeps ids device-locally (gripes.mine) and sees each status; developer side = photonlog --gripes / --gripe-status with keys/gripes.token (worker secret GRIPES_TOKEN)"
metadata:
  type: project
---

Canonical: docs/ideas.md. Client: src/ui/photon_app/ideas.rs (+ blob.rs gripe_*_blocking, state.rs SettingsPage::Ideas, render arm, lang Msg::Ideas*). Worker: fgtw-bootstrap handle_gripe_put/get/list/status (token-gated list/status), DATA_PREFIXES photon-gripes/.

**Decisions:** no identity on a gripe (no hp, no device key, no signature); the id is provenance of its own text only; statuses readable by anyone holding an id; developer token = keys/gripes.token (minted 2026-10-03, set as the wrangler secret); the entry box is single-line (a second MultiTextbox would need the compose box's whole key-routing — deferred). Worker tree had drifted 5 commits behind origin on this machine (lock-out/pq ports); fast-forwarded before deploy.

**How to apply:** set a status with `photonlog --gripe-status <id> <state> [note] [version]`; list with `photonlog --gripes`. Every VSF read on the client goes thru `SectionBuilder::parse_document` (the trust gate blocks hand parses). See [[project_update_flow]] for the release notes path.

**2026-10-28 — public + ticker (Nick: "Always public, it's free software for everyone, I don't hide anything unless it lets someone steal from me"):** gripe_list is tokenless and typed (gripe_list_ack{count} + one `idea` section per gripe; the tab-row string and its reader are gone; the client errors when count ≠ ideas read). gripe_put (new) and gripe_status broadcast hub `idea`; clients with `ideas.ticker` on (or the page open) fetch and notify on ids new since the session baseline. The page ranks the public list by shared words (≥3 letters) while typing ("is it already in the bucket"). OPEN: an FCM topic for the ticker (phones in deep Doze). The fgtw deploy also rebuilds toka — its fluor requirement must follow fluor's version bumps.
