//! Everything the system calls goes through a guard against panics: one that leaves a dispatch block ends the app, and one
//! that leaves a method or a block unwinds through the system's frames. This looks at the sources, as no type can say it.

use splat_testkit::sources::proc_macro2::{Delimiter, Span, TokenStream, TokenTree};
use splat_testkit::sources::syn::spanned::Spanned;
use splat_testkit::sources::syn::visit::{self, Visit};
use splat_testkit::sources::syn::{self, Expr, Stmt};
use splat_testkit::sources::{Crate, Source, Without, line, root, visit_body};

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

/// Macros whose tokens read as no Rust, which the checks therefore cannot look into.
const UNREAD_MACROS: [(&str, &str); 1] = [(
    "layer",
    "names the types and methods of a layer, and the methods it makes forward to the controller",
)];

/// Calls whose arguments run behind a guard of ours.
const GUARDS: [&str; 4] = ["guarded", "contained", "main_soon", "main_after"];

/// What ships of the app, without tests and self-tests.
fn product() -> Crate {
    Crate::read(&root().join("src/main.rs"), Without::TestsAndSelftest)
}

fn scanned(source: &Source) -> Found {
    let mut scan = Scan {
        source,
        found: Found::default(),
    };
    scan.visit_file(&source.syntax);
    scan.found
}

fn scan_text(text: &str) -> Found {
    scanned(&Source::parse(
        "a.rs",
        Vec::new(),
        text.to_string(),
        Without::TestsAndSelftest,
    ))
}

/// What the checks found, each as `file:line: what`.
#[derive(Default)]
struct Found {
    unguarded_methods: Vec<String>,
    unguarded_blocks: Vec<String>,
    unguarded_functions: Vec<String>,
    raw_dispatches: Vec<String>,
    methods: usize,
    blocks: usize,
    guards: usize,
    /// Macros whose tokens read as no Rust, by name and place: what they hold escapes every check but `runs_ours`.
    unread: Vec<String>,
}

struct Scan<'s> {
    source: &'s Source,
    found: Found,
}

/// The name a call goes to: the last segment of its path.
fn callee(call: &syn::ExprCall) -> Option<(String, Vec<String>)> {
    let Expr::Path(path) = &*call.func else {
        return None;
    };
    let segments: Vec<String> = path.path.segments.iter().map(|s| s.ident.to_string()).collect();
    Some((segments.last()?.clone(), segments))
}

fn is_guard(expr: &Expr) -> bool {
    match expr {
        Expr::Call(call) => callee(call).is_some_and(|(name, _)| name == "guarded"),
        Expr::Paren(inner) => is_guard(&inner.expr),
        _ => false,
    }
}

/// Whether the guard is all `block` does, so that nothing in front of it or behind it runs unguarded.
fn guard_is_all(block: &syn::Block) -> bool {
    match block.stmts.as_slice() {
        [Stmt::Expr(expr, _)] => is_guard(expr),
        _ => false,
    }
}

/// Whether the closure handed to a block constructor starts with its guard, and runs nothing of ours behind it, as the
/// system's own completion handler may be called there.
fn guards_first(source: &Source, expr: &Expr) -> bool {
    let Expr::Closure(closure) = expr else {
        return false;
    };
    let Expr::Block(body) = &*closure.body else {
        return is_guard(&closure.body);
    };
    match body.block.stmts.split_first() {
        Some((Stmt::Expr(first, _), behind)) if is_guard(first) => !runs_ours(source, behind),
        _ => false,
    }
}

/// The selector of a method `define_class!` hands to the runtime.
fn selector(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| {
        let syn::Meta::List(outer) = &attr.meta else {
            return None;
        };
        let parsed;
        let inner = if outer.path.is_ident("unsafe") {
            parsed = outer.parse_args::<syn::Meta>().ok()?;
            &parsed
        } else {
            &attr.meta
        };
        let syn::Meta::List(method) = inner else {
            return None;
        };
        let named = method.path.is_ident("method") || method.path.is_ident("method_id");
        named.then(|| method.tokens.to_string().replace(' ', ""))
    })
}

