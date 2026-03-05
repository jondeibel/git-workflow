---
title: "feat: Build gw - Git Stacked Branch Manager"
type: feat
status: active
date: 2026-03-05
deepened: 2026-03-05
origin: docs/brainstorms/2026-03-05-gw-stacked-branches-brainstorm.md
---

# feat: Build gw - Git Stacked Branch Manager

## Enhancement Summary

**Deepened on:** 2026-03-05
**Sections enhanced:** 8
**Research agents used:** Architecture Strategist, Performance Oracle, Security Sentinel, Code Simplicity Reviewer, Git Rebase Automation Best Practices, Context7 (clap docs)

### Key Improvements
1. Extracted a shared propagation engine used by `rebase`, `sync`, `branch remove`, and `adopt` to eliminate duplicated conflict-handling logic
2. Added input validation and security hardening (command injection prevention, path traversal protection, atomic file writes)
3. Optimized performance-critical paths: batched `gh` calls, batched branch validation, eliminated unnecessary base branch checkout during sync
4. Discovered a viable offline squash merge detection fallback using `git commit-tree` + `git cherry`, reducing hard dependency on `gh`
5. Simplified data model: dropped `position` field (array order is sufficient), dropped `toml_edit` dependency

### New Considerations Discovered
- `gw rebase --abort` needs `git update-ref --stdin` with transaction semantics for atomic multi-ref rollback
- State file must be updated BEFORE starting each rebase (not after) for crash recovery correctness
- Branch/stack names require validation against command injection and path traversal attacks
- `gw tree` with `gh` calls per-branch would take 5-10 seconds; must batch into single `gh pr list` call

## Overview

Build `gw`, a Rust CLI tool that manages stacked branches on top of plain git. It tracks parent-child relationships between branches in a stack, automatically propagates rebases to descendants, detects squash-merged branches via GitHub PR numbers, and provides tree visualization of stack state. All metadata lives in `.git/gw/` (local-only). The tool shells out to git for all operations rather than using libgit2.

## Problem Statement

Working with stacked branches in git is manual and error-prone. When you modify a branch in the middle of a stack, you have to manually rebase every descendant. When a branch gets squash-merged into dev, you have to manually detect the merge, remove the branch, and rebase the rest. There's no way to visualize the stack structure, and it's easy to lose track of relationships between branches. Existing tools like Graphite and ghstack are cloud-dependent or opinionated in ways that don't fit every workflow.

## Proposed Solution

A standalone Rust binary (`gw`) that wraps git with stack-aware metadata. It stores stack definitions as TOML files in `.git/gw/stacks/`, tracks propagation state in `.git/gw/state.toml`, and provides commands for creating, syncing, visualizing, and managing stacked branches. (see brainstorm: docs/brainstorms/2026-03-05-gw-stacked-branches-brainstorm.md)

## Technical Approach

### Architecture

```
gw (binary)
├── cli.rs                 # clap derive structs for all commands/subcommands
├── main.rs                # Entry point: parse args, build Context, state guard, dispatch
├── context.rs             # Context struct bundling Git, paths, loaded state
├── commands/
│   ├── mod.rs
│   ├── stack.rs           # gw stack create, gw stack delete, gw stack list
│   ├── branch.rs          # gw branch create, gw branch remove
│   ├── adopt.rs           # gw adopt
│   ├── rebase.rs          # gw rebase, gw rebase --continue, gw rebase --abort
│   ├── sync.rs            # gw sync
│   ├── push.rs            # gw push
│   └── tree.rs            # gw tree
├── git.rs                 # Git struct wrapping std::process::Command
├── propagation.rs         # Shared rebase propagation engine (used by rebase, sync, adopt, branch remove)
├── state.rs               # Stack/propagation state types, TOML load/save, atomic writes
├── validate.rs            # Input validation for branch names, stack names, path safety
└── ui.rs                  # Colored output, tree rendering, prompts
```

### Research Insights: Architecture

**Context struct pattern** (from Architecture Strategist): Every command handler receives a `Context` struct that bundles the `Git` instance, the resolved `.git/gw/` path, and loaded state. This keeps wiring in `main.rs` and business logic in command modules, eliminating duplicated setup.

```rust
pub struct Context {
    pub git: Git,
    pub gw_dir: PathBuf,       // .git/gw/
    pub stacks_dir: PathBuf,   // .git/gw/stacks/
}

impl Context {
    pub fn from_current_dir() -> Result<Self> { ... }
    pub fn load_stack(&self, name: &str) -> Result<StackConfig> { ... }
    pub fn load_all_stacks(&self) -> Result<Vec<StackConfig>> { ... }
    pub fn save_stack(&self, stack: &StackConfig) -> Result<()> { ... }
    pub fn propagation_state(&self) -> Result<Option<PropagationState>> { ... }
}
```

