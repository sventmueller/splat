//! What keeps text typed in the preview literal once it stands in the Markdown source.

use crate::layout::ParaKind;
use crate::map::InsertPoint;
use splat_markdown::text::{Byte, TextAt, decode_entity, line_start};

/// Escapes characters typed into the preview so they stay literal text in Markdown.
pub(super) fn escape_typed(typed: &str, point: &InsertPoint, text: &str) -> String {
    let in_table = matches!(point.para_kind, ParaKind::TableCell { .. });
    if point.code {
        // A `|` in a table ends the cell even inside a code span.
        return if in_table && !matches!(point.para_kind, ParaKind::Code { .. }) {
            typed.replace('|', "\\|")
        } else {
            typed.to_string()
        };
    }
    if point.html {
        return typed.replace('&', "&amp;").replace('<', "&lt;");
    }
    let chars: Vec<char> = typed.chars().collect();
    let before = text.upto(point.src).chars().next_back();
    let after = text.from(point.src).chars().next();
    let mut out = String::new();
    for (n, &c) in chars.iter().enumerate() {
        let prev = if n == 0 { before } else { Some(chars[n - 1]) };
        let next = chars.get(n + 1).copied().or(after);
        match c {
            '\\' | '`' | '*' | '[' | ']' | '<' | '$' | '~' | '^' => {
                out.push('\\');
                out.push(c);
            }
            '&' if decode_entity(&format!(
                "&{}",
                chars[n + 1..].iter().take(LONGEST_REFERENCE).collect::<String>()
            ))
            .is_some() =>
            {
                out.push_str("\\&");
            }
            // The end of what would otherwise read as a character reference.
            ';' if closes_reference(
                &context(text, point.src, LONGEST_REFERENCE),
                tail(&out, LONGEST_REFERENCE),
            ) =>
            {
                out.push_str("\\;");
            }
            '|' if in_table => out.push_str("\\|"),
            // Doubled, they would start a highlight, a spoiler or a comment.
            '=' | '|' | '%' if prev == Some(c) || next == Some(c) => {
                out.push('\\');
                out.push(c);
            }
            ':' if closes_shortcode(
                &context(text, point.src, LONGEST_SHORTCODE),
                tail(&out, LONGEST_SHORTCODE),
            ) =>
            {
                out.push_str("\\:");
            }
            '{' if matches!(point.para_kind, ParaKind::Heading(_)) => out.push_str("\\{"),
            '_' => {
                if prev.is_none_or(|p| !p.is_alphanumeric()) || next.is_none_or(|x| !x.is_alphanumeric()) {
                    out.push('\\');
                }
                out.push('_');
            }
            '\n' if in_table => out.push(' '),
            _ => out.push(c),
        }
    }
    let prefix = text.at(line_start(text, point.src)..point.src);
    if point.at_para_start || prefix.chars().all(|c| matches!(c, ' ' | '\t' | '>')) {
        if out.starts_with(['#', '>', '-', '+', '=', ':']) {
            out.insert(0, '\\');
        } else {
            let digits = out.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 && matches!(out.as_bytes().get(digits), Some(b'.' | b')')) {
                out.insert(digits, '\\');
            }
        }
    }
    out
}

/// More bytes than a character reference, `&CounterClockwiseContourIntegral;`, and an emoji shortcode can have: looking
/// no further back keeps pasting a long text of `&` and `;` linear.
const LONGEST_REFERENCE: usize = 40;

const LONGEST_SHORTCODE: usize = 48;

fn tail(s: &str, bytes: usize) -> &str {
    &s[s.floor_char_boundary(s.len().saturating_sub(bytes))..]
}

fn context(text: &str, end: Byte, bytes: usize) -> String {
    tail(text.upto(end), bytes).to_string()
}

/// Whether a `;` behind `before` and `typed` ends a character reference.
pub(super) fn closes_reference(before: &str, typed: &str) -> bool {
    let text = format!("{before}{typed};");
    text.rfind('&').is_some_and(|at| {
        !text[..at].ends_with('\\') && decode_entity(&text[at..]).is_some_and(|(_, len)| at + len == text.len())
    })
}

/// Whether a `:` typed after `before` and `typed` would end an emoji shortcode such as `:smile:`.
pub(super) fn closes_shortcode(before: &str, typed: &str) -> bool {
    let context = format!("{before}{typed}");
    let name: usize = context
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-'))
        .map(char::len_utf8)
        .sum();
    let rest = &context[..context.len() - name];
    name > 0 && rest.ends_with(':') && !rest.ends_with("\\:")
}
