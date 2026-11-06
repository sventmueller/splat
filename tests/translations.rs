//! Every language translates every text the sources name, and keeps the structure of the quick reference.

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
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    walk(&root.join("src"), &mut files);
    walk(&root.join("crates"), &mut files);
    files.retain(|file| {
        !file.components().any(|part| part.as_os_str() == "tests")
            && file.file_name().is_none_or(|name| name != "tests.rs")
            && !file.starts_with(root.join("crates/splat-testkit"))
    });
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
            splat_i18n::parse(&std::fs::read_to_string(dir.join("Localizable.strings")).unwrap())
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
        assert!(languages >= 34, "{languages} translations");
    }
}

#[test]
fn every_quick_reference_keeps_the_structure_of_the_german_one() {
    use splat_markdown::doc::{Block, BlockKind};
    use splat_markdown::reader::parse as parse_markdown;
    fn shape(blocks: &[Block], out: &mut Vec<String>) {
        for b in blocks {
            out.push(match &b.kind {
                BlockKind::Heading { level, .. } => format!("heading {level}"),
                BlockKind::CodeBlock { info, .. } => format!("code {info}"),
                other => format!("{:?}", std::mem::discriminant(other)),
            });
            shape(b.children(), out);
        }
    }
    let resources = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
    let read = |lang: &str| std::fs::read_to_string(resources.join(format!("{lang}.lproj/QuickReference.md"))).unwrap();
    let german = read("de");
    let mut expected = Vec::new();
    shape(parse_markdown(&german).blocks(), &mut expected);
    let kept: Vec<&str> = german
        .lines()
        .filter(|l| {
            l.starts_with('$') || l.starts_with("\\int") || l.starts_with("> [!") || l.starts_with("fn ") || *l == "---"
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
        shape(parse_markdown(&text).blocks(), &mut found);
        assert_eq!(found, expected, "{lang}: block structure");
        for line in &kept {
            assert!(text.lines().any(|l| l == *line), "{lang}: lost {line:?}");
        }
        assert_eq!(text.matches("[^1]").count(), 2, "{lang}: footnote");
        assert!(text.contains("$E = mc^2$"), "{lang}: inline formula");
    }
}