**Shared propagation engine** (critical finding): `rebase`, `sync`, `branch remove`, and `adopt` all trigger rebase cascades that can conflict. Extract the propagation engine into `propagation.rs` so all four commands share identical conflict-handling, state tracking, and rollback logic. The `operation` field in `state.toml` already distinguishes the source command.

```rust
// propagation.rs
pub struct PropagationEngine<'a> {
    ctx: &'a Context,
    operation: Operation,  // Rebase, Sync, Adopt, BranchRemove
}

impl<'a> PropagationEngine<'a> {
    pub fn propagate(&self, branches: &[BranchToRebase]) -> Result<PropagationResult> { ... }
    pub fn continue_propagation(&self) -> Result<PropagationResult> { ... }
    pub fn abort(&self) -> Result<()> { ... }
}
```

**Key crates:**
- `clap` v4 (derive API) for CLI parsing with nested subcommands
- `serde` + `toml` for reading AND writing stack metadata (no `toml_edit`, see simplification notes)
- `anyhow` for error handling with context
- `colored` for terminal output
- `tempfile` for atomic file writes
- `assert_cmd` + `tempfile` + `predicates` for testing

### Research Insights: Simplification

Based on the Code Simplicity review, these items were simplified or deferred:

- **Dropped `toml_edit`**: These are machine-generated TOML files in `.git/gw/` that no human edits. `toml::to_string_pretty` for writes is sufficient. One fewer dependency, simpler write path.
- **Dropped `position` field from branch entries**: Array order in TOML `[[branches]]` is the canonical ordering. Eliminates a consistency invariant (positions must be sequential with no gaps) that would need maintaining on every mutation.
- **Removed branch naming suggestions**: No convention is defined in the data model. Users will name branches whatever they want. Pure YAGNI.

### Data Model

**Stack metadata** (`.git/gw/stacks/<stack-name>.toml`):
```toml
name = "auth"
base_branch = "dev"

[[branches]]
name = "feature/auth"

[[branches]]
name = "feature/auth-tests"

[[branches]]
name = "feature/auth-ui"
```

