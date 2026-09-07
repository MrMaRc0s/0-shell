# 0-Shell

`0-shell` is a small Unix-like shell written in Rust. Every command is
implemented with Rust's standard filesystem and operating-system APIs: the
shell never launches an external binary, and `Cargo.toml` has no dependencies.

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
| `mkdir` | none | One or more directories. No `-p`: each parent has to exist already. |
| `cat`   | none | One or more files, streamed as raw bytes. |
| `cp`    | `-r`, `-R` | Copies into an existing directory under the source's basename, or to an exact path. Several sources need a directory destination. |
| `mv`    | none | Renames, or moves into an existing directory under the source's basename. Files and directories alike, no flag needed. |
| `rm`    | `-r`, `-R`, `-f` | Refuses a directory without `-r`. |
| `ls`    | `-l`, `-a`, `-F` | Combinable and clusterable: `ls -l -a -F`, `ls -laF`, and `ls -al` are the same request. |

Options may appear in any position, `--` ends option parsing, and an unknown
option is refused (`ls: invalid option -- 'x'`) rather than taken for a
filename.

Anything else is reported as `Command 'name' not found`, and the shell carries
on. No command aborts the shell, and a failing operand never stops the ones
after it: `rm present missing other` removes both `present` and `other`.

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

## Deviations from GNU behavior

### `ls` prints one entry per line, always

The real `ls` prints in columns when its output is a terminal and one per line
when it is piped or redirected, which is why `ls | cat` looks different from
`ls`. Choosing the columns would mean knowing the terminal width, and the only
way to ask for it is the `TIOCGWINSZ` ioctl, which needs a C binding this
project does not have: `Cargo.toml` has no dependencies, by the exercise's
rules. Rather than guess a width and print columns that line up with nothing,
`0-shell` prints the form the host tool itself falls back to.

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
`Cargo.toml` has no dependencies, so both are worked out from the system's own
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
