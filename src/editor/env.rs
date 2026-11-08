//! What the preview needs from around the text: the user's choice of typographic punctuation, the code host of the
//! repository the document lies in, its bibliography and the notes it embeds.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use objc2_foundation::{NSBundle, NSLocale, NSUserDefaults};

use splat_editor_state::bibtex::parse_bibtex;
use splat_editor_state::repository::find_repository;
pub use splat_editor_state::repository::{Repository, repository_root};

use crate::markdown::typography::Quotes;
use crate::util::{contained, ns, read_regular_file};

/// User defaults key of the typographic punctuation switch in the View menu.
pub const TYPOGRAPHY_KEY: &str = "TypographicPunctuation";
/// Embedded notes larger than this show as links.
const MAX_NOTE_BYTES: u64 = 512 * 1024;
/// How long a note found by name is not searched for again.
const FOUND_FOR: Duration = Duration::from_secs(5);

/// Everything the preview of one document reads from outside its text, gathered on the main thread.
#[derive(Clone, Default)]
pub struct Env {
    pub quotes: Option<Quotes>,
    pub repository: Option<Repository>,
    pub citations: Option<Arc<HashMap<String, String>>>,
    /// The bibliography lies with the document's own files, so a file that leaves the Mac may show its entries.
    pub citations_travel: bool,
    /// The sandbox keeps the document's folder closed until the user grants it.
    pub folder_closed: bool,
    /// A repository lies around the folder, whose marks for what changed need the folder granted.
    pub repository_around: bool,
    pub doc_dir: Option<PathBuf>,
}

impl Env {
    /// Reads the disk right now, for an export the user is waiting for.
    pub fn now(doc_dir: Option<PathBuf>, text: &str) -> Env {
        let found = doc_dir
            .as_deref()
            .and_then(|d| find_citations(d, named_bibliography(d, text)));
        Env {
            quotes: typography_on().then(quotes),
            repository: doc_dir.as_deref().and_then(find_repository),
            citations_travel: found.as_ref().is_some_and(|(_, travels)| *travels),
            citations: found.map(|(entries, _)| entries),
            folder_closed: doc_dir.as_deref().is_some_and(|d| !crate::grants::reaches(d)),
            repository_around: doc_dir.as_deref().is_some_and(|d| repository_root(d).is_some()),
            doc_dir,
        }
    }

    /// What a file that leaves the Mac may show: a document from someone else must not name a bibliography of the user's
    /// to have its entries printed into what gets sent back.
    pub fn sendable(mut self) -> Env {
        if !self.citations_travel {
            self.citations = None;
        }
        self
    }

    /// What the disk thread last found, for the preview, which asks after every edit.
    pub fn known(doc_dir: Option<PathBuf>, text: &str) -> Env {
        let around = doc_dir
            .as_deref()
            .map(|d| Question::Around(d.to_path_buf(), named_bibliography(d, text)))
            .and_then(look_up);
        let (repository, found, folder_closed, repository_around) = match around {
            Some(Answer::Around(repository, found, closed, around)) => (repository, found, closed, around),
            _ => (None, None, false, false),
        };
        Env {
            quotes: typography_on().then(quotes),
            repository,
            citations_travel: found.as_ref().is_some_and(|(_, travels)| *travels),
            citations: found.map(|(entries, _)| entries),
            folder_closed,
            repository_around,
            doc_dir,
        }
    }
}

/// How long the first question about something waits for the disk thread; a local disk answers in time, so a document
/// does not lay out twice when it opens.
const FIRST_WAIT: Duration = Duration::from_millis(50);
/// After a wait in vain nothing waits for this long: a document can embed thousands of notes.
const NO_WAIT_FOR: Duration = Duration::from_secs(5);
/// How old an answer may be before the next question sends the disk thread to look again.
const LOOK_AGAIN_AFTER: Duration = if cfg!(test) {
    Duration::from_millis(20)
} else {
    Duration::from_secs(3)
};
/// Answers nobody asked for this long are dropped with the next new question.
const FORGET_AFTER: Duration = Duration::from_secs(60);

