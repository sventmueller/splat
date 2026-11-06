//! Files and pasteboards built to hurt the importers: nesting that overflows a stack, patterns that take quadratic time or
//! memory, archives that inflate to far more than they hold. Each runs at two sizes on a small stack, whose time and memory
//! must grow about as the input does; a thread named after the input tells which one ended the test run.

use std::fmt::Write;
use std::sync::Arc;
use std::time::Duration;

use miniz_oxide::deflate::compress_to_vec;
use splat_import::docx::import;
use splat_import::html::from_html;
use splat_import::rtf::from_rtf;
use splat_import::tabular::from_text;
use splat_import::template::apply_template;
use splat_ooxml::zip::{Zip, crc32};
use splat_testkit::fuzz_import::{Part, generated, word_file};
use splat_testkit::memory::{self, peak_of};
use splat_testkit::timing::{Input, LIMIT, NOTICEABLE, Scaled, Stages, alone, grows_too_fast, holds_to_time, scales};

#[global_allocator]
static COUNTING: memory::Counting = memory::Counting;

/// The pieces of each input at full size; an unoptimized build takes ten times as long for the same work.
const SIZE: usize = if cfg!(debug_assertions) { 10_000 } else { 80_000 };

const W: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
    xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
    xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\" \
    xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"";

fn repeated(piece: &str, count: usize) -> Vec<u8> {
    piece.repeat(count).into_bytes()
}

fn html(name: &'static str, piece: &str, count: usize) -> Case {
    Case {
        name,
        input: repeated(piece, count),
        run: |input| {
            let text = String::from_utf8_lossy(input);
            from_html(&text).into_markdown().len()
        },
    }
}

fn rtf(name: &'static str, piece: &str, count: usize) -> Case {
    let mut input = b"{\\rtf1 ".to_vec();
    input.extend(repeated(piece, count));
    Case {
        name,
        input,
        run: |input| from_rtf(input).into_markdown().len(),
    }
}

fn tabular(name: &'static str, piece: &str, count: usize) -> Case {
    Case {
        name,
        input: repeated(piece, count),
        run: |input| {
            let text = String::from_utf8_lossy(input);
            from_text(&text).map_or(0, |rich| rich.into_markdown().len())
        },
    }
}

fn run_import(input: &[u8]) -> usize {
    import(input, "pictures").map_or(0, |imported| imported.markdown.len())
}

/// A Word file with `xml` as its part `part`.
fn word(name: &'static str, part: Part, xml: String) -> Case {
    Case {
        name,
        input: word_file(xml.as_bytes(), part),
        run: run_import,
    }
}

fn body(name: &'static str, content: String) -> Case {
    word(
        name,
        Part::Document,
        format!("<w:document {W}><w:body>{content}</w:body></w:document>"),
    )
}

/// An input and the importer it is meant for.
struct Case {
    name: &'static str,
    input: Vec<u8>,
    run: fn(&[u8]) -> usize,
}

/// A Word file or template grows by what its parts hold, which deflating hides.
impl Input for Case {
    fn size(&self) -> usize {
        let unpacked = splat_ooxml::zip::Archive::read(&self.input).map_or(0, |archive| {
            let names: Vec<String> = archive.names().map(str::to_string).collect();
            names
                .iter()
                .map(|name| archive.get(name).ok().flatten().map_or(0, |part| part.len()))
                .sum()
        });
        unpacked.max(self.input.len())
    }
}

fn imported(case: &Case) -> Stages {
    let mut stages = Stages::default();
    stages.time("reading", || std::hint::black_box((case.run)(&case.input)));
    stages
}

fn scaled(small: Case, large: Case) -> Scaled<Case> {
    Scaled {
        name: small.name,
        small: Arc::new(small),
        large: Arc::new(large),
        work: imported,
    }
}

