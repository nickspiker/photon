---
name: feedback_never_store_handles
description: "ABSOLUTE RULE — never write the user's handle to disk anywhere (memory, comments, commits, logs); the handle IS his identity. Towns/ISP/architecture are fine to keep and publish."
metadata:
  type: feedback
---

NEVER store the user's handle on disk. Ever. Anywhere. No exceptions.
Possession of the handle re-derives the whole passless identity (identity_seed = handle_to_hash(handle)), so a stored handle is a stored master credential — the single hardest rule in the project.

**Why:** stated three times, escalating, 2026-08-14 ("DO NOT EVER STORE HANDLES ON DISK EVER EVER EVER"). The handle at rest is the attack surface the passless design exists to eliminate — [[project_identity_profile]], [[project_session_registers]].

**What IS fine to keep and publish** (his words 2026-08-14): towns, ISP, device names, architecture, design reasoning — don't scrub those. (His SSN is not forbidden, but there is no reason to record it.) The ONE forbidden datum is the handle.

**How to apply:**
- Never write a handle into a memory file, code comment, commit message, log line or doc. Name people by their petname per [[no-private-handles]] (the public-repo version of this rule).
- The memory corpus is public, so it must be handle-clean regardless.
- When scrubbing, hunt HANDLES specifically, not towns/roles; if unsure whether a token is a handle, treat it as one and remove it.
