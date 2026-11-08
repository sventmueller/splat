use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use block2::RcBlock;
use objc2::rc::{Allocated, Retained, Weak};
use objc2::runtime::{AnyObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSDocument, NSDocumentChangeType, NSDocumentController, NSPrintOperation, NSSaveOperationType};
use objc2_foundation::{
    NSData, NSDate, NSDictionary, NSError, NSFileManager, NSObjectProtocol, NSString, NSTimer, NSURL,
};
use splat_file_state::{
    ChangeMark, Encoding, FailedReads, FileFormat, FileState, Fingerprint, LookTicket, OnFile, Reaction, SaveTicket,
    Stamp, mark_for, reaction,
};

use crate::editor::EditorController;
use crate::test_hooks;
use crate::util::constant;
use crate::util::{BusyVolumes, guarded, lane_of_volume, main_after, main_soon, ns, stamp, volume_of, with_raised};

/// What a document holds besides what NSDocument holds.
pub struct Ivars {
    main: OnMain,
    shared: Arc<Shared>,
    _listed: Listed,
}

impl Ivars {
    fn new(address: usize) -> Ivars {
        let shared = Arc::new(Shared::default());
        Ivars {
            main: OnMain::default(),
            _listed: Listed::new(address, &shared),
            shared,
        }
    }
}

/// The part of a document that only the main thread touches.
#[derive(Default)]
struct OnMain {
    editor: RefCell<Option<Weak<EditorController>>>,
    /// How the file looked when its content was last compared with the text.
    seen: Cell<Stamp>,
    failed_reads: Cell<FailedReads>,
    /// The text is being replaced by what a file holds, which is no change the user made.
    loading: Cell<bool>,
}

/// The part of a document that AppKit reaches on its own threads: it opens a document there, writes a save there and
/// reports a change of the file there.
#[derive(Default)]
struct Shared {
    file: Mutex<FileState>,
    contents: Mutex<Contents>,
}

/// What was read from the file, until the editor takes the text over and from then on holds the only copy.
#[derive(Default)]
struct Contents {
    initial_text: Option<String>,
    format: FileFormat,
}

/// The text a file holds and how it holds it.
struct Read {
    text: String,
    format: FileFormat,
    file: Fingerprint,
}

impl Read {
    /// `None` if the bytes are no text Splat can read.
    fn of(bytes: Vec<u8>) -> Option<Read> {
        let file = Fingerprint::of(&bytes);
        let (encoding, mut text) = Encoding::read(bytes)?;
        let (format, mark) = {
            let (format, rest) = FileFormat::of(&text);
            (format, text.len() - rest.len())
        };
        text.drain(..mark);
        Some(Read {
            text,
            format: FileFormat { encoding, ..format },
            file,
        })
    }
}