fn html_cases(n: usize) -> Vec<Case> {
    vec![
        html("nested divs", "<div>", n),
        html("nested quotes", "<blockquote>", n),
        html("nested lists", "<ul><li>", n),
        html("nested tables", "<table><tr><td>", n),
        html("nested marks", "<b><i><u><s><mark><sup><sub>", n),
        html("closed marks", "<b>a</b><i>b</i>", n),
        html("tags that never end", "<a ", n),
        html("quotes that never end", "<a href=\"", n),
        html("comments that never end", "<!-- x", n),
        html("conditions", "<![if !supportLists]>x<![endif]>", n),
        html("styles that never end", "<style>", n),
        html("closing tags", "</div></b></li></td>", n),
        html("items", "<li>", n),
        html("paragraphs", "<p>a</p>", n),
        html("cells", "<td>", n),
        html("rows", "<tr><td>a", n),
        html(
            "row spans",
            "<table><tr><td rowspan=999999 colspan=64>a<td rowspan=999999>b</table>",
            n,
        ),
        html(
            "row spans in one table",
            &format!("<table>{}", "<tr><td rowspan=999999 colspan=64>a<td>b".repeat(n)),
            1,
        ),
        html(
            "one wide row and many short ones",
            &format!("<table><tr>{}{}</table>", "<td>a".repeat(n), "<tr><td>b".repeat(n)),
            1,
        ),
        html("word lists", "<p style='mso-list:l0 level2 lfo1'>a</p>", n),
        html("word list markers", "<!--[if !supportLists]-->1.", n),
        html("bad pictures", "<img src=\"data:image/png;base64,AAAA\" alt=x>", n),
        html("pictures", "<img src=\"a.png\" alt=\"b\">", n),
        html("spaces", "   \n\t", n),
        html("no-break spaces", "&nbsp;", n),
        html("breaks", "<br>", n),
        html("code", "<code>a</code> ", n),
        html("pre", "<pre>a</pre>", n),
        html("ampersands", "&", n),
        html("stars", "*", n),
        html("words and stars", "a*b_c ", n),
        html("brackets", "[", n),
        html("closing brackets", "](", n),
        html("dollars", "$a ", n),
        html("pipes", "|", n),
        html("addresses", "http://", n),
        html("addresses with marks", "http://a.bc/* ", n),
        html("addresses without spaces", "http://*", n),
        html("colons", ":smile:", n),
        html("headings with braces", "<h1>{{{{{{}</h1>", n),
        html(
            "one link over many runs",
            &format!("<a href=\"{}\">{}</a>", "x".repeat(n), "<b>a</b><i>b</i>".repeat(n)),
            1,
        ),
        html("marks that change", "<b>a<i>b</b>c</i>", n),
        html("emphasis that cannot open", "a<b>\"b\"</b>c", n),
    ]
}

fn rtf_cases(n: usize) -> Vec<Case> {
    vec![
        rtf("open groups", "{", n),
        rtf("closed groups", "}", n),
        rtf("groups", "{\\b a}", n),
        rtf("unicode", "\\u8364?", n),
        rtf("skipped unicode", "\\uc9\\u1 ", n),
        rtf("hex", "\\'e4", n),
        rtf("control words", "\\b ", n),
        rtf("long control words", "\\abcdefghijklmnopqrstuvwxyz", n),
        rtf("numbers", "\\b99999999999999999999", n),
        rtf("binary", "\\bin1000000000 ", n),
        rtf("destinations", "{\\*\\x a}", n),
        rtf("fonts", "{\\fonttbl{\\f1 Menlo;}}", n),
        rtf("font sizes", "\\fs99 a\\par ", n),
        rtf("paragraphs", "a\\par ", n),
        rtf("breaks", "\\line ", n),
        rtf(
            "links",
            "{\\field{\\*\\fldinst{HYPERLINK \"https://a.bc\"}}{\\fldrslt a}}",
            n,
        ),
        rtf("fields that never end", "{\\field{\\*\\fldinst{", n),
        rtf("list items", "\\ls1\\ilvl8 {\\listtext 1.}a\\par ", n),
        rtf("list changes", "\\ls1\\ilvl8 a\\par\\ls2\\ilvl0 b\\par ", n),
        rtf("cells", "a\\cell ", n),
        rtf("rows", "\\trowd\\cellx1\\intbl a\\cell\\row ", n),
        rtf("code lines", "{\\fonttbl{\\f1 Menlo;}}\\f1 a\\par ", n),
        rtf("marks that change", "\\b a\\b0\\i b\\i0 ", n),
    ]
}

fn tabular_cases(n: usize) -> Vec<Case> {
    vec![
        tabular("rows", "a,b\n", n),
        tabular("tabs on one line", "\t", n),
        tabular("commas on one line", ",", n),
        tabular("quotes", "\"", n),
        tabular("quotes and commas", "\",", n),
        tabular("bad quotes", "\"a\"b,", n),
        tabular("doubled quotes", "\"\"", n),
        tabular("quotes that stay open", "\"a,", n),
        tabular("tab rows", "a\tb\n", n),
        tabular("numbers", "1;2\n", n),
    ]
}

