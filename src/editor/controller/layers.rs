//! Who may reach whom, lowest first. A component gets a handle that offers what lies below it, and `Object`, the controller
//! as AppKit knows it, which offers none of the controller's own methods. What a component needs from one above it, and
//! what arrives for it later, it sends the controller as a `Message`, and the controller hands that on. Beyond that, upward
//! go only `invalidate`, `under_way`, `image_arrived`, `toggle_preview`, `toggle_files`, `toggle_sidebar` and
//! `search_notes`. The compiler holds this, as only this module sees what the handles hold; what a component does to every
//! open window, its own among them, goes through what the controller offers the rest of the program.

use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dispatch2::MainThreadBound;
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSDocument, NSPopoverDelegate, NSSharingServicePickerToolbarItemDelegate, NSTextViewDelegate, NSWindow,
    NSWindowController,
};
use objc2_foundation::NSUndoManager;

use super::EditorController;
use crate::editor::actions::Action;
use crate::editor::atom::AtomEditor;
use crate::editor::changes::{ChangeMarks, ChangesAnswer};
use crate::editor::counter::WordCounter;
use crate::editor::editing::Editing;
use crate::editor::export::{ExportFailure, ExportKind, Sharing};
use crate::editor::images::{ImageFile, ImageStore, Pictures};
use crate::editor::js::Renderer;
use crate::editor::mathfield::MathField;
use crate::editor::model::Model;
use crate::editor::notes::{NoteAnswer, Notes, RenameRequest};
use crate::editor::panes::Panes;
use crate::editor::picture::Rendered;
use crate::editor::progressive::Progressive;
use crate::editor::render_queue::Owner;
use crate::editor::scroll::ScrollSync;
use crate::editor::selection::Selection;
use crate::editor::sidebar::SidebarTab;
use crate::editor::state::{Background, Invalidate, State};
use crate::editor::sync::Snapshot;
use crate::editor::update::Updater;
use crate::markdown::front_matter;
use crate::util::{Stamp, main_soon};

/// What reaches the controller later, from another thread or a sheet that closed, or from a component that needs one
/// above it. The controller hands it to the component it is for, in `EditorController::handle`.
pub(in crate::editor) enum Message {
    /// The link sheet closed with an address and a text.
    LinkChosen { url: String, label: String },
    /// The table sheet closed with a size.
    TableChosen { rows: usize, columns: usize },
    /// The open panel closed on an image file.
    ImageChosen(PathBuf),
    /// A pasted picture reached its file in `folder`, or did not.
    PicturePasted { written: Option<PathBuf>, folder: PathBuf },
    /// The metadata form closed with these fields.
    MetadataChosen(Vec<(String, front_matter::Value)>),
    /// A command, as the menu would run it.
    Perform(Action),
    /// A walk, a search or a rename of the notes came back.
    Notes(NoteAnswer),
    /// The user chose the folder the sidebar shows.
    NotesFolder(PathBuf),
    /// The user granted the folder around the document.
    FolderGranted,
    /// The user typed a new name for a note.
    RenameChosen { from: PathBuf, to: PathBuf },
    /// The user agreed to the changes a rename makes to the links.
    RenameConfirmed(RenameRequest, Vec<splat_notes::Rewrite>),
    /// A place of the source to select and show in both panes, as a line and a place in it.
    RevealPlace { line: usize, column: usize },
    /// A look at the repository or a comparison with it came back.
    Changes(ChangesAnswer),
    /// An image file was read.
    ImageLoaded(PathBuf, ImageFile),
    /// These image files changed on disk since they were read.
    ImagesChanged(Vec<(PathBuf, Stamp)>),
    /// The parse in the background ended, with nothing if a newer text made it pointless.
    Parsed {
        generation: u64,
        text: Arc<str>,
        background: Option<Box<Background>>,
    },
    /// The save panel of an export closed on a file.
    ExportTo {
        format: ExportKind,
        path: PathBuf,
        title: String,
    },
    /// An export did not reach its file.
    ExportFailed(ExportFailure),
}

