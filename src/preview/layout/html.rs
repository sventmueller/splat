//! Embedded HTML laid out as preview paragraphs. Its text is not edited character by character: every piece is an
//! atom pointing to where it stands in the source, so a click lands there and the source stays the place to edit.

use std::ops::Range;

use super::blocks::marker_text;
use super::inlines::{align, atom, atom_styled};
use super::{
    ATTACHMENT, Align, Atom, Builder, CharMap, Ctx, ImageAtom, LINE_SEPARATOR, Link, Marker, ParaKind, Run, RunMap,
    RunStyle,
};
use crate::markdown::markup::{self, Element, Node, find, raw_text};
use crate::markdown::style::{TextStyle, styled_element};
use crate::markdown::text::{Byte, count32};
use crate::markdown::url::is_harmless;

/// Offsets in the HTML literal, which inside a quote lacks the `>` prefixes, as source offsets.
struct Map<'t> {
    literal: &'t str,
    /// `None` if the source does not spell the literal: every piece then points to the whole block.
    chars: Option<CharMap>,
    block: Range<Byte>,
    /// Where each char of `literal` starts; a block with thousands of elements asks once per tag.
    char_starts: std::cell::OnceCell<Vec<usize>>,
}

impl Map<'_> {
    /// `range` of the literal as a range of the source.
    fn source(&self, range: &Range<usize>) -> Range<Byte> {
        let Some(chars) = &self.chars else {
            return self.block.clone();
        };
        let at = |byte: usize| match chars {
            CharMap::Verbatim(start) if byte < self.literal.len() => *start + byte,
            CharMap::Verbatim(_) => self.block.end,
            CharMap::Offsets(base, offsets) => {
                let starts = self
                    .char_starts
                    .get_or_init(|| self.literal.char_indices().map(|(b, _)| b).collect());
                offsets
                    .get(starts.partition_point(|&b| b < byte))
                    .map_or(self.block.end, |&o| *base + o as usize)
            }
        };
        let start = at(range.start);
        start..at(range.end).max(start)
    }
}

#[derive(Clone)]
struct Around {
    ctx: Ctx,
    kind: ParaKind,
    style: RunStyle,
}

struct HtmlWriter<'b, 'a, 't> {
    b: &'b mut Builder<'a>,
    map: Map<'t>,
    block: Range<Byte>,
    runs: Vec<Run>,
    /// The list or footnote marker the next paragraph starts with.
    marker: Option<(Marker, Range<Byte>)>,
    in_cell: bool,
    /// Where the current line of a cell starts in `runs`.
    line_start: usize,
}

/// Empty cells an HTML table may ask for beyond its real ones.
const MAX_FILLED_CELLS: usize = 4096;

impl Builder<'_> {
    pub(super) fn html_block(
        &mut self,
        literal: &str,
        range: Range<Byte>,
        ctx: &Ctx,
        marker: Option<(Marker, Range<Byte>)>,
    ) {
        let chars = align(literal, self.text, range.clone());
        let nodes = markup::parse(literal);
        let mut w = HtmlWriter {
            b: self,
            map: Map {
                literal,
                chars,
                block: range.clone(),
                char_starts: Default::default(),
            },
            block: range,
            runs: Vec::new(),
            marker,
            in_cell: false,
            line_start: 0,
        };
        let cx = Around {
            ctx: Ctx {
                html: true,
                ..ctx.clone()
            },
            kind: ParaKind::Body,
            style: RunStyle::default(),
        };
        w.nodes(&nodes, &cx);
        w.flush(&cx);
    }
}

