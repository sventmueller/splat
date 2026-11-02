//! Clicks on links, task checkboxes and footnote markers in the preview.

use std::path::Path;

use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSBeep, NSDocumentController, NSEventModifierFlags, NSTextView, NSWorkspace};
use objc2_foundation::{NSArray, NSRange, NSString, NSURL};

use super::editing::bridge;
use super::links::{self, LinkTarget};
use super::{EditorController, State};
use crate::i18n::{t, tf};
use crate::markdown::commands;
use crate::preview::layout::{Link, Marker};
use crate::preview::map::{Shown, preview_offset};
use crate::util::{DISK, constant, contained, lane_of, ns, storage};

/// Goes where a local link leads, once that is known: a Markdown file (`true`) opens in Splat, anything else only shows
/// in the Finder.
fn open_place(mtm: objc2::MainThreadMarker, place: Option<(std::path::PathBuf, bool)>) {
    let Some((path, markdown)) = place else {
        NSBeep();
        return;
    };
    let Some(url) = path.to_str().map(|p| NSURL::fileURLWithPath(&ns(p))) else {
        return;
    };
    if markdown {
        let done = block2::RcBlock::new(
            |_: *mut objc2_app_kit::NSDocument, _: objc2::runtime::Bool, _: *mut objc2_foundation::NSError| {},
        );
        NSDocumentController::sharedDocumentController(mtm)
            .openDocumentWithContentsOfURL_display_completionHandler(&url, true, &done);
    } else {
        NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
    }
}

/// Looks at a file or folder, which can take as long as a network volume does, so only the answer comes back to the main
/// thread.
fn open_found_in_background(place: Option<&Path>, find: impl FnOnce() -> LinkTarget + Send + 'static) {
    place.map_or(&DISK, lane_of).run(move || {
        let opened = match find() {
            LinkTarget::Document(path) if path.is_file() => Some((path, true)),
            LinkTarget::Document(path) | LinkTarget::File(path) if path.exists() => Some((path, false)),
            _ => None,
        };
        dispatch2::DispatchQueue::main().exec_async(move || {
            if let Some(mtm) = objc2::MainThreadMarker::new() {
                open_place(mtm, opened);
            }
        });
    });
}

impl EditorController {
    /// What `f` makes of the state, if that still belongs to the text: an edit made from an older one would land somewhere
    /// else in it. The click meant what the preview showed then, so it goes unanswered, and the preview catches up.
    fn on_current_state<T>(&self, f: impl FnOnce(&State) -> Option<T>) -> Option<T> {
        let snap = self.snapshot();
        let current = self.ivars().state.borrow().is_current(&snap);
        if !current {
            drop(snap);
            self.update();
            return None;
        }
        f(&self.ivars().state.borrow())
    }

    pub(super) fn link_clicked(&self, view: &NSTextView, link: &AnyObject, index: usize) -> bool {
        let target = if let Some(s) = link.downcast_ref::<NSString>() {
            s.to_string()
        } else if let Some(u) = link.downcast_ref::<NSURL>() {
            u.absoluteString().map(|s| s.to_string()).unwrap_or_default()
        } else {
            return false;
        };
        let shown = Shown(index);
        let link = Link::from_attribute(&target);
        match &link {
            Link::Task => {
                let edit = self.on_current_state(|st| contained(|| bridge(st.text(), st).toggle_task(shown)).flatten());
                if let Some(edit) = edit {
                    self.apply_edit(edit, Some(t("Aufgabe umschalten")), false);
                }
                return true;
            }
            Link::Fold => {
                let edit = self.on_current_state(|st| contained(|| bridge(st.text(), st).toggle_fold(shown)).flatten());
                if let Some(edit) = edit {
                    self.apply_edit(edit, Some(t("Hinweisbox auf- oder zuklappen")), false);
                }
                return true;
            }
            Link::Footnote(name) => {
                let found = {
                    let st = self.ivars().state.borrow();
                    st.index()
                        .paras
                        .iter()
                        .enumerate()
                        .find(|(i, _)| {
                            matches!(&st.index().para(st.layout(), *i).style.marker, Some(Marker::Footnote(n, _)) if n == name)
                        })
                        .map(|(_, pp)| NSRange::new(pp.start.0, pp.len))
                };
                if let Some(range) = found {
                    view.scrollRangeToVisible(range);
                    view.showFindIndicatorForRange(range);
                }
                return true;
            }
            Link::Address(_) | Link::WikiPage(_) | Link::Tag(_) | Link::Reference(_) => {}
        }
        let flags = NSApplication::sharedApplication(self.mtm())
            .currentEvent()
            .map(|e| e.modifierFlags());
        if flags.is_some_and(|f| f.contains(NSEventModifierFlags::Command)) {
            self.open_link(&link);
        } else {
            view.setSelectedRange(NSRange::new(index, 0));
        }
        true
    }

