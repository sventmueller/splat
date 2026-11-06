//! comrak's inline positions go wrong in a few places: behind a link reference definition at the start of a paragraph,
//! and behind a link or footnote over several lines in a quote or list. An edit made from such a position lands
//! somewhere else in the text, so the positions of every leaf are checked against the source. Where they do not fit
//! they are read from the source again, in the order of the tree; what cannot be found there is given up.

use std::ops::Range;

use crate::doc::{Inline, InlineKind};
use crate::text::{Byte, TextAt, decode_entity, offsets};

/// Inlines of a leaf after the check: `None` if their positions could not be told, so the caller keeps them from being
/// edited by showing the source as it stands.
pub(super) fn checked(text: &str, block: &Range<Byte>, mut inlines: Vec<Inline>) -> Option<Vec<Inline>> {
    if inlines.iter().all(|i| plausible(text, i)) {
        return Some(inlines);
    }
    let mut walker = Walker {
        text,
        at: block.start,
        end: block.end.min(text.end()),
        steps: MAX_STEPS,
    };
    inlines.iter_mut().try_for_each(|i| walker.place(i))?;
    Some(inlines)
}

/// Characters looked at to read one leaf again; a text that takes more is not worth it.
const MAX_STEPS: usize = 1 << 20;

/// Whether the source at an inline's position looks like that inline, and likewise for what it holds. This has to say
/// yes wherever comrak is right, and it is cheap enough to ask of every inline.
fn plausible(text: &str, i: &Inline) -> bool {
    let Some(source) = text.get(offsets(&i.range)) else {
        return false;
    };
    let spelled = match &i.kind {
        InlineKind::Text(literal) => literal.chars().next().is_none_or(|first| {
            source.starts_with(first)
                || source.starts_with(['\\', '&', '\0'])
                || (first.is_whitespace() && source.starts_with(char::is_whitespace))
        }),
        InlineKind::SoftBreak | InlineKind::LineBreak => {
            source.chars().all(|c| c.is_whitespace() || c == '\\') || source.starts_with('<')
        }
        InlineKind::Code(_) => source.starts_with('`') && source.ends_with('`'),
        InlineKind::Link { .. } => match source.as_bytes().first() {
            Some(b'[') => source.ends_with([')', ']']),
            Some(b'<') => source.ends_with('>'),
            Some(_) => true,
            None => false,
        },
        InlineKind::Image { .. } => source.starts_with("![") && source.ends_with([')', ']']),
        InlineKind::FootnoteRef { .. } => {
            source.ends_with(']') && (source.starts_with("[^") || source.starts_with("^["))
        }
        InlineKind::Html(_) | InlineKind::HtmlImage { .. } => source.starts_with('<'),
        InlineKind::Math { .. } => source.starts_with(['$', '\\', '`']),
        InlineKind::Emoji(_) => source.starts_with(':'),
        InlineKind::WikiLink { .. } => {
            (source.starts_with("[[") || source.starts_with("![[")) && source.ends_with("]]")
        }
        kind => markers(kind).is_none_or(|markers| {
            source.starts_with('<') || markers.iter().any(|m| source.starts_with(m) && source.ends_with(m))
        }),
    };
    spelled && i.children.iter().all(|c| plausible(text, c))
}

/// The markers around a kind of container, longest first.
fn markers(kind: &InlineKind) -> Option<&'static [&'static str]> {
    Some(match kind {
        InlineKind::Strong => &["**", "__"],
        InlineKind::Emph => &["*", "_"],
        InlineKind::Strikethrough => &["~~", "~"],
        InlineKind::Highlight => &["=="],
        InlineKind::Superscript => &["^"],
        InlineKind::Subscript => &["~"],
        InlineKind::Spoiler => &["||"],
        _ => return None,
    })
}

/// Reads positions from the source, in the order of the tree. Inlines follow each other there, with only the prefixes of
/// quotes and lists, or a link reference definition, between them.
struct Walker<'t> {
    text: &'t str,
    at: Byte,
    end: Byte,
    steps: usize,
}

