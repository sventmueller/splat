//! How the modules of a part of the program depend on each other: downwards, so that each can be read without those that
//! build on it. Between the crates the compiler holds this. `src/editor/controller/layers.rs` sets the order of the components; the
//! handles it hands out make it the one module everybody knows, so it does not count here.

use std::collections::{BTreeMap, BTreeSet};

use splat_testkit::sources::{self, Crate, Source, Without, named, resolve, root};

type Graph = BTreeMap<String, BTreeSet<String>>;

/// The modules directly in `within`, each with those of them it names; `leave_out` count for none.
fn graph(krate: &Crate, within: &[&str], leave_out: &[&str]) -> Graph {
    let within: Vec<String> = within.iter().map(ToString::to_string).collect();
    let node = |module: &[String]| {
        (module.len() > within.len() && module.starts_with(&within))
            .then(|| module[within.len()].clone())
            .filter(|name| !leave_out.contains(&name.as_str()))
    };
    let mut graph: Graph = krate
        .modules
        .iter()
        .filter(|module| module.len() == within.len() + 1)
        .filter_map(|module| node(module))
        .map(|name| (name, BTreeSet::new()))
        .collect();
    for source in &krate.sources {
        for name in named(source) {
            let (Some(from), Some(to)) = (
                node(&name.module),
                resolve(&name.module, &name.path, &krate.modules).and_then(|path| node(&path)),
            ) else {
                continue;
            };
            if from != to && graph.contains_key(&to) {
                graph.get_mut(&from).expect("a module of the graph").insert(to);
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
fn the_modules_of_every_crate_depend_downwards_only() {
    let roots = sources::crate_roots();
    assert!(roots.len() >= 8, "{roots:?}");
    let mut ways_round = Vec::new();
    for crate_root in roots {
        let graph = graph(&Crate::read(&crate_root, Without::Tests), &[], &[]);
        let name = crate_root
            .strip_prefix(root())
            .unwrap_or(&crate_root)
            .display()
            .to_string();
        if name.contains("splat-markdown") {
            assert!(graph.len() > 10, "{graph:?}");
            assert!(
                graph["html"].contains("doc"),
                "the look at the sources sees {:?}",
                graph["html"]
            );
            assert!(
                graph["commands"].contains("doc"),
                "from a folder: {:?}",
                graph["commands"]
            );
        }
        if name.contains("splat-preview") {
            assert!(graph["map"].contains("layout"), "{graph:?}");
        }
        if let Some(found) = cycle(&graph) {
            ways_round.push(format!("{name}: {}", found.join(" -> ")));
        }
    }
    assert!(ways_round.is_empty(), "{ways_round:#?}");
}

#[test]
fn the_modules_of_the_editor_depend_downwards_only() {
    let app = Crate::read(&root().join("src/main.rs"), Without::Tests);
    let graph = graph(&app, &["editor"], &["layers", "selftest"]);
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

fn crate_of(files: &[(&[&str], &str)]) -> Crate {
    Crate::of(
        files
            .iter()
            .map(|(module, text)| {
                let module = module.iter().map(ToString::to_string).collect();
                Source::parse("a.rs", module, (*text).to_string(), Without::Tests)
            })
            .collect(),
    )
}

#[test]
fn a_dependency_is_found_however_far_up_it_is_written() {
    let krate = crate_of(&[
        (&[], "mod doc; mod text; mod commands; mod deep;"),
        (&["doc"], ""),
        (&["text"], ""),
        (&["commands"], "use super::doc::Block;"),
        (&["deep"], "mod a;"),
        (&["deep", "a"], "mod b;"),
        (&["deep", "a", "b"], "fn f() { super::super::super::text::x(); }"),
    ]);
    let graph = graph(&krate, &[], &[]);
    assert!(graph["commands"].contains("doc"), "a folder's mod.rs: {graph:?}");
    assert!(graph["deep"].contains("text"), "three levels up: {graph:?}");
}

#[test]
fn what_only_tests_build_is_no_dependency() {
    let krate = crate_of(&[
        (&[], "mod a; mod b; mod c; mod d;"),
        (&["b"], ""),
        (&["c"], ""),
        (&["d"], ""),
        (
            &["a"],
            "#[cfg(test)]\nmod order_tests { use super::super::b; }\n\
             #[cfg(test)]\nfn helper() { crate::c::x(); }\n\
             fn after_the_tests() { crate::d::y(); }\n",
        ),
    ]);
    assert_eq!(graph(&krate, &[], &[])["a"], BTreeSet::from(["d".to_string()]));
}

/// The places outside `reader/` that name comrak in code.
fn names_the_parser(source: &Source) -> Vec<String> {
    named(source)
        .into_iter()
        .filter(|name| name.path.iter().find(|s| *s != "::").is_some_and(|s| s == "comrak"))
        .map(|name| format!("{}:{}", source.name, name.line))
        .collect()
}

#[test]
fn only_the_reader_names_the_parser() {
    let root = root();
    let reader = root.join("crates/splat-markdown/src/reader");
    let files: Vec<_> = ["src", "crates", "tests", "xtask"]
        .iter()
        .flat_map(|dir| sources::rust_files(&root.join(dir)))
        .filter(|file| !file.starts_with(&reader))
        .collect();
    assert!(files.len() > 50, "{} files found", files.len());
    let found: Vec<String> = files
        .iter()
        .flat_map(|file| names_the_parser(&Source::read(file, Without::Nothing)))
        .collect();
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn the_parser_is_found_however_it_is_named() {
    let found = |text: &str| names_the_parser(&Source::parse("a.rs", Vec::new(), text.to_string(), Without::Nothing));
    assert_eq!(found("use comrak::Options;"), ["a.rs:1"]);
    assert_eq!(found("use {comrak};").len(), 1);
    assert_eq!(found("fn f() {\n    ::comrak::parse();\n}").len(), 1);
    assert_eq!(
        found("fn f() { format!(\"{:?}\", comrak::Options::default()); }").len(),
        1
    );
    assert_eq!(found("extern crate comrak;").len(), 1);
    assert!(found("// comrak::parse\nconst A: &str = \"comrak::parse\";").is_empty());
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
    let root = root();
    let files: Vec<_> = ["src", "crates", "tests", "xtask/src"]
        .iter()
        .flat_map(|dir| sources::rust_files(&root.join(dir)))
        .collect();
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

/// The names in backticks in the first column of the tables of `doc`, split at `::` and `.`, that `words` lacks.
fn names_the_code_lacks(doc: &str, words: &BTreeSet<String>) -> Vec<String> {
    let is_name = |s: &str| {
        s.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    doc.lines()
        .filter_map(|line| line.strip_prefix("| ")?.split('|').next())
        .flat_map(|first| first.split('`').skip(1).step_by(2))
        .flat_map(|ticked| ticked.split(['.', ':']).map(|part| part.trim_end_matches("()")))
        .filter(|name| is_name(name) && !words.contains(*name))
        .map(String::from)
        .collect()
}

#[test]
fn the_documentation_names_only_what_the_code_has() {
    let mut words = BTreeSet::new();
    for dir in ["src", "crates", "xtask/src", "tests"] {
        for file in sources::rust_files(&root().join(dir)) {
            let text = std::fs::read_to_string(&file).unwrap();
            words.extend(
                text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .map(String::from),
            );
        }
    }
    assert!(words.contains("EditorController"), "{} words", words.len());
    for doc in ["docs/glossary.md", "docs/development.md", "docs/partial-parse.md"] {
        let missing = names_the_code_lacks(&std::fs::read_to_string(root().join(doc)).unwrap(), &words);
        assert!(
            missing.is_empty(),
            "{doc} names what the code no longer has: {missing:?}"
        );
    }
}

#[test]
fn a_name_the_code_lacks_is_found() {
    let words: BTreeSet<String> = ["Doc", "parse"].map(String::from).into();
    let doc = "| `Doc::parse`, `Gone` (`splat-x`) | text with `Missing` |\n| `Doc.parse()` | |\nnot a table `Other`\n";
    assert_eq!(names_the_code_lacks(doc, &words), ["Gone"]);
}

/// The lines that send an Objective-C message whose name starts with an underscore, Apple's mark for a method that is not
/// public: App Review turns away an app that calls one.
fn private_calls(text: &str) -> Vec<usize> {
    let private = |message: &str| {
        let mut chars = message.trim_start().chars();
        chars.next() == Some('_') && chars.next().is_some_and(char::is_alphabetic)
    };
    text.lines()
        .enumerate()
        .filter(|(_, line)| {
            line.split("sel!(").skip(1).any(private)
                || line.split("msg_send![").skip(1).any(|call| {
                    let call = call.split(']').next().unwrap_or("");
                    call.split_once(',').is_some_and(|(_, message)| private(message))
                })
        })
        .map(|(n, _)| n + 1)
        .collect()
}

#[test]
fn the_app_calls_no_private_method() {
    let root = root();
    let selftest = root.join("src/editor/selftest");
    let files: Vec<_> = sources::rust_files(&root.join("src"))
        .into_iter()
        .filter(|file| !file.starts_with(&selftest) && *file != root.join("src/editor/selftest.rs"))
        .collect();
    assert!(files.len() > 50, "{} files found", files.len());
    for file in files {
        let found = private_calls(&std::fs::read_to_string(&file).unwrap());
        assert!(found.is_empty(), "{}: lines {found:?}", file.display());
    }
}

#[test]
fn a_private_method_is_found_however_it_is_sent() {
    assert_eq!(
        private_calls("if web.respondsToSelector(sel!(_webProcessIdentifier)) {"),
        [1]
    );
    assert_eq!(
        private_calls("let () = unsafe { msg_send![web, _killWebContentProcess] };"),
        [1]
    );
    assert_eq!(private_calls("x\nmsg_send![&*u,  _private: on]"), [2]);
    assert!(private_calls("msg_send![super(Loader::alloc().set_ivars(())), init]").is_empty());
    assert!(private_calls("sel!(mdGrantFolder:) msg_send![web, isLoading]").is_empty());
}
