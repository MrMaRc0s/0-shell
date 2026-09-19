use std::env;
use std::fmt;
use std::iter::Peekable;
use std::str::Chars;

/// The way a redirection changes a stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// `>`: create or empty the file, then send standard output to it.
    Truncate,
    /// `>>`: send standard output to the end of the file.
    Append,
    /// `<`: take standard input from the file.
    Read,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Redirection {
    pub direction: Direction,
    pub path: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParsedCommand {
    pub name: String,
    pub args: Vec<String>,
    pub redirections: Vec<Redirection>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    UnclosedSingleQuote,
    UnclosedDoubleQuote,
    TrailingEscape,
    RedirectionWithoutDestination,
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::UnclosedSingleQuote => "unclosed single quote",
            Self::UnclosedDoubleQuote => "unclosed double quote",
            Self::TrailingEscape => "trailing escape character",
            Self::RedirectionWithoutDestination => "syntax error near unexpected token 'newline'",
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
    let mut redirections = Vec::new();
    // The operator that has claimed the next word as its path.
    let mut expecting_path: Option<Direction> = None;

    let mut characters = input.chars().peekable();

    while let Some(character) = characters.next() {
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
            // `$NAME` and `${NAME}` expand to the environment variable's
            // value, or to nothing when it is unset — outside single quotes,
            // which is where a real shell also stops expanding them.
            (Quote::None | Quote::Double, '$') => {
                word_started = true;
                match read_variable_name(&mut characters) {
                    Some(name) => {
                        if let Ok(value) = env::var(&name) {
                            current.push_str(&value);
                        }
                    }
                    // Nothing that could name a variable followed the `$`, so
                    // it stands for itself: `echo $` prints a bare `$`.
                    None => current.push('$'),
                }
            }
            (Quote::None, character) if character.is_whitespace() => {
                take_word(
                    &mut words,
                    &mut current,
                    word_started,
                    &mut expecting_path,
                    &mut redirections,
                );
                word_started = false;
            }
            // Outside quotes, `>` and `<` are operators, not text. `>>` is the
            // two-character operator, so it is collected, not a redirect
            // followed by a stray second one.
            (Quote::None, '>' | '<') => {
                take_word(
                    &mut words,
                    &mut current,
                    word_started,
                    &mut expecting_path,
                    &mut redirections,
                );
                word_started = false;

                let direction = if character == '>' {
                    if characters.peek() == Some(&'>') {
                        characters.next();
                        Direction::Append
                    } else {
                        Direction::Truncate
                    }
                } else {
                    Direction::Read
                };
                expecting_path = Some(direction);
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

    take_word(
        &mut words,
        &mut current,
        word_started,
        &mut expecting_path,
        &mut redirections,
    );

    if expecting_path.is_some() {
        return Err(ParseError::RedirectionWithoutDestination);
    }

    if words.is_empty() && redirections.is_empty() {
        return Ok(None);
    }

    let mut words = words.into_iter();
    let name = words.next().unwrap_or_default();

    Ok(Some(ParsedCommand {
        name,
        args: words.collect(),
        redirections,
    }))
}

/// Hands the word being built to its owner: the command's arguments, or the
/// path a redirection is waiting for.
fn take_word(
    words: &mut Vec<String>,
    current: &mut String,
    word_started: bool,
    expecting_path: &mut Option<Direction>,
    redirections: &mut Vec<Redirection>,
) {
    if !word_started {
        return;
    }

    if let Some(direction) = expecting_path.take() {
        redirections.push(Redirection {
            direction,
            path: std::mem::take(current),
        });
    } else {
        words.push(std::mem::take(current));
    }
}

/// Reads the name a `$` introduces: `{NAME}` up to its closing brace (or to
/// the end of input, if it is never closed), or a bare identifier — a letter
/// or underscore, then letters, digits, or underscores. `None` means the `$`
/// named nothing, because the very next character cannot start one: `$9` and
/// `$$` are shells' own positional and process-id parameters, neither of
/// which this shell has a value for, so both are left as literal text rather
/// than silently expanding to nothing.
fn read_variable_name(characters: &mut Peekable<Chars>) -> Option<String> {
    if characters.peek() == Some(&'{') {
        characters.next();
        let mut name = String::new();
        for character in characters.by_ref() {
            if character == '}' {
                break;
            }
            name.push(character);
        }
        return if name.is_empty() { None } else { Some(name) };
    }

    match characters.peek() {
        Some(&character) if character.is_alphabetic() || character == '_' => {}
        _ => return None,
    }

    let mut name = String::new();
    while let Some(&character) = characters.peek() {
        if character.is_alphanumeric() || character == '_' {
            name.push(character);
            characters.next();
        } else {
            break;
        }
    }

    Some(name)
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::{Direction, ParseError, ParsedCommand, Redirection, parse};

    fn command(name: &str, args: &[&str]) -> ParsedCommand {
        ParsedCommand {
            name: name.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            redirections: Vec::new(),
        }
    }

    #[test]
    fn empty_input_has_no_command() {
        assert_eq!(parse("  \t ").unwrap(), None);
    }

    #[test]
    fn separates_the_command_and_arguments() {
        assert_eq!(
            parse("echo something else").unwrap(),
            Some(command("echo", &["something", "else"]))
        );
    }

    #[test]
    fn preserves_quoted_text_as_one_argument() {
        assert_eq!(
            parse("echo \"hello there\" ''").unwrap(),
            Some(ParsedCommand {
                name: "echo".to_string(),
                args: vec!["hello there".to_string(), String::new()],
                redirections: Vec::new(),
            })
        );
    }

    #[test]
    fn escaped_spaces_remain_in_the_argument() {
        assert_eq!(
            parse("echo hello\\ world").unwrap(),
            Some(command("echo", &["hello world"]))
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

    #[test]
    fn reports_a_redirection_with_no_path() {
        assert_eq!(
            parse("echo >"),
            Err(ParseError::RedirectionWithoutDestination)
        );
        assert_eq!(
            parse("echo hi <"),
            Err(ParseError::RedirectionWithoutDestination)
        );
    }

    #[test]
    fn a_truncate_redirection_takes_the_next_word_as_its_path() {
        assert_eq!(
            parse("echo hello > note.txt").unwrap(),
            Some(ParsedCommand {
                name: "echo".to_string(),
                args: vec!["hello".to_string()],
                redirections: vec![Redirection {
                    direction: Direction::Truncate,
                    path: "note.txt".to_string(),
                }],
            })
        );
    }

    #[test]
    fn the_operator_needs_no_whitespace_around_it() {
        assert_eq!(
            parse("echo hello>out file").unwrap(),
            Some(ParsedCommand {
                name: "echo".to_string(),
                args: vec!["hello".to_string(), "file".to_string()],
                redirections: vec![Redirection {
                    direction: Direction::Truncate,
                    path: "out".to_string(),
                }],
            })
        );
    }

    #[test]
    fn double_arrows_append_and_are_distinct_from_two_redirects() {
        assert_eq!(
            parse("echo more >> log.txt").unwrap(),
            Some(ParsedCommand {
                name: "echo".to_string(),
                args: vec!["more".to_string()],
                redirections: vec![Redirection {
                    direction: Direction::Append,
                    path: "log.txt".to_string(),
                }],
            })
        );
    }

    #[test]
    fn a_read_redirection_also_takes_a_quoted_path() {
        assert_eq!(
            parse("cat < \"my file\"").unwrap(),
            Some(ParsedCommand {
                name: "cat".to_string(),
                args: vec![],
                redirections: vec![Redirection {
                    direction: Direction::Read,
                    path: "my file".to_string(),
                }],
            })
        );
    }

    #[test]
    fn operators_inside_quotes_or_escaped_are_literal_text() {
        assert_eq!(
            parse("echo 'a>b' \"c<d\" e\\>f").unwrap(),
            Some(command("echo", &["a>b", "c<d", "e>f"]))
        );
    }

    #[test]
    fn a_bare_dollar_name_expands_to_the_variables_value() {
        // SAFETY: `ZERO_SHELL_PARSER_TEST_BARE` is a name unique to this
        // test, so no other test's read of it races with this write.
        unsafe { env::set_var("ZERO_SHELL_PARSER_TEST_BARE", "value") };

        assert_eq!(
            parse("echo $ZERO_SHELL_PARSER_TEST_BARE end").unwrap(),
            Some(command("echo", &["value", "end"]))
        );

        unsafe { env::remove_var("ZERO_SHELL_PARSER_TEST_BARE") };
    }

    #[test]
    fn a_braced_name_expands_and_keeps_the_word_that_follows_it_apart() {
        // SAFETY: see above.
        unsafe { env::set_var("ZERO_SHELL_PARSER_TEST_BRACED", "value") };

        assert_eq!(
            parse("echo ${ZERO_SHELL_PARSER_TEST_BRACED}end").unwrap(),
            Some(command("echo", &["valueend"]))
        );

        unsafe { env::remove_var("ZERO_SHELL_PARSER_TEST_BRACED") };
    }

    #[test]
    fn an_unset_variable_expands_to_nothing() {
        assert_eq!(
            parse("echo [$ZERO_SHELL_PARSER_TEST_UNSET]").unwrap(),
            Some(command("echo", &["[]"]))
        );
    }

    #[test]
    fn expansion_happens_inside_double_quotes_but_not_single_quotes() {
        // SAFETY: see above.
        unsafe { env::set_var("ZERO_SHELL_PARSER_TEST_QUOTED", "value") };

        assert_eq!(
            parse("echo \"$ZERO_SHELL_PARSER_TEST_QUOTED\" '$ZERO_SHELL_PARSER_TEST_QUOTED'")
                .unwrap(),
            Some(command(
                "echo",
                &["value", "$ZERO_SHELL_PARSER_TEST_QUOTED"]
            ))
        );

        unsafe { env::remove_var("ZERO_SHELL_PARSER_TEST_QUOTED") };
    }

    #[test]
    fn a_dollar_with_nothing_that_can_name_a_variable_is_literal() {
        assert_eq!(
            parse("echo a$ b$$ $9").unwrap(),
            Some(command("echo", &["a$", "b$$", "$9"]))
        );
    }

    #[test]
    fn several_redirections_are_all_remembered_in_order() {
        assert_eq!(
            parse("cat < in.txt > out.txt").unwrap(),
            Some(ParsedCommand {
                name: "cat".to_string(),
                args: vec![],
                redirections: vec![
                    Redirection {
                        direction: Direction::Read,
                        path: "in.txt".to_string(),
                    },
                    Redirection {
                        direction: Direction::Truncate,
                        path: "out.txt".to_string(),
                    },
                ],
            })
        );
    }

    #[test]
    fn a_redirection_alone_is_kept_even_though_there_is_no_command() {
        assert_eq!(
            parse("> empty").unwrap(),
            Some(ParsedCommand {
                name: String::new(),
                args: vec![],
                redirections: vec![Redirection {
                    direction: Direction::Truncate,
                    path: "empty".to_string(),
                }],
            })
        );
    }
}
