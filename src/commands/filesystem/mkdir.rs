use std::fs;
use std::io::{self, Write};

use super::options::{self, Invalid};
use crate::commands::report;

/// Creates each named directory.
///
/// A path that cannot be created is reported and the remaining operands are
/// still attempted, so `mkdir good missing/child other` still creates both
/// `good` and `other`. With `-p` the whole operand is created at once, any
/// missing parents included, and a directory that already exists is passed
/// over without a complaint, as GNU's flag behaves. The long spelling
/// `--parents` is refused like every long option in this shell.
pub fn mkdir<E>(args: &[String], errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    let parsed = match options::parse(args, "p") {
        Ok(parsed) => parsed,
        Err(Invalid::Short(flag)) => return report::invalid_option("mkdir", flag, errors),
        Err(Invalid::Long(option)) => return report::unrecognized_option("mkdir", &option, errors),
    };

    if parsed.operands.is_empty() {
        return report::missing_operand("mkdir", errors);
    }

    for path in &parsed.operands {
        let result = if parsed.has('p') {
            fs::create_dir_all(path)
        } else {
            fs::create_dir(path)
        };
        if let Err(error) = result {
            let subject = format!("cannot create directory {}", report::quoted(path));
            report::io_error("mkdir", &subject, &error, errors)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::mkdir;
    use crate::testing::TempDir;

    #[test]
    fn creates_a_directory() {
        let temp = TempDir::new("mkdir_one");
        let mut errors = Vec::new();

        mkdir(&[temp.arg("new_folder1")], &mut errors).unwrap();

        assert!(temp.path().join("new_folder1").is_dir());
        assert!(errors.is_empty());
    }

    #[test]
    fn creates_every_directory_it_is_given() {
        let temp = TempDir::new("mkdir_many");
        let mut errors = Vec::new();
        let args = vec![temp.arg("new_folder1"), temp.arg("new_folder2")];

        mkdir(&args, &mut errors).unwrap();

        assert!(temp.path().join("new_folder1").is_dir());
        assert!(temp.path().join("new_folder2").is_dir());
        assert!(errors.is_empty());
    }

    #[test]
    fn builds_the_audit_parent_with_two_children() {
        let temp = TempDir::new("mkdir_audit");
        let mut errors = Vec::new();

        mkdir(&[temp.arg("parent")], &mut errors).unwrap();
        let children = vec![temp.arg("parent/child1"), temp.arg("parent/child2")];
        mkdir(&children, &mut errors).unwrap();

        assert!(temp.path().join("parent/child1").is_dir());
        assert!(temp.path().join("parent/child2").is_dir());
        assert!(errors.is_empty());
    }

    #[test]
    fn reports_missing_operands() {
        let mut errors = Vec::new();

        mkdir(&[], &mut errors).unwrap();

        assert_eq!(
            String::from_utf8(errors).unwrap(),
            "mkdir: missing operand\nTry 'mkdir --help' for more information.\n"
        );
    }

    #[test]
    fn reports_a_directory_that_already_exists() {
        let temp = TempDir::new("mkdir_exists");
        let path = temp.arg("taken");
        fs::create_dir(&path).unwrap();
        let mut errors = Vec::new();

        mkdir(std::slice::from_ref(&path), &mut errors).unwrap();

        assert_eq!(
            String::from_utf8(errors).unwrap(),
            format!("mkdir: cannot create directory '{path}': File exists\n")
        );
    }

    #[test]
    fn keeps_going_after_a_failure() {
        let temp = TempDir::new("mkdir_continue");
        let missing = temp.arg("absent/child");
        let args = vec![temp.arg("first"), missing.clone(), temp.arg("last")];
        let mut errors = Vec::new();

        mkdir(&args, &mut errors).unwrap();

        assert!(temp.path().join("first").is_dir());
        assert!(temp.path().join("last").is_dir());
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            format!("mkdir: cannot create directory '{missing}': No such file or directory\n")
        );
    }

    #[test]
    fn p_creates_the_whole_chain_in_one_go() {
        let temp = TempDir::new("mkdir_p_chain");
        let mut errors = Vec::new();

        mkdir(&["-p".to_string(), temp.arg("a/b/c")], &mut errors).unwrap();

        assert!(temp.path().join("a/b/c").is_dir());
        assert!(errors.is_empty());
    }

    #[test]
    fn p_says_nothing_about_a_directory_that_already_exists() {
        let temp = TempDir::new("mkdir_p_existing");
        fs::create_dir(temp.path().join("taken")).unwrap();
        let path = temp.arg("taken/deeper");
        let mut errors = Vec::new();

        mkdir(&["-p".to_string(), path], &mut errors).unwrap();

        assert!(temp.path().join("taken/deeper").is_dir());
        assert!(errors.is_empty());
    }

    #[test]
    fn p_is_silent_about_the_existing_root_and_dot() {
        let _temp = TempDir::new("mkdir_p_root");
        let mut errors = Vec::new();

        mkdir(
            &["-p".to_string(), "/".to_string(), ".".to_string()],
            &mut errors,
        )
        .unwrap();

        assert!(errors.is_empty());
    }

    #[test]
    fn p_reports_a_regular_file_blocking_the_path() {
        let temp = TempDir::new("mkdir_p_blocked");
        let path = temp.arg("blocked");
        fs::write(&path, b"not a directory").unwrap();
        let mut errors = Vec::new();

        mkdir(&["-p".to_string(), path.clone()], &mut errors).unwrap();

        assert_eq!(
            String::from_utf8(errors).unwrap(),
            format!("mkdir: cannot create directory '{path}': File exists\n")
        );
    }
}
