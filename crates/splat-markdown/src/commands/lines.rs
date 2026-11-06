use std::ops::Range;

use super::Edit;
use crate::doc::{BlockKind, Doc};
use crate::text::{
    Byte, ContainerMark, TextAt, block_indent, container_mark, growth, indent_len, line_end, line_start,
    lines_with_ends, next_quote_mark, shifted,
};

/// The kinds of list the list commands write and the menus check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListStyle {
    /// `- item`.
    Bullet,
    /// `1. item`.
    Numbered,
    /// `- [ ] item`, a task list.
    Task,
}

/// Byte range of the full lines touched by `sel`, without the final newline.
pub fn line_span(text: &str, sel: &Range<Byte>) -> Range<Byte> {
    let start = line_start(text, sel.start);
    let mut end_anchor = sel.end;
    if sel.end > sel.start {
        let selected = text.at(sel.clone());
        end_anchor -= if selected.ends_with("\r\n") {
            2
        } else {
            usize::from(selected.ends_with(['\n', '\r']))
        };
    }
    start..line_end(text, end_anchor).max(start)
}

/// Container and block prefixes at the start of a source line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinePrefix {
    /// Byte offset in the line behind its quote marks and the space after each; 0 without one.
    pub quote_end: usize,
    /// The byte range of the list marker, with any checkbox and the spaces behind them, and the kind of list it makes.
    pub list: Option<(Range<usize>, ListStyle)>,
    /// The byte range of the heading's `#` marks, with the indentation in front and the space behind, and the level
    /// they give.
    pub heading: Option<(Range<usize>, u8)>,
    /// Byte offset in the line where its text starts, behind all of these.
    pub content: usize,
}

impl LinePrefix {
    /// Where the first `quotes` quote marks of `line` end, with the space that belongs to each.
    pub fn quote_end_of(line: &str, quotes: usize) -> usize {
        let mut end = 0;
        for _ in 0..quotes {
            let Some(mark) = next_quote_mark(line, end) else {
                break;
            };
            end = mark + 1 + usize::from(line[mark + 1..].starts_with(' '));
        }
        end
    }

    /// The prefixes `line` starts with.
    pub fn parse(line: &str) -> LinePrefix {
        let b = line.as_bytes();
        let quote_end = LinePrefix::quote_end_of(line, usize::MAX);
        let i = quote_end;
        let mut p = LinePrefix {
            quote_end,
            content: i,
            ..Default::default()
        };
        let marker_start = i + indent_len(&line[i..]);
        let mut j = marker_start;
        let space_or_end = |c: Option<&u8>| c.is_none_or(|c| matches!(c, b' ' | b'\t'));
        let kind = container_mark(&line[j..]).and_then(|(mark, len)| {
            let style = match mark {
                ContainerMark::Quote => return None,
                ContainerMark::Bullet => ListStyle::Bullet,
                ContainerMark::Number if len <= 10 => ListStyle::Numbered,
                ContainerMark::Number => return None,
            };
            space_or_end(b.get(j + len)).then(|| {
                j += len;
                style
            })
        });
        if let Some(mut k) = kind {
            while j < b.len() && matches!(b[j], b' ' | b'\t') {
                j += 1;
            }
            if b[j..].len() >= 3 && b[j] == b'[' && matches!(b[j + 1], b' ' | b'x' | b'X') && b[j + 2] == b']' {
                j += 3;
                while j < b.len() && matches!(b[j], b' ' | b'\t') {
                    j += 1;
                }
                k = ListStyle::Task;
            }
            p.list = Some((marker_start..j, k));
            p.content = j;
        }
        let rest = &line[p.content..];
        if let Some(indent) = block_indent(rest) {
            let hashes = rest[indent..].bytes().take_while(|&c| c == b'#').count();
            let after_hashes = p.content + indent + hashes;
            if (1..=6).contains(&hashes) && space_or_end(b.get(after_hashes)) {
                let end = after_hashes + usize::from(line.len() > after_hashes);
                p.heading = Some((p.content..end, u8::try_from(hashes).unwrap_or(6)));
                p.content = end;
            }
        }
        p
    }
}

fn map_lines(text: &str, sel: &Range<Byte>, mut f: impl FnMut(usize, &str, &LinePrefix) -> String) -> Edit {
    let span = line_span(text, sel);
    let mut out = String::new();
    for (n, (line, ending)) in lines_with_ends(text.at(span.clone())).enumerate() {
        out.push_str(&f(n, line, &LinePrefix::parse(line)));
        out.push_str(ending);
    }
    let delta = growth(span.start + out.len(), span.end);
    let new_end = shifted(span.end, delta);
    let selection = if sel.is_empty() {
        let line_end_after = new_end;
        let pos = shifted(sel.start, delta).clamp(span.start, line_end_after);
        let pos = if sel.start == span.start {
            span.start + line_end(&out, Byte::default()).get()
        } else {
            pos
        };
        pos..pos
    } else {
        span.start..new_end
    };
    Edit {
        range: span,
        replacement: out,
        selection,
    }
}

