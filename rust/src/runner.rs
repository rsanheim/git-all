use anyhow::Result;
use crossterm::terminal::size as terminal_size;
use crossterm::tty::IsTty;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::printer::{PlainPrinter, Printer, RepoRow, TtyTablePrinter};
use crate::repo::repo_display_name;
use crate::trace::{RepoTraceSample, TraceSink, TraceSummary};

/// Simple counting semaphore using stdlib primitives.
/// Allows limiting concurrent operations to N at a time.
struct Semaphore {
    count: Mutex<usize>,
    cond: Condvar,
}

impl Semaphore {
    fn new(permits: usize) -> Self {
        Semaphore {
            count: Mutex::new(permits),
            cond: Condvar::new(),
        }
    }

    /// Acquire a permit, blocking if none available.
    fn acquire(&self) {
        let mut count = self.count.lock().unwrap();
        while *count == 0 {
            count = self.cond.wait(count).unwrap();
        }
        *count -= 1;
    }

    /// Release a permit, waking one waiting thread.
    fn release(&self) {
        {
            let mut count = self.count.lock().unwrap();
            *count += 1;
        }
        self.cond.notify_one();
    }
}

const MIN_REPO_NAME_WIDTH: usize = 4;
const MAX_REPO_NAME_WIDTH_CAP: usize = 48;

/// Grace period between the polite SIGTERM and the forceful SIGKILL when a quit
/// tears down in-flight git children.
const KILL_GRACE: Duration = Duration::from_millis(1500);

/// Send a signal to a single process (`pid > 0`). Errors (e.g. the process has
/// already exited) are ignored on purpose.
#[cfg(unix)]
fn signal_pid(pid: i32, sig: i32) {
    // SAFETY: `kill` is async-signal-safe and merely posts a signal; a stale pid
    // yields ESRCH, which we intentionally ignore.
    unsafe {
        libc::kill(pid, sig);
    }
}

#[cfg(not(unix))]
fn signal_pid(_pid: i32, _sig: i32) {
    // No POSIX signals off Unix; a quit falls back to waiting for git to finish.
}

#[cfg(unix)]
const SIG_TERM: i32 = libc::SIGTERM;
#[cfg(unix)]
const SIG_KILL: i32 = libc::SIGKILL;
#[cfg(not(unix))]
const SIG_TERM: i32 = 15;
#[cfg(not(unix))]
const SIG_KILL: i32 = 9;

/// Tracks the pids of in-flight git children so a quit from the TUI can signal
/// them (SIGTERM, then SIGKILL) instead of blocking until every network op
/// finishes. git's own ssh/helper subprocesses cascade-exit when git closes
/// their pipes, so signalling git is enough to tear the operation down.
pub(crate) struct CancelRegistry {
    cancelled: AtomicBool,
    /// repo index -> child pid, present only while that child is running.
    running: Mutex<HashMap<usize, i32>>,
}

impl CancelRegistry {
    fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            running: Mutex::new(HashMap::new()),
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Track a freshly-spawned child. If a quit already happened, signal it right
    /// away so it doesn't outlive the cancel. Registration and [`cancel`] both
    /// take the same lock, so no child can slip through unsignalled.
    fn register(&self, idx: usize, pid: i32) {
        let mut running = self.running.lock().unwrap();
        running.insert(idx, pid);
        if self.cancelled.load(Ordering::SeqCst) {
            signal_pid(pid, SIG_TERM);
        }
    }

    fn unregister(&self, idx: usize) {
        self.running.lock().unwrap().remove(&idx);
    }

    /// Terminate every in-flight child, then escalate to SIGKILL after a grace
    /// period for any that ignore SIGTERM (so a wedged process can't stall the
    /// quit). Safe to call once per run.
    pub(crate) fn cancel(self: &Arc<Self>) {
        let pids: Vec<i32> = {
            let running = self.running.lock().unwrap();
            // Flip the flag under the lock so a concurrently-registering worker
            // either shows up here or observes the flag and self-signals.
            self.cancelled.store(true, Ordering::SeqCst);
            running.values().copied().collect()
        };
        for pid in pids {
            signal_pid(pid, SIG_TERM);
        }
        let reg = Arc::clone(self);
        std::thread::spawn(move || {
            std::thread::sleep(KILL_GRACE);
            for pid in reg.running.lock().unwrap().values() {
                signal_pid(*pid, SIG_KILL);
            }
        });
    }
}

