//! Rows of text separated by tabs, semicolons or commas, as spreadsheets copy them. Prose has commas too, so that a text
//! only counts as separated by `;` or `,` if nothing in it reads like sentences or Markdown.

use crate::rich::{Align, Block, Cell, Piece, Rich, Span, Table};

const LONGEST_CELL: usize = 200;
const MOST_WORDS: usize = 5;
/// From this many words on, a cell behind the first that starts in lowercase goes on a sentence.
const WORDS_OF_A_CLAUSE: usize = 3;

/// A table from tab, semicolon or comma separated text, `None` if it is no table.
pub fn from_text(text: &str) -> Option<Rich> {
    let rows = b"\t;,".iter().copied().find_map(|separator| {
        if !text.as_bytes().contains(&separator) {
            return None;
        }
        let rows = read_rows(text, separator);
        (same_width(&rows) && two_columns_hold_text(&rows) && (separator == b'\t' || reads_as_data(&rows)))
            .then_some(rows)
    })?;
    Some(Rich {
        blocks: vec![Block::Table(table_of(&rows))],
    })
}

/// The rows of `text` by RFC 4180 with the cells as they stand, without the blank lines at its ends.
fn read_rows(text: &str, separator: u8) -> Vec<Vec<String>> {
    let bytes = text.as_bytes();
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut at = 0;
    loop {
        let (cell, behind) = read_cell(text, at, separator);
        row.push(cell);
        at = behind;
        match bytes.get(at) {
            Some(b'\r') if bytes.get(at + 1) == Some(&b'\n') => at += 2,
            Some(b'\r' | b'\n') => at += 1,
            Some(_) => {
                at += 1;
                continue;
            }
            None => {
                rows.push(row);
                break;
            }
        }
        rows.push(std::mem::take(&mut row));
    }
    let blank = |row: &Vec<String>| matches!(row.as_slice(), [only] if only.trim().is_empty());
    let first = rows.iter().position(|row| !blank(row)).unwrap_or(rows.len());
    let last = rows.iter().rposition(|row| !blank(row)).map_or(first, |at| at + 1);
    rows.drain(last..);
    rows.drain(..first);
    rows
}

/// The cell starting at `at`, and where it ends.
fn read_cell(text: &str, at: usize, separator: u8) -> (String, usize) {
    if let Some(quoted) = quoted_cell(text, at, separator) {
        return quoted;
    }
    let rest = &text.as_bytes()[at..];
    let len = rest
        .iter()
        .position(|b| *b == separator || matches!(b, b'\n' | b'\r'))
        .unwrap_or(rest.len());
    (text[at..at + len].to_string(), at + len)
}

/// The cell at `at` if it is quoted properly. A scan that fails ends at the first quote that is not doubled, and the
/// quotes it looks past as doubled cannot start a long scan of their own, so that all of them together stay linear.
fn quoted_cell(text: &str, at: usize, separator: u8) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    if bytes.get(at) != Some(&b'"') {
        return None;
    }
    let mut cell = String::new();
    let mut from = at + 1;
    loop {
        let quote = from + bytes[from..].iter().position(|b| *b == b'"')?;
        cell.push_str(&text[from..quote]);
        let behind = quote + 1;
        match bytes.get(behind) {
            Some(b'"') => {
                cell.push('"');
                from = behind + 1;
            }
            None | Some(b'\n' | b'\r') => return Some((cell, behind)),
            Some(byte) if *byte == separator => return Some((cell, behind)),
            Some(_) => return None,
        }
    }
}

fn same_width(rows: &[Vec<String>]) -> bool {
    rows.first()
        .is_some_and(|first| first.len() >= 2 && rows.iter().all(|row| row.len() == first.len()))
}

/// An empty column is no column: a leading tab only indents, and a trailing comma only ends a line of a poem.
fn two_columns_hold_text(rows: &[Vec<String>]) -> bool {
    let width = rows.first().map_or(0, Vec::len);
    (0..width)
        .filter(|column| rows.iter().any(|row| !row[*column].trim().is_empty()))
        .nth(1)
        .is_some()
}

fn reads_as_data(rows: &[Vec<String>]) -> bool {
    rows.len() >= 2
        && rows.iter().flatten().all(|cell| fits_a_cell(cell))
        && !rows.iter().any(|row| reads_as_prose(row))
        && (separators_stand_alone(rows) || rows.iter().flatten().any(|cell| is_number(cell)))
        && !rows
            .first()
            .is_some_and(|first| first.iter().all(|cell| is_number(cell)))
}

