//! A line of spans as Markdown. Marks become Splat's own markers, except where CommonMark would not read `**` as emphasis
//! in that place, as in `a**"b"**`: there, the same mark is written as the HTML tag Splat reads as well.

use std::fmt::Write;

use super::escape::{Counts, Where, destination, escape_text};
use super::{Image, Mark, Piece, Source, Span};

/// What a line of text is part of.
#[derive(Clone, Copy, Default)]
pub(super) struct Context {
    pub(super) table: bool,
    pub(super) heading: bool,
}

impl Context {
    /// A table row or a heading has to stay on its line.
    fn one_line(self) -> bool {
        self.table || self.heading
    }
}

/// A mark or a link around spans.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind<'a> {
    Mark(Mark),
    Link(&'a str),
}

/// The marks that come together, with the link last so that its text can be marked; this is the order `Mark::ALL` gives.
#[derive(Clone, Copy)]
enum Token<'a> {
    Open(Kind<'a>),
    Close(Kind<'a>),
    Leaf(&'a Span, bool),
}

/// `spans` in the order they are written, with their marks around them. What opens first ends last, so that a mark
/// starting inside another and ending behind it is written in two parts.
fn tokens(spans: &[Span]) -> Vec<Token<'_>> {
    let ends = run_ends(spans);
    let mut out = Vec::with_capacity(spans.len() * 2);
    let mut open: Vec<(Kind, usize)> = Vec::new();
    let mut in_link = 0usize;
    for (at, span) in spans.iter().enumerate() {
        while open.last().is_some_and(|(_, end)| *end <= at) {
            if let Some((kind, _)) = open.pop() {
                in_link -= usize::from(matches!(kind, Kind::Link(_)));
                out.push(Token::Close(kind));
            }
        }
        let mut new: Vec<(Kind, usize)> = Vec::new();
        for (slot, mark) in Mark::ALL.into_iter().enumerate() {
            if span.marks.has(mark) && !open.iter().any(|(kind, _)| *kind == Kind::Mark(mark)) {
                new.push((Kind::Mark(mark), ends[at][slot]));
            }
        }
        if let Some(link) = &span.link
            && !open.iter().any(|(kind, _)| matches!(kind, Kind::Link(_)))
        {
            new.push((Kind::Link(link), ends[at][Mark::ALL.len()]));
        }
        new.sort_by_key(|(_, end)| std::cmp::Reverse(*end));
        for (kind, end) in new {
            let end = open.last().map_or(end, |(_, around)| end.min(*around));
            in_link += usize::from(matches!(kind, Kind::Link(_)));
            out.push(Token::Open(kind));
            open.push((kind, end));
        }
        out.push(Token::Leaf(span, in_link > 0));
    }
    while let Some((kind, _)) = open.pop() {
        out.push(Token::Close(kind));
    }
    out
}

/// For each span and each mark and the link: behind which span they stop going on.
fn run_ends(spans: &[Span]) -> Vec<[usize; Mark::ALL.len() + 1]> {
    let mut ends = vec![[0; Mark::ALL.len() + 1]; spans.len()];
    for at in (0..spans.len()).rev() {
        for (slot, mark) in Mark::ALL.into_iter().enumerate() {
            ends[at][slot] = if at + 1 < spans.len() && spans[at + 1].marks.has(mark) {
                ends[at + 1][slot]
            } else {
                at + 1
            };
        }
        ends[at][Mark::ALL.len()] = if at + 1 < spans.len() && spans[at + 1].link == spans[at].link {
            ends[at + 1][Mark::ALL.len()]
        } else {
            at + 1
        };
    }
    ends
}

fn marker(mark: Mark, closing: bool, html: bool) -> &'static str {
    match (mark, closing, html) {
        (Mark::Bold, _, false) => "**",
        (Mark::Italic, _, false) => "*",
        (Mark::Strike, _, false) => "~~",
        (Mark::Bold, false, true) => "<strong>",
        (Mark::Bold, true, true) => "</strong>",
        (Mark::Italic, false, true) => "<em>",
        (Mark::Italic, true, true) => "</em>",
        (Mark::Strike, false, true) => "<del>",
        (Mark::Strike, true, true) => "</del>",
        (Mark::Underline, false, _) => "<ins>",
        (Mark::Underline, true, _) => "</ins>",
        (Mark::Highlight, false, _) => "<mark>",
        (Mark::Highlight, true, _) => "</mark>",
        (Mark::Superscript, false, _) => "<sup>",
        (Mark::Superscript, true, _) => "</sup>",
        (Mark::Subscript, false, _) => "<sub>",
        (Mark::Subscript, true, _) => "</sub>",
    }
}

