---
name: photon-not-fmt-clean
description: "photon HEAD is not rustfmt-clean — a bare `cargo fmt` churns ~40 unrelated files; scope fmt to files you touched and restore the rest"
metadata: 
  node_type: memory
  type: project
  originSessionId: c5aa3ef0-d46f-4c40-a374-e2549bebaaef
  modified: 2026-08-23T16:41:10.179Z
---

Running `cargo fmt` in photon (observed 2026-08-23) reformatted ~40 files across the tree — HEAD is not fmt-clean and Nick fmt's in dedicated passes ("style: cargo fmt pass" commits exist), not continuously.

**Why:** tree-wide churn buries real changes in review and pollutes topical commits.

**How to apply:** after editing, run `cargo fmt`, then `git checkout --` every file you did not edit; commit leftover fmt churn in touched files as a separate `style: cargo fmt over the files this session touched` commit. For per-ticket commits over shared files, hunk-stage with `git diff | <classifier> | git apply --cached`.

Related: [[macbook-trails-remote]], [[commit-trailer-built-with]]
