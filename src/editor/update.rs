//! Bringing the source colors and the preview in line with the source text: parsing, in the background while the
//! source has the focus, and replacing the preview blocks that changed.

use std::cell::OnceCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use objc2::rc::Weak;
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, sel};
use objc2_app_kit::NSTextStorage;
use objc2_foundation::{NSAttributedString, NSRange};

use super::render::RenderCtx;
use super::scroll::top_offset;
use super::style::{self, SourceFonts};
use super::sync::{BlockDiff, Snapshot};
use super::{EditorController, Invalidate, js, string_of};
use crate::markdown::doc::{DiagramKind, Figure, figures};
use crate::markdown::incremental::{Parsed, Splice, parse_and_color, reparse_and_color};
use crate::preview::layout::{Atom, Layout, Resolver, RunMap, TopBlock, layout, relayout, reusable};
use crate::preview::map::Index;
use crate::util::{ns, perform_later, storage};

/// Up to this many changed blocks render at once; more show as plain text first and render while idle.
pub(super) const PROGRESSIVE_FROM: usize = 300;
/// Blocks rendered right away around the visible part when rendering progressively.
const EAGER_BLOCKS: usize = 120;
/// A whole new text of this many UTF-16 units or more is parsed in the background, so the window of a large document
/// opens before its colors and preview are ready.
const BACKGROUND_FROM: usize = 200_000;

/// Whether `invalidate` updates right away rather than through `refresh`.
pub(super) fn updates_at_once(why: Invalidate, len_u16: usize) -> bool {
    match why {
        Invalidate::Text => len_u16 < BACKGROUND_FROM,
        Invalidate::Preview => true,
        Invalidate::Zoom | Invalidate::Appearance | Invalidate::Arrivals | Invalidate::Layout => false,
    }
}

pub(super) struct Images {
    pub dark: bool,
    pub env: super::env::Env,
}

impl Resolver for Images {
    fn math_ready(&self, tex: &str, display: bool) -> bool {
        js::ready(&js::Key::Math {
            tex: tex.to_string(),
            display,
            dark: self.dark,
        })
    }
    fn diagram_ready(&self, source: &str, kind: DiagramKind) -> bool {
        js::ready(&js::Key::Diagram {
            source: source.to_string(),
            kind,
            dark: self.dark,
        })
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
        super::env::embedded_note(self.env.doc_dir.as_deref()?, target)
    }
}

type ParseJob = Box<dyn FnOnce() + Send>;

thread_local! {
    static PARSER: OnceCell<std::sync::mpsc::Sender<ParseJob>> = const { OnceCell::new() };
}

/// Parsing and layout run here while typing in the source; only the newest job of a document gets computed.
fn parse_in_background(job: impl FnOnce() + Send + 'static) {
    PARSER.with(|p| {
        let sender = p.get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel::<ParseJob>();
            std::thread::Builder::new()
                .name("splat-parse".into())
                .spawn(move || {
                    for job in rx {
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                    }
                })
                .expect("parse thread");
            tx
        });
        let _ = sender.send(Box::new(job));
    });
}

/// Formulas and diagrams of the document, as render keys for one appearance.
pub(super) fn render_key(figure: &Figure, dark: bool) -> js::Key {
    match figure {
        Figure::Math { tex, display } => js::Key::Math {
            tex: tex.to_string(),
            display: *display,
            dark,
        },
        Figure::Diagram { source, kind } => js::Key::Diagram {
            source: source.to_string(),
            kind: *kind,
            dark,
        },
    }
}

pub(super) fn shows_image(block: &TopBlock) -> bool {
    block.paras.iter().flat_map(|p| &p.runs).any(|r| {
        matches!(
            &r.map,
            RunMap::Atom {
                atom: Atom::Image(_),
                ..
            }
        )
    })
}

/// What the preview needs after an update.
#[derive(Debug, PartialEq, Eq)]
enum PreviewWork {
    Keep,
    /// The preview is hidden; it is laid out once it shows again.
    Postpone,
    Lay(LayoutSource),
}

#[derive(Debug, PartialEq, Eq)]
enum LayoutSource {
    /// The layout the background thread computed for this text, without a caret.
    Background,
    /// The previous layout, with the blocks outside the changed ones taken over.
    Partial,
    Full,
}

