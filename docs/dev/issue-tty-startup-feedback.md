# Issue: TTY gives almost no feedback until repos start finishing

## Status

Draft / investigation. No code change proposed yet — this doc frames the problem
and lays out options for discussion.

## Summary

After consolidating the TTY UX onto the sticky-footer printer (shipped 0.7.3+),
`git-all` shows essentially nothing but a frozen one-line summary until repos
begin to complete. On a directory with many repos and network-bound work — e.g.
`git all fetch` in `~/src/oss` or `~/work` — the tool looks hung for the first
several seconds.

An earlier approach printed the *full list of repos first* and then updated each
row in place. That gave better perceived feedback but was dropped during
consolidation because of rendering complexity. This doc explains why the current
design behaves the way it does, what the older approach traded away, and the
options for getting the feedback back.

## Current behavior

The TTY path is [`TtyTablePrinter`](../../rust/src/printer.rs) driven by
[`run_parallel`](../../rust/src/runner.rs). On a run:

1. Repos are discovered and sorted alphabetically; each becomes a
   `RepoRow::pending`.
2. `printer.start()` renders **only** a two-line footer:

   ```text
   --------------------------------------------------------------------------------
   SUMMARY         0 of 98 done | 0 running | 98 pending | 0.0s
   ```

   No repo list is printed. This is pinned by the test
   `tty_table_printer_start_writes_only_the_footer`.
3. Worker threads acquire the semaphore, then send `Started`. On a TTY the row is
   marked running and the footer is redrawn, so it flips to
   `0 of 98 done | 16 running | 82 pending`. (`fetch`/`pull` default to 16
   workers, `status` to 8 — see `default_workers` in `rust/src/lib.rs`.)
4. Finished rows are flushed to scrollback **only in contiguous alphabetical
   order** (`flush_finished_rows` advances `next_to_print` only while the *next*
   row is `Finished`). The footer's "done" count climbs as repos finish, but the
   actual repo lines stay hidden behind the alphabetically-earliest repo that is
   still running.

### Why it feels dead

Three things combine into "no feedback":

* *No scope shown up front.* You never see the list, and the count is buried in
  the footer as `98 pending`. There is no `Fetching 98 repos…` moment.
* *No ticking clock.* The footer is only redrawn on channel events
  (`Started` / `Completed`). Between the initial burst of `Started` events and
  the first `Completed`, nothing redraws — the elapsed time does not advance on
  its own. For network-bound `fetch`/`pull`, that dead window is routinely
  multiple seconds, and the frozen `0.0s` makes it look hung.
* *Head-of-line blocking in the displayed lines.* Because rows flush in
  contiguous alphabetical order, a slow alphabetically-early repo (say
  `activities`) hides every already-finished repo behind it. The footer counter
  moves, but no repo lines appear.

Net: for the first few seconds you stare at a single, static
`0 of 98 done | 16 running | 82 pending | 0.0s` line.

## What the older approach did (and why it was dropped)

The "list first, update in place" UX the older branches had corresponds to
*Option 1: Reserved TTY Slots* in the earlier latency analysis, explored as two
spikes:

* **Reserved rows (Spike 2, `spike/tty-row-updates`).** Print one sorted
  `running…` row per repo up front, then rewrite each row in place on
  completion. Best-in-class for *seeing the whole run at a glance*, but on a
  98-repo workspace it floods the terminal with a "placeholder burst" of 98 rows
  before anything useful happens, and the hand-rolled ANSI cursor math garbled
  scrollback history.
* **Full-page viewport (commit `431b50f`, `crossterm-smart-tty` lineage).** A
  live in-place table that page-stepped through repos with a summary line, then
  flowed the full sorted listing into scrollback at the end via
  `render_complete`. Per the project memory this had *nicer live UX* than the
  sticky footer, but it fought the terminal: each redraw could scroll the top row
  into scrollback (duplicate `abq clean` lines), and on completion the viewport
  parked on whatever page finished last, hiding the rest.

The sticky footer (Spike 4 lineage, `spike-crossterm-smart-tty`) won the
consolidation because it is structurally simpler, keeps clean scrollback for free
(every finished row is a plain `writeln!`), works when repo count exceeds
terminal height, and matches the `cargo build` / `npm install` /
`indicatif::MultiProgress` mental model. The tradeoff we're now feeling: it
optimized scrollback and simplicity at the cost of *startup* and *in-flight*
feedback.

Spike history and metrics live on the `spike/completion-order-output` branch
under `docs/plans/` (`output-spikes.md`, `output-spike-tracker.md`,
`ordered-output-latency.md`), and the shipped design lives in
[`docs/superpowers/plans/2026-05-09-printer-sticky-footer.md`](../superpowers/plans/2026-05-09-printer-sticky-footer.md).

## Constraint worth restating

