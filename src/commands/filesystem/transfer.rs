//! The operand handling and safety guards shared by `cp` and `mv`.
//!
//! Both commands read the same operand shape, one or more sources followed by
//! a destination, and both have to answer the same questions before they touch
//! anything: where does each source land, can this destination hold more than
//! one of them, and is the transfer secretly a no-op onto the source itself.
//! Only the wording differs, so the command doing the asking is passed in.

use std::fs::Metadata;
use std::io::{self, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::commands::report;

/// The command a shared helper is writing messages for.
///
/// GNU's `cp` and `mv` share their transfer code and differ in what they call
/// the transfer, so the verb travels with the name.
#[derive(Clone, Copy)]
pub struct Command {
    /// The name the user typed, which prefixes every message.
    pub name: &'static str,
    /// The verb the messages use for the transfer itself.
    pub verb: &'static str,
}

pub const CP: Command = Command {
    name: "cp",
    verb: "copy",
};

pub const MV: Command = Command {
    name: "mv",
    verb: "move",
};

/// Where one source lands under the destination.
///
/// A destination that is an existing directory receives the source under the
/// source's own basename, which is the audited case for both commands
/// (`cp new_folder1/new_doc.txt new_folder2` and `mv new_folder2 new_folder1`).
/// Any other destination is the target path itself.
///
/// `Ok(None)` means the source has no basename to join, as `..` and a bare `/`
/// do not, and the reason has been reported.
pub fn target_for<E>(
    command: Command,
    source: &Path,
    destination: &Path,
    into_directory: bool,
    errors: &mut E,
) -> io::Result<Option<PathBuf>>
where
    E: Write,
{
    if !into_directory {
        return Ok(Some(destination.to_path_buf()));
    }

    match source.file_name() {
        Some(name) => Ok(Some(destination.join(name))),
        None => {
            let subject = format!(
                "cannot {} {} into {}",
                command.verb,
                report::quoted(source),
                report::quoted(destination)
            );
            report::detail(command.name, &subject, "Invalid argument", errors)?;
            Ok(None)
        }
    }
}

/// Explains why several sources cannot go to this destination.
///
/// The destination has to be a directory to hold them, so it either exists and
/// is something else, or the reason it is not a directory is the OS's own.
pub fn report_bad_target<E>(command: Command, destination: &Path, errors: &mut E) -> io::Result<()>
where
    E: Write,
{
    let subject = format!("target {}", report::quoted(destination));

    match destination.symlink_metadata() {
        Ok(_) => report::detail(command.name, &subject, "Not a directory", errors),
        Err(error) => report::io_error(command.name, &subject, &error, errors),
    }
}

/// Whether two stats describe one and the same file.
///
/// Compared by device and inode rather than by path text, so `a` and `./a`, and
/// a hard link to either, are all recognised as the same file. Which stat the
/// caller takes decides whether a symlink counts as itself or as its target:
/// `cp` copies what a link points at, while `mv` moves the link.
pub fn is_same_file(left: &Metadata, right: &Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino()
}

/// Whether the target sits inside the source directory.
///
/// `cp -r dir dir/copy` would otherwise copy the growing tree into itself and
/// never finish, and `mv dir dir/moved` would be asked to consume its own
/// destination. Both paths are resolved first so that `.`, `..`, and links
/// cannot hide the nesting. A target that does not exist yet is resolved
/// through its parent, which does.
///
/// The comparison is by path component, not by text, so `dir2` is not mistaken
/// for something inside `dir`.
pub fn is_inside(source: &Path, target: &Path) -> bool {
    let Ok(source) = source.canonicalize() else {
        return false;
    };

    let target = match target.canonicalize() {
        Ok(target) => target,
        Err(_) => match (target.parent(), target.file_name()) {
            (Some(parent), Some(name)) => match resolve_parent(parent) {
                Some(parent) => parent.join(name),
                None => return false,
            },
            _ => return false,
        },
    };

    target.starts_with(source)
}

/// Resolves the directory a not-yet-existing target would be created in.
///
/// A bare relative name such as `here` has `""` for a parent, which names the
/// working directory rather than nothing at all. Reading it as nothing is how
/// `mv / here` once slipped past [`is_inside`]: every path is inside `/`, the
/// guard said otherwise, and the cross-device fallback set about copying the
/// whole filesystem into a subdirectory of itself.
fn resolve_parent(parent: &Path) -> Option<PathBuf> {
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };

    parent.canonicalize().ok()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::is_inside;
    use crate::testing::{TempDir, WorkingDirectory};

    #[test]
    fn every_path_is_inside_the_root_directory() {
        // The regression that matters: a bare relative name has an empty
        // parent, and reading that as "nowhere" let `mv / here` past the guard
        // and into copying the filesystem into itself.
        assert!(is_inside(Path::new("/"), Path::new("here")));
        assert!(is_inside(Path::new("/"), Path::new("/tmp/here")));
        assert!(is_inside(Path::new("/"), Path::new("/")));
    }

    #[test]
    fn a_bare_relative_target_is_resolved_against_the_working_directory() {
        let temp = TempDir::new("inside_relative");
        let _directory = WorkingDirectory::enter(temp.path());
        fs::create_dir("tree").unwrap();

        // `tree/here` is inside `tree`; `here` beside it is not.
        assert!(is_inside(Path::new("tree"), Path::new("tree/here")));
        assert!(!is_inside(Path::new("tree"), Path::new("here")));
    }

    #[test]
    fn a_sibling_sharing_a_name_prefix_is_not_inside() {
        let temp = TempDir::new("inside_prefix");
        let directory = temp.arg("dir");
        fs::create_dir(&directory).unwrap();

        // Compared by component, so `dir2` is not read as `dir` plus `2`.
        assert!(!is_inside(
            Path::new(&directory),
            Path::new(&temp.arg("dir2"))
        ));
        assert!(is_inside(
            Path::new(&directory),
            Path::new(&temp.arg("dir/sub"))
        ));
    }

    #[test]
    fn dots_and_links_cannot_hide_the_nesting() {
        let temp = TempDir::new("inside_dots");
        let directory = temp.arg("dir");
        fs::create_dir_all(temp.arg("dir/sub")).unwrap();
        std::os::unix::fs::symlink(&directory, temp.arg("link")).unwrap();

        assert!(is_inside(
            Path::new(&directory),
            Path::new(&temp.arg("dir/sub/../sub/deeper"))
        ));
        assert!(is_inside(
            Path::new(&directory),
            Path::new(&temp.arg("link/deeper"))
        ));
    }
}
