//! Bringing the source colors and the preview in line with the source text: parsing, in the background while the
//! source has the focus, and replacing the preview blocks that changed. What is known after each step lies in `state`.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ops::Range;
use std::rc::Rc;

use objc2::rc::Weak;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, sel};
use objc2_app_kit::{NSTextInputClient, NSTextStorage};
use objc2_foundation::NSRange;

use super::attributes::{attributed, set_typing_attributes};
use super::render::RenderCtx;
use super::scroll::top_offset;
use super::state::{Invalidate, PreviewInput, PreviewPlan, images_of, laid_out};
use super::style::{self, SourceFonts};
use super::sync::Snapshot;
use super::{EditorController, js, string_of};
use crate::markdown::doc::{DiagramKind, Figure, figures};
use crate::markdown::incremental::{PLAIN_FROM, Parsed, Splice, parse_and_color};
use crate::preview::layout::{Layout, Resolver, TopBlock};
use crate::preview::map::Index;
use crate::util::{Lane, contained, free_in_background, from_nsrange, perform_later, shown_to_ns, storage};

/// Up to this many changed blocks render at once; more show as plain text first and render while idle.
pub(super) const PROGRESSIVE_FROM: usize = 300;
/// Blocks rendered right away around the visible part when rendering progressively.
const EAGER_BLOCKS: usize = 120;
/// A whole new text of this many UTF-16 units or more is parsed in the background, so the window of a large document
/// opens before its colors and preview are ready.
const BACKGROUND_FROM: usize = 200_000;
/// Up to this many bytes, typing in the source parses around the edit right away, in about 8 ms. Above, that would eat
/// into the frame the keystroke itself needs, and everything is parsed in the background.
const PARTS_ON_MAIN_UP_TO: usize = 2 << 20;

/// Whether `invalidate` updates right away rather than through `refresh`.
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
pub(super) struct Arrivals {
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
    pub(super) fn is_scheduled(&self) -> bool {
        self.scheduled.get()
    }
}

pub(super) struct Images {
    pub dark: bool,
    pub env: super::env::Env,
}

