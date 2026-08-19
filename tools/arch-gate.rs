//! arch-gate — std-only Rust bitty-executable (rustc, no cargo, no deps) replacing the old python3 metadata parser.
//! Reads `cargo metadata --format-version 1` JSON on stdin; args `<arch_features>` and `<allowlist>`, pipe-separated.
//! Prints `name: feat1, feat2` for each dependency whose RESOLVED feature set contains an architecture feature and
//! is not allowlisted. Silent + exit 0 when clean. Features match as EXACT names, case-insensitive.
//! Being a single std-only file, `rustc` links it with the default `cc` and pulls no build-scripts — so it is
//! immune to cross-env host-linker overrides (the Android NDK clang-14 + mold breakage a cargo bin hit).
use std::collections::{HashMap, HashSet};
use std::io::Read;

/// Just enough JSON: objects and arrays keep structure, strings keep value, scalars are skipped (Other).
enum V {
    S(String),
    A(Vec<V>),
    O(Vec<(String, V)>),
    Other,
}

impl V {
    fn get(&self, k: &str) -> Option<&V> {
        if let V::O(o) = self { o.iter().find(|(key, _)| key == k).map(|(_, v)| v) } else { None }
    }
    fn arr(&self) -> Option<&Vec<V>> {
        if let V::A(a) = self { Some(a) } else { None }
    }
    fn s(&self) -> Option<&str> {
        if let V::S(s) = self { Some(s.as_str()) } else { None }
    }
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> P<'a> {
    fn ws(&mut self) {
        while self.i < self.b.len() {
            match self.b[self.i] {
                b' ' | b'\t' | b'\n' | b'\r' => self.i += 1,
                _ => break,
            }
        }
    }
    fn peek(&self) -> u8 {
        if self.i < self.b.len() { self.b[self.i] } else { 0 }
    }
    fn value(&mut self) -> V {
        self.ws();
        match self.peek() {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => V::S(self.string()),
            b't' | b'n' => { self.i += 4; V::Other } // true / null
            b'f' => { self.i += 5; V::Other }         // false
            _ => { self.number(); V::Other }
        }
    }
    fn number(&mut self) {
        while self.i < self.b.len() {
            match self.b[self.i] {
                b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9' => self.i += 1,
                _ => break,
            }
        }
    }
    fn string(&mut self) -> String {
        self.i += 1; // opening quote
        let mut out: Vec<u8> = Vec::new();
        while self.i < self.b.len() {
            let c = self.b[self.i];
            if c == b'"' {
                self.i += 1;
                break;
            }
            if c == b'\\' {
                self.i += 1;
                match self.peek() {
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'r' => out.push(b'\r'),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'u' => {
                        let h = std::str::from_utf8(&self.b[self.i + 1..(self.i + 5).min(self.b.len())]).unwrap_or("0000");
                        if let Some(ch) = char::from_u32(u32::from_str_radix(h, 16).unwrap_or(0)) {
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        self.i += 4;
                    }
                    other => out.push(other), // " \ / and anything else literal
                }
                self.i += 1;
            } else {
                out.push(c);
                self.i += 1;
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }
    fn array(&mut self) -> V {
        self.i += 1; // [
        let mut v = Vec::new();
        loop {
            self.ws();
            if self.peek() == b']' {
                self.i += 1;
                break;
            }
            v.push(self.value());
            self.ws();
            if self.peek() == b',' {
                self.i += 1;
            } else {
                if self.peek() == b']' {
                    self.i += 1;
                }
                break;
            }
        }
        V::A(v)
    }
    fn object(&mut self) -> V {
        self.i += 1; // {
        let mut o = Vec::new();
        loop {
            self.ws();
            if self.peek() == b'}' {
                self.i += 1;
                break;
            }
            if self.peek() != b'"' {
                break;
            }
            let key = self.string();
            self.ws();
            if self.peek() == b':' {
                self.i += 1;
            }
            let val = self.value();
            o.push((key, val));
            self.ws();
            if self.peek() == b',' {
                self.i += 1;
            } else {
                if self.peek() == b'}' {
                    self.i += 1;
                }
                break;
            }
        }
        V::O(o)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arch: HashSet<String> =
        args.get(1).map(|s| s.split('|').map(|x| x.to_lowercase()).collect()).unwrap_or_default();
    let allow: HashSet<String> = args
        .get(2)
        .map(|s| s.split('|').filter(|x| !x.is_empty()).map(String::from).collect())
        .unwrap_or_default();

    let mut input = Vec::new();
    if std::io::stdin().read_to_end(&mut input).is_err() || input.is_empty() {
        eprintln!("arch-gate: no metadata on stdin");
        std::process::exit(2);
    }
    let root = P { b: &input, i: 0 }.value();

    let mut names: HashMap<String, String> = HashMap::new();
    if let Some(pkgs) = root.get("packages").and_then(V::arr) {
        for pkg in pkgs {
            if let (Some(id), Some(n)) = (pkg.get("id").and_then(V::s), pkg.get("name").and_then(V::s)) {
                names.insert(id.to_string(), n.to_string());
            }
        }
    }
    if let Some(nodes) = root.get("resolve").and_then(|v| v.get("nodes")).and_then(V::arr) {
        for node in nodes {
            let id = node.get("id").and_then(V::s).unwrap_or("");
            let name = names.get(id).map(String::as_str).unwrap_or("?");
            if allow.contains(name) {
                continue;
            }
            let mut hits: Vec<&str> = node
                .get("features")
                .and_then(V::arr)
                .map(|fs| fs.iter().filter_map(V::s).filter(|f| arch.contains(&f.to_lowercase())).collect())
                .unwrap_or_default();
            if !hits.is_empty() {
                hits.sort_unstable();
                println!("{name}: {}", hits.join(", "));
            }
        }
    }
}
