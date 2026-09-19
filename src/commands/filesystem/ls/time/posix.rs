//! POSIX `TZ` rules: `STD offset DST [offset] [,start[/time],end[/time]]`.
//!
//! Two places hand this module a string to parse. A `TZ` that names rules
//! directly, `EET-2EEST,M3.5.0/3`, rather than a zone like `Europe/Athens`,
//! never touches a zone file at all. And every compiled zone file from
//! format version 2 onward carries one of these as a trailing line, meant to
//! be applied to a timestamp past the last transition the file bothered to
//! record — which for the files on this system is some time in 2037.
//!
//! Only the common shape is understood: a month-week-day date
//! (`Mmonth.week.day`, the form every zone file on this system actually
//! uses). The Julian-day forms (`Jn` and `n`) POSIX also allows are rare
//! enough in practice that a rule using one is treated as unparseable rather
//! than taken on faith, which falls back to whatever the caller already does
//! without one — UTC for a `TZ` that named rules directly, the last known
//! offset for a timestamp past a zone file's final transition.

/// A standard offset, and optionally when and how daylight saving changes it.
#[derive(Debug, PartialEq)]
pub struct Rule {
    /// Seconds to add to UTC for standard time. Already flipped from POSIX's
    /// own convention, where the offset is *subtracted* from UTC — positive
    /// for a zone west of Greenwich — to the convention `Zone::offset`
    /// returns everywhere else: added to UTC to get local time.
    std_offset: i32,
    dst: Option<Dst>,
}

#[derive(Debug, PartialEq)]
struct Dst {
    /// Seconds to add to UTC while daylight saving is in effect, in the same
    /// convention as `std_offset`.
    offset: i32,
    start: Date,
    /// Seconds after local midnight standard time that daylight saving
    /// begins.
    start_time: i32,
    end: Date,
    /// Seconds after local midnight daylight time that daylight saving ends.
    end_time: i32,
}

/// A day picked out by its position in a month: the `week`th `weekday` of
/// `month`, `week` 5 meaning the last one regardless of whether a fifth
/// actually falls in the month.
#[derive(Debug, PartialEq, Clone, Copy)]
struct Date {
    month: u32,
    week: u32,
    /// `0` is Sunday, matching POSIX.
    weekday: u32,
}

impl Rule {
    /// The offset in force at `timestamp`, seconds to add to UTC.
    pub fn offset_at(&self, timestamp: i64) -> i32 {
        let Some(dst) = &self.dst else {
            return self.std_offset;
        };

        // The civil year is read straight from the UTC timestamp rather than
        // from a local date, which can be a few hours off right at a year
        // boundary. This only ever runs decades past a zone file's last
        // recorded transition, or for a `TZ` that skipped a zone file
        // entirely; a boundary case that far out, mattering for at most a
        // handful of hours once a year, is not worth a second pass to get
        // exactly right.
        let (year, _, _) = super::civil_from_days(timestamp.div_euclid(super::SECONDS_PER_DAY));

        let start = instant(dst.start, dst.start_time, self.std_offset, year);
        let end = instant(dst.end, dst.end_time, dst.offset, year);

        // Northern-hemisphere zones have `start` before `end` within the
        // year; southern-hemisphere ones run the other way, with daylight
        // saving spanning the turn of the year instead of sitting inside it.
        let in_daylight_saving = if start <= end {
            timestamp >= start && timestamp < end
        } else {
            timestamp >= start || timestamp < end
        };

        if in_daylight_saving {
            dst.offset
        } else {
            self.std_offset
        }
    }
}

/// The UTC instant `date` at `time_of_day` falls on in `year`, read as a
/// local wall-clock time in the zone that `offset` describes.
fn instant(date: Date, time_of_day: i32, offset: i32, year: i64) -> i64 {
    let days = nth_weekday_of_month(year, date.month, date.week, date.weekday);
    days * super::SECONDS_PER_DAY + i64::from(time_of_day) - i64::from(offset)
}

/// The day count of the `week`th `weekday` in `month` of `year`, `week` 5
/// meaning the last one.
fn nth_weekday_of_month(year: i64, month: u32, week: u32, weekday: u32) -> i64 {
    let first_of_month = days_from_civil(year, month, 1);
    let first_weekday = civil_weekday(first_of_month);
    let until_first_match = (i64::from(weekday) - i64::from(first_weekday)).rem_euclid(7);
    let mut day = 1 + until_first_match;

    if week == 5 {
        while day + 7 <= days_in_month(year, month) {
            day += 7;
        }
    } else {
        day += (i64::from(week) - 1) * 7;
    }

    days_from_civil(year, month, day)
}

/// The day of the week `days` (a count from 1970-01-01) falls on, `0` for
/// Sunday through `6` for Saturday. 1970-01-01 itself was a Thursday.
fn civil_weekday(days: i64) -> u32 {
    ((days + 4).rem_euclid(7)) as u32
}

