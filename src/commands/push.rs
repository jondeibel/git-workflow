use anyhow::{bail, Result};

use crate::cli::PushArgs;
use crate::context::Ctx;
use crate::ui;

pub fn run(args: PushArgs, ctx: &Ctx) -> Result<()> {
    let current = ctx.git.current_branch()?;

    if ctx.find_stack_for_branch(&current)?.is_none() {
        bail!(
            "Current branch '{current}' is not tracked by any gw stack.\n\
             Use regular `git push` for untracked branches."
        );
    }

    let diverged = ctx.git.has_diverged_from_remote(&current)?;

    if diverged {
        // Get the remote SHA for --force-with-lease
        let remote_sha = ctx
            .git
            .rev_parse(&format!("refs/remotes/origin/{current}"))
            .unwrap_or_default();

        if !args.yes {
            if !ui::confirm(
                &format!("Branch '{current}' has diverged from remote. Force push with lease?"),
                false,
            ) {
                ui::info("Push cancelled.");
                return Ok(());
            }
        }

        ui::info(&format!("Force pushing '{current}' (with lease)..."));
        ctx.git.push_force_with_lease(&current, &remote_sha)?;
    } else {
        ui::info(&format!("Pushing '{current}'..."));
        ctx.git.push(&current)?;
    }

    ui::success(&format!("Pushed '{current}'."));
    Ok(())
}
