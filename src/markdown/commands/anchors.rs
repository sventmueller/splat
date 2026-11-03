use std::ops::Range;

use crate::markdown::doc::{BlockKind, Doc, plain_text, walk_blocks};
use crate::markdown::text::Byte;

/// A heading, what it says and what a link to it is called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadingAnchor {
    pub level: u8,
    pub title: String,
    pub anchor: String,
    pub range: Range<Byte>,
}

/// Anchor names of all headings as GitHub generates them, or as given with `{#id}`, with the source range of each
/// heading.
pub fn heading_anchors(doc: &Doc) -> Vec<HeadingAnchor> {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut taken = std::collections::HashSet::new();
    walk_blocks(&doc.blocks, &mut |b| {
        if let BlockKind::Heading { id: Some(id), .. } = &b.kind {
            taken.insert(id.to_string());
        }
    });
    let mut out = Vec::new();
    walk_blocks(&doc.blocks, &mut |b| {
        if let BlockKind::Heading { level, id, .. } = &b.kind {
            let title = plain_text(b.inlines());
            let anchor = match id {
                Some(id) => id.to_string(),
                None => {
                    let base = slug(&title);
                    let n = seen.entry(base.clone()).or_insert(0);
                    loop {
                        let anchor = if *n == 0 { base.clone() } else { format!("{base}-{n}") };
                        *n += 1;
                        if taken.insert(anchor.clone()) {
                            break anchor;
                        }
                    }
                }
            };
            out.push(HeadingAnchor {
                level: *level,
                title,
                anchor,
                range: b.range.clone(),
            });
        }
    });
    out
}

/// GitHub's heading anchors: lower case, punctuation dropped, spaces turned into hyphens.
fn slug(title: &str) -> String {
    title
        .trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::reader::parse;

    #[test]
    fn a_generated_anchor_keeps_clear_of_a_given_one_before_or_behind_it() {
        let anchors = |t: &str| -> Vec<String> { heading_anchors(&parse(t)).into_iter().map(|a| a.anchor).collect() };
        assert_eq!(anchors("# A {#b}\n\n# B\n"), ["b", "b-1"]);
        assert_eq!(anchors("# B\n\n# A {#b}\n"), ["b-1", "b"]);
    }

    #[test]
    fn heading_anchors_stay_unique() {
        let d = parse("# a\n\n# a\n\n# a-1\n");
        let anchors: Vec<String> = heading_anchors(&d).into_iter().map(|h| h.anchor).collect();
        assert_eq!(anchors, ["a", "a-1", "a-1-1"]);
    }

    #[test]
    fn a_heading_anchor_reads_every_kind_of_text_in_the_title() {
        let title = |t: &str| heading_anchors(&parse(t)).remove(0);
        assert_eq!(title("a\nb\n===\n").title, "a b");
        assert_eq!(title("# x $y$\n").title, "x y");
        assert_eq!(title("# x :smile:\n").title, "x 😄");
        assert_eq!(title("# a-b_c d!\n").anchor, "a-b_c-d");
    }
}
