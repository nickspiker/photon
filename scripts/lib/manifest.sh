# Sourced, not executed. Self-update manifest helpers (docs/updates.md).
# Callers cd to the repo root first and have R2_BUCKET/R2_PATH/R2_BASE_URL from publish.sh.

R2_DEV_URL="https://brobdingnagian.holdmyoscilloscope.com/photon"

# The current tree's full X.Y.Z version and short commit — what a published artefact is stamped with.
manifest_full_version() { grep -m1 '^version' Cargo.toml | sed -E 's/.*"([0-9]+\.[0-9]+\.[0-9]+)".*/\1/'; }
manifest_commit() { git rev-parse --short=12 HEAD; }

# BLAKE3 of a file (the manifest hash the client re-checks post-download).
manifest_b3() { b3sum "$1" | cut -d' ' -f1; }

# Bump the PATCH number in Cargo.toml (a dev publish; releases stay .0). Refuses to bump a .0 that
# hasn't been claimed by a real build yet? No — a dev publish ALWAYS moves patch: .0 -> .1, .1 -> .2.
# Leaves major.minor untouched. Commits so the published binary's embedded version is reproducible.
manifest_bump_dev_patch() {
    local full major minor patch next
    full=$(manifest_full_version)
    major=$(echo "$full" | cut -d. -f1); minor=$(echo "$full" | cut -d. -f2); patch=$(echo "$full" | cut -d. -f3)
    next=$((patch + 1))
    sed -i -E "s/^version = \"[0-9]+\.[0-9]+\.[0-9]+\"/version = \"${major}.${minor}.${next}\"/" Cargo.toml
    echo "dev patch: ${full} -> ${major}.${minor}.${next}"
}

# Fetch the current dev manifest, merge THIS platform's fresh row into it, re-sign, re-upload.
# Args: <platform-id> <artefact-object-name> <local-artefact-path>
# Requires $PHOTON_SIGNING_KEY (same key photon-signature-signer uses). Build the manifest tool once.
manifest_publish_dev_row() {
    local platform="$1" object="$2" file="$3"
    local full commit hash tool=target/debug/photon-manifest
    full=$(manifest_full_version); commit=$(manifest_commit); hash=$(manifest_b3 "$file")
    [ -x "$tool" ] || cargo build --bin photon-manifest
    # Pull the current dev manifest to merge (ignore failure — first publish starts fresh).
    curl -sSfL "$R2_DEV_URL/manifest-dev.vsf" -o /tmp/manifest-dev-current.vsf 2>/dev/null || true
    local merge_arg=""
    [ -s /tmp/manifest-dev-current.vsf ] && merge_arg="--merge /tmp/manifest-dev-current.vsf"
    "$tool" --channel dev --out /tmp/manifest-dev.vsf $merge_arg \
        --artefact "$platform" "$full" "$commit" "$R2_DEV_URL/$object" "$hash"
    wrangler r2 object put "$R2_BUCKET/$R2_PATH/manifest-dev.vsf" \
        --file /tmp/manifest-dev.vsf --content-type application/octet-stream --remote
    echo "dev manifest: row '$platform' -> $full ($commit) published"
}
