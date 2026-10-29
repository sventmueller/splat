//! The document as Word file (Office Open XML), written from the Markdown structure: headings become Word's
//! heading styles, lists real lists, tables tables. The system's own Word export drops images and tables.

mod html;

use std::collections::HashMap;

use super::doc::{Align, Block, BlockKind, Doc, Figure, Inline, InlineKind, ListKind, diagram_kind, is_math_fence};
use super::typography::{Mark, Quotes, marks, smarten};
use crate::i18n::t;

/// An image for the document, as PNG or JPEG data, with its size in points.
pub struct Picture {
    pub data: Vec<u8>,
    pub jpeg: bool,
    pub width: f64,
    pub height: f64,
}

/// What the document needs from outside the text.
pub struct WordExport<'a> {
    /// The Markdown the document was parsed from.
    pub source: &'a str,
    pub figure: &'a dyn Fn(&Figure) -> Option<Picture>,
    pub image: &'a dyn Fn(&str) -> Option<Picture>,
    /// Quotation marks for straight quotes, when typographic punctuation is on.
    pub quotes: Option<Quotes>,
    /// The web page of `@name`, `#123` or `!123`, when the document lies in a repository.
    pub reference: &'a dyn Fn(&str) -> Option<String>,
    /// How a citation key reads, such as `Doe 2020`.
    pub citation: &'a dyn Fn(&str) -> Option<String>,
}

/// Width of the text on an A4 page with Word's default margins.
const TEXT_WIDTH: f64 = 451.0;
const EMU_PER_POINT: f64 = 12_700.0;
/// Space after a paragraph, in twentieths of a point, as the default paragraph style sets it.
const GAP: u32 = 160;

pub fn to_docx(doc: &Doc, export: &WordExport) -> Vec<u8> {
    let mut w = Writer {
        export,
        body: String::new(),
        media: Vec::new(),
        links: Vec::new(),
        numbers: Vec::new(),
        footnotes: Vec::new(),
        footnote_ix: HashMap::new(),
        last_paragraph: 0,
        gap: false,
    };
    let ctx = Ctx::default();
    for block in &doc.blocks {
        w.block(block, &ctx);
    }
    w.write_footnotes();
    w.package()
}

#[derive(Clone, Default)]
struct Ctx {
    quote: u8,
    /// Numbering id and level of the list item a paragraph belongs to.
    list: Option<(usize, usize)>,
    depth: usize,
    /// The first paragraph of a task item starts with its checkbox.
    task: Option<bool>,
    /// Indentation of definition details, in list levels.
    indent: usize,
    /// Definition terms are bold.
    strong: bool,
}

#[derive(Clone, Copy, Default)]
struct Props {
    bold: bool,
    italic: bool,
    strike: bool,
    underline: bool,
    highlight: bool,
    sup: bool,
    sub: bool,
    code: bool,
    spoiler: bool,
    tag: bool,
}

struct Writer<'a> {
    export: &'a WordExport<'a>,
    body: String,
    media: Vec<(Vec<u8>, bool)>,
    links: Vec<String>,
    /// Ordered lists, each with its own numbering so it restarts, by start number.
    numbers: Vec<usize>,
    footnotes: Vec<(u32, &'a Block)>,
    footnote_ix: HashMap<String, u32>,
    /// Where the last paragraph starts in `body`.
    last_paragraph: usize,
    /// The next paragraph carries the space the block before it could not have after itself.
    gap: bool,
}

impl<'a> Writer<'a> {
    fn block(&mut self, block: &'a Block, ctx: &Ctx) {
        match &block.kind {
            BlockKind::Paragraph => self.paragraph(None, ctx, &block.inlines),
            BlockKind::Heading { level, .. } => {
                self.paragraph(Some(&format!("Heading{}", level.clamp(&1, &6))), ctx, &block.inlines)
            }
            BlockKind::BlockQuote => {
                let inner = Ctx {
                    quote: ctx.quote + 1,
                    ..ctx.clone()
                };
                let start = self.body.len();
                self.children(block, &inner);
                if ctx.quote == 0 {
                    self.end_framed_block(start);
                }
            }
            BlockKind::Alert { kind, title, .. } => {
                let inner = Ctx {
                    quote: ctx.quote + 1,
                    ..ctx.clone()
                };
                let label = title.as_deref().unwrap_or(t(kind.label()));
                let title = Inline {
                    kind: InlineKind::Strong,
                    range: 0..0,
                    children: Box::new([Inline {
                        kind: InlineKind::Text(label.into()),
                        range: 0..0,
                        children: Box::default(),
                    }]),
                };
                let start = self.body.len();
                self.paragraph(None, &inner, std::slice::from_ref(&title));
                self.children(block, &inner);
                if ctx.quote == 0 {
                    self.end_framed_block(start);
                }
            }
            BlockKind::List { kind, .. } => {
                let id = match kind {
                    ListKind::Bullet(_) => 1,
                    ListKind::Ordered { start, .. } => {
                        self.numbers.push(*start);
                        self.numbers.len() + 1
                    }
                };
                for item in &block.children {
                    let inner = Ctx {
                        list: Some((id, ctx.depth)),
                        depth: ctx.depth + 1,
                        task: match &item.kind {
                            BlockKind::Item { task: Some(task), .. } => Some(task.checked),
                            _ => None,
                        },
                        ..ctx.clone()
                    };
                    self.item(item, &inner);
                }
            }
            BlockKind::Item { .. } => self.item(block, ctx),
            BlockKind::CodeBlock { info, literal, .. } => {
                let tex = literal.strip_suffix('\n').unwrap_or(literal);
                let figure = match diagram_kind(info) {
                    Some(kind) => Some((Figure::Diagram { source: literal, kind }, t("Diagramm"))),
                    None if is_math_fence(info) => Some((Figure::Math { tex, display: true }, tex)),
                    None => None,
                };
                match figure.and_then(|(f, alt)| Some(((self.export.figure)(&f)?, alt))) {
                    Some((picture, alt)) => {
                        let run = self.picture(picture, alt);
                        self.raw_paragraph("<w:jc w:val=\"center\"/>", &run);
                    }
                    None => self.code(literal),
                }
            }
            BlockKind::DefinitionList => {
                for item in &block.children {
                    for part in &item.children {
                        let inner = match part.kind {
                            BlockKind::DefinitionTerm => Ctx {
                                strong: true,
                                ..ctx.clone()
                            },
                            _ => Ctx {
                                indent: ctx.indent + 1,
                                ..ctx.clone()
                            },
                        };
                        self.children(part, &inner);
                    }
                }
            }
            BlockKind::Comment => {}
            BlockKind::HtmlBlock { literal } => self.html(literal, ctx),
            BlockKind::MathBlock { literal } => {
                let figure = Figure::Math {
                    tex: literal,
                    display: true,
                };
                match (self.export.figure)(&figure) {
                    Some(picture) => {
                        let run = self.picture(picture, literal);
                        self.raw_paragraph("<w:jc w:val=\"center\"/>", &run);
                    }
                    None => self.code(&format!("$${literal}$$")),
                }
            }
            BlockKind::ThematicBreak => self.raw_paragraph(
                "<w:pBdr><w:bottom w:val=\"single\" w:sz=\"6\" w:space=\"1\" w:color=\"D2D2D7\"/></w:pBdr>",
                "",
            ),
            BlockKind::Table { alignments } => self.table(block, alignments),
            BlockKind::FootnoteDefinition { name, .. } => {
                let ix = self.footnote_number(name);
                self.footnotes.push((ix, block));
            }
            BlockKind::FrontMatter { .. }
            | BlockKind::TableRow { .. }
            | BlockKind::TableCell
            | BlockKind::DefinitionItem
            | BlockKind::DefinitionTerm
            | BlockKind::DefinitionDetails => {}
        }
    }

