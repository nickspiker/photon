---
name: project_update_flow
description: "Self-update BUILT: signed manifest + stamp window, desktop swap+re-exec, Android PackageInstaller auto-install (2026-10-06), release-notice push; RELEASE_NOTES.md → Updates page + site"
metadata:
  type: project
---

Design: docs/updates.md.

**Invariants:**
- Verify-before-exec is the whole security boundary (the update channel is an RCE vector).
- STAMP WINDOW: accept iff `floor < t ≤ now`; t = the signed manifest header's creation stamp, floor = `PHOTON_BUILD_STAMP` of the running build (never mutable state). Forward-dated = "not yet", no skew parameter.
- `now` is staged: system eagle time on the happy path; the retained nunc verdict arbitrates any forward failure; no verdict → spawn consensus and defer ([[project_nunc_clock_check]]).

**Built:**
- Manual (Updates page): installs on ANY version difference, both channels (explicit hop/downgrade by user intent, not window-gated).
- Automatic: `drive_auto_update` polls the release channel every ~6-8 h (gated by `updates.auto`, default on). Desktop release builds self-apply + re-exec; dev builds are NEVER auto (Nick's mandate); the dev channel is never polled automatically.
- Android auto-install (2026-10-06, `on_auto_update_check`): release build + unmetered + no active wave + no contact mid-ceremony → PackageInstaller session (silent once photon is its own installer of record). Nobody re-attests after an update: the session capsule is boot-locked.
- Release-notice push: deploy.sh curls the worker → WS hub `release` event + FCM `updates` topic → immediate poll. Advisory only; the stamp window still gates installs.

**RELEASE NOTES:** `RELEASE_NOTES.md` at repo root, top section `## Upcoming`, shipped sections `## vN` (minor only). Compiled into the Updates page (src/ui/release_notes.rs); deploy.sh renders the newest three onto the site. Write Upcoming bullets AS FEATURES LAND: one plain sentence for the installer, never the builder.

Remaining: rollback-on-crash, in-flight textbox hand-off across exec.
Related: [[nick-publishes]], [[project_android_session_capsule]].

**2026-10-22 deploy shape (bf6a5c0e, Nick: "I'd prefer not to have half uploads"; no resume mode):** release binaries upload to R2 `photon/v<n>/…` (REL_PATH) — never over live names; manifest URLs, both installers (patched at build, guarded) and the site's download links point there; the switch = installers + manifest uploaded last. r2_put waits out a Cloudflare outage (cf_reachable) without spending attempts; the folder 3 releases back is pruned best-effort. Fixed R2 names froze at v124; README APK link → GitHub releases/latest/download. Dev publishes (scripts/publish/dev-*.sh) still overwrite fixed -development names.
