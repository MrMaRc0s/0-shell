//! Working out what the half-typed word in front of the cursor could become.
//!
//! The first word of a line is a command, so it completes against the names
//! this shell knows. Every other word is a path, so it completes against the
//! filesystem. Nothing here consults `$PATH`: this shell runs no external
//! program, so offering the name of one would be offering a command that
//! answers `Command 'name' not found`.

use std::fs;
use std::path::Path;

use crate::commands::COMMAND_NAMES;

/// What Tab found.
pub struct Completion {
    /// Where in the line the replaced word begins.
    pub start: usize,
    /// The word the cursor sits in, replaced by as much as could be settled.
    pub word: String,
    /// Every candidate, kept so the editor can list them when there is a choice.
    pub matches: Vec<String>,
}

/// Completes the word ending at `cursor`.
///
/// The word runs back to the nearest space, so completion works mid-line and
/// not only at the end of it.
pub fn complete(line: &[char], cursor: usize) -> Completion {
    let start = word_start(line, cursor);
    let word: String = line[start..cursor].iter().collect();

    // A word is a command only if nothing but spaces sits before it.
    let is_command = line[..start]
        .iter()
        .all(|character| character.is_whitespace());

    let mut matches = if is_command {
        command_matches(&word)
    } else {
        path_matches(&word)
    };

    matches.sort();
    matches.dedup();

    let word = match matches.as_slice() {
        // Nothing matched, so the word is left exactly as it was.
        [] => word,
        // One candidate: take it whole, and add the space or slash that says so.
        [only] => only.clone(),
        // Several: take as much as they all agree on, which may be nothing.
        many => longest_common_prefix(many),
    };

    Completion {
        start,
        word,
        matches,
    }
}

/// Where the word under the cursor begins.
fn word_start(line: &[char], cursor: usize) -> usize {
    line[..cursor]
        .iter()
        .rposition(|character| character.is_whitespace())
        .map_or(0, |index| index + 1)
}

/// Command names beginning with `prefix`, each finished with a space.
fn command_matches(prefix: &str) -> Vec<String> {
    COMMAND_NAMES
        .iter()
        .filter(|name| name.starts_with(prefix))
        .map(|name| format!("{name} "))
        .collect()
}

/// Paths beginning with `prefix`, directories finished with a slash.
///
/// The prefix is split where the last slash is, so `src/comm` looks inside
/// `src` for names starting with `comm`, and the directory part is preserved in
/// the answer so the word can be swapped in whole.
fn path_matches(prefix: &str) -> Vec<String> {
    let (directory, partial) = match prefix.rfind('/') {
        Some(slash) => (&prefix[..=slash], &prefix[slash + 1..]),
        None => ("", prefix),
    };

    let readable = if directory.is_empty() {
        Path::new(".")
    } else {
        Path::new(directory)
    };

    let Ok(entries) = fs::read_dir(readable) else {
        return Vec::new();
    };

    let mut matches = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };

        // Hidden entries stay hidden until the user types the dot that asks
        // for them, which is what a shell's completion does.
        if !name.starts_with(partial) || (name.starts_with('.') && !partial.starts_with('.')) {
            continue;
        }

        let directories = entry.file_type().is_ok_and(|kind| kind.is_dir());
        let suffix = if directories { "/" } else { " " };

        matches.push(format!("{directory}{name}{suffix}"));
    }

    matches
}

