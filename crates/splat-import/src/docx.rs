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
    DOCUMENT_TYPE, MACRO_DOCUMENT_TYPE, MACRO_TEMPLATE_TYPE, Package, PackageError, Relationships, TEMPLATE_TYPE,
};
use crate::rich::Rich;
use crate::xml::{XmlError, parse};
use crate::zip::ZipError;
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
    /// Bytes in no picture format a Markdown file can point to.
    UnknownFormat,
}

impl fmt::Display for MissingReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            MissingReason::NoPart => "no part of the file",
            MissingReason::Absent => "not in the file",
            MissingReason::Damaged => "damaged",
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

/// Why a Word file cannot be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportError {
    /// Another kind of file, an old `.doc` among them.
    NotAWordFile,
    /// A file protected by a password.
    Encrypted,
    /// More than the import reads into memory.
    TooLarge,
    /// A Word file whose structure is broken.
    Damaged,
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ImportError::NotAWordFile => "not a Word file in the .docx format",
            ImportError::Encrypted => "the Word file is protected by a password",
            ImportError::TooLarge => "the Word file is larger than an import may read",
            ImportError::Damaged => "the Word file is damaged",
        })
    }
}

impl std::error::Error for ImportError {}

impl From<PackageError> for ImportError {
    fn from(error: PackageError) -> ImportError {
        match error {
            PackageError::NotAWordFile => ImportError::NotAWordFile,
            PackageError::Encrypted => ImportError::Encrypted,
            PackageError::TooLarge => ImportError::TooLarge,
            PackageError::Damaged => ImportError::Damaged,
        }
    }
}

impl From<ZipError> for ImportError {
    fn from(error: ZipError) -> ImportError {
        PackageError::from(error).into()
    }
}

impl From<XmlError> for ImportError {
    fn from(error: XmlError) -> ImportError {
        PackageError::from(error).into()
    }
}

/// The main parts a Word file is read for.
const MAIN_TYPES: [&str; 4] = [DOCUMENT_TYPE, TEMPLATE_TYPE, MACRO_DOCUMENT_TYPE, MACRO_TEMPLATE_TYPE];

fn part_document(text: Option<&String>) -> Result<Option<Document<'_>>, ImportError> {
    Ok(text.map(|text| parse(text)).transpose()?)
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
pub fn import(bytes: &[u8], images_folder: &str) -> Result<Imported, ImportError> {
    let package = Package::open(bytes, &MAIN_TYPES)?;
    let main_text = package.text(&package.main)?.ok_or(ImportError::Damaged)?;
    let rels = package.relationships(&package.main)?;
    let related = |kind: &str| -> Result<Option<String>, ImportError> {
        match rels.of_type(kind).and_then(|rel| rel.part.as_deref()) {
            Some(part) => Ok(package.text(part)?),
            None => Ok(None),
        }
    };
    let styles_text = related("styles")?;
    let numbering_text = related("numbering")?;
    let footnotes_text = related("footnotes")?;
    let endnotes_text = related("endnotes")?;

    let main_document = parse(&main_text)?;
    let styles = part_document(styles_text.as_ref())?.map_or_else(Styles::default, |document| Styles::read(&document));
    let numbering =
        part_document(numbering_text.as_ref())?.map_or_else(Numbering::default, |document| Numbering::read(&document));
    let footnotes = part_document(footnotes_text.as_ref())?;
    let endnotes = part_document(endnotes_text.as_ref())?;
    let foot_rels = note_rels(&package, &rels, "footnotes")?;
    let end_rels = note_rels(&package, &rels, "endnotes")?;

    let body = main_document
        .root_element()
        .children()
        .find(|node| package::is_w(*node, "body"))
        .ok_or(ImportError::Damaged)?;
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

fn note_rels(package: &Package, rels: &Relationships, kind: &str) -> Result<Relationships, ImportError> {
    match rels.of_type(kind).and_then(|rel| rel.part.as_deref()) {
        Some(part) => Ok(package.relationships(part)?),
        None => Ok(Relationships::default()),
    }
}
