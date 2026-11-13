---
name: project_brittany_late_delivery
description: 2026-11-12/13 photons to Brittany late or "not delivered" while she is online: relay dropped for a mis-credited direct path, own-fleet era split, re-served rows cleared without her ACK; all pre-v151 code
metadata:
  type: project
---
Owner report 2026-11-13 01:40 UTC. Desktop = device 469e79b8 (also the 2026-11-11 beam sender, [[project_group_beam_2026_11_11]]). Brittany: phone 29ad7ff5 on Verizon cellular (IPv6 ...b379 / ...10d8, CGNAT v4), second device 9ffdecc9 on Nick's LAN 192.168.1.172 (relay closed). Conversation fid 328777d3. No Brittany log submitted. Notes: target/brittany-findings.md (not committed). Findings, not fixed:
0. 01:39 photon (typed on MacBook fe46a74b, relayed by the desktop): sent 8x to ...10d8 only, bytes received, no delivery ACK, "retransmit GAVE UP (undelivered)". Likely her chain on this lane fell out of step after 4 old rows were re-served to her at 01:37-01:38 (triggered by the MacBook's status lacking the desktop's lane: status.rs ~707-787), cleared as "implied-delivered by the sync record" without her ACK (clear_pending_up_to, status.rs:611), while a receiver ACKs-and-skips an out-of-order old row (status.rs ~1474). Recovery only after 2 stuck rounds (~617). Unproven without her log.
1. Relay dropped by a mis-credited direct path (proven 21:05 send, 25 min late): punch-probe answer handler (src/network/status.rs ~4355) credits the address probed even when the answer came from another address; relay_unless_direct_trusted (photon_app.rs:4572) then drops the relay; pt send_tagged (~386) silently refuses a dead address -> nothing sent anywhere.
2. Own-fleet era split (proven 19:20 send, 1 h 36 min late): 154 of Brittany's frames dropped "stale/unknown era" 14:34-20:56; "era REFUSED ... two live eras exist" between Nick's devices 16:01-20:57. Same class as [[project_emma_bond_era_split]].
3. Inbound from Brittany: "LANE FORK garbage decrypt" 22:10-23:37, "this device does not own the ceremony, awaiting sibling chain-sync".
Side: Nick pings c9b5f417, a Brittany key no longer in her fleet (~880 lines); her phone asked history for 4 unknown conversations ~1,380 times.

**How to apply:** messaging code (Photon), not FGTW. Ask the owner for order; get Brittany's log for #0. None of it is from v151 or this week's changes.

**Landed 2026-11-13 (photon e0953fce, unverified in the field):** fixes #1 (answered_path credits the datagram source; Contact::direct_confirmed keeps the relay copy until a direct-path delivery ACK; PTManager::refuses -> relay fallback for chat/ACK/history/ceremony) and #0 (re-serves only on the friend's own report; clear/rearm/ack judged by send order, not stamp; a mismatched-frame ACK clears only its row; out-of-order re-served frames kept in the gap buffer). Open: #2 own-fleet era split (needs Brittany's log), gap buffer not persisted across a restart (a head-hash in the status report would close it = wire change, owner's call), sync.rs ~1568 / wave_ui direct_ok / ping relay still decide on "a path is held".
