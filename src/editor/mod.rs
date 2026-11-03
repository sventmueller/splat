//! The editor window: Markdown source and preview side by side. One controller object serves AppKit as delegate of
//! both views and forwards to the components below, each with the state it owns; README.md describes the rules that keep
//! the delegate callbacks from stepping on each other.

pub mod actions;
mod atom;
mod attributes;
pub mod decoder;
mod dialogs;
mod editing;
mod env;
mod export;
mod images;
mod js;
mod links;
mod mathfield;
mod model;
mod navigation;
mod outline;
mod panes;
mod progressive;
mod remote;
mod render;
mod render_queue;
mod safe_image;
mod scroll;
mod selection;
#[cfg(feature = "selftest")]
pub mod selftest;
mod shadow;
mod state;
mod style;
mod sync;
mod toolbar;
mod update;
mod views;
mod webkit;
mod window;

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSMenu, NSMenuItem, NSMenuItemValidation, NSPopoverDelegate, NSPrintInfo, NSPrintOperation, NSSharingService,
    NSSharingServicePicker, NSSharingServicePickerDelegate, NSSharingServicePickerToolbarItem,
    NSSharingServicePickerToolbarItemDelegate, NSTextDelegate, NSTextStorage, NSTextStorageDelegate,
    NSTextStorageEditActions, NSTextView, NSTextViewDelegate, NSToolbar, NSToolbarDelegate, NSToolbarItem,
    NSToolbarItemIdentifier, NSToolbarItemValidation, NSWindow, NSWindowController, NSWindowDelegate,
};
use objc2_foundation::{NSArray, NSCoder, NSNotification, NSObjectProtocol, NSRange, NSRect, NSUndoManager};

use crate::document::MdDocument;
use crate::util::ns;
use actions::Action;

pub use crate::util::panic_is_contained;
pub use images::{look_at_images, watch_memory};
pub use shadow::{discard_copies, discard_share_files, keep_copies};

use model::Model;
use state::{Invalidate, State};
use sync::Snapshot;

pub struct Ivars {
    document: Weak<MdDocument>,
    panes: panes::PaneState,
    model: Model,
    updates: update::UpdateState,
    marks: selection::Marks,
    atoms: atom::Atoms,
    pictures: images::PictureState,
    scroll: scroll::ScrollState,
    edits: editing::EditState,
    sharing: export::ShareState,
}

