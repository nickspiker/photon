---
name: feedback_never_store_handles
description: "ABSOLUTE RULE — never write the user's handle to disk anywhere (memory, comments, commits, logs); the handle IS his identity. Towns/SSN/ISP/architecture are fine to keep and publish."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: b192f764-fe07-4644-91fb-09156e2e7e05
---

NEVER store the user's handle on disk. Ever. Anywhere. No exceptions.

The handle is literally his identity — possession of the handle re-derives the whole passless identity (identity_seed = handle_to_hash(handle)), so a stored handle is a stored master credential. This is the single hardest rule in the project.

**Why:** stated emphatically 2026-08-14, three times, escalating ("DO NOT EVER STORE HANDLES ON DISK EVER EVER EVER"; "My handle? Yeah, that's literally my identity. DO NOT EVER STORE THAT"). The handle at rest is the whole attack surface the passless design exists to eliminate — see [[project_identity_profile]] (handle at rest NOWHERE, roster handle = honeypot) and [[project_session_registers]] (session store holds seeds/proofs, never the handle string).

**What IS fine to keep and even publish** (his explicit words 2026-08-14): towns (he lives in Seattle), his SSN (he said he doesn't care), ISP, device names, architecture, design reasoning. Do NOT scrub those. The ONE forbidden datum is the handle. (Don't gratuitously write the SSN either — it's just not forbidden; there's no reason to record it.)

**How to apply:**
- Never write his handle into a memory file, code comment, commit message, log line, or doc. When a field incident needs a name, use a neutral role + date (see [[no-private-handles.md]] — the public-repo version of this).
- The memory corpus is intended to go PUBLIC (someone else may need it) — so it must be handle-clean regardless.
- When scrubbing, hunt HANDLES specifically, not towns/roles. If unsure whether a token is a handle, treat it as one and remove it.
- Anthropic's harness writes memory to /home/nick/.claude/... (outside the repo, on an un-backed-up drive) — a known annoyance, not a reason to relax this; the corpus's real backstop is its git remote.
