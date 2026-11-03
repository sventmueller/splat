use std::ops::Range;

use super::Edit;
use super::anchors::heading_anchors;
use super::inline::{leaf_at, longest_backtick_run};
use super::lines::line_span;
use crate::i18n::{t, tf};
use crate::markdown::doc::{AlertKind, Block, BlockKind, Doc, InlineKind, block_path, walk_blocks, walk_inlines};
use crate::markdown::text::{Byte, TextAt, line_end, line_start, lines_with_ends, units};

/// Inserts `block` as its own block at the cursor, adding blank lines where needed.
/// `cursor` is the offset inside `block` where the caret should land.
pub fn insert_block(text: &str, sel: Range<Byte>, block: &str, cursor: usize) -> Edit {
    let at = if sel.is_empty() { sel.start } else { sel.end };
    let end_of_line = line_end(text, at);
    let line_begin = line_start(text, at);
    let insert_at = if text.at(line_begin..end_of_line).trim().is_empty() {
        line_begin
    } else {
        end_of_line
    };
    let before = text.upto(insert_at);
    let after = text.from(insert_at);
    let lead = if before.is_empty() || before.ends_with("\n\n") {
        ""
    } else if before.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let trail = if after.is_empty() {
        "\n"
    } else if after.starts_with("\n\n") {
        ""
    } else if after.starts_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let replacement = format!("{lead}{block}{trail}");
    let caret = insert_at + lead.len() + cursor;
    Edit {
        range: insert_at..insert_at,
        replacement,
        selection: caret..caret,
    }
}

pub fn insert_code_block(text: &str, sel: Range<Byte>, lang: &str) -> Edit {
    if sel.is_empty() {
        let block = format!("```{lang}\n\n```");
        return insert_block(text, sel, &block, lang.len() + 4);
    }
    let span = line_span(text, &sel);
    let body = text.at(span.clone());
    let fence = "`".repeat(longest_backtick_run(body).max(2) + 1);
    let replacement = format!("{fence}{lang}\n{body}\n{fence}");
    let start = span.start + fence.len() + lang.len() + 1;
    Edit {
        range: span,
        replacement,
        selection: start..start + body.len(),
    }
}

/// Sets the language of the fenced code block at the caret, or inserts a new code block in that language.
pub fn code_block_language(doc: &Doc, sel: Range<Byte>, lang: &str) -> Edit {
    let text = doc.text();
    let block = block_path(&doc.blocks, sel.start)
        .into_iter()
        .rev()
        .find(|b| matches!(b.kind, BlockKind::CodeBlock { fenced: true, .. }));
    let Some(block) = block else {
        return insert_code_block(text, sel, lang);
    };
    let fence_end = line_end(text, block.range.start);
    let fence = text.at(block.range.start..fence_end).trim_start();
    let fence_start = fence_end - fence.len();
    let marker = fence.chars().next().unwrap_or('`');
    let info_start = fence_start + fence.chars().take_while(|c| *c == marker).count();
    // Only the first word of the info string is the language; what follows, as `title="x"`, stays.
    let info = text.at(info_start..fence_end);
    let word_start = info_start + (info.len() - info.trim_start().len());
    let word_end = text
        .at(word_start..fence_end)
        .find(char::is_whitespace)
        .map_or(fence_end, |i| word_start + i);
    Edit {
        range: word_start..word_end,
        replacement: lang.to_string(),
        selection: sel,
    }
    .keeping_selection_after(word_start, word_end - word_start, lang.len())
}

