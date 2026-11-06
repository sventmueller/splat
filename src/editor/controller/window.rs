//! Building the editor window.

use objc2::rc::{Retained, Weak};
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadMarker, MainThreadOnly, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAutoresizingMaskOptions, NSBackingStoreType, NSDocumentController, NSScrollView,
    NSSplitViewController, NSSplitViewDividerStyle, NSSplitViewItem, NSTextView, NSToolbarDisplayMode, NSView,
    NSViewBoundsDidChangeNotification, NSViewController, NSWindow, NSWindowOrderingMode, NSWindowStyleMask,
    NSWindowToolbarStyle,
};
use objc2_foundation::{
    NSNotificationCenter, NSPoint, NSRect, NSSize, NSUndoManagerDidRedoChangeNotification,
    NSUndoManagerDidUndoChangeNotification, NSUserDefaults,
};

use super::{EditorController, Ivars};
use crate::document::MdDocument;
use crate::editor::OPEN_EDITORS;
use crate::editor::model::Model;
use crate::editor::panes::FRAME_KEY;
use crate::editor::toolbar::EditorToolbar;
use crate::editor::views::{PreviewTextView, SourceTextView};
use crate::i18n::t;
use crate::util::{constant, layout_manager, ns, storage};

/// The narrowest a window gets, with room for both panes side by side.
const MIN_WINDOW_SIZE: NSSize = NSSize::new(640.0, 420.0);
const MIN_PANE_WIDTH: f64 = 260.0;
const WINDOW_SIZE: NSSize = NSSize::new(1180.0, 760.0);
const _: () = assert!(MIN_WINDOW_SIZE.width >= 2.0 * MIN_PANE_WIDTH && WINDOW_SIZE.width >= MIN_WINDOW_SIZE.width);

/// The first of `others` a new window at `origin` would hide. Windows that do not show are no reason to move.
fn standing_at<W>(
    others: impl IntoIterator<Item = W>,
    origin: NSPoint,
    place: impl Fn(&W) -> (bool, NSPoint),
) -> Option<W> {
    others.into_iter().find(|w| place(w) == (true, origin))
}

/// Opens the window where the last one was moved or sized to, set off from a window already standing there.
fn place_window(window: &NSWindow) {
    let saved = NSUserDefaults::standardUserDefaults()
        .stringForKey(&ns(FRAME_KEY))
        .filter(|s| s.length() > 0);
    let Some(saved) = saved else { return };
    window.setFrameFromString(&saved);
    let origin = window.frame().origin;
    let windows = objc2_app_kit::NSApplication::sharedApplication(window.mtm()).windows();
    let others = windows.iter().filter(|w| !std::ptr::eq(&**w, window));
    let taken = standing_at(others, origin, |w| (w.isVisible(), w.frame().origin));
    if let Some(other) = taken {
        // With the zero point the other window stays put and only reports where the next one goes.
        let next = other.cascadeTopLeftFromPoint(NSPoint::new(0.0, 0.0));
        window.cascadeTopLeftFromPoint(next);
    }
}

impl EditorController {
    pub fn new(mtm: MainThreadMarker, document: &MdDocument, text: &str) -> Retained<Self> {
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), WINDOW_SIZE);
        // SAFETY: releasedWhenClosed is switched off right below, so Rust's reference stays the owner.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setMinSize(MIN_WINDOW_SIZE);
        window.setTabbingIdentifier(&ns("MDDocumentWindow"));
        // SAFETY: Switching it off keeps AppKit from releasing the window behind Rust's back.
        unsafe { window.setReleasedWhenClosed(false) };
        window.center();

        let ivars = Ivars {
            document: Weak::from(document),
            model: Model::new(),
            panes: Default::default(),
            updates: Default::default(),
            scroll: Default::default(),
            marks: Default::default(),
            atoms: Default::default(),
            edits: Default::default(),
            pictures: Default::default(),
            sharing: Default::default(),
            counter: Default::default(),
            changes: Default::default(),
            notes: Default::default(),
            closed: Default::default(),
        };
        let this = Self::alloc(mtm).set_ivars(ivars);
        // SAFETY: NSWindowController's initializer, called once on the newly allocated object.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithWindow: Some(&*window)] };
        build(&this, &window, text);
        OPEN_EDITORS.with(|e| {
            let mut e = e.borrow_mut();
            e.retain(|w| w.load().is_some());
            e.push(Weak::from(&*this));
        });
        this
    }
}