#[derive(Clone, PartialEq, Eq, Hash)]
enum Question {
    /// The repository a folder lies in and the bibliography beside it, or the one its document names.
    Around(PathBuf, Option<PathBuf>),
    /// The note an embed in a folder names.
    Note(PathBuf, String),
}

#[derive(Clone, PartialEq)]
enum Answer {
    /// The repository, the bibliography and whether it may travel, whether the sandbox keeps the folder closed and whether a
    /// repository lies around it.
    Around(Option<Repository>, Option<(Citations, bool)>, bool, bool),
    Note(Option<(String, String)>),
}

impl Question {
    fn answer_before_any_look(&self) -> Answer {
        match self {
            Question::Around(..) => Answer::Around(None, None, false, false),
            Question::Note(..) => Answer::Note(None),
        }
    }

    fn folder(&self) -> &Path {
        match self {
            Question::Around(dir, _) | Question::Note(dir, _) => dir,
        }
    }

    fn look(&self) -> Answer {
        match self {
            Question::Around(dir, named) => Answer::Around(
                find_repository(dir),
                find_citations(dir, named.clone()),
                !crate::grants::reaches(dir),
                repository_root(dir).is_some(),
            ),
            Question::Note(dir, target) => Answer::Note(embedded_note(dir, target)),
        }
    }
}

struct Known {
    answer: Answer,
    at: Instant,
    looking: bool,
    /// The first asker still waits, and sees the answer without being told.
    waiting: bool,
}

#[derive(Default)]
struct Answers {
    known: HashMap<Question, Known>,
    no_wait_until: Option<Instant>,
}

static ANSWERS: Mutex<Option<Answers>> = Mutex::new(None);

fn with_answers<T>(f: impl FnOnce(&mut Answers) -> T) -> Option<T> {
    let mut answers = ANSWERS.lock().ok()?;
    Some(f(answers.get_or_insert_with(Answers::default)))
}

fn look_up(question: Question) -> Option<Answer> {
    enum Next {
        Known(Answer),
        Wait(std::sync::mpsc::Receiver<()>),
    }
    let next = with_answers(|answers| {
        if let Some(known) = answers.known.get_mut(&question) {
            if !known.looking && known.at.elapsed() >= LOOK_AGAIN_AFTER {
                known.looking = true;
                send_to_look(question.clone(), None);
            }
            return Next::Known(known.answer.clone());
        }
        answers
            .known
            .retain(|_, known| known.looking || known.at.elapsed() < FORGET_AFTER);
        let waits = answers.no_wait_until.is_none_or(|until| Instant::now() >= until);
        let answer = question.answer_before_any_look();
        answers.known.insert(
            question.clone(),
            Known {
                answer: answer.clone(),
                at: Instant::now(),
                looking: true,
                waiting: waits,
            },
        );
        let (told, wait) = channel();
        send_to_look(question.clone(), waits.then_some(told));
        if waits { Next::Wait(wait) } else { Next::Known(answer) }
    })?;
    let wait = match next {
        Next::Known(answer) => return Some(answer),
        Next::Wait(wait) => wait,
    };
    let in_time = wait.recv_timeout(FIRST_WAIT).is_ok();
    with_answers(|answers| {
        if !in_time {
            answers.no_wait_until = Some(Instant::now() + NO_WAIT_FOR);
        }
        let known = answers.known.get_mut(&question)?;
        known.waiting = false;
        Some(known.answer.clone())
    })?
}

/// Forgets every answer, as after the user granted a folder: what the sandbox kept closed may be read now.
pub fn forget_known() {
    with_answers(|answers| answers.known.clear());
}

fn send_to_look(question: Question, told: Option<Sender<()>>) {
    crate::util::lane_of(question.folder()).run(move || {
        let answer = contained(|| question.look());
        let changed = with_answers(|answers| {
            let known = answers.known.get_mut(&question)?;
            let seen = std::mem::take(&mut known.waiting);
            known.at = Instant::now();
            known.looking = false;
            let answer = answer?;
            let changed = known.answer != answer;
            known.answer = answer;
            Some(changed && !seen)
        })
        .flatten();
        if let Some(told) = told {
            let _ = told.send(());
        }
        if changed == Some(true) {
            crate::util::main_soon(super::relayout_all);
        }
    });
}

