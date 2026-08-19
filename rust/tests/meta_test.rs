use std::process::Command;

#[test]
fn meta_shows_help_with_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_git-all"))
        .arg("meta")
        .output()
        .expect("failed to execute");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("git-all v"),
        "should contain git-all version"
    );
    assert!(stdout.contains("git"), "should contain git version");
    assert!(stdout.contains("Usage:"), "should contain usage info");
}

#[test]
fn meta_help_shows_same_output() {
    let meta_output = Command::new(env!("CARGO_BIN_EXE_git-all"))
        .arg("meta")
        .output()
        .expect("failed to execute");

    let meta_help_output = Command::new(env!("CARGO_BIN_EXE_git-all"))
        .args(["meta", "help"])
        .output()
        .expect("failed to execute");

    assert!(meta_output.status.success());
    assert!(meta_help_output.status.success());
    assert_eq!(meta_output.stdout, meta_help_output.stdout);
}

#[test]
fn meta_detected_even_with_preceding_global_flag() {
    // Run from inside this crate's own git repo, matching how `git-all meta`
    // is typically invoked in practice (from inside one of many repos).
    let output = Command::new(env!("CARGO_BIN_EXE_git-all"))
        .args(["--dry-run", "meta", "help"])
        .output()
        .expect("failed to execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("git-all v"),
        "expected git-all's own meta help, got stdout={stdout:?} stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn meta_unknown_subcommand_fails() {
    let output = Command::new(env!("CARGO_BIN_EXE_git-all"))
        .args(["meta", "unknown"])
        .output()
        .expect("failed to execute");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unknown meta subcommand"));
}
