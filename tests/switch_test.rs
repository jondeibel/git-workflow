mod common;

use common::{gw_cmd, TestRepo};
use predicates::prelude::*;

#[test]
fn switch_no_stacks() {
    let repo = TestRepo::new();
    // Piped stdin = non-interactive, but no stacks should bail first
    gw_cmd(&repo.path)
        .args(["switch"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No stacks"));
}

#[test]
fn switch_direct_to_tracked_branch() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "auth work");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();

    // Switch back to auth
    gw_cmd(&repo.path)
        .args(["switch", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Switched to auth"));

    assert_eq!(repo.current_branch(), "auth");

    // Switch to auth-tests
    gw_cmd(&repo.path)
        .args(["switch", "auth-tests"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Switched to auth-tests"));

    assert_eq!(repo.current_branch(), "auth-tests");
}

#[test]
fn switch_to_current_branch_no_op() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    // Already on auth
    gw_cmd(&repo.path)
        .args(["switch", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Already on that branch"));
}

#[test]
fn switch_untracked_branch_fails() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    gw_cmd(&repo.path)
        .args(["switch", "nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not tracked by gw"));
}

#[test]
fn switch_non_interactive_without_arg_fails() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    // When run from assert_cmd, stdin is piped (non-interactive)
    gw_cmd(&repo.path)
        .args(["switch"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Interactive mode requires a terminal"));
}

#[test]
fn switch_works_during_propagation() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "auth work");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();

    repo.write_state_toml(
        r#"
operation = "rebase"
stack = "auth"
started_at = "12345"
original_branch = "auth"
original_refs = []
completed = []
remaining = []
"#,
    );

    // Switch should work even during propagation
    gw_cmd(&repo.path)
        .args(["switch", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Switched to auth"));
}

#[test]
fn switch_across_stacks() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "auth work");

    repo.git(&["checkout", &main_branch]);

    gw_cmd(&repo.path)
        .args(["stack", "create", "billing"])
        .assert()
        .success();
    repo.commit_file("b.txt", "b", "billing work");

    // From billing, switch to auth (different stack)
    gw_cmd(&repo.path)
        .args(["switch", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Switched to auth"));

    assert_eq!(repo.current_branch(), "auth");
}

#[test]
fn switch_to_base_branch() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "auth work");

    // Switch back to base branch (e.g. main)
    gw_cmd(&repo.path)
        .args(["switch", &main_branch])
        .assert()
        .success()
        .stdout(predicate::str::contains(&format!("Switched to {main_branch}")));

    assert_eq!(repo.current_branch(), main_branch);
}
