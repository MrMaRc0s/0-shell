# 0-Shell

`0-shell` is a small Unix-like shell written in Rust. Its required commands are implemented with Rust's standard filesystem and operating-system APIs; it does not launch external command binaries.

## Build and run

```sh
cargo build
./target/debug/0-shell
```

The shell displays `$ `, accepts a command after Enter, and exits cleanly on end-of-input.

The implementation order and two-developer work split are recorded in [`PLAN.md`](PLAN.md).

## Development checks

```sh
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```
