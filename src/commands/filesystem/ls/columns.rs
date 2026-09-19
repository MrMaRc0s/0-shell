//! The multi-column listing GNU prints on a real terminal.
//!
//! `-l` never does this, and neither does a piped or redirected listing: both
//! of those already print one name per line, unchanged. This is only for the
//! plain, unadorned listing a person sees at a prompt, the one the rest of
//! this module has always drawn one entry per line regardless of the
//! terminal's width. `ls::mod` decides which of the two applies; this module
//! only knows how to lay a list out once it has been told to.

use std::io::{self, Write};

use crate::signal;

use super::RESET;

/// Two names in the same row leave at least this many spaces between them.
const GAP: usize = 2;

/// One name ready to be laid out: the bytes to print, already carrying its
/// `-F` indicator if there is one, and the color to wrap them in, if any.
pub struct Entry {
    pub name: Vec<u8>,
    pub style: Option<&'static [u8]>,
}

/// Lays `entries` out in as many columns as fit inside `width`, filling each
/// column top to bottom before moving to the next — GNU's default order,
/// down rather than across. `width` is a column count, not a byte count: a
/// name is measured by the bytes it prints, the same byte-oriented world the
/// rest of this `ls` already lives in for sorting, and a color escape counts
/// for nothing, since the terminal does not draw it as a character.
pub fn write<W>(entries: &[Entry], width: usize, output: &mut W) -> io::Result<()>
where
    W: Write,
{
    if entries.is_empty() {
        return Ok(());
    }

    let widths: Vec<usize> = entries.iter().map(|entry| entry.name.len()).collect();
    let columns = best_fit(&widths, width);
    let rows = entries.len().div_ceil(columns);

    let mut column_widths = vec![0usize; columns];
    for (index, name_width) in widths.iter().enumerate() {
        let column = index / rows;
        column_widths[column] = column_widths[column].max(*name_width);
    }

    for row in 0..rows {
        // A Ctrl+C stops the listing at the row it is drawing, the same grain
        // `ls -l` already interrupts at.
        if signal::is_pending() {
            break;
        }

        // `column` is arithmetic here, not just an index: it locates the
        // entry at `column * rows + row` as well as that column's width, so
        // the `enumerate` clippy suggests for a bare index does not apply.
        #[allow(clippy::needless_range_loop)]
        for column in 0..columns {
            let index = column * rows + row;
            if index >= entries.len() {
                break;
            }

            let entry = &entries[index];
            if let Some(style) = entry.style {
                output.write_all(style)?;
            }
            output.write_all(&entry.name)?;
            if entry.style.is_some() {
                output.write_all(RESET)?;
            }

            // No trailing spaces after the last name on a row, whether that
            // is because the columns ran out or the entries did.
            let next = index + rows;
            if column + 1 < columns && next < entries.len() {
                let padding = column_widths[column] - widths[index] + GAP;
                output.write_all(&vec![b' '; padding])?;
            }
        }
        output.write_all(b"\n")?;
    }

    Ok(())
}

/// The most columns `widths` can be arranged into without a row running past
/// `available`.
///
/// Tried by row count, from one row upward, rather than by column count from
/// many down: fixing the row count first and deriving `columns =
/// ceil(count / rows)` from it is what guarantees every column holds at
/// least one entry. Choosing a column count directly and deriving rows from
/// *that* can overshoot — for six names, five columns needs two rows to hold
/// them, but two rows times five columns is ten slots for six names, leaving
/// two columns empty. Six names arrange cleanly into six, three, two, or one
/// columns and never into five.
///
/// Quadratic in the entry count, recomputing every column's width from
/// scratch for every candidate row count, which is what keeps this simple
/// rather than the running totals GNU's own implementation maintains. A
/// directory listing is not a place this cost is ever going to be felt.
fn best_fit(widths: &[usize], available: usize) -> usize {
    let count = widths.len();

    for rows in 1..=count {
        let columns = count.div_ceil(rows);
        let mut total = 0usize;
        let mut fits = true;

        for column in 0..columns {
            let column_width = (0..rows)
                .map(|row| column * rows + row)
                .take_while(|index| *index < count)
                .map(|index| widths[index])
                .max()
                .unwrap_or(0);

            total += column_width;
            if column + 1 < columns {
                total += GAP;
            }
            if total > available {
                fits = false;
                break;
            }
        }

        if fits {
            return columns;
        }
    }

    1
}

#[cfg(test)]
mod tests {
    use super::{Entry, best_fit, write};

    fn entry(name: &str) -> Entry {
        Entry {
            name: name.as_bytes().to_vec(),
            style: None,
        }
    }

    fn drawn(names: &[&str], width: usize) -> String {
        let entries: Vec<Entry> = names.iter().map(|name| entry(name)).collect();
        let mut output = Vec::new();
        write(&entries, width, &mut output).unwrap();
        String::from_utf8(output).unwrap()
    }

    #[test]
    fn everything_fits_the_width_lands_on_one_row() {
        assert_eq!(drawn(&["a", "b", "c"], 80), "a  b  c\n");
    }

    #[test]
    fn a_single_entry_is_its_own_row() {
        assert_eq!(drawn(&["only"], 80), "only\n");
    }

    #[test]
    fn an_empty_list_prints_nothing() {
        assert_eq!(drawn(&[], 80), "");
    }

    #[test]
    fn narrower_than_the_widest_name_still_prints_one_column() {
        assert_eq!(
            drawn(&["short", "a-very-long-name"], 4),
            "short\na-very-long-name\n"
        );
    }

    #[test]
    fn fills_down_each_column_before_moving_to_the_next() {
        // Six names, a width too narrow for six or four columns but wide
        // enough for three: reading down first, the first column holds "one"
        // and "two", not "one" and "four".
        let text = drawn(&["one", "two", "three", "four", "five", "six"], 20);

        assert_eq!(
            text,
            "one  three  five\n\
             two  four   six\n"
        );
    }

    #[test]
    fn each_column_is_only_as_wide_as_its_own_longest_name() {
        // Column 0 holds "a" and "ccc", so it is sized to fit "ccc"; column 1
        // holds "bb" and "d" and is sized to fit "bb" alone, independently.
        let text = drawn(&["a", "ccc", "bb", "d"], 7);

        assert_eq!(text, "a    bb\nccc  d\n");
    }

    #[test]
    fn best_fit_picks_the_widest_layout_that_still_fits() {
        let widths = [1, 1, 1, 1]; // four one-byte names
        // Four columns of one byte, three two-space gaps between them: 4 + 6.
        assert_eq!(best_fit(&widths, 10), 4);
        // One byte narrower and four columns no longer fit. The next layout
        // down is two columns, not three: four one-byte names arrange
        // cleanly into four, two, or one columns and never into three.
        assert_eq!(best_fit(&widths, 9), 2);
    }

    #[test]
    fn best_fit_falls_back_to_one_column_when_nothing_else_fits() {
        assert_eq!(best_fit(&[50], 10), 1);
    }
}
