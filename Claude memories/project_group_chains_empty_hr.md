---
name: project_group_chains_empty_hr
description: 2026-11-13 group chains documents unreadable ("length overflows"): empty hR targets/acked on an unacked group row; writer guarded, docs already on disk need the owner's call
metadata:
  type: project
---
Root cause (2026-11-13, not v151, not the x codebook): `src/storage/friendship.rs::chains_to_vsf_bytes` wrote `targets`/`acked` as `VsfType::hR(..)` with no empty check; a group row no member has acked yet has empty `acked_by`. vsf's flatten writes an hR length as `len - 1`, which wraps to usize::MAX with overflow-checks off (`6 FF×8`), and since vsf 09bb5d3 (VSF-01, 2026-11-02, ~v139) `decoding/helpers.rs::decode_len_minus_one` refuses it ("length overflows"), so the whole chains document (root, lanes, KEM secrets) fails to load and the group can't send or decrypt on that device until a fresh invite. Seen on group a2174ec2 on the desktop 2026-11-07 and 2026-11-13; likely hitting other users' groups on any restart with an unacked group row. Writer bug since 96e8250a (2026-09-25).

Fix landed: the writer leaves an empty list out (the reader already takes a missing field as empty); test `a_group_row_with_no_acks_yet_survives_a_save`. Notes: target/chains-overflow-findings.md.

**Open, owner's call:** (a) documents already on disk: let a refreshed invite reinstall the chains (old KEM secrets lost) or a one-time repair that rewrites the `hR 6 FF×8` fields as absent under a dated MIGRATION-EXPIRES pin; (b) vsf: make every len-1 encoder fail loudly on an empty value instead of wrapping.
