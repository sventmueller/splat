//! What the bytes of an RTF document's text stand for: the code page the header names with `\ansicpg`, `\mac`, `\pc` or
//! `\pca`, or the one a font's `\fcharset` or `\cpg` names. A character of a double-byte code page such as Shift-JIS
//! comes as two bytes, each written as `\'hh` or as itself.

use encoding_rs::{Decoder, Encoding};

/// Windows-1252, which `\ansi` names and which stands in for a code page this reader does not know.
pub(super) const WESTERN: u16 = 1252;
/// Mac OS Roman, which `\mac` names.
pub(super) const MAC_ROMAN: u16 = 10_000;
/// The code page of IBM PCs in the US, which `\pc` names.
pub(super) const DOS_US: u16 = 437;
/// The Western European code page of DOS, which `\pca` names.
pub(super) const DOS_LATIN_1: u16 = 850;

/// The code page a font's `\fcharset` names, or `None` for a font that takes the document's. ANSI (0) is one of those,
/// as Cocoa marks every font so and writes `\'hh` in the code page of `\ansicpg`. Symbol (2) numbers glyphs, not
/// characters, so its bytes are read as Windows-1252.
pub(super) fn of_charset(charset: i32) -> Option<u16> {
    Some(match charset {
        2 => WESTERN,
        77 => MAC_ROMAN,
        78 | 128 => 932,
        79 | 129 => 949,
        80 | 134 => 936,
        81 | 136 => 950,
        89 => 10_007,
        161 => 1253,
        162 => 1254,
        163 => 1258,
        177 | 181 => 1255,
        178..=180 => 1256,
        186 => 1257,
        204 => 1251,
        222 => 874,
        238 => 1250,
        254 => DOS_US,
        255 => DOS_LATIN_1,
        _ => return None,
    })
}

