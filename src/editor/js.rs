//! Renders formulas (MathJax) and diagrams (Mermaid) in a hidden WebKit view.

use std::cell::{OnceCell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSBackingStoreType, NSImage, NSPanel, NSWindowStyleMask};
use objc2_foundation::{NSDictionary, NSNumber, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use objc2_web_kit::{WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKWebView};

use super::EditorController;
use super::picture::{Rendered, Sizing, allowed_side, count_decoded, room_for_more};
use super::render_queue::{Owner, RenderQueue};
use super::safe_image::Decoded;
use super::webkit::{self, ScriptValue};
use crate::markdown::doc::DiagramKind;
use crate::util::{guarded, main_after, main_soon, ns, to_isize};

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    Math {
        tex: String,
        display: bool,
        dark: bool,
        /// The definitions of the document it stands in: they decide what it shows.
        macros: Arc<str>,
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
    listeners: RefCell<Vec<(Owner, Weak<EditorController>)>>,
}

/// Beyond this many rendered formulas and diagrams, or this many bytes of their pictures, those no open document asked for
/// lately go. What is in use stays, and `room_for_more` holds that within the memory all pictures may take.
const CACHE_LIMIT: usize = 1500;
const CACHE_BYTES: usize = 256 << 20;

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
            guarded((), || {
                if let Some(r) = renderer_if_started().filter(|r| r.is_current(web)) {
                    r.queue.borrow_mut().page_ready();
                    r.pump();
                }
            });
        }

        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide(
            &self,
            _web: &WKWebView,
            action: &WKNavigationAction,
            handler: &block2::DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            handler.call((guarded(WKNavigationActionPolicy::Cancel, || {
                webkit::navigation_policy(action, PAGE)
            }),));
        }

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn terminated(&self, web: &WKWebView) {
            guarded((), || {
                if let Some(r) = renderer_if_started().filter(|r| r.is_current(web)) {
                    r.restart();
                }
            });
        }
    }
);

const PAGE: &str = "render.html";

/// The view that renders, sized for the renderer's window, still without its page.
fn web_view(window: &NSPanel, delegate: &NavDelegate) -> Retained<WKWebView> {
    let mtm = window.mtm();
    let web = webkit::new_view(
        mtm,
        NSRect::new(NSPoint::new(0.0, 0.0), window.frame().size),
        &webkit::configuration(mtm),
    );
    // SAFETY: WebKit holds the delegate weakly; the renderer keeps it alive next to the web view.
    unsafe { web.setNavigationDelegate(Some(objc2::runtime::ProtocolObject::from_ref(delegate))) };
    web
}

fn load_page(window: &NSPanel, web: &WKWebView) {
    window.setContentView(Some(web));
    webkit::load_bundled_page(web, PAGE);
}

/// Between creating the first web view and loading its page, so that a key pressed meanwhile gets through; a block
/// dispatched without a delay would run ahead of it.
const TURN_FOR_KEYS: std::time::Duration = std::time::Duration::from_millis(16);

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
            #[cfg(feature = "selftest")]
            HAD_ITS_PAGE_AT_ONCE.set(renderer.web.borrow().window().is_some());
            renderer.watch_load();
            main_after(TURN_FOR_KEYS, || {
                if let Some(r) = renderer_if_started() {
                    load_page(&r.window, &r.web.borrow());
                }
            });
            renderer
        })
        .clone()
    })
}

#[cfg(feature = "selftest")]
thread_local! {
    static HAD_ITS_PAGE_AT_ONCE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(feature = "selftest")]
pub(super) fn had_its_page_at_once() -> bool {
    HAD_ITS_PAGE_AT_ONCE.get()
}

#[cfg(feature = "selftest")]
pub(super) fn started() -> bool {
    renderer_if_started().is_some()
}

#[cfg(feature = "selftest")]
pub(super) fn idle() -> bool {
    renderer_if_started().is_none_or(|r| r.idle())
}

#[cfg(feature = "selftest")]
impl Renderer {
    /// The page itself, for what the selftest tries against it.
    pub(super) fn web(&self) -> Retained<WKWebView> {
        self.web.borrow().clone()
    }
}

