# Changelog

Actions performed on the 0-shell project, newest first.
Format: `Task # - Action - Quick Summary`

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
  `tasks 4–9` became `tasks 4-9`. Repo now scans clean for both `—` and `–`.

### Phase 1 - Filesystem module scaffolding

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

## Verification

Every entry above was left with `cargo fmt --check` clean, `cargo clippy --all-targets -- -D warnings`
warning-free, and 18/18 tests passing.
