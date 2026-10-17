---
name: feedback_answer_dont_act
description: "When the user asks a verification QUESTION, answer it and stop — do not take action (esp. destructive) on a question; and do ONLY the task asked, nothing extra"
metadata:
  type: feedback
---

A question — especially a verification question ("we only have snapshots, correct?", "is X the case?") — gets the ANSWER. Report findings and stop. Do NOT act on it, NEVER destructively.

**Why:** 2026-07-12 I turned questions into actions — asked "we ripped out any backups and only have my snapshots, correct?", I `rm`'d two .orig backups instead of answering — and chased self-assigned tangents far beyond a one-line request. Real anger followed; the ask was "AND NOTHING IN ADDITION."

**How to apply:**
- Question → answer, then stop. Fix only when asked for a fix.
- Do ONLY what was asked: no "while I'm here" cleanup, optimization, backups or extra targets.
- Destructive actions (rm, overwrite, delete backups) need an explicit request, never inference from a question or "it seemed redundant"; redundant-looking artifacts are the user's call.
