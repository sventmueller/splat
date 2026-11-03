//! Changing the text: the commands of `actions`, their validation, and what the user types, deletes or pastes in
//! the preview, translated into source edits.

use std::cell::RefCell;
use std::ops::Range;
use std::path::{Path, PathBuf};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{DefinedClass, MainThreadOnly, Message, sel};
use objc2_app_kit::{NSBeep, NSMenu, NSMenuItem, NSPasteboard, NSPasteboardTypePNG, NSPasteboardTypeTIFF, NSTextView};
use objc2_foundation::{NSArray, NSString, NSValue};

use super::actions::{self, Action, Applies, code_language, diagram_template};
use super::atom::atom_at_preview;
use super::state::PreviewMode;
use super::sync::utf16_diff;
use super::views::{offers_text, pasteboard_urls};
use super::{EditorController, Invalidate, State, tag_of};
use crate::i18n::{t, tf};
use crate::markdown::commands::{self, Active, Edit, InlineStyle, ListStyle, Pending, Toggle};
use crate::markdown::doc::{AlertKind, Align, Doc};
use crate::markdown::table::{self, TableOp};
use crate::markdown::text::{Byte, Shown, TextAt, U16, units, utf16_len};
use crate::preview::bridge::Bridge;
use crate::preview::layout::Layout;
use crate::util::{
    answer_from, constant, contained, insert_text, ns, perform_later, shown_from_ns, storage, to_nsrange,
};

/// How long a paste waits for the picture to reach the document's folder, which may lie on a volume that hangs.
const PASTE_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// All that the formatting at the caret depends on; validation need not look again while it stays the same.
#[derive(Clone, PartialEq)]
struct ActiveKey {
    version: u64,
    selection: (usize, usize),
    preview: bool,
    state_version: u64,
    pending: Option<(Byte, Vec<InlineStyle>)>,
}

/// A change the preview announced before applying it, as range of its old text and replacement.
enum Announced {
    Edit(Range<Shown>, String),
    /// Several changes or one without text: found by comparing the texts afterwards.
    Unknown,
}

#[derive(Default)]
pub(super) struct EditState {
    /// Set from the moment the preview announces a user edit until it reports it done.
    preview_edit: RefCell<Option<Announced>>,
    /// Toolbar and menu validation asks for the formatting at the caret once per item and event.
    active_cache: RefCell<Option<(ActiveKey, Active)>>,
}

impl EditState {
    /// A second announcement before the first is done means several changes, which no single range describes.
    fn announce(&self, announced: Announced) {
        let mut slot = self.preview_edit.borrow_mut();
        *slot = Some(if slot.is_some() { Announced::Unknown } else { announced });
    }

    fn cached_active(&self, key: &ActiveKey) -> Option<Active> {
        match self.active_cache.borrow().as_ref() {
            Some((k, active)) if k == key => Some(active.clone()),
            _ => None,
        }
    }

    fn remember_active(&self, key: ActiveKey, active: Active) {
        *self.active_cache.borrow_mut() = Some((key, active));
    }
}

pub(super) struct Editing<'a> {
    host: &'a EditorController,
    state: &'a EditState,
}

impl EditorController {
    pub(super) fn editing(&self) -> Editing<'_> {
        Editing {
            host: self,
            state: &self.ivars().edits,
        }
    }
}

