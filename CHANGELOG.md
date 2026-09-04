# Changelog

Actions performed on the 0-shell project, newest first.
Format: `Task # - Action - Quick Summary`

## 2026-09-04

### Phase 2 - Filesystem commands

- **Task 5 - Implemented `cat` - Streams each operand to the output as raw bytes.**
  `File::open` per operand, then a 64 KiB block copied straight to the output writer, so a
  large file costs a fixed amount of memory and the bytes are never routed through `String`.
  A file holding invalid UTF-8 comes out unchanged, and nothing is inserted between operands
  or after the last one: `cat a b` is exactly the bytes of `a` followed by the bytes of `b`.
  A directory is caught by checking the metadata of the handle already opened, not by letting
  the first read fail, because the read error wording varies by platform. The result is
  `cat: <path>: Is a directory` before any byte of output is written.
  An unreadable operand is reported and the remaining ones are still printed, so
  `cat before nope.txt after` prints both `before` and `after`.
  The internal `Failure` enum splits the two error directions: a failed *read* is a message on
  `errors` and an `Ok(())`, while a failed *write* to the output is the shell's own stream
  breaking and propagates as `Err`, which is the case `filesystem`'s convention reserves for
  it. `io::copy` was rejected for this reason, since its error does not say which side failed.
  Verified against the host `cat` on the audit case: stdout is byte identical for a text file,
  a file with no trailing newline, and a binary file, checked with `cmp` and `xxd`.
  Not implemented: `cat` with no operands. GNU `cat` copies standard input there, which this
  shell cannot offer, because standard input is the line reader driving the prompt and reading
  it would swallow the next command. It reports `cat: missing operand` instead.
  Worth knowing for the audit: this machine's `/usr/bin/cat` is uutils coreutils 0.2.2, not
  GNU, and uutils appends Rust's errno to the message
  (`cat: nope.txt: No such file or directory (os error 2)`). Our output keeps the GNU wording
  without the suffix, per the task 3 decision. The `Is a directory` case, which is the one the
  audit script names, matches both implementations exactly.

## 2026-09-03

### Housekeeping

- **Task 25 - Fixed `.gitignore` - Repaired a pattern that git read as a filename and a rule that contradicted its own comment.**
  Line 3 (` Ignore all text and markdown files...`) was missing its `#`, so git parsed it as a
  pattern. `DOCs/` and `DOCs/**` excluded the directory the comment said to keep, and a
  re-include cannot rescue files inside an excluded directory. Rewrote as grouped ignores
  (`*.txt`, `*.md`, `*.log`, `/target`) followed by negations. Verified every pattern with
  `git check-ignore --no-index`.

- **No task - Wrote `Tasks.md` - Broke the remaining work into 25 numbered tasks across 4 phases.**
  Audited the repo against `Exercise.md`, `Audit.md`, and `PLAN.md`. Found Developer A's scope
  complete and committed, Developer B's scope not started. Flagged three non-obvious items:
  `ls -l` needs hand-written uid/gid lookup and date formatting because the crate has zero
  dependencies; `mv` needs an `EXDEV` copy-then-delete fallback; the `.gitignore` was hiding
  the project docs from git.

- **No task - Removed em-dashes - Replaced the last en-dash in `Tasks.md` and recorded the rule.**
  The last en-dash range in `Tasks.md` became a plain `tasks 4-9`. The repo, this file
  included, now scans clean for both dash characters (U+2014 and U+2013). Recorded as a
  standing preference so it survives into later sessions.

### Phase 2 - Filesystem commands

- **Task 4 - Implemented `mkdir` - Creates each operand, reports failures, keeps going.**
  `fs::create_dir` per operand, `report::missing_operand` when given none, and
  `mkdir: cannot create directory '<path>': <reason>` on failure. A failed operand does not
  stop the rest, so `mkdir first absent/child last` still creates `first` and `last`.
  Verified the audit scenario end to end through the built binary: `new_folder1`,
  `new_folder2`, then `parent` with `child1` and `child2`, followed by `cd parent` and `pwd`.
  Dropped the `TODO(task 4-9)` attributes from `quoted` and `missing_operand`, which now have
  real callers.
  Not implemented: `-p`. The subject does not require it and the audit does not exercise it,
  so `mkdir -p x` creates a directory literally named `-p` alongside `x`.