/// The first and last character a span is written with, as far as emphasis can tell them apart.
fn leaf_edges(span: &Span, one_line: bool) -> (char, char) {
    match &span.piece {
        Piece::Code(text) if one_line && text.contains("\\|") => text_edges(text),
        Piece::Text(text) => text_edges(text),
        Piece::Code(_) => ('`', '`'),
        Piece::Image(_) => ('!', ')'),
        Piece::Break if one_line => ('<', '>'),
        Piece::Break => ('\\', '\n'),
        Piece::Note(_) => ('[', ']'),
        Piece::Math(_) => ('$', '$'),
    }
}

fn text_edges(text: &str) -> (char, char) {
    (
        text.chars().next().unwrap_or(' '),
        text.chars().next_back().unwrap_or(' '),
    )
}

/// Stands for what comrak looks past when it decides whether emphasis can open or close, so that it is not known.
const LOOKED_PAST: char = '\u{1}';

/// Whether comrak could read a run of `*` meant to open as the end of emphasis that is open already.
fn could_close(before: char, after: char) -> bool {
    !before.is_whitespace() && !(before.is_ascii_punctuation() && after.is_alphanumeric())
}

/// Whether `**` can open where it stands, by what stands around it. Whether a character outside ASCII counts as
/// punctuation is left open by treating it as what `**` cannot use.
fn can_open(before: char, after: char) -> bool {
    before != LOOKED_PAST
        && after != LOOKED_PAST
        && !after.is_whitespace()
        && (after.is_alphanumeric() || before.is_whitespace() || before.is_ascii_punctuation())
}

fn can_close(before: char, after: char) -> bool {
    before != LOOKED_PAST
        && after != LOOKED_PAST
        && !before.is_whitespace()
        && (before.is_alphanumeric() || after.is_whitespace() || after.is_ascii_punctuation())
}

/// How many times the choice is looked at again before all emphasis of a line goes to HTML, as one mark written as a tag
/// can take the place of another away, which would otherwise take quadratic time on a text made for it.
const MOST_ROUNDS: usize = 4;

/// Which tokens are written as HTML tags.
fn choose_tags(tokens: &[Token], one_line: bool) -> Vec<bool> {
    let partners = partners(tokens);
    let mut tags = vec![false; tokens.len()];
    for _ in 0..MOST_ROUNDS {
        if !look_at_markers(tokens, &partners, &mut tags, one_line) {
            return tags;
        }
    }
    tokens
        .iter()
        .map(|t| matches!(t, Token::Open(_) | Token::Close(_)))
        .collect()
}

/// For each marker, where the other end of its pair stands.
fn partners(tokens: &[Token]) -> Vec<usize> {
    let mut partners = vec![0; tokens.len()];
    let mut open = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        match token {
            Token::Open(_) => open.push(at),
            Token::Close(_) => {
                if let Some(start) = open.pop() {
                    partners[at] = start;
                    partners[start] = at;
                }
            }
            Token::Leaf(..) => {}
        }
    }
    partners
}

fn delimiter(token: &Token, tag: bool) -> Option<char> {
    match token {
        Token::Open(Kind::Mark(Mark::Bold | Mark::Italic)) | Token::Close(Kind::Mark(Mark::Bold | Mark::Italic))
            if !tag =>
        {
            Some('*')
        }
        Token::Open(Kind::Mark(Mark::Strike)) | Token::Close(Kind::Mark(Mark::Strike)) if !tag => Some('~'),
        _ => None,
    }
}

