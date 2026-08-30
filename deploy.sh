#!/bin/bash
set -e
# Anchor to the repo root like dev.sh does — every source/sub-script below is repo-relative, and the bridge runs commands from wherever its shell happens to sit (field 2026-08-26: invoked from /, died at the first source).
cd "$(dirname "$0")"

source scripts/lib/github.sh
source scripts/lib/manifest.sh
# ALL sources live up here, before the bump: `source` is a POSIX special builtin, so its failure aborts the shell WITHOUT running the ERR trap — a mid-script source that dies post-bump strands the release commit with no rollback banner (field 2026-08-27, the stranded v67).
source scripts/lib/snapbuild.sh
# The release git flow (provenance-by-tag preflight/tag/advance) — a plain `git push` of an hour-old bump commit is rejected the instant another device pushed to main during the build (field 2026-08-28, five dead deploys). See the lib header; proven by scripts/test/release-git-test.sh.
source scripts/lib/release-git.sh

# Source-level gates FIRST — before the lock, the release bump, or any of the cross-compiles below — so a comment/parse/migration slip fails in under a second, not after the whole platform matrix has built.
source scripts/lib/preflight.sh
preflight_gates

# Version scheme (2026-07-16): major.minor.patch. THIS SCRIPT does the release increment: whatever the tree holds (X.Y.0 fresh, or X.Y.P after dev publishes), the release ships X.(Y+1).0 — minor bumped, patch zeroed (patch 0 is RESERVED for releases; dev publishes bump the patch ≥1 and reach clients via the dev manifest).
# Ordering discipline (same as the dev publishes): refuse a dirty tree, bump, COMMIT THE BUMP FIRST — so every built binary embeds the actual release commit (no "+dirty") and the signed manifest stamps the same HEAD. A failure anywhere rolls that one commit back (trap below), leaving the tree exactly as it started.
# The publish lock keeps a dev-*.sh from bumping the version mid-deploy (and vice versa) — the same race that mis-stamped a dev manifest row on 2026-07-16.
manifest_take_publish_lock
if [ -n "$(git status --porcelain)" ]; then
    # A diff confined to Cargo.lock is DETERMINISTIC BOOKKEEPING, not dirt: sibling path-dep crates bump their own versions and every cargo run lazily re-locks the next one (kete→manifestus→spirix in one evening, each blocking a deploy). Absorb it as its own commit so HEAD still matches the built tree exactly; anything else stays a hard refusal.
    if [ -z "$(git status --porcelain | grep -vE '^.M Cargo\.lock$')" ]; then
        git add Cargo.lock
        git commit -q -m "lock: sibling path-dep re-lock (deploy preflight auto-absorb)"
        git push -q origin main || { echo "ERROR: could not push the lock re-lock commit — reconcile main first."; exit 1; }
        echo "Cargo.lock-only drift absorbed (sibling path-dep versions moved) — committed + pushed."
    else
        echo "ERROR: working tree is dirty — a release stamps HEAD into every binary + the signed manifest."
        echo "       Commit (or stash) first."
        git status --short | head -20
        exit 1
    fi
fi
# Make local main identical to origin BEFORE bumping — a release commit built on a stale base can never fast-forward, and the tag/advance flow below assumes the bump sits on the shared tip. Behind → fast-forwards; diverged (un-pushed local work) → refuses. This is the gate whose absence let five deploys build for an hour and then die at the push (2026-08-28).
release_git_preflight main
# TAG-AUTHORITY VERSIONING (2026-08-30): the shipped set IS the vN tags, so the next number derives from them — never from the mutable Cargo.toml counter, which leaked v67/v68/v70 when a deploy died between bump and publish. No bump commit exists at all now: the version is injected into the BUILD TREE only (the frozen snapshot, below), and the number is EARNED when release_publish_tag lands after R2 is live. A deploy that dies anywhere leaves ZERO git residue — the failure state IS the clean state, so no rollback has to work (the old EXIT-trap rollback failed twice in different ways; SIGKILL beat it by design).
CURRENT_VERSION=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([0-9]+\.[0-9]+\.[0-9]+)".*/\1/')
MAJOR=$(echo "$CURRENT_VERSION" | cut -d. -f1)
SHIP_VERSION=$(release_next_minor)
FULL_VERSION="${MAJOR}.${SHIP_VERSION}.0"
# The provenance tip: the commit every binary embeds (PHOTON_STAMP_COMMIT overrides build.rs's probe, so the snapshot's injected version can't read "+dirty") and the commit the tag will pin. HEAD never moves during the deploy now.
TIP_COMMIT="$(git rev-parse HEAD)"
export PHOTON_STAMP_COMMIT="$(git rev-parse --short=12 HEAD)"
DEPLOY_SHIPPED=0
trap 'rc=$?; if [ "$DEPLOY_SHIPPED" != "1" ]; then echo ""; echo "DEPLOY DID NOT SHIP (exit $rc, last: ${BASH_COMMAND}) — no git residue to undo (tag-authority: the number was never allocated)."; fi' EXIT
# Named-line diagnostics stay on ERR (the EXIT trap knows the last command, ERR knows the LINE); signals convert to an exit so the EXIT trap runs.
trap 'echo ""; echo "DEPLOY FAILED at line $LINENO: ${BASH_COMMAND}"' ERR
trap 'exit 130' INT TERM HUP
echo "Deploying version: $FULL_VERSION (tag-derived; tree was $CURRENT_VERSION) from tip ${PHOTON_STAMP_COMMIT}"


