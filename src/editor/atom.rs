//! The popover that edits a formula or diagram: building blocks, syntax colors, an error line and, for formulas,
//! the visual editor.

use std::cell::{Cell, RefCell};
use std::ops::Range;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSAccessibility, NSAutoresizingMaskOptions, NSBezelStyle, NSButton, NSColor, NSControlSize, NSFont,
    NSFontWeightRegular, NSLineBreakMode, NSPopUpButton, NSPopover, NSPopoverBehavior, NSScrollView,
    NSSegmentSwitchTracking, NSSegmentedControl, NSTextField, NSTextInputClient, NSTextView, NSView, NSViewController,
};
use objc2_foundation::{NSArray, NSPoint, NSRange, NSRect, NSRectEdge, NSSize, NSUndoManager, NSUserDefaults};

use super::attributes::{Attribute, add_attribute};
use super::layers::ForAtoms;
use super::style::token_color;
use super::update::render_key;
use super::{js, mathfield};
use crate::i18n::t;
use crate::markdown::code;
use crate::markdown::doc::figure_at;
use crate::markdown::snippets::{self, DIRECTIONS, DiagramAction, MermaidKind, Shape};
use crate::markdown::text::{Byte, Direction, TextAt, U16, Utf16Map, bytes, units};
use crate::preview::map::preview_offset;
use crate::util::constant;
use crate::util::{byte_range_to_ns, from_nsrange, insert_text, ns, storage};

mod edit;

pub(super) use edit::atom_at_preview;
use edit::atom_content;

const DIAGRAM_ACTIONS: [DiagramAction; 21] = [
    DiagramAction::Node(Shape::Rectangle),
    DiagramAction::Node(Shape::Rounded),
    DiagramAction::Node(Shape::Diamond),
    DiagramAction::Node(Shape::Circle),
    DiagramAction::Node(Shape::Database),
    DiagramAction::Edge,
    DiagramAction::LabeledEdge,
    DiagramAction::Group,
    DiagramAction::Direction("TD"),
    DiagramAction::Direction("LR"),
    DiagramAction::Direction("BT"),
    DiagramAction::Direction("RL"),
    DiagramAction::Participant,
    DiagramAction::Message,
    DiagramAction::Reply,
    DiagramAction::Note,
    DiagramAction::Loop,
    DiagramAction::Alternative,
    DiagramAction::Section,
    DiagramAction::Task,
    DiagramAction::Milestone,
];

pub(super) const VISUAL_DEFAULT: &str = "FormulaEditorCode";

/// What a building-block control puts into the code: an entry of one of the lists, carried in the control's tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BuildingBlock {
    /// An entry of `snippets::FORMULA_BUTTONS`.
    Formula(usize),
    /// An entry of `snippets::GREEK`.
    Greek(usize),
    /// An entry of `snippets::RELATIONS`.
    Relation(usize),
    /// An entry of `DIAGRAM_ACTIONS`.
    Diagram(usize),
}

/// Each list counts in tags of its own, so no entry of one can be read as an entry of another.
const TAGS_PER_LIST: usize = 1000;
const _: () = assert!(
    snippets::FORMULA_BUTTONS.len() < TAGS_PER_LIST
        && snippets::GREEK.len() < TAGS_PER_LIST
        && snippets::RELATIONS.len() < TAGS_PER_LIST
        && DIAGRAM_ACTIONS.len() < TAGS_PER_LIST
);

impl BuildingBlock {
    pub(super) fn tag(self) -> isize {
        let (list, n) = match self {
            BuildingBlock::Formula(n) => (0, n),
            BuildingBlock::Greek(n) => (1, n),
            BuildingBlock::Relation(n) => (2, n),
            BuildingBlock::Diagram(n) => (3, n),
        };
        crate::util::to_isize(list * TAGS_PER_LIST + n)
    }

    pub(super) fn of_tag(tag: isize) -> Option<BuildingBlock> {
        let tag = usize::try_from(tag).ok()?;
        let n = tag % TAGS_PER_LIST;
        match tag / TAGS_PER_LIST {
            0 => Some(BuildingBlock::Formula(n)),
            1 => Some(BuildingBlock::Greek(n)),
            2 => Some(BuildingBlock::Relation(n)),
            3 => Some(BuildingBlock::Diagram(n)),
            _ => None,
        }
    }
}

