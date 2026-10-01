---
name: project_strip_selection_model
description: SHIPPED 2026-10-01 — a message selection is MANUAL only (a tap), never stolen by an incoming/sent message, cleared on entering a conversation; with none selected the newest row's strip shows unasked; the live age follows strip_target()
metadata:
  type: project
---

Nick 2026-10-01: "set a manually selected flag if the user taps on a message … an incoming or sent message does not clear the flag … navigating away clears it." Before: an incoming message in the open conversation FORCE-selected itself (2026-09-12 quick-reply rule, conversation.rs), and `selected_msg` was never cleared on leaving, so a row selected in conversation A left conversation B with NO strip (the explicit selection blocked the newest-row fallback without matching any row) — that was "the last message does not get automatically selected".

**Model now:** `selected_msg` IS the manual flag (only taps set it; strip actions reply/react/edit/wave-back/viewer/delete clear it as before). `open_conversation_with` clears selected_msg + strip_dismissed + selected_msg_copied, so every entry starts with the newest row's strip up unasked (sel_key in render / strip_target in conversation.rs). The force-select block is gone. live_readings/live_digit_edge (driver.rs) resolve thru strip_target() so the auto strip's age is live too. Scroll/zoom never touched the selection (verified).
