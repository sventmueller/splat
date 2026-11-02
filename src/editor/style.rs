use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAccessibilityHighContrastAqua, NSAppearanceNameAccessibilityHighContrastDarkAqua,
    NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSColor, NSFont, NSFontManager, NSFontTraitMask, NSFontWeightBold,
    NSFontWeightRegular, NSMutableParagraphStyle, NSParagraphStyleAttributeName, NSTextStorage, NSWritingDirection,
};
use objc2_foundation::{NSArray, NSRange};

use super::attributes::{Attribute, Attributes, add_attribute, set_attributes};
use crate::markdown::highlight::{Style, Token, changed_region, right_to_left_behind_markers};
use crate::markdown::palette::{self, Appearance, Rgb};
use crate::markdown::text::{Utf16Cursor, line_start};
use crate::util::constant;

pub const SOURCE_SIZE: f64 = 13.0;
pub const PREVIEW_SIZE: f64 = 15.0;

pub fn appearance_of(appearance: &NSAppearance) -> Appearance {
    let names = NSArray::from_slice(&[
        constant!(NSAppearanceNameAqua),
        constant!(NSAppearanceNameDarkAqua),
        constant!(NSAppearanceNameAccessibilityHighContrastAqua),
        constant!(NSAppearanceNameAccessibilityHighContrastDarkAqua),
    ]);
    let best = appearance.bestMatchFromAppearancesWithNames(&names);
    let name = best.map(|n| n.to_string()).unwrap_or_default();
    if name == constant!(NSAppearanceNameAccessibilityHighContrastDarkAqua).to_string() {
        Appearance::DarkHighContrast
    } else if name == constant!(NSAppearanceNameAccessibilityHighContrastAqua).to_string() {
        Appearance::LightHighContrast
    } else if name == constant!(NSAppearanceNameDarkAqua).to_string() {
        Appearance::Dark
    } else {
        Appearance::Light
    }
}

fn srgb(c: Rgb) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(c.0 as f64 / 255.0, c.1 as f64 / 255.0, c.2 as f64 / 255.0, 1.0)
}

thread_local! {
    static TOKEN_COLORS: RefCell<HashMap<Token, Retained<NSColor>>> = RefCell::new(HashMap::new());
}

/// A color that resolves per appearance, including the Increase Contrast variants.
pub fn token_color(token: Token) -> Retained<NSColor> {
    TOKEN_COLORS.with(|cache| {
        cache
            .borrow_mut()
            .entry(token)
            .or_insert_with(|| {
                let block = RcBlock::new(move |appearance: NonNull<NSAppearance>| -> NonNull<NSColor> {
                    // SAFETY: AppKit passes a valid appearance or null.
                    let appearance = unsafe { appearance.as_ref() };
                    let color = srgb(palette::color(token, appearance_of(appearance)));
                    NonNull::new(Retained::autorelease_return(color)).unwrap()
                });
                // SAFETY: The provider only reads the appearance it is given and returns a color.
                unsafe { NSColor::colorWithName_dynamicProvider(None, &block) }
            })
            .clone()
    })
}

#[derive(Clone)]
pub struct SourceFonts {
    regular: Retained<NSFont>,
    bold: Retained<NSFont>,
    italic: Retained<NSFont>,
    bold_italic: Retained<NSFont>,
}

impl SourceFonts {
    pub fn new(zoom: f64) -> Self {
        let size = SOURCE_SIZE * zoom;
        let regular = NSFont::monospacedSystemFontOfSize_weight(size, constant!(NSFontWeightRegular));
        let bold = NSFont::monospacedSystemFontOfSize_weight(size, constant!(NSFontWeightBold));
        let manager = NSFontManager::sharedFontManager(objc2::MainThreadMarker::new().unwrap());
        let italic = manager.convertFont_toHaveTrait(&regular, NSFontTraitMask::ItalicFontMask);
        let bold_italic = manager.convertFont_toHaveTrait(&bold, NSFontTraitMask::ItalicFontMask);
        SourceFonts {
            regular,
            bold,
            italic,
            bold_italic,
        }
    }

    pub fn regular(&self) -> &NSFont {
        &self.regular
    }

    fn pick(&self, bold: bool, italic: bool) -> &NSFont {
        match (bold, italic) {
            (true, true) => &self.bold_italic,
            (true, false) => &self.bold,
            (false, true) => &self.italic,
            (false, false) => &self.regular,
        }
    }
}

pub fn base_source_attributes(fonts: &SourceFonts) -> Attributes {
    Attributes::new(&[
        Attribute::font(fonts.regular()),
        Attribute::foreground(&NSColor::textColor()),
    ])
}

/// Re-applies syntax colors only where they can have changed: around the edit and wherever the new spans differ
/// from the previous ones. Setting attributes on the whole text would make TextKit lay out the whole document again.
pub fn apply_highlight_changes(
    storage: &NSTextStorage,
    old_text: &str,
    old: &[(std::ops::Range<usize>, Style)],
    text: &str,
    spans: &[(std::ops::Range<usize>, Style)],
    fonts: &SourceFonts,
) {
    let (start, end) = {
        let r = changed_region(old_text, old, text, spans);
        (r.start, r.end)
    };
    let mut cursor = Utf16Cursor::new(text);
    let u_start = cursor.at(start);
    let u_end = cursor.at(end.max(start));
    let region = NSRange::new(u_start, u_end - u_start);
    let mut cursor = Utf16Cursor::new(text);
    storage.beginEditing();
    let base = base_source_attributes(fonts);
    set_attributes(storage, &base, region);
    let strike = Attribute::strikethrough();
    for (range, style) in spans.iter().filter(|(r, _)| r.end > start && r.start < end) {
        let a = cursor.at(range.start.max(start));
        let b = cursor.at(range.end.min(end));
        let r = NSRange::new(a, b - a);
        if r.length == 0 {
            continue;
        }
        if style.bold || style.italic {
            add_attribute(storage, &Attribute::font(fonts.pick(style.bold, style.italic)), r);
        }
        if let Some(token) = style.token {
            add_attribute(storage, &Attribute::foreground(&token_color(token)), r);
        }
        if style.strike {
            add_attribute(storage, &strike, r);
        }
    }
    set_line_directions(storage, text, start..end.max(start));
    storage.endEditing();
}

/// TextKit takes a source line's direction from its first letter, for a done task the x of `[x]`; a line begun
/// before the restyled region would keep a stale direction, so it is cleared.
fn set_line_directions(storage: &NSTextStorage, text: &str, region: std::ops::Range<usize>) {
    let rtl = NSMutableParagraphStyle::new();
    rtl.setBaseWritingDirection(NSWritingDirection::RightToLeft);
    let rtl = Attribute::paragraph(&rtl);
    let mut cursor = Utf16Cursor::new(text);
    let mut at = line_start(text, region.start);
    while at < text.len() && at <= region.end {
        let stop = text[at..].find('\n').map_or(text.len(), |i| at + i);
        let (a, b) = (cursor.at(at), cursor.at(stop));
        let line = NSRange::new(a, b - a);
        if right_to_left_behind_markers(&text[at..stop]) {
            add_attribute(storage, &rtl, line);
        } else if at < region.start {
            storage.removeAttribute_range(constant!(NSParagraphStyleAttributeName), line);
        }
        at = stop + 1;
    }
}
