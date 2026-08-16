# photon memory — shared corpus (all machines)

One PRIVATE repo, one memory corpus, every machine's Claude memory dir is a checkout of it. Two naming conventions coexist deliberately (MacBook: kebab-case topic files; desktop: `project_*` files) — filenames never collide; **MEMORY.md is the only merge conflict and it always resolves by UNION** (keep every index line from both sides under the one `# Memory index` heading).

## Joining a machine whose memory dir predates this repo (desktop: do this once)

```sh
cd ~/.claude/projects/<project-dir>/memory   # desktop: /home/nick/.claude/projects/-mnt-Octopus-Code-photon/memory
git init -b main
git remote add origin https://github.com/nickspiker/photon-claude-memory.git
git add -A && git commit -m "desktop memory corpus, pre-merge snapshot"
git fetch origin
git merge origin/main --allow-unrelated-histories
# MEMORY.md conflicts: resolve by UNION of both index sections, then:
git add -A && git commit
git push -u origin main
```

## Every session, every machine

- `git pull` before acting on memory (another machine may have pushed).
- After writing/updating memories: `git add -A && git commit && git push`.
- This repo stays PRIVATE: memories carry handles and family roles the public photon repo must never contain.
