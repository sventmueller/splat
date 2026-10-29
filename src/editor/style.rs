use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAccessibilityHighContrastAqua, NSAppearanceNameAccessibilityHighContrastDarkAqua,
    NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSColor, NSFont, NSFontAttributeName, NSFontManager,
    NSFontTraitMask, NSFontWeightBold, NSFontWeightRegular, NSForegroundColorAttributeName,
    NSStrikethroughStyleAttributeName, NSTextStorage,
};
use objc2_foundation::{NSArray, NSAttributedStringKey, NSDictionary, NSNumber, NSRange};

use crate::markdown::highlight::{Style, Token, changed_region};
use crate::markdown::palette::{self, Appearance, Rgb};
use crate::markdown::text::Utf16Cursor;
use crate::util::constant;

pub const SOURCE_SIZE: f64 = 13.0;
pub const PREVIEW_SIZE: f64 = 15.0;

pub fn attrs(
    pairs: &[(&NSAttributedStringKey, &AnyObject)],
) -> Retained<NSDictionary<NSAttributedStringKey, AnyObject>> {
    let keys: Vec<&NSAttributedStringKey> = pairs.iter().map(|(k, _)| *k).collect();
    let values: Vec<&AnyObject> = pairs.iter().map(|(_, v)| *v).collect();
    NSDictionary::from_slices(&keys, &values)
}

pub fn appearance_of(appearance: &NSAppearance) -> Appearance {
    // SAFETY: Reading AppKit's appearance name constants.
    let names = unsafe {
        NSArray::from_slice(&[
            NSAppearanceNameAqua,
            NSAppearanceNameDarkAqua,
            NSAppearanceNameAccessibilityHighContrastAqua,
            NSAppearanceNameAccessibilityHighContrastDarkAqua,
        ])
    };
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

pub fn base_source_attributes(fonts: &SourceFonts) -> Retained<NSDictionary<NSAttributedStringKey, AnyObject>> {
    let color = NSColor::textColor();
    // SAFETY: Reading AppKit's attribute name constants, paired with values of the types they expect.
    unsafe {
        attrs(&[
            (NSFontAttributeName, fonts.regular()),
            (NSForegroundColorAttributeName, &color),
        ])
    }
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
    // SAFETY: A font and a color for their keys, within the text.
    unsafe { storage.setAttributes_range(Some(&base), region) };
    let strike = NSNumber::new_isize(1);
    for (range, style) in spans.iter().filter(|(r, _)| r.end > start && r.start < end) {
        let a = cursor.at(range.start.max(start));
        let b = cursor.at(range.end.min(end));
        let r = NSRange::new(a, b - a);
        if r.length == 0 {
            continue;
        }
        // SAFETY: Each value has the type its key expects, within the text.
        unsafe {
            if style.bold || style.italic {
                storage.addAttribute_value_range(NSFontAttributeName, fonts.pick(style.bold, style.italic), r);
            }
            if let Some(token) = style.token {
                storage.addAttribute_value_range(NSForegroundColorAttributeName, &token_color(token), r);
            }
            if style.strike {
                storage.addAttribute_value_range(NSStrikethroughStyleAttributeName, &strike, r);
            }
        }
    }
    storage.endEditing();
}