/// How many days `month` of `year` has.
fn days_in_month(year: i64, month: u32) -> i64 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    days_from_civil(next_year, next_month, 1) - days_from_civil(year, month, 1)
}

/// The day count from 1970-01-01 for a proleptic Gregorian civil date.
/// Howard Hinnant's `days_from_civil`, the inverse of the `civil_from_days`
/// this shares the module with — the same shift-the-epoch-to-March
/// construction, run backwards.
fn days_from_civil(year: i64, month: u32, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;

    era * 146_097 + day_of_era - 719_468
}

/// Parses a POSIX `TZ` string, or gives up and returns `None` for anything
/// this module does not understand: a Julian-day rule, a `TZ` that is not
/// this format at all, or one that is simply malformed.
pub fn parse(tz: &str) -> Option<Rule> {
    let (_name, rest) = name(tz)?;
    let (std_posix, rest) = offset(rest)?;
    let std_offset = -std_posix;

    if rest.is_empty() {
        return Some(Rule {
            std_offset,
            dst: None,
        });
    }

    let (_name, rest) = name(rest)?;
    // "If no offset follows dst, the alternative offset is assumed to be one
    // hour ahead of standard time" — one hour ahead in the ordinary sense is
    // one hour *less* in POSIX's own subtract-from-UTC sign.
    let (dst_posix, rest) = offset(rest).unwrap_or((std_posix - 3_600, rest));
    let dst_offset = -dst_posix;

    let Some(rest) = rest.strip_prefix(',') else {
        // A DST name with no rule to say when it applies: POSIX has a
        // historical default for this case, but it is US-specific and long
        // out of date, so this falls back to standard time year-round
        // instead of guessing.
        return Some(Rule {
            std_offset,
            dst: None,
        });
    };

    let (start, rest) = date(rest)?;
    let (start_time, rest) = match rest.strip_prefix('/') {
        Some(rest) => offset(rest)?,
        None => (2 * 3_600, rest),
    };

    let rest = rest.strip_prefix(',')?;
    let (end, rest) = date(rest)?;
    let (end_time, _rest) = match rest.strip_prefix('/') {
        Some(rest) => offset(rest)?,
        None => (2 * 3_600, rest),
    };

    Some(Rule {
        std_offset,
        dst: Some(Dst {
            offset: dst_offset,
            start,
            start_time,
            end,
            end_time,
        }),
    })
}

/// A zone name, quoted in `<...>` or a bare run of letters, and the text
/// after it.
fn name(s: &str) -> Option<(&str, &str)> {
    if let Some(rest) = s.strip_prefix('<') {
        let end = rest.find('>')?;
        Some((&rest[..end], &rest[end + 1..]))
    } else {
        let end = s
            .find(|character: char| !character.is_ascii_alphabetic())
            .unwrap_or(s.len());
        if end == 0 {
            return None;
        }
        Some((&s[..end], &s[end..]))
    }
}

/// A signed `hh[:mm[:ss]]` offset, in seconds, and the text after it.
fn offset(s: &str) -> Option<(i32, &str)> {
    let (sign, s) = match s.as_bytes().first() {
        Some(b'+') => (1, &s[1..]),
        Some(b'-') => (-1, &s[1..]),
        _ => (1, s),
    };

    let (hours, s) = number(s)?;
    let mut seconds = hours * 3_600;
    let mut s = s;

    if let Some(rest) = s.strip_prefix(':') {
        let (minutes, rest) = number(rest)?;
        seconds += minutes * 60;
        s = rest;

        if let Some(rest) = s.strip_prefix(':') {
            let (secs, rest) = number(rest)?;
            seconds += secs;
            s = rest;
        }
    }

    Some((sign * seconds, s))
}

/// The digits at the front of `s`, and the text after them.
fn number(s: &str) -> Option<(i32, &str)> {
    let end = s
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(s.len());
    if end == 0 {
        return None;
    }
    s[..end].parse().ok().map(|n| (n, &s[end..]))
}

/// A month-week-day date, `Mmonth.week.day`, and the text after it. `None`
/// for a Julian-day rule or anything else this module does not read.
fn date(s: &str) -> Option<(Date, &str)> {
    let s = s.strip_prefix('M')?;
    let (month, s) = number(s)?;
    let s = s.strip_prefix('.')?;
    let (week, s) = number(s)?;
    let s = s.strip_prefix('.')?;
    let (weekday, s) = number(s)?;

    if !(1..=12).contains(&month) || !(1..=5).contains(&week) || !(0..=6).contains(&weekday) {
        return None;
    }

    Some((
        Date {
            month: month as u32,
            week: week as u32,
            weekday: weekday as u32,
        },
        s,
    ))
}

