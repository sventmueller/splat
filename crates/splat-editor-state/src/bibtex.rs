//! A BibTeX bibliography as the citations of the preview read it: each entry as its author and year.

use std::collections::HashMap;

/// The fields of an entry that its citation shows.
const CITED: [&str; 4] = ["author", "editor", "year", "date"];

/// The most of a value that is kept: a citation shows two names at most, and a value of thousands of names or of strings
/// joined again and again would otherwise cost time and memory for nothing.
const MOST_VALUE: usize = 4096;

/// BibTeX entries as `key → "Author Year"`, from their author or editor and year or date fields. An `@` starts an entry only
/// between entries, so one in an address or a note stays part of its field; `@comment` and `@preamble` hold no entry, and
/// `@string` defines what a bare word in a field stands for.
pub fn parse_bibtex(text: &str) -> HashMap<String, String> {
    let mut strings = Strings {
        known: HashMap::new(),
        room: text.len(),
    };
    let mut out = HashMap::new();
    for (kind, body) in entries(text) {
        match kind.to_ascii_lowercase().as_str() {
            "comment" | "preamble" => {}
            "string" => {
                for (name, value) in fields(body, &strings, None) {
                    strings.define(name, value);
                }
            }
            _ => {
                let Some((key, rest)) = body.split_once(',') else {
                    continue;
                };
                let shown = citation(&fields(rest, &strings, Some(CITED.as_slice())).into_iter().collect());
                if !shown.is_empty() {
                    out.insert(key.trim().to_string(), shown);
                }
            }
        }
    }
    out
}

/// What the `@string` definitions read so far stand for.
#[derive(Default)]
struct Strings {
    known: HashMap<String, String>,
    /// How many bytes more the definitions may hold: no more than the text has, however often they join each other.
    room: usize,
}

impl Strings {
    fn get(&self, word: &str) -> Option<&str> {
        self.known.get(&word.to_ascii_lowercase()).map(String::as_str)
    }

    fn define(&mut self, name: String, mut value: String) {
        value.truncate(value.floor_char_boundary(self.room));
        self.room -= value.len();
        self.known.insert(name, value);
    }
}

/// The entries of a bibliography as their type and what stands between their delimiters, `{}` or `()`.
fn entries(text: &str) -> Vec<(&str, &str)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = text[from..].find('@').map(|at| from + at) {
        let after = &text[at + 1..];
        let kind_len = after
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(after.len());
        let rest = after[kind_len..].trim_start();
        let open = at + 1 + (after.len() - rest.len());
        let close = match rest.chars().next() {
            Some('{') => '}',
            Some('(') => ')',
            _ => {
                from = at + 1;
                continue;
            }
        };
        let body_start = open + 1;
        let end = closing(&text[body_start..], close).map_or(text.len(), |n| body_start + n);
        found.push((&after[..kind_len], &text[body_start..end]));
        from = (end + 1).min(text.len());
    }
    found
}

/// Where the entry that `body` starts closes with `close`: braces inside it nest, and a quoted value holds a `)`.
fn closing(body: &str, close: char) -> Option<usize> {
    let (mut depth, mut quoted) = (0usize, false);
    for (i, c) in body.char_indices() {
        match c {
            '{' => depth += 1,
            '}' if depth == 0 && close == '}' => return Some(i),
            '}' => depth = depth.saturating_sub(1),
            '"' if depth == 0 => quoted = !quoted,
            ')' if depth == 0 && !quoted && close == ')' => return Some(i),
            _ => {}
        }
    }
    None
}

/// The fields `name = value, …` of an entry body, names in lower case, in their order: all of them, or the first of each
/// name in `wanted`, whose values alone are put together.
fn fields(body: &str, strings: &Strings, wanted: Option<&[&str]>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut rest = body;
    loop {
        rest = rest.trim_start_matches(|c: char| c == ',' || c.is_whitespace());
        let Some(equals) = rest.find(['=', ',']) else {
            return out;
        };
        let name = rest[..equals].trim().to_ascii_lowercase();
        if !rest[equals..].starts_with('=') {
            rest = &rest[equals..];
            continue;
        }
        let keep = !name.is_empty()
            && wanted.is_none_or(|wanted| wanted.contains(&name.as_str()) && out.iter().all(|(seen, _)| *seen != name));
        let (value, behind) = value(&rest[equals + 1..], strings, keep);
        if keep {
            out.push((name, value));
        }
        rest = behind;
    }
}