fn fits_a_cell(cell: &str) -> bool {
    cell.trim().chars().count() <= LONGEST_CELL && cell.split_whitespace().nth(MOST_WORDS).is_none()
}

/// Whether a line starts like a Markdown block or ends like a sentence, or its later cells go on like one.
fn reads_as_prose(row: &[String]) -> bool {
    let starts_block = row.first().is_some_and(|first| starts_markdown_block(first.trim()));
    let ends_sentence = row
        .last()
        .is_some_and(|last| last.trim().ends_with(['.', '!', '?', ';', ':', ',']));
    let goes_on = row.iter().skip(1).any(|cell| {
        let cell = cell.trim();
        cell.chars().next().is_some_and(char::is_lowercase)
            && cell.split_whitespace().nth(WORDS_OF_A_CLAUSE - 1).is_some()
    });
    starts_block || ends_sentence || goes_on
}

fn starts_markdown_block(cell: &str) -> bool {
    let behind = |marker: usize| cell[marker..].chars().next().is_some_and(char::is_whitespace);
    let hashes = cell.bytes().take_while(|b| *b == b'#').count();
    let digits = cell.bytes().take_while(u8::is_ascii_digit).count();
    cell.starts_with(['>', '|'])
        || cell.starts_with("```")
        || cell.starts_with("~~~")
        || (cell.starts_with(['-', '*', '+']) && behind(1))
        || (hashes > 0 && behind(hashes))
        || (digits > 0 && matches!(cell.as_bytes().get(digits), Some(b'.' | b')')) && behind(digits + 1))
}

/// Whether some separator has no space behind it. In prose each one has, also where the cells are few and short.
fn separators_stand_alone(rows: &[Vec<String>]) -> bool {
    rows.iter()
        .any(|row| row.iter().skip(1).any(|cell| !cell.starts_with(' ')))
}

fn is_number(cell: &str) -> bool {
    let cell = cell.trim();
    let cell = cell.strip_prefix(['+', '-', '−']).unwrap_or(cell);
    let cell = cell.strip_prefix(['$', '€', '£', '¥']).unwrap_or(cell);
    let cell = cell.strip_suffix(['%', '€', '$', '£', '¥']).unwrap_or(cell);
    cell.bytes().any(|b| b.is_ascii_digit())
        && cell
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '\'' | ' '))
}

fn table_of(rows: &[Vec<String>]) -> Table {
    let width = rows.first().map_or(0, Vec::len);
    let body = rows.get(1..).unwrap_or_default();
    let alignments = (0..width)
        .map(|column| {
            if !body.is_empty() && body.iter().all(|row| is_number(&row[column])) {
                Align::Right
            } else {
                Align::None
            }
        })
        .collect();
    Table {
        alignments,
        rows: rows
            .iter()
            .map(|row| row.iter().map(|cell| cell_of(cell)).collect())
            .collect(),
    }
}

fn cell_of(text: &str) -> Cell {
    let mut spans = Vec::new();
    for line in text.trim().lines() {
        if !spans.is_empty() {
            spans.push(Span::of(Piece::Break));
        }
        spans.push(Span::text(line));
    }
    Cell::new(spans)
}

#[cfg(test)]
mod tests {
    use splat_markdown::doc::BlockKind;
    use splat_testkit::{Rng, rounds};

    use super::*;

    fn markdown(text: &str) -> Option<String> {
        from_text(text).map(Rich::into_markdown)
    }

    #[test]
    fn tab_separated_text_is_a_table_also_on_one_line() {
        assert_eq!(markdown("a\tb\n1\t2\n").unwrap(), "| a | b |\n|---:|---:|\n| 1 | 2 |\n");
        assert_eq!(markdown("a\tb").unwrap(), "| a | b |\n|---|---|\n");
        assert_eq!(
            markdown("\r\n\r\na\tb\r\nc\td\r\n\r\n").unwrap(),
            "| a | b |\n|---|---|\n| c | d |\n"
        );
        assert_eq!(markdown("a\tb\rc\td\r").unwrap(), "| a | b |\n|---|---|\n| c | d |\n");
    }

    #[test]
    fn lines_of_different_widths_are_no_table() {
        assert_eq!(markdown("a\tb\nc\n"), None);
        assert_eq!(markdown("a\tb\n\nc\td\n"), None);
        assert_eq!(markdown("a;b;c\nd;e\n"), None);
        assert_eq!(markdown("a,b\nc\n"), None);
        assert_eq!(markdown("kein Trenner\nnirgends"), None);
        assert_eq!(markdown(""), None);
        assert_eq!(markdown("\n\n"), None);
    }

