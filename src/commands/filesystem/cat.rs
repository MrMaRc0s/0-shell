use std::fs::File;
use std::io::{self, ErrorKind, Read, Write};

use crate::commands::report;
use crate::signal;

/// The size of the block moved between the file and the output at a time.
///
/// The file is streamed rather than loaded whole so that `cat` on a very large
/// file costs a fixed amount of memory.
const BLOCK_SIZE: usize = 64 * 1024;

/// What `-` stands for in an error about standard input.
const STANDARD_INPUT: &str = "(standard input)";

/// Which side of the copy failed.
///
/// A failure to read the source is reported on `errors` and the remaining
/// operands are still printed. A failure to write to the output is the shell's
/// own stream breaking, so it propagates as the `Err` case that
/// `crate::commands::filesystem` reserves for exactly that.
enum Failure {
    Source(io::Error),
    Output(io::Error),
}

/// Writes the contents of each named file to the output, in the order given.
///
/// Contents are copied as raw bytes, never through `String`: a file may hold
/// any byte sequence, and the output has to match the host `cat` byte for byte.
/// Nothing is added between files, so `cat a b` produces exactly the bytes of
/// `a` followed by the bytes of `b`.
///
/// An operand that cannot be read is reported and the rest are still printed,
/// so `cat present missing other` prints both `present` and `other`.
///
/// `input` is standard input when the shell redirected it with `<`. With no
/// operands, `cat` copies standard input, as GNU's does; the `-` operand means
/// the same. Without a redirection there is no standard input to copy, so the
/// operand-less form keeps the missing-operand error it always had.
pub fn cat<W, E>(
    args: &[String],
    mut input: Option<&mut dyn Read>,
    output: &mut W,
    errors: &mut E,
) -> io::Result<()>
where
    W: Write,
    E: Write,
{
    if args.is_empty() {
        return match input {
            Some(reader) => stream_from(reader, output, errors),
            None => report::missing_operand("cat", errors),
        };
    }

    let mut block = vec![0u8; BLOCK_SIZE];

    for path in args {
        // A Ctrl+C answers for the whole command, not just the file being read:
        // once it arrives, the files that would follow are not opened either.
        if signal::is_pending() {
            break;
        }

        if path == "-"
            && let Some(reader) = input.as_deref_mut()
        {
            match copy(reader, &mut block, output) {
                Ok(()) => {}
                Err(Failure::Source(error)) => {
                    report::io_error("cat", STANDARD_INPUT, &error, errors)?
                }
                Err(Failure::Output(error)) => return Err(error),
            }
            continue;
        }

        match stream(path, &mut block, output) {
            Ok(()) => {}
            Err(Failure::Source(error)) => report::io_error("cat", path, &error, errors)?,
            Err(Failure::Output(error)) => return Err(error),
        }
    }

    Ok(())
}

/// Copies a byte stream to the output, reporting read failures but never
/// giving up on them.
fn stream_from<W, E>(input: &mut dyn Read, output: &mut W, errors: &mut E) -> io::Result<()>
where
    W: Write,
    E: Write,
{
    let mut block = vec![0u8; BLOCK_SIZE];

    match copy(input, &mut block, output) {
        Ok(()) => Ok(()),
        Err(Failure::Source(error)) => report::io_error("cat", STANDARD_INPUT, &error, errors),
        Err(Failure::Output(error)) => Err(error),
    }
}

/// Copies one file to the output through `block`.
fn stream<W>(path: &str, block: &mut [u8], output: &mut W) -> Result<(), Failure>
where
    W: Write,
{
    let mut file = File::open(path).map_err(Failure::Source)?;

    // A directory opens successfully on Unix and only fails on the first read,
    // with wording that varies by platform. Checking the handle we already hold
    // gives the one message the audit expects, and gives it before any byte of
    // output is written.
    let metadata = file.metadata().map_err(Failure::Source)?;
    if metadata.is_dir() {
        return Err(Failure::Source(io::Error::other("Is a directory")));
    }

    copy(&mut file, block, output)
}