    fn children(&mut self, block: &'a Block, ctx: &Ctx) {
        for child in &block.children {
            self.block(child, ctx);
        }
    }

    fn item(&mut self, item: &'a Block, ctx: &Ctx) {
        for (n, child) in item.children.iter().enumerate() {
            let inner = if n == 0 {
                ctx.clone()
            } else {
                // Further paragraphs of an item continue its indentation without a new bullet.
                Ctx {
                    list: ctx.list.map(|(_, level)| (0, level)),
                    task: None,
                    ..ctx.clone()
                }
            };
            self.block(child, &inner);
        }
    }

    fn paragraph(&mut self, style: Option<&str>, ctx: &Ctx, inlines: &[Inline]) {
        let mut ppr = String::new();
        let style = style.or((ctx.quote > 0).then_some("Quote"));
        if let Some(style) = style {
            ppr.push_str(&format!("<w:pStyle w:val=\"{style}\"/>"));
        }
        if let Some((id, level)) = ctx.list {
            if id == 0 {
                ppr.push_str(&format!("<w:ind w:left=\"{}\"/>", 720 * (level + 1)));
            } else {
                ppr.push_str(&format!(
                    "<w:numPr><w:ilvl w:val=\"{level}\"/><w:numId w:val=\"{id}\"/></w:numPr>"
                ));
            }
        } else if ctx.indent > 0 {
            ppr.push_str(&format!("<w:ind w:left=\"{}\"/>", 567 * ctx.indent));
        }
        let mut runs = String::new();
        if let Some(checked) = ctx.task {
            runs.push_str(&text_run(if checked { "☑ " } else { "☐ " }, Props::default()));
        }
        let base = Props {
            bold: ctx.strong,
            ..Props::default()
        };
        self.inlines(inlines, base, &mut runs);
        self.raw_paragraph(&ppr, &runs);
    }

    fn raw_paragraph(&mut self, ppr: &str, runs: &str) {
        let ppr = if std::mem::take(&mut self.gap) {
            with_spacing(ppr, "w:before", GAP)
        } else {
            ppr.to_string()
        };
        self.last_paragraph = self.body.len();
        self.body.push_str("<w:p>");
        if !ppr.is_empty() {
            self.body.push_str(&format!("<w:pPr>{ppr}</w:pPr>"));
        }
        self.body.push_str(runs);
        self.body.push_str("</w:p>");
    }

    fn code(&mut self, literal: &str) {
        let start = self.body.len();
        for line in literal.strip_suffix('\n').unwrap_or(literal).split('\n') {
            let run = text_run(line, Props::default());
            self.raw_paragraph("<w:pStyle w:val=\"Code\"/>", &run);
        }
        self.end_framed_block(start);
    }

    /// Word draws a quote's bar and a code block's shading through the space after their last paragraph, so that
    /// space moves to the start of what follows. `start` is where the block begins in `body`.
    fn end_framed_block(&mut self, start: usize) {
        if self.last_paragraph < start || self.last_paragraph >= self.body.len() {
            return;
        }
        let last = self.body.split_off(self.last_paragraph);
        let patched = match (last.strip_prefix("<w:p><w:pPr>"), last.find("</w:pPr>")) {
            (Some(_), Some(end)) => format!(
                "<w:p><w:pPr>{}{}",
                with_spacing(&last["<w:p><w:pPr>".len()..end], "w:after", 0),
                &last[end..]
            ),
            _ => last.replacen("<w:p>", "<w:p><w:pPr><w:spacing w:after=\"0\"/></w:pPr>", 1),
        };
        self.body.push_str(&patched);
        self.gap = true;
    }

