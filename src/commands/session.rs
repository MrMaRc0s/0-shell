use std::env;
use std::ffi::OsString;
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

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
        writeln!(errors, "cd: {}: {error}", Path::new(&target).display())?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::os::unix::ffi::OsStrExt;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use super::{change_directory, echo, pwd};

    static CURRENT_DIR_LOCK: Mutex<()> = Mutex::new(());

    struct RestoreDirectory(PathBuf);

    impl Drop for RestoreDirectory {
        fn drop(&mut self) {
            env::set_current_dir(&self.0).expect("failed to restore test directory");
        }
    }

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
        let _lock = CURRENT_DIR_LOCK.lock().unwrap();
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
        let _lock = CURRENT_DIR_LOCK.lock().unwrap();
        let original = env::current_dir().unwrap();
        let _restore = RestoreDirectory(original.clone());
        let target = original.parent().unwrap().to_path_buf();
        let args = vec![target.to_string_lossy().into_owned()];
        let mut errors = Vec::new();

        change_directory(&args, None, &mut errors).unwrap();

        assert_eq!(env::current_dir().unwrap(), target);
        assert!(errors.is_empty());
    }

    #[test]
    fn cd_without_an_argument_uses_home() {
        let _lock = CURRENT_DIR_LOCK.lock().unwrap();
        let original = env::current_dir().unwrap();
        let _restore = RestoreDirectory(original.clone());
        let target = original.parent().unwrap().as_os_str().to_owned();
        let mut errors = Vec::new();

        change_directory(&[], Some(target.clone()), &mut errors).unwrap();

        assert_eq!(env::current_dir().unwrap().as_os_str(), target);
        assert!(errors.is_empty());
    }

    #[test]
    fn cd_reports_invalid_input_without_changing_directory() {
        let _lock = CURRENT_DIR_LOCK.lock().unwrap();
        let original = env::current_dir().unwrap();
        let mut errors = Vec::new();

        change_directory(&[], None, &mut errors).unwrap();
        change_directory(&["one".to_string(), "two".to_string()], None, &mut errors).unwrap();

        assert_eq!(env::current_dir().unwrap(), original);
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            "cd: HOME not set\ncd: too many arguments\n"
        );
    }
}
