use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{
    NSColor, NSFont, NSFontManager, NSFontTraitMask, NSFontWeightRegular, NSImage, NSImageSymbolConfiguration,
    NSMutableParagraphStyle, NSTextAlignment, NSTextAttachment, NSTextBlock, NSTextBlockLayer, NSTextBlockValueType,
    NSTextTab, NSTextTable, NSTextTableBlock, NSTextTableLayoutAlgorithm, NSWritingDirection,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSDictionary, NSMutableAttributedString, NSPoint, NSRect, NSRectEdge, NSSize, NSURL,
};

use super::attributes::{Attribute, Attributes, add_attribute, attributed, attributed_mut};
use super::images::{ImageStore, Rendered, Sizing};
use super::style::{PREVIEW_SIZE, token_color};
use super::{js, remote};
use crate::markdown::code;
use crate::markdown::doc::AlertKind;
use crate::markdown::doc::Align;
use crate::markdown::style::Baseline;
use crate::markdown::text::Utf16Map;
use crate::preview::layout::{Atom, Link, Marker, Para, ParaKind, ParaStyle, Run, RunMap, TopBlock};
use crate::util::byte_range_to_ns;
use crate::util::constant;
use crate::util::ns;

pub struct RenderCtx<'a> {
    pub mtm: MainThreadMarker,
    pub zoom: f64,
    pub doc_dir: Option<&'a Path>,
    pub images: &'a RefCell<ImageStore>,
    pub max_image_width: f64,
    pub dark: bool,
    /// Receives a re-render once a remote image has loaded; print rendering passes `None`.
    pub controller: Option<&'a super::EditorController>,
    /// Images from the web load only once the user allowed it for the document.
    pub remote_images: bool,
}

struct TextBlocks {
    groups: HashMap<usize, Retained<NSTextBlock>>,
    tables: HashMap<usize, Retained<NSTextTable>>,
}

const LIST_INDENT: f64 = 26.0;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct FontKey {
    kind: u8,
    size: u64,
    code: bool,
    script: bool,
    bold: bool,
    italic: bool,
}

thread_local! {
    static FONTS: RefCell<HashMap<FontKey, Retained<NSFont>>> = RefCell::new(HashMap::new());
}

/// Standalone text blocks have zero content width unless one is set explicitly.
fn full_width_block() -> Retained<NSTextBlock> {
    let b = NSTextBlock::new();
    b.setContentWidth_type(100.0, NSTextBlockValueType::PercentageValueType);
    b
}
const QUOTE_INDENT: f64 = 14.0;

