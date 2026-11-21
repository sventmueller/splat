//! The view menu, the toolbar, the outline beside the source, and marks the preview shows of a selection.

use std::cell::Cell;
use std::path::Path;

use objc2::rc::Retained;
use objc2::runtime::{ProtocolObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSBitmapImageRep, NSColor, NSMenu, NSSegmentedControl, NSToolbarDisplayMode,
    NSUserInterfaceItemIdentification, NSView, NSWindowButton,
};
use objc2_foundation::{NSData, NSPoint, NSRange};

use super::super::{
    Ctx, Step, append, applied, check, check_in_front, pause, perform, select_in_source, settle, source,
    type_in_preview, type_in_source, when, window_image,
};
use super::{before, remember, sizes, undo, undone};
use crate::editor::actions::Action;
use crate::util::storage;

thread_local! {
    /// The outline's rows before the step under test.
    static ROWS: Cell<isize> = const { Cell::new(0) };
}

/// The menu items of the main menu whose action is `selector`.
fn menu_items(mtm: MainThreadMarker, selector: Sel) -> Vec<Retained<objc2_app_kit::NSMenuItem>> {
    fn walk(menu: &NSMenu, selector: Sel, out: &mut Vec<Retained<objc2_app_kit::NSMenuItem>>) {
        for item in menu.itemArray().iter() {
            if item.action() == Some(selector) {
                out.push(item.clone());
            }
            if let Some(sub) = item.submenu() {
                walk(&sub, selector, out);
            }
        }
    }
    let mut out = Vec::new();
    if let Some(menu) = NSApplication::sharedApplication(mtm).mainMenu() {
        walk(&menu, selector, &mut out);
    }
    out
}

pub(super) fn view_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            select_in_source(&c.e, "fett");
            // Validated by hand: AppKit asks the controller only while the app is in front.
            let validate = |selector: Sel| {
                let items = menu_items(c.e.mtm(), selector);
                for item in &items {
                    c.e.editing().validate(item.action(), item.tag(), Some(item));
                }
                items
            };
            let bold = validate(Action::Bold.spec().selector);
            check(
                "the menu checks bold where the caret is in bold text",
                !bold.is_empty() && bold.iter().all(|i| i.state() == objc2_app_kit::NSControlStateValueOn),
                &format!("{} items", bold.len()),
            );
            let preview = validate(Action::TogglePreview.spec().selector);
            check(
                "the menu offers to hide the shown preview",
                preview.iter().any(|i| i.title().to_string() == "Vorschau ausblenden"),
                "",
            );
            perform(&c.e, Action::ToggleOutline.spec().selector, 0);
        }),
        when(
            "the outline to open",
            |c| c.e.panes().outline_visible(),
            |c| {
                check("the outline opens", c.e.panes().outline_visible(), "");
                perform(&c.e, Action::ToggleOutline.spec().selector, 0);
            },
        ),
        when(
            "the outline to close",
            |c| !c.e.panes().outline_visible(),
            |c| {
                check("the outline closes", !c.e.panes().outline_visible(), "");
                let on = crate::editor::env::typography_on();
                perform(&c.e, Action::Typography.spec().selector, 0);
                check(
                    "typographic punctuation switches",
                    crate::editor::env::typography_on() != on,
                    "",
                );
                perform(&c.e, Action::Typography.spec().selector, 0);
                perform(&c.e, Action::ZoomIn.spec().selector, 0);
                let zoom = c.e.model().read(|st| st.zoom());
                check("zooming in enlarges", zoom > 1.0, &format!("{zoom}"));
                let kept = crate::editor::style::chosen_zoom();
                check(
                    "the zoom is kept for new windows and the next start",
                    kept == zoom,
                    &format!("{kept} kept, {zoom} shown"),
                );
                perform(&c.e, Action::ZoomOut.spec().selector, 0);
                perform(&c.e, Action::ZoomOut.spec().selector, 0);
                let zoom = c.e.model().read(|st| st.zoom());
                check("zooming out shrinks", zoom < 1.0, &format!("{zoom}"));
                perform(&c.e, Action::ZoomReset.spec().selector, 0);
                let zoom = c.e.model().read(|st| st.zoom());
                check(
                    "the actual size comes back, and stays",
                    zoom == 1.0 && crate::editor::style::chosen_zoom() == 1.0,
                    &format!("{zoom}"),
                );
                check(
                    "the menu zooms in with command plus",
                    menu_items(c.e.mtm(), Action::ZoomIn.spec().selector)
                        .iter()
                        .any(|item| item.keyEquivalent().to_string() == "+"),
                    "",
                );
            },
        ),
        settle(|c| {
            select_in_source(&c.e, "drei.");
            remember(c);
            // Typing in the source schedules a parse; typing in the preview before it lands cannot be mapped.
            type_in_source(&c.e, "Q");
            c.e.window().map(|w| w.makeFirstResponder(Some(c.e.preview())));
            type_in_preview(&c.e, "Zzz");
        }),
        settle(|c| {
            let preview = storage(c.e.preview()).string().to_string();
            check(
                "typing in the preview while the source is parsed shows the source again",
                source(&c.e).contains("zwei Q") && !preview.contains("Zzz") && !source(&c.e).contains("Zzz"),
                "",
            );
        }),
        applied(|c| {
            undo(c);
            check("the typing is undone", source(&c.e) == before(c), &sizes(c));
        }),
    ]
}

