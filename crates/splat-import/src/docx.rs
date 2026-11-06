//! Word files as Markdown and the pictures they hold.

mod body;
mod builder;
pub(crate) mod math;
mod numbering;
mod package;
mod runs;
mod styles;
mod tables;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::fmt;

use roxmltree::Document;

use crate::package::{
    DOCUMENT_TYPE, MACRO_DOCUMENT_TYPE, MACRO_TEMPLATE_TYPE, Package, Relationships, TEMPLATE_TYPE, WordError,
};
use crate::rich::Rich;
use crate::xml::{XmlBudget, parse};
use body::{NotesPart, Walker};
use numbering::Numbering;
use package::{W, w_attr, w_val};
use styles::Styles;

/// A picture from a Word file, with the name that the Markdown refers to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Media {
    /// The name the Markdown refers to, in the images folder `import` was given.
    pub file_name: String,
    /// The picture as the Word file holds it.
    pub bytes: Vec<u8>,
}

/// A picture the Markdown goes without; its description stands where it was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissingPicture {
    /// The part of the Word file it was to come from, or the relationship that names none.
    pub part: String,
    /// Why it could not be had.
    pub reason: MissingReason,
}

/// Why a picture of a Word file could not be had.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingReason {
    /// A relationship that the file does not have, or one to something outside it.
    NoPart,
    /// A part the file names but does not hold.
    Absent,
    /// A part that cannot be read.
    Damaged,
    /// A part larger than any one entry of the file may inflate to.
    TooLarge,
    /// Bytes in no picture format a Markdown file can point to.
    UnknownFormat,
}

impl fmt::Display for MissingReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            MissingReason::NoPart => "no part of the file",
            MissingReason::Absent => "not in the file",
            MissingReason::Damaged => "damaged",
            MissingReason::TooLarge => "too large",
            MissingReason::UnknownFormat => "in no known picture format",
        })
    }
}

impl fmt::Display for MissingPicture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "picture {}: {}", self.part, self.reason)
    }
}

/// A Word file as Markdown with its pictures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Imported {
    /// The text, ending with a line break unless it is empty.
    pub markdown: String,
    /// The pictures the Markdown refers to.
    pub media: Vec<Media>,
    /// The pictures the Markdown goes without, each once.
    pub missing: Vec<MissingPicture>,
}

/// The main parts a Word file is read for.
const MAIN_TYPES: [&str; 4] = [DOCUMENT_TYPE, TEMPLATE_TYPE, MACRO_DOCUMENT_TYPE, MACRO_TEMPLATE_TYPE];

fn part_document<'t>(text: Option<&'t String>, budget: &XmlBudget) -> Result<Option<Document<'t>>, WordError> {
    Ok(text.map(|text| parse(text, budget)).transpose()?)
}

/// The notes of a footnotes or endnotes part by their number.
fn notes_by_id<'n, 'i>(document: Option<&'n Document<'i>>, tag: &str) -> HashMap<String, roxmltree::Node<'n, 'i>> {
    let Some(document) = document else {
        return HashMap::new();
    };
    document
        .root_element()
        .children()
        .filter(|node| package::is_w(*node, tag))
        .filter_map(|node| Some((w_attr(node, "id")?.to_string(), node)))
        .collect()
}

/// What decides how paragraphs read and that needs the whole document: whether a heading 1 exists, and which bookmarks
/// a link leads to.
fn look_through(document: &Document, styles: &Styles) -> (bool, HashSet<String>) {
    let mut has_heading_1 = false;
    let mut anchors = HashSet::new();
    for node in document.descendants() {
        if node.tag_name().namespace() != Some(W) {
            continue;
        }
        match node.tag_name().name() {
            "pStyle" => has_heading_1 |= styles.paragraph(w_val(node)).heading == Some(1),
            "outlineLvl" => has_heading_1 |= w_val(node) == Some("0"),
            "hyperlink" => {
                if let Some(anchor) = w_attr(node, "anchor") {
                    anchors.insert(anchor.to_string());
                }
            }
            _ => {}
        }
    }
    (has_heading_1, anchors)
}

/// The document in `bytes`, whose pictures go into `images_folder` next to the Markdown.
pub fn import(bytes: &[u8], images_folder: &str) -> Result<Imported, WordError> {
    import_within(bytes, images_folder, &XmlBudget::default())
}

/// `import` with the XML of all parts coming out of one `budget`.
pub(crate) fn import_within(bytes: &[u8], images_folder: &str, budget: &XmlBudget) -> Result<Imported, WordError> {
    let package = Package::open(bytes, &MAIN_TYPES, budget)?;
    let main_text = package.text(&package.main)?.ok_or(WordError::Damaged)?;
    let rels = package.relationships(&package.main)?;
    let related = |kind: &str| -> Result<Option<String>, WordError> {
        match rels.of_type(kind).and_then(|rel| rel.part.as_deref()) {
            Some(part) => Ok(package.text(part)?),
            None => Ok(None),
        }
    };
    let styles_text = related("styles")?;
    let numbering_text = related("numbering")?;
    let footnotes_text = related("footnotes")?;
    let endnotes_text = related("endnotes")?;

    let main_document = parse(&main_text, budget)?;
    let styles =
        part_document(styles_text.as_ref(), budget)?.map_or_else(Styles::default, |document| Styles::read(&document));
    let numbering = part_document(numbering_text.as_ref(), budget)?
        .map_or_else(Numbering::default, |document| Numbering::read(&document));
    let footnotes = part_document(footnotes_text.as_ref(), budget)?;
    let endnotes = part_document(endnotes_text.as_ref(), budget)?;
    let foot_rels = note_rels(&package, &rels, "footnotes")?;
    let end_rels = note_rels(&package, &rels, "endnotes")?;

    let body = main_document
        .root_element()
        .children()
        .find(|node| package::is_w(*node, "body"))
        .ok_or(WordError::Damaged)?;
    let (has_heading_1, anchors) = look_through(&main_document, &styles);
    let mut walker = Walker::new(
        &package.archive,
        &styles,
        &numbering,
        &rels,
        images_folder,
        &anchors,
        has_heading_1,
        [
            NotesPart {
                notes: notes_by_id(footnotes.as_ref(), "footnote"),
                rels: &foot_rels,
            },
            NotesPart {
                notes: notes_by_id(endnotes.as_ref(), "endnote"),
                rels: &end_rels,
            },
        ],
    );
    let blocks = walker.document(body);
    let (media, missing) = walker.finish()?;
    Ok(Imported {
        markdown: Rich { blocks }.into_markdown(),
        media,
        missing,
    })
}

fn note_rels(package: &Package, rels: &Relationships, kind: &str) -> Result<Relationships, WordError> {
    match rels.of_type(kind).and_then(|rel| rel.part.as_deref()) {
        Some(part) => Ok(package.relationships(part)?),
        None => Ok(Relationships::default()),
    }
}
