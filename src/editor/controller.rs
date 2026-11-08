//! The one object AppKit knows: delegate of both views, target of every command. It forwards to the components, which it
//! alone can reach; everything else asks it by the names below, and a component gets what lies under it through `layers`.

pub(super) mod layers;
pub(super) mod window;

use std::path::{Path, PathBuf};

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSMenu, NSMenuItem, NSMenuItemValidation, NSPasteboard, NSPopoverDelegate, NSPrintInfo, NSPrintOperation,
    NSSharingService, NSSharingServicePicker, NSSharingServicePickerDelegate, NSSharingServicePickerToolbarItem,
    NSSharingServicePickerToolbarItemDelegate, NSTextDelegate, NSTextStorage, NSTextStorageDelegate,
    NSTextStorageEditActions, NSTextView, NSTextViewDelegate, NSToolbar, NSToolbarDelegate, NSToolbarItem,
    NSToolbarItemIdentifier, NSToolbarItemValidation, NSWindow, NSWindowController, NSWindowDelegate,
};
use objc2_foundation::{NSArray, NSCoder, NSNotification, NSObjectProtocol, NSRange, NSRect, NSUndoManager};

use super::actions::Action;
use super::atom::{self, AtomEditor};
use super::changes::{self, ChangeMarks};
use super::counter::{self, WordCounter};
use super::editing::{self, Editing};
use super::export::{self, Sharing};
use super::images::{self, Pictures};
use super::model::Model;
use super::navigation::Links;
use super::notes::{self, Inherited, NoteAnswer, Notes};
use super::panes::{self, Panes, Side};
use super::progressive::Progressive;
use super::scroll::{self, ScrollSync};
use super::selection::{self, Selection};
use super::sidebar::NoteEvent;
use super::state::{Invalidate, State};
use super::sync::Snapshot;
use super::update::{self, Updater};
use super::{js, remote, string_of, style, toolbar};
use crate::document::MdDocument;
use crate::util::guarded;
use layers::{
    ForAtoms, ForChanges, ForCounter, ForEditing, ForLinks, ForNotes, ForPanes, ForPictures, ForProgressive, ForScroll,
    ForSelection, ForSharing, ForUpdater, Message,
};
use splat_editor_state::progress;

pub struct Ivars {
    document: Weak<MdDocument>,
    panes: panes::PaneState,
    model: Model,
    updates: update::UpdateState,
    marks: selection::SelectionMarks,
    atoms: atom::Atoms,
    pictures: images::PictureState,
    scroll: scroll::ScrollState,
    edits: editing::EditState,
    sharing: export::ShareState,
    counter: counter::CounterState,
    changes: changes::ChangeState,
    notes: notes::NotesState,
    /// The window has closed, while what is under way can still hold on to the controller for a moment.
    closed: std::cell::Cell<bool>,
}

/// `define_class!` with `body` and, for every row of `actions::commands!`, the method AppKit finds the command's target by:
/// it asks who responds to the selector of a menu item.
macro_rules! class_with_commands {
    ([$($body:tt)*] $($action:ident = $selector:ident, $applies:ident { $($field:ident: $value:expr),* };)*) => {
        define_class!(
            $($body)*

            impl EditorController {
                $(
                    #[expect(non_snake_case, reason = "named after the selector AppKit sends")]
                    #[unsafe(method($selector:))]
                    fn $selector(&self, s: Option<&AnyObject>) { self.editing().perform(Action::$action, s) }
                )*
            }
        );
    };
}

