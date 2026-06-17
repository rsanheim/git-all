//! Dev automation for git-all. Run via `cargo xtask <task>`.
//!
//! Tasks:
//!   mangen [OUT_DIR]   render the git-all(1) man page (default: target/man)

use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("mangen") => mangen(args.next()),
        other => {
            eprintln!("usage: cargo xtask mangen [OUT_DIR]");
            if let Some(task) = other {
                eprintln!("unknown task: {task}");
            }
            std::process::exit(2);
        }
    }
}

/// Render git-all.1 from the binary's own clap definition (single source of
/// truth), so the man page can never drift from the actual CLI.
fn mangen(out_dir: Option<String>) -> anyhow::Result<()> {
    let out_dir = PathBuf::from(out_dir.unwrap_or_else(|| "target/man".to_string()));
    std::fs::create_dir_all(&out_dir)?;

    let man = clap_mangen::Man::new(git_all::cli_command());
    let mut buffer: Vec<u8> = Vec::new();
    man.render(&mut buffer)?;

    let path = out_dir.join("git-all.1");
    std::fs::write(&path, &buffer)?;
    eprintln!("wrote {} ({} bytes)", path.display(), buffer.len());
    Ok(())
}
