//! Filesystem commands: `mkdir`, `cat`, `cp`, `mv`, `rm`, and `ls`.
//!
//! Each command lives in its own module and is re-exported here so the
//! dispatcher in `crate::commands` sees one flat namespace.
//!
//! # Error-reporting convention
//!
//! Every command in this module follows the pattern established by
//! `crate::commands::session`:
//!
//! * Failures are written to the `errors` writer, never to `output`.
//! * The return value is `io::Result<()>`; the `Err` case is reserved for a
//!   failure to *write* to the streams themselves, not for a failed operation.
//!   A missing file is a message on `errors` and an `Ok(())`.
//! * A command never aborts the shell and never panics. With several operands
//!   it reports the failing one and continues with the rest, so
//!   `rm a missing b` still removes both `a` and `b`.
//!
//! ## Message shape
//!
//! Messages are `<command>: <detail>` and carry no trailing period. GNU
//! coreutils does not use one single template, so each command matches its own
//! real-world wording rather than a house style:
//!
//! | Command | Template |
//! |---------|----------|
//! | `cat`   | `cat: <path>: <reason>` |
//! | `ls`    | `ls: cannot access '<path>': <reason>` |
//! | `mkdir` | `mkdir: cannot create directory '<path>': <reason>` |
//! | `rm`    | `rm: cannot remove '<path>': <reason>` |
//! | `cp`    | `cp: cannot stat '<src>': <reason>` (source side) |
//! | `cp`    | `cp: cannot create regular file '<dst>': <reason>` (target side) |
//! | `mv`    | `mv: cannot stat '<src>': <reason>` (source side) |
//! | `mv`    | `mv: cannot move '<src>' to '<dst>': <reason>` (target side) |
//!
//! Note that `cat` is the odd one out: it quotes nothing and uses the bare
//! `<command>: <path>: <reason>` form. The others wrap the path in plain
//! single quotes and name the action that failed.
//!
//! Operand-count errors use the GNU wording, without the trailing
//! `Try '<command> --help'` line, since this shell has no `--help`:
//!
//! * `mkdir: missing operand`
//! * `rm: missing operand`
//! * `cp: missing file operand`
//! * `mv: missing file operand`
//! * `cp: missing destination file operand after '<src>'`
//! * `mv: missing destination file operand after '<src>'`
//!
//! Two cases are not `io::Error`s at all and have fixed wording:
//!
//! * `rm: cannot remove '<path>': Is a directory` (no `-r` given)
//! * `cp: -r not specified; omitting directory '<path>'`
//!
//! ## `<reason>` comes from the OS, with the Rust suffix removed
//!
//! `<reason>` is the operating system's own text, so it stays correct in any
//! locale and for errno values we never anticipated. It must *not* be
//! `io::Error`'s `Display` output as-is: Rust appends the raw errno, producing
//! `No such file or directory (os error 2)` where Unix tools print only
//! `No such file or directory`. Every reason is therefore rendered by the
//! shared helper that strips the ` (os error N)` suffix; no command formats an
//! `io::Error` directly with `{}`.

mod cat;
mod cp;
mod ls;
mod mkdir;
mod mv;
mod rm;

#[cfg(test)]
mod testing;

pub use cat::cat;
pub use cp::cp;
pub use ls::ls;
pub use mkdir::mkdir;
pub use mv::mv;
pub use rm::rm;
