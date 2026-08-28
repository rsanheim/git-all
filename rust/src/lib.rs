use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use std::process::Command;
use std::time::Instant;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

mod commands;
mod meta;
mod printer;
mod repo;
mod runner;
mod trace;

use commands::{fetch, passthrough, pull, status};
use repo::{ScanDepth, find_git_repos_in, is_inside_git_repo, parse_scan_depth};
use runner::{ExecutionContext, UrlScheme};
use trace::TraceSink;

#[derive(Parser)]
#[command(
    name = "git-all",
    version,
    about = "parallel git across many repositories"
)]
struct Cli {
    /// Print exact commands without executing
    #[arg(long)]
    dry_run: bool,

    /// Force SSH URLs (git@github.com:) for all remotes
    #[arg(long, conflicts_with = "https")]
    ssh: bool,

    /// Force HTTPS URLs (https://github.com/) for all remotes
    #[arg(long, conflicts_with = "ssh")]
    https: bool,

    /// Enable SSH ControlMaster connection multiplexing (off by default)
    #[arg(long, overrides_with = "_no_ssh_multiplexing")]
    ssh_multiplexing: bool,

    #[arg(
        long = "no-ssh-multiplexing",
        overrides_with = "ssh_multiplexing",
        hide = true
    )]
    _no_ssh_multiplexing: bool,

    /// Number of parallel workers (default: command-specific; status=8, fetch/pull=16; 0 = unlimited)
    #[arg(short = 'n', long)]
    workers: Option<usize>,

    /// How deep to scan for repositories (positive integer or "all")
    #[arg(long, default_value = "1", value_parser = parse_scan_depth, value_name = "DEPTH|all")]
    scan_depth: ScanDepth,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Pull all repositories
    Pull {
        /// Additional arguments to pass to git pull
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Fetch all repositories
    Fetch {
        /// Additional arguments to pass to git fetch
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Status of all repositories
    Status {
        /// Additional arguments to pass to git status
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// git-all internal commands (help, version info)
    Meta {
        /// Subcommand (help is the only option)
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Pass through to git (any other command)
    #[command(external_subcommand)]
    External(Vec<String>),
}

/// Build the clap command (the single source of truth for the CLI).
///
/// Used by `cargo xtask mangen` to render the man page so it never drifts from
/// the actual interface. clap_mangen lives only in the xtask crate, so this is
/// the binary's only contribution to man-page generation.
pub fn cli_command() -> clap::Command {
    Cli::command()
}

fn command_label(command: &Option<Commands>) -> &str {
    match command {
        Some(Commands::Pull { .. }) => "pull",
        Some(Commands::Fetch { .. }) => "fetch",
        Some(Commands::Status { .. }) => "status",
        Some(Commands::Meta { .. }) => "meta",
        Some(Commands::External(args)) => args.first().map(String::as_str).unwrap_or("external"),
        None => "none",
    }
}

/// Default worker count when -n is not specified.
/// Network-bound commands benefit from higher concurrency; local-only commands
/// see I/O contention with too many concurrent git processes.
fn default_workers(command: &Option<Commands>) -> usize {
    match command {
        Some(Commands::Fetch { .. }) | Some(Commands::Pull { .. }) => 16,
        _ => 8,
    }
}

/// Exec git with all original args, replacing the git-all process.
/// This is used when git-all is invoked from inside a git repository.
#[cfg(unix)]
fn passthrough_to_git() -> ! {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let err = Command::new("git").args(&args).exec();
    // exec() only returns on error
    eprintln!("git-all: failed to exec git: {}", err);
    std::process::exit(1);
}

#[cfg(not(unix))]
fn passthrough_to_git() -> ! {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let status = Command::new("git")
        .args(&args)
        .status()
        .expect("failed to execute git");
    std::process::exit(status.code().unwrap_or(1));
}

pub fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Detect meta mode using the real clap grammar (the single source of
    // truth for the CLI) rather than just args.first(), so that global
    // flags preceding `meta` (e.g. `--dry-run meta help`) are still
    // recognized. This must take priority over the inside-repo passthrough
    // check below, regardless of flag ordering.
    let argv0 = std::iter::once("git-all".to_string());
    if let Ok(cli) = Cli::try_parse_from(argv0.chain(args.iter().cloned()))
        && let Some(Commands::Meta { args }) = &cli.command
    {
        meta::run(args);
        return Ok(());
    }

    if is_inside_git_repo() {
        passthrough_to_git();
    }

    let mut trace = TraceSink::from_env()?;
    let cli = Cli::parse();

    let cwd = std::env::current_dir()?;
    let scan_started_at = Instant::now();
    let repos = find_git_repos_in(&cwd, cli.scan_depth)?;
    let workers = cli.workers.unwrap_or_else(|| default_workers(&cli.command));
    trace.emit_scan(
        command_label(&cli.command),
        &cwd,
        repos.len(),
        workers,
        scan_started_at.elapsed().as_millis(),
    )?;
    if repos.is_empty() {
        println!("No git repositories found in current directory");
        return Ok(());
    }

    let url_scheme = if cli.ssh {
        Some(UrlScheme::Ssh)
    } else if cli.https {
        Some(UrlScheme::Https)
    } else {
        None
    };

    let mut ctx = ExecutionContext::new(
        cli.dry_run,
        url_scheme,
        cli.ssh_multiplexing,
        workers,
        cwd,
        command_label(&cli.command).to_string(),
        trace,
    );

    if cli.dry_run {
        println!(
            "[git-all v{}] Running in **dry-run mode**, no git commands will be executed. Planned git commands below.",
            env!("CARGO_PKG_VERSION")
        );
    }

    match cli.command {
        Some(Commands::Pull { args }) => pull::run(&mut ctx, &repos, &args),
        Some(Commands::Fetch { args }) => fetch::run(&mut ctx, &repos, &args),
        Some(Commands::Status { args }) => status::run(&mut ctx, &repos, &args),
        Some(Commands::External(args)) => passthrough::run(&mut ctx, &repos, &args),
        Some(Commands::Meta { .. }) => unreachable!(), // handled above
        None => {
            // No command given - show help
            println!("No command specified. Use --help for usage information.");
            Ok(())
        }
    }
}
