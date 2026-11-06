use super::*;

fn source(module: &[&str], text: &str, without: Without) -> Source {
    let module = module.iter().map(ToString::to_string).collect();
    Source::parse("a.rs", module, text.to_string(), without)
}

/// The modules `text` reaches, written in `module`.
fn reached(module: &[&str], text: &str) -> Vec<String> {
    let source = source(module, text, Without::Tests);
    let modules = Crate::of(Vec::new()).modules;
    named(&source)
        .iter()
        .filter_map(|n| resolve(&n.module, &n.path, &modules))
        .map(|path| path.join("::"))
        .collect()
}

fn functions(source: &Source) -> Vec<String> {
    struct Names(Vec<String>);
    impl<'ast> syn::visit::Visit<'ast> for Names {
        fn visit_ident(&mut self, ident: &'ast syn::Ident) {
            self.0.push(ident.to_string());
        }
    }
    let mut names = Names(Vec::new());
    syn::visit::Visit::visit_file(&mut names, &source.syntax);
    names.0
}

#[test]
fn test_code_goes_by_its_attribute_wherever_it_stands() {
    let text = "fn shipped() {}\n\
        #[cfg(test)]\nmod order_tests { fn in_test_module() {} }\n\
        #[cfg(test)]\nfn helper() {}\n\
        fn after_the_tests() { #[cfg(test)] let in_statement = 1; }\n\
        #[test]\nfn a_test() {}\n\
        #[cfg(all(test, unix))]\nfn all_of_them() {}\n\
        #[cfg(any(test, feature = \"selftest\"))]\nfn either() {}\n\
        #[cfg(not(feature = \"selftest\"))]\nfn not_selftest() {}\n\
        #[cfg(feature = \"selftest\")]\nfn selftest() {}\n\
        struct S { kept: u8, #[cfg(test)] field_for_tests: u8 }\n";
    let names = functions(&source(&[], text, Without::Tests));
    for kept in [
        "shipped",
        "after_the_tests",
        "either",
        "not_selftest",
        "selftest",
        "kept",
    ] {
        assert!(names.iter().any(|n| n == kept), "{kept} in {names:?}");
    }
    for gone in [
        "order_tests",
        "in_test_module",
        "helper",
        "in_statement",
        "a_test",
        "all_of_them",
        "field_for_tests",
    ] {
        assert!(!names.iter().any(|n| n == gone), "{gone} in {names:?}");
    }
    let shipped = functions(&source(&[], text, Without::TestsAndSelftest));
    assert!(shipped.iter().any(|n| n == "not_selftest"));
    assert!(!shipped.iter().any(|n| n == "selftest" || n == "either"), "{shipped:?}");
    assert!(
        functions(&source(&[], text, Without::Nothing))
            .iter()
            .any(|n| n == "helper")
    );
}

#[test]
fn a_selftest_statement_goes_with_the_selftest() {
    let text = "fn f() { #[cfg(feature = \"selftest\")] marked(); guarded(); }";
    let names = functions(&source(&[], text, Without::TestsAndSelftest));
    assert!(!names.iter().any(|n| n == "marked"), "{names:?}");
    assert!(names.iter().any(|n| n == "guarded"), "{names:?}");
}

#[test]
fn paths_reach_the_module_they_name_from_where_they_stand() {
    let text = "use super::doc::{A, B};\nuse super::{doc, text::Byte};\nfn f() { crate::url::x(); }\n";
    assert_eq!(
        reached(&["html"], text),
        ["doc::A", "doc::B", "doc", "text::Byte", "url::x"]
    );
    assert_eq!(
        reached(&["commands"], "use super::doc::Block;"),
        ["doc::Block"],
        "from a folder's mod.rs"
    );
    assert_eq!(
        reached(&["a", "b", "c"], "use super::super::super::text::Byte;"),
        ["text::Byte"]
    );
    assert_eq!(reached(&["a"], "use self::inner::X;"), ["a::inner::X"]);
    assert_eq!(
        reached(&["a"], "mod inner { use super::super::b::X; }"),
        ["b::X"],
        "from an inline module"
    );
    assert_eq!(
        reached(&["a"], "fn f() { format!(\"{}\", crate::b::x()); }"),
        ["b::x"],
        "in a macro"
    );
    assert_eq!(
        reached(&["a"], "macro_rules! m { () => { $crate::b::x() } }"),
        ["b::x"],
        "in what reads as no Rust"
    );
    assert!(reached(&["a"], "use std::fmt; fn f() -> Option<u8> { None }").is_empty());
    assert!(
        reached(&["a"], "use super::super::super::x;").is_empty(),
        "above the crate"
    );
}

