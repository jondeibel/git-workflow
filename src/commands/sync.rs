use anyhow::{bail, Result};

use crate::cli::SyncArgs;
use crate::context::Ctx;
use crate::propagation::{self, PropagationResult};
use crate::state::Operation;
use crate::ui;

pub fn run(args: SyncArgs, ctx: &Ctx) -> Result<()> {
    if !ctx.git.is_working_tree_clean()? {
        bail!("You have uncommitted changes. Commit or stash before running this command.");
    }

    let original_branch = ctx.git.current_branch()?;

    // Load stacks to sync
    let stacks = if let Some(ref name) = args.stack {
        vec![ctx.load_stack(name)?]
    } else {
        ctx.load_all_stacks()?
    };

    if stacks.is_empty() {
        ui::info("No stacks to sync.");
        return Ok(());
    }

    // Group stacks by base branch
    let mut synced_bases = std::collections::HashSet::new();

    for mut stack in stacks {
        let base = &stack.base_branch;

        // Fetch and update base branch (once per unique base)
        if synced_bases.insert(base.clone()) {
            ui::info(&format!("Fetching {base}..."));
            match ctx.git.fetch_branch("origin", base) {
                Ok(_) => {
                    // Update local ref to match remote without checkout
                    let _ = ctx
                        .git
                        .update_local_ref(base, &format!("origin/{base}"));
                }
                Err(_) => {
                    ui::warn(&format!(
                        "Could not fetch origin/{base}. Continuing with local state."
                    ));
                }
            }
        }

        if stack.branches.is_empty() {
            continue;
        }

        // Check for merged root branches
        let mut merged_any = false;
        loop {
            let root = match stack.branches.first() {
                Some(b) => b.name.clone(),
                None => break,
            };

            let is_merged = if let Some(ref merged_branch) = args.merged {
                // Manual override
                *merged_branch == root
            } else {
                detect_merged(ctx, &root, &stack.base_branch)?
            };

            if is_merged {
                ui::info(&format!(
                    "Detected: '{root}' was merged into {}",
                    stack.base_branch
                ));
                stack.branches.remove(0);
                merged_any = true;

                if stack.branches.is_empty() {
                    ui::info(&format!(
                        "All branches in stack '{}' have been merged!",
                        stack.name
                    ));
                    ctx.save_stack(&stack)?;
                    break;
                }

                let new_root = &stack.branches[0].name;
                ui::info(&format!("New root: '{new_root}'"));

                // Only process one manual --merged per sync
                if args.merged.is_some() {
                    break;
                }
            } else {
                break;
            }
        }

        if merged_any {
            ctx.save_stack(&stack)?;
        }

        // Rebase remaining stack onto the updated base
        if !stack.branches.is_empty() {
            let branches: Vec<String> =
                stack.branches.iter().map(|b| b.name.clone()).collect();

            // Build onto targets: first branch onto base, rest onto their parent
            let mut targets = Vec::new();
            for branch_name in &branches {
                let parent = stack.parent_of(branch_name).unwrap();
                targets.push(parent);
            }

            ui::info(&format!(
                "Rebasing {} branch{} onto {}...",
                branches.len(),
                if branches.len() == 1 { "" } else { "es" },
                stack.base_branch
            ));

            match propagation::start(
                ctx,
                Operation::Sync,
                &stack.name,
                &branches,
                &targets,
            )? {
                PropagationResult::Success { rebased_count } => {
                    ui::success(&format!(
                        "Stack '{}' synced. {rebased_count} branch{} rebased.",
                        stack.name,
                        if rebased_count == 1 { "" } else { "es" }
                    ));
                }
                PropagationResult::Conflict { branch } => {
                    ui::warn(&format!(
                        "Conflict while syncing stack '{}' at branch '{branch}'.",
                        stack.name
                    ));
                    ui::info("Resolve conflicts and run `gw rebase --continue`.");
                    // Return to let user handle conflict
                    return Ok(());
                }
            }
        }
    }

    // Return to original branch if possible
    if ctx.git.branch_exists(&original_branch)? {
        let _ = ctx.git.checkout(&original_branch);
    }

    Ok(())
}

/// Detect if a branch has been merged into the base branch.
/// Uses gh CLI if available, falls back to tree comparison, then gives up.
fn detect_merged(ctx: &Ctx, branch: &str, base: &str) -> Result<bool> {
    // Try gh CLI first
    if let Ok(result) = detect_merged_via_gh(branch) {
        return Ok(result);
    }

    // Try tree comparison (commit-tree + cherry)
    if let Ok(result) = detect_merged_via_tree(ctx, branch, base) {
        return Ok(result);
    }

    // Can't detect
    Ok(false)
}

/// Detect merge via gh CLI: check if there's a merged PR for this branch.
fn detect_merged_via_gh(branch: &str) -> Result<bool> {
    let output = std::process::Command::new("gh")
        .args([
            "pr",
            "list",
            "--head",
            branch,
            "--state",
            "merged",
            "--json",
            "headRefName",
            "--limit",
            "1",
        ])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            // If the JSON array is non-empty, there's a merged PR
            let trimmed = stdout.trim();
            Ok(trimmed != "[]" && !trimmed.is_empty())
        }
        _ => bail!("gh not available"),
    }
}

/// Detect merge via tree comparison:
/// Create a synthetic squash commit and use git cherry to check equivalence.
fn detect_merged_via_tree(ctx: &Ctx, branch: &str, base: &str) -> Result<bool> {
    // Get the merge base
    let merge_base = ctx.git.merge_base(branch, base)?;

    // Create a synthetic squash commit
    let tree = ctx.git.run(&["rev-parse", &format!("{branch}^{{tree}}")])?;
    let synthetic = ctx.git.run(&[
        "commit-tree",
        &tree,
        "-p",
        &merge_base,
        "-m",
        "synthetic squash for merge detection",
    ])?;

    // Use git cherry to check if base contains an equivalent patch
    let cherry_output = ctx.git.run(&["cherry", base, &synthetic])?;

    // If cherry returns a line starting with "-", the patch is already in base
    Ok(cherry_output.lines().any(|line| line.starts_with('-')))
}
