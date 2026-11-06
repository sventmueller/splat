//! Templates built by hand, as Word and others write them, and what comes out must still be a Word file.

use std::collections::{HashMap, HashSet};

use roxmltree::Document;
use splat_markdown::docx::{Picture, WordExport, to_docx};
use splat_markdown::reader::parse;
use splat_testkit::{Rng, rounds};

use super::serialize::{RELATIONSHIPS, WORD};
use super::*;
use crate::docx::import;
use crate::package::{DOCUMENT_TYPE, resolve};
use crate::zip::tests::{Part as Entry, archive, deflated, stored};
use crate::zip::{Archive, MOST_ENTRY_BYTES};

mod errors;
mod merge;
mod notes;
mod random;
mod relationships;
mod styles;

const W: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"";
const R: &str = "xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"";
const RELS: &str = "xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"";
const TYPES: &str = "xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"";
const REL_BASE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const OFFICE: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml";
const TEMPLATE_MAIN: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.template.main+xml";

const SAMPLE: &str = "# Titel\n\nText mit **fett**, `code` und [Link](https://example.com/a?b=1&c=2).\n\n![Bild](a.png)\n\n\
                      - eins\n- zwei\n\n3. drei\n4. vier\n\n> Zitat\n\n```\ncode\n```\n\n| A | B |\n|---|--:|\n| 1 | 2 |\n";

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n data";

fn export(markdown: &str) -> Vec<u8> {
    to_docx(
        &parse(markdown),
        &WordExport {
            title: None,
            figure: &|_| None,
            image: &|_| {
                Some(Picture {
                    data: PNG.to_vec(),
                    jpeg: false,
                    width: 10.0,
                    height: 10.0,
                })
            },
            quotes: None,
            reference: &|_| None,
            citation: &|_| None,
        },
    )
    .expect("a small document")
}

const FINAL_SECTION: &str = "<w:sectPr w:rsidR=\"00A1\"><w:headerReference w:type=\"default\" r:id=\"rId5\"/>\
    <w:footerReference w:type=\"default\" r:id=\"rId6\"/><w:pgSz w:w=\"12240\" w:h=\"15840\"/></w:sectPr>";

const ROOT_START: &str = "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
    xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
    xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" \
    xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\" mc:Ignorable=\"w14\">";

fn template_document() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n{ROOT_START}<w:body>\
         <w:p><w:pPr><w:pStyle w:val=\"berschrift1\"/></w:pPr><w:r><w:t>Vorlage</w:t></w:r></w:p>\
         <w:p><w:pPr><w:sectPr><w:pgSz w:w=\"1\" w:h=\"2\"/></w:sectPr></w:pPr></w:p>\
         {FINAL_SECTION}</w:body></w:document>"
    )
}

fn template_types(main: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?><Types {TYPES}><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
         <Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"PNG\" ContentType=\"image/png\"/>\
         <Override PartName=\"/word/document.xml\" ContentType=\"{main}\"/>\
         <Override PartName=\"/word/styles.xml\" ContentType=\"{OFFICE}.styles+xml\"/>\
         <Override PartName=\"/word/numbering.xml\" ContentType=\"{OFFICE}.numbering+xml\"/>\
         <Override PartName=\"/word/settings.xml\" ContentType=\"{OFFICE}.settings+xml\"/>\
         <Override PartName=\"/word/header1.xml\" ContentType=\"{OFFICE}.header+xml\"/>\
         <Override PartName=\"/word/footer1.xml\" ContentType=\"{OFFICE}.footer+xml\"/>\
         <Override PartName=\"/word/footnotes.xml\" ContentType=\"{OFFICE}.footnotes+xml\"/>\
         <Override PartName=\"/word/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>\
         </Types>"
    )
}