- **Task 16 (partial) - Added the temp-directory test fixture - `filesystem/testing.rs`.**
  Built early because task 4's tests needed it and copying the same setup into six command
  files would be worse. `TempDir::new(label)` makes a uniquely named directory under the
  system temp directory and removes it recursively on drop. Tests address it with absolute
  paths through `TempDir::arg` instead of changing the working directory, since the working
  directory is process-wide and the harness runs tests in parallel.

### Phase 1 - Filesystem module scaffolding

- **Task 3 - Added the shared report helper - `src/commands/report.rs`, with 7 tests.**
  Placed at `commands/` level rather than inside `filesystem/`, because `cd` and `pwd` in
  `session.rs` leaked the same errno suffix and a helper buried in `filesystem/` could not
  reach them. Provides `reason` (strips ` (os error N)`), `quoted`, `message`, `detail`,
  `io_error`, `missing_operand`, `missing_file_operand`, `missing_destination_operand`.
  Fixed the two existing convention violations: `cd /nonexistent` now prints
  `cd: /nonexistent: No such file or directory`, matching bash exactly instead of appending
  `(os error 2)`; `pwd` routes through `reason` too.
  Four helpers with no caller yet were given a `TODO(task 4-9)` `#[allow(dead_code)]`, since
  test-only callers do not satisfy `dead_code` in a non-test build. Each attribute is deleted
  by the task that first calls the helper; two remain, on `missing_file_operand` and
  `missing_destination_operand`, waiting for `cp` and `mv`.

- **Task 1 - Created the filesystem module tree - Six command files, wired into the dispatcher.**
  Added `src/commands/filesystem/` with `mod.rs` plus `mkdir.rs`, `cat.rs`, `cp.rs`, `mv.rs`,
  `rm.rs`, `ls.rs`. Chose one file per command because `ls -l` alone (tasks 9-13) will run to
  several hundred lines and would swamp anything sharing a file with it. Signatures follow the
  `session.rs` precedent, each command takes only the writers it needs: `mkdir`/`cp`/`mv`/`rm`
  take `errors`, while `cat`/`ls` also take `output`. Each stub reports
  `<name>: not implemented yet` and returns `Continue`.
  Also registered all six arms in `dispatch` (task 14, done early): stubs in a private module
  with no caller trip `dead_code`, which fails `cargo clippy -- -D warnings`, so the scaffold
  could not pass its own build checks unregistered. Tasks 4-9 are now fill-in-the-body only.

- **Task 2 - Pinned the error-reporting convention - Documented as the module doc on `filesystem/mod.rs`.**
  Placed it above the six modules it governs so it cannot drift into a separate file nobody
  reads. Two findings changed what was written:
  1. The `<command>: <path>: <reason>` shape is correct only for `cat`. The other five name the
     failed action and quote the path (`rm: cannot remove 'adir': Is a directory`,
     `ls: cannot access 'x': ...`, `mkdir: cannot create directory 'x': ...`,
     `cp`/`mv: cannot stat 'x': ...`). Applying one template uniformly would have made five of
     six commands visibly wrong against a host shell.
  2. `io::Error` must never be formatted with `{}`. Rust appends the errno, giving
     `No such file or directory (os error 2)` where Unix prints
     `No such file or directory`. The convention requires a shared helper that strips the
     ` (os error N)` suffix.
  Recorded that this machine runs uutils coreutils 0.2.2, not GNU, so GNU wording could not be
  verified locally. Noted that `session.rs:62` currently leaks the `(os error 2)` suffix in
  `cd`, the one existing violation, to be fixed with the task 3 helper.

## Current state

Done: tasks 1, 2, 3, 4, 25, and task 16 in part. Task 14 was absorbed into task 1.
Next: task 5 (`cat`), then 6 (`cp`), 7 (`mv`), 8 (`rm`), and 9 through 13 (`ls`).
The five filesystem commands other than `mkdir` (`cat`, `cp`, `mv`, `rm`, `ls`) are still
stubs reporting `<name>: not implemented yet`.

## Verification

Every entry above was left with `cargo fmt --check` clean, `cargo clippy --all-targets -- -D warnings`
warning-free, and every test passing (31 as of task 4).