define_class!(
    #[unsafe(super(NSWindowController))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDEditorController"]
    #[ivars = Ivars]
    pub struct EditorController;

    unsafe impl NSObjectProtocol for EditorController {}

    unsafe impl NSWindowDelegate for EditorController {
        #[unsafe(method_id(windowWillReturnUndoManager:))]
        fn window_undo_manager(&self, _window: &NSWindow) -> Option<Retained<NSUndoManager>> {
            self.undo_manager()
        }

        #[unsafe(method(windowDidBecomeKey:))]
        fn window_did_become_key(&self, _notification: &NSNotification) {
            self.selection().recolor_marks();
        }

        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, _notification: &NSNotification) {
            self.selection().recolor_marks();
        }

        #[unsafe(method(windowDidMove:))]
        fn window_did_move(&self, _notification: &NSNotification) {
            self.panes().remember_frame();
        }

        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, _notification: &NSNotification) {
            self.panes().remember_frame();
        }
    }

    unsafe impl NSTextStorageDelegate for EditorController {
        #[unsafe(method(textStorage:didProcessEditing:range:changeInLength:))]
        fn storage_edited(&self, storage: &NSTextStorage, mask: NSTextStorageEditActions, range: NSRange, delta: isize) {
            if mask.contains(NSTextStorageEditActions::EditedCharacters) {
                self.updater().source_edited(storage, range, delta);
            }
        }
    }

    unsafe impl NSTextDelegate for EditorController {
        #[unsafe(method(textDidChange:))]
        fn text_did_change(&self, notification: &NSNotification) {
            self.editing().text_did_change(notification.object().as_deref());
        }
    }

    unsafe impl NSTextViewDelegate for EditorController {
        #[unsafe(method_id(undoManagerForTextView:))]
        fn text_view_undo_manager(&self, view: &NSTextView) -> Option<Retained<NSUndoManager>> {
            self.atom_editor().undo_manager_of(view).or_else(|| self.undo_manager())
        }

        #[unsafe(method(textView:shouldChangeTextInRanges:replacementStrings:))]
        fn should_change(&self, view: &NSTextView, ranges: &AnyObject, strings: Option<&AnyObject>) -> bool {
            self.editing().text_will_change(view, ranges, strings);
            true
        }

        #[unsafe(method(textViewDidChangeSelection:))]
        fn selection_changed(&self, notification: &NSNotification) {
            self.editing().selection_did_change(notification.object().as_deref());
        }

        #[unsafe(method(textView:doCommandBySelector:))]
        fn do_command(&self, view: &NSTextView, command: Sel) -> bool {
            self.editing().do_command(view, command)
        }

        #[unsafe(method(textView:doubleClickedOnCell:inRect:atIndex:))]
        fn double_clicked_cell(&self, view: &NSTextView, _cell: &AnyObject, frame: NSRect, index: usize) {
            self.atom_editor().cell_clicked(view, frame, index);
        }

        #[unsafe(method(textView:clickedOnCell:inRect:atIndex:))]
        fn clicked_cell(&self, view: &NSTextView, _cell: &AnyObject, frame: NSRect, index: usize) {
            self.atom_editor().cell_clicked(view, frame, index);
        }

        #[unsafe(method(textView:clickedOnLink:atIndex:))]
        fn clicked_link(&self, view: &NSTextView, link: &AnyObject, index: usize) -> bool {
            self.links().link_clicked(view, link, index)
        }

        #[unsafe(method_id(textView:menu:forEvent:atIndex:))]
        fn context_menu(&self, view: &NSTextView, menu: &NSMenu, _event: &AnyObject, index: usize) -> Option<Retained<NSMenu>> {
            Some(self.editing().adjust_context_menu(menu, view, index))
        }
    }

    unsafe impl NSPopoverDelegate for EditorController {
        #[unsafe(method(popoverDidClose:))]
        fn popover_did_close(&self, notification: &NSNotification) {
            self.atom_editor().popover_closed(notification.object().as_deref());
        }
    }

    unsafe impl NSToolbarDelegate for EditorController {
        #[unsafe(method_id(toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:))]
        fn toolbar_item(&self, _toolbar: &NSToolbar, id: &NSToolbarItemIdentifier, _insert: bool) -> Option<Retained<NSToolbarItem>> {
            toolbar::make(self.mtm(), &id.to_string(), ProtocolObject::from_ref(self), self)
        }

        #[unsafe(method_id(toolbarDefaultItemIdentifiers:))]
        fn toolbar_default(&self, _toolbar: &NSToolbar) -> Retained<NSArray<NSToolbarItemIdentifier>> {
            toolbar::default_identifiers()
        }

        #[unsafe(method_id(toolbarAllowedItemIdentifiers:))]
        fn toolbar_allowed(&self, _toolbar: &NSToolbar) -> Retained<NSArray<NSToolbarItemIdentifier>> {
            toolbar::allowed_identifiers()
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
            self.sharing().services_with(proposed)
        }

        #[unsafe(method(sharingServicePicker:didChooseSharingService:))]
        fn share_chosen(&self, _picker: &NSSharingServicePicker, service: Option<&NSSharingService>) {
            self.sharing().service_chosen(service);
        }
    }

    unsafe impl NSSharingServicePickerToolbarItemDelegate for EditorController {
        // AppKit asks for the items on every toolbar validation, so the file gets the text only when the menu opens.
        #[unsafe(method_id(itemsForSharingServicePickerToolbarItem:))]
        fn share_items(&self, _item: &NSSharingServicePickerToolbarItem) -> Retained<NSArray> {
            self.sharing().items()
        }
    }

    unsafe impl NSMenuItemValidation for EditorController {
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            self.editing().validate(item.action(), item.tag(), Some(item))
        }
    }

    unsafe impl NSToolbarItemValidation for EditorController {
        #[unsafe(method(validateToolbarItem:))]
        fn validate_toolbar_item(&self, item: &NSToolbarItem) -> bool {
            self.editing().validate(item.action(), item.tag(), None)
        }
    }

    impl EditorController {
        #[unsafe(method(scheduledUpdateDue))]
        fn scheduled_update_due(&self) {
            self.updater().scheduled_update_due();
        }

        #[unsafe(method(renderPendingBlocks))]
        fn render_pending_blocks_now(&self) {
            self.progressive().render_pending_blocks();
        }

        #[unsafe(method(requestRenders))]
        fn request_renders(&self) {
            self.updater().request_renders();
        }

        #[unsafe(method(applyArrivedRenders))]
        fn apply_arrived_renders(&self) {
            self.updater().apply_arrived_renders();
        }

        #[unsafe(method(showArrivedImages))]
        fn show_arrived_images_now(&self) {
            self.updater().show_arrived_images();
        }

        #[unsafe(method(syncScroll))]
        fn sync_scroll_now(&self) {
            self.scroll_sync().sync_scroll();
        }

        #[unsafe(method(settlePanes))]
        fn settle_panes_now(&self) {
            self.panes().settle();
        }

        #[unsafe(method(previewEditSettled))]
        fn preview_edit_settled_now(&self) {
            self.editing().preview_edit_settled();
        }

        #[unsafe(method(openAtomUnderCaret))]
        fn open_atom_under_caret(&self) {
            self.atom_editor().open_under_caret();
        }

        #[unsafe(method(panesResized:))]
        fn panes_resized(&self, _notification: &NSNotification) {
            self.updater().panes_resized();
        }

        #[unsafe(method(clipBoundsChanged:))]
        fn clip_bounds_changed(&self, notification: &NSNotification) {
            self.scroll_sync().view_scrolled(notification);
        }

        #[unsafe(method(undoDidChange:))]
        fn undo_did_change(&self, _notification: &NSNotification) {
            self.updater().undo_did_change();
        }

        #[unsafe(method(encodeRestorableStateWithCoder:))]
        fn encode_state(&self, coder: &NSCoder) {
            // SAFETY: The superclass's implementation, with this method's signature.
            let _: () = unsafe { msg_send![super(self), encodeRestorableStateWithCoder: coder] };
            self.panes().encode_state(coder);
        }

        #[unsafe(method(restoreStateWithCoder:))]
        fn restore_state(&self, coder: &NSCoder) {
            // SAFETY: The superclass's implementation, with this method's signature.
            let _: () = unsafe { msg_send![super(self), restoreStateWithCoder: coder] };
            self.panes().restore_state(coder);
        }

        // One method per command of `actions`: AppKit finds a menu item's target by asking who responds to its
        // selector.
        #[unsafe(method(mdBold:))]
        fn md_bold(&self, s: Option<&AnyObject>) { self.editing().perform(Action::Bold, s) }
        #[unsafe(method(mdItalic:))]
        fn md_italic(&self, s: Option<&AnyObject>) { self.editing().perform(Action::Italic, s) }
        #[unsafe(method(mdUnderline:))]
        fn md_underline(&self, s: Option<&AnyObject>) { self.editing().perform(Action::Underline, s) }
        #[unsafe(method(mdStrikethrough:))]
        fn md_strike(&self, s: Option<&AnyObject>) { self.editing().perform(Action::Strikethrough, s) }
        #[unsafe(method(mdHighlight:))]
        fn md_highlight(&self, s: Option<&AnyObject>) { self.editing().perform(Action::Highlight, s) }
        #[unsafe(method(mdInlineCode:))]
        fn md_code(&self, s: Option<&AnyObject>) { self.editing().perform(Action::InlineCode, s) }
        #[unsafe(method(mdSuperscript:))]
        fn md_sup(&self, s: Option<&AnyObject>) { self.editing().perform(Action::Superscript, s) }
        #[unsafe(method(mdSubscript:))]
        fn md_sub(&self, s: Option<&AnyObject>) { self.editing().perform(Action::Subscript, s) }
        #[unsafe(method(mdHeading:))]
        fn md_heading(&self, s: Option<&AnyObject>) { self.editing().perform(Action::Heading, s) }
        #[unsafe(method(mdBulletList:))]
        fn md_bullets(&self, s: Option<&AnyObject>) { self.editing().perform(Action::BulletList, s) }
        #[unsafe(method(mdNumberedList:))]
        fn md_numbers(&self, s: Option<&AnyObject>) { self.editing().perform(Action::NumberedList, s) }
        #[unsafe(method(mdTaskList:))]
        fn md_tasks(&self, s: Option<&AnyObject>) { self.editing().perform(Action::TaskList, s) }
        #[unsafe(method(mdQuote:))]
        fn md_quote(&self, s: Option<&AnyObject>) { self.editing().perform(Action::Quote, s) }
        #[unsafe(method(mdCodeBlock:))]
        fn md_code_block(&self, s: Option<&AnyObject>) { self.editing().perform(Action::CodeBlock, s) }
        #[unsafe(method(mdInsertAlert:))]
        fn md_alert(&self, s: Option<&AnyObject>) { self.editing().perform(Action::Alert, s) }
        #[unsafe(method(mdRemoveFormatting:))]
        fn md_clear(&self, s: Option<&AnyObject>) { self.editing().perform(Action::RemoveFormatting, s) }
        #[unsafe(method(mdEditAtom:))]
        fn md_edit_atom(&self, s: Option<&AnyObject>) { self.editing().perform(Action::EditAtom, s) }
        #[unsafe(method(mdInsertLink:))]
        fn md_link(&self, s: Option<&AnyObject>) { self.editing().perform(Action::Link, s) }
        #[unsafe(method(mdInsertImage:))]
        fn md_image(&self, s: Option<&AnyObject>) { self.editing().perform(Action::Image, s) }
        #[unsafe(method(mdInsertTable:))]
        fn md_table(&self, s: Option<&AnyObject>) { self.editing().perform(Action::Table, s) }
        #[unsafe(method(mdInsertFormula:))]
        fn md_formula(&self, s: Option<&AnyObject>) { self.editing().perform(Action::Formula, s) }
        #[unsafe(method(mdInsertDiagram:))]
        fn md_diagram(&self, s: Option<&AnyObject>) { self.editing().perform(Action::Diagram, s) }
        #[unsafe(method(mdInsertToc:))]
        fn md_toc(&self, s: Option<&AnyObject>) { self.editing().perform(Action::Toc, s) }
        #[unsafe(method(mdInsertFootnote:))]
        fn md_footnote(&self, s: Option<&AnyObject>) { self.editing().perform(Action::Footnote, s) }
        #[unsafe(method(mdInsertRule:))]
        fn md_rule(&self, s: Option<&AnyObject>) { self.editing().perform(Action::Rule, s) }
        #[unsafe(method(mdTableRowAbove:))]
        fn md_row_above(&self, s: Option<&AnyObject>) { self.editing().perform(Action::RowAbove, s) }
        #[unsafe(method(mdTableRowBelow:))]
        fn md_row_below(&self, s: Option<&AnyObject>) { self.editing().perform(Action::RowBelow, s) }
        #[unsafe(method(mdTableColumnLeft:))]
        fn md_col_left(&self, s: Option<&AnyObject>) { self.editing().perform(Action::ColumnLeft, s) }
        #[unsafe(method(mdTableColumnRight:))]
        fn md_col_right(&self, s: Option<&AnyObject>) { self.editing().perform(Action::ColumnRight, s) }
        #[unsafe(method(mdTableDeleteRow:))]
        fn md_del_row(&self, s: Option<&AnyObject>) { self.editing().perform(Action::DeleteRow, s) }
        #[unsafe(method(mdTableDeleteColumn:))]
        fn md_del_col(&self, s: Option<&AnyObject>) { self.editing().perform(Action::DeleteColumn, s) }
        #[unsafe(method(mdTableAlign:))]
        fn md_align(&self, s: Option<&AnyObject>) { self.editing().perform(Action::TableAlign, s) }
        #[unsafe(method(mdTableFormat:))]
        fn md_table_format(&self, s: Option<&AnyObject>) { self.editing().perform(Action::TableFormat, s) }
        #[unsafe(method(mdTogglePreview:))]
        fn md_toggle_preview(&self, s: Option<&AnyObject>) { self.editing().perform(Action::TogglePreview, s) }
        #[unsafe(method(mdToggleOutline:))]
        fn md_toggle_outline(&self, s: Option<&AnyObject>) { self.editing().perform(Action::ToggleOutline, s) }
        #[unsafe(method(mdTypography:))]
        fn md_typography(&self, s: Option<&AnyObject>) { self.editing().perform(Action::Typography, s) }
        #[unsafe(method(mdZoomIn:))]
        fn md_zoom_in(&self, s: Option<&AnyObject>) { self.editing().perform(Action::ZoomIn, s) }
        #[unsafe(method(mdZoomOut:))]
        fn md_zoom_out(&self, s: Option<&AnyObject>) { self.editing().perform(Action::ZoomOut, s) }
        #[unsafe(method(mdZoomReset:))]
        fn md_zoom_reset(&self, s: Option<&AnyObject>) { self.editing().perform(Action::ZoomReset, s) }
        #[unsafe(method(mdNewTab:))]
        fn md_new_tab(&self, s: Option<&AnyObject>) { self.editing().perform(Action::NewTab, s) }
        #[unsafe(method(mdExportHTML:))]
        fn md_export_html(&self, s: Option<&AnyObject>) { self.editing().perform(Action::ExportHtml, s) }
        #[unsafe(method(mdExportWord:))]
        fn md_export_word(&self, s: Option<&AnyObject>) { self.editing().perform(Action::ExportWord, s) }
        #[unsafe(method(mdLoadRemoteImages:))]
        fn md_load_remote_images(&self, s: Option<&AnyObject>) { self.editing().perform(Action::LoadRemoteImages, s) }
        #[unsafe(method(mdEditAtomAt:))]
        fn md_edit_atom_at(&self, s: Option<&AnyObject>) { self.editing().perform(Action::EditAtomAt, s) }
        #[unsafe(method(mdAtomSnippet:))]
        fn md_atom_snippet(&self, s: Option<&AnyObject>) { self.editing().perform(Action::AtomSnippet, s) }
        #[unsafe(method(mdAtomMode:))]
        fn md_atom_mode(&self, s: Option<&AnyObject>) { self.editing().perform(Action::AtomMode, s) }
        #[unsafe(method(mdToolbarSegment:))]
        fn md_toolbar_segment(&self, s: Option<&AnyObject>) { self.editing().perform(Action::ToolbarSegment, s) }
    }
);