fn styles_chain(n: usize) -> String {
    let mut xml = format!("<w:styles {W}>");
    for at in 0..n {
        let _ = write!(
            xml,
            "<w:style w:type=\"paragraph\" w:styleId=\"s{at}\"><w:name w:val=\"s{at}\"/><w:basedOn w:val=\"s{}\"/></w:style>",
            (at + 1) % n
        );
    }
    xml.push_str("</w:styles>");
    xml
}

fn numbering_many(n: usize) -> String {
    let mut xml = format!("<w:numbering {W}>");
    for at in 0..n {
        let _ = write!(
            xml,
            "<w:abstractNum w:abstractNumId=\"{at}\"><w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/></w:lvl></w:abstractNum>\
             <w:num w:numId=\"{at}\"><w:abstractNumId w:val=\"{at}\"/></w:num>"
        );
    }
    xml.push_str("</w:numbering>");
    xml
}

fn notes_many(n: usize) -> String {
    let mut xml = format!("<w:footnotes {W}>");
    for at in 0..n {
        let _ = write!(
            xml,
            "<w:footnote w:id=\"{at}\"><w:p><w:r><w:t>a</w:t><w:footnoteReference w:id=\"{}\"/></w:r></w:p></w:footnote>",
            (at + 1) % n
        );
    }
    xml.push_str("</w:footnotes>");
    xml
}

fn word_cases(n: usize) -> Vec<Case> {
    let attributes: String = (0..n).map(|at| format!(" a{at}=\"1\"")).collect();
    let namespaces: String = (0..n / 10 + 2).map(|at| format!("<a xmlns:n{at}=\"u\">")).collect();
    let run = "<w:r><w:rPr><w:b/></w:rPr><w:t>a</w:t></w:r><w:r><w:t>b</w:t></w:r>";
    let cell = "<w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc>";
    vec![
        body("empty paragraphs", "<w:p/>".repeat(n)),
        body("runs", format!("<w:p>{}</w:p>", run.repeat(n))),
        body("nested content controls", "<w:sdt><w:sdtContent>".repeat(n) + "<w:p/>"),
        body("nested tables", "<w:tbl><w:tr><w:tc>".repeat(n)),
        body("nested insertions", "<w:ins>".repeat(n)),
        body(
            "nested hyperlinks",
            format!("<w:p>{}</w:p>", "<w:hyperlink w:anchor=\"a\">".repeat(n)),
        ),
        body(
            "a wide table",
            format!(
                "<w:tbl><w:tr>{}</w:tr>{}</w:tbl>",
                cell.repeat(n),
                format!("<w:tr>{cell}</w:tr>").repeat(n)
            ),
        ),
        body(
            "cells that span",
            format!(
                "<w:tbl>{}</w:tbl>",
                "<w:tr><w:trPr><w:gridBefore w:val=\"64\"/></w:trPr><w:tc><w:tcPr><w:gridSpan w:val=\"64\"/></w:tcPr><w:p/></w:tc></w:tr>"
                    .repeat(n)
            ),
        ),
        body(
            "fields that never end",
            format!("<w:p>{}</w:p>", "<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r>".repeat(n)),
        ),
        body(
            "field ends alone",
            format!("<w:p>{}</w:p>", "<w:r><w:fldChar w:fldCharType=\"end\"/></w:r>".repeat(n)),
        ),
        body(
            "list items",
            "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"8\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>a</w:t></w:r></w:p>"
                .repeat(n),
        ),
        body(
            "code lines",
            "<w:p><w:r><w:rPr><w:rFonts w:ascii=\"Menlo\"/></w:rPr><w:t>a</w:t></w:r></w:p>".repeat(n),
        ),
        body(
            "note references",
            format!("<w:p>{}</w:p>", "<w:r><w:footnoteReference w:id=\"1\"/></w:r>".repeat(n)),
        ),
        body(
            "the same drawing",
            format!(
                "<w:p>{}</w:p>",
                "<w:r><w:drawing><a:blip r:embed=\"rId1\"/></w:drawing></w:r>".repeat(n)
            ),
        ),
        body(
            "formulas within each other",
            format!(
                "<w:p><m:oMath>{}</m:oMath></w:p>",
                "<m:f><m:num>".repeat(n)
            ),
        ),
        body("formulas", format!("<w:p>{}</w:p>", "<m:oMath><m:r><m:t>α</m:t></m:r></m:oMath>".repeat(n))),
        body(
            "symbols in a formula",
            format!("<w:p><m:oMath><m:r><m:t>{}</m:t></m:r></m:oMath></w:p>", "α<".repeat(n).replace('<', "&lt;")),
        ),
        word("a chain of styles", Part::Styles, styles_chain(n / 4 + 2)),
        word("many numberings", Part::Numbering, numbering_many(n / 4 + 2)),
        word("notes that refer to each other", Part::Footnotes, notes_many(n / 4 + 2)),
        body("one element with many attributes", format!("<w:p{attributes}/>")),
        body("many namespaces in scope", namespaces),
        Case {
            name: "garbage",
            input: repeated("PK\u{5}\u{6}", n),
            run: run_import,
        },
    ]
}

