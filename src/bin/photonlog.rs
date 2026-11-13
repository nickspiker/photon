//! photonlog — decode a `photon.log.vsf` structured log into human-readable lines.
//!
//! The log is a stream of complete VSF records, each `{creation_time (Eagle), section "log" {lvl, msg}}` (see `crate::log` in lib.rs).
//! This walks the records in order and prints `<eagle-time>  [LEVEL]  <msg>`.
//!
//! Pull the file off a phone with: `adb pull /data/user/0/com.photon.messenger/files/photon.log.vsf`
//!
//! Usage: `photonlog [PATH] [flags]` (PATH defaults to ./photon.log.vsf)
//!   -l, --level LEVEL   only records at this severity or higher (TRACE|DEBUG|INFO|WARN|ERROR, or 0..4)
//!   -g, --grep SUBSTR   only records whose message contains SUBSTR (case-insensitive)
//!   -f, --follow        keep reading as new records are appended (tail -f); survives the 16 MiB rotation
//!   -p, --pull          fetch a person's SUBMITTED logs from FGTW and decode them. They are filed under the person's handle proof (typing the handle finds them) and sealed to the maintainer's log key (keys/photon-log-key.vsf, crypto/log_seal.rs), which opens them; the release key then clears the filed copies.
//!   -H, --handle NAME   the person's handle: computes their handle proof (~1 s).
//!   -P, --petname NAME  resolve NAME thru the dev-machine pseudonym map ($FERROS_HANDLE_MAP, lines of `handle = petname`) and pull that identity's logs — the handle never touches a command line, a screen, or a transcript. THE way to pull a person's logs on a dev machine; --handle stays for machines without the map.
//!   -S, --session       this machine's own identity, from the live tohu session registers. No handle typed.
//!   -M, --mailbox       list every filed log, whose and when, newest last (signed by the release key); names come from the pseudonym map when it is present.
//!   -s, --seed HEX64    LEGACY: an identity seed, for logs a pre-M6 Photon sealed under its seed.
//!   -k, --key  HEX64    LEGACY: a pre-M6 raw log key, to open a local sealed blob.
//!
//! Examples: `photonlog -l warn` · `photonlog --pull --petname kea -l warn` · `photonlog --pull --session` · `photonlog --mailbox`.

use std::io::Read;

fn level_name(lvl: u64) -> &'static str {
    match lvl {
        0 => "TRACE",
        1 => "DEBUG",
        2 => "INFO ",
        3 => "WARN ",
        4 => "ERROR",
        _ => "?????",
    }
}

/// Parse a `--level` argument: a name (any case) or a bare number 0..=4.
fn level_from_arg(s: &str) -> Option<u64> {
    match s.to_ascii_uppercase().as_str() {
        "TRACE" => Some(0),
        "DEBUG" => Some(1),
        "INFO" => Some(2),
        "WARN" | "WARNING" => Some(3),
        "ERROR" => Some(4),
        _ => s.parse::<u64>().ok().filter(|n| *n <= 4),
    }
}

/// Eagle oscillations → a readable UTC string for display (display-only conversion is allowed).
fn eagle_display(osc: i64) -> String {
    vsf::types::EagleTime::from_oscillations(osc)
        .to_datetime()
        .format("%Y-%m-%d %H:%M:%S%.3f")
        .to_string()
}

struct Filter {
    min_level: u64,
    grep: Option<String>,
}

/// Decode and print whole records from `buf` (via the shared [`photon_messenger::parse_log_records`] — the in-app viewer walks the same decode), applying the filter.
/// Returns the number of bytes consumed — i.e. the offset of the last COMPLETE record boundary — so a half-written trailing record (mid-append) is left for the next pass instead of being mis-decoded.
fn print_records(buf: &[u8], filter: &Filter) -> usize {
    let (records, consumed) = photon_messenger::parse_log_records(buf);
    for r in &records {
        let pass_level = r.level >= filter.min_level;
        let pass_grep = match &filter.grep {
            Some(g) => r.msg.to_lowercase().contains(g),
            None => true,
        };
        if pass_level && pass_grep {
            let ts = if r.osc != 0 {
                eagle_display(r.osc)
            } else {
                "(no time)".to_string()
            };
            println!("{ts}  [{}]  {}", level_name(r.level), r.msg);
        }
    }
    consumed
}