/// The number a document's requests go by.
fn owner_of(controller: &EditorController) -> Owner {
    std::ptr::from_ref(controller) as usize
}

/// A document has closed.
pub(super) fn forget(controller: &EditorController) {
    if let Some(r) = renderer_if_started() {
        r.forget(owner_of(controller));
    }
}

/// A document that shows no formula or diagram any more, which is no reason to start the renderer.
pub(super) fn asks_for_nothing(controller: &EditorController) {
    if let Some(r) = renderer_if_started() {
        r.request(owner_of(controller), Vec::new());
    }
}

/// Frees what the shared renderer holds for documents that no longer ask for it.
pub fn trim() {
    if let Some(r) = renderer_if_started() {
        r.evict(0, 0);
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
    pub fn math(tex: &str, display: bool, dark: bool, macros: &Arc<str>) -> Key {
        Key::Math {
            tex: tex.to_string(),
            display,
            dark,
            macros: macros.clone(),
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

    /// `controller` hears when renders arrive; what it asks for goes by the number this returns.
    pub fn listen(&self, controller: &EditorController) -> Owner {
        let owner = owner_of(controller);
        let gone: Vec<Owner> = {
            let mut listeners = self.listeners.borrow_mut();
            let gone = listeners
                .iter()
                .filter(|(_, w)| w.load().is_none())
                .map(|(owner, _)| *owner)
                .collect();
            listeners.retain(|(_, w)| w.load().is_some());
            if !listeners.iter().any(|(known, _)| *known == owner) {
                listeners.push((owner, Weak::from(controller)));
            }
            gone
        };
        for owner in gone {
            self.queue.borrow_mut().forget(owner);
        }
        owner
    }

    /// All that `owner` needs now, in the order it needs it.
    pub fn request(self: &Rc<Self>, owner: Owner, keys: Vec<Key>) {
        self.queue.borrow_mut().request(owner, keys);
        self.evict(CACHE_LIMIT, CACHE_BYTES);
        self.pump();
    }

    fn pump(&self) {
        let Some((key, job)) = self.queue.borrow_mut().next() else {
            return;
        };
        main_after(RENDER_TIMEOUT, move || {
            if let Some(r) = renderer_if_started() {
                r.give_up(job);
            }
        });
        match &key {
            Key::Math {
                tex,
                display,
                dark,
                macros,
            } => self.render_math(key.clone(), job, tex, *display, *dark, macros),
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
        let web = web_view(&self.window, &self._delegate);
        load_page(&self.window, &web);
        *self.web.borrow_mut() = web;
    }

    /// WebKit reports a page that never finishes loading to nobody, and queued formulas would wait forever.
    fn watch_load(&self) {
        let job = self.queue.borrow_mut().load_started();
        main_after(RENDER_TIMEOUT, move || {
            if let Some(r) = renderer_if_started()
                && r.queue.borrow().load_timed_out(job)
            {
                r.replace_view();
                r.watch_load();
            }
        });
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
            let listeners: Vec<_> = self.listeners.borrow().iter().filter_map(|(_, w)| w.load()).collect();
            for l in listeners {
                l.renders_arrived();
            }
        }
    }

    fn evict(&self, limit: usize, budget: usize) {
        let (gone, freed) = {
            let mut queue = self.queue.borrow_mut();
            let before = queue.bytes();
            let gone = queue.evict(limit, budget);
            (gone, before - queue.bytes())
        };
        count_decoded(-to_isize(freed));
        for key in gone {
            set_ready(&key, false);
        }
    }

    /// A document has closed: what only it needed stops rendering and lets go of its memory.
    fn forget(&self, owner: Owner) {
        self.listeners.borrow_mut().retain(|(known, _)| *known != owner);
        self.queue.borrow_mut().forget(owner);
        self.evict(0, 0);
        self.notify_if_idle();
    }

    /// Calls `function` of render.js with `args`, in the order it takes them.
    fn call<T: PageAnswer + 'static>(
        &self,
        function: &str,
        args: &[(&str, ScriptValue)],
        done: impl Fn(Answer<T>) + 'static,
    ) {
        let names: Vec<&str> = args.iter().map(|(name, _)| *name).collect();
        let body = format!("return await {function}({});", names.join(", "));
        webkit::call_script(&self.web.borrow(), &body, args, move |result| {
            done(Answer::read(result));
        });
    }

    fn render_math(&self, key: Key, job: u64, tex: &str, display: bool, dark: bool, macros: &str) {
        let color = if dark { "#E8E8E8" } else { "#1D1D1F" };
        let args = [
            ("tex", ScriptValue::Text(tex)),
            ("display", ScriptValue::Flag(display)),
            ("color", ScriptValue::Text(color)),
            ("macros", ScriptValue::Text(macros)),
        ];
        let label = tex.to_string();
        self.call("renderMath", &args, move |answer: Answer<MathAnswer>| {
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
            let sizing = Sizing::Ex {
                width: w,
                height: h,
                valign: va,
            };
            let description = crate::i18n::tf("Formel: {}", &[&label]);
            r.convert(key.clone(), description, sizing, move || {
                super::decoder::svg_to_pixels(&svg, allowed_side())
            });
        });
    }

    /// Turns what the page drew into a picture on another thread, while the page goes on with the next. The time limit of a
    /// render is about the page, which is done with it here: a queue of pictures to convert must not count against it.
    fn convert(
        &self,
        key: Key,
        description: String,
        sizing: Sizing,
        pixels: impl FnOnce() -> Option<Decoded> + Send + 'static,
    ) {
        let free = {
            let mut queue = self.queue.borrow_mut();
            queue.conversion_started();
            queue.release(&key)
        };
        if free {
            self.pump();
        }
        background(move || {
            // The answer must arrive whatever happens, or the renderer never counts as idle again.
            let pixels = room_for_more()
                .then(|| crate::util::contained(pixels).flatten())
                .flatten();
            main_soon(move || {
                let Some(r) = renderer_if_started() else { return };
                r.queue.borrow_mut().conversion_ended();
                let bytes = pixels.as_ref().map_or(0, Decoded::bytes);
                let rendered = pixels
                    .and_then(|pixels| labeled_image(pixels, &description))
                    .map(|image| Rendered { image, sizing });
                let before = r.queue.borrow().bytes();
                r.queue.borrow_mut().note_size(key.clone(), bytes);
                count_decoded(to_isize(r.queue.borrow().bytes()) - to_isize(before));
                r.store(key, rendered);
                r.notify_if_idle();
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
            ("language", ScriptValue::Text(language)),
            ("code", ScriptValue::Text(source)),
            ("dark", ScriptValue::Flag(dark)),
        ];
        // Weak: WebKit keeps this handler until the script answers, and a view held by it could never close.
        let web = Weak::from_retained(&self.web.borrow());
        let label = source.lines().next().unwrap_or(crate::i18n::t("Diagramm")).to_string();
        self.call("renderDiagram", &args, move |answer: Answer<PlacedPicture>| {
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
                let Some(r) = renderer_if_started().filter(|r| r.queue.borrow().answer_counts(job)) else {
                    return;
                };
                let pdf = data.map(|d| d.to_vec());
                let sizing = Sizing::Points {
                    size: NSSize::new(w, h),
                    descent: 0.0,
                };
                let description = crate::i18n::tf("Diagramm: {}", &[&label]);
                r.convert(key.clone(), description, sizing, move || {
                    super::decoder::pdf_to_pixels(&pdf?, allowed_side())
                });
            });
        });
    }
}

/// Runs CPU work off the main thread.
pub fn background(job: impl FnOnce() + Send + 'static) {
    crate::util::BACKGROUND.run(job);
}

/// What a function of render.js answers. `FIELDS` is what the selftest holds that function to, in a real page.
pub(super) trait PageAnswer: Sized {
    #[cfg(feature = "selftest")]
    const FIELDS: &'static [&'static str];
    fn read(fields: &Fields) -> Option<Self>;
}

/// The object a function of render.js returned.
pub(super) struct Fields<'a>(&'a NSDictionary);

impl Fields<'_> {
    fn text(&self, key: &str) -> Option<String> {
        let value = self.0.objectForKey(&ns(key))?;
        value.downcast_ref::<NSString>().map(|s| s.to_string())
    }

    fn number(&self, key: &str) -> Option<f64> {
        let value = self.0.objectForKey(&ns(key))?;
        value.downcast_ref::<NSNumber>().map(|n| n.doubleValue())
    }
}

