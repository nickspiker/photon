---
name: fleet-epoch-arc-design
description: "B1-B3 fleet chain+eggs design decisions: kind-gated Checkpoint op (signing_bytes stability!), epoch state custody+D2D serve, sealed-root distribution, N=256 edges"
metadata: 
  node_type: memory
  type: project
  originSessionId: 296bef77-7c97-45b8-8d90-dc492f93e557
  modified: 2026-08-12T06:43:52.795Z
---

STATUS: B1+B2 + B3 leg one SHIPPED 2026-08-12 (photon a8b9d48, fgtw 300886d — FLAG DAY: fleet update + fgtw worker deploy together, old builds hard-fail parsing kind 3). Remaining: sibling hist_page + pong-tail re-seal (same shape: k field, epoch purpose key, k/k−1 accept), 256-row mint cadence (only bootstrap+rotation edges fire today), refold-time commit reconciliation log, custody rewrite rides only checkpoint wins.

The B1→B3 fleet-crypto arc (started 2026-08-12) — decisions that must survive context loss:

- Membership chain lives in the SHARED fgtw crate (~/Code/fgtw/src/fleet.rs, path dep); the deployed fgtw.org worker uses the same crate → my change covers both, Nick redeploys worker as part of the flag day. Old parsers hard-fail "bad kind 3" — that IS the approved flag day.
- **signing_bytes MUST stay byte-identical for Genesis/Add/Remove** or every deployed fleet chain's signatures break (fleets brick). Checkpoint fields (ckpt_k u64, ckpt_commit [u8;32], ckpt_fanout_epoch u64) append to signing_bytes ONLY when kind==Checkpoint, mirroring the kind-gated VSF positional extras (genesis identity pair / add consent pair at values[6..8]). A pre-change hex vector is pinned in a test before any edit.
- Fold rules: Checkpoint signer must be a current member; membership set unchanged; k strictly sequential (prev+1, from 1) → CheckpointOutOfSequence; nonzero ckpt fields on other kinds → StrayCheckpoint.
- B1: reservoir = labeled CSPRNG eggs (+device pubkey, genesis hash, fleet key as binders) → avalanche_expand_eggs → 2MB pad → epoch_0 = spaghettify(domain ‖ pad); pad+eggs then dropped (eggs-dropped rule applied one level up). Only {k, epoch_k} lives on: vault + fleet-key-sealed custody slot (wiped-device recovery, rides rotation re-seal) + served sibling-to-sibling via fleet-key-sealed ckpt_state frame (the fgtw-independent correctness path, law 6).
- B2: epoch_{k+1} = spaghettify(DOMAIN ‖ epoch_k ‖ settled_root ‖ fleet_key ‖ fanout_epoch ‖ k+1). settled_root = merkle over (timestamp, blake3(content)) leaves per conversation (lane_label dropped from the plan triple — rows don't store it; (stamp,content) is fleet-wide row identity), conversations in token order, non-control rows only. Commit=blake3(DOMAIN ‖ root) rides the chain op; the ROOT VALUE is distributed sealed under epoch_k in a ckpt_root sibling frame — siblings verify commit + AEAD-open success instead of needing set agreement for liveness; a local recompute mismatch is a loud divergence log + history-repair arm, never a wedge. Edges: 256 new settled rows, membership rotation. Mint race resolved by the chain (extends CAS); loser discards + adopts.
- B3: chain_sync / sibling hist_page routes / sibling pong tails re-key from raw fleet_key to epoch_seal_key(epoch_k, purpose); frames carry k publicly; receivers hold k and k−1. Friend pongs (friendship secret) and friend hist_pages (history key) unchanged. recover_fleet_key drops the fan-out epoch today — need a with-epoch variant.
- Existing fleets have no reservoir: first post-update boot mints and races via the k=1 checkpoint; custody written (and readback-verified) BEFORE the chain push.

Related: [[messaging-solidity-phase-a]], [[vsf-toc-section-name-trap]], [[persist-findings-early]].