/// The controller as code elsewhere reaches it, from any thread: a message arrives on the main thread, and only while the
/// window is open.
#[derive(Clone)]
pub(in crate::editor) struct Messenger(Option<Arc<MainThreadBound<Weak<EditorController>>>>);

impl Drop for Messenger {
    fn drop(&mut self) {
        // `MainThreadBound` drops its value elsewhere by waiting for the main thread, which may be waiting for this one.
        if let Some(to) = self.0.take()
            && MainThreadMarker::new().is_none()
        {
            main_soon(move || drop(to));
        }
    }
}

impl Messenger {
    /// The messenger of `controller`.
    pub(in crate::editor) fn of(controller: &EditorController) -> Messenger {
        Messenger(Some(Arc::new(MainThreadBound::new(
            Weak::from(controller),
            controller.mtm(),
        ))))
    }

    /// Hands `message` to the controller now, if its window is still open.
    pub(in crate::editor) fn send(&self, mtm: MainThreadMarker, message: Message) {
        let controller = self.0.as_ref().and_then(|to| to.get(mtm).load());
        if let Some(controller) = controller.filter(|c| !c.has_closed()) {
            controller.handle(message);
        }
    }

    /// Hands `message` to the controller on the main thread when its turn comes, if the window is still open then.
    pub(in crate::editor) fn send_soon(&self, message: Message) {
        let to = self.clone();
        main_soon(move || {
            if let Some(mtm) = MainThreadMarker::new() {
                to.send(mtm, message);
            }
        });
    }
}

/// The controller as AppKit knows it, which offers none of the controller's methods: to schedule its selectors, to send
/// it messages, and to hand it to AppKit or to who reports back to it.
#[derive(Clone, Copy)]
pub(in crate::editor) struct Object<'a>(&'a EditorController);

impl<'a> Object<'a> {
    /// Sends `selector` to the controller after `delay` seconds, as `crate::util::perform_later` does.
    pub(in crate::editor) fn perform_later(self, selector: Sel, delay: f64, common_modes: bool) {
        crate::util::perform_later(self.0, selector, delay, common_modes);
    }

    /// Hands `message` to the controller now.
    pub(in crate::editor) fn send(self, message: Message) {
        self.0.handle(message);
    }

    /// For what sends the controller messages later or from another thread.
    pub(in crate::editor) fn messenger(self) -> Messenger {
        Messenger::of(self.0)
    }

    /// The window's state to restore changed.
    pub(in crate::editor) fn invalidate_restorable_state(self) {
        self.0.invalidateRestorableState();
    }

    /// The controller as the target of a control or menu item.
    pub(in crate::editor) fn target(self) -> &'a AnyObject {
        self.0
    }

    /// The controller as the delegate of a popover.
    pub(in crate::editor) fn popover_delegate(self) -> &'a ProtocolObject<dyn NSPopoverDelegate> {
        ProtocolObject::from_ref(self.0)
    }

    /// The controller as the delegate of a text view.
    pub(in crate::editor) fn text_view_delegate(self) -> &'a ProtocolObject<dyn NSTextViewDelegate> {
        ProtocolObject::from_ref(self.0)
    }

    /// Asks `renderer` to tell this editor what it finishes; the number it asks under.
    pub(in crate::editor) fn listen_to(self, renderer: &Renderer) -> Owner {
        renderer.listen(self.0)
    }

    /// Tells the renderer that this editor waits for nothing anymore.
    pub(in crate::editor) fn asks_for_no_renders(self) {
        crate::editor::js::asks_for_nothing(self.0);
    }

    /// A picture from the web, as `remote::get` answers for this editor, which hears when it arrives.
    pub(in crate::editor) fn web_picture(self, url: &str) -> Option<Option<Rendered>> {
        crate::editor::remote::get(url, Some(self.0))
    }

    /// The image `url` names, as `ImageStore::file` gives it to this editor, which hears when it arrives.
    pub(in crate::editor) fn image_file(
        self,
        images: &mut ImageStore,
        url: &str,
        doc_dir: Option<&Path>,
        dark: bool,
    ) -> Option<Rendered> {
        images.file(url, doc_dir, dark, Some(self.0))
    }

    /// Gives `field` to this editor's formula `latex`.
    pub(in crate::editor) fn attach_formula_field(self, field: &MathField, latex: &str) {
        field.attach(self.0, latex);
    }

    /// Takes `field` from this editor, if it has it.
    pub(in crate::editor) fn detach_formula_field(self, field: &MathField) {
        field.detach(self.0);
    }

    /// Takes the formula field from this editor, if one was ever made.
    pub(in crate::editor) fn detach_formula_field_if_started(self) {
        crate::editor::mathfield::detach_if_started(self.0);
    }

    /// The controller as the delegate of the toolbar's share button.
    pub(in crate::editor) fn share_delegate(self) -> &'a ProtocolObject<dyn NSSharingServicePickerToolbarItemDelegate> {
        ProtocolObject::from_ref(self.0)
    }
}

