use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::OnceLock;

/// What the character reference at the start of `s` stands for and how long it is, by the rules comrak parses text
/// with: only references ending in `;` count, unknown names stay text, and impossible numbers become U+FFFD.
pub fn decode_entity(s: &str) -> Option<(Cow<'static, str>, usize)> {
    static NAMED: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    let end = s[..s.floor_char_boundary(40)].find(';')?;
    let name = s.get(1..end)?;
    if let Some(number) = name.strip_prefix('#') {
        let (digits, radix, most) = match number.strip_prefix(['x', 'X']) {
            Some(hex) => (hex, 16, 6),
            None => (number, 10, 7),
        };
        if digits.is_empty() || digits.len() > most {
            return None;
        }
        let code = u32::from_str_radix(digits, radix).ok()?;
        let c = match code {
            0 | 0xE000 => '\u{FFFD}',
            _ => char::from_u32(code).unwrap_or('\u{FFFD}'),
        };
        return Some((c.to_string().into(), end + 1));
    }
    let named = NAMED.get_or_init(|| {
        entities::ENTITIES
            .iter()
            .filter_map(|e| Some((e.entity.strip_prefix('&')?.strip_suffix(';')?, e.characters)))
            .collect()
    });
    named.get(name).map(|c| (Cow::Borrowed(*c), end + 1))
}

macro_rules! utf16_position {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub usize);

        impl std::ops::Add<usize> for $name {
            type Output = $name;
            fn add(self, units: usize) -> $name {
                $name(self.0 + units)
            }
        }

        impl std::ops::Sub<usize> for $name {
            type Output = $name;
            fn sub(self, units: usize) -> $name {
                $name(self.0 - units)
            }
        }

        impl std::ops::AddAssign<usize> for $name {
            fn add_assign(&mut self, units: usize) {
                self.0 += units;
            }
        }

        /// The distance in UTF-16 units.
        impl std::ops::Sub for $name {
            type Output = usize;
            fn sub(self, earlier: $name) -> usize {
                self.0 - earlier.0
            }
        }
    };
}

utf16_position!(
    /// A position in the text of a source view, in UTF-16 units as AppKit counts them. Everything in the model counts
    /// in bytes, in plain `usize`; with a type of its own, passing one for the other does not compile.
    U16
);

utf16_position!(
    /// A position in the text of the preview view, in UTF-16 units. It has nothing to do with a position in the source
    /// but what `preview_offset` and `insert_point` say.
    Shown
);

/// Where the line holding the byte offset `at` starts.
/// The end of the line that holds `at`, before its line ending.
pub fn line_end(text: &str, at: usize) -> usize {
    text[at..].find(['\n', '\r']).map_or(text.len(), |i| at + i)
}

/// The lines of `text`, each with what ends it: `\n`, `\r\n`, `\r` or, for the last, nothing.
pub fn lines_with_ends(text: &str) -> impl Iterator<Item = (&str, &str)> {
    let mut rest = Some(text);
    std::iter::from_fn(move || {
        let current = rest?;
        let Some(at) = current.find(['\n', '\r']) else {
            rest = None;
            return Some((current, ""));
        };
        let len = if current[at..].starts_with("\r\n") { 2 } else { 1 };
        rest = Some(&current[at + len..]);
        Some((&current[..at], &current[at..at + len]))
    })
}

pub fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind(['\n', '\r']).map_or(0, |i| i + 1)
}

/// A byte offset moved along with an edit before it. An edit cannot move text before the start of the document.
pub fn shifted(at: usize, delta: isize) -> usize {
    at.saturating_add_signed(delta)
}

/// A count where 32 bits are plenty: a text stays far below their reach, and a count at the limit does no harm.
pub fn count32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// How much longer `new` is than `old`, in bytes.
pub fn growth(new: usize, old: usize) -> isize {
    isize::try_from(new).unwrap_or(isize::MAX) - isize::try_from(old).unwrap_or(isize::MAX)
}

/// The length of a range of positions.
pub fn units<T: Copy + std::ops::Sub<Output = usize>>(range: &Range<T>) -> usize {
    range.end - range.start
}

/// Byte offsets of line starts, used to resolve comrak's 1-based line/byte-column positions.
pub struct LineStarts(Vec<usize>);

impl LineStarts {
    pub fn new(text: &str) -> Self {
        let mut starts = vec![0];
        if text.contains('\r') {
            // comrak ends a line at `\r` too, as files from old Macs do.
            let bytes = text.as_bytes();
            starts.extend(bytes.iter().enumerate().filter_map(|(i, &b)| {
                (b == b'\n' || (b == b'\r' && bytes.get(i + 1) != Some(&b'\n'))).then_some(i + 1)
            }));
        } else {
            starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        }
        LineStarts(starts)
    }

