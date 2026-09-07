//! The line editor: one line of input, built a keystroke at a time.
//!
//! This is what replaces the kernel's line discipline once raw mode is on.
//! Everything the terminal used to do for us has to be done here instead:
//! showing the characters as they are typed, moving the cursor, erasing, and
//! deciding what Enter, Ctrl+C, and Ctrl+D mean.

use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;

use super::Line;
use super::completion::complete;
use super::history::History;
use super::keys::{Key, read_key};
use super::raw::RawMode;

/// A line editor, and the history it remembers between lines.
#[derive(Default)]
pub struct Editor {
    history: History,
}

impl Editor {
    /// Reads one line, editing it in place until Enter, Ctrl+C, or Ctrl+D.
    ///
    /// Raw mode lasts only as long as this call. A command therefore runs with
    /// the terminal exactly as the user's shell left it, and an unexpected exit
    /// while a command is running cannot strand the terminal in raw mode.
    pub fn read_line<R, W>(
        &mut self,
        prompt: &str,
        input: &mut R,
        output: &mut W,
    ) -> io::Result<Line>
    where
        R: Read + AsRawFd,
        W: Write,
    {
        let _raw = RawMode::enter(input)?;

        let mut buffer: Vec<char> = Vec::new();
        let mut cursor = 0usize;

        render(prompt, &buffer, cursor, output)?;

        loop {
            let Some(key) = read_key(input)? else {
                return Ok(Line::EndOfInput);
            };

            match key {
                Key::Enter => {
                    write!(output, "\r\n")?;
                    output.flush()?;

                    let line: String = buffer.iter().collect();
                    self.history.remember(&line);

                    return Ok(Line::Entered(line));
                }

                // Ctrl+C abandons the line and asks for another. The `^C` is
                // there so the transcript shows why the line was dropped, which
                // is what every other shell does.
                Key::Interrupt => {
                    write!(output, "^C\r\n")?;
                    output.flush()?;
                    self.history.rewind();

                    return Ok(Line::Interrupted);
                }

                // Ctrl+D ends the shell only on an empty line. With text to the
                // right of the cursor it deletes forwards, as it does in bash,
                // so it cannot end a session by surprise mid-edit.
                Key::EndOfFile => {
                    if buffer.is_empty() {
                        return Ok(Line::EndOfInput);
                    }
                    if cursor < buffer.len() {
                        buffer.remove(cursor);
                    }
                }

                Key::Char(character) => {
                    buffer.insert(cursor, character);
                    cursor += 1;
                }

                Key::Backspace => {
                    if cursor > 0 {
                        cursor -= 1;
                        buffer.remove(cursor);
                    }
                }

                Key::Delete => {
                    if cursor < buffer.len() {
                        buffer.remove(cursor);
                    }
                }

                Key::Left => cursor = cursor.saturating_sub(1),
                Key::Right => cursor = (cursor + 1).min(buffer.len()),
                Key::Home => cursor = 0,
                Key::End => cursor = buffer.len(),

                Key::KillLine => {
                    buffer.clear();
                    cursor = 0;
                }

                Key::Up | Key::Down => {
                    let current: String = buffer.iter().collect();
                    let entry = match key {
                        Key::Up => self.history.previous(&current),
                        _ => self.history.next(),
                    };

                    // Collected before the loop continues so the borrow of the
                    // history ends here rather than fighting the assignment.
                    if let Some(entry) = entry.map(|entry| entry.chars().collect::<Vec<char>>()) {
                        buffer = entry;
                        cursor = buffer.len();
                    }
                }

                Key::Tab => {
                    cursor = expand(&mut buffer, cursor, output)?;
                }

                Key::Ignored => {}
            }

            render(prompt, &buffer, cursor, output)?;
        }
    }
}

/// Acts on Tab, returning where the cursor ends up.
///
/// One candidate is taken whole. Several are taken as far as they agree, and if
/// that settles nothing new the choices are listed instead, which is what
/// pressing Tab twice does elsewhere, without the editor having to notice that
/// it was pressed twice.
fn expand<W>(buffer: &mut Vec<char>, cursor: usize, output: &mut W) -> io::Result<usize>
where
    W: Write,
{
    let completion = complete(buffer, cursor);
    let original: String = buffer[completion.start..cursor].iter().collect();

    if completion.word != original {
        let replacement: Vec<char> = completion.word.chars().collect();
        let length = replacement.len();

        buffer.splice(completion.start..cursor, replacement);

        return Ok(completion.start + length);
    }

    if completion.matches.len() > 1 {
        // The candidates carry the space or slash that would be typed after
        // them. The slash belongs in a listing because it says "directory";
        // the trailing space is only punctuation and is dropped.
        let listed: Vec<&str> = completion
            .matches
            .iter()
            .map(|candidate| candidate.trim_end_matches(' '))
            .collect();

        write!(output, "\r\n{}\r\n", listed.join("  "))?;
    }

    Ok(cursor)
}

