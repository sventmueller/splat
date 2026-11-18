//! Which line of the source a place lies on, for the numbers beside it. A line ends where TextKit ends a paragraph: at a
//! line feed, a carriage return, both together, or a paragraph separator.

use crate::markdown::text::{Byte, U16};

/// Counts lines from the place it was last asked about, so that the next question near it costs only the text in between.
/// An edit before that place makes it count from the start again, which takes some milliseconds in a text of 10 MB.
#[derive(Clone, Copy, Debug, Default)]
pub struct LineCounter {
    known: Option<Place>,
}

/// A place in the text by both its offsets, with the line breaks before it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Place {
    at: U16,
    byte: Byte,
    breaks: usize,
}

impl LineCounter {
    /// The line, counted from 1, that position `at` of `text` lies on; a position past the end lies on the last line.
    pub fn line_at(&mut self, text: &str, at: U16) -> usize {
        let start = self
            .known
            .filter(|known| text.is_char_boundary(known.byte.get()) && known.at.distance(at) < at.get())
            .unwrap_or_default();
        let place = walk(text.as_bytes(), start, at);
        self.known = Some(place);
        place.breaks + 1
    }

    /// The lines of `starts`, in order, of which it keeps only the first: edits below the top of what shows leave that
    /// place as it was.
    pub fn lines_at(&mut self, text: &str, starts: &[U16]) -> Vec<usize> {
        let Some(&first) = starts.first() else {
            return Vec::new();
        };
        self.line_at(text, first);
        let mut place = self.known.unwrap_or_default();
        starts
            .iter()
            .map(|&at| {
                place = walk(text.as_bytes(), place, at);
                place.breaks + 1
            })
            .collect()
    }

    /// The text changed from `from` on; what lies before stays as it was.
    pub fn edited(&mut self, from: U16) {
        if self.known.is_some_and(|known| known.at > from) {
            self.known = None;
        }
    }
}

/// How many lines `text` has, as many as the number of the last one.
pub fn line_count(text: &str) -> usize {
    let bytes = text.as_bytes();
    let feeds = bytes.iter().filter(|&&b| b == b'\n').count();
    let returns = bytes.iter().filter(|&&b| b == b'\r').count();
    let pairs = if returns == 0 {
        0
    } else {
        bytes.windows(2).filter(|pair| *pair == b"\r\n").count()
    };
    feeds + returns - pairs + text.matches('\u{2029}').count() + 1
}

/// Whether a paragraph ends with the character that starts at `i`.
fn breaks_at(bytes: &[u8], i: usize) -> bool {
    match bytes[i] {
        b'\n' => true,
        b'\r' => bytes.get(i + 1) != Some(&b'\n'),
        0xE2 => bytes.get(i + 1..i + 3) == Some(&[0x80, 0xA9]),
        _ => false,
    }
}

/// UTF-16 units of the character whose UTF-8 encoding starts with `lead`.
fn units(lead: u8) -> usize {
    if lead >= 0xF0 { 2 } else { 1 }
}

/// UTF-8 bytes of the character that starts with `lead`.
fn length(lead: u8) -> usize {
    match lead {
        0..0x80 => 1,
        0x80..0xE0 => 2,
        0xE0..0xF0 => 3,
        _ => 4,
    }
}

fn walk(bytes: &[u8], from: Place, at: U16) -> Place {
    if from.at <= at {
        forward(bytes, from, at)
    } else {
        backward(bytes, from, at)
    }
}

fn forward(bytes: &[u8], mut place: Place, at: U16) -> Place {
    let mut i = place.byte.get();
    while i < bytes.len() && place.at < at {
        place.at += units(bytes[i]);
        place.breaks += usize::from(breaks_at(bytes, i));
        i += length(bytes[i]);
    }
    place.byte = Byte::new(i);
    place
}

