//! Text attributes. Nothing outside this file names a key together with a value, so AppKit never gets a value of a
//! type its key does not expect.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, ClassType};
use objc2_app_kit::{
    NSAttachmentAttributeName, NSBackgroundColorAttributeName, NSBaselineOffsetAttributeName, NSColor, NSFont,
    NSFontAttributeName, NSForegroundColorAttributeName, NSLayoutManager, NSLinkAttributeName, NSParagraphStyle,
    NSParagraphStyleAttributeName, NSStrikethroughStyleAttributeName, NSTextAttachment, NSTextView,
    NSToolTipAttributeName, NSUnderlineStyleAttributeName,
};
use objc2_foundation::{
    NSAttributedString, NSAttributedStringKey, NSDictionary, NSMutableAttributedString, NSNumber, NSRange, NSURL,
};

use crate::util::{constant, ns, obj};

pub struct Attribute {
    key: &'static NSAttributedStringKey,
    value: Retained<AnyObject>,
}

impl Attribute {
    fn new<T: ClassType + 'static>(key: &'static NSAttributedStringKey, value: &T) -> Attribute {
        Attribute {
            key,
            value: obj(value.retain()),
        }
    }

    pub fn font(font: &NSFont) -> Attribute {
        Attribute::new(constant!(NSFontAttributeName), font)
    }

    pub fn foreground(color: &NSColor) -> Attribute {
        Attribute::new(constant!(NSForegroundColorAttributeName), color)
    }

    pub fn background(color: &NSColor) -> Attribute {
        Attribute::new(constant!(NSBackgroundColorAttributeName), color)
    }

    pub fn paragraph(style: &NSParagraphStyle) -> Attribute {
        Attribute::new(constant!(NSParagraphStyleAttributeName), style)
    }

    pub fn strikethrough() -> Attribute {
        Attribute::new(constant!(NSStrikethroughStyleAttributeName), &*NSNumber::new_isize(1))
    }

    pub fn underline() -> Attribute {
        Attribute::new(constant!(NSUnderlineStyleAttributeName), &*NSNumber::new_isize(1))
    }

    pub fn baseline_offset(offset: f64) -> Attribute {
        Attribute::new(constant!(NSBaselineOffsetAttributeName), &*NSNumber::new_f64(offset))
    }

    pub fn tool_tip(text: &str) -> Attribute {
        Attribute::new(constant!(NSToolTipAttributeName), &*ns(text))
    }

    pub fn link(url: &NSURL) -> Attribute {
        Attribute::new(constant!(NSLinkAttributeName), url)
    }

    pub fn link_text(target: &str) -> Attribute {
        Attribute::new(constant!(NSLinkAttributeName), &*ns(target))
    }

    pub fn attachment(attachment: &NSTextAttachment) -> Attribute {
        Attribute::new(constant!(NSAttachmentAttributeName), attachment)
    }
}

pub struct Attributes(Retained<NSDictionary<NSAttributedStringKey, AnyObject>>);

impl Attributes {
    pub fn new(attributes: &[Attribute]) -> Attributes {
        let keys: Vec<&NSAttributedStringKey> = attributes.iter().map(|a| a.key).collect();
        let values: Vec<&AnyObject> = attributes.iter().map(|a| &*a.value).collect();
        Attributes(NSDictionary::from_slices(&keys, &values))
    }
}

pub fn attributed(text: &str, attributes: Option<&Attributes>) -> Retained<NSAttributedString> {
    // SAFETY: `Attribute` gives every key a value of the type it expects.
    unsafe {
        NSAttributedString::initWithString_attributes(NSAttributedString::alloc(), &ns(text), attributes.map(|a| &*a.0))
    }
}

pub fn attributed_mut(text: &str, attributes: &Attributes) -> Retained<NSMutableAttributedString> {
    // SAFETY: `Attribute` gives every key a value of the type it expects.
    unsafe {
        NSMutableAttributedString::initWithString_attributes(
            NSMutableAttributedString::alloc(),
            &ns(text),
            Some(&attributes.0),
        )
    }
}

pub fn add_attribute(text: &NSMutableAttributedString, attribute: &Attribute, range: NSRange) {
    // SAFETY: `Attribute` gives every key a value of the type it expects.
    unsafe { text.addAttribute_value_range(attribute.key, &attribute.value, range) };
}

pub fn set_attributes(text: &NSMutableAttributedString, attributes: &Attributes, range: NSRange) {
    // SAFETY: `Attribute` gives every key a value of the type it expects.
    unsafe { text.setAttributes_range(Some(&attributes.0), range) };
}

pub fn add_temporary_attribute(layout: &NSLayoutManager, attribute: &Attribute, range: NSRange) {
    // SAFETY: `Attribute` gives every key a value of the type it expects.
    unsafe { layout.addTemporaryAttribute_value_forCharacterRange(attribute.key, &attribute.value, range) };
}

pub fn set_typing_attributes(view: &NSTextView, attributes: &Attributes) {
    // SAFETY: `Attribute` gives every key a value of the type it expects.
    unsafe { view.setTypingAttributes(&attributes.0) };
}

#[cfg(test)]
mod tests {
    use objc2::DowncastTarget;
    use objc2_foundation::NSString;

    use super::*;

    fn check<T: DowncastTarget>(attribute: Attribute, key: &NSAttributedStringKey) {
        assert_eq!(attribute.key, key);
        assert!(attribute.value.downcast_ref::<T>().is_some(), "value of {key}");
    }

    #[test]
    fn every_constructor_gives_its_key_the_value_type_appkit_expects() {
        let color = NSColor::redColor();
        check::<NSFont>(
            Attribute::font(&NSFont::systemFontOfSize(12.0)),
            constant!(NSFontAttributeName),
        );
        check::<NSColor>(Attribute::foreground(&color), constant!(NSForegroundColorAttributeName));
        check::<NSColor>(Attribute::background(&color), constant!(NSBackgroundColorAttributeName));
        check::<NSParagraphStyle>(
            Attribute::paragraph(&NSParagraphStyle::defaultParagraphStyle()),
            constant!(NSParagraphStyleAttributeName),
        );
        check::<NSNumber>(Attribute::strikethrough(), constant!(NSStrikethroughStyleAttributeName));
        check::<NSNumber>(Attribute::underline(), constant!(NSUnderlineStyleAttributeName));
        check::<NSNumber>(
            Attribute::baseline_offset(0.5),
            constant!(NSBaselineOffsetAttributeName),
        );
        check::<NSString>(Attribute::tool_tip("tip"), constant!(NSToolTipAttributeName));
        let url = NSURL::URLWithString(&ns("https://example.com")).expect("url");
        check::<NSURL>(Attribute::link(&url), constant!(NSLinkAttributeName));
        check::<NSString>(Attribute::link_text("splat-task"), constant!(NSLinkAttributeName));
        check::<NSTextAttachment>(
            Attribute::attachment(&NSTextAttachment::new()),
            constant!(NSAttachmentAttributeName),
        );
    }
}
