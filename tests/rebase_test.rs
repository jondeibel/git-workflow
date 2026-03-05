mod common;

use common::{gw_cmd, TestRepo};
use predicates::prelude::*;

/// Helper: create a stack with 3 branches, each with a unique commit.
fn setup_three_branch_stack(repo: &TestRepo) -> String {
    let main_branch = repo.current_branch();

    gw_cmd(&repo.path)
        .args(["stack", "create", "feature"])
        .assert()
        .success();

    repo.commit_file("a.txt", "a content", "commit a");

    gw_cmd(&repo.path)
        .args(["branch", "create", "feature-tests"])
        .assert()
        .success();

    repo.commit_file("b.txt", "b content", "commit b");

    gw_cmd(&repo.path)
        .args(["branch", "create", "feature-ui"])
        .assert()
        .success();

    repo.commit_file("c.txt", "c content", "commit c");

    main_branch
}

// ============================================================
// gw rebase - successful propagation
// ============================================================

#[test]
fn rebase_propagates_to_descendants() {
    let repo = TestRepo::new();
    let main_branch = setup_three_branch_stack(&repo);

    // Go back to root and add a new commit
    repo.git(&["checkout", "feature"]);
    repo.commit_file("a2.txt", "a2 content", "additional commit on feature");

    // Record pre-rebase SHAs
    let pre_tests_sha = repo.git(&["rev-parse", "feature-tests"]);
    let pre_ui_sha = repo.git(&["rev-parse", "feature-ui"]);

    // Rebase propagation
    gw_cmd(&repo.path)
        .args(["rebase"])
        .assert()
        .success()
        .stdout(predicate::str::contains("2 branches rebased"));

    // SHAs should have changed (branches were rebased)
    let post_tests_sha = repo.git(&["rev-parse", "feature-tests"]);
    let post_ui_sha = repo.git(&["rev-parse", "feature-ui"]);
    assert_ne!(pre_tests_sha, post_tests_sha, "feature-tests should be rebased");
    assert_ne!(pre_ui_sha, post_ui_sha, "feature-ui should be rebased");

    // feature should be ancestor of feature-tests
    let is_ancestor = std::process::Command::new("git")
        .args(["merge-base", "--is-ancestor", "feature", "feature-tests"])
        .current_dir(&repo.path)
        .output()
        .unwrap()
        .status
        .success();
    assert!(is_ancestor);

    // feature-tests should be ancestor of feature-ui
    let is_ancestor = std::process::Command::new("git")
        .args([
            "merge-base",
            "--is-ancestor",
            "feature-tests",
            "feature-ui",
        ])
        .current_dir(&repo.path)
        .output()
        .unwrap()
        .status
        .success();
    assert!(is_ancestor);

    // No state file should remain
    assert!(!repo.state_toml_exists());

    // Should be back on original branch
    assert_eq!(repo.current_branch(), "feature");
}

