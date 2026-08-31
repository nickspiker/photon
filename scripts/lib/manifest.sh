# Sourced, not executed. Self-update manifest helpers (docs/updates.md).
# Callers cd to the repo root first and have R2_BUCKET/R2_PATH/R2_BASE_URL from publish.sh.

R2_DEV_URL="https://brobdingnagian.holdmyoscilloscope.com/photon"

# The current tree's full X.Y.Z version and FULL 40-hex git commit — what a published artefact is stamped with.
manifest_full_version() { grep -m1 '^version' Cargo.toml | sed -E 's/.*"([0-9]+\.[0-9]+\.[0-9]+)".*/\1/'; }
manifest_commit() { git rev-parse HEAD; }

# BLAKE3 of a file (the manifest hash the client re-checks post-download).
manifest_b3() { b3sum "$1" | cut -d' ' -f1; }

# Byte size of a file. `stat -c %s` is GNU-only — BSD/macOS stat spells it `-f%z` and errors out on -c, which took down a macOS publish AFTER the binary was already on R2 (manifest row never written, so the upload was live but unreferenced). Probe once, use everywhere.
manifest_size() { stat -c %s "$1" 2>/dev/null || stat -f%z "$1"; }

# In-place sed. GNU takes `-i` with no argument; BSD/macOS requires a backup suffix, so `sed -i -E ...` there reads "-E" as the SUFFIX and writes Cargo.toml-E while leaving the original untouched — the version bump silently did nothing, and the next publish would have collided on an already-published version. `-i ''` is the BSD spelling for "no backup".
# Usage: sed_i <sed-args...> <file>
sed_i() {
    if sed --version >/dev/null 2>&1; then
        sed -i "$@"          # GNU
    else
        sed -i '' "$@"       # BSD/macOS
    fi
}

# A publish stamps HEAD's commit into the manifest — a dirty tree has no honest commit to claim, so refuse outright (agreed 2026-07-16).
# ONE exception, same as deploy's preflight (2026-08-30): Cargo.lock-ONLY dirt. Sibling path-dep crates re-lock lazily — the first cargo touch after another machine's push (whose lock was written against ITS sibling checkouts) rewrites the lock to match the local tree, and that mechanical re-lock blocked every publish right after a pull (field 2026-08-31). Absorb it as its own commit + push; anything else stays a hard refusal.
manifest_refuse_dirty() {
    local dirt
    dirt="$(git status --porcelain)"
    [ -z "$dirt" ] && return 0
    if [ -z "$(echo "$dirt" | grep -vE '^.M Cargo\.lock$')" ]; then
        git add Cargo.lock
        git commit -q -m "lock: sibling path-dep re-lock (publish preflight auto-absorb)"
        git push -q || { echo "ERROR: lock-absorb push failed — reconcile with origin first."; exit 1; }
        echo "Cargo.lock-only drift absorbed (sibling path-dep versions moved) — committed + pushed."
        return 0
    fi
    echo "ERROR: working tree is dirty — a publish stamps HEAD into the signed manifest, and a dirty build has no honest commit to claim."
    echo "       Commit (or stash) first."
    git status --short | head -20
    exit 1
}

# One publish at a time per box. Without this, a second dev-*.sh started while the first still builds bumps the version mid-run, and the FIRST run's manifest row — which re-read the tree at row time — stamps the SECOND run's version+commit onto its own artefact (happened 2026-07-16: the v0.36.11 android APK published as v0.36.12 + the macos bump commit, so updating installed "12" that self-reports 11). fd 9 stays open for the sourcing script's lifetime; the kernel releases the lock when the script exits, success or failure.
manifest_take_publish_lock() {
    exec 9>>/tmp/photon-publish.lock
    if command -v flock >/dev/null; then
        if ! flock -n 9; then
            echo "Another publish is running — waiting for it to finish..."
            flock 9
        fi
        return
    fi
    # macOS has no flock(1) (util-linux only). Falling through silently would leave the publish UNLOCKED — the exact 2026-07-16 race above, just quieter — so use shlock(1), which ships with macOS and holds a PID-checked lock file: it detects a dead holder and steals the lock, so a crashed publish can't wedge the next one.
    if command -v shlock >/dev/null; then
        local lock=/tmp/photon-publish.pid
        until shlock -f "$lock" -p $$; do
            echo "Another publish is running (pid $(cat "$lock" 2>/dev/null)) — waiting for it to finish..."
            sleep 2
        done
        # The lock file is ours now; drop it when this shell exits, success or failure, mirroring the fd-9 lifetime the flock path relies on.
        trap 'rm -f "'"$lock"'"' EXIT
        return
    fi
    echo "WARNING: neither flock nor shlock found — publishing WITHOUT the one-at-a-time lock."
    echo "         Do not start another publish on this box until this one finishes."
}

