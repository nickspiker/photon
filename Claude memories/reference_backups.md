---
name: reference_backups
description: "Code backup topology: nightly MEGA mirror (03:00 user timer, hard-fail + stamp) and the private github.com/nickspiker/keys repo for Code/keys"
metadata: 
  node_type: memory
  type: reference
  originSessionId: d83fbeaf-685c-4da4-8647-7b49de82fd2c
---

Code backups (fixed 2026-08-23 after a silent week-long gap):
- Nightly mirror /mnt/Harbor/Code → /mnt/Chiton/MEGA/Code: user units sync-code-to-mega.{timer,service} (03:00, Persistent), script Scripts/sync-code-to-mega.sh (gitignore-aware rsync). The week-long gap = ConditionPathIsDirectory still said /mnt/Octopus after the Octopus→Harbor mount rename — Condition skips are SILENT AND GREEN. Now: source check = loud ExecStartPre failure (only the MEGA mount stays a Condition), script skips tracked-but-deleted files (a rename-without-commit killed the whole mirror via rsync 23 + set -e), and writes $DST/.last-sync-completed as a staleness beacon.
- /mnt/Harbor/Code/keys → PRIVATE repo github.com/nickspiker/keys (created 2026-08-23, visibility verified PRIVATE). Commits/pushes there need --no-verify: the global handle-guard hook fail-closes on the pseudonym map, and this private repo IS the sanctioned home of those secrets. Push after key changes — it is a MANUAL push, nothing automates it yet.
