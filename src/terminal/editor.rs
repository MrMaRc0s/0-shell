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
use super::raw::{RawMode, ready_within, terminal_width};

/// How long a lone `ESC` waits for the rest of a sequence before it gives up
/// and resolves as an Escape on its own. Long enough that a real escape
/// sequence, sent in one burst by the terminal, is never mistaken for two
/// separate keys; short enough that letting go of the key does not feel like
/// a pause.
const ESCAPE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(25);

/// A line editor, and the history it remembers between lines.
pub struct Editor {
    history: History,
}

impl Editor {
    /// Starts a new editor over an already-built history: loaded from
    /// `$HOME/.0-shell_history` in real use, empty in a test that has no use
    /// for one.
    pub fn new(history: History) -> Self {
        Self { history }
    }

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
        // Taken once outside the loop: a closure capturing `input` itself to
        // ask it again would borrow it a second time while `read_key` below
        // already holds the first.
        let fd = input.as_raw_fd();
        // Asked once per line rather than before every redraw: a resize
        // mid-line is rare enough not to be worth an ioctl on every keystroke.
        let width = terminal_width(input);

        let mut buffer: Vec<char> = Vec::new();
        let mut cursor = 0usize;

        let mut rows = render(prompt, &buffer, cursor, width, 1, output)?;

        loop {
            let Some(key) = read_key(input, || ready_within(fd, ESCAPE_TIMEOUT))? else {
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
                    let (new_cursor, listed) = expand(&mut buffer, cursor, output)?;
                    cursor = new_cursor;
                    // A listing moved the cursor down onto a fresh row of its
                    // own, so the next redraw has nothing above it to walk
                    // back up into; it draws where the listing left off.
                    if listed {
                        rows = 1;
                    }
                }

                Key::Ignored => {}
            }

            rows = render(prompt, &buffer, cursor, width, rows, output)?;
        }
    }
}

/// Acts on Tab, returning where the cursor ends up and whether a candidate
/// listing was printed.
///
/// One candidate is taken whole. Several are taken as far as they agree, and if
/// that settles nothing new the choices are listed instead, which is what
/// pressing Tab twice does elsewhere, without the editor having to notice that
/// it was pressed twice. A listing moves the cursor down onto a row of its
/// own, which the caller needs to know: the next redraw belongs there, not
/// wherever the line was sitting before Tab was pressed.
fn expand<W>(buffer: &mut Vec<char>, cursor: usize, output: &mut W) -> io::Result<(usize, bool)>
where
    W: Write,
{
    let completion = complete(buffer, cursor);
    let original: String = buffer[completion.start..cursor].iter().collect();

    if completion.word != original {
        let replacement: Vec<char> = completion.word.chars().collect();
        let length = replacement.len();

        buffer.splice(completion.start..cursor, replacement);

        return Ok((completion.start + length, false));
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
        return Ok((cursor, true));
    }

    Ok((cursor, false))
}