fn raw_entry(zip: &mut Zip, name: &str, content: &[u8]) {
    zip.add(name, content).expect("a small file");
}

/// A template with everything already there many times, to merge into.
fn big_template(n: usize) -> Vec<u8> {
    let styles = format!(
        "<w:styles {W}>{}</w:styles>",
        (0..n)
            .map(|at| format!("<w:style w:type=\"paragraph\" w:styleId=\"s{at}\"><w:name w:val=\"s{at}\"/></w:style>"))
            .collect::<String>()
    );
    let mut zip = Zip::default();
    let original = word_file(&numbering_many(n).into_bytes(), Part::Numbering);
    let archive = splat_ooxml::zip::Archive::read(&original).expect("a file just made");
    let names: Vec<String> = archive.names().map(str::to_string).collect();
    for name in names {
        let content = archive.get(&name).expect("a part").expect("a part");
        if name == "word/styles.xml" {
            raw_entry(&mut zip, &name, styles.as_bytes());
        } else {
            raw_entry(&mut zip, &name, &content);
        }
    }
    zip.finish()
}

fn run_template_on_generated(input: &[u8]) -> usize {
    apply_template(&generated(), input).map_or(0, |merged| merged.len())
}

fn run_generated_on_template(input: &[u8]) -> usize {
    apply_template(input, &generated()).map_or(0, |merged| merged.len())
}

/// `to_docx` of a long document, with many lists and links, for `apply_template` to take in.
fn big_generated(n: usize) -> Vec<u8> {
    let mut markdown = String::new();
    for at in 0..n {
        let _ = writeln!(markdown, "{}. item [a](https://a.bc/{at})\n\nparagraph\n", at + 2);
    }
    let doc = splat_markdown::reader::parse(&markdown);
    splat_markdown::docx::to_docx(
        &doc,
        &splat_markdown::docx::WordExport {
            title: None,
            figure: &|_| None,
            image: &|_| None,
            quotes: None,
            reference: &|_| None,
            citation: &|_| None,
        },
    )
    .expect("fits a ZIP")
}

fn template_cases(n: usize) -> Vec<Case> {
    vec![
        Case {
            name: "a template with much in it",
            input: big_template(n),
            run: run_template_on_generated,
        },
        Case {
            name: "a long generated file",
            input: big_generated(n / 4 + 1),
            run: run_generated_on_template,
        },
        Case {
            name: "a broken template",
            input: repeated("<w:p>", n),
            run: run_template_on_generated,
        },
    ]
}

fn cases(n: usize) -> Vec<Case> {
    let mut all = html_cases(n);
    all.extend(rtf_cases(n));
    all.extend(tabular_cases(n));
    all.extend(word_cases(n));
    all.extend(template_cases(n));
    all
}

#[test]
fn memory_is_counted() {
    assert!(memory::counts());
}

#[test]
fn hostile_inputs_neither_overflow_the_stack_nor_take_quadratic_time_or_memory() {
    let pairs = cases(SIZE / 4).into_iter().zip(cases(SIZE));
    let problems = scales(pairs.map(|(small, large)| scaled(small, large)).collect());
    assert!(problems.is_empty(), "{problems:#?}");
}

