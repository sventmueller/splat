//! The editor window: Markdown source and preview side by side. One controller object serves AppKit as delegate of
//! both views; its work is split over the modules below, and README.md describes the rules that keep the delegate
//! callbacks from stepping on each other.

pub mod actions;
mod atom;
mod atom_edit;
pub mod decoder;
mod dialogs;
mod editing;
mod env;
mod export;
mod images;
mod js;
mod links;
mod mathfield;
mod navigation;
mod outline;
mod progressive;
mod remote;
mod render;
mod safe_image;
mod scroll;
mod selection;
#[cfg(feature = "selftest")]
pub mod selftest;
mod style;
mod sync;
mod toolbar;
mod update;
mod views;
mod window;

use std::cell::{Cell, OnceCell, RefCell};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAppearanceCustomization, NSMenu, NSMenuItem, NSMenuItemValidation, NSPopover, NSPopoverDelegate, NSScrollView,
    NSSharingService, NSSharingServicePicker, NSSharingServicePickerDelegate, NSSharingServicePickerToolbarItem,
    NSSharingServicePickerToolbarItemDelegate, NSSplitViewItem, NSTextDelegate, NSTextStorage, NSTextStorageDelegate,
    NSTextStorageEditActions, NSTextView, NSTextViewDelegate, NSToolbar, NSToolbarDelegate, NSToolbarItem,
    NSToolbarItemIdentifier, NSToolbarItemValidation, NSWindow, NSWindowController, NSWindowDelegate,
};
use objc2_foundation::{NSArray, NSCoder, NSNotification, NSObjectProtocol, NSRange, NSRect, NSUndoManager};

use crate::document::MdDocument;
use crate::markdown::commands::{Active, InlineStyle, Pending};
use crate::markdown::doc::Doc;
use crate::preview::layout::Layout;
use crate::preview::map::Index;
use crate::util::ns;
use actions::Action;

use images::ImageStore;
use scroll::ScrollSpan;
use style::SourceFonts;
use sync::{Blocks, Snapshot};
use views::{PreviewTextView, SourceTextView};

/// What the controller knows about the document, derived from the source text of version `version`.
#[derive(Default)]
struct State {
    text: String,
    version: u64,
    doc: Doc,
    layout: Layout,
    index: Index,
    blocks: Blocks,
    caret: Option<usize>,
    zoom: f64,
    fonts: Option<SourceFonts>,
    /// Preview range (before the change) the user edited directly, and the length delta.
    dirty_preview: Option<(Range<usize>, isize)>,
    rendered_dark: Option<bool>,
    stale: Stale,
    /// Styles for the next typed text, valid while the source still equals the stored text.
    pending: Option<(Pending, String)>,
    /// Syntax colors currently applied to the source view.
    spans: std::sync::Arc<Vec<(Range<usize>, crate::markdown::highlight::Style)>>,
    /// Source and preview UTF-16 ranges of every top-level block, for scroll synchronisation; built on first use.
    scroll_spans: Option<Rc<Vec<ScrollSpan>>>,
}

impl State {
    /// The state was computed from the text `snap` holds; equal versions save comparing the texts.
    fn is_current(&self, snap: &Snapshot) -> bool {
        self.version == snap.version || self.text == snap.text
    }
}

/// What the next update rebuilds although the source text is unchanged.
#[derive(Default, Clone, Copy)]
struct Stale {
    /// The syntax colors of the whole source.
    colors: bool,
    /// The layout, which may now show rendered formulas, diagrams or images.
    layout: bool,
}

/// Why the editor redraws although the source text did not change; see `EditorController::invalidate`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Invalidate {
    /// The whole text was replaced, e.g. after loading or reverting.
    Text,
    /// The fonts changed.
    Zoom,
    /// Light and dark appearance switched.
    Appearance,
    /// The preview no longer matches what the controller knows, e.g. after an edit it could not follow.
    Preview,
    /// Rendered formulas, diagrams or loaded images arrived.
    Arrivals,
    /// A setting changed what the preview shows for the same text.
    Layout,
}

#[derive(Clone, PartialEq)]
struct ActiveKey {
    version: u64,
    selection: (usize, usize),
    preview: bool,
    state_version: u64,
    pending: Option<(usize, Vec<InlineStyle>)>,
}

