use std::io::{self, Write};

pub fn echo<W>(args: &[String], output: &mut W) -> io::Result<()>
where
    W: Write,
{
    writeln!(output, "{}", args.join(" "))
}

#[cfg(test)]
mod tests {
    use super::echo;

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
}
