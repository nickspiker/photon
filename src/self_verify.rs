///! Binary self-verification using appended BLAKE3 hash
///!
///! All binaries MUST have a 32-byte BLAKE3 hash appended after build.
///! On startup, we verify the hash matches to detect tampering.
///! Missing or invalid hash = FAIL.

/// Verify that this binary has a valid hash
///
/// Returns Ok(()) ONLY if hash is present and valid
/// Returns Err for any other condition (missing hash, tampered binary)
pub fn verify_binary_hash() -> Result<(), String> {
    // Read our own executable
    let exe_path = std::env::current_exe()
        .map_err(|e| format!("Failed to get executable path: {}", e))?;

    let mut exe_data = std::fs::read(&exe_path)
        .map_err(|e| format!("Failed to read executable: {}", e))?;

    // Check if binary has hash appended (last 32 bytes)
    if exe_data.len() < 32 {
        return Err("Binary too small - hash verification failed!".to_string());
    }

    // Extract appended hash (last 32 bytes)
    let appended_hash = exe_data.split_off(exe_data.len() - 32);

    // Check if it looks like zeros (hash was stripped or never added)
    if appended_hash.iter().all(|&b| b == 0) {
        return Err("Binary hash missing - executable must be built with hash-release!".to_string());
    }

    // Hash the binary (without the appended hash)
    let computed_hash = blake3::hash(&exe_data);

    // Compare hashes
    if computed_hash.as_bytes() != appended_hash.as_slice() {
        return Err("Binary hash mismatch - executable has been tampered with!".to_string());
    }

    Ok(())
}
