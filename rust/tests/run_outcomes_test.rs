#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

fn init_repo(root: &Path, name: &str, alias: &str) {
    let repo = root.join(name);
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .arg(&repo)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["config", "alias.check-outcome", alias])
            .status()
            .unwrap()
            .success()
    );
}

fn cli(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_git-all"));
    command
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_ALL_TRACE")
        .env_remove("GIT_ALL_TRACE_FILE");
    command
}

#[test]
fn mixed_outcomes_report_failures_and_preserve_order_with_or_without_tracing() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    init_repo(
        root,
        "a",
        "!sleep 0.2; printf 'failed intentionally\\n' >&2; exit 7",
    );
    init_repo(root, "b", "!printf 'ERROR: this command succeeded\\n'");
    init_repo(root, "c", "!printf 'completed\\n'");

    for traced in [false, true] {
        let mut command = cli(root);
        let trace_file = root.join("trace.log");
        if traced {
            command.env("GIT_ALL_TRACE_FILE", &trace_file);
        }
        let output = command.args(["-n", "3", "check-outcome"]).output().unwrap();
        assert!(
            output.status.success(),
            "repository failures preserve the existing exit policy"
        );
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "git-all: 1 of 3 repositories failed\n"
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        let lines: Vec<_> = stdout.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("[a ") && lines[0].ends_with("failed intentionally"));
        assert!(lines[1].starts_with("[b ") && lines[1].ends_with("ERROR: this command succeeded"));
        assert!(lines[2].starts_with("[c ") && lines[2].ends_with("completed"));

        if traced {
            let trace = fs::read_to_string(&trace_file).unwrap();
            let repos: Vec<_> = trace
                .lines()
                .filter(|line| line.contains("phase=repo"))
                .collect();
            assert_eq!(repos.len(), 3, "one trace per printed row: {trace}");
            for (idx, name, success) in [(0, "a", false), (1, "b", true), (2, "c", true)] {
                assert!(repos[idx].contains(&format!("repo={name:?}")));
                assert!(repos[idx].ends_with(&format!("success={success}")));
            }
            assert_eq!(
                trace
                    .lines()
                    .filter(|line| line.contains("phase=summary"))
                    .count(),
                1
            );
        }
    }
}

#[test]
fn spawn_errors_count_as_failed_repositories() {
    let temp = tempfile::tempdir().unwrap();
    init_repo(temp.path(), "a", "!exit 0");
    init_repo(temp.path(), "b", "!exit 0");
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let git = bin.join("git");
    // An executable whose interpreter is missing makes Command::spawn fail,
    // independently of whether git is installed elsewhere on the machine.
    fs::write(&git, "#!/nonexistent/git-all-test-interpreter\n").unwrap();
    fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();

    let output = cli(temp.path())
        .env("PATH", &bin)
        .arg("status")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "git-all: 2 of 2 repositories failed\n"
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 2);
    assert!(stdout.lines().all(|line| line.contains("ERROR:")));
}

#[test]
fn successful_and_dry_runs_do_not_report_failures() {
    let temp = tempfile::tempdir().unwrap();
    init_repo(temp.path(), "a", "!exit 9");
    for args in [vec!["status"], vec!["--dry-run", "check-outcome"]] {
        let output = cli(temp.path()).args(args).output().unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
    }
}
