//! A Word file as a package of parts by the Open Packaging Conventions (ECMA-376 Part 2): which part is the main
//! document, how parts relate, and their content, always in the transitional vocabulary, whichever of the two forms of
//! ISO/IEC 29500 the file is written in.

use std::collections::HashMap;

use crate::xml::{self, XmlError};
use crate::zip::{Archive, ZipError};

/// Why a file cannot be read as a Word package.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PackageError {
    NotAWordFile,
    Encrypted,
    TooLarge,
    Damaged,
}

impl From<ZipError> for PackageError {
    fn from(error: ZipError) -> PackageError {
        match error {
            ZipError::NotAnArchive => PackageError::NotAWordFile,
            ZipError::Encrypted => PackageError::Encrypted,
            ZipError::Zip64 | ZipError::TooLarge => PackageError::TooLarge,
            ZipError::Damaged => PackageError::Damaged,
        }
    }
}

impl From<XmlError> for PackageError {
    fn from(error: XmlError) -> PackageError {
        match error {
            XmlError::TooLarge => PackageError::TooLarge,
            XmlError::Damaged => PackageError::Damaged,
        }
    }
}

pub(crate) const TYPES_PART: &str = "[Content_Types].xml";
pub(crate) const DOCUMENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
pub(crate) const TEMPLATE_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.template.main+xml";
pub(crate) const MACRO_DOCUMENT_TYPE: &str = "application/vnd.ms-word.document.macroEnabled.main+xml";
pub(crate) const MACRO_TEMPLATE_TYPE: &str = "application/vnd.ms-word.template.macroEnabledTemplate.main+xml";

/// More than any part of a Word file that is not made to hurt; a part read as XML stays in memory whole.
pub(crate) const MOST_XML_BYTES: u64 = 128 << 20;

pub(crate) const OLE_MAGIC: &[u8] = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1";

/// An old `.doc` or an Office file with a password: both are OLE files, and only the second names this stream.
fn ole_error(bytes: &[u8]) -> PackageError {
    let name: Vec<u8> = "EncryptedPackage".encode_utf16().flat_map(u16::to_le_bytes).collect();
    if bytes.windows(name.len()).any(|window| window == name) {
        PackageError::Encrypted
    } else {
        PackageError::NotAWordFile
    }
}

/// The namespaces of the Strict form with their twins in the transitional one.
const STRICT_NAMESPACES: [(&str, &str); 20] = [
    (
        "http://purl.oclc.org/ooxml/wordprocessingml/main",
        "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
    ),
    (
        "http://purl.oclc.org/ooxml/officeDocument/relationships",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    ),
    (
        "http://purl.oclc.org/ooxml/officeDocument/math",
        "http://schemas.openxmlformats.org/officeDocument/2006/math",
    ),
    (
        "http://purl.oclc.org/ooxml/officeDocument/sharedTypes",
        "http://schemas.openxmlformats.org/officeDocument/2006/sharedTypes",
    ),
    (
        "http://purl.oclc.org/ooxml/officeDocument/extendedProperties",
        "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties",
    ),
    (
        "http://purl.oclc.org/ooxml/officeDocument/customProperties",
        "http://schemas.openxmlformats.org/officeDocument/2006/custom-properties",
    ),
    (
        "http://purl.oclc.org/ooxml/officeDocument/docPropsVTypes",
        "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes",
    ),
    (
        "http://purl.oclc.org/ooxml/officeDocument/bibliography",
        "http://schemas.openxmlformats.org/officeDocument/2006/bibliography",
    ),
    (
        "http://purl.oclc.org/ooxml/officeDocument/customXml",
        "http://schemas.openxmlformats.org/officeDocument/2006/customXml",
    ),
    (
        "http://purl.oclc.org/ooxml/schemaLibrary/main",
        "http://schemas.openxmlformats.org/schemaLibrary/2006/main",
    ),
    (
        "http://purl.oclc.org/ooxml/drawingml/main",
        "http://schemas.openxmlformats.org/drawingml/2006/main",
    ),
    (
        "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing",
        "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
    ),
    (
        "http://purl.oclc.org/ooxml/drawingml/spreadsheetDrawing",
        "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing",
    ),
    (
        "http://purl.oclc.org/ooxml/drawingml/picture",
        "http://schemas.openxmlformats.org/drawingml/2006/picture",
    ),
    (
        "http://purl.oclc.org/ooxml/drawingml/chart",
        "http://schemas.openxmlformats.org/drawingml/2006/chart",
    ),
    (
        "http://purl.oclc.org/ooxml/drawingml/chartDrawing",
        "http://schemas.openxmlformats.org/drawingml/2006/chartDrawing",
    ),
    (
        "http://purl.oclc.org/ooxml/drawingml/diagram",
        "http://schemas.openxmlformats.org/drawingml/2006/diagram",
    ),
    (
        "http://purl.oclc.org/ooxml/drawingml/lockedCanvas",
        "http://schemas.openxmlformats.org/drawingml/2006/lockedCanvas",
    ),
    (
        "http://purl.oclc.org/ooxml/spreadsheetml/main",
        "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
    ),
    (
        "http://purl.oclc.org/ooxml/presentationml/main",
        "http://schemas.openxmlformats.org/presentationml/2006/main",
    ),
];

