# Sourced, not executed. The Claude-artifact ratchet, born from the JPEG incident (2026-08-20): a session bolted `resample_to_jpeg` + an adjustable-quality JpegEncoder beside Nick's AV1 pipeline, and it shipped — a forensically-identifiable foreign format in a codebase whose owner never approved one. This gate makes the CLASS structurally unshippable: house formats only (VSF, AV1 via rav1e/rav1d, Opus, blake3, XChaCha20-Poly1305); introducing ANY output format, codec, or storage location is an OWNER decision.
# Baseline is ZERO. Do not add an allowlist entry to silence a failure — ask Nick first; the entry IS the approval record.

artifact_gate() {
    local off

    # 1. Encode-side foreign image codecs. Decode stays legal (avatars ingest JPEG/PNG/WebP); ENCODING to a foreign format is the banned act. Catches the encoder types and the write_with_encoder composition for jpeg/png/webp/gif/bmp.
    off=$(grep -rn --include="*.rs" -E "JpegEncoder|PngEncoder|WebPEncoder|GifEncoder|BmpEncoder|codecs::(jpeg|png|webp|gif|bmp)::.*Encoder" src/ 2>/dev/null)
    if [ -n "$off" ]; then
        echo "ARTIFACT GATE: foreign image ENCODER in photon — house formats only (VSF/AV1); output formats are an owner decision:"
        echo "$off"
        return 1
    fi

    # 2. Loose-file escape hatch: new fs::write / File::create outside the sanctioned modules. Storage is THE vault (kete) + the log; runtime artifacts live in runtime_dir. Everything else writing files is the sprawl coming back.
    # Sanctioned: storage/mod.rs (vault adapter, write_file, absorb walks), lib.rs (log machinery + crash sidecar), call/spool.rs (runtime-dir spool), platform/ (OS artifacts: autostart entries, control socket), bin/ + tests (tooling), ui/photon_app/attachments.rs (user-directed save-to-Downloads), ui/photon_app/input.rs (dev wipe walks), network/updates.rs (self-update artefact staging).
    off=$(grep -rn --include="*.rs" -E "fs::write\(|File::create\(" src/ 2>/dev/null \
        | grep -v -E "^src/(storage/mod\.rs|lib\.rs|call/spool\.rs|platform/|bin/|network/updates\.rs|ui/photon_app/(attachments|input)\.rs)" \
        | grep -v -E "^[^:]*:[0-9]+:\s*//")
    if [ -n "$off" ]; then
        echo "ARTIFACT GATE: loose-file write outside the sanctioned modules — state belongs in the vault (config census = log + vault); a new write location is an owner decision:"
        echo "$off"
        return 1
    fi

    return 0
}
