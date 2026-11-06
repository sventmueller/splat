//! Embedded HTML as Word paragraphs: headings, aligned paragraphs, pictures at the size asked for, lists, tables
//! with joined cells and the content of `<details>`, which Word shows unfolded.

use super::xml::{Line, ParaProps, ParaStyle, Props, Runs};
use super::{WordCtx, Writer, asked_size, cell_paragraph, cell_start, word_follows};
use crate::doc::Align;
use crate::markup::{self, AddressFor, Destination, Element, Node, Role, destination, raw_text};

/// The highest number a list in Word can start with.
const MAX_START: usize = i32::MAX as usize;

/// Paragraph properties the content of an element gets.
#[derive(Clone, Default)]
struct HtmlPara {
    style: Option<ParaStyle>,
    align: Option<Align>,
    /// Numbering id and level of a list item.
    list: Option<(usize, usize)>,
    quote: bool,
}

impl Writer<'_> {
    pub(super) fn html(&mut self, literal: &str, ctx: &WordCtx) {
        let nodes = markup::parse(literal);
        let mut runs = Runs::default();
        let para = HtmlPara {
            quote: ctx.quote > 0,
            ..HtmlPara::default()
        };
        self.html_nodes(&nodes, &para, Props::default(), &mut runs);
        self.html_flush(&para, &mut runs);
    }

    fn html_flush(&mut self, para: &HtmlPara, runs: &mut Runs) {
        if runs.is_empty() {
            return;
        }
        let props = ParaProps {
            style: para.style.or(para.quote.then_some(ParaStyle::Quote)),
            list: para.list,
            justify: para.align,
            ..ParaProps::default()
        };
        self.push_paragraph(props, std::mem::take(runs));
    }

    fn html_nodes(&mut self, nodes: &[Node], para: &HtmlPara, props: Props, runs: &mut Runs) {
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
                        runs.push(&shown, props);
                    }
                }
                Node::Element(e) => self.html_element(e, para, props, runs),
            }
        }
    }

    fn html_element(&mut self, e: &Element, para: &HtmlPara, props: Props, runs: &mut Runs) {
        match e.role {
            Role::Look(look) => self.html_nodes(&e.children, para, props.nested_in(&look.inline_kind()), runs),
            Role::LineBreak => runs.line_break(),
            Role::Picture => self.html_picture(e, props, runs),
            Role::Link => match e.attr("href").filter(|h| word_follows(h)) {
                Some(href) => {
                    let mut inner = Runs::default();
                    self.html_nodes(&e.children, para, props.linked(), &mut inner);
                    runs.hyperlink(self.link_id(href), inner);
                }
                None => self.html_nodes(&e.children, para, props, runs),
            },
            Role::Heading(level) => {
                self.html_flush(para, runs);
                let heading = HtmlPara {
                    style: Some(ParaStyle::Heading(level)),
                    align: e.align().or(para.align),
                    ..para.clone()
                };
                self.html_nodes(&e.children, &heading, props, runs);
                self.html_flush(&heading, runs);
            }
            Role::Paragraph
            | Role::Division
            | Role::Center
            | Role::Section
            | Role::Summary
            | Role::Term
            | Role::Definition => {
                self.html_flush(para, runs);
                let block = HtmlPara {
                    align: e
                        .align()
                        .or((e.role == Role::Center).then_some(Align::Center))
                        .or(para.align),
                    ..para.clone()
                };
                let props = props.bold(props.text.bold || matches!(e.role, Role::Summary | Role::Term));
                self.html_nodes(&e.children, &block, props, runs);
                self.html_flush(&block, runs);
            }
            Role::Quote => {
                self.html_flush(para, runs);
                let quote = HtmlPara {
                    quote: true,
                    ..para.clone()
                };
                self.html_nodes(&e.children, &quote, props, runs);
                self.html_flush(&quote, runs);
            }
            Role::List { ordered } => {
                self.html_flush(para, runs);
                self.html_list(e, ordered, para, props, runs);
            }
            Role::Preformatted => {
                self.html_flush(para, runs);
                let mut code = String::new();
                raw_text(&e.children, &mut code);
                self.code(code.trim_matches('\n'));
            }
            Role::Rule => {
                self.html_flush(para, runs);
                self.draw_rule(Line::Bottom);
            }
            Role::Table => {
                self.html_flush(para, runs);
                self.html_table(e, props);
            }
            // Word shows `<details>` unfolded, a definition list as paragraphs with its terms in bold, and of a
            // `<picture>` the `<img>` among its sources.
            Role::Details | Role::DefinitionList | Role::PictureSources => {
                self.html_nodes(&e.children, para, props, runs);
            }
            // What changes nothing in Word, and parts of a list or table outside one, give way to what they hold.
            Role::Presentational
            | Role::ListItem
            | Role::TableSection { .. }
            | Role::TableRow
            | Role::TableCell { .. }
            | Role::Unknown => self.html_nodes(&e.children, para, props, runs),
            Role::Empty | Role::Input | Role::Dropped => {}
        }
    }

    fn html_picture(&mut self, e: &Element, props: Props, runs: &mut Runs) {
        let alt = e.attr("alt").unwrap_or("");
        let src = e.attr("src").filter(|src| match destination(src, AddressFor::Picture) {
            Destination::Web | Destination::Relative => true,
            // The pictures of a Word file come from files and the web.
            Destination::PictureData | Destination::Mail | Destination::Refused => false,
        });
        match src.and_then(self.export.image) {
            Some(picture) => {
                let picture = asked_size(picture, e.pixels("width"), e.pixels("height"));
                runs.picture(&self.picture(picture, alt));
            }
            None => runs.push(alt, props),
        }
    }

    fn html_list(&mut self, e: &Element, ordered: bool, para: &HtmlPara, props: Props, runs: &mut Runs) {
        let id = if ordered {
            self.numbers
                .push(e.attr("start").and_then(|s| s.parse().ok()).unwrap_or(1).min(MAX_START));
            self.numbers.len() + 1
        } else {
            1
        };
        let level = para.list.map_or(0, |(_, l)| l + 1);
        for item in e
            .children
            .iter()
            .filter_map(Node::element)
            .filter(|i| i.role == Role::ListItem)
        {
            let item_para = HtmlPara {
                list: Some((id, level)),
                ..para.clone()
            };
            self.html_nodes(&item.children, &item_para, props, runs);
            self.html_flush(&item_para, runs);
        }
    }

    fn html_table(&mut self, e: &Element, props: Props) {
        let rows = e.rows();
        if rows.is_empty() {
            return;
        }
        self.table_gap();
        let columns = rows
            .iter()
            .map(|(r, _)| r.cells().map(Element::span).sum::<u32>())
            .max()
            .unwrap_or(1);
        self.raw(
            "<w:tbl><w:tblPr><w:tblStyle w:val=\"TableGrid\"/><w:tblW w:w=\"0\" w:type=\"auto\"/></w:tblPr><w:tblGrid>",
        );
        self.raw(&"<w:gridCol/>".repeat(columns as usize));
        self.raw("</w:tblGrid>");
        for (row, in_head) in rows {
            self.raw("<w:tr>");
            for cell in row.cells() {
                let header = in_head || cell.role == (Role::TableCell { header: true });
                self.raw(&cell_start(cell.span(), header));
                let mut runs = Runs::default();
                let cell_para = HtmlPara {
                    align: cell.align(),
                    ..HtmlPara::default()
                };
                self.html_nodes(&cell.children, &cell_para, props.bold(header), &mut runs);
                let justify = cell.align().filter(|a| *a != Align::Left);
                self.push_paragraph(cell_paragraph(justify), runs);
                self.raw("</w:tc>");
            }
            self.raw("</w:tr>");
        }
        self.raw("</w:tbl>");
        self.push_paragraph(ParaProps::default(), Runs::default());
    }
}
