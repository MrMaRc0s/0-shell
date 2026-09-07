# Audit Answers

Answers to every question in [`Audit.md`](Audit.md), in the order asked.

Each answer was checked against the release binary (`cargo build --release`,
then `./target/release/0-shell`) in a disposable directory, side by side with
this computer's terminal.

One thing to know before the comparisons: this machine runs **uutils coreutils
0.2.2**, the Rust reimplementation that recent Ubuntu ships, not GNU coreutils.
`cat --version` reports which is installed. Everything the audit compares
matches it exactly.

---

## General

**Question:** Was the project written in Rust?

**Answer:** Yes. The whole project is Rust: `Cargo.toml`, and 25 `.rs` files
under `src/`. It builds with `cargo build` and needs nothing else. `Cargo.toml`
declares no dependencies at all, and `Cargo.lock` holds exactly one package,
this one, so every behaviour below is written here or comes from the Rust
standard library.

---

**Question:** Are the commands mentioned in the subject implemented from scratch,
without calling any external binaries? (The use of external binaries or system
calls that spawn them is strictly forbidden.)

**Answer:** Yes. No shipped code spawns a process. Every command is built on
`std::fs`, `std::io`, and `std::os::unix`.

To check it yourself, grep the source for `std::process`, `Command`, `exec`, and
`system`. Three matches come back, and none of them is a spawn in the shell:

* `src/testing.rs` uses `std::process::id()`, which reads this process's own PID
  to name a scratch directory. It reads a number; it starts nothing. That file is
  test-only, declared `#[cfg(test)] mod testing;` in `src/main.rs`, so it is not
  compiled into the binary at all.
* `src/commands/filesystem/ls/mod.rs` and `src/commands/filesystem/ls/long.rs`
  each call `Command::new("mkfifo")` **inside `#[cfg(test)]` test modules**.
  Creating a FIFO needs the `mkfifo` system call, which the standard library
  does not expose and which cannot be reached without a C binding this project
  deliberately does not have. Those two tests check that `ls -F` marks a FIFO
  with `|` and that `ls -l` marks it with `p`. Both are written to skip
  themselves where `mkfifo` is unavailable.

So the shipped shell spawns nothing, and the two calls that exist are in tests
that never reach the binary. They are pointed out here rather than left for a
grep to turn up, since the honest answer is "none in the program, two in the
test suite, and here is why".

Worth adding: three things that would normally come from libc are done here
instead, for the same reason. Owner and group names are resolved by parsing
`/etc/passwd` and `/etc/group`. Timestamps are turned into local dates by
reading the system's compiled timezone file, with the calendar arithmetic in
`src/commands/filesystem/ls/time.rs`. And the interactive line editor puts the
terminal into raw mode by issuing the `TCGETS` and `TCSETS` ioctls itself, in
`src/terminal/raw.rs`.

That last one is the only `unsafe` in the project and the only assembly: about
sixty lines holding the Linux system-call instruction for x86_64 and aarch64,
with a documented safety note on each call. It is worth being plain about why it
is there, because a grep for `asm!` will find it. A system call is not a spawned
program: this is the same `ioctl` a C program would reach through libc, and the
subject's own resources point at `man 2` pages for exactly this kind of work.
The forbidden thing would have been to run `stty`, which is the easy way to set
these bits and would have been an external binary. Nothing here creates a
process.

---

## Functional

**Question:** Can you confirm that the project runs and displays a unix shell?

**Answer:** Yes. `./target/release/0-shell` starts, prints its prompt, and reads
commands until told to stop. It runs the required command set (`echo`, `pwd`,
`cd`, `exit`, `ls`, `cat`, `cp`, `mv`, `rm`, `mkdir`), reports anything else as
an unknown command, and keeps going after every error.

---

**Question:** Can you confirm that this interpreter displays at least a simple
`$` and waits for you to type a command?

**Answer:** Yes. The prompt is `$ `, a dollar sign and one space, written and
flushed before each read. Attached to a real terminal the first thing it emits
is exactly `'$ '`, and it then blocks waiting for input.