    /// Opens a link from the document. Files are only shown in the Finder and other apps' URLs need a confirmation,
    /// so a link can never start a program or script.
    fn open_link(&self, link: &Link) {
        match link {
            Link::Tag(tag) => self.find_in_source(tag),
            Link::Reference(reference) => {
                let url = super::env::Env::known(self.doc_dir(), "")
                    .repository
                    .and_then(|r| r.url(reference))
                    .and_then(|u| NSURL::URLWithString(&ns(&u)));
                match url {
                    Some(url) => {
                        NSWorkspace::sharedWorkspace().openURL(&url);
                    }
                    None => NSBeep(),
                }
            }
            Link::WikiPage(name) => {
                let (name, doc_dir) = (name.clone(), self.doc_dir());
                let place = doc_dir.clone();
                open_found_in_background(place.as_deref(), move || {
                    links::classify_wiki(&name, doc_dir.as_deref())
                });
            }
            Link::Address(address) => match links::classify(address, self.doc_dir().as_deref()) {
                LinkTarget::Anchor(anchor) => self.reveal_heading(&anchor),
                LinkTarget::Web(address) => {
                    if let Some(url) = NSURL::URLWithString(&ns(&address)) {
                        NSWorkspace::sharedWorkspace().openURL(&url);
                    }
                }
                LinkTarget::App => self.confirm_app_link(address),
                LinkTarget::Nothing => NSBeep(),
                found => {
                    let place = match &found {
                        LinkTarget::Document(path) | LinkTarget::File(path) => Some(path.clone()),
                        _ => None,
                    };
                    open_found_in_background(place.as_deref(), move || found);
                }
            },
            Link::Footnote(_) | Link::Fold | Link::Task => {}
        }
    }

    /// Opens the find bar of the source on `text`, as a click on an Obsidian tag searches for it.
    fn find_in_source(&self, text: &str) {
        let mtm = self.mtm();
        let board = objc2_app_kit::NSPasteboard::pasteboardWithName(constant!(objc2_app_kit::NSPasteboardNameFind));
        board.clearContents();
        board.setString_forType(&ns(text), constant!(objc2_app_kit::NSPasteboardTypeString));
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
            let Some(heading) = commands::heading_anchors(st.doc())
                .into_iter()
                .find(|h| h.anchor == anchor)
            else {
                return;
            };
            heading.range.start
        };
        self.reveal_source(start);
    }

    /// Selects source offset `start` and scrolls both panes to it, flashing the line in the preview.
    pub(super) fn reveal_source(&self, start: usize) {
        let p = {
            let st = self.ivars().state.borrow();
            preview_offset(st.layout(), st.index(), start)
        };
        let at = NSRange::new(self.snapshot().u16_at(start).0, 0);
        self.set_source_selection(at);
        self.source().scrollRangeToVisible(at);
        if self.preview_visible() {
            let string = storage(self.preview()).string();
            let end = string.length();
            let line = (p.0..end).take_while(|&i| string.characterAtIndex(i) != 10).count();
            self.preview().scrollRangeToVisible(NSRange::new(p.0, 0));
            self.preview().showFindIndicatorForRange(NSRange::new(p.0, line.max(1)));
        }
    }
}
