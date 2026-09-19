//! `touch`: create a file if missing, otherwise stamp it with the current time.

use std::fs::{self, FileTimes, OpenOptions};
use std::io::{self, ErrorKind};
use std::time::SystemTime;

use super::options::{self, Invalid};
use crate::commands::report;

/// Writes `touch: cannot touch '<path>': <reason>` and returns `Ok(())`.
fn cannot_touch<E>(path: &str, error: &io::Error, errors: &mut E) -> io::Result<()>
where
    E: io::Write,
{
    report::io_error(
        "touch",
        &format!("cannot touch {}", report::quoted(path)),
        error,
        errors,
    )
}

/// Which flags `touch` accepts. The long spelling `--no-create` is refused
/// like every long option in this shell, and the explicit timestamps that
/// `-t`, `-d`, and `-r` would set are not supported, so those are reported as
/// invalid options rather than taken for filenames. Also read by Tab
/// completion so `touch -<Tab>` offers exactly what `options::parse` below
/// actually takes.
pub(crate) const OPTIONS: &str = "cam";

/// Updates each operand the way GNU `touch` does: a missing file is created
/// (unless `-c`), and an existing one has the requested timestamps set to now.
pub fn touch<E>(args: &[String], errors: &mut E) -> io::Result<()>
where
    E: io::Write,
{
    let parsed = match options::parse(args, OPTIONS) {
        Ok(parsed) => parsed,
        Err(Invalid::Short(flag)) => return report::invalid_option("touch", flag, errors),
        Err(Invalid::Long(option)) => return report::unrecognized_option("touch", &option, errors),
    };

    if parsed.operands.is_empty() {
        return report::missing_file_operand("touch", errors);
    }

    let no_create = parsed.has('c');
    let only_accessed = parsed.has('a');
    let only_modified = parsed.has('m');
    // With no `-a`/`-m` both timestamps are stamped; each flag narrows the
    // update to the other one. A newly created file gets both from the kernel
    // regardless.
    let update_accessed = !only_modified;
    let update_modified = !only_accessed;

    for path in &parsed.operands {
        match fs::metadata(path) {
            Ok(_) => stamp(path, update_accessed, update_modified, errors)?,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                if no_create {
                    continue;
                }
                create(path, errors)?;
            }
            Err(error) => report::io_error("touch", &report::quoted(path), &error, errors)?,
        }
    }

    Ok(())
}

/// Sets the requested timestamps of an existing file to now.
fn stamp<E>(
    path: &str,
    update_accessed: bool,
    update_modified: bool,
    errors: &mut E,
) -> io::Result<()>
where
    E: io::Write,
{
    let mut times = FileTimes::new();
    let now = SystemTime::now();
    if update_accessed {
        times = times.set_accessed(now);
    }
    if update_modified {
        times = times.set_modified(now);
    }

    match fs::File::open(path) {
        Ok(file) => match file.set_times(times) {
            Ok(()) => Ok(()),
            Err(error) => cannot_touch(path, &error, errors),
        },
        Err(error) => cannot_touch(path, &error, errors),
    }
}

/// Creates the file; the kernel stamps a fresh file with both times, which is
/// what `touch` would ask for anyway.
fn create<E>(path: &str, errors: &mut E) -> io::Result<()>
where
    E: io::Write,
{
    // `create_new` refuses rather than overwriting whatever appeared since the
    // check that sent us here.
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(_) => Ok(()),
        Err(error) => cannot_touch(path, &error, errors),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::touch;
    use crate::testing::TempDir;

    fn errors_of(argv: &[&str]) -> Vec<u8> {
        let mut errors = Vec::new();
        let args: Vec<String> = argv.iter().map(|arg| arg.to_string()).collect();
        touch(&args, &mut errors).unwrap();
        errors
    }

    #[test]
    fn creates_a_missing_file() {
        let temp = TempDir::new("touch_create");
        let path = temp.arg("new.txt");

        let errors = errors_of(&[&path]);

        assert!(errors.is_empty());
        let meta = fs::metadata(&path).unwrap();
        assert!(meta.is_file());
        assert_eq!(meta.len(), 0);
    }

    #[test]
    fn keeps_the_content_of_an_existing_file() {
        let temp = TempDir::new("touch_keep");
        let path = temp.arg("doc.txt");
        fs::write(&path, b"kept\n").unwrap();

        let errors = errors_of(&[&path]);

        assert!(errors.is_empty());
        assert_eq!(fs::read(&path).unwrap(), b"kept\n");
    }

    #[test]
    fn updates_the_modification_time_of_an_existing_file() {
        let temp = TempDir::new("touch_mtime");
        let path = temp.arg("old.txt");
        fs::write(&path, b"content\n").unwrap();

        // Stamp it with a date long in the past.
        let past = std::time::UNIX_EPOCH;
        fs::File::open(&path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(past))
            .unwrap();
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), past);

        let errors = errors_of(&[&path]);

        assert!(errors.is_empty());
        assert!(fs::metadata(&path).unwrap().modified().unwrap() > past);
    }

    #[test]
    fn no_create_skips_a_missing_file_silently() {
        let temp = TempDir::new("touch_nocreate");
        let path = temp.arg("absent.txt");

        let errors = errors_of(&["-c", &path]);

        assert!(errors.is_empty());
        assert!(fs::metadata(&path).is_err(), "touch -c created the file");
    }

    #[test]
    fn reports_a_missing_operand() {
        let errors = errors_of(&[]);

        assert_eq!(
            String::from_utf8(errors).unwrap(),
            "touch: missing file operand\nTry 'touch --help' for more information.\n"
        );
    }

    #[test]
    fn refuses_an_unknown_option() {
        let errors = errors_of(&["-t", "x"]);

        assert_eq!(
            String::from_utf8(errors).unwrap(),
            "touch: invalid option -- 't'\nTry 'touch --help' for more information.\n"
        );
    }

    #[test]
    fn reports_a_failed_creation_in_gnus_wording() {
        let temp = TempDir::new("touch_error");
        let path = temp.path().join("missing").join("doc.txt");

        let errors = errors_of(&[path.to_str().unwrap()]);

        assert!(
            String::from_utf8(errors)
                .unwrap()
                .starts_with("touch: cannot touch '")
        );
    }
}