/// The line numbers beside the source, from the top of a text to its end, and the room they take.
pub(super) fn line_number_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            c.e.source().scrollPoint(NSPoint::new(0.0, 0.0));
            perform(&c.e, Action::ToggleLineNumbers.spec().selector, 0);
        }),
        applied(|c| {
            let window = c.e.window().unwrap();
            window.layoutIfNeeded();
            window_image(&window, &c.out.join("line-numbers-top.png"));
            let drawn = c.e.line_numbers().drawn();
            let width = c.e.line_numbers().width();
            check(
                "line numbers take a column beside the source",
                width > 0.0,
                &format!("{width}"),
            );
            let scroll = c.e.source().enclosingScrollView().map(|s| s.frame().origin.x);
            check(
                "the source moves aside for the numbers",
                scroll.is_some_and(|x| (x - width).abs() < 0.5),
                &format!("{scroll:?} for {width}"),
            );
            let text = source(&c.e);
            let top = c.e.panes().source().top_offset().unwrap_or(0);
            let expected = text.encode_utf16().take(top).filter(|&u| u == u16::from(b'\n')).count() + 1;
            check(
                "the first number is the line at the top of the source",
                drawn.first().map(|&(line, _)| line) == Some(expected),
                &format!("{:?}, top at {top}", drawn.first()),
            );
            check(
                "the numbers count up line by line, downwards",
                drawn.len() > 3 && drawn.windows(2).all(|w| w[1].0 == w[0].0 + 1 && w[1].1 > w[0].1),
                &format!("{drawn:?}"),
            );
            let items = menu_items(c.e.mtm(), Action::ToggleLineNumbers.spec().selector);
            for item in &items {
                c.e.editing().validate(item.action(), item.tag(), Some(item));
            }
            check(
                "the View menu checks the line numbers",
                !items.is_empty() && items.iter().all(|i| i.state() == objc2_app_kit::NSControlStateValueOn),
                &format!("{} items", items.len()),
            );
            c.e.source()
                .scrollRangeToVisible(NSRange::new(storage(c.e.source()).length(), 0));
        }),
        applied(|c| {
            let window = c.e.window().unwrap();
            window.layoutIfNeeded();
            window_image(&window, &c.out.join("line-numbers-end.png"));
            let drawn = c.e.line_numbers().drawn();
            let lines = source(&c.e).matches('\n').count() + 1;
            check(
                "the last line has its number, also when it is the empty one after a final line break",
                drawn.last().map(|&(line, _)| line) == Some(lines),
                &format!("{:?} of {lines}", drawn.last()),
            );
            c.e.source().scrollPoint(NSPoint::new(0.0, 0.0));
            perform(&c.e, Action::ToggleLineNumbers.spec().selector, 0);
        }),
        applied(|c| {
            if let Some(window) = c.e.window() {
                window.layoutIfNeeded();
            }
            let scroll = c.e.source().enclosingScrollView().map(|s| s.frame().origin.x);
            check(
                "switching the numbers off gives their room back",
                c.e.line_numbers().width() == 0.0 && scroll == Some(0.0),
                &format!("{scroll:?}"),
            );
        }),
    ]
}

