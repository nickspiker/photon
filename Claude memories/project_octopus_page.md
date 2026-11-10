---
name: project-octopus-page
description: fgtw.org's admin dashboard is a Photon settings page opened by typing "Octopus" in the Diagnostics note; the worker only answers three signed VSF ops; no tokens, cookies or browser login
metadata:
  type: project
---

2026-11-10 (Nick): the server-side octopus capsule (toka bytecode assembled per request, 30 s, 1.4 MB, unauthenticated nuke_all) is GONE from fgtw-bootstrap, with its font, theme and the toka/spirix deps.
Photon draws the dashboard: typing exactly `Octopus` in Settings → Diagnostics → optional note opens an Octopus page at the end of the rail, RAM only (src/ui/photon_app/octopus.rs, render arm in render.rs).
The worker answers `octopus_view` (bucket counts + activity log as repeated typed sections), `octopus_clear {hp, bucket}`, `octopus_nuke {hp}`; clears are budgeted at 1<<9 deletes and answer `more` (network/fgtw/octopus.rs loops).
**Auth, Nick's words:** "no auth whatsoever other than knowledge of Octopus, and of course a working Photon that's attested" — a device-signed VSF doc from a current member of the fleet it names, created within 60 s. No admin identity, no token.

**Why:** everything is framed VSF; passwords, cookies and sign-in pages are phlogiston (passless.org). I built a password page + cookie first and was rightly told off: read passless.org before inventing auth.
**How to apply:** never add a token, cookie, HTML login or browser-side admin; admin = a signed VSF op from an attested device. Maintainer acts (release notice, an Idea's status) are VSF ops signed by the release key (photon-manifest --release-notice, photonlog --gripe-status via crypto::self_verify::release_signing_key), checked against RELEASE_PUBKEY in the worker; the /admin routes and the ADMIN_TOKEN / GRIPES_TOKEN secrets were deleted 2026-11-10. Related: [[project_peers_are_fgtw]].
