use block2::RcBlock;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSModalResponse, NSModalResponseOK, NSOpenPanel, NSTextField, NSView, NSWindow,
};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSURL};
use objc2_uniform_type_identifiers::UTType;

use crate::i18n::t;
use crate::util::ns;

const FORM_WIDTH: f64 = 320.0;
const LABEL_HEIGHT: f64 = 17.0;
const FIELD_HEIGHT: f64 = 24.0;
const LABEL_INDENT: f64 = 4.0;

fn field(mtm: MainThreadMarker, value: &str, placeholder: &str) -> Retained<NSTextField> {
    let f = NSTextField::textFieldWithString(&ns(value), mtm);
    f.setPlaceholderString(Some(&ns(placeholder)));
    f
}

/// Labels above full-width fields, placed by frame: in a stack view the fields shrank to their placeholder, and
/// NSAlert then centred the narrow column.
fn form(mtm: MainThreadMarker, rows: &[(&str, &Retained<NSTextField>)]) -> Retained<NSView> {
    let row = LABEL_HEIGHT + 4.0 + FIELD_HEIGHT;
    let gap = 12.0;
    let height = rows.len() as f64 * row + rows.len().saturating_sub(1) as f64 * gap;
    let view = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(FORM_WIDTH, height)),
    );
    let mut top = height;
    for (label, f) in rows {
        let l = NSTextField::labelWithString(&ns(label), mtm);
        // Indented so the label text lines up with the text inside the field and the alert title.
        l.setFrame(NSRect::new(
            NSPoint::new(LABEL_INDENT, top - LABEL_HEIGHT),
            NSSize::new(FORM_WIDTH - LABEL_INDENT, LABEL_HEIGHT),
        ));
        view.addSubview(&l);
        f.setFrame(NSRect::new(
            NSPoint::new(0.0, top - row),
            NSSize::new(FORM_WIDTH, FIELD_HEIGHT),
        ));
        view.addSubview(f);
        top -= row + gap;
    }
    for pair in rows.windows(2) {
        // SAFETY: Both views belong to the dialog's window, which keeps them alive.
        unsafe { pair[0].1.setNextKeyView(Some(pair[1].1)) };
    }
    view
}

fn alert(mtm: MainThreadMarker, title: &str, info: &str, accessory: &NSView) -> Retained<NSAlert> {
    let a = NSAlert::new(mtm);
    a.setMessageText(&ns(title));
    a.setInformativeText(&ns(info));
    a.addButtonWithTitle(&ns(t("Einfügen")));
    // NSAlert gives Escape only to a button titled "Cancel".
    a.addButtonWithTitle(&ns(t("Abbrechen")))
        .setKeyEquivalent(&ns("\u{1b}"));
    a.setAccessoryView(Some(accessory));
    a.layout();
    a
}

type LinkAlert = (Retained<NSAlert>, Retained<NSTextField>, Retained<NSTextField>);

/// The link dialog with its address and text fields, laid out but not shown.
pub fn link_alert(mtm: MainThreadMarker, selected: &str) -> LinkAlert {
    let looks_like_url = selected.starts_with("http://") || selected.starts_with("https://");
    let url = field(mtm, if looks_like_url { selected } else { "" }, "https://");
    let text = field(mtm, if looks_like_url { "" } else { selected }, t("Angezeigter Text"));
    let view = form(mtm, &[(t("Adresse:"), &url), (t("Text:"), &text)]);
    let a = alert(
        mtm,
        t("Link einfügen"),
        t("Adresse und angezeigten Text eingeben."),
        &view,
    );
    a.window().setInitialFirstResponder(Some(&url));
    (a, url, text)
}

/// Asks for a link target and text; `done` receives (url, text).
pub fn link(window: &NSWindow, selected: &str, done: impl Fn(String, String) + 'static) {
    let (a, url, text) = link_alert(window.mtm(), selected);
    let handler = RcBlock::new(move |response: NSModalResponse| {
        if response == NSAlertFirstButtonReturn {
            let u = url.stringValue().to_string();
            if !u.trim().is_empty() {
                done(u.trim().to_string(), text.stringValue().to_string());
            }
        }
    });
    a.beginSheetModalForWindow_completionHandler(window, Some(&handler));
}

/// Asks for the table size; `done` receives (rows including the header, columns).
pub fn table(window: &NSWindow, done: impl Fn(usize, usize) + 'static) {
    let mtm = window.mtm();
    let rows = field(mtm, "3", t("Zeilen"));
    let cols = field(mtm, "3", t("Spalten"));
    let stack = form(
        mtm,
        &[(t("Zeilen (inklusive Kopfzeile):"), &rows), (t("Spalten:"), &cols)],
    );
    let a = alert(
        mtm,
        t("Tabelle einfügen"),
        t("Größe der neuen Tabelle festlegen."),
        &stack,
    );
    let handler = RcBlock::new(move |response: NSModalResponse| {
        if response == NSAlertFirstButtonReturn {
            let r = rows.integerValue().clamp(2, 100) as usize;
            let c = cols.integerValue().clamp(1, 30) as usize;
            done(r, c);
        }
    });
    a.beginSheetModalForWindow_completionHandler(window, Some(&handler));
}

/// Lets the user pick an image file.
pub fn image(window: &NSWindow, done: impl Fn(Retained<NSURL>) + 'static) {
    let mtm = window.mtm();
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(true);
    panel.setCanChooseDirectories(false);
    panel.setAllowsMultipleSelection(false);
    panel.setPrompt(Some(&ns(t("Einfügen"))));
    panel.setMessage(Some(&ns(t("Bild auswählen, das in das Dokument eingefügt wird."))));
    if let Some(t) = UTType::typeWithIdentifier(&ns("public.image")) {
        panel.setAllowedContentTypes(&NSArray::from_retained_slice(&[t]));
    }
    let p = panel.clone();
    let handler = RcBlock::new(move |response: NSModalResponse| {
        if response == NSModalResponseOK
            && let Some(url) = p.URL()
        {
            done(url);
        }
    });
    panel.beginSheetModalForWindow_completionHandler(window, &handler);
}
