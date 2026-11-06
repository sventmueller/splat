//! The metadata block at the very start of a document, read as a form shows it and changed as a person would change it:
//! what stays the same stays byte for byte. YAML is read line by line and only as far as its top-level keys; whatever
//! else stands there is shown and left alone, so this needs no YAML library.

use std::collections::{HashMap, VecDeque};
use std::ops::Range;

use crate::doc::{BlockKind, Doc};
use crate::edit::Edit;
use crate::text::{Byte, bytes, changed, indent_len, line_end, next_line_start, offsets};

/// What a key holds.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Value {
    /// A scalar as text, with its quotes and escapes taken away: `true` and `2026-11-05` are text as well.
    Text(String),
    /// A flow or block list of scalars, read like `Text`.
    List(Vec<String>),
    /// Nested maps, block scalars, anchors, anything else: shown, and left alone. It holds the source behind the key
    /// with `\n` line ends.
    Complex(String),
}

/// A top-level key and its value.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Field {
    /// With its quotes and escapes taken away.
    pub key: String,
    /// What it holds.
    pub value: Value,
    /// The source lines of the field.
    pub lines: Range<Byte>,
}

/// The metadata of a document. Comment lines, blank lines and what is no field belong to no field.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FrontMatter {
    /// In the order of the text.
    pub fields: Vec<Field>,
    /// The block's range, delimiters included.
    pub block: Range<Byte>,
}

const DELIMITER: &str = "---";
const BOM: char = '\u{FEFF}';

/// Where a block stands in a text.
struct Located {
    /// Where the opening delimiter starts.
    start: usize,
    /// The lines between the delimiters.
    content: Range<usize>,
    /// Behind the closing delimiter.
    closing_end: usize,
    /// What ends the opening delimiter.
    eol: &'static str,
}

/// The block as comrak finds it: it takes the first closing delimiter with a Windows line end before any other, and
/// only `\n` and `\r\n` end a delimiter. The tree tells that there is a block, but its range ends wrongly behind a lone
/// `\r`.
fn locate(text: &str) -> Option<Located> {
    let start = text.strip_prefix(BOM).map_or(0, |_| BOM.len_utf8());
    let behind_opening = text[start..].strip_prefix(DELIMITER)?;
    let eol = ["\r\n", "\n"].into_iter().find(|eol| behind_opening.starts_with(eol))?;
    let first = start + DELIMITER.len() + eol.len();
    let body = &text[first..];
    let line_feed = body
        .find("\n---\r\n")
        .or_else(|| body.find("\n---\n"))
        .or_else(|| body.find("\n---"))?;
    let closing = first + line_feed + 1;
    let closing_end = closing + DELIMITER.len();
    let rest = &text[closing_end..];
    (rest.is_empty() || rest.starts_with('\n') || rest.starts_with("\r\n")).then_some(Located {
        start,
        content: first..closing,
        closing_end,
        eol,
    })
}