    fn line_start(&self, line: usize) -> usize {
        let idx = line.saturating_sub(1).min(self.0.len() - 1);
        self.0[idx]
    }

    /// Start offset of a position whose column is 1-based.
    pub fn start(&self, line: usize, column: usize) -> usize {
        self.line_start(line) + column.saturating_sub(1)
    }

    /// Exclusive end offset of a position whose column points at the last byte (inclusive).
    pub fn end(&self, line: usize, column: usize) -> usize {
        self.line_start(line) + column
    }

    pub fn line_of(&self, offset: usize) -> usize {
        match self.0.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        }
    }

    pub fn line_range(&self, text: &str, line: usize) -> Range<usize> {
        let start = self.0[line];
        let end = self.0.get(line + 1).copied().unwrap_or(text.len());
        start..end
    }
}

/// Maps byte offsets of a UTF-8 string to UTF-16 code-unit offsets as used by NSString. Pure ASCII text needs no
/// table, since both offsets are the same there.
pub struct Utf16Map {
    byte_to_u16: Vec<u32>,
    ascii_len: Option<usize>,
}

impl Utf16Map {
    pub fn new(text: &str) -> Self {
        if text.is_ascii() {
            return Utf16Map {
                byte_to_u16: Vec::new(),
                ascii_len: Some(text.len()),
            };
        }
        let mut byte_to_u16 = Vec::with_capacity(text.len() + 1);
        let mut u16_pos = 0u32;
        for ch in text.chars() {
            for _ in 0..ch.len_utf8() {
                byte_to_u16.push(u16_pos);
            }
            u16_pos += count32(ch.len_utf16());
        }
        byte_to_u16.push(u16_pos);
        Utf16Map {
            byte_to_u16,
            ascii_len: None,
        }
    }

    pub fn len_u16(&self) -> usize {
        match self.ascii_len {
            Some(len) => len,
            None => *self.byte_to_u16.last().unwrap() as usize,
        }
    }

    pub fn to_u16(&self, byte: usize) -> U16 {
        U16(match self.ascii_len {
            Some(len) => byte.min(len),
            None => self.byte_to_u16[byte.min(self.byte_to_u16.len() - 1)] as usize,
        })
    }

    /// Byte offset of the char boundary at or after the given position.
    pub fn to_byte(&self, position: U16) -> usize {
        if let Some(len) = self.ascii_len {
            return position.0.min(len);
        }
        let target = count32(position.0);
        let idx = self.byte_to_u16.partition_point(|&v| v < target);
        idx.min(self.byte_to_u16.len() - 1)
    }
}

/// UTF-16 length of a string slice.
pub fn utf16_len(s: &str) -> usize {
    // Every byte but a continuation byte starts a char; four-byte chars take a surrogate pair. Two counting passes
    // vectorise, one pass adding both does not.
    let b = s.as_bytes();
    b.iter().filter(|&&x| x & 0xC0 != 0x80).count() + b.iter().filter(|&&x| x >= 0xF0).count()
}

/// Whether `text` reads right to left: its first letter is Hebrew, Arabic or another right-to-left script, which is
/// how the Unicode bidirectional algorithm picks the direction of a paragraph. `None` for a text without letters.
pub fn right_to_left(text: &str) -> Option<bool> {
    text.chars()
        .find(|c| c.is_alphabetic())
        .map(|c| matches!(c, '\u{0590}'..='\u{08FF}' | '\u{FB1D}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}'))
}

/// Length of the common byte prefix of two texts.
pub fn common_prefix(a: &[u8], b: &[u8]) -> usize {
    const CHUNK: usize = 4096;
    let n = a.len().min(b.len());
    let mut i = 0;
    while i + CHUNK <= n && a[i..i + CHUNK] == b[i..i + CHUNK] {
        i += CHUNK;
    }
    i + a[i..n].iter().zip(&b[i..n]).take_while(|(x, y)| x == y).count()
}

/// Length of the common byte suffix of two texts, at most `max`.
pub fn common_suffix(a: &[u8], b: &[u8], max: usize) -> usize {
    const CHUNK: usize = 4096;
    let n = a.len().min(b.len()).min(max);
    let (a, b) = (&a[a.len() - n..], &b[b.len() - n..]);
    let mut i = 0;
    while i + CHUNK <= n && a[n - i - CHUNK..n - i] == b[n - i - CHUNK..n - i] {
        i += CHUNK;
    }
    i + a[..n - i]
        .iter()
        .rev()
        .zip(b[..n - i].iter().rev())
        .take_while(|(x, y)| x == y)
        .count()
}

