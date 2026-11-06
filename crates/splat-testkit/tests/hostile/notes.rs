//! Texts built to hurt the notes: counts, metadata, search, renames and the folders of a large tree.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use splat_markdown::counts::{of_doc, of_range};
use splat_markdown::front_matter::{self, Value};
use splat_markdown::reader::parse;
use splat_markdown::text::bytes;
use splat_notes::{Node, Query, Tree, applied, hits_in_text, renames, scan, search};

use super::common::{MEASURING, NOTICEABLE, Stages, applies, grows_too_fast, numbered, scales};

/// Every way through the notes that a text can take, without a disk.
fn consume_note(text: &str) -> Stages {
    let mut stages = Stages::default();
    let doc = stages.time("parse", || parse(text));
    stages.time("counts", || {
        of_doc(&doc);
        let middle = text.floor_char_boundary(text.len() / 2);
        of_range(&doc, bytes(0..middle));
        of_range(&doc, bytes(middle..text.len()));
    });
    let found = stages.time("reading the metadata", || front_matter::read(&doc));
    stages.time("changing the metadata", || {
        let fields = found.map_or(Vec::new(), |block| block.fields);
        let same: Vec<(String, Value)> = fields.iter().map(|f| (f.key.clone(), f.value.clone())).collect();
        let mut changed = same.clone();
        if let Some(last) = changed.last_mut() {
            last.1 = Value::Text("another".into());
        }
        changed.push(("new".into(), Value::List(vec!["a".into(), "b, c".into()])));
        front_matter::edit(&doc, &same, &splat_markdown::text::bytes(0..0));
        for wanted in [changed, Vec::new()] {
            if let Some(edit) = front_matter::edit(&doc, &wanted, &splat_markdown::text::bytes(0..0)) {
                applies(text, edit);
            }
        }
    });
    let file = Path::new("/v/Here.md");
    let tree = note_tree();
    stages.time("searching", || {
        for needle in ["a", "SS", "ß", "aab", "ä", "x y"] {
            for case_sensitive in [false, true] {
                let query = Query::Text {
                    needle: needle.to_string(),
                    case_sensitive,
                };
                hits_in_text(text, &query, file, &tree);
            }
        }
    });
    stages.time("tags and links", || {
        hits_in_text(text, &Query::Tag("tag".into()), file, &tree);
        hits_in_text(text, &Query::LinksTo("/v/Old.md".into()), file, &tree);
    });
    stages.time("a rename", || {
        let rewrites = renames(
            &tree,
            Path::new("/v/Old.md"),
            Path::new("/v/New Name.md"),
            &AtomicBool::new(false),
            &mut |path| (path == file).then(|| text.to_string()),
        );
        for rewrite in rewrites {
            applied(text, &rewrite.replacements);
        }
    });
    stages
}

fn note_tree() -> Tree {
    let note = |name: &str| Node {
        name: name.to_string(),
        path: Path::new("/v").join(name),
        folder: false,
        children: Vec::new(),
    };
    Tree {
        root: "/v".into(),
        nodes: vec![note("Here.md"), note("Old.md")],
        top: vec![0, 1],
        cut: false,
        unreadable: 0,
    }
}

