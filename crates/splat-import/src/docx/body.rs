//! The body of a Word file as blocks: what each paragraph is and where it goes.

use std::collections::{HashMap, HashSet, VecDeque};

use roxmltree::Node;

use super::builder::{BlockBuilder, Para};
use super::numbering::Numbering;
use super::package::{MC, W, child_w, chosen_content, w_attr, w_val};
use super::runs::{Field, Line, RunContext};
use super::styles::{ListPlace, RunProps, Style, Styles};
use super::{Media, MissingPicture, WordError};
use crate::package::Relationships;
use crate::rich::{Align, Block, FlatItem, Item, ListKind, Mark, Piece, Span, marker_kind, marker_task, visit_spans};
use splat_ooxml::zip::Archive;

/// How deep the walker goes before it takes what is left as plain text. Each level costs stack, and neither Word nor
/// anyone else nests this deep in anything that is not made to hurt.
pub(super) const MOST_DEPTH: usize = 64;
/// Word's numbering knows nine levels.
const DEEPEST_LEVEL: usize = 8;
/// The least `w:ind w:left` of a paragraph that continues a list item, half of what a first level indents.
const LEAST_ITEM_INDENT: i64 = 360;
/// The number no list of the numbering part has, for the lists typed as text.
const TYPED_LIST: u64 = u64::MAX;
/// The longest text in front of a tab that is taken for a list's bullet or number, such as `iii.` or `1.2.10.`.
const LONGEST_MARKER: usize = 12;

/// A footnotes or endnotes part: the notes in it and its relationships.
pub(super) struct NotesPart<'a, 'n, 'i> {
    pub(super) notes: HashMap<String, Node<'n, 'i>>,
    pub(super) rels: &'a Relationships,
}

/// Goes through a part of a Word file, with what all of it shares.
pub(super) struct Walker<'a, 'n, 'i> {
    pub(super) archive: &'a Archive<'a>,
    pub(super) styles: &'a Styles,
    numbering: &'a Numbering,
    pub(super) rels: &'a Relationships,
    pub(super) images_folder: &'a str,
    pub(super) anchors: &'a HashSet<String>,
    has_heading_1: bool,
    notes_parts: [NotesPart<'a, 'n, 'i>; 2],
    /// The number of each note referred to so far, by its part and id.
    note_numbers: HashMap<(usize, String), usize>,
    waiting_notes: VecDeque<(usize, Node<'n, 'i>, usize)>,
    /// Fields open at this point, which go on over paragraphs.
    pub(super) fields: Vec<Field>,
    /// How many items each list has had at each level, which go on numbering after anything between them.
    item_counts: HashMap<(u64, usize), u64>,
    /// What each picture part became, so that one used twice is written once.
    pub(super) pictures: HashMap<String, Option<String>>,
    /// The relationship ids or targets of pictures that lead to no part, each reported once.
    pub(super) without_part: HashSet<String>,
    pub(super) media: Vec<Media>,
    pub(super) missing: Vec<MissingPicture>,
    /// Reading the pictures has used up what the archive may inflate to, which ends the import.
    pub(super) over_budget: bool,
    /// An initial letter that waits for the paragraph it starts.
    drop_cap: Vec<Span>,
}

/// What a paragraph's `w:pPr` says.
#[derive(Default)]
struct ParagraphProps<'a> {
    style: Option<&'a str>,
    list: Option<ListPlace>,
    outline: Option<u8>,
    indent: i64,
    /// The first line starts left of the others, as a list item's marker does.
    hanging: bool,
    line_below: bool,
    line_above: bool,
    /// An initial letter set large, which Word writes as a paragraph of its own in front of the rest of its word.
    drop_cap: bool,
}

