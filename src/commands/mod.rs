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
        _ => {
            writeln!(errors, "Command '{}' not found", command.name)?;
            Ok(CommandAction::Continue)
        }
    }
}
