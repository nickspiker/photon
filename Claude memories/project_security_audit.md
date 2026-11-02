---
name: project_security_audit
description: "2026-11-01 security audit (security audit/, locally excluded) — batch 1 FIXED 2026-11-02 (Critical + clear Highs); design items await Nick; Mediums/Lows not started"
metadata:
  node_type: memory
  type: project
  originSessionId: d6c9db89-ccef-475b-b38b-64f51976930f
  modified: 2026-11-02T17:57:37.773Z
---

The audit lives in `security audit/` in the photon checkout (5 reports + NOTES.md), excluded via .git/info/exclude — never commit, move or delete it.

Batch 1 FIXED 2026-11-02:
- UPW-001: Windows toast takes text via env vars (no script built from message text). Also notify-send gets `--`.
- UPW-003/CSTV-06: Windows links open via `rundll32 url.dll,FileProtocolHandler`; `url_chars_ok` (RFC 3986 ASCII only) gates link marks and spans.
- UPW-002/CSTV-05/NET-02: `storage::clean_file_name`; land_blob refuses anything not one plain name (overwrite-within-dir kept, per Nick's 2026-09-20 ruling, no collision suffix).
- UPW-006: admin token rotated → worker secret ADMIN_TOKEN, Bearer header only; keys/fgtw-admin-token (the MacBook needs a copy to send release notices).
- CSTV-03 (HQC decap via FFI, implicit rejection kept), CSTV-04 (McEliece length via try_into).
- NET-01: PT SPEC must be signed (PTSpec::signed_by) and well_formed (≤ 16 MiB, consistent count).
- UPW-005: decode_avatar canonical send/drain loop, frame_size_limit 2048², 8-bit 4:2:0 only, cleanup on every exit.
- UPW-004: music envelopes stream via record::PcmEnvelopes, capped at 2^14 s.
- VSF-01..04 (vsf 09bb5d3): short/elements_bytes/decode_len_minus_one, wire counts bounded by bytes left, VsfSection::try_new for TOC names, duplicate header k/g/h refused; tests/hostile_input.rs.

NOT a finding: device key from machine-id (CSTV-01/NET-03/FGTW-01, LIB tohu) — documented interim stand-in for PIPE's ira (tohu README "Status"/"Terminology"; Security posture reports low via hardware:false; OS keystore rejected as a Google dependency). The auditors missed it; never re-raise. AND the stable device-bound identity is anti-theft by design (Nick 2026-11-02: a car that changes plates whenever someone sits in it is harder to CATCH THE THIEF): a stolen device keeps answering as itself to FGTW, the fleet and friends, so it can be recognised, refused and tracked; a random per-install key lets a thief launder the device with a reinstall.
Awaiting Nick (design): identity seed = BLAKE3(handle) (CSTV-02), release `panic = "abort"`, installer verify-by-executing (UPW-007). Next code batch: LIB-01 nunc Roughtime nonce, VSF-05..12 (incl. vsf image::decode loop = same dav1d bug as UPW-005), then Mediums.

**Why:** Nick asked to "go thru" the audit together (2026-11-02).
**How to apply:** check this list before re-fixing; keep auditor IDs in code comments so a finding can be traced.
