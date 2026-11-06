//! Elements of one Word file written into a part of another. Parts stay what they are in their bytes, apart from what is
//! inserted, so what the source declares for its elements has to be said again in terms of the target's root.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use roxmltree::Node;
use splat_ooxml::xml::{escape_attribute, escape_text};

pub(super) const WORD: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
pub(super) const RELATIONSHIPS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const XML: &str = "http://www.w3.org/XML/1998/namespace";

/// The XML declaration of a part that is written anew.
pub(super) const DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

/// Which prefix means which namespace in the part being written, and what still has to be declared on its root.
pub(super) struct Names {
    prefixes: HashMap<String, String>,
    default: Option<String>,
    taken: HashSet<String>,
    added: Vec<(String, String)>,
}

impl Names {
    /// No namespace in scope, as for a part of its own.
    pub(super) fn none() -> Names {
        Names {
            prefixes: HashMap::new(),
            default: None,
            taken: HashSet::new(),
            added: Vec::new(),
        }
    }

    /// The namespaces in scope on `element`.
    pub(super) fn of(element: Node) -> Names {
        let mut names = Names::none();
        for space in element.namespaces() {
            match space.name() {
                Some(prefix) => {
                    names.taken.insert(prefix.to_string());
                    names
                        .prefixes
                        .entry(space.uri().to_string())
                        .or_insert_with(|| prefix.to_string());
                }
                None => names.default = Some(space.uri().to_string()),
            }
        }
        names
    }

    /// Writes a name in namespace `uri` as the target can read it, declaring the namespace if it has to; `source` is where
    /// the name comes from and suggests the prefix.
    pub(super) fn write_name(
        &mut self,
        uri: Option<&str>,
        local: &str,
        is_element: bool,
        source: Node,
        out: &mut String,
    ) {
        if let Some(uri) = uri {
            let by_default = is_element && self.default.as_deref() == Some(uri);
            if uri == XML {
                out.push_str("xml:");
            } else if !by_default {
                if !self.prefixes.contains_key(uri) {
                    self.declare(uri, source);
                }
                if let Some(prefix) = self.prefixes.get(uri) {
                    out.push_str(prefix);
                    out.push(':');
                }
            }
        }
        out.push_str(local);
    }

    fn declare(&mut self, uri: &str, source: Node) {
        let wished = source.lookup_prefix(uri).filter(|prefix| !self.taken.contains(*prefix));
        let prefix = match wished {
            Some(prefix) => prefix.to_string(),
            None => (1..)
                .map(|n| format!("ns{n}"))
                .find(|prefix| !self.taken.contains(prefix))
                .unwrap_or_default(),
        };
        self.taken.insert(prefix.clone());
        self.added.push((prefix.clone(), uri.to_string()));
        self.prefixes.insert(uri.to_string(), prefix);
    }

    /// What goes into the root's start tag for all that was declared here.
    pub(super) fn declarations(&self) -> String {
        self.added
            .iter()
            .map(|(prefix, uri)| format!(" xmlns:{prefix}=\"{}\"", escape_attribute(uri)))
            .collect()
    }
}

/// How identifiers of the generated file read in the target.
#[derive(Default)]
pub(super) struct Rewrite {
    pub(super) styles: HashMap<String, String>,
    pub(super) numbers: HashMap<String, String>,
    pub(super) abstract_numbers: HashMap<String, String>,
    /// Of the part being written: each part has relationships of its own.
    pub(super) relationships: HashMap<String, String>,
    pub(super) footnotes: HashMap<String, String>,
    pub(super) endnotes: HashMap<String, String>,
    /// Styles are being added to those of the template, which has its default style already.
    pub(super) adding_styles: bool,
}

impl Rewrite {
    /// The value an attribute of an element has in the target, if it has any: a reference to something that did not come
    /// along would dangle, and two default styles in one file are one too many.
    fn value<'v>(&'v self, element: &Node, attribute: (Option<&str>, &str), value: &'v str) -> Option<&'v str> {
        if attribute.0 == Some(RELATIONSHIPS) {
            return self.relationships.get(value).map(String::as_str);
        }
        if attribute.0 != Some(WORD) || element.tag_name().namespace() != Some(WORD) {
            return Some(value);
        }
        let mapped = |map: &'v HashMap<String, String>| map.get(value).map_or(value, String::as_str);
        match (element.tag_name().name(), attribute.1) {
            ("pStyle" | "rStyle" | "tblStyle" | "basedOn" | "next" | "link", "val") | ("style", "styleId") => {
                Some(mapped(&self.styles))
            }
            ("style", "default") if self.adding_styles => None,
            ("numId", "val") | ("num", "numId") => Some(mapped(&self.numbers)),
            ("abstractNum", "abstractNumId") | ("abstractNumId", "val") => Some(mapped(&self.abstract_numbers)),
            ("footnote", "id") => Some(mapped(&self.footnotes)),
            ("endnote", "id") => Some(mapped(&self.endnotes)),
            ("footnoteReference", "id") => self.footnotes.get(value).map(String::as_str),
            ("endnoteReference", "id") => self.endnotes.get(value).map(String::as_str),
            _ => Some(value),
        }
    }
}

