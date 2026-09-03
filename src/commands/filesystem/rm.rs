use std::io::{self, Write};

/// Removes each named file, and directories when `-r` is given.
///
/// Not implemented yet - see task 8.
pub fn rm<E>(args: &[String], errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    let _ = args;
    writeln!(errors, "rm: not implemented yet")
}