/// Trace sample for a completed repo (`None` when `GIT_ALL_TRACE` is off).
type RepoCompletion = Option<RepoTraceSample>;

/// Live execution events emitted by the parallel runner. The TUI consumes these
/// directly (it owns the receiver), so the type is visible crate-wide.
pub(crate) enum RepoEvent {
    Started {
        idx: usize,
    },
    Completed {
        idx: usize,
        result: Result<Output, std::io::Error>,
        trace_sample: Option<RepoTraceSample>,
    },
}

/// URL scheme to force for git operations
#[derive(Clone, Copy)]
pub enum UrlScheme {
    /// Force SSH: git@github.com:user/repo
    Ssh,
    /// Force HTTPS: https://github.com/user/repo
    Https,
}

fn compute_name_width(repos: &[PathBuf], display_root: &Path) -> usize {
    let mut max_len = 0usize;
    for repo in repos {
        let name = repo_display_name(repo, display_root);
        max_len = max_len.max(name.len());
    }

    let capped = max_len.min(MAX_REPO_NAME_WIDTH_CAP);
    capped.max(MIN_REPO_NAME_WIDTH)
}

/// One-line scope banner shown in the TUI header, e.g.
/// `git-all fetch · 98 repos · ~/work · 16 workers`.
fn run_header(ctx: &ExecutionContext, repo_count: usize, max_workers: usize) -> String {
    let workers = match max_workers {
        0 => "unlimited workers".to_string(),
        1 => "1 worker".to_string(),
        n => format!("{n} workers"),
    };
    format!(
        "git-all {} · {} repo{} · {} · {}",
        ctx.command_label(),
        repo_count,
        if repo_count == 1 { "" } else { "s" },
        abbreviate_home(ctx.display_root()),
        workers,
    )
}

/// Render a path with `$HOME` collapsed to `~` for a shorter, friendlier banner.
fn abbreviate_home(path: &Path) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    if home.is_empty() {
        return path.display().to_string();
    }
    match path.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// Cross-cutting options that apply to every git invocation in a run.
#[derive(Clone, Copy)]
pub struct GitInvocationOptions {
    pub url_scheme: Option<UrlScheme>,
    pub ssh_multiplexing: bool,
}

/// Execution context holding configuration for running git commands
pub struct ExecutionContext {
    dry_run: bool,
    url_scheme: Option<UrlScheme>,
    ssh_multiplexing: bool,
    max_connections: usize,
    display_root: PathBuf,
    command_label: String,
    trace: TraceSink,
}

impl ExecutionContext {
    pub fn new(
        dry_run: bool,
        url_scheme: Option<UrlScheme>,
        ssh_multiplexing: bool,
        max_connections: usize,
        display_root: PathBuf,
        command_label: String,
        trace: TraceSink,
    ) -> Self {
        Self {
            dry_run,
            url_scheme,
            ssh_multiplexing,
            max_connections,
            display_root,
            command_label,
            trace,
        }
    }

    pub fn is_dry_run(&self) -> bool {
        self.dry_run
    }

    /// The git subcommand for this run (e.g. `status`, `fetch`), used in the
    /// TUI's scope banner.
    pub fn command_label(&self) -> &str {
        &self.command_label
    }

    pub fn git_invocation_options(&self) -> GitInvocationOptions {
        GitInvocationOptions {
            url_scheme: self.url_scheme,
            ssh_multiplexing: self.ssh_multiplexing,
        }
    }

    pub fn max_connections(&self) -> usize {
        self.max_connections
    }

    pub fn display_root(&self) -> &std::path::Path {
        &self.display_root
    }

    pub fn trace_enabled(&self) -> bool {
        self.trace.enabled()
    }

