//! The notes around a document: the folder it shows, the search through it, opening a note in a tab of this window and
//! renaming one with the links to it. Walking and searching happen on threads of their own, as a volume can hang and
//! the disk thread has others waiting.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::rc::{Retained, Weak};
use objc2::{MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSBeep, NSDocument, NSDocumentController, NSOpenPanel, NSWorkspace,
};
use objc2_foundation::{NSArray, NSError, NSURL};
use splat_notes::{FileHits, MOST_HITS_PER_FILE, Query, Replacement, Rewrite, Tree};

use super::layers::ForNotes;
use super::sidebar::{NoteEvent, Row, RowKind, SidebarTab};
use crate::document::MdDocument;
use crate::i18n::{t, tf};
use crate::util::{guarded, lane_of, main_soon, ns};

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

/// `#tag` searches for that tag, anything else for the text as typed.
fn query_of(text: &str) -> Option<Query> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let is_tag = text.len() > 1 && text.starts_with('#') && !text.contains(char::is_whitespace);
    Some(if is_tag {
        Query::Tag(text.to_string())
    } else {
        Query::Text {
            needle: text.to_string(),
            case_sensitive: false,
        }
    })
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
fn on_own_thread(
    controller: &super::EditorController,
    work: impl FnOnce() -> Option<NoteAnswer> + Send + 'static,
) -> bool {
    if THREADS.fetch_add(1, Ordering::SeqCst) >= MOST_THREADS {
        THREADS.fetch_sub(1, Ordering::SeqCst);
        return false;
    }
    let me = dispatch2::MainThreadBound::new(Weak::from(controller), controller.mtm());
    std::thread::spawn(move || {
        let answer = crate::util::contained(work).flatten();
        THREADS.fetch_sub(1, Ordering::SeqCst);
        main_soon(move || {
            let Some(mtm) = MainThreadMarker::new() else { return };
            if let (Some(answer), Some(c)) = (answer, me.get(mtm).load()) {
                c.note_answer(answer);
            }
        });
    });
    true
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// The rows of a folder's tree.
fn rows_of_tree(tree: &Tree) -> (Vec<Row>, Vec<usize>) {
    let rows = tree
        .nodes
        .iter()
        .map(|node| Row {
            title: node.name.clone(),
            matched: None,
            tip: node.path.display().to_string(),
            symbol: if node.folder { "folder" } else { "doc.text" },
            kind: if node.folder {
                RowKind::Folder
            } else {
                RowKind::Note(node.path.clone())
            },
            key: node.path.display().to_string(),
            children: node.children.clone(),
        })
        .collect();
    (rows, tree.top.clone())
}

/// The rows of search results: a row per file, relative to the folder, with its hits below.
fn rows_of_hits(hits: &[FileHits], root: Option<&Path>) -> (Vec<Row>, Vec<usize>) {
    let mut rows = Vec::new();
    let mut roots = Vec::new();
    for file in hits {
        let shown = root
            .and_then(|root| file.path.strip_prefix(root).ok())
            .unwrap_or(&file.path)
            .display()
            .to_string();
        let top = rows.len();
        roots.push(top);
        rows.push(Row {
            title: shown,
            matched: None,
            tip: file.path.display().to_string(),
            symbol: "doc.text",
            kind: RowKind::Note(file.path.clone()),
            key: file.path.display().to_string(),
            children: Vec::new(),
        });
        for hit in &file.hits {
            let child = rows.len();
            rows[top].children.push(child);
            let (title, matched) = near_the_match(&hit.context, hit.in_context.clone());
            rows.push(Row {
                title,
                matched: Some(matched),
                tip: format!("{}\n{}", tf("Zeile {}", &[&(hit.line + 1).to_string()]), hit.context),
                symbol: "",
                kind: RowKind::Hit(file.path.clone(), hit.line, hit.column),
                key: format!("{}#{}", file.path.display(), hit.range.start),
                children: Vec::new(),
            });
        }
        if file.more {
            let child = rows.len();
            rows[top].children.push(child);
            rows.push(Row {
                title: tf("Mehr als {} Treffer", &[&MOST_HITS_PER_FILE.to_string()]),
                matched: None,
                tip: String::new(),
                symbol: "",
                kind: RowKind::Remark,
                key: format!("{}#more", file.path.display()),
                children: Vec::new(),
            });
        }
    }
    (rows, roots)
}

/// A line of a hit as a narrow sidebar has room for it: it starts shortly before the match, which would otherwise lie
/// beyond the edge of the row.
fn near_the_match(context: &str, matched: Range<usize>) -> (String, Range<usize>) {
    const LEAD_CHARS: usize = 12;
    let lead = context.get(..matched.start).unwrap_or("");
    let Some((cut, _)) = lead.char_indices().rev().nth(LEAD_CHARS) else {
        return (context.to_string(), matched);
    };
    let dots = "…";
    let moved = cut.min(matched.start);
    let start = matched.start - moved + dots.len();
    (format!("{dots}{}", &context[moved..]), start..start + matched.len())
}

/// What the status line says about a search.
fn search_status(hits: &[FileHits], searching: bool) -> String {
    let count: usize = hits.iter().map(|f| f.hits.len()).sum();
    match (count, searching) {
        (0, true) => t("Suche läuft …").to_string(),
        (0, false) => t("Keine Treffer").to_string(),
        _ => tf(
            "Treffer: {} · Dateien: {}",
            &[&count.to_string(), &hits.len().to_string()],
        ),
    }
}

/// A new name for a note: its stem as typed, with the note's extension unless one is typed. `None` if it cannot be a name.
fn renamed_path(path: &Path, typed: &str) -> Option<PathBuf> {
    let typed = typed.trim();
    if typed.is_empty() || typed.contains('/') || typed.contains(':') || typed.starts_with('.') {
        return None;
    }
    let name = if splat_notes::is_note(Path::new(typed)) {
        typed.to_string()
    } else {
        let extension = path.extension()?.to_string_lossy();
        format!("{typed}.{extension}")
    };
    let to = path.with_file_name(name);
    (to != path).then_some(to)
}

/// Writes every text first and moves them all into place after, so that a full disk changes none of them.
fn write_rewrites(writes: &[(PathBuf, String, String)]) -> Vec<PathBuf> {
    let mut failed = Vec::new();
    let mut ready = Vec::new();
    for (path, before, after) in writes {
        let Some(name) = path.file_name() else { continue };
        // What another program changed in the meantime stays as it is.
        let current = std::fs::read(path).ok();
        if current.as_deref() != Some(before.as_bytes()) {
            failed.push(path.clone());
            continue;
        }
        let spare = path.with_file_name(format!(".{}.splat-rename", name.to_string_lossy()));
        let kept = std::fs::metadata(path).ok().map(|m| m.permissions());
        let written = std::fs::write(&spare, after).is_ok()
            && kept.is_none_or(|permissions| std::fs::set_permissions(&spare, permissions).is_ok());
        if written {
            ready.push((spare, path.clone()));
        } else {
            let _ = std::fs::remove_file(&spare);
            failed.push(path.clone());
        }
    }
    if !failed.is_empty() {
        for (spare, _) in &ready {
            let _ = std::fs::remove_file(spare);
        }
        return writes.iter().map(|(path, ..)| path.clone()).collect();
    }
    for (spare, path) in ready {
        if std::fs::rename(&spare, &path).is_err() {
            let _ = std::fs::remove_file(&spare);
            failed.push(path);
        }
    }
    failed
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
        let started = on_own_thread(self.host.object(), move || {
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
        if tree.unreadable > 0 {
            crate::diag::report("folders or notes the sidebar could not read", tree.unreadable);
        }
        let (rows, roots) = rows_of_tree(&tree);
        let name = tree
            .root
            .file_name()
            .map_or_else(|| tree.root.display().to_string(), |n| n.to_string_lossy().into_owned());
        let shown = if tree.cut { tf("{} (gekürzt)", &[&name]) } else { name };
        let tip = tree.root.display().to_string();
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
        let me = dispatch2::MainThreadBound::new(Weak::from(self.host.object()), self.host.mtm());
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
                if let (Some(path), Some(c)) = (path, me.get(chosen.mtm()).load()) {
                    c.use_notes_folder(path);
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
        let me = dispatch2::MainThreadBound::new(Weak::from(self.host.object()), self.host.mtm());
        let me = Arc::new(me);
        let reports = me.clone();
        let started = on_own_thread(self.host.object(), move || {
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
                    let hits = std::mem::take(&mut batch);
                    let reports = reports.clone();
                    main_soon(move || {
                        let Some(mtm) = MainThreadMarker::new() else { return };
                        if let Some(c) = reports.get(mtm).load() {
                            c.note_answer(NoteAnswer::Found {
                                generation,
                                hits,
                                done: false,
                            });
                        }
                    });
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
        drop(me);
    }

    fn found(&self, hits: Vec<FileHits>, done: bool) {
        self.state.hits.borrow_mut().extend(hits);
        if done {
            self.state.searching.set(false);
        }
        let all = self.state.hits.borrow();
        let root = self.state.tree.borrow().as_ref().map(|tree| tree.root.clone());
        let (rows, roots) = rows_of_hits(&all, root.as_deref());
        let status = search_status(&all, !done);
        self.host.panes().with_sidebar(|sidebar| {
            sidebar.hits.show(rows, roots, true);
            sidebar.show_status(&status);
        });
    }

    /// Opens a note as a tab of this window, or brings it forward if it is open, and shows a line and place in it.
    fn open(&self, path: PathBuf, at: Option<(usize, usize)>) {
        if self.document_path().as_deref() == Some(&*path) {
            if let Some(at) = at {
                reveal_place(self.host.object(), at);
            }
            return;
        }
        let Some(address) = path.to_str().map(|p| NSURL::fileURLWithPath(&ns(p))) else {
            return;
        };
        let this = self.host.object().retain();
        let tab = self.host.panes().sidebar_tab().unwrap_or(SidebarTab::Files);
        let handed = (
            self.state.chosen.borrow().clone(),
            self.state.tree.borrow().clone(),
            self.state.wanted.borrow().clone(),
            self.state.hits.borrow().clone(),
        );
        let handed = std::cell::RefCell::new(Some(handed));
        let done = RcBlock::new(
            move |document: *mut NSDocument, already: objc2::runtime::Bool, error: *mut NSError| {
                guarded((), || {
                    // SAFETY: The completion handler's document is nil or a valid NSDocument for the duration of the call.
                    let Some(document) = (unsafe { Retained::retain(document) }) else {
                        // SAFETY: The completion handler's error is nil or a valid NSError for the duration of the call.
                        if let Some(error) = unsafe { Retained::retain(error) } {
                            crate::diag::report("opening a note failed", error.code());
                            objc2_app_kit::NSApplication::sharedApplication(this.mtm()).presentError(&error);
                        }
                        return;
                    };
                    if !already.as_bool() {
                        document.makeWindowControllers();
                        let tab_window = document.windowControllers().firstObject().and_then(|c| c.window());
                        if let (Some(window), Some(tab_window)) = (this.window(), tab_window) {
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
                            keep_shown: already.as_bool(),
                        });
                    }
                    if let Some(at) = at {
                        reveal_place(&editor, at);
                    }
                });
            },
        );
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
        let this = self.host.object().retain();
        super::dialogs::rename(&window, &stem, move |typed| {
            let Some(to) = renamed_path(&path, &typed) else {
                NSBeep();
                return;
            };
            this.note_rename_chosen(path.clone(), to);
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
        let started = on_own_thread(self.host.object(), move || {
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
        let this = self.host.object().retain();
        let waiting = RefCell::new(Some((request, rewrites)));
        let handler = RcBlock::new(move |response: objc2_app_kit::NSModalResponse| {
            guarded((), || {
                if response == NSAlertFirstButtonReturn
                    && let Some((request, rewrites)) = waiting.borrow_mut().take()
                {
                    this.note_rename_confirmed(request, rewrites);
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
        let me = dispatch2::MainThreadBound::new(Weak::from(self.host.object()), self.host.mtm());
        let report = move |failed: Vec<PathBuf>| {
            main_soon(move || {
                let Some(mtm) = MainThreadMarker::new() else { return };
                if let Some(c) = me.get(mtm).load() {
                    c.note_answer(NoteAnswer::Renamed { failed });
                }
            });
        };
        if let Some(document) = moved_document {
            let Some(address) = to.to_str().map(|p| NSURL::fileURLWithPath(&ns(p))) else {
                return;
            };
            let writes = std::cell::RefCell::new(Some(writes));
            let report = std::cell::RefCell::new(Some(report));
            let already_failed = failed.clone();
            let from_for_report = from.clone();
            let moved = RcBlock::new(move |error: *mut NSError| {
                guarded((), || {
                    // SAFETY: The completion handler's error is nil or a valid NSError for the duration of the call.
                    let error = unsafe { error.as_ref() };
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

fn reveal_place(editor: &super::EditorController, (line, column): (usize, usize)) {
    let at = editor.on_source_text(move |text| place_in_text(text, line, column));
    editor.reveal_source(crate::markdown::text::Byte::new(at));
}

/// A line and a place in it as a position of `text`, as near as the text comes: it may have changed since the search.
fn place_in_text(text: &str, line: usize, column: usize) -> usize {
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut seen = 0;
    while seen < line {
        let Some(found) = bytes[start..].iter().position(|&b| b == b'\n' || b == b'\r') else {
            return text.len();
        };
        let at = start + found;
        start = at
            + if bytes[at] == b'\r' && bytes.get(at + 1) == Some(&b'\n') {
                2
            } else {
                1
            };
        seen += 1;
    }
    let line_end = bytes[start..]
        .iter()
        .position(|&b| b == b'\n' || b == b'\r')
        .map_or(text.len(), |found| start + found);
    text.floor_char_boundary((start + column).min(line_end))
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

/// Changes to a text as one edit: from the first replacement to the last, what lies between stays as it was.
pub(super) fn single_edit(text: &str, replacements: &[Replacement]) -> Option<crate::markdown::edit::Edit> {
    use crate::markdown::text::Byte;
    let start = replacements.iter().map(|r| r.range.start).min()?;
    let end = replacements.iter().map(|r| r.range.end).max()?;
    let mut sorted: Vec<&Replacement> = replacements.iter().collect();
    sorted.sort_by_key(|r| r.range.start);
    let mut replacement = String::new();
    let mut at = start;
    for r in sorted {
        replacement.push_str(text.get(at..r.range.start)?);
        replacement.push_str(&r.text);
        at = r.range.end;
    }
    replacement.push_str(text.get(at..end)?);
    let selection = Byte::new(start + replacement.len());
    Some(crate::markdown::edit::Edit {
        range: Byte::new(start)..Byte::new(end),
        replacement,
        selection: selection..selection,
    })
}

#[cfg(feature = "selftest")]
impl Notes<'_> {
    pub(super) fn hit_paths(&self) -> Vec<PathBuf> {
        self.state.hits.borrow().iter().map(|f| f.path.clone()).collect()
    }

    pub(super) fn tree_paths(&self) -> Vec<PathBuf> {
        let tree = self.state.tree.borrow();
        tree.iter()
            .flat_map(|t| t.nodes.iter().filter(|n| !n.folder).map(|n| n.path.clone()))
            .collect()
    }

    pub(super) fn is_searching(&self) -> bool {
        self.state.searching.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use splat_notes::{Hit, Node};

    #[test]
    fn a_hash_sign_in_front_of_one_word_looks_for_a_tag() {
        assert_eq!(query_of("#idee"), Some(Query::Tag("#idee".to_string())));
        assert_eq!(
            query_of("#idee und mehr"),
            Some(Query::Text {
                needle: "#idee und mehr".to_string(),
                case_sensitive: false
            })
        );
        assert_eq!(
            query_of("#"),
            Some(Query::Text {
                needle: "#".to_string(),
                case_sensitive: false
            })
        );
        assert_eq!(query_of("  "), None);
    }

    #[test]
    fn a_new_name_keeps_the_extension_and_stays_in_the_folder() {
        let path = Path::new("/a/Alt.md");
        assert_eq!(renamed_path(path, "Neu"), Some(PathBuf::from("/a/Neu.md")));
        assert_eq!(renamed_path(path, " Neu "), Some(PathBuf::from("/a/Neu.md")));
        assert_eq!(
            renamed_path(path, "Neu.markdown"),
            Some(PathBuf::from("/a/Neu.markdown"))
        );
        assert_eq!(renamed_path(path, "x/Neu"), None);
        assert_eq!(renamed_path(path, "a:b"), None);
        assert_eq!(renamed_path(path, ".versteckt"), None);
        assert_eq!(renamed_path(path, ""), None);
        assert_eq!(renamed_path(path, "Alt"), None, "nothing to do");
    }

    #[test]
    fn a_tree_becomes_rows_with_the_same_shape() {
        let tree = Tree {
            root: PathBuf::from("/r"),
            nodes: vec![
                Node {
                    name: "d".to_string(),
                    path: PathBuf::from("/r/d"),
                    folder: true,
                    children: vec![1],
                },
                Node {
                    name: "n.md".to_string(),
                    path: PathBuf::from("/r/d/n.md"),
                    folder: false,
                    children: vec![],
                },
            ],
            top: vec![0],
            cut: false,
            unreadable: 0,
        };
        let (rows, roots) = rows_of_tree(&tree);
        assert_eq!(roots, [0]);
        assert_eq!(rows[0].kind, RowKind::Folder);
        assert_eq!(rows[0].children, [1]);
        assert_eq!(rows[1].kind, RowKind::Note(PathBuf::from("/r/d/n.md")));
    }

    #[test]
    fn hits_stand_below_their_file_by_the_path_inside_the_folder() {
        let hits = [FileHits {
            path: PathBuf::from("/r/d/n.md"),
            hits: vec![Hit {
                line: 2,
                column: 4,
                range: 40..43,
                context: "ein Wort".to_string(),
                in_context: 4..8,
            }],
            more: true,
        }];
        let (rows, roots) = rows_of_hits(&hits, Some(Path::new("/r")));
        assert_eq!(roots, [0]);
        assert_eq!(rows[0].title, "d/n.md");
        assert_eq!(rows[0].children, [1, 2]);
        assert_eq!(rows[1].kind, RowKind::Hit(PathBuf::from("/r/d/n.md"), 2, 4));
        assert_eq!(rows[1].matched, Some(4..8));
        assert_eq!(rows[2].kind, RowKind::Remark);
    }

    #[test]
    fn a_long_line_starts_shortly_before_its_match() {
        assert_eq!(near_the_match("kurz Wort", 5..9), ("kurz Wort".to_string(), 5..9));
        let line = "Ein ganz langer Satz vor dem äpfel und mehr";
        let at = line.find("äpfel").unwrap();
        let (shown, matched) = near_the_match(line, at..at + "äpfel".len());
        assert_eq!(&shown[matched], "äpfel");
        assert!(shown.starts_with('…'), "{shown}");
        assert!(shown.len() < line.len());
        assert_eq!(near_the_match("äöü", 0..2), ("äöü".to_string(), 0..2));
    }

    #[test]
    fn the_status_says_what_a_search_came_to() {
        let none: [FileHits; 0] = [];
        assert_ne!(search_status(&none, true), search_status(&none, false));
        let some = [FileHits {
            path: PathBuf::from("/a.md"),
            hits: vec![Hit {
                line: 0,
                column: 0,
                range: 0..1,
                context: "a".to_string(),
                in_context: 0..1,
            }],
            more: false,
        }];
        let status = search_status(&some, true);
        assert!(status.contains('1'), "{status}");
    }

    #[test]
    fn several_replacements_make_one_edit_and_leave_what_lies_between() {
        let text = "ein [[Alt]] und [[Alt|x]] Ende";
        let at = |needle: usize| needle..needle + 3;
        let edit = single_edit(
            text,
            &[
                Replacement {
                    range: at(18),
                    text: "Neu".to_string(),
                },
                Replacement {
                    range: at(6),
                    text: "Neu".to_string(),
                },
            ],
        )
        .unwrap();
        assert_eq!(edit.applied_to(text), "ein [[Neu]] und [[Neu|x]] Ende");
        assert_eq!(edit.range.start.get(), 6);
        assert_eq!(edit.range.end.get(), 21);
        assert_eq!(single_edit(text, &[]), None);
    }

    #[test]
    fn a_hit_is_found_again_by_its_line_whatever_ends_the_lines() {
        assert_eq!(place_in_text("ab\ncd\nef", 1, 1), 4);
        assert_eq!(place_in_text("ab\r\ncd\r\nef", 2, 1), 9);
        assert_eq!(place_in_text("ab\rcd", 1, 0), 3);
        assert_eq!(place_in_text("ab\ncd", 0, 99), 2, "not beyond its line");
        assert_eq!(place_in_text("ab", 5, 0), 2, "the text has lost lines since");
        assert_eq!(place_in_text("äb", 0, 1), 0, "on a letter");
    }

    #[test]
    fn links_change_in_all_files_or_in_none() {
        let dir = std::env::temp_dir().join(format!("splat-rewrites-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a.md"), dir.join("b.md"));
        std::fs::write(&a, "alt a").unwrap();
        std::fs::write(&b, "alt b").unwrap();
        let writes = vec![
            (a.clone(), "alt a".to_string(), "neu a".to_string()),
            (b.clone(), "alt b".to_string(), "neu b".to_string()),
        ];
        assert_eq!(write_rewrites(&writes), Vec::<PathBuf>::new());
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "neu a");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "neu b");

        std::fs::write(&b, "anders").unwrap();
        let writes = vec![
            (a.clone(), "neu a".to_string(), "x".to_string()),
            (b.clone(), "neu b".to_string(), "y".to_string()),
        ];
        assert_eq!(write_rewrites(&writes).len(), 2, "another program changed one of them");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "neu a", "so the other stays too");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "anders");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2, "nothing lies around");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_folder_counts_as_the_same_file_only_with_itself() {
        let dir = std::env::temp_dir();
        assert!(same_file(&dir, &dir));
        assert!(!same_file(&dir, Path::new("/gibt-es-nicht")));
    }
}
