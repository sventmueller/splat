//! What in a document comes from the web.

use super::doc::{BlockKind, Doc, InlineKind, walk_blocks, walk_inlines};
use super::{markup, url};

/// Whether the document shows images from the web, which the preview loads only once allowed.
pub fn has_remote_images(doc: &Doc) -> bool {
    fn in_html(nodes: &[markup::Node]) -> bool {
        nodes.iter().any(|n| match n {
            markup::Node::Element(e) => {
                (e.name == "img" && e.attr("src").is_some_and(url::is_https)) || in_html(&e.children)
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
}
