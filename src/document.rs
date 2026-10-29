use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::time::SystemTime;

use block2::RcBlock;
use objc2::rc::{Allocated, Retained, Weak};
use objc2::runtime::{AnyObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSDocument, NSDocumentController, NSPrintOperation};
use objc2_foundation::{NSData, NSDictionary, NSError, NSFileManager, NSObjectProtocol, NSString, NSTimer};

use crate::editor::EditorController;
use crate::util::constant;
use crate::util::ns;

/// What the file had beyond its text, written back on save so that a save changes only what was edited.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
struct Format {
    bom: bool,
    /// Every line ended in CRLF; lines typed since then get CRLF as well.
    crlf: bool,
}

impl Format {
    /// The format of `text` as read, and the text without its byte order mark.
    fn of(text: &str) -> (Format, &str) {
        let (bom, text) = match text.strip_prefix('\u{FEFF}') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let lines = text.matches('\n').count();
        let crlf = lines > 0 && text.matches("\r\n").count() == lines;
        (Format { bom, crlf }, text)
    }

    fn encode(self, text: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(text.len() + 3);
        if self.bom {
            out.extend_from_slice("\u{FEFF}".as_bytes());
        }
        if !self.crlf {
            out.extend_from_slice(text.as_bytes());
            return out;
        }
        for line in text.split_inclusive('\n') {
            match line.strip_suffix('\n') {
                Some(content) if !content.ends_with('\r') => {
                    out.extend_from_slice(content.as_bytes());
                    out.extend_from_slice(b"\r\n");
                }
                _ => out.extend_from_slice(line.as_bytes()),
            }
        }
        out
    }
}

#[derive(Default)]
pub struct Ivars {
    initial_text: RefCell<String>,
    format: Cell<Format>,
    editor: RefCell<Option<Weak<EditorController>>>,
    /// Modification time and size of the file when `watch_files` last looked.
    seen: Cell<Option<(SystemTime, u64)>>,
}

define_class!(
    #[unsafe(super(NSDocument))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDDocument"]
    #[ivars = Ivars]
    pub struct MdDocument;

    unsafe impl NSObjectProtocol for MdDocument {}

    impl MdDocument {
        #[unsafe(method_id(init))]
        fn init(this: Allocated<Self>) -> Option<Retained<Self>> {
            let this = this.set_ivars(Ivars::default());
            // SAFETY: NSDocument's initializer, called once on the newly allocated object.
            unsafe { msg_send![super(this), init] }
        }

        #[unsafe(method(autosavesInPlace))]
        fn autosaves_in_place() -> bool {
            true
        }

        #[unsafe(method(makeWindowControllers))]
        fn make_window_controllers(&self) {
            let text = self.ivars().initial_text.borrow().clone();
            let controller = EditorController::new(self.mtm(), self, &text);
            *self.ivars().editor.borrow_mut() = Some(Weak::from_retained(&controller));
            self.addWindowController(&controller);
        }

        #[unsafe(method(readFromData:ofType:error:))]
        fn read_from_data(&self, data: &NSData, _type_name: &NSString, error: *mut *mut NSError) -> bool {
            match String::from_utf8(data.to_vec()) {
                Ok(text) => {
                    let (format, text) = Format::of(&text);
                    self.ivars().format.set(format);
                    *self.ivars().initial_text.borrow_mut() = text.to_string();
                    if let Some(editor) = self.editor() {
                        editor.replace_all_text(text);
                    }
                    true
                }
                Err(_) => {
                    if !error.is_null() {
                        let e = read_error();
                        // SAFETY: AppKit passes a valid pointer for the error, checked for null above.
                        unsafe { *error = Retained::autorelease_ptr(e) };
                    }
                    false
                }
            }
        }

        // AppKit reports a change by another program here, on a background queue, but keeps showing the old text.
        #[unsafe(method(presentedItemDidChange))]
        fn presented_item_did_change(&self) {
            // SAFETY: NSDocument's implementation, with this method's signature.
            let _: () = unsafe { msg_send![super(self), presentedItemDidChange] };
            let check = || {
                if let Some(mtm) = MainThreadMarker::new() {
                    reload_changed_documents(mtm);
                }
            };
            dispatch2::DispatchQueue::main().exec_async(check);
            // A change within about half a second after Splat's own save comes without a report of its own.
            if let Ok(later) = dispatch2::DispatchTime::try_from(std::time::Duration::from_secs(1)) {
                let _ = dispatch2::DispatchQueue::main().after(later, check);
            }
        }

        #[unsafe(method_id(dataOfType:error:))]
        fn data_of_type(&self, _type_name: &NSString, _error: *mut *mut NSError) -> Option<Retained<NSData>> {
            let text = match self.editor() {
                Some(editor) => editor.exact_text(),
                None => self.text(),
            };
            Some(NSData::with_bytes(&self.ivars().format.get().encode(&text)))
        }

        #[unsafe(method_id(printOperationWithSettings:error:))]
        fn print_operation(
            &self,
            _settings: &NSDictionary<NSString, AnyObject>,
            _error: *mut *mut NSError,
        ) -> Option<Retained<NSPrintOperation>> {
            self.editor().map(|editor| editor.print_operation(&self.printInfo()))
        }
    }
);