/// The source text as last read from the view, kept in step with every edit.
#[derive(Default)]
struct SourceCopy {
    snapshot: RefCell<Option<Rc<Snapshot>>>,
    /// Counts the character edits of the source storage.
    version: Cell<u64>,
    /// Raised by every update; a background result for an older generation is outdated.
    generation: Cell<u64>,
    latest_job: std::sync::Arc<std::sync::atomic::AtomicU64>,
    update_scheduled: Cell<bool>,
}

/// The selection of the focused view, shown as highlight in the other one.
#[derive(Default)]
struct Marks {
    /// The source selection while the preview has the focus. The source then shows it as a highlight: a text view
    /// without focus draws its own selection grey, whatever colour is set for it.
    source_mirror: Cell<Option<NSRange>>,
    /// Ranges currently highlighted as the other view's selection.
    source: Cell<Option<NSRange>>,
    preview: Cell<Option<NSRange>>,
}

/// An open popover editing the source of a formula or diagram in place.
struct AtomEdit {
    popover: Retained<NSPopover>,
    view: Retained<NSTextView>,
    start: usize,
    len: Cell<usize>,
    original: String,
    cancelled: Cell<bool>,
    range_start: usize,
    kind: atom::AtomKind,
    diagram: crate::markdown::snippets::DiagramKind,
    status: Retained<objc2_app_kit::NSTextField>,
    scroll: Retained<NSScrollView>,
    editor_frame: NSRect,
    visual: Cell<bool>,
    /// Text just inserted for a new formula, removed again if the editor closes without one.
    inserted: Cell<Option<(usize, usize)>>,
    /// The document counts as edited while the editor holds changes, so quitting saves them.
    marked_edited: Cell<bool>,
}

#[derive(Default)]
struct Atoms {
    edit: RefCell<Option<AtomEdit>>,
    /// Start of the formula or diagram whose editor just closed; the caret resting on it does not reopen it.
    closed: Cell<Option<usize>>,
}

/// A change the preview announced before applying it, as range of its old text and replacement.
enum Announced {
    Edit(Range<usize>, String),
    /// Several changes or one without text: found by comparing the texts afterwards.
    Unknown,
}

pub struct Ivars {
    document: Weak<MdDocument>,
    source: OnceCell<Retained<SourceTextView>>,
    preview: OnceCell<Retained<PreviewTextView>>,
    preview_item: OnceCell<Retained<NSSplitViewItem>>,
    outline: OnceCell<outline::Outline>,
    outline_item: OnceCell<Retained<NSSplitViewItem>>,
    state: RefCell<State>,
    copy: SourceCopy,
    marks: Marks,
    atoms: Atoms,
    images: RefCell<ImageStore>,
    remote_allowed: Cell<bool>,
    /// The bar above the preview offering to load images from the web.
    remote_banner: OnceCell<Retained<objc2_app_kit::NSView>>,
    /// Raised while the controller changes its own views; the delegate callbacks then ignore the changes.
    busy: Cell<bool>,
    scrolling: Cell<bool>,
    scroll_after_undo: Cell<bool>,
    /// Set from the moment the preview announces a user edit until it reports it done.
    preview_edit: RefCell<Option<Announced>>,
    share_path: RefCell<Option<PathBuf>>,
    /// Toolbar and menu validation asks for the formatting at the caret once per item and event.
    active_cache: RefCell<Option<(ActiveKey, Active)>>,
}

/// Raises a flag for as long as it lives and restores the previous value when dropped, also on an early return.
#[must_use]
struct Raised<'a>(&'a Cell<bool>, bool);

impl<'a> Raised<'a> {
    fn new(flag: &'a Cell<bool>) -> Self {
        Raised(flag, flag.replace(true))
    }
}

