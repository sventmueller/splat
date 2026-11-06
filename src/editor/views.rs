use std::cell::RefCell;

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{AnyThread, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSDragOperation, NSDraggingInfo, NSLayoutManager, NSPasteboard, NSPasteboardType, NSPasteboardTypePNG,
    NSPasteboardTypeString, NSPasteboardTypeTIFF, NSPasteboardTypeURL, NSTextContainer, NSTextContentStorage,
    NSTextLayoutManager, NSTextStorage, NSTextView, NSView,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSDictionary, NSObject, NSObjectProtocol, NSPoint, NSRange, NSRect, NSSize, NSString,
    NSURL, NSUnionRect,
};

use super::EditorController;
use super::attributes::{Attribute, Attributes, attributed};
use super::export_options::{ExportOptions, TitleFacts, filled_in, footer_text};
use crate::util::{constant, guarded};

/// What printed pages show beside the text.
pub struct PageLook {
    pub(super) options: ExportOptions,
    pub(super) facts: TitleFacts,
    /// Where the title page ends in the view; nothing else stands on that page, and it carries neither header nor footer.
    pub(super) title_end: Option<f64>,
}

#[derive(Default)]
pub struct ViewIvars {
    controller: RefCell<Option<Weak<EditorController>>>,
    /// Owns the text system; NSTextView itself does not retain it.
    #[expect(dead_code, reason = "holding it keeps the owner alive")]
    owner: RefCell<Option<Retained<NSObject>>>,
    /// Only for the view that prints.
    pages: RefCell<Option<PageLook>>,
}

fn controller(ivars: &ViewIvars) -> Option<Retained<EditorController>> {
    ivars.controller.borrow().as_ref().and_then(Weak::load)
}

define_class!(
    #[unsafe(super(NSTextView))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDSourceTextView"]
    #[ivars = ViewIvars]
    pub struct SourceTextView;

    unsafe impl NSObjectProtocol for SourceTextView {}

    impl SourceTextView {
        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> bool {
            // SAFETY: The superclass's implementation, with this method's signature.
            let accepted: bool = unsafe { msg_send![super(self), becomeFirstResponder] };
            guarded((), || {
                if accepted && let Some(c) = controller(self.ivars()) {
                    c.source_became_focused();
                }
            });
            accepted
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            let handled = guarded(false, || {
                let pasteboard = sender.draggingPasteboard();
                let point = self.convertPoint_fromView(sender.draggingLocation(), None);
                let index = self.characterIndexForInsertionAtPoint(point);
                controller(self.ivars()).is_some_and(|c| c.handle_source_drop(&pasteboard, index))
            });
            // SAFETY: The superclass's implementation, with this method's signature.
            handled || unsafe { msg_send![super(self), performDragOperation: sender] }
        }

        #[unsafe(method(paste:))]
        fn paste(&self, sender: Option<&AnyObject>) {
            let pasteboard = NSPasteboard::generalPasteboard();
            if guarded(false, || controller(self.ivars()).is_some_and(|c| c.paste_image(&pasteboard))) {
                return;
            }
            // SAFETY: The superclass's implementation, with this method's signature.
            let _: () = unsafe { msg_send![super(self), paste: sender] };
        }

        /// Images are readable too, or AppKit would disable Paste while the pasteboard holds only a picture.
        #[unsafe(method_id(readablePasteboardTypes))]
        fn readable_types(&self) -> Retained<NSArray<NSPasteboardType>> {
            // SAFETY: The superclass's implementation, with this method's signature.
            let own: Retained<NSArray<NSPasteboardType>> = unsafe { msg_send![super(self), readablePasteboardTypes] };
            let mut types = own.to_vec();
            types.extend([constant!(NSPasteboardTypePNG).retain(), constant!(NSPasteboardTypeTIFF).retain()]);
            NSArray::from_retained_slice(&types)
        }

        /// During a live resize NSTextView leaves the container of a long text at its old width until the mouse is
        /// released; TextKit 2 only lays out what is in view, so the text can follow the window right away.
        #[unsafe(method(setFrameSize:))]
        fn set_frame_size(&self, size: NSSize) {
            // SAFETY: The superclass's implementation, with this method's signature.
            let _: () = unsafe { msg_send![super(self), setFrameSize: size] };
            guarded((), || {
                // SAFETY: `owner` keeps the container alive; reading it does not switch the view to TextKit 1, unlike its
                // layout manager.
                let Some(container) = (unsafe { self.textContainer() }) else {
                    return;
                };
                if self.inLiveResize()
                    && let Some(width) = followed_width(size.width, self.textContainerInset().width, container.size().width)
                {
                    container.setSize(NSSize::new(width, container.size().height));
                }
            });
        }
    }
);

