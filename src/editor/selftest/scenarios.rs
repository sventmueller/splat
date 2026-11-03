//! Checks that run after the main click-through, on a section of their own appended to the document: every
//! formatting and table command, the dialogs, link clicks, pasting and dropping, exports, a large change, the
//! toolbar groups, the recovery after a crash and the quick reference.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSApplication, NSBitmapImageRep, NSColor,
    NSDocumentController, NSEventModifierFlags, NSMenu, NSModalResponse, NSModalResponseCancel, NSPasteboard,
    NSPasteboardTypePNG, NSPasteboardTypeString, NSPasteboardTypeTIFF, NSSavePanel, NSScreen, NSSegmentedControl,
    NSTextFinderAction, NSTextFinderBarContainer, NSUserInterfaceItemIdentification, NSWindow,
};
use objc2_foundation::{NSArray, NSData, NSRange, NSRunLoop, NSRunLoopCommonModes, NSTimer, NSURL, NSUserDefaults};

use super::super::actions::Action;
use super::{
    Ctx, EditorController, Step, append, applied, at_least, check, check_in_front, click_preview, editable_fields,
    pause, perform, preview_link, select_in_source, settle, source, type_in_preview, type_in_source, when,
};
use crate::document::MdDocument;
use crate::util::{constant, insert_text, ns, storage};

pub(super) const SECTION: &str = "\n\n## Prüfstand\n\nWort **fett** eins zwei drei.\n\n\
    Links: [Netz](https://example.com/pruefstand) #pruefstand [Andere](andere.md) [Fehlt](fehlt.md) \
    [Einstellungen](x-apple.systempreferences:com.apple.Settings) mit Fußnote[^pruef].\n\n\
    ![png](bild.png) ![jpg](foto.jpg) ![svg](zeichnung.svg) ![pdf](seite.pdf) ![tif](scan.tiff)\n\n\
    | A | B |\n|---|---|\n| 1 | 2 |\n\n[^pruef]: Die Prüffußnote.\n";

const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"10\"><rect width=\"20\" \
                   height=\"10\" fill=\"#3a7\"/></svg>";

thread_local! {
    /// How many documents were open before a click on a link to a missing file.
    static MISSING_LINK_DOCUMENTS: Cell<usize> = const { Cell::new(0) };
    /// The source before the step under test, for the step that checks or undoes it.
    static BEFORE: RefCell<String> = const { RefCell::new(String::new()) };
    /// Whether an alert ran modally while `offer_recovered` was called.
    static MODAL_SEEN: Cell<bool> = const { Cell::new(false) };
    /// The outline's rows before the step under test.
    static ROWS: Cell<isize> = const { Cell::new(0) };
}

fn remember(c: &Ctx) {
    BEFORE.with(|b| *b.borrow_mut() = source(&c.e));
}

fn before() -> String {
    BEFORE.with(|b| b.borrow().clone())
}

fn sizes(c: &Ctx) -> String {
    format!("{} bytes, before {}", source(&c.e).len(), before().len())
}

fn undo(c: &Ctx) {
    if let Some(u) = c.e.document().and_then(|d| d.undoManager()) {
        u.undo();
    }
}

/// A step that undoes the last one and checks the text is as before it.
fn undone(name: String) -> Step {
    applied(move |c| {
        undo(c);
        check(&name, source(&c.e) == before(), &sizes(c));
    })
}

fn doc_dir(c: &Ctx) -> PathBuf {
    c.e.doc_dir().unwrap_or_else(|| c.out.clone())
}

fn sheet(c: &Ctx) -> Option<Retained<NSWindow>> {
    c.e.window().and_then(|w| w.attachedSheet())
}

/// Ends the sheet on the editor's window as if its button `response` was clicked.
fn end_sheet(c: &Ctx, response: NSModalResponse) {
    if let (Some(window), Some(sheet)) = (c.e.window(), sheet(c)) {
        window.endSheet_returnCode(&sheet, response);
    }
}

/// A command on the test section and what it must make of it, written out here so that a command doing another one's work
/// cannot agree with itself. `{n}` stands for the first footnote number the text leaves free and `{toc}` for its table of
/// contents, which depend on the text around the section.
pub(super) struct Command {
    pub(super) name: &'static str,
    needle: &'static str,
    action: Action,
    tag: isize,
    /// What the command acts on, and what becomes of it.
    from: &'static str,
    to: &'static str,
    /// What it adds behind the text, which then loses its last line ends.
    at_end: &'static str,
}

const PARAGRAPH: &str = "Wort **fett** eins zwei drei.";
const TABLE: &str = "| A | B |\n|---|---|\n| 1 | 2 |";

const fn on_paragraph(
    name: &'static str,
    needle: &'static str,
    action: Action,
    tag: isize,
    to: &'static str,
) -> Command {
    Command {
        name,
        needle,
        action,
        tag,
        from: PARAGRAPH,
        to,
        at_end: "",
    }
}

const fn on_table(name: &'static str, action: Action, tag: isize, to: &'static str) -> Command {
    Command {
        name,
        needle: "1 | 2",
        action,
        tag,
        from: TABLE,
        to,
        at_end: "",
    }
}

pub(super) const COMMANDS: [Command; 22] = [
    on_paragraph("bold", "eins", Action::Bold, 0, "Wort **fett** **eins** zwei drei."),
    on_paragraph("italic", "eins", Action::Italic, 0, "Wort **fett** *eins* zwei drei."),
    on_paragraph(
        "underline",
        "eins",
        Action::Underline,
        0,
        "Wort **fett** <ins>eins</ins> zwei drei.",
    ),
    on_paragraph(
        "strikethrough",
        "eins",
        Action::Strikethrough,
        0,
        "Wort **fett** ~~eins~~ zwei drei.",
    ),
    on_paragraph(
        "highlight",
        "eins",
        Action::Highlight,
        0,
        "Wort **fett** <mark>eins</mark> zwei drei.",
    ),
    on_paragraph(
        "inline code",
        "eins",
        Action::InlineCode,
        0,
        "Wort **fett** `eins` zwei drei.",
    ),
    on_paragraph(
        "superscript",
        "eins",
        Action::Superscript,
        0,
        "Wort **fett** <sup>eins</sup> zwei drei.",
    ),
    on_paragraph(
        "subscript",
        "eins",
        Action::Subscript,
        0,
        "Wort **fett** <sub>eins</sub> zwei drei.",
    ),
    on_paragraph(
        "a heading",
        "eins",
        Action::Heading,
        2,
        "## Wort **fett** eins zwei drei.",
    ),
    on_paragraph(
        "a bulleted list",
        "eins",
        Action::BulletList,
        0,
        "- Wort **fett** eins zwei drei.",
    ),
    on_paragraph(
        "a numbered list",
        "eins",
        Action::NumberedList,
        0,
        "1. Wort **fett** eins zwei drei.",
    ),
    on_paragraph(
        "a task list",
        "eins",
        Action::TaskList,
        0,
        "- [ ] Wort **fett** eins zwei drei.",
    ),
    on_paragraph("a quote", "eins", Action::Quote, 0, "> Wort **fett** eins zwei drei."),
    on_paragraph(
        "a code block",
        "eins",
        Action::CodeBlock,
        1,
        "```bash\nWort **fett** eins zwei drei.\n```",
    ),
    on_paragraph(
        "an alert box",
        "eins",
        Action::Alert,
        1,
        "> [!TIP]\n> Wort **fett** eins zwei drei.",
    ),
    on_paragraph(
        "removing formatting",
        "**fett**",
        Action::RemoveFormatting,
        0,
        "Wort fett eins zwei drei.",
    ),
    on_paragraph(
        "a flowchart",
        "drei.",
        Action::Diagram,
        0,
        "Wort **fett** eins zwei drei.\n\n```mermaid\nflowchart TD\n    A[Start] --> B{Entscheidung}\n    B -->|Ja| C[Weiter]\n    B -->|Nein| D[Ende]\n```",
    ),
    on_paragraph(
        "a sequence diagram",
        "drei.",
        Action::Diagram,
        1,
        "Wort **fett** eins zwei drei.\n\n```mermaid\nsequenceDiagram\n    Alice->>Bob: Anfrage\n    Bob-->>Alice: Antwort\n```",
    ),
    on_paragraph(
        "a Gantt chart",
        "drei.",
        Action::Diagram,
        2,
        "Wort **fett** eins zwei drei.\n\n```mermaid\ngantt\n    title Projektplan\n    dateFormat YYYY-MM-DD\n    section Phase 1\n    Aufgabe A :a1, 2026-01-05, 10d\n    Aufgabe B :after a1, 5d\n```",
    ),
    on_paragraph(
        "a table of contents",
        "drei.",
        Action::Toc,
        0,
        "Wort **fett** eins zwei drei.\n\n{toc}",
    ),
    Command {
        name: "a footnote",
        needle: "drei.",
        action: Action::Footnote,
        tag: 0,
        from: PARAGRAPH,
        to: "Wort **fett** eins zwei drei.[^{n}]",
        at_end: "\n\n[^{n}]: \n",
    },
    on_paragraph(
        "a rule",
        "drei.",
        Action::Rule,
        0,
        "Wort **fett** eins zwei drei.\n\n---",
    ),
];