const STRICT_RELATIONSHIP_TYPES: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/";
const RELATIONSHIP_TYPES: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";

/// The transitional twin of an attribute's value in the Strict form; `None` for one that is the same in both.
fn usual_value(name: &str, value: &str) -> Option<String> {
    if name == "conformance" || name.ends_with(":conformance") {
        return (value == "strict").then(|| "transitional".to_string());
    }
    let names_a_vocabulary = name == "xmlns" || name.starts_with("xmlns:") || name == "Type" || name == "uri";
    if !names_a_vocabulary {
        return None;
    }
    if let Some((_, usual)) = STRICT_NAMESPACES.iter().find(|(strict, _)| *strict == value) {
        return Some((*usual).to_string());
    }
    let kind = match value.strip_prefix(STRICT_RELATIONSHIP_TYPES)? {
        "extendedProperties" => "extended-properties",
        "customProperties" => "custom-properties",
        kind => kind,
    };
    Some(format!("{RELATIONSHIP_TYPES}{kind}"))
}

/// `text` with the values in its start tags that name the Strict vocabulary written as in the transitional one, if
/// there are any; text content stays as it is. It only has to be right for XML that roxmltree reads.
fn with_usual_values(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = String::new();
    let mut copied = 0;
    let mut at = 0;
    while let Some(open) = bytes[at..].iter().position(|b| *b == b'<') {
        let tag_start = at + open + 1;
        at = tag_start;
        if matches!(bytes.get(at), Some(b'!' | b'?')) {
            at = xml::behind_markup(bytes, at);
            continue;
        }
        while let Some(&quote) = bytes.get(at).filter(|byte| **byte != b'>') {
            if quote != b'"' && quote != b'\'' {
                at += 1;
                continue;
            }
            let value_start = at + 1;
            let value_end = bytes[value_start..]
                .iter()
                .position(|b| *b == quote)
                .map_or(bytes.len(), |end| value_start + end);
            let name = text[tag_start..at]
                .trim_end()
                .strip_suffix('=')
                .and_then(|before| before.trim_end().rsplit(|c: char| c.is_ascii_whitespace()).next());
            if let Some(usual) = name.and_then(|name| usual_value(name, &text[value_start..value_end])) {
                out.push_str(&text[copied..value_start]);
                out.push_str(&usual);
                copied = value_end;
            }
            at = (value_end + 1).min(bytes.len());
        }
    }
    if copied == 0 {
        return None;
    }
    out.push_str(&text[copied..]);
    Some(out)
}

/// A part with what the Strict form names written as the transitional form does; the Strict values that stay, such as
/// `36pt` for a measure, Word 16 reads in a transitional file as well.
pub(crate) fn in_the_usual_form(name: &str, bytes: Vec<u8>) -> Vec<u8> {
    let is_xml = name.ends_with(".xml") || name.ends_with(".rels");
    if !is_xml || !bytes.windows(20).any(|window| window == b"purl.oclc.org/ooxml/") {
        return bytes;
    }
    match String::from_utf8(bytes) {
        Ok(text) => with_usual_values(&text).map_or_else(|| text.into_bytes(), String::into_bytes),
        Err(error) => error.into_bytes(),
    }
}

/// The text of a part, without the byte order mark it may start with.
fn into_text(bytes: Vec<u8>) -> Result<String, PackageError> {
    let mut text = String::from_utf8(bytes).map_err(|_| PackageError::Damaged)?;
    if text.starts_with('\u{feff}') {
        text.drain(..'\u{feff}'.len_utf8());
    }
    Ok(text)
}

