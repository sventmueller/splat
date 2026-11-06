//! Every language translates every text the sources name, and keeps the structure of the quick reference.

use std::collections::BTreeSet;

use splat_testkit::sources::syn::visit::{self, Visit};
use splat_testkit::sources::syn::{self};
use splat_testkit::sources::{self, Crate, Source, Without, root, visit_body};

/// `SPLAT_I18N_ONLY=fr` checks one language, for working on it while others are incomplete.
fn only_language() -> Option<String> {
    std::env::var("SPLAT_I18N_ONLY").ok()
}

/// Keys marked in `source` with `t("…")`, `tf("…")` or `n("…")`.
fn keys_in(source: &Source) -> BTreeSet<String> {
    struct Keys<'s> {
        source: &'s Source,
        found: BTreeSet<String>,
    }
    impl<'ast> Visit<'ast> for Keys<'_> {
        fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
            let marker = match &*call.func {
                syn::Expr::Path(path) => path
                    .path
                    .segments
                    .last()
                    .is_some_and(|s| ["t", "tf", "n"].iter().any(|m| s.ident == m)),
                _ => false,
            };
            if marker
                && let Some(syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(key),
                    ..
                })) = call.args.first()
            {
                self.found.insert(key.value());
            }
            visit::visit_expr_call(self, call);
        }

        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            if let Some(body) = self.source.body(mac) {
                visit_body(self, &body);
            } else if mac.path.is_ident("macro_rules") {
                keys_in_tokens(mac.tokens.clone(), &mut self.found);
            }
        }
    }
    let mut keys = Keys {
        source,
        found: BTreeSet::new(),
    };
    keys.visit_file(&source.syntax);
    keys.found
}

/// Keys marked inside a `macro_rules!` definition, whose body is tokens rather than code: a marker followed by
/// parentheses that start with a string.
fn keys_in_tokens(tokens: sources::proc_macro2::TokenStream, found: &mut BTreeSet<String>) {
    use splat_testkit::sources::proc_macro2::{Delimiter, TokenTree};
    let mut marker = false;
    for token in tokens {
        match &token {
            TokenTree::Group(group) => {
                if marker
                    && group.delimiter() == Delimiter::Parenthesis
                    && let Some(TokenTree::Literal(first)) = group.stream().into_iter().next()
                    && let Ok(key) = syn::parse2::<syn::LitStr>(TokenTree::Literal(first).into())
                {
                    found.insert(key.value());
                }
                keys_in_tokens(group.stream(), found);
                marker = false;
            }
            TokenTree::Ident(ident) => marker = ["t", "tf", "n"].iter().any(|m| ident == m),
            _ => marker = false,
        }
    }
}

/// Keys the app and its crates mark, outside their tests.
fn source_keys() -> BTreeSet<String> {
    let testkit = root().join("crates/splat-testkit");
    std::iter::once(root().join("src/main.rs"))
        .chain(
            sources::crate_roots()
                .into_iter()
                .filter(|file| !file.starts_with(&testkit)),
        )
        .flat_map(|file| Crate::read(&file, Without::Tests).sources)
        .flat_map(|source| keys_in(&source))
        .collect()
}

#[test]
fn keys_are_found_where_the_code_marks_them() {
    let text = "fn f() {\n    t(\"A\");\n    i18n::tf(\n        \"B {}\",\n        x,\n    );\n    format!(\"{}\", n(\"C\"));\n    \
        let s = \"t(\\\"no key\\\")\";\n    // t(\"no key either\")\n}\n\
        #[cfg(test)]\nfn only_for_tests() { t(\"D\"); }\n\
        fn after_the_tests() { t(\"E\"); }\n\
        macro_rules! table { () => { F = f, { title: n(\"F\"), key: \"no key\" }; } }\n";
    let keys = keys_in(&Source::parse("a.rs", Vec::new(), text.to_string(), Without::Tests));
    assert_eq!(keys, BTreeSet::from(["A", "B {}", "C", "E", "F"].map(String::from)));
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
