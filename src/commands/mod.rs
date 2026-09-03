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
