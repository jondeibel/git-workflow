use anyhow::{Result, bail};
use clap::{CommandFactory, Parser};

use gw::cli::{Cli, Commands, OverviewArgs};
use gw::context::Ctx;
use gw::{commands, state};

fn main() -> Result<()> {
    clap_complete::CompleteEnv::with_factory(Cli::command).complete();
    let cli = Cli::parse();

    // Keep the default view cheap because it is the command used for navigation.
    let command = match cli.command {
        Some(cmd) => {
            if cli.pr || cli.json {
                bail!(
                    "Top-level --pr and --json only apply to the default overview.\n\
                     Use `gw overview --pr`, `gw overview --json`, or command-specific flags."
                );
            }
            cmd
        }
        None => {
            let (ctx, current_branch) = Ctx::discover_with_branch()?;
            let args = OverviewArgs {
                pr: cli.pr,
                json: cli.json,
            };
            return commands::overview::run(args, &ctx, &current_branch);
        }
    };

    // These don't need a Ctx
    if let Commands::Completions(args) = &command {
        return commands::completions::run(&args.shell);
    }
    if let Commands::McpSetup = &command {
        return commands::mcp_setup::run();
    }
    if let Commands::McpServer = &command {
        return commands::mcp_server::run();
    }

    let (ctx, current_branch) = Ctx::discover_with_branch()?;

    // State guard: block most commands if a propagation or split is in progress
    if let Some(ref active) = ctx.active_state()? {
        match active {
            state::ActiveState::Propagation(prop_state) => {
                let allowed = matches!(
                    (&command, &prop_state.operation),
                    (Commands::Rebase(args), state::Operation::Rebase) if args.cont || args.abort
                ) || matches!(
                    (&command, &prop_state.operation),
                    (Commands::Sync(args), state::Operation::Sync) if args.cont || args.abort
                ) || matches!(
                    (&command, &prop_state.operation),
                    (Commands::Split(args), state::Operation::Split) if args.cont || args.abort
                ) || matches!(&command, Commands::Log(_))
                    || matches!(&command, Commands::Overview(_))
                    || matches!(&command, Commands::Switch(_))
                    || matches!(&command, Commands::Status)
                    || matches!(&command, Commands::Diff(_))
                    || matches!(&command, Commands::Doctor(args) if !args.fix);

                if !allowed {
                    let recovery_command = prop_state.operation.recovery_command();
                    bail!(
                        "A {recovery_command} propagation is in progress on stack '{}'.\n\
                         Run `gw {recovery_command} --continue` or `gw {recovery_command} --abort` first.",
                        prop_state.stack,
                    );
                }
            }
            state::ActiveState::Split(split_state) => {
                let allowed = matches!(
                    &command,
                    Commands::Split(args) if args.cont || args.abort
                ) || matches!(&command, Commands::Log(_))
                    || matches!(&command, Commands::Overview(_))
                    || matches!(&command, Commands::Switch(_))
                    || matches!(&command, Commands::Status)
                    || matches!(&command, Commands::Diff(_))
                    || matches!(&command, Commands::Doctor(args) if !args.fix);

                if !allowed {
                    bail!(
                        "A split is in progress on branch '{}'.\n\
                         Run `gw split --continue` or `gw split --abort` first.",
                        split_state.original_branch
                    );
                }
            }
        }
    }

    match command {
        Commands::Status => commands::status::run(&ctx),
        Commands::Diff(args) => commands::diff::run(&ctx, args.stat, args.no_difftastic),
        Commands::Stack(args) => commands::stack::run(args.command, &ctx),
        Commands::Branch(args) => commands::branch::run(args.command, &ctx),
        Commands::Adopt(args) => commands::adopt::run(args, &ctx),
        Commands::Rebase(args) => commands::rebase::run(args, &ctx),
        Commands::Sync(args) => commands::sync::run(args, &ctx),
        Commands::Push(args) => commands::push::run(args, &ctx),
        Commands::Switch(args) => commands::switch::run(args.branch, &ctx),
        Commands::Log(args) => {
            let options = commands::tree::Options::log(args.pr, args.no_pager);
            commands::tree::run(&ctx, &current_branch, options)
        }
        Commands::Overview(args) => commands::overview::run(args, &ctx, &current_branch),
        Commands::Split(args) => commands::split::run(args, &ctx),
        Commands::Config(args) => commands::config::run(args.command, &ctx),
        Commands::Doctor(args) => commands::doctor::run(args, &ctx),
        Commands::Completions(_) | Commands::McpSetup | Commands::McpServer => unreachable!(),
    }
}
