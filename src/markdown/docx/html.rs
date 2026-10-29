//! Embedded HTML as Word paragraphs: headings, aligned paragraphs, pictures at the size asked for, lists, tables
//! with joined cells and the content of `<details>`, which Word shows unfolded.

use super::{Ctx, Props, Writer, asked_size, text_run};
use crate::markdown::markup::{self, Element, Node};

/// Paragraph properties the content of an element gets.
#[derive(Clone, Default)]
struct Para {
    style: Option<String>,
    align: Option<&'static str>,
    /// Numbering id and level of a list item.
    list: Option<(usize, usize)>,
    quote: bool,
}

impl Writer<'_> {
    pub(super) fn html(&mut self, literal: &str, ctx: &Ctx) {
        let nodes = markup::parse(literal);
        let mut runs = String::new();
        let para = Para {
            quote: ctx.quote > 0,
            ..Para::default()
        };
        self.html_nodes(&nodes, &para, Props::default(), &mut runs);
        self.html_flush(&para, &mut runs);
    }

    fn html_flush(&mut self, para: &Para, runs: &mut String) {
        if runs.is_empty() {
            return;
        }
        let mut ppr = String::new();
        if let Some(style) = &para.style {
            ppr.push_str(&format!("<w:pStyle w:val=\"{style}\"/>"));
        } else if para.quote {
            ppr.push_str("<w:pStyle w:val=\"Quote\"/>");
        }
        if let Some((id, level)) = para.list {
            ppr.push_str(&format!(
                "<w:numPr><w:ilvl w:val=\"{level}\"/><w:numId w:val=\"{id}\"/></w:numPr>"
            ));
        }
        if let Some(align) = para.align {
            let value = if align == "center" {
                "center"
            } else if align == "right" {
                "right"
            } else {
                "left"
            };
            ppr.push_str(&format!("<w:jc w:val=\"{value}\"/>"));
        }
        let content = std::mem::take(runs);
        self.raw_paragraph(&ppr, &content);
    }

    fn html_nodes(&mut self, nodes: &[Node], para: &Para, props: Props, runs: &mut String) {
        for node in nodes {
            match node {
                Node::Text { text, .. } => {
                    // As in the preview: only ASCII whitespace collapses, `&nbsp;` keeps its width.
                    let collapsed = text.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
                    let lead = text.starts_with(|c: char| c.is_ascii_whitespace()) && !runs.is_empty();
                    let trail = text.ends_with(|c: char| c.is_ascii_whitespace()) && !collapsed.is_empty();
                    let shown = format!(
                        "{}{collapsed}{}",
                        if lead { " " } else { "" },
                        if trail { " " } else { "" }
                    );
                    if !shown.trim_ascii().is_empty() || (!runs.is_empty() && !shown.is_empty()) {
                        runs.push_str(&text_run(&shown, props));
                    }
                }
                Node::Element(e) => self.html_element(e, para, props, runs),
            }
        }
    }

    fn html_element(&mut self, e: &Element, para: &Para, props: Props, runs: &mut String) {
        let inline = |w: &mut Self, p: Props, runs: &mut String| w.html_nodes(&e.children, para, p, runs);
        match e.name.as_str() {
            "br" => runs.push_str("<w:r><w:br/></w:r>"),
            "img" => {
                if let Some(src) = e.attr("src").filter(|s| markup::safe_url(s))
                    && let Some(picture) = (self.export.image)(src)
                {
                    let picture = asked_size(picture, e.pixels("width"), e.pixels("height"));
                    let run = self.picture(picture, e.attr("alt").unwrap_or(""));
                    runs.push_str(&run);
                }
            }
            "b" | "strong" => inline(self, Props { bold: true, ..props }, runs),
            "i" | "em" | "cite" | "dfn" | "var" => inline(self, Props { italic: true, ..props }, runs),
            "u" | "ins" => inline(
                self,
                Props {
                    underline: true,
                    ..props
                },
                runs,
            ),
            "s" | "del" | "strike" => inline(self, Props { strike: true, ..props }, runs),
            "mark" => inline(
                self,
                Props {
                    highlight: true,
                    ..props
                },
                runs,
            ),
            "sup" => inline(self, Props { sup: true, ..props }, runs),
            "sub" => inline(self, Props { sub: true, ..props }, runs),
            "code" | "kbd" | "samp" | "tt" => inline(self, Props { code: true, ..props }, runs),
            "a" => {
                let mut inner = String::new();
                self.html_nodes(&e.children, para, props, &mut inner);
                match e.attr("href").filter(|h| {
                    let lower = h.to_ascii_lowercase();
                    ["http://", "https://", "mailto:"].iter().any(|s| lower.starts_with(s))
                }) {
                    Some(href) => {
                        let link = self.hyperlink(href, &inner);
                        runs.push_str(&link);
                    }
                    None => runs.push_str(&inner),
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.html_flush(para, runs);
                let heading = Para {
                    style: Some(format!("Heading{}", &e.name[1..])),
                    align: e.align().or(para.align),
                    ..para.clone()
                };
                self.html_nodes(&e.children, &heading, props, runs);
                self.html_flush(&heading, runs);
            }
            "p" | "div" | "center" | "section" | "article" | "header" | "footer" | "main" | "nav" | "aside"
            | "figure" | "figcaption" | "address" | "summary" | "dt" | "dd" => {
                self.html_flush(para, runs);
                let block = Para {
                    align: e.align().or((e.name == "center").then_some("center")).or(para.align),
                    ..para.clone()
                };
                let props = Props {
                    bold: props.bold || matches!(e.name.as_str(), "summary" | "dt"),
                    ..props
                };
                self.html_nodes(&e.children, &block, props, runs);
                self.html_flush(&block, runs);
            }
            "blockquote" => {
                self.html_flush(para, runs);
                let quote = Para {
                    quote: true,
                    ..para.clone()
                };
                self.html_nodes(&e.children, &quote, props, runs);
                self.html_flush(&quote, runs);
            }
            "ul" | "ol" => {
                self.html_flush(para, runs);
                let id = if e.name == "ol" {
                    self.numbers
                        .push(e.attr("start").and_then(|s| s.parse().ok()).unwrap_or(1));
                    self.numbers.len() + 1
                } else {
                    1
                };
                let level = para.list.map_or(0, |(_, l)| l + 1);
                for item in e.children.iter().filter_map(element).filter(|i| i.name == "li") {
                    let item_para = Para {
                        list: Some((id, level)),
                        ..para.clone()
                    };
                    self.html_nodes(&item.children, &item_para, props, runs);
                    self.html_flush(&item_para, runs);
                }
            }
            "pre" => {
                self.html_flush(para, runs);
                let mut code = String::new();
                raw_text(&e.children, &mut code);
                self.code(code.trim_matches('\n'));
            }
            "hr" => {
                self.html_flush(para, runs);
                self.raw_paragraph(
                    "<w:pBdr><w:bottom w:val=\"single\" w:sz=\"6\" w:space=\"1\" w:color=\"D2D2D7\"/></w:pBdr>",
                    "",
                );
            }
            "table" => {
                self.html_flush(para, runs);
                self.html_table(e, props);
            }
            _ => self.html_nodes(&e.children, para, props, runs),
        }
    }

    fn html_table(&mut self, e: &Element, props: Props) {
        let mut rows: Vec<(&Element, bool)> = Vec::new();
        for child in e.children.iter().filter_map(element) {
            match child.name.as_str() {
                "tr" => rows.push((child, false)),
                "thead" | "tbody" | "tfoot" => rows.extend(
                    child
                        .children
                        .iter()
                        .filter_map(element)
                        .filter(|r| r.name == "tr")
                        .map(|r| (r, child.name == "thead")),
                ),
                _ => {}
            }
        }
        if rows.is_empty() {
            return;
        }
        if std::mem::take(&mut self.gap) {
            self.raw_paragraph(
                &format!(
                    "<w:spacing w:after=\"0\" w:line=\"{}\" w:lineRule=\"exact\"/>",
                    super::GAP
                ),
                "",
            );
        }
        let columns = rows
            .iter()
            .map(|(r, _)| {
                r.children
                    .iter()
                    .filter_map(element)
                    .filter(|c| c.name == "td" || c.name == "th")
                    .map(|c| c.pixels("colspan").unwrap_or(1).clamp(1, 64))
                    .sum::<u32>()
            })
            .max()
            .unwrap_or(1);
        self.body.push_str(
            "<w:tbl><w:tblPr><w:tblStyle w:val=\"TableGrid\"/><w:tblW w:w=\"0\" w:type=\"auto\"/></w:tblPr><w:tblGrid>",
        );
        for _ in 0..columns {
            self.body.push_str("<w:gridCol/>");
        }
        self.body.push_str("</w:tblGrid>");
        for (row, in_head) in rows {
            self.body.push_str("<w:tr>");
            for cell in row
                .children
                .iter()
                .filter_map(element)
                .filter(|c| c.name == "td" || c.name == "th")
            {
                let header = in_head || cell.name == "th";
                let span = cell.pixels("colspan").unwrap_or(1).clamp(1, 64);
                self.body.push_str("<w:tc><w:tcPr><w:tcW w:w=\"0\" w:type=\"auto\"/>");
                if span > 1 {
                    self.body.push_str(&format!("<w:gridSpan w:val=\"{span}\"/>"));
                }
                if header {
                    self.body
                        .push_str("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F2F2F4\"/>");
                }
                self.body.push_str("</w:tcPr>");
                let mut runs = String::new();
                let cell_para = Para {
                    align: cell.align(),
                    ..Para::default()
                };
                self.html_nodes(&cell.children, &cell_para, Props { bold: header, ..props }, &mut runs);
                let jc = match cell.align() {
                    Some("center") => "<w:jc w:val=\"center\"/>",
                    Some("right") => "<w:jc w:val=\"right\"/>",
                    _ => "",
                };
                self.raw_paragraph(&format!("<w:spacing w:after=\"0\"/>{jc}"), &runs);
                self.body.push_str("</w:tc>");
            }
            self.body.push_str("</w:tr>");
        }
        self.body.push_str("</w:tbl>");
        self.raw_paragraph("", "");
    }
}

fn element(node: &Node) -> Option<&Element> {
    match node {
        Node::Element(e) => Some(e),
        Node::Text { .. } => None,
    }
}

fn raw_text(nodes: &[Node], out: &mut String) {
    for n in nodes {
        match n {
            Node::Text { text, .. } => out.push_str(text),
            Node::Element(e) if e.name == "br" => out.push('\n'),
            Node::Element(e) => raw_text(&e.children, out),
        }
    }
}