/// AppKit gives no frames of toolbar items, so this looks for the private views of their capsules by class.
fn capsule_ends(view: &NSView, out: &mut Vec<f64>) {
    if view.class().name().to_bytes().ends_with(b"NSToolbarPlatterView") {
        let frame = view.convertRect_toView(view.bounds(), None);
        out.push(frame.origin.x + frame.size.width);
    }
    for sub in view.subviews().iter() {
        capsule_ends(&sub, out);
    }
}

/// The horizontal span of the first view under `view` that `wanted` picks, in window coordinates.
fn span_of(view: &NSView, wanted: &dyn Fn(&NSView) -> bool) -> Option<(f64, f64)> {
    if wanted(view) {
        let frame = view.convertRect_toView(view.bounds(), None);
        return Some((frame.origin.x, frame.origin.x + frame.size.width));
    }
    view.subviews().iter().find_map(|sub| span_of(&sub, wanted))
}

/// Where the zoom button ends, the sidebar button stands and the title begins.
fn title_row(c: &Ctx) -> Option<(f64, (f64, f64), f64)> {
    let window = c.e.window()?;
    let zoom = window.standardWindowButton(NSWindowButton::ZoomButton)?;
    let zoom_end = zoom.convertRect_toView(zoom.bounds(), None);
    // SAFETY: Only read here, on the main thread, while the window holds its content view.
    let frame = unsafe { window.contentView()?.superview() }?;
    let sidebar = Action::ToggleSidebar.spec().selector;
    let button = span_of(&frame, &|v| {
        v.downcast_ref::<objc2_app_kit::NSControl>()
            .is_some_and(|control| control.action() == Some(sidebar))
    })?;
    let title = span_of(&frame, &|v| {
        v.class().name().to_bytes().ends_with(b"NSToolbarTitleField")
    })?;
    Some((zoom_end.origin.x + zoom_end.size.width, button, title.0))
}

/// How far the toolbar's last capsule stands from the window's right edge.
fn toolbar_margin(c: &Ctx) -> Option<f64> {
    let window = c.e.window()?;
    let content = window.contentView()?;
    // SAFETY: Only read here, on the main thread, while the window holds its content view.
    let frame = unsafe { content.superview() }?;
    let mut ends = Vec::new();
    capsule_ends(&frame, &mut ends);
    let last = ends.into_iter().fold(f64::NEG_INFINITY, f64::max);
    last.is_finite().then(|| window.frame().size.width - last)
}

fn set_toolbar_mode(c: &Ctx, mode: NSToolbarDisplayMode) {
    if let Some(toolbar) = c.e.window().and_then(|w| w.toolbar()) {
        toolbar.setDisplayMode(mode);
    }
}

