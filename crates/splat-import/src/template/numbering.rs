//! The generated file's lists behind those of the template, with numbers of their own.

use std::collections::HashMap;

use roxmltree::{Document, Node};

use super::serialize::{Edits, Names, Rewrite, WORD, is_word, write_tree};

/// The new numbers of what is added to the template's numbering.
pub(super) struct NumberPlan<'a, 'i> {
    pub(super) abstract_map: HashMap<String, String>,
    pub(super) number_map: HashMap<String, String>,
    abstracts: Vec<Node<'a, 'i>>,
    numbers: Vec<Node<'a, 'i>>,
}

/// The highest number in the attribute `key` of the children `name` of `root`; one that is no number counts as none.
fn highest(root: &Node, name: &str, key: &str) -> u64 {
    root.children()
        .filter(|node| is_word(node, name))
        .filter_map(|node| node.attribute((WORD, key))?.parse::<u32>().ok())
        .map(u64::from)
        .max()
        .unwrap_or(0)
}

/// Numbers above the template's for everything of `name` in `root`.
fn renumbered<'a, 'i>(
    root: &Node<'a, 'i>,
    name: &str,
    key: &str,
    above: u64,
) -> (HashMap<String, String>, Vec<Node<'a, 'i>>) {
    let mut map = HashMap::new();
    let mut nodes = Vec::new();
    let mut next = above + 1;
    for node in root.children().filter(|node| is_word(node, name)) {
        if let Some(id) = node.attribute((WORD, key)) {
            map.insert(id.to_string(), next.to_string());
            next += 1;
        }
        nodes.push(node);
    }
    (map, nodes)
}

pub(super) fn plan<'a, 'i>(generated: &'a Document<'i>, template: &Document) -> NumberPlan<'a, 'i> {
    let (from, into) = (generated.root_element(), template.root_element());
    let (abstract_map, abstracts) = renumbered(
        &from,
        "abstractNum",
        "abstractNumId",
        highest(&into, "abstractNum", "abstractNumId"),
    );
    let (number_map, numbers) = renumbered(&from, "num", "numId", highest(&into, "num", "numId"));
    NumberPlan {
        abstract_map,
        number_map,
        abstracts,
        numbers,
    }
}

/// The template's numbering with the plan's definitions where the schema wants them: pictures, abstract definitions,
/// numbers and what ends the part, in that order.
pub(super) fn merged(text: &str, root: Node, plan: &NumberPlan, rewrite: &Rewrite) -> String {
    let mut names = Names::of(root);
    let mut abstracts = String::new();
    for node in &plan.abstracts {
        write_tree(*node, &mut names, rewrite, &mut abstracts);
    }
    let mut numbers = String::new();
    for node in &plan.numbers {
        write_tree(*node, &mut names, rewrite, &mut numbers);
    }
    let last_of = |name: &str| root.children().rev().find(|node| is_word(node, name));
    let first_number = root.children().find(|node| is_word(node, "num"));
    let abstracts_at = last_of("abstractNum")
        .map(|node| node.range().end)
        .or(first_number.map(|node| node.range().start))
        .or(last_of("numPicBullet").map(|node| node.range().end))
        .or(root.first_child().map(|node| node.range().start));
    let mut edits = Edits::new(text);
    edits.insert(edits.behind_name(root), names.declarations());
    match abstracts_at {
        Some(at) => {
            edits.insert(at, abstracts);
            edits.insert(last_of("num").map_or(at, |node| node.range().end), numbers);
        }
        None => edits.append_children(root, abstracts + &numbers),
    }
    edits.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"";

    fn merge(template: &str) -> String {
        let generated = format!(
            "<w:numbering {NS}><w:abstractNum w:abstractNumId=\"0\"/><w:abstractNum/>\
             <w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num><w:x/></w:numbering>"
        );
        let generated = Document::parse(&generated).unwrap();
        let template = template.replace("NS", NS);
        let doc = Document::parse(&template).unwrap();
        let plan = plan(&generated, &doc);
        let rewrite = Rewrite {
            abstract_numbers: plan.abstract_map.clone(),
            numbers: plan.number_map.clone(),
            ..Rewrite::default()
        };
        merged(&template, doc.root_element(), &plan, &rewrite).replace(NS, "NS")
    }

    #[test]
    fn additions_stand_where_the_schema_wants_them() {
        assert_eq!(
            merge(
                "<w:numbering NS><w:numPicBullet/><w:abstractNum w:abstractNumId=\"4\"/><w:abstractNum w:abstractNumId=\"x\"/>\
                 <w:num w:numId=\"7\"/><w:numIdMacAtCleanup/></w:numbering>"
            ),
            "<w:numbering NS><w:numPicBullet/><w:abstractNum w:abstractNumId=\"4\"/><w:abstractNum w:abstractNumId=\"x\"/>\
             <w:abstractNum w:abstractNumId=\"5\"></w:abstractNum><w:abstractNum></w:abstractNum>\
             <w:num w:numId=\"7\"/><w:num w:numId=\"8\"><w:abstractNumId w:val=\"5\"/></w:num><w:numIdMacAtCleanup/></w:numbering>"
                .replace("></w:abstractNum>", "/>")
        );
    }

    #[test]
    fn a_template_without_definitions_or_numbers_takes_them_in_the_right_place() {
        let added = "<w:abstractNum w:abstractNumId=\"1\"/><w:abstractNum/><w:num w:numId=\"1\"><w:abstractNumId w:val=\"1\"/></w:num>";
        assert_eq!(
            merge("<w:numbering NS><w:num w:numId=\"3\"/></w:numbering>"),
            "<w:numbering NS><w:abstractNum w:abstractNumId=\"1\"/><w:abstractNum/><w:num w:numId=\"3\"/>\
             <w:num w:numId=\"4\"><w:abstractNumId w:val=\"1\"/></w:num></w:numbering>"
        );
        assert_eq!(
            merge("<w:numbering NS><w:numPicBullet/><w:numIdMacAtCleanup/></w:numbering>"),
            format!("<w:numbering NS><w:numPicBullet/>{added}<w:numIdMacAtCleanup/></w:numbering>")
        );
        assert_eq!(
            merge("<w:numbering NS><w:numIdMacAtCleanup/></w:numbering>"),
            format!("<w:numbering NS>{added}<w:numIdMacAtCleanup/></w:numbering>")
        );
        assert_eq!(
            merge("<w:numbering NS/>"),
            format!("<w:numbering NS>{added}</w:numbering>")
        );
        assert_eq!(
            merge("<w:numbering NS><w:abstractNum w:abstractNumId=\"9\"/></w:numbering>"),
            "<w:numbering NS><w:abstractNum w:abstractNumId=\"9\"/><w:abstractNum w:abstractNumId=\"10\"/><w:abstractNum/>\
             <w:num w:numId=\"1\"><w:abstractNumId w:val=\"10\"/></w:num></w:numbering>"
        );
    }
}
