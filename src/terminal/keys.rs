//! Turning the bytes a terminal sends into keys the editor can act on.
//!
//! In raw mode a keypress is no longer a tidy event. A letter is one byte, an
//! arrow key is three, a control key is a single byte below 32, and a character
//! outside ASCII is two to four bytes of UTF-8. This module reads exactly as
//! many bytes as the key in front of it needs and hands back one [`Key`].

use std::io::{self, Read};

/// One keypress, as far as the line editor cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// A printable character, already decoded from UTF-8.
    Char(char),
    Enter,
    Backspace,
    /// The Delete key, which removes forwards rather than backwards.
    Delete,
    Tab,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    /// Ctrl+C. A signal in canonical mode, an ordinary byte here.
    Interrupt,
    /// Ctrl+D, which ends input only when there is nothing to delete.
    EndOfFile,
    /// Ctrl+U, which clears the line.
    KillLine,
    /// Something this editor has no use for, to be ignored.
    Ignored,
}

/// Reads one keypress, or `None` once the input has ended.
///
/// `ready` answers one question, asked only right after a lone `ESC` byte:
/// has another byte arrived yet. The real terminal answers it by polling the
/// kernel for a short while; a test answers it outright, `true` to resolve an
/// escape sequence the way a complete one already in the buffer would, `false`
/// to resolve a lone Escape the way running out of input would.
pub fn read_key<R, F>(input: &mut R, ready: F) -> io::Result<Option<Key>>
where
    R: Read,
    F: FnOnce() -> bool,
{
    let Some(byte) = read_byte(input)? else {
        return Ok(None);
    };

    let key = match byte {
        0x03 => Key::Interrupt,
        0x04 => Key::EndOfFile,
        0x09 => Key::Tab,
        // Carriage return is what Enter sends once `ICRNL` is off, but a
        // newline is accepted too so nothing depends on that flag.
        0x0d | 0x0a => Key::Enter,
        0x15 => Key::KillLine,
        0x01 => Key::Home,
        0x05 => Key::End,
        // Terminals disagree about which of these Backspace sends.
        0x08 | 0x7f => Key::Backspace,
        0x1b => read_escape(input, ready)?,
        // The remaining control bytes have no meaning here.
        byte if byte < 0x20 => Key::Ignored,
        byte => match read_character(input, byte)? {
            Some(character) => Key::Char(character),
            None => Key::Ignored,
        },
    };

    Ok(Some(key))
}

/// Reads a single byte, or `None` at end of input.
fn read_byte<R>(input: &mut R) -> io::Result<Option<u8>>
where
    R: Read,
{
    let mut byte = [0u8; 1];

    loop {
        return match input.read(&mut byte) {
            Ok(0) => Ok(None),
            Ok(_) => Ok(Some(byte[0])),
            // A `read` cut short by a signal is not a failure; ask again.
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => Err(error),
        };
    }
}