fn build(controller: &EditorController, window: &NSWindow, text: &str) {
    let mtm = controller.mtm();
    let source = SourceTextView::create(mtm, controller);
    let preview = PreviewTextView::create(mtm, controller);
    configure_text_view(controller, &source, false);
    configure_text_view(controller, &preview, true);
    source.setAccessibilityLabel(Some(&ns(t("Markdown-Quelltext"))));
    preview.setAccessibilityLabel(Some(&ns(t("Vorschau"))));

    let split = NSSplitViewController::new(mtm);
    let left = contained_pane(mtm, &source);
    let right = preview_pane(controller, &preview);
    let left_item = NSSplitViewItem::splitViewItemWithViewController(&left);
    let right_item = NSSplitViewItem::splitViewItemWithViewController(&right);
    left_item.setMinimumThickness(MIN_PANE_WIDTH);
    right_item.setMinimumThickness(MIN_PANE_WIDTH);
    right_item.setCanCollapse(true);
    let (sidebar, sidebar_pane) = crate::editor::sidebar::Sidebar::new(mtm, controller);
    let outline_item = NSSplitViewItem::sidebarWithViewController(&sidebar_pane);
    outline_item.setMinimumThickness(160.0);
    outline_item.setMaximumThickness(400.0);
    outline_item.setCollapsed(true);
    split.addSplitViewItem(&outline_item);
    controller.panes().install_sidebar(sidebar, outline_item);
    split.addSplitViewItem(&left_item);
    split.addSplitViewItem(&right_item);
    split.splitView().setDividerStyle(NSSplitViewDividerStyle::Thin);
    window.setContentViewController(Some(&split));
    window.setContentSize(WINDOW_SIZE);
    window.center();

    let toolbar = EditorToolbar::new(mtm, "MDEditorToolbar");
    toolbar.setDelegate(Some(ProtocolObject::from_ref(controller)));
    toolbar.setAllowsUserCustomization(true);
    toolbar.setAutosavesConfiguration(true);
    toolbar.setDisplayMode(NSToolbarDisplayMode::IconOnly);
    window.setToolbar(Some(&toolbar));
    window.setToolbarStyle(NSWindowToolbarStyle::Unified);
    window.addTitlebarAccessoryViewController(&toolbar.margin_accessory());
    place_window(window);
    window.setDelegate(Some(ProtocolObject::from_ref(controller)));

    let center = NSNotificationCenter::defaultCenter();
    // SAFETY: Notification centers hold observers weakly, so a closed window's controller is never called.
    unsafe {
        center.addObserver_selector_name_object(
            controller,
            sel!(panesResized:),
            Some(objc2_app_kit::NSSplitViewDidResizeSubviewsNotification),
            Some(&split.splitView()),
        );
    }
    for view in [&*source as &NSTextView, &*preview as &NSTextView] {
        if let Some(scroll) = view.enclosingScrollView() {
            let clip = scroll.contentView();
            clip.setPostsBoundsChangedNotifications(true);
            // SAFETY: Notification centers hold observers weakly, so a closed window's controller is never called.
            unsafe {
                center.addObserver_selector_name_object(
                    controller,
                    sel!(clipBoundsChanged:),
                    Some(NSViewBoundsDidChangeNotification),
                    Some(&clip),
                );
            }
        }
    }
    if let Some(undo) = controller.undo_manager() {
        for name in [
            constant!(NSUndoManagerDidUndoChangeNotification),
            constant!(NSUndoManagerDidRedoChangeNotification),
        ] {
            // SAFETY: Notification centers hold observers weakly, so a closed window's controller is never called.
            unsafe {
                center.addObserver_selector_name_object(controller, sel!(undoDidChange:), Some(name), Some(&undo))
            };
        }
    }

    storage(&source).setDelegate(Some(ProtocolObject::from_ref(controller)));
    if let Some(lm) = layout_manager(&preview) {
        lm.setAllowsNonContiguousLayout(true);
    }
    if let (Some(left), Some(right)) = (source.enclosingScrollView(), preview.enclosingScrollView()) {
        controller.change_marks().install(&left, &right);
    }
    controller
        .panes()
        .install_views(source.clone(), preview, [left_item, right_item]);
    controller.word_counter().install();
    controller.updater().set_zoom(1.0);
    controller.updater().replace_all_text(text);
    window.setInitialFirstResponder(Some(&source));
}