impl<'a> ParagraphProps<'a> {
    fn read(paragraph: Node<'a, '_>) -> ParagraphProps<'a> {
        let Some(properties) = child_w(paragraph, "pPr") else {
            return ParagraphProps::default();
        };
        let has_line = |side: &str| {
            child_w(properties, "pBdr")
                .and_then(|border| child_w(border, side))
                .is_some_and(|line| !matches!(w_val(line), Some("none" | "nil")))
        };
        ParagraphProps {
            style: child_w(properties, "pStyle").and_then(w_val),
            list: ListPlace::read(properties),
            outline: child_w(properties, "outlineLvl")
                .and_then(w_val)
                .and_then(|level| level.parse().ok()),
            indent: child_w(properties, "ind")
                .and_then(|indent| w_attr(indent, "left"))
                .and_then(|left| left.parse().ok())
                .unwrap_or(0),
            hanging: child_w(properties, "ind").is_some_and(|indent| {
                let twips = |name: &str| w_attr(indent, name).and_then(|value| value.parse::<i64>().ok());
                // Cocoa writes `w:first-line`, which is no name of Word's.
                twips("hanging").is_some_and(|hanging| hanging > 0)
                    || twips("firstLine")
                        .or_else(|| twips("first-line"))
                        .is_some_and(|first| first < 0)
            }),
            line_below: has_line("bottom"),
            line_above: has_line("top"),
            drop_cap: child_w(properties, "framePr")
                .and_then(|frame| w_attr(frame, "dropCap"))
                .is_some_and(|kind| matches!(kind, "drop" | "margin")),
        }
    }
}

/// A list's bullet or number typed in front of a paragraph's text with a tab behind it, as Cocoa writes lists, which can
/// stand in runs of their own.
fn take_typed_marker(spans: &mut Vec<Span>) -> Option<String> {
    let mut prefix = String::new();
    for span in spans.iter() {
        let Piece::Text(text) = &span.piece else { break };
        prefix.push_str(text);
        if prefix.trim_start_matches([' ', '\t']).contains('\t') || prefix.len() > 4 * LONGEST_MARKER {
            break;
        }
    }
    let start = prefix.len() - prefix.trim_start_matches([' ', '\t']).len();
    let (marker, after) = prefix[start..].split_once('\t')?;
    let single_sign = marker.chars().count() == 1 && !marker.chars().all(char::is_alphanumeric);
    let counted = matches!(marker_kind(marker), ListKind::Numbered { .. }) || marker_task(marker).is_some();
    if marker.is_empty() || marker.contains(' ') || marker.chars().count() > LONGEST_MARKER || !(single_sign || counted)
    {
        return None;
    }
    let mut cut = prefix.len() - after.trim_start_matches([' ', '\t']).len();
    let marker = marker.to_string();
    while cut > 0 {
        let Some(Span {
            piece: Piece::Text(text),
            ..
        }) = spans.first_mut()
        else {
            break;
        };
        if text.len() <= cut {
            cut -= text.len();
            spans.remove(0);
        } else {
            text.replace_range(..cut, "");
            cut = 0;
        }
    }
    Some(marker)
}

/// The alignment that a table cell's paragraph sets.
pub(super) fn paragraph_alignment(paragraph: Node) -> Align {
    let alignment = child_w(paragraph, "pPr")
        .and_then(|properties| child_w(properties, "jc"))
        .and_then(w_val);
    match alignment {
        Some("center") => Align::Center,
        Some("right" | "end") => Align::Right,
        _ => Align::None,
    }
}

/// All the text below `node`, when nesting has become too deep to read anything else of it.
pub(super) fn text_below(node: Node) -> String {
    node.descendants()
        .filter(|descendant| descendant.tag_name().name() == "t")
        .filter_map(|text| text.text())
        .collect()
}

/// The text of code as it stands on its lines.
fn plain_text(spans: &[Span]) -> String {
    let mut text = String::new();
    for span in spans {
        match &span.piece {
            Piece::Text(part) | Piece::Code(part) | Piece::Math(part) => text.push_str(part),
            Piece::Break => text.push('\n'),
            Piece::Image(image) => text.push_str(&image.alt),
            Piece::Note(_) => {}
        }
    }
    text
}