fn template_rels() -> String {
    format!(
        "<?xml version=\"1.0\"?><Relationships {RELS}>\
         <Relationship Id=\"rId1\" Type=\"{REL_BASE}/styles\" Target=\"styles.xml\"/>\
         <Relationship Id=\"rId2\" Type=\"{REL_BASE}/numbering\" Target=\"numbering.xml\"/>\
         <Relationship Id=\"rId3\" Type=\"{REL_BASE}/settings\" Target=\"settings.xml\"/>\
         <Relationship Id=\"rId4\" Type=\"{REL_BASE}/theme\" Target=\"theme/theme1.xml\"/>\
         <Relationship Id=\"rId5\" Type=\"{REL_BASE}/header\" Target=\"header1.xml\"/>\
         <Relationship Id=\"rId6\" Type=\"{REL_BASE}/footer\" Target=\"footer1.xml\"/>\
         <Relationship Id=\"rId7\" Type=\"{REL_BASE}/footnotes\" Target=\"footnotes.xml\"/>\
         <Relationship Id=\"rId9\" Type=\"{REL_BASE}/image\" Target=\"media/image1.png\"/>\
         <Relationship Id=\"anker\" Type=\"{REL_BASE}/hyperlink\" Target=\"https://example.org\" TargetMode=\"External\"/>\
         </Relationships>"
    )
}

fn template_styles() -> String {
    format!(
        "<?xml version=\"1.0\"?><w:styles {W}><w:docDefaults/>\
         <w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Standard\"><w:name w:val=\"Normal\"/></w:style>\
         <w:style w:type=\"paragraph\" w:styleId=\"berschrift1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Standard\"/></w:style>\
         <w:style w:type=\"paragraph\" w:styleId=\"Code\"><w:name w:val=\"Quellcode\"/></w:style>\
         <w:style w:type=\"character\" w:styleId=\"Link\"><w:name w:val=\"Hyperlink\"/></w:style>\
         </w:styles>"
    )
}

fn template_numbering() -> String {
    format!(
        "<?xml version=\"1.0\"?><w:numbering {W}><w:abstractNum w:abstractNumId=\"4\"><w:lvl w:ilvl=\"0\"/></w:abstractNum>\
         <w:num w:numId=\"7\"><w:abstractNumId w:val=\"4\"/></w:num></w:numbering>"
    )
}

fn template_footnotes() -> String {
    format!(
        "<?xml version=\"1.0\"?><w:footnotes {W}><w:footnote w:type=\"separator\" w:id=\"-1\"><w:p/></w:footnote>\
         <w:footnote w:type=\"continuationSeparator\" w:id=\"0\"><w:p/></w:footnote></w:footnotes>"
    )
}

fn template_parts(main: &str) -> Vec<Entry> {
    vec![
        stored("[Content_Types].xml", template_types(main).as_bytes()),
        stored("_rels/.rels", format!("<Relationships {RELS}/>").as_bytes()),
        deflated("word/document.xml", template_document().as_bytes()),
        stored("word/_rels/document.xml.rels", template_rels().as_bytes()),
        deflated("word/styles.xml", template_styles().as_bytes()),
        stored("word/numbering.xml", template_numbering().as_bytes()),
        stored("word/settings.xml", format!("<w:settings {W}/>").as_bytes()),
        stored("word/theme/theme1.xml", b"<a:theme xmlns:a=\"urn:a\"/>"),
        stored(
            "word/header1.xml",
            format!("<w:hdr {W}><w:p><w:r><w:t>Kopf</w:t></w:r></w:p></w:hdr>").as_bytes(),
        ),
        stored("word/footer1.xml", format!("<w:ftr {W}/>").as_bytes()),
        stored("word/footnotes.xml", template_footnotes().as_bytes()),
        stored("word/media/image1.png", b"template picture"),
        stored("word/", b""),
        stored("customXml/item1.xml", b"<x/>"),
    ]
}

fn full_template() -> Vec<u8> {
    archive(&template_parts(DOCUMENT_TYPE))
}

fn entries(file: &[u8]) -> HashMap<String, Vec<u8>> {
    let read = Archive::read(file).expect("an archive");
    read.names()
        .map(|name| (name.to_string(), read.get(name).unwrap().unwrap()))
        .collect()
}

