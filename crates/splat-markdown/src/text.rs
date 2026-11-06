//! Positions in a source text and what is counted by them: byte and UTF-16 offsets, lines, the marks that open
//! containers, and the changed part of an edit.

use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::OnceLock;

/// What the character reference at the start of `s` stands for and how long it is, by the rules comrak parses text
/// with: only references ending in `;` count, unknown names stay text, and impossible numbers become U+FFFD.
pub fn decode_entity(s: &str) -> Option<(Cow<'static, str>, usize)> {
    static NAMED: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    let end = s[..s.floor_char_boundary(40)].find(';')?;
    let name = s.strip_prefix('&')?.get(..end - 1)?;
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

/// Defines an offset type counted in one unit, which no other offset type mixes with.
#[macro_export]
macro_rules! position {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(usize);

        impl $name {
            /// The position `at` units from the start.
            pub const fn new(at: usize) -> $name {
                $name(at)
            }

            /// How many units from the start it lies.
            pub const fn get(self) -> usize {
                self.0
            }

            /// How far apart two positions are, whichever comes first.
            pub const fn distance(self, other: $name) -> usize {
                self.0.abs_diff(other.0)
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

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

        impl std::ops::SubAssign<usize> for $name {
            fn sub_assign(&mut self, units: usize) {
                self.0 -= units;
            }
        }

        /// The distance between two positions.
        impl std::ops::Sub for $name {
            type Output = usize;
            fn sub(self, earlier: $name) -> usize {
                self.0 - earlier.0
            }
        }
    };
}

position!(
    /// A byte offset in the source text, which is how the model counts. `TextAt` slices a text by it.
    Byte
);

position!(
    /// A position in the text of a source view, in UTF-16 units as AppKit counts them.
    U16
);

/// `range` in the source text, from plain numbers.
pub const fn bytes(range: Range<usize>) -> Range<Byte> {
    Byte(range.start)..Byte(range.end)
}

/// `range` as plain numbers, for what takes nothing else.
pub const fn offsets(range: &Range<Byte>) -> Range<usize> {
    range.start.0..range.end.0
}

/// Slicing a source text by source positions, which panics at a position outside the text or inside a character;
/// [`TextAt::fit`] brings one from outside into it.
pub trait TextAt {
    /// The part of the text in `range`.
    fn at(&self, range: Range<Byte>) -> &str;
    /// The text from `start` to its end.
    fn from(&self, start: Byte) -> &str;
    /// The text from its start up to `end`.
    fn upto(&self, end: Byte) -> &str;
    /// Whether a character starts or the text ends there.
    fn boundary(&self, at: Byte) -> bool;
    /// The position behind the last byte.
    fn end(&self) -> Byte;
    /// `range` brought into the text: each end at most at its end and moved back to the start of the character it falls
    /// into, and a range that ends in front of its start empty at its end. What reads the text by a range from outside,
    /// which may be stale, takes it through this first.
    fn fit(&self, range: Range<Byte>) -> Range<Byte> {
        let (start, end) = (self.fit_at(range.start), self.fit_at(range.end));
        start.min(end)..end
    }
    /// `at` brought into the text as [`TextAt::fit`] brings the ends of a range.
    fn fit_at(&self, at: Byte) -> Byte;
}

impl TextAt for str {
    fn fit_at(&self, at: Byte) -> Byte {
        Byte(self.floor_char_boundary(at.0))
    }

    fn at(&self, range: Range<Byte>) -> &str {
        &self[range.start.0..range.end.0]
    }

    fn from(&self, start: Byte) -> &str {
        &self[start.0..]
    }

    fn upto(&self, end: Byte) -> &str {
        &self[..end.0]
    }

    fn boundary(&self, at: Byte) -> bool {
        self.is_char_boundary(at.0)
    }

    fn end(&self) -> Byte {
        Byte(self.len())
    }
}

/// The end of the line that holds `at`, before its line ending.
pub fn line_end(text: &str, at: Byte) -> Byte {
    text.from(at).find(['\n', '\r']).map_or(text.end(), |i| at + i)
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

/// Where the line holding `at` starts.
pub fn line_start(text: &str, at: Byte) -> Byte {
    Byte(text.upto(at).rfind(['\n', '\r']).map_or(0, |i| i + 1))
}

/// Which way through a text or a table something goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// Towards the end.
    Forward,
    /// Towards the start.
    Backward,
}

/// Where the line in front of the one starting at `line` starts, if there is one.
pub(crate) fn previous_line_start(text: &str, line: Byte) -> Option<Byte> {
    let before = text.upto(line);
    let end = before
        .strip_suffix("\r\n")
        .or_else(|| before.strip_suffix(['\n', '\r']))?;
    Some(Byte(end.rfind(['\n', '\r']).map_or(0, |i| i + 1)))
}

/// Where the line behind the one holding `at` starts, if a line ending follows.
pub fn next_line_start(text: &str, at: Byte) -> Option<Byte> {
    let end = line_end(text, at);
    let rest = text.from(end);
    if rest.starts_with("\r\n") {
        Some(end + 2)
    } else {
        rest.starts_with(['\n', '\r']).then(|| end + 1)
    }
}

/// What makes two labels of links or footnotes the same, by the Unicode case folding comrak compares them with: `SS` is `ß`.
pub fn label_key(label: &str) -> String {
    caseless::default_case_fold_str(&label.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// How many spaces and tabs `s` starts with.
pub fn indent_len(s: &str) -> usize {
    s.len() - s.trim_start_matches([' ', '\t']).len()
}

/// Whether `s` holds nothing but what stands in front of a line's text in a quote.
pub fn is_quote_prefix(s: &str) -> bool {
    s.bytes().all(|b| matches!(b, b' ' | b'\t' | b'>'))
}

/// A part of a text and where it starts in it. Positions of blocks count from the start of the whole text, while what stands
/// at them is read from the part.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextPart<'a> {
    pub text: &'a str,
    pub start: Byte,
}

impl<'a> TextPart<'a> {
    pub(crate) fn whole(text: &'a str) -> Self {
        TextPart {
            text,
            start: Byte::default(),
        }
    }

    /// The position in the part of what stands at `at` in the whole text; `None` for what lies in front of the part.
    pub(crate) fn in_part(self, at: Byte) -> Option<Byte> {
        at.get().checked_sub(self.start.get()).map(Byte::new)
    }
}

/// How far `s` is indented while what follows can still open a block: up to three spaces, as a fourth or a tab makes code.
pub(crate) fn block_indent(s: &str) -> Option<usize> {
    let spaces = s.len() - s.trim_start_matches(' ').len();
    (spaces < 4 && !s[spaces..].starts_with('\t')).then_some(spaces)
}

/// Where the next quote mark of `line` stands, looked for from `from`.
pub(crate) fn next_quote_mark(line: &str, from: usize) -> Option<usize> {
    let rest = &line[from..];
    let indent = block_indent(rest)?;
    rest[indent..].starts_with('>').then_some(from + indent)
}

/// Where a line's quote marks end, behind its last one; 0 without one.
pub(crate) fn behind_quote_marks(line: &str) -> usize {
    line_marks(line)
        .take_while(|mark| mark.kind == ContainerMark::Quote)
        .last()
        .map_or(0, |mark| mark.range.end)
}

/// A mark that opens a container at the start of what is left of a source line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerMark {
    /// `>`, which opens a block quote.
    Quote,
    /// `-`, `*` or `+`, which open a bullet list item.
    Bullet,
    /// Digits followed by `.` or `)`, which open an ordered list item.
    Number,
    /// `[ ]`, `[x]` or `[X]` behind a list marker, which makes the item a task.
    Task,
}

/// A mark at the start of a source line that opens a container, as [`line_marks`] finds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LineMark {
    /// What the mark opens.
    pub kind: ContainerMark,
    /// Where the mark stands in the line, without the indentation in front of it and the space behind it.
    pub range: Range<usize>,
}

/// Behind `end`, where a mark ends in `line`, and the one space or tab that belongs to it.
pub(crate) fn with_its_space(line: &str, end: usize) -> usize {
    end + usize::from(line[end..].starts_with([' ', '\t']))
}

/// The marks that open containers at the start of `line`, in order, as CommonMark reads them: a quote mark behind at
/// most three spaces, a list marker behind any indentation, of at most nine digits and followed by a space, a tab or
/// the end of the line, and a task's checkbox right behind a list marker, after which only text follows.
pub(crate) fn line_marks(line: &str) -> impl Iterator<Item = LineMark> + '_ {
    let mut from = Some(0);
    let mut behind_list_marker = false;
    std::iter::from_fn(move || {
        let at = from?;
        if let Some(quote) = next_quote_mark(line, at) {
            from = Some(with_its_space(line, quote + 1));
            behind_list_marker = false;
            return Some(LineMark {
                kind: ContainerMark::Quote,
                range: quote..quote + 1,
            });
        }
        let start = at + indent_len(&line[at..]);
        let rest = &line[start..];
        if behind_list_marker && let Some(task) = ["[ ]", "[x]", "[X]"].iter().find(|task| rest.starts_with(**task)) {
            from = None;
            return Some(LineMark {
                kind: ContainerMark::Task,
                range: start..start + task.len(),
            });
        }
        let ends = |len: usize| {
            rest[len..]
                .chars()
                .next()
                .is_none_or(|c| matches!(c, ' ' | '\t' | '\n' | '\r'))
        };
        let Some((kind, len)) = container_mark(rest).filter(|&(kind, len)| match kind {
            ContainerMark::Bullet => ends(len),
            ContainerMark::Number => len <= 10 && ends(len),
            ContainerMark::Quote | ContainerMark::Task => false,
        }) else {
            from = None;
            return None;
        };
        from = Some(with_its_space(line, start + len));
        behind_list_marker = true;
        Some(LineMark {
            kind,
            range: start..start + len,
        })
    })
}