/// How the bytes of a code page become characters.
enum Decoding {
    /// A code page of DOS, by its upper half; the lower half is ASCII.
    Table(&'static [char; 128]),
    /// Any other, through encoding_rs.
    Encoding(&'static Encoding),
}

/// The decoding of a Windows code page number; one this reader does not know is read as Windows-1252.
fn decoding(code_page: u16) -> Decoding {
    Decoding::Encoding(match code_page {
        DOS_US => return Decoding::Table(&DOS_US_UPPER),
        DOS_LATIN_1 => return Decoding::Table(&DOS_LATIN_1_UPPER),
        866 => encoding_rs::IBM866,
        874 => encoding_rs::WINDOWS_874,
        932 | 10_001 => encoding_rs::SHIFT_JIS,
        936 | 10_008 => encoding_rs::GBK,
        949 | 10_003 => encoding_rs::EUC_KR,
        950 | 10_002 => encoding_rs::BIG5,
        1250 => encoding_rs::WINDOWS_1250,
        1251 => encoding_rs::WINDOWS_1251,
        1253 => encoding_rs::WINDOWS_1253,
        1254 => encoding_rs::WINDOWS_1254,
        1255 => encoding_rs::WINDOWS_1255,
        1256 => encoding_rs::WINDOWS_1256,
        1257 => encoding_rs::WINDOWS_1257,
        1258 => encoding_rs::WINDOWS_1258,
        MAC_ROMAN => encoding_rs::MACINTOSH,
        10_007 => encoding_rs::X_MAC_CYRILLIC,
        20_866 => encoding_rs::KOI8_R,
        21_866 => encoding_rs::KOI8_U,
        20_932 | 51_932 => encoding_rs::EUC_JP,
        28_592 => encoding_rs::ISO_8859_2,
        28_593 => encoding_rs::ISO_8859_3,
        28_594 => encoding_rs::ISO_8859_4,
        28_595 => encoding_rs::ISO_8859_5,
        28_596 => encoding_rs::ISO_8859_6,
        28_597 => encoding_rs::ISO_8859_7,
        28_598 => encoding_rs::ISO_8859_8,
        28_603 => encoding_rs::ISO_8859_13,
        28_605 => encoding_rs::ISO_8859_15,
        54_936 => encoding_rs::GB18030,
        65_001 => encoding_rs::UTF_8,
        _ => encoding_rs::WINDOWS_1252,
    })
}

/// Turns the bytes of the text into characters one at a time, holding the first bytes of a character of several until
/// the rest of it arrives.
#[derive(Default)]
pub(super) struct TextBytes {
    /// For a code page whose characters may take several bytes: its number, and the decoder that holds what has come
    /// of a character.
    several: Option<(u16, Decoder)>,
    decoded: String,
}

impl TextBytes {
    /// Whether an ASCII byte stands for itself, as no character of several bytes can be under way.
    pub(super) fn ascii_stands_for_itself(&self) -> bool {
        self.several.is_none()
    }

    /// What `byte` in `code_page` adds to the text: nothing while a character is incomplete, and also a U+FFFD in front
    /// when the byte cannot go on with the character under way.
    pub(super) fn decode(&mut self, code_page: u16, byte: u8) -> &str {
        self.decoded.clear();
        if self.several.as_ref().is_some_and(|(page, _)| *page != code_page) {
            self.flush();
        }
        match decoding(code_page) {
            Decoding::Table(upper) => {
                self.decoded.push(match byte.checked_sub(0x80) {
                    Some(at) => upper[usize::from(at)],
                    None => char::from(byte),
                });
            }
            Decoding::Encoding(encoding) if encoding.is_single_byte() => {
                self.decoded.push_str(&encoding.decode_without_bom_handling(&[byte]).0);
            }
            Decoding::Encoding(encoding) => {
                let (_, decoder) = self
                    .several
                    .get_or_insert_with(|| (code_page, encoding.new_decoder_without_bom_handling()));
                // The decoder writes into the room the string has and no further.
                self.decoded.reserve(decoder.max_utf8_buffer_length(1).unwrap_or(16));
                let _ = decoder.decode_to_string(&[byte], &mut self.decoded, false);
            }
        }
        // WHATWG's decoders give the C1 control character of the same number for a byte a code page leaves undefined.
        if self.decoded.chars().any(|c| ('\u{80}'..='\u{9F}').contains(&c)) {
            self.decoded = self
                .decoded
                .chars()
                .map(|c| {
                    if ('\u{80}'..='\u{9F}').contains(&c) {
                        char::REPLACEMENT_CHARACTER
                    } else {
                        c
                    }
                })
                .collect();
        }
        &self.decoded
    }

    /// A U+FFFD for a character whose bytes stopped before its end, or nothing if no character is under way.
    pub(super) fn end(&mut self) -> &str {
        self.decoded.clear();
        self.flush();
        &self.decoded
    }

    /// Ends the decoder of several bytes, which turns what it holds of a character into U+FFFD.
    fn flush(&mut self) {
        if let Some((_, mut decoder)) = self.several.take() {
            self.decoded.reserve(decoder.max_utf8_buffer_length(0).unwrap_or(16));
            let _ = decoder.decode_to_string(&[], &mut self.decoded, true);
        }
    }
}

/// Code page 437 from 0x80 on.
const DOS_US_UPPER: [char; 128] = [
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', 'É', 'æ', 'Æ', 'ô', 'ö', 'ò', 'û',
    'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', 'á', 'í', 'ó', 'ú', 'ñ', 'Ñ', 'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡',
    '«', '»', '░', '▒', '▓', '│', '┤', '╡', '╢', '╖', '╕', '╣', '║', '╗', '╝', '╜', '╛', '┐', '└', '┴', '┬', '├', '─',
    '┼', '╞', '╟', '╚', '╔', '╩', '╦', '╠', '═', '╬', '╧', '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█',
    '▄', '▌', '▐', '▀', 'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ', 'ε', '∩', '≡', '±', '≥',
    '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²', '■', '\u{A0}',
];

/// Code page 850 from 0x80 on.
const DOS_LATIN_1_UPPER: [char; 128] = [
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', 'É', 'æ', 'Æ', 'ô', 'ö', 'ò', 'û',
    'ù', 'ÿ', 'Ö', 'Ü', 'ø', '£', 'Ø', '×', 'ƒ', 'á', 'í', 'ó', 'ú', 'ñ', 'Ñ', 'ª', 'º', '¿', '®', '¬', '½', '¼', '¡',
    '«', '»', '░', '▒', '▓', '│', '┤', 'Á', 'Â', 'À', '©', '╣', '║', '╗', '╝', '¢', '¥', '┐', '└', '┴', '┬', '├', '─',
    '┼', 'ã', 'Ã', '╚', '╔', '╩', '╦', '╠', '═', '╬', '¤', 'ð', 'Ð', 'Ê', 'Ë', 'È', 'ı', 'Í', 'Î', 'Ï', '┘', '┌', '█',
    '▄', '¦', 'Ì', '▀', 'Ó', 'ß', 'Ô', 'Ò', 'õ', 'Õ', 'µ', 'þ', 'Þ', 'Ú', 'Û', 'Ù', 'ý', 'Ý', '¯', '´', '\u{AD}', '±',
    '‗', '¾', '¶', '§', '÷', '¸', '°', '¨', '·', '¹', '³', '²', '■', '\u{A0}',
];

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded(code_page: u16, bytes: &[u8]) -> String {
        let mut text_bytes = TextBytes::default();
        let mut out = String::new();
        for &byte in bytes {
            out.push_str(text_bytes.decode(code_page, byte));
        }
        out.push_str(text_bytes.end());
        out
    }

    #[test]
    fn every_code_page_reads_its_letters() {
        let cases: &[(u16, &[u8], &str)] = &[
            (DOS_US, b"\x81", "ü"),
            (DOS_LATIN_1, b"\x9b", "ø"),
            (866, b"\x80", "А"),
            (874, b"\xa1", "ก"),
            (932, b"\x82\xa0", "あ"),
            (10_001, b"\x82\xa0", "あ"),
            (936, b"\xd6\xd0", "中"),
            (10_008, b"\xd6\xd0", "中"),
            (949, b"\xb0\xa1", "가"),
            (10_003, b"\xb0\xa1", "가"),
            (950, b"\xa4\xa4", "中"),
            (10_002, b"\xa4\xa4", "中"),
            (1250, b"\xe8", "č"),
            (1251, b"\xc0", "А"),
            (1253, b"\xe1", "α"),
            (1254, b"\xf0", "ğ"),
            (1255, b"\xe0", "א"),
            (1256, b"\xc7", "ا"),
            (1257, b"\xe0", "ą"),
            (1258, b"\xdd", "Ư"),
            (MAC_ROMAN, b"\x8a", "ä"),
            (10_007, b"\x80", "А"),
            (20_866, b"\xc1", "а"),
            (21_866, b"\xa4", "є"),
            (20_932, b"\xa4\xa2", "あ"),
            (51_932, b"\xa4\xa2", "あ"),
            (28_592, b"\xb1", "ą"),
            (28_593, b"\xa1", "Ħ"),
            (28_594, b"\xa2", "ĸ"),
            (28_595, b"\xb0", "А"),
            (28_596, b"\xc7", "ا"),
            (28_597, b"\xe1", "α"),
            (28_598, b"\xe0", "א"),
            (28_603, b"\xe0", "ą"),
            (28_605, b"\xa4", "€"),
            (54_936, b"\x81\x30\x89\x38", "ß"),
            (65_001, b"\xe4\xb8\xad", "中"),
            (WESTERN, b"\x80", "€"),
            (1, b"\x80", "€"),
        ];
        for &(code_page, bytes, letters) in cases {
            assert_eq!(decoded(code_page, bytes), letters, "{code_page}");
        }
    }

    #[test]
    fn a_change_of_code_page_ends_the_character_under_way() {
        let mut text_bytes = TextBytes::default();
        assert_eq!(text_bytes.decode(932, 0x82), "");
        assert!(!text_bytes.ascii_stands_for_itself());
        assert_eq!(text_bytes.decode(WESTERN, 0xe4), "\u{FFFD}ä");
        assert!(text_bytes.ascii_stands_for_itself());
        assert_eq!(text_bytes.end(), "", "nothing under way");
    }

    #[test]
    fn every_charset_names_its_code_page() {
        let cases = [
            (0, None),
            (1, None),
            (2, Some(WESTERN)),
            (77, Some(MAC_ROMAN)),
            (78, Some(932)),
            (128, Some(932)),
            (79, Some(949)),
            (129, Some(949)),
            (80, Some(936)),
            (134, Some(936)),
            (81, Some(950)),
            (136, Some(950)),
            (89, Some(10_007)),
            (161, Some(1253)),
            (162, Some(1254)),
            (163, Some(1258)),
            (177, Some(1255)),
            (181, Some(1255)),
            (178, Some(1256)),
            (180, Some(1256)),
            (186, Some(1257)),
            (204, Some(1251)),
            (222, Some(874)),
            (238, Some(1250)),
            (254, Some(DOS_US)),
            (255, Some(DOS_LATIN_1)),
            (-1, None),
        ];
        for (charset, code_page) in cases {
            assert_eq!(of_charset(charset), code_page, "{charset}");
        }
    }
}