# Dev publish preamble — PUBLISH-CURRENT-THEN-BUMP (2026-07-17): the tree ALREADY holds this publish's version.
# deploy.sh opens the dev line at X.Y.1 the moment a release ships, and every dev publish pre-bumps for the next one on its way out (manifest_end_dev_publish) — so the tree never rests at a version that hasn't been or isn't about to be published, a dev build can NEVER carry patch 0 (patch 0 IS the release marker — the one way to tell the flavours apart), and the first dev publish after a release ships exactly .1.
# Takes the publish lock, refuses dirty, refuses a .0 tree (a half-finished deploy), and PINS the version+commit for manifest_publish_dev_row so the row claims what this run's build embeds.
# Arg: <platform>-<arch> label, carried to the end-bump's commit message.
manifest_begin_dev_publish() {
    local label="$1" full patch
    # Source-level gates FIRST — before the lock, the version bump, or any build — so a comment/parse/migration slip fails in under a second, not after the cross-compile+publish. The single chokepoint every dev-*.sh passes through, so a new platform script can't forget it.
    source "$(dirname "${BASH_SOURCE[0]}")/preflight.sh"
    preflight_gates
    manifest_take_publish_lock
    manifest_refuse_dirty
    full=$(manifest_full_version)
    patch=$(echo "$full" | cut -d. -f3)
    if [ "$patch" = "0" ]; then
        echo "ERROR: tree version is ${full} — patch 0 is the RELEASE marker; a dev build must never wear it."
        echo "       deploy.sh opens the dev line at .1 after every release; if a deploy half-finished, bump the patch and commit."
        exit 1
    fi
    MANIFEST_PUBLISH_VERSION="$full"
    MANIFEST_PUBLISH_COMMIT=$(git rev-parse HEAD)
    MANIFEST_PUBLISH_LABEL="$label"
    echo "dev publish: v${full} (publish-current; the post-publish bump opens the next)"
}

# Dev publish epilogue: the artefact + manifest for the PINNED version are live — bump the patch and commit,
# opening the next dev line so every subsequent local build already wears its own (unpublished) number.
manifest_end_dev_publish() {
    local full major minor patch next
    full=$(manifest_full_version)
    major=$(echo "$full" | cut -d. -f1); minor=$(echo "$full" | cut -d. -f2); patch=$(echo "$full" | cut -d. -f3)
    next=$((patch + 1))
    sed_i -E "s/^version = \"[0-9]+\.[0-9]+\.[0-9]+\"/version = \"${major}.${minor}.${next}\"/" Cargo.toml
    # Cargo.lock records the workspace member's version — refresh it so the tree is exactly two files changed.
    cargo update --workspace --quiet 2>/dev/null || true
    git add Cargo.toml Cargo.lock
    git commit -q -m "dev: ${MANIFEST_PUBLISH_LABEL:-dev} v${full} published; next line v${major}.${minor}.${next}"
    echo "dev line: v${full} published -> tree now v${major}.${minor}.${next} (the NEXT build's number)"
    echo "completed $(date '+%F %T')"
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
        --artefact "$platform" "$arch" "$full" "$commit" "$R2_DEV_URL/$object" "$hash" "$(manifest_size "$file")"
    wrangler r2 object put "$R2_BUCKET/$R2_PATH/manifest-dev.vsf" \
        --file /tmp/manifest-dev.vsf --content-type application/octet-stream --remote
    echo "dev manifest: $platform/$arch -> $full ($commit) published"
}