# Convert to dozenal names for display
dozenal_names() {
    local n=$1
    local digits=("Zil" "Zila" "Zilor" "Ter" "Tera" "Teror" "Lun" "Luna" "Lunor" "Stel" "Stela" "Stelor")
    if [ "$n" -eq 0 ]; then
        echo "Zil"
        return
    fi
    local result=""
    while [ "$n" -gt 0 ]; do
        local digit=$((n % 12))
        if [ -z "$result" ]; then
            result="${digits[$digit]}"
        else
            result="${digits[$digit]} $result"
        fi
        n=$((n / 12))
    done
    echo "$result"
}
DOZENAL_VERSION=$(dozenal_names $SHIP_VERSION)
echo "Dozenal version: $DOZENAL_VERSION"

# Allow release builds (bypasses build.rs safety check)
export PHOTON_ALLOW_RELEASE=1

R2_BUCKET="holdmyoscilloscope"
R2_PATH="photon"
R2_URL="https://brobdingnagian.holdmyoscilloscope.com/$R2_PATH"

# ════════════════════════════════════════════════════════════════════════════════════════════════════
# BUILD PHASE — produce + sign every artefact AND the signed manifest. NOTHING is public yet.
# A failure anywhere in here aborts (set -e / the ERR trap rolls back the release commit) BEFORE the first wrangler put, so a half-built release can never reach R2 (some platforms new, others stale, manifest pointing at absent binaries). The old order built the manifest tool + manifest AFTER the uploads, so a manifest-build failure stranded already-public binaries with no matching manifest — that's what this fixes.
# ════════════════════════════════════════════════════════════════════════════════════════════════════

# SOURCE FREEZE (edit-safe release): reflink-snapshot photon + its whole path-dep closure the instant the build phase starts, then build every target from the FROZEN tree — so editing the live tree while a multi-minute cross-platform deploy runs can't tear a build (the "Read theme.rs (lines 86-89)" corruption a mid-edit deploy hit).
# CARGO_TARGET_DIR stays the REAL ./target (snap + live units carry different unit hashes, so both coexist), so every `./target/release/...` signer call and every artefact path below is untouched.
# Off-btrfs / any snapshot failure → SNAP_DIR stays "." and the live tree builds exactly as before.
SNAP_DIR="."
if snapbuild_take; then
    SNAP_DIR="$SNAPBUILD_ROOT/photon"
    export CARGO_TARGET_DIR="$(pwd)/target"
    echo "Source frozen (reflink snapshot) — edit away, this deploy builds from the frozen tree"
fi
# VERSION INJECTION (tag-authority): the ship version lands in the BUILD TREE only — the snapshot when it took, else the live files with a guaranteed restore. No commit either way; the number is earned at the tag. Cargo.lock gets the same surgical awk as release_advance_main (network-free, deterministic).
inject_version() {
    ( cd "$1" \
        && sed -i -E "s/^version = \"[0-9]+\.[0-9]+\.[0-9]+\"/version = \"${FULL_VERSION}\"/" Cargo.toml \
        && awk -v v="$FULL_VERSION" '
            /^name = "photon-messenger"$/ { print; getline; sub(/^version = "[^"]*"/, "version = \"" v "\""); print; next }
            { print }
        ' Cargo.lock > Cargo.lock.new && mv Cargo.lock.new Cargo.lock )
}
if [ "$SNAP_DIR" != "." ]; then
    inject_version "$SNAP_DIR" || { echo "ERROR: version injection into the snapshot failed"; exit 1; }
