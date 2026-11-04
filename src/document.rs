use std::cell::{Cell, RefCell};
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::{Allocated, Retained, Weak};
use objc2::runtime::{AnyObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{NSDocument, NSDocumentChangeType, NSDocumentController, NSPrintOperation, NSSaveOperationType};
use objc2_foundation::{
    NSData, NSDate, NSDictionary, NSError, NSFileManager, NSObjectProtocol, NSString, NSTimer, NSURL,
};
use splat_file_state::{
    ChangeMark, Encoding, FailedReads, FileFormat, FileState, OnFile, Print, Reaction, Stamp, mark_for, reaction,
};

use crate::editor::EditorController;
use crate::util::constant;
use crate::util::{BusyVolumes, guarded, lane_of_volume, main_after, main_soon, ns, stamp, volume_of, with_raised};

/// Rounds of looks at the files whose answers have been taken in, for a check that something did not happen to wait for.
#[cfg(feature = "selftest")]
pub static LOOKS_TAKEN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Where the next save or reload panics, which nothing else can bring about on demand.
#[cfg(feature = "selftest")]
pub static FAULT_AT: std::sync::Mutex<Option<&'static str>> = std::sync::Mutex::new(None);

#[cfg(feature = "selftest")]
fn fault_at(place: &str) {
    let struck = FAULT_AT
        .lock()
        .ok()
        .is_some_and(|mut fault| fault.take_if(|at| *at == place).is_some());
    assert!(!struck, "a fault for the selftest at {place}");
}

macro_rules! fault_point {
    ($place:literal) => {
        #[cfg(feature = "selftest")]
        fault_at($place);
    };
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
    /// The text is being replaced by what a file holds, which is no change the user made.
    loading: Cell<bool>,
    /// The only state `dataOfType` reads, since AppKit calls that on another thread.
    file: std::sync::Mutex<FileState>,
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
            guarded((), || {
                let text = self.ivars().initial_text.take().unwrap_or_else(|| self.text());
                let controller = with_raised(&self.ivars().loading, || EditorController::new(self.mtm(), self, &text));
                self.with_file(|file| file.loaded(Print::of(text.as_bytes()), None));
                *self.ivars().editor.borrow_mut() = Some(Weak::from_retained(&controller));
                self.addWindowController(&controller);
            });
        }

        // AppKit then reads on another thread, so that a volume that hangs does not stop the app. Nothing else knows the
        // document before it comes back, which is what makes its state safe to touch in `readFromData`.
        #[unsafe(method(canConcurrentlyReadDocumentsOfType:))]
        fn can_concurrently_read(_type_name: &NSString) -> bool {
            true
        }

        #[unsafe(method(readFromData:ofType:error:))]
        fn read_from_data(&self, data: &NSData, _type_name: &NSString, error: *mut *mut NSError) -> bool {
            let read = guarded(false, || self.take_bytes(data.to_vec()));
            if !read && !error.is_null() {
                let e = read_error();
                // SAFETY: AppKit passes a valid pointer for the error, checked for null above.
                unsafe { *error = Retained::autorelease_ptr(e) };
            }
            read
        }

        // AppKit tells of a failed save nothing else: the token below comes back only from one that succeeded.
        #[unsafe(method(saveToURL:ofType:forSaveOperation:completionHandler:))]
        fn save_to_url(
            &self,
            url: &NSURL,
            type_name: &NSString,
            operation: NSSaveOperationType,
            handler: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            guarded((), || {
                fault_point!("the start of a save");
                self.with_file(FileState::begin_save);
            });
            #[cfg(feature = "selftest")]
            {
                eprintln!("SELFTEST NOTE a save begins, operation {}", operation.0);
                if operation == NSSaveOperationType::SaveOperation {
                    SAVES_ASKED_FOR.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
            let handler = handler.copy();
            let this = self.retain();
            let ended = RcBlock::new(move |error: *mut NSError| {
                #[cfg(feature = "selftest")]
                // SAFETY: AppKit passes a valid error or null for the length of the call.
                eprintln!("SELFTEST NOTE a save ends, error {:?}", unsafe { error.as_ref() }.map(|e| e.code()));
                guarded((), || {
                    fault_point!("the end of a save");
                    // SAFETY: AppKit passes a valid error or null for the length of the call.
                    let asks = unsafe { error.as_ref() }.is_some_and(|e| e.localizedRecoveryOptions().is_some());
                    this.with_file(|file| if asks { file.end_save_that_asks() } else { file.end_save() });
                });
                handler.call((error,));
            });
            // SAFETY: NSDocument's implementation, with this method's signature; it copies the block.
            let _: () = unsafe {
                msg_send![super(self), saveToURL: url, ofType: type_name, forSaveOperation: operation, completionHandler: &*ended]
            };
        }

        // Taken with the token that marks the document as saved, on the main thread: its text views are out of reach of
        // `dataOfType`, and file and token then stand for the same text, however long this save waits behind another.
        #[unsafe(method_id(changeCountTokenForSaveOperation:))]
        fn change_count_token(&self, operation: NSSaveOperationType) -> Retained<AnyObject> {
            // After a panic the save finds no text and fails, as it should.
            guarded((), || {
                fault_point!("the text a save takes");
                let bytes = self.encoded_text();
                let text = self.text_print();
                self.with_file(|file| file.take_text(text, bytes));
            });
            // SAFETY: NSDocument's implementation, with this method's signature.
            unsafe { msg_send![super(self), changeCountTokenForSaveOperation: operation] }
        }

        // AppKit calls this once a save has written its file, and clears the change mark unless an undo step closed in the
        // meantime, whatever was typed.
        #[unsafe(method(updateChangeCountWithToken:forSaveOperation:))]
        fn update_change_count_with_token(&self, token: &AnyObject, operation: NSSaveOperationType) {
            // SAFETY: NSDocument's implementation, with this method's signature.
            let _: () = unsafe { msg_send![super(self), updateChangeCountWithToken: token, forSaveOperation: operation] };
            guarded((), || {
                fault_point!("the mark after a save");
                if writes_the_documents_file(operation) {
                    self.with_file(FileState::written);
                }
                self.match_mark_to_text(false);
            });
        }

        // Undoing moves the count by one step, however many saves that step reaches across.
        #[unsafe(method(updateChangeCount:))]
        fn update_change_count(&self, change: NSDocumentChangeType) {
            // SAFETY: NSDocument's implementation, with this method's signature.
            let _: () = unsafe { msg_send![super(self), updateChangeCount: change] };
            guarded((), || {
                if change == NSDocumentChangeType::ChangeReadOtherContents {
                    self.with_file(FileState::read_from_elsewhere);
                } else if change == NSDocumentChangeType::ChangeUndone || change == NSDocumentChangeType::ChangeRedone {
                    self.match_mark_to_text(true);
                }
            });
        }

        // AppKit then writes on another thread, keeping the main thread waiting only until `dataOfType` has returned.
        #[unsafe(method(canAsynchronouslyWriteToURL:ofType:forSaveOperation:))]
        fn can_asynchronously_write(
            &self,
            _url: &NSURL,
            _type_name: &NSString,
            _operation: NSSaveOperationType,
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
            main_soon(check);
            // A change within about half a second after Splat's own save comes without a report of its own.
            main_after(std::time::Duration::from_secs(1), check);
        }

        #[unsafe(method_id(dataOfType:error:))]
        fn data_of_type(&self, _type_name: &NSString, error: *mut *mut NSError) -> Option<Retained<NSData>> {
            let data = guarded(None, || {
                fault_point!("the bytes a save writes");
                self.data_to_write()
            });
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
            let operation = guarded(None, || self.editor().map(|editor| editor.print_operation(&self.printInfo())));
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

/// Saves that began because someone asked for one, unlike an autosave.
#[cfg(feature = "selftest")]
pub static SAVES_ASKED_FOR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Rereads every document whose file another program changed.
pub fn reload_changed_documents(mtm: MainThreadMarker) {
    look_at_files(mtm);
}

/// Looks at the files of open documents every two seconds: macOS does not report every change by another program, and
/// in a long session its reports for a document can stop altogether.
pub fn watch_files(mtm: MainThreadMarker) {
    let block = RcBlock::new(move |_: NonNull<NSTimer>| {
        guarded((), || {
            look_at_files(mtm);
            crate::editor::look_at_images(mtm);
        });
    });
    // SAFETY: The block only touches main-thread objects, and the timer runs on the main run loop it is scheduled on.
    let timer = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(2.0, true, &block) };
    timer.setTolerance(1.0);
}

/// A look at the files of a volume is under way; one on a volume that stopped answering must not pile up behind it.
static LOOKING: BusyVolumes = BusyVolumes::new();

/// A file that grew beyond this while it was open is not read again behind the user's back.
const MAX_RELOAD_BYTES: u64 = 4 << 30;

/// Whether a save puts the text where the document's own file is, not into a copy or among the drafts.
fn writes_the_documents_file(operation: NSSaveOperationType) -> bool {
    operation == NSSaveOperationType::SaveOperation
        || operation == NSSaveOperationType::SaveAsOperation
        || operation == NSSaveOperationType::AutosaveInPlaceOperation
        || operation == NSSaveOperationType::AutosaveAsOperation
}

/// What another thread found at a document's path.
struct FileLook {
    path: String,
    now: Stamp,
    /// What the document gave for it when it was asked for.
    ticket: Option<u64>,
    found: Found,
}

enum Found {
    /// The file looks as it did when the document last compared it.
    Unchanged,
    Content {
        bytes: Vec<u8>,
        print: Print,
        /// In seconds.
        date: Option<f64>,
    },
    Unreadable,
}

/// Documents with unsaved changes keep them, so their files wait until those are saved; AppKit asks before saving over
/// another version.
fn look_at_files(mtm: MainThreadMarker) {
    let mut by_volume: std::collections::BTreeMap<String, Vec<(String, Stamp, Option<u64>)>> = Default::default();
    for document in documents(mtm).iter().filter(|d| !d.isDocumentEdited()) {
        if let Some(path) = document.fileURL().and_then(|u| u.path()).map(|p| p.to_string()) {
            by_volume
                .entry(volume_of(std::path::Path::new(&path)))
                .or_default()
                .push((
                    path,
                    document.ivars().seen.get(),
                    document.with_file(|file| file.look_ticket()),
                ));
        }
    }
    #[cfg(feature = "selftest")]
    if by_volume.is_empty() {
        LOOKS_TAKEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    for (volume, requests) in by_volume {
        let Some(on_volume) = LOOKING.enter(&volume) else {
            continue;
        };
        lane_of_volume(&volume).run(move || {
            let looks: Vec<FileLook> = requests
                .into_iter()
                .map(|(path, seen, ticket)| look(path, seen, ticket))
                .collect();
            main_soon(move || {
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
                        document.take_look(look);
                    }
                }
                #[cfg(feature = "selftest")]
                LOOKS_TAKEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            });
        });
    }
}

fn look(path: String, seen: Stamp, ticket: Option<u64>) -> FileLook {
    let now = stamp(std::path::Path::new(&path));
    let found = if now == seen {
        Found::Unchanged
    } else {
        match crate::util::read_regular_file(std::path::Path::new(&path), MAX_RELOAD_BYTES) {
            Some(bytes) => {
                let date = NSFileManager::defaultManager()
                    .attributesOfItemAtPath_error(&ns(&path))
                    .ok()
                    .and_then(|attributes| attributes.fileModificationDate());
                Found::Content {
                    print: Print::of(&bytes),
                    bytes,
                    date: date.map(|d| d.timeIntervalSinceReferenceDate()),
                }
            }
            None => Found::Unreadable,
        }
    };
    FileLook {
        path,
        now,
        ticket,
        found,
    }
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
        let file = Print::of(&bytes);
        let Some((encoding, text)) = Encoding::read(bytes) else {
            return false;
        };
        let (format, text) = FileFormat::of(&text);
        self.ivars().format.set(FileFormat { encoding, ..format });
        match self.editor() {
            Some(editor) => {
                with_raised(&self.ivars().loading, || {
                    fault_point!("a reload");
                    editor.replace_all_text(text);
                });
            }
            None => self.set_initial_text(text),
        }
        self.with_file(|state| state.loaded(Print::of(text.as_bytes()), Some(file)));
        true
    }

    /// `FileState` is held only for this short: AppKit waits for it on its writing thread.
    fn with_file<R>(&self, f: impl FnOnce(&mut FileState) -> R) -> R {
        let mut file = self
            .ivars()
            .file
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut file)
    }

    /// The source view changed its text, by whatever way: typing, a command, undoing, a formula's editor.
    pub fn text_changed(&self) {
        self.with_file(FileState::text_changed);
        self.match_mark_to_text(true);
    }

    /// Makes the change mark say what the text says. `may_clear` is for what brings the text back to the file's, as
    /// undoing does; an undo step that closes after it must stay free to mark the document as AppKit sees fit.
    fn match_mark_to_text(&self, may_clear: bool) {
        if self.ivars().loading.get() {
            return;
        }
        let edited = self.isDocumentEdited();
        if edited && !may_clear {
            return;
        }
        let Some(editor) = self.editor() else { return };
        let on_file = match self.with_file(|file| file.on_file()) {
            OnFile::Yes => true,
            OnFile::No => false,
            OnFile::IfTextIs(print) => {
                let same = editor.on_source_text(move |text| print.is_of(text.as_bytes()));
                if same {
                    self.with_file(FileState::text_back_on_file);
                }
                same
            }
        };
        let change = match mark_for(edited, on_file, may_clear) {
            ChangeMark::Keep => return,
            ChangeMark::Set => NSDocumentChangeType::ChangeDone,
            ChangeMark::Clear => NSDocumentChangeType::ChangeCleared,
        };
        // SAFETY: NSDocument's implementation, with this method's signature.
        let _: () = unsafe { msg_send![super(self), updateChangeCount: change] };
    }

    fn text_print(&self) -> Print {
        match self.editor() {
            Some(editor) => editor.on_source_text(|text| Print::of(text.as_bytes())),
            None => Print::of(
                self.ivars()
                    .initial_text
                    .borrow()
                    .as_deref()
                    .unwrap_or_default()
                    .as_bytes(),
            ),
        }
    }

    /// What saving writes now, from whichever thread AppKit saves on.
    fn data_to_write(&self) -> Option<Retained<NSData>> {
        if MainThreadMarker::new().is_some() {
            let bytes = self.encoded_text()?;
            let text = self.text_print();
            self.with_file(|file| file.writes_at_once(text, &bytes));
            return Some(NSData::with_bytes(&bytes));
        }
        self.with_file(|file| file.start_writing().map(NSData::with_bytes))
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

    /// A document as one is after its window closed: neither an editor nor the text it took over.
    #[cfg(feature = "selftest")]
    pub fn without_text(mtm: MainThreadMarker) -> Retained<MdDocument> {
        // SAFETY: NSDocument's initializer, called once on the newly allocated object.
        unsafe { msg_send![MdDocument::alloc(mtm), init] }
    }

    /// Whether the document holds the text beside its editor.
    #[cfg(feature = "selftest")]
    pub fn keeps_a_copy_of_its_text(&self) -> bool {
        self.ivars().initial_text.borrow().is_some() || self.with_file(|file| file.bytes().is_some())
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
    fn take_look(&self, look: FileLook) {
        self.match_mark_to_text(false);
        if self.isDocumentEdited() || !self.with_file(|file| file.look_counts(look.ticket)) {
            return;
        }
        let FileLook { now, found, .. } = look;
        let read = match found {
            Found::Unchanged => true,
            Found::Unreadable => false,
            Found::Content { bytes, print, date } => self.take_content(bytes, print, date),
        };
        if read || !self.another_try(now) {
            self.ivars().seen.set(now);
        }
    }

    /// `false` if the content is no text Splat can read.
    fn take_content(&self, bytes: Vec<u8>, print: Print, date: Option<f64>) -> bool {
        let date = date.map(NSDate::dateWithTimeIntervalSinceReferenceDate);
        let own = self.with_file(|file| file.holds(print));
        match reaction(own, || self.shows(&bytes)) {
            Reaction::TakeDate => {
                if !own {
                    let text = self.text_print();
                    self.with_file(|file| file.loaded(text, Some(print)));
                }
                // Otherwise AppKit would warn about a change by another program when Splat saves next.
                if let Some(date) = date {
                    self.setFileModificationDate(Some(&date));
                }
                true
            }
            // The bytes are at hand: asking AppKit to revert would read the file again on the main thread.
            Reaction::Reread if self.take_bytes(bytes) => {
                self.updateChangeCount(NSDocumentChangeType::ChangeCleared);
                if let Some(date) = date {
                    self.setFileModificationDate(Some(&date));
                }
                // Undo steps point into the old text.
                if let Some(undo) = self.undoManager() {
                    undo.removeAllActions();
                }
                true
            }
            Reaction::Reread => false,
        }
    }

    /// A read of the version `now` failed; whether it is worth another one.
    fn another_try(&self, now: Stamp) -> bool {
        let failed = self.ivars().failed_reads.get().after_another(now);
        self.ivars().failed_reads.set(failed);
        failed.try_again()
    }

    pub fn set_initial_text(&self, text: &str) {
        *self.ivars().initial_text.borrow_mut() = Some(text.to_string());
    }
}
