# Research: Programmatic Git Rebase Automation Best Practices

**Date:** 2026-03-05
**Status:** Complete
**Sources:** git-branchless (Rust), git-spice (Go), git-town (Go), git official docs, community patterns

---

## 1. Detecting Rebase Conflicts Programmatically

### Exit Codes

Git rebase returns a **non-zero exit code** (typically 1 or 128) when it encounters conflicts. The official docs only explicitly document exit code 1 for `--exec` command failures, but in practice:

- **Exit 0** = rebase completed successfully
- **Exit 1** = rebase was interrupted (conflict, deliberate break, or exec failure)
- **Exit 128** = fatal error (not a rebase conflict, something else went wrong)

**Best practice from git-spice:** Don't rely on specific exit codes to distinguish conflict types. Instead, check the exit code for non-zero, then inspect filesystem state to determine what happened.

```rust
// Pattern: Run rebase, check exit code, then inspect state
let status = Command::new("git")
    .args(["rebase", "--onto", onto, upstream, branch])
    .status()?;

if !status.success() {
    // Check if we're now in a rebase state (conflict)
    // vs. a hard failure (bad args, etc.)
    if rebase_in_progress()? {
        return Ok(RebaseResult::Conflict);
    } else {
        return Err(anyhow!("rebase failed unexpectedly"));
    }
}
```

### Detecting Active Rebase via Filesystem

Git stores rebase state in one of two directories depending on the backend:

- `.git/rebase-merge/` (merge backend, the default in modern git)
- `.git/rebase-apply/` (apply backend, legacy)

**Key files inside `.git/rebase-merge/`:**

| File | Contents |
|------|----------|
| `head-name` | Full ref name being rebased, e.g. `refs/heads/feature` |
| `orig-head` | Original HEAD OID before rebase started |
| `onto` | The commit being rebased onto |
| `git-rebase-todo` | Remaining rebase instructions |
| `end` | Total number of rebase commands |
| `msgnum` | Current command number |
| `interactive` | Empty marker file (present if interactive) |

**Best practice from git-spice (authoritative pattern):**

```go
// Check both possible state directories
for _, backend := range []string{"rebase-merge", "rebase-apply"} {
    stateDir := filepath.Join(gitDir, backend)
    if _, err := os.Stat(stateDir); err != nil {
        if errors.Is(err, os.ErrNotExist) {
            continue
        }
        return err
    }

    // Read branch being rebased
    head, err := os.ReadFile(filepath.Join(stateDir, "head-name"))
    if err != nil {
        continue
    }

    branchRef := strings.TrimSpace(string(head))
    branch := strings.TrimPrefix(branchRef, "refs/heads/")
    return &RebaseState{Branch: branch, Backend: backend}
}
return ErrNoRebase
```

**Rust equivalent for gw:**

```rust
fn rebase_in_progress(git_dir: &Path) -> Result<Option<RebaseState>> {
    for dir_name in &["rebase-merge", "rebase-apply"] {
        let state_dir = git_dir.join(dir_name);
        if state_dir.exists() {
            let head_name = fs::read_to_string(state_dir.join("head-name"))
                .ok()
                .map(|s| s.trim().trim_start_matches("refs/heads/").to_string());
            return Ok(Some(RebaseState {
                branch: head_name,
                state_dir,
            }));
        }
    }
    Ok(None)
}
```

### Detecting Conflicted Files

After a rebase stops due to conflict, use `git status --porcelain` to detect unmerged files:

```
UU = both modified (most common conflict)
AA = both added
DD = both deleted
AU = added by us
UA = added by them
DU = deleted by us
UD = deleted by them
```

**Programmatic check:**

```bash
git diff --name-only --diff-filter=U
```

Returns the list of conflicted files. Empty output = no conflicts remaining (user resolved them).

---

## 2. Implementing "rebase --continue" from an External Tool

### The Pattern

All three tools (git-branchless, git-spice, git-town) follow the same approach: **delegate to `git rebase --continue`** rather than reimplementing rebase logic.

**git-spice's approach (cleanest pattern for a wrapper tool):**

1. Check that a rebase is actually in progress (`RebaseState()`)
2. Call `git rebase --continue`
3. Inspect the result:
   - If exit 0 AND no rebase state remains: rebase completed successfully
   - If exit 0 AND rebase state still exists: deliberate break (interactive edit)
   - If exit non-zero AND rebase state exists: another conflict was hit
   - If exit non-zero AND no rebase state: unexpected failure

