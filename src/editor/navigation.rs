//! Clicks on links, task checkboxes and footnote markers in the preview.

use std::path::Path;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSApplication, NSBeep, NSDocumentController, NSEventModifierFlags, NSTextView, NSWorkspace};
use objc2_foundation::{NSArray, NSRange, NSString, NSURL};

use splat_editor_state::navigation::{Click, Opening, click_meaning, found_place, opening};
use splat_editor_state::notes::place_in_text;

use super::State;
use super::layers::ForLinks;
use super::links::{self, LinkTarget};
use crate::i18n::{t, tf};
use crate::markdown::commands;
use crate::markdown::text::Byte;
use crate::preview::layout::Link;
use crate::preview::map::{Shown, preview_offset};
use crate::util::{DISK, constant, contained, lane_of, ns};

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
            |_: *mut objc2_app_kit::NSDocument, _: objc2::runtime::Bool, error: *mut objc2_foundation::NSError| {
                crate::util::guarded((), || {
                    // SAFETY: The completion handler's error is nil or a valid NSError for the duration of the call.
                    let Some(error) = (unsafe { Retained::retain(error) }) else {
                        return;
                    };
                    crate::diag::report("opening a linked document failed", error.code());
                    crate::util::main_soon(move || {
                        if let Some(mtm) = objc2::MainThreadMarker::new() {
                            objc2_app_kit::NSApplication::sharedApplication(mtm).presentError(&error);
                        }
                    });
                });
            },
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
    place.map_or(&DISK, lane_of).ask(
        move || found_place(find()),
        move |opened| {
            crate::util::main_soon(move || {
                if let Some(mtm) = objc2::MainThreadMarker::new() {
                    open_place(mtm, opened.flatten());
                }
            });
        },
    );
}

/// The text of a link attribute, in either form AppKit hands it over.
fn link_text(link: &AnyObject) -> Option<String> {
    if let Some(s) = link.downcast_ref::<NSString>() {
        Some(s.to_string())
    } else {
        link.downcast_ref::<NSURL>()
            .map(|u| u.absoluteString().map(|s| s.to_string()).unwrap_or_default())
    }
}

/// An anchor as the heading has it, or as it stands where it is no valid percent encoding.
fn decoded_anchor(anchor: &str) -> String {
    ns(anchor)
        .stringByRemovingPercentEncoding()
        .map_or_else(|| anchor.to_string(), |s| s.to_string())
}

pub(super) struct Links<'a> {
    host: ForLinks<'a>,
}

impl<'a> Links<'a> {
    pub(super) fn new(host: ForLinks<'a>) -> Self {
        Links { host }
    }
}

