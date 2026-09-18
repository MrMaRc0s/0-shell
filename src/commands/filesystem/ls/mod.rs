mod long;
mod time;
mod users;

use std::ffi::OsString;
use std::fs::{self, Metadata};
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};

use super::options::{self, Invalid};
use crate::commands::report;
use long::Item;

/// The escape sequence that ends a colored name. GNU coreutils uses `\e[0m`.
pub(super) const RESET: &[u8] = b"\x1b[0m";

/// The escape sequences that begin a colored name, GNU coreutils' defaults.
const DIRECTORY: &[u8] = b"\x1b[01;34m";
const SYMLINK: &[u8] = b"\x1b[01;36m";
const FIFO: &[u8] = b"\x1b[33m";
const SOCKET: &[u8] = b"\x1b[01;35m";
const DEVICE: &[u8] = b"\x1b[01;33m";
const EXECUTABLE: &[u8] = b"\x1b[01;32m";

/// The color an entry should be printed in, or none.
///
/// This is not the whole of GNU's scheme, only the defaults that matter in a
/// listing: directories, links, FIFOs, sockets, the special device files, and
/// executables. Plain files carry nothing, exactly as they do on the host.
pub(super) fn style_of(metadata: &Metadata, colored: bool) -> Option<&'static [u8]> {
    if !colored {
        return None;
    }

    let file_type = metadata.file_type();

    if file_type.is_symlink() {
        Some(SYMLINK)
    } else if file_type.is_dir() {
        Some(DIRECTORY)
    } else if file_type.is_fifo() {
        Some(FIFO)
    } else if file_type.is_socket() {
        Some(SOCKET)
    } else if file_type.is_char_device() || file_type.is_block_device() {
        Some(DEVICE)
    } else if file_type.is_file() && metadata.permissions().mode() & 0o111 != 0 {
        Some(EXECUTABLE)
    } else {
        None
    }
}

