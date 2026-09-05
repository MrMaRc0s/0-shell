# 0-Shell

`0-shell` is a small Unix-like shell written in Rust. Its required commands are implemented with Rust's standard filesystem and operating-system APIs; it does not launch external command binaries.

## Build and run

```sh
cargo build
./target/debug/0-shell
```

The shell displays `$ `, accepts a command after Enter, and exits cleanly on end-of-input.

The implementation order and two-developer work split are recorded in [`PLAN.md`](PLAN.md).

## `ls` output format

`ls` prints one entry per line, always.

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

One more difference worth knowing about, this one against GNU rather than the
host: given a symbolic link whose target is gone, `0-shell` prints the link's
own name, as the host tool does. GNU `ls` reports
`ls: cannot access 'name': No such file or directory` instead.

## Development checks

```sh
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```
