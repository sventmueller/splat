//! Embedded HTML laid out as preview paragraphs. Its text is not edited character by character: every piece is an
//! atom pointing to where it stands in the source, so a click lands there and the source stays the place to edit.

use std::ops::Range;

use super::{
    ATTACHMENT, AlignKey, Atom, Builder, CharMap, Ctx, ImageAtom, LINE_SEPARATOR, Marker, ParaKind, Run, RunMap,
    RunStyle, align, align_key, atom, atom_styled,
};
use crate::markdown::markup::{self, Element, Node};

/// Offsets in the HTML literal, which inside a quote lacks the `>` prefixes, as source offsets.
struct Map<'t> {
    literal: &'t str,
    chars: CharMap,
    end: usize,
}

impl Map<'_> {
    fn source(&self, range: &Range<usize>) -> Range<usize> {
        let at = |byte: usize| {
            let k = self.literal[..byte.min(self.literal.len())].chars().count();
            self.chars.get(self.literal, k).unwrap_or(self.end)
        };
        let start = at(range.start);
        start..at(range.end).max(start)
    }
}

#[derive(Clone)]
struct Cx {
    ctx: Ctx,
    kind: ParaKind,
    style: RunStyle,
}

struct Writer<'b, 'a, 't> {
    b: &'b mut Builder<'a>,
    map: Map<'t>,
    block: Range<usize>,
    runs: Vec<Run>,
    /// The list or footnote marker the next paragraph starts with.
    marker: Option<(Marker, Range<usize>)>,
}