    pub fn trace_mut(&mut self) -> &mut TraceSink {
        &mut self.trace
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

    /// Spawn the git command without waiting for completion.
    /// Returns immediately with a Child process handle.
    pub fn spawn(&self, opts: GitInvocationOptions) -> std::io::Result<std::process::Child> {
        let mut cmd = Command::new("git");

        // Inject URL scheme override if specified (must come before other args)
        if let Some(scheme) = opts.url_scheme {
            match scheme {
                UrlScheme::Ssh => {
                    cmd.arg("-c")
                        .arg("url.git@github.com:.insteadOf=https://github.com/");
                }
                UrlScheme::Https => {
                    cmd.arg("-c")
                        .arg("url.https://github.com/.insteadOf=git@github.com:");
                }
            }
        }

        // Disable SSH ControlMaster multiplexing by default; it serializes
        // otherwise-parallel network git operations and tanks throughput.
        if !opts.ssh_multiplexing {
            cmd.arg("-c")
                .arg("core.sshCommand=ssh -o ControlMaster=no -o ControlPath=none");
        }

        cmd.arg("-C")
            .arg(&self.repo_path)
            .args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("GIT_TERMINAL_PROMPT", "0")
            .spawn()
    }

    /// Build the full command string for display (used in dry-run)
    pub fn command_string(&self, opts: GitInvocationOptions) -> String {
        let scheme_args = match opts.url_scheme {
            Some(UrlScheme::Ssh) => "-c \"url.git@github.com:.insteadOf=https://github.com/\" ",
            Some(UrlScheme::Https) => "-c \"url.https://github.com/.insteadOf=git@github.com:\" ",
            None => "",
        };
        let ssh_args = if opts.ssh_multiplexing {
            ""
        } else {
            "-c \"core.sshCommand=ssh -o ControlMaster=no -o ControlPath=none\" "
        };
        format!(
            "git {}{}-C {} {}",
            scheme_args,
            ssh_args,
            self.repo_path.display(),
            self.args.join(" ")
        )
    }
}

/// Trait for formatting command output into one line
pub trait OutputFormatter: Sync {
    fn format(&self, output: &Output) -> String;

