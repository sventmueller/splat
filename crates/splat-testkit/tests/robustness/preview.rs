//! Edits made in the preview: what is typed, deleted, split or joined shows as it should.

use splat_markdown::reader::parse;
use splat_markdown::text::{Byte, Direction, TextAt};
use splat_preview::bridge::Bridge;
use splat_preview::layout::{NoImages, layout};
use splat_preview::map::{Index, Shown, preview_offset};
use splat_testkit::{Rng, rounds, with_smallest_failure};

use super::common::{BLOCKS, check, document, limited, ordinary_document, plain_document};

#[test]
fn preview_edits_never_break_the_text() {
    let _limit = limited();
    let mut rounds_rng = Rng::new(0x0dd_ba11_c0ff_ee42 ^ 0x9e37_79b9);
    for _ in 0..rounds(300) {
        let seed = rounds_rng.below(usize::MAX) as u64 | 1;
        with_smallest_failure(&document(&mut rounds_rng), |text| preview_edits_on(text, seed));
    }
}

fn preview_edits_on(text: &str, seed: u64) {
    let text = text.to_string();
    let mut rng = Rng::new(seed);
    {
        let doc = parse(&text);
        let caret = Some(rng.boundary(&text));
        let l = layout(&doc, &NoImages, caret);
        let index = Index::new(&l);
        let len: usize = l.blocks.iter().map(|b| b.len_u16).sum();
        let bridge = Bridge::new(&doc, &l, &index).unwrap();
        let p = rng.below(len + 1);
        let q = (p + rng.below(6)).min(len);
        let typed = ["a", "*", " ", "ä", "\n", "#"][rng.below(6)];
        if let Some(e) = bridge.replace(Shown::new(p)..Shown::new(q), typed) {
            check(&text, &e, "replace");
        }
        if let Some(e) = bridge.newline(Shown::new(p)) {
            check(&text, &e, "newline");
        }
        if let Some(e) = bridge.delete_at_block_start(Shown::new(p)) {
            check(&text, &e, "backspace");
        }
        if let Some(e) = bridge.tab(
            Shown::new(p),
            if rng.below(2) == 0 {
                Direction::Backward
            } else {
                Direction::Forward
            },
        ) {
            check(&text, &e, "tab");
        }
        let s = bridge.source_selection(Shown::new(p)..Shown::new(q));
        assert!(
            s.end <= Byte::new(text.len())
                && text.is_char_boundary(s.start.get())
                && text.is_char_boundary(s.end.get())
        );
        let back = preview_offset(&l, &index, rng.boundary(&text));
        assert!(back <= Shown::new(len), "{back:?} > {len}");
    }
}

/// Text the preview cannot place in the source can only be edited as a whole, so running text must always find its place.
#[test]
fn running_text_finds_its_place_in_the_source() {
    let _limit = limited();
    use splat_preview::layout::{Atom, ParaKind, RunMap};
    let mut rng = Rng::new(0x5eed_a119_000b);
    for _ in 0..rounds(2000) {
        let text = ordinary_document(&mut rng);
        let l = layout(&parse(&text), &NoImages, None);
        for p in l.blocks.iter().flat_map(|b| &b.paras) {
            for r in &p.runs {
                let RunMap::Atom { atom: Atom::Text, src } = &r.map else {
                    continue;
                };
                // Atoms by design: a line end and a callout title.
                let designed = r.text == " " || matches!(p.style.kind, ParaKind::AlertTitle);
                assert!(
                    designed,
                    "{:?} has no place in {:?} of {text:?}",
                    r.text,
                    text.at(src.clone())
                );
            }
        }
    }
}

