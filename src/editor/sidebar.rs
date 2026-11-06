//! The sidebar: the document's outline, the notes of its folder and a search through them. What it shows comes from
//! `notes`; what the user does here goes there as a `NoteEvent`.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::PathBuf;

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSButton, NSColor, NSControlSize, NSControlTextEditingDelegate, NSEvent, NSFont,
    NSFontWeightSemibold, NSImage, NSImageView, NSLayoutConstraint, NSLineBreakMode, NSMenu, NSOutlineView,
    NSOutlineViewDataSource, NSOutlineViewDelegate, NSScrollView, NSSearchField, NSSegmentSwitchTracking,
    NSSegmentedControl, NSStackView, NSTableColumn, NSTableViewStyle, NSTextField, NSUserInterfaceLayoutOrientation,
    NSView, NSViewController,
};
use objc2_foundation::{NSArray, NSNotification, NSObjectProtocol, NSPoint, NSRange, NSRect, NSSize};

use super::EditorController;
use super::attributes::{Attribute, Attributes, add_attribute, attributed_mut};
use super::outline::{Outline, RowItems};
use crate::i18n::t;
use crate::util::{guarded, ns};
use splat_editor_state::notes::chain_to;
pub(super) use splat_editor_state::notes::{Row, RowKind};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(in crate::editor) enum SidebarTab {
    #[default]
    Outline,
    Files,
    Search,
}

impl SidebarTab {
    const ALL: [SidebarTab; 3] = [SidebarTab::Outline, SidebarTab::Files, SidebarTab::Search];

    pub(in crate::editor) fn segment(self) -> isize {
        match self {
            SidebarTab::Outline => 0,
            SidebarTab::Files => 1,
            SidebarTab::Search => 2,
        }
    }

    pub(in crate::editor) fn of_segment(segment: isize) -> SidebarTab {
        match segment {
            1 => SidebarTab::Files,
            2 => SidebarTab::Search,
            _ => SidebarTab::Outline,
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            SidebarTab::Outline => "list.bullet.indent",
            SidebarTab::Files => "folder",
            SidebarTab::Search => "magnifyingglass",
        }
    }

    fn title(self) -> &'static str {
        match self {
            SidebarTab::Outline => t("Gliederung"),
            SidebarTab::Files => t("Dateien"),
            SidebarTab::Search => t("Suche"),
        }
    }
}

/// What the user did in the sidebar.
pub(in crate::editor) enum NoteEvent {
    TabChosen(SidebarTab),
    ChooseFolder,
    Search(String),
    Open(PathBuf),
    OpenHit(PathBuf, usize, usize),
    Rename(PathBuf),
    Reveal(PathBuf),
    LinksTo(PathBuf),
}

#[derive(Default)]
pub struct ListIvars {
    rows: RefCell<Vec<Row>>,
    items: RowItems,
    roots: RefCell<Vec<usize>>,
    open: RefCell<HashSet<String>>,
    controller: RefCell<Option<Weak<EditorController>>>,
    /// Raised while the list is replaced, which moves the selection without the user choosing anything.
    reloading: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDNotesList"]
    #[ivars = ListIvars]
    pub struct NotesList;

    unsafe impl NSObjectProtocol for NotesList {}

    unsafe impl NSOutlineViewDataSource for NotesList {
        #[unsafe(method(outlineView:numberOfChildrenOfItem:))]
        fn children_count(&self, _view: &NSOutlineView, item: Option<&AnyObject>) -> isize {
            guarded(0, || crate::util::to_isize(self.children_of(item).len()))
        }

        #[unsafe(method_id(outlineView:child:ofItem:))]
        fn child(&self, _view: &NSOutlineView, index: isize, item: Option<&AnyObject>) -> Retained<AnyObject> {
            let row = guarded(None, || self.children_of(item).get(crate::util::to_usize(index)).copied());
            self.ivars().items.item_or_none(self.mtm(), row)
        }

        #[unsafe(method(outlineView:isItemExpandable:))]
        fn expandable(&self, _view: &NSOutlineView, item: &AnyObject) -> bool {
            guarded(false, || !self.children_of(Some(item)).is_empty())
        }
    }

    unsafe impl NSControlTextEditingDelegate for NotesList {}

