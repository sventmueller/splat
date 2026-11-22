//! The mark on links of the preview that lead nowhere: to a file that is not there, a note no folder below holds, or a
//! heading the document lacks.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use objc2::sel;
use splat_editor_state::links::{Destination, destination, missing_on_disk};

use super::layers::{ForLinkTargets, Message};
use super::state::links_of;
use crate::markdown::commands;
use crate::util::{DISK, contained, lane_of};

/// Long enough that typing a link does not send a look at the disk for every letter.
const SETTLE_SECONDS: f64 = 0.3;
/// The looks of the file watcher, every two seconds, between two searches for a note that was missing: each walks the
/// folder below the document.
const LOOKS_PER_SEARCH: u32 = 5;

/// Looks again where the links of open previews point, as files and notes come and go.
pub fn look_at_link_targets() {
    for editor in super::open_editors() {
        editor.look_at_link_targets_again();
    }
}

/// What the disk answered, for the folder it was asked from.
pub(in crate::editor) struct DiskAnswer {
    folder: Option<PathBuf>,
    found: Vec<(Destination, Option<bool>)>,
}

#[derive(Default)]
pub(super) struct LinkTargetState {
    /// The links shown that lead nowhere, as `Link::attribute` writes them.
    nowhere: RefCell<Rc<HashSet<String>>>,
    /// The links the preview shows that can lead nowhere, and what each needs.
    shown: RefCell<HashMap<String, Destination>>,
    /// The anchors of `shown` the document has no place for.
    missing_anchors: RefCell<HashSet<String>>,
    /// Whether nothing lies at each destination asked for, for the folder in `folder`.
    on_disk: RefCell<HashMap<Destination, bool>>,
    folder: RefCell<Option<PathBuf>>,
    /// A look at the disk is under way; the next one waits for its answer.
    looking: Cell<bool>,
    looks: Cell<u32>,
}

pub(super) struct LinkTargets<'a> {
    host: ForLinkTargets<'a>,
    state: &'a LinkTargetState,
}

impl<'a> LinkTargets<'a> {
    pub(super) fn new(host: ForLinkTargets<'a>, state: &'a LinkTargetState) -> Self {
        LinkTargets { host, state }
    }
}

