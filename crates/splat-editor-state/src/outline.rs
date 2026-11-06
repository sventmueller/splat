//! The document's headings as the tree the outline in the sidebar shows.

use std::ops::Range;

use crate::markdown::text::Byte;

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

/// Headings as `(level, title, source range)` in document order, nested under the closest heading of a higher
/// level above them. Returns the nodes and the indexes of the top-level ones.
pub fn tree(headings: &[(u8, String, Range<Byte>)]) -> (Vec<Node>, Vec<usize>) {
    let mut nodes: Vec<Node> = Vec::with_capacity(headings.len());
    let mut roots = Vec::new();
    let mut open: Vec<(u8, usize)> = Vec::new();
    for (level, title, range) in headings {
        while open.last().is_some_and(|(l, _)| l >= level) {
            open.pop();
        }
        let i = nodes.len();
        nodes.push(Node {
            title: title.clone(),
            start: range.start,
            children: Vec::new(),
        });
        match open.last() {
            Some(&(_, parent)) => nodes[parent].children.push(i),
            None => roots.push(i),
        }
        open.push((*level, i));
    }
    (nodes, roots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::text::bytes;

    #[test]
    fn headings_nest_under_the_closest_higher_level() {
        let h = |level, title: &str, start| (level, title.to_string(), bytes(start..start + 1));
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
}