/// The note `![[target]]` names, as the disk thread last found it.
pub fn known_note(dir: &Path, target: &str) -> Option<(String, String)> {
    match look_up(Question::Note(dir.to_path_buf(), target.to_string()))? {
        Answer::Note(note) => note,
        Answer::Around(..) => None,
    }
}

pub fn typography_on() -> bool {
    NSUserDefaults::standardUserDefaults().boolForKey(&ns(TYPOGRAPHY_KEY))
}

pub fn set_typography(on: bool) {
    NSUserDefaults::standardUserDefaults().setBool_forKey(on, &ns(TYPOGRAPHY_KEY));
}

/// Quotation marks of the interface language, from the system's locale data.
pub fn quotes() -> Quotes {
    let language = NSBundle::mainBundle()
        .preferredLocalizations()
        .firstObject()
        .map_or_else(|| "de".to_string(), |l| l.to_string());
    let locale = NSLocale::localeWithLocaleIdentifier(&ns(&language));
    let first = |s: objc2::rc::Retained<objc2_foundation::NSString>, fallback: char| {
        s.to_string().chars().next().unwrap_or(fallback)
    };
    Quotes {
        open: first(locale.quotationBeginDelimiter(), '“'),
        close: first(locale.quotationEndDelimiter(), '”'),
        open_single: first(locale.alternateQuotationBeginDelimiter(), '‘'),
        close_single: first(locale.alternateQuotationEndDelimiter(), '’'),
    }
}

/// Bibliographies and embedded notes kept read, the oldest going first: reading one again costs far less than keeping
/// everything an app that stays open for weeks ever showed.
const KEPT_FILES: usize = 64;

type Citations = Arc<HashMap<String, String>>;
static BIBLIOGRAPHIES: FileCache<Citations> = FileCache::new();

/// A bibliography larger than this holds no citations a document needs; `bibliography:` can name any file.
const MAX_BIBLIOGRAPHY_BYTES: u64 = 16 << 20;

/// What was made of the last few files, as long as their date stays. Its lock is never held while a file is read, since a
/// volume that stopped answering would hold up every other volume behind it.
struct FileCache<T>(Mutex<Vec<(PathBuf, SystemTime, T)>>);

impl<T: Clone> FileCache<T> {
    const fn new() -> FileCache<T> {
        FileCache(Mutex::new(Vec::new()))
    }

    /// What `path` holds at the date `modified`, by `read` if that is not known yet.
    fn get(&self, path: &Path, modified: SystemTime, read: impl FnOnce() -> Option<T>) -> Option<T> {
        {
            let entries = self.0.lock().ok()?;
            if let Some((_, _, value)) = entries.iter().find(|(p, m, _)| p == path && *m == modified) {
                return Some(value.clone());
            }
        }
        let value = read()?;
        let mut entries = self.0.lock().ok()?;
        entries.retain(|(p, _, _)| p != path);
        if entries.len() >= KEPT_FILES {
            entries.remove(0);
        }
        entries.push((path.to_path_buf(), modified, value.clone()));
        Some(value)
    }
}

#[cfg(test)]
fn citations(dir: &Path, text: &str) -> Option<Citations> {
    find_citations(dir, named_bibliography(dir, text)).map(|(entries, _)| entries)
}

/// The file the front matter of `text` names as `bibliography:`.
fn named_bibliography(dir: &Path, text: &str) -> Option<PathBuf> {
    text.strip_prefix("---\n")
        .and_then(|rest| rest.split("\n---").next())
        .and_then(|front| {
            front.lines().find_map(|l| {
                let value = l.strip_prefix("bibliography:")?.trim().trim_matches(['"', '\'']);
                (!value.is_empty()).then(|| dir.join(value))
            })
        })
}

