//! Calls into WebKit. objc2-web-kit marks every method unsafe because the framework has not been reviewed for it yet,
//! so most blocks here state only that a method asks for nothing. The navigation delegate, which WebKit holds weakly,
//! stays with the callers that keep it alive.

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSBundle, NSData, NSDictionary, NSError, NSNumber, NSRect, NSString};
use objc2_web_kit::{
    WKContentWorld, WKNavigationAction, WKNavigationActionPolicy, WKPDFConfiguration, WKScriptMessage,
    WKScriptMessageHandler, WKWebView, WKWebViewConfiguration, WKWebsiteDataStore,
};

use crate::util::{ns, obj};

/// A script argument, of a type WebKit hands to the script as a plain value.
pub(super) enum ScriptValue<'a> {
    Text(&'a str),
    Flag(bool),
}

fn script_arguments(args: &[(&str, ScriptValue)]) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys: Vec<Retained<NSString>> = args.iter().map(|(key, _)| ns(key)).collect();
    let values: Vec<Retained<AnyObject>> = args
        .iter()
        .map(|(_, value)| match value {
            ScriptValue::Text(text) => obj(ns(text)),
            ScriptValue::Flag(flag) => obj(NSNumber::new_bool(*flag)),
        })
        .collect();
    let key_refs: Vec<&NSString> = keys.iter().map(|key| &**key).collect();
    let value_refs: Vec<&AnyObject> = values.iter().map(|value| &**value).collect();
    NSDictionary::from_slices(&key_refs, &value_refs)
}

/// A configuration whose pages keep no cookies or storage between documents.
pub(super) fn configuration(mtm: MainThreadMarker) -> Retained<WKWebViewConfiguration> {
    // SAFETY: A plain configuration object, created on the main thread.
    let config = unsafe { WKWebViewConfiguration::new(mtm) };
    // SAFETY: A fresh non-persistent store, set before the configuration is used.
    unsafe { config.setWebsiteDataStore(&WKWebsiteDataStore::nonPersistentDataStore(mtm)) };
    config
}

pub(super) fn add_message_handler(
    config: &WKWebViewConfiguration,
    handler: &ProtocolObject<dyn WKScriptMessageHandler>,
    name: &str,
) {
    // SAFETY: The configuration's own content controller, which retains the handler for as long as the web view lives.
    unsafe {
        config
            .userContentController()
            .addScriptMessageHandler_name(handler, &ns(name))
    };
}

pub(super) fn new_view(mtm: MainThreadMarker, frame: NSRect, config: &WKWebViewConfiguration) -> Retained<WKWebView> {
    // SAFETY: The configuration is complete and the frame finite.
    unsafe { WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, config) }
}

pub(super) fn detach_delegate(web: &WKWebView) {
    // SAFETY: Without a delegate no lifetime is at stake.
    unsafe { web.setNavigationDelegate(None) };
}

/// Loads a page of the app bundle, which may read the bundle's other resources.
pub(super) fn load_bundled_page(web: &WKWebView, name: &str) {
    if let Some(resources) = NSBundle::mainBundle().resourceURL()
        && let Some(page) = resources.URLByAppendingPathComponent(&ns(name))
    {
        // SAFETY: Both URLs are file URLs inside the app bundle, the page within the readable folder.
        unsafe { web.loadFileURL_allowingReadAccessToURL(&page, &resources) };
    }
}

/// Only the bundled page itself may load: rendered formulas and diagrams come from documents, and a link or script
/// in them must not lead the view anywhere else.
pub(super) fn navigation_policy(action: &WKNavigationAction, page: &str) -> WKNavigationActionPolicy {
    // SAFETY: The navigation action carries its request while WebKit asks about it.
    let target = unsafe { action.request() }.URL();
    let expected = NSBundle::mainBundle().resourcePath().map(|r| format!("{r}/{page}"));
    let allowed = target.is_some_and(|u| u.isFileURL() && u.path().map(|p| p.to_string()) == expected);
    if allowed {
        WKNavigationActionPolicy::Allow
    } else {
        WKNavigationActionPolicy::Cancel
    }
}

pub(super) fn message_body(message: &WKScriptMessage) -> Retained<AnyObject> {
    // SAFETY: The message is valid for the duration of this callback.
    unsafe { message.body() }
}

fn send_script(
    web: &WKWebView,
    body: &str,
    args: &[(&str, ScriptValue)],
    done: &DynBlock<dyn Fn(*mut AnyObject, *mut NSError)>,
) {
    // SAFETY: `ScriptValue` only makes values that WebKit hands to the script as plain values.
    unsafe {
        web.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
            &ns(body),
            Some(&script_arguments(args)),
            None,
            &WKContentWorld::pageWorld(web.mtm()),
            Some(done),
        )
    };
}

/// The message of an error WebKit reports, or of what a script threw and did not catch, which it may quote from a document;
/// only its number goes to the log.
fn failure(error: *mut NSError, what: &'static str) -> Option<String> {
    // SAFETY: WebKit passes a valid error or null to the completion handler.
    let error = unsafe { error.as_ref() }?;
    crate::diag::report(what, error.code());
    Some(error.localizedDescription().to_string())
}

pub(super) fn run_script(web: &WKWebView, body: &str, args: &[(&str, ScriptValue)]) {
    let block = RcBlock::new(|_: *mut AnyObject, error: *mut NSError| {
        failure(error, "a script failed");
    });
    send_script(web, body, args, &block);
}

/// `done` gets what the script returned, or the message of its failure.
pub(super) fn call_script(
    web: &WKWebView,
    body: &str,
    args: &[(&str, ScriptValue)],
    done: impl Fn(Result<Option<&AnyObject>, String>) + 'static,
) {
    let block = RcBlock::new(move |result: *mut AnyObject, error: *mut NSError| {
        match failure(error, "a script failed") {
            Some(message) => done(Err(message)),
            // SAFETY: WebKit passes a valid object or null to the completion handler.
            None => done(Ok(unsafe { result.as_ref() })),
        }
    });
    send_script(web, body, args, &block);
}

/// A PDF of `rect` of the page, without a background of its own.
pub(super) fn transparent_pdf(web: &WKWebView, rect: NSRect, done: impl Fn(Option<&NSData>) + 'static) {
    // SAFETY: A plain configuration object, created on the main thread.
    let config = unsafe { WKPDFConfiguration::new(web.mtm()) };
    // SAFETY: A rect within the rendered page and a plain flag.
    unsafe {
        config.setRect(rect);
        config.setAllowTransparentBackground(true);
    }
    let block = RcBlock::new(move |data: *mut NSData, error: *mut NSError| {
        failure(error, "a PDF of a diagram failed");
        // SAFETY: WebKit passes valid data or null to the completion handler.
        done(unsafe { data.as_ref() });
    });
    // SAFETY: The configuration is set up above; the handler runs on the main thread.
    unsafe { web.createPDFWithConfiguration_completionHandler(Some(&config), &block) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_arguments_are_plain_values() {
        let args = script_arguments(&[("text", ScriptValue::Text("x")), ("flag", ScriptValue::Flag(true))]);
        let text = args.objectForKey(&ns("text")).expect("text");
        assert_eq!(
            text.downcast_ref::<NSString>().map(|s| s.to_string()).as_deref(),
            Some("x")
        );
        let flag = args.objectForKey(&ns("flag")).expect("flag");
        assert!(flag.downcast_ref::<NSNumber>().is_some_and(|n| n.boolValue()));
    }
}
