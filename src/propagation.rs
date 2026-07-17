use anyhow::{Context, Result, bail, ensure};

use crate::context::Ctx;
use crate::git::RebaseResult;
use crate::state::{
    Operation, OriginalRef, PropagationActions, PropagationState, PropagationStep, StackConfig,
};
use crate::ui;

pub enum PropagationResult {
    Success { rebased_count: usize },
    Conflict { branch: String },
}

pub struct PropagationPlan {
    operation: Operation,
    stack_name: String,
    steps: Vec<PropagationStep>,
    actions: PropagationActions,
}

impl PropagationPlan {
    pub fn new(
        operation: Operation,
        stack_name: &str,
        branches: &[String],
        onto_targets: &[String],
    ) -> Result<Self> {
        ensure!(
            branches.len() == onto_targets.len(),
            "branches and targets must have same length"
        );
        let steps = branches
            .iter()
            .zip(onto_targets)
            .map(|(branch, onto)| PropagationStep {
                branch: branch.clone(),
                onto: onto.clone(),
                upstream: None,
            })
            .collect();
        Ok(Self {
            operation,
            stack_name: stack_name.to_string(),
            steps,
            actions: PropagationActions::default(),
        })
    }

    pub fn with_upstreams(mut self, upstreams: &[Option<String>]) -> Result<Self> {
        ensure!(
            upstreams.is_empty() || upstreams.len() == self.steps.len(),
            "branches and upstreams must have same length"
        );
        for (step, upstream) in self.steps.iter_mut().zip(upstreams) {
            step.upstream = upstream.clone();
        }
        Ok(self)
    }

    pub fn on_success(mut self, checkout: Option<String>, delete_branches: Vec<String>) -> Self {
        self.actions.success_checkout = checkout;
        self.actions.success_delete_branches = delete_branches;
        self
    }

    pub fn on_abort(
        mut self,
        checkout: Option<String>,
        restore_stack: Option<StackConfig>,
        delete_stack: Option<String>,
        delete_branches: Vec<String>,
    ) -> Self {
        self.actions.abort_checkout = checkout;
        self.actions.abort_restore_stack = restore_stack;
        self.actions.abort_delete_stack = delete_stack;
        self.actions.abort_delete_branches = delete_branches;
        self
    }
}

pub fn start(ctx: &Ctx, plan: PropagationPlan) -> Result<PropagationResult> {
    if plan.steps.is_empty() {
        return Ok(PropagationResult::Success { rebased_count: 0 });
    }

    let original_branch = ctx.git.current_branch()?;
    let original_refs = snapshot_refs(ctx, &plan.steps)?;
    let remaining = plan.steps.iter().map(|step| step.branch.clone()).collect();
    let state = PropagationState {
        operation: plan.operation,
        stack: plan.stack_name,
        started_at: timestamp(),
        original_branch,
        original_refs,
        completed: vec![],
        remaining,
        current: None,
        steps: plan.steps.clone(),
        actions: plan.actions,
    };
    ctx.save_propagation_state(&state)?;
    execute_steps(ctx, &plan.steps, 0, plan.steps.len())
}

pub fn continue_operation(ctx: &Ctx, expected_operation: Operation) -> Result<PropagationResult> {
    let mut state = load_expected_state(ctx, &expected_operation)?;
    ensure_conflicts_resolved(ctx, &state.operation)?;
    show_resume_context(&state);

    if ctx.git.is_rebase_in_progress() {
        let branch = state.current.clone().unwrap_or_default();
        let step = state.completed.len() + 1;
        match ctx.git.rebase_continue()? {
            RebaseResult::Success => ui::step_ok(
                step,
                total_steps(&state),
                &format!("Rebased '{branch}' (conflict resolved)"),
            ),
            RebaseResult::Conflict => return Ok(PropagationResult::Conflict { branch }),
        }
    }

    mark_current_completed(ctx, &mut state)?;
    let steps = remaining_steps(ctx, &state)?;
    if steps.is_empty() {
        finish_success(ctx)?;
        return Ok(PropagationResult::Success {
            rebased_count: state.completed.len(),
        });
    }

    let offset = state.completed.len();
    let total = offset + steps.len();
    match execute_steps(ctx, &steps, offset, total)? {
        PropagationResult::Success { rebased_count } => Ok(PropagationResult::Success {
            rebased_count: rebased_count + offset,
        }),
        conflict => Ok(conflict),
    }
}

