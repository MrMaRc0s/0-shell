# 0-Shell - Remaining Tasks

Status (updated 2026-09-20, 330 tests passing):

- **Done:** everything below. Shell loop and prompt (now `<cwd> $ `), Ctrl+D/EOF
  exit, line parser (quotes + escapes), unknown-command message, `echo`, `pwd`,
  `cd`, `exit`, all filesystem commands (`ls` incl. `-l`/`-a`/`-F`/`-R`,
  `cat`, `cp` incl. `-r`/`-R`/`-p`, `rm` incl. `-r`/`-R`/`-f`/`-i`,
  `mv` incl. `-i` and the `EXDEV` fallback, `mkdir` incl. `-p`),
  dispatcher registration, unit tests for all of them, the end-to-end audit
  test, the stability sweep, and the README.
- **Done (bonus):** Ctrl+C (prompt and during-command, via a `sigaction`
  handler), auto-completion, history (memory + `$HOME/.0-shell_history`),
  current-directory prompt, colorized `ls`, `;` chaining, pipes, redirection,
  environment variables (`export`/`unset`, `$NAME` expansion), `help`, and
  extra commands (`clear`, `touch`, `type`, `which`).
- **Still open:** the hardening list in `ReviewFindings.md` (FIX-007/008,
  remainder of FIX-009/013/014, ARCH-001..007, TEST-001..009).

Original snapshot (as of commit `854dad2`), kept for history:

- **Done at the time:** shell loop and prompt, Ctrl+D/EOF exit, line parser (quotes + escapes),
  unknown-command message, `echo`, `pwd`, `cd`, `exit`, plus unit tests for all of them.
- **Not started at the time:** every filesystem command (`ls`, `cat`, `cp`, `rm`, `mv`, `mkdir`),
  their dispatcher registration, and the final integration/audit pass.

The numbering below is roughly the recommended order.

---

## Phase 1 - Filesystem module scaffolding

### 1. Create the filesystem module tree
Add `src/commands/filesystem/mod.rs` and declare `mod filesystem;` in `src/commands/mod.rs`.
Decide now whether each command gets its own file (`mkdir.rs`, `cat.rs`, `cp.rs`, `mv.rs`,
`rm.rs`, `ls.rs`) or all live in `mod.rs`. Separate files are recommended - `ls -l` alone is
large enough to dominate a shared file.

### 2. Agree on the error-reporting convention
All filesystem commands must follow the pattern already established in
`src/commands/session.rs`: write to the `errors` writer, return `io::Result<()>`, and never
abort the shell. Standard Unix wording is `<command>: <path>: <reason>`, e.g.
`cat: nope.txt: No such file or directory`. Pin this down before writing six commands that
each invent their own format.

### 3. Add a shared path/argument helper
A small helper for "no operands given" (`mkdir: missing operand`) and for turning an
`io::Error` into a Unix-style message, so tasks 4-9 don't each duplicate it.

---

## Phase 2 - Filesystem commands

### 4. Implement `mkdir`
- Accept one or more paths; create each with `std::fs::create_dir`.
- Report failure per-path and keep going with the remaining paths.
- Error on zero arguments (`mkdir: missing operand`).
- Audit relevance: *"mkdir new_folder1 / new_folder2"* and the parent-with-two-children step.

### 5. Implement `cat`
- Accept one or more files; stream each to the output writer.
- Write bytes, not `String` - file contents may not be valid UTF-8, and the audit compares
  output byte-for-byte with the host `cat`.
- Directory argument must produce `cat: <name>: Is a directory`, not a panic.
- Audit relevance: *"cat new_folder1/new_doc.txt"* must match the host terminal exactly.

### 6. Implement `cp`
- `cp <src> <dst-file>` - copy to a new/overwritten file path.
- `cp <src> <dst-dir>` - when the destination is an existing directory, copy *into* it
  keeping the source's basename. This exact case is the audited one
  (`cp new_folder1/new_doc.txt new_folder2`).
- `cp <src>... <dst-dir>` - multiple sources into a directory.
- Preserve file permissions where practical; report unreadable sources without crashing.
- Decide and document whether `cp -r` for directories is in scope (the subject says
  "files and directories as required by normal Unix `cp` behavior"; the audit only exercises
  the file case, so `-r` is a low-priority extra). Update 2026-09-20: decided —
  `-r`/`-R` are in scope and implemented, including the symlink rule.

### 7. Implement `mv`
- Rename within the same filesystem via `std::fs::rename`.
- Moving *into* an existing directory must keep the basename
  (`mv new_folder2 new_folder1` → `new_folder1/new_folder2`) - this is the audited case.
- Handle the cross-device case: `rename` fails with `EXDEV`, so fall back to copy-then-delete
  (reuse the `cp` and `rm` internals rather than duplicating them).
- Works for both files and directories.

### 8. Implement `rm`
- Without flags: remove files only; refuse a directory with
  `rm: <name>: Is a directory`.
- With `-r` (and accept `-R`): recursive directory removal.
- Report a missing path as `rm: <name>: No such file or directory` and continue with the
  remaining operands.
- Guard against the obvious footgun: no arguments must be an error, never a no-op that
  deletes something unexpected.
- Audit relevance: *"rm -r new_folder1"*.

### 9. Implement `ls` - plain listing
- Read the directory with `std::fs::read_dir`, hide dotfiles by default, sort entries by name.
- Support a file argument (prints the name), a directory argument (prints its contents), and
  multiple arguments (headers `name:` between blocks, matching Unix).
