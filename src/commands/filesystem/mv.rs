use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::Path;

use super::options::{self, Invalid};
use super::transfer::{self, MV};
use super::{cp, rm};
use crate::commands::report;

/// Moves or renames each source operand.
///
/// The last operand is the destination. When it is an existing directory each
/// source is moved *into* it under the source's own basename, which is the
/// audited case (`mv new_folder2 new_folder1` puts `new_folder2` inside
/// `new_folder1`); otherwise the single source is renamed to that exact path.
///
/// Files and directories move alike and neither needs a flag, which is where
/// `mv` parts company with `cp` and `rm`: a rename moves a whole tree in one
/// step, so there is nothing for `-r` to switch on.
///
/// A rename cannot cross a filesystem boundary. When the kernel refuses for
/// that reason the move falls back to a recursive copy followed by removing the
/// source, running the same code `cp -r` and `rm -r` do. The source is removed
/// only once the copy is known to be complete, so a fallback that fails partway
/// leaves the original where it is.
///
/// One failing operand is reported and the rest are still moved.
pub fn mv<E>(args: &[String], errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    // `mv` accepts no flags, but the operands still go through the option
    // parser: `-x` has to be refused as an option rather than silently taken
    // for a filename, and `--` has to keep working for a file whose name really
    // does begin with a dash.
    let parsed = match options::parse(args, "") {
        Ok(parsed) => parsed,
        Err(Invalid::Short(flag)) => return report::invalid_option("mv", flag, errors),
        Err(Invalid::Long(option)) => return report::unrecognized_option("mv", &option, errors),
    };

    let (destination, sources) = match parsed.operands.split_last() {
        None => return report::missing_file_operand("mv", errors),
        Some((source, [])) => return report::missing_destination_operand("mv", source, errors),
        Some((destination, sources)) => (Path::new(destination), sources),
    };

    let into_directory = destination.is_dir();
    if sources.len() > 1 && !into_directory {
        return transfer::report_bad_target(MV, destination, errors);
    }

    for source in sources {
        let source = Path::new(source);
        let Some(target) = transfer::target_for(MV, source, destination, into_directory, errors)?
        else {
            continue;
        };

        move_one(source, &target, errors)?;
    }

    Ok(())
}

/// Moves one source to one fully resolved target path.
fn move_one<E>(source: &Path, target: &Path, errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    // The link itself is what moves, so a symlink is never followed to the file
    // it points at: `mv link elsewhere` relocates the link and leaves its
    // target alone.
    let metadata = match source.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) => {
            let subject = format!("cannot stat {}", report::quoted(source));
            return report::io_error("mv", &subject, &error, errors);
        }
    };

    // POSIX defines a rename onto the source itself as a silent success, which
    // would leave the user believing something moved. It is also the shape the
    // mistake `mv dir/file dir` takes, so it is worth saying out loud.
    if target
        .symlink_metadata()
        .is_ok_and(|target| transfer::is_same_file(&metadata, &target))
    {
        let text = format!(
            "{} and {} are the same file",
            report::quoted(source),
            report::quoted(target)
        );
        return report::message("mv", &text, errors);
    }

    // The check above compares the link as itself, which is right for a rename
    // onto the same entry or a hard link to it. A symlink that *resolves* to
    // its destination is the same trap with the follow switched on: for
    // `link -> file.txt`, `mv link file.txt` would replace the file with a
    // link that points at itself, destroying its contents. The followed source
    // is measured against the target's own entry, so moving one link over
    // another that leads to the same file is still allowed.
    if metadata.file_type().is_symlink()
        && target.symlink_metadata().is_ok_and(|target| {
            source
                .metadata()
                .is_ok_and(|source| transfer::is_same_file(&source, &target))
        })
    {
        let text = format!(
            "{} and {} are the same file",
            report::quoted(source),
            report::quoted(target)
        );
        return report::message("mv", &text, errors);
    }

    // A directory cannot swallow itself. `rename` calls this `Invalid
    // argument`, which explains nothing, and the fallback below would otherwise
    // walk a tree that grows as fast as it is read.
    if metadata.is_dir() && transfer::is_inside(source, target) {
        let text = format!(
            "cannot move {} to a subdirectory of itself, {}",
            report::quoted(source),
            report::quoted(target)
        );
        return report::message("mv", &text, errors);
    }

    match fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::CrossesDevices => {
            move_across_filesystems(source, target, errors)
        }
        Err(error) => {
            let subject = format!(
                "cannot move {} to {}",
                report::quoted(source),
                report::quoted(target)
            );
            report::io_error("mv", &subject, &error, errors)
        }
    }
}

