//! The paths a source names, and the module of its crate each one reaches.

use std::collections::BTreeSet;

use proc_macro2::{Span, TokenStream, TokenTree};
use syn::visit::{self, Visit};

use super::{Source, body, visit_body};

/// A path written in a source: `module` is where it stands, which a `super` starts from.
#[derive(Debug)]
pub struct Named {
    pub module: Vec<String>,
    /// As written; `"::"` stands for a leading `::`.
    pub path: Vec<String>,
    pub line: usize,
}

/// Every path `source` names: in `use`, in code, in types, and in macros, also those that read as no Rust.
pub fn named(source: &Source) -> Vec<Named> {
    let mut names = Names {
        source,
        module: source.module.clone(),
        found: Vec::new(),
    };
    names.visit_file(&source.syntax);
    names.found
}

/// The module path `path` reaches when written in `module`: through `crate`, `super` or `self`, or through a module
/// declared in `module`; `None` for anything outside the crate.
pub fn resolve(module: &[String], path: &[String], modules: &BTreeSet<Vec<String>>) -> Option<Vec<String>> {
    let (mut at, rest) = match path.first()?.as_str() {
        "crate" => (Vec::new(), &path[1..]),
        "self" => (module.to_vec(), &path[1..]),
        "super" => {
            let mut at = module.to_vec();
            let mut rest = path;
            while rest.first().is_some_and(|s| s == "super") {
                at.pop()?;
                rest = &rest[1..];
            }
            (at, rest)
        }
        first => {
            let mut child = module.to_vec();
            child.push(first.to_string());
            if !modules.contains(&child) {
                return None;
            }
            (module.to_vec(), path)
        }
    };
    at.extend(rest.iter().filter(|s| *s != "self").cloned());
    Some(at)
}

struct Names<'s> {
    source: &'s Source,
    module: Vec<String>,
    found: Vec<Named>,
}

impl Names<'_> {
    fn note(&mut self, path: Vec<String>, span: Span) {
        self.found.push(Named {
            module: self.module.clone(),
            path,
            line: super::line(span),
        });
    }

    fn use_tree(&mut self, tree: &syn::UseTree, mut prefix: Vec<String>) {
        match tree {
            syn::UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                self.use_tree(&path.tree, prefix);
            }
            syn::UseTree::Name(name) => {
                prefix.push(name.ident.to_string());
                self.note(prefix, name.ident.span());
            }
            syn::UseTree::Rename(rename) => {
                prefix.push(rename.ident.to_string());
                self.note(prefix, rename.ident.span());
            }
            syn::UseTree::Glob(glob) => self.note(prefix, glob.star_token.span),
            syn::UseTree::Group(group) => {
                for tree in &group.items {
                    self.use_tree(tree, prefix.clone());
                }
            }
        }
    }

    /// Paths as tokens show them, `a::b::c`, where a macro holds no Rust a parser reads.
    fn in_tokens(&mut self, tokens: &TokenStream) {
        let trees: Vec<TokenTree> = tokens.clone().into_iter().collect();
        let mut at = 0;
        while at < trees.len() {
            match &trees[at] {
                TokenTree::Group(group) => self.in_tokens(&group.stream()),
                TokenTree::Ident(first) => {
                    let mut path = vec![first.to_string()];
                    let mut next = at + 1;
                    while let (Some(TokenTree::Punct(a)), Some(TokenTree::Punct(b)), Some(TokenTree::Ident(ident))) =
                        (trees.get(next), trees.get(next + 1), trees.get(next + 2))
                    {
                        if a.as_char() != ':' || b.as_char() != ':' {
                            break;
                        }
                        path.push(ident.to_string());
                        next += 3;
                    }
                    if path.len() > 1 {
                        self.note(path, first.span());
                        at = next - 1;
                    }
                }
                TokenTree::Punct(_) | TokenTree::Literal(_) => {}
            }
            at += 1;
        }
    }
}

impl<'ast> Visit<'ast> for Names<'_> {
    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        self.module.push(module.ident.to_string());
        visit::visit_item_mod(self, module);
        self.module.pop();
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        let prefix = if item.leading_colon.is_some() {
            vec!["::".to_string()]
        } else {
            Vec::new()
        };
        self.use_tree(&item.tree, prefix);
    }

    fn visit_item_extern_crate(&mut self, item: &'ast syn::ItemExternCrate) {
        self.note(vec![item.ident.to_string()], item.ident.span());
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        let mut segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        if path.leading_colon.is_some() {
            segments.insert(0, "::".to_string());
        }
        let span = path.segments.first().map_or_else(Span::call_site, |s| s.ident.span());
        self.note(segments, span);
        visit::visit_path(self, path);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.visit_path(&mac.path);
        match body(mac, self.source.without) {
            Some(body) => visit_body(self, &body),
            None => self.in_tokens(&mac.tokens),
        }
    }
}
