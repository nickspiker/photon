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
