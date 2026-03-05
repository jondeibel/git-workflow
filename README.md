# gw

You break a big feature into multiple PRs. You push the first one for review, get feedback, commit a fix, and now you have to manually rebase every branch after it. When that PR finally gets squash-merged, you have to figure out which commit on dev matches your branch, remove it from the chain, and rebase everything again. Over and over.

`gw` handles all of that. It tracks the parent-child relationships between your branches, automatically propagates rebases through the chain, and detects squash merges so it can clean up the stack. Your branches are real git branches, your PRs are normal GitHub PRs, and gw just does the tedious coordination between them.

Everything lives in `.git/gw/` and never gets pushed to the remote.

## Why not Graphite, ghstack, or git-branchless

**Graphite** requires a cloud account and wraps your workflow through their service. `gw` is local-only with zero external dependencies.

**ghstack** rewrites your branches into a format GitHub can display, but what's on your machine doesn't match what's on GitHub. That gets confusing fast.

**git-branchless** reimagines git around changes rather than branches. If your team does one-branch-per-PR and squash-merges into dev, that model doesn't map cleanly.

`gw` is a thin wrapper around git, not a replacement for it. Each branch maps 1:1 to a PR. You push when you're ready. Reviewers see normal diffs. Squash merges get detected automatically.

## Install

```
git clone https://github.com/yourusername/gw.git
cd gw
cargo install --path .
```

Needs a [Rust toolchain](https://rustup.rs/). Optional: `gh` CLI for auto-detecting squash merges and showing PR status in `gw tree`.

### Shell completions

Tab completion for commands, flags, and branch names:

```bash
# zsh (add to ~/.zshrc)
eval "$(gw completions zsh)"

# bash (add to ~/.bashrc)
eval "$(gw completions bash)"

# fish
gw completions fish | source
```

## Quick start

```bash
# Set your base branch (if not main)
gw config set-base dev

# Create a stack
gw stack create auth

# Do work, commit, then add the next branch
gw branch create auth-tests

# See everything
gw tree

# Address PR feedback on auth, then propagate rebases
gw rebase

# Push when ready
gw push

# After auth gets squash-merged
gw sync

# Switch between branches interactively
gw switch
```

## Commands

| Command | What it does |
| --- | --- |
| `gw stack create <name>` | Create a new stack off the base branch |
| `gw stack delete <name>` | Remove stack metadata (branches stay) |
| `gw stack list` | List all stacks |
| `gw branch create <name>` | Add a branch to the current stack |
| `gw branch remove <name>` | Remove a branch and re-parent children |
| `gw adopt <branches...>` | Adopt existing branches into a stack |
| `gw rebase` | Propagate rebases to descendants |
| `gw rebase --continue` | Resume after resolving conflicts |
| `gw rebase --abort` | Roll back all branches |
| `gw sync` | Pull base, detect merges, rebase stack |
| `gw sync --merged <branch>` | Manually indicate a branch was merged |
| `gw push` | Push the current branch |
| `gw switch [branch]` | Switch branches interactively or by name |
| `gw tree` | Show all stacks with branches and commits |
| `gw tree --pr` | Include PR status from GitHub |
| `gw config set-base <branch>` | Set the default base branch |
| `gw config show` | Show current configuration |
