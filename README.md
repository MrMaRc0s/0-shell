# 0-Shell

`0-shell` is a small Unix-like shell written in Rust. Every command is
implemented with Rust's standard filesystem and operating-system APIs: the
shell never launches an external binary, and `Cargo.toml` has no dependencies.

Not launching a binary is the subject's rule. Taking no dependencies is this
project's own choice, kept because it is what makes the shell interesting: the
owner and group columns of `ls -l` are read from `/etc/passwd` and `/etc/group`,
its dates are computed from the system's timezone file, and its line editor
talks to the kernel directly.

## Build and run

```sh
cargo build
./target/debug/0-shell
```

The shell displays `$ `, runs a command once Enter is pressed, and exits
cleanly on `exit` or on end-of-input (Ctrl+D).

The implementation order and two-developer work split are recorded in
[`PLAN.md`](PLAN.md).

## Commands

| Command | Options | Notes |
|---------|---------|-------|
| `echo`  | none | Arguments are printed separated by one space. Quotes and backslash escapes are handled by the parser, not by `echo`. |
| `pwd`   | none | Rejects any argument. |
| `cd`    | none | One operand, or none for `$HOME`. |
| `exit`  | none | Rejects any argument. |
| `clear` | none | Clears the screen and its scrollback (past commands) and homes the cursor, so the shell is left alone at the top. Arguments are ignored. |
| `mkdir` | `-p` | One or more directories. `-p` creates each operand's missing parents in one go and passes over a directory that already exists; without it each parent has to exist already. |
| `cat`   | none | One or more files, streamed as raw bytes, or a redirected file read as standard input. |
| `cp`    | `-r`, `-R` | Copies into an existing directory under the source's basename, or to an exact path. Several sources need a directory destination. |
| `mv`    | none | Renames, or moves into an existing directory under the source's basename. Files and directories alike, no flag needed. |
| `rm`    | `-r`, `-R`, `-f` | Refuses a directory without `-r`. |
| `ls`    | `-l`, `-a`, `-F`, `-R` | Combinable and clusterable: `ls -l -a -F`, `ls -laF`, and `ls -al` are the same request. On a real terminal the output is coloured the way GNU's `--color=auto` is; piped output stays plain. `-R` descends into every subdirectory, heading each one with its path; a symlink to a directory is listed but not walked into. |
| `touch` | `-c`, `-a`, `-m` | Creates a file if missing, otherwise stamps its timestamps to now. `-c` never creates, `-a` and `-m` narrow the stamping to accessed or modified time. GNU `touch`'s `-t`, `-d`, and `-r` are refused as invalid options. |

Options may appear in any position, `--` ends option parsing, and an unknown
option is refused (`ls: invalid option -- 'x'`) rather than taken for a
filename.

Anything else is reported as `Command 'name' not found`, and the shell carries
on. No command aborts the shell, and a failing operand never stops the ones
after it: `rm present missing other` removes both `present` and `other`.

## Redirecting input and output

`>` writes a command's output to a file, creating or emptying it; `>>` appends
to a file, creating it if needed; `<` feeds a file to a command's input. The
last redirection to a stream wins, and several apply left to right. `> file`
alone still creates the file, as it does in any POSIX shell. A redirection that
cannot be opened stops only its command —
`0-shell: cannot create 'x': Permission denied` — and the shell carries on.
`cat` reads a redirected file as standard input, and labels a read error on it
`cat: (standard input)`. `>>`, `>`, and `<` are recognised only outside quotes,
so a file name can genuinely contain them.

On a real terminal, `ls` also colours its output the way GNU's `--color=auto`
does: directories in bold blue, executables in bold green, FIFOs in yellow,
sockets and symlinks in their own colours. Piped or redirected output is plain
bytes, exactly as before.

### What `cp` and `mv` do with the awkward cases

