# Sourced by deploy.sh. THE release git flow, "provenance by tag".
#
# THE PROBLEM (field 2026-08-28, five dead deploys in a row): the old flow committed the version bump, held that commit through the ~1h cross-platform build, then did a plain `git push`. In a multi-machine tree another device (or the operator locally) almost always pushes something during that hour, so origin/main has moved and the push is rejected non-fast-forward — the whole release dies at the very end, after R2 is already public.
#
# THE FIX: split provenance from the branch.
#   * PROVENANCE lives in an immutable `v<n>` TAG on the EXACT built commit — the commit whose hash build.rs baked into every binary (PHOTON_GIT_COMMIT) and whose SHA the signed manifest stamps. Pushing the tag uploads that commit with its hash intact, even though main has moved past its parent. This is what a client, an auditor, or `git checkout v<n>` resolves.
#   * THE BRANCH (main) only ever receives Cargo.toml-only version bumps, each crafted on the FRESHEST origin tip inside a throwaway worktree — so they always fast-forward and never touch the live working tree (which still holds the built .0 bump plus whatever the operator edited during the build). main that moved on GitHub OR locally can no longer wedge anything.
#
# Factored into a sourced lib (not inline in deploy.sh) so the hermetic test at scripts/test/release-git-test.sh can drive it against a scratch remote — the git flow is now provable in seconds instead of only over a 1-hour real deploy.

# release_git_preflight [branch]
# Run BEFORE the version bump. Make the local branch byte-identical to origin/<branch>, or refuse — a release commit built on a stale base can never fast-forward at push time. Fetch, then:
#   up-to-date  → proceed
#   behind      → fast-forward up (origin is a strict descendant)
#   ahead/diverged → REFUSE: local carries commit(s) not on origin, which a release must not bury under its own history. Push or reconcile them first.
# TAG-AUTHORITY VERSIONING (2026-08-30, born from the leaked v67/v68/v70): a version number is EARNED at publish, never allocated at start. The shipped set IS the vN tags, so the next number derives from them — a deploy that dies anywhere before the tag leaves zero git residue and can never strand a number; the next run recomputes the same number and ships it for real. A stale local tag set can only under-count (never skip), and the preflight's tag fetch closes even that.
release_next_minor() {
    local last
    last="$(git tag -l 'v[0-9]*' | sed 's/^v//' | grep -E '^[0-9]+$' | sort -n | tail -1)"
    echo $(( ${last:-0} + 1 ))
}

release_git_preflight() {
    local branch="${1:-main}"
    # --tags because the tag set is the version authority (release_next_minor) — a machine that never deployed must still see every shipped number.
    # --force because the rolling `dev` prerelease tag moves on EVERY dev publish (publish_github_dev re-points it), and a non-force --tags fetch refuses to clobber the stale local copy — which failed this whole preflight the first time a dev publish happened on another view of the repo (field 2026-08-31). The v<n> release tags are immutable so force is a no-op for them.
    git fetch --quiet --force --tags origin "$branch" || { echo "ERROR: git fetch origin $branch failed — cannot verify the branch is current before a release."; return 1; }
    local local_head remote_head base
    local_head="$(git rev-parse HEAD)"
    remote_head="$(git rev-parse "origin/$branch")"
    if [ "$local_head" = "$remote_head" ]; then
        return 0
    fi
    base="$(git merge-base HEAD "origin/$branch")"
    if [ "$base" = "$local_head" ]; then
        echo "Preflight: local $branch is behind origin — fast-forwarding to ${remote_head:0:12} before the release bump."
        git merge --ff-only "origin/$branch" || { echo "ERROR: fast-forward to origin/$branch failed."; return 1; }
        return 0
    fi
    echo "ERROR: local $branch has commit(s) not on origin/$branch — a release must build on the shared tip. Push or reconcile these first:"
    git log --oneline "origin/$branch..HEAD" | head -10
    return 1
}

