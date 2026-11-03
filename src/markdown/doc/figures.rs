use std::ops::Range;

use super::{BlockKind, Doc, InlineKind, walk_blocks, walk_inlines};
use crate::markdown::text::Byte;
use crate::markdown::{markup, url};

/// The formula or diagram a code block with this info string holds.
pub fn code_figure<'a>(info: &str, literal: &'a str) -> Option<Figure<'a>> {
    match diagram_kind(info) {
        Some(kind) => Some(Figure::Diagram { source: literal, kind }),
        None if is_math_fence(info) => Some(Figure::Math {
            tex: literal.strip_suffix('\n').unwrap_or(literal),
            display: true,
        }),
        None => None,
    }
}

/// File names Obsidian embeds as pictures.
pub fn is_image_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        ".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".svg", ".heic", ".tif", ".tiff", ".avif",
    ]
    .iter()
    .any(|e| lower.ends_with(e))
}

/// A formula or diagram, which the preview shows as a rendered image.
pub enum Figure<'a> {
    Math { tex: &'a str, display: bool },
    Diagram { source: &'a str, kind: DiagramKind },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagramKind {
    Mermaid,
    Graphviz,
    PlantUml,
}

/// The diagram language a code block's info string names.
pub fn diagram_kind(info: &str) -> Option<DiagramKind> {
    match info.split_whitespace().next()? {
        "mermaid" => Some(DiagramKind::Mermaid),
        "dot" | "graphviz" => Some(DiagramKind::Graphviz),
        "plantuml" | "puml" => Some(DiagramKind::PlantUml),
        _ => None,
    }
}

/// A ` ```math ` block, as GitHub and GitLab write display formulas.
pub fn is_math_fence(info: &str) -> bool {
    info.split_whitespace().next() == Some("math")
}

/// Lengths of the opening and closing delimiter of a formula's source: `$`, `$$`, `` $` ``, `\(` or `\[`.
pub fn math_delimiters(src: &str) -> (usize, usize) {
    for (open, close) in [("$$", "$$"), ("$`", "`$"), ("\\(", "\\)"), ("\\[", "\\]"), ("$", "$")] {
        if src.len() >= open.len() + close.len() && src.starts_with(open) && src.ends_with(close) {
            return (open.len(), close.len());
        }
    }
    (0, 0)
}

/// Every formula and diagram with its source range, outer ones before those inside them.
pub fn figures(doc: &Doc) -> Vec<(Range<Byte>, Figure<'_>)> {
    let mut out = Vec::new();
    walk_blocks(&doc.blocks, &mut |b| {
        match &b.kind {
            BlockKind::MathBlock { literal } => out.push((
                b.range.clone(),
                Figure::Math {
                    tex: literal,
                    display: true,
                },
            )),
            BlockKind::CodeBlock { info, literal, .. } => {
                out.extend(code_figure(info, literal).map(|figure| (b.range.clone(), figure)));
            }
            _ => {}
        }
        walk_inlines(b.inlines(), &mut |i| {
            if let InlineKind::Math { display, literal } = &i.kind {
                out.push((
                    i.range.clone(),
                    Figure::Math {
                        tex: literal,
                        display: *display,
                    },
                ));
            }
        });
    });
    out
}

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

/// The innermost formula or diagram whose source contains `at`, ends included.
pub fn figure_at(doc: &Doc, at: Byte) -> Option<(Range<Byte>, Figure<'_>)> {
    figures(doc)
        .into_iter()
        .rev()
        .find(|(r, _)| r.start <= at && at <= r.end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::reader::parse;
    use crate::markdown::text::bytes;

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
    fn every_diagram_language_is_known() {
        for (info, kind) in [
            ("mermaid", DiagramKind::Mermaid),
            ("dot", DiagramKind::Graphviz),
            ("graphviz title", DiagramKind::Graphviz),
            ("plantuml", DiagramKind::PlantUml),
            ("puml", DiagramKind::PlantUml),
        ] {
            assert_eq!(diagram_kind(info), Some(kind), "{info}");
        }
        assert_eq!(diagram_kind("rust"), None);
        assert_eq!(diagram_kind(""), None);
    }

    #[test]
    fn the_delimiters_of_a_formula_must_stand_on_both_sides() {
        assert_eq!(math_delimiters("$a$"), (1, 1));
        assert_eq!(math_delimiters("$$a$$"), (2, 2));
        assert_eq!(math_delimiters("$`a`$"), (2, 2));
        assert_eq!(math_delimiters("\\(a\\)"), (2, 2));
        assert_eq!(math_delimiters("\\[a\\]"), (2, 2));
        assert_eq!(math_delimiters("$"), (0, 0), "one mark does not close itself");
        assert_eq!(math_delimiters("$a"), (0, 0));
        assert_eq!(math_delimiters("a$$"), (0, 0));
    }

    #[test]
    fn the_figure_at_a_position_includes_its_ends() {
        let t = "$$\na\n$$\n\ny $b$ z\n";
        let d = parse(t);
        let block_end = d.blocks[0].range.end;
        let inline = t.find("$b$").unwrap();
        assert_eq!(figure_at(&d, Byte::new(0)).unwrap().0, d.blocks[0].range);
        assert_eq!(figure_at(&d, block_end).unwrap().0, d.blocks[0].range);
        assert!(figure_at(&d, block_end + 1).is_none());
        assert!(figure_at(&d, Byte::new(inline - 1)).is_none());
        assert_eq!(figure_at(&d, Byte::new(inline)).unwrap().0, bytes(inline..inline + 3));
        assert_eq!(
            figure_at(&d, Byte::new(inline + 3)).unwrap().0,
            bytes(inline..inline + 3)
        );
        assert!(figure_at(&d, Byte::new(inline + 4)).is_none());
    }
}
