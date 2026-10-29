//! Changing the text: the commands of `actions`, their validation, and what the user types, deletes or pastes in
//! the preview, translated into source edits.

use std::ops::Range;
use std::path::{Path, PathBuf};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{DefinedClass, MainThreadOnly, Message, sel};
use objc2_app_kit::{
    NSBeep, NSMenu, NSMenuItem, NSPasteboard, NSPasteboardTypePNG, NSPasteboardTypeString, NSPasteboardTypeTIFF,
    NSTextInputClient, NSTextView,
};
use objc2_foundation::{NSArray, NSString, NSValue};

use super::actions::{self, Action, Applies, code_language, diagram_template};
use super::sync::{Snapshot, utf16_diff};
use super::views::pasteboard_urls;
use super::{ActiveKey, Announced, EditorController, Invalidate, State, tag_of};
use crate::i18n::{t, tf};
use crate::markdown::commands::{self, Active, Edit, InlineStyle, ListStyle, Pending, Toggle};
use crate::markdown::doc::{AlertKind, Align, Doc};
use crate::markdown::table::{self, TableOp};
use crate::markdown::text::utf16_len;
use crate::preview::bridge::Bridge;
use crate::preview::layout::Layout;
use crate::util::{constant, from_nsrange, ns, storage, to_nsrange};