/// Draws the prompt and the line, and puts the cursor where it belongs.
///
/// Carriage return moves to the start of the line, the text is written, and
/// `ESC [ K` erases whatever the previous, longer line left behind. The cursor
/// is then walked back out from column zero, because that is a count this code
/// knows exactly and the terminal's own idea of the position is not.
///
/// A line longer than the terminal is wide will wrap and confuse this, since
/// there is no attempt to track the extra rows. Commands at a shell prompt are
/// rarely that long, and the fix costs a `TIOCGWINSZ` and a redraw that has to
/// count rows.
fn render<W>(prompt: &str, buffer: &[char], cursor: usize, output: &mut W) -> io::Result<()>
where
    W: Write,
{
    let text: String = buffer.iter().collect();

    write!(output, "\r{prompt}{text}\x1b[K\r")?;

    let columns = prompt.chars().count() + cursor;
    if columns > 0 {
        write!(output, "\x1b[{columns}C")?;
    }

    output.flush()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{expand, render};
    use crate::testing::{TempDir, WorkingDirectory};

    fn buffer(text: &str) -> Vec<char> {
        text.chars().collect()
    }

    fn text(buffer: &[char]) -> String {
        buffer.iter().collect()
    }

    #[test]
    fn tab_takes_a_single_candidate_whole() {
        let mut line = buffer("ec");
        let mut output = Vec::new();

        let cursor = expand(&mut line, 2, &mut output).unwrap();

        assert_eq!(text(&line), "echo ");
        assert_eq!(cursor, 5, "the cursor follows the text in");
        assert!(output.is_empty(), "one candidate needs no listing");
    }

    #[test]
    fn tab_lists_the_choices_when_it_cannot_settle_them() {
        let mut line = buffer("c");
        let mut output = Vec::new();

        let cursor = expand(&mut line, 1, &mut output).unwrap();

        assert_eq!(text(&line), "c", "the line is left alone");
        assert_eq!(cursor, 1);

        // Listed without the trailing space each candidate carries.
        assert_eq!(String::from_utf8(output).unwrap(), "\r\ncat  cd  cp\r\n");
    }

    #[test]
    fn tab_says_nothing_when_there_is_nothing_to_say() {
        let mut line = buffer("zzz");
        let mut output = Vec::new();

        let cursor = expand(&mut line, 3, &mut output).unwrap();

        assert_eq!(text(&line), "zzz");
        assert_eq!(cursor, 3);
        assert!(output.is_empty());
    }

    #[test]
    fn tab_completes_in_the_middle_of_a_line() {
        let temp = TempDir::new("editor_tab_middle");
        let _directory = WorkingDirectory::enter(temp.path());
        fs::write("alpha.txt", b"a").unwrap();

        let mut line = buffer("cat al tail");
        let mut output = Vec::new();

        // The cursor sits after `al`, with ` tail` still to its right.
        let cursor = expand(&mut line, 6, &mut output).unwrap();

        assert_eq!(text(&line), "cat alpha.txt  tail");
        assert_eq!(cursor, 14, "the cursor lands after the completion");
    }

    #[test]
    fn rendering_draws_the_line_and_places_the_cursor() {
        let mut output = Vec::new();

        render("$ ", &buffer("echo"), 4, &mut output).unwrap();

        // Return to column zero, write it, erase the tail of any longer line
        // that was there, then walk out to the cursor: 2 for the prompt, 4 for
        // the text.
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "\r$ echo\x1b[K\r\x1b[6C"
        );
    }

    #[test]
    fn rendering_an_empty_line_does_not_move_the_cursor_needlessly() {
        let mut output = Vec::new();

        render("", &[], 0, &mut output).unwrap();

        // Column zero is where the cursor already is, so no movement is sent.
        assert_eq!(String::from_utf8(output).unwrap(), "\r\x1b[K\r");
    }
}
