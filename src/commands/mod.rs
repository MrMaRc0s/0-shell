mod filesystem;
mod report;
mod session;

use std::io::{self, Write};

use crate::parser::ParsedCommand;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandAction {
    Continue,
    Exit,
}

pub fn dispatch<W, E>(
    command: &ParsedCommand,
    output: &mut W,
    errors: &mut E,
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
        "exit" => session::exit(&command.args, errors),
        "mkdir" => {
            filesystem::mkdir(&command.args, errors)?;
            Ok(CommandAction::Continue)
        }
        "cat" => {
            filesystem::cat(&command.args, output, errors)?;
            Ok(CommandAction::Continue)
        }
        "cp" => {
            filesystem::cp(&command.args, errors)?;
            Ok(CommandAction::Continue)
        }
        "mv" => {
            filesystem::mv(&command.args, errors)?;
            Ok(CommandAction::Continue)
        }
        "rm" => {
            filesystem::rm(&command.args, errors)?;
            Ok(CommandAction::Continue)
        }
        "ls" => {
            filesystem::ls(&command.args, output, errors)?;
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
    use crate::commands::filesystem::testing::TempDir;
    use crate::parser::ParsedCommand;

    fn run(line: &[&str]) -> (String, String, CommandAction) {
        let command = ParsedCommand {
            name: line[0].to_string(),
            args: line[1..].iter().map(|arg| arg.to_string()).collect(),
        };
        let mut output = Vec::new();
        let mut errors = Vec::new();

        let action = dispatch(&command, &mut output, &mut errors).unwrap();

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

        let (_, errors, _) = run(&["rm", "-r", &made]);
        assert!(errors.is_empty(), "rm: {errors}");
        assert!(fs::metadata(&made).is_err(), "rm left the directory behind");
    }

    #[test]
    fn the_mv_arm_reaches_mv_rather_than_the_unknown_command_path() {
        // Task 7 has not landed yet, so `mv` still answers with its own
        // message. What this pins is the wiring: the name must reach the
        // command, whatever the command has to say.
        let (_, errors, action) = run(&["mv", "source", "target"]);

        assert!(errors.starts_with("mv: "), "mv: {errors}");
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

        let (_, _, action) = run(&["exit"]);
        assert_eq!(action, CommandAction::Exit);
    }

    #[test]
    fn an_unknown_name_is_reported_and_the_shell_keeps_going() {
        let (output, errors, action) = run(&["something"]);

        assert!(output.is_empty());
        assert_eq!(errors, "Command 'something' not found\n");
        assert_eq!(action, CommandAction::Continue);
    }
}
