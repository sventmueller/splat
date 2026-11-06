//! The content of a paragraph as spans: runs and their look, links, fields, notes and pictures.

use std::sync::Arc;

use roxmltree::Node;

use super::body::{MOST_DEPTH, Walker, text_below};
use super::builder::BlockBuilder;
use super::math;
use super::package::{A, M, MC, OFFICE, R, V, W, WP, child_w, chosen_content, w_attr, w_val};
use super::styles::RunProps;
use super::{Media, MissingPicture, MissingReason};
use crate::rich::{Image, Mark, Marks, Piece, Source, Span};
use splat_ooxml::zip::ZipError;

/// More of a field's instruction than a link in it needs.
const LONGEST_INSTRUCTION: usize = 1 << 16;

/// What stands around a run in its paragraph.
#[derive(Clone)]
pub(super) struct RunContext {
    /// What the paragraph's style says of the look.
    pub(super) paragraph: RunProps,
    pub(super) link: Option<Arc<str>>,
    pub(super) leads_to_bookmark: bool,
}

/// A paragraph's content as it is read.
#[derive(Default)]
pub(super) struct Line {
    pub(super) spans: Vec<Span>,
    /// Some text stands outside links to bookmarks and fields, so this is no line of a table of contents.
    pub(super) text_outside_links: bool,
    /// Some text is code because a character style calls it that.
    pub(super) named_code: bool,
    /// The formulas among the spans that stand on lines of their own.
    pub(super) display_formulas: Vec<String>,
    /// The name of a bookmark in it that a link leads to.
    pub(super) anchor: Option<String>,
    /// It holds the shape that Word draws a horizontal line with.
    pub(super) horizontal_line: bool,
}

impl Line {
    /// The paragraph's formula, if it holds nothing else and that as a formula of its own.
    pub(super) fn only_display_math(&self) -> Option<String> {
        (!self.display_formulas.is_empty() && self.display_formulas.len() == self.spans.len())
            .then(|| self.display_formulas.join(" \\\\ "))
    }
}

/// A field that is open: `silent` while its text is not to be seen, as its instruction is not, `link` where its result
/// goes if it is a link.
pub(super) struct Field {
    instruction: String,
    /// Its result has started; an instruction is only worth reading once.
    separated: bool,
    silent: bool,
    link: Option<Arc<str>>,
    outer_silent: bool,
    outer_link: Option<Arc<str>>,
}

/// What a run looks like, all that stands around it taken together.
struct RunFormat {
    marks: Marks,
    code: bool,
    named_code: bool,
    hidden: bool,
}

/// A property Word toggles: on in the paragraph's and the character's style cancels out, as it does in Word.
fn toggled(direct: Option<bool>, paragraph: Option<bool>, character: Option<bool>) -> bool {
    direct.unwrap_or(paragraph.unwrap_or(false) ^ character.unwrap_or(false))
}

fn nearest(direct: Option<bool>, paragraph: Option<bool>, character: Option<bool>) -> bool {
    direct.or(character).or(paragraph).unwrap_or(false)
}

/// A field's instruction in its words, with quotes taken away from what they group.
fn instruction_parts(instruction: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut part = String::new();
    let mut quoted = false;
    for c in instruction.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !part.is_empty() {
                    parts.push(std::mem::take(&mut part));
                }
            }
            c => part.push(c),
        }
    }
    if !part.is_empty() {
        parts.push(part);
    }
    parts
}

/// Where a `HYPERLINK` field leads, if its instruction is one.
fn field_link(parts: &[String]) -> Option<String> {
    let (first, rest) = parts.split_first()?;
    if !first.eq_ignore_ascii_case("HYPERLINK") {
        return None;
    }
    let mut url = None;
    let mut anchor = None;
    let mut words = rest.iter();
    while let Some(word) = words.next() {
        match word.to_ascii_lowercase().as_str() {
            "\\l" => anchor = words.next().map(|name| clean_anchor(name)),
            "\\o" | "\\t" => {
                words.next();
            }
            switch if switch.starts_with('\\') => {}
            _ => {
                url.get_or_insert_with(|| word.clone());
            }
        }
    }
    link_of(url.as_deref(), anchor.as_deref())
}

/// A bookmark's name as a link and a heading can both use it.
pub(super) fn clean_anchor(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect()
}