define_class!(
    #[unsafe(super(NSTextView))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDPreviewTextView"]
    #[ivars = ViewIvars]
    pub struct PreviewTextView;

    unsafe impl NSObjectProtocol for PreviewTextView {}

    impl PreviewTextView {
        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> bool {
            // SAFETY: The superclass's implementation, with this method's signature.
            let accepted: bool = unsafe { msg_send![super(self), becomeFirstResponder] };
            guarded((), || {
                if accepted && let Some(c) = controller(self.ivars()) {
                    c.preview_became_focused();
                }
            });
            accepted
        }

        #[unsafe(method_id(readablePasteboardTypes))]
        fn readable_types(&self) -> Retained<NSArray<NSPasteboardType>> {
            NSArray::from_slice(&[
                constant!(NSPasteboardTypeString),
                constant!(NSPasteboardTypePNG),
                constant!(NSPasteboardTypeTIFF),
            ])
        }

        #[unsafe(method(paste:))]
        fn paste(&self, sender: Option<&AnyObject>) {
            let pasteboard = NSPasteboard::generalPasteboard();
            if guarded(false, || {
                controller(self.ivars()).is_some_and(|c| c.preview_takes_input() && c.paste_image(&pasteboard))
            }) {
                return;
            }
            // SAFETY: The superclass's implementation, with this method's signature.
            let _: () = unsafe { msg_send![super(self), paste: sender] };
        }

        #[unsafe(method_id(acceptableDragTypes))]
        fn acceptable_drag_types(&self) -> Retained<NSArray<NSPasteboardType>> {
            NSArray::from_slice(&[constant!(NSPasteboardTypeString), constant!(NSPasteboardTypeURL)])
        }

        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            if !guarded(false, || offers_text(&sender.draggingPasteboard())) {
                NSDragOperation::None
            } else {
                // SAFETY: The superclass's implementation, with this method's signature.
                unsafe { msg_send![super(self), draggingEntered: sender] }
            }
        }

        #[unsafe(method_id(pageHeader))]
        fn page_header(&self) -> Retained<NSAttributedString> {
            guarded(None, || self.page_line(true)).unwrap_or_else(NSAttributedString::new)
        }

        #[unsafe(method_id(pageFooter))]
        fn page_footer(&self) -> Retained<NSAttributedString> {
            guarded(None, || self.page_line(false)).unwrap_or_else(NSAttributedString::new)
        }

        // A title page ends where its text ends, however much of the paper is left.
        #[unsafe(method(adjustPageHeightNew:top:bottom:limit:))]
        fn adjust_page_height(&self, new: *mut f64, top: f64, proposed: f64, limit: f64) {
            // SAFETY: The superclass's implementation, with this method's signature.
            let _: () = unsafe { msg_send![super(self), adjustPageHeightNew: new, top: top, bottom: proposed, limit: limit] };
            guarded((), || {
                let pages = self.ivars().pages.borrow();
                let Some(end) = pages.as_ref().and_then(|look| look.title_end) else {
                    return;
                };
                if top < end && !new.is_null() {
                    // SAFETY: AppKit passes a valid pointer for the new height, checked for null above.
                    unsafe { *new = end.min(proposed) };
                }
            });
        }

        #[unsafe(method(changeFont:))]
        fn change_font(&self, _sender: Option<&AnyObject>) {}

        #[unsafe(method(changeAttributes:))]
        fn change_attributes(&self, _sender: Option<&AnyObject>) {}

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn appearance_changed(&self) {
            // SAFETY: The superclass's implementation, with this method's signature.
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            guarded((), || {
                if let Some(c) = controller(self.ivars()) {
                    c.appearance_changed();
                }
            });
        }
    }
);

