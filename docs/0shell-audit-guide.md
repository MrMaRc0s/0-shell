#### How to use this guide

Each question is followed by a **How to check** block: what to type, and what you should see.

Setup, done once from the project root:

```sh
cargo build --release
mkdir -p /tmp/audit && cd /tmp/audit          # a disposable directory to play in
~/Desktop/0-shell/target/release/0-shell      # start the shell here
```

Open a second terminal (the "host terminal") in the same `/tmp/audit` for the side-by-side comparisons.
The prompt looks like `/tmp/audit $ ` (the current directory, then `$ `).

Note: this machine may ship uutils coreutils instead of GNU. `cat --version` tells you which. Compare `ls` **through a pipe** (`ls | cat`) in the host terminal, because on a real terminal the host arranges names in columns and colours them, while a pipe gives one name per line.

---

#### General

###### Was the project written in Rust?

**How to check**
```sh
cd ~/Desktop/0-shell
cat Cargo.toml          # [package] edition = "2024", [dependencies] is empty
ls src/**/*.rs          # only .rs files
cargo build --release   # builds with cargo alone
```
Expected: every source file is `.rs`, `Cargo.toml` has no dependencies, and the binary is `target/release/0-shell`.

###### Are the commands mentioned in the subject implemented from scratch, without calling any external binaries?

The use of external binaries or system calls that spawn them is strictly forbidden, as the project requires implementing all functionality from scratch without relying on external programs.

**How to check**
```sh
cd ~/Desktop/0-shell
grep -rnE 'std::process|Command::new|\bexec\b|system\(' src
```
Expected: only two hits, `Command::new("mkfifo")` in `src/commands/filesystem/ls/mod.rs` and `src/commands/filesystem/ls/long.rs`. Both are inside `#[cfg(test)]` modules (tests, not compiled into the binary). `src/testing.rs` also uses `std::process::id()` (only reads a PID; test-only).

Also confirm at run time that nothing is spawned. In the host terminal, while the shell is idle, run `pgrep -a 0-shell`, then in the shell run `ls -l` and `cat somefile`. Or run the shell under `strace -f -e trace=execve ./target/release/0-shell` (if strace is installed): the only `execve` is the shell itself, none for `ls`, `cat`, `cp`, and so on.

Also worth mentioning: there are a few `asm!` blocks (`src/terminal/raw.rs`, `src/signal.rs`) that issue raw system calls (`ioctl` for raw mode, `sigaction` for Ctrl+C). These are system calls, not process spawning.

---

#### Functional

##### Open a terminal and run the project.

###### Can you confirm that the project runs and displays a unix shell?

**How to check:** run `./target/release/0-shell`. Expected: a prompt appears (`/current/dir $ `) and the shell waits. Type a few commands (`echo hi`, `pwd`) and see they run.

##### Open a terminal and run the project.

###### Can you confirm that this interpreter displays at least a simple `$` and waits for you to type a command?

**How to check:** start the shell. Expected: the prompt ends in `$ ` and the cursor sits after it, nothing else happens until you type.

##### Try to run a command of your choice.

###### Can you confirm that the interpreter only validates the command if you press enter?

**How to check:** type `pwd` but do **not** press Enter. Expected: the letters appear, but no output. Press Enter: the directory is printed only now. You can also type `echo a`, press Left and Backspace and edit it, and see nothing runs until Enter.

##### Try to run the command `exit`.

###### Can you confirm that the interpreter terminates properly and gives back the parent's shell?

**How to check:** type `exit`. Expected: the shell ends and your parent shell prompt returns. Then run `echo $?` in the parent shell: expected `0`.

##### Try to run the command `echo "something"`. Do the same in your computer terminal.

###### Can you confirm that the displayed message of the project is exactly the same as the computer terminal?

**How to check:** in 0-shell run `echo "something"`, and in the host terminal run `echo "something"`. Expected in both: `something` (quotes are not printed).

##### Try to run the command `echo something else` (without double quotes). Do the same in your computer terminal.

###### Can you confirm that the displayed message of the project is exactly the same as the computer terminal?

**How to check:** run `echo something else` in both. Expected in both: `something else`. Extra edge cases that also match: `echo   spaced   out` gives `spaced out`, `echo "quoted   spaces"` keeps its spaces, and `echo` alone prints an empty line.