    unsafe impl NSOutlineViewDelegate for NotesList {
        #[unsafe(method_id(outlineView:viewForTableColumn:item:))]
        fn view_for(
            &self,
            _view: &NSOutlineView,
            _column: Option<&NSTableColumn>,
            item: &AnyObject,
        ) -> Option<Retained<NSView>> {
            guarded(None, || {
                let rows = self.ivars().rows.borrow();
                Some(row_view(self.mtm(), rows.get(self.ivars().items.row_of(item)?)?))
            })
        }

        #[unsafe(method(outlineViewItemDidExpand:))]
        fn did_expand(&self, notification: &NSNotification) {
            guarded((), || self.remember_open(notification, true));
        }

        #[unsafe(method(outlineViewItemDidCollapse:))]
        fn did_collapse(&self, notification: &NSNotification) {
            guarded((), || self.remember_open(notification, false));
        }
    }

    impl NotesList {
        #[unsafe(method(rowClicked:))]
        fn row_clicked(&self, sender: Option<&AnyObject>) {
            guarded((), || {
                if let Some(view) = sender.and_then(|s| s.downcast_ref::<NSOutlineView>()) {
                    self.chose(view, view.clickedRow());
                }
            });
        }

        #[unsafe(method(noteRename:))]
        fn note_rename(&self, sender: Option<&AnyObject>) {
            guarded((), || self.menu_chose(sender, NoteEvent::Rename));
        }

        #[unsafe(method(noteReveal:))]
        fn note_reveal(&self, sender: Option<&AnyObject>) {
            guarded((), || self.menu_chose(sender, NoteEvent::Reveal));
        }

        #[unsafe(method(noteLinks:))]
        fn note_links(&self, sender: Option<&AnyObject>) {
            guarded((), || self.menu_chose(sender, NoteEvent::LinksTo));
        }
    }
);

impl NotesList {
    fn children_of(&self, item: Option<&AnyObject>) -> Vec<usize> {
        let ivars = self.ivars();
        match item {
            Some(item) => ivars
                .items
                .row_of(item)
                .and_then(|row| ivars.rows.borrow().get(row).map(|r| r.children.clone()))
                .unwrap_or_default(),
            None => ivars.roots.borrow().clone(),
        }
    }

    fn remember_open(&self, notification: &NSNotification, open: bool) {
        if self.ivars().reloading.get() {
            return;
        }
        let Some(info) = notification.userInfo() else { return };
        let Some(item) = info.objectForKey(&ns("NSObject")) else {
            return;
        };
        let row = self.ivars().items.row_of(&item);
        let Some(key) = row.and_then(|i| self.ivars().rows.borrow().get(i).map(|r| r.key.clone())) else {
            return;
        };
        let mut remembered = self.ivars().open.borrow_mut();
        if open {
            remembered.insert(key);
        } else {
            remembered.remove(&key);
        }
    }

    fn controller(&self) -> Option<Retained<EditorController>> {
        self.ivars().controller.borrow().as_ref().and_then(Weak::load)
    }

    fn row_at(&self, view: &NSOutlineView, row: isize) -> Option<Row> {
        let i = self.ivars().items.row_of(&*view.itemAtRow(row)?)?;
        self.ivars().rows.borrow().get(i).cloned()
    }

    fn chose(&self, view: &NSOutlineView, row: isize) {
        let (Some(chosen), Some(controller)) = (self.row_at(view, row), self.controller()) else {
            return;
        };
        match chosen.kind {
            RowKind::Note(path) => controller.note_event(NoteEvent::Open(path)),
            RowKind::Hit(path, line, column) => controller.note_event(NoteEvent::OpenHit(path, line, column)),
            RowKind::Folder => {
                if let Some(item) = view.itemAtRow(row) {
                    // SAFETY: The item comes from this view, and the list's rows outlast the call.
                    unsafe {
                        if view.isItemExpanded(Some(&item)) {
                            view.collapseItem(Some(&item));
                        } else {
                            view.expandItem(Some(&item));
                        }
                    }
                }
            }
            RowKind::Remark => {}
        }
    }

