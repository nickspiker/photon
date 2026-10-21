---
name: project_phonebook_strict_anchor
description: 2026-10-21 CONVICTED + FIXED — every phonebook record "failed verification" for a month (fgtw strict anchor rule vs 3-scheme eggs); fgtw 0bb51f3 declared rule, photon aa94a23d names the cause
metadata:
  type: project
---

From fgtw d5a6ff2 (2026-09-20, records point at an eggs blob) devices signed phonebook records with Ed25519 + Falcon + SPHINCS+, while photon's reader (`fetch_devices` → `RegistryView::verify`, `fetch_address` → `rec.verify_address`) judged them under `EggPolicy::anchor_only()` with the STRICT `pq::verify_eggs` ("every listed egg must verify") — no Falcon key ⇒ reject. Every lookup of a full-bundle fleet failed: Nick's 4 devices + count record, Emma's lookups, 268× "PHONEBOOK: seed lookup failed (record failed verification)" on the desktop. Devices fell back to gossip / LAN multicast / relay for addresses — a likely contributor to "no direct connection" / amber.

NOT the vsf v11/v12 builds (Nick suspected them): the failure was reader-side and predates them; the egg codec never changed.

Fix (Nick 2026-10-21, option 1): fgtw `verify_list` uses `verify_eggs_declared` — eggs the reader holds a key for must verify, the rest skip, verified eggs must cover the policy floor (anchor-only = the old Ed25519 assurance; the worker's chain policy still enforces the fleet floor on put). Proven live: `pid-probe <handle>` now returns verified addresses for 3 devices + pb_devices rows; only the ghost 1be949c1's pre-eggs record (empty blob) is rejected, correctly. Option 2 (strict PQ verification with the chain's declared bundles where a fold is held) is the later upgrade.

Clients pick it up with their next photon build (fgtw is a path dep); no worker redeploy needed.

**Why:** Nick 2026-10-21: "WTF is going on with the phonebook?"
**How to apply:** diagnose with `target/debug/pid-probe <handle>` (handle from keys map, never echoed; see [[reference_log_pull]]); PhonebookError::Unverified now carries the failed check. Related [[project_new_device_history_walk]].
