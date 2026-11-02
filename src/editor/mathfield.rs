//! The visual formula editor: MathLive in a web view that every formula popover borrows in turn.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

use objc2::rc::{Retained, Weak};
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSDictionary, NSNumber, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use objc2_web_kit::{
    WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKScriptMessage,
    WKScriptMessageHandler, WKUserContentController, WKWebView,
};

use super::EditorController;
use super::webkit::{self, ScriptValue};
use crate::util::{contained, ns};

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
            let body = webkit::message_body(message);
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
            handler.call((webkit::navigation_policy(action, PAGE),));
        }

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn terminated(&self, web: &WKWebView) {
            if let Some(field) = FIELD.with(|f| f.get().cloned()) {
                field.loaded.set(false);
                webkit::load_bundled_page(web, PAGE);
            }
        }
    }
);

const PAGE: &str = "mathfield.html";

pub struct MathField {
    pub web: Retained<WKWebView>,
    bridge: Retained<Bridge>,
    pub(super) loaded: Cell<bool>,
    pending: RefCell<Option<String>>,
}

thread_local! {
    static FIELD: OnceCell<Rc<MathField>> = const { OnceCell::new() };
}

pub(super) const DRAWS_BACKGROUND: &str = "drawsBackground";

/// Sets a private boolean property to false and says whether that worked: a macOS without it costs the effect, not the
/// editor.
fn switch_off(object: &NSObject, key: &str) -> bool {
    contained(|| {
        // SAFETY: A boolean for a boolean property; the exception a missing key raises ends in `contained`.
        unsafe {
            objc2_foundation::NSObjectNSKeyValueCoding::setValue_forKey(
                object,
                Some(&NSNumber::new_bool(false)),
                &ns(key),
            )
        }
    })
    .is_some()
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
        let config = webkit::configuration(mtm);
        webkit::add_message_handler(&config, ProtocolObject::from_ref(&*bridge), "splat");
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(480.0, 110.0));
        let web = webkit::new_view(mtm, frame, &config);
        // SAFETY: WebKit holds the delegate weakly; the content controller keeps the bridge alive.
        unsafe { web.setNavigationDelegate(Some(ProtocolObject::from_ref(&*bridge))) };
        // WKWebView has no public switch for a transparent background; this private key is the common workaround.
        switch_off(&web, DRAWS_BACKGROUND);
        webkit::load_bundled_page(&web, PAGE);
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
        webkit::run_script(&self.web, body, &[("latex", ScriptValue::Text(latex))]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_private_key_that_is_gone_costs_only_its_effect() {
        assert!(!switch_off(&NSObject::new(), "noSuchKey"));
    }
}
