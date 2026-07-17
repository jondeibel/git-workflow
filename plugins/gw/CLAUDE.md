# gw - Always Use for Branch Management

This repo uses **gw** for all branch and stack management. Use gw MCP tools instead of raw git for any operation involving branches, pushing, rebasing, or syncing.

## Quick Rules

- **Push**: Use `gw_push` or `gw_force_push`, never `git push`
- **Create branches**: Use `gw_branch_create`, never `git checkout -b`
- **Switch branches**: Use `gw_switch`, never `git checkout <branch>`
- **Rebase**: Use `gw_rebase`, never `git rebase`
- **Sync with remote**: Use `gw_sync` or `gw_sync_rebase`, never `git pull`
- **View stack state**: Use `gw_overview` first; use `gw_log` only when commit history is needed
- **Inspect current work**: Use `gw_status` and `gw_diff_stat` for stack-aware context
- **Raw git is fine for**: `git add`, `git commit`, focused file diffs, `git log`, `git stash`

## Before Any Git Workflow Operation

Always run `gw_overview` first to understand the current stack state before creating branches, pushing, or creating PRs. It skips commit history and is the fast path for large repositories.

## Creating PRs

When creating PRs for stacked branches, create them bottom-up. The root branch PR targets the base branch (e.g. `main`), and each child branch PR targets its parent branch. Use `gw_overview_pr` to check existing PR status; use `gw_log_pr` only when commits are also needed.

## Starting New Work

1. Run `gw_overview` to check existing stacks
2. Create a stack with `gw_stack_create`
3. Commit with git as normal
4. When the next logical unit starts, create the next branch with `gw_branch_create`

## After Pushing Changes to an Earlier Branch

Run `gw_rebase` after committing to a branch that has descendants, so the rebase propagates through the chain.
