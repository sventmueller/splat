//! RTF as the pasteboard offers it, from TextEdit, Word and other Cocoa or Windows apps: read in one pass without
//! recursion, whatever the nesting. The bytes of the text, `\'hh` or as they stand, are read in the code page of their
//! font or of the document.

mod code_page;
mod document;
mod fields;
mod math;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use code_page::{DOS_LATIN_1, DOS_US, MAC_ROMAN, TextBytes, WESTERN, of_charset};
use document::{Collected, ListPlace, Paragraph, assemble};
use fields::hyperlink;

use crate::rich::{
    Align, Block, Cell, Image, MOST_PICTURE_BYTES, Mark, Marks, Piece, Rich, Source, Span, Table, is_monospace_font,
    is_picture,
};
use math::Formula;

/// Groups nested deeper only count, so that their ends still match.
const MOST_DEPTH: usize = 128;
/// What `\plain` and a document without `\fs` set: 12 pt, in half points.
const DEFAULT_SIZE: i32 = 24;
/// Word's numbering knows nine levels.
const DEEPEST_LEVEL: usize = 8;

/// What the text of a group goes into.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Destination {
    Text,
    Skipped,
    FontTable,
    FieldInstruction,
    ListMarker,
    Picture,
    /// `\picprop`, whose shape properties hold a picture's description.
    PictureProperty,
    PropertyName,
    PropertyValue,
    Math,
}

/// What a `}` restores.
#[derive(Clone, Copy)]
struct State {
    marks: Marks,
    font: i32,
    size: i32,
    /// How many units follow a `\u` for readers that do not know it.
    unicode_fallback: u32,
    link: Option<usize>,
    destination: Destination,
}

/// What a paragraph keeps until `\pard`.
#[derive(Clone, Copy, Default)]
struct ParagraphProps {
    list: Option<i32>,
    level: usize,
    in_table: bool,
    /// `\qr` and `\qc`; Word writes `\ql` everywhere, which says no more than nothing.
    align: Align,
    /// What `\outlinelevel` says, which Word writes for its headings.
    outline: Option<u8>,
}

/// The look a span under construction has, to tell whether the next character still fits it.
type RunKey = (Marks, Option<usize>, bool);

/// The text being read and what it has made: the document's, or a footnote's while it is read.
struct Body {
    props: ParagraphProps,
    marker: Option<String>,
    spans: Vec<Span>,
    run: Option<RunKey>,
    least_size: i32,
    chars: usize,
    /// The paragraph holds a formula that Word sets on lines of its own.
    display: bool,
    cell: Vec<Span>,
    /// The alignment of the open cell's first paragraph.
    cell_align: Option<Align>,
    row: Vec<Cell>,
    row_aligns: Vec<Align>,
    rows: Vec<Vec<Cell>>,
    /// The first row's alignments, which are the columns'.
    aligns: Vec<Align>,
    collected: Vec<Collected>,
}

impl Body {
    fn new() -> Body {
        Body {
            props: ParagraphProps::default(),
            marker: None,
            spans: Vec::new(),
            run: None,
            least_size: i32::MAX,
            chars: 0,
            display: false,
            cell: Vec::new(),
            cell_align: None,
            row: Vec::new(),
            row_aligns: Vec::new(),
            rows: Vec::new(),
            aligns: Vec::new(),
            collected: Vec::new(),
        }
    }
}

/// A picture as far as its hex digits have come.
#[derive(Default)]
struct Picture {
    mime: Option<&'static str>,
    bytes: Vec<u8>,
    high: Option<u8>,
    too_large: bool,
    alt: String,
}

impl Picture {
    fn take(&mut self, data: &[u8]) {
        if self.bytes.len() + data.len() > MOST_PICTURE_BYTES {
            self.too_large = true;
        }
        if self.too_large {
            self.bytes = Vec::new();
        } else {
            self.bytes.extend_from_slice(data);
        }
    }

