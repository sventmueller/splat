//! What a macro call holds, read as Rust where it is: a parser sees only tokens there, and the code in `define_class!`,
//! `msg_send!` or `format!` runs as any other.

use proc_macro2::{Delimiter, Group, TokenStream, TokenTree};
use syn::ext::IdentExt;
use syn::parse::{ParseStream, Parser};
use syn::visit::Visit;
use syn::visit_mut::VisitMut;
use syn::{Expr, Ident, Macro, MacroDelimiter, Token};

use super::Without;
use super::strip::Strip;

/// The Rust in a macro call.
pub enum Body {
    /// As in `define_class!` or `thread_local!`.
    Items(syn::File),
    /// As in `format!` or `vec!`.
    Exprs(Vec<Expr>),
    /// `msg_send!`: the receiver unless it is the superclass, which runs none of ours, and the arguments.
    Message {
        receiver: Option<Expr>,
        arguments: Vec<Expr>,
    },
}

/// What `mac` holds, as far as it reads as items, as expressions or as a message; `None` for a `macro_rules!`
/// definition and for what reads as none of them.
pub fn body(mac: &Macro, without: Without) -> Option<Body> {
    let name = mac.path.segments.last()?.ident.to_string();
    match name.as_str() {
        "macro_rules" => return None,
        "sel" => return Some(Body::Exprs(Vec::new())),
        "matches" | "assert_matches" => return matching.parse2(mac.tokens.clone()).ok(),
        _ if name.starts_with("msg_send") => return message.parse2(mac.tokens.clone()).ok(),
        _ => {}
    }
    let mut strip = Strip(without);
    let items = syn::parse2::<syn::File>(mac.tokens.clone())
        .or_else(|_| syn::parse2::<syn::File>(with_attributes_named_only(mac.tokens.clone())));
    if let Ok(mut file) = items.or_else(|_| handed_on(&mac.tokens)) {
        strip.visit_file_mut(&mut file);
        return Some(Body::Items(file));
    }
    let delimiter = match mac.delimiter {
        MacroDelimiter::Paren(_) => Delimiter::Parenthesis,
        MacroDelimiter::Brace(_) => Delimiter::Brace,
        MacroDelimiter::Bracket(_) => Delimiter::Bracket,
    };
    let group = TokenStream::from(TokenTree::Group(Group::new(delimiter, mac.tokens.clone())));
    let mut expr: Expr = syn::parse2(group).ok()?;
    strip.visit_expr_mut(&mut expr);
    Some(Body::Exprs(match expr {
        Expr::Tuple(tuple) => tuple.elems.into_iter().collect(),
        Expr::Array(array) => array.elems.into_iter().collect(),
        Expr::Paren(inner) => vec![*inner.expr],
        other => vec![other],
    }))
}

/// The items of `then, [items]`, as a table macro hands them on to the macro `then`, which wraps them in `define_class!`.
fn handed_on(tokens: &TokenStream) -> syn::Result<syn::File> {
    let trees: Vec<TokenTree> = tokens.clone().into_iter().collect();
    match trees.as_slice() {
        [TokenTree::Ident(_), TokenTree::Punct(comma), TokenTree::Group(items)]
            if comma.as_char() == ',' && items.delimiter() == Delimiter::Bracket =>
        {
            syn::parse2::<syn::File>(with_attributes_named_only(items.stream()))
        }
        _ => Err(syn::Error::new(proc_macro2::Span::call_site(), "no items handed on")),
    }
}

/// `tokens` with `#[name = value]` as `#[name]` where it stands in front of an item: `define_class!` takes a type as the
/// value, as in `#[ivars = Cell<u8>]`, and a parser takes only an expression there.
fn with_attributes_named_only(tokens: TokenStream) -> TokenStream {
    let mut trees: Vec<TokenTree> = tokens.into_iter().collect();
    for at in 1..trees.len() {
        let TokenTree::Group(group) = &trees[at] else {
            continue;
        };
        let pound = matches!(&trees[at - 1], TokenTree::Punct(p) if p.as_char() == '#');
        let inside: Vec<TokenTree> = group.stream().into_iter().collect();
        if let (true, Delimiter::Bracket, [TokenTree::Ident(name), TokenTree::Punct(eq), ..]) =
            (pound, group.delimiter(), inside.as_slice())
            && eq.as_char() == '='
        {
            let mut named = Group::new(Delimiter::Bracket, TokenTree::Ident(name.clone()).into());
            named.set_span(group.span());
            trees[at] = TokenTree::Group(named);
        }
    }
    trees.into_iter().collect()
}

/// `matches!(expr, pattern if guard)`, of which the pattern runs nothing.
fn matching(input: ParseStream) -> syn::Result<Body> {
    let mut exprs = vec![input.parse::<Expr>()?];
    input.parse::<Token![,]>()?;
    syn::Pat::parse_multi_with_leading_vert(input)?;
    if input.parse::<Option<Token![if]>>()?.is_some() {
        exprs.push(input.parse::<Expr>()?);
    }
    input.parse::<Option<Token![,]>>()?;
    input.parse::<TokenStream>()?;
    Ok(Body::Exprs(exprs))
}

/// `msg_send![receiver, a: x, b: y]` or `msg_send![super(self), a]`.
fn message(input: ParseStream) -> syn::Result<Body> {
    let receiver = if input.peek(Token![super]) {
        input.parse::<Token![super]>()?;
        let class;
        syn::parenthesized!(class in input);
        class.parse::<TokenStream>()?;
        None
    } else {
        Some(input.parse::<Expr>()?)
    };
    let mut arguments = Vec::new();
    while !input.is_empty() {
        input.parse::<Token![,]>()?;
        while !input.is_empty() && !input.peek(Token![,]) {
            Ident::parse_any(input)?;
            if input.peek(Token![:]) {
                input.parse::<Token![:]>()?;
                arguments.push(input.parse::<Expr>()?);
            }
        }
    }
    Ok(Body::Message { receiver, arguments })
}

/// Visits what `body` holds.
pub fn visit_body<V>(visitor: &mut V, body: &Body)
where
    V: for<'a> Visit<'a>,
{
    match body {
        Body::Items(file) => visitor.visit_file(file),
        Body::Exprs(exprs) => exprs.iter().for_each(|expr| visitor.visit_expr(expr)),
        Body::Message { receiver, arguments } => {
            receiver
                .iter()
                .chain(arguments)
                .for_each(|expr| visitor.visit_expr(expr));
        }
    }
}
