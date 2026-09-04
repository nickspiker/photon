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

**LEVIATHAN ROUND 2 (fresh log 2026-09-04 18:32, Linux/x86_64 — a different box from the Mac this session runs on).** Relaunch → open refused → repair pruned **43** dangling pointers (Emma's lost 1). After that repair the box is HEALTHY: zero seal failures in the rest of the log. So the 7.5h write-failure window ENDED at the repair.

**Re-diagnosis — the failure is seals failing on READ of blocks already at rest, not a write-path fault.** `write_one`'s read-back mismatch returns `Error::Verify(lba)`, a DIFFERENT error from the "seal verification failed" being logged. So ~43 blocks on Leviathan's disk cannot be opened; every operation whose COW walk touched one failed, which is what produced 7.5h of `chains/settings/message persist failed`. The failures were caused BY the bad blocks, not the reverse — they did not accumulate from the failures.

**Ruled OUT so far:** write ordering (values are fdatasync'd + read-back-verified in `tract.append` before any pointer is committed); shared read-back buffers in the concurrent path (`write_each` allocates a device-local scratch per thread — checked); fence/tract pressure (ZERO TractFull/Fenced/reap lines in either log); disk EIO (the only `os error` lines are network `No route to host`). Filesystems with checksums (BTRFS) would raise EIO rather than hand back silently-wrong bytes — so if the vault is NOT on a checksumming FS, silent media corruption passes straight through to the seal check.

**NOT a scaling limit on the evidence**, though Leviathan is much bigger (15 contacts / 12+ tables vs Emma's 2), and bigger = more blocks = more exposure. Commit latency IS pathological there: 227 SLOW commits on one boot, median 896ms, max 4223ms, for payloads as small as 73 bytes.

**BEFORE NUKING LEVIATHAN — capture forensics** (Nick offered to nuke; friends+MacBook+Android can backfill): copy the vault ring files aside, record the filesystem type, `smartctl -a`, `dmesg | grep -iE 'i/o error|ata|nvme|csum'`, and a memtest. Silent wrong-bytes with no EIO points at media-without-checksums or bad RAM.

**NEXT (unstarted): audit the fence window** against reap, ring migration, and the mid-ladder heartbeat commits (`commit()` on `Fenced`, `put_no_commit`'s ladder) — can the plow ever be allowed onto a block a live committed pointer still references? Deliberately did NOT patch the vault engine on a hypothesis. Related: [[project_manifestus_tombstone_bug]], [[project_storage_layering]].