/// The tag of the control for `action`; every action the controls offer stands in `DIAGRAM_ACTIONS`, which a test holds.
pub(super) fn diagram_tag(action: DiagramAction) -> isize {
    let n = DIAGRAM_ACTIONS
        .iter()
        .position(|a| *a == action)
        .unwrap_or(DIAGRAM_ACTIONS.len());
    BuildingBlock::Diagram(n).tag()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AtomKind {
    Formula { display: bool },
    Diagram(crate::markdown::doc::DiagramKind),
}

/// An open popover editing the source of a formula or diagram in place.
struct AtomEdit {
    popover: Retained<NSPopover>,
    view: Retained<NSTextView>,
    start: Byte,
    len: Cell<usize>,
    original: String,
    cancelled: Cell<bool>,
    range_start: Byte,
    kind: AtomKind,
    diagram: crate::markdown::snippets::MermaidKind,
    status: Retained<objc2_app_kit::NSTextField>,
    scroll: Retained<NSScrollView>,
    editor_frame: NSRect,
    visual: Cell<bool>,
    /// Text just inserted for a new formula, removed again if the editor closes without one.
    inserted: Cell<Option<(Byte, Byte)>>,
    /// Steps within the editor, which end with it. In the document's undo history they would point at a text that the
    /// editor mirrors into the source without a step of its own.
    undo: Retained<NSUndoManager>,
}

#[derive(Default)]
pub(super) struct Atoms {
    edit: RefCell<Option<AtomEdit>>,
    /// Start of the formula or diagram whose editor just closed; the caret resting on it does not reopen it.
    closed: Cell<Option<Byte>>,
}

impl Atoms {
    /// Whether the caret resting on the formula starting at `start` opens its editor.
    fn opens_at(&self, start: Byte) -> bool {
        self.closed.get() != Some(start)
    }

    fn forget_closed(&self) {
        self.closed.set(None);
    }
}

pub(super) struct AtomEditor<'a> {
    host: ForAtoms<'a>,
    state: &'a Atoms,
}

impl<'a> AtomEditor<'a> {
    pub(super) fn new(host: ForAtoms<'a>, state: &'a Atoms) -> Self {
        AtomEditor { host, state }
    }
}

impl AtomKind {
    fn undo_name(self) -> &'static str {
        match self {
            AtomKind::Formula { .. } => t("Formel bearbeiten"),
            AtomKind::Diagram(_) => t("Diagramm bearbeiten"),
        }
    }
}

/// Lays out the building-block controls from left to right.
struct Row<'a> {
    view: Retained<NSView>,
    x: f64,
    target: &'a AnyObject,
    mtm: MainThreadMarker,
}

impl Row<'_> {
    fn place(&mut self, control: &NSView) {
        let size = control.frame().size;
        control.setFrameOrigin(NSPoint::new(self.x, (30.0 - size.height) / 2.0));
        self.view.addSubview(control);
        self.x += size.width + 4.0;
    }

    fn button(&mut self, title: &str, tip: &str, tag: isize) {
        // SAFETY: The target is the window's controller, which outlives the popover holding the button.
        let b = unsafe {
            NSButton::buttonWithTitle_target_action(&ns(title), Some(self.target), Some(sel!(mdAtomSnippet:)), self.mtm)
        };
        b.setBezelStyle(NSBezelStyle::Push);
        b.setControlSize(NSControlSize::Small);
        b.setFont(Some(&NSFont::systemFontOfSize(NSFont::smallSystemFontSize())));
        b.setTag(tag);
        b.setToolTip(Some(&ns(tip)));
        b.sizeToFit();
        self.place(&b);
    }

    fn pull_down(&mut self, title: &str, tip: &str, items: &[(&str, isize)]) {
        let p = NSPopUpButton::initWithFrame_pullsDown(
            NSPopUpButton::alloc(self.mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(60.0, 24.0)),
            true,
        );
        p.setControlSize(NSControlSize::Small);
        p.setFont(Some(&NSFont::systemFontOfSize(NSFont::smallSystemFontSize())));
        p.addItemWithTitle(&ns(title));
        if let Some(menu) = p.menu() {
            for (label, tag) in items {
                let item = crate::util::tagged(self.mtm, label, sel!(mdAtomSnippet:), "", *tag);
                // SAFETY: The target is the window's controller, which outlives the popover holding the menu.
                unsafe { item.setTarget(Some(self.target)) };
                menu.addItem(&item);
            }
        }
        p.setToolTip(Some(&ns(tip)));
        p.sizeToFit();
        self.place(&p);
    }
}

