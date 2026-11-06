//! Who may reach whom, lowest first, as the compiler holds it: only this module and the controller see the ways to the
//! components. A component gets a handle that offers what lies below it and nothing else, and what happens across several of
//! them is for the controller to put together. Upward go only `invalidate`, `under_way`, `image_arrived`, `toggle_preview`,
//! `toggle_files` and `search_notes`.

use std::ops::Deref;
use std::path::PathBuf;

use objc2::rc::{Retained, Weak};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSWindow, NSWindowController};
use objc2_foundation::NSUndoManager;

use super::EditorController;
use crate::document::MdDocument;
use crate::editor::atom::AtomEditor;
use crate::editor::changes::ChangeMarks;
use crate::editor::counter::WordCounter;
use crate::editor::editing::Editing;
use crate::editor::export::Sharing;
use crate::editor::images::Pictures;
use crate::editor::model::Model;
use crate::editor::notes::Notes;
use crate::editor::panes::Panes;
use crate::editor::progressive::Progressive;
use crate::editor::scroll::ScrollSync;
use crate::editor::selection::Selection;
use crate::editor::sidebar::SidebarTab;
use crate::editor::state::{Invalidate, State};
use crate::editor::sync::Snapshot;
use crate::editor::update::Updater;

/// What every component may use: the state and the document, which no component owns.
#[derive(Clone, Copy)]
pub(in crate::editor) struct ForPanes<'a>(&'a EditorController);

impl<'a> ForPanes<'a> {
    pub(super) fn of(controller: &'a EditorController) -> Self {
        Self(controller)
    }

    /// The controller as the object AppKit knows, to hand to AppKit or to who reports back to it.
    pub(in crate::editor) fn object(&self) -> &'a EditorController {
        self.0
    }

    pub(in crate::editor) fn model(&self) -> &'a Model {
        self.0.model()
    }

    pub(in crate::editor) fn mtm(&self) -> MainThreadMarker {
        self.0.mtm()
    }

    pub(in crate::editor) fn window(&self) -> Option<Retained<NSWindow>> {
        NSWindowController::window(self.0)
    }

    pub(in crate::editor) fn document(&self) -> Option<Retained<MdDocument>> {
        self.0.document()
    }

    /// For what holds on to the document longer than this call.
    pub(in crate::editor) fn weak_document(&self) -> Weak<MdDocument> {
        self.0.ivars().document.clone()
    }

    pub(in crate::editor) fn undo_manager(&self) -> Option<Retained<NSUndoManager>> {
        self.0.undo_manager()
    }

    pub(in crate::editor) fn doc_dir(&self) -> Option<PathBuf> {
        self.0.doc_dir()
    }

    pub(in crate::editor) fn is_dark(&self) -> bool {
        self.0.is_dark()
    }

    pub(in crate::editor) fn has_closed(&self) -> bool {
        self.0.has_closed()
    }

    pub(in crate::editor) fn with_text<R>(&self, f: impl for<'b> FnOnce(&'b Snapshot, &'b State) -> R + Send) -> R {
        self.0.with_text(f)
    }

    pub(in crate::editor) fn change_with_text<R>(
        &self,
        f: impl for<'b> FnOnce(&'b Snapshot, &'b mut State) -> R + Send,
    ) -> R {
        self.0.change_with_text(f)
    }

    pub(in crate::editor) fn on_source_text<R>(&self, f: impl for<'b> FnOnce(&'b str) -> R + Send) -> R {
        self.0.on_source_text(f)
    }

    pub(in crate::editor) fn source_text(&self) -> String {
        self.0.source_text()
    }

    /// Marks what is outdated and asks for an update, whoever is the one to notice.
    pub(in crate::editor) fn invalidate(&self, why: Invalidate) {
        self.0.invalidate(why);
    }

    /// What goes on in the views right now, which only the state of the editor above can say about its edits.
    pub(in crate::editor) fn under_way(&self) -> UnderWay {
        UnderWay {
            own_change: self.0.panes().is_silent(),
            preview_edit: self.0.ivars().edits.preview_editing(),
            composition: self.0.panes().is_built() && self.0.panes().preview().has_marked_text(),
        }
    }
}

