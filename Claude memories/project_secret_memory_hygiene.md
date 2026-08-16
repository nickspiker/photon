---
name: project_secret_memory_hygiene
description: hot-secret (VMK/keys) memory handling — do the software mitigations we CAN now (zeroize + mlock + no core dumps + copy discipline); get it RIGHT at the OS layer in ferros
metadata: 
  node_type: memory
  type: project
  originSessionId: 0b164fd9-062c-407e-b4bb-8f6be8d1982d
---

For any hot secret held in RAM (the fan-out-fetched vault-master-key, fleet keys, device secret), do the software protections that are achievable today and don't half-ass the OS layer in ferros.

**Do-now stack (userspace, cross-platform where possible):**
- `zeroize` (Zeroizing / ZeroizeOnDrop) — stops the COMPILER eliding the wipe. That's ALL it guarantees; it's a language-level guarantee, reliable cross-OS.
- `mlock`/`mlockall` (Linux/macOS/Android) / `VirtualLock` (Windows) — pin pages so the OS can't swap the secret to disk. Lock BEFORE the secret lands there; bounded by RLIMIT_MEMLOCK.
- Disable core dumps — RLIMIT_CORE=0 / PR_SET_DUMPABLE=0 (Unix), WER off (Windows). A crash otherwise writes the secret to disk.
- Copy discipline — fixed-size arrays not growable Vec/String (realloc leaves un-zeroed copies), no .clone() of the secret, hold it in exactly ONE place, minimal lifetime. zeroize only scrubs the location you hold, not moved-from stack slots / registers / freed buffers.

**Structurally NOT closeable in userspace (the honest residual, = the PIPE/enclave line):**
- Hibernation (suspend-to-disk) dumps ALL of RAM to hiberfil.sys regardless of mlock — app code cannot prevent it.
- Live-RAM / cold-boot extraction of a key while a session is unlocked.
Both are why hardware key sealing exists: the only way the OS can't leak the key is if it never enters OS-addressable RAM (PIPE's write-only CSR / no address / no DMA; Secure Enclave; TPM).

**Ferros mandate (the "don't fuck it up" part):** ferros is OUR OS, so the hibernation + never-in-general-RAM gaps that are unfixable on Android/desktop are OURS to close properly at the OS layer — key in a write-only region, never pageable, never in a hibernation image. Don't inherit the commodity-OS compromise into ferros; the whole point of controlling the stack is to draw the real floor here.

Per-platform flip: on iOS/Android the right home for a key this sensitive is the platform enclave/Keystore, not mlocked heap — the same vendor-API-vs-PIPE tension in [[project_keyring_design]] / the README device-ID note.

Claim honestly: "best-effort scrubbed and unswappable," NOT "guaranteed gone," with hibernation/cold-boot explicitly out of scope until hardware sealing.
Related: [[project_device_identity_model]] (PIPE endgame), the network-gated VMK design (guest-mode / vault-at-rest).