impl RenderCtx<'_> {
    fn size(&self) -> f64 {
        PREVIEW_SIZE * self.zoom
    }

    fn base_font(&self, kind: &ParaKind) -> Retained<NSFont> {
        let s = self.size();
        match kind {
            ParaKind::Heading(level) => {
                let factor = [1.75, 1.45, 1.2, 1.05, 1.0, 0.9][(*level as usize).clamp(1, 6) - 1];
                NSFont::boldSystemFontOfSize(s * factor)
            }
            ParaKind::Code { .. } => {
                NSFont::monospacedSystemFontOfSize_weight(s * 0.87, constant!(NSFontWeightRegular))
            }
            ParaKind::FootnoteDef | ParaKind::FrontMatter => NSFont::systemFontOfSize(s * 0.87),
            ParaKind::AlertTitle => NSFont::boldSystemFontOfSize(s),
            _ => NSFont::systemFontOfSize(s),
        }
    }

    pub fn render_block(&self, block: &TopBlock) -> Retained<NSMutableAttributedString> {
        let out = NSMutableAttributedString::new();
        let mut blocks = TextBlocks {
            groups: HashMap::new(),
            tables: HashMap::new(),
        };
        for (n, para) in block.paras.iter().enumerate() {
            let next_same_group = block
                .paras
                .get(n + 1)
                .is_some_and(|p| p.style.group == para.style.group && para.style.group != 0);
            let style = self.paragraph_style(para, &mut blocks, next_same_group);
            for run in &para.runs {
                out.appendAttributedString(&self.run(run, para, &style));
            }
            // Every paragraph ends with a newline, also the last one: an unterminated empty table cell
            // carries no text block and drops out of the table.
            let mut nl = vec![
                Attribute::paragraph(&style),
                Attribute::font(&self.base_font(&para.style.kind)),
            ];
            // A newline shows no color, but with one it joins the run before it, and every run slows down edits of the text.
            if let Some(last) = para.runs.last() {
                nl.push(Attribute::foreground(&self.color_for(last, para)));
            }
            out.appendAttributedString(&attributed("\n", Some(&Attributes::new(&nl))));
        }
        out
    }

    /// Appends the block's text with exactly the length `render_block` produces, so offsets stay valid until the
    /// real rendering replaces it.
    pub fn placeholder_text(block: &TopBlock, out: &mut String) {
        for para in &block.paras {
            for run in &para.runs {
                out.push_str(&run.text);
            }
            out.push('\n');
        }
    }

    pub fn placeholder(&self, text: &str) -> Retained<NSMutableAttributedString> {
        let font = NSFont::systemFontOfSize(self.size());
        let color = NSColor::labelColor();
        attributed_mut(
            text,
            &Attributes::new(&[Attribute::font(&font), Attribute::foreground(&color)]),
        )
    }

    fn paragraph_style(
        &self,
        para: &Para,
        blocks: &mut TextBlocks,
        continues_group: bool,
    ) -> Retained<NSMutableParagraphStyle> {
        let s = self.size();
        let style = &para.style;
        let (st, head, list_indent) = base_paragraph_style(style, s);
        match &style.kind {
            ParaKind::Heading(level) => heading_style(&st, s, *level),
            ParaKind::Code { .. } => {
                let b = blocks.groups.entry(style.group).or_insert_with(|| code_block_frame(s));
                st.setTextBlocks(&NSArray::from_slice(&[&**b]));
                st.setParagraphSpacing(0.0);
                st.setLineHeightMultiple(1.05);
                st.setHeadIndent(head);
                st.setFirstLineHeadIndent(head);
            }
            ParaKind::TableCell {
                table,
                row,
                col,
                header,
                align,
                columns,
                span,
            } => {
                let t = blocks
                    .tables
                    .entry(*table as usize)
                    .or_insert_with(|| text_table(s, *columns as usize));
                let span = (*span).max(1);
                let column = if style.mirrored {
                    columns.saturating_sub(col + span)
                } else {
                    *col
                };
                let cell = table_cell(
                    t,
                    s,
                    (
                        crate::util::to_isize(*row as usize),
                        crate::util::to_isize(column as usize),
                        crate::util::to_isize(span as usize),
                    ),
                    *header,
                );
                st.setTextBlocks(&NSArray::from_slice(&[&*cell as &NSTextBlock]));
                st.setAlignment(match align {
                    Align::Left | Align::None => NSTextAlignment::Natural,
                    Align::Center => NSTextAlignment::Center,
                    Align::Right => NSTextAlignment::Right,
                });
                st.setParagraphSpacing(0.0);
                st.setHeadIndent(0.0);
                st.setFirstLineHeadIndent(0.0);
            }
            ParaKind::Rule => {
                let b = rule_frame(s);
                st.setTextBlocks(&NSArray::from_slice(&[&*b]));
                st.setLineHeightMultiple(0.5);
            }
            ParaKind::MathBlock | ParaKind::Diagram => {
                st.setAlignment(NSTextAlignment::Center);
            }
            _ => {}
        }
        if !matches!(style.kind, ParaKind::TableCell { .. }) {
            match style.align {
                Align::Center => st.setAlignment(NSTextAlignment::Center),
                Align::Right => st.setAlignment(NSTextAlignment::Right),
                Align::Left => st.setAlignment(NSTextAlignment::Left),
                Align::None => {}
            }
        }
        if style.quote_depth > 0 && !matches!(style.kind, ParaKind::Code { .. } | ParaKind::TableCell { .. }) {
            let b = blocks
                .groups
                .entry(style.group)
                .or_insert_with(|| quote_frame(style, s));
            st.setTextBlocks(&NSArray::from_slice(&[&**b]));
            st.setHeadIndent(list_indent);
            st.setFirstLineHeadIndent(
                if style.marker.is_some() {
                    list_indent - LIST_INDENT * 0.85
                } else {
                    list_indent
                }
                .max(0.0),
            );
            if continues_group {
                st.setParagraphSpacing(s * 0.5);
            } else {
                st.setParagraphSpacing(0.0);
            }
        }
        st
    }

    fn font_for(&self, run: &Run, para: &Para) -> Retained<NSFont> {
        let key = FontKey {
            kind: match &para.style.kind {
                ParaKind::Heading(level) => *level,
                ParaKind::Code { .. } => 10,
                ParaKind::FootnoteDef | ParaKind::FrontMatter => 11,
                ParaKind::AlertTitle => 12,
                _ => 0,
            },
            size: self.size().to_bits(),
            code: run.style.text.code || run.style.html,
            script: run.style.text.baseline != Baseline::Normal,
            bold: run.style.text.bold,
            italic: run.style.text.italic,
        };
        // Converting fonts through NSFontManager for every run made rendering a large preview take minutes.
        if let Some(font) = FONTS.with(|f| f.borrow().get(&key).cloned()) {
            return font;
        }
        let font = self.make_font(run, para);
        FONTS.with(|f| f.borrow_mut().insert(key, font.clone()));
        font
    }

    fn make_font(&self, run: &Run, para: &Para) -> Retained<NSFont> {
        let mut font = if run.style.text.code || run.style.html {
            NSFont::monospacedSystemFontOfSize_weight(
                self.base_font(&para.style.kind).pointSize() * 0.9,
                constant!(NSFontWeightRegular),
            )
        } else {
            self.base_font(&para.style.kind)
        };
        if run.style.text.baseline != Baseline::Normal {
            font = NSFont::fontWithDescriptor_size(&font.fontDescriptor(), font.pointSize() * 0.72).unwrap_or(font);
        }
        let manager = NSFontManager::sharedFontManager(self.mtm);
        if run.style.text.bold {
            font = manager.convertFont_toHaveTrait(&font, NSFontTraitMask::BoldFontMask);
        }
        if run.style.text.italic {
            font = manager.convertFont_toHaveTrait(&font, NSFontTraitMask::ItalicFontMask);
        }
        font
    }

    fn color_for(&self, run: &Run, para: &Para) -> Retained<NSColor> {
        if run.style.text.spoiler {
            return spoiler_color();
        }
        if matches!(run.style.link, Some(Link::Tag(_))) {
            return NSColor::controlAccentColor();
        }
        if matches!(run.style.link, Some(Link::Reference(_))) {
            return NSColor::linkColor();
        }
        if run.style.citation {
            return NSColor::systemBrownColor();
        }
        if matches!(run.map, RunMap::Atom { atom: Atom::Marker, .. }) {
            return NSColor::secondaryLabelColor();
        }
        if run.style.html || matches!(para.style.kind, ParaKind::FrontMatter) {
            return NSColor::secondaryLabelColor();
        }
        if let Some(kind) = para.style.alert {
            return if matches!(para.style.kind, ParaKind::AlertTitle) {
                alert_color(kind)
            } else {
                NSColor::labelColor()
            };
        }
        if para.style.quote_depth > 0 || matches!(para.style.kind, ParaKind::FootnoteDef) {
            return NSColor::secondaryLabelColor();
        }
        NSColor::labelColor()
    }

    fn run(&self, run: &Run, para: &Para, paragraph: &NSMutableParagraphStyle) -> Retained<NSAttributedString> {
        let font = self.font_for(run, para);
        let mut attributes = vec![
            Attribute::font(&font),
            Attribute::foreground(&self.color_for(run, para)),
            Attribute::paragraph(paragraph),
        ];
        if run.style.text.strike {
            attributes.push(Attribute::strikethrough());
        }
        if run.style.text.underline {
            attributes.push(Attribute::underline());
        }
        if matches!(run.style.link, Some(Link::Tag(_))) {
            attributes.push(Attribute::background(
                &NSColor::controlAccentColor().colorWithAlphaComponent(0.12),
            ));
            attributes.push(Attribute::tool_tip(crate::i18n::t("⌘-Klick sucht den Tag im Dokument")));
        }
        if matches!(run.style.link, Some(Link::Reference(_))) {
            attributes.push(Attribute::tool_tip(crate::i18n::t("⌘-Klick öffnet den Verweis")));
        }
        if run.style.text.spoiler {
            attributes.push(Attribute::background(&spoiler_color()));
            attributes.push(Attribute::tool_tip(crate::i18n::t(
                "Verborgener Text: Zum Lesen markieren",
            )));
        } else if run.style.text.highlight {
            attributes.push(Attribute::background(&highlight_background()));
        } else if run.style.text.code && !matches!(para.style.kind, ParaKind::Code { .. }) {
            attributes.push(Attribute::background(&code_background()));
        }
        if run.style.text.baseline != Baseline::Normal {
            let offset = font.pointSize()
                * if run.style.text.baseline == Baseline::Super {
                    0.55
                } else {
                    -0.25
                };
            attributes.push(Attribute::baseline_offset(offset));
        }
        if let Some(link) = &run.style.link {
            let attribute = link.attribute();
            let url = NSURL::URLWithString(&ns(&attribute)).filter(|_| !attribute.starts_with("splat-"));
            attributes.push(url.map_or_else(|| Attribute::link_text(&attribute), |url| Attribute::link(&url)));
            if let Link::Address(address) = link {
                attributes.push(Attribute::underline());
                attributes.push(Attribute::tool_tip(&format!(
                    "{address}\n{}",
                    crate::i18n::t("⌘-Klick öffnet den Link")
                )));
            }
        }
        if let RunMap::Atom { atom: Atom::Marker, .. } = &run.map
            && let Some(Marker::Task { .. }) = &para.style.marker
        {
            attributes.push(Attribute::link_text(&Link::Task.attribute()));
        }
        let attachment = match &run.map {
            RunMap::Atom { atom, .. } => self.attachment(atom, &font),
            RunMap::Text { .. } => None,
        };
        if let Some(attachment) = attachment {
            attributes.push(Attribute::attachment(&attachment));
        }
        let out = attributed_mut(&run.text, &Attributes::new(&attributes));
        if let (ParaKind::Code { info }, RunMap::Text { .. }) = (&para.style.kind, &run.map) {
            let map = Utf16Map::new(&run.text);
            for (range, token) in code::tokens(info, &run.text) {
                add_attribute(
                    &out,
                    &Attribute::foreground(&token_color(token.into())),
                    byte_range_to_ns(&map, &range),
                );
            }
        }
        Retained::into_super(out)
    }

    fn attachment(&self, atom: &Atom, font: &NSFont) -> Option<Retained<NSTextAttachment>> {
        let rendered = match atom {
            Atom::Image(image) => {
                let (url, alt) = (&image.url, &image.alt);
                let found = if remote::is_remote(url) {
                    self.remote_images
                        .then(|| remote::get(url, self.controller))
                        .flatten()
                        .flatten()
                        .map(|r| self.images.borrow_mut().for_appearance(url, r, self.dark))
                } else {
                    self.images
                        .borrow_mut()
                        .file(url, self.doc_dir, self.dark, self.controller)
                };
                let r = found
                    .map(|r| sized(r, image.width, image.height))
                    .map(|r| zoomed(r, self.zoom))
                    .or_else(|| placeholder("photo", font))?;
                r.image
                    .setAccessibilityDescription(Some(&ns(if alt.is_empty() { url } else { alt })));
                r
            }
            Atom::Math { tex, display } => js::cached(&js::Key::math(tex, *display, self.dark))?,
            Atom::Diagram { source, kind } => js::cached(&js::Key::diagram(source, *kind, self.dark))?,
            Atom::AlertIcon(kind) => {
                let r = placeholder(alert_symbol(*kind), font)?;
                let tint = NSImageSymbolConfiguration::configurationWithHierarchicalColor(&alert_color(*kind));
                let image = r.image.imageWithSymbolConfiguration(&tint)?;
                image.setAccessibilityDescription(Some(&ns(crate::i18n::t(kind.label()))));
                Rendered { image, ..r }
            }
            _ => return None,
        };
        let (mut size, descent) = match rendered.sizing {
            Sizing::Points { size, descent } => (size, descent),
            Sizing::Ex { width, height, valign } => {
                let x = font.xHeight();
                (NSSize::new(width * x, height * x), -valign * x)
            }
        };
        if size.width > self.max_image_width && size.width > 0.0 {
            let scale = self.max_image_width / size.width;
            size = NSSize::new(size.width * scale, size.height * scale);
        }
        let att = NSTextAttachment::new();
        att.setImage(Some(&rendered.image));
        att.setBounds(NSRect::new(NSPoint::new(0.0, -descent), size));
        Some(att)
    }
}

