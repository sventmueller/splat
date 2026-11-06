//! What the target `notes` in `fuzz/` asks of its input: the word counter, the metadata form, and the parts of
//! searching and renaming that need no disk. Input is a text and steps behind it, as in `fuzz_checks`; a step is a byte
//! for what to do, two for a place, and the text it brings along.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use splat_markdown::counts::{Counts, of_doc, of_range};
use splat_markdown::doc::{BlockKind, Doc};
use splat_markdown::front_matter::{self, FrontMatter, Value, scalar};
use splat_markdown::reader::parse;
use splat_markdown::text::{Byte, TextAt, bytes};
use splat_notes::{MOST_HITS_PER_FILE, Query, Tree, applied, hits_in_text, renames};
use unicode_normalization::UnicodeNormalization;

use crate::fuzz_checks::{edit_fits, text_and_steps};

/// What a step asks for.
struct Step {
    what: u8,
    at: usize,
    brings: String,
}

impl Step {
    fn of(raw: &[u8], text: &str) -> Option<Step> {
        let [what, high, low, brings @ ..] = raw else {
            return None;
        };
        let at = (usize::from(*high) << 8 | usize::from(*low)).min(text.len());
        Some(Step {
            what: *what,
            at: text.floor_char_boundary(at),
            brings: String::from_utf8_lossy(brings).into_owned(),
        })
    }
}

/// `of_doc` is `of_range` of everything, and ranges side by side take each character once and each word but the one
/// they cut.
fn counts_hold(text: &str, doc: &Doc, steps: &[Step]) {
    let whole = of_doc(doc);
    assert_eq!(of_range(doc, bytes(0..text.len())), whole, "{text:?}");
    assert!(
        whole.signs <= whole.words && whole.words <= whole.characters,
        "{whole:?} of {text:?}"
    );
    let _ = whole.reading_minutes();
    for step in steps {
        let (a, b) = (
            of_range(doc, bytes(0..step.at)),
            of_range(doc, bytes(step.at..text.len())),
        );
        let what = || format!("{a:?} and {b:?} of {whole:?} at {} in {text:?}", step.at);
        assert_eq!(a.characters + b.characters, whole.characters, "{}", what());
        assert_eq!(a.signs + b.signs, whole.signs, "{}", what());
        assert!(
            a.words + b.words <= whole.words + 1 && a.words + b.words >= whole.words,
            "{}",
            what()
        );
        let none = Counts::default();
        assert_eq!(of_range(doc, bytes(step.at..step.at)), none, "{text:?}");
        assert_eq!(of_range(doc, bytes(text.len() + 1..text.len() + 9)), none, "{text:?}");
    }
}

/// Whether nothing but comments and blank lines stand outside the fields, which is what the form promises its result
/// for: what is no YAML can change its meaning with the fields around it.
fn holds_only_fields(text: &str, block: &FrontMatter) -> bool {
    let is_quiet = |gap: &str| {
        gap.lines().all(|line| {
            let line = line.trim_start_matches([' ', '\t']);
            line.is_empty() || line.starts_with('#')
        }) && !gap.contains('\r')
    };
    let mut at = text[block.block.start.get()..]
        .find('\n')
        .map_or(text.len(), |n| n + 1 + block.block.start.get());
    for field in &block.fields {
        if !text.get(at..field.lines.start.get()).is_some_and(is_quiet) {
            return false;
        }
        at = field.lines.end.get();
    }
    let closing = block.block.end.get().saturating_sub(3);
    text.get(at..closing).is_some_and(is_quiet)
}

/// A value for a change of a field, as a step brings it.
fn value_of(step: &Step, salt: u8) -> Value {
    if (step.what ^ salt) & 0b100 == 0 {
        Value::Text(step.brings.clone())
    } else {
        Value::List(step.brings.split(',').map(ToString::to_string).collect())
    }
}

fn plain(value: &Value) -> Value {
    match value {
        Value::Complex(_) => Value::Complex(String::new()),
        other => other.clone(),
    }
}

