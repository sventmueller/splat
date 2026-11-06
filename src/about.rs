//! The About window: macOS's standard panel, with the project's address and its license.

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSAboutPanelOptionCredits, NSAboutPanelOptionVersion, NSApplication, NSFont, NSMutableParagraphStyle,
    NSTextAlignment,
};
use objc2_foundation::{NSDictionary, NSMutableAttributedString, NSRange, NSURL};

use crate::editor::attributes::{Attribute, Attributes, add_attribute, attributed_mut};
use crate::i18n::t;
use crate::markdown::text::utf16_len;
use crate::util::{constant, ns, obj};

pub fn show(mtm: MainThreadMarker) {
    let keys = [
        constant!(NSAboutPanelOptionVersion),
        constant!(NSAboutPanelOptionCredits),
    ];
    // The version ends in the build number already, which the panel would repeat in parentheses.
    let values = [obj(ns("")), obj(credits())];
    let values: Vec<&AnyObject> = values.iter().map(|v| &**v).collect();
    let options = NSDictionary::from_slices(&keys, &values);
    // SAFETY: the panel takes a string for the version and an attributed string for the credits.
    unsafe { NSApplication::sharedApplication(mtm).orderFrontStandardAboutPanelWithOptions(&options) };
}

/// The address of the project and its license, each license name linked to its text.
fn credits() -> Retained<NSMutableAttributedString> {
    let repository = crate::feedback::REPOSITORY;
    let address = repository.trim_start_matches("https://");
    let shown = format!("{address}\n{}", t("Lizenz: MIT oder Apache 2.0"));
    let centered = NSMutableParagraphStyle::new();
    centered.setAlignment(NSTextAlignment::Center);
    let text = attributed_mut(
        &shown,
        &Attributes::new(&[
            Attribute::font(&NSFont::systemFontOfSize(NSFont::smallSystemFontSize())),
            Attribute::paragraph(&centered),
        ]),
    );
    let links = [
        (address, repository.to_string()),
        ("MIT", format!("{repository}/blob/main/LICENSE-MIT")),
        ("Apache 2.0", format!("{repository}/blob/main/LICENSE-APACHE")),
    ];
    for (name, target) in links {
        let (Some(start), Some(url)) = (shown.find(name), NSURL::URLWithString(&ns(&target))) else {
            continue;
        };
        let range = NSRange::new(utf16_len(&shown[..start]), utf16_len(name));
        add_attribute(&text, &Attribute::link(&url), range);
    }
    text
}
