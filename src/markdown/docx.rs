//! The document as Word file (Office Open XML), written from the Markdown structure: headings become Word's
//! heading styles, lists real lists, tables tables. The system's own Word export drops images and tables.

mod html;
mod xml;

use std::collections::HashMap;

use super::doc::{Align, Block, BlockKind, Doc, Figure, Inline, InlineKind, ListKind, Piece, code_figure};
use super::text::{count32, right_to_left};
use super::typography::{Mark, Open, Quotes, before_text, marks, smarten};
use super::url::is_web_or_mail;
use crate::i18n::t;
use xml::{Line, ParaProps, Props, Runs, escape_xml};

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
        last: None,
        paragraphs: 0,
        media: Vec::new(),
        links: Vec::new(),
        numbers: Vec::new(),
        footnotes: Vec::new(),
        footnote_ix: HashMap::new(),
        gap: false,
        open: Open::default(),
    };
    let ctx = WordCtx::default();
    for block in &doc.blocks {
        w.block(block, &ctx);
    }
    w.write_footnotes();
    w.flush_last();
    w.package()
}

/// Where a paragraph stands in a list.
#[derive(Clone, Copy)]
enum ListPlace {
    /// It starts an item, with this numbering id and level.
    Item(usize, usize),
    /// It continues the item at this level, without a bullet of its own.
    Continued(usize),
}

#[derive(Clone, Default)]
struct WordCtx {
    quote: u8,
    list: Option<ListPlace>,
    depth: usize,
    /// The first paragraph of a task item starts with its checkbox.
    task: Option<bool>,
    /// Indentation of definition details, in list levels.
    indent: usize,
    /// Definition terms are bold.
    strong: bool,
}

struct Writer<'a> {
    export: &'a WordExport<'a>,
    body: String,
    /// The last paragraph, not yet in `body`, as the end of a framed block may still change its spacing.
    last: Option<(ParaProps, Runs)>,
    /// How many paragraphs there are so far, which tells whether `last` lies inside a block.
    paragraphs: usize,
    media: Vec<(Vec<u8>, bool)>,
    links: Vec<String>,
    /// Ordered lists, each with its own numbering so it restarts, by start number.
    numbers: Vec<usize>,
    footnotes: Vec<(u32, &'a Block)>,
    footnote_ix: HashMap<String, u32>,
    /// The next paragraph carries the space the block before it could not have after itself.
    gap: bool,
    /// Quotes the runs of the paragraph being written left open.
    open: Open,
}

