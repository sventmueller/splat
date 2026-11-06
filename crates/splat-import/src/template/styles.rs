//! Which of the template's styles the generated text uses: Word's ids are localized, its names are not, so names decide.

use std::collections::{HashMap, HashSet};

use roxmltree::{Document, Node};

use super::serialize::{WORD, child_value, is_word};

/// How generated style ids read in the template, and the styles it has to take in.
pub(super) struct StylePlan<'a, 'i> {
    pub(super) map: HashMap<String, String>,
    pub(super) added: Vec<Node<'a, 'i>>,
}

/// A style's identity across files: its type and its name in lowercase.
fn identity(style: &Node) -> Option<(String, String)> {
    let kind = style.attribute((WORD, "type")).unwrap_or("paragraph");
    Some((kind.to_string(), child_value(style, "name")?.to_lowercase()))
}

fn styles_of<'a, 'i>(doc: &'a Document<'i>) -> impl Iterator<Item = Node<'a, 'i>> {
    doc.root_element().children().filter(|node| is_word(node, "style"))
}

/// Finds a place for every style in `used` and what it depends on.
pub(super) fn plan<'a, 'i>(used: &[String], generated: &'a Document<'i>, template: &Document) -> StylePlan<'a, 'i> {
    let mut ids = HashSet::new();
    let mut by_identity = HashMap::new();
    for style in styles_of(template) {
        let Some(id) = style.attribute((WORD, "styleId")) else {
            continue;
        };
        ids.insert(id.to_string());
        if let Some(key) = identity(&style) {
            by_identity.entry(key).or_insert_with(|| id.to_string());
        }
    }
    let mut definitions: HashMap<&str, Node<'a, 'i>> = HashMap::new();
    for style in styles_of(generated) {
        if let Some(id) = style.attribute((WORD, "styleId")) {
            definitions.entry(id).or_insert(style);
        }
    }
    let mut plan = StylePlan {
        map: HashMap::new(),
        added: Vec::new(),
    };
    let mut waiting: Vec<&str> = used.iter().rev().map(String::as_str).collect();
    let mut seen = HashSet::new();
    while let Some(id) = waiting.pop() {
        if !seen.insert(id.to_string()) {
            continue;
        }
        let Some(style) = definitions.get(id) else {
            continue;
        };
        if let Some(found) = identity(style).and_then(|key| by_identity.get(&key)) {
            plan.map.insert(id.to_string(), found.clone());
            continue;
        }
        let new_id = free_id(id, &mut ids);
        plan.map.insert(id.to_string(), new_id);
        plan.added.push(*style);
        for dependency in ["link", "next", "basedOn"] {
            if let Some(other) = child_value(style, dependency) {
                waiting.extend(definitions.get_key_value(other).map(|(key, _)| *key));
            }
        }
    }
    plan
}

/// `id`, or if the template uses it for something else the first free one with a number behind it.
fn free_id(id: &str, ids: &mut HashSet<String>) -> String {
    let free = if ids.contains(id) {
        (1..)
            .map(|n| format!("{id}{n}"))
            .find(|candidate| !ids.contains(candidate))
            .unwrap_or_default()
    } else {
        id.to_string()
    };
    ids.insert(free.clone());
    free
}

#[cfg(test)]
mod tests {
    use super::*;

    const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"";

    fn style(id: &str, name: &str, more: &str) -> String {
        format!("<w:style w:styleId=\"{id}\"><w:name w:val=\"{name}\"/>{more}</w:style>")
    }

    fn doc(styles: &[String]) -> String {
        format!("<w:styles {NS}>{}</w:styles>", styles.concat())
    }

    #[test]
    fn names_decide_in_any_case_and_only_within_a_type() {
        let template = doc(&[
            style("berschrift1", "heading 1", ""),
            "<w:style w:type=\"character\" w:styleId=\"Zeichen\"><w:name w:val=\"Code\"/></w:style>".to_string(),
            "<w:style w:styleId=\"Ohne\"/>".to_string(),
            "<w:style><w:name w:val=\"Ohne Id\"/></w:style>".to_string(),
        ]);
        let generated = doc(&[
            style("Heading1", "Heading 1", ""),
            style("Code", "Code", ""),
            "<w:style/>".to_string(),
        ]);
        let (template, generated) = (
            Document::parse(&template).unwrap(),
            Document::parse(&generated).unwrap(),
        );
        let used = ["Heading1", "Code", "Heading1", "Gone"].map(String::from);
        let plan = plan(&used, &generated, &template);
        assert_eq!(plan.map["Heading1"], "berschrift1");
        assert_eq!(plan.map["Code"], "Code", "a paragraph style is no character style");
        assert_eq!(plan.added.len(), 1);
        assert!(!plan.map.contains_key("Gone"));
    }

    #[test]
    fn an_id_in_use_gives_way_and_dependencies_come_along_even_in_a_circle() {
        let template = doc(&[
            style("A", "other", ""),
            style("A1", "another", ""),
            style("Basis", "base", ""),
        ]);
        let generated = doc(&[
            style(
                "A",
                "first",
                "<w:basedOn w:val=\"Base\"/><w:next w:val=\"B\"/><w:link w:val=\"Nowhere\"/>",
            ),
            style("A", "duplicate", ""),
            style("B", "second", "<w:next w:val=\"A\"/>"),
            style("Base", "base", ""),
        ]);
        let (template, generated) = (
            Document::parse(&template).unwrap(),
            Document::parse(&generated).unwrap(),
        );
        let used = ["A".to_string()];
        let plan = plan(&used, &generated, &template);
        assert_eq!(plan.map["A"], "A2");
        assert_eq!(plan.map["B"], "B");
        assert_eq!(plan.map["Base"], "Basis");
        let added: Vec<&str> = plan
            .added
            .iter()
            .filter_map(|n| n.attribute((WORD, "styleId")))
            .collect();
        assert_eq!(added, ["A", "B"]);
    }
}
