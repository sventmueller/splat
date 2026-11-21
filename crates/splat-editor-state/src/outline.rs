//! The document's headings as the tree the outline in the sidebar shows.

use std::ops::Range;

use crate::markdown::doc::{Block, BlockKind, Doc, Splice, plain_text, walk_blocks};
use crate::markdown::text::{Byte, shifted, signed};

/// One heading of the tree.
#[derive(Debug, PartialEq)]
pub struct Node {
    /// The text of the heading.
    pub title: String,
    /// Where the heading starts in the source.
    pub start: Byte,
    /// The headings nested under this one, as indexes of the tree.
    pub children: Vec<usize>,
}

/// A heading as the outline lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// 1 to 6, as `#` to `######` give it.
    pub level: u8,
    /// Its text as a screen reader reads it.
    pub title: String,
    /// Where it stands in the source.
    pub range: Range<Byte>,
    /// The top-level block it stands in, by its index and where that starts.
    block: (usize, Byte),
}

/// The headings of one tree of the document, in the order of the text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Headings {
    /// The version of the tree they were read from, and how many top-level blocks it has.
    tree: Option<(u64, usize)>,
    list: Vec<Heading>,
}

/// What became of the headings since those they follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Followed {
    /// They are those of the same tree.
    Same,
    /// Levels and titles stayed; where they stand may have moved.
    Moved,
    /// Headings may have come, gone or changed.
    Changed,
}

impl Headings {
    /// The headings of `doc`, read from all of it.
    pub fn of(doc: &Doc) -> Headings {
        Headings {
            tree: Some((doc.version(), doc.blocks().len())),
            list: read(doc.blocks(), 0),
        }
    }

    /// The headings, in the order of the text.
    pub fn list(&self) -> &[Heading] {
        &self.list
    }

    /// The headings of `doc`, taken over from these where `splice`, which made `doc` from the tree of the given version,
    /// left the blocks alone; what became of them.
    pub fn follow(self, doc: &Doc, splice: Option<(u64, &Splice)>) -> (Headings, Followed) {
        if self.tree.is_some_and(|(version, _)| version == doc.version()) {
            return (self, Followed::Same);
        }
        let spliced = match (self.tree, splice) {
            (Some((version, blocks)), Some((from, splice))) if version == from => self.spliced(blocks, doc, splice),
            _ => None,
        };
        match spliced {
            Some((list, followed)) => {
                let tree = Some((doc.version(), doc.blocks().len()));
                (Headings { tree, list }, followed)
            }
            None => (Headings::of(doc), Followed::Changed),
        }
    }

    /// The list after `splice` replaced blocks of the tree with `blocks` top-level blocks these were read from; `None`
    /// where it does not fit them.
    fn spliced(self, blocks: usize, doc: &Doc, splice: &Splice) -> Option<(Vec<Heading>, Followed)> {
        let top = doc.blocks();
        let fits = splice.old.start <= splice.old.end
            && splice.old.end <= blocks
            && splice.new.start == splice.old.start
            && splice.new.end <= top.len()
            && blocks - splice.old.len() + splice.new.len() == top.len();
        if !fits {
            return None;
        }
        let mut list = self.list;
        let head = list.partition_point(|h| h.block.0 < splice.old.start);
        let tail = list.partition_point(|h| h.block.0 < splice.old.end);
        let fresh = read(&top[splice.new.clone()], splice.new.start);
        let titled = |h: &Heading| (h.level, h.title.clone());
        let same = list[head..tail].iter().map(titled).eq(fresh.iter().map(titled));
        let mut behind = list.split_off(tail);
        for heading in &mut behind {
            let index = heading.block.0 - splice.old.end + splice.new.end;
            let start = top.get(index)?.range.start;
            let shift = signed(start.get()) - signed(heading.block.1.get());
            if shift != 0 && shift != splice.delta {
                return None;
            }
            heading.block = (index, start);
            heading.range = shifted(heading.range.start, shift)..shifted(heading.range.end, shift);
        }
        list.truncate(head);
        list.extend(fresh);
        list.extend(behind);
        Some((list, if same { Followed::Moved } else { Followed::Changed }))
    }
}

/// The headings in `blocks`, the top-level blocks of a tree from index `first` on.
fn read(blocks: &[Block], first: usize) -> Vec<Heading> {
    let mut list = Vec::new();
    for (i, top) in blocks.iter().enumerate() {
        walk_blocks(std::slice::from_ref(top), &mut |b| {
            if let BlockKind::Heading { level, .. } = &b.kind {
                list.push(Heading {
                    level: *level,
                    title: plain_text(b.inlines()),
                    range: b.range.clone(),
                    block: (first + i, top.range.start),
                });
            }
        });
    }
    list
}

/// Headings in document order, nested under the closest heading of a higher level above them. Returns the nodes and the
/// indexes of the top-level ones.
pub fn tree(headings: &[Heading]) -> (Vec<Node>, Vec<usize>) {
    let mut nodes: Vec<Node> = Vec::with_capacity(headings.len());
    let mut roots = Vec::new();
    let mut open: Vec<(u8, usize)> = Vec::new();
    for heading in headings {
        let level = heading.level;
        while open.last().is_some_and(|&(l, _)| l >= level) {
            open.pop();
        }
        let i = nodes.len();
        nodes.push(Node {
            title: heading.title.clone(),
            start: heading.range.start,
            children: Vec::new(),
        });
        match open.last() {
            Some(&(_, parent)) => nodes[parent].children.push(i),
            None => roots.push(i),
        }
        open.push((level, i));
    }
    (nodes, roots)
}

