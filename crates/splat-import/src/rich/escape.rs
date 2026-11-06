//! Text that stays literal in Splat's dialect: CommonMark, GitHub's extensions and those comrak reads for Splat. Each
//! rule escapes only where the character would start something where it stands, so that prose stays readable.

use splat_markdown::text::decode_entity;

/// What the text is part of.
#[derive(Clone, Copy, Default)]
pub(super) struct Where {
    pub(super) table: bool,
    pub(super) heading: bool,
    /// Link text or a picture's description, in which a bracket closes.
    pub(super) in_brackets: bool,
    /// The text starts a line, where block markers count.
    pub(super) line_start: bool,
}

/// How often characters that come in pairs occur in the text of a paragraph; one alone pairs with nothing.
#[derive(Clone, Copy, Default)]
pub(super) struct Counts {
    pub(super) tilde: usize,
    pub(super) caret: usize,
    pub(super) dollar: usize,
    /// Dollars that could end a formula: with no space in front and no digit behind.
    pub(super) closing_dollars: usize,
    pub(super) pipe: usize,
    /// Closing brackets that could end a link, with an address or another bracket behind them.
    pub(super) closing_brackets: usize,
    /// A backslash stands right in front of a closing bracket, which comrak takes for the end of a formula that an
    /// escaped opening bracket would start.
    pub(super) closes_formula: bool,
    /// An `@` stands somewhere, which can make a mail address of most things behind a `<`.
    pub(super) at_sign: bool,
}

impl Counts {
    /// Takes in text that follows a mark unless it starts a line, and that a mark follows unless it ends one.
    pub(super) fn add(&mut self, text: &str, starts_line: bool, ends_line: bool) {
        self.at_sign |= text.contains('@');
        let mut prev = if starts_line { b' ' } else { b'.' };
        for (at, byte) in text.bytes().enumerate() {
            match byte {
                b'~' => self.tilde += 1,
                b'^' => self.caret += 1,
                b'$' => {
                    self.dollar += 1;
                    if !is_ascii_space(prev) && !text.as_bytes().get(at + 1).is_some_and(u8::is_ascii_digit) {
                        self.closing_dollars += 1;
                    }
                }
                b'|' => self.pipe += 1,
                b']' => {
                    let follower = text.as_bytes().get(at + 1);
                    if matches!(follower, Some(b'(' | b'[')) || (follower.is_none() && !ends_line) {
                        self.closing_brackets += 1;
                    }
                }
                _ => {}
            }
            prev = byte;
        }
    }

    /// Takes in what is written literally, in which comrak still looks for the end of a formula it has started.
    pub(super) fn add_literal(&mut self, text: &str) {
        self.closes_formula |= text.contains("\\]");
        let dollars = text.bytes().filter(|b| *b == b'$').count();
        self.dollar += dollars;
        self.closing_dollars += dollars;
        self.pipe += text.bytes().filter(|b| *b == b'|').count();
    }
}

fn is_ascii_space(byte: u8) -> bool {
    byte.is_ascii_whitespace()
}

/// Whether a run of a character is written as it is, escaped in its first character, or in all.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Escape {
    No,
    First,
    All,
}

/// A character in its surroundings.
struct Place<'a> {
    prev: char,
    /// What follows the run of the character.
    next: char,
    run: usize,
    /// At the start of a line.
    start: bool,
    /// The run touches an end of the text, where a mark may stand next to it.
    edge: bool,
    /// The text from the run on.
    rest: &'a str,
    /// A closing brace follows in the text.
    brace_behind: bool,
}

impl Place<'_> {
    fn between_spaces(&self) -> bool {
        self.prev.is_whitespace() && self.next.is_whitespace()
    }

    fn inside_word(&self) -> bool {
        self.prev.is_alphanumeric() && self.next.is_alphanumeric()
    }

    /// A character that needs a partner: alone in the paragraph and not next to a mark, it has none.
    fn pairs(&self, count: usize) -> Escape {
        if self.between_spaces() || (count == 1 && !self.edge) {
            Escape::No
        } else {
            Escape::All
        }
    }
}

/// The characters whose meaning depends on how many of them stand together.
const RUNS: &str = "*_~^=|$%#\\";

