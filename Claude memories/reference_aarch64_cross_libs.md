---
name: reference_aarch64_cross_libs
description: How to add a missing system lib to the aarch64-linux cross-build (cross-libs/aarch64 + mirrored .pc)
metadata: 
  node_type: memory
  type: reference
  originSessionId: 71fe9349-599c-4d2c-9970-3a988ee9a08f
---

The aarch64-unknown-linux-gnu cross-build (driven by `build-release.sh`, config in `.cargo/config.toml` `[target.aarch64-unknown-linux-gnu]`) links against system libs VENDORED into `cross-libs/aarch64/` — headers come from the Fedora aarch64 sysroot `/usr/aarch64-redhat-linux/sys-root/fc42`, the `.so` is vendored, and each lib has a pkg-config file in `cross-libs/aarch64/pkgconfig/` (build-release.sh points `PKG_CONFIG_PATH/LIBDIR_aarch64_unknown_linux_gnu` there).

When a `*-sys` crate's build fails with "system library X was not found", vendor it:
1. `dnf download --forcearch=aarch64 <pkg>` (non-sudo, download-only), then `rpm2cpio <pkg>.aarch64.rpm | cpio -idmv` to extract the `.so`.
2. Copy the versioned `.so` (e.g. `libfoo.so.N.M.K`) into `cross-libs/aarch64/`, then `ln -sf` the `libfoo.so.N` AND the bare `libfoo.so` linker symlink (the RPM only ships the runtime `.so.N`; the `-devel` symlink `libfoo.so` is what `-lfoo` resolves — you must create it).
3. Write `cross-libs/aarch64/pkgconfig/foo.pc` mirroring `x11.pc`: `prefix=/usr/aarch64-redhat-linux/sys-root/fc42/usr`, `libdir=/mnt/Octopus/Code/photon/cross-libs/aarch64`, `Libs: -L${libdir} -lfoo`, `Cflags: -I${includedir}`.

Done 2026-07-05 for ALSA (`libasound`, from `chirp`→`rodio`→`cpal`→`alsa-sys`; the chime is a hard dep pulled into the ARM-Linux desktop build). `alsa-sys` only LINKS (bindgen off, ships pre-gen bindings) so no headers were needed. Verify with `PHOTON_ALLOW_RELEASE=1 cargo build --release --target aarch64-unknown-linux-gnu` (build.rs panics on release without that env — [[feedback_build_dev_script]]).

Done 2026-07-13 for dbus-1 (from `bluer`→`dbus`→`libdbus-sys`, the pairing-v2 beacon scanner): same recipe, PLUS a transitive wrinkle — Fedora's libdbus-1 DT_NEEDs libsystemd.so.0 which DT_NEEDs libcap.so.2; vendor BOTH versioned `.so`s into `cross-libs/aarch64/` too (rpath-link resolves transitives from that dir; no `.pc` and no bare `.so` symlink needed for transitives, only for the directly-linked lib). `libdbus-sys` also links-only, headerless Cflags fine.
