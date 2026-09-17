//! Port mapping on the home router — NAT-PMP (RFC 6886), PCP MAP (RFC 6887) and UPnP IGD, in that order (Nick 2026-09-17: "Or I do port mapping on my desktop. And all the things.").
//!
//! A device behind a home NAT that opens a stable public port becomes directly reachable to EVERY peer with no punch at all — the strongest anti-relay lever there is, and the one the Jon incident wanted (a Mac on a home router, relay-only all day). The mapping is `external:port → us:our UDP port`; the external address is posted as the reflexive seed, so the published record carries the mapped port and peers aim straight at it. Renewed at half its lifetime on a background thread; a router that answers none of the three protocols costs one round of small timeouts and is never asked again this session.
//!
//! Everything here is plain std sockets and a few hundred bytes of protocol — no crate, nothing async. Cellular has no gateway to ask (the CGNAT is the carrier's), so a phone on mobile data fails fast and quietly; on home Wi-Fi it maps like a desktop.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::Duration;

/// A mapping the router granted: where the world reaches us, for how long, and which protocol said so.
#[derive(Debug, Clone)]
pub struct Mapping {
    pub external: SocketAddrV4,
    pub lifetime: Duration,
    pub via: &'static str,
}

/// The default gateway's IPv4 address — the router that owns our NAT.
pub fn default_gateway() -> Option<Ipv4Addr> {
    #[cfg(target_os = "linux")]
    {
        // /proc/net/route: Iface Destination Gateway Flags … — hex little-endian, the default route has destination 00000000.
        let text = std::fs::read_to_string("/proc/net/route").ok()?;
        for line in text.lines().skip(1) {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() >= 3 && f[1] == "00000000" {
                let g = u32::from_str_radix(f[2], 16).ok()?;
                let ip = Ipv4Addr::from(g.swap_bytes());
                if !ip.is_unspecified() {
                    return Some(ip);
                }
            }
        }
        None
    }
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("route").args(["-n", "get", "default"]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        text.lines()
            .find_map(|l| l.trim().strip_prefix("gateway:"))
            .and_then(|g| g.trim().parse::<Ipv4Addr>().ok())
    }
    #[cfg(target_os = "windows")]
    {
        let out = std::process::Command::new("route").args(["print", "0.0.0.0"]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        text.lines()
            .filter_map(|l| {
                let f: Vec<&str> = l.split_whitespace().collect();
                (f.len() >= 3 && f[0] == "0.0.0.0" && f[1] == "0.0.0.0").then(|| f[2].parse::<Ipv4Addr>().ok()).flatten()
            })
            .next()
    }
    #[cfg(target_os = "android")]
    {
        // No /proc/net/route on modern Android for an app; the LAN address's .1 is the home-router convention and the only cheap guess. Wrong guesses fail in one timeout.
        let ip = crate::network::udp::get_local_ip()?;
        let o = ip.octets();
        Some(Ipv4Addr::new(o[0], o[1], o[2], 1))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows", target_os = "android")))]
    {
        None
    }
}

/// Ask the gateway for a mapping of our UDP `port`, trying NAT-PMP, then PCP, then UPnP. `lifetime` is the ask; the router may grant less (the reply's is the truth).
pub fn request(gateway: Ipv4Addr, port: u16, lifetime: Duration) -> Option<Mapping> {
    natpmp(gateway, port, lifetime)
        .or_else(|| pcp(gateway, port, lifetime))
        .or_else(|| upnp(gateway, port, lifetime))
}

fn udp_to_gateway(gateway: Ipv4Addr, timeout: Duration) -> Option<UdpSocket> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.set_read_timeout(Some(timeout)).ok()?;
    s.connect((gateway, 5351)).ok()?;
    Some(s)
}

/// NAT-PMP (RFC 6886): op 0 = external address, op 1 = map UDP. Two round trips, ~30 bytes each.
fn natpmp(gateway: Ipv4Addr, port: u16, lifetime: Duration) -> Option<Mapping> {
    let s = udp_to_gateway(gateway, Duration::from_millis(500))?;
    // External address.
    s.send(&[0, 0]).ok()?;
    let mut buf = [0u8; 64];
    let n = s.recv(&mut buf).ok()?;
    if n < 12 || buf[0] != 0 || buf[1] != 128 || u16::from_be_bytes([buf[2], buf[3]]) != 0 {
        return None;
    }
    let ext_ip = Ipv4Addr::new(buf[8], buf[9], buf[10], buf[11]);
    // Map UDP: [0, 1, 0, 0, internal port, suggested external port, lifetime].
    let secs = lifetime.as_secs().min(u32::MAX as u64) as u32;
    let mut req = vec![0u8, 1, 0, 0];
    req.extend_from_slice(&port.to_be_bytes());
    req.extend_from_slice(&port.to_be_bytes());
    req.extend_from_slice(&secs.to_be_bytes());
    s.send(&req).ok()?;
    let n = s.recv(&mut buf).ok()?;
    if n < 16 || buf[0] != 0 || buf[1] != 129 || u16::from_be_bytes([buf[2], buf[3]]) != 0 {
        return None;
    }
    let ext_port = u16::from_be_bytes([buf[10], buf[11]]);
    let granted = u32::from_be_bytes([buf[12], buf[13], buf[14], buf[15]]);
    if ext_port == 0 || ext_ip.is_unspecified() || ext_ip.is_private() {
        return None;
    }
    Some(Mapping { external: SocketAddrV4::new(ext_ip, ext_port), lifetime: Duration::from_secs(granted.max(60) as u64), via: "NAT-PMP" })
}

/// PCP (RFC 6887) MAP, version 2, UDP, our local address as the client address, a random nonce, the external address left to the router.
fn pcp(gateway: Ipv4Addr, port: u16, lifetime: Duration) -> Option<Mapping> {
    let local = crate::network::udp::get_local_ip_toward(gateway)?;
    let s = udp_to_gateway(gateway, Duration::from_millis(500))?;
    let secs = lifetime.as_secs().min(u32::MAX as u64) as u32;
    let mut req = Vec::with_capacity(60);
    req.extend_from_slice(&[2u8, 1, 0, 0]); // version 2, opcode MAP, reserved
    req.extend_from_slice(&secs.to_be_bytes());
    req.extend_from_slice(&[0u8; 10]);
    req.extend_from_slice(&[0xff, 0xff]);
    req.extend_from_slice(&local.octets()); // client address, v4-mapped v6
    let nonce: [u8; 12] = rand::random();
    req.extend_from_slice(&nonce);
    req.extend_from_slice(&[17, 0, 0, 0]); // UDP, reserved
    req.extend_from_slice(&port.to_be_bytes());
    req.extend_from_slice(&port.to_be_bytes());
    req.extend_from_slice(&[0u8; 16]); // suggested external address: any
    s.send(&req).ok()?;
    let mut buf = [0u8; 128];
    let n = s.recv(&mut buf).ok()?;
    if n < 60 || buf[0] != 2 || buf[1] != 0x81 || buf[3] != 0 {
        return None;
    }
    let granted = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
    let ext_port = u16::from_be_bytes([buf[42], buf[43]]);
    let ext_ip = Ipv4Addr::new(buf[56], buf[57], buf[58], buf[59]);
    if ext_port == 0 || ext_ip.is_unspecified() || ext_ip.is_private() || buf[54] != 0xff || buf[55] != 0xff {
        return None;
    }
    Some(Mapping { external: SocketAddrV4::new(ext_ip, ext_port), lifetime: Duration::from_secs(granted.max(60) as u64), via: "PCP" })
}

/// UPnP IGD: SSDP discovery on the LAN, the device description for the WANIPConnection control URL, then one SOAP AddPortMapping and one GetExternalIPAddress. String-parsed on purpose — the three fields we need are unambiguous and an XML crate is a dependency the doctrine does not want.
fn upnp(_gateway: Ipv4Addr, port: u16, lifetime: Duration) -> Option<Mapping> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.set_read_timeout(Some(Duration::from_millis(1500))).ok()?;
    let msearch = "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\nST: urn:schemas-upnp-org:service:WANIPConnection:1\r\n\r\n";
    s.send_to(msearch.as_bytes(), "239.255.255.250:1900").ok()?;
    let mut buf = [0u8; 2048];
    let n = s.recv(&mut buf).ok()?;
    let reply = String::from_utf8_lossy(&buf[..n]);
    let location = reply
        .lines()
        .find_map(|l| l.get(..9).filter(|h| h.eq_ignore_ascii_case("location:")).map(|_| l[9..].trim().to_string()))?;
    let http = crate::network::http::blocking_timeout(Duration::from_secs(3));
    let desc = http.get(&location).send().ok()?.text().ok()?;
    let service_type = if desc.contains("WANIPConnection:2") { "urn:schemas-upnp-org:service:WANIPConnection:2" } else { "urn:schemas-upnp-org:service:WANIPConnection:1" };
    let control = {
        let at = desc.find(service_type)?;
        let tail = &desc[at..];
        let c0 = tail.find("<controlURL>")? + "<controlURL>".len();
        let c1 = tail[c0..].find("</controlURL>")? + c0;
        tail[c0..c1].trim().to_string()
    };
    let base = {
        let after = location.find("://").map(|i| i + 3).unwrap_or(0);
        let end = location[after..].find('/').map(|i| after + i).unwrap_or(location.len());
        location[..end].to_string()
    };
    let control_url = if control.starts_with("http") { control } else { format!("{base}{control}") };
    let local = crate::network::udp::get_local_ip()?;
    let secs = lifetime.as_secs();
    let add = format!(
        "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><u:AddPortMapping xmlns:u=\"{service_type}\"><NewRemoteHost></NewRemoteHost><NewExternalPort>{port}</NewExternalPort><NewProtocol>UDP</NewProtocol><NewInternalPort>{port}</NewInternalPort><NewInternalClient>{local}</NewInternalClient><NewEnabled>1</NewEnabled><NewPortMappingDescription>photon</NewPortMappingDescription><NewLeaseDuration>{secs}</NewLeaseDuration></u:AddPortMapping></s:Body></s:Envelope>"
    );
    let ok = http
        .post(&control_url)
        .header("Content-Type", "text/xml; charset=\"utf-8\"")
        .header("SOAPAction", format!("\"{service_type}#AddPortMapping\""))
        .body(add)
        .send()
        .ok()?
        .status()
        .is_success();
    if !ok {
        return None;
    }
    let get_ext = format!(
        "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><u:GetExternalIPAddress xmlns:u=\"{service_type}\"></u:GetExternalIPAddress></s:Body></s:Envelope>"
    );
    let text = http
        .post(&control_url)
        .header("Content-Type", "text/xml; charset=\"utf-8\"")
        .header("SOAPAction", format!("\"{service_type}#GetExternalIPAddress\""))
        .body(get_ext)
        .send()
        .ok()?
        .text()
        .ok()?;
    let e0 = text.find("<NewExternalIPAddress>")? + "<NewExternalIPAddress>".len();
    let e1 = text[e0..].find("</NewExternalIPAddress>")? + e0;
    let ext_ip: Ipv4Addr = text[e0..e1].trim().parse().ok()?;
    if ext_ip.is_unspecified() || ext_ip.is_private() {
        return None;
    }
    // IGD leases of 0 mean permanent; we still renew on our cadence so a rebooted router re-learns us.
    Some(Mapping { external: SocketAddrV4::new(ext_ip, port), lifetime: if secs == 0 { Duration::from_secs(3600) } else { lifetime }, via: "UPnP" })
}

