mod common;

use std::process::Command;

use common::{TestRepo, gw_cmd};
use predicates::prelude::*;

#[test]
fn linked_worktree_shares_stacks_and_keeps_operation_state_local() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();
    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.git(&["checkout", &main_branch]);

    let worktree_parent = tempfile::tempdir().unwrap();
    let worktree_path = worktree_parent.path().join("linked");
    let worktree = worktree_path.to_string_lossy().to_string();
    repo.git(&["worktree", "add", "-b", "worktree-base", &worktree]);

    gw_cmd(&worktree_path)
        .args(["stack", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("auth"));
    gw_cmd(&worktree_path)
        .args([
            "stack",
            "create",
            "worktree-stack",
            "--branch",
            "worktree-root",
            "--base",
            &main_branch,
        ])
        .assert()
        .success();
    assert!(repo.stack_toml_exists("worktree-stack"));

    let git_dir = git_output(&worktree_path, &["rev-parse", "--absolute-git-dir"]);
    let state_dir = std::path::Path::new(&git_dir).join("gw");
    std::fs::create_dir_all(&state_dir).unwrap();
    std::fs::write(
        state_dir.join("state.toml"),
        "operation = \"rebase\"\nstack = \"worktree-stack\"\nstarted_at = \"1\"\noriginal_branch = \"worktree-root\"\noriginal_refs = []\ncompleted = []\nremaining = []\n",
    )
    .unwrap();

    gw_cmd(&repo.path)
        .args(["stack", "list"])
        .assert()
        .success();
}

fn git_output(path: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}
