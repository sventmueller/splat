//! Typographic punctuation and the marks inside running text that Obsidian, Pandoc and code hosts give a meaning:
//! `#tags`, `@mentions`, `#123` issues, `!123` merge requests and `[@key]` citations.

use std::ops::Range;

use super::text::Byte;

/// Quotation marks of a language: double opening and closing, single opening and closing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Quotes {
    /// The opening double quote, `„` in German.
    pub open: char,
    /// The closing double quote, `“` in German.
    pub close: char,
    /// The opening single quote, `‚` in German.
    pub open_single: char,
    /// The closing single quote, `‘` in German.
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

/// The character in front of running text at `at`, past the formatting around it: a quote behind `**` or `<b>` opens or
/// closes by what stands before those.
pub fn before_text(text: &str, at: Byte) -> Option<char> {
    const MOST_TAGS: usize = 200;
    let mut rest = &text[..text.floor_char_boundary(at.get())];
    loop {
        rest = rest.trim_end_matches(['*', '_', '~', '=', '^', '`']);
        let window = rest.floor_char_boundary(rest.len().saturating_sub(MOST_TAGS));
        let tag = rest
            .ends_with('>')
            .then(|| rest[window..].rfind('<'))
            .flatten()
            .map(|at| window + at);
        match tag.filter(|&at| super::markup::read_tag(rest, at).is_some_and(|(_, end)| end == rest.len())) {
            Some(at) => rest = &rest[..at],
            None => return rest.chars().next_back(),
        }
    }
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
pub enum TextMark {
    /// `#tag`, as Obsidian files notes.
    Tag(String),
    /// `@name`, `#123` or `!123`, pointing into the code host of the document's repository.
    Reference(String),
    /// `[@key]` or `[@key; @other]`, Pandoc's citation of a bibliography entry.
    Citation(Vec<String>),
}

/// The marks in `text`, as byte ranges. `references` turns on mentions, issues and merge requests, which only mean
/// something in a repository.
pub fn marks(text: &str, before: Option<char>, references: bool) -> Vec<(Range<usize>, TextMark)> {
    let mut out = Vec::new();
    let mut prev = before;
    let mut i = 0;
    while i < text.len() {
        let c = text[i..].chars().next().unwrap_or(' ');
        let at_word_start = prev.is_none_or(|p| p.is_whitespace() || "([{,;（［｛【「『，；、،؛".contains(p));
        // A citation holds no other bracket, so the search for its end stops at the next one of either kind; searching
        // on would read the rest of a long text once per `[@`.
        if c == '['
            && text[i + 1..].starts_with('@')
            && let Some(len) = text[i + 1..].find(['[', ']']).map(|at| at + 1)
            && text[i + len..].starts_with(']')
        {
            let inside = &text[i + 1..i + len];
            let keys: Vec<String> = inside
                .split(';')
                .filter_map(|part| part.trim().strip_prefix('@'))
                .map(|k| k.trim_end_matches([',', '.']).to_string())
                .filter(|k| !k.is_empty() && k.chars().all(|c| c.is_alphanumeric() || "_:.-/".contains(c)))
                .collect();
            if !keys.is_empty() && keys.len() == inside.split(';').count() {
                out.push((i..i + len + 1, TextMark::Citation(keys)));
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
                '#' if !name.is_empty() && !digits => Some(TextMark::Tag(name.to_string())),
                '#' | '!' if digits && references => Some(TextMark::Reference(format!("{c}{name}"))),
                '@' if !name.is_empty()
                    && references
                    && name.chars().next().is_some_and(|n| n.is_ascii_alphanumeric()) =>
                {
                    Some(TextMark::Reference(format!("@{name}")))
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

/// What running text stands in, which decides how much of it is shown as typed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Within {
    /// Code, inline or a block, which shows every character as typed.
    pub code: bool,
    /// The text of a link, which can hold no other link and so no mark.
    pub link: bool,
    /// Text inside raw HTML, which the page shows as written.
    pub html: bool,
}

/// Running text as the preview and the exports show it: typographic punctuation applied, and where its marks stand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Running {
    /// The text to show.
    pub shown: String,
    /// The marks in `shown`, in text order.
    pub marks: Vec<(Range<usize>, TextMark)>,
}

/// A stretch of [`Running`] text: plain text, or what a writer made of a mark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment<'a, T> {
    /// Text between the marks, and every mark the writer declined.
    Text(&'a str),
    /// A mark the writer took.
    Mark(T),
}

impl Running {
    /// `literal`, which stands at `at` in `source`. Code and raw HTML keep their quotes, and code, raw HTML and link
    /// text hold no marks; `open` carries the open quotes from the earlier text of the same paragraph.
    pub fn of(
        source: &str,
        literal: &str,
        at: Byte,
        within: Within,
        quotes: Option<Quotes>,
        open: &mut Open,
        references: bool,
    ) -> Running {
        if within.code {
            return Running {
                shown: literal.to_string(),
                marks: Vec::new(),
            };
        }
        let before = before_text(source, at);
        let shown = match quotes {
            Some(quotes) if !within.html => smarten(literal, before, quotes, open),
            _ => literal.to_string(),
        };
        let marks = if within.link || within.html {
            Vec::new()
        } else {
            marks(&shown, before, references)
        };
        Running { shown, marks }
    }

    /// The text in order, with each mark `take` makes something of in its place; a mark it declines stays text, joined
    /// to the text around it.
    pub fn segments<T>(&self, mut take: impl FnMut(&str, &TextMark) -> Option<T>) -> Vec<Segment<'_, T>> {
        let mut out = Vec::new();
        let mut done = 0;
        for (range, mark) in &self.marks {
            let Some(made) = take(&self.shown[range.clone()], mark) else {
                continue;
            };
            if range.start > done {
                out.push(Segment::Text(&self.shown[done..range.start]));
            }
            out.push(Segment::Mark(made));
            done = range.end;
        }
        if done < self.shown.len() {
            out.push(Segment::Text(&self.shown[done..]));
        }
        out
    }
}

/// What citation marks with these keys show: the names the bibliography knows for them, or `None` if it misses one, which
/// leaves the text as written.
pub fn citation_shown(keys: &[String], name_of: impl Fn(&str) -> Option<String>) -> Option<String> {
    let names: Option<Vec<String>> = keys.iter().map(|key| name_of(key)).collect();
    Some(format!("({})", names?.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quote_behind_a_slash_opens() {
        let mut open = Default::default();
        assert_eq!(smarten("a/\"b\"", None, GERMAN, &mut open), "a/„b“");
    }

    const GERMAN: Quotes = Quotes {
        open: '„',
        close: '“',
        open_single: '‚',
        close_single: '‘',
    };

    #[test]
    fn formatting_does_not_count_as_the_character_before_a_quote() {
        let text = "x **\"Hallo\"** y";
        assert_eq!(before_text(text, Byte::new(4)), Some(' '));
        assert_eq!(before_text("<b>x", Byte::new(3)), None);
        assert_eq!(before_text("a <b title=\"t\">x", Byte::new(15)), Some(' '));
        assert_eq!(before_text("a>x", Byte::new(2)), Some('>'));
        assert_eq!(before_text("ä", Byte::new(1)), None);
    }

    #[test]
    fn a_citation_shows_all_its_names_or_stays_as_written() {
        let keys = ["doe".to_string(), "roe".to_string()];
        let known = |key: &str| Some(key.to_uppercase());
        assert_eq!(citation_shown(&keys, known).as_deref(), Some("(DOE; ROE)"));
        let one_missing = |key: &str| (key == "doe").then(|| "Doe".to_string());
        assert_eq!(citation_shown(&keys, one_missing), None);
    }

    #[test]
    fn citations_without_a_closing_bracket_are_no_marks() {
        assert!(marks("[@doe und [@roe", None, false).is_empty());
        assert_eq!(marks("[@doe und [@roe]", None, false).len(), 1);
        assert_eq!(marks("[@a [@b [@c; @d] e", None, false).len(), 1);
    }

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
        let found: Vec<(&str, TextMark)> = marks(text, None, true)
            .into_iter()
            .map(|(r, m)| (&text[r], m))
            .collect();
        assert_eq!(
            found,
            [
                ("#idee", TextMark::Tag("idee".into())),
                ("#123", TextMark::Reference("#123".into())),
                ("!7", TextMark::Reference("!7".into())),
                ("@anna", TextMark::Reference("@anna".into())),
                (
                    "[@doe2020; @roe]",
                    TextMark::Citation(vec!["doe2020".into(), "roe".into()])
                ),
                ("#1a", TextMark::Tag("1a".into())),
            ]
        );
        let outside = marks(text, None, false);
        assert!(outside.iter().all(|(_, m)| !matches!(m, TextMark::Reference(_))));
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

    #[test]
    fn three_dots_are_the_source_of_an_ellipsis() {
        assert_eq!(typographic_source('…', "...x"), Some(3));
        assert_eq!(typographic_source('…', "..x"), None);
    }

    #[test]
    fn a_citation_holds_nothing_but_valid_keys() {
        for text in ["[@]", "[@a!]", "[@a; b]", "[@a; @b!]"] {
            assert!(marks(text, None, false).is_empty(), "{text}");
        }
        assert_eq!(marks("[@a; @b]", None, false).len(), 1);
    }

    #[test]
    fn running_text_follows_what_it_stands_in() {
        let of = |within: Within| {
            Running::of(
                "\"a\" #tag",
                "\"a\" #tag",
                Byte::new(0),
                within,
                Some(GERMAN),
                &mut Open::default(),
                true,
            )
        };
        let plain = of(Within::default());
        assert_eq!(plain.shown, "„a“ #tag");
        assert_eq!(plain.marks, [(8..12, TextMark::Tag("tag".into()))]);
        let code = of(Within {
            code: true,
            ..Within::default()
        });
        assert_eq!((code.shown.as_str(), code.marks.len()), ("\"a\" #tag", 0));
        let link = of(Within {
            link: true,
            ..Within::default()
        });
        assert_eq!(
            (link.shown.as_str(), link.marks.len()),
            ("„a“ #tag", 0),
            "a link smartens but holds no mark"
        );
        let html = of(Within {
            html: true,
            ..Within::default()
        });
        assert_eq!((html.shown.as_str(), html.marks.len()), ("\"a\" #tag", 0));
    }

    #[test]
    fn a_declined_mark_stays_part_of_the_text_around_it() {
        let running = Running::of(
            "a #x b #y c",
            "a #x b #y c",
            Byte::new(0),
            Within::default(),
            None,
            &mut Open::default(),
            true,
        );
        let segments = running.segments(|piece, _| (piece == "#y").then_some(piece.len()));
        assert_eq!(
            segments,
            [Segment::Text("a #x b "), Segment::Mark(2), Segment::Text(" c")]
        );
        assert_eq!(running.segments(|_, _| None::<()>), [Segment::Text("a #x b #y c")]);
        let at_the_ends = Running::of(
            "#x",
            "#x",
            Byte::new(0),
            Within::default(),
            None,
            &mut Open::default(),
            true,
        );
        assert_eq!(at_the_ends.segments(|_, _| Some(())), [Segment::Mark(())]);
    }
}
