---
name: project_pigeon_fetch_reach
description: "2026-10-07 Jeff's PDF: fetch asked only the friend's ONE device (fixed c3dd220c, every endpoint rotating); push is one-shot, auto-fetch policy open in TICKETS; blank-twin sibling row fixed"
metadata:
  type: project
---

Jeff (device 7e43f101) couldn't fetch Nick's PDF; the logs showed no fetch attempt at all, so the field failure stays unconvicted. Gaps found and fixed in code:
- `attach_fetch` targeted one device of the friend, so a file held only on the sender's other device was unreachable. FIXED c3dd220c: every friend `device_endpoints` entry (minus refused) joins the list; the retry still asks one per round, rotating.
- Push is one-shot (`attach_send_now`); receivers auto-fetch only image previews + sibling wave recordings — a missed push waits for a manual Fetch. **Policy open for Nick (TICKETS):** receiver auto-fetch under a size cap vs sender re-push.
- A PDF row's only verb is the strip's Save/Fetch pill; silent failure paths are now logged.
**Blank twin (fixed 2026-10-07):** `drain_pending_chain_sends` rebuilt the outgoing row from (text, stamp, reference) and pushed THAT to siblings, so an attachment row arrived as a blank row beside the file. Fix: push the STORED row; inserts fold a blank twin into its file row (`ChatMessage::is_bare`); `Conversation::collapse_bare_twins` hides stored ones at load.
Side note: `cargo test` writes into the live desktop log (ticketed).

**Why:** a file must be fetchable from any device that holds it.
**How to apply:** any path that rebuilds a row for the wire/fleet must carry attach/file/preview, or send the stored row. Related: [[project_pigeon_fetch_fanout]].
