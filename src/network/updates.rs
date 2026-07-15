//! Self-update client (docs/updates.md): fetch the signed version manifest, decide currency, download + verify + apply.
//!
//! Two channels, two manifests on the same R2 the installers serve from: `manifest-release.vsf` (ONE version, every platform row, written by deploy.sh) and `manifest-dev.vsf` (per-platform rows — dev pushes are ad hoc per platform — each row rewritten by its scripts/publish/dev-*.sh). Both are COMPLETE VSF files signed by the release key ([`crate::crypto::self_verify::AUTHOR_PUBKEY`]); `read_verified(_, Some(AUTHOR_PUBKEY))` is the trust gate — an unsigned or wrong-signer manifest is noise, exactly like a bad binary.
//!
//! The apply path re-uses the binary self-verify: every published binary carries a 64-byte appended Ed25519 signature over BLAKE3(rest), so a download is verified ON DISK (`self_verify::verify_file`) before anything execs — plus the manifest's own BLAKE3 must match first. Desktop then swaps atomically and the app re-execs; Android hands the verified APK to the system installer (the OS owns package installs — that's the second click).

use std::path::PathBuf;

use vsf::VsfType;

/// This build's platform/arch id — the manifest row key. ARM is split per-arch (linux-arm64 ≠ linux-x86_64, mac intel ≠ apple silicon): every row names exactly one artefact.
pub const fn platform_id() -> &'static str {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "linux-x86_64"
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        "linux-arm64"
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "windows-x86_64"
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "macos-arm64"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "macos-intel"
    }
    #[cfg(target_os = "android")]
    {
        "android-arm64"
    }
    #[cfg(not(any(
        all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")),
        all(target_os = "windows", target_arch = "x86_64"),
        target_os = "macos",
        target_os = "android"
    )))]
    {
        "unsupported"
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Release,
    Dev,
}

impl Channel {
    pub fn manifest_url(self) -> &'static str {
        match self {
            Channel::Release => "https://brobdingnagian.holdmyoscilloscope.com/photon/manifest-release.vsf",
            Channel::Dev => "https://brobdingnagian.holdmyoscilloscope.com/photon/manifest-dev.vsf",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Channel::Release => "release",
            Channel::Dev => "dev",
        }
    }
}

/// One artefact row from the manifest.
#[derive(Clone, Debug)]
pub struct ManifestRow {
    pub platform: String,
    pub version: String,
    pub commit: String,
    pub url: String,
    pub hash: [u8; 32],
}

/// Fetch + verify + parse a channel's manifest. Trust = the file signature verifying against the RELEASE key — never the URL, never a filename.
pub fn fetch_manifest_blocking(channel: Channel) -> Result<Vec<ManifestRow>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let bytes = client
        .get(channel.manifest_url())
        .send()
        .map_err(|e| format!("manifest fetch: {e}"))?
        .error_for_status()
        .map_err(|e| format!("manifest fetch: {e}"))?
        .bytes()
        .map_err(|e| format!("manifest read: {e}"))?
        .to_vec();
    parse_manifest(&bytes)
}

/// Parse + signature-gate manifest bytes (public for the manifest tool's merge path + tests).
pub fn parse_manifest(bytes: &[u8]) -> Result<Vec<ManifestRow>, String> {
    let (header, header_end) =
        vsf::verification::read_verified(bytes, Some(crate::crypto::self_verify::AUTHOR_PUBKEY))
            .map_err(|e| format!("manifest verification: {e}"))?;
    let section = header
        .primary_section(bytes, header_end)
        .map_err(|e| format!("manifest section: {e}"))?;
    if section.name != "manifest" {
        return Err(format!("unexpected section {:?}", section.name));
    }
    let mut rows = Vec::new();
    for field in section.get_fields("artefact") {
        // Row shape: (x platform, x version, x commit, x url, hb hash) — a native multi-value field, no numbered names.
        let mut strings: Vec<&String> = Vec::new();
        let mut hash: Option<[u8; 32]> = None;
        for v in &field.values {
            match v {
                VsfType::x(s) => strings.push(s),
                VsfType::hb(h) if h.len() == 32 => hash = Some(h.as_slice().try_into().unwrap()),
                _ => {}
            }
        }
        if strings.len() >= 4 {
            if let Some(h) = hash {
                rows.push(ManifestRow {
                    platform: strings[0].clone(),
                    version: strings[1].clone(),
                    commit: strings[2].clone(),
                    url: strings[3].clone(),
                    hash: h,
                });
            }
        }
    }
    if rows.is_empty() {
        return Err("manifest carried no artefact rows".to_string());
    }
    Ok(rows)
}

