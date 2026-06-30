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
//!
//! Examples: `photonlog -l warn` · `photonlog -f -g FGTW` · `photonlog phone.log.vsf -l error`.

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
        let msg = section
            .get_field("msg")
            .and_then(|f| f.values.first())
            .and_then(|v| match v {
                VsfType::x(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();

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

fn main() {
    let mut path: Option<String> = None;
    let mut min_level = 0u64;
    let mut grep: Option<String> = None;
    let mut follow = false;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-f" | "--follow" => follow = true,
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
    let path = path.unwrap_or_else(|| "photon.log.vsf".to_string());
    let filter = Filter { min_level, grep };

    let bytes = match read_all(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("photonlog: cannot read {path}: {e}");
            std::process::exit(1);
        }
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