else
    # Live-tree fallback (snapshot didn't take): patch the two files and ALWAYS restore them at exit — success included, since main's version advances only via release_advance_main after the tag.
    inject_version "." || { echo "ERROR: version injection failed"; exit 1; }
    trap 'rc=$?; git checkout HEAD -- Cargo.toml Cargo.lock 2>/dev/null; if [ "$DEPLOY_SHIPPED" != "1" ]; then echo ""; echo "DEPLOY DID NOT SHIP (exit $rc, last: ${BASH_COMMAND}) — injected version restored; no git residue (tag-authority)."; fi' EXIT
fi
# Run a cargo build from the frozen source (falls back to the live tree if the snapshot didn't take).
# Env vars set inline by the caller (cross sysroots, osxcross wrappers) pass through the subshell unchanged.
# A bare `( subshell )` that fails at the tail of a function called under `set -e` aborts the script but does
# NOT trigger the ERR trap (bash swallows it) — that silently killed the whole deploy at the Redox build with no message and the version bump left committed (2026-08-13: forkpty missing on redox). Catch the failure explicitly, name the target that tore, and `return` non-zero so the ERR trap fires and rolls the bump back.
# Every build's stderr is TEE'd: streamed live to the terminal (nothing hidden) AND captured so warnings can be counted. Cargo only re-emits warnings for crates it recompiles, so a warm-cache deploy would otherwise look pristine while the tree carries lint (2026-08-13: 11 workspace warnings invisible across a 6-target run).
# DEPLOY_WARNINGS accumulates the count across every target for the end-of-deploy summary.
DEPLOY_WARNINGS=0
snap_cargo() {
    local errlog rc; errlog="$(mktemp)"
    # Foreground tee — a pipeline member the shell WAITS on, so the diagnostic can never be lost. The old `2> >(tee …)` process substitution raced the exiting ERR trap and ate the ENTIRE cargo error plus the marker below (2026-08-14: the v54 orb-include failure printed nothing but a line number).
    # cargo emits diagnostics and progress on stderr and nothing meaningful on stdout, so merging the streams keeps the live scroll identical; tee's own (always-0) status would mask the failure under plain set -e, so cargo's verdict comes back via PIPESTATUS.
    ( cd "$SNAP_DIR" && cargo "$@" ) 2>&1 | tee "$errlog" >&2
    rc=${PIPESTATUS[0]}
    if [ "$rc" -ne 0 ]; then
        # cargo's real diagnostic (the error[...] block) has already streamed to stderr above — this only names WHICH invocation tore so it isn't lost in the scroll. Read the cargo error above, not this line.
        echo "" >&2
        echo "^^^ BUILD FAILED here: cargo $* (in $SNAP_DIR) — the cargo error is ABOVE this line ^^^" >&2
        rm -f "$errlog"
        return 1
    fi
    local n; n="$(grep -c '^warning' "$errlog" 2>/dev/null || echo 0)"
    DEPLOY_WARNINGS=$(( DEPLOY_WARNINGS + n ))
    [ "$n" -gt 0 ] && echo "  ⚠ cargo $* — $n warning(s) this build (running total: $DEPLOY_WARNINGS)" >&2
    rm -f "$errlog"
}

# Lint gate: report the TRUE warning state of the tree up front, cache or no cache. The per-build tallies below only catch what each target recompiles; a fully warm cache re-emits nothing, so this cache-fresh check is the one place the deploy always names how much lint the release is shipping. Advisory (never aborts) — surfacing the count is the point, not gating on it.
echo ""
echo "Lint check (cache-fresh warning count for the whole workspace)..."
LINT_WARNINGS="$( ( cd "$SNAP_DIR" && cargo check --workspace --message-format=short 2>&1 ) | grep -E '^warning' | grep -v 'generated .* warning' | sort | uniq)"
LINT_COUNT="$(printf '%s' "$LINT_WARNINGS" | grep -c '^warning' || echo 0)"
if [ "$LINT_COUNT" -gt 0 ]; then
    echo "  ⚠ $LINT_COUNT distinct warning(s) in the tree — this release is NOT lint-clean:"
    printf '%s\n' "$LINT_WARNINGS" | sed 's/^/      /'