impl EditorController {
    fn model(&self) -> &Model {
        &self.ivars().model
    }

    fn document(&self) -> Option<Retained<MdDocument>> {
        self.ivars().document.load()
    }

    fn undo_manager(&self) -> Option<Retained<NSUndoManager>> {
        self.document().and_then(|d| d.undoManager())
    }

    fn doc_dir(&self) -> Option<PathBuf> {
        let url = self.document()?.fileURL()?;
        let path = url.path()?.to_string();
        Path::new(&path).parent().map(Path::to_path_buf)
    }

    fn is_dark(&self) -> bool {
        let appearance = self.panes().preview().effective_appearance();
        matches!(
            style::appearance_of(&appearance),
            crate::markdown::palette::Appearance::Dark | crate::markdown::palette::Appearance::DarkHighContrast
        )
    }

    /// The one way to make the editor redraw what its text alone would not change, like the zoom or a formula that
    /// arrived: marks what is outdated, then updates.
    fn invalidate(&self, why: Invalidate) {
        self.model().change(move |st| st.invalidate(why));
        // The renderer may have dropped pictures for memory, which only a request brings back.
        if matches!(why, Invalidate::Zoom | Invalidate::Appearance | Invalidate::Layout) {
            self.updater().schedule_render_requests();
        }
        if update::updates_at_once(why, self.panes().source().length()) {
            self.updater().update_now();
        } else {
            self.updater().catch_up();
        }
    }

