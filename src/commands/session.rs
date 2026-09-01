use std::env;
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;

pub fn echo<W>(args: &[String], output: &mut W) -> io::Result<()>
where
    W: Write,
{
    writeln!(output, "{}", args.join(" "))
}

pub fn pwd<W, E>(args: &[String], output: &mut W, errors: &mut E) -> io::Result<()>
where
    W: Write,
    E: Write,
{
    if !args.is_empty() {
        writeln!(errors, "pwd: too many arguments")?;
        return Ok(());
    }

    match env::current_dir() {
        Ok(path) => {
            output.write_all(path.as_os_str().as_bytes())?;
            output.write_all(b"\n")
        }
        Err(error) => writeln!(errors, "pwd: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::os::unix::ffi::OsStrExt;

    use super::{echo, pwd};

    #[test]
    fn echo_prints_a_blank_line_without_arguments() {
        let mut output = Vec::new();

        echo(&[], &mut output).unwrap();

        assert_eq!(String::from_utf8(output).unwrap(), "\n");
    }

    #[test]
    fn echo_separates_arguments_with_one_space() {
        let mut output = Vec::new();
        let args = vec!["something".to_string(), "else".to_string()];

        echo(&args, &mut output).unwrap();

        assert_eq!(String::from_utf8(output).unwrap(), "something else\n");
    }

    #[test]
    fn pwd_prints_the_current_directory() {
        let mut output = Vec::new();
        let mut errors = Vec::new();
        let current_dir = env::current_dir().unwrap();
        let mut expected = current_dir.as_os_str().as_bytes().to_vec();
        expected.push(b'\n');

        pwd(&[], &mut output, &mut errors).unwrap();

        assert_eq!(output, expected);
        assert!(errors.is_empty());
    }

    #[test]
    fn pwd_rejects_arguments_without_stopping_the_shell() {
        let mut output = Vec::new();
        let mut errors = Vec::new();

        pwd(&["extra".to_string()], &mut output, &mut errors).unwrap();

        assert!(output.is_empty());
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            "pwd: too many arguments\n"
        );
    }
}
