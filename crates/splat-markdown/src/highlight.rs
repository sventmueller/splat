//! The colors and font traits of the source view, as byte ranges of the Markdown text.

use std::ops::Range;

use super::code::{self, CodeToken};
use super::doc::{Block, BlockKind, Doc, Inline, InlineKind, Splice, walk_blocks};
use super::text::{
    Byte, ContainerMark, LineStarts, TextAt, behind_quote_marks, changed, common_prefix, common_suffix, indent_len,
    line_marks, right_to_left, shifted, signed,
};

/// What a stretch of the source is to the colors of the source view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Token {
    /// A heading, its marks and its text.
    Heading,
    /// Markup characters: emphasis delimiters, fences, rules, pipes, brackets and the like.
    Punctuation,
    /// A code span, or the code of a block whose language has no colors of its own.
    Code,
    /// The text of a link or wiki link, or a footnote reference or label.
    LinkText,
    /// A link address, or an autolink.
    Url,
    /// A list marker or checkbox, or the `:` in front of definition details.
    ListMarker,
    /// The `>` marks of a quote, and the marker of an alert.
    Quote,
    /// Raw HTML, and the tags of HTML elements.
    Html,
    /// A formula, inline or as a block.
    Math,
    /// A keyword in code.
    Keyword,
    /// A string in code.
    String,
    /// A comment in code, a Markdown comment, front matter, or the attribute list of a heading.
    Comment,
    /// A number in code.
    Number,
}

impl From<CodeToken> for Token {
    fn from(t: CodeToken) -> Token {
        match t {
            CodeToken::Keyword => Token::Keyword,
            CodeToken::String => Token::String,
            CodeToken::Comment => Token::Comment,
            CodeToken::Number => Token::Number,
        }
    }
}

/// How a span of the source view looks: its color and its font traits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Style {
    /// What gives it its color; `None` for text in the color of plain text.
    pub token: Option<Token>,
    /// Bold, as headings, strong text and header rows are.
    pub bold: bool,
    /// Italic, as emphasis is.
    pub italic: bool,
    /// Struck through, as `~~text~~` is.
    pub strike: bool,
}

impl Style {
    fn color(token: Token) -> Self {
        Style {
            token: Some(token),
            ..Style::default()
        }
    }
    fn flags(bold: bool, italic: bool, strike: bool) -> Self {
        Style {
            token: None,
            bold,
            italic,
            strike,
        }
    }
}

/// Styled, non-overlapping byte ranges for the source view. Unstyled text is omitted.
pub fn highlight(doc: &Doc) -> Vec<(Range<Byte>, Style)> {
    highlight_blocks(doc.text(), &doc.blocks)
}

pub(super) fn highlight_blocks(text: &str, blocks: &[Block]) -> Vec<(Range<Byte>, Style)> {
    let mut spans: Vec<(Range<Byte>, Style)> = Vec::new();
    // Definitions comrak moved behind the text break the order of the blocks.
    let start = blocks.iter().map(|b| b.range.start).min().unwrap_or_default();
    let end = blocks.iter().map(|b| b.range.end).max().unwrap_or_default();
    let lines = LineStarts::around(text, &(start..end));
    walk_blocks(blocks, &mut |b| block_spans(text, &lines, b, &mut spans));
    flatten(spans)
}

/// The colors of `doc` after `splice` changed it: those of the re-parsed blocks are computed anew, the others taken
/// from `old`, unless a color reaches across the window; whether they were.
pub(crate) fn rehighlight(
    old: &[(Range<Byte>, Style)],
    doc: &Doc,
    splice: &Splice,
) -> (Vec<(Range<Byte>, Style)>, bool) {
    let text = doc.text();
    let window = &splice.window;
    let old_end = shifted(window.end, -splice.delta);
    let head = old.partition_point(|(r, _)| r.start < window.start);
    let tail = old.partition_point(|(r, _)| r.start < old_end);
    let fresh = highlight_blocks(text, &doc.blocks[splice.new.clone()]);
    // comrak's blocks can overlap, as an HTML block behind a tab does; a color across the edge of the window then belongs
    // to both sides, and only the whole text tells which one wins.
    let crosses = old[..head].last().is_some_and(|(r, _)| r.end > window.start)
        || old[..tail].last().is_some_and(|(r, _)| r.end > old_end)
        || fresh.iter().any(|(r, _)| r.start < window.start || r.end > window.end);
    if crosses {
        return (highlight(doc), false);
    }
    let mut out = Vec::with_capacity(head + fresh.len() + old.len() - tail);
    out.extend_from_slice(&old[..head]);
    out.extend(fresh);
    let at = |o: Byte| shifted(o, splice.delta);
    out.extend(old[tail..].iter().map(|(r, s)| (at(r.start)..at(r.end), *s)));
    (out, true)
}

