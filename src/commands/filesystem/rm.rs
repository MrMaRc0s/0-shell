use std::fs;
use std::io::{self, BufRead, ErrorKind, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::confirm::Confirm;
use super::options::{self, Invalid};
use crate::commands::report;
use crate::signal;

/// Removes each named path.
///
/// Files are removed as they are named. A directory needs `-r` (or `-R`), which
/// removes everything under it first, as in Unix. A symlink is unlinked and
/// never followed, so `rm dirlink` removes the link and leaves the directory it
/// points at alone, with or without the flag.
///
/// `-i` asks before every removal, file by file even inside a recursive tree.
/// The answer is read from wherever this command's own standard input is: the
/// file the shell opened with `<`, or the terminal when a person is sitting at
/// one; a piped run with neither answers every question no, the same answer
/// GNU gives a prompt that hits end of input. `-f` silences `-i` too, mirroring
/// the common case where the flag typed last on the command line is the one
/// that wins; this shell does not track which of the two came later.
///
/// One failing operand is reported and the rest are still removed, so
/// `rm present missing other` removes both `present` and `other`.
pub fn rm<E>(
    args: &[String],
    input: Option<&mut dyn BufRead>,
    interactive_terminal: bool,
    errors: &mut E,
) -> io::Result<()>
where
    E: Write,
{
    let parsed = match options::parse(args, "rRfi") {
        Ok(parsed) => parsed,
        Err(Invalid::Short(flag)) => return report::invalid_option("rm", flag, errors),
        Err(Invalid::Long(option)) => return report::unrecognized_option("rm", &option, errors),
    };
    let recursive = parsed.has('r') || parsed.has('R');
    let force = parsed.has('f');
    let interactive = parsed.has('i') && !force;

    if parsed.operands.is_empty() {
        // Never a silent no-op: without operands there is nothing to remove and
        // saying so is the only safe answer. `-f` is the documented exception,
        // since its whole job is to keep quiet about what is not there.
        if force {
            return Ok(());
        }
        return report::missing_operand("rm", errors);
    }

    let stdin = io::stdin();
    // Only the terminal branch below ever fills this in; the `None` it
    // starts as is what keeps it alive long enough to be borrowed from that
    // one arm, not a value meant to be read on its own.
    #[allow(unused_assignments)]
    let mut stdin_lock = None;
    let mut confirm = match (interactive, input) {
        (false, _) => Confirm::Unprompted,
        (true, Some(reader)) => Confirm::Asked(reader),
        (true, None) if interactive_terminal => {
            stdin_lock = Some(stdin.lock());
            Confirm::Asked(stdin_lock.as_mut().unwrap())
        }
        (true, None) => Confirm::Unavailable,
    };

    for operand in &parsed.operands {
        // A Ctrl+C stops the removal at the next operand, so the paths that
        // come after it stay on disk.
        if signal::is_pending() {
            break;
        }

        remove(
            "rm",
            Path::new(operand),
            recursive,
            force,
            &mut confirm,
            errors,
        )?;
    }

    Ok(())
}

/// Removes one path, returning whether it is gone afterwards.
///
/// The caller needs the answer rather than the error: a directory whose
/// contents could not all be removed is not worth an `rmdir` that can only fail
/// a second time with `Directory not empty`, so the child's failure is the only
/// thing reported.
///
/// `command` names the command the messages belong to, since `mv` removes the
/// source of a move across filesystems through this same walk, always
/// unprompted: whatever `-i` asked, it asked before the move started.
pub(super) fn remove<E>(
    command: &str,
    path: &Path,
    recursive: bool,
    force: bool,
    confirm: &mut Confirm,
    errors: &mut E,
) -> io::Result<bool>
where
    E: Write,
{
    // `symlink_metadata` asks about the link itself, so a symlink is a file to
    // be unlinked and never a directory to be walked into.
    let metadata = match path.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) if force && error.kind() == ErrorKind::NotFound => return Ok(true),
        Err(error) => {
            report::io_error(command, &cannot_remove(path), &error, errors)?;
            return Ok(false);
        }
    };

    if metadata.is_dir() {
        if !recursive {
            report::detail(command, &cannot_remove(path), "Is a directory", errors)?;
            return Ok(false);
        }
        return remove_directory(command, path, force, confirm, errors);
    }

    if !confirm.ask(&removal_prompt(command, path, &metadata), errors)? {
        return Ok(false);
    }

    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if force && error.kind() == ErrorKind::NotFound => Ok(true),
        Err(error) => {
            report::io_error(command, &cannot_remove(path), &error, errors)?;
            Ok(false)
        }
    }
}

