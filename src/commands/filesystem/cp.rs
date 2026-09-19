use std::fs::{self, File, FileTimes, Metadata};
use std::io::{self, ErrorKind, Write};
use std::path::Path;

use super::options::{self, Invalid};
use super::transfer::{self, CP, Command};
use crate::commands::report;
use crate::signal;

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
/// `-p` additionally carries over the access and modification times; owner
/// and group stay whatever creating the file gave them, since setting those
/// needs a system call the standard library does not expose and, without
/// root, would only fail. One failing operand is reported and the rest are
/// still copied.
pub fn cp<E>(args: &[String], errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    let parsed = match options::parse(args, "rRp") {
        Ok(parsed) => parsed,
        Err(Invalid::Short(flag)) => return report::invalid_option("cp", flag, errors),
        Err(Invalid::Long(option)) => return report::unrecognized_option("cp", &option, errors),
    };
    let recursive = parsed.has('r') || parsed.has('R');
    let preserve_times = parsed.has('p');

    let (destination, sources) = match parsed.operands.split_last() {
        None => return report::missing_file_operand("cp", errors),
        Some((source, [])) => return report::missing_destination_operand("cp", source, errors),
        Some((destination, sources)) => (Path::new(destination), sources),
    };

    let into_directory = destination.is_dir();
    if sources.len() > 1 && !into_directory {
        return transfer::report_bad_target(CP, destination, errors);
    }

    for source in sources {
        // One Ctrl+C answers for every remaining source: the rest of the tree
        // is not started once the interrupt has arrived.
        if signal::is_pending() {
            break;
        }

        let source = Path::new(source);
        let Some(target) = transfer::target_for(CP, source, destination, into_directory, errors)?
        else {
            continue;
        };

        copy(CP, source, &target, recursive, preserve_times, errors)?;
    }

    Ok(())
}

/// Copies one source to one fully resolved target path.
///
/// The answer is whether the copy is complete, which the caller needs and the
/// `io::Result` does not carry: its `Err` is reserved for a failure to write to
/// the error stream. `mv` deletes the source once a copy across a filesystem
/// boundary has landed, and must not delete it on any other outcome.
pub(super) fn copy<E>(
    command: Command,
    source: &Path,
    target: &Path,
    recursive: bool,
    preserve_times: bool,
    errors: &mut E,
) -> io::Result<bool>
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
            report::io_error(command.name, &subject, &error, errors)?;
            return Ok(false);
        }
    };
    let file_type = metadata.file_type();

    if file_type.is_dir() {
        if !recursive {
            let text = format!(
                "-r not specified; omitting directory {}",
                report::quoted(source)
            );
            report::message(command.name, &text, errors)?;
            return Ok(false);
        }
        return copy_directory(command, source, target, &metadata, preserve_times, errors);
    }

    if file_type.is_symlink() {
        return copy_symlink(command, source, target, errors);
    }

    if file_type.is_file() {
        return copy_file(command, source, target, &metadata, preserve_times, errors);
    }

    // Devices, FIFOs, and sockets need a node the standard library cannot
    // create, and reading a FIFO would hang the shell until something writes
    // to it, so they are skipped rather than half copied.
    let text = format!("omitting special file {}", report::quoted(source));
    report::message(command.name, &text, errors)?;
    Ok(false)
}

/// Copies a regular file, then gives the copy the source's permissions.
fn copy_file<E>(
    command: Command,
    source: &Path,
    target: &Path,
    metadata: &Metadata,
    preserve_times: bool,
    errors: &mut E,
) -> io::Result<bool>
where
    E: Write,
{
    // Creating the target truncates it, so a copy onto the source itself would
    // destroy the very bytes it is about to read.
    if target
        .metadata()
        .is_ok_and(|target| transfer::is_same_file(metadata, &target))
    {
        let text = format!(
            "{} and {} are the same file",
            report::quoted(source),
            report::quoted(target)
        );
        report::message(command.name, &text, errors)?;
        return Ok(false);
    }

    let mut reader = match File::open(source) {
        Ok(file) => file,
        Err(error) => {
            let subject = format!("cannot open {} for reading", report::quoted(source));
            report::io_error(command.name, &subject, &error, errors)?;
            return Ok(false);
        }
    };

    let mut writer = match File::create(target) {
        Ok(file) => file,
        Err(error) => {
            let subject = format!("cannot create regular file {}", report::quoted(target));
            report::io_error(command.name, &subject, &error, errors)?;
            return Ok(false);
        }
    };

    if let Err(error) = io::copy(&mut reader, &mut writer) {
        // The handler does not restart system calls, so an interrupted read
        // wakes as `EINTR`. A Ctrl+C is a silent stop, not a copy failure:
        // whatever reached the target stays there, and no error is invented.
        if signal::is_pending() {
            return Ok(false);
        }
        let subject = format!(
            "cannot {} {} to {}",
            command.verb,
            report::quoted(source),
            report::quoted(target)
        );
        report::io_error(command.name, &subject, &error, errors)?;
        return Ok(false);
    }

    if let Err(error) = writer.set_permissions(metadata.permissions()) {
        let subject = format!("preserving permissions for {}", report::quoted(target));
        report::io_error(command.name, &subject, &error, errors)?;
        return Ok(false);
    }

    if preserve_times && let Err(error) = set_times(&writer, metadata) {
        let subject = format!("preserving timestamps for {}", report::quoted(target));
        report::io_error(command.name, &subject, &error, errors)?;
        return Ok(false);
    }

    Ok(true)
}