pub(super) const TABLE_COMMANDS: [Command; 10] = [
    on_table(
        "row above",
        Action::RowAbove,
        0,
        "| A   | B   |\n|-----|-----|\n|     |     |\n| 1   | 2   |",
    ),
    on_table(
        "row below",
        Action::RowBelow,
        0,
        "| A   | B   |\n|-----|-----|\n| 1   | 2   |\n|     |     |",
    ),
    on_table(
        "column left",
        Action::ColumnLeft,
        0,
        "|     | A   | B   |\n|-----|-----|-----|\n|     | 1   | 2   |",
    ),
    on_table(
        "column right",
        Action::ColumnRight,
        0,
        "| A   |     | B   |\n|-----|-----|-----|\n| 1   |     | 2   |",
    ),
    on_table(
        "deleting a row",
        Action::DeleteRow,
        0,
        "| A   | B   |\n|-----|-----|\n|     |     |",
    ),
    on_table(
        "deleting a column",
        Action::DeleteColumn,
        0,
        "| B   |\n|-----|\n| 2   |",
    ),
    on_table(
        "left alignment",
        Action::TableAlign,
        1,
        "| A   | B   |\n|:----|-----|\n| 1   | 2   |",
    ),
    on_table(
        "centred alignment",
        Action::TableAlign,
        2,
        "|  A  | B   |\n|:---:|-----|\n|  1  | 2   |",
    ),
    on_table(
        "right alignment",
        Action::TableAlign,
        3,
        "|   A | B   |\n|----:|-----|\n|   1 | 2   |",
    ),
    on_table(
        "aligning the table",
        Action::TableFormat,
        0,
        "| A   | B   |\n|-----|-----|\n| 1   | 2   |",
    ),
];

/// The first footnote number `text` leaves free.
fn free_footnote(text: &str) -> usize {
    (1..).find(|n| !text.contains(&format!("[^{n}]"))).unwrap_or(1)
}

/// The table of contents of `text`, whose headings carry no formatting, from nothing but its lines and GitHub's rule for
/// anchors. A single title on top is left out, and so is what lies more than two levels below the highest heading.
fn contents_of(text: &str) -> String {
    let mut fenced = false;
    let mut taken = std::collections::HashMap::new();
    let mut headings: Vec<(usize, String, String)> = Vec::new();
    for line in text.lines() {
        if line.starts_with("```") {
            fenced = !fenced;
        }
        let level = line.chars().take_while(|c| *c == '#').count();
        let Some(title) = line
            .get(level..)
            .and_then(|l| l.strip_prefix(' '))
            .filter(|_| !fenced && level > 0)
        else {
            continue;
        };
        let (title, anchor) = match title.split_once(" {#") {
            Some((title, id)) => (title.to_string(), id.trim_end_matches('}').to_string()),
            None => {
                let slug: String = title
                    .to_lowercase()
                    .chars()
                    .filter(|c| c.is_alphanumeric() || " -_".contains(*c))
                    .map(|c| if c == ' ' { '-' } else { c })
                    .collect();
                let seen: &mut usize = taken.entry(slug.clone()).or_default();
                *seen += 1;
                let anchor = if *seen == 1 {
                    slug
                } else {
                    format!("{slug}-{}", *seen - 1)
                };
                (title.to_string(), anchor)
            }
        };
        headings.push((level, title, anchor));
    }
    if headings.first().is_some_and(|h| h.0 == 1) && headings.iter().filter(|h| h.0 == 1).count() == 1 {
        headings.remove(0);
    }
    let top = headings.iter().map(|h| h.0).min().unwrap_or(1);
    headings
        .iter()
        .filter(|h| h.0 <= top + 2)
        .map(|(level, title, anchor)| format!("{}- [{title}](#{anchor})", "  ".repeat(level - top)))
        .collect::<Vec<_>>()
        .join("\n")
}

impl Command {
    /// What this command makes of `old`, with the last occurrence of what it acts on.
    fn expected(&self, old: &str) -> Option<String> {
        let at = old.rfind(self.from)?;
        let n = free_footnote(old).to_string();
        let to = self.to.replace("{n}", &n).replace("{toc}", &contents_of(old));
        let tail = &old[at + self.from.len()..];
        Some(if self.at_end.is_empty() {
            format!("{}{to}{tail}", &old[..at])
        } else {
            let added = self.at_end.replace("{n}", &n);
            format!("{}{to}{}{added}", &old[..at], tail.trim_end_matches('\n'))
        })
    }

    /// Selects what this command works on and runs it, as the menu does.
    pub(super) fn run(&self, e: &EditorController) {
        select_in_source(e, self.needle);
        perform(e, self.action.spec().selector, self.tag);
    }

    pub(super) fn check_text(&self, e: &EditorController, old: &str) {
        let new = source(e);
        let expected = self.expected(old);
        check(
            &format!("{} writes exactly its text", self.name),
            expected.as_deref() == Some(new.as_str()),
            &format!(
                "expected {:?}, got {:?}",
                tail_from_section(expected.as_deref().unwrap_or("")),
                tail_from_section(&new)
            ),
        );
    }
}

/// What a failure shows of a text: the test section and what follows it, or all of a text without one.
fn tail_from_section(text: &str) -> &str {
    text.rfind("## Prüfstand").map_or(text, |at| &text[at..])
}

/// Runs `command`, checks the text it leaves to the letter, and undoes it.
fn command(command: &'static Command) -> [Step; 2] {
    [
        applied(move |c| {
            remember(c);
            command.run(&c.e);
            command.check_text(&c.e, &before());
        }),
        undone(format!("{} is undone in one step", command.name)),
    ]
}

/// Writes the files the test section links to, in every image format the exports handle differently.
pub(super) fn write_files(dir: &Path) {
    let mut png = Vec::new();
    let _ = image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 30, 30, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
    let mut jpeg = Vec::new();
    let _ = image::RgbImage::from_pixel(2, 2, image::Rgb([30, 30, 200]))
        .write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg);
    let tiff = objc2_app_kit::NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(&png))
        .and_then(|rep| rep.TIFFRepresentation())
        .map(|d| d.to_vec())
        .unwrap_or_default();
    let pdf = super::super::decoder::svg_to_pdf(SVG).unwrap_or_default();
    for (name, bytes) in [
        ("bild.png", png.as_slice()),
        ("foto.jpg", &jpeg),
        ("zeichnung.svg", SVG.as_bytes()),
        ("seite.pdf", &pdf),
        ("scan.tiff", &tiff),
        ("andere.md", b"# Andere\n"),
    ] {
        let _ = std::fs::write(dir.join(name), bytes);
    }
}

