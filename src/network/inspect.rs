//! VSF packet inspection and logging
//!
//! Centralized packet inspection for all network transports (UDP, TCP, PT).
//! Provides human-readable VSF packet formatting with optional noise filtering.

/// Format a VSF packet as a human-readable inspection string (like vsfinfo)
/// Public for use across network modules (FGTW transport, P2P, etc.)
/// Returns empty string for noisy packets (ping/pong/lan_discovery) unless verbose-network is enabled
///
/// Title format: `═══ VSF {transport} {TX|RX} {addr} ({bytes} bytes) ═══`
/// Section names come from the VSF data itself (shown in parsed output below title)
#[cfg(feature = "development")]
pub fn vsf_inspect(data: &[u8], transport: &str, direction: &str, addr: &str) -> String {
    // PT DATA packets: stream_id ('a'-'z') + varint seq + payload - filter unless verbose-network
    #[cfg(not(feature = "verbose-network"))]
    if data.len() > 1 && (b'a'..=b'z').contains(&data[0]) {
        return String::new();
    }

    // Filter noisy packet types unless verbose-network is enabled
    #[cfg(not(feature = "verbose-network"))]
    {
        if is_noisy_packet(data) {
            return String::new();
        }
    }

    let mut result = format!(
        "═══ VSF {} {} {} ({} bytes) ═══\n",
        transport,
        direction,
        addr,
        data.len()
    );

    // Try to parse as VSF file first, then section, fall back to hex dump
    match vsf::inspect::inspect_vsf(data) {
        Ok(formatted) => result.push_str(&strip_ansi_if_needed(&formatted)),
        Err(_) => {
            // Not a complete VSF file - try section format
            match vsf::inspect::inspect_section(data) {
                Ok(formatted) => result.push_str(&strip_ansi_if_needed(&formatted)),
                Err(_) => {
                    // Fall back to hex dump
                    result.push_str(&vsf::inspect::hex_dump(data));
                }
            }
        }
    }

    result
}

/// Decode VSF variable-length uint (for PT DATA sequence numbers)
#[cfg(feature = "development")]
fn decode_vsf_varint(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut value: usize = 0;
    let mut shift = 0;

    for (i, &byte) in bytes.iter().enumerate() {
        value |= ((byte & 0x7F) as usize) << shift;
        shift += 7;

        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }

        if shift >= 32 {
            return None;
        }
    }

    None
}

/// Noisy packet types that should be filtered from inspection logs
/// These match VSF header label names (e.g., `(ping)`) and section names
const NOISY_SECTION_NAMES: &[&str] = &[
    "ping",
    "pong",
    "lan_discovery",
    "pt_spec",
    "pt_ack",
    "pt_nak",
    "pt_ctrl",
    "pt_done",
];

/// Check if a VSF packet is a noisy type that should be filtered
/// Filters: ping/pong, lan_discovery, PT control packets (ack/nak/ctrl/done/spec)
#[cfg(feature = "development")]
pub fn is_noisy_packet(data: &[u8]) -> bool {
    if data.is_empty() {
        return false;
    }

    // PT DATA packets start with stream_id 'a'-'z' - already handled separately, but filter here too
    if (b'a'..=b'z').contains(&data[0]) {
        return true;
    }

    // Check for VSF file format with header (starts with "RÅ<" magic)
    if data.len() > 10 && &data[0..3] == "RÅ".as_bytes() && data[3] == b'<' {
        // Parse the VSF header to get field names
        if let Ok((header, _)) = vsf::VsfHeader::decode(data) {
            // Check if any header field name is noisy
            for field in &header.fields {
                if NOISY_SECTION_NAMES.contains(&field.name.as_str()) {
                    return true;
                }
            }
        }
        return false;
    }

    // Check for VSF section format - starts with '['
    if data[0] == b'[' {
        // Parse just enough to get section type
        let mut ptr = 0;
        if let Ok(section) = vsf::VsfSection::parse(data, &mut ptr) {
            return NOISY_SECTION_NAMES.contains(&section.name.as_str());
        }
    }

    false
}

/// Format a raw VSF section (no header) as a human-readable inspection string
/// Used for decrypted section-only data like FGTW peer lists
#[cfg(feature = "development")]
pub fn section_inspect(data: &[u8], transport: &str, direction: &str, label: &str) -> String {
    let mut result = format!(
        "═══ VSF {} {} {} ({} bytes) ═══\n",
        transport,
        direction,
        label,
        data.len()
    );

    match vsf::inspect::inspect_section(data) {
        Ok(formatted) => result.push_str(&strip_ansi_if_needed(&formatted)),
        Err(_) => {
            // Fall back to hex dump
            result.push_str(&vsf::inspect::hex_dump(data));
        }
    }

    result
}

/// Strip ANSI color codes on platforms that don't support them (Android)
#[cfg(feature = "development")]
fn strip_ansi_if_needed(s: &str) -> String {
    #[cfg(target_os = "android")]
    {
        // Strip ANSI escape sequences on Android
        let mut result = String::with_capacity(s.len());
        let mut in_escape = false;
        for c in s.chars() {
            if c == '\x1b' {
                in_escape = true;
            } else if in_escape {
                if c == 'm' {
                    in_escape = false;
                }
            } else {
                result.push(c);
            }
        }
        result
    }
    #[cfg(not(target_os = "android"))]
    {
        s.to_string()
    }
}