/// Makes the lines of the selection headings of `level`, or plain text for level 0, replacing the underline of a setext
/// title.
pub fn set_heading(doc: &Doc, sel: Range<Byte>, level: u8) -> Edit {
    let text = doc.text();
    let span = line_span(text, &sel);
    let several_lines = text.at(span.clone()).contains(['\n', '\r']);
    let mut edit = map_lines(text, &sel, |_, line, p| {
        let start = p.heading.as_ref().map_or(p.content, |(r, _)| r.start);
        let body = &line[p.content..];
        if level == 0 {
            format!("{}{}", &line[..start], body)
        } else if several_lines && line[p.quote_end..].trim().is_empty() {
            line.to_string()
        } else {
            format!("{}{} {}", &line[..start], "#".repeat(level as usize), body)
        }
    });
    // The underline of a title written that way would otherwise stay behind as text or as a rule. Only the parse can tell
    // one from a rule that stands behind a heading, a list or a quote.
    let underlined = super::leaf_at(doc, shifted(span.end, -1).max(span.start))
        .is_some_and(|leaf| matches!(leaf.kind, BlockKind::Heading { setext: true, .. }));
    if underlined && let Some(end) = setext_underline_behind(text, &span) {
        edit.range.end = end;
    }
    edit
}

/// The end of a line of `=` or `-` right behind the text lines of `span`.
pub(super) fn setext_underline_behind(text: &str, span: &Range<Byte>) -> Option<Byte> {
    let last = lines_with_ends(text.at(span.clone())).last()?.0;
    let body = &last[LinePrefix::parse(last).content..];
    if body.trim().is_empty() {
        return None;
    }
    let behind = text.from(span.end);
    let ending = behind.len() - behind.trim_start_matches(['\r', '\n']).len();
    let line = span.end + ending;
    if ending == 0 || ending > 2 || text.at(span.end..line) == "\n\n" {
        return None;
    }
    let end = line_end(text, line);
    let under = text.at(line..end).trim();
    let under = under.trim_start_matches(['>', ' ']);
    let rule = !under.is_empty() && (under.bytes().all(|b| b == b'=') || under.bytes().all(|b| b == b'-'));
    rule.then_some(end)
}

/// Makes the lines of the selection items of a `style` list, or takes their markers off when all of them are such items
/// already; blank lines stay as they are.
pub fn toggle_list(text: &str, sel: Range<Byte>, style: ListStyle) -> Edit {
    let span = line_span(text, &sel);
    let all_same = lines_with_ends(text.at(span.clone()))
        .map(|(l, _)| l)
        .filter(|l| !l.trim().is_empty())
        .all(|l| LinePrefix::parse(l).list.is_some_and(|(_, k)| k == style));
    let mut number = 0;
    map_lines(text, &sel, |_, line, p| {
        if line.trim().is_empty() {
            return line.to_string();
        }
        let (keep, body) = match &p.list {
            Some((r, _)) => (&line[..r.start], &line[r.end..]),
            None => (&line[..p.quote_end], &line[p.quote_end..]),
        };
        if all_same {
            return format!("{keep}{body}");
        }
        number += 1;
        let marker = match style {
            ListStyle::Bullet => "- ".to_string(),
            ListStyle::Numbered => format!("{number}. "),
            ListStyle::Task => "- [ ] ".to_string(),
        };
        format!("{keep}{marker}{body}")
    })
}