fn located(doc: &Doc) -> Option<Located> {
    let first_block = doc.blocks.first().map(|b| &b.kind);
    (doc.is_too_large() || matches!(first_block, Some(BlockKind::FrontMatter { .. })))
        .then(|| locate(doc.text()))
        .flatten()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Style {
    Plain,
    Single,
    Double,
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Written {
    span: Range<usize>,
    style: Style,
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct ItemLine {
    line: Range<usize>,
    written: Written,
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum Shape {
    /// Nothing stands behind the colon.
    Empty {
        colon_end: usize,
    },
    Scalar(Written),
    Flow {
        span: Range<usize>,
        items: Vec<Written>,
    },
    Block {
        colon_end: usize,
        items: Vec<ItemLine>,
    },
    LeftAlone,
    /// A quote or bracket that never closes, so all that follows in the block belongs to it.
    LeftOpen,
}

struct Entry {
    key: String,
    value: Value,
    lines: Range<usize>,
    shape: Shape,
}

struct Line {
    start: usize,
    content_end: usize,
    next: usize,
}

fn line_at(text: &str, at: usize) -> Line {
    let content_end = line_end(text, Byte::new(at)).get();
    let next = next_line_start(text, Byte::new(content_end)).map_or(text.len(), Byte::get);
    Line {
        start: at,
        content_end,
        next,
    }
}

fn is_blank(s: &str) -> bool {
    s.bytes().all(|b| matches!(b, b' ' | b'\t'))
}

fn is_blank_or_comment(s: &str) -> bool {
    let rest = &s[indent_len(s)..];
    rest.is_empty() || rest.starts_with('#')
}

/// Where a comment starts in what follows a value: a `#` counts only behind a space.
fn comment_start(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    (1..bytes.len()).find(|&i| bytes[i] == b'#' && matches!(bytes[i - 1], b' ' | b'\t'))
}

fn nothing_behind(s: &str) -> bool {
    let indent = indent_len(s);
    is_blank(s) || (indent > 0 && s[indent..].starts_with('#'))
}

/// Whether a plain scalar cannot start with `s`.
fn opens_with_indicator(s: &str) -> bool {
    let followed_by_space = |rest: &[u8]| rest.first().is_none_or(|b| matches!(b, b' ' | b'\t'));
    match s.as_bytes() {
        [
            b',' | b'[' | b']' | b'{' | b'}' | b'#' | b'&' | b'*' | b'!' | b'|' | b'>' | b'\'' | b'"' | b'%' | b'@'
            | b'`',
            ..,
        ] => true,
        [b'-' | b'?' | b':', rest @ ..] => followed_by_space(rest),
        _ => false,
    }
}

/// Whether `s` reads as one plain scalar, and not as a key with a value or the like.
fn reads_as_plain(s: &str) -> bool {
    !s.is_empty() && !opens_with_indicator(s) && !s.ends_with(':') && !s.contains(": ") && !s.contains(":\t")
}

/// The text of a double-quoted scalar at the start of `s` and how long it is; `None` if it does not end on this line or
/// holds an escape YAML does not know.
fn double_quoted(s: &str) -> Option<(String, usize)> {
    let mut out = String::new();
    let mut chars = s.char_indices().skip(1);
    while let Some((at, c)) = chars.next() {
        match c {
            '"' => return Some((out, at + 1)),
            '\\' => {
                let (_, escape) = chars.next()?;
                let digits = match escape {
                    'x' => 2,
                    'u' => 4,
                    'U' => 8,
                    _ => 0,
                };
                if digits > 0 {
                    let mut code = 0;
                    for _ in 0..digits {
                        code = code * 16 + chars.next()?.1.to_digit(16)?;
                    }
                    out.push(char::from_u32(code)?);
                } else {
                    out.push(escaped(escape)?);
                }
            }
            c => out.push(c),
        }
    }
    None
}

/// What YAML reads for a backslash and this character.
fn escaped(c: char) -> Option<char> {
    Some(match c {
        '0' => '\0',
        'a' => '\u{7}',
        'b' => '\u{8}',
        't' | '\t' => '\t',
        'n' => '\n',
        'v' => '\u{B}',
        'f' => '\u{C}',
        'r' => '\r',
        'e' => '\u{1B}',
        ' ' | '"' | '/' | '\\' => c,
        'N' => '\u{85}',
        '_' => '\u{A0}',
        'L' => '\u{2028}',
        'P' => '\u{2029}',
        _ => return None,
    })
}

/// The text of a single-quoted scalar at the start of `s` and how long it is.
fn single_quoted(s: &str) -> Option<(String, usize)> {
    let mut out = String::new();
    let mut from = 1;
    loop {
        let quote = from + s[from..].find('\'')?;
        out.push_str(&s[from..quote]);
        if !s[quote + 1..].starts_with('\'') {
            return Some((out, quote + 1));
        }
        out.push('\'');
        from = quote + 2;
    }
}

/// The scalar at the start of `s`, quoted, with its style and length.
fn quoted(s: &str) -> Option<(String, usize, Style)> {
    if s.starts_with('"') {
        double_quoted(s).map(|(text, len)| (text, len, Style::Double))
    } else {
        single_quoted(s).map(|(text, len)| (text, len, Style::Single))
    }
}

/// The scalar that `text[start..line_end]` holds and nothing else, but for a comment.
fn scalar_at(text: &str, start: usize, line_end: usize) -> Option<(String, Written)> {
    let s = &text[start..line_end];
    if s.starts_with(['"', '\'']) {
        let (value, len, style) = quoted(s)?;
        return nothing_behind(&s[len..]).then_some((
            value,
            Written {
                span: start..start + len,
                style,
            },
        ));
    }
    let value = s[..comment_start(s).unwrap_or(s.len())].trim_end_matches([' ', '\t']);
    reads_as_plain(value).then(|| {
        (
            value.to_string(),
            Written {
                span: start..start + value.len(),
                style: Style::Plain,
            },
        )
    })
}

/// The items of the flow list at the start of `line`, which stands at `offset` in the text, and where the list ends in
/// the line.
fn flow_list(line: &str, offset: usize) -> Option<(Vec<String>, Vec<Written>, usize)> {
    let (mut values, mut items) = (Vec::new(), Vec::new());
    let mut at = 1;
    loop {
        at += indent_len(&line[at..]);
        let rest = &line[at..];
        if rest.starts_with(']') {
            return Some((values, items, at + 1));
        }
        if rest.starts_with(['"', '\'']) {
            let (value, len, style) = quoted(rest)?;
            values.push(value);
            items.push(Written {
                span: offset + at..offset + at + len,
                style,
            });
            at += len;
        } else {
            let value = rest[..rest.find([',', ']'])?].trim_end_matches([' ', '\t']);
            let holds_other = value.contains(['[', '{', '}']) || comment_start(value).is_some();
            if !reads_as_plain(value) || holds_other {
                return None;
            }
            items.push(Written {
                span: offset + at..offset + at + value.len(),
                style: Style::Plain,
            });
            values.push(value.to_string());
            at += value.len();
        }
        at += indent_len(&line[at..]);
        match line.as_bytes().get(at)? {
            b',' => at += 1,
            b']' => return Some((values, items, at + 1)),
            _ => return None,
        }
    }
}

/// Where a key ends at the start of `line`: the key and how far `line` goes through its colon.
fn read_key(line: &str) -> Option<(String, usize)> {
    let colon_of = |behind: usize| {
        let spaces = indent_len(&line[behind..]);
        let rest = line[behind + spaces..].strip_prefix(':')?;
        (rest.is_empty() || rest.starts_with([' ', '\t'])).then_some(behind + spaces + 1)
    };
    if line.starts_with(['"', '\'']) {
        let (key, len, _) = quoted(line)?;
        return Some((key, colon_of(len)?));
    }
    if opens_with_indicator(line) {
        return None;
    }
    let bytes = line.as_bytes();
    for (at, &b) in bytes.iter().enumerate() {
        match b {
            b':' if bytes.get(at + 1).is_none_or(|next| matches!(next, b' ' | b'\t')) => {
                return Some((line[..at].trim_end_matches([' ', '\t']).to_string(), at + 1));
            }
            b'#' if at > 0 && matches!(bytes[at - 1], b' ' | b'\t') => return None,
            _ => {}
        }
    }
    None
}

fn is_dash_item(s: &str) -> bool {
    s.strip_prefix('-')
        .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t']))
}

struct Reader<'t> {
    text: &'t str,
    end: usize,
}

impl Reader<'_> {
    /// Where the lines end that belong to a field whose key stands at `key_indent` and whose own lines end at `from`:
    /// what is indented deeper, and what is blank or a comment only between such lines. With `dashes`, items at the
    /// key's indent count, and with `comments_count`, as in a block scalar, so does what looks like a comment.
    fn field_end(&self, from: usize, key_indent: usize, dashes: bool, comments_count: bool) -> usize {
        let (mut end, mut at) = (from, from);
        while at < self.end {
            let line = line_at(self.text, at);
            let shown = &self.text[line.start..line.content_end];
            let indent = indent_len(shown);
            let deeper = indent > key_indent && (comments_count || !shown[indent..].starts_with('#'));
            if !is_blank_or_comment(shown) || (deeper && !is_blank(shown)) {
                if !(deeper || (dashes && indent == key_indent && is_dash_item(&shown[indent..]))) {
                    break;
                }
                end = line.next;
            }
            at = line.next;
        }
        end
    }

    fn block_list(&self, from: usize, end: usize) -> Option<(Vec<String>, Vec<ItemLine>)> {
        let (mut values, mut items) = (Vec::new(), Vec::new());
        let mut item_indent = None;
        let mut at = from;
        while at < end {
            let line = line_at(self.text, at);
            at = line.next;
            let shown = &self.text[line.start..line.content_end];
            if is_blank_or_comment(shown) {
                continue;
            }
            let indent = indent_len(shown);
            if *item_indent.get_or_insert(indent) != indent || !is_dash_item(&shown[indent..]) {
                return None;
            }
            let dash_end = line.start + indent + 1;
            let value_start = dash_end + indent_len(&self.text[dash_end..line.content_end]);
            let (value, written) = if is_blank_or_comment(&self.text[dash_end..line.content_end]) {
                let span = dash_end..dash_end;
                let style = Style::Plain;
                (String::new(), Written { span, style })
            } else {
                scalar_at(self.text, value_start, line.content_end)?
            };
            values.push(value);
            items.push(ItemLine {
                line: line.start..line.next,
                written,
            });
        }
        Some((values, items))
    }

    /// Where a quoted or flow value that goes on over more lines ends, if it does.
    fn multi_line_end(&self, from: usize) -> Option<usize> {
        let bytes = self.text.as_bytes();
        let (mut depth, mut quote, mut at) = (0usize, None, from);
        while at < self.end {
            let b = bytes[at];
            match quote {
                Some(b'"') if b == b'\\' => at += 1,
                Some(q) if b == q => {
                    quote = None;
                    // A value of one quoted scalar ends with it; a doubled single quote only stands for one.
                    if depth == 0 && !(q == b'\'' && bytes.get(at + 1) == Some(&b'\'')) {
                        return Some(at + 1);
                    }
                }
                Some(_) => {}
                None => match b {
                    b'"' | b'\'' => quote = Some(b),
                    b'[' | b'{' => depth += 1,
                    b']' | b'}' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            return Some(at + 1);
                        }
                    }
                    b'#' if matches!(bytes[at - 1], b' ' | b'\t' | b'\n' | b'\r') => {
                        at = line_end(self.text, Byte::new(at)).get();
                    }
                    _ => {}
                },
            }
            at += 1;
        }
        None
    }

    fn entry(&self, line: &Line, indent: usize, key: String, colon_end: usize) -> Entry {
        let text = self.text;
        let value_start = colon_end + indent_len(&text[colon_end..line.content_end]);
        let first = &text[value_start..line.content_end];
        let field = |lines_end: usize, value: Value, shape: Shape| Entry {
            key: key.clone(),
            value,
            lines: line.start..lines_end,
            shape,
        };
        let left_alone = |from: usize, end: usize| {
            let source = text[from..end].trim_end_matches(['\r', '\n']);
            let value = source.replace("\r\n", "\n").replace('\r', "\n");
            field(end, Value::Complex(value), Shape::LeftAlone)
        };
        let alone_behind = self.field_end(line.next, indent, false, false);
        match first.as_bytes().first() {
            None | Some(b'#') => {
                let end = self.field_end(line.next, indent, true, false);
                if end == line.next {
                    return field(end, Value::Text(String::new()), Shape::Empty { colon_end });
                }
                match self.block_list(line.next, end) {
                    Some((values, items)) => field(end, Value::List(values), Shape::Block { colon_end, items }),
                    None => left_alone(line.next, end),
                }
            }
            Some(b'|' | b'>') => left_alone(value_start, self.field_end(line.next, indent, false, true)),
            Some(b'[') => {
                let list = flow_list(first, value_start).filter(|(_, _, end)| nothing_behind(&first[*end..]));
                match list {
                    Some((values, items, end)) if alone_behind == line.next => {
                        let span = value_start..value_start + end;
                        field(alone_behind, Value::List(values), Shape::Flow { span, items })
                    }
                    _ => self.multi_line(line, indent, value_start, &left_alone),
                }
            }
            Some(b'{') => self.multi_line(line, indent, value_start, &left_alone),
            Some(b'"' | b'\'') => match scalar_at(text, value_start, line.content_end) {
                Some((value, written)) if alone_behind == line.next => {
                    field(alone_behind, Value::Text(value), Shape::Scalar(written))
                }
                _ => self.multi_line(line, indent, value_start, &left_alone),
            },
            Some(_) => match scalar_at(text, value_start, line.content_end) {
                Some((value, written)) if alone_behind == line.next => {
                    field(alone_behind, Value::Text(value), Shape::Scalar(written))
                }
                _ => left_alone(value_start, alone_behind),
            },
        }
    }

    /// A value left alone that starts at `value_start` and may close on a later line.
    fn multi_line(
        &self,
        line: &Line,
        indent: usize,
        value_start: usize,
        left_alone: &dyn Fn(usize, usize) -> Entry,
    ) -> Entry {
        let Some(closed) = self.multi_line_end(value_start) else {
            return Entry {
                shape: Shape::LeftOpen,
                ..left_alone(value_start, self.end)
            };
        };
        let behind = if closed >= line.content_end {
            line_at(self.text, closed - 1).next.min(self.end)
        } else {
            line.next
        };
        left_alone(value_start, self.field_end(behind, indent, false, false))
    }

    fn entries(&self, from: usize) -> Vec<Entry> {
        let mut found = Vec::new();
        let mut top = None;
        let mut at = from;
        while at < self.end {
            let line = line_at(self.text, at);
            let shown = &self.text[line.start..line.content_end];
            at = line.next;
            if is_blank_or_comment(shown) {
                continue;
            }
            let indent = indent_len(shown);
            if *top.get_or_insert(indent) != indent {
                continue;
            }
            if let Some((key, colon_end)) = read_key(&shown[indent..]) {
                let entry = self.entry(&line, indent, key, line.start + indent + colon_end);
                at = entry.lines.end;
                found.push(entry);
            }
        }
        found
    }
}

fn entries_of(text: &str, content: &Range<usize>) -> Vec<Entry> {
    Reader { text, end: content.end }.entries(content.start)
}