/// Gives an open file the source's access and modification times.
///
/// Owner and group are left alone: setting those is `chown`, which has no
/// standard library binding, and without root privilege the kernel would
/// only refuse it anyway, since an ordinary user may not give a file away.
fn set_times(file: &File, metadata: &Metadata) -> io::Result<()> {
    let times = FileTimes::new()
        .set_accessed(metadata.accessed()?)
        .set_modified(metadata.modified()?);
    file.set_times(times)
}

/// Recreates a symlink at the target, pointing at the same place.
fn copy_symlink<E>(
    command: Command,
    source: &Path,
    target: &Path,
    errors: &mut E,
) -> io::Result<bool>
where
    E: Write,
{
    // Recreating the link means removing the target first, so a link that
    // resolves to its own destination would delete the very file it is about
    // to name again: `cp -r link file1` with `link -> file1` leaves `file1` a
    // self-loop and loses the contents. The followed source is measured
    // against the target's own entry, as `copy_file` does for plain files;
    // copying a link onto the link itself is a harmless recreation and passes,
    // because the target's own entry is then the link.
    if source.metadata().is_ok_and(|source| {
        target
            .symlink_metadata()
            .is_ok_and(|target| transfer::is_same_file(&source, &target))
    }) {
        let text = format!(
            "{} and {} are the same file",
            report::quoted(source),
            report::quoted(target)
        );
        report::message(command.name, &text, errors)?;
        return Ok(false);
    }

    let link = match fs::read_link(source) {
        Ok(link) => link,
        Err(error) => {
            let subject = format!("cannot read symbolic link {}", report::quoted(source));
            report::io_error(command.name, &subject, &error, errors)?;
            return Ok(false);
        }
    };

    // `symlink` refuses to overwrite, and a copy is expected to replace what is
    // already there. A target that cannot be removed reports itself below.
    if target.symlink_metadata().is_ok() {
        fs::remove_file(target).ok();
    }

    if let Err(error) = std::os::unix::fs::symlink(&link, target) {
        let subject = format!("cannot create symbolic link {}", report::quoted(target));
        report::io_error(command.name, &subject, &error, errors)?;
        return Ok(false);
    }

    Ok(true)
}

