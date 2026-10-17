---
name: project-fgtw-key-desync
description: "CLOSED 2026-08-14 — fgtw.org bricked every client: the desktop deployed the worker with a stale per-machine key.rs; deploy.sh now refuses a key.rs whose publics differ from photon's pins; canonical key lives in keys/"
metadata:
  type: project
---

2026-08-14: every client failed attest ("not authentic FGTW") because the desktop deployed the worker with its stale pre-rotation `fgtw-bootstrap/src/key.rs` (the FGTW identity was rotated 2026-07-30 on the MacBook). Un-bricked by `wrangler rollback`, then the canonical pair was recovered from the MacBook's deploy copy and redeployed.

**Guard (fgtw-bootstrap bcb830d):** deploy.sh refuses any worker deploy whose key.rs publics ≠ photon's pinned `FGTW_ED25519_PUBLIC_KEY` / X25519 constant, and verifies the live challenge after deploy.
**Key custody (Nick's doctrine):** a key not in /mnt/Harbor/Code/keys → Chiton → MEGA does not exist. Canonical FGTW secret: keys/fgtw-seed-key.rs (0600) + the desktop's gitignored fgtw-bootstrap/src/key.rs.

**Why:** key.rs is gitignored and PER-MACHINE, so a worker deploy silently ships whatever key that machine holds; the MEGA mirror had stalled and ate the MacBook's copy.
**How to apply:** trust the guard; never bypass it. Residue as of 2026-08-14 (unverified since): the Chiton→MEGA mirror was stalled, and the MacBook's key.rs lacked the X25519 const (guard refuses deploys from there). Related: [[project_two_machine_git_divergence]], [[reference_backups]].