else
    echo "  ✓ workspace is lint-clean"
fi

# The two release TOOLS first — a failure to build the signer or the manifest tool must abort before any platform binary is even built, let alone uploaded.
echo ""
echo "Building release tools (signer + manifest)..."
snap_cargo build --release --bin photon-signature-signer --bin photon-manifest

# Build and sign Linux x86_64 (native). SNAP_DIR is exported so build-release.sh builds from the frozen tree too.
export SNAP_DIR
./build-release.sh

# Build Linux ARM64 (cross-compile)
echo ""
echo "Building Linux ARM64 release..."
CFLAGS_aarch64_unknown_linux_gnu="--sysroot=/usr/aarch64-redhat-linux/sys-root/fc42" \
PKG_CONFIG_SYSROOT_DIR=/usr/aarch64-redhat-linux/sys-root/fc42 \
PKG_CONFIG_PATH=/usr/aarch64-redhat-linux/sys-root/fc42/usr/lib64/pkgconfig \
PKG_CONFIG_ALLOW_CROSS=1 \
snap_cargo build --release --target aarch64-unknown-linux-gnu

echo ""
echo "Signing Linux ARM64 binary..."
./target/release/photon-signature-signer target/aarch64-unknown-linux-gnu/release/photon-messenger

# Build Windows (x86_64)
echo ""
echo "Building Windows x86_64 release..."
snap_cargo build --release --target x86_64-pc-windows-gnu

echo ""
echo "Signing Windows x86_64 binary..."
./target/release/photon-signature-signer target/x86_64-pc-windows-gnu/release/photon-messenger.exe

# Build Windows on ARM (aarch64) — native for Snapdragon X / Copilot+ PCs (no x86 emulation).
# Toolchain: the llvm-mingw prebuilt at $WINARM_MINGW (clang/lld/llvm-rc + a complete aarch64-w64-mingw32 ucrt sysroot). aarch64-pc-windows-gnullvm is the LLVM-MinGW target (NOT msvc — needs no MSVC/xwin). The C deps
# (ring, pqcrypto, aws-lc-sys) compile against the vendored sysroot; the wrapper is BOTH the C compiler (via the aarch64-w64-mingw32-clang name cc-rs auto-detects on PATH) AND the linker (it knows its own import libs).
# build.rs uses llvm-rc for the icon on aarch64.
# ARM64 is a REQUIRED target like every other platform: a missing toolchain aborts the release (no silent skip — a deploy either ships every platform or ships none). Install from github.com/mstorsjo/llvm-mingw.
WINARM_MINGW="/mnt/Harbor/Code/llvm-mingw"
if [ ! -x "$WINARM_MINGW/bin/aarch64-w64-mingw32-clang" ]; then
    echo "ERROR: llvm-mingw toolchain not found at $WINARM_MINGW — required for the Windows ARM64 target."
    echo "       Install from github.com/mstorsjo/llvm-mingw (ucrt build), or remove the ARM64 target from this script."
    exit 1
fi
echo ""
echo "Building Windows ARM64 release..."
PATH="$WINARM_MINGW/bin:$PATH" \
CARGO_TARGET_AARCH64_PC_WINDOWS_GNULLVM_LINKER="$WINARM_MINGW/bin/aarch64-w64-mingw32-clang" \
snap_cargo build --release --target aarch64-pc-windows-gnullvm

echo ""
echo "Signing Windows ARM64 binary..."
./target/release/photon-signature-signer target/aarch64-pc-windows-gnullvm/release/photon-messenger.exe

# Build Redox
echo ""
echo "Building Redox release..."
snap_cargo build --release --target x86_64-unknown-redox

echo ""
echo "Signing Redox binary..."
./target/release/photon-signature-signer target/x86_64-unknown-redox/release/photon-messenger

# Apple code signature with a STABLE identity (self-signed 10-yr cert in the keys dir, identifier org.fgtw.photon): macOS TCC keys privacy grants (Local Network etc.) on this identity, so an updated binary keeps its permissions instead of re-prompting every release (the linker's ad-hoc signature had no identity and a per-build identifier). MUST run BEFORE photon-signature-signer — rcodesign rewrites the Mach-O, which would strip an already-appended Ed25519 tail. Gatekeeper is unaffected either way: the curl installer never sets the quarantine xattr.
MACOS_CODESIGN_CERT="/mnt/Harbor/Code/keys/photon-macos-codesign"
apple_sign() {
    rcodesign sign \
        --pem-file "$MACOS_CODESIGN_CERT.crt" \
        --pem-file "$MACOS_CODESIGN_CERT.key" \
        --binary-identifier org.fgtw.photon \
        "$1"
}

