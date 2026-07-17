use std::fmt::Write;

use anyhow::Result;
use colored::Colorize;

use crate::context::Ctx;
use crate::snapshot::{
    self, BaseSnapshot, BranchSnapshot, RemoteStatus, RepositorySnapshot, SnapshotOptions,
    StackSnapshot,
};
use crate::ui;

pub enum View {
    Summary,
    Log,
}

pub struct Options {
    view: View,
    show_pr: bool,
    no_pager: bool,
}

impl Options {
    pub fn summary(show_pr: bool) -> Self {
        Self {
            view: View::Summary,
            show_pr,
            no_pager: false,
        }
    }

    pub fn log(show_pr: bool, no_pager: bool) -> Self {
        Self {
            view: View::Log,
            show_pr,
            no_pager,
        }
    }

    fn shows_commits(&self) -> bool {
        matches!(self.view, View::Log)
    }
}

pub fn run(ctx: &Ctx, current_branch: &str, options: Options) -> Result<()> {
    let snapshot = snapshot::load(
        ctx,
        current_branch,
        SnapshotOptions {
            include_commits: options.shows_commits(),
            include_prs: options.show_pr,
            commit_limit: 10,
        },
    )?;
    for warning in &snapshot.warnings {
        ui::warn(warning);
    }
    if snapshot.stacks.is_empty() {
        ui::info("No stacks. Create one with `gw stack create <name>`.");
        return Ok(());
    }
    let rendered = render_tree(&snapshot);
    ui::output_with_pager(&rendered.content, options.no_pager, rendered.focus_line);
    Ok(())
}

struct RenderedTree {
    content: String,
    focus_line: Option<usize>,
}

#[derive(Default)]
struct OutputBuffer {
    content: String,
    line_count: usize,
    focus_line: Option<usize>,
}

impl OutputBuffer {
    fn push(&mut self, line: String, is_current: bool) {
        let _ = writeln!(self.content, "{line}");
        self.line_count += 1;
        if is_current {
            self.focus_line = Some(self.line_count);
        }
    }

    fn finish(self) -> RenderedTree {
        RenderedTree {
            content: self.content,
            focus_line: self.focus_line,
        }
    }
}

fn render_tree(snapshot: &RepositorySnapshot) -> RenderedTree {
    let mut output = OutputBuffer::default();
    for base in &snapshot.bases {
        let stacks = snapshot
            .stacks
            .iter()
            .filter(|stack| stack.base_branch == base.name)
            .collect::<Vec<_>>();
        render_base(base, &stacks, &mut output);
    }
    output.finish()
}

fn render_base(base: &BaseSnapshot, stacks: &[&StackSnapshot], output: &mut OutputBuffer) {
    output.push(format_base_line(base), false);
    for (index, stack) in stacks.iter().enumerate() {
        render_stack(stack, index + 1 == stacks.len(), output);
    }
}

fn format_base_line(base: &BaseSnapshot) -> String {
    let name = base.name.cyan().bold();
    let Some(upstream) = &base.upstream else {
        return format!("{} {name}", "◇".cyan());
    };
    let tag = match &base.remote {
        RemoteStatus::UpToDate => format!("up to date with {upstream}").dimmed().to_string(),
        RemoteStatus::NeedsPush { ahead } => {
            format!("{ahead} ahead of {upstream}").yellow().to_string()
        }
        RemoteStatus::Behind { behind } => {
            format!("{behind} behind {upstream}").yellow().to_string()
        }
        RemoteStatus::Diverged { ahead, behind } => {
            format!("{ahead} ahead, {behind} behind {upstream}")
                .yellow()
                .to_string()
        }
        RemoteStatus::NoRemote => return format!("{} {name}", "◇".cyan()),
    };
    format!("{} {name}  {tag}", "◇".cyan())
}

fn render_stack(stack: &StackSnapshot, is_last: bool, output: &mut OutputBuffer) {
    if stack.branches.is_empty() {
        return;
    }
    let stack_fork = if is_last { "╰─" } else { "├─" };
    let stack_pipe = if is_last { "   " } else { "│  " };
    output.push(format_stack_line(stack, stack_fork), false);
    for (index, branch) in stack.branches.iter().enumerate() {
        let is_last_branch = index + 1 == stack.branches.len();
        let branch_line = format_branch_line(branch, is_last_branch);
        output.push(
            format!("{}{branch_line}", stack_pipe.dimmed()),
            branch.is_current,
        );
        render_commits(stack_pipe, is_last_branch, branch, output);
    }
}

fn format_stack_line(stack: &StackSnapshot, fork: &str) -> String {
    if stack.behind_base == 0 {
        return format!("{} {}", fork.dimmed(), stack.name.magenta().bold());
    }
    let tag = format!("{} behind {}", stack.behind_base, stack.base_branch).yellow();
    format!("{} {}  {tag}", fork.dimmed(), stack.name.magenta().bold())
}

fn render_commits(
    stack_pipe: &str,
    is_last_branch: bool,
    branch: &BranchSnapshot,
    output: &mut OutputBuffer,
) {
    let branch_pipe = if is_last_branch { "   " } else { "│  " };
    for commit in &branch.commits {
        output.push(
            format!(
                "{}{}{} {} {}",
                stack_pipe.dimmed(),
                branch_pipe.dimmed(),
                "│".dimmed(),
                commit.sha.yellow(),
                commit.subject.dimmed()
            ),
            false,
        );
    }
}

fn format_branch_line(branch: &BranchSnapshot, is_last: bool) -> String {
    let marker = if branch.is_current {
        "@".green().bold().to_string()
    } else {
        "◆".blue().to_string()
    };
    let name = format_branch_name(branch);
    let mut tags = vec![];
    if branch.is_root {
        tags.push("root".blue().dimmed().to_string());
    }
    if let Some(pull_request) = &branch.pull_request {
        tags.push(format_pull_request(
            pull_request.number,
            &pull_request.state,
        ));
    }
    add_remote_tag(&mut tags, &branch.remote);
    if branch.needs_rebase {
        tags.push("needs rebase".yellow().to_string());
    }
    if !branch.exists {
        tags.push("missing".red().to_string());
    }
    let fork = if is_last { "╰─" } else { "├─" };
    format!("{} {marker} {name}{}", fork.dimmed(), format_tags(&tags))
}

fn format_branch_name(branch: &BranchSnapshot) -> String {
    if branch.is_current {
        return branch.name.green().bold().to_string();
    }
    if !branch.exists {
        return branch.name.red().strikethrough().to_string();
    }
    branch.name.white().bold().to_string()
}

fn format_pull_request(number: u64, state: &str) -> String {
    let label = match state {
        "OPEN" => format!("PR #{number} open"),
        "CLOSED" => format!("PR #{number} closed"),
        "MERGED" => format!("PR #{number} merged"),
        _ => format!("PR #{number}"),
    };
    label.magenta().to_string()
}

fn add_remote_tag(tags: &mut Vec<String>, status: &RemoteStatus) {
    match status {
        RemoteStatus::Diverged { .. } => tags.push("diverged".yellow().to_string()),
        RemoteStatus::NeedsPush { .. } => tags.push("needs push".yellow().to_string()),
        RemoteStatus::Behind { .. } => tags.push("behind remote".yellow().to_string()),
        RemoteStatus::UpToDate | RemoteStatus::NoRemote => {}
    }
}

fn format_tags(tags: &[String]) -> String {
    if tags.is_empty() {
        return String::new();
    }
    format!("  {}", tags.join("  "))
}