/// What is hard on the notes, with every count at `quarters` fourths of its full size.
fn note_cases(quarters: usize) -> Vec<(&'static str, String)> {
    let n = |count: usize| count * quarters / 4;
    let block = |content: String| format!("---\n{content}---\n\nText\n");
    vec![
        ("ten megabytes of fields", block("a: b\n".repeat(n(2_000_000)))),
        ("many fields of the same key", block("a: b # c\n\n".repeat(n(200_000)))),
        (
            "one value of ten megabytes",
            block(format!("a: {}\n", "b".repeat(n(10_000_000)))),
        ),
        (
            "one long quoted value",
            block(format!("a: \"{}\"\n", "\\n".repeat(n(1_000_000)))),
        ),
        ("one long key", block(format!("{}: b\n", "a".repeat(n(2_000_000))))),
        (
            "a value without an end",
            block(format!("a: \"{}\n", "b\n".repeat(n(500_000)))),
        ),
        (
            "a list without an end",
            block(format!("a: [{}\n", "b,\n".repeat(n(300_000)))),
        ),
        (
            "a long flow list",
            block(format!("a: [{}c]\n", "b, ".repeat(n(400_000)))),
        ),
        (
            "a long block list",
            block(format!("a:\n{}", "  - b\n".repeat(n(300_000)))),
        ),
        (
            "a long block list with comments",
            block(format!("a:\n{}", "  - b # c\n  # d\n\n".repeat(n(100_000)))),
        ),
        (
            "deep indentation",
            block(format!(
                "a:\n{}",
                numbered(n(4_500), |i| format!("{}b: c\n", " ".repeat(i)))
            )),
        ),
        ("blank lines", block("\n".repeat(n(2_000_000)))),
        ("comments", block("# a\n".repeat(n(500_000)))),
        ("lone returns", block("a: b\r".repeat(n(400_000)))),
        (
            "a block without an end",
            String::from("---\n") + &"a: b\n".repeat(n(400_000)),
        ),
        (
            "delimiters that close nothing",
            String::from("---\n") + &"---x\n".repeat(n(300_000)),
        ),
        ("words", "wort ".repeat(n(1_000_000))),
        ("umlauts", "größe ".repeat(n(500_000))),
        ("Chinese", "东京塔".repeat(n(500_000))),
        ("joined by hyphens", "a-".repeat(n(1_000_000))),
        (
            "marks on one letter",
            String::from("a") + &"\u{301}\u{323}".repeat(n(500_000)),
        ),
        ("sharp s", "ß".repeat(n(1_000_000))),
        ("a word never quite there", "a".repeat(n(2_000_000))),
        ("many hits on a line", "a ".repeat(n(1_000_000))),
        ("many hits on many lines", "a\n".repeat(n(1_000_000))),
        ("references to count around", "&amp;".repeat(n(400_000))),
        ("escapes to count around", "\\*".repeat(n(500_000))),
        (
            "html to count around",
            format!("<div>\n{}</div>\n", "<b>a</b> ".repeat(n(200_000))),
        ),
        (
            "code to count around",
            format!("> ```\n{}> ```\n", "> a b\n".repeat(n(300_000))),
        ),
        ("tags", "#tag ".repeat(n(400_000))),
        ("tags below tags", "#tag/x ".repeat(n(300_000))),
        ("wiki links", "[[Old]] ".repeat(n(100_000))),
        ("wiki links in paragraphs", "[[Old#h|t]]\n\n".repeat(n(100_000))),
        ("open wiki links", "[[Old".repeat(n(300_000))),
        ("links", "[t](Old.md) ".repeat(n(100_000))),
        ("links with spaces", "[t](<Old.md> \"x\") ".repeat(n(60_000))),
        ("definitions", numbered(n(100_000), |i| format!("[r{i}]: Old.md\n"))),
        ("definitions on one line", "[a]: Old.md ".repeat(n(200_000))),
        ("labels without an end", "[a\n\n".repeat(n(300_000))),
        ("labels without a start", "]:".repeat(n(800_000))),
        ("long labels", ("x".repeat(990) + "]:\n").repeat(n(2_000))),
        (
            "list marks in front of a label",
            ("- ".repeat(n(100_000)) + "[a]: Old.md\n").repeat(3),
        ),
    ]
}

#[test]
fn notes_built_to_hurt_take_linear_time() {
    let _alone = MEASURING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let cases = std::sync::Mutex::new(note_cases(1).into_iter().zip(note_cases(4)).collect::<Vec<_>>());
    let suspects = std::sync::Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(2, |n| n.get().min(4));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let Some(((name, small), (_, large))) = cases.lock().unwrap().pop() else {
                        return;
                    };
                    if scales(name, small.clone(), large.clone(), consume_note).is_err() {
                        suspects.lock().unwrap().push((name, small, large));
                    }
                }
            });
        }
    });
    let failures: Vec<String> = suspects
        .into_inner()
        .unwrap()
        .into_iter()
        .filter_map(|(name, small, large)| scales(name, small, large, consume_note).err())
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn the_folders_of_a_large_tree_take_linear_time() {
    let tree_of = |folders: usize| {
        let root = std::env::temp_dir().join(format!("splat-hostile-notes-{}-{folders}", std::process::id()));
        for folder in 0..folders {
            let dir = root.join(format!("f{folder}"));
            std::fs::create_dir_all(&dir).unwrap();
            for note in 0..10 {
                std::fs::write(dir.join(format!("{note}.md")), "[[Old]] #tag\n").unwrap();
            }
        }
        root
    };
    let walk = |root: &Path| {
        let started = Instant::now();
        let tree = scan(root, &AtomicBool::new(false));
        let query = Query::Tag("tag".into());
        let mut hits = 0;
        search(&tree, &query, &AtomicBool::new(false), &mut |found| {
            hits += found.hits.len()
        });
        assert_eq!(hits, tree.notes().count());
        started.elapsed()
    };
    let (small, large) = (tree_of(100), tree_of(400));
    let (small_took, large_took) = (walk(&small), walk(&large));
    std::fs::remove_dir_all(&small).unwrap();
    std::fs::remove_dir_all(&large).unwrap();
    if cfg!(target_arch = "aarch64") && !cfg!(debug_assertions) && !cfg!(coverage) {
        assert!(
            !grows_too_fast(NOTICEABLE, small_took, large_took, 100, 400),
            "{small_took:?}, then {large_took:?}"
        );
    }
}
