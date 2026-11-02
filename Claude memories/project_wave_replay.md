---
name: project_wave_replay
description: 2026-11-01 BUILT (unverified on devices): replayed wave screen (replay.rs) + opt-in kept beam video (H.264 in VSF v(b'm'), Settings → Beam "Save beam video", off by default) + one person page; field test pending
metadata:
  type: project
---
Nick 2026-11-01: "UI for playing back a wave isn't super great in the molecule… video too… uniform" → "Def H.264 and add that format to VSF", "Wave screen replayed is good", "Checkbox in beam settings, unchecked by default", and yes to one person page.
BUILT:
- replay.rs: the card's Play opens a screen over the conversation (viewer pattern) — one tile per voice (face_tiles), ring = that channel's LiveBands band powers from the playback worker, scrub bar, Play/Pause, tap a face to mute that voice; Back leaves it playing on the card.
- Rows name voices: AttachRef::parties (row field `fp`; vault `file_parties` 32-byte ids end to end; history page `file_party` repeated). Pair = [us, them]; group = us + each spool's far party in merge order.
- Beam video: wave/spool.rs beam spool (beamspool-*.tmp, sealed, u32 records, swept at launch, no register — crash loses video), capture at Outbound::send_frame (track 0) and decode_loop before decode (track 1+slot), Party records name tracks; wave/beam_keep.rs → `beam.video` VSF doc (track sections; frame = [e, u n, u g×4, v(b'm', au)]); AttachRole::BeamVideo = 4 row at offer_osc+4; wave/replay_video.rs decodes in step with audio (KeptStream/PlaybackHandle base_osc).
- VSF: ENCODING_H264 = b'm' (vsf 2ec98d4). AGENT.md Rule -1 + artifact-gate list H.264 (at rest only in VSF, opt-in).
- Person page: conv_panel.rs pair_person/conversation_rail, PersonReturn; a pair's rail = About · In numbers; Clear waves/all on the pair person page.
Open: card still draws 2 lanes; VSF export names channels self/peer; whole beam.video built in RAM at keep (long beams heavy); pair conversations have no Mute.

**Why:** uniform playback at any size, video opt-in per Nick.
**How to apply:** field test: two-person and group wave replay; Save beam video on, beam, replay; check "BEAM: video kept" and "WAVE: replaying" lines. Related: [[project_beams]], [[project_group_media_scale]].
