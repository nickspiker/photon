//! Build + sign the update version manifest (docs/updates.md) — invoked by deploy.sh (release channel, every platform at once) and by each scripts/release/publish/dev-*.sh (dev channel, merging its ONE platform section into the fetched current manifest).
//!
//! Output is a COMPLETE VSF file signed by the release key (the same key that signs binaries): ONE SECTION PER ARTEFACT, each named `manifest.photon.<release|development>`, every field NAMED and semantically TYPED — `platform: x`, `arch: x`, `major/minor/patch: z` (native version numbers; `major` omitted while 0, `patch` omitted when 0 so its presence means "dev build"), `commit: hs` (full 20-byte git SHA-1, raw), `url: nu`, `hash: hb`. No numbered field names, no positional values, no digit strings.
//!
//! Usage:
//!   photon-manifest --release-notice --out notice.vsf   (the signed "a release is out" document deploy.sh posts to fgtw.org)
//!   photon-manifest --print-release-bundle   (the release key's public bundle, every scheme, as hex: what fgtw::release pins)
//!   photon-manifest --check-capsule <capsule.vsf> <option-hex64>   (deploy.sh: the ferros kist capsule carries every release egg and Photon's option in its kist section)
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
    let mut notice = false;
    let mut print_bundle = false;
    let mut rows: Vec<ManifestRow> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--channel" => channel = args.next(),
            "--out" => out = args.next(),
            "--merge" => merge = args.next(),
            "--release-notice" => notice = true,
            "--print-release-bundle" => print_bundle = true,
            "--check-capsule" => {
                let (Some(path), Some(option)) = (args.next(), args.next()) else {
                    fail("--check-capsule needs the capsule and the option (64 hex chars)");
                };
                check_capsule(&path, &option);
                return;
            }
            "--artefact" => {
                let (
                    Some(platform),
                    Some(arch),
                    Some(version),
                    Some(commit_hex),
                    Some(url),
                    Some(hash_hex),
                    Some(size_str),
                ) = (
                    args.next(),
                    args.next(),
                    args.next(),
                    args.next(),
                    args.next(),
                    args.next(),
                    args.next(),
                )
                else {
                    fail("--artefact needs 7 values: platform arch major.minor.patch commit-sha1-hex url blake3-hex size-bytes");
                };
                let size: u64 = match size_str.parse() {
                    Ok(n) => n,
                    Err(_) => fail("--artefact size must be the artefact's byte count"),
                };
                let mut v = version.split('.').map(|p| p.parse::<usize>());
                let (Some(Ok(major)), Some(Ok(minor)), Some(Ok(patch)), None) =
                    (v.next(), v.next(), v.next(), v.next())
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
                rows.push(ManifestRow {
                    platform,
                    arch,
                    version: (major, minor, patch),
                    commit,
                    url,
                    hash,
                    size,
                });
            }
            other => fail(&format!("unknown arg {other}")),
        }
    }
    // The release key's PUBLIC bundle (2026-11-10): every scheme's public key derived from the release seed, printed as hex for fgtw's pin (fgtw::release); nothing secret is printed.
    if print_bundle {
        let key = photon_messenger::crypto::self_verify::release_signing_key().unwrap_or_else(|e| fail(&e));
        let bundle = fgtw::pq::SigningBundle::release(&key).public();
        println!("{}", hex::encode(bundle.to_bytes()));
        return;
    }
    // The release notice (2026-11-10): one signed VSF document saying "a release is out", posted by deploy.sh to fgtw.org, which checks the signer is the release key and wakes every device to look at the signed manifest.
    // It replaces the admin token.
    if notice {
        let out = out.unwrap_or_else(|| fail("--out required"));
        let key = photon_messenger::crypto::self_verify::release_signing_key().unwrap_or_else(|e| fail(&e));
        let signer_pub = ed25519_dalek::SigningKey::from_bytes(&key).verifying_key().to_bytes().to_vec();
        let unsigned = vsf::VsfBuilder::new()
            .creation_time_oscillations(vsf::eagle_time_oscillations())
            .signed_only_eggs(VsfType::ke(signer_pub), &fgtw::release::release_slots())
            .add_section("release_notice", vec![("channel".to_string(), VsfType::a(vsf::Ascii::lit("release")))])
            .build()
            .unwrap_or_else(|e| fail(&format!("build: {e}")));
        let signed = fgtw::release::sign_release(unsigned, &fgtw::pq::SigningBundle::release(&key)).unwrap_or_else(|e| fail(&format!("sign: {e}")));
        std::fs::write(&out, &signed).unwrap_or_else(|e| fail(&format!("write: {e}")));
        println!("photon-manifest: wrote the signed release notice to {out}");
        return;
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
            Ok(bytes) => match parse_manifest(&bytes, channel).or_else(|_| photon_messenger::network::updates::parse_legacy_manifest(&bytes, channel)) {
                Ok(existing) => {
                    for row in existing {
                        if !rows
                            .iter()
                            .any(|r| r.platform == row.platform && r.arch == row.arch)
                        {
                            rows.push(row);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("photon-manifest: merge source unverifiable ({e}) — starting fresh")
                }
            },
            Err(e) => eprintln!("photon-manifest: merge source unreadable ({e}) — starting fresh"),
        }
    }
    rows.sort_by(|a, b| (&a.platform, &a.arch).cmp(&(&b.platform, &b.arch)));

    let key = photon_messenger::crypto::self_verify::release_signing_key().unwrap_or_else(|e| fail(&e));
    // Every release egg (2026-11-10, "no single eggs"): the header reserves one slot per scheme for the release key, and sign_release fills each (fgtw::release).
    let signer_pub = ed25519_dalek::SigningKey::from_bytes(&key)
        .verifying_key()
        .to_bytes()
        .to_vec();

    let mut builder = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .signed_only_eggs(VsfType::ke(signer_pub), &fgtw::release::release_slots());
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
        // Artefact byte count — the download bar's denominator when the CDN streams chunked (a cache-busted fresh binary always does). Absent on old manifests reads 0.
        section.add_field_multi("size", vec![VsfType::z(r.size as usize)]);
        section.add_field_multi("url", vec![VsfType::nu(r.url.clone())]);
        section.add_field_multi("hash", vec![VsfType::hb(r.hash.to_vec())]);
        builder = builder.add_section_direct(section);
    }
    let unsigned = builder
        .build()
        .unwrap_or_else(|e| fail(&format!("build: {e}")));
    let signed = fgtw::release::sign_release(unsigned, &fgtw::pq::SigningBundle::release(&key))
        .unwrap_or_else(|e| fail(&format!("sign: {e}")));

    // Round-trip gate before publish: what we just wrote must pass the exact client-side check.
    match parse_manifest(&signed, channel) {
        Ok(parsed) if parsed.len() == rows.len() => {}
        Ok(parsed) => fail(&format!(
            "self-check row count mismatch: wrote {}, parsed {}",
            rows.len(),
            parsed.len()
        )),
        Err(e) => fail(&format!("self-check failed: {e}")),
    }
    std::fs::write(&out, &signed).unwrap_or_else(|e| fail(&format!("write: {e}")));
    println!(
        "photon-manifest: wrote {out} ({} artefact sections, channel {})",
        rows.len(),
        channel.label()
    );
}