fn text_of_part(file: &[u8], name: &str) -> String {
    let part = entries(file).remove(name).unwrap_or_else(|| panic!("no part {name}"));
    String::from_utf8(part).unwrap()
}

/// All that must hold for a Word file to open.
fn assert_valid(file: &[u8]) {
    let parts = entries(file);
    let mut names: Vec<&String> = parts.keys().collect();
    names.sort();
    for name in &names {
        if name.ends_with(".xml") || name.ends_with(".rels") {
            let text = std::str::from_utf8(&parts[*name]).unwrap();
            Document::parse(text).unwrap_or_else(|e| panic!("{name}: {e}\n{text}"));
        }
    }
    let types_text = std::str::from_utf8(&parts["[Content_Types].xml"]).unwrap();
    let types = Document::parse(types_text).unwrap();
    let mut defaults = HashSet::new();
    let mut overrides = HashMap::new();
    for node in types.root_element().children().filter(|n| n.is_element()) {
        match node.tag_name().name() {
            "Default" => {
                assert!(
                    defaults.insert(node.attribute("Extension").unwrap().to_lowercase()),
                    "a default twice: {types_text}"
                );
            }
            _ => {
                let part = node.attribute("PartName").unwrap().trim_start_matches('/').to_string();
                assert!(parts.contains_key(&part), "{part} has a type but is not there");
                overrides.insert(part, node.attribute("ContentType").unwrap().to_string());
            }
        }
    }
    for name in &names {
        let extension = name.rsplit('.').next().unwrap_or("").to_lowercase();
        assert!(
            *name == "[Content_Types].xml" || overrides.contains_key(*name) || defaults.contains(&extension),
            "{name} has no content type: {types_text}"
        );
    }
    assert!(!overrides.values().any(|kind| kind == TEMPLATE_MAIN));
    let main = overrides
        .iter()
        .find(|(_, kind)| *kind == DOCUMENT_TYPE)
        .map(|(part, _)| part.as_str())
        .expect("a main part");
    let mut rel_ids = HashMap::new();
    for name in names.iter().filter(|name| name.ends_with(".rels")) {
        let text = std::str::from_utf8(&parts[*name]).unwrap();
        let doc = Document::parse(text).unwrap();
        let owner = name.replace("_rels/", "");
        let owner = owner.strip_suffix(".rels").unwrap();
        let folder = owner.rsplit_once('/').map_or("", |(folder, _)| folder);
        let mut ids = HashSet::new();
        for rel in doc.root_element().children().filter(|n| n.is_element()) {
            assert!(
                ids.insert(rel.attribute("Id").unwrap().to_string()),
                "an id twice in {name}"
            );
            if rel.attribute("TargetMode") != Some("External") {
                let part = resolve(folder, rel.attribute("Target").unwrap()).unwrap();
                assert!(parts.contains_key(&part), "{name} points to {part}, which is not there");
            }
        }
        rel_ids.insert(owner.to_string(), ids);
    }
    let document_text = std::str::from_utf8(&parts[main]).unwrap();
    let document = Document::parse(document_text).unwrap();
    assert_refers_to_what_exists(&document, rel_ids.get(main), &parts);
}

fn assert_refers_to_what_exists(document: &Document, rels: Option<&HashSet<String>>, parts: &HashMap<String, Vec<u8>>) {
    let values = |part: &str, element: &str, attribute: &str| -> HashSet<String> {
        parts.get(part).map_or_else(HashSet::new, |content| {
            let text = std::str::from_utf8(content).unwrap();
            Document::parse(text)
                .unwrap()
                .descendants()
                .filter(|n| n.tag_name().name() == element)
                .filter_map(|n| n.attribute((WORD, attribute)).map(ToString::to_string))
                .collect()
        })
    };
    let styles = values("word/styles.xml", "style", "styleId");
    let numbers = values("word/numbering.xml", "num", "numId");
    for node in document.descendants().filter(|n| n.is_element()) {
        for attribute in node.attributes().filter(|a| a.namespace() == Some(RELATIONSHIPS)) {
            assert!(
                rels.is_some_and(|rels| rels.contains(attribute.value())),
                "{} does not resolve",
                attribute.value()
            );
        }
        let value = node.attribute((WORD, "val")).unwrap_or("");
        match node.tag_name().name() {
            "pStyle" | "rStyle" | "tblStyle" => assert!(styles.contains(value), "style {value} is not defined"),
            "numId" => assert!(value == "0" || numbers.contains(value), "list {value} is not defined"),
            _ => {}
        }
    }
}

