//! The recovery after a crash and the quick reference.

use std::cell::Cell;

use block2::RcBlock;
use objc2::{MainThreadOnly, sel};
use objc2_app_kit::{NSAlertSecondButtonReturn, NSApplication};
use objc2_foundation::{NSRunLoop, NSRunLoopCommonModes, NSTimer};

use super::super::{Step, applied, check, source, type_in_source, when};
use super::{documents, remember, undone};

thread_local! {
    /// Whether an alert ran modally while `offer_recovered` was called.
    static MODAL_SEEN: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn app_steps() -> Vec<Step> {
    vec![
        applied(|c| {
            remember(c);
            type_in_source(&c.e, "Vor dem Absturz");
        }),
        applied(|c| {
            let files = crate::editor::emergency_save();
            let copy = files.iter().find_map(|f| {
                let text = std::fs::read_to_string(f).ok()?;
                (text == source(&c.e)).then_some(())
            });
            check(
                "a crash leaves a copy of every document with unsaved text",
                copy.is_some(),
                &format!("{files:?}"),
            );
        }),
        undone("the text typed for the crash goes again".into()),
        applied(|c| {
            let mtm = c.e.mtm();
            let says_so = || {
                MODAL_SEEN.with(|m| m.set(false));
                let stop = RcBlock::new(move |_: std::ptr::NonNull<NSTimer>| {
                    let app = NSApplication::sharedApplication(mtm);
                    MODAL_SEEN.with(|m| m.set(app.modalWindow().is_some()));
                    app.stopModalWithCode(NSAlertSecondButtonReturn);
                });
                let timer = unsafe { NSTimer::timerWithTimeInterval_repeats_block(0.5, false, &stop) };
                unsafe { NSRunLoop::currentRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
                crate::editor::offer_recovered(mtm);
                MODAL_SEEN.with(Cell::get)
            };
            let (first, second) = (says_so(), says_so());
            check(
                "the next start says once where the copies are",
                first && !second,
                &format!("first {first}, second {second}"),
            );
            let count = documents(mtm).len();
            let _ = unsafe {
                NSApplication::sharedApplication(mtm).sendAction_to_from(sel!(mdQuickReference:), None, None)
            };
            check(
                "the quick reference opens as a document",
                documents(mtm).len() == count + 1,
                "",
            );
        }),
        when(
            "the quick reference to open",
            |c| {
                documents(c.e.mtm())
                    .iter()
                    .any(|d| d.displayName().to_string() == "Markdown-Kurzreferenz")
            },
            |c| {
                let reference = documents(c.e.mtm())
                    .into_iter()
                    .find(|d| d.displayName().to_string() == "Markdown-Kurzreferenz");
                check(
                    "the quick reference is in the interface language",
                    reference
                        .as_ref()
                        .is_some_and(|d| d.text().starts_with("# Markdown-Kurzreferenz")),
                    "",
                );
                if let Some(d) = reference {
                    d.close();
                }
                if let Some(window) = c.e.window() {
                    window.makeKeyAndOrderFront(None);
                }
            },
        ),
    ]
}
