use anyhow::{Result, bail};

use crate::cli::BranchCommands;
use crate::context::Ctx;
use crate::stash;
use crate::state::BranchEntry;
use crate::ui;
use crate::validate;

pub fn run(cmd: BranchCommands, ctx: &Ctx) -> Result<()> {
    match cmd {
        BranchCommands::Create { name, after } => create(ctx, &name, after.as_deref()),
        BranchCommands::Remove { name } => remove(ctx, &name),
        BranchCommands::Rename { old, new } => rename(ctx, &old, &new),
    }
}

fn create(ctx: &Ctx, name: &str, after: Option<&str>) -> Result<()> {
    validate::validate_branch_name(name)?;

    let current = ctx.git.current_branch()?;
    let parent = after.unwrap_or(&current);
    let mut stack = match ctx.find_stack_for_branch(parent)? {
        Some(s) => s,
        None => {
            bail!(
                "Branch '{parent}' is not tracked by any gw stack.\n\
                 Use `gw stack create` to start a new stack or `gw adopt` to track existing branches."
            );
        }
    };

    if ctx.git.branch_exists(name)? {
        bail!("Branch '{name}' already exists.");
    }

    let head = ctx.git.rev_parse(&format!("refs/heads/{parent}"))?;
    ctx.git.create_branch(name, &head)?;

    let parent_index = stack
        .branch_index(parent)
        .expect("tracked parent should have a stack position");
    stack.branches.insert(
        parent_index + 1,
        BranchEntry {
            name: name.to_string(),
        },
    );
    if let Err(error) = ctx.save_stack(&stack) {
        let _ = ctx.git.delete_branch(name);
        return Err(error);
    }

    stash::checkout_with_stash(ctx, name)?;

    ui::success(&format!("Added '{name}' to stack '{}'", stack.name));
    ui::info(&format!("Child of '{parent}'"));

    Ok(())
}

fn rename(ctx: &Ctx, old: &str, new: &str) -> Result<()> {
    validate::validate_branch_name(old)?;
    validate::validate_branch_name(new)?;
    if ctx.find_stack_for_branch(old)?.is_none() {
        bail!("Branch '{old}' is not tracked by any gw stack.");
    }
    if ctx.git.branch_exists(new)? {
        bail!("Branch '{new}' already exists.");
    }

    ctx.git.rename_branch(old, new)?;
    let mut catalog = ctx.stack_catalog()?;
    if let Err(error) = catalog.rename_branch(old, new) {
        let _ = ctx.git.rename_branch(new, old);
        return Err(error);
    }
    ui::success(&format!("Renamed branch '{old}' to '{new}'."));
    Ok(())
}

fn remove(ctx: &Ctx, name: &str) -> Result<()> {
    let mut stack = match ctx.find_stack_for_branch(name)? {
        Some(s) => s,
        None => {
            bail!("Branch '{name}' is not tracked by any gw stack.");
        }
    };

    let idx = stack
        .branch_index(name)
        .expect("branch_index should succeed since find_stack_for_branch found it");

    // If this is the only branch, suggest stack delete instead
    if stack.branches.len() == 1 {
        bail!(
            "'{name}' is the only branch in stack '{}'. Use `gw stack delete {}` instead.",
            stack.name,
            stack.name
        );
    }

    // Determine if we need to re-parent children
    let has_child = idx + 1 < stack.branches.len();

    if has_child {
        // The child needs to be rebased onto the removed branch's parent
        let parent = stack
            .parent_of(name)
            .expect("tracked branch should have a parent");
        let child_name = stack.branches[idx + 1].name.clone();

        // Check working tree is clean before rebasing
        ctx.require_clean_tree()?;

        let original_branch = ctx.git.current_branch()?;

        // Rebase child onto parent
        ui::info(&format!("Rebasing '{child_name}' onto '{parent}'..."));
        ctx.git.checkout(&child_name)?;
        match ctx.git.rebase(&parent)? {
            crate::git::RebaseResult::Success => {}
            crate::git::RebaseResult::Conflict => {
                // Abort the rebase and report the issue
                ctx.git.rebase_abort()?;
                ctx.git.checkout(&original_branch)?;
                bail!(
                    "Rebase of '{child_name}' onto '{parent}' would cause conflicts.\n\
                     Resolve the conflicts manually before removing '{name}' from the stack."
                );
            }
        }

        // Return to original branch (unless it was the one being removed)
        if original_branch != name {
            ctx.git.checkout(&original_branch)?;
        } else {
            // Stay on the child branch
            ctx.git.checkout(&child_name)?;
        }

        ui::info(&format!("Re-parented '{child_name}' onto '{parent}'"));
    }

    // Remove the branch from the stack
    stack.branches.remove(idx);
    ctx.save_stack(&stack)?;

    let role = if idx == 0 { " (was root)" } else { "" };
    ui::success(&format!(
        "Removed '{name}' from stack '{}'{}",
        stack.name, role
    ));
    ui::info(&format!(
        "Git branch '{name}' still exists (not deleted, just untracked)"
    ));

    Ok(())
}
