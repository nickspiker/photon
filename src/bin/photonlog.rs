//! photonlog — decode a `photon.log.vsf` structured log into human-readable lines.
//!
//! The log is a stream of complete VSF records, each `{creation_time (Eagle), section "log" {lvl, msg}}`
//! (see `crate::log` in lib.rs). This walks the records in order and prints
//! `<eagle-time>  [LEVEL]  <msg>`. Pull the file off a phone with:
//!     adb pull /data/user/0/com.photon.messenger/files/photon.log.vsf
//! then: `photonlog photon.log.vsf` (or pipe a path as the one arg; defaults to ./photon.log.vsf).

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

/// Eagle oscillations → a readable UTC string for display (display-only conversion is allowed).
fn eagle_display(osc: i64) -> String {
    let et = vsf::types::EagleTime::from_oscillations(osc);
    match et.to_datetime() {
        dt => dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
    }
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "photon.log.vsf".to_string());
    let mut bytes = Vec::new();
    match std::fs::File::open(&path).and_then(|mut f| f.read_to_end(&mut bytes)) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("photonlog: cannot read {path}: {e}");
            std::process::exit(1);
        }
    }

    let mut offset = 0usize;
    let mut count = 0u64;
    while offset < bytes.len() {
        let rest = &bytes[offset..];
        // Decode this record's header, then its single "log" section. The section parse advances a pointer
        // to the record's end, which is where the next record begins.
        let (header, header_end) = match VsfHeader::decode(rest) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("photonlog: stopped at byte {offset}: header decode: {e}");
                break;
            }
        };
        let mut ptr = 0usize;
        let section = match VsfSection::parse(&rest[header_end..], &mut ptr) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("photonlog: stopped at byte {offset}: section parse: {e}");
                break;
            }
        };
        let record_len = header_end + ptr;

        let ts = match &header.creation_time {
            Some(VsfType::e(EtType::e6(o))) => eagle_display(*o),
            Some(VsfType::e(EtType::e5(o))) => eagle_display(*o as i64),
            Some(VsfType::e(EtType::e7(o))) => eagle_display(*o as i64),
            _ => "(no time)".to_string(),
        };
        let lvl = section
            .get_field("lvl")
            .and_then(|f| f.values.first())
            .and_then(|v| u64::try_from_vsf(v))
            .unwrap_or(u64::MAX);
        let msg = section
            .get_field("msg")
            .and_then(|f| f.values.first())
            .and_then(|v| match v {
                VsfType::x(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();

        println!("{ts}  [{}]  {msg}", level_name(lvl));
        count += 1;

        if record_len == 0 {
            eprintln!("photonlog: zero-length record at {offset}, stopping");
            break;
        }
        offset += record_len;
    }

    eprintln!("photonlog: {count} records from {path}");
}

/// Tolerant unsigned read — the encoder optimises a small `u(0)` to `u3` on round-trip, so match any width.
trait TryFromVsf {
    fn try_from_vsf(v: &VsfType) -> Option<u64>;
}
impl TryFromVsf for u64 {
    fn try_from_vsf(v: &VsfType) -> Option<u64> {
        use vsf::schema::FromVsfType;
        u64::from_vsf_type(v).ok()
    }
}
