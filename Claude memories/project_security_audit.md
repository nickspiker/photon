---
name: project_security_audit
description: "2026-11-01 security audit (security audit/, locally excluded) — batch 1 FIXED 2026-11-02 (Critical + clear Highs); design items await Nick; Mediums/Lows not started"
metadata:
  node_type: memory
  type: project
  originSessionId: d6c9db89-ccef-475b-b38b-64f51976930f
  modified: 2026-11-03T20:43:12.482Z
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
Awaiting Nick (design): identity seed = BLAKE3(handle) (CSTV-02; see [[project_handle_rename]]), release `panic = "abort"` (UPW-010/NET-22), installer verify-by-executing (UPW-007), remote shell always on (UPW-027).
LIB-01 FIXED 2026-11-03 (nunc 4990bc6): Merkle nonce proof to SREP.ROOT (SHA-512 truncated to the root's length, 0x00 leaf / 0x01 node prefixes, INDX bits pick the side), DELE MINT ≤ MIDP ≤ MAXT, connected UDP socket bound in the server's address family. LATENT BUG FOUND: the client only knew the IETF delegation context, but our VER-less request is answered in the pre-IETF format (64-byte ROOT, µs timestamps, context "…delegation signature--\0"), so NO Roughtime reply ever verified and the source silently contributed nothing; also the socket was bound v4 while Cloudflare resolves v6 first. Both fixed; an #[ignore] live test (`cargo test --all-features -- --ignored live_reply`) proves a real reply passes under its nonce and fails under another. RADI not bounded (unit ambiguity between formats).
VSF BATCH 2 FIXED 2026-11-03 (vsf 8bb057e): VSF-05 E marker read via get, VSF-06 shape products thru packed_view::shape_elements (both tensor parsers, Tensor::new, image from_rgb_tensor checks u32 dims and h*w*3 == bytes), VSF-07 header end via get, VSF-08 thread-local DepthGuard caps renderable nesting at 1<<6, VSF-09 checked_add/checked_sub on trailer/header offsets and ByteSource::read_vec range-checks before vec![0; n], VSF-12 image::from_av1 = the canonical send/drain loop (MAX_ROUNDS 1<<6, frame_size_limit 1<<22, EAGAIN -11/-35 by target, data_unref + close on error). VSF-11: add_encryption_metadata DELETED (no callers anywhere; it wrote the AEAD key into the header beside the ciphertext). VSF-10 DEFERRED to Nick: hp zeroes "signatures" found by scanning the whole file incl. bodies; fixing it to zero only header-located slots CHANGES hp for any existing file whose body happens to contain bytes parsing as g[eprmfsd] → a compatibility decision (vault + wire), not a code fix. Tests: hostile_input.rs gained deep-nesting and overflowing-shape cases.
Next: NET-04..07 replay class, then the Mediums (LIB-02..16, NET-08..11, FGTW-02/03, SPX-01/02, UPW-008..019), then Lows; `cargo audit` pass and blind.rs wording never done.
Attachment names ruling restated by Nick 2026-11-03: overwrite inside Downloads is WANTED (Btrfs, no "(1)" suffix ever); the bug was only the missing boundary (`..` traversal, direction overrides).

**Why:** Nick asked to "go thru" the audit together (2026-11-02).
**How to apply:** check this list before re-fixing; keep auditor IDs in code comments so a finding can be traced.
