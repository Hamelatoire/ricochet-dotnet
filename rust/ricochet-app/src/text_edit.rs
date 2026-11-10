//! Text rules for the editing fields, with no window: where lines begin, the position of a byte offset, moving up and
//! down, and the UTF-16 offsets that the input method uses. Byte offsets are always on a character boundary.

use std::ops::Range;

/// The byte offset where each line begins. The first line always begins at 0, so there is at least one entry.
pub fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(text.match_indices('\n').map(|(index, _)| index + 1));
    starts
}

/// The line and the byte column of an offset. An offset past the end is taken as the end.
pub fn line_and_column(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    let starts = line_starts(text);
    let line = starts.partition_point(|&start| start <= offset).saturating_sub(1);
    (line, offset - starts[line])
}

/// The byte offset of a line and column. The line is clamped to the last one, and the column to the end of its line
/// and to a character boundary.
pub fn offset_of(text: &str, line: usize, column: usize) -> usize {
    let starts = line_starts(text);
    let line = line.min(starts.len() - 1);
    let start = starts[line];
    let end = starts.get(line + 1).map_or(text.len(), |next| next - 1);
    let mut offset = start + column.min(end - start);
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// The offset one line up (`delta` -1) or down (`delta` 1), at the same column where the line is long enough.
pub fn move_vertical(text: &str, offset: usize, delta: isize) -> usize {
    let (line, column) = line_and_column(text, offset);
    let target = line as isize + delta;
    if target < 0 {
        return 0;
    }
    if target as usize >= line_starts(text).len() {
        return text.len();
    }
    offset_of(text, target as usize, column)
}

/// The text of each line, without its line break.
pub fn lines(text: &str) -> Vec<&str> {
    text.split('\n').collect()
}

pub fn offset_from_utf16(text: &str, offset: usize) -> usize {
    let mut utf8 = 0;
    let mut utf16 = 0;
    for ch in text.chars() {
        if utf16 >= offset {
            break;
        }
        utf16 += ch.len_utf16();
        utf8 += ch.len_utf8();
    }
    utf8
}

pub fn offset_to_utf16(text: &str, offset: usize) -> usize {
    let mut utf16 = 0;
    let mut utf8 = 0;
    for ch in text.chars() {
        if utf8 >= offset {
            break;
        }
        utf8 += ch.len_utf8();
        utf16 += ch.len_utf16();
    }
    utf16
}

pub fn range_to_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    offset_to_utf16(text, range.start)..offset_to_utf16(text, range.end)
}

pub fn range_from_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    offset_from_utf16(text, range.start)..offset_from_utf16(text, range.end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_and_columns_round_trip() {
        let text = "ab\ncdé\n\nf";
        assert_eq!(line_starts(text), vec![0, 3, 8, 9]);
        for offset in [0, 1, 2, 3, 4, 5, 7, 8, 9] {
            let (line, column) = line_and_column(text, offset);
            assert_eq!(offset_of(text, line, column), offset, "offset {offset}");
        }
        assert_eq!(line_and_column(text, 6), (1, 3));
        assert_eq!(line_and_column("", 0), (0, 0));
    }

    #[test]
    fn the_column_is_clamped_to_the_end_of_a_short_line() {
        let text = "long line\nx\nlonger";
        assert_eq!(offset_of(text, 1, 7), 11, "the end of the short line, before its break");
        assert_eq!(offset_of(text, 9, 0), 12, "a line past the last is the last");
    }

    #[test]
    fn moving_up_and_down_keeps_the_column_where_it_can() {
        let text = "abcdef\nxy\nghijkl";
        assert_eq!(move_vertical(text, 4, 1), 9, "the short line ends the move");
        assert_eq!(move_vertical(text, 12, -1), 9);
        assert_eq!(move_vertical(text, 3, -1), 0);
        assert_eq!(move_vertical(text, 3, 5), text.len());
    }

    #[test]
    fn moving_never_lands_inside_a_character() {
        let text = "é\nxy";
        assert_eq!(offset_of(text, 0, 1), 0, "column 1 is inside é, so it goes back to its start");
        assert_eq!(move_vertical(text, 2, 1), 5, "the end of the second line");
    }

    #[test]
    fn utf16_offsets_convert_both_ways() {
        let text = "a😀b";
        assert_eq!(offset_to_utf16(text, 5), 3);
        assert_eq!(offset_from_utf16(text, 3), 5);
        assert_eq!(range_from_utf16(text, &(1..3)), 1..5);
    }
}