super::actions::commands!(class_with_commands, [
    #[unsafe(super(NSWindowController))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDEditorController"]
    #[ivars = Ivars]
    pub struct EditorController;

    unsafe impl NSObjectProtocol for EditorController {}

    unsafe impl NSWindowDelegate for EditorController {
        #[unsafe(method_id(windowWillReturnUndoManager:))]
        fn window_undo_manager(&self, _window: &NSWindow) -> Option<Retained<NSUndoManager>> {
            guarded(None, || self.undo_manager())
        }

        #[unsafe(method(windowDidBecomeKey:))]
        fn window_did_become_key(&self, _notification: &NSNotification) {
            guarded((), || {
                self.selection().recolor_marks();
                self.notes().refresh();
                self.change_marks().look();
            });
        }

        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, _notification: &NSNotification) {
            guarded((), || self.selection().recolor_marks());
        }

        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            guarded((), || self.stops_showing());
        }

        #[unsafe(method(windowDidMove:))]
        fn window_did_move(&self, _notification: &NSNotification) {
            guarded((), || self.panes().remember_frame());
        }

        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, _notification: &NSNotification) {
            guarded((), || self.panes().remember_frame());
        }
    }

    unsafe impl NSTextStorageDelegate for EditorController {
        #[unsafe(method(textStorage:didProcessEditing:range:changeInLength:))]
        fn storage_edited(&self, storage: &NSTextStorage, mask: NSTextStorageEditActions, range: NSRange, delta: isize) {
            if mask.contains(NSTextStorageEditActions::EditedCharacters) {
                guarded((), || {
                    // Not the editor's own mirror of its text, so the offsets of an open editor mean nothing anymore.
                    if !self.panes().is_silent() {
                        self.atom_editor().abandon();
                    }
                    self.updater().source_edited(storage, range, delta);
                });
            }
        }
    }

    unsafe impl NSTextDelegate for EditorController {
        #[unsafe(method(textDidChange:))]
        fn text_did_change(&self, notification: &NSNotification) {
            guarded((), || self.editing().text_did_change(notification.object().as_deref()));
        }
    }

    unsafe impl NSTextViewDelegate for EditorController {
        #[unsafe(method_id(undoManagerForTextView:))]
        fn text_view_undo_manager(&self, view: &NSTextView) -> Option<Retained<NSUndoManager>> {
            guarded(None, || self.atom_editor().undo_manager_of(view).or_else(|| self.undo_manager()))
        }

        #[unsafe(method(textView:shouldChangeTextInRanges:replacementStrings:))]
        fn should_change(&self, view: &NSTextView, ranges: &AnyObject, strings: Option<&AnyObject>) -> bool {
            guarded((), || self.editing().text_will_change(view, ranges, strings));
            true
        }

        #[unsafe(method(textViewDidChangeSelection:))]
        fn selection_changed(&self, notification: &NSNotification) {
            guarded((), || self.editing().selection_did_change(notification.object().as_deref()));
        }

        #[unsafe(method(textView:doCommandBySelector:))]
        fn do_command(&self, view: &NSTextView, command: Sel) -> bool {
            guarded(false, || self.editing().do_command(view, command))
        }

        #[unsafe(method(textView:doubleClickedOnCell:inRect:atIndex:))]
        fn double_clicked_cell(&self, view: &NSTextView, _cell: &AnyObject, frame: NSRect, index: usize) {
            guarded((), || self.atom_editor().cell_clicked(view, frame, index));
        }

        #[unsafe(method(textView:clickedOnCell:inRect:atIndex:))]
        fn clicked_cell(&self, view: &NSTextView, _cell: &AnyObject, frame: NSRect, index: usize) {
            guarded((), || self.atom_editor().cell_clicked(view, frame, index));
        }

        #[unsafe(method(textView:clickedOnLink:atIndex:))]
        fn clicked_link(&self, view: &NSTextView, link: &AnyObject, index: usize) -> bool {
            guarded(false, || self.links().link_clicked(view, link, index))
        }

        #[unsafe(method_id(textView:menu:forEvent:atIndex:))]
        fn context_menu(&self, view: &NSTextView, menu: &NSMenu, _event: &AnyObject, index: usize) -> Option<Retained<NSMenu>> {
            guarded(None, || Some(self.editing().adjust_context_menu(menu, view, index)))
        }
    }

    unsafe impl NSPopoverDelegate for EditorController {
        #[unsafe(method(popoverDidClose:))]
        fn popover_did_close(&self, notification: &NSNotification) {
            guarded((), || self.atom_editor().popover_closed(notification.object().as_deref()));
        }
    }

    unsafe impl NSToolbarDelegate for EditorController {
        #[unsafe(method_id(toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:))]
        fn toolbar_item(&self, _toolbar: &NSToolbar, id: &NSToolbarItemIdentifier, _insert: bool) -> Option<Retained<NSToolbarItem>> {
            guarded(None, || toolbar::make(self.mtm(), &id.to_string(), ProtocolObject::from_ref(self), self))
        }

        #[unsafe(method_id(toolbarDefaultItemIdentifiers:))]
        fn toolbar_default(&self, _toolbar: &NSToolbar) -> Retained<NSArray<NSToolbarItemIdentifier>> {
            guarded(NSArray::new(), toolbar::default_identifiers)
        }

        #[unsafe(method_id(toolbarAllowedItemIdentifiers:))]
        fn toolbar_allowed(&self, _toolbar: &NSToolbar) -> Retained<NSArray<NSToolbarItemIdentifier>> {
            guarded(NSArray::new(), toolbar::allowed_identifiers)
        }
    }

    unsafe impl NSSharingServicePickerDelegate for EditorController {
        #[unsafe(method_id(sharingServicePicker:sharingServicesForItems:proposedSharingServices:))]
        fn share_services(
            &self,
            _picker: &NSSharingServicePicker,
            _items: &NSArray,
            proposed: &NSArray<NSSharingService>,
        ) -> Retained<NSArray<NSSharingService>> {
            guarded(objc2::Message::retain(proposed), || self.sharing().services_with(proposed))
        }

        #[unsafe(method(sharingServicePicker:didChooseSharingService:))]
        fn share_chosen(&self, _picker: &NSSharingServicePicker, service: Option<&NSSharingService>) {
            guarded((), || self.sharing().service_chosen(service));
        }
    }

    unsafe impl NSSharingServicePickerToolbarItemDelegate for EditorController {
        // AppKit asks for the items on every toolbar validation, so the file gets the text only when the menu opens.
        #[unsafe(method_id(itemsForSharingServicePickerToolbarItem:))]
        fn share_items(&self, _item: &NSSharingServicePickerToolbarItem) -> Retained<NSArray> {
            guarded(NSArray::new(), || self.sharing().items())
        }
    }

    unsafe impl NSMenuItemValidation for EditorController {
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            guarded(true, || self.editing().validate(item.action(), item.tag(), Some(item)))
        }
    }

    unsafe impl NSToolbarItemValidation for EditorController {
        #[unsafe(method(validateToolbarItem:))]
        fn validate_toolbar_item(&self, item: &NSToolbarItem) -> bool {
            guarded(true, || self.editing().validate(item.action(), item.tag(), None))
        }
    }

    impl EditorController {
        #[unsafe(method(scheduledUpdateDue))]
        fn scheduled_update_due(&self) {
            guarded((), || self.updater().scheduled_update_due());
        }

        #[unsafe(method(renderPendingBlocks))]
        fn render_pending_blocks_now(&self) {
            guarded((), || self.progressive().render_pending_blocks());
        }

        #[unsafe(method(colorPendingSource))]
        fn color_pending_source(&self) {
            guarded((), || self.updater().color_pending_source());
        }

        #[unsafe(method(requestRenders))]
        fn request_renders(&self) {
            guarded((), || self.updater().request_renders());
        }

        #[unsafe(method(applyArrivedRenders))]
        fn apply_arrived_renders(&self) {
            guarded((), || {
                self.updater().apply_arrived_renders();
                self.atom_editor().show_status();
            });
        }

        #[unsafe(method(showArrivedImages))]
        fn show_arrived_images_now(&self) {
            guarded((), || self.updater().show_arrived_images());
        }

        #[unsafe(method(syncScroll))]
        fn sync_scroll_now(&self) {
            guarded((), || self.scroll_sync().sync_scroll());
        }

        #[unsafe(method(settlePanes))]
        fn settle_panes_now(&self) {
            guarded((), || {
                if self.panes().settle() {
                    self.updater().update_now();
                }
            });
        }

        #[unsafe(method(previewEditSettled))]
        fn preview_edit_settled_now(&self) {
            guarded((), || self.editing().preview_edit_settled());
        }

        #[unsafe(method(openAtomUnderCaret))]
        fn open_atom_under_caret(&self) {
            guarded((), || self.atom_editor().open_under_caret());
        }

        #[unsafe(method(panesResized:))]
        fn panes_resized(&self, _notification: &NSNotification) {
            guarded((), || {
                self.updater().panes_resized();
                self.panes().remember_sidebar();
                self.word_counter().place();
                self.change_marks().redraw();
            });
        }

        #[unsafe(method(clipBoundsChanged:))]
        fn clip_bounds_changed(&self, notification: &NSNotification) {
            guarded((), || {
                self.updater().color_visible_source();
                self.scroll_sync().view_scrolled(notification);
                self.change_marks().redraw();
            });
        }

        #[unsafe(method(countWords))]
        fn count_words(&self) {
            guarded((), || self.word_counter().count_now());
        }

        #[unsafe(method(compareWithCommit))]
        fn compare_with_commit(&self) {
            guarded((), || self.change_marks().compare_now());
        }

        #[unsafe(method(undoDidChange:))]
        fn undo_did_change(&self, _notification: &NSNotification) {
            guarded((), || self.updater().undo_did_change());
        }

        #[unsafe(method(encodeRestorableStateWithCoder:))]
        fn encode_state(&self, coder: &NSCoder) {
            // SAFETY: The superclass's implementation, with this method's signature.
            let _: () = unsafe { msg_send![super(self), encodeRestorableStateWithCoder: coder] };
            guarded((), || self.panes().encode_state(coder));
        }

        #[unsafe(method(restoreStateWithCoder:))]
        fn restore_state(&self, coder: &NSCoder) {
            // SAFETY: The superclass's implementation, with this method's signature.
            let _: () = unsafe { msg_send![super(self), restoreStateWithCoder: coder] };
            guarded((), || {
                if let Some(zoom) = self.panes().restore_state(coder) {
                    self.updater().set_zoom(zoom);
                }
            });
        }
    }
]);

