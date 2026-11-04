//! How the modules of a part of the program depend on each other: downwards, so that each can be read without those that
//! build on it. Between the crates the compiler holds this. `src/editor/controller/layers.rs` sets the order of the components; the
//! handles it hands out make it the one module everybody knows, so it does not count here.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

type Graph = BTreeMap<String, BTreeSet<String>>;

fn sources(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

/// The names `path` stands in front of in `text`, in `path::name` as in `path::{a, b::c}`.
fn named_behind<'a>(text: &'a str, path: &str) -> Vec<&'a str> {
    let word = |s: &'a str| s.split(|c: char| !c.is_alphanumeric() && c != '_').next().unwrap_or("");
    let mut names = Vec::new();
    for (at, _) in text.match_indices(path) {
        let rest = &text[at + path.len()..];
        match rest.strip_prefix('{') {
            Some(group) => {
                let group = group.split('}').next().unwrap_or("");
                names.extend(group.split(',').map(|name| word(name.trim())));
            }
            None => names.push(word(rest)),
        }
    }
    names
}

/// The modules below `folder`, each with those it names; `own` is how the code names what lies directly in that folder.
fn graph(folder: &str, own: &str, leave_out: &[&str]) -> Graph {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(folder);
    let mut files = Vec::new();
    sources(&root, &mut files);
    let module_of = |path: &Path| {
        let inside = path.strip_prefix(&root).unwrap();
        let first = inside.components().next().unwrap().as_os_str().to_string_lossy();
        first.trim_end_matches(".rs").to_string()
    };
    let is_module = |name: &str| !["mod", "lib"].contains(&name) && !leave_out.contains(&name);
    let modules: BTreeSet<String> = files.iter().map(|f| module_of(f)).filter(|m| is_module(m)).collect();
    let mut graph: Graph = modules.iter().map(|m| (m.clone(), BTreeSet::new())).collect();
    for file in &files {
        let module = module_of(file);
        if !modules.contains(&module) {
            continue;
        }
        let text = std::fs::read_to_string(file).unwrap();
        let product = text.split("#[cfg(test)]\nmod tests").next().unwrap();
        let nested = file.strip_prefix(&root).unwrap().components().count() > 1;
        let parent = if nested { "super::super::" } else { "super::" };
        let mut named = named_behind(product, own);
        named.extend(named_behind(product, parent));
        for name in named {
            if name != module && modules.contains(name) {
                graph.get_mut(&module).unwrap().insert(name.to_string());
            }
        }
    }
    graph
}

/// A way round through `graph`, if there is one.
fn cycle(graph: &Graph) -> Option<Vec<String>> {
    fn visit(graph: &Graph, at: &str, path: &mut Vec<String>, done: &mut BTreeSet<String>) -> Option<Vec<String>> {
        if let Some(from) = path.iter().position(|m| m == at) {
            return Some(path[from..].to_vec());
        }
        if !done.insert(at.to_string()) {
            return None;
        }
        path.push(at.to_string());
        for next in graph.get(at).into_iter().flatten() {
            if let Some(found) = visit(graph, next, path, done) {
                return Some(found);
            }
        }
        path.pop();
        None
    }
    let mut done = BTreeSet::new();
    graph
        .keys()
        .find_map(|start| visit(graph, start, &mut Vec::new(), &mut done))
}

#[test]
fn the_modules_of_the_model_depend_downwards_only() {
    let graph = graph("crates/splat-markdown/src", "crate::", &[]);
    assert!(graph.len() > 10, "{graph:?}");
    assert!(
        graph["html"].contains("doc"),
        "the look at the sources sees {:?}",
        graph["html"]
    );
    assert_eq!(cycle(&graph), None);
}

#[test]
fn the_modules_of_the_preview_depend_downwards_only() {
    let graph = graph("crates/splat-preview/src", "crate::", &[]);
    assert!(graph["map"].contains("layout"), "{graph:?}");
    assert_eq!(cycle(&graph), None);
}

