//! Bringing the source colors and the preview in line with the source text: parsing, in the background while the
//! source has the focus, and replacing the preview blocks that changed. What is known after each step lies in `state`.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use objc2::rc::Weak;
use objc2::{MainThreadMarker, Message, sel};
use objc2_app_kit::NSTextStorage;
use objc2_foundation::NSRange;

use super::attributes::{attributed, set_typing_attributes};
use super::layers::ForUpdater;
use super::panes::Views;
use super::render::{Live, RenderCtx};
use super::state::{
    Background, Invalidate, PreviewChange, PreviewMode, Recolor, Update, ViewFacts, images_of, laid_out,
};
use super::style::{self, SourceFonts};
use super::{js, string_of};
use crate::markdown::commands::Edit;
use crate::markdown::doc::{DiagramKind, Figure, figures, macros_of};
use crate::markdown::incremental::{Parsed, parse_shared_and_color};
use crate::markdown::text::TextAt;
use crate::preview::layout::{Layout, Resolver, TopBlock};
use crate::preview::map::Index;
use crate::util::{
    Lane, contained, free_in_background, from_nsrange, insert_text, ns, perform_later, shown_to_ns, storage, to_nsrange,
};

/// Up to this many changed blocks render at once; more show as plain text first and render while idle.
pub(super) const PROGRESSIVE_FROM: usize = 300;
/// Blocks rendered right away around the visible part when rendering progressively.
const EAGER_BLOCKS: usize = 120;
/// A whole new text of this many UTF-16 units or more is parsed in the background, so the window of a large document
/// opens before its colors and preview are ready.
const BACKGROUND_FROM: usize = 200_000;

/// Whether `invalidate` updates right away rather than through `catch_up`.
pub(super) fn updates_at_once(why: Invalidate, len_u16: usize) -> bool {
    match why {
        Invalidate::Text => len_u16 < BACKGROUND_FROM,
        Invalidate::Preview => true,
        Invalidate::Zoom
        | Invalidate::Appearance
        | Invalidate::Arrivals
        | Invalidate::RemoteAllowed
        | Invalidate::Layout => false,
    }
}

/// Images that finished loading and wait to be shown, by resolved path or URL.
#[derive(Default)]
struct Arrivals {
    keys: RefCell<HashSet<String>>,
    scheduled: Cell<bool>,
}

impl Arrivals {
    /// Whether showing them still has to be scheduled.
    fn add(&self, key: String) -> bool {
        self.keys.borrow_mut().insert(key);
        !self.scheduled.replace(true)
    }

    fn take(&self) -> HashSet<String> {
        self.scheduled.set(false);
        std::mem::take(&mut *self.keys.borrow_mut())
    }

    /// They could not be shown now and have to be scheduled again.
    fn put_back(&self, keys: HashSet<String>) {
        self.keys.borrow_mut().extend(keys);
        self.scheduled.set(true);
    }

    #[cfg(feature = "selftest")]
    fn is_scheduled(&self) -> bool {
        self.scheduled.get()
    }
}

#[derive(Default)]
pub(super) struct UpdateState {
    fonts: RefCell<Option<SourceFonts>>,
    arrivals: Arrivals,
    parsing: Cell<Parsing>,
}

/// Where the background parse of a document stands. Each one holds a copy of the text, so while keys come faster than a
/// parse only the newest waits.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
enum Parsing {
    #[default]
    Idle,
    Running,
    /// Another is due when this one ends.
    RunningWithAnotherDue,
}

impl Parsing {
    /// What asking for a parse comes to, and whether one starts now.
    fn asked(self) -> (Parsing, bool) {
        match self {
            Parsing::Idle => (Parsing::Running, true),
            Parsing::Running | Parsing::RunningWithAnotherDue => (Parsing::RunningWithAnotherDue, false),
        }
    }

    /// What the end of a parse comes to, and whether another one is due.
    fn ended(self) -> (Parsing, bool) {
        debug_assert_ne!(self, Parsing::Idle, "a parse ended that never started");
        (Parsing::Idle, self == Parsing::RunningWithAnotherDue)
    }
}

pub(super) struct Images {
    pub dark: bool,
    pub env: super::env::Env,
    pub macros: Arc<str>,
}

