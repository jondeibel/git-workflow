use std::collections::{HashMap, HashSet};

use anyhow::Result;

use crate::cli::SyncArgs;
use crate::context::Ctx;
use crate::gh;
use crate::propagation::{self, PropagationPlan, PropagationResult};
use crate::state::{Operation, StackConfig};
use crate::ui;

pub fn run(args: SyncArgs, ctx: &Ctx) -> Result<()> {
    if args.cont {
        return continue_sync(ctx);
    }
    if args.abort {
        return abort_sync(ctx);
    }
    sync_stacks(&args, ctx)
}

struct SyncSession {
    original_branch: String,
    original_stack_name: Option<String>,
    original_base: Option<String>,
    synced_bases: HashSet<String>,
    branches_to_delete: Vec<String>,
}

impl SyncSession {
    fn start(ctx: &Ctx) -> Result<Self> {
        let original_branch = ctx.git.current_branch()?;
        let original_stack = ctx.find_stack_for_branch(&original_branch)?;
        Ok(Self {
            original_branch,
            original_stack_name: original_stack.as_ref().map(|stack| stack.name.clone()),
            original_base: original_stack.map(|stack| stack.base_branch),
            synced_bases: HashSet::new(),
            branches_to_delete: vec![],
        })
    }
}

fn sync_stacks(args: &SyncArgs, ctx: &Ctx) -> Result<()> {
    ctx.require_clean_tree()?;
    let stacks = load_requested_stacks(args, ctx)?;
    if stacks.is_empty() {
        ui::info("No stacks to sync.");
        return Ok(());
    }
    if let Err(error) = ctx.git.run(&["fetch", "--prune", "origin"]) {
        ui::warn(&format!("Could not prune remote branches: {error}"));
    }

    let pull_requests = load_pull_requests(&stacks);
    let delete_on_merge = ctx.load_config()?.should_delete_on_merge();
    let mut session = SyncSession::start(ctx)?;
    for stack in stacks {
        let conflict = sync_stack(
            args,
            ctx,
            stack,
            &pull_requests,
            delete_on_merge,
            &mut session,
        )?;
        if conflict {
            return Ok(());
        }
    }
    finish_sync(ctx, &session)
}

fn load_requested_stacks(args: &SyncArgs, ctx: &Ctx) -> Result<Vec<StackConfig>> {
    let Some(name) = &args.stack else {
        return ctx.load_all_stacks();
    };
    Ok(vec![ctx.load_stack(name)?])
}

fn load_pull_requests(stacks: &[StackConfig]) -> HashMap<String, gh::PrInfo> {
    let branches = stacks
        .iter()
        .flat_map(|stack| stack.branches.iter().map(|branch| branch.name.as_str()))
        .collect::<Vec<_>>();
    gh::batch_pr_status(&branches)
}

fn sync_stack(
    args: &SyncArgs,
    ctx: &Ctx,
    mut stack: StackConfig,
    pull_requests: &HashMap<String, gh::PrInfo>,
    delete_on_merge: bool,
    session: &mut SyncSession,
) -> Result<bool> {
    let stack_before = stack.clone();
    update_base(ctx, &stack.base_branch, &mut session.synced_bases);
    if stack.branches.is_empty() {
        return Ok(false);
    }

    // Removing merged roots changes the parent links needed by --onto.
    let previous_parents = snapshot_parent_refs(ctx, &stack);
    let merged_any = remove_merged_roots(
        args,
        ctx,
        &mut stack,
        pull_requests,
        delete_on_merge,
        &mut session.branches_to_delete,
    )?;
    persist_stack_changes(ctx, &stack, merged_any)?;
    // Open stacks stay pinned until a merge or an explicit rebase requires replaying them.
    if (!merged_any && !args.rebase) || stack.branches.is_empty() {
        return Ok(false);
    }
    rebase_stack(ctx, &stack, stack_before, previous_parents, session)
}

