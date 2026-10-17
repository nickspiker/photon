---
name: project_secret_memory_hygiene
description: "Hot secrets in RAM: zeroize + mlock + no core dumps + copy discipline now; hibernation/cold-boot out of scope until hardware sealing; ferros must close it at the OS layer"
metadata:
  type: project
---

For any hot secret (fleet keys, vault master key, device secret):
**Do now:** `zeroize` (stops the compiler eliding the wipe — that is all it guarantees); `mlock` / `VirtualLock` before the secret lands (bounded by RLIMIT_MEMLOCK); disable core dumps (RLIMIT_CORE=0 / PR_SET_DUMPABLE=0, WER off); copy discipline — fixed-size arrays not Vec/String, no `.clone()`, one location, minimal lifetime.
**Not closeable in userspace:** hibernation dumps all RAM regardless of mlock; live-RAM / cold-boot extraction while unlocked. Only hardware sealing (PIPE's write-only CSR, Secure Enclave, TPM) keeps a key out of OS-addressable RAM. On iOS/Android the right home is the platform Keystore/enclave.
**ferros mandate:** our OS must close it properly — key in a write-only region, never pageable, never in a hibernation image; don't inherit the commodity-OS compromise.

**Why:** a crash dump or swap file otherwise writes the secret to disk.
**How to apply:** claim "best-effort scrubbed and unswappable", never "guaranteed gone" — the same honesty as [[feedback_discard_not_shred]].