impl Links<'_> {
    /// What `f` makes of the state, if that still belongs to the text: an edit made from an older one would land somewhere
    /// else in it. The click meant what the preview showed then, so it goes unanswered, and the preview catches up.
    fn on_current_state<T>(&self, f: impl FnOnce(&State) -> Option<T> + Send) -> Option<T> {
        let answer = self
            .host
            .with_text(|snap, st| (st.is_current(snap) && !st.preview_is_behind()).then(|| f(st)));
        if answer.is_none() {
            self.host.updater().update_now();
        }
        answer.flatten()
    }

    pub(super) fn link_clicked(&self, view: &NSTextView, link: &AnyObject, index: usize) -> bool {
        let Some(target) = link_text(link) else {
            return false;
        };
        let shown = Shown::new(index);
        let link = Link::from_attribute(&target);
        let command_held = NSApplication::sharedApplication(self.host.mtm())
            .currentEvent()
            .is_some_and(|e| e.modifierFlags().contains(NSEventModifierFlags::Command));
        match click_meaning(&link, command_held) {
            Click::ToggleTask => {
                let edit = self.on_current_state(|st| contained(|| st.bridge()?.toggle_task(shown)).flatten());
                if let Some(edit) = edit {
                    self.host
                        .editing()
                        .apply_edit(edit, Some(t("Aufgabe umschalten")), false);
                }
            }
            Click::ToggleFold => {
                let edit = self.on_current_state(|st| contained(|| st.bridge()?.toggle_fold(shown)).flatten());
                if let Some(edit) = edit {
                    self.host
                        .editing()
                        .apply_edit(edit, Some(t("Hinweisbox auf- oder zuklappen")), false);
                }
            }
            Click::ShowFootnote(name) => {
                let found = self.host.model().read(|st| st.footnote_paragraph(name));
                if let Some((start, len)) = found {
                    let range = NSRange::new(start.get(), len);
                    view.scrollRangeToVisible(range);
                    view.showFindIndicatorForRange(range);
                }
            }
            Click::EditMetadata => self.host.editing().perform(super::Action::EditMetadata, None),
            Click::Open(link) => self.open_link(link),
            Click::PlaceCaret => view.setSelectedRange(NSRange::new(index, 0)),
        }
        true
    }

    fn open_link(&self, link: &Link) {
        match opening(link, self.host.doc_dir().as_deref()) {
            Opening::Find(tag) => {
                self.find_in_source(&tag);
                self.host.notes().search_tag(&tag);
            }
            Opening::Reference(reference) => {
                let url = super::env::Env::known(self.host.doc_dir(), "")
                    .repository
                    .and_then(|r| r.url(&reference))
                    .and_then(|u| NSURL::URLWithString(&ns(&u)));
                match url {
                    Some(url) => {
                        NSWorkspace::sharedWorkspace().openURL(&url);
                    }
                    None => NSBeep(),
                }
            }
            Opening::WikiPage(name) => {
                let doc_dir = self.host.doc_dir();
                let place = doc_dir.clone();
                open_found_in_background(place.as_deref(), move || {
                    links::classify_wiki(&name, doc_dir.as_deref())
                });
            }
            Opening::Heading(anchor) => self.reveal_heading(&anchor),
            Opening::Web(address) => {
                if let Some(url) = NSURL::URLWithString(&ns(&address)) {
                    NSWorkspace::sharedWorkspace().openURL(&url);
                }
            }
            Opening::AskFirst(address) => self.confirm_app_link(&address),
            Opening::Refuse => NSBeep(),
            Opening::LookAt(found) => {
                let place = match &found {
                    LinkTarget::Document(path) | LinkTarget::File(path) => Some(path.clone()),
                    _ => None,
                };
                open_found_in_background(place.as_deref(), move || found);
            }
            Opening::Nothing => {}
        }
    }

    /// Opens the find bar of the source on `text`, as a click on an Obsidian tag searches for it.
    fn find_in_source(&self, text: &str) {
        let mtm = self.host.mtm();
        let board = objc2_app_kit::NSPasteboard::pasteboardWithName(constant!(objc2_app_kit::NSPasteboardNameFind));
        board.clearContents();
        board.setString_forType(&ns(text), constant!(objc2_app_kit::NSPasteboardTypeString));
        let source = self.host.panes().for_appkit().source;
        if let Some(window) = self.host.window() {
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
        let Some(window) = self.host.window() else { return };
        let alert = objc2_app_kit::NSAlert::new(self.host.mtm());
        alert.setMessageText(&ns(t("Link in einer anderen App öffnen?")));
        alert.setInformativeText(&ns(&tf(
            "„{}“ wird mit {} geöffnet. Öffne den Link nur, wenn du der Quelle vertraust.",
            &[target, &name],
        )));
        // The safe choice is the default button, so Return does not open the link.
        alert.addButtonWithTitle(&ns(t("Abbrechen")));
        alert.addButtonWithTitle(&ns(t("Öffnen")));
        let handler = block2::RcBlock::new(move |response: objc2_app_kit::NSModalResponse| {
            crate::util::guarded((), || {
                if response == objc2_app_kit::NSAlertSecondButtonReturn {
                    NSWorkspace::sharedWorkspace().openURL(&url);
                }
            });
        });
        alert.beginSheetModalForWindow_completionHandler(&window, Some(&handler));
    }

    /// Scrolls both panes to the heading a `#anchor` link points to.
    pub(super) fn reveal_heading(&self, anchor: &str) {
        let anchor = decoded_anchor(anchor);
        let start = self.host.model().read(|st| {
            contained(|| commands::heading_anchors(st.doc()))
                .unwrap_or_default()
                .into_iter()
                .find(|h| h.anchor == anchor)
                .map(|heading| heading.range.start)
        });
        if let Some(start) = start {
            self.reveal_source(start);
        }
    }

    /// Selects place `column` of `line` and scrolls both panes to it, as near as the text still comes to it.
    pub(super) fn reveal_place(&self, line: usize, column: usize) {
        let at = self.host.on_source_text(move |text| place_in_text(text, line, column));
        self.reveal_source(at);
    }

    /// Selects source position `start` and scrolls both panes to it, flashing the line in the preview.
    pub(super) fn reveal_source(&self, start: Byte) {
        let (p, at) = self.host.with_text(move |snap, st| {
            (
                preview_offset(st.layout(), st.index(), start),
                NSRange::new(snap.u16_at(start).get(), 0),
            )
        });
        self.host.selection().set_source_selection(at);
        let panes = self.host.panes();
        let views = panes.for_appkit();
        views.source.scrollRangeToVisible(at);
        if panes.preview_visible() {
            let string = panes.preview().string();
            let end = string.length();
            let line = (p.get()..end).take_while(|&i| string.characterAtIndex(i) != 10).count();
            views.preview.scrollRangeToVisible(NSRange::new(p.get(), 0));
            views
                .preview
                .showFindIndicatorForRange(NSRange::new(p.get(), line.max(1)));
        }
    }
}

#[cfg(test)]
mod tests {
    use objc2_foundation::NSNumber;

    use super::*;

    #[test]
    fn a_link_comes_as_text_or_as_url_and_nothing_else_counts() {
        assert_eq!(link_text(&ns("splat-task")), Some("splat-task".to_string()));
        let url = NSURL::URLWithString(&ns("https://a.de/b")).unwrap();
        assert_eq!(link_text(&url), Some("https://a.de/b".to_string()));
        assert_eq!(link_text(&NSNumber::new_i32(1)), None);
    }

    #[test]
    fn an_anchor_is_decoded_unless_it_does_not_decode() {
        assert_eq!(decoded_anchor("%C3%BCber-uns"), "über-uns");
        assert_eq!(decoded_anchor("plain"), "plain");
        assert_eq!(decoded_anchor("100%-sicher"), "100%-sicher");
    }
}