/// Reads `block`-sized chunks from `source` and writes them to `output`.
fn copy<W>(source: &mut dyn Read, block: &mut [u8], output: &mut W) -> Result<(), Failure>
where
    W: Write,
{
    loop {
        // The one loop that streams the most, so the one Ctrl+C must be able to
        // stop mid-block. The flag is read without being consumed, so the rest
        // of `cat` — and then the shell — still learn about the interrupt.
        if signal::is_pending() {
            return Ok(());
        }

        let read = match source.read(block) {
            Ok(0) => return Ok(()),
            Ok(read) => read,
            // The handler does not restart system calls, so a blocked read
            // wakes as `EINTR`. That is the interrupt asking to get out: stop
            // if it is recorded, otherwise it was noise and the read goes on.
            Err(error) if error.kind() == ErrorKind::Interrupted => {
                if signal::is_pending() {
                    return Ok(());
                }
                continue;
            }
            Err(error) => return Err(Failure::Source(error)),
        };

        let mut written = 0;
        while written < read {
            match output.write(&block[written..read]) {
                Ok(progress) => written += progress,
                // `write_all` would swallow this error and keep retrying, and
                // a `cat` blocked writing to a busy terminal would never
                // return to the loop-top check above. The interrupted write is
                // therefore answered here instead.
                Err(error) if error.kind() == ErrorKind::Interrupted => {
                    if signal::is_pending() {
                        return Ok(());
                    }
                }
                Err(error) => return Err(Failure::Output(error)),
            }
            if signal::is_pending() {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Cursor;

    use super::{BLOCK_SIZE, cat};
    use crate::testing::TempDir;

    fn write(temp: &TempDir, name: &str, contents: &[u8]) -> String {
        let path = temp.arg(name);
        fs::write(&path, contents).expect("failed to write the test file");
        path
    }

    fn run(paths: &[String], input: Option<&mut dyn std::io::Read>) -> (Vec<u8>, String) {
        let mut output = Vec::new();
        let mut errors = Vec::new();

        cat(paths, input, &mut output, &mut errors).unwrap();

        (output, String::from_utf8(errors).unwrap())
    }

    #[test]
    fn prints_a_file_exactly_as_stored() {
        let temp = TempDir::new("cat_one");
        let path = write(&temp, "new_doc.txt", b"first line\nsecond line\n");

        let (output, errors) = run(std::slice::from_ref(&path), None);

        assert_eq!(output, b"first line\nsecond line\n");
        assert!(errors.is_empty());
    }

    #[test]
    fn adds_no_newline_of_its_own() {
        let temp = TempDir::new("cat_no_newline");
        let path = write(&temp, "unterminated.txt", b"no trailing newline");

        let (output, errors) = run(std::slice::from_ref(&path), None);

        assert_eq!(output, b"no trailing newline");
        assert!(errors.is_empty());
    }

    #[test]
    fn prints_an_empty_file_as_nothing() {
        let temp = TempDir::new("cat_empty");
        let path = write(&temp, "empty.txt", b"");

        let (output, errors) = run(std::slice::from_ref(&path), None);

        assert!(output.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn preserves_bytes_that_are_not_valid_utf8() {
        let temp = TempDir::new("cat_binary");
        let contents = [0x00, 0xff, 0xfe, 0x80, b'\n', 0xc3];
        let path = write(&temp, "binary.bin", &contents);

        let (output, errors) = run(std::slice::from_ref(&path), None);

        assert_eq!(output, contents);
        assert!(errors.is_empty());
    }

    #[test]
    fn streams_a_file_larger_than_one_block() {
        let temp = TempDir::new("cat_large");
        let contents: Vec<u8> = (0..BLOCK_SIZE * 2 + 17).map(|byte| byte as u8).collect();
        let path = write(&temp, "large.bin", &contents);

        let (output, errors) = run(std::slice::from_ref(&path), None);

        assert_eq!(output, contents);
        assert!(errors.is_empty());
    }

    #[test]
    fn joins_several_files_in_order_without_a_separator() {
        let temp = TempDir::new("cat_many");
        let args = vec![
            write(&temp, "first.txt", b"first"),
            write(&temp, "second.txt", b"second\n"),
        ];

        let (output, errors) = run(&args, None);

        assert_eq!(output, b"firstsecond\n");
        assert!(errors.is_empty());
    }

    #[test]
    fn reports_a_missing_file_and_prints_the_rest() {
        let temp = TempDir::new("cat_missing");
        let missing = temp.arg("nope.txt");
        let args = vec![
            write(&temp, "before.txt", b"before\n"),
            missing.clone(),
            write(&temp, "after.txt", b"after\n"),
        ];

        let (output, errors) = run(&args, None);

        assert_eq!(output, b"before\nafter\n");
        assert_eq!(
            errors,
            format!("cat: {missing}: No such file or directory\n")
        );
    }

    #[test]
    fn reports_a_directory_instead_of_panicking() {
        let temp = TempDir::new("cat_directory");
        let path = temp.arg("new_folder1");
        fs::create_dir(&path).unwrap();

        let (output, errors) = run(std::slice::from_ref(&path), None);

        assert!(output.is_empty());
        assert_eq!(errors, format!("cat: {path}: Is a directory\n"));
    }

    #[test]
    fn reports_missing_operands() {
        let (output, errors) = run(&[], None);

        assert!(output.is_empty());
        assert_eq!(
            errors,
            "cat: missing operand\nTry 'cat --help' for more information.\n"
        );
    }

    #[test]
    fn copies_standard_input_byte_for_byte_when_redirected() {
        let mut input = Cursor::new(b"from <\nsecond line".to_vec());

        let (output, errors) = run(&[], Some(&mut input));

        assert_eq!(output, b"from <\nsecond line");
        assert!(errors.is_empty());
    }

    #[test]
    fn a_dash_operand_reads_redirected_standard_input() {
        let mut input = Cursor::new(b"piped".to_vec());
        let args = vec![
            "before.txt".to_string(),
            "-".to_string(),
            "after.txt".to_string(),
        ];

        // Only the `-` operand reaches the redirected stream; the other two
        // names fail to open and are reported around it.
        let mut input = Some(&mut input);
        let input = input
            .as_deref_mut()
            .map(|reader| reader as &mut dyn std::io::Read);
        let (output, errors) = run(&args, input);

        assert_eq!(output, b"piped");
        assert_eq!(
            errors,
            "cat: before.txt: No such file or directory\n\
             cat: after.txt: No such file or directory\n"
        );
    }
}