    fn format_result(&self, result: &Result<Output, std::io::Error>) -> String {
        match result {
            Ok(output) => self.format(output),
            Err(e) => format!("ERROR: {}", e),
        }
    }
}

fn emit_traces_for_printed_rows(
    ctx: &mut ExecutionContext,
    repos: &[PathBuf],
    completions: &[Option<RepoCompletion>],
    printed_indices: &[usize],
    printed_ms: u128,
    summary: &mut TraceSummary,
) -> Result<()> {
    for idx in printed_indices {
        let Some(maybe_sample) = &completions[*idx] else {
            continue;
        };
        if let Some(sample) = maybe_sample {
            summary.record(sample, printed_ms);
            let repo_name = repo_display_name(&repos[*idx], ctx.display_root());
            ctx.trace_mut()
                .emit_repo(*idx, &repo_name, *sample, printed_ms)?;
        }
    }
    Ok(())
}

/// Run commands in parallel across all repos with streaming execution events.
///
/// Repos are pre-sorted alphabetically. The runner emits `Completed` for every
/// repo; when output is a TTY it also emits `Started` so the live table can show
/// running state. Printers decide how to render:
/// - TTY output updates rows in place immediately as repos start and finish
/// - non-TTY output prints only final rows, preserving alphabetical order
///
/// Uses a thread-per-process pattern with `wait_with_output()`, which is
/// deadlock-safe because stdlib drains stdout/stderr concurrently.
pub fn run_parallel<F>(
    ctx: &mut ExecutionContext,
    repos: &[PathBuf],
    build_command: F,
    formatter: &dyn OutputFormatter,
) -> Result<()>
where
    F: Fn(&PathBuf) -> GitCommand + Sync,
{
    let opts = ctx.git_invocation_options();
    let trace_enabled = ctx.trace_enabled();

    if ctx.is_dry_run() {
        for repo in repos {
            let cmd = build_command(repo);
            println!("{}", cmd.command_string(opts));
        }
        return Ok(());
    }

    let name_width = compute_name_width(repos, ctx.display_root());
    let max_workers = ctx.max_connections();
    let run_started_at = Instant::now();
    let mut rows: Vec<RepoRow> = repos
        .iter()
        .map(|repo| RepoRow::pending(repo_display_name(repo, ctx.display_root())))
        .collect();

    let stdout = std::io::stdout();
    let is_tty = stdout.is_tty();
    // The full-screen ratatui TUI takes over an interactive terminal. Trace mode
    // writes structured records to stderr/a file and must stay plain, so it falls
    // back to the line printers.
    let use_tui = is_tty && !trace_enabled;
    // 0 means the terminal did not report a size; the line printer falls back to
    // a sensible default. A real width (however small) is passed through as-is.
    let terminal_columns = if is_tty {
        terminal_size()
            .map(|(columns, _rows)| columns as usize)
            .unwrap_or(0)
    } else {
        0
    };

    let semaphore = if max_workers > 0 && max_workers < repos.len() {
        Some(Arc::new(Semaphore::new(max_workers)))
    } else {
        None
    };

    // Only the TUI has a quit key, so non-TUI runs skip the cancel registry and
    // its per-child pid bookkeeping entirely.
    let cancel: Option<Arc<CancelRegistry>> = use_tui.then(|| Arc::new(CancelRegistry::new()));

    let mut completions: Vec<Option<RepoCompletion>> = (0..repos.len()).map(|_| None).collect();
    let mut summary = TraceSummary::default();

    let (tx, rx) = mpsc::channel();

    let build_git_cmd = &build_command;
    std::thread::scope(|s| -> Result<()> {
        for (idx, _) in repos.iter().enumerate() {
            let tx = tx.clone();
            let sem = semaphore.clone();
            let cancel = cancel.clone();

            s.spawn(move || {
                if let Some(ref sem) = sem {
                    sem.acquire();
                }
                // A quit may have arrived while we waited for a worker slot; if
                // so, don't start new git work.
                if cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
                    if let Some(ref sem) = sem {
                        sem.release();
                    }
                    return;
                }
                if is_tty {
                    let _ = tx.send(RepoEvent::Started { idx });
                }

                let start_ms = run_started_at.elapsed().as_millis();
                let cmd = build_git_cmd(&repos[idx]);
                let spawn_result = cmd.spawn(opts);
                let spawn_ms = run_started_at.elapsed().as_millis();
                let result = match spawn_result {
                    Ok(child) => {
                        // Track the pid so a quit can signal it; drop it from the
                        // registry as soon as it exits so we never signal a
                        // reaped pid.
                        if let Some(ref reg) = cancel {
                            reg.register(idx, child.id() as i32);
                        }
                        let out = child.wait_with_output();
                        if let Some(ref reg) = cancel {
                            reg.unregister(idx);
                        }
                        out
                    }
                    Err(err) => Err(err),
                };

                let trace_sample = trace_enabled.then(|| {
                    let (stdout_bytes, stderr_bytes, success) = match &result {
                        Ok(output) => (
                            output.stdout.len(),
                            output.stderr.len(),
                            output.status.success(),
                        ),
                        Err(_) => (0, 0, false),
                    };
                    RepoTraceSample {
                        start_ms,
                        spawn_ms,
                        exit_ms: run_started_at.elapsed().as_millis(),
                        stdout_bytes,
                        stderr_bytes,
                        success,
                    }
                });

                if let Some(ref sem) = sem {
                    sem.release();
                }

                let _ = tx.send(RepoEvent::Completed {
                    idx,
                    result,
                    trace_sample,
                });
            });
        }
        drop(tx);

        if use_tui {
            // The TUI owns the receiver and renders the live full-screen view.
            let header = run_header(ctx, repos.len(), max_workers);
            let cancel = cancel.clone().expect("use_tui implies a cancel registry");
            crate::tui::run(
                rx,
                &mut rows,
                &header,
                name_width,
                run_started_at,
                formatter,
                cancel,
            )?;
        } else {
            let stdout = std::io::stdout().lock();
            let mut printer: Box<dyn Printer + '_> = if is_tty {
                Box::new(TtyTablePrinter::new(stdout, terminal_columns, name_width))
            } else {
                Box::new(PlainPrinter::new(stdout, name_width))
            };
            printer.start(&rows)?;

            for event in rx {
                match event {
                    RepoEvent::Started { idx } => {
                        rows[idx].mark_running();
                        let elapsed_ms = run_started_at.elapsed().as_millis();
                        let _ = printer.update_row(&rows, idx, elapsed_ms)?;
                    }
                    RepoEvent::Completed {
                        idx,
                        result,
                        trace_sample,
                    } => {
                        rows[idx].mark_finished(formatter.format_result(&result));
                        completions[idx] = Some(trace_sample);
                        let elapsed_ms = run_started_at.elapsed().as_millis();
                        let printed = printer.update_row(&rows, idx, elapsed_ms)?;
                        emit_traces_for_printed_rows(
                            ctx,
                            repos,
                            &completions,
                            &printed,
                            elapsed_ms,
                            &mut summary,
                        )?;
                    }
                }
            }

            let total_ms = run_started_at.elapsed().as_millis();
            let printed = printer.complete(&rows, total_ms)?;
            emit_traces_for_printed_rows(
                ctx,
                repos,
                &completions,
                &printed,
                total_ms,
                &mut summary,
            )?;
        }
        Ok(())
    })?;

    let total_ms = run_started_at.elapsed().as_millis();
    if use_tui {
        // Alt-screen output vanishes on exit, so leave a plain record behind.
        let header = run_header(ctx, repos.len(), max_workers);
        crate::tui::print_summary(&rows, &header, name_width, total_ms)?;
    } else {
        ctx.trace_mut()
            .emit_summary(repos.len(), &summary, total_ms)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_name_width_caps_and_min() {
        let root = PathBuf::from("/workspace");
        let repos = vec![
            root.join("a"),
            root.join("short"),
            root.join("this-is-a-very-long-repository-name-that-exceeds-cap"),
        ];
        let width = compute_name_width(&repos, &root);
        assert_eq!(width, MAX_REPO_NAME_WIDTH_CAP);

        let tiny = vec![root.join("a")];
        let tiny_width = compute_name_width(&tiny, &root);
        assert_eq!(tiny_width, MIN_REPO_NAME_WIDTH);
    }

    #[test]
    fn cancel_registry_flips_flag() {
        let reg = Arc::new(CancelRegistry::new());
        assert!(!reg.is_cancelled());
        // Empty registry: cancel has nothing to signal, it just flips the flag.
        reg.cancel();
        assert!(reg.is_cancelled());
    }

    #[test]
    fn cancel_registry_tracks_running_children() {
        // A non-cancelled registry only bookkeeps pids; it does not signal them,
        // so this touches no real process.
        let reg = CancelRegistry::new();
        reg.register(0, 424_242);
        reg.register(1, 424_243);
        assert_eq!(reg.running.lock().unwrap().len(), 2);
        reg.unregister(0);
        assert!(!reg.is_cancelled());
        assert_eq!(
            reg.running
                .lock()
                .unwrap()
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            vec![1]
        );
    }

    /// Test that large output (>64KB) doesn't cause pipe buffer deadlock.
    /// wait_with_output() internally spawns threads to drain pipes, so this should complete.
    #[test]
    fn test_large_output_no_deadlock() {
        use std::process::Stdio;
        use std::time::{Duration, Instant};

        let start = Instant::now();
        let timeout = Duration::from_secs(5);

        // Spawn a process that outputs 100KB (more than 64KB pipe buffer)
        let child = Command::new("head")
            .args(["-c", "100000", "/dev/zero"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Failed to spawn head command");

        // wait_with_output handles pipe draining internally - no deadlock
        let output = child.wait_with_output().expect("Failed to wait for output");

        // Verify we got all the output
        assert_eq!(
            output.stdout.len(),
            100000,
            "Expected 100000 bytes, got {}",
            output.stdout.len()
        );

        // Verify it didn't take suspiciously long (would indicate near-deadlock)
        assert!(
            start.elapsed() < timeout,
            "Test took too long - possible deadlock: {:?}",
            start.elapsed()
        );
    }
}
