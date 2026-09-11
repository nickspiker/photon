# Numerals: the three forms, DMS, and the unit of account

Photon renders every numeral in the base the person chose (`NumBase`: dozenal by default, hexadecimal, arabic). Binary at rest, always; the base is applied at the render edge thru the helpers in `src/lib.rs` (`fmt_num`, `dms_size`, `dms_age`, `dms_length`, `link_freq_label`, `fmt_halves`, `unit_size`). The digit names are photon vocabulary and are never translated (docs/languages.md).

## The three forms

Every number on screen is one of three kinds, and the kind decides the form, not the screen it sits on.
- **Counts** (peers, devices, unread, chunk three of five) are linear digits in the base. Tera peers is four peers.
- **Magnitudes** (size, age, rate, length; anything you would otherwise give a unit prefix) are **Dozenal Metric Scaling**: doublings, spelled dozenal. Tera of size is sixteen bits.
- **Proportions** (a battery, a download, a rung out of the ladder, a vote share) are one dozenal fraction digit with a dot: .Zil none, .Ter a quarter, .Lun a half, .Stelor nearly whole.
Identifiers (ports, hashes, handles, versions) are never converted. Reputation, when it is built, is a magnitude: signed doublings from the median peer, not a rating.

The same word means a different amount on each scale (Tera is four peers, sixteen bits, or a third), so each form has its own face: plain digits for counts, the `+glyphs` face for the scaling, the dot prefix for a fraction.

## DMS: doublings, spelled dozenal

A quantity shows as how many times its unit has doubled: the bit length of the count, written in dozenal digits (Zil 0, Zila 1, Zilor 2, Ter 3, Tera 4, Teror 5, Lun 6, Luna 7, Lunor 8, Stel 9, Stela 10, Stelor 11). One unit reads Zila, two Zilor, four Ter, eight Tera. It is a logarithm rather than a count, so a bit and a terabyte, a second and the age of the universe, each fit in two digits, and halving or doubling, the only step people feel, is one digit either way. "Metric" is the doubling; "dozenal" is only the numeral it is written in.

## The unit of account

Every scale counts doublings of one physical anchor, chosen so that it is definitional rather than conventional, and so that the range people care about sits above it, where no sign is needed.

| scale | one is | why | below one |
|---|---|---|---|
| size | a bit | the smallest thing that exists in a message | nothing (Zil) |
| age, duration | an Eagle second: 1,420,407,826 oscillations of the hydrogen line | photon's own second, already its clock | reads Zil, "now"; sub-second spans are shown inverted, as a frequency |
| rate, latency | one hertz | a round trip's interesting range is 5 ms to 2 s, 0.5 to 200 Hz, so inverting keeps the floor at one | 1 Hz reads Zila; slower than a second is a failure, not a number |
| length | one wavelength of the hydrogen line, 21.106 cm: the distance light travels in one Eagle oscillation | the one length that is a property of the universe rather than a king's foot, and it is already photon's clock | a minus counts halvings: −Zila is 10.6 cm, −Tera a coin, −ZilaZil a hair; the one scale where sub-unit is everyday, so the sign earns its keep |
| reputation (future) | the median peer in the category | how much, relative to everyone: unbounded and skewed | signed: −Zila is half the median |
| proportions | not DMS: a dot-fraction of the whole, one digit | bounded things are linear | .Zil none, .Lun half, .Stelor nearly whole |

Anchoring length at the Planck length so that every value is positive was considered and rejected: every everyday length then reads as two digits in the Stel range (a person is StelStela) and the human scale disappears from the digits. The hydrogen anchor keeps a hand to a house inside one digit at the cost of a minus on the small.

## Hexadecimal and arabic