##### Try to run the command `pwd`.

###### Did the interpreter display the current working directory?

**How to check:** run `pwd` in 0-shell and in the host terminal from the same directory. Expected: the same absolute path (e.g. `/tmp/audit`).

##### Try to open the project and create a parent directory with two children directories using the command `mkdir`. Then enter the parent directory and do `pwd`.

###### Did the interpreter display the current working directory?

**How to check**
```
mkdir parent
mkdir parent/child1
mkdir parent/child2
ls parent
cd parent
pwd
```
Expected: `ls parent` lists `child1` and `child2`, and `pwd` prints `/tmp/audit/parent`. (Bonus: `mkdir -p a/b/c` creates the whole chain in one go.)

##### Try to enter a directory of your choice by using the command `cd dir/of/your/choice`.

###### Did the interpreter change to the correct path? Use `pwd` to confirm.

**How to check:** `cd /tmp/audit/parent/child1` (or from `/tmp/audit`: `cd parent/child1`), then `pwd`. Expected: the path of that directory. Error cases stay in the shell: `cd /nope` prints `cd: /nope: No such file or directory`.

##### Try to run only the command `cd`.

###### Did the interpreter change to the user's home directory? Use `pwd` to confirm.

**How to check:** run `cd`, then `pwd`. Expected: `/home/steven` (your `$HOME`; compare with `echo $HOME` in the host terminal).

##### Try to run the command `ls` in a directory at your choice. Do the same in your computer terminal.

###### Can you confirm that the output is similar in the project and in your computer terminal?

**How to check:** put a few things in the directory first (from the host terminal: `touch a.txt .hid; mkdir sub; ln -s a.txt lnk; touch s.sh; chmod +x s.sh`). Then run `ls` in 0-shell and `ls | cat` in the host terminal. Expected: same names, same order. On a real terminal 0-shell prints in columns and colours directories like GNU `ls`; through a pipe (`ls | cat` in 0-shell) it is one name per line, like the host.

##### Try to run the command `ls -l -a -F` in a directory at your choice. Do the same in your computer terminal.

###### Can you confirm that the output is similar in the project and in your computer terminal?

**How to check:** run `ls -l -a -F` in 0-shell and `ls -l -a -F | cat` in the host terminal. Expected: same `total` line, permissions, link count, owner, group, size, date, `./` and `../` entries, `/` after directories, `*` after executables. Known tiny difference: for symlinks 0-shell prints `lnk@ -> a.txt` where GNU prints `lnk -> a.txt`. The spellings `ls -laF` and `ls -al` also work. `ls -z` gives `ls: invalid option -- 'z'`.

##### Try to run the commands `mkdir new_folder1` and `mkdir new_folder2` in a directory of your choice.

###### Can you confirm that the directories `new_folder1` and `new_folder2` were created?

**How to check:** run both in 0-shell, then `ls -d new_folder*` in the host terminal (or `ls` in 0-shell). Expected: both directories exist.

##### Using your computer's terminal, create a file named `new_doc.txt` containing some text inside `new_folder1`. Then return to `0-shell` and run `cp new_folder1/new_doc.txt new_folder2`.

###### Can you confirm that `new_doc.txt` was copied into `new_folder2`?

**How to check**
- Host terminal: `echo "some text for the audit" > new_folder1/new_doc.txt`
- 0-shell: `cp new_folder1/new_doc.txt new_folder2`
- Verify: `ls new_folder2` in 0-shell (expected `new_doc.txt`) and `cat new_folder2/new_doc.txt` in the host terminal (expected the same text).

##### Run the command `cat new_folder1/new_doc.txt`. Do the same in your computer terminal.

###### Can you confirm that the project displays the same file contents as the computer terminal?

**How to check:** run `cat new_folder1/new_doc.txt` in both. Expected: identical text. Extra: `cat sub` prints `cat: sub: Is a directory`, `cat nope` prints `cat: nope: No such file or directory`.

##### Run the command `mv new_folder2 new_folder1` to move the directory `new_folder2` into the directory `new_folder1`.

###### Can you confirm that the directory `new_folder2` is now inside `new_folder1`?

