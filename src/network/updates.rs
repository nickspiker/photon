//! Self-update client (docs/updates.md): fetch the signed version manifest, decide currency, download + verify + apply.
//!
//! Two channels, two manifests on the same R2 the installers serve from: `manifest-release.vsf` (ONE version, every platform section, written by deploy.sh) and `manifest-dev.vsf` (per-platform sections — dev pushes are ad hoc per platform — each rewritten by its scripts/publish/dev-*.sh). Both are COMPLETE VSF files signed by the release key ([`crate::crypto::self_verify::AUTHOR_PUBKEY`]); `read_verified(_, Some(AUTHOR_PUBKEY))` is the trust gate — an unsigned or wrong-signer manifest is noise, exactly like a bad binary.
//!
//! Manifest shape (agreed 2026-07-16): ONE SECTION PER ARTEFACT, each named `manifest.photon.<channel>` (the app + channel scoped in the label itself), fields all NAMED and semantically TYPED — `platform: x`, `arch: x`, `major/minor/patch: z` (native version numbers, no arabic digit strings; `major` omitted while 0, `patch` omitted on releases so its PRESENCE means "dev build"), `commit: hs` (the full 20-byte git SHA-1, raw), `url: nu` (VSF's network-URL type), `hash: hb` (BLAKE3 of the artefact). No positional parsing, no numbered field names.
//!
//! The apply path re-uses the binary self-verify: every published binary carries a 64-byte appended Ed25519 signature over BLAKE3(rest), so a download is verified ON DISK (`self_verify::verify_file`) before anything execs — plus the manifest's own BLAKE3 must match first. Desktop then swaps atomically and the app re-execs; Android hands the verified APK to the system installer (the OS owns package installs — that's the second click).

use std::path::PathBuf;

use vsf::VsfType;

/// This build's platform + arch — the manifest section keys. ARM is split per-arch (linux/arm64 ≠ linux/x86_64, mac intel ≠ apple silicon): every section names exactly one artefact.
pub const fn our_platform() -> (&'static str, &'static str) {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        ("Linux", "x86_64")
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        ("Linux", "arm64")
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        ("Windows", "x86_64")
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        ("macOS", "arm64")
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        ("macOS", "x86_64")
    }
    #[cfg(target_os = "android")]
    {
        ("Android", "arm64")
    }
    #[cfg(not(any(
        all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")),
        all(target_os = "windows", target_arch = "x86_64"),
        target_os = "macos",
        target_os = "android"
    )))]
    {
        ("unsupported", "unsupported")
    }
}

/// Human-readable platform id for status lines/logs.
pub fn platform_id() -> String {
    let (p, a) = our_platform();
    format!("{p}/{a}")
}

/// This build's (major, minor, patch) as numbers.
pub fn our_version() -> (usize, usize, usize) {
    (
        env!("CARGO_PKG_VERSION_MAJOR").parse().unwrap_or(0),
        env!("CARGO_PKG_VERSION_MINOR").parse().unwrap_or(0),
        env!("CARGO_PKG_VERSION_PATCH").parse().unwrap_or(0),
    )
}

/// The stamp-window FLOOR: eagle-time oscillations at the moment this binary was built (build.rs). Compiled in — it advances only by exec'ing into a newer build, never by mutable stored state (docs/updates.md).
pub fn build_stamp_osc() -> i64 {
    env!("PHOTON_BUILD_STAMP").parse().unwrap_or(0)
}

/// Stamp-window verdict for the AUTOMATIC update path (docs/updates.md, reconciled to the counter version scheme: `t` = the signed manifest's creation stamp, the floor = this binary's build stamp; forward-only ordering itself rides the version tuple).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StampVerdict {
    /// `floor < t ≤ now` — the manifest is fresher than this build and not from the future.
    Accept,
    /// `t ≤ floor` — a manifest at or before this build's own moment: a replay or stale edge. Discard.
    Stale,
    /// `t > now` — forward-dated. Not an error: "not yet" — defer and re-evaluate on the next poll (with the honest clock consulted before concluding).
    ForwardDated,
}

