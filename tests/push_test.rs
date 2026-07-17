mod common;

use common::{TestRepo, gw_cmd};
use predicates::prelude::*;
use std::process::Command;

#[test]
fn push_not_on_tracked_branch_fails() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["push"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not tracked"));
}

#[test]
fn push_on_tracked_branch_without_remote() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    repo.commit_file("a.txt", "a", "work");

    // Push should fail because there's no remote, but the error should be from git, not gw
    gw_cmd(&repo.path).args(["push"]).assert().failure();
}

#[test]
fn push_yes_flag_exists() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    // Should parse --yes without error (will still fail due to no remote)
    gw_cmd(&repo.path)
        .args(["push", "--yes"])
        .assert()
        .failure(); // fails because no remote, but flag was accepted
}

#[test]
fn push_stack_pushes_current_branch_and_descendants() {
    let repo = TestRepo::new();
    let remote = tempfile::tempdir().unwrap();
    Command::new("git")
        .args(["init", "--bare"])
        .current_dir(remote.path())
        .output()
        .unwrap();
    let remote_path = remote.path().to_string_lossy().to_string();
    repo.git(&["remote", "add", "origin", &remote_path]);

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("auth.txt", "auth", "auth work");
    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();
    repo.commit_file("tests.txt", "tests", "test work");
    repo.git(&["checkout", "auth"]);

    gw_cmd(&repo.path)
        .args(["push", "--stack", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("auth-tests  push"));

    assert!(bare_ref_exists(remote.path(), "auth"));
    assert!(bare_ref_exists(remote.path(), "auth-tests"));
    assert_eq!(repo.git(&["rev-parse", "auth@{upstream}"]), repo.git(&["rev-parse", "auth"]));
    assert_eq!(
        repo.git(&["rev-parse", "auth-tests@{upstream}"]),
        repo.git(&["rev-parse", "auth-tests"])
    );
}

fn bare_ref_exists(remote: &std::path::Path, branch: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--verify", &format!("refs/heads/{branch}")])
        .current_dir(remote)
        .output()
        .unwrap()
        .status
        .success()
}
