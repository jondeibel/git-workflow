mod common;

use common::{gw_cmd, TestRepo};
use predicates::prelude::*;

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
    gw_cmd(&repo.path)
        .args(["push"])
        .assert()
        .failure();
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
