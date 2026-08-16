---
name: feedback-vsf-readers-width-agnostic
description: VSF readers NEVER exact-match integer widths - always as_u64/as_i64/as_usize; auto-sized writes decode as concrete widths so a u(..)/i(..) match never fires on wire data
metadata:
  type: feedback
---

**The rule (Nick, 2026-08-16: "it's always a simple parse AND cast... always a reading problem"):** a VSF value is (key:value) — the KEY names the semantics, the ENCODER picks the width (a port under 256 is legitimately u3). Readers of parsed data must use the width-agnostic accessors — `as_u64()` / `as_usize()` / `as_i64()` (added 2026-08-16) — with `try_from` bounds, NEVER an exact-variant match. `VsfType::u(n, _)` / `i(n)` matches are doubly wrong: auto-sized writes DECODE as the smallest concrete width (u3/u4/…), so those arms never fire on wire data at all.

**Why:** two shipped disasters from this class in one day — `field_u64` had chain_sync + all three ckpt frames parse-dead since B3 ([[project-fleet-epoch-arc-closed]]), and the worker's `read_epoch_register` had the fanout monotonic epoch guard silently disabled (stale-replay overwrite surface) since the VSF-register conversion. 5th+6th victims of the silent-VSF-rejection class ([[vsf-toc-section-name-trap]]).

**How to apply:** every new frame/record reader goes thru the accessors; every new frame gets a build→parse ROUND-TRIP test (in-memory construction never catches this — the decode path must run). The fgtw fold's `FromVsfType` fallback arms are the pattern done right. Full sweep landed photon c44d81e + fgtw 0adfdb7 + worker 36700c2 (deployed) + vsf 383ef86.