/// The menu items of the main menu whose action is `selector`.
fn menu_items(mtm: MainThreadMarker, selector: Sel) -> Vec<Retained<objc2_app_kit::NSMenuItem>> {
    fn walk(menu: &NSMenu, selector: Sel, out: &mut Vec<Retained<objc2_app_kit::NSMenuItem>>) {
        for item in menu.itemArray().iter() {
            if item.action() == Some(selector) {
                out.push(item.clone());
            }
            if let Some(sub) = item.submenu() {
                walk(&sub, selector, out);
            }
        }
    }
    let mut out = Vec::new();
    if let Some(menu) = NSApplication::sharedApplication(mtm).mainMenu() {
        walk(&menu, selector, &mut out);
    }
    out
}

fn documents(mtm: MainThreadMarker) -> Vec<Retained<MdDocument>> {
    NSDocumentController::sharedDocumentController(mtm)
        .documents()
        .iter()
        .filter_map(|d| d.downcast::<MdDocument>().ok())
        .collect()
}

/// Clicks the preview link to `target`, scrolled into view first; whether there was one.
fn click_link(c: &Ctx, target: &str, flags: NSEventModifierFlags) -> bool {
    let Some(at) = preview_link(&c.e, target) else {
        return false;
    };
    c.e.preview().scrollRangeToVisible(NSRange::new(at, 1));
    click_preview(&c.e, at, flags);
    true
}

/// A plain click on the preview link to `target`, handed to the controller as AppKit hands it over: on a window in the
/// background, a real plain click would only bring it to the front.
fn plain_click(c: &Ctx, target: &str) -> Option<usize> {
    let at = preview_link(&c.e, target)?;
    c.e.preview().scrollRangeToVisible(NSRange::new(at, 1));
    let link: &AnyObject = &ns(target);
    c.e.links().link_clicked(c.e.preview(), link, at);
    Some(at)
}

fn cmd_click(c: &Ctx, target: &str) -> bool {
    click_link(c, target, NSEventModifierFlags::Command)
}

/// The save panel of an export, which proposes `name`; it runs in a process of its own and is cancelled here.
fn proposes(c: &Ctx, name: &str) -> bool {
    let proposed = sheet(c)
        .and_then(|s| s.downcast::<NSSavePanel>().ok())
        .map(|p| p.nameFieldStringValue().to_string());
    end_sheet(c, NSModalResponseCancel);
    proposed.is_some_and(|p| p == name)
}

/// Alt text in the test section, and how each image must reach an export: the type and start of the `data:` address in HTML
/// and the extension and start of the file in Word.
const EXPORTED_IMAGES: [(&str, &str, &str, &str, &[u8]); 5] = [
    ("png", "image/png", "iVBORw0KGgo", "png", b"\x89PNG"),
    ("jpg", "image/jpeg", "/9j/", "jpeg", b"\xFF\xD8\xFF"),
    ("svg", "image/svg+xml", "PHN2Zy", "png", b"\x89PNG"),
    ("pdf", "image/png", "iVBORw0KGgo", "png", b"\x89PNG"),
    ("tif", "image/png", "iVBORw0KGgo", "png", b"\x89PNG"),
];

/// The address of the image with `alt` in an exported page.
fn html_image<'a>(page: &'a str, alt: &str) -> Option<&'a str> {
    let at = page.find(&format!("alt=\"{alt}\""))?;
    let open = page[..at].rfind("<img src=\"")? + "<img src=\"".len();
    Some(&page[open..at - 2])
}

/// Exports the test section as HTML and looks at each of its images by itself: the figures of the rest of the document come
/// out as PNG as well, so counting would not notice a missing one.
pub(super) fn check_html_export(e: &EditorController, dir: &Path) {
    use super::super::export::ExportKind;
    let page = dir.join("Export.html");
    let written = e.sharing().export_source().write(ExportKind::Html, "Export", &page);
    let page = std::fs::read_to_string(page).unwrap_or_default();
    check("the HTML export is written", written.is_ok(), &format!("{written:?}"));
    for (alt, kind, start, ..) in EXPORTED_IMAGES {
        let image = html_image(&page, alt);
        check(
            &format!("the HTML export embeds the {alt} image as {kind}"),
            image.is_some_and(|i| i.starts_with(&format!("data:{kind};base64,{start}"))),
            &format!("{:?}", image.map(|i| &i[..i.len().min(40)])),
        );
    }
}

/// Exports the test section as Word and follows each of its images from the paragraph to its file.
pub(super) fn check_word_export(e: &EditorController, dir: &Path) {
    use super::super::export::ExportKind;
    let path = dir.join("Export.docx");
    let written = e.sharing().export_source().write(ExportKind::Word, "Export", &path);
    let entries = crate::markdown::docx::stored_entries(&std::fs::read(path).unwrap_or_default());
    check("the Word export is written", written.is_ok(), &format!("{written:?}"));
    let text = |name: &str| {
        entries
            .iter()
            .find(|(entry, _)| entry == name)
            .map(|(_, content)| String::from_utf8_lossy(content).into_owned())
            .unwrap_or_default()
    };
    let (document, relations) = (text("word/document.xml"), text("word/_rels/document.xml.rels"));
    let media: std::collections::HashSet<&str> = entries
        .iter()
        .map(|(name, _)| name.as_str())
        .filter(|name| name.starts_with("word/media/"))
        .collect();
    check(
        "the Word export has a file for each image",
        media.len() >= EXPORTED_IMAGES.len(),
        &format!("{media:?}"),
    );
    for (alt, .., extension, start) in EXPORTED_IMAGES {
        let file = document.find(&format!("descr=\"{alt}\"")).and_then(|at| {
            let id = document[at..].split("r:embed=\"").nth(1)?.split('"').next()?;
            let relation = relations
                .split("<Relationship ")
                .find(|r| r.contains(&format!("Id=\"{id}\"")))?;
            Some(format!(
                "word/{}",
                relation.split("Target=\"").nth(1)?.split('"').next()?
            ))
        });
        let content = file
            .as_deref()
            .and_then(|f| entries.iter().find(|(name, _)| name == f))
            .map(|(_, content)| content.as_slice());
        check(
            &format!("the Word export carries the {alt} image as {extension}"),
            file.as_deref().is_some_and(|f| f.ends_with(&format!(".{extension}")))
                && content.is_some_and(|c| c.starts_with(start)),
            &format!("{file:?}"),
        );
    }
}

fn export_steps() -> Vec<Step> {
    use super::super::export::ExportKind;
    const SAVE_PANEL: &str = "the save panel to show";
    vec![
        applied(|c| perform(&c.e, Action::ExportHtml.spec().selector, 0)),
        // The panel fills in its name from its own process, a moment after its sheet is there.
        at_least(
            0.8,
            SAVE_PANEL,
            |c| sheet(c).is_some(),
            |c| {
                check(
                    "HTML export proposes the document's name",
                    proposes(c, "QuickReference.html"),
                    "",
                );
                check_html_export(&c.e, &doc_dir(c));
                perform(&c.e, Action::ExportWord.spec().selector, 0);
            },
        ),
        at_least(
            0.8,
            SAVE_PANEL,
            |c| sheet(c).is_some(),
            |c| {
                check(
                    "Word export proposes the document's name",
                    proposes(c, "QuickReference.docx"),
                    "",
                );
                check_word_export(&c.e, &doc_dir(c));
                let failed =
                    c.e.sharing()
                        .export_source()
                        .write(ExportKind::Html, "Export", Path::new("/System/Export.html"));
                if let Err(reason) = &failed {
                    c.e.sharing().report_export_failure(reason);
                }
                check("an export to a place without write access fails", failed.is_err(), "");
            },
        ),
        when(
            SHEET,
            |c| sheet(c).is_some(),
            |c| {
                check("a failed export says so", sheet(c).is_some(), "no alert");
                end_sheet(c, NSAlertFirstButtonReturn);
            },
        ),
    ]
}

