use anyhow::Result;
use std::path::PathBuf;
use std::process::Output;

use crate::runner::{ExecutionContext, GitCommand, OutputFormatter, run_parallel};

struct StatusFormatter;

impl OutputFormatter for StatusFormatter {
    fn format(&self, output: &Output) -> String {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            return stderr.lines().next().unwrap_or("unknown error").to_string();
        }

        let mut modified = 0;
        let mut added = 0;
        let mut deleted = 0;
        let mut untracked = 0;
        let mut renamed = 0;

        for line in stdout.lines() {
            if line.len() < 2 {
                continue;
            }

            let index_status = line.chars().next().unwrap_or(' ');
            let worktree_status = line.chars().nth(1).unwrap_or(' ');

            if index_status == '?' {
                untracked += 1;
                continue;
            }

            match index_status {
                'M' => modified += 1,
                'A' => added += 1,
                'D' => deleted += 1,
                'R' => renamed += 1,
                'U' => modified += 1,
                _ => {}
            }

            // Check worktree status (unstaged changes) - only if not already counted
            if index_status == ' ' {
                match worktree_status {
                    'M' => modified += 1,
                    'D' => deleted += 1,
                    _ => {}
                }
            }
        }

        if modified == 0 && added == 0 && deleted == 0 && untracked == 0 && renamed == 0 {
            return "clean".to_string();
        }

        let mut parts = Vec::new();

        if modified > 0 {
            parts.push(format!("{} modified", modified));
        }
        if added > 0 {
            parts.push(format!("{} added", added));
        }
        if deleted > 0 {
            parts.push(format!("{} deleted", deleted));
        }
        if renamed > 0 {
            parts.push(format!("{} renamed", renamed));
        }
        if untracked > 0 {
            parts.push(format!("{} untracked", untracked));
        }

        parts.join(", ")
    }
}

pub fn run(ctx: &mut ExecutionContext, repos: &[PathBuf], extra_args: &[String]) -> Result<()> {
    let formatter = StatusFormatter;

    run_parallel(
        ctx,
        repos,
        |repo| {
            // --no-optional-locks: status is read-only; skip the index-refresh lock.
            // --porcelain: machine-readable output.
            let mut args = vec![
                "--no-optional-locks".to_string(),
                "status".to_string(),
                "--porcelain".to_string(),
            ];
            args.extend(extra_args.iter().cloned());
            GitCommand::new(repo.clone(), args)
        },
        &formatter,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    fn make_output(stdout: &str) -> Output {
        Output {
            status: ExitStatus::from_raw(0),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    #[test]
    fn test_unmerged_conflict_is_not_clean() {
        let formatter = StatusFormatter;
        let output = make_output("## main\nUU conflicted.txt\n");
        assert_eq!(formatter.format(&output), "1 modified");
    }

    #[test]
    fn test_clean_with_no_changes() {
        let formatter = StatusFormatter;
        let output = make_output("## main\n");
        assert_eq!(formatter.format(&output), "clean");
    }
}