/// What only this module and `layers` may call in the program itself, as a component gets no further than `layers` lets it.
/// The window self-tests act from outside on everything.
macro_rules! within_the_controller {
    ($(fn $name:ident($this:ident) $(-> $kind:ty)? $body:block)*) => {
        impl EditorController {
            $(
                #[cfg(not(feature = "selftest"))]
                fn $name(&$this) $(-> $kind)? $body

                #[cfg(feature = "selftest")]
                pub(in crate::editor) fn $name(&$this) $(-> $kind)? $body
            )*
        }
    };
}

within_the_controller! {
    fn model(self) -> &Model { &self.ivars().model }
    fn panes(self) -> Panes<'_> { Panes::new(ForPanes::of(self), &self.ivars().panes) }
    fn selection(self) -> Selection<'_> { Selection::new(ForSelection::of(self), &self.ivars().marks) }
    fn pictures(self) -> Pictures<'_> { Pictures::new(ForPictures::of(self), &self.ivars().pictures) }
    fn progressive(self) -> Progressive<'_> { Progressive::new(ForProgressive::of(self)) }
    fn scroll_sync(self) -> ScrollSync<'_> { ScrollSync::new(ForScroll::of(self), &self.ivars().scroll) }
    fn updater(self) -> Updater<'_> { Updater::new(ForUpdater::of(self), &self.ivars().updates) }
    fn sharing(self) -> Sharing<'_> { Sharing::new(ForSharing::of(self), &self.ivars().sharing) }
    fn atom_editor(self) -> AtomEditor<'_> { AtomEditor::new(ForAtoms::of(self), &self.ivars().atoms) }
    fn editing(self) -> Editing<'_> { Editing::new(ForEditing::of(self), &self.ivars().edits) }
    fn links(self) -> Links<'_> { Links::new(ForLinks::of(self)) }
    fn word_counter(self) -> WordCounter<'_> { WordCounter::new(ForCounter::of(self), &self.ivars().counter) }
    fn change_marks(self) -> ChangeMarks<'_> { ChangeMarks::new(ForChanges::of(self), &self.ivars().changes) }
    fn notes(self) -> Notes<'_> { Notes::new(ForNotes::of(self), &self.ivars().notes) }

    fn toggle_preview(self) {
        if self.panes().toggle_preview() {
            self.updater().update_now();
        }
    }
}

