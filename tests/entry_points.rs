//! Everything the system calls goes through a guard against panics: one that leaves a dispatch block ends the app, and one
//! that leaves a method or a block unwinds through the system's frames. This looks at the sources, as no type can say it.

use std::path::{Path, PathBuf};

/// Methods without a guard, as they call nothing of ours that can panic.
const NEEDS_NO_GUARD: [(&str, &str); 10] = [
    (
        "readFromData:ofType:error:",
        "behind its guard stands an error made of constants",
    ),
    (
        "dataOfType:error:",
        "behind its guard stands an error made of constants",
    ),
    (
        "printOperationWithSettings:error:",
        "behind its guard stands an error made of constants",
    ),
    (
        "presentedItemDidChange",
        "what it defines runs through main_soon and main_after, which guard it",
    ),
    ("init", "sets its ivars and calls the superclass"),
    ("respondsToSelector:", "compares a selector and asks the superclass"),
    (
        "saveToURL:ofType:forSaveOperation:completionHandler:",
        "guards the state it touches, and hands the system what it was given",
    ),
    (
        "readablePasteboardTypes",
        "adds constants to what the superclass returns",
    ),
    ("acceptableDragTypes", "a list of constants"),
    (
        "outlineView:child:ofItem:",
        "guards the lookup, and an object must come back whatever happens",
    ),
];

/// Blocks without a guard, told by what they hold.
const BLOCKS_THAT_NEED_NO_GUARD: [(&str, &str); 2] = [
    (
        "drawInRect_fromRect_operation_fraction",
        "draws an image, nothing of ours",
    ),
    (
        "palette::color(token",
        "looks a color up in a table, and a color must come back whatever happens",
    ),
];

fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

/// The products' sources, without tests and self-tests.
fn product() -> Vec<(String, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&src, &mut files);
    let is_product = |name: &str| !name.contains("selftest") && !name.ends_with("tests.rs");
    files
        .into_iter()
        .map(|path| (path.strip_prefix(&src).unwrap().display().to_string(), path))
        .filter(|(name, _)| is_product(name))
        .map(|(name, path)| {
            let text = std::fs::read_to_string(path).unwrap();
            let text = text.split("#[cfg(test)]\nmod tests").next().unwrap().to_string();
            (name, text)
        })
        .collect()
}

/// Where the code at `start` stops being code for a moment, and goes on: over a comment, a string or a character.
fn past_what_is_no_code(text: &[u8], start: usize) -> usize {
    match text[start] {
        b'/' if text.get(start + 1) == Some(&b'/') => text[start..]
            .iter()
            .position(|&c| c == b'\n')
            .map_or(text.len(), |n| start + n),
        b'"' => {
            let mut at = start + 1;
            while at < text.len() && text[at] != b'"' {
                at += if text[at] == b'\\' { 2 } else { 1 };
            }
            at + 1
        }
        b'\'' if text.get(start + 2) == Some(&b'\'') => start + 3,
        b'\'' if text.get(start + 1) == Some(&b'\\') && text.get(start + 3) == Some(&b'\'') => start + 4,
        _ => start,
    }
}

/// Where what opens at `open` with `opening` closes again.
fn closing(text: &str, open: usize, opening: u8, closes: u8) -> usize {
    let bytes = text.as_bytes();
    let (mut at, mut depth) = (open, 0);
    while at < bytes.len() {
        let behind = past_what_is_no_code(bytes, at);
        if behind != at {
            at = behind;
            continue;
        }
        if bytes[at] == opening {
            depth += 1;
        } else if bytes[at] == closes {
            depth -= 1;
            if depth == 0 {
                return at;
            }
        }
        at += 1;
    }
    panic!("nothing closes what opens at {open}");
}

/// The body of the function that starts behind `from`.
fn body_behind(text: &str, from: usize) -> &str {
    let signature = from + text[from..].find("fn ").unwrap();
    let parameters = signature + text[signature..].find('(').unwrap();
    let behind = closing(text, parameters, b'(', b')');
    let open = behind + text[behind..].find('{').unwrap();
    &text[open + 1..closing(text, open, b'{', b'}')]
}

