use clap::{Args, Parser, Subcommand};
use clap_complete::{ArgValueCandidates, CompletionCandidate};

fn branch_candidates() -> Vec<CompletionCandidate> {
    let output = std::process::Command::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .output();
    let Ok(output) = output else {
        return vec![];
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(CompletionCandidate::new)
        .collect()
}

fn stack_candidates() -> Vec<CompletionCandidate> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output();
    let Ok(output) = output else {
        return vec![];
    };
    let common_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let entries = std::fs::read_dir(std::path::Path::new(&common_dir).join("gw/stacks"));
    let Ok(entries) = entries else {
        return vec![];
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.path().file_stem()?.to_str().map(str::to_string))
        .map(CompletionCandidate::new)
        .collect()
}

#[derive(Parser)]
#[command(name = "gw", version, about = "Git stacked branch manager")]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Show PR status in stack views
    #[arg(long)]
    pub pr: bool,

    /// Print the fast stack overview as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Manage stacks
    Stack(StackArgs),
    /// Manage branches within a stack
    Branch(BranchArgs),
    /// Adopt existing branches into a stack
    Adopt(AdoptArgs),
    /// Propagate rebases to descendant branches
    Rebase(RebaseArgs),
    /// Sync stacks with the base branch
    Sync(SyncArgs),
    /// Push the current branch
    Push(PushArgs),
    /// Switch to a branch tracked by gw
    Switch(SwitchArgs),
    /// Show status of the current branch in its stack
    Status,
    /// Show diff for the current branch's changes
    Diff(DiffArgs),
    /// Show log of all stacks with branches and commits
    #[command(alias = "tree")]
    Log(TreeArgs),
    /// Show the fast stack overview
    Overview(OverviewArgs),
    /// Split a branch into a stack of focused branches
    Split(SplitArgs),
    /// Configure gw settings
    Config(ConfigArgs),
    /// Check stack metadata and repository health
    Doctor(DoctorArgs),
    /// Generate shell completions
    Completions(CompletionsArgs),
    /// Set up the MCP server for Claude Code
    #[command(name = "mcp-setup")]
    McpSetup,
    /// Run the MCP server (used by Claude Code, not for direct use)
    #[command(name = "mcp-server", hide = true)]
    McpServer,
}

// -- Stack subcommands --

#[derive(Args)]
#[command(flatten_help = true)]
pub struct StackArgs {
    #[command(subcommand)]
    pub command: StackCommands,
}

#[derive(Subcommand)]
pub enum StackCommands {
    /// Create a new stack
    Create {
        /// Name for the stack
        name: String,
        /// Root branch name (prompts interactively if omitted)
        #[arg(long)]
        branch: Option<String>,
        /// Base branch to stack off of (defaults to configured or inferred base)
        #[arg(long, add = ArgValueCandidates::new(branch_candidates))]
        base: Option<String>,
    },
    /// Delete a stack (branches are NOT deleted)
    Delete {
        /// Stack name to delete
        #[arg(add = ArgValueCandidates::new(stack_candidates))]
        name: String,
    },
    /// Rename a stack
    Rename {
        /// Existing stack name
        #[arg(add = ArgValueCandidates::new(stack_candidates))]
        old: String,
        /// New stack name
        new: String,
    },
    /// List all stacks
    List,
}

// -- Branch subcommands --

#[derive(Args)]
#[command(flatten_help = true)]
pub struct BranchArgs {
    #[command(subcommand)]
    pub command: BranchCommands,
}

#[derive(Subcommand)]
pub enum BranchCommands {
    /// Create a new branch in the current stack
    Create {
        /// Branch name
        name: String,
        /// Insert after this tracked branch (defaults to the current branch)
        #[arg(long, add = ArgValueCandidates::new(branch_candidates))]
        after: Option<String>,
    },
    /// Remove a branch from its stack (git branch is NOT deleted)
    Remove {
        /// Branch name to remove
        #[arg(add = ArgValueCandidates::new(branch_candidates))]
        name: String,
    },
    /// Rename a tracked branch and update stack metadata
    Rename {
        /// Existing branch name
        #[arg(add = ArgValueCandidates::new(branch_candidates))]
        old: String,
        /// New branch name
        new: String,
    },
}

// -- Adopt --

#[derive(Args)]
pub struct AdoptArgs {
    /// Branch names in stack order (first = root, last = leaf)
    #[arg(required = true, add = ArgValueCandidates::new(branch_candidates))]
    pub branches: Vec<String>,
    /// Base branch (inferred from first branch's parent if not specified)
    #[arg(long, add = ArgValueCandidates::new(branch_candidates))]
    pub base: Option<String>,
    /// Stack name (defaults to first branch name)
    #[arg(long)]
    pub name: Option<String>,
    /// Skip confirmation prompt
    #[arg(long)]
    pub yes: bool,
}