impl<'t> Walker<'t> {
    fn rest(&self) -> &'t str {
        self.text.get(offsets(&(self.at..self.end))).unwrap_or("")
    }

    /// The first place from here on where `matches` finds what it wants, and what it found there.
    fn search<T>(&mut self, matches: impl Fn(&str) -> Option<T>) -> Option<(Byte, T)> {
        let rest = self.rest();
        let at = self.at;
        for (i, _) in rest.char_indices() {
            self.steps = self.steps.checked_sub(1)?;
            if let Some(found) = matches(&rest[i..]) {
                return Some((at + i, found));
            }
        }
        None
    }

    fn place(&mut self, i: &mut Inline) -> Option<()> {
        let start;
        match &i.kind {
            InlineKind::Text(literal) if literal.is_empty() => start = self.at,
            InlineKind::Text(literal) => {
                let (s, len) = self.search(|rest| spelled_len(literal, rest))?;
                start = s;
                self.at = s + len;
            }
            InlineKind::SoftBreak | InlineKind::LineBreak => {
                start = self.at;
                self.at += line_break_len(self.rest())?;
            }
            InlineKind::Code(_) => {
                let (s, len) = self.search(code_len)?;
                start = s;
                self.at = s + len;
            }
            InlineKind::Html(_) | InlineKind::HtmlImage { .. } => {
                let (s, len) = self.search(|rest| Some(rest.strip_prefix('<')?.find('>')? + 2))?;
                start = s;
                self.at = s + len;
            }
            InlineKind::FootnoteRef { .. } => {
                let (s, len) = self.search(footnote_len)?;
                start = s;
                self.at = s + len;
            }
            InlineKind::Math { literal, .. } => {
                let (s, len) = self.search(|rest| math_len(literal, rest))?;
                start = s;
                self.at = s + len;
            }
            InlineKind::Emoji(_) => {
                let (s, len) = self.search(|rest| Some(rest.strip_prefix(':')?.find(':')? + 2))?;
                start = s;
                self.at = s + len;
            }
            InlineKind::WikiLink { .. } => {
                let (s, len) = self.search(|rest| {
                    (rest.starts_with("[[") || rest.starts_with("![[")).then_some(())?;
                    Some(rest.find("]]")? + 2)
                })?;
                start = s;
                // Its children, the title or else the target, stand behind any `|`.
                let inner = self.text.at(s..s + len - 2);
                self.at = s + inner
                    .rfind('|')
                    .map_or_else(|| inner.find('[').map_or(0, |o| o + 2), |bar| bar + 1);
                i.children.iter_mut().try_for_each(|c| self.place(c))?;
                self.at = s + len;
            }
            InlineKind::Comment => {
                let (s, len) = self.search(comment_len)?;
                start = s;
                self.at = s + len;
            }
            _ => return self.place_container(i),
        }
        i.range = start..self.at;
        Some(())
    }

    fn place_container(&mut self, i: &mut Inline) -> Option<()> {
        let image = matches!(i.kind, InlineKind::Image { .. });
        let link = matches!(i.kind, InlineKind::Link { .. });
        let markers = markers(&i.kind);
        let (start, open) = self.search(|rest| {
            if link {
                return opening_of_link(rest);
            }
            if rest.starts_with('<') {
                return Some(rest.find('>')? + 1);
            }
            if image {
                return rest.starts_with("![").then_some(2);
            }
            markers?.iter().find(|m| rest.starts_with(**m)).map(|m| m.len())
        })?;
        let opening = self.text.at(start..start + open);
        self.at = start + open;
        i.children.iter_mut().try_for_each(|c| self.place(c))?;
        let rest = self.rest();
        let close = if opening == "<" {
            rest.starts_with('>').then_some(1)?
        } else if opening.starts_with('<') {
            rest.starts_with("</")
                .then(|| rest.find('>').map(|e| e + 1))
                .flatten()?
        } else if image || opening == "[" {
            link_close_len(rest)?
        } else if opening.is_empty() {
            0
        } else {
            rest.starts_with(opening).then_some(opening.len())?
        };
        // An address that is its own link has nothing around its text, which may stand behind a line's prefix.
        let start = if opening.is_empty() {
            i.children.first().map_or(start, |c| c.range.start)
        } else {
            start
        };
        self.at += close;
        i.range = start..self.at;
        Some(())
    }
}

/// What opens a link at the start of `rest`: a tag `<a …>`, the `<` of an address in angle brackets, a `[`, or nothing
/// for an address that is its own link. `None` for the prefix of a line in front of it.
fn opening_of_link(rest: &str) -> Option<usize> {
    let is_tag = rest.starts_with("<a") && rest[2..].starts_with([' ', '\t', '\n', '\r', '>']);
    if is_tag {
        Some(rest.find('>')? + 1)
    } else if rest.starts_with(['[', '<']) {
        Some(1)
    } else if rest.starts_with([' ', '\t', '>']) {
        None
    } else {
        Some(0)
    }
}

