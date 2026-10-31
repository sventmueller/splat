use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker, Message};
use objc2_app_kit::{
    NSAttachmentAttributeName, NSBackgroundColorAttributeName, NSBaselineOffsetAttributeName, NSColor, NSFont,
    NSFontAttributeName, NSFontManager, NSFontTraitMask, NSFontWeightRegular, NSForegroundColorAttributeName, NSImage,
    NSImageSymbolConfiguration, NSLinkAttributeName, NSMutableParagraphStyle, NSParagraphStyleAttributeName,
    NSStrikethroughStyleAttributeName, NSTextAlignment, NSTextAttachment, NSTextBlock, NSTextBlockLayer,
    NSTextBlockValueType, NSTextTab, NSTextTable, NSTextTableBlock, NSTextTableLayoutAlgorithm, NSToolTipAttributeName,
    NSUnderlineStyleAttributeName, NSWritingDirection,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSDictionary, NSMutableAttributedString, NSNumber, NSPoint, NSRect, NSRectEdge,
    NSSize, NSURL,
};

use super::images::{ImageStore, Rendered, Sizing};
use super::style::{PREVIEW_SIZE, attrs, token_color};
use super::{js, remote};
use crate::markdown::code;
use crate::markdown::doc::AlertKind;
use crate::markdown::text::Utf16Map;
use crate::preview::layout::{AlignKey, Atom, Marker, Para, ParaKind, Run, RunMap, TopBlock};
use crate::util::byte_range_to_ns;
use crate::util::constant;
use crate::util::{ns, obj};

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

struct Blocks {
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
        let mut blocks = Blocks {
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
            let nl = attrs(&[
                (constant!(NSParagraphStyleAttributeName), &*style),
                (constant!(NSFontAttributeName), &*self.base_font(&para.style.kind)),
            ]);
            // SAFETY: The attributes have the types their keys expect.
            let newline = unsafe {
                NSAttributedString::initWithString_attributes(NSAttributedString::alloc(), &ns("\n"), Some(&nl))
            };
            out.appendAttributedString(&newline);
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
        let dict = attrs(&[
            (constant!(NSFontAttributeName), &*font),
            (constant!(NSForegroundColorAttributeName), &*color),
        ]);
        // SAFETY: The attributes have the types their keys expect.
        unsafe {
            NSMutableAttributedString::initWithString_attributes(
                NSMutableAttributedString::alloc(),
                &ns(text),
                Some(&dict),
            )
        }
    }

