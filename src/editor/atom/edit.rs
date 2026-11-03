//! Which formula or diagram lies where, and the life of its editor: opening it, mirroring its text into the
//! source while typing, and committing or discarding the edit when it closes.

use std::ops::Range;

use objc2::runtime::{AnyObject, Sel};
use objc2::{MainThreadOnly, sel};
use objc2_app_kit::{NSDocumentChangeType, NSTextView};
use objc2_foundation::NSRect;

use super::{AtomEditor, AtomKind};
use crate::editor::{State, mathfield};
use crate::i18n::t;
use crate::markdown::commands::{self, Edit};
use crate::markdown::doc::{BlockKind, Doc, block_path, figure_at};
use crate::markdown::text::{Byte, Shown, TextAt, U16, units};
use crate::preview::layout::{Atom, ParaKind, RunMap};
use crate::util::{contained, ns, perform_later, storage, to_nsrange};

/// What closing a formula or diagram editor does to the document.
#[derive(Debug, PartialEq, Eq)]
enum AtomOutcome {
    /// The source stays as it was before the editor opened.
    Nothing,
    /// The text inserted for a new formula goes again.
    RemoveInserted(Byte, Byte),
    /// What the editor holds replaces what it started with, as one undo step.
    Commit,
}

/// Only Escape discards a new formula: a click into the source closes the editor too, to type the formula there.
fn outcome_of_closing(
    cancelled: bool,
    original: &str,
    inserted: Option<(Byte, Byte)>,
    final_text: &str,
) -> AtomOutcome {
    match inserted {
        Some((a, b)) if cancelled && original.trim().is_empty() => AtomOutcome::RemoveInserted(a, b),
        _ if !cancelled && final_text != original => AtomOutcome::Commit,
        _ => AtomOutcome::Nothing,
    }
}

