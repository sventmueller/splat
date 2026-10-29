//! The popover that edits a formula or diagram: building blocks, syntax colors, an error line and, for formulas,
//! the visual editor.

use std::cell::Cell;
use std::ops::Range;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSAccessibility, NSAutoresizingMaskOptions, NSBezelStyle, NSButton, NSColor, NSControlSize, NSFont,
    NSFontAttributeName, NSFontWeightRegular, NSForegroundColorAttributeName, NSLineBreakMode, NSPopUpButton,
    NSPopover, NSPopoverBehavior, NSScrollView, NSSegmentSwitchTracking, NSSegmentedControl, NSTextField,
    NSTextInputClient, NSTextView, NSView, NSViewController,
};
use objc2_foundation::{NSArray, NSPoint, NSRange, NSRect, NSRectEdge, NSSize, NSUserDefaults};

use super::scroll::char_rect;
use super::style::token_color;
use super::update::render_key;
use super::{AtomEdit, EditorController, js, mathfield};
use crate::i18n::t;
use crate::markdown::code;
use crate::markdown::doc::figure_at;
use crate::markdown::snippets::{self, DIRECTIONS, DiagramAction, DiagramKind, Shape};
use crate::markdown::text::Utf16Map;
use crate::preview::map::preview_offset;
use crate::util::constant;
use crate::util::{byte_range_to_ns, from_nsrange, ns, storage};

const GREEK_TAG: isize = 100;
const RELATION_TAG: isize = 200;
const DIAGRAM_TAG: isize = 1000;

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

const VISUAL_DEFAULT: &str = "FormulaEditorCode";

