//! Partial parses and layouts after an edit match a full parse of the new text.

use splat_markdown::highlight::highlight;
use splat_markdown::incremental::{parse_and_color, reparse_and_color};
use splat_markdown::reader::parse;
use splat_markdown::text::offsets;
use splat_preview::layout::{NoImages, caret_shapes_layout, layout, relayout, reusable, reusable_behind_caret};
use splat_testkit::{Reporting, Rng, rounds};

use super::common::{PIECES, document, limited, ordinary_document};

/// A caret that moves changes a layout only where `caret_shapes_layout` says, and only the block it left has to go.
#[test]
fn a_caret_that_moves_leaves_the_layout_where_it_does_not_shape_it() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_ca4e_0001);
    let (mut kept, mut laid) = (0, 0);
    for round in 0..rounds(1500) {
        let text = if round % 2 == 0 {
            document(&mut rng)
        } else {
            ordinary_document(&mut rng) + "\n\n\n\nEnde  \n\nmehr\n"
        };
        let doc = parse(&text);
        let carets = [
            (rng.below(4) > 0).then(|| rng.boundary(&text)),
            (rng.below(4) > 0).then(|| rng.boundary(&text)),
        ];
        let old = layout(&doc, &NoImages, carets[0]);
        let fresh = layout(&doc, &NoImages, carets[1]);
        let shapes = carets.iter().flatten().any(|&c| caret_shapes_layout(&text, c));
        let what = format!("{carets:?} of {text:?}");
        if shapes {
            laid += 1;
        } else {
            kept += 1;
            assert_eq!(old.blocks, fresh.blocks, "{what}");
        }
        let relaid = relayout(&doc, &NoImages, carets[1], old, &reusable_behind_caret(&doc, carets[0]));
        assert_eq!(relaid.blocks, fresh.blocks, "{what}");
    }
    assert!(
        kept > rounds(1500) / 10 && laid > rounds(1500) / 10,
        "{kept} kept, {laid} laid out"
    );
}

