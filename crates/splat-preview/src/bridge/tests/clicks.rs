//! Clicks on checkboxes, chevrons and details tags.

use super::*;

#[test]
fn the_chevron_of_an_embedded_note_flips_nothing_in_this_text() {
    struct Note;
    impl crate::layout::Resolver for Note {
        fn math_ready(&self, _: &str, _: bool) -> bool {
            false
        }
        fn diagram_ready(&self, _: &str, _: splat_markdown::doc::DiagramKind) -> bool {
            false
        }
        fn embedded_note(&self, _: &str) -> Option<(String, String)> {
            Some(("> [!NOTE]- Titel\n> Inhalt\n".to_string(), String::new()))
        }
    }
    let t = "![[Notiz]]\n";
    let d = splat_markdown::reader::parse(t);
    let l = crate::layout::layout(&d, &Note, None);
    let index = Index::new(&l);
    let bridge = Bridge::new(&d, &l, &index).unwrap();
    let chevron = l
        .blocks
        .iter()
        .flat_map(|b| &b.paras)
        .flat_map(|p| p.runs_at(index.paras[0].start))
        .find(|(_, r)| {
            matches!(
                r.map,
                RunMap::Atom {
                    atom: crate::layout::Atom::Fold { .. },
                    ..
                }
            )
        })
        .map(|(at, _)| at.start)
        .expect("a chevron");
    assert_eq!(bridge.toggle_fold(chevron), None);
}

#[test]
fn the_chevron_of_a_callout_in_this_text_flips_its_sign() {
    let t = "x\n\n> [!NOTE]- Titel\n> Inhalt\n";
    bridge_of!(t => d, l, index, bridge);
    let place = index.paras[1];
    let chevron = index
        .para(&l, 1)
        .runs_at(place.start)
        .find(|(_, r)| {
            matches!(
                r.map,
                RunMap::Atom {
                    atom: crate::layout::Atom::Fold { .. },
                    ..
                }
            )
        })
        .map(|(at, _)| at.start)
        .expect("a chevron");
    let mut out = t.to_string();
    bridge.toggle_fold(chevron).expect("an edit").apply_to(&mut out);
    assert_eq!(out, "x\n\n> [!NOTE]+ Titel\n> Inhalt\n");
}

#[test]
fn toggling_open_looks_at_attributes_and_not_into_their_values() {
    assert_eq!(
        toggle_open("<details title=\"a open b\" open>", true),
        Some((25..30, String::new()))
    );
    assert_eq!(toggle_open("<details title=\"a open b\">", true), None);
    assert_eq!(
        toggle_open("<details open=\"open\">", true),
        Some((8..20, String::new()))
    );
}

#[test]
fn checkboxes_and_chevrons_find_their_own_place_further_down() {
    let t = "Text\n\n- [ ] a\n- [x] b\n";
    assert_eq!(
        edited(t, |b| b.toggle_task(Shown::new(5))),
        "Text\n\n- [x] a\n- [x] b\n"
    );
    assert_eq!(
        edited(t, |b| b.toggle_task(Shown::new(9))),
        "Text\n\n- [ ] a\n- [ ] b\n"
    );
    let t = "x\n\n> [!note]- Titel\n> Inhalt\n\ny\n";
    let chevron = 2 + 1 + " Titel".len();
    assert_eq!(
        edited(t, |b| b.toggle_fold(Shown::new(chevron))),
        "x\n\n> [!note]+ Titel\n> Inhalt\n\ny\n"
    );
    let t = "x\n\n> [!note]+ Titel\n> Inhalt\n\ny\n";
    assert_eq!(
        edited(t, |b| b.toggle_fold(Shown::new(chevron))),
        "x\n\n> [!note]- Titel\n> Inhalt\n\ny\n"
    );
}

#[test]
fn nothing_but_the_chevron_folds_and_nothing_but_a_checkbox_toggles() {
    let t = "x\n\n> [!note]- Titel\n> Inhalt\n\n- [ ] y\n";
    bridge_of!(t => d, l, index, b);
    assert_eq!(b.toggle_fold(Shown::new(0)), None);
    assert_eq!(b.toggle_fold(Shown::new(3)), None, "the title itself");
    assert_eq!(b.toggle_task(Shown::new(0)), None);
}

#[test]
fn the_caret_stays_behind_what_a_click_changed() {
    let t = "- [ ] a\n";
    bridge_of!(t => d, l, index, b);
    let edit = b.toggle_task(Shown::new(0)).unwrap();
    assert_eq!(edit.range, bytes(3..4));
    assert_eq!(edit.selection, bytes(4..4));
}