/// Evaluate the stamp window against a caller-supplied `now` (the staged clock: system eagle time on the happy path, the nunc-adjusted conservative edge when the check fails forward).
pub fn stamp_window(manifest_stamp_osc: i64, now_osc: i64) -> StampVerdict {
    if manifest_stamp_osc <= build_stamp_osc() {
        StampVerdict::Stale
    } else if manifest_stamp_osc > now_osc {
        StampVerdict::ForwardDated
    } else {
        StampVerdict::Accept
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
    /// The per-artefact section name: app + channel scoped in the dictionary label itself.
    pub fn section_name(self) -> &'static str {
        match self {
            Channel::Release => "manifest.photon.release",
            Channel::Dev => "manifest.photon.development",
        }
    }
}

/// One artefact section from the manifest.
#[derive(Clone, Debug)]
pub struct ManifestRow {
    pub platform: String,
    pub arch: String,
    /// (major, minor, patch) — absent fields parse as 0, so a release is (0, Y, 0) until major exists.
    pub version: (usize, usize, usize),
    /// Full git commit (20-byte SHA-1), raw.
    pub commit: Vec<u8>,
    pub url: String,
    pub hash: [u8; 32],
}

impl ManifestRow {
    /// Display form of the version: `major.minor.patch` with the same omissions the wire uses.
    pub fn version_string(&self) -> String {
        let (maj, min, pat) = self.version;
        format!("{maj}.{min}.{pat}")
    }
}

/// Fetch + verify + parse a channel's manifest. Trust = the file signature verifying against the RELEASE key — never the URL, never a filename.
pub fn fetch_manifest_blocking(channel: Channel) -> Result<Vec<ManifestRow>, String> {
    fetch_manifest_stamped_blocking(channel).map(|(_, rows)| rows)
}

/// [`fetch_manifest_blocking`] plus the SIGNED header's creation stamp (eagle-time oscillations) — the `t` of the stamp window. 0 if the header carries no timestamp (treated as maximally stale by [`stamp_window`]).
pub fn fetch_manifest_stamped_blocking(channel: Channel) -> Result<(i64, Vec<ManifestRow>), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    // Cache-bust the manifest too: it's re-signed on every publish under a fixed url, so a stale CDN edge would hide a fresh release. A random query + no-cache forces a revalidated fetch; the signature is re-verified regardless, so a bad edge can only ever mean "not yet", never "wrong".
    let url = format!("{}?v={}", channel.manifest_url(), rand::random::<u64>());
    let bytes = client
        .get(&url)
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .send()
        .map_err(|e| format!("manifest fetch: {e}"))?
        .error_for_status()
        .map_err(|e| format!("manifest fetch: {e}"))?
        .bytes()
        .map_err(|e| format!("manifest read: {e}"))?
        .to_vec();
    parse_manifest_stamped(&bytes, channel)
}

/// Parse + signature-gate manifest bytes: every section named `manifest.photon.<channel>` is one artefact, fields matched by NAME + TYPE (never position). Public for the manifest tool's merge path + tests.
pub fn parse_manifest(bytes: &[u8], channel: Channel) -> Result<Vec<ManifestRow>, String> {
    parse_manifest_stamped(bytes, channel).map(|(_, rows)| rows)
}

