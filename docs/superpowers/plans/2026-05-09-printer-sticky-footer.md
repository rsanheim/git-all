# TTY Sticky-Footer Printer Rewrite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the viewport-paging `TtyTablePrinter` with a sticky-footer design so all completed repos accumulate in scrollback (matching `cargo build` / `npm install` / `indicatif::MultiProgress` UX).

**Architecture:** Finished rows are written as plain `writeln!` lines to stdout — they scroll naturally and live in scrollback. A small fixed-height footer (separator + summary, exactly 2 lines) sits below the latest finished row and is redrawn in place via `MoveUp(2) → Clear(FromCursorDown) → reprint` whenever state changes. Out-of-order finished rows are buffered until contiguous, matching `PlainPrinter`'s existing behavior. No viewport, no page-stepping, no terminal-height tracking.

**Tech Stack:** Rust 2024 edition, `crossterm` for cursor/clear sequences, existing `Printer` trait in `rust/src/printer.rs`. Branch: `crossterm-v3`.

**Why this design:** The current `TtyTablePrinter` redraws a 27-line viewport in place via `MoveUp(N)`, which means rows 1–74 of a 100-repo run are overwritten by `MoveUp` and never reach scrollback. The sticky-footer pattern (used by every mainstream "live updater" CLI) preserves all rows in scrollback for free, keeps the user's invoking command line visible, gives the summary the full terminal width, and is structurally simpler.

---

## File Structure

| File | Change |
|------|--------|
| `rust/src/printer.rs` | Rewrite `TtyTablePrinter`; simplify `FooterState`; delete `Viewport` struct + viewport tests + obsolete TTY tests; add new sticky-footer tests |
| `rust/src/runner.rs` | Drop `terminal_rows` from `terminal_size` lookup; drop `DEFAULT_TERMINAL_ROWS` const; update `TtyTablePrinter::new` call site (4-arg → 3-arg) |

The `Printer` trait, `RepoRow`, `RowState`, `format_repo_name`, `display_repo_name`, and `PlainPrinter` are unchanged. The `Printer` trait's `update_row(&mut self, rows, row_index, elapsed_ms) -> Vec<usize>` contract is unchanged — only the `TtyTablePrinter` implementation changes.

---

## Task 1: Pin new sticky-footer behavior with failing tests

**Files:**
- Modify: `rust/src/printer.rs` (append at end of `mod tests`)

These tests use the post-rewrite API (`TtyTablePrinter::new(writer, columns, repo_width)` — 3 args, no `terminal_rows`). They will fail to compile against the current `TtyTablePrinter`, which is what we want — they pin the spec.

- [ ] **Step 1: Add new tests at the end of `mod tests`**

Append these tests immediately before the closing `}` of `mod tests` in `rust/src/printer.rs`:

```rust
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
            assert!(
                printed.is_empty(),
                "row 1 must buffer until row 0 finishes"
            );
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
```

- [ ] **Step 2: Confirm the new tests fail to compile**

Run: `cd rust && cargo test --no-run 2>&1 | head -40`

Expected: compile errors mentioning `TtyTablePrinter::new` (wrong number of arguments — old API takes 4, new tests pass 3). This proves the tests are pinned against the new API.

- [ ] **Step 3: Do not commit yet**

Hold the commit until Task 2 lands a working impl. (Committing a non-compiling test would force `--no-verify` later, which is forbidden.)

---

## Task 2: Rewrite TtyTablePrinter with sticky-footer impl

**Files:**
- Modify: `rust/src/printer.rs:212-358` (the entire `TtyTablePrinter` block plus its `Printer` impl)

This task replaces the viewport-paging implementation with the sticky-footer design.

- [ ] **Step 1: Replace the `TtyTablePrinter` struct and `impl TtyTablePrinter`**

Find the current block starting at `pub struct TtyTablePrinter<W: Write> {` (around line 212) and ending at the end of `impl<W: Write> TtyTablePrinter<W> { ... }` (around line 333, before `impl<W: Write> Printer for TtyTablePrinter<W>`).

Replace it with:

```rust
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
        self.terminal_columns.max(1)
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
        let separator = "-".repeat(self.terminal_width());
        let summary = format!(
            "{:<width$}  {}",
            "SUMMARY",
            footer.render_message(),
            width = self.repo_width
        );
        writeln!(self.writer, "{}", separator)?;
        writeln!(self.writer, "{}", self.fit_line(&summary))?;
        self.writer.flush()?;
        self.footer_active = true;
        Ok(())
    }
}
```

- [ ] **Step 2: Replace the `Printer` impl for `TtyTablePrinter`**

