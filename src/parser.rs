use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub struct ParsedCommand {
    pub name: String,
    pub args: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    UnclosedSingleQuote,
    UnclosedDoubleQuote,
    TrailingEscape,
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::UnclosedSingleQuote => "unclosed single quote",
            Self::UnclosedDoubleQuote => "unclosed double quote",
            Self::TrailingEscape => "trailing escape character",
        };

        formatter.write_str(message)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Quote {
    None,
    Single,
    Double,
}

pub fn parse(input: &str) -> Result<Option<ParsedCommand>, ParseError> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote = Quote::None;
    let mut escaped = false;
    let mut word_started = false;

    for character in input.chars() {
        if escaped {
            current.push(character);
            word_started = true;
            escaped = false;
            continue;
        }

        match (quote, character) {
            (Quote::None | Quote::Double, '\\') => {
                escaped = true;
                word_started = true;
            }
            (Quote::None, '\'') => {
                quote = Quote::Single;
                word_started = true;
            }
            (Quote::Single, '\'') => quote = Quote::None,
            (Quote::None, '"') => {
                quote = Quote::Double;
                word_started = true;
            }
            (Quote::Double, '"') => quote = Quote::None,
            (Quote::None, character) if character.is_whitespace() => {
                if word_started {
                    words.push(std::mem::take(&mut current));
                    word_started = false;
                }
            }
            (_, character) => {
                current.push(character);
                word_started = true;
            }
        }
    }

    if escaped {
        return Err(ParseError::TrailingEscape);
    }

    match quote {
        Quote::Single => return Err(ParseError::UnclosedSingleQuote),
        Quote::Double => return Err(ParseError::UnclosedDoubleQuote),
        Quote::None => {}
    }

    if word_started {
        words.push(current);
    }

    let mut words = words.into_iter();
    let Some(name) = words.next() else {
        return Ok(None);
    };

    Ok(Some(ParsedCommand {
        name,
        args: words.collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::{ParseError, ParsedCommand, parse};

    #[test]
    fn empty_input_has_no_command() {
        assert_eq!(parse("  \t ").unwrap(), None);
    }

    #[test]
    fn separates_the_command_and_arguments() {
        assert_eq!(
            parse("echo something else").unwrap(),
            Some(ParsedCommand {
                name: "echo".to_string(),
                args: vec!["something".to_string(), "else".to_string()],
            })
        );
    }

    #[test]
    fn preserves_quoted_text_as_one_argument() {
        assert_eq!(
            parse("echo \"hello there\" ''").unwrap(),
            Some(ParsedCommand {
                name: "echo".to_string(),
                args: vec!["hello there".to_string(), String::new()],
            })
        );
    }

    #[test]
    fn escaped_spaces_remain_in_the_argument() {
        assert_eq!(
            parse("echo hello\\ world").unwrap(),
            Some(ParsedCommand {
                name: "echo".to_string(),
                args: vec!["hello world".to_string()],
            })
        );
    }

    #[test]
    fn reports_unclosed_quotes() {
        assert_eq!(
            parse("echo 'unfinished"),
            Err(ParseError::UnclosedSingleQuote)
        );
        assert_eq!(
            parse("echo \"unfinished"),
            Err(ParseError::UnclosedDoubleQuote)
        );
    }
}