/// A method that hands its call on to `perform`, which guards itself.
fn forwards(block: &syn::Block) -> bool {
    let [Stmt::Expr(Expr::MethodCall(perform), _)] = block.stmts.as_slice() else {
        return false;
    };
    let Expr::MethodCall(editing) = &*perform.receiver else {
        return false;
    };
    let action = perform.args.first().is_some_and(
        |first| matches!(first, Expr::Path(path) if path.path.segments.first().is_some_and(|s| s.ident == "Action")),
    );
    perform.method == "perform" && editing.method == "editing" && is_self(&editing.receiver) && action
}

fn is_self(expr: &Expr) -> bool {
    matches!(expr, Expr::Path(path) if path.path.is_ident("self") || path.path.is_ident("this"))
}

fn is_c(sig: &syn::Signature) -> bool {
    sig.abi
        .as_ref()
        .is_some_and(|abi| abi.name.as_ref().is_none_or(|name| name.value() != "Rust"))
}

impl Scan<'_> {
    fn first_line(&self, span: Span) -> String {
        let text = self.source.snippet(span).lines().next().unwrap_or("").trim();
        format!("{}: {text}", self.source.at(span))
    }

    fn method(&mut self, attrs: &[syn::Attribute], sig: &syn::Signature, block: &syn::Block) {
        let Some(selector) = selector(attrs) else {
            return;
        };
        self.found.methods += 1;
        let exempt = NEEDS_NO_GUARD.iter().any(|(name, _)| *name == selector);
        if !exempt && !forwards(block) && runs_ours(self.source, &block.stmts) {
            let at = self.source.at(sig.ident.span());
            self.found.unguarded_methods.push(format!("{at}: {selector}"));
        }
    }

    fn function_for_c(&mut self, sig: &syn::Signature, block: &syn::Block) {
        if is_c(sig) && !guard_is_all(block) {
            self.found.unguarded_functions.push(self.first_line(sig.span()));
        }
    }
}

impl<'ast> Visit<'ast> for Scan<'_> {
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.method(&item.attrs, &item.sig, &item.block);
        self.function_for_c(&item.sig, &item.block);
        visit::visit_impl_item_fn(self, item);
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.method(&item.attrs, &item.sig, &item.block);
        self.function_for_c(&item.sig, &item.block);
        visit::visit_item_fn(self, item);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let Some((name, segments)) = callee(call) {
            let block =
                segments.len() >= 2 && ["RcBlock", "StackBlock"].contains(&segments[segments.len() - 2].as_str());
            if block && name == "new" {
                self.found.blocks += 1;
                let argument = call.args.first();
                let text = argument.map_or("", |a| self.source.snippet(a.span()));
                let exempt = BLOCKS_THAT_NEED_NO_GUARD.iter().any(|(mark, _)| text.contains(mark));
                if !exempt && !argument.is_some_and(|a| guards_first(self.source, a)) {
                    self.found.unguarded_blocks.push(self.first_line(call.span()));
                }
            }
            if name == "guarded" {
                self.found.guards += 1;
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let on_the_main_queue = || match &*call.receiver {
            Expr::Call(queue) => callee(queue).is_some_and(|(name, segments)| {
                name == "main" && segments.len() >= 2 && segments[segments.len() - 2].ends_with("Queue")
            }),
            _ => false,
        };
        if call.method == "exec_async" || call.method == "exec_sync" || (call.method == "after" && on_the_main_queue())
        {
            self.found.raw_dispatches.push(self.first_line(call.span()));
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        match self.source.body(mac) {
            Some(body) => visit_body(self, &body),
            None if !mac.path.is_ident("macro_rules") => {
                let name = mac
                    .path
                    .segments
                    .last()
                    .map(|s| s.ident.to_string())
                    .unwrap_or_default();
                let at = self.source.at(mac.path.span());
                self.found.unread.push(format!("{at}: {name}!"));
            }
            None => {}
        }
    }
}

/// Whether `stmts` run something of ours outside of a guard: a method or field of `self`, a path of the crate, or a
/// function by its plain name, as a method or a path would have shown by their dot or colons, and as neither a tuple
/// struct nor `Some` would by their capital.
fn runs_ours(source: &Source, stmts: &[Stmt]) -> bool {
    let mut ours = Ours { source, found: false };
    stmts.iter().for_each(|stmt| ours.visit_stmt(stmt));
    ours.found
}

struct Ours<'s> {
    source: &'s Source,
    found: bool,
}

