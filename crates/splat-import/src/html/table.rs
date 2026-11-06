//! A table while its rows arrive: `rowspan` turns into empty cells in the rows below, as Markdown has nothing else for it.

use super::style::{Attributes, attr};
use crate::rich::{Align, Block, Cell, MOST_COLUMNS, Span, Table};

/// Empty cells any table may get from `rowspan`.
const FIRST_BUDGET: usize = 1000;
/// And for each real cell, so that a few wide rows of tall cells cannot make a text quadratically long.
const BUDGET_PER_CELL: usize = 4;

/// A table as far as it has been read.
#[derive(Debug)]
pub(super) struct OpenTable {
    rows: Vec<Vec<Cell>>,
    alignments: Vec<Align>,
    /// Columns that a cell of a row above still reaches into, with how many rows it has left, by column.
    reaching: Vec<(usize, u32)>,
    budget: usize,
}

impl Default for OpenTable {
    fn default() -> OpenTable {
        OpenTable {
            rows: Vec::new(),
            alignments: Vec::new(),
            reaching: Vec::new(),
            budget: FIRST_BUDGET,
        }
    }
}

/// A row as far as it has been read.
#[derive(Debug, Default)]
pub(super) struct OpenRow {
    cells: Vec<Cell>,
    column: usize,
    /// How far `reaching` has been gone through.
    next_reaching: usize,
    /// What this row's cells start reaching into, which counts from the next row on.
    starting: Vec<(usize, u32)>,
}

/// What a cell's attributes ask for.
#[derive(Debug)]
pub(super) struct CellShape {
    columns: u32,
    rows: u32,
    align: Align,
}

impl CellShape {
    /// The shape `attrs` ask for, with the declarations a style sheet gives the cell.
    pub(super) fn of(attrs: &Attributes, from_sheet: &str) -> CellShape {
        let number = |name: &str| attr(attrs, name).and_then(|value| value.trim().parse::<u32>().ok());
        CellShape {
            columns: number("colspan").unwrap_or(1).clamp(1, MOST_COLUMNS),
            rows: number("rowspan").unwrap_or(1).max(1),
            align: align_of(attrs, from_sheet),
        }
    }

    pub(super) fn columns(&self) -> u32 {
        self.columns
    }
}

fn align_of(attrs: &Attributes, from_sheet: &str) -> Align {
    let text_align = |css: &str| {
        let css = css.to_ascii_lowercase();
        css.split(';').find_map(|declaration| {
            let (property, value) = declaration.split_once(':')?;
            (property.trim() == "text-align").then(|| value.trim().to_string())
        })
    };
    let value = attr(attrs, "align")
        .map(str::to_ascii_lowercase)
        .or_else(|| attr(attrs, "style").and_then(text_align))
        .or_else(|| text_align(from_sheet))
        .unwrap_or_default();
    match value.as_str() {
        "left" => Align::Left,
        "center" | "middle" => Align::Center,
        "right" => Align::Right,
        _ => Align::None,
    }
}

impl OpenTable {
    /// Takes in the start of a cell in `row`, the last in this table to be open.
    pub(super) fn start_cell(&mut self, row: &mut OpenRow, shape: &CellShape) {
        self.budget += BUDGET_PER_CELL;
        self.fill_reaching(row);
        let columns = shape.columns as usize;
        if self.rows.is_empty() {
            self.alignments.resize(row.column, Align::None);
            self.alignments.push(shape.align);
            self.alignments.resize(row.column + columns, Align::None);
        }
        if shape.rows > 1 && self.budget >= columns {
            self.budget -= columns;
            for column in row.column..row.column + columns {
                row.starting.push((column, shape.rows - 1));
            }
        }
        row.column += columns;
    }

    /// Takes in a paragraph's alignment in the cell of `row` that is open, if the cell gave the column none.
    pub(super) fn align_open_cell(&mut self, row: &OpenRow, columns: u32, attrs: &Attributes, from_sheet: &str) {
        if !self.rows.is_empty() {
            return;
        }
        let align = align_of(attrs, from_sheet);
        if let Some(column) = row.column.checked_sub(columns as usize)
            && let Some(slot @ Align::None) = self.alignments.get_mut(column)
        {
            *slot = align;
        }
    }

    /// Empty cells for the columns that cells above reach into, up to the next that is free.
    fn fill_reaching(&mut self, row: &mut OpenRow) {
        while let Some((column, _)) = self.reaching.get(row.next_reaching) {
            if *column > row.column {
                return;
            }
            row.next_reaching += 1;
            if *column == row.column && self.budget > 0 {
                self.budget -= 1;
                row.cells.push(Cell::new(Vec::new()));
                row.column += 1;
            }
        }
    }

    /// Takes in a row that ended; what it costs to carry on with what reaches on, it pays from the budget too.
    pub(super) fn end_row(&mut self, row: OpenRow) {
        self.budget = self.budget.saturating_sub(self.reaching.len());
        if self.budget == 0 {
            self.reaching.clear();
        }
        self.reaching.retain_mut(|(_, left)| {
            *left -= 1;
            *left > 0
        });
        if !row.starting.is_empty() {
            self.reaching.extend(row.starting);
            self.reaching.sort_unstable();
        }
        if !row.cells.is_empty() {
            self.rows.push(row.cells);
        }
    }

    /// The block this table makes; a single cell is how mails lay out what is no table.
    pub(super) fn into_block(mut self) -> Option<Block> {
        if let [row] = self.rows.as_mut_slice()
            && let [cell] = row.as_mut_slice()
        {
            return Some(Block::Paragraph(std::mem::take(&mut cell.spans)));
        }
        (!self.rows.is_empty()).then_some(Block::Table(Table {
            alignments: self.alignments,
            rows: self.rows,
        }))
    }
}

impl OpenRow {
    /// Takes in a cell that ended.
    pub(super) fn end_cell(&mut self, spans: Vec<Span>, columns: u32) {
        self.cells.push(Cell { spans, columns });
    }
}
