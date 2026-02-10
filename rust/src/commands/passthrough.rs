use anyhow::Result;
use std::path::PathBuf;
use std::process::Output;

use crate::runner::{run_parallel, ExecutionContext, FormattedResult, GitCommand, OutputFormatter};

struct PassthroughFormatter;

impl OutputFormatter for PassthroughFormatter {
    fn format(&self, output: &Output) -> FormattedResult {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            let error_line = stderr
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("unknown error");
            return FormattedResult {
                branch: String::new(),
                message: format!("ERROR: {}", error_line),
            };
        }

        let message = stdout
            .lines()
            .chain(stderr.lines())
            .find(|l| !l.trim().is_empty())
            .unwrap_or("ok")
            .trim()
            .to_string();

        FormattedResult {
            branch: String::new(),
            message,
        }
    }
}

pub fn run(ctx: &ExecutionContext, repos: &[PathBuf], args: &[String]) -> Result<()> {
    if args.is_empty() {
        anyhow::bail!("No git command specified");
    }

    let formatter = PassthroughFormatter;

    run_parallel(
        ctx,
        repos,
        |repo| GitCommand::new(repo.clone(), args.to_vec()),
        &formatter,
    )
}