    fn hex_digit(&mut self, c: char) {
        let Some(digit) = c.to_digit(16).and_then(|digit| u8::try_from(digit).ok()) else {
            return;
        };
        match self.high.take() {
            Some(high) => self.take(&[high << 4 | digit]),
            None => self.high = Some(digit),
        }
    }

    /// The picture, or its description if it cannot be one.
    fn into_image(self) -> Result<Image, String> {
        match self.mime {
            Some(mime) if !self.too_large && is_picture(mime, &self.bytes) => Ok(Image {
                source: Source::Data {
                    mime,
                    bytes: self.bytes,
                },
                alt: self.alt,
            }),
            _ => Err(self.alt),
        }
    }
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
    state: State,
    stack: Vec<State>,
    /// Groups open beyond `MOST_DEPTH`.
    too_deep: usize,
    /// A `\*` came, so that the next control word is a destination to skip unless it is known.
    starred: bool,
    /// Units of a `\u`'s fallback still to skip.
    fallback_left: u32,
    high_surrogate: Option<u16>,

    links: Vec<Arc<str>>,
    instruction: String,
    default_font: i32,
    monospace_fonts: HashSet<i32>,
    /// What `\ansicpg` and its kin say, for fonts that name no code page of their own.
    document_code_page: u16,
    font_code_pages: HashMap<i32, u16>,
    bytes: TextBytes,
    font_entry: i32,
    font_name: String,
    characters_by_size: HashMap<i32, usize>,

    body: Body,
    /// While a footnote is read: the body around it, and how many groups are open with the note's.
    outer: Option<(Body, usize)>,
    notes: Vec<Vec<Collected>>,
    picture: Option<Picture>,
    property_name: String,
    property_value: String,
    formula: Option<Formula>,
}

