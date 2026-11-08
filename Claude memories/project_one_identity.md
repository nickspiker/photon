---
name: project-one-identity
description: "DESIGN 2026-11-08, nothing built: one identity per device for every FGTW app (\"1 means 1\"), shared attest out of Photon, Unbound screen, unified About/Attached/Discover page, open catalog; trust hardening = FIDO2 hmac-secret tag as pocket PIPE, kernel route Linux-only"
metadata:
  type: project
---

Nick 2026-11-08: every FGTW app (Photon, Mahere, Lumis first) checks for an identity on the device; if none, it runs the shared fgtw attest, and then every app sees it ("1 means 1"). On ferros the OS attests. A device with no identity shows "This device is unbound: tap to attest". One About page across the apps doubles as the dashboard (attached apps with grants + Detach, Discover). "No store. Open." — Discover is a signed pointer list; trust is the developer's capsule signature.

Canonical: photon docs/one-identity.md (states Unbound/Locked/Live, what moves out of Photon, the `kists` record from ferros docs/KISTS.md riding fleet state, open decisions, build order) and tohu docs/oracle-hardening.md "Beyond the oracle" (tag + kernel).

**Facts that shaped it:** desktop apps already share one session (tohu `$XDG_RUNTIME_DIR/tohu`); Android sessions are per app, but same-signer apps share ANDROID_ID, so they are one fleet device and attest resolves Member — only the session needs a same-signer channel. Mahere uses tohu only via kete; Lumis is Kotlin UI with no identity.

**Trust hardening (Nick wants both built and noted):** plate sources are readable by any process. A crypto tag as PIPE: FIDO2 `hmac-secret` is the fit (32 B salt in, 32 B out, deterministic, secret never leaves, NFC on phones and USB on desktops, salt = keyed BLAKE3 of handle_proof for per-handle separation). NTAG 424 DNA (Nick's "4xxx") is the wrong shape: symmetric AES means the checker holds the key, and its SUN MAC is counter-bound so nothing repeats. The tag joins the KEY half above frozen v0 (gates fleet signing key or vault unlock), never the plate. Kernel "software PIPE": Linux feasible (SELinux/AppArmor first, LSM later), Android only on flashed kernels, Windows heavy, macOS not viable. Recommended: tag first.

**Why:** Nick's direction for identity across the app family plus the security audit's device-root exposure (which stays NOT a finding for the plate itself — [[feedback_device_root_never_salted]]).
**How to apply:** build in the docs' order (lift attest path into fgtw first); the open decisions in docs/one-identity.md are Nick's. Related: [[project_android_widevine_plate]], [[project_linux_tpm_plate]], [[project_session_registers]], [[project_security_audit]].
