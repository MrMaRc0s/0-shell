use std::env;
use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, IsTerminal, Write};

use crate::commands::report;
use crate::commands::{CommandAction, dispatch};
use crate::parser::{Direction, ParsedCommand, parse};
use crate::signal;
use crate::terminal::{self, Line, LineSource, PipedLines, Terminal};

/// What the shell shows when it wants a command: the current directory, so a
/// person never has to run `pwd` just to see where they are, followed by the
/// same `$ ` this shell has always prompted with. A directory that cannot be
/// read — removed out from under the shell, say — falls back to `$ ` alone
/// rather than showing a stale or empty path.
fn prompt() -> String {
    match env::current_dir() {
        Ok(path) => format!("{} $ ", path.display()),
        Err(_) => "$ ".to_string(),
    }
}

/// A `Write` that remembers the last byte it produced.
///
/// The person's editor draws each prompt by walking the cursor back across the
/// line, a redraw that would erase whatever a command left there. Knowing the
/// last byte sent tells the shell whether the cursor is mid-line — the rest of
/// a command's output that omitted its closing newline — and so whether the
/// prompt has to move to a fresh line before it dares to redraw.
pub struct LastByte<W> {
    inner: W,
    last_byte: u8,
}

impl<W> LastByte<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            last_byte: b'\n',
        }
    }

    /// The last byte written, or `\n` when nothing has been written yet.
    pub fn last_byte(&self) -> u8 {
        self.last_byte
    }
}

