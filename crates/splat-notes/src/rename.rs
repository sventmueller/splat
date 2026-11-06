//! What renaming a note changes in the notes around it: each link that led to it, and only the name in that link. Links
//! in embedded HTML stay as they are, since the tree does not read them: they are a known limit, as is a wiki link to a
//! name it cannot hold.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use splat_markdown::reader::parse;
use splat_markdown::text::decode_entity;

use crate::links::{LinkTarget, Written, cleaned, links_of, unit_at};
use crate::resolve::{Resolver, tidied};
use crate::tree::Tree;

/// A part of a text and what takes its place.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Replacement {
    /// In bytes of the text as `read` gave it.
    pub range: Range<usize>,
    /// What takes its place.
    pub text: String,
}

/// What changes in one note, in the order of its text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Rewrite {
    /// The note as it is called now, which for the renamed note itself is its old name.
    pub path: PathBuf,
    /// In increasing order, with none inside another.
    pub replacements: Vec<Replacement>,
}

/// `text` with `replacements` made. One that does not fit, because it lies outside the text, inside a letter or inside
/// another one, is left out.
pub fn applied(text: &str, replacements: &[Replacement]) -> String {
    let mut sorted: Vec<&Replacement> = replacements.iter().collect();
    sorted.sort_by_key(|replacement| replacement.range.start);
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for replacement in sorted {
        let range = &replacement.range;
        if range.start >= at && text.get(range.clone()).is_some() {
            out.push_str(&text[at..range.start]);
            out.push_str(&replacement.text);
            at = range.end;
        }
    }
    out.push_str(&text[at..]);
    out
}

/// The name of a file as the address of a link writes it. The form of the link decides what needs an escape: in `<a
/// b.md>` a space does not, and non-ASCII letters take one only if the old name had them escaped too. A `%` and what
/// ends a path always do, as the app decodes addresses whatever their form.
fn in_address(name: &str, old: &str, angle: bool) -> String {
    let old_bytes = old.as_bytes();
    let escaped_letters = old_bytes.windows(3).any(|three| {
        three[0] == b'%'
            && matches!(three[1], b'8' | b'9' | b'a'..=b'f' | b'A'..=b'F')
            && char::from(three[2]).is_ascii_hexdigit()
    });
    let mut out = String::with_capacity(name.len());
    for (at, c) in name.char_indices() {
        let needs_escape = match c {
            '%' | '#' | '?' | '\\' | '<' | '>' => true,
            ' ' | '(' | ')' => !angle,
            '&' => decode_entity(&name[at..]).is_some(),
            c if c.is_control() => true,
            c if !c.is_ascii() => escaped_letters,
            _ => false,
        };
        if needs_escape {
            let mut utf8 = [0; 4];
            for byte in c.encode_utf8(&mut utf8).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Where the file name stands in an address as written: behind its last `/`, in front of `?` and `#`.
fn file_name_in(raw: &str) -> Option<Range<usize>> {
    let (mut at, mut start) = (0, 0);
    while let Some((len, c)) = unit_at(&raw[at..]) {
        if raw[at..]
            .get(..3)
            .is_some_and(|three| three.eq_ignore_ascii_case("%2F"))
        {
            start = at + 3;
            at += 3;
            continue;
        }
        match c {
            '?' | '#' => break,
            '/' => start = at + len,
            _ => {}
        }
        at += len;
    }
    (start < at).then_some(start..at)
}

/// Where the name stands in the source between the brackets of a wiki link, without spaces around it, and where its
/// last part starts.
fn name_in(raw: &str) -> Option<(Range<usize>, usize)> {
    let (mut at, mut last) = (0, None);
    while let Some((len, c)) = unit_at(&raw[at..]) {
        match c {
            '#' | '|' | '^' => break,
            '/' => last = Some(at + len),
            _ => {}
        }
        at += len;
    }
    let shown = &raw[..at];
    let start = at - shown.trim_start().len();
    let end = shown.trim_end().len();
    (start < end).then(|| (start..end, last.unwrap_or(start).clamp(start, end)))
}

/// Whether a wiki link can name `name` so that it reads as that again. No escape helps with `#`, `|` and `^`: the name
/// is cut at them after its escapes are read.
fn wiki_can_hold(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['[', ']', '|', '#', '^', '\\', '\n', '\r'])
        && !name.contains("..")
        && name.trim() == name
        && !name.starts_with('/')
        && name
            .match_indices('&')
            .all(|(at, _)| decode_entity(&name[at..]).is_none())
}

/// What a wiki link writes for a note as a path from the folder of the link; `.md` goes without saying.
fn wiki_path(note: &Path, dir: &Path) -> Option<String> {
    let inside = note.strip_prefix(dir).ok()?.to_str()?;
    let without = inside
        .len()
        .checked_sub(3)
        .filter(|&at| inside.get(at..).is_some_and(|end| end.eq_ignore_ascii_case(".md")));
    Some(without.map_or(inside, |at| &inside[..at]).to_string())
}

/// One rename of a note, and how the tree answers before and after it.
struct Plan<'t> {
    before: Resolver<'t>,
    after: Resolver<'t>,
    from: PathBuf,
    to: &'t Path,
    from_name: &'t str,
    to_name: &'t str,
}

impl Plan<'_> {
    /// What the last part of a wiki link's name becomes, and how much of it changes: an extension written stays as
    /// written.
    fn wiki_name(&self, last_part: &str) -> (Range<usize>, String) {
        let extension_of = |name: &str| Path::new(name).extension().map(|e| e.len() + 1).unwrap_or(0);
        let (from_extension, to_extension) = (extension_of(self.from_name), extension_of(self.to_name));
        let to_stem = &self.to_name[..self.to_name.len() - to_extension];
        let same_extension = self.from_name[self.from_name.len() - from_extension..]
            .eq_ignore_ascii_case(&self.to_name[self.to_name.len() - to_extension..]);
        let whole = 0..last_part.len();
        if last_part.eq_ignore_ascii_case(self.from_name) {
            if same_extension {
                (0..last_part.len() - from_extension, to_stem.to_string())
            } else {
                (whole, self.to_name.to_string())
            }
        } else if self.to_name[self.to_name.len() - to_extension..].eq_ignore_ascii_case(".md") {
            (whole, to_stem.to_string())
        } else {
            (whole, self.to_name.to_string())
        }
    }

    /// Whether a wiki link naming `name` in `file` reads, after the rename, as leading to `note`.
    fn leads_after(&self, name: &str, file: &Path, note: &Path) -> bool {
        wiki_can_hold(name) && self.after.wiki(name, file).as_deref() == Some(note)
    }

    /// The change that makes a wiki link lead to `note` again by its path from the folder of `file`.
    fn pinned(&self, name: &Range<usize>, file: &Path, note: &Path) -> Option<Replacement> {
        let path = wiki_path(note, file.parent()?)?;
        self.leads_after(&path, file, note).then_some(Replacement {
            range: name.clone(),
            text: path,
        })
    }

    fn wiki_change(&self, text: &str, target: &str, inner: &Range<usize>, file: &Path) -> Option<Replacement> {
        let was = self.before.wiki(target, file)?;
        let (name, last) = name_in(&text[inner.clone()])?;
        let name = inner.start + name.start..inner.start + name.end;
        let last = inner.start + last;
        if was != self.from {
            // Another note of the new name stood further away until now, and the renamed one must not take its links.
            let keeps = self.after.wiki(target, file).as_deref() == Some(&*was);
            return if keeps { None } else { self.pinned(&name, file, &was) };
        }
        let (part, written) = self.wiki_name(&text[last..name.end]);
        let new_name = format!(
            "{}{written}{}",
            &text[name.start..last + part.start],
            &text[last + part.end..name.end]
        );
        if self.leads_after(&new_name, file, self.to) {
            let range = last + part.start..last + part.end;
            return (text[range.clone()] != written).then_some(Replacement { range, text: written });
        }
        self.pinned(&name, file, self.to)
    }

    fn address_change(&self, text: &str, url: &str, written: &Written, file: &Path) -> Option<Replacement> {
        if self.before.address(url, file)? != self.from {
            return None;
        }
        // comrak takes the spaces off what stands in angle brackets.
        let spaced = &text[written.range.clone()];
        let raw = spaced.trim();
        if cleaned(raw) != url {
            return None;
        }
        let name = file_name_in(raw)?;
        let new_name = in_address(self.to_name, &raw[name.clone()], written.angle);
        if raw[name.clone()] == new_name {
            return None;
        }
        let new_raw = format!("{}{new_name}{}", &raw[..name.start], &raw[name.end..]);
        let start = written.range.start + (spaced.len() - spaced.trim_start().len());
        (self.after.address(&cleaned(&new_raw), file)?.as_path() == self.to).then(|| Replacement {
            range: start + name.start..start + name.end,
            text: new_name,
        })
    }

    fn changes_in(&self, text: &str, file: &Path) -> Vec<Replacement> {
        let doc = parse(text);
        let mut changes: Vec<Replacement> = links_of(&doc)
            .iter()
            .filter_map(|link| match &link.says {
                LinkTarget::Wiki { target, inner } => self.wiki_change(text, target, inner, file),
                LinkTarget::Address {
                    url,
                    written: Some(written),
                } => self.address_change(text, url, written, file),
                LinkTarget::Address { written: None, .. } => None,
            })
            .collect();
        // A picture's address stands behind the links in its alternative text.
        changes.sort_by_key(|change| change.range.start);
        changes
    }
}

