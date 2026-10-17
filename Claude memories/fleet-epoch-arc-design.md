---
name: fleet-epoch-arc-design
description: "B1-B3 fleet epoch design invariants (SHIPPED 2026-08-12/16): kind-gated Checkpoint op keeps signing_bytes stable, epoch derivation, settled-root distribution, k/k−1 acceptance"
metadata:
  type: project
---

B1→B3 fleet-crypto arc SHIPPED (photon a8b9d48 + fgtw 300886d, 2026-08-12, flag day approved; residue closed 2026-08-16 — [[project_fleet_epoch_arc_closed]]). Invariants that still bind any edit to the membership chain (fgtw/src/fleet.rs, shared by photon and the fgtw.org worker):

- **signing_bytes MUST stay byte-identical for Genesis/Add/Remove** or every deployed chain's signatures break. Checkpoint fields (`ckpt_k`, `ckpt_commit`, `ckpt_fanout_epoch`) append ONLY when kind==Checkpoint (same kind-gating as the genesis identity / add consent extras). Pin a pre-change hex vector in a test before any edit.
- Fold: Checkpoint signer must be a current member; membership unchanged; k strictly sequential from 1 (`CheckpointOutOfSequence`); nonzero ckpt fields on other kinds → `StrayCheckpoint`. Unknown op kinds hard-fail ("bad kind") — a new kind is a flag day.
- B1: epoch_0 = spaghettify(domain ‖ 2 MB pad from avalanche_expand_eggs over labeled CSPRNG eggs); pad + eggs dropped. Only {k, epoch_k} lives on: vault + fleet-key-sealed custody slot (written + readback-verified BEFORE the chain push) + sibling ckpt_state frames.
- B2: epoch_{k+1} = spaghettify(DOMAIN ‖ epoch_k ‖ settled_root ‖ fleet_key ‖ fanout_epoch ‖ k+1); settled_root = merkle over (timestamp, blake3(content)) per conversation, token order, non-control rows. The chain carries only the commit; the root rides sealed under epoch_k (ckpt_root frame). A local recompute mismatch is a loud log + history repair, never a wedge. Mint race resolved by the chain (loser adopts).
- B3: chain_sync / sibling hist_page / sibling pong tails seal under `epoch_seal_key(epoch_k, purpose)`, k public on the frame; receivers accept k and k−1. Friend pongs/pages unchanged.