/// The mark `s` starts with and how long it is. Whether it opens a container also depends on what stands behind it, which
/// each caller judges for itself.
pub fn container_mark(s: &str) -> Option<(ContainerMark, usize)> {
    match s.as_bytes().first()? {
        b'>' => Some((ContainerMark::Quote, 1)),
        b'-' | b'*' | b'+' => Some((ContainerMark::Bullet, 1)),
        _ => {
            let digits = s.bytes().take_while(u8::is_ascii_digit).count();
            (digits > 0 && matches!(s.as_bytes().get(digits), Some(b'.' | b')')))
                .then_some((ContainerMark::Number, digits + 1))
        }
    }
}

/// The changed part of a text, as ranges in the old and the new text.
pub fn changed(old: &str, new: &str) -> (Range<Byte>, Range<Byte>) {
    let mut prefix = common_prefix(old.as_bytes(), new.as_bytes());
    while !(old.is_char_boundary(prefix) && new.is_char_boundary(prefix)) {
        prefix -= 1;
    }
    let max_suffix = old.len().min(new.len()) - prefix;
    let mut suffix = common_suffix(old.as_bytes(), new.as_bytes(), max_suffix);
    while !(old.is_char_boundary(old.len() - suffix) && new.is_char_boundary(new.len() - suffix)) {
        suffix -= 1;
    }
    (
        Byte::new(prefix)..old.end() - suffix,
        Byte::new(prefix)..new.end() - suffix,
    )
}