/// The longest beginning that every candidate shares.
fn longest_common_prefix(matches: &[String]) -> String {
    let Some((first, rest)) = matches.split_first() else {
        return String::new();
    };

    let mut shared: Vec<char> = first.chars().collect();
    for candidate in rest {
        let common = candidate
            .chars()
            .zip(shared.iter())
            .take_while(|(candidate, shared)| candidate == *shared)
            .count();
        shared.truncate(common);
    }

    shared.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::complete;
    use crate::testing::{TempDir, WorkingDirectory};

    fn line(text: &str) -> Vec<char> {
        text.chars().collect()
    }

    /// Completes at the end of `text`, which is where Tab is usually pressed.
    fn at_end(text: &str) -> (String, Vec<String>) {
        let line = line(text);
        let completion = complete(&line, line.len());

        (completion.word, completion.matches)
    }

    #[test]
    fn completes_a_command_name_that_only_one_command_shares() {
        let (word, matches) = at_end("ec");

        // The trailing space is part of the answer: the name is settled, so
        // the next thing typed is an argument.
        assert_eq!(word, "echo ");
        assert_eq!(matches, vec!["echo ".to_string()]);
    }

    #[test]
    fn stops_at_what_several_commands_agree_on() {
        let (word, matches) = at_end("c");

        // `cat`, `cd`, `clear`, and `cp` share only the `c` already typed.
        assert_eq!(word, "c");
        assert_eq!(matches, vec!["cat ", "cd ", "clear ", "cp "]);
    }

    #[test]
    fn extends_to_the_longest_shared_beginning() {
        let (word, matches) = at_end("m");

        // `mkdir` and `mv` agree on `m` alone, so nothing is added.
        assert_eq!(word, "m");
        assert_eq!(matches, vec!["mkdir ", "mv "]);
    }

    #[test]
    fn offers_nothing_for_a_name_no_command_has() {
        let (word, matches) = at_end("zzz");

        assert_eq!(word, "zzz");
        assert!(matches.is_empty());
    }

    #[test]
    fn completes_a_path_once_past_the_command() {
        let temp = TempDir::new("complete_path");
        let _directory = WorkingDirectory::enter(temp.path());
        fs::write("alpha.txt", b"a").unwrap();
        fs::write("beta.txt", b"b").unwrap();

        let (word, matches) = at_end("cat al");

        assert_eq!(word, "alpha.txt ");
        assert_eq!(matches, vec!["alpha.txt ".to_string()]);
    }

    #[test]
    fn marks_a_directory_with_a_slash_rather_than_a_space() {
        let temp = TempDir::new("complete_directory");
        let _directory = WorkingDirectory::enter(temp.path());
        fs::create_dir("project").unwrap();

        let (word, _) = at_end("ls pro");

        // A slash says the path can be walked further, and leaves the cursor
        // ready to keep typing rather than starting a new argument.
        assert_eq!(word, "project/");
    }

    #[test]
    fn looks_inside_the_directory_the_word_already_names() {
        let temp = TempDir::new("complete_nested");
        let _directory = WorkingDirectory::enter(temp.path());
        fs::create_dir("project").unwrap();
        fs::write("project/readme.md", b"r").unwrap();

        let (word, _) = at_end("cat project/re");

        // The directory part is carried through, so the word swaps in whole.
        assert_eq!(word, "project/readme.md ");
    }

    #[test]
    fn hides_dotfiles_until_the_dot_is_typed() {
        let temp = TempDir::new("complete_hidden");
        let _directory = WorkingDirectory::enter(temp.path());
        fs::write(".hidden", b"h").unwrap();
        fs::write("shown", b"s").unwrap();

        let (_, matches) = at_end("cat ");
        assert_eq!(matches, vec!["shown ".to_string()], "no dot typed");

        let (word, _) = at_end("cat .");
        assert_eq!(word, ".hidden ", "the dot asks for them");
    }

    #[test]
    fn completes_the_word_the_cursor_is_in_rather_than_the_last_one() {
        let line = line("ec something");
        let completion = complete(&line, 2);

        assert_eq!(completion.start, 0);
        assert_eq!(completion.word, "echo ");
    }

    #[test]
    fn reports_where_the_replaced_word_starts() {
        let line = line("cat al");
        let completion = complete(&line, line.len());

        assert_eq!(completion.start, 4, "just past 'cat '");
    }
}
