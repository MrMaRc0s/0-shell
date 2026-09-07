use std::fs::File;
use std::io::{self, ErrorKind, Read, Write};

use crate::commands::report;

/// The size of the block moved between the file and the output at a time.
///
/// The file is streamed rather than loaded whole so that `cat` on a very large
/// file costs a fixed amount of memory.
const BLOCK_SIZE: usize = 64 * 1024;

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
pub fn cat<W, E>(args: &[String], output: &mut W, errors: &mut E) -> io::Result<()>
where
    W: Write,
    E: Write,
{
    if args.is_empty() {
        // GNU `cat` with no operands copies standard input, which this shell
        // cannot offer: standard input is the line reader driving the prompt,
        // and consuming it here would eat the next command.
        return report::missing_operand("cat", errors);
    }

    let mut block = vec![0u8; BLOCK_SIZE];

    for path in args {
        match stream(path, &mut block, output) {
            Ok(()) => {}
            Err(Failure::Source(error)) => report::io_error("cat", path, &error, errors)?,
            Err(Failure::Output(error)) => return Err(error),
        }
    }

    Ok(())
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

    loop {
        let read = match file.read(block) {
            Ok(0) => return Ok(()),
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(Failure::Source(error)),
        };

        output.write_all(&block[..read]).map_err(Failure::Output)?;
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{BLOCK_SIZE, cat};
    use crate::testing::TempDir;

    fn write(temp: &TempDir, name: &str, contents: &[u8]) -> String {
        let path = temp.arg(name);
        fs::write(&path, contents).expect("failed to write the test file");
        path
    }

    #[test]
    fn prints_a_file_exactly_as_stored() {
        let temp = TempDir::new("cat_one");
        let path = write(&temp, "new_doc.txt", b"first line\nsecond line\n");
        let mut output = Vec::new();
        let mut errors = Vec::new();

        cat(std::slice::from_ref(&path), &mut output, &mut errors).unwrap();

        assert_eq!(output, b"first line\nsecond line\n");
        assert!(errors.is_empty());
    }

    #[test]
    fn adds_no_newline_of_its_own() {
        let temp = TempDir::new("cat_no_newline");
        let path = write(&temp, "unterminated.txt", b"no trailing newline");
        let mut output = Vec::new();
        let mut errors = Vec::new();

        cat(std::slice::from_ref(&path), &mut output, &mut errors).unwrap();

        assert_eq!(output, b"no trailing newline");
        assert!(errors.is_empty());
    }

    #[test]
    fn prints_an_empty_file_as_nothing() {
        let temp = TempDir::new("cat_empty");
        let path = write(&temp, "empty.txt", b"");
        let mut output = Vec::new();
        let mut errors = Vec::new();

        cat(std::slice::from_ref(&path), &mut output, &mut errors).unwrap();

        assert!(output.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn preserves_bytes_that_are_not_valid_utf8() {
        let temp = TempDir::new("cat_binary");
        let contents = [0x00, 0xff, 0xfe, 0x80, b'\n', 0xc3];
        let path = write(&temp, "binary.bin", &contents);
        let mut output = Vec::new();
        let mut errors = Vec::new();

        cat(std::slice::from_ref(&path), &mut output, &mut errors).unwrap();

        assert_eq!(output, contents);
        assert!(errors.is_empty());
    }

    #[test]
    fn streams_a_file_larger_than_one_block() {
        let temp = TempDir::new("cat_large");
        let contents: Vec<u8> = (0..BLOCK_SIZE * 2 + 17).map(|byte| byte as u8).collect();
        let path = write(&temp, "large.bin", &contents);
        let mut output = Vec::new();
        let mut errors = Vec::new();

        cat(std::slice::from_ref(&path), &mut output, &mut errors).unwrap();

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
        let mut output = Vec::new();
        let mut errors = Vec::new();

        cat(&args, &mut output, &mut errors).unwrap();

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
        let mut output = Vec::new();
        let mut errors = Vec::new();

        cat(&args, &mut output, &mut errors).unwrap();

        assert_eq!(output, b"before\nafter\n");
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            format!("cat: {missing}: No such file or directory\n")
        );
    }

    #[test]
    fn reports_a_directory_instead_of_panicking() {
        let temp = TempDir::new("cat_directory");
        let path = temp.arg("new_folder1");
        fs::create_dir(&path).unwrap();
        let mut output = Vec::new();
        let mut errors = Vec::new();

        cat(std::slice::from_ref(&path), &mut output, &mut errors).unwrap();

        assert!(output.is_empty());
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            format!("cat: {path}: Is a directory\n")
        );
    }

    #[test]
    fn reports_missing_operands() {
        let mut output = Vec::new();
        let mut errors = Vec::new();

        cat(&[], &mut output, &mut errors).unwrap();

        assert!(output.is_empty());
        assert_eq!(String::from_utf8(errors).unwrap(), "cat: missing operand\n");
    }
}
