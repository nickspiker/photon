---
name: reference_aarch64_cross_libs
description: How to add a missing system lib to the aarch64-linux cross-build (cross-libs/aarch64 + mirrored .pc)
metadata:
  type: reference
---

The aarch64-unknown-linux-gnu cross-build links against system libs vendored into `cross-libs/aarch64/` (headers from the Fedora sysroot `/usr/aarch64-redhat-linux/sys-root/fc42`, one pkg-config file per lib in `cross-libs/aarch64/pkgconfig/`).

When a `*-sys` crate fails with "system library X was not found":
1. `dnf download --forcearch=aarch64 <pkg>`, then `rpm2cpio <pkg>.aarch64.rpm | cpio -idmv`.
2. Copy the versioned `.so` into `cross-libs/aarch64/`, then `ln -sf` both `libfoo.so.N` and the bare `libfoo.so` (the RPM ships only the runtime name; `-lfoo` needs the bare one).
3. Write `cross-libs/aarch64/pkgconfig/foo.pc` mirroring `x11.pc` (prefix = the sysroot `/usr`, `libdir=/mnt/Harbor/Code/photon/cross-libs/aarch64`).
Transitive DT_NEEDED libs (e.g. libdbus-1 → libsystemd → libcap) only need their versioned `.so` in the same dir — no `.pc`, no bare symlink.

Done for ALSA (2026-07-05, via cpal) and dbus-1 (2026-07-13, via bluer). Both crates link-only, so no headers were needed.
Related: [[feedback_build_dev_script]].