/// The text of an RTF document with what Markdown can say of its look; nothing if it is no RTF.
pub fn from_rtf(data: &[u8]) -> Rich {
    let start = data.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(data.len());
    if !data[start..].starts_with(b"{\\rtf") {
        return Rich::default();
    }
    let mut reader = Reader::new(data, start);
    reader.read();
    reader.finish()
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8], at: usize) -> Reader<'a> {
        Reader {
            data,
            at,
            state: State {
                marks: Marks::default(),
                font: 0,
                size: DEFAULT_SIZE,
                unicode_fallback: 1,
                link: None,
                destination: Destination::Text,
            },
            stack: Vec::new(),
            too_deep: 0,
            starred: false,
            fallback_left: 0,
            high_surrogate: None,
            links: Vec::new(),
            instruction: String::new(),
            default_font: 0,
            monospace_fonts: HashSet::new(),
            document_code_page: WESTERN,
            font_code_pages: HashMap::new(),
            bytes: TextBytes::default(),
            font_entry: 0,
            font_name: String::new(),
            characters_by_size: HashMap::new(),
            body: Body::new(),
            outer: None,
            notes: Vec::new(),
            picture: None,
            property_name: String::new(),
            property_value: String::new(),
            formula: None,
        }
    }

    fn read(&mut self) {
        while let Some(&byte) = self.data.get(self.at) {
            self.at += 1;
            match byte {
                b'{' => self.open_group(),
                b'}' => {
                    if self.close_group() {
                        return;
                    }
                }
                b'\\' => self.control(),
                b'\r' | b'\n' => {}
                byte => self.raw_byte(byte),
            }
        }
    }

    fn open_group(&mut self) {
        self.end_bytes();
        self.starred = false;
        self.fallback_left = 0;
        if self.stack.len() < MOST_DEPTH {
            self.stack.push(self.state);
            if self.state.destination == Destination::Math
                && let Some(formula) = &mut self.formula
            {
                formula.open();
            }
        } else {
            self.too_deep += 1;
        }
    }

    /// Whether this ended the document.
    fn close_group(&mut self) -> bool {
        self.end_bytes();
        self.starred = false;
        self.fallback_left = 0;
        if self.too_deep > 0 {
            self.too_deep -= 1;
            return false;
        }
        let closing = self.state.destination;
        if let Some(state) = self.stack.pop() {
            self.state = state;
        }
        let now = self.state.destination;
        match closing {
            Destination::FontTable => self.end_font_entry(),
            Destination::PropertyValue if now != closing => {
                if self.property_name == "wzDescription"
                    && let Some(picture) = &mut self.picture
                {
                    picture.alt = std::mem::take(&mut self.property_value);
                }
            }
            Destination::Picture if now != closing => self.end_picture(),
            Destination::Math if now == closing => {
                if let Some(formula) = &mut self.formula {
                    formula.close();
                }
            }
            Destination::Math => self.end_formula(),
            _ => {}
        }
        if self.outer.as_ref().is_some_and(|(_, depth)| self.stack.len() < *depth) {
            self.end_note();
        }
        self.stack.is_empty()
    }

    fn raw_byte(&mut self, byte: u8) {
        self.starred = false;
        if self.fallback_left > 0 {
            self.fallback_left -= 1;
        } else {
            self.push_byte(byte);
        }
    }

    /// The code page the text has where it stands: in the font table the one of the font being defined.
    fn code_page(&self) -> u16 {
        let font = if self.state.destination == Destination::FontTable {
            self.font_entry
        } else {
            self.state.font
        };
        self.font_code_pages
            .get(&font)
            .copied()
            .unwrap_or(self.document_code_page)
    }

    fn push_byte(&mut self, byte: u8) {
        if byte.is_ascii() && self.bytes.ascii_stands_for_itself() {
            return self.push_char(char::from(byte));
        }
        let code_page = self.code_page();
        let mut bytes = std::mem::take(&mut self.bytes);
        for c in bytes.decode(code_page, byte).chars() {
            self.push_char(c);
        }
        self.bytes = bytes;
    }

    /// Ends a character whose first bytes wait for the rest, before what comes cannot be the rest.
    fn end_bytes(&mut self) {
        if self.bytes.ascii_stands_for_itself() {
            return;
        }
        let mut bytes = std::mem::take(&mut self.bytes);
        for c in bytes.end().chars() {
            self.push_char(c);
        }
        self.bytes = bytes;
    }

    fn control(&mut self) {
        let data = self.data;
        let Some(&first) = data.get(self.at) else { return };
        if !first.is_ascii_alphabetic() {
            self.at += 1;
            self.symbol(first);
            return;
        }
        let start = self.at;
        while data.get(self.at).is_some_and(u8::is_ascii_alphabetic) {
            self.at += 1;
        }
        let word = std::str::from_utf8(&data[start..self.at]).unwrap_or_default();
        let param = self.read_param();
        self.end_bytes();
        self.word(word, param);
    }

    /// The number behind a control word, and the one space that belongs to it.
    fn read_param(&mut self) -> Option<i32> {
        let data = self.data;
        let negative = data.get(self.at) == Some(&b'-') && data.get(self.at + 1).is_some_and(u8::is_ascii_digit);
        if negative {
            self.at += 1;
        }
        let mut value: Option<i32> = None;
        while let Some(digit) = data.get(self.at).filter(|b| b.is_ascii_digit()) {
            let so_far = value.unwrap_or(0);
            value = Some(so_far.saturating_mul(10).saturating_add(i32::from(digit - b'0')));
            self.at += 1;
        }
        if data.get(self.at) == Some(&b' ') {
            self.at += 1;
        }
        value.map(|v| if negative { -v } else { v })
    }

    fn symbol(&mut self, byte: u8) {
        self.starred = false;
        if byte == b'\'' {
            let byte = self.hex_byte();
            if self.fallback_left > 0 {
                self.fallback_left -= 1;
            } else if let Some(byte) = byte {
                self.push_byte(byte);
            }
            return;
        }
        // A trail byte of a double-byte code page may be one of these three.
        if !matches!(byte, b'{' | b'}' | b'\\') {
            self.end_bytes();
        }
        if self.fallback_left > 0 {
            self.fallback_left -= 1;
            return;
        }
        match byte {
            b'*' => self.starred = true,
            b'\r' | b'\n' => {
                if self.state.destination == Destination::Text {
                    self.end_paragraph();
                }
            }
            b'~' => self.push_char('\u{a0}'),
            b'_' => self.push_char('\u{2011}'),
            b'{' | b'}' | b'\\' => self.push_byte(byte),
            _ => {}
        }
    }

    /// The two hex digits at the reading position, as far as they are some.
    fn hex_byte(&mut self) -> Option<u8> {
        let digit = |at: usize| {
            self.data
                .get(at)
                .and_then(|b| char::from(*b).to_digit(16))
                .and_then(|d| u8::try_from(d).ok())
        };
        let high = digit(self.at)?;
        let Some(low) = digit(self.at + 1) else {
            self.at += 1;
            return None;
        };
        self.at += 2;
        Some(high << 4 | low)
    }

    fn word(&mut self, word: &str, param: Option<i32>) {
        let starred = std::mem::take(&mut self.starred);
        if word == "bin" {
            self.skip_binary(param);
        }
        if self.fallback_left > 0 {
            self.fallback_left -= 1;
            return;
        }
        if self.state.destination == Destination::Skipped || word == "bin" {
            return;
        }
        let in_math = self.state.destination == Destination::Math;
        if starred && !in_math && !matches!(word, "fldinst" | "listtext" | "pntext" | "shppict" | "picprop") {
            self.state.destination = Destination::Skipped;
            return;
        }
        if !in_math && self.starts_destination(word) {
            return;
        }
        match word {
            "u" => return self.unicode(param.unwrap_or(0)),
            "uc" => {
                self.state.unicode_fallback = u32::try_from(param.unwrap_or(1)).unwrap_or(0);
                return;
            }
            _ => {}
        }
        if let Some(c) = special_char(word) {
            return self.push_char(c);
        }
        match self.state.destination {
            Destination::Text => self.text_word(word, param),
            Destination::FontTable if word == "f" => self.font_entry = param.unwrap_or(0),
            Destination::FontTable if word == "fcharset" => self.set_font_code_page(of_charset(param.unwrap_or(0))),
            Destination::FontTable if word == "cpg" => {
                self.set_font_code_page(param.and_then(|page| u16::try_from(page).ok()));
            }
            Destination::FieldInstruction if word == "l" => self.instruction.push_str("\\l "),
            Destination::Picture => {
                if let Some(picture) = &mut self.picture {
                    match word {
                        "pngblip" => picture.mime = Some("image/png"),
                        "jpegblip" => picture.mime = Some("image/jpeg"),
                        _ => {}
                    }
                }
            }
            Destination::Math => {
                if let Some(formula) = &mut self.formula {
                    formula.word(word, param);
                }
            }
            _ => {}
        }
    }

    /// Takes in a control word if it decides where the text of its group goes, and returns whether it did.
    fn starts_destination(&mut self, word: &str) -> bool {
        let in_properties = matches!(
            self.state.destination,
            Destination::PictureProperty | Destination::PropertyName | Destination::PropertyValue
        );
        match word {
            "pict" => self.start_picture(),
            "picprop" if self.state.destination == Destination::Picture => {
                self.state.destination = Destination::PictureProperty;
            }
            "shppict" => {}
            "sp" if in_properties => self.state.destination = Destination::PictureProperty,
            "sn" if in_properties => {
                self.property_name.clear();
                self.state.destination = Destination::PropertyName;
            }
            "sv" if in_properties => {
                self.property_value.clear();
                self.state.destination = Destination::PropertyValue;
            }
            "footnote" => self.start_note(),
            "mmath" => self.start_formula(),
            "fonttbl" => self.state.destination = Destination::FontTable,
            "fldinst" => {
                self.instruction.clear();
                self.state.destination = Destination::FieldInstruction;
            }
            "fldrslt" => self.start_field_result(),
            "listtext" | "pntext" => {
                self.body.marker = Some(String::new());
                self.state.destination = Destination::ListMarker;
            }
            // Cocoa writes a picture as `{{\NeXTGraphic name}¬}`, and the group around it holds only the stand-in.
            "NeXTGraphic" => {
                self.state.destination = Destination::Skipped;
                if self.stack.len() >= 3
                    && let Some(around) = self.stack.last_mut()
                {
                    around.destination = Destination::Skipped;
                }
            }
            word if is_skipped_destination(word) => self.state.destination = Destination::Skipped,
            _ => return false,
        }
        true
    }

    fn start_picture(&mut self) {
        if self.state.destination == Destination::Text && self.picture.is_none() {
            self.picture = Some(Picture::default());
            self.state.destination = Destination::Picture;
        } else {
            self.state.destination = Destination::Skipped;
        }
    }

    /// A picture in a format Markdown cannot show leaves its description, as one from the web that cannot be had does.
    fn end_picture(&mut self) {
        let Some(picture) = self.picture.take() else { return };
        match picture.into_image() {
            Ok(image) => self.push_object(Piece::Image(image)),
            Err(alt) => {
                for c in alt.chars() {
                    self.push_text(c);
                }
            }
        }
    }

    /// Word writes a footnote where its reference stands, so the text around it waits until it ends.
    fn start_note(&mut self) {
        if self.state.destination == Destination::Text && self.outer.is_none() {
            self.outer = Some((std::mem::replace(&mut self.body, Body::new()), self.stack.len()));
        } else {
            self.state.destination = Destination::Skipped;
        }
    }

    fn end_note(&mut self) {
        let Some((outer, _)) = self.outer.take() else { return };
        self.end_body();
        let note = std::mem::replace(&mut self.body, outer);
        self.notes.push(note.collected);
        self.body
            .spans
            .push(Span::of(Piece::Note(self.notes.len().to_string())));
        self.body.run = None;
    }

    fn start_formula(&mut self) {
        if self.state.destination == Destination::Text && self.formula.is_none() {
            self.formula = Some(Formula::new());
            self.state.destination = Destination::Math;
        } else {
            self.state.destination = Destination::Skipped;
        }
    }

    fn end_formula(&mut self) {
        if let Some((tex, display)) = self.formula.take().and_then(Formula::finish) {
            self.body.display |= display;
            self.push_object(Piece::Math(tex));
        }
    }

    /// A picture or formula, which counts as one character of the paragraph.
    fn push_object(&mut self, piece: Piece) {
        self.body.chars += 1;
        self.body.least_size = self.body.least_size.min(self.state.size);
        self.body.spans.push(Span {
            piece,
            marks: Marks::default(),
            link: self.state.link.and_then(|at| self.links.get(at).cloned()),
        });
        self.body.run = None;
    }

    fn start_field_result(&mut self) {
        self.state.destination = Destination::Text;
        if let Some(url) = hyperlink(&self.instruction) {
            self.state.link = Some(self.links.len());
            self.links.push(url.into());
        }
        self.instruction.clear();
    }

    fn skip_binary(&mut self, length: Option<i32>) {
        let length = usize::try_from(length.unwrap_or(0)).unwrap_or(0);
        let end = self.at.saturating_add(length).min(self.data.len());
        if self.state.destination == Destination::Picture
            && let Some(picture) = &mut self.picture
        {
            picture.take(&self.data[self.at..end]);
        }
        self.at = end;
    }

    fn unicode(&mut self, param: i32) {
        self.fallback_left = self.state.unicode_fallback;
        let unit = if param < 0 { param.saturating_add(65_536) } else { param };
        let Ok(unit) = u16::try_from(unit) else {
            return self.push_char(char::REPLACEMENT_CHARACTER);
        };
        match unit {
            0xD800..=0xDBFF => {
                self.lone_surrogate();
                self.high_surrogate = Some(unit);
            }
            0xDC00..=0xDFFF => {
                let joined = self.high_surrogate.take().and_then(|high| {
                    char::from_u32(0x1_0000 + ((u32::from(high) - 0xD800) << 10) + (u32::from(unit) - 0xDC00))
                });
                self.push_char(joined.unwrap_or(char::REPLACEMENT_CHARACTER));
            }
            unit => self.push_char(char::from_u32(u32::from(unit)).unwrap_or(char::REPLACEMENT_CHARACTER)),
        }
    }

    /// A first half of a surrogate pair whose second half never came stands for nothing.
    fn lone_surrogate(&mut self) {
        if self.high_surrogate.take().is_some() {
            self.route_char(char::REPLACEMENT_CHARACTER);
        }
    }

    fn push_char(&mut self, c: char) {
        self.lone_surrogate();
        self.route_char(c);
    }

    fn route_char(&mut self, c: char) {
        match self.state.destination {
            Destination::Text => self.push_text(c),
            Destination::Skipped => {}
            Destination::FontTable if c == ';' => self.end_font_entry(),
            Destination::FontTable => self.font_name.push(c),
            Destination::FieldInstruction => self.instruction.push(c),
            Destination::ListMarker => {
                if let Some(marker) = &mut self.body.marker {
                    marker.push(c);
                }
            }
            Destination::Picture => {
                if let Some(picture) = &mut self.picture {
                    picture.hex_digit(c);
                }
            }
            Destination::PictureProperty => {}
            Destination::PropertyName => self.property_name.push(c),
            Destination::PropertyValue => self.property_value.push(c),
            Destination::Math => {
                if let Some(formula) = &mut self.formula {
                    formula.char(c);
                }
            }
        }
    }

    fn set_font_code_page(&mut self, code_page: Option<u16>) {
        match code_page {
            Some(code_page) => self.font_code_pages.insert(self.font_entry, code_page),
            None => self.font_code_pages.remove(&self.font_entry),
        };
    }

    fn end_font_entry(&mut self) {
        if is_monospace_font(&self.font_name) {
            self.monospace_fonts.insert(self.font_entry);
        }
        self.font_name.clear();
    }

    fn push_text(&mut self, c: char) {
        if !c.is_whitespace() {
            self.body.chars += 1;
            self.body.least_size = self.body.least_size.min(self.state.size);
            if self.outer.is_none() {
                *self.characters_by_size.entry(self.state.size).or_insert(0) += 1;
            }
        }
        let code = self.monospace_fonts.contains(&self.state.font);
        let key = (self.state.marks, self.state.link, code);
        if self.body.run == Some(key)
            && let Some(Span {
                piece: Piece::Text(text) | Piece::Code(text),
                ..
            }) = self.body.spans.last_mut()
        {
            text.push(c);
            return;
        }
        let text = c.to_string();
        self.body.spans.push(Span {
            piece: if code { Piece::Code(text) } else { Piece::Text(text) },
            marks: self.state.marks,
            link: self.state.link.and_then(|at| self.links.get(at).cloned()),
        });
        self.body.run = Some(key);
    }

    fn text_word(&mut self, word: &str, param: Option<i32>) {
        if self.set_mark(word, param) {
            return;
        }
        match word {
            "par" | "sect" | "page" => self.end_paragraph(),
            "line" => self.push_break(),
            "pard" => self.body.props = ParagraphProps::default(),
            "plain" => {
                self.state.marks = Marks::default();
                self.state.size = DEFAULT_SIZE;
                self.state.font = self.default_font;
            }
            "ls" => self.body.props.list = Some(param.unwrap_or(0)),
            "ilvl" => self.body.props.level = usize::try_from(param.unwrap_or(0)).unwrap_or(0).min(DEEPEST_LEVEL),
            "intbl" => self.body.props.in_table = true,
            "qr" => self.body.props.align = Align::Right,
            "qc" => self.body.props.align = Align::Center,
            "ql" | "qj" | "qd" => self.body.props.align = Align::None,
            "outlinelevel" => self.body.props.outline = param.and_then(|level| u8::try_from(level).ok()),
            "cell" => self.end_cell(),
            "row" => self.end_row(),
            "nestcell" | "nestrow" => self.add_to_cell(),
            "f" => self.state.font = param.unwrap_or(self.default_font),
            "fs" => self.state.size = param.unwrap_or(DEFAULT_SIZE).max(1),
            "deff" => {
                self.default_font = param.unwrap_or(0);
                self.state.font = self.default_font;
            }
            "ansi" => self.document_code_page = WESTERN,
            "mac" => self.document_code_page = MAC_ROMAN,
            "pc" => self.document_code_page = DOS_US,
            "pca" => self.document_code_page = DOS_LATIN_1,
            "ansicpg" => self.document_code_page = param.and_then(|page| u16::try_from(page).ok()).unwrap_or(WESTERN),
            _ => {}
        }
    }

    /// Takes in a control word if it sets a mark, and returns whether it did.
    fn set_mark(&mut self, word: &str, param: Option<i32>) -> bool {
        let on = param != Some(0);
        let marks = self.state.marks;
        self.state.marks = match word {
            "b" => marks.with(Mark::Bold, on),
            "i" => marks.with(Mark::Italic, on),
            "strike" | "striked" => marks.with(Mark::Strike, on),
            "super" => marks.with(Mark::Superscript, on).with(Mark::Subscript, false),
            "sub" => marks.with(Mark::Subscript, on).with(Mark::Superscript, false),
            "nosupersub" => marks.with(Mark::Superscript, false).with(Mark::Subscript, false),
            "ulnone" => marks.with(Mark::Underline, false),
            "ulc" => marks,
            word if word.starts_with("ul") => marks.with(Mark::Underline, on),
            _ => return false,
        };
        true
    }

    fn push_break(&mut self) {
        self.body.spans.push(Span {
            piece: Piece::Break,
            marks: self.state.marks,
            link: self.state.link.and_then(|at| self.links.get(at).cloned()),
        });
        self.body.run = None;
    }

    /// The spans of the paragraph so far, which starts anew.
    fn take_spans(&mut self) -> Vec<Span> {
        self.body.run = None;
        self.body.display = false;
        self.body.chars = 0;
        self.body.least_size = i32::MAX;
        std::mem::take(&mut self.body.spans)
    }

    fn end_paragraph(&mut self) {
        if self.body.props.in_table {
            self.add_to_cell();
            self.body.marker = None;
            return;
        }
        let (chars, least_size, display) = (self.body.chars, self.body.least_size, self.body.display);
        let spans = self.take_spans();
        self.end_table();
        if display
            && self.body.props.list.is_none()
            && self.body.marker.is_none()
            && let Some(tex) = lone_formula(&spans)
        {
            self.body.collected.push(Collected::Math(tex));
            return;
        }
        let list = match (self.body.props.list, self.body.marker.take()) {
            (None, None) => None,
            (list, marker) => Some(ListPlace {
                id: list.map_or(0, |id| u64::from(id.unsigned_abs())),
                level: self.body.props.level,
                marker,
            }),
        };
        self.body.collected.push(Collected::Paragraph(Paragraph {
            spans,
            least_size,
            chars,
            list,
            outline: self.body.props.outline,
        }));
    }

    /// Moves the text so far into the current cell; several paragraphs in a cell become lines.
    fn add_to_cell(&mut self) {
        self.body.cell_align.get_or_insert(self.body.props.align);
        let spans = self.take_spans();
        if spans.is_empty() {
            return;
        }
        if !self.body.cell.is_empty() {
            self.body.cell.push(Span::of(Piece::Break));
        }
        self.body.cell.extend(spans);
    }

    fn end_cell(&mut self) {
        self.add_to_cell();
        self.body.row.push(Cell::new(std::mem::take(&mut self.body.cell)));
        let align = self.body.cell_align.take().unwrap_or_default();
        self.body.row_aligns.push(align);
    }

    fn end_row(&mut self) {
        if !self.body.spans.is_empty() || !self.body.cell.is_empty() {
            self.end_cell();
        }
        let aligns = std::mem::take(&mut self.body.row_aligns);
        if !self.body.row.is_empty() {
            if self.body.rows.is_empty() {
                self.body.aligns = aligns;
            }
            self.body.rows.push(std::mem::take(&mut self.body.row));
        }
    }

    fn end_table(&mut self) {
        self.end_row();
        if !self.body.rows.is_empty() {
            self.body.collected.push(Collected::Table(Table {
                alignments: std::mem::take(&mut self.body.aligns),
                rows: std::mem::take(&mut self.body.rows),
            }));
        }
    }

    /// Ends what is still open of the body being read.
    fn end_body(&mut self) {
        if !self.body.props.in_table && !self.body.spans.is_empty() {
            self.end_paragraph();
        }
        self.end_table();
    }

    fn finish(mut self) -> Rich {
        self.end_bytes();
        self.lone_surrogate();
        self.end_formula();
        self.end_note();
        self.end_body();
        let mut blocks = assemble(self.body.collected, &self.characters_by_size);
        for (index, collected) in self.notes.into_iter().enumerate() {
            blocks.push(Block::Note {
                name: (index + 1).to_string(),
                blocks: assemble(collected, &self.characters_by_size),
            });
        }
        Rich { blocks }
    }
}