    /// A menu item's represented object is the item of the row it was opened on.
    fn menu_chose(&self, sender: Option<&AnyObject>, event: fn(PathBuf) -> NoteEvent) {
        let Some(item) = sender.and_then(|s| s.downcast_ref::<objc2_app_kit::NSMenuItem>()) else {
            return;
        };
        let Some(row) = item.representedObject().and_then(|row| self.ivars().items.row_of(&row)) else {
            return;
        };
        let rows = self.ivars().rows.borrow();
        let Some(RowKind::Note(path) | RowKind::Hit(path, ..)) = rows.get(row).map(|r| &r.kind) else {
            return;
        };
        let path = path.clone();
        drop(rows);
        if let Some(controller) = self.controller() {
            controller.note_event(event(path));
        }
    }

    fn menu_for(&self, view: &NSOutlineView, row: isize) -> Option<Retained<NSMenu>> {
        let row_item = view.itemAtRow(row)?;
        let i = self.ivars().items.row_of(&row_item)?;
        let has_note = matches!(
            self.ivars().rows.borrow().get(i).map(|r| &r.kind),
            Some(RowKind::Note(_) | RowKind::Hit(..))
        );
        if !has_note {
            return None;
        }
        let mtm = self.mtm();
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &ns(""));
        for (title, action) in [
            (t("Umbenennen …"), sel!(noteRename:)),
            (t("Im Finder zeigen"), sel!(noteReveal:)),
            (t("Verweise auf diese Notiz suchen"), sel!(noteLinks:)),
        ] {
            let item = crate::util::item(mtm, title, Some(action), "");
            // SAFETY: The target is this list, which the sidebar keeps as long as the menu's view exists; the represented
            // object is the row's item, which the menu item retains.
            unsafe {
                item.setTarget(Some(self));
                item.setRepresentedObject(Some(&row_item));
            }
            menu.addItem(&item);
        }
        Some(menu)
    }
}

define_class!(
    /// Return opens the row, and the context menu belongs to the row under the mouse.
    #[unsafe(super(NSOutlineView))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDNotesOutlineView"]
    pub struct NotesOutlineView;

    unsafe impl NSObjectProtocol for NotesOutlineView {}

    impl NotesOutlineView {
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            // Return and the Return of the number block.
            const RETURN_KEYS: [u16; 2] = [36, 76];
            let handled = guarded(false, || {
                if !RETURN_KEYS.contains(&event.keyCode()) {
                    return false;
                }
                let list = self.notes_list();
                if let Some(list) = list {
                    list.chose(self, self.selectedRow());
                }
                true
            });
            if !handled {
                // SAFETY: The superclass's implementation, with this method's signature.
                let _: () = unsafe { msg_send![super(self), keyDown: event] };
            }
        }

        #[unsafe(method_id(menuForEvent:))]
        fn menu_for_event(&self, event: &NSEvent) -> Option<Retained<NSMenu>> {
            guarded(None, || {
                let point = self.convertPoint_fromView(event.locationInWindow(), None);
                self.notes_list()?.menu_for(self, self.rowAtPoint(point))
            })
        }
    }
);

impl NotesOutlineView {
    fn notes_list(&self) -> Option<Retained<NotesList>> {
        // SAFETY: Only `List::new` makes this view, and it sets a `NotesList` as the data source.
        let source = unsafe { self.dataSource() }?;
        let object: Retained<AnyObject> = source.into();
        object.downcast::<NotesList>().ok()
    }
}

