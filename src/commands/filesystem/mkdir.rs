use std::fs;
use std::io::{self, Write};

use crate::commands::report;

/// Creates each named directory.
///
/// A path that cannot be created is reported and the remaining operands are
/// still attempted, so `mkdir good missing/child other` still creates both
/// `good` and `other`.
pub fn mkdir<E>(args: &[String], errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    if args.is_empty() {
        return report::missing_operand("mkdir", errors);
    }

    for path in args {
        if let Err(error) = fs::create_dir(path) {
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
            "mkdir: missing operand\n"
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
}