/// The size embedded HTML asks for: one side keeps the picture's proportions, both stretch it.
fn sized(r: Rendered, width: Option<u32>, height: Option<u32>) -> Rendered {
    let Sizing::Points { size, descent } = r.sizing else {
        return r;
    };
    if size.width <= 0.0 || size.height <= 0.0 {
        return r;
    }
    let ratio = size.height / size.width;
    let size = match (width.map(f64::from), height.map(f64::from)) {
        (Some(w), Some(h)) => NSSize::new(w, h),
        (Some(w), None) => NSSize::new(w, w * ratio),
        (None, Some(h)) => NSSize::new(h / ratio, h),
        (None, None) => size,
    };
    Rendered {
        sizing: Sizing::Points { size, descent },
        ..r
    }
}

/// An image pixel is one point next to text at 100 %, as in a browser, so the image follows the text's zoom.
fn zoomed(r: Rendered, zoom: f64) -> Rendered {
    match r.sizing {
        Sizing::Points { size, descent } => Rendered {
            sizing: Sizing::Points {
                size: NSSize::new(size.width * zoom, size.height * zoom),
                descent: descent * zoom,
            },
            ..r
        },
        Sizing::Ex { .. } => r,
    }
}

fn placeholder(symbol: &str, font: &NSFont) -> Option<Rendered> {
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(symbol), None)?;
    let h = font.pointSize();
    let size = image.size();
    let scale = if size.height > 0.0 { h / size.height } else { 1.0 };
    Some(Rendered {
        image,
        sizing: Sizing::Points {
            size: NSSize::new(size.width * scale, h),
            descent: font.descender().abs() * 0.5,
        },
    })
}

