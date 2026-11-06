//! Searching the notes of a tree for text, for a tag and for the links to a note.

use std::io::{self, Read};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use caseless::Caseless;
use splat_markdown::doc::{Doc, Inline, InlineKind, walk_blocks};
use splat_markdown::reader::parse;
use splat_markdown::text::{label_key, offsets};
use splat_markdown::typography::{TextMark, before_text, marks};
use unicode_normalization::char::{canonical_combining_class, decompose_canonical};

use crate::links::{LinkTarget, links_of};
use crate::resolve::Resolver;
use crate::tree::Tree;

/// What to search for. Text folds case by Unicode's default rules, which have no language: `ß` finds `SS` and the other
/// way round, `I` finds `i` but not the Turkish `ı`, and `İ` finds `i̇`. In either mode a character finds its other
/// normal form, composed or decomposed. A match starts and ends between whole characters, so `s` does not find half of
/// `ß`, nor `a` the start of `ä`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Query {
    /// Text, on any line. A line break in `needle` has to match one in the file as it is written.
    Text {
        /// What to find; nothing is found for nothing.
        needle: String,
        /// Whether case counts; the normal form never does.
        case_sensitive: bool,
    },
    /// `project` also finds `#project/splat`, but not `#projects`; a leading `#` makes no difference, nor does case.
    Tag(String),
    /// Notes that link to this one. Links in blocks of embedded HTML are not seen.
    LinksTo(PathBuf),
}

/// A place in a note.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hit {
    /// Counted from 0.
    pub line: usize,
    /// Where the match starts in its line, in bytes: unlike `range` this also holds in a document's text, which has neither
    /// the file's byte order mark nor always its line ends.
    pub column: usize,
    /// Bytes of the match in the file.
    pub range: Range<usize>,
    /// The line shortened to about 160 chars around the match, on char boundaries.
    pub context: String,
    /// The match in `context`, as far as it fits there.
    pub in_context: Range<usize>,
}

/// The places in one note.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FileHits {
    /// Where the note lies.
    pub path: PathBuf,
    /// In the order of the text.
    pub hits: Vec<Hit>,
    /// More than `MOST_HITS_PER_FILE` stood there.
    pub more: bool,
}

/// The most places reported for one note.
pub const MOST_HITS_PER_FILE: usize = 200;

/// A note above this size is skipped.
pub(crate) const MOST_FILE_BYTES: u64 = 32 << 20;

const MOST_CONTEXT_CHARS: usize = 160;

/// Collects places in increasing order, going through the text between them once to find their lines.
struct Places<'t> {
    text: &'t str,
    hits: Vec<Hit>,
    more: bool,
    line: usize,
    line_start: usize,
    scanned: usize,
}