    fn table(&mut self, table: &'a Block, alignments: &[Align]) {
        if std::mem::take(&mut self.gap) {
            // A table has no space before, so an empty paragraph of that height stands in.
            self.raw_paragraph(
                &format!("<w:spacing w:after=\"0\" w:line=\"{GAP}\" w:lineRule=\"exact\"/>"),
                "",
            );
        }
        self.body.push_str(
            "<w:tbl><w:tblPr><w:tblStyle w:val=\"TableGrid\"/><w:tblW w:w=\"0\" w:type=\"auto\"/></w:tblPr><w:tblGrid>",
        );
        for _ in alignments {
            self.body.push_str("<w:gridCol/>");
        }
        self.body.push_str("</w:tblGrid>");
        for row in &table.children {
            let header = matches!(row.kind, BlockKind::TableRow { header: true });
            self.body.push_str("<w:tr>");
            if header {
                self.body.push_str("<w:trPr><w:tblHeader/></w:trPr>");
            }
            for (col, cell) in row.children.iter().enumerate() {
                self.body.push_str("<w:tc><w:tcPr><w:tcW w:w=\"0\" w:type=\"auto\"/>");
                if header {
                    self.body
                        .push_str("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F2F2F4\"/>");
                }
                self.body.push_str("</w:tcPr>");
                let jc = match alignments.get(col) {
                    Some(Align::Center) => "<w:jc w:val=\"center\"/>",
                    Some(Align::Right) => "<w:jc w:val=\"right\"/>",
                    _ => "",
                };
                let mut runs = String::new();
                let props = Props {
                    bold: header,
                    ..Props::default()
                };
                self.inlines(&cell.inlines, props, &mut runs);
                self.raw_paragraph(&format!("<w:spacing w:after=\"0\"/>{jc}"), &runs);
                self.body.push_str("</w:tc>");
            }
            self.body.push_str("</w:tr>");
        }
        self.body.push_str("</w:tbl>");
        // Word needs a paragraph between consecutive tables and before the end of the document.
        self.raw_paragraph("", "");
    }

    fn inlines(&mut self, inlines: &[Inline], props: Props, out: &mut String) {
        let mut props = props;
        for inline in inlines {
            match &inline.kind {
                InlineKind::Text(text) => self.text(text, inline.range.start, props, out),
                InlineKind::SoftBreak => out.push_str(&text_run(" ", props)),
                InlineKind::LineBreak => out.push_str("<w:r><w:br/></w:r>"),
                InlineKind::Code(code) => out.push_str(&text_run(code, Props { code: true, ..props })),
                InlineKind::Html(raw) => match tag(raw) {
                    Some((name, open)) => match name.as_str() {
                        "u" | "ins" => props.underline = open,
                        "mark" => props.highlight = open,
                        "sup" => props.sup = open,
                        "sub" => props.sub = open,
                        "s" | "del" => props.strike = open,
                        "b" | "strong" => props.bold = open,
                        "i" | "em" => props.italic = open,
                        "br" => out.push_str("<w:r><w:br/></w:r>"),
                        _ => out.push_str(&text_run(raw, props)),
                    },
                    None => out.push_str(&text_run(raw, props)),
                },
                InlineKind::Emph => self.inlines(&inline.children, Props { italic: true, ..props }, out),
                InlineKind::Strong => self.inlines(&inline.children, Props { bold: true, ..props }, out),
                InlineKind::Strikethrough => self.inlines(&inline.children, Props { strike: true, ..props }, out),
                InlineKind::Underline => self.inlines(
                    &inline.children,
                    Props {
                        underline: true,
                        ..props
                    },
                    out,
                ),
                InlineKind::Highlight => self.inlines(
                    &inline.children,
                    Props {
                        highlight: true,
                        ..props
                    },
                    out,
                ),
                InlineKind::Superscript => self.inlines(&inline.children, Props { sup: true, ..props }, out),
                InlineKind::Subscript => self.inlines(&inline.children, Props { sub: true, ..props }, out),
                InlineKind::Spoiler => self.inlines(&inline.children, Props { spoiler: true, ..props }, out),
                InlineKind::Emoji(emoji) => out.push_str(&text_run(emoji, props)),
                InlineKind::Kbd => self.inlines(&inline.children, Props { code: true, ..props }, out),
                InlineKind::Span => self.inlines(&inline.children, props, out),
                InlineKind::HtmlImage {
                    url,
                    alt,
                    width,
                    height,
                } => match (self.export.image)(url) {
                    Some(picture) => {
                        let picture = asked_size(picture, *width, *height);
                        out.push_str(&self.picture(picture, alt));
                    }
                    None => out.push_str(&text_run(alt, props)),
                },
                InlineKind::Comment => {}
                InlineKind::WikiLink { target, embed } => {
                    let picture = embed.then(|| (self.export.image)(target)).flatten();
                    match picture {
                        Some(picture) => out.push_str(&self.picture(picture, target)),
                        None => self.inlines(&inline.children, props, out),
                    }
                }
                InlineKind::Link { url, .. } => {
                    let lower = url.to_ascii_lowercase();
                    if ["http://", "https://", "mailto:"].iter().any(|s| lower.starts_with(s)) {
                        let mut inner = String::new();
                        self.inlines(&inline.children, props, &mut inner);
                        let link = self.hyperlink(url, &inner);
                        out.push_str(&link);
                    } else {
                        self.inlines(&inline.children, props, out);
                    }
                }
                InlineKind::Image { url, .. } => match (self.export.image)(url) {
                    Some(picture) => {
                        let alt = plain(&inline.children);
                        out.push_str(&self.picture(picture, &alt));
                    }
                    None => self.inlines(&inline.children, props, out),
                },
                InlineKind::FootnoteRef { name, .. } => {
                    let ix = self.footnote_number(name);
                    out.push_str(&text_run(&ix.to_string(), Props { sup: true, ..props }));
                }
                InlineKind::Math { display, literal } => {
                    let figure = Figure::Math {
                        tex: literal,
                        display: *display,
                    };
                    match (self.export.figure)(&figure) {
                        Some(picture) => out.push_str(&self.picture(picture, literal)),
                        None => out.push_str(&text_run(&format!("${literal}$"), Props { code: true, ..props })),
                    }
                }
            }
        }
    }