/// Removes a directory's contents and then the directory itself.
fn remove_directory<E>(
    command: &str,
    path: &Path,
    force: bool,
    confirm: &mut Confirm,
    errors: &mut E,
) -> io::Result<bool>
where
    E: Write,
{
    if let Some(text) = refusal(path) {
        report::message(command, &text, errors)?;
        return Ok(false);
    }

    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if force && error.kind() == ErrorKind::NotFound => return Ok(true),
        Err(error) => {
            report::io_error(command, &cannot_remove(path), &error, errors)?;
            return Ok(false);
        }
    };

    let mut emptied = true;
    for entry in entries {
        // A Ctrl+C stops the walk. The directory is left alone rather than
        // removed half-empty, and the interrupt is reported as nothing more
        // than the new prompt it brings.
        if signal::is_pending() {
            return Ok(false);
        }

        match entry {
            Ok(entry) => {
                emptied &= remove(command, &entry.path(), true, force, confirm, errors)?;
            }
            Err(error) => {
                report::io_error(command, &cannot_remove(path), &error, errors)?;
                emptied = false;
            }
        }
    }

    if !emptied {
        return Ok(false);
    }

    // Asked only once the directory is actually empty, so declining a file
    // inside it does not also ask about removing the directory that file's
    // refusal has already kept from being empty.
    if !confirm.ask(
        &format!("{command}: remove directory {}? ", report::quoted(path)),
        errors,
    )? {
        return Ok(false);
    }

    match fs::remove_dir(path) {
        Ok(()) => Ok(true),
        Err(error) if force && error.kind() == ErrorKind::NotFound => Ok(true),
        Err(error) => {
            report::io_error(command, &cannot_remove(path), &error, errors)?;
            Ok(false)
        }
    }
}

/// The question asked before a file or symlink is unlinked, worded the way
/// GNU words it for each kind.
fn removal_prompt(command: &str, path: &Path, metadata: &fs::Metadata) -> String {
    let kind = if metadata.is_symlink() {
        "symbolic link"
    } else if metadata.len() == 0 {
        "regular empty file"
    } else {
        "regular file"
    };

    format!("{command}: remove {kind} {}? ", report::quoted(path))
}

/// Why a recursive removal must not start, if it must not.
///
/// Kept apart from the walk so the two failsafes can be tested without a test
/// that runs the very removal they exist to prevent.
fn refusal(path: &Path) -> Option<String> {
    // `rm -r .` would empty the working directory while standing in it, and the
    // operand names nothing the user can have meant to delete.
    if ends_in_dot_directory(path) {
        return Some(format!(
            "refusing to remove '.' or '..' directory: skipping {}",
            report::quoted(path)
        ));
    }

    // The failsafe GNU calls `--preserve-root`. The path is resolved first, so
    // `/`, `/usr/..`, and a symlink to `/` are all caught. This shell has no
    // flag to turn the failsafe off, so the advice to use one is left out.
    if path
        .canonicalize()
        .is_ok_and(|resolved| resolved == Path::new("/"))
    {
        // Spelled any way but `/` itself, the message says which path this
        // really is, so `rm -r //` reads as the root removal it would be.
        // Compared as text: `Path` equality is by component, and it reads
        // `//` and `/` as the same path.
        let aliased = if path.as_os_str().as_bytes() == b"/" {
            ""
        } else {
            " (same as '/')"
        };

        return Some(format!(
            "it is dangerous to operate recursively on {}{aliased}",
            report::quoted(path)
        ));
    }

    None
}

