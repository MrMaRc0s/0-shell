# Changelog

Actions performed on the 0-shell project, newest first.
Format: `Task # - Action - Quick Summary`

## 2026-09-05

### Phase 2 - Filesystem commands

- **Task 8 - Implemented `rm` - Files by default, whole trees with `-r`, per-path reporting.**
  A directory needs `-r` or `-R` and is refused otherwise. Symlinks are unlinked and never
  followed, in both modes, so `rm dirlink` removes the link and leaves the directory it points
  at untouched: every stat is an `lstat`, never a `stat`. One failing operand is reported and
  the rest are still removed. Verified the audit case `rm -r new_folder1` end to end.
  Wording note for the team: task 8's text in `Tasks.md` shortens the messages to
  `rm: <name>: Is a directory`, but the host prints
  `rm: cannot remove '<name>': Is a directory`, which is also what the convention in
  `filesystem/mod.rs` pinned down back in task 2. The host wins, since the audit compares
  against a real terminal. `Tasks.md` was written before that convention existed.
  The recursive walk is hand written rather than `fs::remove_dir_all`, which reports one error
  for a whole tree and cannot name the path inside it that actually failed. A directory whose
  contents could not all be removed is not handed to `remove_dir` afterwards: the child's
  failure is the honest one, and the `Directory not empty` that would follow says the same
  thing twice. The host behaves the same way, and there is a test for it that skips itself
  when the process is root, where an unwritable directory refuses nothing.
  Two failsafes, both matching the host. `.` and `..` as the last component are refused with
  `rm: refusing to remove '.' or '..' directory: skipping '<path>'`, and a path resolving to
  the root directory with `rm: it is dangerous to operate recursively on '<path>'`, with the
  host's `(same as '/')` suffix when it was spelled some other way, such as `//`. The advice
  to use `--no-preserve-root` is dropped, as with the `Try '<command> --help'` lines, since
  this shell has no such flag.
  The failsafes live in a separate `refusal` function returning the message rather than
  printing it, so the test suite can prove they hold without ever running the removal they
  exist to prevent. That test earned its keep immediately: the first version of the `.` check
  used `Path::components`, which normalizes an interior `.` away and reads `dir/.` as plain
  `dir`. `rm -r <dir>/.`, the spelling most likely to be typed by accident, went straight
  through and emptied the directory. The check now reads the path text.
  Also added `-f`, which is not in task 8's list. `rm -rf <dir>` is what a hand types even
  when the audit script says `rm -r <dir>`, and `invalid option -- 'f'` at that moment costs
  an audit step. It ignores what is not there, including no operands at all, and changes
  nothing else: a directory without `-r` is still refused, exactly as on the host.
  Verified against the host `rm` across 13 cases covering both flags, both failsafes, a
  symlink to a directory, an unremovable child, a missing operand, and bad options. The error
  text is identical once the host's `Try 'rm --help'` and `--no-preserve-root` lines are
  dropped, and the entries surviving in the directory afterwards are identical too.

## 2026-09-04

### Phase 2 - Filesystem commands

- **Task 6 - Implemented `cp` - File, into-directory, multi-source, and recursive copies.**
  The last operand is the destination. An existing directory means "copy into it under the
  source's basename", which is the audited case; anything else is the exact target path, so
  `cp a b` creates or overwrites `b`. Several sources require a directory target, otherwise
  `cp: target '<dst>': Not a directory` when it exists and the OS reason when it does not.
  `-r` and `-R` are in scope, decided rather than deferred. Task 7's `mv` needs an `EXDEV`
  fallback that copies a directory tree and its own task says to reuse the `cp` internals, so
  the recursive walk had to exist either way, and the subject asks for "files and directories
  as required by normal Unix `cp` behavior". Without the flag a directory source is refused
  with `cp: -r not specified; omitting directory '<path>'`, exactly as GNU does.
  Symlinks follow the real `cp` rule: a plain copy dereferences the link and copies the file
  behind it, a recursive copy recreates the link. Verified against the host both ways.
  Permissions are preserved on files and on directories created by the walk. Directory modes
  are applied after the contents are written, since a source directory without write
  permission would otherwise lock its own copy before the files went in.
  Two guards against footguns that lose data rather than merely fail. Copying a file onto
  itself is refused with `cp: '<src>' and '<dst>' are the same file`, compared by device and
  inode, not by path text, because creating the target truncates it and `cp a ./a` would
  otherwise leave an empty file. Copying a directory into itself is refused with
  `cp: cannot copy a directory, '<src>', into itself, '<dst>'`, checked on canonicalized
  paths, because the walk would otherwise copy the tree it is growing and fill the disk.
  Devices, FIFOs, and sockets are skipped with `cp: omitting special file '<src>'`. The
  standard library cannot create those nodes without libc, and reading a FIFO would hang the
  shell until something wrote to it.
  `cp` copies through `File::open` plus `File::create` plus `io::copy` rather than `fs::copy`,
  because `fs::copy` returns one error for both sides and Unix names the side that failed
  (`cannot open '<src>' for reading` against `cannot create regular file '<dst>'`). No speed
  is lost: `io::copy` between two files uses the same `copy_file_range` path underneath.
  Verified against the host `cp`: the audit case produces an identical tree, a recursive copy
  of a tree with a nested directory, a symlink, and non-default modes is identical under
  `diff -r` and `stat`, and all ten error cases print byte for byte what the host prints once
  its `Try 'cp --help'` lines are dropped.

- **Task 6 (shared) - Added the short-option parser - `filesystem/options.rs`, with 6 tests.**
  Written for `cp -r` but built for tasks 8 and 13, which need `rm -rf` and the clustered
  `ls -laF` with the same invalid-option wording. Each command names the letters it accepts,
  and gets back the flags and the operands in their original order. Flags work in any
  position (`cp a b -r`), cluster (`-rR`), and stop at `--`, so a file genuinely named `-r` is
  still reachable as `cp -- -r dst`. A lone `-` stays an operand. Unknown options are returned
  whole rather than as text, so a short flag reports `cp: invalid option -- 'q'` and a long one
  reports `cp: unrecognized option '--bogus'`, which is what `getopt` prints for each.
  This is a deliberate divergence from `mkdir`, which parses no flags at all and so creates a
  directory named `-p`. `mkdir` gains nothing from the parser today; it can adopt it if `-p`
  is ever wanted.
  Added `report::invalid_option` and `report::unrecognized_option` alongside it, and dropped
  the `TODO(task 4-9)` attributes from `missing_file_operand` and `missing_destination_operand`,
  which `cp` now calls.

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