/// The metadata at the start of `doc`, if it has a block.
pub fn read(doc: &Doc) -> Option<FrontMatter> {
    let located = located(doc)?;
    let fields = entries_of(doc.text(), &located.content)
        .into_iter()
        .map(|entry| Field {
            key: entry.key,
            value: entry.value,
            lines: bytes(entry.lines),
        })
        .collect();
    Some(FrontMatter {
        fields,
        block: bytes(located.start..located.closing_end),
    })
}

/// Whether a character needs an escape: only double quotes can write it, and a line would not hold it.
fn needs_escape(c: char) -> bool {
    c.is_control() || matches!(c, '\u{2028}' | '\u{2029}' | '\u{FEFF}' | '\u{FFFE}' | '\u{FFFF}')
}

fn double_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if needs_escape(c) => out.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// What `edit` writes for one text value: plain if YAML reads it back as the same string, else quoted.
pub fn scalar(value: &str) -> String {
    // `true`, `1.5` and `2026-11-05` stay plain because Splat and static site generators want them typed as written.
    let plain = reads_as_plain(value)
        && !value.starts_with(['-', '?', ':'])
        && !value.starts_with([' ', '\t'])
        && !value.ends_with([' ', '\t'])
        && !value.contains(" #")
        && !value.contains(needs_escape);
    if plain { value.to_string() } else { double_quote(value) }
}

/// `scalar` for an item of a flow list, where a plain scalar ends at more characters.
fn flow_scalar(value: &str) -> String {
    let written = scalar(value);
    if written.starts_with('"') || !value.contains([',', '[', ']', '{', '}']) {
        written
    } else {
        double_quote(value)
    }
}

/// `value` in the style a scalar had, if that style can hold it.
fn in_style(style: Style, value: &str) -> String {
    match style {
        Style::Plain => scalar(value),
        Style::Single if !value.contains(needs_escape) => format!("'{}'", value.replace('\'', "''")),
        Style::Single | Style::Double => double_quote(value),
    }
}

fn flow_text(items: &[String]) -> String {
    let written: Vec<String> = items.iter().map(|item| flow_scalar(item)).collect();
    format!("[{}]", written.join(", "))
}

