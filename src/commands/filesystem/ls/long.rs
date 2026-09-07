//! The `-l` long format: one row per entry, in aligned columns.

use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::time::{Zone, stamp};
use super::users::Names;
use crate::commands::report;

/// One thing to list: the name to print, and where to find it.
pub struct Item {
    pub display: Vec<u8>,
    pub path: PathBuf,
}

/// The size column holds a byte count, or a device's two numbers.
enum Size {
    Bytes(u64),
    Device { major: u64, minor: u64 },
}

/// One rendered row, before the columns are sized.
struct Row {
    mode: String,
    links: String,
    user: String,
    group: String,
    size: Size,
    time: String,
    name: Vec<u8>,
    target: Option<Vec<u8>>,
}

/// Writes the long listing, with the `total` line when this is a directory.
///
/// An entry that cannot be stat'ed is reported and left out; the rest of the
/// listing still lines up, because the columns are measured after the rows are
/// built rather than guessed in advance.
pub fn write<W, E>(
    items: &[Item],
    directory: bool,
    classify: bool,
    output: &mut W,
    errors: &mut E,
) -> io::Result<()>
where
    W: Write,
    E: Write,
{
    let names = Names::load();
    let zone = Zone::local();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0);

    let mut rows = Vec::with_capacity(items.len());
    let mut blocks = 0;

    for item in items {
        // Always the link itself, never its target: `-l` shows what the entry
        // is, and a symlink's own size, time, and mode are what belong here.
        let metadata = match item.path.symlink_metadata() {
            Ok(metadata) => metadata,
            Err(error) => {
                let subject = format!("cannot access {}", report::quoted(&item.path));
                report::io_error("ls", &subject, &error, errors)?;
                continue;
            }
        };

        blocks += metadata.blocks();

        let mut name = item.display.clone();
        if classify {
            name.extend_from_slice(super::indicator(&metadata, true).as_bytes());
        }

        rows.push(Row {
            mode: mode_string(&metadata),
            links: metadata.nlink().to_string(),
            user: names.user(metadata.uid()),
            group: names.group(metadata.gid()),
            size: size_of(&metadata),
            time: stamp(metadata.mtime(), now, &zone),
            name,
            target: read_target(&metadata, &item.path),
        });
    }

    if directory {
        // `blocks` counts 512-byte units; `ls` reports 1024-byte ones.
        writeln!(output, "total {}", blocks / 2)?;
    }

    let widths = Widths::of(&rows);
    for row in &rows {
        write_row(row, &widths, output)?;
    }

    Ok(())
}

/// The width of every column that varies.
struct Widths {
    links: usize,
    user: usize,
    group: usize,
    size: usize,
    major: usize,
    minor: usize,
}

impl Widths {
    fn of(rows: &[Row]) -> Self {
        let mut widths = Self {
            links: 0,
            user: 0,
            group: 0,
            size: 0,
            major: 0,
            minor: 0,
        };

        for row in rows {
            widths.links = widths.links.max(row.links.len());
            widths.user = widths.user.max(row.user.len());
            widths.group = widths.group.max(row.group.len());

            match &row.size {
                Size::Bytes(size) => widths.size = widths.size.max(digits(*size)),
                Size::Device { major, minor } => {
                    widths.major = widths.major.max(digits(*major));
                    widths.minor = widths.minor.max(digits(*minor));
                    // The size column has to hold `major, minor` too.
                    widths.size = widths.size.max(digits(*major) + 2 + digits(*minor));
                }
            }
        }

        widths
    }

    /// The width the two device numbers and their separator occupy.
    ///
    /// The widest major and the widest minor need not belong to the same
    /// entry, so this can exceed the size column, and then the device rows
    /// reach one or two columns further right than the byte counts. That is
    /// what the host tool does.
    fn device(&self) -> usize {
        self.size.max(self.major + 2 + self.minor)
    }
}

fn write_row<W>(row: &Row, widths: &Widths, output: &mut W) -> io::Result<()>
where
    W: Write,
{
    write!(
        output,
        "{} {:>links$} {:<user$} {:<group$} ",
        row.mode,
        row.links,
        row.user,
        row.group,
        links = widths.links,
        user = widths.user,
        group = widths.group,
    )?;

    match &row.size {
        Size::Bytes(size) => write!(output, "{size:>width$}", width = widths.size)?,
        Size::Device { major, minor } => write!(
            output,
            "{major:>major_width$}, {minor:>minor_width$}",
            major_width = widths.device() - 2 - widths.minor,
            minor_width = widths.minor,
        )?,
    }

    write!(output, " {} ", row.time)?;
    output.write_all(&row.name)?;

    if let Some(target) = &row.target {
        output.write_all(b" -> ")?;
        output.write_all(target)?;
    }

    output.write_all(b"\n")
}