/// Writes `start` and all it holds in terms of `names` and `rewrite`, without recursion, as a file may nest deeply.
pub(super) fn write_tree(start: Node, names: &mut Names, rewrite: &Rewrite, out: &mut String) {
    let mut node = start;
    loop {
        write_start(node, names, rewrite, out);
        if let Some(child) = node.first_child() {
            node = child;
            continue;
        }
        loop {
            if node.is_element() && node.has_children() {
                out.push_str("</");
                names.write_name(node.tag_name().namespace(), node.tag_name().name(), true, node, out);
                out.push('>');
            }
            if node == start {
                return;
            }
            match node.next_sibling() {
                Some(next) => {
                    node = next;
                    break;
                }
                None => node = node.parent().unwrap_or(start),
            }
        }
    }
}

fn write_start(node: Node, names: &mut Names, rewrite: &Rewrite, out: &mut String) {
    if node.is_text() {
        out.push_str(&escape_text(node.text().unwrap_or("")));
    }
    if !node.is_element() {
        return;
    }
    out.push('<');
    names.write_name(node.tag_name().namespace(), node.tag_name().name(), true, node, out);
    for attribute in node.attributes() {
        let name = (attribute.namespace(), attribute.name());
        if let Some(value) = rewrite.value(&node, name, attribute.value()) {
            out.push(' ');
            names.write_name(name.0, name.1, false, node, out);
            out.push_str("=\"");
            out.push_str(&escape_attribute(value));
            out.push('"');
        }
    }
    out.push_str(if node.has_children() { ">" } else { "/>" });
}

/// `root` written as a part of its own.
pub(super) fn standalone(root: Node, text: &mut String, rewrite: &Rewrite) {
    let mut names = Names::none();
    let mut element = String::new();
    write_tree(root, &mut names, rewrite, &mut element);
    let name_len = element
        .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .unwrap_or(element.len());
    text.push_str(DECLARATION);
    text.push_str(&element[..name_len]);
    text.push_str(&names.declarations());
    text.push_str(&element[name_len..]);
}

/// Changes to the text of a part, each at a place in the original; those at the same place stay in the order they came.
pub(super) struct Edits<'t> {
    text: &'t str,
    changes: Vec<(Range<usize>, String)>,
}