/// Whether the last component the user typed is `.` or `..`.
///
/// Read from the path text rather than from `Path::components`, which
/// normalizes an interior `.` away: `components` reports `dir/.` as plain `dir`
/// and would let the failsafe through on the one spelling most likely to be
/// typed by accident.
fn ends_in_dot_directory(path: &Path) -> bool {
    let bytes = path.as_os_str().as_bytes();
    let end = bytes
        .iter()
        .rposition(|byte| *byte != b'/')
        .map_or(0, |index| index + 1);
    let start = bytes[..end]
        .iter()
        .rposition(|byte| *byte == b'/')
        .map_or(0, |index| index + 1);

    matches!(&bytes[start..end], b"." | b"..")
}

/// The subject shared by every `rm` failure.
fn cannot_remove(path: &Path) -> String {
    format!("cannot remove {}", report::quoted(path))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{self, Cursor, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use super::{refusal, rm};
    use crate::testing::TempDir;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn write(temp: &TempDir, name: &str, contents: &[u8]) -> String {
        let path = temp.arg(name);
        fs::write(&path, contents).expect("failed to write the test file");
        path
    }

    /// Runs `rm` the way every test but the `-i` ones needs it: no redirected
    /// input, and not at a terminal either, so `-i` would answer every
    /// question no. The `-i` tests below call `rm` directly instead, to
    /// supply the answers they are checking.
    fn run<E: Write>(args: &[String], errors: &mut E) -> io::Result<()> {
        rm(args, None, false, errors)
    }

    /// Whether directory permissions actually restrict this process.
    ///
    /// Running as root nothing below is refused, so a test built on a directory
    /// that cannot be written has nothing left to prove and says so.
    fn permissions_apply(temp: &TempDir) -> bool {
        let directory = temp.arg("permission_probe");
        fs::create_dir(&directory).unwrap();
        let child = temp.arg("permission_probe/child");
        fs::write(&child, b"probe").unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o500)).unwrap();

        let restricted = fs::remove_file(&child).is_err();

        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        fs::remove_dir_all(&directory).unwrap();
        restricted
    }

    #[test]
    fn removes_a_file() {
        let temp = TempDir::new("rm_file");
        let path = write(&temp, "doomed.txt", b"gone soon");
        let mut errors = Vec::new();

        run(std::slice::from_ref(&path), &mut errors).unwrap();

        assert!(!Path::new(&path).exists());
        assert!(errors.is_empty());
    }

    #[test]
    fn reports_a_missing_path_and_removes_the_rest() {
        let temp = TempDir::new("rm_missing");
        let missing = temp.arg("nope.txt");
        let operands = vec![
            write(&temp, "first.txt", b"first"),
            missing.clone(),
            write(&temp, "last.txt", b"last"),
        ];
        let mut errors = Vec::new();

        run(&operands, &mut errors).unwrap();

        assert!(!temp.path().join("first.txt").exists());
        assert!(!temp.path().join("last.txt").exists());
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            format!("rm: cannot remove '{missing}': No such file or directory\n")
        );
    }

    #[test]
    fn refuses_a_directory_without_the_recursive_flag() {
        let temp = TempDir::new("rm_directory");
        let path = temp.arg("keep");
        fs::create_dir(&path).unwrap();
        let mut errors = Vec::new();

        run(std::slice::from_ref(&path), &mut errors).unwrap();

        assert!(Path::new(&path).is_dir());
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            format!("rm: cannot remove '{path}': Is a directory\n")
        );
    }

    #[test]
    fn removes_a_tree_with_the_recursive_flag() {
        let temp = TempDir::new("rm_recursive");
        fs::create_dir(temp.arg("new_folder1")).unwrap();
        fs::create_dir(temp.arg("new_folder1/new_folder2")).unwrap();
        write(&temp, "new_folder1/new_doc.txt", b"audit contents");
        write(&temp, "new_folder1/new_folder2/new_doc.txt", b"copied");
        let operands = vec!["-r".to_string(), temp.arg("new_folder1")];
        let mut errors = Vec::new();

        run(&operands, &mut errors).unwrap();

        assert!(!temp.path().join("new_folder1").exists());
        assert!(errors.is_empty());
    }

    #[test]
    fn accepts_the_capital_recursive_flag_too() {
        let temp = TempDir::new("rm_recursive_capital");
        fs::create_dir(temp.arg("tree")).unwrap();
        write(&temp, "tree/file.txt", b"here");
        let operands = vec!["-R".to_string(), temp.arg("tree")];
        let mut errors = Vec::new();

        run(&operands, &mut errors).unwrap();

        assert!(!temp.path().join("tree").exists());
        assert!(errors.is_empty());
    }

    #[test]
    fn unlinks_a_symlink_without_touching_what_it_points_at() {
        let temp = TempDir::new("rm_symlink");
        let directory = temp.arg("real");
        fs::create_dir(&directory).unwrap();
        write(&temp, "real/kept.txt", b"kept");
        let link = temp.arg("link");
        std::os::unix::fs::symlink(&directory, &link).unwrap();
        let mut errors = Vec::new();

        run(std::slice::from_ref(&link), &mut errors).unwrap();

        assert!(!Path::new(&link).is_symlink());
        assert_eq!(
            fs::read(temp.path().join("real/kept.txt")).unwrap(),
            b"kept"
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn leaves_a_directory_alone_when_a_child_cannot_be_removed() {
        let temp = TempDir::new("rm_blocked_child");
        if !permissions_apply(&temp) {
            return;
        }

        let directory = temp.arg("locked");
        fs::create_dir(&directory).unwrap();
        let child = temp.arg("locked/child.txt");
        fs::write(&child, b"stuck").unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o500)).unwrap();
        let mut errors = Vec::new();

        run(&["-r".to_string(), directory.clone()], &mut errors).unwrap();

        // Only the child is reported: an `rmdir` of a directory that could not
        // be emptied would add a second failure saying the same thing.
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            format!("rm: cannot remove '{child}': Permission denied\n")
        );
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[test]
    fn force_ignores_what_is_not_there() {
        let temp = TempDir::new("rm_force");
        let present = write(&temp, "present.txt", b"present");
        let operands = vec!["-f".to_string(), temp.arg("nope.txt"), present.clone()];
        let mut errors = Vec::new();

        run(&operands, &mut errors).unwrap();
        run(&args(&["-f"]), &mut errors).unwrap();

        assert!(!Path::new(&present).exists());
        assert!(errors.is_empty());
    }

    #[test]
    fn force_still_refuses_a_directory_without_the_recursive_flag() {
        let temp = TempDir::new("rm_force_directory");
        let path = temp.arg("keep");
        fs::create_dir(&path).unwrap();
        let mut errors = Vec::new();

        run(&["-f".to_string(), path.clone()], &mut errors).unwrap();

        assert!(Path::new(&path).is_dir());
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            format!("rm: cannot remove '{path}': Is a directory\n")
        );
    }

    #[test]
    fn refuses_a_dot_directory_instead_of_emptying_it() {
        let temp = TempDir::new("rm_dot");
        write(&temp, "kept.txt", b"kept");
        let operand = temp.arg(".");
        let mut errors = Vec::new();

        run(&["-r".to_string(), operand.clone()], &mut errors).unwrap();

        assert_eq!(fs::read(temp.path().join("kept.txt")).unwrap(), b"kept");
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            format!("rm: refusing to remove '.' or '..' directory: skipping '{operand}'\n")
        );
    }

    #[test]
    fn the_failsafes_name_what_they_refuse() {
        // Checked on the function rather than through a removal, so the suite
        // never runs the command these two exist to stop.
        assert_eq!(
            refusal(Path::new("/")),
            Some("it is dangerous to operate recursively on '/'".to_string())
        );
        assert_eq!(
            refusal(Path::new("..")),
            Some("refusing to remove '.' or '..' directory: skipping '..'".to_string())
        );
        assert_eq!(
            refusal(Path::new("//")),
            Some("it is dangerous to operate recursively on '//' (same as '/')".to_string()),
            "the root failsafe resolves the path instead of reading its text"
        );
        assert_eq!(
            refusal(Path::new("/tmp/ordinary_directory")),
            None,
            "an ordinary path must not be refused"
        );
    }

    #[test]
    fn reports_operand_and_option_mistakes() {
        let mut errors = Vec::new();

        run(&[], &mut errors).unwrap();
        run(&args(&["-q", "path"]), &mut errors).unwrap();
        run(&args(&["--bogus", "path"]), &mut errors).unwrap();

        let help = "Try 'rm --help' for more information.\n";
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            format!(
                "rm: missing operand\n{help}\
                 rm: invalid option -- 'q'\n{help}\
                 rm: unrecognized option '--bogus'\n{help}"
            )
        );
    }

    #[test]
    fn interactive_removes_a_file_the_answer_agrees_to() {
        let temp = TempDir::new("rm_i_yes");
        let path = write(&temp, "doomed.txt", b"gone soon");
        let mut input = Cursor::new(b"y\n".to_vec());
        let mut errors = Vec::new();

        rm(&args(&["-i", &path]), Some(&mut input), false, &mut errors).unwrap();

        assert!(!Path::new(&path).exists());
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            format!("rm: remove regular file '{path}'? ")
        );
    }

    #[test]
    fn interactive_keeps_a_file_the_answer_declines() {
        let temp = TempDir::new("rm_i_no");
        let path = write(&temp, "spared.txt", b"kept");
        let mut input = Cursor::new(b"n\n".to_vec());
        let mut errors = Vec::new();

        rm(&args(&["-i", &path]), Some(&mut input), false, &mut errors).unwrap();

        assert!(Path::new(&path).exists());
        assert_eq!(
            String::from_utf8(errors).unwrap(),
            format!("rm: remove regular file '{path}'? ")
        );
    }

    #[test]
    fn interactive_asks_once_per_file_in_a_recursive_tree() {
        let temp = TempDir::new("rm_i_recursive");
        fs::create_dir(temp.arg("tree")).unwrap();
        write(&temp, "tree/a.txt", b"a");
        write(&temp, "tree/b.txt", b"b");
        // Every question answered yes: two files, then the emptied directory.
        let mut input = Cursor::new(b"y\ny\ny\n".to_vec());
        let mut errors = Vec::new();

        rm(
            &args(&["-ri", &temp.arg("tree")]),
            Some(&mut input),
            false,
            &mut errors,
        )
        .unwrap();

        assert!(!temp.path().join("tree").exists());
        assert_eq!(
            String::from_utf8(errors).unwrap().matches('?').count(),
            3,
            "one question per file and one for the directory"
        );
    }

    #[test]
    fn interactive_leaves_a_directory_behind_when_a_file_inside_it_is_declined() {
        let temp = TempDir::new("rm_i_recursive_decline");
        fs::create_dir(temp.arg("tree")).unwrap();
        write(&temp, "tree/keep.txt", b"kept");
        let mut input = Cursor::new(b"n\n".to_vec());
        let mut errors = Vec::new();

        rm(
            &args(&["-ri", &temp.arg("tree")]),
            Some(&mut input),
            false,
            &mut errors,
        )
        .unwrap();

        assert!(temp.path().join("tree").exists());
        assert!(temp.path().join("tree/keep.txt").exists());
    }

    #[test]
    fn force_silences_interactive_even_when_both_are_given() {
        let temp = TempDir::new("rm_i_force");
        let path = write(&temp, "doomed.txt", b"gone soon");
        let mut errors = Vec::new();

        // No input at all: if `-f` did not silence `-i` here, reading a
        // question's answer would find nothing and the file would survive.
        rm(&args(&["-fi", &path]), None, false, &mut errors).unwrap();

        assert!(!Path::new(&path).exists());
        assert!(errors.is_empty());
    }

    #[test]
    fn interactive_without_a_source_to_ask_declines_without_prompting() {
        let temp = TempDir::new("rm_i_unavailable");
        let path = write(&temp, "spared.txt", b"kept");
        let mut errors = Vec::new();

        // No redirected input, and not at a terminal either: the situation a
        // piped, non-interactive run with `-i` is actually in.
        rm(&args(&["-i", &path]), None, false, &mut errors).unwrap();

        assert!(Path::new(&path).exists());
        assert!(errors.is_empty(), "nothing to ask, so nothing is printed");
    }
}
