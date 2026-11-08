use std::ops::Range;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, Sel};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSButton, NSControl, NSDocument, NSEventModifierFlags, NSImage, NSLayoutManager, NSMenu, NSMenuItem,
    NSTextContainer, NSTextInputClient, NSTextStorage, NSTextView, NSToolbarItem,
};
use objc2_foundation::{NSError, NSRange, NSString, NSUndoManager};

use crate::markdown::text::{Byte, U16, Utf16Map};
use crate::preview::map::Shown;

/// Reads a constant that AppKit, Foundation or WebKit exports.
macro_rules! constant {
    ($name:path) => {
        // SAFETY: the frameworks set their exported constants when they load, before any Rust code runs, and never
        // change them.
        unsafe { $name }
    };
}
pub(crate) use constant;

/// A size as AppKit's signed counts have it; no text or table comes near the difference.
pub fn to_isize(n: usize) -> isize {
    isize::try_from(n).unwrap_or(isize::MAX)
}

/// A count from AppKit as a size; a negative one, which says "none", counts nothing.
pub fn to_usize(n: isize) -> usize {
    usize::try_from(n).unwrap_or(0)
}

/// Upcasts any Objective-C object for heterogeneous containers such as attribute dictionaries.
pub fn obj<T: objc2::ClassType + 'static>(r: Retained<T>) -> Retained<objc2::runtime::AnyObject> {
    r.into()
}

/// How many arguments the method behind `selector` takes: its name has one colon for each.
fn arguments_of(selector: Sel) -> usize {
    selector.name().to_bytes().iter().filter(|&&byte| byte == b':').count()
}

/// Whether `target` has a method for `action` that takes one argument, the sender, as AppKit sends it.
fn answers(target: &AnyObject, action: Sel) -> bool {
    arguments_of(action) == 1 && target.class().responds_to(action)
}

/// Has `control` send `action` to `target`.
pub fn set_action(control: &NSControl, target: &AnyObject, action: Sel) {
    assert!(answers(target, action), "the target has no action {action:?}");
    // SAFETY: The target has a method for the action that takes the sender, checked above; AppKit holds the target as
    // a zeroing weak reference, so one that is gone gets nothing.
    unsafe {
        control.setTarget(Some(target));
        control.setAction(Some(action));
    }
}

/// A push button titled `title` that sends `action` to `target`.
pub fn button(mtm: MainThreadMarker, title: &str, target: &AnyObject, action: Sel) -> Retained<NSButton> {
    // SAFETY: Without a target and an action the button sends nothing until `set_action` gives it both.
    let button = unsafe { NSButton::buttonWithTitle_target_action(&ns(title), None, None, mtm) };
    set_action(&button, target, action);
    button
}

/// A push button showing `image` that sends `action` to `target`.
pub fn image_button(mtm: MainThreadMarker, image: &NSImage, target: &AnyObject, action: Sel) -> Retained<NSButton> {
    // SAFETY: Without a target and an action the button sends nothing until `set_action` gives it both.
    let button = unsafe { NSButton::buttonWithImage_target_action(image, None, None, mtm) };
    set_action(&button, target, action);
    button
}

/// Has `item` send its action to `target` instead of along the responder chain.
pub fn set_target(item: &NSMenuItem, target: &AnyObject) {
    let action = item.action();
    assert!(
        action.is_some_and(|action| answers(target, action)),
        "the target has no action {action:?}"
    );
    // SAFETY: The target has a method for the item's action that takes the sender, checked above; AppKit holds the
    // target as a zeroing weak reference, so one that is gone gets nothing.
    unsafe { item.setTarget(Some(target)) };
}

/// Gives `item` an object for its action to read, which the item retains.
pub fn set_represented(item: &NSMenuItem, object: &AnyObject) {
    // SAFETY: A menu item can represent an object of any type.
    unsafe { item.setRepresentedObject(Some(object)) };
}

