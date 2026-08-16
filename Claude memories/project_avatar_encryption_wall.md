---
name: project_avatar_encryption_wall
description: "Photon avatars are v'e'-encrypted per-handle — no server/admin view can decrypt them; the browser AV1 decode infra built for this belongs in photon, not octopus"
metadata: 
  node_type: memory
  type: project
  originSessionId: 0b164fd9-062c-407e-b4bb-8f6be8d1982d
---

Photon avatars are **encrypted at rest per-handle**: `image/pixels = v'e'(encrypted v'a'(AV1))`, encryption key = `BLAKE3(BLAKE3(handle_plaintext) || "avatar-encryption")` (photon `src/ui/avatar.rs` ~902/1089). The storage key is public (anyone can fetch), the content is ciphertext. Only a holder of the **handle plaintext** (a contact who friended you) can decrypt. FGTW stores them and cannot read them — a deliberate zero-knowledge property (server compromise leaks no faces).

Consequence: the **octopus admin console can never show avatars** — it has handle *proofs*, not plaintexts. An "avatar next to each row" in the admin is cryptographically impossible for real avatars, and giving the admin decryption keys would be a privacy regression (don't). If a per-row visual is ever wanted in admin, use **identicons from public data** (device_pubkey / handle_proof), not decrypted avatars. Real avatar display belongs in **photon** (contacts hold the handle).

Built + deployed anyway (2026-07, all pushed): a full **browser AV1 decode pipeline** — `vsf::image::decode` (behind vsf `image-decode` feature, pulls rav1d), a **vendored rav1d+libc-shim** at `toka/vendor/` that builds for `wasm32-unknown-unknown` (the OS-less target only lacked ~11 libc type-alias/errno symbols; rav1d builds clean for `wasm32-wasi` unshimmed), `fluor::paint::draw_image` (scaling α+darkness blit), toka `draw_image` opcode + per-VM resource table + `CellData::Image` through the table engine, the `/resource_get` VSF endpoint, and the app.js fetch-at-render loop. It works end-to-end; it's just pointed at the wrong consumer. Re-enabling for photon = point `draw_image` at decryptable bytes. See [[feedback_fgtw_deploy_freely]].
