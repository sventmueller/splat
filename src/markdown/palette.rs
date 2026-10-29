use super::highlight::Token;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb(pub u8, pub u8, pub u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    Light,
    Dark,
    LightHighContrast,
    DarkHighContrast,
}

/// Background the light colors are tuned against (NSColor.textBackgroundColor, Aqua).
#[cfg(test)]
pub const LIGHT_BACKGROUND: Rgb = Rgb(255, 255, 255);
/// Background the dark colors are tuned against (NSColor.textBackgroundColor, Dark Aqua).
#[cfg(test)]
pub const DARK_BACKGROUND: Rgb = Rgb(30, 30, 30);

/// VS Code Light+/Dark+ hues, darkened or lightened until each reaches the HIG's 7:1 contrast target.
pub fn color(token: Token, appearance: Appearance) -> Rgb {
    let (light, dark) = match token {
        Token::Heading => (Rgb(0x80, 0x00, 0x00), Rgb(0x7F, 0xB5, 0xE8)),
        Token::Punctuation => (Rgb(0x54, 0x54, 0x54), Rgb(0xAD, 0xAD, 0xAD)),
        Token::Code => (Rgb(0xA3, 0x15, 0x15), Rgb(0xDD, 0xA8, 0x8F)),
        Token::LinkText => (Rgb(0x04, 0x51, 0xA5), Rgb(0x4F, 0xC1, 0xFF)),
        Token::Url => (Rgb(0x0E, 0x5A, 0x6C), Rgb(0x6F, 0xD3, 0xC2)),
        Token::ListMarker => (Rgb(0x04, 0x51, 0xA5), Rgb(0x8F, 0xB2, 0xF0)),
        Token::Quote => (Rgb(0x00, 0x64, 0x00), Rgb(0x8C, 0xC0, 0x7A)),
        Token::Html => (Rgb(0x80, 0x00, 0x80), Rgb(0xD7, 0xA6, 0xD3)),
        Token::Math => (Rgb(0x5E, 0x48, 0x14), Rgb(0xDC, 0xDC, 0xAA)),
        Token::Keyword => (Rgb(0x00, 0x00, 0xFF), Rgb(0x9C, 0xC8, 0xF0)),
        Token::String => (Rgb(0xA3, 0x15, 0x15), Rgb(0xDD, 0xA8, 0x8F)),
        Token::Comment => (Rgb(0x00, 0x64, 0x00), Rgb(0x8C, 0xC0, 0x7A)),
        Token::Number => (Rgb(0x05, 0x5E, 0x3E), Rgb(0xB5, 0xCE, 0xA8)),
    };
    match appearance {
        Appearance::Light => light,
        Appearance::Dark => dark,
        Appearance::LightHighContrast => mix(light, Rgb(0, 0, 0), 0.4),
        Appearance::DarkHighContrast => mix(dark, Rgb(255, 255, 255), 0.4),
    }
}

fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let m = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t).round() as u8;
    Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

#[cfg(test)]
fn luminance(c: Rgb) -> f64 {
    let ch = |v: u8| {
        let s = v as f64 / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(c.0) + 0.7152 * ch(c.1) + 0.0722 * ch(c.2)
}

#[cfg(test)]
fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Token; 13] = [
        Token::Heading,
        Token::Punctuation,
        Token::Code,
        Token::LinkText,
        Token::Url,
        Token::ListMarker,
        Token::Quote,
        Token::Html,
        Token::Math,
        Token::Keyword,
        Token::String,
        Token::Comment,
        Token::Number,
    ];

    #[test]
    fn every_token_reaches_seven_to_one() {
        for t in ALL {
            let l = contrast(color(t, Appearance::Light), LIGHT_BACKGROUND);
            let d = contrast(color(t, Appearance::Dark), DARK_BACKGROUND);
            assert!(l >= 7.0, "{t:?} light {l:.2}");
            assert!(d >= 7.0, "{t:?} dark {d:.2}");
            assert!(contrast(color(t, Appearance::LightHighContrast), LIGHT_BACKGROUND) > l);
            assert!(contrast(color(t, Appearance::DarkHighContrast), DARK_BACKGROUND) > d);
        }
    }
}
