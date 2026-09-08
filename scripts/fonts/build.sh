#!/usr/bin/env bash
# Regenerate the trimmed static fonts in assets/Noto from their full sources in assets/Noto/sources — each cut to the Unicode blocks it is the designated provider for (see tools/font-trim.rs for what "cut" means and why glyph ids never move). The variable script fonts ship whole: their bulk is shaping tables, not spare Latin, and a variable font is outside the trimmer's remit by design.
# Symbol faces never shape or kern, so their GSUB/GPOS/GDEF (and Math's MATH layout table) go, which is exactly what makes --compact's dense renumber safe — see the tool.
# One home per codepoint: fluor's Symbols 2 owns everything it draws (the FE0E pins depend on it), so the other monochrome faces are cut MINUS its coverage.
set -euo pipefail
cd "$(dirname "$0")/../.."
tool=tools/font-trim
if [ ! -x "$tool" ] || [ tools/font-trim.rs -nt "$tool" ]; then
    rustc -O tools/font-trim.rs -o "$tool"
fi
S2=../fluor/assets/Noto_Sans_Symbols2/NotoSansSymbols2-Regular.ttf
# Mono: box drawing, block elements, the currency block — the only bundled face that carries all of them (measured from the cmaps, not the blurbs).
"$tool" assets/Noto/sources/NotoSansMono-Regular.ttf assets/Noto/NotoSansMono-Regular.ttf --minus "$S2" --drop GSUB --drop GPOS --drop GDEF --compact 2500-257F 2580-259F 20A0-20CF
# Math: arrows, operators, the misc/supplemental math blocks, the alphanumerics, arabic mathematical symbols — minus Symbols 2's own arrows.
"$tool" assets/Noto/sources/NotoSansMath-Regular.ttf assets/Noto/NotoSansMath-Regular.ttf --minus "$S2" --drop GSUB --drop GPOS --drop GDEF --drop MATH --compact 2190-21FF 2200-22FF 27C0-27EF 27F0-27FF 2900-297F 2980-29FF 2A00-2AFF 2B00-2BFF 1D400-1D7FF 1EE00-1EEFF
echo "completed $(date)"