#[cfg(test)]
mod tests {
    use super::{days_from_civil, nth_weekday_of_month, parse};

    /// Seconds since the epoch for a UTC civil date and time of day, for
    /// tests to build instants from without leaning on this module's own
    /// `days_from_civil`.
    fn utc(year: i64, month: u32, day: i64, hour: i32, minute: i32) -> i64 {
        days_from_civil(year, month, day) * 86_400
            + i64::from(hour) * 3_600
            + i64::from(minute) * 60
    }

    #[test]
    fn days_from_civil_agrees_with_the_epoch() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(days_from_civil(2000, 2, 29), 11_016, "a leap day");
        assert_eq!(days_from_civil(2026, 1, 1), 20_454);
    }

    #[test]
    fn nth_weekday_finds_the_last_sunday_in_march() {
        // 2026-03-29 is the last Sunday of March 2026 (the fifth one).
        assert_eq!(
            nth_weekday_of_month(2026, 3, 5, 0),
            days_from_civil(2026, 3, 29)
        );
    }

    #[test]
    fn nth_weekday_finds_an_ordinary_first_occurrence() {
        // 2026-11-01 is a Sunday, so the first Sunday of November is that
        // same day.
        assert_eq!(
            nth_weekday_of_month(2026, 11, 1, 0),
            days_from_civil(2026, 11, 1)
        );
    }

    #[test]
    fn parses_a_zone_with_no_daylight_saving() {
        let rule = parse("UTC0").unwrap();

        assert_eq!(rule.offset_at(utc(2026, 6, 1, 12, 0)), 0);
        assert_eq!(rule.offset_at(utc(2026, 12, 1, 12, 0)), 0);
    }

    #[test]
    fn parses_new_yorks_own_rule() {
        // The actual footer `America/New_York`'s zone file carries.
        let rule = parse("EST5EDT,M3.2.0,M11.1.0/2").unwrap();

        // January: standard time, five hours behind UTC.
        assert_eq!(rule.offset_at(utc(2026, 1, 15, 12, 0)), -5 * 3_600);
        // July: daylight saving, four hours behind.
        assert_eq!(rule.offset_at(utc(2026, 7, 15, 12, 0)), -4 * 3_600);
    }

    #[test]
    fn crosses_new_yorks_spring_transition_at_the_right_instant() {
        let rule = parse("EST5EDT,M3.2.0,M11.1.0/2").unwrap();
        // 2026's second Sunday in March is the 8th; clocks spring forward at
        // 02:00 standard time, which is 07:00 UTC.
        let transition = utc(2026, 3, 8, 7, 0);

        assert_eq!(rule.offset_at(transition - 60), -5 * 3_600, "just before");
        assert_eq!(rule.offset_at(transition), -4 * 3_600, "right at it");
    }

    #[test]
    fn parses_a_zone_ahead_of_utc() {
        // The actual footer `Pacific/Auckland`'s zone file carries. New
        // Zealand's daylight saving spans the turn of the year.
        let rule = parse("NZST-12NZDT,M9.5.0,M4.1.0/3").unwrap();

        // January (southern summer): daylight saving, thirteen hours ahead.
        assert_eq!(rule.offset_at(utc(2026, 1, 15, 12, 0)), 13 * 3_600);
        // June (southern winter): standard time, twelve hours ahead.
        assert_eq!(rule.offset_at(utc(2026, 6, 15, 12, 0)), 12 * 3_600);
    }

    #[test]
    fn parses_athens_own_rule() {
        // The actual footer `Europe/Athens`'s zone file carries, and the
        // example the README already names as unsupported without this.
        let rule = parse("EET-2EEST,M3.5.0/3,M10.5.0/4").unwrap();

        assert_eq!(rule.offset_at(utc(2026, 1, 1, 0, 0)), 2 * 3_600);
        assert_eq!(rule.offset_at(utc(2026, 7, 1, 0, 0)), 3 * 3_600);
    }

    #[test]
    fn a_julian_day_rule_is_left_unparsed() {
        assert!(parse("EST5EDT,J60/2,J300/2").is_none());
    }

    #[test]
    fn nonsense_is_left_unparsed() {
        assert!(parse("").is_none());
        assert!(parse("not a tz string").is_none());
    }

    #[test]
    fn a_dst_name_with_no_rule_still_yields_the_standard_offset() {
        let rule = parse("EST5EDT").unwrap();

        assert_eq!(rule.offset_at(utc(2026, 7, 1, 0, 0)), -5 * 3_600);
    }

    #[test]
    fn a_quoted_name_is_accepted_too() {
        let rule = parse("<-05>5<-04>,M3.2.0,M11.1.0/2").unwrap();

        assert_eq!(rule.offset_at(utc(2026, 1, 15, 12, 0)), -5 * 3_600);
    }
}