fn assert_same_text_as(result: &[u8], generated: &[u8]) {
    assert_eq!(
        import(result, "images").expect("the result"),
        import(generated, "images").expect("the generated file")
    );
}

fn set_part(parts: &mut [Entry], name: &str, content: &str) {
    let part = parts
        .iter_mut()
        .find(|part| part.name == name)
        .expect("a part of this name");
    *part = stored(name, content.as_bytes());
}

fn without_part(parts: &mut Vec<Entry>, name: &str) {
    parts.retain(|part| part.name != name);
}

/// `file` with something replaced in one of its parts, as if another program had written it.
fn replaced(file: &[u8], part: &str, what: &str, with: &str) -> Vec<u8> {
    let read = Archive::read(file).unwrap();
    let parts: Vec<Entry> = read
        .names()
        .map(|name| {
            let content = read.get(name).unwrap().unwrap();
            if name == part {
                let text = String::from_utf8(content).unwrap();
                assert!(text.contains(what), "{what} in {part}");
                stored(name, text.replacen(what, with, 1).as_bytes())
            } else {
                stored(name, &content)
            }
        })
        .collect();
    archive(&parts)
}

fn with_part(file: &[u8], name: &str, content: &[u8]) -> Vec<u8> {
    let read = Archive::read(file).unwrap();
    let mut parts: Vec<Entry> = read
        .names()
        .map(|name| stored(name, &read.get(name).unwrap().unwrap()))
        .collect();
    parts.push(stored(name, content));
    archive(&parts)
}

/// The generated file with a footnote part of its own, as `to_docx` may one day write it, and a reference to it.
fn with_notes(reference: &str, part: &str, element: &str) -> Vec<u8> {
    let mut file = replaced(
        &export("Text\n"),
        "word/document.xml",
        "<w:r>",
        &format!("<w:r><w:{reference} w:id=\"2\"/></w:r><w:r>"),
    );
    file = replaced(
        &file,
        "word/_rels/document.xml.rels",
        "</Relationships>",
        &format!("<Relationship Id=\"rIdN\" Type=\"{REL_BASE}/{part}\" Target=\"{part}.xml\"/></Relationships>"),
    );
    file = replaced(
        &file,
        "[Content_Types].xml",
        "</Types>",
        &format!("<Override PartName=\"/word/{part}.xml\" ContentType=\"{OFFICE}.{part}+xml\"/></Types>"),
    );
    file = with_part(
        &file,
        &format!("word/{part}.xml"),
        format!(
            "<?xml version=\"1.0\"?><w:{part} {W} {R}><w:{element} w:type=\"separator\" w:id=\"0\"><w:p/></w:{element}>\
             <w:{element} w:id=\"2\"><w:p><w:pPr><w:pStyle w:val=\"FootnoteText\"/></w:pPr><w:hyperlink r:id=\"rId1\"><w:r><w:t>Notiz</w:t></w:r></w:hyperlink>\
             <w:r><w:drawing r:id=\"rId2\"/></w:r></w:p></w:{element}></w:{part}>"
        )
        .as_bytes(),
    );
    file = with_part(
        &file,
        &format!("word/_rels/{part}.xml.rels"),
        format!(
            "<Relationships {RELS}><Relationship Id=\"rId1\" Type=\"{REL_BASE}/hyperlink\" Target=\"https://a.bc\" TargetMode=\"External\"/>\
             <Relationship Id=\"rId2\" Type=\"{REL_BASE}/image\" Target=\"media/note.png\"/></Relationships>"
        )
        .as_bytes(),
    );
    with_part(&file, "word/media/note.png", PNG)
}
