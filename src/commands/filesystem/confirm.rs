//! The yes/no prompt behind `rm -i` and `mv -i`.
//!
//! Both read the answer from wherever the command's own standard input is:
//! the file the shell opened with `<`, or, failing that, the terminal itself
//! when a person is sitting at one. A piped, non-interactive run with no
//! redirection has no spare standard input to borrow — the shell's own
//! script reader already owns the one there is — so every question in that
//! case is answered no, the way GNU's own prompt answers one that runs into
//! end of input rather than an actual line.

use std::io::{self, BufRead, Write};

/// Whether, and how, a question can be asked.
pub enum Confirm<'a> {
    /// `-i` was not given: nothing is asked, and every question is a yes.
    Unprompted,
    /// `-i` was given, and there is somewhere to read an answer from.
    Asked(&'a mut dyn BufRead),
    /// `-i` was given, but there is nothing to read an answer from. Every
    /// question is a no, and none is even printed, since there is no one
    /// there to read it.
    Unavailable,
}

impl Confirm<'_> {
    /// Writes `prompt`, with no trailing newline of its own, and reads the
    /// line that answers it.
    ///
    /// Anything but a line starting with `y` or `Y` is a no, and so is a
    /// source that runs dry before an answer arrives, the way GNU reads
    /// hitting end of input mid-prompt.
    pub fn ask<E>(&mut self, prompt: &str, errors: &mut E) -> io::Result<bool>
    where
        E: Write,
    {
        let reader = match self {
            Self::Unprompted => return Ok(true),
            Self::Unavailable => return Ok(false),
            Self::Asked(reader) => reader,
        };

        write!(errors, "{prompt}")?;
        errors.flush()?;

        let mut line = String::new();
        let read = reader.read_line(&mut line)?;

        Ok(read > 0 && matches!(line.chars().next(), Some('y' | 'Y')))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::Confirm;

    /// Asks the question against a canned answer, returning whether it read
    /// as yes and what was written to `errors` along the way.
    fn asked(answer: &str) -> (bool, String) {
        let mut reader = Cursor::new(answer.as_bytes().to_vec());
        let mut errors = Vec::new();

        let answered = Confirm::Asked(&mut reader)
            .ask("remove 'x'? ", &mut errors)
            .unwrap();

        (answered, String::from_utf8(errors).unwrap())
    }

    #[test]
    fn a_line_starting_with_y_or_capital_y_is_yes() {
        assert!(asked("y\n").0);
        assert!(asked("Y\n").0);
        assert!(asked("yes\n").0);
    }

    #[test]
    fn anything_else_is_no() {
        assert!(!asked("n\n").0);
        assert!(!asked("\n").0);
        assert!(!asked("no\n").0);
        assert!(!asked(" y\n").0, "leading space is not a bare y");
    }

    #[test]
    fn running_out_of_input_mid_prompt_is_no() {
        assert!(!asked("").0);
    }

    #[test]
    fn the_prompt_is_written_before_the_answer_is_read() {
        let (_, errors) = asked("y\n");

        assert_eq!(errors, "remove 'x'? ");
    }

    #[test]
    fn unprompted_answers_yes_without_writing_anything() {
        let mut errors = Vec::new();

        let answered = Confirm::Unprompted
            .ask("remove 'x'? ", &mut errors)
            .unwrap();

        assert!(answered);
        assert!(errors.is_empty());
    }

    #[test]
    fn unavailable_answers_no_without_writing_anything() {
        let mut errors = Vec::new();

        let answered = Confirm::Unavailable
            .ask("remove 'x'? ", &mut errors)
            .unwrap();

        assert!(!answered);
        assert!(errors.is_empty());
    }
}
