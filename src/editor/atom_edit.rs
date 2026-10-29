//! Which formula or diagram lies where, and the life of its editor: opening it, mirroring its text into the
//! source while typing, and committing or discarding the edit when it closes.

use std::ops::Range;

use objc2::runtime::{AnyObject, Sel};
use objc2::{DefinedClass, MainThreadOnly, sel};
use objc2_app_kit::{NSDocumentChangeType, NSTextView};
use objc2_foundation::NSRect;

use super::atom::AtomKind;
use super::{EditorController, State, mathfield};
use crate::i18n::t;
use crate::markdown::commands::{self, Edit};
use crate::markdown::doc::{BlockKind, Doc, block_path, figure_at};
use crate::preview::layout::{Atom, ParaKind, RunMap};
use crate::util::{ns, perform_later, storage, to_nsrange};

impl EditorController {
    /// Inserts a formula and opens its editor right away: an empty `$$` block is no formula yet for the parser, so
    /// neither the preview nor a click could open the editor on it.
    pub(super) fn insert_formula(&self) {
        self.update();
        let edit = {
            let snap = self.snapshot();
            let st = self.ivars().state.borrow();
            commands::insert_formula(&snap.text, self.current_selection(&snap, &st))
        };
        let start = edit.range.start;
        let inserted = (start, start + edit.replacement.len());
        let fresh = edit.range.is_empty();
        let atom = if edit.replacement.starts_with("$$") || !edit.replacement.starts_with('$') {
            edit.replacement
                .find("$$")
                .zip(edit.replacement.rfind("$$"))
                .map(|(a, b)| start + a..start + b + 2)
        } else {
            Some(start..start + edit.replacement.len())
        };
        self.apply_edit(edit, Some(t("Formel")), false);
        let Some(atom) = atom else { return };
        self.ivars().atoms.closed.set(None);
        self.open_atom_editor(atom, None);
        if fresh && let Some(e) = self.ivars().atoms.edit.borrow().as_ref() {
            e.inserted.set(Some(inserted));
        }
    }

    /// The caret landed on a formula or diagram in the preview: its editor opens once the selection has settled.
    pub(super) fn open_atom_under_caret_later(&self) {
        let Some(range) = self.atom_under_preview_caret() else {
            self.ivars().atoms.closed.set(None);
            return;
        };
        if self.ivars().atoms.closed.get() != Some(range.start) {
            perform_later(self, sel!(openAtomUnderCaret), 0.0, false);
        }
    }

    pub(super) fn atom_under_preview_caret(&self) -> Option<Range<usize>> {
        if self.ivars().atoms.edit.borrow().is_some() || !self.preview_focused() {
            return None;
        }
        let r = self.preview().selectedRange();
        if r.length > 1 {
            return None;
        }
        let snap = self.snapshot();
        let st = self.ivars().state.borrow();
        if !st.is_current(&snap) {
            return None;
        }
        self.atom_at_preview(&st, r.location)
    }

    /// Source range of the formula or diagram rendered at preview offset `p`.
    pub(super) fn atom_at_preview(&self, st: &State, p: usize) -> Option<Range<usize>> {
        let pi = st.index.para_at(p)?;
        let pp = st.index.paras[pi];
        let para = st.index.para(&st.layout, pi);
        if matches!(para.style.kind, ParaKind::MathBlock | ParaKind::Diagram) {
            return Some(para.src.clone());
        }
        let mut u = pp.start;
        for run in &para.runs {
            let len = run.text.encode_utf16().count();
            if let RunMap::Atom {
                atom: Atom::Math { .. } | Atom::Diagram { .. },
                src,
            } = &run.map
                && u <= p
                && p < u + len.max(1)
            {
                return Some(src.clone());
            }
            u += len;
        }
        None
    }