pub fn code_background() -> Retained<NSColor> {
    NSColor::quaternarySystemFillColor()
}

/// Text and background of a spoiler share one color, so it reads only when selected.
fn spoiler_color() -> Retained<NSColor> {
    NSColor::secondaryLabelColor()
}

fn highlight_background() -> Retained<NSColor> {
    NSColor::systemYellowColor().colorWithAlphaComponent(0.4)
}

/// GitHub's alert colors, as system colors that follow the appearance.
pub fn alert_color(kind: AlertKind) -> Retained<NSColor> {
    match kind {
        AlertKind::Note => NSColor::systemBlueColor(),
        AlertKind::Tip => NSColor::systemGreenColor(),
        AlertKind::Important => NSColor::systemPurpleColor(),
        AlertKind::Warning => NSColor::systemOrangeColor(),
        AlertKind::Caution => NSColor::systemRedColor(),
    }
}

/// What every kind of paragraph starts from, with the head and the list indentation it uses.
fn base_paragraph_style(style: &ParaStyle, s: f64) -> (Retained<NSMutableParagraphStyle>, f64, f64) {
    let st = NSMutableParagraphStyle::new();
    // TextKit 1 aligns a paragraph without an explicit direction by the interface language, not by its text.
    st.setBaseWritingDirection(match style.rtl {
        Some(true) => NSWritingDirection::RightToLeft,
        Some(false) => NSWritingDirection::LeftToRight,
        None => NSWritingDirection::Natural,
    });
    let list_indent = style.list_depth as f64 * LIST_INDENT;
    let quote_indent = if matches!(style.kind, ParaKind::Code { .. } | ParaKind::TableCell { .. }) {
        0.0
    } else {
        style.quote_depth as f64 * QUOTE_INDENT
    };
    let head = list_indent + quote_indent;
    st.setHeadIndent(head);
    st.setFirstLineHeadIndent(if style.marker.is_some() {
        head - LIST_INDENT * 0.85
    } else {
        head
    });
    if style.marker.is_some() {
        // SAFETY: Empty options, which NSTextTab accepts.
        let tab = unsafe {
            NSTextTab::initWithTextAlignment_location_options(
                NSTextTab::alloc(),
                NSTextAlignment::Natural,
                head,
                &NSDictionary::new(),
            )
        };
        st.setTabStops(Some(&NSArray::from_slice(&[&*tab])));
    }
    st.setLineHeightMultiple(1.15);
    let spacing = if style.tight && style.list_depth > 0 {
        s * 0.25
    } else {
        s * 0.7
    };
    st.setParagraphSpacing(spacing);
    if style.space_before {
        st.setParagraphSpacingBefore(s * 0.9);
    }
    (st, head, list_indent)
}