/// What the rest of the program tells or asks an editor; nothing of it reaches a component in any other way.
impl EditorController {
    #[cfg(feature = "selftest")]
    pub(in crate::editor) fn command_applies(&self, action: Sel) -> bool {
        self.editing().validate(Some(action), 0, None)
    }

    /// What arrived for a component, later or from one below it; see `layers::Message`.
    pub(in crate::editor) fn handle(&self, message: Message) {
        match message {
            Message::LinkChosen { url, label } => self.editing().link_chosen(&url, &label),
            Message::TableChosen { rows, columns } => self.editing().table_chosen(rows, columns),
            Message::ImageChosen(path) => self.editing().image_chosen(&path),
            Message::PicturePasted { written, folder } => self.editing().picture_pasted(written, &folder),
            Message::MetadataChosen(wanted) => self.metadata_chosen(wanted),
            Message::Perform(action) => self.perform(action),
            Message::Notes(answer) => self.note_answer(answer),
            Message::NotesFolder(folder) => self.use_notes_folder(folder),
            Message::FolderGranted => {
                self.updater().folder_granted();
                self.editing().folder_granted();
                self.notes().rescan();
                self.change_marks().look();
            }
            Message::RenameChosen { from, to } => self.note_rename_chosen(from, to),
            Message::RenameConfirmed(request, rewrites) => self.notes().rename(request, rewrites),
            Message::RevealPlace { line, column } => self.links().reveal_place(line, column),
            Message::Changes(answer) => self.change_marks().answer(answer),
            Message::ImageLoaded(path, file) => self.pictures().image_loaded(path, file),
            Message::ImagesChanged(changed) => self.pictures().read_images_again(changed),
            Message::Parsed {
                generation,
                text,
                background,
            } => self.updater().parse_ended(generation, &text, background.map(|b| *b)),
            Message::ExportTo { format, path, title } => {
                self.sharing().export_to(self.export_source(), format, path, title);
            }
            Message::ExportFailed(failure) => self.report_export_failure(&failure),
        }
    }