/// The buttons above the code: what a formula is made of, or for Mermaid the parts of its kind of diagram.
fn building_blocks(row: &mut Row, kind: AtomKind, diagram_kind: MermaidKind) {
    match kind {
        AtomKind::Formula { .. } => {
            for (n, s) in snippets::FORMULA_BUTTONS.iter().enumerate() {
                row.button(t(s.label), t(s.tip), BuildingBlock::Formula(n).tag());
            }
            let greek: Vec<(&str, isize)> = snippets::GREEK
                .iter()
                .enumerate()
                .map(|(n, (l, _))| (*l, BuildingBlock::Greek(n).tag()))
                .collect();
            row.pull_down("α β", t("Griechische Buchstaben"), &greek);
            let relations: Vec<(&str, isize)> = snippets::RELATIONS
                .iter()
                .enumerate()
                .map(|(n, (l, _))| (*l, BuildingBlock::Relation(n).tag()))
                .collect();
            row.pull_down("≤ →", t("Relationen, Operatoren und Pfeile"), &relations);
        }
        AtomKind::Diagram(_) => match diagram_kind {
            MermaidKind::Flowchart => {
                let shapes: Vec<(&str, isize)> = Shape::ALL
                    .iter()
                    .map(|(s, l)| (t(l), diagram_tag(DiagramAction::Node(*s))))
                    .collect();
                row.pull_down(t("Knoten"), t("Knoten einfügen"), &shapes);
                row.button(
                    t("Verbindung"),
                    t("Neuen Knoten an den letzten anschließen"),
                    diagram_tag(DiagramAction::Edge),
                );
                row.button(
                    t("Mit Beschriftung"),
                    t("Beschriftete Verbindung zu einem neuen Knoten"),
                    diagram_tag(DiagramAction::LabeledEdge),
                );
                row.button(
                    t("Gruppe"),
                    t("Knoten in einem Rahmen gruppieren"),
                    diagram_tag(DiagramAction::Group),
                );
                let directions: Vec<(&str, isize)> = DIRECTIONS
                    .iter()
                    .map(|(d, l)| (t(l), diagram_tag(DiagramAction::Direction(d))))
                    .collect();
                row.pull_down(t("Richtung"), t("Richtung des Diagramms"), &directions);
            }
            MermaidKind::Sequence => {
                for (label, tip, action) in [
                    (t("Teilnehmer"), t("Teilnehmer hinzufügen"), DiagramAction::Participant),
                    (
                        t("Nachricht"),
                        t("Nachricht zwischen zwei Teilnehmern"),
                        DiagramAction::Message,
                    ),
                    (t("Antwort"), t("Antwort zurück"), DiagramAction::Reply),
                    (t("Notiz"), t("Notiz neben einem Teilnehmer"), DiagramAction::Note),
                    (t("Schleife"), t("Wiederholte Nachrichten"), DiagramAction::Loop),
                    (t("Alternative"), t("Fallunterscheidung"), DiagramAction::Alternative),
                ] {
                    row.button(label, tip, diagram_tag(action));
                }
            }
            MermaidKind::Gantt => {
                for (label, tip, action) in [
                    (t("Abschnitt"), t("Neuer Abschnitt"), DiagramAction::Section),
                    (t("Aufgabe"), t("Aufgabe nach der letzten"), DiagramAction::Task),
                    (
                        t("Meilenstein"),
                        t("Meilenstein nach der letzten Aufgabe"),
                        DiagramAction::Milestone,
                    ),
                ] {
                    row.button(label, tip, diagram_tag(action));
                }
            }
            MermaidKind::Other => {}
        },
    }
}

