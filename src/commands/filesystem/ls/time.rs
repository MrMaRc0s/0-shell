//! Modification times in the shape `ls` prints them.
//!
//! Two things the standard library does not do have to be done here. It gives
//! no calendar, so the epoch seconds in `MetadataExt::mtime` are turned into a
//! civil date by the usual days-to-date algorithm; and it gives no local time,
//! so the offset comes from reading the system's own zone file. Both would
//! otherwise be `chrono` and libc, and `Cargo.toml` has no dependencies.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;

mod posix;

const SECONDS_PER_DAY: i64 = 86_400;

/// Half of an average Gregorian year, which is the window `ls` calls recent.
///
/// Inside it a listing shows the time of day; outside it, in either direction,
/// the year. That is the rule the real `ls` uses, down to the hour of slack it
/// allows for a clock that runs slightly ahead.
const SIX_MONTHS: i64 = 15_778_476;
const HOUR: i64 = 3_600;

/// The largest real zone file on a typical system is a few hundred bytes;
/// 64 KiB is generous enough for any realistic file.
const MAX_ZONE_FILE_SIZE: u64 = 64 * 1024;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The local time zone: when each offset from UTC starts, and what it is.
#[derive(Debug, PartialEq)]
pub struct Zone {
    /// Transition timestamps, ascending.
    transitions: Vec<i64>,
    /// The offset that begins at the matching transition, in seconds.
    offsets: Vec<i32>,
    /// The offset in force before the first transition.
    initial: i32,
    /// How to compute the offset past the last transition — a zone file's
    /// own trailing rule, or, for a `TZ` that named rules directly instead of
    /// a zone, the only rule there is.
    beyond: Option<posix::Rule>,
}

impl Zone {
    /// The zone this machine is set to, or UTC if it cannot be read.
    ///
    /// `TZ` is honoured the way the C library honours it, since the real `ls`
    /// answers to it and a listing compared against one has to agree. A `TZ`
    /// naming rules rather than a zone, `EET-2EEST,M3.5.0/3`, is read as a
    /// self-contained rule once a file by that name fails to turn up —
    /// `EET-2EEST,M3.5.0/3` is not a path under `/usr/share/zoneinfo` any
    /// zone file lives at.
    pub fn local() -> Self {
        if let Some(zone) = Self::from_path(&zone_path()) {
            return zone;
        }
        if let Some(zone) = env::var_os("TZ").as_deref().and_then(Self::from_tz_var) {
            return zone;
        }
        Self::utc()
    }

    /// A zone with one fixed offset, used where no zone file is involved.
    pub fn fixed(offset: i32) -> Self {
        Self {
            transitions: Vec::new(),
            offsets: Vec::new(),
            initial: offset,
            beyond: None,
        }
    }

    /// Reads `TZ` itself as a POSIX rule, for the case a zone file lookup
    /// already failed. A leading colon means the rest names a file, which has
    /// already been tried and found missing, so that spelling is not read as
    /// a rule on top of it.
    fn from_tz_var(tz: &OsStr) -> Option<Self> {
        let tz = tz.to_str()?;
        if tz.starts_with(':') {
            return None;
        }

        let rule = posix::parse(tz)?;
        Some(Self {
            transitions: Vec::new(),
            offsets: Vec::new(),
            // Never actually read: with no transitions, `offset` always
            // finds itself past the last one — there is none — and defers
            // to `beyond` instead.
            initial: 0,
            beyond: Some(rule),
        })
    }

    fn utc() -> Self {
        Self::fixed(0)
    }

    fn from_path(path: &PathBuf) -> Option<Self> {
        let meta = fs::metadata(path).ok()?;
        if !meta.is_file() {
            return None;
        }
        if meta.len() > MAX_ZONE_FILE_SIZE {
            return None;
        }
        Self::from_tzif(&fs::read(path).ok()?)
    }

