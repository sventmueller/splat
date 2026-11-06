//! Word's tables: a cell holds whole paragraphs and other tables, which Markdown's cannot, so they flatten to one line.

use roxmltree::Node;

use super::body::{MOST_DEPTH, Walker, paragraph_alignment};
use super::builder::{BlockBuilder, Para};
use super::package::{MC, W, child_w, chosen_content, is_on, is_w, w_val};
use crate::rich::{Align, Block, Cell, MOST_COLUMNS, Piece, Span, Table};

/// What the cells of a column say about its alignment.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum ColumnAlign {
    #[default]
    Unseen,
    All(Align),
    Mixed,
}

impl ColumnAlign {
    fn seen(self, align: Align) -> ColumnAlign {
        match self {
            ColumnAlign::Unseen => ColumnAlign::All(align),
            ColumnAlign::All(known) if known == align => self,
            _ => ColumnAlign::Mixed,
        }
    }
}

#[derive(Default)]
struct Row {
    cells: Vec<Cell>,
    column: usize,
}

/// `spans` behind what stands in `out`, on a line of its own.
fn join_line(out: &mut Vec<Span>, spans: Vec<Span>) {
    if spans.is_empty() {
        return;
    }
    if !out.is_empty() {
        out.push(Span::of(Piece::Break));
    }
    out.extend(spans);
}

/// All of `blocks` on the lines of one cell.
fn flatten(blocks: Vec<Block>, out: &mut Vec<Span>) {
    for block in blocks {
        match block {
            Block::Paragraph(spans) | Block::Heading { spans, .. } => join_line(out, spans),
            Block::List(list) => {
                for item in list.items {
                    flatten(item.blocks, out);
                }
            }
            Block::Quote(blocks) | Block::Note { blocks, .. } => flatten(blocks, out),
            Block::Code { text, .. } => {
                for line in text.split('\n') {
                    join_line(out, vec![Span::of(Piece::Code(line.to_string()))]);
                }
            }
            Block::Table(table) => {
                for row in table.rows {
                    let mut line = Vec::new();
                    for cell in row {
                        if !line.is_empty() {
                            line.push(Span::text(" "));
                        }
                        line.extend(cell.spans);
                    }
                    join_line(out, line);
                }
            }
            Block::Math(tex) => join_line(out, vec![Span::of(Piece::Math(tex))]),
            Block::Rule => {}
        }
    }
}

fn clamped_columns(value: Option<&str>) -> u32 {
    value
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(1)
        .clamp(1, MOST_COLUMNS)
}