const SHEET: &str = "a sheet to show";

fn dialog_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            select_in_source(&c.e, "drei.");
            remember(c);
            perform(&c.e, Action::Table.spec().selector, 0);
        }),
        when(
            SHEET,
            |c| sheet(c).is_some(),
            |c| {
                check("the table dialog opens", sheet(c).is_some(), "no sheet");
                end_sheet(c, NSAlertFirstButtonReturn);
            },
        ),
        when(
            "the new table to reach the text",
            |c| table_rows(&source(&c.e)) > table_rows(&before()),
            |c| {
                check(
                    "the table dialog inserts a table of three by three",
                    table_rows(&source(&c.e)) == table_rows(&before()) + 4,
                    &sizes(c),
                );
            },
        ),
        undone("inserting a table is undone in one step".into()),
        applied(|c| {
            select_in_source(&c.e, "zwei");
            remember(c);
            perform(&c.e, Action::Link.spec().selector, 0);
        }),
        when(
            SHEET,
            |c| sheet(c).is_some(),
            |c| {
                let mut fields = Vec::new();
                if let Some(view) = sheet(c).and_then(|s| s.contentView()) {
                    editable_fields(&view, &mut fields);
                }
                check("the link dialog asks for address and text", fields.len() == 2, "");
                if let Some(address) = fields.first() {
                    address.setStringValue(&ns("https://splat.example"));
                }
                end_sheet(c, NSAlertFirstButtonReturn);
            },
        ),
        when(
            "the link to reach the text",
            |c| source(&c.e).contains("[zwei](https://splat.example)"),
            |c| {
                check(
                    "the link dialog links the selection",
                    source(&c.e).contains("[zwei](https://splat.example)"),
                    &sizes(c),
                );
            },
        ),
        undone("inserting a link is undone in one step".into()),
        applied(|c| {
            remember(c);
            perform(&c.e, Action::Image.spec().selector, 0);
        }),
        when(
            SHEET,
            |c| sheet(c).is_some(),
            |c| {
                check("the image dialog opens", sheet(c).is_some(), "no open panel");
                end_sheet(c, NSModalResponseCancel);
            },
        ),
        at_least(
            0.5,
            "the image dialog to close",
            |c| sheet(c).is_none(),
            |c| {
                check(
                    "a cancelled image dialog leaves the text",
                    source(&c.e) == before(),
                    &sizes(c),
                );
            },
        ),
    ]
}

fn table_rows(text: &str) -> usize {
    text.lines().filter(|l| l.starts_with('|')).count()
}

/// Whether the preview shows the test section's footnote.
fn footnote_in_view(c: &Ctx) -> bool {
    let preview = c.e.preview();
    let text = storage(preview).string().to_string();
    text.find("Die Prüffußnote").is_some_and(|at| {
        let rect = super::super::scroll::char_rect(preview, text[..at].encode_utf16().count());
        let shown = preview.visibleRect();
        rect.origin.y >= shown.origin.y && rect.origin.y <= shown.origin.y + shown.size.height
    })
}

/// What the shared find pasteboard asks for and whether the find bar shows.
fn find_bar(c: &Ctx) -> (Option<String>, bool) {
    // SAFETY: Reading AppKit's constant for the shared find pasteboard.
    let board = NSPasteboard::pasteboardWithName(unsafe { objc2_app_kit::NSPasteboardNameFind });
    let searched = board
        .stringForType(constant!(NSPasteboardTypeString))
        .map(|s| s.to_string());
    (
        searched,
        c.e.source().enclosingScrollView().is_some_and(|s| s.isFindBarVisible()),
    )
}

fn link_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            let clicked = plain_click(c, "splat-footnote:pruef");
            check("the footnote reference is a link", clicked.is_some(), "");
        }),
        when("the footnote to come into view", footnote_in_view, |c| {
            check(
                "a click on a footnote reference shows the footnote",
                footnote_in_view(c),
                "",
            );
            plain_click(c, "https://example.com/pruefstand");
        }),
        applied(|c| {
            let at = preview_link(&c.e, "https://example.com/pruefstand");
            let caret = c.e.preview().selectedRange().location;
            check(
                "a plain click on a web link only places the caret",
                at == Some(caret),
                &format!("{at:?} vs {caret}"),
            );
            check("tags are links", cmd_click(c, "splat-tag:#pruefstand"), "");
        }),
        // AppKit reports a click on a link once a double click can no longer follow.
        when(
            "the ⌘-click on the tag to open the find bar",
            |c| {
                let (searched, bar) = find_bar(c);
                bar && searched.as_deref() == Some("#pruefstand")
            },
            |c| {
                let (searched, bar) = find_bar(c);
                check(
                    "⌘-click on a tag searches for it in the source",
                    bar && searched.as_deref() == Some("#pruefstand"),
                    &format!("{searched:?}, find bar {bar}"),
                );
                let item = objc2_app_kit::NSMenuItem::new(c.e.mtm());
                item.setTag(NSTextFinderAction::HideFindInterface.0);
                // SAFETY: The sender is a menu item whose tag names the action, as the method expects.
                unsafe { c.e.source().performTextFinderAction(Some(&item)) };
                let count = documents(c.e.mtm()).len();
                let opened = cmd_click(c, "andere.md");
                check("links to other documents are links", opened && count > 0, "");
            },
        ),
        when(
            "the linked document to open",
            |c| {
                documents(c.e.mtm())
                    .iter()
                    .any(|d| d.displayName().to_string().starts_with("andere"))
            },
            |c| {
                let other = documents(c.e.mtm())
                    .into_iter()
                    .find(|d| d.displayName().to_string().starts_with("andere"));
                check(
                    "⌘-click on a link to a Markdown file opens it in Splat",
                    other.as_ref().is_some_and(|d| d.text() == "# Andere\n"),
                    "",
                );
                if let Some(d) = other {
                    d.close();
                }
            },
        ),
        at_least(
            0.5,
            "the linked document to close",
            |c| {
                !documents(c.e.mtm())
                    .iter()
                    .any(|d| d.displayName().to_string().starts_with("andere"))
            },
            |c| {
                if let Some(window) = c.e.window() {
                    window.makeKeyAndOrderFront(None);
                }
                let count = documents(c.e.mtm()).len();
                cmd_click(c, "fehlt.md");
                MISSING_LINK_DOCUMENTS.set(count);
            },
        ),
        pause(
            0.7,
            "a click that opens nothing gives no signal when it has arrived",
            |c| {
                check(
                    "⌘-click on a missing file opens nothing",
                    documents(c.e.mtm()).len() == MISSING_LINK_DOCUMENTS.get() && sheet(c).is_none(),
                    "",
                );
                check(
                    "links to other apps are links",
                    cmd_click(c, "x-apple.systempreferences:com.apple.Settings"),
                    "",
                );
            },
        ),
        when(
            SHEET,
            |c| sheet(c).is_some(),
            |c| {
                check("a link to another app asks first", sheet(c).is_some(), "no alert");
                end_sheet(c, NSAlertFirstButtonReturn);
            },
        ),
    ]
}

fn picture(c: &Ctx) -> Vec<u8> {
    std::fs::read(doc_dir(c).join("bild.png")).unwrap_or_default()
}

/// The alert on the window of the unsaved document.
fn untitled_alert(c: &Ctx) -> Option<Retained<NSWindow>> {
    documents(c.e.mtm())
        .into_iter()
        .find(|d| d.fileURL().is_none())
        .and_then(|d| d.editor())
        .and_then(|e| e.window())
        .and_then(|w| w.attachedSheet())
}

