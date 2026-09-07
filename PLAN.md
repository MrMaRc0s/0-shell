# 0-Shell implementation plan

The work is divided by behavior and source-file ownership so both developers can work on separate branches without repeatedly editing the same files. Git and team communication will handle the final integration.

## Developer A - shell core and session commands

Owned files:

- `src/main.rs`
- `src/shell.rs`
- `src/parser.rs`
- `src/commands/mod.rs`
- `src/commands/session.rs`
- core and session-command tests

Tasks:

1. Build the prompt and read-evaluate loop.
2. Parse command names, whitespace-separated arguments, quotes, and escapes.
3. Handle blank input, errors, Ctrl+D, and unknown commands without crashing.
4. Implement `echo`, `pwd`, `cd`, and `exit`.
5. Route all completed commands to their internal Rust implementations.

This is the best part to start first because it creates a runnable shell early and covers the audit's prompt, input, EOF, unknown-command, navigation, and exit checks.

## Developer B - filesystem commands

Owned files:

- `src/commands/filesystem/mod.rs`
- supporting files below `src/commands/filesystem/`
- filesystem-command tests

Tasks:

1. Implement `mkdir` for one or more paths.
2. Implement `cat` for one or more files.
3. Implement `cp` for file destinations and directory destinations.
4. Implement `mv` for files and directories.
5. Implement `rm`, including recursive removal with `-r`.
6. Implement `ls`, including combinable `-l`, `-a`, and `-F` options.
7. Test the audit's full create, copy, read, move, and remove workflow inside temporary directories.

Developer B can build and unit-test these filesystem functions independently. Developer A will add the small dispatcher registration when the branches are integrated.

## Integration and mandatory audit pass

After both parts are complete:

1. Merge the branches and register the filesystem commands in the dispatcher.
2. Run `cargo fmt --check`, `cargo clippy -- -D warnings`, and `cargo test`.
3. Build the binary and manually perform every mandatory audit step in a disposable directory.
4. Compare `echo`, `cat`, and representative `ls` output with the host shell.
5. Search the source for external process execution and confirm the shell never invokes another binary.
6. Update the README so it describes only completed behavior.

## Bonus backlog

Three of the optional features are implemented, all resting on the same piece of
work: at a terminal the shell reads a line itself instead of letting the kernel do it.

- **Ctrl+C** abandons the line and keeps the shell, at the prompt. During a running
  command it still interrupts, which is a deliberate trade recorded in `README.md`.
- **Auto-completion** on Tab, for command names and for paths.
- **Command history** on the arrow keys, held in memory for the session.

Piped input keeps the old path exactly, so no required behavior is touched.

Still not implemented, and out of scope unless that changes: a current-directory
prompt, colors, command chaining, pipes, redirection, environment-variable
expansion, and `help`. None of them is forbidden by the subject; the constraint
that reads "No piping, no redirection, no globbing" sets the minimum required,
not a ceiling, which is why the same subject offers bonus credit for pipes and
redirection two paragraphs later.
