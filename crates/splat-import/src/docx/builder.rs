//! Word's flat paragraphs on their way to blocks: lists nest, code lines and quotes join, and the notes Splat itself
//! writes at the end of a document become notes again.

use crate::rich::{Block, FlatItem, Piece, Span, levels_by_indent, nest_list};

/// What a paragraph turned out to be.
pub(super) enum Para {
    Heading {
        level: u8,
        spans: Vec<Span>,
        anchor: Option<String>,
    },
    Item(FlatItem),
    /// A list item whose bullet or number is typed as text, with its left margin in points, which gives its level.
    Typed {
        item: FlatItem,
        indent: f32,
    },
    /// A further paragraph of the list item before it, if there is one.
    Continued(Vec<Span>),
    /// One line of code.
    Code(String),
    Rule,
    Math(String),
    Plain(Vec<Span>),
}

fn is_blank(spans: &[Span]) -> bool {
    spans
        .iter()
        .all(|span| matches!(&span.piece, Piece::Text(text) if text.trim().is_empty()) || span.piece == Piece::Break)
}

/// A note that Splat wrote into the text, as it is being read.
struct OpenNote {
    name: String,
    sink: Box<BlockBuilder>,
}

/// Blocks being built from paragraphs.
#[derive(Default)]
pub(super) struct BlockBuilder {
    blocks: Vec<Block>,
    items: Vec<FlatItem>,
    /// The left margin of each of `items` that is typed as text.
    indents: Vec<Option<f32>>,
    code: Option<String>,
    quote: Option<Box<BlockBuilder>>,
    note: Option<OpenNote>,
    notes: Vec<Block>,
    /// The last paragraph was the empty one with a line above that Splat writes in front of its notes.
    rule_above_notes: bool,
}

impl BlockBuilder {
    /// Takes in a paragraph, into the quote that is open or starts with it if `quoted`.
    pub(super) fn push(&mut self, para: Para, quoted: bool) {
        self.rule_above_notes = false;
        if let Some(note) = &mut self.note {
            note.sink.push(para, quoted);
            return;
        }
        if matches!(&para, Para::Plain(spans) | Para::Continued(spans) if is_blank(spans)) {
            return;
        }
        if quoted {
            self.close_items();
            self.close_code();
            self.quote.get_or_insert_default().push(para, false);
            return;
        }
        self.close_quote();
        match para {
            Para::Item(item) => {
                self.close_code();
                self.items.push(item);
                self.indents.push(None);
            }
            Para::Typed { item, indent } => {
                self.close_code();
                self.items.push(item);
                self.indents.push(Some(indent));
            }
            Para::Continued(spans) => match self.items.last_mut() {
                Some(last) => last.item.blocks.push(Block::Paragraph(spans)),
                None => self.blocks.push(Block::Paragraph(spans)),
            },
            Para::Code(line) => {
                self.close_items();
                match &mut self.code {
                    Some(code) => {
                        code.push('\n');
                        code.push_str(&line);
                    }
                    None => self.code = Some(line),
                }
            }
            Para::Heading { level, spans, anchor } => self.block(Block::Heading { level, spans, anchor }),
            Para::Rule => self.block(Block::Rule),
            Para::Math(tex) => self.block(Block::Math(tex)),
            Para::Plain(spans) => self.block(Block::Paragraph(spans)),
        }
    }

    /// Takes in a block that is no paragraph.
    pub(super) fn push_block(&mut self, block: Block) {
        self.rule_above_notes = false;
        if let Some(note) = &mut self.note {
            note.sink.push_block(block);
            return;
        }
        self.close_quote();
        self.block(block);
    }

    fn block(&mut self, block: Block) {
        self.close_items();
        self.close_code();
        self.blocks.push(block);
    }

    /// The empty paragraph with a line above it has passed.
    pub(super) fn saw_rule_above_notes(&mut self) {
        self.rule_above_notes = true;
    }

    /// Whether a paragraph in the look of a note, with a number in front, is one of Splat's own notes.
    pub(super) fn expects_written_note(&self) -> bool {
        self.rule_above_notes || self.note.is_some()
    }

    /// Starts one of Splat's own notes: everything behind it belongs to it, until the next.
    pub(super) fn start_written_note(&mut self, name: String) {
        self.close_note();
        self.close_all();
        self.note = Some(OpenNote {
            name,
            sink: Box::default(),
        });
    }

    fn close_note(&mut self) {
        if let Some(note) = self.note.take() {
            self.notes.push(Block::Note {
                name: note.name,
                blocks: note.sink.finish(),
            });
        }
    }

    fn close_items(&mut self) {
        if !self.items.is_empty() {
            let mut items = std::mem::take(&mut self.items);
            levels_by_indent(&mut items, &std::mem::take(&mut self.indents));
            self.blocks.extend(nest_list(items));
        }
    }

    fn close_code(&mut self) {
        if let Some(text) = self.code.take() {
            self.blocks.push(Block::Code {
                language: String::new(),
                text,
            });
        }
    }

    fn close_quote(&mut self) {
        if let Some(quote) = self.quote.take() {
            self.blocks.push(Block::Quote(quote.finish()));
        }
    }

    fn close_all(&mut self) {
        self.close_quote();
        self.close_items();
        self.close_code();
    }

    /// All the blocks, with Splat's own notes at the end.
    pub(super) fn finish(mut self) -> Vec<Block> {
        self.close_note();
        self.close_all();
        self.blocks.append(&mut self.notes);
        self.blocks
    }
}