impl Resolver for Images {
    fn math_ready(&self, tex: &str, display: bool) -> bool {
        js::ready(&js::Key::math(tex, display, self.dark, &self.macros))
    }
    fn diagram_ready(&self, source: &str, kind: DiagramKind) -> bool {
        js::ready(&js::Key::diagram(source, kind, self.dark))
    }
    fn quotes(&self) -> Option<crate::markdown::typography::Quotes> {
        self.env.quotes
    }
    fn references(&self) -> bool {
        self.env.repository.is_some()
    }
    fn citation(&self, key: &str) -> Option<String> {
        self.env.citations.as_ref()?.get(key).cloned()
    }
    fn embedded_note(&self, target: &str) -> Option<(String, String)> {
        super::env::known_note(self.env.doc_dir.as_deref()?, target)
    }
}

/// Parsing and layout run here while typing in the source; only the newest job of a document gets computed. The stack
/// is as large as the main thread's: a document must not behave differently once it is typed in.
static PARSER: Lane = Lane::new("splat-parse").with_stack(8 << 20);

/// Formulas and diagrams of the document, as render keys for one appearance.
pub(super) fn render_key(figure: &Figure, dark: bool, macros: &Arc<str>) -> js::Key {
    match figure {
        Figure::Math { tex, display } => js::Key::math(tex, *display, dark, macros),
        Figure::Diagram { source, kind } => js::Key::diagram(source, *kind, dark),
    }
}

/// The changed blocks rendered right away: all of them, or for a large change those around the visible block, which
/// `visible` finds.
fn eager_blocks(changed: Range<usize>, visible: impl FnOnce() -> usize) -> Range<usize> {
    if changed.len() <= PROGRESSIVE_FROM {
        return changed;
    }
    let near = visible().clamp(changed.start, changed.end);
    let from = near.saturating_sub(EAGER_BLOCKS / 4).max(changed.start);
    from..(from + EAGER_BLOCKS).min(changed.end)
}

pub(super) struct Updater<'a> {
    host: ForUpdater<'a>,
    state: &'a UpdateState,
}

impl<'a> Updater<'a> {
    pub(super) fn new(host: ForUpdater<'a>, state: &'a UpdateState) -> Self {
        Updater { host, state }
    }
}

#[cfg(feature = "selftest")]
impl Updater<'_> {
    pub(super) fn arrivals_scheduled(&self) -> bool {
        self.state.arrivals.is_scheduled()
    }
}

