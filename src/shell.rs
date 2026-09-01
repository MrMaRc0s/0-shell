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
    use std::io::Cursor;

    use super::run_with_io;

    #[test]
    fn exits_cleanly_at_end_of_input() {
        let mut input = Cursor::new(Vec::<u8>::new());
        let mut output = Vec::new();
        let mut errors = Vec::new();

        run_with_io(&mut input, &mut output, &mut errors).unwrap();

        assert_eq!(String::from_utf8(output).unwrap(), "$ \n");
        assert!(errors.is_empty());
    }

    #[test]
    fn dispatches_only_after_reading_a_complete_line() {
        let mut input = Cursor::new(b"missing first\nmissing second\n".to_vec());
        let mut output = Vec::new();
        let mut errors = Vec::new();

        run_with_io(&mut input, &mut output, &mut errors).unwrap();

        assert_eq!(String::from_utf8(output).unwrap(), "$ $ $ \n");
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            "Command 'missing' not found\nCommand 'missing' not found\n"
        );
    }

    #[test]
    fn echo_handles_quoted_and_unquoted_arguments() {
        let mut input = Cursor::new(b"echo \"something!\"\necho something else\n".to_vec());
        let mut output = Vec::new();
        let mut errors = Vec::new();

        run_with_io(&mut input, &mut output, &mut errors).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "$ something!\n$ something else\n$ \n"
        );
        assert!(errors.is_empty());
    }
}
