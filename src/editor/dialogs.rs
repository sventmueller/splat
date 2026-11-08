use std::cell::RefCell;
use std::path::{Path, PathBuf};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSButton, NSControlStateValueOff, NSControlStateValueOn, NSModalResponse,
    NSModalResponseOK, NSOpenPanel, NSTextField, NSView, NSWindow,
};
use objc2_foundation::{NSArray, NSObjectProtocol, NSPoint, NSRect, NSSize, NSURL};
use objc2_uniform_type_identifiers::UTType;

use super::export_options::ExportOptions;
use crate::i18n::t;
use crate::util::{guarded, ns};

const FORM_WIDTH: f64 = 320.0;
const LABEL_HEIGHT: f64 = 17.0;
const FIELD_HEIGHT: f64 = 24.0;
const LABEL_INDENT: f64 = 4.0;

fn field(mtm: MainThreadMarker, value: &str, placeholder: &str) -> Retained<NSTextField> {
    let f = NSTextField::textFieldWithString(&ns(value), mtm);
    f.setPlaceholderString(Some(&ns(placeholder)));
    f
}

/// The height of a form of `rows` rows, and the frames of each row's label and field.
fn form_frames(rows: usize) -> (f64, Vec<(NSRect, NSRect)>) {
    let row = LABEL_HEIGHT + 4.0 + FIELD_HEIGHT;
    let gap = 12.0;
    let height = rows as f64 * row + rows.saturating_sub(1) as f64 * gap;
    let frames = (0..rows)
        .map(|n| {
            let top = height - n as f64 * (row + gap);
            (
                // Indented so the label text lines up with the text inside the field and the alert title.
                NSRect::new(
                    NSPoint::new(LABEL_INDENT, top - LABEL_HEIGHT),
                    NSSize::new(FORM_WIDTH - LABEL_INDENT, LABEL_HEIGHT),
                ),
                NSRect::new(NSPoint::new(0.0, top - row), NSSize::new(FORM_WIDTH, FIELD_HEIGHT)),
            )
        })
        .collect();
    (height, frames)
}

/// Labels above full-width fields, placed by frame: in a stack view the fields shrank to their placeholder, and
/// NSAlert then centred the narrow column.
fn form(mtm: MainThreadMarker, rows: &[(&str, &Retained<NSTextField>)]) -> Retained<NSView> {
    let (height, frames) = form_frames(rows.len());
    let view = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(FORM_WIDTH, height)),
    );
    for ((label, f), (label_frame, field_frame)) in rows.iter().zip(frames) {
        let l = NSTextField::labelWithString(&ns(label), mtm);
        l.setFrame(label_frame);
        view.addSubview(&l);
        f.setFrame(field_frame);
        view.addSubview(f);
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

/// What the address and the text field of the link dialog start with: a selected address is the address.
fn link_fields(selected: &str) -> (&str, &str) {
    if crate::markdown::url::is_web(selected) {
        (selected, "")
    } else {
        ("", selected)
    }
}

/// What the link dialog hands on: no address, no link.
fn link_answer(address: &str, text: &str) -> Option<(String, String)> {
    let address = address.trim();
    (!address.is_empty()).then(|| (address.to_string(), text.to_string()))
}

/// The size of a new table from what was typed: it needs a header and a row, and a field that holds no number counts as 0.
fn table_size(rows: isize, columns: isize) -> (usize, usize) {
    (
        crate::util::to_usize(rows.clamp(2, 100)),
        crate::util::to_usize(columns.clamp(1, 30)),
    )
}

type LinkAlert = (Retained<NSAlert>, Retained<NSTextField>, Retained<NSTextField>);

/// The link dialog with its address and text fields, laid out but not shown.
pub fn link_alert(mtm: MainThreadMarker, selected: &str) -> LinkAlert {
    let (address, shown) = link_fields(selected);
    let url = field(mtm, address, "https://");
    let text = field(mtm, shown, t("Angezeigter Text"));
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
        guarded((), || {
            if response == NSAlertFirstButtonReturn
                && let Some((address, shown)) =
                    link_answer(&url.stringValue().to_string(), &text.stringValue().to_string())
            {
                done(address, shown);
            }
        });
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
        guarded((), || {
            if response == NSAlertFirstButtonReturn {
                let (r, c) = table_size(rows.integerValue(), cols.integerValue());
                done(r, c);
            }
        });
    });
    a.beginSheetModalForWindow_completionHandler(window, Some(&handler));
}

