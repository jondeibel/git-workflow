use std::collections::{BTreeSet, HashMap};
use std::process::Child;

use anyhow::Result;
use serde::Serialize;

use crate::context::Ctx;
use crate::gh;
use crate::git::Git;
use crate::state::StackConfig;

#[derive(Clone, Copy, Default)]
pub struct SnapshotOptions {
    pub include_commits: bool,
    pub include_prs: bool,
    pub commit_limit: usize,
}

#[derive(Debug, Serialize)]
pub struct RepositorySnapshot {
    pub current_branch: String,
    pub stacks: Vec<StackSnapshot>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct StackSnapshot {
    pub name: String,
    pub base_branch: String,
    pub behind_base: usize,
    pub branches: Vec<BranchSnapshot>,
}

#[derive(Debug, Serialize)]
pub struct BranchSnapshot {
    pub name: String,
    pub parent: String,
    pub is_current: bool,
    pub is_root: bool,
    pub exists: bool,
    pub needs_rebase: bool,
    pub remote: RemoteStatus,
    pub commits: Vec<CommitSnapshot>,
    pub pull_request: Option<PullRequestSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommitSnapshot {
    pub sha: String,
    pub subject: String,
}

#[derive(Debug, Serialize)]
pub struct PullRequestSnapshot {
    pub number: u64,
    pub state: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RemoteStatus {
    UpToDate,
    NeedsPush { ahead: usize },
    Behind { behind: usize },
    Diverged { ahead: usize, behind: usize },
    NoRemote,
}

pub fn load(
    ctx: &Ctx,
    current_branch: &str,
    options: SnapshotOptions,
) -> Result<RepositorySnapshot> {
    let catalog = ctx.stack_catalog()?;
    let warnings = catalog
        .integrity_issues()
        .into_iter()
        .map(|issue| issue.message)
        .collect::<Vec<_>>();
    let stacks = catalog.stacks();
    if stacks.is_empty() {
        return Ok(RepositorySnapshot {
            current_branch: current_branch.to_string(),
            stacks: vec![],
            warnings,
        });
    }

    let ref_info = load_ref_info(ctx, stacks)?;
    let queries = spawn_queries(
        ctx,
        stacks,
        &ref_info,
        options.include_commits,
        options.commit_limit,
    );
    let merge_bases = collect_merge_bases(queries.merge_bases);
    let commits = collect_commits(queries.logs);
    let behind_counts = collect_counts(queries.behind_counts);
    let pull_requests = load_pull_requests(stacks, options.include_prs);
    let mut snapshot = build_snapshot(
        current_branch,
        stacks,
        &ref_info,
        &merge_bases,
        &commits,
        &behind_counts,
        &pull_requests,
    );
    snapshot.warnings = warnings;
    Ok(snapshot)
}

fn load_ref_info(ctx: &Ctx, stacks: &[StackConfig]) -> Result<HashMap<String, RefInfo>> {
    let mut ref_names = BTreeSet::new();
    for stack in stacks {
        ref_names.insert(format!("refs/heads/{}", stack.base_branch));
        for branch in &stack.branches {
            ref_names.insert(format!("refs/heads/{}", branch.name));
        }
    }
    let mut args = vec![
        "for-each-ref",
        "--format=%(refname:short)\t%(objectname)\t%(upstream:short)\t%(upstream:track)",
    ];
    args.extend(ref_names.iter().map(String::as_str));
    let output = ctx.git.run(&args)?;
    Ok(parse_ref_info(&output))
}

struct Queries {
    merge_bases: Vec<((String, String), Child)>,
    logs: Vec<(String, Child)>,
    behind_counts: Vec<(String, Child)>,
}

fn spawn_queries(
    ctx: &Ctx,
    stacks: &[StackConfig],
    refs: &HashMap<String, RefInfo>,
    include_commits: bool,
    commit_limit: usize,
) -> Queries {
    let mut queries = Queries {
        merge_bases: vec![],
        logs: vec![],
        behind_counts: vec![],
    };
    for stack in stacks {
        spawn_stack_queries(
            ctx,
            stack,
            refs,
            include_commits,
            commit_limit,
            &mut queries,
        );
    }
    queries
}

fn spawn_stack_queries(
    ctx: &Ctx,
    stack: &StackConfig,
    refs: &HashMap<String, RefInfo>,
    include_commits: bool,
    commit_limit: usize,
    queries: &mut Queries,
) {
    spawn_behind_query(ctx, stack, refs, &mut queries.behind_counts);
    for (index, branch) in stack.branches.iter().enumerate() {
        if !refs.contains_key(&branch.name) {
            continue;
        }
        let parent = stack.parent_of(&branch.name).unwrap_or_default();
        if index > 0
            && refs.contains_key(&parent)
            && let Ok(child) = ctx.git.spawn(&["merge-base", &branch.name, &parent])
        {
            queries
                .merge_bases
                .push(((branch.name.clone(), parent.clone()), child));
        }
        if include_commits && refs.contains_key(&parent) {
            spawn_log(ctx, &branch.name, &parent, commit_limit, &mut queries.logs);
        }
    }
}

fn spawn_behind_query(
    ctx: &Ctx,
    stack: &StackConfig,
    refs: &HashMap<String, RefInfo>,
    queries: &mut Vec<(String, Child)>,
) {
    let Some(root) = stack.root_branch() else {
        return;
    };
    if !refs.contains_key(&root.name) || !refs.contains_key(&stack.base_branch) {
        return;
    }
    let range = format!("{}..{}", root.name, stack.base_branch);
    if let Ok(child) = ctx.git.spawn(&["rev-list", "--count", &range]) {
        queries.push((stack.name.clone(), child));
    }
}

fn spawn_log(ctx: &Ctx, branch: &str, parent: &str, limit: usize, logs: &mut Vec<(String, Child)>) {
    let range = format!("{parent}..{branch}");
    let max_count = format!("--max-count={}", limit.max(1));
    let child = ctx.git.spawn(&[
        "log",
        "--reverse",
        "--oneline",
        "--format=%h %s",
        &max_count,
        &range,
    ]);
    if let Ok(child) = child {
        logs.push((branch.to_string(), child));
    }
}

fn collect_merge_bases(
    children: Vec<((String, String), Child)>,
) -> HashMap<(String, String), String> {
    let mut result = HashMap::new();
    for (pair, child) in children {
        if let Ok(sha) = Git::collect(child)
            && !sha.is_empty()
        {
            result.insert(pair, sha);
        }
    }
    result
}

fn collect_commits(children: Vec<(String, Child)>) -> HashMap<String, Vec<CommitSnapshot>> {
    let mut result = HashMap::new();
    for (branch, child) in children {
        if let Ok(output) = Git::collect(child) {
            result.insert(branch, parse_commits(&output));
        }
    }
    result
}

fn parse_commits(output: &str) -> Vec<CommitSnapshot> {
    output
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (sha, subject) = line.split_once(' ').unwrap_or((line, ""));
            CommitSnapshot {
                sha: sha.to_string(),
                subject: subject.to_string(),
            }
        })
        .collect()
}

fn collect_counts(children: Vec<(String, Child)>) -> HashMap<String, usize> {
    let mut result = HashMap::new();
    for (stack, child) in children {
        let count = Git::collect(child)
            .ok()
            .and_then(|output| output.parse::<usize>().ok())
            .unwrap_or(0);
        result.insert(stack, count);
    }
    result
}

fn load_pull_requests(stacks: &[StackConfig], include_prs: bool) -> HashMap<String, gh::PrInfo> {
    if !include_prs {
        return HashMap::new();
    }
    let branches = stacks
        .iter()
        .flat_map(|stack| stack.branches.iter().map(|branch| branch.name.as_str()))
        .collect::<Vec<_>>();
    gh::batch_pr_status(&branches)
}

fn build_snapshot(
    current_branch: &str,
    stacks: &[StackConfig],
    refs: &HashMap<String, RefInfo>,
    merge_bases: &HashMap<(String, String), String>,
    commits: &HashMap<String, Vec<CommitSnapshot>>,
    behind_counts: &HashMap<String, usize>,
    pull_requests: &HashMap<String, gh::PrInfo>,
) -> RepositorySnapshot {
    let stacks = stacks
        .iter()
        .map(|stack| {
            build_stack_snapshot(
                current_branch,
                stack,
                refs,
                merge_bases,
                commits,
                behind_counts,
                pull_requests,
            )
        })
        .collect();
    RepositorySnapshot {
        current_branch: current_branch.to_string(),
        stacks,
        warnings: vec![],
    }
}

fn build_stack_snapshot(
    current_branch: &str,
    stack: &StackConfig,
    refs: &HashMap<String, RefInfo>,
    merge_bases: &HashMap<(String, String), String>,
    commits: &HashMap<String, Vec<CommitSnapshot>>,
    behind_counts: &HashMap<String, usize>,
    pull_requests: &HashMap<String, gh::PrInfo>,
) -> StackSnapshot {
    let branches = stack
        .branches
        .iter()
        .enumerate()
        .map(|(index, branch)| {
            let parent = stack.parent_of(&branch.name).unwrap_or_default();
            let needs_rebase = if index == 0 {
                behind_counts.get(&stack.name).copied().unwrap_or(0) > 0
            } else {
                needs_rebase(&branch.name, &parent, refs, merge_bases)
            };
            let pull_request =
                pull_requests
                    .get(&branch.name)
                    .map(|pull_request| PullRequestSnapshot {
                        number: pull_request.number,
                        state: pull_request.state.clone(),
                    });
            BranchSnapshot {
                name: branch.name.clone(),
                parent,
                is_current: branch.name == current_branch,
                is_root: index == 0,
                exists: refs.contains_key(&branch.name),
                needs_rebase,
                remote: refs
                    .get(&branch.name)
                    .map(|info| info.remote.clone())
                    .unwrap_or(RemoteStatus::NoRemote),
                commits: commits.get(&branch.name).cloned().unwrap_or_default(),
                pull_request,
            }
        })
        .collect();
    StackSnapshot {
        name: stack.name.clone(),
        base_branch: stack.base_branch.clone(),
        behind_base: behind_counts.get(&stack.name).copied().unwrap_or(0),
        branches,
    }
}

fn needs_rebase(
    branch: &str,
    parent: &str,
    refs: &HashMap<String, RefInfo>,
    merge_bases: &HashMap<(String, String), String>,
) -> bool {
    let pair = (branch.to_string(), parent.to_string());
    let Some(merge_base) = merge_bases.get(&pair) else {
        return false;
    };
    let Some(parent_ref) = refs.get(parent) else {
        return false;
    };
    merge_base != &parent_ref.sha
}

#[derive(Clone)]
struct RefInfo {
    sha: String,
    remote: RemoteStatus,
}

fn parse_ref_info(output: &str) -> HashMap<String, RefInfo> {
    let mut refs = HashMap::new();
    for line in output.lines() {
        let mut parts = line.splitn(4, '\t');
        let Some(name) = parts.next() else {
            continue;
        };
        let sha = parts.next().unwrap_or_default().to_string();
        let upstream = parts.next().unwrap_or_default();
        let track = parts.next().unwrap_or_default();
        refs.insert(
            name.to_string(),
            RefInfo {
                sha,
                remote: parse_remote_status(upstream, track),
            },
        );
    }
    refs
}

fn parse_remote_status(upstream: &str, track: &str) -> RemoteStatus {
    if upstream.is_empty() || track.contains("gone") {
        return RemoteStatus::NoRemote;
    }
    let ahead = parse_count(track, "ahead ");
    let behind = parse_count(track, "behind ");
    if ahead > 0 && behind > 0 {
        return RemoteStatus::Diverged { ahead, behind };
    }
    if ahead > 0 {
        return RemoteStatus::NeedsPush { ahead };
    }
    if behind > 0 {
        return RemoteStatus::Behind { behind };
    }
    RemoteStatus::UpToDate
}

fn parse_count(value: &str, prefix: &str) -> usize {
    value
        .split(prefix)
        .nth(1)
        .and_then(|rest| {
            rest.split(|character: char| !character.is_ascii_digit())
                .next()
        })
        .and_then(|count| count.parse().ok())
        .unwrap_or(0)
}
