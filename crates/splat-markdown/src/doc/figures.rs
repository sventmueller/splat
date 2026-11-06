//! Formulas and diagrams in a document, which the preview shows as rendered images.

use std::ops::Range;

use super::{BlockKind, Doc, InlineKind, walk_blocks, walk_inlines};
use crate::text::Byte;

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
    /// A TeX formula, inline or on its own, from math syntax or a ` ```math ` block.
    Math {
        /// The TeX without its delimiters or fences.
        tex: &'a str,
        /// Whether it is set apart from the line rather than standing inside it.
        display: bool,
    },
    /// A diagram in a code block whose info string names its language, such as ` ```mermaid `.
    Diagram {
        /// The text of the code block, in the diagram's language.
        source: &'a str,
        /// The language the info string names.
        kind: DiagramKind,
    },
}

/// The diagram languages a code block can name for the preview to draw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagramKind {
    /// A ` ```mermaid ` block.
    Mermaid,
    /// A ` ```dot ` or ` ```graphviz ` block.
    Graphviz,
    /// A ` ```plantuml ` or ` ```puml ` block.
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

/// Definitions a formula may make for the formulas behind it.
const DEFINING: [&str; 8] = [
    "\\require",
    "\\newcommand",
    "\\renewcommand",
    "\\newenvironment",
    "\\renewenvironment",
    "\\def",
    "\\gdef",
    "\\let",
];

/// More would be read again for every formula of a document built to take the renderer down.
const MOST_MACROS: usize = 16 << 10;

/// The formulas of a document that define something for the others, in the order they stand in.
pub fn macros_of<'a>(figures: impl Iterator<Item = &'a Figure<'a>>) -> String {
    let mut macros = String::new();
    for figure in figures {
        if let Figure::Math { tex, .. } = figure
            && DEFINING.iter().any(|command| tex.contains(command))
            && macros.len() + tex.len() < MOST_MACROS
        {
            macros.push_str(tex);
            macros.push('\n');
        }
    }
    macros
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
    use crate::reader::parse;
    use crate::text::bytes;

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
    fn only_formulas_that_define_something_count_as_macros() {
        let d = parse("$\\newcommand{\\R}{x}$ und $a$\n\n$$\\def\\b{c}$$\n\n```mermaid\n\\def\n```\n");
        let all: Vec<Figure> = figures(&d).into_iter().map(|(_, f)| f).collect();
        assert_eq!(macros_of(all.iter()), "\\newcommand{\\R}{x}\n\\def\\b{c}\n");
        let none = parse("$a$ $b$\n");
        let plain: Vec<Figure> = figures(&none).into_iter().map(|(_, f)| f).collect();
        assert_eq!(macros_of(plain.iter()), "");
        let loading = parse("$\\require{physics}$ und $\\dv{f}{x}$\n");
        let loads: Vec<Figure> = figures(&loading).into_iter().map(|(_, f)| f).collect();
        assert_eq!(macros_of(loads.iter()), "\\require{physics}\n");
    }

    #[test]
    fn macros_stop_growing_where_they_would_slow_down_every_formula() {
        let tex = format!("\\def\\a{{{}}}", "x".repeat(4 << 10));
        let many: Vec<Figure> = (0..100)
            .map(|_| Figure::Math {
                tex: &tex,
                display: false,
            })
            .collect();
        let macros = macros_of(many.iter());
        assert!(!macros.is_empty() && macros.len() <= MOST_MACROS, "{}", macros.len());
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
