//! Typing through saves, with keys that take the way real ones do: into the app's event queue, through `NSApplication`, as
//! AppKit's autosave sees them. What is typed must stay, whatever saves in between, and text no file holds must show as
//! unsaved. `--by-hand` leaves the keys to a person at the keyboard.

use std::cell::{Cell, RefCell};
use std::ffi::{c_ulong, c_void};
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSEvent, NSEventMask, NSEventModifierFlags, NSTextInputClient, NSWindow};
use objc2_foundation::NSRange;

use super::super::EditorController;
use super::{after, check, check_in_front, editor, finish};

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventCreateKeyboardEvent(source: *const c_void, key: u16, key_down: bool) -> *mut c_void;
    fn CGEventKeyboardSetUnicodeString(event: *mut c_void, length: c_ulong, string: *const u16);
    fn CGEventSetFlags(event: *mut c_void, flags: u64);
    fn CGEventPostToPid(pid: i32, event: *mut c_void);
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(object: *const c_void);
}

const COMMAND: u64 = 1 << 20;
const S_KEY: u16 = 1;

/// A key press for this process alone, which no other app can get whatever is in front.
fn post_key(key: u16, flags: u64, character: Option<u16>) {
    for down in [true, false] {
        // SAFETY: Creating an event takes no source and plain values; it is released right after being posted.
        unsafe {
            let event = CGEventCreateKeyboardEvent(std::ptr::null(), key, down);
            if event.is_null() {
                continue;
            }
            CGEventSetFlags(event, flags);
            if let Some(unit) = character {
                CGEventKeyboardSetUnicodeString(event, 1, &raw const unit);
            }
            CGEventPostToPid(i32::try_from(std::process::id()).unwrap_or(i32::MAX), event);
            CFRelease(event);
        }
    }
}

fn in_front(mtm: MainThreadMarker) -> bool {
    let app = NSApplication::sharedApplication(mtm);
    app.isActive() && app.keyWindow().is_some()
}

fn ending(text: &str) -> &str {
    &text[text.floor_char_boundary(text.len().saturating_sub(12))..]
}

/// About three autosaves.
const KEYS: usize = 125;
const SECONDS_PER_KEY: f64 = 0.4;
/// Halfway, a save by its menu command with typing right behind it.
const SAVE_AT_KEY: usize = 60;
/// How long a person may take to answer `--by-hand`.
const BY_HAND_TICKS: usize = KEYS;

struct Run {
    e: Retained<EditorController>,
    path: String,
    by_hand: bool,
    /// The text so far: what it was, and every key the app saw since.
    expected: Rc<RefCell<String>>,
    other_keys: Rc<Cell<usize>>,
    /// How often the app saw a Command-S that has to start a save: AppKit disables Save without a key window and spends
    /// the key on ending a composition, and a text the file already holds has nothing to write.
    saves_asked: Rc<Cell<usize>>,
    save_posted: Cell<bool>,
    /// The keys are over, and a person who types on must not change what the last checks compare.
    over: Rc<Cell<bool>>,
    last_file: RefCell<Vec<u8>>,
    autosaves: Cell<usize>,
    lost: RefCell<Vec<String>>,
    unmarked: Cell<usize>,
    /// Kept for the length of the run.
    _monitor: Option<Retained<AnyObject>>,
}

impl Run {
    fn file(&self) -> Vec<u8> {
        std::fs::read(&self.path).unwrap_or_default()
    }

    /// What holds after every key.
    fn look(&self, tick: usize) {
        let text = self.e.exact_text();
        let expected = self.expected.borrow().clone();
        if self.other_keys.get() == 0 && text != expected {
            let mut lost = self.lost.borrow_mut();
            if lost.len() < 5 {
                lost.push(format!(
                    "at tick {tick}: {} bytes instead of {}, ending {:?} instead of {:?}",
                    text.len(),
                    expected.len(),
                    ending(&text),
                    ending(&expected),
                ));
            }
            *self.expected.borrow_mut() = text.clone();
        }
        let file = self.file();
        if file != *self.last_file.borrow() {
            self.autosaves.set(self.autosaves.get() + 1);
            *self.last_file.borrow_mut() = file.clone();
        }
        if file != text.as_bytes() && self.e.document().is_some_and(|d| !d.isDocumentEdited()) {
            self.unmarked.set(self.unmarked.get() + 1);
        }
    }