fn push(spans: &mut Vec<(Range<Byte>, Style)>, range: Range<Byte>, paint: Style) {
    if range.start < range.end {
        spans.push((range, paint));
    }
}

fn block_spans(text: &str, lines: &LineStarts, b: &Block, spans: &mut Vec<(Range<Byte>, Style)>) {
    match &b.kind {
        BlockKind::Heading { attrs, .. } => {
            spans.push((
                b.range.clone(),
                Style {
                    token: Some(Token::Heading),
                    bold: true,
                    italic: false,
                    strike: false,
                },
            ));
            if let Some(attrs) = attrs {
                push(spans, attrs.clone(), Style::color(Token::Comment));
            }
        }
        BlockKind::Comment => push(spans, b.range.clone(), Style::color(Token::Comment)),
        BlockKind::DefinitionDetails => {
            if text.at(b.range.clone()).starts_with(':') {
                push(spans, b.range.start..b.range.start + 1, Style::color(Token::ListMarker));
            }
        }
        BlockKind::Item { task, marker } => {
            push(spans, marker.clone(), Style::color(Token::ListMarker));
            if let Some(t) = task {
                let start = text.upto(t.symbol).rfind('[').map_or(t.symbol, Byte::new);
                push(
                    spans,
                    start..(t.symbol + 2).min(text.end()),
                    Style::color(Token::ListMarker),
                );
            }
        }
        BlockKind::BlockQuote => quote_markers(text, lines, b, spans),
        BlockKind::Alert { marker, .. } => {
            quote_markers(text, lines, b, spans);
            spans.push((
                marker.clone(),
                Style {
                    token: Some(Token::Quote),
                    bold: true,
                    italic: false,
                    strike: false,
                },
            ));
        }
        BlockKind::CodeBlock {
            content, fenced, info, ..
        } => {
            if *fenced {
                push(spans, b.range.start..content.start, Style::color(Token::Punctuation));
                push(spans, content.end..b.range.end, Style::color(Token::Punctuation));
            }
            if super::doc::is_math_fence(info) {
                push(spans, content.clone(), Style::color(Token::Math));
            } else if code::known(info) {
                for (r, t) in code::tokens(info, text.at(content.clone())) {
                    push(
                        spans,
                        content.start + r.start..content.start + r.end,
                        Style::color(t.into()),
                    );
                }
            } else {
                push(spans, content.clone(), Style::color(Token::Code));
            }
        }
        BlockKind::HtmlBlock { .. } => push(spans, b.range.clone(), Style::color(Token::Html)),
        BlockKind::FrontMatter { .. } => push(spans, b.range.clone(), Style::color(Token::Comment)),
        BlockKind::ThematicBreak => push(spans, b.range.clone(), Style::color(Token::Punctuation)),
        BlockKind::MathBlock { .. } => push(spans, b.range.clone(), Style::color(Token::Math)),
        BlockKind::Table { .. } => table_spans(text, lines, b, spans),
        BlockKind::TableRow { header: true } => {
            spans.push((b.range.clone(), Style::flags(true, false, false)));
        }
        BlockKind::FootnoteDefinition { label, .. } => push(spans, label.clone(), Style::color(Token::LinkText)),
        _ => {}
    }
    for i in b.inlines() {
        inline_spans(text, i, spans);
    }
}

fn quote_markers(text: &str, lines: &LineStarts, b: &Block, spans: &mut Vec<(Range<Byte>, Style)>) {
    for line in lines.line_of(b.range.start)..=lines.line_of(shifted(b.range.end, -1)) {
        let lr = lines.line_range(text, line);
        let end = behind_quote_marks(text.at(lr.clone()));
        push(spans, lr.start..lr.start + end, Style::color(Token::Quote));
    }
}

fn table_spans(text: &str, lines: &LineStarts, b: &Block, spans: &mut Vec<(Range<Byte>, Style)>) {
    let first = lines.line_of(b.range.start);
    let last = lines.line_of(shifted(b.range.end, -1));
    for line in first..=last {
        let lr = lines.line_range(text, line);
        if line == first + 1 {
            push(spans, lr, Style::color(Token::Punctuation));
            continue;
        }
        for pipe in unescaped_pipes(text.at(lr.clone())) {
            push(
                spans,
                lr.start + pipe..lr.start + pipe + 1,
                Style::color(Token::Punctuation),
            );
        }
    }
}

