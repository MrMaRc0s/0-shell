use std::io::{self, Write};

use crate::parser::ParsedCommand;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandAction {
    Continue,
    Exit,
}

pub fn dispatch<W, E>(
    command: &ParsedCommand,
    _output: &mut W,
    errors: &mut E,
) -> io::Result<CommandAction>
where
    W: Write,
    E: Write,
{
    writeln!(errors, "Command '{}' not found", command.name)?;
    Ok(CommandAction::Continue)
}
