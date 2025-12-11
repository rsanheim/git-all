use anyhow::Result;
use rayon::prelude::*;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;

use crate::repo::repo_name;

const MAX_REPO_NAME_WIDTH: usize = 24;

/// Format repo name with fixed width: truncate long names, pad short ones
fn format_repo_name(name: &str) -> String {
    let display_name = if name.len() > MAX_REPO_NAME_WIDTH {
        format!("{}-...", &name[..MAX_REPO_NAME_WIDTH - 4])
    } else {
        name.to_string()
    };
    format!("[{:<width$}]", display_name, width = MAX_REPO_NAME_WIDTH)
}

/// Execution context holding configuration for running git commands
pub struct ExecutionContext {
    workers: usize,
    dry_run: bool,
}

impl ExecutionContext {
    pub fn new(workers: usize, dry_run: bool) -> Self {
        Self { workers, dry_run }
    }

    pub fn is_dry_run(&self) -> bool {
        self.dry_run
    }
}

/// A git command ready to be executed against a repository
pub struct GitCommand {
    pub repo_path: PathBuf,
    pub args: Vec<String>,
}

impl GitCommand {
    pub fn new(repo_path: PathBuf, args: Vec<String>) -> Self {
        Self { repo_path, args }
    }

    /// Build the command string for display (used in dry-run and errors)
    pub fn command_string(&self) -> String {
        format!(
            "git -C {} {}",
            self.repo_path.display(),
            self.args.join(" ")
        )
    }

    /// Execute the git command, respecting dry-run mode
    pub fn execute(&self, dry_run: bool) -> CommandResult {
        // Single code path: build the command string first
        let cmd_str = self.command_string();

        if dry_run {
            return CommandResult::DryRun(cmd_str);
        }

        // Actually execute
        match Command::new("git")
            .arg("-C")
            .arg(&self.repo_path)
            .args(&self.args)
            .stdin(Stdio::null())
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
        {
            Ok(output) => CommandResult::Executed {
                repo_name: repo_name(&self.repo_path),
                output,
            },
            Err(e) => CommandResult::Error {
                repo_name: repo_name(&self.repo_path),
                message: e.to_string(),
            },
        }
    }
}

/// Result of executing a git command
pub enum CommandResult {
    DryRun(String),
    Executed { repo_name: String, output: Output },
    Error { repo_name: String, message: String },
}

/// Trait for formatting command output into one line
pub trait OutputFormatter: Sync {
    fn format(&self, output: &Output) -> String;
}

/// Run commands in parallel across all repos
pub fn run_parallel<F>(
    ctx: &ExecutionContext,
    repos: &[PathBuf],
    build_command: F,
    formatter: &dyn OutputFormatter,
) -> Result<()>
where
    F: Fn(&PathBuf) -> GitCommand + Sync,
{
    // Build thread pool with specified worker count
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(ctx.workers)
        .build()?;

    // Use a mutex to ensure clean output (no interleaving)
    let stdout = Mutex::new(io::stdout());

    pool.install(|| {
        repos.par_iter().for_each(|repo| {
            let cmd = build_command(repo);
            let result = cmd.execute(ctx.is_dry_run());

            let output_line = match result {
                CommandResult::DryRun(cmd_str) => cmd_str,
                CommandResult::Executed { repo_name, output } => {
                    let formatted = formatter.format(&output);
                    format!("{} {}", format_repo_name(&repo_name), formatted)
                }
                CommandResult::Error { repo_name, message } => {
                    format!("{} ERROR: {}", format_repo_name(&repo_name), message)
                }
            };

            // Lock stdout and print atomically
            let mut handle = stdout.lock().unwrap();
            writeln!(handle, "{}", output_line).ok();
        });
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_repo_name_short() {
        let result = format_repo_name("my-repo");
        assert_eq!(result, "[my-repo                 ]");
        assert_eq!(result.len(), 26); // [ + 24 + ]
    }

    #[test]
    fn test_format_repo_name_exact_length() {
        let result = format_repo_name("exactly-twenty-four-chr");
        assert_eq!(result.len(), 26);
    }

    #[test]
    fn test_format_repo_name_truncated() {
        let result = format_repo_name("this-is-a-very-long-repository-name");
        assert_eq!(result, "[this-is-a-very-long--...]");
        assert_eq!(result.len(), 26);
    }
}
