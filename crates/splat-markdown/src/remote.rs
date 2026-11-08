//! What in a document comes from the web, and what from the files around it.

use super::doc::{BlockKind, Doc, InlineKind, walk_blocks, walk_inlines};
use super::{markup, url};

/// Whether the document shows images from the web, which the preview loads only once allowed.
pub fn has_remote_images(doc: &Doc) -> bool {
    fn in_html(nodes: &[markup::Node]) -> bool {
        nodes.iter().any(|n| match n {
            markup::Node::Element(e) => {
                (e.role == markup::Role::Picture && e.attr("src").is_some_and(url::is_https)) || in_html(&e.children)
            }
            markup::Node::Text { .. } => false,
        })
    }
    let mut found = false;
    walk_blocks(&doc.blocks, &mut |b| {
        if let BlockKind::HtmlBlock { literal } = &b.kind {
            found |= literal.contains("://") && in_html(&markup::parse(literal));
        }
        walk_inlines(b.inlines(), &mut |i| {
            found |= matches!(
                &i.kind,
                InlineKind::Image { url, .. } | InlineKind::HtmlImage { url, .. } if url::is_https(url)
            );
        });
    });
    found
}

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
    let mut found = false;
    walk_blocks(&doc.blocks, &mut |b| {
        if let BlockKind::HtmlBlock { literal } = &b.kind {
            found |= literal.contains("<img") && in_html(&markup::parse(literal));
        }
        walk_inlines(b.inlines(), &mut |i| {
            found |= match &i.kind {
                InlineKind::Image { url, .. } | InlineKind::HtmlImage { url, .. } => local(url),
                InlineKind::WikiLink { .. } => true,
                _ => false,
            };
        });
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::parse;

    #[test]
    fn a_picture_on_the_web_is_one_whatever_the_case_of_its_address() {
        assert!(has_remote_images(&parse("![x](HTTPS://a.de/b.png)\n")));
        assert!(has_remote_images(&parse("<img src=\"HTTPS://a.de/b.png\">\n")));
        assert!(
            !has_remote_images(&parse("![x](http://a.de/b.png)\n")),
            "nothing Splat would load"
        );
    }

    #[test]
    fn web_images_in_markdown_and_html_need_permission() {
        let remote = |t: &str| has_remote_images(&parse(t));
        assert!(remote("![Logo](https://x.de/a.png)\n"));
        assert!(remote(
            "<p align=\"center\">\n  <a href=\"https://x.de\"><img src=\"https://x.de/b.svg\"></a>\n</p>\n"
        ));
        assert!(remote("Text <img src=\"https://x.de/c.png\" width=\"20\"> Text\n"));
        assert!(!remote(
            "<p align=\"center\">\n  <a href=\"https://x.de\"><img src=\"logo.png\"></a>\n</p>\n"
        ));
        assert!(!remote("![Logo](logo.png) [Web](https://x.de)\n"));
    }

    #[test]
    fn pictures_by_path_wiki_links_and_embeds_name_files_around_the_document() {
        let local = |t: &str| has_local_files(&parse(t));
        assert!(local("![Logo](bilder/logo.png)\n"));
        assert!(local("![Logo](file:///Users/a/logo.png)\n"));
        assert!(local("Siehe [[Notiz]].\n"));
        assert!(local("![[Bild.png]]\n"));
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
