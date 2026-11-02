//! Renders formulas (MathJax) and diagrams (Mermaid) in a hidden WebKit view.

use std::cell::{OnceCell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::{Retained, Weak};
use objc2::runtime::NSObject;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSBackingStoreType, NSImage, NSPanel, NSWindowStyleMask};
use objc2_foundation::{NSDictionary, NSNumber, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use objc2_web_kit::{WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKWebView};

use super::EditorController;
use super::images::{Rendered, Sizing};
use super::render_queue::RenderQueue;
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
const RENDER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

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
                l.renders_arrived();
            }
        }
    }

    fn evict(&self, limit: usize) {
        for key in self.queue.borrow_mut().evict(limit) {
            set_ready(&key, false);
        }
    }

    fn call(&self, body: &str, args: &[(&str, ScriptValue)], done: impl Fn(Option<&NSDictionary>) + 'static) {
        webkit::call_script(&self.web.borrow(), body, args, move |result| {
            done(result.and_then(|o| o.downcast_ref::<NSDictionary>()));
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
        self.call("return await renderMath(tex, display, color);", &args, move |dict| {
            let Some(r) = renderer_if_started().filter(|r| r.queue.borrow().answer_counts(job)) else {
                return;
            };
            if let Some(message) = dict.and_then(|d| string(d, "error")).filter(|m| !m.is_empty()) {
                r.queue.borrow_mut().note_error(key.clone(), message);
            }
            let answer = dict.and_then(|d| {
                let svg = string(d, "svg")?;
                Some((svg, number(d, "w")?, number(d, "h")?, number(d, "va").unwrap_or(0.0)))
            });
            let Some((svg, w, h, va)) = answer else {
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
                let pdf = std::panic::catch_unwind(|| svg_to_pdf(&svg)).ok().flatten();
                DispatchQueue::main().exec_async(move || {
                    let Some(r) = renderer_if_started() else { return };
                    r.queue.borrow_mut().conversion_ended();
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
            move |dict| {
                let Some(r) = renderer_if_started().filter(|r| r.queue.borrow().answer_counts(job)) else {
                    return;
                };
                if let Some(message) = dict.and_then(|d| string(d, "error")) {
                    r.queue.borrow_mut().note_error(key.clone(), message);
                }
                let rect =
                    dict.and_then(|d| Some((number(d, "x")?, number(d, "y")?, number(d, "w")?, number(d, "h")?)));
                let (Some((x, y, w, h)), Some(web)) = (rect.filter(|r| r.2 > 0.0 && r.3 > 0.0), web.load()) else {
                    r.finish(key.clone(), None);
                    return;
                };
                let key = key.clone();
                let label = label.clone();
                webkit::transparent_pdf(&web, NSRect::new(NSPoint::new(x, y), NSSize::new(w, h)), move |data| {
                    let rendered = data.and_then(|d| {
                        let image = super::decoder::system_image(&d.to_vec())?;
                        image.setAccessibilityDescription(Some(&ns(&crate::i18n::tf("Diagramm: {}", &[&label]))));
                        Some(Rendered {
                            image,
                            sizing: Sizing::Points {
                                size: NSSize::new(w, h),
                                descent: 0.0,
                            },
                        })
                    });
                    if let Some(r) = renderer_if_started().filter(|r| r.queue.borrow().answer_counts(job)) {
                        r.finish(key.clone(), rendered);
                    }
                });
            },
        );
    }
}

/// Runs CPU work off the main thread, one job after the other.
pub fn background(job: impl FnOnce() + Send + 'static) {
    crate::util::BACKGROUND.run(job);
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

/// MathJax output is plain SVG, so it converts to a vector PDF without WebKit; the SVG still derives from the document,
/// so the conversion runs in a decoder helper.
fn svg_to_pdf(svg: &str) -> Option<Vec<u8>> {
    super::decoder::svg_to_pdf(svg)
}

fn pdf_image(pdf: &[u8], label: &str) -> Option<Retained<NSImage>> {
    let image = super::decoder::system_image(pdf)?;
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
