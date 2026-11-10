//! The Octopus page's three ops on fgtw.org (Nick 2026-11-10): the server only counts its buckets, hands over its activity log and clears; Photon draws the page.
//! Each request is a complete VSF document signed by this device for the identity it belongs to; the worker answers only a current member of that fleet ("no auth whatsoever other than knowledge of Octopus, and of course a working Photon that's attested").
//! Blocking: call off the UI thread.

use super::Keypair;
#[cfg(not(target_os = "ferros"))]
use crate::network::http::SEED_HTTPS as FGTW_URL;
use vsf::file_format::VsfSection;
use vsf::VsfType;

/// One storage namespace on fgtw.org and how many objects it holds.
#[derive(Clone, Debug)]
pub struct OctopusBucket {
    pub name: String,
    pub count: u64,
}

/// One line of the worker's activity log.
#[derive(Clone, Debug)]
pub struct OctopusEntry {
    pub t: i64,
    pub kind: String,
    pub msg: String,
    pub ip: Option<std::net::IpAddr>,
    pub hp: Option<[u8; 32]>,
    pub dev: Option<[u8; 32]>,
}

/// What `octopus_view` answers.
#[derive(Clone, Debug, Default)]
pub struct OctopusView {
    pub buckets: Vec<OctopusBucket>,
    pub entries: Vec<OctopusEntry>,
}

/// ferros reaches no fgtw.org.
#[cfg(target_os = "ferros")]
fn post_signed(_device: &Keypair, _hp: &[u8; 32], op: &str, _fields: Vec<(String, VsfType)>) -> Result<Vec<VsfSection>, String> {
    Err(format!("{op}: no fgtw.org from ferros"))
}

/// Sign `fields` as section `op` for identity `hp` and post it; the answer's sections, or the worker's refusal.
#[cfg(not(target_os = "ferros"))]
fn post_signed(device: &Keypair, hp: &[u8; 32], op: &str, mut fields: Vec<(String, VsfType)>) -> Result<Vec<VsfSection>, String> {
    fields.insert(0, ("hp".to_string(), VsfType::hP(hp.to_vec())));
    let pk = device.public.to_bytes();
    let unsigned = vsf::VsfBuilder::new()
        .creation_time_oscillations(crate::network::time_base::now_osc())
        .signed_only_eggs(VsfType::ke(pk.to_vec()), &super::fleet::envelope_slots(&pk))
        .add_section(op, fields)
        .build()
        .map_err(|e| format!("{op}: build: {e}"))?;
    let body = super::fleet::sign_device_envelope(unsigned, &pk, device.secret.as_bytes()).map_err(|e| format!("{op}: sign: {e}"))?;
    let client = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(1 << 6)).build().map_err(|e| format!("{op}: client: {e}"))?;
    let bytes = client
        .post(FGTW_URL)
        .header("Content-Type", "application/octet-stream")
        .body(body)
        .send()
        .and_then(|r| r.bytes())
        .map_err(|e| format!("{op}: {e}"))?;
    if let Some((reason, detail)) = fgtw::client::error_frame(&bytes) {
        return Err(format!("{reason}: {detail}"));
    }
    crate::storage::record::verified_sections(&bytes, None).map_err(|e| format!("{op}: answer: {e}"))
}

/// The optional `who` field: the identity a request filters to.
fn who_field(who: Option<[u8; 32]>) -> Vec<(String, VsfType)> {
    who.map(|w| vec![("who".to_string(), VsfType::hP(w.to_vec()))]).unwrap_or_default()
}

/// The first value of field `name`.
fn first<'a>(s: &'a VsfSection, name: &str) -> Option<&'a VsfType> {
    s.get_field(name).and_then(|f| f.values.first())
}

/// ASCII text in field `name`.
fn ascii(s: &VsfSection, name: &str) -> Option<String> {
    match first(s, name)? {
        VsfType::a(t) => Some(t.to_string()),
        _ => None,
    }
}

/// A 32-byte value in field `name`, handle proof (`hP`) or device key (`ke`).
fn bytes32(s: &VsfSection, name: &str) -> Option<[u8; 32]> {
    match first(s, name)? {
        VsfType::hP(b) | VsfType::ke(b) => <[u8; 32]>::try_from(b.as_slice()).ok(),
        _ => None,
    }
}

/// Every bucket's count and the activity log, signed by this device for its identity `hp`; with `who`, only what belongs to that identity.
pub fn view_blocking(device: &Keypair, hp: &[u8; 32], who: Option<[u8; 32]>) -> Result<OctopusView, String> {
    let sections = post_signed(device, hp, "octopus_view", who_field(who))?;
    let mut view = OctopusView::default();
    for s in &sections {
        match s.name.as_str() {
            "bucket" => {
                let (Some(name), Some(count)) = (ascii(s, "name"), first(s, "count").and_then(|v| v.as_u64())) else { continue };
                view.buckets.push(OctopusBucket { name, count });
            }
            "entry" => {
                let t = match first(s, "t") {
                    Some(VsfType::e(vsf::types::EtType::e6(o))) => *o,
                    Some(other) => other.as_i64().unwrap_or(0),
                    None => continue,
                };
                let ip = match first(s, "ip") {
                    Some(VsfType::t_u3(t)) => match t.data.len() {
                        4 => <[u8; 4]>::try_from(t.data.as_slice()).ok().map(std::net::IpAddr::from),
                        16 => <[u8; 16]>::try_from(t.data.as_slice()).ok().map(std::net::IpAddr::from),
                        _ => None,
                    },
                    _ => None,
                };
                view.entries.push(OctopusEntry {
                    t,
                    kind: ascii(s, "kind").unwrap_or_default(),
                    msg: ascii(s, "msg").unwrap_or_default(),
                    ip,
                    hp: bytes32(s, "hp"),
                    dev: bytes32(s, "dev"),
                });
            }
            _ => {}
        }
    }
    Ok(view)
}

/// Empty one bucket (`Some(name)`) or everything but the landing page (`None`), or with `who` only that identity's part of it, repeating while the worker says more remain. Returns how many objects went.
pub fn clear_blocking(device: &Keypair, hp: &[u8; 32], bucket: Option<&str>, who: Option<[u8; 32]>) -> Result<u64, String> {
    let (op, mut fields) = match bucket {
        Some(name) => {
            let ascii = vsf::types::Ascii::new(name).map_err(|_| "octopus_clear: a bucket name is ASCII".to_string())?;
            ("octopus_clear", vec![("bucket".to_string(), VsfType::a(ascii))])
        }
        None => ("octopus_nuke", Vec::new()),
    };
    fields.extend(who_field(who));
    let mut total = 0u64;
    loop {
        let sections = post_signed(device, hp, op, fields.clone())?;
        let done = sections.iter().find(|s| s.name == "octopus_cleared").ok_or_else(|| format!("{op}: no octopus_cleared in the answer"))?;
        total += first(done, "deleted").and_then(|v| v.as_u64()).unwrap_or(0);
        if first(done, "more").and_then(|v| v.as_u64()) != Some(1) {
            return Ok(total);
        }
    }
}
