use std::fs::{self, File, Metadata};
use std::io::{self, ErrorKind, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use super::options::{self, Invalid};
use crate::commands::report;

/// Copies each source operand to the destination.
///
/// The last operand is the destination. When it is an existing directory each
/// source is copied *into* it under the source's own basename, which is the
/// audited case (`cp new_folder1/new_doc.txt new_folder2`); otherwise the
/// single source is copied to that exact path, creating or overwriting it.
///
/// Directories need `-r` (or `-R`), as in Unix. Symlinks follow the same rule
/// the real `cp` uses: a plain copy dereferences the link and copies the file
/// it points at, while a recursive copy recreates the link itself.
///
/// Permissions are carried over to every file and directory that is created.
/// One failing operand is reported and the rest are still copied.
pub fn cp<E>(args: &[String], errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    let parsed = match options::parse(args, "rR") {
        Ok(parsed) => parsed,
        Err(Invalid::Short(flag)) => return report::invalid_option("cp", flag, errors),
        Err(Invalid::Long(option)) => return report::unrecognized_option("cp", &option, errors),
    };
    let recursive = parsed.has('r') || parsed.has('R');

    let (destination, sources) = match parsed.operands.split_last() {
        None => return report::missing_file_operand("cp", errors),
        Some((source, [])) => return report::missing_destination_operand("cp", source, errors),
        Some((destination, sources)) => (Path::new(destination), sources),
    };

    let into_directory = destination.is_dir();
    if sources.len() > 1 && !into_directory {
        return report_bad_target(destination, errors);
    }

    for source in sources {
        let source = Path::new(source);
        let target = if into_directory {
            match source.file_name() {
                Some(name) => destination.join(name),
                None => {
                    let subject = format!(
                        "cannot copy {} into {}",
                        report::quoted(source),
                        report::quoted(destination)
                    );
                    report::detail("cp", &subject, "Invalid argument", errors)?;
                    continue;
                }
            }
        } else {
            destination.to_path_buf()
        };

        copy(source, &target, recursive, errors)?;
    }

    Ok(())
}

/// Explains why several sources cannot go to this destination.
///
/// The destination has to be a directory to hold them, so it either exists and
/// is something else, or the reason it is not a directory is the OS's own.
fn report_bad_target<E>(destination: &Path, errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    let subject = format!("target {}", report::quoted(destination));

    match destination.symlink_metadata() {
        Ok(_) => report::detail("cp", &subject, "Not a directory", errors),
        Err(error) => report::io_error("cp", &subject, &error, errors),
    }
}

/// Copies one source to one fully resolved target path.
fn copy<E>(source: &Path, target: &Path, recursive: bool, errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    // A recursive copy preserves the links it walks over, so it asks about the
    // link itself; a plain copy is defined in terms of the file the link points
    // at, so it follows the link, and a dangling one fails to stat exactly as
    // it does under the real `cp`.
    let metadata = if recursive {
        source.symlink_metadata()
    } else {
        source.metadata()
    };
    let metadata = match metadata {
        Ok(metadata) => metadata,
        Err(error) => {
            let subject = format!("cannot stat {}", report::quoted(source));
            return report::io_error("cp", &subject, &error, errors);
        }
    };
    let file_type = metadata.file_type();

    if file_type.is_dir() {
        if !recursive {
            let text = format!(
                "-r not specified; omitting directory {}",
                report::quoted(source)
            );
            return report::message("cp", &text, errors);
        }
        return copy_directory(source, target, &metadata, errors);
    }

    if file_type.is_symlink() {
        return copy_symlink(source, target, errors);
    }

    if file_type.is_file() {
        return copy_file(source, target, &metadata, errors);
    }

    // Devices, FIFOs, and sockets need a node the standard library cannot
    // create, and reading a FIFO would hang the shell until something writes
    // to it, so they are skipped rather than half copied.
    let text = format!("omitting special file {}", report::quoted(source));
    report::message("cp", &text, errors)
}

/// Copies a regular file, then gives the copy the source's permissions.
fn copy_file<E>(source: &Path, target: &Path, metadata: &Metadata, errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    // Creating the target truncates it, so a copy onto the source itself would
    // destroy the very bytes it is about to read.
    if is_same_file(metadata, target) {
        let text = format!(
            "{} and {} are the same file",
            report::quoted(source),
            report::quoted(target)
        );
        return report::message("cp", &text, errors);
    }

    let mut reader = match File::open(source) {
        Ok(file) => file,
        Err(error) => {
            let subject = format!("cannot open {} for reading", report::quoted(source));
            return report::io_error("cp", &subject, &error, errors);
        }
    };

    let mut writer = match File::create(target) {
        Ok(file) => file,
        Err(error) => {
            let subject = format!("cannot create regular file {}", report::quoted(target));
            return report::io_error("cp", &subject, &error, errors);
        }
    };

    if let Err(error) = io::copy(&mut reader, &mut writer) {
        let subject = format!(
            "cannot copy {} to {}",
            report::quoted(source),
            report::quoted(target)
        );
        return report::io_error("cp", &subject, &error, errors);
    }

    if let Err(error) = writer.set_permissions(metadata.permissions()) {
        let subject = format!("preserving permissions for {}", report::quoted(target));
        return report::io_error("cp", &subject, &error, errors);
    }

    Ok(())
}

/// Recreates a symlink at the target, pointing at the same place.
fn copy_symlink<E>(source: &Path, target: &Path, errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    let link = match fs::read_link(source) {
        Ok(link) => link,
        Err(error) => {
            let subject = format!("cannot read symbolic link {}", report::quoted(source));
            return report::io_error("cp", &subject, &error, errors);
        }
    };

    // `symlink` refuses to overwrite, and a copy is expected to replace what is
    // already there. A target that cannot be removed reports itself below.
    if target.symlink_metadata().is_ok() {
        fs::remove_file(target).ok();
    }

    if let Err(error) = std::os::unix::fs::symlink(&link, target) {
        let subject = format!("cannot create symbolic link {}", report::quoted(target));
        return report::io_error("cp", &subject, &error, errors);
    }

    Ok(())
}

/// Copies a directory and everything under it.
fn copy_directory<E>(
    source: &Path,
    target: &Path,
    metadata: &Metadata,
    errors: &mut E,
) -> io::Result<()>
where
    E: Write,
{
    if is_inside(source, target) {
        let text = format!(
            "cannot copy a directory, {}, into itself, {}",
            report::quoted(source),
            report::quoted(target)
        );
        return report::message("cp", &text, errors);
    }

    match fs::create_dir(target) {
        Ok(()) => {}
        // Merging into a directory that is already there is what `cp -r` does.
        Err(error) if error.kind() == ErrorKind::AlreadyExists && target.is_dir() => {}
        Err(error) => {
            let subject = format!("cannot create directory {}", report::quoted(target));
            return report::io_error("cp", &subject, &error, errors);
        }
    }

    let entries = match fs::read_dir(source) {
        Ok(entries) => entries,
        Err(error) => {
            let subject = format!("cannot open {} for reading", report::quoted(source));
            return report::io_error("cp", &subject, &error, errors);
        }
    };

    for entry in entries {
        match entry {
            Ok(entry) => copy(&entry.path(), &target.join(entry.file_name()), true, errors)?,
            Err(error) => {
                let subject = format!("cannot read directory {}", report::quoted(source));
                report::io_error("cp", &subject, &error, errors)?;
            }
        }
    }

    // Permissions come last: a source directory that is not writable would
    // otherwise stop its own contents from being written into the copy.
    if let Err(error) = fs::set_permissions(target, metadata.permissions()) {
        let subject = format!("preserving permissions for {}", report::quoted(target));
        return report::io_error("cp", &subject, &error, errors);
    }

    Ok(())
}

/// Whether the target is the very file described by `metadata`.
///
/// Compared by device and inode rather than by path text, so `a` and `./a`, and
/// a hard link to either, are all recognised as the same file.
fn is_same_file(metadata: &Metadata, target: &Path) -> bool {
    match target.metadata() {
        Ok(target) => target.dev() == metadata.dev() && target.ino() == metadata.ino(),
        Err(_) => false,
    }
}

/// Whether the target sits inside the source directory.
///
/// `cp -r dir dir/copy` would otherwise copy the growing tree into itself and
/// never finish. Both paths are resolved first so that `.`, `..`, and links
/// cannot hide the nesting. A target that does not exist yet is resolved
/// through its parent, which does.
fn is_inside(source: &Path, target: &Path) -> bool {
    let Ok(source) = source.canonicalize() else {
        return false;
    };

    let target = match target.canonicalize() {
        Ok(target) => target,
        Err(_) => match (target.parent(), target.file_name()) {
            (Some(parent), Some(name)) => match parent.canonicalize() {
                Ok(parent) => parent.join(name),
                Err(_) => return false,
            },
            _ => return false,
        },
    };

    target.starts_with(source)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::cp;
    use crate::commands::filesystem::testing::TempDir;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn write(temp: &TempDir, name: &str, contents: &[u8]) -> String {
        let path = temp.arg(name);
        fs::write(&path, contents).expect("failed to write the test file");
        path
    }

    fn mode(path: &str) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn copies_a_file_to_a_new_path() {
        let temp = TempDir::new("cp_file");
        let source = write(&temp, "source.txt", b"contents\n");
        let target = temp.arg("target.txt");
        let mut errors = Vec::new();

        cp(&[source.clone(), target.clone()], &mut errors).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"contents\n");
        assert_eq!(fs::read(&source).unwrap(), b"contents\n");
        assert!(errors.is_empty());
    }

    #[test]
    fn overwrites_an_existing_target_file() {
        let temp = TempDir::new("cp_overwrite");
        let source = write(&temp, "source.txt", b"new\n");
        let target = write(&temp, "target.txt", b"much older and longer\n");
        let mut errors = Vec::new();

        cp(&[source, target.clone()], &mut errors).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"new\n");
        assert!(errors.is_empty());
    }

    #[test]
    fn copies_into_an_existing_directory_keeping_the_basename() {
        let temp = TempDir::new("cp_audit");
        fs::create_dir(temp.arg("new_folder1")).unwrap();
        let target_dir = temp.arg("new_folder2");
        fs::create_dir(&target_dir).unwrap();
        let source = write(&temp, "new_folder1/new_doc.txt", b"audit contents\n");
        let mut errors = Vec::new();

        cp(&[source, target_dir], &mut errors).unwrap();

        let copied = temp.path().join("new_folder2/new_doc.txt");
        assert_eq!(fs::read(copied).unwrap(), b"audit contents\n");
        assert!(errors.is_empty());
    }

    #[test]
    fn copies_several_sources_into_a_directory() {
        let temp = TempDir::new("cp_many");
        let target_dir = temp.arg("into");
        fs::create_dir(&target_dir).unwrap();
        let operands = vec![
            write(&temp, "first.txt", b"first"),
            write(&temp, "second.txt", b"second"),
            target_dir,
        ];
        let mut errors = Vec::new();

        cp(&operands, &mut errors).unwrap();

        assert_eq!(
            fs::read(temp.path().join("into/first.txt")).unwrap(),
            b"first"
        );
        assert_eq!(
            fs::read(temp.path().join("into/second.txt")).unwrap(),
            b"second"
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn preserves_the_permission_bits() {
        let temp = TempDir::new("cp_permissions");
        let source = write(&temp, "script.sh", b"#!/bin/sh\n");
        fs::set_permissions(&source, fs::Permissions::from_mode(0o750)).unwrap();
        let target = temp.arg("copy.sh");
        let mut errors = Vec::new();

        cp(&[source, target.clone()], &mut errors).unwrap();

        assert_eq!(mode(&target), 0o750);
        assert!(errors.is_empty());
    }

    #[test]
    fn reports_a_missing_source_and_copies_the_rest() {
        let temp = TempDir::new("cp_missing");
        let target_dir = temp.arg("into");
        fs::create_dir(&target_dir).unwrap();
        let missing = temp.arg("nope.txt");
        let operands = vec![
            missing.clone(),
            write(&temp, "present.txt", b"present"),
            target_dir,
        ];
        let mut errors = Vec::new();

        cp(&operands, &mut errors).unwrap();

        assert_eq!(
            fs::read(temp.path().join("into/present.txt")).unwrap(),
            b"present"
        );
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            format!("cp: cannot stat '{missing}': No such file or directory\n")
        );
    }

    #[test]
    fn refuses_a_directory_without_the_recursive_flag() {
        let temp = TempDir::new("cp_no_recursive");
        let source = temp.arg("tree");
        fs::create_dir(&source).unwrap();
        let mut errors = Vec::new();

        cp(&[source.clone(), temp.arg("copy")], &mut errors).unwrap();

        assert!(!temp.path().join("copy").exists());
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            format!("cp: -r not specified; omitting directory '{source}'\n")
        );
    }

    #[test]
    fn copies_a_tree_with_the_recursive_flag() {
        let temp = TempDir::new("cp_recursive");
        fs::create_dir(temp.arg("tree")).unwrap();
        fs::create_dir(temp.arg("tree/nested")).unwrap();
        write(&temp, "tree/top.txt", b"top");
        write(&temp, "tree/nested/deep.txt", b"deep");
        fs::set_permissions(temp.arg("tree/nested"), fs::Permissions::from_mode(0o700)).unwrap();
        let mut errors = Vec::new();

        cp(
            &args(&["-r"])
                .into_iter()
                .chain([temp.arg("tree"), temp.arg("copy")])
                .collect::<Vec<_>>(),
            &mut errors,
        )
        .unwrap();

        assert_eq!(fs::read(temp.path().join("copy/top.txt")).unwrap(), b"top");
        assert_eq!(
            fs::read(temp.path().join("copy/nested/deep.txt")).unwrap(),
            b"deep"
        );
        assert_eq!(mode(&temp.arg("copy/nested")), 0o700);
        assert!(errors.is_empty());
    }

    #[test]
    fn accepts_the_capital_recursive_flag_too() {
        let temp = TempDir::new("cp_recursive_capital");
        fs::create_dir(temp.arg("tree")).unwrap();
        write(&temp, "tree/file.txt", b"here");
        let operands = vec!["-R".to_string(), temp.arg("tree"), temp.arg("copy")];
        let mut errors = Vec::new();

        cp(&operands, &mut errors).unwrap();

        assert_eq!(
            fs::read(temp.path().join("copy/file.txt")).unwrap(),
            b"here"
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn recreates_a_symlink_when_recursive_and_follows_it_otherwise() {
        let temp = TempDir::new("cp_symlink");
        write(&temp, "target.txt", b"pointed at\n");
        let link = temp.arg("link");
        std::os::unix::fs::symlink("target.txt", &link).unwrap();
        let mut errors = Vec::new();

        cp(&[link.clone(), temp.arg("followed")], &mut errors).unwrap();
        cp(
            &["-r".to_string(), link, temp.arg("preserved")],
            &mut errors,
        )
        .unwrap();

        assert!(!temp.path().join("followed").is_symlink());
        assert_eq!(
            fs::read(temp.path().join("followed")).unwrap(),
            b"pointed at\n"
        );
        assert!(temp.path().join("preserved").is_symlink());
        assert_eq!(
            fs::read_link(temp.path().join("preserved")).unwrap(),
            std::path::Path::new("target.txt")
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn refuses_to_copy_a_file_onto_itself() {
        let temp = TempDir::new("cp_same_file");
        let source = write(&temp, "source.txt", b"still here\n");
        let same = temp.path().join("./source.txt").display().to_string();
        let mut errors = Vec::new();

        cp(&[source.clone(), same.clone()], &mut errors).unwrap();

        assert_eq!(fs::read(&source).unwrap(), b"still here\n");
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            format!("cp: '{source}' and '{same}' are the same file\n")
        );
    }

    #[test]
    fn refuses_to_copy_a_directory_into_itself() {
        let temp = TempDir::new("cp_into_itself");
        let source = temp.arg("tree");
        fs::create_dir(&source).unwrap();
        write(&temp, "tree/file.txt", b"here");
        let target = temp.arg("tree/copy");
        let mut errors = Vec::new();

        cp(
            &["-r".to_string(), source.clone(), target.clone()],
            &mut errors,
        )
        .unwrap();

        assert!(!temp.path().join("tree/copy").exists());
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            format!("cp: cannot copy a directory, '{source}', into itself, '{target}'\n")
        );
    }

    #[test]
    fn reports_a_plain_target_that_cannot_hold_several_sources() {
        let temp = TempDir::new("cp_bad_target");
        let target = write(&temp, "plain.txt", b"not a directory");
        let operands = vec![
            write(&temp, "first.txt", b"first"),
            write(&temp, "second.txt", b"second"),
            target.clone(),
        ];
        let mut errors = Vec::new();

        cp(&operands, &mut errors).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"not a directory");
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            format!("cp: target '{target}': Not a directory\n")
        );
    }

    #[test]
    fn reports_operand_and_option_mistakes() {
        let mut errors = Vec::new();

        cp(&[], &mut errors).unwrap();
        cp(&args(&["only.txt"]), &mut errors).unwrap();
        cp(&args(&["-q", "a", "b"]), &mut errors).unwrap();
        cp(&args(&["--bogus", "a", "b"]), &mut errors).unwrap();

        assert_eq!(
            String::from_utf8(errors).unwrap(),
            "cp: missing file operand\n\
             cp: missing destination file operand after 'only.txt'\n\
             cp: invalid option -- 'q'\n\
             cp: unrecognized option '--bogus'\n"
        );
    }
}