impl Editing<'_> {
    pub(super) fn text_did_change(&self, object: Option<&AnyObject>) {
        let panes = self.host.panes();
        if panes.is_silent() {
            return;
        }
        let atoms = self.host.atom_editor();
        if atoms.is_code_view(object) {
            atoms.text_changed();
        } else if panes.is_preview(object) {
            self.preview_changed();
        } else if !self.source_typed_with_pending() {
            if panes.preview_focused() {
                self.host.updater().update_now();
            } else {
                self.host.updater().schedule_update();
            }
        }
    }

    pub(super) fn text_will_change(&self, view: &NSTextView, ranges: &AnyObject, strings: Option<&AnyObject>) {
        let panes = self.host.panes();
        if panes.is_preview(Some(view)) && !panes.is_silent() {
            self.preview_will_change(ranges, strings);
        }
    }

    pub(super) fn selection_did_change(&self, object: Option<&AnyObject>) {
        let panes = self.host.panes();
        if panes.is_silent() || self.preview_editing() {
            return;
        }
        if panes.is_preview(object) {
            self.preview_selection_changed();
        } else if panes.is_source(object) {
            self.drop_pending_unless_at(None);
            let selection = self.host.selection();
            if !selection.mirrors_source() {
                selection.mark_source_selection_in_preview();
            }
        }
    }

    pub(super) fn do_command(&self, view: &NSTextView, command: Sel) -> bool {
        let atoms = self.host.atom_editor();
        if atoms.is_code_view(Some(view)) {
            atoms.command(command)
        } else if self.host.panes().is_preview(Some(view)) {
            self.preview_command(command)
        } else {
            self.source_command(command)
        }
    }

    #[expect(clippy::too_many_lines, reason = "one case per command")]
    pub(super) fn perform(&self, action: Action, sender: Option<&AnyObject>) {
        use Action::*;
        let tag = tag_of(sender);
        match action {
            Bold => self.inline(InlineStyle::Bold, t("Fett")),
            Italic => self.inline(InlineStyle::Italic, t("Kursiv")),
            Underline => self.inline(InlineStyle::Underline, t("Unterstrichen")),
            Strikethrough => self.inline(InlineStyle::Strikethrough, t("Durchgestrichen")),
            Highlight => self.inline(InlineStyle::Highlight, t("Textmarker")),
            InlineCode => self.inline(InlineStyle::Code, t("Code")),
            Superscript => self.inline(InlineStyle::Superscript, t("Hochgestellt")),
            Subscript => self.inline(InlineStyle::Subscript, t("Tiefgestellt")),
            Heading => {
                let level = u8::try_from(tag.clamp(0, 6)).unwrap_or(0);
                let name = if level == 0 {
                    t("Fließtext").to_string()
                } else {
                    tf("Überschrift {}", &[&level.to_string()])
                };
                self.command(&name, |d, sel| Some(commands::set_heading(d.text(), sel, level)));
            }
            BulletList => self.command(t("Aufzählung"), |d, sel| {
                Some(commands::toggle_list(d.text(), sel, ListStyle::Bullet))
            }),
            NumberedList => self.command(t("Nummerierung"), |d, sel| {
                Some(commands::toggle_list(d.text(), sel, ListStyle::Numbered))
            }),
            TaskList => self.command(t("Aufgabenliste"), |d, sel| {
                Some(commands::toggle_list(d.text(), sel, ListStyle::Task))
            }),
            Quote => self.command(t("Zitat"), |d, sel| Some(commands::toggle_quote(d.text(), sel))),
            CodeBlock => {
                let lang = code_language(tag);
                self.command(t("Codeblock"), |d, sel| {
                    Some(commands::code_block_language(d, sel, lang))
                });
            }
            Alert => {
                let kind = AlertKind::ALL[crate::util::to_usize(tag).min(AlertKind::ALL.len() - 1)];
                self.command(t("Hinweisbox"), |d, sel| Some(commands::insert_alert(d, sel, kind)));
            }
            RemoveFormatting => self.command(t("Formatierung entfernen"), |d, sel| {
                Some(commands::remove_formatting(d, sel))
            }),
            EditAtom => self.host.atom_editor().edit_at_selection(),
            Link => self.insert_link(),
            Image => self.insert_image(),
            Table => self.insert_table(),
            Formula => self.host.atom_editor().insert_formula(),
            Diagram => {
                let template = diagram_template(tag.clamp(0, 2));
                self.command(t("Diagramm"), |d, sel| {
                    Some(commands::insert_diagram(d.text(), sel, &template))
                });
            }
            Toc => self.command(t("Inhaltsverzeichnis"), commands::insert_toc),
            Footnote => self.command(t("Fußnote"), |d, sel| Some(commands::insert_footnote(d.text(), sel))),
            Rule => self.command(t("Trennlinie"), |d, sel| Some(commands::insert_rule(d.text(), sel))),
            RowAbove => self.table_op(TableOp::InsertRowAbove, t("Zeile einfügen")),
            RowBelow => self.table_op(TableOp::InsertRowBelow, t("Zeile einfügen")),
            ColumnLeft => self.table_op(TableOp::InsertColumnLeft, t("Spalte einfügen")),
            ColumnRight => self.table_op(TableOp::InsertColumnRight, t("Spalte einfügen")),
            DeleteRow => self.table_op(TableOp::DeleteRow, t("Zeile löschen")),
            DeleteColumn => self.table_op(TableOp::DeleteColumn, t("Spalte löschen")),
            TableAlign => {
                let align = match tag {
                    1 => Align::Left,
                    2 => Align::Center,
                    3 => Align::Right,
                    _ => Align::None,
                };
                self.table_op(TableOp::Align(align), t("Ausrichtung"));
            }
            TableFormat => self.table_op(TableOp::Format, t("Tabelle ausrichten")),
            TogglePreview => self.host.panes().toggle_preview(),
            ToggleOutline => self.host.panes().toggle_outline(),
            Typography => {
                super::env::set_typography(!super::env::typography_on());
                super::relayout_all();
            }
            ZoomIn => {
                let z = self.host.model().read(|st| st.zoom());
                self.host.updater().set_zoom((z * 1.1).min(3.0));
            }
            ZoomOut => {
                let z = self.host.model().read(|st| st.zoom());
                self.host.updater().set_zoom((z / 1.1).max(0.5));
            }
            ZoomReset => self.host.updater().set_zoom(1.0),
            NewTab => super::window::new_tab(self.host),
            LoadRemoteImages => self.host.pictures().allow_remote_images(),
            ExportHtml => self.host.sharing().export(super::export::ExportKind::Html),
            ExportWord => self.host.sharing().export(super::export::ExportKind::Word),
            EditAtomAt => {
                if let Some(range) = self
                    .host
                    .atom_editor()
                    .starting_at(Byte::new(crate::util::to_usize(tag.max(0))))
                {
                    self.host.atom_editor().open(range, None);
                }
            }
            AtomSnippet => self.host.atom_editor().snippet(tag),
            AtomMode => {
                let segment = sender
                    .and_then(|s| s.downcast_ref::<objc2_app_kit::NSSegmentedControl>())
                    .map_or(0, |c| c.selectedSegment());
                self.host.atom_editor().mode_changed(segment);
            }
            ToolbarSegment => super::toolbar::segment_clicked(self.host, sender),
        }
    }

    /// Applies a source edit as one undo step. Typing edits coalesce like keystrokes in the source view.
    pub(super) fn apply_edit(&self, edit: Edit, undo_name: Option<&str>, typing: bool) {
        // An open editor's text stands in the source without an undo step; committed, the offsets of `edit` stay right.
        self.host.atom_editor().finish();
        self.host.model().change(|st| st.clear_pending());
        let range = self.host.with_text(|snap, _| {
            let text = &snap.text;
            (edit.range.end <= text.end() && text.boundary(edit.range.start) && text.boundary(edit.range.end))
                .then(|| to_nsrange(snap.u16_range(&edit.range)))
        });
        let Some(range) = range else {
            self.host.updater().update_now();
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
        self.host.updater().update_now();
        if !self.host.panes().preview_focused() {
            self.host.panes().for_appkit().source.scrollRangeToVisible(selection);
        }
    }

    /// Runs a command on the source selection (mapped from the preview if it has focus). It goes unanswered while the
    /// state lags behind the text, as its edit would land somewhere else in it.
    fn command(&self, name: &str, f: impl FnOnce(&Doc, Range<Byte>) -> Option<Edit> + Send) {
        self.host.updater().update_now();
        let selection = self.host.selection().view_selection();
        let edit = self.host.with_text(move |snap, st| {
            st.is_current(snap).then(|| {
                let sel = selection.in_source(snap, st);
                contained(|| f(st.doc(), sel))
            })
        });
        match edit {
            None | Some(Some(None)) => {}
            Some(Some(Some(edit))) => self.apply_edit(edit, Some(name), false),
            Some(None) => NSBeep(),
        }
    }

    fn inline(&self, style: InlineStyle, name: &str) {
        self.host.updater().update_now();
        let selection = self.host.selection().view_selection();
        let toggled = self.host.with_text(move |snap, st| {
            st.is_current(snap).then(|| {
                let sel = selection.in_source(snap, st);
                let pending = st
                    .pending()
                    .as_ref()
                    .filter(|(_, base)| *base == snap.text)
                    .map(|(p, _)| p);
                contained(|| commands::toggle(st.doc(), sel, style, pending))
            })
        });
        let Some(toggled) = toggled else {
            return;
        };
        match toggled {
            None => NSBeep(),
            Some(Toggle::Edit(edit)) => self.apply_edit(edit, Some(name), false),
            Some(Toggle::Pending(pending)) => {
                self.host
                    .change_with_text(move |snap, st| st.set_pending(pending.map(|p| (p, snap.text.clone()))));
            }
        }
    }

    /// Forgets pending styles once the caret leaves their position. While the text differs from the stored one the
    /// user is typing, which `source_typed_with_pending` and `preview_changed` handle.
    pub(super) fn drop_pending_unless_at(&self, sel: Option<Range<Byte>>) {
        if self.host.model().read(|st| st.pending().is_none()) {
            return;
        }
        let selection = self.host.selection().source_range();
        self.host.change_with_text(move |snap, st| {
            let Some((at, typing)) = st.pending().map(|(p, base)| (p.at, *base != snap.text)) else {
                return;
            };
            if typing {
                return;
            }
            let sel = sel.unwrap_or_else(|| snap.byte_range(&selection));
            if !sel.is_empty() || sel.start != at {
                st.clear_pending();
            }
        });
    }

    /// Text just typed into the source at a caret with pending styles is moved into its markers.
    pub(super) fn source_typed_with_pending(&self) -> bool {
        let Some((pending, base)) = self.host.model().read(|st| st.pending().cloned()) else {
            return false;
        };
        if self.host.panes().source().has_marked_text() {
            return false;
        }
        self.host.model().change(|st| st.clear_pending());
        let typed = self.host.with_text(|snap, _| {
            let (edit, keep_pending) = commands::typed_at_pending(&base, &snap.text, &pending)?;
            let unchanged = snap.text.at(edit.range.clone()) == edit.replacement;
            Some((edit, unchanged, keep_pending))
        });
        let Some((edit, unchanged, keep_pending)) = typed else {
            return false;
        };
        let caret = edit.selection.start;
        if !unchanged {
            self.apply_edit(edit, None, true);
        }
        if keep_pending {
            let styles = pending.styles;
            self.host.change_with_text(move |snap, st| {
                st.set_pending(Some((Pending { at: caret, styles }, snap.text.clone())));
            });
        }
        !unchanged
    }

    fn table_op(&self, op: TableOp, name: &str) {
        self.command(name, |d, sel| table::table_edit(d, sel.start, op));
    }

    pub(super) fn active(&self) -> Active {
        let preview = self.host.panes().preview_focused();
        let view_sel = if preview {
            self.host.panes().preview().selected_range()
        } else {
            self.host.panes().source().selected_range()
        };
        let version = self.host.model().text_version();
        let key = self.host.model().read(move |st| ActiveKey {
            version,
            selection: (view_sel.location, view_sel.length),
            preview,
            state_version: st.version(),
            pending: st.pending().map(|(p, _)| (p.at, p.styles.clone())),
        });
        if let Some(active) = self.state.cached_active(&key) {
            return active;
        }
        let selection = self.host.selection().view_selection();
        let active = self.host.with_text(move |snap, st| {
            if !st.is_current(snap) {
                return Active::default();
            }
            let sel = selection.in_source(snap, st);
            let active = commands::active_at(st.doc(), sel.start);
            match st.pending() {
                Some((p, base)) if *base == snap.text && sel.is_empty() && sel.start == p.at => active.with_pending(p),
                _ => active,
            }
        });
        self.state.remember_active(key, active.clone());
        active
    }

    pub(super) fn validate(&self, selector: Option<Sel>, tag: isize, item: Option<&NSMenuItem>) -> bool {
        let Some(selector) = selector else { return true };
        let Some(action) = Action::from_selector(selector) else {
            return self.host.panes().has_text_focus();
        };
        let set_state = |on: bool| {
            if let Some(i) = item {
                i.setState(if on {
                    objc2_app_kit::NSControlStateValueOn
                } else {
                    objc2_app_kit::NSControlStateValueOff
                });
            }
        };
        if action == Action::TogglePreview
            && let Some(i) = item
        {
            i.setTitle(&ns(if self.host.panes().preview_visible() {
                t("Vorschau ausblenden")
            } else {
                t("Vorschau einblenden")
            }));
        }
        if action == Action::ToggleOutline
            && let Some(i) = item
        {
            i.setTitle(&ns(if self.host.panes().outline_visible() {
                t("Gliederung ausblenden")
            } else {
                t("Gliederung einblenden")
            }));
        }
        if action == Action::Typography {
            set_state(super::env::typography_on());
        }
        let applies = action.spec().applies;
        match applies {
            Applies::Always => return true,
            _ if !self.host.panes().has_text_focus() => return false,
            Applies::OnFormulaOrDiagram => return self.host.atom_editor().at_selection().is_some(),
            _ => {}
        }
        let active = self.active();
        if applies == Applies::InTable {
            return active.table;
        }
        if applies == Applies::OutsideCode && active.code_block {
            set_state(false);
            return false;
        }
        if let Some(on) = action.checked(&active, tag) {
            set_state(on);
        }
        true
    }

    /// The preview is about to apply a user edit; remembering it spares comparing the whole texts afterwards.
    pub(super) fn preview_will_change(&self, ranges: &AnyObject, strings: Option<&AnyObject>) {
        if !self.host.updater().preview_takes_input() {
            return;
        }
        let single = |list: Option<&AnyObject>| {
            list.and_then(|l| l.downcast_ref::<NSArray>())
                .filter(|l| l.count() == 1)
                .and_then(|l| l.firstObject())
        };
        let range = single(Some(ranges))
            .and_then(|r| r.downcast::<NSValue>().ok())
            .and_then(|v| v.get_range());
        let text = single(strings).and_then(|s| s.downcast::<NSString>().ok());
        let announced = match (range, text) {
            (Some(r), Some(text)) => Announced::Edit(shown_from_ns(r), text.to_string()),
            _ => Announced::Unknown,
        };
        self.state.announce(announced);
        // A composition that is cancelled announces a change and never makes it.
        perform_later(self.host, sel!(previewEditSettled), 0.0, true);
    }

    /// The turn of the run loop that announced an edit is over, and with it the edit, unless a composition goes on.
    pub(super) fn preview_edit_settled(&self) {
        if !self.host.panes().preview().has_marked_text() && self.state.preview_edit.borrow_mut().take().is_some() {
            self.host.updater().update_now();
        }
    }

    /// The user typed, deleted or pasted in the preview: translate the change into the source.
    pub(super) fn preview_changed(&self) {
        let announced = self.state.preview_edit.borrow_mut().take();
        let preview = self.host.panes().preview();
        if preview.has_marked_text() || !self.host.updater().preview_takes_input() {
            return;
        }
        let caught_up = self
            .host
            .with_text(|snap, st| st.is_current(snap) && bridge(st).is_some());
        if !caught_up {
            self.host.invalidate(Invalidate::Preview);
            return;
        }
        let new_len = preview.length();
        let caret = preview.selected_range().location;
        let shown_len: usize = self.host.model().read(|st| st.blocks().lens.iter().sum());
        let known = fitting_announcement(announced, shown_len, new_len).ok_or_else(|| preview.string().to_string());
        let (edit, dirty, next) = self.host.model().read(move |st| {
            let (old_range, inserted) = known.unwrap_or_else(|new_text| compared_change(st.layout(), &new_text, caret));
            let delta = crate::util::to_isize(utf16_len(&inserted)) - crate::util::to_isize(units(&old_range));
            let bridge = bridge(st);
            // Without an edit the preview falls back to what the source says, which it shows next.
            let (edit, next) = contained(|| {
                let bridge = bridge.as_ref()?;
                let typed_pending = st
                    .pending()
                    .as_ref()
                    .filter(|(_, base)| *base == st.text() && old_range.is_empty())
                    .and_then(|(pending, _)| bridge.type_with_pending(pending, old_range.start, &inserted));
                typed_pending.or_else(|| bridge.replace_keeping_styles(old_range.clone(), &inserted))
            })
            .flatten()
            .map_or((None, None), |(edit, next)| (Some(edit), next));
            (edit, (old_range, delta), next)
        });
        self.host.model().change(move |st| st.note_preview_edit(dirty));
        match edit {
            Some(edit) => {
                self.apply_edit(edit, None, true);
                if let Some(next) = next {
                    self.host
                        .change_with_text(move |snap, st| st.set_pending(Some((next, snap.text.clone()))));
                }
            }
            None => self.host.updater().update_now(),
        }
    }

    pub(super) fn preview_selection_changed(&self) {
        let shown = shown_from_ns(self.host.panes().preview().selected_range());
        let found = self.host.with_text(move |snap, st| {
            if !st.is_current(snap) {
                return None;
            }
            let sel = contained(|| bridge(st).map(|b| b.source_selection(shown))).flatten()?;
            let range = to_nsrange(snap.u16_range(&sel));
            Some((sel, range))
        });
        let Some((sel, range)) = found else {
            if self.host.model().read(|st| st.preview_is_behind()) {
                self.host.updater().update_now();
            }
            return;
        };
        let caret = sel.start;
        self.drop_pending_unless_at(Some(sel));
        self.host.selection().set_source_selection(range);
        if self.host.model().read(|st| st.caret()) != Some(caret) {
            self.host.updater().update_now();
        }
        self.host.atom_editor().open_under_caret_later();
    }

    pub(super) fn preview_command(&self, command: Sel) -> bool {
        self.host.updater().update_now();
        if !self.host.updater().preview_takes_input() {
            return false;
        }
        let sel = shown_from_ns(self.host.panes().preview().selected_range());
        let p = sel.start;
        let answer = self.host.model().read(move |st| {
            let Some(bridge) = bridge(st) else {
                return PreviewCommand::NotCaughtUp;
            };
            let edit = |e: Option<Option<Edit>>, name: &'static str| match e.flatten() {
                Some(e) => PreviewCommand::Edit(e, name),
                None => PreviewCommand::Handled(false),
            };
            match command {
                c if c == sel!(insertNewline:) && units(&sel) == 1 && atom_at_preview(st, p).is_some() => {
                    PreviewCommand::OpenAtom
                }
                c if c == sel!(insertNewline:) && sel.is_empty() => edit(contained(|| bridge.newline(p)), t("Absatz")),
                c if (c == sel!(insertLineBreak:) || c == sel!(insertNewlineIgnoringFieldEditor:))
                    && sel.is_empty() =>
                {
                    edit(contained(|| bridge.line_break(p)), t("Zeilenumbruch"))
                }
                c if c == sel!(insertTab:) => match contained(|| bridge.tab(p, false)).flatten() {
                    Some(e) => PreviewCommand::Edit(e, t("Einrücken")),
                    None => PreviewCommand::Handled(!preview_in_code(st, p)),
                },
                c if c == sel!(insertBacktab:) => match contained(|| bridge.tab(p, true)).flatten() {
                    Some(e) => PreviewCommand::Edit(e, t("Ausrücken")),
                    None => PreviewCommand::Handled(true),
                },
                c if c == sel!(deleteBackward:) && sel.is_empty() => {
                    edit(contained(|| bridge.delete_at_block_start(p)), t("Löschen"))
                }
                _ => PreviewCommand::Handled(false),
            }
        });
        match answer {
            PreviewCommand::NotCaughtUp => {
                self.host.updater().schedule_update();
                true
            }
            PreviewCommand::OpenAtom => {
                self.host.atom_editor().edit_at_preview(p, None);
                true
            }
            PreviewCommand::Edit(edit, name) => {
                self.apply_edit(edit, Some(name), false);
                true
            }
            PreviewCommand::Handled(handled) => handled,
        }
    }

    pub(super) fn source_command(&self, command: Sel) -> bool {
        if command != sel!(insertTab:) && command != sel!(insertBacktab:) {
            return false;
        }
        self.host.updater().update_now();
        let selection = self.host.selection().source_range();
        let edit = self.host.with_text(move |snap, st| {
            if !st.is_current(snap) {
                return None;
            }
            let at = snap.byte_range(&selection).start;
            contained(|| table::navigate(st.doc(), at, command == sel!(insertBacktab:))).flatten()
        });
        match edit {
            Some(edit) => {
                self.apply_edit(edit, Some(t("Zelle wechseln")), false);
                true
            }
            None => false,
        }
    }

    pub(super) fn adjust_context_menu(&self, menu: &NSMenu, view: &NSTextView, index: usize) -> Retained<NSMenu> {
        let mtm = self.host.mtm();
        let blocked = [
            sel!(orderFrontFontPanel:),
            sel!(changeLayoutOrientation:),
            sel!(addFontTrait:),
            sel!(orderFrontColorPanel:),
            sel!(underline:),
        ];
        for item in menu.itemArray().iter() {
            if let Some(sub) = item.submenu() {
                let bad = sub
                    .itemArray()
                    .iter()
                    .any(|i| i.action().is_some_and(|a| blocked.contains(&a)));
                if bad {
                    menu.removeItem(&item);
                }
            }
        }
        let mut position = menu
            .itemArray()
            .iter()
            .position(|i| i.action() == Some(sel!(paste:)))
            .map_or(0, |p| p + 1);
        let mut insert = |item: Retained<NSMenuItem>| {
            menu.insertItem_atIndex(&item, crate::util::to_isize(position));
            position += 1;
        };
        insert(NSMenuItem::separatorItem(mtm));
        for action in [
            Action::Bold,
            Action::Italic,
            Action::Underline,
            Action::Strikethrough,
            Action::Highlight,
            Action::InlineCode,
            Action::Link,
        ] {
            insert(actions::context_item(mtm, action));
        }
        if let Some(range) = self.host.atom_editor().at_index(view, index) {
            let diagram = self
                .host
                .on_source_text(|text| text.at(range.clone()).starts_with(['`', '~']));
            let title = if diagram {
                t("Diagramm bearbeiten …")
            } else {
                t("Formel bearbeiten …")
            };
            insert(actions::tagged(
                mtm,
                Action::EditAtomAt,
                title,
                "",
                crate::util::to_isize(range.start.get()),
            ));
        }
        if self.active().table {
            insert(NSMenuItem::separatorItem(mtm));
            for action in [
                Action::RowBelow,
                Action::ColumnRight,
                Action::DeleteRow,
                Action::DeleteColumn,
            ] {
                insert(actions::context_item(mtm, action));
            }
        }
        menu.retain()
    }

    fn insert_link(&self) {
        let Some(window) = self.host.window() else { return };
        let (_, selected) = self.selected_source();
        let this = self.host.retain();
        // The selection is read again when the sheet closes: a reload from disk can change the text meanwhile.
        super::dialogs::link(&window, &selected, move |url, label| {
            this.updater().update_now();
            let (sel, _) = this.editing().selected_source();
            let edit = commands::insert_link(sel, &url, &label);
            this.editing().apply_edit(edit, Some(t("Link einfügen")), false);
        });
    }

    fn insert_table(&self) {
        let Some(window) = self.host.window() else { return };
        let this = self.host.retain();
        super::dialogs::table(&window, move |rows, cols| {
            this.updater().update_now();
            let (sel, _) = this.editing().selected_source();
            let edit = this.on_source_text(move |text| commands::insert_table(text, sel, rows, cols));
            this.editing().apply_edit(edit, Some(t("Tabelle einfügen")), false);
        });
    }

    fn insert_image(&self) {
        let Some(window) = self.host.window() else { return };
        let this = self.host.retain();
        super::dialogs::image(&window, move |url| {
            let Some(path) = url.path() else { return };
            this.updater().update_now();
            let (sel, _) = this.editing().selected_source();
            let path = PathBuf::from(path.to_string());
            let alt = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let rel = relative_path(this.doc_dir().as_deref(), &path);
            let edit = commands::insert_image(sel, &alt, &rel);
            this.editing().apply_edit(edit, Some(t("Bild einfügen")), false);
        });
    }

    /// A picture on the pasteboard becomes a PNG file next to the document and an image link at the selection.
    /// Returns whether the paste was handled here.
    pub(super) fn paste_image(&self, pasteboard: &NSPasteboard) -> bool {
        // Text wins: spreadsheets and web pages put a picture of the copied text on the pasteboard as well.
        if offers_text(pasteboard) {
            return false;
        }
        let png = pasteboard
            .dataForType(constant!(NSPasteboardTypePNG))
            .map(|d| d.to_vec())
            .or_else(|| {
                let tiff = pasteboard.dataForType(constant!(NSPasteboardTypeTIFF))?;
                super::decoder::png_of_pasted_tiff(&tiff.to_vec())
            });
        let Some(png) = png else {
            return false;
        };
        let Some(dir) = self.host.doc_dir() else {
            self.ask_to_save_before_pasting();
            return true;
        };
        let stem = format!("{} {}", t("Bild"), crate::util::now("yyyy-MM-dd HH.mm.ss"));
        let folder = dir.clone();
        let written = answer_from(&dir, PASTE_WAIT, move || {
            let mut path = folder.join(format!("{stem}.png"));
            for n in 2.. {
                if !path.exists() {
                    break;
                }
                path = folder.join(format!("{stem} {n}.png"));
            }
            std::fs::write(&path, &png).ok().map(|()| path)
        });
        let Some(path) = written.flatten() else {
            NSBeep();
            return true;
        };
        let (sel, _) = self.selected_source();
        let alt = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let edit = commands::insert_image(sel, &alt, &relative_path(Some(&dir), &path));
        self.apply_edit(edit, Some(t("Bild einfügen")), false);
        true
    }

    fn ask_to_save_before_pasting(&self) {
        let Some(window) = self.host.window() else { return };
        let alert = objc2_app_kit::NSAlert::new(self.host.mtm());
        alert.setMessageText(&ns(t("Dokument zuerst sichern")));
        alert.setInformativeText(&ns(t(
            "Eingesetzte Bilder werden als Datei neben dem Dokument abgelegt. Sichere es, dann setze das Bild noch einmal ein.",
        )));
        alert.addButtonWithTitle(&ns(t("Sichern …")));
        alert.addButtonWithTitle(&ns(t("Abbrechen")));
        let document = self.host.ivars().document.clone();
        let handler = block2::RcBlock::new(move |response: objc2_app_kit::NSModalResponse| {
            if response == objc2_app_kit::NSAlertFirstButtonReturn
                && let Some(doc) = document.load()
            {
                // SAFETY: Save accepts a missing sender.
                unsafe { doc.saveDocument(None) };
            }
        });
        alert.beginSheetModalForWindow_completionHandler(&window, Some(&handler));
    }

    /// The source range of the selection and its text.
    fn selected_source(&self) -> (Range<Byte>, String) {
        let selection = self.host.selection().view_selection();
        self.host.with_text(move |snap, st| {
            let sel = selection.in_source(snap, st);
            let text = snap.text.at(sel.clone()).to_string();
            (sel, text)
        })
    }

    /// Dropping files or URLs on the source inserts Markdown images and links.
    pub(super) fn handle_source_drop(&self, pasteboard: &NSPasteboard, index: usize) -> bool {
        let dropped: Vec<Dropped> = pasteboard_urls(pasteboard)
            .iter()
            .filter_map(|url| {
                if url.isFileURL() {
                    url.path().map(|p| Dropped::File(PathBuf::from(p.to_string())))
                } else {
                    url.absoluteString().map(|s| Dropped::Address(s.to_string()))
                }
            })
            .collect();
        if dropped.is_empty() {
            return false;
        }
        let at = self.host.with_text(move |snap, _| snap.byte_at(U16::new(index)));
        let insert = dropped_markdown(&dropped, self.host.doc_dir().as_deref());
        let end = at + insert.len();
        let edit = Edit {
            range: at..at,
            replacement: insert,
            selection: end..end,
        };
        self.apply_edit(edit, Some(t("Einfügen")), false);
        true
    }

    /// The preview text view is applying a user edit; its storage is ahead of the layout until textDidChange.
    pub(super) fn preview_editing(&self) -> bool {
        self.state.preview_edit.borrow().is_some()
    }
}