    fn tick(self: Rc<Self>, n: usize) {
        self.look(n);
        let ticks = if self.by_hand { BY_HAND_TICKS } else { KEYS };
        if n >= ticks {
            self.ended();
            return;
        }
        if !self.by_hand {
            let letter = u16::from(b'a') + u16::try_from(n % 26).unwrap_or(0);
            if n >= SAVE_AT_KEY && !self.save_posted.get() && in_front(self.e.mtm()) {
                self.save_posted.set(true);
                // A letter first: an autosave a moment ago leaves nothing for the save to write.
                post_key(0, 0, Some(letter));
                post_key(S_KEY, COMMAND, None);
            }
            post_key(0, 0, Some(letter));
        }
        // Whoever watches a run for a stall sees that it goes on.
        if n.is_multiple_of(25) {
            eprintln!(
                "SELFTEST NOTE {n} of {ticks} keys, {} writes of the file",
                self.autosaves.get()
            );
        }
        after(SECONDS_PER_KEY, move || self.clone().tick(n + 1));
    }

    fn ended(self: Rc<Self>) {
        self.over.set(true);
        if self.by_hand {
            eprintln!("SELFTEST NOTE stop typing now: keys count for nothing from here on");
        }
        eprintln!(
            "SELFTEST NOTE {} bytes at the end, {} writes of the file, Command-S on unsaved text seen {} times",
            self.e.exact_text().len(),
            self.autosaves.get(),
            self.saves_asked.get()
        );
        // The look at the files comes every two seconds, and it is what used to take the text away.
        after(3.0, move || {
            self.look(usize::MAX);
            let lost = self.lost.borrow().clone();
            check(
                "what is typed stays through every save",
                lost.is_empty(),
                &lost.join("; "),
            );
            check(
                "text no file holds shows as unsaved after every key",
                self.unmarked.get() == 0,
                &format!("{} times not", self.unmarked.get()),
            );
            let saves_begun = crate::document::SAVES_ASKED_FOR.load(std::sync::atomic::Ordering::Relaxed);
            check_in_front(
                self.e.mtm(),
                "every Command-S on unsaved text starts a save",
                saves_begun >= self.saves_asked.get() && (self.by_hand || self.saves_asked.get() > 0),
                &format!("{saves_begun} saves began for {} keys", self.saves_asked.get()),
            );
            check(
                "the file was written while typing went on",
                self.autosaves.get() > 0,
                &format!(
                    "the file never changed, with Command-S on unsaved text seen {} times",
                    self.saves_asked.get()
                ),
            );
            let typed = self.e.exact_text();
            let undo = self.e.document().and_then(|d| d.undoManager());
            if let Some(undo) = &undo {
                undo.undo();
            }
            let undone = self.e.exact_text();
            check(
                "what is typed through saves can still be undone",
                undone.len() < typed.len() && typed.starts_with(&undone),
                &format!("{} bytes of {}", undone.len(), typed.len()),
            );
            if let Some(undo) = &undo {
                undo.redo();
            }
            check(
                "what was undone comes back",
                self.e.exact_text() == typed,
                &format!("{} bytes of {}", self.e.exact_text().len(), typed.len()),
            );
            if let Some(document) = self.e.document() {
                // SAFETY: NSDocument's action method, called on the main thread without a sender.
                unsafe { document.saveDocument(None) };
            }
            let this = self.clone();
            after(3.0, move || {
                check(
                    "a last save writes all that was typed",
                    this.file() == this.e.exact_text().into_bytes(),
                    &format!(
                        "{} bytes on file, {} in the view",
                        this.file().len(),
                        this.e.exact_text().len()
                    ),
                );
                finish();
            });
        });
    }
}

