---
name: project_census_eats_copy_b
description: RESOLVED 2026-11-01 — Linux resilient-launch copy B moved from ~/.local/share/photon (census-swept photon_data_dir) to ~/.local/lib/photon; census stays strict
metadata:
  type: project
---
storage::census_sweep keeps only `<ring>.vsf`, its shadow and `photon.log.vsf` in photon_config_dir AND photon_data_dir. On Linux photon_data_dir is ~/.local/share/photon, where copy B of the resilient launcher used to live, so every launch deleted it (`STORAGE: census — photon-messenger deleted`, Nick's desktop 2026-10-31).
RESOLVED 2026-11-01: Nick: "Your call on the second launcher". Chose to MOVE copy B to ~/.local/lib/photon/photon-messenger (installers/photon.sh default, installers/install-release.sh, scripts/lib/desktop.sh, updates.rs resilient_copies, docs/resilient-launch.md) and keep the census strict — the data dir holds only the vault and the log.

**Why:** a binary is not data; loosening the census would reopen Rule -1's loose-file class.
**How to apply:** ~/.local/lib/photon is the approved copy-B location (Nick delegated the call); anything else new on disk still goes to Nick. Related: [[feedback_launcher_named_photon]].