/// What `preview_work` decides on.
struct PreviewFacts {
    text_changed: bool,
    caret_moved: bool,
    /// The user edited the preview itself, so it no longer shows what its blocks say.
    preview_edited: bool,
    shows_blocks: bool,
    /// Formulas or images that arrived meanwhile change blocks the text did not touch.
    stale_layout: bool,
    visible: bool,
    has_caret: bool,
    background_layout: bool,
    partial_parse: bool,
}

fn preview_work(f: &PreviewFacts) -> PreviewWork {
    if !f.text_changed && !f.caret_moved && !f.preview_edited && f.shows_blocks && !f.stale_layout {
        return PreviewWork::Keep;
    }
    if !f.visible && !f.preview_edited {
        return PreviewWork::Postpone;
    }
    PreviewWork::Lay(if f.background_layout && !f.has_caret {
        LayoutSource::Background
    } else if f.partial_parse && !f.stale_layout {
        LayoutSource::Partial
    } else {
        LayoutSource::Full
    })
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

/// A new preview, computed while the state is borrowed and shown after it is released.
struct PreviewPlan {
    layout: Layout,
    index: Option<Index>,
    keys: Vec<u64>,
    lens: Vec<usize>,
    diff: BlockDiff,
    zoom: f64,
}

impl EditorController {
    /// The source storage changed its characters: the copy of its text follows, or is read anew if it cannot.
    pub(super) fn source_edited(&self, storage: &NSTextStorage, range: NSRange, delta: isize) {
        let version = self.ivars().copy.version.get() + 1;
        self.ivars().copy.version.set(version);
        let Ok(mut slot) = self.ivars().copy.snapshot.try_borrow_mut() else {
            return;
        };
        let inserted = string_of(&storage.string().substringWithRange(range));
        let length = storage.length();
        *slot = slot.take().and_then(|snap| {
            let snap = Rc::try_unwrap(snap).unwrap_or_else(|shared| (*shared).clone());
            let range = range.location..range.location + range.length;
            // A copy that went out of step would end up in every command and the preview.
            snap.patched(range, delta, &inserted, version)
                .filter(|s| s.len_u16 == length)
                .map(Rc::new)
        });
    }

    /// The current source text. Edits patch it in place; it is read from the view only when that is not possible.
    /// Holding on to it across an edit of the source makes that edit copy the whole text.
    pub fn snapshot(&self) -> Rc<Snapshot> {
        let version = self.ivars().copy.version.get();
        if let Some(s) = self
            .ivars()
            .copy
            .snapshot
            .borrow()
            .as_ref()
            .filter(|s| s.version == version)
        {
            return s.clone();
        }
        let string = self.source().string();
        let s = Rc::new(Snapshot::new(string_of(&string), string.length(), version));
        *self.ivars().copy.snapshot.borrow_mut() = Some(s.clone());
        s
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
        let source = self.source();
        let selection = source.selectedRange();
        let clip = source.enclosingScrollView().map(|s| s.contentView());
        let origin = clip.as_ref().map(|c| c.bounds().origin);
        let source_storage = storage(source);
        // A plain string would take the attributes of the replaced text's first character, or TextKit's default font
        // in an empty view, until the colors arrive from the background.
        let fonts = self.ivars().state.borrow().fonts.clone();
        let base = fonts.as_ref().map(style::base_source_attributes);
        // SAFETY: The attributes have the types their keys expect.
        let new_text = unsafe {
            NSAttributedString::initWithString_attributes(NSAttributedString::alloc(), &ns(text), base.as_deref())
        };
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
        if self.ivars().copy.update_scheduled.replace(true) {
            return;
        }
        perform_later(self, sel!(flushUpdate), 0.04, false);
    }

    /// Asks the shared renderer for formulas and diagrams after a short pause in typing.
    fn schedule_render_requests(&self) {
        perform_later(self, sel!(flushRenderRequests), 0.35, false);
    }

    pub(super) fn request_renders(&self) {
        let dark = self.is_dark();
        let (current, other): (Vec<_>, Vec<_>) = {
            let st = self.ivars().state.borrow();
            figures(&st.doc)
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

    /// Loaders and the renderer call this when images arrive, possibly while this controller is itself rendering
    /// and would see its own state half-changed; the update therefore waits for the current event to finish.
    pub fn images_changed(&self) {
        perform_later(self, sel!(applyArrivedImages), 0.0, true);
    }

    pub(super) fn image_loaded(&self, path: std::path::PathBuf, loaded: Option<super::images::Loaded>) {
        self.ivars().images.borrow_mut().finish(path, loaded);
        self.images_changed();
    }

    pub(super) fn set_zoom(&self, zoom: f64) {
        let fonts = SourceFonts::new(zoom);
        let base = style::base_source_attributes(&fonts);
        let regular = fonts.regular().retain();
        {
            let mut st = self.ivars().state.borrow_mut();
            st.zoom = zoom;
            st.fonts = Some(fonts);
        }
        if let Some(source) = self.ivars().source.get() {
            // SAFETY: A font and a color, the types their keys expect.
            unsafe { source.setTypingAttributes(&base) };
            source.setFont(Some(&regular));
            self.invalidate(Invalidate::Zoom);
        }
        self.invalidateRestorableState();
    }

    pub fn appearance_changed(&self) {
        if self.ivars().state.borrow().rendered_dark != Some(self.is_dark()) {
            self.invalidate(Invalidate::Appearance);
        }
    }

    /// Updates right away while the preview has the focus, since its caret depends on the result; otherwise parses in
    /// the background.
    pub(super) fn refresh(&self) {
        if self.preview_focused() {
            self.update();
        } else {
            self.update_in_background();
        }
    }

    /// Re-parses, re-highlights and re-renders the preview blocks that changed.
    pub(super) fn update(&self) {
        if self.ivars().busy.get() {
            return;
        }
        let copy = &self.ivars().copy;
        copy.generation.set(copy.generation.get() + 1);
        let snap = self.snapshot();
        let parsed = {
            let mut guard = self.ivars().state.borrow_mut();
            let st = &mut *guard;
            (!st.is_current(&snap)).then(|| reparse_and_color(&mut st.doc, &st.text, &st.spans, &snap.text))
        };
        self.apply_update(&snap, parsed, None);
    }

    /// Parses and lays out the source on the background thread; the result is applied only if nothing changed since.
    pub(super) fn update_in_background(&self) {
        if self.ivars().busy.get() {
            return;
        }
        let snap = self.snapshot();
        if self.ivars().state.borrow().is_current(&snap) {
            self.update();
            return;
        }
        let copy = &self.ivars().copy;
        let generation = copy.generation.get() + 1;
        copy.generation.set(generation);
        let latest = copy.latest_job.clone();
        latest.store(generation, std::sync::atomic::Ordering::SeqCst);
        let text = snap.text.clone();
        let dark = self.is_dark();
        let env = super::env::Env::new(self.doc_dir(), &text);
        let with_layout = self.preview_visible();
        let me = dispatch2::MainThreadBound::new(Weak::from(self), self.mtm());
        parse_in_background(move || {
            let result = (latest.load(std::sync::atomic::Ordering::SeqCst) == generation).then(|| {
                let parsed = parse_and_color(&text);
                let layout = with_layout.then(|| {
                    let l = layout(&text, &parsed.doc, &Images { dark, env }, None);
                    let index = Index::new(&l);
                    (l, index)
                });
                (parsed, layout)
            });
            dispatch2::DispatchQueue::main().exec_async(move || {
                let Some(mtm) = MainThreadMarker::new() else { return };
                if let (Some(c), Some((parsed, layout))) = (me.get(mtm).load(), result) {
                    c.apply_background(generation, &text, parsed, layout);
                }
            });
        });
    }

    fn apply_background(&self, generation: u64, text: &str, parsed: Parsed, layout: Option<(Layout, Index)>) {
        if generation != self.ivars().copy.generation.get() {
            return;
        }
        let snap = self.snapshot();
        if snap.text != text || self.ivars().busy.get() || self.preview_editing() {
            self.schedule_update();
            return;
        }
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
        let switched = self.ivars().state.borrow().rendered_dark != Some(dark);
        let Some(plan) = self.plan_preview(caret, dark, changed, layout_done, splice.as_ref()) else {
            return;
        };
        let progressive = self.show_preview(plan, dark);
        if progressive {
            self.schedule_pending_blocks();
        }
        if changed || switched {
            self.schedule_render_requests();
        }
        if changed {
            self.refresh_outline();
        }
        if changed && !self.ivars().remote_allowed.get() {
            let remote = crate::markdown::doc::has_remote_images(&self.ivars().state.borrow().doc);
            self.show_remote_banner(remote);
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
        let (old_text, old_spans, spans, fonts) = {
            let mut guard = self.ivars().state.borrow_mut();
            let st = &mut *guard;
            st.version = snap.version;
            let all = std::mem::take(&mut st.stale.colors);
            let old = parsed.map(|parsed| {
                // Freeing a large document tree takes long enough to be felt while typing.
                let old_doc = std::mem::replace(&mut st.doc, parsed.doc);
                js::background(move || drop(old_doc));
                (
                    std::mem::replace(&mut st.text, snap.text.clone()),
                    std::mem::replace(&mut st.spans, Arc::new(parsed.spans)),
                )
            });
            let (old_text, old_spans) = match old {
                Some(old) if !all => old,
                None if !all => return,
                _ => (String::new(), Arc::default()),
            };
            (old_text, old_spans, st.spans.clone(), st.fonts.clone())
        };
        if let Some(fonts) = fonts {
            let _silent = self.silently();
            style::apply_highlight_changes(
                &storage(self.source()),
                &old_text,
                &old_spans,
                &snap.text,
                &spans,
                &fonts,
            );
        }
        js::background(move || drop((old_text, old_spans)));
    }

    /// Lays out the text and finds the blocks that changed; `None` when the preview stays as it is.
    fn plan_preview(
        &self,
        caret: Option<usize>,
        dark: bool,
        changed: bool,
        layout_done: Option<(Layout, Index)>,
        splice: Option<&Splice>,
    ) -> Option<PreviewPlan> {
        let visible = self.preview_visible();
        let shown_len = storage(self.preview()).length();
        let mut guard = self.ivars().state.borrow_mut();
        let st = &mut *guard;
        let stale_layout = std::mem::take(&mut st.stale.layout);
        let work = preview_work(&PreviewFacts {
            text_changed: changed,
            caret_moved: caret != st.caret,
            preview_edited: st.dirty_preview.is_some(),
            shows_blocks: !st.blocks.keys.is_empty(),
            stale_layout,
            visible,
            has_caret: caret.is_some(),
            background_layout: layout_done.is_some(),
            partial_parse: splice.is_some(),
        });
        let source = match work {
            PreviewWork::Keep => return None,
            PreviewWork::Postpone => {
                // The layout no longer matches the text; showing the preview again lays it out anew.
                st.caret = None;
                st.stale.layout = true;
                st.scroll_spans = None;
                return None;
            }
            PreviewWork::Lay(source) => source,
        };
        let old_caret = std::mem::replace(&mut st.caret, caret);
        let images = Images {
            dark,
            env: super::env::Env::new(self.doc_dir(), &st.text),
        };
        let (layout, index) = match (source, layout_done, splice) {
            (LayoutSource::Background, Some((done, index)), _) => (done, Some(index)),
            (LayoutSource::Partial, _, Some(splice)) => {
                let old = std::mem::take(&mut st.layout);
                let reuse = reusable(splice, &st.doc, old_caret);
                (relayout(&st.text, &st.doc, &images, caret, old, &reuse), None)
            }
            _ => (layout(&st.text, &st.doc, &images, caret), None),
        };
        let keys: Vec<u64> = layout.blocks.iter().map(|b| b.key).collect();
        let lens: Vec<usize> = layout.blocks.iter().map(|b| b.len_u16).collect();
        let diff = st.blocks.diff(&keys, st.dirty_preview.take(), shown_len);
        Some(PreviewPlan {
            layout,
            index,
            keys,
            lens,
            diff,
            zoom: st.zoom,
        })
    }

    /// Replaces the changed blocks in the preview view and stores the new layout; returns whether some blocks
    /// were left as placeholders.
    fn show_preview(&self, plan: PreviewPlan, dark: bool) -> bool {
        let changed = plan.diff.prefix..plan.keys.len() - plan.diff.suffix;
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
        let storage = storage(self.preview());
        let range = NSRange::new(plan.diff.replace.start, plan.diff.replace.len());
        {
            let _silent = self.silently();
            storage.beginEditing();
            storage.replaceCharactersInRange_withAttributedString(range, &replacement);
            storage.endEditing();
        }
        let mut guard = self.ivars().state.borrow_mut();
        let st = &mut *guard;
        let index = plan.index.unwrap_or_else(|| Index::new(&plan.layout));
        let old = (
            std::mem::replace(&mut st.index, index),
            std::mem::replace(&mut st.layout, plan.layout),
        );
        js::background(move || drop(old));
        st.blocks.replace(&plan.diff, plan.keys, plan.lens, &eager);
        st.scroll_spans = None;
        st.rendered_dark = Some(dark);
        st.blocks.pending()
    }

    /// Index of the first preview block in view.
    pub(super) fn visible_preview_block(&self) -> usize {
        let top = top_offset(self.preview()).unwrap_or(0);
        self.ivars().state.try_borrow().map_or(0, |st| {
            st.index.block_starts.partition_point(|&s| s <= top).saturating_sub(1)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet() -> PreviewFacts {
        PreviewFacts {
            text_changed: false,
            caret_moved: false,
            preview_edited: false,
            shows_blocks: true,
            stale_layout: false,
            visible: true,
            has_caret: false,
            background_layout: false,
            partial_parse: false,
        }
    }

    #[test]
    fn the_preview_stays_while_nothing_it_shows_changed() {
        assert_eq!(preview_work(&quiet()), PreviewWork::Keep);
        for f in [
            PreviewFacts {
                text_changed: true,
                ..quiet()
            },
            PreviewFacts {
                caret_moved: true,
                ..quiet()
            },
            PreviewFacts {
                preview_edited: true,
                ..quiet()
            },
            PreviewFacts {
                shows_blocks: false,
                ..quiet()
            },
            PreviewFacts {
                stale_layout: true,
                ..quiet()
            },
        ] {
            assert_eq!(preview_work(&f), PreviewWork::Lay(LayoutSource::Full));
        }
    }

    #[test]
    fn a_hidden_preview_waits_unless_the_user_edited_it() {
        let hidden = PreviewFacts {
            visible: false,
            text_changed: true,
            ..quiet()
        };
        assert_eq!(preview_work(&hidden), PreviewWork::Postpone);
        let edited = PreviewFacts {
            preview_edited: true,
            ..hidden
        };
        assert_eq!(preview_work(&edited), PreviewWork::Lay(LayoutSource::Full));
    }

    #[test]
    fn the_cheapest_layout_that_still_fits_is_chosen() {
        let typed = PreviewFacts {
            text_changed: true,
            partial_parse: true,
            background_layout: true,
            ..quiet()
        };
        assert_eq!(preview_work(&typed), PreviewWork::Lay(LayoutSource::Background));
        let with_caret = PreviewFacts {
            has_caret: true,
            ..typed
        };
        assert_eq!(preview_work(&with_caret), PreviewWork::Lay(LayoutSource::Partial));
        let images_arrived = PreviewFacts {
            stale_layout: true,
            ..with_caret
        };
        assert_eq!(preview_work(&images_arrived), PreviewWork::Lay(LayoutSource::Full));
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
    fn only_a_large_new_text_waits_for_the_background() {
        assert!(updates_at_once(Invalidate::Text, 0));
        assert!(updates_at_once(Invalidate::Text, BACKGROUND_FROM - 1));
        assert!(!updates_at_once(Invalidate::Text, BACKGROUND_FROM));
        assert!(updates_at_once(Invalidate::Preview, BACKGROUND_FROM));
        for why in [
            Invalidate::Zoom,
            Invalidate::Appearance,
            Invalidate::Arrivals,
            Invalidate::Layout,
        ] {
            assert!(!updates_at_once(why, 0), "{why:?}");
        }
    }
}
