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

/// Check if a VSF packet is a noisy type (ping/pong/lan_discovery) that should be filtered
#[cfg(feature = "development")]
pub fn is_noisy_packet(data: &[u8]) -> bool {
    // Quick check for VSF section header - look for section type in first bytes
    // VSF sections start with '[' followed by type field
    if data.is_empty() || data[0] != b'[' {
        return false;
    }

    // Parse just enough to get section type
    let mut ptr = 0;
    if let Ok(section) = vsf::VsfSection::parse(data, &mut ptr) {
        return section.name == "status_ping"
            || section.name == "status_pong"
            || section.name == "lan_discovery";
    }

    false
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