# Build macOS Intel
echo ""
echo "Building macOS Intel release..."
CC_x86_64_apple_darwin=/mnt/Harbor/Code/osxcross/target/bin/x86_64-apple-darwin-clang-wrapper \
CXX_x86_64_apple_darwin=/mnt/Harbor/Code/osxcross/target/bin/x86_64-apple-darwin-clang-wrapper \
OSXCROSS_TRIPLE=x86_64-apple-darwin \
CMAKE_TOOLCHAIN_FILE_x86_64_apple_darwin="$(pwd)/scripts/lib/osxcross-cmake.toolchain" \
snap_cargo build --release --target x86_64-apple-darwin

echo ""
echo "Signing macOS Intel binary (Apple identity, then photon Ed25519)..."
apple_sign target/x86_64-apple-darwin/release/photon-messenger
./target/release/photon-signature-signer target/x86_64-apple-darwin/release/photon-messenger

# Build macOS Apple Silicon
echo ""
echo "Building macOS ARM64 release..."
CC_aarch64_apple_darwin=/mnt/Harbor/Code/osxcross/target/bin/aarch64-apple-darwin-clang-wrapper \
CXX_aarch64_apple_darwin=/mnt/Harbor/Code/osxcross/target/bin/aarch64-apple-darwin-clang-wrapper \
CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/mnt/Harbor/Code/osxcross/target/bin/aarch64-apple-darwin-clang-wrapper \
OSXCROSS_TRIPLE=aarch64-apple-darwin \
CMAKE_TOOLCHAIN_FILE_aarch64_apple_darwin="$(pwd)/scripts/lib/osxcross-cmake.toolchain" \
snap_cargo build --release --target aarch64-apple-darwin

echo ""
echo "Signing macOS ARM64 binary (Apple identity, then photon Ed25519)..."
apple_sign target/aarch64-apple-darwin/release/photon-messenger
./target/release/photon-signature-signer target/aarch64-apple-darwin/release/photon-messenger

# Build Android APK
echo ""
echo "Building Android release..."
./scripts/android/build.sh

# Every binary exists + is signed. Read the Windows hash (for the .ps1 installer) and BUILD the signed manifest now — it only reads local files + hashes them, so it belongs in the build phase. Publishing it is the last upload in the publish phase, so a running client never sees a manifest whose binaries aren't up yet.
WINDOWS_SHA256=$(cat target/x86_64-pc-windows-gnu/release/photon-messenger.exe.sha256)

echo ""
echo "Building signed release manifest..."
MANIFEST_TOOL=target/release/photon-manifest
b3() { b3sum "$1" | cut -d' ' -f1; }
COMMIT="$TIP_COMMIT"
"$MANIFEST_TOOL" --channel release --out /tmp/manifest-release.vsf \
    --artefact Linux   x86_64 "$FULL_VERSION" "$COMMIT" "$R2_URL/photon-messenger-linux-x86_64-release"  "$(b3 target/release/photon-messenger)" "$(manifest_size target/release/photon-messenger)" \
    --artefact Linux   arm64  "$FULL_VERSION" "$COMMIT" "$R2_URL/photon-messenger-linux-arm64-release"   "$(b3 target/aarch64-unknown-linux-gnu/release/photon-messenger)" "$(manifest_size target/aarch64-unknown-linux-gnu/release/photon-messenger)" \
    --artefact Windows x86_64 "$FULL_VERSION" "$COMMIT" "$R2_URL/photon-messenger-windows-release.exe"   "$(b3 target/x86_64-pc-windows-gnu/release/photon-messenger.exe)" "$(manifest_size target/x86_64-pc-windows-gnu/release/photon-messenger.exe)" \
    --artefact Windows arm64  "$FULL_VERSION" "$COMMIT" "$R2_URL/photon-messenger-windows-arm64-release.exe" "$(b3 target/aarch64-pc-windows-gnullvm/release/photon-messenger.exe)" "$(manifest_size target/aarch64-pc-windows-gnullvm/release/photon-messenger.exe)" \
    --artefact macOS   x86_64 "$FULL_VERSION" "$COMMIT" "$R2_URL/photon-messenger-macos-intel-release"   "$(b3 target/x86_64-apple-darwin/release/photon-messenger)" "$(manifest_size target/x86_64-apple-darwin/release/photon-messenger)" \
    --artefact macOS   arm64  "$FULL_VERSION" "$COMMIT" "$R2_URL/photon-messenger-macos-arm64-release"   "$(b3 target/aarch64-apple-darwin/release/photon-messenger)" "$(manifest_size target/aarch64-apple-darwin/release/photon-messenger)" \
    --artefact Android arm64  "$FULL_VERSION" "$COMMIT" "$R2_URL/photon-messenger-android-release.apk"   "$(b3 android/app/build/outputs/apk/release/app-release.apk)" "$(manifest_size android/app/build/outputs/apk/release/app-release.apk)"