/// What every component may use: the state and the document, which no component owns.
#[derive(Clone, Copy)]
pub(in crate::editor) struct ForPanes<'a>(&'a EditorController);

impl<'a> ForPanes<'a> {
    pub(super) fn of(controller: &'a EditorController) -> Self {
        Self(controller)
    }

    /// Only for the handles of this module, which reach the components through it.
    fn controller(&self) -> &'a EditorController {
        self.0
    }

    /// The controller as the object AppKit knows.
    pub(in crate::editor) fn object(&self) -> Object<'a> {
        Object(self.0)
    }

    pub(in crate::editor) fn model(&self) -> &'a Model {
        self.0.model()
    }

    pub(in crate::editor) fn mtm(&self) -> MainThreadMarker {
        self.0.mtm()
    }

    pub(in crate::editor) fn window(&self) -> Option<Retained<NSWindow>> {
        NSWindowController::window(self.0)
    }

    /// The document as AppKit knows it.
    pub(in crate::editor) fn document(&self) -> Option<Retained<NSDocument>> {
        self.0.document().map(Retained::into_super)
    }

    /// For what holds on to the document longer than this call.
    pub(in crate::editor) fn weak_document(&self) -> Weak<NSDocument> {
        self.document()
            .map(|document| Weak::from(&*document))
            .unwrap_or_default()
    }

    /// Tells the document that its text changed, for its file state and the shadow copy.
    pub(in crate::editor) fn tell_document_text_changed(&self) {
        if let Some(document) = self.0.document() {
            document.text_changed();
        }
    }

    pub(in crate::editor) fn undo_manager(&self) -> Option<Retained<NSUndoManager>> {
        self.0.undo_manager()
    }

    pub(in crate::editor) fn doc_dir(&self) -> Option<PathBuf> {
        self.0.doc_dir()
    }

    pub(in crate::editor) fn is_dark(&self) -> bool {
        self.0.is_dark()
    }

    pub(in crate::editor) fn has_closed(&self) -> bool {
        self.0.has_closed()
    }

    pub(in crate::editor) fn with_text<R>(&self, f: impl for<'b> FnOnce(&'b Snapshot, &'b State) -> R + Send) -> R {
        self.0.with_text(f)
    }

    pub(in crate::editor) fn change_with_text<R>(
        &self,
        f: impl for<'b> FnOnce(&'b Snapshot, &'b mut State) -> R + Send,
    ) -> R {
        self.0.change_with_text(f)
    }

    pub(in crate::editor) fn on_source_text<R>(&self, f: impl for<'b> FnOnce(&'b str) -> R + Send) -> R {
        self.0.on_source_text(f)
    }

    pub(in crate::editor) fn source_text(&self) -> String {
        self.0.source_text()
    }

    /// Marks what is outdated and asks for an update, whoever is the one to notice.
    pub(in crate::editor) fn invalidate(&self, why: Invalidate) {
        self.0.invalidate(why);
    }

    /// What goes on in the views right now, which only the state of the editor above can say about its edits.
    pub(in crate::editor) fn under_way(&self) -> UnderWay {
        UnderWay {
            own_change: self.0.panes().is_silent(),
            preview_edit: self.0.ivars().edits.preview_editing(),
            composition: self.0.panes().is_built() && self.0.panes().preview().has_marked_text(),
        }
    }
}

