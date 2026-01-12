# Plan: Rename `nit` to `fit`

Complete rename of the CLI tool from `nit` to `fit` across all implementations, scripts, and documentation. No backwards compatibility needed.

## Summary of Changes

**Scope:** 32+ files across Rust, Zig, Crystal implementations, scripts, and documentation.

---

## Phase 1: Rename Source Files and Update Package Configs

### Crystal Implementation
* [ ] Rename `nit-crystal/src/nit.cr` → `nit-crystal/src/fit.cr`
* [ ] Update `nit-crystal/shard.yml`:
  * `name: nit` → `name: fit`
  * `nit:` target → `fit:`
  * `main: src/nit.cr` → `main: src/fit.cr`

### Rust Implementation
* [ ] Update `nit-rust/Cargo.toml`: `name = "nit"` → `name = "fit"`

### Zig Implementation
* [ ] Update `nit-zig/build.zig`:
  * `.name = "nit"` → `.name = "fit"`
  * `"Run nit"` → `"Run fit"`

---

## Phase 2: Update Source Code String References

### Crystal (`nit-crystal/src/fit.cr` after rename)
* [ ] Help text: `nit - parallel git...` → `fit - parallel git...`
* [ ] Usage examples in help string
* [ ] Version output: `puts "nit #{VERSION}"` → `puts "fit #{VERSION}"`

### Crystal spec file
* [ ] `nit-crystal/spec/repo_spec.cr`: `"nit-test-..."` → `"fit-test-..."`

### Rust (`nit-rust/src/main.rs`)
* [ ] Line 17: `#[command(name = "nit"...)]` → `#[command(name = "fit"...)]`
* [ ] Line 71: error message `"nit: failed to exec..."` → `"fit: failed to exec..."`
* [ ] Line 111: dry-run output `"[nit v{}]..."` → `"[fit v{}]..."`

### Zig (`nit-zig/src/main.zig`)
* [ ] Line 36: help text `\\nit - parallel git...` → `\\fit - parallel git...`
* [ ] Line 39+: all usage examples in help string
* [ ] Line 70: version output `"nit {s}\n"` → `"fit {s}\n"`
* [ ] Error messages mentioning `nit`

---

## Phase 3: Rename Wrapper Scripts in `bin/`

* [ ] Rename `bin/nit-rust` → `bin/fit-rust`
* [ ] Rename `bin/nit-zig` → `bin/fit-zig`
* [ ] Rename `bin/nit-crystal` → `bin/fit-crystal`
* [ ] Update each wrapper's internal binary path reference
* [ ] Update symlink: `bin/nit -> nit-rust` becomes `bin/fit -> fit-rust`

---

## Phase 4: Update Build/Test/Install Scripts

### `script/lib.sh`
* [ ] Update all path discovery functions: `nit-*` patterns → `fit-*`
* [ ] Update binary path constants

### `script/build`
* [ ] Update help text and comments

### `script/test`
* [ ] Update help text and comments

### `script/install`
* [ ] Update help text and comments
* [ ] Update install target names (`nit` → `fit`)

### `script/bench`
* [ ] Update all `nit` references (binary names, labels, comments)

---

## Phase 5: Update Documentation

### `README.md`
* [ ] Title and all references (28+ occurrences)

### `CLI.md`
* [ ] All CLI references (40+ occurrences)
* [ ] Update link to `knit-future.md` (keep file but content updated)

### `SPEC.md`
* [ ] All specification references (20+ occurrences)

### `CLAUDE.md`
* [ ] Project overview and examples (15+ occurrences)

### `docs/knit-future.md`
* [ ] Replace all `knit` → `fit` (32 occurrences)
* [ ] Replace `nit` references within that file
* [ ] This becomes `fit` roots-based functionality documentation

### `docs/git-notes.md`
* [ ] Update all references

### `docs/benchmarks.md`
* [ ] Update references

