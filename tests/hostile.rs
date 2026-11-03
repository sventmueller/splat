//! Documents built to hurt, each small enough to arrive as an attachment: nesting that overflows a stack and patterns
//! that take quadratic time. Every consumer of the parsed document, the keystrokes and the commands run on a stack as
//! small as the app's smallest, within a time limit that only a quadratic path misses. Every document has a thread named
//! after it, so a stack overflow, which ends the test run, names its document.

use std::sync::mpsc;
use std::time::Duration;

use splat::markdown::commands::{self, InlineStyle, ListStyle, heading_anchors};
use splat::markdown::doc::{DiagramKind, figures, has_remote_images, parse, walk_blocks, walk_inlines};
use splat::markdown::docx::{WordExport, to_docx};
use splat::markdown::highlight::highlight;
use splat::markdown::html::{Export, to_html};
use splat::markdown::incremental::{memory_estimate, reparse_and_color};
use splat::markdown::typography::Quotes;
use splat::preview::bridge::Bridge;
use splat::preview::layout::{Resolver, layout, relayout, reusable};
use splat::preview::map::{Index, Shown, preview_offset};

const STACK: usize = 2 << 20;
/// An unoptimized build takes ten times as long for linear work too. On Intel the system allocator can spend a minute on
/// such documents moving its regions back and forth, so there the limit only catches endless loops, while Apple Silicon
/// holds the same code to time.
const LIMIT: Duration = Duration::from_secs(if cfg!(debug_assertions) {
    100
} else if cfg!(target_arch = "x86_64") {
    300
} else {
    10
});
const MAX_PAIRED: usize = 16;

struct Everything;