fn link_of(url: Option<&str>, anchor: Option<&str>) -> Option<String> {
    let anchor = anchor.filter(|anchor| !anchor.is_empty());
    match (url, anchor) {
        (Some(url), Some(anchor)) => Some(format!("{url}#{anchor}")),
        (Some(url), None) => Some(url.to_string()),
        (None, Some(anchor)) => Some(format!("#{anchor}")),
        (None, None) => None,
    }
}

/// What `w:sym` stands for: the Symbol font has letters of its own in the private use area.
fn symbol(node: Node) -> Option<char> {
    let code = u32::from_str_radix(w_attr(node, "char")?, 16).ok()?;
    let is_symbol_font = w_attr(node, "font").is_some_and(|font| font.eq_ignore_ascii_case("Symbol"));
    if is_symbol_font
        && (code < 0x100 || (0xF000..=0xF0FF).contains(&code))
        && let Some(c) = symbol_font_char(code & 0xFF)
    {
        return Some(c);
    }
    char::from_u32(code)
}

fn symbol_font_char(code: u32) -> Option<char> {
    const CAPITALS: &str = "ΑΒΧΔΕΦΓΗΙϑΚΛΜΝΟΠΘΡΣΤΥςΩΞΨΖ";
    const SMALL: &str = "αβχδεφγηιϕκλμνοπθρστυϖωξψζ";
    let nth = |letters: &str, from: u32| {
        let at = usize::try_from(code - from).ok()?;
        letters.chars().nth(at)
    };
    match code {
        0x41..=0x5A => nth(CAPITALS, 0x41),
        0x61..=0x7A => nth(SMALL, 0x61),
        0xA3 => Some('≤'),
        0xA5 => Some('∞'),
        0xAC => Some('←'),
        0xAD => Some('↑'),
        0xAE => Some('→'),
        0xAF => Some('↓'),
        0xB1 => Some('±'),
        0xB3 => Some('≥'),
        0xB4 => Some('×'),
        0xB7 => Some('•'),
        0xB8 => Some('÷'),
        0xB9 => Some('≠'),
        0xD6 => Some('√'),
        0xE5 => Some('∑'),
        _ => None,
    }
}

/// The file type of a picture, by its first bytes.
fn picture_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG") {
        Some("png")
    } else if bytes.starts_with(b"\xFF\xD8\xFF") {
        Some("jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("webp")
    } else if bytes.starts_with(b"BM") {
        Some("bmp")
    } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        Some("tiff")
    } else if bytes.starts_with(&[1, 0, 0, 0]) && bytes.get(40..44) == Some(b" EMF") {
        Some("emf")
    } else if bytes.starts_with(&[0xD7, 0xCD, 0xC6, 0x9A])
        || bytes.starts_with(&[1, 0, 9, 0])
        || bytes.starts_with(&[2, 0, 9, 0])
    {
        Some("wmf")
    } else {
        None
    }
}

/// What points to a picture in `folder` from the Markdown, as a link has to spell it.
fn picture_destination(folder: &str, file_name: &str) -> String {
    let folder = folder.trim_end_matches('/');
    if folder.is_empty() {
        return file_name.to_string();
    }
    let mut destination = String::with_capacity(folder.len() + file_name.len() + 1);
    for c in folder.chars() {
        match c {
            ' ' => destination.push_str("%20"),
            '%' => destination.push_str("%25"),
            '#' => destination.push_str("%23"),
            '?' => destination.push_str("%3F"),
            c => destination.push(c),
        }
    }
    destination.push('/');
    destination.push_str(file_name);
    destination
}

/// What a drawing holds, from one walk through it.
#[derive(Default)]
struct Drawing<'n, 'i> {
    pictures: Vec<&'n str>,
    alt: Option<&'n str>,
    boxes: Vec<Node<'n, 'i>>,
    horizontal_line: bool,
}

impl<'n, 'i> Drawing<'n, 'i> {
    fn take_description(&mut self, description: Option<&'n str>) {
        if self.alt.is_none() {
            self.alt = description.filter(|text| !text.trim().is_empty());
        }
    }