---

**Question:** Can you confirm that the interpreter only validates the command if
you press enter?

**Answer:** Yes. The shell reads whole lines, so nothing runs until Enter is
pressed. Driven through a real pseudo-terminal, typing `pwd` and waiting
produced only the terminal's own echo of the three characters and no execution:

```
prompt on start:                    '$ '
after typing 'pwd' with no Enter:   'pwd'
after pressing Enter:               '\r\n/tmp\r\n$ '
```

The directory is printed only after Enter.

---

**Question:** Can you confirm that the interpreter terminates properly and gives
back the parent's shell? (`exit`)

**Answer:** Yes. `exit` stops the loop and the process ends with status code 0,
so the parent shell takes over cleanly. Verified over a pseudo-terminal:
`exit` produced `'$ exit\r\n'` and exit code 0.

`exit` takes no arguments. `exit now` reports `exit: too many arguments` and the
shell keeps running, rather than quitting on a command it did not understand.

---

**Question:** Try to run the command `echo "something"`. Do the same in your
computer terminal. Can you confirm that the displayed message of the project is
exactly the same as the computer terminal?

**Answer:** Yes, identical.

```
project: something
host:    something
```

---

**Question:** Try to run the command `echo something else` (without double
quotes). Do the same in your computer terminal. Can you confirm that the
displayed message of the project is exactly the same as the computer terminal?

**Answer:** Yes, identical.

```
project: something else
host:    something else
```

Both forms were also compared for the awkward cases: `echo` with no arguments
(one blank line), `echo   spaced   out` (repeated spaces collapse, since the
shell splits on whitespace before `echo` ever sees the words), and
`echo "quoted   spaces"` (spaces preserved inside quotes). All identical to the
host.

---

**Question:** Try to run the command `pwd`. Did the interpreter display the
current working directory?

**Answer:** Yes. It prints the absolute path of the working directory, matching
the host `pwd` exactly. `pwd` takes no arguments; `pwd extra` reports
`pwd: too many arguments`.

---

**Question:** Try to open the project and create a parent directory with two
children directories using the command `mkdir`. Then enter the parent directory
and do `pwd`. Did the interpreter display the current working directory?

**Answer:** Yes. The session, run in a scratch directory:

```
$ mkdir parent
$ mkdir parent/child1
$ mkdir parent/child2
$ ls parent
child1
child2
$ cd parent
$ pwd
/tmp/.../play/parent
```

Both children were created and `pwd` reported the new directory correctly.

Note that `mkdir` has no `-p` here, so each parent has to exist before its
child. That is why the three commands above are given separately, which is also
how the audit words the step. `mkdir a/b/c` with no `a` reports
`mkdir: cannot create directory 'a/b/c': No such file or directory`.

---

**Question:** Try to enter a directory of your choice by using the command
`cd dir/of/your/choice`. Did the interpreter change to the correct path? Use
`pwd` to confirm.

**Answer:** Yes. `cd parent/child1` followed by `pwd` printed
`/tmp/.../play/parent/child1`, the same path the host shell reports for that
directory.

A `cd` that cannot succeed reports and changes nothing: `cd /nope` gives
`cd: /nope: No such file or directory`, and `cd afile` gives
`cd: afile: Not a directory`.

---

**Question:** Try to run only the command `cd`. Did the interpreter change to the
user's home directory? Use `pwd` to confirm.

**Answer:** Yes. Bare `cd` moved to `/home/steven`, which is this account's
`$HOME`, confirmed with `pwd` immediately afterwards. If `HOME` is not set the
shell reports `cd: HOME not set` rather than guessing.

---

**Question:** Try to run the command `ls` in a directory at your choice. Do the
same in your computer terminal. Can you confirm that the output is similar in the
project and in your computer terminal?

**Answer:** Yes, and byte for byte rather than merely similar, with one
presentation difference worth stating plainly.