/// How many items two lists have in common at their start, and at their end behind that.
fn common_ends(old: &[String], new: &[String]) -> (usize, usize) {
    let front = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let back = old[front..]
        .iter()
        .rev()
        .zip(new[front..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    (front, back)
}

/// The flow list changed to `new`: what stays keeps how it is written, what takes the place of an item keeps its style.
fn changed_flow(text: &str, span: &Range<usize>, items: &[Written], old: &[String], new: &[String]) -> String {
    let (Some(first), Some(last)) = (items.first(), items.last()) else {
        return flow_text(new);
    };
    if new.is_empty() {
        return "[]".to_string();
    }
    let separator = match items {
        [a, b, ..] => &text[a.span.end..b.span.start],
        _ => ", ",
    };
    let (front, back) = common_ends(old, new);
    let written: Vec<String> = (0..new.len())
        .map(|at| {
            let stays = (at < front).then_some(at).or_else(|| {
                let from_end = new.len() - at;
                (from_end <= back).then(|| old.len() - from_end)
            });
            if let Some(old_at) = stays {
                text[items[old_at].span.clone()].to_string()
            } else if at < old.len() - back {
                let written = in_style(items[at].style, &new[at]);
                if items[at].style == Style::Plain {
                    flow_scalar(&new[at])
                } else {
                    written
                }
            } else {
                flow_scalar(&new[at])
            }
        })
        .collect();
    format!(
        "[{}{}{}]",
        &text[span.start + 1..first.span.start],
        written.join(separator),
        &text[last.span.end..span.end - 1]
    )
}

fn item_with(text: &str, item: &ItemLine, value: &str) -> String {
    let span = &item.written.span;
    let space = if span.is_empty() { " " } else { "" };
    format!(
        "{}{space}{}{}",
        &text[item.line.start..span.start],
        in_style(item.written.style, value),
        &text[span.end..item.line.end]
    )
}

/// A new line of a block list, made like the first line of the list.
fn new_item(text: &str, first: &ItemLine, value: &str, eol: &str) -> String {
    let lead = &text[first.line.start..first.written.span.start];
    let space = if lead.ends_with([' ', '\t']) { "" } else { " " };
    format!("{lead}{space}{}{eol}", scalar(value))
}

/// The lines of a block list changed to `new`, `new` not being empty; the lines between its items stay.
fn changed_block(text: &str, items: &[ItemLine], old: &[String], new: &[String], eol: &str, out: &mut String) {
    let (front, back) = common_ends(old, new);
    let replaced = old.len() - front - back;
    let wanted = &new[front..new.len() - back];
    let mut at = items.first().map_or(0, |item| item.line.start);
    for (index, item) in items.iter().enumerate() {
        if index == old.len() - back {
            for value in wanted.iter().skip(replaced) {
                out.push_str(&new_item(text, &items[0], value, eol));
            }
        }
        out.push_str(&text[at..item.line.start]);
        match index.checked_sub(front).filter(|inside| *inside < replaced) {
            None => out.push_str(&text[item.line.clone()]),
            Some(inside) => {
                if let Some(value) = wanted.get(inside) {
                    out.push_str(&item_with(text, item, value));
                }
            }
        }
        at = item.line.end;
    }
    if back == 0 {
        for value in wanted.iter().skip(replaced) {
            out.push_str(&new_item(text, &items[0], value, eol));
        }
    }
}

/// A field's lines with `written` behind its colon and without the lines of its items; the lines between those stay.
fn without_items(
    text: &str,
    lines: &Range<usize>,
    colon_end: usize,
    written: &str,
    items: &[ItemLine],
    out: &mut String,
) {
    let mut at = colon_end;
    out.push_str(&text[lines.start..colon_end]);
    out.push(' ');
    out.push_str(written);
    for item in items {
        out.push_str(&text[at..item.line.start]);
        at = item.line.end;
    }
    out.push_str(&text[at..lines.end]);
}

fn write_changed(text: &str, entry: &Entry, wanted: &Value, eol: &str, out: &mut String) {
    let lines = &entry.lines;
    let replacing = |span: &Range<usize>, written: &str, out: &mut String| {
        out.push_str(&text[lines.start..span.start]);
        out.push_str(written);
        out.push_str(&text[span.end..lines.end]);
    };
    let behind_colon = |colon_end: usize, written: &str, out: &mut String| {
        out.push_str(&text[lines.start..colon_end]);
        out.push(' ');
        out.push_str(written);
        out.push_str(&text[colon_end..lines.end]);
    };
    match (&entry.shape, wanted) {
        (Shape::LeftAlone | Shape::LeftOpen, _) | (_, Value::Complex(_)) => out.push_str(&text[lines.clone()]),
        (Shape::Empty { colon_end }, Value::Text(value)) => behind_colon(*colon_end, &scalar(value), out),
        (Shape::Empty { colon_end }, Value::List(new)) => behind_colon(*colon_end, &flow_text(new), out),
        (Shape::Scalar(written), Value::Text(value)) => {
            replacing(&written.span, &in_style(written.style, value), out);
        }
        (Shape::Scalar(written), Value::List(new)) => replacing(&written.span, &flow_text(new), out),
        (Shape::Flow { span, .. }, Value::Text(value)) => replacing(span, &scalar(value), out),
        (Shape::Flow { span, items }, Value::List(new)) => {
            let Value::List(old) = &entry.value else {
                return out.push_str(&text[lines.clone()]);
            };
            replacing(span, &changed_flow(text, span, items, old, new), out);
        }
        (Shape::Block { colon_end, items }, Value::Text(value)) => {
            without_items(text, lines, *colon_end, &scalar(value), items, out);
        }
        (Shape::Block { colon_end, items }, Value::List(new)) => {
            let Value::List(old) = &entry.value else {
                return out.push_str(&text[lines.clone()]);
            };
            if new.is_empty() {
                without_items(text, lines, *colon_end, "[]", items, out);
            } else {
                let first = items.first().map_or(lines.end, |item| item.line.start);
                let last = items.last().map_or(lines.end, |item| item.line.end);
                out.push_str(&text[lines.start..first]);
                changed_block(text, items, old, new, eol, out);
                out.push_str(&text[last..lines.end]);
            }
        }
    }
}

/// Whether a value left alone starts on the line behind its key.
fn starts_below_its_key(source: &str) -> bool {
    source.starts_with([' ', '\t']) || is_dash_item(source)
}

/// Writes a field that is not there yet. A value left alone that would close the block stays out.
fn write_new(key: &str, value: &Value, indent: &str, eol: &str, out: &mut String) {
    let behind_key = match value {
        Value::Text(text) => format!(" {}", scalar(text)),
        Value::List(items) => format!(" {}", flow_text(items)),
        Value::Complex(source) if source.is_empty() => String::new(),
        Value::Complex(source) if source.contains("\n---") => return,
        Value::Complex(source) => {
            let lines = source.replace('\n', eol);
            if starts_below_its_key(source) {
                format!("{eol}{lines}")
            } else {
                format!(" {lines}")
            }
        }
    };
    out.push_str(indent);
    out.push_str(&scalar(key));
    out.push(':');
    out.push_str(&behind_key);
    out.push_str(eol);
}

/// A new block at the very start, behind a byte order mark: the first closing delimiter with a Windows line end wins
/// wherever it stands, so a document that holds one needs that line end on this one as well.
fn new_block(text: &str, wanted: &[(String, Value)], selection: &Range<Byte>) -> Option<Edit> {
    let start = text.strip_prefix(BOM).map_or(0, |_| BOM.len_utf8());
    let first_line_end = text[start..].find('\n').map(|at| start + at);
    let crlf = first_line_end.is_some_and(|at| at > start && text.as_bytes()[at - 1] == b'\r');
    let eol = if crlf || text.contains("\n---\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut block = format!("{DELIMITER}{eol}");
    for (key, value) in wanted {
        write_new(key, value, "", eol, &mut block);
    }
    if block.len() == DELIMITER.len() + eol.len() {
        return None;
    }
    block.push_str(&format!("{DELIMITER}{eol}{eol}"));
    let at = Byte::new(start);
    let inserted = block.len();
    Some(
        Edit {
            range: at..at,
            replacement: block,
            selection: selection.clone(),
        }
        .keeping_selection_after(at, 0, inserted),
    )
}

/// The edit that takes the block away with the blank line behind it. If what follows would then read as a block, that
/// line stays, or one takes its place.
fn removal(text: &str, block: &Located, selection: &Range<Byte>) -> Edit {
    let behind = next_line_start(text, Byte::new(block.closing_end)).map_or(text.len(), Byte::get);
    let line = line_at(text, behind);
    let has_blank_line = behind < text.len() && is_blank(&text[line.start..line.content_end]);
    let end = if has_blank_line { line.next } else { behind };
    let replacement = if locate(&text[end..]).is_some() { block.eol } else { "" }.to_string();
    let at = Byte::new(block.start);
    let inserted = replacement.len();
    Edit {
        range: at..Byte::new(end),
        replacement,
        selection: selection.clone(),
    }
    .keeping_selection_after(at, end - block.start, inserted)
}

/// The edit that makes the document's metadata `wanted`, with `selection` staying on the text it stands on; None if the
/// metadata already is. What already stands in the block stays in its order, so the order of `wanted` only tells the new
/// fields apart, and these go behind the last field. A value left alone never changes, whatever `wanted` says, nor does
/// a field become one; both can only go away.
pub fn edit(doc: &Doc, wanted: &[(String, Value)], selection: &Range<Byte>) -> Option<Edit> {
    let selection = &doc.fit(selection.clone());
    let text = doc.text();
    let Some(block) = located(doc) else {
        return if doc.is_too_large() || wanted.is_empty() {
            None
        } else {
            new_block(text, wanted, selection)
        };
    };
    let entries = entries_of(text, &block.content);
    if wanted.is_empty() && entries.is_empty() {
        return None;
    }
    let content = &text[block.content.clone()];
    let mut gaps_hold_something = false;
    let mut at = block.content.start;
    for entry in &entries {
        gaps_hold_something |= !text[at..entry.lines.start].trim().is_empty();
        at = entry.lines.end;
    }
    gaps_hold_something |= !text[at..block.content.end].trim().is_empty();
    if wanted.is_empty() && !gaps_hold_something {
        return Some(removal(text, &block, selection));
    }
    let new_text = rewritten(text, &block, &entries, wanted);
    if new_text == content {
        return None;
    }
    let (old_change, new_change) = changed(content, &new_text);
    let start = Byte::new(block.content.start) + old_change.start.get();
    let range = start..Byte::new(block.content.start) + old_change.end.get();
    let replacement = new_text[offsets(&new_change)].to_string();
    let (removed, inserted) = (range.end - range.start, replacement.len());
    Some(
        Edit {
            range,
            replacement,
            selection: selection.clone(),
        }
        .keeping_selection_after(start, removed, inserted),
    )
}

fn rewritten(text: &str, block: &Located, entries: &[Entry], wanted: &[(String, Value)]) -> String {
    let mut same: HashMap<(&str, &Value), VecDeque<usize>> = HashMap::new();
    let mut by_key: HashMap<&str, VecDeque<usize>> = HashMap::new();
    for (at, entry) in entries.iter().enumerate() {
        same.entry((&entry.key, &entry.value)).or_default().push_back(at);
        by_key.entry(&entry.key).or_default().push_back(at);
    }
    // Of a key that stands twice, what stays as it is keeps its lines, and the others change in turn.
    let mut asked: Vec<Option<&Value>> = vec![None; entries.len()];
    let others: Vec<&(String, Value)> = wanted
        .iter()
        .filter(
            |pair| match same.get_mut(&(pair.0.as_str(), &pair.1)).and_then(VecDeque::pop_front) {
                Some(at) => {
                    asked[at] = Some(&pair.1);
                    false
                }
                None => true,
            },
        )
        .collect();
    let mut new_fields: Vec<(&String, &Value)> = Vec::new();
    for pair in others {
        let queue = by_key.get_mut(pair.0.as_str());
        let free = queue.and_then(|queue| {
            while let Some(at) = queue.pop_front() {
                if asked[at].is_none() {
                    return Some(at);
                }
            }
            None
        });
        match free {
            Some(at) => asked[at] = Some(&pair.1),
            None => new_fields.push((&pair.0, &pair.1)),
        }
    }
    let indent = entries.first().map_or("", |entry| {
        let line = &text[entry.lines.clone()];
        &line[..indent_len(line)]
    });
    let mut out = String::with_capacity(block.content.len());
    let mut at = block.content.start;
    for (entry, value) in entries.iter().zip(&asked) {
        out.push_str(&text[at..entry.lines.start]);
        // Behind a value that never closes, a new field would become part of it.
        if entry.shape == Shape::LeftOpen {
            for (key, value) in new_fields.drain(..) {
                write_new(key, value, indent, block.eol, &mut out);
            }
        }
        match value {
            Some(value) if **value == entry.value => out.push_str(&text[entry.lines.clone()]),
            Some(value) => write_changed(text, entry, value, block.eol, &mut out),
            None => {}
        }
        at = entry.lines.end;
    }
    if entries.is_empty() {
        out.push_str(&text[at..block.content.end]);
        at = block.content.end;
    }
    for (key, value) in new_fields.drain(..) {
        write_new(key, value, indent, block.eol, &mut out);
    }
    out.push_str(&text[at..block.content.end]);
    // Only behind a line feed does the closing delimiter still close, and a removal can leave a lone `\r` in front of
    // it.
    if !out.ends_with('\n') {
        out.push_str(block.eol);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::parse;
    use crate::text::TextAt;

    fn fields(text: &str) -> Vec<(String, Value)> {
        read(&parse(text))
            .map(|block| block.fields.into_iter().map(|f| (f.key, f.value)).collect())
            .unwrap_or_default()
    }

    fn one(text: &str) -> Value {
        let found = fields(&format!("---\n{text}\n---\n"));
        assert_eq!(found.len(), 1, "{text:?}: {found:?}");
        found[0].1.clone()
    }

    fn text_value(value: &str) -> Value {
        Value::Text(value.to_string())
    }

    fn list(items: &[&str]) -> Value {
        Value::List(items.iter().map(ToString::to_string).collect())
    }

    fn pair(key: &str, value: Value) -> (String, Value) {
        (key.to_string(), value)
    }

    fn edited(text: &str, wanted: &[(String, Value)]) -> String {
        let edit = edit(&parse(text), wanted, &bytes(0..0)).expect("a change");
        edit.applied_to(text)
    }

    /// `text` with the value of its one field changed.
    fn changing(text: &str, value: Value) -> String {
        let key = fields(text).remove(0).0;
        edited(text, &[(key, value)])
    }

    #[test]
    fn plain_scalars_are_read_without_their_comment() {
        for (line, value) in [
            ("a: b", "b"),
            ("a:   b c  ", "b c"),
            ("a: b # c", "b"),
            ("a: b\t# c", "b"),
            ("a: b#c", "b#c"),
            ("a: http://x.y/z", "http://x.y/z"),
            ("a: 2026-11-05", "2026-11-05"),
            ("a: 1.5", "1.5"),
            ("a: true", "true"),
            ("a: ~", "~"),
            ("a: -5", "-5"),
            ("a: ?x", "?x"),
            ("a: :x", ":x"),
            ("a: Grüße 日本", "Grüße 日本"),
            ("a: say \"hi\"", "say \"hi\""),
            ("a:", ""),
            ("a:   ", ""),
            ("a: # c", ""),
        ] {
            assert_eq!(one(line), text_value(value), "{line:?}");
        }
    }

    #[test]
    fn quoted_scalars_are_read_with_their_escapes() {
        for (line, value) in [
            ("a: \"b c\"", "b c"),
            ("a: \"b # c\" # d", "b # c"),
            ("a: 'it''s'", "it's"),
            ("a: 'b # c' # d", "b # c"),
            ("a: ''", ""),
            ("a: \"\"", ""),
            (
                r#"a: "x\ty\nz\"\\\/\ \e\a\b\v\f\r\0\_\N\L\P""#,
                "x\ty\nz\"\\/ \u{1B}\u{7}\u{8}\u{B}\u{C}\r\0\u{A0}\u{85}\u{2028}\u{2029}",
            ),
            (r#"a: "\x41\u00E4\U0001F600""#, "Aä😀"),
        ] {
            assert_eq!(one(line), text_value(value), "{line:?}");
        }
    }

    #[test]
    fn keys_may_be_quoted_and_hold_what_a_value_may() {
        for (line, key) in [
            ("a b: c", "a b"),
            ("a:b: c", "a:b"),
            ("a : c", "a"),
            ("\"a: b\": c", "a: b"),
            ("'it''s': c", "it's"),
            ("\"a\" : c", "a"),
            ("Größe: c", "Größe"),
            ("<<: c", "<<"),
            ("-a: c", "-a"),
        ] {
            assert_eq!(fields(&format!("---\n{line}\n---\n"))[0].0, key, "{line:?}");
        }
    }

    #[test]
    fn what_is_no_key_belongs_to_nobody() {
        for content in [
            "just text",
            "- a",
            "? a",
            "[a]: b",
            "#a: b",
            "a #b: c",
            "\"a\"b: c",
            "\"a\":b",
            "\"a: c",
            ": c",
            "   ",
        ] {
            assert_eq!(fields(&format!("---\n{content}\n---\n")), [], "{content:?}");
        }
        let found = fields("---\na: 1\n# comment\n\nstray\nb: 2\n---\n");
        assert_eq!(found, [pair("a", text_value("1")), pair("b", text_value("2"))]);
    }

    #[test]
    fn lists_are_read_in_either_style() {
        for (line, items) in [
            ("a: [b, c]", &["b", "c"][..]),
            ("a: [ b , \"c, d\" ,'e' ]", &["b", "c, d", "e"]),
            ("a: [b, c,]", &["b", "c"]),
            ("a: []", &[]),
            ("a: [ ]", &[]),
            ("a: [b] # c", &["b"]),
            ("a:\n  - b\n  - \"c, d\"", &["b", "c, d"]),
            ("a:\n- b\n- 'c'", &["b", "c"]),
            ("a:\n    - b # d\n    - c", &["b", "c"]),
            ("a:\n  - b\n\n  # comment\n  - c", &["b", "c"]),
            ("a: # comment\n  - b", &["b"]),
            ("a:\n  -\n  - c", &["", "c"]),
            ("a:\n  - # comment\n  - c", &["", "c"]),
            ("a:\n  -\tb", &["b"]),
        ] {
            assert_eq!(one(line), list(items), "{line:?}");
        }
    }

    #[test]
    fn everything_else_is_left_alone_and_shown() {
        for (lines, shown) in [
            ("a: |\n  b\n\n  # c\n  d", "|\n  b\n\n  # c\n  d"),
            ("a: >-\n  b", ">-\n  b"),
            ("a: &x b", "&x b"),
            ("a: *x", "*x"),
            ("a: !t b", "!t b"),
            ("a: {b: c}", "{b: c}"),
            ("a: {b: [c,\n  d]}", "{b: [c,\n  d]}"),
            ("a: [b, [c]]", "[b, [c]]"),
            ("a: [b, {c: d}]", "[b, {c: d}]"),
            ("a: [b: c]", "[b: c]"),
            ("a: [b,, c]", "[b,, c]"),
            ("a: [b] c", "[b] c"),
            ("a: [b c # d", "[b c # d"),
            ("a: [b\n  c]", "[b\n  c]"),
            ("a: [b, \"c\" d]", "[b, \"c\" d]"),
            ("a: \"b\" c", "\"b\" c"),
            ("a: \"\\q\"", "\"\\q\""),
            ("a: \"\\uD800\"", "\"\\uD800\""),
            ("a: \"b\n  c\"", "\"b\n  c\""),
            ("a: 'b\nc'", "'b\nc'"),
            ("a: b: c", "b: c"),
            ("a: b:", "b:"),
            ("a: b\n  c", "b\n  c"),
            ("a: - b", "- b"),
            ("a: ? b", "? b"),
            ("a: %b", "%b"),
            ("a: @b", "@b"),
            ("a: `b", "`b"),
            ("a: ,b", ",b"),
            ("a:\n  b: c\n  d: e", "b: c\n  d: e"),
            ("a:\n  - b: c", "- b: c"),
            ("a:\n  - - b", "- - b"),
            ("a:\n  - b\n    c", "- b\n    c"),
            ("a:\n    - b\n  - c", "- b\n  - c"),
            ("a:\n  - [b]", "- [b]"),
        ] {
            let value = one(lines);
            let Value::Complex(found) = &value else {
                panic!("{lines:?}: {value:?}");
            };
            assert_eq!(found.trim_start(), shown, "{lines:?}");
        }
    }

    #[test]
    fn a_field_takes_only_its_own_lines() {
        let text = "---\n# a\na: 1\n\nb:\n  - x\n  # inner\n  - y\n  # behind\n\nc: |\n  d\n  # kept\n\ne:\n  f: 1\n  # inner\n  g: 2\n  # behind\n# h\ni: 2\n---\n";
        let block = read(&parse(text)).unwrap();
        let lines: Vec<&str> = block.fields.iter().map(|f| text.at(f.lines.clone())).collect();
        assert_eq!(
            lines,
            [
                "a: 1\n",
                "b:\n  - x\n  # inner\n  - y\n",
                "c: |\n  d\n  # kept\n",
                "e:\n  f: 1\n  # inner\n  g: 2\n",
                "i: 2\n"
            ]
        );
        assert_eq!(text.at(block.block), text.trim_end());
    }

    #[test]
    fn a_block_at_the_start_is_found_as_the_parser_finds_it() {
        for (text, found) in [
            ("---\na: 1\n---\n", true),
            ("---\na: 1\n---", true),
            ("---\r\na: 1\r\n---\r\n", true),
            ("\u{FEFF}---\na: 1\n---\n", true),
            ("---\n\n---\n", true),
            ("---\n---\n", false),
            ("--- \na: 1\n---\n", false),
            ("---\na: 1\n--- \n", false),
            ("---\na: 1\n---x\n", false),
            ("---\ra: 1\r---\r", false),
            (" ---\na: 1\n---\n", false),
            ("x\n---\na: 1\n---\n", false),
            ("---\na: 1\n", false),
            ("---\na: 1\n---x\nb\n---\n", true),
            ("---\na: 1\n---\nb\n---\r\n", true),
            ("---\na: 1\rb: 2\n---\n", true),
        ] {
            let doc = parse(text);
            let parsed = matches!(doc.blocks.first().map(|b| &b.kind), Some(BlockKind::FrontMatter { .. }));
            assert_eq!(parsed, found, "the parser on {text:?}");
            assert_eq!(read(&doc).is_some(), found, "{text:?}");
        }
    }

    #[test]
    fn a_closing_delimiter_with_a_windows_line_end_wins_as_it_does_in_the_parser() {
        let text = "---\na: 1\n---\nb\n---\r\nc\n";
        let block = read(&parse(text)).unwrap();
        assert_eq!(text.at(block.block.clone()), "---\na: 1\n---\nb\n---");
        assert_eq!(block.fields.len(), 1);
    }

    #[test]
    fn a_text_too_large_to_parse_shows_its_block_all_the_same() {
        let text = "---\na: 1\n---\n";
        let doc = Doc::too_large(text.into());
        assert_eq!(read(&doc).unwrap().fields[0].key, "a");
        assert!(edit(&doc, &[pair("a", text_value("2"))], &bytes(0..0)).is_some());
        let plain = Doc::too_large("text\n".into());
        assert_eq!(
            edit(&plain, &[pair("a", text_value("2"))], &bytes(0..0)),
            None,
            "it holds a block we cannot see"
        );
    }

    #[test]
    fn indented_keys_keep_their_indent() {
        let text = "---\n  a: 1\n  b: 2\n---\n";
        assert_eq!(fields(text).len(), 2);
        assert_eq!(
            edited(
                text,
                &[
                    pair("a", text_value("1")),
                    pair("b", text_value("2")),
                    pair("c", text_value("3"))
                ]
            ),
            "---\n  a: 1\n  b: 2\n  c: 3\n---\n"
        );
        assert_eq!(
            fields("---\n  a: 1\nb: 2\n---\n").len(),
            1,
            "a line further out is no key"
        );
    }

    #[test]
    fn what_needs_no_change_gives_no_edit() {
        let text = "---\n# c\na:   'b' # d\nc: [ x,y ]\nd:\n  - e\nf: |\n  g\n---\n";
        let wanted: Vec<_> = fields(text);
        assert_eq!(edit(&parse(text), &wanted, &bytes(0..0)), None);
        assert_eq!(edit(&parse("text\n"), &[], &bytes(0..0)), None);
        assert_eq!(
            edit(&parse("---\n\n---\n"), &[], &bytes(0..0)),
            None,
            "nothing to take away"
        );
        let reversed: Vec<_> = wanted.iter().rev().cloned().collect();
        assert_eq!(edit(&parse(text), &reversed, &bytes(0..0)), None, "the order stays");
    }

    #[test]
    fn a_changed_scalar_keeps_its_style_and_its_comment() {
        for (old, value, new) in [
            ("a: b", "c", "a: c"),
            ("a:    b   # d", "c", "a:    c   # d"),
            ("a: 'b' # d", "it's", "a: 'it''s' # d"),
            ("a: \"b\"  # d", "c\"d", "a: \"c\\\"d\"  # d"),
            ("a: b # d", "x: y", "a: \"x: y\" # d"),
            ("a: b", "", "a: \"\""),
            ("a: 'b'", "c\nd", "a: \"c\\nd\""),
            ("a: 'b'", "", "a: ''"),
            ("a:", "c", "a: c"),
            ("a:    # d", "c", "a: c    # d"),
            ("\"a\":   b", "c", "\"a\":   c"),
        ] {
            assert_eq!(
                changing(&format!("---\n{old}\n---\n"), text_value(value)),
                format!("---\n{new}\n---\n"),
                "{old:?}"
            );
        }
    }

    #[test]
    fn a_changed_list_stays_in_its_style() {
        for (old, items, new) in [
            ("a: [b, c]", &["b", "d"][..], "a: [b, d]"),
            ("a: [b,c]", &["b", "c", "d"], "a: [b,c,d]"),
            (
                "a: [ b, \"c, d\" ] # e",
                &["x", "c, d", "y"],
                "a: [ x, \"c, d\", y ] # e",
            ),
            ("a: ['b', c]", &["d", "c"], "a: ['d', c]"),
            ("a: [\"b\"]", &["c, d", "e, f"], "a: [\"c, d\", \"e, f\"]"),
            ("a: [b]", &["c, d"], "a: [\"c, d\"]"),
            ("a: [b, c]", &[], "a: []"),
            ("a: []", &["b", "true"], "a: [b, true]"),
            ("a: [b, c,]", &["b", "d"], "a: [b, d,]"),
            ("a:\n  - b\n  - c", &["b", "d"], "a:\n  - b\n  - d"),
            ("a:\n- b\n- c", &["b", "c", "d"], "a:\n- b\n- c\n- d"),
            (
                "a:\n  -  b # x\n  - c",
                &["d", "b", "c"],
                "a:\n  -  d\n  -  b # x\n  - c",
            ),
            (
                "a:\n  - 'b'\n  - \"c\" # x",
                &["'b'", "d"],
                "a:\n  - '''b'''\n  - \"d\" # x",
            ),
            ("a:\n  - b\n  # z\n  - c\n  - d", &["b", "d"], "a:\n  - b\n  # z\n  - d"),
            (
                "a:\n  - b\n  # z\n  - c",
                &["x", "y", "c"],
                "a:\n  - x\n  - y\n  # z\n  - c",
            ),
            ("a:\n  - b\n  - c", &["x: y"], "a:\n  - \"x: y\""),
            ("a:\n  -\n  - c", &["b", "c"], "a:\n  - b\n  - c"),
            ("a: # e\n  - b\n  # f\n  - c", &[], "a: [] # e\n  # f"),
        ] {
            assert_eq!(
                changing(&format!("---\n{old}\n---\n"), list(items)),
                format!("---\n{new}\n---\n"),
                "{old:?}"
            );
        }
    }

    #[test]
    fn a_new_block_item_is_made_like_the_first_item() {
        assert_eq!(
            changing("---\na:\n  -\n---\n", list(&["", "b"])),
            "---\na:\n  -\n  - b\n---\n"
        );
    }

    #[test]
    fn a_value_can_change_its_kind() {
        for (old, value, new) in [
            ("a: b # c", list(&["d", "e"]), "a: [d, e] # c"),
            ("a:", list(&["d"]), "a: [d]"),
            ("a: [b] # c", text_value("d"), "a: d # c"),
            ("a:\n  - b\n  # c\n  - d", text_value("e"), "a: e\n  # c"),
        ] {
            assert_eq!(
                changing(&format!("---\n{old}\n---\n"), value),
                format!("---\n{new}\n---\n"),
                "{old:?}"
            );
        }
    }

    #[test]
    fn a_value_left_alone_only_goes_away() {
        let text = "---\na: |\n  b\nc: {d: e}\nf: 1\n---\n";
        let kept = [
            pair("a", text_value("x")),
            pair("c", list(&["y"])),
            pair("f", text_value("1")),
        ];
        assert_eq!(edit(&parse(text), &kept, &bytes(0..0)), None);
        assert_eq!(
            edited(text, &[pair("c", text_value("x")), pair("f", text_value("1"))]),
            "---\nc: {d: e}\nf: 1\n---\n"
        );
        assert_eq!(
            edit(
                &parse("---\na: 1\n---\n"),
                &[pair("a", Value::Complex("{b: c}".into()))],
                &bytes(0..0)
            ),
            None,
            "a field does not become one"
        );
    }

    #[test]
    fn a_new_field_goes_behind_the_last_field() {
        let wanted = [pair("a", text_value("1")), pair("b", list(&["c", "d, e"]))];
        assert_eq!(
            edited("---\na: 1\n# behind\n\n---\nx\n", &wanted),
            "---\na: 1\nb: [c, \"d, e\"]\n# behind\n\n---\nx\n"
        );
        assert_eq!(
            edited("---\n# only\n---\n", &[pair("a", text_value("true"))]),
            "---\n# only\na: true\n---\n"
        );
        assert_eq!(
            edited("---\n\n---\n", &[pair("a", text_value("1"))]),
            "---\n\na: 1\n---\n"
        );
        assert_eq!(
            edited(
                "---\na: 1\n---\n",
                &[pair("b", text_value("2")), pair("c", text_value("3"))]
            ),
            "---\nb: 2\nc: 3\n---\n",
            "behind where the last one stood"
        );
        assert_eq!(
            edited(
                "---\na: 1\n---\n",
                &[pair("a", text_value("1")), pair("b: c", text_value(""))]
            ),
            "---\na: 1\n\"b: c\": \"\"\n---\n"
        );
    }

    #[test]
    fn a_new_field_stays_out_of_a_value_that_never_closes() {
        for open in ["a: 'b\nc", "a: [b,\n  c"] {
            let text = format!("---\n{open}\n---\n");
            let found = fields(&text);
            assert_eq!(found.len(), 1, "{open:?}");
            let mut wanted = found.clone();
            wanted.push(pair("d", text_value("1")));
            let new = edited(&text, &wanted);
            assert_eq!(new, format!("---\nd: 1\n{open}\n---\n"));
            assert_eq!(fields(&new).len(), 2);
        }
    }

    #[test]
    fn a_new_field_left_alone_is_written_as_it_was_read() {
        for (value, written) in [
            ("{b: c}", "a: {b: c}"),
            ("|\n  b\n  c", "a: |\n  b\n  c"),
            ("  b: c\n  d: e", "a:\n  b: c\n  d: e"),
            ("- b: c", "a:\n- b: c"),
            ("", "a:"),
        ] {
            let wanted = [pair("a", Value::Complex(value.into()))];
            let new = edited("x\n", &wanted);
            assert_eq!(new, format!("---\n{written}\n---\n\nx\n"), "{value:?}");
        }
        let wanted = [
            pair("a", Value::Complex("  b\n---\nc".into())),
            pair("d", text_value("1")),
        ];
        assert_eq!(
            edited("x\n", &wanted),
            "---\nd: 1\n---\n\nx\n",
            "it would close the block"
        );
        assert_eq!(
            edit(
                &parse("x\n"),
                &[pair("a", Value::Complex("\n---".into()))],
                &bytes(0..0)
            ),
            None,
            "no block for nothing"
        );
    }

    #[test]
    fn a_removed_field_takes_its_lines_along() {
        let text = "---\n# one\na: 1\n\nb:\n  - x\n  # inner\n  - y\n# two\nc: 3\n---\n";
        assert_eq!(
            edited(text, &[pair("a", text_value("1")), pair("c", text_value("3"))]),
            "---\n# one\na: 1\n\n# two\nc: 3\n---\n"
        );
        assert_eq!(
            edited(text, &[]),
            "---\n# one\n\n# two\n---\n",
            "the comments keep the block"
        );
    }

    #[test]
    fn a_removal_leaves_the_closing_delimiter_behind_a_line_feed() {
        let new = edited("---\na: 1\rb: 2\n---\n", &[pair("a", text_value("1"))]);
        assert_eq!(new, "---\na: 1\r\n---\n");
        assert_eq!(fields(&new), [pair("a", text_value("1"))]);
    }

    #[test]
    fn duplicate_keys_are_matched_in_turn() {
        let text = "---\na: 1\na: 2\n---\n";
        assert_eq!(
            edited(text, &[pair("a", text_value("x")), pair("a", text_value("y"))]),
            "---\na: x\na: y\n---\n"
        );
        assert_eq!(edited(text, &[pair("a", text_value("x"))]), "---\na: x\n---\n");
        assert_eq!(
            edited(text, &[pair("a", text_value("2"))]),
            "---\na: 2\n---\n",
            "the one that stays"
        );
        let change = edit(
            &parse(text),
            &[pair("a", text_value("2")), pair("a", text_value("x"))],
            &bytes(0..0),
        )
        .unwrap();
        assert_eq!(change.applied_to(text), "---\na: x\na: 2\n---\n");
    }

    #[test]
    fn a_document_without_a_block_gets_one_at_its_very_start() {
        let wanted = [pair("key", text_value("value"))];
        assert_eq!(edited("Text\n", &wanted), "---\nkey: value\n---\n\nText\n");
        assert_eq!(edited("", &wanted), "---\nkey: value\n---\n\n");
        assert_eq!(
            edited("Text\r\nmehr\r\n", &wanted),
            "---\r\nkey: value\r\n---\r\n\r\nText\r\nmehr\r\n"
        );
        assert_eq!(
            edited("\u{FEFF}Text\n", &wanted),
            "\u{FEFF}---\nkey: value\n---\n\nText\n"
        );
        assert_eq!(edit(&parse("Text\n"), &[], &bytes(0..0)), None);
        assert_eq!(fields(&edited("---\nkein Block\n", &wanted)), wanted);
    }

    #[test]
    fn a_delimiter_with_a_windows_line_end_further_down_does_not_take_the_new_block_with_it() {
        let text = "a\n\n---\r\nb\n";
        let new = edited(text, &[pair("c", text_value("1"))]);
        assert_eq!(new, "---\r\nc: 1\r\n---\r\n\r\na\n\n---\r\nb\n");
        assert_eq!(read(&parse(&new)).unwrap().block, bytes(0..14));
    }

    #[test]
    fn nothing_wanted_takes_a_block_without_comments_away_with_the_blank_line_behind_it() {
        for (text, left) in [
            ("---\na: 1\n---\n\nText\n", "Text\n"),
            ("---\na: 1\n---\nText\n", "Text\n"),
            ("---\na: 1\n---\n\n\nText\n", "\nText\n"),
            ("---\na: 1\n---\n", ""),
            ("---\na: 1\n---", ""),
            ("---\r\na: 1\r\n---\r\n\r\nText\r\n", "Text\r\n"),
            ("\u{FEFF}---\na: 1\n---\n\nText\n", "\u{FEFF}Text\n"),
            ("---\n\na: |\n  b\n\n---\n  \nText\n", "Text\n"),
        ] {
            assert_eq!(edited(text, &[]), left, "{text:?}");
        }
    }

    #[test]
    fn what_follows_a_removed_block_does_not_become_one() {
        for (text, left) in [
            ("---\na: 1\n---\n\n---\nb: 2\n---\n", "\n---\nb: 2\n---\n"),
            ("---\na: 1\n---\n---\nb: 2\n---\n", "\n---\nb: 2\n---\n"),
        ] {
            let new = edited(text, &[]);
            assert_eq!(new, left);
            assert_eq!(read(&parse(&new)), None);
        }
    }

    #[test]
    fn windows_documents_stay_windows_documents() {
        let text = "---\r\na: 1\r\nb:\r\n  - x\r\nc: [y]\r\n# d\r\n---\r\n\r\nText\r\n";
        let wanted = [
            pair("a", text_value("2")),
            pair("b", list(&["x", "z"])),
            pair("c", list(&["y", "w"])),
            pair("e", text_value("3")),
        ];
        assert_eq!(
            edited(text, &wanted),
            "---\r\na: 2\r\nb:\r\n  - x\r\n  - z\r\nc: [y, w]\r\ne: 3\r\n# d\r\n---\r\n\r\nText\r\n"
        );
    }

    #[test]
    fn the_caret_stays_on_the_text_it_stood_on() {
        let caret =
            |text: &str, wanted: &[(String, Value)], at: usize| edit(&parse(text), wanted, &bytes(at..at)).unwrap();
        let stands_on = |text: &str, change: &Edit, what: &str| {
            assert!(
                change.applied_to(text)[change.selection.start.get()..].starts_with(what),
                "{change:?}"
            );
        };
        let text = "---\na: 123\n---\n\nGrüße\n";
        let at = text.find("üße").unwrap();
        stands_on(text, &caret(text, &[pair("a", text_value("1234567"))], at), "üße");
        stands_on(text, &caret(text, &[pair("a", text_value("1"))], at), "üße");
        stands_on(text, &caret(text, &[], at), "üße");
        stands_on("Grüße\n", &caret("Grüße\n", &[pair("a", text_value("1"))], 2), "üße");
        assert_eq!(
            caret(text, &[pair("a", text_value("9"))], 1).selection,
            bytes(1..1),
            "before the change"
        );
        assert_eq!(
            caret(text, &[pair("a", text_value("456"))], 8).selection,
            bytes(10..10),
            "inside the change, behind what takes its place"
        );
        let in_a_letter = caret("Grüße\n", &[pair("a", text_value("1"))], 3);
        stands_on("Grüße\n", &in_a_letter, "üße");
        let wide = edit(&parse("x\n"), &[pair("a", text_value("1"))], &bytes(9..99)).unwrap();
        assert_eq!(wide.selection.end.get(), wide.applied_to("x\n").len());
        let backwards = Byte::new(2)..Byte::new(1);
        let reversed = edit(&parse("xy\n"), &[pair("a", text_value("1"))], &backwards).unwrap();
        assert!(reversed.selection.start <= reversed.selection.end);
    }

    #[test]
    fn an_edit_changes_no_more_than_it_has_to() {
        let text = "---\na: 123\nb: 456\n---\n";
        let change = edit(
            &parse(text),
            &[pair("a", text_value("123")), pair("b", text_value("457"))],
            &bytes(0..0),
        )
        .unwrap();
        assert_eq!(text.at(change.range), "6");
        assert_eq!(change.replacement, "7");
    }

    #[test]
    fn a_scalar_is_plain_only_if_it_reads_back_as_the_same_text() {
        for plain in [
            "a",
            "a b",
            "2026-11-05",
            "1.5",
            "true",
            "no",
            "null",
            "~",
            "a:b",
            "a#b",
            "http://a.de/b?c=d",
            "Grüße",
            "日本語",
            "a, b",
            "a [b]",
            "a'b",
            "a\"b",
            "a\\b",
            "...",
            "=",
            "<<",
        ] {
            assert_eq!(scalar(plain), plain, "{plain:?}");
        }
        for (value, written) in [
            ("", "\"\""),
            (" a", "\" a\""),
            ("a ", "\"a \""),
            ("\ta", "\"\\ta\""),
            ("a: b", "\"a: b\""),
            ("a:", "\"a:\""),
            ("a #b", "\"a #b\""),
            ("a\nb", "\"a\\nb\""),
            ("a\r\nb", "\"a\\r\\nb\""),
            ("a\tb", "\"a\\tb\""),
            ("a\"\\", "\"a\"\\\\\""),
            ("-a", "\"-a\""),
            ("?a", "\"?a\""),
            (":a", "\":a\""),
            (",a", "\",a\""),
            ("[a", "\"[a\""),
            ("]a", "\"]a\""),
            ("{a", "\"{a\""),
            ("}a", "\"}a\""),
            ("#a", "\"#a\""),
            ("&a", "\"&a\""),
            ("*a", "\"*a\""),
            ("!a", "\"!a\""),
            ("|a", "\"|a\""),
            (">a", "\">a\""),
            ("'a", "\"'a\""),
            ("\"a", "\"\\\"a\""),
            ("%a", "\"%a\""),
            ("@a", "\"@a\""),
            ("`a", "\"`a\""),
            ("a\u{0}b", "\"a\\u0000b\""),
            ("a\u{85}b", "\"a\\u0085b\""),
            ("a\u{2028}b", "\"a\\u2028b\""),
            ("\u{FEFF}a", "\"\\uFEFFa\""),
        ] {
            let found = scalar(value);
            assert!(found == written || value == "a\"\\", "{value:?}: {found}");
            assert_eq!(one(&format!("x: {found}")), text_value(value), "{value:?}");
        }
    }

    #[test]
    fn every_way_of_writing_a_value_reads_back_as_that_value() {
        let values = [
            "a",
            "a b",
            "",
            " ",
            "true",
            "~",
            "a: b",
            "a #b",
            "#a",
            "- a",
            "-",
            "--",
            "---",
            "...",
            "[a]",
            "{a}",
            "a,b",
            "a]b",
            "a[b",
            "it's",
            "\"q\"",
            "\\",
            "a\nb",
            "\r",
            "\t",
            "Grüße",
            "日本語",
            "\u{0}",
            "😀",
            "a:",
            ":",
            "?",
            "%",
            "\u{FFFF}",
        ];
        for value in values {
            for style in [Style::Plain, Style::Single, Style::Double] {
                let written = in_style(style, value);
                assert_eq!(one(&format!("x: {written}")), text_value(value), "{style:?} {written}");
                assert_eq!(
                    one(&format!("x:\n  - {written}")),
                    Value::List(vec![value.to_string()]),
                    "{style:?} {written}"
                );
            }
            let flow = flow_text(&[value.to_string(), "x".to_string()]);
            assert_eq!(
                one(&format!("x: {flow}")),
                Value::List(vec![value.to_string(), "x".to_string()]),
                "{flow}"
            );
            let key = scalar(value);
            assert_eq!(fields(&format!("---\n{key}: x\n---\n"))[0].0, value, "{key}");
        }
    }

    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            usize::try_from(self.0 % n.max(1) as u64).unwrap_or(0)
        }

        fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
            items[self.below(items.len())]
        }

        fn chance(&mut self, percent: usize) -> bool {
            self.below(100) < percent
        }
    }

    const VALUES: [&str; 30] = [
        "a",
        "b c",
        "",
        "true",
        "1.5",
        "2026-11-05",
        "~",
        "x: y",
        "x #y",
        "#x",
        "- x",
        "-",
        "---",
        "[x]",
        "{x}",
        "x,y",
        "x]y",
        "it's",
        "\"q\"",
        "\\",
        "x\ny",
        "\r\n",
        "\t",
        "Grüße",
        "日本語",
        "😀",
        "x:",
        ":",
        "\u{0}",
        "  ",
    ];

    fn random_list(rng: &mut Rng) -> Value {
        let items = (0..rng.below(4)).map(|_| rng.pick(&VALUES).to_string()).collect();
        Value::List(items)
    }

    fn random_value(rng: &mut Rng) -> Value {
        if rng.chance(30) {
            random_list(rng)
        } else {
            text_value(rng.pick(&VALUES))
        }
    }

    /// A field as a person may have written it: its lines without line ends.
    fn random_field(rng: &mut Rng, key: &str) -> Vec<String> {
        let value = rng.pick(&VALUES);
        let plain = scalar(value);
        let comment = if rng.chance(30) { "  # comment" } else { "" };
        let spacing = rng.pick(&[" ", "   ", "\t"]);
        let items = || -> Vec<String> { VALUES[..4].iter().map(|v| in_style(Style::Double, v)).collect() };
        match rng.below(8) {
            0 | 1 => vec![format!("{key}:{spacing}{plain}{comment}")],
            2 => vec![format!("{key}:{spacing}{}{comment}", in_style(Style::Single, value))],
            3 => vec![format!("{key}:{spacing}{}{comment}", double_quote(value))],
            4 => vec![format!("{key}:{comment}")],
            5 => vec![format!("{key}: [ a,{} ]{comment}", items().join(","))],
            6 => {
                let mut lines = vec![format!("{key}:")];
                let indent = rng.pick(&["", "  ", "    "]);
                for item in items() {
                    lines.push(format!("{indent}- {item}"));
                    if rng.chance(20) {
                        lines.push(format!("{indent}# between"));
                    }
                }
                lines
            }
            _ => match rng.below(3) {
                0 => vec![
                    format!("{key}: |"),
                    "  block".to_string(),
                    String::new(),
                    "  # text".to_string(),
                ],
                1 => vec![format!("{key}:"), "  nested: 1".to_string(), "  more: 2".to_string()],
                _ => vec![format!("{key}: {{a: 1}}")],
            },
        }
    }

    fn lines_of(text: &str, block: &FrontMatter, key: &str) -> Option<String> {
        block
            .fields
            .iter()
            .find(|f| f.key == key)
            .map(|f| text.at(f.lines.clone()).to_string())
    }

    /// A document with a random block, and the line end it uses.
    fn random_document(rng: &mut Rng) -> (String, &'static str) {
        const KEYS: [&str; 7] = ["title", "tags", "Größe", "\"a: b\"", "date", "x y", "'it''s'"];
        let eol = rng.pick(&["\n", "\r\n"]);
        let mut lines = vec![];
        for key in KEYS {
            if rng.chance(50) {
                continue;
            }
            if rng.chance(20) {
                lines.push(rng.pick(&["# comment", "", "   ", "  # indented"]).to_string());
            }
            lines.extend(random_field(rng, key));
        }
        if rng.chance(20) {
            lines.push("# last".to_string());
        }
        let block = lines.iter().map(|l| format!("{l}{eol}")).collect::<String>();
        let body = rng.pick(&["", "Text\n", "\n\nText\n", "---\nb: 1\n---\n", "---\n"]);
        if lines.is_empty() {
            (body.to_string(), eol)
        } else {
            (format!("---{eol}{block}---{eol}{body}"), eol)
        }
    }

    /// What to ask of the fields `old`, and the keys of those it changes or removes.
    fn random_wanted(rng: &mut Rng, old: &[Field]) -> (Vec<(String, Value)>, Vec<String>) {
        let mut wanted = Vec::new();
        let mut touched = Vec::new();
        for field in old {
            match rng.below(5) {
                0 => touched.push(field.key.clone()),
                1 => {
                    wanted.push((field.key.clone(), random_value(rng)));
                    touched.push(field.key.clone());
                }
                _ => wanted.push((field.key.clone(), field.value.clone())),
            }
        }
        for at in 0..rng.below(3) {
            wanted.push((format!("{} {at}", rng.pick(&VALUES)), random_value(rng)));
        }
        (wanted, touched)
    }

    /// What `wanted` must read back as: a value left alone stays, and the fields of the document keep their order ahead
    /// of the new ones. What a removed field leaves behind may read as part of a value left alone before it, so those
    /// compare by their kind.
    fn expected_fields(old: &[Field], wanted: &[(String, Value)]) -> Vec<(String, Value)> {
        let mut expected = Vec::new();
        let mut new_fields = wanted.to_vec();
        for field in old {
            if let Some(at) = new_fields.iter().position(|w| w.0 == field.key) {
                let (key, value) = new_fields.remove(at);
                let stays = matches!(field.value, Value::Complex(_));
                expected.push((key, if stays { field.value.clone() } else { value }));
            }
        }
        expected.extend(new_fields);
        expected
    }

    fn by_kind(fields: &[(String, Value)]) -> Vec<(String, Value)> {
        let plain = |value: &Value| match value {
            Value::Complex(_) => Value::Complex(String::new()),
            other => other.clone(),
        };
        fields.iter().map(|(key, value)| (key.clone(), plain(value))).collect()
    }

    /// Whether every field outside of `touched` stands in `new` as it stood in `text`.
    fn others_stay(text: &str, new: &str, before: &FrontMatter, after: &FrontMatter, touched: &[String]) -> bool {
        before
            .fields
            .iter()
            .filter(|field| !touched.contains(&field.key))
            .all(|field| {
                let lines = lines_of(new, after, &field.key).unwrap_or_default();
                let unchanged = text.at(field.lines.clone());
                if matches!(field.value, Value::Complex(_)) {
                    lines.starts_with(unchanged)
                } else {
                    lines == unchanged
                }
            })
    }

    #[test]
    fn any_edit_reads_back_as_wanted_and_leaves_every_other_line_alone() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for round in 0..4000 {
            let (text, eol) = random_document(&mut rng);
            let doc = parse(&text);
            let before = read(&doc);
            let old = before.as_ref().map_or(Vec::new(), |b| b.fields.clone());
            let (wanted, touched) = random_wanted(&mut rng, &old);
            let at = text.floor_char_boundary(rng.below(text.len() + 1));
            let what = || format!("round {round}: {text:?} wanted {wanted:?}");
            let change = edit(&doc, &wanted, &bytes(at..at));
            let Some(change) = change else {
                let same = old.iter().all(|f| wanted.iter().any(|w| w.0 == f.key)) && wanted.len() == old.len();
                assert!(same, "no edit for a change: {}", what());
                continue;
            };
            assert!(text.is_char_boundary(change.range.start.get()), "{}", what());
            let new = change.applied_to(&text);
            assert!(new.is_char_boundary(change.selection.start.get()), "{}", what());
            assert!(change.selection.end.get() <= new.len(), "{}", what());
            let new_doc = parse(&new);
            let after = read(&new_doc);
            let found: Vec<_> = after.as_ref().map_or(Vec::new(), |b| {
                b.fields.iter().map(|f| (f.key.clone(), f.value.clone())).collect()
            });
            let expected = expected_fields(&old, &wanted);
            assert_eq!(by_kind(&found), by_kind(&expected), "{} gave {new:?}", what());
            if let (Some(before), Some(after)) = (&before, &after) {
                assert!(
                    others_stay(&text, &new, before, after, &touched),
                    "{} gave {new:?}",
                    what()
                );
            }
            if eol == "\r\n" && text.starts_with("---\r\n") {
                let end = after.as_ref().map_or(0, |b| b.block.end.get());
                let bare = new[..end].match_indices('\n').any(|(at, _)| !new[..at].ends_with('\r'));
                assert!(!bare, "a bare line feed: {} gave {new:?}", what());
            }
            for comment in ["# comment", "# last", "  # indented"] {
                let in_a_change = old
                    .iter()
                    .any(|f| text.at(f.lines.clone()).contains(comment) && touched.contains(&f.key));
                if !in_a_change && !wanted.is_empty() {
                    assert_eq!(
                        text.matches(comment).count(),
                        new.matches(comment).count(),
                        "{comment:?}: {} gave {new:?}",
                        what()
                    );
                }
            }
            assert_eq!(
                edit(&new_doc, &expected, &bytes(0..0)),
                None,
                "not settled: {} gave {new:?}",
                what()
            );
        }
    }

    #[test]
    fn a_quoted_value_on_lines_of_its_own_is_found_to_its_end() {
        let text = "---\na: 'b''c\nd: e\n' # f\ng: \"h\\\"\ni\"\nj: [k, \"l]\", # m ]\n  n]\no: 1\n---\n";
        let found = fields(text);
        let keys: Vec<&str> = found.iter().map(|f| f.0.as_str()).collect();
        assert_eq!(keys, ["a", "g", "j", "o"], "{found:?}");
        let open = "---\na: 'b\nc: d\n---\n";
        assert_eq!(fields(open).len(), 1, "never closed");
    }

    #[test]
    fn a_field_that_closes_on_the_last_line_stays_inside_the_block() {
        let text = "---\na: [b,\n  c]\n---\nd\n";
        let block = read(&parse(text)).unwrap();
        assert_eq!(text.at(block.fields[0].lines.clone()), "a: [b,\n  c]\n");
    }
}