    /// Lends the current source text, which edits patch in place, and the state.
    fn with_text<R>(&self, f: impl for<'a> FnOnce(&'a Snapshot, &'a State) -> R + Send) -> R {
        self.model().with_text(|| self.panes().source().string_and_length(), f)
    }

    /// Like `with_text`, for a change of the state that depends on the text.
    fn change_with_text<R>(&self, f: impl for<'a> FnOnce(&'a Snapshot, &'a mut State) -> R + Send) -> R {
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

    /// Replaces the whole source, e.g. after loading or reverting, without an undo step.
    pub fn replace_all_text(&self, text: &str) {
        self.updater().replace_all_text(text);
    }

    pub fn print_operation(&self, info: &NSPrintInfo) -> Retained<NSPrintOperation> {
        self.sharing().print_operation(info)
    }
}

thread_local! {
    static OPEN_EDITORS: RefCell<Vec<Weak<EditorController>>> = const { RefCell::new(Vec::new()) };
}

/// Lays out every open preview again, after a setting they all follow changed.
fn relayout_all() {
    let editors: Vec<Retained<EditorController>> =
        OPEN_EDITORS.with(|e| e.borrow().iter().filter_map(Weak::load).collect());
    for editor in editors {
        editor.invalidate(Invalidate::Layout);
    }
}

/// Where `emergency_save` leaves the texts, for `offer_recovered` to point to after the next start.
pub(crate) fn recovery_dir() -> Option<PathBuf> {
    // Self-test builds keep their copies with their other output, away from the user's.
    #[cfg(feature = "selftest")]
    {
        let out = std::env::var_os("SPLAT_SELFTEST_OUT").map_or_else(std::env::temp_dir, PathBuf::from);
        Some(out.join("Recovery"))
    }
    #[cfg(not(feature = "selftest"))]
    {
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join("Library/Application Support/Splat/Recovery"))
    }
}