/// Draws the prompt and the line, and puts the cursor where it belongs.
/// Returns how many terminal rows the drawing now occupies, which the next
/// call needs back as `previous_rows` to erase correctly in its turn.
///
/// `previous_rows` walks the cursor back up to the first row of whatever this
/// same line last drew — 1 when nothing has been drawn yet this call, so the
/// first render moves nowhere — and `ESC [ J` erases everything from there
/// to the end of the screen, covering a redraw that grew or shrank either one.
///
/// Without a known terminal width, the line is drawn as it always was: one
/// row, the cursor walked out from column zero by a plain count, because a
/// terminal whose width cannot be asked for is not one this code can safely
/// assume wraps at all. With a width, a line that fills more than one row is
/// walked back to a known anchor — the first row, column zero — and out again
/// to the cursor's own row and column, rather than trusting where the
/// terminal parked the cursor after auto-wrapping the text just written,
/// which is ambiguous exactly on a row boundary.
fn render<W>(
    prompt: &str,
    buffer: &[char],
    cursor: usize,
    width: Option<usize>,
    previous_rows: usize,
    output: &mut W,
) -> io::Result<usize>
where
    W: Write,
{
    let text: String = buffer.iter().collect();
    let prompt_length = prompt.chars().count();
    let cursor_offset = prompt_length + cursor;

    if previous_rows > 1 {
        write!(output, "\x1b[{}A", previous_rows - 1)?;
    }
    write!(output, "\r\x1b[J{prompt}{text}")?;

    let Some(width) = width.filter(|width| *width > 0) else {
        write!(output, "\r")?;
        if cursor_offset > 0 {
            write!(output, "\x1b[{cursor_offset}C")?;
        }
        output.flush()?;
        return Ok(1);
    };

    let total_length = prompt_length + buffer.len();
    let total_rows = total_length.saturating_sub(1) / width + 1;
    // The row and column the cursor lands on, counted the same way: from how
    // many characters precede it, not from the offset itself. Counting the
    // offset directly would put a cursor sitting right at a row boundary one
    // row too far down — a cursor after exactly one full row of thirty
    // characters in a thirty-column terminal is still on that row, at its
    // last column, not at column zero of an otherwise empty row below it.
    let (cursor_row, cursor_column) = match cursor_offset.checked_sub(1) {
        None => (0, 0),
        Some(preceding) => (preceding / width, preceding % width + 1),
    };

    if total_rows > 1 {
        write!(output, "\x1b[{}A", total_rows - 1)?;
    }
    write!(output, "\r")?;
    if cursor_row > 0 {
        write!(output, "\x1b[{cursor_row}B")?;
    }
    if cursor_column > 0 {
        write!(output, "\x1b[{cursor_column}C")?;
    }

    output.flush()?;
    Ok(total_rows)
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

        let (cursor, listed) = expand(&mut line, 2, &mut output).unwrap();

        assert_eq!(text(&line), "echo ");
        assert_eq!(cursor, 5, "the cursor follows the text in");
        assert!(!listed);
        assert!(output.is_empty(), "one candidate needs no listing");
    }

    #[test]
    fn tab_lists_the_choices_when_it_cannot_settle_them() {
        let mut line = buffer("c");
        let mut output = Vec::new();

        let (cursor, listed) = expand(&mut line, 1, &mut output).unwrap();

        assert_eq!(text(&line), "c", "the line is left alone");
        assert_eq!(cursor, 1);
        assert!(listed);

        // Listed without the trailing space each candidate carries.
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "\r\ncat  cd  clear  cp\r\n"
        );
    }

    #[test]
    fn tab_says_nothing_when_there_is_nothing_to_say() {
        let mut line = buffer("zzz");
        let mut output = Vec::new();

        let (cursor, listed) = expand(&mut line, 3, &mut output).unwrap();

        assert_eq!(text(&line), "zzz");
        assert_eq!(cursor, 3);
        assert!(!listed);
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
        let (cursor, listed) = expand(&mut line, 6, &mut output).unwrap();

        assert_eq!(text(&line), "cat alpha.txt  tail");
        assert_eq!(cursor, 14, "the cursor lands after the completion");
        assert!(!listed);
    }

    #[test]
    fn rendering_draws_the_line_and_places_the_cursor() {
        let mut output = Vec::new();

        let rows = render("$ ", &buffer("echo"), 4, None, 1, &mut output).unwrap();

        // Nothing to walk up to yet, erase to the end of the screen, write
        // it, then walk out to the cursor: 2 for the prompt, 4 for the text.
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "\r\x1b[J$ echo\r\x1b[6C"
        );
        assert_eq!(rows, 1);
    }

    #[test]
    fn a_later_redraw_walks_back_up_to_where_the_first_one_started() {
        let mut output = Vec::new();

        // Told the previous draw took 3 rows: it walks up 2 before erasing.
        render("$ ", &buffer("echo"), 4, None, 3, &mut output).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "\x1b[2A\r\x1b[J$ echo\r\x1b[6C"
        );
    }

    #[test]
    fn rendering_an_empty_line_does_not_move_the_cursor_needlessly() {
        let mut output = Vec::new();

        render("", &[], 0, None, 1, &mut output).unwrap();

        // Column zero is where the cursor already is, so no movement is sent.
        assert_eq!(String::from_utf8(output).unwrap(), "\r\x1b[J\r");
    }

    #[test]
    fn a_known_width_wraps_the_cursor_at_the_end_of_the_line_onto_its_own_row() {
        let mut output = Vec::new();

        // "$ " plus ten digits is twelve characters, which fills exactly two
        // rows of a six-column terminal; the cursor at the end sits at the
        // deferred-wrap edge of the second one, column six.
        let rows = render("$ ", &buffer("0123456789"), 10, Some(6), 1, &mut output).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "\r\x1b[J$ 0123456789\x1b[1A\r\x1b[1B\x1b[6C"
        );
        assert_eq!(rows, 2);
    }

    #[test]
    fn a_known_width_places_the_cursor_mid_line_on_its_own_row_too() {
        let mut output = Vec::new();

        // The cursor sits after "0123456", seven characters in: still row
        // one (zero-indexed), column three.
        render("$ ", &buffer("0123456789"), 7, Some(6), 1, &mut output).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "\r\x1b[J$ 0123456789\x1b[1A\r\x1b[1B\x1b[3C"
        );
    }

    #[test]
    fn a_shorter_redraw_walks_up_by_the_previous_height_and_the_new_one_fits_on_one_row() {
        let mut output = Vec::new();

        // The previous line took three rows; this one is short enough for a
        // single row of a ten-column terminal, so only the first walk-up
        // fires, not the second.
        let rows = render("$ ", &buffer("hi"), 2, Some(10), 3, &mut output).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "\x1b[2A\r\x1b[J$ hi\r\x1b[4C"
        );
        assert_eq!(rows, 1);
    }

    #[test]
    fn a_terminal_too_narrow_for_even_one_character_still_returns_a_row() {
        let mut output = Vec::new();

        let rows = render("$ ", &buffer("hi"), 2, Some(0), 1, &mut output).unwrap();

        // Width zero is nonsensical, treated the same as no known width at
        // all rather than dividing by it.
        assert_eq!(String::from_utf8(output).unwrap(), "\r\x1b[J$ hi\r\x1b[4C");
        assert_eq!(rows, 1);
    }
}
