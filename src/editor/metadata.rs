//! The form for a document's metadata, as a sheet: a row per field, fields added and removed, and what cannot be shown in
//! one line stays read-only.

use std::cell::RefCell;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBackingStoreType, NSBeep, NSBezelStyle, NSButton, NSImage, NSModalResponse,
    NSModalResponseCancel, NSModalResponseOK, NSPanel, NSScrollView, NSTextField, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use crate::i18n::t;
use crate::markdown::front_matter::Value;
use crate::util::{guarded, ns};

const FORM_WIDTH: f64 = 480.0;
const ROW_HEIGHT: f64 = 30.0;
const KEY_WIDTH: f64 = 130.0;
const MARGIN: f64 = 20.0;
const LIST_HEIGHT: f64 = 300.0;
const BUTTON_ROW: f64 = 52.0;

/// Fields that static site generators and Obsidian read as lists, so that a new one with commas becomes one.
const LIST_KEYS: [&str; 6] = ["tags", "aliases", "keywords", "categories", "authors", "cssclasses"];

/// A list as one line: its values with commas between them, those holding a comma or a quote in quotes.
pub(super) fn line_of(value: &Value) -> String {
    match value {
        Value::Text(text) | Value::Complex(text) => text.clone(),
        Value::List(items) => items
            .iter()
            .map(|item| {
                if item.contains(',') || item.contains('"') {
                    format!("\"{}\"", item.replace('"', "\"\""))
                } else {
                    item.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// The values of such a line.
fn items_of(line: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut item = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                item.push('"');
                chars.next();
            }
            '"' if item.trim().is_empty() || quoted => {
                quoted = !quoted;
                if quoted {
                    item.clear();
                }
            }
            ',' if !quoted => {
                items.push(item.trim().to_string());
                item.clear();
            }
            c => item.push(c),
        }
    }
    items.push(item.trim().to_string());
    items.retain(|item| !item.is_empty());
    items
}

/// What a row of the form comes to: a field keeps what it was, and a list that stays as it was has no change at all.
fn value_of(old: Option<&Value>, key: &str, line: &str) -> Value {
    match old {
        Some(Value::Complex(_)) => old.cloned().unwrap_or_else(|| Value::Text(line.to_string())),
        Some(Value::List(_)) => Value::List(items_of(line)),
        Some(Value::Text(_)) => Value::Text(line.to_string()),
        None if LIST_KEYS.contains(&key.to_ascii_lowercase().as_str()) => Value::List(items_of(line)),
        None => Value::Text(line.to_string()),
    }
}

/// The fields a form's rows ask for, or `None` if two rows name the same field.
pub(super) fn wanted_fields(rows: &[(String, String, Option<Value>)]) -> Option<Vec<(String, Value)>> {
    let mut wanted: Vec<(String, Value)> = Vec::with_capacity(rows.len());
    for (key, line, old) in rows {
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        if wanted.iter().any(|(other, _)| other == key) {
            return None;
        }
        wanted.push((key.to_string(), value_of(old.as_ref(), key, line)));
    }
    Some(wanted)
}

struct FormRow {
    id: isize,
    key: Retained<NSTextField>,
    value: Retained<NSTextField>,
    remove: Retained<NSButton>,
    old: Option<Value>,
}

pub struct FormIvars {
    rows: RefCell<Vec<FormRow>>,
    list: Retained<NSView>,
    scroll: Retained<NSScrollView>,
    next_id: std::cell::Cell<isize>,
    answer: RefCell<Option<Vec<(String, Value)>>>,
}

define_class!(
    /// The list's rows count from the top.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDFlippedView"]
    struct FlippedView;

    unsafe impl NSObjectProtocol for FlippedView {}

    impl FlippedView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

define_class!(
    #[unsafe(super(objc2::runtime::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDMetadataForm"]
    #[ivars = FormIvars]
    pub struct MetadataForm;

    unsafe impl NSObjectProtocol for MetadataForm {}

    impl MetadataForm {
        #[unsafe(method(formAddField:))]
        fn add_field(&self, _sender: Option<&AnyObject>) {
            guarded((), || {
                let key = self.add_row("", &Value::Text(String::new()), None);
                if let Some(window) = key.window() {
                    window.makeFirstResponder(Some(&*key));
                }
            });
        }

        #[unsafe(method(formRemoveField:))]
        fn remove_field(&self, sender: Option<&AnyObject>) {
            guarded((), || {
                let Some(button) = sender.and_then(|s| s.downcast_ref::<NSButton>()) else {
                    return;
                };
                let id = button.tag();
                let mut rows = self.ivars().rows.borrow_mut();
                if let Some(at) = rows.iter().position(|row| row.id == id) {
                    let row = rows.remove(at);
                    row.key.removeFromSuperview();
                    row.value.removeFromSuperview();
                    row.remove.removeFromSuperview();
                }
                drop(rows);
                self.lay_out();
            });
        }

        #[unsafe(method(formCommit:))]
        fn commit(&self, sender: Option<&AnyObject>) {
            guarded((), || {
                let rows: Vec<(String, String, Option<Value>)> = self
                    .ivars()
                    .rows
                    .borrow()
                    .iter()
                    .map(|row| {
                        (
                            row.key.stringValue().to_string(),
                            row.value.stringValue().to_string(),
                            row.old.clone(),
                        )
                    })
                    .collect();
                let Some(wanted) = wanted_fields(&rows) else {
                    NSBeep();
                    return;
                };
                *self.ivars().answer.borrow_mut() = Some(wanted);
                self.end(sender, NSModalResponseOK);
            });
        }

        #[unsafe(method(formCancel:))]
        fn cancel(&self, sender: Option<&AnyObject>) {
            guarded((), || self.end(sender, NSModalResponseCancel));
        }
    }
);

impl MetadataForm {
    fn end(&self, sender: Option<&AnyObject>, response: NSModalResponse) {
        let panel = sender
            .and_then(|s| s.downcast_ref::<NSView>())
            .and_then(|view| view.window());
        if let Some(panel) = panel
            && let Some(parent) = panel.sheetParent()
        {
            parent.endSheet_returnCode(&panel, response);
        }
    }

    fn add_row(&self, key: &str, value: &Value, old: Option<Value>) -> Retained<NSTextField> {
        let mtm = self.mtm();
        let ivars = self.ivars();
        let id = ivars.next_id.get();
        ivars.next_id.set(id + 1);
        let key_field = NSTextField::textFieldWithString(&ns(key), mtm);
        key_field.setPlaceholderString(Some(&ns(t("Feld"))));
        let value_field = NSTextField::textFieldWithString(&ns(&line_of(value)), mtm);
        value_field.setPlaceholderString(Some(&ns(t("Wert"))));
        if matches!(value, Value::Complex(_)) {
            value_field.setEditable(false);
            value_field.setEnabled(false);
            value_field.setToolTip(Some(&ns(t("Dieser Wert lässt sich nur im Quelltext bearbeiten."))));
            key_field.setEditable(false);
            key_field.setEnabled(false);
        } else if matches!(value, Value::List(_)) {
            value_field.setToolTip(Some(&ns(t("Mehrere Werte mit Kommas trennen"))));
        }
        let symbol = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &ns("minus.circle"),
            Some(&ns(t("Feld entfernen"))),
        );
        // SAFETY: The target is this form, which the sheet's completion handler keeps until the sheet has ended.
        let remove = unsafe {
            match &symbol {
                Some(image) => {
                    NSButton::buttonWithImage_target_action(image, Some(self), Some(sel!(formRemoveField:)), mtm)
                }
                None => NSButton::buttonWithTitle_target_action(
                    &NSString::from_str("−"),
                    Some(self),
                    Some(sel!(formRemoveField:)),
                    mtm,
                ),
            }
        };
        remove.setBordered(false);
        remove.setTag(id);
        remove.setToolTip(Some(&ns(t("Feld entfernen"))));
        for view in [&*key_field as &NSView, &*value_field, &*remove] {
            ivars.list.addSubview(view);
        }
        ivars.rows.borrow_mut().push(FormRow {
            id,
            key: key_field.clone(),
            value: value_field,
            remove,
            old,
        });
        self.lay_out();
        key_field
    }

    fn lay_out(&self) {
        let ivars = self.ivars();
        let rows = ivars.rows.borrow();
        let width = ivars.scroll.contentSize().width;
        ivars.list.setFrameSize(NSSize::new(
            width,
            (rows.len() as f64 * ROW_HEIGHT).max(ivars.scroll.contentSize().height),
        ));
        for (n, row) in rows.iter().enumerate() {
            let y = n as f64 * ROW_HEIGHT + 3.0;
            // A field draws its border a little beyond its frame, which the list's edge would cut.
            row.key
                .setFrame(NSRect::new(NSPoint::new(3.0, y), NSSize::new(KEY_WIDTH, 24.0)));
            row.value.setFrame(NSRect::new(
                NSPoint::new(KEY_WIDTH + 11.0, y),
                NSSize::new(width - KEY_WIDTH - 43.0, 24.0),
            ));
            row.remove.setFrame(NSRect::new(
                NSPoint::new(width - 24.0, y + 2.0),
                NSSize::new(20.0, 20.0),
            ));
        }
        for pair in rows.windows(2) {
            // SAFETY: All of them belong to the sheet, which keeps them alive.
            unsafe {
                pair[0].key.setNextKeyView(Some(&pair[0].value));
                pair[0].value.setNextKeyView(Some(&pair[1].key));
            }
        }
    }
}

fn button(
    mtm: MainThreadMarker,
    form: &MetadataForm,
    title: &str,
    action: objc2::runtime::Sel,
    key: &str,
    x: f64,
) -> Retained<NSButton> {
    // SAFETY: The target is the form, which the sheet's completion handler keeps until the sheet has ended.
    let b = unsafe { NSButton::buttonWithTitle_target_action(&ns(title), Some(form), Some(action), mtm) };
    b.setBezelStyle(NSBezelStyle::Push);
    b.setKeyEquivalent(&ns(key));
    b.sizeToFit();
    let width = b.frame().size.width;
    let x = if x < 0.0 { FORM_WIDTH + x - width } else { x };
    b.setFrameOrigin(NSPoint::new(x, 14.0));
    b
}

/// Shows the form for `fields` on `window`; `done` receives the fields as they stand when the user commits.
pub(super) fn show(window: &NSWindow, fields: &[(String, Value)], done: impl Fn(Vec<(String, Value)>) + 'static) {
    let mtm = window.mtm();
    let height = LIST_HEIGHT + BUTTON_ROW + 2.0 * MARGIN;
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(FORM_WIDTH, height));
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        frame,
        NSWindowStyleMask::Titled,
        NSBackingStoreType::Buffered,
        false,
    );
    // SAFETY: Switching it off keeps AppKit from releasing the panel behind Rust's back.
    unsafe { panel.setReleasedWhenClosed(false) };
    let content = NSView::initWithFrame(NSView::alloc(mtm), frame);
    panel.setContentView(Some(&content));

    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        NSRect::new(
            NSPoint::new(MARGIN, BUTTON_ROW),
            NSSize::new(FORM_WIDTH - 2.0 * MARGIN, LIST_HEIGHT),
        ),
    );
    scroll.setHasVerticalScroller(true);
    scroll.setAutohidesScrollers(true);
    scroll.setDrawsBackground(false);
    scroll.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    // SAFETY: NSView's designated initializer.
    let list: Retained<FlippedView> = unsafe {
        msg_send![super(FlippedView::alloc(mtm).set_ivars(())), initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), scroll.contentSize())]
    };
    scroll.setDocumentView(Some(&list));
    content.addSubview(&scroll);
    let title = NSTextField::labelWithString(&ns(t("Metadaten")), mtm);
    title.setFont(Some(&objc2_app_kit::NSFont::boldSystemFontOfSize(13.0)));
    title.setFrame(NSRect::new(
        NSPoint::new(MARGIN, height - MARGIN - 8.0),
        NSSize::new(FORM_WIDTH - 2.0 * MARGIN, 20.0),
    ));
    content.addSubview(&title);

    let form: Retained<MetadataForm> = {
        let this = MetadataForm::alloc(mtm).set_ivars(FormIvars {
            rows: RefCell::new(Vec::new()),
            list: Retained::into_super(list),
            scroll,
            next_id: std::cell::Cell::new(0),
            answer: RefCell::new(None),
        });
        // SAFETY: NSObject's initializer, called once on the newly allocated object.
        unsafe { msg_send![super(this), init] }
    };
    for (key, value) in fields {
        form.add_row(key, value, Some(value.clone()));
    }
    content.addSubview(&button(
        mtm,
        &form,
        t("Feld hinzufügen"),
        sel!(formAddField:),
        "",
        MARGIN,
    ));
    let commit = button(mtm, &form, t("Übernehmen"), sel!(formCommit:), "\r", -MARGIN);
    let commit_x = commit.frame().origin.x;
    content.addSubview(&commit);
    let cancel = button(mtm, &form, t("Abbrechen"), sel!(formCancel:), "\u{1b}", -MARGIN);
    let cancel_x = commit_x - cancel.frame().size.width - 8.0;
    cancel.setFrameOrigin(NSPoint::new(cancel_x, cancel.frame().origin.y));
    content.addSubview(&cancel);

    let handler = RcBlock::new(move |response: NSModalResponse| {
        guarded((), || {
            if response == NSModalResponseOK
                && let Some(wanted) = form.ivars().answer.borrow_mut().take()
            {
                done(wanted);
            }
        });
    });
    window.beginSheet_completionHandler(&panel, Some(&handler));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_reads_back_from_its_line() {
        let list = Value::List(vec!["a".to_string(), "b, c".to_string(), "d\"e".to_string()]);
        let line = line_of(&list);
        assert_eq!(line, "a, \"b, c\", \"d\"\"e\"");
        assert_eq!(value_of(Some(&list), "tags", &line), list);
        assert_eq!(items_of(" a ,, b "), ["a", "b"]);
        assert_eq!(items_of(""), Vec::<String>::new());
    }

    #[test]
    fn a_row_stays_the_kind_of_field_it_was() {
        let text = Value::Text("alt".to_string());
        assert_eq!(value_of(Some(&text), "title", "a, b"), Value::Text("a, b".to_string()));
        let complex = Value::Complex("x:\n  y: 1".to_string());
        assert_eq!(value_of(Some(&complex), "x", "was anderes"), complex);
    }

    #[test]
    fn a_new_field_with_a_name_for_lists_becomes_one() {
        assert_eq!(
            value_of(None, "Tags", "a, b"),
            Value::List(vec!["a".to_string(), "b".to_string()])
        );
        assert_eq!(value_of(None, "title", "a, b"), Value::Text("a, b".to_string()));
    }

    #[test]
    fn a_row_without_a_name_is_gone_and_two_of_a_name_are_refused() {
        let row = |key: &str, line: &str| (key.to_string(), line.to_string(), None);
        assert_eq!(
            wanted_fields(&[row("a", "1"), row(" ", "2"), row(" b ", "3")]),
            Some(vec![
                ("a".to_string(), Value::Text("1".to_string())),
                ("b".to_string(), Value::Text("3".to_string()))
            ])
        );
        assert_eq!(wanted_fields(&[row("a", "1"), row("a", "2")]), None);
    }
}
