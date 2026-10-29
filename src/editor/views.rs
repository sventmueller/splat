use std::cell::RefCell;

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{AnyThread, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSDragOperation, NSDraggingInfo, NSLayoutManager, NSPasteboard, NSPasteboardType, NSPasteboardTypePNG,
    NSPasteboardTypeString, NSPasteboardTypeTIFF, NSPasteboardTypeURL, NSTextContainer, NSTextContentStorage,
    NSTextLayoutManager, NSTextStorage, NSTextView,
};
use objc2_foundation::{NSArray, NSDictionary, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL};

use super::EditorController;
use crate::util::constant;

#[derive(Default)]
pub struct ViewIvars {
    controller: RefCell<Option<Weak<EditorController>>>,
    /// Owns the text system; NSTextView itself does not retain it.
    #[allow(dead_code)]
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
                c.source_became_focused();
            }
            accepted
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            let pasteboard = sender.draggingPasteboard();
            let point = self.convertPoint_fromView(sender.draggingLocation(), None);
            let index = self.characterIndexForInsertionAtPoint(point);
            let handled = controller(self.ivars()).is_some_and(|c| c.handle_source_drop(&pasteboard, index));
            // SAFETY: The superclass's implementation, with this method's signature.
            handled || unsafe { msg_send![super(self), performDragOperation: sender] }
        }

        #[unsafe(method(paste:))]
        fn paste(&self, sender: Option<&AnyObject>) {
            let pasteboard = NSPasteboard::generalPasteboard();
            if controller(self.ivars()).is_some_and(|c| c.paste_image(&pasteboard)) {
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
            // SAFETY: Reading the container does not switch the view to TextKit 1, unlike its layout manager.
            let Some(container) = (unsafe { self.textContainer() }) else {
                return;
            };
            let width = size.width - 2.0 * self.textContainerInset().width;
            if self.inLiveResize() && (container.size().width - width).abs() >= 0.5 {
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
                c.preview_became_focused();
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
            if controller(self.ivars()).is_some_and(|c| c.paste_image(&pasteboard)) {
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
            let pasteboard = sender.draggingPasteboard();
            let types = NSArray::from_slice(&[constant!(NSPasteboardTypeString)]);
            if pasteboard.availableTypeFromArray(&types).is_none() {
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
                c.appearance_changed();
            }
        }
    }
);

type TextSystem = (Retained<NSObject>, Retained<NSTextContainer>);

/// A TextKit 1 text system for the preview: tables (NSTextTable) only work with NSLayoutManager.
fn text_system() -> TextSystem {
    let storage = NSTextStorage::new();
    let layout = NSLayoutManager::new();
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