#[cfg(test)]
mod tests {
    use splat_markdown::commands::heading_anchors;
    use splat_markdown::incremental::reparse;
    use splat_markdown::reader::parse;
    use splat_testkit::{Rng, rounds};

    use super::*;
    use crate::markdown::text::bytes;

    #[test]
    fn headings_nest_under_the_closest_higher_level() {
        let h = |level, title: &str, start| Heading {
            level,
            title: title.to_string(),
            range: bytes(start..start + 1),
            block: (0, Byte::new(0)),
        };
        let (nodes, roots) = tree(&[
            h(2, "a", 0),
            h(3, "a1", 5),
            h(4, "a1x", 9),
            h(3, "a2", 12),
            h(1, "b", 20),
            h(3, "b1", 30),
        ]);
        assert_eq!(roots, [0, 4]);
        assert_eq!(nodes[0].children, [1, 3]);
        assert_eq!(nodes[1].children, [2]);
        assert_eq!(nodes[4].children, [5]);
        assert_eq!(nodes[3].start, Byte::new(12));
    }

    const PIECES: [&str; 16] = [
        "Wort",
        " ",
        "ä",
        "😀",
        "*",
        "\n",
        "\n\n",
        "# ",
        "## ",
        "> # ",
        "- ## ",
        "Titel\n===",
        "```\n",
        "[[Notiz]]",
        " {#id}",
        "### Kopf *schräg*\n\n",
    ];

    fn document(rng: &mut Rng) -> String {
        let paragraph = |rng: &mut Rng| -> String { (0..1 + rng.below(5)).map(|_| *rng.pick(&PIECES)).collect() };
        (0..2 + rng.below(8))
            .map(|_| paragraph(rng))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// What the outline listed before it followed the tree: what the anchors of the headings say.
    fn as_anchors_say(doc: &Doc) -> Vec<(u8, String, Range<Byte>)> {
        heading_anchors(doc)
            .into_iter()
            .map(|h| (h.level, h.title, h.range))
            .collect()
    }

    fn listed(headings: &Headings) -> Vec<(u8, String, Range<Byte>)> {
        headings
            .list()
            .iter()
            .map(|h| (h.level, h.title.clone(), h.range.clone()))
            .collect()
    }

    /// Headings that follow a partial parse are those read from the whole new tree, and say rightly whether only their
    /// places changed.
    #[test]
    fn headings_that_follow_an_edit_are_those_of_the_whole_tree() {
        let mut rng = Rng::new(0x5eed_0e7a_0001);
        let (mut moved, mut changed) = (0, 0);
        for _ in 0..rounds(1500) {
            let old = document(&mut rng);
            let mut doc = parse(&old);
            let known = Headings::of(&doc);
            assert_eq!(listed(&known), as_anchors_say(&doc), "{old:?}");
            let before = doc.version();
            let range = if rng.below(2) == 0 {
                let at = rng.boundary(&old);
                at..at
            } else {
                rng.range(&old)
            };
            let mut new = old.clone();
            new.replace_range(range.start.get()..range.end.get(), rng.pick(&PIECES));
            let Some(splice) = reparse(&mut doc, &new) else {
                continue;
            };
            let (followed, how) = known.clone().follow(&doc, Some((before, &splice)));
            let what = format!("{old:?} -> {new:?}, {splice:?}");
            let whole = Headings::of(&doc);
            assert_eq!(followed, whole, "{what}");
            let titled = |h: &Headings| h.list().iter().map(|h| (h.level, h.title.clone())).collect::<Vec<_>>();
            assert_eq!(how == Followed::Moved, titled(&known) == titled(&whole), "{what}");
            moved += usize::from(how == Followed::Moved);
            changed += usize::from(how == Followed::Changed);
        }
        assert!(
            moved > rounds(1500) / 10 && changed > rounds(1500) / 50,
            "{moved} moved, {changed} changed"
        );
    }

    #[test]
    fn headings_of_another_tree_are_read_anew() {
        let old = "# A\n\nEins\n\nZwei\n\n## B\n\nDrei\n\nVier\n\n## C\n\nFünf\n";
        let mut doc = parse(old);
        let known = Headings::of(&doc);
        assert_eq!(known.clone().follow(&doc, None), (known.clone(), Followed::Same));
        let before = doc.version();
        let new = old.replace("Zwei", "Zweite");
        let splice = reparse(&mut doc, &new).expect("a part");
        let (followed, how) = known.clone().follow(&doc, Some((before, &splice)));
        assert_eq!(
            (followed.list()[2].range.start, how),
            (Byte::new(new.find("## C").unwrap()), Followed::Moved)
        );
        let (read, how) = known.clone().follow(&doc, Some((before + 1000, &splice)));
        assert_eq!(
            (read, how),
            (Headings::of(&doc), Followed::Changed),
            "the splice of another tree"
        );
        let (read, how) = Headings::default().follow(&doc, Some((before, &splice)));
        assert_eq!((read, how), (Headings::of(&doc), Followed::Changed), "nothing known");
        let wrong = Splice {
            new: splice.new.start..splice.new.end + 9,
            ..splice.clone()
        };
        let (read, how) = known.follow(&doc, Some((before, &wrong)));
        assert_eq!(
            (read, how),
            (Headings::of(&doc), Followed::Changed),
            "a splice that does not fit"
        );
    }
}
