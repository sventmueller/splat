//! Calls into WebKit. objc2-web-kit marks every method unsafe because the framework has not been reviewed for it yet,
//! so most blocks here state only that a method asks for nothing. The navigation delegate, which WebKit holds weakly,
//! stays with the callers that keep it alive.

use std::cell::RefCell;

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly, Message};
use objc2_foundation::{NSBundle, NSData, NSDictionary, NSError, NSNumber, NSRect, NSString};
use objc2_web_kit::{
    WKContentRuleList, WKContentRuleListStore, WKContentWorld, WKNavigationAction, WKNavigationActionPolicy,
    WKPDFConfiguration, WKScriptMessage, WKScriptMessageHandler, WKWebView, WKWebViewConfiguration, WKWebsiteDataStore,
};

use crate::util::{guarded, ns, obj};

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

/// Loads a page of the app bundle, which may read the bundle's other resources, once the view refuses the network.
pub(super) fn load_bundled_page(web: &WKWebView, name: &str) {
    let (page_web, name) = (web.retain(), name.to_string());
    when_offline(web, move || {
        if let Some(resources) = NSBundle::mainBundle().resourceURL()
            && let Some(page) = resources.URLByAppendingPathComponent(&ns(&name))
        {
            // SAFETY: Both URLs are file URLs inside the app bundle, the page within the readable folder.
            unsafe { page_web.loadFileURL_allowingReadAccessToURL(&page, &resources) };
        }
    });
}

/// Runs `then` once `web` holds the offline rules.
pub(super) fn when_offline(web: &WKWebView, then: impl FnOnce() + 'static) {
    let web = web.retain();
    offline(move |rules| {
        if let Some(rules) = rules {
            // SAFETY: The view's own content controller, which keeps the list; adding it twice changes nothing.
            unsafe { web.configuration().userContentController().addContentRuleList(rules) };
        }
        then();
    });
}

/// What the pages may load besides their content security policy, which a script slipping through a vendored library
/// could evade: the app's own files and what a script makes in memory, never the network. WebKit's filters know no
/// alternatives, hence a rule for each scheme.
const OFFLINE_RULES: &str = r#"[
    {"trigger": {"url-filter": ".*"}, "action": {"type": "block"}},
    {"trigger": {"url-filter": "^file:"}, "action": {"type": "ignore-previous-rules"}},
    {"trigger": {"url-filter": "^data:"}, "action": {"type": "ignore-previous-rules"}},
    {"trigger": {"url-filter": "^blob:"}, "action": {"type": "ignore-previous-rules"}},
    {"trigger": {"url-filter": "^about:"}, "action": {"type": "ignore-previous-rules"}}
]"#;

type WaitingForRules = Vec<Box<dyn FnOnce(Option<&WKContentRuleList>)>>;

#[derive(Default)]
struct Rules {
    /// `Some(None)` once compiling failed: the pages then still have their policy.
    compiled: Option<Option<Retained<WKContentRuleList>>>,
    waiting: Option<WaitingForRules>,
}

thread_local! {
    static RULES: RefCell<Rules> = RefCell::default();
}

/// Runs `then` with the compiled offline rules; the first call compiles them, which takes a moment.
fn offline(then: impl FnOnce(Option<&WKContentRuleList>) + 'static) {
    if let Some(compiled) = RULES.with(|r| r.borrow().compiled.clone()) {
        then(compiled.as_deref());
        return;
    }
    let first = RULES.with(|r| {
        let mut r = r.borrow_mut();
        let first = r.waiting.is_none();
        r.waiting.get_or_insert_with(Vec::new).push(Box::new(then));
        first
    });
    if first && let Some(mtm) = MainThreadMarker::new() {
        compile_offline_rules(mtm);
    }
}

fn compile_offline_rules(mtm: MainThreadMarker) {
    // The store calls back on the main thread, where the rules wait.
    let done = completion::<WKContentRuleList>(|list, error| {
        let list = list.map(Message::retain);
        if list.is_none() {
            crate::diag::report(
                "the offline rules of the web views did not compile",
                error.map_or(0, |e| e.code()),
            );
        }
        let waiting = RULES.with(|r| {
            let mut r = r.borrow_mut();
            r.compiled = Some(list.clone());
            r.waiting.take().unwrap_or_default()
        });
        for then in waiting {
            then(list.as_deref());
        }
    });
    // SAFETY: The default store on the main thread, with rules in WebKit's JSON format and a handler for the result.
    unsafe {
        match WKContentRuleListStore::defaultStore(mtm) {
            Some(store) => store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(
                Some(&ns("splat-offline")),
                Some(&ns(OFFLINE_RULES)),
                Some(&done),
            ),
            None => done.call((std::ptr::null_mut(), std::ptr::null_mut())),
        }
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

/// A completion handler for WebKit, which hands it a value and an error, each of them or nil. A panic in `f` ends
/// only `f`.
fn completion<T: objc2::Message + 'static>(
    f: impl Fn(Option<&T>, Option<&NSError>) + 'static,
) -> RcBlock<dyn Fn(*mut T, *mut NSError)> {
    RcBlock::new(move |value: *mut T, error: *mut NSError| {
        guarded((), || {
            // SAFETY: WebKit passes a completion handler a valid value and error or nil, alive for the length of the
            // call.
            let (value, error) = unsafe { (value.as_ref(), error.as_ref()) };
            f(value, error);
        });
    })
}

/// The message of an error WebKit reports, or of what a script threw and did not catch, which it may quote from a document;
/// only its number goes to the log.
fn failure(error: Option<&NSError>, what: &'static str) -> Option<String> {
    let error = error?;
    crate::diag::report(what, error.code());
    Some(thrown_message(error).unwrap_or_else(|| error.localizedDescription().to_string()))
}

/// WebKit's description of an exception is the same for all of them, and only this key of its details says which it was.
fn thrown_message(error: &NSError) -> Option<String> {
    let message = error.userInfo().objectForKey(&ns("WKJavaScriptExceptionMessage"))?;
    Some(message.downcast::<NSString>().ok()?.to_string())
}

pub(super) fn run_script(web: &WKWebView, body: &str, args: &[(&str, ScriptValue)]) {
    let block = completion::<AnyObject>(|_, error| {
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
    let block = completion::<AnyObject>(move |result, error| match failure(error, "a script failed") {
        Some(message) => done(Err(message)),
        None => done(Ok(result)),
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
    let block = completion::<NSData>(move |data, error| {
        failure(error, "a PDF of a diagram failed");
        done(data);
    });
    // SAFETY: The configuration is set up above; the handler runs on the main thread.
    unsafe { web.createPDFWithConfiguration_completionHandler(Some(&config), &block) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thrown_message_comes_from_the_details_of_the_error() {
        let key = ns("WKJavaScriptExceptionMessage");
        let message = obj(ns("Error: bad formula"));
        let details = NSDictionary::from_retained_objects(&[&*key], &[message]);
        // SAFETY: A domain, a number and details of a type the method documents.
        let thrown = unsafe { NSError::errorWithDomain_code_userInfo(&ns("WKErrorDomain"), 4, Some(&details)) };
        assert_eq!(thrown_message(&thrown).as_deref(), Some("Error: bad formula"));
        let other = NSError::new(1, &ns("WKErrorDomain"));
        assert_eq!(thrown_message(&other), None);
    }

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
