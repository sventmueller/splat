//! Typographic punctuation and the marks inside running text that Obsidian, Pandoc and code hosts give a meaning:
//! `#tags`, `@mentions`, `#123` issues, `!123` merge requests and `[@key]` citations.

use std::ops::Range;

/// Quotation marks of a language: double opening and closing, single opening and closing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Quotes {
    pub open: char,
    pub close: char,
    pub open_single: char,
    pub close_single: char,
}

const APOSTROPHE: char = '’';

/// `text` with straight quotes turned into `quotes`, `--` and `---` into dashes and `...` into an ellipsis.
/// `before` is the character in front of the text, which decides whether a quote opens or closes.
pub fn smarten(text: &str, before: Option<char>, quotes: Quotes) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev = before;
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        let (replacement, used) = if rest.starts_with("---") {
            ('—', 3)
        } else if rest.starts_with("--") {
            ('–', 2)
        } else if rest.starts_with("...") {
            ('…', 3)
        } else {
            let opens = prev.is_none_or(|p| p.is_whitespace() || "([{<–—-/".contains(p));
            match c {
                '"' if opens => (quotes.open, 1),
                '"' => (quotes.close, 1),
                '\'' if opens => (quotes.open_single, 1),
                // Inside a word, as in "don't", it is an apostrophe.
                '\'' if prev.is_some_and(char::is_alphanumeric)
                    && rest[1..].chars().next().is_some_and(char::is_alphanumeric) =>
                {
                    (APOSTROPHE, 1)
                }
                '\'' => (quotes.close_single, 1),
                c => (c, c.len_utf8()),
            }
        };
        out.push(replacement);
        prev = Some(replacement);
        rest = &rest[used..];
    }
    out
}

/// How many source bytes at the start of `source` the typographic character `shown` stands for.
pub fn typographic_source(shown: char, source: &str) -> Option<usize> {
    let ascii = match shown {
        '—' => "---",
        '–' => "--",
        '…' => "...",
        '„' | '“' | '”' | '«' | '»' | '‟' => "\"",
        '‚' | '‘' | '’' | '‹' | '›' | '‛' => "'",
        _ => return None,
    };
    if source.starts_with(ascii) {
        Some(ascii.len())
    } else if matches!(shown, '«' | '»' | '‹' | '›' | '„' | '“' | '”') && source.starts_with('\'') {
        // Swedish, Danish and others pair single marks of one kind with double ones of another.
        Some(1)
    } else {
        None
    }
}

/// A mark in running text with a meaning beyond its characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Mark {
    /// `#tag`, as Obsidian files notes.
    Tag(String),
    /// `@name`, `#123` or `!123`, pointing into the code host of the document's repository.
    Reference(String),
    /// `[@key]` or `[@key; @other]`, Pandoc's citation of a bibliography entry.
    Citation(Vec<String>),
}

/// The marks in `text`, as byte ranges. `references` turns on mentions, issues and merge requests, which only mean
/// something in a repository.
pub fn marks(text: &str, before: Option<char>, references: bool) -> Vec<(Range<usize>, Mark)> {
    let mut out = Vec::new();
    let mut prev = before;
    let mut i = 0;
    while i < text.len() {
        let c = text[i..].chars().next().unwrap_or(' ');
        let at_word_start = prev.is_none_or(|p| p.is_whitespace() || "([{,;".contains(p));
        if c == '['
            && text[i + 1..].starts_with('@')
            && let Some(len) = text[i..].find(']')
        {
            let inside = &text[i + 1..i + len];
            let keys: Vec<String> = inside
                .split(';')
                .filter_map(|part| part.trim().strip_prefix('@'))
                .map(|k| k.trim_end_matches([',', '.']).to_string())
                .filter(|k| !k.is_empty() && k.chars().all(|c| c.is_alphanumeric() || "_:.-/".contains(c)))
                .collect();
            if !keys.is_empty() && keys.len() == inside.split(';').count() {
                out.push((i..i + len + 1, Mark::Citation(keys)));
                prev = Some(']');
                i += len + 1;
                continue;
            }
        }
        if at_word_start && matches!(c, '#' | '@' | '!') {
            let name_len: usize = text[i + 1..]
                .chars()
                .take_while(|&n| n.is_alphanumeric() || matches!(n, '_' | '-' | '/'))
                .map(char::len_utf8)
                .sum();
            let name = text[i + 1..i + 1 + name_len].trim_end_matches(['-', '/']);
            let digits = !name.is_empty() && name.chars().all(|n| n.is_ascii_digit());
            let mark = match c {
                '#' if !name.is_empty() && !digits => Some(Mark::Tag(name.to_string())),
                '#' | '!' if digits && references => Some(Mark::Reference(format!("{c}{name}"))),
                '@' if !name.is_empty()
                    && references
                    && name.chars().next().is_some_and(|n| n.is_ascii_alphanumeric()) =>
                {
                    Some(Mark::Reference(format!("@{name}")))
                }
                _ => None,
            };
            if let Some(mark) = mark {
                let end = i + 1 + name.len();
                out.push((i..end, mark));
                prev = text[..end].chars().next_back();
                i = end;
                continue;
            }
        }
        prev = Some(c);
        i += c.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const GERMAN: Quotes = Quotes {
        open: '„',
        close: '“',
        open_single: '‚',
        close_single: '‘',
    };

    #[test]
    fn quotes_dashes_and_ellipses_follow_the_language() {
        assert_eq!(
            smarten("\"Ja\" -- sagt 'er' ... don't --- \"x\"", None, GERMAN),
            "„Ja“ – sagt ‚er‘ … don’t — „x“"
        );
        assert_eq!(smarten("\"", Some('a'), GERMAN), "“");
        assert_eq!(typographic_source('„', "\"x"), Some(1));
        assert_eq!(typographic_source('—', "---"), Some(3));
        assert_eq!(typographic_source('–', "-"), None);
    }

    #[test]
    fn tags_references_and_citations_are_found_where_they_start_a_word() {
        let text = "Siehe #idee, #123 und !7 von @anna, mail@x.de, [@doe2020; @roe] und C#sharp #1a";
        let found: Vec<(&str, Mark)> = marks(text, None, true)
            .into_iter()
            .map(|(r, m)| (&text[r], m))
            .collect();
        assert_eq!(
            found,
            [
                ("#idee", Mark::Tag("idee".into())),
                ("#123", Mark::Reference("#123".into())),
                ("!7", Mark::Reference("!7".into())),
                ("@anna", Mark::Reference("@anna".into())),
                ("[@doe2020; @roe]", Mark::Citation(vec!["doe2020".into(), "roe".into()])),
                ("#1a", Mark::Tag("1a".into())),
            ]
        );
        let outside = marks(text, None, false);
        assert!(outside.iter().all(|(_, m)| !matches!(m, Mark::Reference(_))));
    }
}