fn paste_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            select_in_source(&c.e, "drei.");
            remember(c);
            let board = NSPasteboard::pasteboardWithUniqueName();
            board.clearContents();
            board.setData_forType(Some(&NSData::with_bytes(&picture(c))), constant!(NSPasteboardTypePNG));
            let pasted = c.e.editing().paste_image(&board);
            let pictures = std::fs::read_dir(doc_dir(c))
                .into_iter()
                .flatten()
                .flatten()
                .filter(|f| f.file_name().to_string_lossy().starts_with("Bild "))
                .count();
            check(
                "a pasted picture becomes a file beside the document and an image link",
                pasted && pictures == 1 && source(&c.e).contains("![Bild "),
                &format!("pasted {pasted}, {pictures} files"),
            );
        }),
        undone("pasting a picture is undone in one step".into()),
        applied(|c| {
            let board = NSPasteboard::pasteboardWithUniqueName();
            board.clearContents();
            let tiff = std::fs::read(doc_dir(c).join("scan.tiff")).unwrap_or_default();
            board.setData_forType(Some(&NSData::with_bytes(&tiff)), constant!(NSPasteboardTypeTIFF));
            remember(c);
            let pasted = c.e.editing().paste_image(&board);
            check(
                "a pasted TIFF picture is stored as PNG",
                pasted && source(&c.e).matches("![Bild ").count() == 1,
                &format!("pasted {pasted}"),
            );
        }),
        undone("pasting a TIFF picture is undone in one step".into()),
        applied(|c| {
            let board = NSPasteboard::pasteboardWithUniqueName();
            board.clearContents();
            board.setString_forType(&ns("Text"), constant!(NSPasteboardTypeString));
            board.setData_forType(Some(&NSData::with_bytes(b"x")), constant!(NSPasteboardTypePNG));
            check(
                "text on the pasteboard wins over its picture",
                !c.e.editing().paste_image(&board),
                "",
            );
            let untitled = NSDocumentController::sharedDocumentController(c.e.mtm())
                .openUntitledDocumentAndDisplay_error(true)
                .ok()
                .and_then(|d| d.downcast::<MdDocument>().ok());
            let board = NSPasteboard::pasteboardWithUniqueName();
            board.clearContents();
            board.setData_forType(Some(&NSData::with_bytes(&picture(c))), constant!(NSPasteboardTypePNG));
            let asked = untitled
                .as_ref()
                .and_then(|d| d.editor())
                .map(|e| e.editing().paste_image(&board));
            check(
                "a picture pasted into an unsaved document is taken",
                asked == Some(true),
                "",
            );
        }),
        when(
            "the unsaved document to ask for a save",
            |c| untitled_alert(c).is_some(),
            |c| {
                let untitled = documents(c.e.mtm()).into_iter().find(|d| d.fileURL().is_none());
                let window = untitled.as_ref().and_then(|d| d.editor()).and_then(|e| e.window());
                let alert = untitled_alert(c);
                check(
                    "an unsaved document asks to be saved before a picture is pasted",
                    alert.is_some(),
                    "",
                );
                if let (Some(w), Some(a)) = (&window, &alert) {
                    w.endSheet_returnCode(a, NSAlertSecondButtonReturn);
                }
                if let Some(d) = untitled {
                    d.close();
                }
            },
        ),
        at_least(
            0.5,
            "the unsaved document to close",
            |c| !documents(c.e.mtm()).iter().any(|d| d.fileURL().is_none()),
            |c| {
                if let Some(window) = c.e.window() {
                    window.makeKeyAndOrderFront(None);
                }
                select_in_source(&c.e, "drei.");
                remember(c);
                let dir = doc_dir(c);
                let urls: Vec<Retained<ProtocolObject<dyn objc2_app_kit::NSPasteboardWriting>>> = [
                    NSURL::fileURLWithPath(&ns(dir.join("bild.png").to_str().unwrap_or_default())),
                    NSURL::fileURLWithPath(&ns(dir.join("andere.md").to_str().unwrap_or_default())),
                ]
                .into_iter()
                .chain(NSURL::URLWithString(&ns("https://example.org/seite")))
                .map(ProtocolObject::from_retained)
                .collect();
                let board = NSPasteboard::pasteboardWithUniqueName();
                board.clearContents();
                board.writeObjects(&NSArray::from_retained_slice(&urls));
                let at = c.e.source().selectedRange().location;
                let dropped = c.e.editing().handle_source_drop(&board, at);
                let s = source(&c.e);
                check(
                    "dropped files and addresses become images and links",
                    dropped
                        && s.contains("(bild.png)")
                        && s.contains("(andere.md)")
                        && s.contains("https://example.org/seite"),
                    &format!("dropped {dropped}"),
                );
            },
        ),
        undone("a drop is undone in one step".into()),
    ]
}

fn table_steps() -> Vec<Step> {
    let mut steps = Vec::new();
    for case in &TABLE_COMMANDS {
        steps.extend(command(case));
    }
    for (name, backwards) in [("tab", false), ("backtab", true)] {
        steps.push(applied(move |c| {
            select_in_source(&c.e, "1 | 2");
            c.e.source()
                .setSelectedRange(NSRange::new(c.e.source().selectedRange().location, 0));
            remember(c);
            let command = if backwards {
                sel!(insertBacktab:)
            } else {
                sel!(insertTab:)
            };
            let from = c.e.source().selectedRange();
            let handled = c.e.editing().source_command(command);
            let to = c.e.source().selectedRange();
            check(
                &format!("{name} in a table moves between its cells"),
                handled && to.location != from.location,
                &format!("{from:?} -> {to:?}"),
            );
        }));
        steps.push(applied(|c| {
            if source(&c.e) != before() {
                undo(c);
            }
        }));
    }
    steps
}

fn view_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            select_in_source(&c.e, "fett");
            // Validated by hand: AppKit asks the controller only while the app is in front.
            let validate = |selector: Sel| {
                let items = menu_items(c.e.mtm(), selector);
                for item in &items {
                    c.e.editing().validate(item.action(), item.tag(), Some(item));
                }
                items
            };
            let bold = validate(Action::Bold.spec().selector);
            check(
                "the menu checks bold where the caret is in bold text",
                !bold.is_empty() && bold.iter().all(|i| i.state() == objc2_app_kit::NSControlStateValueOn),
                &format!("{} items", bold.len()),
            );
            let preview = validate(Action::TogglePreview.spec().selector);
            check(
                "the menu offers to hide the shown preview",
                preview.iter().any(|i| i.title().to_string() == "Vorschau ausblenden"),
                "",
            );
            perform(&c.e, Action::ToggleOutline.spec().selector, 0);
        }),
        when(
            "the outline to open",
            |c| c.e.panes().outline_visible(),
            |c| {
                check("the outline opens", c.e.panes().outline_visible(), "");
                perform(&c.e, Action::ToggleOutline.spec().selector, 0);
            },
        ),
        when(
            "the outline to close",
            |c| !c.e.panes().outline_visible(),
            |c| {
                check("the outline closes", !c.e.panes().outline_visible(), "");
                let on = super::super::env::typography_on();
                perform(&c.e, Action::Typography.spec().selector, 0);
                check(
                    "typographic punctuation switches",
                    super::super::env::typography_on() != on,
                    "",
                );
                perform(&c.e, Action::Typography.spec().selector, 0);
                perform(&c.e, Action::ZoomIn.spec().selector, 0);
                let zoom = c.e.model().read(|st| st.zoom());
                check("zooming in enlarges", zoom > 1.0, &format!("{zoom}"));
                perform(&c.e, Action::ZoomOut.spec().selector, 0);
                perform(&c.e, Action::ZoomOut.spec().selector, 0);
                let zoom = c.e.model().read(|st| st.zoom());
                check("zooming out shrinks", zoom < 1.0, &format!("{zoom}"));
                perform(&c.e, Action::ZoomReset.spec().selector, 0);
                let zoom = c.e.model().read(|st| st.zoom());
                check(
                    "the actual size comes back",
                    (zoom - 1.0).abs() < 1e-9,
                    &format!("{zoom}"),
                );
            },
        ),
        settle(|c| {
            select_in_source(&c.e, "drei.");
            remember(c);
            // Typing in the source schedules a parse; typing in the preview before it lands cannot be mapped.
            type_in_source(&c.e, "Q");
            c.e.window().map(|w| w.makeFirstResponder(Some(c.e.preview())));
            type_in_preview(&c.e, "Zzz");
        }),
        settle(|c| {
            let preview = storage(c.e.preview()).string().to_string();
            check(
                "typing in the preview while the source is parsed shows the source again",
                source(&c.e).contains("zwei Q") && !preview.contains("Zzz") && !source(&c.e).contains("Zzz"),
                "",
            );
        }),
        applied(|c| {
            undo(c);
            check("the typing is undone", source(&c.e) == before(), &sizes(c));
        }),
    ]
}