# Patch the Windows installer with the correct per-arch hashes NOW (a build-phase transform, no upload).
sed "s/\$expectedHashX64 = \"[A-F0-9]*\"/\$expectedHashX64 = \"$WINDOWS_SHA256\"/" installers/install-release.ps1 > /tmp/install-release.ps1
WINARM_SHA256=$(cat target/aarch64-pc-windows-gnullvm/release/photon-messenger.exe.sha256)
sed -i "s/\$expectedHashArm64 = \"[A-F0-9]*\"/\$expectedHashArm64 = \"$WINARM_SHA256\"/" /tmp/install-release.ps1

echo ""
echo "BUILD PHASE complete — all 8 platforms + signed manifest built. Nothing public yet."

# ════════════════════════════════════════════════════════════════════════════════════════════════════
# PUBLISH PHASE — everything below goes OUTWARD. Every artefact already exists locally, so the uploads are the first irreversible outward step. (The GitHub mirror / website / notice further down are best-effort once R2 is live, as noted at each.)
# ════════════════════════════════════════════════════════════════════════════════════════════════════

echo ""
echo "Uploading to R2 ($R2_BUCKET/$R2_PATH)..."

# TIMEOUT + RETRY on every put (field 2026-08-28): wrangler holds a dead HTTP stream FOREVER — a deploy sat 20 minutes on install-release.ps1, a few-KB text file, after all eight binaries had uploaded fine. 10 minutes covers the biggest binary on the slowest sane uplink; three attempts covers the transient-connection class; a genuinely dead network still fails LOUDLY into the trap instead of hanging.
r2_put() {
    local attempt
    for attempt in 1 2 3; do
        if timeout 600 wrangler r2 object put "$@"; then
            return 0
        fi
        echo "  ⚠ upload attempt $attempt failed/timed out — retrying: $1" >&2
    done
    echo "  ✗ upload failed after 3 attempts: $1" >&2
    return 1
}

# Upload all release binaries to R2 (flat naming with -release suffix)
r2_put "$R2_BUCKET/$R2_PATH/photon-messenger-linux-x86_64-release" \
    --file target/release/photon-messenger --remote
r2_put "$R2_BUCKET/$R2_PATH/photon-messenger-linux-arm64-release" \
    --file target/aarch64-unknown-linux-gnu/release/photon-messenger --remote
r2_put "$R2_BUCKET/$R2_PATH/photon-messenger-windows-release.exe" \
    --file target/x86_64-pc-windows-gnu/release/photon-messenger.exe --remote
r2_put "$R2_BUCKET/$R2_PATH/photon-messenger-windows-arm64-release.exe" \
    --file target/aarch64-pc-windows-gnullvm/release/photon-messenger.exe --remote
r2_put "$R2_BUCKET/$R2_PATH/photon-messenger-redox-release" \
    --file target/x86_64-unknown-redox/release/photon-messenger --remote
r2_put "$R2_BUCKET/$R2_PATH/photon-messenger-macos-intel-release" \
    --file target/x86_64-apple-darwin/release/photon-messenger --remote
r2_put "$R2_BUCKET/$R2_PATH/photon-messenger-macos-arm64-release" \
    --file target/aarch64-apple-darwin/release/photon-messenger --remote
r2_put "$R2_BUCKET/$R2_PATH/photon-messenger-android-release.apk" \
    --file android/app/build/outputs/apk/release/app-release.apk \
    --content-type application/vnd.android.package-archive --remote

