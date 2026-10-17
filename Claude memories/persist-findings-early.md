---
name: persist-findings-early
description: "Nick edits/rewinds messages mid-session, which deletes ALL conversation context (not just post-edit) — persist load-bearing findings as they land; a rewind erases context, not the repo"
metadata:
  type: feedback
---

Nick undoes by editing earlier messages ("edit one word and BOOP"). Confirmed 2026-08-08: this deletes ALL conversation context, not just what follows the edit — the post-edit session starts cold.

**Why:** an edit forks the transcript and the fork carries nothing over; anything not persisted outside the conversation is gone, including facts established before the edit point.

**How to apply:** write a load-bearing fact or design decision to memory or repo docs the moment it is established; name the files that mattered, not just conclusions.
The other half (2026-08-23): a rewind erases CONTEXT, not the REPO — commits, pushes and working-tree state survive. Before re-implementing anything after an apparent reset, check `git reflog` / `git log` against the last thing context can account for (a session nearly re-did three already-pushed fixes).