impl Drop for Raised<'_> {
    fn drop(&mut self) {
        self.0.set(self.1);
    }
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
            self.recolor_marks();
        }

        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, _notification: &NSNotification) {
            self.recolor_marks();
        }

        #[unsafe(method(windowDidMove:))]
        fn window_did_move(&self, _notification: &NSNotification) {
            self.remember_frame();
        }

        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, _notification: &NSNotification) {
            self.remember_frame();
        }
    }

    unsafe impl NSTextStorageDelegate for EditorController {
        #[unsafe(method(textStorage:didProcessEditing:range:changeInLength:))]
        fn storage_edited(&self, storage: &NSTextStorage, mask: NSTextStorageEditActions, range: NSRange, delta: isize) {
            if mask.contains(NSTextStorageEditActions::EditedCharacters) {
                self.source_edited(storage, range, delta);
            }
        }
    }

    unsafe impl NSTextDelegate for EditorController {
        #[unsafe(method(textDidChange:))]
        fn text_did_change(&self, notification: &NSNotification) {
            if self.ivars().busy.get() {
                return;
            }
            if self.is_atom_view(notification.object().as_deref()) {
                self.atom_text_changed();
            } else if self.is_preview(notification.object().as_deref()) {
                self.preview_changed();
            } else if self.source_typed_with_pending() {
            } else if self.preview_focused() {
                self.update();
            } else {
                self.schedule_update();
            }
        }
    }

    unsafe impl NSTextViewDelegate for EditorController {
        #[unsafe(method_id(undoManagerForTextView:))]
        fn text_view_undo_manager(&self, _view: &NSTextView) -> Option<Retained<NSUndoManager>> {
            self.undo_manager()
        }

        #[unsafe(method(textView:shouldChangeTextInRanges:replacementStrings:))]
        fn should_change(&self, view: &NSTextView, ranges: &AnyObject, strings: Option<&AnyObject>) -> bool {
            if self.is_preview(Some(view)) && !self.ivars().busy.get() {
                self.preview_will_change(ranges, strings);
            }
            true
        }

        #[unsafe(method(textViewDidChangeSelection:))]
        fn selection_changed(&self, notification: &NSNotification) {
            if self.ivars().busy.get() || self.preview_editing() {
                return;
            }
            if self.is_preview(notification.object().as_deref()) {
                self.preview_selection_changed();
            } else if self.is_source(notification.object().as_deref()) {
                self.drop_pending_unless_at(None);
                if self.ivars().marks.source_mirror.get().is_none() {
                    self.mark_source_selection_in_preview();
                }
            }
        }

        #[unsafe(method(textView:doCommandBySelector:))]
        fn do_command(&self, view: &NSTextView, command: Sel) -> bool {
            if self.is_atom_view(Some(view)) {
                self.atom_command(command)
            } else if self.is_preview(Some(view)) {
                self.preview_command(command)
            } else {
                self.source_command(command)
            }
        }

        #[unsafe(method(textView:doubleClickedOnCell:inRect:atIndex:))]
        fn double_clicked_cell(&self, view: &NSTextView, _cell: &AnyObject, frame: NSRect, index: usize) {
            if self.is_preview(Some(view)) {
                self.edit_atom_at_preview(index, Some(frame));
            }
        }

        #[unsafe(method(textView:clickedOnCell:inRect:atIndex:))]
        fn clicked_cell(&self, view: &NSTextView, _cell: &AnyObject, frame: NSRect, index: usize) {
            if self.is_preview(Some(view)) {
                self.edit_atom_at_preview(index, Some(frame));
            }
        }

        #[unsafe(method(textView:clickedOnLink:atIndex:))]
        fn clicked_link(&self, view: &NSTextView, link: &AnyObject, index: usize) -> bool {
            self.link_clicked(view, link, index)
        }

        #[unsafe(method_id(textView:menu:forEvent:atIndex:))]
        fn context_menu(&self, view: &NSTextView, menu: &NSMenu, _event: &AnyObject, index: usize) -> Option<Retained<NSMenu>> {
            Some(self.adjust_context_menu(menu, view, index))
        }
    }

    unsafe impl NSPopoverDelegate for EditorController {
        #[unsafe(method(popoverDidClose:))]
        fn popover_did_close(&self, notification: &NSNotification) {
            let current = self.ivars().atoms.edit.borrow().as_ref().map(|e| e.popover.clone());
            let object = notification.object();
            if let (Some(current), Some(object)) = (current, object)
                && std::ptr::eq(&*current as &AnyObject, &*object)
            {
                self.atom_edit_closed();
            }
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
            self.share_file(true);
            let mut services = proposed.to_vec();
            services.extend(self.export_sharing_services());
            NSArray::from_retained_slice(&services)
        }

        #[unsafe(method(sharingServicePicker:didChooseSharingService:))]
        fn share_chosen(&self, _picker: &NSSharingServicePicker, service: Option<&NSSharingService>) {
            if service.is_some() {
                self.share_file(true);
            }
        }
    }

    unsafe impl NSSharingServicePickerToolbarItemDelegate for EditorController {
        // AppKit asks for the items on every toolbar validation, so the file is only rewritten when the menu opens.
        #[unsafe(method_id(itemsForSharingServicePickerToolbarItem:))]
        fn share_items(&self, _item: &NSSharingServicePickerToolbarItem) -> Retained<NSArray> {
            match self.share_file(false) {
                Some(url) => NSArray::from_retained_slice(&[crate::util::obj(url)]),
                None => NSArray::new(),
            }
        }
    }

    unsafe impl NSMenuItemValidation for EditorController {
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            self.validate(item.action(), item.tag(), Some(item))
        }
    }

    unsafe impl NSToolbarItemValidation for EditorController {
        #[unsafe(method(validateToolbarItem:))]
        fn validate_toolbar_item(&self, item: &NSToolbarItem) -> bool {
            self.validate(item.action(), item.tag(), None)
        }
    }

    impl EditorController {
        #[unsafe(method(flushUpdate))]
        fn flush_update(&self) {
            self.ivars().copy.update_scheduled.set(false);
            self.refresh();
        }

        #[unsafe(method(renderPendingBlocks))]
        fn render_pending_blocks_now(&self) {
            self.render_pending_blocks();
        }

        #[unsafe(method(flushRenderRequests))]
        fn flush_render_requests(&self) {
            self.request_renders();
        }

        #[unsafe(method(applyArrivedImages))]
        fn apply_arrived_images(&self) {
            self.invalidate(Invalidate::Arrivals);
            self.refresh_atom_status();
        }

        #[unsafe(method(openAtomUnderCaret))]
        fn open_atom_under_caret(&self) {
            if let Some(range) = self.atom_under_preview_caret()
                && self.ivars().atoms.closed.get() != Some(range.start)
            {
                self.open_atom_editor(range, None);
            }
        }

        #[unsafe(method(clipBoundsChanged:))]
        fn clip_bounds_changed(&self, notification: &NSNotification) {
            self.view_scrolled(notification);
        }

        #[unsafe(method(undoDidChange:))]
        fn undo_did_change(&self, _notification: &NSNotification) {
            self.ivars().scroll_after_undo.set(true);
            if self.ivars().marks.source_mirror.get().is_some() {
                self.set_source_selection(self.source().selectedRange());
            }
            // With TextKit 2, undoing in the source sends no textDidChange.
            if !self.ivars().busy.get() && self.ivars().state.try_borrow().is_ok() {
                self.refresh();
            }
        }

        #[unsafe(method(encodeRestorableStateWithCoder:))]
        fn encode_state(&self, coder: &NSCoder) {
            // SAFETY: The superclass's implementation, with this method's signature.
            let _: () = unsafe { msg_send![super(self), encodeRestorableStateWithCoder: coder] };
            let collapsed = self.ivars().preview_item.get().is_some_and(|i| i.isCollapsed());
            let outline = self.outline_visible();
            // SAFETY: Window restoration always passes a keyed coder.
            unsafe {
                coder.encodeBool_forKey(collapsed, &ns("previewCollapsed"));
                coder.encodeBool_forKey(outline, &ns("outlineShown"));
                coder.encodeDouble_forKey(self.ivars().state.borrow().zoom, &ns("zoom"));
            }
        }

        #[unsafe(method(restoreStateWithCoder:))]
        fn restore_state(&self, coder: &NSCoder) {
            // SAFETY: The superclass's implementation, with this method's signature.
            let _: () = unsafe { msg_send![super(self), restoreStateWithCoder: coder] };
            if let Some(item) = self.ivars().preview_item.get() {
                // SAFETY: Window restoration always passes a keyed coder.
                item.setCollapsed(unsafe { coder.decodeBoolForKey(&ns("previewCollapsed")) });
            }
            // SAFETY: Window restoration always passes a keyed coder.
            let zoom = unsafe { coder.decodeDoubleForKey(&ns("zoom")) };
            // SAFETY: Window restoration always passes a keyed coder.
            if unsafe { coder.decodeBoolForKey(&ns("outlineShown")) } && !self.outline_visible() {
                self.toggle_outline();
            }
            if zoom > 0.3 {
                self.set_zoom(zoom);
            }
        }

        // One method per command of `actions`: AppKit finds a menu item's target by asking who responds to its
        // selector.
        #[unsafe(method(mdBold:))]
        fn md_bold(&self, s: Option<&AnyObject>) { self.perform(Action::Bold, s) }
        #[unsafe(method(mdItalic:))]
        fn md_italic(&self, s: Option<&AnyObject>) { self.perform(Action::Italic, s) }
        #[unsafe(method(mdUnderline:))]
        fn md_underline(&self, s: Option<&AnyObject>) { self.perform(Action::Underline, s) }
        #[unsafe(method(mdStrikethrough:))]
        fn md_strike(&self, s: Option<&AnyObject>) { self.perform(Action::Strikethrough, s) }
        #[unsafe(method(mdHighlight:))]
        fn md_highlight(&self, s: Option<&AnyObject>) { self.perform(Action::Highlight, s) }
        #[unsafe(method(mdInlineCode:))]
        fn md_code(&self, s: Option<&AnyObject>) { self.perform(Action::InlineCode, s) }
        #[unsafe(method(mdSuperscript:))]
        fn md_sup(&self, s: Option<&AnyObject>) { self.perform(Action::Superscript, s) }
        #[unsafe(method(mdSubscript:))]
        fn md_sub(&self, s: Option<&AnyObject>) { self.perform(Action::Subscript, s) }
        #[unsafe(method(mdHeading:))]
        fn md_heading(&self, s: Option<&AnyObject>) { self.perform(Action::Heading, s) }
        #[unsafe(method(mdBulletList:))]
        fn md_bullets(&self, s: Option<&AnyObject>) { self.perform(Action::BulletList, s) }
        #[unsafe(method(mdNumberedList:))]
        fn md_numbers(&self, s: Option<&AnyObject>) { self.perform(Action::NumberedList, s) }
        #[unsafe(method(mdTaskList:))]
        fn md_tasks(&self, s: Option<&AnyObject>) { self.perform(Action::TaskList, s) }
        #[unsafe(method(mdQuote:))]
        fn md_quote(&self, s: Option<&AnyObject>) { self.perform(Action::Quote, s) }
        #[unsafe(method(mdCodeBlock:))]
        fn md_code_block(&self, s: Option<&AnyObject>) { self.perform(Action::CodeBlock, s) }
        #[unsafe(method(mdInsertAlert:))]
        fn md_alert(&self, s: Option<&AnyObject>) { self.perform(Action::Alert, s) }
        #[unsafe(method(mdRemoveFormatting:))]
        fn md_clear(&self, s: Option<&AnyObject>) { self.perform(Action::RemoveFormatting, s) }
        #[unsafe(method(mdEditAtom:))]
        fn md_edit_atom(&self, s: Option<&AnyObject>) { self.perform(Action::EditAtom, s) }
        #[unsafe(method(mdInsertLink:))]
        fn md_link(&self, s: Option<&AnyObject>) { self.perform(Action::Link, s) }
        #[unsafe(method(mdInsertImage:))]
        fn md_image(&self, s: Option<&AnyObject>) { self.perform(Action::Image, s) }
        #[unsafe(method(mdInsertTable:))]
        fn md_table(&self, s: Option<&AnyObject>) { self.perform(Action::Table, s) }
        #[unsafe(method(mdInsertFormula:))]
        fn md_formula(&self, s: Option<&AnyObject>) { self.perform(Action::Formula, s) }
        #[unsafe(method(mdInsertDiagram:))]
        fn md_diagram(&self, s: Option<&AnyObject>) { self.perform(Action::Diagram, s) }
        #[unsafe(method(mdInsertToc:))]
        fn md_toc(&self, s: Option<&AnyObject>) { self.perform(Action::Toc, s) }
        #[unsafe(method(mdInsertFootnote:))]
        fn md_footnote(&self, s: Option<&AnyObject>) { self.perform(Action::Footnote, s) }
        #[unsafe(method(mdInsertRule:))]
        fn md_rule(&self, s: Option<&AnyObject>) { self.perform(Action::Rule, s) }
        #[unsafe(method(mdTableRowAbove:))]
        fn md_row_above(&self, s: Option<&AnyObject>) { self.perform(Action::RowAbove, s) }
        #[unsafe(method(mdTableRowBelow:))]
        fn md_row_below(&self, s: Option<&AnyObject>) { self.perform(Action::RowBelow, s) }
        #[unsafe(method(mdTableColumnLeft:))]
        fn md_col_left(&self, s: Option<&AnyObject>) { self.perform(Action::ColumnLeft, s) }
        #[unsafe(method(mdTableColumnRight:))]
        fn md_col_right(&self, s: Option<&AnyObject>) { self.perform(Action::ColumnRight, s) }
        #[unsafe(method(mdTableDeleteRow:))]
        fn md_del_row(&self, s: Option<&AnyObject>) { self.perform(Action::DeleteRow, s) }
        #[unsafe(method(mdTableDeleteColumn:))]
        fn md_del_col(&self, s: Option<&AnyObject>) { self.perform(Action::DeleteColumn, s) }
        #[unsafe(method(mdTableAlign:))]
        fn md_align(&self, s: Option<&AnyObject>) { self.perform(Action::TableAlign, s) }
        #[unsafe(method(mdTableFormat:))]
        fn md_table_format(&self, s: Option<&AnyObject>) { self.perform(Action::TableFormat, s) }
        #[unsafe(method(mdTogglePreview:))]
        fn md_toggle_preview(&self, s: Option<&AnyObject>) { self.perform(Action::TogglePreview, s) }
        #[unsafe(method(mdToggleOutline:))]
        fn md_toggle_outline(&self, s: Option<&AnyObject>) { self.perform(Action::ToggleOutline, s) }
        #[unsafe(method(mdTypography:))]
        fn md_typography(&self, s: Option<&AnyObject>) { self.perform(Action::Typography, s) }
        #[unsafe(method(mdZoomIn:))]
        fn md_zoom_in(&self, s: Option<&AnyObject>) { self.perform(Action::ZoomIn, s) }
        #[unsafe(method(mdZoomOut:))]
        fn md_zoom_out(&self, s: Option<&AnyObject>) { self.perform(Action::ZoomOut, s) }
        #[unsafe(method(mdZoomReset:))]
        fn md_zoom_reset(&self, s: Option<&AnyObject>) { self.perform(Action::ZoomReset, s) }
        #[unsafe(method(mdNewTab:))]
        fn md_new_tab(&self, s: Option<&AnyObject>) { self.perform(Action::NewTab, s) }
        #[unsafe(method(mdExportHTML:))]
        fn md_export_html(&self, s: Option<&AnyObject>) { self.perform(Action::ExportHtml, s) }
        #[unsafe(method(mdExportWord:))]
        fn md_export_word(&self, s: Option<&AnyObject>) { self.perform(Action::ExportWord, s) }
        #[unsafe(method(mdLoadRemoteImages:))]
        fn md_load_remote_images(&self, s: Option<&AnyObject>) { self.perform(Action::LoadRemoteImages, s) }
        #[unsafe(method(mdEditAtomAt:))]
        fn md_edit_atom_at(&self, s: Option<&AnyObject>) { self.perform(Action::EditAtomAt, s) }
        #[unsafe(method(mdAtomSnippet:))]
        fn md_atom_snippet(&self, s: Option<&AnyObject>) { self.perform(Action::AtomSnippet, s) }
        #[unsafe(method(mdAtomMode:))]
        fn md_atom_mode(&self, s: Option<&AnyObject>) { self.perform(Action::AtomMode, s) }
        #[unsafe(method(mdToolbarSegment:))]
        fn md_toolbar_segment(&self, s: Option<&AnyObject>) { self.perform(Action::ToolbarSegment, s) }
    }
);

