use std::env;
use std::ffi::OsString;
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::CommandAction;
use super::report;

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
        Err(error) => report::message("pwd", &report::reason(&error), errors),
    }
}

pub fn cd<E>(args: &[String], errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    change_directory(args, env::var_os("HOME"), errors)
}

fn change_directory<E>(args: &[String], home: Option<OsString>, errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    let target = match args {
        [] => match home {
            Some(home) => home,
            None => {
                writeln!(errors, "cd: HOME not set")?;
                return Ok(());
            }
        },
        [path] => OsString::from(path),
        _ => {
            writeln!(errors, "cd: too many arguments")?;
            return Ok(());
        }
    };

    if let Err(error) = env::set_current_dir(&target) {
        let path = Path::new(&target).display().to_string();
        report::io_error("cd", &path, &error, errors)?;
    }

    Ok(())
}

/// Sets each `NAME=VALUE` (or bare `NAME`, left at its current value or made
/// empty) as an environment variable, the way the parser's `$NAME` expansion
/// and a later `cd` or `pwd` both read it back.
///
/// This shell runs no subprocess, so there is no child environment to hand
/// the variable to the way a real `export` does; setting it here is already
/// the whole effect, since everything that reads a variable reads this same
/// process environment.
pub fn export<E>(args: &[String], errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    if args.is_empty() {
        writeln!(errors, "export: missing operand")?;
        return Ok(());
    }

    for arg in args {
        match arg.split_once('=') {
            Some((name, value)) if is_identifier(name) => {
                // SAFETY: this shell is single-threaded and starts no other
                // thread, so nothing else can observe this write in progress.
                unsafe { env::set_var(name, value) };
            }
            None if is_identifier(arg) => {
                if env::var_os(arg).is_none() {
                    // SAFETY: see above.
                    unsafe { env::set_var(arg, "") };
                }
            }
            _ => writeln!(errors, "export: `{arg}': not a valid identifier")?,
        }
    }

    Ok(())
}

/// Removes each name from the environment.
pub fn unset<E>(args: &[String], errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    if args.is_empty() {
        writeln!(errors, "unset: missing operand")?;
        return Ok(());
    }

    for name in args {
        // SAFETY: see `export`, above.
        unsafe { env::remove_var(name) };
    }

    Ok(())
}

/// Whether `name` could stand after `$` in the parser's own expansion:
/// a letter or underscore, then letters, digits, or underscores.
fn is_identifier(name: &str) -> bool {
    let mut characters = name.chars();

    match characters.next() {
        Some(first) if first.is_alphabetic() || first == '_' => {}
        _ => return false,
    }

    characters.all(|character| character.is_alphanumeric() || character == '_')
}

pub fn exit<E>(args: &[String], errors: &mut E) -> io::Result<CommandAction>
where
    E: Write,
{
    if args.is_empty() {
        Ok(CommandAction::Exit)
    } else {
        writeln!(errors, "exit: too many arguments")?;
        Ok(CommandAction::Continue)
    }
}

/// Clears the screen by writing the terminal's clear sequence.
///
/// This is the `clear` of a plain terminal: erase the whole display and its
/// scrollback, then home the cursor, so the shell is left alone at the very
/// top with history gone. Arguments are ignored, since there is nothing to
/// configure; a later operand clears all the same.
pub fn clear<W>(_args: &[String], output: &mut W) -> io::Result<()>
where
    W: Write,
{
    output.write_all(b"\x1b[H\x1b[2J\x1b[3J")?;
    output.flush()
}

/// Reports what each name resolves to, the way bash's own `type` builtin
/// does for one of its builtins.
///
/// This shell never runs an external program, so there is nothing beyond its
/// own command table to check a name against: it is one of these commands or
/// it is nothing at all. With no operands there is nothing to report and
/// nothing is printed, which is what `type` alone does in bash too.
pub fn type_of<W, E>(args: &[String], output: &mut W, errors: &mut E) -> io::Result<()>
where
    W: Write,
    E: Write,
{
    for name in args {
        if super::COMMAND_NAMES.contains(&name.as_str()) {
            writeln!(output, "{name} is a shell builtin")?;
        } else {
            report::message("type", &format!("{name}: not found"), errors)?;
        }
    }
    Ok(())
}