```rust
fn rebase_continue(&self) -> Result<RebaseResult> {
    let status = Command::new("git")
        .args(["rebase", "--continue"])
        .status()?;

    if status.success() {
        // Check if rebase is truly done or just paused deliberately
        if self.rebase_in_progress()? {
            return Ok(RebaseResult::Paused); // deliberate break
        }
        return Ok(RebaseResult::Success);
    }

    // Non-zero exit: check if it's a conflict or hard failure
    if self.rebase_in_progress()? {
        return Ok(RebaseResult::Conflict);
    }

    Err(anyhow!("rebase --continue failed unexpectedly"))
}
```

### Continuation State for Multi-Branch Propagation

git-spice uses a **continuation stack** pattern that's directly relevant to gw's `state.toml`:

1. Before starting propagation, record the full list of branches to rebase
2. On conflict, save the remaining work as "continuations"
3. On `--continue`, first finish the current git rebase, then pop continuations and execute them
4. If another conflict hits during a continuation, push remaining continuations back onto the stack

**This maps directly to gw's `state.toml` design with `completed` and `remaining` arrays.** The key insight is that `gw rebase --continue` should:

1. Check for `.git/gw/state.toml` (gw's propagation state)
2. Check for `.git/rebase-merge/` (git's rebase state)
3. If git rebase is in progress, run `git rebase --continue` first
4. If that succeeds, continue with the next branch in `remaining`
5. If it conflicts again, update `state.toml` and pause

### Detecting User Has Resolved Conflicts

Before calling `git rebase --continue`, verify conflicts are resolved:

```bash
# Returns non-empty if conflicts remain
git diff --name-only --diff-filter=U
```

If there are still unmerged files, `git rebase --continue` will fail. You can pre-check this to give a better error message.

Additionally, git-spice suppresses git's own advice messages with `-c advice.mergeConflict=false` and provides its own, which is a nice UX pattern for wrapper tools.

---

## 3. Safely Restoring Branches to Pre-Rebase State

### git update-ref (the standard approach)

`git update-ref` is the correct tool for programmatically moving branch pointers. All three reference tools use it (or the equivalent) for rollback.

**Basic usage:**

```bash
# Move a branch to a specific commit
git update-ref refs/heads/<branch> <commit-sha>

# Safe update with old-value verification (prevents race conditions)
git update-ref refs/heads/<branch> <new-sha> <expected-old-sha>
```

**Atomic batch updates via --stdin (best practice for multi-branch rollback):**

```bash
git update-ref --stdin <<EOF
start
update refs/heads/feature-tests aaa1111
update refs/heads/feature-ui bbb2222
commit
EOF
```

This is **atomic**: either all refs are updated or none are. This is critical for `gw rebase --abort` where you need to restore multiple branches simultaneously.

**Rust implementation pattern:**

```rust
fn restore_branches(original_refs: &[(String, String)]) -> Result<()> {
    let mut child = Command::new("git")
        .args(["update-ref", "--stdin"])
        .stdin(Stdio::piped())
        .spawn()?;

    let stdin = child.stdin.as_mut().unwrap();
    writeln!(stdin, "start")?;
    for (branch, commit) in original_refs {
        writeln!(stdin, "update refs/heads/{} {}", branch, commit)?;
    }
    writeln!(stdin, "commit")?;
    drop(child.stdin.take());

    let status = child.wait()?;
    if !status.success() {
        bail!("failed to restore branches");
    }
    Ok(())
}
```

### Abort Flow

The complete abort pattern (combining git-spice and git-branchless insights):

1. Check if git rebase is in progress, if so run `git rebase --abort`
2. Read `state.toml` for `original_refs`
3. Use `git update-ref --stdin` to atomically restore all branches
4. Remove `state.toml`
5. Checkout the branch the user was on before the operation

**Important edge case from git-spice:** If the user already ran `git rebase --abort` manually, your tool should still clean up its own state (drain continuations/remove state.toml). git-spice handles this gracefully by checking for rebase state before trying to abort, and proceeding with state cleanup regardless.

---

## 4. Detecting Squash Merges (Branch Merged into Dev)

This is the hardest problem. Squash merges create a single new commit on the target branch, so `git merge-base --is-ancestor` won't detect them (the branch's commits aren't ancestors of dev).

### Approach 1: Forge API (Recommended Primary)

**git-spice's approach (most robust):**

- For branches submitted via the tool: query the forge API for CR (Change Request) status directly using stored change metadata
- For branches submitted manually: call `FindChangesByBranch()` to find PRs by branch name, check their state
- Compare local HEAD SHA with remote HEAD SHA to detect if there are unpushed commits before declaring a branch merged

This maps to gw's plan of using `gh pr list --head <branch> --state merged`.

### Approach 2: Tree Comparison (Best Offline Fallback)

**The `git commit-tree` + `git cherry` technique** (from community, widely used by tools like git-delete-squashed):

```bash
# For each branch, check if its tree is already represented in dev
ancestor=$(git merge-base dev $branch)
tree=$(git rev-parse $branch^{tree})
temp_commit=$(git commit-tree $tree -p $ancestor -m "temp")
result=$(git cherry dev $temp_commit)

if [[ "$result" == "-"* ]]; then
    echo "$branch was squash-merged into dev"
fi
```

**How it works:**

1. Find the merge base between dev and the branch
2. Get the branch's final tree (the full state of files at the branch tip)
3. Create a synthetic squash commit: all branch changes collapsed into one commit, parented at the merge base
4. Use `git cherry` to check if dev contains an equivalent commit (by patch-id of the synthetic commit vs. commits on dev)

**Why this works and `git patch-id` alone doesn't:**

- `git patch-id` compares individual commits, but a squash merge combines N commits into 1
- The `commit-tree` trick creates a single synthetic commit that represents the same final diff as the squash merge
- `git cherry` then uses patch-id internally to compare that one synthetic commit against dev's commits

**Rust implementation:**

```rust
fn is_squash_merged(branch: &str, base: &str) -> Result<bool> {
    let ancestor = git_run(&["merge-base", base, branch])?;
    let tree = git_run(&["rev-parse", &format!("{}^{{tree}}", branch)])?;
    let temp_commit = git_run(&[
        "commit-tree", &tree, "-p", &ancestor, "-m", "temp"
    ])?;
    let cherry = git_run(&["cherry", base, &temp_commit])?;
    Ok(cherry.starts_with('-'))
}
```

**Limitations:**

- Won't detect squash merges where the PR reviewer made additional changes before merging
- Won't detect squash merges if the branch was further rebased/amended between push and merge
- Can produce false positives if two branches produce identical diffs

### Approach 3: Fallback with `IsAncestor` (Catches Regular Merges Only)

**git-spice's local fallback (for unsupported forges):**

```go
// Only catches regular merges and fast-forwards, NOT squash merges
if repo.IsAncestor(ctx, branchHead, trunkHash) {
    // branch was merged (non-squash)
}
```

### Recommended Strategy for gw

Based on the research, your brainstorm's hybrid approach is sound. Recommended priority order:

1. **Primary:** `gh pr list --head <branch> --state merged` (reliable, handles all merge strategies)
2. **Fallback:** Tree comparison via `commit-tree` + `cherry` (handles squash merges offline)
3. **Manual:** `gw sync --merged <branch>` (user override)

Your plan doc already rejected `git patch-id` alone (correct), but the `commit-tree` + `cherry` technique is a viable automated fallback that your brainstorm didn't consider. It's worth adding as the fallback instead of requiring `--merged` for users without `gh`.

---

## 5. Detecting Divergence Between Local and Remote Branches

### The Pattern

For force-push decisions, you need to know if local and remote branches have diverged (neither is an ancestor of the other).

**Three states to detect:**

| Local vs Remote | Meaning | Push Type |
|----------------|---------|-----------|
| Local == Remote | Up to date | No push needed |
| Remote is ancestor of Local | Local is ahead | Regular `git push` |
| Local is ancestor of Remote | Remote is ahead (someone pushed) | Pull first |
| Neither is ancestor | Diverged | `git push --force-with-lease` |

**Implementation:**

```rust
enum BranchDivergence {
    UpToDate,
    LocalAhead,
    RemoteAhead,
    Diverged,
}

fn check_divergence(branch: &str, remote: &str) -> Result<BranchDivergence> {
    let local = git_run(&["rev-parse", branch])?;
    let remote_ref = format!("{}/{}", remote, branch);

    // Check if remote tracking branch exists
    let remote_sha = match git_run(&["rev-parse", "--verify", &remote_ref]) {
        Ok(sha) => sha,
        Err(_) => return Ok(BranchDivergence::LocalAhead), // never pushed
    };

    if local == remote_sha {
        return Ok(BranchDivergence::UpToDate);
    }

    let local_is_ancestor = git_run(&[
        "merge-base", "--is-ancestor", &local, &remote_sha
    ]).is_ok();

    let remote_is_ancestor = git_run(&[
        "merge-base", "--is-ancestor", &remote_sha, &local
    ]).is_ok();

    match (local_is_ancestor, remote_is_ancestor) {
        (false, true) => Ok(BranchDivergence::LocalAhead),
        (true, false) => Ok(BranchDivergence::RemoteAhead),
        _ => Ok(BranchDivergence::Diverged),
    }
}
```

### Force-with-Lease Best Practices

**git-spice's approach:** Uses `--force-with-lease=<ref>:<expected-oid>` with the specific expected OID, not just bare `--force-with-lease`. This is safer because it prevents overwriting changes pushed by someone else between your fetch and your push.

```rust
fn push_force_with_lease(branch: &str, remote: &str) -> Result<()> {
    // Get the current remote tracking ref value
    let remote_ref = format!("{}/{}", remote, branch);
    let expected = git_run(&["rev-parse", &remote_ref])?;

    Command::new("git")
        .args([
            "push",
            "--force-with-lease",
            &format!("{}:refs/heads/{}", branch, branch),
            remote,
        ])
        .status()?;
    Ok(())
}
```

**Important:** Always `git fetch` before checking divergence, or your remote tracking refs may be stale.

### Alternative: `git rev-list --left-right --count`

A more concise way to get ahead/behind counts:

```bash
git rev-list --left-right --count feature...origin/feature
# Output: "3\t1" means 3 ahead, 1 behind
```

---

## 6. Race Condition Prevention for File-Based State (TOML Metadata)

### The Problem

If `gw` is invoked concurrently (unlikely but possible via scripts, CI, or accidental double-execution), two processes could read/modify `.git/gw/stacks/*.toml` or `state.toml` simultaneously, causing corruption.

### Approach 1: Atomic Write via Temp File + Rename (Recommended for v1)

This is the simplest approach and prevents partial writes:

```rust
use std::io::Write;
use tempfile::NamedTempFile;

fn write_toml_atomic(path: &Path, content: &str) -> Result<()> {
    let dir = path.parent().unwrap();
    let mut tmp = NamedTempFile::new_in(dir)?;
    tmp.write_all(content.as_bytes())?;
    tmp.persist(path)?;
    Ok(())
}
```

**Why `new_in(dir)`:** The temp file must be on the same filesystem as the target for `rename()` to be atomic.

**This prevents:**
- Partial writes (crash mid-write leaves old file intact)
- Readers seeing half-written content

**This does NOT prevent:**
- Two writers simultaneously reading stale state and both writing

### Approach 2: File Locking (Recommended if concurrency becomes real)

Use `fs2` crate for advisory file locks:

```rust
use fs2::FileExt;

fn with_state_lock<F, T>(git_dir: &Path, f: F) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    let lock_path = git_dir.join("gw").join("lock");
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&lock_path)?;

    lock_file.lock_exclusive()?; // blocks until lock acquired
    let result = f();
    lock_file.unlock()?;
    result
}
```

**git-spice's approach:** Uses a `sync.RWMutex` in-process (since it's a single-process tool), and relies on git's own ref-level locking for the storage backend (their state lives in a git ref, not flat files). Their `GitBackend` wraps all operations in `mu.RLock()`/`mu.Lock()`.

### Approach 3: State File as Lock (Simplest for gw)

Your `state.toml` already serves as a natural lock for propagation operations:

1. Before starting propagation, check if `state.toml` exists (another operation in progress)
2. Write `state.toml` atomically (temp file + rename)
3. Any command that sees `state.toml` knows an operation is in progress and blocks

**This is exactly what your plan describes as the "state guard" pattern, and it's the right approach.** The existence of `state.toml` is itself the lock for multi-step operations.

For stack TOML files, the risk is lower because:
- Stack metadata changes are small and fast (not multi-step)
- Concurrent gw usage on the same stack is an unusual edge case
- Atomic writes via temp file + rename are sufficient protection

### Recommended Strategy for gw

**v1 (your plan already covers this):**
- Atomic writes for all TOML files (temp file + rename via `tempfile` crate)
- `state.toml` existence as propagation lock (state guard pattern)
- No file locking needed

**v2 (if concurrency becomes a real problem):**
- Add `fs2` file locking around state mutations
- Or store state in a git ref like git-spice does (but this adds significant complexity)

---

## Summary of Tool Patterns

| Concern | git-branchless | git-spice | git-town | Recommended for gw |
|---------|---------------|-----------|----------|-------------------|
| Conflict detection | In-memory rebase first, fall back to on-disk | Exit code + filesystem state check | Rebase strategy config | Exit code + `.git/rebase-merge/` check |
| Rebase continue | `git rebase --continue` delegation | `git rebase --continue` + continuation stack | `git rebase --continue` | Same + `state.toml` remaining list |
| Branch restore | `repo.create_reference()` with OID map | `git update-ref` (via ref-based state) | N/A (different model) | `git update-ref --stdin` atomic batch |
| Squash detection | Not primary focus | Forge API (primary) + `IsAncestor` (fallback) | N/A | `gh` API (primary) + `commit-tree`/`cherry` (fallback) |
| Divergence | Not primary focus | SHA comparison + `IsAncestor` | Sync strategy config | `merge-base --is-ancestor` both directions |
| State safety | In-memory + event log | Git ref storage + RWMutex | Git config | Atomic file writes + state guard |
