use std::io;
use std::path::PathBuf;
use std::process::Output;

use crate::runner::{ExecutionContext, GitCommand, OutputFormatter, RunSummary, run_parallel};

struct PassthroughFormatter;

impl OutputFormatter for PassthroughFormatter {
    fn format(&self, output: &Output) -> String {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            let error_line = stderr
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("unknown error");
            return format!("ERROR: {}", error_line);
        }

        stdout
            .lines()
            .chain(stderr.lines())
            .find(|l| !l.trim().is_empty())
            .unwrap_or("ok")
            .trim()
            .to_string()
    }
}

pub fn run(
    ctx: &mut ExecutionContext,
    repos: &[PathBuf],
    args: &[String],
) -> io::Result<RunSummary> {
    if args.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "No git command specified",
        ));
    }

    let formatter = PassthroughFormatter;

    run_parallel(
        ctx,
        repos,
        |repo| GitCommand::new(repo.clone(), args.to_vec()),
        &formatter,
    )
}