/// Whether a source line reads right to left once its Markdown markers are skipped, though a marker with a Latin
/// letter, as `[x]` or `[!NOTE]`, starts it and would make it read left to right.
pub fn right_to_left_behind_markers(line: &str) -> bool {
    let mut rest = line;
    let mut quoted = false;
    loop {
        let end = line_marks(rest)
            .inspect(|mark| quoted |= mark.kind == ContainerMark::Quote)
            .last()
            .map_or(0, |mark| mark.range.end);
        rest = &rest[end..];
        rest = &rest[indent_len(rest)..];
        let behind_alert = rest
            .strip_prefix("[!")
            .filter(|_| quoted)
            .and_then(|r| Some(&r[r.find(']')? + 1..]));
        match behind_alert {
            Some(behind) => rest = behind.trim_start_matches(['-', '+']),
            None => break,
        }
    }
    right_to_left(rest) == Some(true) && right_to_left(line) != Some(true)
}

/// Byte offsets of `|` characters that delimit table cells. GFM splits cells on every unescaped pipe,
/// including pipes inside code spans.
pub(crate) fn unescaped_pipes(line: &str) -> Vec<usize> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b'|' => out.push(i),
            _ => {}
        }
        i += 1;
    }
    out
}

fn delimiters(i: &Inline, spans: &mut Vec<(Range<Byte>, Style)>, token: Token) {
    let content = i.content_range();
    push(spans, i.range.start..content.start, Style::color(token));
    push(spans, content.end..i.range.end, Style::color(token));
}

fn inline_spans(text: &str, i: &Inline, spans: &mut Vec<(Range<Byte>, Style)>) {
    match &i.kind {
        InlineKind::Strong => {
            spans.push((i.range.clone(), Style::flags(true, false, false)));
            delimiters(i, spans, Token::Punctuation);
        }
        InlineKind::Emph => {
            spans.push((i.range.clone(), Style::flags(false, true, false)));
            delimiters(i, spans, Token::Punctuation);
        }
        InlineKind::Strikethrough => {
            spans.push((i.range.clone(), Style::flags(false, false, true)));
            delimiters(i, spans, Token::Punctuation);
        }
        InlineKind::Underline | InlineKind::Highlight | InlineKind::Superscript | InlineKind::Subscript => {
            let tag = i.written_as_html(text);
            delimiters(i, spans, if tag { Token::Html } else { Token::Punctuation })
        }
        InlineKind::Spoiler => delimiters(i, spans, Token::Punctuation),
        InlineKind::WikiLink { .. } => {
            delimiters(i, spans, Token::Punctuation);
            push(spans, i.content_range(), Style::color(Token::LinkText));
        }
        InlineKind::Emoji(_) => push(spans, i.range.clone(), Style::color(Token::Punctuation)),
        InlineKind::Kbd | InlineKind::Span => delimiters(i, spans, Token::Html),
        InlineKind::HtmlImage { .. } => push(spans, i.range.clone(), Style::color(Token::Html)),
        InlineKind::Link { .. } if i.written_as_html(text) => {
            delimiters(i, spans, Token::Html);
            push(spans, i.content_range(), Style::color(Token::LinkText));
        }
        InlineKind::Comment => push(spans, i.range.clone(), Style::color(Token::Comment)),
        InlineKind::Code(_) => push(spans, i.range.clone(), Style::color(Token::Code)),
        InlineKind::Html(_) => push(spans, i.range.clone(), Style::color(Token::Html)),
        InlineKind::Math { .. } => push(spans, i.range.clone(), Style::color(Token::Math)),
        InlineKind::FootnoteRef { .. } => push(spans, i.range.clone(), Style::color(Token::LinkText)),
        InlineKind::Link { .. } | InlineKind::Image { .. } => {
            let src = text.at(i.range.clone());
            if !src.starts_with('[') && !src.starts_with("![") {
                push(spans, i.range.clone(), Style::color(Token::Url));
            } else {
                let content = i.content_range();
                push(spans, i.range.start..content.start, Style::color(Token::Punctuation));
                push(spans, content.clone(), Style::color(Token::LinkText));
                let tail = text.at(content.end..i.range.end);
                match (tail.find('('), tail.rfind(')')) {
                    (Some(open), Some(close)) if open < close => {
                        push(
                            spans,
                            content.end..content.end + open + 1,
                            Style::color(Token::Punctuation),
                        );
                        push(
                            spans,
                            content.end + open + 1..content.end + close,
                            Style::color(Token::Url),
                        );
                        push(
                            spans,
                            content.end + close..i.range.end,
                            Style::color(Token::Punctuation),
                        );
                    }
                    _ => push(spans, content.end..i.range.end, Style::color(Token::Punctuation)),
                }
            }
        }
        InlineKind::Text(t) => {
            let src = text.at(i.range.clone());
            if src.len() != t.len() {
                let mut prev_backslash = false;
                for (pos, c) in src.char_indices() {
                    if c == '\\' && !prev_backslash {
                        push(
                            spans,
                            i.range.start + pos..i.range.start + pos + 1,
                            Style::color(Token::Punctuation),
                        );
                        prev_backslash = true;
                    } else {
                        prev_backslash = false;
                    }
                }
            }
        }
        _ => {}
    }
    for c in &i.children {
        inline_spans(text, c, spans);
    }
}

