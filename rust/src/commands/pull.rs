use anyhow::Result;
use std::path::PathBuf;
use std::process::Output;

use crate::runner::{ExecutionContext, GitCommand, OutputFormatter, run_parallel};

struct PullFormatter;

impl OutputFormatter for PullFormatter {
    fn format(&self, output: &Output) -> String {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            return stderr.lines().next().unwrap_or("unknown error").to_string();
        }

        // Check for "Already up to date"
        if stdout.contains("Already up to date") {
            return "Already up to date".to_string();
        }

        // Try to extract summary from stdout (e.g., "3 files changed, 10 insertions(+), 5 deletions(-)")
        if let Some(summary_line) = stdout
            .lines()
            .find(|l| l.contains("file changed") || l.contains("files changed"))
        {
            return summary_line.trim().to_string();
        }

        // Check for fast-forward or merge info in stdout
        if let Some(line) = stdout
            .lines()
            .find(|l| l.contains("..") || l.contains("Updating"))
        {
            return line.trim().to_string();
        }

        // Fallback: first non-empty line of stdout, or stderr
        stdout
            .lines()
            .chain(stderr.lines())
            .find(|l| !l.trim().is_empty())
            .unwrap_or("completed")
            .trim()
            .to_string()
    }
}

pub fn run(ctx: &mut ExecutionContext, repos: &[PathBuf], extra_args: &[String]) -> Result<()> {
    let formatter = PullFormatter;

    run_parallel(
        ctx,
        repos,
        |repo| {
            let mut args = vec!["pull".to_string()];
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
    fn test_single_file_fast_forward_shows_summary() {
        let stdout = "Updating 02ed788..ccd5545\nFast-forward\n f.txt | 1 +\n 1 file changed, 1 insertion(+)\n";
        assert_eq!(
            PullFormatter.format(&make_output(stdout)),
            "1 file changed, 1 insertion(+)"
        );
    }

    #[test]
    fn test_multiple_files_fast_forward_shows_summary() {
        let stdout = "Updating 02ed788..ccd5545\nFast-forward\n a.txt | 1 +\n b.txt | 1 -\n 2 files changed, 1 insertion(+), 1 deletion(-)\n";
        assert_eq!(
            PullFormatter.format(&make_output(stdout)),
            "2 files changed, 1 insertion(+), 1 deletion(-)"
        );
    }
}
