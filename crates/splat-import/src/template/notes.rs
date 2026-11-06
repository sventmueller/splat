//! Footnotes and endnotes of the generated file in the template: both work alike, under other names.

use std::collections::HashMap;

use roxmltree::{Document, Node};

use super::serialize::{WORD, is_word};

/// What differs between footnotes and endnotes.
#[derive(Clone, Copy)]
pub(super) struct NoteKind {
    /// The last word of the type of the relationship to the part.
    pub(super) suffix: &'static str,
    /// The default name of the part without its folder and extension.
    pub(super) stem: &'static str,
    pub(super) note: &'static str,
    pub(super) reference: &'static str,
}

pub(super) const KINDS: [NoteKind; 2] = [
    NoteKind {
        suffix: "footnotes",
        stem: "footnotes",
        note: "footnote",
        reference: "footnoteReference",
    },
    NoteKind {
        suffix: "endnotes",
        stem: "endnotes",
        note: "endnote",
        reference: "endnoteReference",
    },
];

/// Fresh ids for what comes from a file's notes, and the notes to be added to the part in the template if there is one.
pub(super) struct NotePlan<'a, 'i> {
    pub(super) map: HashMap<String, String>,
    pub(super) added: Vec<Node<'a, 'i>>,
}

/// Separators and what continues them belong to a file's look, not to its text.
fn is_separator(note: &Node) -> bool {
    note.attribute((WORD, "type")).is_some_and(|kind| kind != "normal")
}

fn notes_of<'a, 'i>(doc: &'a Document<'i>, kind: NoteKind) -> impl Iterator<Item = Node<'a, 'i>> {
    doc.root_element()
        .children()
        .filter(move |node| is_word(node, kind.note))
}

/// Ids stay the same in a part that comes along as a whole; behind the template's notes, they go on from the highest.
pub(super) fn plan<'a, 'i>(
    generated: &'a Document<'i>,
    template: Option<&Document>,
    kind: NoteKind,
) -> NotePlan<'a, 'i> {
    let mut plan = NotePlan {
        map: HashMap::new(),
        added: Vec::new(),
    };
    let Some(template) = template else {
        for note in notes_of(generated, kind) {
            if let Some(id) = note.attribute((WORD, "id")) {
                plan.map.insert(id.to_string(), id.to_string());
            }
        }
        return plan;
    };
    let highest = notes_of(template, kind)
        .filter_map(|note| note.attribute((WORD, "id"))?.parse::<i32>().ok())
        .max()
        .unwrap_or(0);
    let mut next = i64::from(highest.max(0)) + 1;
    for note in notes_of(generated, kind).filter(|note| !is_separator(note)) {
        if let Some(id) = note.attribute((WORD, "id")) {
            plan.map.insert(id.to_string(), next.to_string());
            next += 1;
        }
        plan.added.push(note);
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"";

    #[test]
    fn notes_go_on_above_the_template_and_separators_stay_behind() {
        let generated = format!(
            "<w:footnotes {NS}><w:footnote w:type=\"separator\" w:id=\"-1\"/><w:footnote w:id=\"1\"/>\
             <w:footnote w:type=\"normal\" w:id=\"2\"/><w:footnote/><w:endnote w:id=\"9\"/></w:footnotes>"
        );
        let generated = Document::parse(&generated).unwrap();
        let template = format!(
            "<w:footnotes {NS}><w:footnote w:id=\"-1\"/><w:footnote w:id=\"7\"/><w:footnote w:id=\"x\"/></w:footnotes>"
        );
        let template = Document::parse(&template).unwrap();
        let merged = plan(&generated, Some(&template), KINDS[0]);
        assert_eq!(merged.map["1"], "8");
        assert_eq!(merged.map["2"], "9");
        assert_eq!(merged.map.len(), 2);
        assert_eq!(merged.added.len(), 3);

        let taken_along = plan(&generated, None, KINDS[0]);
        assert_eq!(taken_along.map["-1"], "-1");
        assert_eq!(taken_along.map["2"], "2");
        assert!(taken_along.added.is_empty());

        let empty_template = format!("<w:footnotes {NS}/>");
        let empty_template = Document::parse(&empty_template).unwrap();
        assert_eq!(plan(&generated, Some(&empty_template), KINDS[0]).map["1"], "1");
    }
}