/// Writes `text`, which stands between the characters `before` and `after`, so that it reads as itself.
pub(super) fn escape_text(text: &str, before: char, after: char, within: Where, counts: Counts, out: &mut String) {
    let mut prev = before;
    let mut at = 0;
    if within.line_start {
        at = escape_number(text, out);
        if at > 0 {
            prev = '.';
        }
    }
    let mut stretch = Unbroken::default();
    let last_brace = if within.heading { text.rfind('}') } else { None };
    while let Some(c) = text[at..].chars().next() {
        if matches!(c, 'h' | 'f' | 'w')
            && !within.in_brackets
            && BARE_ADDRESSES.iter().any(|s| text[at..].starts_with(s))
        {
            if at >= stretch.end {
                stretch = Unbroken::at(text, at, after, within.table, counts);
            }
            if let Some((written, read)) = bare_address(&text[at..], prev, &stretch, stretch.end - at) {
                out.push_str(&written);
                prev = text[..at + read].chars().next_back().unwrap_or(prev);
                at += read;
                continue;
            }
        }
        let run = if RUNS.contains(c) {
            text[at..].chars().take_while(|x| *x == c).count()
        } else {
            1
        };
        let behind = at + run * c.len_utf8();
        let place = Place {
            prev,
            next: text[behind..].chars().next().unwrap_or(after),
            run,
            start: within.line_start && at == 0,
            edge: (at == 0 && !before.is_whitespace()) || (behind == text.len() && !after.is_whitespace()),
            rest: &text[at..],
            brace_behind: last_brace.is_some_and(|brace| brace > at),
        };
        write_run(c, &place, rule(c, &place, within, counts), within, counts, out);
        prev = c;
        at = behind;
    }
}

/// What comrak reads as an address on its own, without being told, up to the next space or `<`.
const BARE_ADDRESSES: [&str; 4] = ["http://", "https://", "ftp://", "www."];

/// What ends such an address and would mean something on its own if comrak leaves it out of the address.
const ENDING_MARKERS: &[char] = &['*', '_', '~'];

/// The stretch of text without a space or `<` that an address in it would reach through, looked at once because one
/// text can start many addresses.
#[derive(Default)]
struct Unbroken {
    end: usize,
    /// All of it can be written as it stands.
    inert: bool,
}

impl Unbroken {
    fn at(text: &str, from: usize, after: char, table: bool, counts: Counts) -> Unbroken {
        let rest = &text[from..];
        let len = rest
            .find(|c: char| c.is_ascii_whitespace() || c == '<')
            .unwrap_or(rest.len());
        let stretch = &rest[..len];
        let reaches_on = if len == rest.len() {
            !after.is_ascii_whitespace() && after != '<'
        } else {
            rest[len..].starts_with('<')
        };
        Unbroken {
            end: from + len,
            inert: !reaches_on
                && !stretch.ends_with(ENDING_MARKERS)
                && !(table && stretch.contains('|'))
                && is_inert(stretch, counts),
        }
    }
}

/// At the start of a bare address, comrak takes all of it as it stands, backslashes too, up to the next space. So it is
/// written unchanged if nothing in it means anything; if something does, a backslash in its first part keeps it from
/// being an address and it goes on as text. Gives what to write for how much of `text`, if `text` starts an address.
fn bare_address(text: &str, prev: char, stretch: &Unbroken, stretch_left: usize) -> Option<(String, usize)> {
    let start = BARE_ADDRESSES.iter().find(|start| text.starts_with(**start))?;
    let may_start = if *start == "www." {
        prev.is_ascii_whitespace() || matches!(prev, '*' | '_' | '~' | '(' | '[')
    } else {
        !prev.is_ascii_alphabetic()
    };
    if !may_start {
        return None;
    }
    if stretch.inert {
        return Some((text[..stretch_left].to_string(), stretch_left));
    }
    let cut = start.len() - if start.ends_with("//") { 3 } else { 1 };
    Some((format!("{}\\{}", &start[..cut], &start[cut..]), start.len()))
}