fn toolbar_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            let mtm = c.e.mtm();
            let share = ProtocolObject::from_ref(&*c.e);
            let allowed = super::super::toolbar::allowed_identifiers();
            let missing: Vec<String> = allowed
                .iter()
                .map(|id| id.to_string())
                .filter(|id| !id.starts_with("NSToolbar"))
                .filter(|id| super::super::toolbar::make(mtm, id, share, &c.e).is_none())
                .collect();
            check(
                "every item the toolbar offers can be made",
                missing.is_empty(),
                &format!("{missing:?}"),
            );
            select_in_source(&c.e, "eins");
            remember(c);
            let bold = Action::Bold.spec().selector;
            let control = c.e.window().and_then(|w| w.toolbar()).and_then(|toolbar| {
                toolbar.items().iter().find_map(|item| {
                    let control = item.view()?.downcast::<NSSegmentedControl>().ok()?;
                    let group = NSUserInterfaceItemIdentification::identifier(&*control)?.to_string();
                    let segment = (0..control.segmentCount()).find(|&s| {
                        super::super::toolbar::segment_item(mtm, &group, crate::util::to_usize(s), share, &c.e)
                            .is_some_and(|i| i.action() == Some(bold))
                    })?;
                    Some((control, segment))
                })
            });
            check("the bold button sits in a toolbar group", control.is_some(), "");
            // Its segments are momentary and forget a selection made outside a click.
            if let Some((control, segment)) = control {
                let mode = control.trackingMode();
                control.setTrackingMode(objc2_app_kit::NSSegmentSwitchTracking::SelectOne);
                control.setSelectedSegment(segment);
                super::super::toolbar::segment_clicked(&c.e, Some(&control));
                control.setSelectedSegment(-1);
                control.setTrackingMode(mode);
            }
            check(
                "a toolbar group runs the command of its segment",
                source(&c.e).contains("**eins**"),
                "",
            );
        }),
        undone("a toolbar command is undone in one step".into()),
    ]
}

fn large_change_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            remember(c);
            let text: String = (1..=1000)
                .map(|n| format!("\n\nAbsatz {n} des großen Nachtrags."))
                .collect();
            append(&c.e, &text);
        }),
        settle(|c| {
            let pending = c.e.model().read(|st| st.blocks().pending());
            check("a large change renders the part around the view first", pending, "");
            let end = storage(c.e.preview()).length();
            c.e.preview().scrollRangeToVisible(NSRange::new(end, 0));
        }),
        when(
            "the blocks scrolled into view to render",
            |c| c.e.model().read(|st| st.blocks().placeholders.last().copied()) == Some(false),
            |c| {
                let last = c.e.model().read(|st| st.blocks().placeholders.last().copied());
                check(
                    "blocks scrolled into view are rendered",
                    last == Some(false),
                    &format!("{last:?}"),
                );
            },
        ),
        undone("the large change is undone in one step".into()),
    ]
}

/// A PNG of `side` × `side` pixels.
fn png_of_side(side: u32) -> Vec<u8> {
    let mut png = Vec::new();
    let _ = image::RgbaImage::from_pixel(side, side, image::Rgba([30, 160, 60, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
    png
}

/// The width of the image in `file`, once the preview has it.
fn shown_width(c: &Ctx, file: &str) -> Option<f64> {
    let path = doc_dir(c).join(file);
    c.e.pictures().store().borrow().shown_size(&path).map(|s| s.width)
}

fn image_file_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            let _ = std::fs::write(doc_dir(c).join("bild.png"), png_of_side(7));
        }),
        when(
            "the replaced image to show",
            |c| shown_width(c, "bild.png") == Some(7.0),
            |c| {
                let width = shown_width(c, "bild.png");
                check(
                    "an image replaced on disk shows without reopening",
                    width == Some(7.0),
                    &format!("{width:?}"),
                );
                // Paper does not wait for a load under way.
                let _ = std::fs::write(doc_dir(c).join("druck.png"), png_of_side(3));
                let dir = doc_dir(c);
                let mut images = c.e.pictures().store().borrow_mut();
                let loading = images.file("druck.png", Some(&dir), false, Some(&c.e));
                let printed = images.file("druck.png", Some(&dir), false, None);
                check(
                    "printing does not leave out an image that is still loading",
                    loading.is_none() && printed.is_some(),
                    &format!("preview {}, paper {}", loading.is_some(), printed.is_some()),
                );
            },
        ),
        applied(|c| {
            remember(c);
            append(&c.e, "\n\n![spät](spaeter.png)\n");
        }),
        settle(|c| {
            check(
                "a missing image counts as failed",
                c.e.pictures().store().borrow().shown(&doc_dir(c).join("spaeter.png")) == Some(false),
                "",
            );
            let _ = std::fs::write(doc_dir(c).join("spaeter.png"), png_of_side(5));
        }),
        when(
            "the late image to show",
            |c| shown_width(c, "spaeter.png") == Some(5.0),
            |c| {
                let width = shown_width(c, "spaeter.png");
                check(
                    "an image that appears later shows",
                    width == Some(5.0),
                    &format!("{width:?}"),
                );
                let _ = std::fs::write(doc_dir(c).join("bild.png"), png_of_side(2));
            },
        ),
        undone("the image line is undone in one step".into()),
        when(
            "the image of the next checks to show",
            |c| shown_width(c, "bild.png") == Some(2.0),
            |_| {},
        ),
    ]
}

fn memory_steps() -> Vec<Step> {
    vec![applied(|c| {
        super::super::images::memory_pressure_changed(true);
        check(
            "while memory is short, images decode smaller",
            super::super::images::allowed_side() == 640,
            "",
        );
        let small = super::super::decoder::decode(&png_of_side(700), super::super::images::allowed_side());
        check(
            "the helpers start again after they were ended for memory",
            matches!(small, Some(super::super::safe_image::Decoded::Rgba { width: 640, .. })),
            "",
        );
        super::super::images::memory_pressure_changed(false);
        check(
            "once memory is back, images decode at full size again",
            super::super::images::allowed_side() == super::super::safe_image::SHOWN_SIDE,
            "",
        );
        check("the shown image survives", shown_width(c, "bild.png").is_some(), "");
    })]
}

fn old_encoding_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            let path = doc_dir(c).join("alt.md");
            let mut utf16 = vec![0xFF, 0xFE];
            utf16.extend("# Grüße\n".encode_utf16().flat_map(u16::to_le_bytes));
            let _ = std::fs::write(&path, utf16);
            let done = RcBlock::new(
                |_: *mut objc2_app_kit::NSDocument, _: objc2::runtime::Bool, _: *mut objc2_foundation::NSError| {},
            );
            NSDocumentController::sharedDocumentController(c.e.mtm())
                .openDocumentWithContentsOfURL_display_completionHandler(
                    &NSURL::fileURLWithPath(&ns(path.to_str().unwrap_or_default())),
                    true,
                    &done,
                );
        }),
        when(
            "the file in UTF-16 to open",
            |c| {
                documents(c.e.mtm())
                    .iter()
                    .any(|d| d.displayName().to_string().starts_with("alt"))
            },
            |c| {
                let other = documents(c.e.mtm())
                    .into_iter()
                    .find(|d| d.displayName().to_string().starts_with("alt"));
                check(
                    "a file in UTF-16 opens with its umlauts",
                    other.as_ref().is_some_and(|d| d.text() == "# Grüße\n"),
                    "",
                );
                if let Some(d) = other {
                    d.close();
                }
                if let Some(window) = c.e.window() {
                    window.makeKeyAndOrderFront(None);
                }
            },
        ),
    ]
}