/// The prefix of the lines after the first: quote markers repeat, a list marker becomes indentation, or every row
/// would start a list item of its own.
pub fn continuation_of(prefix: &str) -> String {
    let mut out = String::new();
    let mut at = 0;
    for mark in line_marks(prefix) {
        out.push_str(&prefix[at..mark.range.start]);
        match mark.kind {
            ContainerMark::Quote => out.push('>'),
            // A checkbox stands in the first line only; what follows stays in line with its text.
            ContainerMark::Bullet | ContainerMark::Number | ContainerMark::Task => {
                out.extend(std::iter::repeat_n(' ', mark.range.len()));
            }
        }
        at = mark.range.end;
    }
    let rest = &prefix[at..];
    let indent = indent_len(rest);
    out.push_str(&rest[..indent]);
    // So does a footnote's label.
    out.extend(rest[indent..].chars().map(|_| ' '));
    out
}

/// A position moved along with an edit before it. An edit cannot move text before the start of the document.
pub fn shifted(at: Byte, delta: isize) -> Byte {
    Byte(at.0.saturating_add_signed(delta))
}

/// A count where 32 bits are plenty: a text stays far below their reach, and a count at the limit does no harm.
pub fn count32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// How much later `new` lies than `old`.
pub(crate) fn growth(new: Byte, old: Byte) -> isize {
    signed(new.0) - signed(old.0)
}