/// Byte offset reached after `units` UTF-16 code units from byte `from`, at a char boundary.
pub fn byte_at_u16(text: &str, from: usize, units: usize) -> usize {
    advance_u16(text, from, units).0
}

/// Like `byte_at_u16`, but `None` when `units` ends inside a surrogate pair or beyond the text.
pub fn byte_at_u16_exact(text: &str, from: usize, units: usize) -> Option<usize> {
    let (byte, reached) = advance_u16(text, from, units);
    (reached == units).then_some(byte)
}

fn advance_u16(text: &str, from: usize, units: usize) -> (usize, usize) {
    const CHUNK: usize = 4096;
    let mut at = from;
    let mut count = 0;
    // Counting a whole chunk vectorises; walking it char by char is ten times slower.
    while at + CHUNK < text.len() {
        let end = text.floor_char_boundary(at + CHUNK);
        let n = utf16_len(&text[at..end]);
        if count + n > units {
            break;
        }
        count += n;
        at = end;
    }
    for (i, c) in text[at..].char_indices() {
        if count >= units {
            return (at + i, count);
        }
        count += c.len_utf16();
    }
    (text.len(), count)
}

/// Converts increasing byte offsets of a text to UTF-16 offsets in one forward pass, without a table.
pub struct Utf16Cursor<'a> {
    text: &'a str,
    byte: usize,
    u16: usize,
}

impl<'a> Utf16Cursor<'a> {
    pub fn new(text: &'a str) -> Self {
        Utf16Cursor { text, byte: 0, u16: 0 }
    }

