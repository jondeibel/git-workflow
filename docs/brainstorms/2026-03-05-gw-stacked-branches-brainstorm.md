# Brainstorm: gw - Git Stacked Branch Manager

**Date:** 2026-03-05
**Status:** Complete

## What We're Building

A Rust CLI tool called `gw` that manages stacked branches on top of plain git. It tracks parent-child relationships between branches in a stack, automatically propagates rebases when you modify an upstream branch, and provides a clean tree visualization of your stacks. All metadata lives in `.git/gw/` so nothing gets pushed to the remote.

### Core Capabilities

1. **Stacked branch management** - Create, track, and adopt branches into ordered stacks off a base branch (e.g., dev)
2. **Automatic rebase propagation** - When you commit to a branch in the middle of a stack, all descendant branches get rebased automatically
3. **Controlled sync** - The tip branch (base of stack, closest to dev) stays pinned to the commit on dev when the stack was created. `gw sync` pulls dev, detects if the tip was merged, auto-promotes the next branch, and rebases the remaining stack onto dev
4. **Tree visualization** - Inline terminal tree showing branch relationships, commit counts, and stack structure
5. **Local-only storage** - All metadata in `.git/gw/`, never pushed to remote

### User Workflows

**Creating a new stack:**
```
$ git checkout dev
$ gw stack create auth
  > Created stack 'auth' off dev @ abc1234
  > Created branch 'auth' (tip)
```

**Adding branches to a stack:**
```
$ gw branch create auth-tests
  > Added 'auth-tests' to stack 'auth'
  > Child of 'auth'
```

Auto-detection works here. If you're on a tracked branch and create a new branch via `gw branch create`, it becomes a child of the current branch.

**Adopting existing branches:**
```
$ gw adopt branch-a branch-b branch-c
  > Adopting 3 branches into stack off dev
  > Rebasing branch-b onto branch-a...
  > Rebasing branch-c onto branch-b...
  > Stack created: branch-a (tip) -> branch-b -> branch-c
```

Argument order defines stack order. First arg is the tip (closest to dev), last is the leaf. The base branch is inferred from the first branch's parent. If branches aren't already chained, `gw adopt` rebases them into a proper chain (with a warning before rewriting history).

**Making changes and propagating:**
```
$ git checkout auth        # tip branch, has a PR open
$ git commit ...           # address PR feedback
$ gw rebase                # propagate to all descendants
$ gw push                  # pushes ONLY current branch
```

Descendants are rebased locally but NOT force-pushed. You push each branch individually when you're ready.

**Syncing with dev:**
```
$ gw sync
  > Pulling dev...
  > Detected: 'auth' was merged into dev (via PR #42, squash merge)
  > Removing 'auth' from stack
  > Rebasing remaining branches onto dev
  > New tip: 'auth-tests'
```

**Viewing stacks:**
```
$ gw tree
dev (pinned: abc1234)
+-- feature/auth (tip) <- PR #42 open
    |-- 3 commits ahead
    +-- feature/auth-tests
        +-- feature/auth-ui
            +-- 2 commits

dev (pinned: def5678)
+-- billing/api (tip)
    +-- billing/ui
```

Multiple independent stacks are supported and shown together.

**Removing a branch from a stack:**
```
$ gw branch remove auth-tests
  > Removing 'auth-tests' from stack 'auth'
  > Re-parenting 'auth-ui' onto 'auth'
  > Rebasing auth-ui onto auth...
  > Done. Stack: auth (tip) -> auth-ui
```

Removing a branch from the middle of a stack re-parents its children to the removed branch's parent and rebases them accordingly. The git branch itself is NOT deleted, just untracked from the stack.

**Deleting a stack:**
```
$ gw stack delete auth
  > Untracking stack 'auth'
  > Branches remain: auth, auth-tests, auth-ui (no longer managed by gw)
```

Deleting a stack removes gw's metadata only. The git branches stay intact.

## Why This Approach

**Thin git wrapper with TOML metadata in `.git/gw/`**

- Shells out to `git` for all operations (checkout, rebase, push, pull)
- Stores stack metadata as TOML files (one per stack) in `.git/gw/stacks/`
- Relies on git's existing rebase machinery rather than reimplementing it
- Simple, transparent, debuggable (you can read the TOML files directly)

We chose this over libgit2 (git2-rs) because shelling out to git for rebases is far more reliable than reimplementing rebase logic in-process. The metadata layer is simple enough that TOML files are all we need.

We chose a standalone binary (`gw`) over a git subcommand (`git-gw`) for brevity in daily use. Can always add git subcommand support later.

## Key Decisions

1. **Language: Rust** - Single binary distribution, fast, matches the git tooling ecosystem
2. **Storage: `.git/gw/stacks/*.toml`** - Local-only, human-readable, one file per stack
3. **Git interaction: Shell out to git** - Reliable rebase behavior without reimplementation
4. **Tip = base of stack** - The branch closest to dev. Goes up for PR first. When merged, next branch auto-promotes. (Note: "tip" is counterintuitive in git terms where it usually means the latest commit. Consider renaming to "root" or "base" during implementation if it causes confusion in help text.)
5. **No auto force-push** - `gw push` only pushes the current branch. Descendants are rebased locally but pushed individually
6. **Multiple stacks** - Support several independent stacks simultaneously, all visible in `gw tree`
7. **Three creation paths** - `gw stack create` (new stack), `gw branch create` (add to current stack), `gw adopt` (retroactively stack existing branches with automatic rebasing into a chain)
8. **Sync is deliberate** - Stack stays pinned to the dev commit when created. Only `gw sync` pulls dev and checks for merges
9. **Auto-promote on merge** - When sync detects the tip was merged, it removes it and rebases the remaining stack onto dev automatically
10. **CLI name: `gw`** - Short, quick to type
11. **Conflict handling: Stop and continue** - When rebase propagation hits a conflict, gw pauses and lets you resolve it. `gw rebase --continue` finishes propagating, `gw rebase --abort` rolls back. Propagation state tracked in `.git/gw/state.toml`
12. **Squash merge detection: Hybrid with optional gh** - Primary: if `gh` CLI is available, auto-detect PR numbers and match against `(#NNNNN)` in dev's commit messages. Fallback: `git patch-id` comparison. Based on analysis of the Webflow repo, squash commits consistently include PR numbers but NOT branch names
13. **Branch naming: Suggest but don't enforce** - gw suggests naming patterns when creating branches but accepts whatever you provide
14. **Per-branch metadata: Infer from git** - Push state inferred from git's remote tracking refs rather than stored. Less state to maintain
15. **Branch removal: Re-parent children** - Removing a branch from the middle of a stack rebases its children onto its parent. Stack deletion only removes gw metadata, git branches stay intact
16. **Adopt rebases into chain** - `gw adopt` detects whether branches are already chained and rebases them into proper order if needed, with a warning before history rewrite