/// Formats the paragraph at the caret (or the selected lines) as an alert of `kind`, like a quote. On an alert it
/// changes the type, or removes the alert when it already has this type; on an empty line it inserts a new one.
pub fn insert_alert(doc: &Doc, sel: Range<Byte>, kind: AlertKind) -> Edit {
    let text = doc.text();
    let existing = block_path(&doc.blocks, sel.start)
        .into_iter()
        .rev()
        .find_map(|b| match &b.kind {
            BlockKind::Alert {
                marker, kind: current, ..
            } => Some((b.range.clone(), marker.clone(), *current)),
            _ => None,
        });
    if let Some((block, marker, current)) = existing {
        if current == kind {
            return remove_alert(text, &block);
        }
        let keyword = marker.start + 2..marker.start + text.at(marker.clone()).find(']').unwrap_or(2);
        let keyword_len = units(&keyword);
        return Edit {
            range: keyword.clone(),
            replacement: kind.keyword().to_string(),
            selection: sel,
        }
        .keeping_selection_after(keyword.start, keyword_len, kind.keyword().len());
    }
    let head = format!("> [!{}]", kind.keyword());
    let paragraph = leaf_at(doc, sel.start)
        .filter(|b| sel.is_empty() && matches!(b.kind, BlockKind::Paragraph))
        .filter(|b| block_path(&doc.blocks, b.range.start).len() == 1)
        .map(|b| b.range.clone());
    let lines = match (sel.is_empty(), paragraph) {
        (false, _) => line_span(text, &sel),
        (true, Some(p)) => line_span(text, &p),
        (true, None) => {
            let block = format!("{head}\n> {}", t("Text"));
            let mut e = insert_block(text, sel, &block, head.len() + 3);
            e.selection = e.selection.start..e.selection.start + t("Text").len();
            return e;
        }
    };
    let body: Vec<String> = text
        .at(lines.clone())
        .split('\n')
        .map(|l| {
            if l.is_empty() {
                ">".to_string()
            } else {
                format!("> {l}")
            }
        })
        .collect();
    let replacement = format!("{head}\n{}", body.join("\n"));
    let end = lines.start + replacement.len();
    Edit {
        range: lines.clone(),
        replacement,
        selection: lines.start..end,
    }
}

/// Turns an alert back into plain paragraphs: drops the `[!TYPE]` line and the quote markers.
fn remove_alert(text: &str, block: &Range<Byte>) -> Edit {
    let lines = line_span(text, block);
    let all = text.at(lines.clone());
    // A list marker in front of the alert stays with its first line.
    let (first, _) = lines_with_ends(all).next().unwrap_or(("", ""));
    let marker = &first[..first.find('>').unwrap_or(0)];
    let marker = if marker.trim().is_empty() { "" } else { marker };
    let mut replacement = String::new();
    for (n, (line, ending)) in lines_with_ends(all).skip(1).enumerate() {
        let quote = line.find('>').filter(|at| line[..*at].trim().is_empty());
        let (indent, rest) = quote.map_or(("", line), |at| (&line[..at], &line[at + 1..]));
        let content = rest.strip_prefix(' ').unwrap_or(rest);
        if n == 0 {
            replacement.push_str(marker);
        } else {
            replacement.push_str(indent);
        }
        replacement.push_str(content);
        replacement.push_str(ending);
    }
    if replacement.is_empty() {
        replacement.push_str(marker.trim_end());
    }
    let end = lines.start + replacement.len();
    Edit {
        range: lines.clone(),
        replacement,
        selection: lines.start..end,
    }
}

/// Inserts a linked table of contents, or regenerates the one at the caret. A single top-level title is left out.
pub fn insert_toc(doc: &Doc, sel: Range<Byte>) -> Option<Edit> {
    let text = doc.text();
    let mut headings = heading_anchors(doc);
    if headings.iter().filter(|h| h.level == 1).count() == 1 && headings.first().is_some_and(|h| h.level == 1) {
        headings.remove(0);
    }
    let top = headings.iter().map(|h| h.level).min()?;
    let lines: Vec<String> = headings
        .iter()
        .filter(|h| h.level <= top + 2)
        .map(|h| {
            let indent = "  ".repeat((h.level - top) as usize);
            format!("{indent}- [{}](#{})", escape_label(&h.title), h.anchor)
        })
        .collect();
    let toc = lines.join("\n");
    let existing = block_path(&doc.blocks, sel.start)
        .into_iter()
        .find(|b| matches!(b.kind, BlockKind::List { .. }) && is_toc(b));
    Some(match existing {
        Some(list) => {
            let end = Byte::new(text.upto(list.range.end).trim_end_matches('\n').len());
            Edit {
                range: list.range.start..end,
                replacement: toc.clone(),
                selection: list.range.start..list.range.start + toc.len(),
            }
        }
        None => {
            let mut e = insert_block(text, sel, &toc, toc.len());
            let start = e.selection.start - toc.len();
            e.selection = start..start + toc.len();
            e
        }
    })
}

/// A list whose every link points into the document.
fn is_toc(list: &Block) -> bool {
    let mut links = 0;
    let mut foreign = false;
    walk_blocks(std::slice::from_ref(list), &mut |b| {
        walk_inlines(b.inlines(), &mut |i| {
            if let InlineKind::Link { url, .. } = &i.kind {
                links += 1;
                foreign |= !url.starts_with('#');
            }
        });
    });
    links > 0 && !foreign
}