/// How many bytes of `source` spell `literal`, past what only the source has: escapes and character references.
fn spelled_len(literal: &str, source: &str) -> Option<usize> {
    let mut used = 0;
    let mut shown = literal;
    while let Some(c) = shown.chars().next() {
        let rest = &source[used..];
        if let Some((decoded, len)) = decode_entity(rest).filter(|(_, len)| !shown.starts_with(&rest[..*len])) {
            shown = shown.strip_prefix(&*decoded)?;
            used += len;
        } else if rest.starts_with(c) {
            used += c.len_utf8();
            shown = &shown[c.len_utf8()..];
        } else if c == '\u{FFFD}' && rest.starts_with('\0') {
            used += 1;
            shown = &shown[c.len_utf8()..];
        } else if c.is_ascii_punctuation() && rest.strip_prefix('\\').is_some_and(|r| r.starts_with(c)) {
            used += 1 + c.len_utf8();
            shown = &shown[c.len_utf8()..];
        } else {
            return None;
        }
    }
    Some(used)
}

/// A line end with what stands before it, as far as it belongs to the break: spaces, a backslash or a `<br>`.
fn line_break_len(rest: &str) -> Option<usize> {
    if rest.starts_with('<') {
        return Some(rest.find('>')? + 1);
    }
    let before = rest.len() - rest.trim_start_matches([' ', '\t', '\\']).len();
    let after = &rest[before..];
    let end = if after.starts_with("\r\n") {
        2
    } else if after.starts_with(['\n', '\r']) {
        1
    } else {
        return None;
    };
    Some(before + end)
}

fn code_len(rest: &str) -> Option<usize> {
    let ticks = rest.len() - rest.trim_start_matches('`').len();
    if ticks == 0 {
        return None;
    }
    let mut at = ticks;
    while let Some(next) = rest[at..].find('`') {
        let from = at + next;
        let run = rest[from..].len() - rest[from..].trim_start_matches('`').len();
        if run == ticks {
            return Some(from + run);
        }
        at = from + run;
    }
    None
}

fn footnote_len(rest: &str) -> Option<usize> {
    if rest.starts_with("[^") {
        return Some(rest.find(']')? + 1);
    }
    inline_note_len(rest)
}

