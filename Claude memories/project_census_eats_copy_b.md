---
name: project_census_eats_copy_b
description: 2026-10-31 OPEN — the storage census deletes the resilient launcher's copy B (~/.local/share/photon/photon-messenger) on every desktop launch; needs Nick's call on where copy B lives
metadata:
  type: project
---
storage::census_sweep keeps only `<ring>.vsf`, its shadow and `photon.log.vsf` in photon_config_dir AND photon_data_dir, deleting everything else. On Linux photon_data_dir is ~/.local/share/photon, exactly where scripts/lib/desktop.sh (and the release installer) put the resilient-launch copy B (docs/resilient-launch.md). Nick's desktop logs `STORAGE: census — photon-messenger deleted` at every start (20:59, 21:29, 21:45 on 2026-10-31), so the `photon` launcher's fallback copy never survives a launch. Found while installing chameleon there (it was swept too; it now goes beside ~/.local/bin only).

**Why:** two of Nick's rules collide — the census (only the vault and the log, Rule -1) and the resilient launcher's second copy under a different XDG root.
**How to apply:** don't "fix" by allowlisting without asking — a new allowed file is Nick's decision (Rule -1). Options to put to him: move copy B to its own dir outside the data dir (e.g. ~/.local/lib/photon/), or let the census keep the signed binary. Related: [[feedback_launcher_named_photon]].