impl AtomEditor<'_> {
    /// Inserts a formula and opens its editor right away: an empty `$$` block is no formula yet for the parser, so
    /// neither the preview nor a click could open the editor on it.
    pub(in crate::editor) fn insert_formula(&self) {
        self.host.updater().update_now();
        let selection = self.host.selection().view_selection();
        let edit = self.host.with_text(move |snap, st| {
            let sel = selection.in_source(snap, st);
            contained(|| commands::insert_formula(&snap.text, sel))
        });
        let Some(edit) = edit else {
            return;
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
        self.host.editing().apply_edit(edit, Some(t("Formel")), false);
        let Some(atom) = atom else { return };
        self.state.forget_closed();
        self.open(atom, None);
        if fresh && let Some(e) = self.state.edit.borrow().as_ref() {
            e.inserted.set(Some(inserted));
        }
    }

    /// The caret landed on a formula or diagram in the preview: its editor opens once the selection has settled.
    pub(in crate::editor) fn open_under_caret_later(&self) {
        let Some(range) = self.under_preview_caret() else {
            self.state.forget_closed();
            return;
        };
        if self.state.opens_at(range.start) {
            perform_later(self.host, sel!(openAtomUnderCaret), 0.0, false);
        }
    }

    /// The selection has settled since `open_under_caret_later`.
    pub(in crate::editor) fn open_under_caret(&self) {
        if let Some(range) = self.under_preview_caret()
            && self.state.opens_at(range.start)
        {
            self.open(range, None);
        }
    }

    pub(in crate::editor) fn under_preview_caret(&self) -> Option<Range<Byte>> {
        if self.state.edit.borrow().is_some() || !self.host.panes().preview_focused() {
            return None;
        }
        let r = self.host.panes().preview().selected_range();
        if r.length > 1 {
            return None;
        }
        let p = Shown::new(r.location);
        self.host.with_text(move |snap, st| {
            if !st.is_current(snap) {
                return None;
            }
            atom_at_preview(st, p)
        })
    }

    pub(in crate::editor) fn at_selection(&self) -> Option<Range<Byte>> {
        let selection = self.host.selection().view_selection();
        self.host.with_text(move |snap, st| {
            if !st.is_current(snap) {
                return None;
            }
            match &selection.preview {
                Some(shown) => atom_near_preview(st, shown.start),
                None => atom_at_source(st.doc(), snap.byte_range(&selection.source).start),
            }
        })
    }

    /// Formula or diagram at a character index of the source or preview view, e.g. where a context menu opened.
    pub(in crate::editor) fn at_index(&self, view: &NSTextView, index: usize) -> Option<Range<Byte>> {
        let in_preview = self.host.panes().is_preview(Some(view));
        self.host.with_text(move |snap, st| {
            if !st.is_current(snap) {
                return None;
            }
            if in_preview {
                atom_near_preview(st, Shown::new(index))
            } else {
                atom_at_source(st.doc(), snap.byte_at(U16::new(index)))
            }
        })
    }

    pub(in crate::editor) fn starting_at(&self, start: Byte) -> Option<Range<Byte>> {
        self.host
            .model()
            .read(move |st| atom_at_source(st.doc(), start).filter(|r| r.start == start))
    }

    pub(in crate::editor) fn edit_at_selection(&self) {
        let Some(range) = self.at_selection() else { return };
        self.open(range, None);
    }

    pub(in crate::editor) fn edit_at_preview(&self, p: Shown, frame: Option<NSRect>) {
        let range = self.host.with_text(move |snap, st| {
            if !st.is_current(snap) {
                return None;
            }
            atom_at_preview(st, p)
        });
        if let Some(range) = range {
            self.open(range, frame);
        }
    }

    pub(in crate::editor) fn is_code_view(&self, object: Option<&AnyObject>) -> bool {
        let edit = self.state.edit.borrow();
        match (edit.as_ref(), object) {
            (Some(e), Some(o)) => std::ptr::eq(o, &*e.view as &AnyObject),
            _ => false,
        }
    }

    /// Mirrors the popover text into the source without undo, so the preview follows while typing.
    fn mirror_into_source(&self, range: Range<Byte>, replacement: &str) {
        let target = self
            .host
            .with_text(move |snap, _| (range.end <= snap.text.end()).then(|| to_nsrange(snap.u16_range(&range))));
        let Some(target) = target else {
            return;
        };
        self.host.model().change(|st| st.clear_pending());
        let undo = self.host.undo_manager();
        if let Some(u) = &undo {
            u.disableUndoRegistration();
        }
        self.host
            .panes()
            .silently(|views| storage(views.source).replaceCharactersInRange_withString(target, &ns(replacement)));
        if let Some(u) = &undo {
            u.enableUndoRegistration();
        }
        self.host.updater().update_now();
    }

    pub(in crate::editor) fn text_changed(&self) {
        let (range, new) = {
            let edit = self.state.edit.borrow();
            let Some(e) = edit.as_ref() else { return };
            let new = e.view.string().to_string();
            let range = e.start..e.start + e.len.get();
            e.len.set(new.len());
            if !e.marked_edited.replace(true)
                && let Some(document) = self.host.document()
            {
                document.updateChangeCount(NSDocumentChangeType::ChangeDone);
            }
            (range, new)
        };
        self.mirror_into_source(range, &new);
        self.color_code();
        self.show_status();
    }

    pub(in crate::editor) fn command(&self, command: Sel) -> bool {
        if command == sel!(cancelOperation:) {
            self.cancel();
            return true;
        }
        if command == sel!(insertTab:) || command == sel!(insertBacktab:) {
            return self.tab(command == sel!(insertBacktab:));
        }
        false
    }

    /// The whole text is about to change under an open editor, whose offsets then mean nothing: it closes, and nothing
    /// of it reaches the new text.
    pub(in crate::editor) fn abandon(&self) {
        let Some(edit) = self.state.edit.borrow_mut().take() else {
            return;
        };
        if edit.visual.get() {
            mathfield::field(self.host.mtm()).detach(self.host);
        }
        edit.popover.close();
    }

    /// Commits an open editor and closes it, for what is about to change the source.
    pub(in crate::editor) fn finish(&self) {
        let popover = self.state.edit.borrow().as_ref().map(|e| e.popover.clone());
        if let Some(popover) = popover {
            self.editor_closed();
            popover.close();
        }
    }

    /// Closing the popover commits the edit as one undo step; Escape discards it.
    pub(in crate::editor) fn editor_closed(&self) {
        let Some(edit) = self.state.edit.borrow_mut().take() else {
            return;
        };
        if edit.visual.get() {
            mathfield::field(self.host.mtm()).detach(self.host);
        }
        self.state.closed.set(Some(edit.range_start));
        if edit.marked_edited.get()
            && let Some(document) = self.host.document()
        {
            document.updateChangeCount(NSDocumentChangeType::ChangeUndone);
        }
        let current = edit.start..edit.start + edit.len.get();
        let final_text = edit.view.string().to_string();
        self.mirror_into_source(current, &edit.original);
        let outcome = outcome_of_closing(edit.cancelled.get(), &edit.original, edit.inserted.get(), &final_text);
        if let AtomOutcome::RemoveInserted(a, b) = outcome {
            self.host.editing().apply_edit(
                Edit {
                    range: a..b,
                    replacement: String::new(),
                    selection: a..a,
                },
                Some(t("Formel")),
                false,
            );
        } else if outcome == AtomOutcome::Commit {
            let range = edit.start..edit.start + edit.original.len();
            // The caret goes behind the closing delimiter: inside, the next formula would land in this one.
            let kind = edit.kind;
            let end_of_atom = range.end;
            let closing = self.host.with_text(move |snap, _| {
                let after = snap.text.from(end_of_atom.min(snap.text.end()));
                match kind {
                    AtomKind::Diagram(_) => after
                        .strip_prefix('\n')
                        .map_or(0, |r| 1 + r.find('\n').unwrap_or(r.len())),
                    AtomKind::Formula { .. } => ["\n$$", "$$", "$"]
                        .iter()
                        .find(|d| after.starts_with(**d))
                        .map_or(0, |d| d.len()),
                }
            });
            let end = edit.start + final_text.len() + closing;
            self.host.editing().apply_edit(
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
        if let Some(w) = self.host.window() {
            let responder = w.firstResponder();
            let responder: Option<&AnyObject> = responder.as_deref().map(|r| r.as_ref());
            if !self.host.panes().is_source(responder) && !self.host.panes().is_preview(responder) {
                let views = self.host.panes().for_appkit();
                w.makeFirstResponder(Some(if self.host.panes().preview_visible() {
                    views.preview as &NSTextView
                } else {
                    views.source
                }));
            }
        }
    }
}

fn atom_at_source(doc: &Doc, at: Byte) -> Option<Range<Byte>> {
    figure_at(doc, at).map(|(range, _)| range)
}

/// The editable part of a formula or diagram, the TeX or Mermaid code without delimiters, and what it is.
pub(super) fn atom_content(st: &State, text: &str, range: &Range<Byte>) -> (Range<Byte>, AtomKind) {
    let path = block_path(&st.doc().blocks, range.start);
    if let Some(BlockKind::CodeBlock { content, info, .. }) = path.last().map(|b| &b.kind)
        && path.last().is_some_and(|b| b.range == *range)
    {
        let end = if text.at(content.clone()).ends_with('\n') {
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
    let src = text.at(range.clone());
    let display = src.starts_with("$$") || src.starts_with("\\[");
    let (open, close) = crate::markdown::doc::math_delimiters(src);
    let mut start = range.start + open;
    let mut end = range.end - close;
    if display {
        if text.at(start..end).starts_with('\n') {
            start += 1;
        }
        if end > start && text.at(start..end).ends_with('\n') {
            end -= 1;
        }
    }
    (start..end.max(start), AtomKind::Formula { display })
}

/// Source range of the formula or diagram rendered at preview offset `p`.
pub(in crate::editor) fn atom_at_preview(st: &State, p: Shown) -> Option<Range<Byte>> {
    let pi = st.index().para_at(p)?;
    let pp = st.index().paras[pi];
    let para = st.index().para(st.layout(), pi);
    if matches!(para.style.kind, ParaKind::MathBlock | ParaKind::Diagram) {
        return Some(para.src.clone());
    }
    para.runs_at(pp.start).find_map(|(at, run)| match &run.map {
        RunMap::Atom {
            atom: Atom::Math { .. } | Atom::Diagram { .. },
            src,
        } if at.start <= p && p < at.start + units(&at).max(1) => Some(src.clone()),
        _ => None,
    })
}

/// Formula or diagram at a preview offset, also when the caret sits right behind it in the same paragraph.
fn atom_near_preview(st: &State, p: Shown) -> Option<Range<Byte>> {
    atom_at_preview(st, p).or_else(|| {
        let before = Shown::new(p.get().checked_sub(1)?);
        (st.index().para_at(before) == st.index().para_at(p))
            .then(|| atom_at_preview(st, before))
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_an_editor_commits_only_what_changed_and_was_not_cancelled() {
        assert_eq!(outcome_of_closing(false, "x", None, "y"), AtomOutcome::Commit);
        assert_eq!(outcome_of_closing(false, "x", None, "x"), AtomOutcome::Nothing);
        assert_eq!(outcome_of_closing(true, "x", None, "y"), AtomOutcome::Nothing);
        assert_eq!(
            outcome_of_closing(true, "", Some((Byte::new(3), Byte::new(8))), "y"),
            AtomOutcome::RemoveInserted(Byte::new(3), Byte::new(8)),
            "Escape takes a new formula back"
        );
        assert_eq!(
            outcome_of_closing(false, "", Some((Byte::new(3), Byte::new(8))), "y"),
            AtomOutcome::Commit,
            "a click elsewhere keeps it"
        );
        assert_eq!(
            outcome_of_closing(true, "x", Some((Byte::new(3), Byte::new(8))), "y"),
            AtomOutcome::Nothing
        );
    }
}