/// Whether a stretch reads as itself, whether or not comrak makes a link of it; it does not inside open brackets.
fn is_inert(stretch: &str, counts: Counts) -> bool {
    let bytes = stretch.as_bytes();
    let behind_scheme = stretch.find("://").map_or(0, |at| at + 3);
    stretch.char_indices().all(|(at, c)| {
        let rest = &stretch[at..];
        match c {
            '-' | '.' | '/' | '?' | '#' | '+' | ',' | ';' | '@' | '!' | '\'' | '"' | '(' | ')' => true,
            '%' | '=' => !rest[1..].starts_with(c),
            '&' => !starts_reference(rest),
            ':' => at + 3 == behind_scheme || !closes_shortcode(rest),
            '_' => {
                at > 0
                    && bytes[at - 1].is_ascii_alphanumeric()
                    && bytes.get(at + 1).is_some_and(u8::is_ascii_alphanumeric)
            }
            '~' => counts.tilde == 1,
            c => !c.is_ascii_punctuation() && !c.is_control(),
        }
    })
}

/// Whether `text`, which starts with a `&`, starts a character reference.
fn starts_reference(text: &str) -> bool {
    decode_entity(&text[..text.floor_char_boundary(LONGEST_REFERENCE)]).is_some()
}

/// A number and its `.` or `)` at the start of a line would make a list; returns how much of `text` it has written.
fn escape_number(text: &str, out: &mut String) -> usize {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    let marker = text.as_bytes().get(digits);
    let space_behind = text[digits.min(text.len())..]
        .chars()
        .nth(1)
        .is_none_or(char::is_whitespace);
    if (1..=9).contains(&digits) && matches!(marker, Some(b'.' | b')')) && space_behind {
        out.push_str(&text[..digits]);
        out.push('\\');
        out.push_str(&text[digits..=digits]);
        digits + 1
    } else {
        0
    }
}

fn write_run(c: char, place: &Place, escape: Escape, within: Where, counts: Counts, out: &mut String) {
    let run = place.run;
    let starts_task = place.start && matches!(place.rest.as_bytes().get(1..3), Some(b" ]" | b"x]" | b"X]"));
    let next_bracket = place.next == ']';
    if c == '|' && within.table {
        // comrak takes the backslash out of `\|` before it reads the cell, where two of them pair as a spoiler.
        let written = if counts.pipe > 1 { "&#124;" } else { "\\|" };
        for _ in 0..run {
            out.push_str(written);
        }
        return;
    }
    if c == ']' && within.in_brackets {
        // A backslash in front would end a formula that an escaped `[` started, as comrak reads LaTeX's delimiters.
        out.push_str("&#93;");
        return;
    }
    if c == '[' && escape != Escape::No && counts.closes_formula {
        // comrak finds a task's brackets also behind a reference, but not behind a tag.
        if starts_task {
            out.push_str("<span></span>");
        }
        out.push_str("&#91;");
        return;
    }
    if c == '(' && escape != Escape::No {
        out.push_str("&#40;");
        return;
    }
    if c == '\\' && next_bracket {
        out.push_str(&"&#92;".repeat(run));
        return;
    }
    for n in 0..run {
        if escape == Escape::All || (escape == Escape::First && n == 0) {
            out.push('\\');
        }
        out.push(c);
    }
}

