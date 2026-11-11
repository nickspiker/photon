---
name: project_x_codebook_bump
description: 2026-11-10 vsf `x` Huffman codebook re-issue (major bump, rare-char codewords move): Photon identity derives from x(handle) bytes and history is x text; owner decisions pending
metadata:
  type: project
---
Owner decided (via the ferros session, 2026-11-10) to re-issue vsf's `x` codebook in a fixed order: codes of 20 bits or fewer (712 common chars, all ASCII) stay byte-identical; rare chars' codewords move; vsf major bump; Photon must move in lockstep.

Photon exposure (reported to ferros + owner, bump asked to wait for the owner's picks):
- IDENTITY: `ihi::handle_to_hash` = blake3(VsfType::x(handle).flatten()) feeds identity_seed AND handle_proof. A handle with a moved char becomes a new identity. All 12 handles in the owner's pseudonym map are pure ASCII (checked without printing), so the owner's fleet/contacts are safe; strangers with rare-char handles are not. Options: accept, or pin ihi's handle hashing to the current table forever.
- HISTORY/WIRE: message bodies (plain, link text, file names, reactions), names (settings/fstate published_name, contacts, molecules, cloud backup), drafts, spool, manifests are x. Rare chars decode differently after the bump; ident_typed hashes decoded UTF-8, so mis-decoded rows also change row keys. Options: one-time re-encode with the old decoder under a dated pin, accept garbling, or a new type marker (a permanent old reader, against [[feedback_no_compat_text_hatches]]).

**How to apply:** do not take the vsf bump into Photon until the owner has chosen on both; deploy lockstep. Related: [[feedback_handles_byte_precise]], [[feedback_epoch_fixed]].

**Decided (owner, 2026-11-10):** identities: accept (all handles ASCII); Photon pins ASCII seeds + one proof (e29d6ff8, ascii_identity_kats). History: option (b), accept garbling, no re-encode, no old table. Exposure is small: é 10 bits, ü 11, ö 12, ë 15, 😂/😀 12 bits all stay byte-identical (common emoji are in the unchanged ≤20-bit set). Take the vsf bump when the ferros session sends the branch/commit, lockstep across the owner's devices.

**Landed (2026-11-10):** the reissue folded into vsf 0.11 (211b8fe, force-pushed, no v12 in history). Identity follows the NEW codebook (owner: "no permanent pinning of ihi to the old table"): ihi 92796ee moved off published vsf 0.6 to local vsf 0.11 x-full (no_std handle hashing); Photon 26d9e809 takes ihi by path (one vsf, one ihi in the lock); moved chars £¥§°•…€ pinned to the new value (moved_character_identity_kats), ASCII pins unchanged. I first pinned them to the OLD table (10a5b4bf), which the owner corrected: never freeze identity to the old table.
