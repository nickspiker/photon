/* features.h shim for MinGW cross-builds (x86_64-pc-windows-gnu).

   glibc ships <features.h> (which defines __GNUC_PREREQ); MinGW does not.
   pqcrypto-mlkem's vendored PQClean compat.h #includes <features.h> under
   `#if defined(__GNUC__) && !defined(__clang__)` purely to call __GNUC_PREREQ(7,1),
   assuming GCC implies glibc — false for MinGW-GCC. cc-rs demotes the resulting
   `fatal error: features.h: No such file` to a cargo:warning=, so the real cause
   hides under warning noise while the build script just "fails" opaquely.

   This shim satisfies the include with only what compat.h needs: __GNUC_PREREQ.
   Wired via [env] CFLAGS_x86_64_pc_windows_gnu in .cargo/config.toml. The aarch64
   Windows target uses clang (llvm-mingw), which takes compat.h's __clang__ branch
   and never includes features.h, so it needs no shim. */
#ifndef _PHOTON_MINGW_FEATURES_SHIM_H
#define _PHOTON_MINGW_FEATURES_SHIM_H
#ifndef __GNUC_PREREQ
#  if defined(__GNUC__) && defined(__GNUC_MINOR__)
#    define __GNUC_PREREQ(maj, min) (((__GNUC__ << 16) + __GNUC_MINOR__) >= (((maj) << 16) + (min)))
#  else
#    define __GNUC_PREREQ(maj, min) 0
#  endif
#endif
#endif
