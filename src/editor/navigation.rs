//! Clicks on links, task checkboxes and footnote markers in the preview.

use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSBeep, NSDocumentController, NSEventModifierFlags, NSTextView, NSWorkspace};
use objc2_foundation::{NSArray, NSRange, NSString, NSURL};

use super::EditorController;
use super::editing::bridge;
use super::links::{self, LinkTarget};
use crate::i18n::{t, tf};
use crate::markdown::commands;
use crate::preview::map::preview_offset;
use crate::util::{ns, storage};

impl EditorController {
    pub(super) fn link_clicked(&self, view: &NSTextView, link: &AnyObject, index: usize) -> bool {
        let target = if let Some(s) = link.downcast_ref::<NSString>() {
            s.to_string()
        } else if let Some(u) = link.downcast_ref::<NSURL>() {
            u.absoluteString().map(|s| s.to_string()).unwrap_or_default()
        } else {
            return false;
        };
        if target == "splat-task" {
            let edit = {
                let st = self.ivars().state.borrow();
                bridge(&st.text, &st).toggle_task(index)
            };
            if let Some(edit) = edit {
                self.apply_edit(edit, Some(t("Aufgabe umschalten")), false);
            }
            return true;
        }
        if target == "splat-fold" {
            let edit = {
                let st = self.ivars().state.borrow();
                bridge(&st.text, &st).toggle_fold(index)
            };
            if let Some(edit) = edit {
                self.apply_edit(edit, Some(t("Hinweisbox auf- oder zuklappen")), false);
            }
            return true;
        }
        if let Some(name) = target.strip_prefix("splat-footnote:") {
            let found = {
                let st = self.ivars().state.borrow();
                st.index
                    .paras
                    .iter()
                    .enumerate()
                    .find(|(i, _)| {
                        matches!(&st.index.para(&st.layout, *i).style.marker, Some(crate::preview::layout::Marker::Footnote(n, _)) if **n == *name)
                    })
                    .map(|(_, pp)| NSRange::new(pp.start, pp.len))
            };
            if let Some(range) = found {
                view.scrollRangeToVisible(range);
                view.showFindIndicatorForRange(range);
            }
            return true;
        }
        let flags = NSApplication::sharedApplication(self.mtm())
            .currentEvent()
            .map(|e| e.modifierFlags());
        if flags.is_some_and(|f| f.contains(NSEventModifierFlags::Command)) {
            self.open_link(&target);
        } else {
            view.setSelectedRange(NSRange::new(index, 0));
        }
        true
    }

    /// Opens a link from the document. Files are only shown in the Finder and other apps' URLs need a confirmation,
    /// so a link can never start a program or script.
    fn open_link(&self, target: &str) {
        let mtm = self.mtm();
        if let Some(tag) = target.strip_prefix("splat-tag:") {
            self.find_in_source(tag);
            return;
        }
        if let Some(reference) = target.strip_prefix("splat-ref:") {
            let url = self
                .doc_dir()
                .as_deref()
                .and_then(super::env::repository)
                .and_then(|r| r.url(reference))
                .and_then(|u| NSURL::URLWithString(&ns(&u)));
            match url {
                Some(url) => {
                    NSWorkspace::sharedWorkspace().openURL(&url);
                }
                None => NSBeep(),
            }
            return;
        }
        let file_url = |path: &std::path::Path| path.to_str().map(|p| NSURL::fileURLWithPath(&ns(p)));
        match links::classify(target, self.doc_dir().as_deref()) {
            LinkTarget::Anchor(anchor) => self.reveal_heading(&anchor),
            LinkTarget::Web(link) => {
                if let Some(url) = NSURL::URLWithString(&ns(&link)) {
                    NSWorkspace::sharedWorkspace().openURL(&url);
                }
            }
            LinkTarget::Document(path) if path.is_file() => {
                if let Some(url) = file_url(&path) {
                    let done = block2::RcBlock::new(
                        |_: *mut objc2_app_kit::NSDocument,
                         _: objc2::runtime::Bool,
                         _: *mut objc2_foundation::NSError| {},
                    );
                    NSDocumentController::sharedDocumentController(mtm)
                        .openDocumentWithContentsOfURL_display_completionHandler(&url, true, &done);
                }
            }
            LinkTarget::Document(path) | LinkTarget::File(path) if path.exists() => {
                if let Some(url) = file_url(&path) {
                    NSWorkspace::sharedWorkspace()
                        .activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
                }
            }
            LinkTarget::App => self.confirm_app_link(target),
            _ => NSBeep(),
        }
    }