/// How each entry of the document's BibTeX bibliography reads in a citation, such as `Doe 2020`, and whether the file may
/// travel with the document as its images may. The file is the one front matter names as `bibliography:`, else
/// `references.bib`, `literatur.bib` or `bibliography.bib` beside it.
fn find_citations(dir: &Path, named: Option<PathBuf>) -> Option<(Citations, bool)> {
    let path = named
        .into_iter()
        .chain(["references.bib", "literatur.bib", "bibliography.bib"].map(|n| dir.join(n)))
        .find(|p| p.is_file())?;
    let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok()?;
    let entries = BIBLIOGRAPHIES.get(&path, modified, || {
        let bytes = read_regular_file(&path, MAX_BIBLIOGRAPHY_BYTES)?;
        Some(Arc::new(parse_bibtex(&String::from_utf8(bytes).ok()?)))
    })?;
    Some((entries, may_carry_along(&path, Some(dir))))
}

/// Whether what leaves the Mac as a file may take this one along: a document from someone else must not pull what it names
/// from the user's folders into a file that gets sent on. Its own folder and its repository are what a document brings along.
pub(super) fn may_carry_along(path: &Path, doc_dir: Option<&Path>) -> bool {
    let Some(doc_dir) = doc_dir else {
        return false;
    };
    let home = crate::markdown::url::home();
    let root = what_a_document_brings_along(doc_dir, home.as_deref());
    match (path.canonicalize(), root.canonicalize()) {
        (Ok(file), Ok(root)) => file.starts_with(root),
        _ => false,
    }
}

/// A repository that holds the home folder, as one for dotfiles does, says nothing about what belongs to the document.
fn what_a_document_brings_along<'a>(doc_dir: &'a Path, home: Option<&Path>) -> &'a Path {
    repository_root(doc_dir)
        .filter(|root| !home.is_some_and(|home| home.starts_with(root)))
        .unwrap_or(doc_dir)
}

type Found = (PathBuf, String, Instant, Option<PathBuf>);
static FOUND: Mutex<Vec<Found>> = Mutex::new(Vec::new());
static NOTES: FileCache<Arc<str>> = FileCache::new();

/// Files an embed shows as note; any other file, such as a key or `.env`, stays out of the preview and its exports.
const NOTE_EXTENSIONS: [&str; 4] = ["md", "markdown", "mdown", "mkd"];