fn flatten(spans: Vec<(Range<Byte>, Style)>) -> Vec<(Range<Byte>, Style)> {
    let mut events: Vec<(Byte, bool, usize)> = Vec::with_capacity(spans.len() * 2);
    for (idx, (r, _)) in spans.iter().enumerate() {
        events.push((r.start, true, idx));
        events.push((r.end, false, idx));
    }
    events.sort_by_key(|&(pos, is_start, idx)| (pos, is_start, idx));
    let mut active: Vec<usize> = Vec::new();
    let mut out: Vec<(Range<Byte>, Style)> = Vec::new();
    let mut last = Byte::default();
    for (pos, is_start, idx) in events {
        if pos > last && !active.is_empty() {
            let mut style = Style::default();
            for &a in &active {
                let p = spans[a].1;
                if p.token.is_some() {
                    style.token = p.token;
                }
                style.bold |= p.bold;
                style.italic |= p.italic;
                style.strike |= p.strike;
            }
            match out.last_mut() {
                Some((r, s)) if r.end == last && *s == style => r.end = pos,
                _ => out.push((last..pos, style)),
            }
        }
        last = pos;
        if is_start {
            active.push(idx);
            active.sort_unstable();
        } else if let Some(p) = active.iter().position(|&a| a == idx) {
            active.remove(p);
        }
    }
    out
}

/// Byte range of `text` whose colors can differ from the previous highlighting of `old_text`: the edit itself and
/// every span that changed apart from moving with the edit.
pub fn changed_region(
    old_text: &str,
    old: &[(Range<Byte>, Style)],
    text: &str,
    spans: &[(Range<Byte>, Style)],
) -> Range<Byte> {
    // Most of a long text lies on either side of the edit and only moved with it, which a look at the ends shows quickly.
    let edit = Replaced::of(changed(old_text, text));
    let lead = old.iter().zip(spans).take_while(|(a, b)| edit.leads(a, b)).count();
    let rest = old.len().min(spans.len()) - lead;
    let trail = old
        .iter()
        .rev()
        .zip(spans.iter().rev())
        .take(rest)
        .take_while(|(a, b)| edit.trails(a, b))
        .count();
    edit.region(&old[lead..old.len() - trail], &spans[lead..spans.len() - trail], text)
}

/// [`changed_region`] after a partial parse that kept the colours outside `window`, the part of `text` it parsed again,
/// behind which `text` is `delta` bytes longer than `old_text`. Only the texts and spans around the window are looked at;
/// `None` where they do not lie as such a parse leaves them.
pub fn changed_region_in(
    old_text: &str,
    old: &[(Range<Byte>, Style)],
    text: &str,
    spans: &[(Range<Byte>, Style)],
    window: &Range<Byte>,
    delta: isize,
) -> Option<Range<Byte>> {
    let old_end = shifted(window.end, -delta);
    let fits = signed(text.len()) - signed(old_text.len()) == delta
        && window.start <= window.end
        && window.end <= text.end()
        && window.start <= old_end
        && old_end <= old_text.end()
        && text.boundary(window.start)
        && old_text.boundary(window.start)
        && text.boundary(window.end)
        && old_text.boundary(old_end);
    if !fits {
        return None;
    }
    let head = old.partition_point(|(r, _)| r.start < window.start);
    let old_tail = old.partition_point(|(r, _)| r.start < old_end);
    let tail = spans.partition_point(|(r, _)| r.start < window.end);
    let reaches = |list: &[(Range<Byte>, Style)], i: usize, edge: Byte| i > 0 && list[i - 1].0.end > edge;
    let moved = |(r, s): &(Range<Byte>, Style)| (shifted(r.start, delta)..shifted(r.end, delta), *s);
    let kept = spans.partition_point(|(r, _)| r.start < window.start) == head
        && old.len() - old_tail == spans.len() - tail
        && !reaches(old, head, window.start)
        && !reaches(spans, head, window.start)
        && !reaches(old, old_tail, old_end)
        && !reaches(spans, tail, window.end)
        && (head == 0 || old[head - 1] == spans[head - 1])
        && old.get(old_tail).is_none_or(|span| moved(span) == spans[tail]);
    if !kept {
        return None;
    }
    let edit = Replaced::of(changed_within(old_text, text, window, old_end));
    if edit.old_end > old_end || edit.end > window.end {
        return None;
    }
    let within = old_tail.min(tail);
    let lead = head
        + old[head..within]
            .iter()
            .zip(&spans[head..within])
            .take_while(|(a, b)| edit.leads(a, b))
            .count();
    let behind = old.len() - old_tail;
    let rest = old.len().min(spans.len()) - lead;
    let trail = behind
        + old[..old_tail]
            .iter()
            .rev()
            .zip(spans[..tail].iter().rev())
            .take(rest - behind)
            .take_while(|(a, b)| edit.trails(a, b))
            .count();
    Some(edit.region(&old[lead..old.len() - trail], &spans[lead..spans.len() - trail], text))
}