    pub(super) fn atom_at_selection(&self) -> Option<Range<usize>> {
        let snap = self.snapshot();
        let st = self.ivars().state.borrow();
        if !st.is_current(&snap) {
            return None;
        }
        if self.preview_focused() {
            return self.atom_near_preview(&st, self.preview().selectedRange().location);
        }
        let at = self.source_selection(&snap).start;
        atom_at_source(&st.doc, at)
    }

    /// Formula or diagram at a preview offset, also when the caret sits right behind it in the same paragraph.
    fn atom_near_preview(&self, st: &State, p: usize) -> Option<Range<usize>> {
        self.atom_at_preview(st, p).or_else(|| {
            let before = p.checked_sub(1)?;
            (st.index.para_at(before) == st.index.para_at(p))
                .then(|| self.atom_at_preview(st, before))
                .flatten()
        })
    }

    /// Formula or diagram at a character index of the source or preview view, e.g. where a context menu opened.
    pub(super) fn atom_at_index(&self, view: &NSTextView, index: usize) -> Option<Range<usize>> {
        let snap = self.snapshot();
        let st = self.ivars().state.borrow();
        if !st.is_current(&snap) {
            return None;
        }
        if self.is_preview(Some(view)) {
            self.atom_near_preview(&st, index)
        } else {
            atom_at_source(&st.doc, snap.byte_at(index))
        }
    }

    pub(super) fn atom_starting_at(&self, start: usize) -> Option<Range<usize>> {
        let st = self.ivars().state.borrow();
        atom_at_source(&st.doc, start).filter(|r| r.start == start)
    }

    /// The editable part of a formula or diagram, the TeX or Mermaid code without delimiters, and what it is.
    pub(super) fn atom_content(&self, text: &str, range: &Range<usize>) -> (Range<usize>, AtomKind) {
        let st = self.ivars().state.borrow();
        let path = block_path(&st.doc.blocks, range.start);
        if let Some(BlockKind::CodeBlock { content, info, .. }) = path.last().map(|b| &b.kind)
            && path.last().is_some_and(|b| b.range == *range)
        {
            let end = if text[content.clone()].ends_with('\n') {
                content.end - 1
            } else {
                content.end
            };
            let kind = match crate::markdown::doc::diagram_kind(info) {
                Some(language) => AtomKind::Diagram(language),
                None => AtomKind::Formula { display: true },
            };
            return (content.start..end.max(content.start), kind);
        }
        let src = &text[range.clone()];
        let display = src.starts_with("$$") || src.starts_with("\\[");
        let (open, close) = crate::markdown::doc::math_delimiters(src);
        let mut start = range.start + open;
        let mut end = range.end - close;
        if display {
            if text[start..end].starts_with('\n') {
                start += 1;
            }
            if end > start && text[start..end].ends_with('\n') {
                end -= 1;
            }
        }
        (start..end.max(start), AtomKind::Formula { display })
    }

    pub(super) fn edit_atom_at_selection(&self) {
        let Some(range) = self.atom_at_selection() else { return };
        self.open_atom_editor(range, None);
    }

    pub(super) fn edit_atom_at_preview(&self, p: usize, frame: Option<NSRect>) {
        let range = {
            let snap = self.snapshot();
            let st = self.ivars().state.borrow();
            if !st.is_current(&snap) {
                return;
            }
            self.atom_at_preview(&st, p)
        };
        if let Some(range) = range {
            self.open_atom_editor(range, frame);
        }
    }

    pub(super) fn is_atom_view(&self, object: Option<&AnyObject>) -> bool {
        let edit = self.ivars().atoms.edit.borrow();
        match (edit.as_ref(), object) {
            (Some(e), Some(o)) => std::ptr::eq(o, &*e.view as &AnyObject),
            _ => false,
        }
    }

    /// Mirrors the popover text into the source without undo, so the preview follows while typing.
    fn replace_silently(&self, range: Range<usize>, replacement: &str) {
        let target = {
            let snap = self.snapshot();
            if range.end > snap.text.len() {
                return;
            }
            to_nsrange(snap.u16_range(&range))
        };
        self.ivars().state.borrow_mut().pending = None;
        let undo = self.undo_manager();
        if let Some(u) = &undo {
            u.disableUndoRegistration();
        }
        {
            let _silent = self.silently();
            storage(self.source()).replaceCharactersInRange_withString(target, &ns(replacement));
        }
        if let Some(u) = &undo {
            u.enableUndoRegistration();
        }
        self.update();
    }

