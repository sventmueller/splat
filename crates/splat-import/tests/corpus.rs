//! Files that Microsoft Word 16.114 and macOS `textutil` wrote of one synthetic document, each beside the Markdown it
//! imports to. `SPLAT_CORPUS_WRITE=1 cargo test -p splat-import --test corpus` writes what the import gives instead.

use std::path::{Path, PathBuf};

use splat_import::docx::import;
use splat_import::template::apply_template;
use splat_import::{ImageSource, Pasted, paste_as_markdown};
use splat_markdown::docx::{Picture, WordExport, to_docx};
use splat_markdown::reader::parse;

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus")
}

fn expectation_of(input: &Path) -> PathBuf {
    let mut name = input.file_name().expect("a file").to_os_string();
    name.push(".md");
    input.with_file_name(name)
}

/// What the pasteboard path makes of a file's text, with each picture it carries named by its place in the text.
fn pasted(pasted: &Pasted) -> String {
    let mut count = 0;
    let converted = paste_as_markdown(pasted, &mut |source| match source {
        ImageSource::Address(address) => Some((*address).to_string()),
        ImageSource::Data { mime, .. } => {
            count += 1;
            Some(format!("pasted/{count}.{}", mime.rsplit('/').next().unwrap_or("bin")))
        }
    });
    converted.map(|converted| converted.markdown + "\n").unwrap_or_default()
}

fn imported(input: &Path) -> String {
    let bytes = std::fs::read(input).expect("a corpus file");
    match input.extension().and_then(|extension| extension.to_str()) {
        Some("docx" | "dotx") => {
            let imported = import(&bytes, "images").unwrap_or_else(|error| panic!("{}: {error}", input.display()));
            assert!(
                imported.missing.is_empty(),
                "{}: {:?}",
                input.display(),
                imported.missing
            );
            imported.markdown
        }
        Some("rtf") => pasted(&Pasted {
            rtf: Some(&bytes),
            ..Pasted::default()
        }),
        Some("html") => pasted(&Pasted {
            html: Some(&String::from_utf8_lossy(&bytes)),
            ..Pasted::default()
        }),
        _ => panic!("{} is of no kind the corpus knows", input.display()),
    }
}

#[test]
fn real_files_import_as_expected() {
    let write = std::env::var_os("SPLAT_CORPUS_WRITE").is_some();
    let mut inputs: Vec<PathBuf> = std::fs::read_dir(corpus())
        .expect("the corpus")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension != "md"))
        .collect();
    inputs.sort();
    assert!(inputs.len() >= 8, "{inputs:?}");
    let mut differ = Vec::new();
    for input in &inputs {
        let markdown = imported(input);
        let expectation = expectation_of(input);
        if write {
            std::fs::write(&expectation, &markdown).expect("a writable corpus");
        } else if std::fs::read_to_string(&expectation).ok().as_deref() != Some(markdown.as_str()) {
            differ.push(format!("{}:\n{markdown}", input.display()));
        }
    }
    assert!(differ.is_empty(), "{}", differ.join("\n"));
}

#[test]
fn every_expectation_has_its_file() {
    for entry in std::fs::read_dir(corpus()).expect("the corpus") {
        let path = entry.expect("an entry").path();
        if path.extension().is_some_and(|extension| extension == "md") {
            assert!(path.with_extension("").exists(), "{} has no input", path.display());
        }
    }
}

#[test]
fn what_word_writes_serves_as_a_template_in_either_form() {
    let generated = to_docx(
        &parse("# Titel\n\nText mit **fett** und [Link](https://a.bc).\n\n- eins\n- zwei\n\n![Bild](a.png)\n\nNotiz[^1]\n\n[^1]: Fuß\n"),
        &WordExport {
            title: None,
            figure: &|_| None,
            image: &|_| {
                Some(Picture {
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
    .expect("a small document");
    let text = import(&generated, "images").expect("what Splat writes");
    for name in ["word.docx", "word-strict.docx", "word-template.dotx"] {
        let template = std::fs::read(corpus().join(name)).expect("a corpus file");
        let merged = apply_template(&generated, &template).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(import(&merged, "images").expect("a merged file"), text, "{name}");
    }
}