fn row_view(mtm: MainThreadMarker, row: &Row) -> Retained<NSView> {
    let label = match &row.matched {
        Some(range) => {
            // A label's own line break mode counts for nothing once its text brings a paragraph style or none.
            let one_line = objc2_app_kit::NSMutableParagraphStyle::new();
            one_line.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
            let text = attributed_mut(
                &row.title,
                &Attributes::new(&[
                    Attribute::font(&NSFont::systemFontOfSize(NSFont::smallSystemFontSize())),
                    Attribute::paragraph(&one_line),
                ]),
            );
            let start = crate::markdown::text::utf16_len(&row.title[..range.start]);
            let length = crate::markdown::text::utf16_len(&row.title[range.clone()]);
            // SAFETY: A constant of AppKit, read as a value of its type.
            let semibold = unsafe { NSFontWeightSemibold };
            add_attribute(
                &text,
                &Attribute::font(&NSFont::systemFontOfSize_weight(
                    NSFont::smallSystemFontSize(),
                    semibold,
                )),
                NSRange::new(start, length),
            );
            NSTextField::labelWithAttributedString(&text, mtm)
        }
        None => NSTextField::labelWithString(&ns(&row.title), mtm),
    };
    label.setLineBreakMode(if matches!(row.kind, RowKind::Hit(..)) {
        NSLineBreakMode::ByTruncatingTail
    } else {
        NSLineBreakMode::ByTruncatingMiddle
    });
    label.setToolTip(Some(&ns(&row.tip)));
    if matches!(row.kind, RowKind::Remark) {
        label.setTextColor(Some(&NSColor::secondaryLabelColor()));
    }
    let mut views: Vec<Retained<NSView>> = Vec::with_capacity(2);
    if let Some(image) = (!row.symbol.is_empty())
        .then(|| NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(row.symbol), None))
        .flatten()
    {
        views.push(Retained::into_super(Retained::into_super(
            NSImageView::imageViewWithImage(&image, mtm),
        )));
    }
    label.setContentCompressionResistancePriority_forOrientation(
        1.0,
        objc2_app_kit::NSLayoutConstraintOrientation::Horizontal,
    );
    views.push(Retained::into_super(Retained::into_super(label)));
    let stack = NSStackView::stackViewWithViews(&NSArray::from_retained_slice(&views), mtm);
    stack.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    stack.setSpacing(4.0);
    Retained::into_super(stack)
}

/// A list of the sidebar with what feeds it.
pub(super) struct List {
    pub(super) view: Retained<NotesOutlineView>,
    pub(super) source: Retained<NotesList>,
}

impl List {
    fn new(mtm: MainThreadMarker, controller: &EditorController, label: &str) -> (List, Retained<NSScrollView>) {
        let source: Retained<NotesList> =
            // SAFETY: NSObject's initializer, called once on the newly allocated object.
            unsafe { msg_send![super(NotesList::alloc(mtm).set_ivars(ListIvars::default())), init] };
        *source.ivars().controller.borrow_mut() = Some(Weak::from(controller));
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(220.0, 600.0));
        // SAFETY: NSOutlineView's designated initializer.
        let view: Retained<NotesOutlineView> =
            unsafe { msg_send![super(NotesOutlineView::alloc(mtm).set_ivars(())), initWithFrame: frame] };
        let column = NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &ns("title"));
        column.setWidth(200.0);
        view.addTableColumn(&column);
        // SAFETY: the column was just added to this outline view, which keeps it.
        unsafe { view.setOutlineTableColumn(Some(&column)) };
        view.setHeaderView(None);
        view.setStyle(NSTableViewStyle::SourceList);
        view.setIndentationPerLevel(12.0);
        view.setAutoresizesOutlineColumn(false);
        view.setColumnAutoresizingStyle(
            objc2_app_kit::NSTableViewColumnAutoresizingStyle::FirstColumnOnlyAutoresizingStyle,
        );
        view.setAccessibilityLabel(Some(&ns(label)));
        // SAFETY: the outline view holds its data source and delegate weakly; `List` keeps the source alive.
        unsafe {
            view.setDataSource(Some(ProtocolObject::from_ref(&*source)));
            view.setDelegate(Some(ProtocolObject::from_ref(&*source)));
            view.setTarget(Some(&source));
        }
        // SAFETY: The method takes the sender, which AppKit passes.
        unsafe { view.setAction(Some(sel!(rowClicked:))) };
        let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), frame);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setDrawsBackground(false);
        scroll.setDocumentView(Some(&view));
        (List { view, source }, scroll)
    }

    /// Shows `rows`. Folders stay as the user left them; `open_all` opens those that are new, as with search results.
    pub(super) fn show(&self, rows: Vec<Row>, roots: Vec<usize>, open_all: bool) {
        let ivars = self.source.ivars();
        if open_all {
            let keys = rows.iter().filter(|r| !r.children.is_empty()).map(|r| r.key.clone());
            ivars.open.borrow_mut().extend(keys);
        }
        let count = rows.len();
        *ivars.rows.borrow_mut() = rows;
        *ivars.roots.borrow_mut() = roots;
        ivars.items.replace(self.view.mtm(), count, || {
            crate::util::with_raised(&ivars.reloading, || {
                self.view.reloadData();
                let open: Vec<usize> = {
                    let (rows, remembered) = (ivars.rows.borrow(), ivars.open.borrow());
                    (0..rows.len()).filter(|&i| remembered.contains(&rows[i].key)).collect()
                };
                for item in open.into_iter().filter_map(|i| ivars.items.item(i)) {
                    // SAFETY: The item is a row's own, as `child` hands it out, and the list keeps it.
                    unsafe { self.view.expandItem(Some(&item)) };
                }
            });
        });
    }

    /// Opens the folders down to the note at `path` and selects it.
    pub(super) fn select_note(&self, path: &std::path::Path) {
        let ivars = self.source.ivars();
        let chain = {
            let rows = ivars.rows.borrow();
            let roots = ivars.roots.borrow();
            chain_to(&rows, &roots, path)
        };
        let Some(chain) = chain else {
            return;
        };
        crate::util::with_raised(&ivars.reloading, || {
            let mut row = -1;
            for item in chain.into_iter().filter_map(|i| ivars.items.item(i)) {
                // SAFETY: The item is a row's own, as `child` hands it out, and the list keeps it.
                row = unsafe {
                    self.view.expandItem(Some(&item));
                    self.view.rowForItem(Some(&item))
                };
            }
            if row >= 0 {
                let rows = objc2_foundation::NSIndexSet::indexSetWithIndex(crate::util::to_usize(row));
                self.view.selectRowIndexes_byExtendingSelection(&rows, false);
                self.view.scrollRowToVisible(row);
            }
        });
    }
}