**`0-shell` prints one entry per line, always.** The real `ls` prints in columns
when its output is a terminal and one per line when it is piped, which is why
`ls | cat` looks different from `ls`. Choosing columns means knowing the
terminal width, and the only way to ask is the `TIOCGWINSZ` ioctl, which needs a
C binding this project has no dependency for. Rather than guess a width and
print columns that line up with nothing, the shell prints the form the host tool
itself falls back to. So `ls | cat` in your terminal gives exactly what
`0-shell` gives.

Compared that way, on a directory holding a file, a log, a symlink, an
executable, a dotfile, and a subdirectory:

```
a.txt
b.log
lnk
s.sh
sub
```

Identical to the host, same names in the same order. Sorting is by raw byte
value, which is what the host does here and what GNU `ls` does under `LC_ALL=C`.

---

**Question:** Try to run the command `ls -l -a -F` in a directory at your choice.
Do the same in your computer terminal. Can you confirm that the output is similar
in the project and in your computer terminal?

**Answer:** Yes, byte for byte identical. Same directory as above:

```
total 16
drwxrwxr-x 3 steven steven 160 Sep  7 20:27 ./
drwxrwxr-x 4 steven steven 120 Sep  7 20:27 ../
-rw-rw-r-- 1 steven steven   7 Sep  7 20:27 .hid
-rw-rw-r-- 1 steven steven   6 Sep  7 20:27 a.txt
-rw-r----- 1 steven steven   9 Sep  7 20:27 b.log
lrwxrwxrwx 1 steven steven   5 Sep  7 20:27 lnk@ -> a.txt
-rwxr-xr-x 1 steven steven  10 Sep  7 20:27 s.sh*
drwxrwxr-x 2 steven steven  40 Sep  7 20:27 sub/
```

All three flags are doing their work: `-a` shows `.`, `..`, and `.hid`; `-F`
appends `/` to directories, `*` to executables, and `@` to symlinks; `-l` gives
the permissions, link count, owner, group, size, date, and the `-> a.txt` link
target, with the `total` header and every column aligned to its widest value.

The flags combine in any spelling. `ls -l -a -F`, `ls -laF`, `ls -al`, and
`ls -aFl` all produce the same output, and an unknown flag is refused with
`ls: invalid option -- 'x'` rather than treated as a filename.

`ls -l` was checked well past this small directory, all byte identical to the
host: `/dev`, which exercises the `major, minor` column for device files;
`/etc`; `/var/log`; and `/usr/bin`, which is 1612 entries of mixed owners,
groups, sizes, and dates, including files old enough to print `Mon DD  YYYY`
instead of `Mon DD HH:MM`.

---

**Question:** Try to run the commands `mkdir new_folder1` and `mkdir new_folder2`
in a directory of your choice. Can you confirm that the directories `new_folder1`
and `new_folder2` were created?

**Answer:** Yes. Both appear immediately afterwards, confirmed from the host
terminal:

```
$ mkdir new_folder1
$ mkdir new_folder2
-> after mkdir: new_folder1
                new_folder2
```

---

**Question:** Using your computer's terminal, create a file named `new_doc.txt`
containing some text inside `new_folder1`. Then return to `0-shell` and run
`cp new_folder1/new_doc.txt new_folder2`. Can you confirm that `new_doc.txt` was
copied into `new_folder2`?

**Answer:** Yes. With the file created from the host terminal, the copy landed
inside the destination directory under its own basename:

```
$ cp new_folder1/new_doc.txt new_folder2
$ ls new_folder2
new_doc.txt
```

This is the case where the destination is an existing directory, so the file is
copied *into* it rather than over it. Permissions are carried across.
`cp` also handles a plain target path, several sources into one directory, and
`-r` (or `-R`) for whole directory trees.

---

**Question:** Run the command `cat new_folder1/new_doc.txt`. Do the same in your
computer terminal. Can you confirm that the project displays the same file
contents as the computer terminal?

**Answer:** Yes, byte for byte.

```
$ cat new_folder1/new_doc.txt
some text for the audit
```