#[test]
fn rebase_single_descendant() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    repo.commit_file("a.txt", "a", "commit a");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();

    repo.commit_file("b.txt", "b", "commit b");

    // Go back to root, add commit
    repo.git(&["checkout", "auth"]);
    repo.commit_file("a2.txt", "a2", "more auth work");

    gw_cmd(&repo.path)
        .args(["rebase"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1 branch rebased"));
}

#[test]
fn rebase_no_descendants() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    // On the leaf (only branch), no descendants
    gw_cmd(&repo.path)
        .args(["rebase"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No descendant branches"));
}

#[test]
fn rebase_not_on_tracked_branch_fails() {
    let repo = TestRepo::new();

    // Not on any tracked branch
    gw_cmd(&repo.path)
        .args(["rebase"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not tracked"));
}

#[test]
fn rebase_dirty_working_tree_fails() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    repo.commit_file("a.txt", "a", "commit a");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();

    // Go back to root and dirty the tree
    repo.git(&["checkout", "auth"]);
    std::fs::write(repo.path.join("dirty.txt"), "dirty").unwrap();

    gw_cmd(&repo.path)
        .args(["rebase"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("uncommitted changes"));
}

// ============================================================
// gw rebase --abort
// ============================================================

#[test]
fn rebase_abort_restores_branches() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "feature"])
        .assert()
        .success();

    repo.commit_file("shared.txt", "original", "original shared");

    gw_cmd(&repo.path)
        .args(["branch", "create", "feature-b"])
        .assert()
        .success();

    // Modify shared.txt on feature-b
    repo.commit_file("shared.txt", "from feature-b", "modify shared on b");

    let pre_b_sha = repo.git(&["rev-parse", "feature-b"]);

    // Go to root and modify same file (will cause conflict)
    repo.git(&["checkout", "feature"]);
    repo.commit_file("shared.txt", "from feature root", "modify shared on root");

    // Try rebase - should conflict
    let output = gw_cmd(&repo.path)
        .args(["rebase"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("Conflict"), "expected conflict message, got: {combined}");

    // State file should exist
    assert!(repo.state_toml_exists());

    // Abort should restore everything
    gw_cmd(&repo.path)
        .args(["rebase", "--abort"])
        .assert()
        .success()
        .stdout(predicate::str::contains("restored"));

    // State file should be gone
    assert!(!repo.state_toml_exists());

    // Branch should be back to original SHA
    let post_b_sha = repo.git(&["rev-parse", "feature-b"]);
    assert_eq!(pre_b_sha, post_b_sha, "feature-b should be restored to original SHA");
}

#[test]
fn rebase_abort_without_propagation_fails() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["rebase", "--abort"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No propagation"));
}

#[test]
fn rebase_continue_without_propagation_fails() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["rebase", "--continue"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No propagation"));
}

// ============================================================
// gw rebase with conflict -> continue flow
// ============================================================

#[test]
fn rebase_conflict_then_continue() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "feature"])
        .assert()
        .success();

    repo.commit_file("conflict.txt", "original", "original");

    gw_cmd(&repo.path)
        .args(["branch", "create", "feature-b"])
        .assert()
        .success();

    repo.commit_file("conflict.txt", "from-b", "modify on b");

    // Go to root and create conflicting change
    repo.git(&["checkout", "feature"]);
    repo.commit_file("conflict.txt", "from-root", "modify on root");

    // Rebase should hit a conflict
    let output = gw_cmd(&repo.path)
        .args(["rebase"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("Conflict"), "expected conflict, got: {combined}");

    assert!(repo.state_toml_exists());

    // Resolve the conflict manually
    std::fs::write(repo.path.join("conflict.txt"), "resolved").unwrap();
    repo.git(&["add", "conflict.txt"]);

    // Continue
    gw_cmd(&repo.path)
        .args(["rebase", "--continue"])
        .assert()
        .success()
        .stdout(predicate::str::contains("complete"));

    assert!(!repo.state_toml_exists());

    // Verify the conflict was resolved - file should have our resolved content
    let content = std::fs::read_to_string(repo.path.join("conflict.txt")).unwrap();
    assert!(content.contains("resolved"));
}

// ============================================================
// State guard during propagation
// ============================================================

#[test]
fn state_guard_allows_rebase_continue_and_abort() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "feature"])
        .assert()
        .success();

    repo.commit_file("conflict.txt", "original", "original");

    gw_cmd(&repo.path)
        .args(["branch", "create", "feature-b"])
        .assert()
        .success();

    repo.commit_file("conflict.txt", "from-b", "b change");

    repo.git(&["checkout", "feature"]);
    repo.commit_file("conflict.txt", "from-root", "root change");

    // Create conflict
    let output = gw_cmd(&repo.path).args(["rebase"]).output().unwrap();
    assert!(output.status.success());

    // Other commands should be blocked
    gw_cmd(&repo.path)
        .args(["stack", "list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("propagation is in progress"));

    // But abort should work
    gw_cmd(&repo.path)
        .args(["rebase", "--abort"])
        .assert()
        .success();
}

// ============================================================
// Rebase from middle of stack
// ============================================================

#[test]
fn rebase_from_middle_only_affects_descendants() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "a"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "a");

    gw_cmd(&repo.path)
        .args(["branch", "create", "b"])
        .assert()
        .success();
    repo.commit_file("b.txt", "b", "b");

    gw_cmd(&repo.path)
        .args(["branch", "create", "c"])
        .assert()
        .success();
    repo.commit_file("c.txt", "c", "c");

    // Modify b (middle branch) and propagate
    repo.git(&["checkout", "b"]);
    repo.commit_file("b2.txt", "b2", "more b work");

    let pre_a_sha = repo.git(&["rev-parse", "a"]);
    let pre_c_sha = repo.git(&["rev-parse", "c"]);

    gw_cmd(&repo.path)
        .args(["rebase"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1 branch rebased"));

    // a should NOT have changed (it's an ancestor, not a descendant)
    let post_a_sha = repo.git(&["rev-parse", "a"]);
    assert_eq!(pre_a_sha, post_a_sha, "a should not be affected");

    // c SHOULD have changed (it's a descendant)
    let post_c_sha = repo.git(&["rev-parse", "c"]);
    assert_ne!(pre_c_sha, post_c_sha, "c should be rebased");
}