/// The text of the note `![[target]]` names and the folder it lies in; a note too large to show gives `None`.
pub fn embedded_note(dir: &Path, target: &str) -> Option<(String, String)> {
    let known = {
        let mut cache = FOUND.lock().ok()?;
        cache.retain(|(_, _, at, _)| at.elapsed() < FOUND_FOR);
        cache
            .iter()
            .find(|(d, t, _, _)| d == dir && t == target)
            .map(|(_, _, _, path)| path.clone())
    };
    let found = known.unwrap_or_else(|| {
        let path = super::links::find_note(dir, target);
        if let Ok(mut cache) = FOUND.lock() {
            cache.push((dir.to_path_buf(), target.to_string(), Instant::now(), path.clone()));
        }
        path
    })?;
    // The name of a link says nothing about the file it points at.
    let real = found.canonicalize().ok()?;
    let extension = real.extension()?.to_str()?.to_ascii_lowercase();
    if !NOTE_EXTENSIONS.contains(&extension.as_str()) {
        return None;
    }
    let meta = std::fs::metadata(&real).ok()?;
    if meta.len() > MAX_NOTE_BYTES {
        return None;
    }
    let modified = meta.modified().ok()?;
    let folder = found.parent()?.to_string_lossy().into_owned();
    let text = NOTES.get(&real, modified, || {
        let bytes = read_regular_file(&real, MAX_NOTE_BYTES)?;
        Some(Arc::from(String::from_utf8(bytes).ok()?))
    })?;
    Some((text.to_string(), folder))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_cache_stays_free_for_others_while_a_file_is_read() {
        static CACHE: FileCache<u32> = FileCache::new();
        let date = SystemTime::UNIX_EPOCH;
        let (reading, started) = channel::<()>();
        let (free, finish) = channel::<()>();
        let slow = std::thread::spawn(move || {
            CACHE.get(Path::new("/hangs"), date, || {
                reading.send(()).unwrap();
                finish.recv().unwrap();
                Some(1)
            })
        });
        started.recv().unwrap();
        let quick = std::thread::spawn(move || CACHE.get(Path::new("/answers"), date, || Some(2)));
        assert_eq!(quick.join().unwrap(), Some(2), "held up by the file that hangs");
        free.send(()).unwrap();
        assert_eq!(slow.join().unwrap(), Some(1));
        assert_eq!(CACHE.get(Path::new("/hangs"), date, || None), Some(1), "known by now");
        let later = date + Duration::from_secs(1);
        assert_eq!(
            CACHE.get(Path::new("/hangs"), later, || Some(3)),
            Some(3),
            "a new date reads anew"
        );
    }

    #[test]
    fn a_bibliography_too_large_to_be_one_is_not_read() {
        let dir = std::env::temp_dir().join(format!("splat-bib-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join("references.bib")).unwrap();
        file.set_len(MAX_BIBLIOGRAPHY_BYTES + 1).unwrap();
        assert!(citations(&dir, "").is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bibliographies_and_notes_are_found_beside_the_document() {
        let root = std::env::temp_dir().join(format!("splat-env-{}", std::process::id()));
        let docs = root.join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(docs.join("quellen.bib"), "@book{k, author = {Kant, I.}, year = 1781}").unwrap();
        std::fs::write(docs.join("references.bib"), "@book{r, author = {Roe, R.}, year = 2000}").unwrap();
        let named = citations(&docs, "---\nbibliography: \"quellen.bib\"\n---\n# Text").unwrap();
        assert_eq!(named["k"], "Kant 1781");
        assert_eq!(citations(&docs, "# Ohne Angabe").unwrap()["r"], "Roe 2000");
        assert!(Arc::ptr_eq(
            &named,
            &citations(&docs, "---\nbibliography: quellen.bib\n---\n").unwrap()
        ));

        std::fs::write(docs.join("Notiz.md"), "# Notiz").unwrap();
        std::fs::write(
            docs.join("Groß.md"),
            "x".repeat(usize::try_from(MAX_NOTE_BYTES).unwrap_or(usize::MAX) + 1),
        )
        .unwrap();
        let (text, folder) = embedded_note(&docs, "Notiz").unwrap();
        assert_eq!((text.as_str(), folder.as_str()), ("# Notiz", docs.to_str().unwrap()));
        assert_eq!(embedded_note(&docs, "Notiz").unwrap().0, "# Notiz");
        assert!(embedded_note(&docs, "Groß").is_none());
        assert!(embedded_note(&docs, "Fehlt").is_none());
        std::fs::write(docs.join(".env"), "TOKEN=1").unwrap();
        assert!(embedded_note(&docs, ".env").is_none(), "only notes show");
        std::os::unix::fs::symlink(docs.join(".env"), docs.join("Umgebung.md")).unwrap();
        assert!(
            embedded_note(&docs, "Umgebung").is_none(),
            "a note's name on a link to another file"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_file_leaves_with_a_document_only_from_where_the_document_lies() {
        let root = std::env::temp_dir().join(format!("splat-carry-{}", std::process::id()));
        let repo = root.join("repo");
        let docs = repo.join("docs");
        let plain = root.join("plain");
        for dir in [&repo.join(".git"), &docs, &plain] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let file = |at: &Path| {
            std::fs::write(at, b"x").unwrap();
            at.to_path_buf()
        };
        let (beside, in_repo, elsewhere) = (
            file(&docs.join("a.png")),
            file(&repo.join("b.png")),
            file(&root.join("c.png")),
        );
        let in_plain = file(&plain.join("d.png"));
        let through = plain.join("durch.png");
        std::os::unix::fs::symlink(&elsewhere, &through).unwrap();
        assert!(may_carry_along(&beside, Some(&docs)));
        assert!(
            may_carry_along(&in_repo, Some(&docs)),
            "a repository keeps its pictures anywhere inside"
        );
        assert!(!may_carry_along(&elsewhere, Some(&docs)));
        assert!(may_carry_along(&in_plain, Some(&plain)));
        assert!(!may_carry_along(&elsewhere, Some(&plain)));
        assert!(!may_carry_along(&plain.join("../c.png"), Some(&plain)), "up and out");
        assert!(!may_carry_along(&through, Some(&plain)), "a link that points out");
        assert!(!may_carry_along(&beside, None), "a document without a place");
        assert!(!may_carry_along(&docs.join("missing.png"), Some(&docs)));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_repository_around_the_home_folder_opens_nothing() {
        let root = std::env::temp_dir().join(format!("splat-carry-home-{}", std::process::id()));
        let (home, docs) = (root.join("home"), root.join("home/docs"));
        std::fs::create_dir_all(home.join(".git")).unwrap();
        std::fs::create_dir_all(&docs).unwrap();
        assert_eq!(
            what_a_document_brings_along(&docs, Some(&home)),
            docs,
            "a repository of dotfiles"
        );
        assert_eq!(what_a_document_brings_along(&docs, Some(&root.join("other"))), home);
        assert_eq!(what_a_document_brings_along(&docs, None), home);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_bibliography_from_elsewhere_stays_out_of_what_is_sent() {
        let root = std::env::temp_dir().join(format!("splat-bib-elsewhere-{}", std::process::id()));
        let docs = root.join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(root.join("privat.bib"), "@book{p, author = {Poe, E.}, year = 1845}").unwrap();
        std::fs::write(docs.join("references.bib"), "@book{r, author = {Roe, R.}, year = 2000}").unwrap();
        let named = Env::now(Some(docs.clone()), "---\nbibliography: ../privat.bib\n---\n");
        assert_eq!(
            named.citations.as_ref().map(|c| c["p"].clone()).as_deref(),
            Some("Poe 1845")
        );
        assert!(named.sendable().citations.is_none());
        let beside = Env::now(Some(docs.clone()), "# Text").sendable();
        assert_eq!(beside.citations.map(|c| c["r"].clone()).as_deref(), Some("Roe 2000"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_preview_asks_the_disk_thread_and_follows_what_changes() {
        let dir = std::env::temp_dir().join(format!("splat-known-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("references.bib"), "@book{a, author = {Ada, A.}, year = 1843}").unwrap();
        std::fs::write(dir.join("Notiz.md"), "# Eins").unwrap();
        let until = |found: &dyn Fn() -> bool| {
            let started = Instant::now();
            while !found() && started.elapsed() < Duration::from_secs(10) {
                std::thread::sleep(Duration::from_millis(10));
            }
            found()
        };
        let cited = |key: &str| {
            Env::known(Some(dir.clone()), "# Text")
                .citations
                .is_some_and(|c| c.contains_key(key))
        };
        assert!(until(&|| cited("a")));
        assert!(until(&|| known_note(&dir, "Notiz").is_some_and(|n| n.0 == "# Eins")));
        assert_eq!(known_note(&dir, "Fehlt"), None);
        // Another modification time for the same second: file systems without fine dates would hide the change.
        std::thread::sleep(Duration::from_millis(5));
        std::fs::write(dir.join("references.bib"), "@book{b, author = {Bo, B.}, year = 1900}").unwrap();
        std::fs::write(dir.join("Notiz.md"), "# Zwei, länger").unwrap();
        assert!(until(&|| cited("b")));
        assert!(until(
            &|| known_note(&dir, "Notiz").is_some_and(|n| n.0 == "# Zwei, länger")
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn typographic_quotes_follow_the_setting_and_the_language() {
        let before = typography_on();
        set_typography(!before);
        assert_eq!(typography_on(), !before);
        set_typography(before);
    }
}
