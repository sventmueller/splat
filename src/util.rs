use std::ops::Range;

use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSEventModifierFlags, NSLayoutManager, NSMenu, NSMenuItem, NSTextContainer, NSTextStorage, NSTextView,
};
use objc2_foundation::{NSRange, NSString};

use crate::markdown::text::Utf16Map;

/// Reads a constant that AppKit, Foundation or WebKit exports.
macro_rules! constant {
    ($name:path) => {
        // SAFETY: the frameworks set their exported constants when they load, before any Rust code runs, and never
        // change them.
        unsafe { $name }
    };
}
pub(crate) use constant;

/// Upcasts any Objective-C object for heterogeneous containers such as attribute dictionaries.
pub fn obj<T: objc2::Message>(r: Retained<T>) -> Retained<objc2::runtime::AnyObject> {
    // SAFETY: Any Objective-C object can be viewed as AnyObject.
    unsafe { Retained::cast_unchecked(r) }
}

/// Sends `selector` to `target` after `delay` seconds, replacing a request for it that has not run yet. With
/// `common_modes` it also runs while a scroller is dragged or a menu is open.
pub fn perform_later(target: &objc2::runtime::NSObject, selector: Sel, delay: f64, common_modes: bool) {
    use objc2_foundation::NSObjectNSDelayedPerforming;
    // SAFETY: The selectors passed here take no argument; the run loop retains the target until the call.
    unsafe {
        objc2::runtime::NSObject::cancelPreviousPerformRequestsWithTarget_selector_object(target, selector, None);
        if common_modes {
            let modes = objc2_foundation::NSArray::from_slice(&[objc2_foundation::NSRunLoopCommonModes]);
            target.performSelector_withObject_afterDelay_inModes(selector, None, delay, &modes);
        } else {
            target.performSelector_withObject_afterDelay(selector, None, delay);
        }
    }
}

/// The current local time in `format`, an NSDateFormatter pattern read with fixed English conventions.
pub fn now(format: &str) -> String {
    let formatter = objc2_foundation::NSDateFormatter::new();
    formatter.setLocale(Some(&objc2_foundation::NSLocale::localeWithLocaleIdentifier(&ns(
        "en_US_POSIX",
    ))));
    formatter.setDateFormat(Some(&ns(format)));
    formatter.stringFromDate(&objc2_foundation::NSDate::now()).to_string()
}

pub fn ns(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

pub fn to_nsrange(r: Range<usize>) -> NSRange {
    NSRange::new(r.start, r.end.saturating_sub(r.start))
}

pub fn from_nsrange(r: NSRange) -> Range<usize> {
    r.location..r.location + r.length
}

pub fn byte_range_to_ns(map: &Utf16Map, r: &Range<usize>) -> NSRange {
    to_nsrange(map.to_u16(r.start)..map.to_u16(r.end))
}

/// Key equivalent with modifiers: "cmd", "shift", "alt", "ctrl" joined by '+', e.g. "alt+cmd+p".
pub fn item(mtm: MainThreadMarker, title: &str, action: Option<Sel>, key: &str) -> Retained<NSMenuItem> {
    let mut modifiers = NSEventModifierFlags::empty();
    let mut equivalent = "";
    if !key.is_empty() {
        let parts: Vec<&str> = key.split('+').collect();
        let (last, mods) = parts.split_last().unwrap();
        equivalent = last;
        for m in mods {
            modifiers |= match *m {
                "cmd" => NSEventModifierFlags::Command,
                "shift" => NSEventModifierFlags::Shift,
                "alt" => NSEventModifierFlags::Option,
                "ctrl" => NSEventModifierFlags::Control,
                _ => NSEventModifierFlags::empty(),
            };
        }
    }
    // SAFETY: Menu actions are command selectors that take the sender as their only argument.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &ns(title), action, &ns(equivalent))
    };
    if !equivalent.is_empty() {
        item.setKeyEquivalentModifierMask(modifiers);
    }
    item
}

pub fn tagged(mtm: MainThreadMarker, title: &str, action: Sel, key: &str, tag: isize) -> Retained<NSMenuItem> {
    let i = item(mtm, title, Some(action), key);
    i.setTag(tag);
    i
}

pub fn submenu(
    mtm: MainThreadMarker,
    title: &str,
    items: Vec<Retained<NSMenuItem>>,
) -> (Retained<NSMenuItem>, Retained<NSMenu>) {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &ns(title));
    for i in &items {
        menu.addItem(i);
    }
    let parent = item(mtm, title, None, "");
    parent.setSubmenu(Some(&menu));
    (parent, menu)
}

pub fn separator(mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    NSMenuItem::separatorItem(mtm)
}

pub fn storage(view: &NSTextView) -> Retained<NSTextStorage> {
    // SAFETY: objc2 marks this unsafe because the result may be nil before a text system is attached.
    unsafe { view.textStorage() }.expect("text view without storage")
}

pub fn layout_manager(view: &NSTextView) -> Option<Retained<NSLayoutManager>> {
    // SAFETY: objc2 marks this unsafe because the result may be nil before a text system is attached.
    unsafe { view.layoutManager() }
}

pub fn text_container(view: &NSTextView) -> Option<Retained<NSTextContainer>> {
    // SAFETY: objc2 marks this unsafe because the result may be nil before a text system is attached.
    unsafe { view.textContainer() }
}

#[cfg(test)]
mod exception_tests {
    /// Without objc2's `catch-all`, an Objective-C exception ends the process before the panic hook can save text.
    #[test]
    #[should_panic]
    fn objective_c_exceptions_become_panics() {
        let array = objc2_foundation::NSArray::<objc2_foundation::NSString>::new();
        let _ = array.objectAtIndex(3);
    }
}