/// Asks for a note's new name; `done` receives what was typed.
pub fn rename(window: &NSWindow, name: &str, done: impl Fn(String) + 'static) {
    let mtm = window.mtm();
    let field = field(mtm, name, "");
    let view = form(mtm, &[(t("Name:"), &field)]);
    let a = NSAlert::new(mtm);
    a.setMessageText(&ns(t("Notiz umbenennen")));
    a.addButtonWithTitle(&ns(t("Umbenennen")));
    a.addButtonWithTitle(&ns(t("Abbrechen")))
        .setKeyEquivalent(&ns("\u{1b}"));
    a.setAccessoryView(Some(&view));
    a.layout();
    a.window().setInitialFirstResponder(Some(&field));
    let handler = RcBlock::new(move |response: NSModalResponse| {
        guarded((), || {
            if response == NSAlertFirstButtonReturn {
                done(field.stringValue().to_string());
            }
        });
    });
    a.beginSheetModalForWindow_completionHandler(window, Some(&handler));
}

pub struct OptionIvars {
    template: RefCell<Option<PathBuf>>,
    label: Retained<NSTextField>,
}

define_class!(
    #[unsafe(super(objc2::runtime::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDExportOptionsForm"]
    #[ivars = OptionIvars]
    pub struct OptionsForm;

    unsafe impl NSObjectProtocol for OptionsForm {}

    impl OptionsForm {
        #[unsafe(method(optionsChooseTemplate:))]
        fn choose_template(&self, _sender: Option<&AnyObject>) {
            guarded((), || {
                let panel = NSOpenPanel::openPanel(self.mtm());
                panel.setCanChooseFiles(true);
                panel.setCanChooseDirectories(false);
                panel.setAllowsMultipleSelection(false);
                let kinds: Vec<Retained<UTType>> = ["docx", "dotx"]
                    .iter()
                    .filter_map(|extension| UTType::typeWithFilenameExtension(&ns(extension)))
                    .collect();
                panel.setAllowedContentTypes(&NSArray::from_retained_slice(&kinds));
                if panel.runModal() == NSModalResponseOK
                    && let Some(url) = panel.URL()
                    && let Some(path) = url.path()
                {
                    // Every export reads it again, also after Splat restarted.
                    crate::grants::remember(&url);
                    self.show_template(Some(PathBuf::from(path.to_string())));
                }
            });
        }

        #[unsafe(method(optionsClearTemplate:))]
        fn clear_template(&self, _sender: Option<&AnyObject>) {
            guarded((), || self.show_template(None));
        }
    }
);

impl OptionsForm {
    fn show_template(&self, template: Option<PathBuf>) {
        let shown = template
            .as_deref()
            .and_then(Path::file_name)
            .map_or_else(|| t("Keine").to_string(), |name| name.to_string_lossy().into_owned());
        self.ivars().label.setStringValue(&ns(&shown));
        self.ivars()
            .label
            .setToolTip(template.as_deref().and_then(Path::to_str).map(ns).as_deref());
        *self.ivars().template.borrow_mut() = template;
    }
}