From the latency analysis: in a plain append-only stream you cannot have all
three of (1) strict alphabetical order, (2) a line the instant a repo finishes,
and (3) no placeholders/redraws. Any two are achievable; getting all the "feels
responsive" properties means either giving up strict live ordering (completion
order) or adopting in-place updates (reserved rows / viewport).

## Options

Ordered roughly smallest-to-largest. They are not mutually exclusive — A, B, and
C compose into one modest change.

### Option A — Make the footer feel alive

Keep the sticky footer, but:

* Redraw on a timer (a lightweight ticker event, e.g. every 100–250ms) so the
  elapsed clock advances and the display never looks frozen.
* List the currently in-flight repos in the footer — e.g. grow it to a few lines
  showing `running: repo-a, repo-b, repo-c …(+13)`. This gives immediate
  "something is happening, and here's what" without an N-row burst.

*Pros:* small, stays inside the current architecture, directly kills the
"looks hung" feeling. *Cons:* footer redraw logic gets slightly more stateful;
need to bound the running list to a few names so it doesn't churn.

### Option B — Print a scope header up front

On `start()`, print one permanent line before the footer:

```text
Fetching 98 repos in ~/work (16 workers)
```

*Pros:* trivial; you immediately know the size of the job. *Cons:* on its own it
doesn't fix the dead window between start and first completion (pairs naturally
with A).

### Option C — Flush finished rows in completion order

Drop the contiguous-alphabetical buffering: print each repo the moment it
finishes, with a stable sorted index prefix so runs stay greppable, and
optionally emit a final sorted summary block. This is Spike 1, which measured the
biggest win on the trace metrics (`delayed_repos` collapsed toward zero,
`first_print_ms` dropped from ~500ms to ~140ms).

*Pros:* removes head-of-line blocking in the *displayed* lines — repos appear as
they finish. Smallest change that makes the list actually populate promptly.
*Cons:* live scrollback is no longer strictly alphabetical (mitigated by the
index prefix and/or an optional final sorted summary).

### Option D — Reserved live rows / bounded viewport ("list first")

The UX the older branches had, done within a viewport bounded to terminal height:
show up to `min(N, viewport_height)` live rows with running/finished state,
update in place, and flow all rows into scrollback on completion (the
`render_complete` idea from `431b50f`).

*Pros:* best "see the whole run at a glance" experience. *Cons:* this is the
approach that was ruled out — placeholder burst, viewport/scrollback bookkeeping,
and the terminal-height problem are real. Highest complexity; likely wants a
maintained library rather than hand-rolled cursor math.

### Option E — Adopt a progress library (`indicatif`)

Use `indicatif::MultiProgress` (already the reference model for the sticky
footer) to manage the multi-line live region + scrollback interleaving instead of
hand-rolling it.

*Pros:* offloads the terminal math that bit us in D; well-tested cross-platform.
*Cons:* new dependency; need to confirm non-TTY / redirected output stays plain,
and that it plays well with our per-repo line format.

## Recommendation

Two phases:

* [ ] **Phase 1 (low risk, high value):** Option A + Option B, and strongly
  consider Option C. Together these give an immediate scope line, a live-ticking
  footer that shows what's in flight, and repo lines that populate as work
  finishes — which addresses the actual complaint ("no feedback / looks hung")
  without reopening the viewport complexity.
* [ ] **Phase 2 (if Phase 1 isn't enough):** evaluate Option D or E as the
  "full live table" north star, informed by whether Phase 1 already feels good on
  `~/work` / `~/src/oss`.

This keeps the change proportional to the problem and preserves the sticky
footer's wins (clean scrollback, simplicity, no height limit) while restoring the
startup and in-flight feedback the consolidation traded away.

## Open questions

* Should completion-order (Option C) be the default, or opt-in behind a flag with
  alphabetical-append kept as default? The trace data favors completion order,
  but greppability/muscle memory may favor keeping sorted output the default.
* How many in-flight repo names should the footer show before collapsing to a
  `(+N)` count?
* Is a ticker thread acceptable, or should the redraw cadence be driven some
  other way (e.g. a short channel `recv_timeout`)?

## References

* [`rust/src/printer.rs`](../../rust/src/printer.rs) — `TtyTablePrinter`,
  `PlainPrinter`, footer rendering.
* [`rust/src/runner.rs`](../../rust/src/runner.rs) — `run_parallel`, the
  `Started`/`Completed` event loop.
* [`docs/dev/issue-crossterm-streaming.md`](issue-crossterm-streaming.md) — the
  original streaming investigation.
* [`docs/superpowers/plans/2026-05-09-printer-sticky-footer.md`](../superpowers/plans/2026-05-09-printer-sticky-footer.md) —
  the shipped sticky-footer design.
* Spike docs on branch `spike/completion-order-output`: `docs/plans/output-spikes.md`,
  `docs/plans/output-spike-tracker.md`, `docs/plans/ordered-output-latency.md`.
* Commit `431b50f` — the full-page viewport printer (`render_complete`), prior art
  for Option D.
</content>
</invoke>
