//! Renders formulas (MathJax) and diagrams (Mermaid) in a hidden WebKit view.

use std::cell::{OnceCell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSBackingStoreType, NSImage, NSPanel, NSWindowStyleMask};
use objc2_foundation::{NSDictionary, NSNumber, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use objc2_web_kit::{WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKWebView};

use super::EditorController;
use super::images::{Rendered, Sizing};
use super::render_queue::RenderQueue;
use super::safe_image::Decoded;
use super::webkit::{self, ScriptValue};
use crate::markdown::doc::DiagramKind;
use crate::util::ns;

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
    _delegate: Retained<NavDelegate>,
    queue: RefCell<RenderQueue<Key, Rendered>>,
    listeners: RefCell<Vec<Weak<EditorController>>>,
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
                r.queue.borrow_mut().page_ready();
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
            handler.call((webkit::navigation_policy(action, PAGE),));
        }

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn terminated(&self, web: &WKWebView) {
            if let Some(r) = renderer_if_started().filter(|r| r.is_current(web)) {
                r.restart();
            }
        }
    }
);

const PAGE: &str = "render.html";

/// The view that renders, filling the renderer's window, with the bundled page loading.
fn web_view(window: &NSPanel, delegate: &NavDelegate) -> Retained<WKWebView> {
    let mtm = window.mtm();
    let web = webkit::new_view(
        mtm,
        NSRect::new(NSPoint::new(0.0, 0.0), window.frame().size),
        &webkit::configuration(mtm),
    );
    // SAFETY: WebKit holds the delegate weakly; the renderer keeps it alive next to the web view.
    unsafe { web.setNavigationDelegate(Some(objc2::runtime::ProtocolObject::from_ref(delegate))) };
    window.setContentView(Some(&web));
    webkit::load_bundled_page(&web, PAGE);
    web
}

/// A render that has not answered by then counts as failed, and a new web view takes over.
pub(super) const RENDER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

fn renderer_if_started() -> Option<Rc<Renderer>> {
    RENDERER.with(|r| r.get().cloned())
}

/// The shared renderer; created on first use and kept for the lifetime of the app.
pub fn renderer(mtm: MainThreadMarker) -> Rc<Renderer> {
    RENDERER.with(|cell| {
        cell.get_or_init(|| {
            let renderer = Rc::new(Renderer::new(mtm));
            renderer.watch_load();
            renderer
        })
        .clone()
    })
}

#[cfg(feature = "selftest")]
impl Renderer {
    /// The page itself, for what the selftest tries against it.
    pub(super) fn web(&self) -> Retained<WKWebView> {
        self.web.borrow().clone()
    }
}

/// Frees what the shared renderer holds for documents that no longer ask for it.
pub fn trim() {
    if let Some(r) = renderer_if_started() {
        r.evict(0);
    }
}

pub fn cached(key: &Key) -> Option<Rendered> {
    renderer_if_started().and_then(|r| r.queue.borrow().rendered(key).cloned())
}

/// Keys with a rendered image, readable from the background layout as well.
static READY: std::sync::Mutex<Option<HashSet<Key>>> = std::sync::Mutex::new(None);

pub fn ready(key: &Key) -> bool {
    READY
        .lock()
        .ok()
        .is_some_and(|r| r.as_ref().is_some_and(|set| set.contains(key)))
}

pub(super) fn set_ready(key: &Key, on: bool) {
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
    let outcome = renderer_if_started()?.queue.borrow().outcome(key)?;
    Some(outcome.map_err(|message| message.unwrap_or_else(|| crate::i18n::t("Darstellung fehlgeschlagen").to_string())))
}

impl Key {
    pub fn math(tex: &str, display: bool, dark: bool) -> Key {
        Key::Math {
            tex: tex.to_string(),
            display,
            dark,
        }
    }

    pub fn diagram(source: &str, kind: DiagramKind, dark: bool) -> Key {
        Key::Diagram {
            source: source.to_string(),
            kind,
            dark,
        }
    }
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
        let web = web_view(&window, &delegate);
        window.orderBack(None);
        Renderer {
            window,
            web: RefCell::new(web),
            _delegate: delegate,
            queue: RefCell::default(),
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
        self.queue.borrow_mut().request(keys);
        self.evict(CACHE_LIMIT);
        self.pump();
    }

