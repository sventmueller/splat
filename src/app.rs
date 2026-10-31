use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationDelegate, NSControlStateValueOff, NSControlStateValueOn, NSDocumentController,
    NSMenuItem, NSMenuItemValidation, NSWorkspace,
};
use objc2_foundation::{NSBundle, NSError, NSNotification, NSObjectProtocol};
use objc2_uniform_type_identifiers::UTType;

use crate::document::MdDocument;
use crate::util::{constant, ns};

/// The type macOS gives files ending in .md and .markdown.
const MARKDOWN: &str = "net.daringfireball.markdown";

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

        #[unsafe(method(mdOpenMarkdownHere:))]
        fn open_markdown_here(&self, _sender: Option<&AnyObject>) {
            make_default(MARKDOWN);
        }

        #[unsafe(method(mdToggleUpdateChecks:))]
        fn toggle_update_checks(&self, _sender: Option<&AnyObject>) {
            crate::updates::set_checks_automatically(!crate::updates::checks_automatically());
        }

        #[unsafe(method(mdToggleUpdateDownloads:))]
        fn toggle_update_downloads(&self, _sender: Option<&AnyObject>) {
            crate::updates::set_installs_automatically(!crate::updates::installs_automatically());
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

    unsafe impl NSMenuItemValidation for AppDelegate {
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            let action = item.action();
            let toggle = if action == Some(sel!(mdOpenMarkdownHere:)) {
                let default = opens_markdown();
                // macOS needs some app for Markdown, so being the default cannot be switched off here.
                Some((default, !default))
            } else if action == Some(sel!(mdToggleUpdateChecks:)) {
                Some((crate::updates::checks_automatically(), true))
            } else if action == Some(sel!(mdToggleUpdateDownloads:)) {
                Some((
                    crate::updates::installs_automatically(),
                    crate::updates::can_install_automatically(),
                ))
            } else {
                None
            };
            match toggle {
                Some((on, enabled)) => {
                    item.setState(if on { NSControlStateValueOn } else { NSControlStateValueOff });
                    enabled
                }
                None => true,
            }
        }
    }
);

/// Whether double-clicking a Markdown file opens Splat. macOS keeps the default app by bundle ID, so any copy counts.
pub fn opens_markdown() -> bool {
    opens(MARKDOWN)
}

pub(crate) fn opens(kind: &str) -> bool {
    let Some(kind) = UTType::typeWithIdentifier(&ns(kind)) else {
        return false;
    };
    let handler = NSWorkspace::sharedWorkspace()
        .URLForApplicationToOpenContentType(&kind)
        .and_then(|url| NSBundle::bundleWithURL(&url))
        .and_then(|bundle| bundle.bundleIdentifier());
    handler.is_some() && handler == NSBundle::mainBundle().bundleIdentifier()
}

/// macOS asks the user to confirm a change in a dialog of its own.
pub(crate) fn make_default(kind: &str) {
    let Some(kind) = UTType::typeWithIdentifier(&ns(kind)) else {
        return;
    };
    let done = RcBlock::new(|error: *mut NSError| {
        // SAFETY: the completion handler's error is nil or a valid NSError for the duration of the call.
        let Some(error) = (unsafe { Retained::retain(error) }) else {
            return;
        };
        if canceled(&error) {
            return;
        }
        DispatchQueue::main().exec_async(move || {
            if let Some(mtm) = MainThreadMarker::new() {
                NSApplication::sharedApplication(mtm).presentError(&error);
            }
        });
    });
    NSWorkspace::sharedWorkspace().setDefaultApplicationAtURL_toOpenContentType_completionHandler(
        &NSBundle::mainBundle().bundleURL(),
        &kind,
        Some(&done),
    );
}

/// The user's "Keep" in the confirmation arrives as an error, with userCanceledErr underneath.
fn canceled(error: &NSError) -> bool {
    let cancel = |e: &NSError| e.code() == -128 && *e.domain() == *constant!(objc2_foundation::NSOSStatusErrorDomain);
    cancel(error) || error.underlyingErrors().iter().any(|e| cancel(&e))
}

impl AppDelegate {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSObject's initializer, called once on the newly allocated object.
        unsafe { msg_send![super(this), init] }
    }
}

#[cfg(test)]
mod tests {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSCocoaErrorDomain, NSDictionary, NSError, NSOSStatusErrorDomain, NSUnderlyingErrorKey};

    use super::canceled;
    use crate::util::constant;

    #[test]
    fn only_the_users_keep_counts_as_canceled() {
        let (status, cocoa) = (constant!(NSOSStatusErrorDomain), constant!(NSCocoaErrorDomain));
        let keep = NSError::new(-128, status);
        let underlying: Retained<AnyObject> = keep.clone().into();
        // SAFETY: NSUnderlyingErrorKey holds an NSError.
        let wrapped = unsafe {
            NSError::errorWithDomain_code_userInfo(
                cocoa,
                256,
                Some(&NSDictionary::from_retained_objects(
                    &[NSUnderlyingErrorKey],
                    &[underlying],
                )),
            )
        };
        assert!(canceled(&keep) && canceled(&wrapped));
        assert!(!canceled(&NSError::new(256, cocoa)) && !canceled(&NSError::new(-128, cocoa)));
    }
}