/// A paragraph in monospace only, with no style that calls it inline code: a character style says so on purpose, as
/// in `to_docx` for a paragraph of one code span.
fn is_all_monospace(line: &Line) -> bool {
    !line.named_code
        && line.spans.iter().any(|span| matches!(span.piece, Piece::Code(_)))
        && line.spans.iter().all(|span| {
            span.link.is_none()
                && match &span.piece {
                    Piece::Code(_) | Piece::Break => true,
                    Piece::Text(text) => text.trim().is_empty(),
                    Piece::Image(_) | Piece::Note(_) | Piece::Math(_) => false,
                }
        })
}

/// The number in front of one of the notes that Splat writes, taken out of its first span.
fn written_number(spans: &mut [Span]) -> Option<String> {
    let first = spans.first_mut()?;
    let Piece::Text(text) = &mut first.piece else {
        return None;
    };
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    let space_behind = text.as_bytes().get(digits + 1).is_none_or(u8::is_ascii_whitespace);
    if digits == 0 || text.as_bytes().get(digits) != Some(&b'.') || !space_behind {
        return None;
    }
    let number = text[..digits].to_string();
    let rest = text[digits + 1..].trim_start().to_string();
    *text = rest;
    Some(number)
}

/// The checkbox that `to_docx` and Word's content controls put in front of a list item, taken out of its first span.
fn take_checkbox(spans: &mut [Span]) -> Option<bool> {
    let first = spans.first_mut()?;
    let Piece::Text(text) = &mut first.piece else {
        return None;
    };
    let first_char = text.chars().next()?;
    let checked = match first_char {
        '☐' => false,
        '☑' | '☒' => true,
        _ => return None,
    };
    let rest = text[first_char.len_utf8()..].trim_start().to_string();
    *text = rest;
    Some(checked)
}

/// Whether a span is a superscript number that stands for one of the notes `names`.
fn written_reference<'s>(span: &'s Span, names: &HashSet<String>) -> Option<&'s str> {
    match &span.piece {
        Piece::Text(text) if span.marks.has(Mark::Superscript) && names.contains(text) => Some(text),
        _ => None,
    }
}

