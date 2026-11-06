//! Where each character of a literal stands in the source that spells it. A literal is what the parser read from the
//! source, or what the preview shows of that, and differs from the source by escapes, character references, the line
//! endings a code span reads as spaces, typographic punctuation and, between its characters, what only the source
//! holds, such as the prefixes of a quote.

use crate::text::decode_entity;
use crate::typography::typographic_source;

/// What a literal is to the source it comes from, which decides what may spell one of its characters there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spelling {
    /// Running text, which comrak reads with backslash escapes and character references resolved.
    Text,
    /// Running text with typographic quotes, dashes and ellipses, as the preview and the exports show it.
    Typographic,
    /// The text of a code span, in which comrak reads every line ending as a space.
    CodeSpan,
    /// A code block or HTML, which comrak keeps as written.
    Verbatim,
}

/// What may stand in the source between two characters of a literal, besides what spells them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gaps {
    /// Nothing: the source spells the literal without a break.
    Closed,
    /// Whitespace on either side, the quote marks in front of the text of a line, and a backslash in front of
    /// punctuation.
    LinePrefixes,
    /// Anything, as the tags between the texts of an HTML block or the marks in front of a title.
    Anything,
}

/// Where one character of a literal stands in the source, and how many bytes it takes there and in the literal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    /// The byte of the source where it starts.
    pub at: usize,
    /// How many bytes of the source spell it; 0 for whitespace the source does not have.
    pub in_source: usize,
    /// How many bytes of the literal it takes: more than one character for a reference that stands for several.
    pub in_literal: usize,
}

/// How many bytes at the start of `source` spell the start of `literal`, and how many bytes of the literal that is;
/// `None` if the source spells something else there.
#[inline]
pub fn spelled(literal: &str, source: &str, spelling: Spelling) -> Option<(usize, usize)> {
    let c = literal.chars().next()?;
    let resolved = matches!(spelling, Spelling::Text | Spelling::Typographic);
    if resolved
        && source.starts_with('&')
        && let Some((decoded, len)) = decode_entity(source)
        // Text that holds a reference as written, as an address that is its own link does, keeps it.
        && !literal.starts_with(&source[..len])
    {
        if literal.starts_with(&*decoded) {
            return Some((len, decoded.len()));
        }
        if spelling == Spelling::Typographic && typographic_source(c, &decoded).is_some() {
            return Some((len, c.len_utf8()));
        }
    }
    // Likewise an escaped backslash: both backslashes of the source spell the one of the literal, unless it holds both.
    if resolved && source.starts_with("\\\\") && literal.starts_with('\\') && !literal.starts_with("\\\\") {
        return Some((2, 1));
    }
    if source.starts_with(c) {
        return Some((c.len_utf8(), c.len_utf8()));
    }
    match spelling {
        Spelling::Text | Spelling::Typographic => {
            let escaped = source.strip_prefix('\\');
            if c.is_ascii_punctuation() && escaped.is_some_and(|rest| rest.starts_with(c)) {
                return Some((2, 1));
            }
            if spelling == Spelling::Text {
                return None;
            }
            if let Some(len) = typographic_source(c, source) {
                return Some((len, c.len_utf8()));
            }
            let len = escaped.and_then(|rest| typographic_source(c, rest))?;
            Some((1 + len, c.len_utf8()))
        }
        Spelling::CodeSpan => (c == ' ' && source.starts_with(['\n', '\r'])).then_some((1, 1)),
        Spelling::Verbatim => None,
    }
}

/// Finds the characters of a literal in its source one after the other.
#[derive(Clone, Debug)]
pub struct Aligner<'t> {
    source: &'t str,
    at: usize,
    spelling: Spelling,
    gaps: Gaps,
    /// Between the start of a source line and its text, where the prefix of a quote stands.
    in_line_prefix: bool,
}

impl<'t> Aligner<'t> {
    /// An aligner at the start of `source`. `line_start` says whether a line begins there, so that the marks of a quote
    /// may follow.
    pub fn new(source: &'t str, spelling: Spelling, gaps: Gaps, line_start: bool) -> Self {
        Aligner {
            source,
            at: 0,
            spelling,
            gaps,
            in_line_prefix: line_start,
        }
    }

