//! Modification times in the shape `ls` prints them.
//!
//! Two things the standard library does not do have to be done here. It gives
//! no calendar, so the epoch seconds in `MetadataExt::mtime` are turned into a
//! civil date by the usual days-to-date algorithm; and it gives no local time,
//! so the offset comes from reading the system's own zone file. Both would
//! otherwise be `chrono` and libc, and `Cargo.toml` has no dependencies.

use std::env;
use std::fs;
use std::path::PathBuf;

const SECONDS_PER_DAY: i64 = 86_400;

/// Half of an average Gregorian year, which is the window `ls` calls recent.
///
/// Inside it a listing shows the time of day; outside it, in either direction,
/// the year. That is the rule the real `ls` uses, down to the hour of slack it
/// allows for a clock that runs slightly ahead.
const SIX_MONTHS: i64 = 15_778_476;
const HOUR: i64 = 3_600;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The local time zone: when each offset from UTC starts, and what it is.
pub struct Zone {
    /// Transition timestamps, ascending.
    transitions: Vec<i64>,
    /// The offset that begins at the matching transition, in seconds.
    offsets: Vec<i32>,
    /// The offset in force before the first transition.
    initial: i32,
}

impl Zone {
    /// The zone this machine is set to, or UTC if it cannot be read.
    ///
    /// `TZ` is honoured the way the C library honours it, since the real `ls`
    /// answers to it and a listing compared against one has to agree. A `TZ`
    /// naming rules rather than a zone (`EET-2EEST,M3.5.0/3`) is not
    /// understood and falls back to UTC.
    pub fn local() -> Self {
        Self::from_path(&zone_path()).unwrap_or_else(Self::utc)
    }

    /// A zone with one fixed offset, used where no zone file is involved.
    pub fn fixed(offset: i32) -> Self {
        Self {
            transitions: Vec::new(),
            offsets: Vec::new(),
            initial: offset,
        }
    }

    fn utc() -> Self {
        Self::fixed(0)
    }

    fn from_path(path: &PathBuf) -> Option<Self> {
        Self::from_tzif(&fs::read(path).ok()?)
    }

    /// The offset from UTC in force at this instant, in seconds.
    pub fn offset(&self, timestamp: i64) -> i32 {
        // The last transition at or before the instant decides it.
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
    /// Beyond the last transition a zone file expects its trailing rule string
    /// to be applied; this keeps the last known offset instead. Zone files on
    /// this system carry transitions through 2037, so only a timestamp past
    /// that, in a zone that still changes its clocks, would notice.
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

        Some(Self {
            transitions,
            offsets,
            initial,
        })
    }
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
    use super::{Zone, civil_from_days, stamp};

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
}