    /// The offset from UTC in force at this instant, in seconds.
    pub fn offset(&self, timestamp: i64) -> i32 {
        // Past the last transition the table has nothing more to say, and
        // `beyond` takes over — a zone file's own trailing rule, or, when
        // there were never any transitions to begin with, the only rule a
        // `TZ` that named one directly ever had.
        let past_last_transition = match self.transitions.last() {
            Some(last) => timestamp > *last,
            None => true,
        };
        if past_last_transition && let Some(rule) = &self.beyond {
            return rule.offset_at(timestamp);
        }

        // Otherwise the last transition at or before the instant decides it.
        let index = self
            .transitions
            .partition_point(|start| *start <= timestamp);

        if index == 0 {
            self.initial
        } else {
            self.offsets[index - 1]
        }
    }

    /// Reads a compiled zone file, in either the original or the 64-bit layout.
    ///
    /// Beyond the last transition a zone file carries its own trailing rule,
    /// a POSIX `TZ` string on a line by itself at the very end of the file
    /// from format version 2 onward, meant to be applied from there on. Zone
    /// files on this system carry transitions through 2037, so only a
    /// timestamp past that, in a zone that still changes its clocks, ever
    /// reaches it; a rule this module does not understand — a Julian-day one,
    /// mainly — falls back to the last known offset instead, unchanged from
    /// before this was read at all.
    fn from_tzif(data: &[u8]) -> Option<Self> {
        if data.get(..4)? != b"TZif" {
            return None;
        }
        let version = *data.get(4)?;

        let first = Counts::read(data, 0)?;
        // Version 2 and later repeat everything in 64-bit form after the
        // 32-bit block, and the second copy is the one to read.
        let (counts, start, width) = if version >= b'2' {
            let second = 44 + first.block_size(4);
            (Counts::read(data, second)?, second + 44, 8)
        } else {
            (first, 44, 4)
        };

        // A crafted header can claim counts that do not fit the data.  Reject
        // the file before any allocation sized by those counts.
        if !counts_fit(&counts, data.len(), start, width) {
            return None;
        }

        let mut transitions = Vec::with_capacity(counts.timecnt);
        for index in 0..counts.timecnt {
            let at = start + index * width;
            transitions.push(match width {
                8 => i64::from_be_bytes(data.get(at..at + 8)?.try_into().ok()?),
                _ => i64::from(i32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?)),
            });
        }

        let indices_at = start + counts.timecnt * width;
        let types_at = indices_at + counts.timecnt;

        // Each type is a 4-byte offset, a daylight-saving flag, and an index
        // into the abbreviation strings, which a listing never prints.
        let type_offset = |index: usize| -> Option<i32> {
            let at = types_at + index * 6;
            Some(i32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
        };
        let type_is_dst =
            |index: usize| -> Option<bool> { Some(*data.get(types_at + index * 6 + 4)? != 0) };

        let mut offsets = Vec::with_capacity(counts.timecnt);
        for index in 0..counts.timecnt {
            let kind = usize::from(*data.get(indices_at + index)?);
            offsets.push(type_offset(kind)?);
        }

        // Before the first transition, the first type that is not daylight
        // saving applies, which is what the format prescribes.
        let initial = (0..counts.typecnt)
            .find(|kind| type_is_dst(*kind) == Some(false))
            .or(if counts.typecnt > 0 { Some(0) } else { None })
            .and_then(type_offset)
            .unwrap_or(0);

        // Only a version-2-or-later file carries the trailing rule at all;
        // the original format ends right where the data block does.
        let beyond = if version >= b'2' {
            footer(data, start + counts.block_size(width))
        } else {
            None
        };

        Some(Self {
            transitions,
            offsets,
            initial,
            beyond,
        })
    }
}

/// The rule on the line a version-2+ zone file appends after its data block:
/// `\n<rule>\n`, on its own, meant to be applied from the last recorded
/// transition onward. `None` for a file with nothing there, or a rule this
/// module does not understand.
fn footer(data: &[u8], start: usize) -> Option<posix::Rule> {
    let text = std::str::from_utf8(data.get(start..)?).ok()?;
    let text = text.strip_prefix('\n')?;
    let end = text.find('\n')?;
    posix::parse(&text[..end])
}

/// The six counts in a zone file header, which size everything after it.
struct Counts {
    isutcnt: usize,
    isstdcnt: usize,
    leapcnt: usize,
    timecnt: usize,
    typecnt: usize,
    charcnt: usize,
}

