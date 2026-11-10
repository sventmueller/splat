use objc2::rc::Retained;
use objc2_app_kit::{
    NSColor, NSFont, NSFontManager, NSFontTraitMask, NSFontWeightBold, NSFontWeightRegular, NSMutableParagraphStyle,
    NSParagraphStyleAttributeName, NSTextStorage, NSWritingDirection,
};
use objc2_foundation::NSRange;
pub use splat_render::style::{appearance_of, token_color};

use super::attributes::{Attribute, Attributes, add_attribute, set_attributes};
use crate::markdown::highlight::{Style, changed_region, right_to_left_behind_markers};
use crate::markdown::text::{Byte, TextAt, Utf16Cursor, line_start};
use crate::util::constant;

pub const SOURCE_SIZE: f64 = 13.0;
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
    old: &[(std::ops::Range<Byte>, Style)],
    text: &str,
    spans: &[(std::ops::Range<Byte>, Style)],
    fonts: &SourceFonts,
) {
    apply_highlight_in(storage, text, spans, changed_region(old_text, old, text, spans), fonts);
}

/// Gives `region` of the source the colours `spans` say, whatever it had before.
pub fn apply_highlight_in(
    storage: &NSTextStorage,
    text: &str,
    spans: &[(std::ops::Range<Byte>, Style)],
    region: std::ops::Range<Byte>,
    fonts: &SourceFonts,
) {
    let (start, end) = (region.start, region.end);
    let (region, styled) = styled_ranges(text, spans, start..end);
    crate::util::editing(storage, || {
        let base = base_source_attributes(fonts);
        set_attributes(storage, &base, region);
        let strike = Attribute::strikethrough();
        for (r, style) in styled {
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
    });
}

/// What is restyled in UTF-16 units: all of `region`, and the part of each span within it.
fn styled_ranges<'a>(
    text: &str,
    spans: &'a [(std::ops::Range<Byte>, Style)],
    region: std::ops::Range<Byte>,
) -> (NSRange, Vec<(NSRange, &'a Style)>) {
    let (start, end) = (region.start, region.end);
    let mut cursor = Utf16Cursor::new(text);
    let u_start = cursor.at(start);
    let u_end = cursor.at(end.max(start));
    let whole = NSRange::new(u_start.get(), u_end - u_start);
    let mut cursor = Utf16Cursor::new(text);
    let styled = spans
        .iter()
        .filter(|(r, _)| r.end > start && r.start < end)
        .filter_map(|(range, style)| {
            let a = cursor.at(range.start.max(start));
            let b = cursor.at(range.end.min(end));
            let r = NSRange::new(a.get(), b - a);
            (r.length > 0).then_some((r, style))
        })
        .collect();
    (whole, styled)
}

/// What becomes of the direction of a source line.
#[derive(Clone, Copy, Debug, PartialEq)]
enum LineDirection {
    RightToLeft,
    /// A line begun before the restyled region would keep a stale direction.
    Cleared,
}

/// The lines around `region` whose direction has to be set, in UTF-16 units.
fn line_directions(text: &str, region: std::ops::Range<Byte>) -> Vec<(NSRange, LineDirection)> {
    let mut lines = Vec::new();
    let mut cursor = Utf16Cursor::new(text);
    let mut at = line_start(text, region.start);
    while at < text.end() && at <= region.end {
        let stop = text.from(at).find('\n').map_or(text.end(), |i| at + i);
        let (a, b) = (cursor.at(at), cursor.at(stop));
        let line = NSRange::new(a.get(), b - a);
        if right_to_left_behind_markers(text.at(at..stop)) {
            lines.push((line, LineDirection::RightToLeft));
        } else if at < region.start {
            lines.push((line, LineDirection::Cleared));
        }
        at = stop + 1;
    }
    lines
}

/// TextKit takes a source line's direction from its first letter, for a done task the x of `[x]`.
fn set_line_directions(storage: &NSTextStorage, text: &str, region: std::ops::Range<Byte>) {
    let rtl = NSMutableParagraphStyle::new();
    rtl.setBaseWritingDirection(NSWritingDirection::RightToLeft);
    let rtl = Attribute::paragraph(&rtl);
    for (line, direction) in line_directions(text, region) {
        match direction {
            LineDirection::RightToLeft => add_attribute(storage, &rtl, line),
            LineDirection::Cleared => storage.removeAttribute_range(constant!(NSParagraphStyleAttributeName), line),
        }
    }
}

#[cfg(test)]
mod tests {
    use objc2_app_kit::NSFontDescriptorSymbolicTraits;

    use super::*;
    use crate::markdown::text::bytes;
    use crate::util::ns;

    #[test]
    fn a_style_picks_its_cut_of_the_source_font() {
        let font = |size| NSFont::systemFontOfSize(size);
        let fonts = SourceFonts {
            regular: font(1.0),
            bold: font(2.0),
            italic: font(3.0),
            bold_italic: font(4.0),
        };
        assert_eq!(fonts.pick(false, false).pointSize(), 1.0);
        assert_eq!(fonts.pick(true, false).pointSize(), 2.0);
        assert_eq!(fonts.pick(false, true).pointSize(), 3.0);
        assert_eq!(fonts.pick(true, true).pointSize(), 4.0);
        assert_eq!(fonts.regular().pointSize(), 1.0);
    }

    const BOLD: Style = Style {
        token: None,
        bold: true,
        italic: false,
        strike: false,
    };

    #[test]
    fn a_restyled_region_cuts_the_spans_to_itself_and_counts_in_utf16() {
        let text = "ä𝄞 fett und mehr";
        let span = |range| (bytes(range), BOLD);
        let spans = [span(0..6), span(7..11), span(16..20)];
        let (whole, styled) = styled_ranges(text, &spans, bytes(2..13));
        assert_eq!(
            whole,
            NSRange::new(1, 9),
            "ä is one unit, 𝄞 two, and each of the rest one"
        );
        let ranges: Vec<NSRange> = styled.iter().map(|(r, _)| *r).collect();
        assert_eq!(ranges, [NSRange::new(1, 2), NSRange::new(4, 4)]);
        let (whole, styled) = styled_ranges(text, &spans, bytes(6..7));
        assert_eq!(whole, NSRange::new(3, 1));
        assert!(styled.is_empty(), "the space between the spans");
    }

    #[test]
    fn only_lines_that_read_from_the_right_get_a_direction() {
        let text = "Links\n- [x] שלום\nauch links\n- [x] مرحبا\n";
        let lines = |region| line_directions(text, region);
        assert_eq!(
            lines(bytes(0..3)),
            [],
            "a line that starts in the region has just lost its attributes"
        );
        assert_eq!(
            lines(bytes(0..text.len())),
            [
                (NSRange::new(6, 10), LineDirection::RightToLeft),
                (NSRange::new(28, 11), LineDirection::RightToLeft),
            ]
        );
        let inside_second_line = "Links\n- [x] שלום\n".len() - 3;
        assert_eq!(
            lines(bytes(inside_second_line..inside_second_line)),
            [(NSRange::new(6, 10), LineDirection::RightToLeft)]
        );
        assert_eq!(
            lines(bytes(23..24)),
            [(NSRange::new(17, 10), LineDirection::Cleared)],
            "a line begun before the region may have changed"
        );
        assert_eq!(
            line_directions("שלום\n", bytes(0..4)),
            [],
            "without a marker in front, its first letter says it"
        );
    }

    /// Without a view: colours, fonts and directions as the storage holds them.
    #[test]
    fn the_changes_of_a_highlight_reach_the_text_storage() {
        let manager_free_fonts = SourceFonts {
            regular: NSFont::monospacedSystemFontOfSize_weight(13.0, constant!(NSFontWeightRegular)),
            bold: NSFont::monospacedSystemFontOfSize_weight(13.0, constant!(NSFontWeightBold)),
            italic: NSFont::systemFontOfSize(13.0),
            bold_italic: NSFont::boldSystemFontOfSize(13.0),
        };
        let text = "**fett** und\n- [x] שלום\n";
        let storage = NSTextStorage::new();
        storage.replaceCharactersInRange_withString(NSRange::new(0, 0), &ns(text));
        let spans = [(bytes(0..8), BOLD)];
        apply_highlight_changes(&storage, "", &[], text, &spans, &manager_free_fonts);
        let font_key = constant!(objc2_app_kit::NSFontAttributeName);
        let font_at = |i| {
            // SAFETY: `i` lies inside the text, and the range pointer may be null.
            unsafe { storage.attribute_atIndex_effectiveRange(font_key, i, std::ptr::null_mut()) }
                .and_then(|v| v.downcast::<NSFont>().ok())
                .expect("a font")
        };
        let bold = |i| {
            font_at(i)
                .fontDescriptor()
                .symbolicTraits()
                .contains(NSFontDescriptorSymbolicTraits::TraitBold)
        };
        assert!(bold(3));
        assert!(!bold(9));
        let style_key = constant!(NSParagraphStyleAttributeName);
        let direction_at = |i| {
            // SAFETY: `i` lies inside the text, and the range pointer may be null.
            unsafe { storage.attribute_atIndex_effectiveRange(style_key, i, std::ptr::null_mut()) }
                .and_then(|v| v.downcast::<objc2_app_kit::NSParagraphStyle>().ok())
                .map(|p| p.baseWritingDirection())
        };
        assert_eq!(direction_at(0), None);
        assert_eq!(direction_at(14), Some(NSWritingDirection::RightToLeft));
    }
}
