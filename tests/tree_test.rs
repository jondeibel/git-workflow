mod common;

use std::process::Command;

use common::{TestRepo, gw_cmd};
use predicates::prelude::*;
use tempfile::TempDir;

#[test]
fn tree_no_stacks() {
    let repo = TestRepo::new();
    gw_cmd(&repo.path)
        .args(["tree"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No stacks"));
}

#[test]
fn default_command_works_on_an_unborn_branch() {
    let repo = TempDir::new().unwrap();
    Command::new("git")
        .arg("init")
        .current_dir(repo.path())
        .output()
        .unwrap();

    gw_cmd(repo.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No stacks"));
}

#[test]
fn tree_single_stack_single_branch() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "auth work");

    let output = gw_cmd(&repo.path).args(["tree"]).output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(stdout.contains(&main_branch), "should show base branch");
    assert!(stdout.contains("auth"), "should show branch name");
    assert!(stdout.contains("root"), "should show root marker");
    assert!(stdout.contains("auth work"), "should show commit message");
}

#[test]
fn tree_shows_commits_as_sub_items() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    repo.commit_file("a1.txt", "a1", "first commit");
    repo.commit_file("a2.txt", "a2", "second commit");

    let output = gw_cmd(&repo.path).args(["tree"]).output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(
        stdout.contains("first commit"),
        "should show first commit: {stdout}"
    );
    assert!(
        stdout.contains("second commit"),
        "should show second commit: {stdout}"
    );
}

#[test]
fn default_command_omits_commits() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "commit only shown in log view");

    let output = gw_cmd(&repo.path).output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(stdout.contains("auth"), "should show branch name: {stdout}");
    assert!(
        !stdout.contains("commit only shown in log view"),
        "should omit commits from the summary view: {stdout}"
    );
}

#[test]
fn overview_json_is_structured_and_omits_commits() {
    let repo = TestRepo::new();
    let base = repo.current_branch();
    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "commit omitted from overview");

    let output = gw_cmd(&repo.path)
        .args(["overview", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["bases"][0]["name"], base);
    assert_eq!(value["stacks"][0]["name"], "auth");
    assert_eq!(
        value["stacks"][0]["branches"][0]["commits"],
        serde_json::json!([])
    );
}

#[test]
fn tree_separates_base_remote_lag_from_stack_base_lag() {
    let repo = TestRepo::new();
    let base = repo.current_branch();
    let remote = TempDir::new().unwrap();
    repo.git(&["init", "--bare", remote.path().to_str().unwrap()]);
    repo.git(&["remote", "add", "origin", remote.path().to_str().unwrap()]);
    repo.git(&["push", "--set-upstream", "origin", &base]);

    gw_cmd(&repo.path)
        .args(["stack", "create", "feature"])
        .assert()
        .success();
    repo.commit_file("feature.txt", "feature", "feature work");
    repo.git(&["checkout", &base]);
    repo.commit_file("base-1.txt", "one", "first base advance");
    repo.commit_file("base-2.txt", "two", "second base advance");
    repo.git(&["push", "origin", &base]);

    repo.git(&["checkout", "-b", "remote-advance"]);
    repo.commit_file("remote.txt", "remote", "remote base advance");
    repo.git(&["push", "origin", &format!("remote-advance:{base}")]);
    repo.git(&["checkout", &base]);
    repo.git(&["branch", "-D", "remote-advance"]);
    repo.git(&["fetch", "origin"]);

    let output = gw_cmd(&repo.path).output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains(&format!("1 behind origin/{base}")),
        "should show local base lag from its remote: {stdout}"
    );
    assert!(
        stdout.contains(&format!("2 behind {base}")),
        "should show stack lag from its local base: {stdout}"
    );
}

#[test]
fn tree_multiple_stacks() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "a");

    repo.git(&["checkout", &main_branch]);

    gw_cmd(&repo.path)
        .args(["stack", "create", "billing"])
        .assert()
        .success();
    repo.commit_file("b.txt", "b", "b");

    repo.git(&["checkout", &main_branch]);

    let output = gw_cmd(&repo.path).args(["tree"]).output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(stdout.contains("auth"));
    assert!(stdout.contains("billing"));
}

#[test]
fn tree_highlights_current_branch() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "a");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();
    repo.commit_file("b.txt", "b", "b");

    let output = gw_cmd(&repo.path).args(["tree"]).output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    // Current branch marker (@ in jj style)
    assert!(
        stdout.contains("@"),
        "should have current branch indicator: {stdout}"
    );
}

#[test]
fn tree_three_branch_stack_with_commits() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    gw_cmd(&repo.path)
        .args(["stack", "create", "feature"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "feature work");

    gw_cmd(&repo.path)
        .args(["branch", "create", "feature-tests"])
        .assert()
        .success();
    repo.commit_file("b.txt", "b", "test work");

    gw_cmd(&repo.path)
        .args(["branch", "create", "feature-ui"])
        .assert()
        .success();
    repo.commit_file("c.txt", "c", "ui work");

    let output = gw_cmd(&repo.path).args(["tree"]).output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(
        stdout.contains("feature"),
        "should show root branch: {stdout}"
    );
    assert!(
        stdout.contains("feature-tests"),
        "should show middle branch: {stdout}"
    );
    assert!(
        stdout.contains("feature-ui"),
        "should show leaf branch: {stdout}"
    );
    assert!(stdout.contains(&main_branch), "should show base: {stdout}");
    assert!(
        stdout.contains("feature work"),
        "should show commit messages: {stdout}"
    );
    assert!(stdout.contains("test work"));
    assert!(stdout.contains("ui work"));
}

#[test]
fn tree_missing_branch_shows_indicator() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "a");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();

    repo.git(&["checkout", "auth"]);
    repo.git(&["branch", "-D", "auth-tests"]);

    let output = gw_cmd(&repo.path).args(["tree"]).output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("missing"),
        "should show missing indicator, got: {stdout}"
    );
}

#[test]
fn tree_works_during_propagation() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
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

    gw_cmd(&repo.path)
        .args(["tree"])
        .assert()
        .success()
        .stdout(predicate::str::contains("auth"));
}
