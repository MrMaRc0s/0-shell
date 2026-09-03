use std::io::{self, Write};

/// Copies the source operands to the destination.
///
/// Not implemented yet — see task 6.
pub fn cp<E>(args: &[String], errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    let _ = args;
    writeln!(errors, "cp: not implemented yet")
}
