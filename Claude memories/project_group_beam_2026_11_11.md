---
name: project_group_beam_2026_11_11
description: 2026-11-11 19:00 UTC bad 3-way group beam (Nick, Jeff, Joshua): no keyframe for a late joiner, ~7.5 fps sender cap, WAN treated as LAN, no Jeff-Joshua link, uplink doubling; Jeff black screen unexplained
metadata:
  type: project
---
Owner report 2026-11-11: "shit beam in a molecule between myself, Jeff and Joshua"; Jeff's display went black with no buttons. Group wave 7d4954bf, sender Nick device 469e79b8 (unknown which machine; no log of it). Full notes: target/beam-findings.md (not committed). Findings, not yet fixed:
0. Late joiner never decodes: Joshua joined at +34 s, 205/205 frames "no picture", 0 lost. `h264::Encoder::force_keyframe` (src/wave/h264.rs ~134) has no caller and no keyframe-request wire op; `beam_session::fan_add` / `Outbound::send_frame` (~275/~356) give a new link only the stream description. Desktop keyframes every 600 frames (KEYFRAME_EVERY) or on resize; phone senders never after the first.
1. ~7.5 fps delivered vs 30 planned, 0 loss on two paths: sender-bound (single beam-send thread does camera read/MJPEG decode, self-view, resample, OpenH264 1080p serially). Encoder built with cam_fps 60 and 60 fps timestamps while plan.fps frames are encoded: rate control told the wrong rate.
2. `beam_session::ceiling` (~593) calls it LAN when LAST_LINK_RTT_MS <= 15, but that is 0 or the previous wave's RTT when the sender starts: a fresh wave skips the 1280-wide off-LAN cap; Jeff on cellular (85-225 ms) got 1920x1080.
3. Jeff and Joshua never linked: each had one link (to 469e79b8); "1 direct path(s); 3 with no address ride the lane row"; `send_group_express` (group_wave.rs ~197-246) sends nothing to a device with no address; relaying only when the author is offline. Effectively two 1:1 calls.
4. Uplink doubled when the second link came up (mesh sends each frame per link); ladder watches only the lead link, queue ~71 ms < the 100 ms standing-queue threshold, so it never stepped down.
5. Jeff (Samsung SM-S938U, Android, v151): app drew at ~36 fps the whole beam, no panic/ANR/decoder error; surface lost 19:01:03-:10 around hangup. Candidates: lock-screen flags cleared at waveWentActive (PhotonConnectionService.kt ~1216) or the group picture paint (render.rs ~963-1023 paint_picture_rect). Needs a screenshot/recording or pause/surface-loss log lines.
Side bug: "WakeSender::send failed" (grants.rs:34) on every Android foreground return.

**How to apply:** these are messaging/beam code (Photon), not FGTW. Fix order proposed to the owner: keyframe on join + keyframe request, LAN/WAN ceiling from the live link, encoder fps, per-link congestion, then group signalling between non-author members. Related: [[project_beams]], [[project_group_media_scale]], [[project_group_waves]], [[project_beam_standing_queue]].

**Owner decisions (2026-11-11), fixes approved:**
- Keyframes: no timer. "If my friends don't need a keyframe, don't send one. If they need one, send it now, not in 2 minutes." A receiver whose decoder cannot make a picture (late join, unconcealable loss) requests one; the sender forces an IDR at once. KEYFRAME_EVERY's periodic IDR goes.
- Rolling refresh is what the owner likes (Android: starts transparent, chunks in like a rolling shutter, no network spike). OpenH264 cannot roll; a desktop rolling encoder (hardware NVENC/VAAPI/VideoToolbox/MF intra refresh, or x264 GPL) is a new per-platform dependency = owner decision, offered, not taken.
- "Same network isn't a decision, it's a latency and bandwidth question": the beam ceiling comes from each link's measured latency and throughput, live, never a LAN/WAN boolean.
- Approved too: encoder told the real fps, congestion watched on every link (not only the lead), group members linked to each other (not only to the caller).

**Landed 2026-11-11 (unverified in the field):** photon 2aca3cd5 (beams: KeyAsk wire kind 3, no periodic IDR, per-link measured ceiling starting 1024 wide, real fps, every link's congestion) and d64e47e8 (group waves: fold never-friended members at load/offer/ring/join, peers-first chain ask via ask_members_for_chain, held signals replayed on fold, members hand join/leave on). Open for the owner: 71 ms queue still below the 100 ms step-down threshold; phone SPS/PPS every 30 frames kept for v151 receivers; OpenH264 scene-change IDRs still on; media relay through a member (stage 2b) unbuilt; OpenH264 single-threaded encode (threading/hardware for more fps); Jeff's black screen needs a screenshot or recording.
