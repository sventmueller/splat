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
use super::images::{ImageStore, locate, may_carry_along, resolve_path};
use super::picture::{Rendered, Sizing};
use super::style::{PREVIEW_SIZE, token_color};
use super::{js, remote};
use crate::markdown::code;
use crate::markdown::doc::AlertKind;
use crate::markdown::doc::Align;
use crate::markdown::style::Baseline;
use crate::markdown::text::{Utf16Map, bytes};
use crate::preview::layout::{Atom, Link, Marker, Para, ParaKind, ParaStyle, Run, RunMap, RunStyle, TopBlock};
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
    /// What leaves the Mac as a file shows only the local images `may_carry_along` allows.
    pub leaves_the_mac: bool,
    /// What the formulas of the document define for each other.
    pub macros: std::sync::Arc<str>,
    /// Pictures from the web and what WebKit drew, which the preview shows but does not make.
    pub drawn: &'a dyn Drawn,
}

/// What the preview shows but others make: pictures from the web and the formulas and diagrams WebKit drew.
pub trait Drawn {
    /// A picture from the web, once it has loaded.
    fn web_picture(&self, url: &str) -> Option<Rendered>;
    /// A formula or diagram as WebKit drew it, once it has.
    fn drawing(&self, key: &js::Key) -> Option<Rendered>;
}

/// The app's downloads and WebKit's drawings; `requester` hears when a picture it asked for arrives.
pub struct Live<'a> {
    pub requester: Option<&'a super::EditorController>,
}

impl Drawn for Live<'_> {
    fn web_picture(&self, url: &str) -> Option<Rendered> {
        remote::get(url, self.requester).flatten()
    }

    fn drawing(&self, key: &js::Key) -> Option<Rendered> {
        js::cached(key)
    }
}

struct TextBlocks {
    groups: HashMap<usize, Retained<NSTextBlock>>,
    tables: HashMap<usize, Retained<NSTextTable>>,
}

const LIST_INDENT: f64 = 26.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum FontFace {
    Regular,
    Bold,
    Mono,
}

/// A font as values, before AppKit makes it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FontPlan {
    face: FontFace,
    size: f64,
}

impl FontPlan {
    fn font(self) -> Retained<NSFont> {
        match self.face {
            FontFace::Regular => NSFont::systemFontOfSize(self.size),
            FontFace::Bold => NSFont::boldSystemFontOfSize(self.size),
            FontFace::Mono => NSFont::monospacedSystemFontOfSize_weight(self.size, constant!(NSFontWeightRegular)),
        }
    }
}

/// The font a paragraph of `kind` starts from, at text size `s`.
fn paragraph_font(kind: &ParaKind, s: f64) -> FontPlan {
    let (face, factor) = match kind {
        ParaKind::Heading(level) => (
            FontFace::Bold,
            [1.75, 1.45, 1.2, 1.05, 1.0, 0.9][(*level as usize).clamp(1, 6) - 1],
        ),
        ParaKind::Code { .. } => (FontFace::Mono, 0.87),
        ParaKind::FootnoteDef | ParaKind::FrontMatter => (FontFace::Regular, 0.87),
        ParaKind::AlertTitle => (FontFace::Bold, 1.0),
        _ => (FontFace::Regular, 1.0),
    };
    FontPlan { face, size: s * factor }
}

/// The font of a run: what it starts from, the size a raised or lowered run shrinks to, and the traits added last.
#[derive(Debug, PartialEq)]
struct RunFont {
    plan: FontPlan,
    script_size: Option<f64>,
    bold: bool,
    italic: bool,
}

fn run_font(kind: &ParaKind, s: f64, style: &RunStyle) -> RunFont {
    let base = paragraph_font(kind, s);
    let plan = if style.text.code || style.html {
        FontPlan {
            face: FontFace::Mono,
            size: base.size * 0.9,
        }
    } else {
        base
    };
    RunFont {
        plan,
        script_size: (style.text.baseline != Baseline::Normal).then_some(plan.size * 0.72),
        bold: style.text.bold,
        italic: style.text.italic,
    }
}

