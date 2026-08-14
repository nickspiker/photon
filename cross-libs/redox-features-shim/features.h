/* Redox features.h shim — CHAINS to relibc's real features.h, then adds __GNUC_PREREQ.

   Redox's relibc features.h is musl-derived and (like musl) does NOT define __GNUC_PREREQ.
   pqcrypto-mlkem's PQClean compat.h does `#include <features.h>` then `#if !__GNUC_PREREQ(7,1)`,
   assuming glibc. Unlike MinGW (which has NO features.h — a plain stub suffices there), Redox HAS
   one that its own system headers depend on, so we must NOT replace it — we #include_next the real
   one first (keeping every macro Redox's sys/time.h, stdlib.h, etc. need) and only ADD the missing
   __GNUC_PREREQ. Wired via [env] CFLAGS_x86_64_unknown_redox with this dir FIRST on the -I path. */
#ifndef _PHOTON_REDOX_FEATURES_SHIM_H
#define _PHOTON_REDOX_FEATURES_SHIM_H
#include_next <features.h>
#ifndef __GNUC_PREREQ
#  if defined(__GNUC__) && defined(__GNUC_MINOR__)
#    define __GNUC_PREREQ(maj, min) (((__GNUC__ << 16) + __GNUC_MINOR__) >= (((maj) << 16) + (min)))
#  else
#    define __GNUC_PREREQ(maj, min) 0
#  endif
#endif
#endif
