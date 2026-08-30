#!/bin/bash
# Hermetic test for scripts/lib/release-git.sh — drives the provenance-by-tag release flow against a THROWAWAY local remote, so the git plumbing that used to be provable only over a 1-hour real deploy is now provable in seconds. Touches nothing outside its mktemp dir; no network.
set -u

LIB="$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/release-git.sh"
source "$LIB"

PASS=0; FAIL=0
ok()   { PASS=$((PASS+1)); echo "  ok   - $1"; }
bad()  { FAIL=$((FAIL+1)); echo "  FAIL - $1"; }
check(){ if eval "$2"; then ok "$1"; else bad "$1 [ $2 ]"; fi; }

ROOT="$(mktemp -d)"
trap 'rm -rf "$ROOT"' EXIT
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=t@t

# --- scratch origin (bare) + a seed working tree that publishes the first commit ---
git init -q --bare "$ROOT/origin.git"
seed_pkg() { # <dir> <version>
    mkdir -p "$1"
    cat > "$1/Cargo.toml" <<EOF
[package]
name = "photon-messenger"
version = "$2"
edition = "2021"
EOF
    cat > "$1/Cargo.lock" <<EOF
[[package]]
name = "other-dep"
version = "1.2.3"

[[package]]
name = "photon-messenger"
version = "$2"
dependencies = [
 "other-dep",
]
EOF
}
git clone -q "$ROOT/origin.git" "$ROOT/seed"
( cd "$ROOT/seed" && git checkout -q -b main && seed_pkg . 0.66.1 && echo "code v0" > app.txt \
  && git add -A && git commit -q -m "seed 0.66.1" && git push -q -u origin main )

# A SECOND clone standing in for "another device pushing to origin during our build".
git clone -q "$ROOT/origin.git" "$ROOT/sibling"
( cd "$ROOT/sibling" && git checkout -q main )
sibling_pushes() { # <msg>  — origin advances underneath us
    ( cd "$ROOT/sibling" && git pull -q --ff-only && echo "$1" >> app.txt \
      && git add -A && git commit -q -m "$1" && git push -q origin main )
}

# --- the DEPLOY clone under test ---
git clone -q "$ROOT/origin.git" "$ROOT/deploy"
cd "$ROOT/deploy" && git checkout -q main

echo "== preflight =="
check "up-to-date preflight passes" "release_git_preflight main >/dev/null 2>&1"

sibling_pushes "sibling code A"
# deploy clone is now behind origin; preflight must fast-forward it.
check "behind → fast-forward passes" "release_git_preflight main >/dev/null 2>&1"
check "  and local now == origin/main" "[ \"\$(git rev-parse HEAD)\" = \"\$(git rev-parse origin/main)\" ]"

# create a local-only commit AND advance origin → genuine divergence; preflight must refuse.
echo "local only" >> app.txt && git add -A && git commit -q -m "local-only work"
sibling_pushes "sibling code B"
check "diverged → preflight refuses" "! release_git_preflight main >/dev/null 2>&1"
# recover for the rest of the test: drop the local-only commit, sync.
git reset -q --hard origin/main >/dev/null 2>&1; git pull -q --ff-only

echo "== build the release commit, THEN origin moves under us =="
# mimic deploy: preflight, bump to .0, commit C (this is what build.rs would bake).
release_git_preflight main >/dev/null 2>&1
sed -i -E 's/^version = "[0-9]+\.[0-9]+\.[0-9]+"/version = "0.69.0"/' Cargo.toml
awk -v v=0.69.0 '/^name = "photon-messenger"$/{print;getline;sub(/^version = "[^"]*"/,"version = \"" v "\"");print;next}{print}' Cargo.lock > Cargo.lock.n && mv Cargo.lock.n Cargo.lock
git add Cargo.toml Cargo.lock && git commit -q -m "release: v69 (0.69.0)"
C="$(git rev-parse HEAD)"
sibling_pushes "sibling code C (during our build)"   # origin moves past C's parent

echo "== provenance tag =="
check "publish_tag succeeds despite origin having moved" "release_publish_tag v69 \"$C\" >/dev/null 2>&1"
check "  tag on origin resolves to the exact built commit C" "[ \"\$(git ls-remote origin refs/tags/v69 | cut -f1)\" = \"$C\" ]"
check "  re-publishing the same tag is refused" "! release_publish_tag v69 \"$C\" >/dev/null 2>&1"

echo "== advance main (Cargo.toml-only, on the moved tip) =="
check "advance_main succeeds" "release_advance_main main 0.69.1 'dev line open: v0.69.1' >/dev/null 2>&1"
# pull the new origin/main into a fresh checkout and inspect it.
git fetch -q origin main
NEWTIP="$(git rev-parse origin/main)"
check "  origin/main advanced (tip changed)" "[ \"$NEWTIP\" != \"$C\" ]"
check "  new tip has version 0.69.1" "git show origin/main:Cargo.toml | grep -q '^version = \"0.69.1\"'"
check "  new tip Cargo.lock bumped too" "git show origin/main:Cargo.lock | grep -A1 '^name = \"photon-messenger\"' | grep -q '0.69.1'"
# the advance commit must touch ONLY Cargo.toml + Cargo.lock (never app.txt or anything else).
CHANGED="$(git diff --name-only "${NEWTIP}~1" "${NEWTIP}")"
check "  advance touched ONLY Cargo.toml + Cargo.lock" "[ \"\$(echo \"$CHANGED\" | sort | tr '\n' ',')\" = 'Cargo.lock,Cargo.toml,' ]"
check "  sibling's concurrent code C survived on main" "git show origin/main:app.txt | grep -q 'sibling code C'"
check "  built commit C is NOT on main (provenance is the tag, not the branch)" "! git merge-base --is-ancestor $C origin/main"

echo "== sync local to origin (preserving in-build operator edits) =="
# deploy clone is still sitting on C with, say, an edit the operator made mid-build.
check "  (precondition) local is still on C" "[ \"\$(git rev-parse HEAD)\" = \"$C\" ]"
echo "operator tweak during build" >> operator_note.txt   # an untracked in-build edit
check "sync_to_origin succeeds" "release_sync_to_origin main >/dev/null 2>&1"
check "  local main now == origin dev-open tip" "[ \"\$(git rev-parse HEAD)\" = \"\$(git rev-parse origin/main)\" ]"
check "  next preflight sees no divergence" "release_git_preflight main >/dev/null 2>&1"
check "  operator's in-build edit was preserved" "[ -f operator_note.txt ] && grep -q 'operator tweak' operator_note.txt"

echo "== tag-authority version derivation (numbers earned at publish, never leaked) =="
# The suite has already minted v69 above; a leaked bump commit must not influence the count.
check "next minor = latest tag + 1" "[ \"\$(release_next_minor)\" = \"70\" ]"
git tag v70 >/dev/null 2>&1
check "  after v70 ships, next = 71" "[ \"\$(release_next_minor)\" = \"71\" ]"
git tag weird-tag v69 >/dev/null 2>&1; git tag v7-bad >/dev/null 2>&1
check "  non-vN tags never count" "[ \"\$(release_next_minor)\" = \"71\" ]"

echo ""
echo "==== $PASS passed, $FAIL failed ===="
[ "$FAIL" -eq 0 ]