/// Any change of the fields reads back as asked, leaves every other line alone and needs no second edit.
fn front_matter_holds(text: &str, doc: &Doc, steps: &[Step]) {
    let found = front_matter::read(doc);
    let block_first = matches!(
        doc.blocks().first().map(|b| &b.kind),
        Some(BlockKind::FrontMatter { .. })
    );
    assert_eq!(found.is_some(), block_first, "{text:?}");
    let same = found.as_ref().map_or_else(Vec::new, |block| {
        block.fields.iter().map(|f| (f.key.clone(), f.value.clone())).collect()
    });
    assert_eq!(
        front_matter::edit(doc, &same, &splat_markdown::text::bytes(0..0)),
        None,
        "{text:?}"
    );
    for step in steps {
        let quoted = scalar(&step.brings);
        assert!(!quoted.contains(['\n', '\r']), "{quoted:?}");
        let old = found.as_ref().map_or(&[][..], |block| &block.fields);
        let mut wanted = Vec::new();
        let mut untouched = Vec::new();
        for (at, field) in old.iter().enumerate() {
            let salt = u8::try_from(at % 251).unwrap_or(0);
            match step.what.wrapping_add(salt) % 4 {
                0 => {}
                1 => wanted.push((field.key.clone(), value_of(step, salt))),
                _ => {
                    wanted.push((field.key.clone(), field.value.clone()));
                    untouched.push(field);
                }
            }
        }
        if step.what & 0b1000 != 0 {
            wanted.push((format!("{} {}", step.brings, old.len()), value_of(step, 1)));
        }
        let selection = Byte::new(step.at)..Byte::new(step.at);
        let Some(edit) = front_matter::edit(doc, &wanted, &selection) else {
            continue;
        };
        let new = edit_fits(text, &edit, "front matter");
        let new_doc = parse(&new);
        let after = front_matter::read(&new_doc);
        let once_each = old
            .iter()
            .enumerate()
            .all(|(at, f)| old[..at].iter().all(|other| other.key != f.key));
        if once_each && found.as_ref().is_none_or(|block| holds_only_fields(text, block)) {
            for field in &untouched {
                let lines = after
                    .iter()
                    .flat_map(|block| &block.fields)
                    .find(|f| f.key == field.key)
                    .map(|f| new.at(f.lines.clone()));
                let kept = text.at(field.lines.clone());
                assert!(
                    lines.is_some_and(|l| l.starts_with(kept)),
                    "{kept:?} in {new:?}, from {text:?}"
                );
            }
            let mut shown: Vec<_> = after
                .iter()
                .flat_map(|block| &block.fields)
                .map(|f| (f.key.clone(), plain(&f.value)))
                .collect();
            let mut expected = Vec::new();
            let mut new_fields = wanted.clone();
            for field in old {
                if let Some(at) = new_fields.iter().position(|w| w.0 == field.key) {
                    let (key, value) = new_fields.remove(at);
                    let stays = matches!(field.value, Value::Complex(_));
                    expected.push((key, plain(if stays { &field.value } else { &value })));
                }
            }
            expected.extend(new_fields);
            // New fields go in front of a value that never closes, so only what is there can be held against the
            // result.
            shown.sort_by(|a, b| a.0.cmp(&b.0));
            expected.sort_by(|a, b| a.0.cmp(&b.0));
            assert_eq!(shown, expected, "{text:?} wanted {wanted:?} became {new:?}");
            assert_eq!(
                front_matter::edit(&new_doc, &wanted, &splat_markdown::text::bytes(0..0)),
                None,
                "{text:?} wanted {wanted:?} became {new:?}"
            );
        }
    }
}

/// A tree in memory: this note, the note that gets renamed, and what stands around them.
fn tree_of(names: &[&str]) -> Tree {
    let mut notes = vec!["sub/New.md", "sub/Other.md", "sub/Sub.md"];
    notes.extend_from_slice(names);
    Tree::of_notes(Path::new("/v"), &notes)
}

fn link_count(text: &str, target: &str, file: &Path, tree: &Tree) -> Option<usize> {
    let found = hits_in_text(text, &Query::LinksTo(PathBuf::from(target)), file, tree);
    (!found.more).then_some(found.hits.len())
}