/// Typing inside any ordinary block must show what was typed there, whatever the source around it looks like.
#[test]
fn typing_in_ordinary_documents_shows_what_was_typed() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_7e57_000c);
    let mut checked = 0;
    for _ in 0..rounds(600) {
        let text = ordinary_document(&mut rng);
        let doc = parse(&text);
        let l = layout(&doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge::new(&doc, &l, &index).unwrap();
        let shown = preview(&text);
        // Inside a word: at the edge of a paragraph, formatting or marker the character may belong to either side.
        let mut p = 1 + rng.below(shown.len() - 2);
        while (0xDC00..0xE000).contains(&shown[p]) {
            p -= 1;
        }
        let alphabetic = |u: u16| char::from_u32(u32::from(u)).is_some_and(|c| c.is_alphabetic());
        if p == 0 || !alphabetic(shown[p - 1]) || !alphabetic(shown[p]) {
            continue;
        }
        let edit = bridge
            .replace(Shown::new(p)..Shown::new(p), "q")
            .expect("typing in a word");
        let new_text = check(&text, &edit, "typing");
        let mut expected = shown.clone();
        expected.insert(p, u16::from(b'q'));
        assert_eq!(
            String::from_utf16_lossy(&preview(&new_text)),
            String::from_utf16_lossy(&expected),
            "typed at {p} of {text:?} -> {new_text:?}"
        );
        checked += 1;
    }
    assert!(checked > rounds(600) / 4, "only {checked} checks ran");
}

/// Any mark typed into running text shows as just that mark, or is refused; refusals stay the exception.
#[test]
fn typing_any_mark_shows_it_or_leaves_the_text_alone() {
    let _limit = limited();
    const MARKS: [&str; 26] = [
        "*", "_", "[", "]", "(", ")", "!", "#", ":", "|", "~", "^", "$", "`", "&", "<", ">", "=", "-", "+", ".", ";",
        "%", "\\", "\"", "1",
    ];
    let mut rng = Rng::new(0x5eed_6d61_726b);
    let (mut checked, mut refused) = (0, 0);
    for _ in 0..rounds(600) {
        let text = ordinary_document(&mut rng);
        let doc = parse(&text);
        let l = layout(&doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge::new(&doc, &l, &index).unwrap();
        let shown = preview(&text);
        let mut p = 1 + rng.below(shown.len() - 2);
        while (0xDC00..0xE000).contains(&shown[p]) {
            p -= 1;
        }
        // In text only, not next to a marker or between paragraphs.
        let visible = |u: u16| char::from_u32(u32::from(u)).is_some_and(|c| c.is_alphanumeric() || c == ' ');
        if p == 0 || !visible(shown[p - 1]) || !visible(shown[p]) {
            continue;
        }
        let mark = *rng.pick(&MARKS);
        checked += 1;
        let Some(edit) = bridge.replace(Shown::new(p)..Shown::new(p), mark) else {
            refused += 1;
            continue;
        };
        let new_text = check(&text, &edit, "typing");
        let mut expected = shown.clone();
        expected.insert(p, mark.encode_utf16().next().unwrap());
        assert_eq!(
            String::from_utf16_lossy(&preview(&new_text)),
            String::from_utf16_lossy(&expected),
            "typed {mark:?} at {p} of {text:?} -> {new_text:?}"
        );
    }
    assert!(checked > rounds(600) / 4, "only {checked} checks ran");
    assert!(refused * 100 <= checked, "{refused} of {checked} refused");
}

/// Pieces of a paragraph whose source differs from what the preview shows.
const MARKED: [&str; 11] = [
    " **fett**",
    " *kursiv*",
    " `code`",
    " [Link](#a)",
    " ~~weg~~",
    " <mark>hell</mark>",
    " Caf&eacute;",
    " &rarr;",
    " &#233;",
    " \\*stern\\*",
    " a &amp; b",
];

/// The text of the preview, with a line break after every paragraph.
fn preview(text: &str) -> Vec<u16> {
    let l = layout(&parse(text), &NoImages, None);
    l.blocks
        .iter()
        .flat_map(|b| &b.paras)
        .flat_map(|p| p.runs.iter().map(|r| r.text.as_str()).chain(["\n"]))
        .flat_map(|s| s.encode_utf16().collect::<Vec<_>>())
        .collect()
}

/// Typing a character in the preview must show exactly that character there once the source is laid out again.
#[test]
fn typing_in_the_preview_shows_what_was_typed() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_7e57_0004);
    let mut checked = 0;
    for round in 0..rounds(400) {
        let mut text = plain_document(&mut rng);
        if round % 2 == 1 {
            let at = text.find(' ').unwrap_or(text.len());
            text.insert_str(at, MARKED[rng.below(MARKED.len())]);
        }
        let doc = parse(&text);
        let l = layout(&doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge::new(&doc, &l, &index).unwrap();
        let shown = preview(&text);
        let mut p = rng.below(shown.len());
        // Only inside a paragraph: typing behind its end extends the next block.
        while p > 0 && (0xDC00..0xE000).contains(&shown[p]) {
            p -= 1;
        }
        if shown[p] == u16::from(b'\n') {
            continue;
        }
        let typed = ["a", "ä", "*", "#", "_", "|", "1.", "-", ">", "&amp;", "`", "\\"][rng.below(12)];
        let edit = bridge
            .replace(Shown::new(p)..Shown::new(p), typed)
            .expect("typing in a paragraph");
        let new_text = check(&text, &edit, "typing");
        let mut expected = shown.clone();
        expected.splice(p..p, typed.encode_utf16());
        assert_eq!(
            String::from_utf16_lossy(&preview(&new_text)),
            String::from_utf16_lossy(&expected),
            "typed {typed:?} at {p} of {text:?} -> {new_text:?}"
        );
        checked += 1;
    }
    assert!(checked > rounds(400) / 2, "only {checked} checks ran");
}

