# Sourced, not executed. GitHub Releases upload helper (via gh) — the redundant mirror behind R2.
# R2 is the primary serving origin (fastest edge, stable flat URLs the installers hardcode); GitHub is the fallback. We upload to R2 FIRST, then here, so a failed GitHub step leaves R2 already serving and is cheap to retry alone. Self-verify is origin-blind, so a GitHub-served binary is exactly as trusted.
#
# CDN STALENESS (learned the hard way): GitHub's release-ASSET download URLs are fronted by Fastly and, after a `--clobber` to the same asset name, keep serving the OLD bytes until the edge entry expires.
# A stale-but-signed older binary passes self-verify, so the installer would silently run old code. Two consequences shape the design:
#   * RELEASE channel uses an IMMUTABLE per-version `v<n>` tag — each asset name is written once, never clobbered, so its URL is safe to hardcode and can never go stale.
#   * DEV channel uses CONTENT-HASHED asset names (`...-development-<blake3-8>`) on the reusable `dev` prerelease — every build gets a brand-new, never-before-cached URL, so staleness is impossible.
#     The "which hash is current" question is answered by the GitHub *API* (api.github.com), which is a DIFFERENT endpoint from the Fastly asset CDN and returns fresh (cache-control: no-cache) — so the installer resolves the newest hashed asset with no pointer file and no R2 dependency. This keeps github.com/nickspiker/photon fully self-sufficient when holdmyoscilloscope is down.

GH_REPO="nickspiker/photon"

# ensure_release <tag> <prerelease:true|false>
# Idempotently make sure a release exists for <tag>. Safe to call before every upload.
ensure_release() {
    local tag="$1" prerelease="$2"
    if gh release view "$tag" --repo "$GH_REPO" >/dev/null 2>&1; then
        return 0
    fi
    echo "Creating GitHub release $tag..."
    if [ "$prerelease" = "true" ]; then
        gh release create "$tag" --repo "$GH_REPO" --prerelease \
            --title "Development (rolling)" \
            --notes "Rolling development builds. Assets are replaced in place; not for production. Every binary is Ed25519-signed and self-verifies on launch."
    else
        gh release create "$tag" --repo "$GH_REPO" \
            --title "$tag" \
            --notes "Photon $tag. Every binary is Ed25519-signed and self-verifies on launch."
    fi
}

# publish_github <tag> <asset-name> <local-file>
# Upload (or replace) one asset on <tag>. --clobber keeps the download URL stable across pushes.
publish_github() {
    local tag="$1" name="$2" file="$3"
    if [ ! -f "$file" ]; then
        echo "ERROR: asset not found for GitHub upload: $file"
        return 1
    fi
    # The DOWNLOAD asset name is gh's basename of the uploaded path — NOT the `#label` suffix (that's a
    # display label only). Our on-disk binaries are all named `photon-messenger`, which would collide and
    # give the wrong URL. So symlink the file under the flat asset name in a temp dir and upload THAT.
    # (Symlink, not copy — gh dereferences it, so we don't duplicate a ~27 MB binary just to rename it.)
    local staging
    staging=$(mktemp -d)
    ln -sf "$(readlink -f "$file")" "$staging/$name"
    gh release upload "$tag" "$staging/$name" --repo "$GH_REPO" --clobber
    rm -rf "$staging"
    echo "  ↳ GitHub: https://github.com/$GH_REPO/releases/download/$tag/$name"
}

# blake3_short <file>  -> first 8 hex chars of the file's BLAKE3 digest.
# BLAKE3 matches photon's own hashing; b3sum ships in the dev toolchain (cargo install b3sum).
blake3_short() {
    local file="$1"
    if ! command -v b3sum >/dev/null 2>&1; then
        echo "ERROR: b3sum not found (install with: cargo install b3sum)" >&2
        return 1
    fi
    b3sum --no-names "$file" | cut -c1-8
}

# publish_github_dev <base-name> <local-file>
# Dev channel: content-address the asset so its URL is never CDN-stale. Uploads as
# <base-name>-<hash> to the `dev` prerelease, then prunes older hashed builds for this base-name
# (keeps the most recent DEV_KEEP). Returns nothing; the API is the installer's source of truth.
DEV_KEEP=3
publish_github_dev() {
    local base="$1" file="$2"
    if [ ! -f "$file" ]; then
        echo "ERROR: asset not found for GitHub upload: $file"
        return 1
    fi
    ensure_release dev true
    local hash name
    hash=$(blake3_short "$file") || return 1   # bail rather than upload a hash-less colliding name
    name="${base}-${hash}"
    publish_github dev "$name" "$file"
    prune_github_dev "$base"
}

# prune_github_dev <base-name>
# Delete all but the newest DEV_KEEP assets matching "<base-name>-<hash>" on the `dev` release,
# so the rolling prerelease doesn't accumulate a build per push forever. Newest kept by upload time.
prune_github_dev() {
    local base="$1"
    # List assets on `dev`, keep only this base's hashed variants, sort NEWEST-FIRST by createdAt (raw
    # API order isn't guaranteed), drop the first DEV_KEEP, delete the rest. Best-effort (non-fatal).
    local stale
    stale=$(gh release view dev --repo "$GH_REPO" --json assets \
        -q "[.assets[] | select(.name | startswith(\"${base}-\"))] | sort_by(.createdAt) | reverse | .[${DEV_KEEP}:] | .[].name" 2>/dev/null) || return 0
    local a
    for a in $stale; do
        echo "  ↳ pruning stale dev asset: $a"
        gh release delete-asset dev "$a" --repo "$GH_REPO" --yes 2>/dev/null || true
    done
}
