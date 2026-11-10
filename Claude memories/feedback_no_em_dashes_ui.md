---
name: feedback_no_em_dashes_ui
description: "UI text carries no em dashes; a line return where the text wraps, plain punctuation where it is drawn on one line"
metadata:
  node_type: memory
  type: feedback
  originSessionId: 17b90e8e-8b1a-448e-8ac7-22b057e9a0d7
  modified: 2026-11-10T23:17:49.926Z
---

No em dashes in user-visible text, in any language catalog (Nick 2026-11-10: "can we not put em dashes everywhere? Line returns do just fine").

**Why:** they crowd the text, and long one-line toasts clipped on both sides.

**How to apply:** where the string goes through a wrapping draw (settings Flow, `draw_toast`, the launch error band, `centered_wrapped`, i.e. `wrap_text_lines`), split with `\n`. Where it is drawn on one line (pills, chat rows, the wave bar, Add/Join status rows), use a colon, comma, period or ` · `. A `\n` in a single-line draw does not break. Comments and log lines are not covered by this rule.