impl LinkTargets<'_> {
    /// The links shown that lead nowhere, for the renderer.
    pub(super) fn nowhere(&self) -> Rc<HashSet<String>> {
        self.state.nowhere.borrow().clone()
    }

    pub(super) fn preview_changed(&self) {
        self.host
            .object()
            .perform_later(sel!(lookAtLinkTargets), SETTLE_SECONDS, true);
    }

    /// What the disk says may have changed for all of them, as after a folder grant.
    pub(super) fn forget(&self) {
        self.state.on_disk.borrow_mut().clear();
        self.preview_changed();
    }

    /// Collects the links of the preview and asks the disk about those it has not answered for yet.
    pub(super) fn look_now(&self) {
        let doc_dir = self.host.doc_dir();
        if *self.state.folder.borrow() != doc_dir {
            self.state.on_disk.borrow_mut().clear();
            self.state.folder.replace(doc_dir.clone());
        }
        let (shown, headings) = self.host.model().read(move |st| {
            let mut shown = HashMap::new();
            for link in st.layout().blocks.iter().flat_map(|block| links_of(block)) {
                if let Some(wanted) = destination(link, doc_dir.as_deref()) {
                    shown.entry(link.attribute()).or_insert(wanted);
                }
            }
            let any_anchor = shown.values().any(|d| matches!(d, Destination::Anchor(_)));
            let headings: Option<HashSet<String>> = any_anchor
                .then(|| contained(|| commands::heading_anchors(st.doc())))
                .flatten()
                .map(|found| found.into_iter().map(|h| h.anchor).collect());
            (shown, headings)
        });
        let anchors: Vec<String> = shown
            .values()
            .filter_map(|d| match d {
                Destination::Anchor(anchor) => Some(anchor.clone()),
                _ => None,
            })
            .collect();
        let missing_anchors = match headings {
            Some(headings) => self.host.on_source_text(move |text| {
                anchors
                    .into_iter()
                    .filter(|anchor| {
                        let anchor = super::navigation::decoded_anchor(anchor);
                        !headings.contains(&anchor) && !names_an_element(text, &anchor)
                    })
                    .collect()
            }),
            None => HashSet::new(),
        };
        self.state.missing_anchors.replace(missing_anchors);
        self.state.shown.replace(shown);
        self.settle();
        self.ask_disk(false);
    }

    /// The file watcher's look: files and notes may have come or gone since.
    pub(super) fn look_again(&self) {
        let looks = self.state.looks.get().wrapping_add(1);
        self.state.looks.set(looks);
        self.ask_disk(true);
    }

    /// Asks the disk about the destinations it has not answered for, and with `again` also about those it has, except for
    /// notes it found missing, which only every few looks.
    fn ask_disk(&self, again: bool) {
        if self.state.looking.get() {
            return;
        }
        let search = self.state.looks.get().is_multiple_of(LOOKS_PER_SEARCH);
        let wanted: HashSet<Destination> = {
            let on_disk = self.state.on_disk.borrow();
            self
                .state
                .shown
                .borrow()
                .values()
                .filter(|d| !matches!(d, Destination::Anchor(_)))
                .filter(|d| match on_disk.get(*d) {
                    None => true,
                    Some(_) if !again => false,
                    Some(true) => search || !matches!(d, Destination::Note { .. }),
                    Some(false) => true,
                })
                .cloned()
                .collect()
        };
        if wanted.is_empty() {
            return;
        }
        self.state.looking.set(true);
        let folder = self.state.folder.borrow().clone();
        let lane = folder.as_deref().map_or(&DISK, lane_of);
        let messenger = self.host.object().messenger();
        lane.ask(
            move || {
                wanted
                    .into_iter()
                    .map(|d| {
                        let missing = missing_on_disk(&d);
                        (d, missing)
                    })
                    .collect()
            },
            move |found| {
                let found = found.unwrap_or_default();
                messenger.send_soon(Message::LinkTargets(DiskAnswer { folder, found }));
            },
        );
    }

    pub(super) fn disk_answered(&self, answer: DiskAnswer) {
        self.state.looking.set(false);
        if answer.folder != *self.state.folder.borrow() {
            self.ask_disk(false);
            return;
        }
        {
            let mut on_disk = self.state.on_disk.borrow_mut();
            for (d, missing) in answer.found {
                on_disk.insert(d, missing.unwrap_or(false));
            }
        }
        self.settle();
        self.ask_disk(false);
    }

    /// Takes over what is known now and renders again the blocks whose links changed their mark.
    fn settle(&self) {
        let nowhere: HashSet<String> = {
            let on_disk = self.state.on_disk.borrow();
            let missing_anchors = self.state.missing_anchors.borrow();
            self.state
                .shown
                .borrow()
                .iter()
                .filter(|(_, d)| match d {
                    Destination::Anchor(anchor) => missing_anchors.contains(anchor),
                    other => on_disk.get(*other).copied().unwrap_or(false),
                })
                .map(|(key, _)| key.clone())
                .collect()
        };
        let changed: HashSet<String> = nowhere
            .symmetric_difference(&self.state.nowhere.borrow())
            .cloned()
            .collect();
        if changed.is_empty() {
            return;
        }
        if self.host.under_way().defers_background_results() {
            self.preview_changed();
            return;
        }
        self.state.nowhere.replace(Rc::new(nowhere));
        self.host.link_targets_changed(changed);
    }
}

/// Whether HTML in `text` gives an element the `id` or `name` `anchor`, which a link may point to as well as to a heading.
fn names_an_element(text: &str, anchor: &str) -> bool {
    ["id=\"", "id='", "name=\"", "name='"]
        .iter()
        .any(|attribute| {
            let quote = &attribute[attribute.len() - 1..];
            text.contains(&format!("{attribute}{anchor}{quote}"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_anchor_of_an_html_element_counts_as_a_place() {
        let text = "<a id=\"oben\"></a>\n<span name='mitte'>x</span>\n";
        assert!(names_an_element(text, "oben"));
        assert!(names_an_element(text, "mitte"));
        assert!(!names_an_element(text, "ob"));
        assert!(!names_an_element(text, "unten"));
    }
}