    #[test]
    fn indentation_and_trailing_separators_make_no_column() {
        assert_eq!(markdown("\tfoo\n\tbar\n"), None);
        assert_eq!(markdown("\tfoo"), None);
        assert_eq!(markdown("foo\t\nbar\t\n"), None);
        assert_eq!(markdown("\t\n\t\n"), None);
        assert_eq!(markdown(",a\n,b\n"), None);
    }

    #[test]
    fn semicolons_and_commas_with_real_data_are_tables() {
        assert_eq!(
            markdown("Name;Alter;Ort\nBob;12;Bern\nEva;9;Olten\n").unwrap(),
            "| Name | Alter | Ort |\n|---|---:|---|\n| Bob | 12 | Bern |\n| Eva | 9 | Olten |\n"
        );
        assert_eq!(
            markdown("id,name,price\n1,Apple,1.50\n2,Pear,2.00\n").unwrap(),
            "| id | name | price |\n|---:|---|---:|\n| 1 | Apple | 1.50 |\n| 2 | Pear | 2.00 |\n"
        );
        assert_eq!(
            markdown("Name, Age\nBob, 12\n").unwrap(),
            "| Name | Age |\n|---|---:|\n| Bob | 12 |\n"
        );
        assert_eq!(markdown("a,b\nc,d").unwrap(), "| a | b |\n|---|---|\n| c | d |\n");
    }

    #[test]
    fn one_empty_cell_at_the_end_of_a_line_does_not_end_a_table() {
        assert_eq!(
            markdown("a;b;c\n1;2;\n3;4;5\n").unwrap(),
            "| a | b | c |\n|---:|---:|---|\n| 1 | 2 | |\n| 3 | 4 | 5 |\n"
        );
    }

    #[test]
    fn a_tab_stands_before_a_semicolon_and_a_semicolon_before_a_comma() {
        assert_eq!(
            markdown("a;b\tc,d\ne;f\tg,h\n").unwrap(),
            "| a;b | c,d |\n|---|---|\n| e;f | g,h |\n"
        );
        assert_eq!(
            markdown("a,5;b,5\nx,y;z,w\n").unwrap(),
            "| a,5 | b,5 |\n|---|---|\n| x,y | z,w |\n"
        );
        assert_eq!(markdown("a\tb\nc\nd,e\nf,g"), None, "neither width fits");
    }

    #[test]
    fn quotes_follow_rfc_4180() {
        assert_eq!(
            markdown("\"a,b\",c\n\"say \"\"hi\"\"\",d\n").unwrap(),
            "| a,b | c |\n|---|---|\n| say \"hi\" | d |\n"
        );
        assert_eq!(
            markdown("a\t\"zwei\nZeilen\"\nb\tc\n").unwrap(),
            "| a | zwei<br>Zeilen |\n|---|---|\n| b | c |\n"
        );
        assert_eq!(
            markdown("\"a\" b\tc\nd\te\n").unwrap(),
            "| \"a\" b | c |\n|---|---|\n| d | e |\n"
        );
        assert_eq!(
            markdown("x\"y\tc\nd\te\n").unwrap(),
            "| x\"y | c |\n|---|---|\n| d | e |\n"
        );
        assert_eq!(
            markdown("\"nie zu\tc\nd\te\n").unwrap(),
            "| \"nie zu | c |\n|---|---|\n| d | e |\n"
        );
        assert_eq!(markdown("\"a\"").map(|_| ()), None);
        assert_eq!(
            markdown("\"a\"\tb\r\n\"c\"\td").unwrap(),
            "| a | b |\n|---|---|\n| c | d |\n"
        );
    }

    #[test]
    fn prose_with_commas_stays_text() {
        for prose in [
            "Roses are red, violets are blue,\nsugar is sweet, and so are you.",
            "Roses are red,\nViolets are blue,\n",
            "Hello, world\nHow are you, my friend\n",
            "Dear John, how are you\nI am fine, thank you\n",
            "Apples, pears\nOranges, plums\n",
            "First we went there, and then we came back, and it was late.\nSo we slept, and woke up, and that was all.\n",
            "Wir gingen hin, wir kamen her\nEs war spät, wir waren müde\n",
            "foo(a, b);\nbar(c, d);\n",
            "let a = 1, b = 2\nlet c = 3, d = 4\n",
            "Mit freundlichen Grüssen, Sven\nBern, im November\n",
            "a,b.\nc,d.\n",
            "a,b!\nc,d?\n",
            "a,b:\nc,d:\n",
            "Eins; zwei\nDrei; vier\n",
            "x = 1; y = 2;\nz = 3; w = 4;\n",
        ] {
            assert_eq!(markdown(prose), None, "{prose}");
        }
        let long_cell = format!("{},b\nc,d\n", "x".repeat(LONGEST_CELL + 1));
        assert_eq!(markdown(&long_cell), None);
        let fitting = format!("{},b\nc,d\n", "x".repeat(LONGEST_CELL));
        assert!(markdown(&fitting).is_some());
        assert_eq!(markdown("eins zwei drei vier fünf sechs,b\nc,d\n"), None);
        assert!(markdown("eins zwei drei vier fünf,b\nc,d\n").is_some());
        assert!(markdown("a,Eins zwei drei\nb,Vier fünf sechs\n").is_some());
    }

