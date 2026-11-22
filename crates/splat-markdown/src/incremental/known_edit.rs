//! The partial parse for an editor that knows where its edit lies, which spares comparing all of the old and the new
//! text to find it.

use std::ops::Range;

use super::{Parsed, colored, reparse_changed};
use crate::doc::Doc;
use crate::highlight::Style;
use crate::text::{Byte, TextAt, common_prefix, common_suffix, offsets};

/// [`super::reparse_part_and_color`] for the text that replacing `replaced` of the text of `doc` with `with` gives, with
/// the same result; `doc` as it was as well when `replaced` does not lie on character boundaries within its text.
pub fn reparse_edit_and_color(
    mut doc: Doc,
    spans: &[(Range<Byte>, Style)],
    replaced: Range<Byte>,
    with: &str,
) -> Result<Parsed, Box<Doc>> {
    let old = doc.shared_text();
    let replaced = offsets(&replaced);
    if replaced.start > replaced.end
        || replaced.end > old.len()
        || !old.is_char_boundary(replaced.start)
        || !old.is_char_boundary(replaced.end)
    {
        return Err(Box::new(doc));
    }
    let mut new_text = String::with_capacity(old.len() - replaced.len() + with.len());
    new_text.push_str(&old[..replaced.start]);
    new_text.push_str(with);
    new_text.push_str(&old[replaced.end..]);
    let (old_change, new_change) = changed_around(&old, &replaced, &new_text);
    drop(old);
    match reparse_changed(&mut doc, &new_text, old_change, new_change) {
        Ok(splice) => Ok(colored(doc, spans, splice)),
        Err(_) => Err(Box::new(doc)),
    }
}

/// What [`crate::text::changed`] gives for `old` and `new`, which differ only in `replaced` of `old`, read from there.
fn changed_around(old: &str, replaced: &Range<usize>, new: &str) -> (Range<Byte>, Range<Byte>) {
    let (a, b) = (old.as_bytes(), new.as_bytes());
    let mut prefix = replaced.start + common_prefix(&a[replaced.start..], &b[replaced.start..]);
    while !(old.is_char_boundary(prefix) && new.is_char_boundary(prefix)) {
        prefix -= 1;
    }
    let max_suffix = old.len().min(new.len()) - prefix;
    let kept = old.len() - replaced.end;
    let mut suffix = if kept >= max_suffix {
        max_suffix
    } else {
        kept + common_suffix(&a[..replaced.end], &b[..b.len() - kept], max_suffix - kept)
    };
    while !(old.is_char_boundary(old.len() - suffix) && new.is_char_boundary(new.len() - suffix)) {
        suffix -= 1;
    }
    (
        Byte::new(prefix)..old.end() - suffix,
        Byte::new(prefix)..new.end() - suffix,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::highlight::highlight;
    use crate::incremental::reparse_part_and_color;
    use crate::reader::parse;
    use crate::text::{bytes, changed};

    #[test]
    fn the_change_read_from_the_edit_is_the_one_comparing_all_of_both_texts_finds() {
        let texts = ["", "a", "aa", "aaaa", "abab", "aéa", "éé", "x è y", "a\r\nb", "ab😀ba"];
        let pieces = ["", "a", "b", "aa", "é", "è", "ab", "😀", "\n"];
        for old in texts {
            let ends: Vec<usize> = (0..=old.len()).filter(|&at| old.is_char_boundary(at)).collect();
            for (i, &start) in ends.iter().enumerate() {
                for &end in &ends[i..] {
                    for with in pieces {
                        let new = format!("{}{with}{}", &old[..start], &old[end..]);
                        assert_eq!(
                            changed_around(old, &(start..end), &new),
                            changed(old, &new),
                            "{old:?}, {start}..{end} -> {with:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn an_edit_off_the_characters_leaves_the_document_as_it_was() {
        let old = "Grüße\n\nnoch\n";
        let doc = parse(old);
        let version = doc.version();
        let spans = highlight(&doc);
        let doc = reparse_edit_and_color(doc, &spans, bytes(3..4), "x")
            .err()
            .expect("inside the ü");
        assert_eq!((doc.version(), doc.text()), (version, old));
        let doc = reparse_edit_and_color(*doc, &spans, bytes(5..40), "x")
            .err()
            .expect("beyond the text");
        assert_eq!(doc.version(), version);
    }

    #[test]
    fn an_edit_parses_as_its_text_does() {
        let old = "# Titel\n\nEins\n\nZwei *schief*\n\nDrei\n";
        let new = old.replace("Zwei", "Zweieinhalb");
        let at = old.find("Zwei").expect("in the text");
        let doc = parse(old);
        let spans = highlight(&doc);
        let by_edit = reparse_edit_and_color(doc, &spans, bytes(at..at + 4), "Zweieinhalb").expect("in part");
        let by_text = reparse_part_and_color(parse(old), &spans, &new).expect("in part");
        assert_eq!(by_edit.doc.text(), new);
        assert_eq!(by_edit.doc.blocks(), by_text.doc.blocks());
        assert_eq!(by_edit.spans, by_text.spans);
        assert_eq!(by_edit.splice, by_text.splice);
    }
}
