---
title: "feat: Add interactive branch switch command"
type: feat
status: completed
date: 2026-03-05
---

# feat: Add interactive branch switch command

## Overview

Add a `gw switch` command that lets you interactively switch between gw-tracked branches using an arrow-key selector. When given a branch name argument, it skips the picker and checks out directly.

## Problem Statement / Motivation

Right now switching between stacked branches means remembering exact branch names and running `git checkout <name>`. When you've got multiple stacks with several branches each, that gets tedious. A `gw switch` command with an interactive picker makes it fast to jump between branches without leaving the gw workflow.

## Proposed Solution

### CLI interface

```
gw switch [branch]
```

- **No argument**: show an interactive arrow-key picker (via `dialoguer`) listing all gw-tracked branches grouped by stack
- **With argument**: directly checkout the named branch (must be tracked by gw)

### Interactive picker display

Branches are grouped by stack, showing the stack/base context and marking the current branch:

```
? Switch to branch:
  auth > auth         (root)
> auth > auth-tests   (current)
  auth > auth-ui
  billing > billing   (root)
```

- Format: `{stack} > {branch}` with tags for `root` and `current`
- Current branch is pre-selected (cursor starts there)
- Uses `dialoguer::FuzzySelect` for type-to-filter support
- In non-interactive mode (piped stdin), print an error and exit

### Direct argument mode

```
gw switch auth-tests
```

- Look up the branch across all stacks
- If found and exists locally, `git checkout` it
- If not tracked by gw, error with suggestion to use `git checkout` directly
- No clean-tree requirement (same as `git checkout`, let git handle dirty state)

## Technical Considerations

### New dependency

```toml
# Cargo.toml
dialoguer = "0.11"
```

`dialoguer` provides `FuzzySelect` with arrow-key navigation and type-to-filter. It's the standard Rust crate for interactive terminal prompts (same ecosystem as `indicatif`, `console`).

### Files to create/modify

#### `src/cli.rs`
- Add `Switch(SwitchArgs)` variant to `Commands` enum
- Add `SwitchArgs` struct with optional positional `branch: Option<String>`

```rust
/// Switch to a branch tracked by gw
Switch(SwitchArgs),
```

```rust
#[derive(Args)]
pub struct SwitchArgs {
    /// Branch name to switch to (interactive picker if omitted)
    pub branch: Option<String>,
}
```

#### `src/commands/switch.rs` (new file)
- `run(args, ctx)` function following the same pattern as other commands
- Load all stacks, collect branches, build display strings
- If `args.branch` is Some, find and checkout directly
- If None, show `FuzzySelect` picker
- Handle non-TTY gracefully (bail with message)

```rust
use anyhow::{bail, Result};
use dialoguer::FuzzySelect;

use crate::context::Ctx;
use crate::ui;

pub fn run(branch: Option<String>, ctx: &Ctx) -> Result<()> {
    let stacks = ctx.load_all_stacks()?;

    if stacks.is_empty() {
        bail!("No stacks. Create one with `gw stack create <name>`.");
    }

    // Collect all tracked branches with their stack context
    let mut entries: Vec<(String, String, bool)> = vec![]; // (display, branch_name, is_root)
    let current = ctx.git.current_branch().unwrap_or_default();

    for stack in &stacks {
        for (i, b) in stack.branches.iter().enumerate() {
            entries.push((stack.name.clone(), b.name.clone(), i == 0));
        }
    }

    if let Some(target) = branch {
        // Direct mode
        if !entries.iter().any(|(_, name, _)| name == &target) {
            bail!("Branch '{target}' is not tracked by gw. Use `git checkout` for untracked branches.");
        }
        ctx.git.checkout(&target)?;
        ui::success(&format!("Switched to {target}"));
        return Ok(());
    }

    // Interactive mode
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        bail!("Interactive mode requires a terminal. Pass a branch name: `gw switch <branch>`");
    }

    // Build display items
    let items: Vec<String> = entries.iter().map(|(stack, name, is_root)| {
        let mut label = format!("{stack} > {name}");
        let mut tags = vec![];
        if *is_root { tags.push("root"); }
        if *name == current { tags.push("current"); }
        if !tags.is_empty() {
            label.push_str(&format!("  ({})", tags.join(", ")));
        }
        label
    }).collect();

    // Pre-select current branch
    let default = entries.iter().position(|(_, name, _)| name == &current).unwrap_or(0);

    let selection = FuzzySelect::new()
        .with_prompt("Switch to branch")
        .items(&items)
        .default(default)
        .interact()?;

    let target = &entries[selection].1;
    if *target == current {
        ui::info("Already on that branch.");
        return Ok(());
    }

    ctx.git.checkout(target)?;
    ui::success(&format!("Switched to {target}"));
    Ok(())
}
```

#### `src/commands/mod.rs`
- Add `pub mod switch;`

#### `src/main.rs`
- Add `Commands::Switch(args) => commands::switch::run(args.branch, &ctx),`
- Allow `Switch` through the propagation state guard (read-only command, like `Tree`)

#### `Cargo.toml`
- Add `dialoguer = "0.11"` to `[dependencies]`

### State guard

`gw switch` should be allowed during propagation (like `gw tree`), since switching branches is a read-only navigation action. Add it to the `allowed` match in `main.rs`.

## Acceptance Criteria

- [x] `gw switch` with no args shows interactive arrow-key picker with all tracked branches
- [x] Picker groups branches by stack name with `{stack} > {branch}` format
- [x] Picker supports type-to-filter (fuzzy search)
- [x] Current branch is pre-selected and tagged `(current)`
- [x] Root branches are tagged `(root)`
- [x] Selecting a branch checks it out via `git checkout`
- [x] Selecting the current branch prints "Already on that branch." and exits cleanly
- [x] `gw switch <name>` directly checks out named branch without picker
- [x] `gw switch <name>` errors if branch not tracked by gw
- [x] Non-interactive stdin (piped) prints error with usage hint
- [x] Command is allowed during propagation state (like `gw tree`)
- [x] Integration tests cover direct mode, error cases, and non-interactive fallback

## Dependencies & Risks

- **New dependency**: `dialoguer 0.11` adds ~3 crates to the dependency tree (`dialoguer`, `console`, `indicatif` transitively). These are well-maintained and widely used.
- **Testing interactive prompts**: `dialoguer::FuzzySelect` reads from stdin directly, so integration tests can only cover the direct-argument path and the non-interactive error. Interactive behavior would need manual testing.

## Sources & References

- Similar command patterns: `src/commands/push.rs`, `src/commands/tree.rs`
- Context loading: `src/context.rs:81` (`load_all_stacks`)
- State guard: `src/main.rs:14-33`
- TTY detection pattern: `src/ui.rs:32` (`atty_check`)
- CLI structure: `src/cli.rs`
