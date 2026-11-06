//! The outline sidebar: the document's headings as a tree, a click on one shows it in both panes. Also the objects that
//! stand for the rows of this and the other lists of the sidebar.

use std::cell::{Cell, OnceCell, RefCell};
use std::ops::Range;

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSControlTextEditingDelegate, NSLineBreakMode, NSOutlineView, NSOutlineViewDataSource, NSOutlineViewDelegate,
    NSScrollView, NSTableColumn, NSTableViewStyle, NSTextField, NSView,
};
use objc2_foundation::{NSNotification, NSObjectProtocol, NSPoint, NSRect, NSSize};
use splat_editor_state::outline::{Node, tree};

use super::EditorController;
use crate::markdown::text::Byte;
use crate::util::{guarded, ns};

define_class!(
    /// The item of one row of an outline view, which knows the row by its place in the list's data.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDOutlineRow"]
    #[ivars = usize]
    pub struct RowItem;

    unsafe impl NSObjectProtocol for RowItem {}
);

impl RowItem {
    fn new(mtm: MainThreadMarker, row: usize) -> Retained<RowItem> {
        // SAFETY: NSObject's initializer, called once on the newly allocated object.
        unsafe { msg_send![super(RowItem::alloc(mtm).set_ivars(row)), init] }
    }
}

/// One object per row for as long as the data it stands for: an outline view keeps its items without retaining them and
/// tells them apart by identity.
#[derive(Default)]
pub(super) struct RowItems {
    items: RefCell<Vec<Retained<RowItem>>>,
    /// What `child:ofItem:` hands out when asked for a row the data does not have, as it has to hand out an object.
    none: OnceCell<Retained<RowItem>>,
}

impl RowItems {
    /// Gives `rows` rows objects of their own; the old ones stay alive until `reload`, which shows the new rows, is done.
    pub(super) fn replace<R>(&self, mtm: MainThreadMarker, rows: usize, reload: impl FnOnce() -> R) -> R {
        let old = self
            .items
            .replace((0..rows).map(|row| RowItem::new(mtm, row)).collect());
        let reloaded = reload();
        drop(old);
        reloaded
    }

    /// The object of `row`.
    pub(super) fn item(&self, row: usize) -> Option<Retained<AnyObject>> {
        self.items.borrow().get(row).map(|item| item.clone().into())
    }

    /// The object of `row`, or one that stands for no row.
    pub(super) fn item_or_none(&self, mtm: MainThreadMarker, row: Option<usize>) -> Retained<AnyObject> {
        row.and_then(|row| self.item(row))
            .unwrap_or_else(|| self.none.get_or_init(|| RowItem::new(mtm, usize::MAX)).clone().into())
    }

    /// The row `item` stands for, if it is one of the current rows: an item of rows that were replaced stands for none.
    pub(super) fn row_of(&self, item: &AnyObject) -> Option<usize> {
        let row = *item.downcast_ref::<RowItem>()?.ivars();
        let items = self.items.borrow();
        let current: &AnyObject = items.get(row)?;
        std::ptr::eq(item, current).then_some(row)
    }
}

#[derive(Default)]
pub struct SourceIvars {
    nodes: RefCell<Vec<Node>>,
    roots: RefCell<Vec<usize>>,
    items: RowItems,
    /// The tree as last shown, to tell a new heading from text moving the existing ones.
    shown: RefCell<Vec<(u8, String)>>,
    controller: RefCell<Option<Weak<EditorController>>>,
    /// Set while the tree is replaced, which clears the selection without the user choosing anything.
    reloading: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDOutlineSource"]
    #[ivars = SourceIvars]
    pub struct OutlineSource;

    unsafe impl NSObjectProtocol for OutlineSource {}

    unsafe impl NSOutlineViewDataSource for OutlineSource {
        #[unsafe(method(outlineView:numberOfChildrenOfItem:))]
        fn children_count(&self, _view: &NSOutlineView, item: Option<&AnyObject>) -> isize {
            guarded(0, || crate::util::to_isize(self.children_of(item).len()))
        }

        #[unsafe(method_id(outlineView:child:ofItem:))]
        fn child(&self, _view: &NSOutlineView, index: isize, item: Option<&AnyObject>) -> Retained<AnyObject> {
            let row = guarded(None, || {
                self.children_of(item).get(crate::util::to_usize(index)).copied()
            });
            self.ivars().items.item_or_none(self.mtm(), row)
        }

