use std::ops::Range;

/// Byte offsets of line starts, used to resolve comrak's 1-based line/byte-column positions.
pub struct LineStarts(Vec<usize>);

impl LineStarts {
    pub fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
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
            u16_pos += ch.len_utf16() as u32;
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

    pub fn to_u16(&self, byte: usize) -> usize {
        match self.ascii_len {
            Some(len) => byte.min(len),
            None => self.byte_to_u16[byte.min(self.byte_to_u16.len() - 1)] as usize,
        }
    }

    /// Byte offset of the char boundary at or after the given UTF-16 offset.
    pub fn to_byte(&self, u16_offset: usize) -> usize {
        if let Some(len) = self.ascii_len {
            return u16_offset.min(len);
        }
        let target = u16_offset as u32;
        let idx = self.byte_to_u16.partition_point(|&v| v < target);
        idx.min(self.byte_to_u16.len() - 1)
    }
}

/// UTF-16 length of a string slice.
pub fn utf16_len(s: &str) -> usize {
    // Every byte but a continuation byte starts a char; four-byte chars take a surrogate pair. Two counting passes
    // vectorise, one pass adding both does not.
    let b = s.as_bytes();
    b.iter().filter(|&&x| (x as i8) >= -0x40).count() + b.iter().filter(|&&x| x >= 0xF0).count()
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
        assert_eq!(m.to_u16(0), 0);
        assert_eq!(m.to_u16(1), 1);
        assert_eq!(m.to_u16(3), 2);
        assert_eq!(m.to_u16(7), 4);
        assert_eq!(m.to_byte(2), 3);
        assert_eq!(m.to_byte(4), 7);
        assert_eq!(m.to_byte(3), 7);
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
        assert_eq!((a.len_u16(), a.to_u16(2), a.to_byte(9)), (3, 2, 3));
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
}
