//! What text inside RTF means: the instruction of a field and the marker of a list item.

use crate::rich::ListKind;

const HYPERLINK: &str = "HYPERLINK";

/// The address a field's instruction leads to, if it is a link: `HYPERLINK "url" \l "fragment"`.
pub(super) fn hyperlink(instruction: &str) -> Option<String> {
    let instruction = instruction.trim_start();
    let keyword = instruction.get(..HYPERLINK.len())?;
    let mut rest = &instruction[HYPERLINK.len()..];
    if !keyword.eq_ignore_ascii_case(HYPERLINK) || !rest.starts_with(|c: char| c.is_whitespace() || c == '"') {
        return None;
    }
    let mut url = None;
    let mut fragment = None;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        if let Some(switch) = rest.strip_prefix('\\') {
            let name = switch.chars().next();
            rest = &switch[name.map_or(0, char::len_utf8)..];
            match name {
                Some('l') => fragment = Some(next_argument(&mut rest)),
                Some('o' | 't') => {
                    next_argument(&mut rest);
                }
                _ => {}
            }
        } else {
            let argument = next_argument(&mut rest);
            url.get_or_insert(argument);
        }
    }
    match (url.filter(|u| !u.is_empty()), fragment.filter(|f| !f.is_empty())) {
        (Some(url), Some(fragment)) => Some(format!("{url}#{fragment}")),
        (Some(url), None) => Some(url),
        (None, Some(fragment)) => Some(format!("#{fragment}")),
        (None, None) => None,
    }
}

/// Takes a quoted argument or a word off the start of `text`.
fn next_argument(text: &mut &str) -> String {
    let rest = text.trim_start();
    if let Some(quoted) = rest.strip_prefix('"') {
        let end = quoted.find('"').unwrap_or(quoted.len());
        *text = quoted.get(end + 1..).unwrap_or("");
        return quoted[..end].to_string();
    }
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    *text = &rest[end..];
    rest[..end].to_string()
}

/// What a list item's marker makes of it: the kind of list and whether its box is checked, if it has one.
pub(super) fn list_kind(marker: &str) -> (ListKind, Option<bool>) {
    let marker = marker.trim();
    match marker.chars().next() {
        Some('☐') => return (ListKind::Bullet, Some(false)),
        Some('☑' | '☒') => return (ListKind::Bullet, Some(true)),
        _ => {}
    }
    let inner = marker.strip_prefix('(').unwrap_or(marker);
    let digits = inner.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 {
        let start = inner[..digits].parse().unwrap_or(u64::MAX);
        return (ListKind::Numbered { start }, None);
    }
    let letters = inner.bytes().take_while(u8::is_ascii_alphabetic).count();
    let word = &inner[..letters];
    let is_counted = letters == 1 || word.bytes().all(|b| b"ivxlcdmIVXLCDM".contains(&b));
    if letters > 0 && is_counted && inner[letters..].starts_with(['.', ')']) {
        return (ListKind::Numbered { start: 1 }, None);
    }
    (ListKind::Bullet, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_field_gives_its_address() {
        for (instruction, wanted) in [
            ("HYPERLINK \"https://a.bc\"", Some("https://a.bc")),
            ("  hyperlink \"https://a.bc\"  ", Some("https://a.bc")),
            ("HYPERLINK\"https://a.bc\"", Some("https://a.bc")),
            ("HYPERLINK https://a.bc", Some("https://a.bc")),
            ("HYPERLINK \\l \"anker\"", Some("#anker")),
            ("HYPERLINK \"a.md\" \\l \"anker\"", Some("a.md#anker")),
            ("HYPERLINK \"a.md\" \\o \"Tipp\" \\t \"_blank\" \\m \\n", Some("a.md")),
            ("HYPERLINK \"a.md\" \"zweite\"", Some("a.md")),
            ("HYPERLINK \"", None),
            ("HYPERLINK \"\"", None),
            ("HYPERLINK", None),
            ("HYPERLINKED \"x\"", None),
            ("PAGEREF x", None),
            ("PAGE", None),
            ("", None),
            ("HYPERLÄNK \"x\"", None),
            ("HYPERLINK \\", None),
            ("HYPERLINK \\ä \"x\"", Some("x")),
            ("HYPERLINK \"nie zu", Some("nie zu")),
        ] {
            assert_eq!(hyperlink(instruction).as_deref(), wanted, "{instruction}");
        }
    }

    #[test]
    fn a_marker_tells_the_kind_of_list() {
        let numbered = |start| (ListKind::Numbered { start }, None);
        let bullet = (ListKind::Bullet, None);
        for (marker, wanted) in [
            ("\t1.\t", numbered(1)),
            ("12)", numbered(12)),
            ("(3)", numbered(3)),
            ("1.1.", numbered(1)),
            ("a.", numbered(1)),
            ("b)", numbered(1)),
            ("iv.", numbered(1)),
            ("XII)", numbered(1)),
            ("•", bullet),
            ("\t\u{2043}\t", bullet),
            ("·", bullet),
            ("o", bullet),
            ("§", bullet),
            ("", bullet),
            ("Hallo.", bullet),
            ("☐", (ListKind::Bullet, Some(false))),
            ("☑", (ListKind::Bullet, Some(true))),
            ("☒ ", (ListKind::Bullet, Some(true))),
            ("99999999999999999999999.", numbered(u64::MAX)),
        ] {
            assert_eq!(list_kind(marker), wanted, "{marker:?}");
        }
    }
}
