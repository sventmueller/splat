use std::ops::Range;

use super::{ATTACHMENT, Atom, Builder, CharMap, ImageAtom, LINE_SEPARATOR, Link, Run, RunMap, RunStyle};
use splat_markdown::doc::{Inline, Piece};
use splat_markdown::style::{Baseline, TextStyle};
use splat_markdown::text::{Byte, TextAt, count32, decode_entity};
use splat_markdown::typography::{Running, TextMark, Within, citation_shown, typographic_source};

impl Builder<'_> {
    pub(super) fn inlines(&self, inlines: &[Inline], style: &RunStyle) -> Vec<Run> {
        let mut runs = Vec::new();
        for i in inlines {
            self.inline(i, style, &mut runs);
        }
        runs
    }

    fn inline(&self, i: &Inline, style: &RunStyle, runs: &mut Vec<Run>) {
        let nested = |s: RunStyle, runs: &mut Vec<Run>| {
            for c in &i.children {
                self.inline(c, &s, runs);
            }
        };
        let at = i.range.clone();
        match i.piece() {
            Piece::Text(t) => self.text_runs(t, at, style, runs),
            Piece::SoftBreak => runs.push(atom_styled(Atom::Text, " ", at, style)),
            Piece::LineBreak => runs.push(atom_styled(Atom::Text, &LINE_SEPARATOR.to_string(), at, style)),
            Piece::Code(literal) => runs.push(self.aligned(literal, i.code_content(self.text), style.code())),
            Piece::Html(h) => runs.push(self.aligned(
                h,
                at,
                RunStyle {
                    html: true,
                    ..style.clone()
                },
            )),
            Piece::Styled => nested(style.with_text(|text| text.inside(&i.kind)), runs),
            Piece::Picture(picture) => runs.push(atom_styled(
                Atom::Image(Box::new(ImageAtom {
                    url: picture.url.into(),
                    alt: picture.alt.as_ref().into(),
                    width: picture.width,
                    height: picture.height,
                })),
                &ATTACHMENT.to_string(),
                at,
                style,
            )),
            Piece::Emoji(emoji) => runs.push(atom_styled(Atom::Text, emoji, at, style)),
            Piece::Hidden => {}
            Piece::WikiPage(target) => nested(
                RunStyle {
                    link: Some(Link::WikiPage(target.into())),
                    ..style.clone()
                },
                runs,
            ),
            Piece::Link { url, .. } => nested(
                RunStyle {
                    link: Some(Link::Address(url.into())),
                    ..style.clone()
                },
                runs,
            ),
            Piece::FootnoteRef { name, ix } => {
                let mut s = style.with_text(|text| TextStyle {
                    baseline: Baseline::Super,
                    ..text
                });
                s.link = Some(Link::Footnote(name.into()));
                let atom = Atom::FootnoteRef { name: name.into() };
                runs.push(atom_styled(atom, &format!("[{ix}]"), at, &s));
            }
            Piece::Math { literal, display } => {
                if self.resolver.math_ready(literal, display) {
                    let atom = Atom::Math {
                        tex: literal.into(),
                        display,
                    };
                    runs.push(atom_styled(atom, &ATTACHMENT.to_string(), at, style));
                } else {
                    let shown = self.text.at(at.clone()).to_string();
                    runs.push(atom_styled(Atom::Text, &shown, at, &style.code()));
                }
            }
        }
    }

    /// Running text, with typographic punctuation if wanted, split where tags, references and citations start.
    fn text_runs(&self, literal: &str, src: Range<Byte>, style: &RunStyle, runs: &mut Vec<Run>) {
        if literal.is_empty() {
            return;
        }
        let within = Within {
            code: style.text.code,
            link: style.link.is_some(),
            html: style.html,
        };
        let mut open = self.open.get();
        let Running { shown, marks: found } = Running::of(
            self.text,
            literal,
            src.start,
            within,
            self.resolver.quotes(),
            &mut open,
            self.resolver.references(),
        );
        self.open.set(open);
        let whole = self.aligned(&shown, src.clone(), style.clone());
        if found.is_empty() {
            runs.push(whole);
            return;
        }
        let RunMap::Text { chars, end } = &whole.map else {
            runs.push(whole);
            return;
        };
        self.marked_runs(&shown, chars, *end, found, style, runs);
    }

    /// `shown`, mapped to the source by `chars`, as runs of its own for what `found` marks in it and for what lies between.
    fn marked_runs(
        &self,
        shown: &str,
        chars: &CharMap,
        end: Byte,
        found: Vec<(Range<usize>, TextMark)>,
        style: &RunStyle,
        runs: &mut Vec<Run>,
    ) {
        // Marks come in text order, and counting from the start each time would take quadratic time in a long paragraph.
        let counted = std::cell::Cell::new((0, 0));
        let char_at = |byte: usize| {
            let (from, before) = Some(counted.get()).filter(|(from, _)| *from <= byte).unwrap_or((0, 0));
            let at = before + shown[from..byte].chars().count();
            counted.set((byte, at));
            at
        };
        let source_at = |byte: usize| match chars {
            CharMap::Verbatim(start) if byte < shown.len() => *start + byte,
            CharMap::Verbatim(_) => end,
            CharMap::Offsets(..) => chars.get(shown, char_at(byte)).unwrap_or(end),
        };
        let piece = |bytes: Range<usize>, style: RunStyle| -> Run {
            let first = char_at(bytes.start);
            let count = shown[bytes.clone()].chars().count();
            let map = match chars {
                CharMap::Verbatim(start) => CharMap::Verbatim(*start + bytes.start),
                CharMap::Offsets(base, offsets) => {
                    let head = offsets.get(first).copied().unwrap_or(0);
                    CharMap::Offsets(
                        *base + head as usize,
                        offsets[first..(first + count).min(offsets.len())]
                            .iter()
                            .map(|o| o - head)
                            .collect(),
                    )
                }
            };
            Run {
                text: shown[bytes.clone()].to_string(),
                style,
                map: RunMap::Text {
                    chars: map,
                    end: source_at(bytes.end),
                },
            }
        };
        let mut at = 0;
        for (range, mark) in found {
            if range.start > at {
                runs.push(piece(at..range.start, style.clone()));
            }
            let mut marked = style.clone();
            match &mark {
                TextMark::Tag(name) => marked.link = Some(Link::Tag(format!("#{name}").into())),
                TextMark::Reference(target) => marked.link = Some(Link::Reference(target.as_str().into())),
                TextMark::Citation(_) => marked.citation = true,
            }
            let resolved = match &mark {
                TextMark::Citation(keys) => citation_shown(keys, |key| self.resolver.citation(key)),
                _ => None,
            };
            match resolved {
                Some(shown_as) => runs.push(atom_styled(
                    Atom::Text,
                    &shown_as,
                    source_at(range.start)..source_at(range.end),
                    &marked,
                )),
                None => runs.push(piece(range.clone(), marked)),
            }
            at = range.end;
        }
        if at < shown.len() {
            runs.push(piece(at..shown.len(), style.clone()));
        }
    }

    pub(super) fn aligned(&self, literal: &str, src: Range<Byte>, style: RunStyle) -> Run {
        match align(literal, self.text, src.clone()) {
            Some(chars) => Run {
                text: literal.to_string(),
                style,
                map: RunMap::Text { chars, end: src.end },
            },
            None => atom_styled(Atom::Text, literal, src, &style),
        }
    }
}