    #[test]
    fn markdown_stays_text() {
        for text in [
            "- a, b\n- c, d\n",
            "* a,b\n* c,d\n",
            "+ a,b\n+ c,d\n",
            "1. a,b\n2. c,d\n",
            "1) a,b\n2) c,d\n",
            "# a,b\n## c,d\n",
            "> a,b\n> c,d\n",
            "| a,b |\n| c,d |\n",
            "```a,b\nc,d\n",
            "~~~a,b\nc,d\n",
        ] {
            assert_eq!(markdown(text), None, "{text}");
        }
        assert!(markdown("-5,x\n-6,y\n").is_some(), "a negative number is no list");
        assert!(markdown("#a,b\n#c,d\n").is_some(), "a tag is no heading");
        assert!(markdown("1.5,x\n2.5,y\n").is_some(), "a decimal is no list");
    }

    #[test]
    fn a_first_line_of_numbers_is_no_header() {
        assert_eq!(markdown("1,5\n2,5\n3,5\n"), None);
        assert_eq!(markdown("1,2,3\n4,5,6\n"), None);
        assert_eq!(markdown("1;2\n3;4\n"), None);
        assert_eq!(
            markdown("1\t2\n3\t4\n").unwrap(),
            "| 1 | 2 |\n|---:|---:|\n| 3 | 4 |\n",
            "tabs are clear enough"
        );
    }

    #[test]
    fn a_column_is_right_aligned_only_if_all_its_cells_are_numbers() {
        assert_eq!(
            markdown("a\tb\tc\tleer\n1\tx\t3\t\n2\t2\t-4,5 %\t\n").unwrap(),
            "| a | b | c | leer |\n|---:|---|---:|---|\n| 1 | x | 3 | |\n| 2 | 2 | -4,5 % | |\n"
        );
        for number in [
            "1", "-1", "+1", "−1", "1.5", "1,5", "1'000", "1 000", "50%", "$5", "€ 5", "5 €", "-$5", ".5",
        ] {
            assert!(is_number(number), "{number}");
        }
        for text in ["", " ", "a", "1a", "1-2", "%", "$", "1/2", "..."] {
            assert!(!is_number(text), "{text}");
        }
    }

    #[test]
    fn text_in_cells_stays_literal() {
        assert_eq!(
            markdown("a|b\t*c*\n[d](e)\t<f>\n").unwrap(),
            "| a\\|b | \\*c\\* |\n|---|---|\n| \\[d](e) | \\<f> |\n"
        );
    }

    #[test]
    fn cells_of_nothing_but_spaces_give_no_table() {
        assert_eq!(markdown(" \t \n \t \n"), None);
    }

    #[test]
    fn any_text_gives_a_table_that_reads_back_or_none() {
        const PIECES: &[&str] = &[
            "a", "Wort", "1", "2,5", " ", "\t", ";", ",", "\"", "\"\"", "\n", "\r\n", "\r", "ä", "😀", "- ", "# ", ".",
            "|", "*",
        ];
        let mut rng = Rng::new(0x7AB1_0001);
        for _ in 0..rounds(20_000) {
            let text: String = (0..rng.below(24)).map(|_| *rng.pick(PIECES)).collect();
            if let Some(markdown) = markdown(&text) {
                let doc = splat_markdown::reader::parse(&markdown);
                let first = doc.blocks().first().map(|block| &block.kind);
                assert!(
                    matches!(first, Some(BlockKind::Table { .. })),
                    "{text:?} -> {markdown:?}"
                );
            }
        }
    }

    #[test]
    fn quote_after_quote_takes_no_quadratic_time() {
        let hostile = "\"x,\"\",".repeat(200_000) + "\"y";
        assert_eq!(markdown(&hostile).map(|_| ()), None);
        let unmatched = "\"a,".repeat(200_000);
        assert_eq!(markdown(&unmatched).map(|_| ()), None);
    }
}
