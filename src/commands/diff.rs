use anyhow::{bail, Result};
use std::process::Command;

use crate::context::Ctx;
use crate::ui;

pub fn run(ctx: &Ctx, stat: bool, no_difftastic: bool) -> Result<()> {
    let current = ctx.git.current_branch()?;

    let stack = match ctx.find_stack_for_branch(&current)? {
        Some(s) => s,
        None => bail!(
            "Branch '{current}' is not in any stack.\n\
             Use `gw adopt` to add it, or run `git diff` directly."
        ),
    };

    // parent_of returns base_branch for root branches
    let parent = stack.parent_of(&current).unwrap();

    let merge_base = ctx.git.merge_base(&parent, &current)?;

    let use_difft = !no_difftastic && difft_available();

    if stat {
        // --stat always uses regular git diff (difftastic doesn't support --stat)
        let status = Command::new("git")
            .args(["diff", "--stat", &merge_base, "HEAD"])
            .current_dir(ctx.git.repo_path())
            .status()?;

        if !status.success() {
            bail!("git diff --stat failed");
        }
    } else {
        let mut cmd = Command::new("git");
        cmd.args(["diff", &merge_base, "HEAD"])
            .current_dir(ctx.git.repo_path());

        if use_difft {
            cmd.env("GIT_EXTERNAL_DIFF", "difft");
        }

        let status = cmd.status()?;

        if !status.success() && !use_difft {
            bail!("git diff failed");
        }
    }

    if !stat {
        ui::info(&format!(
            "Showing changes on '{}' since '{}'",
            current, parent
        ));
    }

    Ok(())
}

fn difft_available() -> bool {
    Command::new("difft")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