fn update_base(ctx: &Ctx, base: &str, synced_bases: &mut HashSet<String>) {
    if !synced_bases.insert(base.to_string()) {
        return;
    }
    ui::info(&format!("Fetching {base}..."));
    if ctx.git.fetch_branch("origin", base).is_err() {
        ui::warn(&format!(
            "Could not fetch origin/{base}. Continuing with local state."
        ));
        return;
    }
    if let Err(error) = ctx.git.update_local_ref(base, &format!("origin/{base}")) {
        ui::warn(&format!("Could not update local ref for {base}: {error}"));
    }
}

fn snapshot_parent_refs(ctx: &Ctx, stack: &StackConfig) -> HashMap<String, String> {
    stack
        .branches
        .iter()
        .filter_map(|branch| {
            let parent = stack.parent_of(&branch.name)?;
            let sha = ctx.git.rev_parse(&parent).ok()?;
            Some((branch.name.clone(), sha))
        })
        .collect()
}

fn remove_merged_roots(
    args: &SyncArgs,
    ctx: &Ctx,
    stack: &mut StackConfig,
    pull_requests: &HashMap<String, gh::PrInfo>,
    delete_on_merge: bool,
    branches_to_delete: &mut Vec<String>,
) -> Result<bool> {
    let mut merged_any = false;
    while let Some(root_entry) = stack.branches.first() {
        let root = root_entry.name.clone();
        let merged = match &args.merged {
            Some(merged_branch) => merged_branch == &root,
            None => detect_merged(ctx, &root, &stack.base_branch, pull_requests)?,
        };
        if !merged {
            break;
        }

        ui::info(&format!(
            "Detected: '{root}' was merged into {}",
            stack.base_branch
        ));
        stack.branches.remove(0);
        merged_any = true;
        if delete_on_merge {
            branches_to_delete.push(root);
        }
        let Some(new_root) = stack.root_branch() else {
            ui::info(&format!(
                "All branches in stack '{}' have been merged! Cleaning up stack.",
                stack.name
            ));
            break;
        };
        ui::info(&format!("New root: '{}'", new_root.name));
        if args.merged.is_some() {
            break;
        }
    }
    Ok(merged_any)
}

fn persist_stack_changes(ctx: &Ctx, stack: &StackConfig, changed: bool) -> Result<()> {
    if !changed {
        return Ok(());
    }
    if stack.branches.is_empty() {
        return ctx.delete_stack(&stack.name);
    }
    ctx.save_stack(stack)
}

fn rebase_stack(
    ctx: &Ctx,
    stack: &StackConfig,
    stack_before: StackConfig,
    previous_parents: HashMap<String, String>,
    session: &mut SyncSession,
) -> Result<bool> {
    let branches = stack
        .branches
        .iter()
        .map(|branch| branch.name.clone())
        .collect::<Vec<_>>();
    let targets = branches
        .iter()
        .map(|branch| {
            stack
                .parent_of(branch)
                .expect("branch should have a parent in its stack")
        })
        .collect::<Vec<_>>();
    // Old parents keep squash-merged commits out of the replay set.
    let upstreams = branches
        .iter()
        .map(|branch| previous_parents.get(branch).cloned())
        .collect::<Vec<_>>();
    show_rebase_start(stack, branches.len());

    let checkout = checkout_target(ctx, session)?;
    let plan = PropagationPlan::new(Operation::Sync, &stack.name, &branches, &targets)?
        .with_upstreams(&upstreams)?
        .on_success(Some(checkout), session.branches_to_delete.clone())
        .on_abort(
            Some(session.original_branch.clone()),
            Some(stack_before),
            None,
            vec![],
        );
    match propagation::start(ctx, plan)? {
        PropagationResult::Success { rebased_count } => {
            session.branches_to_delete.clear();
            show_rebase_success(stack, rebased_count);
            Ok(false)
        }
        PropagationResult::Conflict { branch } => {
            ui::warn(&format!(
                "Conflict while syncing stack '{}' at branch '{branch}'.",
                stack.name
            ));
            ui::info("Resolve conflicts and run `gw sync --continue`.");
            Ok(true)
        }
    }
}