impl<'t> Places<'t> {
    fn new(text: &'t str) -> Places<'t> {
        let start = text.strip_prefix('\u{FEFF}').map_or(0, |_| '\u{FEFF}'.len_utf8());
        Places {
            text,
            hits: Vec::new(),
            more: false,
            line: 0,
            line_start: start,
            scanned: start,
        }
    }

    /// Takes a place, as long as there is room for it.
    fn add(&mut self, range: Range<usize>) -> bool {
        if self.hits.len() == MOST_HITS_PER_FILE {
            self.more = true;
            return false;
        }
        let bytes = self.text.as_bytes();
        for at in self.scanned..range.start {
            if bytes[at] == b'\n' || (bytes[at] == b'\r' && bytes.get(at + 1) != Some(&b'\n')) {
                self.line += 1;
                self.line_start = at + 1;
            }
        }
        self.scanned = range.start.max(self.scanned);
        let (context, in_context) = context_of(self.text, &range, self.line_start);
        self.hits.push(Hit {
            line: self.line,
            column: range.start.saturating_sub(self.line_start),
            range,
            context,
            in_context,
        });
        true
    }
}

/// The line around `range`, which starts at `line_start`, shortened to at most `MOST_CONTEXT_CHARS`, and where the
/// match stands in it. A match goes first; what is left is shared by both sides, and one side takes over what the other
/// cannot use.
fn context_of(text: &str, range: &Range<usize>, line_start: usize) -> (String, Range<usize>) {
    let is_line_break = |c: &char| matches!(c, '\n' | '\r');
    let mut end = range.start;
    let mut room = MOST_CONTEXT_CHARS;
    for c in text[range.clone()].chars().take(room).take_while(|c| !is_line_break(c)) {
        end += c.len_utf8();
        room -= 1;
    }
    let before = text[line_start..range.start]
        .chars()
        .rev()
        .take_while(|c| !is_line_break(c))
        .take(room);
    let after = text[end..].chars().take_while(|c| !is_line_break(c)).take(room);
    let (has_before, has_after) = (before.clone().count(), after.clone().count());
    let shown_before = has_before.min(room - has_after.min(room / 2));
    let shown_after = has_after.min(room - shown_before);
    let start = range.start - before.take(shown_before).map(char::len_utf8).sum::<usize>();
    let stop = end + after.take(shown_after).map(char::len_utf8).sum::<usize>();
    (text[start..stop].to_string(), range.start - start..end - start)
}

/// The characters a searched text is compared by, in the order they come, each with the place in the source it stands
/// for and whether a match may begin or end in front of it.
struct Element {
    c: char,
    at: usize,
    boundary: bool,
}

/// Decompositions and case folds to be compared: a canonical caseless match by Unicode's definition D145, without the
/// case fold if it is not wanted.
fn foldings(c: char, case_sensitive: bool, out: &mut Vec<char>) {
    if case_sensitive {
        decompose_canonical(c, |part| out.push(part));
    } else {
        decompose_canonical(c, |part| {
            for folded in std::iter::once(part).default_case_fold() {
                decompose_canonical(folded, |again| out.push(again));
            }
        });
    }
}

/// The vowels and final consonants of Hangul, which a composed syllable holds in itself.
fn continues_a_syllable(c: char) -> bool {
    matches!(c, '\u{1160}'..='\u{11FF}' | '\u{D7B0}'..='\u{D7FF}')
}

/// Puts decomposed characters into the order Unicode defines: what combines stays with its base, and marks of different
/// classes sort by class whatever order they came in.
#[derive(Default)]
struct Sequence {
    pending: Vec<Element>,
    parts: Vec<char>,
}

impl Sequence {
    fn take(&mut self, c: char, at: usize, case_sensitive: bool, sink: &mut impl FnMut(Element)) {
        self.parts.clear();
        foldings(c, case_sensitive, &mut self.parts);
        for (index, &part) in self.parts.iter().enumerate() {
            let starter = canonical_combining_class(part) == 0;
            if starter {
                Self::flush(&mut self.pending, sink);
            }
            self.pending.push(Element {
                c: part,
                at,
                boundary: index == 0 && starter && !continues_a_syllable(part),
            });
        }
    }

    fn flush(pending: &mut Vec<Element>, sink: &mut impl FnMut(Element)) {
        let behind_base = pending
            .first()
            .map_or(0, |first| usize::from(canonical_combining_class(first.c) == 0));
        pending[behind_base..].sort_by_key(|element| canonical_combining_class(element.c));
        pending.drain(..).for_each(sink);
    }
}

/// A text search that has been made ready: the needle in the form the text is compared in, with what Knuth, Morris and
/// Pratt need to go through any text in linear time.
struct Pattern {
    chars: Vec<char>,
    falls_back: Vec<usize>,
    case_sensitive: bool,
}

impl Pattern {
    fn new(needle: &str, case_sensitive: bool) -> Pattern {
        let mut chars = Vec::with_capacity(needle.len());
        let mut sequence = Sequence::default();
        for c in needle.chars() {
            sequence.take(c, 0, case_sensitive, &mut |element| chars.push(element.c));
        }
        Sequence::flush(&mut sequence.pending, &mut |element| chars.push(element.c));
        let mut falls_back = vec![0; chars.len()];
        let mut matched = 0;
        for at in 1..chars.len() {
            while matched > 0 && chars[at] != chars[matched] {
                matched = falls_back[matched - 1];
            }
            if chars[at] == chars[matched] {
                matched += 1;
            }
            falls_back[at] = matched;
        }
        Pattern {
            chars,
            falls_back,
            case_sensitive,
        }
    }

    /// The first byte the text can show of a match, if that is ASCII; lets the search run ahead between candidates.
    fn first_ascii(&self) -> Option<u8> {
        self.chars
            .first()
            .and_then(|c| u8::try_from(*c).ok())
            .filter(u8::is_ascii)
    }
}

/// Goes through elements of a text and reports where the pattern stands between boundaries, without overlaps. A
/// candidate is only known to be a match at the element behind it.
struct Matcher<'p, F> {
    pattern: &'p Pattern,
    /// Place and boundary of the last elements, as long as the pattern is.
    recent: Vec<(usize, bool)>,
    seen: usize,
    matched: usize,
    candidate: Option<usize>,
    not_before: usize,
    report: F,
    done: bool,
}

