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

/// The quotes left open by the text smartened so far. Chinese and Japanese quotes pair by count rather than by
/// spacing, so the count carries from one run of a paragraph to the next.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Open {
    double: bool,
    single: bool,
}

/// `text` with straight quotes turned into `quotes`, `--` and `---` into dashes and `...` into an ellipsis.
/// `before` is the character in front of the text, which decides whether a quote opens or closes; `open` comes from
/// the earlier runs of the same paragraph.
pub fn smarten(text: &str, before: Option<char>, quotes: Quotes, open: &mut Open) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev = before;
    let mut rest = text;
    let Open {
        double: mut double_open,
        single: mut single_open,
    } = *open;
    while let Some(c) = rest.chars().next() {
        let (replacement, used) = if rest.starts_with("---") {
            ('—', 3)
        } else if rest.starts_with("--") {
            ('–', 2)
        } else if rest.starts_with("...") {
            ('…', 3)
        } else {
            let opens = prev.is_none_or(|p| p.is_whitespace() || "([{<–—-/".contains(p));
            // Chinese and Japanese set quotes without a space, so only the pairing tells opening from closing.
            let unspaced = prev.is_some_and(is_cjk);
            let next = rest[c.len_utf8()..].chars().next();
            // Hebrew abbreviates with geresh and gershayim, as in ג׳ירפה and צה״ל, which ASCII writes as quotes.
            let in_hebrew = prev.is_some_and(is_hebrew) && next.is_some_and(is_hebrew);
            match c {
                '"' if in_hebrew => ('״', 1),
                '\'' if in_hebrew => ('׳', 1),
                '"' => {
                    double_open = opens || unspaced && !double_open;
                    (if double_open { quotes.open } else { quotes.close }, 1)
                }
                // Inside a word, as in "don't", it is an apostrophe.
                '\'' if !unspaced
                    && prev.is_some_and(char::is_alphanumeric)
                    && next.is_some_and(char::is_alphanumeric) =>
                {
                    (APOSTROPHE, 1)
                }
                '\'' => {
                    single_open = opens || unspaced && !single_open;
                    (
                        if single_open {
                            quotes.open_single
                        } else {
                            quotes.close_single
                        },
                        1,
                    )
                }
                c => (c, c.len_utf8()),
            }
        };
        out.push(replacement);
        prev = Some(replacement);
        rest = &rest[used..];
    }
    *open = Open {
        double: double_open,
        single: single_open,
    };
    out
}

fn is_hebrew(c: char) -> bool {
    matches!(c, '\u{05D0}'..='\u{05EA}')
}

/// Vowel signs, tone marks and other marks that belong to the letter before them, in the scripts Splat speaks.
fn combining_mark(c: char) -> bool {
    matches!(c,
        '\u{0300}'..='\u{036F}'
        | '\u{0591}'..='\u{05BD}' | '\u{05BF}' | '\u{05C1}'..='\u{05C2}' | '\u{05C4}'..='\u{05C5}' | '\u{05C7}'
        | '\u{0610}'..='\u{061A}' | '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{06D6}'..='\u{06ED}'
        | '\u{0900}'..='\u{0903}' | '\u{093A}'..='\u{094F}' | '\u{0951}'..='\u{0957}' | '\u{0962}'..='\u{0963}'
        | '\u{0E31}' | '\u{0E34}'..='\u{0E3A}' | '\u{0E47}'..='\u{0E4E}'
        | '\u{200C}'..='\u{200D}' | '\u{3099}'..='\u{309A}')
}

/// Characters of Chinese and Japanese text, which runs without spaces: the CJK blocks from radicals to unified
/// ideographs, compatibility ideographs, half- and full-width forms, and the ideographs beyond the BMP.
fn is_cjk(c: char) -> bool {
    matches!(c, '\u{2E80}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}' | '\u{FF00}'..='\u{FFEF}' | '\u{20000}'..='\u{3FFFF}')
}

/// How many source bytes at the start of `source` the typographic character `shown` stands for.
pub fn typographic_source(shown: char, source: &str) -> Option<usize> {
    let ascii = match shown {
        '—' => "---",
        '–' => "--",
        '…' => "...",
        '„' | '“' | '”' | '«' | '»' | '‟' | '「' | '」' | '״' => "\"",
        '‚' | '‘' | '’' | '‹' | '›' | '‛' | '『' | '』' | '׳' => "'",
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
        let at_word_start = prev.is_none_or(|p| p.is_whitespace() || "([{,;（［｛【「『，；、،؛".contains(p));
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
                .take_while(|&n| n.is_alphanumeric() || combining_mark(n) || matches!(n, '_' | '-' | '/'))
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
            smarten(
                "\"Ja\" -- sagt 'er' ... don't --- \"x\"",
                None,
                GERMAN,
                &mut Open::default()
            ),
            "„Ja“ – sagt ‚er‘ … don’t — „x“"
        );
        assert_eq!(smarten("\"", Some('a'), GERMAN, &mut Open::default()), "“");
        assert_eq!(typographic_source('„', "\"x"), Some(1));
        assert_eq!(typographic_source('—', "---"), Some(3));
        assert_eq!(typographic_source('–', "-"), None);
    }

    #[test]
    fn chinese_and_japanese_quotes_pair_without_spaces() {
        const JAPANESE: Quotes = Quotes {
            open: '「',
            close: '」',
            open_single: '『',
            close_single: '』',
        };
        assert_eq!(
            smarten(
                "彼は\"今日は'晴れ'です\"と言った。\"はい\"",
                None,
                JAPANESE,
                &mut Open::default()
            ),
            "彼は「今日は『晴れ』です」と言った。「はい」"
        );
        assert_eq!(
            smarten("Er sagte \"日本\" und \"x\"", None, GERMAN, &mut Open::default()),
            "Er sagte „日本“ und „x“"
        );
        let mut open = Open::default();
        let first = smarten("彼は\"今日は", None, JAPANESE, &mut open);
        let second = smarten("晴れ\"です", Some('\n'), JAPANESE, &mut open);
        assert_eq!(first + &second, "彼は「今日は晴れ」です");
        assert_eq!(typographic_source('「', "\"x"), Some(1));
        const STRAIGHT: Quotes = Quotes {
            open: '"',
            close: '"',
            open_single: '\'',
            close_single: '\'',
        };
        assert_eq!(
            smarten("צה\"ל וג'ירפה \"כן\"", None, STRAIGHT, &mut Open::default()),
            "צה״ל וג׳ירפה \"כן\""
        );
        assert_eq!(typographic_source('״', "\"ל"), Some(1));
        assert_eq!(typographic_source('』', "'"), Some(1));
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
        let scripts = "#แท็ก #हिन्दी فكرة،#وسوم";
        let found: Vec<&str> = marks(scripts, None, false)
            .into_iter()
            .map(|(r, _)| &scripts[r])
            .collect();
        assert_eq!(found, ["#แท็ก", "#हिन्दी", "#وسوم"]);
        let chinese = "想法，#灵感、#12（@anna）和C#语言";
        let found: Vec<&str> = marks(chinese, None, true)
            .into_iter()
            .map(|(r, _)| &chinese[r])
            .collect();
        assert_eq!(found, ["#灵感", "#12", "@anna"]);
    }
}
