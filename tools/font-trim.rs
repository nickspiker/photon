//! font-trim — cut a STATIC TrueType font down to the Unicode ranges it is the designated provider for, without renumbering a single glyph.
//!
//! Usage: font-trim <in.ttf> <out.ttf> [--minus <other.ttf>]... [--drop <TAG>]... [--compact] <RANGE>...   (RANGE = hex `2500-257F` or a single `20AC`)
//!
//! Why glyph IDs stay put: a real subsetter (hb-subset) renumbers glyphs and rewrites GSUB/GPOS/GDEF to match — thousands of lines, and one remap bug silently breaks shaping in the shipped app. This tool never touches a layout table. It rewrites exactly three things: `cmap` (only the kept codepoints), `glyf`/`loca` (outlines of unreachable glyphs emptied; kept outlines byte-identical), and `head.indexToLocFormat`. Everything else is copied verbatim, so GSUB/GPOS/GDEF/hmtx/name remain valid by construction — the family name the fallback chain routes on survives untouched.
//! Reachable = mapped by a kept codepoint, or a component of a kept composite, or the OUTPUT of any GSUB substitution (kept conservatively without computing input reachability — a few dozen glyphs of slack beats an emptied ligature).
//! `--minus` subtracts another font's cmap first: the one-home rule (fluor's Symbols 2 owns everything it draws; this font only fills the holes).
//! `--drop TAG` removes a table outright (GSUB/GPOS/GDEF/MATH for symbol faces that never shape or kern). `--compact` then renumbers the kept glyphs densely — and REFUSES unless every table that could still hold a glyph id is gone, so the renumber can only ever touch what this tool rewrites itself: cmap, glyf composites, hmtx/hhea, loca, maxp, and post (rewritten as format 3, names dropped — a rasterizer never reads them).
//! Statics only: a font carrying fvar/gvar/CFF is refused rather than mangled.
//! std-only, rustc-compiled, no cargo (see scripts/lib/arch-gate.sh for why tools are built this way).

use std::collections::{BTreeMap, BTreeSet};
use std::{env, fs, process};

fn be16(d: &[u8], o: usize) -> Result<u16, String> {
    d.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]])).ok_or_else(|| format!("read u16 past end at {o}"))
}
fn be32(d: &[u8], o: usize) -> Result<u32, String> {
    d.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(|| format!("read u32 past end at {o}"))
}
fn tag(d: &[u8], o: usize) -> Result<[u8; 4], String> {
    d.get(o..o + 4).map(|b| [b[0], b[1], b[2], b[3]]).ok_or_else(|| format!("read tag past end at {o}"))
}

/// Table directory: tag → (offset, length).
fn tables(d: &[u8]) -> Result<BTreeMap<[u8; 4], (usize, usize)>, String> {
    let n = be16(d, 4)? as usize;
    let mut t = BTreeMap::new();
    for i in 0..n {
        let e = 12 + 16 * i;
        t.insert(tag(d, e)?, (be32(d, e + 8)? as usize, be32(d, e + 12)? as usize));
    }
    Ok(t)
}