/// Reads the rest of an escape sequence, having just read the escape itself.
///
/// Arrow and navigation keys arrive as `ESC [ A` or `ESC [ 3 ~` and friends.
/// The shape is fixed: any number of parameter bytes, then one final byte in
/// the range `@` to `~` that ends the sequence. Reading to that final byte
/// matters even for sequences with no meaning here, because bytes left behind
/// are read next as ordinary text: stopping early on `ESC [ 1 ; 5 C`, which is
/// Ctrl+Right, types a literal `5C` into the line.
///
/// Pressing Escape on its own sends the one byte and nothing follows it, which
/// looks identical, byte for byte, to the start of a sequence whose rest just
/// has not arrived yet — the only way to tell the two apart is to wait a
/// little and see. `ready` is that wait, already spent by the time this runs:
/// answering `false` resolves a lone Escape at once rather than blocking on a
/// second byte that was never coming, and the next keystroke is read fresh
/// rather than being swallowed trying to complete a sequence that already
/// ended.
fn read_escape<R, F>(input: &mut R, ready: F) -> io::Result<Key>
where
    R: Read,
    F: FnOnce() -> bool,
{
    if !ready() {
        return Ok(Key::Ignored);
    }

    let Some(introducer) = read_byte(input)? else {
        return Ok(Key::Ignored);
    };

    // `ESC O A` is the same arrow key from a terminal in application mode.
    if introducer != b'[' && introducer != b'O' {
        return Ok(Key::Ignored);
    }

    let mut parameters = Vec::new();
    let ending = loop {
        match read_byte(input)? {
            None => return Ok(Key::Ignored),
            Some(byte @ 0x40..=0x7e) => break byte,
            Some(byte) => parameters.push(byte),
        }
    };

    // A parameter here means a modifier was held, as in Ctrl+Right. The
    // sequence has been consumed; the chord itself is not one this editor uses.
    if ending != b'~' && !parameters.is_empty() {
        return Ok(Key::Ignored);
    }

    let key = match ending {
        b'A' => Key::Up,
        b'B' => Key::Down,
        b'C' => Key::Right,
        b'D' => Key::Left,
        b'H' => Key::Home,
        b'F' => Key::End,
        // `ESC [ 3 ~` and its relatives, numbered by which key was pressed.
        b'~' => match leading_number(&parameters) {
            Some(1 | 7) => Key::Home,
            Some(3) => Key::Delete,
            Some(4 | 8) => Key::End,
            _ => Key::Ignored,
        },
        _ => Key::Ignored,
    };

    Ok(key)
}

/// The first parameter of a sequence, which is the digits before any `;`.
fn leading_number(parameters: &[u8]) -> Option<u32> {
    let digits: Vec<u8> = parameters
        .iter()
        .copied()
        .take_while(u8::is_ascii_digit)
        .collect();

    if digits.is_empty() {
        return None;
    }

    digits.iter().try_fold(0u32, |number, digit| {
        number.checked_mul(10)?.checked_add(u32::from(digit - b'0'))
    })
}

