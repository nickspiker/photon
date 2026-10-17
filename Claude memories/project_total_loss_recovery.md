---
name: project_total_loss_recovery
description: "Total-device-loss recovery = custodian-authorized chain SUPERSESSION (not edit); quorum-not-secret-share; always custodian-gated even with handle (else eviction backdoor); not built"
metadata:
  type: project
---

Designed 2026-07-12, nothing built. Canonical: docs/device-trust-and-recovery.md "Total-loss recovery".
Total device loss leaves no member to consent, so the whole chain is SUPERSEDED, never edited.

**Two locked decisions:**
1. **Quorum, not secret share.** Custodians hold no secret or Shamir share — only standing to co-sign a K-of-N succession attestation (old → new). A rogue quorum is a LOUD, reversible attempt that leaks nothing; secret reconstruction would let K custodians silently become you forever.
2. **Always custodian-gated, never handle-alone.** The handle is public (every friend can derive identity_seed), so handle-alone supersession would let any friend hijack you while you are down. No custodians designated ⇒ total loss is total.

Mechanics: custodian designation = identity-signed op in the chain (survives device loss); recovery is always SAME-handle (old chain superseded, fresh genesis at the same address); friends verify the K sigs and CONFIRM the migration (prompt, never silent), then re-CLUTCH.
Bonus: this is also the stolen-everything answer — supersede off the chain the thief holds.
Open: custodian set as commitment vs plaintext (trust-graph privacy).
Related: [[project_token_private_identity]], [[succession-emit-side-unwired]] (docs/identity-succession.md).
