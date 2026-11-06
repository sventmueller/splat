//! How much a document holds for its reader: words, characters and the minutes they take, for all of it or for a
//! selection.

use std::ops::Range;
use std::sync::OnceLock;

use unicode_normalization::char::is_combining_mark;

use crate::aligner::{Aligner, Gaps, Spelling};
use crate::doc::{Block, BlockKind, Doc, Inline, InlineKind, code_figure};
use crate::markup::{self, Node, Role};
use crate::text::Byte;

/// What a document holds for its reader. A word is a run of letters, digits and marks, which `'`, `’` and `-` inside it
/// hold together, and every Han or kana character is a word of its own, as Microsoft Word counts them; so `3.14` and
/// `snake_case` are two words each. Hangul goes by its spaces. Thai, Lao, Khmer and Myanmar leave no spaces between
/// their words and telling them apart takes a dictionary, so each run of them counts as one word and their count stays
/// far too low. Characters are Unicode scalar values without line breaks, combining marks, variation selectors and the
/// zero width joiner.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Counts {
    /// Words, signs among them.
    pub words: usize,
    /// As defined above, which is less than the bytes and the scalar values of a text in most scripts.
    pub characters: usize,
    /// Of `words`, those read one sign at a time: Chinese, Japanese.
    pub signs: usize,
}

impl Counts {
    /// Minutes of reading, rounded up, 0 for no words.
    pub fn reading_minutes(self) -> usize {
        // Silent reading speeds of the International Reading Speed Texts (Trauzettel-Klosinski et al. 2012).
        const WORDS_PER_MINUTE: u128 = 230;
        const SIGNS_PER_MINUTE: u128 = 300;
        let signs = self.signs.min(self.words);
        let others = (self.words - signs) as u128;
        let parts = others * SIGNS_PER_MINUTE + signs as u128 * WORDS_PER_MINUTE;
        usize::try_from(parts.div_ceil(WORDS_PER_MINUTE * SIGNS_PER_MINUTE)).unwrap_or(usize::MAX)
    }
}

/// What a reader sees of `doc`: text, code, table cells, headings, callout titles and footnotes, but neither front
/// matter, link targets, HTML tags, `%%` comments, formulas, diagrams nor pictures with their alternative text.
pub fn of_doc(doc: &Doc) -> Counts {
    of_part(doc, 0, doc.text().len())
}

/// `of_doc` as far as it lies inside the source `range`. A text run is cut at the range, so a word across its edge
/// counts on both sides, and each character on one of them.
pub fn of_range(doc: &Doc, range: Range<Byte>) -> Counts {
    let range = doc.fit(range);
    if range.is_empty() {
        Counts::default()
    } else {
        of_part(doc, range.start.get(), range.end.get())
    }
}

fn of_part(doc: &Doc, from: usize, to: usize) -> Counts {
    let text = doc.text();
    let mut counter = Counter {
        text,
        from,
        to,
        reaches_end: to == text.len(),
        tally: Tally::default(),
    };
    if doc.is_too_large() {
        counter.run(text, &(Byte::default()..Byte::new(text.len())), Spelling::Verbatim);
    }
    let whole = from == 0 && counter.reaches_end;
    for block in &doc.blocks {
        // A smaller range than all of the text asks about few of the top-level blocks.
        if whole || (block.range.start.get() < to && from < block.range.end.get()) {
            counter.block(block);
        }
    }
    counter.tally.counts
}

/// What a character means to a count.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    /// Counts as a character and ends a word.
    Other,
    /// A letter or a digit.
    Word,
    /// Holds a word together when a letter stands on both sides.
    Joiner,
    /// Han or kana.
    Sign,
    /// Belongs to the character before it: it does not count, and it changes nothing about the word it stands in.
    Mark,
    /// Counts as a character, but no reader sees a gap in a word for it.
    Invisible,
    LineBreak,
}

fn is_sign(c: char) -> bool {
    matches!(c,
        '\u{3005}'..='\u{3007}' | '\u{3021}'..='\u{3029}' | '\u{3038}'..='\u{303C}'
        | '\u{3041}'..='\u{3096}' | '\u{309D}'..='\u{309F}' | '\u{30A1}'..='\u{30FA}' | '\u{30FC}'..='\u{30FF}'
        | '\u{31F0}'..='\u{31FF}' | '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}'
        | '\u{FF66}'..='\u{FF9F}' | '\u{1B000}'..='\u{1B16F}' | '\u{20000}'..='\u{3FFFF}')
}

fn class_of(c: char) -> Class {
    match c {
        '\n' | '\r' | '\u{B}' | '\u{C}' | '\u{85}' | '\u{2028}' | '\u{2029}' => Class::LineBreak,
        '\'' | '’' | '-' => Class::Joiner,
        '\u{200D}' => Class::Mark,
        '\u{AD}' | '\u{200C}' | '\u{2060}' => Class::Invisible,
        c if is_sign(c) => Class::Sign,
        c if is_combining_mark(c) => Class::Mark,
        c if c.is_alphanumeric() => Class::Word,
        _ => Class::Other,
    }
}

