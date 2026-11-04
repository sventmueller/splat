//! Documents as the checks open them, each the way AppKit would.

use std::path::Path;

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly, msg_send};
use objc2_app_kit::NSDocumentController;
use objc2_foundation::{NSError, NSURL};

use crate::document::MdDocument;
use crate::editor::EditorController;
use crate::util::ns;

pub(in crate::editor::selftest) fn url_of(path: &Path) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&ns(path.to_str().unwrap_or_default()))
}

fn with_editor(document: Retained<MdDocument>) -> (Retained<MdDocument>, Retained<EditorController>) {
    document.makeWindowControllers();
    let editor = document.editor().expect("an editor");
    editor.updater().update_now();
    (document, editor)
}

/// A new document with this text, which no window shows and no document controller knows.
pub(in crate::editor::selftest) fn untitled(
    mtm: MainThreadMarker,
    text: &str,
) -> (Retained<MdDocument>, Retained<EditorController>) {
    // SAFETY: NSDocument's initializer, called once on the newly allocated object.
    let document: Retained<MdDocument> = unsafe { msg_send![MdDocument::alloc(mtm), init] };
    document.set_initial_text(text);
    with_editor(document)
}

/// The document of a file, as the document controller opens it.
pub(in crate::editor::selftest) fn opened(
    mtm: MainThreadMarker,
    path: &Path,
) -> (Retained<MdDocument>, Retained<EditorController>) {
    let url = url_of(path);
    let controller = NSDocumentController::sharedDocumentController(mtm);
    let kind = controller.typeForContentsOfURL_error(&url).expect("a document type");
    // SAFETY: NSDocument's initializer, called once on the newly allocated object.
    let document: Retained<MdDocument> = unsafe {
        msg_send![MdDocument::alloc(mtm), initWithContentsOfURL: &*url, ofType: &*kind, error: std::ptr::null_mut::<*mut NSError>()]
    };
    controller.addDocument(&document);
    with_editor(document)
}

/// A draft of an untitled document comes back after a relaunch by this initializer, and a named document's with its name.
pub(in crate::editor::selftest) fn restored(
    mtm: MainThreadMarker,
    name: Option<&Path>,
    draft: &Path,
) -> (Retained<MdDocument>, Retained<EditorController>) {
    let contents = url_of(draft);
    let controller = NSDocumentController::sharedDocumentController(mtm);
    let kind = controller
        .typeForContentsOfURL_error(&contents)
        .expect("a document type");
    let name = name.map(url_of);
    // SAFETY: NSDocument's initializer, called once on the newly allocated object.
    let document: Retained<MdDocument> = unsafe {
        msg_send![MdDocument::alloc(mtm), initForURL: name.as_deref(), withContentsOfURL: &*contents, ofType: &*kind, error: std::ptr::null_mut::<*mut NSError>()]
    };
    controller.addDocument(&document);
    with_editor(document)
}