# Upload install scripts and assets
r2_put "$R2_BUCKET/$R2_PATH/install-release.sh" \
    --file installers/install-release.sh --content-type text/plain --remote
# The resilient-launch shim (docs/resilient-launch.md): the installer fetches this to $HOME/.local/bin/photon-launch and points the .desktop at it.
r2_put "$R2_BUCKET/$R2_PATH/photon-launch.sh" \
    --file installers/photon-launch.sh --content-type text/plain --remote
r2_put "$R2_BUCKET/$R2_PATH/icon-1024.png" \
    --file assets/icon-1024.png --content-type image/png --remote
r2_put "$R2_BUCKET/$R2_PATH/app.png" \
    --file assets/icon-256.png --content-type image/png --remote
r2_put "$R2_BUCKET/$R2_PATH/install-release.ps1" \
    --file /tmp/install-release.ps1 --content-type text/plain --remote

# Manifest LAST: publish it only after every binary it references is live, so a client that polls the fresh manifest never fetches a URL that isn't up yet.
r2_put "$R2_BUCKET/$R2_PATH/manifest-release.vsf" \
    --file /tmp/manifest-release.vsf --content-type application/octet-stream --remote

echo ""
echo "Linux ARM64, Linux x86_64, Windows x86_64, Windows ARM64, Redox, macOS Intel, macOS ARM64, Android binaries + manifest deployed to R2"
echo "  Windows SHA256: $WINDOWS_SHA256"

# R2 is fully live — the release is public. Its provenance is the built commit $COMMIT: the hash build.rs baked into every binary (PHOTON_GIT_COMMIT) and the SHA the signed manifest stamped. Pin it with the IMMUTABLE v<n> tag and push THAT — the commit rides along with its hash intact, even though main has moved past its parent during the ~1h build. This is NOT a branch push (a plain `git push` of the hour-old bump is rejected non-fast-forward the moment any device touched main during the build — five deploys died exactly here, 2026-08-28). main itself only ever gets the fast-forwarding dev-line bump, further down.
# Disarm the signal/error rollback traps the instant provenance is on GitHub: the release is now permanent, and every step below (mirror, website, notice, dev-line) is best-effort — a hiccup there must NEVER roll back a shipped release. (The EXIT trap stays armed but is gated by DEPLOY_SHIPPED, and every best-effort step below is `|| echo`-guarded so set -e can't fire in this zone.)
GH_TAG="v$SHIP_VERSION"
release_publish_tag "$GH_TAG" "$COMMIT" "release v${SHIP_VERSION} (${FULL_VERSION}, ${DOZENAL_VERSION}) — version injected at build (tag-authority), source tip ${PHOTON_STAMP_COMMIT}"
trap - ERR INT TERM HUP

# Mirror the identical signed artefacts to a GitHub Release `v<n>` (redundant fallback behind R2).
# Same bytes as R2 — never rebuild per-destination — so the Windows SHA256 patched above holds everywhere.
# BEST-EFFORT: by this point the release is fully live on R2, so a GitHub hiccup (uploads.github.com 502s are routine; one aborted the whole v39 deploy here, 2026-07-19 — stranding the website update, release notice, and dev-line-open behind an already-shipped release) warns loudly and moves on, never aborts.
mirror() {
    publish_github "$GH_TAG" "$1" "$2" || echo "WARNING: GitHub mirror of $1 failed — continuing (R2 is authoritative and live)"
}
echo ""
echo "Mirroring release to GitHub ($GH_TAG)..."
# The tag already exists on origin (pushed above at $COMMIT), so ensure_release just attaches a Release to it — no --target.
if ensure_release "$GH_TAG" false; then
    mirror "photon-messenger-linux-x86_64-release"  target/release/photon-messenger
    mirror "photon-messenger-linux-arm64-release"   target/aarch64-unknown-linux-gnu/release/photon-messenger
    mirror "photon-messenger-windows-release.exe"   target/x86_64-pc-windows-gnu/release/photon-messenger.exe
    mirror "photon-messenger-windows-arm64-release.exe" target/aarch64-pc-windows-gnullvm/release/photon-messenger.exe
    mirror "photon-messenger-redox-release"         target/x86_64-unknown-redox/release/photon-messenger
    mirror "photon-messenger-macos-intel-release"   target/x86_64-apple-darwin/release/photon-messenger
    mirror "photon-messenger-macos-arm64-release"   target/aarch64-apple-darwin/release/photon-messenger
    mirror "photon-messenger-android-release.apk"   android/app/build/outputs/apk/release/app-release.apk