const fn ascii_class(b: u8) -> Class {
    match b {
        b'\n' | b'\r' | 0xB | 0xC => Class::LineBreak,
        b'\'' | b'-' => Class::Joiner,
        b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' => Class::Word,
        _ => Class::Other,
    }
}

const ASCII_CLASSES: [Class; 128] = {
    let mut table = [Class::Other; 128];
    let mut b = 0;
    while b < 128 {
        table[b as usize] = ascii_class(b);
        b += 1;
    }
    table
};

/// The classes of the Basic Multilingual Plane, made on first use: the lookups in the standard library and in
/// `unicode-normalization` take ten times as long as a character may in Chinese, Hindi or Thai.
fn plane_classes() -> &'static [Class] {
    static CLASSES: OnceLock<Box<[Class]>> = OnceLock::new();
    CLASSES.get_or_init(|| {
        (0..=0xFFFF)
            .map(|code| char::from_u32(code).map_or(Class::Other, class_of))
            .collect()
    })
}

const fn is_word_byte(b: u8) -> bool {
    ((b | 0x20).wrapping_sub(b'a') < 26) | (b.wrapping_sub(b'0') < 10)
}

const fn is_joiner_byte(b: u8) -> bool {
    (b == b'\'') | (b == b'-')
}

const fn is_line_break_byte(b: u8) -> bool {
    b.wrapping_sub(b'\n') < 4
}

fn ascii_prefix(bytes: &[u8]) -> usize {
    let (words, tail) = bytes.as_chunks::<8>();
    let mut at = 0;
    for word in words {
        let high = u64::from_le_bytes(*word) & 0x8080_8080_8080_8080;
        if high != 0 {
            return at + high.trailing_zeros() as usize / 8;
        }
        at += 8;
    }
    at + tail.iter().take_while(|b| b.is_ascii()).count()
}

/// Of the bytes of an ASCII run behind its first two: how many start a word, and how many are line breaks. Nothing
/// branches and the sums stay in bytes, which lets the compiler take many bytes at a time; only one byte at a time
/// would miss the time a whole document may take.
fn starts_and_breaks(run: &[u8]) -> (usize, usize) {
    const LONGEST_SUM: usize = u8::MAX as usize;
    let (mut starts, mut breaks) = (0, 0);
    for ((two_back, one_back), byte) in run[..run.len() - 2]
        .chunks(LONGEST_SUM)
        .zip(run[1..run.len() - 1].chunks(LONGEST_SUM))
        .zip(run[2..].chunks(LONGEST_SUM))
    {
        let (mut piece_starts, mut piece_breaks) = (0u8, 0u8);
        for ((&a, &b), &c) in two_back.iter().zip(one_back).zip(byte) {
            let start = is_word_byte(c) & !is_word_byte(b) & !(is_joiner_byte(b) & is_word_byte(a));
            piece_starts = piece_starts.wrapping_add(u8::from(start));
            piece_breaks = piece_breaks.wrapping_add(u8::from(is_line_break_byte(c)));
        }
        starts += usize::from(piece_starts);
        breaks += usize::from(piece_breaks);
    }
    (starts, breaks)
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Within {
    #[default]
    NoWord,
    Word,
    /// Behind a joiner behind a letter: a letter now goes on with the word.
    Joiner,
}

#[derive(Default)]
struct Tally {
    counts: Counts,
    within: Within,
}

impl Tally {
    /// Nothing a reader takes for one word stands on both sides of here.
    fn gap(&mut self) {
        self.within = Within::NoWord;
    }

    fn step(&mut self, class: Class) {
        match class {
            Class::Word => {
                self.counts.words += usize::from(self.within == Within::NoWord);
                self.counts.characters += 1;
                self.within = Within::Word;
            }
            Class::Sign => {
                self.counts.words += 1;
                self.counts.signs += 1;
                self.counts.characters += 1;
                self.within = Within::NoWord;
            }
            Class::Joiner => {
                self.counts.characters += 1;
                self.within = if self.within == Within::Word {
                    Within::Joiner
                } else {
                    Within::NoWord
                };
            }
            Class::Other => {
                self.counts.characters += 1;
                self.within = Within::NoWord;
            }
            Class::LineBreak => self.within = Within::NoWord,
            Class::Invisible => self.counts.characters += 1,
            Class::Mark => {}
        }
    }

    fn add(&mut self, text: &str) {
        let mut rest = text.as_bytes();
        while !rest.is_empty() {
            let (ascii, behind) = rest.split_at(ascii_prefix(rest));
            self.ascii(ascii);
            rest = self.non_ascii(behind);
        }
    }

    fn ascii(&mut self, run: &[u8]) {
        let (head, tail) = run.split_at(run.len().min(2));
        for &b in head {
            self.step(ASCII_CLASSES[usize::from(b)]);
        }
        if let [.., before, last] = *run
            && !tail.is_empty()
        {
            let (starts, breaks) = starts_and_breaks(run);
            self.counts.words += starts;
            self.counts.characters += tail.len() - breaks;
            self.within = if is_word_byte(last) {
                Within::Word
            } else if is_joiner_byte(last) && is_word_byte(before) {
                Within::Joiner
            } else {
                Within::NoWord
            };
        }
    }

    /// Takes the characters that `bytes`, the rest of a text, starts with up to the next ASCII byte; returns what is
    /// left.
    fn non_ascii<'t>(&mut self, mut bytes: &'t [u8]) -> &'t [u8] {
        if bytes.is_empty() {
            return bytes;
        }
        let classes = plane_classes();
        let at = |code: usize| classes.get(code).copied().unwrap_or(Class::Other);
        loop {
            match *bytes {
                [a @ 0xC0..=0xDF, b, ref rest @ ..] => {
                    self.step(at(usize::from(a & 0x1F) << 6 | usize::from(b & 0x3F)));
                    bytes = rest;
                }
                [a @ 0xE0..=0xEF, b, c, ref rest @ ..] => {
                    self.step(at(usize::from(a & 0x0F) << 12
                        | usize::from(b & 0x3F) << 6
                        | usize::from(c & 0x3F)));
                    bytes = rest;
                }
                [a @ 0xF0..=0xF7, b, c, d, ref rest @ ..] => {
                    let code = u32::from(a & 0x07) << 18
                        | u32::from(b & 0x3F) << 12
                        | u32::from(c & 0x3F) << 6
                        | u32::from(d & 0x3F);
                    self.step(char::from_u32(code).map_or(Class::Other, class_of));
                    bytes = rest;
                }
                _ => return bytes,
            }
        }
    }
}