/// Lists the named directories and files, or the working directory.
///
/// Entries are printed one per line, sorted by name, with dotfiles hidden.
/// Several operands are grouped the way Unix groups them: the files first as
/// one block, then a block per directory under a `<name>:` header, with a blank
/// line between blocks. Operands that cannot be reached are reported and the
/// rest are still listed.
///
/// `-a` adds the hidden entries, `.` and `..` among them. `-F` appends the type
/// indicator: `/` for a directory, `*` for an executable file, `@` for a
/// symlink, `|` for a FIFO, and `=` for a socket. `-l` prints the long format,
/// a `total` line and one aligned row per entry. The flags combine in any
/// spelling: `ls -l -a -F`, `ls -laF`, and `ls -al` are the same command.
///
/// `colored` turns on GNU's name colors, and only ever for a real terminal. A
/// piped listing and a redirected one stay byte-identical to the host tool.
pub fn ls<W, E>(args: &[String], output: &mut W, errors: &mut E, colored: bool) -> io::Result<()>
where
    W: Write,
    E: Write,
{
    let parsed = match options::parse(args, "aFl") {
        Ok(parsed) => parsed,
        Err(Invalid::Short(flag)) => return report::invalid_option("ls", flag, errors),
        Err(Invalid::Long(option)) => return report::unrecognized_option("ls", &option, errors),
    };

    let working_directory = [".".to_string()];
    let operands: &[String] = if parsed.operands.is_empty() {
        &working_directory
    } else {
        &parsed.operands
    };

    // Operands are sorted and split before anything is printed, because a
    // listing shows every file operand together and then each directory, in
    // name order, whatever order they were typed in.
    let all = parsed.has('a');
    let classify = parsed.has('F');
    let long = parsed.has('l');
    // Both flags describe the operand itself, so neither follows a symlink
    // named on the command line.
    let name_the_link = classify || long;

    let mut files = Vec::new();
    let mut directories = Vec::new();
    for operand in operands {
        // A symlink operand is normally followed, so `ls dirlink` lists the
        // directory the link points at. `-F` turns that off, as in Unix: the
        // whole point of the flag is to say what the operand itself is, and
        // `ls -F dirlink` prints `dirlink@`.
        let path = Path::new(operand);
        let metadata = if name_the_link {
            path.symlink_metadata()
        } else {
            // A symlink with nothing at the end of it still names something
            // that is there, so the link itself is described rather than
            // reported missing. This is the host's behavior; GNU `ls` prints
            // `cannot access` here instead.
            path.metadata()
                .or_else(|error| match path.symlink_metadata() {
                    Ok(metadata) => Ok(metadata),
                    Err(_) => Err(error),
                })
        };

        match metadata {
            Ok(metadata) if metadata.is_dir() => directories.push(operand),
            Ok(metadata) => files.push((
                operand,
                indicator(&metadata, classify),
                style_of(&metadata, colored),
            )),
            Err(error) => {
                let subject = format!("cannot access {}", report::quoted(operand));
                report::io_error("ls", &subject, &error, errors)?;
            }
        }
    }
    files.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    directories.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));

    // Headers are for telling blocks apart, so one operand needs none, even
    // when the others failed: `ls missing dir` still heads the `dir` block.
    let headers = operands.len() > 1;
    let mut printed = false;

    if !files.is_empty() {
        if long {
            let items = files
                .iter()
                .map(|(file, _, style)| Item {
                    display: file.as_bytes().to_vec(),
                    path: PathBuf::from(file),
                    style: *style,
                })
                .collect::<Vec<_>>();
            long::write(&items, false, classify, colored, output, errors)?;
        } else {
            for (file, indicator, style) in files {
                write_line(file.as_bytes(), indicator, style, output)?;
            }
        }
        printed = true;
    }

    for directory in directories {
        let path = Path::new(directory);
        let names = match read_names(path, all) {
            Ok(names) => names,
            Err(error) => {
                // Reported before any header is written, so a directory that
                // cannot be read leaves no empty block behind.
                let subject = format!("cannot open directory {}", report::quoted(directory));
                report::io_error("ls", &subject, &error, errors)?;
                continue;
            }
        };

        if printed {
            writeln!(output)?;
        }
        if headers {
            output.write_all(directory.as_bytes())?;
            output.write_all(b":\n")?;
        }
        printed = true;

        if long {
            let items = names
                .iter()
                .map(|name| Item {
                    display: name.as_bytes().to_vec(),
                    path: path.join(name),
                    style: None,
                })
                .collect::<Vec<_>>();
            long::write(&items, true, classify, colored, output, errors)?;
        } else {
            for name in names {
                let path = path.join(&name);
                let metadata = path.symlink_metadata().ok();
                let indicator = metadata
                    .as_ref()
                    .map(|metadata| indicator(metadata, classify))
                    .unwrap_or("");
                let style = metadata
                    .as_ref()
                    .and_then(|metadata| style_of(metadata, colored));

                write_line(name.as_bytes(), indicator, style, output)?;
            }
        }
    }

    Ok(())
}

/// The entries of one directory, in name order.
fn read_names(directory: &Path, all: bool) -> io::Result<Vec<OsString>> {
    let mut names = Vec::new();

    // `read_dir` never yields `.` or `..`, so `-a` has to name them itself.
    // They sort in with everything else rather than leading the listing: `.` is
    // byte 0x2e, so a name like `!bang` still comes first, as it does on the
    // host.
    if all {
        names.push(OsString::from("."));
        names.push(OsString::from(".."));
    }

    for entry in fs::read_dir(directory)? {
        let name = entry?.file_name();
        if !all && name.as_bytes().first() == Some(&b'.') {
            continue;
        }
        names.push(name);
    }

    // Sorted by raw bytes, which is the order `ls` produces under `LC_ALL=C`
    // and the order the host tool produces here. Names are not text: they are
    // byte strings that need not be valid UTF-8.
    names.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));

    Ok(names)
}

/// Writes one name, its `-F` indicator, and a newline, as bytes.
///
/// A colored name is wrapped between its color and the reset sequence, with
/// the indicator inside the color, which is how GNU's `ls` draws it.
fn write_line<W>(
    name: &[u8],
    indicator: &str,
    style: Option<&'static [u8]>,
    output: &mut W,
) -> io::Result<()>
where
    W: Write,
{
    if let Some(color) = style {
        output.write_all(color)?;
    }
    output.write_all(name)?;
    output.write_all(indicator.as_bytes())?;
    if style.is_some() {
        output.write_all(RESET)?;
    }
    output.write_all(b"\n")
}