/// Parsing only around an edit must give exactly the tree a full parse gives, offsets included.
#[test]
fn reparsing_around_an_edit_matches_a_full_parse() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_1ac2_0005);
    let mut spliced = 0;
    for round in 0..rounds(3000) {
        // Several paragraphs, so most edits leave blocks on both sides untouched.
        // Footnotes mostly take a full parse here, so most documents leave them out; the others hold a reference.
        let with_footnotes = round % 4 == 0;
        let mut old: String = (0..rng.below(6) + 2)
            .map(|_| document(&mut rng))
            .map(|d| {
                if with_footnotes {
                    d
                } else {
                    d.replace("[^1]", "x").replace("^[", "^").replace("]:", "]")
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        if rng.below(2) == 0 {
            old.push_str("\n\n[ref]: https://example.com\n\nSiehe [ref] und [Text][ref].\n");
        }
        let _input = Reporting(&old);
        let doc = parse(&old);
        let range = rng.range(&old);
        let chosen = PIECES[rng.below(PIECES.len())];
        let plain = chosen
            .replace("[^1]", "y")
            .replace("[^", "[")
            .replace("^[", "^")
            .replace("]:", "]");
        let piece = if with_footnotes { chosen } else { plain.as_str() };
        let mut new = old.clone();
        new.replace_range(offsets(&range), if rng.below(4) == 0 { "" } else { piece });
        let old_spans = highlight(&doc);
        let old_caret = (rng.below(2) == 0).then(|| rng.boundary(&old));
        let old_layout = layout(&doc, &NoImages, old_caret);
        let parsed = reparse_and_color(doc, &old_spans, &new);
        let what = format!("edit {range:?} -> {piece:?} of {old:?} -> {new:?}, {:?}", parsed.splice);
        let full = parse_and_color(&new);
        assert_eq!(parsed.doc.blocks(), full.doc.blocks(), "{what}");
        assert_eq!(parsed.spans, full.spans, "colors: {what}");
        let Some(splice) = &parsed.splice else {
            continue;
        };
        spliced += 1;
        let new_caret = (rng.below(2) == 0).then(|| rng.boundary(&new));
        let reuse = reusable(splice, &parsed.doc, old_caret);
        let relaid = relayout(&parsed.doc, &NoImages, new_caret, old_layout, &reuse);
        let fresh = layout(&full.doc, &NoImages, new_caret);
        assert_eq!(relaid.blocks, fresh.blocks, "layout: {what}");
    }
    assert!(spliced > rounds(3000) / 5, "only {spliced} edits were parsed in part");
}

const TYPED_INTO: [&str; 45] = [
    "Wort und mehr Wörter",
    "**fett** *schief* `code` ~~weg~~",
    "# Titel\n",
    "Titel\n===",
    "- eins\n- zwei\n  - drei",
    "1. a\n2. b",
    "> Zitat\n> weiter",
    "> [!NOTE]\n> Hinweis",
    "```rust\nlet a = 1;\n```",
    "~~~\ncode\n~~~",
    "    indented code",
    "| a | b |\n|---|---|\n| 1 | 2 |",
    "[a]: https://x.de",
    "[a] und [Text][a]",
    "[^1]: Note",
    "Text[^1] mehr",
    "^[inline]",
    "<!-- comment -->",
    "<div>\nhtml\n</div>",
    "<pre>\nx\n\ny\n</pre>",
    "<script>\nlet x;\n\nlet y;\n</script>",
    "<style>\np{}\n\nq{}\n</style>",
    "<textarea>\na\n\nb\n</textarea>",
    "<?php x\n\ny ?>",
    "<![CDATA[\na\n\nb\n]]>",
    "<!DOCTYPE html>",
    "$$\nx^2\n$$",
    "\\(a\\) und \\[b\\]",
    "$`a`$",
    "%%\nversteckt\n%%",
    "```mermaid\ngraph TD; A-->B; %% c\n```",
    "Term\n: Definition",
    "---",
    "***",
    "- [ ] todo\n- [x] done",
    "![Bild](a.png)",
    "![[Einbettung]]",
    "==mark== ^sup^ ~sub~",
    "{: .class}",
    "---\ntitle: x\n---",
    "a  \nb",
    "&amp; &#123; \\* \\_",
    "日本語の文章 🧑‍🚀 é",
    "   ",
    "\t- tab",
];

const KEYSTROKES: [&str; 56] = [
    "a",
    "e",
    " ",
    "\n",
    "\n\n",
    "*",
    "**",
    "`",
    "```",
    "[",
    "]",
    "(",
    ")",
    "]:",
    "^[",
    "[^1]",
    "<",
    ">",
    "<!--",
    "-->",
    "%%",
    "$",
    "$$",
    "\\(",
    "\\)",
    "\\[",
    "\\]",
    "#",
    "# ",
    "- ",
    "> ",
    "1. ",
    "|",
    "---",
    "~~~",
    "<pre>",
    "</pre>",
    "<script>",
    "</script>",
    "<div>",
    "</div>",
    "&",
    ";",
    "\\",
    "\t",
    "    ",
    "\r\n",
    "ä",
    "🧑",
    "é",
    "=",
    ":",
    "!",
    "[!NOTE]",
    "^",
    "~",
];

fn on_a_letter(text: &str, mut at: usize) -> usize {
    at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    // A view treats a line end of two characters as one.
    if at > 0 && at < text.len() && text.as_bytes()[at - 1] == b'\r' && text.as_bytes()[at] == b'\n' {
        at += 1;
    }
    at
}

/// Keystroke after keystroke, as typing goes: blocks, colors and layout stay those of a full parse, and what a partial parse
/// carries on stays what all of the text says. A single step from a fresh parse would not see what wears off over many.
#[test]
fn typing_on_and_on_reads_the_same_as_parsing_all_of_it() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_11c4_0002);
    let (mut steps, mut partial) = (0, 0);
    for round in 0..rounds(1500) {
        let picked: Vec<&str> = (0..rng.below(14) + 1).map(|_| *rng.pick(&TYPED_INTO)).collect();
        let mut text = picked.join("\n\n");
        if round % 5 == 0 {
            text = text.replace('\n', "\r\n");
        }
        let mut parsed = parse_and_color(&text);
        let mut caret = rng.below(text.len() + 1);
        for _ in 0..rng.below(40) + 2 {
            caret = on_a_letter(&text, caret);
            let mut new = text.clone();
            let key = *rng.pick(&KEYSTROKES);
            match rng.below(10) {
                0..=5 => {
                    new.insert_str(caret, key);
                    caret += key.len();
                }
                6 | 7 => {
                    let start = text[..caret].char_indices().next_back().map_or(0, |(at, _)| at);
                    new.replace_range(start..caret, "");
                    caret = start;
                }
                8 => {
                    caret = rng.below(text.len() + 1);
                    continue;
                }
                _ => {
                    let end = on_a_letter(&text, caret + rng.below(30));
                    new.replace_range(caret..end, key);
                    caret += key.len();
                }
            }
            steps += 1;
            let what = format!("{text:?} -> {new:?}");
            let spans = std::mem::take(&mut parsed.spans);
            let next = reparse_and_color(parsed.doc, &spans, &new);
            let full = parse_and_color(&new);
            assert_eq!(next.doc.blocks(), full.doc.blocks(), "{what}");
            assert_eq!(next.spans, full.spans, "colors: {what}");
            assert_eq!(next.doc.facts(), full.doc.facts(), "facts: {what}");
            assert_eq!(
                layout(&next.doc, &NoImages, None).blocks,
                layout(&full.doc, &NoImages, None).blocks,
                "layout: {what}"
            );
            partial += usize::from(next.splice.is_some());
            parsed = next;
            text = new;
        }
    }
    assert!(
        partial > steps / 4,
        "only {partial} of {steps} keystrokes were parsed in part"
    );
}

