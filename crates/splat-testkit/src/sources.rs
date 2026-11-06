//! The workspace's Rust sources as syntax trees, for the tests that hold the code to rules no type can state: which module
//! may name which, what must run behind a guard against panics, which texts need a translation.

mod body;
mod names;
mod strip;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub use proc_macro2;
use proc_macro2::Span;
pub use syn;
use syn::visit_mut::VisitMut;

pub use body::{Body, body, visit_body};
pub use names::{Named, named, resolve};

/// The directory of the workspace.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .expect("the kit lies in crates/")
}

/// Every Rust file below `dir`, in a stable order.
pub fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// The file each crate in `crates/` starts from, so that a new crate is held to the rules without anyone listing it.
pub fn crate_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = std::fs::read_dir(root().join("crates"))
        .expect("the workspace has crates/")
        .flatten()
        .flat_map(|entry| ["lib.rs", "main.rs"].map(|file| entry.path().join("src").join(file)))
        .filter(|path| path.exists())
        .collect();
    roots.sort();
    roots
}

/// What a build leaves out of the sources.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Without {
    Nothing,
    /// What only `cargo test` compiles: items under `#[cfg(test)]` or `#[test]`.
    Tests,
    /// That and what only the self-test build compiles, under `feature = "selftest"`: what ships.
    TestsAndSelftest,
}

/// A source file with its syntax, from which `without` is gone.
pub struct Source {
    /// Relative to the workspace, as in `crates/splat-markdown/src/doc/mod.rs`.
    pub name: String,
    pub text: String,
    /// The module the file holds, from its crate's root: `["doc"]` for `doc/mod.rs`, none for `lib.rs`.
    pub module: Vec<String>,
    pub syntax: syn::File,
    pub without: Without,
}

impl Source {
    /// Fails with the place of the first error, as a source no rule can read would pass every rule.
    pub fn parse(name: &str, module: Vec<String>, text: String, without: Without) -> Source {
        let mut syntax = syn::parse_file(&text).unwrap_or_else(|error| {
            let at = error.span().start();
            panic!("{name}:{}:{}: {error}", at.line, at.column + 1)
        });
        strip::Strip(without).visit_file_mut(&mut syntax);
        Source {
            name: name.to_string(),
            text,
            module,
            syntax,
            without,
        }
    }

    /// The file below the workspace, read as a module of its own.
    pub fn read(path: &Path, without: Without) -> Source {
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        Source::parse(&relative(path), Vec::new(), text, without)
    }

    /// The text `span` covers.
    pub fn snippet(&self, span: Span) -> &str {
        let range = span.byte_range();
        self.text.get(range).unwrap_or("")
    }

    /// Where `span` starts, as `file:line`.
    pub fn at(&self, span: Span) -> String {
        format!("{}:{}", self.name, line(span))
    }

    /// What `mac` holds, as far as it reads as Rust, without what this source leaves out.
    pub fn body(&self, mac: &syn::Macro) -> Option<Body> {
        body(mac, self.without)
    }
}

/// The line `span` starts on, counted from 1.
pub fn line(span: Span) -> usize {
    span.start().line
}

fn relative(path: &Path) -> String {
    path.strip_prefix(root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// A crate's sources, found the way the compiler finds them: by the `mod` declarations from its root on, so that a file
/// only tests declare is not read as code that ships.
pub struct Crate {
    pub sources: Vec<Source>,
    /// Every module, also those written inline.
    pub modules: BTreeSet<Vec<String>>,
}

impl Crate {
    pub fn read(root_file: &Path, without: Without) -> Crate {
        let mut sources = Vec::new();
        let mut pending = vec![(root_file.to_path_buf(), Vec::new())];
        while let Some((path, module)) = pending.pop() {
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let source = Source::parse(&relative(&path), module, text, without);
            let file_dir = path.parent().expect("a file lies in a directory").to_path_buf();
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let module_dir = if ["mod", "lib", "main"].contains(&stem.as_str()) || source.module.is_empty() {
                file_dir.clone()
            } else {
                file_dir.join(&stem)
            };
            declared(
                &source.syntax.items,
                (&file_dir, &module_dir),
                &source.module,
                &mut pending,
            );
            sources.push(source);
        }
        sources.sort_by(|a, b| a.name.cmp(&b.name));
        Crate::of(sources)
    }

    /// A crate of sources that need not lie on disk, as the checks of the rules make them.
    pub fn of(sources: Vec<Source>) -> Crate {
        let mut modules = BTreeSet::new();
        for source in &sources {
            modules.insert(source.module.clone());
            inline_modules(&source.syntax.items, &source.module, &mut modules);
        }
        Crate { sources, modules }
    }
}

/// The out-of-line modules `items` declare, with their files: a `#[path]` lies beside the declaring file, any other in
/// its module's directory, as `name.rs` or `name/mod.rs`.
fn declared(
    items: &[syn::Item],
    (file_dir, module_dir): (&Path, &Path),
    module: &[String],
    found: &mut Vec<(PathBuf, Vec<String>)>,
) {
    for item in items {
        let syn::Item::Mod(declaration) = item else {
            continue;
        };
        let name = declaration.ident.to_string();
        let mut inner = module.to_vec();
        inner.push(name.clone());
        let below = module_dir.join(&name);
        match &declaration.content {
            Some((_, items)) => declared(items, (&below, &below), &inner, found),
            None => {
                let file = path_attribute(&declaration.attrs).map_or_else(
                    || {
                        let flat = module_dir.join(format!("{name}.rs"));
                        if flat.exists() { flat } else { below.join("mod.rs") }
                    },
                    |path| file_dir.join(path),
                );
                found.push((file, inner));
            }
        }
    }
}

fn path_attribute(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| match &attr.meta {
        syn::Meta::NameValue(pair) if pair.path.is_ident("path") => match &pair.value {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(path),
                ..
            }) => Some(path.value()),
            _ => None,
        },
        _ => None,
    })
}

fn inline_modules(items: &[syn::Item], module: &[String], found: &mut BTreeSet<Vec<String>>) {
    for item in items {
        if let syn::Item::Mod(syn::ItemMod {
            ident,
            content: Some((_, items)),
            ..
        }) = item
        {
            let mut inner = module.to_vec();
            inner.push(ident.to_string());
            inline_modules(items, &inner, found);
            found.insert(inner);
        }
    }
}

#[cfg(test)]
mod tests;
