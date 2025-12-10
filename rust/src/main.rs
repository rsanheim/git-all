use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;
mod repo;
mod runner;

use commands::{fetch, passthrough, pull, status};
use repo::find_git_repos;
use runner::ExecutionContext;

#[derive(Parser)]
#[command(name = "nit", version, about = "parallel git across many repositories")]
struct Cli {
    /// Number of parallel workers
    #[arg(short = 'n', long, default_value_t = 8)]
    workers: usize,

    /// Print exact commands without executing
    #[arg(long)]
    dry_run: bool,

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
    /// Pass through to git (any other command)
    #[command(external_subcommand)]
    External(Vec<String>),
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let repos = find_git_repos()?;
    if repos.is_empty() {
        println!("No git repositories found in current directory");
        return Ok(());
    }

    let ctx = ExecutionContext::new(cli.workers, cli.dry_run);

    if cli.dry_run {
        println!(
            "[nit v{}] Running in **dry-run mode**, no git commands will be executed. Planned git commands below.",
            env!("CARGO_PKG_VERSION")
        );
    }

    match cli.command {
        Some(Commands::Pull { args }) => pull::run(&ctx, &repos, &args),
        Some(Commands::Fetch { args }) => fetch::run(&ctx, &repos, &args),
        Some(Commands::Status { args }) => status::run(&ctx, &repos, &args),
        Some(Commands::External(args)) => passthrough::run(&ctx, &repos, &args),
        None => {
            // No command given - show help
            println!("No command specified. Use --help for usage information.");
            Ok(())
        }
    }
}