/// How far a raised or lowered run stands from the line, for a font of `point_size`.
fn baseline_offset(point_size: f64, baseline: Baseline) -> Option<f64> {
    match baseline {
        Baseline::Normal => None,
        Baseline::Super => Some(point_size * 0.55),
        Baseline::Sub => Some(point_size * -0.25),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum TextColor {
    Spoiler,
    Tag,
    Reference,
    Citation,
    Secondary,
    Alert(AlertKind),
    Label,
}

impl TextColor {
    fn of(run: &Run, para: &Para) -> TextColor {
        if run.style.text.spoiler {
            return TextColor::Spoiler;
        }
        if matches!(run.style.link, Some(Link::Tag(_))) {
            return TextColor::Tag;
        }
        if matches!(run.style.link, Some(Link::Reference(_))) {
            return TextColor::Reference;
        }
        if run.style.citation {
            return TextColor::Citation;
        }
        if matches!(run.map, RunMap::Atom { atom: Atom::Marker, .. }) {
            return TextColor::Secondary;
        }
        if run.style.html || matches!(para.style.kind, ParaKind::FrontMatter) {
            return TextColor::Secondary;
        }
        if let Some(kind) = para.style.alert {
            return if matches!(para.style.kind, ParaKind::AlertTitle) {
                TextColor::Alert(kind)
            } else {
                TextColor::Label
            };
        }
        if para.style.quote_depth > 0 || matches!(para.style.kind, ParaKind::FootnoteDef) {
            return TextColor::Secondary;
        }
        TextColor::Label
    }

    fn color(self) -> Retained<NSColor> {
        match self {
            TextColor::Spoiler => spoiler_color(),
            TextColor::Tag => NSColor::controlAccentColor(),
            TextColor::Reference => NSColor::linkColor(),
            TextColor::Citation => NSColor::systemBrownColor(),
            TextColor::Secondary => NSColor::secondaryLabelColor(),
            TextColor::Alert(kind) => alert_color(kind),
            TextColor::Label => NSColor::labelColor(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum TextBackground {
    Spoiler,
    Highlight,
    Code,
}

/// A code block has a background of its own, around all of its text.
fn text_background(style: &RunStyle, kind: &ParaKind) -> Option<TextBackground> {
    if style.text.spoiler {
        Some(TextBackground::Spoiler)
    } else if style.text.highlight {
        Some(TextBackground::Highlight)
    } else if style.text.code && !matches!(kind, ParaKind::Code { .. }) {
        Some(TextBackground::Code)
    } else {
        None
    }
}

/// The text block around a paragraph.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Frame {
    None,
    HeadingRule,
    Code,
    TableCell,
    Rule,
    Quote,
}

/// A paragraph style as values, before AppKit gets it.
#[derive(Clone, Debug, PartialEq)]
struct ParaLook {
    direction: NSWritingDirection,
    head: f64,
    first_line: f64,
    tab_stop: Option<f64>,
    line_height: f64,
    spacing: f64,
    spacing_before: Option<f64>,
    alignment: Option<NSTextAlignment>,
    frame: Frame,
}

fn para_look(style: &ParaStyle, s: f64, continues_group: bool) -> ParaLook {
    let list_indent = style.list_depth as f64 * LIST_INDENT;
    let framed_alone = matches!(style.kind, ParaKind::Code { .. } | ParaKind::TableCell { .. });
    let quote_indent = if framed_alone {
        0.0
    } else {
        style.quote_depth as f64 * QUOTE_INDENT
    };
    let head = list_indent + quote_indent;
    let mut look = ParaLook {
        // TextKit 1 aligns a paragraph without an explicit direction by the interface language, not by its text.
        direction: match style.rtl {
            Some(true) => NSWritingDirection::RightToLeft,
            Some(false) => NSWritingDirection::LeftToRight,
            None => NSWritingDirection::Natural,
        },
        head,
        first_line: if style.marker.is_some() {
            head - LIST_INDENT * 0.85
        } else {
            head
        },
        tab_stop: style.marker.is_some().then_some(head),
        line_height: 1.15,
        spacing: if style.tight && style.list_depth > 0 {
            s * 0.25
        } else {
            s * 0.7
        },
        spacing_before: style.space_before.then_some(s * 0.9),
        alignment: None,
        frame: Frame::None,
    };
    match &style.kind {
        ParaKind::Heading(level) => {
            look.spacing_before = Some(s * if *level <= 2 { 1.1 } else { 0.8 });
            look.spacing = s * 0.45;
            if *level <= 2 {
                look.frame = Frame::HeadingRule;
            }
        }
        ParaKind::Code { .. } => {
            look.frame = Frame::Code;
            look.spacing = 0.0;
            look.line_height = 1.05;
            look.first_line = head;
        }
        ParaKind::TableCell { align, .. } => {
            look.frame = Frame::TableCell;
            look.alignment = Some(match align {
                Align::Left | Align::None => NSTextAlignment::Natural,
                Align::Center => NSTextAlignment::Center,
                Align::Right => NSTextAlignment::Right,
            });
            look.spacing = 0.0;
            look.head = 0.0;
            look.first_line = 0.0;
        }
        ParaKind::Rule => {
            look.frame = Frame::Rule;
            look.line_height = 0.5;
        }
        ParaKind::MathBlock | ParaKind::Diagram => look.alignment = Some(NSTextAlignment::Center),
        _ => {}
    }
    if !matches!(style.kind, ParaKind::TableCell { .. }) {
        match style.align {
            Align::Center => look.alignment = Some(NSTextAlignment::Center),
            Align::Right => look.alignment = Some(NSTextAlignment::Right),
            Align::Left => look.alignment = Some(NSTextAlignment::Left),
            Align::None => {}
        }
    }
    if style.quote_depth > 0 && !framed_alone {
        look.frame = Frame::Quote;
        look.head = list_indent;
        look.first_line = if style.marker.is_some() {
            list_indent - LIST_INDENT * 0.85
        } else {
            list_indent
        }
        .max(0.0);
        look.spacing = if continues_group { s * 0.5 } else { 0.0 };
    }
    look
}

/// The first column and the span of a table cell, from the side its table reads from.
fn cell_columns(mirrored: bool, columns: u32, col: u32, span: u32) -> (u32, u32) {
    let span = span.max(1);
    let column = if mirrored {
        columns.saturating_sub(col + span)
    } else {
        col
    };
    (column, span)
}

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
        paragraph_font(kind, self.size()).font()
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
                nl.push(Attribute::foreground(&TextColor::of(last, para).color()));
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
        let look = para_look(style, s, continues_group);
        let st = NSMutableParagraphStyle::new();
        st.setBaseWritingDirection(look.direction);
        st.setHeadIndent(look.head);
        st.setFirstLineHeadIndent(look.first_line);
        if let Some(location) = look.tab_stop {
            // SAFETY: Empty options, which NSTextTab accepts.
            let tab = unsafe {
                NSTextTab::initWithTextAlignment_location_options(
                    NSTextTab::alloc(),
                    NSTextAlignment::Natural,
                    location,
                    &NSDictionary::new(),
                )
            };
            st.setTabStops(Some(&NSArray::from_slice(&[&*tab])));
        }
        st.setLineHeightMultiple(look.line_height);
        st.setParagraphSpacing(look.spacing);
        if let Some(before) = look.spacing_before {
            st.setParagraphSpacingBefore(before);
        }
        if let Some(alignment) = look.alignment {
            st.setAlignment(alignment);
        }
        match look.frame {
            Frame::None => {}
            Frame::HeadingRule => st.setTextBlocks(&NSArray::from_slice(&[&*heading_rule_frame(s)])),
            Frame::Code => {
                let b = blocks.groups.entry(style.group).or_insert_with(|| code_block_frame(s));
                st.setTextBlocks(&NSArray::from_slice(&[&**b]));
            }
            Frame::Rule => st.setTextBlocks(&NSArray::from_slice(&[&*rule_frame(s)])),
            Frame::Quote => {
                let b = blocks
                    .groups
                    .entry(style.group)
                    .or_insert_with(|| quote_frame(style, s));
                st.setTextBlocks(&NSArray::from_slice(&[&**b]));
            }
            Frame::TableCell => {
                if let ParaKind::TableCell {
                    table,
                    row,
                    col,
                    header,
                    columns,
                    span,
                    ..
                } = &style.kind
                {
                    let t = blocks
                        .tables
                        .entry(*table as usize)
                        .or_insert_with(|| text_table(s, *columns as usize));
                    let (column, span) = cell_columns(style.mirrored, *columns, *col, *span);
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
                }
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
        let plan = run_font(&para.style.kind, self.size(), &run.style);
        let mut font = plan.plan.font();
        if let Some(size) = plan.script_size {
            font = NSFont::fontWithDescriptor_size(&font.fontDescriptor(), size).unwrap_or(font);
        }
        let manager = NSFontManager::sharedFontManager(self.mtm);
        if plan.bold {
            font = manager.convertFont_toHaveTrait(&font, NSFontTraitMask::BoldFontMask);
        }
        if plan.italic {
            font = manager.convertFont_toHaveTrait(&font, NSFontTraitMask::ItalicFontMask);
        }
        font
    }

    fn run(&self, run: &Run, para: &Para, paragraph: &NSMutableParagraphStyle) -> Retained<NSAttributedString> {
        let font = self.font_for(run, para);
        let mut attributes = vec![
            Attribute::font(&font),
            Attribute::foreground(&TextColor::of(run, para).color()),
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
        match text_background(&run.style, &para.style.kind) {
            Some(TextBackground::Spoiler) => {
                attributes.push(Attribute::background(&spoiler_color()));
                attributes.push(Attribute::tool_tip(crate::i18n::t(
                    "Verborgener Text: Zum Lesen markieren",
                )));
            }
            Some(TextBackground::Highlight) => attributes.push(Attribute::background(&highlight_background())),
            Some(TextBackground::Code) => attributes.push(Attribute::background(&code_background())),
            None => {}
        }
        if let Some(offset) = baseline_offset(font.pointSize(), run.style.text.baseline) {
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
                    byte_range_to_ns(&map, &bytes(range)),
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
                        .then(|| self.drawn.web_picture(url))
                        .flatten()
                        .map(|r| self.images.borrow_mut().for_appearance(url, r, self.dark))
                } else if self.leaves_the_mac && !may_leave_with_the_document(url, self.doc_dir) {
                    None
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
            Atom::Math { tex, display } => {
                self.drawn
                    .drawing(&js::Key::math(tex, *display, self.dark, &self.macros))?
            }
            Atom::Diagram { source, kind } => self.drawn.drawing(&js::Key::diagram(source, *kind, self.dark))?,
            Atom::AlertIcon(kind) => {
                let r = placeholder(alert_symbol(*kind), font)?;
                let tint = NSImageSymbolConfiguration::configurationWithHierarchicalColor(&alert_color(*kind));
                let image = r.image.imageWithSymbolConfiguration(&tint)?;
                image.setAccessibilityDescription(Some(&ns(crate::i18n::t(kind.label()))));
                Rendered { image, ..r }
            }
            _ => return None,
        };
        let att = NSTextAttachment::new();
        att.setImage(Some(&rendered.image));
        att.setBounds(attachment_bounds(rendered.sizing, font.xHeight(), self.max_image_width));
        Some(att)
    }
}

fn may_leave_with_the_document(url: &str, doc_dir: Option<&Path>) -> bool {
    resolve_path(url, doc_dir).is_some_and(|path| may_carry_along(&locate(&path, doc_dir), doc_dir))
}

/// Where an attachment stands against its line and how large it is, no wider than `max_width`.
fn attachment_bounds(sizing: Sizing, x_height: f64, max_width: f64) -> NSRect {
    let (mut size, descent) = match sizing {
        Sizing::Points { size, descent } => (size, descent),
        Sizing::Ex { width, height, valign } => (NSSize::new(width * x_height, height * x_height), -valign * x_height),
    };
    if size.width > max_width && size.width > 0.0 {
        let scale = max_width / size.width;
        size = NSSize::new(size.width * scale, size.height * scale);
    }
    NSRect::new(NSPoint::new(0.0, -descent), size)
}

/// The size embedded HTML asks for: one side keeps the picture's proportions, both stretch it.
fn asked_size(size: NSSize, width: Option<u32>, height: Option<u32>) -> NSSize {
    if size.width <= 0.0 || size.height <= 0.0 {
        return size;
    }
    let ratio = size.height / size.width;
    match (width.map(f64::from), height.map(f64::from)) {
        (Some(w), Some(h)) => NSSize::new(w, h),
        (Some(w), None) => NSSize::new(w, w * ratio),
        (None, Some(h)) => NSSize::new(h / ratio, h),
        (None, None) => size,
    }
}

fn sized(r: Rendered, width: Option<u32>, height: Option<u32>) -> Rendered {
    let Sizing::Points { size, descent } = r.sizing else {
        return r;
    };
    Rendered {
        sizing: Sizing::Points {
            size: asked_size(size, width, height),
            descent,
        },
        ..r
    }
}

/// An image pixel is one point next to text at 100 %, as in a browser, so the image follows the text's zoom. A formula is
/// measured in the height of the text and follows on its own.
fn zoomed_sizing(sizing: Sizing, zoom: f64) -> Sizing {
    match sizing {
        Sizing::Points { size, descent } => Sizing::Points {
            size: NSSize::new(size.width * zoom, size.height * zoom),
            descent: descent * zoom,
        },
        ex @ Sizing::Ex { .. } => ex,
    }
}

fn zoomed(r: Rendered, zoom: f64) -> Rendered {
    Rendered {
        sizing: zoomed_sizing(r.sizing, zoom),
        ..r
    }
}

/// A symbol as tall as its font, standing half its font's descender below the line.
fn symbol_sizing(symbol: NSSize, point_size: f64, descender: f64) -> Sizing {
    let scale = if symbol.height > 0.0 {
        point_size / symbol.height
    } else {
        1.0
    };
    Sizing::Points {
        size: NSSize::new(symbol.width * scale, point_size),
        descent: descender.abs() * 0.5,
    }
}

fn placeholder(symbol: &str, font: &NSFont) -> Option<Rendered> {
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(symbol), None)?;
    let sizing = symbol_sizing(image.size(), font.pointSize(), font.descender());
    Some(Rendered { image, sizing })
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

/// Sets a width in points on one edge of a block.
fn set_edge(block: &NSTextBlock, width: f64, layer: NSTextBlockLayer, edge: NSRectEdge) {
    block.setWidth_type_forLayer_edge(width, NSTextBlockValueType::AbsoluteValueType, layer, edge);
}

fn heading_rule_frame(s: f64) -> Retained<NSTextBlock> {
    let b = full_width_block();
    set_edge(&b, 1.0, NSTextBlockLayer::Border, NSRectEdge::MaxY);
    b.setBorderColor_forEdge(Some(&NSColor::separatorColor()), NSRectEdge::MaxY);
    set_edge(&b, s * 0.3, NSTextBlockLayer::Padding, NSRectEdge::MaxY);
    set_edge(&b, s * 0.5, NSTextBlockLayer::Margin, NSRectEdge::MaxY);
    b
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

#[cfg(test)]
mod tests {
    use objc2_app_kit::NSFontDescriptorSymbolicTraits;

    use super::*;
    use crate::markdown::reader::parse;
    use crate::markdown::style::TextStyle;
    use crate::preview::layout::{CharMap, NoImages, layout};

    fn style(kind: ParaKind) -> ParaStyle {
        ParaStyle {
            kind,
            quote_depth: 0,
            list_depth: 0,
            marker: None,
            group: 0,
            tight: false,
            space_before: false,
            alert: None,
            align: Align::None,
            html: false,
            rtl: None,
            mirrored: false,
        }
    }

    fn cell(align: Align) -> ParaKind {
        ParaKind::TableCell {
            table: 0,
            row: 0,
            col: 0,
            header: false,
            align,
            columns: 2,
            span: 1,
        }
    }

    fn code() -> ParaKind {
        ParaKind::Code { info: "rs".into() }
    }

    fn text(style: TextStyle) -> RunStyle {
        RunStyle {
            text: style,
            ..RunStyle::default()
        }
    }

    fn run(style: RunStyle) -> Run {
        Run {
            text: "x".into(),
            style,
            map: RunMap::Text {
                chars: CharMap::Verbatim(crate::markdown::text::Byte::new(0)),
                end: crate::markdown::text::Byte::new(1),
            },
        }
    }

    fn para(style: ParaStyle) -> Para {
        Para {
            style,
            runs: Vec::new(),
            src: crate::markdown::text::bytes(0..0),
            insert_at: crate::markdown::text::Byte::new(0),
        }
    }

    fn plan(face: FontFace, size: f64) -> FontPlan {
        FontPlan { face, size }
    }

    #[test]
    fn a_paragraph_kind_sets_face_and_size_of_its_font_at_every_zoom() {
        let at = |kind: ParaKind, s| paragraph_font(&kind, s);
        assert_eq!(at(ParaKind::Heading(1), 15.0), plan(FontFace::Bold, 26.25));
        assert_eq!(at(ParaKind::Heading(2), 15.0), plan(FontFace::Bold, 21.75));
        assert_eq!(at(ParaKind::Heading(3), 15.0), plan(FontFace::Bold, 18.0));
        assert_eq!(at(ParaKind::Heading(6), 20.0), plan(FontFace::Bold, 18.0));
        assert_eq!(
            at(ParaKind::Heading(0), 10.0),
            plan(FontFace::Bold, 17.5),
            "the first level"
        );
        assert_eq!(
            at(ParaKind::Heading(9), 10.0),
            plan(FontFace::Bold, 9.0),
            "the last level"
        );
        assert_eq!(at(code(), 10.0), plan(FontFace::Mono, 8.7));
        assert_eq!(at(ParaKind::FootnoteDef, 10.0), plan(FontFace::Regular, 8.7));
        assert_eq!(at(ParaKind::FrontMatter, 10.0), plan(FontFace::Regular, 8.7));
        assert_eq!(at(ParaKind::AlertTitle, 15.0), plan(FontFace::Bold, 15.0));
        assert_eq!(at(ParaKind::Body, 15.0), plan(FontFace::Regular, 15.0));
        assert_eq!(at(cell(Align::None), 30.0), plan(FontFace::Regular, 30.0));
    }

    #[test]
    fn a_plan_makes_the_font_it_says() {
        let traits = |font: &NSFont| font.fontDescriptor().symbolicTraits();
        let bold = plan(FontFace::Bold, 26.25).font();
        assert_eq!(bold.pointSize(), 26.25);
        assert!(traits(&bold).contains(NSFontDescriptorSymbolicTraits::TraitBold));
        let regular = plan(FontFace::Regular, 13.05).font();
        assert_eq!(regular.pointSize(), 13.05);
        assert!(!traits(&regular).contains(NSFontDescriptorSymbolicTraits::TraitBold));
        let mono = plan(FontFace::Mono, 8.7).font();
        assert_eq!(mono.pointSize(), 8.7);
        assert!(mono.isFixedPitch());
    }

    #[test]
    fn a_run_changes_the_font_of_its_paragraph_by_its_own_style() {
        let plain = run_font(&ParaKind::Body, 20.0, &RunStyle::default());
        assert_eq!(
            plain,
            RunFont {
                plan: plan(FontFace::Regular, 20.0),
                script_size: None,
                bold: false,
                italic: false,
            }
        );
        let code_in_heading = run_font(
            &ParaKind::Heading(1),
            20.0,
            &text(TextStyle {
                code: true,
                ..TextStyle::default()
            }),
        );
        assert_eq!(code_in_heading.plan, plan(FontFace::Mono, 31.5));
        let html = RunStyle {
            html: true,
            ..RunStyle::default()
        };
        assert_eq!(run_font(&ParaKind::Body, 20.0, &html).plan, plan(FontFace::Mono, 18.0));
        let raised = run_font(
            &ParaKind::Body,
            20.0,
            &text(TextStyle {
                baseline: Baseline::Super,
                bold: true,
                ..TextStyle::default()
            }),
        );
        assert_eq!(raised.script_size, Some(20.0 * 0.72));
        assert!(raised.bold && !raised.italic);
        let lowered_code = run_font(
            &ParaKind::Body,
            20.0,
            &text(TextStyle {
                baseline: Baseline::Sub,
                code: true,
                italic: true,
                ..TextStyle::default()
            }),
        );
        assert_eq!(lowered_code.script_size, Some(18.0 * 0.72));
        assert!(lowered_code.italic && !lowered_code.bold);
    }

    #[test]
    fn raised_text_stands_above_the_line_and_lowered_text_below_it() {
        assert_eq!(baseline_offset(10.0, Baseline::Normal), None);
        assert_eq!(baseline_offset(10.0, Baseline::Super), Some(5.5));
        assert_eq!(baseline_offset(10.0, Baseline::Sub), Some(-2.5));
    }

    #[test]
    fn the_color_of_text_goes_by_what_it_is_before_where_it_stands() {
        let of = |style: RunStyle, para_style: ParaStyle| TextColor::of(&run(style), &para(para_style));
        let body = style(ParaKind::Body);
        let spoiler = text(TextStyle {
            spoiler: true,
            ..TextStyle::default()
        });
        let tag = RunStyle {
            link: Some(Link::Tag("#a".into())),
            ..RunStyle::default()
        };
        let reference = RunStyle {
            link: Some(Link::Reference("#1".into())),
            ..RunStyle::default()
        };
        let citation = RunStyle {
            citation: true,
            ..RunStyle::default()
        };
        let html = RunStyle {
            html: true,
            ..RunStyle::default()
        };
        let quoted = ParaStyle {
            quote_depth: 1,
            ..body.clone()
        };
        let alert_body = ParaStyle {
            alert: Some(AlertKind::Warning),
            quote_depth: 1,
            ..body.clone()
        };
        let alert_title = ParaStyle {
            kind: ParaKind::AlertTitle,
            ..alert_body.clone()
        };
        assert_eq!(of(RunStyle::default(), body.clone()), TextColor::Label);
        assert_eq!(
            of(
                RunStyle {
                    link: Some(Link::Tag("#a".into())),
                    ..spoiler.clone()
                },
                body.clone()
            ),
            TextColor::Spoiler,
            "a spoiler hides whatever it is"
        );
        assert_eq!(of(tag.clone(), quoted.clone()), TextColor::Tag);
        assert_eq!(of(reference, quoted.clone()), TextColor::Reference);
        assert_eq!(of(citation, quoted.clone()), TextColor::Citation);
        assert_eq!(of(html, body.clone()), TextColor::Secondary);
        assert_eq!(
            of(RunStyle::default(), style(ParaKind::FrontMatter)),
            TextColor::Secondary
        );
        assert_eq!(
            of(RunStyle::default(), style(ParaKind::FootnoteDef)),
            TextColor::Secondary
        );
        assert_eq!(of(RunStyle::default(), quoted), TextColor::Secondary);
        assert_eq!(
            of(RunStyle::default(), alert_title),
            TextColor::Alert(AlertKind::Warning)
        );
        assert_eq!(
            of(RunStyle::default(), alert_body.clone()),
            TextColor::Label,
            "the text of a callout is not grey like a quote"
        );
        let marker = Run {
            map: RunMap::Atom {
                atom: Atom::Marker,
                src: crate::markdown::text::bytes(0..1),
            },
            ..run(RunStyle::default())
        };
        assert_eq!(TextColor::of(&marker, &para(alert_body)), TextColor::Secondary);
    }

    #[test]
    fn a_background_goes_by_spoiler_then_highlight_then_code_outside_code_blocks() {
        let all = |highlight, code, spoiler| {
            text(TextStyle {
                highlight,
                code,
                spoiler,
                ..TextStyle::default()
            })
        };
        let body = ParaKind::Body;
        assert_eq!(text_background(&RunStyle::default(), &body), None);
        assert_eq!(
            text_background(&all(true, true, true), &body),
            Some(TextBackground::Spoiler)
        );
        assert_eq!(
            text_background(&all(true, true, false), &body),
            Some(TextBackground::Highlight)
        );
        assert_eq!(
            text_background(&all(false, true, false), &body),
            Some(TextBackground::Code)
        );
        assert_eq!(text_background(&all(false, true, false), &code()), None);
        assert_eq!(
            text_background(&all(true, false, false), &code()),
            Some(TextBackground::Highlight)
        );
    }

    #[test]
    fn plain_paragraphs_space_and_indent_by_their_size_and_depth() {
        let plain = para_look(&style(ParaKind::Body), 20.0, false);
        assert_eq!(
            plain,
            ParaLook {
                direction: NSWritingDirection::Natural,
                head: 0.0,
                first_line: 0.0,
                tab_stop: None,
                line_height: 1.15,
                spacing: 14.0,
                spacing_before: None,
                alignment: None,
                frame: Frame::None,
            }
        );
        let item = ParaStyle {
            list_depth: 2,
            marker: Some(Marker::Bullet(0)),
            tight: true,
            ..style(ParaKind::Body)
        };
        let look = para_look(&item, 20.0, false);
        assert_eq!((look.head, look.tab_stop), (52.0, Some(52.0)));
        assert_eq!(look.first_line, 52.0 - 26.0 * 0.85);
        assert_eq!(look.spacing, 5.0, "a tight list");
        let tight_outside_a_list = ParaStyle {
            tight: true,
            ..style(ParaKind::Body)
        };
        assert_eq!(para_look(&tight_outside_a_list, 20.0, false).spacing, 14.0);
        let after_table = ParaStyle {
            space_before: true,
            ..style(ParaKind::Body)
        };
        assert_eq!(para_look(&after_table, 20.0, false).spacing_before, Some(18.0));
    }

    #[test]
    fn a_paragraph_reads_in_its_own_direction_and_keeps_an_alignment_asked_for() {
        let with_direction = |rtl| {
            para_look(
                &ParaStyle {
                    rtl,
                    ..style(ParaKind::Body)
                },
                10.0,
                false,
            )
            .direction
        };
        assert_eq!(with_direction(Some(true)), NSWritingDirection::RightToLeft);
        assert_eq!(with_direction(Some(false)), NSWritingDirection::LeftToRight);
        assert_eq!(with_direction(None), NSWritingDirection::Natural);
        let aligned = |kind, align| para_look(&ParaStyle { align, ..style(kind) }, 10.0, false).alignment;
        assert_eq!(aligned(ParaKind::Body, Align::None), None);
        assert_eq!(aligned(ParaKind::Body, Align::Center), Some(NSTextAlignment::Center));
        assert_eq!(aligned(ParaKind::Body, Align::Right), Some(NSTextAlignment::Right));
        assert_eq!(aligned(ParaKind::Body, Align::Left), Some(NSTextAlignment::Left));
        assert_eq!(aligned(ParaKind::MathBlock, Align::None), Some(NSTextAlignment::Center));
        assert_eq!(aligned(ParaKind::Diagram, Align::Right), Some(NSTextAlignment::Right));
        assert_eq!(
            aligned(cell(Align::Center), Align::Right),
            Some(NSTextAlignment::Center),
            "a cell goes by its column"
        );
        assert_eq!(aligned(cell(Align::None), Align::None), Some(NSTextAlignment::Natural));
        assert_eq!(aligned(cell(Align::Left), Align::None), Some(NSTextAlignment::Natural));
        assert_eq!(aligned(cell(Align::Right), Align::None), Some(NSTextAlignment::Right));
    }

    #[test]
    fn the_first_two_heading_levels_have_a_rule_and_more_space_above() {
        let high = para_look(&style(ParaKind::Heading(2)), 10.0, false);
        assert_eq!(high.frame, Frame::HeadingRule);
        assert_eq!((high.spacing_before, high.spacing), (Some(11.0), 4.5));
        let low = para_look(&style(ParaKind::Heading(3)), 10.0, false);
        assert_eq!(low.frame, Frame::None);
        assert_eq!((low.spacing_before, low.spacing), (Some(8.0), 4.5));
    }

    #[test]
    fn code_cells_and_rules_are_framed_without_the_indent_of_a_quote() {
        let in_quote = |kind| ParaStyle {
            quote_depth: 2,
            list_depth: 1,
            marker: Some(Marker::Bullet(0)),
            ..style(kind)
        };
        let code_look = para_look(&in_quote(code()), 10.0, false);
        assert_eq!(code_look.frame, Frame::Code);
        assert_eq!(
            (code_look.head, code_look.first_line),
            (26.0, 26.0),
            "the list counts, the quote not"
        );
        assert_eq!((code_look.spacing, code_look.line_height), (0.0, 1.05));
        let cell_look = para_look(&in_quote(cell(Align::None)), 10.0, false);
        assert_eq!(cell_look.frame, Frame::TableCell);
        assert_eq!(
            (cell_look.head, cell_look.first_line, cell_look.spacing),
            (0.0, 0.0, 0.0)
        );
        let rule = para_look(&style(ParaKind::Rule), 10.0, false);
        assert_eq!((rule.frame, rule.line_height), (Frame::Rule, 0.5));
    }

    #[test]
    fn a_quote_frames_its_paragraphs_and_spaces_them_inside_itself() {
        let quoted = ParaStyle {
            quote_depth: 2,
            list_depth: 1,
            ..style(ParaKind::Body)
        };
        let inside = para_look(&quoted, 10.0, true);
        assert_eq!(inside.frame, Frame::Quote);
        assert_eq!((inside.head, inside.first_line), (26.0, 26.0));
        assert_eq!(inside.spacing, 5.0);
        assert_eq!(para_look(&quoted, 10.0, false).spacing, 0.0, "the frame ends here");
        let item = ParaStyle {
            marker: Some(Marker::Bullet(0)),
            ..quoted
        };
        let look = para_look(&item, 10.0, false);
        assert_eq!(look.first_line, 26.0 - 26.0 * 0.85);
        assert_eq!(
            look.tab_stop,
            Some(26.0 + 28.0),
            "the marker's tab counts the quote as well"
        );
        let unlisted_marker = ParaStyle {
            quote_depth: 1,
            marker: Some(Marker::Footnote("a".into(), "1".into())),
            ..style(ParaKind::Body)
        };
        assert_eq!(
            para_look(&unlisted_marker, 10.0, false).first_line,
            0.0,
            "never in front of the frame"
        );
        let title = ParaStyle {
            quote_depth: 1,
            ..style(ParaKind::Heading(1))
        };
        assert_eq!(
            para_look(&title, 10.0, false).frame,
            Frame::Quote,
            "instead of the rule"
        );
    }

    #[test]
    fn a_mirrored_table_counts_its_columns_from_the_other_side() {
        assert_eq!(cell_columns(false, 4, 1, 1), (1, 1));
        assert_eq!(cell_columns(false, 4, 1, 0), (1, 1), "a span of nothing is one column");
        assert_eq!(cell_columns(true, 4, 0, 1), (3, 1));
        assert_eq!(cell_columns(true, 4, 1, 2), (1, 2));
        assert_eq!(
            cell_columns(true, 2, 3, 1),
            (0, 1),
            "a cell beyond the columns stays in the table"
        );
    }

    fn width(block: &NSTextBlock, layer: NSTextBlockLayer, edge: NSRectEdge) -> f64 {
        block.widthForLayer_edge(layer, edge)
    }

    #[test]
    fn frames_grow_with_the_text_and_stand_on_the_side_it_starts() {
        let rule = heading_rule_frame(20.0);
        assert_eq!(width(&rule, NSTextBlockLayer::Border, NSRectEdge::MaxY), 1.0);
        assert_eq!(width(&rule, NSTextBlockLayer::Padding, NSRectEdge::MaxY), 6.0);
        assert_eq!(width(&rule, NSTextBlockLayer::Margin, NSRectEdge::MaxY), 10.0);
        let code_frame = code_block_frame(20.0);
        assert_eq!(width(&code_frame, NSTextBlockLayer::Padding, NSRectEdge::MinX), 12.0);
        assert_eq!(width(&code_frame, NSTextBlockLayer::Margin, NSRectEdge::MaxY), 14.0);
        let left = quote_frame(&style(ParaKind::Body), 20.0);
        assert_eq!(width(&left, NSTextBlockLayer::Border, NSRectEdge::MinX), 3.0);
        assert_eq!(width(&left, NSTextBlockLayer::Border, NSRectEdge::MaxX), 0.0);
        assert_eq!(width(&left, NSTextBlockLayer::Padding, NSRectEdge::MinX), 14.0);
        let right = quote_frame(
            &ParaStyle {
                mirrored: true,
                ..style(ParaKind::Body)
            },
            20.0,
        );
        assert_eq!(width(&right, NSTextBlockLayer::Border, NSRectEdge::MinX), 0.0);
        assert_eq!(width(&right, NSTextBlockLayer::Border, NSRectEdge::MaxX), 3.0);
        assert_eq!(width(&right, NSTextBlockLayer::Padding, NSRectEdge::MaxX), 14.0);
    }

    #[test]
    fn a_table_cell_knows_its_place_and_only_a_header_has_a_background() {
        let table = text_table(10.0, 3);
        assert_eq!(table.numberOfColumns(), 3);
        let header = table_cell(&table, 10.0, (0, 1, 2), true);
        assert_eq!(
            (
                header.startingRow(),
                header.startingColumn(),
                header.columnSpan(),
                header.rowSpan()
            ),
            (0, 1, 2, 1)
        );
        assert!(header.backgroundColor().is_some());
        assert_eq!(width(&header, NSTextBlockLayer::Padding, NSRectEdge::MinX), 4.0);
        assert!(table_cell(&table, 10.0, (1, 0, 1), false).backgroundColor().is_none());
    }

    #[test]
    fn an_attachment_goes_by_its_points_or_by_the_text_and_never_past_the_column() {
        let points = Sizing::Points {
            size: NSSize::new(100.0, 50.0),
            descent: 4.0,
        };
        assert_eq!(
            attachment_bounds(points, 7.0, 400.0),
            NSRect::new(NSPoint::new(0.0, -4.0), NSSize::new(100.0, 50.0))
        );
        assert_eq!(
            attachment_bounds(points, 7.0, 40.0),
            NSRect::new(NSPoint::new(0.0, -4.0), NSSize::new(40.0, 20.0)),
            "both sides shrink, the descent stays"
        );
        let ex = Sizing::Ex {
            width: 4.0,
            height: 2.0,
            valign: -0.5,
        };
        assert_eq!(
            attachment_bounds(ex, 10.0, 400.0),
            NSRect::new(NSPoint::new(0.0, -5.0), NSSize::new(40.0, 20.0))
        );
    }

    #[test]
    fn one_side_asked_for_keeps_the_proportions_and_two_stretch_them() {
        let size = NSSize::new(200.0, 100.0);
        assert_eq!(asked_size(size, None, None), size);
        assert_eq!(asked_size(size, Some(50), None), NSSize::new(50.0, 25.0));
        assert_eq!(asked_size(size, None, Some(50)), NSSize::new(100.0, 50.0));
        assert_eq!(asked_size(size, Some(10), Some(70)), NSSize::new(10.0, 70.0));
        let empty = NSSize::new(0.0, 10.0);
        assert_eq!(asked_size(empty, Some(50), Some(50)), empty, "nothing to scale");
    }

    #[test]
    fn images_follow_the_zoom_and_formulas_the_text() {
        let points = Sizing::Points {
            size: NSSize::new(10.0, 20.0),
            descent: 2.0,
        };
        assert_eq!(
            zoomed_sizing(points, 1.5),
            Sizing::Points {
                size: NSSize::new(15.0, 30.0),
                descent: 3.0,
            }
        );
        let ex = Sizing::Ex {
            width: 1.0,
            height: 2.0,
            valign: 0.5,
        };
        assert_eq!(zoomed_sizing(ex, 1.5), ex);
    }

    #[test]
    fn a_symbol_is_as_tall_as_its_font() {
        assert_eq!(
            symbol_sizing(NSSize::new(30.0, 20.0), 10.0, -3.0),
            Sizing::Points {
                size: NSSize::new(15.0, 10.0),
                descent: 1.5,
            }
        );
        assert_eq!(
            symbol_sizing(NSSize::new(30.0, 0.0), 10.0, 3.0),
            Sizing::Points {
                size: NSSize::new(30.0, 10.0),
                descent: 1.5,
            }
        );
    }

    #[test]
    fn the_stand_in_of_a_block_has_the_length_of_the_block() {
        let shown = layout(
            &parse("Eins **zwei**\n\n- a\n- b\n\n| x | y |\n|---|---|\n| 1 | 2 |\n"),
            &NoImages,
            None,
        );
        let mut all = String::new();
        for block in &shown.blocks {
            let mut stand_in = String::new();
            RenderCtx::placeholder_text(block, &mut stand_in);
            assert_eq!(stand_in.encode_utf16().count(), block.len_u16, "{stand_in:?}");
            all.push_str(&stand_in);
        }
        assert!(all.starts_with("Eins zwei\n"), "{all:?}");
        assert!(all.ends_with("1\n2\n"), "{all:?}");
    }

    #[test]
    fn every_alert_has_a_symbol_of_the_system() {
        for kind in AlertKind::ALL {
            let name = alert_symbol(kind);
            assert!(
                NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(name), None).is_some(),
                "{name}"
            );
        }
    }
}