/// The switch between the visual editor and LaTeX, at the right end of the status line.
fn mode_switch(target: &AnyObject, visual: bool, width: f64, mtm: MainThreadMarker) -> Retained<NSSegmentedControl> {
    let labels = NSArray::from_retained_slice(&[ns(t("Visuell")), ns("LaTeX")]);
    // SAFETY: The target is the window's controller, which outlives the popover holding the control.
    let mode = unsafe {
        NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
            &labels,
            NSSegmentSwitchTracking::SelectOne,
            Some(target),
            Some(sel!(mdAtomMode:)),
            mtm,
        )
    };
    mode.setControlSize(NSControlSize::Small);
    mode.setFont(Some(&NSFont::systemFontOfSize(NSFont::smallSystemFontSize())));
    mode.setSelectedSegment(if visual { 0 } else { 1 });
    mode.setToolTip(Some(&ns(t("Formel grafisch oder als LaTeX-Code bearbeiten"))));
    mode.sizeToFit();
    mode.setFrameOrigin(NSPoint::new(width - mode.frame().size.width - 8.0, 2.0));
    mode.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
    mode
}

#[cfg(feature = "selftest")]
impl AtomEditor<'_> {
    pub(super) fn is_open(&self) -> bool {
        self.state.edit.borrow().is_some()
    }

    pub(super) fn code_view(&self) -> Option<Retained<NSTextView>> {
        self.state.edit.borrow().as_ref().map(|e| e.view.clone())
    }

    pub(super) fn open_popover(&self) -> Option<Retained<NSPopover>> {
        self.state.edit.borrow().as_ref().map(|e| e.popover.clone())
    }

    pub(super) fn status_text(&self) -> Option<String> {
        self.state
            .edit
            .borrow()
            .as_ref()
            .map(|e| e.status.stringValue().to_string())
    }

    pub(super) fn mark_cancelled(&self) {
        if let Some(edit) = self.state.edit.borrow().as_ref() {
            edit.cancelled.set(true);
        }
    }
}