/// The changes a rename of the note `from` to `to` (same folder) makes in the other notes of `tree`, and in `from`
/// itself for links to itself. `read` gives a note's current text, so that an open document's unsaved text counts, and
/// is asked once per note. A rename to another folder, to a name that is no text or to the same name changes nothing,
/// and a stop gives no changes at all: a part of them would break the links of the rest. A wiki link that cannot hold
/// the new name stays, as does one that keeps leading to another note. A link to another note of the new name, which
/// the renamed note would now take over, gets the path of its note.
pub fn renames(
    tree: &Tree,
    from: &Path,
    to: &Path,
    stop: &AtomicBool,
    read: &mut dyn FnMut(&Path) -> Option<String>,
) -> Vec<Rewrite> {
    let names = from
        .file_name()
        .and_then(|name| name.to_str())
        .zip(to.file_name().and_then(|name| name.to_str()));
    let Some((from_name, to_name)) = names.filter(|_| from != to && from.parent() == to.parent()) else {
        return Vec::new();
    };
    let later = tree.renamed(from, to);
    let plan = Plan {
        before: Resolver::new(tree),
        after: Resolver::new(&later),
        from: tidied(from),
        to,
        from_name,
        to_name,
    };
    let mut rewrites = Vec::new();
    for note in tree.notes() {
        if stop.load(Ordering::Relaxed) {
            return Vec::new();
        }
        let Some(text) = read(&note.path) else {
            continue;
        };
        let replacements = plan.changes_in(&text, &note.path);
        if !replacements.is_empty() {
            rewrites.push(Rewrite {
                path: note.path.clone(),
                replacements,
            });
        }
    }
    rewrites
}

#[cfg(test)]
mod tests;