pub(super) fn atom(atom: Atom, shown: &str, src: Range<Byte>) -> Run {
    Run {
        text: shown.to_string(),
        style: RunStyle::default(),
        map: RunMap::Atom { atom, src },
    }
}

pub(super) fn atom_styled(atom: Atom, shown: &str, src: Range<Byte>, style: &RunStyle) -> Run {
    Run {
        text: shown.to_string(),
        style: style.clone(),
        map: RunMap::Atom { atom, src },
    }
}

/// Source offset of every char of `literal` inside `text[src]`, past what only the source has: escapes, entities and
/// container prefixes. `None` if the source does not spell the literal, as a position would then be a guess, and an
/// edit made from it would land in the wrong place.
pub(crate) fn align(literal: &str, text: &str, src: Range<Byte>) -> Option<CharMap> {
    let source = text.at(src.clone());
    if source == literal {
        return Some(CharMap::Verbatim(src.start));
    }
    let mut out = Vec::with_capacity(literal.chars().count());
    let mut i = 0;
    let mut shown = literal;
    // Between the start of a source line and its text, where the prefix of a quote stands.
    let mut in_prefix = src.start == Byte::default() || text.upto(src.start).ends_with('\n');
    while let Some(c) = shown.chars().next() {
        let mut used = c.len_utf8();
        loop {
            let at = src.start + i;
            let rest = &source[i..];
            let Some(sc) = rest.chars().next() else {
                if !c.is_whitespace() {
                    return None;
                }
                out.push(src.end);
                break;
            };
            // Code spans and code blocks keep a reference as it stands; everywhere else the literal has what it means.
            if sc == '&'
                && let Some((decoded, len)) = decode_entity(rest)
                && !shown.starts_with(&rest[..len])
            {
                let same = shown.starts_with(&*decoded);
                if same || typographic_source(c, &decoded).is_some() {
                    let count = if same { decoded.chars().count() } else { 1 };
                    out.extend(std::iter::repeat_n(at, count));
                    used = if same { decoded.len() } else { used };
                    i += len;
                    in_prefix = false;
                    break;
                }
            }
            if sc == c || (c == ' ' && sc == '\n') {
                out.push(at);
                i += sc.len_utf8();
                in_prefix = sc == '\n';
                break;
            }
            let escaped = rest
                .strip_prefix('\\')
                .and_then(|r| r.chars().next())
                .filter(char::is_ascii_punctuation);
            if escaped == Some(c) {
                out.push(at);
                i += 1 + c.len_utf8();
                in_prefix = false;
                break;
            }
            if let Some(len) = typographic_source(c, rest) {
                out.push(at);
                i += len;
                in_prefix = false;
                break;
            }
            if sc == '\n' {
                in_prefix = true;
                i += 1;
            } else if sc.is_whitespace() || escaped.is_some() || (in_prefix && sc == '>') {
                i += sc.len_utf8();
            } else if c.is_whitespace() {
                // The indentation of an HTML block's first line stands before its range.
                out.push(at);
                break;
            } else {
                return None;
            }
        }
        shown = &shown[used..];
    }
    let base = out.first().copied().unwrap_or(src.start);
    Some(CharMap::Offsets(base, out.iter().map(|&o| count32(o - base)).collect()))
}