/// Deflated data that says what it inflates to.
fn deflated_entry(name: &str, content: &[u8], declared: u32) -> (Vec<u8>, Vec<u8>) {
    let packed = compress_to_vec(content, 6);
    let crc = crc32(content);
    let size = u32::try_from(packed.len()).expect("a small archive");
    let name_len = u16::try_from(name.len()).expect("a short name");
    let header = |signature: u32, central: bool, offset: u32| {
        let mut h = Vec::new();
        h.extend_from_slice(&signature.to_le_bytes());
        if central {
            h.extend_from_slice(&20u16.to_le_bytes());
        }
        for v in [20u16, 0, 8, 0, 0x21] {
            h.extend_from_slice(&v.to_le_bytes());
        }
        for v in [crc, size, declared] {
            h.extend_from_slice(&v.to_le_bytes());
        }
        h.extend_from_slice(&name_len.to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes());
        if central {
            for v in [0u16, 0, 0] {
                h.extend_from_slice(&v.to_le_bytes());
            }
            h.extend_from_slice(&0u32.to_le_bytes());
            h.extend_from_slice(&offset.to_le_bytes());
        }
        h.extend_from_slice(name.as_bytes());
        h
    };
    let mut data = header(0x0403_4b50, false, 0);
    data.extend_from_slice(&packed);
    (data, header(0x0201_4b50, true, 0))
}