impl<'t> Edits<'t> {
    pub(super) fn new(text: &'t str) -> Edits<'t> {
        Edits {
            text,
            changes: Vec::new(),
        }
    }

    pub(super) fn replace(&mut self, range: Range<usize>, with: String) {
        self.changes.push((range, with));
    }

    pub(super) fn insert(&mut self, at: usize, with: String) {
        self.replace(at..at, with);
    }

    /// The offset behind the name in the start tag of `element`, where attributes can be added.
    pub(super) fn behind_name(&self, element: Node) -> usize {
        let start = element.range().start + 1;
        let rest = self.text.get(start..).unwrap_or("");
        start
            + rest
                .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
                .unwrap_or(0)
    }

    /// Adds `content` behind the last child of `parent`, opening it if it was written as an empty tag.
    pub(super) fn append_children(&mut self, parent: Node, content: String) {
        let range = parent.range();
        if let Some(last) = parent.last_child() {
            self.insert(last.range().end, content);
            return;
        }
        let written = self.text.get(range.clone()).unwrap_or("");
        if written.ends_with("/>") {
            let name = self.text.get(range.start + 1..self.behind_name(parent)).unwrap_or("");
            let with = format!(">{content}</{name}>");
            self.replace(range.end - 2..range.end, with);
        } else {
            let close = written.rfind("</").unwrap_or(0);
            self.insert(range.start + close, content);
        }
    }

    pub(super) fn finish(mut self) -> String {
        self.changes.sort_by_key(|(range, _)| range.start);
        let mut out = String::with_capacity(self.text.len());
        let mut done = 0;
        for (range, with) in &self.changes {
            out.push_str(self.text.get(done..range.start).unwrap_or(""));
            out.push_str(with);
            done = done.max(range.end);
        }
        out.push_str(self.text.get(done..).unwrap_or(""));
        out
    }
}

/// The text of a part with `nodes` added behind what its root holds.
pub(super) fn with_children_added(text: &str, root: Node, nodes: &[Node], rewrite: &Rewrite) -> String {
    let mut names = Names::of(root);
    let mut content = String::new();
    for node in nodes {
        write_tree(*node, &mut names, rewrite, &mut content);
    }
    let mut edits = Edits::new(text);
    edits.insert(edits.behind_name(root), names.declarations());
    edits.append_children(root, content);
    edits.finish()
}

/// Whether `node` is the WordprocessingML element of this local name.
pub(super) fn is_word(node: &Node, name: &str) -> bool {
    node.tag_name().name() == name && node.tag_name().namespace() == Some(WORD)
}

/// The `w:val` of a child element.
pub(super) fn child_value<'a>(node: &Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.children()
        .find(|child| is_word(child, name))
        .and_then(|child| child.attribute((WORD, "val")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use roxmltree::Document;

    #[test]
    fn a_tree_is_written_with_the_prefixes_of_the_target() {
        let source = Document::parse(
            "<a:r xmlns:a=\"urn:a\" xmlns:b=\"urn:b\" xml:space=\"preserve\" b:c=\"1&quot;&#9;2\"><a:t>x &amp; y<!-- c --></a:t>\
             <b:u/><?pi x?></a:r>",
        )
        .unwrap();
        let target = Document::parse("<z xmlns=\"urn:a\" xmlns:q=\"urn:b\"/>").unwrap();
        let mut names = Names::of(target.root_element());
        let mut out = String::new();
        write_tree(source.root_element(), &mut names, &Rewrite::default(), &mut out);
        assert_eq!(
            out,
            "<r xml:space=\"preserve\" q:c=\"1&quot;&#9;2\"><t>x &amp; y</t><q:u/></r>"
        );
        assert_eq!(names.declarations(), "");
    }

    #[test]
    fn a_prefix_that_means_something_else_in_the_target_gives_way() {
        let source = Document::parse("<a:r xmlns:a=\"urn:a\" a:b=\"1\" xmlns=\"urn:c\"><d/></a:r>").unwrap();
        let target = Document::parse("<a:z xmlns:a=\"urn:other\" xmlns:ns1=\"urn:o2\"/>").unwrap();
        let mut names = Names::of(target.root_element());
        let mut out = String::new();
        write_tree(source.root_element(), &mut names, &Rewrite::default(), &mut out);
        assert_eq!(out, "<ns2:r ns2:b=\"1\"><ns3:d/></ns2:r>");
        assert_eq!(names.declarations(), " xmlns:ns2=\"urn:a\" xmlns:ns3=\"urn:c\"");
    }

    #[test]
    fn an_attribute_needs_a_prefix_where_elements_need_none() {
        let source = Document::parse("<w:r xmlns:w=\"urn:w\" w:b=\"1\"/>").unwrap();
        let target = Document::parse("<z xmlns=\"urn:w\"/>").unwrap();
        let mut names = Names::of(target.root_element());
        let mut out = String::new();
        write_tree(source.root_element(), &mut names, &Rewrite::default(), &mut out);
        assert_eq!(out, "<r w:b=\"1\"/>");
        assert_eq!(names.declarations(), " xmlns:w=\"urn:w\"");
    }

    #[test]
    fn edits_at_one_place_keep_their_order_and_empty_parents_open() {
        let text = "<a x=\"1\"/>";
        let doc = Document::parse(text).unwrap();
        let root = doc.root_element();
        let mut edits = Edits::new(text);
        edits.insert(edits.behind_name(root), " y=\"2\"".to_string());
        edits.append_children(root, "<b/>".to_string());
        assert_eq!(edits.finish(), "<a y=\"2\" x=\"1\"><b/></a>");

        let bare = "<a/>";
        let doc = Document::parse(bare).unwrap();
        let mut edits = Edits::new(bare);
        edits.insert(edits.behind_name(doc.root_element()), " y=\"2\"".to_string());
        edits.append_children(doc.root_element(), "<b/>".to_string());
        assert_eq!(edits.finish(), "<a y=\"2\"><b/></a>");

        let closed = "<a></a >";
        let doc = Document::parse(closed).unwrap();
        let mut edits = Edits::new(closed);
        edits.append_children(doc.root_element(), "<b/>".to_string());
        assert_eq!(edits.finish(), "<a><b/></a >");

        let full = "<a><c/> </a>";
        let doc = Document::parse(full).unwrap();
        let mut edits = Edits::new(full);
        edits.append_children(doc.root_element(), "<b/>".to_string());
        assert_eq!(edits.finish(), "<a><c/> <b/></a>");
    }

    #[test]
    fn a_part_of_its_own_declares_what_it_uses() {
        let source = Document::parse("<w:s xmlns:w=\"urn:w\"><w:t/></w:s>").unwrap();
        let mut out = String::new();
        standalone(source.root_element(), &mut out, &Rewrite::default());
        assert_eq!(out, format!("{DECLARATION}<w:s xmlns:w=\"urn:w\"><w:t/></w:s>"));
    }

    #[test]
    fn deep_nesting_is_written_without_a_stack() {
        let depth = 100_000;
        let text = format!("{}x{}", "<a>".repeat(depth), "</a>".repeat(depth));
        let doc = crate::xml::parse(&text, &crate::xml::XmlBudget::default()).unwrap();
        let mut out = String::new();
        write_tree(doc.root_element(), &mut Names::none(), &Rewrite::default(), &mut out);
        assert_eq!(out, text);
    }
}