/// The last character a token is written with, or its first.
fn edge_char(token: &Token, tag: bool, last: bool, one_line: bool) -> char {
    match token {
        Token::Leaf(span, _) => {
            let (first, final_char) = leaf_edges(span, one_line);
            if last { final_char } else { first }
        }
        Token::Open(Kind::Link(_)) => '[',
        Token::Close(Kind::Link(_)) => {
            if last {
                ')'
            } else {
                ']'
            }
        }
        Token::Open(Kind::Mark(mark)) => marker_char(marker(*mark, false, tag), last),
        Token::Close(Kind::Mark(mark)) => marker_char(marker(*mark, true, tag), last),
    }
}

fn marker_char(marker: &str, last: bool) -> char {
    let picked = if last {
        marker.chars().next_back()
    } else {
        marker.chars().next()
    };
    picked.unwrap_or('*')
}

/// What emphasis sees on one side, walking over the `~~` comrak skips; `from` is the first token to look at.
fn seen_from(tokens: &[Token], tags: &[bool], from: Option<usize>, backwards: bool, one_line: bool) -> char {
    let mut at = from;
    while let Some(n) = at.filter(|n| *n < tokens.len()) {
        if delimiter(&tokens[n], tags[n]) != Some('~') {
            let seen = edge_char(&tokens[n], tags[n], backwards, one_line);
            return if matches!(seen, '~' | '=') { LOOKED_PAST } else { seen };
        }
        at = if backwards { n.checked_sub(1) } else { Some(n + 1) };
    }
    '\n'
}

/// Moves markers that cannot stay what they are to tags, and returns whether it found any.
fn look_at_markers(tokens: &[Token], partners: &[usize], tags: &mut [bool], one_line: bool) -> bool {
    let mut changed = false;
    let mut at = 0;
    let mut stars_open = 0usize;
    while at < tokens.len() {
        let Some(delim) = delimiter(&tokens[at], tags[at]) else {
            at += 1;
            continue;
        };
        let end = at
            + tokens[at..]
                .iter()
                .zip(&tags[at..])
                .take_while(|(token, tag)| delimiter(token, **tag) == Some(delim))
                .count();
        let before = seen_from(tokens, tags, at.checked_sub(1), true, one_line);
        let after = seen_from(tokens, tags, Some(end), false, one_line);
        let mut closed = false;
        let stars_around = stars_open > 0;
        for n in at..end {
            let fine = match tokens[n] {
                // comrak pairs a run that both closes and opens by the lengths involved, not by what was meant.
                Token::Open(_) => {
                    stars_open += usize::from(delim == '*');
                    !closed && can_open(before, after) && !(delim == '*' && stars_around && could_close(before, after))
                }
                _ => {
                    closed = true;
                    stars_open = stars_open.saturating_sub(usize::from(delim == '*'));
                    can_close(before, after)
                }
            };
            if !fine {
                tags[n] = true;
                tags[partners[n]] = true;
                changed = true;
            }
        }
        at = end;
    }
    changed
}

fn count_pairs(tokens: &[Token], counts: &mut Counts, one_line: bool) {
    let mut starts_line = true;
    for (at, token) in tokens.iter().enumerate() {
        match token {
            Token::Leaf(span, _) => match &span.piece {
                Piece::Text(text) => counts.add(text, starts_line, at + 1 == tokens.len()),
                Piece::Code(text) | Piece::Note(text) => {
                    counts.add_literal(text);
                    if text.contains("\\|") {
                        counts.add(text, false, false);
                    }
                }
                Piece::Image(Image {
                    source: Source::Address(address),
                    alt,
                }) => {
                    counts.add_literal(address);
                    counts.add_literal(alt);
                    counts.add(alt, true, true);
                }
                Piece::Math(_) => counts.add_literal("$$"),
                Piece::Image(_) | Piece::Break => {}
            },
            Token::Close(Kind::Link(url)) => counts.add_literal(url),
            Token::Open(Kind::Mark(Mark::Strike)) => counts.tilde += 2,
            Token::Open(_) | Token::Close(_) => {}
        }
        starts_line = matches!(
            token,
            Token::Leaf(
                Span {
                    piece: Piece::Break,
                    ..
                },
                _
            )
        ) && !one_line;
    }
}