/// Finds in a run's source what stands where, so that a run can be cut at a range however its literal is spelled: every
/// character of the literal has one place, which is why two ranges side by side take each of them once.
struct Cutter<'t>(Aligner<'t>);

impl<'t> Cutter<'t> {
    /// Cuts a literal of the kind `spelling` out of `source`, past whatever only the source holds, as the marks of a
    /// quote or the tags around a text.
    fn new(source: &'t str, spelling: Spelling) -> Self {
        Cutter(Aligner::new(source, spelling, Gaps::Anything, false))
    }

    /// The place of what `literal` starts with, and how long that is in the literal.
    fn next_place(&mut self, literal: &str) -> (usize, usize) {
        let Some(c) = literal.chars().next() else {
            return (self.0.position(), 0);
        };
        match self.0.next(literal) {
            Some(step) => (step.at, step.in_literal),
            None => {
                // Text the source does not spell stays inside the run.
                let source = self.0.source();
                (source.floor_char_boundary(source.len().saturating_sub(1)), c.len_utf8())
            }
        }
    }

    /// The part of `literal` whose places lie in `from..to`.
    fn cut(&mut self, literal: &str, from: usize, to: usize) -> Range<usize> {
        let mut shown = 0;
        let mut first = None;
        while shown < literal.len() {
            let (place, len) = self.next_place(&literal[shown..]);
            if place >= from && first.is_none() {
                first = Some(shown);
            }
            if place >= to {
                return first.unwrap_or(shown)..shown;
            }
            shown += len;
        }
        first.unwrap_or(literal.len())..literal.len()
    }
}

struct Counter<'d> {
    text: &'d str,
    from: usize,
    to: usize,
    /// The range takes what lies at the very end of the text, such as a run without a length there.
    reaches_end: bool,
    tally: Tally,
}

impl<'d> Counter<'d> {
    fn holds(&self, at: usize) -> bool {
        self.from <= at && (at < self.to || self.reaches_end)
    }

    /// `literal`, which `source` in the text spells.
    fn run(&mut self, literal: &str, source: &Range<Byte>, spelling: Spelling) {
        let (start, end) = (source.start.get(), source.end.get());
        if start >= end {
            if self.holds(start) {
                self.tally.add(literal);
            }
        } else if self.from <= start && end <= self.to {
            self.tally.add(literal);
        } else if start < self.to && self.from < end {
            let spelled = self.text.get(start..end).unwrap_or("");
            let (from, to) = (self.from.saturating_sub(start), self.to - start);
            let cut = if spelled == literal {
                from..to.min(literal.len())
            } else {
                Cutter::new(spelled, spelling).cut(literal, from, to)
            };
            self.tally.add(&literal[cut]);
        }
    }

