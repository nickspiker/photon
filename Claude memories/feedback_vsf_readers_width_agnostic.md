---
name: feedback-vsf-readers-width-agnostic
description: VSF integers are canonical BOTH ways - writers always auto-size (VsfType::u/i, never hand widths), readers always widen (as_u64/as_i64, never exact-match); one encoding per number everywhere
metadata:
  type: feedback
---

**Readers (Nick 2026-08-16: "it's always a simple parse AND cast... always a reading problem"):** the KEY names the semantics, the ENCODER picks the width (a port under 256 is legitimately u3). Readers use the width-agnostic accessors `as_u64()` / `as_usize()` / `as_i64()` with `try_from` bounds, NEVER an exact-variant match — auto-sized writes decode as the smallest concrete width, so `VsfType::u(n, _)` / `i(n)` arms never fire on wire data.

**Writers (Nick 2026-08-27: "there is ONE way to encode a number on a wire correctly. One. And on disk."):** always the auto-sizing constructors `VsfType::u(value as usize, false)` / `VsfType::i(value as isize)` — never a hand-width `u3(..)`/`u5(..)`/`i6(..)` (non-canonical second wire form, or a silent-truncation trap).

**Why:** exact-match readers left chain_sync + all ckpt frames parse-dead since B3 ([[project_fleet_epoch_arc_closed]]) and silently disabled the worker's fanout epoch guard — part of the silent-VSF-rejection class ([[reference-vsf-primary-section]]).

**How to apply:** every new frame/record reader goes thru the accessors; every new frame gets a build→parse ROUND-TRIP test (in-memory construction never runs the decode path). The fgtw fold's `FromVsfType` fallback arms are the pattern done right.
