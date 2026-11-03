use std::cell::{Cell, RefCell};
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::{Allocated, Retained, Weak};
use objc2::runtime::{AnyObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSDocument, NSDocumentChangeType, NSDocumentController, NSPrintOperation};
use objc2_foundation::{NSData, NSDate, NSDictionary, NSError, NSFileManager, NSObjectProtocol, NSString, NSTimer};

use crate::editor::EditorController;
use crate::util::constant;
use crate::util::{BusyVolumes, Stamp, lane_of_volume, ns, stamp, volume_of};

#[derive(Clone, Copy, Default, PartialEq, Debug)]
enum Encoding {
    #[default]
    Utf8,
    Utf16Le,
    Utf16Be,
}

impl Encoding {
    /// The file's text and how it was encoded. UTF-16 announces itself with a byte order mark, as Windows editors write
    /// it; anything else that is not UTF-8 gets `None`, since guessing would corrupt the text on the first save.
    fn read(bytes: Vec<u8>) -> Option<(Encoding, String)> {
        let wide = |rest: &[u8], unit: fn([u8; 2]) -> u16| {
            let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|&c| unit(c)).collect();
            String::from_utf16(&units).ok()
        };
        // UTF-32 starts like UTF-16, and half a code unit at the end cannot be written back.
        let whole = |rest: &[u8]| rest.len().is_multiple_of(2) && !rest.starts_with(&[0, 0]);
        if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE").filter(|r| whole(r)) {
            return Some((Encoding::Utf16Le, wide(rest, u16::from_le_bytes)?));
        }
        if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF").filter(|r| whole(r)) {
            return Some((Encoding::Utf16Be, wide(rest, u16::from_be_bytes)?));
        }
        Some((Encoding::Utf8, String::from_utf8(bytes).ok()?))
    }

    fn write(self, text: &str) -> Vec<u8> {
        let wide = |mark: [u8; 2], unit: fn(u16) -> [u8; 2]| {
            let mut out = Vec::with_capacity(text.len() * 2 + 2);
            out.extend_from_slice(&mark);
            out.extend(text.encode_utf16().flat_map(unit));
            out
        };
        match self {
            Encoding::Utf8 => text.as_bytes().to_vec(),
            Encoding::Utf16Le => wide([0xFF, 0xFE], u16::to_le_bytes),
            Encoding::Utf16Be => wide([0xFE, 0xFF], u16::to_be_bytes),
        }
    }
}

/// What the file had beyond its text, written back on save so that a save changes only what was edited.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
struct FileFormat {
    bom: bool,
    /// Every line ended in CRLF; lines typed since then get CRLF as well.
    crlf: bool,
    encoding: Encoding,
}

impl FileFormat {
    /// The format of `text` as read, and the text without its byte order mark.
    fn of(text: &str) -> (FileFormat, &str) {
        let (bom, text) = match text.strip_prefix('\u{FEFF}') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let lines = text.matches('\n').count();
        let crlf = lines > 0 && text.matches("\r\n").count() == lines;
        (
            FileFormat {
                bom,
                crlf,
                encoding: Encoding::Utf8,
            },
            text,
        )
    }

    fn encode(self, text: &str) -> Vec<u8> {
        if !self.bom && !self.crlf {
            return self.encoding.write(text);
        }
        let mut out = String::with_capacity(text.len() + 3);
        if self.bom {
            out.push('\u{FEFF}');
        }
        if !self.crlf {
            out.push_str(text);
            return self.encoding.write(&out);
        }
        for line in text.split_inclusive('\n') {
            match line.strip_suffix('\n') {
                Some(content) if !content.ends_with('\r') => {
                    out.push_str(content);
                    out.push_str("\r\n");
                }
                _ => out.push_str(line),
            }
        }
        self.encoding.write(&out)
    }
}

/// What the saves under way write. A save that takes its text before an earlier one has ended writes the newest text in
/// both, so the text goes with the last of them; one that fails never reports its end and leaves the text in place.
#[derive(Default)]
struct Saving {
    bytes: Option<Vec<u8>>,
    under_way: usize,
}

impl Saving {
    fn started(&mut self, bytes: Option<Vec<u8>>) {
        self.bytes = bytes;
        self.under_way += 1;
    }

    fn ended(&mut self) {
        self.under_way = self.under_way.saturating_sub(1);
        if self.under_way == 0 {
            self.bytes = None;
        }
    }
}