impl Resolver for Images {
    fn math_ready(&self, tex: &str, display: bool) -> bool {
        js::ready(&js::Key::math(tex, display, self.dark))
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
pub(super) fn render_key(figure: &Figure, dark: bool) -> js::Key {
    match figure {
        Figure::Math { tex, display } => js::Key::math(tex, *display, dark),
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

impl EditorController {
    /// The source storage changed its characters: the copy of its text follows, or is read anew if it cannot.
    pub(super) fn source_edited(&self, storage: &NSTextStorage, range: NSRange, delta: isize) {
        // Not the editor's own mirror of its text, so the offsets of an open editor mean nothing anymore.
        if !self.ivars().busy.get() {
            self.abandon_atom_edit();
        }
        let inserted = string_of(&storage.string().substringWithRange(range));
        self.ivars()
            .copy
            .edited(from_nsrange(range), delta, &inserted, storage.length());
    }

    /// The current source text. Edits patch it in place; it is read from the view only when that is not possible.
    /// Holding on to it across an edit of the source makes that edit copy the whole text.
    pub fn snapshot(&self) -> Rc<Snapshot> {
        self.ivars().copy.current(|| {
            let string = self.source().string();
            (string_of(&string), string.length())
        })
    }

    pub fn source_text(&self) -> String {
        self.snapshot().text.clone()
    }

    /// The text exactly as the source view holds it, for saving.
    pub fn exact_text(&self) -> String {
        string_of(&self.source().string())
    }

    /// Replaces the whole source, e.g. after loading or reverting, without an undo step. The caret and the scroll
    /// position stay where they were, as far as the new text reaches.
    pub fn replace_all_text(&self, text: &str) {
        self.abandon_atom_edit();
        let source = self.source();
        let selection = source.selectedRange();
        let clip = source.enclosingScrollView().map(|s| s.contentView());
        let origin = clip.as_ref().map(|c| c.bounds().origin);
        let source_storage = storage(source);
        // A plain string would take the attributes of the replaced text's first character, or TextKit's default font
        // in an empty view, until the colors arrive from the background.
        let fonts = self.ivars().state.borrow().fonts().cloned();
        let base = fonts.as_ref().map(style::base_source_attributes);
        let new_text = attributed(text, base.as_ref());
        let silent = self.silently();
        source_storage
            .replaceCharactersInRange_withAttributedString(NSRange::new(0, source_storage.length()), &new_text);
        let string = source_storage.string();
        let length = string.length();
        let start = match selection.location.min(length) {
            at if at < length => string.rangeOfComposedCharacterSequenceAtIndex(at).location,
            at => at,
        };
        self.set_source_selection(NSRange::new(start, 0));
        drop(silent);
        if let (Some(clip), Some(origin)) = (clip, origin) {
            clip.scrollToPoint(origin);
            if let Some(scroll) = source.enclosingScrollView() {
                scroll.reflectScrolledClipView(&clip);
            }
        }
        self.invalidate(Invalidate::Text);
    }

    pub(super) fn schedule_update(&self) {
        if self.ivars().copy.schedule_update() {
            perform_later(self, sel!(flushUpdate), 0.04, false);
        }
    }

    /// Asks the shared renderer for formulas and diagrams after a short pause in typing.
    pub(super) fn schedule_render_requests(&self) {
        perform_later(self, sel!(flushRenderRequests), 0.35, false);
    }

    pub(super) fn request_renders(&self) {
        let dark = self.is_dark();
        let (current, other): (Vec<_>, Vec<_>) = {
            let st = self.ivars().state.borrow();
            figures(st.doc())
                .iter()
                .map(|(_, f)| (render_key(f, dark), render_key(f, !dark)))
                .unzip()
        };
        if current.is_empty() {
            return;
        }
        let renderer = js::renderer(self.mtm());
        renderer.listen(self);
        // Rendering ahead for the other appearance makes switching instant, but not at the price of doubling the
        // work for documents full of formulas.
        let prefetch = current.len() <= 200;
        renderer.request(current);
        if prefetch {
            renderer.request(other);
        }
    }

    /// The renderer calls this when formulas and diagrams arrive, possibly while this controller is itself rendering
    /// and would see its own state half-changed; the update therefore waits for the current event to finish.
    pub fn renders_arrived(&self) {
        perform_later(self, sel!(applyArrivedRenders), 0.0, true);
    }

    pub(super) fn image_loaded(&self, path: std::path::PathBuf, file: super::images::ImageFile) {
        let key = path.to_string_lossy().into_owned();
        self.ivars().images.borrow_mut().finish(path, file);
        self.image_arrived(key);
    }

    /// An image finished loading. A document with hundreds of them must not start over for each one, so they are
    /// collected, and also not wait until the last one, as a pause in arrivals would make them.
    pub(super) fn image_arrived(&self, key: String) {
        if self.ivars().arrivals.add(key) {
            perform_later(self, sel!(showArrivedImages), 0.1, true);
        }
    }

    /// Renders the blocks of the images that arrived anew, those in view at once and the others when they come near:
    /// an image changes neither the layout nor any other block.
    pub(super) fn show_arrived_images(&self) {
        let arrived = self.ivars().arrivals.take();
        if self.ivars().busy.get() || self.preview_editing() {
            self.ivars().arrivals.put_back(arrived);
            perform_later(self, sel!(showArrivedImages), 0.1, true);
            return;
        }
        let doc_dir = self.doc_dir();
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
        let any = {
            let Ok(mut guard) = self.ivars().state.try_borrow_mut() else {
                self.ivars().arrivals.put_back(arrived);
                perform_later(self, sel!(showArrivedImages), 0.1, true);
                return;
            };
            guard.mark_arrived(shows_arrived)
        };
        if any {
            self.render_visible_blocks();
            self.schedule_pending_blocks();
        }
    }

    pub(super) fn set_zoom(&self, zoom: f64) {
        let fonts = SourceFonts::new(zoom);
        let base = style::base_source_attributes(&fonts);
        let regular = fonts.regular().retain();
        {
            self.ivars().state.borrow_mut().set_fonts(zoom, fonts);
        }
        if let Some(source) = self.ivars().source.get() {
            set_typing_attributes(source, &base);
            source.setFont(Some(&regular));
            self.invalidate(Invalidate::Zoom);
        }
        self.invalidateRestorableState();
    }

    pub fn appearance_changed(&self) {
        if self.ivars().state.borrow().rendered_dark() != Some(self.is_dark()) {
            self.invalidate(Invalidate::Appearance);
        }
    }

    /// Updates right away while the preview has the focus, since its caret depends on the result; otherwise parses in
    /// the background.
    pub(super) fn refresh(&self) {
        if self.preview_focused() {
            self.update();
        } else if !self.update_part() {
            self.update_in_background();
        }
    }

    /// Updates right away if only a part of a text of moderate size has to be parsed; whether that is so.
    fn update_part(&self) -> bool {
        if self.ivars().busy.get() {
            return true;
        }
        let snap = self.snapshot();
        let visible = self.preview_visible();
        let parsed = {
            let mut st = self.ivars().state.borrow_mut();
            if st.wants_whole_parse(visible) || snap.text.len() > PARTS_ON_MAIN_UP_TO || st.is_current(&snap) {
                return false;
            }
            st.parse_part(&snap.text)
        };
        let Some(parsed) = parsed else {
            return false;
        };
        self.ivars().copy.next_generation();
        self.ivars().state.borrow_mut().set_preview_failed(false);
        self.apply_update(&snap, Some(parsed), None);
        true
    }

    /// Re-parses, re-highlights and re-renders the preview blocks that changed.
    pub(super) fn update(&self) {
        // Replacing the text under a composition would write its marked text and offsets into the wrong place; its end
        // comes back here.
        if self.ivars().busy.get() || self.preview().hasMarkedText() {
            return;
        }
        self.ivars().copy.next_generation();
        let snap = self.snapshot();
        let parsed = {
            let mut st = self.ivars().state.borrow_mut();
            (!st.is_current(&snap)).then(|| st.parse(&snap.text))
        };
        self.apply_update(&snap, parsed, None);
    }

    /// Parses and lays out the source on the background thread; the result is applied only if nothing changed since.
    pub(super) fn update_in_background(&self) {
        if self.ivars().busy.get() {
            return;
        }
        let snap = self.snapshot();
        let (current, stale_layout) = {
            let st = self.ivars().state.borrow();
            (st.is_current(&snap), st.stale_layout())
        };
        // Formulas that arrived change the layout only, but in a large text that takes long as well.
        let small = updates_at_once(Invalidate::Text, snap.len_u16);
        if current && (small || !stale_layout || !self.preview_visible()) {
            self.update();
            return;
        }
        let copy = &self.ivars().copy;
        let generation = copy.next_generation();
        let latest = copy.latest_generation();
        let text = snap.text.clone();
        let dark = self.is_dark();
        let env = super::env::Env::known(self.doc_dir(), &text);
        let with_layout = self.preview_visible();
        let me = dispatch2::MainThreadBound::new(Weak::from(self), self.mtm());
        PARSER.run(move || {
            let current = || latest.load(std::sync::atomic::Ordering::SeqCst) == generation;
            if !current() {
                return;
            }
            let parsed = contained(|| parse_and_color(&text));
            let mut failed = parsed.is_none();
            let parsed = parsed.unwrap_or_else(Parsed::plain);
            // A newer text is waiting; the layout of this one would be thrown away.
            if !current() {
                return;
            }
            let layout = with_layout.then(|| {
                let l = laid_out(&text, &parsed.doc, &Images { dark, env }, None);
                failed |= l.is_none();
                let l = l.unwrap_or_default();
                let index = Index::new(&l);
                (l, index)
            });
            if !current() {
                return;
            }
            dispatch2::DispatchQueue::main().exec_async(move || {
                let Some(mtm) = MainThreadMarker::new() else { return };
                if let Some(c) = me.get(mtm).load() {
                    c.apply_background(generation, &text, parsed, layout, failed);
                }
            });
        });
    }

    fn apply_background(
        &self,
        generation: u64,
        text: &str,
        parsed: Parsed,
        layout: Option<(Layout, Index)>,
        failed: bool,
    ) {
        // Freeing a large document tree takes long enough to be felt while typing.
        if !self.ivars().copy.is_latest(generation) {
            free_in_background((parsed, layout));
            return;
        }
        let snap = self.snapshot();
        if snap.text != text || self.ivars().busy.get() || self.preview_editing() {
            free_in_background((parsed, layout));
            self.schedule_update();
            return;
        }
        self.ivars().state.borrow_mut().set_preview_failed(failed);
        #[cfg(feature = "selftest")]
        let started = std::time::Instant::now();
        self.apply_update(&snap, Some(parsed), layout);
        #[cfg(feature = "selftest")]
        if std::env::var_os("SPLAT_BENCH").is_some() {
            eprintln!(
                "BENCH applying a background result on the main thread: {:.1} ms",
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
    }

    /// Brings source colors and preview in line with `snap`. `parsed` is `None` when the text is unchanged; `layout`
    /// is one computed elsewhere for that text without a caret.
    fn apply_update(&self, snap: &Snapshot, parsed: Option<Parsed>, layout_done: Option<(Layout, Index)>) {
        let preview_focused = self.preview_focused();
        let caret = preview_focused.then(|| self.source_selection(snap).start);
        let dark = self.is_dark();
        let changed = parsed.is_some();
        let splice = parsed.as_ref().and_then(|p| p.splice.clone());
        self.apply_colors(snap, parsed);
        let switched = self.ivars().state.borrow().rendered_dark() != Some(dark);
        let plan = self.plan_preview(caret, dark, changed, layout_done, splice.as_ref());
        let shown = plan.is_some();
        if let Some(plan) = plan
            && self.show_preview(plan, dark)
        {
            self.schedule_pending_blocks();
        }
        // These depend on the text, not on the preview: the outline stays visible while the preview is hidden, and a
        // formula typed meanwhile is rendered when the preview comes back.
        if changed || switched {
            self.schedule_render_requests();
        }
        if changed {
            self.refresh_outline();
        }
        if changed && !self.ivars().remote_allowed.get() {
            let remote = crate::markdown::doc::has_remote_images(self.ivars().state.borrow().doc());
            self.show_remote_banner(remote);
        }
        if !shown {
            return;
        }
        if preview_focused {
            self.place_preview_selection();
        }
        self.refresh_marks();
        if self.ivars().scroll_after_undo.replace(false) {
            self.reveal_selection_in_preview();
        }
    }

    /// Takes over a new parse result and recolors the source where its colors changed, or all of it when the
    /// colors are stale.
    fn apply_colors(&self, snap: &Snapshot, parsed: Option<Parsed>) {
        let Some(recolor) = self.ivars().state.borrow_mut().adopt(snap, parsed) else {
            return;
        };
        if let Some(fonts) = &recolor.fonts {
            let _silent = self.silently();
            style::apply_highlight_changes(
                &storage(self.source()),
                &recolor.old_text,
                &recolor.old_spans,
                &snap.text,
                &recolor.spans,
                fonts,
            );
        }
        free_in_background((recolor.old_text, recolor.old_spans));
    }

    /// Lays out the text and finds the blocks that changed; `None` when the preview stays as it is.
    fn plan_preview(
        &self,
        caret: Option<usize>,
        dark: bool,
        text_changed: bool,
        layout_done: Option<(Layout, Index)>,
        splice: Option<&Splice>,
    ) -> Option<PreviewPlan> {
        let input = PreviewInput {
            caret,
            text_changed,
            layout_done,
            splice,
            visible: self.preview_visible(),
            shown_len: storage(self.preview()).length(),
        };
        let doc_dir = self.doc_dir();
        self.ivars().state.borrow_mut().plan_preview(input, |text| Images {
            dark,
            env: super::env::Env::known(doc_dir, text),
        })
    }

    /// Replaces the changed blocks in the preview view and stores the new layout; returns whether some blocks
    /// were left as placeholders.
    fn show_preview(&self, plan: PreviewPlan, dark: bool) -> bool {
        let changed = plan.diff.prefix..plan.keys.len() - plan.diff.suffix;
        let notice = {
            let st = self.ivars().state.borrow();
            if st.preview_failed() {
                Some(crate::i18n::t("Darstellung fehlgeschlagen"))
            } else {
                (st.text().len() >= PLAIN_FROM).then(|| {
                    crate::i18n::t(
                        "Dieses Dokument ist zu groß für die Vorschau. Der Text lässt sich weiter bearbeiten.",
                    )
                })
            }
        };
        let eager = eager_blocks(changed.clone(), || self.visible_preview_block());
        let doc_dir = self.doc_dir();
        let ctx = RenderCtx {
            mtm: self.mtm(),
            zoom: plan.zoom,
            doc_dir: doc_dir.as_deref(),
            images: &self.ivars().images,
            max_image_width: (self.preview().bounds().size.width - 60.0).max(200.0),
            dark,
            controller: Some(self),
            remote_images: self.ivars().remote_allowed.get(),
        };
        let replacement = objc2_foundation::NSMutableAttributedString::new();
        let mut plain = String::new();
        for i in changed {
            let block = &plan.layout.blocks[i];
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
        let storage = storage(self.preview());
        let range = shown_to_ns(plan.diff.replace.clone());
        {
            let _silent = self.silently();
            storage.beginEditing();
            storage.replaceCharactersInRange_withAttributedString(range, &replacement);
            storage.endEditing();
        }
        self.ivars().state.borrow_mut().adopt_preview(plan, &eager, dark)
    }

    /// Index of the first preview block in view.
    pub(super) fn visible_preview_block(&self) -> usize {
        let top = top_offset(self.preview()).unwrap_or(0);
        self.ivars().state.try_borrow().map_or(0, |st| {
            st.index()
                .block_starts
                .partition_point(|&s| s.0 <= top)
                .saturating_sub(1)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