#[test]
fn the_open_attribute_goes_and_comes_whatever_way_it_is_written() {
    let toggled = |tag: &str, open: bool| {
        let (range, replacement) = toggle_open(tag, open).unwrap();
        format!("{}{replacement}{}", &tag[..range.start], &tag[range.end..])
    };
    assert_eq!(toggled("<details>", false), "<details open>");
    assert_eq!(toggled("<details/>", false), "<details open/>");
    assert_eq!(toggled("<details class=\"a\">", false), "<details class=\"a\" open>");
    assert_eq!(toggled("<details open>", true), "<details>");
    assert_eq!(toggled("<DETAILS OPEN>", true), "<DETAILS>");
    assert_eq!(toggled("<details open class=\"a\">", true), "<details class=\"a\">");
    assert_eq!(toggled("<details open=\"\">", true), "<details>");
    assert_eq!(toggled("<details open='open' id=x>", true), "<details id=x>");
    assert_eq!(toggled("<details open=open id=x>", true), "<details id=x>");
    assert_eq!(toggled("<details open = \"\">", true), "<details>");
    assert_eq!(
        toggled("<details class=\"open\" open>", true),
        "<details class=\"open\">"
    );
    assert_eq!(toggled("<details data-open open>", true), "<details data-open>");
    assert_eq!(toggled("<details open=\"a\" b>", true), "<details b>");
    assert_eq!(toggled("<details open='a' b>", true), "<details b>");
    assert_eq!(toggled("<details openly open>", true), "<details openly>");
}

#[test]
fn a_task_checkbox_toggles_both_ways() {
    assert_eq!(edited("- [ ] a\n", |b| b.toggle_task(Shown::new(0))), "- [x] a\n");
    assert_eq!(edited("- [x] a\n", |b| b.toggle_task(Shown::new(0))), "- [ ] a\n");
}

#[test]
fn the_chevron_of_an_indented_details_changes_its_opening_tag() {
    for (t, want) in [
        (
            "  <details>\n  <summary>Mehr</summary>\n  Inhalt\n  </details>\n",
            "  <details open>\n  <summary>Mehr</summary>\n  Inhalt\n  </details>\n",
        ),
        (
            " <details>\n<summary>Mehr</summary>\n\nInhalt\n</details>\n",
            " <details open>\n<summary>Mehr</summary>\n\nInhalt\n</details>\n",
        ),
    ] {
        bridge_of!(t => d, l, index, b);
        let mut out = t.to_string();
        b.toggle_fold(Shown::new(0)).unwrap().apply_to(&mut out);
        assert_eq!(out, want);
    }
}

#[test]
fn only_a_whole_details_tag_is_ever_toggled() {
    assert_eq!(toggle_open("<details>", false), Some((8..8, " open".to_string())));
    assert_eq!(toggle_open("<details>\n</details>", false), None);
    assert_eq!(toggle_open("<summary>", false), None);
    assert_eq!(toggle_open("nichts", false), None);
}

#[test]
fn details_open_and_close_in_their_tag() {
    let apply = |tag: &str, open: bool| {
        let (r, with) = toggle_open(tag, open).unwrap();
        format!("{}{with}{}", &tag[..r.start], &tag[r.end..])
    };
    assert_eq!(apply("<details>", false), "<details open>");
    assert_eq!(apply("<details open>", true), "<details>");
    assert_eq!(
        apply("<details class=\"x\" open=\"open\">", true),
        "<details class=\"x\">"
    );
    assert_eq!(apply("<details OPEN data-opener=1>", true), "<details data-opener=1>");
    assert!(closes_shortcode("so :smile", ""));
    assert!(!closes_shortcode("Uhr", ""));
}

#[test]
fn a_click_on_details_leaves_the_caret_behind_the_change() {
    let t = "x\n\n<details>\n<summary>Mehr</summary>\n\nInhalt\n\n</details>\n";
    let chevron = 2;
    assert_eq!(
        with_caret(t, |b| b.toggle_fold(Shown::new(chevron))),
        "x\n\n<details open|>\n<summary>Mehr</summary>\n\nInhalt\n\n</details>\n"
    );
}

#[test]
fn a_click_before_a_chevron_folds_nothing() {
    let t = "x\n\n> [!note]- Titel\n> Inhalt\n";
    let chevron = 2 + 1 + " Titel".len();
    assert_eq!(edit_of(t, |b| b.toggle_fold(Shown::new(2))), None, "the icon");
    assert_eq!(
        edit_of(t, |b| b.toggle_fold(Shown::new(chevron - 1))),
        None,
        "the title's end"
    );
}

#[test]
fn a_quoted_open_value_goes_with_its_spaces() {
    let (range, replacement) = toggle_open("<details open=\"a b\" c>", true).unwrap();
    assert_eq!((range, replacement.as_str()), (8..19, ""));
}
