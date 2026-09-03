use std::io::{self, Write};

/// Lists directory contents, with the `-l`, `-a`, and `-F` options.
///
/// Not implemented yet - see tasks 9 through 13.
pub fn ls<W, E>(args: &[String], output: &mut W, errors: &mut E) -> io::Result<()>
where
    W: Write,
    E: Write,
{
    let _ = (args, output);
    writeln!(errors, "ls: not implemented yet")
}