fn without_comments(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `text` without the calls of `name`, whose arguments are guarded or the superclass's.
fn without_calls(text: &str, name: &str, opening: u8, closes: u8) -> String {
    let mut text = text.to_string();
    while let Some(at) = text.find(name) {
        let open = at + name.len() - 1;
        let close = closing(&text, open, opening, closes);
        text.replace_range(at..=close, "");
    }
    text
}

/// What the body runs outside of a guard that is ours.
fn runs_unguarded(body: &str) -> bool {
    let code = without_comments(body);
    let code = without_calls(&code, "guarded(", b'(', b')');
    let code = without_calls(&code, "contained(", b'(', b')');
    let code = without_calls(&code, "main_soon(", b'(', b')');
    let code = without_calls(&code, "main_after(", b'(', b')');
    let code = without_calls(&code, "msg_send![", b'[', b']');
    ["self.", "this.", "crate::", "super::"]
        .iter()
        .any(|ours| code.contains(ours))
        || calls_a_free_function(&code)
}

/// Whether `code` calls a function by its plain name, as a method or a path would have shown by their dot or colons, and as
/// neither a tuple struct nor `Some` would by their capital.
fn calls_a_free_function(code: &str) -> bool {
    let bytes = code.as_bytes();
    code.match_indices('(').any(|(open, _)| {
        let start = code[..open]
            .rfind(|c: char| !c.is_alphanumeric() && c != '_')
            .map_or(0, |at| at + 1);
        let name = &code[start..open];
        let by_itself = start == 0 || !matches!(bytes[start - 1], b'.' | b':' | b'!');
        by_itself
            && name.starts_with(|c: char| c.is_ascii_lowercase())
            && ![
                "if", "match", "while", "for", "return", "in", "fn", "let", "as", "move", "unsafe",
            ]
            .contains(&name)
    })
}

/// The selectors of the methods in `source` that run something of ours outside of a guard. `perform` guards itself.
fn unguarded_methods(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (at, _) in source.match_indices("#[unsafe(method") {
        let open = at + source[at..].find('(').unwrap();
        let open = open + 1 + source[open + 1..].find('(').unwrap();
        let selector = &source[open + 1..closing(source, open, b'(', b')')];
        let body = body_behind(source, at);
        let forwards = body.trim().starts_with("self.editing().perform(Action::");
        if !forwards && !NEEDS_NO_GUARD.iter().any(|(name, _)| *name == selector) && runs_unguarded(body) {
            found.push(selector.to_string());
        }
    }
    found
}

fn unguarded_blocks(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (at, _) in source.match_indices("RcBlock::new(") {
        let open = at + "RcBlock::new".len();
        let block = &source[open..=closing(source, open, b'(', b')')];
        let exempt = BLOCKS_THAT_NEED_NO_GUARD.iter().any(|(mark, _)| block.contains(mark));
        if !exempt && !guards_first(block) {
            found.push(block.lines().next().unwrap_or("").to_string());
        }
    }
    found
}

fn without_selftest_statements(text: &str) -> String {
    const MARK: &str = "#[cfg(feature = \"selftest\")]";
    let mut text = text.to_string();
    while let Some(at) = text.find(MARK) {
        let end = text[at..].find(';').map_or(text.len(), |n| at + n + 1);
        text.replace_range(at..end, "");
    }
    text
}

/// Whether the guard is what a block does, not one thing among others that it does.
fn guards_first(block: &str) -> bool {
    let Some(at) = block.find("guarded(") else {
        return false;
    };
    let head = &block[..at];
    let Some((behind_arguments, _)) = head.match_indices('|').nth(1) else {
        return false;
    };
    // What only the self-tests build may stand in front of it.
    !without_selftest_statements(&head[behind_arguments..]).contains(';')
}

fn raw_dispatches(source: &str) -> Vec<String> {
    source
        .lines()
        .filter(|line| {
            line.contains(".exec_async(") || line.contains(".exec_sync(") || line.contains("Queue::main().after(")
        })
        .map(|line| line.trim().to_string())
        .collect()
}

fn unguarded_functions_for_c(source: &str) -> Vec<String> {
    source
        .match_indices("extern \"C\" fn ")
        .filter(|(at, _)| !body_behind(source, *at).contains("guarded("))
        .map(|(at, _)| source[at..].lines().next().unwrap_or("").to_string())
        .collect()
}

#[test]
fn every_method_the_system_calls_is_guarded() {
    let found: Vec<String> = product()
        .iter()
        .flat_map(|(name, text)| unguarded_methods(text).into_iter().map(move |m| format!("{name}: {m}")))
        .collect();
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn every_block_the_system_calls_is_guarded() {
    let found: Vec<String> = product()
        .iter()
        .flat_map(|(name, text)| unguarded_blocks(text).into_iter().map(move |b| format!("{name}: {b}")))
        .collect();
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn every_function_the_system_calls_is_guarded() {
    let found: Vec<String> = product()
        .iter()
        .flat_map(|(name, text)| {
            unguarded_functions_for_c(text)
                .into_iter()
                .map(move |f| format!("{name}: {f}"))
        })
        .collect();
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn only_the_guarded_helpers_go_to_the_main_queue() {
    let found: Vec<String> = product()
        .iter()
        .filter(|(name, _)| !name.ends_with("lanes.rs"))
        .flat_map(|(name, text)| raw_dispatches(text).into_iter().map(move |d| format!("{name}: {d}")))
        .collect();
    assert!(found.is_empty(), "{found:#?}");
}

/// What stands between two calls by hand stays open when it panics, and the main thread survives that: a raised flag then
/// silences its document for good, a storage in editing tells nobody of a change, and a disabled manager leaves every later
/// change without an undo. `with_raised`, `editing` and `without_undo` hold for that case.
fn brackets_by_hand(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    for (n, line) in lines.iter().enumerate() {
        let code = line.trim();
        if code.starts_with("//") {
            continue;
        }
        if code.contains(".beginEditing()") || code.contains(".disableUndoRegistration()") {
            found.push(code.to_string());
        }
        if let Some(flag) = code.strip_suffix(".set(true);") {
            let lowered = format!("{flag}.set(false);");
            if lines[n + 1..].iter().take(20).any(|next| next.trim() == lowered) {
                found.push(flag.to_string());
            }
        }
    }
    found
}

#[test]
fn what_has_to_end_ends_also_after_a_panic() {
    let found: Vec<String> = product()
        .iter()
        .filter(|(name, _)| !name.ends_with("util/appkit.rs"))
        .flat_map(|(name, text)| brackets_by_hand(text).into_iter().map(move |b| format!("{name}: {b}")))
        .collect();
    assert!(found.is_empty(), "{found:#?}");
}

/// An address has one reading, in `splat_markdown::url`: its scheme may come in any case and with what a browser ignores.
fn addresses_read_by_hand(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .filter(|line| {
            [
                "starts_with(\"http",
                "starts_with(\"file:",
                "starts_with(\"data:",
                "strip_prefix(\"file:",
            ]
            .iter()
            .any(|by_hand| line.contains(by_hand))
        })
        .map(str::to_string)
        .collect()
}

#[test]
fn addresses_are_read_in_one_place() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for dir in ["src", "crates/splat-markdown/src", "crates/splat-preview/src"] {
        sources(&root.join(dir), &mut files);
    }
    assert!(files.len() > 50, "{} files found", files.len());
    for file in &files {
        let name = file.to_string_lossy();
        // `env.rs` reads the remote of a Git repository, which no document names.
        if name.contains("selftest") || name.ends_with("env.rs") || name.ends_with("tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(file).unwrap();
        let product = text.split("#[cfg(test)]\nmod tests").next().unwrap();
        let found = addresses_read_by_hand(product);
        assert!(found.is_empty(), "{}: {found:#?}", file.display());
    }
}

#[test]
fn an_address_read_by_hand_is_found() {
    assert_eq!(addresses_read_by_hand("if url.starts_with(\"https://\") {\n").len(), 1);
    assert_eq!(addresses_read_by_hand("url.strip_prefix(\"file://\")\n").len(), 1);
    assert!(addresses_read_by_hand("// url.starts_with(\"https://\")\n").is_empty());
    assert!(addresses_read_by_hand("url::is_https(url)\n").is_empty());
}

#[test]
fn a_bracket_by_hand_is_found() {
    let flag = "a.flag.set(true);\nself.go();\na.flag.set(false);\n";
    assert_eq!(brackets_by_hand(flag), ["a.flag"]);
    assert_eq!(brackets_by_hand("store.beginEditing();\n"), ["store.beginEditing();"]);
    assert_eq!(brackets_by_hand("undo.disableUndoRegistration();\n").len(), 1);
    assert!(
        brackets_by_hand("a.flag.set(true);\nself.go();\n").is_empty(),
        "a flag that stays"
    );
    assert!(
        brackets_by_hand("a.flag.set(true);\nb.flag.set(false);\n").is_empty(),
        "two flags"
    );
    assert!(brackets_by_hand("// store.beginEditing();\n").is_empty());
}

#[test]
fn the_sources_hold_what_these_checks_look_for() {
    let all: String = product().iter().map(|(_, text)| text.as_str()).collect();
    assert!(
        all.matches("#[unsafe(method").count() > 100,
        "the checks look at the wrong place"
    );
    assert!(all.matches("RcBlock::new(").count() > 15);
    assert!(all.matches("guarded(").count() > 80);
}

#[test]
fn a_method_without_a_guard_is_found() {
    let plain = "#[unsafe(method(a:))]\nfn a(&self) {\n    self.go();\n}\n";
    assert_eq!(unguarded_methods(plain), ["a:"]);
    let behind = "#[unsafe(method(a:))]\nfn a(&self) {\n    guarded((), || self.go());\n    self.more();\n}\n";
    assert_eq!(unguarded_methods(behind), ["a:"], "what stands behind the guard");
    let next = "#[unsafe(method(a:))]\nfn a(&self) { self.go(); }\n#[unsafe(method(b:))]\nfn b(&self) { guarded((), || self.go()); }\n";
    assert_eq!(
        unguarded_methods(next),
        ["a:"],
        "the guard of the next method counts for nothing"
    );
    let in_a_comment = "#[unsafe(method(a:))]\nfn a(&self) {\n    // guarded(\n    self.go();\n}\n";
    assert_eq!(unguarded_methods(in_a_comment), ["a:"]);
}

#[test]
fn a_free_function_counts_as_something_of_ours() {
    assert!(calls_a_free_function("go();"));
    assert!(calls_a_free_function("let a = go(1);"));
    assert!(!calls_a_free_function("a.go();"));
    assert!(!calls_a_free_function("Some(1); A::go(); println!(\"\");"));
    assert!(!calls_a_free_function("if (a) { return (b); }"));
    let free = "#[unsafe(method(a:))]\nfn a(&self) {\n    go();\n}\n";
    assert_eq!(unguarded_methods(free), ["a:"]);
}

#[test]
fn a_guard_has_to_be_what_a_block_does() {
    assert_eq!(
        unguarded_blocks("let b = RcBlock::new(|x| { go(x); guarded((), || y()) });\n").len(),
        1
    );
    assert!(unguarded_blocks("let b = RcBlock::new(move |x| { guarded((), || y(x)) });\n").is_empty());
}

#[test]
fn a_method_that_needs_no_guard_passes() {
    assert!(unguarded_methods("#[unsafe(method(a))]\nfn a() -> bool {\n    true\n}\n").is_empty());
    let guarded = "#[unsafe(method(a:))]\nfn a(&self) {\n    guarded((), || self.go());\n}\n";
    assert!(unguarded_methods(guarded).is_empty());
    let with_super = "#[unsafe(method(a:))]\nfn a(&self) {\n    let _: () = unsafe { msg_send![super(self), a] };\n    guarded((), || self.go());\n}\n";
    assert!(unguarded_methods(with_super).is_empty());
    let id = "#[unsafe(method_id(a:))]\nfn a(&self) -> Option<Retained<A>> {\n    guarded(None, || self.go())\n}\n";
    assert!(unguarded_methods(id).is_empty());
    let forwards = "#[unsafe(method(a:))]\nfn a(&self, s: S) { self.editing().perform(Action::A, s) }\n";
    assert!(unguarded_methods(forwards).is_empty());
}

#[test]
fn blocks_functions_and_dispatches_without_a_guard_are_found() {
    assert_eq!(unguarded_blocks("let b = RcBlock::new(|x| go(x));\n").len(), 1);
    assert!(unguarded_blocks("let b = RcBlock::new(|x| guarded((), || go(x)));\n").is_empty());
    assert_eq!(
        unguarded_functions_for_c("extern \"C\" fn a(_: *mut c_void) {\n    go();\n}\n").len(),
        1
    );
    assert!(unguarded_functions_for_c("extern \"C\" fn a(_: *mut c_void) {\n    guarded((), go);\n}\n").is_empty());
    assert_eq!(
        raw_dispatches("DispatchQueue::main().exec_async(move || go());\n").len(),
        1
    );
    assert_eq!(
        raw_dispatches("let _ = DispatchQueue::main().after(when, go);\n").len(),
        1
    );
    assert!(raw_dispatches("main_soon(move || go());\n").is_empty());
}

#[test]
fn what_closes_is_found_past_strings_and_comments() {
    let text = "{ \"}\" // }\n '}' { } }";
    assert_eq!(closing(text, 0, b'{', b'}'), text.len() - 1);
}