/// A count of bytes as a length change: a text stays far below the limit, and a count at the limit does no harm.
pub fn signed(n: usize) -> isize {
    isize::try_from(n).unwrap_or(isize::MAX)
}

/// The length of a range of positions.
pub fn units<T: Copy + std::ops::Sub<Output = usize>>(range: &Range<T>) -> usize {
    range.end - range.start
}

/// Byte offsets of line starts, used to resolve comrak's 1-based line/byte-column positions.
pub(crate) struct LineStarts(Vec<Byte>);

impl LineStarts {
    pub(crate) fn new(text: &str) -> Self {
        let mut starts = vec![Byte(0)];
        if text.contains('\r') {
            // comrak ends a line at `\r` too, as files from old Macs do.
            let bytes = text.as_bytes();
            starts.extend(bytes.iter().enumerate().filter_map(|(i, &b)| {
                (b == b'\n' || (b == b'\r' && bytes.get(i + 1) != Some(&b'\n'))).then_some(Byte(i + 1))
            }));
        } else {
            starts.extend(text.match_indices('\n').map(|(i, _)| Byte(i + 1)));
        }
        LineStarts(starts)
    }

    /// Only the lines of `span` and the start of the next, for those who ask by `line_of` and `line_range`.
    pub(crate) fn around(text: &str, span: &Range<Byte>) -> Self {
        let first = line_start(text, span.start.min(text.end()));
        let last = next_line_start(text, span.end.min(text.end())).unwrap_or(text.end());
        let part = LineStarts::new(text.at(first..last));
        LineStarts(part.0.into_iter().map(|at| first + at.get()).collect())
    }

    fn line_start(&self, line: usize) -> Byte {
        let idx = line.saturating_sub(1).min(self.0.len() - 1);
        self.0[idx]
    }

    /// Start offset of a position whose column is 1-based.
    pub(crate) fn start(&self, line: usize, column: usize) -> Byte {
        self.line_start(line) + column.saturating_sub(1)
    }

    /// Exclusive end offset of a position whose column points at the last byte (inclusive).
    pub(crate) fn end(&self, line: usize, column: usize) -> Byte {
        self.line_start(line) + column
    }

    pub(crate) fn line_of(&self, offset: Byte) -> usize {
        match self.0.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        }
    }

    pub(crate) fn line_range(&self, text: &str, line: usize) -> Range<Byte> {
        let start = self.0[line];
        let end = self.0.get(line + 1).copied().unwrap_or(text.end());
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
    /// The map of the offsets in `text`.
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

    /// The length of the whole text in UTF-16 units.
    pub fn len_u16(&self) -> usize {
        match self.ascii_len {
            Some(len) => len,
            None => *self.byte_to_u16.last().unwrap() as usize,
        }
    }

    /// The UTF-16 position of `byte`; a byte inside a character gives the character's start, one beyond the text its
    /// end.
    pub fn to_u16(&self, byte: Byte) -> U16 {
        U16(match self.ascii_len {
            Some(len) => byte.0.min(len),
            None => self.byte_to_u16[byte.0.min(self.byte_to_u16.len() - 1)] as usize,
        })
    }

    /// Byte offset of the char boundary at or after the given position.
    pub fn to_byte(&self, position: U16) -> Byte {
        if let Some(len) = self.ascii_len {
            return Byte(position.0.min(len));
        }
        let target = count32(position.0);
        let idx = self.byte_to_u16.partition_point(|&v| v < target);
        Byte(idx.min(self.byte_to_u16.len() - 1))
    }
}