    fn block(&mut self, block: &'d Block) {
        self.tally.gap();
        match &block.kind {
            BlockKind::CodeBlock {
                info, literal, content, ..
            } => {
                if code_figure(info, literal).is_none() {
                    self.run(literal, content, Spelling::Verbatim);
                }
            }
            BlockKind::HtmlBlock { literal } => self.html(literal, &block.range),
            BlockKind::Alert {
                title: Some(title),
                marker,
                ..
            } => self.run(title, marker, Spelling::Text),
            _ => {}
        }
        self.inlines(block.inlines());
        for child in block.children() {
            self.block(child);
        }
        self.tally.gap();
    }

    fn inlines(&mut self, inlines: &'d [Inline]) {
        for inline in inlines {
            match &inline.kind {
                InlineKind::Text(text) => self.run(text, &inline.range, Spelling::Text),
                InlineKind::Code(text) => self.run(text, &inline.range, Spelling::CodeSpan),
                InlineKind::Emoji(emoji) => {
                    if self.holds(inline.range.start.get()) {
                        self.tally.add(emoji);
                    }
                }
                InlineKind::SoftBreak
                | InlineKind::LineBreak
                | InlineKind::Math { .. }
                | InlineKind::Image { .. }
                | InlineKind::HtmlImage { .. }
                | InlineKind::WikiLink { embed: true, .. } => self.tally.gap(),
                InlineKind::Html(_) | InlineKind::FootnoteRef { .. } | InlineKind::Comment => {}
                InlineKind::Link { .. }
                | InlineKind::WikiLink { embed: false, .. }
                | InlineKind::Emph
                | InlineKind::Strong
                | InlineKind::Strikethrough
                | InlineKind::Underline
                | InlineKind::Highlight
                | InlineKind::Superscript
                | InlineKind::Subscript
                | InlineKind::Spoiler
                | InlineKind::Kbd
                | InlineKind::Span => self.inlines(&inline.children),
            }
        }
    }

    /// The text of an HTML block, which the reader sees without its tags.
    fn html(&mut self, literal: &str, range: &Range<Byte>) {
        let (start, end) = (range.start.get(), range.end.get());
        if self.to <= start || end <= self.from {
            return;
        }
        let nodes = markup::parse(literal);
        if self.from <= start && end <= self.to {
            self.html_nodes(&nodes, None);
        } else {
            let source = self.text.get(start..end).unwrap_or("");
            let wanted = self.from.saturating_sub(start)..self.to - start;
            self.html_nodes(&nodes, Some((&mut Cutter::new(source, Spelling::Text), &wanted)));
        }
    }