    fn pump(&self) {
        let Some((key, job)) = self.queue.borrow_mut().next() else {
            return;
        };
        if let Ok(when) = DispatchTime::try_from(RENDER_TIMEOUT) {
            let _ = DispatchQueue::main().after(when, move || {
                if let Some(r) = renderer_if_started() {
                    r.give_up(job);
                }
            });
        }
        match &key {
            Key::Math { tex, display, dark } => self.render_math(key.clone(), job, tex, *display, *dark),
            Key::Diagram { source, kind, dark } => self.render_diagram(key.clone(), job, source, *kind, *dark),
        }
    }

    fn finish(&self, key: Key, result: Option<Rendered>) {
        let current = self.queue.borrow_mut().release(&key);
        self.store(key, result);
        self.notify_if_idle();
        if current {
            self.pump();
        }
    }

    /// Ends render attempt `job` if it is still running: it failed, and a new view takes over. A script that never
    /// returns keeps its web process busy, and that process neither loads the page again nor quits with its view.
    fn give_up(&self, job: u64) {
        if self.queue.borrow().timed_out(job) {
            self.replace_view();
            self.lost_page();
            self.watch_load();
        }
    }

    /// A new web view takes over from one that does not answer.
    fn replace_view(&self) {
        let stuck = self.web.borrow().clone();
        webkit::detach_delegate(&stuck);
        // WebKit has no public way to stop a page's process, and its private `_killWebContentProcess` leaves a
        // busy one running; the private process number is used where it exists.
        if stuck.respondsToSelector(objc2::sel!(_webProcessIdentifier)) {
            // SAFETY: A WKWebView method without arguments that returns a pid_t, present as just checked.
            let pid: i32 = unsafe { msg_send![&*stuck, _webProcessIdentifier] };
            crate::util::end_process(pid);
        }
        *self.web.borrow_mut() = web_view(&self.window, &self._delegate);
    }

    /// WebKit reports a page that never finishes loading to nobody, and queued formulas would wait forever.
    fn watch_load(&self) {
        let job = self.queue.borrow_mut().load_started();
        if let Ok(when) = DispatchTime::try_from(RENDER_TIMEOUT) {
            let _ = DispatchQueue::main().after(when, move || {
                if let Some(r) = renderer_if_started()
                    && r.queue.borrow().load_timed_out(job)
                {
                    r.replace_view();
                    r.watch_load();
                }
            });
        }
    }

    fn is_current(&self, web: &WKWebView) -> bool {
        std::ptr::eq(web, &**self.web.borrow())
    }

    /// The web process ended: the page loads again in a new one.
    fn restart(&self) {
        self.lost_page();
        webkit::load_bundled_page(&self.web.borrow(), PAGE);
        self.watch_load();
    }

    /// Waits for the page to be ready again.
    fn lost_page(&self) {
        self.queue.borrow_mut().lost_page();
        self.notify_if_idle();
    }

    fn store(&self, key: Key, result: Option<Rendered>) {
        let key_for_layout = key.clone();
        if self.queue.borrow_mut().store(key, result) {
            set_ready(&key_for_layout, true);
        }
    }

    /// Nothing is queued, rendering or converting.
    pub fn idle(&self) -> bool {
        self.queue.borrow().idle()
    }

    /// Documents re-render once everything queued has arrived, not after every single formula.
    fn notify_if_idle(&self) {
        if self.idle() {
            let listeners: Vec<_> = self.listeners.borrow().iter().filter_map(Weak::load).collect();
            for l in listeners {
                l.updater().renders_arrived();
            }
        }
    }

    fn evict(&self, limit: usize) {
        for key in self.queue.borrow_mut().evict(limit) {
            set_ready(&key, false);
        }
    }

