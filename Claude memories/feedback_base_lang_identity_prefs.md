---
name: feedback-base-lang-identity-prefs
description: Owner 2026-11-22 — number base and language are IDENTITY preferences (the identity's space, fgtw), not Photon fleet settings; one home for every app
metadata:
  type: feedback
---

Owner, 2026-11-22 (via the ferros session, verbatim): "That's an identity preference, not Photon. Same with language."

The number base and the UI language belong to the identity's own space (ferros_space carries LANG; BASE is being added beside it), not Photon's fleet `display.base` / `display.lang`. On ferros, Photon follows the device through `set_host_numerals` (photon c7935249), and the device follows the identity.

**Why:** they're the person's preferences across every app on every device (see [[project-one-identity]]), and identity logic lives in fgtw ([[feedback-fgtw-out-of-photon]]).
**How to apply:** format = ferros_space's (BASE = tatau code 10 / 12 DMS / 16 / DLS as a u, read width-agnostic; LANG = vsf wl ISO 639-3); readers and writers in fgtw. Migration still OPEN: Photon desktop and Android move display.base/display.lang into the identity's space, read it through fgtw, then retire the fleet keys with a dated pin ([[feedback-pins-are-dated]]). Never add a new Photon-local base or language store.