/// Completes a UTF-8 character whose first byte has already been read.
fn read_character<R>(input: &mut R, first: u8) -> io::Result<Option<char>>
where
    R: Read,
{
    // The leading byte says how many follow: `110xxxxx` one, `1110xxxx` two,
    // `11110xxx` three. Anything else is not a start byte.
    let length = match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => return Ok(None),
    };

    let mut bytes = [first, 0, 0, 0];
    for slot in bytes.iter_mut().take(length).skip(1) {
        match read_byte(input)? {
            Some(byte) => *slot = byte,
            None => return Ok(None),
        }
    }

    let text = std::str::from_utf8(&bytes[..length]);

    Ok(text.ok().and_then(|text| text.chars().next()))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{Key, read_key};

    /// Decodes an entire byte stream into the keys it represents.
    ///
    /// `ready` always answers `true`: every sequence here is handed over
    /// whole, so there is always a next byte already sitting in the buffer
    /// for an escape to find, the same as a real terminal that already
    /// finished sending one by the time this module gets around to asking.
    fn keys(bytes: &[u8]) -> Vec<Key> {
        let mut input = Cursor::new(bytes.to_vec());
        let mut pressed = Vec::new();

        while let Some(key) = read_key(&mut input, || true).unwrap() {
            pressed.push(key);
        }

        pressed
    }

    #[test]
    fn reads_plain_characters() {
        assert_eq!(keys(b"ls"), vec![Key::Char('l'), Key::Char('s')], "letters");
    }

    #[test]
    fn reads_the_control_keys_the_editor_acts_on() {
        assert_eq!(keys(b"\x03"), vec![Key::Interrupt], "Ctrl+C");
        assert_eq!(keys(b"\x04"), vec![Key::EndOfFile], "Ctrl+D");
        assert_eq!(keys(b"\x09"), vec![Key::Tab], "Tab");
        assert_eq!(keys(b"\x15"), vec![Key::KillLine], "Ctrl+U");
        assert_eq!(keys(b"\x01"), vec![Key::Home], "Ctrl+A");
        assert_eq!(keys(b"\x05"), vec![Key::End], "Ctrl+E");
    }

    #[test]
    fn accepts_either_byte_for_enter_and_for_backspace() {
        assert_eq!(keys(b"\r"), vec![Key::Enter], "carriage return");
        assert_eq!(keys(b"\n"), vec![Key::Enter], "newline");
        assert_eq!(keys(b"\x7f"), vec![Key::Backspace], "delete byte");
        assert_eq!(keys(b"\x08"), vec![Key::Backspace], "backspace byte");
    }

    #[test]
    fn reads_the_arrow_keys() {
        assert_eq!(
            keys(b"\x1b[A\x1b[B\x1b[C\x1b[D"),
            vec![Key::Up, Key::Down, Key::Right, Key::Left]
        );
    }

    #[test]
    fn reads_arrows_from_a_terminal_in_application_mode() {
        // The same keys, introduced by `O` rather than `[`.
        assert_eq!(keys(b"\x1bOA\x1bOD"), vec![Key::Up, Key::Left]);
    }

    #[test]
    fn reads_the_numeric_navigation_sequences() {
        assert_eq!(keys(b"\x1b[3~"), vec![Key::Delete], "Delete");
        assert_eq!(keys(b"\x1b[1~"), vec![Key::Home], "Home");
        assert_eq!(keys(b"\x1b[4~"), vec![Key::End], "End");
        assert_eq!(
            keys(b"\x1b[H\x1b[F"),
            vec![Key::Home, Key::End],
            "short form"
        );
    }

    #[test]
    fn ignores_sequences_it_has_no_use_for() {
        assert_eq!(keys(b"\x1b[99~"), vec![Key::Ignored], "unknown code");
    }

    #[test]
    fn a_modified_arrow_is_swallowed_whole() {
        // Ctrl+Right is `ESC [ 1 ; 5 C`. Reading only as far as the `;` would
        // leave `5C` behind to be typed into the line as two characters.
        assert_eq!(keys(b"\x1b[1;5C"), vec![Key::Ignored]);
        assert_eq!(
            keys(b"\x1b[1;5Cls"),
            vec![Key::Ignored, Key::Char('l'), Key::Char('s')],
            "and the keys after it still read correctly"
        );
    }

    #[test]
    fn decodes_characters_outside_ascii() {
        // Two bytes, three bytes, and four: an accent, an arrow, an emoji.
        assert_eq!(
            keys("é→🦀".as_bytes()),
            vec![Key::Char('é'), Key::Char('→'), Key::Char('🦀')]
        );
    }

    #[test]
    fn a_truncated_character_is_ignored_rather_than_guessed() {
        // The lead byte of a two-byte character, with nothing following it.
        assert_eq!(keys(b"\xc3"), vec![Key::Ignored]);
    }

    #[test]
    fn reports_the_end_of_the_input() {
        let mut input = Cursor::new(Vec::new());

        assert_eq!(read_key(&mut input, || true).unwrap(), None);
    }

    #[test]
    fn a_lone_escape_resolves_to_ignored_without_reading_past_it() {
        // `ready` says no: nothing else arrived in time, so this is a lone
        // Escape rather than the start of a sequence.
        let mut input = Cursor::new(b"\x1ba".to_vec());

        let first = read_key(&mut input, || false).unwrap();
        let second = read_key(&mut input, || true).unwrap();

        // The Escape resolves on its own, and `a` is still there to be read
        // next rather than having been consumed trying to complete a sequence.
        assert_eq!(first, Some(Key::Ignored));
        assert_eq!(second, Some(Key::Char('a')));
    }

    #[test]
    fn a_complete_sequence_still_resolves_when_ready_says_yes() {
        let mut input = Cursor::new(b"\x1b[A".to_vec());

        assert_eq!(read_key(&mut input, || true).unwrap(), Some(Key::Up));
    }
}