impl Updater<'_> {
    /// The source storage changed its characters: the copy of its text follows, or is read anew if it cannot.
    pub(super) fn source_edited(&self, storage: &NSTextStorage, range: NSRange, delta: isize) {
        let inserted = string_of(&storage.string().substringWithRange(range));
        self.host
            .model()
            .source_edited(from_nsrange(range), delta, &inserted, storage.length());
        if let Some(document) = self.host.document() {
            document.text_changed();
        }
    }

    /// Replaces the whole source, e.g. after loading or reverting, without an undo step. The caret and the scroll
    /// position stay where they were, as far as the new text reaches.
    pub(super) fn replace_all_text(&self, text: &str) {
        let panes = self.host.panes();
        let source = panes.source();
        let selection = source.selected_range();
        let clip = source.enclosing_scroll_view().map(|s| s.contentView());
        let origin = clip.as_ref().map(|c| c.bounds().origin);
        // A plain string would take the attributes of the replaced text's first character, or TextKit's default font
        // in an empty view, until the colors arrive from the background.
        let fonts = self.state.fonts.borrow().clone();
        let base = fonts.as_ref().map(style::base_source_attributes);
        let new_text = attributed(text, base.as_ref());
        panes.silently(|views| {
            let source_storage = storage(views.source);
            source_storage
                .replaceCharactersInRange_withAttributedString(NSRange::new(0, source_storage.length()), &new_text);
            let string = source_storage.string();
            let length = string.length();
            let start = match selection.location.min(length) {
                at if at < length => string.rangeOfComposedCharacterSequenceAtIndex(at).location,
                at => at,
            };
            self.host.selection().set_source_selection(NSRange::new(start, 0));
        });
        if let (Some(clip), Some(origin)) = (clip, origin) {
            clip.scrollToPoint(origin);
            if let Some(scroll) = source.enclosing_scroll_view() {
                scroll.reflectScrolledClipView(&clip);
            }
        }
        self.host.invalidate(Invalidate::Text);
    }

    /// Applies a source edit as one undo step. Typing edits coalesce like keystrokes in the source view.
    pub(super) fn apply_edit(&self, edit: Edit, undo_name: Option<&str>, typing: bool) {
        self.host.model().change(|st| st.clear_pending());
        let range = self.host.with_text(|snap, _| {
            let text = &snap.text;
            (edit.range.end <= text.end() && text.boundary(edit.range.start) && text.boundary(edit.range.end))
                .then(|| to_nsrange(snap.u16_range(&edit.range)))
        });
        let Some(range) = range else {
            self.update_now();
            return;
        };
        let replacement = ns(&edit.replacement);
        let selection = self.host.panes().silently(|views| {
            let source = views.source;
            if typing {
                insert_text(source, &replacement, range);
            } else {
                // Without this, NSTextView merges the command into the preceding typing undo step.
                source.breakUndoCoalescing();
                if source.shouldChangeTextInRange_replacementString(range, Some(&replacement)) {
                    storage(source).replaceCharactersInRange_withString(range, &replacement);
                    source.didChangeText();
                    if let (Some(name), Some(undo)) = (undo_name, self.host.undo_manager()) {
                        undo.setActionName(&ns(name));
                    }
                }
                source.breakUndoCoalescing();
            }
            let selection = self
                .host
                .with_text(|snap, _| to_nsrange(snap.u16_range(&edit.selection)));
            self.host.selection().set_source_selection(selection);
            selection
        });
        self.update_now();
        if !self.host.panes().preview_focused() {
            self.host.panes().for_appkit().source.scrollRangeToVisible(selection);
        }
    }

    /// The right one while typing in the source: runs `catch_up` once the keys pause for a moment.
    pub(super) fn schedule_update(&self) {
        if self.host.model().schedule_update() {
            perform_later(self.host.object(), sel!(scheduledUpdateDue), 0.04, false);
        }
    }

    /// The panes changed size; dragging the divider can bring back a preview without a command saying so.
    pub(super) fn panes_resized(&self) {
        if self.host.panes().preview_visible() && self.host.model().read(|st| st.stale_layout()) {
            self.schedule_update();
        }
    }

    /// The pause `schedule_update` waited for is over.
    pub(super) fn scheduled_update_due(&self) {
        self.host.model().update_ran();
        self.catch_up();
    }

    /// Asks the shared renderer for formulas and diagrams after a short pause in typing.
    pub(super) fn schedule_render_requests(&self) {
        perform_later(self.host.object(), sel!(requestRenders), 0.35, false);
    }

    pub(super) fn request_renders(&self) {
        if self.host.has_closed() {
            return;
        }
        self.host.pictures().forget_images_no_longer_shown();
        let dark = self.host.is_dark();
        let (current, other, macros) = self.host.model().read(move |st| {
            let all = figures(st.doc());
            let macros: Arc<str> = macros_of(all.iter().map(|(_, f)| f)).into();
            let (current, other): (Vec<_>, Vec<_>) = all
                .iter()
                .map(|(_, f)| (render_key(f, dark, &macros), render_key(f, !dark, &macros)))
                .unzip();
            (current, other, macros)
        });
        self.host.model().change(move |st| st.set_macros(macros));
        if current.is_empty() {
            js::asks_for_nothing(self.host.object());
            return;
        }
        let renderer = js::renderer(self.host.mtm());
        let owner = renderer.listen(self.host.object());
        let mut keys = current;
        // Rendering ahead for the other appearance makes switching instant, but not at the price of doubling the
        // work for documents full of formulas.
        if keys.len() <= 200 {
            keys.extend(other);
        }
        renderer.request(owner, keys);
    }

    /// The renderer calls this when formulas and diagrams arrive, possibly while this controller is itself rendering
    /// and would see its own state half-changed; the update therefore waits for the current event to finish.
    pub(super) fn renders_arrived(&self) {
        perform_later(self.host.object(), sel!(applyArrivedRenders), 0.0, true);
    }

    pub(super) fn apply_arrived_renders(&self) {
        self.host.invalidate(Invalidate::Arrivals);
    }

    /// The undo manager undid or redid a step, in the source or elsewhere.
    pub(super) fn undo_did_change(&self) {
        self.host.scroll_sync().undo_happened();
        let selection = self.host.selection();
        if selection.mirrors_source() {
            selection.set_source_selection(self.host.panes().source().selected_range());
        }
        // With TextKit 2, undoing in the source sends no textDidChange.
        if !self.host.panes().is_silent() {
            self.catch_up();
        }
    }

    /// The addresses of the images from the web this document shows, wherever they stand.
    pub(super) fn web_images_shown(&self) -> Vec<String> {
        self.host.model().read(|st| {
            let urls = st.layout().blocks.iter().flat_map(|block| images_of(block));
            urls.filter(|url| super::remote::is_remote(url))
                .map(str::to_string)
                .collect()
        })
    }

    /// An image finished loading. A document with hundreds of them must not start over for each one, so they are
    /// collected, and also not wait until the last one, as a pause in arrivals would make them.
    pub(super) fn image_arrived(&self, key: String) {
        if self.state.arrivals.add(key) {
            perform_later(self.host.object(), sel!(showArrivedImages), 0.1, true);
        }
    }

    /// Renders the blocks of the images that arrived anew, those in view at once and the others when they come near:
    /// an image changes neither the layout nor any other block.
    pub(super) fn show_arrived_images(&self) {
        let arrived = self.state.arrivals.take();
        if self.host.under_way().defers_background_results() {
            self.state.arrivals.put_back(arrived);
            perform_later(self.host.object(), sel!(showArrivedImages), 0.1, true);
            return;
        }
        let doc_dir = self.host.doc_dir();
        let shows_arrived = |block: &TopBlock| {
            images_of(block).any(|url| {
                let key = if super::remote::is_remote(url) {
                    Some(url.to_string())
                } else {
                    super::images::resolve_path(url, doc_dir.as_deref()).map(|p| p.to_string_lossy().into_owned())
                };
                key.is_some_and(|k| arrived.contains(&k))
            })
        };
        if self.host.model().change(move |st| st.mark_arrived(shows_arrived)) {
            self.host.progressive().render_visible_blocks();
            self.host.progressive().schedule_pending_blocks();
        }
    }

    pub(super) fn set_zoom(&self, zoom: f64) {
        let fonts = SourceFonts::new(zoom);
        let base = style::base_source_attributes(&fonts);
        let regular = fonts.regular().retain();
        *self.state.fonts.borrow_mut() = Some(fonts);
        self.host.model().change(move |st| st.set_zoom(zoom));
        let panes = self.host.panes();
        if panes.is_built() {
            panes.silently(|views| {
                set_typing_attributes(views.source, &base);
                views.source.setFont(Some(&regular));
            });
            self.host.invalidate(Invalidate::Zoom);
        }
        self.host.object().invalidateRestorableState();
    }

    pub(super) fn appearance_changed(&self) {
        if self.host.model().read(|st| st.rendered_dark()) != Some(self.host.is_dark()) {
            self.host.invalidate(Invalidate::Appearance);
        }
    }

    /// The right one when nobody reads the result next, after a delay or an undo: updates right away while the preview
    /// has the focus, since its caret depends on the result; otherwise only what changed, or in the background.
    pub(super) fn catch_up(&self) {
        if self.host.panes().is_silent() {
            return;
        }
        if self.host.panes().preview_focused() {
            self.update_now();
        } else if !self.updated_in_part() {
            self.update_in_background();
        }
    }

    fn view_facts(&self) -> ViewFacts {
        ViewFacts {
            caret_at: self
                .host
                .panes()
                .preview_focused()
                .then(|| self.host.selection().source_range().start),
            visible: self.host.panes().preview_visible(),
            shown_len: self.host.panes().preview().length(),
            dark: self.host.is_dark(),
        }
    }

    /// Whether only a part of a text of moderate size had to be parsed, and everything is up to date.
    fn updated_in_part(&self) -> bool {
        let views = self.view_facts();
        let doc_dir = self.host.doc_dir();
        let macros = self.host.model().read(|st| st.macros());
        let update = self.host.change_with_text(move |snap, st| {
            st.advance_in_part(snap, views, |text| images_for(views.dark, doc_dir, text, macros))
        });
        let Some(update) = update else {
            return false;
        };
        self.host.model().next_generation();
        self.show_update(update);
        true
    }

    /// The right one when the caller reads the result next, as every command does: re-parses, re-highlights and
    /// re-renders the preview blocks that changed, unless a composition or the controller's own change is under way.
    pub(super) fn update_now(&self) {
        // Replacing the text under a composition would write its marked text and offsets into the wrong place; its end
        // comes back here.
        if self.host.under_way().holds_back_an_update() {
            return;
        }
        self.host.model().next_generation();
        self.advance(None);
    }

    /// Brings the state in line with the source text, then the views with the state.
    fn advance(&self, background: Option<Background>) {
        let views = self.view_facts();
        let doc_dir = self.host.doc_dir();
        let macros = self.host.model().read(|st| st.macros());
        let update = self.host.change_with_text(move |snap, st| {
            st.advance(snap, views, background, |text| {
                images_for(views.dark, doc_dir, text, macros)
            })
        });
        self.show_update(update);
    }

    /// The right one for a text too long to parse between two keys: parses and lays out the source on the background
    /// thread, and the result is applied only if nothing changed since.
    pub(super) fn update_in_background(&self) {
        if self.host.panes().is_silent() {
            return;
        }
        let (current, stale_layout, len_u16) = self
            .host
            .with_text(|snap, st| (st.is_current(snap), st.stale_layout(), snap.len_u16));
        // Formulas that arrived change the layout only, but in a large text that takes long as well.
        let small = updates_at_once(Invalidate::Text, len_u16);
        if current && (small || !stale_layout || !self.host.panes().preview_visible()) {
            self.update_now();
            return;
        }
        let generation = self.host.model().next_generation();
        let (parsing, starts) = self.state.parsing.get().asked();
        self.state.parsing.set(parsing);
        if !starts {
            return;
        }
        let latest = self.host.model().latest_generation();
        let text: Arc<str> = self.host.on_source_text(|text| Arc::from(text));
        let dark = self.host.is_dark();
        let env = super::env::Env::known(self.host.doc_dir(), &text);
        let macros = self.host.model().read(|st| st.macros());
        let with_layout = self.host.panes().preview_visible();
        let me = dispatch2::MainThreadBound::new(Weak::from(self.host.object()), self.host.mtm());
        PARSER.run(move || {
            let current = || latest.load(std::sync::atomic::Ordering::SeqCst) == generation;
            let work = || {
                if !current() {
                    return None;
                }
                let parsed = contained(|| parse_shared_and_color(text.clone()));
                let mut failed = parsed.is_none();
                let parsed = parsed.unwrap_or_else(|| Parsed::failed(text.clone()));
                // A newer text is waiting; the layout of this one would be thrown away.
                if !current() {
                    free_in_background(parsed);
                    return None;
                }
                let layout = with_layout.then(|| {
                    let l = laid_out(&parsed.doc, &Images { dark, env, macros }, None);
                    failed |= l.is_none();
                    let l = l.unwrap_or_else(|| Layout::nothing_of(&parsed.doc));
                    let index = Index::new(&l);
                    (l, index)
                });
                Some(Background { parsed, layout, failed })
            };
            // Whatever became of it, the main thread hears: the next parse waits for that.
            let background = contained(work).flatten();
            crate::util::main_soon(move || {
                let Some(mtm) = MainThreadMarker::new() else { return };
                if let Some(c) = me.get(mtm).load() {
                    c.parse_ended(generation, &text, background);
                }
            });
        });
    }

    /// The parse under way came back, with nothing if a newer text made it pointless.
    pub(super) fn parse_ended(&self, generation: u64, text: &str, background: Option<Background>) {
        let (parsing, again) = self.state.parsing.get().ended();
        self.state.parsing.set(parsing);
        if let Some(background) = background {
            self.apply_background(generation, text, background);
        }
        if again {
            self.update_in_background();
        }
    }

    fn apply_background(&self, generation: u64, text: &str, background: Background) {
        // Freeing a large document tree takes long enough to be felt while typing.
        if !self.host.model().is_latest(generation) {
            free_in_background(background);
            return;
        }
        let same_text = self.host.on_source_text(|current| current == text);
        if !same_text || self.host.under_way().defers_background_results() {
            free_in_background(background);
            self.schedule_update();
            return;
        }
        crate::test_hooks::timed("applying a background result on the main thread", || {
            self.advance(Some(background));
        });
    }

    /// Brings source colors and preview in line with the state, which already says what comes next: until the preview
    /// view shows that, nothing may measure one against the other, so all of this counts as the controller's own.
    fn show_update(&self, update: Update) {
        let Update {
            recolor,
            preview,
            text_changed,
            appearance_switched,
        } = update;
        let preview_focused = self.host.panes().preview_focused();
        let shown = preview.is_some();
        let pending = self.host.panes().silently(|views| {
            if let Some(recolor) = recolor {
                self.apply_colors(views, recolor);
            }
            preview.map(|change| self.show_preview(views, change))
        });
        if pending == Some(true) {
            self.host.progressive().schedule_pending_blocks();
        }
        // These depend on the text, not on the preview: the outline stays visible while the preview is hidden, and a
        // formula typed meanwhile is rendered when the preview comes back.
        if text_changed || appearance_switched {
            self.schedule_render_requests();
        }
        if text_changed {
            self.host.panes().show_headings_if_open();
            self.host.word_counter().text_changed();
            self.host.change_marks().text_changed();
        }
        if text_changed && !self.host.pictures().remote_allowed() {
            let remote = self
                .host
                .model()
                .read(|st| crate::markdown::remote::has_remote_images(st.doc()));
            self.host.pictures().show_remote_banner(remote);
        }
        if !shown {
            return;
        }
        if preview_focused {
            self.host.selection().place_preview_selection();
        }
        self.host.selection().show_marks();
        if self.host.scroll_sync().take_undo_happened() {
            self.host.selection().reveal_selection_in_preview();
        }
    }

    /// Recolors the source where its colors changed, or all of it when the colors are stale.
    fn apply_colors(&self, views: Views<'_>, recolor: Recolor) {
        let fonts = self.state.fonts.borrow().clone();
        if let Some(fonts) = &fonts {
            style::apply_highlight_changes(
                &storage(views.source),
                &recolor.old_text,
                &recolor.old_spans,
                &recolor.text,
                &recolor.spans,
                fonts,
            );
        }
        free_in_background((recolor.old_text, recolor.old_spans));
    }

    /// Replaces the changed blocks in the preview view; returns whether blocks are left to render.
    fn show_preview(&self, views: Views<'_>, change: PreviewChange) -> bool {
        let PreviewChange {
            blocks,
            diff,
            mode,
            zoom,
        } = change;
        let changed = diff.prefix..diff.prefix + blocks.len();
        let notice = match mode {
            PreviewMode::Shows => None,
            PreviewMode::Failed => Some(crate::i18n::t("Darstellung fehlgeschlagen")),
            PreviewMode::TooLarge => Some(crate::i18n::t(
                "Dieses Dokument ist zu groß für die Vorschau. Der Text lässt sich weiter bearbeiten.",
            )),
        };
        let eager = eager_blocks(changed.clone(), || self.host.progressive().visible_preview_block());
        let doc_dir = self.host.doc_dir();
        let live = Live {
            requester: Some(self.host.object()),
        };
        let ctx = RenderCtx {
            mtm: self.host.mtm(),
            zoom,
            doc_dir: doc_dir.as_deref(),
            images: self.host.pictures().store(),
            max_image_width: self.host.panes().preview_image_width(),
            dark: self.host.is_dark(),
            controller: Some(self.host.object()),
            remote_images: self.host.pictures().remote_allowed(),
            leaves_the_mac: false,
            macros: self.host.model().read(|st| st.macros()),
            drawn: &live,
        };
        let replacement = objc2_foundation::NSMutableAttributedString::new();
        let mut plain = String::new();
        for (i, block) in changed.clone().zip(&blocks) {
            if eager.contains(&i) {
                if !plain.is_empty() {
                    replacement.appendAttributedString(&ctx.placeholder(&std::mem::take(&mut plain)));
                }
                replacement.appendAttributedString(&ctx.render_block(block));
            } else {
                RenderCtx::placeholder_text(block, &mut plain);
            }
        }
        if !plain.is_empty() {
            replacement.appendAttributedString(&ctx.placeholder(&plain));
        }
        if let Some(notice) = notice {
            replacement.appendAttributedString(&ctx.placeholder(notice));
        }
        let storage = storage(views.preview);
        crate::util::editing(&storage, || {
            storage.replaceCharactersInRange_withAttributedString(shown_to_ns(diff.replace), &replacement);
        });
        let takes_input = mode == PreviewMode::Shows;
        if views.preview.isEditable() != takes_input {
            views.preview.setEditable(takes_input);
        }
        self.host.model().change(move |st| st.preview_shown(changed, &eager))
    }

    /// Whether what is typed into the preview can reach the source: a notice in its place stands for no text.
    pub(super) fn preview_takes_input(&self) -> bool {
        self.host.model().read(|st| st.preview_mode()) == PreviewMode::Shows
    }
}

