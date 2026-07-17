# Sourced, not executed. Self-update manifest helpers (docs/updates.md).
# Callers cd to the repo root first and have R2_BUCKET/R2_PATH/R2_BASE_URL from publish.sh.

R2_DEV_URL="https://brobdingnagian.holdmyoscilloscope.com/photon"

# The current tree's full X.Y.Z version and FULL 40-hex git commit — what a published artefact is stamped with.
manifest_full_version() { grep -m1 '^version' Cargo.toml | sed -E 's/.*"([0-9]+\.[0-9]+\.[0-9]+)".*/\1/'; }
manifest_commit() { git rev-parse HEAD; }

# BLAKE3 of a file (the manifest hash the client re-checks post-download).
manifest_b3() { b3sum "$1" | cut -d' ' -f1; }

# A publish stamps HEAD's commit into the manifest — a dirty tree has no honest commit to claim, so refuse outright (agreed 2026-07-16).
manifest_refuse_dirty() {
    if [ -n "$(git status --porcelain)" ]; then
        echo "ERROR: working tree is dirty — a publish stamps HEAD into the signed manifest, and a dirty build has no honest commit to claim."
        echo "       Commit (or stash) first."
        git status --short | head -20
        exit 1
    fi
}

# One publish at a time per box. Without this, a second dev-*.sh started while the first still builds bumps the version mid-run, and the FIRST run's manifest row — which re-read the tree at row time — stamps the SECOND run's version+commit onto its own artefact (happened 2026-07-16: the v0.36.11 android APK published as v0.36.12 + the macos bump commit, so updating installed "12" that self-reports 11). fd 9 stays open for the sourcing script's lifetime; the kernel releases the lock when the script exits, success or failure.
manifest_take_publish_lock() {
    exec 9>>/tmp/photon-publish.lock
    if ! flock -n 9; then
        echo "Another publish is running — waiting for it to finish..."
        flock 9
    fi
}

# Dev publish preamble: take the publish lock, refuse dirty, bump the PATCH (releases stay .0), COMMIT the bump — so the
# subsequent build embeds a clean HEAD whose commit is exactly what the manifest will claim.
# PINS the version+commit for manifest_publish_dev_row: the row must claim what THIS run's build embeds, never whatever the tree happens to say when the row finally publishes.
# Arg: <platform>-<arch> label for the commit message.
manifest_begin_dev_publish() {
    local label="$1" full major minor patch next
    manifest_take_publish_lock
    manifest_refuse_dirty
    full=$(manifest_full_version)
    major=$(echo "$full" | cut -d. -f1); minor=$(echo "$full" | cut -d. -f2); patch=$(echo "$full" | cut -d. -f3)
    next=$((patch + 1))
    sed -i -E "s/^version = \"[0-9]+\.[0-9]+\.[0-9]+\"/version = \"${major}.${minor}.${next}\"/" Cargo.toml
    # Cargo.lock records the workspace member's version — refresh it so the tree is exactly two files changed.
    cargo update --workspace --quiet 2>/dev/null || true
    git add Cargo.toml Cargo.lock
    git commit -q -m "dev: ${label} v${major}.${minor}.${next}"
    MANIFEST_PUBLISH_VERSION="${major}.${minor}.${next}"
    MANIFEST_PUBLISH_COMMIT=$(git rev-parse HEAD)
    echo "dev patch: ${full} -> ${MANIFEST_PUBLISH_VERSION} (committed)"
}

# Fetch the current dev manifest, merge THIS platform's fresh artefact section into it, re-sign, re-upload.
# Args: <platform> <arch> <artefact-object-name> <local-artefact-path>
# Requires $PHOTON_SIGNING_KEY (same key photon-signature-signer uses).
manifest_publish_dev_row() {
    local platform="$1" arch="$2" object="$3" file="$4"
    local full commit hash tool=target/debug/photon-manifest
    # The PINNED stamp from manifest_begin_dev_publish — re-reading the tree here races anything that moved it since the bump (a concurrent publish, a mid-build commit). Fallback only for a caller that never pinned.
    full="${MANIFEST_PUBLISH_VERSION:-$(manifest_full_version)}"
    commit="${MANIFEST_PUBLISH_COMMIT:-$(manifest_commit)}"
    hash=$(manifest_b3 "$file")
    [ -x "$tool" ] || cargo build --bin photon-manifest
    # Pull the current dev manifest to merge (ignore failure — first publish starts fresh). Clear the previous run's copy FIRST: a failed fetch must not silently merge a stale leftover.
    rm -f /tmp/manifest-dev-current.vsf
    curl -sSfL "$R2_DEV_URL/manifest-dev.vsf" -o /tmp/manifest-dev-current.vsf 2>/dev/null || true
    local merge_arg=""
    [ -s /tmp/manifest-dev-current.vsf ] && merge_arg="--merge /tmp/manifest-dev-current.vsf"
    "$tool" --channel development --out /tmp/manifest-dev.vsf $merge_arg \
        --artefact "$platform" "$arch" "$full" "$commit" "$R2_DEV_URL/$object" "$hash"
    wrangler r2 object put "$R2_BUCKET/$R2_PATH/manifest-dev.vsf" \
        --file /tmp/manifest-dev.vsf --content-type application/octet-stream --remote
    echo "dev manifest: $platform/$arch -> $full ($commit) published"
}