impl<W: Write> Write for LastByte<W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buffer)?;
        if written > 0 {
            self.last_byte = buffer[written - 1];
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

pub fn run() -> io::Result<()> {
    // A Ctrl+C while a command runs arrives as a signal, not as the byte the
    // prompt's raw mode turns it into. The handler installed here records it
    // instead of letting it end the process, and the commands stop early when
    // they see it. Failure to install keeps the old behaviour: an interrupt
    // simply ends the shell.
    match signal::install() {
        Ok(()) => {}
        Err(error) => eprintln!("0-shell: signal handler failed to install: {error}"),
    }

    let mut errors = io::stderr().lock();

    // A person gets the line editor; anything else gets plain line reading.
    // Deciding here rather than inside the loop keeps the piped path exactly
    // what it always was, which is the path the audit script takes.
    let colored = io::stdout().is_terminal();
    let width = terminal::width();

    if let Some(mut terminal) = Terminal::new() {
        let stdout = io::stdout();
        let mut output = LastByte::new(stdout.lock());
        return run_loop(
            &mut terminal,
            &mut output,
            &mut errors,
            colored,
            width,
            true,
        );
    }

    let stdin = io::stdin();
    let mut piped = PipedLines::new(stdin.lock());
    let stdout = io::stdout();
    let mut output = LastByte::new(stdout.lock());

    run_loop(&mut piped, &mut output, &mut errors, colored, width, false)
}

/// The read, parse, dispatch loop, whatever the lines are coming from.
///
/// `colored` is whether standard output is a terminal. It lets `ls` match the
/// host tool, which also colors names only when it is writing to a person.
/// `width` is that terminal's width in columns, which the same `ls` reads to
/// arrange its short format into columns rather than one name per line.
/// `interactive_terminal` is the same "is a person there" question asked of
/// standard input: it lets `rm -i` and `mv -i` know a person is there to read
/// an unredirected question from, rather than the shell's own script reader
/// owning the only standard input there is.
fn run_loop<S, W, E>(
    source: &mut S,
    output: &mut LastByte<W>,
    errors: &mut E,
    colored: bool,
    width: Option<usize>,
    interactive_terminal: bool,
) -> io::Result<()>
where
    S: LineSource,
    W: Write,
    E: Write,
{
    loop {
        // The person's editor draws the prompt by walking the cursor back to
        // the start of the line and erasing it, a move that would ride over any
        // command output still sitting there because it ended without a
        // newline. Get off that line first, so the output stays on the screen.
        if source.needs_fresh_line() && output.last_byte() != b'\n' {
            write!(output, "\r\n")?;
        }

        match source.next_line(&prompt(), output)? {
            Line::EndOfInput => {
                writeln!(output)?;
                return Ok(());
            }
            // Ctrl+C: the line is abandoned, nothing runs, and the shell asks
            // again. This is the whole of the bonus, and it is this short only
            // because raw mode stopped the kernel turning it into a signal.
            Line::Interrupted => {}
            // A piped line that could not be decoded; keep reading.
            Line::Invalid => writeln!(errors, "0-shell: line is not valid UTF-8")?,
            Line::Entered(line) => {
                // A Ctrl+C from the previous command is stale before this one
                // starts, and must not be allowed to stop it. The flag is
                // cleared once here rather than per stream, and whatever the
                // command does with it is settled after dispatch.
                signal::reset();

                match parse(&line) {
                    Ok(Some(command)) => {
                        // The output and input files are opened before the
                        // command runs, because a redirection that cannot be
                        // opened has to stop the command rather than run it
                        // against nothing.
                        let (output_file, input_file) = match open_redirections(&command) {
                            Ok(redirects) => redirects,
                            Err(redirect) => {
                                writeln!(errors, "0-shell: {}", redirect.message())?;
                                continue;
                            }
                        };

                        // A redirection with no command at all (`> file`) still
                        // performs the redirection, as POSIX requires: the file
                        // was opened above and is now dropped. There is nothing
                        // to run.
                        if command.name.is_empty() {
                            continue;
                        }

                        // The colour decision belongs to this command's stream:
                        // a `> file` redirect sends it to a file, and files
                        // never get colors, whatever the terminal behind the
                        // shell is.
                        let colored = colored && output_file.is_none();

                        let mut input = input_file.map(BufReader::new);
                        let input = input.as_mut().map(|file| file as &mut dyn io::BufRead);

                        let action = match output_file {
                            Some(mut file) => dispatch(
                                &command,
                                input,
                                &mut file,
                                errors,
                                colored,
                                width,
                                interactive_terminal,
                            )?,
                            None => dispatch(
                                &command,
                                input,
                                output,
                                errors,
                                colored,
                                width,
                                interactive_terminal,
                            )?,
                        };

                        // A Ctrl+C that reached a command's own loops is left
                        // standing here so it is not carried into the next
                        // line. Commands read the flag without consuming it, so
                        // this is the one place the signal is put to rest.
                        signal::take();

                        if action == CommandAction::Exit {
                            return Ok(());
                        }
                    }
                    Ok(None) => {}
                    Err(error) => writeln!(errors, "0-shell: {error}")?,
                }
            }
        }
    }
}

/// A redirection that could not be opened.
enum RedirectError {
    /// `>` or `>>` failed: the file exists but cannot be created or written.
    Output { path: String, error: io::Error },
    /// `<` failed: the file cannot be opened for reading.
    Input { path: String, error: io::Error },
}

impl RedirectError {
    /// The shell's report, in the same `<action> '<path>': <reason>` shape the
    /// commands use.
    fn message(&self) -> String {
        match self {
            Self::Output { path, error } => {
                format!("cannot create '{}': {}", path, report::reason(error))
            }
            Self::Input { path, error } => format!(
                "cannot open '{}' for reading: {}",
                path,
                report::reason(error)
            ),
        }
    }
}

/// Opens the files a line's redirections name.
///
/// Several redirections to the same stream are resolved left to right, the
/// last one winning, which is what a real shell does: `> a > b` writes `b`
/// and leaves `a` truncated and empty. Each file is opened as it is reached,
/// so an earlier redirection's side effect happens even when a later one wins.
fn open_redirections(
    command: &ParsedCommand,
) -> Result<(Option<File>, Option<File>), RedirectError> {
    let mut output = None;
    let mut input = None;

    for redirection in &command.redirections {
        match redirection.direction {
            Direction::Truncate | Direction::Append => {
                let file =
                    open_output(&redirection.path, redirection.direction).map_err(|error| {
                        RedirectError::Output {
                            path: redirection.path.clone(),
                            error,
                        }
                    })?;
                output = Some(file);
            }
            Direction::Read => {
                let file = open_input(&redirection.path).map_err(|error| RedirectError::Input {
                    path: redirection.path.clone(),
                    error,
                })?;
                input = Some(file);
            }
        }
    }

    Ok((output, input))
}

/// `>` creates or empties the file; `>>` creates or extends it.
fn open_output(path: &str, direction: Direction) -> io::Result<File> {
    match direction {
        Direction::Truncate => File::create(path),
        _ => OpenOptions::new().append(true).create(true).open(path),
    }
}

/// `<` opens an existing file for reading. A directory is refused here, with
/// the wording `cat` would otherwise have reached on its first read.
fn open_input(path: &str) -> io::Result<File> {
    let file = File::open(path)?;
    if file.metadata()?.is_dir() {
        return Err(io::Error::other("Is a directory"));
    }
    Ok(file)
}

/// Drives the loop over one reader, for the tests and the audit transcript.
#[cfg(test)]
fn run_with_io<R, W, E>(input: &mut R, output: &mut W, errors: &mut E) -> io::Result<()>
where
    R: std::io::BufRead,
    W: Write,
    E: Write,
{
    let mut piped = PipedLines::new(input);
    let mut output = LastByte::new(output);

    run_loop(&mut piped, &mut output, errors, false, None, false)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{self, Cursor, Write};

    use super::{LastByte, run_loop, run_with_io};
    use crate::terminal::{Line, LineSource};
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
    fn last_byte_remembers_the_most_recent_byte_written() {
        let mut output = LastByte::new(Vec::new());

        assert_eq!(
            output.last_byte(),
            b'\n',
            "a fresh stream reads as a full line"
        );

        write!(output, "test").unwrap();
        assert_eq!(output.last_byte(), b't');
        writeln!(output).unwrap();
        assert_eq!(output.last_byte(), b'\n');

        assert_eq!(String::from_utf8(output.inner).unwrap(), "test\n");
    }

    #[test]
    fn the_editor_path_moves_past_output_that_ended_mid_line() {
        struct Drawing {
            first: bool,
        }
        impl LineSource for Drawing {
            fn next_line<W>(&mut self, prompt: &str, output: &mut W) -> io::Result<Line>
            where
                W: Write,
            {
                write!(output, "{prompt}")?;
                if std::mem::take(&mut self.first) {
                    Ok(Line::Entered(String::new()))
                } else {
                    Ok(Line::EndOfInput)
                }
            }

            fn needs_fresh_line(&self) -> bool {
                true
            }
        }

        // Held, not entered: the prompt below has to match whatever this
        // process's actual working directory is, and nothing may change it
        // while that is being read.
        let _directory = WorkingDirectory::hold();
        let prompt = super::prompt();
        let mut output = Vec::new();
        let mut errors = Vec::new();
        let mut source = Drawing { first: true };

        run_loop(
            &mut source,
            &mut LastByte::new(&mut output),
            &mut errors,
            false,
            None,
            false,
        )
        .unwrap();

        // The first prompt stands in for the tail of a command's output that
        // did not end with a newline, so the second redraw moves down a row
        // first rather than drawing over it.
        assert_eq!(output, format!("{prompt}\r\n{prompt}\n").into_bytes());
    }

    #[test]
    fn a_plain_source_keeps_the_prompt_in_place() {
        struct Plain;
        impl LineSource for Plain {
            fn next_line<W>(&mut self, prompt: &str, output: &mut W) -> io::Result<Line>
            where
                W: Write,
            {
                write!(output, "{prompt}")?;
                Ok(Line::EndOfInput)
            }
        }

        let _directory = WorkingDirectory::hold();
        let prompt = super::prompt();
        let mut output = Vec::new();
        let mut errors = Vec::new();

        run_loop(
            &mut Plain,
            &mut LastByte::new(&mut output),
            &mut errors,
            false,
            None,
            false,
        )
        .unwrap();

        assert_eq!(output, format!("{prompt}\n").into_bytes());
    }

    #[test]
    fn exits_cleanly_at_end_of_input() {
        let _directory = WorkingDirectory::hold();
        let prompt = super::prompt();

        let (output, errors) = script("");

        assert_eq!(output, format!("{prompt}\n"));
        assert!(errors.is_empty());
    }

    #[test]
    fn dispatches_only_after_reading_a_complete_line() {
        let _directory = WorkingDirectory::hold();
        let prompt = super::prompt();

        let (output, errors) = script("missing first\nmissing second\n");

        assert_eq!(output, format!("{prompt}{prompt}{prompt}\n"));
        assert_eq!(
            errors,
            "Command 'missing' not found\nCommand 'missing' not found\n"
        );
    }

    #[test]
    fn echo_handles_quoted_and_unquoted_arguments() {
        let _directory = WorkingDirectory::hold();
        let prompt = super::prompt();

        let (output, errors) = script("echo \"something!\"\necho something else\n");

        assert_eq!(
            output,
            format!("{prompt}something!\n{prompt}something else\n{prompt}\n")
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn exit_stops_before_later_commands_are_read() {
        let _directory = WorkingDirectory::hold();
        let prompt = super::prompt();

        let (output, errors) = script("echo before\nexit\necho after\n");

        assert_eq!(output, format!("{prompt}before\n{prompt}"));
        assert!(errors.is_empty());
    }

    #[test]
    fn an_invalid_utf8_line_is_handled_and_later_lines_still_run() {
        let _directory = WorkingDirectory::hold();
        let prompt = super::prompt();

        let mut input = Cursor::new(vec![
            0xff, b'\n', b'e', b'c', b'h', b'o', b' ', b'h', b'i', b'\n',
        ]);
        let mut output = Vec::new();
        let mut errors = Vec::new();

        run_with_io(&mut input, &mut output, &mut errors).unwrap();

        assert_eq!(
            output,
            format!("{prompt}{prompt}hi\n{prompt}\n").into_bytes()
        );
        assert_eq!(errors, b"0-shell: line is not valid UTF-8\n");
    }

    #[test]
    fn runs_the_audit_script_from_start_to_finish() {
        // The one test that types what the auditor types. Everything is
        // relative, as it is at a real prompt, so the working directory is
        // borrowed for the duration rather than sidestepped with absolute
        // paths the way the command tests do.
        let temp = TempDir::new("shell_audit");
        let _directory = WorkingDirectory::enter(temp.path());
        let prompt = super::prompt();

        // "Try to run the commands `mkdir new_folder1` and `mkdir new_folder2`
        // in a directory of your choice."
        let (output, errors) = script("mkdir new_folder1\nmkdir new_folder2\nls\n");

        assert_eq!(
            output,
            format!("{prompt}{prompt}{prompt}new_folder1\nnew_folder2\n{prompt}\n")
        );
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
            format!(
                "{prompt}{prompt}some text\n{prompt}{prompt}new_doc.txt\nnew_folder2\n\
                 {prompt}{prompt}{prompt}\n"
            )
        );
        assert!(errors.is_empty(), "{errors}");

        // "Can you confirm that the directory `new_folder1` was removed?"
        assert_eq!(
            fs::read_dir(".").unwrap().count(),
            0,
            "the script left something behind"
        );
    }

    #[test]
    fn a_truncate_redirection_writes_the_commands_output_to_a_file() {
        let temp = TempDir::new("shell_redirect_out");
        let _directory = WorkingDirectory::enter(temp.path());
        let prompt = super::prompt();

        let (output, errors) = script("echo hello > note.txt\nls\n");

        assert_eq!(
            output,
            format!("{prompt}{prompt}note.txt\n{prompt}\n"),
            "prompts and the listing only"
        );
        assert!(errors.is_empty(), "{errors}");
        assert_eq!(fs::read("note.txt").unwrap(), b"hello\n");
    }

    #[test]
    fn an_append_redirection_adds_to_the_file() {
        let temp = TempDir::new("shell_redirect_append");
        let _directory = WorkingDirectory::enter(temp.path());
        let prompt = super::prompt();

        let (output, errors) = script("echo one >> log\ncat log\necho two >> log\ncat log\n");

        assert_eq!(
            output,
            format!("{prompt}{prompt}one\n{prompt}{prompt}one\ntwo\n{prompt}\n")
        );
        assert!(errors.is_empty(), "{errors}");
        assert_eq!(fs::read("log").unwrap(), b"one\ntwo\n");
    }

    #[test]
    fn a_read_redirection_feeds_cat_standard_input() {
        let temp = TempDir::new("shell_redirect_in");
        let _directory = WorkingDirectory::enter(temp.path());
        let prompt = super::prompt();

        let (output, errors) = script("echo seed > in.txt\ncat < in.txt\n");

        assert_eq!(output, format!("{prompt}{prompt}seed\n{prompt}\n"));
        assert!(errors.is_empty(), "{errors}");
    }

    #[test]
    fn cat_marks_standard_input_when_there_is_no_redirection() {
        let temp = TempDir::new("shell_cat_stdin");
        let _directory = WorkingDirectory::enter(temp.path());
        let prompt = super::prompt();

        let (output, errors) = script("cat\ncat -\n");

        assert_eq!(output, format!("{prompt}{prompt}{prompt}\n"));
        assert_eq!(
            errors,
            "cat: missing operand\n\
             Try 'cat --help' for more information.\n\
             cat: -: No such file or directory\n"
        );
    }

    #[test]
    fn a_redirection_that_cannot_be_opened_stops_the_command() {
        let temp = TempDir::new("shell_redirect_fail");
        let _directory = WorkingDirectory::enter(temp.path());
        let prompt = super::prompt();

        let (output, errors) = script("echo hi > nowhere/nope\necho after\n");

        assert_eq!(
            output,
            format!("{prompt}{prompt}after\n{prompt}\n"),
            "the second command still ran"
        );
        assert!(
            errors.starts_with("0-shell: cannot create 'nowhere/nope': "),
            "{errors}"
        );
    }

    #[test]
    fn a_redirection_alone_still_creates_the_file() {
        let temp = TempDir::new("shell_redirect_bare");
        let _directory = WorkingDirectory::enter(temp.path());
        let prompt = super::prompt();

        let (output, errors) = script("> empty\nls\ncat empty\n");

        assert_eq!(output, format!("{prompt}{prompt}empty\n{prompt}{prompt}\n"));
        assert!(errors.is_empty(), "{errors}");
        assert!(fs::read("empty").unwrap().is_empty());
    }

    #[test]
    fn the_last_redirection_to_a_stream_wins() {
        let temp = TempDir::new("shell_redirect_last");
        let _directory = WorkingDirectory::enter(temp.path());
        let prompt = super::prompt();

        let (output, errors) = script("echo only > first > final\ncat final\n");
        // `first` was created and truncated, then `final` received the output.
        assert_eq!(output, format!("{prompt}{prompt}only\n{prompt}\n"));
        assert!(errors.is_empty(), "{errors}");
        assert!(fs::read("first").unwrap().is_empty());
        assert_eq!(fs::read("final").unwrap(), b"only\n");
    }
}