        #[unsafe(method(outlineView:isItemExpandable:))]
        fn expandable(&self, _view: &NSOutlineView, item: &AnyObject) -> bool {
            guarded(false, || !self.children_of(Some(item)).is_empty())
        }
    }

    unsafe impl NSControlTextEditingDelegate for OutlineSource {}

    unsafe impl NSOutlineViewDelegate for OutlineSource {
        #[unsafe(method_id(outlineView:viewForTableColumn:item:))]
        fn view_for(
            &self,
            _view: &NSOutlineView,
            _column: Option<&NSTableColumn>,
            item: &AnyObject,
        ) -> Option<Retained<NSView>> {
            guarded(None, || {
                let row = self.ivars().items.row_of(item)?;
                let title = self.ivars().nodes.borrow().get(row)?.title.clone();
                let label = NSTextField::labelWithString(&ns(&title), self.mtm());
                label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
                label.setToolTip(Some(&ns(&title)));
                Some(Retained::into_super(Retained::into_super(label)))
            })
        }

        #[unsafe(method(outlineViewSelectionDidChange:))]
        fn selection_changed(&self, notification: &NSNotification) {
            guarded((), || {
                if self.ivars().reloading.get() {
                    return;
                }
                let Some(view) = notification.object().and_then(|o| o.downcast::<NSOutlineView>().ok()) else {
                    return;
                };
                let start = view
                    .itemAtRow(view.selectedRow())
                    .and_then(|item| self.ivars().items.row_of(&item))
                    .and_then(|row| self.ivars().nodes.borrow().get(row).map(|n| n.start));
                let controller = self.ivars().controller.borrow().as_ref().and_then(Weak::load);
                if let (Some(start), Some(c)) = (start, controller) {
                    c.reveal_source(start);
                }
            });
        }
    }
);

impl OutlineSource {
    fn children_of(&self, item: Option<&AnyObject>) -> Vec<usize> {
        let ivars = self.ivars();
        match item {
            Some(item) => ivars
                .items
                .row_of(item)
                .and_then(|row| ivars.nodes.borrow().get(row).map(|n| n.children.clone()))
                .unwrap_or_default(),
            None => ivars.roots.borrow().clone(),
        }
    }
}

/// The sidebar view and the object feeding it.
pub struct Outline {
    pub view: Retained<NSOutlineView>,
    source: Retained<OutlineSource>,
}

impl Outline {
    pub fn new(mtm: MainThreadMarker, controller: &EditorController) -> (Outline, Retained<NSScrollView>) {
        let source: Retained<OutlineSource> =
            // SAFETY: NSObject's initializer, called once on the newly allocated object.
            unsafe { msg_send![super(OutlineSource::alloc(mtm).set_ivars(SourceIvars::default())), init] };
        *source.ivars().controller.borrow_mut() = Some(Weak::from(controller));
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(220.0, 600.0));
        let view = NSOutlineView::initWithFrame(NSOutlineView::alloc(mtm), frame);
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
        // SAFETY: the outline view holds its data source and delegate weakly; `Outline` keeps the source alive.
        unsafe {
            view.setDataSource(Some(ProtocolObject::from_ref(&*source)));
            view.setDelegate(Some(ProtocolObject::from_ref(&*source)));
        }
        let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), frame);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setDrawsBackground(false);
        scroll.setDocumentView(Some(&view));
        (Outline { view, source }, scroll)
    }

    /// Shows the headings; the tree is only rebuilt when headings were added, removed or renamed, so typing does
    /// not collapse what the user opened.
    pub fn update(&self, headings: &[(u8, String, Range<Byte>)]) {
        let ivars = self.source.ivars();
        let (nodes, roots) = tree(headings);
        let shape: Vec<(u8, String)> = headings.iter().map(|(l, t, _)| (*l, t.clone())).collect();
        let changed = *ivars.shown.borrow() != shape;
        let count = nodes.len();
        *ivars.nodes.borrow_mut() = nodes;
        *ivars.roots.borrow_mut() = roots;
        if changed {
            *ivars.shown.borrow_mut() = shape;
            ivars.items.replace(self.source.mtm(), count, || {
                crate::util::with_raised(&ivars.reloading, || {
                    self.view.reloadData();
                    // SAFETY: a missing item means every top-level item, which the method accepts.
                    unsafe { self.view.expandItem_expandChildren(None, true) };
                });
            });
        }
    }
}