The host prints the same. `cat` copies raw bytes and never passes the contents
through a string type, so a file that is not valid UTF-8 survives intact. This
was checked against the host on text files, a dotfile, several files in one
command, and a 200 KB binary file, all identical.

A directory argument is reported, not a crash: `cat sub` gives
`cat: sub: Is a directory`.

---

**Question:** Run the command `mv new_folder2 new_folder1` to move the directory
`new_folder2` into the directory `new_folder1`. Can you confirm that the
directory `new_folder2` is now inside `new_folder1`?

**Answer:** Yes.

```
$ mv new_folder2 new_folder1
$ ls new_folder1
new_doc.txt
new_folder2
```

`new_folder1` now holds both the original file and the moved directory. Because
the destination is an existing directory, the source moved into it keeping its
own name, giving `new_folder1/new_folder2`.

`mv` needs no flag for a directory: a rename moves a whole tree in one step.
Where the destination is on a different filesystem a rename is impossible, so
the move falls back to copying the tree and then removing the source, and the
source is deleted only once the copy has fully landed.

---

**Question:** Run the command `rm -r new_folder1` to remove the directories and
files created above. Can you confirm that the directory `new_folder1` was
removed?

**Answer:** Yes. `new_folder1` and everything inside it, including the moved
`new_folder2`, are gone. The `ls` straight afterwards no longer lists it, and
from the host terminal the directory does not exist and `cat` on the file inside
it reports the file is gone.

Without `-r`, a directory is refused rather than removed:
`rm: cannot remove 'new_folder1': Is a directory`. `rm` with no operands is an
error, never a silent no-op, and `rm -r /` and `rm -r .` are refused outright.

---

**Question:** Try to press `Ctrl+D` while the shell is waiting for input. Did the
shell exit gracefully and return control to the parent shell?

**Answer:** Yes. Tested by sending a real Ctrl+D keypress over a
pseudo-terminal, both at an empty prompt and after running a command:

```
Ctrl+D at an empty prompt:  output '$ \r\n',            exit code 0
Ctrl+D after a command:     output '$ pwd\r\n/tmp\r\n$ \r\n', exit code 0
```

End of input ends the loop. The shell moves to a fresh line so the parent's
prompt does not land beside its own, and exits with status 0.

---

**Question:** Run an unrecognized command, such as `something`. Did the shell
print `Command 'something' not found`?

**Answer:** Yes, exactly that:

```
Command 'something' not found
```

The message goes to standard error, and the shell carries on to the next
command rather than exiting.

---

## Bonus

**Question:** Did the student handle `Ctrl + C` gracefully?

**Answer:** Yes, at the prompt, which is where it matters. Pressing Ctrl+C
while typing prints `^C`, throws the half-typed line away, and returns a fresh
prompt. The shell does not exit.

Verified over a pseudo-terminal, checking the process itself rather than the
screen, since a terminal echoes typed characters even after the program behind
it has died:

```
Ctrl+C at an empty prompt         -> shell alive
Ctrl+C with a half-typed line     -> shell alive
Ctrl+C while a command is running -> shell exits (signal 2)
```

The third line is a deliberate limit and is stated rather than hidden. The
shell takes the terminal into raw mode only while a line is being edited, which
is what stops the kernel turning Ctrl+C into a `SIGINT`; a command then runs
with the terminal as the user's own shell left it. Holding raw mode throughout
would make the shell survive that case too, at the price of making a long
`cp -r` impossible to stop, because commands here run in-process rather than as
child processes: there is no child to signal and no cancellation point to poll.
Between a shell that cannot be interrupted and one that can, the escape hatch
was judged worth more.

No external program is involved. Terminal modes are set with the `TCSETS`
ioctl, issued directly in `src/terminal/raw.rs`; `stty` would have been the easy
way and is exactly what the subject forbids.

---

**Question:** Did the student add auto-completion when writing a command?

**Answer:** Yes. Tab completes the word the cursor is in.