/// The end of a paragraph is where most typing happens, and what follows it in the source must stay untouched.
#[test]
fn typing_at_the_end_of_a_paragraph_stays_in_it() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_e4d0_000a);
    let mut checked = 0;
    let mut refused = 0;
    for _ in 0..rounds(300) {
        let text = ordinary_document(&mut rng);
        let doc = parse(&text);
        let l = layout(&doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge::new(&doc, &l, &index).unwrap();
        let shown = preview(&text);
        for pp in &index.paras {
            let p = pp.start.get() + pp.len;
            let Some(edit) = bridge.replace(Shown::new(p)..Shown::new(p), "a") else {
                refused += 1;
                continue;
            };
            let new_text = check(&text, &edit, "typing at a paragraph's end");
            let mut expected = shown.clone();
            expected.insert(p, u16::from(b'a'));
            assert_eq!(
                String::from_utf16_lossy(&preview(&new_text)),
                String::from_utf16_lossy(&expected),
                "at {p} of {text:?} -> {new_text:?}"
            );
            checked += 1;
        }
    }
    assert!(checked > rounds(300), "only {checked} checks ran");
    assert!(refused * 50 <= checked, "{refused} edits refused, {checked} made");
}

/// What is selected in a paragraph of the preview and deleted is gone once the source is laid out again, and nothing
/// else is. Spaces at the ends of a line and paragraphs left empty do not show in Markdown.
#[test]
fn deleting_in_the_preview_removes_what_was_selected() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_de1e_0009);
    let mut checked = 0;
    let lines = |shown: &[u16]| -> Vec<String> {
        String::from_utf16_lossy(shown)
            .split('\n')
            .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|l| !l.is_empty())
            .collect()
    };
    for round in 0..rounds(400) {
        let mut text = plain_document(&mut rng);
        if round % 2 == 1 {
            let at = text.find(' ').unwrap_or(text.len());
            text.insert_str(at, MARKED[rng.below(MARKED.len())]);
        }
        if round % 3 == 0 {
            text.push_str("\n\n");
            text.push_str(rng.pick(&BLOCKS));
        }
        let doc = parse(&text);
        let l = layout(&doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge::new(&doc, &l, &index).unwrap();
        let shown = preview(&text);
        let first = rng.below(shown.len());
        let mut last = first + rng.below(8);
        last = last.min(shown.len());
        let boundary = |p: usize| p >= shown.len() || !(0xDC00..0xE000).contains(&shown[p]);
        if !boundary(first) || !boundary(last) || shown[first..last].contains(&u16::from(b'\n')) || first == last {
            continue;
        }
        let edit = bridge
            .replace(Shown::new(first)..Shown::new(last), "")
            .expect("deleting in a paragraph");
        let new_text = check(&text, &edit, "deleting");
        let mut expected = shown.clone();
        expected.drain(first..last);
        assert_eq!(
            lines(&preview(&new_text)),
            lines(&expected),
            "deleted {first}..{last} of {text:?} -> {new_text:?}"
        );
        checked += 1;
    }
    assert!(checked > rounds(400) / 3, "only {checked} checks ran");
}

