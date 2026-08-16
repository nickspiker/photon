---
name: project-android-ime-model
description: "Android keyboard model — surface NEVER resizes for the IME (adjustNothing), app lifts bottom strips via reported inset; span-pin removed"
metadata: 
  node_type: memory
  type: project
  originSessionId: bf3c2e39-d57b-4469-8848-1780b1b5c927
---

THE Android keyboard model (decided 2026-07-25, photon 165d765 + fluor 4a8ebbe): the surface never resizes for the IME.
Manifest = adjustNothing; the Activity's old manual SurfaceView shrink is deleted; the insets listener reports keyboard height to Rust via ptr-less nativeImeInset → jni_android::ime_inset_px().
Photon subtracts `ime_lift()` from bottom-anchored strips (conversation compose bar + message list anchors, layout AND render sides must match); a per-tick diff (`last_ime_inset`) triggers relayout since no resize event fires.
Consequence: the full-screen harmonic mean IS the scale by construction — fluor's span_basis/basis_width pin and Viewport::with_span are DELETED, don't reintroduce them.
Rotation preserves scale automatically (2wh/(w+h) symmetric); split-screen resizes SHOULD rescale.
Caveat: API 26-29 report ime()=0 under adjustNothing (ticketed; fleet is all API 30+).
Launch/AddDevice screens don't lift yet — only the conversation screen does (v1 = the user's actual pain point).
