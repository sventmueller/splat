//! Building the editor window, and what concerns the window as a whole: tabs, the preview pane, sharing and
//! printing.

use std::cell::{Cell, OnceCell, RefCell};
use std::path::PathBuf;

use objc2::rc::{Retained, Weak};
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAutoresizingMaskOptions,
    NSBackingStoreType, NSDocumentController, NSPrintInfo, NSPrintOperation, NSPrintingPaginationMode, NSScrollView,
    NSSharingService, NSSplitViewController, NSSplitViewDividerStyle, NSSplitViewItem, NSTextView, NSToolbar,
    NSToolbarDisplayMode, NSView, NSViewBoundsDidChangeNotification, NSViewController, NSWindow, NSWindowOrderingMode,
    NSWindowStyleMask, NSWindowToolbarStyle,
};
use objc2_foundation::{
    NSArray, NSNotificationCenter, NSPoint, NSRect, NSSize, NSURL, NSUndoManagerDidRedoChangeNotification,
    NSUndoManagerDidUndoChangeNotification, NSUserDefaults,
};

/// User defaults key of the frame the last moved or resized document window had.
const FRAME_KEY: &str = "DocumentWindowFrame";

/// Opens the window where the last one was moved or sized to, set off from a window already standing there.
fn place_window(window: &NSWindow) {
    let saved = NSUserDefaults::standardUserDefaults()
        .stringForKey(&ns(FRAME_KEY))
        .filter(|s| s.length() > 0);
    let Some(saved) = saved else { return };
    window.setFrameFromString(&saved);
    let origin = window.frame().origin;
    let taken = objc2_app_kit::NSApplication::sharedApplication(window.mtm())
        .windows()
        .iter()
        .find(|w| !std::ptr::eq(&**w, window) && w.isVisible() && w.frame().origin == origin);
    if let Some(other) = taken {
        // With the zero point the other window stays put and only reports where the next one goes.
        let next = other.cascadeTopLeftFromPoint(NSPoint::new(0.0, 0.0));
        window.cascadeTopLeftFromPoint(next);
    }
}

/// Height of the bar above the preview that offers to load images from the web.
const BANNER_HEIGHT: f64 = 48.0;
/// User defaults key listing the documents whose images from the web load without asking.
pub(super) const ALLOWED_KEY: &str = "RemoteImagesAllowed";
const MAX_ALLOWED_DOCUMENTS: usize = 500;

pub(super) fn allowed_documents() -> Vec<String> {
    NSUserDefaults::standardUserDefaults()
        .stringArrayForKey(&ns(ALLOWED_KEY))
        .map(|a| a.iter().map(|s| s.to_string()).collect())
        .unwrap_or_default()
}

use super::views::{self, PreviewTextView, SourceTextView};
use super::{EditorController, Ivars, OPEN_EDITORS, State};
use crate::document::MdDocument;
use crate::i18n::t;
use crate::util::constant;
use crate::util::{layout_manager, ns, storage};

