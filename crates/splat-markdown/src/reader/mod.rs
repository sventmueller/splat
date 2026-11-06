//! Markdown read into the tree of `doc` by comrak. Only this module names the parser; what it relies on in comrak is
//! written down in the tests of `assumptions.rs`, which an upgrade that changes it fails.

use std::ops::Range;
use std::sync::Arc;

use comrak::nodes::{NodeValue, Sourcepos};
use comrak::{Arena, Options, parse_document};

use crate::doc::facts::{Unclosed, from_the_end_by_paragraph, unclosed_formulas};
use crate::doc::{Block, Doc, Inline, InlineKind};
use crate::text::{Byte, LineStarts, TextAt};

#[cfg(test)]
mod assumptions;
mod blocks;
mod inlines;
mod notes;
mod positions;

fn options() -> Options<'static> {
    let mut o = Options::default();
    o.extension.strikethrough = true;
    o.extension.table = true;
    o.extension.tasklist = true;
    o.extension.footnotes = true;
    o.extension.math_dollars = true;
    o.extension.autolink = true;
    o.extension.alerts = true;
    o.extension.front_matter_delimiter = Some("---".into());
    o.extension.superscript = true;
    // Single tildes then mean subscript, two still strike through.
    o.extension.subscript = true;
    o.extension.highlight = true;
    o.extension.shortcodes = true;
    o.extension.description_lists = true;
    o.extension.header_attributes = true;
    o.extension.wikilinks_title_after_pipe = true;
    o.extension.ofm_callouts = true;
    o.extension.ofm_comments = true;
    o.extension.inline_footnotes = true;
    o.extension.spoiler = true;
    o.extension.math_code = true;
    o.extension.math_latex = true;
    o
}

/// How many more opening than closing marks of inline notes or formulas a text may hold before they are switched off.
pub(super) const UNCLOSED_MARKS: usize = 1000;

/// comrak searches the rest of a paragraph for the end of every `^[`, which takes minutes when thousands have none.
fn limit_inline_notes(options: &mut Options, text: &str) {
    if inline_notes_may_be_limited(text) && unclosed(text, "^[", "]") > UNCLOSED_MARKS {
        options.extension.inline_footnotes = false;
    }
}

/// comrak searches the rest of a paragraph for the end of every opening delimiter of a formula as well.
fn limit_formulas(options: &mut Options, text: &str) {
    let Unclosed { latex, code } = unclosed_formulas(text);
    if latex > UNCLOSED_MARKS {
        options.extension.math_latex = false;
    }
    if code > UNCLOSED_MARKS {
        options.extension.math_code = false;
    }
}

/// How many `open` have no `close` of their own behind them in their paragraph; a `close` in front of an `open` closes
/// nothing.
fn unclosed(text: &str, open: &str, close: &str) -> usize {
    let bytes = text.as_bytes();
    let closers = std::cell::Cell::new(0usize);
    let mut unclosed = 0usize;
    from_the_end_by_paragraph(
        text,
        |at| {
            if bytes[at..].starts_with(close.as_bytes()) {
                closers.set(closers.get() + 1);
            } else if bytes[at..].starts_with(open.as_bytes()) {
                match closers.get().checked_sub(1) {
                    Some(left) => closers.set(left),
                    None => unclosed += 1,
                }
            }
        },
        || closers.set(0),
    );
    unclosed
}

/// Whether `limit_inline_notes` could apply to `text`, whatever its number of `]`.
fn inline_notes_may_be_limited(text: &str) -> bool {
    text.matches("^[").count() > UNCLOSED_MARKS
}

/// `text` read into a tree.
pub fn parse(text: &str) -> Doc {
    parse_shared(text.into())
}

/// [`parse`] for a text that is shared already, which the tree keeps without a copy.
pub fn parse_shared(text: Arc<str>) -> Doc {
    let blocks = parse_part(&text, true);
    Doc::new(text, blocks)
}

