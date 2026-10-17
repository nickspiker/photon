---
name: reference-log-pull
description: "pull submitted device logs: photonlog --pull --session (own machine) or --handle <LEFT-column map bytes>; pull to a file first; use target/debug/photonlog; desktop local log = tmpfs soft-mode"
metadata:
  type: reference
---

**Pulling:**
- `photonlog --pull --session` pulls THIS machine's submitted logs from the live tohu registers — preferred, no handle typed.
- `photonlog --pull --handle <handle>` for anyone else. **MAP-COLUMN TRAP:** `keys/claude-pseudonym-map.txt` is `<handle> = <petname>` — the secret handle is the LEFT column. Passing the petname derives a garbage tag that "finds nothing" (cost an hour, 2026-08-21). Extract the left column mechanically into a shell var; never echo it.
- PULL TO A FILE FIRST (`> /tmp/x.log`), then grep: submissions are swept within 24 h and can vanish between pulls.
- Use `target/debug/photonlog` after `./scripts/dev.sh` (release builds are disabled; `target/release/photonlog` is stale). Gripes: `--gripes` / `--gripe-status`.
- One identity covers every device's submissions; submissions are whole-log snapshots split by `── photon-logs/<tag>/<eagle>-dev<id>.vsf ──`; the log rotates at 16 MiB.

**Local desktop log:** volatile tmpfs `/tmp/photon-<user>/photon.log.vsf` (decode with photonlog, `-g` grep, `-l` level). Soft mode batches in RAM and reaches disk only on edges (panic/background/submit/threshold/quit/update re-exec); a successful submit clears it by design.

**Nick's devices** (Nick = the user's test identity, handed over for debugging 2026-07-23 — delete this line if asked): desktop Leviathan 90e571bf; MacBook fe46a74b; Pixel cacbc223; 1be949c1 = the old, locked-out phone (ghost).

**Naming traps:**
1. Ceremony/punch/status lines print `fp(contact.handle_proof)`; a SIBLING contact carries OUR OWN hp, so sibling ceremonies log as "our own hp".
2. CHAT lines print the pid pseudonym for siblings, not the fleet-page device name — two naming systems, same hardware.
3. Ground-truth pids with `src/bin/pid-probe.rs` against the map (never commit a handle) — trust it over any hand-written label.

Android landmine: changing the APK signing key rotates ANDROID_ID → new device key → the device needs re-pairing (Emma's phone, 2026-07-23).
Log tag `CALL:` became `WAVE:` on 2026-09-23.