fn of_the_crate(path: &syn::Path) -> bool {
    path.segments.len() > 1
        && path
            .segments
            .first()
            .is_some_and(|s| s.ident == "crate" || s.ident == "super" || s.ident == "self")
}

/// What reads as no Rust, by its tokens: `self.`, a path of the crate, or a plain name in front of parentheses.
fn tokens_run_ours(tokens: &TokenStream) -> bool {
    let trees: Vec<TokenTree> = tokens.clone().into_iter().collect();
    trees.iter().enumerate().any(|(at, tree)| match tree {
        TokenTree::Group(group) => tokens_run_ours(&group.stream()),
        TokenTree::Ident(ident) => {
            let next = trees.get(at + 1);
            let punct = |c: char| matches!(next, Some(TokenTree::Punct(p)) if p.as_char() == c);
            let behind = at.checked_sub(1).and_then(|before| trees.get(before));
            let by_itself = !matches!(behind, Some(TokenTree::Punct(p)) if matches!(p.as_char(), '.' | ':' | '!'));
            let name = ident.to_string();
            ((name == "self" || name == "this") && punct('.'))
                || ((name == "crate" || name == "super") && punct(':'))
                || (by_itself
                    && name.starts_with(|c: char| c.is_ascii_lowercase())
                    && matches!(next, Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis))
        }
        TokenTree::Punct(_) | TokenTree::Literal(_) => false,
    })
}

impl<'ast> Visit<'ast> for Ours<'_> {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let Expr::Path(path) = &*call.func {
            let name = path
                .path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default();
            if GUARDS.contains(&name.as_str()) {
                return;
            }
            let plain = path.path.segments.len() == 1 && path.qself.is_none();
            if plain && name.starts_with(|c: char| c.is_ascii_lowercase()) {
                self.found = true;
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        self.found |= is_self(&call.receiver);
        visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_field(&mut self, field: &'ast syn::ExprField) {
        self.found |= is_self(&field.base);
        visit::visit_expr_field(self, field);
    }

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        self.found |= of_the_crate(&path.path);
        visit::visit_expr_path(self, path);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.found |= of_the_crate(&mac.path);
        match self.source.body(mac) {
            Some(body) => visit_body(self, &body),
            None => self.found |= tokens_run_ours(&mac.tokens),
        }
    }

    /// An item in a body runs only when called, which is a call of a plain name.
    fn visit_item(&mut self, _: &'ast syn::Item) {}
}

/// Every finding of one kind across the product.
fn across_the_product(kind: fn(Found) -> Vec<String>) -> Vec<String> {
    product()
        .sources
        .iter()
        .flat_map(|source| kind(scanned(source)))
        .collect()
}