**How to check:** run `mv new_folder2 new_folder1`, then `ls new_folder1`. Expected: `new_doc.txt` and `new_folder2`. `ls` at the top level no longer shows `new_folder2`. The moved directory still contains its copied `new_doc.txt` (`ls new_folder1/new_folder2`).

##### Run the command `rm -r new_folder1` to remove the directories and files created above.

###### Can you confirm that the directory `new_folder1` was removed?

**How to check:** run `rm -r new_folder1`, then `ls` in 0-shell and `ls new_folder1` in the host terminal. Expected: it is gone (`No such file or directory`). Safety: `rm new_folder1` on a directory without `-r` is refused (`Is a directory`), and `rm -r /` and `rm -r .` are refused.

##### Try to press `Ctrl+D` while the shell is waiting for input.

###### Did the shell exit gracefully and return control to the parent shell?

**How to check:** at an empty prompt, press `Ctrl+D`. Expected: the shell ends and the parent prompt returns on a fresh line; `echo $?` in the parent gives `0`. (With text already typed, `Ctrl+D` instead deletes forward, like bash. It only exits on an empty line.)

##### Run an unrecognized command, such as `something`.

###### Did the shell print `Command 'something' not found`?

**How to check:** type `something`. Expected exactly: `Command 'something' not found`, and you get a new prompt (the shell does not exit).

---

#### Bonus

###### +Did the student handle `Ctrl + C` gracefully?

**How to check**
1. At an empty prompt press `Ctrl+C`: prints `^C`, new prompt, shell still alive.
2. Type `echo half`, don't press Enter, press `Ctrl+C`: the line is thrown away, nothing runs, new prompt.
3. Interrupt a running command: e.g. `cat` (no operands, waits on input) or a big `cp -r` / `ls -R /`, press `Ctrl+C`: the command stops and the prompt returns. The shell does not exit.

###### +Did the student add auto-completion when writing a command?

**How to check** (press Tab):
- `ec` + Tab becomes `echo `.
- `c` + Tab lists `cat cd clear cp`.
- `ls sub` + Tab (with a `sub` directory) completes to `ls sub/`.
- `ls -` + Tab offers the `ls` flags (`-l -a -F -R`).
- Hidden files only complete after typing the leading dot.

###### +Did the student implement piping?

**How to check**
```
ls -l | cat
echo hello | cat
cat new_doc.txt | cat
echo a ; echo b
ls | cat ; echo done
echo "a|b"
```
Expected: piped output arrives unchanged, `;` runs both commands, and `|` inside quotes stays literal (`a|b`).

###### +Did the student add colors to the errors or directories?

**How to check:** on a real terminal run `ls` in a directory with a subdirectory, an executable and a symlink. Expected: directories in bold blue, executables in bold green, symlinks in cyan. Piped (`ls | cat`) output is plain with no escape codes. Errors are deliberately not coloured, so if the auditor wants colours in errors the answer is no; directories yes.

###### +Did the student implement redirection?

**How to check**
```
echo one > out.txt
echo two >> out.txt
cat out.txt              # one, two
cat < out.txt            # same
ls > listing.txt
> empty.txt              # creates an empty file
echo x > /root/nope      # 0-shell: cannot create '/root/nope': Permission denied (shell survives)
```

###### +Did the student display the current directory in the prompt?

**How to check:** look at the prompt (`/tmp/audit $ `), run `cd sub`, and see it change to `/tmp/audit/sub $ `.

###### +Did the student add any other features or commands to the project?

**How to check**
```
help                       # lists every command with its usage
ls --help                  # every command answers --help
touch f ; touch -c nofile
clear
type ls ; which cp
export NAME=world ; echo "hello $NAME" ; echo ${NAME} ; unset NAME
mkdir -p a/b/c
cp -r a a2 ; rm -ri a2      # answer with y / n
mv -i f g
ls -R
```
Also: Up/Down arrow history (survives restarts in `~/.0-shell_history`), Left/Right/Home/End, `Ctrl+A`, `Ctrl+E`, `Ctrl+U`, and `;` chaining.

---

#### Optional: run the project's own tests

```sh
cargo fmt --check
cargo clippy -- -D warnings
cargo test              # ~330 tests, including one that replays the whole audit script
```
