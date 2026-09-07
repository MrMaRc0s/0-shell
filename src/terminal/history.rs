//! The lines already entered, and where the user is while browsing them.

/// How many lines are kept. Old ones fall off the front.
const CAPACITY: usize = 500;

/// Previously entered lines, newest last.
#[derive(Default)]
pub struct History {
    entries: Vec<String>,
    /// How far back the user has walked, or `None` while typing a fresh line.
    position: Option<usize>,
    /// The half-typed line that Up was pressed on, kept so Down can give it back.
    draft: String,
}

impl History {
    /// Records a line that was just run and stops any browsing.
    ///
    /// A blank line is not worth remembering, and neither is a line identical
    /// to the one before it: holding Enter on the same command should not fill
    /// the history with copies of it.
    pub fn remember(&mut self, line: &str) {
        self.position = None;
        self.draft.clear();

        if line.trim().is_empty() || self.entries.last().is_some_and(|last| last == line) {
            return;
        }

        if self.entries.len() == CAPACITY {
            self.entries.remove(0);
        }

        self.entries.push(line.to_string());
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

#[cfg(test)]
mod tests {
    use super::{CAPACITY, History};

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
}