pub(super) fn diagram_tag(action: DiagramAction) -> isize {
    DIAGRAM_TAG + DIAGRAM_ACTIONS.iter().position(|a| *a == action).unwrap_or(0) as isize
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AtomKind {
    Formula { display: bool },
    Diagram(crate::markdown::doc::DiagramKind),
}

impl AtomKind {
    pub fn undo_name(self) -> &'static str {
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

impl EditorController {
    pub(super) fn open_atom_editor(&self, range: Range<usize>, frame: Option<NSRect>) {
        if self.ivars().atoms.edit.borrow().is_some() {
            return;
        }
        let mtm = self.mtm();
        let (content, kind, original, source_at) = {
            let snap = self.snapshot();
            let (content, kind) = self.atom_content(&snap.text, &range);
            (
                content.clone(),
                kind,
                snap.text[content].to_string(),
                snap.u16_at(range.start),
            )
        };
        let width = 560.0;
        let editor_height = if matches!(kind, AtomKind::Diagram(_)) {
            220.0
        } else {
            120.0
        };
        let size = NSSize::new(width, 30.0 + editor_height + 26.0);
        let root = NSView::initWithFrame(NSView::alloc(mtm), NSRect::new(NSPoint::new(0.0, 0.0), size));

        let target: &AnyObject = self;
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
            _ => DiagramKind::Other,
        };
        match kind {
            AtomKind::Formula { .. } => {
                for (n, s) in snippets::FORMULA_BUTTONS.iter().enumerate() {
                    row.button(t(s.label), t(s.tip), n as isize);
                }
                let greek: Vec<(&str, isize)> = snippets::GREEK
                    .iter()
                    .enumerate()
                    .map(|(n, (l, _))| (*l, GREEK_TAG + n as isize))
                    .collect();
                row.pull_down("α β", t("Griechische Buchstaben"), &greek);
                let relations: Vec<(&str, isize)> = snippets::RELATIONS
                    .iter()
                    .enumerate()
                    .map(|(n, (l, _))| (*l, RELATION_TAG + n as isize))
                    .collect();
                row.pull_down("≤ →", t("Relationen, Operatoren und Pfeile"), &relations);
            }
            AtomKind::Diagram(_) => match diagram_kind {
                DiagramKind::Flowchart => {
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
                DiagramKind::Sequence => {
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
                DiagramKind::Gantt => {
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
                DiagramKind::Other => {}
            },
        }
        row.view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin,
        );
        root.addSubview(&row.view);

        let editor_frame = NSRect::new(NSPoint::new(0.0, 26.0), NSSize::new(width, editor_height));
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
        view.setString(&ns(&original));
        view.setAccessibilityLabel(Some(&ns(if matches!(kind, AtomKind::Diagram(_)) {
            t("Mermaid-Code")
        } else {
            t("LaTeX-Formel")
        })));
        view.setDelegate(Some(ProtocolObject::from_ref(self)));
        scroll.setDocumentView(Some(&view));
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
            let w = mode.frame().size.width;
            mode.setFrameOrigin(NSPoint::new(width - w - 8.0, 2.0));
            mode.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
            root.addSubview(&mode);
            status.setFrameSize(NSSize::new(width - w - 28.0, 16.0));
        }

        let controller = NSViewController::new(mtm);
        controller.setView(&root);
        let popover = NSPopover::new(mtm);
        popover.setBehavior(NSPopoverBehavior::Transient);
        popover.setContentViewController(Some(&controller));
        popover.setContentSize(size);
        popover.setDelegate(Some(ProtocolObject::from_ref(self)));

        let (anchor_view, rect): (&NSTextView, NSRect) = if let Some(frame) = frame {
            (self.preview(), frame)
        } else if self.preview_visible() {
            let st = self.ivars().state.borrow();
            let p = preview_offset(&st.layout, &st.index, range.start);
            drop(st);
            // NSPopover refuses to appear at a spot that is scrolled out of view.
            self.preview().scrollRangeToVisible(NSRange::new(p, 1));
            (self.preview(), char_rect(self.preview(), p))
        } else {
            self.source().scrollRangeToVisible(NSRange::new(source_at, 1));
            (self.source(), char_rect(self.source(), source_at))
        };
        *self.ivars().atoms.edit.borrow_mut() = Some(AtomEdit {
            popover: popover.clone(),
            view: view.clone(),
            start: content.start,
            len: Cell::new(content.len()),
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
            marked_edited: Cell::new(false),
        });
        self.color_atom_code();
        self.refresh_atom_status();
        popover.showRelativeToRect_ofView_preferredEdge(rect, anchor_view, NSRectEdge::MaxY);
        if !popover.isShown() {
            self.ivars().atoms.edit.borrow_mut().take();
            return;
        }
        if let Some(w) = view.window() {
            w.makeFirstResponder(Some(&view));
        }
        // SAFETY: Select All accepts a missing sender.
        unsafe { view.selectAll(None) };
        if visual {
            self.set_atom_visual(true);
        }
    }

    /// Switches a formula popover between the visual editor and the LaTeX code.
    pub(super) fn set_atom_visual(&self, visual: bool) {
        let (root, frame, scroll, latex, view) = {
            let edit = self.ivars().atoms.edit.borrow();
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
        let field = mathfield::field(self.mtm());
        scroll.setHidden(visual);
        if visual {
            if let Some(root) = root {
                field.web.setFrame(frame);
                field.web.setAutoresizingMask(
                    NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
                );
                root.addSubview(&field.web);
            }
            field.web.setHidden(false);
            field.attach(self, &latex);
            if let Some(w) = field.web.window() {
                w.makeFirstResponder(Some(&field.web));
            }
        } else {
            field.detach();
            if let Some(w) = view.window() {
                w.makeFirstResponder(Some(&view));
            }
        }
        self.refresh_atom_status();
    }

    pub(super) fn atom_mode_changed(&self, segment: isize) {
        let visual = segment == 0;
        NSUserDefaults::standardUserDefaults().setBool_forKey(!visual, &ns(VISUAL_DEFAULT));
        self.set_atom_visual(visual);
    }

    /// The visual editor changed the formula: keep the code view and the document in step.
    pub(super) fn visual_formula_changed(&self, latex: &str) {
        let view = {
            let edit = self.ivars().atoms.edit.borrow();
            let Some(e) = edit.as_ref() else { return };
            e.view.clone()
        };
        if view.string().to_string() == latex {
            return;
        }
        view.setString(&ns(latex));
        self.atom_text_changed();
    }

    pub(super) fn cancel_atom_edit(&self) {
        let popover = {
            let edit = self.ivars().atoms.edit.borrow();
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
    pub(super) fn atom_snippet(&self, tag: isize) {
        let (kind, visual, view) = {
            let edit = self.ivars().atoms.edit.borrow();
            let Some(e) = edit.as_ref() else { return };
            (e.kind, e.visual.get(), e.view.clone())
        };
        if tag >= DIAGRAM_TAG {
            let Some(action) = DIAGRAM_ACTIONS.get((tag - DIAGRAM_TAG) as usize) else {
                return;
            };
            let code = view.string().to_string();
            let map = Utf16Map::new(&code);
            let caret = map.to_byte(view.selectedRange().location);
            let edit = snippets::diagram_snippet(&code, caret, *action, &today());
            let select = edit.range.start + edit.select.start..edit.range.start + edit.select.end;
            replace_in_view(&view, &edit.range, &edit.text, Some(select));
            return;
        }
        let template = if tag >= RELATION_TAG {
            snippets::RELATIONS.get((tag - RELATION_TAG) as usize).map(|r| r.1)
        } else if tag >= GREEK_TAG {
            snippets::GREEK.get((tag - GREEK_TAG) as usize).map(|g| g.1)
        } else {
            snippets::FORMULA_BUTTONS.get(tag as usize).map(|s| s.text)
        };
        let Some(template) = template else { return };
        if !matches!(kind, AtomKind::Formula { .. }) {
            return;
        }
        if visual {
            mathfield::field(self.mtm()).insert(&snippets::mathlive_template(template));
            return;
        }
        let code = view.string().to_string();
        let map = Utf16Map::new(&code);
        let sel = from_nsrange(view.selectedRange());
        let sel = map.to_byte(sel.start)..map.to_byte(sel.end);
        let (text, next) = snippets::formula_insert(template, &code[sel.clone()]);
        let select = match next {
            Some(p) => sel.start + p.start..sel.start + p.end,
            None => sel.start + text.len()..sel.start + text.len(),
        };
        replace_in_view(&view, &sel, &text, Some(select));
    }

    /// Tab and Shift-Tab in the formula code jump between placeholders.
    pub(super) fn atom_tab(&self, backwards: bool) -> bool {
        let view = {
            let edit = self.ivars().atoms.edit.borrow();
            match edit.as_ref() {
                Some(e) if matches!(e.kind, AtomKind::Formula { .. }) => e.view.clone(),
                _ => return false,
            }
        };
        let code = view.string().to_string();
        let map = Utf16Map::new(&code);
        let sel = from_nsrange(view.selectedRange());
        let from = map.to_byte(if backwards { sel.start } else { sel.end });
        let Some(next) = snippets::next_placeholder(&code, from, backwards) else {
            return false;
        };
        view.setSelectedRange(byte_range_to_ns(&map, &next));
        true
    }

    /// Colors the TeX or Mermaid code in the popover. Attribute changes stay out of the undo history.
    pub(super) fn color_atom_code(&self) {
        let (view, lang) = {
            let edit = self.ivars().atoms.edit.borrow();
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
        store.beginEditing();
        // SAFETY: Fonts and colors are the values these attribute keys expect.
        unsafe {
            store.addAttribute_value_range(NSFontAttributeName, &code_font(), all);
            store.addAttribute_value_range(NSForegroundColorAttributeName, &NSColor::textColor(), all);
            for (range, token) in code::tokens(lang, &text) {
                store.addAttribute_value_range(
                    NSForegroundColorAttributeName,
                    &token_color(token.into()),
                    byte_range_to_ns(&map, &range),
                );
            }
        }
        store.endEditing();
    }

    /// Shows the MathJax or Mermaid error for the current code, or a hint when there is none.
    pub(super) fn refresh_atom_status(&self) {
        let Some((status, kind, diagram, visual)) = self
            .ivars()
            .atoms
            .edit
            .borrow()
            .as_ref()
            .map(|e| (e.status.clone(), e.kind, e.diagram, e.visual.get()))
        else {
            return;
        };
        let outcome = self.atom_key().and_then(|k| js::outcome(&k));
        let (message, error) = match outcome {
            Some(Err(e)) => (format!("⚠︎ {}", e.lines().next().unwrap_or("")), true),
            _ => {
                let hint = match (kind, diagram) {
                    (AtomKind::Diagram(crate::markdown::doc::DiagramKind::Mermaid), DiagramKind::Other) => {
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
    fn atom_key(&self) -> Option<js::Key> {
        let start = self.ivars().atoms.edit.borrow().as_ref()?.start;
        let dark = self.is_dark();
        let st = self.ivars().state.borrow();
        figure_at(&st.doc, start).map(|(_, figure)| render_key(&figure, dark))
    }
}

fn code_font() -> Retained<NSFont> {
    NSFont::monospacedSystemFontOfSize_weight(13.0, constant!(NSFontWeightRegular))
}

/// Replaces a byte range of the popover code like typing would, so it lands in the popover's undo history.
fn replace_in_view(view: &NSTextView, range: &Range<usize>, text: &str, select: Option<Range<usize>>) {
    let code = view.string().to_string();
    let map = Utf16Map::new(&code);
    if let Some(w) = view.window() {
        w.makeFirstResponder(Some(view));
    }
    // SAFETY: The inserted object is an NSString, one of the two types this method accepts.
    unsafe { view.insertText_replacementRange(&ns(text), byte_range_to_ns(&map, range)) };
    if let Some(select) = select {
        let new = view.string().to_string();
        let new_map = Utf16Map::new(&new);
        if select.end <= new.len() {
            view.setSelectedRange(byte_range_to_ns(&new_map, &select));
        }
    }
}

/// Today's date as `YYYY-MM-DD` in the user's time zone, for new Gantt tasks.
fn today() -> String {
    crate::util::now("yyyy-MM-dd")
}