impl Shared {
    /// `FileState` is held only for this short: AppKit waits for it on its writing thread.
    fn with_file<R>(&self, f: impl FnOnce(&mut FileState) -> R) -> R {
        f(&mut self.file.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn with_contents<R>(&self, f: impl FnOnce(&mut Contents) -> R) -> R {
        f(&mut self.contents.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Keeps what was read for an editor still to come.
    fn hold(&self, read: Read) {
        let text = Fingerprint::of(read.text.as_bytes());
        self.with_contents(|contents| {
            contents.format = read.format;
            contents.initial_text = Some(read.text);
        });
        self.with_file(|file| file.loaded(text, Some(read.file)));
    }

    /// The content of a file becomes the text of a document without an editor; `false` if it is no text Splat can read.
    fn read(&self, bytes: Vec<u8>) -> bool {
        let Some(read) = Read::of(bytes) else {
            return false;
        };
        self.hold(read);
        true
    }

    fn take_initial_text(&self) -> Option<String> {
        self.with_contents(|contents| contents.initial_text.take())
    }

    /// The shared part of the document at `address`, for as long as that document exists.
    fn of(address: usize) -> Option<Arc<Shared>> {
        listed().get(&address).cloned()
    }
}

/// The shared part of every document by the document's address, which a method that AppKit calls on another thread
/// reaches without a reference to the document, as only the main thread may hold one.
static LISTED: Mutex<BTreeMap<usize, Arc<Shared>>> = Mutex::new(BTreeMap::new());

fn listed() -> MutexGuard<'static, BTreeMap<usize, Arc<Shared>>> {
    LISTED.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A document's entry in `LISTED`. The ivars drop it when the document is freed, before its address can be taken again.
struct Listed(usize);

impl Listed {
    fn new(address: usize, shared: &Arc<Shared>) -> Listed {
        listed().insert(address, shared.clone());
        Listed(address)
    }
}

impl Drop for Listed {
    fn drop(&mut self) {
        listed().remove(&self.0);
    }
}

/// A save between its start and its completion handler. It ends the save in `FileState` once, however the handler goes:
/// called, panicking, or dropped by AppKit without a call.
struct SaveUnderWay {
    shared: Arc<Shared>,
    ticket: Option<SaveTicket>,
}

impl SaveUnderWay {
    fn begin(shared: &Arc<Shared>) -> SaveUnderWay {
        SaveUnderWay {
            ticket: Some(shared.with_file(FileState::begin_save)),
            shared: shared.clone(),
        }
    }

    /// `asks` when AppKit follows the save with a question, as about a file another program changed.
    fn end(mut self, asks: bool) {
        if let Some(ticket) = self.ticket.take() {
            self.shared.with_file(|file| {
                if asks {
                    file.end_save_that_asks(ticket);
                } else {
                    file.end_save(ticket);
                }
            });
        }
    }
}

impl Drop for SaveUnderWay {
    fn drop(&mut self) {
        if let Some(ticket) = self.ticket.take() {
            self.shared.with_file(|file| file.end_save(ticket));
        }
    }
}

define_class!(
    /// A Markdown document, as AppKit's document architecture opens, shows and saves it.
    // objc2 allows a subclass of NSDocument no other thread kind. The methods AppKit also calls on other threads take
    // `this: *const Self`, and reach only `Shared` there.
    #[unsafe(super(NSDocument))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDDocument"]
    #[ivars = Ivars]
    pub struct MdDocument;

    unsafe impl NSObjectProtocol for MdDocument {}

    impl MdDocument {
        // Also on the thread that opens the document, where nothing else knows it yet.
        #[unsafe(method_id(init))]
        fn init(this: Allocated<Self>) -> Option<Retained<Self>> {
            let ivars = Ivars::new(Allocated::as_ptr(&this).addr());
            let this = this.set_ivars(ivars);
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
                let text = self.shared().take_initial_text().unwrap_or_else(|| self.text());
                let controller = with_raised(&self.main().loading, || EditorController::new(self.mtm(), self, &text));
                self.with_file(|file| file.loaded(Fingerprint::of(text.as_bytes()), None));
                *self.main().editor.borrow_mut() = Some(Weak::from_retained(&controller));
                self.addWindowController(&controller);
            });
        }

        // AppKit then makes the document on another thread, so that a volume that hangs does not stop the app.
        #[unsafe(method(canConcurrentlyReadDocumentsOfType:))]
        fn can_concurrently_read(_type_name: &NSString) -> bool {
            true
        }

        // On another thread when the document controller opens the document, on the main thread when it reverts.
        #[unsafe(method(readFromData:ofType:error:))]
        fn read_from_data(this: *const Self, data: &NSData, _type_name: &NSString, error: *mut *mut NSError) -> bool {
            let read = guarded(false, || {
                // SAFETY: AppKit calls this on the document it reads into and keeps it alive for the call.
                match unsafe { Self::on_main_thread(this) } {
                    Some(document) => document.take_bytes(data.to_vec()),
                    None => Self::elsewhere(this).is_some_and(|shared| shared.read(data.to_vec())),
                }
            });
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
            let save = guarded(None, || {
                test_hooks::fault_at("the start of a save");
                Some(SaveUnderWay::begin(&self.ivars().shared))
            });
            test_hooks::note(|| format!("a save begins, operation {}", operation.0));
            if operation == NSSaveOperationType::SaveOperation {
                test_hooks::count(&test_hooks::SAVES_ASKED_FOR);
            }
            let handler = handler.copy();
            let save = Cell::new(save);
            let ended = crate::util::error_handler(move |error| {
                // AppKit's own handler has to run also after a panic in what ends the save.
                guarded((), || {
                    let save = save.take();
                    let code = error.map(|e| e.code());
                    test_hooks::note(|| format!("a save ends, error {code:?}"));
                    test_hooks::fault_at("the end of a save");
                    let asks = error.is_some_and(|e| e.localizedRecoveryOptions().is_some());
                    if let Some(save) = save {
                        save.end(asks);
                    }
                });
                handler.call((error.map_or(std::ptr::null_mut(), |e| std::ptr::from_ref(e).cast_mut()),));
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
                test_hooks::fault_at("the text a save takes");
                let bytes = self.encoded_text();
                let text = self.text_fingerprint();
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
                test_hooks::fault_at("the mark after a save");
                if writes_the_documents_file(operation) {
                    self.with_file(FileState::written);
                }
                self.match_mark_to_text(false);
            });
        }

        // Undoing moves the count by one step, however many saves that step reaches across. A restored draft reports that
        // it was read from elsewhere on the thread that opens it.
        #[unsafe(method(updateChangeCount:))]
        fn update_change_count(this: *const Self, change: NSDocumentChangeType) {
            // SAFETY: NSDocument's implementation, with this method's signature; AppKit calls it on other threads too.
            let _: () = unsafe { msg_send![super(this), updateChangeCount: change] };
            guarded((), || {
                if change == NSDocumentChangeType::ChangeReadOtherContents {
                    if let Some(shared) = Self::elsewhere(this) {
                        shared.with_file(FileState::read_from_elsewhere);
                    }
                } else if change == NSDocumentChangeType::ChangeUndone || change == NSDocumentChangeType::ChangeRedone {
                    // SAFETY: AppKit calls this on the document whose count changes and keeps it alive for the call.
                    if let Some(document) = unsafe { Self::on_main_thread(this) } {
                        document.match_mark_to_text(true);
                    }
                }
            });
        }

        // AppKit then writes on another thread, keeping the main thread waiting only until `dataOfType` has returned.
        #[unsafe(method(canAsynchronouslyWriteToURL:ofType:forSaveOperation:))]
        fn can_asynchronously_write(
            _this: *const Self,
            _url: &NSURL,
            _type_name: &NSString,
            _operation: NSSaveOperationType,
        ) -> bool {
            true
        }

        // AppKit reports a change by another program here, on the queue of the file presenter, but keeps showing the old
        // text.
        #[unsafe(method(presentedItemDidChange))]
        fn presented_item_did_change(this: *const Self) {
            // SAFETY: NSDocument's implementation, with this method's signature; the file presenter calls it there.
            let _: () = unsafe { msg_send![super(this), presentedItemDidChange] };
            let check = || {
                if let Some(mtm) = MainThreadMarker::new() {
                    reload_changed_documents(mtm);
                }
            };
            main_soon(check);
            // A change within about half a second after Splat's own save comes without a report of its own.
            main_after(std::time::Duration::from_secs(1), check);
        }

        // On the writing thread of a save that writes there, on the main thread otherwise.
        #[unsafe(method_id(dataOfType:error:))]
        fn data_of_type(this: *const Self, _type_name: &NSString, error: *mut *mut NSError) -> Option<Retained<NSData>> {
            let data = guarded(None, || {
                test_hooks::fault_at("the bytes a save writes");
                // SAFETY: AppKit calls this on the document it saves and keeps it alive for the call.
                match unsafe { Self::on_main_thread(this) } {
                    Some(document) => document.data_to_write(),
                    None => Self::elsewhere(this)?.with_file(|file| file.start_writing().map(NSData::with_bytes)),
                }
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
            crate::editor::look_at_repositories();
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
    ticket: LookTicket,
    found: Found,
}

enum Found {
    /// The file looks as it did when the document last compared it.
    Unchanged,
    Content {
        bytes: Vec<u8>,
        fingerprint: Fingerprint,
        /// In seconds.
        date: Option<f64>,
    },
    Unreadable,
}

/// Documents with unsaved changes keep them, so their files wait until those are saved; AppKit asks before saving over
/// another version.
fn look_at_files(mtm: MainThreadMarker) {
    let mut by_volume: BTreeMap<String, Vec<(String, Stamp, LookTicket)>> = BTreeMap::new();
    for document in documents(mtm).iter().filter(|d| !d.isDocumentEdited()) {
        if let Some(path) = document.fileURL().and_then(|u| u.path()).map(|p| p.to_string()) {
            by_volume
                .entry(volume_of(std::path::Path::new(&path)))
                .or_default()
                .push((
                    path,
                    document.main().seen.get(),
                    document.with_file(|file| file.look_ticket()),
                ));
        }
    }
    if by_volume.is_empty() {
        test_hooks::count(&test_hooks::LOOKS_TAKEN);
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
                test_hooks::count(&test_hooks::LOOKS_TAKEN);
            });
        });
    }
}

fn look(path: String, seen: Stamp, ticket: LookTicket) -> FileLook {
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
                    fingerprint: Fingerprint::of(&bytes),
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
    NSError::new(512, constant!(objc2_foundation::NSCocoaErrorDomain))
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
    /// The receiver of a method that AppKit may call on any thread, if this is the main thread.
    ///
    /// # Safety
    ///
    /// `this` is the receiver of the method being called, which AppKit keeps alive for the call.
    unsafe fn on_main_thread<'a>(this: *const Self) -> Option<&'a Self> {
        MainThreadMarker::new()?;
        // SAFETY: On the main thread, where a main-thread-only object may be referenced, and the caller vouches for the
        // pointer.
        Some(unsafe { &*this })
    }

    /// The part of the receiver that is safe on whatever thread AppKit calls it, found without a reference to it.
    fn elsewhere(this: *const Self) -> Option<Arc<Shared>> {
        Shared::of(this.addr())
    }

    /// What only the main thread may touch.
    fn main(&self) -> &OnMain {
        debug_assert!(
            MainThreadMarker::new().is_some(),
            "the main-thread state of a document off the main thread"
        );
        &self.ivars().main
    }

    fn shared(&self) -> &Shared {
        &self.ivars().shared
    }

    /// The editor that shows the document, once its window controllers exist.
    pub fn editor(&self) -> Option<Retained<EditorController>> {
        self.main().editor.borrow().as_ref().and_then(Weak::load)
    }

    /// The source text, from the editor or, before there is one, as read.
    pub fn text(&self) -> String {
        match self.editor() {
            Some(editor) => editor.source_text(),
            None => self
                .shared()
                .with_contents(|contents| contents.initial_text.clone())
                .unwrap_or_default(),
        }
    }

    /// The content of a file now becomes the document's; `false` if it is no text Splat can read.
    fn take_bytes(&self, bytes: Vec<u8>) -> bool {
        let Some(read) = Read::of(bytes) else {
            return false;
        };
        let Some(editor) = self.editor() else {
            self.shared().hold(read);
            return true;
        };
        self.shared().with_contents(|contents| contents.format = read.format);
        with_raised(&self.main().loading, || {
            test_hooks::fault_at("a reload");
            editor.replace_all_text(&read.text);
        });
        let text = Fingerprint::of(read.text.as_bytes());
        self.with_file(|file| file.loaded(text, Some(read.file)));
        true
    }

    fn with_file<R>(&self, f: impl FnOnce(&mut FileState) -> R) -> R {
        self.shared().with_file(f)
    }

    /// The source view changed its text, by whatever way: typing, a command, undoing, a formula's editor.
    pub fn text_changed(&self) {
        self.with_file(FileState::text_changed);
        self.match_mark_to_text(true);
    }

    /// Makes the change mark say what the text says. `may_clear` is for what brings the text back to the file's, as
    /// undoing does; an undo step that closes after it must stay free to mark the document as AppKit sees fit.
    fn match_mark_to_text(&self, may_clear: bool) {
        if self.main().loading.get() {
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
            OnFile::IfTextIs(fingerprint) => {
                let same = editor.on_source_text(move |text| fingerprint.is_of(text.as_bytes()));
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

    fn text_fingerprint(&self) -> Fingerprint {
        match self.editor() {
            Some(editor) => editor.on_source_text(|text| Fingerprint::of(text.as_bytes())),
            None => self.shared().with_contents(|contents| {
                Fingerprint::of(contents.initial_text.as_deref().unwrap_or_default().as_bytes())
            }),
        }
    }

    /// What a save on the main thread writes now.
    fn data_to_write(&self) -> Option<Retained<NSData>> {
        let bytes = self.encoded_text()?;
        let text = self.text_fingerprint();
        self.with_file(|file| file.writes_at_once(text, &bytes));
        Some(NSData::with_bytes(&bytes))
    }

    /// `None` once the editor that held the text is gone: a save must then fail rather than empty the file.
    fn encoded_text(&self) -> Option<Vec<u8>> {
        match self.editor() {
            Some(editor) => {
                let format = self.shared().with_contents(|contents| contents.format);
                Some(format.encode(&editor.exact_text()))
            }
            None => self.shared().with_contents(|contents| {
                let format = contents.format;
                contents.initial_text.as_deref().map(|text| format.encode(text))
            }),
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
        self.shared().with_contents(|contents| contents.initial_text.is_some())
            || self.with_file(|file| file.bytes().is_some())
    }

    /// Whether `bytes` are what saving the document now would write.
    fn shows(&self, bytes: &[u8]) -> bool {
        let format = self.shared().with_contents(|contents| contents.format);
        match self.editor() {
            Some(editor) => editor.on_source_text(|text| format.encode(text) == bytes),
            None => self.shared().with_contents(|contents| {
                contents
                    .initial_text
                    .as_deref()
                    .is_some_and(|text| format.encode(text) == bytes)
            }),
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
            Found::Content {
                bytes,
                fingerprint,
                date,
            } => self.take_content(bytes, fingerprint, date),
        };
        if read || !self.another_try(now) {
            self.main().seen.set(now);
        }
    }

    /// `false` if the content is no text Splat can read.
    fn take_content(&self, bytes: Vec<u8>, fingerprint: Fingerprint, date: Option<f64>) -> bool {
        let date = date.map(NSDate::dateWithTimeIntervalSinceReferenceDate);
        let own = self.with_file(|file| file.is_own_file(fingerprint));
        match reaction(own, || self.shows(&bytes)) {
            Reaction::TakeDate => {
                if !own {
                    let text = self.text_fingerprint();
                    self.with_file(|file| file.loaded(text, Some(fingerprint)));
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
        let failed = self.main().failed_reads.get().after_another(now);
        self.main().failed_reads.set(failed);
        failed.try_again()
    }

    /// The text a document without an editor starts with.
    pub fn set_initial_text(&self, text: &str) {
        self.shared()
            .with_contents(|contents| contents.initial_text = Some(text.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared_across_threads<T: Send + Sync>() {}

    /// An address that belongs to no document while it lives.
    fn address(place: &u64) -> usize {
        std::ptr::from_ref(place).addr()
    }

    #[test]
    fn what_other_threads_reach_is_safe_there() {
        shared_across_threads::<Shared>();
        shared_across_threads::<SaveUnderWay>();
        shared_across_threads::<Arc<Shared>>();
    }

    #[test]
    fn a_file_read_on_another_thread_waits_for_the_editor_in_its_format() {
        let shared = Arc::new(Shared::default());
        let file = b"\xEF\xBB\xBF# A\r\nB\r\n".to_vec();
        let reader = shared.clone();
        let bytes = file.clone();
        assert!(std::thread::spawn(move || reader.read(bytes)).join().unwrap());
        let format = shared.with_contents(|contents| contents.format);
        assert!(format.bom && format.crlf, "{format:?}");
        assert_eq!(shared.take_initial_text().as_deref(), Some("# A\r\nB\r\n"));
        assert_eq!(shared.take_initial_text(), None, "the editor holds the only copy");
        assert!(shared.with_file(|state| state.is_own_file(Fingerprint::of(&file))));
        assert_eq!(shared.with_file(|state| state.on_file()), OnFile::Yes);
    }

    #[test]
    fn a_file_that_is_no_text_leaves_the_document_as_it_was() {
        let shared = Shared::default();
        assert!(!shared.read(b"Gr\xFC\xDFe".to_vec()));
        assert_eq!(shared.take_initial_text(), None);
        assert!(shared.with_file(|state| state.is_own_file(Fingerprint::default())));
    }

    #[test]
    fn a_utf16_file_reads_as_its_text_without_the_mark() {
        let read = Read::of(b"\xFF\xFEa\0\xE4\0".to_vec()).expect("text");
        assert_eq!(read.text, "aä");
        assert_eq!(read.format.encode(&read.text), b"\xFF\xFEa\0\xE4\0");
    }

    #[test]
    fn a_save_ends_once_however_its_handler_goes() {
        let shared = Arc::new(Shared::default());
        let looks_count = |shared: &Shared| shared.with_file(|state| state.look_counts(state.look_ticket()));
        let take =
            |shared: &Shared| shared.with_file(|state| state.take_text(Fingerprint::of(b"T"), Some(b"T".to_vec())));

        let save = SaveUnderWay::begin(&shared);
        assert!(!looks_count(&shared), "under way");
        save.end(false);
        assert!(looks_count(&shared), "called");

        let save = SaveUnderWay::begin(&shared);
        take(&shared);
        drop(save);
        assert!(looks_count(&shared), "dropped without a call");
        assert_eq!(shared.with_file(|state| state.bytes().map(<[u8]>::to_vec)), None);

        let save = SaveUnderWay::begin(&shared);
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _held = save;
            panic!("a bug in the handler");
        }));
        assert!(panicked.is_err());
        assert!(looks_count(&shared), "panicked");

        let save = SaveUnderWay::begin(&shared);
        take(&shared);
        save.end(true);
        assert!(looks_count(&shared));
        assert_eq!(
            shared.with_file(|state| state.bytes().map(<[u8]>::to_vec)),
            Some(b"T".to_vec()),
            "kept for the answer to the question"
        );
    }

    #[test]
    fn overlapping_saves_on_several_threads_each_end_once() {
        let shared = Arc::new(Shared::default());
        let saves: Vec<SaveUnderWay> = (0..8).map(|_| SaveUnderWay::begin(&shared)).collect();
        let threads: Vec<_> = saves
            .into_iter()
            .enumerate()
            .map(|(n, save)| std::thread::spawn(move || if n % 2 == 0 { save.end(false) } else { drop(save) }))
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert!(shared.with_file(|state| state.look_counts(state.look_ticket())));
    }

    #[test]
    fn the_shared_part_is_found_by_address_until_its_document_goes() {
        let place = 0u64;
        let shared = Arc::new(Shared::default());
        let listed = Listed::new(address(&place), &shared);
        let at = address(&place);
        let found = std::thread::spawn(move || Shared::of(at)).join().unwrap();
        assert!(found.is_some_and(|found| Arc::ptr_eq(&found, &shared)));
        let other = 1u64;
        assert!(Shared::of(address(&other)).is_none());
        drop(listed);
        assert!(Shared::of(address(&place)).is_none());
        assert_eq!(Arc::strong_count(&shared), 1, "the list let go of it");
    }
}