fn show_rebase_start(stack: &StackConfig, branch_count: usize) {
    let suffix = if branch_count == 1 { "" } else { "es" };
    ui::info(&format!(
        "Rebasing {branch_count} branch{suffix} onto {}...",
        stack.base_branch
    ));
}

fn show_rebase_success(stack: &StackConfig, rebased_count: usize) {
    let suffix = if rebased_count == 1 { "" } else { "es" };
    ui::success(&format!(
        "Stack '{}' synced. {rebased_count} branch{suffix} rebased.",
        stack.name
    ));
}

fn finish_sync(ctx: &Ctx, session: &SyncSession) -> Result<()> {
    let target = checkout_target(ctx, session)?;
    if let Err(error) = ctx.git.checkout(&target) {
        ui::warn(&format!("Could not switch to '{target}': {error}"));
    } else if target != session.original_branch {
        ui::info(&format!("Switched to '{target}'"));
    }
    for branch in &session.branches_to_delete {
        if let Err(error) = ctx.git.delete_branch(branch) {
            ui::warn(&format!(
                "Could not delete local branch '{branch}': {error}"
            ));
            continue;
        }
        ui::info(&format!("Deleted local branch '{branch}'"));
    }
    Ok(())
}

fn checkout_target(ctx: &Ctx, session: &SyncSession) -> Result<String> {
    if ctx
        .find_stack_for_branch(&session.original_branch)?
        .is_some()
    {
        return Ok(session.original_branch.clone());
    }
    let original_stack = session
        .original_stack_name
        .as_ref()
        .and_then(|name| ctx.load_stack(name).ok());
    if let Some(stack) = original_stack {
        if let Some(root) = stack.root_branch() {
            return Ok(root.name.clone());
        }
        return Ok(stack.base_branch);
    }
    if let Some(base) = &session.original_base {
        return Ok(base.clone());
    }
    ctx.default_base_branch()
}

fn continue_sync(ctx: &Ctx) -> Result<()> {
    match propagation::continue_operation(ctx, Operation::Sync)? {
        PropagationResult::Success { rebased_count } => {
            show_continue_success(rebased_count);
        }
        PropagationResult::Conflict { branch } => {
            ui::warn(&format!("Sync paused again at '{branch}'."));
        }
    }
    Ok(())
}

fn show_continue_success(rebased_count: usize) {
    let suffix = if rebased_count == 1 { "" } else { "es" };
    ui::success(&format!(
        "Sync complete. {rebased_count} branch{suffix} rebased."
    ));
}

fn abort_sync(ctx: &Ctx) -> Result<()> {
    propagation::abort(ctx, Operation::Sync)?;
    ui::success("Sync aborted. Branches and stack metadata restored.");
    Ok(())
}

fn detect_merged(
    ctx: &Ctx,
    branch: &str,
    base: &str,
    pull_requests: &HashMap<String, gh::PrInfo>,
) -> Result<bool> {
    if gh::is_branch_merged(pull_requests, branch) {
        return Ok(true);
    }
    if let Ok(result) = detect_merged_via_tree(ctx, branch, base) {
        return Ok(result);
    }
    Ok(false)
}

fn detect_merged_via_tree(ctx: &Ctx, branch: &str, base: &str) -> Result<bool> {
    let merge_base = ctx.git.merge_base(branch, base)?;
    let tree = ctx.git.run(&["rev-parse", &format!("{branch}^{{tree}}")])?;
    let synthetic = ctx.git.run(&[
        "commit-tree",
        &tree,
        "-p",
        &merge_base,
        "-m",
        "synthetic squash for merge detection",
    ])?;
    let cherry_output = ctx.git.run(&["cherry", base, &synthetic])?;
    Ok(cherry_output.lines().any(|line| line.starts_with('-')))
}