pub fn abort(ctx: &Ctx, expected_operation: Operation) -> Result<()> {
    let state = load_expected_state(ctx, &expected_operation)?;
    ctx.git.rebase_abort()?;
    restore_refs(ctx, &state.original_refs)?;
    restore_metadata(ctx, &state.actions)?;

    if let Some(branch) = &state.actions.abort_checkout {
        ctx.git.checkout(branch)?;
    } else {
        ctx.git.checkout(&state.original_branch)?;
    }
    delete_branches(ctx, &state.actions.abort_delete_branches)?;
    ctx.remove_propagation_state()?;
    Ok(())
}

fn snapshot_refs(ctx: &Ctx, steps: &[PropagationStep]) -> Result<Vec<OriginalRef>> {
    steps
        .iter()
        .map(|step| {
            let commit = ctx.git.rev_parse(&format!("refs/heads/{}", step.branch))?;
            Ok(OriginalRef {
                branch: step.branch.clone(),
                commit,
            })
        })
        .collect()
}

fn load_expected_state(ctx: &Ctx, expected: &Operation) -> Result<PropagationState> {
    let state = ctx
        .propagation_state()?
        .context("No propagation in progress.")?;
    if state.operation == *expected {
        return Ok(state);
    }
    bail!(
        "A {} operation is in progress. Use `gw {} --continue` or `gw {} --abort`.",
        state.operation.recovery_command(),
        state.operation.recovery_command(),
        state.operation.recovery_command()
    )
}

fn ensure_conflicts_resolved(ctx: &Ctx, operation: &Operation) -> Result<()> {
    if !ctx.git.has_unresolved_conflicts()? {
        return Ok(());
    }
    let command = operation.recovery_command();
    bail!(
        "There are still unresolved conflicts.\n\
         Resolve them and run `git add`, then `gw {command} --continue`."
    )
}

fn show_resume_context(state: &PropagationState) {
    if state.completed.is_empty() {
        return;
    }
    let completed = state.completed.len();
    let total = total_steps(state);
    let suffix = if total == 1 { "" } else { "es" };
    ui::info(&format!(
        "Resuming: {completed} of {total} branch{suffix} already rebased"
    ));
}

fn total_steps(state: &PropagationState) -> usize {
    let current_count = usize::from(state.current.is_some());
    state.completed.len() + current_count + state.remaining.len()
}

fn mark_current_completed(ctx: &Ctx, state: &mut PropagationState) -> Result<()> {
    let Some(current) = state.current.take() else {
        return Ok(());
    };
    if !state.completed.contains(&current) {
        state.completed.push(current);
    }
    ctx.save_propagation_state(state)
}

fn remaining_steps(ctx: &Ctx, state: &PropagationState) -> Result<Vec<PropagationStep>> {
    if !state.steps.is_empty() {
        return Ok(state.steps.clone());
    }
    let stack = ctx.load_stack(&state.stack)?;
    state
        .remaining
        .iter()
        .map(|branch| {
            let onto = stack
                .parent_of(branch)
                .with_context(|| format!("Could not find parent for branch '{branch}'"))?;
            Ok(PropagationStep {
                branch: branch.clone(),
                onto,
                upstream: None,
            })
        })
        .collect()
}

