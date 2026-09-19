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
| `cp`    | `-r`, `-R`, `-p` | Copies into an existing directory under the source's basename, or to an exact path. Several sources need a directory destination. Permission bits are always carried over; `-p` additionally carries over the access and modification times. Owner and group are not: that needs `chown`, which has no standard library binding, and without root the kernel would only refuse it. |
| `mv`    | `-i` | Renames, or moves into an existing directory under the source's basename. Files and directories alike, no flag needed. `-i` asks before overwriting an existing destination. |
| `rm`    | `-r`, `-R`, `-f`, `-i` | Refuses a directory without `-r`. `-i` asks before every removal, file by file even inside a recursive tree; `-f` silences it. |
| `ls`    | `-l`, `-a`, `-F`, `-R` | Combinable and clusterable: `ls -l -a -F`, `ls -laF`, and `ls -al` are the same request. On a real terminal the output is coloured the way GNU's `--color=auto` is, and the short format is arranged into columns; piped output stays plain and one name per line. `-R` descends into every subdirectory, heading each one with its path; a symlink to a directory is listed but not walked into. |
| `touch` | `-c`, `-a`, `-m` | Creates a file if missing, otherwise stamps its timestamps to now. `-c` never creates, `-a` and `-m` narrow the stamping to accessed or modified time. GNU `touch`'s `-t`, `-d`, and `-r` are refused as invalid options. |
| `type`  | none | Says whether each name is one of this shell's own commands, the way bash's builtin does for one of its. With no operand it says nothing, which is also what bash does. |
| `which` | none | The same question `type` answers, in the wording the standalone `which` uses. There is no `$PATH` to search and so no path to print: this shell runs no external program, and the honest answer is only ever "one of mine" or "not mine". |
| `export` | none | Sets each `NAME=VALUE`, or a bare `NAME` to the empty string if it has no value yet, as an environment variable — the same one `cd` reads for `$HOME`. Rejects a name that is not a valid identifier. |
| `unset`  | none | Removes each named environment variable. |
| `help`   | none | Lists every command above with its own `--help` text, so the two can never disagree about what a command does. |

Options may appear in any position, `--` ends option parsing, and an unknown
option is refused (`ls: invalid option -- 'x'`) rather than taken for a
filename.

Every command answers `--help`, anywhere on its line, ahead of its own option
parsing — `cp -z --help` prints usage rather than complaining about `-z`. The
text is this shell's own: a short reminder of the flags actually implemented,
not the page GNU's own `--help` prints. It exists mainly so the line GNU
follows a usage mistake with is telling the truth: `cp: missing file
operand`, then `Try 'cp --help' for more information.`.

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

`rm -i` and `mv -i` read their yes-or-no answers the same way: from a file
redirected with `<` when there is one, from the terminal itself when a person
is sitting at one, and, lacking both — a piped, non-interactive run with no
redirection of its own — from nowhere at all. That last case answers every
question no rather than blocking on a standard input the shell's own script
reader already owns, the same answer GNU gives a prompt that runs into end of
input instead of a line.

On a real terminal, `ls` also colours its output the way GNU's `--color=auto`
does: directories in bold blue, executables in bold green, FIFOs in yellow,
sockets and symlinks in their own colours, and arranges the short format into
columns sized to the terminal's width. Piped or redirected output is plain
bytes, one name per line, exactly as before.

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

The first word of a line completes against this shell's own command names. A
later word starting with a dash completes against that command's own flags
instead: `ls -` offers `-l`, `-a`, `-F`, `-R`, and a flag already typed clusters
the rest onto it, so `ls -l` next offers `-la`, `-lF`, `-lR`. Every other word
completes against the filesystem. One candidate is filled in whole, with a
trailing space, or a `/` when it is a directory so the path can be walked
further. Several candidates fill in as much as they all agree on, and list
themselves when they agree on nothing more. Hidden files stay hidden until the
leading dot is typed.

Completion never consults `$PATH`. This shell runs no external program, so a
name from `$PATH` would complete to something that answers
`Command 'name' not found`.

### History

Up and Down walk through the lines entered this session, keeping the half-typed
line so Down returns to it. Blank lines and immediate repeats are not recorded.