#[test]
fn every_method_the_system_calls_is_guarded() {
    let found = across_the_product(|f| f.unguarded_methods);
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn every_block_the_system_calls_is_guarded() {
    let found = across_the_product(|f| f.unguarded_blocks);
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn every_function_the_system_calls_is_guarded() {
    let found = across_the_product(|f| f.unguarded_functions);
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn only_the_guarded_helpers_go_to_the_main_queue() {
    let found: Vec<String> = across_the_product(|f| f.raw_dispatches)
        .into_iter()
        .filter(|at| !at.starts_with("src/util/lanes.rs:"))
        .collect();
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn the_sources_hold_what_these_checks_look_for() {
    let product = product();
    let found: Vec<Found> = product.sources.iter().map(scanned).collect();
    let methods: usize = found.iter().map(|f| f.methods).sum();
    let blocks: usize = found.iter().map(|f| f.blocks).sum();
    let guards: usize = found.iter().map(|f| f.guards).sum();
    assert!(methods > 100, "the checks look at the wrong place: {methods} methods");
    assert!(blocks > 15, "{blocks} blocks");
    assert!(guards > 80, "{guards} guards");
    assert!(
        product.sources.iter().all(|s| !s.name.contains("selftest")),
        "the self-test is no product"
    );
    let unread: Vec<String> = found
        .into_iter()
        .flat_map(|f| f.unread)
        .filter(|at| {
            !UNREAD_MACROS
                .iter()
                .any(|(name, _)| at.ends_with(&format!(": {name}!")))
        })
        .collect();
    assert!(unread.is_empty(), "macros the checks cannot read: {unread:#?}");
}

#[test]
fn a_method_handed_on_to_another_macro_is_checked() {
    let text = "commands!(class, [\n#[ivars = Cell<u8>]\npub struct A;\nimpl A {\n#[unsafe(method(a:))]\nfn a(&self) {\nself.go();\n}\n}\n]);\n";
    assert_eq!(scan_text(text).unguarded_methods, ["a.rs:6: a:"]);
}

#[test]
fn a_method_without_a_guard_is_found() {
    let method = |body: &str| format!("impl A {{\n#[unsafe(method(a:))]\nfn a(&self) {{\n{body}\n}}\n}}\n");
    let unguarded = |text: &str| scan_text(text).unguarded_methods;
    assert_eq!(unguarded(&method("self.go();")), ["a.rs:3: a:"]);
    assert_eq!(
        unguarded(&method("guarded((), || self.go());\nself.more();")).len(),
        1,
        "what stands behind the guard"
    );
    let next = "impl A {\n#[unsafe(method(a:))]\nfn a(&self) { self.go(); }\n#[unsafe(method(b:))]\nfn b(&self) { guarded((), || self.go()); }\n}\n";
    assert_eq!(
        unguarded(next),
        ["a.rs:3: a:"],
        "the guard of the next method counts for nothing"
    );
    assert_eq!(unguarded(&method("// guarded(\nself.go();")).len(), 1);
    assert_eq!(unguarded(&method("let x = \"guarded(\"; self.go();")).len(), 1);
    assert_eq!(
        unguarded(&method("eprintln!(\"{}\", self.name());")).len(),
        1,
        "in a macro"
    );
    assert_eq!(unguarded(&method("crate::util::go();")).len(), 1);
}

#[test]
fn what_a_message_carries_counts_as_code() {
    let method = |body: &str| format!("impl A {{\n#[unsafe(method(a:))]\nfn a(&self) {{\n{body}\n}}\n}}\n");
    let unguarded = |text: &str| scan_text(text).unguarded_methods;
    assert_eq!(
        unguarded(&method("let _: () = unsafe { msg_send![super(self), a: self.go()] };")).len(),
        1
    );
    assert_eq!(
        unguarded(&method("let _: () = unsafe { msg_send![self.target(), a] };")).len(),
        1
    );
    assert_eq!(
        unguarded(&method("let _: () = unsafe { msg_send![super(self), a: crate::x()] };")).len(),
        1
    );
    assert!(unguarded(&method("let _: () = unsafe { msg_send![super(self), a: sender] };")).is_empty());
}

#[test]
fn a_free_function_counts_as_something_of_ours() {
    let method = |body: &str| format!("impl A {{\n#[unsafe(method(a:))]\nfn a(&self) {{\n{body}\n}}\n}}\n");
    let runs = |body: &str| !scan_text(&method(body)).unguarded_methods.is_empty();
    assert!(runs("go();"));
    assert!(runs("let a = go(1);"));
    assert!(!runs("a.go();"));
    assert!(!runs("Some(1); A::go(); println!(\"\");"));
    assert!(!runs("if a { return b; }"));
    assert!(tokens_run_ours(&"x => go()".parse().unwrap()));
    assert!(!tokens_run_ours(&"x => A::go(), y.go()".parse().unwrap()));
}

#[test]
fn a_guard_has_to_be_what_a_block_does() {
    let unguarded = |text: &str| scan_text(text).unguarded_blocks.len();
    assert_eq!(
        unguarded("fn f() { let b = RcBlock::new(|x| { go(x); guarded((), || y()) }); }"),
        1
    );
    assert_eq!(
        unguarded("fn f() { let b = RcBlock::new(|x| { guarded((), || y()); go(x) }); }"),
        1,
        "behind it"
    );
    let handler = "fn f() { let b = RcBlock::new(move |x| { guarded((), || y()); handler.call((x,)); }); }";
    assert_eq!(unguarded(handler), 0, "the system's own handler behind it");
    assert_eq!(
        unguarded("fn f() { let b = RcBlock::new(move |x| { guarded((), || y(x)) }); }"),
        0
    );
    let selftest =
        "fn f() { let b = RcBlock::new(|x| {\n#[cfg(feature = \"selftest\")]\nnote();\nguarded((), || y(x))\n}); }";
    assert_eq!(unguarded(selftest), 0, "what only the self-test builds");
}

#[test]
fn a_method_that_needs_no_guard_passes() {
    let unguarded = |text: &str| scan_text(text).unguarded_methods;
    assert!(unguarded("impl A {\n#[unsafe(method(a))]\nfn a() -> bool {\n    true\n}\n}").is_empty());
    let guarded = "impl A {\n#[unsafe(method(a:))]\nfn a(&self) {\n    guarded((), || self.go());\n}\n}";
    assert!(unguarded(guarded).is_empty());
    let with_super = "impl A {\n#[unsafe(method(a:))]\nfn a(&self) {\n    let _: () = unsafe { msg_send![super(self), a] };\n    guarded((), || self.go());\n}\n}";
    assert!(unguarded(with_super).is_empty());
    let id = "impl A {\n#[unsafe(method_id(a:))]\nfn a(&self) -> Option<Retained<A>> {\n    guarded(None, || self.go())\n}\n}";
    assert!(unguarded(id).is_empty());
    let forwards = "impl A {\n#[unsafe(method(a:))]\nfn a(&self, s: S) { self.editing().perform(Action::A, s) }\n}";
    assert!(unguarded(forwards).is_empty());
    let more =
        "impl A {\n#[unsafe(method(a:))]\nfn a(&self, s: S) { self.editing().perform(Action::A, s); self.go() }\n}";
    assert_eq!(unguarded(more).len(), 1, "what follows the forward");
    let in_a_class = "define_class!(\n#[unsafe(super(NSObject))]\nstruct A;\nimpl A {\n#[unsafe(method(a:))]\nfn a(&self) { self.go(); }\n}\n);";
    assert_eq!(unguarded(in_a_class), ["a.rs:6: a:"], "in define_class!");
}

#[test]
fn blocks_functions_and_dispatches_without_a_guard_are_found() {
    assert_eq!(
        scan_text("fn f() { let b = RcBlock::new(|x| go(x)); }")
            .unguarded_blocks
            .len(),
        1
    );
    assert!(
        scan_text("fn f() { let b = RcBlock::new(|x| guarded((), || go(x))); }")
            .unguarded_blocks
            .is_empty()
    );
    assert_eq!(
        scan_text("fn f() { let b = block2::StackBlock::new(|x| found.set(x)); }").unguarded_blocks,
        ["a.rs:1: block2::StackBlock::new(|x| found.set(x))"]
    );
    let c = |body: &str| scan_text(&format!("extern \"C\" fn a(_: *mut c_void) {{\n{body}\n}}\n")).unguarded_functions;
    assert_eq!(c("go();").len(), 1);
    assert!(c("guarded((), go);").is_empty());
    assert!(c("crate::util::guarded((), || go())").is_empty());
    assert_eq!(c("let x = go();\nguarded((), || x);").len(), 1, "in front of the guard");
    assert_eq!(c("if ready() { guarded((), go); }").len(), 1, "the guard inside");
    assert_eq!(
        scan_text("fn outer() {\n    unsafe extern \"C\" fn inner() { go() }\n}")
            .unguarded_functions
            .len(),
        1
    );
    assert!(
        scan_text("unsafe extern \"C\" { fn declared(); }")
            .unguarded_functions
            .is_empty()
    );
    assert_eq!(
        scan_text("fn f() { DispatchQueue::main().exec_async(move || go()); }")
            .raw_dispatches
            .len(),
        1
    );
    assert_eq!(
        scan_text("fn f() { let _ = DispatchQueue::main().after(when, go); }")
            .raw_dispatches
            .len(),
        1
    );
    assert!(
        scan_text("fn f() { main_soon(move || go()); }")
            .raw_dispatches
            .is_empty()
    );
}

/// What stands between two calls by hand stays open when it panics, and the main thread survives that: a raised flag then
/// silences its document for good, a storage in editing tells nobody of a change, and a disabled manager leaves every later
/// change without an undo. `with_raised`, `editing` and `without_undo` hold for that case.
fn brackets_by_hand(source: &Source) -> Vec<String> {
    struct Brackets<'s> {
        source: &'s Source,
        found: Vec<String>,
    }
    /// The receiver of `receiver.set(value)`, as written without spaces.
    fn set(source: &Source, expr: &Expr, value: bool) -> Option<String> {
        let Expr::MethodCall(call) = expr else {
            return None;
        };
        let literal = matches!(call.args.first(), Some(Expr::Lit(syn::ExprLit { lit: syn::Lit::Bool(b), .. })) if b.value == value);
        (call.method == "set" && call.args.len() == 1 && literal)
            .then(|| source.snippet(call.receiver.span()).split_whitespace().collect())
    }
    struct Lowers<'a> {
        source: &'a Source,
        flag: &'a str,
        within: std::ops::RangeInclusive<usize>,
        found: bool,
    }
    impl<'ast> Visit<'ast> for Lowers<'_> {
        fn visit_expr(&mut self, expr: &'ast Expr) {
            self.found |= self.within.contains(&line(expr.span()))
                && set(self.source, expr, false).is_some_and(|f| f == self.flag);
            visit::visit_expr(self, expr);
        }
    }
    impl<'ast> Visit<'ast> for Brackets<'_> {
        fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
            if call.method == "beginEditing" || call.method == "disableUndoRegistration" {
                let at = self.source.at(call.method.span());
                self.found.push(format!("{at}: {}", call.method));
            }
            visit::visit_expr_method_call(self, call);
        }
        fn visit_block(&mut self, block: &'ast syn::Block) {
            for (n, stmt) in block.stmts.iter().enumerate() {
                let Stmt::Expr(expr, _) = stmt else { continue };
                let Some(flag) = set(self.source, expr, true) else {
                    continue;
                };
                let at = line(expr.span());
                let mut lowers = Lowers {
                    source: self.source,
                    flag: &flag,
                    within: at..=at + 20,
                    found: false,
                };
                block.stmts[n + 1..].iter().for_each(|later| lowers.visit_stmt(later));
                if lowers.found {
                    let at = self.source.at(expr.span());
                    self.found.push(format!("{at}: {flag}"));
                }
            }
            visit::visit_block(self, block);
        }
        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            if let Some(body) = self.source.body(mac) {
                visit_body(self, &body);
            }
        }
    }
    let mut brackets = Brackets {
        source,
        found: Vec::new(),
    };
    brackets.visit_file(&source.syntax);
    brackets.found
}

