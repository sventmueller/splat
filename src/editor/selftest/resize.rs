//! How the window follows a resize, measured while AppKit's own tracking runs.

use std::cell::Cell;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSDocumentController};
use objc2_foundation::{NSRange, NSTimer};

use super::super::EditorController;
use super::super::actions::Action;
use super::super::sidebar::SidebarTab;
use super::{after, check, editor, finish, perform, wait};
use crate::document::MdDocument;

/// Drags the window's lower right corner through AppKit's own resize tracking, as a user does, and checks that the
/// source text follows the width while the mouse is still down.
pub(super) fn resize_probe(mtm: MainThreadMarker) {
    let Some((e, window)) = editor(mtm).and_then(|e| Some((e.clone(), e.window()?))) else {
        check("open document", false, "no editor window");
        finish();
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    app.activate();
    window.makeKeyAndOrderFront(None);
    // Halfway down a long text, where NSTextView kept the old width until the mouse was released.
    e.source()
        .scrollRangeToVisible(NSRange::new(e.source_text().len() / 2, 0));
    let (steps, lag) = (std::rc::Rc::new(Cell::new(0usize)), std::rc::Rc::new(Cell::new(0.0f64)));
    let observer = {
        let (e, window, steps, lag) = (e.clone(), window.clone(), steps.clone(), lag.clone());
        RcBlock::new(move |_: std::ptr::NonNull<objc2_foundation::NSNotification>| {
            if !window.inLiveResize() {
                return;
            }
            let source = e.source();
            let expected = source.frame().size.width - 2.0 * source.textContainerInset().width;
            let actual = crate::util::text_container(source).map_or(0.0, |c| c.size().width);
            steps.set(steps.get() + 1);
            lag.set(lag.get().max((expected - actual).abs()));
        })
    };
    let token = unsafe {
        objc2_foundation::NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(objc2_app_kit::NSWindowDidResizeNotification),
            Some(&window),
            None,
            &observer,
        )
    };
    let (width, number) = (window.frame().size.width, window.windowNumber());
    // The divider between source and preview, which the user may have moved before.
    let divider = e
        .source()
        .enclosingScrollView()
        .map(|s| s.convertPoint_toView(objc2_foundation::NSPoint::new(s.frame().size.width + 0.5, 300.0), None))
        .unwrap_or_default();
    let event = move |kind: objc2_app_kit::NSEventType, at: objc2_foundation::NSPoint| {
        objc2_app_kit::NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            kind,
            at,
            objc2_app_kit::NSEventModifierFlags::empty(),
            objc2_foundation::NSProcessInfo::processInfo().systemUptime(),
            number,
            None,
            0,
            1,
            1.0,
        )
    };
    // The events come one by one from a timer that also runs while AppKit tracks the resize.
    let tick = std::rc::Rc::new(Cell::new(0u32));
    let timer_block = RcBlock::new(move |timer: std::ptr::NonNull<NSTimer>| {
        let t = tick.get() + 1;
        tick.set(t);
        use objc2_app_kit::NSEventType as Kind;
        let at = |x: f64, y: f64| objc2_foundation::NSPoint::new(x, y);
        let (kind, point) = match t {
            2 => (Kind::LeftMouseDown, divider),
            3..=5 => (
                Kind::LeftMouseDragged,
                at(divider.x - f64::from(t - 2) * 50.0, divider.y),
            ),
            6 => (Kind::LeftMouseUp, at(divider.x - 150.0, divider.y)),
            10 => (Kind::LeftMouseDown, at(width - 2.0, 2.0)),
            11..=15 => (Kind::LeftMouseDragged, at(width - 2.0 + f64::from(t - 10) * 40.0, 2.0)),
            16 => (Kind::LeftMouseUp, at(width + 198.0, 2.0)),
            24 => {
                let _ = &token;
                unsafe { timer.as_ref() }.invalidate();
                check(
                    "the source re-wraps while the window is resized",
                    steps.get() > 0 && lag.get() < 0.5,
                    &format!(
                        "{} live steps, text width behind by up to {:.0} pt",
                        steps.get(),
                        lag.get()
                    ),
                );
                sidebar_keeps_the_ratio(mtm);
                return;
            }
            _ => return,
        };
        let Some(event) = event(kind, point) else { return };
        if kind == Kind::LeftMouseDown {
            window.sendEvent(&event);
        } else {
            app.postEvent_atStart(&event, false);
        }
    });
    let timer = unsafe { NSTimer::timerWithTimeInterval_repeats_block(0.12, true, &timer_block) };
    unsafe {
        objc2_foundation::NSRunLoop::currentRunLoop().addTimer_forMode(&timer, objc2_foundation::NSRunLoopCommonModes)
    };
}

/// The widths of the source and the preview pane.
fn pane_widths(e: &EditorController) -> (f64, f64) {
    let width = |view: &objc2_app_kit::NSTextView| {
        view.enclosingScrollView()
            .and_then(|scroll| unsafe { scroll.superview() })
            .map_or(0.0, |pane| pane.frame().size.width)
    };
    (width(e.source()), width(e.preview()))
}

