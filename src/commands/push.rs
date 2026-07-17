use anyhow::{Result, bail};

use crate::cli::PushArgs;
use crate::context::Ctx;
use crate::ui;

pub fn run(args: PushArgs, ctx: &Ctx) -> Result<()> {
    let current = ctx.git.current_branch()?;
    let stack = ctx.find_stack_for_branch(&current)?.ok_or_else(|| {
        anyhow::anyhow!(
            "Current branch '{current}' is not tracked by any gw stack.\n\
             Use regular `git push` for untracked branches."
        )
    })?;
    let branches = branches_to_push(&stack, &current, args.stack);
    let actions = plan_pushes(ctx, &branches)?;
    show_plan(&actions);

    if actions
        .iter()
        .any(|action| matches!(action.kind, PushKind::Behind))
    {
        bail!("At least one branch is behind its remote. Sync or reconcile it before pushing.");
    }
    let force_count = actions
        .iter()
        .filter(|action| matches!(action.kind, PushKind::Force { .. }))
        .count();
    if force_count > 0 && !args.yes {
        let prompt = format!(
            "Force push {force_count} branch{} with lease?",
            plural(force_count)
        );
        if !ui::confirm(&prompt, false) {
            ui::info("Push cancelled.");
            return Ok(());
        }
    }

    let pushed = execute_pushes(ctx, &actions)?;
    ui::success(&format!("Pushed {pushed} branch{}.", plural(pushed)));
    Ok(())
}

fn branches_to_push(
    stack: &crate::state::StackConfig,
    current: &str,
    include_descendants: bool,
) -> Vec<String> {
    if !include_descendants {
        return vec![current.to_string()];
    }
    let index = stack
        .branch_index(current)
        .expect("tracked branch should have a stack position");
    stack.branches[index..]
        .iter()
        .map(|branch| branch.name.clone())
        .collect()
}

struct PushAction {
    branch: String,
    kind: PushKind,
}

enum PushKind {
    Normal,
    Force { expected_sha: String },
    Behind,
    UpToDate,
}

fn plan_pushes(ctx: &Ctx, branches: &[String]) -> Result<Vec<PushAction>> {
    branches
        .iter()
        .map(|branch| {
            let local_sha = ctx.git.rev_parse(&format!("refs/heads/{branch}"))?;
            let remote_sha = ctx.git.remote_branch_sha("origin", branch)?;
            let kind = classify_push(ctx, &local_sha, remote_sha.as_deref())?;
            let kind = match (kind, remote_sha) {
                (PushKind::Force { .. }, Some(expected_sha)) => PushKind::Force { expected_sha },
                (other, _) => other,
            };
            Ok(PushAction {
                branch: branch.clone(),
                kind,
            })
        })
        .collect()
}

fn classify_push(ctx: &Ctx, local_sha: &str, remote_sha: Option<&str>) -> Result<PushKind> {
    let Some(remote_sha) = remote_sha else {
        return Ok(PushKind::Normal);
    };
    if local_sha == remote_sha {
        return Ok(PushKind::UpToDate);
    }
    if ctx.git.is_ancestor(remote_sha, local_sha)? {
        return Ok(PushKind::Normal);
    }
    if ctx.git.is_ancestor(local_sha, remote_sha)? {
        return Ok(PushKind::Behind);
    }
    Ok(PushKind::Force {
        expected_sha: remote_sha.to_string(),
    })
}

fn show_plan(actions: &[PushAction]) {
    ui::info("Push plan:");
    for action in actions {
        let label = match action.kind {
            PushKind::Normal => "push",
            PushKind::Force { .. } => "force-with-lease",
            PushKind::Behind => "blocked: behind remote",
            PushKind::UpToDate => "already up to date",
        };
        ui::info(&format!("  {}  {label}", action.branch));
    }
}

fn execute_pushes(ctx: &Ctx, actions: &[PushAction]) -> Result<usize> {
    let mut pushed = 0;
    for action in actions {
        match &action.kind {
            PushKind::Normal => ctx.git.push(&action.branch)?,
            PushKind::Force { expected_sha } => {
                ctx.git
                    .push_force_with_lease(&action.branch, expected_sha)?;
            }
            PushKind::Behind | PushKind::UpToDate => continue,
        }
        pushed += 1;
    }
    Ok(pushed)
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "es" }
}
