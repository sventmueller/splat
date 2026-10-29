//! The visual formula editor: MathLive in a web view that every formula popover borrows in turn.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSBundle, NSDictionary, NSNumber, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use objc2_web_kit::{
    WKContentWorld, WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKScriptMessage,
    WKScriptMessageHandler, WKUserContentController, WKWebView,
};

use super::EditorController;
use crate::util::{ns, obj};

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDMathFieldBridge"]
    #[ivars = RefCell<Option<Weak<EditorController>>>]
    struct Bridge;

    unsafe impl NSObjectProtocol for Bridge {}

    unsafe impl WKScriptMessageHandler for Bridge {
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn received(&self, _content: &WKUserContentController, message: &WKScriptMessage) {
            // SAFETY: The message is valid for the duration of this callback.
            let body = unsafe { message.body() };
            let Some(dict) = body.downcast_ref::<NSDictionary>() else {
                return;
            };
            let Some(editor) = self.ivars().borrow().as_ref().and_then(Weak::load) else {
                return;
            };
            let latex = ns("latex");
            if let Some(value) = dict.objectForKey(&latex)
                && let Some(s) = value.downcast_ref::<NSString>()
            {
                editor.visual_formula_changed(&s.to_string());
            } else if dict.objectForKey(&ns("cancel")).is_some() {
                editor.cancel_atom_edit();
            }
        }
    }

    unsafe impl WKNavigationDelegate for Bridge {
        #[unsafe(method(webView:didFinishNavigation:))]
        fn finished(&self, _web: &WKWebView, _navigation: Option<&WKNavigation>) {
            if let Some(field) = FIELD.with(|f| f.get().cloned()) {
                field.loaded.set(true);
                if let Some(latex) = field.pending.borrow_mut().take() {
                    field.run("setLatex(latex)", &latex);
                }
            }
        }

        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide(
            &self,
            _web: &WKWebView,
            action: &WKNavigationAction,
            handler: &block2::DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            handler.call((super::js::navigation_policy(action, "mathfield.html"),));
        }

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn terminated(&self, web: &WKWebView) {
            if let Some(field) = FIELD.with(|f| f.get().cloned()) {
                field.loaded.set(false);
                load_page(web);
            }
        }
    }
);

fn load_page(web: &WKWebView) {
    if let Some(resources) = NSBundle::mainBundle().resourceURL()
        && let Some(page) = resources.URLByAppendingPathComponent(&ns("mathfield.html"))
    {
        // SAFETY: Both URLs are file URLs inside the app bundle, the page within the readable folder.
        unsafe { web.loadFileURL_allowingReadAccessToURL(&page, &resources) };
    }
}

pub struct MathField {
    pub web: Retained<WKWebView>,
    bridge: Retained<Bridge>,
    pub(super) loaded: Cell<bool>,
    pending: RefCell<Option<String>>,
}

thread_local! {
    static FIELD: OnceCell<Rc<MathField>> = const { OnceCell::new() };
}

/// The shared visual editor; its page loads once, the first time a formula popover opens.
pub fn field(mtm: MainThreadMarker) -> Rc<MathField> {
    FIELD.with(|f| f.get_or_init(|| Rc::new(MathField::new(mtm))).clone())
}

impl MathField {
    fn new(mtm: MainThreadMarker) -> MathField {
        let bridge: Retained<Bridge> =
            // SAFETY: NSObject's initializer, called once on the newly allocated object.
            unsafe { msg_send![super(Bridge::alloc(mtm).set_ivars(RefCell::new(None))), init] };
        let config = super::js::configuration(mtm);
        // SAFETY: The configuration's own content controller.
        let content = unsafe { config.userContentController() };
        // SAFETY: The content controller retains the handler for as long as the web view lives.
        unsafe { content.addScriptMessageHandler_name(ProtocolObject::from_ref(&*bridge), &ns("splat")) };
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(480.0, 110.0));
        // SAFETY: The configuration is complete and the frame finite.
        let web = unsafe { WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &config) };
        // SAFETY: WebKit holds the delegate weakly; the content controller keeps the bridge alive.
        unsafe { web.setNavigationDelegate(Some(ProtocolObject::from_ref(&*bridge))) };
        // WKWebView has no public switch for a transparent background; this private key is the common workaround.
        let object: &NSObject = &web;
        // SAFETY: WKWebView has a boolean property under this key.
        unsafe {
            objc2_foundation::NSObjectNSKeyValueCoding::setValue_forKey(
                object,
                Some(&NSNumber::new_bool(false)),
                &ns("drawsBackground"),
            )
        };
        load_page(&web);
        MathField {
            web,
            bridge,
            loaded: Cell::new(false),
            pending: RefCell::new(None),
        }
    }

    /// Shows `latex` and reports edits to `editor` until `detach`.
    pub fn attach(&self, editor: &EditorController, latex: &str) {
        *self.bridge.ivars().borrow_mut() = Some(Weak::from(editor));
        if self.loaded.get() {
            self.run("setLatex(latex)", latex);
        } else {
            *self.pending.borrow_mut() = Some(latex.to_string());
        }
    }

    pub fn detach(&self) {
        *self.bridge.ivars().borrow_mut() = None;
        self.web.removeFromSuperview();
    }

    /// Inserts a template in MathLive syntax at the selection.
    pub fn insert(&self, template: &str) {
        self.run("insertLatex(latex)", template);
    }

    pub fn run(&self, body: &str, latex: &str) {
        let key = ns("latex");
        let dict = NSDictionary::from_slices(&[&*key], &[&*obj(ns(latex)) as &AnyObject]);
        // SAFETY: The arguments are property-list values, which WebKit hands to the script as plain values.
        unsafe {
            self.web
                .callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
                    &ns(body),
                    Some(&dict),
                    None,
                    &WKContentWorld::pageWorld(self.web.mtm()),
                    None,
                )
        };
    }
}
