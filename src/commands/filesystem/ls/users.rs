//! Owner and group names, read from `/etc/passwd` and `/etc/group`.
//!
//! `ls -l` prints names where the filesystem stores numbers. Turning one into
//! the other is normally `getpwuid`, which lives in libc, and `Cargo.toml` has
//! no dependencies, so the two files behind that call are parsed here instead.
//!
//! This sees only what the files hold. A machine whose accounts come from LDAP
//! or another NSS source resolves fewer names than the real `ls` would, and
//! every id that resolves to nothing is printed as the number itself, which is
//! also what `ls` does for an id no longer in the files.

use std::collections::HashMap;
use std::fs;

/// The id-to-name tables, read once per listing.
pub struct Names {
    users: HashMap<u32, String>,
    groups: HashMap<u32, String>,
}

impl Names {
    /// Reads both files. A file that cannot be read leaves its table empty,
    /// which costs numeric output rather than a failed listing.
    pub fn load() -> Self {
        Self {
            users: read_table("/etc/passwd"),
            groups: read_table("/etc/group"),
        }
    }

    /// The owner's name, or the uid when nothing claims it.
    pub fn user(&self, uid: u32) -> String {
        self.users
            .get(&uid)
            .cloned()
            .unwrap_or_else(|| uid.to_string())
    }

    /// The group's name, or the gid when nothing claims it.
    pub fn group(&self, gid: u32) -> String {
        self.groups
            .get(&gid)
            .cloned()
            .unwrap_or_else(|| gid.to_string())
    }
}

/// Parses `name:password:id:...` lines, which `/etc/passwd` and `/etc/group`
/// share for the first three fields.
fn read_table(path: &str) -> HashMap<u32, String> {
    let mut table = HashMap::new();

    let Ok(contents) = fs::read_to_string(path) else {
        return table;
    };

    for line in contents.lines() {
        let mut fields = line.split(':');
        let (Some(name), Some(_password), Some(id)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };

        if let Ok(id) = id.parse::<u32>() {
            // First entry wins, as in the files' own lookup order, so an id
            // listed twice resolves to the name nearest the top.
            table.entry(id).or_insert_with(|| name.to_string());
        }
    }

    table
}

#[cfg(test)]
mod tests {
    use super::{Names, read_table};

    #[test]
    fn resolves_the_current_user_the_way_the_system_files_do() {
        let names = Names::load();
        let expected = read_table("/etc/passwd");

        // Whatever `/etc/passwd` holds for uid 0, the table must agree with it,
        // and a machine without the file must not stop the listing.
        match expected.get(&0) {
            Some(name) => assert_eq!(names.user(0), *name),
            None => assert_eq!(names.user(0), "0"),
        }
    }

    #[test]
    fn falls_back_to_the_number_for_an_unknown_id() {
        let names = Names::load();

        assert_eq!(names.user(4_294_967_294), "4294967294");
        assert_eq!(names.group(4_294_967_294), "4294967294");
    }

    #[test]
    fn skips_lines_that_are_not_entries() {
        let table = read_table("/proc/self/cmdline");

        // Nothing in there parses as `name:x:id`, and nothing panics either.
        assert!(table.is_empty());
    }
}