/// `drwxr-xr-x`: the type, then three groups of three permission bits.
fn mode_string(metadata: &std::fs::Metadata) -> String {
    let file_type = metadata.file_type();
    let kind = if file_type.is_dir() {
        'd'
    } else if file_type.is_symlink() {
        'l'
    } else if file_type.is_char_device() {
        'c'
    } else if file_type.is_block_device() {
        'b'
    } else if file_type.is_fifo() {
        'p'
    } else if file_type.is_socket() {
        's'
    } else {
        '-'
    };

    let mode = metadata.permissions().mode();
    let mut text = String::with_capacity(10);
    text.push(kind);

    // Each trio ends in a letter that carries two meanings at once: the execute
    // bit, and the set-user, set-group, or sticky bit above it. A capital says
    // the special bit is set while the execute bit under it is not.
    for (shift, special, set, unset) in [
        (6, 0o4000, 's', 'S'),
        (3, 0o2000, 's', 'S'),
        (0, 0o1000, 't', 'T'),
    ] {
        text.push(if mode & (0o400 >> (6 - shift)) != 0 {
            'r'
        } else {
            '-'
        });
        text.push(if mode & (0o200 >> (6 - shift)) != 0 {
            'w'
        } else {
            '-'
        });

        let executable = mode & (0o100 >> (6 - shift)) != 0;
        text.push(match (mode & special != 0, executable) {
            (true, true) => set,
            (true, false) => unset,
            (false, true) => 'x',
            (false, false) => '-',
        });
    }

    text
}

/// A byte count, or the two numbers behind a device file.
fn size_of(metadata: &std::fs::Metadata) -> Size {
    let file_type = metadata.file_type();

    if file_type.is_char_device() || file_type.is_block_device() {
        let device = metadata.rdev();
        // The kernel packs both numbers into one word, in the split glibc's
        // `major` and `minor` macros undo.
        Size::Device {
            major: (device >> 8) & 0xfff | ((device >> 32) & !0xfff),
            minor: (device & 0xff) | ((device >> 12) & !0xff),
        }
    } else {
        Size::Bytes(metadata.len())
    }
}

/// Where a symlink points, for the `name -> target` half of its row.
fn read_target(metadata: &std::fs::Metadata, path: &Path) -> Option<Vec<u8>> {
    if !metadata.file_type().is_symlink() {
        return None;
    }

    std::fs::read_link(path)
        .ok()
        .map(|target| target.as_os_str().as_bytes().to_vec())
}

fn digits(value: u64) -> usize {
    value.to_string().len()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::mode_string;
    use crate::testing::TempDir;

    fn mode_of(path: &str) -> String {
        mode_string(&fs::symlink_metadata(path).unwrap())
    }

    #[test]
    fn renders_types_and_permission_bits() {
        let temp = TempDir::new("ls_mode");
        let file = temp.arg("file");
        fs::write(&file, b"").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        let directory = temp.arg("directory");
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
        let link = temp.arg("link");
        std::os::unix::fs::symlink("file", &link).unwrap();

        assert_eq!(mode_of(&file), "-rw-r--r--");
        assert_eq!(mode_of(&directory), "drwxr-xr-x");
        assert_eq!(mode_of(&link), "lrwxrwxrwx");
    }

    #[test]
    fn renders_the_set_id_and_sticky_bits() {
        let temp = TempDir::new("ls_mode_special");
        let file = temp.arg("file");
        fs::write(&file, b"").unwrap();
        let directory = temp.arg("directory");
        fs::create_dir(&directory).unwrap();

        fs::set_permissions(&file, fs::Permissions::from_mode(0o4755)).unwrap();
        assert_eq!(
            mode_of(&file),
            "-rwsr-xr-x",
            "set-user-id over an execute bit"
        );

        fs::set_permissions(&file, fs::Permissions::from_mode(0o2640)).unwrap();
        assert_eq!(mode_of(&file), "-rw-r-S---", "set-group-id without one");

        fs::set_permissions(&directory, fs::Permissions::from_mode(0o1777)).unwrap();
        assert_eq!(
            mode_of(&directory),
            "drwxrwxrwt",
            "sticky over an execute bit"
        );

        fs::set_permissions(&directory, fs::Permissions::from_mode(0o1666)).unwrap();
        assert_eq!(mode_of(&directory), "drw-rw-rwT", "sticky without one");
    }

    #[test]
    fn renders_a_socket_and_a_fifo() {
        let temp = TempDir::new("ls_mode_special_files");
        let socket = temp.arg("sock");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();

        assert!(mode_of(&socket).starts_with('s'));

        let fifo = temp.arg("pipe");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .is_ok_and(|status| status.success());
        if made {
            assert!(mode_of(&fifo).starts_with('p'));
        }
    }
}
