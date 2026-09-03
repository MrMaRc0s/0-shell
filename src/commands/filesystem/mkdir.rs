use std::io::{self, Write};

/// Creates each named directory.
///
/// Not implemented yet — see task 4.
pub fn mkdir<E>(args: &[String], errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    let _ = args;
    writeln!(errors, "mkdir: not implemented yet")
}
