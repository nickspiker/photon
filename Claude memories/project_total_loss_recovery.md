---
name: project_total_loss_recovery
description: Total-device-loss recovery = custodian-authorized chain SUPERSESSION (not edit); quorum-not-secret-share; always custodian-gated even with handle (else eviction backdoor)
metadata: 
  node_type: memory
  type: project
  originSessionId: 63c01259-bf21-462c-a8f7-66960ca42e2f
---

Total-loss recovery mechanics REASONED 2026-07-12 (design, nothing built; custodes crate is the home — see [[project_manifestus_custodes_split]]).
Problem: total device loss → no member device → consent-only removal ([[project_device_loaners]]) blocks re-entry (can't remove dead devices, can't pair a new one with no existing member). Resolution: SUPERSEDE the whole chain, never edit it ("break it, move on" applied to the fleet).

TWO LOAD-BEARING DECISIONS (lock these):
1. **Quorum-not-secret-share.** Custodians hold no secret / no Shamir share of your identity — they hold STANDING to co-sign a succession attestation. K-of-N signing "old identity → new identity" is a one-time bridge, not standing power. Rejects secret-reconstruction (K compromised custodians would silently BECOME you forever + read everything). Quorum model: rogue-K = a LOUD, alerted, reversible succession ATTEMPT that leaks nothing. This is the only model consistent with "vouch, never seize."
2. **Always custodian-gated, NEVER identity-key/handle-alone.** SHARPENED 2026-07-12 (user: "you won't forget your handle, and your friends have it"): the handle is PUBLIC by design — `identity_seed = BLAKE3(x(handle))` is a CHEAP hash, friends type+store your handle to add you, so EVERY friend can already derive your identity_seed. The only things stopping a friend from re-genesis-ing you are (a) first-come (live chain blocks it) and (b) they lack device_secret + S. In total-loss (a) is gone. So handle/identity-key-alone supersession would let ANY friend who holds your handle (= all of them) hijack you when vulnerable. Custodian-gating elevates "knows your handle" (everyone) → "authorized to move your identity" (K-of-N). NOT about a thief reading your shirt — about your whole friend list already holding the handle. Corollary: no custodians designated = total loss is TOTAL (unlinked fresh start).

MECHANICS:
- Custodian designation = identity-signed op IN THE CHAIN (persists on FGTW past device loss — you lose devices, not the chain), names N + threshold K. Open: commitment(hash) vs plaintext (trust-graph privacy); bilateral ack so a custodian knows their standing.
- Succession attestation = K-of-N custodian sigs over old_hp→new_hp, nunc-timestamped.
- Worker: verify K sigs against designated set, mark old_hp SUPERSEDED (whole-identity supersession, NOT device removal — consent-only stays pure), point to new_hp; new device does normal genesis on new_hp.
- Friend migration = custodian-AUTHORIZED but friend-CONFIRMED (prompt, not silent takeover); carries history + trust level; re-CLUTCH for fresh braid. Friends verify K sigs against the custodian set they learned folding the old chain.

RESOLVED 2026-07-12: recovery is ALWAYS same-handle (you never forget the handle — it's memorable; "handle lost" was a non-case). Same handle_proof, old chain superseded, fresh genesis at the same address, friends don't re-address you (they have the handle) — just accept recovered device-set + new S. Lighter than the earlier new-handle sketch.
OPEN (recommendations in parens): commitment(hash) vs plaintext custodian set (trust-graph privacy); friend-migration history-reassociation UX.
BONUS: this IS the stolen-EVERYTHING answer — don't fight the thief on the old chain, supersede off it; friends migrate; thief left holding a dead identity every friend refuses. Pairs with the friend-side trust override gap in [[project_token_private_identity]].