pub struct ActionIvars {
    controller: Weak<EditorController>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDSidebarActions"]
    #[ivars = ActionIvars]
    pub struct SidebarActions;

    unsafe impl NSObjectProtocol for SidebarActions {}

    impl SidebarActions {
        #[unsafe(method(sidebarTabChosen:))]
        fn tab_chosen(&self, sender: Option<&AnyObject>) {
            guarded((), || {
                let Some(control) = sender.and_then(|s| s.downcast_ref::<NSSegmentedControl>()) else {
                    return;
                };
                if let Some(c) = self.ivars().controller.load() {
                    c.note_event(NoteEvent::TabChosen(SidebarTab::of_segment(control.selectedSegment())));
                }
            });
        }

        #[unsafe(method(sidebarChooseFolder:))]
        fn choose_folder(&self, _sender: Option<&AnyObject>) {
            guarded((), || {
                if let Some(c) = self.ivars().controller.load() {
                    c.note_event(NoteEvent::ChooseFolder);
                }
            });
        }

        #[unsafe(method(sidebarSearch:))]
        fn search(&self, sender: Option<&AnyObject>) {
            guarded((), || {
                let Some(field) = sender.and_then(|s| s.downcast_ref::<NSSearchField>()) else {
                    return;
                };
                if let Some(c) = self.ivars().controller.load() {
                    c.note_event(NoteEvent::Search(field.stringValue().to_string()));
                }
            });
        }
    }
);

/// The views of the sidebar.
pub(super) struct Sidebar {
    pub(super) outline: Outline,
    pub(super) files: List,
    pub(super) hits: List,
    tabs: Retained<NSSegmentedControl>,
    pages: [Retained<NSView>; 3],
    folder: Retained<NSTextField>,
    pub(super) field: Retained<NSSearchField>,
    status: Retained<NSTextField>,
    /// The target of the controls, which hold it weakly.
    #[expect(dead_code, reason = "holding it keeps the target alive")]
    actions: Retained<SidebarActions>,
    tab: Cell<SidebarTab>,
}

fn activate(constraints: Vec<Retained<NSLayoutConstraint>>) {
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&constraints));
}

