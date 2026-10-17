---
name: project_dms_age
description: DMS age = floor log2 of OSCILLATIONS ago (second 30, hour 42, day 46, year 55), floor glyphs only; sizes = floor log2 of bits (byte Ter); shown under messages in dozenal/hex modes; the Base page holds the base pills + legend
metadata:
  type: project
---

DMS (Nick 2026-09-09) = a doubling count of how long ago: each digit up = twice as long ago. Since 2026-10-01 it is a PURE logarithm of OSCILLATIONS (time's Zil = one hydrogen-line oscillation), floor glyphs only: second 30, minute 36, hour 42, day 46, month 51, year 55, lifetime 61, universe 89; zero reads "now".
Code: `crate::dms_age` / `dms_log` (lib.rs); the live Base counter keeps two digits via `dms_fine`.
Sizes use the same doubling count over BITS: `dms_size(bytes)`, floor log2 (bit = Zil, byte = Ter); gate `dms_ui()` (dozenal AND hex; arabic keeps kB/MB and unit'd ages).

The base choice is fleet-wide `display.base` (radix 12/16/10) on the Base/Dozenal settings page (rail label follows `num_base()`), with the why prose, cheat sheet and legend; About keeps the pitch, version and clock line.
**How to apply:** any new age or size display goes thru `dms_age` / `dms_size`; new dozenal explainer material belongs on the Base page, not About; docs/dozenal.md is canonical. See [[project_eagle_units_first_principles]], [[project_dozenal_datetime]], [[project_numeral_forms]].
