use crossterm::cursor::{MoveToColumn, MoveUp};
use crossterm::queue;
use crossterm::terminal::{Clear, ClearType};
use std::io::{self, Write};

/// Fallback width used when the terminal does not report its size (0 columns).
/// A real width, however small, is always respected as-is.
pub(crate) const DEFAULT_TERMINAL_COLUMNS: usize = 80;

fn display_repo_name(name: &str, width: usize) -> String {
    if name.len() <= width {
        return name.to_string();
    }
    if width <= 4 {
        return name.chars().take(width).collect();
    }
    let end = name.floor_char_boundary(width - 4);
    format!("{}-...", &name[..end])
}

pub(crate) fn format_repo_name(name: &str, width: usize) -> String {
    format!(
        "[{:<width$}]",
        display_repo_name(name, width),
        width = width
    )
}

/// Names of in-progress repos for the footer label, fitted to `width`.
/// A single repo reads "name in-progress"; several are comma-joined with a
/// trailing " ..." when not all of them fit.
fn in_progress_label(names: &[&str], width: usize) -> String {
    if let [only] = names {
        let label = format!("{only} in-progress");
        return if label.len() <= width {
            label
        } else {
            display_repo_name(only, width)
        };
    }
    let mut label = String::new();
    let mut shown = 0;
    for (i, name) in names.iter().enumerate() {
        let sep = if i == 0 { "" } else { ", " };
        let more = if i + 1 < names.len() { " ..." } else { "" };
        if label.len() + sep.len() + name.len() + more.len() > width {
            break;
        }
        label.push_str(sep);
        label.push_str(name);
        shown += 1;
    }
    if shown < names.len() {
        if label.is_empty() {
            label = names[0].chars().take(width.saturating_sub(4)).collect();
        }
        label.push_str(" ...");
    }
    let end = label.floor_char_boundary(width);
    label.truncate(end);
    label
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowState {
    Pending,
    Running,
    Finished,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoRow {
    pub repo: String,
    pub output: String,
    pub state: RowState,
    started_ms: Option<u128>,
}

impl RepoRow {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn pending(repo: String) -> Self {
        Self {
            repo,
            output: "pending".to_string(),
            state: RowState::Pending,
            started_ms: None,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn running(repo: String) -> Self {
        Self {
            repo,
            output: "running".to_string(),
            state: RowState::Running,
            started_ms: Some(0),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn finished(repo: String, output: String) -> Self {
        Self {
            repo,
            output,
            state: RowState::Finished,
            started_ms: None,
        }
    }

    pub fn mark_running(&mut self, elapsed_ms: u128) {
        self.output = "running".to_string();
        self.state = RowState::Running;
        self.started_ms = Some(elapsed_ms);
    }

    pub fn mark_finished(&mut self, output: String) {
        self.output = output;
        self.state = RowState::Finished;
        self.started_ms = None;
    }
}

pub struct FooterState {
    pub total_rows: usize,
    pub complete: usize,
    pub running: usize,
    pub pending: usize,
    pub elapsed_ms: u128,
}

impl FooterState {
    pub fn render_message(&self) -> String {
        format!(
            "{} of {} done | {} running | {} pending | {:.1}s",
            self.complete,
            self.total_rows,
            self.running,
            self.pending,
            self.elapsed_ms as f64 / 1000.0,
        )
    }
}

pub trait Printer {
    fn start(&mut self, rows: &[RepoRow]) -> io::Result<()>;
    fn update_row(
        &mut self,
        rows: &[RepoRow],
        row_index: usize,
        elapsed_ms: u128,
    ) -> io::Result<Vec<usize>>;
    fn complete(&mut self, rows: &[RepoRow], elapsed_ms: u128) -> io::Result<Vec<usize>>;

    /// Refresh live progress even when no process has started or finished.
    fn tick(&mut self, _rows: &[RepoRow], _elapsed_ms: u128) -> io::Result<()> {
        Ok(())
    }
}

pub struct PlainPrinter<W: Write> {
    writer: W,
    repo_width: usize,
    next_to_print: usize,
}

impl<W: Write> PlainPrinter<W> {
    pub fn new(writer: W, repo_width: usize) -> Self {
        Self {
            writer,
            repo_width,
            next_to_print: 0,
        }
    }

    fn flush_finished_rows(&mut self, rows: &[RepoRow]) -> io::Result<Vec<usize>> {
        let mut printed = Vec::new();
        while self.next_to_print < rows.len()
            && rows[self.next_to_print].state == RowState::Finished
        {
            let row = &rows[self.next_to_print];
            writeln!(
                self.writer,
                "{} {}",
                format_repo_name(&row.repo, self.repo_width),
                row.output
            )?;
            printed.push(self.next_to_print);
            self.next_to_print += 1;
        }
        Ok(printed)
    }
}

impl<W: Write> Printer for PlainPrinter<W> {
    fn start(&mut self, _rows: &[RepoRow]) -> io::Result<()> {
        Ok(())
    }

    fn update_row(
        &mut self,
        rows: &[RepoRow],
        row_index: usize,
        _elapsed_ms: u128,
    ) -> io::Result<Vec<usize>> {
        if rows[row_index].state != RowState::Finished {
            return Ok(Vec::new());
        }
        self.flush_finished_rows(rows)
    }

    fn complete(&mut self, rows: &[RepoRow], _elapsed_ms: u128) -> io::Result<Vec<usize>> {
        self.flush_finished_rows(rows)
    }
}

pub struct TtyTablePrinter<W: Write> {
    writer: W,
    terminal_columns: usize,
    repo_width: usize,
    next_to_print: usize,
    footer_active: bool,
}

impl<W: Write> TtyTablePrinter<W> {
    const FOOTER_HEIGHT: u16 = 2;

    pub fn new(writer: W, terminal_columns: usize, repo_width: usize) -> Self {
        Self {
            writer,
            terminal_columns,
            repo_width,
            next_to_print: 0,
            footer_active: false,
        }
    }

    fn terminal_width(&self) -> usize {
        if self.terminal_columns == 0 {
            DEFAULT_TERMINAL_COLUMNS
        } else {
            self.terminal_columns
        }
    }

    fn fit_line(&self, line: &str) -> String {
        let width = self.terminal_width();
        if line.len() <= width {
            return line.to_string();
        }
        let end = line.floor_char_boundary(width);
        line[..end].to_string()
    }

    fn render_finished_row(&self, row: &RepoRow) -> String {
        format!(
            "{} {}",
            format_repo_name(&row.repo, self.repo_width),
            row.output
        )
    }

    fn flush_finished_rows(&mut self, rows: &[RepoRow]) -> io::Result<Vec<usize>> {
        let mut printed = Vec::new();
        while self.next_to_print < rows.len()
            && rows[self.next_to_print].state == RowState::Finished
        {
            let row = &rows[self.next_to_print];
            writeln!(self.writer, "{}", self.render_finished_row(row))?;
            printed.push(self.next_to_print);
            self.next_to_print += 1;
        }
        Ok(printed)
    }

    fn clear_footer(&mut self) -> io::Result<()> {
        if self.footer_active {
            queue!(
                self.writer,
                MoveToColumn(0),
                MoveUp(Self::FOOTER_HEIGHT),
                Clear(ClearType::FromCursorDown)
            )?;
            self.footer_active = false;
        }
        Ok(())
    }

    fn render_footer(&mut self, rows: &[RepoRow], elapsed_ms: u128) -> io::Result<()> {
        let mut complete = 0usize;
        let mut running = 0usize;
        let mut pending = 0usize;
        for row in rows {
            match row.state {
                RowState::Finished => complete += 1,
                RowState::Running => running += 1,
                RowState::Pending => pending += 1,
            }
        }
        let footer = FooterState {
            total_rows: rows.len(),
            complete,
            running,
            pending,
            elapsed_ms,
        };
        let label = if self.next_to_print == rows.len() {
            "done".to_string()
        } else {
            // Row order keeps the repo blocking output first in the label.
            let names_in = |state: RowState| -> Vec<&str> {
                rows.iter()
                    .filter(|row| row.state == state)
                    .map(|row| row.repo.as_str())
                    .collect()
            };
            let mut names = names_in(RowState::Running);
            if names.is_empty() {
                names = names_in(RowState::Pending);
            }
            in_progress_label(&names, self.repo_width)
        };
        let summary = format!(
            "{} {}",
            format_repo_name(&label, self.repo_width),
            footer.render_message()
        );
        let progress = "-".repeat(self.terminal_width());
        writeln!(self.writer, "{}", self.fit_line(&progress))?;
        writeln!(self.writer, "{}", self.fit_line(&summary))?;
        self.writer.flush()?;
        self.footer_active = true;
        Ok(())
    }
}

impl<W: Write> Printer for TtyTablePrinter<W> {
    fn start(&mut self, rows: &[RepoRow]) -> io::Result<()> {
        self.render_footer(rows, 0)
    }

    fn update_row(
        &mut self,
        rows: &[RepoRow],
        _row_index: usize,
        elapsed_ms: u128,
    ) -> io::Result<Vec<usize>> {
        self.clear_footer()?;
        let printed = self.flush_finished_rows(rows)?;
        self.render_footer(rows, elapsed_ms)?;
        Ok(printed)
    }

    fn complete(&mut self, rows: &[RepoRow], elapsed_ms: u128) -> io::Result<Vec<usize>> {
        self.clear_footer()?;
        let printed = self.flush_finished_rows(rows)?;
        self.render_footer(rows, elapsed_ms)?;
        Ok(printed)
    }

    fn tick(&mut self, rows: &[RepoRow], elapsed_ms: u128) -> io::Result<()> {
        self.clear_footer()?;
        self.render_footer(rows, elapsed_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Clone, Default)]
    struct SharedBuffer(Rc<RefCell<Vec<u8>>>);

    impl SharedBuffer {
        fn rendered(&self) -> String {
            String::from_utf8(self.0.borrow().clone()).expect("utf8")
        }
    }

    impl Write for SharedBuffer {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn strip_ansi_sequences(input: &str) -> String {
        let mut stripped = String::new();
        let mut chars = input.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch != '\u{1b}' {
                stripped.push(ch);
                continue;
            }

            if chars.next() != Some('[') {
                continue;
            }

            for code_ch in chars.by_ref() {
                if code_ch.is_ascii_alphabetic() {
                    break;
                }
            }
        }
        stripped
    }

    #[test]
    fn format_repo_name_pads_short() {
        let result = format_repo_name("my-repo", 24);
        assert_eq!(result, "[my-repo                 ]");
        assert_eq!(result.len(), 26);
    }

    #[test]
    fn format_repo_name_exact_length() {
        let result = format_repo_name("exactly-twenty-four-chr", 24);
        assert_eq!(result.len(), 26);
    }

    #[test]
    fn format_repo_name_truncates_long() {
        let result = format_repo_name("this-is-a-very-long-repository-name", 24);
        assert_eq!(result, "[this-is-a-very-long--...]");
        assert_eq!(result.len(), 26);
    }

    #[test]
    fn display_repo_name_handles_multibyte_truncation() {
        // floor_char_boundary ensures we never slice through a UTF-8 code point.
        let name = "héllo-wörld-🦀-crates";
        let _ = display_repo_name(name, 10);
    }

    #[test]
    fn plain_printer_formats_repo_and_output_without_ansi() {
        let mut output = Vec::new();
        let rows = vec![RepoRow::finished(
            "agentic-dev".to_string(),
            "running".to_string(),
        )];

        {
            let mut printer = PlainPrinter::new(&mut output, 12);
            printer.start(&rows).expect("plain start");
            printer.update_row(&rows, 0, 0).expect("plain finish");
        }

        let rendered = String::from_utf8(output).expect("utf8");
        assert_eq!(rendered, "[agentic-dev ] running\n");
        assert!(!rendered.contains('\u{1b}'));
    }

    #[test]
    fn plain_printer_truncates_long_repo_names() {
        let mut output = Vec::new();
        let rows = vec![RepoRow::finished(
            "this-is-a-very-long-repository-name".to_string(),
            "clean".to_string(),
        )];

        {
            let mut printer = PlainPrinter::new(&mut output, 24);
            printer.start(&rows).expect("plain start");
            printer.update_row(&rows, 0, 0).expect("plain finish");
        }

        let rendered = String::from_utf8(output).expect("utf8");
        assert_eq!(rendered, "[this-is-a-very-long--...] clean\n");
    }

    #[test]
    fn plain_printer_buffers_out_of_order_finished_rows_until_contiguous() {
        let output = SharedBuffer::default();
        let mut rows = vec![
            RepoRow::running("activities".to_string()),
            RepoRow::running("agentic-dev".to_string()),
        ];

        {
            let mut printer = PlainPrinter::new(output.clone(), 12);
            printer.start(&rows).expect("plain start");

            rows[1] = RepoRow::finished("agentic-dev".to_string(), "clean".to_string());
            printer
                .update_row(&rows, 1, 100)
                .expect("plain first finish");
            assert_eq!(output.rendered(), "");

            rows[0] = RepoRow::finished("activities".to_string(), "clean".to_string());
            printer
                .update_row(&rows, 0, 200)
                .expect("plain second finish");
        }

        let rendered = output.rendered();
        assert_eq!(rendered, "[activities  ] clean\n[agentic-dev ] clean\n");
    }

    #[test]
    fn footer_message_includes_done_running_pending_and_elapsed() {
        let footer = FooterState {
            total_rows: 98,
            complete: 41,
            running: 8,
            pending: 49,
            elapsed_ms: 2100,
        };

        assert_eq!(
            footer.render_message(),
            "41 of 98 done | 8 running | 49 pending | 2.1s"
        );
    }

    #[test]
    fn tty_table_printer_renders_summary_row_with_separator() {
        let rows = vec![RepoRow::finished(
            "activities".to_string(),
            "clean".to_string(),
        )];
        let mut output = Vec::new();

        {
            let mut printer = TtyTablePrinter::new(&mut output, 80, 14);
            printer.start(&rows).expect("tty start");
            printer.complete(&rows, 1200).expect("tty complete");
        }

        let stripped = strip_ansi_sequences(&String::from_utf8(output).expect("utf8"));
        assert!(stripped.contains("[done          ] 1 of 1 done"));
        assert!(stripped.contains("1.2s"));
        assert!(stripped.contains("----"));
    }

    #[test]
    fn tty_table_printer_separator_respects_tiny_terminal_width() {
        let rows = vec![RepoRow::finished(
            "activities".to_string(),
            "clean".to_string(),
        )];
        let mut output = Vec::new();

        {
            let mut printer = TtyTablePrinter::new(&mut output, 1, 14);
            printer.complete(&rows, 1200).expect("tty complete");
        }

        let stripped = strip_ansi_sequences(&String::from_utf8(output).expect("utf8"));
        let separator_lines: Vec<&str> = stripped
            .lines()
            .filter(|line| !line.is_empty() && line.chars().all(|c| c == '-'))
            .collect();
        assert!(!separator_lines.is_empty(), "expected a separator line");
        for line in &separator_lines {
            assert_eq!(line.len(), 1, "separator should match terminal width");
        }
    }

    #[test]
    fn tty_table_printer_falls_back_to_default_width_when_size_unknown() {
        // terminal_columns == 0 means the terminal did not report its size
        // (e.g. a pty with no winsize). The footer must render at the default
        // width instead of collapsing to a single character.
        let rows = vec![RepoRow::finished(
            "activities".to_string(),
            "clean".to_string(),
        )];
        let mut output = Vec::new();

        {
            let mut printer = TtyTablePrinter::new(&mut output, 0, 14);
            printer.complete(&rows, 1200).expect("tty complete");
        }

        let stripped = strip_ansi_sequences(&String::from_utf8(output).expect("utf8"));
        let separator_lines: Vec<&str> = stripped
            .lines()
            .filter(|line| !line.is_empty() && line.chars().all(|c| c == '-'))
            .collect();
        assert!(!separator_lines.is_empty(), "expected a separator line");
        for line in &separator_lines {
            assert_eq!(
                line.len(),
                DEFAULT_TERMINAL_COLUMNS,
                "0-width terminal should fall back to the default width"
            );
        }
        assert!(stripped.contains("[done"), "footer must not be truncated");
    }

    #[test]
    fn tty_table_printer_start_writes_only_live_progress() {
        let rows = vec![
            RepoRow::pending("activities".to_string()),
            RepoRow::pending("agentic-dev".to_string()),
        ];
        let mut output = Vec::new();

        {
            let mut printer = TtyTablePrinter::new(&mut output, 80, 14);
            printer.start(&rows).expect("tty start");
        }

        let rendered = String::from_utf8(output).expect("utf8");
        let stripped = strip_ansi_sequences(&rendered);
        assert_eq!(stripped.lines().count(), 2, "only the footer: {stripped:?}");
        assert!(stripped.contains("[activities ...] 0 of 2 done"));
        assert!(stripped.contains("2 pending"));
    }

    #[test]
    fn tty_table_printer_writes_finished_rows_as_plain_lines_in_repo_order() {
        let mut rows = vec![
            RepoRow::running("activities".to_string()),
            RepoRow::running("agentic-dev".to_string()),
        ];
        let mut output = Vec::new();

        {
            let mut printer = TtyTablePrinter::new(&mut output, 80, 14);
            printer.start(&rows).expect("tty start");
            rows[0].mark_finished("clean".to_string());
            printer.update_row(&rows, 0, 100).expect("tty update 0");
            rows[1].mark_finished("1 modified".to_string());
            printer.update_row(&rows, 1, 200).expect("tty update 1");
        }

        let stripped = strip_ansi_sequences(&String::from_utf8(output).expect("utf8"));
        let activities_pos = stripped.find("activities").expect("activities printed");
        let agentic_pos = stripped.find("agentic-dev").expect("agentic-dev printed");
        assert!(activities_pos < agentic_pos);
        assert!(stripped.contains("clean"));
        assert!(stripped.contains("1 modified"));
    }

    #[test]
    fn tty_table_printer_buffers_out_of_order_finished_rows_until_contiguous() {
        let mut rows = vec![
            RepoRow::running("activities".to_string()),
            RepoRow::running("agentic-dev".to_string()),
        ];
        let output = SharedBuffer::default();

        {
            let mut printer = TtyTablePrinter::new(output.clone(), 80, 14);
            printer.start(&rows).expect("tty start");

            rows[1].mark_finished("clean".to_string());
            let printed = printer.update_row(&rows, 1, 100).expect("tty late finish");
            assert!(printed.is_empty(), "row 1 must buffer until row 0 finishes");
            let mid = strip_ansi_sequences(&output.rendered());
            assert!(
                !mid.contains("agentic-dev  clean"),
                "agentic-dev row not yet flushed, got: {mid:?}"
            );

            rows[0].mark_finished("clean".to_string());
            let printed = printer
                .update_row(&rows, 0, 200)
                .expect("tty contiguous finish");
            assert_eq!(printed, vec![0, 1]);
        }

        let stripped = strip_ansi_sequences(&output.rendered());
        let activities_pos = stripped.find("activities").expect("activities printed");
        let agentic_pos = stripped.find("agentic-dev").expect("agentic-dev printed");
        assert!(activities_pos < agentic_pos);
    }

    #[test]
    fn tty_table_printer_redraws_footer_in_place_via_moveup_and_clear() {
        let mut rows = vec![RepoRow::running("activities".to_string())];
        let mut output = Vec::new();

        {
            let mut printer = TtyTablePrinter::new(&mut output, 80, 14);
            printer.start(&rows).expect("tty start");
            rows[0].mark_finished("clean".to_string());
            printer.update_row(&rows, 0, 100).expect("tty update");
        }

        let rendered = String::from_utf8(output).expect("utf8");
        assert!(
            rendered.contains("\x1b[2A"),
            "expected MoveUp(2) escape; got: {rendered:?}"
        );
        assert!(
            rendered.contains("\x1b[J") || rendered.contains("\x1b[0J"),
            "expected Clear(FromCursorDown) escape; got: {rendered:?}"
        );
    }

    #[test]
    fn tty_table_printer_does_not_truncate_finished_row_content() {
        let long_status = "2410 modified, 473 deleted, 47 untracked";
        let mut rows = vec![RepoRow::running("iOS-Doximity".to_string())];
        let mut output = Vec::new();

        {
            let mut printer = TtyTablePrinter::new(&mut output, 40, 14);
            printer.start(&rows).expect("tty start");
            rows[0].mark_finished(long_status.to_string());
            printer.update_row(&rows, 0, 100).expect("tty update");
        }

        let stripped = strip_ansi_sequences(&String::from_utf8(output).expect("utf8"));
        assert!(
            stripped.contains(long_status),
            "long status should pass through unchanged; got: {stripped:?}"
        );
    }

    #[test]
    fn tty_table_printer_truncates_footer_to_terminal_width() {
        let rows = vec![RepoRow::pending("activities".to_string())];
        let mut output = Vec::new();

        {
            let mut printer = TtyTablePrinter::new(&mut output, 30, 14);
            printer.start(&rows).expect("tty start");
        }

        let stripped = strip_ansi_sequences(&String::from_utf8(output).expect("utf8"));
        for line in stripped.lines() {
            assert!(line.len() <= 30, "{line:?} wider than terminal");
        }
    }

    #[test]
    fn tty_table_printer_complete_flushes_remaining_rows_and_renders_final_footer() {
        let mut rows = vec![
            RepoRow::running("activities".to_string()),
            RepoRow::running("agentic-dev".to_string()),
        ];
        let mut output = Vec::new();

        {
            let mut printer = TtyTablePrinter::new(&mut output, 80, 14);
            printer.start(&rows).expect("tty start");
            rows[0].mark_finished("clean".to_string());
            rows[1].mark_finished("clean".to_string());
            let printed = printer.complete(&rows, 1500).expect("tty complete");
            assert_eq!(printed, vec![0, 1]);
        }

        let stripped = strip_ansi_sequences(&String::from_utf8(output).expect("utf8"));
        assert!(stripped.contains("activities"));
        assert!(stripped.contains("agentic-dev"));
        assert!(stripped.contains("2 of 2 done"));
        assert!(stripped.contains("1.5s"));
    }

    #[test]
    fn tty_tick_advances_blocking_repo_runtime_without_flushing_later_results() {
        let output = SharedBuffer::default();
        let mut rows = vec![
            RepoRow::pending("repo-053".to_string()),
            RepoRow::finished("repo-054".to_string(), "clean".to_string()),
        ];
        rows[0].mark_running(2000);
        let mut printer = TtyTablePrinter::new(output.clone(), 60, 8);

        printer.start(&rows).expect("tty start");
        printer.tick(&rows, 17_000).expect("first quiet tick");
        let rendered = strip_ansi_sequences(&output.rendered());
        assert!(
            rendered.contains("--------\n[repo-053] 1 of 2 done | 1 running | 0 pending | 17.0s\n")
        );
        assert!(!rendered.contains("[repo-054"));

        printer.tick(&rows, 32_000).expect("second quiet tick");
        let rendered = strip_ansi_sequences(&output.rendered());
        assert!(rendered.contains("[repo-053] 1 of 2 done | 1 running | 0 pending | 32.0s"));
        assert!(!rendered.contains("[repo-054"));

        rows[0].mark_finished("clean".to_string());
        assert_eq!(printer.update_row(&rows, 0, 32_100).unwrap(), vec![0, 1]);
        let rendered = strip_ansi_sequences(&output.rendered());
        assert!(rendered.ends_with("[done    ] 2 of 2 done | 0 running | 0 pending | 32.1s\n"));
        assert_eq!(rendered.matches("[repo-053] clean").count(), 1);
        assert_eq!(rendered.matches("[repo-054] clean").count(), 1);
    }

    #[test]
    fn tty_blocking_repo_runtime_remains_visible_with_long_name_at_60_columns() {
        let mut rows = vec![RepoRow::pending(
            "a-repository-with-an-extremely-long-name-that-is-still-running".to_string(),
        )];
        rows[0].mark_running(500);
        let output = SharedBuffer::default();
        let mut printer = TtyTablePrinter::new(output.clone(), 60, 48);

        printer.tick(&rows, 30_500).expect("quiet tick");

        let rendered = strip_ansi_sequences(&output.rendered());
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines[0], "-".repeat(60));
        assert!(
            lines[1].starts_with("[a-repository-with-an-extremely-long-name-tha-...] 0 of 1"),
            "{}",
            lines[1]
        );
        assert!(lines[1].len() <= 60, "{}", lines[1]);
    }

    #[test]
    fn in_progress_label_fits_names_and_marks_overflow() {
        assert_eq!(in_progress_label(&["foo"], 20), "foo in-progress");
        assert_eq!(in_progress_label(&["foo"], 10), "foo");
        assert_eq!(in_progress_label(&["a", "b", "c"], 20), "a, b, c");
        assert_eq!(
            in_progress_label(&["cadburyeggs", "foo-b", "foo-c", "foo-d"], 29),
            "cadburyeggs, foo-b, foo-c ..."
        );
        assert_eq!(in_progress_label(&["abcdefghij", "b"], 8), "abcd ...");
        assert_eq!(in_progress_label(&["abcdefghij", "b"], 8).len(), 8);
        assert_eq!(in_progress_label(&["abcdefghij", "b"], 3), " ..");
        assert_eq!(in_progress_label(&[], 8), "");
    }

    #[test]
    fn tty_footer_lists_running_repos_in_row_order() {
        let mut rows = vec![
            RepoRow::pending("alpha".to_string()),
            RepoRow::pending("beta".to_string()),
            RepoRow::pending("gamma".to_string()),
        ];
        rows[2].mark_running(0);
        rows[1].mark_running(500);
        rows[0].mark_running(900);
        let output = SharedBuffer::default();
        let mut printer = TtyTablePrinter::new(output.clone(), 80, 30);

        printer.tick(&rows, 1_000).expect("tick");

        let rendered = strip_ansi_sequences(&output.rendered());
        let expected = format!("{} 0 of 3 done", format_repo_name("alpha, beta, gamma", 30));
        assert!(rendered.contains(&expected), "{rendered}");
    }

    #[test]
    fn plain_tick_does_not_produce_progress_output() {
        let mut output = Vec::new();
        let rows = vec![RepoRow::running("repo-053".to_string())];
        let mut printer = PlainPrinter::new(&mut output, 8);

        printer.start(&rows).expect("plain start");
        printer.tick(&rows, 30_000).expect("plain tick");

        assert!(output.is_empty());
    }
}