/// The formula a paragraph holds if it holds nothing else.
fn lone_formula(spans: &[Span]) -> Option<String> {
    let mut formulas = spans.iter().filter(|span| match &span.piece {
        Piece::Text(text) => !text.chars().all(char::is_whitespace),
        _ => true,
    });
    match (formulas.next(), formulas.next()) {
        (
            Some(Span {
                piece: Piece::Math(tex),
                ..
            }),
            None,
        ) => Some(tex.clone()),
        _ => None,
    }
}

fn special_char(word: &str) -> Option<char> {
    Some(match word {
        "tab" => '\t',
        "emdash" => '—',
        "endash" => '–',
        "emspace" => '\u{2003}',
        "enspace" => '\u{2002}',
        "qmspace" => '\u{2005}',
        "bullet" => '•',
        "lquote" => '‘',
        "rquote" => '’',
        "ldblquote" => '“',
        "rdblquote" => '”',
        _ => return None,
    })
}

/// Groups whose text is no text of the document, by what they start with. `\headery` and `\footery` are no such thing.
fn is_skipped_destination(word: &str) -> bool {
    matches!(
        word,
        "colortbl"
            | "stylesheet"
            | "info"
            | "object"
            | "header"
            | "headerl"
            | "headerr"
            | "headerf"
            | "footer"
            | "footerl"
            | "footerr"
            | "footerf"
            | "mmathPict"
            | "ftnsep"
            | "ftnsepc"
            | "aftnsep"
            | "aftnsepc"
            | "ftncn"
            | "aftncn"
            | "annotation"
            | "filetbl"
            | "revtbl"
            | "rsidtbl"
            | "listtable"
            | "listoverridetable"
            | "generator"
            | "themedata"
            | "colorschememapping"
            | "datastore"
            | "latentstyles"
            | "xmlnstbl"
            | "mmathPr"
            | "nonshppict"
            | "shp"
            | "shpinst"
            | "sp"
            | "sn"
            | "sv"
            | "do"
            | "bkmkstart"
            | "bkmkend"
            | "tc"
            | "xe"
            | "pn"
            | "falt"
            | "panose"
            | "pgdsctbl"
    )
}