    fn html_nodes(&mut self, nodes: &[Node], mut cut: Option<(&mut Cutter, &Range<usize>)>) {
        for node in nodes {
            match node {
                Node::Text { text, .. } => match &mut cut {
                    Some((aligner, wanted)) => {
                        let part = aligner.cut(text, wanted.start, wanted.end);
                        self.tally.add(&text[part]);
                    }
                    None => self.tally.add(text),
                },
                Node::Element(element) => {
                    let inline = match element.role {
                        Role::Look(_) | Role::Presentational | Role::Link => true,
                        Role::LineBreak
                        | Role::Picture
                        | Role::PictureSources
                        | Role::Heading(_)
                        | Role::Paragraph
                        | Role::Division
                        | Role::Center
                        | Role::Section
                        | Role::Quote
                        | Role::Preformatted
                        | Role::Rule
                        | Role::List { .. }
                        | Role::ListItem
                        | Role::DefinitionList
                        | Role::Term
                        | Role::Definition
                        | Role::Table
                        | Role::TableSection { .. }
                        | Role::TableRow
                        | Role::TableCell { .. }
                        | Role::Details
                        | Role::Summary
                        | Role::Empty
                        | Role::Input
                        | Role::Dropped
                        | Role::Unknown => false,
                    };
                    if !inline {
                        self.tally.gap();
                    }
                    let inner = cut.as_mut().map(|(aligner, wanted)| (&mut **aligner, *wanted));
                    self.html_nodes(&element.children, inner);
                    if !inline {
                        self.tally.gap();
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::parse;
    use crate::text::bytes;

    fn counted(text: &str) -> Counts {
        of_doc(&parse(text))
    }

    fn words(text: &str) -> usize {
        counted(text).words
    }

    fn characters(text: &str) -> usize {
        counted(text).characters
    }

    #[test]
    fn a_word_is_a_run_of_letters_digits_and_marks() {
        for (text, wanted) in [
            ("", 0),
            ("eins zwei\n", 2),
            ("  eins\t zwei  \n", 2),
            ("a1 2b 3\n", 3),
            ("3.14 snake_case\n", 4),
            ("Grüße aus Köln\n", 3),
            ("Gru\u{308}ße\n", 1),
            ("!!! ... ---\n", 0),
        ] {
            assert_eq!(words(text), wanted, "{text:?}");
        }
    }

    #[test]
    fn apostrophes_and_hyphens_hold_a_word_together_only_inside_it() {
        for (text, wanted) in [
            ("don't\n", 1),
            ("don’t\n", 1),
            ("Müller-Lüdenscheidt\n", 1),
            ("a-b-c\n", 1),
            ("'quoted'\n", 1),
            ("-a- b\n", 2),
            ("a--b\n", 2),
            ("a-'b\n", 2),
            ("a - b\n", 2),
            ("a-\nb\n", 2),
            ("rock-'n'-roll\n", 3),
        ] {
            assert_eq!(words(text), wanted, "{text:?}");
        }
    }

    #[test]
    fn a_word_goes_on_across_formatting_but_not_across_what_a_reader_sees_between() {
        for (text, wanted) in [
            ("bo**ld**er\n", 1),
            ("a`b`c\n", 1),
            ("Müller-*Lüdenscheidt*\n", 1),
            ("a%%x%%b\n", 1),
            ("a<span>b</span>c\n", 1),
            ("a\nb\n", 2),
            ("a  \nb\n", 2),
            ("a<br>b\n", 2),
            ("a![x](y.png)b\n", 2),
            ("a$x$b\n", 2),
            ("a:rocket:b\n", 2),
            ("# a\nb\n", 2),
            ("|a|b|\n|-|-|\n|c|d|\n", 4),
        ] {
            assert_eq!(words(text), wanted, "{text:?}");
        }
    }

    #[test]
    fn each_han_or_kana_character_is_a_word_and_a_sign() {
        let counts = counted("東京タワーは高いです\n");
        assert_eq!(
            counts,
            Counts {
                words: 10,
                characters: 10,
                signs: 10
            }
        );
        let mixed = counted("Splat 日本語 2024-11 中文\n");
        assert_eq!((mixed.words, mixed.signs), (7, 5));
        assert_eq!(words("a日b\n"), 3);
        assert_eq!(words("日-本\n"), 2);
        assert_eq!(counted("か\u{3099}\n").signs, 1, "a voiced mark on its kana");
        assert_eq!(counted("𠮷\n").signs, 1, "an ideograph beyond the first plane");
        assert_eq!(counted("、。「」・\n").words, 0);
    }

    #[test]
    fn scripts_with_spaces_go_by_them_and_those_without_by_their_runs() {
        assert_eq!(words("한국어 문장 입니다\n"), 3);
        assert_eq!(words("ภาษาไทยไม่มีช่องว่าง\n"), 1);
        assert_eq!(words("ພາສາລາວ ຂອງ\n"), 2);
        assert_eq!(words("ភាសាខ្មែរ\n"), 1);
        assert_eq!(words("မြန်မာဘာသာ\n"), 1);
        assert_eq!(words("हिन्दी भाषा\n"), 2);
        assert_eq!(words("می‌خواهم\n"), 1, "a non-joiner stands inside a Persian word");
        assert_eq!(words("Soft\u{AD}hyphen\n"), 1);
    }

    #[test]
    fn characters_leave_out_breaks_marks_selectors_and_joiners() {
        assert_eq!(characters("a b\n"), 3);
        assert_eq!(characters("a\nb\n"), 2);
        assert_eq!(characters("a\r\nb\r\n"), 2);
        assert_eq!(characters("e\u{301}\n"), 1);
        assert_eq!(characters("❤\u{FE0F}\n"), 1, "a variation selector");
        assert_eq!(characters("👩\u{200D}💻\n"), 2, "a zero width joiner");
        assert_eq!(characters("😀\n"), 1);
        assert_eq!(characters("```\na\u{2028}b\n```\n"), 2, "a line separator in code");
        assert_eq!(words("❤\u{FE0F} 👩\u{200D}💻\n"), 0, "a mark starts no word");
        assert_eq!(characters("a\u{200C}b\n"), 3, "only the joiner is left out, as asked");
    }

    #[test]
    fn what_a_reader_sees_counts_and_what_only_the_source_holds_does_not() {
        for (text, wanted) in [
            ("---\nkey: value\nother: more\n---\n\neins\n", 1),
            ("[zwei drei](https://a.de/vier/fünf)\n", 2),
            ("[zwei drei][r]\n\n[r]: https://a.de/vier \"fünf\"\n", 2),
            ("![Bild mit Text](bild.png) eins\n", 1),
            ("![[Notiz drin]] eins\n", 1),
            ("[[Notiz|Titel hier]]\n", 2),
            ("[[Notiz da]]\n", 2),
            ("<img src=\"a.png\" alt=\"gar nicht\"> eins\n", 1),
            ("eins %%kein Wort davon%% zwei\n", 2),
            ("%%\nkein Wort davon\n%%\n\neins\n", 1),
            ("eins <!-- kein Wort --> zwei\n", 2),
            ("eins $a + b$ zwei\n", 2),
            ("$$\na + b\n$$\n\neins\n", 1),
            ("```math\na + b\n```\n\neins\n", 1),
            ("```mermaid\ngraph TD\nA --> B\n```\n\neins\n", 1),
            ("```dot\ndigraph a\n```\n", 0),
            ("```rust\nfn main\n```\n", 2),
            ("    code hier\n", 2),
            ("`code hier` eins\n", 3),
            ("| Kopf eins | zwei |\n|---|---|\n| drei | vier fünf |\n", 6),
            ("# Titel hier {#id .klasse}\n\nText\n", 3),
            ("> [!bug]- Absturz heute\n> Inhalt\n", 3),
            ("> [!NOTE] Mein Titel\n> b\n", 3),
            ("> [!NOTE]\n> b\n", 1),
            ("Fuß[^1] und^[Noch *mehr* davon] Ende\n\n[^1]: Die Notiz\n", 8),
            ("- [ ] Aufgabe eins\n- [x] zwei\n", 3),
            ("Begriff\n\n: Erklärung davon\n", 3),
            ("||geheim bleibt|| x^2^ ~~weg~~\n", 4),
            (
                "<div>\nHallo <b>Wel</b>t\n<script>kein wort</script><!-- nein -->\n</div>\n",
                2,
            ),
            ("<table><tr><td>a</td><td>b</td></tr></table>\n", 2),
            ("<a href=\"nein\">ja</a>\n", 1),
            ("a &amp; b \\* c &#66;\n", 4),
        ] {
            assert_eq!(words(text), wanted, "{text:?}");
        }
    }

    #[test]
    fn the_reading_time_goes_by_words_and_by_signs_and_is_rounded_up() {
        let minutes = |words, signs| {
            Counts {
                words,
                characters: 0,
                signs,
            }
            .reading_minutes()
        };
        assert_eq!(minutes(0, 0), 0);
        assert_eq!(minutes(1, 0), 1);
        assert_eq!(minutes(230, 0), 1);
        assert_eq!(minutes(231, 0), 2);
        assert_eq!(minutes(300, 300), 1);
        assert_eq!(minutes(301, 301), 2);
        assert_eq!(minutes(530, 300), 2);
        assert_eq!(minutes(531, 300), 3);
        assert_eq!(minutes(1, 5), 1, "more signs than words cannot be");
        assert!(minutes(usize::MAX, 0) > 0);
    }

    #[test]
    fn a_range_counts_the_same_text_as_far_as_it_lies_inside() {
        let text = "---\na: b c d\n---\n\n# Eins zwei\n\nDrei **vier** [fünf](x-y) `sechs sieben`\n";
        let doc = parse(text);
        assert_eq!(of_range(&doc, bytes(0..text.len())), of_doc(&doc));
        let at = |needle: &str| text.find(needle).unwrap();
        let part = |from: &str, to: &str| of_range(&doc, bytes(at(from)..at(to)));
        assert_eq!(part("a: b", "Eins").words, 0, "front matter");
        assert_eq!(part("Eins", "Drei").words, 2);
        assert_eq!(part("zwei", "vier").words, 2);
        assert_eq!(part("ei\n", "ier").words, 3, "\"ei\", \"Drei\" and \"v\"");
        assert_eq!(part("fünf", "x-y").words, 1);
        assert_eq!(part("x-y", "sieben").words, 1, "behind the address, \"sechs\"");
        assert_eq!(of_range(&doc, bytes(at("Eins")..at("Eins"))), Counts::default());
        assert_eq!(of_range(&doc, bytes(at("zwei")..at("Eins"))), Counts::default());
    }

    #[test]
    fn a_range_beyond_the_text_or_inside_a_letter_is_brought_to_its_edges() {
        let doc = parse("Füße\n");
        assert_eq!(of_range(&doc, bytes(0..99)), of_doc(&doc));
        assert_eq!(
            of_range(&doc, bytes(1..2)),
            Counts::default(),
            "up to the middle of the ü"
        );
        assert_eq!(of_range(&doc, bytes(2..3)).characters, 1, "from the middle of the ü");
        assert_eq!(of_range(&doc, bytes(90..99)), Counts::default());
    }

    #[test]
    fn a_run_spelled_otherwise_in_the_source_is_cut_by_what_it_spells() {
        let text = "a &amp; \\* b &#66; c\n";
        let doc = parse(text);
        let upto = |needle: &str| of_range(&doc, bytes(0..text.find(needle).unwrap()));
        assert_eq!(upto("&amp").characters, 2, "\"a \"");
        assert_eq!(upto("; \\*").characters, 3, "the reference counts where it starts");
        assert_eq!(upto("\\*").characters, 4);
        assert_eq!(upto("* b").characters, 5, "so does the escape");
        assert_eq!(upto("c\n").characters, of_doc(&doc).characters - 1);
    }

    #[test]
    fn code_is_cut_by_its_text_past_what_stands_around_it() {
        let quoted = "> ```\n> eins zwei\n> drei\n> ```\n";
        let doc = parse(quoted);
        let at = |needle: &str| quoted.find(needle).unwrap();
        assert_eq!(
            of_range(&doc, bytes(at("zwei")..at("> ```\n>") + quoted.len())).words,
            2
        );
        assert_eq!(of_range(&doc, bytes(at("eins")..at("drei"))).words, 2);
        assert_eq!(of_range(&doc, bytes(0..at("eins") + 2)).words, 1);
        let span = "`  a\nb  ` c\n";
        let doc = parse(span);
        assert_eq!(
            of_range(&doc, bytes(0..5)).characters,
            3,
            "a space, the a and the space that the line break reads as"
        );
        assert_eq!(of_doc(&doc).words, 3);
        let fenced = "```\na\n```\n";
        assert_eq!(of_doc(&parse(fenced)).characters, 1);
    }

    #[test]
    fn a_title_and_a_block_of_html_are_cut_by_what_they_show() {
        let title = "> [!bug]- Absturz heute\n> Inhalt\n";
        let doc = parse(title);
        let at = |needle: &str| title.find(needle).unwrap();
        assert_eq!(of_range(&doc, bytes(at("heute")..at("Inhalt"))).words, 1);
        let html = "<div>\nHallo <b>Wel</b>t und &amp;\n</div>\n\nDanach\n";
        let doc = parse(html);
        let at = |needle: &str| html.find(needle).unwrap();
        assert_eq!(of_range(&doc, bytes(at("Hallo")..at("Wel"))).words, 1);
        assert_eq!(of_range(&doc, bytes(at("Wel")..at("und"))).words, 1, "as one word");
        assert_eq!(of_range(&doc, bytes(at("und")..at("</div"))).characters, 5);
        assert_eq!(of_range(&doc, bytes(at("Danach")..html.len())).words, 1);
        let quoted = "> <div>\n> Hallo <b>Welt</b>\n> </div>\n";
        let doc = parse(quoted);
        let at = |needle: &str| quoted.find(needle).unwrap();
        assert_eq!(of_range(&doc, bytes(at("Welt")..quoted.len())).words, 1);
    }

    #[test]
    fn ranges_side_by_side_take_each_character_once() {
        for text in [
            "Grüße **aus** `Köln`, &amp; \\* [t](u) mehr\n\n> ```\n> eins zwei\n> ```\n\n<div>\nHallo <b>Wel</b>t\n</div>\n",
            "東京タワー :rocket: a^[Notiz da] b[^1]\n\n[^1]: Fuß\n\n> [!bug]- Absturz\n> x\n",
            "|a|b|\n|-|-|\n|`c\\|d`|&#66;|\n\n- [ ] Aufgabe\n\n# Titel\n",
        ] {
            let doc = parse(text);
            let whole = of_doc(&doc);
            for middle in (0..=text.len()).filter(|&at| text.is_char_boundary(at)) {
                let (a, b) = (
                    of_range(&doc, bytes(0..middle)),
                    of_range(&doc, bytes(middle..text.len())),
                );
                assert_eq!(a.characters + b.characters, whole.characters, "{text:?} at {middle}");
                assert_eq!(a.signs + b.signs, whole.signs, "{text:?} at {middle}");
                assert!(
                    a.words + b.words >= whole.words && a.words + b.words <= whole.words + 1,
                    "{text:?} at {middle}: {a:?} {b:?} of {whole:?}"
                );
            }
        }
    }

    #[test]
    fn a_literal_the_source_does_not_spell_stays_inside_its_run() {
        assert_eq!(Cutter::new("abc", Spelling::Text).cut("xyz", 2, 9), 0..3);
        assert_eq!(Cutter::new("abc", Spelling::Text).cut("xyz", 0, 2), 0..0);
        assert_eq!(
            Cutter::new("`a\nb`", Spelling::CodeSpan).cut("a b", 2, 3),
            1..2,
            "the space a line ending reads as"
        );
        assert_eq!(Cutter::new("ab", Spelling::Text).next_place(""), (0, 0));
    }

    #[test]
    fn a_run_without_a_length_counts_where_it_stands() {
        let words_of = |from: usize, to: usize, at: usize| {
            let mut counter = Counter {
                text: "ab",
                from,
                to,
                reaches_end: to == 2,
                tally: Tally::default(),
            };
            counter.run("x", &bytes(at..at), Spelling::Text);
            counter.tally.counts.words
        };
        assert_eq!(words_of(0, 1, 0), 1);
        assert_eq!(words_of(1, 2, 0), 0);
        assert_eq!(words_of(0, 1, 1), 0, "it belongs to the range behind");
        assert_eq!(words_of(1, 2, 2), 1, "at the very end of the text");
    }

    #[test]
    fn a_block_of_html_outside_the_range_is_not_read() {
        let text = "- <div>eins</div>\n- <div>zwei drei</div>\n";
        let doc = parse(text);
        assert_eq!(of_range(&doc, bytes(text.find("zwei").unwrap()..text.len())).words, 2);
    }

    #[test]
    fn a_text_too_large_to_parse_counts_as_it_stands() {
        let doc = Doc::too_large("eins *zwei* drei\n".into());
        assert_eq!(of_doc(&doc).words, 3);
        assert_eq!(of_range(&doc, bytes(5..11)).words, 1);
    }

    #[test]
    fn the_quick_paths_count_as_one_character_at_a_time_does() {
        let pieces = [
            "a", "b1", "-", "'", " ", "\n", "\r", "...", "ä", "é", "日", "か", "’", "\u{301}", "\u{200D}", "\u{FE0F}",
            "😀", "𠮷", "한", "ก", "\u{2028}", "\u{AD}", "x-y",
        ];
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            usize::try_from(seed % n as u64).unwrap_or(0)
        };
        for _ in 0..3000 {
            let text: String = (0..next(600)).map(|_| pieces[next(pieces.len())]).collect();
            let mut quick = Tally::default();
            quick.add(&text);
            let mut slow = Tally::default();
            for c in text.chars() {
                slow.step(class_of(c));
            }
            assert_eq!(quick.counts, slow.counts, "{text:?}");
            assert!(quick.within == slow.within, "{text:?}");
        }
    }

    #[test]
    #[ignore = "a measurement, for a quiet Mac: cargo test --release -p splat-markdown -- --ignored --nocapture counting"]
    fn counting_ten_megabytes_takes_about_15_ms() {
        const SIZE: usize = 10 << 20;
        let filled = |paragraph: &str| paragraph.repeat(SIZE / paragraph.len());
        let english = "The quick brown fox jumps over the lazy dog, and **then** it doesn't stop to `look` back at [the farm](https://example.com/farm) where it was born. Nobody has seen it since, though many have tried.\n\n";
        let german = "Der Müller-Lüdenscheidt läuft durch die große Straße, und **dann** grüßt er die Bäckerin über den Zaun hinweg. Später fährt er nach Köln und schreibt ihr einen langen Brief über das schöne Wetter.\n\n";
        let chinese = "东京塔是日本最高的建筑之一，每年有许多游客前来参观。它的**灯光**在夜晚非常美丽，人们常常在这里拍照留念。\n\n";
        let hindi = "हिन्दी भाषा में लिखा गया यह एक लंबा अनुच्छेद है जो गिनती की गति को मापने के लिए बनाया गया है।\n\n";
        let list = "- [ ] Aufgabe *eins* mit Text\n- zwei\n\n> Zitat `x`\n\n| a | b |\n|---|---|\n| c | d |\n\n";
        for (name, text) in [
            ("words only", filled("word ")),
            ("English prose", filled(english)),
            ("German prose", filled(german)),
            ("Chinese", filled(chinese)),
            ("Hindi", filled(hindi)),
            ("short blocks", filled(list)),
        ] {
            let doc = parse(&text);
            let counted: Vec<(std::time::Duration, Counts)> = (0..5)
                .map(|_| {
                    let started = std::time::Instant::now();
                    let counts = of_doc(&doc);
                    (started.elapsed(), counts)
                })
                .collect();
            let fastest = counted.iter().map(|(took, _)| *took).min().unwrap_or_default();
            println!(
                "{name}: first {:?}, fastest {fastest:?}, {:?}",
                counted[0].0, counted[0].1
            );
            if name != "short blocks" && cfg!(target_arch = "aarch64") {
                assert!(fastest.as_millis() <= 15, "{name}: {fastest:?}");
            }
        }
    }

    #[test]
    fn the_prefix_of_ascii_is_found_at_any_length() {
        for len in 0..40 {
            let ascii = "a".repeat(len);
            assert_eq!(ascii_prefix(ascii.as_bytes()), len);
            let more = format!("{ascii}ä");
            assert_eq!(ascii_prefix(more.as_bytes()), len);
        }
    }

    #[test]
    fn the_table_of_classes_says_what_each_character_is() {
        for b in 0..128u8 {
            assert_eq!(ascii_class(b), class_of(char::from(b)), "{b}");
        }
        assert_eq!(plane_classes()[0xE4], Class::Word);
        assert_eq!(plane_classes()[0xD800], Class::Other, "no character");
        assert_eq!(class_of('\u{E0100}'), Class::Mark, "a selector beyond the first plane");
    }
}
