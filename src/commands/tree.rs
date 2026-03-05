use anyhow::Result;
use colored::Colorize;

use crate::context::Ctx;
use crate::gh;
use crate::ui;

pub fn run(ctx: &Ctx) -> Result<()> {
    let stacks = ctx.load_all_stacks()?;

    if stacks.is_empty() {
        ui::info("No stacks. Create one with `gw stack create <name>`.");
        return Ok(());
    }

    let current_branch = ctx.git.current_branch().unwrap_or_default();
    let all_branches = ctx.git.all_local_branches().unwrap_or_default();
    let pr_status = gh::batch_pr_status();

    // Group stacks by base branch (preserve order)
    let mut by_base: Vec<(String, Vec<usize>)> = vec![];
    for (i, stack) in stacks.iter().enumerate() {
        if let Some(entry) = by_base.iter_mut().find(|(b, _)| *b == stack.base_branch) {
            entry.1.push(i);
        } else {
            by_base.push((stack.base_branch.clone(), vec![i]));
        }
    }

    let mut first_output = true;
    for (base, stack_indices) in &by_base {
        for &stack_idx in stack_indices {
            let stack = &stacks[stack_idx];
            let branch_count = stack.branches.len();
            if branch_count == 0 {
                continue;
            }

            if !first_output {
                println!();
            }
            first_output = false;

            println!("{}  {}", "◇".cyan(), base.cyan().bold());

            for (idx, branch) in stack.branches.iter().enumerate() {
                let is_last = idx == branch_count - 1;
                let is_current = branch.name == current_branch;
                let is_root = idx == 0;
                let exists = all_branches.contains(&branch.name);

                let marker = if is_current {
                    "@".green().bold().to_string()
                } else {
                    "◆".blue().to_string()
                };

                let name_str = if is_current {
                    branch.name.green().bold().to_string()
                } else if !exists {
                    branch.name.red().strikethrough().to_string()
                } else {
                    branch.name.white().bold().to_string()
                };

                let mut tags: Vec<String> = vec![];
                if is_root {
                    tags.push("root".blue().dimmed().to_string());
                }
                if let Some(info) = pr_status.get(&branch.name) {
                    tags.push(gh::format_pr_status(info).magenta().to_string());
                }
                if exists {
                    match branch_remote_status(ctx, &branch.name) {
                        Ok(RemoteStatus::Diverged) => {
                            tags.push("diverged".yellow().to_string());
                        }
                        Ok(RemoteStatus::NeedsPush) => {
                            tags.push("needs push".yellow().to_string());
                        }
                        _ => {}
                    }
                }
                if !exists {
                    tags.push("missing".red().to_string());
                }
                let tag_str = if tags.is_empty() {
                    String::new()
                } else {
                    format!("  {}", tags.join("  "))
                };

                let fork = if is_last { "╰─" } else { "├─" };
                let pipe = if is_last { " " } else { "│" };

                println!("{}  {marker}  {name_str}{tag_str}", fork.dimmed());

                if exists {
                    let parent_name = stack.parent_of(&branch.name).unwrap_or_default();
                    if let Ok(commits) =
                        ctx.git.log_oneline(&parent_name, &branch.name, 10)
                    {
                        for (sha, subject) in &commits {
                            println!(
                                "{}   {} {} {}",
                                pipe.dimmed(),
                                "│".dimmed(),
                                sha.yellow(),
                                subject.dimmed()
                            );
                        }
                    }
                }

                if !is_last {
                    println!("{}", pipe.dimmed());
                }
            }
        }
    }

    Ok(())
}

enum RemoteStatus {
    UpToDate,
    NeedsPush,
    Diverged,
    NoRemote,
}

/// Check remote status in a single pass (replaces separate has_diverged + check_needs_push).
fn branch_remote_status(ctx: &Ctx, branch: &str) -> Result<RemoteStatus> {
    let remote_ref = format!("refs/remotes/origin/{branch}");
    let remote_exists = ctx.git.run(&["rev-parse", "--verify", &remote_ref]).is_ok();

    if !remote_exists {
        return Ok(RemoteStatus::NoRemote);
    }

    let local_sha = ctx.git.rev_parse(&format!("refs/heads/{branch}"))?;
    let remote_sha = ctx.git.rev_parse(&remote_ref)?;

    if local_sha == remote_sha {
        return Ok(RemoteStatus::UpToDate);
    }

    let local_is_ancestor = ctx.git.is_ancestor(&local_sha, &remote_sha)?;
    let remote_is_ancestor = ctx.git.is_ancestor(&remote_sha, &local_sha)?;

    if remote_is_ancestor {
        Ok(RemoteStatus::NeedsPush)
    } else if local_is_ancestor {
        Ok(RemoteStatus::UpToDate) // behind remote, not diverged from push perspective
    } else {
        Ok(RemoteStatus::Diverged)
    }
}
