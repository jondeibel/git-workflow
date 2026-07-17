mod common;

use common::{TestRepo, gw_cmd};
use predicates::prelude::*;

#[test]
fn doctor_reports_and_repairs_missing_tracked_branches() {
    let repo = TestRepo::new();
    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();
    repo.git(&["checkout", "auth"]);
    repo.git(&["branch", "-D", "auth-tests"]);

    gw_cmd(&repo.path)
        .args(["doctor"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "tracks missing branch 'auth-tests'",
        ));

    gw_cmd(&repo.path)
        .args(["doctor", "--fix"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed 1 missing branch entry"));
    assert!(!repo.read_stack_toml("auth").contains("auth-tests"));
}