pub fn insert_rule(text: &str, sel: Range<Byte>) -> Edit {
    insert_block(text, sel, "---", 3)
}

pub fn insert_table(text: &str, sel: Range<Byte>, rows: usize, cols: usize) -> Edit {
    let table = crate::markdown::table::new_table(rows, cols);
    let mut e = insert_block(text, sel, &table, 2);
    let start = e.selection.start;
    e.selection = start..start + tf("Spalte {}", &["1"]).len();
    e
}

pub fn insert_link(sel: Range<Byte>, url: &str, label: &str) -> Edit {
    let label = if label.trim().is_empty() { url } else { label };
    let replacement = format!("[{}]({})", escape_label(label), link_destination(url));
    let end = sel.start + replacement.len();
    Edit {
        range: sel,
        replacement,
        selection: end..end,
    }
}

pub fn insert_image(sel: Range<Byte>, alt: &str, path: &str) -> Edit {
    let replacement = format!("![{}]({})", escape_label(alt), link_destination(path));
    let end = sel.start + replacement.len();
    Edit {
        range: sel,
        replacement,
        selection: end..end,
    }
}

fn escape_label(s: &str) -> String {
    s.replace('\\', "\\\\").replace('[', "\\[").replace(']', "\\]")
}

pub fn link_destination(url: &str) -> String {
    if url.contains([' ', '(', ')']) {
        format!("<{}>", url.replace('>', "%3E"))
    } else {
        url.to_string()
    }
}

pub fn insert_formula(text: &str, sel: Range<Byte>) -> Edit {
    let selected = text.at(sel.clone());
    if !selected.trim().is_empty() && !selected.contains('\n') {
        let replacement = format!("${}$", selected.trim());
        let start = sel.start + 1;
        let end = sel.start + replacement.len() - 1;
        return Edit {
            range: sel,
            replacement,
            selection: start..end,
        };
    }
    if selected.trim().is_empty() {
        return insert_block(text, sel, "$$\n\n$$", 3);
    }
    let body = selected.trim();
    let replacement = format!("$$\n{body}\n$$");
    let start = sel.start + 3;
    Edit {
        range: sel,
        replacement,
        selection: start..start + body.len(),
    }
}

pub fn insert_diagram(text: &str, sel: Range<Byte>, template: &str) -> Edit {
    let block = format!("```mermaid\n{template}\n```");
    insert_block(text, sel, &block, "```mermaid\n".len())
}

pub fn insert_footnote(text: &str, sel: Range<Byte>) -> Edit {
    let mut n = 1;
    while text.contains(&format!("[^{n}]")) {
        n += 1;
    }
    let at = sel.end;
    let reference = format!("[^{n}]");
    let (head, tail) = (text.upto(at), text.from(at));
    let tail_trimmed = tail.trim_end_matches('\n');
    let sep = if format!("{head}{tail_trimmed}").is_empty() {
        ""
    } else {
        "\n\n"
    };
    let definition = format!("{sep}[^{n}]: ");
    let replacement = format!("{reference}{tail_trimmed}{definition}\n");
    let caret = at + reference.len() + tail_trimmed.len() + definition.len();
    Edit {
        range: at..text.end(),
        replacement,
        selection: caret..caret,
    }
}

#[cfg(test)]
mod tests {
    use super::super::inline::active_at;
    use super::super::tests::{applied, run};
    use super::*;
    use crate::markdown::reader::parse;
    use crate::markdown::text::bytes;

    #[test]
    fn the_info_string_keeps_what_follows_the_language() {
        let t = "```ignore title=\"x\"\ncode\n```\n";
        assert_eq!(
            applied(t, &code_block_language(&parse(t), bytes(25..25), "rust")),
            "```rust title=\"x\"\ncode\n```\n"
        );
    }

    #[test]
    fn removing_an_alert_in_a_list_keeps_the_item() {
        let t = "- > [!NOTE]\n  > x\n";
        assert_eq!(applied(t, &remove_alert(t, &bytes(0..t.len() - 1))), "- x\n");
    }

    #[test]
    fn blocks_get_blank_lines() {
        let (s, _) = run("Absatz", 3..3, |d, r| insert_rule(d.text(), r));
        assert_eq!(s, "Absatz\n\n---\n");
        let (s, sel) = run("A\n\nB\n", 0..0, |d, r| insert_table(d.text(), r, 2, 2));
        assert!(s.starts_with("A\n\n| Spalte 1 | Spalte 2 |"));
        assert!(s.ends_with("|\n\nB\n"));
        assert_eq!(sel, "Spalte 1");
    }