/// The mapping worker: ask once, post the external address as the reflexive seed, renew at half-life, stop when `stop` reads true. One thread per network epoch — the caller restarts it on a LAN move.
pub fn run(port: u16, stop: std::sync::Arc<std::sync::atomic::AtomicBool>) {
    use std::sync::atomic::Ordering;
    let Some(gw) = default_gateway() else {
        crate::log("PORTMAP: no default gateway to ask (cellular, or no route) — skipping");
        return;
    };
    let ask = Duration::from_secs(7200);
    let mut first = true;
    while !stop.load(Ordering::Relaxed) {
        match request(gw, port, ask) {
            Some(m) => {
                if first {
                    crate::logf!("PORTMAP: {} mapped {} → our UDP {} for {}s via gateway {} — the world reaches us here without a punch", m.via, m.external, port, m.lifetime.as_secs(), gw);
                } else {
                    crate::logf!("PORTMAP: {} renewed {} ({}s)", m.via, m.external, m.lifetime.as_secs());
                }
                first = false;
                super::post_reflexive_seed(SocketAddr::V4(m.external));
                // Renew at half-life, checking the stop flag every few seconds.
                let renew_at = std::time::Instant::now() + m.lifetime / 2;
                while std::time::Instant::now() < renew_at && !stop.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_secs(5));
                }
            }
            None => {
                if first {
                    crate::logf!("PORTMAP: gateway {} answered none of NAT-PMP / PCP / UPnP — no mapping this session", gw);
                    return;
                }
                crate::log("PORTMAP: renewal refused — the mapping lapses; a peer echo or FGTW re-seeds the address");
                return;
            }
        }
    }
}