impl EditorController {
    fn source(&self) -> &SourceTextView {
        self.ivars().source.get().expect("source view")
    }

    fn preview(&self) -> &PreviewTextView {
        self.ivars().preview.get().expect("preview view")
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

    fn is_preview(&self, object: Option<&AnyObject>) -> bool {
        let preview: &AnyObject = self.preview();
        object.is_some_and(|o| std::ptr::eq(o, preview))
    }

    fn is_source(&self, object: Option<&AnyObject>) -> bool {
        let Some(source) = self.ivars().source.get() else {
            return false;
        };
        let source: &AnyObject = source;
        object.is_some_and(|o| std::ptr::eq(o, source))
    }

    fn preview_focused(&self) -> bool {
        let Some(window) = self.window() else { return false };
        let responder = window.firstResponder();
        let preview: &AnyObject = self.preview();
        responder.is_some_and(|r| std::ptr::eq(&*r as &AnyObject, preview))
    }

    fn preview_visible(&self) -> bool {
        self.ivars().preview_item.get().is_some_and(|i| !i.isCollapsed())
    }

    /// The preview text view is applying a user edit; its storage is ahead of the layout until textDidChange.
    fn preview_editing(&self) -> bool {
        self.ivars().preview_edit.try_borrow().map_or(true, |e| e.is_some())
    }

    fn is_dark(&self) -> bool {
        let appearance = self.preview().effectiveAppearance();
        matches!(
            style::appearance_of(&appearance),
            crate::markdown::palette::Appearance::Dark | crate::markdown::palette::Appearance::DarkHighContrast
        )
    }

    /// While the result lives, the controller's own changes to its views do not count as user input.
    fn silently(&self) -> Raised<'_> {
        Raised::new(&self.ivars().busy)
    }

