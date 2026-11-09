//! Markdown documents built to hurt, through the preview, the exports, the keystrokes and the commands.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use splat_markdown::commands::{self, InlineStyle, ListStyle, heading_anchors};
use splat_markdown::doc::Align;
use splat_markdown::doc::facts::TextFacts;
use splat_markdown::doc::{DiagramKind, Doc, figures, walk_blocks, walk_inlines};
use splat_markdown::docx::{Picture, WordExport, to_docx};
use splat_markdown::highlight::highlight;
use splat_markdown::html::{Export, to_html};
use splat_markdown::incremental::{memory_estimate, reparse_and_color};
use splat_markdown::reader::parse;
use splat_markdown::remote::has_local_files;
use splat_markdown::table::{self, TableOp};
use splat_markdown::text::{Byte, bytes};
use splat_markdown::typography::Quotes;
use splat_preview::bridge::Bridge;
use splat_preview::layout::{Resolver, layout, relayout, reusable};
use splat_preview::map::{Index, Shown, preview_offset};

use splat_testkit::timing::{LIMIT, NOTICEABLE, STACK, Scaled, Stages, alone, grows_too_fast, holds_to_time, scales};

use super::common::{applies, numbered};

const MAX_PAIRED: usize = 16;

/// An edit in the preview reads the text it would make for as far as `MAX_CHECKED` goes, and from there on not at all, so
/// its time grows faster up to that size and then stops; only other programs on the Mac can take it above this.
const NOTICEABLE_IN_THE_PREVIEW: Duration = Duration::from_millis(2000);

/// Says that formulas and diagrams are rendered, so that the preview takes the path of pictures with them, or not.
struct Everything(bool);

impl Resolver for Everything {
    fn math_ready(&self, _: &str, _: bool) -> bool {
        self.0
    }
    fn diagram_ready(&self, _: &str, _: DiagramKind) -> bool {
        self.0
    }
    fn quotes(&self) -> Option<Quotes> {
        Some(QUOTES)
    }
    fn references(&self) -> bool {
        true
    }
    fn citation(&self, key: &str) -> Option<String> {
        Some(key.to_string())
    }
}

const QUOTES: Quotes = Quotes {
    open: '„',
    close: '“',
    open_single: '‚',
    close_single: '‘',
};

fn exports(text: &str, rendered: bool) -> usize {
    let reference = |r: &str| Some(format!("https://example.com/{r}"));
    let citation = |k: &str| Some(k.to_string());
    let picture = || Picture {
        data: b"\x89PNG".to_vec(),
        jpeg: false,
        width: 10.0,
        height: 10.0,
    };
    let page = to_html(
        &parse(text),
        &Export {
            title: "Probe",
            figure: &|_| rendered.then(|| "<img>".to_string()),
            embed: &|_| rendered.then(|| "data:image/png;base64,".to_string()),
            rebase: &|link| link.to_string(),
            quotes: Some(QUOTES),
            reference: &reference,
            citation: &citation,
        },
    );
    let word = to_docx(
        &parse(text),
        &WordExport {
            title: None,
            figure: &|_| rendered.then(picture),
            image: &|_| rendered.then(picture),
            quotes: Some(QUOTES),
            reference: &reference,
            citation: &citation,
        },
    )
    .expect("a small document");
    assert!(
        page.trim_end().ends_with("</html>") && word.starts_with(b"PK"),
        "an export without its frame"
    );
    page.len() + word.len()
}

fn commands_on(text: &str, doc: &Doc, middle: Byte) {
    applies(text, commands::toggle_inline(doc, middle..middle, InlineStyle::Bold));
    applies(
        text,
        commands::toggle_inline(doc, bytes(0..text.len()), InlineStyle::Italic),
    );
    applies(text, commands::remove_formatting(doc, bytes(0..text.len())));
    applies(text, commands::set_heading(&parse(text), bytes(0..text.len()), 2));
    applies(
        text,
        commands::toggle_list(text, bytes(0..text.len()), ListStyle::Bullet),
    );
    applies(text, commands::toggle_quote(text, bytes(0..text.len())));
    if let Some(edit) = commands::insert_toc(doc, bytes(0..0)) {
        applies(text, edit);
    }
    for op in [
        TableOp::InsertRowAbove,
        TableOp::InsertRowBelow,
        TableOp::InsertColumnLeft,
        TableOp::InsertColumnRight,
        TableOp::DeleteRow,
        TableOp::DeleteColumn,
        TableOp::Align(Align::Center),
        TableOp::Format,
    ] {
        if let Some(edit) = table::table_edit(doc, middle, op) {
            applies(text, edit);
        }
    }
}