/// Moves by copying and then removing, for a target on another filesystem.
///
/// `rename` edits directory entries and cannot span mount points, so a move
/// onto a different filesystem has to be done the long way. Both halves are the
/// code `cp -r` and `rm -r` already run, told to name themselves `mv` so the
/// messages read as the command the user typed.
fn move_across_filesystems<E>(source: &Path, target: &Path, errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    // Always recursive: `mv` moves a directory without being asked twice, and a
    // recursive copy is also the one that recreates a symlink rather than
    // duplicating the file it points at.
    if !cp::copy(MV, source, target, true, errors)? {
        // The copy said why it stopped. Whatever reached the target stays
        // there, as it does under GNU `mv`, but the source is left alone: a
        // half-copied file is not a moved one.
        return Ok(());
    }

    // Removal is unforced, so a source that cannot be deleted is reported
    // rather than passed over in silence. The copy that already landed is not
    // rolled back.
    rm::remove("mv", source, true, false, errors)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};

    use super::mv;
    use crate::testing::{TempDir, WorkingDirectory};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn write(temp: &TempDir, name: &str, contents: &[u8]) -> String {
        let path = temp.arg(name);
        fs::write(&path, contents).expect("failed to write the test file");
        path
    }

    /// Runs `mv` and returns whatever it had to say.
    fn run(operands: &[String]) -> String {
        let mut errors = Vec::new();
        mv(operands, &mut errors).unwrap();
        String::from_utf8(errors).unwrap()
    }

    /// A scratch directory on a filesystem other than the temporary one.
    ///
    /// `mv`'s fallback needs a genuine `EXDEV` from the kernel, which cannot be
    /// simulated. `/dev/shm` is a separate `tmpfs` on a normal Linux system and
    /// usually supplies one. A machine where it does not skips the tests that
    /// ask for it, since the alternative is running them on the rename path
    /// they were written to avoid and reporting a pass that means nothing. The
    /// note is visible under `cargo test -- --nocapture`.
    fn other_filesystem(label: &str) -> Option<TempDir> {
        let shm = Path::new("/dev/shm");
        let elsewhere = shm.is_dir().then(|| TempDir::new_in(shm, label))?;

        let here = std::env::temp_dir().metadata().ok()?.dev();
        let there = elsewhere.path().metadata().ok()?.dev();
        if here == there {
            eprintln!("skipped: {shm:?} is not a separate filesystem here");
            return None;
        }

        Some(elsewhere)
    }

    #[test]
    fn renames_a_file_to_a_new_path() {
        let temp = TempDir::new("mv_rename");
        let source = write(&temp, "source.txt", b"contents\n");
        let target = temp.arg("target.txt");

        let errors = run(&args(&[&source, &target]));

        assert_eq!(fs::read(&target).unwrap(), b"contents\n");
        assert!(fs::symlink_metadata(&source).is_err(), "source survived");
        assert!(errors.is_empty(), "{errors}");
    }

    #[test]
    fn moves_a_directory_into_a_directory_keeping_the_basename() {
        // The audited move: `mv new_folder2 new_folder1`.
        let temp = TempDir::new("mv_audit");
        let first = temp.arg("new_folder1");
        let second = temp.arg("new_folder2");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        fs::write(temp.arg("new_folder2/new_doc.txt"), b"audit contents\n").unwrap();

        let errors = run(&args(&[&second, &first]));

        let moved = temp.path().join("new_folder1/new_folder2/new_doc.txt");
        assert_eq!(fs::read(moved).unwrap(), b"audit contents\n");
        assert!(fs::symlink_metadata(&second).is_err(), "source survived");
        assert!(errors.is_empty(), "{errors}");
    }

    #[test]
    fn moves_a_whole_tree_without_asking_for_a_flag() {
        let temp = TempDir::new("mv_tree");
        let source = temp.arg("tree");
        fs::create_dir_all(temp.arg("tree/inner")).unwrap();
        fs::write(temp.arg("tree/inner/deep.txt"), b"deep\n").unwrap();
        let target = temp.arg("moved");

        let errors = run(&args(&[&source, &target]));

        assert_eq!(
            fs::read(temp.path().join("moved/inner/deep.txt")).unwrap(),
            b"deep\n"
        );
        assert!(fs::symlink_metadata(&source).is_err(), "source survived");
        assert!(errors.is_empty(), "{errors}");
    }

    #[test]
    fn moves_several_sources_into_a_directory() {
        let temp = TempDir::new("mv_many");
        let into = temp.arg("into");
        fs::create_dir(&into).unwrap();
        let first = write(&temp, "first.txt", b"first");
        let second = write(&temp, "second.txt", b"second");

        let errors = run(&args(&[&first, &second, &into]));

        assert_eq!(
            fs::read(temp.path().join("into/first.txt")).unwrap(),
            b"first"
        );
        assert_eq!(
            fs::read(temp.path().join("into/second.txt")).unwrap(),
            b"second"
        );
        assert!(errors.is_empty(), "{errors}");
    }

    #[test]
    fn moves_the_link_rather_than_what_it_points_at() {
        let temp = TempDir::new("mv_symlink");
        let file = write(&temp, "file.txt", b"pointed at\n");
        let link = temp.arg("link");
        std::os::unix::fs::symlink("file.txt", &link).unwrap();
        let target = temp.arg("moved_link");

        let errors = run(&args(&[&link, &target]));

        assert!(fs::symlink_metadata(&target).unwrap().is_symlink());
        assert_eq!(fs::read_link(&target).unwrap(), PathBuf::from("file.txt"));
        assert_eq!(fs::read(&file).unwrap(), b"pointed at\n", "target followed");
        assert!(errors.is_empty(), "{errors}");
    }

    #[test]
    fn refuses_to_move_a_symlink_onto_its_own_target() {
        // `link -> file.txt`, then `mv link file.txt`: a rename would replace
        // the file with a link pointing at itself and destroy its contents.
        let temp = TempDir::new("mv_symlink_target");
        let file = write(&temp, "file.txt", b"precious\n");
        let link = temp.arg("link");
        std::os::unix::fs::symlink("file.txt", &link).unwrap();

        let errors = run(&args(&[&link, &file]));

        assert_eq!(
            errors,
            format!("mv: '{link}' and '{file}' are the same file\n")
        );
        assert_eq!(fs::read(&file).unwrap(), b"precious\n", "target destroyed");
        assert!(
            fs::symlink_metadata(&link).unwrap().is_symlink(),
            "source lost"
        );
    }

    #[test]
    fn moving_one_link_over_another_to_the_same_target_still_works() {
        // A rename onto a link is a rename onto the link's own entry, not what
        // it points at, so two links to one file move over each other freely.
        let temp = TempDir::new("mv_link_over_link");
        let file = write(&temp, "file.txt", b"shared\n");
        let first = temp.arg("first");
        let second = temp.arg("second");
        std::os::unix::fs::symlink("file.txt", &first).unwrap();
        std::os::unix::fs::symlink("file.txt", &second).unwrap();

        let errors = run(&args(&[&first, &second]));

        assert!(errors.is_empty(), "{errors}");
        assert!(fs::symlink_metadata(&second).unwrap().is_symlink());
        assert_eq!(fs::read_link(&second).unwrap(), PathBuf::from("file.txt"));
        assert_eq!(fs::read(&file).unwrap(), b"shared\n");
    }

    #[test]
    fn overwrites_an_existing_target_file() {
        let temp = TempDir::new("mv_overwrite");
        let source = write(&temp, "source.txt", b"new\n");
        let target = write(&temp, "target.txt", b"much older and longer\n");

        let errors = run(&args(&[&source, &target]));

        assert_eq!(fs::read(&target).unwrap(), b"new\n");
        assert!(errors.is_empty(), "{errors}");
    }

    #[test]
    fn falls_back_to_copying_across_a_filesystem_boundary() {
        let Some(elsewhere) = other_filesystem("mv_crossing") else {
            return;
        };
        let temp = TempDir::new("mv_crossing_source");
        fs::create_dir_all(temp.arg("tree/inner")).unwrap();
        fs::write(temp.arg("tree/inner/deep.txt"), b"deep\n").unwrap();
        std::os::unix::fs::symlink("inner/deep.txt", temp.arg("tree/link")).unwrap();
        fs::set_permissions(
            temp.arg("tree/inner/deep.txt"),
            <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o741),
        )
        .unwrap();
        let source = temp.arg("tree");
        let target = elsewhere.arg("tree");

        let errors = run(&args(&[&source, &target]));

        assert!(errors.is_empty(), "{errors}");
        assert_eq!(
            fs::read(elsewhere.path().join("tree/inner/deep.txt")).unwrap(),
            b"deep\n"
        );
        // A move keeps the link a link, and keeps the bits it arrived with.
        let link = elsewhere.path().join("tree/link");
        assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(
            fs::metadata(elsewhere.path().join("tree/inner/deep.txt"))
                .unwrap()
                .mode()
                & 0o777,
            0o741
        );
        assert!(fs::symlink_metadata(&source).is_err(), "source survived");
    }

    #[test]
    fn keeps_the_source_when_a_crossing_copy_cannot_finish() {
        let Some(elsewhere) = other_filesystem("mv_crossing_failure") else {
            return;
        };
        let temp = TempDir::new("mv_crossing_failure_source");
        let source = write(&temp, "source.txt", b"still here\n");
        // A directory nothing may be written into, so the copy half fails and
        // the removal half must not run.
        let closed = elsewhere.arg("closed");
        fs::create_dir(&closed).unwrap();
        fs::set_permissions(
            &closed,
            <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o500),
        )
        .unwrap();

        let errors = run(&args(&[&source, &format!("{closed}/source.txt")]));

        assert!(errors.contains("Permission denied"), "{errors}");
        assert_eq!(
            fs::read(&source).unwrap(),
            b"still here\n",
            "a failed move deleted its source"
        );
    }

    #[test]
    fn reports_missing_operands() {
        assert_eq!(run(&[]), "mv: missing file operand\n");
        assert_eq!(
            run(&args(&["only.txt"])),
            "mv: missing destination file operand after 'only.txt'\n"
        );
    }

    #[test]
    fn reports_a_missing_source_and_moves_the_rest() {
        let temp = TempDir::new("mv_missing");
        let into = temp.arg("into");
        fs::create_dir(&into).unwrap();
        let missing = temp.arg("nope.txt");
        let present = write(&temp, "present.txt", b"present");

        let errors = run(&args(&[&missing, &present, &into]));

        assert_eq!(
            errors,
            format!("mv: cannot stat '{missing}': No such file or directory\n")
        );
        assert_eq!(
            fs::read(temp.path().join("into/present.txt")).unwrap(),
            b"present"
        );
    }

    #[test]
    fn refuses_to_move_a_file_onto_itself() {
        let temp = TempDir::new("mv_same");
        let source = write(&temp, "same.txt", b"untouched\n");

        let errors = run(&args(&[&source, &source]));

        assert_eq!(
            errors,
            format!("mv: '{source}' and '{source}' are the same file\n")
        );
        assert_eq!(fs::read(&source).unwrap(), b"untouched\n");
    }

    #[test]
    fn refuses_to_move_a_directory_into_its_own_subdirectory() {
        let temp = TempDir::new("mv_into_itself");
        let source = temp.arg("tree");
        fs::create_dir_all(temp.arg("tree/inner")).unwrap();
        let target = temp.arg("tree/inner");

        let errors = run(&args(&[&source, &target]));

        assert_eq!(
            errors,
            format!("mv: cannot move '{source}' to a subdirectory of itself, '{target}/tree'\n")
        );
        assert!(fs::metadata(&source).unwrap().is_dir(), "source lost");
    }

    #[test]
    fn refuses_to_move_a_directory_into_a_bare_relative_name_inside_itself() {
        // The shape `mv / here` takes: an absolute source and a target named
        // relative to the working directory, which is itself inside the source.
        // Left unguarded this became a copy of the source into its own
        // subdirectory, which for `/` meant the whole filesystem.
        let temp = TempDir::new("mv_relative_inside");
        let _directory = WorkingDirectory::enter(temp.path());
        let source = temp.path().to_str().unwrap().to_string();

        let errors = run(&args(&[&source, "here"]));

        assert!(
            errors.starts_with(&format!(
                "mv: cannot move '{source}' to a subdirectory of itself"
            )),
            "{errors}"
        );
        assert!(
            fs::symlink_metadata("here").is_err(),
            "the move started anyway"
        );
    }

    #[test]
    fn reports_a_target_that_cannot_hold_several_sources() {
        let temp = TempDir::new("mv_bad_target");
        let first = write(&temp, "first.txt", b"first");
        let second = write(&temp, "second.txt", b"second");
        let target = write(&temp, "target.txt", b"a file, not a directory");

        let errors = run(&args(&[&first, &second, &target]));

        assert_eq!(errors, format!("mv: target '{target}': Not a directory\n"));
        assert_eq!(fs::read(&first).unwrap(), b"first", "source lost");
    }

    #[test]
    fn reports_a_directory_that_is_in_the_way() {
        // Moving into a directory lands on the source's own basename, and here
        // that name is taken by a directory with something in it. `rename`
        // replaces an empty directory but never a full one.
        let temp = TempDir::new("mv_occupied");
        let source = temp.arg("source");
        fs::create_dir(&source).unwrap();
        let target = temp.arg("target");
        fs::create_dir_all(temp.arg("target/source")).unwrap();
        fs::write(temp.arg("target/source/occupant.txt"), b"in the way").unwrap();

        let errors = run(&args(&[&source, &target]));

        assert_eq!(
            errors,
            format!("mv: cannot move '{source}' to '{target}/source': Directory not empty\n")
        );
        assert!(fs::metadata(&source).unwrap().is_dir(), "source lost");
        assert_eq!(
            fs::read(temp.path().join("target/source/occupant.txt")).unwrap(),
            b"in the way"
        );
    }

    #[test]
    fn rejects_an_unknown_flag_rather_than_taking_it_for_a_filename() {
        assert_eq!(
            run(&args(&["-x", "source", "target"])),
            "mv: invalid option -- 'x'\n"
        );
    }

    #[test]
    fn moves_a_file_whose_name_begins_with_a_dash_after_a_double_dash() {
        let temp = TempDir::new("mv_dash");
        let source = write(&temp, "-r", b"dashed\n");
        let target = temp.arg("moved");

        let errors = run(&args(&["--", &source, &target]));

        assert_eq!(fs::read(&target).unwrap(), b"dashed\n");
        assert!(errors.is_empty(), "{errors}");
    }
}