fn read_all(path: &str) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Resolve a petname to its handle thru the dev-machine pseudonym map: lines of `handle = petname`, handle LEFT (the secret), petname RIGHT (the safe prose alias) — the same file the handle-guard reads, so the two tools can never disagree on which column is which.
/// Match = case-insensitive on the petname's first word (the right column may carry a parenthetical note); ambiguity is an error listing the petnames (safe to print), never a guess — a fuzzy match pulling the WRONG person's logs would be worse than no match.
fn petname_to_handle(petname: &str) -> Result<String, String> {
    let map_path = std::env::var("FERROS_HANDLE_MAP")
        .unwrap_or_else(|_| "/mnt/Harbor/Code/keys/claude-pseudonym-map.txt".to_string());
    let map = std::fs::read_to_string(&map_path)
        .map_err(|e| format!("cannot read pseudonym map {map_path}: {e} (set FERROS_HANDLE_MAP)"))?;
    let want = petname.to_lowercase();
    let mut hits: Vec<(String, String)> = Vec::new();
    for line in map.lines() {
        let line = line.trim_end();
        if line.trim_start().starts_with('#') {
            continue;
        }
        let Some((handle, pet)) = line.split_once(" = ") else {
            continue;
        };
        let pet_word = pet.split_whitespace().next().unwrap_or("");
        if pet.to_lowercase() == want || pet_word.to_lowercase() == want {
            hits.push((handle.to_string(), pet.to_string()));
        }
    }
    match hits.len() {
        0 => Err(format!("no map entry with petname '{petname}'")),
        1 => Ok(hits.remove(0).0),
        _ => Err(format!(
            "petname '{petname}' is ambiguous in the map: {}",
            hits.iter().map(|(_, p)| p.as_str()).collect::<Vec<_>>().join(" | ")
        )),
    }
}

