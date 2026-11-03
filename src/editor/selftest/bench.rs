//! Timings of opening, typing and scrolling a document, printed for `make bench`.

use objc2::MainThreadMarker;
use objc2_foundation::NSRange;

use super::*;

fn timed(label: &str, f: impl FnOnce()) {
    let t = std::time::Instant::now();
    f();
    eprintln!("BENCH {label}: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
}

/// Times from `opened`, when the document was asked to open, until its window has the text applied: the call that opens it
/// returns long before. Then goes on with the rest.
pub(super) fn after_first_render(mtm: MainThreadMarker, opened: std::time::Instant) {
    let shown = editor(mtm).is_some_and(|e| quiet(&e) && storage(e.preview()).length() > 0);
    if shown {
        eprintln!(
            "BENCH open+first render: {:.0} ms",
            opened.elapsed().as_secs_f64() * 1000.0
        );
        if std::env::var_os("SPLAT_BENCH_IDLE").is_some() {
            after(12.0, || std::process::exit(0));
        } else {
            after(2.0, move || bench(mtm));
        }
    } else if opened.elapsed().as_secs_f64() > 300.0 {
        eprintln!("BENCH no first render within 300 s");
        std::process::exit(1);
    } else {
        after(0.01, move || after_first_render(mtm, opened));
    }
}

/// Measures the hot paths on the opened benchmark file and quits.
fn bench(mtm: MainThreadMarker) {
    let Some(e) = editor(mtm) else {
        eprintln!("BENCH no editor");
        std::process::exit(1);
    };
    let text = source(&e);
    eprintln!("BENCH size: {} bytes, {} lines", text.len(), text.lines().count());
    eprintln!(
        "BENCH source on TextKit 2: {}",
        e.source().textLayoutManager().is_some()
    );
    let parsed = std::cell::RefCell::new(None);
    timed("parse + colors (background part 1)", || {
        *parsed.borrow_mut() = Some(crate::markdown::incremental::parse_and_color(&text))
    });
    let parsed = parsed.into_inner().unwrap();
    timed("preview layout (background part 2)", || {
        let _ = crate::preview::layout::layout(
            &parsed.doc,
            &super::super::update::Images {
                dark: false,
                env: Default::default(),
            },
            None,
        );
    });
    let mut at = text.len() / 2;
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    let map = Utf16Map::new(&text);
    e.window().unwrap().makeFirstResponder(Some(e.source()));
    e.source()
        .setSelectedRange(NSRange::new(map.to_u16(Byte::new(at)).get(), 0));
    timed("keystroke in the middle + synchronous update", || {
        type_in_source(&e, "x");
        e.updater().update_now();
    });
    for _ in 0..3 {
        timed("  another one", || {
            type_in_source(&e, "x");
            e.updater().update_now();
        });
    }
    timed("keystroke in the source view alone", || type_in_source(&e, "y"));
    timed("handing the text to the background", || {
        e.updater().update_in_background()
    });
    timed("15 toolbar validations after the keystroke", || {
        for _ in 0..15 {
            e.editing().validate(Some(sel!(mdBold:)), 0, None);
        }
    });
    let clip = e.source().enclosingScrollView().unwrap().contentView();
    let y = e.source().bounds().size.height / 2.0;
    timed("scroll source to the middle + sync preview", || {
        clip.scrollToPoint(objc2_foundation::NSPoint::new(0.0, y));
        e.source().enclosingScrollView().unwrap().reflectScrolledClipView(&clip);
    });
    for fraction in [0.25, 0.75, 0.1, 0.9, 0.5] {
        let y = e.source().bounds().size.height * fraction;
        e.scroll_sync().hold(true);
        timed(
            &format!("jump source to {:.0} % without sync, with redraw", fraction * 100.0),
            || {
                clip.scrollToPoint(objc2_foundation::NSPoint::new(0.0, y));
                e.source().enclosingScrollView().unwrap().reflectScrolledClipView(&clip);
                e.window().unwrap().displayIfNeeded();
            },
        );
        e.scroll_sync().hold(false);
        e.scroll_sync().follow_source_next();
        timed("  sync of the preview to it", || e.scroll_sync().sync_scroll());
    }
    let (pending, blocks) = e.model().read(|st| {
        (
            st.blocks().placeholders.iter().filter(|p| **p).count(),
            st.layout().blocks.len(),
        )
    });
    eprintln!("BENCH preview blocks: {blocks}, still plain text: {pending}");
    for (n, fraction) in [0.3, 0.6, 0.85, 0.15].into_iter().enumerate() {
        after(3.0 + n as f64 * 0.6, move || {
            let e = editor(MainThreadMarker::new().unwrap()).unwrap();
            let clip = e.source().enclosingScrollView().unwrap().contentView();
            clip.scrollToPoint(objc2_foundation::NSPoint::new(
                0.0,
                e.source().bounds().size.height * fraction,
            ));
            e.source().enclosingScrollView().unwrap().reflectScrolledClipView(&clip);
        });
        after(3.4 + n as f64 * 0.6, move || {
            let e = editor(MainThreadMarker::new().unwrap()).unwrap();
            let tlm = e.source().textLayoutManager().unwrap();
            let content = tlm.textContentManager().unwrap();
            let Some(viewport) = tlm.textViewportLayoutController().viewportRange() else {
                eprintln!("BENCH sync check: no viewport");
                return;
            };
            use objc2_app_kit::NSTextElementProvider;
            let first =
                content.offsetFromLocation_toLocation(&content.documentRange().location(), &viewport.location());
            let text = source(&e);
            let byte = crate::markdown::text::byte_at_u16(&text, Byte::default(), crate::util::to_usize(first.max(0)));
            let source_block = e.model().read(move |st| {
                st.layout()
                    .blocks
                    .partition_point(|b| b.src.start <= byte)
                    .saturating_sub(1)
            });
            let preview_block = e.progressive().visible_preview_block();
            eprintln!(
                "BENCH sync check at {:.0} %: source shows block {source_block}, preview block {preview_block}",
                fraction * 100.0
            );
        });
    }
    for (n, fraction) in [0.35, 0.65, 0.05, 0.95].into_iter().enumerate() {
        after(2.0 + n as f64 * 0.25, move || {
            let e = editor(MainThreadMarker::new().unwrap()).unwrap();
            let clip = e.preview().enclosingScrollView().unwrap().contentView();
            e.scroll_sync().hold(true);
            timed(
                &format!("jump preview to {:.0} % without sync, with redraw", fraction * 100.0),
                || {
                    clip.scrollToPoint(objc2_foundation::NSPoint::new(
                        0.0,
                        e.preview().bounds().size.height * fraction,
                    ));
                    e.preview()
                        .enclosingScrollView()
                        .unwrap()
                        .reflectScrolledClipView(&clip);
                    e.window().unwrap().displayIfNeeded();
                },
            );
            e.scroll_sync().hold(false);
        });
    }
    for (n, fraction) in [0.4, 0.7, 0.2].into_iter().enumerate() {
        after(5.5 + n as f64 * 0.6, move || {
            let e = editor(MainThreadMarker::new().unwrap()).unwrap();
            let clip = e.preview().enclosingScrollView().unwrap().contentView();
            let t = std::time::Instant::now();
            clip.scrollToPoint(objc2_foundation::NSPoint::new(
                0.0,
                e.preview().bounds().size.height * fraction,
            ));
            e.preview()
                .enclosingScrollView()
                .unwrap()
                .reflectScrolledClipView(&clip);
            eprintln!(
                "BENCH jump preview to {:.0} % + sync source: {:.1} ms",
                fraction * 100.0,
                t.elapsed().as_secs_f64() * 1000.0
            );
        });
        after(5.9 + n as f64 * 0.6, move || {
            let e = editor(MainThreadMarker::new().unwrap()).unwrap();
            let tlm = e.source().textLayoutManager().unwrap();
            let content = tlm.textContentManager().unwrap();
            use objc2_app_kit::NSTextElementProvider;
            let top = e.source().visibleRect().origin.y - e.source().textContainerOrigin().y;
            let Some(fragment) = tlm.textLayoutFragmentForPosition(objc2_foundation::NSPoint::new(0.0, top.max(0.0)))
            else {
                eprintln!("BENCH sync check: no fragment at the source top");
                return;
            };
            let first = content.offsetFromLocation_toLocation(
                &content.documentRange().location(),
                &fragment.rangeInElement().location(),
            );
            let text = source(&e);
            let byte = crate::markdown::text::byte_at_u16(&text, Byte::default(), crate::util::to_usize(first.max(0)));
            let source_block = e.model().read(move |st| {
                st.layout()
                    .blocks
                    .partition_point(|b| b.src.start <= byte)
                    .saturating_sub(1)
            });
            let preview_block = e.progressive().visible_preview_block();
            eprintln!(
                "BENCH sync check preview {:.0} %: preview block {preview_block}, source shows block {source_block}",
                fraction * 100.0
            );
        });
    }
    after(8.0, move || {
        let e = editor(MainThreadMarker::new().unwrap()).unwrap();
        eprintln!("BENCH -- second keystroke after the images settled");
        e.source().scrollRangeToVisible(e.source().selectedRange());
        e.window().unwrap().displayIfNeeded();
        timed("keystroke in the source view alone", || type_in_source(&e, "z"));
        timed("reading the text from the source view", || e.with_text(|_, _| ()));
        timed("handing the text to the background", || {
            e.updater().update_in_background()
        });
    });
    after(9.0, move || {
        let e = editor(MainThreadMarker::new().unwrap()).unwrap();
        eprintln!("BENCH -- scrolling the preview down in screen steps");
        let clip = e.preview().enclosingScrollView().unwrap().contentView();
        let step = clip.bounds().size.height;
        for n in 0..40 {
            let y = clip.bounds().origin.y + step;
            clip.scrollToPoint(objc2_foundation::NSPoint::new(0.0, y));
            e.preview()
                .enclosingScrollView()
                .unwrap()
                .reflectScrolledClipView(&clip);
            let t = std::time::Instant::now();
            e.progressive().render_pending_blocks();
            if n % 10 == 9 {
                eprintln!(
                    "BENCH   scroll step {}: rendering what came into view {:.1} ms",
                    n + 1,
                    t.elapsed().as_secs_f64() * 1000.0
                );
            }
        }
    });
    after(11.0, move || {
        let e = editor(MainThreadMarker::new().unwrap()).unwrap();
        let clip = e.preview().enclosingScrollView().unwrap().contentView();
        let (mut worst, mut unformatted) = (0.0f64, 0usize);
        for k in 0..30 {
            let y = e.preview().bounds().size.height * (k as f64 * 0.033);
            let t = std::time::Instant::now();
            clip.scrollToPoint(objc2_foundation::NSPoint::new(0.0, y));
            e.preview()
                .enclosingScrollView()
                .unwrap()
                .reflectScrolledClipView(&clip);
            worst = worst.max(t.elapsed().as_secs_f64() * 1000.0);
            let visible = e.preview().visibleRect();
            let top = super::super::scroll::offset_at(e.preview(), visible.origin.y).unwrap_or(0);
            let bottom =
                super::super::scroll::offset_at(e.preview(), visible.origin.y + visible.size.height).unwrap_or(0);
            unformatted += e.model().read(move |st| {
                let block_at = |offset: usize| {
                    st.index()
                        .block_starts
                        .partition_point(|&s| s.get() <= offset)
                        .saturating_sub(1)
                };
                let (top, bottom) = (block_at(top), block_at(bottom));
                st.blocks().placeholders[top..=bottom.min(st.blocks().placeholders.len() - 1)]
                    .iter()
                    .filter(|p| **p)
                    .count()
            });
        }
        eprintln!(
            "BENCH fast scrolling over the preview: slowest step {worst:.1} ms, unformatted blocks in view {unformatted}"
        );
    });
    after(14.0, move || {
        let e = editor(MainThreadMarker::new().unwrap()).unwrap();
        let pending = e
            .model()
            .read(|st| st.blocks().placeholders.iter().filter(|p| **p).count());
        eprintln!("BENCH still plain text after 14 s (only rendered near the visible part): {pending}");
        eprintln!(
            "BENCH source still on TextKit 2: {}",
            e.source().textLayoutManager().is_some()
        );
        if std::env::var_os("SPLAT_BENCH_TYPING").is_some() {
            eprintln!("BENCH TYPING START");
            keep_typing(30);
            return;
        }
        eprintln!("BENCH done");
        std::process::exit(0);
    });
}

/// Types one character per second, as fast as a 10 MB document finishes a background update.
fn keep_typing(left: usize) {
    if left == 0 {
        eprintln!("BENCH TYPING END");
        after(4.0, || std::process::exit(0));
        return;
    }
    after(1.0, move || {
        let e = editor(MainThreadMarker::new().unwrap()).unwrap();
        type_in_source(&e, "w");
        keep_typing(left - 1);
    });
}