    pub(in crate::editor) fn reveal_source(&self, start: crate::markdown::text::Byte) {
        self.links().reveal_source(start);
    }

    /// What the images of a preview that shows hold, for a look at their files.
    pub(in crate::editor) fn images_shown(&self) -> Option<&std::cell::RefCell<images::ImageStore>> {
        self.panes().preview_visible().then(|| self.pictures().store())
    }

    /// Whether the document has views to hold a text yet, and the version of that text.
    pub(in crate::editor) fn version_of_shown_text(&self) -> Option<u64> {
        self.panes().is_built().then(|| self.model().text_version())
    }

    /// The text as the view holds it, for what may run at any moment, like the save after a panic.
    pub(in crate::editor) fn text_of_built_view(&self) -> Option<String> {
        Some(string_of(&self.panes().source_if_built()?.string()))
    }

    pub(in crate::editor) fn renders_arrived(&self) {
        self.updater().renders_arrived();
    }

    pub(in crate::editor) fn web_images_shown(&self) -> Vec<String> {
        self.updater().web_images_shown()
    }

    pub(in crate::editor) fn image_arrived(&self, key: String) {
        self.updater().image_arrived(key);
    }

    pub(in crate::editor) fn source_became_focused(&self) {
        self.selection().source_became_focused();
    }

    pub(in crate::editor) fn preview_became_focused(&self) {
        self.selection().preview_became_focused();
    }

    pub(in crate::editor) fn handle_source_drop(&self, pasteboard: &NSPasteboard, at: usize) -> bool {
        self.editing().handle_source_drop(pasteboard, at)
    }

    pub(in crate::editor) fn paste_image(&self, pasteboard: &NSPasteboard) -> bool {
        self.editing().paste_image(pasteboard)
    }

    pub(in crate::editor) fn preview_takes_input(&self) -> bool {
        self.updater().preview_takes_input()
    }

    pub(in crate::editor) fn appearance_changed(&self) {
        self.updater().appearance_changed();
    }

    pub(in crate::editor) fn visual_formula_changed(&self, latex: &str) {
        self.atom_editor().visual_formula_changed(latex);
    }

    pub(in crate::editor) fn cancel_formula(&self) {
        self.atom_editor().cancel();
    }

    pub(in crate::editor) fn export_source(&self) -> export::ExportSource {
        self.sharing().export_source()
    }

    pub(in crate::editor) fn report_export_failure(&self, failure: &export::ExportFailure) {
        self.sharing().report_export_failure(failure);
    }

    pub(in crate::editor) fn perform(&self, action: Action) {
        self.editing().perform(action, None);
    }

    pub(in crate::editor) fn setting_changed(&self) {
        self.word_counter().setting_changed();
        self.change_marks().look();
    }

    pub(in crate::editor) fn look_at_repository(&self) {
        self.change_marks().look();
    }

    pub(in crate::editor) fn draw_change_bars(&self, side: Side, bars: &objc2_app_kit::NSView) {
        self.change_marks().draw(side, bars);
    }

    pub(in crate::editor) fn metadata_chosen(&self, wanted: Vec<(String, crate::markdown::front_matter::Value)>) {
        self.editing().metadata_chosen(wanted);
    }

    pub(in crate::editor) fn note_event(&self, event: NoteEvent) {
        self.notes().event(event);
    }

    pub(in crate::editor) fn note_answer(&self, answer: NoteAnswer) {
        self.notes().answer(answer);
    }