/// Writes the text of every open document to a recovery folder; runs from the panic hook right before the app
/// ends, so it reads only the text views and no state that might be mid-change. Returns the written files.
pub fn emergency_save() -> Vec<PathBuf> {
    if MainThreadMarker::new().is_none() {
        return vec![];
    }
    let Some(dir) = recovery_dir() else {
        return vec![];
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return vec![];
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    // The panic this runs from may have struck while the list was borrowed.
    let editors: Vec<Retained<EditorController>> = OPEN_EDITORS.with(|e| {
        e.try_borrow()
            .map(|e| e.iter().filter_map(Weak::load).collect())
            .unwrap_or_default()
    });
    let mut written = Vec::new();
    let mut failed = false;
    for (n, editor) in editors.iter().enumerate() {
        let Some(source) = editor.panes().source_if_built() else {
            continue;
        };
        let document = editor.ivars().document.load();
        // What is saved anyway need not lie around a second time.
        if document.as_ref().is_some_and(|d| !d.isDocumentEdited()) {
            continue;
        }
        let name = crate::util::file_name_of(
            &document.map_or_else(|| "Dokument".to_string(), |d| d.displayName().to_string()),
        );
        let path = dir.join(format!("{name}-{stamp}-{n}.md"));
        if std::fs::write(&path, string_of(&source.string())).is_ok() {
            written.push(path);
        } else {
            failed = true;
        }
    }
    // These are newer than the copies, and the next start would offer both; if one is missing, the copies are all that
    // is left of its text.
    if !failed {
        discard_copies();
    }
    written
}

/// After a crash, says once where `emergency_save` put the texts; nothing else would show the folder.
pub fn offer_recovered(mtm: MainThreadMarker) {
    const SHOWN_UNTIL: &str = "RecoveredTextsShownUntil";
    let Some(dir) = recovery_dir() else { return };
    let newest = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.metadata().ok()?.modified().ok())
        .filter_map(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|age| age.as_secs_f64())
        .fold(0.0, f64::max);
    let defaults = objc2_foundation::NSUserDefaults::standardUserDefaults();
    if newest <= defaults.doubleForKey(&ns(SHOWN_UNTIL)) {
        return;
    }
    defaults.setDouble_forKey(newest, &ns(SHOWN_UNTIL));
    let alert = objc2_app_kit::NSAlert::new(mtm);
    alert.setMessageText(&ns(crate::i18n::t("Splat wurde unerwartet beendet")));
    alert.setInformativeText(&ns(crate::i18n::t(
        "Der Text der geöffneten Dokumente wurde in einem eigenen Ordner gesichert.",
    )));
    alert.addButtonWithTitle(&ns(crate::i18n::t("Im Finder zeigen")));
    alert.addButtonWithTitle(&ns(crate::i18n::t("OK")));
    if alert.runModal() == objc2_app_kit::NSAlertFirstButtonReturn
        && let Some(path) = dir.to_str()
    {
        objc2_app_kit::NSWorkspace::sharedWorkspace().openURL(&objc2_foundation::NSURL::fileURLWithPath(&ns(path)));
    }
}