Find the current `impl<W: Write> Printer for TtyTablePrinter<W> { ... }` block (around line 335) and replace it with:

```rust
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
}
```

- [ ] **Step 3: Run cargo build**

Run: `cd rust && cargo build 2>&1 | tail -30`

Expected: build succeeds. If you see errors about `Viewport` being undefined inside `TtyTablePrinter`, you missed deleting the old `render_frame` method — re-check that the entire old `impl<W: Write> TtyTablePrinter<W>` block was replaced.

If you see errors about `FooterState` field mismatches (`visible_start`, `visible_end` not found), that's expected — those fields will be removed in Task 3. For now, leave `FooterState` as-is and add temporary placeholder values inside `render_footer`:

```rust
let footer = FooterState {
    visible_start: 0,
    visible_end: rows.len(),
    total_rows: rows.len(),
    complete,
    running,
    pending,
    elapsed_ms,
};
```

This keeps the build green so we can address `FooterState` cleanly in Task 3.

- [ ] **Step 4: Do not commit yet**

The test module still has obsolete tests using the 4-arg constructor — they will block compilation of `cargo test`. Task 4 deletes them.

---

## Task 3: Simplify FooterState message format

**Files:**
- Modify: `rust/src/printer.rs:120-143` (the `FooterState` struct and `render_message`)
- Modify: `rust/src/printer.rs` (the `footer_includes_slice_counts_elapsed_and_pending` test)

The viewport's `(visible_start-visible_end of total)` no longer makes sense — there is no viewport. Replace with a clean cargo-style message.

- [ ] **Step 1: Replace `FooterState` struct definition**

Find:

```rust
pub struct FooterState {
    pub visible_start: usize,
    pub visible_end: usize,
    pub total_rows: usize,
    pub complete: usize,
    pub running: usize,
    pub pending: usize,
    pub elapsed_ms: u128,
}
```

Replace with:

```rust
pub struct FooterState {
    pub total_rows: usize,
    pub complete: usize,
    pub running: usize,
    pub pending: usize,
    pub elapsed_ms: u128,
}
```

- [ ] **Step 2: Replace `render_message`**

Find:

```rust
impl FooterState {
    pub fn render_message(&self) -> String {
        format!(
            "({}-{} of {}) | {} complete | {} running | {} pending | {:.1}s elapsed",
            self.visible_start,
            self.visible_end,
            self.total_rows,
            self.complete,
            self.running,
            self.pending,
            self.elapsed_ms as f64 / 1000.0,
        )
    }
}
```

Replace with:

```rust
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
```

- [ ] **Step 3: Remove the placeholder fields from `render_footer`**

In `TtyTablePrinter::render_footer` (added in Task 2), remove the temporary `visible_start: 0, visible_end: rows.len(),` lines so the construction reads:

```rust
let footer = FooterState {
    total_rows: rows.len(),
    complete,
    running,
    pending,
    elapsed_ms,
};
```

- [ ] **Step 4: Update the FooterState test**

Find the test:

```rust
#[test]
fn footer_includes_slice_counts_elapsed_and_pending() {
    let footer = FooterState {
        visible_start: 24,
        visible_end: 47,
        total_rows: 98,
        complete: 41,
        running: 8,
        pending: 49,
        elapsed_ms: 2100,
    };

    assert_eq!(
        footer.render_message(),
        "(24-47 of 98) | 41 complete | 8 running | 49 pending | 2.1s elapsed"
    );
}
```

Replace with:

```rust
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
```

- [ ] **Step 5: Run cargo build**

Run: `cd rust && cargo build 2>&1 | tail -20`

Expected: build succeeds. `cargo test` will still fail compilation in the test module due to leftover obsolete tests (Task 4 cleans those up).

---

## Task 4: Remove obsolete viewport struct, viewport tests, and obsolete TTY tests

**Files:**
- Modify: `rust/src/printer.rs` — delete `Viewport` struct (lines 78–118 in the pre-task file) and seven obsolete tests

The new design has no viewport, so all `Viewport`-related code goes away. Also remove TTY tests whose premises no longer hold.

- [ ] **Step 1: Delete the `Viewport` struct and its impl**

Delete this entire block (around lines 78–118 of `printer.rs` before this task):

```rust
pub struct Viewport {
    pub start: usize,
    pub end: usize,
}

impl Viewport {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn for_rows(rows: &[RepoRow], height: usize) -> Self {
        // ...
    }

    pub fn with_page_steps(
        rows: &[RepoRow],
        height: usize,
        _previous_start: usize,
        desired_overlap: usize,
    ) -> Self {
        // ...
    }
}
```

