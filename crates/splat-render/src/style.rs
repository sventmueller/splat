//! How the preview's text looks: its size, and the colours of syntax, which follow the appearance as it changes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAccessibilityHighContrastAqua, NSAppearanceNameAccessibilityHighContrastDarkAqua,
    NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSColor,
};
use objc2_foundation::NSArray;
use splat_markdown::highlight::Token;
use splat_markdown::palette::{self, Appearance, Rgb};

use crate::appkit::constant;

pub const PREVIEW_SIZE: f64 = 15.0;

pub fn appearance_of(appearance: &NSAppearance) -> Appearance {
    let names = NSArray::from_slice(&[
        constant!(NSAppearanceNameAqua),
        constant!(NSAppearanceNameDarkAqua),
        constant!(NSAppearanceNameAccessibilityHighContrastAqua),
        constant!(NSAppearanceNameAccessibilityHighContrastDarkAqua),
    ]);
    let best = appearance.bestMatchFromAppearancesWithNames(&names);
    appearance_named(&best.map(|n| n.to_string()).unwrap_or_default())
}

fn appearance_named(name: &str) -> Appearance {
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
                    let color = crate::lanes::guarded(NSColor::labelColor(), || {
                        // SAFETY: `NonNull` rules out null, and AppKit keeps the appearance alive while it asks for its
                        // color.
                        let appearance = unsafe { appearance.as_ref() };
                        srgb(palette::color(token, appearance_of(appearance)))
                    });
                    NonNull::new(Retained::autorelease_return(color)).unwrap()
                });
                // SAFETY: The provider only reads the appearance it is given and returns a color.
                unsafe { NSColor::colorWithName_dynamicProvider(None, &block) }
            })
            .clone()
    })
}

#[cfg(test)]
mod tests {
    use objc2_app_kit::NSColorSpace;

    use super::*;

    fn named(name: &objc2_app_kit::NSAppearanceName) -> Retained<NSAppearance> {
        NSAppearance::appearanceNamed(name).expect("an appearance of the system")
    }

    #[test]
    fn each_appearance_of_the_system_has_its_palette() {
        assert_eq!(
            appearance_of(&named(constant!(NSAppearanceNameAqua))),
            Appearance::Light
        );
        assert_eq!(
            appearance_of(&named(constant!(NSAppearanceNameDarkAqua))),
            Appearance::Dark
        );
        let by_name = |name: &objc2_app_kit::NSAppearanceName| appearance_named(&name.to_string());
        assert_eq!(by_name(constant!(NSAppearanceNameAqua)), Appearance::Light);
        assert_eq!(by_name(constant!(NSAppearanceNameDarkAqua)), Appearance::Dark);
        assert_eq!(
            by_name(constant!(NSAppearanceNameAccessibilityHighContrastAqua)),
            Appearance::LightHighContrast
        );
        assert_eq!(
            by_name(constant!(NSAppearanceNameAccessibilityHighContrastDarkAqua)),
            Appearance::DarkHighContrast
        );
        assert_eq!(appearance_named(""), Appearance::Light, "no match among them");
    }

    /// The components of `color` as the given appearance resolves it.
    fn resolved(color: &NSColor, name: &objc2_app_kit::NSAppearanceName) -> Rgb {
        let out = std::cell::Cell::new(Rgb(0, 0, 0));
        let block = RcBlock::new(|| {
            let c = color
                .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
                .expect("an sRGB color");
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "components are 0 to 1"
            )]
            let byte = |v: f64| (v * 255.0).round() as u8;
            out.set(Rgb(
                byte(c.redComponent()),
                byte(c.greenComponent()),
                byte(c.blueComponent()),
            ));
        });
        named(name).performAsCurrentDrawingAppearance(&block);
        out.get()
    }

    #[test]
    fn a_token_takes_its_color_from_the_palette_of_the_appearance() {
        for token in [Token::Heading, Token::Code, Token::LinkText] {
            let color = token_color(token);
            for (name, appearance) in [
                (constant!(NSAppearanceNameAqua), Appearance::Light),
                (constant!(NSAppearanceNameDarkAqua), Appearance::Dark),
            ] {
                assert_eq!(
                    resolved(&color, name),
                    palette::color(token, appearance),
                    "{token:?} {appearance:?}"
                );
            }
        }
    }

    #[test]
    fn colors_of_the_palette_reach_appkit_unchanged() {
        let c = srgb(Rgb(255, 128, 0));
        assert_eq!(c.redComponent(), 1.0);
        assert_eq!(c.greenComponent(), 128.0 / 255.0);
        assert_eq!(c.blueComponent(), 0.0);
        assert_eq!(c.alphaComponent(), 1.0);
    }
}