define_class!(
    /// Redraws the view for any change instead of the changed range: to find that range's place, AppKit lays it out and
    /// adds up the whole layout, for every piece it lays out, which froze a preview of 1 MB of tables for minutes.
    #[unsafe(super(NSLayoutManager))]
    #[name = "MDPreviewLayoutManager"]
    struct PreviewLayoutManager;

    unsafe impl NSObjectProtocol for PreviewLayoutManager {}

    impl PreviewLayoutManager {
        #[unsafe(method(invalidateDisplayForCharacterRange:))]
        fn invalidate_characters(&self, _range: NSRange) {
            guarded((), || self.redraw_view());
        }

        #[unsafe(method(invalidateDisplayForGlyphRange:))]
        fn invalidate_glyphs(&self, _range: NSRange) {
            guarded((), || self.redraw_view());
        }
    }
);

impl PreviewLayoutManager {
    fn create() -> Retained<Self> {
        // SAFETY: NSLayoutManager's designated initializer.
        unsafe { msg_send![super(Self::alloc().set_ivars(())), init] }
    }

    fn redraw_view(&self) {
        // SAFETY: The result is retained on return, and only the main thread changes the views of a layout manager.
        let view = MainThreadMarker::new().and_then(|mtm| unsafe { self.firstTextView(mtm) });
        let Some(view) = view else {
            return;
        };
        let shown = NSUnionRect(view.visibleRect(), view.preparedContentRect());
        // SAFETY: NSView's implementation, with this method's signature; NSTextView's own adds up the whole layout first.
        let _: () = unsafe { msg_send![super(&*view, NSView::class()), setNeedsDisplayInRect: shown] };
    }
}

type TextSystem = (Retained<NSObject>, Retained<NSTextContainer>);

/// A TextKit 1 text system for the preview: tables (NSTextTable) only work with NSLayoutManager.
fn text_system() -> TextSystem {
    let storage = NSTextStorage::new();
    let layout = PreviewLayoutManager::create();
    storage.addLayoutManager(&layout);
    let container = NSTextContainer::initWithSize(NSTextContainer::alloc(), NSSize::new(1.0e7, 1.0e7));
    container.setWidthTracksTextView(true);
    layout.addTextContainer(&container);
    let owner = Retained::into_super(Retained::into_super(Retained::into_super(storage)));
    (owner, container)
}

/// A TextKit 2 text system for the source: it lays out only what is in view. Asking the source view for its
/// `layoutManager` would switch it back to TextKit 1 for good.
fn text_system_2() -> TextSystem {
    let content = NSTextContentStorage::new();
    let layout = NSTextLayoutManager::new();
    content.addTextLayoutManager(&layout);
    let container = NSTextContainer::initWithSize(NSTextContainer::alloc(), NSSize::new(1.0e7, 1.0e7));
    container.setWidthTracksTextView(true);
    layout.setTextContainer(Some(&container));
    (Retained::into_super(Retained::into_super(content)), container)
}

fn init_view<T>(mtm: MainThreadMarker, controller: &EditorController, (owner, container): TextSystem) -> Retained<T>
where
    T: DefinedClass<Ivars = ViewIvars> + MainThreadOnly + ClassType<Super = NSTextView>,
{
    let ivars = ViewIvars {
        controller: RefCell::new(Some(Weak::from_retained(&controller.retain()))),
        owner: RefCell::new(Some(owner)),
        pages: RefCell::new(None),
    };
    let this = T::alloc(mtm).set_ivars(ivars);
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(400.0, 400.0));
    // SAFETY: NSTextView's designated initializer with a container of its own.
    unsafe { msg_send![super(this), initWithFrame: frame, textContainer: &*container] }
}

