//! The outline sidebar: the document's headings as a tree, a click on one shows it in both panes.

use std::cell::{Cell, RefCell};
use std::ops::Range;

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSControlTextEditingDelegate, NSLineBreakMode, NSOutlineView, NSOutlineViewDataSource,
    NSOutlineViewDelegate, NSScrollView, NSTableColumn, NSTableViewStyle, NSTextField, NSView, NSViewController,
};
use objc2_foundation::{NSNotification, NSNumber, NSObjectProtocol, NSPoint, NSRect, NSSize};

use super::EditorController;
use crate::util::ns;

/// One heading of the tree.
#[derive(Debug, PartialEq)]
pub struct Node {
    pub title: String,
    pub start: usize,
    pub children: Vec<usize>,
}

/// Headings as `(level, title, source range)` in document order, nested under the closest heading of a higher
/// level above them. Returns the nodes and the indexes of the top-level ones.
pub fn tree(headings: &[(u8, String, Range<usize>)]) -> (Vec<Node>, Vec<usize>) {
    let mut nodes: Vec<Node> = Vec::with_capacity(headings.len());
    let mut roots = Vec::new();
    let mut open: Vec<(u8, usize)> = Vec::new();
    for (level, title, range) in headings {
        while open.last().is_some_and(|(l, _)| l >= level) {
            open.pop();
        }
        let i = nodes.len();
        nodes.push(Node {
            title: title.clone(),
            start: range.start,
            children: Vec::new(),
        });
        match open.last() {
            Some(&(_, parent)) => nodes[parent].children.push(i),
            None => roots.push(i),
        }
        open.push((*level, i));
    }
    (nodes, roots)
}

#[derive(Default)]
pub struct SourceIvars {
    nodes: RefCell<Vec<Node>>,
    roots: RefCell<Vec<usize>>,
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
            self.children_of(item).len() as isize
        }

        #[unsafe(method_id(outlineView:child:ofItem:))]
        fn child(&self, _view: &NSOutlineView, index: isize, item: Option<&AnyObject>) -> Retained<AnyObject> {
            let i = self.children_of(item).get(index as usize).copied().unwrap_or(0);
            crate::util::obj(NSNumber::new_usize(i))
        }

        #[unsafe(method(outlineView:isItemExpandable:))]
        fn expandable(&self, _view: &NSOutlineView, item: &AnyObject) -> bool {
            !self.children_of(Some(item)).is_empty()
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
            let title = index_of(item).and_then(|i| self.ivars().nodes.borrow().get(i).map(|n| n.title.clone()));
            title.map(|title| {
                let label = NSTextField::labelWithString(&ns(&title), self.mtm());
                label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
                label.setToolTip(Some(&ns(&title)));
                Retained::into_super(Retained::into_super(label))
            })
        }

        #[unsafe(method(outlineViewSelectionDidChange:))]
        fn selection_changed(&self, notification: &NSNotification) {
            if self.ivars().reloading.get() {
                return;
            }
            let Some(view) = notification.object().and_then(|o| o.downcast::<NSOutlineView>().ok()) else {
                return;
            };
            let start = view
                .itemAtRow(view.selectedRow())
                .and_then(|item| index_of(&item))
                .and_then(|i| self.ivars().nodes.borrow().get(i).map(|n| n.start));
            let controller = self.ivars().controller.borrow().as_ref().and_then(Weak::load);
            if let (Some(start), Some(c)) = (start, controller) {
                c.reveal_source(start);
            }
        }
    }
);

fn index_of(item: &AnyObject) -> Option<usize> {
    item.downcast_ref::<NSNumber>().map(|n| n.as_usize())
}

impl OutlineSource {
    fn children_of(&self, item: Option<&AnyObject>) -> Vec<usize> {
        match item.and_then(index_of) {
            Some(i) => self
                .ivars()
                .nodes
                .borrow()
                .get(i)
                .map(|n| n.children.clone())
                .unwrap_or_default(),
            None => self.ivars().roots.borrow().clone(),
        }
    }
}

/// The sidebar view and the object feeding it.
pub struct Outline {
    pub view: Retained<NSOutlineView>,
    source: Retained<OutlineSource>,
}

impl Outline {
    pub fn new(mtm: MainThreadMarker, controller: &EditorController) -> (Outline, Retained<NSViewController>) {
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
        scroll.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        let pane = NSViewController::new(mtm);
        pane.setView(&scroll);
        (Outline { view, source }, pane)
    }

    /// Shows the headings; the tree is only rebuilt when headings were added, removed or renamed, so typing does
    /// not collapse what the user opened.
    pub fn update(&self, headings: &[(u8, String, Range<usize>)]) {
        let ivars = self.source.ivars();
        let (nodes, roots) = tree(headings);
        let shape: Vec<(u8, String)> = headings.iter().map(|(l, t, _)| (*l, t.clone())).collect();
        let changed = *ivars.shown.borrow() != shape;
        *ivars.nodes.borrow_mut() = nodes;
        *ivars.roots.borrow_mut() = roots;
        if changed {
            *ivars.shown.borrow_mut() = shape;
            ivars.reloading.set(true);
            self.view.reloadData();
            // SAFETY: a missing item means every top-level item, which the method accepts.
            unsafe { self.view.expandItem_expandChildren(None, true) };
            ivars.reloading.set(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_nest_under_the_closest_higher_level() {
        let h = |level, title: &str, start| (level, title.to_string(), start..start + 1);
        let (nodes, roots) = tree(&[
            h(2, "a", 0),
            h(3, "a1", 5),
            h(4, "a1x", 9),
            h(3, "a2", 12),
            h(1, "b", 20),
            h(3, "b1", 30),
        ]);
        assert_eq!(roots, [0, 4]);
        assert_eq!(nodes[0].children, [1, 3]);
        assert_eq!(nodes[1].children, [2]);
        assert_eq!(nodes[4].children, [5]);
        assert_eq!(nodes[3].start, 12);
    }
}