    fn call<T: 'static>(
        &self,
        body: &str,
        args: &[(&str, ScriptValue)],
        fields: fn(&NSDictionary) -> Option<T>,
        done: impl Fn(Answer<T>) + 'static,
    ) {
        webkit::call_script(&self.web.borrow(), body, args, move |result| {
            done(Answer::read(result, fields));
        });
    }

    fn render_math(&self, key: Key, job: u64, tex: &str, display: bool, dark: bool) {
        let color = if dark { "#E8E8E8" } else { "#1D1D1F" };
        let args = [
            ("tex", ScriptValue::Text(tex)),
            ("display", ScriptValue::Flag(display)),
            ("color", ScriptValue::Text(color)),
        ];
        let label = tex.to_string();
        let call = "return await renderMath(tex, display, color);";
        self.call(call, &args, MathAnswer::read, move |answer| {
            let Some(r) = renderer_if_started().filter(|r| r.queue.borrow().answer_counts(job)) else {
                return;
            };
            if let Some(message) = answer.error {
                r.queue.borrow_mut().note_error(key.clone(), message);
            }
            let Some(MathAnswer { svg, w, h, va }) = answer.value else {
                r.finish(key.clone(), None);
                return;
            };
            // The next formula renders in WebKit while this one converts on the background thread.
            let free = {
                let mut queue = r.queue.borrow_mut();
                queue.conversion_started();
                queue.release(&key)
            };
            if free {
                r.pump();
            }
            let key = key.clone();
            let label = label.clone();
            background(move || {
                // The answer must arrive whatever happens, or the renderer never counts as idle again.
                let pixels = crate::util::contained(|| super::decoder::svg_to_pixels(&svg)).flatten();
                DispatchQueue::main().exec_async(move || {
                    let Some(r) = renderer_if_started() else { return };
                    r.queue.borrow_mut().conversion_ended();
                    let rendered = pixels
                        .and_then(|pixels| labeled_image(pixels, &crate::i18n::tf("Formel: {}", &[&label])))
                        .map(|image| Rendered {
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

    fn render_diagram(&self, key: Key, job: u64, source: &str, kind: DiagramKind, dark: bool) {
        let language = match kind {
            DiagramKind::Mermaid => "mermaid",
            DiagramKind::Graphviz => "graphviz",
            DiagramKind::PlantUml => "plantuml",
        };
        let args = [
            ("code", ScriptValue::Text(source)),
            ("language", ScriptValue::Text(language)),
            ("dark", ScriptValue::Flag(dark)),
        ];
        // Weak: WebKit keeps this handler until the script answers, and a view held by it could never close.
        let web = Weak::from_retained(&self.web.borrow());
        let label = source.lines().next().unwrap_or(crate::i18n::t("Diagramm")).to_string();
        self.call(
            "return await renderDiagram(language, code, dark);",
            &args,
            PlacedPicture::read,
            move |answer| {
                let Some(r) = renderer_if_started().filter(|r| r.queue.borrow().answer_counts(job)) else {
                    return;
                };
                if let Some(message) = answer.error {
                    r.queue.borrow_mut().note_error(key.clone(), message);
                }
                let (Some(PlacedPicture { x, y, w, h }), Some(web)) = (answer.value, web.load()) else {
                    r.finish(key.clone(), None);
                    return;
                };
                let key = key.clone();
                let label = label.clone();
                webkit::transparent_pdf(&web, NSRect::new(NSPoint::new(x, y), NSSize::new(w, h)), move |data| {
                    let pdf = data.map(|d| d.to_vec());
                    let key = key.clone();
                    let label = label.clone();
                    background(move || {
                        // The answer must arrive whatever happens, or the renderer never counts as idle again.
                        let pixels = crate::util::contained(|| pdf.and_then(|pdf| super::decoder::pdf_to_pixels(&pdf)))
                            .flatten();
                        DispatchQueue::main().exec_async(move || {
                            let rendered = pixels
                                .and_then(|pixels| labeled_image(pixels, &crate::i18n::tf("Diagramm: {}", &[&label])))
                                .map(|image| Rendered {
                                    image,
                                    sizing: Sizing::Points {
                                        size: NSSize::new(w, h),
                                        descent: 0.0,
                                    },
                                });
                            if let Some(r) = renderer_if_started().filter(|r| r.queue.borrow().answer_counts(job)) {
                                r.finish(key.clone(), rendered);
                            }
                        });
                    });
                });
            },
        );
    }
}

/// Runs CPU work off the main thread, one job after the other.
pub fn background(job: impl FnOnce() + Send + 'static) {
    crate::util::BACKGROUND.run(job);
}

/// What a function of render.js answered: the reason it gives or threw for a failure, and what it rendered.
#[derive(Debug, PartialEq)]
struct Answer<T> {
    error: Option<String>,
    value: Option<T>,
}

impl<T> Answer<T> {
    fn read(result: Result<Option<&AnyObject>, String>, fields: fn(&NSDictionary) -> Option<T>) -> Answer<T> {
        let answer = match result {
            Ok(answer) => answer.and_then(|o| o.downcast_ref::<NSDictionary>()),
            Err(thrown) => {
                return Answer {
                    error: Some(thrown),
                    value: None,
                };
            }
        };
        Answer {
            error: answer.and_then(|d| string(d, "error")).filter(|m| !m.is_empty()),
            value: answer.and_then(fields),
        }
    }
}

/// What `renderMath` draws.
#[derive(Debug, PartialEq)]
struct MathAnswer {
    svg: String,
    w: f64,
    h: f64,
    va: f64,
}

impl MathAnswer {
    fn read(d: &NSDictionary) -> Option<MathAnswer> {
        Some(MathAnswer {
            svg: string(d, "svg")?,
            w: number(d, "w")?,
            h: number(d, "h")?,
            va: number(d, "va").unwrap_or(0.0),
        })
    }
}

/// Where `renderDiagram` placed its picture on the page.
#[derive(Debug, PartialEq)]
struct PlacedPicture {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl PlacedPicture {
    fn read(d: &NSDictionary) -> Option<PlacedPicture> {
        let placed = PlacedPicture {
            x: number(d, "x")?,
            y: number(d, "y")?,
            w: number(d, "w")?,
            h: number(d, "h")?,
        };
        (placed.w > 0.0 && placed.h > 0.0).then_some(placed)
    }
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

/// The image of pixels from a helper, named for those who cannot see it.
fn labeled_image(pixels: Decoded, description: &str) -> Option<Retained<NSImage>> {
    let image = super::decoder::image(pixels)?;
    image.setAccessibilityDescription(Some(&ns(description)));
    Some(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::ns;

    fn answer(fields: &[(&str, &AnyObject)]) -> Retained<NSDictionary> {
        let keys: Vec<Retained<NSString>> = fields.iter().map(|(k, _)| ns(k)).collect();
        let key_refs: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
        let values: Vec<&AnyObject> = fields.iter().map(|(_, v)| *v).collect();
        let typed = NSDictionary::from_slices(&key_refs, &values);
        // SAFETY: Only the type of the keys is forgotten, which readers ask for again.
        unsafe { Retained::cast_unchecked(typed) }
    }

    fn num(n: f64) -> Retained<NSNumber> {
        NSNumber::new_f64(n)
    }

    #[test]
    fn an_answer_is_read_into_its_fields() {
        let (svg, w, h) = (ns("<svg/>"), num(4.0), num(2.0));
        let d = answer(&[("svg", &svg), ("w", &w), ("h", &h)]);
        let read = Answer::read(Ok(Some(&d)), MathAnswer::read);
        assert_eq!(
            read,
            Answer {
                error: None,
                value: Some(MathAnswer {
                    svg: "<svg/>".to_string(),
                    w: 4.0,
                    h: 2.0,
                    va: 0.0,
                }),
            }
        );
    }

    #[test]
    fn a_failure_is_a_reason_whether_returned_or_thrown() {
        let reason = ns("bad formula");
        let d = answer(&[("error", &reason)]);
        let returned = Answer::read(Ok(Some(&d)), MathAnswer::read);
        assert_eq!(returned.error.as_deref(), Some("bad formula"));
        assert!(returned.value.is_none());
        let thrown = Answer::read(Err("Type error".to_string()), MathAnswer::read);
        assert_eq!(thrown.error.as_deref(), Some("Type error"));
        assert!(thrown.value.is_none());
    }

    #[test]
    fn a_picture_without_a_size_is_none() {
        let (zero, one) = (num(0.0), num(1.0));
        let flat = answer(&[("x", &one), ("y", &one), ("w", &one), ("h", &zero)]);
        assert_eq!(Answer::read(Ok(Some(&flat)), PlacedPicture::read).value, None);
        let found = answer(&[("x", &one), ("y", &one), ("w", &one), ("h", &one)]);
        assert!(Answer::read(Ok(Some(&found)), PlacedPicture::read).value.is_some());
    }

    #[test]
    fn mathjax_like_svg_converts() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="40" height="20" viewBox="0 -750 1000 1000" color="#1D1D1F"><defs><path id="g" d="M10 10L500 10L500 -500Z"/></defs><g fill="currentColor"><use xlink:href="#g"/></g></svg>"##;
        let pdf =
            super::super::safe_image::svg_to_pdf(svg, std::sync::Arc::new(svg2pdf::usvg::fontdb::Database::new()))
                .expect("pdf");
        assert!(pdf.starts_with(b"%PDF"));
    }
}