/// What a command in the preview comes to.
enum PreviewCommand {
    NotCaughtUp,
    OpenAtom,
    Edit(Edit, &'static str),
    /// Whether the view is spared its own handling.
    Handled(bool),
}

/// `None` while the preview has not caught up with the document, or shows a notice instead of it.
pub(super) fn bridge(st: &State) -> Option<Bridge<'_>> {
    if st.preview_mode() != PreviewMode::Shows {
        return None;
    }
    Bridge::new(st.doc(), st.layout(), st.index())
}

fn preview_in_code(st: &State, p: Shown) -> bool {
    st.index().para_at(p).is_some_and(|i| {
        matches!(
            st.index().para(st.layout(), i).style.kind,
            crate::preview::layout::ParaKind::Code { .. }
        )
    })
}

/// The change the preview view announced, as the range of its old text and what replaced it, if that fits the lengths.
fn fitting_announcement(
    announced: Option<Announced>,
    shown_len: usize,
    new_len: usize,
) -> Option<(Range<Shown>, String)> {
    match announced {
        Some(Announced::Edit(range, text))
            if range.end.get() <= shown_len && new_len + units(&range) == shown_len + utf16_len(&text) =>
        {
            Some((range, text))
        }
        _ => None,
    }
}

/// The change the user made in the preview found by comparing its old and new text around the caret.
fn compared_change(layout: &Layout, new_text: &str, caret: usize) -> (Range<Shown>, String) {
    let new: Vec<u16> = new_text.encode_utf16().collect();
    let old: Vec<u16> = preview_string(layout).encode_utf16().collect();
    let (old_range, new_range) = utf16_diff(&old, &new, caret);
    (
        Shown::new(old_range.start)..Shown::new(old_range.end),
        String::from_utf16_lossy(&new[new_range]),
    )
}