else
    echo "WARNING: GitHub release creation failed — skipping the mirror entirely (R2 is authoritative and live)"
fi
# Binaries only — no installer scripts on GitHub. The README carries the GitHub-fallback install commands (they fetch these assets by name from the latest release), so the scripts aren't needed here.

# Update website version and date
WEBSITE_DIR="/mnt/Chiton/MEGA/holdmyoscilloscope/photon"
DEPLOY_DATE=$(date +%Y-%m-%d)
sed_i "s/Version: [^·]*· Updated: [^<]*/Version: $DOZENAL_VERSION · Updated: $DEPLOY_DATE/" "$WEBSITE_DIR/index.html"
echo "Updated website: Version $DOZENAL_VERSION, Date $DEPLOY_DATE"

# Deploy website to Cloudflare Pages
echo ""
echo "Deploying website..."
# Guarded: past the provenance tag the release is permanent, so a website hiccup must not abort into the EXIT trap.
(cd /mnt/Chiton/MEGA/holdmyoscilloscope && ./deploy.sh) || echo "WARNING: website deploy failed (non-fatal — the release is live; re-deploy the site when convenient)."

# Rollback traps were already disarmed right after R2 went live (above), where the release became permanent.

# Ring the release notice: the worker broadcasts "release" over the WS hub (every RUNNING client polls the signed manifest now instead of at its 6-8h cadence) and fires the FCM `updates` topic
# (wakes dozed Android subscribers). Advisory only — what installs is still gated by the manifest signature + stamp window — so best-effort: a failed curl just leaves everyone on the slow cadence.
echo ""
echo "Sending release notice (hub + FCM topic)..."
curl -s "https://fgtw.org/admin/release-notice?auth=f6d46fc44bd35b1b7204640d8cade6b2d01ef5e6ba96261200bcb728003c2724" || echo "release notice failed (non-fatal)"

# OPEN THE DEV LINE (2026-07-17): main must never rest at X.Y.0 — patch 0 IS the release marker, so a dev build compiled from a .0 tree masquerades as the release ("already on latest release" on a dev build, observed live). With provenance-by-tag, main never even HOLDS the .0 commit (that lives only as the tag); it advances straight to the .1 dev line. advance_main crafts that Cargo.toml-only bump on the FRESHEST origin tip inside a throwaway worktree, so it fast-forwards no matter what moved on main during the build, and never touches the live working tree. Best-effort: the release already shipped via the tag.
DEV_OPEN="${MAJOR}.${SHIP_VERSION}.1"
release_advance_main main "$DEV_OPEN" "dev line open: v${DEV_OPEN} (release v${SHIP_VERSION} shipped at .0, tag ${GH_TAG})" \
    || echo "WARNING: dev-line-open failed — main still at its pre-release tip; bump it manually. The release itself is LIVE (tag ${GH_TAG})."
# The built .0 commit was LOCAL-ONLY (its provenance is the tag). Bring local onto the new origin tip so the next release's preflight sees no divergence — preserving any edits made during the build.
release_sync_to_origin main || echo "WARNING: could not sync local to origin/main — run 'git fetch && git checkout -B main origin/main' when convenient."

echo ""
echo "Install with:"
echo "  curl -sSfL https://brobdingnagian.holdmyoscilloscope.com/$R2_PATH/install-release.sh | sh"
echo "  powershell -ExecutionPolicy Bypass -c \"irm https://brobdingnagian.holdmyoscilloscope.com/$R2_PATH/install-release.ps1 | iex\""

# The ONLY success banner. If a deploy exits before printing this, it did NOT ship — no matter how clean the last line looked (a silent Redox abort read as green all the way to "call your mum", 2026-08-13).
# Never vouch for a release you didn't watch print this line.
DEPLOY_SHIPPED=1
echo ""
echo "════════════════════════════════════════════════════════════"
echo "  ✓ DEPLOY COMPLETE — v${SHIP_VERSION} (${FULL_VERSION}) is PUBLIC"
if [ "${LINT_COUNT:-0}" -gt 0 ]; then
    echo "  ⚠ shipped with ${LINT_COUNT} outstanding warning(s) — see the lint check at the top"
else
    echo "  ✓ lint-clean"
fi
echo "════════════════════════════════════════════════════════════"
echo "completed $(date '+%F %T')"
