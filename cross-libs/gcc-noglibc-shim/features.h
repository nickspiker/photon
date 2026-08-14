/* features.h shim for GCC cross-targets that lack glibc (x86_64-pc-windows-gnu, x86_64-unknown-redox).

   glibc ships <features.h>, which defines __GNUC_PREREQ. Non-glibc GCC toolchains (MinGW, the Redox
   cross-gcc) do not. pqcrypto-mlkem's vendored PQClean compat.h, under `#if __GNUC__ && !__clang__`,
   does `#include <features.h>` then `#if !__GNUC_PREREQ(7, 1)`. Without the header this fails two ways:
     - MinGW: `fatal error: features.h: No such file or directory`
     - Redox: `error: missing binary operator before token "("`  (__GNUC_PREREQ undefined -> !(...)(7,1))
   cc-rs demotes both to cargo:warning=, so cargo only reports an opaque build-script failure.

   This shim provides exactly what compat.h needs: __GNUC_PREREQ. Wired via [env] CFLAGS_<target> in
   .cargo/config.toml. The aarch64 Windows target uses clang, takes compat.h's __clang__ branch, and never
   includes features.h -> it needs no shim. */
#ifndef _PHOTON_GCC_NOGLIBC_FEATURES_SHIM_H
#define _PHOTON_GCC_NOGLIBC_FEATURES_SHIM_H
#ifndef __GNUC_PREREQ
#  if defined(__GNUC__) && defined(__GNUC_MINOR__)
#    define __GNUC_PREREQ(maj, min) (((__GNUC__ << 16) + __GNUC_MINOR__) >= (((maj) << 16) + (min)))
#  else
#    define __GNUC_PREREQ(maj, min) 0
#  endif
#endif
#endif