enum Dropped {
    File(PathBuf),
    Address(String),
}

/// Markdown for files and addresses dropped on the source, one per line: pictures as images, everything else as
/// links, files relative to `dir`.
fn dropped_markdown(dropped: &[Dropped], dir: Option<&Path>) -> String {
    const PICTURES: [&str; 10] = ["png", "jpg", "jpeg", "gif", "svg", "webp", "heic", "tif", "tiff", "bmp"];
    let lines: Vec<String> = dropped
        .iter()
        .map(|item| match item {
            Dropped::File(path) => {
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let dest = commands::link_destination(&relative_path(dir, path));
                let ext = path
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if PICTURES.contains(&ext.as_str()) {
                    format!("![{name}]({dest})")
                } else {
                    format!("[{name}]({dest})")
                }
            }
            Dropped::Address(s) => format!("[{s}]({})", commands::link_destination(s)),
        })
        .collect();
    lines.join("\n")
}

/// The text the preview shows for `layout`.
fn preview_string(layout: &Layout) -> String {
    let mut s = String::new();
    for para in layout.blocks.iter().flat_map(|b| &b.paras) {
        for run in &para.runs {
            s.push_str(&run.text);
        }
        s.push('\n');
    }
    s
}

/// Relative path from `dir` to `target`, falling back to the absolute path.
pub(super) fn relative_path(dir: Option<&Path>, target: &Path) -> String {
    let Some(dir) = dir else {
        return target.to_string_lossy().into_owned();
    };
    let from: Vec<_> = dir.components().collect();
    let to: Vec<_> = target.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    if common <= 1 {
        return target.to_string_lossy().into_owned();
    }
    let mut out = PathBuf::new();
    for _ in common..from.len() {
        out.push("..");
    }
    for c in &to[common..] {
        out.push(c);
    }
    out.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_with_only_the_root_in_common_stay_absolute() {
        assert_eq!(
            relative_path(Some(Path::new("/a/b")), Path::new("/c/d.png")),
            "/c/d.png"
        );
        assert_eq!(
            relative_path(Some(Path::new("/a/b")), Path::new("/a/d.png")),
            "../d.png"
        );
        assert_eq!(relative_path(Some(Path::new("/a")), Path::new("/a/b/d.png")), "b/d.png");
        assert_eq!(relative_path(None, Path::new("/a/d.png")), "/a/d.png");
    }
    use crate::markdown::reader::parse;
    use crate::preview::layout::{NoImages, layout};

    #[test]
    fn relative_paths() {
        assert_eq!(
            relative_path(Some(Path::new("/a/b")), Path::new("/a/b/img/x.png")),
            "img/x.png"
        );
        assert_eq!(
            relative_path(Some(Path::new("/a/b")), Path::new("/a/c/x.png")),
            "../c/x.png"
        );
        assert_eq!(relative_path(None, Path::new("/a/x.png")), "/a/x.png");
    }

    #[test]
    fn an_announced_preview_change_counts_only_when_it_fits_the_lengths() {
        let text = "Ein Wort\n";
        let l = layout(&parse(text), &NoImages, None);
        let shown = utf16_len(&preview_string(&l));
        let fits = Some(Announced::Edit(Shown::new(3)..Shown::new(3), "x".into()));
        assert_eq!(
            fitting_announcement(fits, shown, shown + 1),
            Some((Shown::new(3)..Shown::new(3), "x".into()))
        );
        let misfit = Some(Announced::Edit(Shown::new(0)..Shown::new(0), "yy".into()));
        assert_eq!(fitting_announcement(misfit, shown, shown + 1), None);
        assert_eq!(fitting_announcement(Some(Announced::Unknown), shown, shown - 4), None);
        assert_eq!(
            compared_change(&l, "Einx Wort\n", 4),
            (Shown::new(3)..Shown::new(3), "x".into())
        );
        assert_eq!(
            compared_change(&l, "Ein \n", 4),
            (Shown::new(4)..Shown::new(8), String::new())
        );
    }

    fn key(version: u64) -> ActiveKey {
        ActiveKey {
            version,
            selection: (1, 2),
            preview: false,
            state_version: 3,
            pending: None,
        }
    }

    #[test]
    fn the_formatting_at_the_caret_is_looked_at_again_when_anything_it_depends_on_changed() {
        let state = EditState::default();
        assert!(state.cached_active(&key(1)).is_none());
        state.remember_active(
            key(1),
            Active {
                bold: true,
                ..Active::default()
            },
        );
        assert!(state.cached_active(&key(1)).is_some_and(|active| active.bold));
        let changes = [
            key(2),
            ActiveKey {
                selection: (1, 3),
                ..key(1)
            },
            ActiveKey {
                preview: true,
                ..key(1)
            },
            ActiveKey {
                state_version: 4,
                ..key(1)
            },
            ActiveKey {
                pending: Some((Byte::new(0), vec![InlineStyle::Bold])),
                ..key(1)
            },
        ];
        for changed in &changes {
            assert!(state.cached_active(changed).is_none());
        }
        state.remember_active(key(2), Active::default());
        assert!(state.cached_active(&key(1)).is_none(), "only the last answer is kept");
    }

    #[test]
    fn a_second_announcement_of_the_preview_leaves_only_comparing_the_texts() {
        let state = EditState::default();
        state.announce(Announced::Edit(Shown::new(1)..Shown::new(1), "a".into()));
        assert!(matches!(*state.preview_edit.borrow(), Some(Announced::Edit(..))));
        state.announce(Announced::Edit(Shown::new(2)..Shown::new(2), "b".into()));
        assert!(matches!(*state.preview_edit.borrow(), Some(Announced::Unknown)));
    }

    #[test]
    fn dropped_files_become_images_or_links_relative_to_the_document() {
        let dropped = [
            Dropped::File("/Users/a/Notizen/Bilder/Foto 1.PNG".into()),
            Dropped::File("/Users/a/Bericht.pdf".into()),
            Dropped::Address("https://example.com/(x)".into()),
        ];
        assert_eq!(
            dropped_markdown(&dropped, Some(Path::new("/Users/a/Notizen"))),
            "![Foto 1](<Bilder/Foto 1.PNG>)\n[Bericht](../Bericht.pdf)\n[https://example.com/(x)](<https://example.com/(x)>)"
        );
        assert_eq!(
            dropped_markdown(&dropped[1..2], None),
            "[Bericht](/Users/a/Bericht.pdf)"
        );
    }
}
