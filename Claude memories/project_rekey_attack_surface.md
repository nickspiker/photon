---
name: project_rekey_attack_surface
description: "Re-key/history-injection threat model (docs/rekey-threat-model.md): only a trusted device key can force a re-key; revocation works via fold-respecting trust (2026-07-09); MAC-in-ACK buildable, unbuilt"
metadata:
  type: project
---

Canonical: docs/rekey-threat-model.md (+ docs/device-lifecycle.md §3).
- A re-key is implicit: a ClutchOffer with keys different from what we completed ⇒ nuke chains + zeroize the history key. Friction-free by design — that's the surface.
- A keyless spoofer can't force one (the offer must be signed by a device we trust for that friend). An attacker WITH a trusted device key can DoS (force re-keys) and INJECT fabricated history (rows stored recovered = true, add-only, no UI cue) — but can't read old messages or impersonate to third parties.
- **Revocation works (2026-07-09, ae5f509 + 9360e7f):** `knows_device` / `answerable_pubkeys` are FOLD-RESPECTING — once a contact's chain folds, only current members are trusted, the first-met `public_identity` included; a fold failure never arms trust-nobody. The CLUTCH gates call `knows_device`. Freshness is monotonic (chain-tip eagle time; a stale read can't resurrect a removed device).
- **MAC-in-ACK** (tag each message pair with spaghettify(message ‖ secret), reject recovered rows that don't match) became buildable with the friend-blinded identity S ([[project_token_private_identity]]); not built.
- No portable user secret exists (seeds derive from the handle); only an off-device secret (memorized / PIPE) would stop injection after device theft.

**Why:** device theft is the realistic attacker, and re-keys are friction-free on purpose.
**How to apply:** trust decisions go thru `knows_device`, never the first-met pin; the re-key notification is event-shown ([[feedback_no_time_based_ui]]). E2E on real devices still owed.