/// Return splits what is there and adds no text and loses none, whatever the block.
#[test]
fn return_keeps_every_visible_character() {
    use splat_preview::layout::{Atom, RunMap};
    let _limit = limited();
    let visible = |text: &str| -> String {
        layout(&parse(text), &NoImages, None)
            .blocks
            .iter()
            .flat_map(|b| &b.paras)
            .flat_map(|p| &p.runs)
            .filter(|r| !matches!(r.map, RunMap::Atom { atom: Atom::Marker, .. }))
            .flat_map(|r| r.text.chars())
            .filter(|c| !c.is_whitespace())
            .collect()
    };
    let mut rng = Rng::new(0x5eed_4e7e_0001);
    let mut checked = 0;
    let mut refused = 0;
    for _ in 0..rounds(400) {
        let text = ordinary_document(&mut rng);
        let doc = parse(&text);
        let l = layout(&doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge::new(&doc, &l, &index).unwrap();
        let shown = preview(&text);
        let mut p = 1 + rng.below(shown.len() - 2);
        while (0xDC00..0xE000).contains(&shown[p]) {
            p -= 1;
        }
        let alphabetic = |u: u16| char::from_u32(u32::from(u)).is_some_and(|c| c.is_alphabetic());
        if p == 0 || !alphabetic(shown[p - 1]) || !alphabetic(shown[p]) {
            continue;
        }
        let Some(edit) = bridge.newline(Shown::new(p)) else {
            refused += 1;
            continue;
        };
        let out = check(&text, &edit, "return");
        assert_eq!(visible(&out), visible(&text), "return at {p} of {text:?} -> {out:?}");
        checked += 1;
    }
    assert!(checked > rounds(400) / 4, "only {checked} checks ran");
    assert!(refused * 50 <= checked, "{refused} edits refused, {checked} made");
}

/// Joining two paragraphs shows both their texts as they were, and what lay hidden between them stays in the source.
#[test]
fn joining_paragraphs_keeps_what_they_show_and_what_lies_between_them() {
    const TEXTS: [&str; 12] = [
        "Wort",
        "**fett**",
        "*schräg*",
        "`code`",
        "[Link](#a)",
        "~~weg~~",
        "<u>unter</u>",
        "Caf&eacute;",
        "😀 Wort",
        "# Titel",
        "- Punkt",
        "> Zitat",
    ];
    const HIDDEN: [&str; 3] = ["<!-- Notiz -->", "[r]: http://x.y", "%% Kommentar %%"];
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_101e_0001);
    let (mut joined, mut refused) = (0, 0);
    use splat_preview::layout::{Atom, RunMap};
    let shown = |text: &str| -> String {
        let l = layout(&parse(text), &NoImages, None);
        l.blocks
            .iter()
            .flat_map(|b| &b.paras)
            .flat_map(|p| &p.runs)
            .filter(|r| !matches!(r.map, RunMap::Atom { atom: Atom::Marker, .. }))
            .flat_map(|r| r.text.chars())
            .filter(|c| !c.is_whitespace())
            .collect()
    };
    for _ in 0..rounds(400) {
        let (a, b) = (*rng.pick(&TEXTS), *rng.pick(&TEXTS));
        let hidden = (rng.below(2) == 0).then(|| *rng.pick(&HIDDEN));
        let text = match hidden {
            Some(hidden) => format!("{a}\n\n{hidden}\n\n{b}\n"),
            None => format!("{a}\n\n{b}\n"),
        };
        let doc = parse(&text);
        let l = layout(&doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge::new(&doc, &l, &index).unwrap();
        let shown_paras: Vec<_> = index.paras.iter().filter(|p| p.len > 0).collect();
        let [first, second] = shown_paras[..] else {
            continue;
        };
        let at = second.start.get();
        // Empty paragraphs between them are breaks of their own.
        if at != first.start.get() + first.len + 1 {
            continue;
        }
        let Some(edit) = bridge.replace(Shown::new(at - 1)..Shown::new(at), "") else {
            refused += 1;
            continue;
        };
        let out = check(&text, &edit, "joining");
        assert_eq!(shown(&out), shown(&text), "{text:?} -> {out:?}");
        if let Some(hidden) = hidden {
            assert!(out.contains(hidden), "{text:?} -> {out:?}");
        }
        joined += 1;
    }
    assert!(joined > rounds(400) / 2, "only {joined} checks ran");
    assert!(refused * 10 <= joined, "{refused} joins refused, {joined} made");
}

/// What the preview shows of a position and what the source holds there lead back to each other.
#[test]
fn positions_map_back_and_forth_between_preview_and_source() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_a119_000d);
    let mut checked = 0;
    for _ in 0..rounds(400) {
        let text = ordinary_document(&mut rng);
        let doc = parse(&text);
        let l = layout(&doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge::new(&doc, &l, &index).unwrap();
        let shown = preview(&text);
        for _ in 0..8 {
            let p = rng.below(shown.len() - 1);
            let letter = char::from_u32(u32::from(shown[p])).filter(|c| c.is_ascii_alphabetic());
            let Some(letter) = letter else { continue };
            if p == 0 || !char::from_u32(u32::from(shown[p - 1])).is_some_and(|c| c.is_ascii_alphabetic()) {
                continue;
            }
            let selected = bridge.source_selection(Shown::new(p)..Shown::new(p + 1));
            assert_eq!(text.at(selected.clone()), letter.to_string(), "{p} of {text:?}");
            let at = bridge.source_at(Shown::new(p)).expect("a place");
            assert_eq!(at, selected.start, "{p} of {text:?}");
            assert_eq!(preview_offset(&l, &index, at), Shown::new(p), "{at:?} of {text:?}");
            checked += 1;
        }
    }
    assert!(checked > rounds(400), "only {checked} checks ran");
}