fn backward(bytes: &[u8], mut place: Place, at: U16) -> Place {
    let mut i = place.byte.get();
    while i > 0 && place.at > at {
        i -= 1;
        while bytes[i] & 0xC0 == 0x80 {
            i -= 1;
        }
        place.at -= units(bytes[i]);
        place.breaks -= usize::from(breaks_at(bytes, i));
    }
    place.byte = Byte::new(i);
    place
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The line of every position, counted the slow way.
    fn lines_by_position(text: &str) -> Vec<usize> {
        let mut lines = Vec::new();
        let mut line = 1;
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            for _ in 0..c.len_utf16() {
                lines.push(line);
            }
            if c == '\n' || c == '\u{2029}' || (c == '\r' && chars.peek() != Some(&'\n')) {
                line += 1;
            }
        }
        lines.push(line);
        lines
    }

    #[test]
    fn every_kind_of_line_end_starts_a_line() {
        let mut counter = LineCounter::default();
        let text = "a\nb\r\nc\rd\u{2029}e";
        let starts: Vec<usize> = [0, 2, 5, 7, 9]
            .iter()
            .map(|&at| counter.line_at(text, U16::new(at)))
            .collect();
        assert_eq!(starts, [1, 2, 3, 4, 5]);
        assert_eq!(line_count(text), 5);
        assert_eq!(line_count(""), 1);
        assert_eq!(line_count("ends with a break\n"), 2);
        assert_eq!(LineCounter::default().line_at("ends with a break\n", U16::new(99)), 2);
    }

    #[test]
    fn counting_on_from_any_place_finds_what_counting_from_the_start_finds() {
        let text = "# Überschrift\n\n😀 Absatz mit\r\nzwei Zeilen\r\rund – Strich\u{2029}Ende\n";
        let expected = lines_by_position(text);
        let mut counter = LineCounter::default();
        // Forward, backward and jumps from wherever the last question left it.
        let mut at = 0usize;
        for step in [5usize, 13, 3, 29, 7, 41, 2, 17, 11, 23] {
            at = (at * 7 + step) % expected.len();
            assert_eq!(counter.line_at(text, U16::new(at)), expected[at], "at {at}");
        }
        for at in (0..expected.len()).rev() {
            assert_eq!(counter.line_at(text, U16::new(at)), expected[at], "back at {at}");
        }
        assert_eq!(line_count(text), expected[expected.len() - 1]);
    }

    #[test]
    fn an_edit_before_the_last_place_makes_it_count_anew() {
        let mut counter = LineCounter::default();
        assert_eq!(counter.line_at("a\nb\nc\nd", U16::new(6)), 4);
        counter.edited(U16::new(6));
        assert_eq!(
            counter.line_at("a\nb\nc\nd!", U16::new(7)),
            4,
            "an edit behind it keeps what it knew"
        );
        counter.edited(U16::new(0));
        assert_eq!(counter.known, None);
        assert_eq!(counter.line_at("\n\na\nb\nc\nd!", U16::new(8)), 6);
    }

    #[test]
    fn the_lines_in_view_keep_the_first_as_the_place_to_count_from() {
        let text = "a\nb\nc\nd\ne";
        let mut counter = LineCounter::default();
        assert_eq!(counter.lines_at(text, &[]), Vec::<usize>::new());
        let starts = [4, 6, 8].map(U16::new);
        assert_eq!(counter.lines_at(text, &starts), [3, 4, 5]);
        counter.edited(U16::new(5));
        assert!(counter.known.is_some(), "typing below the top keeps it");
        assert_eq!(counter.lines_at("a\nb\nc!\nd\ne", &[4, 7, 9].map(U16::new)), [3, 4, 5]);
        counter.edited(U16::new(1));
        assert_eq!(counter.known, None);
    }

    #[test]
    fn a_place_the_text_no_longer_has_is_forgotten() {
        let mut counter = LineCounter::default();
        assert_eq!(counter.line_at("ab\nc", U16::new(1)), 1);
        assert_eq!(
            counter.line_at("ä\nb", U16::new(2)),
            2,
            "the old place lies inside a character now"
        );
        assert_eq!(counter.line_at("a", U16::new(1)), 1, "and now past the end");
    }

    #[test]
    fn a_position_inside_a_surrogate_pair_counts_with_its_character() {
        let mut counter = LineCounter::default();
        assert_eq!(counter.line_at("ab\n😀\n", U16::new(4)), 2);
        assert_eq!(counter.line_at("ab\n😀\n", U16::new(6)), 3);
        assert_eq!(counter.line_at("ab\n😀\n", U16::new(4)), 2, "counted back into it");
    }
}
