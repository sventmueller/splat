use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSApplication, NSApplicationDelegate, NSDocumentController};
use objc2_foundation::{NSNotification, NSObjectProtocol};

use crate::document::MdDocument;
use crate::util::ns;

/// The German reference, for when the bundle has none in the interface language.
const QUICK_REFERENCE: &str = include_str!("../resources/de.lproj/QuickReference.md");

/// The quick reference in the interface language, chosen by the bundle like any localized resource.
fn quick_reference_text() -> String {
    let bundle = objc2_foundation::NSBundle::mainBundle();
    bundle
        .pathForResource_ofType(Some(&ns("QuickReference")), Some(&ns("md")))
        .and_then(|p| std::fs::read_to_string(p.to_string()).ok())
        .unwrap_or_else(|| QUICK_REFERENCE.to_string())
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDAppDelegate"]
    pub struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            // WebKit needs about a second to start; doing it now keeps the first formula from waiting on it.
            crate::util::perform_later(self, objc2::sel!(warmRenderer), 1.0, false);
            crate::document::watch_files(self.mtm());
            // The alert would stop a self-test run; the self-test shows it itself.
            #[cfg(not(feature = "selftest"))]
            crate::editor::offer_recovered(self.mtm());
            #[cfg(feature = "selftest")]
            crate::editor::selftest::start(self.mtm());
        }

        #[unsafe(method(applicationDidBecomeActive:))]
        fn did_become_active(&self, _notification: &NSNotification) {
            crate::document::reload_changed_documents(self.mtm());
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn should_terminate_after_last_window_closed(&self, _app: &NSApplication) -> bool {
            false
        }

        #[unsafe(method(applicationSupportsSecureRestorableState:))]
        fn supports_secure_restorable_state(&self, _app: &NSApplication) -> bool {
            true
        }
    }

    impl AppDelegate {
        #[unsafe(method(warmRenderer))]
        fn warm_renderer(&self) {
            crate::editor::warm_renderer(self.mtm());
        }

        #[unsafe(method(mdQuickReference:))]
        fn quick_reference(&self, _sender: Option<&AnyObject>) {
            let mtm = self.mtm();
            let controller = NSDocumentController::sharedDocumentController(mtm);
            let Some(kind) = controller.defaultType() else { return };
            let Ok(doc) = controller.makeUntitledDocumentOfType_error(&kind) else { return };
            if let Ok(md) = doc.clone().downcast::<MdDocument>() {
                md.set_initial_text(&quick_reference_text());
                // objc2-app-kit has no binding for -setDisplayName:.
                // SAFETY: NSDocument implements -setDisplayName: with a string argument; objc2 has no binding for it.
                let _: () = unsafe { msg_send![&*md, setDisplayName: &*ns(crate::i18n::t("Markdown-Kurzreferenz"))] };
            }
            controller.addDocument(&doc);
            doc.makeWindowControllers();
            doc.showWindows();
        }
    }
);

impl AppDelegate {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSObject's initializer, called once on the newly allocated object.
        unsafe { msg_send![super(this), init] }
    }
}