/// What a function of render.js answered: the reason it gives or threw for a failure, and what it rendered.
#[derive(Debug, PartialEq)]
struct Answer<T> {
    error: Option<String>,
    value: Option<T>,
}

impl<T: PageAnswer> Answer<T> {
    fn read(result: Result<Option<&AnyObject>, String>) -> Answer<T> {
        let answer = match result {
            Ok(answer) => answer.and_then(|o| o.downcast_ref::<NSDictionary>()).map(Fields),
            Err(thrown) => {
                return Answer {
                    error: Some(thrown),
                    value: None,
                };
            }
        };
        Answer {
            error: answer.as_ref().and_then(|f| f.text("error")).filter(|m| !m.is_empty()),
            value: answer.as_ref().and_then(T::read),
        }
    }
}

/// What `renderMath` draws.
#[derive(Debug, PartialEq)]
pub(super) struct MathAnswer {
    svg: String,
    w: f64,
    h: f64,
    va: f64,
}

impl PageAnswer for MathAnswer {
    #[cfg(feature = "selftest")]
    const FIELDS: &'static [&'static str] = &["error", "h", "svg", "va", "w"];

    fn read(fields: &Fields) -> Option<MathAnswer> {
        Some(MathAnswer {
            svg: fields.text("svg")?,
            w: fields.number("w")?,
            h: fields.number("h")?,
            va: fields.number("va").unwrap_or(0.0),
        })
    }
}

