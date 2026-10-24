---
name: project-hit-wipe-damage
description: per-frame HIT_NONE wipes must stay inside ctx.damage_clip — pills re-stamp only pixels they paint, so a region-wide wipe on a narrow frame kills every pill outside the damage
metadata:
  type: project
---

CONVICTED 2026-10-24 (675d88d9): the conversation panel and Settings wiped their whole region's hit stamps every frame; fluor's `draw_pill_immediate` stamps only pixels it paints this frame (the hit stamp rides the paint decision), so on a caret-blink or chrome-only frame every pill outside the damage lost its hit — Leave neither lit nor clicked, a press read as HIT_NONE and armed a window drag.
Fix: intersect the wipe with `ctx.damage_clip`; content-painted hovers (stub pills, rows) must be in `apply_hover`'s row_hover set or they never brighten.

**Why:** narrow frames (widget-only damage, scroll memmove) are normal; any screen-wide wipe without the damage intersect reintroduces dead buttons. Same class as [[project_ready_first_paint]].
**How to apply:** a new screen that clears its hit region per frame clears `region ∩ damage_clip`; a new stub-pill hit range joins `row_hover` in driver.rs.