/// Sets a width in points on one edge of a block.
fn set_edge(block: &NSTextBlock, width: f64, layer: NSTextBlockLayer, edge: NSRectEdge) {
    block.setWidth_type_forLayer_edge(width, NSTextBlockValueType::AbsoluteValueType, layer, edge);
}

fn heading_style(st: &NSMutableParagraphStyle, s: f64, level: u8) {
    st.setParagraphSpacingBefore(s * if level <= 2 { 1.1 } else { 0.8 });
    st.setParagraphSpacing(s * 0.45);
    if level <= 2 {
        let b = full_width_block();
        set_edge(&b, 1.0, NSTextBlockLayer::Border, NSRectEdge::MaxY);
        b.setBorderColor_forEdge(Some(&NSColor::separatorColor()), NSRectEdge::MaxY);
        set_edge(&b, s * 0.3, NSTextBlockLayer::Padding, NSRectEdge::MaxY);
        set_edge(&b, s * 0.5, NSTextBlockLayer::Margin, NSRectEdge::MaxY);
        st.setTextBlocks(&NSArray::from_slice(&[&*b]));
    }
}

fn code_block_frame(s: f64) -> Retained<NSTextBlock> {
    let b = full_width_block();
    b.setBackgroundColor(Some(&code_background()));
    b.setWidth_type_forLayer(
        s * 0.6,
        NSTextBlockValueType::AbsoluteValueType,
        NSTextBlockLayer::Padding,
    );
    set_edge(&b, s * 0.7, NSTextBlockLayer::Margin, NSRectEdge::MaxY);
    b
}