- [ ] **Step 2: Delete obsolete viewport tests**

Find and delete these four tests by name:

* `viewport_follows_first_unfinished_repo`
* `viewport_page_step_keeps_first_page_until_anchor_reaches_next_page`
* `viewport_page_step_advances_by_page_minus_overlap`
* `viewport_page_step_shows_final_page_when_all_rows_finish`

- [ ] **Step 3: Delete obsolete TtyTablePrinter tests**

Find and delete these tests by name. Their premises — paged viewport, prompt-row reservation, in-place updates of finished rows, line-truncation of row content — no longer apply:

* `tty_table_printer_renders_pending_rows_without_headers`
* `tty_table_printer_leaves_a_row_for_the_shell_prompt`
* `tty_table_printer_keeps_printed_lines_within_terminal_width`
* `tty_table_printer_updates_finished_rows_without_waiting_for_earlier_rows`
* `tty_table_printer_keeps_completed_rows_in_place`

- [ ] **Step 4: Replace `tty_table_printer_renders_summary_row_with_separator`**

Find this test:

```rust
#[test]
fn tty_table_printer_renders_summary_row_with_separator() {
    let rows = vec![RepoRow::finished(
        "activities".to_string(),
        "clean".to_string(),
    )];
    let mut output = Vec::new();

    {
        let mut printer = TtyTablePrinter::new(&mut output, 6, 80, 14);
        printer.start(&rows).expect("tty start");
        printer.complete(&rows, 1200).expect("tty complete");
    }

    let rendered = String::from_utf8(output).expect("utf8");
    assert!(rendered.contains("SUMMARY"));
    assert!(rendered.contains("elapsed"));
    assert!(rendered.contains("----"));
}
```

Replace with:

```rust
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
```

- [ ] **Step 5: Replace `tty_table_printer_separator_respects_tiny_terminal_width`**

This test's old premise was "every output line ≤ terminal width." That no longer holds because finished rows pass through unmodified — `[activities  ] clean` is 21 chars, far wider than a 1-col terminal. The test's *real* intent was always "the separator respects terminal width," so narrow it to that.

Find this test:

```rust
#[test]
fn tty_table_printer_separator_respects_tiny_terminal_width() {
    let rows = vec![RepoRow::finished(
        "activities".to_string(),
        "clean".to_string(),
    )];
    let mut output = Vec::new();

    {
        let mut printer = TtyTablePrinter::new(&mut output, 6, 1, 14);
        printer.complete(&rows, 1200).expect("tty complete");
    }

    let rendered = String::from_utf8(output).expect("utf8");
    let stripped = strip_ansi_sequences(&rendered);
    assert!(stripped.lines().any(|line| line == "-"));
    for line in stripped.lines() {
        assert!(line.len() <= 1, "{line:?} was wider than terminal");
    }
}
```

Replace with:

```rust
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
```

- [ ] **Step 6: Run the full test suite**

Run: `cd rust && cargo test 2>&1 | tail -30`

Expected: all 30+ tests pass (count drops from current 39 because we deleted 9 obsolete tests and added 7 new ones, net −2). If anything fails, read the failure carefully — the impl from Task 2 may need a fix, not the test.

- [ ] **Step 6: Commit Tasks 1–4 as a single rewrite commit**

Run:

```bash
git add rust/src/printer.rs
git commit -m "$(cat <<'EOF'
rewrite tty printer as sticky footer

Finished rows now write to stdout as plain lines, accumulating in
scrollback in repo order. A 2-line footer (separator + summary) sits
below the latest finished row and is redrawn in place via MoveUp(2) +
Clear(FromCursorDown) on each update.

Replaces the previous viewport+page-step design that redrew a 27-line
table in place — that approach overwrote rows 1-74 of a 100-repo run
via MoveUp, leaving only the last page visible after exit and pulling
the user's invoking command line into scrollback on the first frame.
EOF
)"
```

---

## Task 5: Update runner.rs to drop `terminal_rows`

**Files:**
- Modify: `rust/src/runner.rs:50-51` (drop `DEFAULT_TERMINAL_ROWS`)
- Modify: `rust/src/runner.rs:264-283` (terminal size lookup + printer construction)

- [ ] **Step 1: Drop the `DEFAULT_TERMINAL_ROWS` constant**

Find:

```rust
const DEFAULT_TERMINAL_COLUMNS: usize = 80;
const DEFAULT_TERMINAL_ROWS: usize = 24;
```

Replace with:

