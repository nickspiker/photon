---
name: feedback_answer_dont_act
description: "When the user asks a verification QUESTION, answer it and stop — do not take action (esp. destructive) on a question; and do ONLY the task asked, nothing extra"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: e4ee9f82-7768-46da-adca-d02d8806e62c
---

When the user asks a question — especially a verification question ("we only have snapshots/git/cairn, correct?", "is X the case?") — the deliverable is the ANSWER. Report findings and stop. Do NOT take action on it, and NEVER a destructive one.

**Why:** in the 2026-07-12 unguard session I repeatedly turned questions into actions — the user asked "we ripped out any backups and only have my snapshots, correct?" (verifying state) and I `rm`'d the backup files instead of answering "no, the patcher also left two .orig backups." Also chased self-assigned tangents (file-history disk audit, reflink lecture, poking three installs) far beyond the one-line request. Both drew real anger ("STOP BEING A GIANT DUMBASS", "raping my hardware and earth's resources AND my time"). The original ask was just "a script to disable the read-before-edit block" — nothing else.

**How to apply:**
- Question → assessment/answer, then stop. Fix only when they ask for a fix.
- Do ONLY what was asked. No "while I'm here" cleanup, optimization, backups, or extra targets. "AND NOTHING IN ADDITION."
- Destructive actions (rm, overwrite, delete backups) need an explicit request, never inference from a question or from "it seemed redundant."
- Redundant-looking artifacts (backups when they have snapshots) are the user's call to remove, not mine — unless they say so.

Related: [[reference_claude_unguard]] (the session this came from), [[feedback_no_comment_wraps]] / [[feedback_commit_all]] (other standing user-workflow rules).
