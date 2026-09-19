//! The lines already entered, and where the user is while browsing them.
//!
//! Between sessions the lines live in `$HOME/.0-shell_history`, one per line,
//! oldest first — the same shape bash's own history file takes. A session
//! with no `$HOME`, or a file that cannot be read or written, is not an
//! error: history just does not survive that session, the same way it
//! already does not survive a piped run at all.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::PathBuf;

/// How many lines are kept in memory. Old ones fall off the front.
const CAPACITY: usize = 500;

/// The name of the file a session's history is read from and appended to,
/// inside `$HOME`.
const FILE_NAME: &str = ".0-shell_history";

/// Previously entered lines, newest last.
#[derive(Default)]
pub struct History {
    entries: Vec<String>,
    /// How far back the user has walked, or `None` while typing a fresh line.
    position: Option<usize>,
    /// The half-typed line that Up was pressed on, kept so Down can give it back.
    draft: String,
    /// The history file, open for appending, when there is one to append to.
    file: Option<fs::File>,
}

impl History {
    /// Reads `$HOME/.0-shell_history` into memory and keeps it open so every
    /// later line entered this session is appended to it as it is remembered.
    ///
    /// This is the constructor the real terminal uses; `Default` stays a
    /// plain in-memory history with no file behind it; a test that reaches
    /// for one has no business touching a real person's history file.
    pub fn load() -> Self {
        let mut history = Self::default();

        let Some(path) = history_path() else {
            return history;
        };

        if let Ok(contents) = fs::read_to_string(&path) {
            for line in contents.lines() {
                history.remember_in_memory(line);
            }
        }

        // A file that cannot be opened for appending is not fatal: the
        // session simply stops saving, the way it already behaves when
        // `$HOME` is not set at all.
        history.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok();

        history
    }

    /// Records a line that was just run and stops any browsing.
    ///
    /// A blank line is not worth remembering, and neither is a line identical
    /// to the one before it: holding Enter on the same command should not fill
    /// the history with copies of it. A line that is kept is also appended to
    /// the history file, when this session has one open.
    pub fn remember(&mut self, line: &str) {
        if self.remember_in_memory(line) {
            self.save(line);
        }
    }

    /// The in-memory half of [`Self::remember`], returning whether the line
    /// was actually new. Used both by a live session and, without the saving
    /// half, to replay the file [`Self::load`] just read.
    fn remember_in_memory(&mut self, line: &str) -> bool {
        self.position = None;
        self.draft.clear();

        if line.trim().is_empty() || self.entries.last().is_some_and(|last| last == line) {
            return false;
        }

        if self.entries.len() == CAPACITY {
            self.entries.remove(0);
        }

        self.entries.push(line.to_string());
        true
    }

    /// Appends one line to the history file, if this session has one open.
    ///
    /// Best effort: a history file that stops accepting writes partway
    /// through a session — a filled disk, a permission change — is not worth
    /// interrupting the session over, so the failure is silently dropped.
    fn save(&mut self, line: &str) {
        if let Some(file) = &mut self.file {
            writeln!(file, "{line}").ok();
        }
    }

    /// The line one step further back, if there is one.
    ///
    /// `current` is what the user has typed so far. It is kept on the first step
    /// back so that walking forward again returns to it rather than losing it.
    pub fn previous(&mut self, current: &str) -> Option<&str> {
        let position = match self.position {
            None => {
                self.draft = current.to_string();
                self.entries.len().checked_sub(1)?
            }
            Some(0) => return None,
            Some(position) => position - 1,
        };

        self.position = Some(position);

        Some(&self.entries[position])
    }

    /// The line one step forward, or the draft once the walk is over.
    pub fn next(&mut self) -> Option<&str> {
        let position = self.position? + 1;

        if position >= self.entries.len() {
            self.position = None;
            return Some(&self.draft);
        }

        self.position = Some(position);

        Some(&self.entries[position])
    }

    /// Stops browsing, so the next Up starts again from the newest line.
    pub fn rewind(&mut self) {
        self.position = None;
    }
}