/// Typing over a selection in a paragraph shows the typed text in its place and leaves the rest of what is shown.
#[test]
fn typing_over_a_selection_replaces_exactly_it() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_de1e_000e);
    let mut checked = 0;
    let lines = |shown: &[u16]| -> Vec<String> {
        String::from_utf16_lossy(shown)
            .split('\n')
            .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|l| !l.is_empty())
            .collect()
    };
    for round in 0..rounds(400) {
        let mut text = plain_document(&mut rng);
        if round % 2 == 1 {
            let at = text.find(' ').unwrap_or(text.len());
            text.insert_str(at, MARKED[rng.below(MARKED.len())]);
        }
        if round % 3 == 0 {
            text.push_str("\n\n");
            text.push_str(rng.pick(&BLOCKS));
        }
        let doc = parse(&text);
        let l = layout(&doc, &NoImages, None);
        let index = Index::new(&l);
        let bridge = Bridge::new(&doc, &l, &index).unwrap();
        let shown = preview(&text);
        let first = rng.below(shown.len());
        let last = (first + 1 + rng.below(6)).min(shown.len());
        let boundary = |p: usize| p >= shown.len() || !(0xDC00..0xE000).contains(&shown[p]);
        if !boundary(first) || !boundary(last) || shown[first..last].contains(&u16::from(b'\n')) {
            continue;
        }
        let edit = bridge
            .replace(Shown::new(first)..Shown::new(last), "q")
            .expect("typing over text");
        let new_text = check(&text, &edit, "typing over");
        let mut expected = shown.clone();
        expected.splice(first..last, [u16::from(b'q')]);
        assert_eq!(
            lines(&preview(&new_text)),
            lines(&expected),
            "typed over {first}..{last} of {text:?} -> {new_text:?}"
        );
        checked += 1;
    }
    assert!(checked > rounds(400) / 2, "only {checked} checks ran");
}
