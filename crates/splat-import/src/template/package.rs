//! A Word file as a package: which part is what, by its content type and by the relationships between parts.

use roxmltree::Node;
use splat_ooxml::xml::escape_attribute;

use super::WordError;
use super::serialize::{DECLARATION, Edits, Names};
use crate::package::{DOCUMENT_TYPE, Package, Relationships, TEMPLATE_TYPE, folder_of, rels_name};
use crate::xml::{self, XmlBudget};

const PACKAGE_RELATIONSHIPS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// A part read as text, with the kind of relationship that leads to it.
pub(super) struct PartText {
    pub(super) name: String,
    pub(super) kind: String,
    pub(super) text: String,
}

/// A Word file opened, with the relationships of its main part.
pub(super) struct Word<'a> {
    pub(super) package: Package<'a>,
    pub(super) rels: Relationships,
}

impl<'a> Word<'a> {
    /// A document or a template in `bytes`; a file with macros would carry them into a document that cannot have them.
    pub(super) fn open(bytes: &'a [u8], budget: &'a XmlBudget) -> Result<Word<'a>, WordError> {
        let package = Package::open(bytes, &[DOCUMENT_TYPE, TEMPLATE_TYPE], budget)?;
        let rels = package.relationships(&package.main)?;
        Ok(Word { package, rels })
    }

    /// The part the main part relates to by a type whose last word is `kind`, as `styles`.
    pub(super) fn related(&self, kind: &str) -> Result<Option<PartText>, WordError> {
        let Some((rel, name)) = self.rels.of_type(kind).and_then(|rel| Some((rel, rel.part.clone()?))) else {
            return Ok(None);
        };
        Ok(self.package.text(&name)?.map(|text| PartText {
            name,
            kind: rel.kind.clone(),
            text,
        }))
    }
}

/// How a part reads as a relationship's target from `folder`.
pub(super) fn target_from(folder: &str, part: &str) -> String {
    if folder.is_empty() {
        return part.to_string();
    }
    match part.strip_prefix(folder).and_then(|rest| rest.strip_prefix('/')) {
        Some(relative) => relative.to_string(),
        None => format!("/{part}"),
    }
}

/// The relationships of a part in the template, with those being added.
pub(super) struct RelsPart {
    /// The name of this part.
    pub(super) name: String,
    /// The folder of the part they belong to.
    pub(super) folder: String,
    existing: Option<String>,
    next: u64,
    added: Vec<(String, String, String, bool)>,
}

impl RelsPart {
    /// What the template has for its part `part`, and if `part` is not there yet, nothing.
    pub(super) fn of(word: &Word, part: &str) -> Result<RelsPart, WordError> {
        let name = rels_name(part);
        let existing = word.package.text(&name)?;
        let highest = match &existing {
            Some(text) => Relationships::read(text, "", word.package.budget)?
                .iter()
                .filter_map(|rel| rel.id.strip_prefix("rId")?.parse::<u32>().ok())
                .max()
                .unwrap_or(0),
            None => 0,
        };
        Ok(RelsPart {
            name,
            folder: folder_of(part).to_string(),
            existing,
            next: u64::from(highest) + 1,
            added: Vec::new(),
        })
    }

    /// Adds a relationship and gives its id, above all the template has.
    pub(super) fn add(&mut self, kind: &str, target: &str, external: bool) -> String {
        let id = format!("rId{}", self.next);
        self.next += 1;
        self.added
            .push((id.clone(), kind.to_string(), target.to_string(), external));
        id
    }

    /// The part's text if something was added; reading what the template has takes from `budget`.
    pub(super) fn finish(self, budget: &XmlBudget) -> Result<Option<(String, String)>, WordError> {
        if self.added.is_empty() {
            return Ok(None);
        }
        let text = match &self.existing {
            Some(text) => {
                let doc = xml::parse(text, budget)?;
                let root = doc.root_element();
                let mut names = Names::of(root);
                let mut element = String::new();
                names.write_name(Some(PACKAGE_RELATIONSHIPS), "Relationship", true, root, &mut element);
                let content = self.added_elements(&element);
                let mut edits = Edits::new(text);
                edits.insert(edits.behind_name(root), names.declarations());
                edits.append_children(root, content);
                edits.finish()
            }
            None => format!(
                "{DECLARATION}<Relationships xmlns=\"{PACKAGE_RELATIONSHIPS}\">{}</Relationships>",
                self.added_elements("Relationship")
            ),
        };
        Ok(Some((self.name, text)))
    }