History also survives between sessions, in `$HOME/.0-shell_history`, one line
per entry, oldest first — the same shape bash's own history file takes. A new
session reads it in before the first prompt, and every line kept during the
session is appended to it as it is entered, not only rewritten at exit, so a
crash or a `kill` loses nothing already typed. A session with no `$HOME`, or a
history file that cannot be read or written, is not an error: history simply
does not survive it, the same way it already does not survive a piped run.

### Escape, on its own

Pressing Escape on its own sends one byte, indistinguishable from the start of
an arrow key until whatever would have followed either does or does not show
up. Resolving that needs a timer: wait a short while (25ms, long enough that a
real escape sequence — sent in one burst by the terminal — is never split into
two keys) for another byte, and read it as a lone Escape if none arrives. There
is no dedicated wait syscall in `src/terminal/raw.rs`; the wait polls
`FIONREAD`, the same ioctl call used everywhere else in that module, in a short
sleep loop instead of adding a fifth syscall's numbers and argument layout to
get right blind on four systems already.

### Long lines

A line longer than the terminal is wide wraps, the way typed text always has,
and a redraw now tracks the extra rows rather than losing the cursor to them.
Knowing the terminal's width (the same `TIOCGWINSZ` `ls` reads for its own
columns) turns "how many characters" into "which row and column," for both the
whole line and the cursor's own place in it. A redraw walks back up to the
first row of whatever it drew last time, erases from there to the end of the
screen — covering a line that just grew *or* shrank — and draws again, ending
by walking out to the cursor's row and column rather than trusting wherever
the terminal parked the cursor after auto-wrapping the text just written. That
parked position is genuinely ambiguous exactly on a row boundary — still the
last row, one column past the last character, or already the next, empty
one? — which is why the walk starts from a known anchor instead of trusting it.
Without a known width — a terminal this system cannot ask, or none at all —
the line is drawn the way it always was, as a single row.

## Deviations from GNU behavior

### `ls` in columns on a real terminal

The real `ls` prints in columns when its output is a terminal and one per line
when it is piped or redirected, which is why `ls | cat` looks different from
`ls`. Columns need the terminal width, which comes from the `TIOCGWINSZ` ioctl,
issued the same way the three raw-mode ioctls in `src/terminal/raw.rs` already
are.

The short format is arranged the way GNU's own is: as many columns as fit the
width, sized independently, filling down each column before moving on to the
next, and only ever on a real terminal. Piped or redirected output keeps the
one-per-line form regardless, unchanged from before, which is also the form
the host tool itself falls back to there: a side by side comparison through
`ls | cat` still shows the same names in the same order on both. `-l` never
uses columns, on a real terminal or off one, which is also what GNU does.

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
saving is honoured, since the file records every changeover.

A `TZ` holding rules rather than a zone name, `EET-2EEST,M3.5.0/3,M10.5.0/4`,
is read as a self-contained rule once a file by that name fails to turn up —
that string is not a path under `/usr/share/zoneinfo` any zone file lives at.
The same reader also handles what a zone file itself carries past the last
changeover it bothered to record, which on this system is some time in 2037:
every such file from format version 2 onward appends its own trailing rule, a
line just like `TZ` itself can hold, meant to be applied from there on.
`src/commands/filesystem/ls/time/posix.rs` reads the common shape both take —
a month, a week within it, and a day of the week, `Mmonth.week.day` — which is
the only shape a zone file on this system actually uses. The rarer Julian-day
forms POSIX also allows are left unparsed rather than taken on faith, and fall
back to whatever the caller already did without a rule at all: UTC for a `TZ`
naming one directly, the last known offset for a timestamp past a zone file's
final changeover.

A symbolic link named directly, with its target gone, is reported missing:
`ls: cannot access 'name': No such file or directory`, the same wording a
missing name gets. That is GNU's answer. The host tool installed here,
[uutils coreutils](https://github.com/uutils/coreutils), prints the link's own
name instead and calls it done, so a side-by-side comparison against it will
disagree on this one case; matching real GNU is the choice that holds up
against the tool the audit is actually worded around.

### Error messages

Messages follow GNU's wording, with one deliberate departure.

The operating system's own text is used for the reason, so it stays correct in
any locale, but Rust's errno suffix is stripped: `No such file or directory`,
not `No such file or directory (os error 2)`.

GNU follows an operand or option error with `Try 'cp --help' for more
information.`, and so does this shell: every command answers `--help`, so the
line is telling the truth rather than pointing at nothing.

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
