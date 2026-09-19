mod filesystem;
pub(crate) mod report;
mod session;

use std::io::{self, BufRead, Read, Write};

use crate::parser::ParsedCommand;

/// Every command name this shell answers to, for Tab completion.
///
/// Kept beside `dispatch` because it has to match the arms below: a name
/// offered here that `dispatch` does not know would complete to
/// `Command 'name' not found`. Deliberately not drawn from `$PATH`, since this
/// shell runs no external program and would be offering names it cannot run.
pub const COMMAND_NAMES: [&str; 14] = [
    "cat", "cd", "clear", "cp", "echo", "exit", "ls", "mkdir", "mv", "pwd", "rm", "touch", "type",
    "which",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandAction {
    Continue,
    Exit,
}

pub fn dispatch<W, E>(
    command: &ParsedCommand,
    input: Option<&mut dyn BufRead>,
    output: &mut W,
    errors: &mut E,
    colored: bool,
    interactive_terminal: bool,
) -> io::Result<CommandAction>
where
    W: Write,
    E: Write,
{
    match command.name.as_str() {
        "echo" => {
            session::echo(&command.args, output)?;
            Ok(CommandAction::Continue)
        }
        "pwd" => {
            session::pwd(&command.args, output, errors)?;
            Ok(CommandAction::Continue)
        }
        "cd" => {
            session::cd(&command.args, errors)?;
            Ok(CommandAction::Continue)
        }
        "clear" => {
            session::clear(&command.args, output)?;
            Ok(CommandAction::Continue)
        }
        "exit" => session::exit(&command.args, errors),
        "mkdir" => {
            filesystem::mkdir(&command.args, errors)?;
            Ok(CommandAction::Continue)
        }
        "cat" => {
            // `cat` only ever reads, so a `dyn BufRead` serves it exactly as
            // a `dyn Read` would; the supertrait coercion needs stating.
            let input = input.map(|reader| reader as &mut dyn Read);
            filesystem::cat(&command.args, input, output, errors)?;
            Ok(CommandAction::Continue)
        }
        "cp" => {
            filesystem::cp(&command.args, errors)?;
            Ok(CommandAction::Continue)
        }
        "mv" => {
            filesystem::mv(&command.args, input, interactive_terminal, errors)?;
            Ok(CommandAction::Continue)
        }
        "rm" => {
            filesystem::rm(&command.args, input, interactive_terminal, errors)?;
            Ok(CommandAction::Continue)
        }
        "ls" => {
            filesystem::ls(&command.args, output, errors, colored)?;
            Ok(CommandAction::Continue)
        }
        "touch" => {
            filesystem::touch(&command.args, errors)?;
            Ok(CommandAction::Continue)
        }
        "type" => {
            session::type_of(&command.args, output, errors)?;
            Ok(CommandAction::Continue)
        }
        "which" => {
            session::which(&command.args, output, errors)?;
            Ok(CommandAction::Continue)
        }
        _ => {
            writeln!(errors, "Command '{}' not found", command.name)?;
            Ok(CommandAction::Continue)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{CommandAction, dispatch};
    use crate::parser::ParsedCommand;
    use crate::testing::TempDir;

    fn run(line: &[&str]) -> (String, String, CommandAction) {
        let command = ParsedCommand {
            name: line[0].to_string(),
            args: line[1..].iter().map(|arg| arg.to_string()).collect(),
            redirections: Vec::new(),
        };
        let mut output = Vec::new();
        let mut errors = Vec::new();

        let action = dispatch(&command, None, &mut output, &mut errors, false, false).unwrap();

        (
            String::from_utf8(output).unwrap(),
            String::from_utf8(errors).unwrap(),
            action,
        )
    }

    #[test]
    fn every_filesystem_command_reaches_its_implementation() {
        let temp = TempDir::new("dispatch");
        let made = temp.arg("made");
        let file = temp.arg("made/doc.txt");
        let copy = temp.arg("made/copy.txt");

        let (_, errors, action) = run(&["mkdir", &made]);
        assert!(errors.is_empty(), "mkdir: {errors}");
        assert!(fs::metadata(&made).unwrap().is_dir());
        assert_eq!(action, CommandAction::Continue);

        fs::write(&file, b"dispatched\n").unwrap();

        let (output, errors, _) = run(&["cat", &file]);
        assert_eq!(output, "dispatched\n", "cat");
        assert!(errors.is_empty(), "cat: {errors}");

        let (_, errors, _) = run(&["cp", &file, &copy]);
        assert!(errors.is_empty(), "cp: {errors}");
        assert_eq!(fs::read(&copy).unwrap(), b"dispatched\n");

        let (output, errors, _) = run(&["ls", &made]);
        assert_eq!(output, "copy.txt\ndoc.txt\n", "ls");
        assert!(errors.is_empty(), "ls: {errors}");

        let (output, errors, _) = run(&["ls", "-l", &file]);
        assert!(output.starts_with("-rw"), "ls -l: {output}");
        assert!(errors.is_empty(), "ls -l: {errors}");

        let moved = temp.arg("made/moved.txt");
        let (_, errors, _) = run(&["mv", &copy, &moved]);
        assert!(errors.is_empty(), "mv: {errors}");
        assert_eq!(fs::read(&moved).unwrap(), b"dispatched\n");

        let (_, errors, _) = run(&["rm", "-r", &made]);
        assert!(errors.is_empty(), "rm: {errors}");
        assert!(fs::metadata(&made).is_err(), "rm left the directory behind");
    }

    #[test]
    fn the_mv_arm_moves_and_reports_through_the_dispatcher() {
        let temp = TempDir::new("dispatch_mv");
        let source = temp.arg("source.txt");
        let target = temp.arg("target.txt");
        fs::write(&source, b"moved\n").unwrap();

        let (output, errors, action) = run(&["mv", &source, &target]);

        assert!(output.is_empty(), "mv: {output}");
        assert!(errors.is_empty(), "mv: {errors}");
        assert_eq!(fs::read(&target).unwrap(), b"moved\n");
        assert!(fs::symlink_metadata(&source).is_err(), "source survived");
        assert_eq!(action, CommandAction::Continue);

        // A failure has to come back through the same arm rather than reaching
        // the unknown-command path.
        let (_, errors, action) = run(&["mv", &temp.arg("nope.txt"), &target]);

        assert!(errors.starts_with("mv: cannot stat "), "mv: {errors}");
        assert_eq!(action, CommandAction::Continue);
    }

    #[test]
    fn session_commands_still_route_and_exit_still_stops_the_shell() {
        let (output, errors, action) = run(&["echo", "one", "two"]);
        assert_eq!(output, "one two\n");
        assert!(errors.is_empty());
        assert_eq!(action, CommandAction::Continue);

        let (output, _, _) = run(&["pwd"]);
        assert!(output.starts_with('/'), "pwd: {output}");

        let (output, errors, action) = run(&["clear"]);
        assert_eq!(output, "\x1b[H\x1b[2J\x1b[3J");
        assert!(errors.is_empty());
        assert_eq!(action, CommandAction::Continue);

        let (_, _, action) = run(&["exit"]);
        assert_eq!(action, CommandAction::Exit);
    }

    #[test]
    fn the_touch_arm_creates_and_reports_through_the_dispatcher() {
        let temp = TempDir::new("dispatch_touch");
        let path = temp.arg("new.txt");

        let (output, errors, action) = run(&["touch", &path]);

        assert!(output.is_empty(), "touch: {output}");
        assert!(errors.is_empty(), "touch: {errors}");
        assert!(fs::metadata(&path).unwrap().is_file());
        assert_eq!(action, CommandAction::Continue);

        // A failure has to come back through the same arm rather than reaching
        // the unknown-command path.
        let (_, errors, action) = run(&["touch", &temp.arg("missing/doc.txt")]);

        assert!(
            errors.starts_with("touch: cannot touch "),
            "touch: {errors}"
        );
        assert_eq!(action, CommandAction::Continue);
    }

    #[test]
    fn an_unknown_name_is_reported_and_the_shell_keeps_going() {
        let (output, errors, action) = run(&["something"]);

        assert!(output.is_empty());
        assert_eq!(errors, "Command 'something' not found\n");
        assert_eq!(action, CommandAction::Continue);
    }
}