/// Copies a directory and everything under it.
fn copy_directory<E>(
    command: Command,
    source: &Path,
    target: &Path,
    metadata: &Metadata,
    preserve_times: bool,
    errors: &mut E,
) -> io::Result<bool>
where
    E: Write,
{
    if transfer::is_inside(source, target) {
        let text = format!(
            "cannot {} a directory, {}, into itself, {}",
            command.verb,
            report::quoted(source),
            report::quoted(target)
        );
        report::message(command.name, &text, errors)?;
        return Ok(false);
    }

    match fs::create_dir(target) {
        Ok(()) => {}
        // Merging into a directory that is already there is what `cp -r` does.
        Err(error) if error.kind() == ErrorKind::AlreadyExists && target.is_dir() => {}
        Err(error) => {
            let subject = format!("cannot create directory {}", report::quoted(target));
            report::io_error(command.name, &subject, &error, errors)?;
            return Ok(false);
        }
    }

    let entries = match fs::read_dir(source) {
        Ok(entries) => entries,
        Err(error) => {
            let subject = format!("cannot open {} for reading", report::quoted(source));
            report::io_error(command.name, &subject, &error, errors)?;
            return Ok(false);
        }
    };

    // Every entry is attempted even after one fails, so a single unreadable
    // file does not strand the rest of the tree. `mv` needs the running answer:
    // a partial copy must not have its source removed. A Ctrl+C stops the walk
    // right there and answers incomplete, which is exactly what `mv` needs to
    // hear before it deletes its source.
    let mut complete = true;
    for entry in entries {
        if signal::is_pending() {
            return Ok(false);
        }

        match entry {
            Ok(entry) => {
                let target = target.join(entry.file_name());
                complete &= copy(
                    command,
                    &entry.path(),
                    &target,
                    true,
                    preserve_times,
                    errors,
                )?;
            }
            Err(error) => {
                let subject = format!("cannot read directory {}", report::quoted(source));
                report::io_error(command.name, &subject, &error, errors)?;
                complete = false;
            }
        }
    }

    // Permissions come last: a source directory that is not writable would
    // otherwise stop its own contents from being written into the copy.
    if let Err(error) = fs::set_permissions(target, metadata.permissions()) {
        let subject = format!("preserving permissions for {}", report::quoted(target));
        report::io_error(command.name, &subject, &error, errors)?;
        return Ok(false);
    }

    // Timestamps come last for the same reason: writing the entries above
    // just moved the directory's own modification time to now, and that is
    // exactly what this is here to undo.
    if preserve_times {
        let opened = File::open(target).and_then(|file| set_times(&file, metadata));
        if let Err(error) = opened {
            let subject = format!("preserving timestamps for {}", report::quoted(target));
            report::io_error(command.name, &subject, &error, errors)?;
            return Ok(false);
        }
    }

    Ok(complete)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::cp;
    use crate::testing::TempDir;

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
    fn leaves_the_timestamp_at_the_moment_of_copying_without_p() {
        let temp = TempDir::new("cp_no_preserve_times");
        let source = write(&temp, "old.txt", b"aged\n");
        let past = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        fs::File::open(&source)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(past))
            .unwrap();
        let target = temp.arg("copy.txt");
        let mut errors = Vec::new();

        cp(&[source, target.clone()], &mut errors).unwrap();

        assert!(fs::metadata(&target).unwrap().modified().unwrap() > past);
        assert!(errors.is_empty());
    }

    #[test]
    fn p_preserves_the_modification_time_of_a_file() {
        let temp = TempDir::new("cp_p_file");
        let source = write(&temp, "old.txt", b"aged\n");
        let past = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        fs::File::open(&source)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(past))
            .unwrap();
        let target = temp.arg("copy.txt");
        let mut errors = Vec::new();

        cp(&["-p".to_string(), source, target.clone()], &mut errors).unwrap();

        assert_eq!(fs::metadata(&target).unwrap().modified().unwrap(), past);
        assert!(errors.is_empty());
    }

    #[test]
    fn p_preserves_the_modification_time_of_a_directory_copied_recursively() {
        let temp = TempDir::new("cp_p_directory");
        fs::create_dir(temp.arg("tree")).unwrap();
        write(&temp, "tree/inside.txt", b"here");
        let past = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        fs::File::open(temp.arg("tree"))
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(past))
            .unwrap();
        let mut errors = Vec::new();

        cp(
            &args(&["-rp"])
                .into_iter()
                .chain([temp.arg("tree"), temp.arg("copy")])
                .collect::<Vec<_>>(),
            &mut errors,
        )
        .unwrap();

        assert_eq!(
            fs::metadata(temp.arg("copy")).unwrap().modified().unwrap(),
            past
        );
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
    fn refuses_a_recursive_copy_of_a_link_onto_its_own_target() {
        // `link -> file1` copied with `-r` onto `file1` recreates the link by
        // deleting the target first, which would leave `file1` a self-loop and
        // lose its contents.
        let temp = TempDir::new("cp_r_symlink_target");
        let target = write(&temp, "file1", b"precious\n");
        let link = temp.arg("link");
        std::os::unix::fs::symlink("file1", &link).unwrap();
        let mut errors = Vec::new();

        cp(
            &["-r".to_string(), link.clone(), target.clone()],
            &mut errors,
        )
        .unwrap();

        assert_eq!(
            String::from_utf8(errors).unwrap(),
            format!("cp: '{link}' and '{target}' are the same file\n")
        );
        assert_eq!(
            fs::read(&target).unwrap(),
            b"precious\n",
            "target destroyed"
        );
        assert!(
            fs::symlink_metadata(&link).unwrap().is_symlink(),
            "source lost"
        );
    }

    #[test]
    fn copying_a_link_onto_itself_is_a_harmless_recreation() {
        // `cp -r link link` removes and recreates the very same entry, so it
        // must not be refused as a same-file copy.
        let temp = TempDir::new("cp_r_link_self");
        let file = write(&temp, "file1", b"still here\n");
        let link = temp.arg("link");
        std::os::unix::fs::symlink("file1", &link).unwrap();
        let mut errors = Vec::new();

        cp(&["-r".to_string(), link.clone(), link], &mut errors).unwrap();

        assert!(errors.is_empty());
        assert_eq!(fs::read(&file).unwrap(), b"still here\n");
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