/// The top-level blocks of a part of a document; only the document's start may hold front matter.
pub fn parse_part(text: &str, at_start: bool) -> Vec<Block> {
    let arena = Arena::new();
    let mut opts = options();
    if !at_start {
        opts.extension.front_matter_delimiter = None;
    }
    limit_inline_notes(&mut opts, text);
    limit_formulas(&mut opts, text);
    let root = parse_document(&arena, text, &opts);
    let lines = LineStarts::new(text);
    let cx = Reader {
        text,
        lines: &lines,
        depth: std::cell::Cell::new(0),
        escapes: Default::default(),
    };
    let mut blocks = Vec::new();
    let mut inline_notes = Vec::new();
    let written = notes::written_like_inline_notes(text);
    for node in root.children() {
        if let NodeValue::FootnoteDefinition(f) = &node.data().value
            && notes::is_inline_note(&f.name)
            && !written.contains(f.name.as_str())
        {
            inline_notes.push((blocks.len() + inline_notes.len(), f.name.clone(), node));
        } else {
            blocks.extend(cx.block(node));
        }
    }
    notes::place_inline_notes(text, &mut blocks, inline_notes);
    blocks.shrink_to_fit();
    blocks
}

/// Nesting beyond this stays raw text: every consumer of the tree recurses, and a file with thousands of nested
/// quotes would otherwise overflow the stack.
const MAX_DEPTH: usize = 64;

struct Reader<'t> {
    text: &'t str,
    lines: &'t LineStarts,
    depth: std::cell::Cell<usize>,
    /// The `\|` of the table cell being read.
    escapes: std::cell::RefCell<Vec<Byte>>,
}

struct Level<'c>(&'c std::cell::Cell<usize>);

impl Drop for Level<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

impl Reader<'_> {
    fn enter(&self) -> Option<Level<'_>> {
        let d = self.depth.get();
        (d < MAX_DEPTH).then(|| {
            self.depth.set(d + 1);
            Level(&self.depth)
        })
    }

    fn raw_text(&self, range: Range<Byte>) -> Inline {
        Inline {
            kind: InlineKind::Text(self.text.at(range.clone()).into()),
            range,
            children: Box::default(),
        }
    }

    fn range(&self, sp: Sourcepos) -> Range<Byte> {
        let escapes = self.escapes.borrow();
        let start =
            behind_escaped_pipes(&escapes, self.lines.start(sp.start.line, sp.start.column)).min(self.text.end());
        let end = behind_escaped_pipes(&escapes, self.lines.end(sp.end.line, sp.end.column)).min(self.text.end());
        let start = floor_char(self.text, start);
        let end = ceil_char(self.text, end.max(start));
        start..end
    }
}

