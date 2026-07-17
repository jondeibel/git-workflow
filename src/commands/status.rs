use anyhow::{Context, Result};
use colored::Colorize;

use crate::context::Ctx;
use crate::git::Git;
use crate::snapshot::{self, BranchSnapshot, RemoteStatus, SnapshotOptions, StackSnapshot};

pub fn run(ctx: &Ctx) -> Result<()> {
    let current = ctx.git.current_branch()?;
    let working_tree = ctx.git.spawn(&["status", "--porcelain"])?;
    let snapshot = snapshot::load(
        ctx,
        &current,
        SnapshotOptions {
            include_commits: true,
            include_prs: false,
            commit_limit: 100,
        },
    )?;
    let (stack, branch, index) = find_current(&snapshot.stacks).with_context(|| {
        format!("Branch '{current}' is not in any stack.\nUse `gw adopt` to add it.")
    })?;
    let working_tree = parse_working_tree(&Git::collect(working_tree)?);
    render_header(stack, branch, index);
    render_commits(branch);
    render_flags(branch, &working_tree);
    Ok(())
}

fn find_current(stacks: &[StackSnapshot]) -> Option<(&StackSnapshot, &BranchSnapshot, usize)> {
    for stack in stacks {
        if let Some((index, branch)) = stack
            .branches
            .iter()
            .enumerate()
            .find(|(_, branch)| branch.is_current)
        {
            return Some((stack, branch, index));
        }
    }
    None
}

fn render_header(stack: &StackSnapshot, branch: &BranchSnapshot, index: usize) {
    let total = stack.branches.len();
    let position = position_label(index, total);
    println!(
        "{}  {}  {}",
        "@".green().bold(),
        branch.name.green().bold(),
        position,
    );
    println!("{}  {}", "stack".dimmed(), stack.name.magenta().bold());
    let parent = if branch.is_root {
        format!("{} {}", branch.parent.cyan(), "(base)".dimmed())
    } else {
        branch.parent.white().bold().to_string()
    };
    println!("   {}  {}", "↑".dimmed(), parent);
    if let Some(child) = stack.branches.get(index + 1) {
        println!("   {}  {}", "↓".dimmed(), child.name.white().bold());
    }
    println!();
}

fn position_label(index: usize, total: usize) -> String {
    if total == 1 {
        return "only branch".dimmed().to_string();
    }
    if index == 0 {
        return "root".blue().dimmed().to_string();
    }
    if index + 1 == total {
        return "leaf".blue().dimmed().to_string();
    }
    format!("{}", format!("{} of {total}", index + 1).dimmed())
}

fn render_commits(branch: &BranchSnapshot) {
    let suffix = if branch.commits.len() == 1 { "" } else { "s" };
    println!(
        "{}",
        format!("{} commit{suffix}", branch.commits.len()).dimmed()
    );
    for commit in &branch.commits {
        println!("  {} {}", commit.sha.yellow(), commit.subject);
    }
    if branch.commits.is_empty() {
        println!("  {}", "(no commits yet)".dimmed());
    }
    println!();
}

fn render_flags(branch: &BranchSnapshot, working_tree: &WorkingTree) {
    let mut flags = vec![format_working_tree(working_tree)];
    flags.push(format_remote(&branch.remote));
    if branch.needs_rebase {
        flags.push("behind parent, needs rebase".yellow().to_string());
    }
    for flag in flags {
        println!("  {} {flag}", "•".dimmed());
    }
}

enum WorkingTree {
    Clean,
    Dirty {
        staged: usize,
        unstaged: usize,
        untracked: usize,
    },
}

fn parse_working_tree(output: &str) -> WorkingTree {
    if output.is_empty() {
        return WorkingTree::Clean;
    }
    let mut staged = 0;
    let mut unstaged = 0;
    let mut untracked = 0;
    for line in output.lines().filter(|line| line.len() >= 2) {
        let left = line.as_bytes()[0];
        let right = line.as_bytes()[1];
        if left == b'?' {
            untracked += 1;
            continue;
        }
        if left != b' ' {
            staged += 1;
        }
        if right != b' ' {
            unstaged += 1;
        }
    }
    WorkingTree::Dirty {
        staged,
        unstaged,
        untracked,
    }
}

fn format_working_tree(working_tree: &WorkingTree) -> String {
    let WorkingTree::Dirty {
        staged,
        unstaged,
        untracked,
    } = working_tree
    else {
        return "clean".green().to_string();
    };
    let mut parts = vec![];
    if *staged > 0 {
        parts.push(format!("{staged} staged").green().to_string());
    }
    if *unstaged > 0 {
        parts.push(format!("{unstaged} modified").yellow().to_string());
    }
    if *untracked > 0 {
        parts.push(format!("{untracked} untracked").dimmed().to_string());
    }
    parts.join("  ")
}

fn format_remote(status: &RemoteStatus) -> String {
    match status {
        RemoteStatus::UpToDate => "pushed".green().to_string(),
        RemoteStatus::NeedsPush { ahead } => {
            format!("{ahead} ahead, needs push").yellow().to_string()
        }
        RemoteStatus::Behind { behind } => format!("{behind} behind remote").yellow().to_string(),
        RemoteStatus::Diverged { ahead, behind } => {
            format!("diverged ({ahead} ahead, {behind} behind remote)")
                .yellow()
                .to_string()
        }
        RemoteStatus::NoRemote => "not pushed".dimmed().to_string(),
    }
}
