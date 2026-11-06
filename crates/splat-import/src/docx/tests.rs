//! Word files built by hand, as Word and others write them, and what they become.

use splat_markdown::docx::{Picture, WordExport, to_docx};
use splat_markdown::reader::parse as read_markdown;
use splat_testkit::{Rng, rounds};

use super::package::{A, M, MC, R, V, W, WP};
use super::*;
use splat_ooxml::zip::testing::{Part, archive, deflated, stored};

mod fields;
mod files;
mod limits;
mod lists;
mod notes;
mod paragraphs;
mod pictures;
mod random;
mod round_trip;
mod runs;
mod tables;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest";
const JPEG: &[u8] = b"\xFF\xD8\xFF\xE0rest";

/// The parts of a Word file for a test.
#[derive(Default)]
struct Fixture {
    styles: Option<String>,
    numbering: Option<String>,
    footnotes: Option<String>,
    endnotes: Option<String>,
    /// Further relationships of the main part: id, target, external.
    rels: Vec<(&'static str, &'static str, bool)>,
    /// Further entries of the archive.
    parts: Vec<(&'static str, Vec<u8>)>,
}

fn wrapped(tag: &str, inner: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?><w:{tag} xmlns:w=\"{W}\" xmlns:r=\"{R}\" xmlns:m=\"{M}\" xmlns:wp=\"{WP}\" \
         xmlns:a=\"{A}\" xmlns:v=\"{V}\" xmlns:mc=\"{MC}\">{inner}</w:{tag}>"
    )
}

impl Fixture {
    fn file(&self, body: &str) -> Vec<u8> {
        let mut overrides = String::from(
            "<Override PartName=\"/word/document.xml\" \
             ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>",
        );
        overrides.push_str("<Default Extension=\"xml\" ContentType=\"application/xml\"/>");
        let mut rels = String::new();
        let mut parts = vec![stored(
            "word/document.xml",
            wrapped("document", &format!("<w:body>{body}</w:body>")).as_bytes(),
        )];
        for (id, kind, content) in [
            ("rIdS", "styles", &self.styles),
            ("rIdN", "numbering", &self.numbering),
            ("rIdF", "footnotes", &self.footnotes),
            ("rIdE", "endnotes", &self.endnotes),
        ] {
            if let Some(content) = content {
                rels.push_str(&format!(
                    "<Relationship Id=\"{id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}\" Target=\"{kind}.xml\"/>"
                ));
                parts.push(deflated(&format!("word/{kind}.xml"), wrapped(kind, content).as_bytes()));
            }
        }
        for (id, target, external) in &self.rels {
            let mode = if *external { " TargetMode=\"External\"" } else { "" };
            rels.push_str(&format!(
                "<Relationship Id=\"{id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/x\" Target=\"{target}\"{mode}/>"
            ));
        }
        parts.push(stored(
            "word/_rels/document.xml.rels",
            format!("<Relationships>{rels}<Unknown/><Relationship Id=\"incomplete\"/></Relationships>").as_bytes(),
        ));
        parts.push(stored(
            "[Content_Types].xml",
            format!("<Types>{overrides}</Types>").as_bytes(),
        ));
        for (name, content) in &self.parts {
            parts.push(deflated(name, content));
        }
        archive(&parts)
    }

    fn markdown(&self, body: &str) -> String {
        import(&self.file(body), "images").expect("a word file").markdown
    }
}

fn markdown(body: &str) -> String {
    Fixture::default().markdown(body)
}

fn p(inner: &str) -> String {
    format!("<w:p>{inner}</w:p>")
}

fn t(text: &str) -> String {
    format!("<w:r><w:t xml:space=\"preserve\">{text}</w:t></w:r>")
}

fn marked(props: &str, text: &str) -> String {
    format!("<w:r><w:rPr>{props}</w:rPr><w:t xml:space=\"preserve\">{text}</w:t></w:r>")
}

fn styled(style: &str, inner: &str) -> String {
    format!("<w:p><w:pPr><w:pStyle w:val=\"{style}\"/></w:pPr>{inner}</w:p>")
}

fn style(id: &str, name: &str, extra: &str) -> String {
    format!("<w:style w:type=\"paragraph\" w:styleId=\"{id}\"><w:name w:val=\"{name}\"/>{extra}</w:style>")
}

fn with_styles(styles: &str) -> Fixture {
    Fixture {
        styles: Some(styles.to_string()),
        ..Fixture::default()
    }
}

/// The Markdown that Splat's Word export of `source` imports to.
fn round_trip(source: &str) -> String {
    let word = to_docx(
        &read_markdown(source),
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
    .expect("a small document");
    import(&word, "media").expect("what Splat writes").markdown
}