#[derive(Default)]
pub struct Ivars {
    /// The text until the editor takes it over, which from then on holds the only copy.
    initial_text: RefCell<Option<String>>,
    format: Cell<FileFormat>,
    editor: RefCell<Option<Weak<EditorController>>>,
    /// How the file looked when its content was last compared with the text.
    seen: Cell<Stamp>,
    failed_reads: Cell<FailedReads>,
    /// What the save under way writes; the only state `dataOfType` reads, since AppKit calls that on another thread.
    saving: std::sync::Mutex<Saving>,
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
            let text = self.ivars().initial_text.take().unwrap_or_else(|| self.text());
            let controller = EditorController::new(self.mtm(), self, &text);
            *self.ivars().editor.borrow_mut() = Some(Weak::from_retained(&controller));
            self.addWindowController(&controller);
        }

        // AppKit then reads on another thread, so that a volume that hangs does not stop the app. Nothing else knows the
        // document before it comes back, which is what makes its state safe to touch in `readFromData`.
        #[unsafe(method(canConcurrentlyReadDocumentsOfType:))]
        fn can_concurrently_read(_type_name: &NSString) -> bool {
            true
        }

        #[unsafe(method(readFromData:ofType:error:))]
        fn read_from_data(&self, data: &NSData, _type_name: &NSString, error: *mut *mut NSError) -> bool {
            let read = self.take_bytes(data.to_vec());
            if !read && !error.is_null() {
                let e = read_error();
                // SAFETY: AppKit passes a valid pointer for the error, checked for null above.
                unsafe { *error = Retained::autorelease_ptr(e) };
            }
            read
        }

        // Taken with the token that marks the document as saved, on the main thread: its text views are out of reach of
        // `dataOfType`, and file and token then stand for the same text, however long this save waits behind another.
        #[unsafe(method_id(changeCountTokenForSaveOperation:))]
        fn change_count_token(&self, operation: objc2_app_kit::NSSaveOperationType) -> Retained<AnyObject> {
            if let Ok(mut saving) = self.ivars().saving.lock() {
                saving.started(self.encoded_text());
            }
            // SAFETY: NSDocument's implementation, with this method's signature.
            unsafe { msg_send![super(self), changeCountTokenForSaveOperation: operation] }
        }

        // AppKit calls this once a save has written its file.
        #[unsafe(method(updateChangeCountWithToken:forSaveOperation:))]
        fn update_change_count(&self, token: &AnyObject, operation: objc2_app_kit::NSSaveOperationType) {
            // SAFETY: NSDocument's implementation, with this method's signature.
            let _: () = unsafe { msg_send![super(self), updateChangeCountWithToken: token, forSaveOperation: operation] };
            if let Ok(mut saving) = self.ivars().saving.lock() {
                saving.ended();
            }
        }

        // AppKit then writes on another thread, keeping the main thread waiting only until `dataOfType` has returned.
        #[unsafe(method(canAsynchronouslyWriteToURL:ofType:forSaveOperation:))]
        fn can_asynchronously_write(
            &self,
            _url: &objc2_foundation::NSURL,
            _type_name: &NSString,
            _operation: objc2_app_kit::NSSaveOperationType,
        ) -> bool {
            true
        }

        // AppKit reports a change by another program here, on a background queue, but keeps showing the old text. So off
        // the main thread, despite the class: nothing here may use `self` but to call super as AppKit expects.
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
        fn data_of_type(&self, _type_name: &NSString, error: *mut *mut NSError) -> Option<Retained<NSData>> {
            let data = self.data_to_write();
            if data.is_none() && !error.is_null() {
                // SAFETY: AppKit passes a valid pointer for the error, checked for null above.
                unsafe { *error = Retained::autorelease_ptr(write_error()) };
            }
            data
        }

        #[unsafe(method_id(printOperationWithSettings:error:))]
        fn print_operation(
            &self,
            _settings: &NSDictionary<NSString, AnyObject>,
            error: *mut *mut NSError,
        ) -> Option<Retained<NSPrintOperation>> {
            let operation = self.editor().map(|editor| editor.print_operation(&self.printInfo()));
            if operation.is_none() && !error.is_null() {
                // SAFETY: AppKit passes a valid pointer for the error, checked for null above.
                unsafe { *error = Retained::autorelease_ptr(write_error()) };
            }
            operation
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
    look_at_files(mtm);
}

/// Looks at the files of open documents every two seconds: macOS does not report every change by another program, and
/// in a long session its reports for a document can stop altogether.
pub fn watch_files(mtm: MainThreadMarker) {
    let block = RcBlock::new(move |_: NonNull<NSTimer>| {
        look_at_files(mtm);
        crate::editor::look_at_images(mtm);
    });
    // SAFETY: The block only touches main-thread objects, and the timer runs on the main run loop it is scheduled on.
    let timer = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(2.0, true, &block) };
    timer.setTolerance(1.0);
}