impl Builder<'_> {
    pub(super) fn html_block(
        &mut self,
        literal: &str,
        range: Range<usize>,
        ctx: &Ctx,
        marker: Option<(Marker, Range<usize>)>,
    ) {
        let chars = align(literal, self.text, range.clone());
        let nodes = markup::parse(literal);
        let mut w = Writer {
            b: self,
            map: Map {
                literal,
                chars,
                end: range.end,
            },
            block: range,
            runs: Vec::new(),
            marker,
        };
        let cx = Cx {
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

impl Writer<'_, '_, '_> {
    fn flush(&mut self, cx: &Cx) {
        if let Some(last) = self.runs.last_mut()
            && matches!(last.map, RunMap::Atom { atom: Atom::Text, .. })
        {
            let trimmed = last.text.trim_end_matches(' ').len();
            last.text.truncate(trimmed);
        }
        self.runs.retain(|r| !r.text.is_empty());
        if self.runs.is_empty() {
            return;
        }
        let runs = std::mem::take(&mut self.runs);
        let marker = self.marker.take();
        let src = self.block.clone();
        self.b
            .push_para(cx.kind.clone(), &cx.ctx, marker, src.clone(), runs, src.end);
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

    fn nodes(&mut self, nodes: &[Node], cx: &Cx) {
        for node in nodes {
            match node {
                Node::Text { text, range } => self.text(text, range, &cx.style),
                Node::Element(e) => self.element(e, cx),
            }
        }
    }

    fn element(&mut self, e: &Element, cx: &Cx) {
        let src = self.map.source(&e.tag);
        let styled = |f: &dyn Fn(&mut RunStyle)| {
            let mut c = cx.clone();
            f(&mut c.style);
            c
        };
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
            "b" | "strong" => self.nodes(&e.children, &styled(&|s| s.bold = true)),
            "i" | "em" | "cite" | "dfn" | "var" => self.nodes(&e.children, &styled(&|s| s.italic = true)),
            "u" | "ins" => self.nodes(&e.children, &styled(&|s| s.underline = true)),
            "s" | "del" | "strike" => self.nodes(&e.children, &styled(&|s| s.strike = true)),
            "mark" => self.nodes(&e.children, &styled(&|s| s.highlight = true)),
            "sup" => self.nodes(&e.children, &styled(&|s| s.superscript = true)),
            "sub" => self.nodes(&e.children, &styled(&|s| s.subscript = true)),
            "code" | "kbd" | "samp" | "tt" => self.nodes(&e.children, &styled(&|s| s.code = true)),
            "a" => {
                let href = e.attr("href").filter(|h| markup::safe_url(h)).map(Into::into);
                let mut c = cx.clone();
                if href.is_some() {
                    c.style.link = href;
                }
                self.nodes(&e.children, &c);
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = e.name[1..].parse().unwrap_or(1);
                self.block(e, cx, ParaKind::Heading(level));
            }
            "p" | "div" | "center" | "section" | "article" | "header" | "footer" | "main" | "nav" | "aside"
            | "figure" | "figcaption" | "address" | "summary" => self.block(e, cx, ParaKind::Body),
            "hr" => {
                self.flush(cx);
                let run = atom(Atom::Rule, "\u{00A0}", src);
                self.runs.push(run);
                let rule = Cx {
                    kind: ParaKind::Rule,
                    ..cx.clone()
                };
                self.flush(&rule);
            }
            "pre" => {
                self.flush(cx);
                let mut code = String::new();
                raw_text(&e.children, &mut code);
                let code = code.trim_matches('\n').to_string();
                if !code.is_empty() {
                    let mut c = cx.clone();
                    c.kind = ParaKind::Code { info: "".into() };
                    c.ctx.group = self.b.next_group();
                    c.style.code = true;
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
                for item in e.children.iter().filter_map(element) {
                    let mut c = cx.clone();
                    if item.name == "dd" {
                        c.ctx.list_depth += 1;
                    } else {
                        c.style.bold = true;
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
    fn block(&mut self, e: &Element, cx: &Cx, kind: ParaKind) {
        self.flush(cx);
        let mut c = cx.clone();
        c.kind = kind;
        if let Some(a) = e
            .align()
            .map(align_key)
            .or((e.name == "center").then_some(AlignKey::Center))
        {
            c.ctx.align = a;
        }
        self.nodes(&e.children, &c);
        self.flush(&c);
    }

    fn image(&mut self, e: &Element, src: Range<usize>, style: &RunStyle) {
        let alt = e.attr("alt").unwrap_or("");
        match e.attr("src").filter(|u| markup::safe_url(u)) {
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

    fn list(&mut self, e: &Element, cx: &Cx) {
        self.flush(cx);
        let start: usize = e.attr("start").and_then(|s| s.parse().ok()).unwrap_or(1);
        let depth = cx.ctx.list_depth;
        for (n, item) in e
            .children
            .iter()
            .filter_map(element)
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

    fn table(&mut self, e: &Element, cx: &Cx) {
        self.flush(cx);
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
        let cells = |row: &Element| -> Vec<Element> {
            row.children
                .iter()
                .filter_map(element)
                .filter(|c| c.name == "td" || c.name == "th")
                .cloned()
                .collect()
        };
        let span = |cell: &Element| cell.pixels("colspan").unwrap_or(1).clamp(1, 64);
        let columns = rows
            .iter()
            .map(|(r, _)| cells(r).iter().map(span).sum::<u32>())
            .max()
            .unwrap_or(0);
        if columns == 0 {
            return;
        }
        self.b.table += 1;
        let table = self.b.table as u32;
        let mut inner = cx.ctx.clone();
        inner.group = self.b.next_group();
        let start = self.b.paras.len();
        for (row, (r, in_head)) in rows.iter().enumerate() {
            let mut col = 0;
            for cell in cells(r) {
                let header = *in_head || cell.name == "th";
                let width = span(&cell).min(columns - col);
                let c = Cx {
                    ctx: inner.clone(),
                    kind: ParaKind::TableCell {
                        table,
                        row: row as u32,
                        col,
                        header,
                        align: cell.align().map(align_key).unwrap_or_default(),
                        columns,
                        span: width,
                    },
                    style: RunStyle {
                        bold: header,
                        ..cx.style.clone()
                    },
                };
                self.cell(&cell, &c);
                col += width;
                if col >= columns {
                    break;
                }
            }
            while col < columns {
                let c = Cx {
                    ctx: inner.clone(),
                    kind: ParaKind::TableCell {
                        table,
                        row: row as u32,
                        col,
                        header: *in_head,
                        align: AlignKey::Natural,
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

    /// A table cell is one paragraph, so block elements inside it run together.
    fn cell(&mut self, cell: &Element, cx: &Cx) {
        self.nodes(
            &cell.children,
            &Cx {
                kind: ParaKind::Body,
                ..cx.clone()
            },
        );
        let src = self.map.source(&cell.range);
        let mut runs = std::mem::take(&mut self.runs);
        for r in &mut runs {
            r.style.bold |= cx.style.bold;
        }
        self.b
            .push_para(cx.kind.clone(), &cx.ctx, None, src.clone(), runs, src.end);
    }

    fn details(&mut self, e: &Element, cx: &Cx) {
        self.flush(cx);
        let open = e.attr("open").is_some();
        let toggle = RunStyle {
            link: Some("splat-fold".into()),
            ..cx.style.clone()
        };
        self.runs.push(atom_styled(
            Atom::Details { open },
            if open { "▾ " } else { "▸ " },
            self.map.source(&e.tag),
            &toggle,
        ));
        let summary = Cx {
            style: RunStyle {
                bold: true,
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

fn element(node: &Node) -> Option<&Element> {
    match node {
        Node::Element(e) => Some(e),
        Node::Text { .. } => None,
    }
}

fn find<'n>(nodes: &'n [Node], name: &str) -> Option<&'n Element> {
    nodes.iter().filter_map(element).find_map(|e| {
        if e.name == name {
            Some(e)
        } else {
            find(&e.children, name)
        }
    })
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