    fn added_elements(&self, element: &str) -> String {
        self.added
            .iter()
            .map(|(id, kind, target, external)| {
                format!(
                    "<{element} Id=\"{id}\" Type=\"{}\" Target=\"{}\"{}/>",
                    escape_attribute(kind),
                    escape_attribute(target),
                    if *external { " TargetMode=\"External\"" } else { "" }
                )
            })
            .collect()
    }
}

/// The template's `[Content_Types].xml` with what the merge adds to it, and the main part as a document.
pub(super) fn merged_types(
    text: &str,
    main: &str,
    defaults: &[(String, String)],
    overrides: &[(String, String)],
    budget: &XmlBudget,
) -> Result<String, WordError> {
    let doc = xml::parse(text, budget)?;
    let root = doc.root_element();
    let mut edits = Edits::new(text);
    let main_override = root.children().find(|node| {
        node.tag_name().name() == "Override"
            && node.attribute("PartName").map(|name| name.trim_start_matches('/')) == Some(main)
    });
    if let Some(attribute) = main_override
        .iter()
        .flat_map(Node::attributes)
        .find(|attribute| attribute.name() == "ContentType" && attribute.value() == TEMPLATE_TYPE)
    {
        edits.replace(attribute.range(), format!("ContentType=\"{DOCUMENT_TYPE}\""));
    }
    let uri = root.tag_name().namespace();
    let mut names = Names::of(root);
    let mut content = String::new();
    for (extension, kind) in defaults {
        write_type(
            &mut names,
            uri,
            root,
            "Default",
            ("Extension", extension),
            kind,
            &mut content,
        );
    }
    for (name, kind) in overrides {
        let part = format!("/{name}");
        write_type(
            &mut names,
            uri,
            root,
            "Override",
            ("PartName", &part),
            kind,
            &mut content,
        );
    }
    if !content.is_empty() {
        edits.append_children(root, content);
    }
    Ok(edits.finish())
}

fn write_type(
    names: &mut Names,
    uri: Option<&str>,
    root: Node,
    element: &str,
    key: (&str, &str),
    kind: &str,
    out: &mut String,
) {
    out.push('<');
    names.write_name(uri, element, true, root, out);
    out.push_str(&format!(
        " {}=\"{}\" ContentType=\"{}\"/>",
        key.0,
        escape_attribute(key.1),
        escape_attribute(kind)
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_target_is_relative_where_it_can_be() {
        assert_eq!(target_from("word", "word/media/a.png"), "media/a.png");
        assert_eq!(target_from("doc", "word/media/a.png"), "/word/media/a.png");
        assert_eq!(target_from("word", "wordy/a.png"), "/wordy/a.png");
        assert_eq!(target_from("", "word/a.png"), "word/a.png");
    }

    #[test]
    fn types_under_another_prefix_get_it_too() {
        let text = "<t:Types xmlns:t=\"urn:t\"><t:Override PartName=\"/m.xml\" ContentType=\"a\"/></t:Types>";
        let merged = merged_types(
            text,
            "m.xml",
            &[("png".to_string(), "image/png".to_string())],
            &[("x.xml".to_string(), "b\"".to_string())],
            &XmlBudget::default(),
        )
        .unwrap();
        assert_eq!(
            merged,
            "<t:Types xmlns:t=\"urn:t\"><t:Override PartName=\"/m.xml\" ContentType=\"a\"/>\
             <t:Default Extension=\"png\" ContentType=\"image/png\"/><t:Override PartName=\"/x.xml\" ContentType=\"b&quot;\"/></t:Types>"
        );
    }
}