#[test]
fn the_modules_of_the_editor_depend_downwards_only() {
    let graph = graph("src/editor", "crate::editor::", &["layers", "selftest"]);
    assert!(graph["remote"].contains("picture"), "{:?}", graph["remote"]);
    assert_eq!(cycle(&graph), None);
}

#[test]
fn a_way_round_is_found() {
    let edges = |list: &[(&str, &str)]| {
        let mut graph = Graph::new();
        for (from, to) in list {
            graph.entry((*from).to_string()).or_default().insert((*to).to_string());
        }
        graph
    };
    assert_eq!(cycle(&edges(&[("a", "b"), ("b", "c")])), None);
    let found = cycle(&edges(&[("a", "b"), ("b", "c"), ("c", "a")])).unwrap();
    assert_eq!(found, ["a", "b", "c"]);
}

#[test]
fn names_are_found_in_either_way_of_writing_them() {
    let text = "use super::doc::{A, B};\nuse super::{doc, text::Byte};\ncrate::url::x();\n";
    assert_eq!(named_behind(text, "super::"), ["doc", "doc", "text"]);
    assert_eq!(named_behind(text, "crate::"), ["url"]);
}

#[test]
fn only_the_reader_names_the_parser() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for dir in ["src", "crates", "tests", "xtask"] {
        sources(&root.join(dir), &mut files);
    }
    assert!(files.len() > 50, "{} files found", files.len());
    let reader = root.join("crates/splat-markdown/src/reader");
    let this_test = root.join(file!());
    for file in files.iter().filter(|f| !f.starts_with(&reader) && **f != this_test) {
        for (n, line) in std::fs::read_to_string(file).unwrap().lines().enumerate() {
            let code = line.trim_start();
            let names = code.contains("comrak::") || (code.contains("use ") && code.contains("comrak"));
            assert!(code.starts_with("//") || !names, "{}:{}: {line}", file.display(), n + 1);
        }
    }
}

/// Where a doc comment stops short of the width and goes on with a new sentence: a text that is filled would have gone on on
/// the same line, so what follows is most likely left over from something that moved away.
fn doc_lines_that_do_not_belong_together(text: &str, width: usize) -> Vec<usize> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    for (n, pair) in lines.windows(2).enumerate() {
        let (this, next) = (pair[0].trim_start(), pair[1].trim_start());
        let (Some(this_text), Some(next_text)) = (this.strip_prefix("/// "), next.strip_prefix("/// ")) else {
            continue;
        };
        let next_word = next_text.split(' ').next().unwrap_or("");
        let could_go_on = pair[0].chars().count() + 1 + next_word.chars().count() <= width;
        if this_text.ends_with('.') && could_go_on && next_text.starts_with(char::is_uppercase) {
            found.push(n + 1);
        }
    }
    found
}

#[test]
fn a_doc_comment_stays_with_what_it_describes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for dir in ["src", "crates", "tests", "xtask/src"] {
        sources(&root.join(dir), &mut files);
    }
    assert!(files.len() > 50, "{} files found", files.len());
    for file in files {
        let found = doc_lines_that_do_not_belong_together(&std::fs::read_to_string(&file).unwrap(), 120);
        assert!(found.is_empty(), "{}: lines {found:?}", file.display());
    }
}

#[test]
fn a_doc_comment_left_over_is_found() {
    let left_over = "/// What moved away.\n/// What stands here.\nfn a() {}\n";
    assert_eq!(doc_lines_that_do_not_belong_together(left_over, 120), [1]);
    let filled = "/// A long first sentence.\n/// Another one.\n";
    assert!(
        doc_lines_that_do_not_belong_together(filled, 26).is_empty(),
        "the width ended the line"
    );
    assert!(doc_lines_that_do_not_belong_together("/// One.\n/// and on.\n", 120).is_empty());
    assert!(doc_lines_that_do_not_belong_together("/// One.\n///\n/// Two.\n", 120).is_empty());
}
