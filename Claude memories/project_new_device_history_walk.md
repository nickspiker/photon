---
name: project_new_device_history_walk
description: 2026-10-21 Emma's NEW device stuck at "testing the secure channel" 3.5 h — braid strand miss on 40-day-old own rows; the urgent history walk was blocked 3 ways; relay-twin amber FIXED (c69e838a)
metadata:
  type: project
---

Incident 2026-10-20/21 (Emma's new Android device `34addd91` on Nick's LAN 192.168.1.159, release v0.121.0; Nick's desktop 90e571bf, MacBook fe46a74b):
- The label ("testing the secure channel" = clutch_step 11 = Complete && !chain_woven) was HONEST: the desktop's probe (and every frame from it) was braided to Emma rows ~40 days old written on her OTHER device; the new device logged `LANE: braid strand miss … holding this frame (no advance, no ACK)` for each retransmit until 00:52 UTC. Real messages already seal (conversation.rs ~1753) — no label fix needed; a "catching up on history" label belongs with the fix below.
- The strand miss DOES arm an urgent history walk, but it was blocked: (1) the new device rejected the MacBook: "device fe46a74b unknown to every friend contact (stale fold / era split)" — its view of Nick's fleet was stale; (2) MacBook pages failed: "page open failed … ours key#aa2b1faa theirs key#d7124177" → "walk PARKED … resumes when the key changes" (era/key split, cf. the open Mac era stall); (3) desktop pages arrived after 95 requests had expired → "DROPPED — rid unmatched and sender is not a fold-trusted sibling".
- AMBER (FIXED c69e838a): senders send ceremony frames direct AND relayed; the relayed twin (~130 ms later, from 0.0.0.0) set reached_via_relay unconditionally in the offer/KEM/proof handlers (status.rs) — now only when validated_path is None, like the pong handler.

FIXED 2026-10-21: stale friend fold → an unknown device answering for a known one (mismatched pong) refreshes that friend's fold, once per responder (photon 4459eee2, plus a "FLEET: X's fleet folded — grew to N" log); late history pages → the rid registry keeps minted rids 8× the 45 s in-flight timeout (2aaa55f5). The phonebook was dead too: see [[project_phonebook_strict_anchor]].
OPEN: the MacBook ↔ new-device history key split (ours key#aa2b1faa, theirs key#d7124177, walk parked) needs a fresh repro log; a "catching up on history with X" label for the strand-miss hold.

**Why:** Nick 2026-10-21: "Stuck on testing the secure channel even tho it works fine… then it went back to amber on my side."
**How to apply:** evidence = Emma's submitted log (photonlog --pull --handle, see [[reference_log_pull]]) + the desktop's /tmp/photon-nick log; related [[project_history_recovery]], [[project_braid_v2]].

Item 3 findings so far (2026-10-21):
- Nick's fleet chain is FINE: `pid-probe <handle>` folds 4 members (90e571bf desktop, 1be949c1, fe46a74b MacBook, cacbc223 Pixel). So Emma's new device was working from a stale / never-refreshed copy of Nick's device list ("ping from unknown device … stale fold on OUR side" for both the desktop and the MacBook at 21:22). A friend-fold success path logs nothing (protocol.rs drain), so whether/when it refreshed is invisible — add a log.
- NEW, possibly systemic: every phonebook record of Nick's devices fails verification — pid-probe "registry record: Unverified" ×4 and "pb_devices: fetch failed: record failed verification"; desktop log 268× "PHONEBOOK: seed lookup failed (record failed verification)" since at least 2026-10-20 19:34 (log start), Emma 5×. Raised by fgtw phonebook `view.verify(handle_proof)` (photon src/network/fgtw/phonebook_client.rs ~L153). No fgtw phonebook commits since 10-15, so possibly older. Address lookup feeds direct paths — a candidate for "no direct connection".