impl PreviewTextView {
    pub fn set_pages(&self, look: PageLook) {
        *self.ivars().pages.borrow_mut() = Some(look);
    }

    /// The header or footer of the page being printed.
    fn page_line(&self, header: bool) -> Option<Retained<NSAttributedString>> {
        let pages = self.ivars().pages.borrow();
        let look = pages.as_ref()?;
        let operation = objc2_app_kit::NSPrintOperation::currentOperation(self.mtm())?;
        let page = crate::util::to_usize(operation.currentPage());
        let count = operation.pageRange().length;
        if page <= 1 && look.title_end.is_some() {
            return None;
        }
        let text = if header {
            filled_in(&look.options.header, &look.facts, page, count)
        } else {
            footer_text(&look.options, &look.facts, page, count)
        };
        let style = objc2_app_kit::NSMutableParagraphStyle::new();
        style.setAlignment(objc2_app_kit::NSTextAlignment::Center);
        let attributes = Attributes::new(&[
            Attribute::font(&objc2_app_kit::NSFont::systemFontOfSize(9.0)),
            Attribute::foreground(&objc2_app_kit::NSColor::colorWithWhite_alpha(0.35, 1.0)),
            Attribute::paragraph(&style),
        ]);
        Some(attributed(&text, Some(&attributes)))
    }
}

impl SourceTextView {
    pub fn create(mtm: MainThreadMarker, controller: &EditorController) -> Retained<Self> {
        init_view(mtm, controller, text_system_2())
    }
}

impl PreviewTextView {
    pub fn create(mtm: MainThreadMarker, controller: &EditorController) -> Retained<Self> {
        init_view(mtm, controller, text_system())
    }
}

/// The preview and the print view are made with a text system of TextKit 1, in `text_system`.
impl crate::util::TextKitOne for PreviewTextView {
    fn text_view(&self) -> &NSTextView {
        self
    }
}

/// The width a text container has to take for a view of `view_width`, unless it stands there already.
fn followed_width(view_width: f64, inset: f64, container_width: f64) -> Option<f64> {
    let width = view_width - 2.0 * inset;
    ((container_width - width).abs() >= 0.5).then_some(width)
}

pub fn offers_text(pasteboard: &NSPasteboard) -> bool {
    let text = NSArray::from_slice(&[constant!(NSPasteboardTypeString)]);
    pasteboard.availableTypeFromArray(&text).is_some()
}

/// File URLs and web URLs on a pasteboard, in drop order.
pub fn pasteboard_urls(pasteboard: &NSPasteboard) -> Vec<Retained<NSURL>> {
    let classes = NSArray::from_slice(&[NSURL::class()]);
    let options = NSDictionary::<NSString, AnyObject>::new();
    // SAFETY: The classes are NSURL, the options a dictionary of flags, as the pasteboard expects.
    let objects = unsafe { pasteboard.readObjectsForClasses_options(&classes, Some(&options)) };
    objects
        .map(|a| a.iter().filter_map(|o| o.downcast::<NSURL>().ok()).collect())
        .unwrap_or_default()
}

/// Off-screen text view at paper width, used to paginate the PDF and print output.
pub fn print_view(mtm: MainThreadMarker, width: f64) -> Retained<PreviewTextView> {
    let (owner, container) = text_system();
    let ivars = ViewIvars {
        controller: RefCell::new(None),
        owner: RefCell::new(Some(owner)),
        pages: RefCell::new(None),
    };
    let this = PreviewTextView::alloc(mtm).set_ivars(ivars);
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, 100.0));
    let view: Retained<PreviewTextView> =
        // SAFETY: NSTextView's designated initializer with a container of its own.
        unsafe { msg_send![super(this), initWithFrame: frame, textContainer: &*container] };
    view.setVerticallyResizable(true);
    view.setHorizontallyResizable(false);
    view.setTextContainerInset(NSSize::new(0.0, 0.0));
    view.setDrawsBackground(false);
    view
}

