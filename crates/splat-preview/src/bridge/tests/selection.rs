//! Opening a bridge, and selections of the preview in the source.

use super::*;

#[test]
fn a_layout_of_another_parse_opens_no_bridge() {
    let t = "Text\n";
    let doc = splat_markdown::reader::parse(t);
    let laid_out = crate::layout::layout(&doc, &crate::layout::NoImages, None);
    let index = Index::new(&laid_out);
    assert!(Bridge::new(&doc, &laid_out, &index).is_some());
    let same_text = splat_markdown::reader::parse(t);
    assert!(Bridge::new(&same_text, &laid_out, &index).is_none());
    let nothing = Layout::nothing_of(&same_text);
    assert!(Bridge::new(&same_text, &nothing, &Index::new(&nothing)).is_some());
    assert!(Bridge::new(&doc, &Layout::default(), &Index::default()).is_none());
}

/// The source range of a selection of the preview of `t`.
fn selected(t: &str, from: usize, to: usize) -> String {
    bridge_of!(t => d, l, index, b);
    t.at(b.source_selection(Shown::new(from)..Shown::new(to))).to_string()
}

#[test]
fn a_selection_of_the_preview_starts_in_its_first_character_and_counts_utf16() {
    let t = "ä😀 **fett** wort ![b](c.png) ende\n";
    // In the preview: ä, 😀 as two units, space, then `fett`.
    assert_eq!(selected(t, 0, 1), "ä");
    assert_eq!(selected(t, 1, 3), "😀");
    assert_eq!(selected(t, 4, 8), "fett");
    assert_eq!(selected(t, 6, 8), "tt");
    assert_eq!(selected(t, 9, 13), "** wort".trim_start_matches("** "));
    assert_eq!(selected(t, 14, 15), "![b](c.png)");
}

#[test]
fn selection_maps_to_source() {
    let t = "a **fett** b\n";
    let d = parse(t);
    let l = layout(&d, &NoImages, None);
    let i = Index::new(&l);
    let b = Bridge::new(&d, &l, &i).unwrap();
    let r = b.source_selection(Shown::new(2)..Shown::new(6));
    assert_eq!(t.at(r), "fett");
}