    /// Goes through what `node` holds, and leaves text boxes for their paragraphs to be read in their own time.
    fn look_through(&mut self, node: Node<'n, 'i>, depth: usize) {
        if depth >= MOST_DEPTH {
            return;
        }
        for child in node.children().filter(|child| child.is_element()) {
            let tag = child.tag_name();
            match (tag.namespace(), tag.name()) {
                (Some(A), "blip") => self.pictures.extend(child.attribute((R, "embed"))),
                (Some(V), "imagedata") => self.pictures.extend(child.attribute((R, "id"))),
                (Some(WP), "docPr") => {
                    self.take_description(child.attribute("descr"));
                    self.take_description(child.attribute("title"));
                }
                (Some(W), "txbxContent") => self.boxes.push(child),
                (Some(V), "rect") if child.attribute((OFFICE, "hr")).is_some() => self.horizontal_line = true,
                (Some(MC), "AlternateContent") => {
                    if let Some(chosen) = chosen_content(child) {
                        self.look_through(chosen, depth + 1);
                    }
                }
                (Some(V), "shape") => {
                    self.take_description(child.attribute("alt"));
                    self.look_through(child, depth + 1);
                }
                _ => self.look_through(child, depth + 1),
            }
        }
    }
}

impl Walker<'_, '_, '_> {
    /// Whether text at this point belongs to a field's instruction or a table of contents.
    fn silent(&self) -> bool {
        self.fields.last().is_some_and(|field| field.silent)
    }

    fn link_here(&self, around: &RunContext) -> Option<Arc<str>> {
        self.fields
            .last()
            .and_then(|field| field.link.clone())
            .or_else(|| around.link.clone())
    }

    fn look(&self, run: Node, around: &RunContext) -> RunFormat {
        let properties = child_w(run, "rPr");
        let direct = properties.map_or_else(RunProps::default, RunProps::read);
        let character = properties
            .and_then(|properties| child_w(properties, "rStyle"))
            .and_then(w_val)
            .and_then(|id| self.styles.character(id));
        let named_code = character.is_some_and(|style| style.inline_code);
        let from_style = character.map_or_else(RunProps::default, |style| style.run);
        let paragraph = around.paragraph;
        let marks = Marks::default()
            .with(Mark::Bold, toggled(direct.bold, paragraph.bold, from_style.bold))
            .with(
                Mark::Italic,
                toggled(direct.italic, paragraph.italic, from_style.italic),
            )
            .with(
                Mark::Strike,
                toggled(direct.strike, paragraph.strike, from_style.strike),
            )
            .with(
                Mark::Underline,
                nearest(direct.underline, paragraph.underline, from_style.underline),
            )
            .with(
                Mark::Highlight,
                nearest(direct.highlight, paragraph.highlight, from_style.highlight),
            )
            .with(
                Mark::Superscript,
                nearest(direct.superscript, paragraph.superscript, from_style.superscript),
            )
            .with(
                Mark::Subscript,
                nearest(direct.subscript, paragraph.subscript, from_style.subscript),
            );
        RunFormat {
            marks,
            code: named_code || nearest(direct.monospace, paragraph.monospace, from_style.monospace),
            named_code,
            hidden: nearest(direct.hidden, paragraph.hidden, from_style.hidden),
        }
    }

    pub(super) fn inlines(
        &mut self,
        parent: Node,
        depth: usize,
        around: &RunContext,
        line: &mut Line,
        sink: &mut BlockBuilder,
    ) {
        for child in parent.children().filter(|child| child.is_element()) {
            self.inline(child, depth + 1, around, line, sink);
        }
    }

    fn inline(&mut self, node: Node, depth: usize, around: &RunContext, line: &mut Line, sink: &mut BlockBuilder) {
        if depth >= MOST_DEPTH {
            if !self.silent() {
                line.spans.push(Span::text(text_below(node)));
                line.text_outside_links = true;
            }
            return;
        }
        let tag = node.tag_name();
        match (tag.namespace(), tag.name()) {
            (Some(W), "r") => self.run(node, depth, around, line, sink),
            (Some(W), "hyperlink") => {
                let inner = self.hyperlink(node, around);
                self.inlines(node, depth, &inner, line, sink);
            }
            (Some(W), "ins" | "moveTo" | "smartTag" | "customXml" | "sdt" | "sdtContent" | "dir" | "bdo") => {
                self.inlines(node, depth, around, line, sink);
            }
            (Some(W), "fldSimple") => {
                let height = self.fields.len();
                self.open_field();
                if let Some(instruction) = w_attr(node, "instr") {
                    self.push_instruction(instruction);
                }
                self.separate_field();
                self.inlines(node, depth, around, line, sink);
                self.fields.truncate(height);
            }
            (Some(W), "bookmarkStart") => {
                if line.anchor.is_none()
                    && let Some(name) = w_attr(node, "name").filter(|name| self.anchors.contains(*name))
                {
                    line.anchor = Some(name.to_string());
                }
            }
            (Some(M), "oMath") => self.formula(math::to_latex(node, depth), false, around, line),
            (Some(M), "oMathPara") => self.formula(math::lines(node, depth), true, around, line),
            (Some(MC), "AlternateContent") => {
                if let Some(chosen) = chosen_content(node) {
                    self.inlines(chosen, depth, around, line, sink);
                }
            }
            _ => {}
        }
    }

    fn formula(&self, tex: String, display: bool, around: &RunContext, line: &mut Line) {
        if self.silent() {
            return;
        }
        line.text_outside_links = true;
        if display {
            line.display_formulas.push(tex.clone());
        }
        line.spans.push(Span {
            piece: Piece::Math(tex),
            marks: Marks::default(),
            link: self.link_here(around),
        });
    }

    fn hyperlink(&self, node: Node, around: &RunContext) -> RunContext {
        let target = node
            .attribute((R, "id"))
            .and_then(|id| self.rels.get(id))
            .map(|rel| rel.target.as_str());
        let anchor = w_attr(node, "anchor").map(clean_anchor);
        let link = link_of(target, anchor.as_deref());
        RunContext {
            paragraph: around.paragraph,
            leads_to_bookmark: around.leads_to_bookmark || (target.is_none() && link.is_some()),
            link: link.map(Arc::from).or_else(|| around.link.clone()),
        }
    }

    fn run(&mut self, run: Node, depth: usize, around: &RunContext, line: &mut Line, sink: &mut BlockBuilder) {
        let look = self.look(run, around);
        for child in run.children().filter(|child| child.is_element()) {
            self.run_child(child, depth + 1, &look, around, line, sink);
        }
    }

    fn run_child(
        &mut self,
        node: Node,
        depth: usize,
        look: &RunFormat,
        around: &RunContext,
        line: &mut Line,
        sink: &mut BlockBuilder,
    ) {
        if depth >= MOST_DEPTH {
            return;
        }
        let tag = node.tag_name();
        match (tag.namespace(), tag.name()) {
            (Some(W), "t") => {
                let text = node.text().unwrap_or("").replace(['\n', '\r'], " ");
                self.push_text(&text, look, around, line);
            }
            (Some(W), "tab" | "ptab") => self.push_text("\t", look, around, line),
            (Some(W), "noBreakHyphen") => self.push_text("\u{2011}", look, around, line),
            (Some(W), "sym") => {
                if let Some(c) = symbol(node) {
                    self.push_text(c.encode_utf8(&mut [0; 4]), look, around, line);
                }
            }
            (Some(W), "br") if matches!(w_attr(node, "type"), Some("page" | "column")) => {}
            (Some(W), "br" | "cr") => {
                if !self.silent() && !look.hidden {
                    line.spans.push(Span::of(Piece::Break));
                }
            }
            (Some(W), "instrText") => self.push_instruction(node.text().unwrap_or("")),
            (Some(W), "fldChar") => match w_attr(node, "fldCharType") {
                Some("begin") => self.open_field(),
                Some("separate") => self.separate_field(),
                Some("end") => {
                    self.fields.pop();
                }
                _ => {}
            },
            (Some(W), "footnoteReference") => self.note_reference(node, 0, look, line),
            (Some(W), "endnoteReference") => self.note_reference(node, 1, look, line),
            (Some(W), "drawing" | "pict" | "object") => self.drawing(node, depth, look, around, line, sink),
            (Some(MC), "AlternateContent") => {
                if let Some(chosen) = chosen_content(node) {
                    for child in chosen.children().filter(|child| child.is_element()) {
                        self.run_child(child, depth + 1, look, around, line, sink);
                    }
                }
            }
            _ => {}
        }
    }

    fn push_text(&self, text: &str, look: &RunFormat, around: &RunContext, line: &mut Line) {
        if text.is_empty() || self.silent() || look.hidden {
            return;
        }
        line.text_outside_links |= self.fields.is_empty() && !around.leads_to_bookmark;
        line.named_code |= look.named_code;
        line.spans.push(Span {
            piece: if look.code {
                Piece::Code(text.to_string())
            } else {
                Piece::Text(text.to_string())
            },
            marks: look.marks,
            link: self.link_here(around),
        });
    }

    fn note_reference(&mut self, node: Node, part: usize, look: &RunFormat, line: &mut Line) {
        if self.silent() || look.hidden {
            return;
        }
        if let Some(number) = w_attr(node, "id").and_then(|id| self.note_number(part, id)) {
            line.spans.push(Span::of(Piece::Note(number.to_string())));
        }
    }

    fn open_field(&mut self) {
        let (outer_silent, outer_link) = self
            .fields
            .last()
            .map_or((false, None), |field| (field.silent, field.link.clone()));
        self.fields.push(Field {
            instruction: String::new(),
            separated: false,
            silent: true,
            link: outer_link.clone(),
            outer_silent,
            outer_link,
        });
    }

    fn push_instruction(&mut self, text: &str) {
        if let Some(field) = self
            .fields
            .last_mut()
            .filter(|field| !field.separated && field.instruction.len() < LONGEST_INSTRUCTION)
        {
            field.instruction.push_str(text);
        }
    }

    /// The field's instruction has ended and its result starts: a table of contents is made anew by whoever needs one.
    fn separate_field(&mut self) {
        let Some(field) = self.fields.last_mut().filter(|field| !field.separated) else {
            return;
        };
        field.separated = true;
        let parts = instruction_parts(&field.instruction);
        let is_contents = parts.first().is_some_and(|word| word.eq_ignore_ascii_case("TOC"));
        field.silent = field.outer_silent || is_contents;
        field.link = field_link(&parts).map(Arc::from).or_else(|| field.outer_link.clone());
    }

    fn drawing(
        &mut self,
        node: Node,
        depth: usize,
        look: &RunFormat,
        around: &RunContext,
        line: &mut Line,
        sink: &mut BlockBuilder,
    ) {
        if self.silent() || look.hidden {
            return;
        }
        let mut found = Drawing::default();
        found.look_through(node, depth);
        line.horizontal_line |= found.horizontal_line;
        let alt = found.alt.unwrap_or("").trim().to_string();
        for id in &found.pictures {
            let destination = self.picture(id);
            let piece = match destination {
                Some(destination) => Piece::Image(Image {
                    source: Source::Address(destination),
                    alt: alt.clone(),
                }),
                None => Piece::Text(alt.clone()),
            };
            line.text_outside_links = true;
            line.spans.push(Span {
                piece,
                marks: look.marks,
                link: self.link_here(around),
            });
        }
        if found.pictures.is_empty() && found.boxes.is_empty() && !alt.is_empty() {
            line.text_outside_links = true;
            line.spans.push(Span {
                piece: Piece::Text(alt),
                marks: look.marks,
                link: self.link_here(around),
            });
        }
        for text_box in found.boxes {
            self.blocks(text_box, depth, sink);
        }
    }

    /// Where the picture of a relationship goes, written once however often it is used; `None` if it cannot be used.
    fn picture(&mut self, id: &str) -> Option<String> {
        let Some(part) = self.rels.get(id).and_then(|rel| rel.part.clone()) else {
            let named = self.rels.get(id).map_or(id, |rel| rel.target.as_str()).to_string();
            if self.without_part.insert(named.clone()) {
                self.missing.push(MissingPicture {
                    part: named,
                    reason: MissingReason::NoPart,
                });
            }
            return None;
        };
        if let Some(known) = self.pictures.get(&part) {
            return known.clone();
        }
        let destination = match self.archive.get(&part) {
            // What was read before this picture used up what the archive may inflate in all, so the file as a whole is
            // too large; a picture too large on its own is only missing.
            Err(ZipError::TooLarge) => {
                self.over_budget = true;
                None
            }
            read => match self.new_picture(read) {
                Ok(destination) => Some(destination),
                Err(reason) => {
                    self.missing.push(MissingPicture {
                        part: part.clone(),
                        reason,
                    });
                    None
                }
            },
        };
        self.pictures.insert(part, destination.clone());
        destination
    }

    /// Where a picture read from the archive goes, or why it goes nowhere.
    fn new_picture(&mut self, read: Result<Option<Vec<u8>>, ZipError>) -> Result<String, MissingReason> {
        let bytes = match read {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return Err(MissingReason::Absent),
            Err(ZipError::EntryTooLarge) => return Err(MissingReason::TooLarge),
            Err(_) => return Err(MissingReason::Damaged),
        };
        let extension = picture_extension(&bytes).ok_or(MissingReason::UnknownFormat)?;
        let file_name = format!("image{}.{extension}", self.media.len() + 1);
        let destination = picture_destination(self.images_folder, &file_name);
        self.media.push(Media { file_name, bytes });
        Ok(destination)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_instruction_splits_into_its_words() {
        assert_eq!(
            instruction_parts(" HYPERLINK  \"https://a.bc/d e\" \\o \"Tipp\" "),
            ["HYPERLINK", "https://a.bc/d e", "\\o", "Tipp"]
        );
        assert_eq!(instruction_parts(""), Vec::<String>::new());
    }

    #[test]
    fn a_hyperlink_field_leads_where_its_instruction_says() {
        let link = |instruction: &str| field_link(&instruction_parts(instruction));
        assert_eq!(link("HYPERLINK \"https://a.bc\""), Some("https://a.bc".to_string()));
        assert_eq!(link("hyperlink \"a.md\" \\l \"b c\""), Some("a.md#bc".to_string()));
        assert_eq!(link("HYPERLINK \\l \"_Toc1\" \\h"), Some("#_Toc1".to_string()));
        assert_eq!(
            link("HYPERLINK \"x\" \"y\" \\t \"_blank\" \\o \"Tip\""),
            Some("x".to_string())
        );
        assert_eq!(link("HYPERLINK \\h"), None);
        assert_eq!(link("PAGEREF _Toc1 \\h"), None);
        assert_eq!(link(""), None);
    }

    #[test]
    fn symbols_read_as_the_characters_they_show() {
        let read = |xml: &str| {
            let doc = format!("<w:sym xmlns:w=\"{W}\" {xml}/>");
            symbol(roxmltree::Document::parse(&doc).unwrap().root_element())
        };
        assert_eq!(read("w:font=\"Symbol\" w:char=\"F061\""), Some('α'));
        assert_eq!(read("w:font=\"symbol\" w:char=\"0044\""), Some('Δ'));
        assert_eq!(read("w:font=\"Symbol\" w:char=\"F0AE\""), Some('→'));
        for (code, wanted) in [
            ("A3", '≤'),
            ("A5", '∞'),
            ("AC", '←'),
            ("AD", '↑'),
            ("AF", '↓'),
            ("B1", '±'),
            ("B3", '≥'),
            ("B4", '×'),
            ("B7", '•'),
            ("B8", '÷'),
            ("B9", '≠'),
            ("D6", '√'),
            ("E5", '∑'),
        ] {
            assert_eq!(
                read(&format!("w:font=\"Symbol\" w:char=\"{code}\"")),
                Some(wanted),
                "{code}"
            );
        }
        assert_eq!(read("w:font=\"Symbol\" w:char=\"F001\""), char::from_u32(0xF001));
        assert_eq!(read("w:font=\"Wingdings\" w:char=\"F0E0\""), char::from_u32(0xF0E0));
        assert_eq!(read("w:font=\"Symbol\" w:char=\"20AC\""), Some('€'));
        assert_eq!(read("w:font=\"Symbol\" w:char=\"xyz\""), None);
        assert_eq!(read(""), None);
        assert_eq!(read("w:char=\"110000\""), None);
    }

    #[test]
    fn a_picture_is_known_by_its_first_bytes() {
        for (bytes, wanted) in [
            (&b"\x89PNG\r\n\x1a\n"[..], Some("png")),
            (b"\xFF\xD8\xFF\xE0", Some("jpeg")),
            (b"GIF89a", Some("gif")),
            (b"GIF87a", Some("gif")),
            (b"RIFF0000WEBP", Some("webp")),
            (b"RIFF0000WAVE", None),
            (b"BM00", Some("bmp")),
            (b"II*\0", Some("tiff")),
            (b"MM\0*", Some("tiff")),
            (&[0xD7, 0xCD, 0xC6, 0x9A], Some("wmf")),
            (&[1, 0, 9, 0], Some("wmf")),
            (&[2, 0, 9, 0], Some("wmf")),
            (b"<svg/>", None),
            (b"", None),
        ] {
            assert_eq!(picture_extension(bytes), wanted, "{bytes:?}");
        }
        let mut emf = vec![1, 0, 0, 0];
        emf.extend_from_slice(&[0; 36]);
        emf.extend_from_slice(b" EMF");
        assert_eq!(picture_extension(&emf), Some("emf"));
        assert_eq!(picture_extension(&emf[..30]), None);
    }

    #[test]
    fn a_destination_spells_the_folder_so_that_it_parses() {
        assert_eq!(picture_destination("", "a.png"), "a.png");
        assert_eq!(picture_destination("/", "a.png"), "a.png");
        assert_eq!(picture_destination("Bilder", "a.png"), "Bilder/a.png");
        assert_eq!(picture_destination("Meine Bilder/", "a.png"), "Meine%20Bilder/a.png");
        assert_eq!(picture_destination("100%#1?", "a.png"), "100%25%231%3F/a.png");
    }
}
