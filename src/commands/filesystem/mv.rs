use std::io::{self, Write};

/// Moves or renames the source operands.
///
/// Not implemented yet - see task 7.
pub fn mv<E>(args: &[String], errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    let _ = args;
    writeln!(errors, "mv: not implemented yet")
}