/// Unicode cmap: codepoint → glyph id, from the best (3,10) format-12 or (3,1)/(0,x) format-4 subtable.
fn cmap(d: &[u8], t: &BTreeMap<[u8; 4], (usize, usize)>) -> Result<BTreeMap<u32, u16>, String> {
    let (co, _) = *t.get(b"cmap").ok_or("no cmap")?;
    let n = be16(d, co + 2)? as usize;
    let mut best: Option<(u8, usize)> = None;
    for i in 0..n {
        let e = co + 4 + 8 * i;
        let (pid, eid, off) = (be16(d, e)?, be16(d, e + 2)?, be32(d, e + 4)? as usize);
        let sub = co + off;
        let fmt = be16(d, sub)?;
        let rank = match (pid, eid, fmt) {
            (3, 10, 12) => 4,
            (0, _, 12) => 3,
            (3, 1, 4) => 2,
            (0, _, 4) => 1,
            _ => 0,
        };
        if rank > 0 && best.map_or(true, |(r, _)| rank > r) {
            best = Some((rank, sub));
        }
    }
    let (_, sub) = best.ok_or("no usable Unicode cmap subtable")?;
    let mut m = BTreeMap::new();
    match be16(d, sub)? {
        12 => {
            let groups = be32(d, sub + 12)? as usize;
            for g in 0..groups {
                let e = sub + 16 + 12 * g;
                let (s, en, gid) = (be32(d, e)?, be32(d, e + 4)?, be32(d, e + 8)?);
                for (k, cp) in (s..=en).enumerate() {
                    let gid = gid as usize + k;
                    if gid != 0 && gid <= u16::MAX as usize {
                        m.insert(cp, gid as u16);
                    }
                }
            }
        }
        4 => {
            let seg = be16(d, sub + 6)? as usize / 2;
            let ends = sub + 14;
            let starts = ends + seg * 2 + 2;
            let deltas = starts + seg * 2;
            let ranges = deltas + seg * 2;
            for i in 0..seg {
                let (en, s) = (be16(d, ends + 2 * i)? as u32, be16(d, starts + 2 * i)? as u32);
                let delta = be16(d, deltas + 2 * i)?;
                let ro = be16(d, ranges + 2 * i)? as usize;
                if s == 0xFFFF {
                    continue;
                }
                for cp in s..=en {
                    let gid = if ro == 0 {
                        (cp as u16).wrapping_add(delta)
                    } else {
                        let a = ranges + 2 * i + ro + 2 * (cp - s) as usize;
                        let g = be16(d, a)?;
                        if g == 0 { 0 } else { g.wrapping_add(delta) }
                    };
                    if gid != 0 {
                        m.insert(cp, gid);
                    }
                }
            }
        }
        f => return Err(format!("unsupported cmap format {f}")),
    }
    Ok(m)
}

/// Coverage table → glyph ids.
fn coverage(d: &[u8], o: usize, out: &mut BTreeSet<u16>) -> Result<(), String> {
    match be16(d, o)? {
        1 => {
            let n = be16(d, o + 2)? as usize;
            for i in 0..n {
                out.insert(be16(d, o + 4 + 2 * i)?);
            }
        }
        2 => {
            let n = be16(d, o + 2)? as usize;
            for i in 0..n {
                let e = o + 4 + 6 * i;
                let (s, en) = (be16(d, e)?, be16(d, e + 2)?);
                for g in s..=en {
                    out.insert(g);
                }
            }
        }
        f => return Err(format!("coverage format {f}")),
    }
    Ok(())
}

/// Every glyph any GSUB lookup can PRODUCE. Conservative on purpose — no input-reachability closure.
fn gsub_outputs(d: &[u8], t: &BTreeMap<[u8; 4], (usize, usize)>) -> Result<BTreeSet<u16>, String> {
    let mut out = BTreeSet::new();
    let Some(&(go, _)) = t.get(b"GSUB") else { return Ok(out) };
    let ll = go + be16(d, go + 8)? as usize;
    let lookups = be16(d, ll)? as usize;
    for i in 0..lookups {
        let lo = ll + be16(d, ll + 2 + 2 * i)? as usize;
        let ty = be16(d, lo)?;
        let subs = be16(d, lo + 4)? as usize;
        for s in 0..subs {
            let mut so = lo + be16(d, lo + 6 + 2 * s)? as usize;
            let mut ty = ty;
            if ty == 7 {
                // Extension: real type + 32-bit offset to the wrapped subtable.
                ty = be16(d, so + 2)?;
                so += be32(d, so + 4)? as usize;
            }
            match ty {
                1 => {
                    let fmt = be16(d, so)?;
                    let mut cov = BTreeSet::new();
                    coverage(d, so + be16(d, so + 2)? as usize, &mut cov)?;
                    if fmt == 1 {
                        let delta = be16(d, so + 4)?;
                        for g in cov {
                            out.insert(g.wrapping_add(delta));
                        }
                    } else {
                        let n = be16(d, so + 4)? as usize;
                        for k in 0..n {
                            out.insert(be16(d, so + 6 + 2 * k)?);
                        }
                    }
                }
                2 | 3 => {
                    // Multiple / Alternate: count of sets, each a glyph list.
                    let n = be16(d, so + 4)? as usize;
                    for k in 0..n {
                        let set = so + be16(d, so + 6 + 2 * k)? as usize;
                        let m = be16(d, set)? as usize;
                        for j in 0..m {
                            out.insert(be16(d, set + 2 + 2 * j)?);
                        }
                    }
                }
                4 => {
                    let n = be16(d, so + 4)? as usize;
                    for k in 0..n {
                        let set = so + be16(d, so + 6 + 2 * k)? as usize;
                        let m = be16(d, set)? as usize;
                        for j in 0..m {
                            let lig = set + be16(d, set + 2 + 2 * j)? as usize;
                            out.insert(be16(d, lig)?);
                        }
                    }
                }
                8 => {
                    let bt = be16(d, so + 4)? as usize;
                    let la_o = so + 6 + 2 * bt;
                    let la = be16(d, la_o)? as usize;
                    let gc_o = la_o + 2 + 2 * la;
                    let gc = be16(d, gc_o)? as usize;
                    for k in 0..gc {
                        out.insert(be16(d, gc_o + 2 + 2 * k)?);
                    }
                }
                _ => {} // 5/6 contextual: they only invoke lookups already walked above.
            }
        }
    }
    Ok(out)
}

