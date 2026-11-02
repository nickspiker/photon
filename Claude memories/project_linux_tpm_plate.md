---
name: project_linux_tpm_plate
description: "2026-11-02 Linux TPM plate BUILT (tohu device::tpm_plate, raw TPM 2.0 over /dev/tpmrm0, EK template, hashed, flushed); photon logs it; needs the user in tss; UNVERIFIED against a real chip; key half (HMAC primary) and Windows TBS next"
metadata:
  node_type: memory
  type: project
  originSessionId: d6c9db89-ccef-475b-b38b-64f51976930f
  modified: 2026-11-02T23:07:50.406Z
---

Nick picked Linux as the next platform after Android (2026-11-02, "Poke away, tis all yours!"). Desktop is where the oracle is weakest: machine-id is world-readable and per install, every VM is a free device.

BUILT 2026-11-02: tohu `device::tpm_plate() -> io::Result<[u8;32]>` (cfg linux, `mod tpm` in lib.rs): hand-marshalled TPM2_CreatePrimary under TPM_RH_ENDORSEMENT with the TCG EK Credential Profile RSA-2048 template (attributes 0x000300B2, the fixed PolicySecret digest 8371…69aa, AES-128-CFB, null scheme, 256-byte zero unique), password session with empty auth, over /dev/tpmrm0 (open rw, one write, one read); parses outPublic (size-prefixed TPM2B_PUBLIC, the same bytes `tpm2_createek -G rsa -u` writes, so `b3sum` of that file equals the plate) → BLAKE3; FlushContext on the transient handle. Two unit tests cover the marshalling and the response parse. No C library: tohu's dependency-list audit claim holds. photon driver.rs core_init (cfg linux) logs `device plate: tpm <prefix>` on a worker thread, or `a TPM is present but Photon may not open it (add this user to the tss group)`, or `none (<e>)`. NOTHING derives from the plate yet.

ACCESS: /dev/tpmrm0 is root:tss 0660 on Fedora/Debian/Ubuntu; Photon installs without root, so the plate is opportunistic: present when the user ran `sudo usermod -aG tss <user>` once, absent otherwise = today's machine-id posture. Windows TBS needs no elevation.

VERIFIED 2026-11-02 on this box's AMD fTPM: Nick ran `sudo usermod -aG tss nick` + `newgrp tss`; two `tpm2_createek -G rsa -u` runs were byte-identical, b3sum = c823b5cd…e034, and tohu's tpm_plate() (throwaway example run under `sg tss`) returned the SAME 32 bytes. Trap: `tpm2_createek -c -` PERSISTS the EK (0x81000000 + 0x81000003, evicted with tpm2_evictcontrol -C o; pre-existing handles 0x81000001/2 and 0x81010001 left alone). `sg tss -c "<cmd>"` reaches the chip from a Claude session without re-login. Photon's own `device plate: tpm c823b5cd` log line still unseen (needs Nick's re-login + relaunch).

KEY HALF BUILT + VERIFIED 2026-11-02 (tohu cb488db): `device::tpm_hmac(msg)` = CreatePrimary keyed-hash HMAC-SHA256 template (attrs 0x00040472, policy = the 32-byte literal "tohu.tpm.hmac-primary.v0"+zeros, scheme HMAC/SHA256) under RH_ENDORSEMENT → TPM2_HMAC → FlushContext; `device::tpm_root()` = HMAC over "tohu.device-root.v0". On the fTPM: root 1408209a… identical across calls, different message → different tag, transient + persistent handle lists unchanged after. Photon driver.rs logs `device plate: tpm <p>, key root <r>` (prefixes). NOTHING derives from it yet; the swap to device_secret = BLAKE3(tpm_root) is a v0-source swap + thirty-version conversion pin like Android (Nick's call when). Threat model Nick stated: "they could if they had the box and knew the handle but that's about it, one or the other, nope".

VERIFIED IN-APP 2026-11-02 23:17 after Nick rebooted (tss membership live) and dev.sh reloaded: `device plate: tpm c823b5cd, key root 1408209a`, both stable across the reboot. NEXT: Windows thru TBS (Tbsip_Submit_Command, same raw commands; open question: can a standard user CreatePrimary under the endorsement hierarchy); then the answered form (AK + certification) with a verifier side. Caveats in docs: fTPM seed resets on firmware reset/CPU swap = new plate; AMD/Intel fTPM EK certificates live on vendor servers (only the answered form needs them).

Related: [[project_android_widevine_plate]], [[feedback_device_root_never_salted]], tohu/docs/oracle-hardening.md.
