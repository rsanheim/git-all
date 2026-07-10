//! Full-screen ratatui TUI for live `git-all` runs.
//!
//! Active only on an interactive stdout that is not `--dry-run` or trace mode.
//! It owns the runner's [`RepoEvent`] channel and renders a yazi-style bordered
//! layout — a scope header, a scrolling per-repo table, and a progress footer —
//! then restores the terminal cleanly on completion, quit, or panic.

use std::io::{self, Write};
use std::sync::Once;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::cursor::{Hide, Show};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Gauge, Paragraph, Row, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Table, TableState,
};

use crate::printer::{RepoRow, RowState, format_repo_name};
use crate::runner::{OutputFormatter, RepoEvent};

/// Redraw / input-poll cadence. Drives the spinner and elapsed clock.
const TICK: Duration = Duration::from_millis(100);

/// Braille spinner frames for in-flight repos.
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// A small, terminal-safe palette. Named colors keep it readable everywhere;
// indexed grays give the yazi-like muted frame and subtle row highlight.
const ACCENT: Color = Color::Cyan;
const FRAME: Color = Color::Indexed(240);
const MUTED: Color = Color::DarkGray;
const TEXT: Color = Color::Gray;
const HIGHLIGHT_BG: Color = Color::Indexed(236);
const GAUGE_BG: Color = Color::Indexed(238);

/// Per-repo visual outcome, derived from row state and the result string.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Outcome {
    Pending,
    Running,
    Ok,
    Notable,
    Error,
}

impl Outcome {
    fn color(self) -> Color {
        match self {
            Outcome::Pending => MUTED,
            Outcome::Running => ACCENT,
            Outcome::Ok => Color::Green,
            Outcome::Notable => Color::Yellow,
            Outcome::Error => Color::Red,
        }
    }

    /// Static glyph. Running rows substitute the animated spinner frame.
    fn glyph(self) -> &'static str {
        match self {
            Outcome::Pending => "○",
            Outcome::Running => "◍",
            Outcome::Ok => "✓",
            Outcome::Notable => "●",
            Outcome::Error => "✗",
        }
    }
}

fn outcome_of(row: &RepoRow) -> Outcome {
    match row.state {
        RowState::Pending => Outcome::Pending,
        RowState::Running => Outcome::Running,
        RowState::Finished => classify_finished(&row.output),
    }
}

/// Split finished results into green (nothing to do), yellow (something changed),
/// and red (failed) buckets from the formatter's one-line summary.
fn classify_finished(output: &str) -> Outcome {
    let lower = output.to_ascii_lowercase();
    if lower.contains("error") || lower.contains("fatal") || lower.contains("fail") {
        Outcome::Error
    } else if matches!(
        output,
        "clean" | "no new commits" | "fetched" | "Already up to date"
    ) {
        Outcome::Ok
    } else {
        Outcome::Notable
    }
}

#[derive(Default)]
struct Counts {
    total: usize,
    done: usize,
    ok: usize,
    notable: usize,
    error: usize,
    running: usize,
    pending: usize,
}

fn tally(rows: &[RepoRow]) -> Counts {
    let mut c = Counts {
        total: rows.len(),
        ..Counts::default()
    };
    for row in rows {
        match outcome_of(row) {
            Outcome::Pending => c.pending += 1,
            Outcome::Running => c.running += 1,
            Outcome::Ok => {
                c.done += 1;
                c.ok += 1;
            }
            Outcome::Notable => {
                c.done += 1;
                c.notable += 1;
            }
            Outcome::Error => {
                c.done += 1;
                c.error += 1;
            }
        }
    }
    c
}

/// Index of the last repo that has left the pending state — the "leading edge"
/// of activity the view auto-follows so progress stays on screen.
fn frontier(rows: &[RepoRow]) -> usize {
    rows.iter()
        .rposition(|r| r.state != RowState::Pending)
        .unwrap_or(0)
}

/// Restore raw mode, alt-screen, and the cursor on the way out. Held for the
/// lifetime of a TUI session so any early return leaves the terminal usable.
struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, Hide)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
}

/// Restore the terminal before a panic prints, so a crash mid-run doesn't leave
/// the shell in raw mode / the alt-screen.
fn install_panic_hook() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let original = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore_terminal();
            original(info);
        }));
    });
}

