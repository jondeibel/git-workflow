use anyhow::{bail, ensure, Context, Result};

use crate::context::Ctx;
use crate::git::RebaseResult;
use crate::state::{Operation, OriginalRef, PropagationState};
use crate::ui;

/// Result of a propagation attempt.
pub enum PropagationResult {
    /// All branches rebased successfully.
    Success { rebased_count: usize },
    /// A conflict was encountered. State saved for continue/abort.
    Conflict { branch: String },
}

/// Start a new propagation: rebase a list of branches in order.
/// Each branch is rebased onto the one before it (or the given base for the first).
///
/// `branches_to_rebase` are in topological order (first = child of current, last = leaf).
/// `onto_targets[i]` is what `branches_to_rebase[i]` should be rebased onto.
/// `upstream_overrides` optionally provides the old parent for --onto rebases (needed
/// after squash merges where the old parent's commits are already in the target).
pub fn start(
    ctx: &Ctx,
    operation: Operation,
    stack_name: &str,
    branches_to_rebase: &[String],
    onto_targets: &[String],
) -> Result<PropagationResult> {
    start_with_upstreams(ctx, operation, stack_name, branches_to_rebase, onto_targets, &[])
}

/// Like `start`, but with explicit upstream overrides for --onto rebases.
pub fn start_with_upstreams(
    ctx: &Ctx,
    operation: Operation,
    stack_name: &str,
    branches_to_rebase: &[String],
    onto_targets: &[String],
    upstream_overrides: &[Option<String>],
) -> Result<PropagationResult> {
    if branches_to_rebase.is_empty() {
        return Ok(PropagationResult::Success { rebased_count: 0 });
    }

    ensure!(
        branches_to_rebase.len() == onto_targets.len(),
        "branches and targets must have same length"
    );

    let original_branch = ctx.git.current_branch()?;

    // Collect pre-rebase refs for all branches
    let original_refs: Vec<OriginalRef> = branches_to_rebase
        .iter()
        .map(|b| {
            let commit = ctx.git.rev_parse(&format!("refs/heads/{b}"))?;
            Ok(OriginalRef {
                branch: b.clone(),
                commit,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    // Write initial state BEFORE starting any rebases
    let state = PropagationState {
        operation,
        stack: stack_name.to_string(),
        started_at: chrono_now(),
        original_branch: original_branch.clone(),
        original_refs,
        completed: vec![],
        remaining: branches_to_rebase.to_vec(),
        current: None,
    };
    ctx.save_propagation_state(&state)?;

    // Execute the propagation
    execute_propagation(
        ctx,
        branches_to_rebase,
        onto_targets,
        upstream_overrides,
        0,
        branches_to_rebase.len(),
    )
}

/// Continue a previously paused propagation.
pub fn continue_propagation(ctx: &Ctx) -> Result<PropagationResult> {
    let state = ctx
        .propagation_state()?
        .context("No propagation in progress.")?;

    // Check for unresolved conflicts
    if ctx.git.has_unresolved_conflicts()? {
        bail!(
            "There are still unresolved conflicts.\n\
             Resolve them and run `git add`, then `gw rebase --continue`."
        );
    }

    let completed_count = state.completed.len();
    let has_current = state.current.is_some();
    let remaining_count = state.remaining.len();
    let total = completed_count + if has_current { 1 } else { 0 } + remaining_count;

    // Show resume context
    if completed_count > 0 {
        ui::info(&format!(
            "Resuming: {} of {} branch{} already rebased",
            completed_count,
            total,
            if total == 1 { "" } else { "es" }
        ));
    }

    // If a git rebase is in progress, continue it first
    if ctx.git.is_rebase_in_progress() {
        let current_branch = state.current.clone().unwrap_or_default();
        let step = completed_count + 1;
        match ctx.git.rebase_continue()? {
            RebaseResult::Success => {
                ui::step_ok(
                    step,
                    total,
                    &format!("Rebased '{current_branch}' (conflict resolved)"),
                );
            }
            RebaseResult::Conflict => {
                // Still conflicting after continue
                let branch = state.current.unwrap_or_default();
                return Ok(PropagationResult::Conflict { branch });
            }
        }
    }

    // Figure out what's left to do
    let remaining = state.remaining.clone();
    let offset = completed_count + if has_current { 1 } else { 0 };

    if remaining.is_empty() {
        // Everything was already done, just clean up
        ctx.remove_propagation_state()?;
        return Ok(PropagationResult::Success {
            rebased_count: offset,
        });
    }

    // Load the stack to determine onto targets for remaining branches
    let stack = ctx.load_stack(&state.stack)?;

    let mut onto_targets = Vec::new();
    for branch_name in &remaining {
        let parent = stack.parent_of(branch_name).context(format!(
            "Could not find parent for branch '{branch_name}' in stack '{}'",
            state.stack
        ))?;
        onto_targets.push(parent);
    }

    match execute_propagation(ctx, &remaining, &onto_targets, &[], offset, total)? {
        PropagationResult::Success { rebased_count } => Ok(PropagationResult::Success {
            rebased_count: rebased_count + offset,
        }),
        conflict => Ok(conflict),
    }
}

/// Abort the current propagation and restore all branches.
pub fn abort(ctx: &Ctx) -> Result<()> {
    let state = ctx
        .propagation_state()?
        .context("No propagation in progress.")?;

    // Abort any in-progress git rebase
    ctx.git.rebase_abort()?;

    // Restore all branches atomically
    if !state.original_refs.is_empty() {
        let updates: Vec<(String, String)> = state
            .original_refs
            .iter()
            .map(|r| (r.branch.clone(), r.commit.clone()))
            .collect();
        ctx.git.update_ref_transaction(&updates)?;
    }

    // Return to original branch
    let _ = ctx.git.checkout(&state.original_branch);

    ctx.remove_propagation_state()?;

    Ok(())
}

/// Internal: execute the remaining propagation steps.
///
/// `progress_offset` is the number of branches already completed before this call
/// (used for accurate `[N/total]` display on continue).
/// `total_branches` is the total number of branches in the full propagation.
fn execute_propagation(
    ctx: &Ctx,
    branches: &[String],
    onto_targets: &[String],
    upstream_overrides: &[Option<String>],
    progress_offset: usize,
    total_branches: usize,
) -> Result<PropagationResult> {
    let mut completed_count = 0;

    for (i, (branch, onto)) in branches.iter().zip(onto_targets.iter()).enumerate() {
        // Update state BEFORE starting the rebase (crash recovery)
        update_current(ctx, branch, &branches[i + 1..])?;

        let pre_sha = ctx.git.rev_parse(&format!("refs/heads/{branch}"))?;

        ctx.git.checkout(branch)?;

        // Use --onto rebase if an upstream override is provided (squash merge case)
        let upstream = upstream_overrides.get(i).and_then(|o| o.as_deref());
        let result = if let Some(upstream) = upstream {
            ctx.git.rebase_onto(onto, upstream)?
        } else {
            ctx.git.rebase(onto)?
        };

        let step = progress_offset + i + 1;
        match result {
            RebaseResult::Success => {
                completed_count += 1;
                let post_sha = ctx.git.rev_parse(&format!("refs/heads/{branch}"))?;
                if pre_sha == post_sha {
                    ui::step_skip(
                        step,
                        total_branches,
                        &format!("'{branch}' already up-to-date"),
                    );
                } else {
                    ui::step_ok(
                        step,
                        total_branches,
                        &format!("Rebased '{branch}' onto '{onto}'"),
                    );
                }
            }
            RebaseResult::Conflict => {
                ui::step_warn(
                    step,
                    total_branches,
                    &format!("Conflict rebasing '{branch}' onto '{onto}'"),
                );
                ui::info("Resolve the conflicts, then run:");
                ui::info("  git add <resolved files>");
                ui::info("  gw rebase --continue");
                ui::info("");
                ui::info("Or abort the entire propagation:");
                ui::info("  gw rebase --abort");
                return Ok(PropagationResult::Conflict {
                    branch: branch.clone(),
                });
            }
        }
    }

    // All done, clean up state
    ctx.remove_propagation_state()?;

    Ok(PropagationResult::Success {
        rebased_count: completed_count,
    })
}

/// Update the propagation state file to reflect current progress.
fn update_current(ctx: &Ctx, current: &str, remaining_after: &[String]) -> Result<()> {
    if let Some(mut state) = ctx.propagation_state()? {
        // Move previous current to completed if it existed
        if let Some(prev) = state.current.take() {
            if !state.completed.contains(&prev) {
                state.completed.push(prev);
            }
        }
        state.current = Some(current.to_string());
        state.remaining = remaining_after.to_vec();
        ctx.save_propagation_state(&state)?;
    }
    Ok(())
}

fn chrono_now() -> String {
    // Simple ISO 8601 timestamp without pulling in chrono crate
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{since_epoch}")
}