/// The preview with the bar for images from the web above it, hidden until a document has some.
fn preview_pane(controller: &EditorController, preview: &NSTextView) -> Retained<NSViewController> {
    let pane = contained_pane(controller.mtm(), preview);
    let container = pane.view();
    let pictures = controller.pictures();
    let banner = pictures.build_banner(container.bounds().size);
    container.addSubview(&banner);
    pictures.install_banner(banner);
    pane
}

/// A scroll view in a view of its own, for what floats above it: a scroll view lays out only what it knows of.
fn contained_pane(mtm: MainThreadMarker, view: &NSTextView) -> Retained<NSViewController> {
    let pane = pane(mtm, view);
    let scroll = pane.view();
    let container = NSView::initWithFrame(NSView::alloc(mtm), scroll.frame());
    scroll.setFrame(container.bounds());
    scroll.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    container.addSubview(&scroll);
    pane.setView(&container);
    pane
}

fn pane(mtm: MainThreadMarker, view: &NSTextView) -> Retained<NSViewController> {
    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(500.0, 600.0)),
    );
    scroll.setHasVerticalScroller(true);
    scroll.setHasHorizontalScroller(false);
    scroll.setAutohidesScrollers(true);
    scroll.setDocumentView(Some(view));
    let controller = NSViewController::new(mtm);
    controller.setView(&scroll);
    controller
}

fn configure_text_view(controller: &EditorController, view: &NSTextView, preview: bool) {
    view.setMinSize(NSSize::new(0.0, 0.0));
    view.setMaxSize(NSSize::new(1.0e7, 1.0e7));
    view.setVerticallyResizable(true);
    view.setHorizontallyResizable(false);
    view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
    view.setTextContainerInset(NSSize::new(18.0, 16.0));
    view.setRichText(preview);
    view.setAllowsUndo(!preview);
    view.setUsesFindBar(true);
    view.setIncrementalSearchingEnabled(true);
    view.setUsesFontPanel(false);
    view.setUsesRuler(false);
    view.setImportsGraphics(false);
    view.setUsesInspectorBar(false);
    view.setAutomaticQuoteSubstitutionEnabled(false);
    view.setAutomaticDashSubstitutionEnabled(false);
    view.setAutomaticTextReplacementEnabled(false);
    view.setAutomaticLinkDetectionEnabled(false);
    view.setSmartInsertDeleteEnabled(false);
    view.setContinuousSpellCheckingEnabled(true);
    view.setAllowsDocumentBackgroundColorChange(false);
    view.setDisplaysLinkToolTips(true);
    if preview {
        // Inline predictions and math results are inserted into the text storage before the user accepts
        // them, which the preview would otherwise translate into Markdown.
        view.setInlinePredictionType(objc2_app_kit::NSTextInputTraitType::No);
        view.setMathExpressionCompletionType(objc2_app_kit::NSTextInputTraitType::No);
        view.setWritingToolsBehavior(objc2_app_kit::NSWritingToolsBehavior::Limited);
    }
    view.setDelegate(Some(ProtocolObject::from_ref(controller)));
}

/// Opens a new untitled document as a tab of this window, like the plus button of the tab bar.
pub(in crate::editor) fn new_tab(controller: &EditorController) {
    let documents = NSDocumentController::sharedDocumentController(controller.mtm());
    let Ok(doc) = documents.openUntitledDocumentAndDisplay_error(false) else {
        return;
    };
    doc.makeWindowControllers();
    let tab = doc.windowControllers().firstObject().and_then(|c| c.window());
    if let (Some(window), Some(tab)) = (controller.window(), tab) {
        window.addTabbedWindow_ordered(&tab, NSWindowOrderingMode::Above);
    }
    doc.showWindows();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_window_moves_away_only_from_a_visible_one_at_its_place() {
        let here = NSPoint::new(10.0, 20.0);
        let elsewhere = NSPoint::new(30.0, 20.0);
        let place = |w: &(bool, NSPoint, &'static str)| (w.0, w.1);
        let found = |others: Vec<(bool, NSPoint, &'static str)>| standing_at(others, here, place).map(|w| w.2);
        assert_eq!(found(vec![]), None);
        assert_eq!(found(vec![(true, elsewhere, "a")]), None);
        assert_eq!(found(vec![(false, here, "hidden")]), None);
        assert_eq!(
            found(vec![
                (false, here, "hidden"),
                (true, elsewhere, "a"),
                (true, here, "b"),
                (true, here, "c")
            ]),
            Some("b")
        );
    }
}