    /// Running text, with typographic punctuation, tags, references and citations as in the preview.
    fn text(&mut self, literal: &str, at: usize, props: Props, out: &mut String) {
        let source = self.export.source;
        let before = source
            .get(..source.floor_char_boundary(at))
            .and_then(|s| s.chars().next_back());
        let shown = match self.export.quotes {
            Some(q) if !props.code => smarten(literal, before, q),
            _ => literal.to_string(),
        };
        let mut done = 0;
        for (range, mark) in marks(&shown, before, true) {
            let piece = &shown[range.clone()];
            let replacement = match &mark {
                Mark::Tag(_) => Some(text_run(piece, Props { tag: true, ..props })),
                Mark::Reference(r) => {
                    (self.export.reference)(r).map(|url| self.hyperlink(&url, &text_run(piece, props)))
                }
                Mark::Citation(keys) => {
                    let names: Option<Vec<String>> = keys.iter().map(|k| (self.export.citation)(k)).collect();
                    names.map(|n| text_run(&format!("({})", n.join("; ")), props))
                }
            };
            let Some(replacement) = replacement else { continue };
            if range.start > done {
                out.push_str(&text_run(&shown[done..range.start], props));
            }
            out.push_str(&replacement);
            done = range.end;
        }
        if done < shown.len() || done == 0 {
            out.push_str(&text_run(&shown[done..], props));
        }
    }

    fn hyperlink(&mut self, url: &str, runs: &str) -> String {
        self.links.push(url.to_string());
        format!(
            "<w:hyperlink r:id=\"rIdL{}\">{}</w:hyperlink>",
            self.links.len(),
            runs.replace("<w:rPr>", "<w:rPr><w:rStyle w:val=\"Hyperlink\"/>")
        )
    }

    fn footnote_number(&mut self, name: &str) -> u32 {
        let next = self.footnote_ix.len() as u32 + 1;
        *self.footnote_ix.entry(name.to_string()).or_insert(next)
    }

    fn write_footnotes(&mut self) {
        if self.footnotes.is_empty() {
            return;
        }
        self.raw_paragraph(
            "<w:pBdr><w:top w:val=\"single\" w:sz=\"4\" w:space=\"1\" w:color=\"D2D2D7\"/></w:pBdr>",
            "",
        );
        let mut notes = std::mem::take(&mut self.footnotes);
        notes.sort_by_key(|(ix, _)| *ix);
        for (ix, block) in notes {
            for (n, child) in block.children.iter().enumerate() {
                let mut runs = String::new();
                if n == 0 {
                    runs.push_str(&text_run(&format!("{ix}. "), Props::default()));
                }
                self.inlines(&child.inlines, Props::default(), &mut runs);
                self.raw_paragraph("<w:pStyle w:val=\"FootnoteText\"/>", &runs);
            }
        }
    }

    /// An inline picture, no wider than the text.
    fn picture(&mut self, picture: Picture, alt: &str) -> String {
        let scale = (TEXT_WIDTH / picture.width.max(1.0)).min(1.0);
        let cx = (picture.width * scale * EMU_PER_POINT).round() as u64;
        let cy = (picture.height * scale * EMU_PER_POINT).round() as u64;
        self.media.push((picture.data, picture.jpeg));
        let n = self.media.len();
        let ext = if picture.jpeg { "jpeg" } else { "png" };
        format!(
            "<w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/>\
             <wp:docPr id=\"{n}\" name=\"image{n}\" descr=\"{alt}\"/><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
             <pic:pic><pic:nvPicPr><pic:cNvPr id=\"{n}\" name=\"image{n}.{ext}\"/><pic:cNvPicPr/></pic:nvPicPr>\
             <pic:blipFill><a:blip r:embed=\"rIdI{n}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
             <pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>\
             </pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>",
            alt = xml(alt)
        )
    }

    fn package(self) -> Vec<u8> {
        let document = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document {NAMESPACES}><w:body>{}\
             <w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1417\" w:right=\"1417\" w:bottom=\"1134\" w:left=\"1417\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr>\
             </w:body></w:document>",
            self.body
        );
        let mut rels = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
             <Relationship Id=\"rIdStyles\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>\
             <Relationship Id=\"rIdNumbering\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/>\
             <Relationship Id=\"rIdFonts\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/fontTable\" Target=\"fontTable.xml\"/>\
             <Relationship Id=\"rIdSettings\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings\" Target=\"settings.xml\"/>",
        );
        for (n, (_, jpeg)) in self.media.iter().enumerate() {
            rels.push_str(&format!(
                "<Relationship Id=\"rIdI{0}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/image{0}.{1}\"/>",
                n + 1,
                if *jpeg { "jpeg" } else { "png" }
            ));
        }
        for (n, url) in self.links.iter().enumerate() {
            rels.push_str(&format!(
                "<Relationship Id=\"rIdL{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"{}\" TargetMode=\"External\"/>",
                n + 1,
                xml(url)
            ));
        }
        rels.push_str("</Relationships>");
        let mut zip = Zip::default();
        zip.add("[Content_Types].xml", CONTENT_TYPES.as_bytes());
        zip.add("_rels/.rels", PACKAGE_RELS.as_bytes());
        zip.add("word/document.xml", document.as_bytes());
        zip.add("word/_rels/document.xml.rels", rels.as_bytes());
        zip.add("word/styles.xml", STYLES.as_bytes());
        zip.add("word/numbering.xml", numbering(&self.numbers).as_bytes());
        zip.add("word/fontTable.xml", FONT_TABLE.as_bytes());
        zip.add("word/settings.xml", SETTINGS.as_bytes());
        for (n, (data, jpeg)) in self.media.iter().enumerate() {
            zip.add(
                &format!("word/media/image{}.{}", n + 1, if *jpeg { "jpeg" } else { "png" }),
                data,
            );
        }
        zip.finish()
    }
}