impl HtmlWriter<'_, '_, '_> {
    fn flush(&mut self, cx: &Around) {
        if let Some(last) = self.runs.last_mut()
            && matches!(last.map, RunMap::Atom { atom: Atom::Text, .. })
        {
            let trimmed = last.text.trim_end_matches(' ').len();
            last.text.truncate(trimmed);
        }
        // Only what came since the last line of a cell: a cell of many lines would otherwise take quadratic time.
        let tail = self.runs.split_off(self.line_start.min(self.runs.len()));
        self.runs.extend(tail.into_iter().filter(|r| !r.text.is_empty()));
        if self.runs.is_empty() {
            return;
        }
        if self.in_cell {
            self.end_line_in_cell();
            return;
        }
        let runs = std::mem::take(&mut self.runs);
        let marker = self.marker.take();
        let src = self.block.clone();
        self.b
            .push_para(cx.kind.clone(), &cx.ctx, marker, src.clone(), runs, src.end);
    }

    /// A table cell is one paragraph, so the paragraphs inside it become its lines.
    fn end_line_in_cell(&mut self) {
        let is_end = |r: &Run| r.text.chars().eq([LINE_SEPARATOR]);
        if self.runs.last().is_none_or(is_end) {
            return;
        }
        if let Some((marker, src)) = self.marker.take() {
            let shown = format!("{} ", marker_text(&marker).trim_end());
            self.runs.insert(self.line_start, atom(Atom::Text, &shown, src));
        }
        let end = match self.runs.last().map(|r| &r.map) {
            Some(RunMap::Atom { src, .. }) => src.end,
            _ => self.block.end,
        };
        self.runs.push(atom(Atom::Text, &LINE_SEPARATOR.to_string(), end..end));
        self.line_start = self.runs.len();
    }

    fn text(&mut self, text: &str, range: &Range<usize>, style: &RunStyle) {
        let mut collapsed = String::with_capacity(text.len());
        let mut space = self.runs.is_empty()
            || self
                .runs
                .last()
                .is_some_and(|r| r.text.ends_with([' ', LINE_SEPARATOR]));
        for c in text.chars() {
            // HTML collapses only ASCII whitespace; `&nbsp;` and the other Unicode spaces keep their width.
            if c.is_ascii_whitespace() {
                if !space {
                    collapsed.push(' ');
                    space = true;
                }
            } else {
                collapsed.push(c);
                space = false;
            }
        }
        if !collapsed.is_empty() {
            self.runs
                .push(atom_styled(Atom::Text, &collapsed, self.map.source(range), style));
        }
    }

    fn nodes(&mut self, nodes: &[Node], cx: &Around) {
        for node in nodes {
            match node {
                Node::Text { text, range } => self.text(text, range, &cx.style),
                Node::Element(e) => self.element(e, cx),
            }
        }
    }

    fn element(&mut self, e: &Element, cx: &Around) {
        let src = self.map.source(&e.tag);
        if let Some(kind) = styled_element(&e.name) {
            let mut c = cx.clone();
            c.style.text = c.style.text.inside(&kind);
            self.nodes(&e.children, &c);
            return;
        }
        match e.name.as_str() {
            "br" => {
                if let Some(last) = self.runs.last_mut()
                    && matches!(last.map, RunMap::Atom { atom: Atom::Text, .. })
                {
                    let kept = last.text.trim_end_matches(' ').len();
                    last.text.truncate(kept);
                }
                self.runs
                    .push(atom_styled(Atom::Text, &LINE_SEPARATOR.to_string(), src, &cx.style));
            }
            "img" => self.image(e, src, &cx.style),
            "picture" => {
                if let Some(img) = find(&e.children, "img") {
                    let src = self.map.source(&img.tag);
                    self.image(img, src, &cx.style);
                }
            }
            "a" => {
                let mut c = cx.clone();
                if let Some(href) = e.attr("href").filter(|h| is_harmless(h)) {
                    c.style.link = Some(Link::Address(href.into()));
                }
                self.nodes(&e.children, &c);
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = e.name[1..].parse().unwrap_or(1);
                self.block(e, cx, ParaKind::Heading(level));
            }
            name if markup::makes_paragraphs(name) => self.block(e, cx, ParaKind::Body),
            "hr" if self.in_cell => self.flush(cx),
            "hr" => {
                self.flush(cx);
                let run = atom(Atom::Rule, "\u{00A0}", src);
                self.runs.push(run);
                let rule = Around {
                    kind: ParaKind::Rule,
                    ..cx.clone()
                };
                self.flush(&rule);
            }
            "pre" => {
                self.flush(cx);
                let mut code = String::new();
                raw_text(&e.children, &mut code);
                let mut code = code.trim_matches('\n').to_string();
                if self.in_cell {
                    code = code.replace('\n', &LINE_SEPARATOR.to_string());
                }
                if !code.is_empty() {
                    let mut c = cx.clone();
                    c.kind = ParaKind::Code { info: "".into() };
                    c.ctx.group = self.b.next_group();
                    c.style.text.code = true;
                    self.runs
                        .push(atom_styled(Atom::Text, &code, self.map.source(&e.range), &c.style));
                    self.flush(&c);
                }
            }
            "blockquote" => {
                self.flush(cx);
                let mut c = cx.clone();
                c.ctx.quote_depth += 1;
                c.ctx.group = self.b.next_group();
                self.nodes(&e.children, &c);
                self.flush(&c);
            }
            "ul" | "ol" => self.list(e, cx),
            "dl" => {
                self.flush(cx);
                for item in e.children.iter().filter_map(Node::element) {
                    let mut c = cx.clone();
                    if item.name == "dd" {
                        c.ctx.list_depth += 1;
                    } else {
                        c.style.text.bold = true;
                    }
                    self.nodes(&item.children, &c);
                    self.flush(&c);
                }
            }
            "table" => self.table(e, cx),
            "details" => self.details(e, cx),
            _ => self.nodes(&e.children, cx),
        }
    }

    /// A block element: what came before ends its paragraph, and it gets paragraphs of its own.
    fn block(&mut self, e: &Element, cx: &Around, kind: ParaKind) {
        self.flush(cx);
        let mut c = cx.clone();
        c.kind = kind;
        if let Some(a) = e.align().or((e.name == "center").then_some(Align::Center)) {
            c.ctx.align = a;
        }
        self.nodes(&e.children, &c);
        self.flush(&c);
    }

    fn image(&mut self, e: &Element, src: Range<Byte>, style: &RunStyle) {
        let alt = e.attr("alt").unwrap_or("");
        match e.attr("src").filter(|u| is_harmless(u)) {
            Some(url) => self.runs.push(atom_styled(
                Atom::Image(Box::new(ImageAtom {
                    url: url.into(),
                    alt: alt.into(),
                    width: e.pixels("width"),
                    height: e.pixels("height"),
                })),
                &ATTACHMENT.to_string(),
                src,
                style,
            )),
            None if !alt.is_empty() => self.runs.push(atom_styled(Atom::Text, alt, src, style)),
            None => {}
        }
    }

    fn list(&mut self, e: &Element, cx: &Around) {
        self.flush(cx);
        let start: usize = e.attr("start").and_then(|s| s.parse().ok()).unwrap_or(1);
        let depth = cx.ctx.list_depth;
        for (n, item) in e
            .children
            .iter()
            .filter_map(Node::element)
            .filter(|i| i.name == "li")
            .enumerate()
        {
            let mut c = cx.clone();
            c.ctx.list_depth = depth + 1;
            c.kind = ParaKind::Body;
            let marker = if e.name == "ol" {
                Marker::Number(start + n)
            } else {
                Marker::Bullet(depth)
            };
            self.marker = Some((marker, self.map.source(&item.tag)));
            self.nodes(&item.children, &c);
            self.flush(&c);
            self.marker = None;
        }
    }

    fn table(&mut self, e: &Element, cx: &Around) {
        self.flush(cx);
        let rows = e.rows();
        if self.in_cell {
            for (row, _) in &rows {
                for cell in row.cells() {
                    self.nodes(&cell.children, cx);
                    let src = self.map.source(&cell.range);
                    self.runs.push(atom_styled(Atom::Text, " ", src, &cx.style));
                }
                self.flush(cx);
            }
            return;
        }
        let columns = rows
            .iter()
            .map(|(r, _)| r.cells().map(Element::span).sum::<u32>())
            .max()
            .unwrap_or(0);
        if columns == 0 {
            return;
        }
        // Short rows are filled up so that every cell has its background, unless that is what makes a table huge: `<td>`
        // a few thousand times, then as many `<tr>`, would ask for gigabytes.
        let cells: usize = rows.iter().map(|(r, _)| r.cells().count()).sum();
        let fills_rows = rows.len().saturating_mul(columns as usize) <= 2 * cells + MAX_FILLED_CELLS;
        self.b.table += 1;
        let table = count32(self.b.table);
        let mut inner = cx.ctx.clone();
        inner.group = self.b.next_group();
        let start = self.b.paras.len();
        for (row, (r, in_head)) in rows.iter().enumerate() {
            let mut col = 0;
            for cell in r.cells() {
                let header = *in_head || cell.name == "th";
                let width = cell.span().min(columns - col);
                let c = Around {
                    ctx: inner.clone(),
                    kind: ParaKind::TableCell {
                        table,
                        row: count32(row),
                        col,
                        header,
                        align: cell.align().unwrap_or_default(),
                        columns,
                        span: width,
                    },
                    style: RunStyle {
                        text: TextStyle {
                            bold: header,
                            ..cx.style.text
                        },
                        ..cx.style.clone()
                    },
                };
                self.cell(cell, &c);
                col += width;
                if col >= columns {
                    break;
                }
            }
            while fills_rows && col < columns {
                let c = Around {
                    ctx: inner.clone(),
                    kind: ParaKind::TableCell {
                        table,
                        row: count32(row),
                        col,
                        header: *in_head,
                        align: Align::None,
                        columns,
                        span: 1,
                    },
                    style: cx.style.clone(),
                };
                let at = self.map.source(&r.range).end;
                self.b.push_para(c.kind.clone(), &c.ctx, None, at..at, vec![], at);
                col += 1;
            }
        }
        self.b.set_direction(start, inner.group);
    }

    fn cell(&mut self, cell: &Element, cx: &Around) {
        self.in_cell = true;
        self.nodes(&cell.children, cx);
        self.flush(cx);
        self.in_cell = false;
        self.line_start = 0;
        let src = self.map.source(&cell.range);
        let mut runs = std::mem::take(&mut self.runs);
        if runs.last().is_some_and(|r| r.text.chars().eq([LINE_SEPARATOR])) {
            runs.pop();
        }
        for r in &mut runs {
            r.style.text.bold |= cx.style.text.bold;
        }
        self.b
            .push_para(cx.kind.clone(), &cx.ctx, None, src.clone(), runs, src.end);
    }

    fn details(&mut self, e: &Element, cx: &Around) {
        self.flush(cx);
        let open = e.attr("open").is_some();
        let chevron = if open { "▾ " } else { "▸ " };
        let tag = self.map.source(&e.tag);
        // Without the tag's place in the source there is nothing a click could change.
        let run = if self.map.chars.is_some() {
            let toggle = RunStyle {
                link: Some(Link::Fold),
                ..cx.style.clone()
            };
            atom_styled(Atom::Details { open }, chevron, tag, &toggle)
        } else {
            atom_styled(Atom::Text, chevron, tag, &cx.style)
        };
        self.runs.push(run);
        let summary = Around {
            style: RunStyle {
                text: TextStyle {
                    bold: true,
                    ..cx.style.text
                },
                ..cx.style.clone()
            },
            ..cx.clone()
        };
        match find(&e.children, "summary") {
            Some(s) => self.nodes(&s.children, &summary),
            None => {
                let label = crate::i18n::t("Details");
                let src = self.map.source(&e.tag);
                self.runs.push(atom_styled(Atom::Text, label, src, &summary.style));
            }
        }
        self.flush(cx);
        if open {
            let body: Vec<Node> = e
                .children
                .iter()
                .filter(|n| !matches!(n, Node::Element(s) if s.name == "summary"))
                .cloned()
                .collect();
            self.nodes(&body, cx);
            self.flush(cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{laid, para_with, paras, texts};
    use super::super::{NoImages, Para, layout};
    use super::*;
    use crate::markdown::reader::parse;
    use crate::markdown::text::{TextAt, bytes};

    #[test]
    fn source_offsets_are_the_same_with_and_without_a_table_of_chars() {
        let literal = "<p>ä</p><b>ö</b>";
        let starts: Vec<u32> = literal.char_indices().map(|(b, _)| count32(b)).collect();
        let verbatim = Map {
            literal,
            chars: Some(CharMap::Verbatim(Byte::new(100))),
            block: bytes(100..200),
            char_starts: Default::default(),
        };
        let table = Map {
            literal,
            chars: Some(CharMap::Offsets(Byte::new(100), starts.into())),
            block: bytes(100..200),
            char_starts: Default::default(),
        };
        for range in [0..3, 3..5, 9..12, 12..14, 14..literal.len(), 0..literal.len()] {
            assert_eq!(verbatim.source(&range), table.source(&range), "{range:?}");
        }
        assert_eq!(verbatim.source(&(3..literal.len())), bytes(103..200));
    }

    #[test]
    fn embedded_html_renders_and_points_into_its_source() {
        let t = "<p align=\"center\">\n  <img src=\"logo.png\" width=\"120\">\n  <br><b>Splat</b> &amp; <a href=\"https://x.de\">Web</a>\n</p>\n\n\
                 <details>\n<summary>Mehr</summary>\n\nVersteckt\n\n</details>\n\n<details open>\n<summary>Offen</summary>\n\nSichtbar\n\n</details>\n\n\
                 <table><tr><th colspan=2>Kopf</th></tr><tr><td>a</td><td>b</td></tr></table>\n\n\
                 Text mit <kbd>⌘</kbd> und <img src=\"x.png\" height=\"10\"> <!-- weg --> <b>fett</b>\n";
        let d = parse(t);
        let l = layout(&d, &NoImages, None);
        let o = ATTACHMENT;
        let shown = texts(&l);
        assert_eq!(
            shown,
            vec![
                format!("{o}\u{2028}Splat & Web"),
                "▸ Mehr".to_string(),
                "▾ Offen".into(),
                "Sichtbar".into(),
                "Kopf".into(),
                "a".into(),
                "b".into(),
                format!("Text mit ⌘ und {o}  fett"),
            ]
        );
        let paras: Vec<&Para> = l.blocks.iter().flat_map(|b| &b.paras).collect();
        assert_eq!(paras[0].style.align, Align::Center);
        assert!(paras[0].style.html);
        let RunMap::Atom {
            atom: Atom::Image(img),
            src,
        } = &paras[0].runs[0].map
        else {
            panic!()
        };
        assert_eq!(
            (img.width, t.at(src.clone())),
            (Some(120), "<img src=\"logo.png\" width=\"120\">")
        );
        let web = paras[0].runs.iter().find(|r| r.text == "Web").unwrap();
        assert_eq!(web.style.link, Some(Link::Address("https://x.de".into())));
        let RunMap::Atom {
            atom: Atom::Details { open: false },
            src,
        } = &paras[1].runs[0].map
        else {
            panic!()
        };
        assert_eq!(t.at(src.clone()), "<details>");
        assert!(matches!(
            paras[4].style.kind,
            ParaKind::TableCell {
                span: 2,
                header: true,
                ..
            }
        ));
        let last = paras[7];
        assert!(last.runs.iter().any(|r| r.text == "⌘" && r.style.text.code));
        assert!(
            last.runs
                .iter()
                .any(|r| matches!(&r.map, RunMap::Atom { atom: Atom::Image(i), .. } if i.height == Some(10)))
        );
        assert!(last.runs.iter().any(|r| r.text == "fett" && r.style.text.bold));
    }

    #[test]
    fn embedded_html_collapses_only_ascii_whitespace() {
        let t = "<p><img src=\"a.png\">&nbsp;&nbsp;&nbsp;<img src=\"b.png\"></p>\n\n<p>a  \n\t b&#8195;c</p>\n";
        let d = parse(t);
        let l = layout(&d, &NoImages, None);
        let o = ATTACHMENT;
        assert_eq!(
            texts(&l),
            vec![format!("{o}\u{a0}\u{a0}\u{a0}{o}"), "a b\u{2003}c".to_string()]
        );
    }

    #[test]
    fn html_blocks_become_headings_rules_code_quotes_lists_and_tables() {
        let t = "<div>\n<h3>Titel</h3>\n<hr>\n<pre>\nlet x = 1;\n</pre>\n<blockquote>Zitat</blockquote>\n\
                 <ol start=\"3\"><li>drei</li><li>vier</li></ol>\n<ul><li>Punkt</li></ul>\n\
                 <dl><dt>Begriff</dt><dd>Erklärung</dd></dl>\n\
                 <picture><source srcset=\"a.webp\"><img src=\"a.png\" alt=\"Bild\"></picture>\n\
                 <img src=\"javascript:x\" alt=\"Ersatz\">\n\
                 <table><thead><tr><th>K</th><th>L</th></tr></thead><tbody><tr><td>1</td></tr></tbody></table>\n\
                 </div>\n";
        let d = parse(t);
        let l = layout(&d, &NoImages, None);
        let paras: Vec<&Para> = l.blocks.iter().flat_map(|b| &b.paras).collect();
        let text = |p: &Para| p.runs.iter().map(|r| r.text.as_str()).collect::<String>();
        let find = |s: &str| {
            *paras
                .iter()
                .find(|p| text(p).contains(s))
                .unwrap_or_else(|| panic!("{s}"))
        };
        assert_eq!(find("Titel").style.kind, ParaKind::Heading(3));
        assert!(paras.iter().any(|p| p.style.kind == ParaKind::Rule));
        assert!(matches!(find("let x = 1;").style.kind, ParaKind::Code { .. }));
        assert_eq!(find("Zitat").style.quote_depth, 1);
        assert_eq!(find("drei").style.marker, Some(Marker::Number(3)));
        assert_eq!(find("vier").style.marker, Some(Marker::Number(4)));
        assert_eq!(find("Punkt").style.marker, Some(Marker::Bullet(0)));
        assert!(find("Begriff").runs.iter().all(|r| r.style.text.bold));
        assert_eq!(find("Erklärung").style.list_depth, 1);
        assert!(find("Ersatz").runs.iter().any(|r| r.text == "Ersatz"));
        let images: Vec<String> = paras
            .iter()
            .flat_map(|p| &p.runs)
            .filter_map(|r| match &r.map {
                RunMap::Atom {
                    atom: Atom::Image(i), ..
                } => Some(i.url.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(images, ["a.png"]);
        let cells: Vec<(&str, bool)> = paras
            .iter()
            .filter_map(|p| match p.style.kind {
                ParaKind::TableCell { header, .. } => Some((p.runs.first().map_or("", |r| r.text.as_str()), header)),
                _ => None,
            })
            .collect();
        assert_eq!(cells, [("K", true), ("L", true), ("1", false), ("", false)]);
    }

    #[test]
    fn a_picture_element_shows_its_image_and_a_refused_one_its_text() {
        let image = |t: &str| {
            let l = laid(t);
            paras(&l).iter().any(|p| {
                p.runs.iter().any(|r| {
                    matches!(
                        r.map,
                        RunMap::Atom {
                            atom: Atom::Image(_),
                            ..
                        }
                    )
                })
            })
        };
        assert!(image(
            "<picture><source srcset=\"b.png\"><img src=\"a.png\"></picture>\n"
        ));
        let l = laid("<p><img src=\"javascript:x\" alt=\"A\"></p>\n");
        assert_eq!(texts(&l), ["A"]);
        assert!(
            texts(&laid("<p><img src=\"javascript:x\"> b</p>\n"))
                .iter()
                .all(|t| t.trim() == "b")
        );
        let empty = laid("<p><img src=\"javascript:x\">b</p>\n");
        assert!(paras(&empty)[0].runs.iter().all(|r| !r.text.is_empty()));
    }

    #[test]
    fn only_a_center_element_centers_itself() {
        assert_eq!(para_with(&laid("<center>a</center>\n"), "a").style.align, Align::Center);
        assert_eq!(para_with(&laid("<p>a</p>\n"), "a").style.align, Align::None);
    }

    #[test]
    fn a_list_in_html_goes_deeper_with_its_nesting() {
        let l = laid("<ul><li>a<ul><li>b</li></ul></li></ul>\n");
        assert_eq!(para_with(&l, "a").style.list_depth, 1);
        let b = para_with(&l, "b");
        assert_eq!(b.style.list_depth, 2);
        assert_eq!(b.style.marker, Some(Marker::Bullet(1)));
    }

    #[test]
    fn tables_in_html_count_up_and_keep_bold_of_their_head_and_their_text() {
        let l =
            laid("<table><tr><th>a</th><td>b</td><td><b>c</b></td></tr></table><table><tr><td>d</td></tr></table>\n");
        assert!(para_with(&l, "a").runs[0].style.text.bold);
        assert!(!para_with(&l, "b").runs[0].style.text.bold);
        assert!(para_with(&l, "c").runs[0].style.text.bold);
        let table = |text: &str| match para_with(&l, text).style.kind {
            ParaKind::TableCell { table, .. } => table,
            _ => panic!("no cell"),
        };
        assert_eq!((table("a"), table("d")), (1, 2));
    }

    #[test]
    fn details_in_html_fold_by_a_click_and_their_summary_is_bold() {
        let t = "<details><summary>a</summary>b</details>\n";
        let l = laid(t);
        let p = paras(&l)[0];
        assert_eq!(p.runs[0].style.link, Some(Link::Fold));
        let summary = p.runs.iter().find(|r| r.text == "a").unwrap();
        assert!(summary.style.text.bold && summary.style.link.is_none());
    }

    #[test]
    fn a_refused_picture_without_a_text_leaves_nothing_behind() {
        assert!(
            paras(&laid("<p><img src=\"javascript:x\"></p>\n"))
                .iter()
                .all(|p| p.runs.is_empty())
        );
    }

    #[test]
    fn blocks_in_a_table_cell_of_html_become_its_lines() {
        let cell = |content: &str| {
            let t = format!("<table><tr><td>{content}</td><td>z</td></tr></table>\n\n<p>behind</p>\n");
            let l = laid(&t);
            let shown = texts(&l);
            assert_eq!(
                shown.len(),
                3,
                "the cell, its neighbour and the paragraph behind: {shown:?}"
            );
            assert_eq!(shown[1], "z");
            assert_eq!(
                para_with(&l, "behind").style.kind,
                ParaKind::Body,
                "it ends with the table"
            );
            assert!(matches!(paras(&l)[0].style.kind, ParaKind::TableCell { .. }));
            shown[0].clone()
        };
        let line = LINE_SEPARATOR;
        assert_eq!(cell("a"), "a");
        assert_eq!(cell("<p>a</p>"), "a");
        assert_eq!(cell("<p>a</p><p>b</p>"), format!("a{line}b"));
        assert_eq!(cell("a<p>b</p>c"), format!("a{line}b{line}c"));
        assert_eq!(cell("<div><p>a</p></div>"), "a");
        assert_eq!(cell("<ul><li>a</li><li>b</li></ul>"), format!("• a{line}• b"));
        assert_eq!(cell("x<ol start=\"3\"><li>a</li></ol>"), format!("x{line}3. a"));
        assert_eq!(cell("<hr>a"), "a");
        assert_eq!(cell("<pre>x\ny</pre>"), format!("x{line}y"));
        assert_eq!(cell("<table><tr><td>a</td><td>b</td></tr></table>"), "a b");
        assert_eq!(cell("<h2>a</h2>b"), format!("a{line}b"));
    }
}
