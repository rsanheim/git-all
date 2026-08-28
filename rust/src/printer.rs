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
}

impl RepoRow {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn pending(repo: String) -> Self {
        Self {
            repo,
            output: "pending".to_string(),
            state: RowState::Pending,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn running(repo: String) -> Self {
        Self {
            repo,
            output: "running".to_string(),
            state: RowState::Running,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn finished(repo: String, output: String) -> Self {
        Self {
            repo,
            output,
            state: RowState::Finished,
        }
    }

    pub fn mark_running(&mut self) {
        self.output = "running".to_string();
        self.state = RowState::Running;
    }

    pub fn mark_finished(&mut self, output: String) {
        self.output = output;
        self.state = RowState::Finished;
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

    /// Redraw live progress without any row-state change (e.g. a clock tick).
    /// No-op for printers with no live region.
    fn tick(&mut self, rows: &[RepoRow], elapsed_ms: u128) -> io::Result<()> {
        let _ = (rows, elapsed_ms);
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
    /// Lines the live footer currently occupies (0 when not drawn). The footer
    /// grows a line while repos are in flight, so its height varies.
    footer_height: u16,
    /// One-time scope line printed above the footer on `start`.
    header: Option<String>,
}

impl<W: Write> TtyTablePrinter<W> {
    /// Max in-flight repo names to list in the footer before collapsing the
    /// remainder into a "+N more" count.
    const MAX_RUNNING_NAMES: usize = 6;

    pub fn new(writer: W, terminal_columns: usize, repo_width: usize) -> Self {
        Self {
            writer,
            terminal_columns,
            repo_width,
            next_to_print: 0,
            footer_height: 0,
            header: None,
        }
    }

    /// Set the one-time scope line shown above the footer.
    pub fn with_header(mut self, header: Option<String>) -> Self {
        self.header = header;
        self
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
        if self.footer_height > 0 {
            queue!(
                self.writer,
                MoveToColumn(0),
                MoveUp(self.footer_height),
                Clear(ClearType::FromCursorDown)
            )?;
            self.footer_height = 0;
        }
        Ok(())
    }

    fn render_footer(&mut self, rows: &[RepoRow], elapsed_ms: u128) -> io::Result<()> {
        let mut complete = 0usize;
        let mut running = 0usize;
        let mut pending = 0usize;
        let mut running_names: Vec<&str> = Vec::new();
        for row in rows {
            match row.state {
                RowState::Finished => complete += 1,
                RowState::Running => {
                    running += 1;
                    if running_names.len() < Self::MAX_RUNNING_NAMES {
                        running_names.push(&row.repo);
                    }
                }
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
        let separator = "-".repeat(self.terminal_width());
        let summary = format!(
            "{:<width$}  {}",
            "SUMMARY",
            footer.render_message(),
            width = self.repo_width
        );

        let mut height: u16 = 0;
        writeln!(self.writer, "{}", separator)?;
        height += 1;
        writeln!(self.writer, "{}", self.fit_line(&summary))?;
        height += 1;
        if running > 0 {
            let mut line = format!("running: {}", running_names.join(", "));
            if running > running_names.len() {
                line.push_str(&format!(", +{} more", running - running_names.len()));
            }
            writeln!(self.writer, "{}", self.fit_line(&line))?;
            height += 1;
        }
        self.writer.flush()?;
        self.footer_height = height;
        Ok(())
    }
}

impl<W: Write> Printer for TtyTablePrinter<W> {
    fn start(&mut self, rows: &[RepoRow]) -> io::Result<()> {
        if let Some(header) = self.header.clone() {
            let line = self.fit_line(&header);
            writeln!(self.writer, "{}", line)?;
        }
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
        assert!(stripped.contains("SUMMARY"));
        assert!(stripped.contains("1 of 1 done"));
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
        assert!(stripped.contains("SUMMARY"), "footer must not be truncated");
    }

    #[test]
    fn tty_table_printer_start_writes_only_the_footer() {
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
        assert!(
            !stripped.contains("activities"),
            "no rows should be printed yet, got: {stripped:?}"
        );
        assert!(stripped.contains("SUMMARY"));
        assert!(stripped.contains("0 of 2 done"));
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
        // The start footer is 3 lines (separator + summary + running list) while
        // "activities" is in flight, so the in-place clear moves up by 3.
        assert!(
            rendered.contains("\x1b[3A"),
            "expected MoveUp(3) escape; got: {rendered:?}"
        );
        assert!(
            rendered.contains("\x1b[J") || rendered.contains("\x1b[0J"),
            "expected Clear(FromCursorDown) escape; got: {rendered:?}"
        );
    }

    #[test]
    fn tty_table_printer_prints_header_once_above_the_footer() {
        let rows = vec![RepoRow::pending("activities".to_string())];
        let mut output = Vec::new();

        {
            let mut printer = TtyTablePrinter::new(&mut output, 80, 14).with_header(Some(
                "git-all fetch · 1 repo · ~/work · 8 workers".to_string(),
            ));
            printer.start(&rows).expect("tty start");
        }

        let stripped = strip_ansi_sequences(&String::from_utf8(output).expect("utf8"));
        assert!(stripped.contains("git-all fetch · 1 repo · ~/work · 8 workers"));
        assert!(stripped.contains("SUMMARY"));
        // Header sits above the footer.
        let header_pos = stripped.find("git-all fetch").expect("header printed");
        let summary_pos = stripped.find("SUMMARY").expect("summary printed");
        assert!(header_pos < summary_pos);
    }

    #[test]
    fn tty_table_printer_footer_lists_in_flight_repos() {
        let rows = vec![
            RepoRow::running("activities".to_string()),
            RepoRow::pending("agentic-dev".to_string()),
        ];
        let mut output = Vec::new();

        {
            let mut printer = TtyTablePrinter::new(&mut output, 80, 14);
            printer.start(&rows).expect("tty start");
        }

        let stripped = strip_ansi_sequences(&String::from_utf8(output).expect("utf8"));
        assert!(stripped.contains("running: activities"));
        assert!(stripped.contains("0 of 2 done | 1 running | 1 pending"));
    }

    #[test]
    fn tty_table_printer_running_line_caps_names_and_counts_the_rest() {
        let rows: Vec<RepoRow> = (0..10)
            .map(|i| RepoRow::running(format!("repo-{i:02}")))
            .collect();
        let mut output = Vec::new();

        {
            // Wide terminal so the running line is not truncated.
            let mut printer = TtyTablePrinter::new(&mut output, 200, 14);
            printer.start(&rows).expect("tty start");
        }

        let stripped = strip_ansi_sequences(&String::from_utf8(output).expect("utf8"));
        // Six names shown, remaining four summarized.
        assert!(stripped.contains("repo-05"));
        assert!(!stripped.contains("repo-06"));
        assert!(stripped.contains("+4 more"));
    }

    #[test]
    fn tty_table_printer_tick_redraws_footer_with_updated_elapsed() {
        let rows = vec![RepoRow::running("activities".to_string())];
        let mut output = Vec::new();

        {
            let mut printer = TtyTablePrinter::new(&mut output, 80, 14);
            printer.start(&rows).expect("tty start");
            printer.tick(&rows, 2500).expect("tty tick");
        }

        let rendered = String::from_utf8(output).expect("utf8");
        let stripped = strip_ansi_sequences(&rendered);
        assert!(
            stripped.contains("2.5s"),
            "tick should show new elapsed time"
        );
        // A tick clears the previous footer in place before redrawing it.
        assert!(
            rendered.contains("\x1b[3A"),
            "tick should move up over the footer"
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
}