impl<F: FnMut(Range<usize>) -> bool> Matcher<'_, F> {
    fn push(&mut self, element: Element) {
        if let Some(start) = self.candidate.take()
            && element.boundary
        {
            self.not_before = element.at;
            self.done = !(self.report)(start..element.at);
        }
        let size = self.pattern.chars.len();
        self.recent[self.seen % size] = (element.at, element.boundary);
        self.seen += 1;
        let mut matched = self.matched;
        while matched > 0 && self.pattern.chars[matched] != element.c {
            matched = self.pattern.falls_back[matched - 1];
        }
        if self.pattern.chars[matched] == element.c {
            matched += 1;
        }
        if matched == size {
            let (start, boundary) = self.recent[self.seen % size];
            if boundary && start >= self.not_before {
                self.candidate = Some(start);
            }
            matched = self.pattern.falls_back[size - 1];
        }
        self.matched = matched;
    }

    fn is_idle(&self) -> bool {
        self.matched == 0 && self.candidate.is_none()
    }
}

/// Calls `report` for the matches of `pattern` in `text` behind `start`, until it says to stop.
fn find_text(text: &str, start: usize, pattern: &Pattern, report: impl FnMut(Range<usize>) -> bool) {
    if pattern.chars.is_empty() {
        return;
    }
    let mut matcher = Matcher {
        pattern,
        recent: vec![(0, false); pattern.chars.len()],
        seen: 0,
        matched: 0,
        candidate: None,
        not_before: 0,
        report,
        done: false,
    };
    let bytes = text.as_bytes();
    let first_ascii = pattern.first_ascii();
    let case_sensitive = pattern.case_sensitive;
    let fold = move |b: u8| if case_sensitive { b } else { b.to_ascii_lowercase() };
    let mut sequence = Sequence::default();
    let mut at = start;
    while at < bytes.len() && !matcher.done {
        let b = bytes[at];
        if b < 0x80 {
            Sequence::flush(&mut sequence.pending, &mut |element| matcher.push(element));
            if matcher.is_idle() {
                // Nothing is under way, so the search can run up to the next character that may start a match.
                let ahead = bytes[at..]
                    .iter()
                    .position(|&b| b >= 0x80 || first_ascii == Some(fold(b)));
                at += ahead.unwrap_or(bytes.len() - at);
                if ahead.is_none() || bytes[at] >= 0x80 {
                    continue;
                }
            }
            matcher.push(Element {
                c: char::from(fold(bytes[at])),
                at,
                boundary: true,
            });
            at += 1;
        } else {
            let c = text[at..].chars().next().unwrap_or(' ');
            sequence.take(c, at, case_sensitive, &mut |element| matcher.push(element));
            at += c.len_utf8();
        }
    }
    Sequence::flush(&mut sequence.pending, &mut |element| matcher.push(element));
    if !matcher.done
        && let Some(found) = matcher.candidate.take()
    {
        (matcher.report)(found..bytes.len());
    }
}

/// Whether the tag `name` is what a search for `wanted` asks for, both in the form `label_key` gives them.
fn tag_matches(name: &str, wanted: &str) -> bool {
    name.strip_prefix(wanted)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// The tags in `inlines` that a search for `wanted` asks for, as ranges of the text: as `typography` marks them, so
/// neither in code nor in a link.
fn tags_in(text: &str, inlines: &[Inline], wanted: &str, out: &mut Vec<Range<usize>>) {
    for inline in inlines {
        match &inline.kind {
            InlineKind::Text(literal) => {
                let source = offsets(&inline.range);
                let spelled = text.get(source.clone()).unwrap_or("");
                let mut from = 0;
                for (shown, mark) in marks(literal, before_text(text, inline.range.start), false) {
                    let TextMark::Tag(name) = mark else {
                        continue;
                    };
                    let tag = &literal[shown];
                    let found = spelled.get(from..).and_then(|rest| rest.find(tag)).map(|at| from + at);
                    let range = match found {
                        Some(at) => source.start + at..source.start + at + tag.len(),
                        None => source.clone(),
                    };
                    from = range.end - source.start;
                    if tag_matches(&label_key(&name), wanted) {
                        out.push(range);
                    }
                }
            }
            InlineKind::Link { .. } | InlineKind::Image { .. } | InlineKind::WikiLink { .. } | InlineKind::Kbd => {}
            kind if kind.holds_inlines() => tags_in(text, &inline.children, wanted, out),
            _ => {}
        }
    }
}

enum Prepared<'t> {
    Text(Pattern),
    Tag(String),
    LinksTo(Resolver<'t>, &'t Path),
}