fn copies_of(c: &Ctx) -> Vec<PathBuf> {
    let folder = c.out.join("Unsaved").join(std::process::id().to_string());
    std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .collect()
}

fn unsaved_copy_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            remember(c);
            type_in_source(&c.e, "Q");
            super::super::shadow::copy_unsaved();
        }),
        when(
            "the copy of the unsaved text to be written",
            |c| {
                copies_of(c)
                    .iter()
                    .any(|f| std::fs::read_to_string(f).is_ok_and(|t| t == source(&c.e)))
            },
            |c| {
                check(
                    "unsaved text has a copy that outlives a crash",
                    copies_of(c)
                        .iter()
                        .any(|f| std::fs::read_to_string(f).is_ok_and(|t| t == source(&c.e))),
                    "",
                );
                // What a run that crashed leaves behind.
                let ended = c.out.join("Unsaved/999999");
                let _ = std::fs::create_dir_all(&ended);
                let _ = std::fs::write(ended.join("Notiz.md"), "Text vor dem Absturz");
                super::super::shadow::adopt_left_behind();
                check(
                    "the next start takes over the copies of a crashed run",
                    std::fs::read_to_string(c.out.join("Recovery/999999-Notiz.md"))
                        .is_ok_and(|t| t == "Text vor dem Absturz")
                        && !ended.exists(),
                    "",
                );
                check("a running app keeps its own copies", !copies_of(c).is_empty(), "");
                undo(c);
                if let Some(d) = c.e.document() {
                    d.updateChangeCount(objc2_app_kit::NSDocumentChangeType::ChangeCleared);
                }
                super::super::shadow::copy_unsaved();
            },
        ),
        when(
            "the copy of the saved text to go",
            |c| copies_of(c).is_empty(),
            |c| {
                check(
                    "a saved document needs no copy",
                    copies_of(c).is_empty(),
                    &format!("{:?}", copies_of(c)),
                );
                check("undoing leaves the text of before", source(&c.e) == before(), &sizes(c));
                if let Some(d) = c.e.document() {
                    d.updateChangeCount(objc2_app_kit::NSDocumentChangeType::ChangeDone);
                }
            },
        ),
    ]
}

/// The file changes while a formula's editor is open: its offsets mean nothing then, and nothing of it may reach the text.
fn reload_under_an_open_editor_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            remember(c);
            append(&c.e, "\n\nFormel $a+b$ steht hier.\n");
        }),
        settle(|c| {
            let at = source(&c.e).find("a+b").unwrap_or(0);
            super::set_source_caret(&c.e, at);
            c.e.atom_editor().edit_at_selection();
            check(
                "the formula's editor opens for the reload check",
                c.e.atom_editor().is_open(),
                "no editor",
            );
        }),
        // Its popover is still fading in when it counts as open.
        at_least(
            0.3,
            "the formula's editor to open",
            |c| c.e.atom_editor().is_open(),
            |c| {
                let replaced = "# Von außen geändert\n\nGanz anderer Text.\n";
                c.e.updater().replace_all_text(replaced);
                check(
                    "a reload closes the formula's editor",
                    !c.e.atom_editor().is_open(),
                    "still open",
                );
            },
        ),
        when(
            "the formula's editor to close and the reloaded text to settle",
            |c| !c.e.atom_editor().is_open() && super::settled(&c.e),
            |c| {
                check(
                    "nothing of a closed editor reaches the reloaded text",
                    source(&c.e) == "# Von außen geändert\n\nGanz anderer Text.\n",
                    &source(&c.e),
                );
                c.e.updater().replace_all_text(&before());
            },
        ),
        settle(|_| {}),
    ]
}

const FILES_LOOKED_AT: &str =
    "files are looked at every two seconds, and a change that must not arrive gives no signal";

/// What is saved is exactly what the source holds, and unsaved text outlives a change of its file by another program.
fn saving_and_unsaved_text_steps() -> Vec<Step> {
    let path = |c: &Ctx| c.e.document().and_then(|d| d.fileURL()?.path()).map(|p| p.to_string());
    vec![
        applied(|c| {
            if let Some(d) = c.e.document() {
                unsafe { d.saveDocument(None) };
            }
        }),
        when(
            "the document to be saved",
            |c| c.e.document().is_some_and(|d| !d.isDocumentEdited()),
            move |c| {
                let saved = path(c).and_then(|p| std::fs::read(p).ok()).unwrap_or_default();
                check(
                    "a saved file holds exactly the text of the source",
                    saved == c.e.exact_text().into_bytes(),
                    &format!("{} bytes, {} in the source", saved.len(), c.e.exact_text().len()),
                );
                remember(c);
                type_in_source(&c.e, "Ungespeichert");
                if let Some(p) = path(c) {
                    let _ = std::fs::write(p, "Fremder Text\n");
                }
            },
        ),
        pause(3.0, FILES_LOOKED_AT, |c| {
            check(
                "unsaved text outlives a change of its file by another program",
                source(&c.e).contains("Ungespeichert") && !source(&c.e).contains("Fremder Text"),
                &sizes(c),
            );
        }),
        applied(move |c| {
            if let Some(p) = path(c) {
                let _ = std::fs::write(p, before());
            }
        }),
        pause(3.0, FILES_LOOKED_AT, |c| {
            undo(c);
            check(
                "the unsaved text can be taken back",
                source(&c.e) == before(),
                &sizes(c),
            );
        }),
        // AppKit autosaves every 15 to 25 seconds; one that falls in here finds the file changed and asks in a sheet.
        pause(1.5, "an autosave gives no signal when it has asked", |c| {
            end_sheet(c, NSAlertFirstButtonReturn);
        }),
        when("the question of an autosave to close", |c| sheet(c).is_none(), |_| {}),
    ]
}

/// Without a display an animated collapse only ends at the item's next toggle, and then undoes that toggle.
fn show_outline(c: &Ctx, shown: bool) {
    c.e.panes().set_outline_collapsed(!shown);
    c.e.panes().show_headings_if_open();
}

/// Like `show_outline`, for the preview.
fn show_preview(c: &Ctx, shown: bool) {
    c.e.panes().set_preview_collapsed(!shown);
    c.e.updater().update_now();
}

/// The outline stays in the window while the preview is hidden, and so must follow the text.
fn outline_without_preview_steps() -> Vec<Step> {
    let rows = |c: &Ctx| c.e.panes().outline_rows().unwrap_or(0);
    vec![
        applied(|c| {
            remember(c);
            show_preview(c, false);
        }),
        when(
            "the preview to hide",
            |c| !c.e.panes().preview_visible(),
            |c| {
                show_outline(c, true);
            },
        ),
        when(
            "the outline to show",
            |c| c.e.panes().outline_visible(),
            move |c| {
                ROWS.with(|r| r.set(rows(c)));
                append(&c.e, "\n\n# Neu in der Gliederung\n");
            },
        ),
        when(
            "the outline to follow the text",
            move |c| rows(c) > ROWS.with(Cell::get),
            move |c| {
                check(
                    "the outline follows the text while the preview is hidden",
                    rows(c) == ROWS.with(Cell::get) + 1,
                    &format!("{} rows, before {}", rows(c), ROWS.with(Cell::get)),
                );
                undo(c);
                show_outline(c, false);
            },
        ),
        when(
            "the outline to hide",
            |c| !c.e.panes().outline_visible(),
            |c| show_preview(c, true),
        ),
        when(
            "the preview to show and settle",
            |c| c.e.panes().preview_visible() && super::settled(&c.e),
            |c| check("the outline check leaves the text", source(&c.e) == before(), &sizes(c)),
        ),
    ]
}

