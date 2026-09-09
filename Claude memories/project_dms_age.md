---
name: project_dms_age
description: DMS = the dozenal age SHIPPED 2026-09-09 — bit length of seconds-ago (0 now, 1 = 1 s, 12 = ZilaZil ≈ an hour, 17 day, 22 month, 25 year, 58 universe); Dozenal settings page (rail reads Dozenal/Hexadecimal/Arabic) holds three base pills + why + cheat sheet + legend; NumBase{Dozenal,Hex,Arabic} = display.base radix
metadata:
  type: project
---

DMS (Nick 2026-09-09: "log2 seconds from now — now is zil, one sec zila, zilor 2, ter 4, tera 8") = the BIT LENGTH of the seconds-ago count: `crate::dms_bits` / `crate::dms_age` (glyphs). Each digit up = twice as long ago. Zila Zil (12) ≈ an hour, Zila Teror (17) a day, Zila Stela (22) a month, Zilor Zila (25) a year, Tera Stela (58) the age of the universe — all of time fits in two dozenal digits, so triple digits never occur. A fractional dozenal digit would be a semitone of time (twelfth of an octave) — not built, an option.

Shown: the message details-strip age in dozenal mode (`Msg::AgoDms`); arabic mode keeps the unit'd count. The legend lives on the DOZENAL settings page (`SettingsPage::Dozenal`, rail label reads Dozenal/Hexadecimal/Arabic per `crate::num_base()`), which also took the fleet-wide base choice (three pills; `display.base` = radix 12/16/10, `display.dozenal` bool still read as fallback; hex = plain 0-F digits, zoom per-256, durations M:SS in hex; `dozenal_ui()` now means base == Dozenal and gates the twelve-shaped sites only), the why / why-YOU-dozenal prose (the decimal-mode easter egg), the digit cheat sheet (+ custodian riddle on tap), from About. About keeps the pitch, the version, the clock line. Nick said a separate toggle for DMS "may" come later.

**How to apply:** any new age display goes thru `dms_age` in dozenal mode; new dozenal explainer material belongs on the Dozenal page, not About. See [[project_dozenal_datetime]], [[project_languages]].

SIZES (Nick 2026-09-09, "I say bits ... just like time in seconds"): a file / recording / log size renders as the SAME doubling count over BITS — `dms_size(bytes)` = fmt_num(bit length of bytes×8): a byte is Tera (4), a kilobyte Zila Tera (14), a megabyte Zilor Zil (24), a gigabyte Zilor Tera (34). Gate = `dms_ui()` (dozenal AND hex; arabic keeps kB/MB and the unit'd age). Sites: `types::size_label` → RecordingBubble/FileBubble `size: &str`, `human_bytes` (Diagnostics + log toast), and the age gate in the detail strip now uses dms_ui too. Bare, no sigil, like the age; the Dozenal page legend still lists seconds only (a bytes column is the open follow-up).