/// What goes on in the views that an update must not cut into.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(in crate::editor) struct UnderWay {
    /// The controller changes its own views.
    pub(in crate::editor) own_change: bool,
    /// The preview announced an edit, which ends with this turn of the run loop.
    pub(in crate::editor) preview_edit: bool,
    /// The input method holds marked text in the preview, whose offsets a new text would make wrong.
    pub(in crate::editor) composition: bool,
}

impl UnderWay {
    /// Whether a result that can wait has to wait.
    pub(in crate::editor) fn defers_background_results(self) -> bool {
        self.own_change || self.preview_edit
    }

    /// Whether even an update the caller reads the result of has to wait: the end of a composition comes back to it.
    pub(in crate::editor) fn holds_back_an_update(self) -> bool {
        self.own_change || self.composition
    }
}

macro_rules! layer {
    ($(#[$doc:meta])* $name:ident above $below:ident, reaches $method:ident -> $component:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy)]
        pub(in crate::editor) struct $name<'a>($below<'a>);

        impl<'a> $name<'a> {
            pub(super) fn of(controller: &'a EditorController) -> Self {
                Self($below::of(controller))
            }

            pub(in crate::editor) fn $method(&self) -> $component<'a> {
                self.controller().$method()
            }
        }

        impl<'a> Deref for $name<'a> {
            type Target = $below<'a>;

            fn deref(&self) -> &$below<'a> {
                &self.0
            }
        }
    };
}

layer!(ForSelection above ForPanes, reaches panes -> Panes);
layer!(ForPictures above ForSelection, reaches selection -> Selection);
layer!(ForProgressive above ForPictures, reaches pictures -> Pictures);
layer!(ForCounter above ForProgressive, reaches progressive -> Progressive);
layer!(ForChanges above ForCounter, reaches word_counter -> WordCounter);
layer!(ForScroll above ForChanges, reaches change_marks -> ChangeMarks);
layer!(ForUpdater above ForScroll, reaches scroll_sync -> ScrollSync);
layer!(ForSharing above ForUpdater, reaches updater -> Updater);
layer!(ForAtoms above ForSharing, reaches sharing -> Sharing);
layer!(ForEditing above ForAtoms, reaches atom_editor -> AtomEditor);
layer!(ForNotes above ForEditing, reaches editing -> Editing);
layer!(ForLinks above ForNotes, reaches notes -> Notes);

impl ForPictures<'_> {
    /// A picture that arrived reports to whoever waits for it, up to the update that shows it.
    pub(in crate::editor) fn image_arrived(&self, key: String) {
        self.controller().updater().image_arrived(key);
    }
}

impl ForEditing<'_> {
    pub(in crate::editor) fn toggle_preview(&self) {
        self.controller().toggle_preview();
    }

    pub(in crate::editor) fn toggle_files(&self) {
        if self.controller().panes().toggle_tab(SidebarTab::Files) {
            self.controller().notes().rescan();
        }
        self.controller().updater().update_offer();
    }

    /// Shows the sidebar on the tab it showed last, or hides it.
    pub(in crate::editor) fn toggle_sidebar(&self) {
        let panes = self.controller().panes();
        if panes.toggle_tab(panes.sidebar_tab().unwrap_or_default()) {
            self.controller().notes().rescan();
        }
        self.controller().updater().update_offer();
    }

    pub(in crate::editor) fn search_notes(&self) {
        self.controller().notes().show_search();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_composition_holds_back_an_update_and_an_edit_only_what_can_wait() {
        let under_way = |own_change, preview_edit, composition| UnderWay {
            own_change,
            preview_edit,
            composition,
        };
        for (now, can_wait, update) in [
            (under_way(false, false, false), false, false),
            (under_way(true, false, false), true, true),
            (under_way(false, true, false), true, false),
            (under_way(false, false, true), false, true),
        ] {
            assert_eq!(now.defers_background_results(), can_wait, "{now:?}");
            assert_eq!(now.holds_back_an_update(), update, "{now:?}");
        }
    }
}