impl Resolver for Everything {
    fn math_ready(&self, _: &str, _: bool) -> bool {
        false
    }
    fn diagram_ready(&self, _: &str, _: DiagramKind) -> bool {
        false
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

fn exports(text: &str) -> usize {
    let reference = |r: &str| Some(format!("https://example.com/{r}"));
    let citation = |k: &str| Some(k.to_string());
    let page = to_html(
        text,
        &Export {
            title: "Probe",
            figure: &|_| None,
            embed: &|_| None,
            rebase: &|link| link.to_string(),
            quotes: Some(QUOTES),
            reference: &reference,
            citation: &citation,
        },
    );
    let word = to_docx(
        &parse(text),
        &WordExport {
            source: text,
            figure: &|_| None,
            image: &|_| None,
            quotes: Some(QUOTES),
            reference: &reference,
            citation: &citation,
        },
    );
    page.len() + word.len()
}

/// Every step the preview and the exports take with a document.
fn consume(text: &str) {
    let doc = parse(text);
    let spans = highlight(text, &doc);
    figures(&doc);
    has_remote_images(&doc);
    heading_anchors(&doc);
    let laid_out = layout(text, &doc, &Everything, None);
    let index = Index::new(&laid_out);
    exports(text);

    let middle = text.floor_char_boundary(text.len() / 2);
    let bridge = Bridge {
        text,
        doc: &doc,
        layout: &laid_out,
        index: &index,
    };
    let shown: usize = laid_out.blocks.iter().map(|b| b.len_u16).sum();
    let at = preview_offset(&laid_out, &index, middle);
    bridge.replace(at..at, "x");
    bridge.replace(Shown(0)..Shown(shown), "x");
    bridge.replace(Shown(at.0 / 2)..at, "");
    bridge.newline(at);
    bridge.source_selection(Shown(0)..Shown(shown));

    commands::toggle_inline(text, &doc, middle..middle, InlineStyle::Bold);
    commands::toggle_inline(text, &doc, 0..text.len(), InlineStyle::Italic);
    commands::remove_formatting(text, &doc, 0..text.len());
    commands::set_heading(text, 0..text.len(), 2);
    commands::toggle_list(text, 0..text.len(), ListStyle::Bullet);
    commands::toggle_quote(text, 0..text.len());
    commands::insert_toc(text, &doc, 0..0);

    let mut typed = text.to_string();
    typed.insert(middle, 'x');
    let mut reparsed = doc;
    let parsed = reparse_and_color(&mut reparsed, text, &spans, &typed);
    if let Some(splice) = &parsed.splice {
        let reuse = reusable(splice, &parsed.doc, None);
        relayout(&typed, &parsed.doc, &Everything, None, laid_out, &reuse);
    }
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

fn numbered(count: usize, line: impl Fn(usize) -> String) -> String {
    (0..count).map(line).collect()
}

#[expect(clippy::too_many_lines, reason = "one line per document")]
fn cases() -> Vec<(&'static str, String)> {
    vec![
        ("nested quotes", "> ".repeat(100_000) + "x"),
        ("nested lists on a line", "- ".repeat(100_000) + "x"),
        (
            "nested lists by indent",
            numbered(3_000, |i| format!("{}- x\n", " ".repeat(i * 2))),
        ),
        ("nested div", "<div>\n".repeat(200_000)),
        (
            "nested div with images",
            "<div><img src=\"https://a.de/x.png\">\n".repeat(50_000),
        ),
        ("nested tables", "<table><tr><td>".repeat(50_000)),
        ("nested details", "<details>\n\n".repeat(50_000)),
        ("nested lists in html", "<ul><li>".repeat(50_000)),
        ("nested quotes in html", "<blockquote>".repeat(50_000)),
        ("open inline html", "<b>".repeat(300_000)),
        (
            "nested inline html",
            "<b>".repeat(50_000) + "x" + &"</b>".repeat(50_000),
        ),
        (
            "nested inline html in emphasis",
            (format!("{}*", "<b>".repeat(MAX_PAIRED))).repeat(100)
                + "x"
                + &format!("*{}", "</b>".repeat(MAX_PAIRED)).repeat(100),
        ),
        ("nested links", "[".repeat(30_000) + "x" + &"](a)".repeat(30_000)),
        ("nested emphasis", "*".repeat(30_000) + "x" + &"*".repeat(30_000)),
        ("open brackets", "[".repeat(500_000)),
        ("open links", "[a](".repeat(200_000)),
        ("open images", "![".repeat(200_000)),
        ("open wiki links", "[[a".repeat(200_000)),
        ("open citations", "[@a ".repeat(200_000)),
        ("emphasis", "*a ".repeat(300_000)),
        ("angle brackets", "<".repeat(500_000)),
        ("dollars", "$a ".repeat(300_000)),
        ("tildes", "~".repeat(500_000)),
        ("highlights", "==a ".repeat(200_000)),
        ("superscripts", "^a ".repeat(300_000)),
        ("spoilers", "||a ".repeat(200_000)),
        ("shortcodes", ":a:".repeat(300_000)),
        ("html comments", "<!-- ".repeat(200_000)),
        ("quotation marks", "\"a ".repeat(300_000)),
        ("link references", "[a]: b\n".repeat(100_000) + &"[a] ".repeat(100_000)),
        ("footnote references", "[^1] ".repeat(100_000) + "\n\n[^1]: x\n"),
        (
            "footnote definitions",
            numbered(30_000, |i| format!("[^{i}] ")) + "\n\n" + &numbered(30_000, |i| format!("[^{i}]: x\n")),
        ),
        ("inline footnotes", "^[a] ".repeat(30_000)),
        (
            "inline footnotes in paragraphs",
            numbered(30_000, |i| format!("^[{i}]\n\n")),
        ),
        ("nested inline footnotes", "^[".repeat(20_000)),
        ("headings", "# a\n".repeat(200_000)),
        ("blank lines", "\n".repeat(2_000_000)),
        ("one long line", "wort ".repeat(1_000_000)),
        ("wide table", table(30_000, 1, "x")),
        ("large table", table(500, 500, "x")),
        ("short rows in a wide table", table(3_000, 3_000, "")),
        (
            "front matter without end",
            String::from("---\n") + &"a: b\n".repeat(300_000),
        ),
        ("code fences", "```\na\n```\n".repeat(100_000)),
        ("comments", "%%\n".repeat(200_000)),
        ("definition lists", "a\n\n: b\n\n".repeat(50_000)),
        ("escapes", "\\".repeat(500_000)),
        ("entities", "&amp;".repeat(200_000)),
        ("autolinks", "https://a.de ".repeat(100_000)),
        ("tags in a paragraph", "#tag ".repeat(200_000)),
        ("references in a paragraph", "@user #12 ".repeat(100_000)),
        ("citations in a paragraph", "[@doe] ".repeat(200_000)),
        ("heading attributes", "# a {#i .k}\n".repeat(100_000)),
        ("callouts", "> [!NOTE]- a\n\n".repeat(100_000)),
        ("open tags", "<div ".repeat(200_000)),
        ("open tags with attributes", "<a b=c ".repeat(100_000)),
        ("open tags with quotes", "<a b=\"c\" ".repeat(100_000)),
        ("dropped elements", "<style></style>".repeat(100_000)),
        ("open dropped elements", "<style>".repeat(100_000)),
        ("named entities", "&eacute;".repeat(100_000)),
        ("unknown entities", "&gibtsnicht;".repeat(100_000)),
        ("bold in one paragraph", "w **b** ".repeat(100_000)),
        (
            "formatted paragraphs behind front matter",
            String::from("---\na: b\n---\n\n")
                + &"Satz **fett** *schräg* `code` [Link](https://a.de)\n\n".repeat(30_000),
        ),
        ("escaped pipes in a table", table(2, 1, &"\\|ä".repeat(100_000))),
        ("zero bytes", "\0".repeat(500_000)),
        ("right-to-left marks", "\u{202E}א ".repeat(300_000)),
        ("combining marks", String::from("a") + &"\u{0301}".repeat(500_000)),
        (
            "a table asking for millions of empty cells",
            String::from("<table><tr>") + &"<td>".repeat(4_000) + &"<tr>".repeat(4_000),
        ),
        (
            "closing tags that close nothing",
            String::from("<div>") + &"<x>".repeat(70_000) + &"</y>".repeat(70_000) + "\n\nabc\n\ndef\n",
        ),
        ("html blocks left open", "<div>\n\n".repeat(100_000)),
        (
            "html blocks left open and closing something else",
            "<div>\n\n".repeat(50_000) + &"</x>\n\n".repeat(50_000),
        ),
        (
            "a table cell of many lines",
            String::from("<table><tr><td><ul>") + &"<li>a".repeat(100_000),
        ),
        ("open formulas in brackets", "\\[ ".repeat(100_000)),
        ("open formulas in parentheses", "\\( ".repeat(100_000)),
        ("open formulas in backticks", "$` ".repeat(100_000)),
        (
            "inline footnotes behind their closing brackets",
            "]".repeat(100_000) + &"^[".repeat(100_000),
        ),
        (
            "an endless diagram line",
            String::from("```mermaid\n") + &"a[".repeat(100_000) + "\n```\n",
        ),
    ]
}

/// Runs a document on a thread of its own and waits at most `LIMIT`, so an endless loop fails as well.
fn survives(name: &'static str, text: String) -> Result<(), String> {
    let (done, finished) = mpsc::channel();
    std::thread::Builder::new()
        .name(format!("hostile document: {name}"))
        .stack_size(STACK)
        .spawn(move || {
            consume(&text);
            let _ = done.send(());
        })
        .map_err(|e| format!("{name}: {e}"))?;
    match finished.recv_timeout(LIMIT) {
        Ok(()) => Ok(()),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!("{name}: over {LIMIT:?}")),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(format!("{name}: panicked")),
    }
}

#[test]
fn pasting_text_full_of_marks_takes_linear_time() {
    for (name, paste) in [
        ("references", "&a ".repeat(100_000)),
        ("ampersands", "&".repeat(200_000)),
        ("semicolons", ";".repeat(200_000)),
        ("colons", ":a".repeat(100_000)),
    ] {
        let (done, finished) = mpsc::channel();
        std::thread::spawn(move || {
            let text = "x\n";
            let doc = parse(text);
            let laid_out = layout(text, &doc, &Everything, None);
            let index = Index::new(&laid_out);
            let bridge = Bridge {
                text,
                doc: &doc,
                layout: &laid_out,
                index: &index,
            };
            bridge.replace(Shown(1)..Shown(1), &paste);
            let _ = done.send(());
        });
        assert!(finished.recv_timeout(LIMIT).is_ok(), "{name}");
    }
}

#[test]
fn a_small_table_stays_small_however_many_cells_it_asks_for() {
    let text = String::from("<table><tr>") + &"<td>".repeat(4_000) + &"<tr>".repeat(4_000);
    let laid_out = layout(&text, &parse(&text), &Everything, None);
    let paragraphs: usize = laid_out.blocks.iter().map(|b| b.paras.len()).sum();
    assert!(paragraphs < 10_000, "{paragraphs} paragraphs from {} bytes", text.len());
}

#[test]
fn the_memory_estimate_keeps_up_with_what_a_parse_builds() {
    const LEAST_PER_NODE: u64 = 150;
    for (name, text) in cases() {
        let thread = std::thread::Builder::new().stack_size(STACK).spawn(move || {
            let doc = parse(&text);
            let mut nodes = 1;
            walk_blocks(&doc.blocks, &mut |b| {
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
fn hostile_documents_neither_overflow_the_stack_nor_take_quadratic_time() {
    let cases = std::sync::Mutex::new(cases());
    let failures = std::sync::Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(2, |n| n.get().min(4));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let Some((name, text)) = cases.lock().unwrap().pop() else {
                        return;
                    };
                    if let Err(failure) = survives(name, text) {
                        failures.lock().unwrap().push(failure);
                    }
                }
            });
        }
    });
    let failures = failures.into_inner().unwrap();
    assert!(failures.is_empty(), "{failures:#?}");
}