    /// UTF-16 offset of `byte`, or of the character `byte` falls into; fastest for increasing offsets.
    pub fn at(&mut self, byte: usize) -> usize {
        let byte = self.text.floor_char_boundary(byte.min(self.text.len()));
        if byte < self.byte {
            self.byte = 0;
            self.u16 = 0;
        }
        self.u16 += utf16_len(&self.text[self.byte..byte]);
        self.byte = byte;
        self.u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_starts_behind_the_line_break_before_it() {
        assert_eq!(line_start("eins\nzwei", 0), 0);
        assert_eq!(line_start("eins\nzwei", 4), 0);
        assert_eq!(line_start("eins\nzwei", 5), 5);
        assert_eq!(line_start("eins\nzwei", 8), 5);
    }

    #[test]
    fn character_references_follow_the_parser() {
        let decoded = |s: &str| decode_entity(s).map(|(text, len)| (text.into_owned(), len));
        assert_eq!(decoded("&eacute; au"), Some(("é".into(), 8)));
        assert_eq!(decoded("&#233;"), Some(("é".into(), 6)));
        assert_eq!(decoded("&#xE9;"), Some(("é".into(), 6)));
        assert_eq!(decoded("&NotEqualTilde;"), Some(("≂\u{338}".into(), 15)));
        assert_eq!(decoded("&#0;"), Some(("\u{FFFD}".into(), 4)));
        assert_eq!(decoded("&#xD800;"), Some(("\u{FFFD}".into(), 8)));
        for text in [
            "&gibtsnicht;",
            "& b;",
            "&#;",
            "&#x;",
            "&#12345678;",
            "&amp",
            "&#-1;",
            "&#1a;",
        ] {
            assert_eq!(decoded(text), None, "{text}");
        }
    }

    #[test]
    fn the_first_letter_decides_the_direction() {
        assert_eq!(right_to_left("12. • משימה"), Some(true));
        assert_eq!(right_to_left("«النص» English"), Some(true));
        assert_eq!(right_to_left("Splat בעברית"), Some(false));
        assert_eq!(right_to_left("١٢٣ ..."), None);
    }

    #[test]
    fn cursor_inside_a_character_counts_up_to_it() {
        let mut cursor = Utf16Cursor::new("aä😀b");
        assert_eq!(cursor.at(2), 1);
        assert_eq!(cursor.at(5), 2);
        assert_eq!(cursor.at(99), 5);
    }

    #[test]
    fn utf16_roundtrip_with_umlauts_and_emoji() {
        let s = "aä😀b";
        let m = Utf16Map::new(s);
        assert_eq!(m.len_u16(), 5);
        assert_eq!(m.to_u16(0), U16(0));
        assert_eq!(m.to_u16(1), U16(1));
        assert_eq!(m.to_u16(3), U16(2));
        assert_eq!(m.to_u16(7), U16(4));
        assert_eq!(m.to_byte(U16(2)), 3);
        assert_eq!(m.to_byte(U16(4)), 7);
        assert_eq!(m.to_byte(U16(3)), 7);
        assert_eq!(
            (byte_at_u16(s, 0, 2), byte_at_u16(s, 1, 1), byte_at_u16(s, 0, 9)),
            (3, 3, 8)
        );
        let long = "ä😀".repeat(3000);
        assert_eq!(byte_at_u16(&long, 0, 2999 * 3), 2999 * 6);
        assert_eq!(byte_at_u16_exact(&long, 0, 2999 * 3 + 1), Some(2999 * 6 + 2));
        assert_eq!(byte_at_u16_exact(&long, 0, 2999 * 3 + 2), None);
        assert_eq!(byte_at_u16_exact(&long, 0, 3000 * 3), Some(long.len()));
        assert_eq!(byte_at_u16_exact(&long, 0, 3000 * 3 + 1), None);
        assert_eq!((utf16_len("aä😀"), utf16_len("")), (4, 0));
        let long_a = "x".repeat(9000) + "abc" + &"y".repeat(9000);
        let long_b = "x".repeat(9000) + "aXc" + &"y".repeat(9000);
        assert_eq!(common_prefix(long_a.as_bytes(), long_b.as_bytes()), 9001);
        assert_eq!(common_suffix(long_a.as_bytes(), long_b.as_bytes(), usize::MAX), 9001);
        assert_eq!(common_suffix(b"abc", b"abc", 2), 2);
        let mut c = Utf16Cursor::new(s);
        assert_eq!((c.at(3), c.at(7), c.at(1)), (2, 4, 1));
        let a = Utf16Map::new("abc");
        assert_eq!((a.len_u16(), a.to_u16(2), a.to_byte(U16(9))), (3, U16(2), 3));
    }

    #[test]
    fn line_starts_resolve_inclusive_ends() {
        let s = "ab\nÜx\n";
        let l = LineStarts::new(s);
        assert_eq!(l.start(2, 1), 3);
        assert_eq!(l.end(2, 3), 6);
        assert_eq!(&s[l.start(2, 1)..l.end(2, 3)], "Üx");
        assert_eq!(l.end(3, 0), 7);
        assert_eq!(l.line_of(4), 1);
    }

    #[test]
    fn a_character_reference_may_have_as_many_digits_as_its_base_allows() {
        assert_eq!(decode_entity("&#x01F600;"), Some(("😀".into(), 10)));
        assert_eq!(decode_entity("&#128512;"), Some(("😀".into(), 9)));
        assert_eq!(decode_entity("&#0128512;"), Some(("😀".into(), 10)));
        assert_eq!(decode_entity("&#x001F600;"), None);
        assert_eq!(decode_entity("&#00128512;"), None);
    }

    #[test]
    fn a_line_beyond_the_end_starts_where_the_last_one_does() {
        let lines = LineStarts::new("a\nb");
        assert_eq!(lines.line_start(2), 2);
        assert_eq!(lines.line_start(99), 2);
    }

    #[test]
    fn long_texts_count_utf16_like_walking_them_letter_by_letter() {
        let text = "aé😀".repeat(3000);
        let walked = |units: usize| {
            let mut count = 0;
            for (i, c) in text.char_indices() {
                if count >= units {
                    return (i, count);
                }
                count += c.len_utf16();
            }
            (text.len(), count)
        };
        for units in [0, 1, 3, 2000, 4096, 5000, 11_999, 12_000, 20_000] {
            assert_eq!(advance_u16(&text, 0, units), walked(units), "{units}");
        }
    }

    #[test]
    fn the_length_of_a_range_is_the_distance_of_its_ends() {
        assert_eq!(units(&(2_usize..5)), 3);
    }

    #[test]
    fn a_position_beyond_a_text_with_umlauts_lands_at_its_end() {
        let map = Utf16Map::new("aé😀");
        assert_eq!(map.to_u16(100), U16(4));
        assert_eq!(map.to_byte(U16(100)), 7);
    }

    #[test]
    fn long_texts_compare_correctly_on_both_sides_of_every_chunk() {
        let same = "x".repeat(10_000);
        let different_at = |at: usize| {
            let mut other = same.clone();
            other.replace_range(at..at + 1, "y");
            other
        };
        for at in [0, 100, 4096, 5000, 9000, 9999] {
            let other = different_at(at);
            assert_eq!(common_prefix(same.as_bytes(), other.as_bytes()), at, "{at}");
            assert_eq!(
                common_suffix(same.as_bytes(), other.as_bytes(), 10_000),
                9999 - at,
                "{at}"
            );
        }
        assert_eq!(common_prefix(same.as_bytes(), same.as_bytes()), 10_000);
        assert_eq!(common_suffix(same.as_bytes(), same.as_bytes(), 6000), 6000);
    }
}