/// Where the relationships of a part are.
pub(crate) fn rels_name(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((folder, file)) => format!("{folder}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// The folder a part stands in, which its relationships' targets start from.
pub(crate) fn folder_of(part: &str) -> &str {
    part.rsplit_once('/').map_or("", |(folder, _)| folder)
}

/// The part a relationship's target names, seen from `folder`. ECMA-376 Part 2 resolves a target against the name of
/// its source part by RFC 3986, whose removal of dot segments drops a `..` above the root (its section 5.4.2 has
/// `../../../g` as `/g`), so no name leads out of the package. `None` for the root itself, which is no part.
pub(crate) fn resolve(folder: &str, target: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    let rest = match target.strip_prefix('/') {
        Some(absolute) => absolute,
        None => {
            parts.extend(folder.split('/').filter(|part| !part.is_empty()));
            target
        }
    };
    for part in rest.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// A relationship of a part.
#[derive(Clone, Debug)]
pub(crate) struct Relationship {
    pub(crate) id: String,
    pub(crate) kind: String,
    /// As it stands in the file.
    pub(crate) target: String,
    pub(crate) external: bool,
    /// The part it leads to, unless it leads outside the package.
    pub(crate) part: Option<String>,
}

/// The relationships of a part, in the order of the file.
#[derive(Debug, Default)]
pub(crate) struct Relationships {
    list: Vec<Relationship>,
    /// The last of each id.
    by_id: HashMap<String, usize>,
    /// The first that leads to a part, by the last word of its type.
    by_type: HashMap<String, usize>,
}

impl Relationships {
    /// The relationships in the `.rels` text of a part in `folder`.
    pub(crate) fn read(text: &str, folder: &str) -> Result<Relationships, PackageError> {
        let document = xml::parse(text)?;
        let mut rels = Relationships::default();
        for node in document.root_element().children() {
            if node.tag_name().name() != "Relationship" {
                continue;
            }
            let (Some(id), Some(target)) = (node.attribute("Id"), node.attribute("Target")) else {
                continue;
            };
            let kind = node.attribute("Type").unwrap_or("");
            let external = node.attribute("TargetMode") == Some("External");
            let part = if external { None } else { resolve(folder, target) };
            let at = rels.list.len();
            if part.is_some()
                && let Some(word) = kind.rsplit('/').next()
            {
                rels.by_type.entry(word.to_string()).or_insert(at);
            }
            rels.by_id.insert(id.to_string(), at);
            rels.list.push(Relationship {
                id: id.to_string(),
                kind: kind.to_string(),
                target: target.to_string(),
                external,
                part,
            });
        }
        Ok(rels)
    }

    pub(crate) fn get(&self, id: &str) -> Option<&Relationship> {
        self.by_id.get(id).map(|at| &self.list[*at])
    }

    /// The first relationship to a part whose type ends in `/kind`.
    pub(crate) fn of_type(&self, kind: &str) -> Option<&Relationship> {
        self.by_type.get(kind).map(|at| &self.list[*at])
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &Relationship> {
        self.list.iter()
    }
}

/// A Word file opened: its archive and what `[Content_Types].xml` says.
pub(crate) struct Package<'a> {
    pub(crate) archive: Archive<'a>,
    /// `[Content_Types].xml` as it stands.
    pub(crate) types: String,
    pub(crate) main: String,
    /// Content types by lowercase extension.
    pub(crate) defaults: HashMap<String, String>,
    /// Content types by part.
    pub(crate) overrides: HashMap<String, String>,
}

impl<'a> Package<'a> {
    /// The package in `bytes`, whose main part is the first with one of `main_types`.
    pub(crate) fn open(bytes: &'a [u8], main_types: &[&str]) -> Result<Package<'a>, PackageError> {
        if bytes.starts_with(OLE_MAGIC) {
            return Err(ole_error(bytes));
        }
        let archive = Archive::read(bytes)?;
        let types = archive.get(TYPES_PART)?.ok_or(PackageError::NotAWordFile)?;
        let types = into_text(in_the_usual_form(TYPES_PART, types))?;
        let document = xml::parse(&types)?;
        let mut main = None;
        let mut defaults = HashMap::new();
        let mut overrides = HashMap::new();
        for node in document.root_element().children().filter(roxmltree::Node::is_element) {
            let kind = node.attribute("ContentType").unwrap_or("");
            match node.tag_name().name() {
                "Default" => {
                    let extension = node.attribute("Extension").unwrap_or("").to_ascii_lowercase();
                    defaults.insert(extension, kind.to_string());
                }
                "Override" => {
                    let name = node.attribute("PartName").unwrap_or("").trim_start_matches('/');
                    if main_types.contains(&kind) {
                        main.get_or_insert_with(|| name.to_string());
                    }
                    overrides.insert(name.to_string(), kind.to_string());
                }
                _ => {}
            }
        }
        drop(document);
        let main = main.ok_or(PackageError::NotAWordFile)?;
        if !archive.contains(&main) {
            return Err(PackageError::Damaged);
        }
        Ok(Package {
            archive,
            types,
            main,
            defaults,
            overrides,
        })
    }

    /// The content of the part `name` in the transitional form, if the package has such a part.
    pub(crate) fn part(&self, name: &str) -> Result<Option<Vec<u8>>, PackageError> {
        Ok(self.archive.get(name)?.map(|bytes| in_the_usual_form(name, bytes)))
    }

    /// The part `name` as XML text, which has to fit into what can be read whole.
    pub(crate) fn text(&self, name: &str) -> Result<Option<String>, PackageError> {
        if self.archive.size_of(name).is_some_and(|size| size > MOST_XML_BYTES) {
            return Err(PackageError::TooLarge);
        }
        self.part(name)?.map(into_text).transpose()
    }

    /// The relationships of the part `name`; a part without them has none.
    pub(crate) fn relationships(&self, name: &str) -> Result<Relationships, PackageError> {
        match self.text(&rels_name(name))? {
            Some(text) => Relationships::read(&text, folder_of(name)),
            None => Ok(Relationships::default()),
        }
    }
}

#[cfg(test)]
mod tests;