`cp -r` and `-R` are supported and behave as Unix `cp` does, including the
symlink rule: a plain copy follows a link and copies the file it points at,
while a recursive copy recreates the link itself. Device files, FIFOs, and
sockets are reported as `cp: omitting special file 'name'` rather than half
copied, since creating those nodes needs a system call the standard library
does not expose.

`mv` takes no flags. A rename moves a whole tree in one step, so there is
nothing for `-r` to switch on. A rename cannot cross a filesystem boundary, so
when the kernel refuses for that reason (`EXDEV`) the move falls back to the
same recursive copy `cp -r` runs, followed by the same removal `rm -r` runs.
The source is deleted only once the copy is known to have completed, so a
fallback that fails partway leaves the original where it is.

## Interactive editing

At a terminal the shell reads a line itself rather than letting the kernel do
it, which is what makes the three bonus features below possible. Piped or
redirected input takes the plain path instead, unchanged and unaware any of this
exists, so scripts and the audit transcript behave exactly as they always did.

The editor talks to the kernel through the `TCSETS`/`TCGETS` ioctls, issued by
the shell itself (the only assembly in the project). Each supported system uses
its own kernel's constants and calling convention — Linux and macOS both get
raw mode. On any other system, or when input is piped, the shell detects it
cannot enter raw mode and takes the plain reading path, so every required
behaviour is still present and only the three bonus features are absent.

| Key | Does |
|---|---|
| `Tab` | Completes the word at the cursor |
| `Up` / `Down` | Walks through previous lines |
| `Left` / `Right`, `Home` / `End` | Moves within the line |
| `Backspace`, `Delete` | Removes either side of the cursor |
| `Ctrl+A` / `Ctrl+E` | Start and end of line |
| `Ctrl+U` | Clears the line |
| `Ctrl+C` | Abandons the line, keeps the shell |
| `Ctrl+D` | Exits on an empty line, deletes forwards otherwise |

### Ctrl+C

At the prompt, Ctrl+C prints `^C`, throws the half-typed line away, and gives a
fresh prompt. The shell does not exit. This works because raw mode turns off the
terminal's signal generation, so Ctrl+C arrives as byte 3 for the editor to act
on rather than as a `SIGINT` that would end the process.

**While a command is running, Ctrl+C still interrupts the shell.** That is
deliberate. Raw mode is held only while a line is being edited, so a command
runs with the terminal exactly as the user's own shell left it. The alternative,
holding raw mode throughout, would make the shell immortal at the cost of making
a long `cp -r` impossible to stop: commands run in-process, not as children, so
there is no child to signal and no cancellation point to check. Between a shell
that cannot be interrupted and one that can, the escape hatch is worth more.

### Auto-completion

The first word of a line completes against this shell's own command names; every
other word completes against the filesystem. One candidate is filled in whole,
with a trailing space, or a `/` when it is a directory so the path can be walked
further. Several candidates fill in as much as they all agree on, and list
themselves when they agree on nothing more. Hidden files stay hidden until the
leading dot is typed.

Completion never consults `$PATH`. This shell runs no external program, so a
name from `$PATH` would complete to something that answers
`Command 'name' not found`.

### History

Up and Down walk through the lines entered this session, keeping the half-typed
line so Down returns to it. Blank lines and immediate repeats are not recorded.
History lives in memory for the session and is not written to a file.

### What the editor does not do

A line longer than the terminal is wide will wrap, and the redraw does not track
the extra rows, so a very long command can leave the display untidy until the
next prompt. Pressing Escape on its own is not resolved until the next keypress,
because a lone escape is indistinguishable from the start of an arrow key
without a timer.

## Deviations from GNU behavior

### `ls` prints one entry per line, always

The real `ls` prints in columns when its output is a terminal and one per line
when it is piped or redirected, which is why `ls | cat` looks different from
`ls`. Columns need the terminal width, which comes from the `TIOCGWINSZ` ioctl.

This was once impossible here and no longer is: `src/terminal/raw.rs` issues
ioctls directly for the line editor, and asking for the width would be a third
one. Staying with one entry per line is now a choice rather than a limit. It is
the form the host tool itself falls back to, so a side by side comparison shows
the same names in the same order and `ls | cat` on the host matches this shell
exactly. Columns would make that comparison depend on both terminals being the
same width.