/// Puts `> ` in front of the lines of the selection, or takes one quote level off when all of them are quoted.
pub fn toggle_quote(text: &str, sel: Range<Byte>) -> Edit {
    let span = line_span(text, &sel);
    let all_quoted = lines_with_ends(text.at(span.clone())).all(|(l, _)| LinePrefix::parse(l).quote_end > 0);
    map_lines(text, &sel, |_, line, _| {
        if all_quoted {
            let trimmed = line.trim_start_matches(' ');
            let rest = trimmed.strip_prefix('>').unwrap_or(trimmed);
            rest.strip_prefix(' ').unwrap_or(rest).to_string()
        } else if line.is_empty() {
            ">".to_string()
        } else {
            format!("> {line}")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::super::tests::{applied, run};
    use super::*;
    use crate::text::bytes;

    fn heading(t: &str, at: usize, level: u8) -> String {
        applied(t, &set_heading(&crate::reader::parse(t), bytes(at..at), level))
    }

    #[test]
    fn a_heading_level_replaces_the_underline_of_a_title() {
        let t = "Title\n=====\n\nx\n";
        assert_eq!(heading(t, 2, 2), "## Title\n\nx\n");
        assert_eq!(heading(t, 2, 0), "Title\n\nx\n");
        assert_eq!(heading("Title\n\n---\n", 2, 2), "## Title\n\n---\n");
    }

    #[test]
    fn a_rule_behind_a_line_is_no_underline_unless_the_parse_says_so() {
        assert_eq!(heading("# Title\n---\n", 2, 2), "## Title\n---\n");
        assert_eq!(heading("- a\n---\n", 2, 2), "- ## a\n---\n");
        assert_eq!(heading("> a\n---\n", 2, 2), "> ## a\n---\n");
        assert_eq!(
            heading("- a\n  ---\n", 2, 2),
            "- ## a\n",
            "the underline of a title in a list"
        );
        assert_eq!(
            heading("> a\n> ---\n", 2, 2),
            "> ## a\n",
            "the underline of a title in a quote"
        );
    }

    #[test]
    fn prefixes_may_be_followed_by_a_tab() {
        assert_eq!(
            applied("-\tfoo\n", &toggle_list("-\tfoo\n", bytes(2..2), ListStyle::Bullet)),
            "foo\n"
        );
        assert_eq!(heading("#\tfoo\n", 2, 2), "## foo\n");
    }

    #[test]
    fn a_heading_level_leaves_the_empty_lines_of_a_selection_alone() {
        let across = |t: &str, level: u8| {
            let mut out = t.to_string();
            set_heading(&crate::reader::parse(t), bytes(0..t.len()), level).apply_to(&mut out);
            out
        };
        assert_eq!(across("a\n\nb\n", 2), "## a\n\n## b\n");
        assert_eq!(across("> a\n>\n> b\n", 2), "> ## a\n>\n> ## b\n");
        assert_eq!(across("\n", 2), "## \n", "the only line, which the cursor stands on");
        assert_eq!(heading("", 0, 2), "## ");
    }

    #[test]
    fn a_heading_level_replaces_the_marks_of_an_indented_heading() {
        assert_eq!(heading(" # T\n", 2, 2), "## T\n");
        assert_eq!(heading("   # T\n", 2, 0), "T\n");
        assert_eq!(heading("> # T\n", 3, 2), "> ## T\n");
        assert_eq!(
            heading("    # T\n", 5, 2),
            "##     # T\n",
            "four spaces make code, not a heading"
        );
    }

    #[test]
    fn headings_lists_quotes() {
        let (s, _) = run("Titel\nText", 0..0, |d, r| set_heading(d, r, 2));
        assert_eq!(s, "## Titel\nText");
        let (s, _) = run("## Titel", 0..0, |d, r| set_heading(d, r, 0));
        assert_eq!(s, "Titel");
        let (s, _) = run("a\nb", 0..3, |d, r| toggle_list(d.text(), r, ListStyle::Numbered));
        assert_eq!(s, "1. a\n2. b");
        let (s, _) = run(&s, 0..s.len(), |d, r| toggle_list(d.text(), r, ListStyle::Task));
        assert_eq!(s, "- [ ] a\n- [ ] b");
        let (s, _) = run(&s, 0..s.len(), |d, r| toggle_list(d.text(), r, ListStyle::Task));
        assert_eq!(s, "a\nb");
        let (s, _) = run("a\n\nb", 0..4, |d, r| toggle_quote(d.text(), r));
        assert_eq!(s, "> a\n>\n> b");
        let (s, _) = run(&s, 0..s.len(), |d, r| toggle_quote(d.text(), r));
        assert_eq!(s, "a\n\nb");
    }

    #[test]
    fn the_lines_of_a_selection_end_before_a_line_it_only_touches() {
        assert_eq!(line_span("ab\ncd\n", &bytes(0..3)), bytes(0..2));
        assert_eq!(
            line_span("ab\ncd\n", &bytes(3..3)),
            bytes(3..5),
            "a caret has its own line"
        );
    }

    #[test]
    fn a_line_prefix_allows_three_spaces_before_each_mark() {
        assert_eq!(LinePrefix::parse(" > a").quote_end, 3);
        assert_eq!(LinePrefix::parse("   > a").quote_end, 5);
        assert_eq!(LinePrefix::parse("    > a").quote_end, 0, "that much is code");
        assert_eq!(LinePrefix::parse(">   > a").quote_end, 6);
        assert_eq!(LinePrefix::parse("  - a").list, Some((2..4, ListStyle::Bullet)));
    }

    #[test]
    fn a_line_prefix_needs_a_space_behind_its_marks() {
        assert_eq!(LinePrefix::parse("1.x").list, None);
        assert_eq!(LinePrefix::parse("1 ").list, None);
        assert_eq!(LinePrefix::parse("1. a").list, Some((0..3, ListStyle::Numbered)));
        assert_eq!(LinePrefix::parse("---").list, None);
    }

    #[test]
    fn a_line_prefix_finds_tasks_at_any_indent_and_a_heading_without_text() {
        assert_eq!(LinePrefix::parse("  - [x] a").list, Some((2..8, ListStyle::Task)));
        let heading = LinePrefix::parse("#");
        assert_eq!(heading.heading, Some((0..1, 1)));
        assert_eq!(heading.content, 1);
    }

    #[test]
    fn a_line_command_leaves_the_caret_at_the_same_letter_or_at_the_end_of_the_line() {
        let at = |t: &str, at: usize| set_heading(&crate::reader::parse(t), bytes(at..at), 1).selection;
        assert_eq!(at("abc", 1), bytes(3..3));
        assert_eq!(at("a\nbc", 2), bytes(6..6));
    }
}
