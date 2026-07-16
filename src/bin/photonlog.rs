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
//!   -p, --pull          fetch this identity's SUBMITTED logs straight from FGTW (needs --handle or --seed), decrypt, and decode — no manual R2 wrangling. The seed derives both the retrieval tag (to find them) and the key (to open them).
//!   -H, --handle NAME   the peer's handle. Derives their identity seed on the spot (cheap) — the friendly way to identify whose logs to pull. Use this; --seed is the raw-bytes escape hatch.
//!   -s, --seed HEX64    the peer's 32-byte identity seed directly (deterministic from their handle). Equivalent to --handle but pre-derived.
//!   -k, --key  HEX64    like --seed for local decrypt but you already hold the raw 32-byte log key (skips the seed→key derivation; can't --pull).
//!
//! Examples: `photonlog -l warn` · `photonlog --pull --handle alice -l warn` · `photonlog --pull --seed <64hex>` · `photonlog her-blob.vsf --handle alice`.

use std::io::Read;
use vsf::file_format::{VsfHeader, VsfSection};
use vsf::types::EtType;
use vsf::VsfType;

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
    vsf::types::EagleTime::from_oscillations(osc).to_datetime().format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

struct Filter {
    min_level: u64,
    grep: Option<String>,
}

/// Decode and print whole records from `buf`, applying the filter.
/// Returns the number of bytes consumed — i.e. the offset of the last COMPLETE record boundary — so a
/// half-written trailing record (mid-append) is left for the next pass instead of being mis-decoded.
fn print_records(buf: &[u8], filter: &Filter) -> usize {
    let mut off = 0usize;
    while off < buf.len() {
        let rest = &buf[off..];
        let (header, header_end) = match VsfHeader::decode(rest) {
            Ok(h) => h,
            Err(_) => break, // incomplete tail — stop, retry next pass
        };
        let mut ptr = 0usize;
        let section = match VsfSection::parse(&rest[header_end..], &mut ptr) {
            Ok(s) => s,
            Err(_) => break,
        };
        let rec = header_end + ptr;
        if rec == 0 {
            break;
        }

        let lvl = section
            .get_field("lvl")
            .and_then(|f| f.values.first())
            .and_then(u64_of)
            .unwrap_or(u64::MAX);
        let template = section
            .get_field("msg")
            .and_then(|f| f.values.first())
            .and_then(|v| match v {
                VsfType::x(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();
        // Structured records: typed `val` fields substitute into the template's slots at READ time — the record stored numbers binary; this terminal render picks the base (current mixed arabic per the display doctrine).
        let vals: Vec<photon_messenger::LogValue> = section
            .get_fields("val")
            .iter()
            .filter_map(|f| f.values.first())
            .map(|v| match v {
                VsfType::u(n, _) => photon_messenger::LogValue::U(*n as u128),
                VsfType::i6(n) => photon_messenger::LogValue::I(*n as i128),
                VsfType::f6(n) => photon_messenger::LogValue::F(*n),
                VsfType::x(s) => photon_messenger::LogValue::T(s.clone()),
                VsfType::v_u3(vec) if vec.data.len() == 6 || vec.data.len() == 18 => {
                    let (ip_bytes, port_bytes) = vec.data.split_at(vec.data.len() - 2);
                    let port = u16::from_le_bytes([port_bytes[0], port_bytes[1]]);
                    let ip: std::net::IpAddr = if ip_bytes.len() == 4 {
                        std::net::Ipv4Addr::new(ip_bytes[0], ip_bytes[1], ip_bytes[2], ip_bytes[3]).into()
                    } else {
                        let mut o = [0u8; 16];
                        o.copy_from_slice(ip_bytes);
                        std::net::Ipv6Addr::from(o).into()
                    };
                    photon_messenger::LogValue::Addr(std::net::SocketAddr::new(ip, port))
                }
                other => photon_messenger::LogValue::T(format!("{other:?}")),
            })
            .collect();
        let msg = if vals.is_empty() {
            template
        } else {
            photon_messenger::render_log_line(&template, &vals)
        };

        let pass_level = lvl >= filter.min_level;
        let pass_grep = match &filter.grep {
            Some(g) => msg.to_lowercase().contains(g),
            None => true,
        };
        if pass_level && pass_grep {
            let ts = match &header.creation_time {
                Some(VsfType::e(EtType::e6(o))) => eagle_display(*o),
                Some(VsfType::e(EtType::e5(o))) => eagle_display(*o as i64),
                Some(VsfType::e(EtType::e7(o))) => eagle_display(*o as i64),
                _ => "(no time)".to_string(),
            };
            println!("{ts}  [{}]  {msg}", level_name(lvl));
        }
        off += rec;
    }
    off
}

fn read_all(path: &str) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Parse a 64-char hex string into a 32-byte array (identity seed or raw log key).
fn parse_hex32(s: &str) -> Option<[u8; 32]> {
    let v = (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect::<Option<Vec<u8>>>()?;
    v.try_into().ok()
}

fn main() {
    let mut path: Option<String> = None;
    let mut min_level = 0u64;
    let mut grep: Option<String> = None;
    let mut follow = false;
    // Decrypt / pull key material. `seed` (the identity seed) drives BOTH the retrieval tag (find) and the log key (decrypt); `raw_key` is the log key alone (decrypt a local blob only, can't pull).
    let mut seed: Option<[u8; 32]> = None;
    let mut raw_key: Option<[u8; 32]> = None;
    let mut pull = false;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-f" | "--follow" => follow = true,
            "-p" | "--pull" => pull = true,
            "-H" | "--handle" => match args.next() {
                // The friendly path: derive the identity seed straight from the handle (cheap — ihi::handle_to_hash, NOT the memory-hard proof), using photon's exact canonicalization so it matches the submitter's seed.
                Some(h) => seed = Some(photon_messenger::storage::contacts::derive_identity_seed(&h)),
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

    // Pull mode: fetch this identity's submitted logs straight from FGTW, decrypt, and decode — no manual R2 wrangling. The seed derives the retrieval tag (to find them) and the log key (to open them); knowledge of the seed IS the whole capability.
    if pull {
        let Some(seed) = seed else {
            eprintln!("photonlog: --pull needs --handle <name> (or --seed <64hex>) to identify whose logs to fetch");
            std::process::exit(2);
        };
        let tag = photon_messenger::log_retrieval_tag(&seed);
        let key = photon_messenger::log_encryption_key(&seed);
        use photon_messenger::network::fgtw::{log_get_blocking, log_list_blocking};
        let keys = match log_list_blocking(&tag) {
            Ok(k) => k,
            Err(e) => {
                eprintln!("photonlog: log_list failed: {e}");
                std::process::exit(1);
            }
        };
        if keys.is_empty() {
            eprintln!("photonlog: no submitted logs for that identity (tag {})", hex::encode(&tag[..6]));
            return;
        }
        eprintln!("photonlog: {} submitted log(s) for tag {}", keys.len(), hex::encode(&tag[..6]));
        for k in &keys {
            match log_get_blocking(k) {
                Ok(ct) => match photon_messenger::storage::decrypt_bytes(&ct, &key) {
                    Ok(plain) => {
                        println!("\n── {k} ──");
                        print_records(&plain, &filter);
                    }
                    Err(e) => eprintln!("photonlog: {k}: decrypt failed ({e})"),
                },
                Err(e) => eprintln!("photonlog: {k}: fetch failed ({e})"),
            }
        }
        return;
    }

    // Decrypt key for a LOCAL sealed blob: the raw log key if given, else derived from the seed.
    let log_key: Option<[u8; 32]> = raw_key.or_else(|| seed.as_ref().map(photon_messenger::log_encryption_key));

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

/// Tolerant unsigned read — the encoder optimises a small `u(0)` to `u3` on round-trip, so match any width.
fn u64_of(v: &VsfType) -> Option<u64> {
    use vsf::schema::FromVsfType;
    u64::from_vsf_type(v).ok()
}