/// Whether a window opened now shows the sidebar, and on which tab.
fn new_window_sidebar(mtm: MainThreadMarker) -> Option<(bool, Option<SidebarTab>)> {
    let document = NSDocumentController::sharedDocumentController(mtm)
        .openUntitledDocumentAndDisplay_error(false)
        .ok()?;
    document.makeWindowControllers();
    let shown = document
        .downcast_ref::<MdDocument>()
        .and_then(|d| d.editor())
        .map(|e| (e.panes().sidebar_visible(), e.panes().sidebar_tab()));
    document.close();
    shown
}

/// Calls `then` once the panes have moved off `from` and stand still, which they do some time after the sidebar came or
/// went and report nothing about; panes that never do are a failed check.
fn after_the_panes_settle(mtm: MainThreadMarker, from: (f64, f64), name: &'static str, then: impl Fn() + 'static) {
    let last = Cell::new(from);
    let steady = move || {
        let Some(e) = editor(mtm) else { return false };
        if let Some(window) = e.window() {
            window.layoutIfNeeded();
        }
        let now = pane_widths(&e);
        now != from && now == last.replace(now)
    };
    wait(0.0, 5.0, 3, steady, move |settled| {
        if !settled {
            check(name, false, "waited 5 s for it");
        }
        then();
    });
}

/// The divider was moved off the middle above: the sidebar coming and going leaves that share to the source, and a new
/// window opens the sidebar as the last one left it.
fn sidebar_keeps_the_ratio(mtm: MainThreadMarker) {
    let Some(e) = editor(mtm) else { return };
    let measure = |e: &EditorController| {
        if let Some(window) = e.window() {
            window.layoutIfNeeded();
        }
        let (source, preview) = pane_widths(e);
        (source, preview, source / (source + preview).max(1.0))
    };
    let toggle = |e: &EditorController| perform(e, Action::ToggleSidebar.spec().selector, 0);
    let before = measure(&e);
    toggle(&e);
    let from = (before.0, before.1);
    after_the_panes_settle(mtm, from, "the panes to settle with the sidebar shown", move || {
        let Some(e) = editor(mtm) else { return };
        let shown = measure(&e);
        let opened_shown = new_window_sidebar(mtm);
        toggle(&e);
        let from = (shown.0, shown.1);
        after_the_panes_settle(mtm, from, "the panes to settle with the sidebar hidden", move || {
            let Some(e) = editor(mtm) else { return };
            let hidden = measure(&e);
            let near = |a: f64, b: f64| (a - b).abs() < 0.005;
            check(
                "the sidebar takes its room from source and preview alike",
                shown.0 + shown.1 < before.0 + before.1 - 100.0 && near(shown.2, before.2) && near(hidden.2, before.2),
                &format!(
                    "source and preview {:.0} + {:.0}, with the sidebar {:.0} + {:.0}, without it again {:.0} + {:.0}",
                    before.0, before.1, shown.0, shown.1, hidden.0, hidden.1
                ),
            );
            let opened_hidden = new_window_sidebar(mtm);
            check(
                "a new window opens the sidebar as the last one left it",
                opened_shown == Some((true, Some(SidebarTab::Outline)))
                    && opened_hidden.is_some_and(|(shown, _)| !shown),
                &format!("after showing {opened_shown:?}, after hiding {opened_hidden:?}"),
            );
            panes_toggled_while_scrolled_away(mtm, PANE_TOGGLE_ROUNDS);
        });
    });
}

const PANE_TOGGLE_ROUNDS: usize = 100;

/// When a pane stops moving, TextKit 2 scrolls the source back to its selection from a block it runs after layout, and
/// laying out again from that scroll frees the block: a crash here is the failure, so the scrolls stay off the selection.
fn panes_toggled_while_scrolled_away(mtm: MainThreadMarker, rounds: usize) {
    let Some(e) = editor(mtm) else {
        check("open document", false, "no editor window");
        finish();
        return;
    };
    if rounds == 0 {
        check(
            "a pane can open and close with the source scrolled off its selection",
            true,
            "",
        );
        finish();
        return;
    }
    let scroll = move |e: &EditorController, factor: usize| {
        let at = NSRange::new(rounds * factor % e.source_text().len().max(1), 0);
        e.source().scrollRangeToVisible(at);
        at
    };
    if let Some(window) = e.window() {
        window.makeFirstResponder(Some(e.source()));
    }
    e.source().setSelectedRange(scroll(&e, 977));
    e.toggle_preview();
    // Timed to land while the pane moves, which reports nothing along the way.
    after(0.03, move || {
        let Some(e) = editor(mtm) else { return };
        scroll(&e, 613);
        e.toggle_preview();
        after(0.03, move || {
            let Some(e) = editor(mtm) else { return };
            scroll(&e, 331);
            after(0.12, move || panes_toggled_while_scrolled_away(mtm, rounds - 1));
        });
    });
}