```rust
const DEFAULT_TERMINAL_COLUMNS: usize = 80;
```

- [ ] **Step 2: Simplify the terminal size lookup**

Find:

```rust
    let stdout = std::io::stdout();
    let is_tty = stdout.is_tty();
    let (terminal_columns, terminal_rows) = if is_tty {
        terminal_size()
            .map(|(columns, rows)| (columns as usize, rows as usize))
            .unwrap_or((DEFAULT_TERMINAL_COLUMNS, DEFAULT_TERMINAL_ROWS))
    } else {
        (0, 0)
    };
    let stdout = stdout.lock();
    let mut printer: Box<dyn Printer + '_> = if is_tty {
        Box::new(TtyTablePrinter::new(
            stdout,
            terminal_rows,
            terminal_columns,
            name_width,
        ))
    } else {
        Box::new(PlainPrinter::new(stdout, name_width))
    };
```

Replace with:

```rust
    let stdout = std::io::stdout();
    let is_tty = stdout.is_tty();
    let terminal_columns = if is_tty {
        terminal_size()
            .map(|(columns, _rows)| columns as usize)
            .unwrap_or(DEFAULT_TERMINAL_COLUMNS)
    } else {
        0
    };
    let stdout = stdout.lock();
    let mut printer: Box<dyn Printer + '_> = if is_tty {
        Box::new(TtyTablePrinter::new(stdout, terminal_columns, name_width))
    } else {
        Box::new(PlainPrinter::new(stdout, name_width))
    };
```

- [ ] **Step 3: Run cargo build and cargo test**

Run: `cd rust && cargo build 2>&1 | tail -10 && cargo test 2>&1 | tail -10`

Expected: both succeed; all tests pass.

- [ ] **Step 4: Commit**

Run:

```bash
git add rust/src/runner.rs
git commit -m "$(cat <<'EOF'
drop terminal_rows from runner — sticky-footer printer ignores height

The new TtyTablePrinter writes finished rows as plain stdout lines and
only needs terminal width for footer truncation. Drop the rows side of
the terminal_size lookup and DEFAULT_TERMINAL_ROWS const.
EOF
)"
```

---

## Task 6: Manual verification with PTY replay

**Files:** none modified. This task verifies the rewrite against a real run.

The Python harness used during the design review is the verification tool. We re-run the same scenarios and confirm:
1. All 100+ repos appear in scrollback in repo order.
2. The footer is exactly 2 lines wide × `terminal_columns`.
3. The user's invoking command line is preserved in the visible area at the start of output.
4. No `MoveUp(N)` for N > 2 appears in the byte stream.

- [ ] **Step 1: Build and install**

Run: `cd /Users/rsanheim/src/rsanheim/git-all && script/install -t rust`

Expected: build succeeds; binary at `~/.local/bin/git-all`.

- [ ] **Step 2: Capture a run at 30×100**

The PTY harness `/tmp/run_pty.py` and replay simulator `/tmp/replay.py` from earlier sessions are still present. If `/tmp/run_pty.py` is missing, recreate it from this content:

```python
#!/usr/bin/env python3
"""Spawn a command under a PTY at a chosen size; write output bytes to a file."""
import fcntl
import os
import pty
import select
import struct
import sys
import termios

if len(sys.argv) < 5:
    print("usage: run_pty.py <rows> <cols> <out> <cmd...>", file=sys.stderr)
    sys.exit(1)

rows = int(sys.argv[1])
cols = int(sys.argv[2])
out_path = sys.argv[3]
argv = sys.argv[4:]

pid, fd = pty.fork()
if pid == 0:
    winsize = struct.pack("HHHH", rows, cols, 0, 0)
    fcntl.ioctl(0, termios.TIOCSWINSZ, winsize)
    os.execvp(argv[0], argv)

winsize = struct.pack("HHHH", rows, cols, 0, 0)
try:
    fcntl.ioctl(fd, termios.TIOCSWINSZ, winsize)
except OSError:
    pass

with open(out_path, "wb") as out:
    while True:
        try:
            r, _, _ = select.select([fd], [], [], 30)
        except OSError:
            break
        if not r:
            break
        try:
            data = os.read(fd, 65536)
        except OSError:
            break
        if not data:
            break
        out.write(data)

os.waitpid(pid, 0)
```

Run:

```bash
cd ~/work && python3 /tmp/run_pty.py 30 100 /tmp/git-all-sticky-30x100.txt /Users/rsanheim/.local/bin/git-all status
```

Expected: completes in a few seconds; capture file is non-empty.

- [ ] **Step 3: Verify all repos are present in the captured byte stream in order**

