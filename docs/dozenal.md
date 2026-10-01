# Numerals: the two forms, DMS, and the one atom

Photon renders every numeral in the base the person chose (`NumBase`: dozenal by default, hexadecimal, arabic). Binary at rest, always; the base is applied at the render edge thru the helpers in `src/lib.rs` (`fmt_num`, `fmt_mag`, `fmt_share`, `dms_size`, `dms_age`, `dms_length`, `dms_fine`, `link_freq_label`, `fmt_halves`, `unit_size`). The digit names are photon vocabulary and are never translated (docs/languages.md). The Base settings page is the lesson: it teaches everything below, in this order, in the person's own base.

## The digits and their names

Twelve glyphs, drawn by the Oxanium `+glyphs` face at 0x10..0x1B. The names carry the values: four stems count by threes (Zil 0, Ter 3, Lun 6, Stel 9) and three endings add nothing, one or two (plain, -a, -or). Lunor is Lun + 2 = 8; Stela is Stel + 1 = 10. Zila Zil is twelve, a dozen; Zila Zil Zil a gross.

Two marks, both chosen because the Zil glyph reads as a dash (Nick 2026-10-01): the radix point is a RAISED dot, U+00B7 (`DOZENAL_POINT`), and the sign of a signed doubling count is an ARROW, ALWAYS SHOWN — `↑` at or above the unit, `↓` below (`DMS_UP`/`DMS_DOWN`): `↑Ter` a person, `↓Tera` a coin, `↑Zil` the unit itself, so a signed column aligns. Unsigned scales (age, size, counts, a reputation's support) carry no arrow. A bar and the fraction slash were tried and both read as a digit. Oxanium lacks the arrows; Noto Symbols in the fallback chain draws them.

Plain arithmetic carries at twelve: Zila + Zila = Zilor, Ter + Tera = Luna, Lun + Lun = Zila Zil, Ter × Tera = Zila Zil. After the point the digits are twelfths, then gross-ths, and the shares people use are exact single digits: .Lun a half, .Tera a third, .Ter a quarter, .Zilor a sixth, .Zila a twelfth.

## The two forms (and names)

Every number on screen is one of two kinds, and the kind decides the form, not the screen it sits on (Nick 2026-09-15, "everything as exponential magnitude when in dozenal": the earlier three-form system with linear counts is retired).
- **Magnitudes** — how much: size, age, rate, length, AND counts (peers, devices, messages). **Dozenal Metric Scaling**: the doubling count, spelled dozenal, thru `fmt_mag` (counts, one fraction digit) or the per-scale `dms_*` helpers. Tera of size is sixteen bits; a count of three is Zila.Luna.
- **Shares** — how much of a whole: a battery, a download, a grade. A radix point and the digits after it, thru `fmt_share`: .Zil none, .Ter a quarter, .Lun a half, .Stelor nearly whole. The dot is the marker.
- **Names** are neither: a version, an era, the day of a month, a port, a hash, a handle. Plain digits, never scaled.

## DMS: doublings, spelled dozenal

A quantity shows as how many times its unit has doubled: the floor of log base two of the ratio, written in dozenal digits. One unit reads Zil, two Zila, four Zilor, eight Ter. Zero has no logarithm and reads as a word: an age of nothing is "now", a size of nothing is "empty". Nick, 2026-09-11: "straight up log base something conversion; the only evenly spaced scale on this slide rule". It is a logarithm rather than a count, so a bit and a terabyte, a second and the age of the universe, each fit in two digits, and halving or doubling, the only step people feel, is one digit either way. "Metric" is the doubling; "dozenal" is only the numeral it is written in.

**The floor rule** (Nick 2026-09-30, "I don't say I'm six foot because I'm five eleven"): a reading of k means AT LEAST 2^k of the unit, for halvings too. A coin at −3.4 reads −Tera (it is at least a sixteenth of the wavelength); a grain of rice at −11.15 hydrogens-offset reads −Zila Zil, not −Stelor. `doublings_of` is `log2().floor()`.

**Arithmetic on doubling counts.** Multiply = add the counts (Ter × Tera = Luna: 8 × 16 = 128). Divide = subtract. Add is the awkward one: the bigger wins and the smaller nudges it up — the same size again adds one whole doubling (Lun + Lun = Luna as magnitudes), one doubling smaller adds .Luna (log2 1.5 = .585 ≈ 7/12), two smaller .Ter, three .Zilor, four .Zila, five or more nothing visible. A count times a size is an add, because a count is a doubling count too: three files of a megabyte = Zila.Luna + Zila Stelor = Zilor Zil.Luna.

**Fine form** (`dms_fine`): the floor, a radix point, then fraction-of-a-doubling digits, each a twelfth then a gross-th of one doubling (.Lun = ×√2, .Tera = ×2^(1/3)). One fraction digit ≈ 6 %, two ≈ 0.5 %. "Four digits" = two before the point, two after: a 51 kg teenager is Luna Stela.Luna Lun hydrogens (2^94.625). The floor rule holds at the last digit shown; both fraction digits are always drawn (the width is the precision — unlike a share, where a trailing Zil is unearned). Only ratios ≥ 1 get fraction digits.

## The one atom

Every scale counts doublings of a property of a single atom, hydrogen-1 (protium: one proton, one bound electron, in the lower hyperfine level), whose hyperfine line is already photon's clock (`vsf::OSCILLATIONS_PER_SECOND`, defined in the Milky Way–Andromeda barycentric frame). Definitional, not conventional; and each unit is placed so the range people care about sits above it. Because the scale is a logarithm, a unit is only where Zil sits, and moving it by a whole number of doublings costs nothing: an addend, never a factor, and a dozen-multiple addend leaves the low digit unchanged.

| scale | one is | why | below one | shown today |
|---|---|---|---|---|
| size | a bit | information is dimensionless; the one unit that is not the atom's | nothing (Zil is one bit; "empty" is the word) | yes |
| age, duration | ONE OSCILLATION of the line (2026-09-30: "1 second is not Zil" — the Eagle second was 2^30.4 oscillations, a factor, which the addend rule forbids) | the clock's own tick, offset zero like every other scale; a second is a landmark at Zilor Lun.Tera, a minute Ter Zil, an hour Ter Lun, a day Ter Stela, a year Tera Luna, the universe Luna Teror | nothing is below one; "now" is the word for no age at all | yes, live: a message's age is the floor glyphs, woken at each doubling; the Base counter shows two fraction digits, woken every t/208 |
| rate, latency | a round trip is a DURATION, same scale | with Zil at one oscillation every span is positive, so the hertz inversion retired (`link_rtt_label`) | 66 ms reads Zilor Zilor.Lun | yes |
| time of day | not DMS: a SHARE of today, fixed width (`day_share_glyphs`) | a day is a cycle, and a position in a cycle is a share, which is linear by nature — "how much is a log, where is a share"; Zil midnight, Lun noon, digits of two hours, ten minutes, fifty seconds, four seconds, a third of a second, a thirty-fifth (inksurf convention) | hex = seconds since local midnight; arabic = the wall clock | yes, live on the Base page (six digits) |
| a date | names: `year month-glyph day weekday .share`, spaces only, months zero-indexed glyphs, weekdays words (`fmt_when`) | always the whole line — a bare share on its own read as a decimal with no context (Nick 2026-10-01) | hex = the raw oscillation stamp since the Eagle epoch | under a tapped message |
| length | one wavelength of the line, 21.106 cm: the distance light travels in one Eagle oscillation | a property of the universe rather than a king's foot | a minus counts halvings: −Zila a hand, −Tera a coin, −Zila Zil a hair | yes |
| speed | c, the light that joins the two above | the only speed that is the same for everyone; rest is −∞ (there is no absolute rest) | everything with mass: walking −Zilor Tera, a car −Zilor Zil, sound −Zila Lunor, orbit −Zila Tera | imagined |
| temperature | the line's own photon, h·f/k_B = 0.068 K | k_B = 1; absolute zero is −∞ (nothing reaches it) | freezing Stelor, a room Zila Zil, the Sun Zila Tera, Planck Stel Zilor, all positive without an offset | imagined |
| mass | the protium atom itself | mass = how many hydrogens; a second property of the SAME atom, not a photon's mass-equivalent | positive from a molecule up: person Luna Stelor, car Lunor Ter, Earth three digits | imagined |
| grade (reputation) | even: praise and dings in balance, or nothing yet | r = log2((P+1)/(N+1)), signed doublings of praise over dings, distinct people; a spotless record's reading IS its evidence, and the gap under perfect is −r (the reciprocal), so perfect is off the top like light on the speed scale | −Zila: dings outweigh praise two to one; beside it s = log2(P+N+1), what stands behind it, equal to r only when spotless | ladder only |

**Reputation in log form (2026-09-30, "−1 is the reciprocal").** The share form (1 − 1/E) was retired: it could not show a ding on a big record at all (20,000 reviews read .Stelor Stelor before and after). `rep_reading(P, N)` = log2((P+1)/(N+1)) with one fraction digit, negative rendered as the minus of the swapped pair; `rep_support` = log2(P+N+1). A ding halves the odds, so the first costs ~1 doubling whoever you are, then .Luna, .Ter, .Zilor, .Zila, nothing — the size-add rule backwards. Twenty thousand spotless: Zila Zilor.Ter; one ding: Zila Zila.Ter. Four spotless: Zilor.Ter; one ding: Zila.Ter; one for and four against: −Zila.Ter. Volume cannot buy a doubling (one person is one piece); only distinct people can. No total, no rank, per claim, attached to who gave it — unchanged.

The atom's rest energy is 2^47.18 line photons: the one measured, non-integer constant in the system (it plays the role h plays in SI), harmless because nothing in photon converts mass to energy. Anchoring length at the Planck length was considered and rejected: the human scale disappears from the digits. Anchoring mass on a photon's mass-equivalent was rejected 2026-09-30: a photon has no mass.

**Time's Zil is the oscillation (built 2026-10-01).** The Eagle second (1,420,407,826 oscillations, the SI second measured in hydrogen) was the one inherited convention and sat a non-integer 30.4 doublings from the anchor; no integer offset was kept either, so every scale is at offset zero and c = Zil. The epoch and the oscillation clock never moved — this is the render edge only. Live readings wake at digit edges, never timers: a two-fraction-digit age's last digit ticks every t/208 (one tick per frame at 208 frames whatever the rate, then every 2, 4, 8 frames), the twelfths digit every t/17, the whole doubling at frames 1, 2, 4, 8.

## Hexadecimal and arabic

Hexadecimal is linear everywhere and shows what the machine holds: ages and durations as the plain seconds count, sizes as the bit count, a round trip in seconds with a hexadecimal fraction (66 ms is 0.10E), a length in millimetres, with no scaling and no M:SS. Arabic shows the ledger world's conventional units. Diagnostics record timestamps are wall-clock coordinates for correlating with photonlog and adb, and stay arabic clock time in every base.

The Base page's pills and cheat-sheet columns run in historical order — arabic, hexadecimal, dozenal — with dozenal the default. The page explains and never argues: no "fingers" line, no scold; the case for twelve is the exact thirds beside the reputation ladder, where it does work. "Base ten" is not a name in photon's text: every base writes itself as 10, so the numerals are called arabic, the way the pill is.

## The length scale, one row per doubling

The unit is the hydrogen line's wavelength (`HYDROGEN_LINE_METRES` = c / `vsf::OSCILLATIONS_PER_SECOND`). Each row is twice the last; a minus counts halvings below the unit.

| doublings | reads | length | landmark |
|---|---|---|---|
| -114 | −StelLun | 0.0102 qm | Planck length |
| -113 | −StelTeror | 0.0203 qm |  |
| -112 | −StelTera | 0.0406 qm |  |
| -111 | −StelTer | 0.0813 qm |  |
| -110 | −StelZilor | 0.163 qm |  |
| -109 | −StelZila | 0.325 qm |  |
| -108 | −StelZil | 0.65 qm |  |
| -107 | −LunorStelor | 1.3 qm |  |
| -106 | −LunorStela | 2.6 qm |  |
| -105 | −LunorStel | 5.2 qm |  |
| -104 | −LunorLunor | 10.4 qm |  |
| -103 | −LunorLuna | 20.8 qm |  |
| -102 | −LunorLun | 41.6 qm |  |
| -101 | −LunorTeror | 83.2 qm |  |
| -100 | −LunorTera | 166 qm |  |
| -99 | −LunorTer | 333 qm |  |
| -98 | −LunorZilor | 666 qm |  |
| -97 | −LunorZila | 1.33 rm |  |
| -96 | −LunorZil | 2.66 rm |  |
| -95 | −LunaStelor | 5.33 rm |  |
| -94 | −LunaStela | 10.7 rm |  |
| -93 | −LunaStel | 21.3 rm |  |
| -92 | −LunaLunor | 42.6 rm |  |
| -91 | −LunaLuna | 85.2 rm |  |
| -90 | −LunaLun | 170 rm |  |
| -89 | −LunaTeror | 341 rm |  |
| -88 | −LunaTera | 682 rm |  |
| -87 | −LunaTer | 1.36 ym |  |
| -86 | −LunaZilor | 2.73 ym |  |
| -85 | −LunaZila | 5.46 ym |  |
| -84 | −LunaZil | 10.9 ym |  |
| -83 | −LunStelor | 21.8 ym |  |
| -82 | −LunStela | 43.6 ym |  |
| -81 | −LunStel | 87.3 ym |  |
| -80 | −LunLunor | 175 ym |  |
| -79 | −LunLuna | 349 ym |  |
| -78 | −LunLun | 698 ym |  |
| -77 | −LunTeror | 1.4 zm |  |
| -76 | −LunTera | 2.79 zm |  |
| -75 | −LunTer | 5.59 zm |  |
| -74 | −LunZilor | 11.2 zm |  |
| -73 | −LunZila | 22.3 zm |  |
| -72 | −LunZil | 44.7 zm |  |
| -71 | −TerorStelor | 89.4 zm |  |
| -70 | −TerorStela | 179 zm |  |
| -69 | −TerorStel | 358 zm |  |
| -68 | −TerorLunor | 715 zm |  |
| -67 | −TerorLuna | 1.43 am |  |
| -66 | −TerorLun | 2.86 am |  |
| -65 | −TerorTeror | 5.72 am |  |
| -64 | −TerorTera | 11.4 am |  |
| -63 | −TerorTer | 22.9 am |  |
| -62 | −TerorZilor | 45.8 am |  |
| -61 | −TerorZila | 91.5 am |  |
| -60 | −TerorZil | 183 am |  |
| -59 | −TeraStelor | 366 am |  |
| -58 | −TeraStela | 732 am |  |
| -57 | −TeraStel | 1.46 fm |  |
| -56 | −TeraLunor | 2.93 fm |  |
| -55 | −TeraLuna | 5.86 fm |  |
| -54 | −TeraLun | 11.7 fm |  |
| -53 | −TeraTeror | 23.4 fm |  |
| -52 | −TeraTera | 46.9 fm |  |
| -51 | −TeraTer | 93.7 fm |  |
| -50 | −TeraZilor | 187 fm |  |
| -49 | −TeraZila | 375 fm |  |
| -48 | −TeraZil | 750 fm | proton radius |
| -47 | −TerStelor | 1.5 pm |  |
| -46 | −TerStela | 3 pm |  |
| -45 | −TerStel | 6 pm |  |
| -44 | −TerLunor | 12 pm |  |
| -43 | −TerLuna | 24 pm |  |
| -42 | −TerLun | 48 pm |  |
| -41 | −TerTeror | 96 pm |  |
| -40 | −TerTera | 192 pm |  |
| -39 | −TerTer | 384 pm |  |
| -38 | −TerZilor | 768 pm |  |
| -37 | −TerZila | 1.54 nm |  |
| -36 | −TerZil | 3.07 nm |  |
| -35 | −ZilorStelor | 6.14 nm |  |
| -34 | −ZilorStela | 12.3 nm |  |
| -33 | −ZilorStel | 24.6 nm |  |
| -32 | −ZilorLunor | 49.1 nm | hydrogen atom (Bohr radius) |
| -31 | −ZilorLuna | 98.3 nm |  |
| -30 | −ZilorLun | 197 nm |  |
| -29 | −ZilorTeror | 393 nm |  |
| -28 | −ZilorTera | 786 nm |  |
| -27 | −ZilorTer | 1.57 µm | DNA strand width |
| -26 | −ZilorZilor | 3.15 µm |  |
| -25 | −ZilorZila | 6.29 µm |  |
| -24 | −ZilorZil | 12.6 µm |  |
| -23 | −ZilaStelor | 25.2 µm |  |
| -22 | −ZilaStela | 50.3 µm | a virus |
| -21 | −ZilaStel | 101 µm |  |
| -20 | −ZilaLunor | 201 µm |  |
| -19 | −ZilaLuna | 403 µm |  |
| -18 | −ZilaLun | 805 µm |  |
| -17 | −ZilaTeror | 1.61 mm |  |
| -16 | −ZilaTera | 3.22 mm |  |
| -15 | −ZilaTer | 6.44 mm | red blood cell |
| -14 | −ZilaZilor | 12.9 mm |  |
| -13 | −ZilaZila | 25.8 mm |  |
| -12 | −ZilaZil | 51.5 mm | a hair's width |
| -11 | −Stelor | 103 mm |  |
| -10 | −Stela | 206 mm |  |
| -9 | −Stel | 412 mm |  |
| -8 | −Lunor | 824 mm | a millimetre |
| -7 | −Luna | 0.165 cm |  |
| -6 | −Lun | 0.33 cm | an ant |
| -5 | −Teror | 0.66 cm |  |
| -4 | −Tera | 1.32 cm | a coin |
| -3 | −Ter | 2.64 cm |  |
| -2 | −Zilor | 5.28 cm |  |
| -1 | −Zila | 10.6 cm | a hand |
| +0 | Zil | 21.1 cm | THE UNIT: one hydrogen-line wavelength, light per Eagle oscillation |
| +1 | Zila | 42.2 cm |  |
| +2 | Zilor | 84.4 cm | a bald eagle, nose to tail; a metre |
| +3 | Ter | 1.69 m | a person |
| +4 | Tera | 3.38 m | a car |
| +5 | Teror | 6.75 m |  |
| +6 | Lun | 13.5 m |  |
| +7 | Luna | 27 m | a blue whale |
| +8 | Lunor | 54 m | a football pitch |
| +9 | Stel | 108 m |  |
| +10 | Stela | 216 m | the Eiffel Tower |
| +11 | Stelor | 432 m |  |
| +12 | ZilaZil | 865 m | a kilometre; a mile |
| +13 | ZilaZila | 1.73 km |  |
| +14 | ZilaZilor | 3.46 km |  |
| +15 | ZilaTer | 6.92 km | Everest |
| +16 | ZilaTera | 13.8 km |  |
| +17 | ZilaTeror | 27.7 km | a marathon |
| +18 | ZilaLun | 55.3 km |  |
| +19 | ZilaLuna | 111 km |  |
| +20 | ZilaLunor | 221 km |  |
| +21 | ZilaStel | 443 km |  |
| +22 | ZilaStela | 885 km |  |
| +23 | ZilaStelor | 1.77e+03 km |  |
| +24 | ZilorZil | 3.54e+03 km | Earth's radius |
| +25 | ZilorZila | 7.08e+03 km |  |
| +26 | ZilorZilor | 1.42e+04 km |  |
| +27 | ZilorTer | 2.83e+04 km |  |
| +28 | ZilorTera | 5.67e+04 km |  |
| +29 | ZilorTeror | 1.13e+05 km |  |
| +30 | ZilorLun | 2.27e+05 km | a light-second; the Moon |
| +31 | ZilorLuna | 4.53e+05 km |  |
| +32 | ZilorLunor | 9.06e+05 km | the Sun's diameter |
| +33 | ZilorStel | 1.81e+06 km |  |
| +34 | ZilorStela | 3.63e+06 km |  |
| +35 | ZilorStelor | 7.25e+06 km |  |
| +36 | TerZil | 1.45e+07 km | a light-minute |
| +37 | TerZila | 2.9e+07 km |  |
| +38 | TerZilor | 5.8e+07 km |  |
| +39 | TerTer | 1.16e+08 km | the Sun (one AU) |
| +40 | TerTera | 2.32e+08 km |  |
| +41 | TerTeror | 4.64e+08 km |  |
| +42 | TerLun | 9.28e+08 km |  |
| +43 | TerLuna | 1.86e+09 km |  |
| +44 | TerLunor | 3.71e+09 km | Pluto |
| +45 | TerStel | 7.43e+09 km |  |
| +46 | TerStela | 1.49e+10 km | Voyager 1 |
| +47 | TerStelor | 2.97e+10 km |  |
| +48 | TeraZil | 5.94e+10 km |  |
| +49 | TeraZila | 1.19e+11 km |  |
| +50 | TeraZilor | 2.38e+11 km |  |
| +51 | TeraTer | 4.75e+11 km |  |
| +52 | TeraTera | 0.1 ly |  |
| +53 | TeraTeror | 0.201 ly |  |
| +54 | TeraLun | 0.402 ly |  |
| +55 | TeraLuna | 0.804 ly | a light-year |
| +56 | TeraLunor | 1.61 ly |  |
| +57 | TeraStel | 3.21 ly | Proxima Centauri |
| +58 | TeraStela | 6.43 ly |  |
| +59 | TeraStelor | 12.9 ly |  |
| +60 | TerorZil | 25.7 ly |  |
| +61 | TerorZila | 51.4 ly |  |
| +62 | TerorZilor | 103 ly |  |
| +63 | TerorTer | 206 ly |  |
| +64 | TerorTera | 412 ly |  |
| +65 | TerorTeror | 823 ly |  |
| +66 | TerorLun | 1.65e+03 ly |  |
| +67 | TerorLuna | 3.29e+03 ly |  |
| +68 | TerorLunor | 6.58e+03 ly |  |
| +69 | TerorStel | 1.32e+04 ly |  |
| +70 | TerorStela | 2.63e+04 ly |  |
| +71 | TerorStelor | 5.27e+04 ly | the Milky Way, across |
| +72 | LunZil | 1.05e+05 ly |  |
| +73 | LunZila | 2.11e+05 ly |  |
| +74 | LunZilor | 4.21e+05 ly |  |
| +75 | LunTer | 8.43e+05 ly |  |
| +76 | LunTera | 1.69e+06 ly | Andromeda |
| +77 | LunTeror | 3.37e+06 ly |  |
| +78 | LunLun | 6.74e+06 ly |  |
| +79 | LunLuna | 1.35e+07 ly |  |
| +80 | LunLunor | 2.7e+07 ly |  |
| +81 | LunStel | 5.39e+07 ly |  |
| +82 | LunStela | 1.08e+08 ly |  |
| +83 | LunStelor | 2.16e+08 ly |  |
| +84 | LunaZil | 4.32e+08 ly |  |
| +85 | LunaZila | 8.63e+08 ly |  |
| +86 | LunaZilor | 1.73e+09 ly |  |
| +87 | LunaTer | 3.45e+09 ly |  |
| +88 | LunaTera | 6.9e+09 ly |  |
| +89 | LunaTeror | 1.38e+10 ly |  |
| +90 | LunaLun | 2.76e+10 ly |  |
| +91 | LunaLuna | 5.52e+10 ly | the observable universe, across |
| +92 | LunaLunor | 1.1e+11 ly |  |