# release_publish_tag <tag> <commit>
# PROVENANCE: pin the exact built commit under an immutable tag and push it. The commit's hash is already baked into every binary and stamped in the manifest, so it must reach GitHub unchanged; pushing the tag ref uploads the commit object even when main has moved past its parent. Refuses a tag that already exists locally or on origin — a reused release number is never right.
release_publish_tag() {
    local tag="$1" commit="$2"
    if git rev-parse -q --verify "refs/tags/$tag" >/dev/null 2>&1; then
        echo "ERROR: tag $tag already exists locally — release-number reuse. Pick a fresh number or delete the stale tag."; return 1
    fi
    if git ls-remote --exit-code --tags origin "refs/tags/$tag" >/dev/null 2>&1; then
        echo "ERROR: tag $tag already exists on origin — release-number reuse."; return 1
    fi
    # Annotated when a message is given (tag-authority: the tagged tip's Cargo.toml holds the PRE-release version, so the tag message is where the shipped version is recorded); lightweight otherwise.
    if [ -n "${3:-}" ]; then
        git tag -a "$tag" "$commit" -m "$3" || { echo "ERROR: could not create tag $tag at $commit."; return 1; }
    else
        git tag "$tag" "$commit" || { echo "ERROR: could not create tag $tag at $commit."; return 1; }
    fi
    # Push the tag ref explicitly (refs/tags/…:refs/tags/…) so nothing else rides along; the commit object travels with it.
    git push origin "refs/tags/$tag" || { echo "ERROR: pushing tag $tag failed."; git tag -d "$tag" >/dev/null 2>&1; return 1; }
    echo "Provenance: tag $tag → ${commit:0:12} pushed (the built commit, hash intact)."
}

# release_advance_main <branch> <new_version> <commit_msg>
# Move <branch>'s Cargo.toml/Cargo.lock to <new_version> with a commit that touches ONLY those two files, crafted on the freshest origin tip in a throwaway worktree so it always fast-forwards and never involves the live working tree. Bounded retry: if origin advances between the fetch and the push, rebuild on the new tip and re-push. Best-effort by contract — the caller treats failure as non-fatal (the release itself is already live via the tag).
release_advance_main() {
    local branch="$1" new_version="$2" msg="$3"
    local wt rc=1 try
    wt="$(mktemp -d)"
    for try in 1 2 3 4 5; do
        git fetch --quiet origin "$branch" || { echo "advance-main: fetch failed (try $try)"; continue; }
        git worktree remove --force "$wt" >/dev/null 2>&1
        rmdir "$wt" 2>/dev/null
        git worktree add -q --detach "$wt" "origin/$branch" || { echo "advance-main: worktree add failed (try $try)"; continue; }
        (
            cd "$wt" || exit 1
            # Cargo.toml: the single package version line.
            sed -i -E "s/^version = \"[0-9]+\.[0-9]+\.[0-9]+\"/version = \"$new_version\"/" Cargo.toml
            # Cargo.lock: photon-messenger's own version — the line cargo writes directly under its name entry. Network-free (no `cargo update`), surgical, deterministic.
            awk -v v="$new_version" '
                /^name = "photon-messenger"$/ { print; getline; sub(/^version = "[^"]*"/, "version = \"" v "\""); print; next }
                { print }
            ' Cargo.lock > Cargo.lock.new && mv Cargo.lock.new Cargo.lock
            git add Cargo.toml Cargo.lock
            git commit -q -m "$msg"
        ) || { echo "advance-main: commit build failed (try $try)"; continue; }
        if git -C "$wt" push origin "HEAD:$branch" 2>/dev/null; then
            echo "advance-main: $branch → $new_version (Cargo.toml-only, fast-forward)."
            rc=0
            break
        fi
        echo "advance-main: push rejected — origin moved, rebuilding on the new tip (try $try)."
    done
    git worktree remove --force "$wt" >/dev/null 2>&1
    rmdir "$wt" 2>/dev/null
    return "$rc"
}

# release_sync_to_origin <branch>
# Post-release cleanup. The built commit C lived ONLY as a local commit (its provenance is the tag, not the branch), and origin/<branch> has since advanced to the dev-open bump — so local <branch> still sits on C and would look "diverged" to the NEXT release's preflight. Bring local onto the new origin tip, PRESERVING any edits the operator made during the build: stash across the move, and if the restore conflicts, say so loudly and leave the stash intact rather than dropping the work. Best-effort — the release is already live.
release_sync_to_origin() {
    local branch="$1"
    git fetch --quiet origin "$branch" || { echo "sync: fetch origin $branch failed — leaving local as-is."; return 1; }
    local dirty=0
    [ -n "$(git status --porcelain)" ] && dirty=1
    [ "$dirty" = 1 ] && git stash push -u -q -m "deploy: operator edits during build"
    if ! git checkout -q -B "$branch" "origin/$branch"; then
        echo "sync: checkout to origin/$branch failed."
        [ "$dirty" = 1 ] && git stash pop -q 2>/dev/null
        return 1
    fi
    if [ "$dirty" = 1 ]; then
        git stash pop -q 2>/dev/null || echo "WARNING: in-build edits conflicted while restoring onto the new tip — they are safe in 'git stash list'; resolve with 'git stash pop'."
    fi
    echo "sync: local $branch → origin tip ($(git rev-parse --short HEAD)); the release itself is the tag."
}
