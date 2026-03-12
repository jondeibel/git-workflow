mod common;

use common::{gw_cmd, TestRepo};
use predicates::prelude::*;

// ============================================================
// gw adopt - basic cases
// ============================================================

#[test]
fn adopt_already_chained_branches() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    // Create a chain of branches manually
    repo.commit_file("base.txt", "base", "base commit");
    repo.git(&["branch", "feature-a"]);
    repo.git(&["checkout", "feature-a"]);
    repo.commit_file("a.txt", "a", "commit a");
    repo.git(&["branch", "feature-b"]);
    repo.git(&["checkout", "feature-b"]);
    repo.commit_file("b.txt", "b", "commit b");

    repo.git(&["checkout", &main_branch]);

    // Adopt them (already chained, no rebase needed)
    gw_cmd(&repo.path)
        .args([
            "adopt",
            "feature-a",
            "feature-b",
            "--base",
            &main_branch,
            "--yes",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created stack"))
        .stdout(predicate::str::contains("feature-a (root) -> feature-b"));

    // Verify TOML
    assert!(repo.stack_toml_exists("feature-a"));
    let toml = repo.read_stack_toml("feature-a");
    assert!(toml.contains(&format!("base_branch = \"{main_branch}\"")));
    assert!(toml.contains("name = \"feature-a\""));
    assert!(toml.contains("name = \"feature-b\""));

    // Verify order in TOML (feature-a should come before feature-b)
    let a_pos = toml.find("name = \"feature-a\"").unwrap();
    let b_pos = toml.find("name = \"feature-b\"").unwrap();
    assert!(a_pos < b_pos);
}

#[test]
fn adopt_with_custom_stack_name() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    repo.git(&["branch", "branch-a"]);
    repo.git(&["checkout", "branch-a"]);
    repo.commit_file("a.txt", "a", "a");

    repo.git(&["checkout", &main_branch]);

    gw_cmd(&repo.path)
        .args([
            "adopt",
            "branch-a",
            "--base",
            &main_branch,
            "--name",
            "my-stack",
            "--yes",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created stack 'my-stack'"));

    assert!(repo.stack_toml_exists("my-stack"));
}

#[test]
fn adopt_unchained_branches_rebases_with_yes() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    // Create two independent branches off main (not chained)
    repo.git(&["branch", "branch-a"]);
    repo.git(&["checkout", "branch-a"]);
    repo.commit_file("a.txt", "a", "commit a");

    repo.git(&["checkout", &main_branch]);
    repo.git(&["branch", "branch-b"]);
    repo.git(&["checkout", "branch-b"]);
    repo.commit_file("b.txt", "b", "commit b");

    repo.git(&["checkout", &main_branch]);

    // Adopt with --yes to skip prompt
    gw_cmd(&repo.path)
        .args([
            "adopt",
            "branch-a",
            "branch-b",
            "--base",
            &main_branch,
            "--yes",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Rebasing 2 branches into a chain"))
        .stdout(predicate::str::contains("Created stack"));

    // Verify branch-b is now a descendant of branch-a
    let a_sha = repo.git(&["rev-parse", "branch-a"]);
    let is_ancestor = std::process::Command::new("git")
        .args(["merge-base", "--is-ancestor", &a_sha, "branch-b"])
        .current_dir(&repo.path)
        .output()
        .unwrap()
        .status
        .success();
    assert!(is_ancestor, "branch-a should be ancestor of branch-b after adopt");
}

#[test]
fn adopt_nonexistent_branch_fails() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["adopt", "no-such-branch", "--base", "main", "--yes"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not exist"));
}

#[test]
fn adopt_already_tracked_branch_fails() {
    let repo = TestRepo::new();

    // Create a stack with a branch
    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    // Try to adopt the already-tracked branch
    gw_cmd(&repo.path)
        .args(["adopt", "auth", "--base", "main", "--yes"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already tracked"));
}

#[test]
fn adopt_duplicate_stack_name_fails() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    repo.git(&["checkout", &main_branch]);
    repo.git(&["branch", "other-branch"]);
    repo.git(&["checkout", "other-branch"]);
    repo.commit_file("x.txt", "x", "x");
    repo.git(&["checkout", &main_branch]);

    // Try to adopt with the same stack name
    gw_cmd(&repo.path)
        .args([
            "adopt",
            "other-branch",
            "--base",
            &main_branch,
            "--name",
            "auth",
            "--yes",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn adopt_invalid_branch_name_rejected() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["adopt", "--force", "--base", "main", "--yes"])
        .assert()
        .failure();
}

#[test]
fn adopt_infers_base_from_main() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    // Create a branch off main
    repo.git(&["branch", "feature-x"]);
    repo.git(&["checkout", "feature-x"]);
    repo.commit_file("x.txt", "x", "commit x");
    repo.git(&["checkout", &main_branch]);

    // Adopt without --base (should infer main/master)
    gw_cmd(&repo.path)
        .args(["adopt", "feature-x", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created stack"));

    let toml = repo.read_stack_toml("feature-x");
    assert!(toml.contains(&format!("base_branch = \"{main_branch}\"")));
}

#[test]
fn adopt_nonexistent_base_fails() {
    let repo = TestRepo::new();

    repo.git(&["branch", "feature-a"]);

    gw_cmd(&repo.path)
        .args(["adopt", "feature-a", "--base", "no-such-base", "--yes"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not exist"));
}

// ============================================================
// Adopt with three branches
// ============================================================

#[test]
fn adopt_three_branches_already_chained() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    // Build a manual chain: main -> a -> b -> c
    repo.git(&["branch", "a"]);
    repo.git(&["checkout", "a"]);
    repo.commit_file("a.txt", "a", "a");

    repo.git(&["branch", "b"]);
    repo.git(&["checkout", "b"]);
    repo.commit_file("b.txt", "b", "b");

    repo.git(&["branch", "c"]);
    repo.git(&["checkout", "c"]);
    repo.commit_file("c.txt", "c", "c");

    repo.git(&["checkout", &main_branch]);

    gw_cmd(&repo.path)
        .args(["adopt", "a", "b", "c", "--base", &main_branch, "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("a (root) -> b -> c"));

    let toml = repo.read_stack_toml("a");
    let a_pos = toml.find("name = \"a\"").unwrap();
    let b_pos = toml.find("name = \"b\"").unwrap();
    let c_pos = toml.find("name = \"c\"").unwrap();
    assert!(a_pos < b_pos && b_pos < c_pos);
}