/// A picture at the size embedded HTML asks for, in pixels of 0.75 pt like everything else on paper.
fn asked_size(picture: Picture, width: Option<u32>, height: Option<u32>) -> Picture {
    if picture.width <= 0.0 || picture.height <= 0.0 {
        return picture;
    }
    let ratio = picture.height / picture.width;
    let (w, h) = match (width.map(|w| f64::from(w) * 0.75), height.map(|h| f64::from(h) * 0.75)) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, w * ratio),
        (None, Some(h)) => (h / ratio, h),
        (None, None) => (picture.width, picture.height),
    };
    Picture {
        width: w,
        height: h,
        ..picture
    }
}

/// Paragraph properties with `attribute` set on their spacing, unless it is set already; Word requires the spacing
/// after list numbering and before indentation and alignment.
fn with_spacing(ppr: &str, attribute: &str, value: u32) -> String {
    if ppr.contains(&format!("{attribute}=")) {
        return ppr.to_string();
    }
    if ppr.contains("<w:spacing ") {
        return ppr.replacen("<w:spacing ", &format!("<w:spacing {attribute}=\"{value}\" "), 1);
    }
    let at = ["<w:ind", "<w:jc"]
        .iter()
        .filter_map(|tag| ppr.find(tag))
        .min()
        .unwrap_or(ppr.len());
    format!("{}<w:spacing {attribute}=\"{value}\"/>{}", &ppr[..at], &ppr[at..])
}

/// The name of a single HTML tag and whether it opens, for the inline tags Splat writes.
fn tag(raw: &str) -> Option<(String, bool)> {
    let inner = raw
        .trim()
        .strip_prefix('<')?
        .strip_suffix('>')?
        .trim_end_matches('/')
        .trim();
    let (name, open) = match inner.strip_prefix('/') {
        Some(name) => (name.trim(), false),
        None => (inner, true),
    };
    name.chars()
        .all(|c| c.is_ascii_alphanumeric())
        .then(|| (name.to_ascii_lowercase(), open))
}

fn plain(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for i in inlines {
        match &i.kind {
            InlineKind::Text(t) | InlineKind::Code(t) => out.push_str(t),
            _ => out.push_str(&plain(&i.children)),
        }
    }
    out
}

fn text_run(text: &str, p: Props) -> String {
    let mut rpr = String::new();
    if p.code {
        rpr.push_str("<w:rStyle w:val=\"CodeChar\"/>");
    }
    // In the order Word's schema requires.
    for (on, tag) in [
        (p.bold, "<w:b/>"),
        (p.italic, "<w:i/>"),
        (p.strike, "<w:strike/>"),
        (p.spoiler, "<w:color w:val=\"1D1D1F\"/>"),
        (p.tag && !p.spoiler, "<w:color w:val=\"0066CC\"/>"),
        (p.highlight, "<w:highlight w:val=\"yellow\"/>"),
        (p.underline, "<w:u w:val=\"single\"/>"),
        (p.spoiler, "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"1D1D1F\"/>"),
        (p.sup, "<w:vertAlign w:val=\"superscript\"/>"),
        (p.sub, "<w:vertAlign w:val=\"subscript\"/>"),
    ] {
        if on {
            rpr.push_str(tag);
        }
    }
    format!(
        "<w:r><w:rPr>{rpr}</w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r>",
        xml(text)
    )
}

/// Escapes text for XML and drops the control characters XML 1.0 does not allow.
fn xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

fn numbering(ordered: &[usize]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">",
    );
    for (id, decimal) in [(0, false), (1, true)] {
        out.push_str(&format!(
            "<w:abstractNum w:abstractNumId=\"{id}\"><w:multiLevelType w:val=\"hybridMultilevel\"/>"
        ));
        for level in 0..9 {
            let (format, text) = if decimal {
                ("decimal", format!("%{}.", level + 1))
            } else {
                ("bullet", ["•", "◦", "▪"][level % 3].to_string())
            };
            out.push_str(&format!(
                "<w:lvl w:ilvl=\"{level}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{format}\"/><w:lvlText w:val=\"{text}\"/>\
                 <w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr></w:lvl>",
                720 * (level + 1)
            ));
        }
        out.push_str("</w:abstractNum>");
    }
    out.push_str("<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>");
    for (n, start) in ordered.iter().enumerate() {
        out.push_str(&format!(
            "<w:num w:numId=\"{}\"><w:abstractNumId w:val=\"1\"/><w:lvlOverride w:ilvl=\"0\"><w:startOverride w:val=\"{start}\"/></w:lvlOverride></w:num>",
            n + 2
        ));
    }
    out.push_str("</w:numbering>");
    out
}

const NAMESPACES: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
    xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
    xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
    xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
    xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"";

const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Default Extension=\"png\" ContentType=\"image/png\"/>\
<Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
<Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>\
<Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/>\
<Override PartName=\"/word/fontTable.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml\"/>\
<Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/>\
</Types>";

const PACKAGE_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
</Relationships>";