#[test]
fn a_crate_is_read_by_what_it_declares() {
    let dir = std::env::temp_dir().join(format!("splat-sources-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("folder")).unwrap();
    std::fs::create_dir_all(dir.join("inline")).unwrap();
    std::fs::create_dir_all(dir.join("file")).unwrap();
    let files = [
        (
            "lib.rs",
            "mod file;\nmod folder;\nmod inline { mod inner; }\n#[cfg(test)]\nmod tests;\n",
        ),
        ("file.rs", "mod below;\n"),
        ("file/below.rs", ""),
        ("folder/mod.rs", "mod child;\n"),
        ("folder/child.rs", ""),
        ("inline/inner.rs", ""),
        ("tests.rs", "this is no Rust"),
        ("stray.rs", "this is no Rust either"),
    ];
    for (name, text) in files {
        std::fs::write(dir.join(name), text).unwrap();
    }
    let read = Crate::read(&dir.join("lib.rs"), Without::Tests);
    let modules: Vec<String> = read.modules.iter().map(|m| m.join("::")).collect();
    assert_eq!(
        modules,
        [
            "",
            "file",
            "file::below",
            "folder",
            "folder::child",
            "inline",
            "inline::inner"
        ]
    );
    assert_eq!(read.sources.len(), 6);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn macros_are_read_as_the_rust_they_hold() {
    let text = "define_class!(\n#[unsafe(super(NSObject))]\nstruct A;\nimpl A {\n#[unsafe(method(a:))]\nfn a(&self) {}\n}\n);\n\
        fn f() { msg_send![super(self), b: one(), c: two()]; msg_send![x.y(), d]; format!(\"{}\", three()); vec![four(); 2]; }";
    let names = functions(&source(&[], text, Without::Tests));
    assert!(
        !names.iter().any(|n| n == "one"),
        "a plain visit sees only tokens there"
    );
    struct Calls(Vec<String>);
    impl<'ast> syn::visit::Visit<'ast> for Calls {
        fn visit_ident(&mut self, ident: &'ast syn::Ident) {
            self.0.push(ident.to_string());
        }
        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            if let Some(body) = body(mac, Without::Tests) {
                visit_body(self, &body);
            }
        }
    }
    let mut calls = Calls(Vec::new());
    syn::visit::Visit::visit_file(&mut calls, &source(&[], text, Without::Tests).syntax);
    for seen in ["a", "one", "two", "x", "y", "three", "four"] {
        assert!(calls.0.iter().any(|n| n == seen), "{seen} in {:?}", calls.0);
    }
    assert!(
        !calls.0.iter().any(|n| n == "b" || n == "d"),
        "selectors are no code: {:?}",
        calls.0
    );
}

#[test]
fn a_source_says_where_its_code_stands() {
    let source = source(&[], "fn a() {}\n\nfn b() { go(); }\n", Without::Tests);
    let syn::Item::Fn(b) = &source.syntax.items[1] else {
        panic!("a function");
    };
    assert_eq!(source.at(b.sig.ident.span()), "a.rs:3");
    assert_eq!(source.snippet(b.sig.ident.span()), "b");
}

#[test]
#[should_panic(expected = "a.rs:2:")]
fn a_source_no_parser_reads_fails_with_its_place() {
    source(&[], "fn a() {}\nfn (", Without::Tests);
}

#[test]
fn the_workspace_is_found() {
    assert!(root().join("Cargo.toml").exists());
    let roots = crate_roots();
    assert!(roots.len() >= 8, "{roots:?}");
    assert!(rust_files(&root().join("tests")).len() >= 3);
}