/// Has `item` send `action` along the responder chain.
pub fn set_toolbar_action(item: &NSToolbarItem, action: Sel) {
    assert_eq!(arguments_of(action), 1, "{action:?} does not take the sender");
    // SAFETY: Along the responder chain AppKit sends the action only to an object that answers it, and the method
    // behind it takes the sender, checked above.
    unsafe { item.setAction(Some(action)) };
}

/// A completion handler for the system, which hands it an error or nil. A panic in `f` ends only `f`.
pub fn error_handler(f: impl Fn(Option<&NSError>) + 'static) -> RcBlock<dyn Fn(*mut NSError)> {
    RcBlock::new(move |error: *mut NSError| {
        crate::util::guarded((), || {
            // SAFETY: The system passes a completion handler a valid error or nil, alive for the length of the call.
            f(unsafe { error.as_ref() });
        });
    })
}

/// A completion handler for opening a document, which AppKit hands the document or nil, whether it was open already
/// and an error or nil. A panic in `f` ends only `f`.
pub fn open_handler(
    f: impl Fn(Option<Retained<NSDocument>>, bool, Option<Retained<NSError>>) + 'static,
) -> RcBlock<dyn Fn(*mut NSDocument, Bool, *mut NSError)> {
    RcBlock::new(move |document: *mut NSDocument, already: Bool, error: *mut NSError| {
        crate::util::guarded((), || {
            // SAFETY: AppKit passes a completion handler a valid document and error or nil, alive for the length of
            // the call.
            let (document, error) = unsafe { (Retained::retain(document), Retained::retain(error)) };
            f(document, already.as_bool(), error);
        });
    })
}