fn checksum(b: &[u8]) -> u32 {
    let mut s: u32 = 0;
    for c in b.chunks(4) {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        s = s.wrapping_add(u32::from_be_bytes(w));
    }
    s
}

/// Format 12 (all) + format 4 (BMP) cmap for `map`, under (3,10)/(3,1)/(0,4)/(0,3).
fn build_cmap(map: &BTreeMap<u32, u16>) -> Vec<u8> {
    // Contiguous (codepoint, glyph) runs → format-12 groups.
    let mut groups: Vec<(u32, u32, u32)> = Vec::new();
    for (&cp, &g) in map {
        if let Some(last) = groups.last_mut() {
            if last.1 + 1 == cp && (last.2 + (last.1 - last.0) + 1) == g as u32 {
                last.1 = cp;
                continue;
            }
        }
        groups.push((cp, cp, g as u32));
    }
    let mut f12 = Vec::new();
    f12.extend_from_slice(&12u16.to_be_bytes());
    f12.extend_from_slice(&0u16.to_be_bytes());
    f12.extend_from_slice(&((16 + 12 * groups.len()) as u32).to_be_bytes());
    f12.extend_from_slice(&0u32.to_be_bytes());
    f12.extend_from_slice(&(groups.len() as u32).to_be_bytes());
    for (s, e, g) in &groups {
        f12.extend_from_slice(&s.to_be_bytes());
        f12.extend_from_slice(&e.to_be_bytes());
        f12.extend_from_slice(&g.to_be_bytes());
    }
    // Format 4: one segment per contiguous BMP codepoint run, glyphs listed explicitly (idDelta 0, idRangeOffset into glyphIdArray).
    let mut segs: Vec<(u16, u16, Vec<u16>)> = Vec::new();
    for (&cp, &g) in map.range(..0x10000u32) {
        let cp = cp as u16;
        if let Some(last) = segs.last_mut() {
            if last.1.wrapping_add(1) == cp && cp != 0xFFFF {
                last.1 = cp;
                last.2.push(g);
                continue;
            }
        }
        segs.push((cp, cp, vec![g]));
    }
    segs.push((0xFFFF, 0xFFFF, vec![]));
    let n = segs.len();
    let mut ends = Vec::new();
    let mut starts = Vec::new();
    let mut deltas = Vec::new();
    let mut ros = Vec::new();
    let mut gids: Vec<u16> = Vec::new();
    for (i, (s, e, glyphs)) in segs.iter().enumerate() {
        ends.push(*e);
        starts.push(*s);
        if glyphs.is_empty() {
            deltas.push(1u16);
            ros.push(0u16);
        } else {
            deltas.push(0);
            // offset from this idRangeOffset slot to glyphIdArray[len]: (n - i) slots to the array start, plus current fill.
            ros.push(((n - i) * 2 + gids.len() * 2) as u16);
            gids.extend_from_slice(glyphs);
        }
    }
    let len4 = 16 + n * 8 + gids.len() * 2;
    let mut f4 = Vec::new();
    f4.extend_from_slice(&4u16.to_be_bytes());
    f4.extend_from_slice(&(len4 as u16).to_be_bytes());
    f4.extend_from_slice(&0u16.to_be_bytes());
    f4.extend_from_slice(&((n * 2) as u16).to_be_bytes());
    let mut es = 0u16;
    let mut p2 = 1usize;
    while p2 * 2 <= n {
        p2 *= 2;
        es += 1;
    }
    f4.extend_from_slice(&((p2 * 2) as u16).to_be_bytes());
    f4.extend_from_slice(&es.to_be_bytes());
    f4.extend_from_slice(&((n * 2 - p2 * 2) as u16).to_be_bytes());
    for v in &ends { f4.extend_from_slice(&v.to_be_bytes()); }
    f4.extend_from_slice(&0u16.to_be_bytes());
    for v in &starts { f4.extend_from_slice(&v.to_be_bytes()); }
    for v in &deltas { f4.extend_from_slice(&v.to_be_bytes()); }
    for v in &ros { f4.extend_from_slice(&v.to_be_bytes()); }
    for v in &gids { f4.extend_from_slice(&v.to_be_bytes()); }
    // Header: four encoding records, the two subtables shared.
    let recs: [(u16, u16, bool); 4] = [(0, 3, false), (0, 4, true), (3, 1, false), (3, 10, true)];
    let head_len = 4 + 8 * recs.len();
    let o4 = head_len;
    let o12 = o4 + f4.len();
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(recs.len() as u16).to_be_bytes());
    for (p, e, is12) in recs {
        out.extend_from_slice(&p.to_be_bytes());
        out.extend_from_slice(&e.to_be_bytes());
        out.extend_from_slice(&((if is12 { o12 } else { o4 }) as u32).to_be_bytes());
    }
    out.extend_from_slice(&f4);
    out.extend_from_slice(&f12);
    out
}