/// Every step the preview and the exports take with a document.
fn consume(text: &str) -> Stages {
    let mut stages = Stages::default();
    let doc = stages.time("parse", || parse(text));
    let spans = stages.time("colors", || highlight(&doc));
    stages.time("facts", || TextFacts::of(&doc));
    stages.time("what the document holds", || {
        figures(&doc);
        has_local_files(&doc);
        heading_anchors(&doc);
    });
    stages.time("layout with images", || layout(&doc, &Everything(true), None));
    let laid_out = stages.time("layout", || layout(&doc, &Everything(false), None));
    let index = stages.time("index", || Index::new(&laid_out));
    stages.time("exports", || {
        exports(text, false);
        exports(text, true);
    });

    let middle = text.floor_char_boundary(text.len() / 2);
    let bridge = Bridge::new(&doc, &laid_out, &index).unwrap();
    let shown: usize = laid_out.blocks.iter().map(|b| b.len_u16).sum();
    let at = preview_offset(&laid_out, &index, Byte::new(middle));
    stages.time_noticeable_from("editing in the preview", NOTICEABLE_IN_THE_PREVIEW, || {
        for edit in [
            bridge.replace(at..at, "x"),
            bridge.replace(Shown::new(0)..Shown::new(shown), "x"),
            bridge.replace(Shown::new(at.get() / 2)..at, ""),
            bridge.newline(at),
        ]
        .into_iter()
        .flatten()
        {
            applies(text, edit);
        }
        bridge.source_selection(Shown::new(0)..Shown::new(shown));
    });
    stages.time("commands", || commands_on(text, &doc, Byte::new(middle)));

    let mut typed = text.to_string();
    typed.insert(middle, 'x');
    stages.time("a keystroke", || {
        let parsed = reparse_and_color(doc, &spans, &typed);
        if let Some(splice) = &parsed.splice {
            let reuse = reusable(splice, &parsed.doc, None);
            relayout(&parsed.doc, &Everything(false), None, laid_out, &reuse);
        }
    });
    stages
}

fn table(columns: usize, rows: usize, cell: &str) -> String {
    let row = format!("|{}\n", format!("{cell}|").repeat(columns));
    format!(
        "|{}\n|{}\n{}",
        "a|".repeat(columns),
        "-|".repeat(columns),
        row.repeat(rows)
    )
}

