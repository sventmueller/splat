//! Renders formulas (MathJax) and diagrams (Mermaid) in a hidden WebKit view.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObject};
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSBackingStoreType, NSImage, NSPanel, NSWindowStyleMask};
use objc2_foundation::{
    NSBundle, NSData, NSDictionary, NSError, NSNumber, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL,
};
use objc2_web_kit::{
    WKContentWorld, WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate,
    WKPDFConfiguration, WKWebView, WKWebViewConfiguration, WKWebsiteDataStore,
};

use super::EditorController;
use super::images::{Rendered, Sizing};
use crate::markdown::doc::DiagramKind;
use crate::util::{ns, obj};

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    Math {
        tex: String,
        display: bool,
        dark: bool,
    },
    Diagram {
        source: String,
        kind: DiagramKind,
        dark: bool,
    },
}

pub struct Renderer {
    window: Retained<NSPanel>,
    web: RefCell<Retained<WKWebView>>,
    page: Option<(Retained<NSURL>, Retained<NSURL>)>,
    /// The key WebKit is rendering, and a number identifying that attempt for its time limit.
    current: RefCell<Option<Key>>,
    job: Cell<u64>,
    _delegate: Retained<NavDelegate>,
    ready: Cell<bool>,
    busy: Cell<bool>,
    queue: RefCell<VecDeque<Key>>,
    pending: RefCell<HashSet<Key>>,
    failed: RefCell<HashSet<Key>>,
    done: RefCell<HashMap<Key, Rendered>>,
    errors: RefCell<HashMap<Key, String>>,
    listeners: RefCell<Vec<Weak<EditorController>>>,
    /// Last time each rendered key was requested, so the least recently used ones are dropped first.
    used: RefCell<HashMap<Key, u64>>,
    tick: Cell<u64>,
    /// Formulas whose SVG is being converted to PDF on the background thread.
    converting: Cell<usize>,
}

/// Rendered formulas and diagrams kept in memory; a document with more than this re-renders on demand.
const CACHE_LIMIT: usize = 1500;

thread_local! {
    static RENDERER: OnceCell<Rc<Renderer>> = const { OnceCell::new() };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDRenderNavigationDelegate"]
    struct NavDelegate;

    unsafe impl NSObjectProtocol for NavDelegate {}

    unsafe impl WKNavigationDelegate for NavDelegate {
        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish(&self, web: &WKWebView, _navigation: Option<&WKNavigation>) {
            if let Some(r) = renderer_if_started().filter(|r| r.is_current(web)) {
                r.ready.set(true);
                r.pump();
            }
        }

        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide(
            &self,
            _web: &WKWebView,
            action: &WKNavigationAction,
            handler: &block2::DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            handler.call((navigation_policy(action, "render.html"),));
        }

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn terminated(&self, web: &WKWebView) {
            if let Some(r) = renderer_if_started().filter(|r| r.is_current(web)) {
                r.restart();
            }
        }
    }
);

/// The view that renders, filling the renderer's window, with the bundled page loading.
fn web_view(
    window: &NSPanel,
    delegate: &NavDelegate,
    page: Option<&(Retained<NSURL>, Retained<NSURL>)>,
) -> Retained<WKWebView> {
    let mtm = window.mtm();
    // SAFETY: The configuration is complete and the frame finite.
    let web = unsafe {
        WKWebView::initWithFrame_configuration(
            WKWebView::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), window.frame().size),
            &configuration(mtm),
        )
    };
    // SAFETY: WebKit holds the delegate weakly; the renderer keeps it alive next to the web view.
    unsafe { web.setNavigationDelegate(Some(objc2::runtime::ProtocolObject::from_ref(delegate))) };
    window.setContentView(Some(&web));
    if let Some((page, resources)) = page {
        // SAFETY: Both URLs are file URLs inside the app bundle, the page within the readable folder.
        unsafe { web.loadFileURL_allowingReadAccessToURL(page, resources) };
    }
    web
}