/// A look at the files of a volume is under way; one on a volume that stopped answering must not pile up behind it.
static LOOKING: BusyVolumes = BusyVolumes::new();

/// What a document does about a file that may have changed.
#[derive(Debug, PartialEq)]
enum Reaction {
    /// Its own text goes first, or the file is older than what it knows.
    Keep,
    /// The file holds what the document shows, so only AppKit's idea of its date is out of date.
    TakeDate,
    Reread,
}

fn reaction(edited: bool, same_content: bool, known_date: Option<f64>, file_date: Option<f64>) -> Reaction {
    if edited {
        Reaction::Keep
    // The content decides, not the date: a write right after Splat's own save leaves AppKit believing it knows the new
    // date.
    } else if same_content {
        Reaction::TakeDate
    // A save since the file was read makes these bytes the older version.
    } else if known_date.zip(file_date).is_some_and(|(known, file)| known > file) {
        Reaction::Keep
    } else {
        Reaction::Reread
    }
}

/// A file that grew beyond this while it was open is not read again behind the user's back.
const MAX_RELOAD_BYTES: u64 = 4 << 30;

/// How often a file that reads as no text is read again, as one being written may for a moment.
const READ_TRIES: u8 = 5;

/// Failed reads of one version of a file.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
struct FailedReads {
    version: Stamp,
    tries: u8,
}

impl FailedReads {
    /// Counts a failed read of the version `now`.
    fn after_another(self, now: Stamp) -> FailedReads {
        let tries = if self.version == now {
            self.tries.saturating_add(1)
        } else {
            1
        };
        FailedReads { version: now, tries }
    }

    fn try_again(self) -> bool {
        self.tries < READ_TRIES
    }
}

/// What another thread found at a document's path.
struct Look {
    path: String,
    now: Stamp,
    /// The content and its date in seconds, if the file changed since the document last compared it.
    changed: Option<(Vec<u8>, Option<f64>)>,
}

/// Documents with unsaved changes keep them, so their files wait until those are saved; AppKit asks before saving over
/// another version.
fn look_at_files(mtm: MainThreadMarker) {
    let mut by_volume: std::collections::BTreeMap<String, Vec<(String, Stamp)>> = Default::default();
    for document in documents(mtm).iter().filter(|d| !d.isDocumentEdited()) {
        if let Some(path) = document.fileURL().and_then(|u| u.path()).map(|p| p.to_string()) {
            by_volume
                .entry(volume_of(std::path::Path::new(&path)))
                .or_default()
                .push((path, document.ivars().seen.get()));
        }
    }
    for (volume, requests) in by_volume {
        let Some(on_volume) = LOOKING.enter(&volume) else {
            continue;
        };
        lane_of_volume(&volume).run(move || {
            let looks: Vec<Look> = requests.into_iter().map(|(path, seen)| look(path, seen)).collect();
            dispatch2::DispatchQueue::main().exec_async(move || {
                drop(on_volume);
                let Some(mtm) = MainThreadMarker::new() else { return };
                let open = documents(mtm);
                for look in looks {
                    let path = Some(look.path.as_str());
                    let same = |d: &&Retained<MdDocument>| {
                        d.fileURL()
                            .and_then(|u| u.path())
                            .as_deref()
                            .map(|p| p.to_string())
                            .as_deref()
                            == path
                    };
                    if let Some(document) = open.iter().find(same) {
                        document.take_look(look.now, look.changed);
                    }
                }
            });
        });
    }
}

fn look(path: String, seen: Stamp) -> Look {
    let now = stamp(std::path::Path::new(&path));
    let changed = if now == seen {
        None
    } else {
        crate::util::read_regular_file(std::path::Path::new(&path), MAX_RELOAD_BYTES).map(|bytes| {
            let date = NSFileManager::defaultManager()
                .attributesOfItemAtPath_error(&ns(&path))
                .ok()
                .and_then(|attributes| attributes.fileModificationDate());
            (bytes, date.map(|d| d.timeIntervalSinceReferenceDate()))
        })
    };
    Look { path, now, changed }
}

fn documents(mtm: MainThreadMarker) -> Vec<Retained<MdDocument>> {
    NSDocumentController::sharedDocumentController(mtm)
        .documents()
        .iter()
        .filter_map(|d| d.downcast::<MdDocument>().ok())
        .collect()
}