/// UTF-16 length of a string slice.
pub fn utf16_len(s: &str) -> usize {
    // Every byte but a continuation byte starts a char; four-byte chars take a surrogate pair. No slice holds more bytes
    // than a `usize` counts, and additions checked for overflow, as in `Iterator::count`, would keep the loops from
    // vectorising.
    let b = s.as_bytes();
    let (mut chars, mut pairs) = (0usize, 0usize);
    if b.len() < VECTORISING_PAYS_FROM {
        for &x in b {
            chars = chars.wrapping_add(usize::from(x & 0xC0 != 0x80));
            pairs = pairs.wrapping_add(usize::from(x >= 0xF0));
        }
    } else {
        for &x in b {
            chars = chars.wrapping_add(usize::from(x & 0xC0 != 0x80));
        }
        for &x in b {
            pairs = pairs.wrapping_add(usize::from(x >= 0xF0));
        }
    }
    chars + pairs
}

/// Two counting passes vectorise, which most of the many short runs of a layout pay for and a long text gains from.
const VECTORISING_PAYS_FROM: usize = 64;

/// Whether `text` reads right to left: its first letter is Hebrew, Arabic or another right-to-left script, which is
/// how the Unicode bidirectional algorithm picks the direction of a paragraph. `None` for a text without letters.
pub fn right_to_left(text: &str) -> Option<bool> {
    text.chars()
        .find(|c| c.is_alphabetic())
        .map(|c| matches!(c, '\u{0590}'..='\u{08FF}' | '\u{FB1D}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}'))
}

/// Length of the common byte prefix of two texts.
pub(crate) fn common_prefix(a: &[u8], b: &[u8]) -> usize {
    const CHUNK: usize = 4096;
    let n = a.len().min(b.len());
    let mut i = 0;
    while i + CHUNK <= n && a[i..i + CHUNK] == b[i..i + CHUNK] {
        i += CHUNK;
    }
    i + a[i..n].iter().zip(&b[i..n]).take_while(|(x, y)| x == y).count()
}

/// Length of the common byte suffix of two texts, at most `max`.
pub(crate) fn common_suffix(a: &[u8], b: &[u8], max: usize) -> usize {
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

/// The place reached after `units` UTF-16 code units from `from`, at a char boundary.
pub fn byte_at_u16(text: &str, from: Byte, units: usize) -> Byte {
    advance_u16(text, from, units).0
}

/// Like `byte_at_u16`, but `None` when `units` ends inside a surrogate pair or beyond the text.
pub fn byte_at_u16_exact(text: &str, from: Byte, units: usize) -> Option<Byte> {
    let (byte, reached) = advance_u16(text, from, units);
    (reached == units).then_some(byte)
}

fn advance_u16(text: &str, from: Byte, units: usize) -> (Byte, usize) {
    const CHUNK: usize = 4096;
    let mut at = from.0;
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
            return (Byte(at + i), count);
        }
        count += c.len_utf16();
    }
    (text.end(), count)
}

/// Converts increasing positions of a text to UTF-16 positions in one forward pass, without a table.
pub struct Utf16Cursor<'a> {
    text: &'a str,
    byte: Byte,
    u16: U16,
}

impl<'a> Utf16Cursor<'a> {
    /// A cursor at the start of `text`.
    pub fn new(text: &'a str) -> Self {
        Utf16Cursor {
            text,
            byte: Byte(0),
            u16: U16(0),
        }
    }