/// Parse a 64-char hex string into a 32-byte array (identity seed or raw log key).
fn parse_hex32(s: &str) -> Option<[u8; 32]> {
    let v = (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    v.try_into().ok()
}

// LEGACY LOG PULL — MIGRATION-EXPIRES: 2027-02-01. A Photon from before M6 (ferros docs/PHOTON.md) sealed its log under a key derived from the identity seed and filed it under a tag from the same seed, so anyone who knew the handle could open it; the reader stays only until those Photons have updated. Grep the tag to find every piece.
fn legacy_log_key(identity_seed: &[u8; 32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(identity_seed);
    hasher.update(b"photon.log.v0");
    *hasher.finalize().as_bytes()
}

// LEGACY LOG PULL — MIGRATION-EXPIRES: 2027-02-01.
fn legacy_log_tag(identity_seed: &[u8; 32]) -> [u8; 32] {
    let mut input = Vec::with_capacity(13 + 32);
    input.extend_from_slice(b"photon_log_v1");
    input.extend_from_slice(identity_seed);
    ihi::spaghettify(&input)
}

/// The pseudonym map's (handle, petname) pairs, or none when the map is absent; the handles stay in memory only.
fn map_pairs() -> Vec<(String, String)> {
    let map_path = std::env::var("FERROS_HANDLE_MAP").unwrap_or_else(|_| "/mnt/Harbor/Code/keys/claude-pseudonym-map.txt".to_string());
    let Ok(map) = std::fs::read_to_string(&map_path) else { return Vec::new() };
    map.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.trim_end().split_once(" = ").map(|(h, p)| (h.to_string(), p.to_string())))
        .collect()
}

/// Pull, open and print every log filed under a handle proof, then clear the filed copies once each one opened (the local cache keeps the sealed documents; the 24 h sweep is the backstop).
fn pull_filed(hp: &[u8; 32], filter: &Filter) {
    use photon_messenger::network::fgtw::{log_consume_blocking, log_get_filed_blocking, log_list_by_proof_blocking, parse_filed_log};
    let log_key = match photon_messenger::crypto::log_seal::log_key() {
        Ok(k) => k,
        Err(e) => {
            eprintln!("photonlog: the log key: {e}");
            std::process::exit(2);
        }
    };
    let keys = match log_list_by_proof_blocking(hp) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("photonlog: log_list failed: {e}");
            std::process::exit(1);
        }
    };
    if keys.is_empty() {
        eprintln!("photonlog: no filed logs for {}", photon_messenger::fp(hp));
        return;
    }
    eprintln!("photonlog: {} filed log(s) for {}", keys.len(), photon_messenger::fp(hp));
    // Filed logs never change (each is filed under its own time), so a cached document is never fetched again; the cache holds them sealed, as fgtw.org did.
    let cache_dir = std::env::var("HOME").map(|h| std::path::PathBuf::from(h).join(".cache/photon-logs")).unwrap_or_else(|_| std::path::PathBuf::from(".photon-log-cache"));
    if let Err(e) = std::fs::create_dir_all(&cache_dir) {
        eprintln!("photonlog: cache dir {} not made ({e}); every log is fetched", cache_dir.display());
    }
    let mut failures = 0usize;
    for k in &keys {
        let path = cache_dir.join(k.replace('/', "_"));
        let doc = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => match log_get_filed_blocking(k) {
                Ok(b) => {
                    if let Err(e) = std::fs::write(&path, &b) {
                        eprintln!("photonlog: caching {} failed: {e}", path.display());
                    }
                    b
                }
                Err(e) => {
                    eprintln!("photonlog: {k}: fetch failed ({e})");
                    failures += 1;
                    continue;
                }
            },
        };
        let filed = match parse_filed_log(&doc) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("photonlog: {k}: {e}");
                failures += 1;
                continue;
            }
        };
        println!("\n── {k} ── device {} ──", photon_messenger::fp(&filed.device));
        if let Some(note) = &filed.note {
            match photon_messenger::crypto::log_seal::open(note, &filed.ephemeral, &log_key).map(String::from_utf8) {
                Ok(Ok(text)) => println!("★ NOTE FROM SUBMITTER: {text}"),
                _ => eprintln!("photonlog: {k}: the note did not open"),
            }
        }
        match photon_messenger::crypto::log_seal::open(&filed.log, &filed.ephemeral, &log_key) {
            Ok(plain) => {
                print_records(&plain, filter);
            }
            Err(e) => {
                eprintln!("photonlog: {k}: did not open ({e})");
                failures += 1;
            }
        }
    }
    if failures > 0 {
        eprintln!("photonlog: {failures} log(s) failed; the filed copies stay for a retry");
        return;
    }
    let release = match photon_messenger::crypto::self_verify::release_signing_key() {
        Ok(k) => k,
        Err(e) => {
            eprintln!("photonlog: filed copies kept until the 24 h sweep ({e})");
            return;
        }
    };
    match log_consume_blocking(&release, hp) {
        Ok(()) => eprintln!("photonlog: consumed; the filed copies are deleted (the cache keeps them sealed)"),
        Err(e) => eprintln!("photonlog: clearing the filed copies failed ({e}); they stay until the 24 h sweep"),
    }
}

