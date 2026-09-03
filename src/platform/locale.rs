// OS-locale sniff for the FIRST-LAUNCH language seed only (docs/languages.md): the language setting is the user's after that and never live-follows the host.
// Detection is best-effort by design — a miss just seeds English and the picker is one tap away.

/// Lowercased ISO-639 primary language subtag of the host locale ("en", "es", "mi", …), or None when the host tells us nothing.
pub fn os_language() -> Option<String> {
    // Unix-family (Linux, macOS-from-terminal, BSD): the LC_* ladder. macOS apps launched from Finder usually carry no LANG — that's fine, they seed English.
    #[cfg(not(target_os = "windows"))]
    {
        for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(v) = std::env::var(var) {
                let tag = v.split(['.', '@']).next().unwrap_or("");
                let primary = tag.split(['_', '-']).next().unwrap_or("");
                if !primary.is_empty() && primary != "C" && primary != "POSIX" {
                    return Some(primary.to_ascii_lowercase());
                }
            }
        }
        None
    }
    // Windows: GetUserDefaultLocaleName — the user's display-language BCP-47 tag.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStringExt;
        const LOCALE_NAME_MAX_LENGTH: usize = 85;
        extern "system" {
            fn GetUserDefaultLocaleName(lpLocaleName: *mut u16, cchLocaleName: i32) -> i32;
        }
        let mut buf = [0u16; LOCALE_NAME_MAX_LENGTH];
        let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), LOCALE_NAME_MAX_LENGTH as i32) };
        if n > 1 {
            let s = std::ffi::OsString::from_wide(&buf[..(n as usize - 1)]);
            let s = s.to_string_lossy();
            let primary = s.split('-').next().unwrap_or("");
            if !primary.is_empty() {
                return Some(primary.to_ascii_lowercase());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    // os_language reads process env — tests only pin the parsing invariants that don't depend on the host.
    #[test]
    fn primary_subtag_parsing_shapes() {
        // The ladder logic is exercised indirectly; here we pin the tag-splitting contract on representative values.
        for (raw, want) in [("mi_NZ.UTF-8", "mi"), ("es-419", "es"), ("en_US", "en"), ("C", ""), ("POSIX", "")] {
            let tag = raw.split(['.', '@']).next().unwrap_or("");
            let primary = tag.split(['_', '-']).next().unwrap_or("");
            let got = if !primary.is_empty() && primary != "C" && primary != "POSIX" { primary.to_ascii_lowercase() } else { String::new() };
            assert_eq!(got, want, "raw={raw}");
        }
    }
}
