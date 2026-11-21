//! Renders formulas (MathJax) and diagrams (Mermaid) in a hidden WebKit view.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSBackingStoreType, NSImage, NSPanel, NSWindowStyleMask};
use objc2_foundation::{NSDictionary, NSNumber, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use objc2_web_kit::{WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKWebView};

use splat_decoder::Decoded;
pub use splat_editor_state::render_queue::Owner;
use splat_editor_state::render_queue::RenderQueue;
use splat_markdown::doc::{DiagramKind, Doc, Figure, figures, macros_of};

use crate::appkit::{ns, to_isize};
use crate::lanes::{guarded, main_after, main_soon};
use crate::picture::{Rendered, Sizing, allowed_side, count_decoded, room_for_more};
use crate::viewer::{Held, Viewer};
use crate::webkit::{self, ScriptValue};

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
    mtm: MainThreadMarker,
    /// The web view and its window, from the first render that needs them until `RENDERER_IDLE` without one.
    page: RefCell<Option<Page>>,
    queue: RefCell<RenderQueue<Key, Rendered>>,
    listeners: RefCell<Vec<(Owner, Held)>>,
    /// When the page started or was last given something to render.
    last_use: Cell<Instant>,
    /// The number of the timer that may end the page; one that finds another number has been replaced.
    watch: Cell<u64>,
}

/// The hidden window, its web view and the delegate that hears from it, which end together.
struct Page {
    window: Retained<NSPanel>,
    web: Retained<WKWebView>,
    delegate: Retained<NavDelegate>,
}

/// Of the formulas and diagrams no open document wants, such as each half-typed formula, at most this many, holding at
/// most this many bytes of pictures, stay for a document that asks again; the least recently asked for go first. What
/// a document wants stays, and `room_for_more` holds that within the memory all pictures may take.
const CACHE_LIMIT: usize = 200;
const CACHE_BYTES: usize = 32 << 20;

/// After this long without a render, the web view, its window and its web process end; what they rendered stays.
const RENDERER_IDLE: Duration = Duration::from_secs(120);

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
    // SAFETY: WebKit holds the delegate weakly; the page keeps it alive next to the web view.
    unsafe { web.setNavigationDelegate(Some(objc2::runtime::ProtocolObject::from_ref(delegate))) };
    web
}

fn load_page(window: &NSPanel, web: &WKWebView) {
    window.setContentView(Some(web));
    webkit::load_bundled_page(web, PAGE);
}

/// Between creating a web view and loading its page, so that a key pressed meanwhile gets through; a block dispatched
/// without a delay would run ahead of it.
const TURN_FOR_KEYS: Duration = Duration::from_millis(16);

/// A render that has not answered by then counts as failed, and a new web view takes over.
pub const RENDER_TIMEOUT: Duration = Duration::from_secs(10);

fn renderer_if_started() -> Option<Rc<Renderer>> {
    RENDERER.with(|r| r.get().cloned())
}

/// The shared renderer, created on first use and kept with what it rendered for the lifetime of the app. Its web view,
/// whose processes take about 150 MB, starts with the first render that needs it and ends after `RENDERER_IDLE` without
/// one.
pub fn renderer(mtm: MainThreadMarker) -> Rc<Renderer> {
    RENDERER.with(|cell| cell.get_or_init(|| Rc::new(Renderer::new(mtm))).clone())
}

#[cfg(feature = "selftest")]
thread_local! {
    static HAD_ITS_PAGE_AT_ONCE: Cell<bool> = const { Cell::new(false) };
    /// What a check set in place of `RENDERER_IDLE`.
    static IDLE_FOR_CHECKS: Cell<Option<Duration>> = const { Cell::new(None) };
}

#[cfg(feature = "selftest")]
pub fn had_its_page_at_once() -> bool {
    HAD_ITS_PAGE_AT_ONCE.get()
}

/// Whether the web view runs.
#[cfg(feature = "selftest")]
pub fn started() -> bool {
    renderer_if_started().is_some_and(|r| r.page.borrow().is_some())
}

#[cfg(feature = "selftest")]
pub fn idle() -> bool {
    renderer_if_started().is_none_or(|r| r.idle())
}

