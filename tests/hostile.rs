//! Documents built to hurt, each small enough to arrive as an attachment: nesting that overflows a stack and patterns
//! that take quadratic time. Every consumer of the parsed document, the keystrokes and the commands run on a stack as
//! small as the app's smallest. Each document runs at two sizes, whose times must grow about as the text does. Every
//! document has a thread named after it, so a stack overflow, which ends the test run, names its document.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use splat::markdown::commands::{self, Edit, InlineStyle, ListStyle, heading_anchors};
use splat::markdown::doc::{DiagramKind, Doc, figures, has_remote_images, walk_blocks, walk_inlines};
use splat::markdown::docx::{Picture, WordExport, to_docx};
use splat::markdown::highlight::highlight;
use splat::markdown::html::{Export, to_html};
use splat::markdown::incremental::{memory_estimate, reparse_and_color};
use splat::markdown::reader::parse;
use splat::markdown::style::Align;
use splat::markdown::table::{self, TableOp};
use splat::markdown::text::{Byte, bytes};
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
        text,
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

/// An edit a command returned has to apply to the text it was made for.
fn applies(text: &str, edit: Edit) {
    let mut out = text.to_string();
    assert!(
        edit.range.end <= Byte::new(text.len())
            && text.is_char_boundary(edit.range.start.get())
            && text.is_char_boundary(edit.range.end.get()),
        "an edit outside the text: {:?} of {} bytes",
        edit.range,
        text.len()
    );
    edit.apply_to(&mut out);
    assert!(
        edit.selection.end <= Byte::new(out.len())
            && out.is_char_boundary(edit.selection.start.get())
            && out.is_char_boundary(edit.selection.end.get()),
        "a selection outside the result: {:?} of {} bytes",
        edit.selection,
        out.len()
    );
}

fn commands_on(text: &str, doc: &Doc, middle: Byte) {
    applies(text, commands::toggle_inline(doc, middle..middle, InlineStyle::Bold));
    applies(
        text,
        commands::toggle_inline(doc, bytes(0..text.len()), InlineStyle::Italic),
    );
    applies(text, commands::remove_formatting(doc, bytes(0..text.len())));
    applies(text, commands::set_heading(text, bytes(0..text.len()), 2));
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
fn consume(text: &str) {
    let doc = parse(text);
    let spans = highlight(&doc);
    figures(&doc);
    has_remote_images(&doc);
    heading_anchors(&doc);
    layout(&doc, &Everything(true), None);
    let laid_out = layout(&doc, &Everything(false), None);
    let index = Index::new(&laid_out);
    exports(text, false);
    exports(text, true);

    let middle = text.floor_char_boundary(text.len() / 2);
    let bridge = Bridge::new(&doc, &laid_out, &index).unwrap();
    let shown: usize = laid_out.blocks.iter().map(|b| b.len_u16).sum();
    let at = preview_offset(&laid_out, &index, Byte::new(middle));
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
    commands_on(text, &doc, Byte::new(middle));

    let mut typed = text.to_string();
    typed.insert(middle, 'x');
    let mut reparsed = doc;
    let parsed = reparse_and_color(&mut reparsed, &spans, &typed);
    if let Some(splice) = &parsed.splice {
        let reuse = reusable(splice, &parsed.doc, None);
        relayout(&parsed.doc, &Everything(false), None, laid_out, &reuse);
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
/// The documents, with every count at `quarters` fourths of its full size.
fn cases(quarters: usize) -> Vec<(&'static str, String)> {
    let n = |count: usize| count * quarters / 4;
    vec![
        ("nested quotes", "> ".repeat(n(100_000)) + "x"),
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

/// Runs a document on a thread of its own and waits at most `LIMIT`, so an endless loop fails as well.
fn survives(name: &'static str, text: &std::sync::Arc<str>) -> Result<Duration, String> {
    let (done, finished) = mpsc::channel();
    let text = text.clone();
    std::thread::Builder::new()
        .name(format!("hostile document: {name}"))
        .stack_size(STACK)
        .spawn(move || {
            let started = Instant::now();
            consume(&text);
            let _ = done.send(started.elapsed());
        })
        .map_err(|e| format!("{name}: {e}"))?;
    match finished.recv_timeout(LIMIT) {
        Ok(took) => Ok(took),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!("{name}: over {LIMIT:?}")),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(format!("{name}: panicked")),
    }
}

/// Whether time that grows like `times` with a text growing like `sizes` is more than linear work with some noise. Below
/// `NOTICEABLE` no document hurts, and short times say little.
fn grows_too_fast(small: Duration, large: Duration, small_len: usize, large_len: usize) -> bool {
    const NOTICEABLE: Duration = Duration::from_millis(400);
    const SHORTEST: Duration = Duration::from_millis(20);
    const NOISE: f64 = 2.25;
    #[expect(clippy::cast_precision_loss, reason = "a ratio of text lengths far below 2^52")]
    let allowed = large_len as f64 / small_len.max(1) as f64 * NOISE;
    large > NOTICEABLE && large.as_secs_f64() / small.max(SHORTEST).as_secs_f64() > allowed
}

/// A document at a fourth of its size and at full size, within the limit and with times that grow as the text does.
/// Only Apple Silicon is held to that, as `LIMIT` says.
fn scales(name: &'static str, small: String, large: String) -> Result<(), String> {
    let (small_len, large_len) = (small.len(), large.len());
    let (small, large): (std::sync::Arc<str>, std::sync::Arc<str>) = (small.into(), large.into());
    let mut small_took = survives(name, &small)?;
    let mut large_took = survives(name, &large)?;
    if !cfg!(target_arch = "aarch64") || cfg!(debug_assertions) {
        return Ok(());
    }
    // Other documents run alongside, so a single slow run proves nothing.
    for _ in 0..2 {
        if !grows_too_fast(small_took, large_took, small_len, large_len) {
            return Ok(());
        }
        small_took = small_took.min(survives(name, &small)?);
        large_took = large_took.min(survives(name, &large)?);
    }
    if grows_too_fast(small_took, large_took, small_len, large_len) {
        return Err(format!(
            "{name}: {small_took:?} for {small_len} bytes, but {large_took:?} for {large_len} bytes"
        ));
    }
    Ok(())
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
            let laid_out = layout(&doc, &Everything(false), None);
            let index = Index::new(&laid_out);
            let bridge = Bridge::new(&doc, &laid_out, &index).unwrap();
            bridge.replace(Shown::new(1)..Shown::new(1), &paste);
            let _ = done.send(());
        });
        assert!(finished.recv_timeout(LIMIT).is_ok(), "{name}");
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
    let cases = std::sync::Mutex::new(cases(1).into_iter().zip(cases(4)).collect::<Vec<_>>());
    let failures = std::sync::Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(2, |n| n.get().min(4));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let Some(((name, small), (_, large))) = cases.lock().unwrap().pop() else {
                        return;
                    };
                    if let Err(failure) = scales(name, small, large) {
                        failures.lock().unwrap().push(failure);
                    }
                }
            });
        }
    });
    let failures = failures.into_inner().unwrap();
    assert!(failures.is_empty(), "{failures:#?}");
}