    /// The one way to make the editor redraw what its text alone would not change: marks what is outdated, then
    /// updates.
    fn invalidate(&self, why: Invalidate) {
        {
            let mut guard = self.ivars().state.borrow_mut();
            let st = &mut *guard;
            match why {
                Invalidate::Text | Invalidate::Zoom | Invalidate::Appearance => {
                    st.stale.colors = true;
                    st.blocks.forget();
                    st.dirty_preview = None;
                }
                Invalidate::Preview => {
                    st.blocks.forget();
                    st.dirty_preview = None;
                }
                Invalidate::Layout => {
                    st.stale.layout = true;
                    st.blocks.forget();
                    st.dirty_preview = None;
                }
                Invalidate::Arrivals => {
                    st.stale.layout = true;
                    // A loaded image keeps the block's text, so its key would not change on its own.
                    let layout = &st.layout;
                    st.blocks
                        .invalidate(|i| layout.blocks.get(i).is_some_and(update::shows_image));
                }
            }
            if why == Invalidate::Text {
                st.pending = None;
            }
        }
        if why == Invalidate::Text || why == Invalidate::Preview {
            self.update();
        } else {
            self.refresh();
        }
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
fn recovery_dir() -> Option<PathBuf> {
    // Self-test builds keep their copies with their other output, away from the user's.
    #[cfg(feature = "selftest")]
    if let Some(out) = std::env::var_os("SPLAT_SELFTEST_OUT") {
        return Some(PathBuf::from(out).join("Recovery"));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/Splat/Recovery"))
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
    let editors: Vec<Retained<EditorController>> = OPEN_EDITORS.with(|e| {
        e.try_borrow()
            .map(|e| e.iter().filter_map(Weak::load).collect())
            .unwrap_or_default()
    });
    let mut written = Vec::new();
    for (n, editor) in editors.iter().enumerate() {
        let Some(source) = editor.ivars().source.get() else {
            continue;
        };
        let name = editor
            .ivars()
            .document
            .load()
            .map(|d| d.displayName().to_string())
            .unwrap_or_else(|| "Dokument".into())
            .replace(['/', ':'], "-");
        let path = dir.join(format!("{name}-{stamp}-{n}.md"));
        if std::fs::write(&path, string_of(&source.string())).is_ok() {
            written.push(path);
        }
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
