//! How the window follows a resize, measured while AppKit's own tracking runs.

use std::cell::Cell;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSRange, NSTimer};

use super::super::EditorController;
use super::{after, check, editor, finish};

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
            let actual = unsafe { source.textContainer() }.map_or(0.0, |c| c.size().width);
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
                panes_toggled_while_scrolled_away(mtm, PANE_TOGGLE_ROUNDS);
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