    fn paragraph_style(
        &self,
        para: &Para,
        blocks: &mut Blocks,
        continues_group: bool,
    ) -> Retained<NSMutableParagraphStyle> {
        let st = NSMutableParagraphStyle::new();
        // TextKit 1 aligns a paragraph without an explicit direction by the interface language, not by its text.
        st.setBaseWritingDirection(match para.style.rtl {
            Some(true) => NSWritingDirection::RightToLeft,
            Some(false) => NSWritingDirection::LeftToRight,
            None => NSWritingDirection::Natural,
        });
        let s = self.size();
        let style = &para.style;
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
        match &style.kind {
            ParaKind::Heading(level) => {
                st.setParagraphSpacingBefore(s * if *level <= 2 { 1.1 } else { 0.8 });
                st.setParagraphSpacing(s * 0.45);
                if *level <= 2 {
                    let b = full_width_block();
                    b.setWidth_type_forLayer_edge(
                        1.0,
                        NSTextBlockValueType::AbsoluteValueType,
                        NSTextBlockLayer::Border,
                        NSRectEdge::MaxY,
                    );
                    b.setBorderColor_forEdge(Some(&NSColor::separatorColor()), NSRectEdge::MaxY);
                    b.setWidth_type_forLayer_edge(
                        s * 0.3,
                        NSTextBlockValueType::AbsoluteValueType,
                        NSTextBlockLayer::Padding,
                        NSRectEdge::MaxY,
                    );
                    b.setWidth_type_forLayer_edge(
                        s * 0.5,
                        NSTextBlockValueType::AbsoluteValueType,
                        NSTextBlockLayer::Margin,
                        NSRectEdge::MaxY,
                    );
                    st.setTextBlocks(&NSArray::from_slice(&[&*b]));
                }
            }
            ParaKind::Code { .. } => {
                let b = blocks.groups.entry(style.group).or_insert_with(|| {
                    let b = full_width_block();
                    b.setBackgroundColor(Some(&code_background()));
                    b.setWidth_type_forLayer(
                        s * 0.6,
                        NSTextBlockValueType::AbsoluteValueType,
                        NSTextBlockLayer::Padding,
                    );
                    b.setWidth_type_forLayer_edge(
                        s * 0.7,
                        NSTextBlockValueType::AbsoluteValueType,
                        NSTextBlockLayer::Margin,
                        NSRectEdge::MaxY,
                    );
                    b
                });
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
                let t = blocks.tables.entry(*table as usize).or_insert_with(|| {
                    let t = NSTextTable::new();
                    t.setContentWidth_type(100.0, NSTextBlockValueType::PercentageValueType);
                    t.setNumberOfColumns(*columns as usize);
                    t.setLayoutAlgorithm(NSTextTableLayoutAlgorithm::AutomaticLayoutAlgorithm);
                    t.setCollapsesBorders(true);
                    t.setHidesEmptyCells(false);
                    t.setWidth_type_forLayer_edge(
                        s * 0.7,
                        NSTextBlockValueType::AbsoluteValueType,
                        NSTextBlockLayer::Margin,
                        NSRectEdge::MaxY,
                    );
                    t
                });
                let span = (*span).max(1);
                let column = if style.mirrored {
                    columns.saturating_sub(col + span)
                } else {
                    *col
                };
                let cell = NSTextTableBlock::initWithTable_startingRow_rowSpan_startingColumn_columnSpan(
                    NSTextTableBlock::alloc(),
                    t,
                    *row as isize,
                    1,
                    column as isize,
                    span as isize,
                );
                cell.setWidth_type_forLayer(1.0, NSTextBlockValueType::AbsoluteValueType, NSTextBlockLayer::Border);
                cell.setBorderColor(Some(&NSColor::separatorColor()));
                cell.setWidth_type_forLayer(
                    s * 0.4,
                    NSTextBlockValueType::AbsoluteValueType,
                    NSTextBlockLayer::Padding,
                );
                if *header {
                    cell.setBackgroundColor(Some(&code_background()));
                }
                st.setTextBlocks(&NSArray::from_slice(&[&*cell as &NSTextBlock]));
                st.setAlignment(match align {
                    AlignKey::Left | AlignKey::Natural => NSTextAlignment::Natural,
                    AlignKey::Center => NSTextAlignment::Center,
                    AlignKey::Right => NSTextAlignment::Right,
                });
                st.setParagraphSpacing(0.0);
                st.setHeadIndent(0.0);
                st.setFirstLineHeadIndent(0.0);
            }
            ParaKind::Rule => {
                let b = full_width_block();
                b.setWidth_type_forLayer_edge(
                    1.0,
                    NSTextBlockValueType::AbsoluteValueType,
                    NSTextBlockLayer::Border,
                    NSRectEdge::MaxY,
                );
                b.setBorderColor_forEdge(Some(&NSColor::separatorColor()), NSRectEdge::MaxY);
                b.setWidth_type_forLayer_edge(
                    s * 0.5,
                    NSTextBlockValueType::AbsoluteValueType,
                    NSTextBlockLayer::Margin,
                    NSRectEdge::MaxY,
                );
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
                AlignKey::Center => st.setAlignment(NSTextAlignment::Center),
                AlignKey::Right => st.setAlignment(NSTextAlignment::Right),
                AlignKey::Left => st.setAlignment(NSTextAlignment::Left),
                AlignKey::Natural => {}
            }
        }
        if style.quote_depth > 0 && !matches!(style.kind, ParaKind::Code { .. } | ParaKind::TableCell { .. }) {
            let b = blocks.groups.entry(style.group).or_insert_with(|| {
                let b = full_width_block();
                let edge = if style.mirrored {
                    NSRectEdge::MaxX
                } else {
                    NSRectEdge::MinX
                };
                b.setWidth_type_forLayer_edge(
                    3.0,
                    NSTextBlockValueType::AbsoluteValueType,
                    NSTextBlockLayer::Border,
                    edge,
                );
                let border = style.alert.map_or_else(NSColor::tertiaryLabelColor, alert_color);
                b.setBorderColor_forEdge(Some(&border), edge);
                b.setWidth_type_forLayer_edge(
                    s * 0.7,
                    NSTextBlockValueType::AbsoluteValueType,
                    NSTextBlockLayer::Padding,
                    edge,
                );
                b.setWidth_type_forLayer_edge(
                    s * 0.7,
                    NSTextBlockValueType::AbsoluteValueType,
                    NSTextBlockLayer::Margin,
                    NSRectEdge::MaxY,
                );
                b
            });
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
            code: run.style.code || run.style.html,
            script: run.style.superscript || run.style.subscript,
            bold: run.style.bold,
            italic: run.style.italic,
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
        let mut font = if run.style.code || run.style.html {
            NSFont::monospacedSystemFontOfSize_weight(
                self.base_font(&para.style.kind).pointSize() * 0.9,
                constant!(NSFontWeightRegular),
            )
        } else {
            self.base_font(&para.style.kind)
        };
        if run.style.superscript || run.style.subscript {
            font = NSFont::fontWithDescriptor_size(&font.fontDescriptor(), font.pointSize() * 0.72).unwrap_or(font);
        }
        let manager = NSFontManager::sharedFontManager(self.mtm);
        if run.style.bold {
            font = manager.convertFont_toHaveTrait(&font, NSFontTraitMask::BoldFontMask);
        }
        if run.style.italic {
            font = manager.convertFont_toHaveTrait(&font, NSFontTraitMask::ItalicFontMask);
        }
        font
    }

    fn color_for(&self, run: &Run, para: &Para) -> Retained<NSColor> {
        if run.style.spoiler {
            return spoiler_color();
        }
        if run.style.tag {
            return NSColor::controlAccentColor();
        }
        if run.style.reference {
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
        let color = self.color_for(run, para);
        // SAFETY: Reading AppKit's attribute name constants, paired with values of the types they expect.
        let mut keys: Vec<(&objc2_foundation::NSAttributedStringKey, Retained<AnyObject>)> = unsafe {
            vec![
                (NSFontAttributeName, obj(font.clone())),
                (NSForegroundColorAttributeName, obj(color)),
                (NSParagraphStyleAttributeName, obj(paragraph.retain())),
            ]
        };
        let one = || obj(NSNumber::new_isize(1));
        // SAFETY: Reading AppKit's attribute name constants, paired with values of the types they expect.
        unsafe {
            if run.style.strike {
                keys.push((NSStrikethroughStyleAttributeName, one()));
            }
            if run.style.underline {
                keys.push((NSUnderlineStyleAttributeName, one()));
            }
            if run.style.tag {
                keys.push((
                    NSBackgroundColorAttributeName,
                    obj(NSColor::controlAccentColor().colorWithAlphaComponent(0.12)),
                ));
                keys.push((
                    NSToolTipAttributeName,
                    obj(ns(crate::i18n::t("⌘-Klick sucht den Tag im Dokument"))),
                ));
            }
            if run.style.reference {
                keys.push((
                    NSToolTipAttributeName,
                    obj(ns(crate::i18n::t("⌘-Klick öffnet den Verweis"))),
                ));
            }
            if run.style.spoiler {
                keys.push((NSBackgroundColorAttributeName, obj(spoiler_color())));
                keys.push((
                    NSToolTipAttributeName,
                    obj(ns(crate::i18n::t("Verborgener Text: Zum Lesen markieren"))),
                ));
            } else if run.style.highlight {
                keys.push((NSBackgroundColorAttributeName, obj(highlight_background())));
            } else if run.style.code && !matches!(para.style.kind, ParaKind::Code { .. }) {
                keys.push((NSBackgroundColorAttributeName, obj(code_background())));
            }
            if run.style.superscript || run.style.subscript {
                let offset = font.pointSize() * if run.style.superscript { 0.55 } else { -0.25 };
                keys.push((NSBaselineOffsetAttributeName, obj(NSNumber::new_f64(offset))));
            }
            if let Some(link) = &run.style.link {
                let value: Retained<AnyObject> = match NSURL::URLWithString(&ns(link)) {
                    Some(url) if !link.starts_with("splat-") => obj(url),
                    _ => obj(ns(link)),
                };
                keys.push((NSLinkAttributeName, value));
                if !link.starts_with("splat-") {
                    keys.push((NSUnderlineStyleAttributeName, one()));
                    keys.push((
                        NSToolTipAttributeName,
                        obj(ns(&format!("{link}\n{}", crate::i18n::t("⌘-Klick öffnet den Link")))),
                    ));
                }
            }
        }
        if let RunMap::Atom { atom: Atom::Marker, .. } = &run.map
            && let Some(Marker::Task { .. }) = &para.style.marker
        {
            keys.push((constant!(NSLinkAttributeName), obj(ns("splat-task"))));
        }
        let attachment = match &run.map {
            RunMap::Atom { atom, .. } => self.attachment(atom, &font),
            RunMap::Text { .. } => None,
        };
        if let Some(att) = attachment {
            keys.push((constant!(NSAttachmentAttributeName), obj(att)));
        }
        let pairs: Vec<(&objc2_foundation::NSAttributedStringKey, &AnyObject)> =
            keys.iter().map(|(k, v)| (*k, &**v)).collect();
        let dict = attrs(&pairs);
        // SAFETY: The attributes have the types their keys expect.
        let out = unsafe {
            NSMutableAttributedString::initWithString_attributes(
                NSMutableAttributedString::alloc(),
                &ns(&run.text),
                Some(&dict),
            )
        };
        if let (ParaKind::Code { info }, RunMap::Text { .. }) = (&para.style.kind, &run.map) {
            let map = Utf16Map::new(&run.text);
            for (range, token) in code::tokens(info, &run.text) {
                // SAFETY: A color for the foreground color key, within the run's text.
                unsafe {
                    out.addAttribute_value_range(
                        NSForegroundColorAttributeName,
                        &token_color(token.into()),
                        byte_range_to_ns(&map, &range),
                    )
                };
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
            Atom::Math { tex, display } => js::cached(&js::Key::Math {
                tex: tex.to_string(),
                display: *display,
                dark: self.dark,
            })?,
            Atom::Diagram { source, kind } => js::cached(&js::Key::Diagram {
                source: source.to_string(),
                kind: *kind,
                dark: self.dark,
            })?,
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

pub fn alert_symbol(kind: AlertKind) -> &'static str {
    match kind {
        AlertKind::Note => "info.circle",
        AlertKind::Tip => "lightbulb",
        AlertKind::Important => "exclamationmark.bubble",
        AlertKind::Warning => "exclamationmark.triangle",
        AlertKind::Caution => "exclamationmark.octagon",
    }
}