/// What goes on in the views that an update must not cut into.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(in crate::editor) struct UnderWay {
    /// The controller changes its own views.
    pub(in crate::editor) own_change: bool,
    /// The preview announced an edit, which ends with this turn of the run loop.
    pub(in crate::editor) preview_edit: bool,
    /// The input method holds marked text in the preview, whose offsets a new text would make wrong.
    pub(in crate::editor) composition: bool,
}

impl UnderWay {
    /// Whether a result that can wait has to wait.
    pub(in crate::editor) fn defers_background_results(self) -> bool {
        self.own_change || self.preview_edit
    }

    /// Whether even an update the caller reads the result of has to wait: the end of a composition comes back to it.
    pub(in crate::editor) fn holds_back_an_update(self) -> bool {
        self.own_change || self.composition
    }
}

macro_rules! layer {
    ($(#[$doc:meta])* $name:ident above $below:ident, reaches $method:ident -> $component:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy)]
        pub(in crate::editor) struct $name<'a>($below<'a>);

        impl<'a> $name<'a> {
            pub(super) fn of(controller: &'a EditorController) -> Self {
                Self($below::of(controller))
            }

            pub(in crate::editor) fn $method(&self) -> $component<'a> {
                self.object().$method()
            }
        }

        impl<'a> Deref for $name<'a> {
            type Target = $below<'a>;

            fn deref(&self) -> &$below<'a> {
                &self.0
            }
        }
    };
}

layer!(ForSelection above ForPanes, reaches panes -> Panes);
layer!(ForPictures above ForSelection, reaches selection -> Selection);
layer!(ForProgressive above ForPictures, reaches pictures -> Pictures);
layer!(ForCounter above ForProgressive, reaches progressive -> Progressive);
layer!(ForChanges above ForCounter, reaches word_counter -> WordCounter);
layer!(ForScroll above ForChanges, reaches change_marks -> ChangeMarks);
layer!(ForUpdater above ForScroll, reaches scroll_sync -> ScrollSync);
layer!(ForSharing above ForUpdater, reaches updater -> Updater);
layer!(ForAtoms above ForSharing, reaches sharing -> Sharing);
layer!(ForEditing above ForAtoms, reaches atom_editor -> AtomEditor);
layer!(ForNotes above ForEditing, reaches editing -> Editing);
layer!(ForLinks above ForNotes, reaches notes -> Notes);

impl ForPictures<'_> {
    /// A picture that arrived reports to whoever waits for it, up to the update that shows it.
    pub(in crate::editor) fn image_arrived(&self, key: String) {
        self.object().updater().image_arrived(key);
    }
}

impl ForEditing<'_> {
    pub(in crate::editor) fn toggle_preview(&self) {
        self.object().toggle_preview();
    }

    pub(in crate::editor) fn toggle_files(&self) {
        if self.object().panes().toggle_tab(SidebarTab::Files) {
            self.object().notes().rescan();
        }
    }

    pub(in crate::editor) fn search_notes(&self) {
        self.object().notes().show_search();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_composition_holds_back_an_update_and_an_edit_only_what_can_wait() {
        let under_way = |own_change, preview_edit, composition| UnderWay {
            own_change,
            preview_edit,
            composition,
        };
        for (now, can_wait, update) in [
            (under_way(false, false, false), false, false),
            (under_way(true, false, false), true, true),
            (under_way(false, true, false), true, false),
            (under_way(false, false, true), false, true),
        ] {
            assert_eq!(now.defers_background_results(), can_wait, "{now:?}");
            assert_eq!(now.holds_back_an_update(), update, "{now:?}");
        }
    }
}
