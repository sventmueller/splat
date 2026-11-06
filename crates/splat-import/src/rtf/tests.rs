//! RTF as Cocoa and Word write it, and what is made to hurt.

use splat_markdown::reader::parse;
use splat_testkit::{Rng, rounds};

use super::*;

fn markdown(rtf: &str) -> String {
    from_rtf(rtf.as_bytes()).into_markdown()
}

/// `body` in the frame of a document.
fn doc(body: &str) -> String {
    format!("{{\\rtf1\\ansi{{\\fonttbl\\f0\\fswiss Helvetica;\\f1\\fmodern Menlo-Regular;}}\\f0\\fs24 {body}}}")
}

const COCOA: &str = r#"{\rtf1\ansi\ansicpg1252\cocoartf2822
\cocoatextscaling0\cocoaplatform0{\fonttbl\f0\fswiss\fcharset0 Helvetica;\f1\fswiss\fcharset0 Helvetica-Bold;\f2\fswiss\fcharset0 Helvetica-Oblique;
\f3\fmodern\fcharset0 Menlo-Regular;}
{\colortbl;\red255\green255\blue255;\red0\green0\blue233;}
{\*\expandedcolortbl;;\cssrgb\c0\c0\c93333;}
{\*\listtable{\list\listtemplateid1\listhybrid{\listlevel\levelnfc23\levelnfcn23\leveljc0\leveljcn0\levelfollow0\levelstartat1\levelspace360\levelindent0{\*\levelmarker \{disc\}}{\leveltext\leveltemplateid1\'01\uc0\u8226 ;}{\levelnumbers;}\fi-360\li720\lin720 }{\listname ;}\listid1}}
{\*\listoverridetable{\listoverride\listid1\listoverridecount0\ls1}}
\paperw11900\paperh16840\margl1440\margr1440\vieww11520\viewh8400\viewkind0
\pard\tx720\tx1440\pardirnatural\partightenfactor0

\f1\b\fs48 \cf0 Titel
\f0\b0\fs24 \
Hallo 
\f1\b Welt
\f0\b0  und 
\f2\i kursiv
\f0\i0 , 
\ul unter
\ulnone  
\strike \strikec0 weg
\strike0\striked0  x
\super 2
\nosupersub  ist \uc0\u8364  \'e4.\
\pard\tx220\tx720\pardeftab720\li720\fi-720\pardirnatural\partightenfactor0
\ls1\ilvl0\cf0 {\listtext	\uc0\u8226 	}eins\
{\listtext	\uc0\u8226 	}zwei\
\pard\tx940\tx1440\pardeftab720\li1440\fi-1440\pardirnatural\partightenfactor0
\ls1\ilvl1\cf0 {\listtext	\uc0\u8259 	}tief\
\pard\tx720\tx1440\pardirnatural\partightenfactor0
\cf0 \
Ein {\field{\*\fldinst{HYPERLINK "https://a.bc/d"}}{\fldrslt \cf3 \ul \ulc3 Link}} hier.\
}"#;

#[test]
fn what_textedit_writes_keeps_its_structure() {
    assert_eq!(
        markdown(COCOA),
        "# Titel\n\nHallo **Welt** und *kursiv*, <ins>unter</ins> ~~weg~~ x<sup>2</sup> ist € ä.\n\n- eins\n- zwei\n  - tief\n\nEin [Link](https://a.bc/d) hier.\n"
    );
}

const COCOA_TABLE: &str = r"{\rtf1\ansi\ansicpg1252\cocoartf2822
{\fonttbl\f0\fswiss\fcharset0 Helvetica;}
\pard\pardirnatural\partightenfactor0

\f0\fs24 \cf0 Davor\
\
\itap1\trowd \taflags1 \trgaph108\trleft-108 \trbrdrt\brdrnil \tbllvertalt\tblindtype3\tblind0\tblindtype3 \clvertalt \clshdrawnil \clwWidth1000\clftsWidth3 \cellx4320
\clvertalt \clshdrawnil \clwWidth1000\clftsWidth3 \cellx8640
\pard\intbl\itap1\pardeftab720\partightenfactor0
\cf0 A\cell
\pard\intbl\itap1\pardeftab720\partightenfactor0
\cf0 \b B\b0 \cell \row

\itap1\trowd \taflags1 \clvertalt \cellx4320
\clvertalt \cellx8640
\pard\intbl\itap1\pardeftab720\partightenfactor0
\cf0 1\
2\cell
\pard\intbl\itap1\pardeftab720\partightenfactor0
\cf0 \cell \lastrow\row
\pard\pardirnatural\partightenfactor0
\cf0 Danach\
}";

#[test]
fn a_table_from_cocoa_is_a_table_with_lines_in_its_cells() {
    assert_eq!(
        markdown(COCOA_TABLE),
        "Davor\n\n| A | B |\n|---|---|\n| 1<br>2 | |\n\nDanach\n"
    );
}

const WORD: &str = r#"{\rtf1\adeflang1025\ansi\ansicpg1252\uc1\adeff31507\deff0\stshfdbch31505\stshfloch31506\deflang1031\deflangfe1031\themelang1031
{\fonttbl{\f0\fbidi \froman\fcharset0\fprq2{\*\panose 02020603050405020304}Times New Roman;}{\f3\fbidi \froman\fcharset2\fprq2{\*\panose 05050102010706020507}Symbol;}
{\f11\fbidi \fmodern\fcharset0\fprq1{\*\panose 020b0609020204030204}Consolas{\*\falt Courier};}}
{\colortbl;\red0\green0\blue0;}{\*\defchp \fs22}{\stylesheet{\ql Normal;}{\*\cs10 Default Paragraph Font;}}
{\*\listtable{\list\listtemplateid123{\listlevel\levelnfc23{\leveltext\'01\u-3913 ?;}}\listid1}}{\*\listoverridetable{\listoverride\listid1\ls1}}
{\*\rsidtbl \rsid123\rsid456}{\*\generator Microsoft Word 16.0;}{\info{\title Geheim}{\author Sven}}
{\*\themedata 504b0304}{\*\datastore 01050000}{\*\latentstyles\lsdstimax376{\lsdlockedexcept \lsdqformat1 heading 1;}}
\paperw11906\paperh16838\margl1417\margr1417\margt1417\margb1134\gutter0\ltrsect
\widowctrl\ftnbj\aenddoc\trackmoves0\trackformatting1\headery708\footery708\colsx708
{\header \pard\plain Kopfzeile\par}{\footer \pard\plain Fusszeile\par}
\pard\plain \ltrpar\ql \li0\ri0\sa160\widctlpar\wrapdefault\itap0 \rtlch\fcs1 \af31507\afs22\alang1025 \ltrch\fcs0 \fs22\lang1031\cgrid {\rtlch\fcs1 \af31507 \ltrch\fcs0 \insrsid123 Das kostet 5 \u8364\'80 und }{\rtlch\fcs1 \ab\af31507 \ltrch\fcs0 \b\insrsid123 fett}{\rtlch\fcs1 \af31507 \ltrch\fcs0 \insrsid123  und }{\rtlch\fcs1 \af31507 \ltrch\fcs0 \f11\insrsid123 code}{\rtlch\fcs1 \af31507 \ltrch\fcs0 \insrsid123 .
\par }\pard\plain \ltrpar\ql \fi-360\li720\ri0\sa160\widctlpar\jclisttab\tx720\wrapdefault\ls1\itap0 \rtlch\fcs1 \af31507\afs22 \ltrch\fcs0 \fs22 {\listtext\pard\plain\ltrpar \rtlch\fcs1 \af3\afs22 \ltrch\fcs0 \f3\fs22\insrsid123 \loch\af3\dbch\af31505\hich\f3 \'b7\tab}{\rtlch\fcs1 \af31507 \ltrch\fcs0 \insrsid123 Punkt eins
\par }\pard\plain \ltrpar\ql \fi-360\li720\ls1\ilvl1\itap0 {\listtext\pard\plain \f3 o\tab}Punkt tief\par
\pard\plain \ltrpar\ql {\field{\*\fldinst {\rtlch\fcs1 \af31507 \ltrch\fcs0 \insrsid123  HYPERLINK "http://a.bc/" \\l "anker" }}{\fldrslt {\rtlch\fcs1 \af31507 \ltrch\fcs0 \ul\cf2\insrsid123 Sprung}}}\sectd\par
\pard\plain Seite {\field{\*\fldinst PAGE }{\fldrslt 7}} Ende\par
{\*\latentstyles x}}"#;

#[test]
fn what_word_writes_keeps_its_text_and_drops_its_tables() {
    assert_eq!(
        markdown(WORD),
        "Das kostet 5 € und **fett** und `code`.\n\n- Punkt eins\n  - Punkt tief\n\n[Sprung](http://a.bc/#anker)\n\nSeite 7 Ende\n"
    );
}

#[test]
fn old_word_lists_mark_their_items_with_pntext() {
    let rtf = r"{\rtf1\ansi\deff0{\fonttbl{\f0 Arial;}{\f1 Symbol;}}\pard{\pntext\f1\'B7\tab}{\*\pn\pnlvlblt\pnf1\pnindent0{\pntxtb\'B7}}\fi-720\li720 Erstens\par{\pntext\f1\'B7\tab}Zweitens\par\pard{\pntext 3.\tab}Drittens\par{\pntext 4.\tab}Viertens\par\pard Text\par}";
    assert_eq!(
        markdown(rtf),
        "- Erstens\n- Zweitens\n\n3. Drittens\n4. Viertens\n\nText\n"
    );
}

#[test]
fn numbers_start_where_the_marker_says_and_boxes_make_tasks() {
    let rtf = doc(r"\pard\ls2\ilvl0 {\listtext 5.}a\par{\listtext 6.}b\par");
    assert_eq!(markdown(&rtf), "5. a\n6. b\n");
    let tasks = doc(r"\pard\ls1 {\listtext \u9744  }a\par{\listtext \uc0\u9745 }b\par{\listtext \uc0\u9746 }c\par");
    assert_eq!(markdown(&tasks), "- [ ] a\n- [x] b\n- [x] c\n");
    assert_eq!(markdown(&doc(r"\pard\ls1 a\par b\par")), "- a\n- b\n");
    assert_eq!(markdown(&doc(r"\pard\ls1\ilvl99 a\par")), "- a\n");
    assert_eq!(markdown(&doc(r"\pard\ls1 \par")), "");
    assert_eq!(
        markdown(&doc(r"\pard\ls1 a\par\pard\ls2 {\listtext 1.}b\par\pard c")),
        "- a\n\n1. b\n\nc\n"
    );
    assert_eq!(
        markdown(&doc(r"\pard\ls1 a\par\pard\par\ls1 b\par")),
        "- a\n- b\n",
        "an empty paragraph does not end a list"
    );
}

#[test]
fn a_marker_is_no_text_of_the_document() {
    let marks_in_marker = doc(r"\pard\ls1 {\listtext\b\f1 1.\line\tab\par\cell\row}a\par b");
    assert_eq!(markdown(&marks_in_marker), "1. a\n\n- b\n");
}

#[test]
fn every_mark_has_its_word_and_zero_ends_it() {
    for (on, off, written) in [
        (r"\b", r"\b0", "**x**"),
        (r"\i", r"\i0", "*x*"),
        (r"\strike", r"\strike0", "~~x~~"),
        (r"\striked1", r"\striked0", "~~x~~"),
        (r"\ul", r"\ul0", "<ins>x</ins>"),
        (r"\ul", r"\ulnone", "<ins>x</ins>"),
        (r"\uld", r"\ulnone", "<ins>x</ins>"),
        (r"\uldb", r"\ulnone", "<ins>x</ins>"),
        (r"\ulwave", r"\ulnone", "<ins>x</ins>"),
        (r"\ulw", r"\ulnone", "<ins>x</ins>"),
        (r"\super", r"\nosupersub", "<sup>x</sup>"),
        (r"\sub", r"\nosupersub", "<sub>x</sub>"),
        (r"\super", r"\super0", "<sup>x</sup>"),
        (r"\sub", r"\sub0", "<sub>x</sub>"),
        (r"\b", r"\plain", "**x**"),
    ] {
        let rtf = doc(&format!("a {on} x{off}  b"));
        assert_eq!(markdown(&rtf), format!("a {written}  b\n").replace("  b", " b"), "{on}");
    }
    assert_eq!(
        markdown(&doc(r"\ulc2 a\strikec2 b")),
        "ab\n",
        "colors of lines are no lines"
    );
    assert_eq!(markdown(&doc(r"\super a\sub b")), "<sup>a</sup><sub>b</sub>\n");
}

#[test]
fn a_group_keeps_its_marks_to_itself() {
    assert_eq!(markdown(&doc(r"a{\b b{\i c}d}e")), "a**b<em>c</em>d**e\n");
    assert_eq!(markdown(&doc(r"\b a{\b0 b}c")), "**a**b**c**\n");
    assert_eq!(markdown(&doc(r"{\f1 c}d")), "`c`d\n");
    assert_eq!(markdown(&doc(r"{\uc3 \u8364 abc}\u8364 ?")), "€€\n");
}

#[test]
fn unicode_escapes_skip_what_they_replace() {
    assert_eq!(markdown(&doc(r"\u228?x")), "äx\n");
    assert_eq!(markdown(&doc(r"\uc1\u-3913 ?x")), "\u{F0B7}x\n");
    assert_eq!(markdown(&doc(r"\uc2\u228??x")), "äx\n");
    assert_eq!(markdown(&doc(r"\uc2\u228\'e4\'e4x")), "äx\n");
    assert_eq!(markdown(&doc(r"\uc0\u228x")), "äx\n");
    assert_eq!(
        markdown(&doc(r"\u228\b x")),
        "äx\n",
        "a control word counts as one unit"
    );
    assert_eq!(markdown(&doc(r"\u228\~x")), "äx\n", "a symbol too");
    assert_eq!(markdown(&doc(r"\uc3\u228{x}")), "äx\n", "a group ends the skipping");
    assert_eq!(markdown(&doc(r"{\uc3\u228}x")), "äx\n");
    assert_eq!(markdown(&doc(r"\uc-1\u228x")), "äx\n");
    assert_eq!(markdown(&doc(r"\uc\u228?x")), "äx\n", "without a number one unit");
    assert_eq!(markdown(&doc(r"\uc0\u70000 x")), "\u{FFFD}x\n");
    assert_eq!(markdown(&doc(r"\uc0\u x")), "x\n", "U+0000 is nothing");
    assert_eq!(markdown(&doc(r"\u x")), "", "with its fallback");
}

#[test]
fn surrogate_pairs_are_joined_and_lone_ones_replaced() {
    assert_eq!(markdown(&doc(r"\uc0\u55357\u56832 x")), "😀x\n");
    assert_eq!(markdown(&doc(r"\uc0\u-10179\u-8704 x")), "😀x\n");
    assert_eq!(markdown(&doc(r"\u55357?\u56832?x")), "😀x\n");
    assert_eq!(markdown(&doc(r"\uc0\u55357 x")), "\u{FFFD}x\n");
    assert_eq!(markdown(&doc(r"\uc0\u56832 x")), "\u{FFFD}x\n");
    assert_eq!(markdown(&doc(r"\uc0\u55357\u55357\u56832")), "\u{FFFD}😀\n");
    assert_eq!(markdown(&doc(r"a\uc0\u55357")), "a\u{FFFD}\n");
    assert_eq!(markdown(r"{\rtf1 a\uc0\u55357"), "a\u{FFFD}\n");
}

#[test]
fn bytes_are_windows_1252() {
    assert_eq!(markdown(&doc(r"\'80\'e4\'9f")), "€äŸ\n");
    assert_eq!(markdown(&doc(r"\'81x")), "\u{FFFD}x\n");
    assert_eq!(markdown(&doc(r"\'zzx")), "zzx\n", "no hex digits, no byte");
    assert_eq!(markdown(&doc(r"\'8zx")), "zx\n");
    let raw = [&b"{\\rtf1 "[..], &[0xE4, 0x80], b"}"].concat();
    assert_eq!(from_rtf(&raw).into_markdown(), "ä€\n");
}

#[test]
fn symbols_and_words_for_characters_read_as_those() {
    assert_eq!(
        markdown(&doc(
            r"a\~b\_c\-d\{e\}f\\g\|h\:i\emdash j\endash k\emspace l\enspace m\qmspace n\bullet o\lquote p\rquote q\ldblquote r\rdblquote s\tab t"
        )),
        "a\u{a0}b\u{2011}cd{e}f\\ghi—j–k\u{2003}l\u{2002}m\u{2005}n•o‘p’q“r”s\tt\n"
    );
}

#[test]
fn line_ends_mean_nothing_unless_escaped() {
    assert_eq!(markdown("{\\rtf1 a\nb\r\nc}"), "abc\n");
    assert_eq!(markdown("{\\rtf1 a\\\nb\\\r\nc}"), "a\n\nb\n\nc\n");
    assert_eq!(markdown(&doc(r"a\line b")), "a\\\nb\n");
    assert_eq!(markdown(&doc(r"a\par b\sect c\page d")), "a\n\nb\n\nc\n\nd\n");
    assert_eq!(markdown(&doc(r"\pard a\pard b")), "ab\n", "`\\pard` ends no paragraph");
}

#[test]
fn destinations_that_hold_no_text_are_skipped_whole() {
    for destination in [
        r"\colortbl",
        r"\stylesheet",
        r"\info",
        r"\pict",
        r"\object",
        r"\header",
        r"\headerl",
        r"\headerr",
        r"\headerf",
        r"\footer",
        r"\footerl",
        r"\footerr",
        r"\footerf",
        r"\mmathPict",
        r"\nonshppict",
        r"\ftnsep",
        r"\annotation",
        r"\listtable",
        r"\generator",
        r"\shp",
        r"\*\anything",
        r"\*\generator",
        r"\*\pn",
    ] {
        assert_eq!(
            markdown(&doc(&format!("a{{{destination} geheim {{nochmal \\b}} \\par}}b"))),
            "ab\n",
            "{destination}"
        );
    }
    assert_eq!(
        markdown(&doc(r"a\headery708\footery708 b")),
        "ab\n",
        "they only look like destinations"
    );
    assert_eq!(markdown(&doc(r"\*x")), "x\n", "a star in front of text means nothing");
    assert_eq!(markdown(&doc(r"a{\*\*\anything x}b")), "ab\n");
    assert_eq!(markdown(&doc(r"{\*{x}}")), "x\n");
    assert_eq!(markdown(&doc(r"{\*}x")), "x\n");
}

/// The pictures of a document, with their kind, size and description.
fn pictures(rtf: &str) -> Vec<(&'static str, usize, String)> {
    let mut found = Vec::new();
    crate::rich::visit_spans(&mut from_rtf(rtf.as_bytes()).blocks, &mut |span| {
        if let Piece::Image(Image {
            source: Source::Data { mime, bytes },
            alt,
        }) = &span.piece
        {
            found.push((*mime, bytes.len(), alt.clone()));
        }
    });
    found
}

const PNG: &str = "89504e470d0a1a0a0000";

#[test]
fn pictures_word_writes_come_with_their_description() {
    let picture = |blip: &str, data: &str| {
        doc(&format!(
            r"a{{\*\shppict{{\pict{{\*\picprop\shplid1025{{\sp{{\sn fFlipH}}{{\sv 0}}}}{{\sp{{\sn wzDescription}}{{\sv Ein {{\b blaues}} Bild}}}}}}\picw706\pich529{blip}\bliptag255{{\*\blipuid 68b5c465}}{data}}}}}{{\nonshppict{{\pict\wmetafile8 0102}}}}b\par"
        ))
    };
    assert_eq!(
        pictures(&picture(r"\pngblip", &format!("{PNG}\n0a 0b"))),
        [("image/png", 12, "Ein blaues Bild".to_string())]
    );
    assert_eq!(
        pictures(&picture(r"\jpegblip", "ffd8ffe0")),
        [("image/jpeg", 4, "Ein blaues Bild".to_string())]
    );
    assert_eq!(markdown(&picture(r"\emfblip", "0100")), "aEin blaues Bildb\n");
    assert_eq!(
        markdown(&picture(r"\pngblip", "0100")),
        "aEin blaues Bildb\n",
        "what says it is a PNG has to be one"
    );
    assert_eq!(
        markdown(&doc(&format!(r"a{{\pict\pngblip {PNG}}}b"))),
        "ab\n",
        "pasted data needs a place"
    );
    assert_eq!(
        pictures(&doc(&format!(r"{{\pict\pngblip {PNG}{{\pict\jpegblip ffd8ff}}}}"))),
        [("image/png", 10, String::new())],
        "a picture within a picture is none"
    );
}

#[test]
fn a_picture_beyond_the_limit_is_dropped_without_keeping_it() {
    let mut rtf = format!("{{\\rtf1 {{\\pict\\pngblip\\bin{} ", MOST_PICTURE_BYTES + 1).into_bytes();
    rtf.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    rtf.resize(rtf.len() + MOST_PICTURE_BYTES - 7, 0);
    rtf.extend_from_slice(b"}}");
    let mut found = 0;
    crate::rich::visit_spans(&mut from_rtf(&rtf).blocks, &mut |span| {
        found += usize::from(matches!(span.piece, Piece::Image(_)));
    });
    assert_eq!(found, 0);
    let mut rtf = format!("{{\\rtf1 {{\\pict\\pngblip\\bin{} ", MOST_PICTURE_BYTES).into_bytes();
    rtf.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    rtf.resize(rtf.len() + MOST_PICTURE_BYTES - 8, 0);
    rtf.extend_from_slice(b"}}");
    let mut found = 0;
    crate::rich::visit_spans(&mut from_rtf(&rtf).blocks, &mut |span| {
        found += usize::from(matches!(span.piece, Piece::Image(_)));
    });
    assert_eq!(found, 1);
}

#[test]
fn word_footnotes_come_with_their_reference() {
    let note = |text: &str| format!(r"{{\super\chftn{{\footnote\pard\plain\fs20{{\super\chftn}} {text}}}}}");
    assert_eq!(
        markdown(&doc(&format!(
            r"Erst{} dann{}.\par Weiter.",
            note(r"Die {\i erste}.\par Zweiter Absatz."),
            note("Die zweite.")
        ))),
        "Erst[^1] dann[^2].\n\nWeiter.\n\n[^1]: Die *erste*.\n\n    Zweiter Absatz.\n\n[^2]: Die zweite.\n"
    );
    assert_eq!(
        markdown(&doc(&format!(r"\intbl a{}\cell b\cell\row\pard c", note("im Feld")))),
        "| a[^1] | b |\n|---|---|\n\nc\n\n[^1]: im Feld\n"
    );
    assert_eq!(
        markdown(&doc(r"a{\footnote b{\footnote c}d}e")),
        "a[^1]e\n\n[^1]: bd\n",
        "a note in a note is none"
    );
    assert_eq!(
        markdown(&doc(r"a{\footnote b")),
        "a[^1]\n\n[^1]: b\n",
        "an open note ends with the document"
    );
    assert_eq!(markdown(&doc(r"a{\footnote }b")), "ab\n", "an empty note is none");
}

#[test]
fn word_formulas_become_latex() {
    let display = r"{\mmath{\*\moMathPara{\*\moMath{\i{\mr\mscr0\msty2 x}}{\mr =}{\mf{\mfPr{\mctrlPr\f34 }}{\mnum{\mr 1}}{\mden{\mrad{\mradPr{\mdegHide on}}{\mdeg}{\me{\msSup{\me{\mr b}}{\msup{\mr 2}}}}}}}}}}{\mmathPict{\*\shppict{\pict\pngblip 89504e47}}}";
    assert_eq!(
        markdown(&doc(&format!(r"Vorher:\par {display}\par Nachher.\par"))),
        "Vorher:\n\n$$x=\\frac{1}{\\sqrt{{b}^{2}}}$$\n\nNachher.\n"
    );
    assert_eq!(
        markdown(&doc(
            r"Die Fl\'e4che ist {\mmath{\*\moMath{\mr A}{\mr =}{\mr a}{\mr \u8901\'5f}{\mr b}}{\mmathPict{\*\shppict{\pict\pngblip 89504e47}}}}{\mmathPict x} gro\'df."
        )),
        "Die Fläche ist $A=a\\cdot b$ groß.\n"
    );
    assert_eq!(
        markdown(&doc(r"a {\mmath{\*\moMathPara{\*\moMath{\mr x}}}} b")),
        "a $x$ b\n",
        "a formula of its own lines within text stays in the line"
    );
    assert_eq!(
        markdown(&doc(r"{\mmath{\*\moMath{\mr\msty0 sin}{\mr x<&>}{\mr\mnor Text}}}")),
        "$\\sin x<\\&>\\text{Text}$\n"
    );
    assert_eq!(markdown(&doc(r"a{\mmath{\*\moMath}}b")), "ab\n");
    assert_eq!(
        markdown(&doc(
            r"{\mmath{\*\moMath{\mnary{\mnaryPr{\mchr \u8721 ?{\mr z}}}{\msub{\mr i}}{\msup{\mr n}}{\me{\mr x}}}}}"
        )),
        "$\\sum_{i}^{n}{x}$\n",
        "a value keeps its text whatever else stands in its group"
    );
    assert_eq!(
        markdown(&doc(r"{\mmath{\*\moMath{\mr x\'01y\u-2 ?}{\mr\mnor a b}}}")),
        "$xy\\text{a b}$\n",
        "what XML cannot hold is left out, and spaces stay"
    );
    assert_eq!(
        markdown(&doc(r"a{\mmath{\*\moMath{\mr x}{\mf{\mnum{\mr 1}}{\mden{\mr 2}")),
        "a$x\\frac{1}{2}$\n",
        "a formula the document ends in is closed as far as it came"
    );
    assert_eq!(
        markdown(&doc(r"a{\mmath{\*\moMath{\mr x}")),
        "a$x$\n",
        "an open formula ends with the document"
    );
}

#[test]
fn word_aligns_the_paragraphs_in_cells_and_writes_its_heading_style_on_every_run() {
    assert_eq!(
        markdown(&doc(
            r"\intbl\ql a\cell\pard\intbl\qr b\cell\pard\intbl\qc c\cell\row\pard\intbl\qc 1\cell 2\cell 3\cell\row"
        )),
        "| a | b | c |\n|---|---:|:---:|\n| 1 | 2 | 3 |\n"
    );
    assert_eq!(
        markdown(&doc(r"\pard\outlinelevel2\b\i {Schluss}\par\pard\plain Text")),
        "### Schluss\n\nText\n"
    );
    assert_eq!(
        markdown(&doc(r"\pard\outlinelevel2 {\i Halb} schr\'e4g\par\pard\plain Text")),
        "### *Halb* schräg\n\nText\n"
    );
}

#[test]
fn binary_data_can_hold_anything() {
    assert_eq!(markdown(&doc(r"a{\pict\bin4 }}}}b}c")), "ac\n");
    assert_eq!(markdown(&doc(r"a\bin2 }}b")), "ab\n");
    assert_eq!(markdown(&doc(r"a\bin b")), "ab\n");
    assert_eq!(markdown(&doc(r"a\bin-5 b")), "ab\n");
    assert_eq!(
        markdown(&doc(r"\uc1\u228\bin2 xxy")),
        "äy\n",
        "also in place of a fallback"
    );
    assert_eq!(markdown(r"{\rtf1 a\bin999999999 b}"), "a\n");
}

#[test]
fn a_link_lasts_as_long_as_the_result_of_its_field() {
    assert_eq!(
        markdown(&doc(
            r#"{\field{\*\fldinst{HYPERLINK "https://a.bc" }}{\fldrslt a \b b}} c"#
        )),
        "[a **b**](https://a.bc) c\n"
    );
    assert_eq!(
        markdown(&doc(r##"{\field{\*\fldinst HYPERLINK "#a"}{\fldrslt x}}"##)),
        "[x](#a)\n"
    );
    assert_eq!(
        markdown(&doc(r#"{\field{\*\fldinst HYPERLINK \l "a"}{\fldrslt x}}"#)),
        "[x](#a)\n"
    );
    assert_eq!(
        markdown(&doc(r#"{\field{\*\fldinst HYPERLINK \\l "a"}{\fldrslt x}}"#)),
        "[x](#a)\n"
    );
    assert_eq!(
        markdown(&doc(r#"{\field{\*\fldinst HYPERLINK "javascript:x"}{\fldrslt x}}"#)),
        "x\n"
    );
    assert_eq!(markdown(&doc(r"{\field{\*\fldinst PAGE}{\fldrslt 7}}")), "7\n");
    assert_eq!(markdown(&doc(r"{\fldrslt x}")), "x\n", "a result without a field");
    assert_eq!(
        markdown(&doc(
            r#"{\field{\*\fldinst HYPERLINK "a"}}{\field{\*\fldinst PAGE}{\fldrslt x}}"#
        )),
        "x\n",
        "an instruction belongs to its own result"
    );
    assert_eq!(
        markdown(&doc(r#"{\field{\*\fldinst HYPERL\uc0\u73 NK "a"\b}{\fldrslt x}}"#)),
        "[x](a)\n",
        "the instruction is text, whatever writes it"
    );
    assert_eq!(
        markdown(&doc(r"{\field{\fldinst PAGE}{\fldrslt x\line y}}")),
        "x\\\ny\n"
    );
}

#[test]
fn a_monospace_font_gives_code() {
    assert_eq!(markdown(&doc(r"a {\f1 b\b c} d")), "a `b`**`c`** d\n");
    let one_font = r"{\rtf1\deff1{\fonttbl{\f1 Consolas;}}a}";
    assert_eq!(markdown(one_font), "`a`\n", "the default font counts");
    let without_semicolon = r"{\rtf1{\fonttbl{\f0 Arial;}{\f1 Courier New}}\f1 a\plain b\f c}";
    assert_eq!(markdown(without_semicolon), "`a`bc\n");
}

#[test]
fn lines_of_code_in_a_row_are_one_code_block() {
    assert_eq!(
        markdown(&doc(r"\f1 fn a() \{\par    b\tab c\par\par\}\par\par\f0 Text")),
        "```\nfn a() {\n   b\tc\n\n}\n```\n\nText\n"
    );
    assert_eq!(
        markdown(&doc(r"\f1 eine\par\f0 Text")),
        "`eine`\n\nText\n",
        "one alone stays in its line"
    );
    assert_eq!(markdown(&doc(r"\f1 a\line b\par c")), "```\na\nb\nc\n```\n");
    assert_eq!(
        markdown(&doc(r"\f1\b a\par b")),
        "**`a`**\n\n**`b`**\n",
        "marked code is not plain"
    );
    assert_eq!(
        markdown(&doc(r#"{\field{\*\fldinst HYPERLINK "x"}{\fldrslt \f1 a}}\par \f1 b"#)),
        "[`a`](x)\n\n`b`\n",
        "linked code neither"
    );
    assert_eq!(
        markdown(&doc(r"\pard\ls1\f1 a\par b\par")),
        "- `a`\n- `b`\n",
        "and list items keep being items"
    );
    assert_eq!(
        markdown(&doc(r"\pard\ls1 a\par\pard\f1 b\par c\par\f0\pard\ls1 d")),
        "- a\n\n```\nb\nc\n```\n\n- d\n"
    );
    assert_eq!(
        markdown(&doc(r"\f1 a{\f0  }b\par c")),
        "```\na b\nc\n```\n",
        "spaces in another font make no difference"
    );
    assert_eq!(markdown(&doc(r"\f1 a{\f0 x}\par b")), "`a`x\n\n`b`\n");
}

const LONGEST: usize = 200;

#[test]
fn an_outline_level_makes_a_heading_whatever_its_size() {
    let body = "Das ist der Text des Dokuments, viel laenger als jede Ueberschrift.";
    let rtf = doc(&format!(
        r"\pard\outlinelevel0 Eins\par\pard\outlinelevel2\fs48 Drei\par\pard\outlinelevel7 Acht\par\pard\outlinelevel9\fs48 Text\par\pard\fs24 {body}"
    ));
    assert_eq!(
        markdown(&rtf),
        format!("# Eins\n\n### Drei\n\n###### Acht\n\nText\n\n{body}\n")
    );
    let long = "x".repeat(LONGEST + 1);
    assert_eq!(
        markdown(&doc(&format!(r"\outlinelevel1 {long}\par\pard {body}"))),
        format!("## {long}\n\n{body}\n"),
        "Word says so of a long one too"
    );
}

#[test]
fn clearly_larger_short_paragraphs_are_headings() {
    let body = "Das ist der Text des Dokuments, viel laenger als jede Ueberschrift.";
    for (size, level) in [(44, "# "), (36, "## "), (28, "### "), (26, ""), (24, "")] {
        let rtf = doc(&format!(r"\fs{size} Titel\par\fs24 {body}"));
        assert_eq!(markdown(&rtf), format!("{level}Titel\n\n{body}\n"), "{size}");
    }
    assert_eq!(
        markdown(&doc(r"\fs48 Ganz gross")),
        "Ganz gross\n",
        "one size is the body's"
    );
    assert_eq!(
        markdown(&doc(&format!(r"\fs48 Tit{{\fs24 el}}\par\fs24 {body}"))),
        format!("Titel\n\n{body}\n"),
        "only the whole of a paragraph counts"
    );
    assert_eq!(
        markdown(&doc(&format!(r"\fs48\b Titel\par\plain {body}"))),
        format!("# Titel\n\n{body}\n"),
        "bold is what a heading is anyway"
    );
    assert_eq!(
        markdown(&doc(&format!(r"\fs48{{\fs12  }}Titel\par\plain {body}"))),
        format!("# Titel\n\n{body}\n"),
        "a space in another size makes no difference"
    );
    let long = "x".repeat(LONGEST + 1);
    let short = "x".repeat(LONGEST);
    let text = "y".repeat(3 * LONGEST);
    assert_eq!(
        markdown(&doc(&format!(r"\fs48 {long}\par\fs24 {text}"))),
        format!("{long}\n\n{text}\n")
    );
    assert_eq!(
        markdown(&doc(&format!(r"\fs48 {short}\par\fs24 {text}"))),
        format!("# {short}\n\n{text}\n")
    );
    assert_eq!(
        markdown(&doc(&format!(r"\fs\fs-5 a\par\plain {body}"))),
        format!("a\n\n{body}\n")
    );
    assert_eq!(
        markdown(&doc(r"\fs48 ab\par\fs24 cd")),
        "# ab\n\ncd\n",
        "the same number of letters: the smaller size is the body's"
    );
    assert_eq!(
        markdown(&doc(&format!(r"\pard\ls1\fs48 a\par\plain\pard {body}"))),
        format!("- a\n\n{body}\n")
    );
}

#[test]
fn tables_end_where_text_comes_or_the_document_does() {
    assert_eq!(
        markdown(&doc(r"\trowd\intbl a\cell b\cell\row")),
        "| a | b |\n|---|---|\n"
    );
    assert_eq!(
        markdown(&doc(r"\intbl a\cell b\row")),
        "| a | b |\n|---|---|\n",
        "`\\row` ends an open cell"
    );
    assert_eq!(
        markdown(&doc(r"\trowd\intbl a\cell")),
        "| a |\n|---|\n",
        "cut off after a cell"
    );
    assert_eq!(markdown(&doc(r"\trowd\intbl a")), "| a |\n|---|\n", "cut off in a cell");
    assert_eq!(markdown(&doc(r"\row\cell\row")), "", "a row of nothing");
    assert_eq!(
        markdown(&doc(r"\trowd\intbl a\cell\row\pard b\par")),
        "| a |\n|---|\n\nb\n",
        "a paragraph outside ends it"
    );
    assert_eq!(
        markdown(&doc(r"\trowd\intbl a\cell\row\pard\par\trowd\intbl b\cell\row")),
        "| a |\n|---|\n\n| b |\n|---|\n",
        "also an empty one"
    );
    assert_eq!(
        markdown(&doc(r"\intbl a\nestcell b\nestrow c\cell d\cell\row")),
        "| a<br>b<br>c | d |\n|---|---|\n",
        "nested tables flatten"
    );
    assert_eq!(
        markdown(&doc(r"\intbl{\listtext x}a\par b\cell\row")),
        "| a<br>b |\n|---|\n"
    );
    assert_eq!(
        markdown(&doc(r"\f1 a\par b\par\f0\intbl c\cell\row")),
        "```\na\nb\n```\n\n| c |\n|---|\n"
    );
}

#[test]
fn what_is_no_rtf_gives_nothing() {
    for data in [&b""[..], b"   ", b"text", b"{\\rt", b"{rtf1 a}", b"\xFF\xFE"] {
        assert_eq!(from_rtf(data), Rich::default());
    }
    assert_eq!(markdown(" \n {\\rtf1 a}"), "a\n", "spaces in front are fine");
}

#[test]
fn the_document_ends_with_its_group() {
    assert_eq!(markdown(r"{\rtf1 a}b"), "a\n");
    assert_eq!(markdown(r"{\rtf1 a}}}b"), "a\n");
    assert_eq!(
        markdown(r"{\rtf1 a{\b b"),
        "a**b**\n",
        "a cut-off document still has its text"
    );
    assert_eq!(markdown(r"{\rtf1 a\"), "a\n");
    assert_eq!(markdown(r"{\rtf1 a\'"), "a\n");
    assert_eq!(markdown(r"{\rtf1 a\'e"), "a\n");
    assert_eq!(markdown(r"{\rtf1 a\-"), "a\n");
}

#[test]
fn numbers_of_any_length_do_no_harm() {
    assert_eq!(
        markdown(&doc(r"\fs99999999999999999999 a\b99999999999999999 b\ul-99999999999 c")),
        "a**b<ins>c</ins>**\n"
    );
    assert_eq!(markdown(&doc(r"\-5 a")), "5 a\n", "a dash alone is a symbol");
    assert_eq!(markdown(&doc(r"\b-x")), "**-x**\n", "a dash without digits is text");
}

#[test]
fn a_picture_from_cocoa_leaves_nothing_behind() {
    let picture = "{\\rtf1\\ansi a{{\\NeXTGraphic bild.png \\width1200 \\height800 \\noorient}\u{ac}}b}";
    assert_eq!(markdown(picture), "ab\n");
    assert_eq!(
        markdown(r"{\rtf1 a{\NeXTGraphic x}b}"),
        "ab\n",
        "alone in its group, it takes only that"
    );
}

#[test]
fn groups_nested_too_deep_keep_their_ends_apart() {
    let depth = 100_000;
    let rtf = format!(r"{{\rtf1 a{}b{}c}}", "{".repeat(depth), "}".repeat(depth));
    assert_eq!(markdown(&rtf), "abc\n");
    let marked = format!(
        r"{{\rtf1 {}\b x{}y}}",
        "{".repeat(MOST_DEPTH + 5),
        "}".repeat(MOST_DEPTH + 5)
    );
    assert_eq!(markdown(&marked), "y\n".replace('y', "**x**y"));
    let deep_in_a_skipped_group = format!(r"{{\rtf1 a{{\pict {}x{}}}b}}", "{".repeat(depth), "}".repeat(depth));
    assert_eq!(markdown(&deep_in_a_skipped_group), "ab\n");
}

#[test]
fn a_font_table_takes_only_names() {
    let rtf = r"{\rtf1{\fonttbl{\f0\b\froman Arial;}{\f1\uc0\u77 enlo;}{\f2 Geheim\par;}}a\f1 b}";
    assert_eq!(markdown(rtf), "a`b`\n");
}

const PIECES: &[&str] = &[
    "{",
    "}",
    "\\",
    "\\b ",
    "\\b0 ",
    "\\i ",
    "\\ul ",
    "\\ulnone ",
    "\\strike ",
    "\\super ",
    "\\sub ",
    "\\nosupersub ",
    "\\plain ",
    "\\par ",
    "\\pard ",
    "\\line ",
    "\\tab ",
    "\\cell ",
    "\\row ",
    "\\trowd ",
    "\\intbl ",
    "\\nestcell ",
    "\\ls1 ",
    "\\ilvl1 ",
    "{\\listtext 1.}",
    "{\\pntext \\'b7}",
    "\\*",
    "\\fonttbl ",
    "{\\fonttbl\\f1 Menlo;}",
    "\\f1 ",
    "\\f0 ",
    "\\fs48 ",
    "\\fs24 ",
    "\\u228 ",
    "\\uc0 ",
    "\\uc2 ",
    "\\u55357 ",
    "\\u56832 ",
    "\\'e4",
    "\\'",
    "\\bin3 ",
    "\\bin99999 ",
    "{\\pict ",
    "{\\*\\shppict{\\pict\\pngblip 89504e470d0a1a0a",
    "{\\*\\picprop{\\sp{\\sn wzDescription}{\\sv ",
    "\\pngblip ",
    "\\jpegblip ffd8ff",
    "{\\footnote ",
    "\\chftn ",
    "{\\mmath{\\*\\moMathPara{\\*\\moMath ",
    "{\\mr ",
    "{\\mf{\\mnum ",
    "{\\mden ",
    "{\\mdegHide on}",
    "\\msty0 ",
    "\\qr ",
    "{\\field{\\*\\fldinst{HYPERLINK \"https://a.bc\"}}{\\fldrslt ",
    "{\\*\\fldinst HYPERLINK ",
    "{\\fldrslt ",
    "\"",
    "a",
    "Wort",
    " ",
    "*",
    "_",
    "[",
    "]",
    "(",
    ")",
    "#",
    "- ",
    "1. ",
    ";",
    "\n",
    "\r\n",
    "\u{e4}",
    "\u{1F600}",
];

#[test]
fn any_rtf_gives_markdown_that_can_be_read() {
    let mut rng = Rng::new(0x47F0_0001);
    for _ in 0..rounds(20_000) {
        let mut rtf = String::from("{\\rtf1 ");
        for _ in 0..rng.below(40) {
            rtf.push_str(rng.pick(PIECES));
        }
        let written = markdown(&rtf);
        let _ = parse(&written);
    }
}

#[test]
fn any_bytes_are_taken() {
    let mut rng = Rng::new(0x47F0_0002);
    for _ in 0..rounds(20_000) {
        let mut data = b"{\\rtf1 ".to_vec();
        for _ in 0..rng.below(64) {
            let byte = u8::try_from(rng.below(256)).unwrap_or(0);
            data.push(if rng.below(3) == 0 { b'\\' } else { byte });
        }
        let _ = from_rtf(&data).into_markdown();
    }
}

#[test]
fn long_inputs_take_no_quadratic_time() {
    let n = 200_000;
    let text = "a\\b b\\b0 c\\par ".repeat(n);
    assert_eq!(markdown(&doc(&text)).matches("**b**").count(), n);
    let cells = "\\intbl x\\cell ".repeat(n);
    assert!(markdown(&doc(&cells)).len() > n);
    let fields = "{\\field{\\*\\fldinst HYPERLINK \"a\"}{\\fldrslt x}}".repeat(n / 4);
    assert!(markdown(&doc(&fields)).len() > n / 4);
    let unclosed_instructions = "{\\*\\fldinst x".repeat(n);
    assert_eq!(markdown(&doc(&unclosed_instructions)), "");
    let notes = "a{\\footnote b}".repeat(n / 4);
    assert_eq!(markdown(&doc(&notes)).matches("[^").count(), n / 4 * 2);
    let formulas = "{\\mmath{\\*\\moMath{\\mr x}}}".repeat(n / 4);
    assert_eq!(markdown(&doc(&formulas)).matches('x').count(), n / 4);
    let deep_formula = format!(
        "{{\\mmath{{\\*\\moMath {}x{}}}}}",
        "{\\mf{\\mnum ".repeat(n),
        "}}".repeat(n)
    );
    assert!(markdown(&doc(&deep_formula)).contains('x'));
}