/// What [`changed`] finds for texts that are alike in front of `window` of `new` and behind it, where it ends at `old_end`
/// in `old`: only what lies between is compared.
fn changed_within(old: &str, new: &str, window: &Range<Byte>, old_end: Byte) -> (Range<Byte>, Range<Byte>) {
    let (old_bytes, new_bytes) = (old.as_bytes(), new.as_bytes());
    let from = window.start.get();
    let mut prefix = from + common_prefix(&old_bytes[from..], &new_bytes[from..]);
    while !(old.is_char_boundary(prefix) && new.is_char_boundary(prefix)) {
        prefix -= 1;
    }
    let max_suffix = old.len().min(new.len()) - prefix;
    let alike_behind = new.len() - window.end.get();
    let mut suffix = if max_suffix <= alike_behind {
        max_suffix
    } else {
        let (old_part, new_part) = (&old_bytes[..old_end.get()], &new_bytes[..window.end.get()]);
        alike_behind + common_suffix(old_part, new_part, max_suffix - alike_behind)
    };
    while !(old.is_char_boundary(old.len() - suffix) && new.is_char_boundary(new.len() - suffix)) {
        suffix -= 1;
    }
    (
        Byte::new(prefix)..old.end() - suffix,
        Byte::new(prefix)..new.end() - suffix,
    )
}

/// Where an edit replaced text: from `start` on, up to `old_end` of the old text and `end` of the new one.
struct Replaced {
    start: Byte,
    old_end: Byte,
    end: Byte,
}

impl Replaced {
    fn of((old, new): (Range<Byte>, Range<Byte>)) -> Replaced {
        Replaced {
            start: old.start,
            old_end: old.end,
            end: new.end,
        }
    }

    /// Where a position of the old text lies in the new one.
    fn moved(&self, o: Byte) -> Byte {
        if o <= self.start {
            o
        } else if o >= self.old_end {
            self.end + (o - self.old_end)
        } else {
            self.end
        }
    }

    /// Whether an old span ahead of the edit is the same in the new text.
    fn leads(&self, old: &(Range<Byte>, Style), new: &(Range<Byte>, Style)) -> bool {
        old == new && old.0.end <= self.start
    }

    /// Whether an old span behind the edit only moved with it.
    fn trails(&self, old: &(Range<Byte>, Style), new: &(Range<Byte>, Style)) -> bool {
        old.1 == new.1 && old.0.start >= self.old_end && (self.moved(old.0.start)..self.moved(old.0.end)) == new.0
    }