const STYLES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Helvetica Neue\" w:hAnsi=\"Helvetica Neue\" w:eastAsia=\"Helvetica Neue\" w:cs=\"Helvetica Neue\"/><w:sz w:val=\"22\"/><w:szCs w:val=\"22\"/></w:rPr></w:rPrDefault>\
<w:pPrDefault><w:pPr><w:spacing w:after=\"160\" w:line=\"276\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>\
<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:spacing w:before=\"360\" w:after=\"120\"/><w:outlineLvl w:val=\"0\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"36\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading2\"><w:name w:val=\"heading 2\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:spacing w:before=\"280\" w:after=\"100\"/><w:outlineLvl w:val=\"1\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"30\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading3\"><w:name w:val=\"heading 3\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:spacing w:before=\"240\" w:after=\"80\"/><w:outlineLvl w:val=\"2\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"26\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading4\"><w:name w:val=\"heading 4\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:outlineLvl w:val=\"3\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"24\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading5\"><w:name w:val=\"heading 5\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:outlineLvl w:val=\"4\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"22\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading6\"><w:name w:val=\"heading 6\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:outlineLvl w:val=\"5\"/></w:pPr><w:rPr><w:b/><w:i/><w:sz w:val=\"22\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Quote\"><w:name w:val=\"Quote\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:pBdr><w:left w:val=\"single\" w:sz=\"18\" w:space=\"8\" w:color=\"D2D2D7\"/></w:pBdr><w:ind w:left=\"567\"/></w:pPr><w:rPr><w:color w:val=\"555555\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Code\"><w:name w:val=\"Code\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F3F3F5\"/></w:pPr><w:rPr><w:rFonts w:ascii=\"Menlo\" w:hAnsi=\"Menlo\" w:cs=\"Menlo\"/><w:sz w:val=\"18\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"FootnoteText\"><w:name w:val=\"footnote text\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:spacing w:after=\"60\"/></w:pPr><w:rPr><w:sz w:val=\"18\"/></w:rPr></w:style>\
<w:style w:type=\"character\" w:styleId=\"CodeChar\"><w:name w:val=\"Code Char\"/><w:rPr><w:rFonts w:ascii=\"Menlo\" w:hAnsi=\"Menlo\" w:cs=\"Menlo\"/><w:sz w:val=\"19\"/><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F3F3F5\"/></w:rPr></w:style>\
<w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/><w:rPr><w:color w:val=\"0563C1\"/><w:u w:val=\"single\"/></w:rPr></w:style>\
<w:style w:type=\"table\" w:styleId=\"TableGrid\"><w:name w:val=\"Table Grid\"/><w:tblPr><w:tblBorders>\
<w:top w:val=\"single\" w:sz=\"4\" w:color=\"D2D2D7\"/><w:left w:val=\"single\" w:sz=\"4\" w:color=\"D2D2D7\"/><w:bottom w:val=\"single\" w:sz=\"4\" w:color=\"D2D2D7\"/>\
<w:right w:val=\"single\" w:sz=\"4\" w:color=\"D2D2D7\"/><w:insideH w:val=\"single\" w:sz=\"4\" w:color=\"D2D2D7\"/><w:insideV w:val=\"single\" w:sz=\"4\" w:color=\"D2D2D7\"/>\
</w:tblBorders><w:tblCellMar><w:left w:w=\"108\" w:type=\"dxa\"/><w:right w:w=\"108\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr></w:style>\
</w:styles>";

/// Without a font, Word sets Times New Roman. Helvetica Neue comes with every Mac; where it is missing, Word picks
/// a substitute by the family given here, a sans serif like Arial.
const FONT_TABLE: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<w:fonts xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
<w:font w:name=\"Helvetica Neue\"><w:altName w:val=\"Arial\"/><w:family w:val=\"swiss\"/><w:pitch w:val=\"variable\"/></w:font>\
<w:font w:name=\"Menlo\"><w:altName w:val=\"Consolas\"/><w:family w:val=\"modern\"/><w:pitch w:val=\"fixed\"/></w:font>\
</w:fonts>";

/// Without a compatibility mode Word opens the document in the mode of Word 2007 and says so in the title bar.
const SETTINGS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:compat>\
<w:compatSetting w:name=\"compatibilityMode\" w:uri=\"http://schemas.microsoft.com/office/word\" w:val=\"15\"/>\
</w:compat></w:settings>";

/// A ZIP archive with uncompressed entries, which Office reads like any other.
#[derive(Default)]
struct Zip {
    data: Vec<u8>,
    directory: Vec<u8>,
    count: u16,
}

impl Zip {
    fn add(&mut self, name: &str, content: &[u8]) {
        let crc = crc32(content);
        let offset = self.data.len() as u32;
        let size = content.len() as u32;
        let header = |signature: u32, central: bool| {
            let mut h = Vec::new();
            h.extend_from_slice(&signature.to_le_bytes());
            if central {
                h.extend_from_slice(&20u16.to_le_bytes());
            }
            // Version 2.0, names in UTF-8, stored, no time stamp.
            for v in [20u16, 0x0800, 0, 0, 0x21] {
                h.extend_from_slice(&v.to_le_bytes());
            }
            for v in [crc, size, size] {
                h.extend_from_slice(&v.to_le_bytes());
            }
            h.extend_from_slice(&(name.len() as u16).to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes());
            if central {
                for v in [0u16, 0, 0] {
                    h.extend_from_slice(&v.to_le_bytes());
                }
                h.extend_from_slice(&0u32.to_le_bytes());
                h.extend_from_slice(&offset.to_le_bytes());
            }
            h.extend_from_slice(name.as_bytes());
            h
        };
        self.data.extend(header(0x0403_4b50, false));
        self.data.extend_from_slice(content);
        self.directory.extend(header(0x0201_4b50, true));
        self.count += 1;
    }