impl EditorController {
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
                let level = tag.clamp(0, 6) as u8;
                let name = if level == 0 {
                    t("Fließtext").to_string()
                } else {
                    tf("Überschrift {}", &[&level.to_string()])
                };
                self.command(&name, |t, _, sel| Some(commands::set_heading(t, sel, level)));
            }
            BulletList => self.command(t("Aufzählung"), |t, _, sel| {
                Some(commands::toggle_list(t, sel, ListStyle::Bullet))
            }),
            NumberedList => self.command(t("Nummerierung"), |t, _, sel| {
                Some(commands::toggle_list(t, sel, ListStyle::Numbered))
            }),
            TaskList => self.command(t("Aufgabenliste"), |t, _, sel| {
                Some(commands::toggle_list(t, sel, ListStyle::Task))
            }),
            Quote => self.command(t("Zitat"), |t, _, sel| Some(commands::toggle_quote(t, sel))),
            CodeBlock => {
                let lang = code_language(tag);
                self.command(t("Codeblock"), |t, d, sel| {
                    Some(commands::code_block_language(t, d, sel, lang))
                });
            }
            Alert => {
                let kind = AlertKind::ALL[tag.clamp(0, AlertKind::ALL.len() as isize - 1) as usize];
                self.command(t("Hinweisbox"), |t, d, sel| {
                    Some(commands::insert_alert(t, d, sel, kind))
                });
            }
            RemoveFormatting => self.command(t("Formatierung entfernen"), |t, d, sel| {
                Some(commands::remove_formatting(t, d, sel))
            }),
            EditAtom => self.edit_atom_at_selection(),
            Link => self.insert_link(),
            Image => self.insert_image(),
            Table => self.insert_table(),
            Formula => self.insert_formula(),
            Diagram => {
                let template = diagram_template(tag.clamp(0, 2));
                self.command(t("Diagramm"), |text, _, sel| {
                    Some(commands::insert_diagram(text, sel, &template))
                });
            }
            Toc => self.command(t("Inhaltsverzeichnis"), commands::insert_toc),
            Footnote => self.command(t("Fußnote"), |t, _, sel| Some(commands::insert_footnote(t, sel))),
            Rule => self.command(t("Trennlinie"), |t, _, sel| Some(commands::insert_rule(t, sel))),
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
            TogglePreview => self.toggle_preview(),
            ToggleOutline => self.toggle_outline(),
            Typography => {
                super::env::set_typography(!super::env::typography_on());
                super::relayout_all();
            }
            ZoomIn => {
                let z = self.ivars().state.borrow().zoom;
                self.set_zoom((z * 1.1).min(3.0));
            }
            ZoomOut => {
                let z = self.ivars().state.borrow().zoom;
                self.set_zoom((z / 1.1).max(0.5));
            }
            ZoomReset => self.set_zoom(1.0),
            NewTab => self.new_tab(),
            LoadRemoteImages => self.allow_remote_images(),
            ExportHtml => self.export(super::export::Format::Html),
            ExportWord => self.export(super::export::Format::Word),
            EditAtomAt => {
                if let Some(range) = self.atom_starting_at(tag.max(0) as usize) {
                    self.open_atom_editor(range, None);
                }
            }
            AtomSnippet => self.atom_snippet(tag),
            AtomMode => {
                let segment = sender
                    .and_then(|s| s.downcast_ref::<objc2_app_kit::NSSegmentedControl>())
                    .map_or(0, |c| c.selectedSegment());
                self.atom_mode_changed(segment);
            }
            ToolbarSegment => self.toolbar_segment(sender),
        }
    }

    /// Applies a source edit as one undo step. Typing edits coalesce like keystrokes in the source view.
    pub(super) fn apply_edit(&self, edit: Edit, undo_name: Option<&str>, typing: bool) {
        self.ivars().state.borrow_mut().pending = None;
        let source = self.source();
        let range = {
            let snap = self.snapshot();
            let text = &snap.text;
            if edit.range.end > text.len()
                || !text.is_char_boundary(edit.range.start)
                || !text.is_char_boundary(edit.range.end)
            {
                drop(snap);
                self.update();
                return;
            }
            to_nsrange(snap.u16_range(&edit.range))
        };
        let replacement = ns(&edit.replacement);
        let silent = self.silently();
        if typing {
            // SAFETY: The inserted object is an NSString, one of the two types this method accepts.
            unsafe { source.insertText_replacementRange(&replacement, range) };
        } else {
            // Without this, NSTextView merges the command into the preceding typing undo step.
            source.breakUndoCoalescing();
            if source.shouldChangeTextInRange_replacementString(range, Some(&replacement)) {
                storage(source).replaceCharactersInRange_withString(range, &replacement);
                source.didChangeText();
                if let (Some(name), Some(undo)) = (undo_name, self.undo_manager()) {
                    undo.setActionName(&ns(name));
                }
            }
            source.breakUndoCoalescing();
        }
        let selection = to_nsrange(self.snapshot().u16_range(&edit.selection));
        self.set_source_selection(selection);
        drop(silent);
        self.update();
        if !self.preview_focused() {
            source.scrollRangeToVisible(selection);
        }
    }

    /// Runs a command on the source selection (mapped from the preview if it has focus).
    fn command(&self, name: &str, f: impl FnOnce(&str, &Doc, Range<usize>) -> Option<Edit>) {
        self.update();
        let snap = self.snapshot();
        let edit = {
            let st = self.ivars().state.borrow();
            let sel = self.current_selection(&snap, &st);
            f(&snap.text, &st.doc, sel)
        };
        drop(snap);
        if let Some(edit) = edit {
            self.apply_edit(edit, Some(name), false);
        }
    }

    pub(super) fn current_selection(&self, snap: &Snapshot, st: &State) -> Range<usize> {
        if self.preview_focused() {
            let r = from_nsrange(self.preview().selectedRange());
            bridge(&snap.text, st).source_selection(r)
        } else {
            self.source_selection(snap)
        }
    }

    fn inline(&self, style: InlineStyle, name: &str) {
        self.update();
        let snap = self.snapshot();
        let toggled = {
            let st = self.ivars().state.borrow();
            let sel = self.current_selection(&snap, &st);
            let pending = st
                .pending
                .as_ref()
                .filter(|(_, base)| *base == snap.text)
                .map(|(p, _)| p);
            commands::toggle(&snap.text, &st.doc, sel, style, pending)
        };
        match toggled {
            Toggle::Edit(edit) => {
                drop(snap);
                self.apply_edit(edit, Some(name), false);
            }
            Toggle::Pending(pending) => {
                self.ivars().state.borrow_mut().pending = pending.map(|p| (p, snap.text.clone()));
            }
        }
    }

    /// Forgets pending styles once the caret leaves their position. While the text differs from the stored one the
    /// user is typing, which `source_typed_with_pending` and `preview_changed` handle.
    pub(super) fn drop_pending_unless_at(&self, sel: Option<Range<usize>>) {
        let Some(at) = self.ivars().state.borrow().pending.as_ref().map(|(p, _)| p.at) else {
            return;
        };
        let snap = self.snapshot();
        let typing = self
            .ivars()
            .state
            .borrow()
            .pending
            .as_ref()
            .is_some_and(|(_, base)| *base != snap.text);
        if typing {
            return;
        }
        let sel = sel.unwrap_or_else(|| self.source_selection(&snap));
        if !sel.is_empty() || sel.start != at {
            self.ivars().state.borrow_mut().pending = None;
        }
    }

    /// Text just typed into the source at a caret with pending styles is moved into its markers.
    pub(super) fn source_typed_with_pending(&self) -> bool {
        let Some((pending, base)) = self.ivars().state.borrow().pending.clone() else {
            return false;
        };
        if self.source().hasMarkedText() {
            return false;
        }
        self.ivars().state.borrow_mut().pending = None;
        let (edit, unchanged, keep_pending) = {
            let snap = self.snapshot();
            let Some((edit, keep_pending)) = commands::typed_at_pending(&base, &snap.text, &pending) else {
                return false;
            };
            let unchanged = snap.text[edit.range.clone()] == edit.replacement;
            (edit, unchanged, keep_pending)
        };
        let caret = edit.selection.start;
        if !unchanged {
            self.apply_edit(edit, None, true);
        }
        if keep_pending {
            let styles = pending.styles;
            self.ivars().state.borrow_mut().pending = Some((Pending { at: caret, styles }, self.source_text()));
        }
        !unchanged
    }

    fn table_op(&self, op: TableOp, name: &str) {
        self.command(name, |t, d, sel| table::table_edit(t, d, sel.start, op));
    }

    pub(super) fn active(&self) -> Active {
        let preview = self.preview_focused();
        let view_sel = if preview {
            self.preview().selectedRange()
        } else {
            self.source().selectedRange()
        };
        let key = {
            let st = self.ivars().state.borrow();
            ActiveKey {
                version: self.ivars().copy.version.get(),
                selection: (view_sel.location, view_sel.length),
                preview,
                state_version: st.version,
                pending: st.pending.as_ref().map(|(p, _)| (p.at, p.styles.clone())),
            }
        };
        if let Some((k, a)) = self.ivars().active_cache.borrow().as_ref()
            && *k == key
        {
            return a.clone();
        }
        let snap = self.snapshot();
        let active = {
            let st = self.ivars().state.borrow();
            if !st.is_current(&snap) {
                Active::default()
            } else {
                let sel = self.current_selection(&snap, &st);
                let active = commands::active_at(&st.doc, sel.start);
                match &st.pending {
                    Some((p, base)) if *base == snap.text && sel.is_empty() && sel.start == p.at => {
                        active.with_pending(p)
                    }
                    _ => active,
                }
            }
        };
        *self.ivars().active_cache.borrow_mut() = Some((key, active.clone()));
        active
    }

    fn has_text_focus(&self) -> bool {
        let Some(window) = self.window() else { return false };
        let Some(responder) = window.firstResponder() else {
            return false;
        };
        let r: &AnyObject = &responder;
        let source: &AnyObject = self.source();
        let preview: &AnyObject = self.preview();
        std::ptr::eq(r, source) || std::ptr::eq(r, preview)
    }

    pub(super) fn validate(&self, selector: Option<Sel>, tag: isize, item: Option<&NSMenuItem>) -> bool {
        let Some(selector) = selector else { return true };
        let Some(action) = Action::from_selector(selector) else {
            return self.has_text_focus();
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
            i.setTitle(&ns(if self.preview_visible() {
                t("Vorschau ausblenden")
            } else {
                t("Vorschau einblenden")
            }));
        }
        if action == Action::ToggleOutline
            && let Some(i) = item
        {
            i.setTitle(&ns(if self.outline_visible() {
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
            _ if !self.has_text_focus() => return false,
            Applies::OnFormulaOrDiagram => return self.atom_at_selection().is_some(),
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
            (Some(r), Some(text)) => Announced::Edit(from_nsrange(r), text.to_string()),
            _ => Announced::Unknown,
        };
        let mut slot = self.ivars().preview_edit.borrow_mut();
        *slot = Some(if slot.is_some() { Announced::Unknown } else { announced });
    }

    /// The user typed, deleted or pasted in the preview: translate the change into the source.
    pub(super) fn preview_changed(&self) {
        let announced = self.ivars().preview_edit.borrow_mut().take();
        let preview = self.preview();
        if preview.hasMarkedText() {
            return;
        }
        let snap = self.snapshot();
        if !self.ivars().state.borrow().is_current(&snap) {
            drop(snap);
            self.invalidate(Invalidate::Preview);
            return;
        }
        let new_len = storage(preview).length();
        let (edit, dirty, next) = {
            let st = self.ivars().state.borrow();
            let shown_len: usize = st.blocks.lens.iter().sum();
            let (old_range, inserted) = preview_change(
                announced,
                shown_len,
                new_len,
                &st.layout,
                || preview.string().to_string(),
                preview.selectedRange().location,
            );
            let delta = utf16_len(&inserted) as isize - old_range.len() as isize;
            let bridge = bridge(&snap.text, &st);
            let typed_pending = st
                .pending
                .as_ref()
                .filter(|(_, base)| *base == st.text && old_range.is_empty())
                .and_then(|(pending, _)| bridge.type_with_pending(pending, old_range.start, &inserted));
            let (edit, next) = match typed_pending {
                Some((edit, next)) => (Some(edit), next),
                None => (bridge.replace(old_range.clone(), &inserted), None),
            };
            (edit, (old_range, delta), next)
        };
        drop(snap);
        self.ivars().state.borrow_mut().dirty_preview = Some(dirty);
        match edit {
            Some(edit) => {
                self.apply_edit(edit, None, true);
                if let Some(next) = next {
                    self.ivars().state.borrow_mut().pending = Some((next, self.source_text()));
                }
            }
            None => self.update(),
        }
    }

    pub(super) fn preview_selection_changed(&self) {
        let snap = self.snapshot();
        let sel = {
            let st = self.ivars().state.borrow();
            if !st.is_current(&snap) {
                return;
            }
            bridge(&snap.text, &st).source_selection(from_nsrange(self.preview().selectedRange()))
        };
        self.drop_pending_unless_at(Some(sel.clone()));
        self.set_source_selection(to_nsrange(snap.u16_range(&sel)));
        drop(snap);
        let caret_moved = self.ivars().state.borrow().caret != Some(sel.start);
        if caret_moved {
            self.update();
        }
        self.open_atom_under_caret_later();
    }

    pub(super) fn preview_command(&self, command: Sel) -> bool {
        self.update();
        let sel = from_nsrange(self.preview().selectedRange());
        let p = sel.start;
        let edit = {
            let st = self.ivars().state.borrow();
            let bridge = bridge(&st.text, &st);
            match command {
                c if c == sel!(insertNewline:) && sel.len() == 1 && self.atom_at_preview(&st, p).is_some() => {
                    drop(st);
                    self.edit_atom_at_preview(p, None);
                    return true;
                }
                c if c == sel!(insertNewline:) && sel.is_empty() => bridge.newline(p).map(|e| (e, t("Absatz"))),
                c if (c == sel!(insertLineBreak:) || c == sel!(insertNewlineIgnoringFieldEditor:))
                    && sel.is_empty() =>
                {
                    bridge.line_break(p).map(|e| (e, t("Zeilenumbruch")))
                }
                c if c == sel!(insertTab:) => match bridge.tab(p, false) {
                    Some(e) => Some((e, t("Einrücken"))),
                    None => return !preview_in_code(&st, p),
                },
                c if c == sel!(insertBacktab:) => match bridge.tab(p, true) {
                    Some(e) => Some((e, t("Ausrücken"))),
                    None => return true,
                },
                c if c == sel!(deleteBackward:) && sel.is_empty() => {
                    bridge.delete_at_block_start(p).map(|e| (e, t("Löschen")))
                }
                _ => None,
            }
        };
        match edit {
            Some((edit, name)) => {
                self.apply_edit(edit, Some(name), false);
                true
            }
            None => false,
        }
    }

    pub(super) fn source_command(&self, command: Sel) -> bool {
        if command != sel!(insertTab:) && command != sel!(insertBacktab:) {
            return false;
        }
        self.update();
        let snap = self.snapshot();
        let edit = {
            let sel = self.source_selection(&snap);
            let st = self.ivars().state.borrow();
            table::navigate(&snap.text, &st.doc, sel.start, command == sel!(insertBacktab:))
        };
        drop(snap);
        match edit {
            Some(edit) => {
                self.apply_edit(edit, Some(t("Zelle wechseln")), false);
                true
            }
            None => false,
        }
    }

    pub(super) fn adjust_context_menu(&self, menu: &NSMenu, view: &NSTextView, index: usize) -> Retained<NSMenu> {
        let mtm = self.mtm();
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
            menu.insertItem_atIndex(&item, position as isize);
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
        if let Some(range) = self.atom_at_index(view, index) {
            let diagram = self.snapshot().text[range.clone()].starts_with(['`', '~']);
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
                range.start as isize,
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
        let Some(window) = self.window() else { return };
        let (_, selected) = self.selected_source();
        let this = self.retain();
        // The selection is read again when the sheet closes: a reload from disk can change the text meanwhile.
        super::dialogs::link(&window, &selected, move |url, label| {
            this.update();
            let (sel, _) = this.selected_source();
            let edit = commands::insert_link(sel, &url, &label);
            this.apply_edit(edit, Some(t("Link einfügen")), false);
        });
    }

    fn insert_table(&self) {
        let Some(window) = self.window() else { return };
        let this = self.retain();
        super::dialogs::table(&window, move |rows, cols| {
            this.update();
            let (sel, _) = this.selected_source();
            let edit = commands::insert_table(&this.snapshot().text, sel, rows, cols);
            this.apply_edit(edit, Some(t("Tabelle einfügen")), false);
        });
    }

    fn insert_image(&self) {
        let Some(window) = self.window() else { return };
        let this = self.retain();
        super::dialogs::image(&window, move |url| {
            let Some(path) = url.path() else { return };
            this.update();
            let (sel, _) = this.selected_source();
            let path = PathBuf::from(path.to_string());
            let alt = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let rel = relative_path(this.doc_dir().as_deref(), &path);
            let edit = commands::insert_image(sel, &alt, &rel);
            this.apply_edit(edit, Some(t("Bild einfügen")), false);
        });
    }

    /// A picture on the pasteboard becomes a PNG file next to the document and an image link at the selection.
    /// Returns whether the paste was handled here.
    pub fn paste_image(&self, pasteboard: &NSPasteboard) -> bool {
        // Text wins: spreadsheets and web pages put a picture of the copied text on the pasteboard as well.
        let text = NSArray::from_slice(&[constant!(NSPasteboardTypeString)]);
        if pasteboard.availableTypeFromArray(&text).is_some() {
            return false;
        }
        let png = pasteboard
            .dataForType(constant!(NSPasteboardTypePNG))
            .map(|d| d.to_vec())
            .or_else(|| {
                pasteboard
                    .dataForType(constant!(NSPasteboardTypeTIFF))
                    .and_then(|tiff| png_of(&tiff))
            });
        let Some(png) = png else {
            return false;
        };
        let Some(dir) = self.doc_dir() else {
            self.ask_to_save_before_pasting();
            return true;
        };
        let stem = format!("{} {}", t("Bild"), crate::util::now("yyyy-MM-dd HH.mm.ss"));
        let mut path = dir.join(format!("{stem}.png"));
        for n in 2.. {
            if !path.exists() {
                break;
            }
            path = dir.join(format!("{stem} {n}.png"));
        }
        if std::fs::write(&path, &png).is_err() {
            NSBeep();
            return true;
        }
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
        let Some(window) = self.window() else { return };
        let alert = objc2_app_kit::NSAlert::new(self.mtm());
        alert.setMessageText(&ns(t("Dokument zuerst sichern")));
        alert.setInformativeText(&ns(t(
            "Eingesetzte Bilder werden als Datei neben dem Dokument abgelegt. Sichere es, dann setze das Bild noch einmal ein.",
        )));
        alert.addButtonWithTitle(&ns(t("Sichern …")));
        alert.addButtonWithTitle(&ns(t("Abbrechen")));
        let document = self.ivars().document.clone();
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
    fn selected_source(&self) -> (Range<usize>, String) {
        let snap = self.snapshot();
        let st = self.ivars().state.borrow();
        let sel = self.current_selection(&snap, &st);
        let text = snap.text[sel.clone()].to_string();
        (sel, text)
    }

    /// Dropping files or URLs on the source inserts Markdown images and links.
    pub fn handle_source_drop(&self, pasteboard: &NSPasteboard, index: usize) -> bool {
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
        let at = self.snapshot().byte_at(index);
        let insert = dropped_markdown(&dropped, self.doc_dir().as_deref());
        let end = at + insert.len();
        let edit = Edit {
            range: at..at,
            replacement: insert,
            selection: end..end,
        };
        self.apply_edit(edit, Some(t("Einfügen")), false);
        true
    }
}

pub(super) fn bridge<'a>(text: &'a str, st: &'a State) -> Bridge<'a> {
    Bridge {
        text,
        doc: &st.doc,
        layout: &st.layout,
        index: &st.index,
    }
}

fn preview_in_code(st: &State, p: usize) -> bool {
    st.index.para_at(p).is_some_and(|i| {
        matches!(
            st.index.para(&st.layout, i).style.kind,
            crate::preview::layout::ParaKind::Code { .. }
        )
    })
}

/// Screenshots arrive as TIFF from some apps; Markdown viewers show PNG everywhere.
fn png_of(tiff: &objc2_foundation::NSData) -> Option<Vec<u8>> {
    let rep = objc2_app_kit::NSBitmapImageRep::imageRepWithData(tiff)?;
    // SAFETY: an empty property dictionary, which every file type accepts.
    let png = unsafe {
        rep.representationUsingType_properties(
            objc2_app_kit::NSBitmapImageFileType::PNG,
            &objc2_foundation::NSDictionary::new(),
        )
    }?;
    Some(png.to_vec())
}

/// The change the user made in the preview, as the range of its old text and what replaced it: the change the view
/// announced if it fits the lengths, else the difference between the old and the new text around the caret.
fn preview_change(
    announced: Option<Announced>,
    shown_len: usize,
    new_len: usize,
    layout: &Layout,
    new_text: impl FnOnce() -> String,
    caret: usize,
) -> (Range<usize>, String) {
    match announced {
        Some(Announced::Edit(range, text))
            if range.end <= shown_len && new_len + range.len() == shown_len + utf16_len(&text) =>
        {
            (range, text)
        }
        _ => {
            let new: Vec<u16> = new_text().encode_utf16().collect();
            let old: Vec<u16> = preview_string(layout).encode_utf16().collect();
            let (old_range, new_range) = utf16_diff(&old, &new, caret);
            (old_range, String::from_utf16_lossy(&new[new_range]))
        }
    }
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
    use crate::markdown::doc::parse;
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
        let l = layout(text, &parse(text), &NoImages, None);
        let shown = utf16_len(&preview_string(&l));
        let fits = Some(Announced::Edit(3..3, "x".into()));
        assert_eq!(
            preview_change(fits, shown, shown + 1, &l, || unreachable!(), 4),
            (3..3, "x".into())
        );
        let misfit = Some(Announced::Edit(0..0, "yy".into()));
        assert_eq!(
            preview_change(misfit, shown, shown + 1, &l, || "Einx Wort\n".into(), 4),
            (3..3, "x".into())
        );
        assert_eq!(
            preview_change(Some(Announced::Unknown), shown, shown - 4, &l, || "Ein \n".into(), 4),
            (4..8, String::new())
        );
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