/// A field value up to the comma that ends it: braced or quoted parts and bare words, joined by `#`. Returns the value
/// without its braces, empty unless `keep`, and what follows it.
fn value<'a>(text: &'a str, strings: &Strings, keep: bool) -> (String, &'a str) {
    let mut joined = String::new();
    let mut rest = text.trim_start();
    loop {
        let (part, behind) = match rest.chars().next() {
            Some('{') => delimited(&rest[1..], '}'),
            Some('"') => delimited(&rest[1..], '"'),
            _ => {
                let end = rest.find([',', '#', '}', ')', '\n']).unwrap_or(rest.len());
                let word = rest[..end].trim();
                let known = if keep { strings.get(word) } else { None };
                (known.unwrap_or(word), &rest[end..])
            }
        };
        if keep {
            let room = MOST_VALUE.saturating_sub(joined.len());
            joined.push_str(&part[..part.floor_char_boundary(room)]);
        }
        rest = behind.trim_start();
        match rest.strip_prefix('#') {
            Some(more) => rest = more.trim_start(),
            None => return (joined.replace(['{', '}'], "").trim().to_string(), rest),
        }
    }
}

/// What stands before `close` at the outer level of `text`, and what follows `close`.
fn delimited(text: &str, close: char) -> (&str, &str) {
    let mut depth = 0usize;
    for (i, c) in text.char_indices() {
        match c {
            '{' => depth += 1,
            '}' if depth > 0 => depth -= 1,
            c if c == close && depth == 0 => return (&text[..i], &text[i + c.len_utf8()..]),
            _ => {}
        }
    }
    (text, "")
}

