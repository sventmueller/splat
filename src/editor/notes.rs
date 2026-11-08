//! The notes around a document: the folder it shows, the search through it, opening a note in a tab of this window and
//! renaming one with the links to it. Walking and searching happen on threads of their own, as a volume can hang and
//! the disk thread has others waiting.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::MainThreadOnly;
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSBeep, NSDocumentController, NSOpenPanel, NSWorkspace};
use objc2_foundation::{NSArray, NSURL};
use splat_notes::{FileHits, Query, Rewrite, Tree};

use splat_editor_state::notes::{query_of, renamed_path, rows_of_hits, rows_of_tree, search_status, write_rewrites};

use super::layers::{ForNotes, Message, Messenger};
use super::sidebar::{NoteEvent, SidebarTab};
use crate::document::MdDocument;
use crate::i18n::{t, tf};
use crate::util::{guarded, lane_of, ns};

/// More hits than anyone reads, and a query of one letter in a large folder would otherwise fill the memory.
const MOST_HITS: usize = 5_000;
/// How often hits that keep coming show.
const SHOW_EVERY: Duration = Duration::from_millis(120);
/// Threads looking at a volume that stopped answering wait with it, and more of them help nobody.
const MOST_THREADS: usize = 8;
static THREADS: AtomicUsize = AtomicUsize::new(0);

/// What came back from another thread.
pub(in crate::editor) enum NoteAnswer {
    Scanned {
        generation: u64,
        tree: Tree,
    },
    Found {
        generation: u64,
        hits: Vec<FileHits>,
        done: bool,
    },
    RenameFound {
        request: RenameRequest,
        rewrites: Vec<Rewrite>,
    },
    Renamed {
        failed: Vec<PathBuf>,
    },
}

/// A rename the user asked for, on its way.
pub(in crate::editor) struct RenameRequest {
    from: PathBuf,
    to: PathBuf,
    /// The texts the changes are computed from, by file.
    texts: HashMap<PathBuf, String>,
}

/// What the search looks for.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Wanted {
    Nothing,
    Typed(String),
    LinksTo(PathBuf),
}

impl Wanted {
    fn query(&self) -> Option<Query> {
        match self {
            Wanted::Nothing => None,
            Wanted::Typed(text) => query_of(text),
            Wanted::LinksTo(path) => Some(Query::LinksTo(path.clone())),
        }
    }
}

#[derive(Default)]
pub(super) struct NotesState {
    /// The folder the user chose for this window, instead of the one around its document.
    chosen: RefCell<Option<PathBuf>>,
    tree: RefCell<Option<Arc<Tree>>>,
    /// What the last walk and the last search are told when they are out of date.
    stop_scan: RefCell<Option<Arc<AtomicBool>>>,
    stop_search: RefCell<Option<Arc<AtomicBool>>>,
    scans: Cell<u64>,
    searches: Cell<u64>,
    wanted: RefCell<Option<Wanted>>,
    hits: RefCell<Vec<FileHits>>,
    searching: Cell<bool>,
}

pub(super) struct Notes<'a> {
    host: ForNotes<'a>,
    state: &'a NotesState,
}

impl<'a> Notes<'a> {
    pub(super) fn new(host: ForNotes<'a>, state: &'a NotesState) -> Self {
        Notes { host, state }
    }
}