impl AtomEditor<'_> {
    /// The undo history of the editor's code view; every other view has the document's.
    pub(super) fn undo_manager_of(&self, view: &NSTextView) -> Option<Retained<NSUndoManager>> {
        match self.state.edit.borrow().as_ref() {
            Some(edit) if std::ptr::eq(&*edit.view, view) => Some(edit.undo.clone()),
            _ => None,
        }
    }

    pub(super) fn cell_clicked(&self, view: &NSTextView, frame: NSRect, index: usize) {
        if self.host.panes().is_preview(Some(view)) {
            self.edit_at_preview(crate::preview::map::Shown::new(index), Some(frame));
        }
    }

    pub(super) fn popover_closed(&self, object: Option<&AnyObject>) {
        let current = self.state.edit.borrow().as_ref().map(|e| e.popover.clone());
        if let (Some(current), Some(object)) = (current, object)
            && std::ptr::eq(&*current as &AnyObject, object)
        {
            self.editor_closed();
        }
    }

    pub(super) fn open(&self, range: Range<Byte>, frame: Option<NSRect>) {
        if self.state.edit.borrow().is_some() {
            return;
        }
        let mtm = self.host.mtm();
        let (content, kind, original, source_at) = self.host.with_text(|snap, st| {
            let (content, kind) = atom_content(st, &snap.text, &range);
            (
                content.clone(),
                kind,
                snap.text.at(content).to_string(),
                snap.u16_at(range.start),
            )
        });
        let width = 560.0;
        let editor_height = if matches!(kind, AtomKind::Diagram(_)) {
            220.0
        } else {
            120.0
        };
        let size = NSSize::new(width, 30.0 + editor_height + 26.0);
        let root = NSView::initWithFrame(NSView::alloc(mtm), NSRect::new(NSPoint::new(0.0, 0.0), size));

        let target: &AnyObject = self.host.object();
        let mut row = Row {
            view: NSView::initWithFrame(
                NSView::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, size.height - 32.0), NSSize::new(width, 30.0)),
            ),
            x: 8.0,
            target,
            mtm,
        };
        // Building blocks exist for Mermaid only.
        let diagram_kind = match kind {
            AtomKind::Diagram(crate::markdown::doc::DiagramKind::Mermaid) => snippets::diagram_kind(&original),
            _ => MermaidKind::Other,
        };
        building_blocks(&mut row, kind, diagram_kind);
        row.view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin,
        );
        root.addSubview(&row.view);

        let editor_frame = NSRect::new(NSPoint::new(0.0, 26.0), NSSize::new(width, editor_height));
        let (scroll, view) = self.code_view_in_scroll_view(editor_frame, &original, kind);
        root.addSubview(&scroll);

        let status = NSTextField::labelWithString(&ns(""), mtm);
        status.setFont(Some(&NSFont::systemFontOfSize(NSFont::smallSystemFontSize())));
        status.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        status.setFrame(NSRect::new(NSPoint::new(10.0, 5.0), NSSize::new(width - 20.0, 16.0)));
        status.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        root.addSubview(&status);

        let visual = matches!(kind, AtomKind::Formula { .. })
            && !NSUserDefaults::standardUserDefaults().boolForKey(&ns(VISUAL_DEFAULT));
        if matches!(kind, AtomKind::Formula { .. }) {
            let mode = mode_switch(target, visual, width, mtm);
            status.setFrameSize(NSSize::new(mode.frame().origin.x - 20.0, 16.0));
            root.addSubview(&mode);
        }

        let popover = self.popover(&root, size);

        let (anchor_view, rect) = self.anchor(range.start, source_at, frame);
        *self.state.edit.borrow_mut() = Some(AtomEdit {
            popover: popover.clone(),
            view: view.clone(),
            start: content.start,
            len: Cell::new(units(&content)),
            original,
            cancelled: Cell::new(false),
            range_start: range.start,
            kind,
            diagram: diagram_kind,
            status,
            scroll,
            editor_frame,
            visual: Cell::new(false),
            inserted: Cell::new(None),
            undo: NSUndoManager::new(mtm),
        });
        self.color_code();
        self.show_status();
        popover.showRelativeToRect_ofView_preferredEdge(rect, anchor_view, NSRectEdge::MaxY);
        if !popover.isShown() && !crate::test_hooks::keeps_unshown_editor() {
            let never_shown = self.state.edit.borrow_mut().take();
            drop(never_shown);
            return;
        }
        if let Some(w) = view.window() {
            w.makeFirstResponder(Some(&view));
        }
        // SAFETY: Select All accepts a missing sender.
        unsafe { view.selectAll(None) };
        if visual {
            self.set_visual(true);
        }
    }

    fn popover(&self, root: &NSView, size: NSSize) -> Retained<NSPopover> {
        let mtm = self.host.mtm();
        let controller = NSViewController::new(mtm);
        controller.setView(root);
        let popover = NSPopover::new(mtm);
        popover.setBehavior(NSPopoverBehavior::Transient);
        popover.setAnimates(crate::test_hooks::ANIMATES);
        popover.setContentViewController(Some(&controller));
        popover.setContentSize(size);
        popover.setDelegate(Some(ProtocolObject::from_ref(self.host.object())));
        popover
    }

    /// Where the popover points: at a given frame, or at the formula in whichever view shows it.
    fn anchor(&self, start: Byte, source_at: U16, frame: Option<NSRect>) -> (&NSTextView, NSRect) {
        let panes = self.host.panes();
        let views = panes.for_appkit();
        if let Some(frame) = frame {
            return (views.preview, frame);
        }
        if panes.preview_visible() {
            let p = self
                .host
                .model()
                .read(move |st| preview_offset(st.layout(), st.index(), start));
            // NSPopover refuses to appear at a spot that is scrolled out of view.
            views.preview.scrollRangeToVisible(NSRange::new(p.get(), 1));
            (views.preview, panes.preview().char_rect(p.get()))
        } else {
            views.source.scrollRangeToVisible(NSRange::new(source_at.get(), 1));
            (views.source, panes.source().char_rect(source_at.get()))
        }
    }

    /// The text view for a formula's or diagram's code, in its scroll view.
    fn code_view_in_scroll_view(
        &self,
        editor_frame: NSRect,
        original: &str,
        kind: AtomKind,
    ) -> (Retained<NSScrollView>, Retained<NSTextView>) {
        let mtm = self.host.mtm();
        let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), editor_frame);
        scroll.setHasVerticalScroller(true);
        scroll.setDrawsBackground(false);
        scroll.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        let view = NSTextView::initWithFrame(
            NSTextView::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), editor_frame.size),
        );
        view.setRichText(false);
        view.setAllowsUndo(true);
        view.setVerticallyResizable(true);
        view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        view.setTextContainerInset(NSSize::new(10.0, 10.0));
        view.setAutomaticQuoteSubstitutionEnabled(false);
        view.setAutomaticDashSubstitutionEnabled(false);
        view.setAutomaticTextReplacementEnabled(false);
        view.setContinuousSpellCheckingEnabled(false);
        view.setDrawsBackground(false);
        view.setFont(Some(&code_font()));
        view.setString(&ns(original));
        view.setAccessibilityLabel(Some(&ns(if matches!(kind, AtomKind::Diagram(_)) {
            t("Mermaid-Code")
        } else {
            t("LaTeX-Formel")
        })));
        view.setDelegate(Some(ProtocolObject::from_ref(self.host.object())));
        scroll.setDocumentView(Some(&view));
        (scroll, view)
    }

    /// Switches a formula popover between the visual editor and the LaTeX code.
    pub(super) fn set_visual(&self, visual: bool) {
        let (root, frame, scroll, latex, view) = {
            let edit = self.state.edit.borrow();
            let Some(e) = edit.as_ref() else { return };
            if !matches!(e.kind, AtomKind::Formula { .. }) {
                return;
            }
            e.visual.set(visual);
            (
                // SAFETY: The popover still shows the scroll view, so its superview is alive.
                unsafe { e.scroll.superview() },
                e.editor_frame,
                e.scroll.clone(),
                e.view.string().to_string(),
                e.view.clone(),
            )
        };
        scroll.setHidden(visual);
        if visual {
            let field = mathfield::field(self.host.mtm());
            if let Some(root) = root {
                field.web.setFrame(frame);
                field.web.setAutoresizingMask(
                    NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
                );
                root.addSubview(&field.web);
            }
            field.web.setHidden(false);
            field.attach(self.host.object(), &latex);
            if let Some(w) = field.web.window() {
                w.makeFirstResponder(Some(&field.web));
            }
        } else {
            mathfield::detach_if_started(self.host.object());
            if let Some(w) = view.window() {
                w.makeFirstResponder(Some(&view));
            }
        }
        self.show_status();
    }

    pub(super) fn mode_changed(&self, segment: isize) {
        let visual = segment == 0;
        NSUserDefaults::standardUserDefaults().setBool_forKey(!visual, &ns(VISUAL_DEFAULT));
        self.set_visual(visual);
    }

    /// The visual editor changed the formula: keep the code view and the document in step.
    pub(super) fn visual_formula_changed(&self, latex: &str) {
        let view = {
            let edit = self.state.edit.borrow();
            let Some(e) = edit.as_ref() else { return };
            e.view.clone()
        };
        if view.string().to_string() == latex {
            return;
        }
        view.setString(&ns(latex));
        self.text_changed();
    }

    pub(super) fn cancel(&self) {
        let popover = {
            let edit = self.state.edit.borrow();
            edit.as_ref().map(|e| {
                e.cancelled.set(true);
                e.popover.clone()
            })
        };
        if let Some(p) = popover {
            p.close();
        }
    }

    /// A building-block control was used; the tag says which one.
    pub(super) fn snippet(&self, tag: isize) {
        let (kind, visual, view) = {
            let edit = self.state.edit.borrow();
            let Some(e) = edit.as_ref() else { return };
            (e.kind, e.visual.get(), e.view.clone())
        };
        let Some(block) = BuildingBlock::of_tag(tag) else {
            return;
        };
        if let BuildingBlock::Diagram(n) = block {
            let Some(action) = DIAGRAM_ACTIONS.get(n) else {
                return;
            };
            let code = view.string().to_string();
            let map = Utf16Map::new(&code);
            let caret = map.to_byte(U16::new(view.selectedRange().location));
            let edit = snippets::diagram_snippet(&code, caret.get(), *action, &today());
            let select = edit.range.start + edit.select.start..edit.range.start + edit.select.end;
            replace_in_view(&view, &bytes(edit.range), &edit.text, Some(bytes(select)));
            return;
        }
        let template = match block {
            BuildingBlock::Formula(n) => snippets::FORMULA_BUTTONS.get(n).map(|s| s.text),
            BuildingBlock::Greek(n) => snippets::GREEK.get(n).map(|g| g.1),
            BuildingBlock::Relation(n) => snippets::RELATIONS.get(n).map(|r| r.1),
            BuildingBlock::Diagram(_) => None,
        };
        let Some(template) = template else { return };
        if !matches!(kind, AtomKind::Formula { .. }) {
            return;
        }
        if visual {
            mathfield::field(self.host.mtm()).insert(&snippets::mathlive_template(template));
            return;
        }
        let code = view.string().to_string();
        let map = Utf16Map::new(&code);
        let sel = from_nsrange(view.selectedRange());
        let sel = map.to_byte(sel.start)..map.to_byte(sel.end);
        let (text, next) = snippets::formula_insert(template, code.at(sel.clone()));
        let select = match next {
            Some(p) => sel.start + p.start..sel.start + p.end,
            None => sel.start + text.len()..sel.start + text.len(),
        };
        replace_in_view(&view, &sel, &text, Some(select));
    }

    /// Tab and Shift-Tab in the formula code jump between placeholders.
    pub(super) fn tab(&self, way: Direction) -> bool {
        let view = {
            let edit = self.state.edit.borrow();
            match edit.as_ref() {
                Some(e) if matches!(e.kind, AtomKind::Formula { .. }) => e.view.clone(),
                _ => return false,
            }
        };
        let code = view.string().to_string();
        let map = Utf16Map::new(&code);
        let sel = from_nsrange(view.selectedRange());
        let from = map.to_byte(if way == Direction::Backward { sel.start } else { sel.end });
        let Some(next) = snippets::next_placeholder(&code, from.get(), way) else {
            return false;
        };
        view.setSelectedRange(byte_range_to_ns(&map, &bytes(next)));
        true
    }

    /// Colors the TeX or Mermaid code in the popover. Attribute changes stay out of the undo history.
    pub(super) fn color_code(&self) {
        let (view, lang) = {
            let edit = self.state.edit.borrow();
            let Some(e) = edit.as_ref() else { return };
            (
                e.view.clone(),
                match e.kind {
                    AtomKind::Diagram(crate::markdown::doc::DiagramKind::Mermaid) => "mermaid",
                    AtomKind::Diagram(_) => "",
                    AtomKind::Formula { .. } => "tex",
                },
            )
        };
        let text = view.string().to_string();
        let store = storage(&view);
        let map = Utf16Map::new(&text);
        if map.len_u16() != store.length() {
            return;
        }
        let all = NSRange::new(0, store.length());
        crate::util::editing(&store, || {
            add_attribute(&store, &Attribute::font(&code_font()), all);
            add_attribute(&store, &Attribute::foreground(&NSColor::textColor()), all);
            for (range, token) in code::tokens(lang, &text) {
                add_attribute(
                    &store,
                    &Attribute::foreground(&token_color(token.into())),
                    byte_range_to_ns(&map, &bytes(range)),
                );
            }
        });
    }

    /// Shows the MathJax or Mermaid error for the current code, or a hint when there is none.
    pub(super) fn show_status(&self) {
        let Some((status, kind, diagram, visual)) = self
            .state
            .edit
            .borrow()
            .as_ref()
            .map(|e| (e.status.clone(), e.kind, e.diagram, e.visual.get()))
        else {
            return;
        };
        let outcome = self.render_key().and_then(|k| js::outcome(&k));
        let (message, error) = match outcome {
            Some(Err(e)) => (format!("⚠︎ {}", e.lines().next().unwrap_or("")), true),
            _ => {
                let hint = match (kind, diagram) {
                    (AtomKind::Diagram(crate::markdown::doc::DiagramKind::Mermaid), MermaidKind::Other) => {
                        t("Bausteine gibt es für Fluss-, Sequenz- und Gantt-Diagramme")
                    }
                    (AtomKind::Formula { .. }, _) if !visual => t("Tab springt zum nächsten □"),
                    _ => "",
                };
                (hint.to_string(), false)
            }
        };
        status.setStringValue(&ns(&message));
        let color = if error {
            NSColor::systemRedColor()
        } else {
            NSColor::secondaryLabelColor()
        };
        status.setTextColor(Some(&color));
        status.setToolTip(error.then(|| ns(&message)).as_deref());
    }

    /// Render key of the formula or diagram being edited, as the preview requests it.
    fn render_key(&self) -> Option<js::Key> {
        let start = self.state.edit.borrow().as_ref()?.start;
        let dark = self.host.is_dark();
        let macros = self.host.model().read(|st| st.macros());
        self.host
            .model()
            .read(move |st| figure_at(st.doc(), start).map(|(_, figure)| render_key(&figure, dark, &macros)))
    }
}

