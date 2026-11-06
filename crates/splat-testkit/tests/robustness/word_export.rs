//! Word documents of any text.

use splat_markdown::docx::{WordExport, stored_entries, to_docx};
use splat_markdown::reader::parse;
use splat_testkit::fuzz_checks::xml_problem;
use splat_testkit::{Rng, rounds};

use super::common::{document, limited};

/// Word refuses a document that is not well-formed XML, whatever stands in the text.
#[test]
fn word_documents_are_well_formed_xml() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_0d0c_0001);
    for _ in 0..rounds(300) {
        let text = document(&mut rng);
        let word = to_docx(
            &parse(&text),
            &WordExport {
                title: None,
                figure: &|_| None,
                image: &|_| None,
                quotes: None,
                reference: &|_| None,
                citation: &|_| None,
            },
        )
        .expect("a small document");
        let parts = stored_entries(&word);
        assert!(parts.iter().any(|(name, _)| name == "word/document.xml"), "{text:?}");
        for (name, part) in parts {
            if name.ends_with(".xml") || name.ends_with(".rels") {
                assert_eq!(xml_problem(&part), None, "{name} of {text:?}");
            }
        }
    }
}
