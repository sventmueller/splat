//! Removing what a build leaves out, by the attributes the compiler reads, at every place an attribute can stand.

use syn::punctuated::Punctuated;
use syn::visit_mut::{self, VisitMut};
use syn::{Attribute, Expr, ImplItem, Item, Meta, Stmt, Token, TraitItem};

use super::Without;

pub(super) struct Strip(pub(super) Without);

impl Strip {
    pub(super) fn keeps(&self, attrs: &[Attribute]) -> bool {
        !attrs.iter().any(|attr| self.leaves_out(attr))
    }

    fn leaves_out(&self, attr: &Attribute) -> bool {
        if self.0 == Without::Nothing {
            return false;
        }
        if attr.path().is_ident("test") {
            return true;
        }
        attr.path().is_ident("cfg")
            && attr
                .parse_args::<Meta>()
                .is_ok_and(|condition| self.holds(&condition) == Some(false))
    }

    /// Whether `condition` holds in the build, if that follows from what the build leaves out.
    fn holds(&self, condition: &Meta) -> Option<bool> {
        match condition {
            Meta::Path(path) if path.is_ident("test") => Some(false),
            Meta::NameValue(pair) if pair.path.is_ident("feature") => {
                let selftest = matches!(&pair.value, Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(name), .. }) if name.value() == "selftest");
                (selftest && self.0 == Without::TestsAndSelftest).then_some(false)
            }
            Meta::List(list) => {
                let inner = list
                    .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                    .ok()?;
                let values: Vec<Option<bool>> = inner.iter().map(|c| self.holds(c)).collect();
                if list.path.is_ident("not") {
                    values.first().copied().flatten().map(|value| !value)
                } else if list.path.is_ident("all") {
                    if values.contains(&Some(false)) {
                        Some(false)
                    } else {
                        values.iter().all(|v| *v == Some(true)).then_some(true)
                    }
                } else if list.path.is_ident("any") {
                    if values.contains(&Some(true)) {
                        Some(true)
                    } else {
                        values.iter().all(|v| *v == Some(false)).then_some(false)
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

pub(super) fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(i) => &i.attrs,
        Item::Enum(i) => &i.attrs,
        Item::ExternCrate(i) => &i.attrs,
        Item::Fn(i) => &i.attrs,
        Item::ForeignMod(i) => &i.attrs,
        Item::Impl(i) => &i.attrs,
        Item::Macro(i) => &i.attrs,
        Item::Mod(i) => &i.attrs,
        Item::Static(i) => &i.attrs,
        Item::Struct(i) => &i.attrs,
        Item::Trait(i) => &i.attrs,
        Item::TraitAlias(i) => &i.attrs,
        Item::Type(i) => &i.attrs,
        Item::Union(i) => &i.attrs,
        Item::Use(i) => &i.attrs,
        _ => &[],
    }
}

fn impl_item_attrs(item: &ImplItem) -> &[Attribute] {
    match item {
        ImplItem::Const(i) => &i.attrs,
        ImplItem::Fn(i) => &i.attrs,
        ImplItem::Type(i) => &i.attrs,
        ImplItem::Macro(i) => &i.attrs,
        _ => &[],
    }
}

fn trait_item_attrs(item: &TraitItem) -> &[Attribute] {
    match item {
        TraitItem::Const(i) => &i.attrs,
        TraitItem::Fn(i) => &i.attrs,
        TraitItem::Type(i) => &i.attrs,
        TraitItem::Macro(i) => &i.attrs,
        _ => &[],
    }
}

/// The attributes of an expression that stands as a statement, for the kinds that do.
fn expr_attrs(expr: &Expr) -> &[Attribute] {
    match expr {
        Expr::Assign(e) => &e.attrs,
        Expr::Block(e) => &e.attrs,
        Expr::Call(e) => &e.attrs,
        Expr::ForLoop(e) => &e.attrs,
        Expr::If(e) => &e.attrs,
        Expr::Loop(e) => &e.attrs,
        Expr::Macro(e) => &e.attrs,
        Expr::Match(e) => &e.attrs,
        Expr::MethodCall(e) => &e.attrs,
        Expr::Path(e) => &e.attrs,
        Expr::Return(e) => &e.attrs,
        Expr::Unsafe(e) => &e.attrs,
        Expr::While(e) => &e.attrs,
        _ => &[],
    }
}

fn stmt_attrs(stmt: &Stmt) -> &[Attribute] {
    match stmt {
        Stmt::Local(local) => &local.attrs,
        Stmt::Item(item) => item_attrs(item),
        Stmt::Expr(expr, _) => expr_attrs(expr),
        Stmt::Macro(mac) => &mac.attrs,
    }
}

impl VisitMut for Strip {
    fn visit_file_mut(&mut self, file: &mut syn::File) {
        file.items.retain(|item| self.keeps(item_attrs(item)));
        visit_mut::visit_file_mut(self, file);
    }

    fn visit_item_mod_mut(&mut self, module: &mut syn::ItemMod) {
        if let Some((_, items)) = &mut module.content {
            items.retain(|item| self.keeps(item_attrs(item)));
        }
        visit_mut::visit_item_mod_mut(self, module);
    }

    fn visit_item_impl_mut(&mut self, block: &mut syn::ItemImpl) {
        block.items.retain(|item| self.keeps(impl_item_attrs(item)));
        visit_mut::visit_item_impl_mut(self, block);
    }

    fn visit_item_trait_mut(&mut self, block: &mut syn::ItemTrait) {
        block.items.retain(|item| self.keeps(trait_item_attrs(item)));
        visit_mut::visit_item_trait_mut(self, block);
    }

    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        block.stmts.retain(|stmt| self.keeps(stmt_attrs(stmt)));
        visit_mut::visit_block_mut(self, block);
    }

    fn visit_expr_match_mut(&mut self, expr: &mut syn::ExprMatch) {
        expr.arms.retain(|arm| self.keeps(&arm.attrs));
        visit_mut::visit_expr_match_mut(self, expr);
    }

    fn visit_fields_named_mut(&mut self, fields: &mut syn::FieldsNamed) {
        let kept = std::mem::take(&mut fields.named)
            .into_pairs()
            .filter(|pair| self.keeps(&pair.value().attrs));
        fields.named = kept.collect();
        visit_mut::visit_fields_named_mut(self, fields);
    }

    fn visit_item_enum_mut(&mut self, item: &mut syn::ItemEnum) {
        let kept = std::mem::take(&mut item.variants)
            .into_pairs()
            .filter(|pair| self.keeps(&pair.value().attrs));
        item.variants = kept.collect();
        visit_mut::visit_item_enum_mut(self, item);
    }
}