/// Connects the notes `to_docx` wrote into the text with their references, which are superscript numbers. A note that
/// nothing refers to was no note, and goes back to being text.
fn connect_written_notes(blocks: Vec<Block>) -> Vec<Block> {
    let names: HashSet<String> = blocks
        .iter()
        .filter_map(|block| match block {
            Block::Note { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    if names.is_empty() {
        return blocks;
    }
    let mut blocks = blocks;
    let mut referred = HashSet::new();
    visit_spans(&mut blocks, &mut |span| {
        if let Some(name) = written_reference(span, &names).map(str::to_string) {
            span.marks = span.marks.with(Mark::Superscript, false);
            span.piece = Piece::Note(name.clone());
            referred.insert(name);
        }
    });
    let mut out = Vec::with_capacity(blocks.len());
    for block in blocks {
        match block {
            Block::Note { name, mut blocks } if !referred.contains(&name) => {
                let number = Span::text(format!("{name}. "));
                match blocks.first_mut() {
                    Some(Block::Paragraph(spans)) => spans.insert(0, number),
                    _ => out.push(Block::Paragraph(vec![number])),
                }
                out.extend(blocks);
            }
            block => out.push(block),
        }
    }
    out
}

impl<'a, 'n, 'i> Walker<'a, 'n, 'i> {
    #[expect(
        clippy::too_many_arguments,
        reason = "all of them are read-only inputs from `import`, named at its one call"
    )]
    pub(super) fn new(
        archive: &'a Archive<'a>,
        styles: &'a Styles,
        numbering: &'a Numbering,
        rels: &'a Relationships,
        images_folder: &'a str,
        anchors: &'a HashSet<String>,
        has_heading_1: bool,
        notes_parts: [NotesPart<'a, 'n, 'i>; 2],
    ) -> Walker<'a, 'n, 'i> {
        Walker {
            archive,
            styles,
            numbering,
            rels,
            images_folder,
            anchors,
            has_heading_1,
            notes_parts,
            note_numbers: HashMap::new(),
            waiting_notes: VecDeque::new(),
            fields: Vec::new(),
            item_counts: HashMap::new(),
            pictures: HashMap::new(),
            without_part: HashSet::new(),
            media: Vec::new(),
            missing: Vec::new(),
            over_budget: false,
            drop_cap: Vec::new(),
        }
    }

    /// The blocks of a document's body, with the notes it refers to at the end.
    pub(super) fn document(&mut self, body: Node) -> Vec<Block> {
        let mut sink = BlockBuilder::default();
        self.blocks(body, 0, &mut sink);
        if !self.drop_cap.is_empty() {
            sink.push(Para::Plain(std::mem::take(&mut self.drop_cap)), false);
        }
        let mut blocks = connect_written_notes(sink.finish());
        self.write_notes(&mut blocks);
        blocks
    }

    /// The pictures of the document and those it goes without, unless reading them took more than the archive may give.
    pub(super) fn finish(self) -> Result<(Vec<Media>, Vec<MissingPicture>), WordError> {
        if self.over_budget {
            return Err(WordError::TooLarge);
        }
        Ok((self.media, self.missing))
    }

    /// The number of the note `id` of a notes part if there is such a note; one that is new waits to be written.
    pub(super) fn note_number(&mut self, part: usize, id: &str) -> Option<usize> {
        let note = *self.notes_parts[part].notes.get(id)?;
        let key = (part, id.to_string());
        if let Some(number) = self.note_numbers.get(&key) {
            return Some(*number);
        }
        let number = self.note_numbers.len() + 1;
        self.note_numbers.insert(key, number);
        self.waiting_notes.push_back((part, note, number));
        Some(number)
    }

    /// Writes the notes as they come up; one may refer to the next, and each is written once.
    fn write_notes(&mut self, blocks: &mut Vec<Block>) {
        let body_rels = self.rels;
        while let Some((part, note, number)) = self.waiting_notes.pop_front() {
            self.rels = self.notes_parts[part].rels;
            self.fields.clear();
            let mut sink = BlockBuilder::default();
            self.blocks(note, 0, &mut sink);
            blocks.push(Block::Note {
                name: number.to_string(),
                blocks: sink.finish(),
            });
        }
        self.rels = body_rels;
    }

    pub(super) fn blocks(&mut self, parent: Node, depth: usize, sink: &mut BlockBuilder) {
        for child in parent.children().filter(|child| child.is_element()) {
            self.block(child, depth + 1, sink);
        }
    }

    fn block(&mut self, node: Node, depth: usize, sink: &mut BlockBuilder) {
        if depth >= MOST_DEPTH {
            sink.push(Para::Plain(vec![Span::text(text_below(node))]), false);
            return;
        }
        let tag = node.tag_name();
        match (tag.namespace(), tag.name()) {
            (Some(W), "p") => self.paragraph(node, depth, sink),
            (Some(W), "tbl") => self.table(node, depth, sink),
            (Some(W), "sdt" | "sdtContent" | "customXml" | "smartTag" | "ins" | "moveTo") => {
                self.blocks(node, depth, sink);
            }
            (Some(MC), "AlternateContent") => {
                if let Some(chosen) = chosen_content(node) {
                    self.blocks(chosen, depth, sink);
                }
            }
            _ => {}
        }
    }

    fn heading_of(&self, props: &ParagraphProps, style: Style) -> Option<u8> {
        match props.outline {
            Some(level) if level < 9 => Some((level + 1).min(6)),
            Some(_) => None,
            None => style.heading,
        }
    }

    fn paragraph(&mut self, node: Node, depth: usize, sink: &mut BlockBuilder) {
        let props = ParagraphProps::read(node);
        let style = self.styles.paragraph(props.style);
        let heading = self.heading_of(&props, style);
        // What these styles make all of their text look like says nothing about a part of it.
        let look_of_style = heading.is_some() || style.title.is_some() || style.quote || style.toc;
        let around = RunContext {
            paragraph: if look_of_style { RunProps::default() } else { style.run },
            link: None,
            leads_to_bookmark: false,
        };
        let mut line = Line::default();
        self.inlines(node, depth, &around, &mut line, sink);
        if style.toc && !line.text_outside_links {
            return;
        }
        if props.drop_cap {
            self.drop_cap.append(&mut line.spans);
            return;
        }
        if !self.drop_cap.is_empty() {
            let mut spans = std::mem::take(&mut self.drop_cap);
            spans.append(&mut line.spans);
            line.spans = spans;
        }
        let title = style.title.filter(|_| !self.has_heading_1);
        if let Some(level) = heading.or(title) {
            sink.push(
                Para::Heading {
                    level,
                    spans: line.spans,
                    anchor: line.anchor,
                },
                style.quote,
            );
            return;
        }
        if style.footnote_text && sink.expects_written_note() {
            let number = written_number(&mut line.spans);
            if let Some(number) = number {
                sink.start_written_note(number);
            }
        }
        self.classify(&props, style, line, sink);
    }

    /// Where a paragraph that is no heading goes.
    fn classify(&mut self, props: &ParagraphProps, style: Style, mut line: Line, sink: &mut BlockBuilder) {
        if line.spans.is_empty() && props.line_above {
            sink.saw_rule_above_notes();
            return;
        }
        let para = if line.spans.is_empty() && (props.line_below || line.horizontal_line) {
            Para::Rule
        } else if let Some(tex) = line.only_display_math() {
            Para::Math(tex)
        } else if let Some(mut item) = self.list_item(props, style) {
            item.item.task = take_checkbox(&mut line.spans);
            item.item.blocks.push(Block::Paragraph(line.spans));
            Para::Item(item)
        } else if props.hanging
            && let Some(marker) = take_typed_marker(&mut line.spans)
        {
            Para::Typed {
                item: FlatItem {
                    level: 0,
                    list: TYPED_LIST,
                    kind: marker_kind(&marker),
                    item: Item {
                        task: marker_task(&marker),
                        blocks: vec![Block::Paragraph(line.spans)],
                    },
                },
                indent: props.indent as f32 / 20.0,
            }
        } else if style.code || is_all_monospace(&line) {
            Para::Code(plain_text(&line.spans))
        } else if props.indent >= LEAST_ITEM_INDENT {
            Para::Continued(line.spans)
        } else {
            Para::Plain(line.spans)
        };
        sink.push(para, style.quote);
    }

    /// A list item without its content, if the paragraph belongs to a list that has a marker.
    fn list_item(&mut self, props: &ParagraphProps, style: Style) -> Option<FlatItem> {
        let place = props.list.or(style.list).filter(|place| place.id != 0)?;
        let level = place.level.unwrap_or(0).min(DEEPEST_LEVEL);
        let kind = self.numbering.kind(place.id, level)?;
        let count = self.item_counts.entry((place.id, level)).or_insert(0);
        let before = *count;
        *count += 1;
        for deeper in level + 1..=DEEPEST_LEVEL {
            self.item_counts.remove(&(place.id, deeper));
        }
        Some(FlatItem {
            level,
            list: place.id,
            kind: match kind {
                ListKind::Numbered { start } => ListKind::Numbered {
                    start: start.saturating_add(before),
                },
                ListKind::Bullet => ListKind::Bullet,
            },
            item: Item {
                task: None,
                blocks: Vec::new(),
            },
        })
    }
}
