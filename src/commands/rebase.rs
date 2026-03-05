use anyhow::{bail, Result};

use crate::cli::RebaseArgs;
use crate::context::Ctx;
use crate::propagation::{self, PropagationResult};
use crate::state::Operation;
use crate::ui;

pub fn run(args: RebaseArgs, ctx: &Ctx) -> Result<()> {
    if args.cont {
        return do_continue(ctx);
    }
    if args.abort {
        return do_abort(ctx);
    }
    do_rebase(ctx)
}

fn do_rebase(ctx: &Ctx) -> Result<()> {
    let current = ctx.git.current_branch()?;

    let stack = match ctx.find_stack_for_branch(&current)? {
        Some(s) => s,
        None => {
            bail!(
                "Current branch '{current}' is not tracked by any gw stack.\n\
                 Use `gw stack create` to start a new stack or `gw adopt` to track existing branches."
            );
        }
    };

    ctx.require_clean_tree()?;

    let descendants = stack.descendants_of(&current);
    if descendants.is_empty() {
        ui::info("No descendant branches to rebase.");
        return Ok(());
    }

    let branches: Vec<String> = descendants.iter().map(|b| b.name.clone()).collect();
    let targets: Vec<String> = branches
        .iter()
        .map(|b| stack.parent_of(b).expect("descendant should have a parent"))
        .collect();

    ui::info(&format!(
        "Propagating rebase to {} descendant branch{}...",
        branches.len(),
        if branches.len() == 1 { "" } else { "es" }
    ));

    match propagation::start(ctx, Operation::Rebase, &stack.name, &branches, &targets)? {
        PropagationResult::Success { rebased_count } => {
            // Return to original branch
            ctx.git.checkout(&current)?;
            ui::success(&format!(
                "Rebase propagation complete. {rebased_count} branch{} rebased.",
                if rebased_count == 1 { "" } else { "es" }
            ));
        }
        PropagationResult::Conflict { branch } => {
            ui::warn(&format!(
                "Propagation paused at '{branch}'. Resolve conflicts and continue."
            ));
        }
    }

    Ok(())
}

fn do_continue(ctx: &Ctx) -> Result<()> {
    match propagation::continue_propagation(ctx)? {
        PropagationResult::Success { rebased_count } => {
            // Try to return to the original branch from state
            // (state was already removed by continue_propagation on success)
            ui::success(&format!(
                "Rebase propagation complete. {rebased_count} branch{} rebased.",
                if rebased_count == 1 { "" } else { "es" }
            ));
        }
        PropagationResult::Conflict { branch } => {
            ui::warn(&format!(
                "Propagation paused at '{branch}'. Resolve conflicts and continue."
            ));
        }
    }
    Ok(())
}

fn do_abort(ctx: &Ctx) -> Result<()> {
    propagation::abort(ctx)?;
    ui::success("Rebase propagation aborted. All branches restored to their previous state.");
    Ok(())
}
