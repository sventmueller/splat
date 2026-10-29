use std::ops::Range;

use super::code::{self, CodeToken};
use super::doc::{Block, BlockKind, Doc, Inline, InlineKind, walk_blocks};
use super::text::{LineStarts, common_prefix, common_suffix};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Token {
    Heading,
    Punctuation,
    Code,
    LinkText,
    Url,
    ListMarker,
    Quote,
    Html,
    Math,
    Keyword,
    String,
    Comment,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Style {
    pub token: Option<Token>,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
}

#[derive(Clone, Copy)]
struct Paint {
    token: Option<Token>,
    bold: bool,
    italic: bool,
    strike: bool,
}

impl Paint {
    fn color(token: Token) -> Self {
        Paint {
            token: Some(token),
            bold: false,
            italic: false,
            strike: false,
        }
    }
    fn flags(bold: bool, italic: bool, strike: bool) -> Self {
        Paint {
            token: None,
            bold,
            italic,
            strike,
        }
    }
}

/// Styled, non-overlapping byte ranges for the source view. Unstyled text is omitted.
pub fn highlight(text: &str, doc: &Doc) -> Vec<(Range<usize>, Style)> {
    highlight_blocks(text, &doc.blocks)
}

fn highlight_blocks(text: &str, blocks: &[Block]) -> Vec<(Range<usize>, Style)> {
    let mut spans: Vec<(Range<usize>, Paint)> = Vec::new();
    let lines = LineStarts::new(text);
    walk_blocks(blocks, &mut |b| block_spans(text, &lines, b, &mut spans));
    flatten(spans)
}

/// The colors of `text` after `splice` changed `doc`: those of the re-parsed blocks are computed anew, the others
/// taken from `old`.
pub fn rehighlight(
    old: &[(Range<usize>, Style)],
    text: &str,
    doc: &Doc,
    splice: &super::incremental::Splice,
) -> Vec<(Range<usize>, Style)> {
    let window = &splice.window;
    let old_end = (window.end as isize - splice.delta) as usize;
    let head = old.partition_point(|(r, _)| r.start < window.start);
    let tail = old.partition_point(|(r, _)| r.start < old_end);
    let fresh = highlight_blocks(text, &doc.blocks[splice.new.clone()]);
    let mut out = Vec::with_capacity(head + fresh.len() + old.len() - tail);
    out.extend_from_slice(&old[..head]);
    out.extend(fresh);
    let at = |o: usize| (o as isize + splice.delta) as usize;
    out.extend(old[tail..].iter().map(|(r, s)| (at(r.start)..at(r.end), *s)));
    out
}

fn push(spans: &mut Vec<(Range<usize>, Paint)>, range: Range<usize>, paint: Paint) {
    if range.start < range.end {
        spans.push((range, paint));
    }
}

fn block_spans(text: &str, lines: &LineStarts, b: &Block, spans: &mut Vec<(Range<usize>, Paint)>) {
    match &b.kind {
        BlockKind::Heading { attrs, .. } => {
            spans.push((
                b.range.clone(),
                Paint {
                    token: Some(Token::Heading),
                    bold: true,
                    italic: false,
                    strike: false,
                },
            ));
            if let Some(attrs) = attrs {
                push(spans, attrs.clone(), Paint::color(Token::Comment));
            }
        }
        BlockKind::Comment => push(spans, b.range.clone(), Paint::color(Token::Comment)),
        BlockKind::DefinitionDetails => {
            if text[b.range.clone()].starts_with(':') {
                push(spans, b.range.start..b.range.start + 1, Paint::color(Token::ListMarker));
            }
        }
        BlockKind::Item { task, marker } => {
            push(spans, marker.clone(), Paint::color(Token::ListMarker));
            if let Some(t) = task {
                let start = text[..t.symbol].rfind('[').unwrap_or(t.symbol);
                push(
                    spans,
                    start..(t.symbol + 2).min(text.len()),
                    Paint::color(Token::ListMarker),
                );
            }
        }
        BlockKind::BlockQuote => quote_markers(text, lines, b, spans),
        BlockKind::Alert { marker, .. } => {
            quote_markers(text, lines, b, spans);
            spans.push((
                marker.clone(),
                Paint {
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
                push(spans, b.range.start..content.start, Paint::color(Token::Punctuation));
                push(spans, content.end..b.range.end, Paint::color(Token::Punctuation));
            }
            if super::doc::is_math_fence(info) {
                push(spans, content.clone(), Paint::color(Token::Math));
            } else if code::known(info) {
                for (r, t) in code::tokens(info, &text[content.clone()]) {
                    push(
                        spans,
                        content.start + r.start..content.start + r.end,
                        Paint::color(t.into()),
                    );
                }
            } else {
                push(spans, content.clone(), Paint::color(Token::Code));
            }
        }
        BlockKind::HtmlBlock { .. } => push(spans, b.range.clone(), Paint::color(Token::Html)),
        BlockKind::FrontMatter { .. } => push(spans, b.range.clone(), Paint::color(Token::Comment)),
        BlockKind::ThematicBreak => push(spans, b.range.clone(), Paint::color(Token::Punctuation)),
        BlockKind::MathBlock { .. } => push(spans, b.range.clone(), Paint::color(Token::Math)),
        BlockKind::Table { .. } => table_spans(text, lines, b, spans),
        BlockKind::TableRow { header: true } => {
            spans.push((b.range.clone(), Paint::flags(true, false, false)));
        }
        BlockKind::FootnoteDefinition { label, .. } => push(spans, label.clone(), Paint::color(Token::LinkText)),
        _ => {}
    }
    for i in &b.inlines {
        inline_spans(text, i, spans);
    }
}

fn quote_markers(text: &str, lines: &LineStarts, b: &Block, spans: &mut Vec<(Range<usize>, Paint)>) {
    for line in lines.line_of(b.range.start)..=lines.line_of(b.range.end.saturating_sub(1)) {
        let lr = lines.line_range(text, line);
        let s = &text[lr.clone()];
        let mut end = 0;
        for (i, c) in s.char_indices() {
            match c {
                '>' => end = i + 1,
                ' ' | '\t' => {}
                _ => break,
            }
        }
        push(spans, lr.start..lr.start + end, Paint::color(Token::Quote));
    }
}

fn table_spans(text: &str, lines: &LineStarts, b: &Block, spans: &mut Vec<(Range<usize>, Paint)>) {
    let first = lines.line_of(b.range.start);
    let last = lines.line_of(b.range.end.saturating_sub(1));
    for line in first..=last {
        let lr = lines.line_range(text, line);
        if line == first + 1 {
            push(spans, lr, Paint::color(Token::Punctuation));
            continue;
        }
        for pipe in unescaped_pipes(&text[lr.clone()]) {
            push(
                spans,
                lr.start + pipe..lr.start + pipe + 1,
                Paint::color(Token::Punctuation),
            );
        }
    }
}

/// Byte offsets of `|` characters that delimit table cells. GFM splits cells on every unescaped pipe,
/// including pipes inside code spans.
pub fn unescaped_pipes(line: &str) -> Vec<usize> {
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

fn delimiters(i: &Inline, spans: &mut Vec<(Range<usize>, Paint)>, token: Token) {
    let content = i.content_range();
    push(spans, i.range.start..content.start, Paint::color(token));
    push(spans, content.end..i.range.end, Paint::color(token));
}

fn inline_spans(text: &str, i: &Inline, spans: &mut Vec<(Range<usize>, Paint)>) {
    match &i.kind {
        InlineKind::Strong => {
            spans.push((i.range.clone(), Paint::flags(true, false, false)));
            delimiters(i, spans, Token::Punctuation);
        }
        InlineKind::Emph => {
            spans.push((i.range.clone(), Paint::flags(false, true, false)));
            delimiters(i, spans, Token::Punctuation);
        }
        InlineKind::Strikethrough => {
            spans.push((i.range.clone(), Paint::flags(false, false, true)));
            delimiters(i, spans, Token::Punctuation);
        }
        InlineKind::Underline | InlineKind::Highlight | InlineKind::Superscript | InlineKind::Subscript => {
            let tag = text[i.range.clone()].starts_with('<');
            delimiters(i, spans, if tag { Token::Html } else { Token::Punctuation })
        }
        InlineKind::Spoiler => delimiters(i, spans, Token::Punctuation),
        InlineKind::WikiLink { .. } => {
            delimiters(i, spans, Token::Punctuation);
            push(spans, i.content_range(), Paint::color(Token::LinkText));
        }
        InlineKind::Emoji(_) => push(spans, i.range.clone(), Paint::color(Token::Punctuation)),
        InlineKind::Kbd | InlineKind::Span => delimiters(i, spans, Token::Html),
        InlineKind::HtmlImage { .. } => push(spans, i.range.clone(), Paint::color(Token::Html)),
        InlineKind::Link { .. } if text[i.range.clone()].starts_with('<') => {
            delimiters(i, spans, Token::Html);
            push(spans, i.content_range(), Paint::color(Token::LinkText));
        }
        InlineKind::Comment => push(spans, i.range.clone(), Paint::color(Token::Comment)),
        InlineKind::Code(_) => push(spans, i.range.clone(), Paint::color(Token::Code)),
        InlineKind::Html(_) => push(spans, i.range.clone(), Paint::color(Token::Html)),
        InlineKind::Math { .. } => push(spans, i.range.clone(), Paint::color(Token::Math)),
        InlineKind::FootnoteRef { .. } => push(spans, i.range.clone(), Paint::color(Token::LinkText)),
        InlineKind::Link { .. } | InlineKind::Image { .. } => {
            let src = &text[i.range.clone()];
            if !src.starts_with('[') && !src.starts_with("![") {
                push(spans, i.range.clone(), Paint::color(Token::Url));
            } else {
                let content = i.content_range();
                push(spans, i.range.start..content.start, Paint::color(Token::Punctuation));
                push(spans, content.clone(), Paint::color(Token::LinkText));
                let tail = &text[content.end..i.range.end];
                match (tail.find('('), tail.rfind(')')) {
                    (Some(open), Some(close)) if open < close => {
                        push(
                            spans,
                            content.end..content.end + open + 1,
                            Paint::color(Token::Punctuation),
                        );
                        push(
                            spans,
                            content.end + open + 1..content.end + close,
                            Paint::color(Token::Url),
                        );
                        push(
                            spans,
                            content.end + close..i.range.end,
                            Paint::color(Token::Punctuation),
                        );
                    }
                    _ => push(spans, content.end..i.range.end, Paint::color(Token::Punctuation)),
                }
            }
        }
        InlineKind::Text(t) => {
            let src = &text[i.range.clone()];
            if src.len() != t.len() {
                let mut prev_backslash = false;
                for (pos, c) in src.char_indices() {
                    if c == '\\' && !prev_backslash {
                        push(
                            spans,
                            i.range.start + pos..i.range.start + pos + 1,
                            Paint::color(Token::Punctuation),
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

fn flatten(spans: Vec<(Range<usize>, Paint)>) -> Vec<(Range<usize>, Style)> {
    let mut events: Vec<(usize, bool, usize)> = Vec::with_capacity(spans.len() * 2);
    for (idx, (r, _)) in spans.iter().enumerate() {
        events.push((r.start, true, idx));
        events.push((r.end, false, idx));
    }
    events.sort_by_key(|&(pos, is_start, idx)| (pos, is_start, idx));
    let mut active: Vec<usize> = Vec::new();
    let mut out: Vec<(Range<usize>, Style)> = Vec::new();
    let mut last = 0;
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
    old: &[(Range<usize>, Style)],
    text: &str,
    spans: &[(Range<usize>, Style)],
) -> Range<usize> {
    let mut prefix = common_prefix(old_text.as_bytes(), text.as_bytes());
    while !(old_text.is_char_boundary(prefix) && text.is_char_boundary(prefix)) {
        prefix -= 1;
    }
    let max_suffix = old_text.len().min(text.len()) - prefix;
    let mut suffix = common_suffix(old_text.as_bytes(), text.as_bytes(), max_suffix);
    while !(old_text.is_char_boundary(old_text.len() - suffix) && text.is_char_boundary(text.len() - suffix)) {
        suffix -= 1;
    }
    let old_edit_end = old_text.len() - suffix;
    let edit_end = text.len() - suffix;
    let shift = |o: usize| {
        if o <= prefix {
            o
        } else if o >= old_edit_end {
            o - old_edit_end + edit_end
        } else {
            edit_end
        }
    };
    let lead = old
        .iter()
        .zip(spans)
        .take_while(|(a, b)| a == b && a.0.end <= prefix)
        .count();
    let rest = old.len().min(spans.len()) - lead;
    let trail = old
        .iter()
        .rev()
        .zip(spans.iter().rev())
        .take(rest)
        .take_while(|(a, b)| a.1 == b.1 && a.0.start >= old_edit_end && (shift(a.0.start)..shift(a.0.end)) == b.0)
        .count();
    let changed_new = &spans[lead..spans.len() - trail];
    let changed_old = &old[lead..old.len() - trail];
    let start = changed_new
        .iter()
        .map(|s| s.0.start)
        .chain(changed_old.iter().map(|s| s.0.start.min(prefix)))
        .fold(prefix, usize::min);
    let end = changed_new
        .iter()
        .map(|s| s.0.end)
        .chain(changed_old.iter().map(|s| shift(s.0.end)))
        .fold(edit_end, usize::max)
        .min(text.len());
    start..end.max(start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::doc::parse;

    fn styled(t: &str) -> Vec<(&str, Style)> {
        let d = parse(t);
        highlight(t, &d).into_iter().map(|(r, s)| (&t[r], s)).collect()
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
        let r = changed_region(a, &highlight(a, &parse(a)), b, &highlight(b, &parse(b)));
        assert_eq!(r, 7..15);
        let c = "Text **fetter** und\n\n```\ncode\n```\nnach\n";
        let r = changed_region(b, &highlight(b, &parse(b)), c, &highlight(c, &parse(c)));
        assert!(r.start >= 21 && r.start <= b.len() && r.end == c.len(), "{r:?}");
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
}