    /// Opens the find bar of the source on `text`, as a click on an Obsidian tag searches for it.
    fn find_in_source(&self, text: &str) {
        let mtm = self.mtm();
        // SAFETY: Reading AppKit's constant for the shared find pasteboard.
        let board = objc2_app_kit::NSPasteboard::pasteboardWithName(unsafe { objc2_app_kit::NSPasteboardNameFind });
        board.clearContents();
        // SAFETY: A string for the string type.
        unsafe { board.setString_forType(&ns(text), objc2_app_kit::NSPasteboardTypeString) };
        let source = self.source();
        if let Some(window) = self.window() {
            window.makeFirstResponder(Some(source));
        }
        // The find bar takes its search string from the find pasteboard.
        for action in [
            objc2_app_kit::NSTextFinderAction::ShowFindInterface,
            objc2_app_kit::NSTextFinderAction::NextMatch,
        ] {
            let item = objc2_app_kit::NSMenuItem::new(mtm);
            item.setTag(action.0);
            // SAFETY: The sender is a menu item whose tag names the action, as the method expects.
            unsafe { source.performTextFinderAction(Some(&item)) };
        }
    }

    fn confirm_app_link(&self, target: &str) {
        let workspace = NSWorkspace::sharedWorkspace();
        let Some(url) = NSURL::URLWithString(&ns(target)) else {
            NSBeep();
            return;
        };
        let Some(app) = workspace.URLForApplicationToOpenURL(&url).and_then(|a| a.path()) else {
            NSBeep();
            return;
        };
        let name = objc2_foundation::NSFileManager::defaultManager()
            .displayNameAtPath(&app)
            .to_string();
        let Some(window) = self.window() else { return };
        let alert = objc2_app_kit::NSAlert::new(self.mtm());
        alert.setMessageText(&ns(t("Link in einer anderen App öffnen?")));
        alert.setInformativeText(&ns(&tf(
            "„{}“ wird mit {} geöffnet. Öffne den Link nur, wenn du der Quelle vertraust.",
            &[target, &name],
        )));
        // The safe choice is the default button, so Return does not open the link.
        alert.addButtonWithTitle(&ns(t("Abbrechen")));
        alert.addButtonWithTitle(&ns(t("Öffnen")));
        let handler = block2::RcBlock::new(move |response: objc2_app_kit::NSModalResponse| {
            if response == objc2_app_kit::NSAlertSecondButtonReturn {
                NSWorkspace::sharedWorkspace().openURL(&url);
            }
        });
        alert.beginSheetModalForWindow_completionHandler(&window, Some(&handler));
    }

    /// Scrolls both panes to the heading a `#anchor` link points to.
    pub(super) fn reveal_heading(&self, anchor: &str) {
        let anchor = ns(anchor)
            .stringByRemovingPercentEncoding()
            .map_or_else(|| anchor.to_string(), |s| s.to_string());
        let start = {
            let st = self.ivars().state.borrow();
            let Some((.., range)) = commands::heading_anchors(&st.doc).into_iter().find(|h| h.2 == anchor) else {
                return;
            };
            range.start
        };
        self.reveal_source(start);
    }

    /// Selects source offset `start` and scrolls both panes to it, flashing the line in the preview.
    pub(super) fn reveal_source(&self, start: usize) {
        let p = {
            let st = self.ivars().state.borrow();
            preview_offset(&st.layout, &st.index, start)
        };
        let at = NSRange::new(self.snapshot().u16_at(start), 0);
        self.set_source_selection(at);
        self.source().scrollRangeToVisible(at);
        if self.preview_visible() {
            let string = storage(self.preview()).string();
            let end = string.length();
            let line = (p..end).take_while(|&i| string.characterAtIndex(i) != 10).count();
            self.preview().scrollRangeToVisible(NSRange::new(p, 0));
            self.preview().showFindIndicatorForRange(NSRange::new(p, line.max(1)));
        }
    }
}