#[expect(clippy::too_many_lines, reason = "one line per document")]
/// The documents, with every count at `quarters` fourths of its full size.
fn cases(quarters: usize) -> Vec<(&'static str, String)> {
    let n = |count: usize| count * quarters / 4;
    vec![
        ("nested quotes", "> ".repeat(n(100_000)) + "x"),
        (
            "link definitions between paragraphs",
            numbered(n(160_000), |i| format!("para {i}\n\n[l{i}]: url\n\n")),
        ),
        ("nested lists on a line", "- ".repeat(n(100_000)) + "x"),
        (
            "nested lists by indent",
            numbered(n(3_000), |i| format!("{}- x\n", " ".repeat(i * 2))),
        ),
        ("nested div", "<div>\n".repeat(n(200_000))),
        (
            "nested div with images",
            "<div><img src=\"https://a.de/x.png\">\n".repeat(n(50_000)),
        ),
        ("nested tables", "<table><tr><td>".repeat(n(50_000))),
        ("nested details", "<details>\n\n".repeat(n(50_000))),
        ("nested lists in html", "<ul><li>".repeat(n(50_000))),
        ("nested quotes in html", "<blockquote>".repeat(n(50_000))),
        ("open inline html", "<b>".repeat(n(300_000))),
        (
            "nested inline html",
            "<b>".repeat(n(50_000)) + "x" + &"</b>".repeat(n(50_000)),
        ),
        (
            "nested inline html in emphasis",
            (format!("{}*", "<b>".repeat(MAX_PAIRED))).repeat(n(100))
                + "x"
                + &format!("*{}", "</b>".repeat(MAX_PAIRED)).repeat(n(100)),
        ),
        ("nested links", "[".repeat(n(30_000)) + "x" + &"](a)".repeat(n(30_000))),
        ("nested emphasis", "*".repeat(n(30_000)) + "x" + &"*".repeat(n(30_000))),
        ("open brackets", "[".repeat(n(500_000))),
        ("open links", "[a](".repeat(n(200_000))),
        ("open images", "![".repeat(n(200_000))),
        ("open wiki links", "[[a".repeat(n(200_000))),
        ("open citations", "[@a ".repeat(n(200_000))),
        ("emphasis", "*a ".repeat(n(300_000))),
        ("angle brackets", "<".repeat(n(500_000))),
        ("dollars", "$a ".repeat(n(300_000))),
        ("tildes", "~".repeat(n(500_000))),
        ("highlights", "==a ".repeat(n(200_000))),
        ("superscripts", "^a ".repeat(n(300_000))),
        ("spoilers", "||a ".repeat(n(200_000))),
        ("shortcodes", ":a:".repeat(n(300_000))),
        ("html comments", "<!-- ".repeat(n(200_000))),
        ("quotation marks", "\"a ".repeat(n(300_000))),
        (
            "link references",
            "[a]: b\n".repeat(n(100_000)) + &"[a] ".repeat(n(100_000)),
        ),
        ("footnote references", "[^1] ".repeat(n(100_000)) + "\n\n[^1]: x\n"),
        (
            "footnote definitions",
            numbered(n(30_000), |i| format!("[^{i}] ")) + "\n\n" + &numbered(n(30_000), |i| format!("[^{i}]: x\n")),
        ),
        ("inline footnotes", "^[a] ".repeat(n(30_000))),
        (
            "inline footnotes in paragraphs",
            numbered(n(30_000), |i| format!("^[{i}]\n\n")),
        ),
        ("nested inline footnotes", "^[".repeat(n(20_000))),
        (
            "inline footnotes closed in the next paragraph",
            format!("{}\n\n{}", "^[".repeat(n(30_000)), "]".repeat(n(30_000))),
        ),
        (
            "formulas closed in the next paragraph",
            format!("{}\n\n{}", "\\( ".repeat(n(30_000)), "\\) ".repeat(n(30_000))),
        ),
        (
            "code formulas closed in the next paragraph",
            format!("{}\n\n{}", "$` ".repeat(n(30_000)), "`$ ".repeat(n(30_000))),
        ),
        ("labels like those of inline notes", "[^__inline_".repeat(n(100_000))),
        ("lines from an old Mac", "a\r".repeat(n(200_000))),
        ("paragraphs from an old Mac", "a\r\r".repeat(n(100_000))),
        ("blank lines from an old Mac", "\r".repeat(n(500_000))),
        ("lines from Windows", "a\r\n".repeat(n(200_000))),
        (
            "a list number at the end of the numbers",
            "<ol start=\"18446744073709551615\"><li>a<li>b</ol>\n".repeat(n(1_000)),
        ),
        ("link definitions", "[a]: b\n".repeat(n(100_000))),
        ("brackets and definitions", "[a] [b]: c\n\n".repeat(n(100_000))),
        ("headings", "# a\n".repeat(n(200_000))),
        ("blank lines", "\n".repeat(n(2_000_000))),
        ("one long line", "wort ".repeat(n(1_000_000))),
        ("wide table", table(n(30_000), 1, "x")),
        ("large table", table(n(500), 500, "x")),
        ("short rows in a wide table", table(n(3_000), 3_000, "")),
        (
            "front matter without end",
            String::from("---\n") + &"a: b\n".repeat(n(300_000)),
        ),
        ("code fences", "```\na\n```\n".repeat(n(100_000))),
        ("comments", "%%\n".repeat(n(200_000))),
        ("definition lists", "a\n\n: b\n\n".repeat(n(50_000))),
        ("escapes", "\\".repeat(n(500_000))),
        ("entities", "&amp;".repeat(n(200_000))),
        ("autolinks", "https://a.de ".repeat(n(100_000))),
        ("tags in a paragraph", "#tag ".repeat(n(200_000))),
        ("references in a paragraph", "@user #12 ".repeat(n(100_000))),
        ("citations in a paragraph", "[@doe] ".repeat(n(200_000))),
        ("heading attributes", "# a {#i .k}\n".repeat(n(100_000))),
        ("callouts", "> [!NOTE]- a\n\n".repeat(n(100_000))),
        ("open tags", "<div ".repeat(n(200_000))),
        ("open tags with attributes", "<a b=c ".repeat(n(100_000))),
        ("open tags with quotes", "<a b=\"c\" ".repeat(n(100_000))),
        ("dropped elements", "<style></style>".repeat(n(100_000))),
        ("open dropped elements", "<style>".repeat(n(100_000))),
        ("named entities", "&eacute;".repeat(n(100_000))),
        ("unknown entities", "&gibtsnicht;".repeat(n(100_000))),
        ("bold in one paragraph", "w **b** ".repeat(n(100_000))),
        (
            "formatted paragraphs behind front matter",
            String::from("---\na: b\n---\n\n")
                + &"Satz **fett** *schräg* `code` [Link](https://a.de)\n\n".repeat(n(30_000)),
        ),
        ("escaped pipes in a table", table(2, 1, &"\\|ä".repeat(n(100_000)))),
        ("zero bytes", "\0".repeat(n(500_000))),
        ("right-to-left marks", "\u{202E}א ".repeat(n(300_000))),
        ("combining marks", String::from("a") + &"\u{0301}".repeat(n(500_000))),
        (
            "a table asking for millions of empty cells",
            String::from("<table><tr>") + &"<td>".repeat(n(4_000)) + &"<tr>".repeat(n(4_000)),
        ),
        (
            "closing tags that close nothing",
            String::from("<div>") + &"<x>".repeat(n(70_000)) + &"</y>".repeat(n(70_000)) + "\n\nabc\n\ndef\n",
        ),
        ("html blocks left open", "<div>\n\n".repeat(n(100_000))),
        (
            "html blocks left open and closing something else",
            "<div>\n\n".repeat(n(50_000)) + &"</x>\n\n".repeat(n(50_000)),
        ),
        (
            "a table cell of many lines",
            String::from("<table><tr><td><ul>") + &"<li>a".repeat(n(100_000)),
        ),
        ("open formulas in brackets", "\\[ ".repeat(n(100_000))),
        ("open formulas in parentheses", "\\( ".repeat(n(100_000))),
        ("open formulas in backticks", "$` ".repeat(n(100_000))),
        (
            "inline footnotes behind their closing brackets",
            "]".repeat(n(100_000)) + &"^[".repeat(n(100_000)),
        ),
        (
            "an endless diagram line",
            String::from("```mermaid\n") + &"a[".repeat(n(100_000)) + "\n```\n",
        ),
        (
            "an endless diagram line of every opening mark",
            String::from("```mermaid\n") + &"a[b(c{".repeat(n(100_000)) + "\n```\n",
        ),
    ]
}

