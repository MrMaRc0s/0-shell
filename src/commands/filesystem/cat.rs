use std::io::{self, Write};

/// Writes the contents of each named file to the output.
///
/// Not implemented yet - see task 5.
pub fn cat<W, E>(args: &[String], output: &mut W, errors: &mut E) -> io::Result<()>
where
    W: Write,
    E: Write,
{
    let _ = (args, output);
    writeln!(errors, "cat: not implemented yet")
}
