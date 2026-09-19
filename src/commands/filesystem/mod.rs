//! Filesystem commands: `mkdir`, `cat`, `cp`, `mv`, `rm`, `ls`, and `touch`.
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
//! | `ls`    | `ls: cannot access '<path>': <reason>` (an operand) |
//! | `ls`    | `ls: cannot open directory '<path>': <reason>` (its contents) |
//! | `mkdir` | `mkdir: cannot create directory '<path>': <reason>` |
//! | `rm`    | `rm: cannot remove '<path>': <reason>` |
//! | `cp`    | `cp: cannot stat '<src>': <reason>` (source side) |
//! | `cp`    | `cp: cannot create regular file '<dst>': <reason>` (target side) |
//! | `cp`    | `cp: cannot create directory '<dst>': <reason>` (target side, `-r`) |
//! | `mv`    | `mv: cannot stat '<src>': <reason>` (source side) |
//! | `mv`    | `mv: cannot move '<src>' to '<dst>': <reason>` (target side) |
//! | `touch` | `touch: cannot touch '<path>': <reason>` |
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
//! * `touch: missing file operand`
//! * `cp: missing destination file operand after '<src>'`
//! * `mv: missing destination file operand after '<src>'`
//!
//! Some cases are not `io::Error`s at all and have fixed wording:
//!
//! * `rm: cannot remove '<path>': Is a directory` (no `-r` given)
//! * `rm: refusing to remove '.' or '..' directory: skipping '<path>'`
//! * `rm: it is dangerous to operate recursively on '<path>'` (the root failsafe)
//! * `cp: -r not specified; omitting directory '<path>'`
//! * `cp: omitting special file '<src>'` (a device, FIFO, or socket)
//! * `mv: cannot move '<src>' to a subdirectory of itself, '<dst>'`
//!
//! `-i`'s prompts are fixed wording too, written without a trailing newline
//! since an answer follows on the same line:
//!
//! * `rm: remove regular file '<path>'? ` (a plain file, `regular empty file`
//!   when it has nothing in it)
//! * `rm: remove symbolic link '<path>'? `
//! * `rm: remove directory '<path>'? ` (once its contents are already gone)
//! * `mv: overwrite '<dst>'? ` (only when the destination already exists)
//!
//! `cp` and `mv` share their transfer code, so they share these, each naming
//! itself and its own verb (`copy` or `move`):
//!
//! * `<command>: target '<dst>': Not a directory` (several sources, one plain
//!   target)
//! * `<command>: '<src>' and '<dst>' are the same file`
//! * `<command>: cannot <verb> a directory, '<src>', into itself, '<dst>'`
//! * `<command>: cannot <verb> '<src>' into '<dst>': Invalid argument` (a
//!   source with no basename to join, such as `..`)
//!
//! `mv` reaches the copy and removal messages by a second route. A rename
//! cannot cross a filesystem boundary, and the fallback for that runs the same
//! code `cp -r` and `rm -r` do, so a cross-device move can report
//! `mv: cannot create regular file '<dst>': <reason>` or
//! `mv: cannot remove '<src>': <reason>`. GNU's `mv` shares its transfer code
//! the same way and reports the same shapes.
//!
//! Unknown flags use the wording `getopt` produces, again without the
//! `Try '<command> --help'` line:
//!
//! * `cp: invalid option -- 'q'`
//! * `cp: unrecognized option '--bogus'`
//!
//! `mv` accepts no options at all, but still parses them, so that `-q` is
//! refused as an option rather than taken for a filename.
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
mod confirm;
mod cp;
mod ls;
mod mkdir;
mod mv;
mod options;
mod rm;
mod touch;
mod transfer;

pub use cat::cat;
pub use cp::cp;
pub use ls::ls;
pub use mkdir::mkdir;
pub use mv::mv;
pub use rm::rm;
pub use touch::touch;
