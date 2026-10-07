---
name: project_pigeon_fetch_reach
description: "2026-10-07 Jeff's PDF 'not fetching': fetch asked only the friend contact's ONE device (fixed c3dd220c — every known endpoint, one per ask, rotating); pigeon push is one-shot with no receiver auto-fetch for files (policy open in TICKETS); logs held no fetch attempt at all"
metadata:
  type: project
---

**Report (Nick 2026-10-07):** Jeff (device 7e43f101, on v0.111.2 dev) can't fetch a .pdf Nick sent; both submitted logs.

**What the logs proved:** Jeff's log covers only his 00:41-00:56 session (app opened 00:41, sent a photo 00:45, 10-min wave, submitted) — ZERO `attach: fetch` lines; Nick's phone (21:42-00:56) and desktop (15:44-00:59) logged ZERO `ATTACH:` request outcomes from Jeff (the server logs every outcome, even refusals). So no request ever left Jeff in the logged window, and the send itself is outside every log (not on the phone or desktop logs — likely the MacBook fe46a74b or earlier).

**Gaps found in code (not field-convicted for this PDF):**
- `attach_fetch` targets = each contact's `race_addrs()` + `device_key()` — ONE device of the friend (the one in their hand). A file held only on the sender's other device was unreachable. FIXED c3dd220c: friend `device_endpoints` (minus primary + refused) join the list at their own addr + relay leg; the 20 s retry tick still asks one device per round, rotating thru the list. Server side already serves any fold-trusted requester (`knows_device` + `attach_wire_key` from replicated chains).
- Push is one-shot (`attach_send_now`); `attach_confirmed` is bookkeeping only; receiver auto-fetches only image previews + sibling wave recordings → a missed push waits for a manual Fetch tap. Policy decision for Nick in TICKETS (receiver auto-fetch under a size cap vs sender re-push).
- A PDF row has NO tappable visual (visuals = images, audio band, text/code) — its only verb is the strip's slot-4 Save/Fetch pill; strict `blob_present` there → fetch (logged). Silent paths left: `SaveFailed` (now logged), `attach_fetch` early returns (no token/keypair/checker/frame).

**Side findings:** `cargo test` writes into the live desktop log (fake "PIGEON: reassembled bytes do not match" lines) — ticketed. Nick's phone is buffering a head gap on Jeff's lane (hp 9c3ecae3, "expected prev 7eb582a5…").

Related: [[project_pigeon_fetch_fanout]], [[project_attachments]], [[reference_log_pull]].
