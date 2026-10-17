---
name: reference_claude_unguard
description: "~/.local/bin/claude-code-unguard force-opens Claude Code's Edit read-before-edit guard (hardcodes the tengu_velvet_hammer gate true); re-run + reload after every update; file-history lives on tmpfs"
metadata:
  type: reference
---

`~/.local/bin/claude-code-unguard` (node script, 2026-07-12) disables the Edit "file has not been read yet" guard so Edit works without a prior Read (multi-writer workflows, context cost).

**Mechanism — FORCE-TRUE, not default-flip:** the guard is gated by the `tengu_velvet_hammer` flag; flipping its baked default does nothing because the server pushes an explicit false. The script replaces the whole first gate-read expression with `!0`, space-padded to the original length, so no server value re-enables it. Write's gate (`tengu_velvet_mallet`) is deliberately left alone. Naming: hammer = EDIT, mallet = WRITE.

**Targets:** the Bun-compiled binaries that actually run — `~/.vscode/extensions/anthropic.claude-code-*/resources/native-binary/claude` and the npm/nvm `bin/claude.exe` — not the npm `cli.js`. Atomic write + `--version` smoke test, idempotent, NO `.orig` backups (Nick deleted them in anger; do not re-add).

**Re-run after every update, then reload** the VS Code window / restart the session (the running process holds the old binary).

Safe because Edit still matches `old_string` against a fresh disk read.

Also: `~/.claude/file-history` is a symlink to `/dev/shm/claude-file-history-$UID` (tmpfs; the copy lacks FICLONE so it hammered the SSD); the user service `claude-file-history-tmpdir.service` recreates the dir at login.