pub(super) fn toolbar_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            let mtm = c.e.mtm();
            let share = ProtocolObject::from_ref(&*c.e);
            let allowed = crate::editor::toolbar::allowed_identifiers();
            let missing: Vec<String> = allowed
                .iter()
                .map(|id| id.to_string())
                .filter(|id| !id.starts_with("NSToolbar"))
                .filter(|id| crate::editor::toolbar::make(mtm, id, share, &c.e).is_none())
                .collect();
            check(
                "every item the toolbar offers can be made",
                missing.is_empty(),
                &format!("{missing:?}"),
            );
            select_in_source(&c.e, "eins");
            remember(c);
            let bold = Action::Bold.spec().selector;
            let control = c.e.window().and_then(|w| w.toolbar()).and_then(|toolbar| {
                toolbar.items().iter().find_map(|item| {
                    let control = item.view()?.downcast::<NSSegmentedControl>().ok()?;
                    let group = NSUserInterfaceItemIdentification::identifier(&*control)?.to_string();
                    let segment = (0..control.segmentCount()).find(|&s| {
                        crate::editor::toolbar::segment_item(mtm, &group, crate::util::to_usize(s), share, &c.e)
                            .is_some_and(|i| i.action() == Some(bold))
                    })?;
                    Some((control, segment))
                })
            });
            check("the bold button sits in a toolbar group", control.is_some(), "");
            // Its segments are momentary and forget a selection made outside a click.
            if let Some((control, segment)) = control {
                let mode = control.trackingMode();
                control.setTrackingMode(objc2_app_kit::NSSegmentSwitchTracking::SelectOne);
                control.setSelectedSegment(segment);
                crate::editor::toolbar::segment_clicked(&c.e, Some(&control));
                control.setSelectedSegment(-1);
                control.setTrackingMode(mode);
            }
            // The action goes through the responder chain, which starts at a key window.
            check_in_front(
                c.e.mtm(),
                "a toolbar group runs the command of its segment",
                source(&c.e).contains("**eins**"),
                "",
            );
        }),
        undone("a toolbar command is undone in one step".into()),
        applied(|c| set_toolbar_mode(c, NSToolbarDisplayMode::LabelOnly)),
        pause(0.5, "the toolbar reports no end of its layout", |c| {
            let margin = toolbar_margin(c);
            let close =
                c.e.window()
                    .and_then(|w| w.standardWindowButton(NSWindowButton::CloseButton))
                    .map(|b| b.convertRect_toView(b.bounds(), None).origin.x);
            check(
                "the text-only toolbar stands as far from the edge as the close button",
                margin.zip(close).is_some_and(|(m, x)| (m - x).abs() <= 2.0),
                &format!("margin {margin:?}, close button at {close:?}"),
            );
            set_toolbar_mode(c, NSToolbarDisplayMode::IconOnly);
        }),
        pause(0.5, "the toolbar reports no end of its layout", |c| {
            let margin = toolbar_margin(c);
            check(
                "the toolbar with symbols keeps its place at the edge",
                margin.is_some_and(|m| (m - 8.0).abs() <= 1.0),
                &format!("{margin:?}"),
            );
            let row = title_row(c);
            check(
                "the sidebar button stands between the window buttons and the title",
                row.is_some_and(|(zoom, (start, end), title)| zoom < start && end <= title),
                &format!("zoom button ends, sidebar button spans, title begins: {row:?}"),
            );
        }),
    ]
}

/// Without a display an animated collapse only ends at the item's next toggle, and then undoes that toggle.
fn show_outline(c: &Ctx, shown: bool) {
    c.e.panes().set_outline_collapsed(!shown);
    c.e.panes().show_headings_if_open();
}

/// Like `show_outline`, for the preview.
fn show_preview(c: &Ctx, shown: bool) {
    c.e.panes().set_preview_collapsed(!shown);
    c.e.updater().update_now();
}

/// The title and the start of every heading, read from all of the document.
fn headings_of(c: &Ctx) -> Vec<(String, crate::markdown::text::Byte)> {
    c.e.model().read(|st| {
        let headings = splat_editor_state::outline::Headings::of(st.doc());
        headings
            .list()
            .iter()
            .map(|h| (h.title.clone(), h.range.start))
            .collect()
    })
}

