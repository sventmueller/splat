//! What the target `import` in `fuzz/` asks of its input: every importer takes any bytes without a panic and what it writes
//! reads as Markdown with no link Splat would not follow. It lives here for the same reason as `fuzz_checks`.

use splat_import::docx::import;
use splat_import::html::from_html;
use splat_import::rich::Rich;
use splat_import::rtf::from_rtf;
use splat_import::tabular::from_text;
use splat_import::template::apply_template;
use splat_import::{ImageSource, Pasted, WordError, paste_as_markdown};
use splat_markdown::doc::{InlineKind, walk_blocks, walk_inlines};
use splat_markdown::docx::{WordExport, to_docx};
use splat_markdown::reader::parse;
use splat_markdown::url::is_harmless;
use splat_ooxml::zip::{Archive, Zip};

use crate::fuzz_checks::xml_problem;

/// Each input in as many roles as it can have.
pub fn check_import(data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    check_markdown(from_html(&text));
    check_markdown(from_rtf(data));
    if let Some(table) = from_text(&text) {
        check_markdown(table);
    }
    check_paste(&text, data);
    check_word_file(data);
    check_word_file(&word_file(data, Part::Document));
    check_word_file(&word_file(data, Part::Styles));
    check_word_file(&word_file(data, Part::Numbering));
    check_word_file(&word_file(data, Part::Footnotes));
    check_template(data);
}

/// Where in a Word file an input goes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Document,
    Styles,
    Numbering,
    Footnotes,
}

const MAIN_TYPE: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const NAMESPACES: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
    xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"";

/// A valid archive of a Word file whose part `part` is `content`, and the others are small.
pub fn word_file(content: &[u8], part: Part) -> Vec<u8> {
    let rel_type = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let types = format!(
        "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
         <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
         <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
         <Override PartName=\"/word/document.xml\" ContentType=\"{MAIN_TYPE}\"/></Types>"
    );
    let rels = format!(
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         <Relationship Id=\"rId1\" Type=\"{rel_type}/styles\" Target=\"styles.xml\"/>\
         <Relationship Id=\"rId2\" Type=\"{rel_type}/numbering\" Target=\"numbering.xml\"/>\
         <Relationship Id=\"rId3\" Type=\"{rel_type}/footnotes\" Target=\"footnotes.xml\"/></Relationships>"
    );
    let document = format!(
        "<w:document {NAMESPACES}><w:body><w:p><w:pPr><w:pStyle w:val=\"a\"/><w:numPr><w:ilvl w:val=\"0\"/>\
         <w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>x</w:t><w:footnoteReference w:id=\"1\"/></w:r></w:p>\
         </w:body></w:document>"
    );
    let empty = |root: &str| format!("<w:{root} {NAMESPACES}/>");
    let pick = |wanted: Part, other: String| {
        if part == wanted {
            content.to_vec()
        } else {
            other.into_bytes()
        }
    };
    let mut zip = Zip::default();
    let entries: [(&str, Vec<u8>); 6] = [
        ("[Content_Types].xml", types.into_bytes()),
        ("word/_rels/document.xml.rels", rels.into_bytes()),
        ("word/document.xml", pick(Part::Document, document)),
        ("word/styles.xml", pick(Part::Styles, empty("styles"))),
        ("word/numbering.xml", pick(Part::Numbering, empty("numbering"))),
        ("word/footnotes.xml", pick(Part::Footnotes, empty("footnotes"))),
    ];
    for (name, bytes) in entries {
        zip.add(name, &bytes).expect("a small file");
    }
    zip.finish()
}

fn check_markdown(rich: Rich) {
    check_written(&rich.into_markdown());
}

/// `markdown` reads, and all it links to is what Splat would follow.
fn check_written(markdown: &str) {
    let doc = parse(markdown);
    walk_blocks(doc.blocks(), &mut |block| {
        walk_inlines(block.inlines(), &mut |inline| {
            if let InlineKind::Link { url, .. } | InlineKind::Image { url, .. } = inline.kind() {
                assert!(is_harmless(url), "{url:?} in {markdown:?}");
            }
        });
    });
}

fn check_paste(text: &str, data: &[u8]) {
    let mut place = |source: &ImageSource| match source {
        ImageSource::Address(address) => address.starts_with('h').then(|| "javascript:x".to_string()),
        ImageSource::Data { bytes, .. } => (!bytes.is_empty()).then(|| "pictures/a b.png".to_string()),
    };
    for pasted in [
        Pasted {
            html: Some(text),
            rtf: None,
            plain: None,
        },
        Pasted {
            html: None,
            rtf: Some(data),
            plain: None,
        },
        Pasted {
            html: None,
            rtf: None,
            plain: Some(text),
        },
    ] {
        if let Some(converted) = paste_as_markdown(&pasted, &mut place)
            && pasted.plain.is_none()
        {
            check_written(&converted.markdown);
        }
    }
}

fn check_word_file(file: &[u8]) {
    let Ok(imported) = import(file, "pic tures") else {
        return;
    };
    check_written(&imported.markdown);
    for media in imported.media {
        assert!(
            media.file_name.starts_with("image")
                && media
                    .file_name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.'),
            "{}",
            media.file_name
        );
    }
}

/// What `to_docx` writes for a little of everything.
pub fn generated() -> Vec<u8> {
    let doc = parse(
        "# Titel\n\nText **fett** [Link](https://a.bc) ![Bild](a.png)\n\n- a\n  - b\n\n3. c\n\n| A | B |\n|---|--:|\n| 1 | 2 |\n\n\
         > Zitat\n\n```\ncode\n```\n",
    );
    to_docx(
        &doc,
        &WordExport {
            title: None,
            figure: &|_| None,
            image: &|_| {
                Some(splat_markdown::docx::Picture {
                    data: b"\x89PNG\r\n\x1a\n".to_vec(),
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

fn check_template(data: &[u8]) {
    let generated = generated();
    let mut results = vec![apply_template(data, data), apply_template(&generated, data)];
    for part in [Part::Document, Part::Styles, Part::Numbering, Part::Footnotes] {
        let file = word_file(data, part);
        results.push(apply_template(&generated, &file));
        results.push(apply_template(&file, &generated));
    }
    for result in results.into_iter().flatten() {
        check_merged(&result);
    }
}

/// A merged file is an archive whose parts are XML, and it reads again.
fn check_merged(merged: &[u8]) {
    let archive = Archive::read(merged).expect("what a merge wrote");
    let names: Vec<String> = archive.names().map(str::to_string).collect();
    for name in names {
        if name.ends_with(".xml") || name.ends_with(".rels") {
            let part = archive
                .get(&name)
                .expect("what a merge wrote")
                .expect("a part it names");
            assert_eq!(xml_problem(&part), None, "{name}");
        }
    }
    assert_ne!(import(merged, "p").err(), Some(WordError::NotAWordFile));
    check_word_file(merged);
}
