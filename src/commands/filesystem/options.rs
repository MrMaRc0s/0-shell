//! Shared short-option parsing for the filesystem commands.
//!
//! Unix tools accept their flags in any position and let them cluster, so
//! `cp -r a b`, `cp a b -r`, and `rm -rf x` all have to work. Each command
//! names the letters it accepts and gets back the flags it was given plus the
//! operands in their original order.

/// An option the command does not accept.
#[derive(Debug, PartialEq, Eq)]
pub enum Invalid {
    /// A short flag letter, reported as `invalid option -- 'x'`.
    Short(char),
    /// A whole long option, reported as `unrecognized option '--x'`.
    Long(String),
}

/// The flags and operands of one command line.
pub struct Options {
    flags: Vec<char>,
    pub operands: Vec<String>,
}

impl Options {
    /// Whether the flag was given, however it was spelled or clustered.
    pub fn has(&self, flag: char) -> bool {
        self.flags.contains(&flag)
    }
}

/// Splits `args` into the flags the command accepts and its operands.
///
/// `allowed` holds every flag letter the command recognises, so `rm` passes
/// `"rRf"` and `ls` passes `"laF"`. Clustered flags are expanded, `--` ends
/// option parsing and is dropped, and a lone `-` is an operand. A file whose
/// name really does start with a dash is therefore reachable as `cp -- -r dst`,
/// exactly as it is in a real shell.
pub fn parse(args: &[String], allowed: &str) -> Result<Options, Invalid> {
    let mut flags = Vec::new();
    let mut operands = Vec::new();
    let mut options_ended = false;

    for arg in args {
        if options_ended || !arg.starts_with('-') || arg == "-" {
            operands.push(arg.clone());
            continue;
        }

        if arg == "--" {
            options_ended = true;
            continue;
        }

        if arg.starts_with("--") {
            return Err(Invalid::Long(arg.clone()));
        }

        for flag in arg.chars().skip(1) {
            if !allowed.contains(flag) {
                return Err(Invalid::Short(flag));
            }
            if !flags.contains(&flag) {
                flags.push(flag);
            }
        }
    }

    Ok(Options { flags, operands })
}

#[cfg(test)]
mod tests {
    use super::{Invalid, parse};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn separates_flags_from_operands_in_any_position() {
        let parsed = parse(&args(&["-r", "source", "target", "-R"]), "rR").unwrap();

        assert!(parsed.has('r'));
        assert!(parsed.has('R'));
        assert_eq!(parsed.operands, args(&["source", "target"]));
    }

    #[test]
    fn expands_clustered_flags() {
        let parsed = parse(&args(&["-laF", "dir"]), "laF").unwrap();

        assert!(parsed.has('l'));
        assert!(parsed.has('a'));
        assert!(parsed.has('F'));
        assert_eq!(parsed.operands, args(&["dir"]));
    }

    #[test]
    fn keeps_a_lone_dash_as_an_operand() {
        let parsed = parse(&args(&["-"]), "r").unwrap();

        assert!(!parsed.has('r'));
        assert_eq!(parsed.operands, args(&["-"]));
    }

    #[test]
    fn stops_parsing_options_at_a_double_dash() {
        let parsed = parse(&args(&["--", "-r", "target"]), "r").unwrap();

        assert!(!parsed.has('r'));
        assert_eq!(parsed.operands, args(&["-r", "target"]));
    }

    #[test]
    fn rejects_an_unknown_short_flag() {
        match parse(&args(&["-rq", "path"]), "rR") {
            Err(Invalid::Short(flag)) => assert_eq!(flag, 'q'),
            _ => panic!("expected an invalid short flag"),
        }
    }

    #[test]
    fn rejects_a_long_option_whole() {
        match parse(&args(&["--bogus"]), "rR") {
            Err(Invalid::Long(option)) => assert_eq!(option, "--bogus"),
            _ => panic!("expected an unrecognized long option"),
        }
    }
}