fn rule(c: char, place: &Place, within: Where, counts: Counts) -> Escape {
    match c {
        '\\' if place.run > 1 || place.next.is_ascii_punctuation() || place.next == '\n' => Escape::All,
        '`' => Escape::All,
        '*' if place.start || !place.between_spaces() => Escape::All,
        '_' if place.start || !(place.between_spaces() || place.inside_word()) => Escape::All,
        '~' if place.start && (place.run >= 3 || place.next.is_whitespace()) => Escape::All,
        '~' => place.pairs(counts.tilde),
        '^' if place.next == '[' => Escape::All,
        '^' => place.pairs(counts.caret),
        '$' if place.run > 1 => Escape::All,
        // comrak looks for ASCII spaces around dollars, and for any space around the rest.
        '$' if counts.closing_dollars == 0
            || (place.next.is_ascii_whitespace() && place.prev.is_ascii_whitespace()) =>
        {
            Escape::No
        }
        '$' if counts.dollar == 1 && !place.edge => Escape::No,
        '$' => Escape::All,
        '|' if place.start => Escape::All,
        '|' => place.pairs(counts.pipe),
        '=' if place.run == 2 && !place.between_spaces() => Escape::First,
        '=' if place.start && (place.next.is_whitespace() || place.next == '\n') => Escape::First,
        '%' if place.run > 1 => Escape::All,
        '<' if place.next.is_ascii_alphabetic()
            || matches!(place.next, '/' | '!' | '?')
            || (counts.at_sign && !place.next.is_whitespace()) =>
        {
            Escape::All
        }
        '>' if place.start => Escape::First,
        '&' if starts_reference(place.rest) => Escape::First,
        '[' if place.start
            || within.in_brackets
            || matches!(place.next, '[' | '^')
            || counts.closing_brackets > 0
            || (place.prev == ']' && place.edge) =>
        {
            Escape::All
        }
        // Right behind a footnote reference, it would read as the address of a link around it.
        '(' if place.prev == ']' && place.edge => Escape::All,
        '!' if place.next == '[' && place.edge => Escape::All,
        '#' if place.start && place.run <= 6 && place.next.is_whitespace() => Escape::First,
        '#' if within.heading && place.next == '\n' && !place.prev.is_alphanumeric() => Escape::First,
        '+' if place.start && place.next.is_whitespace() => Escape::First,
        '-' if place.start && !place.next.is_alphanumeric() => Escape::First,
        ':' if place.start && !place.next.is_alphanumeric() => Escape::First,
        ':' if (place.prev == ']' && place.edge) || closes_shortcode(place.rest) => Escape::First,
        '{' if within.heading && (place.edge || place.brace_behind) => Escape::First,
        _ => Escape::No,
    }
}

/// More bytes than a character reference, `&CounterClockwiseContourIntegral;`, can have.
const LONGEST_REFERENCE: usize = 40;
const LONGEST_SHORTCODE: usize = 48;

/// Whether `rest`, which starts with a `:`, starts a shortcode comrak reads as an emoji.
fn closes_shortcode(rest: &str) -> bool {
    let name_len = rest[1..]
        .bytes()
        .take(LONGEST_SHORTCODE + 1)
        .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'-'))
        .count();
    name_len > 0
        && name_len <= LONGEST_SHORTCODE
        && rest.as_bytes().get(name_len + 1) == Some(&b':')
        && emojis::get_by_shortcode(&rest[1..=name_len]).is_some()
}

/// A link destination that reads as `url`, in the form that needs the fewest changes to it.
pub(super) fn destination(url: &str, table: bool) -> String {
    let url = url.trim();
    let mut plain = String::with_capacity(url.len());
    let mut angled = String::with_capacity(url.len() + 2);
    angled.push('<');
    let mut depth = 0usize;
    let mut bare = !url.starts_with('<');
    for (at, c) in url.char_indices() {
        let next = url[at + c.len_utf8()..].chars().next();
        match c {
            '\\' if next.is_none_or(|n| n.is_ascii_punctuation()) => {
                plain.push_str("\\\\");
                angled.push_str("\\\\");
            }
            // comrak decodes references in an address before it takes out backslashes.
            '&' if starts_reference(&url[at..]) => {
                plain.push_str("&amp;");
                angled.push_str("&amp;");
            }
            '<' | '>' => {
                bare = false;
                plain.push(c);
                angled.push('\\');
                angled.push(c);
            }
            '|' if table => {
                plain.push_str("%7C");
                angled.push_str("%7C");
            }
            c if c.is_control() => {
                let encoded = percent(c);
                plain.push_str(&encoded);
                angled.push_str(&encoded);
            }
            ' ' => {
                bare = false;
                plain.push(c);
                angled.push(c);
            }
            '(' | ')' => {
                if c == '(' {
                    depth += 1;
                } else if depth == 0 {
                    bare = false;
                } else {
                    depth -= 1;
                }
                bare &= depth <= MOST_PARENTHESES;
                plain.push(c);
                angled.push(c);
            }
            c => {
                plain.push(c);
                angled.push(c);
            }
        }
    }
    if bare && depth == 0 {
        plain
    } else {
        angled.push('>');
        angled
    }
}

/// comrak stops reading a destination at more parentheses within each other.
const MOST_PARENTHESES: usize = 16;

fn percent(c: char) -> String {
    let mut bytes = [0; 4];
    c.encode_utf8(&mut bytes)
        .bytes()
        .map(|byte| format!("%{byte:02X}"))
        .collect()
}
