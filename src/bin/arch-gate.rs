//! arch-gate — Rust replacement for the old `python3 -c` in scripts/lib/arch-gate.sh (no Python in the build path).
//!
//! Reads `cargo metadata --format-version 1` JSON on stdin. Args: `<arch_features>` and `<allowlist>`, both pipe-separated. Prints one `name: feat1, feat2` line per dependency whose RESOLVED feature set contains an architecture feature and is not allowlisted. Silent (and exit 0) when clean; the shell treats any output as the failure. The features are matched as EXACT names, case-insensitive — same as the old `^(a|b|..)$` regex.
use std::collections::{HashMap, HashSet};
use std::io::Read;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arch: HashSet<String> =
        args.get(1).map(|s| s.split('|').map(|x| x.to_lowercase()).collect()).unwrap_or_default();
    let allow: HashSet<String> = args
        .get(2)
        .map(|s| s.split('|').filter(|x| !x.is_empty()).map(String::from).collect())
        .unwrap_or_default();

    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() || input.trim().is_empty() {
        eprintln!("arch-gate: no metadata on stdin");
        std::process::exit(2);
    }
    let data: serde_json::Value = match serde_json::from_str(&input) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("arch-gate: bad metadata JSON: {e}");
            std::process::exit(2);
        }
    };

    // id → package name.
    let mut names: HashMap<&str, &str> = HashMap::new();
    if let Some(pkgs) = data["packages"].as_array() {
        for p in pkgs {
            if let (Some(id), Some(n)) = (p["id"].as_str(), p["name"].as_str()) {
                names.insert(id, n);
            }
        }
    }

    // Each resolve node carries its RESOLVED feature set; flag arch features on non-allowlisted packages.
    if let Some(nodes) = data["resolve"]["nodes"].as_array() {
        for node in nodes {
            let id = node["id"].as_str().unwrap_or("");
            let name = *names.get(id).unwrap_or(&"?");
            if allow.contains(name) {
                continue;
            }
            let mut hits: Vec<&str> = node["features"]
                .as_array()
                .map(|fs| {
                    fs.iter()
                        .filter_map(|f| f.as_str())
                        .filter(|f| arch.contains(&f.to_lowercase()))
                        .collect()
                })
                .unwrap_or_default();
            if !hits.is_empty() {
                hits.sort_unstable();
                println!("{name}: {}", hits.join(", "));
            }
        }
    }
}
