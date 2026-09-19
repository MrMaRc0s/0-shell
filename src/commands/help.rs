//! `--help` text for every command this shell answers to.
//!
//! GNU follows a usage mistake with `Try '<command> --help' for more
//! information.`, so a `--help` worth trying has to exist for that line to be
//! honest. This is not GNU's own `--help`, which runs to a page of aligned
//! columns and a bug-reporting footer per command: it is a short reminder of
//! the usage line and the flags this shell actually implements, which is a
//! strict subset of GNU's own. `--help` is recognised anywhere on the command
//! line, ahead of ordinary option parsing, the way GNU itself checks for it
//! before erroring out on anything else that is wrong with the arguments.
//!
//! Checked once, centrally, before `dispatch` reaches a command's own
//! implementation: every command below prints its text and runs nothing else,
//! so none of the filesystem or session functions need to know `--help`
//! exists.

/// The text for `name`'s `--help`, or `None` for a name this shell does not
/// answer to at all.
pub fn text(name: &str) -> Option<&'static str> {
    Some(match name {
        "echo" => "Usage: echo [STRING]...\nPrint STRING(s) separated by a single space.\n",
        "pwd" => "Usage: pwd\nPrint the current working directory.\n",
        "cd" => {
            "Usage: cd [DIRECTORY]\nChange the working directory to DIRECTORY, or to $HOME with \
             none given.\n"
        }
        "exit" => "Usage: exit\nExit the shell.\n",
        "clear" => "Usage: clear\nClear the terminal screen and its scrollback.\n",
        "mkdir" => {
            "Usage: mkdir [-p] DIRECTORY...\n\
             Create each DIRECTORY.\n\
             \n\
             \x20 -p    create missing parent directories as needed, and no error\n\
             \x20       if a named directory already exists\n"
        }
        "cat" => {
            "Usage: cat [FILE]...\n\
             Concatenate FILE(s), or standard input with none given, to standard\n\
             output.\n"
        }
        "cp" => {
            "Usage: cp [-r|-R] [-p] SOURCE... DEST\n\
             Copy SOURCE to DEST, or several SOURCEs into an existing DEST directory.\n\
             \n\
             \x20 -r, -R  copy directories recursively\n\
             \x20 -p      preserve access and modification timestamps\n"
        }
        "mv" => {
            "Usage: mv [-i] SOURCE... DEST\n\
             Rename SOURCE to DEST, or move several SOURCEs into an existing DEST\n\
             directory.\n\
             \n\
             \x20 -i    prompt before overwriting an existing destination\n"
        }
        "rm" => {
            "Usage: rm [-r|-R] [-f] [-i] FILE...\n\
             Remove each FILE.\n\
             \n\
             \x20 -r, -R  remove directories and their contents recursively\n\
             \x20 -f      ignore nonexistent files, never prompt\n\
             \x20 -i      prompt before every removal\n"
        }
        "ls" => {
            "Usage: ls [-l] [-a] [-F] [-R] [FILE]...\n\
             List information about FILE(s), or the current directory with none given.\n\
             \n\
             \x20 -l    use a long listing format\n\
             \x20 -a    do not hide entries starting with .\n\
             \x20 -F    append an indicator (one of */=@|) to entries\n\
             \x20 -R    list subdirectories recursively\n"
        }
        "touch" => {
            "Usage: touch [-c] [-a] [-m] FILE...\n\
             Change FILE timestamps to now, creating it first unless -c is given.\n\
             \n\
             \x20 -c    do not create any files\n\
             \x20 -a    change only the access time\n\
             \x20 -m    change only the modification time\n"
        }
        "type" => {
            "Usage: type NAME...\n\
             Say whether each NAME is one of this shell's own commands.\n"
        }
        "which" => {
            "Usage: which NAME...\n\
             The same question `type` answers, in `which`'s own wording.\n"
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::text;
    use crate::commands::COMMAND_NAMES;

    #[test]
    fn every_command_name_has_help_text() {
        for name in COMMAND_NAMES {
            assert!(text(name).is_some(), "{name} has no --help text");
        }
    }

    #[test]
    fn every_entry_starts_with_a_usage_line() {
        for name in COMMAND_NAMES {
            let text = text(name).unwrap();
            assert!(
                text.starts_with(&format!("Usage: {name}")),
                "{name}'s --help does not open with its own usage line: {text:?}"
            );
        }
    }

    #[test]
    fn an_unknown_name_has_no_help_text() {
        assert_eq!(text("nope"), None);
    }
}