#[cfg(test)]
mod tests {
    use super::super::tests::{Everything, Surroundings, laid, paras};
    use super::super::{Marker, NoImages, Para, Resolver, layout};
    use super::*;
    use splat_markdown::doc::DiagramKind;
    use splat_markdown::reader::parse;
    use splat_markdown::text::bytes;
    use splat_markdown::typography::Quotes;

    #[test]
    fn marks_in_a_mapped_text_find_their_place_in_the_source() {
        struct Marks;
        impl Resolver for Marks {
            fn math_ready(&self, _: &str, _: bool) -> bool {
                false
            }
            fn diagram_ready(&self, _: &str, _: DiagramKind) -> bool {
                false
            }
        }
        let text = "ä \\* #eins und ö #zwei und #drei";
        let doc = splat_markdown::reader::parse(text);
        let laid = layout(&doc, &Marks, None);
        let tags: Vec<&str> = laid.blocks[0].paras[0]
            .runs
            .iter()
            .filter(|r| matches!(r.style.link, Some(Link::Tag(_))))
            .map(|r| match &r.map {
                RunMap::Text { chars, end } => text.at(chars.get(&r.text, 0).unwrap()..*end),
                RunMap::Atom { .. } => "",
            })
            .collect();
        assert_eq!(tags, ["#eins", "#zwei", "#drei"]);
    }

    #[test]
    fn every_named_entity_finds_its_place_in_the_source() {
        let t = "Caf&eacute; &NotEqualTilde; und &rarr; ende\n";
        let (chars, shown) = map_of(t);
        assert_eq!(shown, "Café ≂\u{338} und → ende");
        let at = |c: char| chars[shown.chars().position(|s| s == c).unwrap()];
        assert_eq!(t.at(at('é')..at('é') + 8), "&eacute;");
        assert_eq!(chars[5], chars[6], "one reference, two characters");
        assert_eq!(t.at(at('→')..at('→') + 6), "&rarr;");
        assert_eq!(t.at(at('n')..at('n') + 2), "nd");
    }