### `docs/issue-crossterm-streaming.md`
* [ ] Update references (e.g., `nit-rust` → `fit-rust`)

---

## Phase 6: Update Configuration Files

### `.gitignore`
* [ ] `nit-rust/target` → `fit-rust/target`
* [ ] `nit-zig/zig-out` → `fit-zig/zig-out`
* [ ] `nit-crystal/bin` → `fit-crystal/bin`

### `.circleci/config.yml`
* [ ] Update working directory paths for all implementations

---

## Phase 7: Rename Implementation Directories

**Must be done after all file content updates to avoid broken paths during editing.**

* [ ] `mv nit-rust fit-rust`
* [ ] `mv nit-zig fit-zig`
* [ ] `mv nit-crystal fit-crystal`

---

## Phase 8: Rebuild and Verify

* [ ] Run `script/build` to rebuild all implementations
* [ ] Run `script/test` to verify tests pass
* [ ] Grep for any remaining `nit` references: `grep -ri "nit" --include="*.rs" --include="*.zig" --include="*.cr" --include="*.md" --include="*.sh" --include="*.yml" --include="*.toml"`

---

## Phase 9: Verify Everything Works

### Verify `fit` CLI works
* [ ] `./bin/fit-rust --version` outputs `fit X.X.X`
* [ ] `./bin/fit-rust --help` shows `fit` in usage
* [ ] `./bin/fit-rust status` runs successfully
* [ ] `./bin/fit-zig --version` outputs `fit X.X.X`
* [ ] `./bin/fit-zig status` runs successfully
* [ ] `./bin/fit-crystal --version` outputs `fit X.X.X`
* [ ] `./bin/fit-crystal status` runs successfully
* [ ] `./bin/fit` symlink works (points to fit-rust)

### Verify all scripts work
* [ ] `script/build` works for all implementations
* [ ] `script/build -t rust` works
* [ ] `script/build -t zig` works
* [ ] `script/build -t crystal` works
* [ ] `script/test` works for all implementations
* [ ] `script/install -t rust` works
* [ ] `script/bench` runs without errors (quick sanity check)

---

## Phase 10: Rename Git Repository

* [ ] Rename via GitHub settings: `rsanheim/nit` → `rsanheim/fit`
* [ ] Update local remote URL after GitHub rename

---

## Phase 11: Dotfile Cleanup (Final)

* [ ] Check for any local dotfile configs that reference `nit` (e.g., shell aliases, PATH entries)
* [ ] Update `~/.local/bin/nit` if installed there → `~/.local/bin/fit`
* [ ] Any other local machine configs that may reference the old name

---

## Verification Checklist (Summary)

* [ ] All `fit` CLIs output correct version and help
* [ ] `fit` runs git operations successfully
* [ ] All scripts (build, test, install, bench) work
* [ ] No remaining `nit` references in repo (except git history)
* [ ] CI config points to correct directories
* [ ] GitHub repo renamed
* [ ] Local dotfiles updated

---

## Files Summary

**Implementation directories (3):**
`nit-rust/`, `nit-zig/`, `nit-crystal/`

**Package configs (3):**
`Cargo.toml`, `shard.yml`, `build.zig`

**Source files with string changes (5):**
`main.rs`, `main.zig`, `nit.cr` (→ `fit.cr`), `runner.cr`, `repo_spec.cr`

**Scripts (5):**
`script/lib.sh`, `script/build`, `script/test`, `script/install`, `script/bench`

**Bin wrappers (3):**
`bin/nit-rust`, `bin/nit-zig`, `bin/nit-crystal`

**Documentation (8):**
`README.md`, `CLI.md`, `SPEC.md`, `CLAUDE.md`, `docs/knit-future.md`, `docs/git-notes.md`, `docs/benchmarks.md`, `docs/issue-crossterm-streaming.md`

**Config (2):**
`.gitignore`, `.circleci/config.yml`
