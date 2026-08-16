---
name: persist-findings-early
description: "Nick edits/rewinds messages mid-session, which deletes ALL conversation context (not just post-edit) — persist load-bearing findings as they land, not at session end"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 749c1c88-4ac9-4f2f-9651-e136c93bcc59
  modified: 2026-08-08T08:46:36.689Z
---

Nick undoes by editing earlier messages ("edit one word and BOOP"). Confirmed 2026-08-08: this deletes ALL conversation context, not just everything after the edit point — the post-edit session starts completely cold. It has burned him repeatedly.

**Why:** message edits fork the transcript and the fork carries nothing over — anything not persisted outside the conversation is unrecoverable, including facts established before the edit point.

**How to apply:** when a session establishes a load-bearing fact or design decision (e.g. braid/chain semantics), write it to memory or repo docs at the moment it's established. Keep exploration cheap to replay: name the files that mattered, not just conclusions.