/// Pins `view` to the sides of `parent`, with `top` as its upper edge.
fn fill_below(view: &NSView, parent: &NSView, top: &objc2_app_kit::NSLayoutYAxisAnchor, gap: f64) {
    view.setTranslatesAutoresizingMaskIntoConstraints(false);
    parent.addSubview(view);
    activate(vec![
        view.topAnchor().constraintEqualToAnchor_constant(top, gap),
        view.bottomAnchor().constraintEqualToAnchor(&parent.bottomAnchor()),
        view.leadingAnchor().constraintEqualToAnchor(&parent.leadingAnchor()),
        view.trailingAnchor().constraintEqualToAnchor(&parent.trailingAnchor()),
    ]);
}

/// Pins `view` to the sides of `parent` with a margin, with `top` as its upper edge and at a fixed height.
fn row_below(view: &NSView, parent: &NSView, top: &objc2_app_kit::NSLayoutYAxisAnchor, gap: f64) {
    view.setTranslatesAutoresizingMaskIntoConstraints(false);
    parent.addSubview(view);
    activate(vec![
        view.topAnchor().constraintEqualToAnchor_constant(top, gap),
        view.leadingAnchor()
            .constraintEqualToAnchor_constant(&parent.leadingAnchor(), 10.0),
        view.trailingAnchor()
            .constraintEqualToAnchor_constant(&parent.trailingAnchor(), -10.0),
    ]);
}

impl Sidebar {
    #[expect(clippy::too_many_lines, reason = "one flat list of the sidebar's views")]
    pub(super) fn new(mtm: MainThreadMarker, controller: &EditorController) -> (Sidebar, Retained<NSViewController>) {
        let actions: Retained<SidebarActions> = {
            let this = SidebarActions::alloc(mtm).set_ivars(ActionIvars {
                controller: Weak::from(controller),
            });
            // SAFETY: NSObject's initializer, called once on the newly allocated object.
            unsafe { msg_send![super(this), init] }
        };
        let (outline, outline_scroll) = Outline::new(mtm, controller);
        let (files, files_scroll) = List::new(mtm, controller, t("Dateien"));
        let (hits, hits_scroll) = List::new(mtm, controller, t("Suchergebnisse"));

        // Its width is what the sidebar opens with.
        let container = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(220.0, 600.0)),
        );
        let images: Vec<Retained<NSImage>> = SidebarTab::ALL
            .iter()
            .filter_map(|tab| {
                NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(tab.symbol()), Some(&ns(tab.title())))
            })
            .collect();
        // SAFETY: The target is `actions`, which `Sidebar` keeps as long as the control exists.
        let tabs = unsafe {
            NSSegmentedControl::segmentedControlWithImages_trackingMode_target_action(
                &NSArray::from_retained_slice(&images),
                NSSegmentSwitchTracking::SelectOne,
                Some(&actions),
                Some(sel!(sidebarTabChosen:)),
                mtm,
            )
        };
        tabs.setSegmentDistribution(objc2_app_kit::NSSegmentDistribution::FillEqually);
        for tab in SidebarTab::ALL {
            tabs.setToolTip_forSegment(Some(&ns(tab.title())), tab.segment());
        }
        tabs.setSelectedSegment(SidebarTab::Outline.segment());
        row_below(&tabs, &container, &container.safeAreaLayoutGuide().topAnchor(), 6.0);

        let page = |content: &NSView| {
            let host = NSView::new(mtm);
            fill_below(&host, &container, &tabs.bottomAnchor(), 6.0);
            let _ = content;
            host
        };
        let outline_page = page(&outline_scroll);
        fill_below(&outline_scroll, &outline_page, &outline_page.topAnchor(), 0.0);

        let files_page = page(&files_scroll);
        let folder = NSTextField::labelWithString(&ns(""), mtm);
        folder.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
        folder.setFont(Some(&NSFont::systemFontOfSize(NSFont::smallSystemFontSize())));
        folder.setTextColor(Some(&NSColor::secondaryLabelColor()));
        folder.setContentCompressionResistancePriority_forOrientation(
            1.0,
            objc2_app_kit::NSLayoutConstraintOrientation::Horizontal,
        );
        let choose = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &ns("folder.badge.gearshape"),
            Some(&ns(t("Ordner wählen …"))),
        )
        .map(|image| {
            // SAFETY: The target is `actions`, which `Sidebar` keeps as long as the button exists.
            unsafe {
                NSButton::buttonWithImage_target_action(&image, Some(&actions), Some(sel!(sidebarChooseFolder:)), mtm)
            }
        });
        let header_views: Vec<Retained<NSView>> = {
            let mut views = vec![Retained::into_super(Retained::into_super(folder.clone()))];
            if let Some(button) = &choose {
                button.setBordered(false);
                button.setControlSize(NSControlSize::Small);
                button.setToolTip(Some(&ns(t("Ordner wählen …"))));
                views.push(Retained::into_super(Retained::into_super(button.clone())));
            }
            views
        };
        let header = NSStackView::stackViewWithViews(&NSArray::from_retained_slice(&header_views), mtm);
        header.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        row_below(&header, &files_page, &files_page.topAnchor(), 0.0);
        fill_below(&files_scroll, &files_page, &header.bottomAnchor(), 4.0);

        let search_page = page(&hits_scroll);
        let field = NSSearchField::new(mtm);
        field.setPlaceholderString(Some(&ns(t("In allen Notizen suchen"))));
        // SAFETY: The target is `actions`, which `Sidebar` keeps as long as the field exists.
        unsafe { field.setTarget(Some(&actions)) };
        // SAFETY: The method takes the sender, which AppKit passes.
        unsafe { field.setAction(Some(sel!(sidebarSearch:))) };
        row_below(&field, &search_page, &search_page.topAnchor(), 0.0);
        let status = NSTextField::labelWithString(&ns(""), mtm);
        status.setFont(Some(&NSFont::systemFontOfSize(NSFont::smallSystemFontSize())));
        status.setTextColor(Some(&NSColor::secondaryLabelColor()));
        status.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
        row_below(&status, &search_page, &field.bottomAnchor(), 4.0);
        fill_below(&hits_scroll, &search_page, &status.bottomAnchor(), 4.0);

        let pane = NSViewController::new(mtm);
        pane.setView(&container);
        let sidebar = Sidebar {
            outline,
            files,
            hits,
            tabs,
            pages: [outline_page, files_page, search_page],
            folder,
            field,
            status,
            actions,
            tab: Cell::new(SidebarTab::Outline),
        };
        sidebar.select(SidebarTab::Outline);
        (sidebar, pane)
    }

    pub(super) fn tab(&self) -> SidebarTab {
        self.tab.get()
    }

    pub(super) fn select(&self, tab: SidebarTab) {
        self.tab.set(tab);
        self.tabs.setSelectedSegment(tab.segment());
        for (page, each) in self.pages.iter().zip(SidebarTab::ALL) {
            page.setHidden(each != tab);
        }
    }

    pub(super) fn show_folder(&self, name: &str, tip: &str) {
        self.folder.setStringValue(&ns(name));
        self.folder.setToolTip(Some(&ns(tip)));
    }

    pub(super) fn show_status(&self, text: &str) {
        self.status.setStringValue(&ns(text));
    }
}

