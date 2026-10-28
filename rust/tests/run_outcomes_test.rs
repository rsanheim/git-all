#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::process::Command;

fn init_repo(root: &Path, name: &str, alias: &str) {
    let repo = root.join(name);
    fs::create_dir(&repo).unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["config", "alias.check-outcome", alias],
    ] {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
}

fn cli(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_git-all"));
    command.current_dir(root);
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_ALL_TRACE",
        "GIT_ALL_TRACE_FILE",
    ] {
        command.env_remove(key);
    }
    command
}

#[test]
fn failure_summary_and_traces_use_exit_status_not_output() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    init_repo(root, "a", "!exit 7");
    init_repo(root, "b", "!echo 'ERROR: this command succeeded'");
    for traced in [false, true] {
        let mut command = cli(root);
        let trace_file = root.join("trace.log");
        if traced {
            command.env("GIT_ALL_TRACE_FILE", &trace_file);
        }
        let output = command.arg("check-outcome").output().unwrap();
        assert!(output.status.success(), "preserve the existing exit policy");
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "git-all: 1 of 2 repositories failed\n"
        );
        if traced {
            let trace = fs::read_to_string(trace_file).unwrap();
            let repos: Vec<_> = trace
                .lines()
                .filter(|line| line.contains("phase=repo"))
                .collect();
            assert_eq!(repos.len(), 2, "one trace per completed repo: {trace}");
            for (line, name, success) in [(repos[0], "a", false), (repos[1], "b", true)] {
                assert!(line.contains(&format!("repo={name:?}")));
                assert!(line.ends_with(&format!("success={success}")));
            }
        }
    }
    // The same fixture succeeds with status, and dry-run must not execute its failing alias.
    for args in [vec!["status"], vec!["--dry-run", "check-outcome"]] {
        let output = cli(root).args(args).output().unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn spawn_errors_count_as_failed_repositories() {
    let temp = tempfile::tempdir().unwrap();
    init_repo(temp.path(), "a", "!exit 0");
    // No git executable can be found in this nonexistent PATH directory.
    let output = cli(temp.path())
        .env("PATH", temp.path().join("missing-bin"))
        .arg("status")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "git-all: 1 of 1 repositories failed\n"
    );
    assert!(String::from_utf8(output.stdout).unwrap().contains("ERROR:"));
}