    pub(super) fn atom_text_changed(&self) {
        let (range, new) = {
            let edit = self.ivars().atoms.edit.borrow();
            let Some(e) = edit.as_ref() else { return };
            let new = e.view.string().to_string();
            let range = e.start..e.start + e.len.get();
            e.len.set(new.len());
            if !e.marked_edited.replace(true)
                && let Some(document) = self.document()
            {
                document.updateChangeCount(NSDocumentChangeType::ChangeDone);
            }
            (range, new)
        };
        self.replace_silently(range, &new);
        self.color_atom_code();
        self.refresh_atom_status();
    }

    pub(super) fn atom_command(&self, command: Sel) -> bool {
        if command == sel!(cancelOperation:) {
            self.cancel_atom_edit();
            return true;
        }
        if command == sel!(insertTab:) || command == sel!(insertBacktab:) {
            return self.atom_tab(command == sel!(insertBacktab:));
        }
        false
    }

    /// Closing the popover commits the edit as one undo step; Escape discards it.
    pub(super) fn atom_edit_closed(&self) {
        let Some(edit) = self.ivars().atoms.edit.borrow_mut().take() else {
            return;
        };
        if edit.visual.get() {
            mathfield::field(self.mtm()).detach();
        }
        self.ivars().atoms.closed.set(Some(edit.range_start));
        if edit.marked_edited.get()
            && let Some(document) = self.document()
        {
            document.updateChangeCount(NSDocumentChangeType::ChangeUndone);
        }
        let current = edit.start..edit.start + edit.len.get();
        let final_text = edit.view.string().to_string();
        self.replace_silently(current, &edit.original);
        // Only Escape discards a new formula: a click into the source closes the editor too, to type the formula
        // there.
        if let Some((a, b)) = edit
            .inserted
            .get()
            .filter(|_| edit.cancelled.get() && edit.original.trim().is_empty())
        {
            self.apply_edit(
                Edit {
                    range: a..b,
                    replacement: String::new(),
                    selection: a..a,
                },
                Some(t("Formel")),
                false,
            );
        } else if !edit.cancelled.get() && final_text != edit.original {
            let range = edit.start..edit.start + edit.original.len();
            // The caret goes behind the closing delimiter: inside, the next formula would land in this one.
            let closing = {
                let snap = self.snapshot();
                let after = &snap.text[range.end.min(snap.text.len())..];
                match edit.kind {
                    AtomKind::Diagram(_) => after
                        .strip_prefix('\n')
                        .map_or(0, |r| 1 + r.find('\n').unwrap_or(r.len())),
                    AtomKind::Formula { .. } => ["\n$$", "$$", "$"]
                        .iter()
                        .find(|d| after.starts_with(**d))
                        .map_or(0, |d| d.len()),
                }
            };
            let end = edit.start + final_text.len() + closing;
            self.apply_edit(
                Edit {
                    range,
                    replacement: final_text,
                    selection: end..end,
                },
                Some(edit.kind.undo_name()),
                false,
            );
        }
        // A click into one of the views closed the editor; that view keeps the focus.
        if let Some(w) = self.window() {
            let responder = w.firstResponder();
            let responder: Option<&AnyObject> = responder.as_deref().map(|r| r.as_ref());
            if !self.is_source(responder) && !self.is_preview(responder) {
                w.makeFirstResponder(Some(if self.preview_visible() {
                    self.preview() as &NSTextView
                } else {
                    self.source()
                }));
            }
        }
    }
}

fn atom_at_source(doc: &Doc, at: usize) -> Option<Range<usize>> {
    figure_at(doc, at).map(|(range, _)| range)
}