/// The outline stays in the window while the preview is hidden, and so must follow the text.
pub(super) fn outline_without_preview_steps() -> Vec<Step> {
    let rows = |c: &Ctx| c.e.panes().outline_rows().unwrap_or(0);
    vec![
        applied(|c| {
            remember(c);
            show_preview(c, false);
        }),
        when(
            "the preview to hide",
            |c| !c.e.panes().preview_visible(),
            |c| {
                show_outline(c, true);
            },
        ),
        when(
            "the outline to show",
            |c| c.e.panes().outline_visible(),
            move |c| {
                ROWS.with(|r| r.set(rows(c)));
                append(&c.e, "\n\n# Neu in der Gliederung\n");
            },
        ),
        when(
            "the outline to follow the text",
            move |c| rows(c) > ROWS.with(Cell::get),
            move |c| {
                check(
                    "the outline follows the text while the preview is hidden",
                    rows(c) == ROWS.with(Cell::get) + 1,
                    &format!("{} rows, before {}", rows(c), ROWS.with(Cell::get)),
                );
                let at = source(&c.e).rfind("# Neu in der Gliederung").unwrap_or(0);
                c.e.editing().apply_edit(
                    crate::markdown::commands::Edit {
                        range: crate::markdown::text::bytes(at..at),
                        replacement: "Davor\n\n".to_string(),
                        selection: crate::markdown::text::bytes(at..at),
                    },
                    None,
                    false,
                );
            },
        ),
        when(
            "the outline to move along with the text in front of a heading",
            |c| c.e.panes().outline_nodes() == headings_of(c),
            move |c| {
                check(
                    "the outline moves its headings along with the text in front of them",
                    c.e.panes().outline_nodes() == headings_of(c) && rows(c) == ROWS.with(Cell::get) + 1,
                    &format!("{:?}", c.e.panes().outline_nodes().last()),
                );
                undo(c);
                undo(c);
                show_outline(c, false);
            },
        ),
        when(
            "the outline to hide",
            |c| !c.e.panes().outline_visible(),
            |c| show_preview(c, true),
        ),
        when(
            "the preview to show and settle",
            |c| c.e.panes().preview_visible() && super::super::settled(&c.e),
            |c| {
                check(
                    "the outline check leaves the text",
                    source(&c.e) == before(c),
                    &sizes(c),
                )
            },
        ),
    ]
}

/// Captures the preview's part of the screen: only the window server shows what was really redrawn, and a capture of the
/// window itself changes its size along the way.
fn capture_preview(c: &Ctx, path: &Path) {
    let window = c.e.window().expect("a window");
    let number = window.windowNumber();
    let captured = std::process::Command::new("screencapture")
        .args(["-x", "-o", &format!("-l{number}")])
        .arg(path)
        .status();
    check_in_front(
        c.e.mtm(),
        "the preview can be captured",
        captured.is_ok_and(|s| s.success()),
        &format!("window {number}"),
    );
}

/// The colors of a capture, without the right edge, where a scroller fades.
fn colors_in(c: &Ctx, path: &Path) -> Vec<Retained<NSColor>> {
    let Some(image) = std::fs::read(path)
        .ok()
        .and_then(|png| NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(&png)))
    else {
        return Vec::new();
    };
    let preview = c.e.preview();
    let window = c.e.window().expect("a window");
    let shown = preview.convertRect_toView(preview.visibleRect(), None);
    let scale = window.backingScaleFactor();
    #[expect(clippy::cast_possible_truncation, reason = "pixels of a window")]
    let pixels = |points: f64| (points * scale) as isize;
    let (left, wide) = (pixels(shown.origin.x), pixels(shown.size.width));
    let top = pixels(window.frame().size.height - shown.origin.y - shown.size.height);
    let high = pixels(shown.size.height);
    (left..left + wide * 9 / 10)
        .step_by(4)
        .flat_map(|x| (top..top + high).step_by(4).map(move |y| (x, y)))
        .filter_map(|(x, y)| image.colorAtX_y(x, y))
        .collect()
}

pub(super) fn marks_on_screen_steps() -> Vec<Step> {
    const REDRAWN: &str = "only the screen shows a redraw, and it gives no signal when that has happened";
    vec![
        applied(|c| {
            c.e.window().expect("a window").makeFirstResponder(Some(c.e.source()));
            c.e.source().setSelectedRange(NSRange::new(0, 0));
            c.e.source().scrollRangeToVisible(NSRange::new(0, 0));
        }),
        pause(1.0, REDRAWN, |c| {
            capture_preview(c, &c.out.join("marks-before.png"));
            c.e.source().setSelectedRange(NSRange::new(0, 300));
        }),
        pause(0.5, REDRAWN, |c| {
            capture_preview(c, &c.out.join("marks-after.png"));
            let before = colors_in(c, &c.out.join("marks-before.png"));
            let after = colors_in(c, &c.out.join("marks-after.png"));
            let changed = before.iter().zip(&after).filter(|(a, b)| a != b).count();
            check_in_front(
                c.e.mtm(),
                "what is selected in the source gets marked on screen in the preview",
                changed > 100 && before.len() == after.len(),
                &format!("{changed} of {} and {} points changed", before.len(), after.len()),
            );
            c.e.source().setSelectedRange(NSRange::new(0, 0));
        }),
    ]
}
