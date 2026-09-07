use std::io::{self, BufRead, Write};

use crate::commands::{CommandAction, dispatch};
use crate::parser::parse;

pub fn run() -> io::Result<()> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut output = io::stdout().lock();
    let mut errors = io::stderr().lock();

    run_with_io(&mut input, &mut output, &mut errors)
}

fn run_with_io<R, W, E>(input: &mut R, output: &mut W, errors: &mut E) -> io::Result<()>
where
    R: BufRead,
    W: Write,
    E: Write,
{
    let mut line = String::new();

    loop {
        write!(output, "$ ")?;
        output.flush()?;

        line.clear();
        if input.read_line(&mut line)? == 0 {
            writeln!(output)?;
            return Ok(());
        }

        match parse(&line) {
            Ok(Some(command)) => {
                if dispatch(&command, output, errors)? == CommandAction::Exit {
                    return Ok(());
                }
            }
            Ok(None) => {}
            Err(error) => writeln!(errors, "0-shell: {error}")?,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Cursor;

    use super::run_with_io;
    use crate::testing::{TempDir, WorkingDirectory};

    /// Feeds a script to the shell and returns what each stream received.
    fn script(input: &str) -> (String, String) {
        let mut input = Cursor::new(input.as_bytes().to_vec());
        let mut output = Vec::new();
        let mut errors = Vec::new();

        run_with_io(&mut input, &mut output, &mut errors).unwrap();

        (
            String::from_utf8(output).unwrap(),
            String::from_utf8(errors).unwrap(),
        )
    }

    #[test]
    fn exits_cleanly_at_end_of_input() {
        let (output, errors) = script("");

        assert_eq!(output, "$ \n");
        assert!(errors.is_empty());
    }

    #[test]
    fn dispatches_only_after_reading_a_complete_line() {
        let (output, errors) = script("missing first\nmissing second\n");

        assert_eq!(output, "$ $ $ \n");
        assert_eq!(
            errors,
            "Command 'missing' not found\nCommand 'missing' not found\n"
        );
    }

    #[test]
    fn echo_handles_quoted_and_unquoted_arguments() {
        let (output, errors) = script("echo \"something!\"\necho something else\n");

        assert_eq!(output, "$ something!\n$ something else\n$ \n");
        assert!(errors.is_empty());
    }

    #[test]
    fn exit_stops_before_later_commands_are_read() {
        let (output, errors) = script("echo before\nexit\necho after\n");

        assert_eq!(output, "$ before\n$ ");
        assert!(errors.is_empty());
    }

    #[test]
    fn runs_the_audit_script_from_start_to_finish() {
        // The one test that types what the auditor types. Everything is
        // relative, as it is at a real prompt, so the working directory is
        // borrowed for the duration rather than sidestepped with absolute
        // paths the way the command tests do.
        let temp = TempDir::new("shell_audit");
        let _directory = WorkingDirectory::enter(temp.path());

        // "Try to run the commands `mkdir new_folder1` and `mkdir new_folder2`
        // in a directory of your choice."
        let (output, errors) = script("mkdir new_folder1\nmkdir new_folder2\nls\n");

        assert_eq!(output, "$ $ $ new_folder1\nnew_folder2\n$ \n");
        assert!(errors.is_empty(), "{errors}");

        // "Using your computer's terminal, create a file named `new_doc.txt`
        // containing some text inside `new_folder1`." The audit hands this step
        // to the host, so the test does it directly rather than through the
        // shell being tested.
        fs::write("new_folder1/new_doc.txt", b"some text\n").unwrap();

        // The rest of the script, in the order the audit asks for it: copy into
        // a directory, read the file back, move a directory into a directory,
        // then remove everything that was made.
        let (output, errors) = script(
            "cp new_folder1/new_doc.txt new_folder2\n\
             cat new_folder1/new_doc.txt\n\
             mv new_folder2 new_folder1\n\
             ls new_folder1\n\
             rm -r new_folder1\n\
             ls\n",
        );

        assert_eq!(
            output,
            // `cp` and `mv` say nothing when they succeed, `cat` prints the
            // text the host wrote, and `ls new_folder1` shows the copy's source
            // beside the directory that was just moved in.
            "$ $ some text\n$ $ new_doc.txt\nnew_folder2\n$ $ $ \n"
        );
        assert!(errors.is_empty(), "{errors}");

        // "Can you confirm that the directory `new_folder1` was removed?"
        assert_eq!(
            fs::read_dir(".").unwrap().count(),
            0,
            "the script left something behind"
        );
    }
}