impl<'a> Writer<'a> {
    fn block(&mut self, block: &'a Block, ctx: &WordCtx) {
        match &block.kind {
            BlockKind::Paragraph => self.paragraph(None, ctx, block.inlines()),
            BlockKind::Heading { level, .. } => {
                self.paragraph(Some(&format!("Heading{}", level.clamp(&1, &6))), ctx, block.inlines())
            }
            BlockKind::BlockQuote => {
                let inner = WordCtx {
                    quote: ctx.quote + 1,
                    ..ctx.clone()
                };
                let start = self.paragraphs;
                self.children(block, &inner);
                if ctx.quote == 0 {
                    self.end_framed_block(start);
                }
            }
            BlockKind::Alert { kind, title, .. } => {
                self.alert(block, ctx, title.as_deref().unwrap_or(t(kind.label())));
            }
            BlockKind::List { kind, .. } => self.list(block, kind, ctx),
            BlockKind::Item { .. } => self.item(block, ctx),
            BlockKind::CodeBlock { info, literal, .. } => {
                let figure = code_figure(info, literal);
                let picture = figure.as_ref().and_then(|f| (self.export.figure)(f));
                match (picture, figure) {
                    (Some(picture), Some(Figure::Math { tex, .. })) => self.centered_picture(picture, tex),
                    (Some(picture), _) => self.centered_picture(picture, t("Diagramm")),
                    _ => self.code(literal),
                }
            }
            BlockKind::DefinitionList => self.definition_list(block, ctx),
            BlockKind::Comment => {}
            BlockKind::HtmlBlock { literal } => self.html(literal, ctx),
            BlockKind::MathBlock { literal } => {
                let figure = Figure::Math {
                    tex: literal,
                    display: true,
                };
                match (self.export.figure)(&figure) {
                    Some(picture) => self.centered_picture(picture, literal),
                    None => self.code(&format!("$${literal}$$")),
                }
            }
            BlockKind::ThematicBreak => self.rule(Line::Bottom),
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

    fn alert(&mut self, block: &'a Block, ctx: &WordCtx, label: &str) {
        let inner = WordCtx {
            quote: ctx.quote + 1,
            ..ctx.clone()
        };
        let title = Inline {
            kind: InlineKind::Strong,
            range: 0..0,
            children: Box::new([Inline {
                kind: InlineKind::Text(label.into()),
                range: 0..0,
                children: Box::default(),
            }]),
        };
        let start = self.paragraphs;
        self.paragraph(None, &inner, std::slice::from_ref(&title));
        self.children(block, &inner);
        if ctx.quote == 0 {
            self.end_framed_block(start);
        }
    }

    fn list(&mut self, block: &'a Block, kind: &ListKind, ctx: &WordCtx) {
        let id = match kind {
            ListKind::Bullet(_) => 1,
            ListKind::Ordered { start, .. } => {
                self.numbers.push(*start);
                self.numbers.len() + 1
            }
        };
        for item in block.children() {
            let inner = WordCtx {
                list: Some(ListPlace::Item(id, ctx.depth)),
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

    fn definition_list(&mut self, block: &'a Block, ctx: &WordCtx) {
        for part in block.children().iter().flat_map(|item| item.children()) {
            let inner = match part.kind {
                BlockKind::DefinitionTerm => WordCtx {
                    strong: true,
                    ..ctx.clone()
                },
                _ => WordCtx {
                    indent: ctx.indent + 1,
                    ..ctx.clone()
                },
            };
            self.children(part, &inner);
        }
    }

    fn children(&mut self, block: &'a Block, ctx: &WordCtx) {
        for child in block.children() {
            self.block(child, ctx);
        }
    }

    fn item(&mut self, item: &'a Block, ctx: &WordCtx) {
        for (n, child) in item.children().iter().enumerate() {
            let inner = if n == 0 {
                ctx.clone()
            } else {
                // Further paragraphs of an item continue its indentation without a new bullet.
                WordCtx {
                    list: ctx.list.map(|place| match place {
                        ListPlace::Item(_, level) | ListPlace::Continued(level) => ListPlace::Continued(level),
                    }),
                    task: None,
                    ..ctx.clone()
                }
            };
            self.block(child, &inner);
        }
    }

    fn paragraph(&mut self, style: Option<&str>, ctx: &WordCtx, inlines: &[Inline]) {
        let mut para = ParaProps {
            style: style.or((ctx.quote > 0).then_some("Quote")).map(str::to_string),
            ..ParaProps::default()
        };
        match ctx.list {
            Some(ListPlace::Item(id, level)) => para.list = Some((id, level)),
            Some(ListPlace::Continued(level)) => para.indent = Some(720 * (level + 1)),
            None if ctx.indent > 0 => para.indent = Some(567 * ctx.indent),
            None => {}
        }
        let mut runs = Runs::default();
        if let Some(checked) = ctx.task {
            runs.push(if checked { "☑ " } else { "☐ " }, Props::default());
        }
        self.inlines(inlines, Props::default().bold(ctx.strong), &mut runs);
        self.push_paragraph(para, runs);
    }

    fn push_paragraph(&mut self, mut para: ParaProps, runs: Runs) {
        self.flush_last();
        if std::mem::take(&mut self.gap) {
            para.before.get_or_insert(GAP);
        }
        para.bidi |= !para.is_code() && right_to_left(runs.text()) == Some(true);
        self.open = Open::default();
        self.paragraphs += 1;
        self.last = Some((para, runs));
    }

    fn flush_last(&mut self) {
        let Some((para, runs)) = self.last.take() else { return };
        let props = para.to_xml();
        self.body.push_str("<w:p>");
        if !props.is_empty() {
            self.body.push_str(&format!("<w:pPr>{props}</w:pPr>"));
        }
        self.body.push_str(runs.xml());
        self.body.push_str("</w:p>");
    }

    /// Markup around paragraphs, as of a table.
    fn raw(&mut self, xml: &str) {
        self.flush_last();
        self.body.push_str(xml);
    }

    fn rule(&mut self, line: Line) {
        let para = ParaProps {
            line: Some(line),
            ..ParaProps::default()
        };
        self.push_paragraph(para, Runs::default());
    }

    fn centered_picture(&mut self, picture: Picture, alt: &str) {
        let mut runs = Runs::default();
        let drawing = self.picture(picture, alt);
        runs.picture(&drawing);
        let para = ParaProps {
            justify: Some(Align::Center),
            ..ParaProps::default()
        };
        self.push_paragraph(para, runs);
    }

    /// A table has no space before, so an empty paragraph of that height stands in.
    fn table_gap(&mut self) {
        if std::mem::take(&mut self.gap) {
            let para = ParaProps {
                after: Some(0),
                exact_line: Some(GAP),
                ..ParaProps::default()
            };
            self.push_paragraph(para, Runs::default());
        }
    }

    fn code(&mut self, literal: &str) {
        let start = self.paragraphs;
        for line in literal.strip_suffix('\n').unwrap_or(literal).split('\n') {
            let mut run = Runs::default();
            run.push(line, Props::default());
            self.push_paragraph(ParaProps::styled("Code"), run);
        }
        self.end_framed_block(start);
    }

    /// Word draws a quote's bar and a code block's shading through the space after their last paragraph, so that
    /// space moves to the start of what follows. `start` is the number of paragraphs before the block.
    fn end_framed_block(&mut self, start: usize) {
        if self.paragraphs == start {
            return;
        }
        if let Some((last, _)) = &mut self.last {
            last.after.get_or_insert(0);
            self.gap = true;
        }
    }

    fn table(&mut self, table: &'a Block, alignments: &[Align]) {
        self.table_gap();
        let mirrored = right_to_left(&self.export.source[table.range.clone()]) == Some(true);
        self.raw(&format!(
            "<w:tbl><w:tblPr><w:tblStyle w:val=\"TableGrid\"/>{}<w:tblW w:w=\"0\" w:type=\"auto\"/></w:tblPr><w:tblGrid>",
            if mirrored { "<w:bidiVisual/>" } else { "" }
        ));
        self.raw(&"<w:gridCol/>".repeat(alignments.len()));
        self.raw("</w:tblGrid>");
        for row in table.children() {
            let header = matches!(row.kind, BlockKind::TableRow { header: true });
            self.raw("<w:tr>");
            if header {
                self.raw("<w:trPr><w:tblHeader/></w:trPr>");
            }
            for (col, cell) in row.children().iter().enumerate() {
                self.raw(&cell_start(1, header));
                let justify = match alignments.get(col) {
                    Some(Align::Center) => Some(Align::Center),
                    Some(Align::Right) => Some(Align::Right),
                    _ => None,
                };
                let mut runs = Runs::default();
                self.inlines(cell.inlines(), Props::default().bold(header), &mut runs);
                self.push_paragraph(cell_paragraph(justify), runs);
                self.raw("</w:tc>");
            }
            self.raw("</w:tr>");
        }
        self.raw("</w:tbl>");
        // Word needs a paragraph between consecutive tables and before the end of the document.
        self.push_paragraph(ParaProps::default(), Runs::default());
    }

    fn inlines(&mut self, inlines: &[Inline], props: Props, out: &mut Runs) {
        for inline in inlines {
            match inline.piece() {
                Piece::Text(text) => self.text(text, inline.range.start, props, out),
                Piece::SoftBreak => out.push(" ", props),
                Piece::LineBreak => out.line_break(),
                Piece::Code(code) => out.push(code, props.code()),
                Piece::Html(raw) => out.push(raw, props),
                Piece::Emoji(emoji) => out.push(emoji, props),
                Piece::Styled => self.inlines(&inline.children, props.inside(&inline.kind), out),
                Piece::Hidden => {}
                Piece::WikiPage(_) => self.inlines(&inline.children, props, out),
                Piece::Picture(picture) => match (self.export.image)(picture.url) {
                    Some(found) => {
                        let found = asked_size(found, picture.width, picture.height);
                        out.picture(&self.picture(found, &picture.alt));
                    }
                    None => out.push(&picture.alt, props),
                },
                Piece::Link { url, .. } => {
                    if is_web_or_mail(url) {
                        let mut inner = Runs::default();
                        self.inlines(&inline.children, props.linked(), &mut inner);
                        out.hyperlink(self.link_id(url), inner);
                    } else {
                        self.inlines(&inline.children, props, out);
                    }
                }
                Piece::FootnoteRef { name, .. } => {
                    let ix = self.footnote_number(name);
                    out.push(&ix.to_string(), props.superscript());
                }
                Piece::Math { literal, display } => {
                    let figure = Figure::Math { tex: literal, display };
                    match (self.export.figure)(&figure) {
                        Some(found) => out.picture(&self.picture(found, literal)),
                        None => out.push(&format!("${literal}$"), props.code()),
                    }
                }
            }
        }
    }

    /// Running text, with typographic punctuation, tags, references and citations as in the preview.
    fn text(&mut self, literal: &str, at: usize, props: Props, out: &mut Runs) {
        if literal.is_empty() {
            return;
        }
        let before = before_text(self.export.source, at);
        let shown = match self.export.quotes {
            Some(q) if !props.text.code => smarten(literal, before, q, &mut self.open),
            _ => literal.to_string(),
        };
        let mut done = 0;
        for (range, mark) in marks(&shown, before, true) {
            let piece = &shown[range.clone()];
            let mut marked = Runs::default();
            match &mark {
                Mark::Tag(_) => marked.push(piece, Props { tag: true, ..props }),
                Mark::Reference(r) => {
                    let Some(url) = (self.export.reference)(r) else {
                        continue;
                    };
                    let mut linked = Runs::default();
                    linked.push(piece, props.linked());
                    marked.hyperlink(self.link_id(&url), linked);
                }
                Mark::Citation(keys) => {
                    let names: Option<Vec<String>> = keys.iter().map(|k| (self.export.citation)(k)).collect();
                    let Some(names) = names else { continue };
                    marked.push(&format!("({})", names.join("; ")), props);
                }
            }
            if range.start > done {
                out.push(&shown[done..range.start], props);
            }
            out.append(marked);
            done = range.end;
        }
        if done < shown.len() || done == 0 {
            out.push(&shown[done..], props);
        }
    }

    /// The number of the relationship that points to `url`.
    fn link_id(&mut self, url: &str) -> usize {
        self.links.push(url.to_string());
        self.links.len()
    }

    fn footnote_number(&mut self, name: &str) -> u32 {
        let next = count32(self.footnote_ix.len()) + 1;
        *self.footnote_ix.entry(name.to_string()).or_insert(next)
    }

    fn write_footnotes(&mut self) {
        if self.footnotes.is_empty() {
            return;
        }
        self.rule(Line::Top);
        let mut notes = std::mem::take(&mut self.footnotes);
        notes.sort_by_key(|(ix, _)| *ix);
        for (ix, block) in notes {
            let mut number = Some(format!("{ix}. "));
            for child in block.children() {
                let mut runs = Runs::default();
                let paragraph = matches!(child.kind, BlockKind::Paragraph);
                if let Some(number) =
                    number.take_if(|_| paragraph || !child.children().is_empty() || !child.inlines().is_empty())
                {
                    runs.push(&number, Props::default());
                }
                if paragraph {
                    self.inlines(child.inlines(), Props::default(), &mut runs);
                }
                // A list or code block can't follow a number on its line, so the number stands above it.
                if !runs.is_empty() {
                    self.push_paragraph(ParaProps::styled("FootnoteText"), runs);
                }
                if !paragraph {
                    self.block(child, &WordCtx::default());
                }
            }
        }
    }

    /// An inline picture, no wider than the text.
    fn picture(&mut self, picture: Picture, alt: &str) -> String {
        let scale = (TEXT_WIDTH / picture.width.max(1.0)).min(1.0);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "scaled to fit the page"
        )]
        let (cx, cy) = (
            (picture.width * scale * EMU_PER_POINT).round() as u64,
            (picture.height * scale * EMU_PER_POINT).round() as u64,
        );
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
            alt = escape_xml(alt)
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
                escape_xml(url)
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

/// A table cell's opening up to its paragraphs, with its width in columns.
fn cell_start(span: u32, header: bool) -> String {
    let mut out = String::from("<w:tc><w:tcPr><w:tcW w:w=\"0\" w:type=\"auto\"/>");
    if span > 1 {
        out.push_str(&format!("<w:gridSpan w:val=\"{span}\"/>"));
    }
    if header {
        out.push_str("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F2F2F4\"/>");
    }
    out.push_str("</w:tcPr>");
    out
}

fn cell_paragraph(justify: Option<Align>) -> ParaProps {
    ParaProps {
        after: Some(0),
        justify,
        ..ParaProps::default()
    }
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

// Without the 64-bit extension of the format: a document stays far below its 4 GB.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a document stays far below the 4 GB of a ZIP without extensions"
)]
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
        for (i, entry) in (0u32..).zip(t.iter_mut()) {
            let mut c = i;
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
    use crate::markdown::doc::{AlertKind, parse};

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
    fn right_to_left_paragraphs_and_tables_are_marked_for_word() {
        let zip = export(
            "# כותרת\n\n- פריט\n\nEnglish\n\n& \"שלום\" <ברכה>\n\n| שם | ערך |\n|---|---|\n| 1 | 2 |\n\n```\n# הערה\n```\n",
        );
        let files = entries(&zip);
        let document =
            String::from_utf8(files.iter().find(|(n, _)| n == "word/document.xml").unwrap().1.clone()).unwrap();
        assert!(
            document.contains("<w:pStyle w:val=\"Heading1\"/><w:bidi/>"),
            "{document}"
        );
        assert!(
            document.contains("<w:bidi/><w:ind ") || document.contains("</w:numPr><w:bidi/>"),
            "{document}"
        );
        assert!(
            document.contains("<w:tblStyle w:val=\"TableGrid\"/><w:bidiVisual/>"),
            "{document}"
        );
        assert_eq!(document.matches("<w:bidi/>").count(), 5, "{document}");
    }

    #[test]
    fn quotes_without_spaces_pair_across_runs_of_a_paragraph_only() {
        let zip = export("彼は\"今日は\n晴れ\"です\n\n\"x\"\n");
        let files = entries(&zip);
        let document =
            String::from_utf8(files.iter().find(|(n, _)| n == "word/document.xml").unwrap().1.clone()).unwrap();
        assert!(document.contains("晴れ“です") && document.contains("„x“"), "{document}");
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
    fn footnotes_keep_their_lists_and_code() {
        let files = entries(&export(
            "x[^1]\n\n[^1]: note\n\n    - item-a\n    - item-b\n\n    ```\n    code-c\n    ```\n",
        ));
        let document =
            String::from_utf8(files.iter().find(|(n, _)| n == "word/document.xml").unwrap().1.clone()).unwrap();
        for wanted in ["note", "item-a", "item-b", "code-c"] {
            assert!(document.contains(wanted), "{wanted} missing: {document}");
        }
        assert!(document.find("note").unwrap() < document.find("item-a").unwrap());
    }

    #[test]
    fn a_footnote_starting_with_a_list_shows_its_number_above_it() {
        let files = entries(&export("x[^1]\n\n[^1]:\n    - item\n"));
        let document =
            String::from_utf8(files.iter().find(|(n, _)| n == "word/document.xml").unwrap().1.clone()).unwrap();
        assert!(
            document.find(">1. <").unwrap() < document.find("item").unwrap(),
            "{document}"
        );
    }

    #[test]
    fn a_link_around_code_is_valid_word() {
        let files = entries(&export("[`code`](https://x.example) [plain](https://y.example)\n"));
        let document =
            String::from_utf8(files.iter().find(|(n, _)| n == "word/document.xml").unwrap().1.clone()).unwrap();
        for run in document.split("<w:r>").skip(1) {
            assert!(run.matches("w:rStyle").count() <= 1, "{run}");
        }
        assert!(document.contains("<w:rStyle w:val=\"Hyperlink\"/>"));
    }

    fn export_with(text: &str, figure: &dyn Fn(&Figure) -> Option<Picture>) -> Vec<u8> {
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
                citation: &|_| None,
                figure,
                image: &|_| None,
            },
        )
    }

    fn document(text: &str) -> String {
        let files = entries(&export(text));
        String::from_utf8(files.into_iter().find(|(n, _)| n == "word/document.xml").unwrap().1).unwrap()
    }

    #[test]
    fn a_figure_with_a_picture_is_centered_and_says_what_it_shows() {
        let picture = |_: &Figure| {
            Some(Picture {
                data: b"\x89PNG".to_vec(),
                jpeg: false,
                width: 10.0,
                height: 10.0,
            })
        };
        let word = |text: &str| {
            let files = entries(&export_with(text, &picture));
            String::from_utf8(files.into_iter().find(|(n, _)| n == "word/document.xml").unwrap().1).unwrap()
        };
        let diagram = word("```mermaid\nabcd\n```\n");
        assert!(diagram.contains(&format!("descr=\"{}\"", t("Diagramm"))), "{diagram}");
        assert!(diagram.contains("<w:jc w:val=\"center\"/>"), "{diagram}");
        assert!(!diagram.contains("abcd"));
        let formula = word("```math\nx^2\n```\n");
        assert!(formula.contains("descr=\"x^2\""), "{formula}");
    }

    #[test]
    fn an_alert_is_a_bold_title_over_its_text_in_a_quote() {
        let word = document("> [!NOTE]\n> Inhalt\n\nDanach\n");
        let label = format!(
            "<w:b/></w:rPr><w:t xml:space=\"preserve\">{}",
            t(AlertKind::Note.label())
        );
        assert!(word.contains(&label), "{word}");
        assert_eq!(word.matches("<w:pStyle w:val=\"Quote\"/>").count(), 2, "{word}");
        assert!(
            word.contains(
                "<w:spacing w:before=\"160\"/></w:pPr><w:r><w:rPr></w:rPr><w:t xml:space=\"preserve\">Danach"
            ),
            "{word}"
        );
    }

    #[test]
    fn only_a_quote_makes_a_paragraph_a_quote() {
        assert!(!document("a\n").contains("Quote"));
    }

    #[test]
    fn lists_keep_their_depth_tasks_and_further_paragraphs() {
        let word = document("- a\n  - b\n\n    c\n");
        assert!(
            word.contains("<w:ilvl w:val=\"0\"/>") && word.contains("<w:ilvl w:val=\"1\"/>"),
            "{word}"
        );
        assert!(word.contains("<w:ind w:left=\"1440\"/>"), "{word}");
        let word = document("- [x] a\n\n  b\n\n- [ ] c\n");
        assert!(word.contains("☑ ") && word.contains("☐ "), "{word}");
        assert_eq!(
            word.matches("☑ ").count(),
            1,
            "a further paragraph has no box of its own"
        );
        assert_eq!(word.matches("<w:ilvl").count(), 2, "and no bullet: {word}");
    }

    #[test]
    fn a_definition_inside_a_definition_goes_further_in() {
        let word = document("A\n\n: B\n\n  C\n\n  : D\n");
        assert!(word.contains("<w:ind w:left=\"1134\"/>"), "{word}");
    }

    #[test]
    fn a_centered_table_column_is_centered() {
        assert!(document("| a |\n|:-:|\n| 1 |\n").contains("<w:jc w:val=\"center\"/>"));
    }

    #[test]
    fn a_keyboard_key_keeps_its_straight_quotes() {
        let word = document("<kbd>\"a\"</kbd>\n");
        assert!(!word.contains("„a“"), "{word}");
    }

    #[test]
    fn code_keeps_its_straight_quotes() {
        let word = document("a `\"b\"` \"c\"\n");
        assert!(word.contains("\"b\"") || word.contains("&quot;b&quot;"), "{word}");
        assert!(word.contains("„c“"), "{word}");
    }

    #[test]
    fn text_around_a_mark_stays_in_its_order_without_empty_runs() {
        let word = document("#12 und mehr\n");
        assert!(word.contains(">#12<"), "{word}");
        assert!(word.contains("> und mehr<"), "{word}");
        assert!(!word.contains("preserve\"></w:t>"), "{word}");
        let word = document("nur #12\n");
        assert!(word.contains(">nur <") && !word.contains("preserve\"></w:t>"), "{word}");
    }

    #[test]
    fn footnotes_count_up_and_a_second_reference_keeps_the_number() {
        let word = document("a[^x] b[^y] c[^x]\n\n[^x]: X\n\n[^y]: Y\n");
        assert!(word.contains("1. ") && word.contains("2. "), "{word}");
        assert!(!word.contains("3. "), "{word}");
    }

    #[test]
    fn a_footnote_of_a_heading_shows_its_number() {
        let word = document("a[^x]\n\n[^x]: # Kopf\n");
        assert!(word.contains("1. "), "{word}");
    }

    #[test]
    fn a_wide_picture_shrinks_in_both_directions() {
        let word = document("![a](a.png)\n");
        assert!(word.contains("cy=\"1909233\""), "{word}");
    }

    #[test]
    fn a_picture_takes_the_size_asked_for_and_keeps_its_shape() {
        let size = |width, height| {
            let p = asked_size(
                Picture {
                    data: Vec::new(),
                    jpeg: false,
                    width: 900.0,
                    height: 300.0,
                },
                width,
                height,
            );
            (p.width, p.height)
        };
        assert_eq!(size(Some(200), None), (150.0, 50.0));
        assert_eq!(size(None, Some(100)), (225.0, 75.0));
        assert_eq!(size(Some(200), Some(100)), (150.0, 75.0));
        assert_eq!(size(None, None), (900.0, 300.0));
        for (width, height) in [(0.0, 300.0), (900.0, 0.0)] {
            let p = asked_size(
                Picture {
                    data: Vec::new(),
                    jpeg: false,
                    width,
                    height,
                },
                Some(10),
                Some(10),
            );
            assert_eq!((p.width, p.height), (width, height), "no size to keep the shape of");
        }
    }

    #[test]
    fn a_cell_spans_columns_only_when_asked_and_has_no_space_after() {
        assert!(!cell_start(1, false).contains("gridSpan"));
        assert!(cell_start(2, false).contains("<w:gridSpan w:val=\"2\"/>"));
        assert_eq!(cell_paragraph(None).after, Some(0));
    }

    #[test]
    fn bullets_change_with_the_depth_and_start_over() {
        let levels = numbering(&[]);
        for (level, bullet) in [(0, "•"), (1, "◦"), (2, "▪"), (3, "•"), (4, "◦")] {
            let wanted = format!(
                "<w:lvl w:ilvl=\"{level}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"{bullet}\"/>"
            );
            assert!(levels.contains(&wanted), "{wanted} in {levels}");
        }
    }

    #[test]
    fn the_end_of_the_zip_counts_its_entries() {
        let zip = export("a\n");
        let end = zip.len() - 22;
        assert_eq!(&zip[end..end + 4], &0x0605_4b50u32.to_le_bytes());
        let count = u16::from_le_bytes([zip[end + 10], zip[end + 11]]);
        assert_eq!(usize::from(count), entries(&zip).len());
    }

    #[test]
    fn embedded_html_in_a_quote_stays_a_quote() {
        assert!(document("> <div>a</div>\n").contains("<w:pStyle w:val=\"Quote\"/>"));
        assert!(!document("<div>a</div>\n").contains("Quote"));
    }

    #[test]
    fn embedded_html_collapses_spaces_like_a_browser() {
        let word = document("<p> a</p>\n");
        assert!(word.contains(">a<"), "nothing before the first word: {word}");
        let word = document("<p>a <b>b</b></p>\n");
        assert!(word.contains(">a <"), "a space before the next element: {word}");
        let word = document("<p><b>a</b> <i>b</i></p>\n");
        assert!(word.contains("> <"), "one space between two elements: {word}");
        assert!(!word.contains(">  <"), "{word}");
        assert!(
            !document("<div>\n</div>\n").contains("preserve"),
            "a tag on its own line adds nothing"
        );
    }

    #[test]
    fn embedded_breaks_links_and_code_keep_their_meaning() {
        assert!(document("<p>a<br>b</p>\n").contains("<w:br/>"));
        let link = document("<p><a href=\"https://x.de\">l</a></p>\n");
        assert!(link.contains("<w:hyperlink") && link.contains("Hyperlink"), "{link}");
        let script = document("<p><a href=\"javascript:x\">l</a></p>\n");
        assert!(!script.contains("<w:hyperlink") && script.contains(">l<"), "{script}");
        assert!(document("<pre>a\n b</pre>\n").contains("<w:pStyle w:val=\"Code\"/>"));
        assert!(document("<ol><li>a</li></ol>\n").contains("<w:numId w:val=\"2\"/>"));
    }

    #[test]
    fn embedded_alignment_reaches_headings_and_blocks() {
        assert!(document("<h1 align=\"center\">T</h1>\n").contains("<w:jc w:val=\"center\"/>"));
        assert!(document("<center>a</center>\n").contains("<w:jc w:val=\"center\"/>"));
        assert!(!document("<div>a</div>\n").contains("<w:jc"));
    }

    #[test]
    fn embedded_summaries_and_terms_are_bold_and_other_blocks_are_not() {
        let bold = "<w:b/></w:rPr><w:t xml:space=\"preserve\">a";
        assert!(document("<details><summary>a</summary></details>\n").contains(bold));
        assert!(document("<dl><dt>a</dt></dl>\n").contains(bold));
        assert!(document("<b><div>a</div></b>\n").contains(bold), "it stays bold inside");
        assert!(!document("<div>a</div>\n").contains(bold));
    }

    #[test]
    fn only_head_cells_of_an_embedded_table_are_shaded() {
        let shaded = |html: &str| document(html).matches("w:fill=\"F2F2F4\"").count();
        assert_eq!(shaded("<table><tr><th>a</th><td>b</td><td>c</td></tr></table>\n"), 1);
        assert_eq!(
            shaded("<table><thead><tr><td>a</td><td>b</td></tr></thead></table>\n"),
            2
        );
    }

    #[test]
    fn a_missing_picture_leaves_its_text_wherever_it_is_written() {
        assert!(document("![Bild](fehlt.png)\n").contains("Bild"));
        assert!(document("Text <img src=\"fehlt.png\" alt=\"Bild\"> Text\n").contains("Bild"));
        assert!(document("<div><img src=\"fehlt.png\" alt=\"Bild\"></div>\n").contains("Bild"));
    }

    #[test]
    fn text_has_one_place_on_its_line() {
        let word = document("<sup><sub>x</sub></sup>\n");
        assert_eq!(word.matches("<w:vertAlign").count(), 1, "{word}");
    }

    #[test]
    fn a_list_stays_within_what_word_knows() {
        let deep: String = (0..12).map(|i| format!("{}- x\n", "  ".repeat(i))).collect();
        assert!(!document(&deep).contains("<w:ilvl w:val=\"9\"/>"));
        let files = entries(&export("<ol start=\"99999999999\"><li>x</li></ol>\n"));
        let numbering =
            String::from_utf8(files.into_iter().find(|(n, _)| n == "word/numbering.xml").unwrap().1).unwrap();
        assert!(!numbering.contains("99999999999"), "{numbering}");
    }

    #[test]
    fn a_paragraph_in_an_aligned_cell_takes_its_alignment() {
        let word = document("<table><tr><td align=\"right\"><p>a</p></td></tr></table>\n");
        let at = word.find(">a<").unwrap();
        assert!(word[..at].contains("<w:jc w:val=\"right\"/>"), "{word}");
    }

    #[test]
    fn a_footnote_reference_stands_raised_and_a_line_break_stays() {
        assert!(document("a[^x]\n\n[^x]: X\n").contains("<w:vertAlign w:val=\"superscript\"/>"));
        assert!(document("a  \nb\n").contains("<w:br/>"));
    }
}
