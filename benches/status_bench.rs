use std::path::{Path, PathBuf};
use std::process::Command;

use criterion::{Criterion, criterion_group, criterion_main};

struct BenchRepo {
    // Hold TempDir so it doesn't get dropped
    _dir: tempfile::TempDir,
    path: PathBuf,
    gw_bin: PathBuf,
}

impl BenchRepo {
    fn new() -> Self {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().to_path_buf();

        git(&path, &["init"]);
        git(&path, &["config", "user.email", "bench@test.com"]);
        git(&path, &["config", "user.name", "Bench"]);

        write_and_commit(&path, "README.md", "# bench repo", "initial commit");

        let gw_bin = assert_cmd::cargo::cargo_bin!("gw").to_path_buf();

        let repo = Self {
            _dir: dir,
            path,
            gw_bin,
        };

        // Create a stack with 3 branches, each with a couple commits
        repo.gw(&["stack", "create", "feat"]);

        write_and_commit(&repo.path, "feat1.txt", "feat 1", "feat: first change");
        write_and_commit(&repo.path, "feat2.txt", "feat 2", "feat: second change");

        repo.gw(&["branch", "create", "feat-tests"]);
        write_and_commit(&repo.path, "test1.txt", "test 1", "test: add tests");
        write_and_commit(&repo.path, "test2.txt", "test 2", "test: more tests");

        repo.gw(&["branch", "create", "feat-docs"]);
        write_and_commit(&repo.path, "docs1.txt", "docs 1", "docs: add docs");

        // Go back to the middle branch for the benchmark
        repo.gw(&["switch", "feat-tests"]);

        repo
    }

    fn gw(&self, args: &[&str]) -> String {
        let output = Command::new(&self.gw_bin)
            .args(args)
            .current_dir(&self.path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "gw {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn run_status(&self) -> std::process::Output {
        Command::new(&self.gw_bin)
            .arg("status")
            .current_dir(&self.path)
            .output()
            .unwrap()
    }
}

fn git(path: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_and_commit(path: &Path, file: &str, content: &str, message: &str) {
    std::fs::write(path.join(file), content).unwrap();
    git(path, &["add", file]);
    git(path, &["commit", "-m", message]);
}

fn bench_status(c: &mut Criterion) {
    let repo = BenchRepo::new();

    // Sanity check: status should succeed
    let output = repo.run_status();
    assert!(output.status.success(), "status failed during setup");

    c.bench_function("gw status", |b| {
        b.iter(|| {
            let output = repo.run_status();
            assert!(output.status.success());
        });
    });
}

criterion_group!(benches, bench_status);
criterion_main!(benches);