The first word of a line is a command, so it completes against this shell's own
command names. Every other word is a path, so it completes against the
filesystem. A single candidate is filled in whole and finished with a space, or
with a `/` when it names a directory so the path can be walked further. Several
candidates fill in as much as they all agree on, and list themselves when they
agree on nothing further. Hidden files stay hidden until the leading dot is
typed, and completion works mid-line, not only at the end of one.

Observed at a real terminal:

```
ec      + Tab  ->  echo
c       + Tab  ->  lists: cat  cd  cp
ls pro  + Tab  ->  ls project/
ls al   + Tab  ->  lists: album.txt  alpha.txt
```

Completion never consults `$PATH`. This shell runs no external program, so
offering a name from `$PATH` would be offering a command that answers
`Command 'name' not found`.

---

**Question:** Did the student implement piping?

**Answer:** No. Not implemented, and out of scope per `PLAN.md`. Each line is one
command; `|` is passed through as an ordinary argument.

---

**Question:** Did the student add colors to the errors or directories?

**Answer:** No. Output is uncoloured. This was a deliberate choice as well as a
scope one: the audit compares output against the host terminal, and colour codes
would show up as differences in that comparison.

---

**Question:** Did the student implement redirection?

**Answer:** No. Not implemented, and out of scope per `PLAN.md`. `>` and `<` are
ordinary arguments.

---

**Question:** Did the student display the current directory in the prompt?

**Answer:** No. The prompt is the plain `$ ` the subject asks for, kept that way
so it matches what the audit expects to see.

---

**Question:** Did the student add any other features or commands to the project?

**Answer:** No extra commands, but two further bonuses from the subject's list
are implemented, and the required commands go beyond the minimum in several
places.

**Command history.** Up and Down walk through the lines entered this session.
The half-typed line is kept when the walk starts, so Down returns to it. Blank
lines and immediate repeats are not recorded. History is held in memory for the
session and not written to a file.

**A line editor.** Left and Right, Home and End (also Ctrl+A and Ctrl+E),
Backspace and Delete, and Ctrl+U to clear the line. Ctrl+D ends the shell only
on an empty line and deletes forwards otherwise, as it does in bash, so a
session cannot end by surprise mid-edit. Characters outside ASCII are decoded
from UTF-8 and edit as single characters rather than as bytes.

All of this applies only when the input really is a terminal. Piped or
redirected input takes the plain reading path, so scripts, the test suite, and
the audit's own transcript are byte for byte what they were before any of it
existed. Two known limits: a line longer than the terminal is wide will wrap and
untidy the redraw, and a lone Escape keypress is not resolved until the next
key, being indistinguishable from the start of an arrow key without a timer.

The required commands also go beyond the minimum:

* **Option parsing shared by every command.** Flags work in any position, cluster
  (`ls -laF`), tolerate repeats, and stop at `--`, so a file genuinely named
  `-r` is reachable as `mv -- -r target`. An unknown flag is refused
  (`ls: invalid option -- 'x'`) instead of being taken for a filename.
* **`cp -r` and `-R`,** including the real `cp` symlink rule: a plain copy
  follows a link and copies what it points at, a recursive copy recreates the
  link. Special files are reported and skipped rather than half copied.
* **`rm -f`,** and two failsafes: `rm -r /` is refused even when spelled `//` or
  reached through a symlink, and `rm -r .` or `..` is refused.
* **`mv` across filesystems,** falling back to copy-then-remove when a rename
  cannot cross a mount point, and leaving the source untouched if the copy does
  not complete.
* **`ls -l` written without libc or a date library**, resolving owner and group
  from `/etc/passwd` and `/etc/group` and computing local dates from the
  system's timezone file, daylight saving included.
* **Errors that never stop the shell.** No command panics or exits on bad input,
  and a failing operand does not stop the others: `rm present missing other`
  removes both `present` and `other`. This was tested deliberately against
  missing operands, nonexistent paths, permission-denied paths, a file where a
  directory belongs, unclosed quotes, very long input, and binary files.
* **168 automated tests**, including one that drives the shell through this
  audit's entire script and checks the result.