/// The row for THIS build's platform, if the manifest carries one.
pub fn our_row(rows: &[ManifestRow]) -> Option<ManifestRow> {
    rows.iter().find(|r| r.platform == platform_id()).cloned()
}

/// Download an artefact to `dest`, then gate it twice: BLAKE3 against the signed manifest's hash, and (for desktop binaries) the appended Ed25519 self-signature on disk. Nothing execs unless both pass.
fn download_verified(row: &ManifestRow, dest: &PathBuf, check_binary_sig: bool) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let bytes = client
        .get(&row.url)
        .send()
        .map_err(|e| format!("artefact fetch: {e}"))?
        .error_for_status()
        .map_err(|e| format!("artefact fetch: {e}"))?
        .bytes()
        .map_err(|e| format!("artefact read: {e}"))?
        .to_vec();
    let got = blake3::hash(&bytes);
    if got.as_bytes() != &row.hash {
        return Err("artefact hash mismatch vs signed manifest".to_string());
    }
    std::fs::write(dest, &bytes).map_err(|e| format!("artefact write: {e}"))?;
    if check_binary_sig {
        crate::crypto::self_verify::verify_file(dest).map_err(|e| {
            let _ = std::fs::remove_file(dest);
            format!("binary signature: {e}")
        })?;
    }
    Ok(())
}

/// Desktop one-click apply: download next to the current exe, verify (hash + appended signature), swap atomically. Returns the exe path for the caller to re-exec into. Unix: rename() over the path is atomic and the running process keeps its open inode. Windows: the running exe is locked against overwrite but CAN be renamed aside — shuffle to .old (deleted on some future launch), place the new exe, done.
#[cfg(not(target_os = "android"))]
pub fn apply_desktop_blocking(row: &ManifestRow) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let staged = exe.with_extension("update-staged");
    download_verified(row, &staged, true)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("chmod: {e}"))?;
        std::fs::rename(&staged, &exe).map_err(|e| format!("swap: {e}"))?;
    }
    #[cfg(windows)]
    {
        let old = exe.with_extension("old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&exe, &old).map_err(|e| format!("shuffle aside: {e}"))?;
        if let Err(e) = std::fs::rename(&staged, &exe) {
            // Roll back so the install stays runnable.
            let _ = std::fs::rename(&old, &exe);
            return Err(format!("swap: {e}"));
        }
    }
    crate::log(&format!(
        "UPDATE: applied {} {} ({}) — re-exec pending",
        row.platform, row.version, row.commit
    ));
    Ok(exe)
}

/// Sweep a leftover `.old` from a prior Windows swap (call once at startup; no-op elsewhere/absent).
pub fn sweep_old_binary() {
    if let Ok(exe) = std::env::current_exe() {
        let old = exe.with_extension("old");
        if old.exists() {
            let _ = std::fs::remove_file(&old);
        }
    }
}

/// Android: download + hash-verify the APK into the app's files dir and return its path — the caller hands it to the system installer (the second click). No appended-signature check: APKs are signed by the Android keystore and verified by the OS installer; integrity here = the BLAKE3 from the SIGNED manifest.
#[cfg(target_os = "android")]
pub fn download_apk_blocking(row: &ManifestRow) -> Result<PathBuf, String> {
    let dir = kete::android_vault_dirs()
        .map(|(files, _)| files)
        .ok_or("android files dir not wired")?;
    let dest = std::path::Path::new(&dir).join("photon-update.apk");
    download_verified(row, &dest, false)?;
    Ok(dest)
}