/// comrak removes the backslash of `\|` inside table cells before parsing inlines, so its positions behind each escape
/// are one byte short.
fn behind_escaped_pipes(escapes: &[Byte], offset: Byte) -> Byte {
    let (mut low, mut high) = (0, escapes.len());
    while low < high {
        let mid = (low + high) / 2;
        if escapes[mid] - mid < offset {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    offset + low
}

fn floor_char(s: &str, at: Byte) -> Byte {
    Byte::new(s.floor_char_boundary(at.get()))
}

fn ceil_char(s: &str, mut i: Byte) -> Byte {
    while i < s.end() && !s.boundary(i) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {

    use super::*;

    fn formulas_are_limited(text: &str) -> bool {
        let Unclosed { latex, code } = unclosed_formulas(text);
        latex > UNCLOSED_MARKS || code > UNCLOSED_MARKS
    }

    use crate::doc::{AlertKind, BlockKind, Figure, figures, math_delimiters, walk_blocks, walk_inlines};
    use crate::reader::notes::INLINE_NOTE;

    fn kinds(inlines: &[Inline]) -> Vec<String> {
        inlines.iter().map(|i| format!("{:?}", i.kind)).collect()
    }

    #[test]
    fn thousands_of_unclosed_inline_notes_are_plain_text() {
        let text = "^[a ".repeat(1500);
        let doc = parse(&text);
        assert_eq!(doc.blocks.len(), 1);
        assert_eq!(parse("Erst^[inline]").blocks.len(), 2);
    }

    #[test]
    fn only_formulas_without_an_end_are_given_up() {
        let math = |d: &Doc| {
            let mut found = 0;
            walk_blocks(&d.blocks, &mut |b| {
                walk_inlines(b.inlines(), &mut |i| {
                    found += usize::from(matches!(i.kind, InlineKind::Math { .. }))
                })
            });
            found
        };
        let closed = "\\(x\\) ".repeat(1500);
        assert_eq!(math(&parse(&closed)), 1500);
        let open = format!("\\(x\\) {}", "\\( ".repeat(1500));
        assert_eq!(math(&parse(&open)), 0);
        assert!(!formulas_are_limited(&"\\* ".repeat(1500)));
        assert!(!formulas_are_limited(&"\\(x\\) ".repeat(1500)), "all of them closed");
        assert!(formulas_are_limited(&"\\( ".repeat(1500)));
        assert!(formulas_are_limited(&"$` ".repeat(1500)));
        assert!(
            formulas_are_limited(&format!("{}\n\n{}", "\\( ".repeat(1500), "\\) ".repeat(1500))),
            "a closer in the next paragraph closes nothing"
        );
        assert!(!formulas_are_limited(&"$`x`$ ".repeat(1500)));
    }

    #[test]
    fn extended_syntax_becomes_its_own_nodes() {
        let text = "# Titel {#eigene-id .k}\n\n\
                    ==hell== x^2^ H~2~O ~~weg~~ :rocket: ||geheim|| %%still%% [[Seite|Titel]] ![[bild.png]] $`a+b`$ \\(x\\) ^[Notiz *fett*]\n\n\
                    ```math\nx^2\n```\n\nBegriff\n\n: Erklärung\n\n> [!bug]- Absturz\n> Inhalt\n\n> [!NOTE]\n> GitHub\n\n%%\nBlock\n%%\n";
        let doc = parse(text);
        let BlockKind::Heading { id, attrs, .. } = &doc.blocks[0].kind else {
            panic!()
        };
        assert_eq!(id.as_deref(), Some("eigene-id"));
        assert_eq!(text.at(attrs.clone().unwrap()), "{#eigene-id .k}");
        assert_eq!(kinds(doc.blocks[0].inlines()), ["Text(\"Titel\")"]);
        let p = doc.blocks[1].inlines();
        let find = |k: &str| p.iter().find(|i| format!("{:?}", i.kind).starts_with(k)).unwrap();
        assert_eq!(text.at(find("Highlight").content_range()), "hell");
        assert_eq!(text.at(find("Superscript").content_range()), "2");
        assert_eq!(text.at(find("Subscript").range.clone()), "~2~");
        assert_eq!(text.at(find("Strikethrough").range.clone()), "~~weg~~");
        assert_eq!(find("Emoji").kind, InlineKind::Emoji("🚀".into()));
        assert_eq!(text.at(find("Spoiler").content_range()), "geheim");
        assert_eq!(text.at(find("Comment").range.clone()), "%%still%%");
        let InlineKind::WikiLink { target, embed: false } = &find("WikiLink").kind else {
            panic!()
        };
        assert_eq!(&**target, "Seite");
        assert!(p.iter().any(|i| i.kind
            == InlineKind::WikiLink {
                target: "bild.png".into(),
                embed: true
            }));
        let maths: Vec<&str> = p
            .iter()
            .filter(|i| matches!(i.kind, InlineKind::Math { .. }))
            .map(|i| text.at(i.range.clone()))
            .collect();
        assert_eq!(maths, ["$`a+b`$", "\\(x\\)"]);
        assert_eq!(math_delimiters("$`a+b`$"), (2, 2));
        assert_eq!(math_delimiters("\\(x\\)"), (2, 2));
        assert!(figures(&doc).iter().any(|(_, f)| matches!(
            f,
            Figure::Math {
                tex: "x^2",
                display: true
            }
        )));
        let list = doc.blocks.iter().find(|b| b.kind == BlockKind::DefinitionList).unwrap();
        let parts: Vec<&BlockKind> = list.children()[0].children().iter().map(|b| &b.kind).collect();
        assert_eq!(parts, [&BlockKind::DefinitionTerm, &BlockKind::DefinitionDetails]);
        let alerts: Vec<&BlockKind> = doc
            .blocks
            .iter()
            .map(|b| &b.kind)
            .filter(|k| matches!(k, BlockKind::Alert { .. }))
            .collect();
        let BlockKind::Alert {
            kind,
            title,
            fold: Some(fold),
            ..
        } = alerts[0]
        else {
            panic!("{alerts:?}")
        };
        assert_eq!(
            (*kind, title.as_deref(), fold.open, text.at(fold.sign..fold.sign + 1)),
            (AlertKind::Caution, Some("Absturz"), false, "-")
        );
        assert!(matches!(
            alerts[1],
            BlockKind::Alert {
                kind: AlertKind::Note,
                title: None,
                fold: None,
                ..
            }
        ));
        assert!(doc.blocks.iter().any(|b| b.kind == BlockKind::Comment));
        let note = doc.blocks.last().unwrap();
        assert!(matches!(&note.kind, BlockKind::FootnoteDefinition { name, .. } if name.starts_with(INLINE_NOTE)));
        let inner = note.children()[0].inlines();
        assert_eq!(text.at(inner[0].range.clone()), "Notiz ");
        assert_eq!(text.at(inner[1].range.clone()), "*fett*");
        assert_eq!(text.at(note.range.clone()), "^[Notiz *fett*]");
        // Footnotes follow their references, so a definition at the end of the text stays last.
        let ordered = parse("Erst^[inline] dann[^1].\n\n[^1]: Letzte.\n");
        let names: Vec<&str> = ordered
            .blocks
            .iter()
            .filter_map(|b| match &b.kind {
                BlockKind::FootnoteDefinition { name, .. } => Some(&**name),
                _ => None,
            })
            .collect();
        assert_eq!(names, ["__inline_1", "1"]);
    }

    pub(super) fn slice<'a>(t: &'a str, r: &Range<Byte>) -> &'a str {
        t.at(r.clone())
    }

    #[test]
    fn inlines_of_a_table_cell_find_their_place_behind_escaped_pipes() {
        for t in [
            "a|b\n-|-\n\\|ä",
            "a|b\n-|-\n\\|ä\n",
            "a|b\n-|-\n\\|ä**b**|x\n",
            "a|b\n-|-\nä\\|ö\\|ü `c`|x\n",
        ] {
            let d = parse(t);
            let mut seen = 0;
            walk_blocks(&d.blocks, &mut |b| {
                walk_inlines(b.inlines(), &mut |i| {
                    assert!(
                        t.is_char_boundary(i.range.start.get()) && t.is_char_boundary(i.range.end.get()),
                        "{t:?} {i:?}"
                    );
                    if let InlineKind::Strong = i.kind {
                        assert_eq!(slice(t, &i.range), "**b**");
                        seen += 1;
                    }
                    if let InlineKind::Code(_) = i.kind {
                        assert_eq!(slice(t, &i.range), "`c`");
                        seen += 1;
                    }
                    if let InlineKind::Text(text) = &i.kind {
                        assert_eq!(slice(t, &i.range).replace("\\|", "|"), &**text, "{t:?}");
                    }
                });
            });
            assert_eq!(
                seen,
                usize::from(t.contains("**")) + usize::from(t.contains('`')),
                "{t:?}"
            );
        }
    }

    #[test]
    fn deep_nesting_stays_bounded() {
        let t = format!("{}x\n", "> ".repeat(500));
        let d = parse(&t);
        let mut depth = 0;
        let mut level: &[Block] = &d.blocks;
        while let Some(b) = level.first() {
            depth += 1;
            level = b.children();
        }
        assert!(depth <= MAX_DEPTH + 1, "{depth}");
        let t = format!("{}x{}\n", "*".repeat(400), "*".repeat(400));
        parse(&t);
    }

    #[test]
    fn inline_notes_are_only_switched_off_when_far_more_open_than_close() {
        let on = |text: String| {
            let mut o = options();
            limit_inline_notes(&mut o, &text);
            o.extension.inline_footnotes
        };
        assert!(on("^[".repeat(1000)));
        assert!(!on("^[".repeat(1001)));
        assert!(!on("^[".repeat(1002)));
        assert!(on("^[".repeat(1001) + "]"));
        assert!(!on("^[".repeat(1002) + "]"));
    }

    #[test]
    fn positions_stay_right_behind_many_escaped_pipes() {
        let t = "a|b\n-|-\n\\|\\|\\|\\|\\|\\|`c`|x\n";
        let d = parse(t);
        let mut code = None;
        walk_blocks(&d.blocks, &mut |b| {
            walk_inlines(b.inlines(), &mut |i| {
                if let InlineKind::Code(_) = i.kind {
                    code = Some(i.range.clone());
                }
            })
        });
        assert_eq!(slice(t, &code.unwrap()), "`c`");
    }

    #[test]
    fn a_position_inside_a_letter_moves_to_its_edge() {
        assert_eq!(floor_char("aéb", Byte::new(2)), Byte::new(1));
        assert_eq!(floor_char("aéb", Byte::new(3)), Byte::new(3));
        assert_eq!(floor_char("aéb", Byte::new(0)), Byte::new(0));
        assert_eq!(ceil_char("aéb", Byte::new(2)), Byte::new(3));
        assert_eq!(ceil_char("aéb", Byte::new(1)), Byte::new(1));
        assert_eq!(ceil_char("aéb", Byte::new(4)), Byte::new(4));
    }
}