So an auditor comparing side by side sees the same names in the same order, one
per line instead of in columns. `ls | cat` in the host terminal produces exactly
what `0-shell` produces.

Entries are sorted by raw byte value, which is what the host tool does here and
what GNU `ls` does under `LC_ALL=C`. A GNU `ls` running in a locale with its own
collation rules orders mixed-case names differently: it gives
`a.txt apple Banana`, where byte order gives `Banana a.txt apple`, since every
capital letter sorts before every lowercase one in ASCII.

### What `ls -l` resolves by hand

Two of the long format's columns have no standard library call behind them, and
this project takes no dependencies, so both are worked out from the system's own
files.

Owner and group names come from `/etc/passwd` and `/etc/group`. An id that
appears in neither is printed as the number, which is what `ls` does for an id
whose account is gone. A machine whose accounts live in LDAP or another NSS
source resolves fewer names than the real `ls` would.

Times are local, and the offset comes from the compiled zone file the system
points at: `$TZ` when it names a zone, `/etc/localtime` otherwise. Daylight
saving is honoured, since the file records every changeover. Two limits: a `TZ`
holding rules rather than a zone name (`EET-2EEST,M3.5.0/3`) falls back to UTC,
and a timestamp past the last changeover a zone file lists, which on this system
is in 2037, keeps the last known offset instead of applying the file's trailing
rule.

One more difference worth knowing about, this one against GNU rather than the
host: given a symbolic link whose target is gone, `0-shell` prints the link's
own name, as the host tool does. GNU `ls` reports
`ls: cannot access 'name': No such file or directory` instead.

### Error messages

Messages follow GNU's wording, with two deliberate departures.

The operating system's own text is used for the reason, so it stays correct in
any locale, but Rust's errno suffix is stripped: `No such file or directory`,
not `No such file or directory (os error 2)`.

GNU follows an operand error with `Try 'cp --help' for more information.`. That
line is left out, because this shell has no `--help` to try.

## Comparing against the host terminal

The audit asks for several commands to be compared against the same command in
the computer's own terminal. Worth knowing before doing that: recent Ubuntu
ships [uutils coreutils](https://github.com/uutils/coreutils), a Rust
reimplementation, in place of GNU coreutils. `cat --version` says which is
installed. The two agree on everything the audit compares, but they do not agree
on every error message.

Against uutils coreutils 0.2.2, `0-shell` produces byte-identical output for:

* `echo`, quoted and unquoted, including empty and repeated-space arguments.
* `cat`, for text, dotfiles, several operands at once, and a 200 KB binary file.
* `ls`, `ls -a`, `ls -F`, `ls -l`, `ls -l -a -F`, and `ls -laF`, compared
  through a pipe for the reason given above.
* `ls -l /dev`, including the `major, minor` column for device files, and
  `ls -l` over `/etc`, `/var/log`, and `/usr/bin`, the last being 1612 entries
  of mixed owners, groups, sizes, and dates.

Two error messages differ, and in both the host is the one departing from GNU:
uutils writes `cat: nope.txt: No such file or directory (os error 2)` with the
errno suffix, and `mkdir: subdir: File exists` where GNU and `0-shell` write
`mkdir: cannot create directory 'subdir': File exists`.

## Development checks

```sh
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

The tests are unit tests beside the code they cover, plus one end-to-end test in
`src/shell.rs` that drives the shell through the audit's whole script. Scratch
directories come from the fixture in `src/testing.rs`, which also hands out the
single lock that every test touching the process-wide working directory has to
hold.

Two of `mv`'s tests need a real `EXDEV` from the kernel, which cannot be faked,
so they move a file between the temporary directory and `/dev/shm`. On a machine
where those are the same filesystem the tests skip themselves and say so under
`cargo test -- --nocapture`, rather than passing on the code path they exist to
avoid.