fn rule_frame(s: f64) -> Retained<NSTextBlock> {
    let b = full_width_block();
    set_edge(&b, 1.0, NSTextBlockLayer::Border, NSRectEdge::MaxY);
    b.setBorderColor_forEdge(Some(&NSColor::separatorColor()), NSRectEdge::MaxY);
    set_edge(&b, s * 0.5, NSTextBlockLayer::Margin, NSRectEdge::MaxY);
    b
}

/// The bar beside a quote or callout, on the side its text starts.
fn quote_frame(style: &ParaStyle, s: f64) -> Retained<NSTextBlock> {
    let b = full_width_block();
    let edge = if style.mirrored {
        NSRectEdge::MaxX
    } else {
        NSRectEdge::MinX
    };
    set_edge(&b, 3.0, NSTextBlockLayer::Border, edge);
    let border = style.alert.map_or_else(NSColor::tertiaryLabelColor, alert_color);
    b.setBorderColor_forEdge(Some(&border), edge);
    set_edge(&b, s * 0.7, NSTextBlockLayer::Padding, edge);
    set_edge(&b, s * 0.7, NSTextBlockLayer::Margin, NSRectEdge::MaxY);
    b
}

fn text_table(s: f64, columns: usize) -> Retained<NSTextTable> {
    let t = NSTextTable::new();
    t.setContentWidth_type(100.0, NSTextBlockValueType::PercentageValueType);
    t.setNumberOfColumns(columns);
    t.setLayoutAlgorithm(NSTextTableLayoutAlgorithm::AutomaticLayoutAlgorithm);
    t.setCollapsesBorders(true);
    t.setHidesEmptyCells(false);
    set_edge(&t, s * 0.7, NSTextBlockLayer::Margin, NSRectEdge::MaxY);
    t
}

/// A cell at (row, column) spanning some columns.
fn table_cell(
    table: &NSTextTable,
    s: f64,
    (row, column, span): (isize, isize, isize),
    header: bool,
) -> Retained<NSTextTableBlock> {
    let cell = NSTextTableBlock::initWithTable_startingRow_rowSpan_startingColumn_columnSpan(
        NSTextTableBlock::alloc(),
        table,
        row,
        1,
        column,
        span,
    );
    cell.setWidth_type_forLayer(1.0, NSTextBlockValueType::AbsoluteValueType, NSTextBlockLayer::Border);
    cell.setBorderColor(Some(&NSColor::separatorColor()));
    cell.setWidth_type_forLayer(
        s * 0.4,
        NSTextBlockValueType::AbsoluteValueType,
        NSTextBlockLayer::Padding,
    );
    if header {
        cell.setBackgroundColor(Some(&code_background()));
    }
    cell
}

pub fn alert_symbol(kind: AlertKind) -> &'static str {
    match kind {
        AlertKind::Note => "info.circle",
        AlertKind::Tip => "lightbulb",
        AlertKind::Important => "exclamationmark.bubble",
        AlertKind::Warning => "exclamationmark.triangle",
        AlertKind::Caution => "exclamationmark.octagon",
    }
}
