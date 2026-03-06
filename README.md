# gw

You break a big feature into multiple PRs. You push the first one for review, get feedback, commit a fix, and now you have to manually rebase every branch after it. When that PR finally gets squash-merged, you have to figure out which commit on dev matches your branch, remove it from the chain, and rebase everything again. Over and over.

`gw` handles all of that. It tracks the parent-child relationships between your branches, automatically propagates rebases through the chain, and detects squash merges so it can clean up the stack. Your branches are real git branches, your PRs are normal GitHub PRs, and gw just does the tedious coordination between them.

Everything lives in `.git/gw/` and never gets pushed to the remote.

<p align="center">
  <img src="docs/gw-tree.png" alt="gw log output showing three stacks with branches and commits" width="640">
</p>

## Table of contents

- [Why this exists](#why-this-exists)
- [Install](#install)
- [Quick start](#quick-start)
- [Already have branches? Adopt them](#already-have-branches-adopt-them)
- [Configuration](#configuration)
- [Commands](#commands)

## Why this exists

Stacking PRs is the best way to ship large features. You break the work into small, reviewable pieces that build on each other, and your reviewers get focused diffs instead of a 2,000 line monster PR. The problem is that git and GitHub don't know your branches are related, so every time you update one branch in the middle of the chain, you're stuck manually rebasing everything that comes after it. And when a PR gets squash-merged, you have to untangle which commits landed, remove the branch, and rebase the rest onto the updated base. It's the kind of work that should take zero thought but ends up eating real time and creating real mistakes.

The force push problem makes it worse. Most stacking tools automatically rebase your entire stack onto the latest base branch whenever you sync. That means every branch gets new SHAs and needs a force push. GitHub handles force pushes badly in reviews: your reviewer's "viewed" state resets, inline comments get orphaned, and the diff becomes impossible to follow because GitHub can't track what actually changed between the old and new versions of a force-pushed branch. You're stacking PRs to make review easier, and the tool is actively making it harder.

There are good tools out there, but they each come with a tradeoff that doesn't sit right for teams already deep in GitHub's workflow.

**Graphite** works well if your whole team buys in, but it requires a cloud account, wraps your push workflow through their service, and adds a dashboard layer on top of GitHub. For what should be a local git operation, that's a lot of surface area and a SaaS dependency you might not want.

**ghstack** is clever but it rewrites your branches into synthetic ones that GitHub can display as individual PRs. What's on your machine doesn't match what's on GitHub, and that impedance mismatch gets confusing when you're debugging a rebase or trying to understand why a PR diff looks different than what you see locally.

**git-branchless** is powerful but it's a fundamentally different way of thinking about git. It's inspired by Mercurial and Phabricator, and if your team already does one-branch-per-PR with squash merges, that abstraction doesn't map to your workflow without rewiring your mental model.

`gw` takes a different approach. Your branches are real git branches. Your PRs are normal GitHub PRs. Your reviewers see normal diffs. Nothing gets rewritten and nothing gets synced to a cloud service. It just handles the grunt work: propagating rebases through the chain when you push fixes, detecting squash merges when PRs land, and cleaning up the stack so you don't have to.

The key design decision is that `gw sync` does not rebase your stack onto the latest base branch unless a branch was actually merged or you explicitly ask for it with `--rebase`. Your stack stays pinned to the base commit it was created from. That means your open PRs don't get force pushed just because someone else merged to main, and your reviewers' progress doesn't get blown away. When you do want to update, you decide when.

## Install

```
git clone https://github.com/deibeljc/git-workflow.git
cd git-workflow
cargo install --path .
```

Needs a [Rust toolchain](https://rustup.rs/). Optional: `gh` CLI for auto-detecting squash merges and showing PR status in `gw log`.

### Shell completions

Tab completion for commands, flags, and branch names:

```bash
# zsh (add to ~/.zshrc)
source <(gw completions zsh)

# bash (add to ~/.bashrc)
eval "$(gw completions bash)"

# fish
gw completions fish | source
```

### Claude Code integration

`gw` has a built-in MCP server so Claude Code can manage your stacks directly. Set it up with one command:

```bash
gw mcp-setup
```

This writes the MCP config to `.mcp.json` and you're good to go. Restart Claude Code and it can use gw tools natively.

## Quick start

```bash
# Set your base branch (if not main)
gw config set-base dev

# Create a stack
gw stack create auth

# Do work, commit, then add the next branch
gw branch create auth-tests

# See everything (just `gw` also works)
gw log

# Address PR feedback on auth, then propagate rebases
gw rebase

# Push when ready
gw push

# After auth gets squash-merged
gw sync

# Explicitly rebase onto latest base when you're ready
gw sync --rebase

# Switch between branches interactively
gw switch
```

## Already have branches? Adopt them

If you've already got a chain of branches you've been managing by hand, you don't need to recreate anything. Just tell gw about them:

```bash
gw adopt feature-api feature-tests feature-ui --base dev
```

The argument order defines the stack order, so `feature-api` becomes the root and `feature-ui` becomes the leaf. If the branches aren't already rebased into a chain, gw handles that for you and asks before making changes. You can also name the stack explicitly with `--name` or let it default to the first branch name.

This is the easiest way to migrate onto gw. You keep all your existing branches and commits, gw just starts tracking the relationships between them.

## Configuration

Config lives in `.git/gw/config.toml` and is per-repo. View it with `gw config show`.

| Setting | Default | Command | Description |
| --- | --- | --- | --- |
| `default_base` | auto-detected | `gw config set-base <branch>` | Base branch for new stacks (e.g. `dev`, `main`) |
| `delete_on_merge` | `false` | `gw config set-delete-on-merge true` | Delete local branches after sync detects they were merged |

By default, `gw sync` removes merged branches from the stack but keeps the local git branches around in case you need them. If you'd rather have sync clean everything up automatically:

```bash
gw config set-delete-on-merge true
```

## Commands

| Command | What it does |
| --- | --- |
| `gw` | Show all stacks (alias for `gw log`) |
| `gw log` | Show all stacks with branches and commits |
| `gw log --pr` | Include PR status from GitHub |
| `gw stack create <name>` | Create a new stack off the base branch |
| `gw stack delete <name>` | Remove stack metadata (branches stay) |
| `gw stack list` | List all stacks |
| `gw branch create <name>` | Add a branch to the current stack |
| `gw branch remove <name>` | Remove a branch and re-parent children |
| `gw adopt <branches...>` | Adopt existing branches into a stack |
| `gw rebase` | Propagate rebases to descendants |
| `gw rebase --continue` | Resume after resolving conflicts |
| `gw rebase --abort` | Roll back all branches |
| `gw sync` | Fetch base, detect merges, rebase stack |
| `gw sync --rebase` | Explicitly rebase stack onto latest base |
| `gw sync --merged <branch>` | Manually indicate a branch was merged |
| `gw push` | Push the current branch |
| `gw switch [branch]` | Switch branches interactively or by name |
| `gw config set-base <branch>` | Set the default base branch |
| `gw config set-delete-on-merge <bool>` | Auto-delete local branches on merge |
| `gw config show` | Show current configuration |
| `gw completions <shell>` | Generate shell completions (zsh/bash/fish) |
| `gw mcp-setup` | Configure the MCP server for Claude Code |
