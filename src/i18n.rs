//! Interface texts in the language macOS picks for the app from the user's preferred languages. The German text is
//! the key; `<language>.lproj/Localizable.strings` maps it, and a missing entry shows the German text.

use std::collections::HashMap;
use std::sync::OnceLock;

use objc2_foundation::{NSBundle, NSString};

static TEXTS: OnceLock<HashMap<String, &'static str>> = OnceLock::new();

fn texts() -> &'static HashMap<String, &'static str> {
    TEXTS.get_or_init(|| {
        let bundle = NSBundle::mainBundle();
        let path = bundle.pathForResource_ofType(
            Some(&NSString::from_str("Localizable")),
            Some(&NSString::from_str("strings")),
        );
        let content = path.and_then(|p| std::fs::read_to_string(p.to_string()).ok());
        parse(content.as_deref().unwrap_or(""))
            .into_iter()
            .map(|(k, v)| (k, &*Box::leak(v.into_boxed_str())))
            .collect()
    })
}

/// The text for `key` in the interface language.
pub fn t(key: &'static str) -> &'static str {
    texts().get(key).copied().unwrap_or(key)
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

    /// `SPLAT_I18N_ONLY=fr` checks one language, for working on it while others are incomplete.
    fn only_language() -> Option<String> {
        std::env::var("SPLAT_I18N_ONLY").ok()
    }

    /// Keys marked in the source with `t("…")`, `tf("…")` or `n("…")`.
    fn source_keys() -> std::collections::BTreeSet<String> {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
        let mut files = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );
        let mut keys = std::collections::BTreeSet::new();
        for file in files {
            let source = std::fs::read_to_string(&file).unwrap();
            // Only the test module is left out; `#[cfg(test)]` also marks single items further up.
            let code = source.split("#[cfg(test)]\nmod tests").next().unwrap_or("");
            for marker in ["t(", "tf(", "n("] {
                for (i, _) in code.match_indices(marker) {
                    let before = code[..i].chars().next_back();
                    if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    // rustfmt may move the literal to the next line.
                    let Some(rest) = code[i + marker.len()..].trim_start().strip_prefix('"') else {
                        continue;
                    };
                    if let Some(end) = rest.find('"') {
                        keys.insert(rest[..end].to_string());
                    }
                }
            }
        }
        keys
    }

    #[test]
    fn every_language_translates_every_text() {
        let keys = source_keys();
        assert!(keys.len() > 200, "{} keys found", keys.len());
        let resources = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
        let mut languages = 0;
        for entry in std::fs::read_dir(&resources).unwrap().flatten() {
            let dir = entry.path();
            let Some(name) = dir
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".lproj"))
            else {
                continue;
            };
            if only_language().is_some_and(|only| only != name) {
                continue;
            }
            assert!(dir.join("QuickReference.md").exists(), "{name}: no QuickReference.md");
            assert!(dir.join("InfoPlist.strings").exists(), "{name}: no InfoPlist.strings");
            if name == "de" {
                continue;
            }
            languages += 1;
            let table: std::collections::HashMap<String, String> =
                parse(&std::fs::read_to_string(dir.join("Localizable.strings")).unwrap())
                    .into_iter()
                    .collect();
            let missing: Vec<&String> = keys
                .iter()
                .filter(|k| table.get(*k).is_none_or(|v| v.trim().is_empty()))
                .collect();
            assert!(missing.is_empty(), "{name}: missing {missing:?}");
            let stale: Vec<&String> = table.keys().filter(|k| !keys.contains(*k)).collect();
            assert!(stale.is_empty(), "{name}: not in the code {stale:?}");
            for (key, value) in &table {
                assert_eq!(
                    key.matches("{}").count(),
                    value.matches("{}").count(),
                    "{name}: placeholders of {key:?}"
                );
            }
        }
        if only_language().is_none() {
            assert!(languages >= 23, "{languages} translations");
        }
    }

    #[test]
    fn every_quick_reference_keeps_the_structure_of_the_german_one() {
        use crate::markdown::doc::{Block, BlockKind, parse as parse_markdown};
        fn shape(blocks: &[Block], out: &mut Vec<String>) {
            for b in blocks {
                out.push(match &b.kind {
                    BlockKind::Heading { level, .. } => format!("heading {level}"),
                    BlockKind::CodeBlock { info, .. } => format!("code {info}"),
                    other => format!("{:?}", std::mem::discriminant(other)),
                });
                shape(&b.children, out);
            }
        }
        let resources = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
        let read =
            |lang: &str| std::fs::read_to_string(resources.join(format!("{lang}.lproj/QuickReference.md"))).unwrap();
        let german = read("de");
        let mut expected = Vec::new();
        shape(&parse_markdown(&german).blocks, &mut expected);
        let kept: Vec<&str> = german
            .lines()
            .filter(|l| {
                l.starts_with('$')
                    || l.starts_with("\\int")
                    || l.starts_with("> [!")
                    || l.starts_with("fn ")
                    || *l == "---"
            })
            .collect();
        for entry in std::fs::read_dir(&resources).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(lang) = name.strip_suffix(".lproj") else {
                continue;
            };
            if only_language().is_some_and(|only| only != lang) {
                continue;
            }
            let text = read(lang);
            let mut found = Vec::new();
            shape(&parse_markdown(&text).blocks, &mut found);
            assert_eq!(found, expected, "{lang}: block structure");
            for line in &kept {
                assert!(text.lines().any(|l| l == *line), "{lang}: lost {line:?}");
            }
            assert_eq!(text.matches("[^1]").count(), 2, "{lang}: footnote");
            assert!(text.contains("$E = mc^2$"), "{lang}: inline formula");
        }
    }

    #[test]
    fn missing_texts_stay_german_and_take_arguments() {
        assert_eq!(t("Fett"), "Fett");
        assert_eq!(tf("Überschrift {}", &["2"]), "Überschrift 2");
    }
}