/// Run the live TUI, consuming runner events until every repo finishes or the
/// user quits. Mutates `rows` in place so the caller can print a final record.
pub(crate) fn run(
    rx: Receiver<RepoEvent>,
    rows: &mut [RepoRow],
    header: &str,
    name_width: usize,
    started_at: Instant,
    formatter: &dyn OutputFormatter,
) -> Result<()> {
    install_panic_hook();
    let _guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    let mut follow = true;
    let mut selected = 0usize;
    let last = rows.len().saturating_sub(1);

    loop {
        // Drain everything the workers have sent since the last frame.
        let mut disconnected = false;
        loop {
            match rx.try_recv() {
                Ok(RepoEvent::Started { idx }) => rows[idx].mark_running(),
                Ok(RepoEvent::Completed { idx, result, .. }) => {
                    rows[idx].mark_finished(formatter.format_result(&result));
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }

        if follow {
            selected = frontier(rows);
        }
        selected = selected.min(last);

        let elapsed = started_at.elapsed();
        terminal.draw(|f| draw(f, rows, header, name_width, elapsed, selected))?;

        // Exit once the channel is closed and nothing is left in flight.
        if disconnected && rows.iter().all(|r| r.state == RowState::Finished) {
            break;
        }

        if !event::poll(TICK)? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => break,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
            KeyCode::Char('j') | KeyCode::Down => {
                follow = false;
                selected = (selected + 1).min(last);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                follow = false;
                selected = selected.saturating_sub(1);
            }
            KeyCode::Char('g') | KeyCode::Home => {
                follow = false;
                selected = 0;
            }
            KeyCode::Char('G') | KeyCode::End => follow = true,
            _ => {}
        }
    }

    Ok(())
}

fn draw(
    f: &mut Frame,
    rows: &[RepoRow],
    header: &str,
    name_width: usize,
    elapsed: Duration,
    selected: usize,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(1),    // repo table
            Constraint::Length(4), // progress footer
        ])
        .split(f.area());

    render_header(f, chunks[0], header, elapsed);
    render_table(f, chunks[1], rows, name_width, elapsed, selected);
    render_footer(f, chunks[2], rows);
}

fn framed(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(FRAME))
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
}

fn render_header(f: &mut Frame, area: Rect, header: &str, elapsed: Duration) {
    let block = framed("git-all");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(10), Constraint::Length(10)])
        .split(inner);

    f.render_widget(
        Paragraph::new(Line::from(header)).style(Style::default().fg(TEXT)),
        cols[0],
    );
    f.render_widget(
        Paragraph::new(Line::from(fmt_elapsed(elapsed)))
            .alignment(Alignment::Right)
            .style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        cols[1],
    );
}

fn render_table(
    f: &mut Frame,
    area: Rect,
    rows: &[RepoRow],
    name_width: usize,
    elapsed: Duration,
    selected: usize,
) {
    let c = tally(rows);
    let block = framed(&format!("repositories · {}/{} done", c.done, c.total));
    let spinner = SPINNER[(elapsed.as_millis() / 90) as usize % SPINNER.len()];

    let table_rows: Vec<Row> = rows
        .iter()
        .map(|row| {
            let outcome = outcome_of(row);
            let glyph = if outcome == Outcome::Running {
                spinner
            } else {
                outcome.glyph()
            };
            let result = match outcome {
                Outcome::Pending => "pending".to_string(),
                Outcome::Running => "running…".to_string(),
                _ => row.output.clone(),
            };
            let name_style = if outcome == Outcome::Pending {
                Style::default().fg(MUTED)
            } else {
                Style::default().fg(TEXT)
            };
            Row::new(vec![
                Cell::from(Span::styled(
                    glyph,
                    Style::default()
                        .fg(outcome.color())
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(Span::styled(row.repo.clone(), name_style)),
                Cell::from(Span::styled(result, Style::default().fg(outcome.color()))),
            ])
        })
        .collect();

    let name_col = (name_width as u16).clamp(8, 48);
    let widths = [
        Constraint::Length(1),
        Constraint::Length(name_col),
        Constraint::Min(10),
    ];
    let table = Table::new(table_rows, widths)
        .header(
            Row::new(vec![
                Cell::from(""),
                Cell::from("repository"),
                Cell::from("result"),
            ])
            .style(Style::default().fg(MUTED).add_modifier(Modifier::BOLD)),
        )
        .block(block)
        .column_spacing(1)
        .row_highlight_style(Style::default().bg(HIGHLIGHT_BG));

    let mut state = TableState::default().with_selected(Some(selected));
    f.render_stateful_widget(table, area, &mut state);

    // Scrollbar only when the list overflows the visible rows (minus borders and
    // the table's own header row).
    let visible = area.height.saturating_sub(3) as usize;
    if rows.len() > visible && visible > 0 {
        let mut sb = ScrollbarState::new(rows.len()).position(selected);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_symbol("█")
                .track_symbol(Some("│"))
                .style(Style::default().fg(FRAME)),
            area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut sb,
        );
    }
}

fn render_footer(f: &mut Frame, area: Rect, rows: &[RepoRow]) {
    let c = tally(rows);
    let block = framed("progress");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    let ratio = if c.total == 0 {
        0.0
    } else {
        c.done as f64 / c.total as f64
    };
    f.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(Color::Green).bg(GAUGE_BG))
            .ratio(ratio)
            .label(format!("{}/{}  {:.0}%", c.done, c.total, ratio * 100.0)),
        lines[0],
    );

    let status = Line::from(vec![
        count_span("✓", c.ok, "ok", Color::Green),
        Span::raw("   "),
        count_span("●", c.notable, "changed", Color::Yellow),
        Span::raw("   "),
        count_span("✗", c.error, "error", Color::Red),
        Span::raw("   "),
        count_span("◍", c.running, "running", ACCENT),
        Span::raw("   "),
        count_span("○", c.pending, "pending", MUTED),
    ]);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(10), Constraint::Length(20)])
        .split(lines[1]);
    f.render_widget(Paragraph::new(status), cols[0]);
    f.render_widget(
        Paragraph::new(Line::from("q quit · j/k scroll"))
            .alignment(Alignment::Right)
            .style(Style::default().fg(MUTED)),
        cols[1],
    );
}