impl EditorController {
    pub fn new(mtm: MainThreadMarker, document: &MdDocument, text: &str) -> Retained<Self> {
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1180.0, 760.0));
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
        window.setMinSize(NSSize::new(640.0, 420.0));
        window.setTabbingIdentifier(&ns("MDDocumentWindow"));
        // SAFETY: Switching it off keeps AppKit from releasing the window behind Rust's back.
        unsafe { window.setReleasedWhenClosed(false) };
        window.center();

        let ivars = Ivars {
            document: Weak::from(document),
            source: OnceCell::new(),
            preview: OnceCell::new(),
            preview_item: OnceCell::new(),
            outline: OnceCell::new(),
            outline_item: OnceCell::new(),
            state: RefCell::new(State {
                zoom: 1.0,
                ..Default::default()
            }),
            copy: Default::default(),
            marks: Default::default(),
            atoms: Default::default(),
            images: Default::default(),
            remote_allowed: Cell::new(false),
            remote_banner: OnceCell::new(),
            busy: Cell::new(false),
            scrolling: Cell::new(false),
            scroll_after_undo: Cell::new(false),
            preview_edit: RefCell::new(None),
            share_path: RefCell::new(None),
            active_cache: RefCell::new(None),
        };
        let this = Self::alloc(mtm).set_ivars(ivars);
        // SAFETY: NSWindowController's initializer, called once on the newly allocated object.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithWindow: Some(&*window)] };
        this.build(&window, text);
        OPEN_EDITORS.with(|e| {
            if let Ok(mut e) = e.try_borrow_mut() {
                e.retain(|w| w.load().is_some());
                e.push(Weak::from(&*this));
            }
        });
        this
    }

    fn build(&self, window: &NSWindow, text: &str) {
        let mtm = self.mtm();
        let source = SourceTextView::create(mtm, self);
        let preview = PreviewTextView::create(mtm, self);
        self.configure_text_view(&source, false);
        self.configure_text_view(&preview, true);
        source.setAccessibilityLabel(Some(&ns(t("Markdown-Quelltext"))));
        preview.setAccessibilityLabel(Some(&ns(t("Vorschau"))));

        let split = NSSplitViewController::new(mtm);
        let left = self.pane(&source);
        let right = self.preview_pane(&preview);
        let left_item = NSSplitViewItem::splitViewItemWithViewController(&left);
        let right_item = NSSplitViewItem::splitViewItemWithViewController(&right);
        left_item.setMinimumThickness(260.0);
        right_item.setMinimumThickness(260.0);
        right_item.setCanCollapse(true);
        let (outline, sidebar) = super::outline::Outline::new(mtm, self);
        let outline_item = NSSplitViewItem::sidebarWithViewController(&sidebar);
        outline_item.setMinimumThickness(160.0);
        outline_item.setMaximumThickness(400.0);
        outline_item.setCollapsed(true);
        split.addSplitViewItem(&outline_item);
        let _ = self.ivars().outline.set(outline);
        let _ = self.ivars().outline_item.set(outline_item);
        split.addSplitViewItem(&left_item);
        split.addSplitViewItem(&right_item);
        split.splitView().setDividerStyle(NSSplitViewDividerStyle::Thin);
        window.setContentViewController(Some(&split));
        window.setContentSize(NSSize::new(1180.0, 760.0));
        window.center();

        let toolbar = NSToolbar::initWithIdentifier(NSToolbar::alloc(mtm), &ns("MDEditorToolbar"));
        toolbar.setDelegate(Some(ProtocolObject::from_ref(self)));
        toolbar.setAllowsUserCustomization(true);
        toolbar.setAutosavesConfiguration(true);
        toolbar.setDisplayMode(NSToolbarDisplayMode::IconOnly);
        window.setToolbar(Some(&toolbar));
        window.setToolbarStyle(NSWindowToolbarStyle::Unified);
        place_window(window);
        window.setDelegate(Some(ProtocolObject::from_ref(self)));

        let center = NSNotificationCenter::defaultCenter();
        for view in [&*source as &NSTextView, &*preview as &NSTextView] {
            if let Some(scroll) = view.enclosingScrollView() {
                let clip = scroll.contentView();
                clip.setPostsBoundsChangedNotifications(true);
                // SAFETY: Notification centers hold observers weakly, so a closed window's controller is never called.
                unsafe {
                    center.addObserver_selector_name_object(
                        self,
                        sel!(clipBoundsChanged:),
                        Some(NSViewBoundsDidChangeNotification),
                        Some(&clip),
                    );
                }
            }
        }
        if let Some(undo) = self.undo_manager() {
            for name in [
                constant!(NSUndoManagerDidUndoChangeNotification),
                constant!(NSUndoManagerDidRedoChangeNotification),
            ] {
                // SAFETY: Notification centers hold observers weakly, so a closed window's controller is never called.
                unsafe { center.addObserver_selector_name_object(self, sel!(undoDidChange:), Some(name), Some(&undo)) };
            }
        }

        storage(&source).setDelegate(Some(ProtocolObject::from_ref(self)));
        if let Some(lm) = layout_manager(&preview) {
            lm.setAllowsNonContiguousLayout(true);
        }
        let _ = self.ivars().source.set(source.clone());
        let _ = self.ivars().preview.set(preview);
        let _ = self.ivars().preview_item.set(right_item);
        self.set_zoom(1.0);
        self.replace_all_text(text);
        window.setInitialFirstResponder(Some(&source));
    }

    /// The preview with the bar for images from the web above it, hidden until a document has some.
    fn preview_pane(&self, preview: &NSTextView) -> Retained<NSViewController> {
        let mtm = self.mtm();
        let controller = self.pane(preview);
        let scroll = controller.view();
        let container = NSView::initWithFrame(NSView::alloc(mtm), scroll.frame());
        scroll.setFrame(container.bounds());
        scroll.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        container.addSubview(&scroll);
        let banner = self.remote_banner(container.bounds().size);
        container.addSubview(&banner);
        controller.setView(&container);
        let _ = self.ivars().remote_banner.set(banner);
        let allowed = self.document_path().is_some_and(|p| allowed_documents().contains(&p));
        self.ivars().remote_allowed.set(allowed);
        controller
    }

    fn remote_banner(&self, size: NSSize) -> Retained<NSView> {
        let mtm = self.mtm();
        let frame = NSRect::new(
            NSPoint::new(0.0, size.height - BANNER_HEIGHT),
            NSSize::new(size.width, BANNER_HEIGHT),
        );
        let banner = objc2_app_kit::NSBox::initWithFrame(objc2_app_kit::NSBox::alloc(mtm), frame);
        banner.setBoxType(objc2_app_kit::NSBoxType::Custom);
        banner.setTitlePosition(objc2_app_kit::NSTitlePosition::NoTitle);
        banner.setBorderWidth(0.0);
        banner.setContentViewMargins(NSSize::new(0.0, 0.0));
        banner.setFillColor(&objc2_app_kit::NSColor::controlBackgroundColor());
        banner.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin,
        );
        banner.setHidden(true);
        let content = banner.contentView().expect("box content view");
        // SAFETY: the target is this controller, which owns the window the button sits in.
        let button = unsafe {
            objc2_app_kit::NSButton::buttonWithTitle_target_action(
                &ns(t("Bilder laden")),
                Some(self),
                Some(sel!(mdLoadRemoteImages:)),
                mtm,
            )
        };
        button.setControlSize(objc2_app_kit::NSControlSize::Small);
        button.setFont(Some(&objc2_app_kit::NSFont::systemFontOfSize(
            objc2_app_kit::NSFont::smallSystemFontSize(),
        )));
        button.sizeToFit();
        let b = button.frame().size;
        button.setFrameOrigin(NSPoint::new(
            size.width - b.width - 12.0,
            (BANNER_HEIGHT - b.height) / 2.0,
        ));
        button.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
        content.addSubview(&button);
        let label = objc2_app_kit::NSTextField::wrappingLabelWithString(
            &ns(t(
                "Bilder aus dem Internet sind blockiert: Beim Laden erfahren deren Server, dass und wann du dieses Dokument öffnest.",
            )),
            mtm,
        );
        label.setFont(Some(&objc2_app_kit::NSFont::systemFontOfSize(
            objc2_app_kit::NSFont::smallSystemFontSize(),
        )));
        label.setTextColor(Some(&objc2_app_kit::NSColor::secondaryLabelColor()));
        label.setMaximumNumberOfLines(2);
        label.setFrame(NSRect::new(
            NSPoint::new(12.0, 4.0),
            NSSize::new((size.width - b.width - 36.0).max(40.0), BANNER_HEIGHT - 8.0),
        ));
        label.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        content.addSubview(&label);
        let line = objc2_app_kit::NSBox::initWithFrame(
            objc2_app_kit::NSBox::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(size.width, 1.0)),
        );
        line.setBoxType(objc2_app_kit::NSBoxType::Separator);
        line.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        content.addSubview(&line);
        Retained::into_super(banner)
    }

    /// Shows or hides the bar for images from the web and gives the preview the room that is left.
    pub(super) fn show_remote_banner(&self, show: bool) {
        let Some(banner) = self.ivars().remote_banner.get() else {
            return;
        };
        if banner.isHidden() != show {
            return;
        }
        banner.setHidden(!show);
        // SAFETY: the banner sits in the preview pane for the window's lifetime, so its superview is alive.
        let container = unsafe { banner.superview() };
        let (Some(container), Some(scroll)) = (container, self.preview().enclosingScrollView()) else {
            return;
        };
        let bounds = container.bounds();
        let height = bounds.size.height - if show { BANNER_HEIGHT } else { 0.0 };
        scroll.setFrame(NSRect::new(
            bounds.origin,
            NSSize::new(bounds.size.width, height.max(0.0)),
        ));
    }

    /// The user allowed images from the web for this document; it is remembered for the file.
    pub(super) fn allow_remote_images(&self) {
        self.ivars().remote_allowed.set(true);
        self.show_remote_banner(false);
        if let Some(path) = self.document_path() {
            let mut paths = allowed_documents();
            paths.retain(|p| *p != path);
            paths.push(path);
            let keep = paths.len().saturating_sub(MAX_ALLOWED_DOCUMENTS);
            let list = NSArray::from_retained_slice(&paths[keep..].iter().map(|p| ns(p)).collect::<Vec<_>>());
            // SAFETY: an array of strings is a property list value, as user defaults require.
            unsafe { NSUserDefaults::standardUserDefaults().setObject_forKey(Some(&list), &ns(ALLOWED_KEY)) };
        }
        self.invalidate(super::Invalidate::Arrivals);
    }

    /// Remembers where the window stands and how large it is, for the next window and the next start.
    pub(super) fn remember_frame(&self) {
        let Some(window) = self.window() else { return };
        if window.styleMask().contains(NSWindowStyleMask::FullScreen) || !window.isVisible() {
            return;
        }
        // SAFETY: A string is a property list value, as user defaults require.
        unsafe {
            NSUserDefaults::standardUserDefaults()
                .setObject_forKey(Some(&window.stringWithSavedFrame()), &ns(FRAME_KEY))
        };
    }

    fn document_path(&self) -> Option<String> {
        Some(self.document()?.fileURL()?.path()?.to_string())
    }

    fn pane(&self, view: &NSTextView) -> Retained<NSViewController> {
        let mtm = self.mtm();
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

    fn configure_text_view(&self, view: &NSTextView, preview: bool) {
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
        view.setDelegate(Some(ProtocolObject::from_ref(self)));
    }

    /// Opens a new untitled document as a tab of this window, like the plus button of the tab bar.
    pub(super) fn new_tab(&self) {
        let controller = NSDocumentController::sharedDocumentController(self.mtm());
        let Ok(doc) = controller.openUntitledDocumentAndDisplay_error(false) else {
            return;
        };
        doc.makeWindowControllers();
        let tab = doc.windowControllers().firstObject().and_then(|c| c.window());
        if let (Some(window), Some(tab)) = (self.window(), tab) {
            window.addTabbedWindow_ordered(&tab, NSWindowOrderingMode::Above);
        }
        doc.showWindows();
    }

    pub(super) fn outline_visible(&self) -> bool {
        self.ivars().outline_item.get().is_some_and(|i| !i.isCollapsed())
    }

    pub(super) fn toggle_outline(&self) {
        let Some(item) = self.ivars().outline_item.get() else {
            return;
        };
        let collapsed = item.isCollapsed();
        objc2_app_kit::NSAnimatablePropertyContainer::animator(&**item).setCollapsed(!collapsed);
        if collapsed {
            self.show_headings();
        }
        self.invalidateRestorableState();
    }

    /// Shows the current headings in the outline, if it is open; a closed one catches up when it opens.
    pub(super) fn refresh_outline(&self) {
        if self.outline_visible() {
            self.show_headings();
        }
    }

    fn show_headings(&self) {
        let Some(outline) = self.ivars().outline.get() else {
            return;
        };
        let headings: Vec<_> = crate::markdown::commands::heading_anchors(&self.ivars().state.borrow().doc)
            .into_iter()
            .map(|(level, title, _, range)| (level, title, range))
            .collect();
        outline.update(&headings);
    }

    pub(super) fn toggle_preview(&self) {
        let Some(item) = self.ivars().preview_item.get() else {
            return;
        };
        let collapsed = item.isCollapsed();
        objc2_app_kit::NSAnimatablePropertyContainer::animator(&**item).setCollapsed(!collapsed);
        if collapsed {
            self.update();
        } else if self.preview_focused() {
            self.window().map(|w| w.makeFirstResponder(Some(self.source())));
        }
        self.invalidateRestorableState();
    }

    /// The exports as entries of the share menu: PDF through the document's own export, HTML and Word like their
    /// menu commands.
    pub(super) fn export_sharing_services(&self) -> Vec<Retained<NSSharingService>> {
        let Some(image) = objc2_app_kit::NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &ns("arrow.down.document"),
            None,
        ) else {
            return Vec::new();
        };
        let service = |title: &str, handler: block2::RcBlock<dyn Fn()>| {
            NSSharingService::initWithTitle_image_alternateImage_handler(
                <NSSharingService as objc2::AnyThread>::alloc(),
                &ns(title),
                &image,
                None,
                &handler,
            )
        };
        let document = self.ivars().document.clone();
        let mut services = vec![service(
            t("Als PDF exportieren …"),
            block2::RcBlock::new(move || {
                if let Some(doc) = document.load() {
                    // SAFETY: The export action accepts a missing sender.
                    unsafe { doc.saveDocumentToPDF(None) };
                }
            }),
        )];
        for action in [super::Action::ExportHtml, super::Action::ExportWord] {
            let me = Weak::from(self);
            services.push(service(
                t(action.spec().title),
                block2::RcBlock::new(move || {
                    if let Some(c) = me.load() {
                        c.perform(action, None);
                    }
                }),
            ));
        }
        services
    }

    /// Temporary Markdown file the share menu hands out, so unsaved changes go along; `refresh` writes the current
    /// text into it. Until then the file stays empty, since AppKit offers no service for a file that does not exist.
    pub(super) fn share_file(&self, refresh: bool) -> Option<Retained<NSURL>> {
        self.ivars().source.get()?;
        let doc = self.document()?;
        let name = match doc.fileURL().and_then(|u| u.lastPathComponent()) {
            Some(name) => name.to_string(),
            None => format!("{}.md", doc.displayName()),
        }
        .replace(['/', ':'], "-");
        let previous = self.ivars().share_path.borrow().clone();
        let path = match previous.filter(|p| p.file_name().is_some_and(|n| *n == *name)) {
            Some(path) => path,
            None => {
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()?
                    .as_nanos();
                let dir = std::env::temp_dir().join("Splat-Teilen").join(stamp.to_string());
                std::fs::create_dir_all(&dir).ok()?;
                dir.join(name)
            }
        };
        if refresh {
            std::fs::write(&path, &self.snapshot().text).ok()?;
        } else if !path.exists() {
            std::fs::File::create(&path).ok()?;
        }
        let url = NSURL::fileURLWithPath(&ns(path.to_str()?));
        *self.ivars().share_path.borrow_mut() = Some::<PathBuf>(path);
        Some(url)
    }

    pub fn print_operation(&self, info: &NSPrintInfo) -> Retained<NSPrintOperation> {
        let mtm = self.mtm();
        let margin = 56.0;
        info.setLeftMargin(margin);
        info.setRightMargin(margin);
        info.setTopMargin(margin);
        info.setBottomMargin(margin);
        info.setHorizontalPagination(NSPrintingPaginationMode::Clip);
        info.setVerticalPagination(NSPrintingPaginationMode::Automatic);
        info.setVerticallyCentered(false);
        info.setHorizontallyCentered(false);
        let width = info.paperSize().width - 2.0 * margin;
        let view = views::print_view(mtm, width);
        if let Some(aqua) = NSAppearance::appearanceNamed(constant!(NSAppearanceNameAqua)) {
            view.setAppearance(Some(&aqua));
        }
        let out = self.printable(width);
        storage(&view).setAttributedString(&out);
        views::fit_height(&view);
        NSPrintOperation::printOperationWithView_printInfo(&view, info)
    }
}