    /// The edit and every span between the alike ones at either end, old or new.
    fn region(&self, old: &[(Range<Byte>, Style)], new: &[(Range<Byte>, Style)], text: &str) -> Range<Byte> {
        let start = new
            .iter()
            .map(|s| s.0.start)
            .chain(old.iter().map(|s| s.0.start.min(self.start)))
            .fold(self.start, Byte::min);
        let end = new
            .iter()
            .map(|s| s.0.end)
            .chain(old.iter().map(|s| self.moved(s.0.end)))
            .fold(self.end, Byte::max)
            .min(text.end());
        start..end.max(start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::parse;
    use crate::text::bytes;

    fn styled(t: &str) -> Vec<(&str, Style)> {
        let d = parse(t);
        highlight(&d).into_iter().map(|(r, s)| (t.at(r), s)).collect()
    }

    #[test]
    fn a_done_task_in_hebrew_reads_right_to_left() {
        assert!(right_to_left_behind_markers("- [x] משימה"));
        assert!(right_to_left_behind_markers("> 1. [X] مهمة"));
        assert!(right_to_left_behind_markers("> [!NOTE]- הערה"));
        assert!(!right_to_left_behind_markers("- [ ] משימה"));
        assert!(!right_to_left_behind_markers("- [x] task"));
        assert!(!right_to_left_behind_markers("2024 [x] שנה"));
        assert!(!right_to_left_behind_markers("[x] משימה"));
        assert!(!right_to_left_behind_markers("[!NOTE] הערה"));
    }

    #[test]
    fn strong_gets_bold_content_and_punctuation_delimiters() {
        let s = styled("a **bö** c\n");
        assert_eq!(s[0].0, "**");
        assert_eq!(s[0].1.token, Some(Token::Punctuation));
        assert!(s[0].1.bold);
        assert_eq!(s[1].0, "bö");
        assert!(s[1].1.bold && s[1].1.token.is_none());
        assert_eq!(s[2].0, "**");
    }

    #[test]
    fn link_parts() {
        let s = styled("[t](http://x)\n");
        let parts: Vec<_> = s.iter().map(|(t, st)| (*t, st.token)).collect();
        assert_eq!(
            parts,
            vec![
                ("[", Some(Token::Punctuation)),
                ("t", Some(Token::LinkText)),
                ("](", Some(Token::Punctuation)),
                ("http://x", Some(Token::Url)),
                (")", Some(Token::Punctuation)),
            ]
        );
    }

    #[test]
    fn table_pipes_and_delimiter_row() {
        let s = styled("| a | `x\\|y` |\n|---|---|\n| 1 | 2 |\n");
        let pipes: usize = s
            .iter()
            .filter(|(_, st)| st.token == Some(Token::Punctuation))
            .map(|(t, _)| t.matches('|').count())
            .sum();
        assert_eq!(pipes, 6 + 3);
        assert!(s.iter().any(|(t, _)| t.contains("|---|---|")));
        assert!(s.iter().any(|(t, st)| *t == "`x\\|y`" && st.token == Some(Token::Code)));
    }

    #[test]
    fn code_blocks_with_a_known_language_get_token_colors() {
        let s = styled("```python\nif x: # c\n```\n\n```\nif x\n```\n");
        assert!(s.iter().any(|(t, st)| *t == "if" && st.token == Some(Token::Keyword)));
        assert!(s.iter().any(|(t, st)| *t == "# c" && st.token == Some(Token::Comment)));
        assert!(s.iter().any(|(t, st)| *t == "if x\n" && st.token == Some(Token::Code)));
    }

    #[test]
    fn only_the_changed_region_is_recolored() {
        let a = "Text **fett** und\n\n```\ncode\n";
        let b = "Text **fetter** und\n\n```\ncode\n";
        let r = changed_region(a, &highlight(&parse(a)), b, &highlight(&parse(b)));
        assert_eq!(r, bytes(7..15));
        let c = "Text **fetter** und\n\n```\ncode\n```\nnach\n";
        let r = changed_region(b, &highlight(&parse(b)), c, &highlight(&parse(c)));
        assert!(
            r.start >= Byte::new(21) && r.start <= Byte::new(b.len()) && r.end == Byte::new(c.len()),
            "{r:?}"
        );
    }

    #[test]
    fn quote_and_list_markers() {
        let s = styled("> - [ ] x\n");
        assert!(
            s.iter()
                .any(|(t, st)| t.trim() == ">" && st.token == Some(Token::Quote))
        );
        assert!(
            s.iter()
                .any(|(t, st)| t.contains('[') && st.token == Some(Token::ListMarker))
        );
    }

    fn token_of(t: &str, piece: &str) -> Option<Token> {
        let spans = styled(t);
        let found = spans.iter().find(|(text, _)| *text == piece);
        found.unwrap_or_else(|| panic!("no {piece:?} in {spans:?}")).1.token
    }

    #[test]
    fn every_kind_of_block_has_its_color() {
        let t = "# T {#a}\n";
        assert_eq!(token_of(t, "# T "), Some(Token::Heading));
        assert!(styled(t)[0].1.bold);
        assert_eq!(token_of(t, "{#a}"), Some(Token::Comment));
        assert_eq!(token_of("%%\na\n%%\n", "%%\na\n%%"), Some(Token::Comment));
        assert_eq!(token_of("x\n\nT\n: d\n", ":"), Some(Token::ListMarker));
        assert_eq!(token_of("<div>\nx\n</div>\n", "<div>\nx\n</div>"), Some(Token::Html));
        assert_eq!(token_of("---\na: b\n---\n", "---\na: b\n---"), Some(Token::Comment));
        assert_eq!(token_of("x\n\n---\n", "---"), Some(Token::Punctuation));
        assert_eq!(token_of("$$\na\n$$\n", "$$\na\n$$"), Some(Token::Math));
        assert_eq!(token_of("x[^a]\n\n[^a]: b\n", "[^a]:"), Some(Token::LinkText));
    }

    #[test]
    fn a_table_head_is_bold_and_the_rest_is_not() {
        let spans = styled("a|b\n-|-\n1|2\n");
        assert!(spans.iter().find(|(t, _)| *t == "a").unwrap().1.bold);
        assert!(!spans.iter().any(|(t, s)| *t == "1" && s.bold));
    }

    #[test]
    fn an_alert_shows_its_marker_and_all_quote_marks_of_a_line() {
        let s = styled("> [!NOTE] a\n> b\n");
        assert!(
            s.iter()
                .any(|(t, st)| *t == "[!NOTE] a" && st.token == Some(Token::Quote) && st.bold)
        );
        assert_eq!(token_of("> > a\n", "> >"), Some(Token::Quote));
    }

    #[test]
    fn right_to_left_needs_a_real_marker_in_front_of_it() {
        assert!(
            !right_to_left_behind_markers(". [x] משימה"),
            "a dot without a number is no list"
        );
        assert!(
            !right_to_left_behind_markers("1.[x] משימה"),
            "nor a number without a space behind it"
        );
        assert!(right_to_left_behind_markers("> [!NOTE]- - [x] הערה"));
    }

    #[test]
    fn an_escaped_pipe_splits_no_cell() {
        assert_eq!(unescaped_pipes("a\\|b|c"), [4]);
        assert_eq!(unescaped_pipes("|\\\\|"), [0, 3]);
    }

    #[test]
    fn every_kind_of_inline_has_its_color_and_style() {
        for (t, delimiter, token) in [
            ("a <u>b</u>\n", "<u>", Token::Html),
            ("a ==b==\n", "==", Token::Punctuation),
            ("a ^b^\n", "^", Token::Punctuation),
            ("a ~b~\n", "~", Token::Punctuation),
            ("a ||b||\n", "||", Token::Punctuation),
            ("a <kbd>b</kbd>\n", "<kbd>", Token::Html),
            ("a <span>b</span>\n", "<span>", Token::Html),
            ("a [[b]]\n", "[[", Token::Punctuation),
            ("a <a href=\"u\">b</a>\n", "<a href=\"u\">", Token::Html),
        ] {
            assert_eq!(token_of(t, delimiter), Some(token), "{t}");
        }
        assert_eq!(token_of("a [[b]]\n", "b"), Some(Token::LinkText));
        assert_eq!(token_of("a <a href=\"u\">b</a>\n", "b"), Some(Token::LinkText));
        assert_eq!(token_of("a :smile: b\n", ":smile:"), Some(Token::Punctuation));
        assert_eq!(
            token_of("a <img src=\"x.png\"> b\n", "<img src=\"x.png\">"),
            Some(Token::Html)
        );
        assert_eq!(token_of("a %%c%% b\n", "%%c%%"), Some(Token::Comment));
        assert_eq!(token_of("a <foo> b\n", "<foo>"), Some(Token::Html));
        assert_eq!(token_of("a $b$ c\n", "$b$"), Some(Token::Math));
        assert_eq!(token_of("x[^a]\n\n[^a]: b\n", "[^a]"), Some(Token::LinkText));
        assert_eq!(token_of("a http://x.de b\n", "http://x.de"), Some(Token::Url));
        assert_eq!(token_of("![a](u)\n", "u"), Some(Token::Url));
        assert_eq!(token_of("![a](u)\n", "a"), Some(Token::LinkText));
        assert_eq!(token_of("![a](u)\n", "!["), Some(Token::Punctuation));
    }

    #[test]
    fn overlapping_styles_add_up() {
        let inner = |t: &'static str| styled(t).into_iter().find(|(x, _)| *x == "a").unwrap().1;
        let italic = inner("x *a* y\n");
        assert!(italic.italic && !italic.bold && !italic.strike);
        let strike = inner("x ~~a~~ y\n");
        assert!(strike.strike && !strike.bold && !strike.italic);
        let both = inner("x ***a*** y\n");
        assert!(both.bold && both.italic);
        let all = inner("x ~~***a***~~ y\n");
        assert!(all.bold && all.italic && all.strike);
    }

    #[test]
    fn neighbouring_spans_of_one_style_become_one() {
        let bold = Style::flags(true, false, false);
        assert_eq!(
            flatten(vec![(bytes(0..2), bold), (bytes(2..4), bold)]),
            [(bytes(0..4), bold)]
        );
    }

    #[test]
    fn only_the_backslash_of_an_escape_is_marked() {
        let marks = |t: &'static str| {
            styled(t)
                .into_iter()
                .filter(|(_, s)| s.token == Some(Token::Punctuation))
                .map(|(x, _)| x)
                .collect::<Vec<_>>()
        };
        assert_eq!(marks("a \\* b\n"), ["\\"]);
        assert_eq!(marks("a \\\\ b\n"), ["\\"], "a backslash that is the text itself");
        assert!(marks("a\\b\n").is_empty(), "no escape");
    }

    #[test]
    fn a_changed_region_starts_and_ends_at_whole_letters() {
        let region = |a: &str, b: &str| changed_region(a, &[], b, &[]);
        assert_eq!(region("x é y", "x è y"), bytes(2..4), "the letters begin alike");
        assert_eq!(region("x éy", "x ĩy"), bytes(2..4), "the letters end alike");
    }

    /// A generator of its own, as the one in `splat-testkit` depends on this crate.
    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            usize::try_from(self.0 % u64::try_from(n.max(1)).unwrap_or(1)).unwrap_or(0)
        }

        fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
            items[self.below(items.len())]
        }
    }

    const PIECES: [&str; 32] = [
        "Wort",
        " ",
        "ä",
        "😀",
        "**",
        "*",
        "`",
        "~~",
        "[",
        "](u)",
        "\n",
        "\n\n",
        "> ",
        "- ",
        "1. ",
        "# ",
        "Titel\n===",
        "```\n",
        "```rust\nlet x = 1;\n```",
        "<div>\n",
        "</div>",
        "\t",
        "    ",
        "| a | b |\n|---|---|\n",
        "|",
        "<u>x</u>",
        "\n- [ ] a\n",
        "$",
        "<!-- c -->",
        "> [!NOTE]\n> ",
        "==",
        "<b>",
    ];

    fn paragraph(rng: &mut Rng) -> String {
        (0..1 + rng.below(8)).map(|_| rng.pick(&PIECES)).collect()
    }

    /// The colours of a partial parse are those of a full one, so the region it finds must be the one that comparing all
    /// of them finds.
    #[test]
    fn the_region_after_a_partial_parse_is_the_one_comparing_everything_finds() {
        let mut rng = Rng(0x5eed_c010_0001);
        let mut kept = 0;
        for _ in 0..2000 {
            let old = (0..2 + rng.below(8))
                .map(|_| paragraph(&mut rng))
                .collect::<Vec<_>>()
                .join("\n\n");
            let doc = parse(&old);
            let old_spans = highlight(&doc);
            let start = old.floor_char_boundary(rng.below(old.len() + 1));
            let end = old.floor_char_boundary(start + rng.below(6)).max(start);
            let mut new = old.clone();
            new.replace_range(start..end, if rng.below(4) == 0 { "" } else { rng.pick(&PIECES) });
            let parsed = crate::incremental::reparse_and_color(doc, &old_spans, &new);
            let Some(splice) = parsed.splice.as_ref().filter(|_| parsed.kept_colors) else {
                continue;
            };
            kept += 1;
            let what = format!("{old:?} -> {new:?}, {splice:?}");
            let old_end = shifted(splice.window.end, -splice.delta);
            assert_eq!(
                changed_within(&old, &new, &splice.window, old_end),
                changed(&old, &new),
                "{what}"
            );
            assert_eq!(
                changed_region_in(&old, &old_spans, &new, &parsed.spans, &splice.window, splice.delta),
                Some(changed_region(&old, &old_spans, &new, &parsed.spans)),
                "{what}"
            );
        }
        assert!(kept > 200, "only {kept} partial parses kept their colours");
    }

    #[test]
    fn a_window_that_does_not_fit_the_texts_or_colours_finds_no_region() {
        let span = |r: Range<usize>| (bytes(r), Style::color(Token::Code));
        let (old, new) = ("abc def", "abc xdef");
        let region = |old_spans: &[(Range<Byte>, Style)], spans: &[(Range<Byte>, Style)], window, delta| {
            changed_region_in(old, old_spans, new, spans, &bytes(window), delta)
        };
        assert_eq!(region(&[], &[], 4..5, 1), Some(bytes(4..5)));
        assert_eq!(region(&[], &[], 4..5, 2), None, "another length");
        assert_eq!(region(&[], &[], 4..9, 1), None, "beyond the text");
        assert_eq!(region(&[], &[], 0..1, 1), None, "the change outside the window");
        assert_eq!(region(&[span(2..5)], &[span(2..6)], 4..5, 1), None, "across the start");
        assert_eq!(region(&[], &[span(4..6)], 4..5, 1), None, "across the end");
        assert_eq!(
            region(&[span(4..6)], &[], 4..6, 1),
            None,
            "across the end of the old window"
        );
        assert_eq!(region(&[span(5..6)], &[], 4..5, 1), None, "a span went missing behind");
        assert_eq!(
            region(&[span(0..1)], &[span(0..2)], 4..5, 1),
            None,
            "the one in front differs"
        );
        assert_eq!(
            region(&[span(5..6)], &[span(5..7)], 4..5, 1),
            None,
            "the one behind did not move"
        );
        assert_eq!(
            region(
                &[span(0..1), span(5..6)],
                &[span(0..1), span(4..5), span(6..7)],
                4..5,
                1
            ),
            Some(bytes(4..5))
        );
        assert_eq!(
            changed_region_in("ä", &[], "ö", &[], &bytes(1..2), 0),
            None,
            "inside a letter"
        );
    }

    #[test]
    fn spans_behind_the_edit_only_move() {
        let a = "*a* *b* *c* X *d* *e* *f* *g*\n";
        let b = "*a* *b* *c* Y *d* *e* *f* *g*\n";
        let r = changed_region(a, &highlight(&parse(a)), b, &highlight(&parse(b)));
        assert_eq!(r, bytes(12..13));
    }
}