impl Walker<'_, '_, '_> {
    pub(super) fn table(&mut self, node: Node, depth: usize, sink: &mut BlockBuilder) {
        let mut rows = Vec::new();
        let mut aligns = Vec::new();
        self.rows(node, depth + 1, &mut rows, &mut aligns);
        if let [only] = rows.as_mut_slice()
            && let [cell] = only.as_mut_slice()
        {
            sink.push(Para::Plain(std::mem::take(&mut cell.spans)), false);
            return;
        }
        let alignments = aligns
            .into_iter()
            .map(|column| match column {
                ColumnAlign::All(align) => align,
                ColumnAlign::Unseen | ColumnAlign::Mixed => Align::None,
            })
            .collect();
        sink.push_block(Block::Table(Table { alignments, rows }));
    }

    fn rows(&mut self, parent: Node, depth: usize, rows: &mut Vec<Vec<Cell>>, aligns: &mut Vec<ColumnAlign>) {
        if depth >= MOST_DEPTH {
            return;
        }
        for child in parent.children().filter(|child| child.is_element()) {
            let tag = child.tag_name();
            match (tag.namespace(), tag.name()) {
                (Some(W), "tr") => rows.extend(self.row(child, depth + 1, aligns)),
                (Some(W), "sdt" | "sdtContent" | "customXml" | "ins" | "moveTo") => {
                    self.rows(child, depth + 1, rows, aligns);
                }
                (Some(MC), "AlternateContent") => {
                    if let Some(chosen) = chosen_content(child) {
                        self.rows(chosen, depth + 1, rows, aligns);
                    }
                }
                _ => {}
            }
        }
    }

    /// A row's cells, or `None` for a row that a tracked change has deleted.
    fn row(&mut self, node: Node, depth: usize, aligns: &mut Vec<ColumnAlign>) -> Option<Vec<Cell>> {
        let mut row = Row::default();
        if let Some(properties) = child_w(node, "trPr") {
            if child_w(properties, "del").is_some_and(is_on) {
                return None;
            }
            if let Some(before) = child_w(properties, "gridBefore") {
                for _ in 0..clamped_columns(w_val(before)) {
                    row.cells.push(Cell::new(Vec::new()));
                    row.column += 1;
                }
            }
        }
        self.cells(node, depth, &mut row, aligns);
        Some(row.cells)
    }

    fn cells(&mut self, parent: Node, depth: usize, row: &mut Row, aligns: &mut Vec<ColumnAlign>) {
        if depth >= MOST_DEPTH {
            return;
        }
        for child in parent.children().filter(|child| child.is_element()) {
            let tag = child.tag_name();
            match (tag.namespace(), tag.name()) {
                (Some(W), "tc") => self.cell(child, depth + 1, row, aligns),
                (Some(W), "sdt" | "sdtContent" | "customXml" | "ins" | "moveTo") => {
                    self.cells(child, depth + 1, row, aligns);
                }
                _ => {}
            }
        }
    }

    fn cell(&mut self, node: Node, depth: usize, row: &mut Row, aligns: &mut Vec<ColumnAlign>) {
        let properties = child_w(node, "tcPr");
        let columns = clamped_columns(
            properties
                .and_then(|properties| child_w(properties, "gridSpan"))
                .and_then(w_val),
        );
        let merged_into_above = properties
            .and_then(|properties| child_w(properties, "vMerge"))
            .is_some_and(|merge| !matches!(w_val(merge), Some("restart")));
        let mut spans = Vec::new();
        if !merged_into_above {
            let mut sink = BlockBuilder::default();
            self.blocks(node, depth, &mut sink);
            flatten(sink.finish(), &mut spans);
        }
        if columns == 1 && !spans.is_empty() {
            let align = node
                .children()
                .find(|child| is_w(*child, "p"))
                .map_or(Align::None, paragraph_alignment);
            if aligns.len() <= row.column {
                aligns.resize(row.column + 1, ColumnAlign::Unseen);
            }
            aligns[row.column] = aligns[row.column].seen(align);
        }
        row.column += columns as usize;
        row.cells.push(Cell { spans, columns });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_column_knows_whether_its_cells_agree() {
        let right = ColumnAlign::Unseen.seen(Align::Right);
        assert!(right == ColumnAlign::All(Align::Right));
        assert!(right.seen(Align::Right) == right);
        assert!(right.seen(Align::None) == ColumnAlign::Mixed);
        assert!(ColumnAlign::Mixed.seen(Align::Right) == ColumnAlign::Mixed);
    }

    #[test]
    fn the_width_of_a_cell_stays_within_bounds() {
        assert_eq!(clamped_columns(None), 1);
        assert_eq!(clamped_columns(Some("x")), 1);
        assert_eq!(clamped_columns(Some("0")), 1);
        assert_eq!(clamped_columns(Some("3")), 3);
        assert_eq!(clamped_columns(Some("100000")), MOST_COLUMNS);
    }

    #[test]
    fn what_a_cell_holds_comes_down_to_lines() {
        let mut spans = Vec::new();
        flatten(
            vec![
                Block::Paragraph(vec![Span::text("a")]),
                Block::Heading {
                    level: 1,
                    spans: vec![Span::text("b")],
                    anchor: None,
                },
                Block::Rule,
                Block::Code {
                    language: String::new(),
                    text: "c\nd".to_string(),
                },
                Block::Math("e".to_string()),
                Block::Quote(vec![Block::Paragraph(vec![Span::text("f")])]),
                Block::Paragraph(Vec::new()),
                Block::Table(Table {
                    alignments: Vec::new(),
                    rows: vec![vec![Cell::new(vec![Span::text("g")]), Cell::new(vec![Span::text("h")])]],
                }),
            ],
            &mut spans,
        );
        let br = Span::of(Piece::Break);
        assert_eq!(
            spans,
            vec![
                Span::text("a"),
                br.clone(),
                Span::text("b"),
                br.clone(),
                Span::of(Piece::Code("c".to_string())),
                br.clone(),
                Span::of(Piece::Code("d".to_string())),
                br.clone(),
                Span::of(Piece::Math("e".to_string())),
                br.clone(),
                Span::text("f"),
                br,
                Span::text("g"),
                Span::text(" "),
                Span::text("h"),
            ]
        );
    }
}