fn start(e: Retained<EditorController>, window: Retained<NSWindow>) {
    let by_hand = std::env::var_os("SPLAT_TYPING_BY_HAND").is_some();
    window.makeKeyAndOrderFront(None);
    window.makeFirstResponder(Some(e.source()));
    let end = e.source().string().length();
    e.source().setSelectedRange(NSRange::new(end, 0));
    let path = e
        .document()
        .and_then(|d| d.fileURL()?.path())
        .map(|p| p.to_string())
        .unwrap_or_default();
    let expected = Rc::new(RefCell::new(e.exact_text()));
    let other_keys = Rc::new(Cell::new(0));
    let saves_asked = Rc::new(Cell::new(0));
    let over = Rc::new(Cell::new(false));
    let (seen, others, asked, ended) = (expected.clone(), other_keys.clone(), saves_asked.clone(), over.clone());
    let (asked_of, asked_at) = (e.clone(), path.clone());
    let block = RcBlock::new(move |event: NonNull<NSEvent>| {
        // SAFETY: AppKit passes a valid event for the length of the call.
        let key = unsafe { event.as_ref() };
        if ended.get() {
            return std::ptr::null_mut();
        }
        let command = key
            .modifierFlags()
            .intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control);
        if !command && let Some(characters) = key.characters() {
            let characters = characters.to_string();
            // A dead key has no characters of its own, and what it becomes depends on the next key.
            if !characters.is_empty()
                && characters
                    .chars()
                    .all(|c| !c.is_control() && !('\u{F700}'..='\u{F8FF}').contains(&c))
            {
                seen.borrow_mut().push_str(&characters);
            } else {
                others.set(others.get() + 1);
            }
        }
        if command
            && key
                .charactersIgnoringModifiers()
                .is_some_and(|c| c.to_string().eq_ignore_ascii_case("s"))
        {
            let other_modifiers =
                NSEventModifierFlags::Control | NSEventModifierFlags::Option | NSEventModifierFlags::Shift;
            let flags = key.modifierFlags();
            let edited = asked_of.document().is_some_and(|d| d.isDocumentEdited());
            let front = in_front(asked_of.mtm());
            let composing = asked_of.source().hasMarkedText();
            if flags.contains(NSEventModifierFlags::Command)
                && !flags.intersects(other_modifiers)
                && edited
                && front
                && !composing
            {
                asked.set(asked.get() + 1);
            }
            // What the next look at a save that did not happen needs: a person's key cannot be sent again.
            eprintln!(
                "SELFTEST NOTE a key with S: flags {:#x}, characters {:?}, repeat {}, edited {edited}, in front {front}, composing {composing}, {} bytes in the view, {} on file",
                flags.0,
                key.charactersIgnoringModifiers().map(|c| c.to_string()),
                key.isARepeat(),
                asked_of.exact_text().len(),
                std::fs::metadata(&asked_at).map_or(0, |file| file.len()),
            );
        }
        event.as_ptr()
    });
    // SAFETY: The handler stays on the main thread, and the run keeps the monitor for as long as it lasts.
    let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block) };
    let last_file = RefCell::new(std::fs::read(&path).unwrap_or_default());
    if by_hand {
        eprintln!(
            "SELFTEST NOTE type letters into the window for {:.0} seconds, press Command-S once after some of them, type on at once, and go on until told to stop",
            BY_HAND_TICKS as f64 * SECONDS_PER_KEY
        );
    }
    let run = Rc::new(Run {
        e,
        path,
        by_hand,
        expected,
        other_keys,
        saves_asked,
        save_posted: Cell::new(false),
        over,
        last_file,
        autosaves: Cell::new(0),
        lost: RefCell::default(),
        unmarked: Cell::new(0),
        _monitor: monitor,
    });
    run.tick(0);
}

/// Keys reach the view only while the app is in front, which macOS decides; xtask asks for it from outside.
pub(super) fn run(mtm: MainThreadMarker) {
    run_when_in_front(mtm, 0);
}

fn run_when_in_front(mtm: MainThreadMarker, tries: u32) {
    let app = NSApplication::sharedApplication(mtm);
    let Some((e, window)) = editor(mtm).and_then(|e| Some((e.clone(), e.window()?))) else {
        check("the document opens", false, "no window");
        finish();
        return;
    };
    window.makeKeyAndOrderFront(None);
    if app.isActive() && window.isKeyWindow() {
        check("the app is in front for the keys", true, "");
        // What only the app in front does.
        super::controller::a_cancelled_composition_ends_the_edit_it_announced(mtm);
        window.makeKeyAndOrderFront(None);
        start(e, window);
    } else if tries < 200 {
        after(0.1, move || run_when_in_front(mtm, tries + 1));
    } else {
        check(
            "the app is in front for the keys",
            false,
            "another app stayed in front for 20 seconds",
        );
        finish();
    }
}