/// Where the line of a character ends, in a view with its whole text laid out.
pub fn line_bottom(view: &PreviewTextView, index: usize) -> f64 {
    let Some(lm) = crate::util::layout_manager(view) else {
        return 0.0;
    };
    let glyph = lm.glyphIndexForCharacterAtIndex(index);
    // SAFETY: A null range pointer is allowed when the effective range is not wanted.
    let line = unsafe { lm.lineFragmentRectForGlyphAtIndex_effectiveRange(glyph, std::ptr::null_mut()) };
    line.origin.y + line.size.height + view.textContainerOrigin().y
}

pub fn fit_height(view: &PreviewTextView) {
    let (Some(lm), Some(tc)) = (crate::util::layout_manager(view), crate::util::text_container(view)) else {
        return;
    };
    lm.ensureLayoutForTextContainer(&tc);
    let used = lm.usedRectForTextContainer(&tc);
    view.setFrameSize(NSSize::new(view.frame().size.width, used.size.height + 4.0));
}

#[cfg(test)]
mod tests {
    use objc2_foundation::NSData;

    use super::*;
    use crate::util::ns;

    #[test]
    fn a_container_follows_the_view_only_when_it_stands_elsewhere() {
        assert_eq!(followed_width(500.0, 18.0, 300.0), Some(464.0));
        assert_eq!(followed_width(500.0, 18.0, 464.0), None);
        assert_eq!(
            followed_width(500.0, 18.0, 464.4),
            None,
            "too little to lay anything out again"
        );
        assert_eq!(followed_width(500.0, 18.0, 464.5), Some(464.0));
        assert_eq!(followed_width(500.0, 18.0, 463.5), Some(464.0));
    }

    /// NSPasteboard shares state between its objects, and two threads at once make it throw now and then.
    static ONE_PASTEBOARD_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn a_drag_is_taken_for_its_text_and_not_for_a_picture_alone() {
        let _alone = ONE_PASTEBOARD_TEST.lock();
        let board = NSPasteboard::pasteboardWithUniqueName();
        board.clearContents();
        board.setData_forType(Some(&NSData::with_bytes(b"x")), constant!(NSPasteboardTypePNG));
        assert!(!offers_text(&board));
        board.clearContents();
        board.setString_forType(&ns("Text"), constant!(NSPasteboardTypeString));
        assert!(offers_text(&board));
    }

    #[test]
    fn a_drop_keeps_its_addresses_in_order_and_nothing_else() {
        let _alone = ONE_PASTEBOARD_TEST.lock();
        let board = NSPasteboard::pasteboardWithUniqueName();
        board.clearContents();
        let urls = [
            NSURL::fileURLWithPath(&ns("/tmp/a b.png")),
            NSURL::URLWithString(&ns("https://example.org/seite")).unwrap(),
        ];
        let objects: Vec<_> = urls.into_iter().map(ProtocolObject::from_retained).collect();
        let written: Vec<Retained<ProtocolObject<dyn objc2_app_kit::NSPasteboardWriting>>> = objects;
        board.writeObjects(&NSArray::from_retained_slice(&written));
        let read: Vec<String> = pasteboard_urls(&board)
            .iter()
            .filter_map(|u| u.absoluteString().map(|s| s.to_string()))
            .collect();
        assert_eq!(read, ["file:///tmp/a%20b.png", "https://example.org/seite"]);
        board.clearContents();
        board.setString_forType(&ns("kein Link"), constant!(NSPasteboardTypeString));
        assert!(pasteboard_urls(&board).is_empty());
    }
}
