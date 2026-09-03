//! Shared error reporting for the built-in commands.
//!
//! The wording conventions these helpers implement are documented on
//! `crate::commands::filesystem`. Nothing here writes to the output stream or
//! returns a failing `io::Result` for a failed operation: the `Err` case is
//! reserved for a failure to write to the error stream itself.

use std::io::{self, Write};
use std::path::Path;

/// The operating system's text for an error, without Rust's errno suffix.
///
/// `io::Error` renders an OS error as `No such file or directory (os error 2)`,
/// while Unix tools print only `No such file or directory`. Errors that did not
/// come from the OS have no suffix and are returned unchanged.
pub fn reason(error: &io::Error) -> String {
    let text = error.to_string();

    if error.raw_os_error().is_none() {
        return text;
    }

    match text.rfind(" (os error ") {
        Some(index) if text.ends_with(')') => text[..index].to_string(),
        _ => text,
    }
}

/// Wraps a path in the single quotes that most Unix tools use.
pub fn quoted<P>(path: P) -> String
where
    P: AsRef<Path>,
{
    format!("'{}'", path.as_ref().display())
}

/// Writes `<command>: <text>`.
pub fn message<E>(command: &str, text: &str, errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    writeln!(errors, "{command}: {text}")
}

/// Writes `<command>: <subject>: <reason>` for a fixed reason.
pub fn detail<E>(command: &str, subject: &str, reason: &str, errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    writeln!(errors, "{command}: {subject}: {reason}")
}

/// Writes `<command>: <subject>: <reason>` for an operating-system error.
pub fn io_error<E>(
    command: &str,
    subject: &str,
    error: &io::Error,
    errors: &mut E,
) -> io::Result<()>
where
    E: Write,
{
    detail(command, subject, &reason(error), errors)
}

/// Writes `<command>: missing operand`, used by `mkdir` and `rm`.
pub fn missing_operand<E>(command: &str, errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    message(command, "missing operand", errors)
}

/// Writes `<command>: missing file operand`, used by `cp` and `mv`.
// TODO(task 4-9): drop this attribute once a command calls it.
#[allow(dead_code)]
pub fn missing_file_operand<E>(command: &str, errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    message(command, "missing file operand", errors)
}

/// Writes `<command>: missing destination file operand after '<source>'`.
// TODO(task 4-9): drop this attribute once a command calls it.
#[allow(dead_code)]
pub fn missing_destination_operand<E>(command: &str, source: &str, errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    message(
        command,
        &format!("missing destination file operand after {}", quoted(source)),
        errors,
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;

    use super::{
        detail, io_error, message, missing_destination_operand, missing_file_operand,
        missing_operand, quoted, reason,
    };

    fn not_found() -> io::Error {
        fs::File::open("/zero_shell_missing_path").unwrap_err()
    }

    #[test]
    fn reason_drops_the_rust_errno_suffix() {
        let error = not_found();

        assert_eq!(error.raw_os_error(), Some(2));
        assert!(error.to_string().ends_with("(os error 2)"));
        assert_eq!(reason(&error), "No such file or directory");
    }

    #[test]
    fn reason_keeps_errors_that_did_not_come_from_the_os() {
        let error = io::Error::other("made up");

        assert_eq!(reason(&error), "made up");
    }

    #[test]
    fn quoted_wraps_a_path_in_single_quotes() {
        assert_eq!(
            quoted("new_folder1/new_doc.txt"),
            "'new_folder1/new_doc.txt'"
        );
    }

    #[test]
    fn message_prefixes_the_command_name() {
        let mut errors = Vec::new();

        message("rm", "missing operand", &mut errors).unwrap();

        assert_eq!(String::from_utf8(errors).unwrap(), "rm: missing operand\n");
    }

    #[test]
    fn detail_writes_the_command_subject_and_reason() {
        let mut errors = Vec::new();

        detail("rm", "cannot remove 'adir'", "Is a directory", &mut errors).unwrap();

        assert_eq!(
            String::from_utf8(errors).unwrap(),
            "rm: cannot remove 'adir': Is a directory\n"
        );
    }

    #[test]
    fn io_error_reports_unix_wording_without_the_errno() {
        let mut errors = Vec::new();

        io_error("cat", "nope.txt", &not_found(), &mut errors).unwrap();

        assert_eq!(
            String::from_utf8(errors).unwrap(),
            "cat: nope.txt: No such file or directory\n"
        );
    }

    #[test]
    fn operand_errors_use_the_documented_wording() {
        let mut errors = Vec::new();

        missing_operand("mkdir", &mut errors).unwrap();
        missing_file_operand("cp", &mut errors).unwrap();
        missing_destination_operand("mv", "source.txt", &mut errors).unwrap();

        assert_eq!(
            String::from_utf8(errors).unwrap(),
            "mkdir: missing operand\n\
             cp: missing file operand\n\
             mv: missing destination file operand after 'source.txt'\n"
        );
    }
}