/// `spans`, which `Rich::tidied` made, as Markdown.
pub(super) fn write_line(spans: &[Span], context: Context, out: &mut String) {
    let tokens = tokens(spans);
    let tags = choose_tags(&tokens, context.one_line());
    let mut counts = Counts::default();
    count_pairs(&tokens, &mut counts, context.one_line());
    let mut line_start = !context.heading && !context.table;
    let mut prev = '\n';
    for (at, token) in tokens.iter().enumerate() {
        match token {
            Token::Open(Kind::Mark(mark)) => out.push_str(marker(*mark, false, tags[at])),
            Token::Close(Kind::Mark(mark)) => out.push_str(marker(*mark, true, tags[at])),
            Token::Open(Kind::Link(_)) => out.push('['),
            Token::Close(Kind::Link(url)) => {
                let _ = write!(out, "]({})", destination(url, context.table));
            }
            Token::Leaf(span, in_link) => {
                let after = tokens
                    .get(at + 1)
                    .map_or('\n', |t| edge_char(t, tags[at + 1], false, context.one_line()));
                let within = Where {
                    table: context.table,
                    heading: context.heading,
                    in_brackets: *in_link,
                    line_start,
                };
                write_leaf(span, prev, after, within, counts, out);
            }
        }
        line_start = matches!(
            token,
            Token::Leaf(
                Span {
                    piece: Piece::Break,
                    ..
                },
                _
            )
        ) && !context.one_line();
        prev = out.chars().next_back().unwrap_or('\n');
    }
}

fn write_leaf(span: &Span, before: char, after: char, within: Where, counts: Counts, out: &mut String) {
    match &span.piece {
        Piece::Text(text) => escape_text(text, before, after, within, counts, out),
        // comrak keeps a backslash in front of a `|` in a cell, or ends the cell there; this can only be written as text.
        Piece::Code(code) if within.table && code.contains("\\|") => {
            escape_text(code, before, after, within, counts, out);
        }
        Piece::Code(code) => write_code(code, within.table, out),
        Piece::Image(image) => {
            if let Source::Address(address) = &image.source {
                out.push_str("![");
                let alt = Where {
                    in_brackets: true,
                    line_start: false,
                    ..within
                };
                escape_text(&image.alt, '[', ']', alt, counts, out);
                let _ = write!(out, "]({})", destination(address, within.table));
            }
        }
        Piece::Break if within.table || within.heading => out.push_str("<br>"),
        Piece::Break => out.push_str("\\\n"),
        Piece::Note(name) => {
            let _ = write!(out, "[^{name}]");
        }
        Piece::Math(tex) => write_math(tex, after, within.table, out),
    }
}

/// A code span with a fence no longer than needed; a space on each side keeps backticks at its edges apart from it, and
/// comrak takes those spaces away again.
pub(super) fn write_code(code: &str, table: bool, out: &mut String) {
    let longest = longest_run(code, '`');
    let fence = "`".repeat(longest + 1);
    let pad = if code.starts_with('`') || code.ends_with('`') {
        " "
    } else {
        ""
    };
    out.push_str(&fence);
    out.push_str(pad);
    if table {
        out.push_str(&code.replace('|', "\\|"));
    } else {
        out.push_str(code);
    }
    out.push_str(pad);
    out.push_str(&fence);
}

pub(super) fn longest_run(text: &str, of: char) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for c in text.chars() {
        current = if c == of { current + 1 } else { 0 };
        longest = longest.max(current);
    }
    longest
}

/// `$tex$`, or `` $`tex`$ `` where dollars would not read as its ends: a dollar inside, or a digit behind it.
fn write_math(tex: &str, after: char, table: bool, out: &mut String) {
    let tex_in_cell;
    let written = if table {
        tex_in_cell = tex.replace("\\|", "\\Vert ").replace('|', "\\|");
        &tex_in_cell
    } else {
        tex
    };
    let lone_dollar = tex.char_indices().any(|(at, c)| c == '$' && !tex[..at].ends_with('\\'));
    if !lone_dollar && !after.is_ascii_digit() && !tex.starts_with('`') && !tex.ends_with('\\') {
        let _ = write!(out, "${written}$");
    } else if !tex.contains("`$") {
        let _ = write!(out, "$`{written}`$");
    } else {
        write_code(&format!("${tex}$"), table, out);
    }
}