/// A configuration whose pages keep no cookies or storage between documents.
pub(super) fn configuration(mtm: MainThreadMarker) -> Retained<WKWebViewConfiguration> {
    // SAFETY: A plain configuration object, created on the main thread.
    let config = unsafe { WKWebViewConfiguration::new(mtm) };
    // SAFETY: A fresh non-persistent store, set before the configuration is used.
    unsafe { config.setWebsiteDataStore(&WKWebsiteDataStore::nonPersistentDataStore(mtm)) };
    config
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

/// A render that has not answered by then counts as failed, and a new web view takes over.
const RENDER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

fn renderer_if_started() -> Option<Rc<Renderer>> {
    RENDERER.with(|r| r.get().cloned())
}

/// The shared renderer; created on first use and kept for the lifetime of the app.
pub fn renderer(mtm: MainThreadMarker) -> Rc<Renderer> {
    RENDERER.with(|cell| cell.get_or_init(|| Rc::new(Renderer::new(mtm))).clone())
}

pub fn cached(key: &Key) -> Option<Rendered> {
    renderer_if_started().and_then(|r| r.done.borrow().get(key).cloned())
}

/// Keys with a rendered image, readable from the background layout as well.
static READY: std::sync::Mutex<Option<HashSet<Key>>> = std::sync::Mutex::new(None);

pub fn ready(key: &Key) -> bool {
    READY
        .lock()
        .ok()
        .is_some_and(|r| r.as_ref().is_some_and(|set| set.contains(key)))
}

fn set_ready(key: &Key, on: bool) {
    if let Ok(mut r) = READY.lock() {
        let set = r.get_or_insert_with(HashSet::new);
        if on {
            set.insert(key.clone());
        } else {
            set.remove(key);
        }
    }
}

/// What a finished render reports: `None` while the key is still queued, else the MathJax or Mermaid error.
pub fn outcome(key: &Key) -> Option<Result<(), String>> {
    let r = renderer_if_started()?;
    if let Some(message) = r.errors.borrow().get(key) {
        return Some(Err(message.clone()));
    }
    if r.done.borrow().contains_key(key) {
        return Some(Ok(()));
    }
    r.failed
        .borrow()
        .contains(key)
        .then(|| Err(crate::i18n::t("Darstellung fehlgeschlagen").to_string()))
}

impl Renderer {
    fn new(mtm: MainThreadMarker) -> Renderer {
        // WebKit only lays out and measures text inside a real window, so the view lives in an
        // invisible window far off screen instead of being created detached. A panel, because AppKit counts every
        // other visible window as open and then answers a click on the Dock icon without opening a document.
        let frame = NSRect::new(NSPoint::new(-30000.0, -30000.0), NSSize::new(1600.0, 1200.0));
        let window = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            frame,
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        );
        // SAFETY: Switching it off keeps AppKit from releasing the window behind Rust's back.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setAlphaValue(0.0);
        window.setHidesOnDeactivate(false);
        window.setIgnoresMouseEvents(true);
        window.setExcludedFromWindowsMenu(true);
        // SAFETY: NSObject's initializer, called once on the newly allocated object.
        let delegate: Retained<NavDelegate> = unsafe { msg_send![super(NavDelegate::alloc(mtm).set_ivars(())), init] };
        let page = NSBundle::mainBundle()
            .resourceURL()
            .and_then(|resources| Some((resources.URLByAppendingPathComponent(&ns("render.html"))?, resources)));
        let web = web_view(&window, &delegate, page.as_ref());
        window.orderBack(None);
        Renderer {
            window,
            web: RefCell::new(web),
            page,
            current: RefCell::new(None),
            job: Cell::new(0),
            _delegate: delegate,
            ready: Cell::new(false),
            busy: Cell::new(false),
            queue: RefCell::new(VecDeque::new()),
            pending: RefCell::new(HashSet::new()),
            failed: RefCell::new(HashSet::new()),
            done: RefCell::new(HashMap::new()),
            errors: RefCell::new(HashMap::new()),
            used: RefCell::new(HashMap::new()),
            tick: Cell::new(0),
            converting: Cell::new(0),
            listeners: RefCell::new(Vec::new()),
        }
    }

    pub fn listen(&self, controller: &EditorController) {
        let mut l = self.listeners.borrow_mut();
        l.retain(|w| w.load().is_some());
        if !l
            .iter()
            .any(|w| w.load().is_some_and(|c| std::ptr::eq(&*c, controller)))
        {
            l.push(Weak::from(controller));
        }
    }

    /// Queues keys that are neither rendered, failed nor already queued.
    pub fn request(self: &Rc<Self>, keys: Vec<Key>) {
        let tick = self.tick.get() + 1;
        self.tick.set(tick);
        {
            let mut used = self.used.borrow_mut();
            for key in &keys {
                used.insert(key.clone(), tick);
            }
        }
        self.evict();
        {
            let done = self.done.borrow();
            let failed = self.failed.borrow();
            let mut pending = self.pending.borrow_mut();
            let mut queue = self.queue.borrow_mut();
            for key in keys {
                if !done.contains_key(&key) && !failed.contains(&key) && pending.insert(key.clone()) {
                    queue.push_back(key);
                }
            }
        }
        self.pump();
    }

    fn pump(&self) {
        if !self.ready.get() || self.busy.get() {
            return;
        }
        let Some(key) = self.queue.borrow_mut().pop_front() else {
            return;
        };
        self.busy.set(true);
        let job = self.job.get() + 1;
        self.job.set(job);
        *self.current.borrow_mut() = Some(key.clone());
        if let Ok(when) = DispatchTime::try_from(RENDER_TIMEOUT) {
            let _ = DispatchQueue::main().after(when, move || {
                if let Some(r) = renderer_if_started() {
                    r.give_up(job);
                }
            });
        }
        match &key {
            Key::Math { tex, display, dark } => self.render_math(key.clone(), tex, *display, *dark),
            Key::Diagram { source, kind, dark } => self.render_diagram(key.clone(), source, *kind, *dark),
        }
    }

    fn finish(&self, key: Key, result: Option<Rendered>) {
        let current = self.release(&key);
        self.store(key, result);
        self.notify_if_idle();
        if current {
            self.pump();
        }
    }

    /// Frees WebKit for the next key if `key` is the one it renders; an answer after a restart is only stored.
    fn release(&self, key: &Key) -> bool {
        if self.current.borrow().as_ref() != Some(key) {
            return false;
        }
        *self.current.borrow_mut() = None;
        self.busy.set(false);
        true
    }

    /// Ends render attempt `job` if it is still running: it failed, and a new view takes over. A script that never
    /// returns keeps its web process busy, and that process neither loads the page again nor quits with its view.
    fn give_up(&self, job: u64) {
        if job == self.job.get() && self.busy.get() {
            let stuck = self.web.borrow().clone();
            // SAFETY: Detaching the delegate of a view that is about to go away.
            unsafe { stuck.setNavigationDelegate(None) };
            // WebKit has no public way to stop a page's process, and its private `_killWebContentProcess` leaves a
            // busy one running; the private process number is used where it exists.
            if stuck.respondsToSelector(objc2::sel!(_webProcessIdentifier)) {
                // SAFETY: A WKWebView method without arguments that returns a pid_t, present as just checked.
                let pid: i32 = unsafe { msg_send![&*stuck, _webProcessIdentifier] };
                if pid > 0 {
                    // SAFETY: The process belongs to this view alone, as its website data store is its own (see
                    // `configuration`), so nothing else depends on it.
                    unsafe { kill(pid, SIGKILL) };
                }
            }
            *self.web.borrow_mut() = web_view(&self.window, &self._delegate, self.page.as_ref());
            self.reset();
        }
    }

    fn is_current(&self, web: &WKWebView) -> bool {
        std::ptr::eq(web, &**self.web.borrow())
    }

    /// The web process ended: the page loads again in a new one.
    fn restart(&self) {
        self.reset();
        if let Some((page, resources)) = &self.page {
            // SAFETY: Both URLs are file URLs inside the app bundle, the page within the readable folder.
            unsafe { self.web.borrow().loadFileURL_allowingReadAccessToURL(page, resources) };
        }
    }

    /// Counts the running render as failed and waits for the page to be ready again.
    fn reset(&self) {
        let job = self.job.get();
        if let Some(key) = self.current.borrow_mut().take() {
            self.store(key, None);
        }
        self.job.set(job + 1);
        self.busy.set(false);
        self.ready.set(false);
        self.notify_if_idle();
    }

    fn store(&self, key: Key, result: Option<Rendered>) {
        self.pending.borrow_mut().remove(&key);
        match result {
            Some(r) => {
                set_ready(&key, true);
                self.done.borrow_mut().insert(key, r);
            }
            None => {
                self.failed.borrow_mut().insert(key);
            }
        }
    }

    /// Nothing is queued, rendering or converting.
    pub fn idle(&self) -> bool {
        self.queue.borrow().is_empty() && !self.busy.get() && self.converting.get() == 0
    }

    /// Documents re-render once everything queued has arrived, not after every single formula.
    fn notify_if_idle(&self) {
        if self.idle() {
            let listeners: Vec<_> = self.listeners.borrow().iter().filter_map(Weak::load).collect();
            for l in listeners {
                l.images_changed();
            }
        }
    }

    /// Once the cache is full, drops what no open document asked for in its last few requests. Formulas still in
    /// use stay, however many there are: dropping them would only make the documents request them again.
    fn evict(&self) {
        if self.done.borrow().len() + self.failed.borrow().len() <= CACHE_LIMIT {
            return;
        }
        let recent = self.tick.get().saturating_sub(8);
        let stale: Vec<Key> = self
            .done
            .borrow()
            .keys()
            .chain(self.failed.borrow().iter())
            .filter(|k| self.used.borrow().get(*k).copied().unwrap_or(0) < recent)
            .cloned()
            .collect();
        for key in stale {
            set_ready(&key, false);
            self.done.borrow_mut().remove(&key);
            self.failed.borrow_mut().remove(&key);
            self.errors.borrow_mut().remove(&key);
            self.used.borrow_mut().remove(&key);
        }
    }

    fn call(&self, body: &str, args: &[(&str, Retained<AnyObject>)], done: impl Fn(Option<&NSDictionary>) + 'static) {
        let keys: Vec<Retained<NSString>> = args.iter().map(|(k, _)| ns(k)).collect();
        let key_refs: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
        let values: Vec<&AnyObject> = args.iter().map(|(_, v)| &**v).collect();
        let dict = NSDictionary::from_slices(&key_refs, &values);
        let block = RcBlock::new(move |result: *mut AnyObject, _error: *mut NSError| {
            // SAFETY: WebKit passes a valid object or null to the completion handler.
            let dict = unsafe { result.as_ref() }.and_then(|o| o.downcast_ref::<NSDictionary>());
            done(dict);
        });
        // SAFETY: The arguments are property-list values, which WebKit hands to the script as plain values.
        unsafe {
            self.web
                .borrow()
                .callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
                    &ns(body),
                    Some(&dict),
                    None,
                    &WKContentWorld::pageWorld(self.window.mtm()),
                    Some(&block),
                )
        };
    }

    fn render_math(&self, key: Key, tex: &str, display: bool, dark: bool) {
        let color = if dark { "#E8E8E8" } else { "#1D1D1F" };
        let args = [
            ("tex", obj(ns(tex))),
            ("display", obj(NSNumber::new_bool(display))),
            ("color", obj(ns(color))),
        ];
        let label = tex.to_string();
        self.call("return await renderMath(tex, display, color);", &args, move |dict| {
            if let (Some(r), Some(message)) = (renderer_if_started(), dict.and_then(|d| string(d, "error")))
                && !message.is_empty()
            {
                r.errors.borrow_mut().insert(key.clone(), message);
            }
            let Some(r) = renderer_if_started() else { return };
            let job = dict.and_then(|d| {
                let svg = string(d, "svg")?;
                Some((svg, number(d, "w")?, number(d, "h")?, number(d, "va").unwrap_or(0.0)))
            });
            let Some((svg, w, h, va)) = job else {
                r.finish(key.clone(), None);
                return;
            };
            // The next formula renders in WebKit while this one converts on the background thread.
            r.converting.set(r.converting.get() + 1);
            if r.release(&key) {
                r.pump();
            }
            let key = key.clone();
            let label = label.clone();
            background(move || {
                let pdf = svg_to_pdf(&svg);
                DispatchQueue::main().exec_async(move || {
                    let Some(r) = renderer_if_started() else { return };
                    r.converting.set(r.converting.get() - 1);
                    let rendered = pdf.and_then(|pdf| pdf_image(&pdf, &label)).map(|image| Rendered {
                        image,
                        sizing: Sizing::Ex {
                            width: w,
                            height: h,
                            valign: va,
                        },
                    });
                    r.store(key, rendered);
                    r.notify_if_idle();
                });
            });
        });
    }

    fn render_diagram(&self, key: Key, source: &str, kind: DiagramKind, dark: bool) {
        let language = match kind {
            DiagramKind::Mermaid => "mermaid",
            DiagramKind::Graphviz => "graphviz",
            DiagramKind::PlantUml => "plantuml",
        };
        let args = [
            ("code", obj(ns(source))),
            ("language", obj(ns(language))),
            ("dark", obj(NSNumber::new_bool(dark))),
        ];
        // Weak: WebKit keeps this handler until the script answers, and a view held by it could never close.
        let web = Weak::from_retained(&self.web.borrow());
        let label = source.lines().next().unwrap_or(crate::i18n::t("Diagramm")).to_string();
        self.call(
            "return await renderDiagram(language, code, dark);",
            &args,
            move |dict| {
                if let (Some(r), Some(message)) = (renderer_if_started(), dict.and_then(|d| string(d, "error"))) {
                    r.errors.borrow_mut().insert(key.clone(), message);
                }
                let rect =
                    dict.and_then(|d| Some((number(d, "x")?, number(d, "y")?, number(d, "w")?, number(d, "h")?)));
                let (Some((x, y, w, h)), Some(web)) = (rect.filter(|r| r.2 > 0.0 && r.3 > 0.0), web.load()) else {
                    if let Some(r) = renderer_if_started() {
                        r.finish(key.clone(), None);
                    }
                    return;
                };
                // SAFETY: A plain configuration object, created on the main thread.
                let config = unsafe { WKPDFConfiguration::new(web.mtm()) };
                // SAFETY: A rect within the rendered page and a plain flag.
                unsafe {
                    config.setRect(NSRect::new(NSPoint::new(x, y), NSSize::new(w, h)));
                    config.setAllowTransparentBackground(true);
                }
                let key = key.clone();
                let label = label.clone();
                let block = RcBlock::new(move |data: *mut NSData, _error: *mut NSError| {
                    // SAFETY: WebKit passes valid data or null to the completion handler.
                    let rendered = unsafe { data.as_ref() }.and_then(|d| {
                        let image = NSImage::initWithData(NSImage::alloc(), d)?;
                        image.setAccessibilityDescription(Some(&ns(&crate::i18n::tf("Diagramm: {}", &[&label]))));
                        Some(Rendered {
                            image,
                            sizing: Sizing::Points {
                                size: NSSize::new(w, h),
                                descent: 0.0,
                            },
                        })
                    });
                    if let Some(r) = renderer_if_started() {
                        r.finish(key.clone(), rendered);
                    }
                });
                // SAFETY: The configuration is set up above; the handler runs on the main thread.
                unsafe { web.createPDFWithConfiguration_completionHandler(Some(&config), &block) };
            },
        );
    }
}

unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}

const SIGKILL: i32 = 9;

type Job = Box<dyn FnOnce() + Send>;

thread_local! {
    static WORKER: OnceCell<std::sync::mpsc::Sender<Job>> = const { OnceCell::new() };
}

/// Runs CPU work off the main thread, one job after the other.
pub fn background(job: impl FnOnce() + Send + 'static) {
    WORKER.with(|w| {
        let sender = w.get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel::<Job>();
            std::thread::Builder::new()
                .name("splat-render".into())
                .spawn(move || {
                    for job in rx {
                        // A failing conversion must not take the thread down, or no formula would render anymore.
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                    }
                })
                .expect("render thread");
            tx
        });
        let _ = sender.send(Box::new(job));
    });
}

fn string(d: &NSDictionary, key: &str) -> Option<String> {
    let k = ns(key);
    let v = d.objectForKey(&k)?;
    v.downcast_ref::<NSString>().map(|s| s.to_string())
}

fn number(d: &NSDictionary, key: &str) -> Option<f64> {
    let k = ns(key);
    let v = d.objectForKey(&k)?;
    v.downcast_ref::<NSNumber>().map(|n| n.doubleValue())
}

/// MathJax output is pure paths, so it converts to a vector PDF without WebKit; the SVG still derives from the
/// document, so the conversion runs in a decoder helper.
fn svg_to_pdf(svg: &str) -> Option<Vec<u8>> {
    super::decoder::svg_to_pdf(svg)
}

fn pdf_image(pdf: &[u8], label: &str) -> Option<Retained<NSImage>> {
    let data = NSData::with_bytes(pdf);
    let image = NSImage::initWithData(NSImage::alloc(), &data)?;
    image.setAccessibilityDescription(Some(&ns(&crate::i18n::tf("Formel: {}", &[label]))));
    Some(image)
}

#[cfg(test)]
mod tests {
    #[test]
    fn mathjax_like_svg_converts() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="40" height="20" viewBox="0 -750 1000 1000" color="#1D1D1F"><defs><path id="g" d="M10 10L500 10L500 -500Z"/></defs><g fill="currentColor"><use xlink:href="#g"/></g></svg>"##;
        let pdf =
            super::super::safe_image::svg_to_pdf(svg, std::sync::Arc::new(svg2pdf::usvg::fontdb::Database::new()))
                .expect("pdf");
        assert!(pdf.starts_with(b"%PDF"));
    }
}
