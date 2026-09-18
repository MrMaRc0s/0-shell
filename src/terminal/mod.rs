//! Reading a line of input, from a person or from a pipe.
//!
//! The shell has two very different callers. A person at a terminal wants the
//! line editing this module provides: Tab completion, history on the arrow
//! keys, and a Ctrl+C that abandons the line instead of the shell. A pipe wants
//! none of it and would be corrupted by the escape sequences that draw it.
//!
//! Both are served through [`LineSource`], so the shell loop is written once.
//! Which one is used is decided at startup by asking whether the input really
//! is a terminal, so a script, a test, and the audit's own piped session all
//! take exactly the path they took before any of this existed.

mod completion;
mod editor;
mod history;
mod keys;
mod raw;

use std::io::{self, BufRead, IsTerminal, Stdin, Write};

use editor::Editor;
use raw::RawMode;

/// How a line of input ended.
pub enum Line {
    /// The user pressed Enter. The line may still be empty.
    Entered(String),
    /// The user pressed Ctrl+C. There is no line, and the shell carries on.
    Interrupted,
    /// Input ended, by Ctrl+D at an empty prompt or by a pipe running dry.
    EndOfInput,
    /// The line could not be decoded as UTF-8 (a piped line), the shell
    /// reports it and keeps reading.
    Invalid,
}

/// Somewhere the shell can ask for its next line.
pub trait LineSource {
    /// Shows `prompt` and reads until the line ends one way or another.
    fn next_line<W>(&mut self, prompt: &str, output: &mut W) -> io::Result<Line>
    where
        W: Write;
}

/// Lines read from a pipe, a file, or a test.
///
/// The prompt is still written, because the audit compares a transcript that
/// contains it and the shell has always behaved this way when piped.
pub struct PipedLines<R> {
    input: R,
}

impl<R> PipedLines<R> {
    pub fn new(input: R) -> Self {
        Self { input }
    }
}

impl<R> LineSource for PipedLines<R>
where
    R: BufRead,
{
    fn next_line<W>(&mut self, prompt: &str, output: &mut W) -> io::Result<Line>
    where
        W: Write,
    {
        write!(output, "{prompt}")?;
        output.flush()?;

        // read_until stops at the byte rather than decoding it, so a line
        // that is not valid UTF-8 is reported instead of ending the shell.
        let mut bytes = Vec::new();
        if self.input.read_until(b'\n', &mut bytes)? == 0 {
            return Ok(Line::EndOfInput);
        }

        match String::from_utf8(bytes) {
            Ok(line) => Ok(Line::Entered(line)),
            Err(_) => Ok(Line::Invalid),
        }
    }
}

/// Lines typed by a person, with editing, history, and completion.
pub struct Terminal {
    editor: Editor,
    input: Stdin,
}

impl Terminal {
    /// Prepares the editor, or reports that this input cannot support one.
    ///
    /// Two questions, because they can disagree. `is_terminal` settles what the
    /// input is attached to, and entering raw mode settles whether the terminal
    /// will actually take the settings the editor depends on. A `None` here is
    /// not a failure: it sends the shell down the plain reading path, where
    /// every required behaviour is identical and only the bonuses are absent.
    pub fn new() -> Option<Self> {
        let input = io::stdin();

        if !input.is_terminal() || !io::stdout().is_terminal() {
            return None;
        }

        // Entered and dropped at once, purely to learn whether it works. The
        // editor takes raw mode again, per line, when it needs it.
        drop(RawMode::enter(&input).ok()?);

        Some(Self {
            editor: Editor::default(),
            input,
        })
    }
}

impl LineSource for Terminal {
    fn next_line<W>(&mut self, prompt: &str, output: &mut W) -> io::Result<Line>
    where
        W: Write,
    {
        self.editor.read_line(prompt, &mut self.input, output)
    }
}
