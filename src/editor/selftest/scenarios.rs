//! Checks that run after the main click-through, on a section of their own appended to the document: every
//! formatting and table command, the dialogs, link clicks, pasting and dropping, exports, a large change, the
//! toolbar groups, the recovery after a crash and the quick reference.

use std::path::{Path, PathBuf};

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSDocumentController, NSModalResponse, NSWindow};
use objc2_foundation::NSData;

use super::super::actions::Action;
use super::{Ctx, EditorController, Plan, Step, applied, check, perform, select_in_source, settle, source, when};
use crate::document::MdDocument;

mod app;
mod dialogs;
mod exports;
mod files;
mod large;
mod links;
mod paste;
mod saving;
mod tables;
mod view;

use app::app_steps;
use dialogs::dialog_steps;
use exports::export_steps;
use files::{image_file_steps, memory_steps, old_encoding_steps};
use large::{huge_document_steps, large_change_steps};
use links::link_steps;
use paste::paste_steps;
use saving::{
    another_version_on_disk_steps, reload_under_an_open_editor_steps, saving_and_unsaved_text_steps,
    typing_through_a_save_steps, unsaved_copy_steps,
};
use tables::table_steps;
use view::{marks_on_screen_steps, outline_without_preview_steps, toolbar_steps, view_steps};

pub(super) const SECTION: &str = "\n\n## Prüfstand\n\nWort **fett** eins zwei drei.\n\n\
    Links: [Netz](https://example.com/pruefstand) #pruefstand [Andere](andere.md) [Fehlt](fehlt.md) \
    [Einstellungen](x-apple.systempreferences:com.apple.Settings) [[andere]] mit Fußnote[^pruef].\n\n\
    ![png](bild.png) ![jpg](foto.jpg) ![svg](zeichnung.svg) ![pdf](seite.pdf) ![tif](scan.tiff)\n\n\
    | A | B |\n|---|---|\n| 1 | 2 |\n\n[^pruef]: Die Prüffußnote.\n";

const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"10\"><rect width=\"20\" \
                   height=\"10\" fill=\"#3a7\"/></svg>";

fn remember(c: &Ctx) {
    *c.before.borrow_mut() = source(&c.e);
}

fn before(c: &Ctx) -> String {
    c.before.borrow().clone()
}

fn sizes(c: &Ctx) -> String {
    format!("{} bytes, before {}", source(&c.e).len(), before(c).len())
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
        check(&name, source(&c.e) == before(c), &sizes(c));
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
            command.check_text(&c.e, &before(c));
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

fn documents(mtm: MainThreadMarker) -> Vec<Retained<MdDocument>> {
    NSDocumentController::sharedDocumentController(mtm)
        .documents()
        .iter()
        .filter_map(|d| d.downcast::<MdDocument>().ok())
        .collect()
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
    let written = e.export_source().write(ExportKind::Html, "Export", &page);
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
    let written = e.export_source().write(ExportKind::Word, "Export", &path);
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

const SHEET: &str = "a sheet to show";

fn picture(c: &Ctx) -> Vec<u8> {
    std::fs::read(doc_dir(c).join("bild.png")).unwrap_or_default()
}

/// Each group of checks by its name, for `--only`.
fn groups() -> Vec<(&'static str, Vec<Step>)> {
    vec![
        ("commands", COMMANDS.iter().flat_map(|case| command(case)).collect()),
        ("outline without preview", outline_without_preview_steps()),
        ("saving and unsaved text", saving_and_unsaved_text_steps()),
        ("another version on disk", another_version_on_disk_steps()),
        ("typing through a save", typing_through_a_save_steps()),
        ("tables", table_steps()),
        ("view", view_steps()),
        ("dialogs", dialog_steps()),
        ("links", link_steps()),
        ("paste", paste_steps()),
        ("exports", export_steps()),
        ("toolbar", toolbar_steps()),
        ("large change", large_change_steps()),
        ("image files", image_file_steps()),
        ("memory", memory_steps()),
        ("old encoding", old_encoding_steps()),
        ("unsaved copies", unsaved_copy_steps()),
        ("marks on screen", marks_on_screen_steps()),
        ("huge document", huge_document_steps()),
        ("word count", super::notes::word_count_steps()),
        ("changes since the commit", super::notes::change_steps()),
        ("notes", super::notes::note_steps()),
        ("metadata", super::notes::metadata_steps()),
        ("paste as markdown", super::notes::paste_markdown_steps()),
        ("export options", super::notes::export_option_steps()),
        ("word import", super::notes::word_import_steps()),
        ("app", app_steps()),
        // Last: on a locked screen, where popovers do not close, the open editor would keep the window from being key.
        ("reload under an open editor", reload_under_an_open_editor_steps()),
    ]
}

/// The document of a group, opened from the template with the section and files all groups work on, so that nothing the
/// groups before did or failed to do reaches it.
fn own_document(group: &'static str) -> Vec<Step> {
    document_holding(group, |c| format!("{}{SECTION}", c.template))
}

/// A document of its own for `group`, holding `text` beside the files all groups work on, which the steps from here on
/// work on in place of the one before.
pub(super) fn document_holding(group: &'static str, text: impl Fn(&Ctx) -> String + 'static) -> Vec<Step> {
    let file = move |c: &Ctx| {
        c.out
            .join("Gruppen")
            .join(group.replace(' ', "-"))
            .join("QuickReference.md")
    };
    let open = move |c: &Ctx, path: &Path| {
        NSDocumentController::sharedDocumentController(c.e.mtm())
            .documents()
            .iter()
            .filter_map(|d| d.downcast::<MdDocument>().ok())
            .find(|d| {
                d.fileURL()
                    .and_then(|u| u.path())
                    .is_some_and(|p| Path::new(&p.to_string()) == path)
            })
    };
    vec![
        applied(move |c| {
            let path = file(c);
            let dir = path.parent().expect("a folder").to_path_buf();
            std::fs::create_dir_all(&dir).expect("a test folder");
            write_files(&dir);
            std::fs::write(&path, text(c)).expect("a test file");
            super::open_document(c.e.mtm(), path.to_str().unwrap_or_default());
        }),
        when(
            &format!("the document of \"{group}\" to open"),
            move |c| {
                open(c, &file(c)).is_some_and(|d| d.editor().is_some_and(|e| e.window().is_some_and(|w| w.isVisible())))
            },
            move |c| {
                let Some(next) = open(c, &file(c)).and_then(|d| d.editor()) else {
                    return;
                };
                if let Some(previous) = c.e.document() {
                    previous.updateChangeCount(objc2_app_kit::NSDocumentChangeType::ChangeCleared);
                    previous.close();
                }
                if let Some(window) = next.window() {
                    window.makeKeyAndOrderFront(None);
                }
                super::work_on(c, next);
            },
        ),
        settle(|_| {}),
    ]
}

pub(super) fn add_steps(plan: &mut Plan) {
    let only = std::env::var("SPLAT_SELFTEST_ONLY").ok();
    let mut found = false;
    for (name, group) in groups() {
        if only.as_deref().is_none_or(|only| only == name) {
            found = true;
            plan.add(name, own_document(name));
            plan.add(name, group);
        }
    }
    if !found {
        plan.add(
            "scenarios",
            vec![applied(|_| check("the group named by --only exists", false, ""))],
        );
    }
}