#[test]
fn what_has_to_end_ends_also_after_a_panic() {
    let found: Vec<String> = product()
        .sources
        .iter()
        .filter(|source| source.name != "src/util/appkit.rs")
        .flat_map(brackets_by_hand)
        .collect();
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn a_bracket_by_hand_is_found() {
    let found = |body: &str| {
        brackets_by_hand(&Source::parse(
            "a.rs",
            Vec::new(),
            format!("fn f() {{\n{body}\n}}"),
            Without::Tests,
        ))
    };
    assert_eq!(
        found("a.flag.set(true);\nself.go();\na.flag.set(false);"),
        ["a.rs:2: a.flag"]
    );
    assert_eq!(found("store.beginEditing();"), ["a.rs:2: beginEditing"]);
    assert_eq!(found("undo.disableUndoRegistration();").len(), 1);
    assert_eq!(
        found("a.flag.set(true);\nif x { a.flag.set(false); }").len(),
        1,
        "lowered further in"
    );
    assert!(found("a.flag.set(true);\nself.go();").is_empty(), "a flag that stays");
    assert!(found("a.flag.set(true);\nb.flag.set(false);").is_empty(), "two flags");
    assert!(found("// store.beginEditing();").is_empty());
}

/// An address has one reading, in `splat_markdown::url`: its scheme may come in any case and with what a browser ignores.
fn addresses_read_by_hand(source: &Source) -> Vec<String> {
    struct Addresses<'s> {
        source: &'s Source,
        found: Vec<String>,
    }
    impl<'ast> Visit<'ast> for Addresses<'_> {
        fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
            if let Some(Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(prefix),
                ..
            })) = call.args.first()
            {
                let prefix = prefix.value();
                let schemes: &[&str] = match call.method.to_string().as_str() {
                    "starts_with" => &["http", "file:", "data:"],
                    "strip_prefix" => &["file:"],
                    _ => &[],
                };
                if schemes.iter().any(|scheme| prefix.starts_with(scheme)) {
                    let at = self.source.at(call.method.span());
                    self.found.push(format!("{at}: {}({prefix:?})", call.method));
                }
            }
            visit::visit_expr_method_call(self, call);
        }
        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            if let Some(body) = self.source.body(mac) {
                visit_body(self, &body);
            }
        }
    }
    let mut addresses = Addresses {
        source,
        found: Vec::new(),
    };
    addresses.visit_file(&source.syntax);
    addresses.found
}