```bash
python3 - <<'PY'
import re
with open('/tmp/git-all-sticky-30x100.txt','rb') as f:
    raw = f.read().decode('utf-8','replace')
stripped = re.sub(r'\x1b\[[0-9;]*[A-Za-z]', '', raw)
# repo names appear at start of plain lines, in brackets like "[activities  ]"
names = re.findall(r'\[([a-zA-Z][^\]]{0,40})\s*\]', stripped)
print(f"distinct names found: {len(set(names))}")
print(f"first 5: {names[:5]}")
print(f"last 5:  {names[-5:]}")
PY
```

Expected: ~100 distinct names. First names alphabetically early (`activities`, `agentic-dev`, etc.), last names alphabetically late.

- [ ] **Step 4: Verify the cursor-movement budget**

```bash
python3 - <<'PY'
import re
with open('/tmp/git-all-sticky-30x100.txt','rb') as f:
    raw = f.read().decode('utf-8','replace')
moveups = [int(m) for m in re.findall(r'\x1b\[(\d+)A', raw)]
print(f"MoveUp values seen: {sorted(set(moveups))}")
print(f"total MoveUp ops:   {len(moveups)}")
assert all(n == 2 for n in moveups), f"unexpected MoveUp value: {set(moveups)}"
print("OK — all cursor moves are MoveUp(2)")
PY
```

Expected: `MoveUp values seen: [2]` and `OK — all cursor moves are MoveUp(2)`. Any value other than 2 means the printer is moving over more than the footer.

- [ ] **Step 5: Replay through simulator and inspect final visible state**

Run: `python3 /tmp/replay.py /tmp/git-all-sticky-30x100.txt 30 100 25 | tail -20`

Expected: final visible screen shows the last few finished repo rows + separator + summary on the last two visible rows. Scrollback line count should be ~95+ (most of the 100 repos went into scrollback).

- [ ] **Step 6: Capture and verify a narrow-terminal run (24×60)**

```bash
cd ~/work && python3 /tmp/run_pty.py 24 60 /tmp/git-all-sticky-24x60.txt /Users/rsanheim/.local/bin/git-all status
python3 /tmp/replay.py /tmp/git-all-sticky-24x60.txt 24 60 18 | tail -20
```

Expected: visible state has the latest few rows + 60-char separator + summary truncated to ≤60 chars on the summary line. Crucially, the summary should now lead with `N of 101 done | ...` so the most useful info isn't truncated.

- [ ] **Step 7: Verify scroll-back contains all repos in order (real terminal sanity check)**

The user runs this themselves in a real interactive terminal at any size:

```bash
cd ~/work && git-all status | wc -l
cd ~/work && git-all status | head -5
cd ~/work && git-all status | tail -5
```

Note: piping makes `is_tty()` false, so this exercises `PlainPrinter`, not `TtyTablePrinter`. To exercise the TTY printer's scrollback in a real terminal, the user must run interactively and scroll up.

The user can manually verify by running `git-all status` in their actual terminal, then scrolling up to confirm all 100 repos are visible in scrollback above the final summary.

---

## Task 7: Self-review and polish (optional version bump)

- [ ] **Step 1: Re-read the diff one last time**

Run: `git diff main...HEAD -- rust/src/printer.rs rust/src/runner.rs | head -200`

Sanity check: `Viewport` is gone; no references to `terminal_rows` in printer.rs; new `TtyTablePrinter` struct has 5 fields (`writer`, `terminal_columns`, `repo_width`, `next_to_print`, `footer_active`); `FooterState` has 5 fields (no `visible_start`/`visible_end`).

- [ ] **Step 2: (Optional) Bump rc version**

If you want a clean version cut for this milestone:

```bash
# Edit each of these from 0.7.1-rc.3 → 0.7.1-rc.4:
#   rust/Cargo.toml
#   README.md
#   docs/index.md
# Then regenerate Cargo.lock:
cd rust && cargo build
git add rust/Cargo.toml rust/Cargo.lock README.md docs/index.md
git commit -m "bump to rc4"
```

- [ ] **Step 3: Final cargo test run**

Run: `cd rust && cargo test 2>&1 | tail -10`

Expected: all tests pass.

---

## Summary of files touched

| File | Lines added | Lines removed | Net |
|------|-------------|---------------|-----|
| `rust/src/printer.rs` | ~200 (new tests + new impl) | ~250 (Viewport, page-step impl, obsolete tests) | ~−50 |
| `rust/src/runner.rs` | ~5 | ~10 | ~−5 |

The rewrite ends up smaller than what it replaces.
