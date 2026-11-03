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
    NSArray, NSDictionary, NSObject, NSObjectProtocol, NSPoint, NSRange, NSRect, NSSize, NSString, NSURL, NSUnionRect,
};

use super::EditorController;
use crate::util::constant;

#[derive(Default)]
pub struct ViewIvars {
    controller: RefCell<Option<Weak<EditorController>>>,
    /// Owns the text system; NSTextView itself does not retain it.
    #[expect(dead_code, reason = "holding it keeps the owner alive")]
    owner: RefCell<Option<Retained<NSObject>>>,
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
            if accepted && let Some(c) = controller(self.ivars()) {
                c.selection().source_became_focused();
            }
            accepted
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            let pasteboard = sender.draggingPasteboard();
            let point = self.convertPoint_fromView(sender.draggingLocation(), None);
            let index = self.characterIndexForInsertionAtPoint(point);
            let handled = controller(self.ivars()).is_some_and(|c| c.editing().handle_source_drop(&pasteboard, index));
            // SAFETY: The superclass's implementation, with this method's signature.
            handled || unsafe { msg_send![super(self), performDragOperation: sender] }
        }

        #[unsafe(method(paste:))]
        fn paste(&self, sender: Option<&AnyObject>) {
            let pasteboard = NSPasteboard::generalPasteboard();
            if controller(self.ivars()).is_some_and(|c| c.editing().paste_image(&pasteboard)) {
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
            if accepted && let Some(c) = controller(self.ivars()) {
                c.selection().preview_became_focused();
            }
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
            if controller(self.ivars()).is_some_and(|c| c.updater().preview_takes_input() && c.editing().paste_image(&pasteboard)) {
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
            if !offers_text(&sender.draggingPasteboard()) {
                NSDragOperation::None
            } else {
                // SAFETY: The superclass's implementation, with this method's signature.
                unsafe { msg_send![super(self), draggingEntered: sender] }
            }
        }

        #[unsafe(method(changeFont:))]
        fn change_font(&self, _sender: Option<&AnyObject>) {}

        #[unsafe(method(changeAttributes:))]
        fn change_attributes(&self, _sender: Option<&AnyObject>) {}

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn appearance_changed(&self) {
            // SAFETY: The superclass's implementation, with this method's signature.
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            if let Some(c) = controller(self.ivars()) {
                c.updater().appearance_changed();
            }
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
            self.redraw_view();
        }

        #[unsafe(method(invalidateDisplayForGlyphRange:))]
        fn invalidate_glyphs(&self, _range: NSRange) {
            self.redraw_view();
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
    };
    let this = T::alloc(mtm).set_ivars(ivars);
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(400.0, 400.0));
    // SAFETY: NSTextView's designated initializer with a container of its own.
    unsafe { msg_send![super(this), initWithFrame: frame, textContainer: &*container] }
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

pub fn fit_height(view: &NSTextView) {
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