/// AppKit words the message for a failed save itself.
fn write_error() -> Retained<NSError> {
    let info = NSDictionary::<objc2_foundation::NSErrorUserInfoKey, AnyObject>::new();
    // SAFETY: An empty user info, as the Cocoa error domain accepts.
    unsafe { NSError::errorWithDomain_code_userInfo(objc2_foundation::NSCocoaErrorDomain, 512, Some(&info)) }
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
            None => self.ivars().initial_text.borrow().clone().unwrap_or_default(),
        }
    }

    /// The content of a file now becomes the document's; `false` if it is no text Splat can read.
    fn take_bytes(&self, bytes: Vec<u8>) -> bool {
        let Some((encoding, text)) = Encoding::read(bytes) else {
            return false;
        };
        let (format, text) = FileFormat::of(&text);
        self.ivars().format.set(FileFormat { encoding, ..format });
        match self.editor() {
            Some(editor) => editor.replace_all_text(text),
            None => self.set_initial_text(text),
        }
        true
    }

    /// What saving writes now, from whichever thread AppKit saves on.
    fn data_to_write(&self) -> Option<Retained<NSData>> {
        if MainThreadMarker::new().is_some() {
            return self.encoded_text().as_deref().map(NSData::with_bytes);
        }
        self.ivars()
            .saving
            .lock()
            .ok()?
            .bytes
            .as_deref()
            .map(NSData::with_bytes)
    }

    /// `None` once the editor that held the text is gone: a save must then fail rather than empty the file.
    fn encoded_text(&self) -> Option<Vec<u8>> {
        let format = self.ivars().format.get();
        match self.editor() {
            Some(editor) => Some(format.encode(&editor.exact_text())),
            None => self
                .ivars()
                .initial_text
                .borrow()
                .as_deref()
                .map(|text| format.encode(text)),
        }
    }

    /// Whether the document holds the text beside its editor.
    #[cfg(feature = "selftest")]
    pub fn keeps_a_copy_of_its_text(&self) -> bool {
        self.ivars().initial_text.borrow().is_some() || self.ivars().saving.lock().is_ok_and(|s| s.bytes.is_some())
    }

    /// Whether `bytes` are what saving the document now would write.
    fn shows(&self, bytes: &[u8]) -> bool {
        let format = self.ivars().format.get();
        match self.editor() {
            Some(editor) => editor.on_source_text(|text| format.encode(text) == bytes),
            None => self
                .ivars()
                .initial_text
                .borrow()
                .as_deref()
                .is_some_and(|text| format.encode(text) == bytes),
        }
    }

    /// Takes in what another thread found: rereads the document if its file no longer holds the text it shows.
    fn take_look(&self, now: Stamp, changed: Option<(Vec<u8>, Option<f64>)>) {
        if self.isDocumentEdited() {
            return;
        }
        let before = self.ivars().seen.replace(now);
        let Some((bytes, date)) = changed else { return };
        let known = self.fileModificationDate().map(|d| d.timeIntervalSinceReferenceDate());
        match reaction(false, self.shows(&bytes), known, date) {
            Reaction::Keep => {}
            Reaction::TakeDate => {
                // Otherwise AppKit would warn about a change by another program when Splat saves next.
                if let Some(date) = date.map(NSDate::dateWithTimeIntervalSinceReferenceDate) {
                    self.setFileModificationDate(Some(&date));
                }
            }
            // The bytes are at hand: asking AppKit to revert would read the file again on the main thread.
            Reaction::Reread if self.take_bytes(bytes) => {
                self.updateChangeCount(NSDocumentChangeType::ChangeCleared);
                if let Some(date) = date.map(NSDate::dateWithTimeIntervalSinceReferenceDate) {
                    self.setFileModificationDate(Some(&date));
                }
                // Undo steps point into the old text.
                if let Some(undo) = self.undoManager() {
                    undo.removeAllActions();
                }
            }
            Reaction::Reread => {
                let failed = self.ivars().failed_reads.get().after_another(now);
                self.ivars().failed_reads.set(failed);
                if failed.try_again() {
                    self.ivars().seen.set(before);
                }
            }
        }
    }

    pub fn set_initial_text(&self, text: &str) {
        *self.ivars().initial_text.borrow_mut() = Some(text.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::{Encoding, FailedReads, FileFormat, READ_TRIES, Reaction, Saving, reaction};

    #[test]
    fn the_text_of_a_save_goes_with_the_last_save_under_way() {
        let mut saving = Saving::default();
        saving.started(Some(vec![1]));
        saving.started(Some(vec![2]));
        saving.ended();
        assert_eq!(saving.bytes, Some(vec![2]), "the second save has yet to read it");
        saving.ended();
        assert_eq!(saving.bytes, None);
        saving.ended();
        saving.started(Some(vec![3]));
        assert_eq!(saving.bytes, Some(vec![3]), "an end too many counts for nothing");
    }

    #[test]
    fn a_file_that_stays_unreadable_is_not_read_forever() {
        let version = |seconds| Some((std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds), 1));
        let mut failed = FailedReads::default();
        for _ in 1..READ_TRIES {
            failed = failed.after_another(version(1));
            assert!(failed.try_again());
        }
        failed = failed.after_another(version(1));
        assert!(!failed.try_again());
        assert!(
            failed.after_another(version(2)).try_again(),
            "a new version gets new tries"
        );
    }

    #[test]
    fn a_file_may_only_reread_a_document_without_changes() {
        assert_eq!(
            reaction(true, false, None, None),
            Reaction::Keep,
            "unsaved text goes first"
        );
        assert_eq!(reaction(false, true, Some(1.0), Some(2.0)), Reaction::TakeDate);
        assert_eq!(
            reaction(false, false, Some(3.0), Some(2.0)),
            Reaction::Keep,
            "a save since the file was read"
        );
        assert_eq!(reaction(false, false, Some(2.0), Some(3.0)), Reaction::Reread);
        assert_eq!(reaction(false, false, None, None), Reaction::Reread);
    }

    #[test]
    fn byte_order_marks_that_do_not_announce_utf_16_are_refused() {
        assert!(Encoding::read(b"\xFF\xFE\0\0a\0\0\0".to_vec()).is_none(), "UTF-32");
        assert!(Encoding::read(b"\xFF\xFEa\0b".to_vec()).is_none(), "half a code unit");
        assert!(Encoding::read(b"\xFE\xFF\0a\0".to_vec()).is_none(), "half a code unit");
    }

    #[test]
    fn a_file_keeps_every_mix_of_mark_and_line_ends() {
        for (name, file) in [
            ("plain", "a\nb\n"),
            ("mark, line feeds", "\u{FEFF}a\nb\n"),
            ("no mark, Windows", "a\r\nb\r\n"),
            ("mark, Windows", "\u{FEFF}a\r\nb\r\n"),
            ("old Mac", "a\rb\r"),
        ] {
            let (format, text) = FileFormat::of(file);
            assert_eq!(format.encode(text), file.as_bytes(), "{name}");
        }
    }

    #[test]
    fn a_save_keeps_the_byte_order_mark_and_crlf_line_ends() {
        let (format, text) = FileFormat::of("\u{FEFF}# A\r\n\r\nB\r\n");
        assert_eq!(
            format,
            FileFormat {
                bom: true,
                crlf: true,
                encoding: Encoding::Utf8
            }
        );
        assert_eq!(text, "# A\r\n\r\nB\r\n");
        let typed = format!("{text}C\nD");
        assert_eq!(format.encode(&typed), "\u{FEFF}# A\r\n\r\nB\r\nC\r\nD".as_bytes());
    }

    #[test]
    fn mixed_line_ends_stay_as_they_are() {
        let (format, text) = FileFormat::of("a\r\nb\nc\n");
        assert_eq!(format, FileFormat::default());
        assert_eq!(format.encode(&format!("{text}d\n")), b"a\r\nb\nc\nd\n");
    }

    fn format_of(bytes: &[u8]) -> (FileFormat, String) {
        let (encoding, text) = Encoding::read(bytes.to_vec()).expect("text");
        let (format, text) = FileFormat::of(&text);
        (FileFormat { encoding, ..format }, text.to_string())
    }

    #[test]
    fn utf16_with_a_byte_order_mark_opens_and_saves_as_it_came() {
        let cases: [(&[u8], &str, Encoding); 3] = [
            (b"\xFF\xFEa\0\xE4\0\r\0\n\0", "aä\r\n", Encoding::Utf16Le),
            (b"\xFE\xFF\0a\0\xE4\0\r\0\n", "aä\r\n", Encoding::Utf16Be),
            ("Grüße\n".as_bytes(), "Grüße\n", Encoding::Utf8),
        ];
        for (bytes, expected, encoding) in cases {
            let (format, text) = format_of(bytes);
            assert_eq!((text.as_str(), format.encoding), (expected, encoding));
            assert_eq!(format.encode(&text), bytes, "{encoding:?}");
        }
    }

    #[test]
    fn other_encodings_and_broken_wide_text_are_refused() {
        assert!(Encoding::read(b"Gr\xFC\xDFe".to_vec()).is_none(), "no guessing");
        assert!(Encoding::read(b"\x89PNG\r\n\x1a\n".to_vec()).is_none());
        assert!(Encoding::read(b"\xFF\xFE\x00\xD8".to_vec()).is_none());
    }
}