/// How long the inline note `^[…]` at the start of `rest` is, found by its brackets: comrak's end position is unreliable.
pub(super) fn inline_note_len(rest: &str) -> Option<usize> {
    let inner = rest.strip_prefix("^[")?;
    let mut depth = 1;
    let mut chars = inner.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(2 + i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// A formula, whose delimiters are as many `$` on both sides, or `` $`…`$ ``, `\(…\)` and `\[…\]`.
fn math_len(literal: &str, rest: &str) -> Option<usize> {
    let close = if rest.starts_with("$`") {
        "`$"
    } else if rest.starts_with("$$") {
        "$$"
    } else if rest.starts_with('$') {
        "$"
    } else if rest.starts_with("\\(") {
        "\\)"
    } else if rest.starts_with("\\[") {
        "\\]"
    } else {
        return None;
    };
    let open = if close == "`$" { 2 } else { close.len() };
    let content = rest[open..]
        .starts_with(literal)
        .then_some(open + literal.len())
        .or_else(|| rest[open..].find(close).map(|at| open + at))?;
    rest[content..].starts_with(close).then_some(content + close.len())
}

fn comment_len(rest: &str) -> Option<usize> {
    if let Some(inner) = rest.strip_prefix("%%") {
        return Some(inner.find("%%")? + 4);
    }
    if rest.starts_with("<!--") {
        return Some(rest.find("-->")? + 3);
    }
    None
}

/// What closes a link or image behind its text: `](destination "title")`, `][label]` or a lone `]`.
fn link_close_len(rest: &str) -> Option<usize> {
    let behind = rest.strip_prefix(']')?;
    if behind.starts_with('[') {
        return Some(1 + behind.find(']')? + 1);
    }
    if !behind.starts_with('(') {
        return Some(1);
    }
    let mut depth = 0;
    let mut title: Option<char> = None;
    let mut before = '(';
    let mut chars = behind.char_indices();
    while let Some((i, c)) = chars.next() {
        match (title, c) {
            (_, '\\') => {
                chars.next();
            }
            (Some(q), _) if c == q => title = None,
            (Some(_), _) => {}
            (None, '"' | '\'') if before.is_whitespace() => title = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => {
                depth -= 1;
                if depth == 0 {
                    return Some(1 + i + 1);
                }
            }
            _ => {}
        }
        before = c;
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::doc::{Block, Inline, InlineKind, walk_blocks, walk_inlines};
    use crate::reader::parse;
    use crate::text::{Byte, TextAt, bytes};

    /// Every text of the document as it stands in the source, in order.
    fn texts(source: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        walk_blocks(&parse(source).blocks, &mut |b: &Block| {
            walk_inlines(b.inlines(), &mut |i: &Inline| {
                if let InlineKind::Text(t) = &i.kind {
                    out.push((t.to_string(), source.at(i.range.clone()).to_string()));
                }
            });
        });
        out
    }

    fn stand_where_they_say(source: &str, expected: &[&str]) {
        let found = texts(source);
        for (literal, spelled) in &found {
            assert_eq!(literal, spelled, "in {source:?}");
        }
        let literals: Vec<&str> = found.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(literals, expected, "in {source:?}");
    }

    #[test]
    fn a_link_behind_the_prefix_of_a_line_keeps_its_brackets() {
        for source in [
            "a\r\n [t](u) b\r\n",
            "[x](y) a\r\n  [t](u) b\r\n",
            "> a\r\n>  [t](u) b\r\n",
        ] {
            let mut links = Vec::new();
            walk_blocks(&parse(source).blocks, &mut |b: &Block| {
                walk_inlines(b.inlines(), &mut |i: &Inline| {
                    if matches!(i.kind, InlineKind::Link { .. }) {
                        links.push(source.at(i.range.clone()).to_string());
                    }
                });
            });
            assert_eq!(links.last().map(String::as_str), Some("[t](u)"), "{source:?}");
        }
    }

    #[test]
    fn texts_behind_a_link_definition_find_their_place() {
        stand_where_they_say("[a]: b\nfoo **bar** baz\n", &["foo ", "bar", " baz"]);
        stand_where_they_say("> [a]: b\n> foo **bar** baz\n", &["foo ", "bar", " baz"]);
        stand_where_they_say("[r]: u\nFuß^[a-z]\n", &["Fuß", "a-z"]);
    }

    /// Every inline with its range and its children's, moved by `by`.
    fn ranges(inlines: &[Inline], by: usize) -> Vec<(String, std::ops::Range<Byte>)> {
        let mut out = Vec::new();
        for i in inlines {
            let name = format!("{:?}", i.kind);
            out.push((name, i.range.start + by..i.range.end + by));
            out.extend(ranges(&i.children, by));
        }
        out
    }

    /// comrak is right about these without the definition in front, so that is what they must be with it too.
    #[test]
    fn every_kind_of_inline_finds_its_place_behind_a_definition() {
        for body in [
            "foo *e* _i_ **s** __b__ baz",
            "a ~~x~~ ~y~ ==h== ^u^ ||sp|| z",
            "a `c` ``d`e`` z",
            "a <b>t</b> <u>v</u> <kbd>k</kbd> z",
            "a <img src=\"p.png\" alt=\"q\" width=\"3\"> <span>s</span> z",
            "a $m$ $$n$$ $`k`$ \\(l\\) \\[o\\] z",
            "a :smile: :rocket: z",
            "a [[Seite]] [[Seite|Titel]] ![[bild.png]] z",
            "a %%z%% <!-- y --> z",
            "a [l](u) [m](u \"t\") [n][x] [x] ![i](p) ![j](p \"t\") z",
            "a <https://a.de> https://b.de www.c.de z",
            "a[^1] b^[n] c^[n [m] o] z",
            "a\nb  \nc\\\nd<br>e",
            "a \\* b &amp; c &eacute; d",
            "***a*** **_b_** *`c`* [**d**](e) z",
            "\n".trim_start(),
        ] {
            if body.is_empty() {
                continue;
            }
            let definition = "[x]: y\n";
            let alone = parse(&format!("{body}\n\n{definition}\n[^1]: note\n"));
            let behind = parse(&format!("{definition}{body}\n\n[^1]: note\n"));
            let expected = ranges(alone.blocks[0].inlines(), definition.len());
            assert_eq!(ranges(behind.blocks[0].inlines(), 0), expected, "in {body:?}");
        }
    }

    #[test]
    fn inside_a_quote_a_definition_counts_its_prefix_as_well() {
        let alone = parse("> foo `c` <b>t</b> z\n");
        let behind = parse("> [x]: y\n> foo `c` <b>t</b> z\n");
        let block = |d: &crate::doc::Doc| d.blocks[0].children()[0].clone();
        assert_eq!(
            ranges(block(&behind).inlines(), 0),
            ranges(block(&alone).inlines(), "[x]: y\n> ".len()),
        );
    }

    #[test]
    fn delimiters_of_a_formula_or_a_code_span_are_read_from_the_source() {
        use super::{code_len, comment_len, footnote_len, line_break_len, link_close_len, math_len};
        assert_eq!(code_len("`a` b"), Some(3));
        assert_eq!(code_len("``a`b`` c"), Some(7), "a shorter run inside");
        assert_eq!(code_len("`a"), None);
        assert_eq!(code_len("a`"), None);
        assert_eq!(math_len("x", "$x$ y"), Some(3));
        assert_eq!(math_len("x", "$$x$$ y"), Some(5));
        assert_eq!(math_len("x", "$`x`$ y"), Some(5));
        assert_eq!(math_len("x", "\\(x\\) y"), Some(5));
        assert_eq!(math_len("x", "\\[x\\] y"), Some(5));
        assert_eq!(math_len("other", "$x$ y"), Some(3), "what stands in the source decides");
        assert_eq!(math_len("x", "x$"), None);
        assert_eq!(comment_len("%%a%% b"), Some(5));
        assert_eq!(comment_len("<!-- a --> b"), Some(10));
        assert_eq!(comment_len("%a"), None);
        assert_eq!(footnote_len("[^1] b"), Some(4));
        assert_eq!(footnote_len("^[a] b"), Some(4));
        assert_eq!(footnote_len("^[a [b] \\] c] d"), Some(13), "nested and escaped");
        assert_eq!(footnote_len("^[a"), None);
        assert_eq!(footnote_len("a"), None);
        assert_eq!(line_break_len("\nb"), Some(1));
        assert_eq!(line_break_len("\r\nb"), Some(2));
        assert_eq!(line_break_len("\rb"), Some(1));
        assert_eq!(line_break_len("  \nb"), Some(3));
        assert_eq!(line_break_len("\\\nb"), Some(2));
        assert_eq!(line_break_len("<br>b"), Some(4));
        assert_eq!(line_break_len("b"), None);
        assert_eq!(link_close_len("](u) z"), Some(4));
        assert_eq!(link_close_len("](u(v)w) z"), Some(8), "parentheses in the address");
        assert_eq!(link_close_len("](u \"a)b\") z"), Some(10), "a parenthesis in the title");
        assert_eq!(link_close_len("](it's) z"), Some(7), "an apostrophe in the address");
        assert_eq!(link_close_len("][x] z"), Some(4));
        assert_eq!(link_close_len("] z"), Some(1));
        assert_eq!(link_close_len("](u"), None);
        assert_eq!(link_close_len("z"), None);
    }

    #[test]
    fn a_paragraph_behind_a_link_definition_starts_at_its_text() {
        let source = "[a]: b\nfoo\n";
        assert_eq!(parse(source).blocks[0].range, bytes(7..10));
    }

    #[test]
    fn texts_behind_a_link_over_several_lines_find_their_place() {
        for (source, expected) in [
            ("> [a](b\n> \"c\") **d** e\n", ["a", " ", "d", " e"].as_slice()),
            ("- [a](b\n  \"c\") **d**\n", ["a", " ", "d"].as_slice()),
            ("[a](\n  b) **d**\n", ["a", " ", "d"].as_slice()),
        ] {
            stand_where_they_say(source, expected);
        }
    }

    #[test]
    fn texts_find_their_place_behind_a_note_over_several_lines() {
        stand_where_they_say("> ^[a\n> b] **x** y\n", &[" ", "x", " y", "a\n> b"]);
    }

    #[test]
    fn lines_may_end_in_a_lone_carriage_return() {
        stand_where_they_say(
            "x\r\ry\r\r# H\r\rsome **bold** word\r",
            &["x", "y", "H", "some ", "bold", " word"],
        );
        stand_where_they_say("a\rb\n\n**c** d\n", &["a", "b", "c", " d"]);
    }

    #[test]
    fn a_note_with_a_wide_character_behind_a_definition_does_not_fail() {
        let doc = parse("[a]: b\n^[日本語]\n");
        assert_eq!(doc.blocks.len(), 2, "{:?}", doc.blocks);
    }

    #[test]
    fn what_cannot_be_read_again_is_given_up_and_not_guessed() {
        use super::{Walker, spelled_len};
        assert_eq!(spelled_len("a&b", "a&amp;b"), Some(7));
        assert_eq!(spelled_len("a*b", "a\\*b"), Some(4));
        assert_eq!(spelled_len("ab", "ax"), None);
        let mut walker = Walker {
            text: "nichts",
            at: Byte::new(0),
            end: Byte::new(6),
            steps: 100,
        };
        let mut missing = Inline {
            kind: InlineKind::Strong,
            range: bytes(0..2),
            children: Box::default(),
        };
        assert!(walker.place(&mut missing).is_none());
    }
}