/// `$HOME/.0-shell_history`, or `None` when `$HOME` is not set.
fn history_path() -> Option<PathBuf> {
    let home = env::var_os("HOME")?;
    if home.is_empty() {
        return None;
    }
    Some(PathBuf::from(home).join(FILE_NAME))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{CAPACITY, FILE_NAME, History};
    use crate::testing::{EnvHome, TempDir};

    fn filled(lines: &[&str]) -> History {
        let mut history = History::default();
        for line in lines {
            history.remember(line);
        }
        history
    }

    #[test]
    fn walks_back_through_the_lines_newest_first() {
        let mut history = filled(&["one", "two", "three"]);

        assert_eq!(history.previous(""), Some("three"));
        assert_eq!(history.previous(""), Some("two"));
        assert_eq!(history.previous(""), Some("one"));
        // The oldest line is as far back as there is.
        assert_eq!(history.previous(""), None);
    }

    #[test]
    fn walking_forward_ends_at_the_half_typed_line() {
        let mut history = filled(&["one", "two"]);

        // The draft is captured on the first step back, not before.
        assert_eq!(history.previous("dra"), Some("two"));
        assert_eq!(history.previous("dra"), Some("one"));
        assert_eq!(history.next(), Some("two"));
        assert_eq!(history.next(), Some("dra"), "the draft comes back");
        assert_eq!(history.next(), None, "and there is nothing beyond it");
    }

    #[test]
    fn forward_does_nothing_until_a_walk_has_started() {
        let mut history = filled(&["one"]);

        assert_eq!(history.next(), None);
    }

    #[test]
    fn does_not_remember_blank_lines() {
        let mut history = filled(&["", "   ", "\t"]);

        assert_eq!(history.previous(""), None);
    }

    #[test]
    fn does_not_remember_the_same_line_twice_in_a_row() {
        let mut history = filled(&["ls", "ls", "ls", "pwd", "ls"]);

        // Repeats collapse, but the same line later on is its own entry.
        assert_eq!(history.previous(""), Some("ls"));
        assert_eq!(history.previous(""), Some("pwd"));
        assert_eq!(history.previous(""), Some("ls"));
        assert_eq!(history.previous(""), None);
    }

    #[test]
    fn remembering_a_line_stops_the_current_walk() {
        let mut history = filled(&["one", "two"]);

        assert_eq!(history.previous(""), Some("two"));
        history.remember("three");

        // The next walk starts again from the newest line.
        assert_eq!(history.previous(""), Some("three"));
    }

    #[test]
    fn rewind_starts_the_next_walk_from_the_newest_line() {
        let mut history = filled(&["one", "two"]);

        assert_eq!(history.previous(""), Some("two"));
        assert_eq!(history.previous(""), Some("one"));
        history.rewind();

        assert_eq!(history.previous(""), Some("two"));
    }

    #[test]
    fn old_lines_fall_off_the_front_at_capacity() {
        let mut history = History::default();
        for index in 0..CAPACITY + 10 {
            history.remember(&format!("line {index}"));
        }

        assert_eq!(history.entries.len(), CAPACITY);
        assert_eq!(history.entries.first().unwrap(), "line 10");
        assert_eq!(
            history.previous(""),
            Some(format!("line {}", CAPACITY + 9).as_str())
        );
    }

    #[test]
    fn a_default_history_has_no_file_behind_it() {
        // The constructor every test above uses must never touch a real
        // person's history file, whatever `$HOME` happens to be set to.
        let history = History::default();

        assert!(history.file.is_none());
    }

    #[test]
    fn load_reads_previous_lines_in_order() {
        let temp = TempDir::new("history_load");
        let _home = EnvHome::set(temp.path());
        fs::write(temp.path().join(FILE_NAME), "one\ntwo\nthree\n").unwrap();

        let mut history = History::load();

        assert_eq!(history.previous(""), Some("three"));
        assert_eq!(history.previous(""), Some("two"));
        assert_eq!(history.previous(""), Some("one"));
    }

    #[test]
    fn load_with_no_file_starts_empty_but_still_saves() {
        let temp = TempDir::new("history_load_missing");
        let _home = EnvHome::set(temp.path());

        let mut history = History::load();
        assert_eq!(history.previous(""), None, "nothing to have loaded");

        history.remember("first");
        assert_eq!(
            fs::read_to_string(temp.path().join(FILE_NAME)).unwrap(),
            "first\n"
        );
    }

    #[test]
    fn remembering_a_line_appends_it_to_the_open_file() {
        let temp = TempDir::new("history_save");
        let _home = EnvHome::set(temp.path());
        fs::write(temp.path().join(FILE_NAME), "one\n").unwrap();

        let mut history = History::load();
        history.remember("two");
        history.remember("two"); // a repeat, which is not saved either
        history.remember("three");

        assert_eq!(
            fs::read_to_string(temp.path().join(FILE_NAME)).unwrap(),
            "one\ntwo\nthree\n"
        );
    }

    #[test]
    fn load_without_home_set_starts_empty_and_saves_nothing() {
        let _home = EnvHome::unset();

        let mut history = History::load();
        history.remember("line");

        // Nothing to assert on disk with no `$HOME` to hold a file, only that
        // remembering a line without anywhere to save it does not panic.
        assert_eq!(history.previous(""), Some("line"));
    }
}