/// Hits lie on letters inside the text, in order, and show themselves in their context; a text in another normal form
/// gives as many.
fn search_holds(text: &str, steps: &[Step], tree: &Tree) {
    let file = Path::new("/v/here.md");
    for step in steps {
        let query = Query::Text {
            needle: step.brings.clone(),
            case_sensitive: step.what & 1 == 0,
        };
        let found = hits_in_text(text, &query, file, tree);
        let mut at = 0;
        for hit in &found.hits {
            assert!(
                hit.range.start >= at && text.get(hit.range.clone()).is_some(),
                "{hit:?} in {text:?}"
            );
            at = hit.range.end;
            assert!(hit.context.get(hit.in_context.clone()).is_some(), "{hit:?} in {text:?}");
            assert!(hit.context.chars().count() <= 160, "{hit:?}");
            assert!(!hit.context.contains(['\n', '\r']), "{hit:?}");
        }
        assert!(found.hits.len() <= MOST_HITS_PER_FILE);
        let first = text.find(&step.brings);
        let stands_alone = first.is_some_and(|first| {
            text[first + step.brings.len()..]
                .chars()
                .next()
                .is_none_or(|next| next.is_ascii())
        });
        if step.brings.is_ascii() && !step.brings.is_empty() && stands_alone {
            assert!(!found.hits.is_empty(), "{:?} in {text:?}", step.brings);
        }
        let decomposed: String = text.nfd().collect();
        let other = hits_in_text(&decomposed, &query, file, tree);
        if !found.more && !other.more {
            assert_eq!(found.hits.len(), other.hits.len(), "{:?} in {text:?}", step.brings);
        }
        for tag in step.brings.split(' ') {
            let tags = hits_in_text(text, &Query::Tag(tag.to_string()), file, tree);
            for hit in tags.hits {
                assert!(text.get(hit.range.clone()).is_some(), "{hit:?} in {text:?}");
            }
        }
    }
}

/// What a rename changes lies on letters, in order, and every link that led to the note leads to its new name, but for
/// links in embedded HTML, which stay; no other note loses or gains a link.
fn rename_holds(text: &str) {
    let (from, to) = (Path::new("/v/Old.md"), Path::new("/v/New.md"));
    let tree = tree_of(&["Here.md", "Old.md"]);
    let later = tree.renamed(from, to);
    let files = ["/v/Here.md", "/v/Old.md", "/v/sub/Sub.md"].map(Path::new);
    let mut asked = 0;
    let rewrites = renames(&tree, from, to, &AtomicBool::new(false), &mut |path| {
        asked += 1;
        files.contains(&path).then(|| text.to_string())
    });
    assert_eq!(asked, tree.notes().count(), "{text:?}");
    let new_of = |path: &Path| {
        rewrites
            .iter()
            .find(|r| r.path == path)
            .map_or(text.to_string(), |rewrite| {
                let mut at = 0;
                for replacement in &rewrite.replacements {
                    let fits = replacement.range.start >= at && text.get(replacement.range.clone()).is_some();
                    assert!(fits, "{replacement:?} in {text:?}");
                    at = replacement.range.end;
                }
                applied(text, &rewrite.replacements)
            })
    };
    let html_links = text.to_ascii_lowercase().matches("<a").count();
    for file in files {
        let new = new_of(file);
        let later_file = if file == from { to } else { file };
        let before = |target: &str| link_count(text, target, file, &tree);
        let after = |target: &str| link_count(&new, target, later_file, &later);
        let what = || format!("{file:?}: {text:?} became {new:?}");
        if let (Some(was), Some(was_to), Some(now), Some(old)) = (
            before("/v/Old.md"),
            before("/v/New.md"),
            after("/v/New.md"),
            after("/v/Old.md"),
        ) {
            assert!(now + old >= was + was_to, "{}", what());
            assert!(old <= html_links, "{}", what());
        }
        for other in ["/v/Here.md", "/v/sub/Other.md", "/v/sub/Sub.md", "/v/sub/New.md"] {
            assert_eq!(before(other), after(other), "{other}: {}", what());
        }
    }
}

/// The promises of the notes hold for any input.
pub fn check_notes(data: &[u8]) {
    let Some((text, raw_steps)) = text_and_steps(data) else {
        return;
    };
    let steps: Vec<Step> = raw_steps.filter_map(|raw| Step::of(raw, &text)).collect();
    let doc = parse(&text);
    counts_hold(&text, &doc, &steps);
    front_matter_holds(&text, &doc, &steps);
    search_holds(&text, &steps, &tree_of(&["here.md", "Old.md"]));
    rename_holds(&text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_with_something_else_than_fields_makes_no_promise() {
        let held = |text: &str| holds_only_fields(text, &front_matter::read(&parse(text)).expect("a block"));
        assert!(held("---\na: 1\n# b\n\nc: 2\n---\n"));
        assert!(!held("---\nstray\na: 1\n---\n"));
        assert!(!held("---\na: 1\nstray\n---\n"));
    }

    #[test]
    fn plain_inputs_keep_every_promise() {
        check_notes(b"---\na: 1\n---\n\n[[Old]] [x](Old.md) #tag\xFF\x05\x00\x08b, c\xFF\x09\x00\x10Old");
        check_notes(b"");
        check_notes(b"\xFF\xFF");
    }
}