/// Asks what exports add to the text; `done` receives the options when the user confirms.
pub fn export_options(window: &NSWindow, current: ExportOptions, done: impl Fn(ExportOptions) + 'static) {
    let mtm = window.mtm();
    let check = |title: &str, on: bool| {
        // SAFETY: Without a target and an action the box only holds its state, which is read when the sheet ends.
        let b = unsafe { NSButton::checkboxWithTitle_target_action(&ns(title), None, None, mtm) };
        b.setState(if on {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        b
    };
    let numbers = check(t("Seitenzahlen"), current.page_numbers);
    let title_page = check(t("Titelblatt aus den Metadaten"), current.title_page);
    let header = field(mtm, &current.header, "{title}");
    let footer = field(mtm, &current.footer, "{author} · {date}");
    let fields = form(mtm, &[(t("Kopfzeile:"), &header), (t("Fußzeile:"), &footer)]);

    let label = NSTextField::labelWithString(&ns(""), mtm);
    label.setLineBreakMode(objc2_app_kit::NSLineBreakMode::ByTruncatingMiddle);
    let options: Retained<OptionsForm> = {
        let this = OptionsForm::alloc(mtm).set_ivars(OptionIvars {
            template: RefCell::new(None),
            label: label.clone(),
        });
        // SAFETY: NSObject's initializer, called once on the newly allocated object.
        unsafe { msg_send![super(this), init] }
    };
    options.show_template(current.word_template.clone());
    let action_button = |title: &str, action: objc2::runtime::Sel| {
        let b = crate::util::button(mtm, title, &options, action);
        b.setControlSize(objc2_app_kit::NSControlSize::Small);
        b.sizeToFit();
        b
    };
    let choose = action_button(t("Wählen …"), objc2::sel!(optionsChooseTemplate:));
    let clear = action_button(t("Entfernen"), objc2::sel!(optionsClearTemplate:));
    let template_title = NSTextField::labelWithString(&ns(t("Word-Vorlage:")), mtm);

    let fields_height = fields.frame().size.height;
    let total = fields_height + 3.0 * 24.0 + 2.0 * LABEL_HEIGHT + 24.0;
    let view = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(FORM_WIDTH, total)),
    );
    let place = |control: &NSView, x: f64, y: f64, width: f64, height: f64| {
        control.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(width, height)));
        view.addSubview(control);
    };
    let mut y = total - 20.0;
    place(&numbers, LABEL_INDENT, y, FORM_WIDTH - LABEL_INDENT, 18.0);
    y -= 24.0;
    place(&title_page, LABEL_INDENT, y, FORM_WIDTH - LABEL_INDENT, 18.0);
    y -= fields_height + 8.0;
    place(&fields, 0.0, y, FORM_WIDTH, fields_height);
    y -= LABEL_HEIGHT + 12.0;
    place(
        &template_title,
        LABEL_INDENT,
        y,
        FORM_WIDTH - LABEL_INDENT,
        LABEL_HEIGHT,
    );
    y -= 26.0;
    let (choose_width, clear_width) = (choose.frame().size.width, clear.frame().size.width);
    place(
        &label,
        LABEL_INDENT,
        y,
        FORM_WIDTH - LABEL_INDENT - choose_width - clear_width - 12.0,
        LABEL_HEIGHT,
    );
    place(
        &choose,
        FORM_WIDTH - choose_width - clear_width - 4.0,
        y - 3.0,
        choose_width,
        24.0,
    );
    place(&clear, FORM_WIDTH - clear_width, y - 3.0, clear_width, 24.0);

    let a = NSAlert::new(mtm);
    a.setMessageText(&ns(t("Exportoptionen")));
    a.setInformativeText(&ns(t(
        "Kopf- und Fußzeile gelten für PDF und Drucken, eine Word-Vorlage bringt ihre eigenen mit. Dort stehen {title}, {author}, {date}, {page} und {pages} für den jeweiligen Wert.",
    )));
    a.addButtonWithTitle(&ns(t("Übernehmen")));
    a.addButtonWithTitle(&ns(t("Abbrechen")))
        .setKeyEquivalent(&ns("\u{1b}"));
    a.setAccessoryView(Some(&view));
    a.layout();
    let handler = RcBlock::new(move |response: NSModalResponse| {
        guarded((), || {
            if response == NSAlertFirstButtonReturn {
                done(ExportOptions {
                    page_numbers: numbers.state() == NSControlStateValueOn,
                    title_page: title_page.state() == NSControlStateValueOn,
                    header: header.stringValue().to_string(),
                    footer: footer.stringValue().to_string(),
                    word_template: options.ivars().template.borrow().clone(),
                });
            }
        });
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
        guarded((), || {
            if response == NSModalResponseOK
                && let Some(url) = p.URL()
            {
                done(url);
            }
        });
    });
    panel.beginSheetModalForWindow_completionHandler(window, &handler);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selected_address_goes_into_the_address_field_and_all_else_into_the_text() {
        assert_eq!(link_fields("https://a.de"), ("https://a.de", ""));
        assert_eq!(link_fields("http://a.de"), ("http://a.de", ""));
        assert_eq!(link_fields("a.de"), ("", "a.de"));
        assert_eq!(link_fields("Text mit https://a.de"), ("", "Text mit https://a.de"));
        assert_eq!(link_fields(""), ("", ""));
    }

    #[test]
    fn a_link_needs_an_address_and_keeps_its_text_as_typed() {
        assert_eq!(
            link_answer("  https://a.de \n", " Text "),
            Some(("https://a.de".to_string(), " Text ".to_string()))
        );
        assert_eq!(link_answer("x", ""), Some(("x".to_string(), String::new())));
        assert_eq!(link_answer("", "Text"), None);
        assert_eq!(link_answer(" \t\n", "Text"), None);
    }

    #[test]
    fn a_table_has_a_header_and_a_row_and_stays_within_reason() {
        assert_eq!(table_size(3, 3), (3, 3));
        assert_eq!(table_size(0, 0), (2, 1), "what no number gives");
        assert_eq!(table_size(-5, -5), (2, 1));
        assert_eq!(table_size(2, 1), (2, 1));
        assert_eq!(table_size(100, 30), (100, 30));
        assert_eq!(table_size(101, 31), (100, 30));
    }

    #[test]
    fn labels_stand_above_full_width_fields_from_the_top_down() {
        let (height, frames) = form_frames(2);
        assert_eq!(height, 102.0);
        let frame = |x, y, w, h| NSRect::new(NSPoint::new(x, y), NSSize::new(w, h));
        assert_eq!(
            frames,
            [
                (frame(4.0, 85.0, 316.0, 17.0), frame(0.0, 57.0, 320.0, 24.0)),
                (frame(4.0, 28.0, 316.0, 17.0), frame(0.0, 0.0, 320.0, 24.0)),
            ]
        );
        assert_eq!(form_frames(1).0, 45.0, "no gap below a single row");
        assert_eq!(form_frames(0), (0.0, Vec::new()));
    }
}