/// The character `-F` appends to a name of this kind.
pub(super) fn indicator(metadata: &Metadata, classify: bool) -> &'static str {
    if !classify {
        return "";
    }

    let file_type = metadata.file_type();

    if file_type.is_symlink() {
        "@"
    } else if file_type.is_dir() {
        "/"
    } else if file_type.is_fifo() {
        "|"
    } else if file_type.is_socket() {
        "="
    } else if file_type.is_file() && metadata.permissions().mode() & 0o111 != 0 {
        // Any of the three execute bits, whether or not this user holds it,
        // which is what `ls` marks.
        "*"
    } else {
        // Regular files, block and character devices carry no mark.
        ""
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::ls;
    use crate::testing::TempDir;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn touch(temp: &TempDir, name: &str) -> String {
        let path = temp.arg(name);
        fs::write(&path, b"").expect("failed to write the test file");
        path
    }

    fn listing<W: AsRef<[String]>>(operands: W) -> (String, String) {
        let mut output = Vec::new();
        let mut errors = Vec::new();

        ls(operands.as_ref(), &mut output, &mut errors, false).unwrap();

        (
            String::from_utf8(output).unwrap(),
            String::from_utf8(errors).unwrap(),
        )
    }

    fn listing_colored<W: AsRef<[String]>>(operands: W) -> (String, String) {
        let mut output = Vec::new();
        let mut errors = Vec::new();

        ls(operands.as_ref(), &mut output, &mut errors, true).unwrap();

        (
            String::from_utf8(output).unwrap(),
            String::from_utf8(errors).unwrap(),
        )
    }

    #[test]
    fn lists_one_entry_per_line_sorted_by_name() {
        let temp = TempDir::new("ls_sorted");
        for name in ["Zebra", "apple", "Banana", "_under", "b file", "1num"] {
            touch(&temp, name);
        }

        let (output, errors) = listing(vec![temp.arg("")]);

        assert_eq!(output, "1num\nBanana\nZebra\n_under\napple\nb file\n");
        assert!(errors.is_empty());
    }

    #[test]
    fn hides_dotfiles() {
        let temp = TempDir::new("ls_hidden");
        touch(&temp, ".hidden");
        touch(&temp, "visible");
        fs::create_dir(temp.arg(".hiddendir")).unwrap();

        let (output, errors) = listing(vec![temp.arg("")]);

        assert_eq!(output, "visible\n");
        assert!(errors.is_empty());
    }

    #[test]
    fn prints_the_name_of_a_file_operand() {
        let temp = TempDir::new("ls_file");
        let path = touch(&temp, "one.txt");

        let (output, errors) = listing(vec![path.clone()]);

        assert_eq!(output, format!("{path}\n"));
        assert!(errors.is_empty());
    }

    #[test]
    fn prints_no_header_for_a_single_directory() {
        let temp = TempDir::new("ls_single_directory");
        touch(&temp, "inside.txt");

        let (output, errors) = listing(vec![temp.arg("")]);

        assert_eq!(output, "inside.txt\n");
        assert!(errors.is_empty());
    }

    #[test]
    fn groups_files_first_then_directories_under_headers() {
        let temp = TempDir::new("ls_groups");
        let file = touch(&temp, "plain.txt");
        let first = temp.arg("d1");
        let second = temp.arg("d2");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        touch(&temp, "d1/inside");

        let (output, errors) = listing(vec![second.clone(), file.clone(), first.clone()]);

        assert_eq!(output, format!("{file}\n\n{first}:\ninside\n\n{second}:\n"));
        assert!(errors.is_empty());
    }

    #[test]
    fn lists_an_empty_directory_as_nothing() {
        let temp = TempDir::new("ls_empty");

        let (output, errors) = listing(vec![temp.arg("")]);

        assert!(output.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn follows_a_symlink_to_a_directory() {
        let temp = TempDir::new("ls_symlink");
        let directory = temp.arg("real");
        fs::create_dir(&directory).unwrap();
        touch(&temp, "real/inside");
        let link = temp.arg("link");
        std::os::unix::fs::symlink(&directory, &link).unwrap();

        let (output, errors) = listing(vec![link]);

        assert_eq!(output, "inside\n");
        assert!(errors.is_empty());
    }

    #[test]
    fn reports_an_operand_that_cannot_be_reached_and_lists_the_rest() {
        let temp = TempDir::new("ls_missing");
        let missing = temp.arg("nope");
        let directory = temp.arg("present");
        fs::create_dir(&directory).unwrap();
        touch(&temp, "present/inside");

        let (output, errors) = listing(vec![missing.clone(), directory.clone()]);

        // The header stays: it is the second of two operands, whatever became
        // of the first.
        assert_eq!(output, format!("{directory}:\ninside\n"));
        assert_eq!(
            errors,
            format!("ls: cannot access '{missing}': No such file or directory\n")
        );
    }

    #[test]
    fn names_a_symlink_that_points_at_nothing() {
        let temp = TempDir::new("ls_dangling");
        let link = temp.arg("dangling");
        std::os::unix::fs::symlink(temp.arg("nowhere"), &link).unwrap();

        let (named, errors) = listing(vec![link.clone()]);
        let (classified, _) = listing(vec!["-F".to_string(), link.clone()]);
        let (missing_output, missing_errors) = listing(vec![temp.arg("nowhere")]);

        assert_eq!(named, format!("{link}\n"));
        assert_eq!(classified, format!("{link}@\n"));
        assert!(errors.is_empty());

        // A name with nothing behind it at all is still an error.
        assert!(missing_output.is_empty());
        assert_eq!(
            missing_errors,
            format!(
                "ls: cannot access '{}': No such file or directory\n",
                temp.arg("nowhere")
            )
        );
    }

    #[test]
    fn reports_a_directory_it_cannot_open_without_leaving_a_header() {
        let temp = TempDir::new("ls_unreadable");
        let locked = temp.arg("locked");
        fs::create_dir(&locked).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let readable = temp.arg("readable");
        fs::create_dir(&readable).unwrap();
        touch(&temp, "readable/inside");

        let (output, errors) = listing(vec![locked.clone(), readable.clone()]);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();

        if errors.is_empty() {
            // Running as root nothing is unreadable, so there is nothing to test.
            return;
        }

        assert_eq!(output, format!("{readable}:\ninside\n"));
        assert_eq!(
            errors,
            format!("ls: cannot open directory '{locked}': Permission denied\n")
        );
    }

    /// A directory holding one of every kind of entry `-F` can mark.
    fn every_kind(temp: &TempDir) -> String {
        let directory = temp.arg("kinds");
        fs::create_dir(&directory).unwrap();
        fs::create_dir(temp.arg("kinds/sub")).unwrap();
        touch(temp, "kinds/plain");
        touch(temp, "kinds/.hidden");
        let script = touch(temp, "kinds/script");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink("sub", temp.arg("kinds/dirlink")).unwrap();
        std::os::unix::fs::symlink("nowhere", temp.arg("kinds/dangling")).unwrap();

        directory
    }

    #[test]
    fn all_includes_the_dot_entries_and_dotfiles_in_name_order() {
        let temp = TempDir::new("ls_all");
        touch(&temp, ".hidden");
        touch(&temp, "visible");
        touch(&temp, "!bang");

        let (output, errors) = listing(vec!["-a".to_string(), temp.arg("")]);

        // `.` is byte 0x2e, so `!bang` sorts ahead of it, as on the host.
        assert_eq!(
            output,
            "!bang
.
..
.hidden
visible
"
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn all_lists_the_dot_entries_of_an_empty_directory() {
        let temp = TempDir::new("ls_all_empty");

        let (output, errors) = listing(vec!["-a".to_string(), temp.arg("")]);

        assert_eq!(
            output,
            ".
..
"
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn classify_marks_each_kind_of_entry() {
        let temp = TempDir::new("ls_classify");
        let directory = every_kind(&temp);

        let (output, errors) = listing(vec!["-F".to_string(), directory]);

        assert_eq!(
            output,
            "dangling@
dirlink@
plain
script*
sub/
"
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn classify_marks_a_socket_and_a_fifo() {
        let temp = TempDir::new("ls_classify_special");
        let directory = temp.arg("kinds");
        fs::create_dir(&directory).unwrap();

        // A socket comes from the standard library. A FIFO has no standard
        // library call behind it, so that half of the test runs only where the
        // host has `mkfifo`.
        let _socket = std::os::unix::net::UnixListener::bind(temp.arg("kinds/sock")).unwrap();
        let piped = std::process::Command::new("mkfifo")
            .arg(temp.arg("kinds/pipe"))
            .status()
            .is_ok_and(|status| status.success());

        let (output, errors) = listing(vec!["-F".to_string(), directory]);

        let expected = if piped { "pipe|\nsock=\n" } else { "sock=\n" };
        assert_eq!(output, expected);
        assert!(errors.is_empty());
    }

    #[test]
    fn classify_and_all_combine() {
        let temp = TempDir::new("ls_classify_all");
        let directory = every_kind(&temp);

        let (clustered, errors) = listing(vec!["-aF".to_string(), directory.clone()]);
        let (separate, _) = listing(
            args(&["-a", "-F"])
                .into_iter()
                .chain([directory])
                .collect::<Vec<_>>(),
        );

        assert_eq!(
            clustered,
            "./
../
.hidden
dangling@
dirlink@
plain
script*
sub/
"
        );
        assert_eq!(
            separate, clustered,
            "clustered flags must match separate ones"
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn classify_names_a_symlink_operand_instead_of_following_it() {
        let temp = TempDir::new("ls_classify_operand");
        let directory = temp.arg("real");
        fs::create_dir(&directory).unwrap();
        touch(&temp, "real/inside");
        let link = temp.arg("link");
        std::os::unix::fs::symlink(&directory, &link).unwrap();

        let (followed, _) = listing(vec![link.clone()]);
        let (classified, errors) = listing(vec!["-F".to_string(), link.clone()]);

        assert_eq!(
            followed,
            "inside
"
        );
        assert_eq!(
            classified,
            format!(
                "{link}@
"
            )
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn classify_marks_file_operands_and_leaves_headers_alone() {
        let temp = TempDir::new("ls_classify_operands");
        let script = touch(&temp, "script");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let plain = touch(&temp, "plain");
        let directory = temp.arg("sub");
        fs::create_dir(&directory).unwrap();

        let operands = vec![
            "-F".to_string(),
            script.clone(),
            plain.clone(),
            directory.clone(),
        ];
        let (output, errors) = listing(operands);

        assert_eq!(
            output,
            format!(
                "{plain}
{script}*

{directory}:
"
            )
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn long_format_rows_carry_every_column() {
        let temp = TempDir::new("ls_long_row");
        let path = temp.arg("f");
        fs::write(&path, b"hello\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        let (output, errors) = listing(vec!["-l".to_string(), path.clone()]);

        let row = output.trim_end();
        let fields: Vec<&str> = row.split_whitespace().collect();
        assert_eq!(fields[0], "-rw-r--r--", "mode");
        assert_eq!(fields[1], "1", "hard links");
        assert!(!fields[2].is_empty(), "owner");
        assert!(!fields[3].is_empty(), "group");
        assert_eq!(fields[4], "6", "size in bytes");
        assert!(
            [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"
            ]
            .contains(&fields[5]),
            "month: {row}"
        );
        assert!(
            (1..=31).contains(&fields[6].parse::<u32>().unwrap()),
            "day of month: {row}"
        );
        // Written a moment ago, so the recent form: hours and minutes.
        let (hours, minutes) = fields[7].split_once(':').expect("a time of day");
        assert!(hours.len() == 2 && minutes.len() == 2, "time: {row}");
        assert_eq!(fields[8], path, "the name closes the row");
        assert!(errors.is_empty());
    }

    #[test]
    fn long_format_totals_a_directory_but_not_a_file() {
        let temp = TempDir::new("ls_long_total");
        let file = temp.arg("f");
        fs::write(&file, b"hello\n").unwrap();

        let (directory, _) = listing(vec!["-l".to_string(), temp.arg("")]);
        let (single, errors) = listing(vec!["-l".to_string(), file]);

        assert!(
            directory.starts_with("total "),
            "a directory block is headed by its block count: {directory}"
        );
        assert!(
            !single.starts_with("total "),
            "a file operand has no total: {single}"
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn long_format_aligns_the_columns_to_the_widest_value() {
        let temp = TempDir::new("ls_long_align");
        fs::write(temp.arg("large"), vec![b'x'; 12_000]).unwrap();
        fs::write(temp.arg("small"), b"xx").unwrap();

        let (output, errors) = listing(vec!["-l".to_string(), temp.arg("")]);

        let rows: Vec<&str> = output.lines().skip(1).collect();
        // The size column is as wide as `12000`, so the two byte counts are
        // right-aligned against each other and every later column lines up.
        assert!(rows[0].contains(" 12000 "), "{}", rows[0]);
        assert!(rows[1].contains("     2 "), "{}", rows[1]);
        assert_eq!(
            rows[0].find(" 12000 ").unwrap() + " 12000".len(),
            rows[1].find("     2 ").unwrap() + "     2".len(),
            "both size fields end in the same column"
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn long_format_shows_a_symlink_and_its_target() {
        let temp = TempDir::new("ls_long_symlink");
        fs::write(temp.arg("f"), b"x").unwrap();
        let link = temp.arg("link");
        std::os::unix::fs::symlink("f", &link).unwrap();

        let (plain, _) = listing(vec!["-l".to_string(), link.clone()]);
        let (classified, errors) = listing(vec!["-lF".to_string(), link.clone()]);

        assert!(
            plain.starts_with('l'),
            "a symlink row starts with l: {plain}"
        );
        assert!(plain.trim_end().ends_with(&format!("{link} -> f")));
        // `-F` marks the link, and leaves the target alone.
        assert!(classified.trim_end().ends_with(&format!("{link}@ -> f")));
        assert!(errors.is_empty());
    }

    #[test]
    fn rejects_options_it_does_not_have() {
        let (output, errors) = listing(args(&["-q"]));
        let (_, long) = listing(args(&["--bogus"]));

        assert!(output.is_empty());
        assert_eq!(errors, "ls: invalid option -- 'q'\n");
        assert_eq!(long, "ls: unrecognized option '--bogus'\n");
    }

    #[test]
    fn colored_listings_wrap_kinds_in_the_style_of_the_host() {
        let temp = TempDir::new("ls_color");
        let directory = temp.arg("sub");
        fs::create_dir(&directory).unwrap();
        let script = touch(&temp, "tool");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        touch(&temp, "plain");

        let (output, _) = listing_colored(vec!["-F".to_string(), temp.arg("")]);

        assert_eq!(
            output,
            "plain\n\x1b[01;34msub/\x1b[0m\n\x1b[01;32mtool*\x1b[0m\n"
        );
    }

    #[test]
    fn colored_listings_leave_the_color_off_when_asked() {
        let temp = TempDir::new("ls_color_off");
        let directory = temp.arg("sub");
        fs::create_dir(&directory).unwrap();
        touch(&temp, "plain");

        let (plain, _) = listing(vec![temp.arg("")]);
        let (colored, _) = listing_colored(vec![temp.arg("")]);

        assert_eq!(plain, "plain\nsub\n");
        assert_eq!(colored, "plain\n\x1b[01;34msub\x1b[0m\n");
    }

    #[test]
    fn colored_long_rows_wrap_only_the_name_column() {
        let temp = TempDir::new("ls_color_long");
        let directory = temp.arg("sub");
        fs::create_dir(&directory).unwrap();

        let (output, _) = listing_colored(vec!["-l".to_string(), temp.arg("")]);

        let rows: Vec<&str> = output.split('\n').skip(1).collect();
        assert!(
            rows[0].starts_with("drwxr-xr-x 2"),
            "the columns before the name are untouched: {}",
            rows[0]
        );
        assert!(
            rows[0].ends_with("\x1b[01;34msub\x1b[0m"),
            "only the name column is wrapped: {}",
            rows[0]
        );
    }
}
