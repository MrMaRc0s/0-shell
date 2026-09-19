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

    use super::{change_directory, clear, echo, exit, pwd, type_of, which};
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
