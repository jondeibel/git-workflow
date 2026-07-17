use anyhow::{Result, bail};

use crate::catalog::CatalogIssue;
use crate::cli::DoctorArgs;
use crate::context::Ctx;
use crate::state::ActiveState;
use crate::ui;

pub fn run(args: DoctorArgs, ctx: &Ctx) -> Result<()> {
    let local_branches = ctx.git.all_local_branches()?;
    let mut catalog = ctx.stack_catalog()?;
    let mut issues = catalog.diagnose(&local_branches);
    issues.extend(active_state_issues(ctx, &local_branches)?);

    if issues.is_empty() {
        ui::success("No gw metadata problems found.");
        return Ok(());
    }
    print_issues(&issues);
    if !args.fix {
        bail!(
            "Found {} metadata problem{}.",
            issues.len(),
            plural(issues.len())
        );
    }

    let repaired = catalog.remove_missing_branches(&local_branches)?;
    if repaired > 0 {
        ui::success(&format!(
            "Removed {repaired} missing branch entr{}.",
            if repaired == 1 { "y" } else { "ies" }
        ));
    }
    let mut remaining = catalog.diagnose(&local_branches);
    remaining.extend(active_state_issues(ctx, &local_branches)?);
    if remaining.is_empty() {
        ui::success("All repairable metadata problems were fixed.");
        return Ok(());
    }
    bail!(
        "{} problem{} require manual repair.",
        remaining.len(),
        plural(remaining.len())
    )
}

fn active_state_issues(
    ctx: &Ctx,
    local_branches: &std::collections::HashSet<String>,
) -> Result<Vec<CatalogIssue>> {
    let Some(state) = ctx.active_state()? else {
        return Ok(vec![]);
    };
    let referenced = match state {
        ActiveState::Propagation(propagation) => propagation
            .original_refs
            .into_iter()
            .map(|original| original.branch)
            .collect::<Vec<_>>(),
        ActiveState::Split(split) => split.created_branches,
    };
    Ok(referenced
        .into_iter()
        .filter(|branch| !local_branches.contains(branch))
        .map(|branch| CatalogIssue {
            message: format!("Active operation references missing branch '{branch}'."),
            repairable: false,
        })
        .collect())
}

fn print_issues(issues: &[CatalogIssue]) {
    for issue in issues {
        let repair = if issue.repairable {
            " (repairable with --fix)"
        } else {
            ""
        };
        ui::warn(&format!("{}{}", issue.message, repair));
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}