/// `Author Year` as a citation shows an entry: one or two last names, or the first with "et al.".
fn citation(fields: &HashMap<String, String>) -> String {
    let people = fields.get("author").or_else(|| fields.get("editor"));
    let names: Vec<&str> = people
        .map(|people| {
            people
                .split(" and ")
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(|p| match p.split_once(',') {
                    Some((last, _)) => last.trim(),
                    None => p.rsplit(' ').next().unwrap_or(p),
                })
                .collect()
        })
        .unwrap_or_default();
    let year = fields
        .get("year")
        .cloned()
        .or_else(|| fields.get("date").map(|d| d.chars().take(4).collect()));
    let who = match names.as_slice() {
        [] => String::new(),
        [one] => (*one).to_string(),
        [a, b] => format!("{a} & {b}"),
        [a, ..] => format!("{a} et al."),
    };
    [who, year.unwrap_or_default()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bibtex_entries_read_as_author_and_year() {
        let entries = parse_bibtex(
            "@article{doe2020,\n  author = {Doe, Jane and Roe, Rick},\n  year = 2020,\n  title = {A {B} c}\n}\n\
             @book{solo, author = \"Max Mustermann\", date = {2019-04-01}}\n\
             @misc{many, author = {A, a and B, b and C, c}, year = {2021}}",
        );
        assert_eq!(entries["doe2020"], "Doe & Roe 2020");
        assert_eq!(entries["solo"], "Mustermann 2019");
        assert_eq!(entries["many"], "A et al. 2021");
        let odd = parse_bibtex(
            "@comment no braces\n@misc{nofields}\n@misc{bare, author = Plain,\n year = 1999\n}\n\
             @misc{nested, title = {author = no}, editor = {Ed, E.}, year = {2001}}",
        );
        assert_eq!(odd["bare"], "Plain 1999");
        assert_eq!(odd["nested"], "Ed 2001");
        assert!(!odd.contains_key("nofields"));
    }

    #[test]
    fn an_at_sign_inside_a_field_belongs_to_the_field() {
        let entries = parse_bibtex(
            "@article{mail,\n  author = {Doe, Jane},\n  email = {jane@example.org},\n  url = {https://x.org/@doe},\n  \
             note = \"write to a@b.c\",\n  year = 2020\n}\n@book{next, author = {Roe, R.}, year = 1999}",
        );
        assert_eq!(entries["mail"], "Doe 2020", "the year behind the address counts");
        assert_eq!(entries["next"], "Roe 1999", "and the entry behind it stays one");
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn comments_strings_and_preambles_are_no_entries() {
        let entries = parse_bibtex(
            "@COMMENT{@book{hidden, author = {X, Y}, year = 1}}\n\
             @preamble{\"\\newcommand{\\x}{@y}\"}\n\
             @String{doe = \"Doe, Jane\"}\n@string(first = {Ada})\n\
             @book{named, Author = doe, YEAR = 2001}\n\
             @book{joined, author = first # \" Lovelace\", year = 1843}",
        );
        assert!(!entries.contains_key("hidden"));
        assert_eq!(entries["named"], "Doe 2001");
        assert_eq!(entries["joined"], "Lovelace 1843");
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn parentheses_delimit_an_entry_as_braces_do() {
        let entries = parse_bibtex(
            "@book(paren, author = {Roe, R.}, title = \"a (b) c\", note = {x) y}, year = 1999)\n\
             @misc{after, author = {Ada, A.}, year = 1843}",
        );
        assert_eq!(entries["paren"], "Roe 1999");
        assert_eq!(entries["after"], "Ada 1843");
    }

    #[test]
    fn strings_that_join_each_other_hold_no_more_than_the_text() {
        let mut text = String::from("@string{s0 = \"Doe, Jane and Roe, Rick and \"}\n");
        for n in 1..64 {
            text.push_str(&format!("@string{{s{n} = s{m} # s{m}}}\n", m = n - 1));
        }
        text.push_str("@book{early, author = s2, year = 1}\n@book{late, author = s63 # \" Ada\", year = 2}");
        let entries = parse_bibtex(&text);
        assert_eq!(entries["early"], "Doe et al. 1");
        assert!(entries["late"].ends_with(" 2"), "{}", entries["late"]);
        assert!(entries["late"].len() < MOST_VALUE, "a citation, not the joined strings");
    }

    #[test]
    fn a_value_is_read_to_its_end_but_kept_only_as_far_as_a_citation_needs() {
        let many: Vec<String> = (0..2000).map(|n| format!("Name{n}, Ä.")).collect();
        let text = format!(
            "@article{{big, abstract = {{{}}}, author = {{{}}}, author = {{Second, S.}}, year = 2020}}\n\
             @book{{next, editor = {{Ed, E.}}, year = 1999}}",
            "x".repeat(100_000),
            many.join(" and "),
        );
        let entries = parse_bibtex(&text);
        assert_eq!(entries["big"], "Name0 et al. 2020", "the first author field counts");
        assert_eq!(entries["next"], "Ed 1999");
        let long = format!("{{{}}}, rest", "ä".repeat(MOST_VALUE));
        let (skipped, behind) = value(&long, &Strings::default(), false);
        assert_eq!((skipped.as_str(), behind), ("", ", rest"));
        let (kept, _) = value(&format!("{{a{}}}", "ä".repeat(MOST_VALUE)), &Strings::default(), true);
        assert_eq!(kept.len(), MOST_VALUE - 1, "cut between two letters");
    }

    #[test]
    fn an_entry_that_never_closes_ends_with_the_text() {
        let entries = parse_bibtex("@book{open, author = {Kant, I.}, year = 1781");
        assert_eq!(entries["open"], "Kant 1781");
        assert_eq!(
            parse_bibtex("@book{k, author = {Kant, I.")["k"],
            "Kant",
            "nor does its value"
        );
        assert!(parse_bibtex("@").is_empty());
        assert_eq!(parse_bibtex("@book{k, = x, junk, author = \"Solo\"}")["k"], "Solo");
    }
}