#[test]
fn addresses_are_read_in_one_place() {
    let root = root();
    let crates = [
        "src/main.rs",
        "crates/splat-markdown/src/lib.rs",
        "crates/splat-preview/src/lib.rs",
    ];
    let sources: Vec<Source> = crates
        .iter()
        .flat_map(|file| Crate::read(&root.join(file), Without::TestsAndSelftest).sources)
        .collect();
    assert!(sources.len() > 50, "{} files found", sources.len());
    let found: Vec<String> = sources
        .iter()
        // `env.rs` reads the remote of a Git repository, which no document names.
        .filter(|source| source.name != "src/editor/env.rs")
        .flat_map(addresses_read_by_hand)
        .collect();
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn an_address_read_by_hand_is_found() {
    let found = |body: &str| {
        addresses_read_by_hand(&Source::parse(
            "a.rs",
            Vec::new(),
            format!("fn f() {{\n{body}\n}}"),
            Without::Tests,
        ))
    };
    assert_eq!(found("if url.starts_with(\"https://\") {}").len(), 1);
    assert_eq!(found("url.strip_prefix(\"file://\");").len(), 1);
    assert_eq!(found("assert!(url.starts_with(\"data:\"));").len(), 1, "in a macro");
    assert!(found("// url.starts_with(\"https://\")").is_empty());
    assert!(found("url::is_https(url);").is_empty());
}