fn images_for(dark: bool, doc_dir: Option<PathBuf>, text: &str, macros: Arc<str>) -> Images {
    Images {
        dark,
        env: super::env::Env::known(doc_dir, text),
        macros,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_newest_parse_waits_behind_the_one_running() {
        let (running, starts) = Parsing::Idle.asked();
        assert_eq!((running, starts), (Parsing::Running, true));
        let (due, starts) = running.asked();
        assert_eq!((due, starts), (Parsing::RunningWithAnotherDue, false));
        assert_eq!(due.asked(), (Parsing::RunningWithAnotherDue, false), "still one");
        assert_eq!(due.ended(), (Parsing::Idle, true));
        assert_eq!(running.ended(), (Parsing::Idle, false));
    }

    #[test]
    fn small_changes_render_at_once_large_ones_around_the_visible_block() {
        assert_eq!(eager_blocks(3..40, || unreachable!()), 3..40);
        let changed = 10..10 + 3 * PROGRESSIVE_FROM;
        for visible in [0, 10, 11, 500, changed.end - 1, changed.end, changed.end + 50] {
            let eager = eager_blocks(changed.clone(), || visible);
            assert!(eager.len() <= EAGER_BLOCKS, "{eager:?} for {visible}");
            assert!(
                changed.start <= eager.start && eager.end <= changed.end,
                "{eager:?} for {visible}"
            );
            let seen = visible.clamp(changed.start, changed.end - 1);
            assert!(eager.contains(&seen), "{eager:?} misses {seen}");
        }
    }

    #[test]
    fn images_that_arrive_together_are_shown_together() {
        let arrivals = Arrivals::default();
        assert!(arrivals.add("a".into()), "the first one schedules the showing");
        assert!(!arrivals.add("b".into()), "the next ones wait for it");
        assert_eq!(arrivals.take().len(), 2);
        assert!(arrivals.take().is_empty());
        assert!(
            arrivals.add("c".into()),
            "the next arrival after the showing schedules anew"
        );
    }

    #[test]
    fn images_that_could_not_be_shown_wait_with_the_ones_arriving_meanwhile() {
        let arrivals = Arrivals::default();
        arrivals.add("a".into());
        arrivals.put_back(arrivals.take());
        assert!(
            !arrivals.add("b".into()),
            "putting them back comes with its own schedule"
        );
        assert_eq!(arrivals.take().len(), 2);
    }

    #[test]
    fn only_a_large_new_text_waits_for_the_background() {
        assert!(updates_at_once(Invalidate::Text, 0));
        assert!(updates_at_once(Invalidate::Text, BACKGROUND_FROM - 1));
        assert!(!updates_at_once(Invalidate::Text, BACKGROUND_FROM));
        assert!(updates_at_once(Invalidate::Preview, BACKGROUND_FROM));
        for why in [
            Invalidate::Zoom,
            Invalidate::Appearance,
            Invalidate::Arrivals,
            Invalidate::RemoteAllowed,
            Invalidate::Layout,
        ] {
            assert!(!updates_at_once(why, 0), "{why:?}");
        }
    }
}