/// Ends the web view after `time` without a render instead of `RENDERER_IDLE`, or after that again with `None`.
#[cfg(feature = "selftest")]
pub fn end_unused_after(time: Option<Duration>) {
    IDLE_FOR_CHECKS.set(time);
    if let Some(r) = renderer_if_started() {
        r.watch_idle(Duration::ZERO);
    }
}

fn idle_time() -> Duration {
    #[cfg(feature = "selftest")]
    if let Some(time) = IDLE_FOR_CHECKS.get() {
        return time;
    }
    RENDERER_IDLE
}

#[cfg(feature = "selftest")]
impl Renderer {
    /// The page itself, for what the selftest tries against it, started if it does not run; a try counts as a render.
    pub fn web(&self) -> Retained<WKWebView> {
        self.used();
        let running = self.running().map(|(_, web)| web);
        running.unwrap_or_else(|| self.start_page())
    }
}

/// A document has closed.
pub fn forget(viewer: &Rc<dyn Viewer>) {
    if let Some(r) = renderer_if_started() {
        let owner = r.owner_of(viewer);
        r.forget(owner);
    }
}

/// A document that shows no formula or diagram any more, which is no reason to start the renderer.
pub fn asks_for_nothing(viewer: &Rc<dyn Viewer>) {
    if let Some(r) = renderer_if_started()
        && let Some(owner) = r.owner_of(viewer)
    {
        r.request(owner, Vec::new());
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

/// Marks `key` as rendered or not without rendering it, as a window self-test does to see what an arrival changes.
#[cfg(feature = "selftest")]
pub fn set_ready(key: &Key, on: bool) {
    mark_ready(key, on);
}

fn mark_ready(key: &Key, on: bool) {
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
    Some(outcome.map_err(|message| message.unwrap_or_else(|| splat_i18n::t("Darstellung fehlgeschlagen").to_string())))
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

    /// The key of a formula or diagram of a document, for one appearance.
    pub fn of(figure: &Figure, dark: bool, macros: &Arc<str>) -> Key {
        match figure {
            Figure::Math { tex, display } => Key::math(tex, *display, dark, macros),
            Figure::Diagram { source, kind } => Key::diagram(source, *kind, dark),
        }
    }
}

/// Beyond this many formulas and diagrams, a document renders only for the appearance it shows.
const RENDERED_AHEAD: usize = 200;

/// What a document needs rendered, in the order it needs it, and the definitions its formulas share.
pub struct Wanted {
    pub keys: Vec<Key>,
    pub macros: Arc<str>,
}

/// The formulas and diagrams of `doc` for its appearance, then for the other one: rendering ahead makes switching
/// instant, but not at the price of doubling the work for documents full of formulas.
pub fn wanted(doc: &Doc, dark: bool) -> Wanted {
    let all = figures(doc);
    let macros: Arc<str> = macros_of(all.iter().map(|(_, f)| f)).into();
    let (mut keys, other): (Vec<_>, Vec<_>) = all
        .iter()
        .map(|(_, f)| (Key::of(f, dark, &macros), Key::of(f, !dark, &macros)))
        .unzip();
    if keys.len() <= RENDERED_AHEAD {
        keys.extend(other);
    }
    Wanted { keys, macros }
}

/// A layout's answer to whether a formula or diagram shows as a picture: once it is rendered. Everything else a
/// resolver may know stays at its default.
pub struct Ready {
    pub dark: bool,
    pub macros: Arc<str>,
}

impl splat_preview::layout::Resolver for Ready {
    fn math_ready(&self, tex: &str, display: bool) -> bool {
        ready(&Key::math(tex, display, self.dark, &self.macros))
    }

    fn diagram_ready(&self, source: &str, kind: DiagramKind) -> bool {
        ready(&Key::diagram(source, kind, self.dark))
    }
}

impl Page {
    fn new(mtm: MainThreadMarker) -> Page {
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
        Page { window, web, delegate }
    }
}

impl Renderer {
    fn new(mtm: MainThreadMarker) -> Renderer {
        Renderer {
            mtm,
            page: RefCell::new(None),
            queue: RefCell::default(),
            listeners: RefCell::new(Vec::new()),
            last_use: Cell::new(Instant::now()),
            watch: Cell::new(0),
        }
    }

    /// The window and the web view, while the page runs.
    fn running(&self) -> Option<(Retained<NSPanel>, Retained<WKWebView>)> {
        self.page
            .borrow()
            .as_ref()
            .map(|page| (page.window.clone(), page.web.clone()))
    }

    /// Starts the page as the first render needs it: the web view now, its page in a later turn of the main loop.
    fn start_page(&self) -> Retained<WKWebView> {
        let page = Page::new(self.mtm);
        let web = page.web.clone();
        #[cfg(feature = "selftest")]
        HAD_ITS_PAGE_AT_ONCE.set(web.window().is_some());
        *self.page.borrow_mut() = Some(page);
        self.used();
        self.watch_idle(idle_time());
        self.watch_load();
        main_after(TURN_FOR_KEYS, || {
            if let Some((window, web)) = renderer_if_started().and_then(|r| r.running()) {
                load_page(&window, &web);
            }
        });
        web
    }

    fn used(&self) {
        self.last_use.set(Instant::now());
    }

    /// Looks after `wait` whether the page can end; a look planned before is dropped.
    fn watch_idle(&self, wait: Duration) {
        let watch = self.watch.get() + 1;
        self.watch.set(watch);
        main_after(wait, move || {
            if let Some(r) = renderer_if_started()
                && r.watch.get() == watch
            {
                r.end_if_unused();
            }
        });
    }

    /// Ends the page once it has had nothing to do for `idle_time()`, and otherwise looks again when that may be so.
    fn end_if_unused(&self) {
        if self.page.borrow().is_none() {
            return;
        }
        let (idle, quiet) = (idle_time(), self.last_use.get().elapsed());
        if !self.idle() {
            self.watch_idle(idle);
        } else if quiet >= idle {
            self.end_page();
        } else {
            self.watch_idle(idle.saturating_sub(quiet));
        }
    }

    /// Ends the web view, its window and its web process. What they rendered stays, a late answer of theirs no longer
    /// counts, and the next render starts a new page.
    fn end_page(&self) {
        let Some(page) = self.page.borrow_mut().take() else {
            return;
        };
        self.queue.borrow_mut().lost_page();
        webkit::detach_delegate(&page.web);
        page.window.setContentView(None);
        page.window.close();
    }

    /// `viewer` hears when renders arrive; what it asks for goes by the owner this returns.
    pub fn listen(&self, viewer: &Rc<dyn Viewer>) -> Owner {
        let (owner, gone) = {
            let mut listeners = self.listeners.borrow_mut();
            let gone: Vec<Owner> = listeners
                .iter()
                .filter(|(_, w)| w.load().is_none())
                .map(|(owner, _)| *owner)
                .collect();
            listeners.retain(|(_, w)| w.load().is_some());
            let known = listeners.iter().find(|(_, w)| w.is(viewer)).map(|(owner, _)| *owner);
            let owner = known.unwrap_or_else(|| {
                let owner = Owner::unique();
                listeners.push((owner, Held::of(viewer)));
                owner
            });
            (owner, gone)
        };
        for owner in gone {
            self.queue.borrow_mut().forget(owner);
        }
        owner
    }

    /// The owner `viewer` asks as, while it listens.
    fn owner_of(&self, viewer: &Rc<dyn Viewer>) -> Option<Owner> {
        self.listeners
            .borrow()
            .iter()
            .find(|(_, w)| w.is(viewer))
            .map(|(owner, _)| *owner)
    }

    /// All that `owner` needs now, in the order it needs it.
    pub fn request(self: &Rc<Self>, owner: Owner, keys: Vec<Key>) {
        self.queue.borrow_mut().request(owner, keys);
        self.evict(CACHE_LIMIT, CACHE_BYTES);
        self.pump();
    }

    fn pump(&self) {
        if self.page.borrow().is_none() {
            if self.queue.borrow().waits() {
                self.start_page();
            }
            return;
        }
        let Some((key, job)) = self.queue.borrow_mut().next() else {
            return;
        };
        self.used();
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
        self.used();
        let current = self.queue.borrow_mut().release(&key);
        self.store(key, result);
        self.notify_if_idle();
        if current {
            self.pump();
        }
    }

    /// Ends render attempt `job` if it is still running: it failed, and a new view takes over. A script that never
    /// returns keeps its web process busy until Splat quits: that process neither loads the page again nor quits with its
    /// view, and WebKit has no public way to end it.
    fn give_up(&self, job: u64) {
        if self.queue.borrow().timed_out(job) {
            self.replace_view();
            self.lost_page();
            self.watch_load();
        }
    }

    /// A new web view takes over from one that does not answer.
    fn replace_view(&self) {
        let Some((window, stuck, delegate)) = self
            .page
            .borrow()
            .as_ref()
            .map(|page| (page.window.clone(), page.web.clone(), page.delegate.clone()))
        else {
            return;
        };
        webkit::detach_delegate(&stuck);
        let web = web_view(&window, &delegate);
        load_page(&window, &web);
        if let Some(page) = self.page.borrow_mut().as_mut() {
            page.web = web;
        }
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
        self.page
            .borrow()
            .as_ref()
            .is_some_and(|page| std::ptr::eq(web, &*page.web))
    }

    /// The web process ended: the page loads again in a new one.
    fn restart(&self) {
        self.lost_page();
        if let Some((_, web)) = self.running() {
            webkit::load_bundled_page(&web, PAGE);
        }
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
            mark_ready(&key_for_layout, true);
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
            mark_ready(&key, false);
        }
    }

    /// A document has closed: what only it needed stops rendering and lets go of its memory.
    fn forget(&self, owner: Option<Owner>) {
        if let Some(owner) = owner {
            self.listeners.borrow_mut().retain(|(known, _)| *known != owner);
            self.queue.borrow_mut().forget(owner);
        }
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
        // `pump` hands out keys only while the page runs.
        let Some((_, web)) = self.running() else {
            return;
        };
        let names: Vec<&str> = args.iter().map(|(name, _)| *name).collect();
        let body = format!("return await {function}({});", names.join(", "));
        webkit::call_script(&web, &body, args, move |result| {
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
            let description = splat_i18n::tf("Formel: {}", &[&label]);
            r.convert(key.clone(), description, sizing, move || {
                splat_decoder::svg_to_pixels(&svg, allowed_side())
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
                .then(|| crate::lanes::contained(pixels).flatten())
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
        let web = self.running().map(|(_, web)| Weak::from_retained(&web));
        let label = source.lines().next().unwrap_or(splat_i18n::t("Diagramm")).to_string();
        self.call("renderDiagram", &args, move |answer: Answer<PlacedPicture>| {
            let Some(r) = renderer_if_started().filter(|r| r.queue.borrow().answer_counts(job)) else {
                return;
            };
            if let Some(message) = answer.error {
                r.queue.borrow_mut().note_error(key.clone(), message);
            }
            let (Some(PlacedPicture { x, y, w, h }), Some(web)) = (answer.value, web.as_ref().and_then(Weak::load))
            else {
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
                let description = splat_i18n::tf("Diagramm: {}", &[&label]);
                r.convert(key.clone(), description, sizing, move || {
                    splat_decoder::pdf_to_pixels(&pdf?, allowed_side())
                });
            });
        });
    }
}

/// Runs CPU work off the main thread.
pub fn background(job: impl FnOnce() + Send + 'static) {
    crate::lanes::BACKGROUND.run(job);
}

/// What a function of render.js answers. `FIELDS` is what the selftest holds that function to, in a real page.
pub trait PageAnswer: Sized {
    #[cfg(feature = "selftest")]
    const FIELDS: &'static [&'static str];
    fn read(fields: &Fields) -> Option<Self>;
}

/// The object a function of render.js returned.
pub struct Fields<'a>(&'a NSDictionary);

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
pub struct MathAnswer {
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
pub struct PlacedPicture {
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
    let image = splat_decoder::image(pixels)?;
    image.setAccessibilityDescription(Some(&ns(description)));
    Some(image)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn no_two_documents_ask_as_the_same_owner() {
        let owners: HashSet<Owner> = (0..1000).map(|_| Owner::unique()).collect();
        assert_eq!(owners.len(), 1000);
    }
}