/// The same for ordinary documents with links defined anywhere: this is where typing has to stay partial.
#[test]
fn reparsing_around_links_with_definitions_matches_a_full_parse() {
    const BLOCKS: [&str; 16] = [
        "Wort und mehr",
        "Ein [a] und [Text][b] und [c]",
        "**fett** [a] *schief*",
        "[Direkt](https://x.de) [a]",
        "# Titel [b]",
        "- eins [a]\n- zwei\n  - drei [c]",
        "> Zitat [a]\n> weiter",
        "```\nx[0]: y\n[a]\n```",
        "```mermaid\n%%{init}%%\ngraph\n```",
        "| a | [a] |\n|---|---|\n| 1 | 2 |",
        "[a]: http://a.de",
        "[b]: <http://b.de> \"Titel\"",
        "[c]:\n  http://c.de\n  'Titel'",
        "> [d]: http://d.de",
        "- [e]: http://e.de",
        "Text a[0]: b",
    ];
    const TYPED: [&str; 12] = [
        "x",
        " ",
        "[",
        "]",
        "]:",
        "[a]",
        "[d]",
        "\n",
        "\n\n",
        "[neu]: http://n.de\n\n",
        "*",
        "`",
    ];
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_11c4_0001);
    let (mut spliced, mut whole) = (0, 0);
    for _ in 0..rounds(3000) {
        let mut picked: Vec<&str> = (0..rng.below(14) + 3).map(|_| *rng.pick(&BLOCKS)).collect();
        // A label defined twice takes a full parse.
        let mut seen = std::collections::HashSet::new();
        picked.retain(|b| !(b.contains("]:") && !b.contains("x[0]") && !b.contains("a[0]")) || seen.insert(*b));
        let old = picked.join("\n\n");
        let _input = Reporting(&old);
        let doc = parse(&old);
        let at = rng.boundary(&old);
        let mut new = old.clone();
        new.insert_str(at.get(), rng.pick(&TYPED));
        let spans = highlight(&doc);
        let parsed = reparse_and_color(doc, &spans, &new);
        let full = parse_and_color(&new);
        let what = format!("{at:?} of {old:?} -> {new:?}, {:?}", parsed.splice);
        assert_eq!(parsed.doc.blocks(), full.doc.blocks(), "{what}");
        assert_eq!(parsed.spans, full.spans, "colors: {what}");
        // A definition inside a quote or list takes a full parse.
        if parsed.splice.is_some() {
            spliced += 1;
        } else if !old.contains("> [d]") && !old.contains("- [e]") {
            whole += 1;
        }
    }
    assert!(
        spliced > whole,
        "{spliced} edits were parsed in part, {whole} as a whole"
    );
}