    #[test]
    fn a_literal_the_source_does_not_spell_is_no_map() {
        assert!(align("xyz", "abc", bytes(0..3)).is_none());
        assert!(align("Café", "Caf&unbekannt;", bytes(0..14)).is_none());
        assert!(align("ab cd", "ab", bytes(0..2)).is_none());
        // Whitespace is what both sides may have without the other.
        assert!(align("  a\n", "a", bytes(0..1)).is_some());
        assert!(align("a  b", "a\n> b", bytes(0..5)).is_some());
        // Inside code a reference stays as it stands.
        let code = "&amp;\n> x";
        let Some(CharMap::Offsets(base, offsets)) = align("&amp;\nx", code, bytes(0..code.len())) else {
            panic!()
        };
        assert_eq!(base, Byte::new(0));
        assert_eq!(offsets.last().map(|&o| o as usize), code.find('x'));
    }

    #[test]
    fn text_without_a_place_in_the_source_edits_as_a_whole() {
        let src = "abc\n";
        let b = Builder {
            text: src,
            resolver: &NoImages,
            group: 0,
            table: 0,
            paras: Vec::new(),
            open: Default::default(),
            embeds: 0,
            embeds_refused: false,
        };
        let run = b.aligned("xyz", bytes(0..3), RunStyle::default());
        assert_eq!(
            run.map,
            RunMap::Atom {
                atom: Atom::Text,
                src: bytes(0..3)
            }
        );
    }

    #[test]
    fn a_quoted_entity_turns_typographic_in_its_place() {
        struct German;
        impl Resolver for German {
            fn math_ready(&self, _: &str, _: bool) -> bool {
                false
            }
            fn diagram_ready(&self, _: &str, _: DiagramKind) -> bool {
                false
            }
            fn quotes(&self) -> Option<Quotes> {
                Some(Quotes {
                    open: '„',
                    close: '“',
                    open_single: '‚',
                    close_single: '‘',
                })
            }
        }
        let t = "a &quot;b&quot; \\\"c\\\" d\n";
        let l = layout(&parse(t), &German, None);
        let run = &l.blocks[0].paras[0].runs[0];
        assert_eq!(run.text, "a „b“ „c“ d");
        assert!(matches!(run.map, RunMap::Text { .. }), "{run:?}");
    }

    fn map_of(t: &str) -> (Vec<Byte>, String) {
        let l = layout(&parse(t), &NoImages, None);
        let run = &l.blocks[0].paras[0].runs[0];
        let RunMap::Text { chars, .. } = &run.map else {
            panic!("{run:?}")
        };
        let offsets = (0..run.text.chars().count())
            .map(|k| chars.get(&run.text, k).unwrap())
            .collect();
        (offsets, run.text.clone())
    }

    #[test]
    fn escapes_and_entities_align_to_source() {
        let t = "a \\* b &amp; c\n";
        let d = parse(t);
        let l = layout(&d, &NoImages, None);
        let run = &l.blocks[0].paras[0].runs[0];
        let RunMap::Text { chars, .. } = &run.map else { panic!() };
        let chars: Vec<Byte> = (0..run.text.chars().count())
            .map(|k| chars.get(&run.text, k).unwrap())
            .collect();
        let lit: Vec<char> = l.blocks[0].paras[0].runs[0].text.chars().collect();
        assert_eq!(lit[2], '*');
        assert_eq!(t.at(chars[2]..chars[2] + 2), "\\*");
        assert_eq!(lit[6], '&');
        assert_eq!(t.at(chars[6]..chars[6] + 5), "&amp;");
        assert_eq!(chars[8], Byte::new(t.find(" c").unwrap() + 1));
        assert_eq!(decode_entity(&format!("&{}😀;", "a".repeat(38))), None);
    }

    #[test]
    fn quotes_without_spaces_pair_across_line_breaks_and_formatting() {
        let t = "彼は\"今日は\n晴れ\"です\n\n\"**強調**\"と\"言\"\n";
        let d = parse(t);
        let l = layout(&d, &Surroundings, None);
        let shown: Vec<String> = l.blocks[..2]
            .iter()
            .map(|b| b.paras[0].runs.iter().map(|r| r.text.as_str()).collect())
            .collect();
        assert!(
            shown[0].starts_with("彼は„今日は") && shown[0].ends_with("晴れ“です"),
            "{shown:?}"
        );
        assert_eq!(shown[1], "„強調“と„言“");
    }

