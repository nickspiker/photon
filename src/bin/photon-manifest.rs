//! Build + sign the update version manifest (docs/updates.md) — invoked by deploy.sh (release channel, every platform at once) and by each scripts/publish/dev-*.sh (dev channel, merging its ONE platform section into the fetched current manifest).
//!
//! Output is a COMPLETE VSF file signed by the release key (the same key that signs binaries): ONE SECTION PER ARTEFACT, each named `manifest.photon.<release|development>`, every field NAMED and semantically TYPED — `platform: x`, `arch: x`, `major/minor/patch: z` (native version numbers; `major` omitted while 0, `patch` omitted when 0 so its presence means "dev build"), `commit: hs` (full 20-byte git SHA-1, raw), `url: nu`, `hash: hb`. No numbered field names, no positional values, no digit strings.
//!
//! Usage:
//!   photon-manifest --channel development --out manifest-dev.vsf \
//!       [--merge existing-manifest.vsf] \
//!       --artefact <platform> <arch> <major.minor.patch> <commit-sha1-hex40> <url> <blake3-hex64> [--artefact ...]
//!
//! `--merge` keeps every section of the existing (signature-verified) manifest whose (platform, arch) is NOT re-supplied — how a single-platform dev publish updates its section without clobbering the others. The signing key comes from $PHOTON_SIGNING_KEY (same contract as photon-signature-signer).

use photon_messenger::network::updates::{parse_manifest, Channel, ManifestRow};
use vsf::VsfType;

fn fail(msg: &str) -> ! {
    eprintln!("photon-manifest: {msg}");
    std::process::exit(1);
}

fn main() {
    let mut channel: Option<String> = None;
    let mut out: Option<String> = None;
    let mut merge: Option<String> = None;
    let mut rows: Vec<ManifestRow> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--channel" => channel = args.next(),
            "--out" => out = args.next(),
            "--merge" => merge = args.next(),
            "--artefact" => {
                let (Some(platform), Some(arch), Some(version), Some(commit_hex), Some(url), Some(hash_hex)) =
                    (args.next(), args.next(), args.next(), args.next(), args.next(), args.next())
                else {
                    fail("--artefact needs 6 values: platform arch major.minor.patch commit-sha1-hex url blake3-hex");
                };
                let mut v = version.split('.').map(|p| p.parse::<usize>());
                let (Some(Ok(major)), Some(Ok(minor)), Some(Ok(patch)), None) = (v.next(), v.next(), v.next(), v.next())
                else {
                    fail("--artefact version must be major.minor.patch");
                };
                let commit = hex::decode(&commit_hex).unwrap_or_default();
                if commit.len() != 20 {
                    fail("--artefact commit must be the FULL 40-hex-char git SHA-1 (20 bytes)");
                }
                let hash_v = hex::decode(&hash_hex).unwrap_or_default();
                let hash: [u8; 32] = match hash_v.as_slice().try_into() {
                    Ok(h) => h,
                    Err(_) => fail("--artefact hash must be 64 hex chars (BLAKE3)"),
                };
                rows.push(ManifestRow { platform, arch, version: (major, minor, patch), commit, url, hash });
            }
            other => fail(&format!("unknown arg {other}")),
        }
    }
    let channel = match channel.as_deref() {
        Some("release") => Channel::Release,
        Some("development") | Some("dev") => Channel::Dev,
        _ => fail("--channel release|development required"),
    };
    let out = out.unwrap_or_else(|| fail("--out required"));
    if rows.is_empty() {
        fail("at least one --artefact required");
    }

    // Merge: carry forward every existing section whose (platform, arch) isn't being re-supplied. The existing file must verify (same trust gate as the client) — a corrupt/tampered current manifest is dropped, not merged.
    if let Some(path) = merge {
        match std::fs::read(&path) {
            Ok(bytes) => match parse_manifest(&bytes, channel) {
                Ok(existing) => {
                    for row in existing {
                        if !rows.iter().any(|r| r.platform == row.platform && r.arch == row.arch) {
                            rows.push(row);
                        }
                    }
                }
                Err(e) => eprintln!("photon-manifest: merge source unverifiable ({e}) — starting fresh"),
            },
            Err(e) => eprintln!("photon-manifest: merge source unreadable ({e}) — starting fresh"),
        }
    }
    rows.sort_by(|a, b| (&a.platform, &a.arch).cmp(&(&b.platform, &b.arch)));

    // Signing key: $PHOTON_SIGNING_KEY (the release key file, same as photon-signature-signer).
    let key_path = std::env::var("PHOTON_SIGNING_KEY")
        .unwrap_or_else(|_| fail("PHOTON_SIGNING_KEY env var required (release signing key file)"));
    let key_bytes = std::fs::read(&key_path).unwrap_or_else(|e| fail(&format!("key read: {e}")));
    let key: [u8; 32] = match key_bytes.as_slice().try_into() {
        Ok(k) => k,
        Err(_) => fail("signing key file must be exactly 32 bytes"),
    };
    // sign_file attaches the `ge` signature to the `ke` (signer pubkey) already in the header — so declare it, same as fgtw's signed_req.
    let signer_pub = ed25519_dalek::SigningKey::from_bytes(&key)
        .verifying_key()
        .to_bytes()
        .to_vec();

    let mut builder = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .signed_only(VsfType::ke(signer_pub));
    for r in &rows {
        // One section per artefact; absent-when-zero for major (uncounted) and patch (presence = dev build).
        let mut section = vsf::VsfSection::new(channel.section_name());
        section.add_field_multi("platform", vec![VsfType::x(r.platform.clone())]);
        section.add_field_multi("arch", vec![VsfType::x(r.arch.clone())]);
        let (major, minor, patch) = r.version;
        if major != 0 {
            section.add_field_multi("major", vec![VsfType::z(major)]);
        }
        section.add_field_multi("minor", vec![VsfType::z(minor)]);
        if patch != 0 {
            section.add_field_multi("patch", vec![VsfType::z(patch)]);
        }
        section.add_field_multi("commit", vec![VsfType::hs(r.commit.clone())]);
        section.add_field_multi("url", vec![VsfType::nu(r.url.clone())]);
        section.add_field_multi("hash", vec![VsfType::hb(r.hash.to_vec())]);
        builder = builder.add_section_direct(section);
    }
    let unsigned = builder.build().unwrap_or_else(|e| fail(&format!("build: {e}")));
    let signed = vsf::verification::sign_file(unsigned, &key)
        .unwrap_or_else(|e| fail(&format!("sign: {e}")));

    // Round-trip gate before publish: what we just wrote must pass the exact client-side check.
    match parse_manifest(&signed, channel) {
        Ok(parsed) if parsed.len() == rows.len() => {}
        Ok(parsed) => fail(&format!("self-check row count mismatch: wrote {}, parsed {}", rows.len(), parsed.len())),
        Err(e) => fail(&format!("self-check failed: {e}")),
    }
    std::fs::write(&out, &signed).unwrap_or_else(|e| fail(&format!("write: {e}")));
    println!(
        "photon-manifest: wrote {out} ({} artefact sections, channel {})",
        rows.len(),
        channel.label()
    );
}