    fn finish(mut self) -> Vec<u8> {
        let start = self.data.len() as u32;
        let size = self.directory.len() as u32;
        self.data.append(&mut self.directory);
        self.data.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        for v in [0u16, 0, self.count, self.count] {
            self.data.extend_from_slice(&v.to_le_bytes());
        }
        self.data.extend_from_slice(&size.to_le_bytes());
        self.data.extend_from_slice(&start.to_le_bytes());
        self.data.extend_from_slice(&0u16.to_le_bytes());
        self.data
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, entry) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *entry = c;
        }
        t
    });
    !bytes
        .iter()
        .fold(!0u32, |c, &b| table[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::doc::parse;

    fn export(text: &str) -> Vec<u8> {
        to_docx(
            &parse(text),
            &WordExport {
                source: text,
                quotes: Some(Quotes {
                    open: '„',
                    close: '“',
                    open_single: '‚',
                    close_single: '‘',
                }),
                reference: &|r| (r == "#12").then(|| "https://example.com/issues/12".to_string()),
                citation: &|k| (k == "doe").then(|| "Doe 2020".to_string()),
                figure: &|_| None,
                image: &|_| {
                    Some(Picture {
                        data: b"\x89PNG".to_vec(),
                        jpeg: false,
                        width: 900.0,
                        height: 300.0,
                    })
                },
            },
        )
    }

    /// Entry names and contents of a stored ZIP, read with nothing but the format description.
    fn entries(zip: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        let mut at = 0;
        while zip.get(at..at + 4) == Some(&0x0403_4b50u32.to_le_bytes()) {
            let field = |o: usize, n: usize| {
                zip[at + o..at + o + n]
                    .iter()
                    .rev()
                    .fold(0usize, |v, b| v << 8 | *b as usize)
            };
            let (size, name_len, extra) = (field(18, 4), field(26, 2), field(28, 2));
            let name = String::from_utf8(zip[at + 30..at + 30 + name_len].to_vec()).unwrap();
            let start = at + 30 + name_len + extra;
            let content = zip[start..start + size].to_vec();
            assert_eq!(crc32(&content) as usize, field(14, 4), "{name}");
            out.push((name, content));
            at = start + size;
        }
        out
    }

    #[test]
    fn crc_matches_the_reference_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn documents_keep_structure_images_and_safe_links_only() {
        let zip = export(
            "# Titel\n\nText mit **fett** und <mark>hell</mark> & [Web](https://example.com) [Skript](javascript:x)\n\n\
             - a\n- b\n\n1. eins\n2. zwei\n\n| A | B |\n|---|--:|\n| 1 | 2 |\n\n![Bild](a.png)\n\nNote[^1]\n\n[^1]: Fuß",
        );
        let files = entries(&zip);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        for part in [
            "[Content_Types].xml",
            "_rels/.rels",
            "word/document.xml",
            "word/styles.xml",
            "word/media/image1.png",
        ] {
            assert!(names.contains(&part), "{part} missing in {names:?}");
        }
        let document =
            String::from_utf8(files.iter().find(|(n, _)| n == "word/document.xml").unwrap().1.clone()).unwrap();
        assert!(document.contains("<w:pStyle w:val=\"Heading1\"/>"));
        assert!(document.contains("<w:b/></w:rPr><w:t xml:space=\"preserve\">fett"));
        assert!(document.contains("<w:highlight w:val=\"yellow\"/></w:rPr><w:t xml:space=\"preserve\">hell"));
        assert!(document.contains("&amp;"));
        assert!(document.contains("<w:numId w:val=\"1\"/>") && document.contains("<w:numId w:val=\"2\"/>"));
        assert!(document.contains("<w:tbl>") && document.contains("<w:jc w:val=\"right\"/>"));
        let extent = document.find("<wp:extent").map(|i| &document[i..i + 40]);
        assert!(
            document.contains("<wp:extent cx=\"5727700\""),
            "wide images shrink to the text width: {extent:?}"
        );
        assert!(document.contains("Fuß"));
        let rels = String::from_utf8(
            files
                .iter()
                .find(|(n, _)| n == "word/_rels/document.xml.rels")
                .unwrap()
                .1
                .clone(),
        )
        .unwrap();
        assert!(rels.contains("https://example.com"));
        assert!(!rels.contains("javascript"));
    }

    #[test]
    fn quote_bars_and_code_shading_end_at_their_text_and_type_is_sans_serif() {
        let files = entries(&export(
            "> Zitat\n>\n> - Punkt\n\nDanach\n\n```\ncode\n```\n\n| A |\n|---|\n| 1 |\n\n> [!NOTE]\n> Hinweis\n",
        ));
        for (name, content) in &files {
            if name.ends_with(".xml") || name.ends_with(".rels") {
                let xml = std::str::from_utf8(content).unwrap();
                svg2pdf::usvg::roxmltree::Document::parse(xml).unwrap_or_else(|e| panic!("{name}: {e}"));
            }
        }
        let part = |name: &str| String::from_utf8(files.iter().find(|(n, _)| n == name).unwrap().1.clone()).unwrap();
        let document = part("word/document.xml");
        assert!(
            document.contains("<w:pStyle w:val=\"Quote\"/><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr><w:spacing w:after=\"0\"/>"),
            "the last paragraph of a quote has no space after: {document}"
        );
        assert!(
            document.contains(
                "<w:spacing w:before=\"160\"/></w:pPr><w:r><w:rPr></w:rPr><w:t xml:space=\"preserve\">Danach"
            )
        );
        assert!(document.contains("<w:pStyle w:val=\"Code\"/><w:spacing w:after=\"0\"/>"));
        assert!(
            document.contains("<w:spacing w:after=\"0\" w:line=\"160\" w:lineRule=\"exact\"/></w:pPr></w:p><w:tbl>")
        );
        assert_eq!(
            document.matches("w:before=").count(),
            1,
            "the table and the end take no extra space"
        );
        assert!(part("word/styles.xml").contains("<w:rFonts w:ascii=\"Helvetica Neue\""));
        assert!(part("word/fontTable.xml").contains("<w:family w:val=\"swiss\"/>"));
        let types = part("[Content_Types].xml");
        let rels = part("word/_rels/document.xml.rels");
        for target in ["fontTable.xml", "settings.xml"] {
            assert!(
                types.contains(&format!("/word/{target}")) && rels.contains(target),
                "{target}"
            );
        }
    }

    #[test]
    fn extended_syntax_and_running_text_reach_the_document() {
        let files = entries(&export(
            "Er sagte \"Ja\" -- #idee, #12, [@doe] :rocket: ||geheim|| %%still%% ==hell==\n\n\
             Begriff\n\n: Erklärung\n\n```math\nx^2\n```\n",
        ));
        let document =
            String::from_utf8(files.iter().find(|(n, _)| n == "word/document.xml").unwrap().1.clone()).unwrap();
        for wanted in [
            "„Ja“ – ",
            "#idee",
            "(Doe 2020)",
            "🚀",
            "geheim",
            "hell",
            "Begriff",
            "Erklärung",
            "x^2",
        ] {
            assert!(document.contains(wanted), "{wanted} missing: {document}");
        }
        assert!(!document.contains("still"));
        assert!(document.contains("<w:color w:val=\"0066CC\"/></w:rPr><w:t xml:space=\"preserve\">#idee"));
        assert!(document.contains("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"1D1D1F\"/>"));
        assert!(document.contains("<w:b/></w:rPr><w:t xml:space=\"preserve\">Begriff"));
        assert!(document.contains("<w:ind w:left=\"567\"/>"));
        let rels = String::from_utf8(
            files
                .iter()
                .find(|(n, _)| n == "word/_rels/document.xml.rels")
                .unwrap()
                .1
                .clone(),
        )
        .unwrap();
        assert!(rels.contains("https://example.com/issues/12"));
    }

    #[test]
    fn embedded_html_becomes_word_paragraphs() {
        let files = entries(&export(
            "<p align=\"center\"><img src=\"logo.png\" width=\"200\"><br><b>Splat</b> <a href=\"https://x.de\">Web</a></p>\n\n\
             <h2>Kopf</h2>\n\n<ul><li>eins<li>zwei</ul>\n\n<table><tr><th colspan=2>A</th></tr><tr><td>1</td><td>2</td></tr></table>\n\n\
             <details>\n<summary>Mehr</summary>\n\nInhalt\n\n</details>\n",
        ));
        for (name, content) in &files {
            if name.ends_with(".xml") || name.ends_with(".rels") {
                svg2pdf::usvg::roxmltree::Document::parse(std::str::from_utf8(content).unwrap())
                    .unwrap_or_else(|e| panic!("{name}: {e}"));
            }
        }
        let document =
            String::from_utf8(files.iter().find(|(n, _)| n == "word/document.xml").unwrap().1.clone()).unwrap();
        assert!(document.contains("<w:jc w:val=\"center\"/>"), "{document}");
        assert!(
            document.contains("<wp:extent cx=\"1905000\" cy=\"635000\"/>"),
            "200 px wide at 0.75 pt: {document}"
        );
        assert!(document.contains("<w:b/></w:rPr><w:t xml:space=\"preserve\">Splat"));
        assert!(document.contains("<w:pStyle w:val=\"Heading2\"/>"));
        assert_eq!(document.matches("<w:numId w:val=\"1\"/>").count(), 2);
        assert!(document.contains("<w:gridSpan w:val=\"2\"/>") && document.contains("<w:gridCol/><w:gridCol/>"));
        assert!(document.contains("Mehr") && document.contains("Inhalt"));
    }

    #[test]
    fn embedded_html_keeps_non_breaking_spaces_in_word() {
        let files = entries(&export("<p>a&nbsp;&nbsp;&nbsp;b   \n c</p>\n"));
        let document =
            String::from_utf8(files.iter().find(|(n, _)| n == "word/document.xml").unwrap().1.clone()).unwrap();
        assert!(document.contains("a\u{a0}\u{a0}\u{a0}b c"), "{document}");
    }

    #[test]
    fn embedded_html_quotes_lists_code_and_table_parts_reach_word() {
        let files = entries(&export(
            "<div align=\"right\">\n<blockquote>Zitat <a href=\"javascript:x\">kein Link</a></blockquote>\n\
             <ol start=\"4\"><li>vier<ul><li>tief</li></ul></li></ol>\n<pre>\nlet x = 1;<br>y\n</pre>\n<hr>\n\
             <table><thead><tr><th align=\"center\">K</th><th align=\"right\">L</th></tr></thead>\
             <tbody><tr><td>1</td></tr></tbody></table>\n<table></table>\n<p>Ende</p>\n</div>\n",
        ));
        let document =
            String::from_utf8(files.iter().find(|(n, _)| n == "word/document.xml").unwrap().1.clone()).unwrap();
        svg2pdf::usvg::roxmltree::Document::parse(&document).unwrap();
        assert!(document.contains("<w:pStyle w:val=\"Quote\"/>"), "{document}");
        assert!(document.contains("kein Link") && !document.contains("javascript"));
        assert!(document.contains("<w:ilvl w:val=\"1\"/>"), "nested list: {document}");
        let numbering =
            String::from_utf8(files.iter().find(|(n, _)| n == "word/numbering.xml").unwrap().1.clone()).unwrap();
        assert!(numbering.contains("<w:startOverride w:val=\"4\"/>"), "{numbering}");
        assert!(document.contains("let x = 1;") && document.contains("w:bottom w:val=\"single\""));
        assert!(document.contains("<w:jc w:val=\"right\"/>") && document.contains("<w:jc w:val=\"center\"/>"));
        assert!(document.contains("Ende"));
    }

    #[test]
    fn spacing_goes_after_numbering_and_before_indentation() {
        assert_eq!(
            with_spacing("<w:pStyle w:val=\"Quote\"/><w:ind w:left=\"720\"/>", "w:after", 0),
            "<w:pStyle w:val=\"Quote\"/><w:spacing w:after=\"0\"/><w:ind w:left=\"720\"/>"
        );
        assert_eq!(
            with_spacing("<w:spacing w:before=\"160\"/>", "w:after", 0),
            "<w:spacing w:after=\"0\" w:before=\"160\"/>"
        );
        assert_eq!(
            with_spacing("<w:spacing w:after=\"0\"/>", "w:after", 0),
            "<w:spacing w:after=\"0\"/>"
        );
    }
}