Stacks are strictly linear: each branch has exactly one parent and at most one child. Array order defines the stack order (first = root/closest to base, last = leaf/furthest from base). (see brainstorm: decision #4)

**Note on "tip" terminology**: The brainstorm uses "tip" to mean the branch closest to dev (base of the stack). In git, "tip" usually means the latest commit (furthest point). This plan uses **"root"** for the branch closest to the base and **"leaf"** for the branch furthest from the base. Resolve this naming before writing any user-facing strings.

**Propagation state** (`.git/gw/state.toml`, only exists during active propagation):
```toml
operation = "rebase"  # or "sync", "adopt", "branch_remove"
stack = "auth"
started_at = "2026-03-05T14:30:00Z"
original_branch = "feature/auth"  # branch user was on before propagation

# Pre-operation refs for full rollback
[[original_refs]]
branch = "feature/auth-tests"
commit = "aaa1111"

[[original_refs]]
branch = "feature/auth-ui"
commit = "bbb2222"

# Propagation progress
completed = ["feature/auth-tests"]  # already rebased successfully
remaining = ["feature/auth-ui"]     # still to do
current = "feature/auth-ui"         # currently in-progress (or conflicted)
```

### Research Insights: State Management

**Update state BEFORE starting each rebase** (from Architecture Strategist): If the process crashes between a successful rebase and the state file update, `--continue` would incorrectly re-rebase an already-moved branch. Write `state.toml` with `current` set to the next branch BEFORE calling `git rebase`. If the rebase succeeds, move to next. If it fails or crashes, `--continue` correctly retries the `current` branch (git rebase is resumable).

**Atomic file writes** (from Security Sentinel + Best Practices Research): Use `tempfile::NamedTempFile` to write to a temp file in the same directory, then `persist()` (atomic rename). This prevents corruption from power loss, Ctrl-C, or concurrent access. Apply to ALL TOML writes.

```rust
use tempfile::NamedTempFile;
use std::io::Write;

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let dir = path.parent().context("no parent directory")?;
    let mut tmp = NamedTempFile::new_in(dir)?;
    tmp.write_all(content.as_bytes())?;
    tmp.persist(path)?;
    Ok(())
}
```

**Full rollback via `git update-ref --stdin`** (from Best Practices Research): For `gw rebase --abort`, use `git update-ref --stdin` with `start`/`update`/`commit` transaction semantics to atomically restore all branches to their pre-propagation refs. This is how git-branchless and git-spice handle multi-ref restoration.

```
start
update refs/heads/feature/auth-tests aaa1111
update refs/heads/feature/auth-ui bbb2222
commit
```

### State Guard

Every `gw` command checks for `.git/gw/state.toml` on startup. If an active propagation exists:
- `gw rebase --continue` and `gw rebase --abort` are allowed
- `gw tree` is allowed (read-only)
- All other commands are blocked with: "A rebase propagation is in progress. Run `gw rebase --continue` or `gw rebase --abort` first."

### Input Validation

### Research Insights: Security

**Command injection prevention** (from Security Sentinel, severity: HIGH): Branch and stack names come from user input and get passed as arguments to `git` and `gh`. The `Git::run` method MUST use `Command::new("git").args(args)` (array form), never shell string concatenation. Rust's `std::process::Command` does not invoke a shell, so arguments are passed directly to exec. But git itself parses its arguments, so a branch named `--force` could be misinterpreted.

**Path traversal prevention** (from Security Sentinel, severity: HIGH): Stack names are used to construct file paths (`.git/gw/stacks/<name>.toml`). A name like `../../config` would write to `.git/config`. Validate and canonicalize.

**Validation rules** (implement in `validate.rs`):

```rust
/// Validate a stack name for filesystem safety and git compatibility
pub fn validate_stack_name(name: &str) -> Result<()> {
    // Must not be empty
    // Must match: [a-zA-Z0-9][a-zA-Z0-9_-]*
    // Must not contain: / \ .. null bytes, control characters
    // Must not start with: -
    // After constructing path, canonicalize and verify it's under .git/gw/stacks/
}

/// Validate a branch name for git safety
pub fn validate_branch_name(name: &str) -> Result<()> {
    // Must not start with: -
    // Must not contain: .. (two consecutive dots), control characters, ~ ^ : ? * [ \
    // Must not end with: .lock, /
    // Consider: delegate to `git check-ref-format --branch <name>`
}
```

Apply validation at TWO boundaries:
1. CLI input (when user provides names)
2. TOML deserialization (when reading stored values, since files could be manually edited)

**Additional security measures:**
- Prefix refs with `refs/heads/` when passing to git to avoid refspec ambiguity
- When stdin is not a TTY, default to "no" on destructive prompts (force-push), or require `--yes` flag
- Strip `GIT_DIR` and `GIT_WORK_TREE` env vars when spawning git to prevent context leakage

### Git Interaction Layer

A `Git` struct that wraps `std::process::Command`:

```rust
pub struct Git {
    repo_path: PathBuf,
}

impl Git {
    fn run(&self, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.repo_path)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .output()
            .context("failed to execute git")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "git {} failed (exit {}): {}",
                args.join(" "),
                output.status.code().unwrap_or(-1),
                stderr.trim()
            ));
        }

        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    }

    fn current_branch(&self) -> Result<String> { ... }
    fn create_branch(&self, name: &str) -> Result<()> { ... }
    fn checkout(&self, branch: &str) -> Result<()> { ... }
    fn rebase(&self, onto: &str) -> Result<RebaseResult> { ... }
    fn push(&self, branch: &str) -> Result<()> { ... }
    fn push_force_with_lease(&self, branch: &str, expected: &str) -> Result<()> { ... }
    fn rev_parse(&self, refspec: &str) -> Result<String> { ... }
    fn merge_base(&self, a: &str, b: &str) -> Result<String> { ... }
    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool> { ... }
    fn has_diverged_from_remote(&self, branch: &str) -> Result<bool> { ... }
    fn is_working_tree_clean(&self) -> Result<bool> { ... }
    fn branch_exists(&self, name: &str) -> Result<bool> { ... }
    fn all_local_branches(&self) -> Result<HashSet<String>> { ... }
    fn commit_count_between(&self, base: &str, head: &str) -> Result<usize> { ... }
    fn update_ref_transaction(&self, updates: &[(String, String)]) -> Result<()> { ... }
    fn is_rebase_in_progress(&self) -> bool { ... }
    fn fetch_branch(&self, remote: &str, branch: &str) -> Result<()> { ... }
    fn update_local_ref(&self, branch: &str, remote: &str) -> Result<()> { ... }
}
```

### Research Insights: Git Interaction Performance

**Batch branch existence validation** (from Performance Oracle): Instead of calling `git branch --list <name>` N times (one per branch), call `git branch --list` once (returns all branches), parse into a `HashSet<String>`, and check membership in-memory. Collapses N subprocess spawns into 1.

```rust
fn all_local_branches(&self) -> Result<HashSet<String>> {
    let output = self.run(&["branch", "--format=%(refname:short)"])?;
    Ok(output.lines().map(|s| s.to_string()).collect())
}
```

**Scope validation to relevant stacks** (from Performance Oracle): Don't load and validate all stacks eagerly. `gw push` only needs the current branch's stack. `gw rebase` only needs the current branch's stack. Load all stacks only for `gw tree` and `gw sync`.

### Research Insights: Rebase Conflict Detection

**Detecting conflicts programmatically** (from Best Practices Research, sourced from git-spice): Check the exit code of `git rebase` for non-zero, then inspect for `.git/rebase-merge/` or `.git/rebase-apply/` directories. Their presence confirms a conflict state vs. a hard failure.

```rust
pub enum RebaseResult {
    Success,
    Conflict,  // .git/rebase-merge/ exists
    Error(String),
}

fn rebase(&self, onto: &str) -> Result<RebaseResult> {
    let output = Command::new("git")
        .args(&["rebase", onto])
        .current_dir(&self.repo_path)
        .output()?;

    if output.status.success() {
        return Ok(RebaseResult::Success);
    }

    // Check for conflict state
    if self.repo_path.join(".git/rebase-merge").exists()
        || self.repo_path.join(".git/rebase-apply").exists()
    {
        return Ok(RebaseResult::Conflict);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(anyhow!("git rebase failed: {}", stderr.trim()))
}
```

**Before calling `--continue`**, verify no unresolved conflicts remain:
```
git diff --name-only --diff-filter=U
```
If this produces output, there are still unresolved files.

### Error Handling Strategy

### Research Insights: Error Design

**Typed results for the Git layer, anyhow for everything else** (from Architecture Strategist): Use typed enums (`RebaseResult`) where callers need to pattern-match on distinct outcomes (conflict vs. success). Use `anyhow::Result` with `.context()` for infrastructure errors (TOML parse, filesystem, git not found). Document this convention at the top of `git.rs`.

### Command Specifications

#### `gw stack create <name>` (optionally `--base <branch>`)
1. Validate stack name via `validate_stack_name`
2. Verify we're in a git repo
3. Determine base branch: `--base` flag, or current branch
4. Verify no stack with this name exists
5. Verify no git branch with this name exists (if it does, suggest `gw adopt`)
6. Create a new git branch at the base branch HEAD
7. Write `.git/gw/stacks/<name>.toml` atomically
8. Checkout the new branch
9. Print confirmation

#### `gw stack delete <name>`
1. Verify the stack exists
2. Remove `.git/gw/stacks/<name>.toml`
3. Print the list of git branches that are no longer managed (branches are NOT deleted)

#### `gw stack list`
1. Read all `.git/gw/stacks/*.toml` files
2. Print stack names with branch counts

#### `gw branch create <name>`
1. Validate branch name via `validate_branch_name`
2. Verify current branch is tracked by a stack
3. Verify current branch is the leaf of the stack (strictly linear)
4. Create git branch at HEAD
5. Update stack TOML: append branch to array
6. Checkout the new branch

**If not on a tracked branch:** Error with "Current branch is not tracked by any gw stack. Use `gw stack create` to start a new stack or `gw adopt` to track existing branches."

#### `gw branch remove <name>`
1. Find which stack contains the branch
2. If the branch has a child, re-parent the child to the removed branch's parent
3. Use the **propagation engine** to rebase the child (and its descendants) onto the new parent
4. Update stack TOML: remove the branch from array
5. Git branch is NOT deleted, just untracked
6. If removing the only branch in a stack, suggest `gw stack delete` instead

**Removing the root branch:** The next branch becomes the new root.

#### `gw adopt <branch-a> <branch-b> ... [--base <branch>]`
1. Validate all branch names
2. Infer base branch: `--base` flag, or compute `git merge-base` of first branch against well-known bases (dev, develop, main, master) and pick the nearest
3. Verify no existing stack contains any of these branches
4. Check if branches are already chained in the given order
5. If not chained: warn "This will rebase branches to form a chain. Continue? [y/N]" (skip with `--yes`)
6. Use the **propagation engine** for rebasing
7. Create stack TOML with inferred name (first branch name, or `--name` flag)
8. Print the resulting stack structure

#### `gw rebase`
1. Verify current branch is tracked
2. Verify working tree is clean (error if dirty: "You have uncommitted changes. Commit or stash before running this command.")
3. Find all descendants of the current branch in the stack
4. If no descendants, print "No descendant branches to rebase" and exit
5. Delegate to the **propagation engine**

#### `gw rebase --continue`
1. Verify `state.toml` exists with active propagation
2. Verify no unresolved conflicts (`git diff --name-only --diff-filter=U`)
3. Delegate to `PropagationEngine::continue_propagation`

#### `gw rebase --abort`
1. Verify `state.toml` exists with active propagation
2. Abort current git rebase if one is in progress
3. Restore ALL branches atomically via `git update-ref --stdin` transaction
4. Remove `state.toml`
5. Checkout original branch (from `state.toml`'s `original_branch`)
6. Print "Rebase propagation aborted. All branches restored to their previous state."

#### `gw sync [--stack <name>] [--merged <branch>]`
1. Determine which stacks to sync: all stacks sharing the base branch (default) or a specific stack via `--stack`
2. Verify working tree is clean
3. Fetch the base branch: `git fetch origin <base>` then `git update-ref refs/heads/<base> origin/<base>` (avoids checking out the base branch, saving subprocess spawns and working tree churn)
4. For each stack being synced:
   a. Check if the root branch was merged into the updated base
   b. **Detection priority:**
      - If `--merged <branch>` was passed: treat that branch as merged
      - If `gh` is available and authenticated: `gh pr list --head <root-branch> --state merged --json headRefName` (single call, see performance notes)
      - If `gh` is unavailable: try tree comparison fallback (see below)
      - If nothing works: print "Cannot auto-detect. Use `gw sync --merged <branch>`"
   c. If merged: remove the root from the stack, promote the next branch
   d. Repeat merge detection for the new root (handles multiple consecutive merges)
   e. Use the **propagation engine** to rebase the remaining stack onto the updated base
5. Return to the branch the user was on before sync

### Research Insights: Squash Merge Detection

**Tree comparison fallback** (from Best Practices Research): When `gh` is unavailable, use a `git commit-tree` + `git cherry` technique to detect if a branch's changes are already present in the base. This creates a synthetic squash commit and checks for equivalence:

```bash
# Get the merge-base between the root branch and the updated base
MERGE_BASE=$(git merge-base root-branch dev)

# Create a synthetic squash commit using the root branch's tree
SYNTHETIC=$(git commit-tree root-branch^{tree} -p $MERGE_BASE -m "synthetic")

# Check if dev contains an equivalent patch
git cherry dev $SYNTHETIC
```

If `git cherry` returns a line starting with `-`, the patch is already in dev (meaning the branch was merged). This works for squash merges because it compares the resulting tree state, not individual commit SHAs.

**Detection priority becomes:** `--merged` flag > `gh` API > tree comparison > manual prompt

#### `gw push`
1. Verify current branch is tracked
2. Check if the branch has diverged from its remote tracking branch
3. If diverged:
   - If stdin is a TTY: prompt "Branch has diverged from remote. Force push with lease? [y/N]"
   - If stdin is not a TTY: error "Branch has diverged. Use --yes to force push with lease in non-interactive mode."
4. If not diverged: regular `git push`
5. Use `--force-with-lease=<ref>:<expected-oid>` with explicit expected SHA for safety
6. Only pushes the current branch, never descendants

#### `gw tree`
1. Read all stack TOML files
2. Batch-validate branch existence: single `git branch --list` call, check membership
3. Batch-fetch PR status: single `gh pr list --json number,headRefName,state` call (if gh available), filter client-side
4. For each stack:
   a. Print the base branch
   b. For each branch in order:
      - Print with tree connectors (Unicode box-drawing characters)
      - Mark the root with "(root)"
      - Show commit count ahead of parent
      - Mark current branch with `*`
      - Show PR status from batch results (best-effort, omit if gh fails)
      - Check remote tracking: show "needs push" if local is ahead of remote
5. Highlight the current branch in the output

### Research Insights: Tree Performance

**Batch ALL external calls** (from Performance Oracle): Without batching, a 10-branch stack with `gh` available would require 20+ git subprocess spawns plus 10 `gh` invocations (5-10 seconds). With batching:

| Call | Before | After |
|------|--------|-------|
| Branch existence | N `git branch --list <name>` calls | 1 `git branch --format=%(refname:short)` |
| PR status | N `gh pr view` calls | 1 `gh pr list --json number,headRefName,state` |
| Commit counts | N `git rev-list --count` calls | N calls (hard to batch, but fast at ~5ms each) |
| Remote divergence | N calls | N calls (could parallel with threads, defer for v1) |

**Estimated wall time after batching:** 100-300ms for 10 branches without network, 300-600ms with one `gh` call.

Example output:
```
dev
└── feature/auth (root) * ← PR #42 open
    ├── 3 commits ahead of dev
    └── feature/auth-tests [needs push]
        └── feature/auth-ui
            └── 2 commits ahead of feature/auth-tests

dev
└── billing/api (root)
    └── billing/ui
```

Commit counts are ahead of parent branch (maps to what would be in a single PR).

### Untracked Branch Behavior Matrix

| Command | On tracked branch | On untracked branch | On base branch |
|---------|------------------|--------------------|----|
| `stack create` | Works (uses current as base) | Works (uses current as base) | Works (intended use) |
| `stack delete` | Works | Works | Works |
| `stack list` | Works | Works | Works |
| `branch create` | Works (auto-detects stack) | Error: "not tracked" | Error: "not tracked, use stack create" |
| `branch remove` | Works | Works (takes branch name arg) | Works (takes branch name arg) |
| `adopt` | Works | Works | Works |
| `rebase` | Works | Error: "not tracked" | Error: "not tracked" |
| `sync` | Works | Works (syncs all/specified stacks) | Works (syncs all/specified stacks) |
| `push` | Works | Error: "not tracked" | Error: "not tracked" |
| `tree` | Works | Works | Works |

### TOML/Git State Divergence Handling

On every command that reads stack state, gw validates that tracked branches still exist in git (using the batched `all_local_branches()` call):
- If a branch in a stack no longer exists: warn "Branch 'X' no longer exists in git. Run `gw branch remove X` to clean up." Continue with remaining branches.
- If ALL branches in a stack are gone: warn "Stack 'Y' has no remaining branches. Run `gw stack delete Y` to clean up."
- Never auto-delete metadata. Always require explicit user action.

### Init / First Run

No explicit `gw init` command. gw auto-creates `.git/gw/` and `.git/gw/stacks/` on the first command that needs to write metadata. If not in a git repo, error: "Not a git repository. Run `git init` first."

### Implementation Phases

#### Phase 1: Foundation

Project scaffolding, core infrastructure, and security fundamentals.

**Tasks:**
- Initialize Rust project with `cargo init`
- Set up `Cargo.toml` with dependencies:
  ```toml
  [dependencies]
  clap = { version = "4", features = ["derive"] }
  serde = { version = "1", features = ["derive"] }
  toml = "0.8"
  anyhow = "1"
  colored = "3"
  tempfile = "3"

  [dev-dependencies]
  assert_cmd = "2"
  predicates = "3"
  tempfile = "3"
  ```
- Implement `validate.rs`: stack name and branch name validation (security-critical, must be Phase 1)
- Implement `Git` struct in `git.rs` with core methods: `run`, `current_branch`, `rev_parse`, `branch_exists`, `all_local_branches`, `create_branch`, `checkout`, `is_working_tree_clean`, `is_rebase_in_progress`
- Implement `Context` struct in `context.rs`
- Implement state types in `state.rs`: `StackConfig`, `BranchEntry`, load/save TOML with atomic writes
- Implement CLI skeleton in `cli.rs` with clap derive structs for all commands (parsing only, no logic)
- Implement `main.rs` with Context creation, state guard check, and command dispatch
- Implement `.git/gw/` auto-creation
- Set up test infrastructure: `TestRepo` helper in `tests/common/mod.rs`

**CLI structure (from Context7 clap docs):**
```rust
#[derive(Parser)]
#[command(name = "gw", version, about = "Git stacked branch manager")]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    Stack(StackArgs),
    Branch(BranchArgs),
    Adopt(AdoptArgs),
    Rebase(RebaseArgs),
    Sync(SyncArgs),
    Push(PushArgs),
    Tree,
}

#[derive(Args)]
#[command(flatten_help = true)]
struct StackArgs {
    #[command(subcommand)]
    command: StackCommands,
}

#[derive(Subcommand)]
enum StackCommands {
    Create { name: String, #[arg(long)] base: Option<String> },
    Delete { name: String },
    List,
}
```

**Success criteria:** `gw --help` works, `gw stack list` runs (returns empty), `Git` struct can execute commands against a real repo, input validation rejects malicious names.

**Files:**
- `Cargo.toml`
- `src/main.rs`
- `src/cli.rs`
- `src/context.rs`
- `src/git.rs`
- `src/state.rs`
- `src/validate.rs`
- `src/ui.rs`
- `src/commands/mod.rs`
- `tests/common/mod.rs`

#### Phase 2: Stack and Branch Management

Core CRUD operations for stacks and branches.

**Tasks:**
- Implement `gw stack create` in `src/commands/stack.rs`
- Implement `gw stack delete` in `src/commands/stack.rs`
- Implement `gw stack list` in `src/commands/stack.rs`
- Implement `gw branch create` in `src/commands/branch.rs` with auto-detection of parent stack
- Implement `gw branch remove` in `src/commands/branch.rs` with re-parenting (uses propagation engine for rebase)
- Implement `gw adopt` in `src/commands/adopt.rs` with chain detection, rebase via propagation engine, and confirmation prompt
- Add `merge_base`, `is_ancestor`, `fetch_branch`, `update_local_ref` methods to `Git` struct
- State divergence validation on command entry (batched)
- Integration tests for all stack/branch CRUD operations

**Success criteria:** Can create stacks, add branches, adopt existing branches, remove branches with re-parenting, delete stacks. All operations correctly update TOML metadata.

**Files:**
- `src/commands/stack.rs`
- `src/commands/branch.rs`
- `src/commands/adopt.rs`
- `src/git.rs` (additions)
- `src/ui.rs` (prompts)
- `tests/stack_test.rs`
- `tests/branch_test.rs`
- `tests/adopt_test.rs`

#### Phase 3: Rebase Propagation Engine

The shared propagation engine with conflict handling and rollback. This is the heart of the tool.

**Tasks:**
- Implement `PropagationEngine` in `src/propagation.rs`:
  - Write `state.toml` with original refs BEFORE starting propagation
  - Update `current` field BEFORE each rebase (crash recovery)
  - Detect conflicts via exit code + `.git/rebase-merge/` directory
  - Support `continue` and `abort` operations
  - Full rollback via `git update-ref --stdin` transaction
- Implement `gw rebase` in `src/commands/rebase.rs` (delegates to engine)
- Implement state guard in `main.rs`: check for active propagation before dispatching any command
- Add `RebaseResult` enum, `update_ref_transaction`, `is_rebase_in_progress` to `Git` struct
- Dirty working tree detection before rebase operations
- Integration tests: successful propagation, conflict handling, continue, abort with full rollback, crash recovery (state.toml exists but no git rebase in progress)

**Success criteria:** Can propagate rebases through a stack, handle conflicts with pause/continue/abort, full rollback restores all branches atomically.

**Files:**
- `src/propagation.rs`
- `src/commands/rebase.rs`
- `src/state.rs` (propagation state additions)
- `src/main.rs` (state guard)
- `src/git.rs` (RebaseResult, update_ref_transaction)
- `tests/rebase_test.rs`

#### Phase 4: Sync and Push

Squash merge detection and controlled pushing.

**Tasks:**
- Implement `gw sync` in `src/commands/sync.rs`
- Implement `gh` CLI detection: check if `gh` is installed and authenticated (`gh auth status`)
- Implement batch PR merge detection: single `gh pr list --state merged --json headRefName` call
- Implement tree comparison fallback for squash merge detection (commit-tree + cherry)
- Implement `gw sync --merged <branch>` manual fallback
- Implement iterative root promotion (handle multiple consecutive merges)
- Avoid checking out base branch: use `git fetch` + `git update-ref` instead
- Implement `gw push` in `src/commands/push.rs` with divergence detection, TTY check, and `--force-with-lease=ref:sha` with explicit expected SHA
- Integration tests for sync (with mocked gh output), push with divergence

**Success criteria:** Sync detects merged branches via gh (or tree comparison fallback), promotes roots, rebases remaining stack. Push prompts before force pushing (refuses in non-interactive mode without --yes). Manual `--merged` fallback works when gh is unavailable.

**Files:**
- `src/commands/sync.rs`
- `src/commands/push.rs`
- `src/git.rs` (push methods, tree comparison)
- `tests/sync_test.rs`
- `tests/push_test.rs`

#### Phase 5: Tree Visualization

Rich terminal output showing stack state.

**Tasks:**
- Implement `gw tree` in `src/commands/tree.rs`
- Build tree renderer in `ui.rs` using Unicode box-drawing characters
- Batch all external calls: single `git branch` for existence, single `gh pr list` for PR status
- Show: branch names, root marker, current branch marker, commit counts ahead of parent, "needs push" indicator
- Colored output: current branch highlighted, root branch labeled, "needs push" in yellow
- Handle multiple stacks in output
- Handle empty state (no stacks)
- Integration tests for tree output formatting

**Success criteria:** `gw tree` displays all stacks with accurate branch relationships, commit counts, current branch highlighting, and optional PR status. Completes in under 500ms for 10 branches.

**Files:**
- `src/commands/tree.rs`
- `src/ui.rs` (tree rendering)
- `tests/tree_test.rs`

#### Phase 6: Polish and Edge Cases

Hardening, error messages, and quality of life.

**Tasks:**
- Improve error messages across all commands (contextual hints, suggestions)
- Handle all "not on tracked branch" cases consistently per the behavior matrix
- Handle TOML/git state divergence (warn about missing branches)
- Add `--verbose` global flag for debugging (show git commands being run)
- Add shell completions generation (`clap_complete`) - cut if running behind
- Add `gw --version` with build info
- Ensure all operations return user to their original branch on failure
- End-to-end integration tests covering the full workflow (create stack, add branches, commit, rebase propagate, sync after merge, push)
- README with installation and usage

**Success criteria:** Tool handles edge cases gracefully, provides helpful error messages, and the full workflow end-to-end test passes.

**Files:**
- Various (error handling improvements across all command files)
- `src/cli.rs` (global flags, completions)
- `tests/e2e_test.rs`
- `README.md`

## Alternative Approaches Considered

1. **libgit2 via git2-rs** - Rejected because rebase implementation is significantly harder than shelling out to git. The metadata layer is simple enough that in-process git access adds complexity without proportional benefit. (see brainstorm: "Why This Approach")

2. **Git subcommand plugin (`git-gw`)** - Rejected for brevity. `gw` is faster to type than `git gw`. Can be added later as a symlink. (see brainstorm: "Why This Approach")

3. **Branching stack topology** - Rejected for v1. Strictly linear stacks keep the data model as a simple ordered list and simplify every algorithm. Can revisit if real need emerges.

4. **Auto-stash for dirty working trees** - Rejected. Explicit "commit or stash first" error is simpler and avoids stash conflict edge cases.

5. **`git patch-id` as squash merge fallback** - Rejected after investigation. Squash merges create a single new commit, so patch-id comparison against individual branch commits won't match. Replaced with tree comparison technique (commit-tree + cherry) as offline fallback.

6. **`toml_edit` for format-preserving writes** - Rejected. These are machine-generated files that no human edits. `toml::to_string_pretty` is sufficient. One fewer dependency.

7. **`position` field in branch entries** - Rejected. Array order is the canonical ordering. The position field would create a consistency invariant that has to be maintained on every mutation.

## Acceptance Criteria

### Functional Requirements

- [ ] `gw stack create` creates a stack with TOML metadata in `.git/gw/stacks/`
- [ ] `gw stack delete` removes metadata without deleting git branches
- [ ] `gw stack list` shows all stacks with branch counts
- [ ] `gw branch create` adds a branch to the current stack with auto-detection
- [ ] `gw branch remove` re-parents children and rebases them via propagation engine
- [ ] `gw adopt` takes existing branches and rebases them into a chain with confirmation
- [ ] `gw rebase` propagates rebases to all descendants with conflict handling
- [ ] `gw rebase --continue` resumes propagation after conflict resolution
- [ ] `gw rebase --abort` fully rolls back all branches atomically via `update-ref --stdin`
- [ ] `gw sync` pulls base, detects merged roots (gh > tree comparison > manual), auto-promotes, rebases remaining stack
- [ ] `gw sync --merged` provides manual merge indication without `gh`
- [ ] `gw sync` handles multiple consecutively merged branches in one run
- [ ] `gw push` pushes current branch only, prompts before force-with-lease (refuses in non-interactive without --yes)
- [ ] `gw tree` displays all stacks with branch relationships, commit counts, and current branch highlighting
- [ ] State guard blocks commands during active rebase propagation
- [ ] Dirty working tree detection prevents rebase operations
- [ ] Missing branch detection warns without auto-deleting metadata
- [ ] Input validation rejects malicious branch/stack names

### Non-Functional Requirements

- [ ] All metadata stored in `.git/gw/` (never pushed to remote)
- [ ] Single binary with no runtime dependencies beyond git (and optionally gh)
- [ ] Commands complete in under 1 second for typical stack sizes (< 10 branches)
- [ ] `gw tree` completes in under 500ms with batched external calls
- [ ] Helpful error messages with contextual suggestions for every failure mode
- [ ] All TOML writes are atomic (temp file + rename)

### Quality Gates

- [ ] Integration tests for every command's happy path
- [ ] Integration tests for conflict handling, abort/continue flow
- [ ] Integration tests for "not on tracked branch" behavior
- [ ] Integration tests for input validation (malicious names rejected)
- [ ] End-to-end test covering full create-branch-commit-rebase-sync-push workflow

## Dependencies and Prerequisites

- Rust toolchain (stable)
- git (installed and on PATH)
- gh CLI (optional, for squash merge detection and PR status in tree; tree comparison fallback works without it)

## Risk Analysis and Mitigation

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Rebase conflicts break propagation state | High | Medium | Shared propagation engine + full rollback via `update-ref --stdin` transaction |
| TOML metadata drifts from git state | Medium | Medium | Batched branch validation on every command, warn without auto-fixing |
| `gh` CLI not available or not authenticated | Medium | Low | Three-tier detection: gh > tree comparison > `--merged` manual fallback |
| Command injection via branch names | Low | High | Input validation in `validate.rs`, array-form `Command`, refs/heads/ prefixing |
| Path traversal via stack names | Low | High | Input validation + path canonicalization in `validate.rs` |
| Git version differences in output format | Low | Medium | Parse git output conservatively, test against multiple git versions |
| Concurrent gw usage corrupts TOML | Low | Medium | Atomic writes (temp + rename). Advisory file lock deferred to v2 |
| Process crash mid-propagation | Medium | Medium | State updated BEFORE each rebase, `--continue` retries current branch |

## Sources and References

### Origin

- **Brainstorm document:** [docs/brainstorms/2026-03-05-gw-stacked-branches-brainstorm.md](docs/brainstorms/2026-03-05-gw-stacked-branches-brainstorm.md) - Key decisions carried forward: thin git wrapper approach, TOML metadata in `.git/gw/`, root = base of stack, hybrid squash merge detection, stop-and-continue conflict handling, strictly linear stacks

### Research

- **Git rebase automation best practices** - Patterns from git-branchless, git-spice, and git-town for conflict detection (`.git/rebase-merge/`), continuation stacks, `update-ref --stdin` transactions, and tree comparison for squash merge detection
- **Architecture review** - Context struct pattern, shared propagation engine extraction, state update ordering for crash recovery
- **Performance analysis** - Subprocess cost model on macOS (~5-15ms per spawn), batching strategies for branch validation and gh calls
- **Security review** - Input validation requirements, command injection prevention, path traversal protection, atomic write patterns

### External References

- clap v4 derive API: https://docs.rs/clap/latest/clap/_derive/index.html
- toml crate: https://docs.rs/toml/latest/toml/
- assert_cmd for CLI testing: https://docs.rs/assert_cmd/latest/assert_cmd/
- tempfile crate (atomic writes): https://docs.rs/tempfile/latest/tempfile/
- gh CLI: https://cli.github.com/manual/
