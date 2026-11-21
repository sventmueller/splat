//! What in a document comes from the files around it.

use super::doc::{Block, BlockKind, Doc, Inline, InlineKind};
use super::{markup, url};

/// Whether the document names files around it: a picture by its path, a wiki link or an embed. A sandboxed app reads them
/// only once the user granted their folder.
pub fn has_local_files(doc: &Doc) -> bool {
    fn local(address: &str) -> bool {
        !address.trim().is_empty() && url::scheme(address).is_none_or(|scheme| scheme == "file")
    }
    fn in_html(nodes: &[markup::Node]) -> bool {
        nodes.iter().any(|n| match n {
            markup::Node::Element(e) => {
                (e.role == markup::Role::Picture && e.attr("src").is_some_and(local)) || in_html(&e.children)
            }
            markup::Node::Text { .. } => false,
        })
    }
    fn names_one(i: &Inline) -> bool {
        let here = match &i.kind {
            InlineKind::Image { url, .. } | InlineKind::HtmlImage { url, .. } => local(url),
            InlineKind::WikiLink { .. } => true,
            _ => false,
        };
        here || i.children.iter().any(names_one)
    }
    fn holds_one(b: &Block) -> bool {
        let html = match &b.kind {
            BlockKind::HtmlBlock { literal } => literal.contains("<img") && in_html(&markup::parse(literal)),
            _ => false,
        };
        html || b.inlines().iter().any(names_one) || b.children().iter().any(holds_one)
    }
    doc.blocks.iter().any(holds_one)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::parse;

    #[test]
    fn pictures_by_path_wiki_links_and_embeds_name_files_around_the_document() {
        let local = |t: &str| has_local_files(&parse(t));
        assert!(local("![Logo](bilder/logo.png)\n"));
        assert!(local("![Logo](file:///Users/a/logo.png)\n"));
        assert!(local("Siehe [[Notiz]].\n"));
        assert!(local("![[Bild.png]]\n"));
        assert!(
            local("# T\n\n> - a\n>\n>   ![Logo](bilder/logo.png)\n"),
            "deep inside blocks"
        );
        assert!(local("*a **[[Notiz]]** b*\n"), "deep inside inlines");
        assert!(local("<p>\n<img src=\"logo.png\">\n</p>\n"));
        assert!(local("Text <img src=\"logo.png\" width=\"20\"> Text\n"));
        assert!(
            !local("![Logo](https://x.de/a.png) [Datei](notiz.md)\n"),
            "a link opens on a click only"
        );
        assert!(!local("![Punkt](data:image/png;base64,AAAA)\n"));
        assert!(!local("<p>\n<img src=\"https://x.de/a.png\">\n</p>\n"));
        assert!(!local(
            "<div>\n<img alt=\"x\"> <iframe src=\"seite.html\"></iframe>\n</div>\n"
        ));
        assert!(!local("# Nur Text\n"));
    }
}