    pub(in crate::editor) fn use_notes_folder(&self, folder: PathBuf) {
        self.notes().use_folder(folder);
    }

    pub(in crate::editor) fn inherit_notes(&self, inherited: Inherited) {
        self.notes().inherit(inherited);
    }

    pub(in crate::editor) fn notes_may_have_changed(&self) {
        self.notes().refresh();
    }

    pub(in crate::editor) fn note_rename_chosen(&self, from: PathBuf, to: PathBuf) {
        self.notes().rename_chosen(from, to);
    }

    /// Changes this document's text by `replacements`, if it still is `before`.
    pub(in crate::editor) fn replace_in_text(&self, before: &str, replacements: &[splat_notes::Replacement]) -> bool {
        self.editing().replace_in_text(before, replacements)
    }

    pub(in crate::editor) fn document(&self) -> Option<Retained<MdDocument>> {
        self.ivars().document.load()
    }

    pub(in crate::editor) fn undo_manager(&self) -> Option<Retained<NSUndoManager>> {
        self.document().and_then(|d| d.undoManager())
    }

    pub(in crate::editor) fn doc_dir(&self) -> Option<PathBuf> {
        let url = self.document()?.fileURL()?;
        let path = url.path()?.to_string();
        Path::new(&path).parent().map(Path::to_path_buf)
    }

    pub(in crate::editor) fn is_dark(&self) -> bool {
        let appearance = self.panes().preview().effective_appearance();
        matches!(
            style::appearance_of(&appearance),
            crate::markdown::palette::Appearance::Dark | crate::markdown::palette::Appearance::DarkHighContrast
        )
    }

    /// The one way to make the editor redraw what its text alone would not change, like the zoom or a formula that
    /// arrived: marks what is outdated, then updates.
    pub(in crate::editor) fn invalidate(&self, why: Invalidate) {
        self.model().change(move |st| st.invalidate(why));
        // The renderer may have dropped pictures for memory, which only a request brings back.
        if matches!(why, Invalidate::Zoom | Invalidate::Appearance | Invalidate::Layout) {
            self.updater().schedule_render_requests();
        }
        if progress::updates_at_once(why, self.panes().source().length()) {
            self.updater().update_now();
        } else {
            self.updater().catch_up();
        }
    }

    /// Lends the current source text, which edits patch in place, and the state.
    pub(in crate::editor) fn with_text<R>(&self, f: impl for<'a> FnOnce(&'a Snapshot, &'a State) -> R + Send) -> R {
        self.model().with_text(|| self.panes().source().string_and_length(), f)
    }

    /// Like `with_text`, for a change of the state that depends on the text.
    pub(in crate::editor) fn change_with_text<R>(
        &self,
        f: impl for<'a> FnOnce(&'a Snapshot, &'a mut State) -> R + Send,
    ) -> R {
        self.model()
            .change_with_text(|| self.panes().source().string_and_length(), f)
    }

    /// What `f` makes of the current source text, for those outside the editor who need no copy of it.
    pub fn on_source_text<R>(&self, f: impl for<'a> FnOnce(&'a str) -> R + Send) -> R {
        self.with_text(|snap, _| f(&snap.text))
    }

    pub fn source_text(&self) -> String {
        self.on_source_text(str::to_string)
    }

    /// The text exactly as the source view holds it, for saving.
    pub fn exact_text(&self) -> String {
        string_of(&self.panes().source().string())
    }

    /// Replaces the whole source, e.g. after loading or reverting, without an undo step. An open editor's offsets mean
    /// nothing then.
    pub fn replace_all_text(&self, text: &str) {
        self.atom_editor().abandon();
        self.updater().replace_all_text(text);
    }

    pub fn print_operation(&self, info: &NSPrintInfo) -> Retained<NSPrintOperation> {
        self.sharing().print_operation(info)
    }
}

impl EditorController {
    /// The window closes: formulas and images only this document waited for are of no use to anybody.
    fn stops_showing(&self) {
        self.ivars().closed.set(true);
        js::forget(self);
        remote::stops_showing(self);
    }

    /// Nobody will see what this document asks for from now on.
    pub(in crate::editor) fn has_closed(&self) -> bool {
        self.ivars().closed.get()
    }

    #[cfg(feature = "selftest")]
    pub(in crate::editor) fn stop_showing_for_a_check(&self) {
        self.stops_showing();
    }
}