    /// The position of `byte`, or of the character `byte` falls into; fastest for increasing positions.
    pub fn at(&mut self, byte: Byte) -> U16 {
        let byte = self.text.fit_at(byte);
        if byte < self.byte {
            self.byte = Byte(0);
            self.u16 = U16(0);
        }
        self.u16 += utf16_len(self.text.at(self.byte..byte));
        self.byte = byte;
        self.u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_next_line_starts_behind_any_line_end() {
        let at = |text: &str, from: usize| next_line_start(text, Byte::new(from)).map(Byte::get);
        assert_eq!(at("a\nb", 0), Some(2));
        assert_eq!(at("a\r\nb", 0), Some(3));
        assert_eq!(at("a\rb", 0), Some(2));
        assert_eq!(at("ab", 0), None);
        assert_eq!(at("a\n", 1), Some(2));
    }

    #[test]
    fn labels_compare_by_case_folding_and_with_their_spaces_collapsed() {
        assert_eq!(label_key("  Foo \t\n BAR "), "foo bar");
        assert_eq!(label_key("ß"), label_key("SS"));
        assert_ne!(label_key("a b"), label_key("ab"));
    }

    #[test]
    fn a_block_opens_behind_up_to_three_spaces() {
        assert_eq!(block_indent("a"), Some(0));
        assert_eq!(block_indent("   a"), Some(3));
        assert_eq!(block_indent("    a"), None);
        assert_eq!(block_indent(" \ta"), None);
        assert_eq!(next_quote_mark("  > a", 0), Some(2));
        assert_eq!(next_quote_mark("    > a", 0), None);
        assert_eq!(next_quote_mark("\t> a", 0), None);
        assert_eq!(next_quote_mark("> > a", 2), Some(2));
    }

    #[test]
    fn the_marks_of_a_quote_end_behind_the_last_of_them() {
        assert_eq!(behind_quote_marks("\t> a"), 0, "code, no quote");
        assert_eq!(behind_quote_marks(">\t> a"), 3);
        assert_eq!(behind_quote_marks("> > a"), 3);
        assert_eq!(behind_quote_marks("  >a"), 3);
        assert_eq!(behind_quote_marks("   a"), 0);
        assert_eq!(behind_quote_marks(">  "), 1);
        assert!(is_quote_prefix(" >\t"));
        assert!(!is_quote_prefix("> a"));
        assert_eq!(indent_len(" \ta"), 2);
    }

    #[test]
    fn the_marks_of_a_line_are_read_as_commonmark_reads_them() {
        use ContainerMark::{Bullet, Number, Quote, Task};
        for (line, wanted, why) in [
            ("a", vec![], "text"),
            ("> a", vec![(Quote, 0..1)], ""),
            ("   > a", vec![(Quote, 3..4)], "three spaces"),
            ("    > a", vec![], "four spaces make code"),
            ("\t> a", vec![], "so does a tab"),
            (
                ">\t> a",
                vec![(Quote, 0..1), (Quote, 2..3)],
                "a tab belongs to the mark",
            ),
            (">> a", vec![(Quote, 0..1), (Quote, 1..2)], "without a space"),
            ("- a", vec![(Bullet, 0..1)], ""),
            ("    * a", vec![(Bullet, 4..5)], "an item nested in another"),
            ("+\ta", vec![(Bullet, 0..1)], ""),
            ("-a", vec![], "no space behind"),
            ("-", vec![(Bullet, 0..1)], "an empty item"),
            ("-\r\nb", vec![(Bullet, 0..1)], "an empty item before more lines"),
            ("1. a", vec![(Number, 0..2)], ""),
            ("123456789) a", vec![(Number, 0..10)], "nine digits"),
            ("1234567890. a", vec![], "ten digits"),
            ("1.a", vec![], "no space behind"),
            ("2024 [x] a", vec![], "a year"),
            ("[x] a", vec![], "a checkbox only behind a list marker"),
            ("> - [x] a", vec![(Quote, 0..1), (Bullet, 2..3), (Task, 4..7)], ""),
            ("- [X]a", vec![(Bullet, 0..1), (Task, 2..5)], ""),
            (
                "- [ ] - a",
                vec![(Bullet, 0..1), (Task, 2..5)],
                "only text behind a checkbox",
            ),
            ("- > 1. a", vec![(Bullet, 0..1), (Quote, 2..3), (Number, 4..6)], ""),
            ("-     > a", vec![(Bullet, 0..1)], "code inside the item"),
        ] {
            let found: Vec<(ContainerMark, Range<usize>)> = line_marks(line).map(|m| (m.kind, m.range)).collect();
            assert_eq!(found, wanted, "{line:?}: {why}");
        }
    }

    #[test]
    fn a_continuation_line_repeats_quotes_and_keeps_in_line_with_the_text() {
        assert_eq!(continuation_of("-\t"), " \t", "a tab stays a tab");
        assert_eq!(continuation_of("- > "), "  > ");
        assert_eq!(continuation_of("    - > "), "      > ");
        assert_eq!(continuation_of("1234567890. "), "            ");
    }

    #[test]
    fn a_mark_is_told_from_what_only_starts_like_one() {
        assert_eq!(container_mark("> a"), Some((ContainerMark::Quote, 1)));
        assert_eq!(container_mark("- a"), Some((ContainerMark::Bullet, 1)));
        assert_eq!(container_mark("12) a"), Some((ContainerMark::Number, 3)));
        assert_eq!(container_mark("12 a"), None);
        assert_eq!(container_mark(". a"), None);
        assert_eq!(container_mark(""), None);
    }

    #[test]
    fn a_text_counts_the_same_in_utf16_at_any_length() {
        for unit in ["a", "ä", "€", "😀", "aä€😀"] {
            for count in [0, 1, 7, 15, 16, 17, 31, 63, 64, 65, 200, 1000] {
                let text = unit.repeat(count);
                assert_eq!(utf16_len(&text), text.encode_utf16().count(), "{count} × {unit}");
            }
        }
    }

    #[test]
    fn slicing_by_positions_is_slicing_by_their_numbers() {
        let text = "aä😀\nb";
        for start in 0..=text.len() {
            let at = Byte::new(start);
            assert_eq!(text.boundary(at), text.is_char_boundary(start), "{start}");
            if !text.is_char_boundary(start) {
                continue;
            }
            assert_eq!(text.from(at), &text[start..]);
            assert_eq!(text.upto(at), &text[..start]);
            for end in (start..=text.len()).filter(|&end| text.is_char_boundary(end)) {
                assert_eq!(text.at(bytes(start..end)), &text[start..end]);
            }
        }
        assert_eq!(text.end(), Byte::new(text.len()));
        assert_eq!(offsets(&bytes(1..3)), 1..3);
    }

    #[test]
    fn positions_count_like_the_numbers_they_hold() {
        let mut at = Byte::new(4);
        at += 3;
        at -= 2;
        assert_eq!(at + 1, Byte::new(6));
        assert_eq!(at - 1, Byte::new(4));
        assert_eq!(at - Byte::new(2), 3);
        assert_eq!((at.distance(Byte::new(9)), Byte::new(9).distance(at)), (4, 4));
        assert_eq!(Byte::default().get(), 0);
        assert_eq!(format!("{:?}", bytes(1..2)), "1..2", "a failed test reads plainly");
    }

    #[test]
    fn a_line_starts_behind_the_line_break_before_it() {
        assert_eq!(line_start("eins\nzwei", Byte::new(0)), Byte::new(0));
        assert_eq!(line_start("eins\nzwei", Byte::new(4)), Byte::new(0));
        assert_eq!(line_start("eins\nzwei", Byte::new(5)), Byte::new(5));
        assert_eq!(line_start("eins\nzwei", Byte::new(8)), Byte::new(5));
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
        assert_eq!(cursor.at(Byte::new(2)), U16::new(1));
        assert_eq!(cursor.at(Byte::new(5)), U16::new(2));
        assert_eq!(cursor.at(Byte::new(99)), U16::new(5));
    }

    #[test]
    fn utf16_roundtrip_with_umlauts_and_emoji() {
        let s = "aä😀b";
        let m = Utf16Map::new(s);
        assert_eq!(m.len_u16(), 5);
        assert_eq!(m.to_u16(Byte::new(0)), U16::new(0));
        assert_eq!(m.to_u16(Byte::new(1)), U16::new(1));
        assert_eq!(m.to_u16(Byte::new(3)), U16::new(2));
        assert_eq!(m.to_u16(Byte::new(7)), U16::new(4));
        assert_eq!(m.to_byte(U16::new(2)), Byte::new(3));
        assert_eq!(m.to_byte(U16::new(4)), Byte::new(7));
        assert_eq!(m.to_byte(U16::new(3)), Byte::new(7));
        assert_eq!(
            (
                byte_at_u16(s, Byte::new(0), 2),
                byte_at_u16(s, Byte::new(1), 1),
                byte_at_u16(s, Byte::new(0), 9)
            ),
            (Byte::new(3), Byte::new(3), Byte::new(8))
        );
        let long = "ä😀".repeat(3000);
        assert_eq!(byte_at_u16(&long, Byte::new(0), 2999 * 3), Byte::new(2999 * 6));
        assert_eq!(
            byte_at_u16_exact(&long, Byte::new(0), 2999 * 3 + 1),
            Some(Byte::new(2999 * 6 + 2))
        );
        assert_eq!(byte_at_u16_exact(&long, Byte::new(0), 2999 * 3 + 2), None);
        assert_eq!(
            byte_at_u16_exact(&long, Byte::new(0), 3000 * 3),
            Some(Byte::new(long.len()))
        );
        assert_eq!(byte_at_u16_exact(&long, Byte::new(0), 3000 * 3 + 1), None);
        assert_eq!((utf16_len("aä😀"), utf16_len("")), (4, 0));
        let long_a = "x".repeat(9000) + "abc" + &"y".repeat(9000);
        let long_b = "x".repeat(9000) + "aXc" + &"y".repeat(9000);
        assert_eq!(common_prefix(long_a.as_bytes(), long_b.as_bytes()), 9001);
        assert_eq!(common_suffix(long_a.as_bytes(), long_b.as_bytes(), usize::MAX), 9001);
        assert_eq!(common_suffix(b"abc", b"abc", 2), 2);
        let mut c = Utf16Cursor::new(s);
        assert_eq!(
            (c.at(Byte::new(3)), c.at(Byte::new(7)), c.at(Byte::new(1))),
            (U16::new(2), U16::new(4), U16::new(1))
        );
        let a = Utf16Map::new("abc");
        assert_eq!(
            (a.len_u16(), a.to_u16(Byte::new(2)), a.to_byte(U16::new(9))),
            (3, U16::new(2), Byte::new(3))
        );
    }

    #[test]
    fn line_starts_resolve_inclusive_ends() {
        let s = "ab\nÜx\n";
        let l = LineStarts::new(s);
        assert_eq!(l.start(2, 1), Byte::new(3));
        assert_eq!(l.end(2, 3), Byte::new(6));
        assert_eq!(s.at(l.start(2, 1)..l.end(2, 3)), "Üx");
        assert_eq!(l.end(3, 0), Byte::new(7));
        assert_eq!(l.line_of(Byte::new(4)), 1);
    }

    #[test]
    fn a_character_reference_may_have_as_many_digits_as_its_base_allows() {
        assert_eq!(decode_entity("amp;"), None, "not a reference without its ampersand");
        assert_eq!(decode_entity("&#x01F600;"), Some(("😀".into(), 10)));
        assert_eq!(decode_entity("&#128512;"), Some(("😀".into(), 9)));
        assert_eq!(decode_entity("&#0128512;"), Some(("😀".into(), 10)));
        assert_eq!(decode_entity("&#x001F600;"), None);
        assert_eq!(decode_entity("&#00128512;"), None);
    }

    #[test]
    fn a_line_beyond_the_end_starts_where_the_last_one_does() {
        let lines = LineStarts::new("a\nb");
        assert_eq!(lines.line_start(2), Byte::new(2));
        assert_eq!(lines.line_start(99), Byte::new(2));
    }

    #[test]
    fn long_texts_count_utf16_like_walking_them_letter_by_letter() {
        let text = "aé😀".repeat(3000);
        let walked = |units: usize| {
            let mut count = 0;
            for (i, c) in text.char_indices() {
                if count >= units {
                    return (Byte::new(i), count);
                }
                count += c.len_utf16();
            }
            (text.end(), count)
        };
        for units in [0, 1, 3, 2000, 4096, 5000, 11_999, 12_000, 20_000] {
            assert_eq!(advance_u16(&text, Byte::new(0), units), walked(units), "{units}");
        }
    }

    #[test]
    fn the_length_of_a_range_is_the_distance_of_its_ends() {
        assert_eq!(units(&(2_usize..5)), 3);
    }

    #[test]
    fn a_position_beyond_a_text_with_umlauts_lands_at_its_end() {
        let map = Utf16Map::new("aé😀");
        assert_eq!(map.to_u16(Byte::new(100)), U16::new(4));
        assert_eq!(map.to_byte(U16::new(100)), Byte::new(7));
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