    #[test]
    fn footnote_appends_definition() {
        let (s, _) = run("Text[^1]\n\n[^1]: a\n", 4..4, |d, r| insert_footnote(d.text(), r));
        assert_eq!(s, "Text[^2][^1]\n\n[^1]: a\n\n[^2]: \n");
    }

    #[test]
    fn code_block_language_is_inserted_or_changed() {
        let (s, _) = run("a\n", 0..0, |d, r| code_block_language(d, r, "python"));
        assert!(s.contains("```python\n"), "{s:?}");
        let t = "```js\nlet x\n```\n";
        let (s, sel) = run(t, 6..9, |d, r| code_block_language(d, r, "rust"));
        assert_eq!(s, "```rust\nlet x\n```\n");
        assert_eq!(sel, "let");
        assert_eq!(active_at(&parse(&s), Byte::new(9)).code_lang.as_deref(), Some("rust"));
    }

    #[test]
    fn alerts_are_inserted_wrapped_and_retyped() {
        let (s, _) = run("Absatz\n", 2..2, |d, r| insert_alert(d, r, AlertKind::Tip));
        assert_eq!(s, "> [!TIP]\n> Absatz\n");
        let (s, sel) = run("A\n\n", 2..2, |d, r| insert_alert(d, r, AlertKind::Tip));
        assert_eq!(s, "A\n\n> [!TIP]\n> Text\n\n");
        assert_eq!(sel, "Text");
        let (s, _) = run("> [!NOTE]\n> Inhalt\n", 12..12, |d, r| {
            insert_alert(d, r, AlertKind::Note)
        });
        assert_eq!(s, "Inhalt\n");
        let (s, _) = run("a\nb\n", 0..3, |d, r| insert_alert(d, r, AlertKind::Note));
        assert_eq!(s, "> [!NOTE]\n> a\n> b\n");
        let (s, sel) = run("> [!NOTE]\n> Inhalt\n", 12..18, |d, r| {
            insert_alert(d, r, AlertKind::Warning)
        });
        assert_eq!(s, "> [!WARNING]\n> Inhalt\n");
        assert_eq!(sel, "Inhalt");
    }

    #[test]
    fn table_of_contents_links_headings_and_updates_in_place() {
        let t = "# Titel\n\n## Erste Ebene\n\n### Größe & Maß\n\n## Erste Ebene\n";
        let d = parse(t);
        let e = insert_toc(&d, bytes(8..8)).unwrap();
        let mut s = t.to_string();
        e.apply_to(&mut s);
        assert!(
            s.contains(
                "- [Erste Ebene](#erste-ebene)\n  - [Größe & Maß](#größe--maß)\n- [Erste Ebene](#erste-ebene-1)"
            ),
            "{s}"
        );
        let s2 = s.replace("## Erste Ebene\n\n### Größe", "## Neu\n\n### Größe");
        let at = s2.find("- [").unwrap() + 2;
        let e = insert_toc(&parse(&s2), bytes(at..at)).unwrap();
        let mut s3 = s2.clone();
        e.apply_to(&mut s3);
        assert_eq!(s3.matches("](#").count(), 3);
        assert!(s3.contains("- [Neu](#neu)"), "{s3}");
    }

    #[test]
    fn a_block_in_an_empty_place_gets_no_blank_line_before_it() {
        assert_eq!(run("", 0..0, |d, r| insert_rule(d.text(), r)).0, "---\n");
        assert_eq!(run("a\n\n", 3..3, |d, r| insert_rule(d.text(), r)).0, "a\n\n---\n");
    }

    #[test]
    fn a_code_block_is_longer_than_the_fences_inside_and_the_caret_lands_in_it() {
        assert_eq!(insert_code_block("", bytes(0..0), "rs").selection, bytes(6..6));
        let code = |t: &str| run(t, 0..t.len(), |d, r| insert_code_block(d.text(), r, ""));
        assert_eq!(code("x"), ("```\nx\n```".to_string(), "x".to_string()));
        assert_eq!(code("a```b"), ("````\na```b\n````".to_string(), "a```b".to_string()));
    }

    #[test]
    fn a_caret_in_the_language_of_a_code_block_stays_in_it() {
        let t = "```rust\nx\n```\n";
        let edit = code_block_language(&parse(t), bytes(5..5), "go");
        assert_eq!(edit.selection, bytes(5..5));
    }

