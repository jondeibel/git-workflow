mod cli;
mod commands;
mod context;
mod git;
mod state;
mod ui;
mod validate;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Commands};
use context::Ctx;

fn main() -> Result<()> {
    let cli = Cli::parse();

    let ctx = Ctx::discover()?;

    // State guard: block most commands if a propagation is in progress
    if let Some(ref prop_state) = ctx.propagation_state()? {
        let allowed = matches!(
            &cli.command,
            Commands::Rebase(args) if args.cont || args.abort
        ) || matches!(&cli.command, Commands::Tree);

        if !allowed {
            ui::error(&format!(
                "A {} propagation is in progress on stack '{}'.",
                match prop_state.operation {
                    state::Operation::Rebase => "rebase",
                    state::Operation::Sync => "sync",
                    state::Operation::Adopt => "adopt",
                    state::Operation::BranchRemove => "branch remove",
                },
                prop_state.stack
            ));
            ui::info("Run `gw rebase --continue` or `gw rebase --abort` first.");
            std::process::exit(1);
        }
    }

    match cli.command {
        Commands::Stack(args) => commands::stack::run(args.command, &ctx),
        Commands::Branch(_args) => {
            ui::info("Branch commands are not yet implemented (Phase 2).");
            Ok(())
        }
        Commands::Adopt(_args) => {
            ui::info("Adopt is not yet implemented (Phase 2).");
            Ok(())
        }
        Commands::Rebase(_args) => {
            ui::info("Rebase is not yet implemented (Phase 3).");
            Ok(())
        }
        Commands::Sync(_args) => {
            ui::info("Sync is not yet implemented (Phase 4).");
            Ok(())
        }
        Commands::Push(_args) => {
            ui::info("Push is not yet implemented (Phase 4).");
            Ok(())
        }
        Commands::Tree => {
            ui::info("Tree is not yet implemented (Phase 5).");
            Ok(())
        }
    }
}