/// How long pasting takes into a document of one line.
fn pasting(paste: String) -> Duration {
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        let text = "x\n";
        let doc = parse(text);
        let laid_out = layout(&doc, &Everything(false), None);
        let index = Index::new(&laid_out);
        let bridge = Bridge::new(&doc, &laid_out, &index).unwrap();
        let started = Instant::now();
        bridge.replace(Shown::new(1)..Shown::new(1), &paste);
        let _ = done.send(started.elapsed());
    });
    finished.recv_timeout(LIMIT).expect("within the limit")
}

#[test]
fn pasting_text_full_of_marks_takes_linear_time() {
    for (name, mark, count) in [
        ("references", "&a ", 100_000),
        ("ampersands", "&", 200_000),
        ("semicolons", ";", 200_000),
        ("colons", ":a", 100_000),
    ] {
        let shortest_of = |paste: &str| (0..3).map(|_| pasting(paste.to_string())).min().unwrap();
        let (small, large) = (mark.repeat(count / 4), mark.repeat(count));
        let alone = alone();
        let (small_took, large_took) = (shortest_of(&small), shortest_of(&large));
        drop(alone);
        if holds_to_time() {
            assert!(
                !grows_too_fast(NOTICEABLE, small_took, large_took, small.len(), large.len()),
                "{name}: {small_took:?}, then {large_took:?}"
            );
        }
    }
}

#[test]
fn a_small_table_stays_small_however_many_cells_it_asks_for() {
    let text = String::from("<table><tr>") + &"<td>".repeat(4_000) + &"<tr>".repeat(4_000);
    let laid_out = layout(&parse(&text), &Everything(false), None);
    let paragraphs: usize = laid_out.blocks.iter().map(|b| b.paras.len()).sum();
    assert!(paragraphs < 10_000, "{paragraphs} paragraphs from {} bytes", text.len());
}

#[test]
fn the_memory_estimate_keeps_up_with_what_a_parse_builds() {
    const LEAST_PER_NODE: u64 = 150;
    for (name, text) in cases(4) {
        let thread = std::thread::Builder::new().stack_size(STACK).spawn(move || {
            let doc = parse(&text);
            let mut nodes = 1;
            walk_blocks(doc.blocks(), &mut |b| {
                nodes += 1;
                walk_inlines(b.inlines(), &mut |_| nodes += 1);
            });
            memory_estimate(&text) / nodes
        });
        let per_node = thread.expect("a thread").join().expect("no panic");
        assert!(per_node >= LEAST_PER_NODE, "{name}: {per_node} per node");
    }
}

#[test]
fn hostile_documents_neither_overflow_the_stack_nor_take_quadratic_time_or_memory() {
    let cases = cases(1)
        .into_iter()
        .zip(cases(4))
        .map(|((name, small), (_, large))| Scaled {
            name,
            small: small.into(),
            large: large.into(),
            work: consume,
        })
        .collect();
    let failures = scales::<str>(cases);
    assert!(failures.is_empty(), "{failures:#?}");
}
