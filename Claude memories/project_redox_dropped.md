---
name: project-redox-dropped
description: 2026-10-15 Nick dropped the Redox target entirely (deploy, Cargo, cfgs, docs, site, CV); ferros is the first non-mainstream OS Photon will target
metadata:
  type: project
---

2026-10-15: Nick, after an honest look at Redox (no WiFi stack ever, aarch64 Tier 3 in rustc, no host tools): "let's ditch the redox target completely. From the holdmyoscilloscope site and all of it. ferros is likely the first non mainstream OS."
Removed: deploy.sh build/sign/R2/mirror legs, Cargo.toml redox tables, .cargo/config.toml target + CFLAGS, cross-libs/redox-features-shim, every `target_os = "redox"` cfg arm, renderer_redox.rs (dead), README/docs rows, the site's download link + platform tile + "Yes, Redox" line, the CV platform list.
Kept on purpose: deploy.sh comments citing the 2026-08-13 silent Redox abort (they explain why its guards exist); ferros pages saying ferros is based on Redox/RedoxFS (ferros lineage, not a Photon target); transitive redox_* crates in Cargo.lock.
The release manifest never listed Redox, so no updater depends on it; old redox binaries still sit on R2 and on GitHub releases ≤ v120.

**Why:** Redox stalled where it matters (wireless, ARM); carrying a build leg cost deploy time and cfg clutter for a platform nobody runs.
**How to apply:** never re-add a redox cfg arm or build leg; a platform beyond the mainstream five means ferros.
ferros context (Nick, 2026-10-16): ferros got WiFi and its full network stack up in about a week — the thing Redox never shipped — so a ferros Photon target has a radio from day one.