/// The ferros kist capsule (deploy.sh, scripts/lib/ferros-kist.sh) is Photon's: signed by the release key with every egg (fgtw::release, the check ferros's installer runs), its `hp` a true content hash, and its `kist` section's `option` Photon's (2026-11-10: the option left the header for its own field).
fn check_capsule(path: &str, option_hex: &str) {
    let bytes = std::fs::read(path).unwrap_or_else(|e| fail(&format!("{path}: {e}")));
    // The option comes from the deploy's command line: refuse anything but exactly 32 bytes.
    let option: [u8; 32] = hex::decode(option_hex)
        .ok()
        .and_then(|v| v.try_into().ok())
        .unwrap_or_else(|| fail("--check-capsule option must be 64 hex chars"));
    fgtw::release::verify_release(&bytes).unwrap_or_else(|e| fail(&format!("{path}: {e}")));
    let sections = photon_messenger::storage::record::verified_sections(&bytes, Some(fgtw::release::RELEASE_ED25519))
        .unwrap_or_else(|e| fail(&format!("{path}: {e}")));
    let named = sections
        .iter()
        .find(|s| s.name == "kist")
        .and_then(|s| s.get_field("option").and_then(|f| f.values.first()));
    match named {
        Some(VsfType::hR(o)) if o[..] == option[..] => {}
        _ => fail(&format!("{path}: its option is not Photon's")),
    }
    println!("photon-manifest: {path} is Photon's kist: every release egg verifies, option {option_hex}, blake3 {}", blake3::hash(&bytes).to_hex());
}