/// Sends `selector` to `target` after `delay` seconds, replacing a request for it that has not run yet. With
/// `common_modes` it also runs while a scroller is dragged or a menu is open.
pub fn perform_later(target: &objc2::runtime::NSObject, selector: Sel, delay: f64, common_modes: bool) {
    use objc2_foundation::NSObjectNSDelayedPerforming;
    assert_eq!(arguments_of(selector), 0, "{selector:?} would get nil as its argument");
    // SAFETY: A selector without arguments, checked above; the run loop retains the target until the call.
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

pub fn to_nsrange(r: Range<U16>) -> NSRange {
    NSRange::new(r.start.get(), r.end.get().saturating_sub(r.start.get()))
}

pub fn from_nsrange(r: NSRange) -> Range<U16> {
    U16::new(r.location)..U16::new(r.location + r.length)
}

/// A range of the preview view's text.
pub fn shown_to_ns(r: Range<Shown>) -> NSRange {
    NSRange::new(r.start.get(), r.end.get().saturating_sub(r.start.get()))
}

/// A range of the preview view's text.
pub fn shown_from_ns(r: NSRange) -> Range<Shown> {
    Shown::new(r.location)..Shown::new(r.location + r.length)
}

pub fn byte_range_to_ns(map: &Utf16Map, r: &Range<Byte>) -> NSRange {
    to_nsrange(map.to_u16(r.start)..map.to_u16(r.end))
}

/// The modifiers and the key of a key equivalent; the key can be `+` itself, the separator.
pub fn split_shortcut(key: &str) -> (&str, &str) {
    match key.strip_suffix('+') {
        Some(modifiers) => (modifiers.trim_end_matches('+'), "+"),
        None => key.rsplit_once('+').unwrap_or(("", key)),
    }
}

/// Key equivalent with modifiers: "cmd", "shift", "alt", "ctrl" joined by '+', e.g. "alt+cmd+p".
pub fn item(mtm: MainThreadMarker, title: &str, action: Option<Sel>, key: &str) -> Retained<NSMenuItem> {
    let (mods, equivalent) = split_shortcut(key);
    let mut modifiers = NSEventModifierFlags::empty();
    for m in mods.split('+') {
        modifiers |= match m {
            "cmd" => NSEventModifierFlags::Command,
            "shift" => NSEventModifierFlags::Shift,
            "alt" => NSEventModifierFlags::Option,
            "ctrl" => NSEventModifierFlags::Control,
            _ => NSEventModifierFlags::empty(),
        };
    }
    assert!(
        action.is_none_or(|action| arguments_of(action) <= 1),
        "{action:?} takes more than the sender"
    );
    // SAFETY: With at most one argument, checked above, the action takes nothing but the sender AppKit passes.
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
    // SAFETY: A text view keeps its text system alive, those made in `views.rs` through their `owner`.
    unsafe { view.textStorage() }.expect("text view without storage")
}

/// A text view built for TextKit 1, whose layout manager can be asked for without changing it. Asking a TextKit 2 view
/// for one switches it to TextKit 1 for good, so the source view is no such view; `editor::geometry` serves both.
pub trait TextKitOne {
    /// The view as AppKit knows it.
    fn text_view(&self) -> &NSTextView;
}

/// The layout manager of a view built for TextKit 1.
pub fn layout_manager(view: &impl TextKitOne) -> Option<Retained<NSLayoutManager>> {
    // SAFETY: A text view keeps its text system alive, those made in `views.rs` through their `owner`.
    unsafe { view.text_view().layoutManager() }
}

pub fn text_container(view: &NSTextView) -> Option<Retained<NSTextContainer>> {
    // SAFETY: A text view keeps its text system alive, those made in `views.rs` through their `owner`.
    unsafe { view.textContainer() }
}

/// Runs `f`, then `end`, also when `f` panics, as the main thread survives that.
fn with_end<R>(end: impl FnOnce(), f: impl FnOnce() -> R) -> R {
    struct End<E: FnOnce()>(Option<E>);
    impl<E: FnOnce()> Drop for End<E> {
        fn drop(&mut self) {
            if let Some(end) = self.0.take() {
                end();
            }
        }
    }
    let _end = End(Some(end));
    f()
}

/// The changes `f` makes to `storage`, as one. A storage left in editing tells nobody of any change from then on.
pub fn editing<R>(storage: &NSTextStorage, f: impl FnOnce() -> R) -> R {
    storage.beginEditing();
    with_end(|| storage.endEditing(), f)
}

/// What `f` does, which undoing is not to know of. A manager left disabled would leave every later change without an undo.
pub fn without_undo<R>(undo: Option<&NSUndoManager>, f: impl FnOnce() -> R) -> R {
    if let Some(undo) = undo {
        undo.disableUndoRegistration();
    }
    with_end(
        || {
            if let Some(undo) = undo {
                undo.enableUndoRegistration();
            }
        },
        f,
    )
}

/// Replaces `range` like typing would.
pub fn insert_text(view: &NSTextView, text: &NSString, range: NSRange) {
    // SAFETY: The inserted object is an NSString, one of the two types this method accepts.
    unsafe { view.insertText_replacementRange(text, range) };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shortcut_splits_into_modifiers_and_key() {
        assert_eq!(split_shortcut(""), ("", ""));
        assert_eq!(split_shortcut("cmd+b"), ("cmd", "b"));
        assert_eq!(split_shortcut("alt+cmd+1"), ("alt+cmd", "1"));
        assert_eq!(split_shortcut("cmd++"), ("cmd", "+"));
    }

    #[test]
    fn a_storage_ends_its_editing_when_what_edits_it_panics() {
        let storage = NSTextStorage::new();
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            editing(&storage, || {
                storage.replaceCharactersInRange_withString(NSRange::new(0, 0), &NSString::from_str("a"));
                assert!(
                    !storage.editedMask().is_empty(),
                    "the change waits for the end of the editing"
                );
                panic!("in the editing");
            });
        }));
        assert!(panicked.is_err());
        assert!(storage.editedMask().is_empty(), "the change went out");
    }

    #[test]
    fn a_selector_has_one_argument_for_each_colon() {
        assert_eq!(arguments_of(objc2::sel!(a)), 0);
        assert_eq!(arguments_of(objc2::sel!(a:)), 1);
        assert_eq!(arguments_of(objc2::sel!(a:b:)), 2);
    }

    #[test]
    #[should_panic(expected = "would get nil")]
    fn a_selector_that_expects_an_argument_is_not_performed_later() {
        perform_later(&objc2::runtime::NSObject::new(), objc2::sel!(a:), 0.0, false);
    }
}
