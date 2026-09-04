---
name: project_vault_seal_failures
description: "OPEN + ACTIVE 2026-09-04: vault seal-verification failures on two devices; Leviathan is failing on WRITES for 7.5h+ (ACKs withheld). Fence/block-reuse suspected, NOT write ordering."
metadata:
  type: project
---

Two field incidents, same day, different severity.

**Emma's Android (one-shot, self-healed).** Android killed the app mid-flush to install v0.83.0 (`ExitInfo: ... desc=stop com.photon.messenger due to installPackageLI`). Next boot: `mirror replicate failed (seal verification failed)` → `vault open refused` → `repair open succeeded — 1 dangling pointer(s) pruned`. Exactly 1 value lost (the in-flight write). Next launch opened strictly, 384 rows intact — `vault_degraded` is session-scoped, so the banner cleared on its own.

**Nick's Leviathan (ACTIVE, severe).** First failure 2026-09-04 10:51:31, still failing 18:31 — 7.5+ hours. Failing on **writes**, not just open: `STORAGE CRITICAL: chains persist failed — withholding N gated signal(s) (ACK/transmit)`, plus `async message persist failed`, `async conv-state persist failed`, `SETTINGS: persist failed` — all `Vault: seal verification failed`. Withheld ACKs mean peers resend forever. Recurred across a relaunch AND an update, so it is not transient. First failure landed during a heavy history burst (dozens of pages served + PIPE injections from MacBook fe46a74b) — i.e. under reap/fence pressure.

**Diagnosis so far — write ORDERING is already correct, do not "fix" it.** Verified by reading the chain: `tract.append` calls `mirror.write_verified_batch(...)?` (write → flush → read back → compare) and returns only when durable; `host.rs` `flush` is a real fdatasync on Linux/Android, F_FULLFSYNC on macOS; the spine entry referencing those blocks is appended later in `commit()`. So values are durable+verified BEFORE any pointer to them is published. Nick initially chose "enforce value-durable-before-pointer" — that would be a no-op.

**The live hypothesis is BLOCK REUSE, not tearing.** The tract is a ring (`(plow + i) % len`), so the plow wraps and overwrites. A SIGKILL is not a power loss — once fdatasync returns the kernel owns the data — so a freshly written block should not be tearable by an install kill. That points at a still-referenced block being overwritten. `fence_limit` (`Err(Error::Fenced)` when `plow + n > limit`, last FENCE_K generations restorable) is the mechanism meant to prevent exactly that.

**NEXT (unstarted): audit the fence window** against reap, ring migration, and the mid-ladder heartbeat commits (`commit()` on `Fenced`, `put_no_commit`'s ladder) — can the plow ever be allowed onto a block a live committed pointer still references? Deliberately did NOT patch the vault engine on a hypothesis. Related: [[project_manifestus_tombstone_bug]], [[project_storage_layering]].