/// Where `renderDiagram` placed its picture on the page.
#[derive(Debug, PartialEq)]
pub(super) struct PlacedPicture {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl PageAnswer for PlacedPicture {
    #[cfg(feature = "selftest")]
    const FIELDS: &'static [&'static str] = &["h", "w", "x", "y"];

    fn read(fields: &Fields) -> Option<PlacedPicture> {
        let placed = PlacedPicture {
            x: fields.number("x")?,
            y: fields.number("y")?,
            w: fields.number("w")?,
            h: fields.number("h")?,
        };
        (placed.w > 0.0 && placed.h > 0.0).then_some(placed)
    }
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
        let read: Answer<MathAnswer> = Answer::read(Ok(Some(&d)));
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
        let returned: Answer<MathAnswer> = Answer::read(Ok(Some(&d)));
        assert_eq!(returned.error.as_deref(), Some("bad formula"));
        assert!(returned.value.is_none());
        let thrown: Answer<MathAnswer> = Answer::read(Err("Type error".to_string()));
        assert_eq!(thrown.error.as_deref(), Some("Type error"));
        assert!(thrown.value.is_none());
    }

    #[test]
    fn a_picture_without_a_size_is_none() {
        let (zero, one) = (num(0.0), num(1.0));
        let flat = answer(&[("x", &one), ("y", &one), ("w", &one), ("h", &zero)]);
        assert_eq!(Answer::<PlacedPicture>::read(Ok(Some(&flat))).value, None);
        let found = answer(&[("x", &one), ("y", &one), ("w", &one), ("h", &one)]);
        assert!(Answer::<PlacedPicture>::read(Ok(Some(&found))).value.is_some());
    }

    #[test]
    fn mathjax_like_svg_converts() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="40" height="20" viewBox="0 -750 1000 1000" color="#1D1D1F"><defs><path id="g" d="M10 10L500 10L500 -500Z"/></defs><g fill="currentColor"><use xlink:href="#g"/></g></svg>"##;
        let pdf =
            super::super::safe_image::svg_to_pdf(svg, std::sync::Arc::new(usvg::fontdb::Database::new())).expect("pdf");
        assert!(pdf.starts_with(b"%PDF"));
    }
}