fn execute_steps(
    ctx: &Ctx,
    steps: &[PropagationStep],
    progress_offset: usize,
    total_steps: usize,
) -> Result<PropagationResult> {
    let mut rebased_count = 0;
    for (index, step) in steps.iter().enumerate() {
        set_current_step(ctx, step, &steps[index + 1..])?;
        let changed = execute_step(ctx, step)?;
        let progress = progress_offset + index + 1;
        if changed {
            ui::step_ok(
                progress,
                total_steps,
                &format!("Rebased '{}' onto '{}'", step.branch, step.onto),
            );
            rebased_count += 1;
            continue;
        }
        if !ctx.git.is_rebase_in_progress() {
            ui::step_skip(
                progress,
                total_steps,
                &format!("'{}' already up-to-date", step.branch),
            );
            rebased_count += 1;
            continue;
        }
        show_conflict_guidance(ctx, progress, total_steps, step)?;
        return Ok(PropagationResult::Conflict {
            branch: step.branch.clone(),
        });
    }
    finish_success(ctx)?;
    Ok(PropagationResult::Success { rebased_count })
}

fn execute_step(ctx: &Ctx, step: &PropagationStep) -> Result<bool> {
    let before = ctx.git.rev_parse(&format!("refs/heads/{}", step.branch))?;
    ctx.git.checkout(&step.branch)?;
    let result = match &step.upstream {
        Some(upstream) => ctx.git.rebase_onto(&step.onto, upstream)?,
        None => ctx.git.rebase(&step.onto)?,
    };
    if matches!(result, RebaseResult::Conflict) {
        return Ok(false);
    }
    let after = ctx.git.rev_parse(&format!("refs/heads/{}", step.branch))?;
    Ok(before != after)
}

fn set_current_step(
    ctx: &Ctx,
    current: &PropagationStep,
    remaining: &[PropagationStep],
) -> Result<()> {
    let Some(mut state) = ctx.propagation_state()? else {
        bail!("Propagation state disappeared while rebasing.");
    };
    if let Some(previous) = state.current.take()
        && !state.completed.contains(&previous)
    {
        state.completed.push(previous);
    }
    state.current = Some(current.branch.clone());
    state.remaining = remaining.iter().map(|step| step.branch.clone()).collect();
    state.steps = remaining.to_vec();
    ctx.save_propagation_state(&state)
}

fn show_conflict_guidance(
    ctx: &Ctx,
    progress: usize,
    total: usize,
    step: &PropagationStep,
) -> Result<()> {
    let state = ctx
        .propagation_state()?
        .context("Propagation state disappeared while reporting a conflict.")?;
    let command = state.operation.recovery_command();
    ui::step_warn(
        progress,
        total,
        &format!("Conflict rebasing '{}' onto '{}'", step.branch, step.onto),
    );
    ui::info("Resolve the conflicts, then run:");
    ui::info("  git add <resolved files>");
    ui::info(&format!("  gw {command} --continue"));
    ui::info("");
    ui::info("Or abort the entire operation:");
    ui::info(&format!("  gw {command} --abort"));
    Ok(())
}

fn finish_success(ctx: &Ctx) -> Result<()> {
    let state = ctx
        .propagation_state()?
        .context("Propagation state disappeared before completion.")?;
    if let Some(branch) = &state.actions.success_checkout {
        ctx.git.checkout(branch)?;
    } else {
        ctx.git.checkout(&state.original_branch)?;
    }
    delete_branches(ctx, &state.actions.success_delete_branches)?;
    ctx.remove_propagation_state()
}

fn restore_refs(ctx: &Ctx, refs: &[OriginalRef]) -> Result<()> {
    if refs.is_empty() {
        return Ok(());
    }
    let updates = refs
        .iter()
        .map(|original| (original.branch.clone(), original.commit.clone()))
        .collect::<Vec<_>>();
    ctx.git.update_ref_transaction(&updates)
}

fn restore_metadata(ctx: &Ctx, actions: &PropagationActions) -> Result<()> {
    if let Some(stack) = &actions.abort_restore_stack {
        ctx.save_stack(stack)?;
    }
    if let Some(stack_name) = &actions.abort_delete_stack {
        ctx.delete_stack(stack_name)?;
    }
    Ok(())
}

fn delete_branches(ctx: &Ctx, branches: &[String]) -> Result<()> {
    for branch in branches {
        if !ctx.git.branch_exists(branch)? {
            continue;
        }
        ctx.git.delete_branch(branch)?;
        ui::info(&format!("Deleted local branch '{branch}'"));
    }
    Ok(())
}

fn timestamp() -> String {
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{since_epoch}")
}
