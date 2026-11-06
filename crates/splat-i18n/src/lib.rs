//! Interface texts in the language macOS picks for the app from the user's preferred languages. The German text is
//! the key; `<language>.lproj/Localizable.strings` maps it, and a missing entry shows the German text.

#![forbid(unsafe_code)]
#![warn(missing_docs, unreachable_pub)]

use std::collections::HashMap;
use std::sync::OnceLock;

static TEXTS: OnceLock<HashMap<String, &'static str>> = OnceLock::new();

/// Makes the entries of a `.strings` file the interface texts; only the first call counts.
pub fn install(strings_file: &str) {
    TEXTS.get_or_init(|| {
        parse(strings_file)
            .into_iter()
            .map(|(k, v)| (k, &*Box::leak(v.into_boxed_str())))
            .collect()
    });
}

/// The text for `key` in the interface language; the key itself before `install`.
pub fn t(key: &'static str) -> &'static str {
    TEXTS.get().and_then(|texts| texts.get(key).copied()).unwrap_or(key)
}

/// Like `t`, with each `{}` replaced by the next of `args`.
pub fn tf(key: &'static str, args: &[&str]) -> String {
    let mut out = String::new();
    let mut parts = t(key).split("{}");
    if let Some(first) = parts.next() {
        out.push_str(first);
    }
    for (n, part) in parts.enumerate() {
        out.push_str(args.get(n).copied().unwrap_or(""));
        out.push_str(part);
    }
    out
}

/// Marks a key where `t` cannot run, such as in a constant; the text is translated where it is shown.
pub const fn n(key: &'static str) -> &'static str {
    key
}

/// Entries of a `.strings` file: `"key" = "value";` lines with `\"`, `\\` and `\n` escapes, and comments.
pub fn parse(content: &str) -> Vec<(String, String)> {
    fn quoted(s: &str) -> Option<(String, &str)> {
        let s = s.trim_start().strip_prefix('"')?;
        let mut out = String::new();
        let mut chars = s.char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => return Some((out, &s[i + 1..])),
                '\\' => match chars.next()?.1 {
                    'n' => out.push('\n'),
                    other => out.push(other),
                },
                c => out.push(c),
            }
        }
        None
    }
    content
        .lines()
        .filter_map(|line| {
            let (key, rest) = quoted(line)?;
            let rest = rest.trim_start().strip_prefix('=')?;
            let (value, rest) = quoted(rest)?;
            rest.trim_start().starts_with(';').then_some((key, value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_files_parse_with_escapes() {
        let entries = parse("/* Kommentar */\n\"A \\\"b\\\"\" = \"C\\nD\";\n\n\"x\" = \"y\";\n");
        assert_eq!(
            entries,
            vec![
                ("A \"b\"".to_string(), "C\nD".to_string()),
                ("x".to_string(), "y".to_string())
            ]
        );
    }

    #[test]
    fn missing_texts_stay_german_and_take_arguments() {
        assert_eq!(t("Fett"), "Fett");
        assert_eq!(tf("Überschrift {}", &["2"]), "Überschrift 2");
    }
}