fn code_font() -> Retained<NSFont> {
    NSFont::monospacedSystemFontOfSize_weight(13.0, constant!(NSFontWeightRegular))
}

/// Replaces a byte range of the popover code like typing would, so it lands in the popover's undo history.
fn replace_in_view(view: &NSTextView, range: &Range<Byte>, text: &str, select: Option<Range<Byte>>) {
    let code = view.string().to_string();
    let map = Utf16Map::new(&code);
    if let Some(w) = view.window() {
        w.makeFirstResponder(Some(view));
    }
    insert_text(view, &ns(text), byte_range_to_ns(&map, range));
    if let Some(select) = select {
        let new = view.string().to_string();
        let new_map = Utf16Map::new(&new);
        if select.end <= new.end() {
            view.setSelectedRange(byte_range_to_ns(&new_map, &select));
        }
    }
}

/// Today's date as `YYYY-MM-DD` in the user's time zone, for new Gantt tasks.
fn today() -> String {
    crate::util::now("yyyy-MM-dd")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_caret_resting_on_a_closed_editor_opens_it_only_after_leaving() {
        let atoms = Atoms::default();
        assert!(atoms.opens_at(Byte::new(3)));
        atoms.closed.set(Some(Byte::new(3)));
        assert!(!atoms.opens_at(Byte::new(3)));
        assert!(atoms.opens_at(Byte::new(9)), "another formula opens right away");
        atoms.forget_closed();
        assert!(atoms.opens_at(Byte::new(3)));
    }

    #[test]
    fn a_building_block_comes_back_from_its_tag_and_no_list_reaches_into_another() {
        let all: Vec<BuildingBlock> = (0..snippets::FORMULA_BUTTONS.len())
            .map(BuildingBlock::Formula)
            .chain((0..snippets::GREEK.len()).map(BuildingBlock::Greek))
            .chain((0..snippets::RELATIONS.len()).map(BuildingBlock::Relation))
            .chain((0..DIAGRAM_ACTIONS.len()).map(BuildingBlock::Diagram))
            .collect();
        for block in &all {
            assert_eq!(BuildingBlock::of_tag(block.tag()), Some(*block));
        }
        let tags: std::collections::HashSet<isize> = all.iter().map(|b| b.tag()).collect();
        assert_eq!(tags.len(), all.len());
        assert_eq!(BuildingBlock::of_tag(-1), None);
        assert_eq!(BuildingBlock::of_tag(4 * crate::util::to_isize(TAGS_PER_LIST)), None);
    }

    #[test]
    fn every_diagram_action_a_control_offers_has_a_tag_of_its_own() {
        let offered = Shape::ALL
            .iter()
            .map(|(s, _)| DiagramAction::Node(*s))
            .chain(DIRECTIONS.iter().map(|(d, _)| DiagramAction::Direction(d)));
        for action in offered {
            assert!(DIAGRAM_ACTIONS.contains(&action), "{action:?}");
        }
    }
}
