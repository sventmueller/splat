//! `word/numbering.xml` as what kind of list a paragraph belongs to.

use std::collections::HashMap;

use roxmltree::{Document, Node};

use super::package::{child_w, is_w, w_attr, w_val};
use crate::rich::ListKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Bullet,
    Number,
    /// A list level without a marker, which reads as no list.
    Nothing,
}

#[derive(Clone, Copy, Debug)]
struct Level {
    format: Format,
    start: u64,
}

impl Level {
    fn read(level: Node) -> Level {
        let format = match level.descendants().find(|node| is_w(*node, "numFmt")).and_then(w_val) {
            Some("bullet") => Format::Bullet,
            Some("none") => Format::Nothing,
            _ => Format::Number,
        };
        Level {
            format,
            start: start_of(level).unwrap_or(1),
        }
    }
}

fn start_of(level: Node) -> Option<u64> {
    w_val(child_w(level, "start")?)?.parse().ok()
}

fn level_number(level: Node) -> Option<usize> {
    w_attr(level, "ilvl")?.parse().ok()
}

/// A `w:num`: an abstract definition with the start numbers that it changes.
#[derive(Debug)]
struct Num {
    definition: String,
    starts: HashMap<usize, u64>,
}

/// The numbering definitions of a document.
#[derive(Debug, Default)]
pub(super) struct Numbering {
    definitions: HashMap<String, HashMap<usize, Level>>,
    nums: HashMap<u64, Num>,
}

impl Numbering {
    /// The definitions in a `word/numbering.xml`.
    pub(super) fn read(document: &Document) -> Numbering {
        let mut numbering = Numbering::default();
        for node in document.root_element().children() {
            if is_w(node, "abstractNum") {
                if let Some(id) = w_attr(node, "abstractNumId") {
                    let levels = node
                        .children()
                        .filter(|child| is_w(*child, "lvl"))
                        .filter_map(|level| Some((level_number(level)?, Level::read(level))))
                        .collect();
                    numbering.definitions.insert(id.to_string(), levels);
                }
            } else if is_w(node, "num") {
                numbering.read_num(node);
            }
        }
        numbering
    }

    fn read_num(&mut self, num: Node) -> Option<()> {
        let id = w_attr(num, "numId")?.parse().ok()?;
        let definition = w_val(child_w(num, "abstractNumId")?)?.to_string();
        let starts = num
            .children()
            .filter(|child| is_w(*child, "lvlOverride"))
            .filter_map(|over| {
                let start = w_val(child_w(over, "startOverride")?)?.parse().ok()?;
                Some((level_number(over)?, start))
            })
            .collect();
        self.nums.insert(id, Num { definition, starts });
        Some(())
    }

    /// The kind of list `id` makes at `level`, with the number it starts with; `None` if it has no marker there. A list
    /// the file does not define is still a list.
    pub(super) fn kind(&self, id: u64, level: usize) -> Option<ListKind> {
        let Some(num) = self.nums.get(&id) else {
            return Some(ListKind::Bullet);
        };
        let defined = self
            .definitions
            .get(&num.definition)
            .and_then(|levels| levels.get(&level));
        let Some(defined) = defined else {
            return Some(ListKind::Bullet);
        };
        match defined.format {
            Format::Bullet => Some(ListKind::Bullet),
            Format::Nothing => None,
            Format::Number => Some(ListKind::Numbered {
                start: num.starts.get(&level).copied().unwrap_or(defined.start),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::package::W;
    use super::*;

    fn numbering(body: &str) -> Numbering {
        let xml = format!("<w:numbering xmlns:w=\"{W}\">{body}</w:numbering>");
        Numbering::read(&Document::parse(&xml).unwrap())
    }

    #[test]
    fn a_level_tells_bullet_or_number_and_its_start() {
        let all = numbering(
            "<w:abstractNum w:abstractNumId=\"0\">\
               <w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/></w:lvl>\
               <w:lvl w:ilvl=\"1\"><w:start w:val=\"4\"/><w:numFmt w:val=\"decimal\"/></w:lvl>\
               <w:lvl w:ilvl=\"2\"><w:numFmt w:val=\"none\"/></w:lvl>\
               <w:lvl w:ilvl=\"3\"><w:start w:val=\"x\"/></w:lvl>\
               <w:lvl><w:numFmt w:val=\"none\"/></w:lvl>\
             </w:abstractNum>\
             <w:abstractNum/>\
             <w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>\
             <w:num w:numId=\"2\"><w:abstractNumId w:val=\"0\"/><w:lvlOverride w:ilvl=\"1\"><w:startOverride w:val=\"9\"/></w:lvlOverride><w:lvlOverride w:ilvl=\"2\"/></w:num>\
             <w:num w:numId=\"3\"><w:abstractNumId w:val=\"unknown\"/></w:num>\
             <w:num><w:abstractNumId w:val=\"0\"/></w:num>\
             <w:num w:numId=\"5\"/>",
        );
        assert_eq!(all.kind(1, 0), Some(ListKind::Bullet));
        assert_eq!(all.kind(1, 1), Some(ListKind::Numbered { start: 4 }));
        assert_eq!(all.kind(2, 1), Some(ListKind::Numbered { start: 9 }));
        assert_eq!(all.kind(1, 2), None);
        assert_eq!(all.kind(1, 3), Some(ListKind::Numbered { start: 1 }));
        assert_eq!(all.kind(1, 4), Some(ListKind::Bullet), "a level it does not define");
        assert_eq!(all.kind(3, 0), Some(ListKind::Bullet), "a definition that is not there");
        assert_eq!(all.kind(5, 0), Some(ListKind::Bullet), "a list without a definition");
        assert_eq!(all.kind(99, 0), Some(ListKind::Bullet));
    }

    #[test]
    fn a_format_behind_alternate_content_counts() {
        let all = numbering(
            "<w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"0\"><x xmlns:mc=\"urn:mc\"><w:numFmt w:val=\"bullet\"/></x></w:lvl></w:abstractNum>\
             <w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>",
        );
        assert_eq!(all.kind(1, 0), Some(ListKind::Bullet));
    }
}
