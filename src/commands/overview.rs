use anyhow::Result;

use crate::cli::OverviewArgs;
use crate::commands::tree;
use crate::context::Ctx;
use crate::snapshot::{self, SnapshotOptions};

pub fn run(args: OverviewArgs, ctx: &Ctx, current_branch: &str) -> Result<()> {
    if !args.json {
        return tree::run(ctx, current_branch, tree::Options::summary(args.pr));
    }
    let snapshot = snapshot::load(
        ctx,
        current_branch,
        SnapshotOptions {
            include_commits: false,
            include_prs: args.pr,
            commit_limit: 10,
        },
    )?;
    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    Ok(())
}
