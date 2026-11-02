use crate::markdown::text::{line_end, line_start};
use std::ops::Range;

use unicode_width::UnicodeWidthStr;

use super::commands::{Edit, continuation_of};
use super::doc::{Align, Block, BlockKind, Doc, block_path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub rows: Vec<Vec<String>>,
    pub aligns: Vec<Align>,
    /// Whole lines covered by the table, without the trailing newline.
    pub range: Range<usize>,
    /// Container prefix of the first line, e.g. `> ` inside a block quote or `- ` in a list item.
    pub prefix: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableOp {
    InsertRowAbove,
    InsertRowBelow,
    InsertColumnLeft,
    InsertColumnRight,
    DeleteRow,
    DeleteColumn,
    Align(Align),
    Format,
}

pub fn table_block(doc: &Doc, offset: usize) -> Option<&Block> {
    block_path(&doc.blocks, offset)
        .into_iter()
        .rev()
        .find(|b| matches!(b.kind, BlockKind::Table { .. }))
}

pub fn table_at(text: &str, doc: &Doc, offset: usize) -> Option<(Table, Cell)> {
    let block = table_block(doc, offset)?;
    let BlockKind::Table { alignments } = &block.kind else {
        return None;
    };
    let line_begin = line_start(text, block.range.start);
    let prefix = text[line_begin..block.range.start].to_string();
    let end = line_end(text, block.range.end);
    let end = if text[block.range.clone()].ends_with('\n') {
        block.range.end - 1
    } else {
        end
    };
    let cols = alignments.len();
    let mut rows = Vec::new();
    let mut cell = Cell { row: 0, col: 0 };
    for (r, row) in block.children().iter().enumerate() {
        // comrak drops cells beyond the header's number, and rewriting the table would lose what they say.
        let behind = row.children().last().map_or(row.range.start, |c| c.range.end);
        if !text[behind.min(row.range.end)..row.range.end]
            .trim_matches([' ', '\t', '|', '\r', '\n'])
            .is_empty()
        {
            return None;
        }
        // A cell missing in the source gets an empty one from comrak, whose range holds the row's closing pipe.
        let mut cells: Vec<String> = row
            .children()
            .iter()
            .map(|c| {
                if c.inlines().is_empty() {
                    String::new()
                } else {
                    text[c.range.clone()].trim().to_string()
                }
            })
            .collect();
        cells.resize(cols, String::new());
        rows.push(cells);
        if row.range.start <= offset && offset <= row.range.end {
            cell.row = r;
            cell.col = row
                .children()
                .iter()
                .position(|c| offset <= c.range.end)
                .unwrap_or(cols - 1);
        }
    }
    if rows.is_empty() {
        return None;
    }
    Some((
        Table {
            rows,
            aligns: alignments.to_vec(),
            range: line_begin..end,
            prefix,
        },
        cell,
    ))
}

impl Table {
    fn widths(&self) -> Vec<usize> {
        (0..self.aligns.len())
            .map(|c| self.rows.iter().map(|r| r[c].width()).max().unwrap_or(0).max(3))
            .collect()
    }

    /// Renders the table with aligned pipes; returns the text and the content offset of every cell.
    pub fn render(&self) -> (String, Vec<Vec<usize>>) {
        let widths = self.widths();
        let following = continuation_of(&self.prefix);
        let mut out = String::new();
        let mut offsets = Vec::new();
        for (r, row) in self.rows.iter().enumerate() {
            if r > 0 {
                out.push('\n');
            }
            out.push_str(if r == 0 { &self.prefix } else { &following });
            out.push('|');
            let mut row_offsets = Vec::new();
            for (c, cell) in row.iter().enumerate() {
                let pad = widths[c] - cell.width();
                let (left, right) = match self.aligns[c] {
                    Align::Right => (pad, 0),
                    Align::Center => (pad / 2, pad - pad / 2),
                    _ => (0, pad),
                };
                out.push(' ');
                out.push_str(&" ".repeat(left));
                row_offsets.push(out.len());
                out.push_str(cell);
                out.push_str(&" ".repeat(right));
                out.push_str(" |");
            }
            offsets.push(row_offsets);
            if r == 0 {
                out.push('\n');
                out.push_str(&following);
                out.push('|');
                for (c, w) in widths.iter().enumerate() {
                    let dashes = |n: usize| "-".repeat(n);
                    let spec = match self.aligns[c] {
                        Align::None => dashes(w + 2),
                        Align::Left => format!(":{}", dashes(w + 1)),
                        Align::Right => format!("{}:", dashes(w + 1)),
                        Align::Center => format!(":{}:", dashes(*w)),
                    };
                    out.push_str(&spec);
                    out.push('|');
                }
            }
        }
        (out, offsets)
    }

    pub fn apply(&mut self, op: TableOp, at: Cell) -> Cell {
        let cols = self.aligns.len();
        match op {
            TableOp::InsertRowAbove => {
                let row = at.row.max(1);
                self.rows.insert(row, vec![String::new(); cols]);
                Cell { row, col: at.col }
            }
            TableOp::InsertRowBelow => {
                self.rows.insert(at.row + 1, vec![String::new(); cols]);
                Cell {
                    row: at.row + 1,
                    col: at.col,
                }
            }
            TableOp::InsertColumnLeft | TableOp::InsertColumnRight => {
                let col = if op == TableOp::InsertColumnLeft {
                    at.col
                } else {
                    at.col + 1
                };
                for row in &mut self.rows {
                    row.insert(col, String::new());
                }
                self.aligns.insert(col, Align::None);
                Cell { row: at.row, col }
            }
            TableOp::DeleteRow => {
                if at.row == 0 || self.rows.len() <= 2 {
                    for c in &mut self.rows[at.row] {
                        c.clear();
                    }
                    return at;
                }
                self.rows.remove(at.row);
                Cell {
                    row: at.row.min(self.rows.len() - 1),
                    col: at.col,
                }
            }
            TableOp::DeleteColumn => {
                if cols <= 1 {
                    return at;
                }
                for row in &mut self.rows {
                    row.remove(at.col);
                }
                self.aligns.remove(at.col);
                Cell {
                    row: at.row,
                    col: at.col.min(cols - 2),
                }
            }
            TableOp::Align(a) => {
                self.aligns[at.col] = a;
                at
            }
            TableOp::Format => at,
        }
    }
}

/// Applies a structural table operation and returns the edit with the cursor in the target cell.
pub fn table_edit(text: &str, doc: &Doc, offset: usize, op: TableOp) -> Option<Edit> {
    let (mut table, cell) = table_at(text, doc, offset)?;
    let target = table.apply(op, cell);
    Some(replace_table(&table, target, false))
}

fn replace_table(table: &Table, target: Cell, select: bool) -> Edit {
    let (rendered, offsets) = table.render();
    let start = table.range.start + offsets[target.row][target.col];
    let len = if select {
        table.rows[target.row][target.col].len()
    } else {
        0
    };
    Edit {
        range: table.range.clone(),
        replacement: rendered,
        selection: start..start + len,
    }
}

/// Moves to the next (or previous) cell, appending a row after the last cell. The table is re-aligned.
pub fn navigate(text: &str, doc: &Doc, offset: usize, backwards: bool) -> Option<Edit> {
    let (mut table, cell) = table_at(text, doc, offset)?;
    let cols = table.aligns.len();
    let index = cell.row * cols + cell.col;
    let target = if backwards {
        if index == 0 {
            return Some(replace_table(&table, cell, true));
        }
        index - 1
    } else {
        index + 1
    };
    if target >= table.rows.len() * cols {
        table.rows.push(vec![String::new(); cols]);
    }
    let target = Cell {
        row: target / cols,
        col: target % cols,
    };
    Some(replace_table(&table, target, true))
}

/// Moves to the same column in the next row, appending a row at the end of the table.
pub fn move_down(text: &str, doc: &Doc, offset: usize) -> Option<Edit> {
    let (mut table, cell) = table_at(text, doc, offset)?;
    if cell.row + 1 >= table.rows.len() {
        table.rows.push(vec![String::new(); table.aligns.len()]);
    }
    Some(replace_table(
        &table,
        Cell {
            row: cell.row + 1,
            col: cell.col,
        },
        false,
    ))
}

pub fn new_table(rows: usize, cols: usize) -> String {
    let rows = rows.max(2);
    let cols = cols.max(1);
    let mut table = Table {
        rows: vec![vec![String::new(); cols]; rows],
        aligns: vec![Align::None; cols],
        range: 0..0,
        prefix: String::new(),
    };
    for (c, cell) in table.rows[0].iter_mut().enumerate() {
        *cell = crate::i18n::tf("Spalte {}", &[&(c + 1).to_string()]);
    }
    table.render().0
}

#[cfg(test)]
mod hidden_cells_tests {
    use super::*;
    use crate::markdown::doc::parse;

    #[test]
    fn a_table_with_more_cells_than_its_header_is_not_rewritten() {
        let t = "| a |\n|---|\n| 1 | 2 | 3 |\n";
        assert_eq!(navigate(t, &parse(t), t.find('1').unwrap(), false), None);
    }

    #[test]
    fn a_cell_the_source_does_not_have_stays_empty() {
        let t = "| a | b |\n|---|---|\n| 1 |\n";
        let edit = navigate(t, &parse(t), t.find('a').unwrap(), false).unwrap();
        assert_eq!(edit.replacement, "| a   | b   |\n|-----|-----|\n| 1   |     |");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::doc::parse;

    fn apply(text: &str, e: &Edit) -> String {
        let mut s = text.to_string();
        s.replace_range(e.range.clone(), &e.replacement);
        s
    }

    #[test]
    fn format_aligns_pipes_with_umlauts() {
        let t = "x\n\n|a|bbbb|\n|-|:-:|\n|ä|1|\n\ny\n";
        let d = parse(t);
        let e = table_edit(t, &d, t.find("|a").unwrap() + 1, TableOp::Format).unwrap();
        let out = apply(t, &e);
        assert_eq!(out, "x\n\n| a   | bbbb |\n|-----|:----:|\n| ä   |  1   |\n\ny\n");
    }

    #[test]
    fn insert_and_delete_columns_and_rows() {
        let t = "| a | b |\n|---|---|\n| 1 | 2 |\n";
        let d = parse(t);
        let at = t.find('1').unwrap();
        let e = table_edit(t, &d, at, TableOp::InsertColumnRight).unwrap();
        let out = apply(t, &e);
        assert_eq!(out, "| a   |     | b   |\n|-----|-----|-----|\n| 1   |     | 2   |\n");
        assert_eq!(&out[e.selection.start..e.selection.start + 1], " ");
        let d2 = parse(&out);
        let e2 = table_edit(&out, &d2, out.find('1').unwrap(), TableOp::DeleteRow).unwrap();
        let out2 = apply(&out, &e2);
        assert!(out2.contains("|     |     |     |"));
    }

    #[test]
    fn tab_after_last_cell_appends_row() {
        let t = "| a | b |\n|---|---|\n| 1 | 2 |";
        let d = parse(t);
        let e = navigate(t, &d, t.find('2').unwrap(), false).unwrap();
        let out = apply(t, &e);
        assert_eq!(out.lines().count(), 4);
        assert_eq!(e.selection.len(), 0);
    }

    #[test]
    fn table_in_a_list_item_stays_one_item() {
        let t = "- | a | b |\n  |---|---|\n  | 1 | 2 |\n";
        let d = parse(t);
        let e = table_edit(t, &d, t.find('1').unwrap(), TableOp::Format).unwrap();
        assert_eq!(apply(t, &e), "- | a   | b   |\n  |-----|-----|\n  | 1   | 2   |\n");
        assert_eq!(continuation_of("> 12. "), ">     ");
        assert_eq!(continuation_of("> > "), "> > ");
    }

    #[test]
    fn table_inside_quote_keeps_prefix() {
        let t = "> | a | b |\n> |---|---|\n> | 1 | 2 |\n";
        let d = parse(t);
        let e = table_edit(t, &d, t.find('1').unwrap(), TableOp::Format).unwrap();
        let out = apply(t, &e);
        assert_eq!(out, "> | a   | b   |\n> |-----|-----|\n> | 1   | 2   |\n");
    }

    #[test]
    fn behind_the_last_cell_of_a_row_is_still_that_cell() {
        let t = "| a | b |\n|---|---|\n| 1 | 2 |  \n";
        let d = parse(t);
        let (_, cell) = table_at(t, &d, t.find("2 |").unwrap() + 3).unwrap();
        assert_eq!((cell.row, cell.col), (1, 1));
    }

    #[test]
    fn shift_tab_goes_back_one_cell() {
        let t = "| a | b |\n|---|---|\n| 1 | 2 |\n";
        let d = parse(t);
        let e = navigate(t, &d, t.find('2').unwrap(), true).unwrap();
        let out = apply(t, &e);
        assert_eq!(&out[e.selection.clone()], "1");
    }

    #[test]
    fn spaces_behind_the_table_belong_to_it() {
        let t = "| a |\n|---|\n| 1 |  \n\nx\n";
        let d = parse(t);
        let e = table_edit(t, &d, t.find('1').unwrap(), TableOp::Format).unwrap();
        assert_eq!(apply(t, &e), "| a   |\n|-----|\n| 1   |\n\nx\n");
    }

    #[test]
    fn right_and_centered_columns_pad_on_their_side() {
        let format = |t: &str| {
            let d = parse(t);
            apply(t, &table_edit(t, &d, 2, TableOp::Format).unwrap())
        };
        assert_eq!(
            format("| aaaaa |\n|----:|\n| b |\n"),
            "| aaaaa |\n|------:|\n|     b |\n"
        );
        assert_eq!(
            format("| aaaaa |\n|:---:|\n| b |\n"),
            "| aaaaa |\n|:-----:|\n|   b   |\n"
        );
    }

    #[test]
    fn tab_and_shift_tab_go_cell_by_cell_through_three_rows() {
        let t = "| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n";
        let d = parse(t);
        let go = |from: char, backwards: bool| {
            let e = navigate(t, &d, t.find(from).unwrap(), backwards).unwrap();
            let out = apply(t, &e);
            assert_eq!(out.lines().count(), 4, "no new row from {from}");
            out[e.selection.clone()].to_string()
        };
        assert_eq!(go('1', false), "2");
        assert_eq!(go('3', false), "4");
        assert_eq!(go('b', false), "1");
        assert_eq!(go('2', true), "1");
        assert_eq!(go('1', true), "b");
        assert_eq!(go('a', true), "a", "the first cell has nothing before it");
    }

    #[test]
    fn return_goes_down_one_row_in_the_same_column() {
        let t = "| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n";
        let d = parse(t);
        let down = |from: char| {
            let e = move_down(t, &d, t.find(from).unwrap()).unwrap();
            let mut s = t.to_string();
            s.replace_range(e.range.clone(), &e.replacement);
            s[e.selection.start..].chars().next().unwrap()
        };
        assert_eq!(down('b'), '2');
        assert_eq!(down('2'), '4');
    }

    #[test]
    fn only_quote_marks_repeat_and_list_marks_become_indentation() {
        assert_eq!(continuation_of("> "), "> ");
        assert_eq!(continuation_of("- "), "  ");
        assert_eq!(continuation_of("1. "), "   ");
        assert_eq!(continuation_of(". "), ". ", "no number");
        assert_eq!(continuation_of("1 "), "1 ", "no mark");
    }
}