/// Captures the preview's part of the screen: only the window server shows what was really redrawn, and a capture of the
/// window itself changes its size along the way.
fn capture_preview(c: &Ctx, path: &Path) {
    let preview = c.e.preview();
    let window = c.e.window().expect("a window");
    let shown = window.convertRectToScreen(preview.convertRect_toView(preview.visibleRect(), None));
    let main = NSScreen::screens(MainThreadMarker::new().expect("the main thread"))
        .firstObject()
        .expect("a screen");
    let top = main.frame().size.height - shown.origin.y - shown.size.height;
    let region = format!("{},{top},{},{}", shown.origin.x, shown.size.width, shown.size.height);
    let captured = std::process::Command::new("screencapture")
        .args(["-x", "-R", &region])
        .arg(path)
        .status();
    check(
        "the preview can be captured",
        captured.is_ok_and(|s| s.success()),
        &region,
    );
}

/// The colors of a capture, without the right edge, where a scroller fades.
fn colors_in(path: &Path) -> Vec<Retained<NSColor>> {
    let Some(image) = std::fs::read(path)
        .ok()
        .and_then(|png| NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(&png)))
    else {
        return Vec::new();
    };
    let (wide, high) = (image.pixelsWide(), image.pixelsHigh());
    (0..wide * 9 / 10)
        .step_by(4)
        .flat_map(|x| (0..high).step_by(4).map(move |y| (x, y)))
        .filter_map(|(x, y)| image.colorAtX_y(x, y))
        .collect()
}

fn marks_on_screen_steps() -> Vec<Step> {
    const REDRAWN: &str = "only the screen shows a redraw, and it gives no signal when that has happened";
    vec![
        applied(|c| {
            c.e.window().expect("a window").makeFirstResponder(Some(c.e.source()));
            c.e.source().setSelectedRange(NSRange::new(0, 0));
            c.e.source().scrollRangeToVisible(NSRange::new(0, 0));
        }),
        pause(1.0, REDRAWN, |c| {
            capture_preview(c, &c.out.join("marks-before.png"));
            c.e.source().setSelectedRange(NSRange::new(0, 300));
        }),
        pause(0.5, REDRAWN, |c| {
            capture_preview(c, &c.out.join("marks-after.png"));
            let before = colors_in(&c.out.join("marks-before.png"));
            let after = colors_in(&c.out.join("marks-after.png"));
            let changed = before.iter().zip(&after).filter(|(a, b)| a != b).count();
            check_in_front(
                c.e.mtm(),
                "what is selected in the source gets marked on screen in the preview",
                changed > 100 && before.len() == after.len(),
                &format!("{changed} of {} and {} points changed", before.len(), after.len()),
            );
            c.e.source().setSelectedRange(NSRange::new(0, 0));
        }),
    ]
}

fn huge_document_steps() -> Vec<Step> {
    const NOTICE: &str = "Dieses Dokument ist zu groß für die Vorschau. Der Text lässt sich weiter bearbeiten.";
    vec![
        applied(|c| {
            remember(c);
            let huge = "| Zeile |\n".repeat(3 << 20);
            append(&c.e, &huge);
        }),
        settle(|c| {
            let preview = storage(c.e.preview()).string().to_string();
            check(
                "a huge document says why it has no preview",
                preview == NOTICE,
                &preview,
            );
            let before = source(&c.e).len();
            c.e.preview().setSelectedRange(NSRange::new(0, 0));
            insert_text(c.e.preview(), &ns("x"), NSRange::new(0, 0));
            check(
                "the preview of a huge document takes no input",
                !c.e.preview().isEditable()
                    && !c.e.updater().preview_takes_input()
                    && source(&c.e).len() == before
                    && storage(c.e.preview()).string().to_string() == NOTICE,
                "",
            );
        }),
        undone("the huge text is undone in one step".into()),
        settle(|c| {
            let preview = storage(c.e.preview()).string().to_string();
            check(
                "the preview comes back with the smaller text",
                preview.contains("Prüffußnote") && !preview.contains(NOTICE),
                "",
            );
            check(
                "the preview takes input again with the smaller text",
                c.e.preview().isEditable() && c.e.updater().preview_takes_input(),
                "",
            );
        }),
    ]
}

fn app_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            let files = super::super::emergency_save();
            let copy = files.iter().find_map(|f| {
                let text = std::fs::read_to_string(f).ok()?;
                (text == source(&c.e)).then_some(())
            });
            check(
                "a crash leaves a copy of every open document",
                copy.is_some(),
                &format!("{files:?}"),
            );
            let mtm = c.e.mtm();
            MODAL_SEEN.with(|m| m.set(false));
            let stop = RcBlock::new(move |_: std::ptr::NonNull<NSTimer>| {
                let app = NSApplication::sharedApplication(mtm);
                MODAL_SEEN.with(|m| m.set(app.modalWindow().is_some()));
                app.stopModalWithCode(NSAlertSecondButtonReturn);
            });
            let timer = unsafe { NSTimer::timerWithTimeInterval_repeats_block(0.5, false, &stop) };
            unsafe { NSRunLoop::currentRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
            super::super::offer_recovered(mtm);
            let stamp = NSUserDefaults::standardUserDefaults().doubleForKey(&ns("RecoveredTextsShownUntil"));
            check(
                "the next start says once where the copies are",
                MODAL_SEEN.with(Cell::get) && stamp > 0.0,
                &format!("stamp {stamp}"),
            );
            let count = documents(mtm).len();
            let _ = unsafe {
                NSApplication::sharedApplication(mtm).sendAction_to_from(sel!(mdQuickReference:), None, None)
            };
            check(
                "the quick reference opens as a document",
                documents(mtm).len() == count + 1,
                "",
            );
        }),
        when(
            "the quick reference to open",
            |c| {
                documents(c.e.mtm())
                    .iter()
                    .any(|d| d.displayName().to_string() == "Markdown-Kurzreferenz")
            },
            |c| {
                let reference = documents(c.e.mtm())
                    .into_iter()
                    .find(|d| d.displayName().to_string() == "Markdown-Kurzreferenz");
                check(
                    "the quick reference is in the interface language",
                    reference
                        .as_ref()
                        .is_some_and(|d| d.text().starts_with("# Markdown-Kurzreferenz")),
                    "",
                );
                if let Some(d) = reference {
                    d.close();
                }
                if let Some(window) = c.e.window() {
                    window.makeKeyAndOrderFront(None);
                }
            },
        ),
    ]
}

pub(super) fn steps() -> Vec<Step> {
    let mut steps = vec![
        applied(|c| {
            write_files(&doc_dir(c));
            if let Some(window) = c.e.window() {
                window.makeKeyAndOrderFront(None);
            }
            append(&c.e, SECTION);
        }),
        settle(|_| {}),
    ];
    for case in &COMMANDS {
        steps.extend(command(case));
    }
    steps.extend(outline_without_preview_steps());
    steps.extend(saving_and_unsaved_text_steps());
    steps.extend(table_steps());
    steps.extend(view_steps());
    steps.extend(dialog_steps());
    steps.extend(link_steps());
    steps.extend(paste_steps());
    steps.extend(export_steps());
    steps.extend(toolbar_steps());
    steps.extend(large_change_steps());
    steps.extend(image_file_steps());
    steps.extend(memory_steps());
    steps.extend(old_encoding_steps());
    steps.extend(unsaved_copy_steps());
    steps.extend(marks_on_screen_steps());
    steps.extend(huge_document_steps());
    steps.extend(app_steps());
    // Last: on a locked screen, where popovers do not close, the open editor would keep the window from being key.
    steps.extend(reload_under_an_open_editor_steps());
    steps
}
