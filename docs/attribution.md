# Attribution

A commit says who **built** it and with what. Claude is a tool, not an author.

```
Built with Claude <version>
```

Never `Co-Authored-By: Claude …`. Co-authorship claims a person; a tool does not co-write a commit any more than a compiler does. The rule holds for every repo in the fleet — photon, fluor, fgtw, vsf, kete, manifestus, tohu, limbus, opsin.

## Why this needs a ratchet

The banned trailer keeps coming back because a host can inject it as a default instruction mid-session, overriding what the session was told at the start. So the rule lives in the tree, where the build enforces it, rather than only in a memory a session may not have read.

- `.githooks/commit-msg` refuses it at the moment of writing. Armed with `git config core.hooksPath .githooks` (per clone — a fresh clone must run it once).
- `scripts/lib/trailer-gate.sh` runs in `preflight_gates`, so every build entry point (dev, publish, deploy) refuses to proceed while an unpushed commit carries it.

The gate scopes to unpushed commits — the ones still cheap to amend.

## The history that already carries it

Commits from before this ratchet still carry the wrong trailer, across several repos and several months. Rewriting them is possible but not free: it changes every hash from the first offender forward, so every clone must `reset --hard`, every published hash in release notes and memory goes stale, and the public repos need a force push. That is an owner's decision, deliberately not automated here. If it is ever taken, do it once across all repos in one sitting, and re-verify each sibling clone afterwards.