impl Counts {
    fn read(data: &[u8], offset: usize) -> Option<Self> {
        if data.get(offset..offset + 4)? != b"TZif" {
            return None;
        }

        let number = |index: usize| -> Option<usize> {
            let at = offset + 20 + index * 4;
            let bytes: [u8; 4] = data.get(at..at + 4)?.try_into().ok()?;
            Some(u32::from_be_bytes(bytes) as usize)
        };

        Some(Self {
            isutcnt: number(0)?,
            isstdcnt: number(1)?,
            leapcnt: number(2)?,
            timecnt: number(3)?,
            typecnt: number(4)?,
            charcnt: number(5)?,
        })
    }

    /// The size of the data block that follows a header, for this time width.
    fn block_size(&self, time_width: usize) -> usize {
        self.timecnt * (time_width + 1)
            + self.typecnt * 6
            + self.charcnt
            + self.leapcnt * (time_width + 4)
            + self.isstdcnt
            + self.isutcnt
    }
}

/// Checks that the header counts can actually fit inside the data slice.
/// Returns false on overflow or when the tables would extend past the end.
fn counts_fit(counts: &Counts, data_len: usize, start: usize, width: usize) -> bool {
    let Some(trans) = start.checked_add(counts.timecnt.saturating_mul(width)) else {
        return false;
    };
    let Some(indices) = trans.checked_add(counts.timecnt) else {
        return false;
    };
    let Some(types_end) = indices.checked_add(counts.typecnt.saturating_mul(6)) else {
        return false;
    };
    types_end <= data_len
}

/// Which zone file to read: what `TZ` names, or the system's own.
fn zone_path() -> PathBuf {
    let Some(tz) = env::var_os("TZ") else {
        return PathBuf::from("/etc/localtime");
    };

    let tz = tz.to_string_lossy().into_owned();
    // A leading colon is allowed and means the rest is a file name.
    let name = tz.strip_prefix(':').unwrap_or(&tz);

    if name.is_empty() {
        PathBuf::from("/etc/localtime")
    } else if name.starts_with('/') {
        PathBuf::from(name)
    } else {
        PathBuf::from("/usr/share/zoneinfo").join(name)
    }
}

/// One modification time, as `ls` writes it.
///
/// A time inside the recent window is shown to the minute, `Sep  5 12:28`.
/// Anything older, or more than an hour ahead of the clock, shows the year
/// instead: `Mar  2  2019`. Both spellings are twelve characters wide, which
/// is why the column needs no padding of its own.
pub fn stamp(seconds: i64, now: i64, zone: &Zone) -> String {
    let local = seconds + i64::from(zone.offset(seconds));
    let (_, month, day) = civil_from_days(local.div_euclid(SECONDS_PER_DAY));
    let name = MONTHS[(month - 1) as usize];

    if seconds > now - SIX_MONTHS && seconds <= now + HOUR {
        let time = local.rem_euclid(SECONDS_PER_DAY);
        format!(
            "{name} {day:>2} {:02}:{:02}",
            time / 3_600,
            (time % 3_600) / 60
        )
    } else {
        let (year, _, _) = civil_from_days(local.div_euclid(SECONDS_PER_DAY));
        format!("{name} {day:>2}  {year}")
    }
}

/// The civil date of a day counted from 1970-01-01.
///
/// Howard Hinnant's algorithm: shift the epoch to 0000-03-01 so that leap days
/// land at the end of a four-century era, then unwind era, year, and day of
/// year in turn. Correct for any date the type can hold, with no table and no
/// special case for February.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;

    (year + i64::from(month <= 2), month, day)
}

#[cfg(test)]
mod tests {
    use super::{MAX_ZONE_FILE_SIZE, Zone, civil_from_days, stamp};
    use crate::testing::TempDir;
    use std::ffi::OsStr;
    use std::fs;

