use std::ops::Range;

use super::Edit;
use crate::markdown::text::{Byte, TextAt, growth, line_end, line_start, lines_with_ends, shifted};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListStyle {
    Bullet,
    Numbered,
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

/// The prefix of the lines after the first: quote markers repeat, a list marker becomes indentation, or every row
/// would start a list item of its own.
pub fn continuation_of(prefix: &str) -> String {
    let mut out = String::new();
    let mut rest = prefix;
    loop {
        let indent = rest.len() - rest.trim_start_matches([' ', '\t']).len();
        out.push_str(&rest[..indent]);
        rest = &rest[indent..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let marker = if rest.starts_with('>') {
            out.push('>');
            1
        } else if rest.starts_with(['-', '*', '+']) {
            out.push(' ');
            1
        } else if digits > 0 && rest[digits..].starts_with(['.', ')']) {
            out.push_str(&" ".repeat(digits + 1));
            digits + 1
        } else {
            out.push_str(rest);
            return out;
        };
        rest = &rest[marker..];
    }
}

/// Container and block prefixes at the start of a source line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinePrefix {
    pub quote_end: usize,
    pub list: Option<(Range<usize>, ListStyle)>,
    pub heading: Option<(Range<usize>, u8)>,
    pub content: usize,
}

impl LinePrefix {
    pub fn parse(line: &str) -> LinePrefix {
        let b = line.as_bytes();
        let mut i = 0;
        let mut quote_end = 0;
        loop {
            let mut j = i;
            while j < b.len() && b[j] == b' ' && j - i < 3 {
                j += 1;
            }
            if j < b.len() && b[j] == b'>' {
                j += 1;
                if j < b.len() && b[j] == b' ' {
                    j += 1;
                }
                i = j;
                quote_end = i;
            } else {
                break;
            }
        }
        let mut p = LinePrefix {
            quote_end,
            content: i,
            ..Default::default()
        };
        let mut j = i;
        while j < b.len() && (b[j] == b' ' || b[j] == b'\t') {
            j += 1;
        }
        let marker_start = j;
        let mut kind = None;
        let space_or_end = |c: Option<&u8>| c.is_none_or(|c| matches!(c, b' ' | b'\t'));
        if j < b.len() && matches!(b[j], b'-' | b'*' | b'+') && space_or_end(b.get(j + 1)) {
            j += 1;
            kind = Some(ListStyle::Bullet);
        } else {
            let digits = b[j..].iter().take_while(|c| c.is_ascii_digit()).count();
            if (1..=9).contains(&digits)
                && matches!(b.get(j + digits), Some(b'.' | b')'))
                && space_or_end(b.get(j + digits + 1))
            {
                j += digits + 1;
                kind = Some(ListStyle::Numbered);
            }
        }
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
        let hashes = rest.bytes().take_while(|&c| c == b'#').count();
        if (1..=6).contains(&hashes) && space_or_end(rest.as_bytes().get(hashes)) {
            let end = p.content + hashes + usize::from(rest.len() > hashes);
            p.heading = Some((p.content..end, u8::try_from(hashes).unwrap_or(6)));
            p.content = end;
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

pub fn set_heading(text: &str, sel: Range<Byte>, level: u8) -> Edit {
    let span = line_span(text, &sel);
    let mut edit = map_lines(text, &sel, |_, line, p| {
        let start = p.heading.as_ref().map_or(p.content, |(r, _)| r.start);
        let body = &line[p.content..];
        if level == 0 {
            format!("{}{}", &line[..start], body)
        } else {
            format!("{}{} {}", &line[..start], "#".repeat(level as usize), body)
        }
    });
    // The underline of a title written that way would otherwise stay behind as text or as a rule.
    if let Some(end) = setext_underline_behind(text, &span) {
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
    use crate::markdown::text::bytes;

    #[test]
    fn a_heading_level_replaces_the_underline_of_a_title() {
        let t = "Title\n=====\n\nx\n";
        assert_eq!(applied(t, &set_heading(t, bytes(2..2), 2)), "## Title\n\nx\n");
        assert_eq!(applied(t, &set_heading(t, bytes(2..2), 0)), "Title\n\nx\n");
        let separate = "Title\n\n---\n";
        assert_eq!(
            applied(separate, &set_heading(separate, bytes(2..2), 2)),
            "## Title\n\n---\n"
        );
    }

    #[test]
    fn prefixes_may_be_followed_by_a_tab() {
        assert_eq!(
            applied("-\tfoo\n", &toggle_list("-\tfoo\n", bytes(2..2), ListStyle::Bullet)),
            "foo\n"
        );
        assert_eq!(
            applied("#\tfoo\n", &set_heading("#\tfoo\n", bytes(2..2), 2)),
            "## foo\n"
        );
    }

    #[test]
    fn headings_lists_quotes() {
        let (s, _) = run("Titel\nText", 0..0, |d, r| set_heading(d.text(), r, 2));
        assert_eq!(s, "## Titel\nText");
        let (s, _) = run("## Titel", 0..0, |d, r| set_heading(d.text(), r, 0));
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
        assert_eq!(set_heading("abc", bytes(1..1), 1).selection, bytes(3..3));
        assert_eq!(set_heading("a\nbc", bytes(2..2), 1).selection, bytes(6..6));
    }
}