fn count_span(glyph: &str, n: usize, label: &str, color: Color) -> Span<'static> {
    Span::styled(format!("{glyph} {n} {label}"), Style::default().fg(color))
}

fn fmt_elapsed(elapsed: Duration) -> String {
    let secs = elapsed.as_secs_f64();
    if secs < 60.0 {
        format!("{secs:.1}s")
    } else {
        format!("{}m{:02}s", (secs / 60.0) as u64, (secs % 60.0) as u64)
    }
}

/// Print a plain, greppable record of the run to normal scrollback after the
/// alt-screen closes, since its contents vanish on exit.
pub(crate) fn print_summary(
    rows: &[RepoRow],
    header: &str,
    name_width: usize,
    elapsed_ms: u128,
) -> io::Result<()> {
    let mut out = io::stdout().lock();
    writeln!(out, "{header}")?;
    for row in rows {
        writeln!(
            out,
            "{} {}",
            format_repo_name(&row.repo, name_width),
            row.output
        )?;
    }
    let c = tally(rows);
    writeln!(
        out,
        "{} of {} done · {} ok · {} changed · {} error · {:.1}s",
        c.done,
        c.total,
        c.ok,
        c.notable,
        c.error,
        elapsed_ms as f64 / 1000.0,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn rows() -> Vec<RepoRow> {
        vec![
            RepoRow::finished("activities".to_string(), "clean".to_string()),
            RepoRow::finished("billing-svc".to_string(), "2 modified".to_string()),
            RepoRow::finished("edge-proxy".to_string(), "fatal: bad ref".to_string()),
            RepoRow::running("gateway".to_string()),
            RepoRow::pending("zebra-cache".to_string()),
        ]
    }

    #[test]
    fn classify_buckets_results_by_color() {
        assert_eq!(classify_finished("clean"), Outcome::Ok);
        assert_eq!(classify_finished("no new commits"), Outcome::Ok);
        assert_eq!(
            classify_finished("2 modified, 1 untracked"),
            Outcome::Notable
        );
        assert_eq!(classify_finished("1 branch updated"), Outcome::Notable);
        assert_eq!(
            classify_finished("fatal: not a git repository"),
            Outcome::Error
        );
        assert_eq!(classify_finished("ERROR: broken pipe"), Outcome::Error);
    }

    #[test]
    fn tally_counts_each_state() {
        let c = tally(&rows());
        assert_eq!(c.total, 5);
        assert_eq!(c.done, 3);
        assert_eq!(c.ok, 1);
        assert_eq!(c.notable, 1);
        assert_eq!(c.error, 1);
        assert_eq!(c.running, 1);
        assert_eq!(c.pending, 1);
    }

    #[test]
    fn frontier_tracks_last_active_repo() {
        // gateway (idx 3) is the last non-pending row.
        assert_eq!(frontier(&rows()), 3);
    }

    #[test]
    fn fmt_elapsed_switches_to_minutes() {
        assert_eq!(fmt_elapsed(Duration::from_millis(3400)), "3.4s");
        assert_eq!(fmt_elapsed(Duration::from_secs(75)), "1m15s");
    }

    #[test]
    fn draw_renders_header_table_and_footer() {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let data = rows();
        terminal
            .draw(|f| {
                draw(
                    f,
                    &data,
                    "git-all status · 5 repos · ~/work · 8 workers",
                    14,
                    Duration::from_millis(1200),
                    3,
                )
            })
            .expect("draw");

        let buffer = terminal.backend().buffer().clone();
        let text: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
        assert!(text.contains("git-all"));
        assert!(text.contains("activities"));
        assert!(text.contains("repositories"));
        assert!(text.contains("progress"));
    }
}
