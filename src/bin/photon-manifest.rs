//! Build + sign the update version manifest (docs/updates.md) — invoked by deploy.sh (release channel, every platform row at once) and by each scripts/publish/dev-*.sh (dev channel, merging its ONE platform row into the fetched current manifest).
//!
//! Output is a COMPLETE VSF file signed by the release key (the same key that signs binaries), section `manifest`, one native multi-value `artefact` field per row: `(x platform, x version, x commit, x url, hb hash)`. No numbered field names — the row multiplicity IS the plurality.
//!
//! Usage:
//!   photon-manifest --channel dev --out manifest-dev.vsf \
//!       [--merge existing-manifest.vsf] \
//!       --artefact <platform> <version> <commit> <url> <blake3-hex> [--artefact ...]
//!
//! `--merge` keeps every row of the existing (signature-verified) manifest whose platform is NOT re-supplied — how a single-platform dev publish updates its row without clobbering the others. The signing key comes from $PHOTON_SIGNING_KEY (same contract as photon-signature-signer).

use photon_messenger::network::updates::{parse_manifest, ManifestRow};
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
                let (Some(platform), Some(version), Some(commit), Some(url), Some(hash_hex)) =
                    (args.next(), args.next(), args.next(), args.next(), args.next())
                else {
                    fail("--artefact needs 5 values: platform version commit url blake3-hex");
                };
                let hash_v = hex::decode(&hash_hex).unwrap_or_default();
                let hash: [u8; 32] = match hash_v.as_slice().try_into() {
                    Ok(h) => h,
                    Err(_) => fail("--artefact hash must be 64 hex chars (BLAKE3)"),
                };
                rows.push(ManifestRow { platform, version, commit, url, hash });
            }
            other => fail(&format!("unknown arg {other}")),
        }
    }
    let channel = channel.unwrap_or_else(|| fail("--channel dev|release required"));
    let out = out.unwrap_or_else(|| fail("--out required"));
    if rows.is_empty() {
        fail("at least one --artefact required");
    }

    // Merge: carry forward every existing row whose platform isn't being re-supplied. The existing file must verify (same trust gate as the client) — a corrupt/tampered current manifest is dropped, not merged.
    if let Some(path) = merge {
        match std::fs::read(&path) {
            Ok(bytes) => match parse_manifest(&bytes) {
                Ok(existing) => {
                    for row in existing {
                        if !rows.iter().any(|r| r.platform == row.platform) {
                            rows.push(row);
                        }
                    }
                }
                Err(e) => eprintln!("photon-manifest: merge source unverifiable ({e}) — starting fresh"),
            },
            Err(e) => eprintln!("photon-manifest: merge source unreadable ({e}) — starting fresh"),
        }
    }
    rows.sort_by(|a, b| a.platform.cmp(&b.platform));

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

    let mut section = vsf::VsfSection::new("manifest");
    section.add_field_multi("channel", vec![VsfType::x(channel.clone())]);
    for r in &rows {
        section.add_field_multi(
            "artefact",
            vec![
                VsfType::x(r.platform.clone()),
                VsfType::x(r.version.clone()),
                VsfType::x(r.commit.clone()),
                VsfType::x(r.url.clone()),
                VsfType::hb(r.hash.to_vec()),
            ],
        );
    }
    let unsigned = vsf::VsfBuilder::new()
        .creation_time_oscillations(vsf::eagle_time_oscillations())
        .signed_only(VsfType::ke(signer_pub))
        .add_section_direct(section)
        .build()
        .unwrap_or_else(|e| fail(&format!("build: {e}")));
    let signed = vsf::verification::sign_file(unsigned, &key)
        .unwrap_or_else(|e| fail(&format!("sign: {e}")));

    // Round-trip gate before publish: what we just wrote must pass the exact client-side check.
    if let Err(e) = parse_manifest(&signed) {
        fail(&format!("self-check failed: {e}"));
    }
    std::fs::write(&out, &signed).unwrap_or_else(|e| fail(&format!("write: {e}")));
    println!("photon-manifest: wrote {out} ({} rows, channel {channel})", rows.len());
}
