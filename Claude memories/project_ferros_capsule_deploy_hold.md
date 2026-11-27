---
name: project_ferros_capsule_deploy_hold
description: "HOLD (2026-11-26): no Photon deploy may publish the ferros capsule until both phones run a ferros image built from >= 374cb1ba; the capsule now wants the mic right, which an older kernel refuses, so Photon would not launch at all"
metadata:
  node_type: memory
  type: project
  originSessionId: 17b90e8e-8b1a-448e-8ac7-22b057e9a0d7
  modified: 2026-11-27T00:38:24.361Z
---

Photon 35244165 moved to the new slots (MIC_SLOT 10, AUDIO_SLOT 13 -> 14) and ferros 374cb1ba added `--want mic` to tools/photon-capsule.sh.
A ferros kernel from before 91c9386f refuses the whole launch of a capsule that wants a right outside its vocabulary (ferros process.rs, "a right outside the vocabulary"), so a Photon capsule built now does not start on a phone still on the old image.
Deploy order (ferros KISTS.md "Deploy order"): (a) the device's kernel, init, map and audio capsule from >= 374cb1ba, done by the ferros session when a phone is on USB; (b) only then the Photon capsule with the mic want.

**Why:** scripts/release/deploy.sh builds and publishes the ferros kist from ../ferros every time; published before (a), it reaches the phones and Photon on ferros stops launching.
**How to apply:** before any deploy.sh (or ferros kist publish), ask the ferros session whether (a) is on both phones (the 8a and the Pro). If not, tell Nick that the deploy would ship a capsule the phones cannot launch, and hold it or ship without the ferros capsule. Desktop and Android are unaffected. Delete this memory once ferros confirms (a). Related: [[project_ferros_media_latency]], [[feedback_deploy_detached]].