define_class!(
    #[unsafe(super(NSDocumentController))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDDocumentController"]
    pub struct DocumentController;

    unsafe impl NSObjectProtocol for DocumentController {}

    impl DocumentController {
        // AppKit shows the plus button of the tab bar whenever newWindowForTab: is reachable; the toolbar has its own.
        #[unsafe(method(respondsToSelector:))]
        fn responds_to(&self, selector: Sel) -> bool {
            // SAFETY: The superclass's implementation, with this method's signature.
            selector != sel!(newWindowForTab:) && unsafe { msg_send![super(self), respondsToSelector: selector] }
        }

        // The menu places the share item itself, next to the exports.
        #[unsafe(method(allowsAutomaticShareMenu))]
        fn allows_automatic_share_menu(&self) -> bool {
            false
        }
    }
);

impl DocumentController {
    /// Must run before anything asks for the shared document controller: the first instance becomes it.
    pub fn install(mtm: MainThreadMarker) -> Retained<Self> {
        // SAFETY: NSDocumentController's initializer, called once on the newly allocated object.
        unsafe { msg_send![super(Self::alloc(mtm).set_ivars(())), init] }
    }
}

/// Rereads every document whose file another program changed.
pub fn reload_changed_documents(mtm: MainThreadMarker) {
    for document in documents(mtm) {
        reload_if_changed(&document);
    }
}

/// Looks at the files of open documents every two seconds: macOS does not report every change by another program, and
/// in a long session its reports for a document can stop altogether.
pub fn watch_files(mtm: MainThreadMarker) {
    let block = RcBlock::new(move |_: NonNull<NSTimer>| {
        for document in documents(mtm) {
            let Some(path) = document.fileURL().and_then(|u| u.path()) else {
                continue;
            };
            let now = std::fs::metadata(path.to_string())
                .ok()
                .and_then(|m| Some((m.modified().ok()?, m.len())));
            let before = document.ivars().seen.replace(now);
            if before != now {
                reload_if_changed(&document);
            }
        }
    });
    // SAFETY: The block only touches main-thread objects, and the timer runs on the main run loop it is scheduled on.
    let timer = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(2.0, true, &block) };
    timer.setTolerance(1.0);
}

fn documents(mtm: MainThreadMarker) -> Vec<Retained<MdDocument>> {
    NSDocumentController::sharedDocumentController(mtm)
        .documents()
        .iter()
        .filter_map(|d| d.downcast::<MdDocument>().ok())
        .collect()
}

/// Rereads the document if its file no longer holds the text it shows. A document with unsaved changes keeps them;
/// AppKit asks before saving over the other version.
fn reload_if_changed(document: &MdDocument) {
    if document.isDocumentEdited() {
        return;
    }
    let (Some(url), Some(kind)) = (document.fileURL(), document.fileType()) else {
        return;
    };
    let Some(path) = url.path() else { return };
    let Ok(bytes) = std::fs::read(path.to_string()) else {
        return;
    };
    // The content decides, not the date: a write right after Splat's own save leaves AppKit believing it knows the
    // new date.
    if document.shows(&bytes) {
        let on_disk = NSFileManager::defaultManager()
            .attributesOfItemAtPath_error(&path)
            .ok()
            .and_then(|attributes| attributes.fileModificationDate());
        if let Some(date) = on_disk {
            // Otherwise AppKit would warn about a change by another program when Splat saves next.
            document.setFileModificationDate(Some(&date));
        }
    } else if document.revertToContentsOfURL_ofType_error(&url, &kind).is_ok()
        && let Some(undo) = document.undoManager()
    {
        // Undo steps point into the old text.
        undo.removeAllActions();
    }
}

fn read_error() -> Retained<NSError> {
    let info = NSDictionary::from_slices(
        &[constant!(objc2_foundation::NSLocalizedDescriptionKey)],
        &[&*ns(crate::i18n::t(
            "Die Datei ist kein UTF-8-Text und kann nicht als Markdown geöffnet werden.",
        )) as &AnyObject],
    );
    // SAFETY: The user info holds only strings, as the Cocoa error domain expects.
    unsafe { NSError::errorWithDomain_code_userInfo(objc2_foundation::NSCocoaErrorDomain, 259, Some(&info)) }
}

impl MdDocument {
    pub fn editor(&self) -> Option<Retained<EditorController>> {
        self.ivars().editor.borrow().as_ref().and_then(Weak::load)
    }

    pub fn text(&self) -> String {
        match self.editor() {
            Some(editor) => editor.source_text(),
            None => self.ivars().initial_text.borrow().clone(),
        }
    }

    /// Whether `bytes` are what saving the document now would write.
    fn shows(&self, bytes: &[u8]) -> bool {
        let format = self.ivars().format.get();
        match self.editor() {
            Some(editor) => format.encode(&editor.snapshot().text) == bytes,
            None => format.encode(&self.ivars().initial_text.borrow()) == bytes,
        }
    }

    pub fn set_initial_text(&self, text: &str) {
        *self.ivars().initial_text.borrow_mut() = text.to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::Format;

    #[test]
    fn a_save_keeps_the_byte_order_mark_and_crlf_line_ends() {
        let (format, text) = Format::of("\u{FEFF}# A\r\n\r\nB\r\n");
        assert_eq!(format, Format { bom: true, crlf: true });
        assert_eq!(text, "# A\r\n\r\nB\r\n");
        let typed = format!("{text}C\nD");
        assert_eq!(format.encode(&typed), "\u{FEFF}# A\r\n\r\nB\r\nC\r\nD".as_bytes());
    }

    #[test]
    fn mixed_line_ends_stay_as_they_are() {
        let (format, text) = Format::of("a\r\nb\nc\n");
        assert_eq!(format, Format::default());
        assert_eq!(format.encode(&format!("{text}d\n")), b"a\r\nb\nc\nd\n");
    }
}