pub fn warm_renderer(mtm: MainThreadMarker) {
    js::renderer(mtm);
}

fn tag_of(sender: Option<&AnyObject>) -> isize {
    let Some(s) = sender else { return 0 };
    if let Some(item) = s.downcast_ref::<NSMenuItem>() {
        item.tag()
    } else if let Some(control) = s.downcast_ref::<objc2_app_kit::NSControl>() {
        control.tag()
    } else if let Some(item) = s.downcast_ref::<NSToolbarItem>() {
        item.tag()
    } else {
        0
    }
}

/// Converting through UTF-16 takes a third of the time `to_string` needs, which goes through `UTF8String`.
fn string_of(s: &objc2_foundation::NSString) -> String {
    let len = s.length();
    let mut units = vec![0u16; len];
    let buffer = std::ptr::NonNull::new(units.as_mut_ptr()).expect("vector buffer");
    // SAFETY: The buffer holds exactly `len` units, and the range covers the whole string.
    unsafe { s.getCharacters_range(buffer, NSRange::new(0, len)) };
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// The most methods the class may have outside `define_class!`; what does more belongs to a component.
    const MOST_METHODS: usize = 40;

    fn sources(dir: &Path) -> Vec<std::path::PathBuf> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(sources(&path));
            } else if path.extension().is_some_and(|e| e == "rs") {
                found.push(path);
            }
        }
        found
    }

    /// Items at the top level of a file: inside `define_class!` the selector methods stand deeper.
    fn methods_on_the_class(text: &str) -> usize {
        let mut inside = false;
        let mut methods = 0;
        for line in text.lines() {
            if inside {
                inside = line != "}";
                let item = line.strip_prefix("    ").filter(|l| !l.starts_with([' ', '/', '#']));
                if item.is_some_and(|l| l.starts_with("fn ") || l.contains(" fn ")) {
                    methods += 1;
                }
            } else {
                inside = line.starts_with("impl EditorController");
            }
        }
        methods
    }

    #[test]
    fn counts_the_methods_of_top_level_blocks_only() {
        let text = r#"
impl EditorController {
    /// fn in a comment
    fn a(&self) {
        fn inner() {}
    }

    pub(super) fn b(
        &self,
    ) {}
}

impl Other {
    fn c() {}
}

define_class!(
    impl EditorController {
        fn d(&self) {}
    }
);
"#;
        assert_eq!(methods_on_the_class(text), 2);
    }

    #[test]
    fn the_controller_only_forwards() {
        let editor = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/editor");
        let methods: usize = sources(&editor)
            .iter()
            .filter(|f| !f.to_string_lossy().contains("selftest"))
            .map(|f| {
                let text = std::fs::read_to_string(f).unwrap();
                methods_on_the_class(text.split("#[cfg(test)]").next().unwrap_or(""))
            })
            .sum();
        assert!(methods > 10, "the count looks at the wrong place: {methods}");
        assert!(
            methods <= MOST_METHODS,
            "{methods} methods on EditorController, at most {MOST_METHODS}"
        );
    }
}