/// A document whose references all have definitions, written before, between or behind the rest of the text.
fn document_with_footnotes(rng: &mut Rng) -> String {
    const PLAIN: &[&str] = &[
        "Wort und mehr",
        "Fuß **fett** und *schief*",
        "# Titel",
        "- eins\n- zwei",
        "> Zitat",
        "```\ncode [^a-z]\n```",
        "| a | b |\n|---|---|\n| 1 | 2 |",
        "[Link](https://x.de)",
        "$$\nx\n$$",
    ];
    let notes = rng.below(5) + 1;
    let place = rng.below(3);
    let (mut text, mut behind) = (Vec::new(), Vec::new());
    for i in 0..notes {
        for _ in 0..rng.below(4) + 1 {
            text.push(PLAIN[rng.below(PLAIN.len())].to_string());
        }
        text.push(match rng.below(3) {
            0 => format!("Text[^n{i}] weiter"),
            1 => format!("Mit [^n{i}] und **fett[^n{i}]**"),
            _ => format!("- Punkt[^n{i}]"),
        });
        let definition = match rng.below(4) {
            0 => format!("[^n{i}]: Note\n    und mehr"),
            1 => format!("[^n{i}]: Note\n\n    Absatz"),
            2 if i + 1 < notes => format!("[^n{i}]: Note mit[^n{}]", i + 1),
            _ => format!("[^n{i}]: Note"),
        };
        if place == 0 || (place == 2 && rng.below(2) == 0) {
            behind.push(definition);
        } else {
            text.push(definition);
        }
    }
    for _ in 0..rng.below(3) {
        text.push(PLAIN[rng.below(PLAIN.len())].to_string());
    }
    text.extend(behind);
    text.join("\n\n") + if rng.below(4) == 0 { "" } else { "\n" }
}

/// Footnotes keep their numbers, colors and places in the preview when only what lies around them is parsed again.
#[test]
fn reparsing_around_footnotes_matches_a_full_parse() {
    let _limit = limited();
    const PIECES: &[&str] = &[
        "x",
        " ",
        "ß",
        "😀",
        "*",
        "`",
        "\n",
        "\n\n",
        "[",
        "]",
        "^",
        ":",
        "    ",
        "> ",
        "[^n0]",
        "[^n1]",
        "[^neu]",
        "[^n1]:",
        "\n[^neu]: Neu\n",
        "^[i]",
        "```",
    ];
    let mut rng = Rng::new(0x5eed_1ac2_0007);
    let mut spliced = 0;
    for _ in 0..rounds(3000) {
        let old = document_with_footnotes(&mut rng);
        let typing = rng.below(2) == 0;
        let range = if typing {
            let at = rng.boundary(&old);
            at..at
        } else {
            rng.range(&old)
        };
        let piece = if typing { "x" } else { PIECES[rng.below(PIECES.len())] };
        let mut new = old.clone();
        new.replace_range(offsets(&range), if !typing && rng.below(5) == 0 { "" } else { piece });
        let doc = parse(&old);
        let old_spans = highlight(&doc);
        let old_caret = (rng.below(2) == 0).then(|| rng.boundary(&old));
        let old_layout = layout(&doc, &NoImages, old_caret);
        let parsed = reparse_and_color(doc, &old_spans, &new);
        let what = format!("edit {range:?} -> {piece:?} of {old:?} -> {new:?}, {:?}", parsed.splice);
        let full = parse_and_color(&new);
        assert_eq!(parsed.doc.blocks(), full.doc.blocks(), "{what}");
        assert_eq!(parsed.spans, full.spans, "colors: {what}");
        let Some(splice) = &parsed.splice else {
            continue;
        };
        spliced += 1;
        let new_caret = (rng.below(2) == 0).then(|| rng.boundary(&new));
        let reuse = reusable(splice, &parsed.doc, old_caret);
        let relaid = relayout(&parsed.doc, &NoImages, new_caret, old_layout, &reuse);
        let fresh = layout(&full.doc, &NoImages, new_caret);
        assert_eq!(relaid.blocks, fresh.blocks, "layout: {what}");
    }
    assert!(spliced > rounds(3000) / 5, "only {spliced} edits were parsed in part");
}