/// [`parse_manifest`] plus the verified header's creation stamp — `t` for the stamp window. The stamp is INSIDE the signature (the whole header is signed), so it's as trustworthy as the rows.
pub fn parse_manifest_stamped(bytes: &[u8], channel: Channel) -> Result<(i64, Vec<ManifestRow>), String> {
    let (header, header_end) =
        vsf::verification::read_verified(bytes, Some(crate::crypto::self_verify::AUTHOR_PUBKEY))
            .map_err(|e| format!("manifest verification: {e}"))?;
    let stamp_osc = match &header.creation_time {
        Some(VsfType::e(vsf::types::EtType::e6(osc))) => *osc,
        _ => 0,
    };
    let sections = header
        .sections(bytes, header_end)
        .map_err(|e| format!("manifest sections: {e}"))?;
    let mut rows = Vec::new();
    for section in &sections {
        if section.name != channel.section_name() {
            continue;
        }
        // Named single-value fields; absent numeric = 0 (major while uncounted, patch on releases).
        let text = |name: &str| -> Option<String> {
            section.get_fields(name).first().and_then(|f| f.values.first()).and_then(|v| match v {
                VsfType::x(s) => Some(s.clone()),
                VsfType::nu(s) => Some(s.clone()),
                _ => None,
            })
        };
        let num = |name: &str| -> usize {
            section
                .get_fields(name)
                .first()
                .and_then(|f| f.values.first())
                .and_then(|v| match v {
                    VsfType::z(n) => Some(*n),
                    VsfType::u(n, _) => Some(*n),
                    _ => None,
                })
                .unwrap_or(0)
        };
        let hash: Option<[u8; 32]> = section.get_fields("hash").first().and_then(|f| f.values.first()).and_then(|v| match v {
            VsfType::hb(h) if h.len() == 32 => h.as_slice().try_into().ok(),
            _ => None,
        });
        let commit: Vec<u8> = section
            .get_fields("commit")
            .first()
            .and_then(|f| f.values.first())
            .and_then(|v| match v {
                VsfType::hs(c) => Some(c.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let (Some(platform), Some(arch), Some(url), Some(hash)) =
            (text("platform"), text("arch"), text("url"), hash)
        else {
            continue;
        };
        rows.push(ManifestRow {
            platform,
            arch,
            version: (num("major"), num("minor"), num("patch")),
            commit,
            url,
            hash,
        });
    }
    if rows.is_empty() {
        return Err("manifest carried no artefact sections for this channel".to_string());
    }
    Ok((stamp_osc, rows))
}

/// The section for THIS build's platform + arch, if the manifest carries one.
pub fn our_row(rows: &[ManifestRow]) -> Option<ManifestRow> {
    let (p, a) = our_platform();
    rows.iter().find(|r| r.platform == p && r.arch == a).cloned()
}

/// Download an artefact to `dest`, then gate it twice: BLAKE3 against the signed manifest's hash, and (for desktop binaries) the appended Ed25519 self-signature on disk. Nothing execs unless both pass. `progress(done, total)` fires as chunks stream in (total = 0 when the server sent no length) — the Updates page renders it as the download bar.
fn download_verified(
    row: &ManifestRow,
    dest: &PathBuf,
    check_binary_sig: bool,
    progress: &dyn Fn(u64, u64),
) -> Result<(), String> {
    use std::io::Read;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    // Cache-bust by the CONTENT HASH: the artefact lives at a FIXED url per platform, so after a new build is uploaded the CDN (Cloudflare) can keep serving the stale cached binary while the freshly-fetched manifest already carries the new hash — the "hash mismatch vs signed manifest" a Windows update hit (2026-07-17). `?v=<hash>` makes the URL change exactly when the content does: a new version misses the cache (fresh fetch), an unchanged one stays cacheable. The hash is the integrity anchor either way; this only defeats stale edges.
    let url = format!("{}?v={}", row.url, hex::encode(row.hash));
    let mut resp = client
        .get(&url)
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .send()
        .map_err(|e| format!("artefact fetch: {e}"))?
        .error_for_status()
        .map_err(|e| format!("artefact fetch: {e}"))?;
    let total = resp.content_length().unwrap_or(0);
    let mut bytes: Vec<u8> = Vec::with_capacity(total as usize);
    let mut chunk = vec![0u8; 1 << 16];
    loop {
        let n = resp.read(&mut chunk).map_err(|e| format!("artefact read: {e}"))?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..n]);
        progress(bytes.len() as u64, total);
    }
    let got = blake3::hash(&bytes);
    if got.as_bytes() != &row.hash {
        return Err(format!(
            "downloaded {} bytes but the hash didn't match the signed manifest (stale CDN copy?) — nothing installed, running version untouched; retry shortly",
            bytes.len()
        ));
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
pub fn apply_desktop_blocking(row: &ManifestRow, progress: &dyn Fn(u64, u64)) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let staged = exe.with_extension("update-staged");
    download_verified(row, &staged, true, progress)?;
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
    crate::logf!("UPDATE: applied {}/{} {} ({}) — re-exec pending", row.platform, row.arch, row.version_string(), hex::encode(&row.commit));
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
pub fn download_apk_blocking(row: &ManifestRow, progress: &dyn Fn(u64, u64)) -> Result<PathBuf, String> {
    let dir = kete::android_vault_dirs()
        .map(|(files, _)| files)
        .ok_or("android files dir not wired")?;
    let dest = std::path::Path::new(&dir).join("photon-update.apk");
    download_verified(row, &dest, false, progress)?;
    Ok(dest)
}
