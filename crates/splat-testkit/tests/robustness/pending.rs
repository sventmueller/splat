//! Typing at a caret where styles wait for the next word, in the source and in the preview.

use splat_markdown::commands::{self, InlineStyle, Toggle};
use splat_markdown::reader::parse;
use splat_preview::bridge::Bridge;
use splat_preview::layout::{NoImages, layout};
use splat_preview::map::{Index, preview_offset};
use splat_testkit::{Rng, rounds};

use super::common::{check, document, limited};

const PENDING: [InlineStyle; 6] = [
    InlineStyle::Bold,
    InlineStyle::Italic,
    InlineStyle::Code,
    InlineStyle::Highlight,
    InlineStyle::Superscript,
    InlineStyle::Strikethrough,
];

/// Text typed in the source at a caret with pending styles moves into their markers and leaves the rest alone.
#[test]
fn typing_at_pending_styles_in_the_source_never_breaks_the_text() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_9e4d_0006);
    let mut checked = 0;
    for _ in 0..rounds(400) {
        let base = document(&mut rng);
        let at = rng.boundary(&base);
        let style = *rng.pick(&PENDING);
        let Toggle::Pending(Some(pending)) = commands::toggle(&parse(&base), at..at, style, None) else {
            continue;
        };
        let typed = *rng.pick(&["x", "neu", " ", " a", "ä😀"]);
        let mut text = base.clone();
        text.insert_str(pending.at.get(), typed);
        let Some((edit, keep_pending)) = commands::typed_at_pending(&parse(&base), &text, &pending) else {
            panic!("typing {typed:?} at {pending:?} of {base:?} was not recognised");
        };
        let out = check(&text, &edit, "typed at pending");
        assert!(out.contains(typed.trim()), "{typed:?} lost in {out:?}");
        assert_eq!(keep_pending, typed.trim().is_empty(), "{typed:?} -> {out:?}");
        checked += 1;
    }
    assert!(checked > rounds(400) / 10, "only {checked} checks ran");
}

/// The same in the preview: the edit fits the text, and whitespace keeps the styles waiting at the new caret.
#[test]
fn typing_at_pending_styles_in_the_preview_never_breaks_the_text() {
    let _limit = limited();
    let mut rng = Rng::new(0x5eed_9e4d_0007);
    let mut checked = 0;
    for _ in 0..rounds(400) {
        let text = document(&mut rng);
        let doc = parse(&text);
        let at = rng.boundary(&text);
        let Toggle::Pending(Some(pending)) = commands::toggle(&doc, at..at, *rng.pick(&PENDING), None) else {
            continue;
        };
        let l = layout(&doc, &NoImages, Some(pending.at));
        let index = Index::new(&l);
        let bridge = Bridge::new(&doc, &l, &index).unwrap();
        let typed = *rng.pick(&["x", " ", "ä", "*"]);
        let p = preview_offset(&l, &index, pending.at);
        let Some((edit, next)) = bridge.type_with_pending(&pending, p, typed) else {
            continue;
        };
        let out = check(&text, &edit, "preview typed at pending");
        assert_eq!(next.is_some(), typed.trim().is_empty(), "{typed:?} -> {out:?}");
        if let Some(next) = next {
            assert_eq!(next.at, edit.selection.start, "{typed:?} -> {out:?}");
        }
        checked += 1;
    }
    assert!(checked > rounds(400) / 10, "only {checked} checks ran");
}