Hexadecimal is linear everywhere and shows what the machine holds: ages and durations as the plain seconds count, sizes as the bit count, a round trip in milliseconds, a length in millimetres, with no scaling and no M:SS. Arabic shows the ledger world's conventional units. Diagnostics record timestamps are wall-clock coordinates for correlating with photonlog and adb, and stay arabic clock time in every base.

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
| +0 | Zila | 21.1 cm | THE UNIT: one hydrogen-line wavelength, light per Eagle oscillation |
| +1 | Zilor | 42.2 cm |  |
| +2 | Ter | 84.4 cm | a bald eagle, nose to tail; a metre |
| +3 | Tera | 1.69 m | a person |
| +4 | Teror | 3.38 m | a car |
| +5 | Lun | 6.75 m |  |
| +6 | Luna | 13.5 m |  |
| +7 | Lunor | 27 m | a blue whale |
| +8 | Stel | 54 m | a football pitch |
| +9 | Stela | 108 m |  |
| +10 | Stelor | 216 m | the Eiffel Tower |
| +11 | ZilaZil | 432 m |  |
| +12 | ZilaZila | 865 m | a kilometre; a mile |
| +13 | ZilaZilor | 1.73 km |  |
| +14 | ZilaTer | 3.46 km |  |
| +15 | ZilaTera | 6.92 km | Everest |
| +16 | ZilaTeror | 13.8 km |  |
| +17 | ZilaLun | 27.7 km | a marathon |
| +18 | ZilaLuna | 55.3 km |  |
| +19 | ZilaLunor | 111 km |  |
| +20 | ZilaStel | 221 km |  |
| +21 | ZilaStela | 443 km |  |
| +22 | ZilaStelor | 885 km |  |
| +23 | ZilorZil | 1.77e+03 km |  |
| +24 | ZilorZila | 3.54e+03 km | Earth's radius |
| +25 | ZilorZilor | 7.08e+03 km |  |
| +26 | ZilorTer | 1.42e+04 km |  |
| +27 | ZilorTera | 2.83e+04 km |  |
| +28 | ZilorTeror | 5.67e+04 km |  |
| +29 | ZilorLun | 1.13e+05 km |  |
| +30 | ZilorLuna | 2.27e+05 km | a light-second; the Moon |
| +31 | ZilorLunor | 4.53e+05 km |  |
| +32 | ZilorStel | 9.06e+05 km | the Sun's diameter |
| +33 | ZilorStela | 1.81e+06 km |  |
| +34 | ZilorStelor | 3.63e+06 km |  |
| +35 | TerZil | 7.25e+06 km |  |
| +36 | TerZila | 1.45e+07 km | a light-minute |
| +37 | TerZilor | 2.9e+07 km |  |
| +38 | TerTer | 5.8e+07 km |  |
| +39 | TerTera | 1.16e+08 km | the Sun (one AU) |
| +40 | TerTeror | 2.32e+08 km |  |
| +41 | TerLun | 4.64e+08 km |  |
| +42 | TerLuna | 9.28e+08 km |  |
| +43 | TerLunor | 1.86e+09 km |  |
| +44 | TerStel | 3.71e+09 km | Pluto |
| +45 | TerStela | 7.43e+09 km |  |
| +46 | TerStelor | 1.49e+10 km | Voyager 1 |
| +47 | TeraZil | 2.97e+10 km |  |
| +48 | TeraZila | 5.94e+10 km |  |
| +49 | TeraZilor | 1.19e+11 km |  |
| +50 | TeraTer | 2.38e+11 km |  |
| +51 | TeraTera | 4.75e+11 km |  |
| +52 | TeraTeror | 0.1 ly |  |
| +53 | TeraLun | 0.201 ly |  |
| +54 | TeraLuna | 0.402 ly |  |
| +55 | TeraLunor | 0.804 ly | a light-year |
| +56 | TeraStel | 1.61 ly |  |
| +57 | TeraStela | 3.21 ly | Proxima Centauri |
| +58 | TeraStelor | 6.43 ly |  |
| +59 | TerorZil | 12.9 ly |  |
| +60 | TerorZila | 25.7 ly |  |
| +61 | TerorZilor | 51.4 ly |  |
| +62 | TerorTer | 103 ly |  |
| +63 | TerorTera | 206 ly |  |
| +64 | TerorTeror | 412 ly |  |
| +65 | TerorLun | 823 ly |  |
| +66 | TerorLuna | 1.65e+03 ly |  |
| +67 | TerorLunor | 3.29e+03 ly |  |
| +68 | TerorStel | 6.58e+03 ly |  |
| +69 | TerorStela | 1.32e+04 ly |  |
| +70 | TerorStelor | 2.63e+04 ly |  |
| +71 | LunZil | 5.27e+04 ly | the Milky Way, across |
| +72 | LunZila | 1.05e+05 ly |  |
| +73 | LunZilor | 2.11e+05 ly |  |
| +74 | LunTer | 4.21e+05 ly |  |
| +75 | LunTera | 8.43e+05 ly |  |
| +76 | LunTeror | 1.69e+06 ly | Andromeda |
| +77 | LunLun | 3.37e+06 ly |  |
| +78 | LunLuna | 6.74e+06 ly |  |
| +79 | LunLunor | 1.35e+07 ly |  |
| +80 | LunStel | 2.7e+07 ly |  |
| +81 | LunStela | 5.39e+07 ly |  |
| +82 | LunStelor | 1.08e+08 ly |  |
| +83 | LunaZil | 2.16e+08 ly |  |
| +84 | LunaZila | 4.32e+08 ly |  |
| +85 | LunaZilor | 8.63e+08 ly |  |
| +86 | LunaTer | 1.73e+09 ly |  |
| +87 | LunaTera | 3.45e+09 ly |  |
| +88 | LunaTeror | 6.9e+09 ly |  |
| +89 | LunaLun | 1.38e+10 ly |  |
| +90 | LunaLuna | 2.76e+10 ly |  |
| +91 | LunaLunor | 5.52e+10 ly | the observable universe, across |
| +92 | LunaStel | 1.1e+11 ly |  |
