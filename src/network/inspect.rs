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

// =============================================================================
// Centralized VSF disk I/O with automatic dev-mode inspection
// =============================================================================

use ed25519_dalek::SigningKey;
use std::path::Path;
use vsf::VsfBuilder;
use vsf::VsfType;

/// Write encrypted VSF data to disk as a complete, signed VSF file
///
/// Wraps the encrypted payload in a proper VSF file with:
/// - VSF header with magic, version, timestamp
/// - Provenance hash for content integrity
/// - Ed25519 signature from the device key (signs file hash)
/// - The encrypted payload as a v(b'e', ...) field
///
/// In development mode, logs both the encrypted payload and decrypted content.
pub fn vsf_write(
    path: &Path,
    encrypted: &[u8],
    label: &str,
    #[allow(unused_variables)] decrypted: Option<&[u8]>,
    device_secret: &[u8; 32],
) -> std::io::Result<()> {
    #[cfg(feature = "development")]
    crate::log(&format!("STORAGE: vsf_write: start {}", label));

    // Derive device pubkey
    let signing_key = SigningKey::from_bytes(device_secret);
    let device_pubkey = signing_key.verifying_key();

    #[cfg(feature = "development")]
    crate::log("STORAGE: vsf_write: building unsigned VSF...");

    // Build VSF file with placeholder signature (zeros) - sign_file will fill it
    let unsigned_vsf = VsfBuilder::new()
        .creation_time_nanos(vsf::eagle_time_nanos())
        .signature_ed25519(*device_pubkey.as_bytes(), [0u8; 64]) // Placeholder
        .add_section(
            "encrypted",
            vec![("payload".to_string(), VsfType::v(b'e', encrypted.to_vec()))],
        )
        .build()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    #[cfg(feature = "development")]
    crate::log("STORAGE: vsf_write: signing VSF...");

    // Sign properly using VSF library (signs file hash, fills hp and ge)
    let vsf_file = vsf::verification::sign_file(unsigned_vsf, device_secret)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    #[cfg(feature = "development")]
    crate::log("STORAGE: vsf_write: signed");

    #[cfg(feature = "development")]
    crate::log(&format!(
        "STORAGE: vsf_write: {} file_len={}",
        label,
        vsf_file.len()
    ));

    #[cfg(feature = "development")]
    crate::log(&format!("STORAGE: vsf_write: writing to {:?}", path));

    std::fs::write(path, &vsf_file)?;

    #[cfg(feature = "development")]
    crate::log("STORAGE: vsf_write: write complete");

    Ok(())
}

/// Read encrypted VSF data from disk with signature verification
///
/// Uses VSF library's verify_file_signature to verify integrity, then extracts payload.
///
/// Returns the encrypted bytes. Caller is responsible for decryption.
/// After decryption, call `vsf_read_decrypted` to log the decrypted content.
pub fn vsf_read(path: &Path, label: &str, device_secret: &[u8; 32]) -> std::io::Result<Vec<u8>> {
    let file_bytes = std::fs::read(path)?;

    #[cfg(feature = "development")]
    {
        let msg = vsf_inspect(&file_bytes, "Disk", "Read", label);
        if !msg.is_empty() {
            println!("{}", msg);
        }
    }

    // Verify signature using VSF library
    match vsf::verification::verify_file_signature(&file_bytes) {
        Ok(true) => {} // Signature valid
        Ok(false) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}: Signature verification failed", label),
            ));
        }
        Err(e) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}: {}", label, e),
            ));
        }
    }

    // Verify file was signed by our device
    let signing_key = SigningKey::from_bytes(device_secret);
    let expected_pubkey = signing_key.verifying_key();
    let file_pubkey = vsf::verification::extract_signer_pubkey(&file_bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    if file_pubkey != *expected_pubkey.as_bytes() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "{}: Signed by different device (expected {}, got {})",
                label,
                hex::encode(&expected_pubkey.as_bytes()[..8]),
                hex::encode(&file_pubkey[..8])
            ),
        ));
    }

    // Parse to extract encrypted payload
    let (header, header_len) = vsf::VsfHeader::decode(&file_bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{:?}", e)))?;

    let mut ptr = header_len;
    let section = vsf::VsfSection::parse(&file_bytes, &mut ptr)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{:?}", e)))?;

    for field in &section.fields {
        if field.name == "payload" {
            if let Some(VsfType::v(b'e', data)) = field.values.first() {
                #[cfg(feature = "development")]
                crate::log(&format!(
                    "STORAGE: vsf_read: {} verified, payload_len={}",
                    label,
                    data.len()
                ));
                return Ok(data.clone());
            }
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("{}: No encrypted payload", label),
    ))
}

/// Log decrypted VSF content after reading (call after successful decryption)
///
/// This is separate from vsf_read because decryption happens in the caller.
#[cfg(feature = "development")]
pub fn vsf_read_decrypted(decrypted: &[u8], label: &str) {
    let msg = section_inspect(decrypted, "Disk", "Read", &format!("{} (decrypted)", label));
    if !msg.is_empty() {
        println!("{}", msg);
    }
}