/// Starts `work` on a thread of its own and sends what it returns to the window, unless too many threads wait already.
fn on_own_thread(messenger: Messenger, work: impl FnOnce() -> Option<NoteAnswer> + Send + 'static) -> bool {
    if THREADS.fetch_add(1, Ordering::SeqCst) >= MOST_THREADS {
        THREADS.fetch_sub(1, Ordering::SeqCst);
        return false;
    }
    std::thread::spawn(move || {
        let answer = crate::util::contained(work).flatten();
        THREADS.fetch_sub(1, Ordering::SeqCst);
        if let Some(answer) = answer {
            messenger.send_soon(Message::Notes(answer));
        }
    });
    true
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

impl Notes<'_> {
    fn document_path(&self) -> Option<PathBuf> {
        let path = self.host.document()?.fileURL()?.path()?.to_string();
        Some(PathBuf::from(path))
    }

    pub(super) fn event(&self, event: NoteEvent) {
        match event {
            NoteEvent::TabChosen(tab) => self.tab_chosen(tab),
            NoteEvent::ChooseFolder => self.choose_folder(),
            NoteEvent::Search(text) => {
                let wanted = if text.trim().is_empty() {
                    Wanted::Nothing
                } else {
                    Wanted::Typed(text)
                };
                self.search(wanted);
            }
            NoteEvent::Open(path) => self.open(path, None),
            NoteEvent::OpenHit(path, line, column) => self.open(path, Some((line, column))),
            NoteEvent::Rename(path) => self.ask_for_name(path),
            NoteEvent::Reveal(path) => {
                if let Some(path) = path.to_str() {
                    let urls = NSArray::from_retained_slice(&[NSURL::fileURLWithPath(&ns(path))]);
                    NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&urls);
                }
            }
            NoteEvent::LinksTo(path) => {
                self.host.panes().show_tab(SidebarTab::Search);
                self.search(Wanted::LinksTo(path));
            }
        }
    }

    fn tab_chosen(&self, tab: SidebarTab) {
        self.host.panes().show_tab(tab);
        self.rescan();
    }

    /// Looks at the folder for the tab just chosen, while the sidebar may still be on its way into view.
    pub(super) fn rescan(&self) {
        if self.host.panes().sidebar_tab() != Some(SidebarTab::Outline) {
            self.scan();
        }
    }

    /// The sidebar opened, the window came to the front or a file changed: looks at the folder again if it shows.
    pub(super) fn refresh(&self) {
        let panes = self.host.panes();
        if !panes.sidebar_visible() || panes.sidebar_tab() == Some(SidebarTab::Outline) {
            return;
        }
        self.scan();
    }

    fn scan(&self) {
        let file = self.document_path();
        let chosen = self.state.chosen.borrow().clone();
        if file.is_none() && chosen.is_none() {
            self.host.panes().with_sidebar(|sidebar| {
                sidebar.show_folder(t("Kein Ordner"), "");
                sidebar.files.show(Vec::new(), Vec::new(), false);
            });
            return;
        }
        let generation = self.state.scans.get() + 1;
        let stop = Arc::new(AtomicBool::new(false));
        let previous = self.state.stop_scan.borrow_mut().replace(stop.clone());
        if let Some(previous) = previous {
            previous.store(true, Ordering::Relaxed);
        }
        let started = on_own_thread(self.host.object().messenger(), move || {
            let root = match (chosen, file) {
                (Some(chosen), _) => chosen,
                (None, Some(file)) => splat_notes::root_for(&file, home().as_deref()),
                (None, None) => return None,
            };
            Some(NoteAnswer::Scanned {
                generation,
                tree: scan_root(&root, &stop),
            })
        });
        if started {
            self.state.scans.set(generation);
        }
    }

    pub(super) fn answer(&self, answer: NoteAnswer) {
        match answer {
            NoteAnswer::Scanned { generation, tree } if generation == self.state.scans.get() => self.scanned(tree),
            NoteAnswer::Found { generation, hits, done } if generation == self.state.searches.get() => {
                self.found(hits, done)
            }
            NoteAnswer::RenameFound { request, rewrites } => self.confirm_rename(request, rewrites),
            NoteAnswer::Renamed { failed } => {
                if !failed.is_empty() {
                    self.report_rename_failure(&failed);
                }
                self.refresh_everywhere();
            }
            NoteAnswer::Scanned { .. } | NoteAnswer::Found { .. } => {}
        }
    }

    fn scanned(&self, tree: Tree) {
        if tree.unreadable() > 0 {
            crate::diag::report("folders or notes the sidebar could not read", tree.unreadable());
        }
        let (rows, roots) = rows_of_tree(&tree);
        let name = tree.root().file_name().map_or_else(
            || tree.root().display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let shown = if tree.is_cut() {
            tf("{} (gekürzt)", &[&name])
        } else {
            name
        };
        let tip = tree.root().display().to_string();
        let document = self.document_path();
        self.host.panes().with_sidebar(|sidebar| {
            sidebar.show_folder(&shown, &tip);
            sidebar.files.show(rows, roots, false);
            if let Some(document) = &document {
                sidebar.files.select_note(document);
            }
        });
        *self.state.tree.borrow_mut() = Some(Arc::new(tree));
        let waiting = self.state.wanted.borrow().clone();
        if self.state.searching.get()
            && let Some(wanted) = waiting
        {
            self.search(wanted);
        }
    }

    fn choose_folder(&self) {
        let Some(window) = self.host.window() else { return };
        let panel = NSOpenPanel::openPanel(self.host.mtm());
        panel.setCanChooseFiles(false);
        panel.setCanChooseDirectories(true);
        panel.setAllowsMultipleSelection(false);
        panel.setPrompt(Some(&ns(t("Wählen"))));
        let messenger = self.host.object().messenger();
        let chosen = panel.clone();
        let handler = RcBlock::new(move |response: objc2_app_kit::NSModalResponse| {
            guarded((), || {
                if response != objc2_app_kit::NSModalResponseOK {
                    return;
                }
                let path = chosen
                    .URL()
                    .and_then(|u| u.path())
                    .map(|p| PathBuf::from(p.to_string()));
                if let Some(path) = path {
                    messenger.send(chosen.mtm(), Message::NotesFolder(path));
                }
            });
        });
        panel.beginSheetModalForWindow_completionHandler(&window, &handler);
    }

    /// Shows `folder` in this window from now on.
    pub(super) fn use_folder(&self, folder: PathBuf) {
        *self.state.chosen.borrow_mut() = Some(folder);
        *self.state.tree.borrow_mut() = None;
        self.scan();
    }

    /// Shows the sidebar's search, with the field ready for typing.
    pub(super) fn show_search(&self) {
        let panes = self.host.panes();
        panes.show_tab(SidebarTab::Search);
        panes.with_sidebar(|sidebar| {
            if let Some(window) = self.host.window() {
                window.makeFirstResponder(Some(&*sidebar.field));
            }
        });
        self.rescan();
    }

    /// Searches the folder for a tag, as a click on the tag asks for.
    pub(super) fn search_tag(&self, tag: &str) {
        if self.document_path().is_none() && self.state.chosen.borrow().is_none() {
            return;
        }
        self.host.panes().show_tab(SidebarTab::Search);
        self.host
            .panes()
            .with_sidebar(|sidebar| sidebar.field.setStringValue(&ns(tag)));
        self.search(Wanted::Typed(tag.to_string()));
    }

    fn search(&self, wanted: Wanted) {
        let previous = self.state.stop_search.borrow_mut().take();
        if let Some(previous) = previous {
            previous.store(true, Ordering::Relaxed);
        }
        let generation = self.state.searches.get() + 1;
        self.state.searches.set(generation);
        self.state.hits.borrow_mut().clear();
        *self.state.wanted.borrow_mut() = Some(wanted.clone());
        let Some(query) = wanted.query() else {
            self.state.searching.set(false);
            self.host.panes().with_sidebar(|sidebar| {
                sidebar.hits.show(Vec::new(), Vec::new(), false);
                sidebar.show_status("");
            });
            return;
        };
        self.state.searching.set(true);
        self.host
            .panes()
            .with_sidebar(|sidebar| sidebar.show_status(&search_status(&[], true)));
        let Some(tree) = self.state.tree.borrow().clone() else {
            // The walk that ends in `scanned` starts this search again.
            self.scan();
            return;
        };
        let stop = Arc::new(AtomicBool::new(false));
        *self.state.stop_search.borrow_mut() = Some(stop.clone());
        let messenger = self.host.object().messenger();
        let reports = messenger.clone();
        let started = on_own_thread(messenger, move || {
            let mut batch = Vec::new();
            let mut last_shown = Instant::now();
            let mut total = 0;
            let searched = splat_notes::search(&tree, &query, &stop, &mut |hits| {
                total += hits.hits.len();
                batch.push(hits);
                if total >= MOST_HITS {
                    stop.store(true, Ordering::Relaxed);
                }
                if last_shown.elapsed() >= SHOW_EVERY {
                    last_shown = Instant::now();
                    reports.send_soon(Message::Notes(NoteAnswer::Found {
                        generation,
                        hits: std::mem::take(&mut batch),
                        done: false,
                    }));
                }
            });
            if searched.unreadable > 0 {
                crate::diag::report("notes the search could not read", searched.unreadable);
            }
            Some(NoteAnswer::Found {
                generation,
                hits: batch,
                done: true,
            })
        });
        if !started {
            self.state.searching.set(false);
            NSBeep();
        }
    }

    fn found(&self, hits: Vec<FileHits>, done: bool) {
        self.state.hits.borrow_mut().extend(hits);
        if done {
            self.state.searching.set(false);
        }
        let root = self.state.tree.borrow().as_ref().map(|tree| tree.root().to_path_buf());
        // AppKit runs callbacks while the list reloads, and whatever they set off must find the hits free to change.
        let ((rows, roots), status) = {
            let all = self.state.hits.borrow();
            (rows_of_hits(&all, root.as_deref()), search_status(&all, !done))
        };
        self.host.panes().with_sidebar(|sidebar| {
            sidebar.hits.show(rows, roots, true);
            sidebar.show_status(&status);
        });
    }

    /// Opens a note as a tab of this window, or brings it forward if it is open, and shows a line and place in it.
    fn open(&self, path: PathBuf, at: Option<(usize, usize)>) {
        if self.document_path().as_deref() == Some(&*path) {
            if let Some((line, column)) = at {
                self.host.object().send(Message::RevealPlace { line, column });
            }
            return;
        }
        let Some(address) = path.to_str().map(|p| NSURL::fileURLWithPath(&ns(p))) else {
            return;
        };
        let (mtm, window) = (self.host.mtm(), self.host.window());
        let tab = self.host.panes().sidebar_tab().unwrap_or(SidebarTab::Files);
        let handed = (
            self.state.chosen.borrow().clone(),
            self.state.tree.borrow().clone(),
            self.state.wanted.borrow().clone(),
            self.state.hits.borrow().clone(),
        );
        let handed = std::cell::RefCell::new(Some(handed));
        let done = crate::util::open_handler(move |document, already, error| {
            let Some(document) = document else {
                if let Some(error) = error {
                    crate::diag::report("opening a note failed", error.code());
                    objc2_app_kit::NSApplication::sharedApplication(mtm).presentError(&error);
                }
                return;
            };
            if !already {
                document.makeWindowControllers();
                let tab_window = document.windowControllers().firstObject().and_then(|c| c.window());
                if let (Some(window), Some(tab_window)) = (&window, tab_window) {
                    window.addTabbedWindow_ordered(&tab_window, objc2_app_kit::NSWindowOrderingMode::Above);
                }
            }
            document.showWindows();
            let Some(editor) = document.downcast::<MdDocument>().ok().and_then(|d| d.editor()) else {
                return;
            };
            if let Some((chosen, tree, wanted, hits)) = handed.borrow_mut().take() {
                editor.inherit_notes(Inherited {
                    tab,
                    chosen,
                    tree,
                    wanted,
                    hits,
                    keep_shown: already,
                });
            }
            if let Some((line, column)) = at {
                editor.handle(Message::RevealPlace { line, column });
            }
        });
        NSDocumentController::sharedDocumentController(self.host.mtm())
            .openDocumentWithContentsOfURL_display_completionHandler(&address, false, &done);
    }

    /// Takes over what the window that opened this one showed.
    pub(super) fn inherit(&self, inherited: Inherited) {
        let Inherited {
            tab,
            chosen,
            tree,
            wanted,
            hits,
            keep_shown,
        } = inherited;
        if keep_shown {
            return;
        }
        *self.state.chosen.borrow_mut() = chosen;
        *self.state.tree.borrow_mut() = tree;
        *self.state.wanted.borrow_mut() = wanted.clone();
        *self.state.hits.borrow_mut() = hits;
        self.host.panes().show_tab(tab);
        self.host.panes().with_sidebar(|sidebar| {
            if let Some(Wanted::Typed(text)) = &wanted {
                sidebar.field.setStringValue(&ns(text));
            }
        });
        self.found(Vec::new(), true);
        self.rescan();
    }

    fn ask_for_name(&self, path: PathBuf) {
        let Some(window) = self.host.window() else { return };
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (messenger, mtm) = (self.host.object().messenger(), self.host.mtm());
        super::dialogs::rename(&window, &stem, move |typed| {
            let Some(to) = renamed_path(&path, &typed) else {
                NSBeep();
                return;
            };
            messenger.send(mtm, Message::RenameChosen { from: path.clone(), to });
        });
    }

    /// Finds what has to change, from the text of open documents where it differs from the file.
    pub(super) fn rename_chosen(&self, from: PathBuf, to: PathBuf) {
        let Some(tree) = self.state.tree.borrow().clone() else {
            NSBeep();
            return;
        };
        let open: HashMap<PathBuf, String> = super::open_documents(self.host.mtm())
            .into_iter()
            .filter_map(|document| {
                let path = document.fileURL()?.path()?.to_string();
                Some((PathBuf::from(path), document.editor()?.source_text()))
            })
            .collect();
        let taken = to.exists() && !same_file(&from, &to);
        if taken {
            let alert = NSAlert::new(self.host.mtm());
            alert.setMessageText(&ns(t("Der Name ist bereits vergeben.")));
            if let Some(window) = self.host.window() {
                alert.beginSheetModalForWindow_completionHandler(&window, None);
            }
            return;
        }
        let started = on_own_thread(self.host.object().messenger(), move || {
            let stop = AtomicBool::new(false);
            let mut texts = HashMap::new();
            let rewrites = splat_notes::renames(&tree, &from, &to, &stop, &mut |path: &Path| {
                let text = open
                    .get(path)
                    .cloned()
                    .or_else(|| String::from_utf8(std::fs::read(path).ok()?).ok())?;
                texts.insert(path.to_path_buf(), text.clone());
                Some(text)
            });
            Some(NoteAnswer::RenameFound {
                request: RenameRequest { from, to, texts },
                rewrites,
            })
        });
        if !started {
            NSBeep();
        }
    }

    /// With links to change, this reaches other files, so the user sees how many first.
    fn confirm_rename(&self, request: RenameRequest, rewrites: Vec<Rewrite>) {
        let links: usize = rewrites.iter().map(|r| r.replacements.len()).sum();
        if links == 0 {
            self.rename(request, rewrites);
            return;
        }
        let Some(window) = self.host.window() else { return };
        let alert = NSAlert::new(self.host.mtm());
        alert.setMessageText(&ns(t("Verweise anpassen?")));
        alert.setInformativeText(&ns(&tf(
            "Verweise: {} · Dateien: {}. Diese Änderung lässt sich in nicht geöffneten Dateien nicht widerrufen.",
            &[&links.to_string(), &rewrites.len().to_string()],
        )));
        alert.addButtonWithTitle(&ns(t("Umbenennen")));
        alert
            .addButtonWithTitle(&ns(t("Abbrechen")))
            .setKeyEquivalent(&ns("\u{1b}"));
        let (messenger, mtm) = (self.host.object().messenger(), self.host.mtm());
        let waiting = RefCell::new(Some((request, rewrites)));
        let handler = RcBlock::new(move |response: objc2_app_kit::NSModalResponse| {
            guarded((), || {
                if response == NSAlertFirstButtonReturn
                    && let Some((request, rewrites)) = waiting.borrow_mut().take()
                {
                    messenger.send(mtm, Message::RenameConfirmed(request, rewrites));
                }
            });
        });
        alert.beginSheetModalForWindow_completionHandler(&window, Some(&handler));
    }

    /// The note moves first: if that fails, nothing else has changed. Open documents take their changes as an undo step
    /// and follow the move themselves; closed files are written together on the volume's thread.
    pub(super) fn rename(&self, request: RenameRequest, rewrites: Vec<Rewrite>) {
        let RenameRequest { from, to, texts } = request;
        let documents = super::open_documents(self.host.mtm());
        let open_at = |path: &Path| {
            documents
                .iter()
                .find(|d| {
                    d.fileURL()
                        .and_then(|u| u.path())
                        .is_some_and(|p| p.to_string() == path.to_string_lossy())
                })
                .cloned()
        };
        let moved_document = open_at(&from);
        let mut writes = Vec::new();
        let mut failed = Vec::new();
        for Rewrite { path, replacements } in &rewrites {
            let Some(before) = texts.get(path) else {
                failed.push(path.clone());
                continue;
            };
            if let Some(editor) = open_at(path).and_then(|d| d.editor()) {
                if !editor.replace_in_text(before, replacements) {
                    failed.push(path.clone());
                }
            } else {
                let after = splat_notes::applied(before, replacements);
                let target = if *path == from { to.clone() } else { path.clone() };
                writes.push((target, before.clone(), after));
            }
        }
        let lane = lane_of(&from);
        let messenger = self.host.object().messenger();
        let report = move |failed: Vec<PathBuf>| messenger.send_soon(Message::Notes(NoteAnswer::Renamed { failed }));
        if let Some(document) = moved_document {
            let Some(address) = to.to_str().map(|p| NSURL::fileURLWithPath(&ns(p))) else {
                return;
            };
            let writes = std::cell::RefCell::new(Some(writes));
            let report = std::cell::RefCell::new(Some(report));
            let already_failed = failed.clone();
            let from_for_report = from.clone();
            let moved = crate::util::error_handler(move |error| {
                let (Some(writes), Some(report)) = (writes.borrow_mut().take(), report.borrow_mut().take()) else {
                    return;
                };
                let mut failed = already_failed.clone();
                if let Some(error) = error {
                    crate::diag::report("moving a note failed", error.code());
                    failed.push(from_for_report.clone());
                    report(failed);
                    return;
                }
                lane.run(move || {
                    failed.extend(write_rewrites(&writes));
                    report(failed);
                });
            });
            document.moveToURL_completionHandler(&address, Some(&moved));
        } else {
            lane.run(move || {
                if let Err(error) = std::fs::rename(&from, &to) {
                    crate::diag::report("renaming a note failed", error.raw_os_error().unwrap_or(0));
                    failed.push(from);
                    report(failed);
                    return;
                }
                failed.extend(write_rewrites(&writes));
                report(failed);
            });
        }
    }

    fn report_rename_failure(&self, failed: &[PathBuf]) {
        let Some(window) = self.host.window() else { return };
        let names: Vec<String> = failed
            .iter()
            .take(12)
            .filter_map(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect();
        let alert = NSAlert::new(self.host.mtm());
        alert.setMessageText(&ns(t("Nicht alles ließ sich umbenennen")));
        alert.setInformativeText(&ns(&tf("Diese Dateien blieben unverändert: {}", &[&names.join(", ")])));
        alert.beginSheetModalForWindow_completionHandler(&window, None);
    }

    fn refresh_everywhere(&self) {
        for document in super::open_documents(self.host.mtm()) {
            if let Some(editor) = document.editor() {
                editor.notes_may_have_changed();
            }
        }
    }
}

/// A folder the size of a home folder asks for every permission macOS has, so of that one only its own notes show.
fn scan_root(root: &Path, stop: &AtomicBool) -> Tree {
    if home().as_deref() == Some(root) {
        splat_notes::scan_flat(root, stop)
    } else {
        splat_notes::scan(root, stop)
    }
}

/// Whether two paths are one file, as a rename that only changes case on a case-insensitive volume has them.
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.ino() == b.ino() && a.dev() == b.dev(),
        _ => false,
    }
}

/// What a window hands to the one it opens from its sidebar.
pub(in crate::editor) struct Inherited {
    tab: SidebarTab,
    chosen: Option<PathBuf>,
    tree: Option<Arc<Tree>>,
    wanted: Option<Wanted>,
    hits: Vec<FileHits>,
    /// The window was open already and stays as it was.
    keep_shown: bool,
}

#[cfg(feature = "selftest")]
impl Notes<'_> {
    pub(super) fn hit_paths(&self) -> Vec<PathBuf> {
        self.state.hits.borrow().iter().map(|f| f.path.clone()).collect()
    }

    pub(super) fn tree_paths(&self) -> Vec<PathBuf> {
        let tree = self.state.tree.borrow();
        tree.iter()
            .flat_map(|t| t.notes().map(|n| n.path().to_path_buf()))
            .collect()
    }

    pub(super) fn is_searching(&self) -> bool {
        self.state.searching.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_counts_as_the_same_file_only_with_itself() {
        let dir = std::env::temp_dir();
        assert!(same_file(&dir, &dir));
        assert!(!same_file(&dir, Path::new("/gibt-es-nicht")));
    }
}