/// Reports whether each name is one of this shell's own commands.
///
/// The real `which` searches `$PATH` for an executable and prints where it
/// lives. This shell runs no external program and has no `$PATH` to search,
/// so there is no path to print; what it can still say is whether the name
/// is one it answers to at all.
pub fn which<W, E>(args: &[String], output: &mut W, errors: &mut E) -> io::Result<()>
where
    W: Write,
    E: Write,
{
    for name in args {
        if super::COMMAND_NAMES.contains(&name.as_str()) {
            writeln!(output, "{name}: shell builtin")?;
        } else {
            report::message("which", &format!("no {name} in this shell"), errors)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::os::unix::ffi::OsStrExt;

    use super::{change_directory, clear, echo, exit, export, pwd, type_of, unset, which};
    use crate::commands::CommandAction;
    use crate::testing::WorkingDirectory;

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
        let _directory = WorkingDirectory::hold();
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

    #[test]
    fn cd_changes_to_the_requested_directory() {
        let directory = WorkingDirectory::hold();
        let target = directory.original().parent().unwrap().to_path_buf();
        let args = vec![target.to_string_lossy().into_owned()];
        let mut errors = Vec::new();

        change_directory(&args, None, &mut errors).unwrap();

        assert_eq!(env::current_dir().unwrap(), target);
        assert!(errors.is_empty());
    }

    #[test]
    fn cd_without_an_argument_uses_home() {
        let directory = WorkingDirectory::hold();
        let target = directory
            .original()
            .parent()
            .unwrap()
            .as_os_str()
            .to_owned();
        let mut errors = Vec::new();

        change_directory(&[], Some(target.clone()), &mut errors).unwrap();

        assert_eq!(env::current_dir().unwrap().as_os_str(), target);
        assert!(errors.is_empty());
    }

    #[test]
    fn cd_reports_invalid_input_without_changing_directory() {
        let directory = WorkingDirectory::hold();
        let mut errors = Vec::new();

        change_directory(&[], None, &mut errors).unwrap();
        change_directory(&["one".to_string(), "two".to_string()], None, &mut errors).unwrap();

        assert_eq!(env::current_dir().unwrap(), directory.original());
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            "cd: HOME not set\ncd: too many arguments\n"
        );
    }

    #[test]
    fn export_sets_a_new_variable_and_leaves_an_existing_one_alone() {
        let mut errors = Vec::new();

        export(&["ZERO_SHELL_TEST_EXPORT_A=one".to_string()], &mut errors).unwrap();
        assert_eq!(env::var("ZERO_SHELL_TEST_EXPORT_A").unwrap(), "one");

        // A bare name that already has a value is left untouched.
        export(&["ZERO_SHELL_TEST_EXPORT_A".to_string()], &mut errors).unwrap();
        assert_eq!(env::var("ZERO_SHELL_TEST_EXPORT_A").unwrap(), "one");

        assert!(errors.is_empty());

        // SAFETY: `ZERO_SHELL_TEST_EXPORT_A` is a name unique to this test, so
        // no other test's read of it races with this cleanup.
        unsafe { env::remove_var("ZERO_SHELL_TEST_EXPORT_A") };
    }

    #[test]
    fn export_rejects_a_name_that_is_not_a_valid_identifier() {
        let mut errors = Vec::new();

        export(&["1NOPE=x".to_string()], &mut errors).unwrap();

        assert_eq!(
            String::from_utf8(errors).unwrap(),
            "export: `1NOPE=x': not a valid identifier\n"
        );
    }

    #[test]
    fn export_without_arguments_reports_a_missing_operand() {
        let mut errors = Vec::new();

        export(&[], &mut errors).unwrap();

        assert_eq!(
            String::from_utf8(errors).unwrap(),
            "export: missing operand\n"
        );
    }

    #[test]
    fn unset_removes_a_variable() {
        // SAFETY: `ZERO_SHELL_TEST_UNSET_A` is a name unique to this test, so
        // no other test's read of it races with this write.
        unsafe { env::set_var("ZERO_SHELL_TEST_UNSET_A", "x") };
        let mut errors = Vec::new();

        unset(&["ZERO_SHELL_TEST_UNSET_A".to_string()], &mut errors).unwrap();

        assert!(env::var_os("ZERO_SHELL_TEST_UNSET_A").is_none());
        assert!(errors.is_empty());
    }

    #[test]
    fn unset_without_arguments_reports_a_missing_operand() {
        let mut errors = Vec::new();

        unset(&[], &mut errors).unwrap();

        assert_eq!(
            String::from_utf8(errors).unwrap(),
            "unset: missing operand\n"
        );
    }

    #[test]
    fn clear_writes_the_screen_and_scrollback_reset_sequence() {
        let mut output = Vec::new();

        clear(&[], &mut output).unwrap();

        assert_eq!(output, b"\x1b[H\x1b[2J\x1b[3J");
    }

    #[test]
    fn clear_ignores_arguments() {
        let mut output = Vec::new();

        clear(&["extra".to_string()], &mut output).unwrap();

        assert_eq!(output, b"\x1b[H\x1b[2J\x1b[3J");
    }

    #[test]
    fn type_reports_a_builtin_and_an_unknown_name() {
        let mut output = Vec::new();
        let mut errors = Vec::new();
        let args = vec!["ls".to_string(), "nope".to_string()];

        type_of(&args, &mut output, &mut errors).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "ls is a shell builtin\n"
        );
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            "type: nope: not found\n"
        );
    }

    #[test]
    fn type_says_nothing_without_operands() {
        let mut output = Vec::new();
        let mut errors = Vec::new();

        type_of(&[], &mut output, &mut errors).unwrap();

        assert!(output.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn which_reports_a_builtin_and_an_unknown_name() {
        let mut output = Vec::new();
        let mut errors = Vec::new();
        let args = vec!["cp".to_string(), "nope".to_string()];

        which(&args, &mut output, &mut errors).unwrap();

        assert_eq!(String::from_utf8(output).unwrap(), "cp: shell builtin\n");
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            "which: no nope in this shell\n"
        );
    }

    #[test]
    fn exit_requests_that_the_shell_stops() {
        let mut errors = Vec::new();

        let action = exit(&[], &mut errors).unwrap();

        assert_eq!(action, CommandAction::Exit);
        assert!(errors.is_empty());
    }

    #[test]
    fn exit_with_arguments_reports_an_error_and_continues() {
        let mut errors = Vec::new();

        let action = exit(&["unexpected".to_string()], &mut errors).unwrap();

        assert_eq!(action, CommandAction::Continue);
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            "exit: too many arguments\n"
        );
    }
}