/// An archive of deflated entries; `declared` is what each says it inflates to.
fn bomb(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for (name, content, declared) in entries {
        let offset = u32::try_from(out.len()).expect("a small archive");
        let (mut data, mut central) = deflated_entry(name, content, *declared);
        let offset_at = central.len() - name.len() - 4;
        central[offset_at..offset_at + 4].copy_from_slice(&offset.to_le_bytes());
        out.append(&mut data);
        directory.append(&mut central);
    }
    let start = u32::try_from(out.len()).expect("a small archive");
    let size = u32::try_from(directory.len()).expect("a small archive");
    let count = u16::try_from(entries.len()).expect("few entries");
    out.append(&mut directory);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    for v in [0u16, 0, count, count] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

const TYPES: &str = "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
    <Override PartName=\"/word/document.xml\" \
    ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";

fn word_bomb(document: &[u8]) -> Vec<u8> {
    bomb(&[
        (
            "[Content_Types].xml",
            TYPES.as_bytes(),
            u32::try_from(TYPES.len()).expect("short"),
        ),
        (
            "word/document.xml",
            document,
            u32::try_from(document.len()).expect("a small bomb"),
        ),
    ])
}

#[test]
fn a_bomb_takes_time_and_memory_as_it_inflates_and_stops_at_the_limits() {
    let quarter = vec![b' '; 32 << 20];
    let whole = vec![b' '; 128 << 20];
    let (small, large) = (word_bomb(&quarter), word_bomb(&whole));
    assert!(large.len() < 2 << 20, "{}", large.len());
    let alone = alone();
    let measured = |file: &[u8]| {
        let mut peak = 0;
        let took = alone.fastest(3, || {
            let (refused, held) = peak_of(|| import(file, "p").is_err());
            assert!(refused);
            peak = held;
        });
        (took, peak)
    };
    let (small_took, small_peak) = measured(&small);
    let (large_took, large_peak) = measured(&large);
    assert!(large_took < LIMIT, "{large_took:?}");
    assert!(
        !holds_to_time() || !grows_too_fast(NOTICEABLE, small_took, large_took, 32, 128),
        "{small_took:?} {large_took:?}"
    );
    assert!(
        !memory::grows_too_much(small_peak, large_peak, 32, 128),
        "{small_peak} bytes, then {large_peak} bytes"
    );

    let lying = bomb(&[
        (
            "[Content_Types].xml",
            TYPES.as_bytes(),
            u32::try_from(TYPES.len()).expect("short"),
        ),
        ("word/document.xml", &whole, 1024),
    ]);
    let mut peak = 0;
    let took = alone.fastest(1, || {
        let (refused, held) = peak_of(|| import(&lying, "p").err());
        assert_eq!(refused, Some(splat_import::WordError::Damaged));
        peak = held;
    });
    assert!(
        peak < 1 << 20 && (!holds_to_time() || took < Duration::from_secs(1)),
        "stops when it passes what it said: {peak} bytes, {took:?}"
    );

    let too_much = vec![b' '; (128 << 20) + 1];
    assert_eq!(
        import(&word_bomb(&too_much), "p").err(),
        Some(splat_import::WordError::TooLarge)
    );
}

/// A page from Cocoa whose style sheet names one class `count` times, for declarations as long.
fn cocoa_style_sheet(count: usize) -> Case {
    let page = format!(
        "<html><head><meta name=\"Generator\" content=\"Cocoa HTML Writer\"><style type=\"text/css\">{}{{{}}}</style>\
         </head><body><p class=\"c\">a</p></body></html>",
        ".c,".repeat(count),
        "font-weight: bold; ".repeat(count),
    );
    Case {
        name: "a style sheet from Cocoa that names one class many times",
        input: page.into_bytes(),
        run: |input| from_html(&String::from_utf8_lossy(input)).into_markdown().len(),
    }
}

#[test]
fn a_style_sheet_that_names_one_class_many_times_takes_memory_as_its_text_does() {
    let problems = scales(vec![scaled(cocoa_style_sheet(500), cocoa_style_sheet(2_000))]);
    assert!(problems.is_empty(), "{problems:#?}");
}

/// Just under the nodes the XML budget of one import allows.
const NEARLY_ALL_NODES: usize = 15_990_000;

/// A Word file whose document holds `in_document` and whose four other parts each hold `in_others` elements that no
/// importer reads.
fn crowded_word_file(in_document: usize, in_others: usize) -> Vec<u8> {
    let relationship = |id: usize, kind: &str| {
        format!(
            "<Relationship Id=\"rId{id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}\" \
             Target=\"{kind}.xml\"/>"
        )
    };
    let kinds = ["styles", "numbering", "footnotes", "endnotes"];
    let rels = format!(
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{}</Relationships>",
        kinds
            .iter()
            .enumerate()
            .map(|(id, kind)| relationship(id, kind))
            .collect::<String>()
    );
    let part = |root: &str, inside: (&str, &str), nodes: usize| -> Vec<u8> {
        format!(
            "<w:{root} {W}>{}{}{}</w:{root}>",
            inside.0,
            "<w:a/>".repeat(nodes),
            inside.1
        )
        .into_bytes()
    };
    let mut parts = vec![
        ("[Content_Types].xml".to_string(), TYPES.as_bytes().to_vec()),
        ("word/_rels/document.xml.rels".to_string(), rels.into_bytes()),
        (
            "word/document.xml".to_string(),
            part("document", ("<w:body>", "</w:body>"), in_document),
        ),
    ];
    for kind in kinds {
        parts.push((format!("word/{kind}.xml"), part(kind, ("", ""), in_others)));
    }
    let entries: Vec<(&str, &[u8], u32)> = parts
        .iter()
        .map(|(name, content)| {
            let size = u32::try_from(content.len()).expect("within the size of a part");
            (name.as_str(), content.as_slice(), size)
        })
        .collect();
    bomb(&entries)
}

/// Several parts of a Word file are in memory at once, so the XML of all of them shares one budget: a file full in
/// every part takes about what a file with one full part takes, not five times as much.
#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "an unoptimized build takes minutes to make and read the eighty million nodes"
)]
fn a_word_file_full_in_every_part_takes_no_more_memory_than_one_full_part() {
    const PARTS: usize = 5;
    let one_full = crowded_word_file(NEARLY_ALL_NODES, 0);
    let spread = crowded_word_file(NEARLY_ALL_NODES / PARTS, NEARLY_ALL_NODES / PARTS);
    let all_full = crowded_word_file(NEARLY_ALL_NODES, NEARLY_ALL_NODES);
    let _alone = alone();
    let (one_read, one_peak) = peak_of(|| import(&one_full, "p").is_ok());
    let (spread_read, spread_peak) = peak_of(|| import(&spread, "p").is_ok());
    let (all_refused, all_peak) = peak_of(|| import(&all_full, "p").err());
    eprintln!("one full part: {one_peak} bytes; spread: {spread_peak} bytes; all full: {all_peak} bytes");
    assert!(one_read && spread_read, "within the budget is read");
    assert_eq!(all_refused, Some(splat_import::WordError::TooLarge));
    for (what, peak) in [("spread over the parts", spread_peak), ("full in every part", all_peak)] {
        assert!(
            peak <= one_peak / 2 * 3,
            "{what}: {peak} bytes against {one_peak} bytes for one full part"
        );
    }
}