    /// The source it reads.
    pub fn source(&self) -> &'t str {
        self.source
    }

    /// Where in the source the next character is looked for.
    pub fn position(&self) -> usize {
        self.at
    }

    /// Where the character `literal` starts with stands, from the current position on and past what the gaps allow; the
    /// aligner moves behind it. `None` if the source does not spell it there.
    #[inline]
    pub fn next(&mut self, literal: &str) -> Option<Step> {
        // Almost every character is ASCII and stands for itself, and only a reference or a backslash has to be looked
        // at before that holds.
        let first = *literal.as_bytes().first()?;
        if first.is_ascii() && first != b'&' && first != b'\\' && self.source.as_bytes().get(self.at) == Some(&first) {
            let step = Step {
                at: self.at,
                in_source: 1,
                in_literal: 1,
            };
            self.in_line_prefix = matches!(first, b'\n' | b'\r');
            self.at += 1;
            return Some(step);
        }
        let c = literal.chars().next()?;
        loop {
            let rest = &self.source[self.at..];
            let Some(found) = rest.chars().next() else {
                // Whitespace at the end of a literal may have no counterpart in its source.
                return (self.gaps == Gaps::LinePrefixes && c.is_whitespace()).then_some(Step {
                    at: self.at,
                    in_source: 0,
                    in_literal: c.len_utf8(),
                });
            };
            if found == c && !matches!(c, '&' | '\\') {
                let len = c.len_utf8();
                let step = Step {
                    at: self.at,
                    in_source: len,
                    in_literal: len,
                };
                self.in_line_prefix = matches!(c, '\n' | '\r');
                self.at += len;
                return Some(step);
            }
            if let Some((in_source, in_literal)) = spelled(literal, rest, self.spelling) {
                let step = Step {
                    at: self.at,
                    in_source,
                    in_literal,
                };
                self.in_line_prefix = matches!(rest.as_bytes()[in_source - 1], b'\n' | b'\r');
                self.at += in_source;
                return Some(step);
            }
            match self.gaps {
                Gaps::Closed => return None,
                Gaps::Anything => self.at += found.len_utf8(),
                Gaps::LinePrefixes => {
                    let escape = found == '\\' && rest[1..].starts_with(|n: char| n.is_ascii_punctuation());
                    if found.is_whitespace() || escape || (self.in_line_prefix && found == '>') {
                        self.in_line_prefix |= matches!(found, '\n' | '\r');
                        self.at += found.len_utf8();
                    } else if c.is_whitespace() {
                        // As the indentation of an HTML block's first line, which stands before its range.
                        return Some(Step {
                            at: self.at,
                            in_source: 0,
                            in_literal: c.len_utf8(),
                        });
                    } else {
                        return None;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[expect(clippy::too_many_lines, reason = "one line per rule")]
    fn every_rule_spells_what_comrak_reads() {
        use Spelling::{CodeSpan, Text, Typographic, Verbatim};
        let all = [Text, Typographic, CodeSpan, Verbatim];
        let resolved = [Text, Typographic];
        for (literal, source, spellings, wanted, rule) in [
            ("a", "a", &all[..], Some((1, 1)), "the same character"),
            ("ä", "ä", &all[..], Some((2, 2)), "the same character, wider"),
            ("a", "b", &all[..], None, "another character"),
            ("", "a", &all[..], None, "nothing left to spell"),
            ("\0", "\0", &all[..], Some((1, 1)), "a NUL, which comrak keeps"),
            ("&", "&amp;", &resolved[..], Some((5, 1)), "a named reference"),
            ("é", "&#233;", &resolved[..], Some((6, 2)), "a decimal reference"),
            ("é", "&#xE9;", &resolved[..], Some((6, 2)), "a hexadecimal reference"),
            (
                "≂\u{338}",
                "&NotEqualTilde;",
                &resolved[..],
                Some((15, 5)),
                "two characters",
            ),
            ("\u{FFFD}", "&#0;", &resolved[..], Some((4, 3)), "an impossible number"),
            ("&amp;", "&amp;", &all[..], Some((1, 1)), "a reference kept as written"),
            (
                "&",
                "&amp;",
                &[CodeSpan, Verbatim][..],
                Some((1, 1)),
                "code keeps references",
            ),
            (
                "é",
                "&eacute;",
                &[CodeSpan, Verbatim][..],
                None,
                "code keeps references",
            ),
            ("&x", "&x;", &all[..], Some((1, 1)), "an unknown name"),
            ("*", "\\*", &resolved[..], Some((2, 1)), "an escape"),
            ("*", "\\*", &[CodeSpan, Verbatim][..], None, "code keeps backslashes"),
            ("\\", "\\a", &all[..], Some((1, 1)), "a backslash before a letter"),
            ("a", "\\a", &resolved[..], None, "only punctuation is escaped"),
            ("\\b", "\\\\b", &resolved[..], Some((2, 1)), "an escaped backslash"),
            (
                "\\\\",
                "\\\\",
                &all[..],
                Some((1, 1)),
                "two backslashes kept as written",
            ),
            (
                "\\b",
                "\\\\b",
                &[CodeSpan, Verbatim][..],
                Some((1, 1)),
                "code keeps both backslashes",
            ),
            (
                "\\",
                "\\\n",
                &all[..],
                Some((1, 1)),
                "the backslash of a hard break is no escape",
            ),
            ("\n", "\\\n", &all[..], None, "and belongs to no line ending"),
            (" ", "\n", &[CodeSpan][..], Some((1, 1)), "a line ending in a code span"),
            (
                " ",
                "\r",
                &[CodeSpan][..],
                Some((1, 1)),
                "a lone carriage return in a code span",
            ),
            (
                "\r ",
                "\r\n",
                &[CodeSpan][..],
                Some((1, 1)),
                "the carriage return of a pair stays",
            ),
            (
                " ",
                "\n",
                &[Text, Typographic, Verbatim][..],
                None,
                "line endings stay elsewhere",
            ),
            ("„", "\"", &[Typographic][..], Some((1, 3)), "a typographic quote"),
            ("–", "--", &[Typographic][..], Some((2, 3)), "a dash"),
            ("…", "...", &[Typographic][..], Some((3, 3)), "an ellipsis"),
            (
                "„",
                "&quot;",
                &[Typographic][..],
                Some((6, 3)),
                "a typographic quote from a reference",
            ),
            (
                "„",
                "\\\"",
                &[Typographic][..],
                Some((2, 3)),
                "a typographic quote from an escape",
            ),
            (
                "„",
                "\"",
                &[Text, CodeSpan, Verbatim][..],
                None,
                "typography only where it is shown",
            ),
        ] {
            for &spelling in spellings {
                assert_eq!(
                    spelled(literal, source, spelling),
                    wanted,
                    "{rule}: {literal:?} in {source:?} as {spelling:?}"
                );
            }
        }
    }

    fn steps(literal: &str, source: &str, spelling: Spelling, gaps: Gaps) -> Option<Vec<(usize, usize)>> {
        let mut aligner = Aligner::new(source, spelling, gaps, true);
        let mut shown = literal;
        let mut out = Vec::new();
        while !shown.is_empty() {
            let step = aligner.next(shown)?;
            out.push((step.at, step.in_source));
            shown = &shown[step.in_literal..];
        }
        Some(out)
    }

    #[test]
    #[expect(clippy::too_many_lines, reason = "one line per case")]
    fn gaps_are_what_the_source_may_hold_between_characters() {
        use Gaps::{Anything, Closed, LinePrefixes};
        use Spelling::{CodeSpan, Text, Verbatim};
        for (literal, source, spelling, gaps, wanted, why) in [
            ("ab", "ab", Text, Closed, Some(vec![(0, 1), (1, 1)]), "nothing between"),
            ("ab", "a b", Text, Closed, None, "a closed gap"),
            (
                "a\nb",
                "> a\n> b",
                Verbatim,
                LinePrefixes,
                Some(vec![(2, 1), (3, 1), (6, 1)]),
                "a quote",
            ),
            (
                "a\nb",
                "a\n  b",
                Verbatim,
                LinePrefixes,
                Some(vec![(0, 1), (1, 1), (4, 1)]),
                "an item",
            ),
            (
                "a\nb",
                "a\r> b",
                Verbatim,
                LinePrefixes,
                Some(vec![(0, 1), (4, 0), (4, 1)]),
                "a lone CR ends a line",
            ),
            (
                "a>b",
                "a\\>b",
                Text,
                LinePrefixes,
                Some(vec![(0, 1), (1, 2), (3, 1)]),
                "an escape",
            ),
            (
                "a\\b",
                "a\\\\b",
                Text,
                Closed,
                Some(vec![(0, 1), (1, 2), (3, 1)]),
                "an escaped backslash",
            ),
            (
                "a\nb",
                "a  \nb",
                Verbatim,
                LinePrefixes,
                Some(vec![(0, 1), (3, 1), (4, 1)]),
                "spaces before a line end",
            ),
            (
                "a\\b",
                "a>\\b",
                Verbatim,
                LinePrefixes,
                None,
                "a quote mark only in front of a line",
            ),
            (
                "a b",
                "a\nb",
                CodeSpan,
                LinePrefixes,
                Some(vec![(0, 1), (1, 1), (2, 1)]),
                "a line ending",
            ),
            (
                " a",
                "a",
                Verbatim,
                LinePrefixes,
                Some(vec![(0, 0), (0, 1)]),
                "whitespace only in the literal",
            ),
            (
                "a ",
                "a",
                Verbatim,
                LinePrefixes,
                Some(vec![(0, 1), (1, 0)]),
                "whitespace beyond the source",
            ),
            ("ab", "a", Verbatim, LinePrefixes, None, "a letter beyond the source"),
            (
                "ab",
                "a<i>b",
                Text,
                Anything,
                Some(vec![(0, 1), (4, 1)]),
                "tags between",
            ),
            ("ab", "a", Text, Anything, None, "nothing to find"),
        ] {
            assert_eq!(
                steps(literal, source, spelling, gaps),
                wanted,
                "{why}: {literal:?} in {source:?}"
            );
        }
        let mut aligner = Aligner::new("ab", Text, Anything, false);
        assert_eq!(
            (aligner.next(""), aligner.source(), aligner.position()),
            (None, "ab", 0)
        );
    }
}