fn parse_range(s: &str) -> Result<(u32, u32), String> {
    let (a, b) = s.split_once('-').unwrap_or((s, s));
    let p = |x: &str| u32::from_str_radix(x.trim_start_matches("U+"), 16).map_err(|e| format!("bad range {s}: {e}"));
    Ok((p(a)?, p(b)?))
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() < 3 {
        return Err("usage: font-trim <in.ttf> <out.ttf> [--minus <other.ttf>]... <RANGE>...".into());
    }
    let (src, dst) = (&args[0], &args[1]);
    let mut minus: Vec<String> = Vec::new();
    let mut drops: BTreeSet<[u8; 4]> = BTreeSet::new();
    let mut compact = false;
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    let mut i = 2;
    while i < args.len() {
        if args[i] == "--minus" {
            minus.push(args.get(i + 1).ok_or("--minus needs a path")?.clone());
            i += 2;
        } else if args[i] == "--drop" {
            let t = args.get(i + 1).ok_or("--drop needs a tag")?;
            let b = t.as_bytes();
            if b.len() != 4 {
                return Err(format!("--drop {t}: a tag is exactly four bytes"));
            }
            drops.insert([b[0], b[1], b[2], b[3]]);
            i += 2;
        } else if args[i] == "--compact" {
            compact = true;
            i += 1;
        } else {
            ranges.push(parse_range(&args[i])?);
            i += 1;
        }
    }
    let d = fs::read(src).map_err(|e| format!("{src}: {e}"))?;
    let t = tables(&d)?;
    for forbidden in [b"CFF ", b"fvar", b"gvar"] {
        if t.contains_key(forbidden) {
            return Err(format!("{src}: has {} — statics only, this tool does not instance or subset variable/CFF fonts", String::from_utf8_lossy(forbidden)));
        }
    }
    let full = cmap(&d, &t)?;
    let mut drop: BTreeSet<u32> = BTreeSet::new();
    for m in &minus {
        let md = fs::read(m).map_err(|e| format!("{m}: {e}"))?;
        drop.extend(cmap(&md, &tables(&md)?)?.keys());
    }
    // Invisible helpers every font may keep: ZWNJ/ZWJ, variation selectors, NBSP.
    let always = [0x200C, 0x200D, 0xFE0E, 0xFE0F, 0x00A0];
    let mut keep_map: BTreeMap<u32, u16> = BTreeMap::new();
    for (&cp, &g) in &full {
        let wanted = always.contains(&cp) || ranges.iter().any(|&(a, b)| cp >= a && cp <= b);
        if wanted && !drop.contains(&cp) {
            keep_map.insert(cp, g);
        }
    }
    // Reachable glyphs: mapped, composite components (transitively), any GSUB output.
    let (glo, _) = *t.get(b"glyf").ok_or("no glyf")?;
    let (lo, _) = *t.get(b"loca").ok_or("no loca")?;
    let (ho, _) = *t.get(b"head").ok_or("no head")?;
    let (mo, _) = *t.get(b"maxp").ok_or("no maxp")?;
    let num = be16(&d, mo + 4)? as usize;
    let long = be16(&d, ho + 50)? == 1;
    let loca = |g: usize| -> Result<usize, String> { if long { Ok(be32(&d, lo + 4 * g)? as usize) } else { Ok(be16(&d, lo + 2 * g)? as usize * 2) } };
    let mut keep: BTreeSet<u16> = keep_map.values().copied().collect();
    keep.insert(0);
    if !drops.contains(b"GSUB") {
        keep.extend(gsub_outputs(&d, &t)?);
    }
    if compact {
        // The renumber is only safe when no surviving table can name a glyph id except the ones rewritten below.
        let allowed: [&[u8; 4]; 13] = [b"cmap", b"glyf", b"loca", b"hmtx", b"hhea", b"maxp", b"post", b"head", b"OS/2", b"name", b"cvt ", b"fpgm", b"prep"];
        for tg in t.keys() {
            if !drops.contains(tg) && !allowed.contains(&tg) && tg != b"gasp" && tg != b"DSIG" {
                return Err(format!("--compact refused: table {} survives and may hold glyph ids — drop it or skip --compact", String::from_utf8_lossy(tg)));
            }
        }
    }
    let mut stack: Vec<u16> = keep.iter().copied().collect();
    while let Some(g) = stack.pop() {
        let (s, e) = (loca(g as usize)?, loca(g as usize + 1)?);
        if e <= s || (be16(&d, glo + s)? as i16) >= 0 {
            continue;
        }
        let mut o = glo + s + 10;
        loop {
            let flags = be16(&d, o)?;
            let comp = be16(&d, o + 2)?;
            if keep.insert(comp) {
                stack.push(comp);
            }
            o += 4 + if flags & 0x0001 != 0 { 4 } else { 2 };
            o += if flags & 0x0008 != 0 { 2 } else if flags & 0x0040 != 0 { 4 } else if flags & 0x0080 != 0 { 8 } else { 0 };
            if flags & 0x0020 == 0 {
                break;
            }
        }
    }
    // Glyph id map: identity (stable ids, unreachable outlines emptied) or dense (compact).
    let newid: BTreeMap<u16, u16> = if compact {
        keep.iter().enumerate().map(|(n, &g)| (g, n as u16)).collect()
    } else {
        (0..num as u16).map(|g| (g, g)).collect()
    };
    let out_num = if compact { keep.len() } else { num };
    // New glyf/loca: kept outlines verbatim (4-aligned) — composite component ids rewritten under compact — the rest empty.
    let mut glyf = Vec::new();
    let mut nloca = Vec::with_capacity(4 * (out_num + 1));
    let order: Vec<u16> = if compact { keep.iter().copied().collect() } else { (0..num as u16).collect() };
    for &g in &order {
        nloca.extend_from_slice(&(glyf.len() as u32).to_be_bytes());
        if !keep.contains(&g) {
            continue;
        }
        let (s, e) = (loca(g as usize)?, loca(g as usize + 1)?);
        let mut body = d.get(glo + s..glo + e).ok_or("glyph past end")?.to_vec();
        if compact && e > s && (be16(&body, 0)? as i16) < 0 {
            let mut o = 10;
            loop {
                let flags = be16(&body, o)?;
                let comp = be16(&body, o + 2)?;
                let n = *newid.get(&comp).ok_or("component not in keep set")?;
                body[o + 2..o + 4].copy_from_slice(&n.to_be_bytes());
                o += 4 + if flags & 0x0001 != 0 { 4 } else { 2 };
                o += if flags & 0x0008 != 0 { 2 } else if flags & 0x0040 != 0 { 4 } else if flags & 0x0080 != 0 { 8 } else { 0 };
                if flags & 0x0020 == 0 {
                    break;
                }
            }
        }
        glyf.extend_from_slice(&body);
        while glyf.len() % 4 != 0 {
            glyf.push(0);
        }
    }
    nloca.extend_from_slice(&(glyf.len() as u32).to_be_bytes());
    let mut head = d[ho..ho + t[b"head"].1].to_vec();
    head[50..52].copy_from_slice(&1u16.to_be_bytes());
    head[8..12].copy_from_slice(&0u32.to_be_bytes());
    // hmtx/hhea/maxp/post under compact: full (advance, lsb) per kept glyph, counts updated, glyph names dropped.
    let (hho, hhl) = *t.get(b"hhea").ok_or("no hhea")?;
    let (hmo, _) = *t.get(b"hmtx").ok_or("no hmtx")?;
    let nh = be16(&d, hho + 34)? as usize;
    let mut hhea = d[hho..hho + hhl].to_vec();
    let mut hmtx = Vec::new();
    let mut maxp = d[mo..mo + t[b"maxp"].1].to_vec();
    let mut post = d[t[b"post"].0..t[b"post"].0 + t[b"post"].1].to_vec();
    if compact {
        for &g in &order {
            let g = g as usize;
            let adv = be16(&d, hmo + 4 * g.min(nh - 1))?;
            let lsb = if g < nh { be16(&d, hmo + 4 * g + 2)? } else { be16(&d, hmo + 4 * nh + 2 * (g - nh))? };
            hmtx.extend_from_slice(&adv.to_be_bytes());
            hmtx.extend_from_slice(&lsb.to_be_bytes());
        }
        hhea[34..36].copy_from_slice(&(out_num as u16).to_be_bytes());
        maxp[4..6].copy_from_slice(&(out_num as u16).to_be_bytes());
        post.truncate(32);
        post[0..4].copy_from_slice(&0x00030000u32.to_be_bytes());
    }
    // The cmap must name glyphs by their NEW ids under compact — the one place the renumber reaches outside glyf. (Caught by tests/glyph_fallback_probe.rs: with old ids every mapped glyph was out of range and laid out as nothing.)
    let keep_map: BTreeMap<u32, u16> = keep_map.iter().map(|(&cp, g)| (cp, newid[g])).collect();
    let new_cmap = build_cmap(&keep_map);
    // Reassemble: same table set, sorted by tag, 4-aligned, checksums recomputed.
    let mut out_tables: BTreeMap<[u8; 4], Vec<u8>> = BTreeMap::new();
    for (&tg, &(o, l)) in &t {
        if drops.contains(&tg) {
            continue;
        }
        let body = match &tg {
            b"cmap" => new_cmap.clone(),
            b"glyf" => glyf.clone(),
            b"loca" => nloca.clone(),
            b"head" => head.clone(),
            b"hhea" => hhea.clone(),
            b"maxp" => maxp.clone(),
            b"post" => post.clone(),
            b"hmtx" if compact => hmtx.clone(),
            b"DSIG" => continue,
            _ => d.get(o..o + l).ok_or("table past end")?.to_vec(),
        };
        out_tables.insert(tg, body);
    }
    let n = out_tables.len();
    let mut p2 = 1usize;
    let mut es = 0u16;
    while p2 * 2 <= n { p2 *= 2; es += 1; }
    let mut out = Vec::new();
    out.extend_from_slice(&d[0..4]);
    out.extend_from_slice(&(n as u16).to_be_bytes());
    out.extend_from_slice(&((p2 * 16) as u16).to_be_bytes());
    out.extend_from_slice(&es.to_be_bytes());
    out.extend_from_slice(&(((n - p2) * 16) as u16).to_be_bytes());
    let mut offset = 12 + 16 * n;
    let mut dir = Vec::new();
    let mut bodies = Vec::new();
    for (tg, body) in &out_tables {
        dir.extend_from_slice(tg);
        dir.extend_from_slice(&checksum(body).to_be_bytes());
        dir.extend_from_slice(&(offset as u32).to_be_bytes());
        dir.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let mut b = body.clone();
        while b.len() % 4 != 0 { b.push(0); }
        offset += b.len();
        bodies.push(b);
    }
    out.extend_from_slice(&dir);
    let head_at = 12 + 16 * n + out_tables.range(..*b"head").map(|(_, b)| (b.len() + 3) & !3).sum::<usize>();
    for b in bodies { out.extend_from_slice(&b); }
    let adj = 0xB1B0AFBAu32.wrapping_sub(checksum(&out));
    out[head_at + 8..head_at + 12].copy_from_slice(&adj.to_be_bytes());
    fs::write(dst, &out).map_err(|e| format!("{dst}: {e}"))?;
    println!("{}: {} → {} codepoints, {} → {} glyphs{}, {} → {} KB", dst, full.len(), keep_map.len(), num, keep.len(), if compact { " (renumbered)" } else { " (ids stable, rest emptied)" }, d.len() / 1024, out.len() / 1024);
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("font-trim: {e}");
        process::exit(1);
    }
}