fn main() {
    let mut path: Option<String> = None;
    let mut min_level = 0u64;
    let mut grep: Option<String> = None;
    let mut follow = false;
    // Whose logs: `hp` (the handle proof) finds the logs filed since M6, which the maintainer's log key opens; `seed` finds and opens a pre-M6 Photon's (LEGACY LOG PULL); `raw_key` opens a local pre-M6 blob.
    let mut hp: Option<[u8; 32]> = None;
    let mut seed: Option<[u8; 32]> = None;
    let mut raw_key: Option<[u8; 32]> = None;
    let mut pull = false;
    let mut mailbox = false;

    let mut gripes_list = false;
    let mut gripe_status: Option<([u8; 32], String, String, String)> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-f" | "--follow" => follow = true,
            "-p" | "--pull" => pull = true,
            // The gripes (docs/ideas.md): list them, or set one's status, signed by the release key.
            "--gripes" => gripes_list = true,
            "--gripe-status" => {
                let id = args.next().as_deref().and_then(parse_hex32);
                let state = args.next();
                let note = args.next().unwrap_or_default();
                let version = args.next().unwrap_or_default();
                match (id, state) {
                    (Some(id), Some(state)) => gripe_status = Some((id, state, note, version)),
                    _ => {
                        eprintln!("photonlog: --gripe-status <id 64hex> <received|seen|planned|working|fixed|declined|duplicate> [note] [version]");
                        std::process::exit(2);
                    }
                }
            }
            "-M" | "--mailbox" => mailbox = true,
            "-H" | "--handle" => match args.next() {
                // The handle proof finds the person's filed logs (~1 s, memory-hard); the identity seed (cheap) finds a pre-M6 Photon's (LEGACY LOG PULL). Photon's exact derivations, so they match the submitter's.
                Some(h) => {
                    hp = Some(photon_messenger::types::Handle::username_to_handle_proof(&h));
                    seed = Some(photon_messenger::storage::contacts::derive_identity_seed(&h));
                }
                None => {
                    eprintln!("photonlog: --handle needs the handle string");
                    std::process::exit(2);
                }
            },
            "-s" | "--seed" => match args.next().as_deref().and_then(parse_hex32) {
                Some(s) => seed = Some(s),
                None => {
                    eprintln!("photonlog: --seed needs 64 hex chars (the 32-byte identity seed)");
                    std::process::exit(2);
                }
            },
            "-P" | "--petname" => match args.next() {
                Some(p) => match petname_to_handle(&p) {
                    Ok(h) => {
                        hp = Some(photon_messenger::types::Handle::username_to_handle_proof(&h));
                        seed = Some(photon_messenger::storage::contacts::derive_identity_seed(&h));
                    }
                    Err(e) => {
                        eprintln!("photonlog: {e}");
                        std::process::exit(2);
                    }
                },
                None => {
                    eprintln!("photonlog: --petname needs the person's petname (right column of the map)");
                    std::process::exit(2);
                }
            },
            // The live tohu registers hold the identity THIS device's app submits under, proof and seed both; neither appears on a command line or a screen.
            "-S" | "--session" => match tohu::session() {
                Some(s) => {
                    hp = Some(s.handle_proof);
                    seed = Some(s.identity_seed);
                }
                None => {
                    eprintln!("photonlog: no live session in the tohu registers — open photon once this boot (resume or attest), then retry");
                    std::process::exit(2);
                }
            },
            "-k" | "--key" => match args.next().as_deref().and_then(parse_hex32) {
                Some(k) => raw_key = Some(k),
                None => {
                    eprintln!("photonlog: --key needs 64 hex chars (the 32-byte log key)");
                    std::process::exit(2);
                }
            },
            "-l" | "--level" => match args.next().and_then(|v| level_from_arg(&v)) {
                Some(n) => min_level = n,
                None => {
                    eprintln!("photonlog: --level needs TRACE|DEBUG|INFO|WARN|ERROR or 0..4");
                    std::process::exit(2);
                }
            },
            "-g" | "--grep" => match args.next() {
                Some(s) => grep = Some(s.to_lowercase()),
                None => {
                    eprintln!("photonlog: --grep needs a substring");
                    std::process::exit(2);
                }
            },
            other if other.starts_with('-') => {
                eprintln!("photonlog: unknown flag {other}");
                std::process::exit(2);
            }
            other => path = Some(other.to_string()),
        }
    }
    let filter = Filter { min_level, grep };

    // The developer's side of the Ideas page (docs/ideas.md): the list is public, and a status is signed by the release key, which fgtw.org checks (no token, 2026-11-10).
    if gripes_list || gripe_status.is_some() {
        if let Some((id, state, note, version)) = gripe_status {
            let key = match photon_messenger::crypto::self_verify::release_signing_key() {
                Ok(k) => k,
                Err(e) => {
                    eprintln!("photonlog: {e}");
                    std::process::exit(2);
                }
            };
            match photon_messenger::network::fgtw::gripe_status_blocking(&key, &id, &state, &note, &version) {
                Ok(()) => println!("{} → {}{}", hex::encode(&id[..8]), state, if note.is_empty() { String::new() } else { format!(" ({note})") }),
                Err(e) => {
                    eprintln!("photonlog: gripe_status failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        if gripes_list {
            match photon_messenger::network::fgtw::gripe_list_blocking() {
                Ok(mut rows) => {
                    rows.sort_by_key(|g| g.received_osc);
                    for g in &rows {
                        println!("{}  {:<4} {:<9} {}  {}", hex::encode(g.id), g.kind, g.state, eagle_display(g.received_osc), g.text.replace('\n', " / "));
                        if !g.note.is_empty() || !g.version.is_empty() {
                            println!("    note: {}{}", g.note, if g.version.is_empty() { String::new() } else { format!(" [{}]", g.version) });
                        }
                    }
                    eprintln!("photonlog: {} gripe(s)", rows.len());
                }
                Err(e) => {
                    eprintln!("photonlog: gripe_list failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        return;
    }

    // The maintainer's mailbox (M6): every filed log, whose and when. A name comes from the pseudonym map: each handle's proof is computed (~1 s each) until every person in the mailbox is named.
    if mailbox {
        let key = match photon_messenger::crypto::self_verify::release_signing_key() {
            Ok(k) => k,
            Err(e) => {
                eprintln!("photonlog: {e}");
                std::process::exit(2);
            }
        };
        let mut entries = match photon_messenger::network::fgtw::log_mailbox_blocking(&key) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("photonlog: log_mailbox failed: {e}");
                std::process::exit(1);
            }
        };
        entries.sort_by_key(|e| e.filed_osc);
        let mut names: Vec<([u8; 32], String)> = Vec::new();
        let mut unnamed: Vec<[u8; 32]> = entries.iter().map(|e| e.handle_proof).collect();
        unnamed.sort_unstable();
        unnamed.dedup();
        for (handle, pet) in map_pairs() {
            if unnamed.is_empty() {
                break;
            }
            let proof = photon_messenger::types::Handle::username_to_handle_proof(&handle);
            if let Some(i) = unnamed.iter().position(|p| *p == proof) {
                unnamed.swap_remove(i);
                names.push((proof, pet));
            }
        }
        for e in &entries {
            let who = names.iter().find(|(p, _)| *p == e.handle_proof).map(|(_, n)| n.clone()).unwrap_or_else(|| photon_messenger::fp(&e.handle_proof));
            println!("{}  {:<24} {:>10}  {}", eagle_display(e.filed_osc), who, photon_messenger::unit_size(e.size, photon_messenger::SizeUnit::KiB), e.key);
        }
        eprintln!("photonlog: {} filed log(s)", entries.len());
        return;
    }

    // Pull mode, M6: the logs filed under the person's handle proof, opened with the maintainer's log key.
    if pull {
        match hp {
            Some(hp) => pull_filed(&hp, &filter),
            None if seed.is_none() => {
                eprintln!("photonlog: --pull needs --petname <name>, --handle <name> or --session to identify whose logs to fetch");
                std::process::exit(2);
            }
            None => {}
        }
    }

    // LEGACY LOG PULL — MIGRATION-EXPIRES: 2027-02-01: a pre-M6 Photon's logs, found and opened by the identity seed.
    if pull {
        let Some(seed) = seed else {
            return;
        };
        let tag = legacy_log_tag(&seed);
        let key = legacy_log_key(&seed);
        use photon_messenger::network::fgtw::{
            log_delete_blocking, log_get_blocking, log_list_blocking,
        };
        let keys = match log_list_blocking(&tag) {
            Ok(k) => k,
            Err(e) => {
                eprintln!("photonlog: log_list failed: {e}");
                std::process::exit(1);
            }
        };
        if keys.is_empty() {
            eprintln!(
                "photonlog: no pre-M6 logs for that identity (tag {})",
                hex::encode(&tag[..6])
            );
            return;
        }
        eprintln!(
            "photonlog: {} pre-M6 log(s) for tag {}",
            keys.len(),
            hex::encode(&tag[..6])
        );
        // Local ciphertext cache: submitted logs are IMMUTABLE (each submission is a fresh eagle-stamped key), so anything already on disk never needs re-fetching — a pull that used to re-download the whole history (minutes) now only fetches the new submissions. Cache the CIPHERTEXT, so the at-rest copy stays as sealed as R2's; the seed still gates reading.
        let cache_dir = std::env::var("HOME")
            .map(|h| std::path::PathBuf::from(h).join(".cache/photon-logs"))
            .unwrap_or_else(|_| std::path::PathBuf::from(".photon-log-cache"));
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            eprintln!("photonlog: cache dir {} not made ({e}); every log is fetched", cache_dir.display());
        }
        let cache_path = |k: &str| cache_dir.join(k.replace('/', "_"));
        let mut fetched = 0usize;
        let mut cached = 0usize;
        // A cached empty file = a prior "note absent" verdict (notes are optional; most logs have none) — skips the per-log round-trip that dominated pull time.
        let mut get_cached = |k: &str| -> Option<Vec<u8>> {
            let path = cache_path(k);
            if let Ok(bytes) = std::fs::read(&path) {
                cached += 1;
                return if bytes.is_empty() { None } else { Some(bytes) };
            }
            match log_get_blocking(k) {
                Ok(ct) => {
                    fetched += 1;
                    if let Err(e) = std::fs::write(&path, &ct) {
                        eprintln!("photonlog: caching {} failed: {e}", path.display());
                    }
                    Some(ct)
                }
                Err(_) => {
                    if let Err(e) = std::fs::write(&path, b"") {
                        eprintln!("photonlog: caching the absent verdict at {} failed: {e}", path.display());
                    }
                    None
                }
            }
        };
        let mut failures = 0usize;
        for k in &keys {
            // The submitter's note rides as a sealed `.note` sidecar next to the `.vsf` (absent when none was typed) — surface it ABOVE the log so the human context leads the dump.
            let note_key = format!("{}.note", k.strip_suffix(".vsf").unwrap_or(k));
            if let Some(ct) = get_cached(&note_key) {
                if let Ok(plain) = photon_messenger::storage::decrypt_bytes(&ct, &key) {
                    if let Ok(text) = String::from_utf8(plain) {
                        println!("\n── {k} ──");
                        println!("★ NOTE FROM SUBMITTER: {text}");
                    }
                }
            }
            match get_cached(k) {
                Some(ct) => match photon_messenger::storage::decrypt_bytes(&ct, &key) {
                    Ok(plain) => {
                        println!("\n── {k} ──");
                        print_records(&plain, &filter);
                    }
                    Err(e) => {
                        eprintln!("photonlog: {k}: decrypt failed ({e})");
                        failures += 1;
                    }
                },
                None => {
                    eprintln!("photonlog: {k}: fetch failed (and not cached)");
                    failures += 1;
                }
            }
        }
        eprintln!(
            "photonlog: {} fetched, {} from cache ({})",
            fetched,
            cached,
            cache_dir.display()
        );
        // The pull IS the consumption edge: everything decoded clean is now held locally (the ciphertext cache is the archive), so the server copies have no further purpose and get wiped. Any failure leaves them for a retry; the 24h sweep remains the backstop for never-pulled logs.
        if failures == 0 {
            match log_delete_blocking(&tag) {
                Ok(()) => eprintln!(
                    "photonlog: consumed — server copies deleted (cache keeps the ciphertext)"
                ),
                Err(e) => eprintln!(
                    "photonlog: server wipe failed ({e}) — copies remain until the 24h sweep"
                ),
            }
        } else {
            eprintln!("photonlog: {failures} object(s) failed — leaving server copies for a retry");
        }
        return;
    }

    // Decrypt key for a LOCAL sealed blob: the raw log key if given, else derived from the seed.
    let log_key: Option<[u8; 32]> =
        raw_key.or_else(|| seed.as_ref().map(legacy_log_key));

    let path = path.unwrap_or_else(|| "photon.log.vsf".to_string());

    let bytes = match read_all(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("photonlog: cannot read {path}: {e}");
            std::process::exit(1);
        }
    };

    // A sealed (submitted) log is ChaCha20-Poly1305 ciphertext, not a VSF record stream — decrypt it to the plaintext log before decoding. Follow mode is meaningless for a static blob, so a decrypt is always a single decode.
    let bytes = if let Some(key) = log_key {
        follow = false;
        match photon_messenger::storage::decrypt_bytes(&bytes, &key) {
            Ok(plain) => plain,
            Err(e) => {
                eprintln!("photonlog: decrypt failed ({e}) — wrong seed/key for this log?");
                std::process::exit(1);
            }
        }
    } else {
        bytes
    };

    let mut consumed = print_records(&bytes, &filter);

    if !follow {
        eprintln!("photonlog: {path} ({} bytes)", bytes.len());
        return;
    }

    // Follow mode: poll for growth, decoding only the newly-appended whole records.
    // A SHRINK means the 16 MiB rotation trimmed the file — reset to the new start and note it.
    eprintln!("photonlog: following {path} (Ctrl-C to stop)");
    loop {
        std::thread::sleep(std::time::Duration::from_millis(300));
        let bytes = match read_all(&path) {
            Ok(b) => b,
            Err(_) => continue, // momentarily gone (e.g. mid-rotation) — try again
        };
        if bytes.len() < consumed {
            eprintln!("── log rotated ──");
            consumed = 0;
        }
        if bytes.len() > consumed {
            consumed += print_records(&bytes[consumed..], &filter);
        }
    }
}