#[cfg(feature = "selftest")]
impl List {
    fn row_titled(&self, title: &str) -> Option<isize> {
        let ivars = self.source.ivars();
        (0..self.view.numberOfRows())
            .find(|&row| {
                self.source
                    .row_at(&self.view, row)
                    .is_some_and(|found| found.title == title)
            })
            .filter(|_| ivars.rows.borrow().iter().any(|row| row.title == title))
    }

    /// What a click on the row titled `title` does.
    pub(super) fn click(&self, title: &str) -> bool {
        let row = self.row_titled(title);
        if let Some(row) = row {
            self.source.chose(&self.view, row);
        }
        row.is_some()
    }

    /// The items of the context menu of the row titled `title`.
    pub(super) fn menu_of(&self, title: &str) -> Vec<String> {
        self.row_titled(title)
            .and_then(|row| self.source.menu_for(&self.view, row))
            .map(|menu| menu.itemArray().iter().map(|item| item.title().to_string()).collect())
            .unwrap_or_default()
    }

    pub(super) fn shown_rows(&self) -> isize {
        self.view.numberOfRows()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tab_is_found_again_by_its_segment() {
        for tab in SidebarTab::ALL {
            assert_eq!(SidebarTab::of_segment(tab.segment()), tab);
        }
        assert_eq!(SidebarTab::of_segment(-1), SidebarTab::Outline, "no segment selected");
    }
}