impl<'t> Prepared<'t> {
    fn new(query: &'t Query, tree: &'t Tree) -> Prepared<'t> {
        match query {
            Query::Text { needle, case_sensitive } => Prepared::Text(Pattern::new(needle, *case_sensitive)),
            Query::Tag(tag) => {
                let tag = tag.trim();
                let tag = tag.strip_prefix('#').unwrap_or(tag).trim_end_matches('/');
                Prepared::Tag(label_key(tag))
            }
            Query::LinksTo(target) => Prepared::LinksTo(Resolver::new(tree), target),
        }
    }

    fn places_in(&self, text: &str, file: &Path) -> (Vec<Hit>, bool) {
        let mut places = Places::new(text);
        match self {
            Prepared::Text(pattern) => find_text(text, places.scanned, pattern, |range| places.add(range)),
            Prepared::Tag(wanted) => {
                if !wanted.is_empty() && text.contains('#') {
                    let doc = parse(text);
                    let mut found = Vec::new();
                    walk_blocks(doc.blocks(), &mut |block| {
                        tags_in(text, block.inlines(), wanted, &mut found)
                    });
                    add_all(&mut places, found);
                }
            }
            Prepared::LinksTo(resolver, target) => {
                if text.contains('[') {
                    add_all(&mut places, links_to(&parse(text), resolver, file, target));
                }
            }
        }
        (places.hits, places.more)
    }
}

fn links_to(doc: &Doc, resolver: &Resolver, file: &Path, target: &Path) -> Vec<Range<usize>> {
    links_of(doc)
        .into_iter()
        .filter(|link| {
            let leads_to = match &link.says {
                LinkTarget::Wiki { target, .. } => resolver.wiki(target, file),
                LinkTarget::Address { url, .. } => resolver.address(url, file),
            };
            leads_to.is_some_and(|path| resolver.same(&path, target))
        })
        .map(|link| link.whole)
        .collect()
}

fn add_all(places: &mut Places, mut ranges: Vec<Range<usize>>) {
    ranges.sort_by_key(|range| range.start);
    for range in ranges {
        if !places.add(range) {
            return;
        }
    }
}

/// What a note holds as text, or `None` if it is no regular file within the limit in valid UTF-8; the look at the file
/// comes first because opening a named pipe would never return.
fn read_note(path: &Path) -> io::Result<Option<String>> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() || meta.len() > MOST_FILE_BYTES {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MOST_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MOST_FILE_BYTES {
        return Ok(None);
    }
    Ok(String::from_utf8(bytes).ok())
}

/// What a search went through, beyond what it found.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Searched {
    /// How many notes could not be read, and so were passed over.
    pub unreadable: usize,
}

/// Goes through the notes of `tree` in the order they show and calls `found` for each that holds what `query` asks for.
/// Files over 32 MiB or not in UTF-8 are skipped, and each file is read once.
pub fn search(tree: &Tree, query: &Query, stop: &AtomicBool, found: &mut dyn FnMut(FileHits)) -> Searched {
    let prepared = Prepared::new(query, tree);
    let mut searched = Searched::default();
    for note in tree.notes() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let text = match read_note(&note.path) {
            Ok(Some(text)) => text,
            Ok(None) => continue,
            Err(_) => {
                searched.unreadable += 1;
                continue;
            }
        };
        let (hits, more) = prepared.places_in(&text, &note.path);
        if !hits.is_empty() {
            found(FileHits {
                path: note.path.clone(),
                hits,
                more,
            });
        }
    }
    searched
}

/// What `search` finds in the note `file` when it holds `text`, without a look at the disk.
pub fn hits_in_text(text: &str, query: &Query, file: &Path, tree: &Tree) -> FileHits {
    let (hits, more) = Prepared::new(query, tree).places_in(text, file);
    FileHits {
        path: file.to_path_buf(),
        hits,
        more,
    }
}

#[cfg(test)]
pub(crate) mod tests;