- Default output format: the audit compares against the host `ls`. Multi-column output is
  what a terminal `ls` produces, but real `ls` falls back to one-per-line when piped; decide
  which you're matching and note it in the README so the auditor isn't surprised.

### 10. Implement `ls -a`
Include `.` and `..` and dotfiles in the listing.

### 11. Implement `ls -F`
Append the type indicator: `/` directories, `*` executables, `@` symlinks, `|` FIFOs,
`=` sockets.

### 12. Implement `ls -l` - the long format
The largest single task in the project. Each line needs:
- File type character + permission bits rendered as `drwxr-xr-x` (from
  `std::os::unix::fs::PermissionsExt::mode`).
- Hard link count, from `MetadataExt::nlink`.
- **Owner and group names.** There is no libc dependency in `Cargo.toml`, so `uid`/`gid`
  must be resolved by parsing `/etc/passwd` and `/etc/group` yourself, falling back to the
  numeric id when no entry matches. Decide this early - it's the one part of `ls -l` that
  isn't a straight std-library call.
- Size (and `major, minor` for device files, as in the subject's `/dev` example).
- Modification time formatted like Unix: `Mon DD HH:MM` for recent files, `Mon DD  YYYY` for
  files older than ~6 months. Requires converting `MetadataExt::mtime` (Unix epoch seconds)
  into a civil date/time yourself - a date algorithm, since there is no `chrono` dependency.
- The `total <blocks>` header line, summed from `MetadataExt::blocks() / 2`.
- Column alignment: each field is right-padded to the widest value in the listing.
- Symlinks shown as `name -> target`.

### 13. Make `ls` flags combinable
`ls -l -a -F`, `ls -laF`, and `ls -al` must all work - the audit runs `ls -l -a -F`
explicitly. Parse clustered short flags, and reject unknown flags with
`ls: invalid option -- 'x'` rather than treating them as filenames.

### 14. Register every filesystem command in the dispatcher
Add the `mkdir`/`cat`/`cp`/`mv`/`rm`/`ls` arms to `dispatch` in `src/commands/mod.rs`.
This is PLAN.md's stated integration point between Developer A and Developer B.

---

## Phase 3 - Tests

### 15. Unit tests for each filesystem command
Follow the existing style in `session.rs`: pass `Vec<u8>` writers, assert on output and error
bytes. Every command needs at least a success case and a failure case (missing path,
wrong type, missing operand).

### 16. Temporary-directory test fixture
Filesystem tests need a scratch directory created and torn down per test. There is no
`tempfile` dependency, so either add one or write a small RAII helper (the `RestoreDirectory`
guard in `session.rs` is the pattern to follow). Note that tests changing the process's
current directory must share a lock, as the existing tests already do.

### 17. End-to-end test of the audit workflow
One test that drives `run_with_io` with the audit's full script:
`mkdir` → `cp` → `cat` → `mv` → `rm -r`, inside a temp directory, asserting the final state.

---

## Phase 4 - Integration and audit rehearsal

### 18. Run the full check suite
`cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` - all clean.

### 19. Verify no external binaries are spawned
Grep the source for `std::process`, `Command`, `exec`, `system`. The audit asks this
explicitly and it is a hard fail. Confirm `Cargo.toml` has no dependency that shells out.

### 20. Walk every mandatory audit question by hand
Build the release binary and run each item in `Audit.md` in a disposable directory,
side-by-side with the host terminal:
prompt display · Enter-only execution · `exit` · `echo "something"` · `echo something else` ·
`pwd` · `mkdir` parent+children then `cd`+`pwd` · `cd dir/of/your/choice` · bare `cd` ·
`ls` · `ls -l -a -F` · `cp` into a directory · `cat` · `mv` directory into directory ·
`rm -r` · Ctrl+D · unknown command message.

### 21. Diff output against the host shell for the picky cases
`echo`, `cat`, and `ls -l` are the three where "similar" isn't enough. Capture both outputs
to files and `diff` them.

### 22. Stability sweep
Deliberately abuse the shell and confirm it never panics or exits unexpectedly:
no arguments, too many arguments, nonexistent paths, a file where a directory is expected,
permission-denied paths (e.g. `/root`), unclosed quotes, very long input, `cd` into a file,
`cat` of a binary file, and empty input lines.

### 23. Update the README
Describe the finished command set and any documented deviations from GNU behavior
(e.g. the `ls` column decision from task 9, whether `cp -r` is supported).

### 24. Decide the bonus question — decided and shipped (2026-09-20)
`PLAN.md` once declared the whole bonus backlog out of scope. That no longer
holds: the cheapest wins (current-directory prompt, colorized output) and the
expensive ones (Ctrl+C handling incl. during-command, pipes) are all
implemented, plus history, chaining, redirection, environment variables, and
`help`. Nothing in the subject forbids them. See `ReviewFindings.md` for the
remaining hardening items, which are robustness work, not missing features.

---

## Housekeeping

### 25. Fix `.gitignore`
The uncommitted change adds `*.md` with only `README.md`, `COMMANDS.md`, and `RESULTS.md`
re-included - which means `Exercise.md`, `Audit.md`, `PLAN.md`, and this `Tasks.md` are now
ignored, and `COMMANDS.md`/`RESULTS.md` don't exist yet. Also line 3 of the change
(`  Ignore all text and markdown files...`) is missing its `#`, so git reads it as a pattern.
Decide what actually belongs in the repo and rewrite the file before committing.
