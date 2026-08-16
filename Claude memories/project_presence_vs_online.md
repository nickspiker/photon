---
name: project-presence-vs-online
description: "presence ≠ online — online = the avatar ring (always works); presence = opt-in busy/song/mood broadcast, \"show my presence to contacts\" DEFAULTS UNCHECKED"
metadata: 
  node_type: memory
  type: project
  originSessionId: f4272721-c713-4a82-a97a-db8106029756
---

User doctrine 2026-07-23: the "show my presence to contacts" setting is NOT an online indicator and must default OFF (unchecked).
Online/reachability is conveyed by the ring around the avatar and is not gated by this setting.
"Presence" here means the rich broadcast layer: busy state, now-playing song, mood, "that shit" — deliberate self-disclosure, so opt-in.

Ring colour spec (VSF RGB, convert thru the proper pipeline to the Rec.2020 output target — never raw-passed):
- 0x00FFFF pure cyan = direct connection in the same room (LAN)
- 0x00FF00 = direct WAN (no relay)
- 0xFFB000 = relay

Related: [[project-party-colour-perceptual]], [[project-theme-rec2020]].