    #[test]
    fn an_alert_goes_behind_a_heading_and_its_removal_selects_the_text() {
        let (s, _) = run("# T\n", 1..1, |d, r| insert_alert(d, r, AlertKind::Tip));
        assert_eq!(s, "# T\n\n> [!TIP]\n> Text\n\n");
        let (s, sel) = run("x\n\n> [!NOTE]\n> a\n", 15..15, |d, r| {
            insert_alert(d, r, AlertKind::Note)
        });
        assert_eq!(s, "x\n\na\n");
        assert_eq!(sel, "a");
    }

    fn toc(text: &str, at: usize) -> (String, String) {
        run(text, at..at, |d, r| insert_toc(d, r).unwrap())
    }

    #[test]
    fn only_a_single_first_title_is_left_out_of_the_table_of_contents() {
        assert!(toc("## a\n\n# b\n\n## c\n", 0).0.contains("[a](#a)"));
        let (_, listed) = toc("# a\n\n# b\n", 0);
        assert_eq!(listed, "- [a](#a)\n- [b](#b)");
        let (_, listed) = toc("# a\n\n## b\n", 0);
        assert_eq!(listed, "- [b](#b)");
    }

    #[test]
    fn the_table_of_contents_goes_three_levels_deep() {
        let (_, listed) = toc("# x\n\n# y\n\n## a\n\n### b\n\n#### c\n", 0);
        assert_eq!(listed, "- [x](#x)\n- [y](#y)\n  - [a](#a)\n    - [b](#b)");
    }

    #[test]
    fn a_table_of_contents_replaces_only_a_list_of_links_into_the_document() {
        let again = |list: &str| {
            let t = format!("# h\n\n# i\n\n{list}\n");
            let at = t.find(list).unwrap() + 2;
            let (s, listed) = toc(&t, at);
            assert_eq!(listed, "- [h](#h)\n- [i](#i)");
            list.lines().all(|line| s.contains(line))
        };
        assert!(!again("- [a](#a)"), "a table of contents is regenerated");
        assert!(again("- a"), "no links");
        assert!(again("- [a](http://x)"), "links elsewhere");
        assert!(again("- [a](#a)\n- [b](http://x)"), "some links elsewhere");
    }

    #[test]
    fn a_new_link_or_image_leaves_the_caret_behind_it_and_quotes_awkward_addresses() {
        let t = "ab cd";
        let edit = insert_link(bytes(3..3), "u", "");
        let mut out = t.to_string();
        edit.apply_to(&mut out);
        assert_eq!(out, "ab [u](u)cd");
        assert_eq!(edit.selection, bytes(9..9));
        let edit = insert_image(bytes(3..3), "a", "p q");
        let mut out = t.to_string();
        edit.apply_to(&mut out);
        assert_eq!(out, "ab ![a](<p q>)cd");
        assert_eq!(edit.selection, bytes(14..14));
        assert_eq!(link_destination("u"), "u");
        assert_eq!(link_destination("a(b"), "<a(b>");
        assert_eq!(link_destination("a b>c"), "<a b%3Ec>");
    }

    #[test]
    fn a_formula_wraps_a_selection_and_otherwise_gets_its_own_block() {
        let formula = |t: &str, r: Range<usize>| run(t, r, |d, r| insert_formula(d.text(), r));
        assert_eq!(formula("a x b", 2..3), ("a $x$ b".to_string(), "x".to_string()));
        assert_eq!(formula("a  x b", 2..4), ("a $x$ b".to_string(), "x".to_string()));
        assert_eq!(
            formula("c a\nb", 2..5),
            ("c $$\na\nb\n$$".to_string(), "a\nb".to_string())
        );
        assert_eq!(formula("", 0..0).0, "$$\n\n$$\n");
        assert_eq!(insert_formula("", bytes(0..0)).selection, bytes(3..3));
        assert_eq!(
            formula("a  b", 1..3).0,
            "a  b\n\n$$\n\n$$\n",
            "only spaces are no formula"
        );
    }

    #[test]
    fn a_new_footnote_leaves_the_caret_in_its_definition() {
        let t = "ab cd\n";
        let edit = insert_footnote(t, bytes(2..2));
        let mut s = t.to_string();
        edit.apply_to(&mut s);
        assert_eq!(s, "ab[^1] cd\n\n[^1]: \n");
        assert_eq!(s.from(edit.selection.start), "\n");
    }
}
