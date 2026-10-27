---
name: project_old_self_import_removal
description: The import of the old self conversation (→ a real atom, titled preferred name or "Self", English only) ships in v133 and is DELETED at v145 — grep "OLD-SELF IMPORT"
metadata:
  type: project
---

Nick 2026-10-27: "that import of old will be english only and version out in 12 versions, no reason to keep old non atoms around in the code."

Ships in v133 (commits 1bdb842b + the title commit). Every piece is tagged `OLD-SELF IMPORT — delete at v145` in code: found_self_atom + adopt_old_self_conversation (molecules.rs), the launch worker block (network/handle_query.rs), the launch and resume hooks (launch.rs, driver.rs), the self-row retirement migration (driver.rs), the self-contact refusals (devices.rs roster merge, launch.rs cloud merge), and the in-place id test (types/molecule.rs).
Title on import: the preferred name (profile.name), or the literal "Self" when blank — English only, never localized.

**Why:** old non-atom self conversations only need converting once; after a dozen releases every live device has converted.
**How to apply:** at the v145 deploy (or the first one after it), grep the tag and delete every tagged piece (and its now-dead helpers), then drop this memory. Related: [[project_conversation_convergence]], [[feedback_atoms_not_notes]].