    #[test]
    fn running_text_marks_and_embedded_notes() {
        let t = "Er sagte \"Ja\" -- #idee, #12 von @anna [@doe] [@fehlt]\n\n![[Notiz]]\n";
        let d = parse(t);
        let l = layout(&d, &Surroundings, None);
        let runs = &l.blocks[0].paras[0].runs;
        let shown: String = runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(shown, "Er sagte „Ja“ – #idee, #12 von @anna (Doe 2020) [@fehlt]");
        let quote = runs.iter().find(|r| r.text.starts_with("Er sagte")).unwrap();
        let RunMap::Text { chars, .. } = &quote.map else {
            panic!()
        };
        assert_eq!(&t.from(chars.get(&quote.text, 9).unwrap())[..1], "\"");
        let tag = runs.iter().find(|r| r.text == "#idee").unwrap();
        assert_eq!(tag.style.link, Some(Link::Tag("#idee".into())));
        assert!(
            runs.iter()
                .any(|r| r.text == "#12" && r.style.link == Some(Link::Reference("#12".into())))
        );
        assert!(
            runs.iter()
                .any(|r| r.text == "@anna" && matches!(r.style.link, Some(Link::Reference(_))))
        );
        let cite = runs.iter().find(|r| r.text == "(Doe 2020)").unwrap();
        assert!(matches!(&cite.map, RunMap::Atom { src, .. } if t.at(src.clone()) == "[@doe]"));
        assert!(runs.iter().any(|r| r.text == "[@fehlt]" && r.style.citation));
        let embedded: Vec<&Para> = l.blocks[1].paras.iter().collect();
        assert_eq!(embedded.len(), 3, "{embedded:?}");
        assert!(
            embedded
                .iter()
                .all(|p| p.style.quote_depth == 1 && t.at(p.src.clone()) == "![[Notiz]]")
        );
        assert!(matches!(
            embedded[1].style.marker,
            Some(Marker::Task { symbol: None, .. })
        ));
        assert!(
            embedded[2]
                .runs
                .iter()
                .any(|r| r.style.link == Some(Link::WikiPage("Notiz".into())))
        );
    }

    #[test]
    fn inline_html_is_marked_and_a_footnote_reference_raised() {
        let l = laid("a <foo> b[^x]\n\n[^x]: c\n");
        let runs = &paras(&l)[0].runs;
        assert!(runs.iter().find(|r| r.text == "<foo>").unwrap().style.html);
        assert!(runs.iter().find(|r| r.text == "[1]").unwrap().style.text.baseline == Baseline::Super);
    }

    #[test]
    fn code_html_and_links_keep_their_text_as_it_is() {
        let shown = |t: &str| {
            let l = layout(&parse(t), &Everything, None);
            let runs: Vec<Run> = paras(&l)[0].runs.clone();
            runs
        };
        let kbd = shown("<kbd>\"a\"</kbd>\n");
        assert!(kbd.iter().any(|r| r.text == "\"a\""), "{kbd:?}");
        let html = shown("<p>\"a\"</p>\n");
        assert!(html.iter().any(|r| r.text.contains("\"a\"")), "{html:?}");
        assert!(
            shown("[#tag](https://x.de)\n")
                .iter()
                .all(|r| !matches!(r.style.link, Some(Link::Tag(_))))
        );
        assert!(
            shown("<kbd>#tag</kbd>\n")
                .iter()
                .all(|r| !matches!(r.style.link, Some(Link::Tag(_))))
        );
        assert!(
            shown("<p>#tag</p>\n")
                .iter()
                .all(|r| !matches!(r.style.link, Some(Link::Tag(_))))
        );
    }

    #[test]
    fn a_mark_at_either_end_of_a_text_leaves_no_empty_run() {
        for t in ["#tag x\n", "x #tag\n", "#tag\n"] {
            let l = layout(&parse(t), &Everything, None);
            assert!(paras(&l)[0].runs.iter().all(|r| !r.text.is_empty()), "{t}");
        }
    }

    #[test]
    fn a_line_break_collapsed_to_a_space_still_finds_its_place() {
        let t = "x `a\nb` y\n";
        let l = laid(t);
        let run = paras(&l)[0].runs.iter().find(|r| r.text.contains('b')).unwrap();
        let RunMap::Text { chars, .. } = &run.map else {
            panic!("{run:?}");
        };
        let b = run.text.chars().position(|c| c == 'b').unwrap();
        assert_eq!(chars.get(&run.text, b), Some(Byte::new(5)));
        assert_eq!(
            chars.get(&run.text, b - 1),
            Some(Byte::new(4)),
            "the space stands for the line break"
        );
    }
}