    #[test]
    fn converts_days_to_civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29), "a leap day");
        assert_eq!(civil_from_days(20_453), (2025, 12, 31));
        assert_eq!(civil_from_days(20_454), (2026, 1, 1));
        assert_eq!(civil_from_days(24_836), (2037, 12, 31));
    }

    #[test]
    fn shows_the_time_for_a_recent_stamp_and_the_year_for_an_old_one() {
        let utc = Zone::fixed(0);
        // 2026-09-05 09:29:00 UTC.
        let now = 1_788_600_540;

        assert_eq!(stamp(now - 60, now, &utc), "Sep  5 09:28");
        assert_eq!(stamp(1_551_499_506, now, &utc), "Mar  2  2019");
    }

    #[test]
    fn draws_the_recent_window_where_the_real_ls_draws_it() {
        let utc = Zone::fixed(0);
        let now = 1_788_600_540;

        // Just inside six months, and just outside it.
        assert_eq!(stamp(now - 15_778_000, now, &utc), "Mar  6 18:42");
        assert_eq!(stamp(now - 15_779_000, now, &utc), "Mar  6  2026");
        // An hour of slack for a clock that runs ahead, and no more.
        assert_eq!(stamp(now + 3_500, now, &utc), "Sep  5 10:27");
        assert_eq!(stamp(now + 3_700, now, &utc), "Sep  5  2026");
    }

    #[test]
    fn applies_the_offset_of_the_zone() {
        let now = 1_788_600_540;
        let ahead = Zone::fixed(3 * 3_600);
        let behind = Zone::fixed(-5 * 3_600);

        assert_eq!(stamp(now, now, &ahead), "Sep  5 12:29");
        assert_eq!(stamp(now, now, &behind), "Sep  5 04:29");
    }

    #[test]
    fn reads_the_system_zone_file_and_its_daylight_saving_changes() {
        let zone = Zone::local();
        let summer = 1_788_600_540; // 2026-09-05, northern summer
        let winter = 1_772_000_000; // 2026-02-25

        // Whatever this machine is set to, an offset is a whole number of
        // minutes and lies inside the range the world uses.
        for stamp in [summer, winter] {
            let offset = zone.offset(stamp);
            assert_eq!(offset % 60, 0, "offsets are whole minutes");
            assert!((-12 * 3_600..=14 * 3_600).contains(&offset));
        }
    }

    #[test]
    fn a_header_claiming_huge_counts_is_rejected_before_allocating() {
        // 44-byte header: magic + version + 4 reserved bytes + 6 counts.
        let mut header = vec![0u8; 44];
        header[0..4].copy_from_slice(b"TZif");
        header[4] = b'1';
        // isutcnt, isstdcnt, leapcnt, timecnt, typecnt, charcnt
        let max = u32::MAX.to_be_bytes();
        header[20..24].copy_from_slice(&max);
        header[24..28].copy_from_slice(&max);
        header[28..32].copy_from_slice(&max);
        header[32..36].copy_from_slice(&max);
        header[36..40].copy_from_slice(&max);
        header[40..44].copy_from_slice(&max);

        assert_eq!(Zone::from_tzif(&header), None);
    }

    #[test]
    fn rejects_a_zone_source_that_is_not_a_regular_file() {
        let tmp = TempDir::new("zone_test");
        let dir = tmp.path().join("not_a_file");
        fs::create_dir(&dir).unwrap();
        assert_eq!(Zone::from_path(&dir), None);
    }

    #[test]
    fn rejects_a_zone_file_above_the_size_cap() {
        let tmp = TempDir::new("zone_test");
        let path = tmp.path().join("big.zone");
        let size = (MAX_ZONE_FILE_SIZE + 1) as usize;
        let mut data = vec![0u8; size];
        data[0..4].copy_from_slice(b"TZif");
        fs::write(&path, &data).unwrap();
        assert_eq!(Zone::from_path(&path), None);
    }

    #[test]
    fn reads_a_valid_empty_zone_file_below_the_cap() {
        let tmp = TempDir::new("zone_test");
        let path = tmp.path().join("empty.zone");
        let mut data = vec![0u8; 44];
        data[0..4].copy_from_slice(b"TZif");
        fs::write(&path, &data).unwrap();
        assert!(Zone::from_path(&path).is_some());
    }

    #[test]
    fn a_tz_naming_rules_directly_is_read_as_a_rule_rather_than_falling_back_to_utc() {
        // The example the README used to name as unsupported: Athens' own
        // rule, naming its standard and daylight-saving offsets and the
        // dates that switch between them, with no zone file involved at all.
        let zone = Zone::from_tz_var(OsStr::new("EET-2EEST,M3.5.0/3,M10.5.0/4")).unwrap();

        assert_eq!(
            zone.offset(1_767_225_600),
            2 * 3_600,
            "2026-01-01: standard"
        );
        assert_eq!(
            zone.offset(1_782_950_400),
            3 * 3_600,
            "2026-07-01: daylight saving"
        );
    }

    #[test]
    fn a_tz_naming_a_file_explicitly_is_not_read_as_a_rule() {
        // A leading colon means "the rest is a file name", already tried and
        // already missing by the time this is reached; reading the same text
        // as a rule on top of that would make an explicit file reference
        // silently resolve to something else instead of just failing.
        assert!(Zone::from_tz_var(OsStr::new(":EET-2EEST,M3.5.0/3,M10.5.0/4")).is_none());
    }

    #[test]
    fn a_zone_file_with_a_trailing_rule_extrapolates_past_its_last_transition() {
        // Built by hand rather than read from a real file, so the test does
        // not depend on transition tables that will themselves eventually
        // run out: one transition, then a footer good for anything after it.
        let mut header = vec![0u8; 44];
        header[0..4].copy_from_slice(b"TZif");
        header[4] = b'2';
        // A version-1 block with one transition and one type, skipped over
        // (this module reads the version-2 copy that follows it).
        header[20..24].copy_from_slice(&0u32.to_be_bytes()); // isutcnt
        header[24..28].copy_from_slice(&0u32.to_be_bytes()); // isstdcnt
        header[28..32].copy_from_slice(&0u32.to_be_bytes()); // leapcnt
        header[32..36].copy_from_slice(&1u32.to_be_bytes()); // timecnt
        header[36..40].copy_from_slice(&1u32.to_be_bytes()); // typecnt
        header[40..44].copy_from_slice(&0u32.to_be_bytes()); // charcnt
        let v1_transition = 0i32.to_be_bytes(); // 1970-01-01
        let v1_type_index = [0u8];
        let v1_type = {
            let mut t = vec![0u8; 6];
            t[0..4].copy_from_slice(&(-5 * 3_600i32).to_be_bytes());
            t
        };
        header.extend_from_slice(&v1_transition);
        header.extend_from_slice(&v1_type_index);
        header.extend_from_slice(&v1_type);

        // The version-2 header and block: the same single transition, this
        // time as an 8-byte timestamp, and no offset after it but the
        // footer's own rule.
        let mut v2 = vec![0u8; 44];
        v2[0..4].copy_from_slice(b"TZif");
        v2[4] = b'2';
        v2[32..36].copy_from_slice(&1u32.to_be_bytes()); // timecnt
        v2[36..40].copy_from_slice(&1u32.to_be_bytes()); // typecnt
        let v2_transition = 0i64.to_be_bytes();
        let v2_type_index = [0u8];
        let mut v2_type = vec![0u8; 6];
        v2_type[0..4].copy_from_slice(&(-5 * 3_600i32).to_be_bytes());
        v2.extend_from_slice(&v2_transition);
        v2.extend_from_slice(&v2_type_index);
        v2.extend_from_slice(&v2_type);
        v2.extend_from_slice(b"\nEST5EDT,M3.2.0,M11.1.0/2\n");

        header.extend_from_slice(&v2);

        let zone = Zone::from_tzif(&header).unwrap();

        // At the one recorded transition, the table's own offset.
        assert_eq!(zone.offset(0), -5 * 3_600);
        // Decades past it, in July: the footer's rule, daylight saving.
        assert_eq!(zone.offset(1_782_950_400), -4 * 3_600, "2026-07-01");
        // And in January: the footer's rule, standard time.
        assert_eq!(zone.offset(1_767_225_600), -5 * 3_600, "2026-01-01");
    }

    #[test]
    fn the_actual_system_zoneinfo_footers_this_module_targets_still_parse() {
        // The exact strings a handful of real zone files on a typical Linux
        // system carry, read straight from disk rather than retyped, so a
        // change to either this parser or those files' own format is caught.
        for zone in [
            "America/New_York",
            "Europe/Athens",
            "Pacific/Auckland",
            "UTC",
        ] {
            let path = format!("/usr/share/zoneinfo/{zone}");
            let Ok(data) = fs::read(&path) else {
                continue;
            };

            assert!(
                Zone::from_tzif(&data).is_some(),
                "{zone}'s own zone file failed to parse"
            );
        }
    }
}