// -- Rebase --

#[derive(Args)]
pub struct RebaseArgs {
    /// Continue after resolving conflicts
    #[arg(long = "continue", id = "continue", conflicts_with = "abort")]
    pub cont: bool,
    /// Abort and roll back all branches
    #[arg(long, conflicts_with = "continue")]
    pub abort: bool,
}

// -- Sync --

#[derive(Args)]
pub struct SyncArgs {
    /// Only sync a specific stack
    #[arg(long, add = ArgValueCandidates::new(stack_candidates))]
    pub stack: Option<String>,
    /// Manually indicate a branch was merged (when gh is unavailable)
    #[arg(long, add = ArgValueCandidates::new(branch_candidates))]
    pub merged: Option<String>,
    /// Rebase the entire stack onto the latest base branch
    #[arg(long)]
    pub rebase: bool,
    /// Continue sync after resolving conflicts
    #[arg(long = "continue", id = "sync_continue", conflicts_with_all = ["sync_abort", "stack", "merged", "rebase"])]
    pub cont: bool,
    /// Abort sync and restore branches and metadata
    #[arg(long = "abort", id = "sync_abort", conflicts_with_all = ["sync_continue", "stack", "merged", "rebase"])]
    pub abort: bool,
}

// -- Push --

#[derive(Args)]
pub struct PushArgs {
    /// Skip confirmation prompt for force push
    #[arg(long)]
    pub yes: bool,
    /// Push the current branch and every descendant in its stack
    #[arg(long, alias = "descendants")]
    pub stack: bool,
}

// -- Switch --

#[derive(Args)]
pub struct SwitchArgs {
    /// Branch name to switch to (interactive picker if omitted)
    #[arg(add = ArgValueCandidates::new(branch_candidates))]
    pub branch: Option<String>,
}

// -- Tree --

#[derive(Args)]
pub struct TreeArgs {
    /// Show PR status from GitHub (requires gh CLI, adds latency)
    #[arg(long)]
    pub pr: bool,
    /// Disable pager, print directly to stdout
    #[arg(long)]
    pub no_pager: bool,
}

#[derive(Args)]
pub struct OverviewArgs {
    /// Show PR status from GitHub (requires gh CLI, adds latency)
    #[arg(long)]
    pub pr: bool,
    /// Print structured JSON
    #[arg(long)]
    pub json: bool,
}

// -- Diff --

#[derive(Args)]
pub struct DiffArgs {
    /// Show diffstat summary instead of full diff
    #[arg(long)]
    pub stat: bool,
    /// Use regular git diff instead of difftastic
    #[arg(long)]
    pub no_difftastic: bool,
}

// -- Split --

#[derive(Args)]
pub struct SplitArgs {
    /// Plan file mapping commits to branches (non-interactive mode)
    #[arg(long)]
    pub plan: Option<String>,
    /// Base branch to split from (defaults to merge-base detection)
    #[arg(long, add = ArgValueCandidates::new(branch_candidates))]
    pub base: Option<String>,
    /// Stack name for the new stack (defaults to original branch name)
    #[arg(long)]
    pub name: Option<String>,
    /// Skip confirmation prompt
    #[arg(long)]
    pub yes: bool,
    /// Continue after resolving cherry-pick conflicts
    #[arg(
        long = "continue",
        id = "split_continue",
        conflicts_with = "split_abort"
    )]
    pub cont: bool,
    /// Abort the split and clean up created branches
    #[arg(long = "abort", id = "split_abort", conflicts_with = "split_continue")]
    pub abort: bool,
}

// -- Config --

#[derive(Args)]
#[command(flatten_help = true)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommands,
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Set the default base branch (e.g., dev, main, master)
    #[command(name = "set-base")]
    SetBase {
        /// Branch name to use as the default base
        #[arg(add = ArgValueCandidates::new(branch_candidates))]
        branch: String,
    },
    /// Set whether to delete local branches after they're merged
    #[command(name = "set-delete-on-merge")]
    SetDeleteOnMerge {
        /// true or false
        #[arg(value_parser = ["true", "false"])]
        value: String,
    },
    /// Show current configuration
    Show,
}

// -- Completions --

#[derive(Args)]
pub struct CompletionsArgs {
    /// Shell to generate completions for
    #[arg(value_parser = ["zsh", "bash", "fish"])]
    pub shell: String,
}

#[derive(Args)]
pub struct DoctorArgs {
    /// Apply repairs that only remove stale metadata
    #[arg(long)]
    pub fix: bool,
}
