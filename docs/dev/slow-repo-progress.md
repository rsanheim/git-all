# Progress while one repository is slow

The Rust runner already executes repositories concurrently and receives start
and completion events independently. The apparent freeze has two causes:

* Both printers buffer completed rows until preceding repositories finish, to
  preserve alphabetical output (SPEC section 3.2.4).
* The TTY footer redraws only on start/completion events. Once the other repos
  finish, even its elapsed clock stops advancing until the slow repo exits.

## Focused fix

Keep alphabetical rows and refresh the existing two-line TTY footer every
250 ms while the event channel is quiet. Use the separator line to identify
the repository blocking output and its own running time. Completion counts
remain in the summary:

```text
WAITING repo-053 | running 16.0s
SUMMARY   119 of 120 done | 1 running | 0 pending | 16.6s
```

In this example, 52 repository rows have printed, repo 53 is still running,
and 67 other repositories have finished. Their rows wait for repo 53 so output
stays alphabetical. The summary's 119 completions include those 67 rows.

A repository waiting for a worker is labeled `pending`; its running clock
begins when a worker starts it. The final footer restores the separator. The
footer remains two lines, using the existing crossterm cursor/clear code;
names are shortened before the runtime at narrow widths.

Piped output still blocks on the event channel without periodic wakeups,
progress text, or ANSI sequences. Git execution, output draining, worker limits,
trace bookkeeping, and dependencies are unchanged.

## Comparison

Separate prototypes were exercised using the existing `script/tty-lab` and
tmux captures, with 120 local repositories and a 30-second delay at repo 53:

| Approach | While repo 53 runs | Tradeoff |
| --- | --- | --- |
| Baseline | Rows stop at 52; summary says 119 done but clock remains at 1.3s | Appears frozen and does not identify the blocker |
| Alphabetical rows + timed footer | Rows stop at 52; footer identifies repo 53 and keeps both clocks advancing; summary counts all completed repos | Preserves the output contract; selected fix |
| Completion-order rows + timed footer | All 119 other rows print before repo 53; footer names the remaining repo | Changes deterministic alphabetical output required by the spec |

The ordered prototype's repo clock advanced from 14.0s to 16.0s in successive
live captures. Its progress line also remained readable at 60 columns.

## Reproduce

The lab was recovered from the earlier startup-feedback branch and extended
with numbered names and one targeted delay. It uses local remotes and a git
latency shim, so the experiment does not update personal checkouts or need the
network:

```bash
script/build -t rust --dev
script/tty-lab 120 --numbered --remotes --min-ms 50 --max-ms 50 \
  --slow-repo repo-053 --slow-ms 30000 \
  --bin "$PWD/rust/target/debug/git-all" --run pull
```

Run in a terminal to see the live footer. Use `--slow-ms 15000` for a shorter
stall. `script/test -t rust` covers footer ticks, per-repo timing, buffered row
ordering, narrow names, final row uniqueness, and plain output behavior.
